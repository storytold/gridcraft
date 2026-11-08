//! Draw › Ink Replay: the button starts the animation, which paints the strokes in drawing order
//! and stops on a second click without touching the document.

use egui::Shape;
use egui::epaint::ColorMode;
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::model::ShapeKind;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

const INK: egui::Color32 = egui::Color32::from_rgb(0xA1, 0x2B, 0xCD);

/// Painted path shapes in the stroke's colour: the committed ink, or the replayed ink.
fn ink_paths(h: &egui_kittest::Harness<'static, SheetApp>) -> usize {
    h.output().shapes.iter().filter(|s| matches!(&s.shape, Shape::Path(p) if p.stroke.color == ColorMode::Solid(INK))).count()
}

#[test]
fn ink_replay_button_animates_in_order_then_stops() {
    let mut session = Session::new();
    session.new_workbook();
    for points in [[[40, 40], [160, 40]], [[200, 40], [320, 40]]] {
        session.execute("draw.stroke", json!({"points": points, "color": "#A12BCD", "width": 2})).unwrap();
    }
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
