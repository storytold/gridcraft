//! Formulas › Error Checking is a split button, as in Excel: the button opens the Error Checking
//! dialog, and its arrow opens a menu whose Circular References submenu lists a cell on each
//! circular reference and selects the one picked, on its own sheet.

use egui::{Event, Shape, pos2, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// The submenu item, with the arrow egui puts after a submenu's label.
const CIRCULAR: &str = "Circular References ⏵";

fn harness(loops: bool) -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.insertSheet", json!({"name": "Q1 Data"})).unwrap();
    if loops {
        s.execute("cell.set", json!({"cell": "B3", "input": "=B4"})).unwrap();
        s.execute("cell.set", json!({"cell": "B4", "input": "=B3"})).unwrap();
    }
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    if loops {
        s.execute("cell.set", json!({"cell": "C2", "input": "=C2+1"})).unwrap();
    }
    s.execute("selection.set", json!({"range": "H20"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1400.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().ui.ribbon_tab = "Formulas".into();
    h.run_steps(4);
    h
}

/// The painted "Error Checking" ribbon label: its left edge, right edge and vertical center.
fn label(h: &Harness) -> (f32, f32, f32) {
    h.output()
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Text(t) if t.galley.job.text == "Error Checking" => {
                Some((t.pos.x, t.pos.x + t.galley.size().x, t.pos.y + t.galley.size().y / 2.0))
            }
            _ => None,
        })
        .expect("the Formulas tab shows Error Checking")
}

fn click(h: &mut Harness, at: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(2);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
}

fn open_menu(h: &mut Harness) {
    let (_, right, y) = label(h);
    // The arrow sits right of the label, past the 6 px gap after the text.
    click(h, pos2(right + 13.0, y));
}

fn hover(h: &mut Harness, label: &str) {
    let at = h.get_by_label(label).rect().center();
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(6);
}

#[test]
fn error_checking_button_opens_the_dialog_and_its_arrow_opens_the_menu() {
    let mut h = harness(false);
    let (left, _, y) = label(&h);
    click(&mut h, pos2(left + 4.0, y));
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("errorChecking"), "the button opens Error Checking");
    h.state_mut().dialog = None;
    h.run_steps(2);
    open_menu(&mut h);
    assert!(h.state().dialog.is_none(), "the arrow opens a menu, not the dialog");
    h.get_by_label("Error Checking…").click();
    h.run_steps(4);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("errorChecking"), "the menu's first item opens Error Checking");
}

#[test]
fn circular_references_is_unavailable_without_a_circular_reference() {
    let mut h = harness(false);
    open_menu(&mut h);
    assert!(h.get_by_label(CIRCULAR).accesskit_node().is_disabled());
}

#[test]
fn circular_references_lists_the_loops_and_selects_the_cell_picked() {
    let mut h = harness(true);
    open_menu(&mut h);
    assert!(!h.get_by_label(CIRCULAR).accesskit_node().is_disabled());
    hover(&mut h, CIRCULAR);
    assert!(h.query_by_label("Sheet1!C2").is_some(), "the loop on the active sheet is listed");
    // A sheet name with a space is quoted, as in a formula.
    let other = ["'Q1 Data'!B3", "'Q1 Data'!B4"].into_iter().find(|l| h.query_by_label(l).is_some()).expect("the loop on Q1 Data is listed");
    h.get_by_label(other).click();
    h.run_steps(4);
    let app = h.state();
    let d = app.session.active().unwrap();
    assert_eq!(d.wb.active_sheet, 1, "picking a cell on Q1 Data goes to Q1 Data");
    assert_eq!(format!("'Q1 Data'!{}", d.selection.active.a1()), other);
}

#[test]
fn circular_references_scrolls_to_the_cell_picked() {
    let mut h = harness(false);
    h.state_mut().run("cell.set", json!({"cell": "C500", "input": "=C500+1"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "H20"})).unwrap();
    h.run_steps(4);
    assert_eq!(h.state().view().scroll.y, 0.0, "setup: the view is at the top of the sheet");
    open_menu(&mut h);
    hover(&mut h, CIRCULAR);
    h.get_by_label("Sheet1!C500").click();
    h.run_steps(4);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "C500");
    assert!(h.state().view().scroll.y > 0.0, "the grid scrolls to the cell picked");
}

#[test]
fn circular_references_scrolls_a_list_taller_than_the_window() {
    let mut h = harness(false);
    for row in 1..=100 {
        h.state_mut().run("cell.set", json!({"cell": format!("A{row}"), "input": format!("=A{row}+1")})).unwrap();
    }
    h.state_mut().run("selection.set", json!({"range": "H20"})).unwrap();
    h.run_steps(4);
    open_menu(&mut h);
    hover(&mut h, CIRCULAR);
    let window = h.ctx.content_rect();
    assert!(h.get_by_label("Sheet1!A100").rect().bottom() > window.bottom(), "setup: the last cell starts below the window");
    hover(&mut h, "Sheet1!A1");
    for _ in 0..20 {
        h.input_mut().events.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: vec2(0.0, -10.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(4);
    }
    let last = h.get_by_label("Sheet1!A100").rect();
    assert!(last.bottom() <= window.bottom(), "the wheel brings the last cell into the window: {last:?} in {window:?}");
    click(&mut h, last.center());
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A100");
}
