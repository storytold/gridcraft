//! IMAGE: the picture it fetches shows in its cell and keeps its formula through XLSX. It is
//! `#BLOCKED!` when no fetcher is installed or the host refuses the URL, `#CONNECT!` when the request
//! fails, and `#VALUE!` when the URL is not https or the response is not a picture.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_engine::calc::{FetchError, ImageFetcher};
use serde_json::json;

/// A valid 1x1 PNG, so the XLSX writer accepts it as a picture.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0x63, 0x64, 0x60, 0xf8, 0x5f,
    0x0f, 0x00, 0x02, 0x87, 0x01, 0x80, 0xeb, 0x47, 0xba, 0x92, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Serves one picture and one non-picture by exact URL, refuses one host, and fails any other URL
/// like an unreachable host.
#[derive(Debug)]
struct CannedImages;

impl ImageFetcher for CannedImages {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        match url {
            "https://img.test/logo.png" => Ok(PNG.to_vec()),
            "https://img.test/vector.svg" => Ok(b"<svg/>".to_vec()),
            "https://img.test/anim.gif" => Ok(b"GIF89a\x01\x00\x01\x00\x00\x00\x00;".to_vec()),
            "https://intranet.test/logo.png" => Err(FetchError::Blocked),
            _ => Err(FetchError::Connect),
        }
    }
}

fn set(s: &mut Session, cell: &str, input: &str) {
    s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
}

fn value(s: &Session, cell: &str) -> Value {
    s.doc().unwrap().wb.sheet(0).unwrap().value(CellRef::parse(cell).unwrap())
}

fn has_picture(s: &Session, cell: &str) -> bool {
    s.doc().unwrap().wb.sheet(0).unwrap().cell_pictures.contains_key(&CellRef::parse(cell).unwrap())
}

#[test]
fn image_shows_the_fetched_picture_with_its_alt_text() {
    let mut s = Session::new();
    s.new_workbook();
    s.set_image_fetcher(Some(Arc::new(CannedImages)));
    set(&mut s, "A1", "=IMAGE(\"https://img.test/logo.png\",\"Company logo\")");
    assert_eq!(value(&s, "A1"), Value::from("Company logo"));
    let sheet = s.doc().unwrap().wb.sheet(0).unwrap();
    let picture = sheet.cell_pictures.get(&CellRef::parse("A1").unwrap()).unwrap();
    assert_eq!(picture.data, PNG);
    assert_eq!(picture.mime, "image/png");
    assert_eq!(picture.alt, "Company logo");
}

#[test]
fn image_without_a_fetcher_is_blocked_and_no_picture() {
    let mut s = Session::new();
    s.new_workbook();
    set(&mut s, "A1", "=IMAGE(\"https://img.test/logo.png\")");
    assert_eq!(value(&s, "A1"), Value::Error(CellError::Blocked));
    assert!(!has_picture(&s, "A1"));
}

#[test]
fn image_errors_say_why_there_is_no_picture() {
    let mut s = Session::new();
    s.new_workbook();
    s.set_image_fetcher(Some(Arc::new(CannedImages)));
    for (cell, formula, error) in [
        ("A1", "=IMAGE(\"http://img.test/logo.png\")", CellError::Value),
        ("A2", "=IMAGE(\"https://img.test/vector.svg\")", CellError::Value),
        ("A3", "=IMAGE(\"https://img.test/missing.png\")", CellError::Connect),
        ("A4", "=IMAGE(\"https://intranet.test/logo.png\")", CellError::Blocked),
        ("A5", "=IMAGE(\"https://img.test/logo.png\",\"x\",3,40)", CellError::Value),
    ] {
        set(&mut s, cell, formula);
        assert_eq!(value(&s, cell), Value::Error(error), "{formula}");
        assert!(!has_picture(&s, cell), "{formula}");
    }
    // The new errors work like any other: ISERROR sees them and ERROR.TYPE numbers them.
    set(&mut s, "B3", "=ISERROR(A3)");
    set(&mut s, "B4", "=ERROR.TYPE(A3)&\",\"&ERROR.TYPE(A4)");
    assert_eq!(value(&s, "B3"), Value::Bool(true));
    assert_eq!(value(&s, "B4"), Value::from("10,11"));
}

#[test]
fn changing_the_formula_removes_the_picture() {
    let mut s = Session::new();
    s.new_workbook();
    s.set_image_fetcher(Some(Arc::new(CannedImages)));
    set(&mut s, "A1", "=IMAGE(\"https://img.test/logo.png\",\"Logo\")");
    assert!(has_picture(&s, "A1"));
    set(&mut s, "A1", "=1+1");
    assert_eq!(value(&s, "A1"), Value::Number(2.0));
    assert!(!has_picture(&s, "A1"));
}

#[test]
fn image_formula_and_picture_survive_xlsx_round_trip() {
    let mut s = Session::new();
    s.new_workbook();
    s.set_image_fetcher(Some(Arc::new(CannedImages)));
    set(&mut s, "A1", "=IMAGE(\"https://img.test/logo.png\",\"Logo\")");
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sheet = wb.sheet(0).unwrap();
    let a1 = CellRef::parse("A1").unwrap();
    assert!(sheet.cell_pictures.contains_key(&a1));
    assert!(sheet.cell(a1).and_then(|c| c.formula.as_ref()).is_some());
}

#[test]
fn image_sizing_is_stored_and_survives_xlsx_round_trip() {
    use gridcraft_engine::core::PictureSizing;
    let mut s = Session::new();
    s.new_workbook();
    s.set_image_fetcher(Some(Arc::new(CannedImages)));
    set(&mut s, "A1", "=IMAGE(\"https://img.test/logo.png\",\"Logo\",3,40,60)");
    set(&mut s, "A2", "=IMAGE(\"https://img.test/logo.png\",\"Logo\",1)");
    let a1 = CellRef::parse("A1").unwrap();
    let a2 = CellRef::parse("A2").unwrap();
    let sheet = s.doc().unwrap().wb.sheet(0).unwrap();
    assert_eq!(sheet.cell_pictures[&a1].sizing, PictureSizing::Custom { width: 60, height: 40 });
    assert_eq!(sheet.cell_pictures[&a2].sizing, PictureSizing::Fill);

    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sheet = wb.sheet(0).unwrap();
    assert_eq!(sheet.cell_pictures[&a1].sizing, PictureSizing::Custom { width: 60, height: 40 });
    assert_eq!(sheet.cell_pictures[&a2].sizing, PictureSizing::Fill);
}

/// A file's pictures survive opening it when the session has a fetcher: the open-time calculation
/// must already see it, or the IMAGE cells would turn into `#VALUE!` and lose their pictures.
#[test]
fn pictures_survive_opening_a_file_with_a_fetcher() {
    let mut s = Session::new();
    s.new_workbook();
    s.set_image_fetcher(Some(Arc::new(CannedImages)));
    set(&mut s, "A1", "=IMAGE(\"https://img.test/logo.png\",\"Logo\")");
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (wb, _) = gridcraft_engine::io::open_bytes("logo.xlsx", &bytes).unwrap();
    let doc = gridcraft_engine::DocState::new_with_images(wb, None, "logo".into(), gridcraft_engine::Trust::Local, Some(Arc::new(CannedImages)));
    let a1 = CellRef::parse("A1").unwrap();
    assert!(doc.wb.sheet(0).unwrap().cell_pictures.contains_key(&a1));
    assert_eq!(doc.wb.sheet(0).unwrap().value(a1), Value::from("Logo"));
}

/// A picture the file format cannot hold (here a GIF) does not stop the save: the formula and its
/// value are written without it, and the picture is fetched again when the file opens.
#[test]
fn unstorable_picture_is_left_out_of_the_file() {
    let mut s = Session::new();
    s.new_workbook();
    s.set_image_fetcher(Some(Arc::new(CannedImages)));
    set(&mut s, "A1", "=IMAGE(\"https://img.test/anim.gif\",\"Anim\")");
    assert!(has_picture(&s, "A1"));
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sheet = wb.sheet(0).unwrap();
    let a1 = CellRef::parse("A1").unwrap();
    assert!(!sheet.cell_pictures.contains_key(&a1));
    assert!(sheet.cell(a1).and_then(|c| c.formula.as_ref()).is_some());
}
