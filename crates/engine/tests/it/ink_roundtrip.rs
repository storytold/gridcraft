//! Native ink remains editable after XLSX saves, including a save after an unrelated cell edit.

use gridcraft_engine::Session;
use gridcraft_engine::cmd::draw::{ink_points, ink_width};
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_engine::model::{Shape, ShapeKind};
use serde_json::json;

fn workbook() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

fn reopen(session: &mut Session) -> Session {
    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "ink.xlsx", "base64": saved["base64"]})).unwrap();
    reopened
}

fn shapes(session: &Session) -> &[Shape] {
    &session.doc().unwrap().wb.active().unwrap().shapes
}

fn assert_anchor(actual: &Shape, expected: &Shape) {
    assert_eq!(actual.anchor.cell, expected.anchor.cell);
    assert_eq!(actual.anchor.mode, expected.anchor.mode);
    for (name, actual, expected) in [
        ("dx", actual.anchor.dx, expected.anchor.dx),
        ("dy", actual.anchor.dy, expected.anchor.dy),
        ("width", actual.anchor.width, expected.anchor.width),
        ("height", actual.anchor.height, expected.anchor.height),
    ] {
        assert!((actual - expected).abs() < 0.001, "anchor {name}: {actual} != {expected}");
    }
}

fn assert_ink(actual: &Shape, expected: &Shape) {
    assert_eq!(actual.kind, ShapeKind::Ink);
    let actual_points = ink_points(actual);
    let expected_points = ink_points(expected);
    assert_eq!(actual_points.len(), expected_points.len());
    for (actual, expected) in actual_points.iter().zip(&expected_points) {
        for axis in 0..2 {
            // Native ink stores coordinates in InkML's 1/1000 cm units: 1/26.46 px, so a point
            // lands within 0.02 px of where it was drawn (well under a device pixel).
            assert!((actual[axis] - expected[axis]).abs() < 0.05, "point {actual:?} != {expected:?}");
        }
    }
    assert_eq!(actual.line, expected.line);
    assert_eq!(ink_width(actual), ink_width(expected));
    assert_anchor(actual, expected);
}

#[test]
fn ink_geometry_and_style_survive_two_saves_and_a_cell_edit() {
    let mut session = workbook();
    session
        .execute("draw.stroke", json!({"points": [[130.5, 45.25], [172.5, 86.25], [151.5, 112.25], [215.5, 59.25]], "color": "#A12BCD", "width": 3}))
        .unwrap();
    session
        .execute("draw.stroke", json!({"points": [[300, 180], [360, 180], [360, 230], [300, 230], [300, 180]], "color": "#16843B", "width": 7}))
        .unwrap();
    let expected = shapes(&session).to_vec();
    assert_eq!(expected.len(), 2);

    for pass in 0..2 {
        session = reopen(&mut session);
        assert_eq!(shapes(&session).len(), expected.len());
        for (actual, expected) in shapes(&session).iter().zip(&expected) {
            assert_ink(actual, expected);
        }
        if pass == 0 {
            session.execute("cell.set", json!({"cell": "Z30", "input": "42"})).unwrap();
        }
    }
    assert_eq!(session.doc().unwrap().wb.active().unwrap().value(CellRef::parse("Z30").unwrap()), Value::Number(42.0));
}

#[test]
fn reopened_ink_can_still_be_converted_to_a_shape() {
    for (points, expected_kind, expected_name) in [
        (vec![[100, 100], [200, 100], [200, 180], [100, 180], [100, 100]], ShapeKind::Rectangle, "Rectangle"),
        (vec![[100, 100], [140, 110], [220, 130]], ShapeKind::Line, "Line"),
    ] {
        let mut session = workbook();
        session.execute("draw.stroke", json!({"points": points, "color": "#A12BCD", "width": 5})).unwrap();
        let original = shapes(&session)[0].clone();
        session = reopen(&mut session);
        let id = shapes(&session)[0].id;
        let converted = session.execute("draw.inkToShape", json!({"id": id})).unwrap();
        assert_eq!(converted["kind"], expected_name);
        let shape = &shapes(&session)[0];
        assert_eq!(shape.kind, expected_kind);
        assert_eq!(shape.line, original.line);
        assert!(shape.text.is_empty());
        assert_anchor(shape, &original);
    }
}

#[test]
fn ordinary_shape_text_is_not_mistaken_for_ink_points() {
    let mut session = workbook();
    session
        .execute(
            "insert.shape",
            json!({"kind": "rectangle", "at": "D6", "width": 120, "height": 60, "fill": "#112233", "line": "#A12BCD", "text": "0.0,0.0;50.0,20.0"}),
        )
        .unwrap();
    let original = shapes(&session)[0].clone();
    for _ in 0..2 {
        session = reopen(&mut session);
        assert_eq!(shapes(&session).len(), 1);
        let shape = &shapes(&session)[0];
        assert_eq!(shape.kind, ShapeKind::Rectangle);
        assert_eq!(shape.text, original.text);
        assert_eq!(shape.fill, original.fill);
        assert_eq!(shape.line, original.line);
        assert_anchor(shape, &original);
        assert!(session.execute("draw.inkToShape", json!({})).is_err());
    }
}

#[test]
fn horizontal_vertical_and_signed_highlighter_strokes_survive_reopening() {
    for points in [vec![[25.1, 50.2], [140.3, 50.2]], vec![[40.2, 10.1], [40.2, 100.3]], vec![[-12.3, -4.2], [20.1, 10.2], [80.3, -1.4]]] {
        let mut session = workbook();
        session.execute("draw.stroke", json!({"points": points, "color": "#16843B", "width": 12})).unwrap();
        let expected = shapes(&session)[0].clone();
        for _ in 0..2 {
            session = reopen(&mut session);
            assert_ink(&shapes(&session)[0], &expected);
        }
    }
}

/// A stroke keeps its brush, colour, width, pressure and creation key through a save and reopen.
#[test]
fn engine_strokes_keep_their_ink_style_and_key_across_save_and_open() {
    let mut session = workbook();
    session.execute("draw.stroke", json!({"points": [[130.5, 45.25], [172.5, 86.25], [151.5, 112.25]], "color": "#A12BCD", "width": 3})).unwrap();
    let before = shapes(&session)[0].clone();
    let ink = before.ink.as_ref().expect("the engine creates native ink");
    assert_eq!(ink.tool, gridcraft_engine::model::InkTool::Pen);
    assert!(ink.key.as_deref().is_some_and(|k| k.starts_with('{') && k.ends_with('}')), "{:?}", ink.key);
    assert!(ink.width_cm > 0.0);

    let reopened = reopen(&mut session);
    // No fallback picture leaks in as a separate object.
    assert!(reopened.doc().unwrap().wb.active().unwrap().images.is_empty());
    let after = &shapes(&reopened)[0];
    let after_ink = after.ink.as_ref().expect("editable ink after reopen");
    assert_eq!(after_ink.tool, ink.tool);
    assert_eq!(after_ink.width_cm, ink.width_cm);
    assert_eq!(after_ink.height_cm, ink.height_cm);
    assert_eq!(after_ink.effect, ink.effect);
    assert_eq!(after_ink.order, ink.order);
    assert_eq!(after_ink.key, ink.key, "the key is assigned once and survives saves");
    assert_eq!(after.line, before.line);
}

/// A stroke drawn after reopening mints a key that no stroke already in the file holds.
#[test]
fn a_stroke_drawn_in_a_reopened_workbook_takes_a_key_no_other_stroke_holds() {
    let mut session = workbook();
    session.execute("draw.stroke", json!({"points": [[100.0, 100.0], [140.0, 150.0]], "width": 3})).unwrap();
    let first_key = shapes(&session)[0].ink.as_ref().and_then(|i| i.key.clone()).expect("the first stroke has a key");
    session = reopen(&mut session);
    assert_eq!(shapes(&session)[0].ink.as_ref().and_then(|i| i.key.clone()), Some(first_key.clone()), "a save keeps the stroke's key");
    session.execute("draw.stroke", json!({"points": [[300.0, 120.0], [360.0, 170.0]], "width": 3})).unwrap();
    let keys: Vec<String> = shapes(&session).iter().filter_map(|s| s.ink.as_ref()?.key.clone()).collect();
    assert_eq!(keys.len(), 2, "{keys:?}");
    assert_ne!(keys[0], keys[1], "two strokes share a creation id: {keys:?}");
    // The second save writes both, and reopening still tells them apart.
    session = reopen(&mut session);
    let keys: std::collections::HashSet<String> = shapes(&session).iter().filter_map(|s| s.ink.as_ref()?.key.clone()).collect();
    assert_eq!(keys.len(), 2, "the keys stay distinct after another save");
}

/// A copied sheet gives its strokes fresh keys, so the two sheets' strokes stay distinct.
#[test]
fn a_copied_sheet_rekeys_its_ink_strokes() {
    let mut session = workbook();
    session.execute("draw.stroke", json!({"points": [[100.0, 100.0], [140.0, 150.0]], "width": 3})).unwrap();
    let original = shapes(&session)[0].ink.as_ref().and_then(|i| i.key.clone()).expect("a stroke has a key");
    session.execute("sheet.move", json!({"sheet": 0, "to": 1, "copy": true})).unwrap();
    let keys: Vec<String> =
        session.doc().unwrap().wb.sheets.iter().flat_map(|s| s.shapes.iter()).filter_map(|x| x.ink.as_ref()?.key.clone()).collect();
    assert_eq!(keys.len(), 2, "{keys:?}");
    assert_eq!(keys[0], original, "the original keeps its key");
    assert_ne!(keys[0], keys[1], "the copy takes a key of its own");
    // The save writes both sheets, and no creation id appears twice.
    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "copy.xlsx", "base64": saved["base64"]})).unwrap();
    let keys: std::collections::HashSet<String> =
        reopened.doc().unwrap().wb.sheets.iter().flat_map(|s| s.shapes.iter()).filter_map(|x| x.ink.as_ref()?.key.clone()).collect();
    assert_eq!(keys.len(), 2, "the saved copy keeps two distinct creation ids");
}
