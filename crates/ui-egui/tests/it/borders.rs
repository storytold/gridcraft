//! Border menu and dialog behavior through real egui controls.

use egui::{Color32, Shape, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{BorderLine, BorderStyle, Borders, Color};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.run("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    session.run("selection.set", json!({"range": "A1:B2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(6);
    h
}

fn borders(h: &egui_kittest::Harness<'static, SheetApp>, cell: &str) -> Borders {
    let doc = h.state().session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    doc.wb.styles.get(sheet.style_id(CellRef::parse(cell).unwrap())).borders
}

#[test]
fn diagram_menu_applies_left_right_inside_and_outside_then_undoes() {
    for label in ["Left Border", "Right Border", "Inside Borders", "Outside Borders"] {
        let mut h = harness();
        h.get_by_label("Border presets").click();
        h.run_steps(6);
        h.get_by_label(label).click();
        h.run_steps(6);
        let a = borders(&h, "A1");
        let b = borders(&h, "B2");
        match label {
            "Left Border" => {
                assert_eq!(a.left.style, BorderStyle::Thin);
                assert!(a.right.is_none() && b.left.is_none() && b.right.is_none());
            }
            "Right Border" => {
                assert_eq!(b.right.style, BorderStyle::Thin);
                assert!(a.left.is_none() && a.right.is_none() && b.left.is_none());
            }
            "Inside Borders" => {
                assert_eq!(a.right.style, BorderStyle::Thin);
                assert_eq!(a.bottom.style, BorderStyle::Thin);
                assert_eq!(b.left.style, BorderStyle::Thin);
                assert_eq!(b.top.style, BorderStyle::Thin);
                assert!(a.left.is_none() && a.top.is_none() && b.right.is_none() && b.bottom.is_none());
            }
            _ => {
                assert_eq!(a.left.style, BorderStyle::Thin);
                assert_eq!(a.top.style, BorderStyle::Thin);
                assert_eq!(b.right.style, BorderStyle::Thin);
                assert_eq!(b.bottom.style, BorderStyle::Thin);
                assert!(a.right.is_none() && a.bottom.is_none() && b.left.is_none() && b.top.is_none());
            }
        }
        let sheet = h.state().session.active().unwrap().wb.active().unwrap();
        assert_eq!(sheet.value(CellRef::new(0, 0)), Value::Number(1.0));
        assert_eq!(sheet.value(CellRef::new(1, 1)), Value::Number(4.0));
        h.state_mut().run("edit.undo", json!({})).unwrap();
        assert_eq!(borders(&h, "A1"), Borders::default());
        assert_eq!(borders(&h, "B2"), Borders::default());
    }
}

fn colored_borders() -> Borders {
    let line = |color| BorderLine { style: BorderStyle::Thin, color: Color::Rgb(color) };
    Borders {
        top: line(0xab192d),
        bottom: line(0x174faa),
        left: line(0x16864d),
        right: line(0x985213),
        diag_down: line(0x7c2ba2),
        diag_up: line(0x248088),
    }
}

fn preview_lines(h: &egui_kittest::Harness<'static, SheetApp>, color: Color32) -> usize {
    let preview = h.get_by_label("Border preview").rect();
    h.output()
        .shapes
        .iter()
        .filter(|shape| match &shape.shape {
            Shape::LineSegment { points, stroke } => stroke.color == color && points.iter().all(|p| preview.contains(*p)),
            _ => false,
        })
        .count()
}

#[test]
fn border_preview_uses_the_colored_draft_until_ok_and_cancel_discards_it() {
    let mut h = harness();
    let original = colored_borders();
    h.state_mut().run("home.formatCells", json!({"style": {"borders": original}})).unwrap();
    h.state_mut().open_dialog("formatCells", json!({"tab": "Border"}));
    h.run_steps(6);
    for line in [original.top, original.bottom, original.left, original.right, original.diag_down, original.diag_up] {
        let [r, g, b] = line.color.resolve(&Default::default()).unwrap();
        assert_eq!(preview_lines(&h, Color32::from_rgb(r, g, b)), 1);
    }
    h.get_by_label("Top double").click();
    h.run_steps(6);
    assert_eq!(borders(&h, "A1"), original);
    assert_eq!(preview_lines(&h, Color32::from_rgb(0xab, 0x19, 0x2d)), 2);
    h.get_by_label(" Cancel ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(borders(&h, "A1"), original);
    h.state_mut().open_dialog("formatCells", json!({"tab": "Border"}));
    h.run_steps(6);
    h.get_by_label("Top double").click();
    h.run_steps(6);
    h.get_by_label("   OK   ").click();
    h.run_steps(6);
    assert!(h.state().dialog.is_none());
    assert_eq!(borders(&h, "A1").top.style, BorderStyle::Double);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(borders(&h, "A1"), original);
}

/// #376: a merged range's outline lives on the cells along each edge (the right edge on the last
/// column's cells, the bottom on the last row's), not on the anchor alone. All four sides are drawn.
#[test]
fn merged_range_draws_the_outline_from_its_edge_cells() {
    let mut h = harness();
    h.state_mut().run("home.borders", json!({"range": "B2:E5", "preset": "thickOutside", "color": "#C81E1E"})).unwrap();
    h.state_mut().run("home.mergeCells", json!({"range": "B2:E5"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "H10"})).unwrap();
    h.run_steps(6);
    assert!(borders(&h, "B2").right.is_none(), "the anchor carries only the top and left edges");
    let red = Color32::from_rgb(0xC8, 0x1E, 0x1E);
    let segs: Vec<[egui::Pos2; 2]> = h
        .output()
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::LineSegment { points, stroke } if stroke.color == red => Some(*points),
            _ => None,
        })
        .collect();
    let vertical = |s: &[egui::Pos2; 2]| (s[0].x - s[1].x).abs() < 0.5;
    let xs: Vec<f32> = segs.iter().filter(|s| vertical(s)).map(|s| s[0].x).collect();
    let ys: Vec<f32> = segs.iter().filter(|s| !vertical(s)).map(|s| s[0].y).collect();
    let (min_x, max_x) = xs.iter().fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    let (min_y, max_y) = ys.iter().fold((f32::MAX, f32::MIN), |(a, b), y| (a.min(*y), b.max(*y)));
    // Left and right edges (four rows each), top and bottom edges (four columns each).
    assert_eq!(xs.iter().filter(|x| (**x - min_x).abs() < 0.5).count(), 4, "left edge");
    assert_eq!(xs.iter().filter(|x| (**x - max_x).abs() < 0.5).count(), 4, "right edge");
    assert_eq!(ys.iter().filter(|y| (**y - min_y).abs() < 0.5).count(), 4, "top edge");
    assert_eq!(ys.iter().filter(|y| (**y - max_y).abs() < 0.5).count(), 4, "bottom edge");
    assert!(max_x - min_x > 100.0 && max_y - min_y > 40.0, "the outline spans the whole merged area");
}
