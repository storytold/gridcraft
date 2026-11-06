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
    let r = hub.listen(conn, Some("editor"), Duration::ZERO, || true).unwrap().unwrap();
    ask_agent::run_for(h.state_mut(), conn, r.id, "cell.set", json!({"cell": cell, "input": val})).unwrap();
    hub.reply(conn, r.id, ask_agent::Reply { text: "done", done: true, ..Default::default() }, ask_agent::current_target(h.state()).as_ref())
        .unwrap();
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
    let _ = hub.listen(conn, Some("editor"), Duration::ZERO, || true);
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range": "B2:C4"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);

    // 1. Send; while the agent works its cells are held, and its note lands on B2.
    send(&mut h, "clean values");
    let r = hub.listen(conn, Some("editor"), Duration::ZERO, || true).unwrap().expect("request queued");
    assert_eq!((r.instruction.as_str(), r.selection["range"].as_str().unwrap()), ("clean values", "B2:C4"));
    hub.reply(conn, r.id, ask_agent::Reply { text: "Scanning...", done: false, ..Default::default() }, ask_agent::current_target(h.state()).as_ref())
        .unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("Scanning...").is_some());
    assert!(h.state_mut().run("cell.set", json!({"cell": "B3", "input": "x"})).unwrap_err().contains("Held by Agent (editor)"));
    // Explicit target blocked even when current selection is outside the held range:
    h.state_mut().run("selection.set", json!({"cell": "Z1"})).unwrap();
    assert!(h.state_mut().run("cell.set", json!({"cell": "B3", "input": "x"})).unwrap_err().contains("Held by Agent (editor)"));
    assert!(
        h.state_mut().run("edit.undo", json!({})).unwrap_err().contains("Held by Agent"),
        "moving the selection must not bypass history ownership"
    );
    // In-cell typing is blocked by the hold as well:
    h.state_mut().run("selection.set", json!({"cell": "B3"})).unwrap();
    h.input_mut().events.push(egui::Event::Text("typed".into()));
    h.run_steps(2);
    h.input_mut().events.push(egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
    assert!(h.state().toast.as_ref().unwrap().0.contains("Held by Agent (editor)"));
    h.state_mut().cancel_edit();
    ask_agent::run_for(h.state_mut(), conn, r.id, "cell.set", json!({"cell": "B3", "input": "north"})).unwrap();
    hub.reply(
        conn,
        r.id,
        ask_agent::Reply { text: "Cleaned row 3", done: true, ..Default::default() },
        ask_agent::current_target(h.state()).as_ref(),
    )
    .unwrap();
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

    h.state_mut().run("ui.askAgent", json!({"close":true})).unwrap();
    // 5. Keep closes and keeps a newly selected cell.
    h.state_mut().run("selection.set", json!({"range":"B4"})).unwrap();
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
    let _ = hub.listen(conn, Some("editor"), Duration::ZERO, || true);
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range": "B2:C5"})).unwrap();
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
    let _ = hub.listen(conn, Some("editor"), Duration::ZERO, || true);
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range": "B2:C4"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);
    send(&mut h, "clean values");
    let r = hub.listen(conn, Some("editor"), Duration::ZERO, || true).unwrap().expect("request queued");
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
    hub.reply(conn, r.id, ask_agent::Reply { text: "done", done: true, ..Default::default() }, ask_agent::current_target(h.state()).as_ref())
        .unwrap();
    h.run_steps(2);
    h.state_mut().run("ui.askAgent", json!({"close": true})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    assert_eq!(h.state().ask_agent.open.as_ref().and_then(|o| o.id), None);
}

#[test]
fn failed_partial_edits_remain_reviewable_and_undoable() {
    let hub = AgentHub::default();
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range":"B3"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);
    send(&mut h, "edit this cell");
    let request = hub.listen(31, Some("worker"), Duration::ZERO, || true).unwrap().unwrap();
    ask_agent::run_for(h.state_mut(), 31, request.id, "cell.set", json!({"input":"partial"})).unwrap();
    let current = ask_agent::current_target(h.state());
    hub.reply(31, request.id, ask_agent::Reply { error: Some("<script>literal failure</script>"), ..Default::default() }, current.as_ref()).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label_contains("<script>literal failure</script>").is_some());
    let note = h.state().session.active().unwrap().wb.active().unwrap().comments.values().next().unwrap();
    assert!(note.text.contains("Error: <script>literal failure</script>"), "the kept partial edits retain their literal failure explanation");
    assert!(h.query_by_label("Keep").is_some());
    click_bubble_undo(&mut h);
    h.run_steps(2);
    assert_eq!(cell_text(&mut h, "B3"), "North");
    assert!(h.state().ask_agent.open.is_none());
}

#[test]
fn failed_review_rejects_reused_undo_depth_after_a_human_edit() {
    let hub = AgentHub::default();
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range":"B3"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);
    send(&mut h, "edit this cell");
    let request = hub.listen(32, Some("worker"), Duration::ZERO, || true).unwrap().unwrap();
    ask_agent::run_for(h.state_mut(), 32, request.id, "cell.set", json!({"input":"partial"})).unwrap();
    let current = ask_agent::current_target(h.state());
    hub.reply(32, request.id, ask_agent::Reply { error: Some("failed"), ..Default::default() }, current.as_ref()).unwrap();
    let depth = h.state().session.active().unwrap().undo.len();
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.state_mut().run("cell.set", json!({"cell":"B3","input":"human"})).unwrap();
    assert_eq!(h.state().session.active().unwrap().undo.len(), depth);
    h.run_steps(2);
    click_bubble_undo(&mut h);
    h.run_steps(2);
    assert_eq!(cell_text(&mut h, "B3"), "human");
    assert!(h.state().ask_agent.open.is_some());
    assert!(ask_agent::command(h.state_mut(), &json!({"close":true,"_externalAgent":true})).unwrap_err().starts_with("HUMAN_REVIEW_REQUIRED"));
    h.get_by_label("Keep").click();
    h.run_steps(2);
    assert!(h.state().ask_agent.open.is_some(), "Keep must enforce the same provenance fence");
}

#[test]
fn captured_scope_is_not_replaced_by_selection_or_busy_instructions() {
    let hub = AgentHub::default();
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range":"B3"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);
    h.state_mut().run("selection.set", json!({"range":"Z1"})).unwrap();
    send(&mut h, "edit original selection");
    let request = hub.listen(33, Some("worker"), Duration::ZERO, || true).unwrap().unwrap();
    assert_eq!(request.selection["range"], "B3");
    assert!(ask_agent::command(h.state_mut(), &json!({"instruction":"replace busy thread"})).unwrap_err().starts_with("HELD"));
    assert_eq!(h.state().ask_agent.open.as_ref().unwrap().id, Some(request.id));
    for (command, params) in [
        ("cell.set", json!({"range":"B3","cell":"Z1","input":"bad"})),
        ("range.setValues", json!({"range":"B3","values":[[1],[2]]})),
        ("edit.undo", json!({})),
    ] {
        assert!(ask_agent::run_for(h.state_mut(), 33, request.id, command, params).unwrap_err().starts_with("OUTSIDE_SELECTION"));
    }
    assert!(ask_agent::run_for(h.state_mut(), 33, request.id, "home.bold", json!([])).unwrap_err().starts_with("INVALID_PARAMS"));
    h.state_mut().run("selection.set", json!({"range":"A1:Z99"})).unwrap();
    assert!(
        ask_agent::run_for(h.state_mut(), 33, request.id, "cell.set", json!({"cell":"B3","input":"=1","array":true}))
            .unwrap_err()
            .starts_with("OUTSIDE_SELECTION")
    );
    let outcome = gridcraft_ui_egui::control::handle(
        h.state_mut(),
        &egui::Context::default(),
        "engine.execute",
        &json!({"request":"invalid","_externalAgent":true,"command":"home.insertRows","params":{"rows":"1:1"}}),
    );
    let gridcraft_ui_egui::control::Outcome::Done(outcome) = outcome else { panic!("expected a direct error") };
    assert!(outcome["error"].as_str().unwrap().starts_with("INVALID_PARAMS"));
    // A misleading range cannot move a cell.set with its cell omitted to the live selection.
    ask_agent::run_for(h.state_mut(), 33, request.id, "cell.set", json!({"range":"Z1","input":"captured"})).unwrap();
    assert_eq!(cell_text(&mut h, "B3"), "captured");
    assert_eq!(cell_text(&mut h, "Z1"), "");
}

#[test]
fn answer_notes_wait_for_the_original_workbook() {
    let hub = AgentHub::default();
    let mut h = harness(hub.clone());
    h.state_mut().run("selection.set", json!({"range":"B3"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);
    send(&mut h, "edit original book");
    let request = hub.listen(34, Some("worker"), Duration::ZERO, || true).unwrap().unwrap();
    let original = h.state().session.active().unwrap().uid;
    ask_agent::run_for(h.state_mut(), 34, request.id, "cell.set", json!({"input":"changed"})).unwrap();
    h.state_mut().session.new_workbook();
    let reply = ask_agent::handle_reply(h.state(), 34, &json!({"id":request.id,"text":"Finished"}));
    assert_eq!(reply["ok"], true);
    h.run_steps(2);
    assert!(h.state().session.active().unwrap().wb.active().unwrap().comments.is_empty());
    let _guard = h.state_mut().session.enter_target(gridcraft_engine::Target::Doc(original)).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().comments.len(), 1);
}

#[test]
/// A trimmed checkpoint cannot be mistaken for a newer entry at the same undo depth.
fn trimming_history_during_agent_edits_refuses_partial_review() {
    let hub = AgentHub::default();
    let conn = 88;
    let mut h = harness(hub.clone());
    h.state_mut().session.active_mut().unwrap().undo.clear();
    h.state_mut().run("selection.set", json!({"range":"B3:B4"})).unwrap();
    ask_agent::open_from_ui(h.state_mut());
    h.run_steps(2);
    send(&mut h, "edit both selected cells");
    let request = hub.listen(conn, Some("editor"), Duration::ZERO, || true).unwrap().unwrap();
    ask_agent::run_for(h.state_mut(), conn, request.id, "cell.set", json!({"cell":"B3","input":"agent north"})).unwrap();
    // Simulate the retained-size estimate of a large checkpoint; the next real edit
    // runs the engine's normal history-budget trim without allocating a huge workbook.
    h.state_mut().session.active_mut().unwrap().undo.last_mut().unwrap().retained_bytes = 129 * 1024 * 1024;
    ask_agent::run_for(h.state_mut(), conn, request.id, "cell.set", json!({"cell":"B4","input":"agent south"})).unwrap();
    assert_eq!(h.state().session.active().unwrap().undo.len(), 1, "the engine actually trimmed the first checkpoint");
    hub.reply(conn, request.id, ask_agent::Reply { text: "done", done: true, ..Default::default() }, ask_agent::current_target(h.state()).as_ref())
        .unwrap();
    h.run_steps(2);
    click_bubble_undo(&mut h);
    h.run_steps(2);
    assert_eq!(cell_text(&mut h, "B3"), "agent north");
    assert_eq!(cell_text(&mut h, "B4"), "agent south", "a lost checkpoint must not allow an incomplete Undo");
    assert!(h.state().ask_agent.open.is_some(), "the unresolved review stays available");
    assert!(h.state().toast.as_ref().unwrap().0.contains("changed"));
    h.get_by_label("Keep").click();
    h.run_steps(2);
    assert!(h.state().ask_agent.open.is_some(), "Keep uses the same checkpoint guard");
}
