//! Public-command regressions for the additional insertion review findings.
use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_model::StyleId;
use serde_json::json;

/// Create an empty workbook for public-command regressions.
fn book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// Read the effective style, including inherited row or column formatting.
fn style(s: &Session, address: &str) -> StyleId {
    s.doc().unwrap().wb.active().unwrap().style_id(CellRef::parse(address).unwrap())
}

#[test]
/// A recycled snapshot style must not round away copied digits before the final format is restored.
fn copied_number_precision_survives_style_id_reuse_before_paste_commit() {
    for command in ["home.insertCopiedCells", "edit.pasteSpecial"] {
        let mut s = book();
        s.execute("cell.set", json!({"cell":"B2","input":"1.23456789"})).unwrap();
        s.execute("home.numberFormat", json!({"range":"B2","code":"0.00000000"})).unwrap();
        let copied_style = style(&s, "B2");
        s.execute("edit.copy", json!({"range":"B2"})).unwrap();
        s.execute("edit.undo", json!({})).unwrap();
        s.execute("home.numberFormat", json!({"range":"D4","code":"0"})).unwrap();
        assert_eq!(style(&s, "D4"), copied_style, "the live catalog reused the clipboard's numeric style id");
        let option = s.execute("formulas.calculationOptions", json!({"precisionAsDisplayed":true})).unwrap();
        assert_eq!(option["precisionAsDisplayed"], true);
        s.execute(command, json!({"at":"B5","what":"all"})).unwrap();
        let number = |s: &Session| s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("B5").unwrap());
        assert_eq!(number(&s), Value::Number(1.23456789), "{command} must map styles before precision-as-displayed commits");
        assert_eq!(s.doc().unwrap().wb.styles.get(style(&s, "B5")).num_fmt.as_str(), "0.00000000");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(number(&s), Value::Empty);
        s.execute("edit.redo", json!({})).unwrap();
        assert_eq!(number(&s), Value::Number(1.23456789));
    }
}

#[test]
/// Empty typed input and empty matrix strings retain default overrides, without storing ordinary blanks.
fn clearing_input_keeps_an_explicit_default_over_inherited_formats() {
    for (line, cell) in [("3:3", "B3"), ("C:C", "C2")] {
        for (command, params) in [
            ("cell.set", json!({"cell":cell,"input":""})),
            ("range.setValues", json!({"range":cell,"values":[[""]]})),
            ("range.setValues", json!({"range":cell,"values":[[""]],"parse":false})),
        ] {
            let mut s = book();
            s.execute("home.bold", json!({"range":line})).unwrap();
            s.execute("cell.set", json!({"cell":cell,"input":"7"})).unwrap();
            s.execute("edit.clearFormats", json!({"range":cell})).unwrap();
            assert_eq!(style(&s, cell), StyleId::DEFAULT);
            s.execute(command, params).unwrap();
            assert_eq!(style(&s, cell), StyleId::DEFAULT, "{command} must preserve the explicit reset");
            assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap()), Value::Empty);
            for format in ["xlsx", "json"] {
                let bytes = s.execute("file.saveBytes", json!({"format":format})).unwrap();
                let mut loaded = Session::new();
                loaded.execute("file.open", json!({"name":format!("book.{format}"),"base64":bytes["base64"]})).unwrap();
                assert_eq!(style(&loaded, cell), StyleId::DEFAULT, "{format} must retain the empty reset");
            }
        }
    }
    let mut s = book();
    s.execute("cell.set", json!({"cell":"Z9","input":""})).unwrap();
    s.execute("range.setValues", json!({"range":"Y8","values":[[""]]})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().cells.len(), 0, "ordinary empty inputs must remain sparse");
}

#[test]
/// A true cut moves live cells and must not reinterpret their styles through an older copy catalog.
fn a_cut_uses_the_live_style_after_undo_and_reformatting() {
    let mut s = book();
    s.execute("cell.set", json!({"cell":"B2","input":"1.23456789"})).unwrap();
    s.execute("home.numberFormat", json!({"range":"B2","code":"0.00000000"})).unwrap();
    s.execute("edit.cut", json!({"range":"B2"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("home.numberFormat", json!({"range":"B2","code":"0"})).unwrap();
    s.execute("edit.pasteSpecial", json!({"at":"B5","what":"all"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.styles.get(style(&s, "B5")).num_fmt.as_str(), "0");
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("B2").unwrap()), Value::Empty);
}
