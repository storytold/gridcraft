//! Sorting from a filter popup targets that filter, even when another region is selected (#1097).

use egui::{Event, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

fn value(app: &SheetApp, cell: &str) -> Value {
    app.session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

#[test]
fn popup_sorts_the_clicked_filter_without_moving_headers_or_table_totals() {
    check_popup_sort(false);
}

#[test]
fn enter_on_a_sort_button_preserves_the_clicked_filter_header_and_table_totals() {
    check_popup_sort(true);
}

fn check_popup_sort(press_enter: bool) {
    for table in [false, true] {
        for descending in [false, true] {
            let mut session = Session::new();
            session.new_workbook();
            session.execute("cell.set", json!({"cell":"A1", "input":"Summary"})).unwrap();
            session
                .execute("range.setValues", json!({"range":"A2", "values":[["ID","Company","Amount"],[1,"Zeta",10],[2,"Alpha",20],[3,"Gamma",30]]}))
                .unwrap();
            if table {
                session.execute("insert.table", json!({"range":"A2:C5", "header":true, "name":"Payments"})).unwrap();
                session.execute("table.totalRow", json!({"table":"Payments", "on":true})).unwrap();
            } else {
                session.execute("data.filter", json!({"range":"A2:C5"})).unwrap();
            }
            session.execute("range.setValues", json!({"range":"H1", "values":[["Other", "Value"],["z",2],["a",1]]})).unwrap();
            session.execute("insert.table", json!({"range":"H1:I3", "header":true, "name":"Other"})).unwrap();
            session.execute("selection.set", json!({"cell":"H2"})).unwrap();
            let app = SheetApp::new(session, Default::default());
            let totals: Vec<_> = ["A6", "B6", "C6"].map(|cell| value(&app, cell)).into();
            let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
                |ui, app: &mut SheetApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
            h.run_steps(4);
            let button = {
                let app = h.state();
                let sh = app.session.active().unwrap().wb.active().unwrap();
                let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
                grid::filter_button_rect(&geo, sh, CellRef::parse("B2").unwrap()).center()
            };
            h.input_mut().events.push(Event::PointerMoved(button));
            for pressed in [true, false] {
                h.input_mut().events.push(Event::PointerButton { pos: button, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            }
            h.run_steps(4);
            assert!(h.state().grid.filter_menu.is_some(), "the clicked filter popup opens");
            let label = if descending { "↓  Sort Descending" } else { "↑  Sort Ascending" };
            if press_enter {
                h.get_by_label(label).focus();
                h.run_steps(2);
                h.key_press(egui::Key::Enter);
            } else {
                h.get_by_label(label).click();
            }
            h.run_steps(4);

            assert!(h.state().grid.filter_menu.is_none(), "sorting closes the popup");
            assert_eq!(value(h.state(), "A1"), Value::from("Summary"));
            assert_eq!(value(h.state(), "B2"), Value::from("Company"));
            assert_eq!(value(h.state(), "B3"), Value::from(if descending { "Zeta" } else { "Alpha" }), "table={table}, descending={descending}");
            assert_eq!(value(h.state(), "B5"), Value::from(if descending { "Alpha" } else { "Zeta" }));
            assert_eq!(value(h.state(), "H2"), Value::from("z"), "the selected table must not be sorted");
            assert_eq!(value(h.state(), "H3"), Value::from("a"));
            assert_eq!(["A6", "B6", "C6"].map(|cell| value(h.state(), cell)).as_slice(), totals.as_slice());
        }
    }
}
