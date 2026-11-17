//! Numbers in the workbook's region (pt-BR writes "1,5"): dialogs read them, the grid paints them
//! and the status bar shows them with the region's decimal separator.

use egui::{Event, Key, Shape, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type H = egui_kittest::Harness<'static, SheetApp>;

/// A pt-BR workbook with `cells` typed in, `select` selected.
fn harness(cells: &[(&str, &str)], select: &str) -> H {
    let mut session = Session::new();
    session.new_workbook();
    let pt_br = gridcraft_engine::core::region::find("pt-BR").unwrap();
    session.set_region(pt_br);
    for (cell, input) in cells {
        session.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    session.execute("selection.set", json!({"range": select})).unwrap();
    let mut h = crate::common::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(6);
    h
}

fn number(h: &H, cell: &str) -> f64 {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap()).as_f64().unwrap()
}

#[test]
fn dialogs_read_numbers_in_the_workbook_region() {
    let mut h = harness(&[("B2", "1"), ("B5", "=B2*3+4")], "B5");
    // Goal Seek: "25,5" is 25.5, not an unreadable target that became 0.
    h.state_mut().open_dialog("goalSeek", json!({}));
    h.run_steps(4);
    let d = h.state_mut().dialog.as_mut().unwrap();
    d.values.insert("to".into(), json!("25,5"));
    d.values.insert("changing".into(), json!("B2"));
    h.run_steps(2);
    h.get_by_label("   OK   ").click();
    h.run_steps(3);
    assert!(h.state().dialog.is_none());
    let b2 = number(&h, "B2");
    assert!((b2 - 21.5 / 3.0).abs() < 1e-6, "B2*3+4 = 25,5 needs B2 = 7,1667, got {b2}");

    // A number field (Column Width) takes "12,5" typed into it.
    h.state_mut().open_dialog("columnWidth", json!({}));
    h.run_steps(6);
    for c in "12,5".chars() {
        h.input_mut().events.push(Event::Text(c.to_string()));
        h.run_steps(1);
    }
    h.key_press(Key::Enter);
    h.run_steps(6);
    assert!(h.state().dialog.is_none(), "Enter confirmed the dialog");
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    let chars = gridcraft_engine::cmd::format::points_to_chars(sheet.col_width(CellRef::parse("B5").unwrap().col) as f64);
    assert!((chars - 12.5).abs() < 0.2, "column B is 12,5 characters wide, got {chars}");
}

#[test]
fn general_numbers_paint_the_region_decimal() {
    let h = harness(&[("A1", "1,5")], "H20");
    let painted: Vec<String> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    assert!(painted.iter().any(|t| t == "1,5"), "A1 is painted as 1,5");
    assert!(!painted.iter().any(|t| t == "1.5"), "A1 is not painted with a point");
}

#[test]
fn status_bar_statistics_use_the_region_decimal() {
    let h = harness(&[("A1", "4"), ("A2", "5")], "A1:A2");
    // A whole sum shows no bare separator ("9," before) and the average its comma.
    assert!(h.query_by_label("Sum: 9").is_some(), "the sum is 9");
    assert!(h.query_by_label("Average: 4,5").is_some(), "the average is 4,5");
}
