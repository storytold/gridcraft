//! The text being typed into a cell sits exactly where the cell draws it once entered, so Enter
//! doesn't make it jump (Top, Middle and Bottom alignment, normal and tall rows, 100% and 134%).

use egui::{Context, Pos2, RawInput, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid};
use serde_json::json;

fn frame(ctx: &Context, app: &mut SheetApp) -> egui::FullOutput {
    let mut output =
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), ..Default::default() }, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        });
    output.textures_delta.clear();
    output
}

/// Where the text "Abc" shows inside B3 (its galley's top-left corner).
fn text_pos(ctx: &Context, app: &mut SheetApp) -> Pos2 {
    for _ in 0..3 {
        frame(ctx, app);
    }
    let out = frame(ctx, app);
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let cell = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sh, CellRef::parse("B3").unwrap());
    // The last one painted: while editing, the editor covers the cell's own text.
    out.shapes
        .iter()
        .rev()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "Abc" && cell.expand(4.0).contains(t.pos) => Some(t.pos),
            _ => None,
        })
        .expect("Abc drawn in B3")
}

#[test]
fn typed_text_sits_where_the_entered_text_is_drawn() {
    for (height, zoom) in [(None, 100), (Some(90.0), 100), (None, 134), (Some(90.0), 134)] {
        for align in ["home.alignTop", "home.alignMiddle", "home.alignBottom"] {
            let mut session = Session::new();
            session.new_workbook();
            session.execute("view.zoom", json!({"percent": zoom})).unwrap();
            if let Some(h) = height {
                session.execute("home.rowHeight", json!({"rows": "3:3", "height": h})).unwrap();
            }
            session.execute(align, json!({"range": "B3"})).unwrap();
            session.execute("cell.set", json!({"cell": "B3", "input": "Abc"})).unwrap();
            session.execute("selection.set", json!({"cell": "B3"})).unwrap();
            let ctx = Context::default();
            let mut app = SheetApp::new(session, Default::default());
            let entered = text_pos(&ctx, &mut app);
            app.begin_edit(None, false);
            let typed = text_pos(&ctx, &mut app);
            assert!(
                (typed.y - entered.y).abs() < 0.5,
                "{align}, row height {height:?}, zoom {zoom}: typed at y {} but drawn at y {}",
                typed.y,
                entered.y
            );
        }
    }
}
