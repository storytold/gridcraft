//! Native print onboarding preserves engine PDF generation and submits the selected settings.
use gridcraft_engine::Session;
use gridcraft_ui_egui::printing::{Duplex, Feedback, Printer};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

#[test]
/// Opening settings and exporting do not spool; a configured native print submits its settings, and a spool error is reported.
fn native_print_uses_selected_settings_and_reports_spool_errors() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.run("cell.set", json!({"cell":"A1", "input":"A sheet to print"})).unwrap();
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let observed = jobs.clone();
    app.services.print_native = Some(Box::new(move |bytes, settings| {
        observed.borrow_mut().push((bytes, settings));
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Err("printer is offline".into())).unwrap();
        receiver
    }));
    app.run_or_alert("file.print", json!({}));
    assert!(app.backstage.is_some(), "a shortcut opens settings without submitting");
    let pdf = app.run("file.print", json!({"sheets":"active"})).unwrap();
    assert!(pdf["base64"].as_str().is_some(), "the engine still returns PDF bytes without submitting");
    assert!(jobs.borrow().is_empty(), "opening settings/exporting does not spool");
    app.printing.printers.push(Printer { name: "Chosen queue".into(), label: String::new(), default: false });
    let configured = app.run("ui.printSettings", json!({"printer":"Chosen queue", "copies":3,"duplex":"shortEdge","grayscale":true})).unwrap();
    assert_eq!(configured["ready"], true);
    assert_eq!(configured["printers"][0]["name"], "Chosen queue");
    app.run("ui.printSubmit", json!({"sheets":"active"})).unwrap();
    assert!(app.printing.busy());
    let jobs = jobs.borrow();
    assert_eq!(jobs.len(), 1);
    assert!(jobs[0].0.starts_with(b"%PDF"));
    assert_eq!(jobs[0].1.printer, "Chosen queue");
    assert_eq!(jobs[0].1.copies, 3);
    assert_eq!(jobs[0].1.duplex, Some(Duplex::ShortEdge));
    assert!(jobs[0].1.grayscale);
    app.poll_printing(&egui::Context::default());
    assert_eq!(app.printing.message, Some(Feedback::Failed("printer is offline".into())));
    assert!(app.message.as_ref().unwrap().1.contains("printer is offline"));
}

#[test]
/// Print settings stay visible and accessible in an 800x700 window; toggling Grayscale updates the settings and readiness.
fn print_settings_are_visible_in_a_narrow_window() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::prepare(SheetApp::new(session, Services::default()));
    app.run("cell.set", json!({"cell":"A1", "input":"A sheet to print"})).unwrap();
    app.services.printers = Some(Box::new(|| {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(vec![Printer { name: "Test queue".into(), label: String::new(), default: true }]).unwrap();
        receiver
    }));
    app.services.print_native = Some(Box::new(|_, _| {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Ok("queued".into())).unwrap();
        receiver
    }));
    app.run("ui.backstage", json!({"page":"print"})).unwrap();
    let mut harness = crate::common::builder().with_size(egui::vec2(800.0, 700.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    assert!(!harness.get_by_label("Refresh printers").accesskit_node().is_disabled());
    assert!(!harness.get_by_label("Grayscale").accesskit_node().is_disabled());
    harness.get_by_label("Grayscale").click();
    harness.run_steps(2);
    assert!(harness.state().printing.settings.grayscale);
    assert!(harness.state().printing.ready());
}

#[test]
/// Every malformed print-settings request errors and leaves the prior settings unchanged.
fn invalid_print_settings_are_rejected_without_partial_changes() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.printing.printers.push(Printer { name: "Queue".into(), label: String::new(), default: true });
    let before = app.run("ui.printSettings", json!({})).unwrap();
    for params in [
        json!({"printer":"Missing"}),
        json!({"copies":0}),
        json!({"copies":1000}),
        json!({"copies":1.5}),
        json!({"copies":-1}),
        json!({"copies":"3"}),
        json!({"duplex":"maybe"}),
        json!({"grayscale":"yes"}),
        json!({"refresh":"yes"}),
        json!({"copies":3,"duplex":"invalid"}),
        json!({"from":0}),
        json!({"from":-1}),
        json!({"from":2.5}),
        json!({"from":"2"}),
        json!({"to":"x"}),
        json!({"to":true}),
        json!({"to":u64::MAX}),
        json!({"from":2,"to":0}),
        json!({"pages":2}),
        json!([]),
    ] {
        assert!(app.run("ui.printSettings", params.clone()).is_err(), "{params}");
        assert_eq!(app.run("ui.printSettings", json!({})).unwrap(), before, "no partial update: {params}");
    }
}

#[test]
/// Two fast jobs each surface the same receipt exactly once.
fn repeated_fast_jobs_each_surface_an_identical_receipt() {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.printing.printers.push(Printer { name: "Queue".into(), label: String::new(), default: true });
    app.run("ui.printSettings", json!({"printer":"Queue"})).unwrap();
    app.services.print_native = Some(Box::new(|_, _| {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Ok("same receipt".into())).unwrap();
        receiver
    }));
    for _ in 0..2 {
        app.toast = None;
        app.run("ui.printSubmit", json!({})).unwrap();
        // Completion may be consumed by the shared widget before the app's logic pass.
        app.printing.poll(&egui::Context::default());
        app.poll_printing(&egui::Context::default());
        assert!(app.toast.as_ref().unwrap().0.contains("same receipt"));
        app.toast = None;
        app.poll_printing(&egui::Context::default());
        assert!(app.toast.is_none());
    }
}

/// A workbook of 5 pages (A1:A50, a page break every 10 rows) whose footer numbers its pages.
fn five_pages() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    let rows: Vec<Vec<serde_json::Value>> = (1..=50).map(|r| vec![json!(format!("R{r}"))]).collect();
    session.run("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    session.run("pageLayout.pageSetup", json!({"rowBreaks": [11, 21, 31, 41], "footer": "&CPage &P of &N"})).unwrap();
    session
}

/// Replaces the text of the field labelled `label` with `text` (empty clears it), as typed.
fn type_into(h: &mut egui_kittest::Harness<'_, SheetApp>, label: &str, text: &str) {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let rect = h.get_by_role_and_label(Role::TextInput, label).rect();
    h.input_mut().events.push(egui::Event::PointerMoved(rect.center()));
    h.run_steps(1);
    h.get_by_role_and_label(Role::TextInput, label).click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(1);
    if text.is_empty() {
        h.key_press(egui::Key::Backspace);
    } else {
        h.input_mut().events.push(egui::Event::Text(text.into()));
    }
    h.run_steps(3);
}

/// The footers a PDF prints, in page order.
fn footers(pdf: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(pdf).lines().filter_map(|l| l.strip_prefix("(Page ")?.strip_suffix(") Tj")).map(|f| format!("Page {f}")).collect()
}

#[test]
/// `ui.printSettings` holds the Print view's page range: `from` and `to` are set, read back and
/// cleared with `null`, and `ui.printSubmit` prints that range unless the call names its own.
fn print_settings_hold_the_page_range_that_print_submit_prints() {
    let mut app = SheetApp::new(five_pages(), Services::default());
    app.printing.printers.push(Printer { name: "Queue".into(), label: String::new(), default: true });
    let jobs = Rc::new(RefCell::new(Vec::new()));
    let observed = jobs.clone();
    app.services.print_native = Some(Box::new(move |bytes, _| {
        observed.borrow_mut().push(bytes);
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Ok("queued".into())).unwrap();
        receiver
    }));
    let unknown = app.run("ui.printSettings", json!({"pages": 2})).unwrap_err();
    assert!(unknown.contains("from, to"), "the error lists the settings it takes: {unknown}");
    let set = app.run("ui.printSettings", json!({"printer": "Queue", "from": 2, "to": 3})).unwrap();
    assert_eq!((&set["from"], &set["to"]), (&json!(2), &json!(3)));
    assert_eq!(app.run("ui.printSettings", json!({})).unwrap()["from"], 2, "the range stays until changed");
    let submit = |app: &mut SheetApp, params: serde_json::Value| {
        app.run("ui.printSubmit", params).unwrap();
        app.printing.poll(&egui::Context::default());
        app.poll_printing(&egui::Context::default());
        footers(&jobs.borrow_mut().pop().unwrap())
    };
    assert_eq!(submit(&mut app, json!({})), ["Page 2 of 5", "Page 3 of 5"]);
    // A call that names a range prints it instead: `from` alone runs to the last page.
    assert_eq!(submit(&mut app, json!({"from": 4})), ["Page 4 of 5", "Page 5 of 5"]);
    assert_eq!(submit(&mut app, json!({"from": null})).len(), 5, "an explicit null prints every page");
    // `null` clears one bound, then the other.
    let set = app.run("ui.printSettings", json!({"to": null})).unwrap();
    assert_eq!((&set["from"], &set["to"]), (&json!(2), &json!(null)));
    assert_eq!(submit(&mut app, json!({})), ["Page 2 of 5", "Page 3 of 5", "Page 4 of 5", "Page 5 of 5"]);
    app.run("ui.printSettings", json!({"from": null})).unwrap();
    assert_eq!(app.print_pages, gridcraft_ui_egui::printing::PageRange::default());
    assert_eq!(submit(&mut app, json!({})).len(), 5);
    // A range after the last page is refused when printing, with nothing sent to the printer.
    app.run("ui.printSettings", json!({"from": 6})).unwrap();
    let e = app.run("ui.printSubmit", json!({})).unwrap_err();
    assert!(e.contains("nothing to print"), "{e}");
    assert!(jobs.borrow().is_empty());
}

#[test]
/// File › Print's Pages … to … fields set the page range, the preview shows the pages that will
/// print under their own numbers, and the fields follow `ui.printSettings`. In German too.
fn the_print_views_page_fields_set_the_range_and_the_preview_follows() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut app = crate::common::prepare(SheetApp::new(five_pages(), Services::default()));
    app.run("ui.backstage", json!({"page": "print"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    assert!(h.query_by_label("Page 1 of 5").is_some());
    type_into(&mut h, "Pages", "2");
    type_into(&mut h, "to", "3x");
    let range = h.state().print_pages;
    assert_eq!((range.from, range.to), (Some(2), Some(3)), "digits only");
    assert!(h.query_by_label("Page 2 of 5").is_some(), "the preview starts at the range's first page, under its own number");
    h.get_by_label("▶").click();
    h.run_steps(3);
    assert!(h.query_by_label("Page 3 of 5").is_some());
    assert!(h.get_by_label("▶").accesskit_node().is_disabled(), "page 3 is the last page that prints");
    assert!(h.get_all_by_role_and_label(Role::Button, "Print").all(|b| !b.accesskit_node().is_disabled()));
    // An emptied field has no bound; a range after the last page has nothing to print, and Print is off.
    type_into(&mut h, "to", "");
    assert_eq!(h.state().print_pages.to, None);
    type_into(&mut h, "Pages", "9");
    assert!(h.query_by_label("Nothing to print.").is_some());
    // The square Print button, not the navigation entry of the same name.
    assert!(h.get_all_by_role_and_label(Role::Button, "Print").any(|b| b.accesskit_node().is_disabled()));
    // The fields show what `ui.printSettings` sets once they are left (Tab moves on to "to").
    h.key_press(egui::Key::Tab);
    h.run_steps(2);
    h.state_mut().run("ui.printSettings", json!({"from": 4, "to": null})).unwrap();
    h.run_steps(3);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Pages").value().as_deref(), Some("4"));
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "to").value().as_deref(), Some(""));
    assert!(h.query_by_label("Page 4 of 5").is_some());
    // German labels.
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.run_steps(3);
    assert_eq!(h.get_by_role_and_label(Role::TextInput, "Seiten").value().as_deref(), Some("4"));
    assert!(h.query_by_role_and_label(Role::TextInput, "bis").is_some());
    assert!(h.query_by_label("Seite 4 von 5").is_some());
}

#[test]
/// Export › Create PDF exports the page range typed under it.
fn create_pdf_exports_the_page_range_under_it() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let dir = std::env::temp_dir().join(format!("gridcraft-export-range-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let target = dir.join("Range.pdf");
    let path = target.to_string_lossy().into_owned();
    let services = Services { pick_save: Some(Box::new(move |_| Some(path.clone()))), ..Default::default() };
    let mut app = crate::common::prepare(SheetApp::new(five_pages(), services));
    app.run("ui.backstage", json!({"page": "export"})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    type_into(&mut h, "Pages", "4");
    h.get_by_role_and_label(Role::Button, "Create PDF").click();
    h.run_steps(3);
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    assert_eq!(footers(&std::fs::read(&target).unwrap()), ["Page 4 of 5", "Page 5 of 5"]);
    assert_eq!(h.state().print_pages, gridcraft_ui_egui::printing::PageRange::default(), "File › Print keeps its own range");
    let _ = std::fs::remove_dir_all(&dir);
}
