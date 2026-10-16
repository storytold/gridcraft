//! Language selection must affect UI entry, never the canonical engine/file boundary.
use gridcraft_engine::{
    Session,
    core::{CellError, CellRef, Value},
};
use gridcraft_ui_egui::{SheetApp, UiState, i18n::Language};
use serde_json::json;

fn app() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = Language::En;
    app
}
fn formula(app: &SheetApp) -> String {
    app.session.active().unwrap().wb.active().unwrap().cell(CellRef::new(0, 0)).unwrap().formula.as_ref().unwrap().text.to_string()
}
fn enter(app: &mut SheetApp, text: &str) -> bool {
    app.begin_edit(Some(text.into()), false);
    app.commit_edit(0, 0, false, false)
}

#[test]
fn language_controls_entry_display_persistence_and_xlsx_roundtrip() {
    let mut app = app();
    assert!(!enter(&mut app, "=SUM(1;2)"));
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    assert!(enter(&mut app, "=SI(VERDADERO;SUMA(1,5;2,25);0)"));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    let saved = app.run("file.save", json!({"format":"xlsx"})).unwrap();
    app.run("file.open", json!({"name":"locale.xlsx", "base64":saved["base64"]})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SI(VERDADERO;SUMA(1,5;2,25);0)");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    let state: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert_eq!(state.language, Language::Es);
    app.run("app.language.set", json!({"language":"en"})).unwrap();
    app.begin_edit(None, false);
    assert_eq!(app.editor.as_ref().unwrap().text, "=IF(TRUE,SUM(1.5,2.25),0)");
}

#[test]
fn language_does_not_change_api_syntax_or_an_edit_already_in_progress() {
    let mut app = app();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    assert!(app.run("cell.set", json!({"cell":"A1","input":"=SUM(1,2)"})).is_ok());
    assert!(app.run("cell.set", json!({"cell":"A2","input":"=SUMA(1;2)"})).is_err());
    app.begin_edit(Some("=SUMA(1,5;2)".into()), false);
    app.run("app.language.set", json!({"language":"en"})).unwrap();
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "SUM(1.5,2)");
}

#[test]
fn defined_and_local_names_survive_localized_editing() {
    let mut app = app();
    app.run("formulas.defineName", json!({"name":"FALSO", "refersTo":"=7"})).unwrap();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    assert!(enter(&mut app, "=LET(SUMA;LAMBDA(x;x+1);SUMA(2))+FALSO"));
    let original = formula(&app);
    app.begin_edit(None, false);
    assert_eq!(app.editor.as_ref().unwrap().text, "=LET(SUMA;LAMBDA(x;x+1);SUMA(2))+FALSO");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), original);
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(10.0));
}

#[test]
fn localized_argument_hints_ignore_decimal_commas_arrays_and_nested_calls() {
    let mut app = app();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    for (text, expected) in [("=SI(1,5;SUMA(2;3);", 2), ("=SUMA({1,5\\2;3\\4};", 1), ("=SUMA(Table1[[a],[b]];", 1)] {
        app.begin_edit(Some(text.into()), false);
        assert_eq!(app.editor.as_ref().unwrap().current_function().unwrap().1, expected);
    }
}

#[test]
fn localized_autocomplete_is_rendered_and_inserts_the_selected_name() {
    let mut app = app();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
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
    assert!(h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUMA"));
    assert!(!h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUM"));
    if let Ok(path) = std::env::var("GRIDCRAFT_LOCALE_SCREENSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    let ed = h.state_mut().editor.as_mut().unwrap();
    ed.ac_index = ed.autocomplete.iter().position(|n| n == "SUMA").unwrap();
    assert!(ed.accept_autocomplete());
    assert_eq!(ed.text, "=SUMA(");
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("1,5;".into()));
    h.run_steps(4);
    assert_eq!(h.state().editor.as_ref().unwrap().current_function(), Some(("SUMA".into(), 1)));
    if let Ok(path) = std::env::var("GRIDCRAFT_LOCALE_SCREENSHOT") {
        h.render().unwrap().save(std::path::Path::new(&path).with_file_name("spanish-hints.png")).unwrap();
        h.state_mut().cancel_edit();
        h.state_mut().ui.ribbon_tab = "View".into();
        h.run_steps(4);
        h.render().unwrap().save(std::path::Path::new(&path).with_file_name("spanish-setting.png")).unwrap();
    }
    h.state_mut().cancel_edit();
    h.state_mut().run("app.language.set", json!({"language":"en"})).unwrap();
    h.state_mut().begin_edit(Some("=SU".into()), true);
    h.run_steps(4);
    h.input_mut().events.push(egui::Event::Text("M".into()));
    h.run_steps(4);
    assert!(h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUM"));
    assert!(!h.state().editor.as_ref().unwrap().autocomplete.iter().any(|n| n == "SUMA"));
}

#[test]
fn locale_preserves_text_cells_and_existing_entry_conveniences() {
    let mut app = app();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    assert!(enter(&mut app, "=SUMA(1,5;2"));
    assert_eq!(formula(&app), "SUM(1.5,2)");
    assert!(enter(&mut app, "-1,5"));
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(-1.5));
    app.run("home.numberFormat", json!({"format":"@"})).unwrap();
    assert!(enter(&mut app, "=SUM(1,2)"));
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::text("=SUM(1,2)"));
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(1,2)");
}

#[test]
fn canonical_engine_templates_enter_the_selected_formula_language() {
    let mut app = app();
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    app.run("formulas.insertFunction", json!({"name":"SUM"})).unwrap();
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUMA(");
    app.cancel_edit();
    app.run("cell.set", json!({"cell":"A1","input":"2"})).unwrap();
    app.run("selection.set", json!({"range":"A2"})).unwrap();
    app.run("formulas.autoSum", json!({"enter":false})).unwrap();
    assert!(app.editor.as_ref().unwrap().text.starts_with("=SUMA("));
    app.cancel_edit();
    // Text supplied by the user is already in the selected UI syntax; do not translate it twice.
    app.begin_edit(Some("=SUMA(1,5;2)".into()), false);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUMA(1,5;2)");
    assert!(app.commit_edit(0, 0, false, false));
    if let Ok(path) = std::env::var("GRIDCRAFT_LOCALE_SCREENSHOT") {
        app.ui.ribbon_tab = "Formulas".into();
        app.run("formulas.insertFunction", json!({"name":"SUM"})).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(5);
        h.render().unwrap().save(std::path::Path::new(&path).with_file_name("spanish-ribbon-template.png")).unwrap();
    }
}

#[test]
fn portuguese_interface_retains_its_canonical_formula_fallback() {
    let mut app = app();
    app.run("app.language.set", json!({"code":"pt-BR"})).unwrap();
    assert_eq!(app.ui.language, Language::PtBr);
    assert_eq!(app.ui.language.tr("Data"), "Dados");
    assert!(enter(&mut app, "=SUM(1.5,2.25)"));
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(1.5,2.25)");
    assert!(!enter(&mut app, "=SUMA(1;2)"));
    let restored: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert_eq!(restored.language, Language::PtBr);
    assert_eq!(Language::Es.tr("Page Layout|Orientation"), "Orientación");
}

#[test]
fn czech_interface_retains_its_canonical_formula_fallback() {
    // The Czech table translates interface labels only: formulas keep canonical English names,
    // `,` separators and `.` decimals until a Czech formula locale exists.
    let mut app = app();
    app.run("app.language.set", json!({"code":"cs-CZ"})).unwrap();
    assert_eq!(app.ui.language, Language::Cs);
    assert_eq!(app.ui.language.tr("Formulas"), "Vzorce");
    assert!(enter(&mut app, "=SUM(1.5,2.25)"));
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(1.5,2.25)");
    assert!(!enter(&mut app, "=SUMA(1;2)"));
    let restored: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert_eq!(restored.language, Language::Cs);
}

#[test]
fn user_defined_prefix_is_hidden_while_editing_and_kept_on_commit() {
    // Excel shows `_xludf.EOMONTH()` as `=EOMONTH()`; the cell still holds the user-defined call
    // (#726), and committing the edit unchanged must not turn it into the built-in.
    let mut app = app();
    app.run("cell.set", json!({"cell":"A1","input":"=_xludf.EOMONTH()&_xludf.CONCAT(\"_xludf.x\")"})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=EOMONTH()&CONCAT(\"_xludf.x\")");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "_xludf.EOMONTH()&_xludf.CONCAT(\"_xludf.x\")");
    assert_eq!(app.session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Error(CellError::Name));
    app.run("app.language.set", json!({"language":"es"})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=EOMONTH()&CONCAT(\"_xludf.x\")");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "_xludf.EOMONTH()&_xludf.CONCAT(\"_xludf.x\")");
}

#[test]
fn turkish_interface_uses_localized_formulas_and_xlsx_roundtrip() {
    let mut app = app();
    app.run("app.language.set", json!({"code":"tr-TR"})).unwrap();
    assert_eq!(app.ui.language, Language::Tr);
    assert_eq!(app.ui.language.tr("Data"), "Veri");
    assert_eq!(app.ui.language.tr("Formulas"), "Formüller");
    assert_eq!(app.ui.language.tr("Formula Builder"), "Formül Oluşturucu");
    assert_eq!(app.ui.language.tr("Insert Function"), "Fonksiyon Ekle");
    assert_eq!(app.ui.language.tr("Enter"), "Gir");
    assert_eq!(app.ui.language.tr("Search:"), "Ara:");
    assert_eq!(app.ui.language.tr("e.g. lookup, average, date"), "örn. arama, ortalama, tarih");
    assert_eq!(app.ui.language.tr("MathTrig"), "Matematik ve Trigonometri");
    assert_eq!(app.ui.language.tr("Financial"), "Finansal");
    assert!(enter(&mut app, "=BAĞ_DEĞ_SAY(A1:A10)"));
    assert_eq!(formula(&app), "COUNT(A1:A10)");
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=BAĞ_DEĞ_SAY(A1:A10)");
    app.cancel_edit();
    assert!(enter(&mut app, "=SAY(A1:A10)"));
    assert_eq!(formula(&app), "COUNT(A1:A10)");
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=BAĞ_DEĞ_SAY(A1:A10)");
    app.cancel_edit();
    assert!(enter(&mut app, "=DEVRESEL_ÖDEME(0,05/12;60;-10000)"));
    assert_eq!(formula(&app), "PMT(0.05/12,60,-10000)");
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=DEVRESEL_ÖDEME(0,05/12;60;-10000)");
    app.cancel_edit();
    assert!(enter(&mut app, "=EĞER(DOĞRU;TOPLA(1,5;2,25);0)"));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    let saved = app.run("file.save", json!({"format":"xlsx"})).unwrap();
    app.run("file.open", json!({"name":"locale_tr.xlsx", "base64":saved["base64"]})).unwrap();
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=EĞER(DOĞRU;TOPLA(1,5;2,25);0)");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(formula(&app), "IF(TRUE,SUM(1.5,2.25),0)");
    let restored: UiState = serde_json::from_str(&serde_json::to_string(&app.ui).unwrap()).unwrap();
    assert_eq!(restored.language, Language::Tr);
}

#[test]
fn turkish_error_messages_and_error_codes_localized() {
    let mut app = app();
    app.run("app.language.set", json!({"code":"tr-TR"})).unwrap();
    assert_eq!(app.ui.language, Language::Tr);

    // Syntax error in formula shows localized error message in alert box
    app.begin_edit(Some("=TOPLA(1 2)".into()), false);
    assert!(!app.commit_edit(0, 0, false, false));
    let msg = app.message.as_ref().unwrap();
    assert_eq!(msg.0, "GridCraft");
    assert!(msg.1.contains("bekleniyordu"), "Message should be Turkish: {}", msg.1);

    // Unterminated string syntax error
    app.begin_edit(Some("=\"merhaba".into()), false);
    assert!(!app.commit_edit(0, 0, false, false));
    let msg = app.message.as_ref().unwrap();
    assert_eq!(msg.1, "Kapatılmamış metin dizesi (tırnak işareti eksik)");

    // Cell error literal in formula
    app.cancel_edit();
    assert!(enter(&mut app, "=#BAŞV!"));
    assert_eq!(formula(&app), "#REF!");
    app.begin_edit(None, true);
    assert_eq!(app.editor.as_ref().unwrap().text, "=#BAŞV!");
    app.cancel_edit();

    // Division by zero evaluated cell error displays #SAYI/0!
    assert!(enter(&mut app, "=1/0"));
    assert_eq!(formula(&app), "1/0");

    // Check status bar and common alert translations
    assert_eq!(app.ui.language.tr("OK"), "Tamam");
    assert_eq!(app.ui.language.tr("Circular References"), "Döngüsel Başvurular");
    assert_eq!(app.ui.language.tr("Macros preserved (not executed)"), "Makrolar korundu (çalıştırılmadı)");
    assert_eq!(app.ui.language.tr("Spill range is not empty"), "Taşma aralığı boş değil, formülün yayılacağı hücreleri temizleyin.");
    assert_eq!(app.ui.language.tr("The name you entered is not valid."), "Girdiğiniz ad geçerli değil.");
    assert_eq!(app.ui.language.tr("Reference is not valid."), "Başvuru geçerli değil.");
}
