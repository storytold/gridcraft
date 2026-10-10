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
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
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
