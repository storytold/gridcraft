//! Form controls through real pointer and keyboard input: dragging a slider (one undo step),
//! paging on the track, clicking and holding spin arrows, keyboard steps, a button running its
//! macro, Cmd+click or right-click selecting a control without running it, right-to-left
//! sheets, and the Insert › Controls buttons, context menu and dialogs.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{FormControl, ObjKind, ObjRef};
use gridcraft_ui_egui::form_controls::{Part, part_at, slider_parts};
use gridcraft_ui_egui::{SheetApp, chartview, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

struct Ids {
    slider: u32,
    spin: u32,
    button: u32,
}

fn fixture() -> (Harness, Ids) {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "B1", "input": "20"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=B1*10"})).unwrap();
    let id = |v: serde_json::Value| v["control"].as_u64().unwrap() as u32;
    let slider =
        id(s.execute("insert.slider", json!({"at": "E3", "width": 200, "height": 20, "link": "B1", "max": 100, "step": 1, "page": 10})).unwrap());
    let spin =
        id(s.execute("insert.spinButton", json!({"at": "E8", "width": 20, "height": 40, "link": "B2", "max": 50, "step": 2, "value": 10})).unwrap());
    s.execute(
        "macro.save",
        json!({"name": "Mark", "steps": [{"command": "cell.set", "params": {"cell": "H1", "input": "pressed"}}], "output": {"onConflict": "overwrite"}}),
    )
    .unwrap();
    let button = id(s.execute("insert.button", json!({"at": "E12", "text": "Mark it", "macro": "Mark"})).unwrap());
    s.execute("selection.set", json!({"cell": "A20"})).unwrap();
    let mut h = egui_kittest::Harness::builder()
        .with_size(vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_os(egui::os::OperatingSystem::Mac)
        .build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            SheetApp::new(s, Default::default()),
        );
    h.run_steps(4);
    (h, Ids { slider, spin, button })
}

fn control(h: &Harness, id: u32) -> FormControl {
    h.state().session.doc().unwrap().wb.active().unwrap().controls.iter().find(|c| c.id == id).cloned().unwrap()
}

fn rect(h: &Harness, id: u32) -> Rect {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    chartview::object_rect(&Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll), sh, ObjRef::new(ObjKind::Control, id)).unwrap()
}

fn cell(h: &Harness, a: &str) -> Value {
    h.state().session.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap())
}

fn undo_len(h: &Harness) -> usize {
    h.state().session.doc().unwrap().undo.len()
}

fn button(h: &mut Harness, p: Pos2, pressed: bool, which: PointerButton, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: which, pressed, modifiers });
    h.step();
}

fn click_with(h: &mut Harness, p: Pos2, modifiers: Modifiers) {
    h.run_steps(30);
    button(h, p, true, PointerButton::Primary, modifiers);
    button(h, p, false, PointerButton::Primary, modifiers);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

fn click(h: &mut Harness, p: Pos2) {
    click_with(h, p, Modifiers::NONE);
}

fn key(h: &mut Harness, key: Key) {
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

fn thumb(h: &Harness, id: u32) -> Rect {
    let c = control(h, id);
    slider_parts(rect(h, id), &c, 1.0).1
}

#[test]
fn dragging_the_slider_is_one_undo_step() {
    let (mut h, ids) = fixture();
    let start = thumb(&h, ids.slider).center();
    assert_eq!(control(&h, ids.slider).value, 20);
    let before = undo_len(&h);
    let r = rect(&h, ids.slider);
    let (track, _) = slider_parts(r, &control(&h, ids.slider), 1.0);
    h.run_steps(30);
    button(&mut h, start, true, PointerButton::Primary, Modifiers::NONE);
    for k in 1..=6 {
        let x = start.x + (track.left() + track.width() * 0.7 - start.x) * k as f32 / 6.0;
        h.input_mut().events.push(Event::PointerMoved(Pos2::new(x, start.y + 3.0)));
        h.step();
    }
    let mid = control(&h, ids.slider).value;
    assert!((68..=72).contains(&mid), "the thumb follows the pointer: {mid}");
    assert_eq!(cell(&h, "B1"), Value::Number(mid as f64), "the linked cell follows while dragging");
    assert_eq!(cell(&h, "C1"), Value::Number(mid as f64 * 10.0), "and its dependents");
    button(&mut h, Pos2::new(track.left() + track.width() * 0.7, start.y), false, PointerButton::Primary, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(undo_len(&h), before + 1, "the whole drag is one undo step");
    assert_eq!(h.state().selected_object, None, "working a control doesn't select it");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(cell(&h, "B1"), Value::Number(20.0));
    assert_eq!(control(&h, ids.slider).value, 20);
}

#[test]
fn the_track_pages_and_editing_the_cell_moves_the_thumb() {
    let (mut h, ids) = fixture();
    let r = rect(&h, ids.slider);
    let after = Pos2::new(r.right() - 4.0, r.center().y);
    assert_eq!(part_at(r, &control(&h, ids.slider), 1.0, after), Some(Part::TrackAfter));
    click(&mut h, after);
    assert_eq!(cell(&h, "B1"), Value::Number(30.0), "a click on the track moves a page");
    click(&mut h, Pos2::new(r.left() + 3.0, r.center().y));
    assert_eq!(cell(&h, "B1"), Value::Number(20.0));
    let was = thumb(&h, ids.slider).center().x;
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "90"})).unwrap();
    h.run_steps(2);
    assert_eq!(control(&h, ids.slider).value, 90);
    assert!(thumb(&h, ids.slider).center().x > was + 100.0, "editing the linked cell moves the thumb");
}

#[test]
fn spin_arrows_step_and_repeat_while_held() {
    let (mut h, ids) = fixture();
    let r = rect(&h, ids.spin);
    let up = Pos2::new(r.center().x, r.top() + 5.0);
    let down = Pos2::new(r.center().x, r.bottom() - 5.0);
    click(&mut h, up);
    assert_eq!(cell(&h, "B2"), Value::Number(12.0));
    click(&mut h, down);
    click(&mut h, down);
    assert_eq!(cell(&h, "B2"), Value::Number(8.0));
    // Held: after a pause it repeats, and the whole hold is one undo step.
    let before = undo_len(&h);
    h.run_steps(30);
    button(&mut h, up, true, PointerButton::Primary, Modifiers::NONE);
    h.run_steps(60);
    button(&mut h, up, false, PointerButton::Primary, Modifiers::NONE);
    let v = control(&h, ids.spin).value;
    assert!(v >= 14, "holding the arrow repeats: {v}");
    assert_eq!(undo_len(&h), before + 1);
    // It stops at the maximum.
    h.run_steps(30);
    button(&mut h, up, true, PointerButton::Primary, Modifiers::NONE);
    h.run_steps(400);
    button(&mut h, up, false, PointerButton::Primary, Modifiers::NONE);
    assert_eq!(control(&h, ids.spin).value, 50);
}

#[test]
fn the_keyboard_steps_the_control_last_worked() {
    let (mut h, ids) = fixture();
    let r = rect(&h, ids.spin);
    click(&mut h, Pos2::new(r.center().x, r.top() + 5.0));
    assert_eq!(h.state().grid.focused_control, Some(ids.spin));
    key(&mut h, Key::ArrowUp);
    key(&mut h, Key::ArrowUp);
    assert_eq!(control(&h, ids.spin).value, 16);
    key(&mut h, Key::ArrowDown);
    assert_eq!(control(&h, ids.spin).value, 14);
    key(&mut h, Key::End);
    assert_eq!(control(&h, ids.spin).value, 50);
    let tr = rect(&h, ids.slider);
    click(&mut h, Pos2::new(tr.right() - 4.0, tr.center().y));
    assert_eq!(h.state().grid.focused_control, Some(ids.slider));
    key(&mut h, Key::ArrowRight);
    assert_eq!(control(&h, ids.slider).value, 31);
    key(&mut h, Key::PageDown);
    assert_eq!(control(&h, ids.slider).value, 41);
    key(&mut h, Key::PageUp);
    key(&mut h, Key::ArrowLeft);
    assert_eq!(control(&h, ids.slider).value, 30);
    key(&mut h, Key::Home);
    assert_eq!(cell(&h, "B1"), Value::Number(0.0));
    // Escape lets go; the arrows move the cell cursor again.
    key(&mut h, Key::Escape);
    assert_eq!(h.state().grid.focused_control, None);
    key(&mut h, Key::ArrowDown);
    assert_eq!(control(&h, ids.slider).value, 0);
    assert_eq!(h.state().session.doc().unwrap().selection.active.a1(), "A21");
}

#[test]
fn a_button_runs_its_macro_and_cmd_or_right_click_only_selects() {
    let (mut h, ids) = fixture();
    let r = rect(&h, ids.button);
    click_with(&mut h, r.center(), Modifiers::COMMAND);
    assert_eq!(h.state().selected_object, Some(ObjRef::new(ObjKind::Control, ids.button)));
    assert_eq!(cell(&h, "H1"), Value::Empty, "Cmd+click selects without running");
    // A click on a selected control moves it rather than running it.
    click(&mut h, r.center());
    assert_eq!(cell(&h, "H1"), Value::Empty);
    // Click elsewhere, then right-click: selects, doesn't run.
    click(&mut h, Pos2::new(r.right() + 300.0, r.bottom() + 100.0));
    assert_eq!(h.state().selected_object, None);
    h.run_steps(30);
    button(&mut h, r.center(), true, PointerButton::Secondary, Modifiers::NONE);
    button(&mut h, r.center(), false, PointerButton::Secondary, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(h.state().selected_object, Some(ObjRef::new(ObjKind::Control, ids.button)));
    assert_eq!(cell(&h, "H1"), Value::Empty);
    assert_eq!(h.state().grid.object_menu.map(|(_, kind, id)| (kind, id)), Some(("control", ids.button)));
    // Escape closes the menu (which would otherwise take the next click at the pointer), then
    // the selection is dropped, so the click below reaches the button itself.
    key(&mut h, Key::Escape);
    assert!(h.state().grid.object_menu.is_none(), "Escape closes the control's menu");
    h.state_mut().selected_object = None;
    // A plain click runs the macro, as one undo step.
    let before = undo_len(&h);
    click(&mut h, r.center());
    assert_eq!(cell(&h, "H1"), Value::text("pressed"));
    assert_eq!(undo_len(&h), before + 1);
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    // Pressing and sliding off before letting go doesn't run it.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(30);
    button(&mut h, r.center(), true, PointerButton::Primary, Modifiers::NONE);
    button(&mut h, r.right_bottom() + vec2(80.0, 80.0), false, PointerButton::Primary, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(cell(&h, "H1"), Value::Empty);
}

#[test]
fn holding_enter_or_space_presses_a_focused_button_once() {
    let (mut h, ids) = fixture();
    let r = rect(&h, ids.button);
    click(&mut h, r.center());
    assert_eq!(h.state().grid.focused_control, Some(ids.button));
    for k in [Key::Enter, Key::Space] {
        let before = undo_len(&h);
        // A held key: one press, then the system's repeats, then the release.
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(1);
        for _ in 0..5 {
            h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE });
            h.run_steps(1);
        }
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
        assert_eq!(undo_len(&h), before + 1, "{k:?} ran the macro once");
        assert_eq!(cell(&h, "H1"), Value::text("pressed"));
        assert_eq!(cell(&h, "A20"), Value::Empty, "{k:?} typed nothing into the cell");
    }
}

#[test]
fn a_button_from_a_file_asks_before_it_runs() {
    use egui_kittest::kittest::Queryable as _;
    let (mut h, ids) = fixture();
    h.state_mut().session.doc_mut().unwrap().trust = gridcraft_engine::Trust::FromFile;
    let r = rect(&h, ids.button);
    click(&mut h, r.center());
    assert_eq!(cell(&h, "H1"), Value::Empty, "nothing runs before the person says so");
    let d = h.state().dialog.clone().expect("a prompt");
    assert_eq!(d.name(), "controlQuestion");
    let text = d.values["text"].as_str().unwrap_or("").to_string();
    assert!(text.contains("came with a workbook you opened"), "{text}");
    h.get_by_label(" Run ").click();
    h.run_steps(2);
    assert_eq!(cell(&h, "H1"), Value::text("pressed"), "Run presses it again with the approval");
    assert!(h.state().dialog.is_none());
}

#[test]
fn on_a_right_to_left_sheet_a_control_works_where_it_is_drawn() {
    let (mut h, ids) = fixture();
    h.state_mut().run("view.rightToLeft", json!({"on": true})).unwrap();
    h.run_steps(3);
    let screen = |h: &Harness, id: u32| {
        let app = h.state();
        let sh = app.session.doc().unwrap().wb.active().unwrap();
        let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
        geo.mirror_rect(chartview::object_rect(&geo, sh, ObjRef::new(ObjKind::Control, id)).unwrap())
    };
    // The slider moves as a whole and keeps its minimum on the left.
    let r = screen(&h, ids.slider);
    click(&mut h, Pos2::new(r.right() - 4.0, r.center().y));
    assert_eq!(cell(&h, "B1"), Value::Number(30.0), "the right end of the track pages up");
    let r = screen(&h, ids.spin);
    click(&mut h, Pos2::new(r.center().x, r.top() + 5.0));
    assert_eq!(cell(&h, "B2"), Value::Number(12.0), "the upper arrow adds a step");
}

// ---------------------------------------------------------------- ribbon, menus and dialogs

fn insert_tab() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    let mut app = SheetApp::new(s, Default::default());
    app.ui.ribbon_tab = "Insert".into();
    let mut h = egui_kittest::Harness::builder()
        .with_size(vec2(1400.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_os(egui::os::OperatingSystem::Mac)
        .build_ui_state(
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

fn controls(h: &Harness) -> Vec<FormControl> {
    h.state().session.doc().unwrap().wb.active().unwrap().controls.clone()
}

fn cells_rect(h: &Harness) -> Rect {
    h.state().grid.cells_rect.unwrap()
}

#[test]
fn the_controls_group_places_by_click_or_drag() {
    use egui_kittest::kittest::Queryable as _;
    let mut h = insert_tab();
    // A click places a slider at its default size.
    h.get_by_label("Slider").click();
    h.run_steps(2);
    assert_eq!(h.state().grid.placing, Some(gridcraft_engine::model::ControlKind::Slider));
    let at = cells_rect(&h).min + vec2(300.0, 120.0);
    click(&mut h, at);
    let list = controls(&h);
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].anchor.width, list[0].anchor.height, list[0].horizontal), (144.0, 18.0, true));
    assert_eq!(h.state().grid.placing, None, "placing is one-shot");
    assert_eq!(h.state().selected_object, Some(ObjRef::new(ObjKind::Control, list[0].id)), "the new control is selected");
    // A tall drag draws a vertical slider of that size.
    h.get_by_label("Slider").click();
    h.run_steps(2);
    let from = cells_rect(&h).min + vec2(600.0, 100.0);
    h.run_steps(30);
    button(&mut h, from, true, PointerButton::Primary, Modifiers::NONE);
    h.input_mut().events.push(Event::PointerMoved(from + vec2(10.0, 60.0)));
    h.step();
    button(&mut h, from + vec2(20.0, 150.0), false, PointerButton::Primary, Modifiers::NONE);
    h.run_steps(2);
    let s = controls(&h).pop().unwrap();
    // The drawn size is in sheet points (screen pixels over the zoom).
    let drawn = rect(&h, s.id);
    assert!(!s.horizontal && (drawn.width() - 20.0).abs() < 1.5 && (drawn.height() - 150.0).abs() < 1.5, "{drawn:?}");
    // Escape disarms.
    h.get_by_label("Spin Button").click();
    h.run_steps(2);
    key(&mut h, Key::Escape);
    assert_eq!(h.state().grid.placing, None);
    // A button asks what to run.
    h.get_by_label("Button").click();
    h.run_steps(2);
    let spot = cells_rect(&h).min + vec2(100.0, 300.0);
    click(&mut h, spot);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("assignAction"));
}

#[test]
fn assign_action_and_format_control_dialogs() {
    use egui_kittest::kittest::Queryable as _;
    let (mut h, ids) = fixture();
    // Right-click opens the control's menu; Assign Action picks a command.
    let r = rect(&h, ids.button);
    h.run_steps(30);
    button(&mut h, r.center(), true, PointerButton::Secondary, Modifiers::NONE);
    button(&mut h, r.center(), false, PointerButton::Secondary, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(h.state().grid.object_menu.map(|(_, kind, id)| (kind, id)), Some(("control", ids.button)));
    // The ribbon has a Delete button of its own.
    for item in ["Edit Text", "Assign Action…", "Format Control…", "Delete"] {
        assert!(h.query_all_by_label(item).next().is_some(), "{item}");
    }
    h.get_by_label("Assign Action…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("assignAction"));
    assert!(h.state().grid.object_menu.is_none(), "choosing an item closes the menu");
    // The dialog settles its size (and so its place) over the first frames.
    h.run_steps(30);
    let radio = h.get_by_label("Run a command").rect();
    click(&mut h, radio.center());
    assert_eq!(h.state().dialog.as_ref().map(|d| d.values["mode"].clone()), Some(json!("command")));
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("command".into(), json!("cell.set"));
        d.values.insert("params".into(), json!("{\"cell\": \"H2\", \"input\": \"cmd\"}"));
    }
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().message);
    assert!(matches!(control(&h, ids.button).action, Some(gridcraft_engine::model::ControlAction::Command { ref id, .. }) if id == "cell.set"));
    h.state_mut().selected_object = None;
    click(&mut h, r.center());
    assert_eq!(cell(&h, "H2"), Value::text("cmd"), "{:?}", h.state().message);

    // Format Control: refuses broken limits, then applies good ones and writes the link.
    h.state_mut().open_dialog("formatControl", json!({"id": ids.spin}));
    h.run_steps(2);
    assert!(h.query_by_label("Incremental change:").is_some());
    assert!(h.query_by_label("Page change:").is_none(), "a spin button has no page change");
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("max".into(), json!("0"));
    }
    h.run_steps(2);
    assert!(h.query_by_label_contains("must be greater than Minimum").is_some());
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("max".into(), json!("40"));
        d.values.insert("value".into(), json!("30"));
        d.values.insert("link".into(), json!("$D$1"));
    }
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(2);
    let c = control(&h, ids.spin);
    assert_eq!((c.max, c.value, c.link.as_deref()), (40, 30, Some("$D$1")));
    assert_eq!(cell(&h, "D1"), Value::Number(30.0));

    // Double-clicking a selected button edits its text.
    h.state_mut().selected_object = Some(ObjRef::new(ObjKind::Control, ids.button));
    h.state_mut().open_object(ObjRef::new(ObjKind::Control, ids.button));
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.title.as_str()), Some("Edit Text"));
}
