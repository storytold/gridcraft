//! The File tab opens the Backstage view, which exposes the file operations without a native menu.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::{Services, SheetApp};

/// A file dialog that has already answered `path`.
fn answered(path: Option<String>) -> std::sync::mpsc::Receiver<Option<String>> {
    let (answer, picked) = std::sync::mpsc::channel();
    answer.send(path).unwrap();
    picked
}

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

/// Opens File (the Backstage view) and clicks a navigation entry or page button.
fn choose(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    if h.state().backstage.is_none() {
        h.get_by_label("File").click();
        h.run_steps(3);
    }
    click(h, label);
}

fn click(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(3);
}

fn page(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<&'static str> {
    h.state().backstage.as_ref().map(|b| b.page.id())
}

#[test]
fn new_and_open_from_the_file_tab_keep_the_ribbon_as_it_was() {
    let opened = Arc::new(Mutex::new(0));
    let capture = opened.clone();
    let mut h = harness(Services {
        pick_open: Some(Box::new(move || {
            *capture.lock().unwrap() += 1;
            answered(None)
        })),
        ..Default::default()
    });
    h.state_mut().ui.ribbon_tab = "Data".into();
    h.state_mut().ui.ribbon_collapsed = true;
    h.run_steps(2);
    h.get_by_label("File").click();
    h.run_steps(3);
    assert_eq!(page(&h), Some("home"), "the File tab opens the Backstage view on Home");
    choose(&mut h, "New");
    assert_eq!(page(&h), Some("new"));
    click(&mut h, "Blank workbook");
    assert_eq!(h.state().session.documents().len(), 2);
    assert!(h.state().backstage.is_none(), "creating a workbook returns to the sheet");
    choose(&mut h, "Open");
    click(&mut h, "Browse…");
    assert_eq!(*opened.lock().unwrap(), 1, "Browse invokes the platform file picker");
    assert_eq!(page(&h), Some("open"), "a cancelled pick stays on File › Open");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().backstage.is_none(), "Esc returns to the sheet");
    assert_eq!(h.state().ui.ribbon_tab, "Data");
    assert!(h.state().ui.ribbon_collapsed);
}

#[test]
fn save_actions_include_the_cell_draft_and_close_keeps_the_unsaved_prompt() {
    let downloads = Arc::new(Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    let capture = downloads.clone();
    let mut h = harness(Services {
        download: Some(Box::new(move |name, bytes| capture.lock().unwrap().push((name.to_string(), bytes.to_vec())))),
        ..Default::default()
    });
    h.input_mut().events.push(egui::Event::Text("123".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    choose(&mut h, "Save");
    assert!(h.state().editor.is_none(), "opening File commits the cell draft");
    assert_eq!(page(&h), Some("saveAs"), "a workbook without a file saves through Save As");
    click(&mut h, "Download (.xlsx)");
    assert!(h.state().backstage.is_none());
    choose(&mut h, "Save As");
    click(&mut h, "Download (.xlsx)");
    choose(&mut h, "Export");
    click(&mut h, "CSV (Comma delimited)");
    let saved = downloads.lock().unwrap();
    assert_eq!(saved.len(), 3);
    for (_, bytes) in saved.iter().take(2) {
        let (wb, _) = gridcraft_engine::io::open_bytes("saved.xlsx", bytes).unwrap();
        assert_eq!(wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0));
    }
    assert!(saved[2].0.ends_with(".csv"));
    assert!(String::from_utf8_lossy(&saved[2].1).contains("123"));
    drop(saved);
    choose(&mut h, "Close");
    assert!(h.state().backstage.is_none());
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0));
}

/// #901, #385: the system file dialog doesn't run inside the frame (a modal panel there let winit
/// re-enter its event handler on macOS, and the app aborted). Export asks for a path and goes on
/// drawing; the file is written on the frame after the dialog answers.
#[test]
fn export_writes_the_file_once_the_save_dialog_answers() {
    let dir = std::env::temp_dir().join(format!("gridcraft-export-waits-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let reply = Arc::new(Mutex::new(None::<std::sync::mpsc::Sender<Option<String>>>));
    let keep = reply.clone();
    let mut h = harness(Services {
        pick_save: Some(Box::new(move |_| {
            let (answer, picked) = std::sync::mpsc::channel();
            *keep.lock().unwrap() = Some(answer);
            picked
        })),
        ..Default::default()
    });
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    choose(&mut h, "Export");
    click(&mut h, "CSV (Comma delimited)");
    let answer = reply.lock().unwrap().take().expect("Export asks the save dialog for a path");
    let target = dir.join("Report.csv");
    h.run_steps(3);
    assert!(!target.exists(), "nothing is written before the dialog answers");
    assert_eq!(page(&h), Some("export"), "File stays open while the dialog is up");
    answer.send(Some(target.to_string_lossy().into_owned())).unwrap();
    h.run_steps(2);
    assert!(std::fs::read_to_string(&target).unwrap().contains("42"));
    assert!(h.state().backstage.is_none(), "a written export returns to the sheet");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Close › Save on a workbook that was never saved asks where to save it; the workbook closes once
/// the save dialog answers and the file is written, and stays open when the dialog is cancelled.
#[test]
fn close_then_save_closes_the_workbook_once_the_save_dialog_answers() {
    let dir = std::env::temp_dir().join(format!("gridcraft-close-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (answer, closes) in [(Some(dir.join("Kept.xlsx").to_string_lossy().into_owned()), true), (None, false)] {
        let reply = Arc::new(Mutex::new(None::<std::sync::mpsc::Sender<Option<String>>>));
        let keep = reply.clone();
        let mut h = harness(Services {
            pick_save: Some(Box::new(move |_| {
                let (answer, picked) = std::sync::mpsc::channel();
                *keep.lock().unwrap() = Some(answer);
                picked
            })),
            ..Default::default()
        });
        h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
        let uid = h.state().session.active().unwrap().uid;
        choose(&mut h, "Close");
        assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
        click(&mut h, "Save");
        let sender = reply.lock().unwrap().take().expect("Save asks where to save the workbook");
        h.run_steps(3);
        assert!(h.state().session.documents().iter().any(|d| d.uid == uid), "the workbook stays open while the dialog is up");
        sender.send(answer).unwrap();
        h.run_steps(3);
        let open = h.state().session.documents().iter().any(|d| d.uid == uid);
        assert_eq!(open, !closes, "closes once saved; a cancelled dialog keeps it open");
    }
    assert!(dir.join("Kept.xlsx").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn file_options_opens_the_options_page_and_pages_are_reachable_by_command() {
    let mut h = harness(Services::default());
    let r = h.state_mut().run("file.options", serde_json::json!({})).unwrap();
    assert_eq!(r["calculation"]["mode"], "automatic");
    h.run_steps(3);
    assert_eq!(page(&h), Some("options"));
    click(&mut h, "Info");
    assert_eq!(page(&h), Some("info"));
    for id in ["print", "export", "saveAs", "new", "open", "home"] {
        h.state_mut().run("ui.backstage", serde_json::json!({"page": id})).unwrap();
        h.run_steps(2);
        assert_eq!(page(&h), Some(id));
    }
    assert!(h.state_mut().run("ui.backstage", serde_json::json!({"page": "account"})).is_err());
    h.state_mut().run("ui.backstage", serde_json::json!({"page": "close"})).unwrap();
    assert!(h.state().backstage.is_none());
    // The back arrow returns to the sheet too.
    h.state_mut().run("ui.backstage", serde_json::json!({})).unwrap();
    h.run_steps(2);
    click(&mut h, "Back");
    assert!(h.state().backstage.is_none());
}

#[test]
fn export_as_pdf_is_delivered_as_a_download() {
    let files = Arc::new(Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    let capture = files.clone();
    let mut h = harness(Services {
        download: Some(Box::new(move |name, bytes| capture.lock().unwrap().push((name.to_string(), bytes.to_vec())))),
        ..Default::default()
    });
    h.state_mut().run("file.exportPdf", serde_json::json!({})).unwrap();
    let got = files.lock().unwrap();
    assert_eq!(got.len(), 1, "Export as PDF hands a file to the browser");
    assert!(got[0].0.ends_with(".pdf"));
    assert!(got[0].1.starts_with(b"%PDF-"));
}

#[test]
fn save_as_offers_pdf_and_leaves_the_workbook_file_alone() {
    let dir = std::env::temp_dir().join(format!("gridcraft-save-as-type-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let (capture, target) = (asked.clone(), dir.clone());
    let mut h = harness(Services {
        pick_save: Some(Box::new(move |suggested| {
            capture.lock().unwrap().push(suggested.to_string());
            answered(Some(target.join("Report").to_string_lossy().into_owned()))
        })),
        ..Default::default()
    });
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    choose(&mut h, "Save As");
    h.state_mut().backstage.as_mut().unwrap().save_format = "pdf";
    h.run_steps(2);
    click(&mut h, "Browse…");
    assert!(asked.lock().unwrap().first().is_some_and(|s| s.ends_with(".pdf")), "the picker is offered a .pdf name");
    assert!(std::fs::read(dir.join("Report.pdf")).unwrap().starts_with(b"%PDF-"));
    let d = h.state().session.active().unwrap();
    assert!(d.path.is_none() && d.is_dirty(), "a PDF is a copy, not the workbook's file");
    let _ = std::fs::remove_dir_all(&dir);
}

fn pdf_picker_harness(dir: &std::path::Path) -> (Arc<Mutex<Vec<String>>>, egui_kittest::Harness<'static, SheetApp>) {
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let (capture, target) = (asked.clone(), dir.to_path_buf());
    let h = harness(Services {
        pick_save: Some(Box::new(move |suggested| {
            capture.lock().unwrap().push(suggested.to_string());
            answered(Some(target.join("Report.xlsx").to_string_lossy().into_owned()))
        })),
        ..Default::default()
    });
    (asked, h)
}

#[test]
fn the_palette_export_as_pdf_asks_where_to_save() {
    let dir = std::env::temp_dir().join(format!("gridcraft-export-pdf-palette-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (asked, mut h) = pdf_picker_harness(&dir);
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    // The palette and menus go through run_or_alert.
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({}));
    h.run_steps(2);
    assert!(asked.lock().unwrap().first().is_some_and(|s| s.ends_with(".pdf")), "the picker is offered a .pdf name");
    assert!(std::fs::read(dir.join("Report.pdf")).unwrap().starts_with(b"%PDF-"), "the PDF is written, whatever extension the picker returned");
    assert!(h.state().message.is_none());
    let d = h.state().session.active().unwrap();
    assert!(d.path.is_none(), "a PDF is a copy, not the workbook's file");
    // Other params survive the round trip through the picker.
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({"range": "nope"}));
    h.run_steps(2);
    assert!(h.state().message.is_some(), "a bad range is reported, not swallowed");
    // A path in the params skips the picker.
    let before = asked.lock().unwrap().len();
    let named = dir.join("Named.pdf");
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({"path": named.to_string_lossy()}));
    assert_eq!(asked.lock().unwrap().len(), before);
    assert!(named.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_export_pdf_picker_writes_nothing() {
    let mut h = harness(Services { pick_save: Some(Box::new(|_| answered(None))), ..Default::default() });
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({}));
    h.run_steps(2);
    assert!(h.state().message.is_none());
    assert!(h.state_mut().session.take_ui_requests().is_empty());
}
