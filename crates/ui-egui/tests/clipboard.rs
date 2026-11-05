//! Copy/cut route rich tables to the host without a later egui text write replacing them.

use std::cell::RefCell;
use std::rc::Rc;

use egui::{Event, OutputCommand};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::{CellRuns, Color, Run, RunFont};
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
fn copy_publishes_run_spans_with_the_cells_own_style() {
    // Issue #907: rich runs copy out as per-run <span> styles through the real path
    // (Event::Copy → edit.copy {"html":true} → Services::copy_html), not just in the engine's io.
    let mut session = Session::new();
    session.new_workbook();
    let text = "plain red plain";
    session.execute("range.setValues", json!({"range": "A1", "values": [[text]]})).unwrap();
    session.execute("home.bold", json!({"range": "A1"})).unwrap();
    {
        // Seed runs after the value: a command-set value drops the cell's runs entry.
        let d = session.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let sh = wb.sheet_mut(0).unwrap();
        let plain = RunFont { bold: Some(false), ..RunFont::default() };
        let red = RunFont { color: Some(Color::rgb(255, 0, 0)), ..RunFont::default() };
        sh.cell_runs.insert(
            CellRef::new(0, 0),
            CellRuns {
                runs: vec![Run { start: 0, len: 6, font: plain.clone() }, Run { start: 6, len: 3, font: red }, Run { start: 9, len: 6, font: plain }],
            },
        );
    }
    session.execute("selection.set", json!({"range": "A1:A1"})).unwrap();
    let calls = Rc::new(RefCell::new(Vec::new()));
    let received = calls.clone();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(
            session,
            Services {
                copy_html: Some(Box::new(move |html, _text| {
                    received.borrow_mut().push(html.to_string());
                    Ok(())
                })),
                ..Default::default()
            },
        ),
    );
    h.run_steps(4);
    h.input_mut().events.push(Event::Copy);
    h.step();
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    // The <td> carries the cell's own bold; the run's Some(false) must switch it off inside.
    assert!(calls[0].contains("font-weight:bold;"), "cell style on the td, got: {}", calls[0]);
    assert!(calls[0].contains("<span style=\"font-weight:normal;\">plain </span>"), "got: {}", calls[0]);
    assert!(calls[0].contains("<span style=\"color:#FF0000;\">red</span>"), "got: {}", calls[0]);
    assert_eq!(calls[0].matches("<span").count(), 3, "all three runs emit spans, got: {}", calls[0]);
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

#[test]
fn pasting_reads_the_clipboard_html_for_the_numbers_behind_the_text() {
    // #257: currency text from another spreadsheet stays a number when the host offers the HTML.
    let mut h = harness(Services {
        paste_html: Some(Box::new(|| Some(r#"<table><tr><td x:num="10000">Rp10,000.00</td></tr></table>"#.to_string()))),
        ..Default::default()
    });
    h.state_mut().run("selection.set", json!({"cell": "D5"})).unwrap();
    h.input_mut().events.push(Event::Paste("Rp10,000.00".into()));
    h.run_steps(2);
    let value = h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(gridcraft_engine::core::CellRef::new(4, 3))));
    assert_eq!(value, Some(gridcraft_engine::core::Value::number(10000.0)));
}

#[test]
fn copying_multiple_areas_copies_lined_up_areas_and_explains_the_rest() {
    // Issue #348: lined-up areas copy as one block; others get Excel's message instead of nothing.
    let mut h = harness(Services::default());
    h.state_mut().session.execute("selection.set", json!({"range": "B1:B2,A1:A2"})).unwrap();
    h.input_mut().events.push(Event::Copy);
    h.step();
    assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(text) if text == "Name\tScore\nAda\t42\n")));
    assert!(h.state().message.is_none());
    assert_eq!(h.state().session.clipboard.as_ref().map(|c| c.areas.len()), Some(2));

    h.state_mut().session.execute("selection.set", json!({"range": "A1,B2"})).unwrap();
    h.input_mut().events.push(Event::Copy);
    h.step();
    assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(_))));
    assert_eq!(h.state().message.as_ref().map(|(_, text)| text.as_str()), Some("This action won't work on multiple selections."));
    assert_eq!(h.state().session.clipboard.as_ref().map(|c| c.areas.len()), Some(2), "the earlier copy stays");
}
