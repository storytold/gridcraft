//! Headless UI test of the Ask Agent bubble flow: send, reply, keep, undo, and follow-up.

use std::time::Duration;

use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::ask_agent::{self, AgentHub};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(hub: AgentHub) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "B2", "values": [["Region", "Q1"], ["North", 120], ["South", 80]]})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, a: &mut SheetApp| {
            a.logic(ui.ctx());
            a.ui(ui);
        },
        SheetApp::new(session, Default::default()).with_agent_hub(hub),
    );
    h.run_steps(4);
    h
}

fn send(h: &mut Harness, text: &str) {
    h.state_mut().ask_agent.draft = text.into();
    h.run_steps(1);
    h.get_by_label("Send").click();
    h.run_steps(2);
}

fn step(h: &mut Harness, hub: &AgentHub, conn: u64, prompt: &str, cell: &str, val: &str) -> ask_agent::Request {
    send(h, prompt);
    let r = hub.listen(conn, Some("editor"), Duration::ZERO, || true).unwrap();
    ask_agent::run_for(h.state_mut(), conn, r.id, "cell.set", json!({"cell": cell, "input": val})).unwrap();
    hub.reply(conn, r.id, "done", true, None).unwrap();
    h.run_steps(2);
    r
}

fn cell_text(h: &mut Harness, cell: &str) -> String {
    h.state_mut().run("cell.get", json!({"cell": cell})).unwrap()["text"].as_str().unwrap().to_string()
}

#[test]
fn bubble_flow_send_reply_followup_undo_refusal_and_keep() {
    let hub = AgentHub::default();
    let conn = 1;
    hub.listen(conn, Some("editor"), Duration::ZERO, || true);
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range": "B2:C4"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);

    // 1. Send; while the agent works its cells are held, and its note lands on B2.
    send(&mut h, "clean values");
    let r = hub.listen(conn, Some("editor"), Duration::ZERO, || true).expect("request queued");
    assert_eq!((r.instruction.as_str(), r.selection["range"].as_str().unwrap()), ("clean values", "B2:C4"));
    hub.reply(conn, r.id, "Scanning...", false, None).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Scanning...").is_some());
    assert!(h.state_mut().run("cell.set", json!({"cell": "B3", "input": "x"})).unwrap_err().contains("Held by Agent (editor)"));
    ask_agent::run_for(h.state_mut(), conn, r.id, "cell.set", json!({"cell": "B3", "input": "north"})).unwrap();
    hub.reply(conn, r.id, "Cleaned row 3", true, None).unwrap();
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().undo.last().unwrap().label == "Ask Agent");

    // 2. Follow-up carries the thread.
    let r2 = step(&mut h, &hub, conn, "also clean south", "B4", "south");
    assert_eq!((r2.history.len(), r2.history[0].instruction.as_str()), (1, "clean values"));

    // 3. Undo reverts everything the agent did, across the request and its follow-up.
    h.get_by_label("Undo").click();
    h.run_steps(2);
    assert!(h.state().ask_agent.open.is_none() && cell_text(&mut h, "B3") == "North" && cell_text(&mut h, "B4") == "South");

    // 4. Undo is refused once the person edited after the agent.
    ask_agent::open_from_ui(h.state_mut());
    step(&mut h, &hub, conn, "clean values", "B3", "north");
    h.state_mut().run("cell.set", json!({"cell": "B3", "input": "human"})).unwrap();
    h.get_by_label("Undo").click();
    h.run_steps(1);
    assert!(h.state().toast.as_ref().unwrap().0.contains("changed") && cell_text(&mut h, "B3") == "human");

    // 5. Keep closes and keeps.
    ask_agent::open_from_ui(h.state_mut());
    step(&mut h, &hub, conn, "capitalise", "B4", "SOUTH");
    h.get_by_label("Keep").click();
    h.run_steps(2);
    assert!(h.state().ask_agent.open.is_none() && cell_text(&mut h, "B4") == "SOUTH");
}
