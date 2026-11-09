//! Draw › Ink Replay: the button starts the animation, which paints the strokes in drawing order
//! and stops on a second click without touching the document.

use egui::Shape;
use egui::epaint::ColorMode;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::model::{Color, ShapeKind};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const INK: egui::Color32 = egui::Color32::from_rgb(0xA1, 0x2B, 0xCD);

/// A session with one horizontal ink stroke per pair of sheet points.
fn session_with(strokes: &[[[i32; 2]; 2]]) -> Session {
    let mut session = Session::new();
    session.new_workbook();
    for points in strokes {
        session.execute("draw.stroke", json!({"points": points, "color": "#A12BCD", "width": 2})).unwrap();
    }
    session
}

/// A session whose only stroke is `points` sheet points.
fn session_with_one(points: &[[i32; 2]]) -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("draw.stroke", json!({"points": points, "color": "#A12BCD", "width": 2})).unwrap();
    session
}

/// The Draw tab on screen, ready for the Ink Replay button.
fn harness(session: Session) -> Harness {
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = "Draw".into();
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(&ui.ctx().clone());
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

/// Painted path shapes in the stroke's colour: the committed ink, or the replayed ink.
fn ink_paths(h: &Harness) -> usize {
    h.output().shapes.iter().filter(|s| matches!(&s.shape, Shape::Path(p) if p.stroke.color == ColorMode::Solid(INK))).count()
}

/// The widths of the painted horizontal ink paths (the stroke runs left to right), in painting
/// order. A stroke over 8 pt is a highlighter: painted translucent by its width.
fn ink_widths(h: &Harness) -> Vec<f32> {
    let highlight = INK.gamma_multiply(0.45);
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Path(p)
                if p.points.len() == 2 && (p.points[0].y - p.points[1].y).abs() < 0.01 && p.stroke.color == ColorMode::Solid(highlight) =>
            {
                Some(p.stroke.width)
            }
            _ => None,
        })
        .collect()
}

/// The button starts the animation on the first frame, draws each stroke progressively in drawing
/// order, and a second click puts the sheet's own ink back without editing it.
#[test]
fn ink_replay_button_animates_in_order_then_stops() {
    let mut h = harness(session_with(&[[[40, 40], [160, 40]], [[200, 40], [320, 40]]]));
    assert_eq!(ink_paths(&h), 2, "both strokes are painted before replay");
    assert!(h.state().grid.replay.is_none());

    h.get_by_label("Ink Replay").click();
    h.run_steps(1);
    assert!(h.state().grid.replay.is_some(), "the button starts the replay");
    assert!(h.state().session.ink_replay);
    assert!(ink_paths(&h) <= 1, "the first frame draws at most the first stroke, progressively");

    h.run_steps(60);
    assert_eq!(ink_paths(&h), 2, "finished strokes stay drawn");

    h.get_by_label("Ink Replay").click();
    h.run_steps(2);
    assert!(h.state().grid.replay.is_none(), "the second click stops the replay");
    assert!(!h.state().session.ink_replay);
    assert_eq!(ink_paths(&h), 2, "the sheet's own ink is painted again");
    let strokes = h.state().session.active().unwrap().wb.active().unwrap().shapes.iter().filter(|s| s.kind == ShapeKind::Ink).count();
    assert_eq!(strokes, 2, "replaying never edits the document");
}

/// Replay hides the sheet's own ink while it runs, so a stroke drawn meanwhile must end it rather
/// than stay hidden behind the snapshot.
#[test]
fn replay_stops_when_the_sheets_ink_changes() {
    let mut h = harness(session_with(&[[[40, 40], [160, 40]], [[200, 40], [320, 40]]]));
    h.get_by_label("Ink Replay").click();
    h.run_steps(1);
    assert!(h.state().grid.replay.is_some());

    h.state_mut().session.execute("draw.stroke", json!({"points": [[40, 120], [160, 120]], "color": "#A12BCD"})).unwrap();
    h.run_steps(2);
    assert!(h.state().grid.replay.is_none(), "a new stroke ends the replay");
    assert!(!h.state().session.ink_replay);
    assert_eq!(ink_paths(&h), 3, "the sheet's ink, the new stroke included, is painted again");
}

/// Zooming and other document edits are not ink changes: the replay keeps running through them.
#[test]
fn replay_survives_a_zoom() {
    let mut h = harness(session_with(&[[[40, 40], [160, 40]], [[200, 40], [320, 40]]]));
    h.get_by_label("Ink Replay").click();
    h.run_steps(1);
    assert!(h.state().grid.replay.is_some());

    h.state_mut().run("view.zoom", json!({"percent": 150})).unwrap();
    h.run_steps(2);
    assert!(h.state().grid.replay.is_some(), "zooming does not end the replay");
}

/// Replayed ink is the sheet's ink: a legacy stroke wider than the pen allows (its width is the
/// red channel of `fill`) paints as wide during the replay as it does after it.
#[test]
fn replay_paints_a_stroke_at_the_sheets_own_width() {
    let mut session = session_with(&[[[40, 40], [160, 40]]]);
    {
        let d = session.doc_mut().unwrap();
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        let stroke = wb.sheet_mut(0).unwrap().shapes.iter_mut().find(|s| s.kind == ShapeKind::Ink).unwrap();
        // No `InkStyle`: the legacy polyline, 60 pt wide in the red channel of `fill`.
        stroke.ink = None;
        stroke.fill = Color::rgb(60, 0, 0);
    }
    let mut h = harness(session);
    h.run_steps(2);
    let before = ink_widths(&h);
    assert_eq!(before.len(), 1, "the legacy stroke is painted");

    h.get_by_label("Ink Replay").click();
    h.run_steps(60);
    assert_eq!(ink_widths(&h), before, "the replay paints the stroke at the sheet's own width");
}

/// The points of the painted ink paths, in painting order.
fn ink_points(h: &Harness) -> Vec<Vec<egui::Pos2>> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Path(p) if p.points.len() >= 2 && p.stroke.color == ColorMode::Solid(INK) => Some(p.points.clone()),
            _ => None,
        })
        .collect()
}

/// A right-to-left sheet moves a floating object as a whole instead of reflecting it. Replayed ink
/// is the same object, so it lands on exactly the committed stroke's points — an L-shaped stroke
/// would open the other way if the mirror pass reflected it.
#[test]
fn replay_moves_ink_with_a_right_to_left_sheet() {
    // An L: reflection turns it into a mirrored L, which differs from moving it.
    let mut session = session_with_one(&[[40, 40], [160, 40], [160, 120]]);
    session.execute("draw.stroke", json!({"points": [[220, 40], [320, 40], [320, 140]], "color": "#A12BCD", "width": 2})).unwrap();
    std::sync::Arc::make_mut(&mut session.doc_mut().unwrap().wb).sheet_mut(0).unwrap().right_to_left = true;
    let mut h = harness(session);
    h.run_steps(2);
    let committed = ink_points(&h);
    assert_eq!(committed.len(), 2, "both strokes are painted on the mirrored sheet");
    assert!(committed.iter().all(|p| p.len() == 3), "each stroke keeps its three points");

    h.get_by_label("Ink Replay").click();
    h.run_steps(1);
    assert!(h.state().grid.replay.is_some(), "the button starts the replay on a mirrored sheet");
    h.run_steps(60);
    assert_eq!(ink_points(&h), committed, "replayed ink lands where the committed ink does, not reflected");
}
