//! Reference boxes (RefEdit) in dialogs: selecting on the grid fills the box, and the collapse
//! button shrinks the dialog to the box while the person selects. Real pointer and keyboard input.

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::control::{Outcome, handle};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type H = egui_kittest::Harness<'static, SheetApp>;

/// A control-channel request, as a script or an agent sends it.
fn control(h: &mut H, method: &str, params: serde_json::Value) -> serde_json::Value {
    let ctx = h.ctx.clone();
    let r = match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    };
    h.run_steps(3);
    r
}

fn harness(session: Session) -> H {
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

/// A workbook whose B5 is a formula of B2, with B2 selected.
fn workbook() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "B2", "input": "1"})).unwrap();
    s.execute("cell.set", json!({"cell": "B5", "input": "=B2*3+4"})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    s
}

fn open_goal_seek(h: &mut H) {
    h.state_mut().open_dialog("goalSeek", json!({}));
    h.run_steps(4);
}

fn field(h: &H, key: &str) -> String {
    h.state().dialog.as_ref().and_then(|d| d.values.get(key).and_then(|v| v.as_str()).map(str::to_string)).unwrap_or_default()
}

fn collapsed(h: &H) -> Option<String> {
    h.state().dialog.as_ref().and_then(|d| d.collapsed().map(str::to_string))
}

fn selection(h: &H) -> (usize, String) {
    let d = h.state().session.active().unwrap();
    (d.wb.active_sheet(), d.selection.a1())
}

fn cell_pos(h: &H, cell: &str) -> egui::Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
}

fn press(h: &mut H, pos: egui::Pos2, pressed: bool) {
    h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
}

/// Presses on `from`, drags to `to` and releases, as selecting a range with the mouse.
fn drag(h: &mut H, from: &str, to: &str) {
    let (a, b) = (cell_pos(h, from), cell_pos(h, to));
    h.input_mut().events.push(Event::PointerMoved(a));
    press(h, a, true);
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerMoved((a + b.to_vec2()) / 2.0));
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerMoved(b));
    h.run_steps(2);
    press(h, b, false);
    h.run_steps(3);
}

fn click_at(h: &mut H, pos: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(pos));
    press(h, pos, true);
    press(h, pos, false);
    h.run_steps(3);
}

fn key(h: &mut H, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
    h.run_steps(3);
}

fn type_text(h: &mut H, t: &str) {
    h.input_mut().events.push(Event::Text(t.into()));
    h.run_steps(3);
}

/// The Goal Seek window (the collapsed box is also named after the dialog).
fn dialog_rect(h: &H) -> egui::Rect {
    h.get_by_role_and_label(egui::accesskit::Role::Window, "Goal Seek").rect()
}

/// The Goal Seek dialog's text boxes, top to bottom: Set cell, To value, By changing cell.
fn dialog_boxes(h: &H) -> Vec<egui::Rect> {
    let title = dialog_rect(h);
    h.get_all_by_role(egui::accesskit::Role::TextInput).map(|n| n.rect()).filter(|r| title.contains(r.center())).collect()
}

/// Clicks the collapse (or expand) button of the dialog's `n`th reference box.
fn click_button(h: &mut H, label: &str, n: usize) {
    let rect = h.get_all_by_label(label).nth(n).map(|b| b.rect()).unwrap();
    click_at(h, rect.center());
}

#[test]
fn collapsing_shrinks_the_dialog_and_a_dragged_range_fills_the_box() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    assert_eq!(h.get_all_by_label("Collapse dialog").count(), 2, "Set cell and By changing cell are reference boxes");
    let full = dialog_rect(&h);
    click_button(&mut h, "Collapse dialog", 1);
    assert_eq!(collapsed(&h).as_deref(), Some("changing"));
    assert!(h.query_by_label("To value:").is_none(), "only the reference box shows while collapsed");
    assert_eq!(h.get_all_by_label("Expand dialog").count(), 1);
    assert_eq!(dialog_rect(&h).top(), full.top(), "the collapsed dialog keeps its place");

    drag(&mut h, "C3", "D6");
    assert_eq!(field(&h, "changing"), "$C$3:$D$6");
    assert_eq!(field(&h, "set"), "B5", "only the box being picked into changes");

    // The box has the keyboard back after the drag: typing replaces the reference.
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "B2");
    assert_eq!(field(&h, "changing"), "B2");

    // Escape expands the dialog instead of closing it.
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_some());
    assert_eq!(collapsed(&h), None);
    assert!(h.query_by_label("To value:").is_some());
    assert_eq!(dialog_rect(&h), full, "expanded, the dialog comes back where it was");
}

#[test]
fn enter_and_the_expand_button_restore_the_dialog_and_ok_runs_goal_seek() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    drag(&mut h, "B2", "B2");
    assert_eq!(field(&h, "changing"), "$B$2");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(collapsed(&h), None, "Enter expands the dialog");
    assert!(h.state().dialog.is_some(), "Enter in the collapsed dialog does not press OK");

    click_button(&mut h, "Collapse dialog", 0);
    assert_eq!(collapsed(&h).as_deref(), Some("set"));
    click_button(&mut h, "Expand dialog", 0);
    assert_eq!(collapsed(&h), None);

    let to = dialog_boxes(&h)[1];
    click_at(&mut h, to.center());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "25");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let b2 = h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("B2").unwrap());
    assert!((b2.as_f64().unwrap() - 7.0).abs() < 1e-6, "{b2:?}");
    assert_eq!(selection(&h), (0, "B5".into()), "the selection from before picking comes back");
}

#[test]
fn a_range_on_another_sheet_carries_the_sheet_name_and_cancel_returns_home() {
    let mut s = workbook();
    s.execute("home.insertSheet", json!({"name": "Rates Table", "before": 1})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    let mut h = harness(s);
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // The sheet tabs' "Next sheet" arrow, below the grid at the left.
    let grid = h.state().grid.rect.unwrap();
    click_at(&mut h, egui::pos2(grid.left() + 44.0, grid.bottom() + 16.0));
    assert_eq!(selection(&h).0, 1, "the next sheet is active while picking");
    drag(&mut h, "A2", "A4");
    assert_eq!(field(&h, "changing"), "'Rates Table'!$A$2:$A$4");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(collapsed(&h), None);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none(), "Escape in the expanded dialog cancels it");
    assert_eq!(selection(&h), (0, "B5".into()), "cancelling returns to the sheet and selection");
}

#[test]
fn switching_sheets_while_picking_keeps_a_typed_reference() {
    let mut s = workbook();
    s.execute("home.insertSheet", json!({"name": "Rates Table", "before": 1})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    let mut h = harness(s);
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "B2");
    assert_eq!(field(&h, "changing"), "B2");
    // The sheet tabs' "Next sheet" and "Previous sheet" arrows, below the grid at the left.
    let grid = h.state().grid.rect.unwrap();
    click_at(&mut h, egui::pos2(grid.left() + 44.0, grid.bottom() + 16.0));
    assert_eq!(selection(&h).0, 1, "the next sheet is active while picking");
    assert_eq!(field(&h, "changing"), "B2", "changing sheets alone is not a pick");
    click_at(&mut h, egui::pos2(grid.left() + 20.0, grid.bottom() + 16.0));
    assert_eq!(selection(&h).0, 0, "back on the dialog's own sheet");
    assert_eq!(field(&h, "changing"), "B2", "coming back is not a pick either");
    click_at(&mut h, egui::pos2(grid.left() + 44.0, grid.bottom() + 16.0));
    drag(&mut h, "A2", "A4");
    assert_eq!(field(&h, "changing"), "'Rates Table'!$A$2:$A$4", "a selection on the other sheet still is");
}

#[test]
fn clicking_into_a_box_picks_into_it_and_another_field_stops_picking() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    let boxes = dialog_boxes(&h);
    assert_eq!(boxes.len(), 3);
    click_at(&mut h, boxes[2].center());
    let b2 = cell_pos(&h, "B2");
    click_at(&mut h, b2);
    assert_eq!(field(&h, "changing"), "$B$2", "a click on the grid fills the box last clicked into");
    click_at(&mut h, boxes[1].center());
    let d9 = cell_pos(&h, "D9");
    click_at(&mut h, d9);
    assert_eq!(field(&h, "changing"), "$B$2", "To value is not a reference box, so picking stopped");
    assert_eq!(field(&h, "set"), "B5");
}

#[test]
fn the_collapse_button_is_named_and_works_from_the_keyboard() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    h.get_all_by_label("Collapse dialog").nth(1).unwrap().focus();
    h.run_steps(2);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert_eq!(collapsed(&h).as_deref(), Some("changing"));
    h.get_by_label("Expand dialog").focus();
    h.run_steps(2);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert_eq!(collapsed(&h), None);
}

#[test]
fn data_analysis_picks_its_input_and_output_ranges() {
    let mut h = harness(workbook());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    let buttons = h.get_all_by_label("Collapse dialog").count();
    assert_eq!(buttons, 2, "Input Range and Output Range are reference boxes");
    click_button(&mut h, "Collapse dialog", 0);
    assert_eq!(collapsed(&h).as_deref(), Some("range"));
    drag(&mut h, "B1", "B6");
    assert_eq!(field(&h, "range"), "$B$1:$B$6");
    key(&mut h, Key::Escape, Modifiers::NONE);
    click_button(&mut h, "Collapse dialog", 1);
    assert_eq!(collapsed(&h).as_deref(), Some("output"));
    drag(&mut h, "D2", "D2");
    assert_eq!(field(&h, "output"), "$D$2");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(field(&h, "outputTo"), "range", "picking an output range chooses it");
}

#[test]
fn choosing_new_worksheet_ply_after_picking_an_output_range_sticks() {
    let mut h = harness(workbook());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 1);
    drag(&mut h, "D2", "D2");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(field(&h, "outputTo"), "range");

    let ply = h.get_by_label("New Worksheet Ply:").rect();
    click_at(&mut h, ply.center());
    h.run_steps(3);
    assert_eq!(field(&h, "outputTo"), "newSheet", "the radio choice is not overridden by the Output Range box");
    let labels = h.get_by_label("Labels in first row").rect();
    click_at(&mut h, labels.center());
    assert_eq!(field(&h, "outputTo"), "newSheet", "a checkbox click keeps the choice");

    // Picking into Output Range ended with the radio choice: a later selection leaves it alone.
    drag(&mut h, "F4", "F5");
    assert_eq!(field(&h, "output"), "$D$2");
    assert_eq!(field(&h, "outputTo"), "newSheet");

    // Typing a range in the box chooses Output Range again.
    let boxes: Vec<egui::Rect> = h
        .get_all_by_role(egui::accesskit::Role::TextInput)
        .filter(|n| n.rect().center().y > ply.top() - 40.0 && n.rect().center().y < ply.top())
        .map(|n| n.rect())
        .collect();
    assert_eq!(boxes.len(), 1, "the Output Range box, on the row above New Worksheet Ply");
    click_at(&mut h, boxes[0].center());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "H1");
    assert_eq!(field(&h, "output"), "H1");
    assert_eq!(field(&h, "outputTo"), "range");
}

#[test]
fn solver_picks_its_objective_variables_and_constraint_cells() {
    let mut h = harness(workbook());
    h.state_mut().open_dialog("solver", json!({}));
    h.run_steps(4);
    assert_eq!(h.get_all_by_label("Collapse dialog").count(), 4, "objective, variables and both sides of a constraint");
    click_button(&mut h, "Collapse dialog", 1);
    assert_eq!(collapsed(&h).as_deref(), Some("variables"));
    drag(&mut h, "B2", "B3");
    assert_eq!(field(&h, "variables"), "$B$2:$B$3");
    key(&mut h, Key::Enter, Modifiers::NONE);
    click_button(&mut h, "Collapse dialog", 2);
    drag(&mut h, "D4", "D4");
    assert_eq!(field(&h, "newLhs"), "$D$4");
    assert_eq!(field(&h, "objective"), "B5");
}

#[test]
fn a_sheet_inserted_while_picking_keeps_references_and_the_way_home() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // A sheet inserted before the dialog's own sheet (from the ribbon, which stays usable) moves
    // that sheet to index 1 and becomes the active sheet at index 0.
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Inputs", "before": 0})).unwrap();
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    drag(&mut h, "A2", "A3");
    assert_eq!(field(&h, "changing"), "Inputs!$A$2:$A$3", "a range on the new sheet carries its name");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert_eq!(selection(&h), (1, "B5".into()), "cancelling returns to the dialog's own sheet, now at index 1");
}

#[test]
fn a_box_for_one_reference_takes_only_the_area_being_selected() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // Several areas, as Ctrl+click adds them: By changing cell takes one reference.
    h.state_mut().session.execute("selection.set", json!({"range": "A1,C3:C4"})).unwrap();
    h.run_steps(3);
    assert_eq!(field(&h, "changing"), "$C$3:$C$4", "only the area being selected, not a union");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);

    // Solver's variable cells take several areas, as in Excel.
    h.state_mut().open_dialog("solver", json!({}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 1);
    assert_eq!(collapsed(&h).as_deref(), Some("variables"));
    h.state_mut().session.execute("selection.set", json!({"range": "A1,C3:C4"})).unwrap();
    h.run_steps(3);
    assert_eq!(field(&h, "variables"), "$A$1,$C$3:$C$4");
    key(&mut h, Key::Escape, Modifiers::NONE);
    click_button(&mut h, "Collapse dialog", 0);
    assert_eq!(collapsed(&h).as_deref(), Some("objective"));
    h.state_mut().session.execute("selection.set", json!({"range": "B5,D7"})).unwrap();
    h.run_steps(3);
    assert_eq!(field(&h, "objective"), "$D$7", "the objective is one cell");
}

#[test]
fn renaming_the_dialogs_sheet_while_picking_keeps_the_way_home() {
    let mut s = workbook();
    s.execute("home.insertSheet", json!({"name": "Rates", "before": 1})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    let mut h = harness(s);
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // The dialog's own sheet is renamed from the ribbon while the box picks.
    h.state_mut().session.execute("sheet.rename", json!({"sheet": 0, "name": "Model"})).unwrap();
    h.run_steps(3);
    drag(&mut h, "C3", "C3");
    assert_eq!(field(&h, "changing"), "$C$3", "the renamed sheet is still the dialog's own: no sheet name");
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(3);
    drag(&mut h, "A2", "A2");
    assert_eq!(field(&h, "changing"), "Rates!$A$2");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert_eq!(selection(&h), (0, "B5".into()), "cancelling returns to the renamed sheet and its selection");
}

/// A workbook whose first sheet (the dialog's own, `Sheet1`) is followed by `Rates`.
fn two_sheets() -> Session {
    let mut s = workbook();
    s.execute("home.insertSheet", json!({"name": "Rates", "before": 1})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    s
}

fn sheet_names(h: &H) -> Vec<String> {
    h.state().session.active().unwrap().wb.sheets.iter().map(|s| s.name.clone()).collect()
}

#[test]
fn a_deleted_sheet_is_not_taken_for_one_inserted_in_its_place() {
    let mut h = harness(two_sheets());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // The dialog's own sheet is deleted, and a new sheet then takes its place and its count.
    h.state_mut().session.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Fresh", "before": 0})).unwrap();
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    assert_eq!(sheet_names(&h), ["Fresh", "Rates"]);
    drag(&mut h, "C3", "C3");
    assert_eq!(field(&h, "changing"), "Fresh!$C$3", "the new sheet is not the dialog's own: picks carry its name");

    // OK is refused rather than running unqualified references against another sheet.
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some(), "the dialog stays open");
    let message = h.state().message.as_ref().map(|m| m.1.clone()).unwrap_or_default();
    assert!(message.contains("was deleted"), "{message}");
    let b2 = h.state().session.active().unwrap().wb.sheets[1].value(CellRef::parse("B2").unwrap());
    assert_eq!(b2.as_f64(), None, "Goal Seek changed nothing: {b2:?}");

    // Cancel doesn't put the old selection onto the new sheet.
    h.state_mut().message = None;
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert_eq!(selection(&h), (0, "C3".into()), "the selection on the new sheet is left alone");
}

#[test]
fn a_deleted_sheet_qualifies_picks_and_cancel_stays_put() {
    let mut h = harness(two_sheets());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    h.state_mut().session.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    assert_eq!(sheet_names(&h), ["Rates"]);
    drag(&mut h, "A2", "A3");
    assert_eq!(field(&h, "changing"), "Rates!$A$2:$A$3");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert_eq!(selection(&h), (0, "A2:A3".into()), "nothing to go back to: the selection stays");
}

#[test]
fn data_analysis_refuses_ok_once_its_sheet_is_deleted() {
    let mut h = harness(two_sheets());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    h.state_mut().session.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some(), "the dialog stays open");
    let message = h.state().message.as_ref().map(|m| m.1.clone()).unwrap_or_default();
    assert!(message.contains("was deleted"), "{message}");
    assert_eq!(sheet_names(&h), ["Rates"], "no report sheet was added");
}

#[test]
fn ok_stays_refused_after_a_failed_ok_once_the_sheet_is_deleted() {
    let mut h = harness(two_sheets());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    drag(&mut h, "B1", "B6");
    assert_eq!(field(&h, "range"), "$B$1:$B$6");
    key(&mut h, Key::Escape, Modifiers::NONE);
    // An output range the command refuses: OK fails and the dialog stays open.
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.values.insert("outputTo".into(), json!("range"));
    d.values.insert("output".into(), json!("not a reference"));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some(), "the failed OK leaves the dialog open");
    assert!(h.state().message.is_some(), "and says why");
    h.state_mut().message = None;

    // The dialog's sheet is deleted; with a usable output range, OK must still be refused rather
    // than run `$B$1:$B$6` against Rates.
    h.state_mut().session.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    h.state_mut().dialog.as_mut().unwrap().values.insert("output".into(), json!("Rates!$H$1"));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some(), "the dialog stays open");
    let message = h.state().message.as_ref().map(|m| m.1.clone()).unwrap_or_default();
    assert!(message.contains("was deleted"), "{message}");
    let h1 = h.state().session.active().unwrap().wb.sheets[0].value(CellRef::parse("H1").unwrap());
    assert!(h1.is_empty(), "no report was written on Rates: {h1:?}");
}

#[test]
fn a_second_ok_after_a_failed_one_runs_on_the_dialogs_sheet() {
    let mut h = harness(two_sheets());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    drag(&mut h, "B1", "B6");
    key(&mut h, Key::Escape, Modifiers::NONE);
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.values.insert("outputTo".into(), json!("range"));
    d.values.insert("output".into(), json!("not a reference"));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some());
    h.state_mut().message = None;
    // The person looks at another sheet, then fixes the output range and presses OK again.
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(3);
    h.state_mut().dialog.as_mut().unwrap().values.insert("output".into(), json!("$H$1"));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().message);
    let wb = &h.state().session.active().unwrap().wb;
    assert!(!wb.sheets[0].value(CellRef::parse("H1").unwrap()).is_empty(), "the report is on the dialog's own sheet");
    assert!(wb.sheets[1].value(CellRef::parse("H1").unwrap()).is_empty(), "not on the sheet shown");
}

#[test]
fn solver_model_range_is_a_reference_box_until_its_area_is_hidden() {
    let mut h = harness(workbook());
    h.state_mut().open_dialog("solver", json!({}));
    h.run_steps(4);
    h.get_by_label("Load/Save").click();
    h.run_steps(3);
    assert_eq!(h.get_all_by_label("Collapse dialog").count(), 5, "Model Range is a reference box too");
    click_button(&mut h, "Collapse dialog", 4);
    assert_eq!(collapsed(&h).as_deref(), Some("modelRange"));
    drag(&mut h, "H1", "H3");
    assert_eq!(field(&h, "modelRange"), "$H$1:$H$3");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(collapsed(&h), None);
    assert_eq!(field(&h, "modelRange"), "$H$1:$H$3", "expanding keeps the picked range");

    // Hiding the Load/Save area ends picking into its box: a selection on the grid no longer
    // changes a range that isn't shown.
    h.get_by_label("Load/Save").click();
    h.run_steps(3);
    assert!(h.query_by_label("Model Range:").is_none(), "the area is hidden");
    drag(&mut h, "C3", "C3");
    assert_eq!(field(&h, "modelRange"), "$H$1:$H$3");
}

#[test]
fn a_click_elsewhere_keeps_the_collapsed_box_picking() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // The Name Box takes the keyboard: the collapsed box still picks (expanded, another text
    // field ends picking, since the person moved on to it).
    let name_box = h.get_by_label("Name Box").rect();
    click_at(&mut h, name_box.center());
    assert_eq!(collapsed(&h).as_deref(), Some("changing"), "a click elsewhere doesn't expand the dialog");
    drag(&mut h, "C3", "D6");
    assert_eq!(field(&h, "changing"), "$C$3:$D$6", "the box still takes the selection");
    // The title bar and ribbon stay usable: Undo takes back the last edit (B5's formula) and
    // puts the selection back where it was before that edit (A1, where `workbook()` entered it),
    // which the box follows like any other selection; the dialog stays collapsed.
    let undo = h.get_by_label("Undo").rect();
    click_at(&mut h, undo.center());
    assert_eq!(collapsed(&h).as_deref(), Some("changing"));
    assert!(h.state().dialog.is_some());
    assert_eq!(selection(&h), (0, "A1".into()));
    assert_eq!(field(&h, "changing"), "$A$1");
    drag(&mut h, "D2", "D2");
    assert_eq!(field(&h, "changing"), "$D$2");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(collapsed(&h), None, "Escape expands it");
    assert!(h.state().dialog.is_some());
}

#[test]
fn another_dialog_opened_while_picking_puts_the_selection_back() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    drag(&mut h, "C3", "D6");
    assert_eq!(field(&h, "changing"), "$C$3:$D$6");
    assert_eq!(selection(&h), (0, "C3:D6".into()));
    // A ribbon command opens another dialog while the box picks: the dialog it replaces is gone,
    // and the sheet and selection from before picking come back, as they do when it is cancelled.
    h.state_mut().open_dialog("goTo", json!({}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("goTo"));
    assert_eq!(selection(&h), (0, "B5".into()));
}

#[test]
fn arrow_keys_with_shift_select_a_range_into_the_collapsed_box() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // Shift+arrow first: a plain-key match would take it as a move and lose the extension.
    key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(selection(&h), (0, "B5:B6".into()));
    assert_eq!(field(&h, "changing"), "$B$5:$B$6");
    key(&mut h, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(field(&h, "changing"), "$B$5:$C$6");
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(field(&h, "changing"), "$B$4", "an arrow without Shift moves the active cell");
    key(&mut h, Key::ArrowLeft, Modifiers::NONE);
    assert_eq!(field(&h, "changing"), "$A$4");
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    // Ctrl+arrow (Cmd on a Mac) jumps to the edge of the data: up from B4 (empty) to B2 (1).
    key(&mut h, Key::ArrowUp, Modifiers::COMMAND);
    assert_eq!(field(&h, "changing"), "$B$2");
    assert_eq!(collapsed(&h).as_deref(), Some("changing"), "the dialog stays collapsed");
    // The box keeps the keyboard: typing still replaces the reference.
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "B3");
    assert_eq!(field(&h, "changing"), "B3");
    // Expanded, the arrows edit the box (they move its caret), not the selection.
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(collapsed(&h), None);
    let changing = dialog_boxes(&h)[2];
    click_at(&mut h, changing.center());
    let before = selection(&h);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowLeft, Modifiers::SHIFT);
    assert_eq!(selection(&h), before);
    assert_eq!(field(&h, "changing"), "B3");
}

#[test]
fn f4_cycles_the_reference_between_absolute_and_relative() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    drag(&mut h, "B2", "B2");
    assert_eq!(field(&h, "changing"), "$B$2");
    for expected in ["B$2", "$B2", "B2", "$B$2"] {
        key(&mut h, Key::F4, Modifiers::NONE);
        assert_eq!(field(&h, "changing"), expected);
    }
    assert_eq!(collapsed(&h).as_deref(), Some("changing"), "F4 doesn't expand the dialog");
    // Expanded, F4 still works in the focused box, on a typed range with a sheet name.
    key(&mut h, Key::Escape, Modifiers::NONE);
    let changing = dialog_boxes(&h)[2];
    click_at(&mut h, changing.center());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "'Rates Table'!A2:A4");
    key(&mut h, Key::F4, Modifiers::NONE);
    assert_eq!(field(&h, "changing"), "'Rates Table'!$A$2:$A$4");
    // A field that is not a reference box leaves F4 alone.
    let to = dialog_boxes(&h)[1];
    click_at(&mut h, to.center());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "25");
    key(&mut h, Key::F4, Modifiers::NONE);
    assert_eq!(field(&h, "to"), "25");
    assert_eq!(field(&h, "changing"), "'Rates Table'!$A$2:$A$4");
}

/// `workbook()` with a second sheet whose name needs quoting (a space and an apostrophe), holding
/// 1 in A2 and 1..6 in A1:A6, and a formula on the first sheet that reads its A2.
fn quoted_sheet() -> Session {
    let mut s = workbook();
    s.execute("home.insertSheet", json!({"name": "O'Brien Rates", "before": 1})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    for (i, v) in (1..=6).enumerate() {
        s.execute("cell.set", json!({"cell": format!("A{}", i + 1), "input": v.to_string()})).unwrap();
    }
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    // 3 * A2 + 4 with A2 = 2 is 10; Goal Seek to 25 wants A2 = 7.
    s.execute("cell.set", json!({"cell": "B5", "input": "='O''Brien Rates'!A2*3+4"})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    s
}

/// Clicks the sheet tabs' "Next sheet" arrow, below the grid at the left.
fn next_sheet(h: &mut H) {
    let grid = h.state().grid.rect.unwrap();
    click_at(h, egui::pos2(grid.left() + 44.0, grid.bottom() + 16.0));
}

#[test]
fn ok_after_a_cross_sheet_pick_runs_goal_seek_with_the_quoted_sheet_name() {
    let mut h = harness(quoted_sheet());
    open_goal_seek(&mut h);
    assert_eq!(field(&h, "set"), "B5");
    click_button(&mut h, "Collapse dialog", 1);
    next_sheet(&mut h);
    assert_eq!(selection(&h).0, 1);
    drag(&mut h, "A2", "A2");
    assert_eq!(field(&h, "changing"), "'O''Brien Rates'!$A$2", "quoted as the command's parser reads it");
    key(&mut h, Key::Escape, Modifiers::NONE);
    let to = dialog_boxes(&h)[1];
    click_at(&mut h, to.center());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "25");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().message);
    let wb = &h.state().session.active().unwrap().wb;
    let a2 = wb.sheets[1].value(CellRef::parse("A2").unwrap());
    assert!((a2.as_f64().unwrap() - 7.0).abs() < 1e-6, "the changing cell on the other sheet: {a2:?}");
    let b5 = wb.sheets[0].value(CellRef::parse("B5").unwrap());
    assert!((b5.as_f64().unwrap() - 25.0).abs() < 1e-6, "{b5:?}");
    assert_eq!(selection(&h), (0, "B5".into()), "back on the dialog's sheet and selection");
}

#[test]
fn data_analysis_runs_on_a_range_picked_on_another_sheet() {
    let mut h = harness(quoted_sheet());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    next_sheet(&mut h);
    drag(&mut h, "A1", "A6");
    assert_eq!(field(&h, "range"), "'O''Brien Rates'!$A$1:$A$6");
    key(&mut h, Key::Escape, Modifiers::NONE);
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.values.insert("outputTo".into(), json!("range"));
    d.values.insert("output".into(), json!("$H$1"));
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().message);
    // The report is on the dialog's own sheet and read the other sheet's numbers: the mean of
    // 1..6 is 21 / 6 = 3.5.
    let wb = &h.state().session.active().unwrap().wb;
    let report = &wb.sheets[0];
    let mean = (0..20).find_map(|row| {
        let label = report.value(CellRef::new(row, 7));
        (label.as_text().map(str::trim) == Some("Mean")).then(|| report.value(CellRef::new(row, 8)))
    });
    assert_eq!(mean.and_then(|v| v.as_f64()), Some(3.5), "{:?}", (0..20).map(|r| report.value(CellRef::new(r, 7))).collect::<Vec<_>>());
    assert!(wb.sheets[1].value(CellRef::parse("H1").unwrap()).is_empty(), "nothing was written on the picked sheet");
}

#[test]
fn enter_while_collapsed_only_expands_the_dialog() {
    let mut h = harness(workbook());
    open_goal_seek(&mut h);
    let to = dialog_boxes(&h)[1];
    click_at(&mut h, to.center());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "25");
    click_button(&mut h, "Collapse dialog", 1);
    drag(&mut h, "B2", "B2");
    assert_eq!(field(&h, "changing"), "$B$2");
    assert!(h.query_by_label("   OK   ").is_none(), "the collapsed dialog shows no OK button");
    let b2 = |h: &H| h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse("B2").unwrap()).as_f64();
    let undo_steps = |h: &H| h.state().session.active().unwrap().undo.len();
    let steps = undo_steps(&h);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(collapsed(&h), None, "Enter expands the dialog");
    assert!(h.state().dialog.is_some(), "and doesn't close it");
    assert_eq!(b2(&h), Some(1.0), "Goal Seek did not run");
    assert_eq!(undo_steps(&h), steps, "no undo step was added");
    // OK is what runs it.
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    assert!((b2(&h).unwrap() - 7.0).abs() < 1e-6);
    assert_eq!(undo_steps(&h), steps + 1);
}

#[test]
fn a_successful_analysis_to_a_new_sheet_leaves_that_sheet_active() {
    let mut h = harness(workbook());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    drag(&mut h, "B1", "B6");
    assert_eq!(field(&h, "range"), "$B$1:$B$6");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(field(&h, "outputTo"), "newSheet", "New Worksheet Ply is the default output");
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "{:?}", h.state().message);
    // The command added the report sheet and showed it; closing the dialog must not go back to
    // the sheet it was opened on, as Excel shows the new sheet.
    assert_eq!(sheet_names(&h).len(), 2);
    assert_eq!(selection(&h).0, 1, "the report sheet stays active");
}

#[test]
fn a_scripted_cancel_or_ok_goes_back_to_the_dialogs_sheet_like_the_buttons() {
    // Cancel from the control channel puts the sheet and selection back.
    let mut h = harness(two_sheets());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    next_sheet(&mut h);
    drag(&mut h, "A2", "A2");
    assert_eq!(field(&h, "changing"), "Rates!$A$2");
    assert_eq!(selection(&h), (1, "A2".into()));
    let r = control(&mut h, "ui.dialog.cancel", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(h.state().dialog.is_none());
    assert_eq!(selection(&h), (0, "B5".into()), "a scripted cancel returns home as the button does");

    // OK from the control channel runs the command on the dialog's sheet: `$H$1` means the sheet
    // the dialog was opened on, not the sheet the box picked on.
    let mut h = harness(quoted_sheet());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    next_sheet(&mut h);
    drag(&mut h, "A1", "A6");
    assert_eq!(field(&h, "range"), "'O''Brien Rates'!$A$1:$A$6");
    control(&mut h, "ui.dialog.set", json!({"field": "outputTo", "value": "range"}));
    control(&mut h, "ui.dialog.set", json!({"field": "output", "value": "$H$1"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(h.state().dialog.is_none());
    let wb = &h.state().session.active().unwrap().wb;
    assert!(!wb.sheets[0].value(CellRef::parse("H1").unwrap()).is_empty(), "the report is on the dialog's own sheet");
    assert!(wb.sheets[1].value(CellRef::parse("H1").unwrap()).is_empty(), "not on the sheet shown while picking");
    assert_eq!(selection(&h).0, 0);

    // A deleted dialog sheet refuses a scripted OK as it refuses the button (Goal Seek presses
    // its own OK, so `ui.dialog.confirm` refuses it by design; Data Analysis has a command).
    let mut h = harness(two_sheets());
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    h.state_mut().session.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(r["error"].as_str().unwrap_or("").contains("was deleted"), "{r}");
    assert!(h.state().dialog.is_some(), "the dialog stays open");
}

#[test]
fn a_text_box_draft_is_saved_before_a_scripted_cancel_leaves_the_picked_sheet() {
    let mut s = two_sheets();
    s.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    let id =
        s.execute("insert.textBox", json!({"at": "B2", "width": 240, "height": 120, "text": "Rate note"})).unwrap()["shape"].as_u64().unwrap() as u32;
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    let mut h = harness(s);
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    next_sheet(&mut h);
    // The person starts editing the text box on the picked sheet (what its double-click does)
    // and types a new note.
    assert!(h.state_mut().begin_text_box_edit(id), "the text box is being edited");
    h.run_steps(3);
    assert!(h.state().text_box_editor.is_some());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "Picked note");
    assert_eq!(h.state().text_box_editor.as_ref().map(|ed| ed.text.as_str()), Some("Picked note"));
    let shape_text =
        |h: &H, sheet: usize| h.state().session.active().unwrap().wb.sheets[sheet].shapes.iter().find(|s| s.id == id).unwrap().text.clone();
    assert_eq!(shape_text(&h, 1), "Rate note", "not saved yet");
    // A scripted cancel leaves the sheet: the draft is saved first, as the ribbon's sheet switch
    // saves it, instead of being stranded on a sheet that is no longer shown.
    let r = control(&mut h, "ui.dialog.cancel", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(h.state().text_box_editor.is_none(), "the draft was committed: {:?}", h.state().text_box_editor);
    assert_eq!(shape_text(&h, 1), "Picked note");
    assert_eq!(selection(&h), (0, "B5".into()));
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
}

/// `quoted_sheet()` with a text box on its second sheet and a third sheet, Scratch, so that a
/// draft of the text box can be left where it cannot be saved (see [`strand_a_draft`]).
fn draft_workbook() -> (Session, u32) {
    let mut s = quoted_sheet();
    s.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    let id =
        s.execute("insert.textBox", json!({"at": "D2", "width": 200, "height": 100, "text": "Rate note"})).unwrap()["shape"].as_u64().unwrap() as u32;
    s.execute("home.insertSheet", json!({"name": "Scratch", "before": 2})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    (s, id)
}

/// With the second sheet shown: starts editing its text box `id`, types a note, then has a
/// script show Scratch straight through the engine. The draft's worksheet is no longer the
/// active one, so it can't be saved, and showing the dialog's sheet again (which saves drafts
/// first) is refused.
fn strand_a_draft(h: &mut H, id: u32) {
    assert!(h.state_mut().begin_text_box_edit(id));
    h.run_steps(3);
    key(h, Key::A, Modifiers::COMMAND);
    type_text(h, "Picked note");
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 2})).unwrap();
    h.run_steps(3);
    assert_eq!(selection(h).0, 2);
}

fn draft_text(h: &H) -> Option<String> {
    h.state().text_box_editor.as_ref().map(|ed| ed.text.clone())
}

#[test]
fn a_draft_that_cannot_be_saved_stops_a_confirm_after_a_cross_sheet_pick() {
    let (s, id) = draft_workbook();
    let mut h = harness(s);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    click_button(&mut h, "Collapse dialog", 0);
    next_sheet(&mut h);
    drag(&mut h, "A1", "A6");
    assert_eq!(field(&h, "range"), "'O''Brien Rates'!$A$1:$A$6");
    key(&mut h, Key::Escape, Modifiers::NONE);
    control(&mut h, "ui.dialog.set", json!({"field": "outputTo", "value": "range"}));
    control(&mut h, "ui.dialog.set", json!({"field": "output", "value": "$H$1"}));
    strand_a_draft(&mut h, id);
    let h1 = |h: &H, sheet: usize| h.state().session.active().unwrap().wb.sheets[sheet].value(CellRef::parse("H1").unwrap());

    // The control channel's confirm: the dialog's sheet can't be shown again, so the command
    // doesn't run anywhere (`$H$1` would have meant the sheet shown, Scratch).
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(h.state().dialog.is_some(), "the dialog stays open");
    assert!(h.state().text_box_editor.is_some(), "the draft is kept");
    for sheet in 0..3 {
        assert!(h1(&h, sheet).is_empty(), "no report on sheet {sheet}: {:?}", h1(&h, sheet));
    }
    assert_eq!(selection(&h).0, 2, "still on the sheet shown");

    // The window's OK: the same, with the reason shown.
    h.state_mut().message = None;
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some(), "the dialog stays open");
    assert!(h.state().message.is_some(), "the reason is shown");
    for sheet in 0..3 {
        assert!(h1(&h, sheet).is_empty(), "no report on sheet {sheet}: {:?}", h1(&h, sheet));
    }
    assert_eq!(h.state().text_box_editor.as_ref().map(|ed| ed.text.as_str()), Some("Picked note"), "the person's text is kept");
}

#[test]
fn a_scripted_cancel_with_an_unsavable_draft_keeps_the_dialog_until_it_can_go_home() {
    let (s, id) = draft_workbook();
    let mut h = harness(s);
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    next_sheet(&mut h);
    drag(&mut h, "A2", "A2");
    assert_eq!(field(&h, "changing"), "'O''Brien Rates'!$A$2");
    key(&mut h, Key::Escape, Modifiers::NONE);
    strand_a_draft(&mut h, id);
    // The dialog's sheet can't be shown again: the script gets the reason, the dialog stays.
    let r = control(&mut h, "ui.dialog.cancel", json!({}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(h.state().dialog.is_some(), "the dialog stays open");
    assert_eq!(draft_text(&h).as_deref(), Some("Picked note"), "the draft is kept");
    assert_eq!(selection(&h).0, 2);
    // Back on the draft's sheet, cancelling saves the draft and goes home.
    h.state_mut().session.execute("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(3);
    let r = control(&mut h, "ui.dialog.cancel", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(h.state().dialog.is_none());
    assert_eq!(draft_text(&h), None, "the draft was saved");
    let text = h.state().session.active().unwrap().wb.sheets[1].shapes.iter().find(|s| s.id == id).unwrap().text.clone();
    assert_eq!(text, "Picked note");
    assert_eq!(selection(&h), (0, "B5".into()));
}

#[test]
fn the_windows_cancel_with_an_unsavable_draft_closes_the_dialog_and_says_why() {
    let (s, id) = draft_workbook();
    let mut h = harness(s);
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    h.run_steps(4);
    let title = h.state().dialog.as_ref().unwrap().title.clone();
    click_button(&mut h, "Collapse dialog", 0);
    next_sheet(&mut h);
    drag(&mut h, "A1", "A6");
    key(&mut h, Key::Escape, Modifiers::NONE);
    strand_a_draft(&mut h, id);
    h.get_by_label(" Cancel ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "Cancel closes the dialog whatever the draft's state");
    let message = h.state().message.clone().unwrap_or_default();
    assert_eq!(message.0, title, "the dialog says why it could not go home: {message:?}");
    assert_eq!(draft_text(&h).as_deref(), Some("Picked note"), "the draft is kept");
    assert_eq!(selection(&h).0, 2, "the selection stays where it is");
    let wb = &h.state().session.active().unwrap().wb;
    for sheet in 0..3 {
        assert!(wb.sheets[sheet].value(CellRef::parse("H1").unwrap()).is_empty(), "nothing ran on sheet {sheet}");
    }
}

#[test]
fn goal_seeks_cancel_with_an_unsavable_draft_closes_without_running() {
    let (s, id) = draft_workbook();
    let mut h = harness(s);
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    next_sheet(&mut h);
    drag(&mut h, "A2", "A2");
    key(&mut h, Key::Escape, Modifiers::NONE);
    // A To value OK would use, so a run by mistake would show on the other sheet's A2.
    let to = dialog_boxes(&h)[1];
    click_at(&mut h, to.center());
    key(&mut h, Key::A, Modifiers::COMMAND);
    type_text(&mut h, "25");
    strand_a_draft(&mut h, id);
    h.get_by_label(" Cancel ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none(), "Cancel is not a confirmation: the dialog closes");
    let a2 = h.state().session.active().unwrap().wb.sheets[1].value(CellRef::parse("A2").unwrap());
    assert_eq!(a2.as_f64(), Some(2.0), "Goal Seek did not run: {a2:?}");
    assert_eq!(draft_text(&h).as_deref(), Some("Picked note"), "the draft is kept");
    let message = h.state().message.clone().unwrap_or_default();
    assert_eq!(message.0, "Goal Seek", "the reason is shown: {message:?}");
    assert_eq!(selection(&h).0, 2);
}

#[test]
fn a_sheet_deleted_and_another_inserted_in_one_batch_is_not_taken_for_a_rename() {
    let mut h = harness(two_sheets());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    // One script batch, between two frames: the dialog's sheet goes and another takes its place,
    // which leaves the name list the same length with one entry changed, as a rename would.
    let s = &mut h.state_mut().session;
    s.execute("home.deleteSheet", json!({"sheet": 0})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Fresh", "before": 0})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    h.run_steps(3);
    assert_eq!(sheet_names(&h), ["Fresh", "Rates"]);
    drag(&mut h, "C3", "C3");
    assert_eq!(field(&h, "changing"), "Fresh!$C$3", "the new sheet is not the dialog's own: picks carry its name");
    key(&mut h, Key::Escape, Modifiers::NONE);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_some(), "OK is refused");
    let message = h.state().message.as_ref().map(|m| m.1.clone()).unwrap_or_default();
    assert!(message.contains("was deleted"), "{message}");
    h.state_mut().message = None;
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert_eq!(selection(&h), (0, "C3".into()), "cancelling does not put the old selection onto the new sheet");
}

#[test]
fn undoing_and_redoing_the_dialogs_sheets_rename_still_follow_it() {
    let mut h = harness(two_sheets());
    open_goal_seek(&mut h);
    click_button(&mut h, "Collapse dialog", 1);
    h.state_mut().session.execute("sheet.rename", json!({"sheet": 0, "name": "Model"})).unwrap();
    h.run_steps(3);
    drag(&mut h, "C3", "C3");
    assert_eq!(field(&h, "changing"), "$C$3", "renamed, still the dialog's own sheet");
    // Undo brings the old name back; redo the new one: both are recorded steps that rename.
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(sheet_names(&h)[0], "Sheet1");
    drag(&mut h, "C4", "C4");
    assert_eq!(field(&h, "changing"), "$C$4");
    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(sheet_names(&h)[0], "Model");
    drag(&mut h, "C5", "C5");
    assert_eq!(field(&h, "changing"), "$C$5");
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert_eq!(selection(&h), (0, "B5".into()), "cancelling returns to the renamed sheet and its selection");
}
