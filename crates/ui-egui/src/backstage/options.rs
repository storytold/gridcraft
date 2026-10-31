use super::{Key, Language, Modifiers, RichText, SheetApp, Tokens, Ui, dim, i18n, json, section, theme, vec2};

/// Excel's File › Options page: General (display theme, interface language, Show formula bar, and
/// the font, font size and number of sheets of new workbooks), Editing (AutoComplete) and Save (the
/// autosave checkbox toggling `app.ui.autosave`), plus Formulas (calculation mode and iterative
/// calculation) when a document is active. Theme, language, formula bar, new-workbook, AutoComplete
/// and calculation changes run their commands through `run_or_alert`; calculation settings are
/// saved with the workbook.
pub(super) fn options(app: &mut SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    section(ui, lang, "Options|General");
    general_options(app, ui, lang);
    ui.add_space(6.0);
    let mut formula_bar = app.session.prefs.formula_bar;
    if ui.checkbox(&mut formula_bar, lang.tr("Show formula bar")).changed() {
        app.run_or_alert("view.formulaBar", json!({"on": formula_bar}));
    }
    ui.add_space(14.0);
    new_workbook_options(app, ui, lang);
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
    let Some(calc) = app.session.active().map(|d| d.wb.calc.clone()) else { return };
    ui.add_space(18.0);
    section(ui, lang, "Formulas");
    ui.label(lang.tr("Workbook calculation"));
    calc_options(app, ui, lang, &calc);
    ui.add_space(6.0);
    dim(ui, lang.tr("Calculation settings are saved with the workbook."));
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

/// The font sizes the Font size list offers, as the Home tab's size list does.
const FONT_SIZES: [f32; 16] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 22.0, 24.0, 26.0, 28.0, 36.0, 48.0, 72.0];

/// The font, font size and number of sheets of new workbooks (`options.newWorkbooks`). The next
/// new workbook takes a change at once; open workbooks keep their own font.
fn new_workbook_options(app: &mut SheetApp, ui: &mut Ui, lang: Language) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(lang.tr("New workbooks")).font(theme::ui_bold(13.0)).color(t.text));
    ui.add_space(6.0);
    let font = app.session.prefs.new_workbook_font();
    let sheets = app.session.prefs.new_workbook_sheets();
    egui::Grid::new("backstage_options_new_workbooks").num_columns(2).spacing(vec2(18.0, 10.0)).show(ui, |ui| {
        let label = ui.label(lang.tr("Use this as the default font"));
        // The Home tab's standard fonts first, then the installed ones, as Format Cells lists them.
        craft_ui::ComboBox::from_id_salt("backstage_default_font")
            .width(200.0)
            .selected_text(font.name.as_str())
            .show_ui(ui, |ui| {
                for name in crate::fonts::families() {
                    if ui.selectable_label(name == font.name, name.as_str()).clicked() && name != font.name {
                        app.run_or_alert("options.newWorkbooks", json!({"font": name}));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();
        let label = ui.label(lang.tr("Font size"));
        craft_ui::ComboBox::from_id_salt("backstage_default_font_size")
            .width(80.0)
            .selected_text(size_text(font.size))
            .show_ui(ui, |ui| {
                for size in FONT_SIZES {
                    if ui.selectable_label(size == font.size, size_text(size)).clicked() && size != font.size {
                        app.run_or_alert("options.newWorkbooks", json!({"fontSize": size}));
                    }
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();
        let label = ui.label(lang.tr("Include this many sheets"));
        let id = ui.make_persistent_id("backstage_new_workbook_sheets");
        let committed = crate::widgets::committed_text(ui, id, &sheets.to_string(), Some(80.0));
        if let Some(response) = ui.ctx().read_response(id) {
            response.labelled_by(label.id);
        }
        if let Some(text) = committed {
            match parse_sheets(&text) {
                Some(n) if n != sheets => {
                    app.run_or_alert("options.newWorkbooks", json!({"sheets": n}));
                }
                Some(_) => {}
                None => refuse(app, ui, "The number of sheets must be a whole number from 1 to 255."),
            }
        }
        ui.end_row();
    });
    ui.add_space(4.0);
    dim(ui, lang.tr("These settings apply to workbooks created from now on."));
}

/// A font size as the size boxes show it: `11`, `10.5`.
fn size_text(size: f32) -> String {
    if size.fract() == 0.0 { format!("{}", size as i32) } else { format!("{size}") }
}

/// Include this many sheets as typed: a whole number from 1 to 255, or `None`.
fn parse_sheets(text: &str) -> Option<usize> {
    let n = text.trim().parse::<usize>().ok()?;
    (1..=gridcraft_engine::cmd::file::MAX_NEW_SHEETS).contains(&n).then_some(n)
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
