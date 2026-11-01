//! Opening a `.txt` or `.prn` file asks how it splits, with a preview, as Excel's Text Import
//! Wizard does (#2553). A `.csv` opens straight away.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use serde_json::json;

/// A German report: a title, `;`, decimal commas and thousands dots, a quoted `;`.
const REPORT: &str = "Umsatzbericht Oktober 2026\r\n\r\nArtikel;Umsatz\r\nApfel;1.234,50\r\n\"Pflaume; blau\";99,00\r\n";

fn harness(services: Services) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    h
}

/// `text` written to `name` in a fresh temp dir named after `tag`.
fn file(tag: &str, name: &str, text: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("gridcraft-text-import-ui-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn dialog(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<String> {
    h.state().dialog.as_ref().map(|d| d.name().to_string())
}

/// What the dialog's preview found, as `file.previewText` reports it.
fn found(h: &egui_kittest::Harness<'static, SheetApp>) -> serde_json::Value {
    h.state().dialog.as_ref().and_then(|d| d.result.as_ref()).map(|r| r["r"]["text"].clone()).unwrap_or_default()
}

fn cell(h: &egui_kittest::Harness<'static, SheetApp>, a1: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(a1).unwrap())
}

#[test]
fn a_text_file_opens_through_text_import_with_what_detection_found() {
    let path = file("detect", "report.txt", REPORT);
    let p = path.to_string_lossy().to_string();
    let mut h = harness(Services::default());
    let before = h.state().session.documents().len();
    h.state_mut().open_path(&p);
    h.run_steps(3);
    assert_eq!(dialog(&h).as_deref(), Some("textImport"));
    assert_eq!(h.state().session.documents().len(), before, "nothing opens before OK");
    h.check_accessibility();
    let f = found(&h);
    assert_eq!(f["delimiters"], json!([";"]));
    assert_eq!((f["startRow"].as_u64(), f["decimal"].as_str()), (Some(3), Some(",")));
    // The preview shows the table's first rows, the quoted `;` kept in its field.
    let preview = h.state().dialog.as_ref().and_then(|d| d.result.as_ref()).map(|r| r["r"]["preview"].clone()).unwrap_or_default();
    assert_eq!(preview[2], json!(["Pflaume; blau", "99,00"]));

    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!(cell(&h, "A1"), Value::text("Artikel"));
    assert_eq!(cell(&h, "B2"), Value::Number(1234.5));
    assert_eq!(cell(&h, "A3"), Value::text("Pflaume; blau"));
    assert_eq!(h.state().ui.recent.first(), Some(&p));

    // Opened again while open: switched to, nothing to ask.
    let docs = h.state().session.documents().len();
    h.state_mut().open_path(&p);
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!(h.state().session.documents().len(), docs);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn delimited_starts_from_what_detection_found_and_the_settings_reach_file_open() {
    let path = file("delimited", "report.txt", REPORT);
    let mut h = harness(Services::default());
    h.state_mut().open_path(&path.to_string_lossy());
    h.run_steps(3);
    h.get_by_label("Delimited").click();
    h.run_steps(3);
    let values = h.state().dialog.as_ref().map(|d| d.values.clone()).unwrap_or_default();
    assert_eq!((values["mode"].as_str(), values["semicolon"].as_bool(), values["tab"].as_bool()), (Some("delimited"), Some(true), Some(false)));
    assert_eq!(values["startRow"], json!(3), "the detected start row is kept");
    h.check_accessibility();
    // Keep every field as text: the amount stays the text the file holds.
    h.state_mut().dialog.as_mut().unwrap().values.insert("asText".into(), json!(true));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!(cell(&h, "B2"), Value::text("1.234,50"));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn cancel_opens_nothing_and_a_csv_opens_straight_away() {
    let path = file("cancel", "report.txt", REPORT);
    let mut h = harness(Services::default());
    let before = h.state().session.documents().len();
    h.state_mut().open_path(&path.to_string_lossy());
    h.run_steps(3);
    h.get_by_label(" Cancel ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!(h.state().session.documents().len(), before);
    assert!(h.state().message.is_none());
    // A .csv is detected and opened without asking, as Excel opens it.
    let csv = file("csv", "report.csv", REPORT);
    h.state_mut().open_path(&csv.to_string_lossy());
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!(cell(&h, "A1"), Value::text("Umsatzbericht Oktober 2026"));
    assert_eq!(cell(&h, "B4"), Value::Number(1234.5));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(csv.parent().unwrap()).unwrap();
}

#[test]
fn a_text_file_picked_on_the_web_opens_through_text_import_too() {
    let inbox: Inbox = Default::default();
    let mut h = harness(Services { inbox: Some(inbox.clone()), ..Default::default() });
    inbox.lock().unwrap().push(("t.prn".into(), b"plain     a,b       12\r\nr2        d2\r\n".to_vec()));
    h.run_steps(3);
    assert_eq!(dialog(&h).as_deref(), Some("textImport"));
    assert!(h.state().dialog.as_ref().is_some_and(|d| d.values.contains_key("base64")));
    assert_eq!(found(&h)["fixedWidths"], json!([10, 20]));
    // Fixed width starts from the widths detection found.
    h.get_by_label("Fixed width").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.values["fixedWidths"].clone()), Some(json!("10, 20")));
    h.check_accessibility();
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert_eq!(dialog(&h), None);
    assert_eq!((cell(&h, "A1"), cell(&h, "B1"), cell(&h, "C1")), (Value::text("plain"), Value::text("a,b"), Value::Number(12.0)));
}

#[test]
fn text_files_picked_together_are_asked_about_one_after_another() {
    let inbox: Inbox = Default::default();
    let mut h = harness(Services { inbox: Some(inbox.clone()), ..Default::default() });
    inbox.lock().unwrap().extend([
        ("a.txt".to_string(), b"a1\tb1\r\n".to_vec()),
        ("b.prn".to_string(), b"plain     a,b       12\r\n".to_vec()),
        ("c.csv".to_string(), b"c1,d1\r\n".to_vec()),
    ]);
    h.run_steps(3);
    let asked = |h: &egui_kittest::Harness<'static, SheetApp>| h.state().dialog.as_ref().and_then(|d| d.values.get("name").cloned());
    assert_eq!(asked(&h), Some(json!("a.txt")));
    assert_eq!(inbox.lock().unwrap().len(), 2, "the files after it wait for the dialog");
    h.get_by_label(" Cancel ").click();
    h.run_steps(3);
    assert_eq!(asked(&h), Some(json!("b.prn")));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    // Nothing asks about the .csv: it opens once the dialog before it closes.
    assert_eq!(dialog(&h), None);
    assert!(inbox.lock().unwrap().is_empty());
    let titles: Vec<String> = h.state().session.documents().iter().map(|d| d.title.clone()).collect();
    assert!(!titles.iter().any(|t| t.starts_with("a.")), "{titles:?}");
    assert!(titles.iter().any(|t| t.starts_with("b.")) && titles.iter().any(|t| t.starts_with("c.")), "{titles:?}");
    assert_eq!(cell(&h, "A1"), Value::text("c1"));
}
