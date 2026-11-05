//! x14 data bars and icon sets (#703): the pure geometry of negative bars (which side of the
//! axis they grow to) and which shape an icon set draws.

use egui::{Rect, pos2};
use gridcraft_engine::cf::BarLook;
use gridcraft_ui_egui::grid::{IconShape, bar_geometry, cf_icon_shape};

/// A cell 60x20 at (100, 50): drawable width 56 after the 2px insets, so an axis at 1/3 sits at
/// x = 102 + 56/3.
const CELL: Rect = Rect { min: pos2(100.0, 50.0), max: pos2(160.0, 70.0) };
const AXIS_X: f32 = 102.0 + 56.0 / 3.0;

fn look(frac: f32, axis_frac: f32, axis: Option<[u8; 3]>) -> BarLook {
    BarLook { frac, axis_frac, color: [0x63, 0x8E, 0xC6], gradient: true, border: None, negative_fill: [0xFF, 0x00, 0x00], axis }
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn negative_bars_grow_left_of_the_axis() {
    let (bar, axis_x) = bar_geometry(CELL, &look(-0.5, 1.0 / 3.0, Some([0x80, 0x80, 0x80])));
    let axis_x = axis_x.expect("an axis colour asks for the axis line");
    assert!(close(axis_x, AXIS_X));
    assert!(close(bar.right(), axis_x), "a negative bar ends at the axis");
    assert!(close(bar.left(), axis_x - 0.5 * (axis_x - 102.0)), "and covers half of the left side");
    assert!(bar.left() >= CELL.left(), "never past the cell's left edge");
    assert_eq!((bar.top(), bar.height()), (CELL.top() + 2.0, CELL.height() - 4.0));
}

#[test]
fn positive_bars_start_at_the_axis() {
    let (bar, axis_x) = bar_geometry(CELL, &look(0.75, 1.0 / 3.0, None));
    assert!(axis_x.is_none(), "no axis colour, no axis line");
    assert!(close(bar.left(), AXIS_X), "a positive bar starts at the axis");
    assert!(close(bar.width(), 0.75 * (158.0 - AXIS_X)), "and covers 3/4 of the right side");
    assert!(bar.right() <= CELL.right() - 2.0, "never past the cell's right edge");
    // The geometry anchors the bar even when the axis line itself is hidden.
    let (hidden, axis_x) = bar_geometry(CELL, &look(0.75, 1.0 / 3.0, None));
    assert_eq!((hidden, axis_x), (bar, None));
}

#[test]
fn one_sided_bounds_park_the_axis_on_an_edge() {
    let (positive, axis_x) = bar_geometry(CELL, &look(0.9, 0.0, Some([0, 0, 0])));
    assert!(close(axis_x.unwrap(), 102.0), "all-positive bounds park the axis left");
    assert!(close(positive.left(), 102.0) && close(positive.width(), 0.9 * 56.0));
    let (negative, axis_x) = bar_geometry(CELL, &look(-0.74, 1.0, Some([0, 0, 0])));
    assert!(close(axis_x.unwrap(), 158.0), "all-negative bounds park the axis right");
    assert!(close(negative.right(), 158.0) && close(negative.width(), 0.74 * 56.0));
}

#[test]
fn border_colour_does_not_change_the_geometry() {
    // Stroking is the caller's concern: the rect and axis x depend only on the fractions and
    // the axis colour, never on `border`.
    let plain = look(0.5, 0.5, Some([9, 9, 9]));
    let bordered = BarLook { border: Some([0, 0, 0]), ..plain.clone() };
    assert_eq!(bar_geometry(CELL, &plain), bar_geometry(CELL, &bordered));
}

#[test]
fn icon_sets_map_to_shapes() {
    let shape = |set: &str| cf_icon_shape(set);
    assert_eq!(shape("3Arrows"), IconShape::Arrow);
    assert_eq!(shape("4ArrowsGray"), IconShape::Arrow);
    assert_eq!(shape("3TrafficLights1"), IconShape::TrafficLight);
    assert_eq!(shape("4Rating"), IconShape::Rating);
    assert_eq!(shape("5Quarters"), IconShape::Quarter);
    assert_eq!(shape("3Triangles"), IconShape::Triangle);
    assert_eq!(shape("3Stars"), IconShape::Star);
    assert_eq!(shape("5Boxes"), IconShape::Box);
    assert_eq!(shape("3Symbols"), IconShape::Dot, "unknown families fall back to a dot");
}
