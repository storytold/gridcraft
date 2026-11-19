//! The Comments pane: one Show Comments button, and a pane that keeps its width (#710).

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

const WIDTH: f32 = 1200.0;

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut h = crate::common::builder().with_step_dt(1.0 / 60.0).with_size(vec2(WIDTH, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn grid_right(h: &Harness) -> f32 {
    h.state().grid.rect.unwrap().right()
}

/// A comment whose author line and reply row are wide enough to push the pane if they don't wrap.
fn open_pane_on_long_comment(h: &mut Harness) {
    let text = "A long comment that should wrap inside the pane instead of pushing it wider. ".repeat(6);
    let author = "Someone With A Rather Long Display Name, Finance Department, Head Office";
    h.state_mut().run("review.newComment", json!({"cell": "B2", "text": text, "author": author})).unwrap();
    h.state_mut().run("review.showComments", json!({"on": true})).unwrap();
    h.run_steps(3);
}

fn drag(h: &mut Harness, from: Pos2, to: Pos2) {
    let press = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.input_mut().events.extend([Event::PointerMoved(from), press(from, true)]);
    h.step();
    for i in 1..=6 {
        let t = i as f32 / 6.0;
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * t));
        h.step();
    }
    h.input_mut().events.push(press(to, false));
    h.run_steps(3);
}

#[test]
fn the_pane_does_not_grow_on_its_own_and_keeps_a_manual_resize() {
    let mut h = harness();
    // Between the grid and the right edge of the window, before any pane is open.
    let gutter = WIDTH - grid_right(&h);
    open_pane_on_long_comment(&mut h);
    let opened = grid_right(&h);
    assert!(WIDTH - opened < WIDTH / 2.0, "the pane opens at its default width, got {}", WIDTH - opened);
    h.run_steps(60);
    assert_eq!(grid_right(&h), opened, "the pane must not widen itself frame after frame");

    // Drag the pane's left edge 150px to the left: the new width stays.
    let edge = pos2(opened + gutter, 450.0);
    drag(&mut h, edge, edge - vec2(150.0, 0.0));
    let resized = grid_right(&h);
    // The grab point is a few pixels off the exact edge, so allow some slack on the 150px.
    assert!(opened - resized > 120.0, "the drag widens the pane, grid edge {opened} -> {resized}");
    h.run_steps(60);
    assert_eq!(grid_right(&h), resized, "a manual resize must hold");

    // Dragged almost to the window's left edge, the pane stops at half the window.
    let edge = pos2(resized + gutter, 450.0);
    drag(&mut h, edge, pos2(60.0, 450.0));
    assert!(grid_right(&h) > WIDTH / 2.0 - gutter - 30.0, "the pane is capped at half the window, grid edge {}", grid_right(&h));
}

/// How many separate ribbon buttons show `tip` when hovered, scanning the big-button row.
fn buttons_with_tip(h: &mut Harness, tip: &str) -> usize {
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    let (mut count, mut inside) = (0, false);
    let mut x = 2.0;
    while x < WIDTH {
        h.input_mut().events.push(Event::PointerMoved(pos2(x, 120.0)));
        h.run_steps(6);
        let shown = h.query_all_by_value(tip).next().is_some();
        if shown && !inside {
            count += 1;
        }
        inside = shown;
        x += 8.0;
    }
    count
}

#[test]
fn review_has_a_single_show_comments_button() {
    let mut h = harness();
    h.state_mut().ui.ribbon_tab = "Review".into();
    h.run_steps(2);
    // The scan has to find a known single button, or a count of one below would prove nothing.
    assert_eq!(buttons_with_tip(&mut h, "New Comment"), 1);
    assert_eq!(buttons_with_tip(&mut h, "Comments pane"), 1);
}
