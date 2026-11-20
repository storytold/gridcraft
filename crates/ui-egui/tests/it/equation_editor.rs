//! The equation editor through real input: opening it from the ribbon and its access keys,
//! typing LaTeX, the quick-insert buttons, the error in the preview, inserting, and editing an
//! equation by double-clicking it.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, os::OperatingSystem, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::model::{ObjKind, ObjRef, Shape, ShapeKind};
use gridcraft_ui_egui::{SheetApp, chartview, grid::Geo};
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(s: Session) -> Harness {
    let mut h =
        egui_kittest::Harness::builder().with_os(OperatingSystem::Mac).with_size(vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_ui_state(
            |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SheetApp::new(s, Default::default()),
        );
    h.run_steps(4);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

fn type_text(h: &mut Harness, text: &str) {
    h.input_mut().events.push(Event::Text(text.into()));
    h.run_steps(2);
}

fn cmd_enter(h: &mut Harness) {
    h.input_mut().events.extend([key_event(Key::Enter, true, Modifiers::COMMAND), key_event(Key::Enter, false, Modifiers::COMMAND)]);
    h.run_steps(3);
}

fn dialog_latex(h: &Harness) -> Option<String> {
    h.state().dialog.as_ref().filter(|d| d.name() == "equation").map(|d| d.values.get("latex").and_then(Json::as_str).unwrap_or("").to_string())
}

fn equations(h: &Harness) -> Vec<Shape> {
    h.state().session.doc().unwrap().wb.active().unwrap().shapes.iter().filter(|s| s.kind == ShapeKind::Equation).cloned().collect()
}

#[test]
fn the_ribbon_button_opens_the_editor_and_typing_inserts_an_equation() {
    let mut h = harness(blank());
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Insert"})).unwrap();
    h.run_steps(2);
    h.get_by_label("Equation").click();
    h.run_steps(3);
    assert_eq!(dialog_latex(&h).as_deref(), Some(""), "a new, empty equation");
    // The source has focus: typing goes straight in.
    type_text(&mut h, "x^2 + y^2 = r^2");
    assert_eq!(dialog_latex(&h).as_deref(), Some("x^2 + y^2 = r^2"));
    assert!(h.query_by_label_contains("Error at character").is_none());
    cmd_enter(&mut h);
    assert!(h.state().dialog.is_none(), "Cmd+Enter inserts and closes");
    let eqs = equations(&h);
    assert_eq!(eqs.len(), 1);
    assert_eq!(eqs[0].text, "x^2 + y^2 = r^2");
    assert_eq!(eqs[0].anchor.cell.a1(), "A1", "at the active cell");
}

#[test]
fn errors_show_their_position_and_block_ok() {
    let mut h = harness(blank());
    h.state_mut().open_dialog("equation", json!({}));
    h.run_steps(3);
    type_text(&mut h, "a + \\foo");
    assert!(h.query_by_label_contains("Error at character 5").is_some(), "the error names the position");
    cmd_enter(&mut h);
    assert!(h.state().dialog.is_some(), "an equation with an error can't be inserted");
    assert!(equations(&h).is_empty());
    // The formula bar's cancel button shares the label; the dialog's comes last.
    h.get_all_by_label("Cancel").last().expect("the dialog's Cancel").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert!(equations(&h).is_empty());
}

#[test]
fn quick_insert_buttons_insert_at_the_caret() {
    let mut h = harness(blank());
    h.state_mut().open_dialog("equation", json!({}));
    h.run_steps(3);
    type_text(&mut h, "y = ");
    h.get_by_label("a/b").click();
    h.run_steps(3);
    assert_eq!(dialog_latex(&h).as_deref(), Some("y = \\frac{}{}"));
    // The caret sits in the numerator.
    type_text(&mut h, "1");
    assert_eq!(dialog_latex(&h).as_deref(), Some("y = \\frac{1}{}"));
    h.get_by_label("\u{03C0}").click();
    h.run_steps(3);
    assert_eq!(dialog_latex(&h).as_deref(), Some("y = \\frac{1\\pi }{}"));
}

#[test]
fn the_access_keys_open_the_editor() {
    let mut h = harness(blank());
    h.input_mut().events.extend([key_event(Key::F10, true, Modifiers::NONE), key_event(Key::F10, false, Modifiers::NONE)]);
    h.run_steps(2);
    for k in [Key::N, Key::E] {
        h.input_mut().events.extend([
            key_event(k, true, Modifiers::NONE),
            Event::Text(k.name().to_ascii_lowercase()),
            key_event(k, false, Modifiers::NONE),
        ]);
        h.run_steps(2);
    }
    h.run_steps(2);
    assert_eq!(dialog_latex(&h).as_deref(), Some(""), "N, E opens Insert Equation");
}

#[test]
fn double_click_edits_an_equation_in_one_undo_step() {
    let mut s = blank();
    let id =
        s.execute("insert.equation", json!({"latex": "a^2 + b^2 = c^2", "cell": "C4", "size": 18})).unwrap()["equation"].as_u64().unwrap() as u32;
    let mut h = harness(s);
    let o = ObjRef::new(ObjKind::Equation, id);
    let r = {
        let app = h.state();
        let sh = app.session.doc().unwrap().wb.active().unwrap();
        chartview::object_rect(&Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll), sh, o).unwrap()
    };
    h.run_steps(45);
    for _ in 0..2 {
        for pressed in [true, false] {
            let p: Pos2 = r.center();
            h.input_mut().events.push(Event::PointerMoved(p));
            h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            h.step();
        }
        h.run_steps(2);
    }
    assert_eq!(h.state().selected_object, Some(o));
    assert_eq!(dialog_latex(&h).as_deref(), Some("a^2 + b^2 = c^2"), "the editor opens on the equation's source");
    assert_eq!(h.state().dialog.as_ref().unwrap().title, "Edit Equation");
    h.state_mut().dialog.as_mut().unwrap().values.insert("latex".into(), json!("a^2 + b^2 = c^2 + d^2"));
    h.run_steps(2);
    cmd_enter(&mut h);
    assert!(h.state().dialog.is_none());
    assert_eq!(equations(&h)[0].text, "a^2 + b^2 = c^2 + d^2");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert_eq!(equations(&h)[0].text, "a^2 + b^2 = c^2");
}

#[test]
fn the_gallery_inserts_its_examples() {
    let mut h = harness(blank());
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Insert"})).unwrap();
    h.run_steps(2);
    h.get_by_label("Equation options").click();
    h.run_steps(3);
    assert!(h.query_by_label("Insert New Equation...").is_some());
    // Gallery rows are painted previews; each carries its source as hover text. Click the first.
    let first = h.get_by_label("Insert New Equation...").rect();
    let row = first.center() + vec2(0.0, first.height() / 2.0 + 40.0);
    h.input_mut().events.push(Event::PointerMoved(row));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: row, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.step();
    }
    h.run_steps(3);
    let eqs = equations(&h);
    assert_eq!(eqs.len(), 1, "one gallery example inserted");
    assert_eq!(eqs[0].text, gridcraft_engine::math::GALLERY[0].1);
}

#[test]
fn reopening_after_escape_focuses_the_source_again() {
    let mut h = harness(blank());
    h.state_mut().run("ui.ribbonTab", json!({"tab": "Insert"})).unwrap();
    h.run_steps(2);
    for round in 0..2 {
        h.get_by_label("Equation").click();
        h.run_steps(3);
        assert_eq!(dialog_latex(&h).as_deref(), Some(""), "round {round}: the editor is open");
        type_text(&mut h, "x");
        assert_eq!(dialog_latex(&h).as_deref(), Some("x"), "round {round}: typing goes into the source");
        // Escape closes the dialog without OK or Cancel.
        h.input_mut().events.extend([key_event(Key::Escape, true, Modifiers::NONE), key_event(Key::Escape, false, Modifiers::NONE)]);
        h.run_steps(3);
        assert!(h.state().dialog.is_none(), "round {round}: Escape closes the editor");
    }
    assert!(equations(&h).is_empty(), "nothing was inserted");
}
