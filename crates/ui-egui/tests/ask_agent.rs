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

/// The bubble's Undo; the title bar has a button with the same label, and the bubble is drawn on top.
fn click_bubble_undo(h: &mut Harness) {
    h.get_all_by_label("Undo").last().expect("the bubble's Undo button").click();
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
    // Explicit target blocked even when current selection is outside the held range:
    h.state_mut().run("selection.set", json!({"cell": "Z1"})).unwrap();
    assert!(h.state_mut().run("cell.set", json!({"cell": "B3", "input": "x"})).unwrap_err().contains("Held by Agent (editor)"));
    // In-cell typing is blocked by the hold as well:
    h.state_mut().run("selection.set", json!({"cell": "B3"})).unwrap();
    h.state_mut().begin_edit(Some("typed".into()), false);
    assert!(!h.state_mut().commit_edit(0, 0, false, false));
    assert!(h.state().toast.as_ref().unwrap().0.contains("Held by Agent (editor)"));
    h.state_mut().cancel_edit();
    ask_agent::run_for(h.state_mut(), conn, r.id, "cell.set", json!({"cell": "B3", "input": "north"})).unwrap();
    hub.reply(conn, r.id, "Cleaned row 3", true, None).unwrap();
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().undo.last().unwrap().label == "Ask Agent");

    // 2. Follow-up carries the thread.
    let r2 = step(&mut h, &hub, conn, "also clean south", "B4", "south");
    assert_eq!((r2.history.len(), r2.history[0].instruction.as_str()), (1, "clean values"));

    // 3. Undo reverts everything the agent did, across the request and its follow-up.
    click_bubble_undo(&mut h);
    h.run_steps(2);
    assert!(h.state().ask_agent.open.is_none() && cell_text(&mut h, "B3") == "North" && cell_text(&mut h, "B4") == "South");

    // 4. Undo is refused once the person edited after the agent.
    ask_agent::open_from_ui(h.state_mut());
    step(&mut h, &hub, conn, "clean values", "B3", "north");
    h.state_mut().run("cell.set", json!({"cell": "B3", "input": "human"})).unwrap();
    click_bubble_undo(&mut h);
    h.run_steps(1);
    assert!(h.state().toast.as_ref().unwrap().0.contains("changed") && cell_text(&mut h, "B3") == "human");

    // 5. Keep closes and keeps.
    ask_agent::open_from_ui(h.state_mut());
    step(&mut h, &hub, conn, "capitalise", "B4", "SOUTH");
    h.get_by_label("Keep").click();
    h.run_steps(2);
    assert!(h.state().ask_agent.open.is_none() && cell_text(&mut h, "B4") == "SOUTH");
}

#[test]
fn a_persons_edit_between_agent_turns_keeps_its_own_undo_step() {
    let hub = AgentHub::default();
    let conn = 7;
    hub.listen(conn, Some("editor"), Duration::ZERO, || true);
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range": "B2:C4"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);

    // 1. The agent edits (its steps and its note are one undo step), then the person edits.
    step(&mut h, &hub, conn, "clean values", "B3", "north");
    let after_agent = h.state().session.active().unwrap().undo.len();
    h.state_mut().run("cell.set", json!({"cell": "B4", "input": "human"})).unwrap();
    assert_eq!(h.state().session.active().unwrap().undo.len(), after_agent + 1);

    // 2. A later turn of the same thread must not merge into, or drop, the person's step.
    step(&mut h, &hub, conn, "also south", "B5", "south");
    assert_eq!(h.state().session.active().unwrap().undo.len(), after_agent + 2, "the person's step survived");

    // 3. Undo takes back the agent's last turn, then the person's own edit, then the first turn.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!((cell_text(&mut h, "B4"), cell_text(&mut h, "B5")), ("human".into(), String::new()));
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!((cell_text(&mut h, "B3"), cell_text(&mut h, "B4")), ("north".into(), "South".into()));
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(cell_text(&mut h, "B3"), "North");
}

#[test]
fn reopening_keeps_a_bubble_that_owns_a_request() {
    let hub = AgentHub::default();
    let conn = 9;
    hub.listen(conn, Some("editor"), Duration::ZERO, || true);
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range": "B2:C4"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);
    send(&mut h, "clean values");
    let r = hub.listen(conn, Some("editor"), Duration::ZERO, || true).expect("request queued");
    let before = h.state().ask_agent.open.as_ref().and_then(|o| o.id);
    assert_eq!(before, Some(r.id));

    // Reopening from the ribbon, the menu or the control command must not orphan the request:
    // the hub would keep it held and every write without its id would stay refused.
    ask_agent::open_from_ui(h.state_mut());
    assert_eq!(h.state().ask_agent.open.as_ref().and_then(|o| o.id), before, "the bubble kept its request");
    assert!(hub.snapshot(r.id).is_some(), "the request is still in the hub");
    let opened = h.state_mut().run("ui.askAgent", json!({})).unwrap();
    assert_eq!(opened["id"].as_u64(), Some(r.id));

    // A bubble with no request may still re-anchor to the selection.
    hub.reply(conn, r.id, "done", true, None).unwrap();
    h.run_steps(2);
    h.state_mut().run("ui.askAgent", json!({"close": true})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    assert_eq!(h.state().ask_agent.open.as_ref().and_then(|o| o.id), None);
}
