//! Copy/cut route rich tables to the host without a later egui text write replacing them.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Event, OutputCommand};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

fn harness(services: Services) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Score"], ["Ada", 42]]})).unwrap();
    session.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
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

#[test]
fn copy_and_cut_publish_html_and_text_without_overwriting_rich_clipboard() {
    for event in [Event::Copy, Event::Cut] {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let received = calls.clone();
        let mut h = harness(Services {
            copy_html: Some(Box::new(move |html, text| {
                received.borrow_mut().push((html.to_string(), text.to_string()));
                Ok(())
            })),
            ..Default::default()
        });
        h.input_mut().events.push(event.clone());
        h.step();
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].0.contains("<table"));
        assert!(calls[0].0.contains("Ada"));
        assert_eq!(calls[0].1, "Name\tScore\nAda\t42\n");
        assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(_))));
        assert_eq!(h.state().session.clipboard.as_ref().map(|c| c.cut), Some(matches!(event, Event::Cut)));
    }
}

#[test]
fn missing_or_failed_rich_clipboard_falls_back_to_plain_text() {
    for fail in [false, true] {
        let mut services = Services::default();
        if fail {
            services.copy_html = Some(Box::new(|_, _| Err("clipboard unavailable".into())));
        }
        let mut h = harness(services);
        h.input_mut().events.push(Event::Copy);
        h.step();
        assert!(
            h.output().platform_output.commands.iter().any(|c| { matches!(c, OutputCommand::CopyText(text) if text == "Name\tScore\nAda\t42\n") })
        );
    }
}

#[test]
fn programmatic_copy_does_not_write_to_the_system_clipboard() {
    let calls = Rc::new(RefCell::new(0));
    let received = calls.clone();
    let mut h = harness(Services {
        copy_html: Some(Box::new(move |_, _| {
            *received.borrow_mut() += 1;
            Ok(())
        })),
        ..Default::default()
    });
    let copied = h.state_mut().run("edit.copy", json!({})).unwrap();
    assert!(copied.get("html").is_none(), "HTML is only built when requested");
    let copied = h.state_mut().run("edit.copy", json!({"html": true})).unwrap();
    assert!(copied["html"].is_string());
    assert_eq!(*calls.borrow(), 0);
    assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(_))));
}
