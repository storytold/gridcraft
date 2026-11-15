use super::{
    Color32, Icon, Json, Key, Language, Modifiers, Rect, RichText, Sense, SheetApp, Stroke, StrokeKind, Tokens, Ui, dim, i18n, icons, json, pos2,
    section, theme, vec2,
};

/// Excel's File › Options page: General (display theme, interface language and Show formula bar),
/// Editing (AutoComplete) and Save (the autosave checkbox toggling `app.ui.autosave`), plus Formulas
/// (calculation mode and iterative calculation) and the display options for this workbook (sheet
/// tabs and scroll bars) and for a worksheet (`sheet`: the one picked in its list) when a
/// document is active. Theme, language, formula
/// bar, AutoComplete and calculation changes run their commands through `run_or_alert`; calculation
/// settings are saved with the workbook.
pub(super) fn options(app: &mut SheetApp, ui: &mut Ui, sheet: &mut Option<u64>) {
    let lang = app.ui.language;
    section(ui, lang, "Options|General");
    general_options(app, ui, lang);
    ui.add_space(6.0);
    let mut formula_bar = app.session.prefs.formula_bar;
    if ui.checkbox(&mut formula_bar, lang.tr("Show formula bar")).changed() {
        app.run_or_alert("view.formulaBar", json!({"on": formula_bar}));
    }
    let mut highlight_row = app.ui.highlight_active_row;
    if ui.checkbox(&mut highlight_row, lang.tr("Highlight the active cell's row")).changed() {
        app.run_or_alert("options.highlightActiveRow", json!({"on": highlight_row}));
    }
    ui.add_space(18.0);
    section(ui, lang, "Options|Editing");
    let mut autocomplete = app.ui.autocomplete;
    if ui.checkbox(&mut autocomplete, lang.tr("Enable AutoComplete for cell values")).changed() {
        app.run_or_alert("options.autoComplete", json!({"on": autocomplete}));
    }
    ui.add_space(18.0);
    section(ui, lang, "Options|Save");
    let mut autosave = app.ui.autosave;
    if ui.checkbox(&mut autosave, lang.tr("Save .xlsx workbooks automatically after every change")).changed() {
        app.toggle_autosave();
    }
    let Some((calc, view)) = app.session.active().map(|d| (d.wb.calc.clone(), d.wb.view)) else { return };
    ui.add_space(18.0);
    // Excel's Advanced › Display options for this workbook: saved with the workbook (#3101).
    section(ui, lang, "Display options for this workbook");
    for (on, label, id) in [
        (view.show_horizontal_scroll, "Show horizontal scroll bar", "view.showHorizontalScrollBar"),
        (view.show_vertical_scroll, "Show vertical scroll bar", "view.showVerticalScrollBar"),
        (view.show_sheet_tabs, "Show sheet tabs", "view.showSheetTabs"),
    ] {
        let mut on = on;
        if ui.checkbox(&mut on, lang.tr(label)).changed() {
            app.run_or_alert(id, json!({"on": on}));
        }
    }
    ui.add_space(18.0);
    worksheet_options(app, ui, lang, sheet);
    ui.add_space(18.0);
    section(ui, lang, "Formulas");
    ui.label(lang.tr("Workbook calculation"));
    calc_options(app, ui, lang, &calc);
    ui.add_space(6.0);
    dim(ui, lang.tr("Calculation settings are saved with the workbook."));
}

/// Excel's Advanced › Display options for this worksheet (#3100), for the sheet picked in the
/// list beside the heading (`picked`, a sheet's uid; the active sheet until one is picked). Each
/// control runs its sheet command with `sheet`; all of it is saved with the workbook.
fn worksheet_options(app: &mut SheetApp, ui: &mut Ui, lang: Language, picked: &mut Option<u64>) {
    let Some(wb) = app.session.active().map(|d| d.wb.clone()) else { return };
    let index = picked.and_then(|uid| wb.sheets.iter().position(|sh| sh.uid.get() == uid)).unwrap_or(wb.active_sheet());
    let Some(sheet) = wb.sheet(index) else { return };
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let heading = RichText::new(lang.tr("Display options for this worksheet")).font(theme::ui_bold(15.0)).color(t.text);
        let label = ui.label(heading);
        ui.add_space(8.0);
        craft_ui::ComboBox::from_id_salt("backstage_options_sheet")
            .width(160.0)
            .selected_text(sheet.name.as_str())
            .show_ui(ui, |ui| {
                for (i, sh) in wb.sheets.iter().enumerate() {
                    if ui.selectable_label(i == index, sh.name.as_str()).clicked() {
                        *picked = Some(sh.uid.get());
                    }
                }
            })
            .response
            .labelled_by(label.id);
    });
    ui.add_space(8.0);
    let name = sheet.name.as_str();
    for (on, label, id) in [
        (sheet.show_headings, "Show row and column headers", "view.headings"),
        (sheet.show_formulas, "Show formulas in cells instead of their calculated results", "formulas.showFormulas"),
        (sheet.show_zeros, "Show a zero in cells that have zero value", "view.showZeros"),
        (sheet.show_outline_symbols, "Show outline symbols if an outline is applied", "view.showOutlineSymbols"),
        (sheet.show_gridlines, "Show gridlines", "view.gridlines"),
    ] {
        let mut on = on;
        if ui.checkbox(&mut on, lang.tr(label)).changed() {
            app.run_or_alert(id, json!({"sheet": name, "on": on}));
        }
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.label(lang.tr("Gridline color"));
        let current = sheet.gridline_color.zip(sheet.gridline_rgb());
        if let Some(pick) = gridline_color_picker(ui, lang, current) {
            app.run_or_alert("view.gridlineColor", json!({"sheet": name, "colorIndex": pick}));
        }
    });
}

/// The Gridline color button: the sheet's colour (`current`: its palette index and RGB; the theme's
/// gridline colour for Automatic) and a menu of Automatic and the 56 palette colours, eight to a
/// row in palette order. Returns the pick as `view.gridlineColor` takes it: an index or "automatic".
fn gridline_color_picker(ui: &mut Ui, lang: Language, current: Option<(u8, [u8; 3])>) -> Option<Json> {
    let t = Tokens::get(ui.ctx());
    let automatic = lang.tr("Automatic");
    let hex = |[r, g, b]: [u8; 3]| format!("#{r:02X}{g:02X}{b:02X}");
    let shown = current.map_or_else(|| automatic.to_string(), |(_, rgb)| hex(rgb));
    let (rect, button) = ui.allocate_exact_size(vec2(118.0, 22.0), Sense::click());
    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, lang.tr("Gridline color")));
    let p = ui.painter();
    p.rect_filled(rect, 3.0, t.input_bg);
    p.rect_stroke(rect, 3.0, Stroke::new(1.0, if button.hovered() { t.accent } else { t.input_border }), StrokeKind::Inside);
    let swatch = Rect::from_min_size(rect.min + vec2(5.0, 5.0), vec2(22.0, rect.height() - 10.0));
    p.rect_filled(swatch, 1.0, current.map_or(t.gridline, |(_, [r, g, b])| Color32::from_rgb(r, g, b)));
    p.rect_stroke(swatch, 1.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    p.text(pos2(swatch.right() + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, &shown, theme::ui_font(12.5), t.text);
    icons::paint(p, Rect::from_center_size(pos2(rect.right() - 10.0, rect.center().y), vec2(10.0, 10.0)), Icon::Chevron, t.text_dim);
    let mut pick = None;
    egui::Popup::menu(&button).show(|ui| {
        if ui.selectable_label(current.is_none(), automatic).clicked() {
            pick = Some(json!("automatic"));
            ui.close();
        }
        ui.add_space(4.0);
        egui::Grid::new("backstage_gridline_colors").min_col_width(18.0).spacing(vec2(3.0, 3.0)).show(ui, |ui| {
            for index in 1..=56u8 {
                let Some(rgb @ [r, g, b]) = gridcraft_engine::core::color::indexed_color(index) else { continue };
                let selected = current.is_some_and(|(i, _)| i == index);
                let (rect, swatch) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                swatch.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, hex(rgb)));
                let p = ui.painter();
                p.rect_filled(rect, 1.0, Color32::from_rgb(r, g, b));
                p.rect_stroke(rect, 1.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
                if selected || swatch.hovered() {
                    p.rect_stroke(rect.expand(1.0), 1.0, Stroke::new(1.5, t.accent), StrokeKind::Outside);
                }
                if swatch.on_hover_text(hex(rgb)).clicked() {
                    pick = Some(json!(index));
                    ui.close();
                }
                if index % 8 == 0 {
                    ui.end_row();
                }
            }
        });
    });
    pick
}

/// The display theme and interface language controls of the General section.
fn general_options(app: &mut SheetApp, ui: &mut Ui, lang: Language) {
    egui::Grid::new("backstage_options_general").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
        let label = ui.label(lang.tr("Display theme"));
        let mode = app.ui.theme_mode();
        let current = match mode {
            "system" => "System",
            "dark" => "Dark",
            _ => "Light",
        };
        craft_ui::ComboBox::from_id_salt("backstage_theme")
            .width(160.0)
            .selected_text(lang.tr(current))
            .show_ui(ui, |ui| {
                for (value, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                    if ui.selectable_label(mode == value, lang.tr(label)).clicked() {
                        app.run_or_alert("view.theme", json!({"mode": value}));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();
        let label = ui.label(lang.tr("Interface language"));
        craft_ui::ComboBox::from_id_salt("backstage_language")
            .width(160.0)
            .selected_text(lang.name())
            .show_ui(ui, |ui| {
                for l in i18n::Language::for_picker() {
                    if ui.selectable_label(l == lang, l.name()).clicked() {
                        app.run_or_alert("app.language.set", json!({"language": l.code()}));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();
    });
}

/// The calculation mode, and iterative calculation, of the active workbook.
fn calc_options(app: &mut SheetApp, ui: &mut Ui, lang: Language, calc: &gridcraft_engine::model::CalcSettings) {
    use gridcraft_engine::model::CalcMode;
    for (mode, id, label) in [
        (CalcMode::Automatic, "automatic", "Automatic"),
        (CalcMode::AutomaticExceptTables, "automaticExceptTables", "Automatic Except for Data Tables"),
        (CalcMode::Manual, "manual", "Manual"),
    ] {
        if ui.radio(calc.mode == mode, lang.tr(label)).clicked() && calc.mode != mode {
            app.run_or_alert("formulas.calculationOptions", json!({"mode": id}));
        }
    }
    ui.add_space(4.0);
    let mut iterative = calc.iterative;
    if ui.checkbox(&mut iterative, lang.tr("Enable iterative calculation")).changed() {
        app.run_or_alert("formulas.calculationOptions", json!({"iterative": iterative}));
    }
    // Maximum Iterations and Maximum Change apply only while iteration is on, as in Excel.
    iteration_options(app, ui, lang, calc, iterative);
    // TODO: add "Set precision as displayed" here once `formulas.calculationOptions` takes
    // `precisionAsDisplayed` (PR #1366); the model already stores it.
}

/// Maximum Iterations and Maximum Change, live only while iterative calculation is on.
fn iteration_options(app: &mut SheetApp, ui: &mut Ui, lang: Language, calc: &gridcraft_engine::model::CalcSettings, iterative: bool) {
    ui.add_enabled_ui(iterative, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            egui::Grid::new("backstage_options_iteration").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
                let label = ui.label(lang.tr("Maximum Iterations:"));
                let id = ui.make_persistent_id("backstage_max_iterations");
                // Text left in the field when iteration is switched off (it loses focus) is dropped.
                let committed = crate::widgets::committed_text(ui, id, &calc.max_iterations.to_string(), Some(80.0));
                if let Some(response) = ui.ctx().read_response(id) {
                    response.labelled_by(label.id);
                }
                if let Some(text) = committed.filter(|_| iterative) {
                    match parse_max_iterations(&text) {
                        Some(n) if n != calc.max_iterations => {
                            app.run_or_alert("formulas.calculationOptions", json!({"maxIterations": n}));
                        }
                        Some(_) => {}
                        None => refuse(app, ui, "Maximum Iterations must be a whole number from 1 to 32767."),
                    }
                }
                ui.end_row();
                let label = ui.label(lang.tr("Maximum Change:"));
                let id = ui.make_persistent_id("backstage_max_change");
                let committed = crate::widgets::committed_text(ui, id, &calc.max_change.to_string(), Some(80.0));
                if let Some(response) = ui.ctx().read_response(id) {
                    response.labelled_by(label.id);
                }
                if let Some(text) = committed.filter(|_| iterative) {
                    match parse_max_change(&text, lang.formula_locale()) {
                        Some(d) if d != calc.max_change => {
                            app.run_or_alert("formulas.calculationOptions", json!({"maxChange": d}));
                        }
                        Some(_) => {}
                        None => refuse(app, ui, "Maximum Change must be a number of 0 or more."),
                    }
                }
                ui.end_row();
            });
        });
    });
}

/// Refuses a typed setting with a message, leaving the workbook as it was. The Enter that
/// committed the field is consumed so it doesn't also dismiss the message box in this frame.
fn refuse(app: &mut SheetApp, ui: &mut Ui, text: &str) {
    ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    app.message = Some(("GridCraft".into(), app.ui.language.tr(text).into()));
}

/// Maximum Iterations as typed: a whole number from 1 to 32767, or `None` (Excel refuses others).
fn parse_max_iterations(text: &str) -> Option<u32> {
    let n = text.trim().parse::<u32>().ok()?;
    (1..=gridcraft_engine::model::CalcSettings::MAX_ITERATIONS).contains(&n).then_some(n)
}

/// Maximum Change as typed: a finite number of 0 or more, or `None`. The interface's decimal
/// separator is accepted as well as a dot.
fn parse_max_change(text: &str, locale: gridcraft_engine::formula::FormulaLocale) -> Option<f64> {
    let d = text.trim().replace(locale.decimal_separator(), ".").parse::<f64>().ok()?;
    (d.is_finite() && d >= 0.0).then_some(d)
}

/// The platform the app was built for, e.g. "macOS (aarch64)"; the web build reports "Web".
pub(super) fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        _ if cfg!(target_arch = "wasm32") => return "Web".into(),
        other => other,
    };
    format!("{os} ({})", std::env::consts::ARCH)
}
