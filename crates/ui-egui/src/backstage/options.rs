use super::{Key, Language, Modifiers, SheetApp, Ui, dim, i18n, json, section, vec2};

/// Excel's File › Options page: General (display theme, interface language and Show formula bar),
/// Editing (AutoComplete) and Save (the autosave checkbox toggling `app.ui.autosave`), plus Formulas
/// (calculation mode and iterative calculation) and the display options for this workbook (sheet
/// tabs and scroll bars) when a document is active. Theme, language, formula
/// bar, AutoComplete and calculation changes run their commands through `run_or_alert`; calculation
/// settings are saved with the workbook.
pub(super) fn options(app: &mut SheetApp, ui: &mut Ui, bs: &mut super::Backstage) {
    let lang = app.ui.language;
    section(ui, lang, "Options|General");
    general_options(app, ui, lang);
    custom_lists(app, ui, bs);
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
    section(ui, lang, "Formulas");
    ui.label(lang.tr("Workbook calculation"));
    calc_options(app, ui, lang, &calc);
    ui.add_space(6.0);
    dim(ui, lang.tr("Calculation settings are saved with the workbook."));
}

fn custom_lists(app: &mut SheetApp, ui: &mut Ui, bs: &mut super::Backstage) {
    ui.add_space(18.0);
    section(ui, app.ui.language, "Custom Lists");
    let lists = &app.session.prefs.custom_lists;
    if bs.custom_list_index >= lists.len() {
        bs.custom_list_index = lists.len().saturating_sub(1);
    }
    if !lists.is_empty() {
        craft_ui::ComboBox::from_id_salt("backstage_custom_list").selected_text(format!("Custom List {}", bs.custom_list_index + 1)).show_ui(
            ui,
            |ui| {
                for (i, items) in lists.iter().enumerate() {
                    if ui.selectable_label(bs.custom_list_index == i, items.first().map(String::as_str).unwrap_or("Custom List")).clicked() {
                        bs.custom_list_index = i;
                        bs.custom_list_draft = custom_list_to_draft(items);
                        bs.custom_list_draft_initialized = true;
                    }
                }
            },
        );
        if !bs.custom_list_draft_initialized {
            bs.custom_list_draft = lists.get(bs.custom_list_index).map(|l| custom_list_to_draft(l)).unwrap_or_default();
            bs.custom_list_draft_initialized = true;
        }
    }
    let label = ui.label("Enter one item per line. Use \\n for a line break within an item:");
    ui.add(egui::TextEdit::multiline(&mut bs.custom_list_draft).desired_rows(6).desired_width(420.0)).labelled_by(label.id);
    ui.horizontal(|ui| {
        if ui.button("Add").clicked() {
            let before = app.session.prefs.custom_lists.len();
            app.run_or_alert("options.addCustomList", json!({"items": custom_list_items_from_draft(&bs.custom_list_draft)}));
            if app.session.prefs.custom_lists.len() > before {
                bs.custom_list_index = app.session.prefs.custom_lists.len() - 1;
            }
        }
        if ui.add_enabled(!app.session.prefs.custom_lists.is_empty(), egui::Button::new("Replace")).clicked() {
            app.run_or_alert(
                "options.replaceCustomList",
                json!({"index": bs.custom_list_index + 1, "items": custom_list_items_from_draft(&bs.custom_list_draft)}),
            );
        }
        if ui.add_enabled(!app.session.prefs.custom_lists.is_empty(), egui::Button::new("Delete")).clicked() {
            app.run_or_alert("options.deleteCustomList", json!({"index": bs.custom_list_index + 1}));
            bs.custom_list_index = bs.custom_list_index.min(app.session.prefs.custom_lists.len().saturating_sub(1));
            bs.custom_list_draft = app.session.prefs.custom_lists.get(bs.custom_list_index).map(|l| custom_list_to_draft(l)).unwrap_or_default();
            bs.custom_list_draft_initialized = true;
        }
        if ui.add_enabled(app.session.active().is_some(), egui::Button::new("Import Selection")).clicked() {
            let before = app.session.prefs.custom_lists.len();
            app.run_or_alert("options.importCustomList", json!({}));
            if app.session.prefs.custom_lists.len() > before {
                bs.custom_list_index = app.session.prefs.custom_lists.len() - 1;
                bs.custom_list_draft = custom_list_to_draft(&app.session.prefs.custom_lists[bs.custom_list_index]);
                bs.custom_list_draft_initialized = true;
            }
        }
    });
}

/// Encode item boundaries for the line-based Custom Lists editor without losing embedded newlines.
fn custom_list_to_draft(items: &[String]) -> String {
    items.iter().map(|item| item.replace('\\', "\\\\").replace('\n', "\\n")).collect::<Vec<_>>().join("\n")
}

/// Decode the editor's `\\n` and `\\\\` escapes while keeping other backslashes literal.
fn custom_list_items_from_draft(draft: &str) -> Vec<String> {
    draft
        .lines()
        .map(|line| {
            let mut item = String::with_capacity(line.len());
            let mut chars = line.chars();
            while let Some(ch) = chars.next() {
                if ch != '\\' {
                    item.push(ch);
                    continue;
                }
                match chars.next() {
                    Some('n') => item.push('\n'),
                    Some('\\') => item.push('\\'),
                    Some(other) => {
                        item.push('\\');
                        item.push(other);
                    }
                    None => item.push('\\'),
                }
            }
            item
        })
        .collect()
}

#[cfg(test)]
mod custom_list_editor_tests {
    use super::{custom_list_items_from_draft, custom_list_to_draft};

    #[test]
    fn custom_list_editor_preserves_embedded_newlines_and_backslashes() {
        let items = vec!["first\nsecond".to_owned(), r"literal \n sequence".to_owned(), "trailing\\".to_owned()];
        assert_eq!(custom_list_items_from_draft(&custom_list_to_draft(&items)), items);
    }
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
