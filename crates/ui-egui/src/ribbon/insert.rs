use super::controls::{act, anchor_keytip, control_keytip, invoke, popup_command, sep};
use super::{
    Icon, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, big_button, big_split_button, icons, json, small_button, small_toggle_button, vec2,
};

/// Draws the Insert tab: PivotTable, Table, Pictures, Shapes, Icons, charts, slicer, sparklines, form controls (button, slider, spin button), checkbox, links, comments, text box, header/footer and symbols.
pub(super) fn insert(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::PivotTable, "PivotTable", "PivotTable", false);
    control_keytip(app, ui, &__k, "insert.pivotTable");
    if __k.clicked() {
        act(app, "insert.pivotTable", json!({}));
    }
    let __k = big_button(ui, Icon::Table, "Table", "Table", false);
    control_keytip(app, ui, &__k, "insert.table");
    if __k.clicked() {
        act(app, "insert.table", json!({}));
    }
    sep(ui);
    let pic = big_button(ui, Icon::Picture, "Pictures", "Insert a picture", true);
    anchor_keytip(app, ui, &pic, &["insert.picture"]);
    pic.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), "Pictures"));
    egui::Popup::menu(&pic).show(|ui| {
        for (label, placement) in [("Place in Cell", "cell"), ("Place over Cells", "overCells")] {
            if ui.button(label).clicked() {
                app.open_dialog("insertPicture", json!({"placement": placement}));
                ui.close();
            }
        }
    });
    let shapes = big_button(ui, Icon::Shapes, "Shapes", "Shapes", true);
    anchor_keytip(app, ui, &shapes, &["insert.shape"]);
    egui::Popup::menu(&shapes).show(|ui| {
        for (label, kind) in [
            ("Rectangle", "rectangle"),
            ("Rounded Rectangle", "roundedRectangle"),
            ("Oval", "ellipse"),
            ("Triangle", "triangle"),
            ("Line", "line"),
            ("Arrow", "arrow"),
            ("Text Box", "textBox"),
        ] {
            if ui.button(label).clicked() {
                act(app, "insert.shape", json!({"kind": kind}));
            }
        }
    });
    let icn = big_button(ui, Icon::Theme, "Icons", "Insert an icon", true);
    anchor_keytip(app, ui, &icn, &["insert.icons"]);
    egui::Popup::menu(&icn).show(|ui| {
        ui.set_width(260.0);
        egui::Grid::new("icon_lib").show(ui, |ui| {
            for (i, (name, icon)) in icons::LIBRARY.iter().enumerate() {
                let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 4.0, Tokens::get(ui.ctx()).hover);
                }
                icons::paint(ui.painter(), r.shrink(6.0), *icon, Tokens::get(ui.ctx()).text);
                if resp.on_hover_text(*name).clicked() {
                    act(app, "insert.icons", json!({"name": name}));
                    ui.close();
                }
                if (i + 1) % 6 == 0 {
                    ui.end_row();
                }
            }
        });
    });
    sep(ui);
    let __k = big_button(ui, Icon::Chart, "Recommended\nCharts", "Recommended Charts", false);
    control_keytip(app, ui, &__k, "insert.recommendedCharts");
    if __k.clicked() {
        // Menu-style: with no chart type, run_or_alert opens the Insert Chart dialog.
        act(app, "insert.recommendedCharts", json!({}));
    }
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            for (icon, tip, cid, subs) in [
                (
                    Icon::ChartBar,
                    "Column or Bar",
                    "insert.chart.column",
                    vec![
                        ("Clustered Column", "column", ""),
                        ("Stacked Column", "column", "stacked"),
                        ("100% Stacked Column", "column", "stacked100"),
                        ("Clustered Bar", "bar", ""),
                        ("Stacked Bar", "bar", "stacked"),
                        ("100% Stacked Bar", "bar", "stacked100"),
                    ],
                ),
                (
                    Icon::ChartLine,
                    "Line or Area",
                    "insert.chart.line",
                    vec![
                        ("Line", "line", ""),
                        ("Line with Markers", "line", "markers"),
                        ("Stacked Line", "line", "stacked"),
                        ("Area", "area", ""),
                        ("Stacked Area", "area", "stacked"),
                        ("100% Stacked Area", "area", "stacked100"),
                    ],
                ),
                (Icon::ChartPie, "Pie or Doughnut", "insert.chart.pie", vec![("Pie", "pie", ""), ("Doughnut", "doughnut", "")]),
            ] {
                let r = small_button(ui, icon, "", tip, true);
                control_keytip(app, ui, &r, cid);
                let forced_c = invoke(app, cid);
                egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
        ui.horizontal(|ui| {
            for (icon, tip, cid, subs) in [
                (
                    Icon::ChartScatter,
                    "Scatter or Bubble",
                    "insert.chart.scatter",
                    vec![("Scatter", "scatter", ""), ("Scatter with Lines", "scatter", "lines"), ("Bubble", "bubble", "")],
                ),
                (Icon::Chart, "Statistic", "insert.chart.statistic", vec![("Histogram", "histogram", ""), ("Box & Whisker", "boxWhisker", "")]),
                (
                    Icon::Theme,
                    "Hierarchy & more",
                    "insert.chart.hierarchy",
                    vec![
                        ("Treemap", "treemap", ""),
                        ("Sunburst", "sunburst", ""),
                        ("Waterfall", "waterfall", ""),
                        ("Funnel", "funnel", ""),
                        ("Radar", "radar", ""),
                        ("Stock", "stock", ""),
                        ("Combo", "combo", ""),
                    ],
                ),
            ] {
                let r = small_button(ui, icon, "", tip, true);
                control_keytip(app, ui, &r, cid);
                let forced_c = invoke(app, cid);
                egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
                    for (label, k, s) in &subs {
                        if ui.button(*label).clicked() {
                            act(app, "insert.chart", json!({"type": k, "subtype": s}));
                        }
                    }
                });
            }
        });
    });
    sep(ui);
    if big_button(ui, Icon::Filter, "Slicer", "Insert a slicer to filter a table", false).clicked() {
        act(app, "insert.slicer", json!({}));
    }
    sep(ui);
    let sp = big_button(ui, Icon::Sparkline, "Sparklines", "Sparklines", true);
    anchor_keytip(app, ui, &sp, &["dialog:sparkline"]);
    egui::Popup::menu(&sp).show(|ui| {
        for (label, kind) in [("Line", "line"), ("Column", "column"), ("Win/Loss", "winLoss")] {
            if ui.button(label).clicked() {
                app.open_dialog("sparkline", json!({"type": kind}));
            }
        }
    });
    sep(ui);
    // Form controls: draw a button, slider or spin button on the sheet (a click places it at its
    // default size). Their key tips run the command at the active cell.
    use gridcraft_engine::model::ControlKind;
    let placing = app.grid.placing;
    ui.vertical(|ui| {
        for (icon, kind, label, tip, id) in [
            (Icon::Button, ControlKind::Button, "Button", "Draw a button that runs a macro or a command", "insert.button"),
            (Icon::Slider, ControlKind::Slider, "Slider", "Draw a slider that sets a whole number in a linked cell", "insert.slider"),
            (Icon::Spin, ControlKind::Spin, "Spin Button", "Draw a spin button that steps a whole number in a linked cell", "insert.spinButton"),
        ] {
            let on = placing == Some(kind);
            let r = small_toggle_button(ui, icon, label, tip, on);
            control_keytip(app, ui, &r, id);
            if r.clicked() {
                app.grid.placing = if placing == Some(kind) { None } else { Some(kind) };
                app.selected_object = None;
            }
        }
    });
    sep(ui);
    // Cell Controls.
    if big_button(ui, Icon::Checkbox, "Checkbox", "Insert checkboxes in the selected cells", false).clicked() {
        act(app, "insert.checkbox", json!({}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::Link, "Link", "Insert Link", false);
    control_keytip(app, ui, &__k, "dialog:insertLink");
    if __k.clicked() {
        app.open_dialog("insertLink", json!({}));
    }
    let __k = big_button(ui, Icon::Comment, "Comment", "New Comment", false);
    control_keytip(app, ui, &__k, "dialog:comment");
    if __k.clicked() {
        app.open_dialog("comment", json!({"threaded": true}));
    }
    sep(ui);
    let __k = big_button(ui, Icon::TextBox, "Text\nBox", "Text Box", false);
    control_keytip(app, ui, &__k, "insert.textBox");
    if __k.clicked() {
        act(app, "insert.textBox", json!({}));
    }
    let __k = big_button(ui, Icon::PageLayout, "Header &\nFooter", "Header & Footer", false);
    control_keytip(app, ui, &__k, "dialog:headerFooter");
    if __k.clicked() {
        // As in Excel: Page Layout view, editing the header in place (the dialog is under Page Setup).
        crate::paper_view::start_editing(app);
    }
    // Symbols: Equation, then Symbol.
    sep(ui);
    let (eq, eq_menu) = big_split_button(ui, Icon::Equation, "Equation", "Insert an equation, or pick one from the gallery");
    control_keytip(app, ui, &eq, "dialog:equation");
    if eq.clicked() {
        app.open_dialog("equation", json!({}));
    }
    egui::Popup::menu(&eq_menu).show(|ui| {
        ui.set_width(300.0);
        if ui.add(egui::Button::new(app.ui.language.tr("Insert New Equation...")).frame(false).min_size(vec2(290.0, 24.0))).clicked() {
            app.open_dialog("equation", json!({}));
            ui.close();
        }
        ui.separator();
        if let Some(latex) = crate::equation::gallery(ui) {
            act(app, "insert.equation", json!({"latex": latex}));
            ui.close();
        }
    });
    let sym = big_button(ui, Icon::Symbol, "Symbol", "Symbol", true);
    control_keytip(app, ui, &sym, "insert.symbol");
    // Opened by its key tip too (Alt+I, S): the gallery picks the symbol, nothing is inserted before.
    let forced = invoke(app, "insert.symbol");
    egui::Popup::menu(&sym).open_memory(popup_command(forced, sym.clicked())).show(|ui| {
        ui.set_width(260.0);
        ui.horizontal_wrapped(|ui| {
            for ch in [
                "€", "£", "¥", "©", "®", "™", "±", "≠", "≤", "≥", "÷", "×", "∞", "µ", "α", "β", "π", "Σ", "Ω", "°", "•", "…", "→", "←", "↑", "↓",
                "✓", "★", "½", "¼",
            ] {
                // No bundled face has ✓: the button paints a code-drawn check instead of a tofu box.
                if crate::widgets::glyph_button(ui, ch.chars().next().unwrap_or('?'), &format!("Insert {ch}")).clicked() {
                    act(app, "insert.symbol", json!({"char": ch}));
                }
            }
        });
    });
}

pub(super) fn draw(app: &mut SheetApp, ui: &mut Ui) {
    use crate::pen_icon;

    let tool = app.session.draw_tool.clone();
    let brush = app.session.draw_brush.clone();
    let color = app.session.draw_color.clone();
    let width = app.session.draw_width;
    let effect = app.session.draw_effect.clone();

    // One Select tool: `draw.lasso` selects ink the way Select Objects does and the grid has no
    // freehand-loop selection, so a second "Lasso Select" button would be the same button twice.
    let __k = big_button(ui, Icon::Shapes, "Select\nObjects", "Select and move ink and shapes", false);
    control_keytip(app, ui, &__k, "draw.lasso");
    if __k.clicked() {
        act(app, "draw.lasso", json!({}));
    }
    let er = big_button(ui, Icon::Eraser, "Eraser", "Erase ink strokes", false);
    if tool == "eraser" {
        ui.painter().rect_stroke(er.rect, 5.0, Stroke::new(2.0, Tokens::get(ui.ctx()).accent), StrokeKind::Inside);
    }
    if er.clicked() {
        act(app, "draw.eraser", json!({}));
    }
    sep(ui);

    // Excel's default pens and the person's own, each tile painted in its own colour and shape.
    app.session.ensure_draw_pens();
    let tr = crate::i18n::current(ui.ctx());
    for pen in app.session.draw_pens.clone() {
        let selected = tool == "pen"
            && brush == pen.tool
            && color.eq_ignore_ascii_case(&pen.color)
            && effect == pen.effect.as_deref().unwrap_or_default()
            && (width - pen.width).abs() < 0.01;
        // A custom pen has no Excel name; naming it after its brush alone gave every custom Pen the
        // same accessible name, so its gallery id tells them apart.
        let label = match gridcraft_engine::cmd::draw::pen_label(&pen) {
            Some(name) => name.to_string(),
            None => format!("{} {}", pen_icon::tool_label(&pen.tool), pen.id),
        };
        let label = label.as_str();
        let (main, arrow) = pen_icon::pen_tile(
            ui,
            pen_icon::PenTile {
                id: pen.id,
                tool: &pen.tool,
                color: hex_color(&pen.color),
                effect: pen.effect.as_deref(),
                tip: tr.tr(label),
                selected,
            },
        );
        if main.clicked() {
            act(app, "draw.pen", json!({"on": true, "pen": pen.id}));
        }
        egui::Popup::menu(&arrow).show(|ui| pen_dropdown(app, ui, &pen));
    }
    let add = big_button(ui, Icon::Plus, "Add", "Add a pen to the gallery", true);
    egui::Popup::menu(&add).show(|ui| {
        for t in pen_icon::TOOLS {
            if ui.button(tr.tr(pen_icon::tool_label(t))).clicked() {
                act(app, "draw.addPen", json!({"brush": t}));
                ui.close();
            }
        }
    });
    sep(ui);
    let __k = big_button(ui, Icon::Shapes, "Ink to\nShape", "Convert the last ink stroke to a shape", false);
    control_keytip(app, ui, &__k, "draw.inkToShape");
    if __k.clicked() {
        act(app, "draw.inkToShape", json!({}));
    }
}

/// A pen dropdown's colour for a `#RRGGBB` string.
fn hex_color(hex: &str) -> egui::Color32 {
    gridcraft_engine::model::Color::from_hex(hex)
        .and_then(|c| c.resolve(&gridcraft_engine::model::Theme::default()))
        .map(crate::theme::color32)
        .unwrap_or(egui::Color32::GRAY)
}

/// The sixteen solid colours Excel offers for a pen.
const PEN_SOLIDS: [&str; 16] = [
    "#000000", "#FFFFFF", "#A5A5A5", "#D9D9D9", "#C00000", "#FF0000", "#FFC000", "#FFFF00", "#92D050", "#00B050", "#00B0F0", "#0070C0", "#002060",
    "#7030A0", "#E36C0A", "#7F6000",
];

/// The body of a pen's dropdown: thickness steps for the brush, the solid colours, the eight
/// textures, More Colors… and, for a pen from the gallery, Delete Tool. A change applies to the
/// pen being drawn with at once, so the tile's preview follows.
fn pen_dropdown(app: &mut SheetApp, ui: &mut Ui, pen: &gridcraft_engine::cmd::draw::DrawPen) {
    use crate::pen_icon;
    let (tool, color, width, effect, id) = (pen.tool.as_str(), pen.color.as_str(), pen.width, pen.effect.as_deref(), pen.id);
    let deletable = id >= gridcraft_engine::cmd::draw::DEFAULT_PEN_COUNT;
    let tr = crate::i18n::current(ui.ctx());
    let parsed = gridcraft_engine::cmd::draw::parse_tool(tool).unwrap_or_default();
    let steps = gridcraft_engine::cmd::draw::widths_mm(parsed);
    let t = Tokens::get(ui.ctx());
    // The thickness slider: Excel's steps for the brush. Its own label names it for a screen
    // reader, and the value shown is the pen's real width when it is between steps.
    let mut idx = steps.iter().enumerate().min_by(|a, b| (a.1 - width).abs().total_cmp(&(b.1 - width).abs())).map_or(0, |(i, _)| i);
    let slider = egui::Slider::new(&mut idx, 0..=steps.len().saturating_sub(1)).text(tr.tr("Thickness")).custom_formatter(move |v, _| {
        let step = steps.get(v as usize).copied().unwrap_or(width);
        let mm = if (step - width).abs() > 0.01 { width } else { step };
        format!("{mm} mm")
    });
    if ui.add(slider).changed()
        && let Some(mm) = steps.get(idx).copied()
    {
        apply_pen_change(app, id, tool, color, mm, effect);
    }
    ui.add_space(2.0);
    ui.label(egui::RichText::new(tr.tr("Colors")).small());
    for row in PEN_SOLIDS.chunks(8) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for hex in row {
                let (r, resp) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::click());
                let c = hex_color(hex);
                ui.painter().rect_filled(r, 2.0, c);
                let on = hex.eq_ignore_ascii_case(color);
                let edge = if on || resp.has_focus() {
                    t.accent
                } else if resp.hovered() {
                    t.text_dim
                } else {
                    egui::Color32::from_gray(160)
                };
                ui.painter().rect_stroke(r, 2.0, Stroke::new(if on { 2.0 } else { 1.0 }, edge), StrokeKind::Inside);
                if resp.has_focus() {
                    ui.painter().rect_stroke(r.expand(1.0), 2.5, Stroke::new(1.0, t.accent), StrokeKind::Outside);
                }
                let name = format!("{} {hex}", tr.tr("Pen colour"));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), &name));
                if resp.on_hover_text(*hex).clicked() {
                    // A solid colour choice replaces the texture: keeping the effect would make
                    // `stroke_color` paint the texture's base colour instead of the chosen one.
                    apply_pen_change(app, id, tool, hex, width, None);
                    ui.close();
                }
            }
        });
    }
    ui.add_space(2.0);
    ui.label(egui::RichText::new(tr.tr("Textures")).small());
    for row in pen_icon::EFFECTS.chunks(4) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for name in row {
                let (r, resp) = ui.allocate_exact_size(vec2(34.0, 24.0), Sense::click());
                pen_icon::paint_effect_swatch(ui.painter(), r, name);
                let on = effect == Some(*name);
                if on || resp.has_focus() {
                    ui.painter().rect_stroke(r.expand(1.0), 2.0, Stroke::new(2.0, t.accent), StrokeKind::Outside);
                } else if resp.hovered() {
                    ui.painter().rect_stroke(r.expand(1.0), 2.0, Stroke::new(1.0, t.text_dim), StrokeKind::Outside);
                }
                let label = tr.tr(pen_icon::effect_label(name));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
                if resp.on_hover_text(label).clicked() {
                    // The engine resets the colour when only the effect changes, so carry the
                    // texture's base colour from the model's single table.
                    let base = gridcraft_engine::model::InkEffect::from_name(name).map(|e| e.base_hex()).unwrap_or("#1F5FC9");
                    apply_pen_change(app, id, tool, base, width, Some(name));
                    ui.close();
                }
            }
        });
    }
    if ui.button(tr.tr("Solid")).clicked() {
        apply_pen_change(app, id, tool, color, width, None);
        ui.close();
    }
    // More Colors…: a submenu, so opening the picker does not close the pen dropdown; the picker
    // applies as it moves. Its stored HSVA is keyed by the pen's current colour, so a swatch or
    // texture chosen since it was last open cannot leave a stale value behind.
    ui.menu_button(tr.tr("More Colors…"), |ui| {
        let hv_id = ui.scope_id().with(("draw_pen_more_colors", id));
        let cur = hex_color(color);
        let current = [cur.r(), cur.g(), cur.b()];
        let stored: Option<egui::ecolor::Hsva> = ui.data_mut(|d| d.get_temp(hv_id));
        // Keep the stored hue while it still renders as the pen's colour, so moving the picker
        // does not reset it, and a swatch or texture chosen since cannot leave a stale value.
        let close = |h: &egui::ecolor::Hsva| {
            let [r, g, b] = h.to_srgb();
            r.abs_diff(current[0]) <= 1 && g.abs_diff(current[1]) <= 1 && b.abs_diff(current[2]) <= 1
        };
        let mut hsva = stored.filter(close).unwrap_or_else(|| egui::ecolor::Hsva::from_srgb(current));
        let before = hsva;
        egui::color_picker::color_picker_hsva_2d(ui, &mut hsva, egui::color_picker::Alpha::Opaque);
        if hsva != before {
            ui.data_mut(|d| d.insert_temp(hv_id, hsva));
            let [r, g, b] = hsva.to_srgb();
            let hex = format!("#{r:02X}{g:02X}{b:02X}");
            // Same as the swatches: a picked colour is solid, not textured.
            apply_pen_change(app, id, tool, &hex, width, None);
        }
    });
    if deletable && ui.button(tr.tr("Delete Tool")).clicked() {
        act(app, "draw.deletePen", json!({"id": id}));
        ui.close();
    }
}

/// Applies a dropdown choice to the gallery pen and makes it the pen in hand: the dropdown was
/// opened from that pen's own tile, so the tile it changed is the one it draws with.
fn apply_pen_change(app: &mut SheetApp, id: u32, tool: &str, color: &str, width: f32, effect: Option<&str>) {
    act(app, "draw.updatePen", json!({"id": id, "brush": tool, "color": color, "width": width, "effect": effect.unwrap_or("")}));
    act(app, "draw.pen", json!({"on": true, "pen": id}));
}
