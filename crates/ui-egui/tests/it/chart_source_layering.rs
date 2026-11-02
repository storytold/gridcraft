//! #2861: a selected chart's source outline belongs behind floating objects, in cell space.

use egui::{Color32, Rect, Shape, pos2, vec2};
use gridcraft_engine::core::RangeRef;
use gridcraft_engine::model::Anchor;
use gridcraft_ui_egui::{SheetApp, theme::Tokens};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const SOURCE: &str = "B2:B12";
const SHAPE_FILL: Color32 = Color32::from_rgb(0x12, 0x34, 0x56);
const CELL_FILL: Color32 = Color32::from_rgb(0xEF, 0xDC, 0xBA);

/// The source range crosses a shape and its selected chart, while its top remains uncovered.
fn fixture(rtl: bool, frozen: bool) -> (Harness, u32) {
    let mut session = crate::support::blank_session();
    let values: Vec<_> = (1..=11).map(|value| json!([value])).collect();
    session.execute("range.setValues", json!({"range": "B2", "values": values})).unwrap();
    session.execute("home.fillColor", json!({"range": SOURCE, "color": "#EFDCBA"})).unwrap();
    session.execute("selection.set", json!({"range": SOURCE})).unwrap();
    let chart = session.execute("insert.chart", json!({"type": "column", "at": "B7", "width": 260, "height": 130})).unwrap();
    let id = chart["chart"].as_u64().unwrap() as u32;
    session.execute("insert.shape", json!({"kind": "rectangle", "at": "B4", "width": 160, "height": 35, "fill": "123456"})).unwrap();
    session.execute("selection.set", json!({"cell": "A1"})).unwrap();
    session.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
    if frozen {
        session.execute("view.freezePanes", json!({"cell": "A2"})).unwrap();
    }
    let mut h = crate::support::harness(session, 4);
    h.state_mut().selected_chart = Some(id);
    if frozen {
        h.state_mut().view_mut().unwrap().scroll.y = 20.0;
    }
    h.run_steps(3);
    (h, id)
}

/// The exact on-screen source rectangle and its pane clips, including RTL and frozen rows.
fn source_geometry(h: &Harness) -> (Rect, Vec<Rect>) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = app.grid.geo.as_ref().unwrap();
    let range = RangeRef::parse(SOURCE).unwrap();
    let rect = geo.mirror_rect(geo.range_rect(sheet, range));
    let clips = geo.range_panes(range).into_iter().map(|pane| geo.mirror_rect(pane.intersect(geo.cells))).collect();
    (rect, clips)
}

/// Allows only floating-point rounding when RTL translation and reflection reach the same bounds.
fn same_rect(a: Rect, b: Rect) -> bool {
    a.min.distance(b.min) < 0.01 && a.max.distance(b.max) < 0.01
}

/// The last frame's actual source-outline paint commands, with their order and visible clip.
fn source_strokes(h: &Harness) -> Vec<(usize, Rect)> {
    let (source, _) = source_geometry(h);
    let color = Tokens::get(&h.ctx).ref_color(0, true);
    h.output()
        .shapes
        .iter()
        .enumerate()
        .filter_map(|(index, shape)| match &shape.shape {
            Shape::Rect(rect) if same_rect(rect.rect, source) && rect.stroke.color == color && rect.stroke.width == 1.5 => {
                Some((index, shape.clip_rect))
            }
            _ => None,
        })
        .collect()
}

/// Floating objects move as a whole on RTL sheets, so their on-screen bounds remain unflipped.
fn object_rect(h: &Harness, anchor: &Anchor) -> Rect {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = app.grid.geo.as_ref().unwrap();
    geo.mirror_rect(Rect::from_min_size(
        pos2(geo.x(sheet, anchor.cell.col) + anchor.dx * geo.z, geo.y(sheet, anchor.cell.row) + anchor.dy * geo.z),
        vec2(anchor.width * geo.z, anchor.height * geo.z),
    ))
}

/// Finds an opaque object fill in final paint order, rather than relying on implementation calls.
fn fill_index(h: &Harness, bounds: Rect, color: Color32) -> usize {
    h.output()
        .shapes
        .iter()
        .position(|shape| matches!(&shape.shape, Shape::Rect(rect) if same_rect(rect.rect, bounds) && rect.fill == color))
        .expect("the overlapping floating object's fill was painted")
}

/// Opaque objects must cover the overlapping source stroke while the uncovered stroke stays visible.
#[test]
fn chart_source_outlines_stay_below_every_overlapping_object() {
    for rtl in [false, true] {
        for frozen in [false, true] {
            let (h, id) = fixture(rtl, frozen);
            let sheet = h.state().session.active().unwrap().wb.active().unwrap();
            let chart = sheet.charts.iter().find(|chart| chart.id == id).unwrap();
            let chart_bounds = object_rect(&h, &chart.anchor);
            let shape_bounds = object_rect(&h, &sheet.shapes[0].anchor);
            let chart_fill = fill_index(&h, chart_bounds, Tokens::get(&h.ctx).grid_bg);
            let shape_fill = fill_index(&h, shape_bounds, SHAPE_FILL);
            let (source, expected_clips) = source_geometry(&h);
            let strokes = source_strokes(&h);
            assert_eq!(strokes.len(), expected_clips.len(), "source remains painted: rtl={rtl}, frozen={frozen}");
            for ((_, clip), expected) in strokes.iter().zip(expected_clips) {
                assert_eq!(*clip, expected, "source clips stay in cell space");
            }
            let visible_top = source.top().max(strokes[0].1.top());
            let x = if rtl { source.left() + 0.75 } else { source.right() - 0.75 };
            let uncovered = pos2(x, visible_top + 4.0);
            assert!(source.contains(uncovered) && strokes[0].1.contains(uncovered));
            assert!(!chart_bounds.contains(uncovered) && !shape_bounds.contains(uncovered), "uncovered source outline remains visible");
            let background = h
                .output()
                .shapes
                .iter()
                .enumerate()
                .find_map(|(index, shape)| match &shape.shape {
                    Shape::Rect(rect) if rect.fill == CELL_FILL && rect.rect.contains(uncovered) && shape.clip_rect.contains(uncovered) => {
                        Some(index)
                    }
                    _ => None,
                })
                .expect("the uncovered source cell has an opaque fill");
            assert!(strokes[0].0 > background, "source outline remains above cell backgrounds");
            for (name, bounds, fill) in [("chart", chart_bounds, chart_fill), ("shape", shape_bounds, shape_fill)] {
                let covered = pos2(x, bounds.center().y);
                assert!(source.contains(covered) && bounds.contains(covered), "fixture overlaps the {name}'s source stroke");
                for (stroke, _) in &strokes {
                    assert!(*stroke < fill, "source outline paints over {name}: rtl={rtl}, frozen={frozen}, stroke={stroke}, fill={fill}");
                }
            }
        }
    }
}

/// Moving the source pass must preserve deselection, hidden-object and offscreen-chart behavior.
#[test]
fn unselected_hidden_and_offscreen_charts_do_not_highlight_sources() {
    for state in ["unselected", "hidden", "offscreen"] {
        let (mut h, id) = fixture(false, false);
        assert!(!source_strokes(&h).is_empty(), "selected visible chart highlights its source");
        match state {
            "unselected" => h.state_mut().selected_chart = None,
            "hidden" => {
                h.state_mut().run("view.showObjects", json!({"on": false})).unwrap();
            }
            _ => {
                h.state_mut().run("object.move", json!({"kind": "chart", "id": id, "at": "B100"})).unwrap();
            }
        }
        h.run_steps(3);
        assert!(source_strokes(&h).is_empty(), "{state} chart must not highlight its source");
    }
}
