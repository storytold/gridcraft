//! The interface language also picks the conventions numbers and dates are written with
//! (View ▸ Interface language): German shows German separators and month names, every other
//! language keeps en-US. Format codes and typed input stay en-US either way.
use gridcraft_engine::{Session, core::CellRef};
use gridcraft_ui_egui::{SheetApp, i18n::Language};
use serde_json::json;

fn app() -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    app.ui.language = Language::En;
    app
}

fn shown(app: &SheetApp, cell: CellRef) -> String {
    let d = app.session.active().unwrap();
    gridcraft_engine::display::value_text(&d.wb, d.wb.active().unwrap(), cell)
}

/// A workbook with one number in `#,##0.00` (A1) and one date in `dddd, mmmm d, yyyy` (B1).
fn sheet(app: &mut SheetApp) {
    app.run("range.setValues", json!({"range": "A1:B1", "values": [[1234.56, 46310.0]]})).unwrap();
    app.run("home.numberFormat", json!({"range": "A1", "code": "#,##0.00"})).unwrap();
    app.run("home.numberFormat", json!({"range": "B1", "code": "dddd, d. mmmm yyyy"})).unwrap();
}

#[test]
fn german_switches_number_and_date_output() {
    let mut app = app();
    sheet(&mut app);
    assert_eq!(shown(&app, CellRef::new(0, 0)), "1,234.56");
    assert_eq!(shown(&app, CellRef::new(0, 1)), "Thursday, 15. October 2026");
    app.run("app.language.set", json!({"language": "de"})).unwrap();
    assert_eq!(shown(&app, CellRef::new(0, 0)), "1.234,56");
    assert_eq!(shown(&app, CellRef::new(0, 1)), "Donnerstag, 15. Oktober 2026");
    // Back to English: the US conventions return.
    app.run("app.language.set", json!({"language": "en"})).unwrap();
    assert_eq!(shown(&app, CellRef::new(0, 0)), "1,234.56");
    assert_eq!(shown(&app, CellRef::new(0, 1)), "Thursday, 15. October 2026");
}

#[test]
fn other_languages_keep_the_en_us_conventions() {
    let mut app = app();
    sheet(&mut app);
    for language in ["es", "zh", "ja", "ko", "ru", "pt", "cs"] {
        app.run("app.language.set", json!({"language": language})).unwrap();
        assert_eq!(shown(&app, CellRef::new(0, 0)), "1,234.56", "{language}");
        assert_eq!(shown(&app, CellRef::new(0, 1)), "Thursday, 15. October 2026", "{language}");
    }
    // The app.language.english/japanese shortcuts set the locale too.
    app.run("app.language.set", json!({"language": "de"})).unwrap();
    app.run("app.language.japanese", json!({})).unwrap();
    assert_eq!(shown(&app, CellRef::new(0, 0)), "1,234.56");
    app.run("app.language.set", json!({"language": "de"})).unwrap();
    app.run("app.language.english", json!({})).unwrap();
    assert_eq!(shown(&app, CellRef::new(0, 0)), "1,234.56");
}

/// `TEXT()` reads its conventions from the workbook, so it follows the interface language as well
/// while the format code and the comma argument separator stay en-US.
#[test]
fn text_function_follows_the_interface_language() {
    let mut app = app();
    app.run("cell.set", json!({"cell": "A1", "input": "=TEXT(1234.5,\"#,##0.00\")"})).unwrap();
    assert_eq!(shown(&app, CellRef::new(0, 0)), "1,234.50");
    app.run("app.language.set", json!({"language": "de"})).unwrap();
    assert_eq!(shown(&app, CellRef::new(0, 0)), "1.234,50");
}
