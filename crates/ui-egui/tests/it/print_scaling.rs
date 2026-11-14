//! Scaling is selected through the Print UI and shared by preview and native submission.
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::{Session, cmd::print::PrintScaling};
use gridcraft_ui_egui::{Services, SheetApp, i18n::Language, printing::Printer};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

/// A saved custom scale remains the opening default; the second sheet exercises workbook scope.
fn app(jobs: Rc<RefCell<Vec<Vec<u8>>>>) -> SheetApp {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"name":"Tall"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"paper":"A4","printArea":"A1:AN200","scale":42,"fitToPage":false})).unwrap();
    s.execute("home.insertSheet", json!({"name":"Other"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"printArea":"A1:B2","scale":80,"fitToPage":false})).unwrap();
    s.execute("sheet.activate", json!({"sheet":"Tall"})).unwrap();
    let services = Services {
        printers: Some(Box::new(|| {
            let (sender, receiver) = std::sync::mpsc::channel();
            sender.send(vec![Printer { name: "Queue".into(), label: String::new(), default: true }]).unwrap();
            receiver
        })),
        print_native: Some(Box::new(move |bytes, _settings| {
            jobs.borrow_mut().push(bytes);
            let (sender, receiver) = std::sync::mpsc::channel();
            sender.send(Ok("queued".into())).unwrap();
            receiver
        })),
        ..Default::default()
    };
    let mut app = crate::common::prepare(SheetApp::new(s, services));
    app.ui.language = Language::En;
    app.printing.printers.push(Printer { name: "Queue".into(), label: String::new(), default: true });
    app.run("ui.printSettings", json!({"printer":"Queue"})).unwrap();
    app.run("ui.backstage", json!({"page":"print"})).unwrap();
    app
}

/// Exercise the actual scaling ComboBox, including the selection click and its repaint.
fn select(h: &mut crate::common::Harness, label: &str) {
    h.get_by_label("Print Scaling").click();
    h.run_steps(6);
    h.get_by_label(label).click();
    h.run_steps(3);
}

/// Compare painted cached pagination with the selected job's engine preview.
fn preview(h: &mut crate::common::Harness) -> Value {
    let params = h.state().backstage.as_ref().unwrap().print_params();
    let result = h.state_mut().run("file.printPreview", params).unwrap();
    let text = format!("Page 1 of {}", result["pages"]);
    assert!(
        h.output().shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()==text)),
        "preview must repaint with {text}"
    );
    result
}

/// All options work in a narrow window, invalidate cached preview and reach native printing.
#[test]
fn print_scaling_picker_updates_preview_and_native_job() {
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let app = app(jobs.clone());
    let before = serde_json::to_value(&app.session.doc().unwrap().wb).unwrap();
    let revision = app.session.doc().unwrap().revision;
    let mut h = crate::common::harness_with(crate::common::builder().with_size(egui::vec2(800.0, 700.0)), app, 4);
    assert_eq!(h.state().backstage.as_ref().unwrap().scaling, PrintScaling::PageSetup);
    let initial = preview(&mut h);
    assert!(initial["pages"].as_u64().unwrap() > 1);
    for (index, (mode, label)) in [
        (PrintScaling::Sheet, "Fit Sheet on One Page"),
        (PrintScaling::NoScaling, "No Scaling"),
        (PrintScaling::Columns, "Fit All Columns on One Page"),
        (PrintScaling::Rows, "Fit All Rows on One Page"),
        (PrintScaling::PageSetup, "Use Page Setup"),
    ]
    .into_iter()
    .enumerate()
    {
        select(&mut h, label);
        assert_eq!(h.state().backstage.as_ref().unwrap().scaling, mode);
        let p = preview(&mut h);
        if mode == PrintScaling::Sheet {
            assert_eq!(p["pages"], 1);
        }
        if mode == PrintScaling::NoScaling {
            assert!(p["pageList"].as_array().unwrap().iter().all(|page| page["scale"] == 100.0));
        }
        let submitted = h.state_mut().run("ui.printSubmit", json!({})).unwrap();
        assert_eq!(submitted["pages"], p["pages"], "native job uses {label}");
        assert_eq!(jobs.borrow().len(), index + 1);
        assert!(jobs.borrow().last().unwrap().starts_with(b"%PDF"));
        h.run_steps(3);
    }
    select(&mut h, "Fit Sheet on One Page");
    h.get_by_label("Print What").click();
    h.run_steps(6);
    h.get_by_label("Print Entire Workbook").click();
    h.run_steps(3);
    let all = preview(&mut h);
    assert_eq!(all["pages"], 2);
    assert_eq!(h.state_mut().run("ui.printSubmit", json!({})).unwrap()["pages"], 2);
    h.run_steps(3);
    let explicit = json!({"sheets":"active","scaling":"none"});
    let expected = h.state_mut().run("file.printPreview", explicit.clone()).unwrap();
    assert_eq!(h.state_mut().run("ui.printSubmit", explicit).unwrap()["pages"], expected["pages"], "explicit control parameters override the picker");
    h.run_steps(3);
    assert_eq!(h.state_mut().run("ui.printSubmit", json!({"fitToPage":true})).unwrap()["pages"], 2, "legacy overrides stay unambiguous");
    assert_eq!(serde_json::to_value(&h.state().session.doc().unwrap().wb).unwrap(), before);
    assert_eq!(h.state().session.doc().unwrap().revision, revision);
}

/// Direct null and empty-object submissions inherit the same selected scope and scaling.
#[test]
fn null_print_submission_inherits_selected_scaling_and_scope() {
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let mut app = app(jobs.clone());
    let before = serde_json::to_value(&app.session.doc().unwrap().wb).unwrap();
    let revision = app.session.doc().unwrap().revision;
    let dirty = app.session.doc().unwrap().is_dirty();
    let ctx = egui::Context::default();
    for all in [false, true] {
        for mode in [PrintScaling::Sheet, PrintScaling::NoScaling, PrintScaling::Columns, PrintScaling::Rows, PrintScaling::PageSetup] {
            let bs = app.backstage.as_mut().unwrap();
            bs.print_all = all;
            bs.scaling = mode;
            let params = bs.print_params();
            let preview = app.run("file.printPreview", params).unwrap();
            let empty = app.run("ui.printSubmit", json!({})).unwrap();
            assert_eq!(empty["pages"], preview["pages"]);
            app.poll_printing(&ctx);
            let count = jobs.borrow().len();
            let null = app.run("ui.printSubmit", Value::Null).unwrap();
            assert_eq!(null["pages"], empty["pages"], "null must retain scope all={all} and scaling {mode:?}");
            let submitted = jobs.borrow();
            assert_eq!(submitted.len(), count + 1);
            let pdf = submitted.last().unwrap();
            assert!(pdf.starts_with(b"%PDF"));
            assert_eq!(null["bytes"].as_u64().unwrap(), pdf.len() as u64);
            drop(submitted);
            app.poll_printing(&ctx);
        }
    }
    assert_eq!(serde_json::to_value(&app.session.doc().unwrap().wb).unwrap(), before);
    assert_eq!(app.session.doc().unwrap().revision, revision);
    assert_eq!(app.session.doc().unwrap().is_dirty(), dirty);
}

/// Switching sheets without editing invalidates cached pages and keeps preview and print aligned.
#[test]
fn print_preview_follows_sheet_switch_without_document_revision() {
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let mut h = crate::common::harness_with(crate::common::builder().with_size(egui::vec2(1100.0, 800.0)), app(jobs.clone()), 4);
    let uid = h.state().session.doc().unwrap().uid;
    let revision = h.state().session.doc().unwrap().revision;
    let dirty = h.state().session.doc().unwrap().is_dirty();
    let tall = preview(&mut h);
    assert!(tall["pages"].as_u64().unwrap() > 1);
    h.state_mut().run("sheet.activate", json!({"sheet":"Other"})).unwrap();
    assert_eq!(h.state().session.doc().unwrap().uid, uid);
    assert_eq!(h.state().session.doc().unwrap().revision, revision);
    assert!(h.state().backstage.is_some());
    h.run_steps(4);
    let other = preview(&mut h);
    assert_eq!(other["pages"], 1);
    assert_eq!(other["pageList"][0]["sheet"], "Other");
    assert!(h.output().shapes.iter().any(|shape| matches!(&shape.shape,egui::Shape::Text(t) if t.galley.text()=="Other · A1:B2")));
    assert_eq!(h.state_mut().run("ui.printSubmit", json!({})).unwrap()["pages"], 1);
    assert_eq!(jobs.borrow().len(), 1);
    assert!(jobs.borrow()[0].starts_with(b"%PDF"));
    h.run_steps(3);
    h.state_mut().run("sheet.activate", json!({"sheet":"Tall"})).unwrap();
    h.run_steps(4);
    assert_eq!(preview(&mut h)["pages"], tall["pages"]);
    assert_eq!(h.state().session.doc().unwrap().revision, revision);
    assert_eq!(h.state().session.doc().unwrap().is_dirty(), dirty);
}
