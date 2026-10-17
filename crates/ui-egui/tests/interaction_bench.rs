//! Deterministic interaction-latency budgets for the CI performance stage.
//!
//! These tests use a fixed frame clock instead of wall time, so a busy shared runner cannot turn
//! a performance check flaky. The oracle is the visible result: while a cell-range drag is held,
//! the painted outline must catch the selected cells within [`DRAG_FRAME_BUDGET`] rendered frames.

use egui::{Event, Modifiers, PointerButton, Pos2, Rect};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};

type Harness = egui_kittest::Harness<'static, SheetApp>;

const FPS: f32 = 120.0;
const DT: f32 = 1.0 / FPS;
const DRAG_FRAME_BUDGET: usize = 2;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).with_step_dt(DT).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    // Exercise the opt-in path: a drag must stay responsive even when animations are enabled.
    h.state_mut().ui.animations = true;
    h.ctx.all_styles_mut(|style| style.animation_time = 0.1);
    h.step();
    h
}

fn point(h: &Harness, a1: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
}

fn selected_rect(h: &Harness) -> Rect {
    let app = h.state();
    let doc = app.session.active().unwrap();
    let sheet = doc.wb.active().unwrap();
    Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).range_rect(sheet, doc.selection.current())
}

fn same_rect(a: Rect, b: Rect) -> bool {
    (a.min - b.min).length_sq() < 0.01 && (a.max - b.max).length_sq() < 0.01
}

fn frame(h: &mut Harness, event: Event) {
    h.input_mut().events.push(event);
    h.step();
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

/// Returns rendered frames from the endpoint event until the painted outline reaches the actual
/// selection. The endpoint frame is frame zero.
fn drag_visual_latency(h: &mut Harness, from: &str, to: &str) -> usize {
    let from = point(h, from);
    frame(h, Event::PointerMoved(from));
    frame(h, button(from, true));
    let to = point(h, to);
    frame(h, Event::PointerMoved(to));

    for frames in 0..=120 {
        if same_rect(h.state().grid.glide.drawn(), selected_rect(h)) {
            return frames;
        }
        h.step();
    }
    panic!("selection outline did not reach its drag endpoint within one second");
}

#[test]
#[ignore = "run by the interaction-performance CI job"]
fn drag_selection_visual_latency_stays_within_budget() {
    let mut h = harness();
    let frames = drag_visual_latency(&mut h, "C3", "F7");
    let millis = frames as f32 * 1_000.0 / FPS;
    println!("drag selection visual latency: {frames} frames ({millis:.1} ms at {FPS:.0} Hz)");
    assert!(
        frames <= DRAG_FRAME_BUDGET,
        "drag outline took {frames} frames ({millis:.1} ms) to catch the pointer; budget is {DRAG_FRAME_BUDGET} frames"
    );
}
