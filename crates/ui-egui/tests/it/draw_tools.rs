//! Draw ▸ Drawing Tools: each tile is its own pen to a screen reader, and the tools describe what
//! they really do.

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn draw_tab() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    app.ui.ribbon_tab = "Draw".into();
    let mut h = crate::common::builder().with_size(vec2(1440.0, 700.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        app,
    );
    h.run_steps(4);
    h
}

#[test]
fn every_pen_tile_carries_its_own_name() {
    // The six default pens are all `pen` or their own tool, so naming a tile after the tool gave
    // the Galaxy pen and the Red pen the same accessible name ("Pen"): a screen reader could not
    // tell them apart, and every custom pen was another "Pen".
    let h = draw_tab();
    for pen in ["Galaxy pen", "Red pen", "Pencil", "Highlighter", "Fountain Pen", "Brush Pen"] {
        assert!(h.query_by_label(pen).is_some(), "{pen} names its own tile");
        assert!(h.query_by_label(&format!("{pen} options")).is_some(), "{pen}'s chevron names it too");
    }
    assert!(h.query_by_label("Pen").is_none(), "no tile falls back to the tool's generic name");
}

#[test]
fn the_lasso_button_describes_what_it_really_does() {
    // `draw.lasso` selects ink the way Select Objects does (its spec says so); the grid has no
    // freehand-loop selection, so the tooltip must not promise one.
    let mut h = draw_tab();
    h.ctx.all_styles_mut(|s| s.interaction.tooltip_delay = 0.0);
    h.get_by_label("Lasso Select").hover();
    h.run_steps(6);
    assert!(h.query_by_value("Select and move ink and shapes, as Select Objects does").is_some(), "the Lasso Select tooltip tells the truth");
}
