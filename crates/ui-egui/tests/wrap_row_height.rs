//! A row of wrapped text grows to the text as the grid draws it (#1969): the app lends the
//! engine its fonts (`Session::text_width`), so a word measured wider than the engine's estimate
//! still gets its line, at the screen's scale too.
//!
//! Fonts are the shared harness's pinned set (bundled faces plus craft-fonts), never the host's:
//! the installed-font scan finishes at a different frame on every machine and would change the
//! cell font between the edit and the measurement below. A separate test binary, because that
//! scan binds faces process-wide when any other UI test runs it.

use egui::{Color32, Context, FontId, RawInput, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

const TEXT: &str = "Since the 1960s, NASA has left hardware across the solar system\u{2013}on the Moon, on Mars, and in deep space.";

/// Runs a few frames of the app in a 1200 × 800 window.
fn frames(ctx: &Context, app: &mut SheetApp) {
    for _ in 0..4 {
        let mut output =
            ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), ..Default::default() }, |ui| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            });
        output.textures_delta.clear();
    }
}

#[test]
fn a_wrapped_row_fits_the_text_as_the_grid_draws_it() {
    // The grid lays text out at the screen's scale, where line breaks differ from 1 pixel per
    // point: on a 1.5 or 1.6 display the issue's sentence wrapped to one line more than a row
    // measured at 1 pixel per point held.
    for ppp in [1.0, 1.25, 1.5, 1.6, 1.75, 2.0] {
        let ctx = Context::default();
        ctx.set_pixels_per_point(ppp);
        ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
        gridcraft_ui_egui::theme::apply(&ctx, false);
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        app.use_prepared_fonts();
        frames(&ctx, &mut app);
        for chars in [6, 8, 9, 12, 14, 20] {
            app.run("home.columnWidth", json!({"cols": "A:A", "chars": chars})).unwrap();
            app.run("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
            app.run("range.setValues", json!({"range": "A1", "values": [[TEXT]]})).unwrap();
            frames(&ctx, &mut app);
            let wb = &app.session.active().unwrap().wb;
            let sheet = wb.active().unwrap();
            let st = wb.styles.get(sheet.style_id(gridcraft_engine::cell(0, 0)));
            let size = st.font.size * 96.0 / 72.0;
            let font = FontId::new(size, gridcraft_ui_egui::theme::cell_family(&st.font.name, st.font.bold, st.font.italic));
            // The grid wraps at the column's width less 2.5 px a side, in the cell font.
            let wrap = sheet.col_width(0) - 5.0;
            let drawn = ctx.fonts_mut(|f| f.layout_job(egui::text::LayoutJob::simple(TEXT.into(), font, Color32::PLACEHOLDER, wrap)).rows.len());
            // The engine gives each line 1.22 × the font size plus 5 px, whatever the face's own
            // line height, so compare lines, not pixels.
            let row = sheet.row_height(0);
            let lines = ((row - 5.0) / (size * 1.22)).round() as usize;
            assert_eq!(lines, drawn, "{ppp} px/pt, {chars} chars: the row ({row} px) has one line per drawn line");
        }
    }
}
