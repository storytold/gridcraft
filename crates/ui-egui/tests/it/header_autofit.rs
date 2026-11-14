//! Header AutoFit and resize gestures, driven through real egui pointer events.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness(selection: &str) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "range.setValues",
            json!({"range": "A1", "values": [
                ["Short", "This column has considerably longer content", "Third", "Unchanged"],
                ["Two\nlines", "B", "C", "D"],
                ["Three\ntext\nlines", "B", "C", "D"]
            ]}),
        )
        .unwrap();
    session.execute("home.columnWidth", json!({"cols": "A:D", "width": 100})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "1:4", "height": 60})).unwrap();
    session.execute("selection.set", json!({"range": selection})).unwrap();
    let mut h = crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    h.state_mut().session.journal.clear();
    h
}

fn edge(h: &egui_kittest::Harness<'_, SheetApp>, rows: bool, index: u32) -> (Pos2, f32) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = if rows {
        pos2(geo.rect.left() + geo.header_w / 2.0, geo.y(sheet, index + 1))
    } else {
        pos2(geo.x(sheet, index + 1), geo.rect.top() + geo.header_h / 2.0)
    };
    (pos, geo.z)
}

fn button(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2, pressed: bool) {
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    h.step();
}

fn sizes(h: &egui_kittest::Harness<'_, SheetApp>, rows: bool) -> Vec<f32> {
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    (0..4).map(|index| if rows { sheet.row_height(index) } else { sheet.col_width(index) }).collect()
}

#[test]
fn header_double_click_autofits_selected_areas_or_only_the_clicked_line() {
    for (rows, selection, clicked, changed) in [
        (false, "B:B", 1, [false, true, false, false]),
        (false, "A:C", 1, [true, true, true, false]),
        (false, "A:A,C:C", 0, [true, false, true, false]),
        (false, "A:B", 2, [false, false, true, false]),
        (true, "1:3", 1, [true, true, true, false]),
        (true, "1:1,3:3", 0, [true, false, true, false]),
        (true, "1:2", 2, [false, false, true, false]),
    ] {
        let mut h = harness(selection);
        let before = sizes(&h, rows);
        let selection_before = h.state().session.active().unwrap().selection.clone();
        let (pos, _) = edge(&h, rows, clicked);
        button(&mut h, pos, true);
        button(&mut h, pos, false);
        h.step(); // Let the first click's resize state settle before the second click.
        button(&mut h, pos, true);
        button(&mut h, pos, false);
        h.run_steps(2);

        for (index, ((old, new), changed)) in before.iter().zip(sizes(&h, rows)).zip(changed).enumerate() {
            assert_eq!(*old != new, changed, "selection {selection}, clicked {clicked}, line {index}: {old} → {new}");
        }
        assert_eq!(h.state().session.active().unwrap().selection, selection_before);
        let command = if rows { "home.autofitRowHeight" } else { "home.autofitColumnWidth" };
        assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == command).count(), 1);
        assert_eq!(h.state().session.active().unwrap().undo.len(), 1);
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(sizes(&h, rows), before, "AutoFit should undo as a single operation");
    }
}

#[test]
fn dragging_a_header_boundary_still_resizes_normally() {
    for rows in [false, true] {
        let mut h = harness(if rows { "2:2" } else { "B:B" });
        let before = sizes(&h, rows);
        let (start, zoom) = edge(&h, rows, 1);
        let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        button(&mut h, start + delta, false);
        h.run_steps(2);
        let after = sizes(&h, rows);
        assert_eq!(after[1], (before[1] + 30.0 / zoom).round());
        for index in [0, 2, 3] {
            assert_eq!(before[index], after[index]);
        }
        assert!(!h.state().session.journal.iter().any(|(id, _)| id.starts_with("home.autofit")));
    }
}

#[test]
fn dragging_a_boundary_inside_a_multi_area_selection_resizes_every_selected_line() {
    // Issue #341: Ctrl-selected rows/columns resize together; unselected lines keep their size.
    for (rows, selection, dragged, changed) in [
        (false, "A:A,C:C", 2, [true, false, true, false]),
        (false, "A:A,C:C", 0, [true, false, true, false]),
        (false, "A:A,C:D", 3, [true, false, true, true]),
        (false, "A:A,C:C", 1, [false, true, false, false]),
        (false, "A:A,C3", 0, [true, false, false, false]),
        (true, "1:1,3:3", 2, [true, false, true, false]),
        (true, "1:1,3:3", 0, [true, false, true, false]),
        (true, "1:1,3:4", 3, [true, false, true, true]),
        (true, "1:1,3:3", 1, [false, true, false, false]),
        (true, "1:1,C3", 0, [true, false, false, false]),
    ] {
        let mut h = harness(selection);
        let before = sizes(&h, rows);
        let (start, zoom) = edge(&h, rows, dragged);
        let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        button(&mut h, start + delta, false);
        h.run_steps(2);
        let after = sizes(&h, rows);
        let expected = (before[dragged as usize] + 30.0 / zoom).round();
        for (index, changed) in changed.iter().enumerate() {
            let want = if *changed { expected } else { before[index] };
            assert_eq!(after[index], want, "selection {selection}, dragged {dragged}, line {index}");
        }
        assert_eq!(h.state().session.active().unwrap().undo.len(), 1, "selection {selection}: one undo step");
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(sizes(&h, rows), before, "selection {selection}: undo restores every line");
    }
}

/// AutoFit keeps the actual painted heading clear of its filter button at every tested zoom.
#[test]
fn a_multi_selection_resize_previews_on_the_dragged_line_and_applies_on_release() {
    // Issue #1082: only the dragged line moves while the button is held; Esc cancels.
    for rows in [false, true] {
        let mut h = harness(if rows { "1:3" } else { "A:C" });
        let before = sizes(&h, rows);
        let (start, zoom) = edge(&h, rows, 1);
        let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        let held = sizes(&h, rows);
        assert_eq!(held[1], (before[1] + 30.0 / zoom).round(), "the dragged line previews");
        for index in [0, 2, 3] {
            assert_eq!(held[index], before[index], "line {index} waits for the release");
        }
        button(&mut h, start + delta, false);
        h.run_steps(2);
        let after = sizes(&h, rows);
        for (index, size) in after.iter().take(3).enumerate() {
            assert_eq!(*size, held[1], "line {index} takes the width on release");
        }
        assert_eq!(after[3], before[3]);
        assert_eq!(h.state().session.active().unwrap().undo.len(), 1);
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(sizes(&h, rows), before);

        // Esc during the drag cancels everything.
        let mut h = harness(if rows { "1:3" } else { "A:C" });
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        h.input_mut().events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        button(&mut h, start + delta, false);
        h.run_steps(2);
        assert_eq!(sizes(&h, rows), before, "Esc restores the sizes");
    }
}

#[test]
fn escaping_a_resize_drag_keeps_an_earlier_size_edit() {
    // A resize drag right after another resize shares its undo label; Esc must cancel only the drag.
    for (rows, selection) in [(false, "B:B"), (true, "2:2"), (false, "A:C"), (true, "1:3")] {
        let mut h = harness(selection);
        let key = if rows { ("home.rowHeight", "rows", "2:2", "height") } else { ("home.columnWidth", "cols", "B:B", "width") };
        h.state_mut().session.execute(key.0, json!({key.1: key.2, key.3: 80})).unwrap();
        h.run_steps(2);
        let earlier = sizes(&h, rows);
        let undo_len = h.state().session.active().unwrap().undo.len();
        let (start, _) = edge(&h, rows, 1);
        let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        assert_ne!(sizes(&h, rows), earlier, "the drag previews");
        h.input_mut().events.push(Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        button(&mut h, start + delta, false);
        h.run_steps(2);
        assert_eq!(sizes(&h, rows), earlier, "Esc cancels only the drag (rows: {rows}, selection {selection})");
        assert_eq!(h.state().session.active().unwrap().undo.len(), undo_len, "the earlier edit stays on the undo stack");
    }
}

#[test]
fn a_resize_drag_after_another_resize_is_its_own_undo_step() {
    for (rows, selection) in [(false, "B:B"), (true, "2:2"), (false, "A:C"), (true, "1:3")] {
        let mut h = harness(selection);
        let key = if rows { ("home.rowHeight", "rows", "2:2", "height") } else { ("home.columnWidth", "cols", "B:B", "width") };
        h.state_mut().session.execute(key.0, json!({key.1: key.2, key.3: 80})).unwrap();
        h.run_steps(2);
        let earlier = sizes(&h, rows);
        let (start, _) = edge(&h, rows, 1);
        let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        button(&mut h, start + delta, false);
        h.run_steps(2);
        assert_eq!(h.state().session.active().unwrap().undo.len(), 2, "selection {selection}: two undo steps");
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(sizes(&h, rows), earlier, "selection {selection}: one undo reverts only the drag");
    }
}

#[test]
fn table_filter_autofit_works_through_command_and_header_double_click() {
    for zoom in [50, 100, 200] {
        for double_click in [false, true] {
            let mut h = harness("A:A");
            h.state_mut().session.execute("range.setValues", json!({"range":"A1", "values":[["Quarterly Revenue"], ["1"], ["2"]]})).unwrap();
            h.state_mut().session.execute("insert.table", json!({"range":"A1:A3", "header":true})).unwrap();
            h.state_mut().session.execute("home.rowHeight", json!({"rows":"1:1", "height":20})).unwrap();
            h.state_mut().session.execute("view.zoom", json!({"percent":zoom})).unwrap();
            h.run_steps(3);
            h.state_mut().session.active_mut().unwrap().undo.clear();
            h.state_mut().session.journal.clear();
            if double_click {
                let (pos, _) = edge(&h, false, 0);
                button(&mut h, pos, true);
                button(&mut h, pos, false);
                h.step();
                button(&mut h, pos, true);
                button(&mut h, pos, false);
            } else {
                h.state_mut().run("home.autofitColumnWidth", json!({"cols":"A:A"})).unwrap();
            }
            h.run_steps(2);
            let app = h.state();
            let sheet = app.session.active().unwrap().wb.active().unwrap();
            let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
            let cell = gridcraft_engine::core::CellRef::new(0, 0);
            let rect = geo.cell_rect(sheet, cell);
            let button = gridcraft_ui_egui::grid::filter_button_rect(&geo, sheet, cell);
            // AutoFit uses the installed renderer font; the headless average-glyph estimate
            // can be wider and does not describe the text actually painted in this cell.
            let heading = h
                .output()
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == "Quarterly Revenue" && rect.contains(text.pos) => Some(text),
                    _ => None,
                })
                .expect("the table heading is painted in its cell");
            assert!(heading.pos.x + heading.galley.size().x < button.left(), "heading overlaps filter: zoom={zoom}, double_click={double_click}");
            assert_eq!(app.session.active().unwrap().undo.len(), 1);
            h.state_mut().session.execute("edit.undo", json!({})).unwrap();
            assert_eq!(sizes(&h, false)[0], 100.0);
        }
    }
}

fn press_escape(h: &mut egui_kittest::Harness<'_, SheetApp>) {
    h.input_mut().events.push(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
}

#[test]
fn a_rejected_final_preview_releases_with_the_size_last_shown() {
    // The engine refuses rows over 546 px and columns over 1800 px; the release must apply the last
    // accepted preview to the whole selection, not retry the refused size.
    for rows in [false, true] {
        let mut h = harness(if rows { "1:3" } else { "A:C" });
        let before = sizes(&h, rows);
        let (start, zoom) = edge(&h, rows, 1);
        let unit = |d: f32| if rows { vec2(0.0, d) } else { vec2(d, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + unit(15.0)));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + unit(30.0)));
        h.step();
        let shown = sizes(&h, rows)[1];
        assert_eq!(shown, (before[1] + 30.0 / zoom).round());
        h.input_mut().events.push(Event::PointerMoved(start + unit(4000.0 * zoom)));
        h.step();
        assert_eq!(sizes(&h, rows)[1], shown, "the refused size is not shown");
        button(&mut h, start + unit(4000.0 * zoom), false);
        h.run_steps(2);
        let after = sizes(&h, rows);
        for (index, size) in after.iter().take(3).enumerate() {
            assert_eq!(*size, shown, "line {index} takes the size that was shown (rows: {rows})");
        }
        assert_eq!(after[3], before[3]);
    }
}

#[test]
fn escaping_a_resize_drag_leaves_redo_history_as_it_was() {
    for rows in [false, true] {
        for with_redo in [false, true] {
            let mut h = harness(if rows { "1:3" } else { "A:C" });
            let cmd = if rows { ("home.rowHeight", "rows", "4:4", "height") } else { ("home.columnWidth", "cols", "D:D", "width") };
            let mut redo_len = 0;
            let mut d_size = 0.0;
            if with_redo {
                h.state_mut().session.execute(cmd.0, json!({cmd.1: cmd.2, cmd.3: 80})).unwrap();
                h.state_mut().session.execute("edit.undo", json!({})).unwrap();
                h.run_steps(2);
                redo_len = h.state().session.active().unwrap().redo.len();
                assert_eq!(redo_len, 1);
                d_size = sizes(&h, rows)[3];
            }
            let before = sizes(&h, rows);
            let (start, _) = edge(&h, rows, 1);
            let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
            button(&mut h, start, true);
            h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
            h.step();
            h.input_mut().events.push(Event::PointerMoved(start + delta));
            h.step();
            assert_ne!(sizes(&h, rows), before, "the drag previews");
            press_escape(&mut h);
            button(&mut h, start + delta, false);
            h.run_steps(2);
            assert_eq!(sizes(&h, rows), before);
            assert_eq!(h.state().session.active().unwrap().redo.len(), redo_len, "Esc leaves the redo stack alone (rows: {rows}, redo: {with_redo})");
            // Ctrl+Y brings back only what was undone before the drag.
            let redone = h.state_mut().session.execute("edit.redo", json!({}));
            if with_redo {
                redone.unwrap();
                let redone = sizes(&h, rows);
                assert_eq!(redone[3], 80.0);
                assert_ne!(redone[3], d_size);
                assert_eq!(redone[..3], before[..3], "the cancelled drag does not come back");
            } else {
                assert!(redone.is_err(), "nothing to redo after a cancelled drag");
            }
        }
    }
}

#[test]
fn escaping_a_resize_drag_keeps_a_pending_copy_and_format_painter() {
    // Esc cancels the drag only: the marked copy and Format Painter stay, whether the release comes
    // in a later frame or in the same frame as Esc.
    for rows in [false, true] {
        for (same_frame, modifiers) in [(false, Modifiers::NONE), (true, Modifiers::NONE), (false, Modifiers::SHIFT)] {
            let mut h = harness(if rows { "1:3" } else { "A:C" });
            h.state_mut().session.execute("edit.copy", json!({"range": "A1:B2"})).unwrap();
            h.state_mut().session.execute("edit.formatPainter", json!({"sticky": true})).unwrap();
            h.run_steps(2);
            assert!(h.state().session.clipboard.is_some() && h.state().session.format_painter.is_some());
            let before = sizes(&h, rows);
            let (start, _) = edge(&h, rows, 1);
            let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
            button(&mut h, start, true);
            h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
            h.step();
            h.input_mut().events.push(Event::PointerMoved(start + delta));
            h.step();
            assert_ne!(sizes(&h, rows), before, "the drag previews");
            let pos = start + delta;
            let esc = Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers };
            if same_frame {
                h.input_mut().events.extend([esc, Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers }]);
            } else {
                h.input_mut().events.push(esc);
                h.step();
                h.input_mut()
                    .events
                    .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers }]);
            }
            h.step();
            h.run_steps(2);
            let what = format!("rows: {rows}, same frame: {same_frame}, modifiers: {modifiers:?}");
            assert_eq!(sizes(&h, rows), before, "the drag is cancelled ({what})");
            assert!(h.state().session.clipboard.is_some(), "the copy is still marked ({what})");
            assert!(h.state().session.format_painter.is_some(), "Format Painter is still on ({what})");
        }
    }
}
