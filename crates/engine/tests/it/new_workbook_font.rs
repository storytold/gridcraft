//! The font, font size and number of sheets of new workbooks: File › Options › General and
//! `options.newWorkbooks`, kept in prefs.json (#3017). Opened workbooks keep their own Normal style.

use std::io::Read;

use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_engine::model::{Font, Style, StyleId, StyleTable, Workbook};
use gridcraft_engine::{Prefs, Session, io};
use serde_json::json;

fn styles_xml(wb: &Workbook) -> String {
    let bytes = gridcraft_xlsx::write_xlsx(wb).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("xl/styles.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

/// The elements of the `list` part of `xml` that start with `open`, in order, each up to the next.
fn items(xml: &str, list: &str, open: &str) -> Vec<String> {
    let part = xml.split(&format!("<{list}")).nth(1).unwrap().split(&format!("</{list}>")).next().unwrap();
    part.split(open).skip(1).map(str::to_string).collect()
}

fn normal_font(s: &Session) -> Font {
    s.doc().unwrap().wb.styles.get(StyleId::DEFAULT).font.clone()
}

fn cell_style(s: &Session, a1: &str) -> StyleId {
    s.doc().unwrap().wb.active().unwrap().style_id(CellRef::parse(a1).unwrap())
}

fn cell_font(s: &Session, a1: &str) -> Font {
    s.doc().unwrap().wb.styles.get(cell_style(s, a1)).font.clone()
}

fn name_and_size(f: &Font) -> (&str, f32) {
    (f.name.as_str(), f.size)
}

fn settings(s: &Session) -> (String, f32, usize) {
    (s.prefs.default_font.clone(), s.prefs.default_font_size, s.prefs.sheets_in_new_workbook)
}

#[test]
fn a_new_workbook_takes_its_normal_font_and_sheets_from_the_preferences() {
    let mut s = Session::new();
    let set = s.execute("options.newWorkbooks", json!({"font": "Arial", "fontSize": 10, "sheets": 3})).unwrap();
    assert_eq!(set, json!({"font": "Arial", "fontSize": 10.0, "sheets": 3}));
    assert_eq!(settings(&s), ("Arial".into(), 10.0, 3));
    s.execute("file.new", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 3);
    assert_eq!(name_and_size(&normal_font(&s)), ("Arial", 10.0), "the Normal style");

    // A typed cell shows the Normal font, and a format on top keeps it.
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    assert_eq!(name_and_size(&cell_font(&s, "A1")), ("Arial", 10.0));
    s.execute("home.bold", json!({"range": "B1"})).unwrap();
    let b1 = cell_font(&s, "B1");
    assert!(b1.bold && name_and_size(&b1) == ("Arial", 10.0), "{b1:?}");
    // The Normal cell style is the workbook's own Normal style, not Calibri 11.
    s.execute("home.cellStyle", json!({"range": "B1", "name": "Normal"})).unwrap();
    assert_eq!(cell_style(&s, "B1"), StyleId::DEFAULT);

    // Saved, font 0 is Arial 10: the font of cell format 0 and of cellStyleXfs xf 0, which the
    // Normal cell style (builtinId 0) names. Arial is not the theme's body font: no scheme link.
    let xml = styles_xml(&s.doc().unwrap().wb);
    let fonts = items(&xml, "fonts", "<font>");
    let font0 = fonts.first().unwrap();
    assert!(font0.contains(r#"<sz val="10"/>"#) && font0.contains(r#"<name val="Arial"/>"#), "{xml}");
    assert!(!font0.contains("scheme"), "{xml}");
    assert!(items(&xml, "cellStyleXfs", "<xf ").first().unwrap().contains(r#"fontId="0""#), "{xml}");
    assert!(items(&xml, "cellXfs", "<xf ").first().unwrap().contains(r#"fontId="0""#), "{xml}");
    assert!(xml.contains(r#"<cellStyle name="Normal" xfId="0" builtinId="0"/>"#), "{xml}");
    // Read back, it is still the Normal font.
    let bytes = io::save_bytes(&s.doc().unwrap().wb, "Book1.xlsx").unwrap();
    let (back, _) = io::open_bytes("Book1.xlsx", &bytes).unwrap();
    assert_eq!(name_and_size(&back.styles.get(StyleId::DEFAULT).font), ("Arial", 10.0));
    assert_eq!(back.sheets.len(), 3);
}

#[test]
fn an_opened_workbook_keeps_its_own_normal_font_whatever_the_preferences() {
    // defont.xlsx: a Normal style of Arial 10 and text in A1 and B1, built in code.
    let mut maker = Session::new();
    maker.new_workbook();
    maker.execute("range.setValues", json!({"range": "A1", "values": [["a", "b"]]})).unwrap();
    let mut wb = (*maker.doc().unwrap().wb).clone();
    wb.styles = StyleTable::with_default(Style { font: Font { name: "Arial".into(), size: 10.0, ..Font::default() }, ..Style::default() });
    let bytes = io::save_bytes(&wb, "defont.xlsx").unwrap();

    let mut s = Session::new();
    s.execute("options.newWorkbooks", json!({"font": "Verdana", "fontSize": 14, "sheets": 2})).unwrap();
    s.execute("file.open", json!({"name": "defont.xlsx", "base64": io::base64_encode(&bytes)})).unwrap();
    assert_eq!(name_and_size(&normal_font(&s)), ("Arial", 10.0));
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1, "the file's sheets, not the preference's");
    s.execute("cell.set", json!({"cell": "C1", "input": "c"})).unwrap();
    for a1 in ["A1", "B1", "C1"] {
        assert_eq!(name_and_size(&cell_font(&s, a1)), ("Arial", 10.0), "{a1}");
    }
    let font0 = items(&styles_xml(&s.doc().unwrap().wb), "fonts", "<font>").first().cloned().unwrap();
    assert!(font0.contains(r#"<sz val="10"/>"#) && font0.contains(r#"<name val="Arial"/>"#), "{font0}");

    // File › New in the same session follows the preferences; the opened workbook keeps Arial 10.
    s.execute("file.new", json!({})).unwrap();
    assert_eq!(name_and_size(&normal_font(&s)), ("Verdana", 14.0));
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    let opened = &s.documents()[0].wb;
    assert_eq!(name_and_size(&opened.styles.get(StyleId::DEFAULT).font), ("Arial", 10.0));
}

#[test]
fn a_change_applies_to_the_next_new_workbook_and_leaves_open_ones_alone() {
    let mut s = Session::new();
    // Without a setting, Calibri 11 and one sheet, as before.
    assert_eq!(s.execute("options.newWorkbooks", json!({})).unwrap(), json!({"font": "Calibri", "fontSize": 11.0, "sheets": 1}));
    s.new_workbook();
    assert_eq!(s.doc().unwrap().wb.styles.get(StyleId::DEFAULT), &Style::default());
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);

    s.execute("options.newWorkbooks", json!({"font": "  Georgia ", "fontSize": 12.5})).unwrap();
    let first = &s.documents()[0];
    assert_eq!(name_and_size(&first.wb.styles.get(StyleId::DEFAULT).font), ("Calibri", 11.0));
    assert!(!first.is_dirty() && first.undo.is_empty(), "a preference is no change to an open workbook");
    s.new_workbook();
    assert_eq!(name_and_size(&normal_font(&s)), ("Georgia", 12.5), "the name is trimmed");
    // {} reports the settings and changes nothing; a size between half points is rounded, as Font Size rounds it.
    assert_eq!(s.execute("options.newWorkbooks", json!({})).unwrap(), json!({"font": "Georgia", "fontSize": 12.5, "sheets": 1}));
    assert_eq!(s.execute("options.newWorkbooks", json!({"fontSize": 10.3})).unwrap()["fontSize"], 10.5);
    assert_eq!(s.execute("options.newWorkbooks", json!({"fontSize": null, "sheets": 255})).unwrap()["sheets"], 255);
    s.new_workbook();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 255);
}

#[test]
fn bad_settings_are_refused_and_change_nothing() {
    let mut s = Session::new();
    s.execute("options.newWorkbooks", json!({"font": "Arial", "fontSize": 10, "sheets": 3})).unwrap();
    let long = "x".repeat(32);
    for bad in [
        json!({"font": ""}),
        json!({"font": "   "}),
        json!({"font": long}),
        json!({"font": 5}),
        json!({"font": "Ari\nal"}),
        json!({"font": ["Arial"]}),
        json!({"fontSize": 0}),
        json!({"fontSize": 0.5}),
        json!({"fontSize": 409.5}),
        json!({"fontSize": -11}),
        json!({"fontSize": 1e300}),
        json!({"fontSize": "11"}),
        json!({"sheets": 0}),
        json!({"sheets": 256}),
        json!({"sheets": 2.5}),
        json!({"sheets": -1}),
        json!({"sheets": "3"}),
        json!({"sheets": u64::MAX}),
        // One bad value refuses the whole call.
        json!({"font": "Verdana", "fontSize": 12, "sheets": 0}),
    ] {
        assert!(s.execute("options.newWorkbooks", bad.clone()).is_err(), "{bad}");
        assert_eq!(settings(&s), ("Arial".into(), 10.0, 3), "{bad}");
    }
    // The limits themselves are taken: a 31-character name, 1 and 409 points, 1 sheet.
    let name = "ä".repeat(31);
    let r = s.execute("options.newWorkbooks", json!({"font": name.clone(), "fontSize": 409, "sheets": 1})).unwrap();
    assert_eq!(r, json!({"font": name, "fontSize": 409.0, "sheets": 1}));
    assert_eq!(s.execute("options.newWorkbooks", json!({"fontSize": 1})).unwrap()["fontSize"], 1.0);
}

#[test]
fn the_preferences_file_holds_the_settings_and_unusable_values_fall_back() {
    // The issue's prefs.json: Arial 10 and three sheets.
    let mut s = Session::new();
    s.prefs = serde_json::from_str(r#"{"defaultFont": "Arial", "defaultFontSize": 10, "sheetsInNewWorkbook": 3}"#).unwrap();
    s.execute("file.new", json!({})).unwrap();
    assert_eq!(name_and_size(&normal_font(&s)), ("Arial", 10.0));
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 3);
    // The command keeps them under the same keys.
    s.execute("options.newWorkbooks", json!({"font": "Verdana", "fontSize": 9, "sheets": 2})).unwrap();
    let saved = serde_json::to_value(&s.prefs).unwrap();
    assert_eq!((&saved["defaultFont"], &saved["defaultFontSize"], &saved["sheetsInNewWorkbook"]), (&json!("Verdana"), &json!(9.0), &json!(2)));

    // A prefs.json from before the settings had a control keeps Calibri 11 and one sheet.
    let old: Prefs = serde_json::from_str(r#"{"userName": "Ann"}"#).unwrap();
    assert_eq!(name_and_size(&old.new_workbook_font()), ("Calibri", 11.0));
    assert_eq!(old.new_workbook_sheets(), 1);
    // Values edited by hand that the command would refuse give the defaults instead.
    let odd: Prefs = serde_json::from_str(r#"{"defaultFont": "", "defaultFontSize": 0, "sheetsInNewWorkbook": 0}"#).unwrap();
    let mut s = Session::new();
    s.prefs = odd;
    s.new_workbook();
    assert_eq!(name_and_size(&normal_font(&s)), ("Calibri", 11.0));
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
    assert_eq!(s.execute("options.newWorkbooks", json!({})).unwrap(), json!({"font": "Calibri", "fontSize": 11.0, "sheets": 1}));
    s.prefs.default_font = "y".repeat(10_000);
    s.prefs.default_font_size = f32::INFINITY;
    s.prefs.sheets_in_new_workbook = usize::MAX;
    assert_eq!(name_and_size(&s.prefs.new_workbook_font()), ("Calibri", 11.0));
    assert_eq!(s.prefs.new_workbook_sheets(), 255);
}

#[test]
fn a_pivot_table_report_uses_the_workbooks_normal_font() {
    let mut s = Session::new();
    s.execute("options.newWorkbooks", json!({"font": "Arial", "fontSize": 10})).unwrap();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Region", "Sales"], ["East", "10"], ["West", "4"]]})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:B3", "destination": "Sheet1!F1", "rows": ["Region"], "values": ["Sales"]})).unwrap();
    let wb = &s.doc().unwrap().wb;
    let sh = wb.active().unwrap();
    let report: Vec<_> = sh.cells.iter_range(RangeRef::parse("F1:H10").unwrap()).filter(|(_, c)| !c.value.is_empty()).collect();
    assert!(report.len() >= 4, "{report:?}");
    for (at, cell) in report {
        assert_eq!(name_and_size(&wb.styles.get(cell.style).font), ("Arial", 10.0), "{}", at.a1());
    }
}
