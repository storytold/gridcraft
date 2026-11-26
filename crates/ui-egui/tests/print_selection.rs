//! File › Print › Settings offers "Print Selection": only the selected cells are previewed and sent.
use std::{cell::RefCell, rc::Rc};

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// A sheet whose print area (F10:H20, "Outside") is not where the selection (A1:B2, "Inside") is,
/// so printing the selection and printing the sheet give different pages.
fn harness(jobs: Rc<RefCell<Vec<Vec<u8>>>>) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.run("cell.set", json!({"cell": "A1", "input": "Inside"})).unwrap();
    app.run("cell.set", json!({"cell": "D4", "input": "Second area"})).unwrap();
    app.run("cell.set", json!({"cell": "H20", "input": "Outside"})).unwrap();
    app.run("pageLayout.pageSetup", json!({"printArea": "F10:H20"})).unwrap();
    app.run("selection.set", json!({"range": "A1:B2"})).unwrap();
    app.services.print = Some(Box::new(move |bytes| jobs.borrow_mut().push(bytes.to_vec())));
    app.run("ui.backstage", json!({"page": "print"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1000.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn choose_selection(h: &mut egui_kittest::Harness<'static, SheetApp>) {
    h.get_all_by_role(egui::accesskit::Role::ComboBox).next().expect("print scope combo").click();
    h.run_steps(3);
    h.get_by_label("Print Selection").click();
    h.run_steps(4);
}

#[test]
fn print_selection_previews_and_prints_only_the_selected_cells() {
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness(jobs.clone());
    assert!(h.query_by_label("Sheet1 · F10:H20").is_some(), "the sheet prints its print area by default");
    choose_selection(&mut h);
    assert!(h.query_by_label("Sheet1 · A1:B2").is_some(), "the preview shows the selection");
    assert!(h.query_by_label("Sheet1 · F10:H20").is_none());

    h.get_all_by_role_and_label(egui::accesskit::Role::Button, "Print").last().expect("Print button").click();
    h.run_steps(3);
    let jobs = jobs.borrow();
    assert_eq!(jobs.len(), 1);
    assert!(contains(&jobs[0], "(Inside) Tj"), "the selected cell is printed");
    assert!(!contains(&jobs[0], "(Outside) Tj"), "the print area is not");
}

#[test]
fn the_preview_follows_the_selection_and_each_area_gets_its_own_page() {
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let mut h = harness(jobs.clone());
    choose_selection(&mut h);
    assert!(h.query_by_label("Sheet1 · A1:B2").is_some());

    // Same document revision, other selection: the cached preview must not be reused.
    h.state_mut().run("selection.set", json!({"range": "D4:E5"})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("Sheet1 · D4:E5").is_some(), "the preview is rebuilt for the new selection");
    assert!(h.query_by_label("Sheet1 · A1:B2").is_none());

    // Two selected areas: Excel prints each area on its own page.
    h.state_mut().run("selection.set", json!({"range": "A1:B2,D4:E5"})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label("Page 1 of 2").is_some(), "two areas, two pages");
}
