//! Pivot field arrangement driven by pointer input, rather than direct move commands.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "range.setValues",
            json!({"range":"A1", "values":[
                ["Region","Product","Sales","Units"],
                ["North","Apples",10,1], ["North","Pears",20,2],
                ["South","Apples",30,3], ["South","Pears",40,4],
                ["North","Apples",50,5], ["South","Pears",60,6]
            ]}),
        )
        .unwrap();
    session.execute("insert.pivotTable", json!({"source":"Sheet1!A1:D7","rows":["Region"],"values":["Sales"]})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1000.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    h.state_mut().session.journal.clear();
    h
}

fn button(h: &egui_kittest::Harness<'_, SheetApp>, field: &str) -> Pos2 {
    h.get_by_label(&format!("{field} ▾")).rect().center()
}

fn empty_area(h: &egui_kittest::Harness<'_, SheetApp>, label: &str) -> Pos2 {
    let r = h.get_by_label(label).rect();
    pos2(r.center().x, r.bottom() + 30.0)
}

fn pointer(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2, pressed: bool) {
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    h.step();
}

fn drag(h: &mut egui_kittest::Harness<'_, SheetApp>, start: Pos2, end: Pos2) {
    pointer(h, start, true);
    h.input_mut().events.push(Event::PointerMoved(start + vec2(12.0, 0.0)));
    h.step();
    h.input_mut().events.push(Event::PointerMoved(end));
    h.step();
    pointer(h, end, false);
    h.run_steps(2);
}

#[test]
fn dragging_rows_to_columns_moves_the_field_as_one_undo_step() {
    let mut h = harness();
    let start = button(&h, "Region");
    let end = empty_area(&h, "Columns");
    drag(&mut h, start, end);
    let d = h.state().session.active().unwrap();
    let pt = &d.wb.active().unwrap().pivots[0];
    assert!(pt.rows.is_empty());
    assert_eq!(pt.columns[0].source_col, "Region");
    assert_eq!(d.undo.len(), 1);
    assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == "pivot.moveField").count(), 1);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows[0].source_col, "Region");
    assert!(pt.columns.is_empty());
}

#[test]
fn assigned_field_can_move_through_all_four_areas() {
    let mut h = harness();
    for (from, to) in [("Region", "Columns"), ("Region", "Filters"), ("Region", "Values"), ("Count of Region", "Rows")] {
        let start = button(&h, from);
        let end = empty_area(&h, to);
        drag(&mut h, start, end);
        assert!(h.state().message.is_none());
        let fields = h.state_mut().session.run("pivot.fields", json!({})).unwrap();
        let key = to.to_lowercase();
        assert!(fields[&key].as_array().unwrap().iter().any(|f| f["field"] == "Region"));
    }
}

#[test]
fn dragging_source_to_values_adds_distinct_summaries() {
    let mut h = harness();
    for _ in 0..2 {
        let start = h.get_by_label("Sales").rect().center();
        let last = h.state().session.active().unwrap().wb.active().unwrap().pivots[0].values.last().unwrap().name.clone();
        let rect = h.get_by_label(&format!("{last} ▾")).rect();
        drag(&mut h, start, pos2(rect.center().x, rect.bottom() + 4.0));
    }
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.values.len(), 3);
    assert!(pt.values.iter().all(|v| v.source_col == "Sales"));
    assert_ne!(pt.values[0].name, pt.values[1].name);
    assert_ne!(pt.values[1].name, pt.values[2].name);
    assert_eq!(h.state().session.active().unwrap().undo.len(), 2);
}

#[test]
fn escape_and_pane_whitespace_cancel_without_mutation() {
    let mut h = harness();
    let original = h.state().session.active().unwrap().wb.active().unwrap().pivots[0].clone();
    let start = button(&h, "Region");
    let end = empty_area(&h, "Columns");
    pointer(&mut h, start, true);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.step();
    h.input_mut().events.push(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.step();
    pointer(&mut h, end, false);
    h.run_steps(2);
    let title = h.get_by_label("PivotTable Fields").rect().center();
    let start = button(&h, "Region");
    drag(&mut h, start, title);
    let d = h.state().session.active().unwrap();
    assert_eq!(serde_json::to_value(&d.wb.active().unwrap().pivots[0]).unwrap(), serde_json::to_value(&original).unwrap());
    assert!(d.undo.is_empty());
}

#[test]
fn clicking_still_opens_the_field_menu() {
    let mut h = harness();
    h.get_by_label("Region ▾").click();
    h.run_steps(2);
    assert!(h.query_by_label("Move to Column Labels").is_some());
}

#[test]
fn duplicate_values_reorder_preserves_the_exact_summary_and_settings() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Sales","area":"values","func":"average","name":"Mean sales"})).unwrap();
    h.state_mut()
        .session
        .execute("pivot.valueSettings", json!({"field":"Mean sales","numberFormat":"0.000","showAs":"percentOfGrandTotal"}))
        .unwrap();
    h.run_steps(3);
    let original = h.state().session.active().unwrap().wb.active().unwrap().pivots[0].values.clone();
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let start = button(&h, "Mean sales");
    let target = h.get_by_label("Sum of Sales ▾").rect();
    drag(&mut h, start, pos2(target.center().x, target.top() + 1.0));
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(serde_json::to_value(&pt.values[0]).unwrap(), serde_json::to_value(&original[1]).unwrap());
    assert_eq!(serde_json::to_value(&pt.values[1]).unwrap(), serde_json::to_value(&original[0]).unwrap());
    // Moving down must account for the original item's removal.
    let start = button(&h, "Mean sales");
    let target = h.get_by_label("Sum of Sales ▾").rect();
    drag(&mut h, start, pos2(target.center().x, target.bottom() + 3.0));
    let d = h.state().session.active().unwrap();
    assert_eq!(serde_json::to_value(&d.wb.active().unwrap().pivots[0].values).unwrap(), serde_json::to_value(&original).unwrap());
    assert_eq!(d.undo.len(), 2);
}

#[test]
fn dragging_does_not_mutate_until_release_and_same_slot_is_a_noop() {
    let mut h = harness();
    let start = button(&h, "Region");
    let end = empty_area(&h, "Columns");
    pointer(&mut h, start, true);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.step();
    let d = h.state().session.active().unwrap();
    assert_eq!(d.wb.active().unwrap().pivots[0].rows[0].source_col, "Region");
    assert!(d.undo.is_empty());
    pointer(&mut h, end, false);
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().pivots[0].columns[0].source_col, "Region");
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let start = button(&h, "Region");
    drag(&mut h, start, start + vec2(15.0, 0.0));
    assert!(h.state().session.active().unwrap().undo.is_empty());
}

#[test]
fn drag_label_has_room_for_an_ordinary_field_caption() {
    let mut h = harness();
    let start = button(&h, "Sum of Sales");
    pointer(&mut h, start, true);
    h.input_mut().events.push(Event::PointerMoved(start + vec2(20.0, 0.0)));
    h.step();
    h.run_steps(2);
    let label = h.get_by_label("Sum of Sales").rect();
    assert!(label.height() <= 24.0, "drag label wrapped: {label:?}");
    pointer(&mut h, start, false);
}

#[test]
fn dropping_one_summary_outside_removes_only_that_instance() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Sales","area":"values","func":"average","name":"Mean sales"})).unwrap();
    h.run_steps(3);
    let start = button(&h, "Mean sales");
    drag(&mut h, start, pos2(400.0, 300.0));
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.values.len(), 1);
    assert_eq!(pt.values[0].name, "Sum of Sales");
    assert_eq!(pt.rows[0].source_col, "Region");
}

#[test]
fn changed_document_or_revision_cannot_receive_an_old_drag() {
    for new_document in [false, true] {
        let mut h = harness();
        let start = button(&h, "Region");
        let end = empty_area(&h, "Columns");
        pointer(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(end));
        h.step();
        if new_document {
            h.state_mut().session.new_workbook();
        } else {
            h.state_mut().session.execute("pivot.valueSettings", json!({"field":"Sum of Sales","name":"Total sales"})).unwrap();
        }
        h.run_steps(2);
        h.state_mut().session.active_mut().unwrap().undo.clear();
        pointer(&mut h, end, false);
        h.run_steps(2);
        let d = h.state().session.active().unwrap();
        assert!(d.undo.is_empty());
        if !new_document {
            assert!(d.wb.active().unwrap().pivots[0].columns.is_empty());
        }
    }
}

#[test]
fn calculated_summary_cannot_be_dropped_into_rows() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.calculatedField", json!({"name":"Double sales","formula":"=Sales*2"})).unwrap();
    h.run_steps(3);
    let caption = h.state().session.active().unwrap().wb.active().unwrap().pivots[0].values.last().unwrap().name.clone();
    let original = serde_json::to_value(&h.state().session.active().unwrap().wb.active().unwrap().pivots[0]).unwrap();
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let start = button(&h, &caption);
    let end = button(&h, "Region");
    drag(&mut h, start, end);
    let d = h.state().session.active().unwrap();
    assert_eq!(serde_json::to_value(&d.wb.active().unwrap().pivots[0]).unwrap(), original);
    assert!(d.undo.is_empty());
    assert!(h.state().message.is_none());
}

#[test]
fn all_cross_area_directions_work() {
    for from in ["rows", "columns", "filters", "values"] {
        for to in ["Rows", "Columns", "Filters", "Values"] {
            if from == to.to_lowercase() {
                continue;
            }
            let mut h = harness();
            if from != "rows" {
                h.state_mut().session.execute("pivot.moveField", json!({"field":"Region","from":"rows","to":from})).unwrap();
                h.run_steps(3);
            }
            let caption = if from == "values" { "Count of Region" } else { "Region" };
            let start = button(&h, caption);
            let target = if to == "Values" {
                let r = h.get_by_label("Sum of Sales ▾").rect();
                pos2(r.center().x, r.top() + 1.0)
            } else {
                empty_area(&h, to)
            };
            drag(&mut h, start, target);
            let fields = h.state_mut().session.run("pivot.fields", json!({})).unwrap();
            assert!(fields[to.to_lowercase()].as_array().unwrap().iter().any(|f| f["field"] == "Region"), "{from} -> {to}");
            assert!(h.state().message.is_none(), "{from} -> {to}");
        }
    }
}

#[test]
fn source_checkboxes_keep_their_click_behaviour() {
    let mut h = harness();
    h.get_by_label("Product").click();
    h.run_steps(3);
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows[1].source_col, "Product");
    h.get_by_label("Product").click();
    h.run_steps(3);
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.len(), 1);
    assert_eq!(pt.rows[0].source_col, "Region");
}

#[test]
fn dropping_source_before_its_existing_neighbour_does_not_move_it_past_that_neighbour() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Product","area":"rows"})).unwrap();
    h.run_steps(3);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let start = h.get_by_label("Region").rect().center();
    let neighbour = h.get_by_label("Product ▾").rect();
    drag(&mut h, start, pos2(neighbour.center().x, neighbour.top() + 1.0));
    let d = h.state().session.active().unwrap();
    let pt = &d.wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Product"]);
    assert!(d.undo.is_empty());
    let start = h.get_by_label("Region").rect().center();
    let neighbour = h.get_by_label("Product ▾").rect();
    drag(&mut h, start, pos2(neighbour.center().x, neighbour.bottom() + 3.0));
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Product", "Region"]);
}

#[test]
fn moving_a_summary_to_its_existing_categorical_role_removes_only_the_summary() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Product","area":"rows"})).unwrap();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Region","area":"values"})).unwrap();
    h.run_steps(3);
    let start = button(&h, "Count of Region");
    let neighbour = h.get_by_label("Product ▾").rect();
    drag(&mut h, start, pos2(neighbour.center().x, neighbour.top() + 1.0));
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Product"]);
    assert_eq!(pt.values.len(), 1);
    assert_eq!(pt.values[0].source_col, "Sales");
}

#[test]
fn a_short_window_keeps_the_pane_footer_above_the_sheet_tabs() {
    let mut h = harness();
    h.set_size(vec2(1000.0, 600.0));
    h.run_steps(5);
    let bottom = h.state().grid.rect.unwrap().bottom();
    assert!(h.get_by_label("Refresh").rect().bottom() <= bottom);
    assert!(h.get_by_label("Calculated Field…").rect().bottom() <= bottom);
}

#[test]
fn holding_a_drag_near_the_area_edge_scrolls_to_later_fields() {
    let mut h = harness();
    for index in 0..10 {
        h.state_mut().session.execute("pivot.addField", json!({"field":"Sales","area":"values","name":format!("Extra {index}")})).unwrap();
    }
    h.run_steps(4);
    let start = button(&h, "Sum of Sales");
    let edge = pos2(start.x, h.get_by_label("Refresh").rect().top() - 20.0);
    pointer(&mut h, start, true);
    h.input_mut().events.push(Event::PointerMoved(start + vec2(12.0, 0.0)));
    h.step();
    assert!(egui::DragAndDrop::has_any_payload(&h.ctx));
    h.input_mut().events.push(Event::PointerMoved(edge));
    h.step();
    h.run_steps(90);
    pointer(&mut h, edge, false);
    h.run_steps(3);
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert!(pt.values.iter().position(|v| v.name == "Sum of Sales").unwrap() > 4);
    assert_eq!(pt.values.len(), 11);
}

#[test]
fn virtual_values_moves_between_axes_but_not_into_values_or_outside() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Sales","area":"values","name":"Second sales"})).unwrap();
    h.run_steps(3);
    assert!(h.query_by_label("Σ Values ▾").is_some());
    let start = button(&h, "Σ Values");
    let end = empty_area(&h, "Rows");
    drag(&mut h, start, end);
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert!(pt.values_on_rows);
    assert_eq!(pt.values.len(), 2);
    assert!(h.query_by_label("Σ Values ▾").is_some());
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let start = button(&h, "Σ Values");
    let end = empty_area(&h, "Values");
    drag(&mut h, start, end);
    let start = button(&h, "Σ Values");
    let end = empty_area(&h, "Filters");
    drag(&mut h, start, end);
    let start = button(&h, "Σ Values");
    drag(&mut h, start, pos2(400.0, 300.0));
    let d = h.state().session.active().unwrap();
    assert!(d.wb.active().unwrap().pivots[0].values_on_rows);
    assert_eq!(d.wb.active().unwrap().pivots[0].values.len(), 2);
    assert!(d.undo.is_empty());
    let start = button(&h, "Σ Values");
    let end = empty_area(&h, "Columns");
    drag(&mut h, start, end);
    assert!(!h.state().session.active().unwrap().wb.active().unwrap().pivots[0].values_on_rows);
}

#[test]
fn ordinary_row_drag_across_virtual_values_updates_its_visible_order() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Product","area":"rows"})).unwrap();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Sales","area":"values","name":"Second sales"})).unwrap();
    h.state_mut().session.execute("pivot.moveField", json!({"field":"Σ Values","from":"columns","to":"rows","position":0})).unwrap();
    h.run_steps(3);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let virtual_rect = h.get_by_label("Σ Values ▾").rect();
    let start = button(&h, "Region");
    drag(&mut h, start, pos2(virtual_rect.center().x, virtual_rect.top() + 1.0));
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Product"]);
    assert_eq!(pt.values_position, Some(1));
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let virtual_rect = h.get_by_label("Σ Values ▾").rect();
    let start = button(&h, "Region");
    drag(&mut h, start, pos2(virtual_rect.center().x, virtual_rect.bottom() - 1.0));
    let d = h.state().session.active().unwrap();
    assert_eq!(d.wb.active().unwrap().pivots[0].values_position, Some(0));
    assert_eq!(d.undo.len(), 1);
}

#[test]
fn source_drop_after_virtual_values_keeps_the_indicated_order_and_one_undo_step() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Product","area":"rows"})).unwrap();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Sales","area":"values","name":"Second sales"})).unwrap();
    h.state_mut().session.execute("pivot.moveField", json!({"field":"Σ Values","from":"columns","to":"rows","position":1})).unwrap();
    h.run_steps(3);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let start = h.get_by_label("Units").rect().center();
    let target = h.get_by_label("Σ Values ▾").rect();
    drag(&mut h, start, pos2(target.center().x, target.bottom() - 1.0));
    let d = h.state().session.active().unwrap();
    let pt = &d.wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Units", "Product"]);
    assert_eq!(pt.values_position, Some(1), "the dropped field must follow Σ Values");
    assert_eq!(d.undo.len(), 1);
    if let Ok(path) = std::env::var("GRIDCRAFT_PIVOT_DROP_SCREENSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Product"]);
    assert_eq!(pt.values_position, Some(1));
    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Units", "Product"]);
    assert_eq!(pt.values_position, Some(1));
}

#[test]
fn cross_area_drop_after_virtual_values_keeps_the_indicated_order_and_one_undo_step() {
    let mut h = harness();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Product","area":"rows"})).unwrap();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Sales","area":"values","name":"Second sales"})).unwrap();
    h.state_mut().session.execute("pivot.moveField", json!({"field":"Σ Values","from":"columns","to":"rows","position":1})).unwrap();
    h.state_mut().session.execute("pivot.addField", json!({"field":"Units","area":"columns"})).unwrap();
    h.run_steps(3);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    let start = button(&h, "Units");
    let target = h.get_by_label("Σ Values ▾").rect();
    drag(&mut h, start, pos2(target.center().x, target.bottom() - 1.0));
    let d = h.state().session.active().unwrap();
    let pt = &d.wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Units", "Product"]);
    assert_eq!(pt.values_position, Some(1), "the dropped field must follow Σ Values");
    assert_eq!(d.undo.len(), 1);
    h.state_mut().session.execute("edit.undo", json!({})).unwrap();
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Product"]);
    assert_eq!(pt.values_position, Some(1));
    h.state_mut().session.execute("edit.redo", json!({})).unwrap();
    let pt = &h.state().session.active().unwrap().wb.active().unwrap().pivots[0];
    assert_eq!(pt.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region", "Units", "Product"]);
    assert_eq!(pt.values_position, Some(1));
}
