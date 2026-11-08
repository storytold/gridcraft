//! Ink Replay reports the sheet's ink in drawing order, survives a save/reopen and never edits
//! the document.

use gridcraft_engine::Session;
use serde_json::json;

fn workbook() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

#[test]
fn replay_reports_strokes_in_draw_order() {
    let mut session = workbook();
    let strokes = [[[10.0f64, 10.0], [60.0, 20.0]], [[200.0, 10.0], [260.0, 30.0]], [[400.0, 10.0], [460.0, 40.0]]];
    let mut ids = Vec::new();
    for points in strokes {
        let r = session.execute("draw.stroke", json!({"points": points, "color": "#123456", "width": 3})).unwrap();
        ids.push(r["shape"].as_u64().unwrap());
    }

    let r = session.execute("draw.inkReplay", json!({})).unwrap();
    assert_eq!(r["replaying"], true);
    let list = r["strokes"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    for (i, stroke) in list.iter().enumerate() {
        assert_eq!(stroke["order"], i);
        assert_eq!(stroke["id"].as_u64(), Some(ids[i]));
        assert_eq!(stroke["points"], 2);
    }

    // Empty parameters toggle: the second call stops it.
    let r = session.execute("draw.inkReplay", json!({})).unwrap();
    assert_eq!(r["replaying"], false);
    assert!(r["strokes"].as_array().unwrap().is_empty());
}

#[test]
fn replay_needs_ink_and_ignores_hostile_params() {
    let mut session = workbook();
    assert!(session.execute("draw.inkReplay", json!({})).is_err());
    // Wrong types and unknown keys are ignored, not trusted.
    assert!(session.execute("draw.inkReplay", json!({"on": "true", "strokes": 1e308})).is_err());

    session.execute("draw.stroke", json!({"points": [[0, 0], [10, 10]]})).unwrap();
    assert_eq!(session.execute("draw.inkReplay", json!({"on": true})).unwrap()["replaying"], true);
    assert_eq!(session.execute("draw.inkReplay", json!({"on": false})).unwrap()["replaying"], false);
}

#[test]
fn replay_order_survives_save_and_reopen() {
    let mut session = workbook();
    for x in [10.0, 200.0, 400.0] {
        session.execute("draw.stroke", json!({"points": [[x, 10.0], [x + 40.0, 30.0]]})).unwrap();
    }
    let saved = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();

    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name": "ink.xlsx", "base64": saved})).unwrap();
    let r = reopened.execute("draw.inkReplay", json!({})).unwrap();
    let list = r["strokes"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    let shapes = &reopened.doc().unwrap().wb.active().unwrap().shapes;
    let cols: Vec<u32> = list
        .iter()
        .map(|stroke| {
            let id = u32::try_from(stroke["id"].as_u64().unwrap()).unwrap();
            shapes.iter().find(|s| s.id == id).unwrap().anchor.cell.col
        })
        .collect();
    assert!(cols.windows(2).all(|w| w[0] < w[1]), "strokes keep their drawing order after reopening: {cols:?}");
}

#[test]
fn replay_leaves_the_document_alone() {
    let mut session = workbook();
    session.execute("draw.stroke", json!({"points": [[5, 5], [50, 50]]})).unwrap();
    let before = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    session.execute("draw.inkReplay", json!({})).unwrap();
    session.execute("draw.inkReplay", json!({"on": false})).unwrap();
    let after = session.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap()["base64"].as_str().unwrap().to_string();
    assert_eq!(before, after, "Ink Replay must not change what the workbook saves");
}
