//! Shared Format Cells controls, with optional inheritance toggles for conditional formats.
use egui::{Color32, RichText};
use gridcraft_engine::model::{BorderStyle, Color, PatternType, Style, StyleProperty, Underline as FontUnderline, VertAlign};
use serde_json::json;

use crate::{SheetApp, dialogs::Dialog, theme};

#[derive(Clone, Debug, Default)]
pub(crate) struct ColorDraft {
    text: String,
    applied: Option<Color>,
}

pub(crate) type ColorDrafts = std::collections::BTreeMap<&'static str, ColorDraft>;

pub(crate) fn validate_colors(d: &Dialog) -> Result<(), String> {
    let style: Style = d.values.get("style").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let enabled = |p| d.name == "formatCells" || style.sets(p);
    for (slot, active) in [
        ("font", enabled(StyleProperty::FontColor)),
        ("fillForeground", enabled(StyleProperty::Fill) && style.fill.pattern != PatternType::None),
        ("fillBackground", enabled(StyleProperty::Fill) && !matches!(style.fill.pattern, PatternType::None | PatternType::Solid)),
        ("Left", enabled(StyleProperty::LeftBorder) && !style.borders.left.is_none()),
        ("Right", enabled(StyleProperty::RightBorder) && !style.borders.right.is_none()),
        ("Top", enabled(StyleProperty::TopBorder) && !style.borders.top.is_none()),
        ("Bottom", enabled(StyleProperty::BottomBorder) && !style.borders.bottom.is_none()),
        ("Diagonal down", enabled(StyleProperty::DiagonalDown) && !style.borders.diag_down.is_none()),
        ("Diagonal up", enabled(StyleProperty::DiagonalUp) && !style.borders.diag_up.is_none()),
    ] {
        if active && d.color_drafts.get(slot).is_some_and(|draft| draft.applied.is_some() && Color::from_hex(&draft.text).is_none()) {
            return Err("Enter a valid hex color or choose a color from the picker.".into());
        }
    }
    Ok(())
}

fn active_style(app: &SheetApp) -> Style {
    let Some(doc) = app.session.active() else { return Style::default() };
    let Some(sheet) = doc.wb.active() else { return Style::default() };
    let at = doc.selection.active;
    match sheet.table_at(at) {
        Some(table) => gridcraft_engine::tables::table_cell_style(&doc.wb, doc.wb.active_sheet, table, at),
        None => doc.wb.styles.get(sheet.style_id(at)).clone(),
    }
}

pub(crate) fn preview(app: &SheetApp, ui: &mut egui::Ui, style: &Style) {
    let Some(doc) = app.session.active() else { return };
    let lang = app.ui.language;
    let theme = &doc.wb.theme;
    let mut effective = active_style(app);
    effective.apply_differential(style);
    let value = doc.wb.active().map(|s| s.value(doc.selection.active)).unwrap_or_default();
    let formatted = gridcraft_engine::display::format(&value, effective.num_fmt.as_str(), &doc.wb);
    let text = if value.is_empty() { "AaBbCcYyZz 123.45".into() } else { formatted.text };
    let font_color = effective.font.color.resolve(theme).map(theme::color32).unwrap_or(Color32::BLACK);
    let color = if style.sets(StyleProperty::FontColor) { font_color } else { formatted.color.map(crate::grid::numfmt_color).unwrap_or(font_color) };
    let fill = match effective.fill.pattern {
        PatternType::None => theme::Tokens::get(ui.ctx()).grid_bg,
        PatternType::Solid => effective.fill.fg.resolve(theme).map(theme::color32).unwrap_or(Color32::WHITE),
        _ => {
            let fg = effective.fill.fg.resolve(theme).unwrap_or([0, 0, 0]);
            let bg = effective.fill.bg.resolve(theme).unwrap_or([255, 255, 255]);
            theme::color32([0, 1, 2].map(|i| ((fg[i] as u16 + bg[i] as u16 * 3) / 4) as u8))
        }
    };
    ui.label(lang.tr("Preview"));
    let preview = egui::Frame::new().fill(fill).inner_margin(12.0).show(ui, |ui| {
        ui.set_min_width(320.0);
        let mut text = RichText::new(text).color(color).font(egui::FontId::new(
            effective.font.size.clamp(6.0, 36.0) * 96.0 / 72.0,
            theme::cell_family(&effective.font.name, effective.font.bold, effective.font.italic),
        ));
        if effective.font.underline != FontUnderline::None {
            text = text.underline();
        }
        if effective.font.strike {
            text = text.strikethrough();
        }
        ui.label(text);
    });
    let rect = preview.response.rect.shrink(2.0);
    for (line, points) in [
        (effective.borders.top, [rect.left_top(), rect.right_top()]),
        (effective.borders.bottom, [rect.left_bottom(), rect.right_bottom()]),
        (effective.borders.left, [rect.left_top(), rect.left_bottom()]),
        (effective.borders.right, [rect.right_top(), rect.right_bottom()]),
        (effective.borders.diag_down, [rect.left_top(), rect.right_bottom()]),
        (effective.borders.diag_up, [rect.left_bottom(), rect.right_top()]),
    ] {
        crate::grid::paint_border(ui.painter(), points, line.style, line.color.resolve(theme).map(theme::color32).unwrap_or(Color32::BLACK));
    }
}

// Keep the shared editor's English labels and reuse translations from Format Cells.
fn translated_label<'a>(lang: crate::i18n::Language, label: &'a str, key: &'a str) -> &'a str {
    let translated = lang.tr(key);
    if translated == key { label } else { translated }
}

fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, id: &str, value: &mut T, options: &[(T, &str)], lang: crate::i18n::Language) {
    let label = options.iter().find(|(v, _)| v == value).map(|(_, n)| *n).unwrap_or("");
    egui::ComboBox::from_id_salt(id).selected_text(lang.tr(label)).show_ui(ui, |ui| {
        for (v, name) in options {
            ui.selectable_value(value, *v, lang.tr(name));
        }
    });
}

fn override_row(
    ui: &mut egui::Ui,
    set: &mut std::collections::BTreeSet<StyleProperty>,
    conditional: bool,
    property: StyleProperty,
    label: &str,
    draw: impl FnOnce(&mut egui::Ui),
) {
    ui.push_id(property, |ui| {
        ui.horizontal(|ui| {
            if !conditional {
                ui.label(label);
                draw(ui);
                return;
            }
            let mut enabled = set.contains(&property);
            if ui.checkbox(&mut enabled, label).changed() {
                if enabled {
                    set.insert(property);
                } else {
                    set.remove(&property);
                }
            }
            ui.add_enabled_ui(enabled, draw);
        });
    });
}

fn color(ui: &mut egui::Ui, value: &mut Color, theme: &gridcraft_engine::model::Theme, draft: &mut ColorDraft, lang: crate::i18n::Language) {
    let hex = |c: Color| {
        let [r, g, b] = c.resolve(theme).unwrap_or([0, 0, 0]);
        format!("#{r:02X}{g:02X}{b:02X}")
    };
    let mut rgb = value.resolve(theme).unwrap_or([0, 0, 0]);
    if ui.color_edit_button_srgb(&mut rgb).changed() {
        *value = Color::rgb(rgb[0], rgb[1], rgb[2]);
    }
    if draft.applied != Some(*value) {
        draft.text = hex(*value);
    }
    let response = ui.add(egui::TextEdit::singleline(&mut draft.text).desired_width(85.0));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, ui.is_enabled(), "Color hex"));
    if response.changed()
        && let Some(c) = Color::from_hex(&draft.text)
    {
        *value = c;
    }
    if ui.small_button(lang.tr("Automatic")).clicked() {
        *value = Color::Auto;
        draft.text = hex(*value);
    }
    ui.menu_button(lang.tr("Palette"), |ui| {
        if let Some(c) = crate::widgets::color_palette(ui, &theme.colors, "Automatic") {
            *value = if c == "none" { Color::Auto } else { Color::from_hex(&c).unwrap_or_default() };
            draft.text = hex(*value);
            ui.close();
        }
    });
    draft.applied = Some(*value);
}

/// Number, Font, Border and Fill controls shared by Format Cells and conditional rules.
/// `specified` adds inheritance toggles to the same controls for differential formatting.
pub(crate) fn options(
    app: &SheetApp,
    ui: &mut egui::Ui,
    style: &mut Style,
    tab: &str,
    specified: Option<&mut std::collections::BTreeSet<StyleProperty>>,
    drafts: &mut ColorDrafts,
) {
    use StyleProperty::*;
    let lang = app.ui.language;
    let conditional = specified.is_some();
    let mut unused = Default::default();
    let set = specified.unwrap_or(&mut unused);
    let theme = app.session.active().map(|doc| doc.wb.theme.clone()).unwrap_or_default();
    let mut draw = |ui: &mut egui::Ui| {
        match tab {
            "Number" => {
                let mut draw = |ui: &mut egui::Ui| {
                    let sample = app.session.active().and_then(|doc| doc.wb.active().map(|s| s.value(doc.selection.active))).unwrap_or_default();
                    let mut code = style.num_fmt.as_str().to_string();
                    ui.columns(2, |cols| {
                        cols[0].label(egui::RichText::new(lang.tr("Category:")).strong());
                        for name in [
                            "General",
                            "Number",
                            "Currency",
                            "Accounting",
                            "Short Date",
                            "Long Date",
                            "Time",
                            "Percentage",
                            "Fraction",
                            "Scientific",
                            "Text",
                        ] {
                            let c = gridcraft_engine::cmd::format::format_code_for(name).to_string();
                            let response = cols[0].selectable_label(code == c, lang.tr(name));
                            if response.clicked() {
                                code = c;
                            }
                        }
                        cols[1].label(egui::RichText::new(lang.tr("Sample")).strong());
                        let preview =
                            app.session.active().map(|doc| gridcraft_engine::display::format(&sample, &code, &doc.wb).text).unwrap_or_default();
                        cols[1].label(egui::RichText::new(preview).font(theme::ui_font(15.0)));
                        cols[1].add_space(8.0);
                        cols[1].label("Type (custom format code):");
                        cols[1].text_edit_singleline(&mut code);
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
                    style.num_fmt = gridcraft_engine::model::NumFmt::new(&code);
                };
                if conditional {
                    override_row(ui, set, true, NumberFormat, translated_label(lang, "Number format", "Number"), draw);
                } else {
                    draw(ui);
                }
            }
            "Font" => {
                override_row(ui, set, conditional, FontName, translated_label(lang, "Font family", "Font"), |ui| {
                    egui::ComboBox::from_id_salt("family").selected_text(&style.font.name).show_ui(ui, |ui| {
                        for family in crate::fonts::families() {
                            ui.selectable_value(&mut style.font.name, family.clone(), family);
                        }
                    });
                });
                override_row(ui, set, conditional, FontSize, translated_label(lang, "Font size", "Size"), |ui| {
                    ui.add(egui::DragValue::new(&mut style.font.size).range(1.0..=409.0).suffix(" pt"));
                });
                for (property, label, value) in [
                    (Bold, "Bold", &mut style.font.bold),
                    (Italic, "Italic", &mut style.font.italic),
                    (Strike, "Strikethrough", &mut style.font.strike),
                ] {
                    override_row(ui, set, conditional, property, lang.tr(label), |ui| {
                        choice(ui, label, value, &[(false, "Off"), (true, "On")], lang);
                    });
                }
                override_row(ui, set, conditional, StyleProperty::Underline, lang.tr("Underline"), |ui| {
                    choice(
                        ui,
                        "underline",
                        &mut style.font.underline,
                        &[
                            (FontUnderline::None, "None"),
                            (FontUnderline::Single, "Single"),
                            (FontUnderline::Double, "Double"),
                            (FontUnderline::SingleAccounting, "Single Accounting"),
                            (FontUnderline::DoubleAccounting, "Double Accounting"),
                        ],
                        lang,
                    );
                });
                override_row(ui, set, conditional, FontVertical, translated_label(lang, "Vertical position", "Normal"), |ui| {
                    choice(
                        ui,
                        "vertical",
                        &mut style.font.vert,
                        &[(VertAlign::Baseline, "Normal"), (VertAlign::Superscript, "Superscript"), (VertAlign::Subscript, "Subscript")],
                        lang,
                    );
                });
                override_row(ui, set, conditional, FontColor, translated_label(lang, "Font color", "Color:"), |ui| {
                    color(ui, &mut style.font.color, &theme, drafts.entry("font").or_default(), lang);
                });
            }
            "Fill" => override_row(ui, set, conditional, Fill, lang.tr("Format Cells|Fill"), |ui| {
                ui.vertical(|ui| {
                    choice(
                        ui,
                        "pattern",
                        &mut style.fill.pattern,
                        &[
                            (PatternType::None, "No fill"),
                            (PatternType::Solid, "Solid"),
                            (PatternType::Gray50, "50% gray"),
                            (PatternType::Gray75, "75% gray"),
                            (PatternType::Gray25, "25% gray"),
                            (PatternType::Gray125, "12.5% gray"),
                            (PatternType::Gray0625, "6.25% gray"),
                            (PatternType::DarkHorizontal, "Dark horizontal"),
                            (PatternType::DarkVertical, "Dark vertical"),
                            (PatternType::DarkDown, "Dark down"),
                            (PatternType::DarkUp, "Dark up"),
                            (PatternType::DarkGrid, "Dark grid"),
                            (PatternType::DarkTrellis, "Dark trellis"),
                            (PatternType::LightHorizontal, "Light horizontal"),
                            (PatternType::LightVertical, "Light vertical"),
                            (PatternType::LightDown, "Light down"),
                            (PatternType::LightUp, "Light up"),
                            (PatternType::LightGrid, "Light grid"),
                            (PatternType::LightTrellis, "Light trellis"),
                        ],
                        lang,
                    );
                    ui.horizontal(|ui| {
                        ui.label(lang.tr("Foreground"));
                        color(ui, &mut style.fill.fg, &theme, drafts.entry("fillForeground").or_default(), lang);
                    });
                    if !matches!(style.fill.pattern, PatternType::None | PatternType::Solid) {
                        ui.horizontal(|ui| {
                            ui.label(lang.tr("Background"));
                            color(ui, &mut style.fill.bg, &theme, drafts.entry("fillBackground").or_default(), lang);
                        });
                    }
                });
            }),
            "Border" => {
                let old_diagonal = style.borders.diag_down;
                ui.horizontal(|ui| {
                    if ui.button(lang.tr("Outline")).clicked() {
                        for p in [LeftBorder, RightBorder, TopBorder, BottomBorder] {
                            set.insert(p);
                        }
                        for line in [&mut style.borders.left, &mut style.borders.right, &mut style.borders.top, &mut style.borders.bottom] {
                            if line.is_none() {
                                line.style = BorderStyle::Thin;
                            }
                        }
                    }
                    if ui.button(lang.tr("No borders")).clicked() {
                        for p in [LeftBorder, RightBorder, TopBorder, BottomBorder, DiagonalDown, DiagonalUp] {
                            set.insert(p);
                        }
                        style.borders = Default::default();
                    }
                });
                for (p, label, line) in [
                    (LeftBorder, "Left", &mut style.borders.left),
                    (RightBorder, "Right", &mut style.borders.right),
                    (TopBorder, "Top", &mut style.borders.top),
                    (BottomBorder, "Bottom", &mut style.borders.bottom),
                    (DiagonalDown, "Diagonal down", &mut style.borders.diag_down),
                    (DiagonalUp, "Diagonal up", &mut style.borders.diag_up),
                ] {
                    override_row(ui, set, conditional, p, lang.tr(label), |ui| {
                        let styles = &[
                            (BorderStyle::None, "None"),
                            (BorderStyle::Thin, "Thin"),
                            (BorderStyle::Medium, "Medium"),
                            (BorderStyle::Thick, "Thick"),
                            (BorderStyle::Dashed, "Dashed"),
                            (BorderStyle::Dotted, "Dotted"),
                            (BorderStyle::Double, "Double"),
                            (BorderStyle::Hair, "Hair"),
                            (BorderStyle::MediumDashed, "Medium dashed"),
                            (BorderStyle::DashDot, "Dash dot"),
                            (BorderStyle::MediumDashDot, "Medium dash dot"),
                            (BorderStyle::DashDotDot, "Dash dot dot"),
                            (BorderStyle::MediumDashDotDot, "Medium dash dot dot"),
                            (BorderStyle::SlantDashDot, "Slant dash dot"),
                        ];
                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                for (style, name) in styles.iter().take(7) {
                                    let response = ui.selectable_label(line.style == *style, lang.tr(&name.to_ascii_lowercase()));
                                    response.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::SelectableLabel,
                                            ui.is_enabled(),
                                            format!("{} {}", lang.tr(label), lang.tr(&name.to_ascii_lowercase())),
                                        )
                                    });
                                    if response.clicked() {
                                        line.style = *style;
                                    }
                                }
                                ui.menu_button(lang.tr("More styles"), |ui| {
                                    for (style, name) in styles.iter().skip(7) {
                                        if ui.selectable_value(&mut line.style, *style, lang.tr(name)).clicked() {
                                            ui.close();
                                        }
                                    }
                                });
                            });
                            ui.horizontal(|ui| {
                                ui.label(lang.tr("Color"));
                                color(ui, &mut line.color, &theme, drafts.entry(label).or_default(), lang);
                            });
                        });
                    });
                }
                // Both diagonal directions share a line style in XLSX.
                if conditional && set.contains(&DiagonalDown) && set.contains(&DiagonalUp) {
                    if style.borders.diag_down != old_diagonal {
                        style.borders.diag_up = style.borders.diag_down;
                    } else {
                        style.borders.diag_down = style.borders.diag_up;
                    }
                }
            }
            _ => {}
        }
    };
    if !conditional && tab == "Number" {
        draw(ui);
    } else {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, draw);
    }
    if !conditional && tab == "Border" {
        crate::border_preview::sample(ui, &style.borders, &theme);
    }
}

pub(crate) fn editor(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog) {
    let lang = app.ui.language;
    let patch: Style = d.values.get("style").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let mut set = patch.differential_properties();
    let mut style = active_style(app);
    style.apply_differential(&patch);
    ui.horizontal(|ui| {
        for tab in ["Number", "Font", "Border", "Fill"] {
            if ui.selectable_label(d.tab == tab, lang.tr(if tab == "Fill" { "Format Cells|Fill" } else { tab })).clicked() {
                d.tab = tab.into();
            }
        }
    });
    ui.separator();
    ui.weak(lang.tr("Checked properties override the cell format. Unchecked properties stay unchanged."));
    options(app, ui, &mut style, &d.tab, Some(&mut set), &mut d.color_drafts);
    ui.separator();
    let mut selected = Style::default();
    for property in &set {
        selected.copy_property(&style, *property);
    }
    // Keep imported fields not exposed by the four conditional-format tabs.
    selected.align = patch.align;
    selected.protection = patch.protection;
    selected.specified = Some(set);
    preview(app, ui, &selected);
    if ui.button(lang.tr("Clear custom formatting")).clicked() {
        selected = Style::default();
        selected.specified = Some(Default::default());
    }
    if let Ok(value) = serde_json::to_value(&selected) {
        d.values.insert("style".into(), value);
    }
    d.values.insert("preset".into(), json!("custom"));
}
