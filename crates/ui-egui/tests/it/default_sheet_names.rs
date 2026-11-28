//! New workbooks and sheets are named in the interface language (Pasta1 and Planilha1 in
//! Portuguese), as the menus are; a workbook already open keeps its names.

use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

#[test]
fn a_new_workbook_is_named_in_the_interface_language() {
    let mut h = crate::support::blank_harness(4);
    let first = h.state().session.active().unwrap().title.clone();
    // No frame in between: the language command itself updates the names, so a command later in
    // the same batch (control channel, MCP) already uses them.
    h.state_mut().run("app.language.set", json!({"language": "pt"})).unwrap();
    let (book, sheet) = (Language::PtBr.tr("Default name|Book"), Language::PtBr.tr("Default name|Sheet"));
    assert!(book != "Book" && sheet != "Sheet", "Portuguese has its own words: {book}, {sheet}");
    h.state_mut().run("file.new", json!({})).unwrap();
    h.state_mut().run("home.insertSheet", json!({})).unwrap();
    h.run_steps(2);
    let d = h.state().session.active().unwrap();
    assert!(d.title.starts_with(book), "{}", d.title);
    assert!(d.wb.sheets.iter().all(|sh| sh.name.starts_with(sheet)), "{:?}", d.wb.sheets.iter().map(|sh| &sh.name).collect::<Vec<_>>());
    let docs = h.state().session.documents();
    assert!(docs.iter().any(|d| d.title == first), "the workbook already open keeps its name");
}
