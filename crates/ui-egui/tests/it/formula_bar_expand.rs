//! Ctrl+Shift+U (Control+Shift+U in Excel for Mac) expands and collapses the formula bar.

use egui::{Event, Key, Modifiers, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::keymap::{Shortcut, lookup};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Ctrl+Shift on Windows and Linux sets `ctrl`, `command` and `shift`.
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };
/// Control+Shift on a Mac: `ctrl` without `command`.
const MAC_CONTROL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: false };

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "text"})).unwrap();
    session.execute("selection.set", json!({"range": "A1"})).unwrap();
    crate::support::harness(session, 4)
}

fn ctrl_shift_u(h: &mut Harness, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: Key::U, physical_key: Some(Key::U), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: Key::U, physical_key: Some(Key::U), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn expanded(h: &Harness) -> bool {
    h.state().ui.formula_bar_expanded
}

fn underlined(h: &Harness) -> bool {
    h.state().session.journal.iter().any(|(id, _)| id == "home.underline")
}

#[test]
fn ctrl_shift_u_on_the_grid_expands_and_collapses_the_formula_bar() {
    let mut h = harness();
    assert!(!expanded(&h));
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(expanded(&h), "the first Ctrl+Shift+U expands the formula bar");
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(!expanded(&h), "the second Ctrl+Shift+U collapses it");
    assert!(!underlined(&h), "Ctrl+Shift+U is not Underline");
}

#[test]
fn control_shift_u_expands_the_formula_bar_in_excel_for_mac() {
    let mut h = harness();
    h.ctx.set_os(egui::os::OperatingSystem::Mac);
    h.run_steps(2);
    ctrl_shift_u(&mut h, MAC_CONTROL_SHIFT);
    assert!(expanded(&h));
    assert!(h.state().editor.is_none(), "Control+Shift+U does not start editing as Control+U does");
    let expand = Some(Shortcut::Run("view.expandFormulaBar", json!({})));
    assert_eq!(lookup(Key::U, Some(Key::U), MAC_CONTROL_SHIFT, true), expand);
    assert_eq!(lookup(Key::U, Some(Key::U), CTRL_SHIFT, false), expand);
    // Control+U on a Mac still edits the active cell.
    assert_eq!(lookup(Key::U, Some(Key::U), Modifiers::CTRL, true), Some(Shortcut::Edit));
}

#[test]
fn ctrl_shift_u_while_editing_keeps_the_entry() {
    let mut h = harness();
    h.state_mut().begin_edit(Some("draft".into()), false);
    h.run_steps(2);
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(expanded(&h), "Ctrl+Shift+U works in the cell editor too");
    let ed = h.state().editor.as_ref().expect("the edit stays open");
    assert_eq!(ed.text, "draft");
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    assert!(!expanded(&h));
    assert!(h.state().editor.is_some());
}

#[test]
fn expand_formula_bar_command_sets_or_toggles_the_state() {
    let mut h = harness();
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"on": true})), Ok(json!({"on": true})));
    assert!(expanded(&h));
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"on": true})), Ok(json!({"on": true})));
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({})), Ok(json!({"on": false})));
    assert!(!expanded(&h));
}

fn drag(h: &mut Harness, from: egui::Pos2, to: egui::Pos2) {
    use egui::PointerButton;
    h.input_mut().events.extend([
        Event::PointerMoved(from),
        Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
    ]);
    h.step();
    for i in 1..=6 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
        h.step();
    }
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(20);
}

fn bar(h: &Harness) -> gridcraft_ui_egui::formula_bar::BarLayout {
    gridcraft_ui_egui::formula_bar::BarLayout::get(&h.ctx)
}

#[test]
fn dragging_the_bottom_edge_sets_the_height_that_expanding_returns_to() {
    let mut h = harness();
    h.run_steps(20);
    let start = bar(&h);
    assert_eq!(start.visible_lines, 1, "one line of text shows a one-line bar");
    assert!(
        start.handle.height() > 0.0 && (start.handle.bottom() - start.field.bottom()).abs() <= 4.0,
        "the handle is the bar's bottom edge: {start:?}"
    );
    // Drag the edge down about five and a half rows: the bar snaps to whole lines.
    let row = 17.0;
    drag(&mut h, start.handle.center(), start.handle.center() + vec2(0.0, 5.5 * row));
    let lines = h.state().ui.formula_bar_lines;
    assert!(expanded(&h), "a taller bar is expanded");
    assert!((6..=7).contains(&lines), "about six more lines: {lines}");
    assert_eq!(bar(&h).visible_lines, lines, "the field shows the dragged lines: {:?}", bar(&h));
    // Collapse, then expand: back to the dragged height, not the default five lines.
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    h.run_steps(20);
    assert!(!expanded(&h));
    assert_eq!(bar(&h).visible_lines, 1);
    ctrl_shift_u(&mut h, CTRL_SHIFT);
    h.run_steps(20);
    assert_eq!(bar(&h).visible_lines, lines, "expanding returns to the dragged height");
    // Dragging the edge up to one line collapses the bar and keeps the expanded height.
    let edge = bar(&h).handle.center();
    drag(&mut h, edge, edge - vec2(0.0, 20.0 * row));
    assert!(!expanded(&h), "a one-line bar is collapsed");
    assert_eq!(bar(&h).visible_lines, 1);
    assert_eq!(h.state().ui.formula_bar_lines, lines);
}

#[test]
fn expand_formula_bar_with_lines_sets_the_expanded_height() {
    let mut h = harness();
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"lines": 8})), Ok(json!({"on": true, "lines": 8})));
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"lines": 1})), Ok(json!({"on": false, "lines": 8})), "one line collapses");
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({})), Ok(json!({"on": true})));
    assert_eq!(h.state().ui.formula_bar_lines, 8, "expanding returns to the set height");
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"lines": 0})), Ok(json!({"on": false, "lines": 8})), "zero lines is one line");
    assert_eq!(h.state_mut().run("view.expandFormulaBar", json!({"lines": u64::MAX})), Ok(json!({"on": true, "lines": 100})), "capped");
}

#[test]
fn an_expanded_formula_bar_leaves_room_for_the_worksheet_in_a_short_window() {
    // At 400 pt the title bar and ribbon leave about 236 pt: half the window as a formula bar
    // would leave the status bar and sheet tabs no room for any of the grid.
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 400.0)), crate::common::app(session), 4);
    h.state_mut().run("view.expandFormulaBar", json!({"lines": 100})).unwrap();
    h.run_steps(60);
    let cells = h.state().grid.cells_rect.expect("the grid is drawn");
    assert!(cells.height() >= two_rows_at(100.0) - 1.0, "two rows stay visible under a {} pt bar: {cells:?}", bar(&h).height);
    assert!(bar(&h).visible_lines > 1, "the bar is still expanded");
}

/// Two default rows at `percent` zoom, as the grid draws them.
fn two_rows_at(percent: f32) -> f32 {
    2.0 * gridcraft_engine::model::DEFAULT_ROW_HEIGHT * percent / 100.0 * gridcraft_ui_egui::grid::DISPLAY_SCALE
}

#[test]
fn an_expanded_formula_bar_leaves_room_for_the_worksheet_at_any_zoom() {
    // At 400% the headings and rows are four times taller: the room kept under the bar grows too.
    let mut session = Session::new();
    session.new_workbook();
    session.execute("view.zoom", json!({"percent": 400})).unwrap();
    let mut h = crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 600.0)), crate::common::app(session), 4);
    h.state_mut().run("view.expandFormulaBar", json!({"lines": 100})).unwrap();
    h.run_steps(60);
    let cells = h.state().grid.cells_rect.expect("the grid is drawn");
    assert!(cells.height() >= two_rows_at(400.0) - 1.0, "two rows stay visible under a {} pt bar: {cells:?}", bar(&h).height);
}

#[test]
fn a_growing_collapsed_formula_bar_leaves_room_for_the_worksheet_in_a_short_window() {
    // A five-line cell grows the collapsed bar to five lines, but not past the room the window has.
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "a\nb\nc\nd\ne"})).unwrap();
    session.execute("selection.set", json!({"range": "A1"})).unwrap();
    let mut h = crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 360.0)), crate::common::app(session), 4);
    h.run_steps(60);
    assert!(!expanded(&h));
    let cells = h.state().grid.cells_rect.expect("the grid is drawn");
    assert!(cells.height() >= two_rows_at(100.0) - 1.0, "two rows stay visible under a {} pt bar: {cells:?}", bar(&h).height);
}

#[test]
fn an_expanded_formula_bar_leaves_room_for_the_page_layout_ruler() {
    // Page Layout view takes the ruler above the headings out of the grid: the room kept under
    // the bar holds it as well as the headings and two rows. 480 pt leaves that room under a bar
    // of a few lines whatever the UI font (at 400 pt only a one-line bar fits, on Linux not even
    // that).
    let mut session = Session::new();
    session.new_workbook();
    session.execute("view.pageLayout", json!({})).unwrap();
    let mut h = crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 480.0)), crate::common::app(session), 4);
    h.state_mut().run("view.expandFormulaBar", json!({"lines": 100})).unwrap();
    h.run_steps(60);
    let grid = h.state().grid.rect.expect("the grid is drawn");
    // The first row starts below the page gap and the page's top margin, and rows are drawn at
    // the print scale too.
    let (_, pages) = h.state().grid.paper.clone().expect("Page Layout view paginated the sheet");
    let z = gridcraft_ui_egui::grid::DISPLAY_SCALE * pages.scale;
    let need = gridcraft_ui_egui::paper_view::RULER
        + gridcraft_ui_egui::grid::COL_HEADER_H
        + (pages.gap + pages.margins[2]) as f32 * z
        + 2.0 * gridcraft_engine::model::DEFAULT_ROW_HEIGHT * z;
    assert!(grid.height() >= need - 1.0, "ruler, headings, page top and two rows under a {} pt bar: {grid:?} < {need}", bar(&h).height);
    assert!(bar(&h).visible_lines > 1, "the bar is still expanded");
}

#[test]
fn an_expanded_formula_bar_leaves_room_for_the_column_outline() {
    // A column outline puts its band of level buttons above the headings: the room kept under
    // the bar holds the band as well as the headings and two rows, under a bar of a few lines.
    let mut session = Session::new();
    session.new_workbook();
    for cols in ["B:G", "C:F", "D:E"] {
        session.execute("data.group", json!({"cols": cols})).unwrap();
    }
    let mut h = crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 480.0)), crate::common::app(session), 4);
    h.state_mut().run("view.expandFormulaBar", json!({"lines": 100})).unwrap();
    h.run_steps(60);
    let cells = h.state().grid.cells_rect.expect("the grid is drawn");
    assert!(cells.height() >= two_rows_at(100.0) - 1.0, "two rows stay visible under a {} pt bar: {cells:?}", bar(&h).height);
    assert!(bar(&h).visible_lines > 1, "the bar is still expanded");
}
