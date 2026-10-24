//! Text-box editing through real pointer and keyboard input, including document boundaries.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{Shape, Workbook};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const ORIGINAL: &str = "First line\nSecond line";

fn at(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

fn fixture() -> (Harness, u32) {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B2", "input": "cell stays"})).unwrap();
    session.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let id = session.execute("insert.textBox", json!({"at": "B2", "width": 240, "height": 120, "text": ORIGINAL})).unwrap()["shape"].as_u64().unwrap()
        as u32;
    let mut h = egui_kittest::Harness::builder()
        .with_size(vec2(1200.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_os(egui::os::OperatingSystem::Mac)
        .build_ui_state(
            |ui, app: &mut SheetApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            SheetApp::new(session, Default::default()),
        );
    h.run_steps(4);
    (h, id)
}

fn shape(h: &Harness, id: u32) -> &Shape {
    h.state().session.doc().unwrap().wb.active().unwrap().shapes.iter().find(|s| s.id == id).unwrap()
}

fn shape_text(wb: &Workbook, sheet: usize, id: u32) -> &str {
    &wb.sheet(sheet).unwrap().shapes.iter().find(|s| s.id == id).unwrap().text
}

fn shape_rect(h: &Harness, id: u32) -> Rect {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let a = shape(h, id).anchor;
    Rect::from_min_size(pos2(geo.x(sh, a.cell.col) + a.dx * geo.z, geo.y(sh, a.cell.row) + a.dy * geo.z), vec2(a.width, a.height) * geo.z)
}

fn cell_center(h: &Harness, address: &str) -> Pos2 {
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sh, at(address)).center()
}

fn pointer(h: &mut Harness, p: Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
    h.step();
}

fn click(h: &mut Harness, p: Pos2) {
    pointer(h, p, true);
    pointer(h, p, false);
    h.run_steps(2);
}

fn edit_box(h: &mut Harness, id: u32) {
    // Separate this gesture from egui's 600 ms triple-click window as well.
    h.run_steps(45);
    let p = shape_rect(h, id).center();
    click(h, p);
    click(h, p);
}

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    h.input_mut().events.push(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    h.step();
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

fn event(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(2);
}

fn replace(h: &mut Harness, text: &str) {
    key(h, Key::A, Modifiers::COMMAND);
    event(h, Event::Paste(text.into()));
}

fn assert_cell_unchanged(h: &Harness) {
    assert_eq!(h.state().session.doc().unwrap().wb.active().unwrap().value(at("B2")), Value::text("cell stays"));
    assert!(h.state().editor.is_none(), "text-box input must not start a cell editor");
}

#[test]
fn double_click_edits_multiline_text_and_commits_on_outside_click() {
    let (mut h, id) = fixture();
    edit_box(&mut h, id);
    assert_eq!(h.state().selected_chart, Some(id));
    let anchor = shape(&h, id).anchor;
    let start = shape_rect(&h, id).min + vec2(5.0, 10.0);
    drag(&mut h, start, start + vec2(60.0, 0.0));
    assert_eq!(shape(&h, id).anchor, anchor, "dragging inside the editor selects text instead of moving the box");
    h.input_mut().events.push(Event::Copy);
    h.step();
    assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if !text.is_empty())));
    key(&mut h, Key::A, Modifiers::COMMAND);
    h.input_mut().events.push(Event::Copy);
    h.step();
    assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(text) if text == ORIGINAL)));
    event(&mut h, Event::Cut);
    event(&mut h, Event::Paste("New first line".into()));
    key(&mut h, Key::Enter, Modifiers::NONE);
    event(&mut h, Event::Text("New second line".into()));
    assert_cell_unchanged(&h);
    assert_eq!(h.state().session.doc().unwrap().selection.active, at("B2"));

    let outside = cell_center(&h, "J15");
    click(&mut h, outside);
    assert_eq!(shape(&h, id).text, "New first line\nNew second line");
    assert_cell_unchanged(&h);
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    pointer(h, from, true);
    // Cross egui's drag threshold while still inside the object/resize handle.
    h.input_mut().events.push(Event::PointerMoved(from + vec2(5.0, 5.0)));
    h.step();
    h.input_mut().events.push(Event::PointerMoved(to));
    h.step();
    pointer(h, to, false);
    h.run_steps(2);
}

#[test]
fn escape_discards_text_and_the_box_still_moves_and_resizes() {
    let (mut h, id) = fixture();
    edit_box(&mut h, id);
    replace(&mut h, "discard this edit");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(shape(&h, id).text, ORIGINAL);
    assert_cell_unchanged(&h);
    h.run_steps(30);

    let before = shape_rect(&h, id);
    drag(&mut h, before.center(), before.center() + vec2(40.0, 30.0));
    let moved = shape_rect(&h, id);
    assert!(moved.left() > before.left() && moved.top() > before.top(), "ordinary dragging still moves the box");
    assert!((moved.size() - before.size()).abs().max_elem() < 0.1);

    let handle = moved.right_bottom() - vec2(7.0, 7.0);
    drag(&mut h, handle, handle + vec2(45.0, 25.0));
    let resized = shape_rect(&h, id);
    assert!(resized.width() > moved.width() && resized.height() > moved.height(), "the corner still resizes the box");
    assert_eq!(shape(&h, id).text, ORIGINAL);
    assert_cell_unchanged(&h);

    edit_box(&mut h, id);
    replace(&mut h, "Offscreen draft");
    h.state_mut().view_mut().unwrap().scroll = vec2(900.0, 700.0);
    h.run_steps(2);
    assert_eq!(h.state().text_box_editor.as_ref().unwrap().text, "Offscreen draft");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(shape(&h, id).text, ORIGINAL);
    assert_cell_unchanged(&h);
}

#[test]
fn saving_and_switching_context_commit_to_the_original_text_box() {
    let (mut h, id) = fixture();
    edit_box(&mut h, id);
    replace(&mut h, "Saved draft\nSecond paragraph");
    h.state_mut().run("sheet.read", json!({})).unwrap();
    assert_eq!(shape(&h, id).text, ORIGINAL, "inspecting the sheet must not commit a draft");
    assert!(h.state().text_box_editor.is_some());
    let saved = h.state_mut().run("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let bytes = gridcraft_engine::io::base64_decode(saved["base64"].as_str().unwrap()).unwrap();
    let (reopened, _) = gridcraft_engine::io::open_bytes("edited.xlsx", &bytes).unwrap();
    assert_eq!(shape_text(&reopened, 0, id), "Saved draft\nSecond paragraph");
    assert_eq!(reopened.sheet(0).unwrap().value(at("B2")), Value::text("cell stays"));

    // Save and Save As must work while the editor still owns keyboard focus.
    let downloaded = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = downloaded.clone();
    h.state_mut().services.download = Some(Box::new(move |_, bytes| *sink.lock().unwrap() = bytes.to_vec()));
    for modifiers in [Modifiers::COMMAND, Modifiers::COMMAND | Modifiers::SHIFT] {
        edit_box(&mut h, id);
        replace(&mut h, "Saved from the text editor");
        key(&mut h, Key::S, modifiers);
        let (saved, _) = gridcraft_engine::io::open_bytes("shortcut.xlsx", &downloaded.lock().unwrap()).unwrap();
        assert_eq!(shape_text(&saved, 0, id), "Saved from the text editor");
        downloaded.lock().unwrap().clear();
    }

    h.state_mut().run("home.insertSheet", json!({"name": "Other"})).unwrap();
    let other_id = h.state_mut().run("insert.textBox", json!({"at": "B2", "text": "Other sheet"})).unwrap()["shape"].as_u64().unwrap() as u32;
    h.state_mut().run("sheet.activate", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    edit_box(&mut h, id);
    replace(&mut h, "Original sheet only");
    h.state_mut().run("sheet.activate", json!({"sheet": 1})).unwrap();
    let wb = &h.state().session.doc().unwrap().wb;
    assert_eq!(shape_text(wb, 0, id), "Original sheet only");
    assert_eq!(shape_text(wb, 1, other_id), "Other sheet");

    h.state_mut().run("file.new", json!({})).unwrap();
    let other_doc_id = h.state_mut().run("insert.textBox", json!({"at": "B2", "text": "Other workbook"})).unwrap()["shape"].as_u64().unwrap() as u32;
    h.state_mut().run("window.activate", json!({"index": 0})).unwrap();
    h.state_mut().run("sheet.activate", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    edit_box(&mut h, id);
    replace(&mut h, "Original workbook only");
    h.state_mut().run("window.activate", json!({"index": 1})).unwrap();
    let docs = h.state().session.documents();
    assert_eq!(shape_text(&docs[0].wb, 0, id), "Original workbook only");
    assert_eq!(shape_text(&docs[1].wb, 0, other_doc_id), "Other workbook");
    assert_eq!(docs[0].wb.sheet(0).unwrap().value(at("B2")), Value::text("cell stays"));
}

#[test]
fn protected_sheet_refuses_to_open_the_text_box_editor() {
    let (mut h, id) = fixture();
    h.state_mut().session.execute("review.protectSheet", json!({})).unwrap();
    edit_box(&mut h, id);
    assert!(h.state().text_box_editor.is_none(), "a protected sheet's text box must not open an editor");
    assert!(h.state().message.as_ref().is_some_and(|(_, m)| m.contains("protected sheet")));
    assert_eq!(shape(&h, id).text, ORIGINAL);
    assert_cell_unchanged(&h);
}
