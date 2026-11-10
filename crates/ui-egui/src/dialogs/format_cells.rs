use egui::{Sense, Stroke, StrokeKind, vec2};
use gridcraft_engine::model::{Color as CellColor, Fill, GradientFill, GradientKind, GradientStop, PatternType, Theme};

use super::show::ok_cancel;
use super::{Dialog, SheetApp, json, theme};

fn gradient_color_button(ui: &mut egui::Ui, color: &mut CellColor, theme: &Theme, name: &str) {
    let mut rgb = color.resolve(theme).unwrap_or([0, 0, 0]);
    let resp = ui.color_edit_button_srgb(&mut rgb);
    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(name.to_string()));
    if resp.changed() {
        *color = CellColor::rgb(rgb[0], rgb[1], rgb[2]);
    }
}

fn fill_tab(ui: &mut egui::Ui, fill: &mut Fill, theme: &Theme, lang: crate::i18n::Language) {
    let was = if fill.is_gradient() {
        2
    } else if fill.pattern != PatternType::None {
        1
    } else {
        0
    };
    let mut mode = was;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut mode, 0, lang.tr("No Fill"));
        ui.selectable_value(&mut mode, 1, lang.tr("Solid"));
        ui.selectable_value(&mut mode, 2, lang.tr("Gradient"));
    });
    if mode != was {
        match mode {
            0 => *fill = Fill::default(),
            1 => {
                let c = fill
                    .gradient
                    .as_ref()
                    .and_then(|g| g.stops.first())
                    .map(|s| s.color)
                    // An automatic stop leaves the solid fill invisible: fall back to the
                    // gradient mode's start colour (Accent 1) instead.
                    .filter(|c| *c != CellColor::Auto)
                    .unwrap_or_else(|| if fill.fg == CellColor::Auto { CellColor::Theme(4, 0) } else { fill.fg });
                *fill = Fill::solid(c);
            }
            _ => {
                let start = if fill.pattern == PatternType::Solid && fill.fg != CellColor::Auto { fill.fg } else { CellColor::Theme(4, 0) };
                *fill = Fill::gradient(
                    GradientKind::Linear { degree: 90.0 },
                    vec![GradientStop { position: 0.0, color: start }, GradientStop { position: 1.0, color: CellColor::Theme(0, 0) }],
                );
            }
        }
    }
    ui.add_space(4.0);
    match mode {
        0 => {
            ui.weak(lang.tr("No fill"));
        }
        1 => {
            ui.label(lang.tr("Background Color:"));
            if let Some(c) = crate::widgets::color_palette(ui, &theme.colors, lang.tr("No Color")) {
                *fill = if c == "none" { Fill::default() } else { Fill::solid(CellColor::from_hex(&c).unwrap_or_default()) };
            }
        }
        _ => {
            let Some(mut g) = fill.gradient.clone() else { return };
            let mut linear = g.is_linear();
            ui.horizontal(|ui| {
                ui.radio_value(&mut linear, true, lang.tr("Linear"));
                ui.radio_value(&mut linear, false, lang.tr("Path"));
            });
            if linear != g.is_linear() {
                g.kind =
                    if linear { GradientKind::Linear { degree: 90.0 } } else { GradientKind::Path { left: 0.5, right: 0.5, top: 0.5, bottom: 0.5 } };
            }
            match &mut g.kind {
                GradientKind::Linear { degree } => {
                    ui.add(egui::Slider::new(degree, 0.0..=360.0).text(lang.tr("Angle")));
                    ui.horizontal(|ui| {
                        for (label, value) in [("Vertical", 90.0), ("Diagonal up", 315.0), ("Horizontal", 0.0), ("Diagonal down", 45.0)] {
                            if ui.small_button(lang.tr(label)).clicked() {
                                *degree = value;
                            }
                        }
                    });
                }
                GradientKind::Path { left, right, top, bottom } => {
                    ui.horizontal(|ui| {
                        if ui.small_button(lang.tr("From center")).clicked() {
                            *left = 0.5;
                            *right = 0.5;
                            *top = 0.5;
                            *bottom = 0.5;
                        }
                        if ui.small_button(lang.tr("From corner")).clicked() {
                            *left = 0.0;
                            *right = 0.0;
                            *top = 0.0;
                            *bottom = 0.0;
                        }
                    });
                    ui.add(egui::Slider::new(left, 0.0..=1.0).text(lang.tr("Left")));
                    ui.add(egui::Slider::new(right, 0.0..=1.0).text(lang.tr("Right")));
                    ui.add(egui::Slider::new(top, 0.0..=1.0).text(lang.tr("Top")));
                    ui.add(egui::Slider::new(bottom, 0.0..=1.0).text(lang.tr("Bottom")));
                }
            }
            ui.separator();
            let count = g.stops.len();
            let mut remove = None;
            let mut editing = false;
            for i in 0..count {
                ui.horizontal(|ui| {
                    // Each control names the stop it edits: "×" alone says nothing in a row of stops.
                    let stop_name = format!("{}{}", lang.tr("Gradient|Stop"), i + 1);
                    ui.label(stop_name.as_str());
                    gradient_color_button(ui, &mut g.stops[i].color, theme, &format!("{} {}", stop_name, lang.tr("Fill Color")));
                    let mut pct = g.stops[i].position * 100.0;
                    let resp = ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix("%"));
                    ui.ctx().accesskit_node_builder(resp.id, |node| node.set_label(stop_name.clone()));
                    if resp.changed() {
                        g.stops[i].position = pct / 100.0;
                    }
                    // Keyboard changes set `changed` but not `dragged`; keep the row bound to
                    // the same stop while its slider still owns focus as well.
                    editing |= resp.dragged() || resp.has_focus();
                    if count > 2 {
                        let remove_stop = ui.small_button("×");
                        ui.ctx().accesskit_node_builder(remove_stop.id, |node| node.set_label(format!("{} {stop_name}", lang.tr("Delete"))));
                        if remove_stop.clicked() {
                            remove = Some(i);
                        }
                    }
                });
            }
            if let Some(i) = remove {
                g.stops.remove(i);
            }
            if ui.small_button(lang.tr("Add Stop")).clicked() {
                let color = g.stops.last().map(|s| s.color).unwrap_or(CellColor::Auto);
                g.stops.push(GradientStop { position: 0.5, color });
            }
            // Sorting while a stop is being dragged or keyboard-edited would move a different
            // stop under that slider. Keep the row order until the edit ends; only the preview
            // is sorted.
            let preview = GradientFill::new(g.kind, g.stops.clone());
            let (rect, _) = ui.allocate_exact_size(vec2(240.0, 54.0), Sense::hover());
            crate::gradient::paint_rect(ui.painter(), rect, &preview, theme, false);
            let t = crate::theme::Tokens::get(ui.ctx());
            ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
            *fill = if editing {
                Fill { pattern: PatternType::None, fg: CellColor::Auto, bg: CellColor::Auto, gradient: Some(g) }
            } else {
                Fill::gradient(preview.kind, preview.stops)
            };
        }
    }
}

/// Draws the six-tab Format Cells editor (number, alignment, font, border, fill, protection); the Number tab shows a sample preview, and the style draft updates.
pub(super) fn format_cells(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    // `d.tab` stays the English name (it is what `open_dialog` and tests pass); only the drawn
    // labels are translated. The Fill tab is keyed by context: the Alignment tab's "Fill" is a
    // different word in some languages.
    let lang = app.ui.language;
    ui.horizontal(|ui| {
        for (tab, key) in [
            ("Number", "Number"),
            ("Alignment", "Alignment"),
            ("Font", "Font"),
            ("Border", "Border"),
            ("Fill", "Format Cells|Fill"),
            ("Protection", "Protection"),
        ] {
            if ui.selectable_label(d.tab == tab, lang.tr(key)).clicked() {
                d.tab = tab.to_string();
            }
        }
    });
    ui.separator();
    let mut st: gridcraft_engine::model::Style = d.values.get("style").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let sample = app.session.active().and_then(|doc| doc.wb.active().map(|s| s.value(doc.selection.active))).unwrap_or_default();
    match d.tab.as_str() {
        "Number" => {
            let mut code = st.num_fmt.as_str().to_string();
            ui.columns(2, |cols| {
                cols[0].label(egui::RichText::new(lang.tr("Category:")).strong());
                for name in
                    ["General", "Number", "Currency", "Accounting", "Short Date", "Long Date", "Time", "Percentage", "Fraction", "Scientific", "Text"]
                {
                    let c = gridcraft_engine::cmd::format::format_code_for(name).to_string();
                    if cols[0].selectable_label(code == c, lang.tr(name)).clicked() {
                        code = c;
                    }
                }
                cols[1].label(egui::RichText::new(lang.tr("Sample")).strong());
                let preview = app.session.active().map(|doc| gridcraft_engine::display::format(&sample, &code, &doc.wb).text).unwrap_or_default();
                cols[1].label(egui::RichText::new(preview).font(theme::ui_font(15.0)));
                cols[1].add_space(8.0);
                let label = cols[1].label(lang.tr("Type (custom format code):"));
                cols[1].text_edit_singleline(&mut code).labelled_by(label.id);
                for c in [
                    "0",
                    "0.00",
                    "#,##0",
                    "#,##0.00",
                    "#,##0;[Red]-#,##0",
                    "0%",
                    "0.00%",
                    "0.00E+00",
                    "# ?/?",
                    "m/d/yyyy",
                    "d-mmm-yy",
                    "mmmm d, yyyy",
                    "h:mm AM/PM",
                    "h:mm:ss",
                    "[h]:mm:ss",
                    "@",
                ] {
                    if cols[1].small_button(c).clicked() {
                        code = c.to_string();
                    }
                }
            });
            st.num_fmt = gridcraft_engine::model::NumFmt::new(&code);
        }
        "Alignment" => {
            use gridcraft_engine::model::{HAlign, ReadingOrder, VAlign};
            ui.label(egui::RichText::new(lang.tr("Horizontal")).strong());
            ui.horizontal_wrapped(|ui| {
                for (h, n) in [
                    (HAlign::General, "General"),
                    (HAlign::Left, "Left"),
                    (HAlign::Center, "Center"),
                    (HAlign::Right, "Right"),
                    (HAlign::Fill, "Fill"),
                    (HAlign::Justify, "Justify"),
                    (HAlign::CenterAcross, "Center Across Selection"),
                    (HAlign::Distributed, "Distributed"),
                ] {
                    ui.radio_value(&mut st.align.h, h, lang.tr(n));
                }
            });
            ui.label(egui::RichText::new(lang.tr("Vertical")).strong());
            ui.horizontal_wrapped(|ui| {
                for (v, n) in [
                    (VAlign::Top, "Top"),
                    (VAlign::Center, "Center"),
                    (VAlign::Bottom, "Bottom"),
                    (VAlign::Justify, "Justify"),
                    (VAlign::Distributed, "Distributed"),
                ] {
                    ui.radio_value(&mut st.align.v, v, lang.tr(n));
                }
            });
            ui.label(egui::RichText::new(lang.tr("Reading direction")).strong());
            ui.horizontal_wrapped(|ui| {
                for (order, name) in
                    [(ReadingOrder::Context, "Context"), (ReadingOrder::LeftToRight, "Left to right"), (ReadingOrder::RightToLeft, "Right to left")]
                {
                    let selected = st.align.reading_order.unwrap_or_default() == order;
                    if ui.radio(selected, lang.tr(name)).clicked() {
                        st.align.reading_order = Some(order);
                    }
                }
            });
            let mut indent = st.align.indent as f32;
            ui.add(egui::Slider::new(&mut indent, 0.0..=15.0).text(lang.tr("Indent")));
            st.align.indent = indent as u8;
            let mut rot = if st.align.rotation == 255 { 0.0 } else { st.align.rotation as f32 };
            ui.add(egui::Slider::new(&mut rot, -90.0..=90.0).text(lang.tr("Orientation (degrees)")));
            st.align.rotation = rot as i16;
            ui.checkbox(&mut st.align.wrap, lang.tr("Wrap text"));
            ui.checkbox(&mut st.align.shrink, lang.tr("Shrink to fit"));
        }
        "Font" => {
            craft_ui::ComboBox::from_label(lang.tr("Font")).selected_text(st.font.name.clone()).show_ui(ui, |ui| {
                for f in crate::fonts::families() {
                    ui.selectable_value(&mut st.font.name, f.clone(), f.clone());
                }
            });
            ui.add(egui::Slider::new(&mut st.font.size, 6.0..=72.0).text(lang.tr("Size")));
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.font.bold, lang.tr("Bold"));
                ui.checkbox(&mut st.font.italic, lang.tr("Italic"));
                ui.checkbox(&mut st.font.strike, lang.tr("Strikethrough"));
            });
            use gridcraft_engine::model::{Underline, VertAlign};
            ui.horizontal(|ui| {
                for (u, n) in [
                    (Underline::None, "None"),
                    (Underline::Single, "Single"),
                    (Underline::Double, "Double"),
                    (Underline::SingleAccounting, "Single Accounting"),
                    (Underline::DoubleAccounting, "Double Accounting"),
                ] {
                    ui.radio_value(&mut st.font.underline, u, lang.tr(n));
                }
            });
            ui.horizontal(|ui| {
                ui.radio_value(&mut st.font.vert, VertAlign::Baseline, lang.tr("Normal"));
                ui.radio_value(&mut st.font.vert, VertAlign::Superscript, lang.tr("Superscript"));
                ui.radio_value(&mut st.font.vert, VertAlign::Subscript, lang.tr("Subscript"));
            });
            ui.label(lang.tr("Color:"));
            let colors = app.session.active().map(|doc| doc.wb.theme.colors).unwrap_or_default();
            if let Some(c) = crate::widgets::color_palette(ui, &colors, lang.tr("Automatic")) {
                st.font.color =
                    if c == "none" { gridcraft_engine::model::Color::Auto } else { gridcraft_engine::model::Color::from_hex(&c).unwrap_or_default() };
            }
        }
        "Border" => {
            use gridcraft_engine::model::{BorderLine, BorderStyle};
            let line = BorderLine { style: BorderStyle::Thin, color: gridcraft_engine::model::Color::Auto };
            ui.horizontal(|ui| {
                if ui.button(lang.tr("None")).clicked() {
                    st.borders = Default::default();
                }
                if ui.button(lang.tr("Outline")).clicked() {
                    st.borders.top = line;
                    st.borders.bottom = line;
                    st.borders.left = line;
                    st.borders.right = line;
                }
            });
            for (n, slot) in [
                ("Top", &mut st.borders.top),
                ("Bottom", &mut st.borders.bottom),
                ("Left", &mut st.borders.left),
                ("Right", &mut st.borders.right),
                ("Diagonal ↘", &mut st.borders.diag_down),
                ("Diagonal ↗", &mut st.borders.diag_up),
            ] {
                let n = lang.tr(n);
                ui.horizontal(|ui| {
                    ui.label(format!("{n}:"));
                    for (s, label) in [
                        (BorderStyle::None, "none"),
                        (BorderStyle::Thin, "thin"),
                        (BorderStyle::Medium, "medium"),
                        (BorderStyle::Thick, "thick"),
                        (BorderStyle::Dashed, "dashed"),
                        (BorderStyle::Dotted, "dotted"),
                        (BorderStyle::Double, "double"),
                    ] {
                        let label = lang.tr(label);
                        let response = ui.selectable_label(slot.style == s, label);
                        let selected = slot.style == s;
                        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, format!("{n} {label}")));
                        if response.clicked() {
                            slot.style = s;
                        }
                    }
                });
            }
            let theme = app.session.active().map(|doc| &doc.wb.theme).cloned().unwrap_or_default();
            crate::border_preview::sample(ui, &st.borders, &theme);
        }
        "Fill" => {
            let theme = app.session.active().map(|doc| doc.wb.theme.clone()).unwrap_or_default();
            fill_tab(ui, &mut st.fill, &theme, lang);
        }
        "Protection" => {
            ui.checkbox(&mut st.protection.locked, lang.tr("Locked"));
            ui.checkbox(&mut st.protection.hidden, lang.tr("Hidden"));
            ui.label(
                egui::RichText::new(
                    lang.tr("Locking cells or hiding formulas has no effect until you protect the worksheet (Review › Protect Sheet)."),
                )
                .small(),
            );
        }
        _ => {}
    }
    if let Ok(v) = serde_json::to_value(&st) {
        d.values.insert("style".into(), v);
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm && let Some(style) = d.values.get("style").cloned() {
        app.run_or_alert("home.formatCells", json!({"style": style}));
    }
    if !open {
        app.dialog = None;
        *confirm = true; // closes
    }
}
