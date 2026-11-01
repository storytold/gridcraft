//! A Line or Arrow keeps the direction it was drawn in: `insert.shape`'s `flipH`/`flipV`, Ink to
//! Shape, and an XLSX save and reopen (#2904).

use gridcraft_engine::Session;
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
    reopened.execute("file.open", json!({"name": "lines.xlsx", "base64": saved["base64"]})).unwrap();
    reopened
}

fn last(session: &Session) -> Shape {
    session.doc().unwrap().wb.active().unwrap().shapes.last().cloned().unwrap()
}

#[test]
fn inserted_lines_keep_their_direction_through_a_reopen() {
    let mut session = workbook();
    session.execute("insert.shape", json!({"kind": "arrow", "at": "C4", "dx": 5, "dy": 7, "width": 0, "height": 90, "flipV": true})).unwrap();
    let s = last(&session);
    assert_eq!((s.anchor.dx, s.anchor.dy, s.anchor.width, s.anchor.height), (5.0, 7.0, 0.0, 90.0), "a vertical arrow is 0 wide");
    assert_eq!(s.line_ends(), ([0.0, 90.0], [0.0, 0.0]), "flipV: drawn upward, head at the top");
    session.execute("insert.shape", json!({"kind": "line", "at": "F2", "width": 100, "height": 30, "flipH": true})).unwrap();
    let reopened = reopen(&mut session);
    let shapes = &reopened.doc().unwrap().wb.active().unwrap().shapes;
    assert_eq!((shapes[0].kind, shapes[0].flip_h, shapes[0].flip_v), (ShapeKind::Arrow, false, true));
    assert_eq!((shapes[1].kind, shapes[1].flip_h, shapes[1].flip_v), (ShapeKind::Line, true, false));
    assert!(shapes[0].anchor.width.abs() < 0.01 && (shapes[0].anchor.height - 90.0).abs() < 0.5);
}

#[test]
fn hostile_shape_offsets_and_sizes_are_clamped() {
    let mut session = workbook();
    session.execute("insert.shape", json!({"kind": "line", "dx": -50, "dy": 1e300, "width": -10, "height": 1e300, "flipH": "yes"})).unwrap();
    let s = last(&session);
    assert_eq!((s.anchor.dx, s.anchor.dy, s.anchor.width, s.anchor.height), (0.0, 10_000.0, 0.0, 10_000.0));
    assert!(!s.flip_h, "a non-boolean flip is ignored");
    session.execute("insert.shape", json!({"kind": "rectangle", "width": 0})).unwrap();
    assert_eq!(last(&session).anchor.width, 1.0, "only lines may be zero wide");
}

#[test]
fn ink_to_shape_keeps_a_lines_stroke_direction() {
    for (points, flips) in [
        (vec![[300, 200], [250, 180], [100, 120]], (true, true)),
        (vec![[100, 200], [180, 150], [260, 90]], (false, true)),
        (vec![[100, 100], [180, 130], [260, 160]], (false, false)),
    ] {
        let mut session = workbook();
        session.execute("draw.stroke", json!({"points": points})).unwrap();
        assert_eq!(session.execute("draw.inkToShape", json!({})).unwrap()["kind"], "Line");
        let s = last(&session);
        assert_eq!((s.flip_h, s.flip_v), flips, "{points:?}");
    }
}
