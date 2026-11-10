//! Header AutoFit measured with the fonts the grid draws in (#1316).
//!
//! A separate test binary: these tests run the process-wide installed-font scan, whose faces bind
//! from the next frame and would change text widths under the drag tests in `header_autofit.rs`
//! running in parallel threads of the same process.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness(selection: &str) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "range.setValues",
            json!({"range": "A1", "values": [
                ["Short", "This column has considerably longer content", "Third", "Unchanged"],
                ["Two\nlines", "B", "C", "D"],
                ["Three\ntext\nlines", "B", "C", "D"]
            ]}),
        )
        .unwrap();
    session.execute("home.columnWidth", json!({"cols": "A:D", "width": 100})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "1:4", "height": 60})).unwrap();
    session.execute("selection.set", json!({"range": selection})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    h.state_mut().session.journal.clear();
    h
}

fn edge(h: &egui_kittest::Harness<'_, SheetApp>, rows: bool, index: u32) -> (Pos2, f32) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = if rows {
        pos2(geo.rect.left() + geo.header_w / 2.0, geo.y(sheet, index + 1))
    } else {
        pos2(geo.x(sheet, index + 1), geo.rect.top() + geo.header_h / 2.0)
    };
    (pos, geo.z)
}

fn button(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2, pressed: bool) {
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    h.step();
}

#[test]
fn header_double_click_fits_the_text_as_the_grid_draws_it() {
    // Issue #1316: `@` and `%` are far wider in every real font than the engine's average-glyph
    // estimate, so AutoFit has to measure with the font the grid draws the cell in.
    let text = "sarmstrong@example.com %%% @@@";
    let mut h = harness("A1");
    h.state_mut().session.execute("range.setValues", json!({"range":"A1", "values":[[text]]})).unwrap();
    // Let the installed-font scan finish and bind its faces first, so the cell's font can't
    // change between the AutoFit and the measurement below.
    gridcraft_ui_egui::fonts::catalog();
    h.run_steps(6);
    let (pos, _) = edge(&h, false, 0);
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.step();
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.run_steps(2);

    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let font = &app.session.active().unwrap().wb.styles.get(sheet.style_id(gridcraft_engine::core::CellRef::new(0, 0))).font;
    let id = egui::FontId::new(font.size * 96.0 / 72.0, gridcraft_ui_egui::theme::cell_family(&font.name, font.bold, font.italic));
    let drawn = h.ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), id, egui::Color32::BLACK)).size().x;
    let estimate = gridcraft_engine::cmd::format::approx_text_width(text, font.size, font.bold);
    assert!(drawn > estimate + 6.0, "fixture: the drawn text ({drawn}) must be wider than the estimate ({estimate}) plus padding");
    // Two 2.5 px paddings and the gridline, as the grid lays out the cell.
    assert!(sheet.col_width(0) >= drawn + 6.0, "column {} is narrower than the drawn text {drawn} + padding", sheet.col_width(0));
}

#[test]
fn the_quick_measure_is_never_narrower_than_the_drawn_text() {
    // Columns with many distinct values are measured by summing glyph advances; that must cover
    // the laid-out text, or AutoFit would clip again on large columns.
    let h = harness("A1");
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let mut font = app.session.active().unwrap().wb.styles.get(sheet.style_id(gridcraft_engine::core::CellRef::new(0, 0))).font.clone();
    for bold in [false, true] {
        font.bold = bold;
        for text in ["sarmstrong@example.com", "Quarterly revenue summary", "WWW MMM Wide Headers", "AVATAR To Ty", "fi ffl 1,234.56"] {
            let quick = gridcraft_ui_egui::theme::cell_text_width(&h.ctx, text, &font, false);
            let exact = gridcraft_ui_egui::theme::cell_text_width(&h.ctx, text, &font, true);
            assert!(exact > 0.0 && quick >= exact, "{text:?} bold={bold}: quick {quick} < drawn {exact}");
        }
    }
}
