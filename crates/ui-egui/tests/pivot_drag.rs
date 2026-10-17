//! Dragging fields in the PivotTable Fields pane, through real egui pointer events, and the
//! commands a drop plans.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::pivot_pane::{PivotDrag, plan_drop};
use serde_json::json;

fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range": "A1:D4", "values": [
            ["Region", "Product", "Sales", "Units"],
            ["North", "Pens", 10, 1],
            ["South", "Ink", 20, 2],
            ["North", "Ink", 30, 3],
        ]}),
    )
    .unwrap();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!$A$1:$D$4", "rows": ["Region"], "values": ["Sales"]})).unwrap();
    s
}

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1400.0, 900.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn names<T>(list: &[T], name: impl Fn(&T) -> &str) -> Vec<String> {
    list.iter().map(|f| name(f).to_string()).collect()
}

fn rows(s: &Session) -> Vec<String> {
    names(&s.active().unwrap().wb.active().unwrap().pivots[0].rows, |f| &f.source_col)
}

fn columns(s: &Session) -> Vec<String> {
    names(&s.active().unwrap().wb.active().unwrap().pivots[0].columns, |f| &f.source_col)
}

/// Presses at `from`, drags in a few steps to `to`, releases there.
fn drag(h: &mut egui_kittest::Harness<'_, SheetApp>, from: Pos2, to: Pos2) {
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(from), press(from, true)]);
    h.step();
    for k in 1..=4 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (k as f32 / 4.0)));
        h.step();
    }
    h.input_mut().events.push(press(to, false));
    h.run_steps(3);
}

/// A point inside the area box under the `label` heading (Filters, Columns, Rows or Values).
fn area_box(h: &egui_kittest::Harness<'_, SheetApp>, label: &str) -> Pos2 {
    let r = h.get_by_label(label).rect();
    pos2(r.left() + 30.0, r.bottom() + 40.0)
}

#[test]
fn dragging_a_field_between_areas_moves_it() {
    let mut h = harness(session());
    let from = h.get_by_label("Region ▾").rect().center();
    let to = area_box(&h, "Columns");
    drag(&mut h, from, to);
    let s = &h.state().session;
    assert!(rows(s).is_empty(), "Region left Rows");
    assert_eq!(columns(s), vec!["Region".to_string()]);
    assert_eq!(s.journal.iter().filter(|(id, _)| id == "pivot.moveField").count(), 1);
}

#[test]
fn dragging_a_field_from_the_list_adds_it_where_it_is_dropped() {
    let mut h = harness(session());
    // Ahead of Region in Rows: drop on the top half of Region's entry.
    let from = h.get_by_label("Product").rect().center();
    let region = h.get_by_label("Region ▾").rect();
    drag(&mut h, from, pos2(region.center().x, region.top() + 3.0));
    assert_eq!(rows(&h.state().session), vec!["Product".to_string(), "Region".to_string()]);
}

#[test]
fn dragging_a_field_off_the_pane_removes_it() {
    let mut h = harness(session());
    let from = h.get_by_label("Region ▾").rect().center();
    drag(&mut h, from, pos2(200.0, 400.0));
    assert!(rows(&h.state().session).is_empty());
}

#[test]
fn a_click_still_opens_the_field_menu() {
    let mut h = harness(session());
    h.get_by_label("Region ▾").click();
    h.run_steps(2);
    assert!(h.query_by_label("Remove Field").is_some());
    assert_eq!(rows(&h.state().session), vec!["Region".to_string()]);
}

fn shown(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn reordering_within_an_area_counts_past_the_dragged_field() {
    let d = PivotDrag { field: "A".into(), from: Some("rows") };
    let cmds = plan_drop("P", &d, "rows", &shown(&["A", "B", "C"]), 3);
    assert_eq!(cmds, vec![("pivot.moveField", json!({"pivot": "P", "field": "A", "from": "rows", "to": "rows", "position": 2}))]);
    // Dropping a field back where it was does nothing.
    assert!(plan_drop("P", &d, "rows", &shown(&["A", "B", "C"]), 0).is_empty());
    assert!(plan_drop("P", &d, "rows", &shown(&["A", "B", "C"]), 1).is_empty());
}

#[test]
fn values_pseudo_field_only_drops_on_rows_or_columns_and_keeps_its_place() {
    // Dropped right after Σ Values in Rows: the field goes in, then Σ Values is put back ahead of it.
    let d = PivotDrag { field: "C".into(), from: Some("columns") };
    let cmds = plan_drop("P", &d, "rows", &shown(&["A", "Σ Values", "B"]), 2);
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].1["position"], json!(1));
    assert_eq!(cmds[1].1, json!({"pivot": "P", "field": "Σ Values", "from": "rows", "to": "rows", "position": 1}));

    // The commands land the field where it was dropped.
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1:E3", "values": [["A", "B", "C", "X", "Y"], ["a", "b", "c", 1, 2], ["a2", "b2", "c2", 3, 4]]}))
        .unwrap();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!$A$1:$E$3", "rows": ["A", "B"], "columns": ["C"], "values": ["X", "Y"]})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows", "position": 1})).unwrap();
    let name = s.active().unwrap().wb.active().unwrap().pivots[0].name.clone();
    for (id, p) in plan_drop(&name, &d, "rows", &shown(&["A", "Σ Values", "B"]), 2) {
        s.execute(id, p).unwrap();
    }
    let pt = &s.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(names(&pt.rows, |f| &f.source_col), vec!["A", "C", "B"]);
    assert_eq!(pt.values_position, Some(1), "Σ Values sits between A and the dropped C");
}
