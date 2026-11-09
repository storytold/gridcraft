//! Generated-name prefixes taken from every interface catalog must reach the commands that use them:
//! a translation with a character the shared validator rejects would block creating a workbook, a
//! sheet or a table in that language.

use gridcraft_engine::Session;
use gridcraft_ui_egui::i18n::Language;
use serde_json::json;

#[test]
fn every_language_generated_prefixes_are_accepted() {
    for language in Language::ALL {
        let book = language.tr("Book");
        let sheet = language.tr("Sheet");
        let column = language.tr("Generated|Column");
        let mut session = Session::new();
        if let Err(error) = session.execute("file.new", json!({"bookPrefix": book, "sheetPrefix": sheet})) {
            panic!("{language:?}: file.new rejected `{book}`/`{sheet}`: {error}");
        }
        let sheets: Vec<String> = session.doc().map(|doc| doc.wb.sheets.iter().map(|s| s.name.clone()).collect()).unwrap_or_default();
        assert!(!sheets.is_empty() && sheets.iter().all(|name| name.starts_with(sheet)), "{language:?}: {sheets:?} lacks `{sheet}`");
        if let Err(error) = session.execute("home.insertSheet", json!({"namePrefix": sheet})) {
            panic!("{language:?}: home.insertSheet rejected `{sheet}`: {error}");
        }
        session.execute("range.setValues", json!({"range": "A1", "values": [[1, 2]]})).unwrap();
        if let Err(error) = session.execute("insert.table", json!({"range": "A1:B1", "header": false, "columnPrefix": column})) {
            panic!("{language:?}: insert.table rejected `{column}`: {error}");
        }
        let columns: Vec<String> = session
            .doc()
            .ok()
            .and_then(|doc| {
                doc.wb.sheets.iter().flat_map(|sheet| sheet.tables.iter()).next().map(|t| t.columns.iter().map(|c| c.name.clone()).collect())
            })
            .unwrap_or_default();
        assert!(!columns.is_empty() && columns.iter().all(|name| name.starts_with(column)), "{language:?}: {columns:?} lacks `{column}`");
    }
}
