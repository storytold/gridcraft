//! The formula language and region affect what a person types and reads, never the canonical
//! text the engine, the API and files use.
use gridcraft_engine::{
    Session,
    core::{CellRef, Value},
};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn app() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    english(&mut app);
    app
}

/// A Spanish interface (function names follow it, as in Excel) with Spain's separators.
fn spanish(app: &mut SheetApp) {
    app.run("app.language.set", json!({"language": "es"})).unwrap();
    app.run("app.setLocale", json!({"formulaLanguage": "followUi", "regionalFormat": "es-ES"})).unwrap();
}

fn english(app: &mut SheetApp) {
    app.run("app.language.set", json!({"language": "en"})).unwrap();
    app.run("app.setLocale", json!({"formulaLanguage": "followUi", "regionalFormat": "en-US"})).unwrap();
}

fn formula(app: &SheetApp) -> String {
    app.session.active().unwrap().wb.active().unwrap().cell(CellRef::new(0, 0)).unwrap().formula.as_ref().unwrap().text.to_string()
}

fn value(app: &SheetApp) -> Value {
    app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0))
}

fn editor_text(app: &SheetApp) -> &str {
    &app.editor.as_ref().unwrap().text
}

fn enter(app: &mut SheetApp, text: &str) -> bool {
    app.begin_edit(Some(text.into()), false);
    app.commit_edit(0, 0, false, false)
}

#[test]
fn language_controls_entry_display_and_xlsx_roundtrip() {
    let mut app = app();
    assert!(!enter(&mut app, "=SUM(1;2)"));
    spanish(&mut app);
    assert!(enter(&mut app, "=SI(VERDADERO;SUMA(1,5;2,25);0)"));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    let saved = app.run("file.save", json!({"format": "xlsx"})).unwrap();
    app.run("file.open", json!({"name": "locale.xlsx", "base64": saved["base64"]})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(editor_text(&app), "=SI(VERDADERO;SUMA(1,5;2,25);0)");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    english(&mut app);
    app.begin_edit(None, false);
    assert_eq!(editor_text(&app), "=IF(TRUE,SUM(1.5,2.25),0)");
}

#[test]
fn language_does_not_change_api_syntax_and_rewrites_an_edit_in_progress() {
    let mut app = app();
    spanish(&mut app);
    assert!(app.run("cell.set", json!({"cell": "A1", "input": "=SUM(1,2)"})).is_ok());
    assert!(app.run("cell.set", json!({"cell": "A2", "input": "=SUMA(1;2)"})).is_err());
    app.begin_edit(Some("=SUMA(1,5;2)".into()), false);
    english(&mut app);
    assert_eq!(editor_text(&app), "=SUM(1.5,2)");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "SUM(1.5,2)");
}

#[test]
fn defined_and_local_names_survive_localized_editing() {
    let mut app = app();
    app.run("formulas.defineName", json!({"name": "FALSO", "refersTo": "=7"})).unwrap();
    spanish(&mut app);
    assert!(enter(&mut app, "=LET(SUMA;LAMBDA(x;x+1);SUMA(2))+FALSO"));
    let original = formula(&app);
    assert_eq!(original, "LET(SUMA,LAMBDA(x,x+1),SUMA(2))+FALSO");
    app.begin_edit(None, false);
    assert_eq!(editor_text(&app), "=LET(SUMA;LAMBDA(x;x+1);SUMA(2))+FALSO");
    // Switching languages while editing keeps the names names.
    english(&mut app);
    assert_eq!(editor_text(&app), "=LET(SUMA,LAMBDA(x,x+1),SUMA(2))+FALSO");
    spanish(&mut app);
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), original);
    assert_eq!(value(&app), Value::Number(10.0));
}

#[test]
fn localized_argument_hints_ignore_decimal_commas_arrays_and_nested_calls() {
    let mut app = app();
    spanish(&mut app);
    for (text, expected) in [("=SI(1,5;SUMA(2;3);", 2), ("=SUMA({1,5\\2;3\\4};", 1), ("=SUMA(Table1[[a],[b]];", 1)] {
        app.begin_edit(Some(text.into()), false);
        assert_eq!(app.editor.as_ref().unwrap().current_function().unwrap().1, expected, "{text}");
    }
}

#[test]
fn localized_autocomplete_is_rendered_and_inserts_the_selected_name() {
    let mut app = app();
    spanish(&mut app);
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h.state_mut().begin_edit(Some("=SU".into()), true);
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("M".into()));
    h.run_steps(4);
    let offered = |h: &egui_kittest::Harness<SheetApp>, n: &str| h.state().editor.as_ref().unwrap().autocomplete.iter().any(|x| x == n);
    assert!(offered(&h, "SUMA"));
    assert!(!offered(&h, "SUM"));
    let ed = h.state_mut().editor.as_mut().unwrap();
    ed.ac_index = ed.autocomplete.iter().position(|n| n == "SUMA").unwrap();
    assert!(ed.accept_autocomplete());
    assert_eq!(ed.text, "=SUMA(");
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("1,5;".into()));
    h.run_steps(4);
    assert_eq!(h.state().editor.as_ref().unwrap().current_function(), Some(("SUMA".into(), 1)));
    h.state_mut().cancel_edit();
    english(h.state_mut());
    h.state_mut().begin_edit(Some("=SU".into()), true);
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("M".into()));
    h.run_steps(4);
    assert!(offered(&h, "SUM"));
    assert!(!offered(&h, "SUMA"));
}

#[test]
fn locale_preserves_text_cells_and_existing_entry_conveniences() {
    let mut app = app();
    spanish(&mut app);
    assert!(enter(&mut app, "=SUMA(1,5;2"));
    assert_eq!(formula(&app), "SUM(1.5,2)");
    assert!(enter(&mut app, "-1,5"));
    assert_eq!(value(&app), Value::Number(-1.5));
    app.run("home.numberFormat", json!({"code": "@"})).unwrap();
    assert!(enter(&mut app, "=SUM(1,2)"));
    assert_eq!(value(&app), Value::text("=SUM(1,2)"));
    app.begin_edit(None, true);
    assert_eq!(editor_text(&app), "=SUM(1,2)");
}

#[test]
fn canonical_engine_templates_enter_the_selected_formula_language() {
    let mut app = app();
    spanish(&mut app);
    app.run("formulas.insertFunction", json!({"name": "SUM"})).unwrap();
    assert_eq!(editor_text(&app), "=SUMA(");
    app.cancel_edit();
    app.run("cell.set", json!({"cell": "A1", "input": "2"})).unwrap();
    app.run("selection.set", json!({"range": "A2"})).unwrap();
    app.run("formulas.autoSum", json!({"enter": false})).unwrap();
    assert!(editor_text(&app).starts_with("=SUMA("), "{}", editor_text(&app));
    app.cancel_edit();
    // Text the user supplies is already in the selected syntax; it is not translated twice.
    app.begin_edit(Some("=SUMA(1,5;2)".into()), false);
    assert_eq!(editor_text(&app), "=SUMA(1,5;2)");
    assert!(app.commit_edit(0, 0, false, false));
}

#[test]
fn portuguese_interface_writes_portuguese_formulas() {
    let mut app = app();
    app.run("app.language.set", json!({"code": "pt-BR"})).unwrap();
    app.run("app.setLocale", json!({"regionalFormat": "pt-BR"})).unwrap();
    assert_eq!(app.session.prefs.ui_language, "pt-BR");
    assert!(enter(&mut app, "=SOMA(1,5;2,25)"));
    assert_eq!(formula(&app), "SUM(1.5,2.25)");
    app.begin_edit(None, true);
    assert_eq!(editor_text(&app), "=SOMA(1,5;2,25)");
    // Spanish names are not Portuguese ones.
    assert!(enter(&mut app, "=SUMA(1;2)"));
    assert_eq!(value(&app), Value::Error(gridcraft_engine::core::CellError::Name));
}
