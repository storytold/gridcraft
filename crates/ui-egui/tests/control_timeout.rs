//! A control request whose sender stopped waiting is dropped unrun (#1914).

use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{ControlRequest, SheetApp};
use serde_json::json;

fn app() -> (SheetApp, egui::Context) {
    let mut s = Session::new();
    s.new_workbook();
    (SheetApp::new(s, Default::default()), egui::Context::default())
}

fn a1(app: &SheetApp) -> String {
    let d = app.session.active().unwrap();
    d.wb.active().unwrap().cell(CellRef::parse("A1").unwrap()).map(|c| format!("{:?}", c.value)).unwrap_or_else(|| "empty".into())
}

/// `set` writes A1, so a request that did run is visible in the workbook.
fn set_a1(text: &str) -> serde_json::Value {
    json!({"command": "cell.set", "params": {"cell": "A1", "input": text}})
}

#[test]
fn a_request_its_sender_gave_up_on_does_not_run_later() {
    let (mut app, ctx) = app();
    let (tx, rx) = std::sync::mpsc::channel();
    app.control_rx = Some(rx);

    // One request the sender is still waiting for, and one it gave up on after its reply timed
    // out. Before the fix both ran on the next UI pass: the client was told its command had
    // failed and the command was applied anyway, so an agent that retried ran it twice.
    let (kept, kept_rx) = ControlRequest::new("engine.execute", set_a1("kept"));
    tx.send(kept).unwrap();
    gridcraft_ui_egui::control::poll(&mut app, &ctx);
    assert!(kept_rx.try_recv().is_ok(), "a request still waited for is answered");
    assert_eq!(a1(&app), "Text(\"kept\")");

    let (dropped, dropped_rx) = ControlRequest::new("engine.execute", set_a1("gave up"));
    assert!(ControlRequest::abandon(&dropped.state), "the sender gives up while it is queued");
    tx.send(dropped).unwrap();
    gridcraft_ui_egui::control::poll(&mut app, &ctx);
    assert_eq!(a1(&app), "Text(\"kept\")", "the abandoned request did not run");
    assert!(dropped_rx.try_recv().is_err(), "and nothing was replied to it");
}

/// Which of the two sides wins the race is settled once: a request the app has started is not
/// abandoned (the app will answer, so the sender reports an unknown outcome instead of a failure),
/// and one already abandoned is never started.
#[test]
fn taking_and_abandoning_a_request_settle_once() {
    let (taken, _rx) = ControlRequest::new("engine.execute", json!({}));
    assert!(taken.take(), "a pending request can be taken");
    assert!(taken.take(), "and taking it again is the same answer");
    assert!(!ControlRequest::abandon(&taken.state), "it cannot then be abandoned");

    let (given_up, _rx) = ControlRequest::new("engine.execute", json!({}));
    assert!(ControlRequest::abandon(&given_up.state), "a pending request can be abandoned");
    assert!(ControlRequest::abandon(&given_up.state), "and again is the same answer");
    assert!(!given_up.take(), "it cannot then be taken");
}
