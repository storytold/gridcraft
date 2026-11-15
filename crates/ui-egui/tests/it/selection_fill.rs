//! The selection's fill leaves the active cell clear, so it is plain where typing goes.

use egui::Rect;
use gridcraft_engine::Session;
use gridcraft_engine::core::RangeRef;
use gridcraft_ui_egui::grid::{Geo, selection_fill};
use serde_json::json;

fn session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

fn geo(session: &Session) -> Geo {
    let sheet = session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1200.0, 800.0)), egui::Vec2::ZERO)
}

/// Where a cell or range is on screen.
fn cells(session: &Session, a1: &str) -> Rect {
    let sheet = session.active().unwrap().wb.active().unwrap();
    geo(session).range_rect(sheet, RangeRef::parse(a1).unwrap())
}

/// The fill at rest: the current area's outline is on its cells.
fn fill(session: &Session) -> Vec<Rect> {
    let doc = session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    let geo = geo(session);
    selection_fill(&geo, sheet, &doc.selection, geo.range_rect(sheet, doc.selection.current()))
}

fn shaded(fill: &[Rect], cell: Rect) -> bool {
    fill.iter().any(|part| part.intersects(cell.shrink(1.0)))
}

fn area(fill: &[Rect]) -> f32 {
    fill.iter().map(Rect::area).sum()
}

#[test]
fn the_active_cell_of_a_range_is_not_shaded() {
    let mut s = session();
    // At the corner where a drag began, then wherever Tab and Enter move it inside the range.
    for active in ["B2", "C4", "D5"] {
        s.execute("selection.set", json!({"range": "B2:D5", "active": active})).unwrap();
        let fill = fill(&s);
        assert!(!shaded(&fill, cells(&s, active)), "{active} stays clear: {fill:?}");
        for other in ["B2", "B5", "C3", "C4", "D2", "D5"].into_iter().filter(|c| *c != active) {
            assert!(shaded(&fill, cells(&s, other)), "{other} is shaded with {active} active");
        }
        assert!((area(&fill) - (cells(&s, "B2:D5").area() - cells(&s, active).area())).abs() < 0.5, "the rest of the range, once");
    }
}

#[test]
fn a_single_selected_cell_has_no_fill() {
    let mut s = session();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    assert!(fill(&s).is_empty());
}

#[test]
fn other_areas_are_shaded_whole_and_a_lone_active_cell_not_at_all() {
    let mut s = session();
    s.execute("selection.set", json!({"range": "A1:A3,C2:D3,F6", "active": "F6"})).unwrap();
    assert_eq!(s.active().unwrap().selection.ranges.len(), 3);
    let fill = fill(&s);
    assert!(!shaded(&fill, cells(&s, "F6")));
    assert!(shaded(&fill, cells(&s, "A2")) && shaded(&fill, cells(&s, "D3")));
    assert!((area(&fill) - (cells(&s, "A1:A3").area() + cells(&s, "C2:D3").area())).abs() < 0.5, "{fill:?}");
}

#[test]
fn a_merged_active_cell_is_left_clear_as_a_whole() {
    let mut s = session();
    s.execute("selection.set", json!({"range": "B2:C3"})).unwrap();
    s.execute("home.mergeCells", json!({})).unwrap();
    s.execute("selection.set", json!({"range": "B2:E6", "active": "B2"})).unwrap();
    let fill = fill(&s);
    assert!(!shaded(&fill, cells(&s, "B2:C3")), "the merged cell, not just its first cell: {fill:?}");
    assert!(shaded(&fill, cells(&s, "D2")) && shaded(&fill, cells(&s, "B4")) && shaded(&fill, cells(&s, "E6")));
    assert!((area(&fill) - (cells(&s, "B2:E6").area() - cells(&s, "B2:C3").area())).abs() < 0.5);
}

#[test]
fn the_current_area_is_shaded_where_its_gliding_outline_is() {
    let mut s = session();
    s.execute("selection.set", json!({"range": "A1:A3,C2:D3", "active": "C2"})).unwrap();
    let doc = s.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    // Mid-glide the outline of the current area (C2:D3) is somewhere else on screen.
    let outline = Rect::from_min_max(egui::pos2(600.0, 400.0), egui::pos2(700.0, 500.0));
    let fill = selection_fill(&geo(&s), sheet, &doc.selection, outline);
    assert!(fill.contains(&outline), "the current area follows its outline: {fill:?}");
    assert!(shaded(&fill, cells(&s, "A2")), "the other areas stay on their cells");
    assert!(!shaded(&fill, cells(&s, "D3")), "nothing is left behind on the current area's cells");
}
