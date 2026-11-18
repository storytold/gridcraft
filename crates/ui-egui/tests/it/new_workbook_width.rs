//! File > Options sets the column width for future blank workbooks.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp, i18n::Language};
use serde_json::json;

/// English decimal input updates the new-workbook column width preference.
#[test]
fn options_width_field_applies_to_new_workbooks_only() {
    check_options_width_input(Language::En, "112.5");
}

/// Spanish comma-decimal input updates the new-workbook column width preference.
#[test]
fn options_width_field_accepts_spanish_decimal_separator() {
    check_options_width_input(Language::Es, "112,5");
}

/// German and French comma-decimal inputs are accepted via the UI decimal separator.
#[test]
fn options_width_field_accepts_german_and_french_decimal_separator() {
    for language in [Language::De, Language::Fr] {
        check_options_width_input(language, "112,5");
    }
}

/// Exercises the File › Options default-width input in `language` and checks blank-workbook behavior.
fn check_options_width_input(language: Language, input: &str) {
    let mut session = Session::new();
    let original = session.new_workbook();

    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1400.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::prepare(SheetApp::new(session, Services::default())),
    );

    h.run_steps(4);
    h.state_mut().run("app.language.set", json!({"language": language.code()})).unwrap();
    h.run_steps(3);
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);

    let label = language.tr("Default column width for new workbooks (pixels)");

    assert_eq!(h.get_by_role_and_label(Role::TextInput, label).value().as_deref(), Some("64"),);

    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);

    for c in input.chars() {
        h.input_mut().events.push(egui::Event::Text(c.to_string()));
        h.run_steps(2);
    }

    h.key_press(egui::Key::Enter);
    h.run_steps(3);

    assert_eq!(h.state().session.prefs.default_column_width, 112.5,);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, label).value().as_deref(), Some(input));

    assert_eq!(h.state().session.documents()[original].wb.sheets[0].default_col_width, 64.0,);

    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(3);

    assert_eq!(h.state().session.doc().unwrap().wb.active().unwrap().default_col_width, 112.5,);
}

/// German rejects grouping dots (`1.500`), and an untouched focused field does not overwrite a newer preference.
#[test]
fn options_width_rejects_german_thousands_dot_and_preserves_newer_pref_when_untouched() {
    let mut session = Session::new();
    session.new_workbook();

    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1400.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::prepare(SheetApp::new(session, Services::default())),
    );

    h.run_steps(4);
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.run_steps(3);
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);

    let label = Language::De.tr("Default column width for new workbooks (pixels)");

    // 1. Focus while showing 64, update preference externally to 120, press Enter without editing: keeps 120.
    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);
    h.state_mut().run("options.newWorkbookColumnWidth", json!({"width": 120.0})).unwrap();
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.prefs.default_column_width, 120.0, "untouched focused field must not overwrite newer preference");

    // 2. Typing `1.500` in German (comma-decimal locale) is rejected instead of saving 1.5 px.
    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    for c in "1.500".chars() {
        h.input_mut().events.push(egui::Event::Text(c.to_string()));
        h.run_steps(2);
    }
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.prefs.default_column_width, 120.0, "German grouping dot must be rejected");
    assert!(h.state().message.is_some(), "invalid German width must show validation error");
    h.key_press(egui::Key::Enter);
    h.run_steps(2);

    // 3. Re-typing the initial value (`64`) after an external update (`120`) commits `64`, while `-1e-50` is rejected.
    h.state_mut().run("options.newWorkbookColumnWidth", json!({"width": 64.0})).unwrap();
    h.run_steps(2);
    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);
    h.state_mut().run("options.newWorkbookColumnWidth", json!({"width": 120.0})).unwrap();
    h.run_steps(1);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    for c in "80".chars() {
        h.input_mut().events.push(egui::Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    for c in "64".chars() {
        h.input_mut().events.push(egui::Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.prefs.default_column_width, 64.0, "deliberate edit back to 64 must commit");

    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    for c in "-1e-50".chars() {
        h.input_mut().events.push(egui::Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().session.prefs.default_column_width, 64.0, "-1e-50 must be rejected before f32 rounding");
    assert!(h.state().message.is_some());
}
