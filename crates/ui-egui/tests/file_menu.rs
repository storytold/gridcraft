//! The shared ribbon exposes basic file operations without requiring a native menu.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::{Services, SheetApp};

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

fn choose(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    h.get_by_label("File").click();
    h.run_steps(3);
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn new_and_open_are_visible_even_when_the_ribbon_is_collapsed() {
    let opened = Arc::new(Mutex::new(0));
    let capture = opened.clone();
    let mut h = harness(Services {
        pick_open: Some(Box::new(move || {
            *capture.lock().unwrap() += 1;
            None
        })),
        ..Default::default()
    });
    h.state_mut().ui.ribbon_tab = "Data".into();
    h.state_mut().ui.ribbon_collapsed = true;
    h.run_steps(2);
    choose(&mut h, "New Workbook");
    assert_eq!(h.state().session.documents().len(), 2);
    choose(&mut h, "Open…");
    assert_eq!(*opened.lock().unwrap(), 1, "Open invokes the platform file picker");
    assert_eq!(h.state().ui.ribbon_tab, "Data");
    assert!(h.state().ui.ribbon_collapsed);
    assert!(h.query_by_label("Open…").is_none(), "choosing an action dismisses File");
}

#[test]
fn save_actions_include_the_cell_draft_and_close_keeps_the_unsaved_prompt() {
    let downloads = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let capture = downloads.clone();
    let mut h = harness(Services { download: Some(Box::new(move |_, bytes| capture.lock().unwrap().push(bytes.to_vec()))), ..Default::default() });
    h.input_mut().events.push(egui::Event::Text("123".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    choose(&mut h, "Save");
    choose(&mut h, "Save As…");
    let saved = downloads.lock().unwrap();
    assert_eq!(saved.len(), 2);
    for bytes in saved.iter() {
        let (wb, _) = gridcraft_engine::io::open_bytes("saved.xlsx", bytes, &gridcraft_locale::INVARIANT).unwrap();
        assert_eq!(wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0));
    }
    drop(saved);
    assert!(h.state().editor.is_none());
    choose(&mut h, "Close");
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0));
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
