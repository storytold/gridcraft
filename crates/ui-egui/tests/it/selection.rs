//! Cell selection consumes every pointer position without editing cell contents.

use egui::{Event, Modifiers, PointerButton, Pos2};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, RangeRef};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::{Value, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(initial: &str) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["sentinel", 2], [3, "untouched"]]})).unwrap();
    session.execute("cell.set", json!({"cell": "C3", "input": "123"})).unwrap();
    session.execute("cell.set", json!({"cell": "F7", "input": "=C3*2"})).unwrap();
    session.execute("selection.set", json!({"cell": initial})).unwrap();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn point(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
}

fn cells(h: &Harness) -> Value {
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    json!(sheet.cells.iter().map(|(pos, cell)| (pos.a1(), cell)).collect::<Vec<_>>())
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.run_steps(1);
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn press(h: &mut Harness, cell: &str) {
    let from = point(h, cell);
    frame(h, Event::PointerMoved(from));
    frame(h, button(from, true));
}

fn click_with_modifiers(h: &mut Harness, cell: &str, modifiers: Modifiers) {
    let pos = point(h, cell);
    frame(h, Event::ModifiersChanged(modifiers));
    frame(h, Event::PointerMoved(pos));
    for pressed in [true, false] {
        frame(h, Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
    }
    h.run_steps(2);
}

fn assert_selection(h: &Harness, start: &str, end: &str) {
    let selection = &h.state().session.active().unwrap().selection;
    let start = CellRef::parse(start).unwrap();
    assert_eq!(selection.current(), RangeRef::new(start, CellRef::parse(end).unwrap()));
    assert_eq!(selection.anchor, start);
    assert_eq!(selection.active, start);
}

#[test]
fn single_movement_selects_from_the_pressed_cell_in_both_directions() {
    for (initial, start, end) in [("A1", "C3", "F7"), ("C3", "C3", "F7"), ("A1", "F7", "C3"), ("F7", "F7", "C3")] {
        let mut h = harness(initial);
        let before = cells(&h);
        press(&mut h, start);
        let to = point(&h, end);
        frame(&mut h, Event::PointerMoved(to));
        assert_selection(&h, start, end);
        frame(&mut h, button(to, false));
        h.run_steps(1);
        assert_selection(&h, start, end);
        assert_eq!(cells(&h), before);
    }
}

#[test]
fn release_consumes_a_new_endpoint_without_an_intermediate_movement_frame() {
    let mut h = harness("A1");
    let before = cells(&h);
    press(&mut h, "C3");
    let mid = point(&h, "D4");
    frame(&mut h, Event::PointerMoved(mid));
    let to = point(&h, "F7");
    frame(&mut h, button(to, false));
    assert_selection(&h, "C3", "F7");
    assert_eq!(cells(&h), before);
}

#[test]
fn multiple_movement_frames_keep_the_anchor_and_cell_contents() {
    let mut h = harness("A1");
    let before = cells(&h);
    press(&mut h, "C3");
    for end in ["D4", "F7", "B2"] {
        let to = point(&h, end);
        frame(&mut h, Event::PointerMoved(to));
        assert_selection(&h, "C3", end);
    }
    let to = point(&h, "B2");
    frame(&mut h, button(to, false));
    assert_selection(&h, "C3", "B2");
    assert_eq!(cells(&h), before);
}

#[test]
fn clicking_then_hovering_does_not_extend_selection() {
    let mut h = harness("A1");
    let before = cells(&h);
    press(&mut h, "C3");
    let from = point(&h, "C3");
    frame(&mut h, button(from, false));
    let to = point(&h, "F7");
    frame(&mut h, Event::PointerMoved(to));
    assert_selection(&h, "C3", "C3");
    assert_eq!(cells(&h), before);
}

#[test]
fn formula_point_mode_consumes_first_movement_and_release() {
    let mut h = harness("A1");
    let before = cells(&h);
    h.state_mut().begin_edit(Some("=".into()), false);
    h.run_steps(2);
    press(&mut h, "C3");
    let mid = point(&h, "D4");
    frame(&mut h, Event::PointerMoved(mid));
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=C3:D4");
    let to = point(&h, "F7");
    frame(&mut h, button(to, false));
    h.run_steps(1);
    let editor = h.state().editor.as_ref().unwrap();
    assert_eq!(editor.text, "=C3:F7");
    assert_eq!(editor.point_cell, CellRef::parse("F7"));
    assert_eq!(editor.point_anchor, Some((0, CellRef::parse("C3").unwrap())));
    assert_eq!(cells(&h), before);
}

#[test]
fn formula_point_drag_keeps_the_sheet_qualifier() {
    let mut h = harness("A1");
    let before = cells(&h);
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other sheet"})).unwrap();
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    h.state_mut().begin_edit(Some("=".into()), false);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Other sheet"})).unwrap();
    h.run_steps(2);
    let other_before = cells(&h);
    press(&mut h, "C3");
    let mid = point(&h, "D4");
    frame(&mut h, Event::PointerMoved(mid));
    assert_eq!(h.state().editor.as_ref().unwrap().text, "='Other sheet'!C3:D4");
    let to = point(&h, "F7");
    frame(&mut h, button(to, false));
    assert_eq!(h.state().editor.as_ref().unwrap().text, "='Other sheet'!C3:F7");
    assert_eq!(cells(&h), other_before);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    assert_eq!(cells(&h), before);
}

#[test]
fn formula_modifier_command_click_appends_an_argument() {
    let mut h = harness("J10");
    let before = cells(&h);
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.run_steps(2);
    click_with_modifiers(&mut h, "C3", Modifiers::NONE);
    let command = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    click_with_modifiers(&mut h, "F7", command);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=SUM(C3,F7");
    assert_eq!(cells(&h), before);
}

#[test]
fn formula_modifier_shift_click_extends_the_pointed_range() {
    let mut h = harness("J10");
    let before = cells(&h);
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.run_steps(2);
    click_with_modifiers(&mut h, "C3", Modifiers::NONE);
    click_with_modifiers(&mut h, "F7", Modifiers::SHIFT);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=SUM(C3:F7");
    assert_eq!(cells(&h), before);
}

#[test]
fn formula_modifier_command_click_keeps_previous_arguments_replaceable_independently() {
    for command in [Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE }, Modifiers { ctrl: true, command: true, ..Modifiers::NONE }] {
        let mut h = harness("J10");
        let before = cells(&h);
        h.state_mut().begin_edit(Some("=SUM(".into()), false);
        h.run_steps(2);
        click_with_modifiers(&mut h, "C3", Modifiers::NONE);
        click_with_modifiers(&mut h, "D4", command);
        click_with_modifiers(&mut h, "F7", command);
        assert_eq!(h.state().editor.as_ref().unwrap().text, "=SUM(C3,D4,F7");
        click_with_modifiers(&mut h, "G7", Modifiers::NONE);
        assert_eq!(h.state().editor.as_ref().unwrap().text, "=SUM(C3,D4,G7");
        click_with_modifiers(&mut h, "F9", Modifiers::SHIFT);
        assert_eq!(h.state().editor.as_ref().unwrap().text, "=SUM(C3,D4,F7:G9");
        assert_eq!(cells(&h), before);
    }
}

#[test]
fn formula_modifier_shift_click_keeps_its_anchor_in_both_directions() {
    for (start, first, second, expected) in [("C3", "F7", "B2", "B2:C3"), ("F7", "C3", "H9", "F7:H9")] {
        let mut h = harness("J10");
        let before = cells(&h);
        h.state_mut().begin_edit(Some("=SUM(".into()), false);
        h.run_steps(2);
        click_with_modifiers(&mut h, start, Modifiers::NONE);
        click_with_modifiers(&mut h, first, Modifiers::SHIFT);
        click_with_modifiers(&mut h, second, Modifiers::SHIFT);
        assert_eq!(h.state().editor.as_ref().unwrap().text, format!("=SUM({expected}"));
        assert_eq!(cells(&h), before);
    }
}

#[test]
fn formula_modifier_command_click_respects_locale_and_a_typed_separator() {
    use gridcraft_engine::formula::FormulaLocale;
    let command = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    for (locale, prefix, separator) in [(FormulaLocale::En, "=SUM(", ","), (FormulaLocale::Es, "=SUMA(", ";")] {
        let mut h = harness("J10");
        h.state_mut().begin_edit(Some(prefix.into()), false);
        h.state_mut().editor.as_mut().unwrap().locale = locale;
        h.run_steps(2);
        click_with_modifiers(&mut h, "C3", command);
        assert_eq!(h.state().editor.as_ref().unwrap().text, format!("{prefix}C3"));
        click_with_modifiers(&mut h, "F7", command);
        assert_eq!(h.state().editor.as_ref().unwrap().text, format!("{prefix}C3{separator}F7"));
        frame(&mut h, Event::ModifiersChanged(Modifiers::NONE));
        frame(&mut h, Event::Text(separator.into()));
        h.run_steps(2);
        assert!(h.state().editor.as_ref().unwrap().point_anchor.is_none());
        click_with_modifiers(&mut h, "G8", command);
        assert_eq!(h.state().editor.as_ref().unwrap().text, format!("{prefix}C3{separator}F7{separator}G8"));
    }
}

#[test]
fn formula_modifier_shift_click_does_not_reuse_an_anchor_from_another_sheet() {
    let mut h = harness("J10");
    h.state_mut().session.execute("home.insertSheet", json!({"name": "Other sheet"})).unwrap();
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    h.state_mut().begin_edit(Some("=SUM(".into()), false);
    h.run_steps(2);
    click_with_modifiers(&mut h, "C3", Modifiers::NONE);
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Other sheet"})).unwrap();
    h.run_steps(2);
    click_with_modifiers(&mut h, "F7", Modifiers::SHIFT);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=SUM('Other sheet'!F7");
    click_with_modifiers(&mut h, "D4", Modifiers::SHIFT);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=SUM('Other sheet'!D4:F7");
}

#[test]
fn formula_modifier_shift_click_then_arrow_moves_from_the_clicked_endpoint() {
    use egui::Key;
    for (modifiers, expected) in [(Modifiers::SHIFT, "=SUM(C3:G7"), (Modifiers::NONE, "=SUM(G7")] {
        let mut h = harness("J10");
        h.state_mut().begin_edit(Some("=SUM(".into()), false);
        h.run_steps(2);
        click_with_modifiers(&mut h, "C3", Modifiers::NONE);
        click_with_modifiers(&mut h, "F7", Modifiers::SHIFT);
        frame(&mut h, Event::ModifiersChanged(modifiers));
        for pressed in [true, false] {
            frame(&mut h, Event::Key { key: Key::ArrowRight, physical_key: None, pressed, repeat: false, modifiers });
        }
        h.run_steps(2);
        assert_eq!(h.state().editor.as_ref().unwrap().text, expected);
        assert_eq!(h.state().editor.as_ref().unwrap().point_cell, CellRef::parse("G7"));
    }
}

#[test]
fn formula_arrow_pointing_restarts_from_the_edited_cell_after_typing() {
    let mut h = harness("B4");
    h.state_mut().begin_edit(Some("=".into()), false);
    h.run_steps(2);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(1);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(1);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=B2");
    frame(&mut h, Event::Text("+".into()));
    h.run_steps(1);
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(1);
    assert_eq!(h.state().editor.as_ref().unwrap().text, "=B2+B3");
}

#[test]
fn ctrl_click_deselects_a_cell_inside_a_ctrl_dragged_range() {
    let mut h = harness("A1");
    h.state_mut().session.execute("range.setValues", json!({"range": "A1:D3", "values": [[1, 2, 3, 4], [5, 6, 7, 8], [9, 10, 11, 12]]})).unwrap();
    h.run_steps(2);

    let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
    let b1 = point(&h, "B1");
    let d3 = point(&h, "D3");
    frame(&mut h, Event::ModifiersChanged(ctrl));
    frame(&mut h, Event::PointerMoved(b1));
    frame(&mut h, Event::PointerButton { pos: b1, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h, Event::PointerMoved(d3));
    frame(&mut h, Event::PointerButton { pos: d3, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h.run_steps(2);

    let count = |h: &Harness| {
        let d = h.state().session.active().unwrap();
        gridcraft_engine::display::stats(d.wb.active().unwrap(), &d.selection.ranges).count
    };
    assert_eq!(count(&h), 10, "A1 plus B1:D3 selects 10 populated cells");

    // Ctrl+click C2 inside the Ctrl-dragged range B1:D3: C2 must be removed, leaving the other 9 cells.
    click_with_modifiers(&mut h, "C2", ctrl);
    let sel = &h.state().session.active().unwrap().selection;
    assert!(!sel.contains(CellRef::parse("C2").unwrap()), "C2 must be deselected, got {}", sel.a1());
    assert_eq!(count(&h), 9, "deselecting C2 must reduce Count from 10 to 9 (selection: {})", sel.a1());
    for kept in ["A1", "B1", "C1", "D1", "B2", "D2", "B3", "C3", "D3"] {
        assert!(sel.contains(CellRef::parse(kept).unwrap()), "{kept} must stay selected, got {}", sel.a1());
    }

    // A slightly wobbly Ctrl+click that crosses egui's drag threshold must still deselect, not
    // turn the remaining split area into a selection drag.
    let c1 = point(&h, "C1");
    frame(&mut h, Event::ModifiersChanged(ctrl));
    frame(&mut h, Event::PointerMoved(c1));
    frame(&mut h, Event::PointerButton { pos: c1, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h, Event::PointerMoved(c1 + egui::vec2(8.0, 0.0)));
    frame(&mut h, Event::PointerButton { pos: c1 + egui::vec2(8.0, 0.0), button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h.run_steps(2);
    let sel = &h.state().session.active().unwrap().selection;
    assert!(!sel.contains(CellRef::parse("C1").unwrap()), "wobbly Ctrl+click must still deselect C1, got {}", sel.a1());
    assert_eq!(count(&h), 8, "Count must drop to 8 after deselecting C1 (selection: {})", sel.a1());

    // Ctrl+clicking C2 again re-selects it.
    click_with_modifiers(&mut h, "C2", ctrl);
    assert!(h.state().session.active().unwrap().selection.contains(CellRef::parse("C2").unwrap()));
    assert_eq!(count(&h), 9);

    // Ctrl+clicking a merged cell inside a selected range removes the whole merged cell so merge
    // expansion does not pull it right back into the selection.
    h.state_mut().session.execute("home.mergeCells", json!({"range": "B2:C2"})).unwrap();
    h.state_mut().session.execute("selection.set", json!({"range": "A1:D3"})).unwrap();
    h.run_steps(2);
    click_with_modifiers(&mut h, "C2", ctrl);
    let sel = &h.state().session.active().unwrap().selection;
    assert!(
        !sel.contains(CellRef::parse("B2").unwrap()) && !sel.contains(CellRef::parse("C2").unwrap()),
        "merged B2:C2 must be deselected, got {}",
        sel.a1()
    );

    // A plain click on the same cell right after Ctrl+click must select it, not enter edit mode
    // on whatever cell is currently active.
    click_with_modifiers(&mut h, "C2", Modifiers::NONE);
    assert!(h.state().editor.is_none(), "plain click after Ctrl+click must not open the cell editor");

    // Neighboring merges crossing BOTH horizontal and vertical split boundaries (A1:B2 and C3:D3
    // merged, A1:D3 selected): Ctrl+clicking C2 must keep C2 excluded while retaining every other cell.
    let mut h_merge = harness("A1");
    h_merge.state_mut().session.execute("home.mergeCells", json!({"range": "A1:B2"})).unwrap();
    h_merge.state_mut().session.execute("home.mergeCells", json!({"range": "C3:D3"})).unwrap();
    h_merge.state_mut().session.execute("selection.set", json!({"range": "A1:D3"})).unwrap();
    h_merge.run_steps(2);
    click_with_modifiers(&mut h_merge, "C2", ctrl);
    let sel = &h_merge.state().session.active().unwrap().selection;
    assert!(!sel.contains(CellRef::parse("C2").unwrap()), "C2 must stay deselected beside merged A1:B2 and C3:D3, got {}", sel.a1());
    for kept in ["A1", "B1", "C1", "D1", "A2", "B2", "D2", "A3", "B3", "C3", "D3"] {
        assert!(sel.contains(CellRef::parse(kept).unwrap()), "{kept} must stay selected, got {}", sel.a1());
    }

    // Protected sheet allowing only unlocked cell selection: Ctrl+click removal must keep a
    // selectable active cell rather than failing silently on a locked range start.
    let mut h_prot = harness("A1");
    h_prot.state_mut().session.execute("home.formatCells", json!({"range": "B2,D2", "style": {"protection": {"locked": false}}})).unwrap();
    h_prot.state_mut().session.execute("selection.set", json!({"range": "A1:D3", "active": "B2"})).unwrap();
    h_prot.state_mut().session.execute("review.protectSheet", json!({"selectLockedCells": false})).unwrap();
    h_prot.run_steps(2);
    click_with_modifiers(&mut h_prot, "C2", ctrl);
    let sel = &h_prot.state().session.active().unwrap().selection;
    assert!(!sel.contains(CellRef::parse("C2").unwrap()), "C2 must be deselected on protected sheet, got {}", sel.a1());
    assert_eq!(sel.active, CellRef::parse("B2").unwrap(), "unlocked active cell B2 must stay active");
    click_with_modifiers(&mut h_prot, "B2", ctrl);
    let sel = &h_prot.state().session.active().unwrap().selection;
    assert!(!sel.contains(CellRef::parse("B2").unwrap()), "removing active unlocked cell B2 must succeed, got {}", sel.a1());
    assert_eq!(sel.active, CellRef::parse("D2").unwrap(), "active cell must move to remaining unlocked cell D2");

    // Sparse search reaches distant unlocked cells past any small row prefix (e.g. A1 and A20002).
    let mut h_far = harness("A1");
    h_far.state_mut().session.execute("home.formatCells", json!({"range": "A1,A20002", "style": {"protection": {"locked": false}}})).unwrap();
    h_far.state_mut().session.execute("selection.set", json!({"range": "A1:A20002", "active": "A1"})).unwrap();
    h_far.state_mut().session.execute("review.protectSheet", json!({"selectLockedCells": false})).unwrap();
    h_far.run_steps(2);
    click_with_modifiers(&mut h_far, "A1", ctrl);
    let sel = &h_far.state().session.active().unwrap().selection;
    assert!(!sel.contains(CellRef::parse("A1").unwrap()), "A1 must be deselected from A1:A20002, got {}", sel.a1());
    assert_eq!(sel.active, CellRef::parse("A20002").unwrap(), "active cell must move to distant unlocked cell A20002");

    // Full-sheet protected range where B:XFD column styles override default locked style to
    // unlocked while selectUnlockedCells is false: sparse default-style fallback must finish
    // immediately instead of scanning 17 billion cells.
    let mut h_full = harness("A1");
    h_full.state_mut().session.execute("home.formatCells", json!({"range": "B:XFD", "style": {"protection": {"locked": false}}})).unwrap();
    h_full.state_mut().session.execute("selection.set", json!({"range": "A1:XFD1048576", "active": "A1"})).unwrap();
    h_full.state_mut().session.execute("review.protectSheet", json!({"selectLockedCells": true, "selectUnlockedCells": false})).unwrap();
    h_full.run_steps(2);
    // Ctrl-click column A header (above A1): remaining B:XFD has no selectable cell, so the search
    // returns immediately and selection.set safely rejects the removal without hanging.
    let a1_pos = point(&h_full, "A1");
    let geo = h_full.state().grid.geo.as_ref().unwrap();
    let col_a_header = egui::pos2(a1_pos.x, geo.cells.top() - geo.header_h * 0.5);
    frame(&mut h_full, Event::ModifiersChanged(ctrl));
    frame(&mut h_full, Event::PointerMoved(col_a_header));
    for pressed in [true, false] {
        frame(&mut h_full, Event::PointerButton { pos: col_a_header, button: PointerButton::Primary, pressed, modifiers: ctrl });
    }
    h_full.run_steps(2);
    assert_eq!(h_full.state().session.active().unwrap().selection.active, CellRef::parse("A1").unwrap());
}

#[test]
fn ctrl_drag_from_selected_areas_and_merge_header_deselect() {
    let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };

    // Ctrl-dragging from the sole selected cell extends it instead of dropping the drag.
    let mut h_drag = harness("B2");
    let b2 = point(&h_drag, "B2");
    let d4 = point(&h_drag, "D4");
    frame(&mut h_drag, Event::ModifiersChanged(ctrl));
    frame(&mut h_drag, Event::PointerMoved(b2));
    frame(&mut h_drag, Event::PointerButton { pos: b2, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h_drag, Event::PointerMoved(b2 + egui::vec2(8.0, 0.0)));
    frame(&mut h_drag, Event::PointerMoved(d4));
    frame(&mut h_drag, Event::PointerButton { pos: d4, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h_drag.run_steps(2);
    assert_eq!(h_drag.state().session.active().unwrap().selection.a1(), "B2:D4");

    // Slow Ctrl-drag starting on a cell inside a multi-cell range extends from it rather than
    // deselecting it on the first within-cell drag frame.
    let mut h_slow = harness("A1:D3");
    let b2 = point(&h_slow, "B2");
    let e5 = point(&h_slow, "E5");
    frame(&mut h_slow, Event::ModifiersChanged(ctrl));
    frame(&mut h_slow, Event::PointerMoved(b2));
    frame(&mut h_slow, Event::PointerButton { pos: b2, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h_slow, Event::PointerMoved(b2 + egui::vec2(8.0, 0.0)));
    frame(&mut h_slow, Event::PointerMoved(e5));
    frame(&mut h_slow, Event::PointerButton { pos: e5, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h_slow.run_steps(2);
    assert_eq!(h_slow.state().session.active().unwrap().selection.a1(), "A1:D3,B2:E5");

    // Single-movement Ctrl-drag from a selected column/row header extends to the target header.
    let mut h_col = harness("A:A");
    let col_a = {
        let a1 = point(&h_col, "A1");
        let geo = h_col.state().grid.geo.as_ref().unwrap();
        egui::pos2(a1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    let col_c = {
        let c1 = point(&h_col, "C1");
        let geo = h_col.state().grid.geo.as_ref().unwrap();
        egui::pos2(c1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    frame(&mut h_col, Event::ModifiersChanged(ctrl));
    frame(&mut h_col, Event::PointerMoved(col_a));
    frame(&mut h_col, Event::PointerButton { pos: col_a, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h_col, Event::PointerMoved(col_c));
    frame(&mut h_col, Event::PointerButton { pos: col_c, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h_col.run_steps(2);
    assert_eq!(h_col.state().session.active().unwrap().selection.a1(), "A:C");

    let mut h_row = harness("1:1");
    let row_1 = {
        let a1 = point(&h_row, "A1");
        let geo = h_row.state().grid.geo.as_ref().unwrap();
        egui::pos2(geo.cells.left() - geo.header_w * 0.5, a1.y)
    };
    let row_3 = {
        let a3 = point(&h_row, "A3");
        let geo = h_row.state().grid.geo.as_ref().unwrap();
        egui::pos2(geo.cells.left() - geo.header_w * 0.5, a3.y)
    };
    frame(&mut h_row, Event::ModifiersChanged(ctrl));
    frame(&mut h_row, Event::PointerMoved(row_1));
    frame(&mut h_row, Event::PointerButton { pos: row_1, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h_row, Event::PointerMoved(row_3));
    frame(&mut h_row, Event::PointerButton { pos: row_3, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h_row.run_steps(2);
    assert_eq!(h_row.state().session.active().unwrap().selection.a1(), "1:3");

    // Multi-frame Shift+F8 Add-mode drag from a sole selected column header does not duplicate A:A.
    let mut h_add_col = harness("A:A");
    frame(&mut h_add_col, Event::ModifiersChanged(Modifiers::SHIFT));
    for pressed in [true, false] {
        frame(&mut h_add_col, Event::Key { key: egui::Key::F8, physical_key: None, pressed, repeat: false, modifiers: Modifiers::SHIFT });
    }
    frame(&mut h_add_col, Event::ModifiersChanged(Modifiers::NONE));
    let col_a = {
        let a1 = point(&h_add_col, "A1");
        let geo = h_add_col.state().grid.geo.as_ref().unwrap();
        egui::pos2(a1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    let col_c = {
        let c1 = point(&h_add_col, "C1");
        let geo = h_add_col.state().grid.geo.as_ref().unwrap();
        egui::pos2(c1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    frame(&mut h_add_col, Event::PointerMoved(col_a));
    frame(&mut h_add_col, Event::PointerButton { pos: col_a, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    frame(&mut h_add_col, Event::PointerMoved(col_a + egui::vec2(8.0, 0.0)));
    frame(&mut h_add_col, Event::PointerMoved(col_c));
    frame(&mut h_add_col, Event::PointerButton { pos: col_c, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h_add_col.run_steps(2);
    assert_eq!(h_add_col.state().session.active().unwrap().selection.a1(), "A:C");

    // Wobbly Ctrl-click crossing an internal boundary of a merged cell still deselects the merge.
    let mut h_mwob = harness("A1");
    h_mwob.state_mut().session.execute("home.mergeCells", json!({"range": "B2:C2"})).unwrap();
    h_mwob.state_mut().session.execute("selection.set", json!({"range": "A1:D3"})).unwrap();
    h_mwob.run_steps(2);
    let b2 = point(&h_mwob, "B2");
    let c2 = point(&h_mwob, "C2");
    frame(&mut h_mwob, Event::ModifiersChanged(ctrl));
    frame(&mut h_mwob, Event::PointerMoved(b2));
    frame(&mut h_mwob, Event::PointerButton { pos: b2, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h_mwob, Event::PointerMoved(c2));
    frame(&mut h_mwob, Event::PointerButton { pos: c2, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h_mwob.run_steps(2);
    let sel = &h_mwob.state().session.active().unwrap().selection;
    assert!(
        !sel.contains(CellRef::parse("B2").unwrap()) && !sel.contains(CellRef::parse("C2").unwrap()),
        "wobbly Ctrl+click inside merge B2:C2 must deselect it, got {}",
        sel.a1()
    );

    // Header deselection across a crossing merge must not let merge-expansion pull column A back in.
    let mut h_mhdr = harness("A1");
    h_mhdr.state_mut().session.execute("home.mergeCells", json!({"range": "A1:B2"})).unwrap();
    h_mhdr.state_mut().session.execute("selection.set", json!({"range": "A:A,A1:C3"})).unwrap();
    h_mhdr.run_steps(2);
    let col_a = {
        let a1 = point(&h_mhdr, "A1");
        let geo = h_mhdr.state().grid.geo.as_ref().unwrap();
        egui::pos2(a1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    frame(&mut h_mhdr, Event::ModifiersChanged(ctrl));
    frame(&mut h_mhdr, Event::PointerMoved(col_a));
    for pressed in [true, false] {
        frame(&mut h_mhdr, Event::PointerButton { pos: col_a, button: PointerButton::Primary, pressed, modifiers: ctrl });
    }
    h_mhdr.run_steps(2);
    let sel = &h_mhdr.state().session.active().unwrap().selection;
    assert!(
        !sel.contains(CellRef::parse("A1").unwrap()) && !sel.contains(CellRef::parse("A2").unwrap()),
        "column A must stay deselected across crossing merge A1:B2, got {}",
        sel.a1()
    );
    assert!(
        sel.contains(CellRef::parse("C1").unwrap()) && sel.contains(CellRef::parse("B3").unwrap()),
        "non-crossing cells outside column A must stay selected, got {}",
        sel.a1()
    );

    // Switching to Add mode (Shift+F8) and clicking the same cell within 450ms must not open the editor.
    let mut h_mode = harness("B2");
    click_with_modifiers(&mut h_mode, "B2", Modifiers::NONE);
    frame(&mut h_mode, Event::ModifiersChanged(Modifiers::SHIFT));
    for pressed in [true, false] {
        frame(&mut h_mode, Event::Key { key: egui::Key::F8, physical_key: None, pressed, repeat: false, modifiers: Modifiers::SHIFT });
    }
    frame(&mut h_mode, Event::ModifiersChanged(Modifiers::NONE));
    click_with_modifiers(&mut h_mode, "B2", Modifiers::NONE);
    assert!(h_mode.state().editor.is_none(), "Add-mode click within 450ms must not open the editor");
}

#[test]
fn ctrl_header_drag_keeps_other_areas_and_merge_or_protection_guards_hold() {
    let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };

    // Ctrl-dragging a selected column header in normal mode keeps unrelated selected areas.
    let mut h_keep = harness("E:E,A:A");
    let col_a = {
        let a1 = point(&h_keep, "A1");
        let geo = h_keep.state().grid.geo.as_ref().unwrap();
        egui::pos2(a1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    let col_c = {
        let c1 = point(&h_keep, "C1");
        let geo = h_keep.state().grid.geo.as_ref().unwrap();
        egui::pos2(c1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    frame(&mut h_keep, Event::ModifiersChanged(ctrl));
    frame(&mut h_keep, Event::PointerMoved(col_a));
    frame(&mut h_keep, Event::PointerButton { pos: col_a, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h_keep, Event::PointerMoved(col_c));
    frame(&mut h_keep, Event::PointerButton { pos: col_c, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h_keep.run_steps(2);
    assert_eq!(h_keep.state().session.active().unwrap().selection.a1(), "E:E,A:C");

    // On a protected sheet that forbids selecting locked cells, Ctrl-dragging from a locked cell
    // inside the selection must not resize the existing range from the old unlocked anchor.
    let mut h_prot = harness("A1");
    h_prot.state_mut().session.execute("home.formatCells", json!({"range": "A1", "style": {"protection": {"locked": false}}})).unwrap();
    h_prot.state_mut().session.execute("selection.set", json!({"range": "A1:D3", "active": "A1"})).unwrap();
    h_prot.state_mut().session.execute("review.protectSheet", json!({"selectLockedCells": false})).unwrap();
    h_prot.run_steps(2);
    let b2 = point(&h_prot, "B2");
    let e5 = point(&h_prot, "E5");
    frame(&mut h_prot, Event::ModifiersChanged(ctrl));
    frame(&mut h_prot, Event::PointerMoved(b2));
    frame(&mut h_prot, Event::PointerButton { pos: b2, button: PointerButton::Primary, pressed: true, modifiers: ctrl });
    frame(&mut h_prot, Event::PointerMoved(e5));
    frame(&mut h_prot, Event::PointerButton { pos: e5, button: PointerButton::Primary, pressed: false, modifiers: ctrl });
    h_prot.run_steps(2);
    assert_eq!(h_prot.state().session.active().unwrap().selection.a1(), "A1:D3");

    // Subtracting A2 from column A when A1:B1 is merged must not pull B1 into the selection.
    let mut h_spill = harness("A1");
    h_spill.state_mut().session.execute("home.mergeCells", json!({"range": "A1:B1"})).unwrap();
    h_spill.state_mut().session.execute("selection.set", json!({"range": "A:A"})).unwrap();
    h_spill.run_steps(2);
    click_with_modifiers(&mut h_spill, "A2", ctrl);
    let sel = &h_spill.state().session.active().unwrap().selection;
    assert!(!sel.contains(CellRef::parse("A2").unwrap()), "A2 must be deselected, got {}", sel.a1());
    assert!(!sel.contains(CellRef::parse("B1").unwrap()), "B1 outside column A must not be added, got {}", sel.a1());
    assert!(sel.contains(CellRef::parse("A3").unwrap()), "remaining cells in column A must stay selected, got {}", sel.a1());

    // Ctrl+Shift-dragging a header inside an existing selection extends rather than toggling it off.
    let ctrl_shift = Modifiers { ctrl: true, command: true, shift: true, ..Modifiers::NONE };
    let mut h_cs = harness("A:C");
    let col_b = {
        let b1 = point(&h_cs, "B1");
        let geo = h_cs.state().grid.geo.as_ref().unwrap();
        egui::pos2(b1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    let col_d = {
        let d1 = point(&h_cs, "D1");
        let geo = h_cs.state().grid.geo.as_ref().unwrap();
        egui::pos2(d1.x, geo.cells.top() - geo.header_h * 0.5)
    };
    frame(&mut h_cs, Event::ModifiersChanged(ctrl_shift));
    frame(&mut h_cs, Event::PointerMoved(col_b));
    frame(&mut h_cs, Event::PointerButton { pos: col_b, button: PointerButton::Primary, pressed: true, modifiers: ctrl_shift });
    frame(&mut h_cs, Event::PointerMoved(col_d));
    frame(&mut h_cs, Event::PointerButton { pos: col_d, button: PointerButton::Primary, pressed: false, modifiers: ctrl_shift });
    h_cs.run_steps(2);
    let sel = &h_cs.state().session.active().unwrap().selection;
    assert!(
        sel.contains(CellRef::parse("A1").unwrap()) && sel.contains(CellRef::parse("D1").unwrap()),
        "Ctrl+Shift header drag must extend A:C to D instead of toggling A:B off, got {}",
        sel.a1()
    );
}
