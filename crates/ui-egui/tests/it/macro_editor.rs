//! The macro step editor: editing, Add Step, breakpoints, dragging steps, and the debugger
//! (step, continue, run to the end as one undo step, stop with keep or undo).

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{Step, StepKind};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    harness_with(Default::default())
}

fn harness_with(services: Services) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, services);
    app.ui.ribbon_tab = "Automate".into();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

fn value(h: &Harness, a: &str) -> Value {
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap_or_default()))).unwrap_or_default()
}

/// Three steps that write A1, A2, A3.
fn three(h: &mut Harness) {
    let steps: Vec<_> = ["A1", "A2", "A3"].iter().map(|c| json!({"command": "cell.set", "params": {"cell": c, "input": c}})).collect();
    h.state_mut().run("macro.save", json!({"macro": {"name": "Three", "output": {"onConflict": "overwrite"}, "steps": steps}})).unwrap();
    h.state_mut().open_dialog("macroEditor", json!({"macro": "Three"}));
    h.run_steps(3);
}

fn draft_cells(h: &Harness) -> Vec<String> {
    let ed = h.state().macros.editor.as_ref().unwrap();
    ed.draft
        .steps
        .iter()
        .map(|s| match &s.kind {
            StepKind::Command { params, .. } => params["cell"].as_str().unwrap_or("").to_string(),
            _ => String::new(),
        })
        .collect()
}

fn click(h: &mut Harness, label: &str) {
    h.get_by_label(label).click();
    h.run_steps(3);
}

#[test]
fn new_macro_add_step_through_the_picker_and_save() {
    let mut h = harness();
    click(&mut h, "New Macro");
    assert!(h.state().macros.editor.is_some());
    click(&mut h, "Add Step");
    h.input_mut().events.push(Event::Text("bold".into()));
    h.run_steps(2);
    h.input_mut().events.push(Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    let ed = h.state().macros.editor.clone().unwrap();
    assert!(matches!(&ed.draft.steps[..], [s] if matches!(&s.kind, StepKind::Command { id, .. } if id == "home.bold")));
    assert_eq!(ed.selected.as_deref(), Some("1"));
    click(&mut h, "Save");
    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.macros.len(), 1);
    assert_eq!(wb.macros[0].name, ed.draft.name);
    assert!(!h.state().macros.editor.as_ref().unwrap().dirty);
}

#[test]
fn the_gutter_toggles_breakpoints() {
    let mut h = harness();
    three(&mut h);
    click(&mut h, "Breakpoint 2");
    assert!(h.state().macros.editor.as_ref().unwrap().draft.steps[1].breakpoint);
    click(&mut h, "Breakpoint 2");
    assert!(!h.state().macros.editor.as_ref().unwrap().draft.steps[1].breakpoint);
}

#[test]
fn dragging_a_step_reorders_the_list() {
    let mut h = harness();
    three(&mut h);
    let from = h.get_by_label("Step 1: Set A1 to A1").rect().center();
    let to = h.get_by_label("Step 3: Set A3 to A3").rect().center() + egui::vec2(0.0, 8.0);
    h.input_mut().events.push(Event::PointerMoved(from));
    h.input_mut().events.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    for k in 1..=6 {
        let p = from + (to - from) * (k as f32 / 6.0);
        h.input_mut().events.push(Event::PointerMoved(p));
        h.run_steps(1);
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(draft_cells(&h), ["A2", "A3", "A1"]);
    let ed = h.state().macros.editor.as_ref().unwrap();
    assert!(ed.dirty);
    assert_eq!(ed.selected.as_deref(), Some("3"), "the moved step stays selected");
}

#[test]
fn step_continue_and_finish_as_one_undo_step() {
    let mut h = harness();
    three(&mut h);
    click(&mut h, "Breakpoint 3");
    let undo_before = h.state().session.active().unwrap().undo.len();
    // Continue starts the run (saving the breakpoint first) and stops before step 3.
    click(&mut h, "Continue");
    let ed = h.state().macros.editor.clone().unwrap();
    assert_eq!(ed.debug.as_ref().and_then(|d| d["current"]["step"].as_str()), Some("3"));
    assert_eq!(ed.selected.as_deref(), Some("3"), "the current step is selected");
    assert!(h.query_by_label_contains("Paused before step 3").is_some());
    assert_eq!((value(&h, "A1"), value(&h, "A3")), (Value::Text("A1".into()), Value::Empty));
    assert!(h.state().grid.macro_overlay.as_ref().is_some_and(|o| !o.target.is_empty()), "the target is outlined");
    // Clearing the breakpoint while paused, then Step runs step 3 and the run finishes.
    click(&mut h, "Breakpoint 3");
    click(&mut h, "Step");
    assert!(h.state().session.macro_debug.is_none());
    assert_eq!(value(&h, "A3"), Value::Text("A3".into()));
    let ed = h.state().macros.editor.clone().unwrap();
    assert!(ed.outcome.as_deref().is_some_and(|o| o.starts_with("Finished")), "{:?}", ed.outcome);
    let doc = h.state().session.active().unwrap();
    // The breakpoint save is one entry, the run another.
    assert_eq!(doc.undo.last().map(|e| e.label.as_str()), Some("Run Macro: Three"));
    assert!(doc.undo.len() <= undo_before + 2);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!((value(&h, "A1"), value(&h, "A3")), (Value::Empty, Value::Empty));
}

#[test]
fn run_to_here_then_stop_asks_whether_to_keep_the_changes() {
    for keep in [false, true] {
        let mut h = harness();
        three(&mut h);
        click(&mut h, "Step 3: Set A3 to A3");
        click(&mut h, "Run to Here");
        assert_eq!(
            h.state().macros.editor.as_ref().unwrap().debug.as_ref().and_then(|d| d["current"]["step"].as_str()),
            Some("3"),
            "{:?} {:?}",
            h.state().message,
            h.state().macros.editor.as_ref().unwrap().outcome
        );
        click(&mut h, "Stop");
        assert!(h.state().macros.editor.as_ref().unwrap().ask_stop);
        click(&mut h, if keep { "Keep Changes" } else { "Undo Changes" });
        assert!(h.state().session.macro_debug.is_none());
        let want = if keep { Value::Text("A2".into()) } else { Value::Empty };
        assert_eq!(value(&h, "A2"), want, "keep = {keep}");
        assert_eq!(value(&h, "A3"), Value::Empty);
    }
}

/// Step 3's Cell box is picking from the grid, with the selection it last saw (A1).
fn pick_step_3_cell(h: &mut Harness) {
    let refs = &mut h.state_mut().macros.editor.as_mut().unwrap().refs;
    refs.pick = Some("ref:3:cell".into());
    refs.seen = "$A$1".into();
}

#[test]
fn a_pick_ends_when_its_box_leaves_the_screen() {
    // Choosing another step hides the picking box; a selection made meanwhile must not land in
    // it when the step is shown again.
    let mut h = harness();
    three(&mut h);
    click(&mut h, "Step 3: Set A3 to A3");
    pick_step_3_cell(&mut h);
    h.run_steps(2);
    click(&mut h, "Step 1: Set A1 to A1");
    assert!(h.state().macros.editor.as_ref().unwrap().refs.pick.is_none(), "the pick ends with its box");
    h.state_mut().run("selection.set", json!({"range": "F9"})).unwrap();
    h.run_steps(2);
    click(&mut h, "Step 3: Set A3 to A3");
    let ed = h.state().macros.editor.as_ref().unwrap();
    assert!(!ed.dirty, "the draft stays clean");
    assert_eq!(draft_cells(&h), ["A1", "A2", "A3"]);
}

#[test]
fn debugging_never_writes_the_selection_into_a_reference_box() {
    // A reference box that was picking from the grid stays out of it once an action starts, and
    // the selections the debugger makes while it runs never land in a box or dirty the draft.
    let mut h = harness();
    three(&mut h);
    click(&mut h, "Step 3: Set A3 to A3");
    pick_step_3_cell(&mut h);
    h.run_steps(2);
    assert_eq!(draft_cells(&h), ["A1", "A2", "A3"]);
    click(&mut h, "Run to Here");
    assert!(h.state().session.macro_debug.is_some());
    let ed = h.state().macros.editor.as_ref().unwrap();
    assert!(ed.refs.pick.is_none(), "the action ends the pick");
    assert!(!ed.dirty);
    // A box focused while debugging does not take the selection either.
    pick_step_3_cell(&mut h);
    h.state_mut().run("selection.set", json!({"range": "F9"})).unwrap();
    h.run_steps(3);
    let ed = h.state().macros.editor.as_ref().unwrap();
    assert!(!ed.dirty, "the draft stays clean");
    assert_eq!(draft_cells(&h), ["A1", "A2", "A3"]);
    h.state_mut().macros.editor.as_mut().unwrap().refs.pick = None;
    click(&mut h, "Stop");
    click(&mut h, "Undo Changes");
    assert!(h.state().session.macro_debug.is_none());
}

#[test]
fn step_into_from_the_macros_dialog_pauses_before_the_first_step() {
    let mut h = harness();
    three(&mut h);
    h.state_mut().dialog = None;
    h.state_mut().macros.editor = None;
    h.state_mut().open_dialog("macros", json!({"name": "Three"}));
    h.run_steps(2);
    click(&mut h, "Step Into");
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("macroEditor"));
    assert!(h.state().session.macro_debug.is_some());
    assert!(h.query_by_label_contains("Debugging “Three”").is_some(), "the status bar says so");
}

#[test]
fn a_recorded_macro_opens_in_the_editor() {
    let mut h = harness();
    click(&mut h, "Record Macro");
    // The Record Macro dialog comes first.
    h.run_steps(2);
    click(&mut h, "OK");
    assert!(h.state().session.is_recording());
    h.state_mut().run("cell.set", json!({"cell": "B2", "input": "5"})).unwrap();
    h.state_mut().run("home.bold", json!({})).unwrap();
    h.run_steps(2);
    click(&mut h, "Stop");
    let name = h.state().session.active().unwrap().wb.macros[0].name.clone();
    h.state_mut().open_dialog("macroEditor", json!({"macro": name}));
    h.run_steps(3);
    let ed = h.state().macros.editor.clone().unwrap();
    assert!(!ed.draft.steps.is_empty());
    assert!(!ed.dirty);
}

#[test]
fn run_from_the_editor_asks_before_writing_over_data() {
    let mut h = harness();
    h.state_mut().run("cell.set", json!({"cell": "A2", "input": "keep me"})).unwrap();
    let steps = vec![json!({"command": "cell.set", "params": {"cell": "A2", "input": "new"}})];
    h.state_mut().run("macro.save", json!({"macro": {"name": "Over", "steps": steps}})).unwrap();
    h.state_mut().open_dialog("macroEditor", json!({"macro": "Over", "check": true}));
    h.run_steps(3);
    // Check marks the cell it would write over.
    let o = h.state().grid.macro_overlay.clone().unwrap();
    assert_eq!(o.conflicts.iter().map(|(_, r)| r.a1()).collect::<Vec<_>>(), ["A2"]);
    click(&mut h, "Run");
    assert!(h.state().macros.question.as_ref().is_some_and(|q| q.choices.len() == 3));
    click(&mut h, "Overwrite");
    assert_eq!(value(&h, "A2"), Value::Text("new".into()));
}

#[test]
fn a_new_macro_never_replaces_one_with_its_name() {
    let mut h = harness();
    three(&mut h);
    h.state_mut().macros.editor = None;
    h.state_mut().dialog = None;
    h.run_steps(2);
    click(&mut h, "New Macro");
    for button in ["Save", "Check", "Run"] {
        {
            let ed = h.state_mut().macros.editor.as_mut().unwrap();
            ed.draft.name = "three".into();
            ed.draft.steps = vec![Step::command("cell.set", json!({"cell": "B1", "input": "x"}))];
            ed.dirty = true;
        }
        h.run_steps(2);
        click(&mut h, button);
        let wb = &h.state().session.active().unwrap().wb;
        assert_eq!(wb.macros.len(), 1, "{button}");
        assert_eq!(wb.macros[0].name, "Three", "{button}");
        assert_eq!(wb.macros[0].steps.len(), 3, "{button}: the saved macro is intact");
        assert!(value(&h, "B1").is_empty(), "{button}: nothing ran");
        let msg = h.state_mut().message.take().map(|(_, m)| m).unwrap_or_default();
        assert!(msg.contains("A macro called “three” already exists. Choose another name."), "{button}: {msg}");
        assert!(h.state().macros.editor.as_ref().unwrap().saved_as.is_none());
        h.run_steps(2);
    }
}

#[test]
fn a_rename_that_lands_before_a_failed_save_is_remembered() {
    let mut h = harness();
    three(&mut h);
    let big = "x".repeat(5 * 1024 * 1024);
    {
        let ed = h.state_mut().macros.editor.as_mut().unwrap();
        ed.draft.name = "Renamed".into();
        // Too large to keep in the workbook: the rename lands, the save after it fails.
        ed.draft.steps.push(Step::command("cell.set", json!({"cell": "A4", "input": big})));
        ed.dirty = true;
    }
    h.run_steps(2);
    click(&mut h, "Save");
    let msg = h.state_mut().message.take().map(|(_, m)| m).unwrap_or_default();
    assert!(msg.contains("too large"), "{msg}");
    assert_eq!(h.state().macros.editor.as_ref().unwrap().saved_as.as_deref(), Some("Renamed"));
    // Trimmed back, the next Save works.
    h.state_mut().macros.editor.as_mut().unwrap().draft.steps.pop();
    h.run_steps(2);
    click(&mut h, "Save");
    assert!(h.state_mut().message.take().is_none());
    let wb = &h.state().session.active().unwrap().wb;
    assert_eq!(wb.macros.len(), 1);
    assert_eq!((wb.macros[0].name.as_str(), wb.macros[0].steps.len()), ("Renamed", 3));
}

#[test]
fn another_dialog_never_replaces_the_open_editor() {
    let mut h = harness();
    three(&mut h);
    h.state_mut().macros.editor.as_mut().unwrap().draft.name = "Unsaved".into();
    h.run_steps(2);
    for name in ["formatCells", "macros", "macroEditor"] {
        h.state_mut().open_dialog(name, json!({}));
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("macroEditor"), "{name}");
        assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.name.as_str()), Some("Unsaved"), "{name}");
    }
    click(&mut h, "New Macro");
    assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.name.as_str()), Some("Unsaved"));
}

#[test]
fn enter_in_change_never_picks_a_control_step() {
    let mut h = harness();
    three(&mut h);
    let enter = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
    for search in ["", "stop"] {
        h.state_mut().macros.editor.as_mut().unwrap().selected = Some("1".into());
        h.run_steps(2);
        click(&mut h, "Change…");
        if !search.is_empty() {
            h.input_mut().events.push(Event::Text(search.into()));
            h.run_steps(2);
        }
        h.input_mut().events.push(enter.clone());
        h.run_steps(3);
        let ed = h.state().macros.editor.as_ref().unwrap();
        assert!(matches!(&ed.draft.steps[0].kind, StepKind::Command { .. }), "{search:?}: {:?}", ed.draft.steps[0].kind);
    }
}

#[test]
fn the_editor_works_only_on_the_workbook_it_opened_in() {
    let mut h = harness();
    // A new macro whose one step writes A1, not saved yet.
    h.state_mut().open_dialog("macroEditor", json!({"new": "Fresh"}));
    h.run_steps(3);
    let step = Step::command("cell.set", json!({"cell": "A1", "input": "ran"}));
    h.state_mut().macros.editor.as_mut().unwrap().draft.steps = vec![step];
    let book1 = h.state().session.active().unwrap().uid;
    // Another workbook becomes active while the editor is open (File › New, Switch Window, an agent).
    h.state_mut().session.new_workbook();
    h.run_steps(3);
    let mut ed = h.state_mut().macros.editor.take().unwrap();
    let app = h.state_mut();
    // Save, Check, Run and Debug never act on the other workbook.
    assert!(!gridcraft_ui_egui::macro_editor::save(app, &mut ed));
    gridcraft_ui_egui::macro_editor::check(app, &mut ed);
    gridcraft_ui_egui::macro_editor::run(app, &mut ed);
    gridcraft_ui_egui::macro_editor::debug_action(app, &mut ed, "step");
    assert!(app.session.macro_debug.is_none());
    assert!(app.session.active().unwrap().wb.macros.is_empty(), "nothing was saved into the other workbook");
    assert_eq!(value(&h, "A1"), Value::Empty, "nothing ran there");
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("Book1")), "{:?}", h.state().message);
    h.state_mut().message = None;
    h.state_mut().macros.editor = Some(ed);
    h.run_steps(3);
    // The editor stays with its own workbook's window: it is not drawn over the other one.
    assert!(h.state().dialog.as_ref().is_none_or(|d| d.name() != "macroEditor"), "the editor is not shown here");
    // Back in its own workbook, the editor shows again and saves there.
    let i = h.state().session.documents().iter().position(|d| d.uid == book1).unwrap();
    h.state_mut().session.set_active(i);
    h.run_steps(3);
    assert!(h.state().dialog.as_ref().is_some_and(|d| d.name() == "macroEditor"), "the editor is back with its workbook");
    let mut ed = h.state_mut().macros.editor.take().unwrap();
    let app = h.state_mut();
    assert!(gridcraft_ui_egui::macro_editor::save(app, &mut ed));
    assert_eq!(app.session.active().unwrap().wb.macros.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["Fresh"]);
    app.macros.editor = Some(ed);
    h.run_steps(3);
    // Closing the workbook (without saving it) closes its editor.
    h.state_mut().run("file.close", json!({"force": true})).unwrap();
    h.run_steps(3);
    assert!(h.state().macros.editor.is_none());
    assert!(h.state().dialog.as_ref().is_none_or(|d| d.name() != "macroEditor"));
}

#[test]
fn a_deferred_close_acts_on_the_editors_window_whichever_window_is_active_meanwhile() {
    let mut h = harness();
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let book1 = h.state().session.active().unwrap().uid;
    let home = h.state().session.active_window().unwrap();
    // Another workbook in a window of its own.
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(2);
    let book2 = h.state().session.active().unwrap().uid;
    let other = h.state().session.active_window().unwrap();
    assert_ne!(other, home);
    // Back in the editor's window, File › Close asks about the draft and waits.
    h.state_mut().enter_window(home);
    h.run_steps(2);
    let r = h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, &name));
    assert_eq!(h.state().macros.editor.as_ref().and_then(|e| e.then_close.as_ref()).map(|c| c.window), Some(home));
    // The other window is the session's active one when the question is answered (a native
    // window the person clicked on while the question stood), while the editor's window draws
    // the question: the answer is given through the editor's own frame, as its button gives it.
    let ctx = h.ctx.clone();
    let app = h.state_mut();
    let mut d = app.dialog.take().expect("the editor is its window's dialog");
    assert_eq!(d.name(), "macroEditor");
    app.session.activate_window(other);
    assert_eq!(app.session.active().map(|d| d.uid), Some(book2));
    // Don't Save: the question's answer closes the editor without saving.
    let ed = app.macros.editor.as_mut().unwrap();
    ed.ask_close = false;
    ed.dirty = false;
    ed.close_requested = true;
    let mut open = true;
    let mut out = ctx.run_pass(egui::RawInput::default(), |ctx| gridcraft_ui_egui::macro_editor::window(app, ctx, &mut d, &mut open));
    out.textures_delta.clear(); // not painted
    assert!(!open, "the editor closed");
    // The close that was asked in the editor's window closes that workbook, not the other.
    assert!(!is_open(&h, book1), "the editor's workbook closed");
    assert!(is_open(&h, book2), "the workbook of the window activated meanwhile stays open");
    assert!(h.state().session.window(other).is_some());
    assert!(h.state().macros.editor.is_none());
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
}

/// Asks the window to close (the close button, Alt+F4, quitting) and returns the commands the
/// app answered with over the next frames.
fn request_window_close(h: &mut Harness) -> Vec<egui::ViewportCommand> {
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    step_commands(h)
}

fn step_commands(h: &mut Harness) -> Vec<egui::ViewportCommand> {
    let mut out = Vec::new();
    for _ in 0..4 {
        h.step();
        out.extend(h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default());
    }
    out
}

#[test]
fn quitting_asks_about_the_editors_unsaved_steps_first() {
    let mut h = harness();
    three(&mut h);
    h.state_mut().macros.editor.as_mut().unwrap().draft.name = "Unsaved".into();
    h.state_mut().macros.editor.as_mut().unwrap().dirty = true;
    // As if the workbook had just been saved.
    let d = h.state_mut().session.active_mut().unwrap();
    d.saved = d.wb.clone();
    h.run_steps(2);
    // Every workbook is saved, but the draft is not: the window stays open and the editor asks.
    let sent = request_window_close(&mut h);
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "{sent:?}");
    assert!(!sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    assert!(h.query_by_label_contains("Save changes to macro “Unsaved”").is_some());
    // Cancel keeps the app and the draft.
    // The formula bar's Cancel button has the same name; the question's is the button drawn last.
    h.get_all_by_role_and_label(egui::Role::Button, "Cancel").last().unwrap().click();
    let sent = step_commands(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.name.as_str()), Some("Unsaved"));
    // Asked again, Don't Save closes the editor and then the window.
    request_window_close(&mut h);
    h.get_by_label("Don't Save").click();
    let sent = step_commands(&mut h);
    assert!(h.state().macros.editor.is_none());
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
}

#[test]
fn quitting_with_the_editor_open_still_asks_about_unsaved_workbooks() {
    let mut h = harness();
    three(&mut h);
    h.state_mut().run("cell.set", json!({"cell": "Z9", "input": "unsaved"})).unwrap();
    h.run_steps(2);
    let sent = request_window_close(&mut h);
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "{sent:?}");
    // The saved, unchanged editor closes and the window close goes on (the window then asks to
    // close again, which a test harness has to do by hand); the workbook is asked about as
    // without the editor.
    assert!(h.state().macros.editor.is_none());
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    request_window_close(&mut h);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
}

#[test]
fn a_dialog_asked_for_over_the_editor_says_why_it_did_not_open() {
    let mut h = harness();
    three(&mut h);
    // File › Close on a changed workbook asks about saving it, which can't replace the editor.
    h.state_mut().run("cell.set", json!({"cell": "Z9", "input": "unsaved"})).unwrap();
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("macroEditor"));
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("Close the macro step editor")), "{:?}", h.state().message);
}

#[test]
fn the_control_channels_cancel_closes_the_editor_as_its_own_close_does() {
    let mut h = harness();
    three(&mut h);
    let ctx = h.ctx.clone();
    let reply = |o: gridcraft_ui_egui::control::Outcome| match o {
        gridcraft_ui_egui::control::Outcome::Done(v) => v,
        _ => json!(null),
    };
    // Unsaved steps are only the editor's to ask about: cancel is refused and keeps them.
    h.state_mut().macros.editor.as_mut().unwrap().dirty = true;
    let r = reply(gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, "ui.dialog.cancel", &json!({})));
    assert!(r.get("error").is_some(), "{r}");
    assert!(h.state().macros.editor.is_some());
    // A saved editor closes, and takes its overlays with it.
    h.state_mut().macros.editor.as_mut().unwrap().dirty = false;
    h.get_by_label("Check").click();
    h.run_steps(3);
    assert!(h.state().grid.macro_overlay.is_some());
    let r = reply(gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, "ui.dialog.cancel", &json!({})));
    assert!(r.get("error").is_none(), "{r}");
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert!(h.state().macros.editor.is_none());
    assert!(h.state().grid.macro_overlay.is_none());
}

#[test]
fn the_editor_fits_a_narrow_window() {
    for size in [egui::vec2(600.0, 480.0), egui::vec2(480.0, 400.0)] {
        let mut session = Session::new();
        session.new_workbook();
        let app = SheetApp::new(session, Default::default());
        let mut h = egui_kittest::Harness::builder().with_size(size).build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        h.run_steps(4);
        three(&mut h);
        h.run_steps(4);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let window = h.get_by_label_contains("Macro: Three").rect();
        assert!(screen.contains_rect(window), "the editor at {window:?} is outside the {size:?} window");
        // The editor's own controls (Delete is left out: the ribbon has one too).
        for label in ["Add Step", "Duplicate", "Check", "Run", "Step", "Continue", "Run to Here", "Stop", "Details", "Inputs", "Output", "Save"] {
            let r = h.get_by_label(label).rect();
            assert!(screen.contains_rect(r), "{label} at {r:?} is outside the {size:?} window");
        }
    }
}

#[test]
fn a_macro_at_the_step_limit_opens_without_freezing_the_editor() {
    let mut h = harness();
    let steps: Vec<_> = (0..gridcraft_engine::model::MAX_MACRO_STEPS)
        .map(|i| json!({"command": "cell.set", "params": {"cell": format!("A{}", i + 1), "input": "x"}}))
        .collect();
    h.state_mut().run("macro.save", json!({"macro": {"name": "Big", "steps": steps}})).unwrap();
    h.state_mut().open_dialog("macroEditor", json!({"macro": "Big"}));
    h.run_steps(2);
    // Each row used to copy the whole list: five frames took about a minute. Now well under a
    // second; the bound leaves room for a slow machine.
    let t = gridcraft_ui_egui::now_ms();
    h.run_steps(5);
    let ms = gridcraft_ui_egui::now_ms() - t;
    assert!(ms < 10_000.0, "five frames took {ms} ms");
    assert!(h.query_by_label_contains("Step 1: ").is_some());
}

/// Opens the editor on a new macro and gives its draft a step, as editing it does.
fn edited_new_macro(h: &mut Harness) {
    h.state_mut().open_dialog("macroEditor", json!({}));
    h.run_steps(3);
    let ed = h.state_mut().macros.editor.as_mut().unwrap();
    ed.draft.steps.push(Step::command("cell.set", json!({"cell": "A1", "input": "7"})));
    ed.dirty = true;
    h.run_steps(2);
}

/// Clicks a button of the editor's Save changes question: the question's buttons are drawn last
/// (the formula bar has a Cancel and the editor a Save of their own).
fn answer(h: &mut Harness, label: &str) {
    h.get_all_by_role_and_label(egui::Role::Button, label).last().unwrap().click();
    h.run_steps(4);
}

/// Whether the workbook `uid` is still open (the app starts a blank one when the last closes).
fn is_open(h: &Harness, uid: u64) -> bool {
    h.state().session.documents().iter().any(|d| d.uid == uid)
}

fn asks_about(h: &Harness, name: &str) -> bool {
    h.query_by_label_contains(&format!("Save changes to macro “{name}”")).is_some()
}

#[test]
fn closing_the_workbook_asks_about_an_unsaved_draft_first() {
    let mut h = harness();
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let book = h.state().session.active().unwrap().uid;
    assert!(!h.state().session.active().unwrap().is_dirty());
    // The workbook is saved, but the draft is not: File › Close asks about the draft and waits.
    let r = h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, &name));
    assert!(is_open(&h, book));
    // Cancel keeps the workbook and the draft.
    answer(&mut h, "Cancel");
    assert!(is_open(&h, book));
    assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.steps.len()), Some(1));
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("macroEditor"));
    // A window's close button (`window.close`) asks the same way.
    let window = h.state().session.active_window().unwrap();
    let r = h.state_mut().run("window.close", json!({"window": window})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, &name));
    answer(&mut h, "Cancel");
    assert!(is_open(&h, book));
    // Don't Save closes the editor, then the workbook (which has nothing of its own to ask about).
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(3);
    answer(&mut h, "Don't Save");
    assert!(h.state().macros.editor.is_none());
    assert!(!is_open(&h, book));
    assert!(h.state().dialog.is_none());
}

#[test]
fn save_in_the_question_saves_the_macro_then_the_workbook_asks_about_itself() {
    let path = std::env::temp_dir().join(format!("gridcraft-macro-draft-{}.xlsx", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let target = path.to_string_lossy().to_string();
    let mut h = harness_with(Services { pick_save: Some(Box::new(move |_| Some(target.clone()))), ..Default::default() });
    edited_new_macro(&mut h);
    let book = h.state().session.active().unwrap().uid;
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(3);
    answer(&mut h, "Save");
    // The macro is in the workbook now, which makes it unsaved: closing it goes on and asks.
    assert!(h.state().macros.editor.is_none());
    assert_eq!(h.state().session.active().unwrap().wb.macros.len(), 1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    h.get_all_by_role_and_label(egui::Role::Button, "Save").last().unwrap().click();
    h.run_steps(4);
    assert!(!is_open(&h, book));
    let mut s = Session::new();
    s.execute("file.open", json!({"path": path.to_str().unwrap()})).unwrap();
    let wb = &s.active().unwrap().wb;
    assert_eq!(wb.macros.len(), 1);
    assert_eq!(wb.macros[0].steps.len(), 1);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_draft_that_cannot_be_saved_keeps_its_workbook_open() {
    let mut h = harness();
    h.state_mut().run("macro.save", json!({"macro": {"name": "Taken", "steps": []}})).unwrap();
    let d = h.state_mut().session.active_mut().unwrap();
    d.saved = d.wb.clone();
    edited_new_macro(&mut h);
    let book = h.state().session.active().unwrap().uid;
    // A new macro never replaces one with its name, so Save is refused.
    h.state_mut().macros.editor.as_mut().unwrap().draft.name = "Taken".into();
    h.state_mut().run("file.close", json!({})).unwrap();
    h.run_steps(3);
    answer(&mut h, "Save");
    assert!(h.state().message.is_some());
    h.state_mut().message = None;
    h.run_steps(2);
    assert!(h.state().macros.editor.is_some());
    // Closing the editor itself later never closes the workbook the earlier question was about.
    h.state_mut().macros.editor.as_mut().unwrap().close_requested = true;
    h.run_steps(3);
    answer(&mut h, "Don't Save");
    assert!(h.state().macros.editor.is_none());
    assert!(is_open(&h, book));
}

#[test]
fn an_untouched_new_macro_does_not_make_closing_ask() {
    let mut h = harness();
    h.state_mut().open_dialog("macroEditor", json!({}));
    h.run_steps(3);
    assert!(h.state().macros.editor.as_ref().unwrap().dirty);
    let book = h.state().session.active().unwrap().uid;
    let r = h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], true);
    assert!(!is_open(&h, book));
    h.run_steps(3);
    assert!(h.state().macros.editor.is_none());
    assert!(h.state().dialog.is_none());
}

#[test]
fn a_new_macros_name_change_alone_makes_closing_ask() {
    let mut h = harness();
    h.state_mut().open_dialog("macroEditor", json!({}));
    h.run_steps(3);
    let book = h.state().session.active().unwrap().uid;
    // Only the name changes, as typing in the Name box does.
    let ed = h.state_mut().macros.editor.as_mut().unwrap();
    ed.draft.name = "Renamed".into();
    ed.dirty = true;
    h.run_steps(2);
    let r = h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, "Renamed"));
    assert!(is_open(&h, book));
}

/// Quit (Cmd+Q, the macOS Quit menu, a Dock quit) and the commands the app answered with over the
/// next frames.
fn request_quit(h: &mut Harness) -> Vec<egui::ViewportCommand> {
    let ctx = h.ctx.clone();
    h.state_mut().request_quit(&ctx);
    step_commands(h)
}

#[test]
fn quit_asks_about_the_editors_unsaved_steps_first() {
    let mut h = harness();
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    assert!(!h.state().session.active().unwrap().is_dirty());
    // Every workbook is saved, but the draft is not: the app stays and the editor asks.
    let sent = request_quit(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    assert!(!h.state().allow_close);
    assert!(asks_about(&h, &name));
    // Cancel keeps the app and the draft.
    answer(&mut h, "Cancel");
    let sent = step_commands(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.steps.len()), Some(1));
    // Asked again, Don't Save closes the editor and the quit goes on.
    request_quit(&mut h);
    assert!(asks_about(&h, &name));
    h.get_by_label("Don't Save").click();
    let sent = step_commands(&mut h);
    assert!(h.state().macros.editor.is_none());
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
}

#[test]
fn a_refused_save_while_quitting_stops_the_quit() {
    let mut h = harness();
    h.state_mut().run("macro.save", json!({"macro": {"name": "Taken", "steps": []}})).unwrap();
    let d = h.state_mut().session.active_mut().unwrap();
    d.saved = d.wb.clone();
    edited_new_macro(&mut h);
    // A new macro never replaces one with its name, so the question's Save is refused.
    h.state_mut().macros.editor.as_mut().unwrap().draft.name = "Taken".into();
    for quit in [request_quit, request_window_close] {
        h.run_steps(2);
        let sent = quit(&mut h);
        assert!(sent.contains(&egui::ViewportCommand::CancelClose) || !h.state().allow_close, "{sent:?}");
        assert!(asks_about(&h, "Taken"));
        answer(&mut h, "Save");
        assert!(h.state().message.is_some());
        h.state_mut().message = None;
        assert!(h.state().macros.editor.is_some());
        // Closing the editor later, on its own, no longer quits the app.
        h.state_mut().macros.editor.as_mut().unwrap().close_requested = true;
        h.run_steps(3);
        h.get_by_label("Don't Save").click();
        let sent = step_commands(&mut h);
        assert!(h.state().macros.editor.is_none());
        assert!(!sent.contains(&egui::ViewportCommand::Close), "the app keeps running: {sent:?}");
        assert!(!h.state().allow_close);
        // The next round starts with the same draft in the editor again.
        edited_new_macro(&mut h);
        h.state_mut().macros.editor.as_mut().unwrap().draft.name = "Taken".into();
    }
}

#[test]
fn an_untouched_new_macro_closes_without_asking_however_the_editor_closes() {
    let mut h = harness();
    let ctx = h.ctx.clone();
    let open_new = |h: &mut Harness| {
        h.state_mut().open_dialog("macroEditor", json!({}));
        h.run_steps(3);
        assert!(h.state().macros.editor.as_ref().is_some_and(|e| e.dirty && e.saved_as.is_none()));
    };
    // Its own close button.
    open_new(&mut h);
    h.state_mut().macros.editor.as_mut().unwrap().close_requested = true;
    h.run_steps(3);
    assert!(h.state().macros.editor.is_none(), "no Save changes question for a macro nobody edited");
    // The control channel's cancel.
    open_new(&mut h);
    let r = gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, "ui.dialog.cancel", &json!({}));
    assert!(matches!(&r, gridcraft_ui_egui::control::Outcome::Done(v) if v.get("error").is_none()));
    h.run_steps(2);
    assert!(h.state().macros.editor.is_none());
    // Quit, with every workbook saved: the editor closes and the quit goes on.
    open_new(&mut h);
    let sent = request_quit(&mut h);
    assert!(h.state().macros.editor.is_none());
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
}

#[test]
fn closing_the_window_that_shows_the_editor_asks_about_its_draft() {
    let mut h = harness();
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let home = h.state().session.active_window().unwrap();
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(3);
    let other = h.state().session.active_window().unwrap();
    assert_ne!(other, home);
    // The workbook stays open in its other window, but the editor's window would take the draft.
    let r = h.state_mut().run("window.close", json!({"window": home})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert_eq!(h.state().session.active_window(), Some(home), "the editor's window comes forward to ask");
    assert!(asks_about(&h, &name));
    answer(&mut h, "Cancel");
    assert!(h.state().session.window(home).is_some());
    assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.steps.len()), Some(1));
    // File › Close in the editor's window asks too; Don't Save closes that window and the editor.
    let r = h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, &name));
    answer(&mut h, "Don't Save");
    assert!(h.state().macros.editor.is_none());
    assert!(h.state().session.window(home).is_none());
    assert!(h.state().session.window(other).is_some(), "the workbook stays open in its other window");
}

#[test]
fn closing_the_window_that_shows_a_saved_editor_closes_the_editor_with_it() {
    let mut h = harness();
    three(&mut h);
    let home = h.state().session.active_window().unwrap();
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(3);
    let r = h.state_mut().run("window.close", json!({"window": home})).unwrap();
    assert_eq!(r["closed"], true);
    h.run_steps(3);
    // Nothing is left of an editor no window shows: a new one opens, and nothing was lost.
    assert!(h.state().macros.editor.is_none());
    h.state_mut().open_dialog("macroEditor", json!({"macro": "Three"}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("macroEditor"));
}

#[test]
fn a_close_request_during_the_editors_save_dialog_waits_for_it() {
    let mut h = harness();
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let book1 = h.state().session.active().unwrap().uid;
    let home = h.state().session.active_window().unwrap();
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(2);
    let book2 = h.state().session.active().unwrap().uid;
    h.state_mut().enter_window(home);
    h.run_steps(2);
    // Cmd+W: the editor asks about the draft and the close waits.
    let r = h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, &name));
    // Cmd+W again, then the window's close button, while the question stands: the same close,
    // so both wait on the same question; it is not asked twice and nothing closes under it.
    let r = h.state_mut().run("file.close", json!({})).unwrap();
    assert_eq!(r["closed"], false);
    let r = h.state_mut().run("window.close", json!({"window": home})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert_eq!(h.get_all_by_label_contains("Save changes to macro").count(), 1, "one question");
    assert!(is_open(&h, book1) && is_open(&h, book2));
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    let pending = h.state().macros.editor.as_ref().and_then(|e| e.then_close.as_ref()).expect("the close waits");
    assert_eq!((pending.id.as_str(), pending.whole), ("file.close", true), "the close that opened the question is the pending one");
    // Don't Save: the editor closes and the workbook closes once; the other workbook stays.
    answer(&mut h, "Don't Save");
    assert!(h.state().macros.editor.is_none());
    assert!(!is_open(&h, book1));
    assert!(is_open(&h, book2), "the close ran once");
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}

#[test]
fn a_different_close_during_the_editors_question_is_refused_until_it_is_answered() {
    let mut h = harness();
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let book = h.state().session.active().unwrap().uid;
    let home = h.state().session.active_window().unwrap();
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(3);
    let other = h.state().session.active_window().unwrap();
    // The editor's window closes while the workbook stays open in the other: the editor asks.
    let r = h.state_mut().run("window.close", json!({"window": home})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, &name));
    // The Backstage Close (the whole workbook) while that question stands asks for more than
    // the question was asked for: refused with the reason; the question and the workbook stay.
    let e = h.state_mut().run("file.close", json!({"document": true})).unwrap_err();
    assert!(e.contains("asking whether to save") && e.contains("its window closes"), "{e}");
    h.run_steps(2);
    assert_eq!(h.get_all_by_label_contains("Save changes to macro").count(), 1, "one question");
    assert!(is_open(&h, book));
    assert_eq!(h.state().session.windows().len(), 2);
    // Don't Save closes the editor's window alone, as the question was asked for.
    answer(&mut h, "Don't Save");
    assert!(h.state().macros.editor.is_none());
    assert!(h.state().session.window(home).is_none());
    assert!(h.state().session.window(other).is_some());
    assert!(is_open(&h, book), "the workbook stays open in its other window");
    // The reverse: the workbook is closing when a close of the editor's window alone arrives.
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(3);
    h.state_mut().enter_window(other);
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let r = h.state_mut().run("file.close", json!({"document": true})).unwrap();
    assert_eq!(r["closed"], false);
    h.run_steps(3);
    assert!(asks_about(&h, &name));
    let e = h.state_mut().run("window.close", json!({"window": other})).unwrap_err();
    assert!(e.contains("the workbook closes"), "{e}");
    // Cancel keeps the workbook, both windows and the draft.
    answer(&mut h, "Cancel");
    assert!(is_open(&h, book));
    assert_eq!(h.state().session.windows().len(), 2);
    assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.steps.len()), Some(1));
    assert!(h.state().macros.editor.as_ref().is_some_and(|e| e.then_close.is_none()));
}

#[test]
fn quitting_with_a_draft_in_one_window_and_unsaved_cells_in_another_asks_in_order_and_cancel_stops_it() {
    let mut h = harness();
    // Book1, saved, with an unsaved draft in its window; Book2 with unsaved cells in its own.
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let book1 = h.state().session.active().unwrap().uid;
    let a = h.state().session.active_window().unwrap();
    assert!(!h.state().session.active().unwrap().is_dirty());
    h.state_mut().run("file.new", json!({})).unwrap();
    h.state_mut().run("cell.set", json!({"cell": "Z9", "input": "unsaved"})).unwrap();
    h.run_steps(2);
    let book2 = h.state().session.active().unwrap().uid;
    let title2 = h.state().session.active().unwrap().display_title();
    let b = h.state().session.active_window().unwrap();
    assert_ne!(a, b);
    let both_open =
        |h: &Harness| is_open(h, book1) && is_open(h, book2) && h.state().session.window(a).is_some() && h.state().session.window(b).is_some();
    // Quit from Book2's window: the editor's question comes first, in its own window.
    let sent = request_quit(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::Close) && !h.state().allow_close, "{sent:?}");
    assert_eq!(h.state().session.active_window(), Some(a), "the editor's window comes forward");
    assert!(asks_about(&h, &name));
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("macroEditor"), "no workbook is asked about yet");
    // Don't Save: the draft goes, the editor closes and the quit goes on (the window asks to
    // close again, which a test harness has to do by hand).
    h.get_all_by_role_and_label(egui::Role::Button, "Don't Save").last().unwrap().click();
    let sent = step_commands(&mut h);
    assert!(h.state().macros.editor.is_none());
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    request_window_close(&mut h);
    // Book2 is asked about next, in its window, as the quit's question.
    assert_eq!(h.state().session.active_window(), Some(b), "the unsaved workbook's window comes forward");
    let d = h.state().dialog.as_ref().expect("Book2 is asked about");
    assert_eq!(d.name(), "saveChanges");
    assert_eq!(d.values.get("title").and_then(serde_json::Value::as_str), Some(title2.as_str()));
    assert_eq!(d.values.get("quit"), Some(&json!(true)), "{:?}", d.values);
    assert!(both_open(&h));
    // Cancel: the quit stops with both windows and workbooks open and Book2's changes kept; the
    // draft the person discarded stays discarded.
    h.get_all_by_role_and_label(egui::Role::Button, "Cancel").last().unwrap().click();
    let sent = step_commands(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::Close) && !h.state().allow_close, "{sent:?}");
    assert!(h.state().dialog.is_none());
    assert!(both_open(&h));
    assert!(h.state().session.documents().iter().find(|d| d.uid == book2).unwrap().is_dirty());
    assert!(h.state().macros.editor.is_none());
    // A quit cancelled at the editor's question stops before any workbook is asked about.
    h.state_mut().enter_window(a);
    edited_new_macro(&mut h);
    let name = h.state().macros.editor.as_ref().unwrap().draft.name.clone();
    let sent = request_quit(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    assert!(asks_about(&h, &name));
    answer(&mut h, "Cancel");
    let sent = step_commands(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::Close) && !h.state().allow_close, "{sent:?}");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("macroEditor"), "no Save Changes question followed");
    assert_eq!(h.state().macros.editor.as_ref().map(|e| e.draft.steps.len()), Some(1), "the draft is kept");
    assert!(both_open(&h));
}
