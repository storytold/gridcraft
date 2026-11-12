//! Ink Replay reports the sheet's ink in drawing order, survives a save/reopen and never edits
//! the document.

use gridcraft_engine::Session;
use serde_json::json;
use std::sync::Arc;

/// A fresh session with one empty sheet.
fn workbook() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

/// Strokes are reported in the order they were drawn, with their id and point count, and empty
/// parameters toggle the replay off.
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

/// No ink is a typed error, and hostile parameters are ignored rather than trusted.
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

/// The recorded order is written to the file and read back: a reopened workbook replays in the
/// order its strokes were drawn, not the order the drawing stores them in.
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
    // The order itself came from the file (`gridcraftOrder`), not from the reading order.
    for (i, stroke) in list.iter().enumerate() {
        assert_eq!(stroke["order"], i);
    }
}

/// A stroke's recorded order wins over its position in the drawing's shape list.
#[test]
fn replay_follows_recorded_order_over_document_order() {
    let mut session = workbook();
    for x in [10.0, 200.0, 400.0] {
        session.execute("draw.stroke", json!({"points": [[x, 10.0], [x + 40.0, 30.0]]})).unwrap();
    }
    // The drawing is stored in the reverse document order: the recorded order must win.
    let d = session.doc_mut().unwrap();
    Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap().shapes.reverse();
    let r = session.execute("draw.inkReplay", json!({})).unwrap();
    let list = r["strokes"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    let shapes = &session.doc().unwrap().wb.active().unwrap().shapes;
    let cols: Vec<u32> = list
        .iter()
        .map(|stroke| {
            let id = u32::try_from(stroke["id"].as_u64().unwrap()).unwrap();
            shapes.iter().find(|s| s.id == id).unwrap().anchor.cell.col
        })
        .collect();
    assert!(cols.windows(2).all(|w| w[0] < w[1]), "the recorded draw order wins over document order: {cols:?}");
}

/// Without any recorded order (InkML from Excel records none; legacy DrawingML ink has no
/// `InkStyle` at all) the replay keeps document order.
#[test]
fn replay_falls_back_to_document_order_without_a_recorded_order() {
    // Ink from Excel: no order recorded. Legacy DrawingML ink: no `InkStyle` at all.
    for legacy in [false, true] {
        let mut session = workbook();
        for x in [10.0, 200.0, 400.0] {
            session.execute("draw.stroke", json!({"points": [[x, 10.0], [x + 40.0, 30.0]]})).unwrap();
        }
        let d = session.doc_mut().unwrap();
        let sh = Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap();
        for sp in &mut sh.shapes {
            match (&mut sp.ink, legacy) {
                (Some(ink), false) => ink.order = 0,
                (ink, true) => *ink = None,
                (None, false) => {}
            }
        }
        sh.shapes.reverse();
        let r = session.execute("draw.inkReplay", json!({})).unwrap();
        let list = r["strokes"].as_array().unwrap();
        let shapes = &session.doc().unwrap().wb.active().unwrap().shapes;
        let cols: Vec<u32> = list
            .iter()
            .map(|stroke| {
                let id = u32::try_from(stroke["id"].as_u64().unwrap()).unwrap();
                shapes.iter().find(|s| s.id == id).unwrap().anchor.cell.col
            })
            .collect();
        assert!(cols.windows(2).all(|w| w[0] > w[1]), "no recorded order (legacy: {legacy}) replays in document order: {cols:?}");
    }
}

/// Replaying never edits the document and leaves no undo step behind.
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

/// A key restored with uppercase digits or padding names the same stroke: the held-key set keeps
/// its canonical form, so the mint-past check cannot miss the identity under another spelling.
#[test]
fn held_keys_keep_the_canonical_spelling_of_an_unclean_key() {
    use gridcraft_engine::cmd::draw::held_ink_keys;
    let mut session = workbook();
    let shape = session.execute("draw.stroke", json!({"points": [[0, 0], [10, 10]]})).unwrap()["shape"].as_u64().unwrap();
    let key = {
        let d = session.doc().unwrap();
        d.wb.active().unwrap().shapes.iter().find(|s| s.id as u64 == shape).unwrap().ink.as_ref().unwrap().key.clone().unwrap()
    };
    // The same identity in another spelling, as JSON restore can leave it.
    let unclean = format!(" {} ", key.to_ascii_uppercase());
    {
        let d = session.doc_mut().unwrap();
        let sh = std::sync::Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap();
        sh.shapes.iter_mut().find(|s| s.id as u64 == shape).unwrap().ink.as_mut().unwrap().key = Some(unclean.clone());
    }
    let held = held_ink_keys(&session.doc().unwrap().wb);
    assert_eq!(held.len(), 1, "one stroke, one identity: {held:?}");
    assert!(held.contains(&key), "the canonical spelling is held, so minting moves past it: {held:?}");
    assert!(!held.contains(&unclean), "the unclean spelling itself is not the identity: {held:?}");
}
