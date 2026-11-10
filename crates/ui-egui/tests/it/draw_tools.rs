//! Draw ▸ Drawing Tools: each tile is its own pen to a screen reader, and the tools describe what
//! they really do.

use egui::vec2;
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A harness on the Draw tab, sized to show the pen gallery.
fn draw_tab() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    draw_tab_with(session)
}

/// The Draw-tab harness for a session that already holds something, such as an imported texture.
fn draw_tab_with(session: Session) -> Harness {
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

/// Each default pen tile answers with its own accessible name, so a screen reader can tell them apart.
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

/// The lasso tooltip promises what the command does — selecting ink — not a freehand loop the grid cannot draw.
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

/// A gallery tile answers with the texture's name, and the import tile says what it does, so the
/// imported-texture part of the pen dropdown can be worked by a screen reader.
#[test]
fn an_imported_texture_tile_names_its_texture() {
    let image = image::RgbaImage::from_pixel(8, 8, image::Rgba([30, 120, 200, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute("draw.textureImport", serde_json::json!({"name": "stripes.png", "base64": gridcraft_engine::io::base64_encode(&bytes.into_inner())}))
        .unwrap();
    let mut h = draw_tab_with(session);
    // Open a pen's dropdown, where the imported textures sit beside Excel's eight.
    h.get_by_label("Galaxy pen options").click();
    h.run_steps(6);
    assert!(h.query_by_label("stripes.png").is_some(), "the tile carries the texture's name");
    assert!(h.query_by_label("Import a texture…").is_some(), "the + tile says what it opens");
}

/// Deleting a pen from its dropdown while the gallery is being walked does not index past the end:
/// `draw.deletePen` shrinks `draw_pens` inside the loop that is reading it (#3606 review).
#[test]
fn deleting_a_pen_from_its_dropdown_does_not_break_the_gallery_loop() {
    let mut session = Session::new();
    session.new_workbook();
    for _ in 0..2 {
        session.execute("draw.addPen", serde_json::json!({"brush": "pen"})).unwrap();
    }
    let mut h = draw_tab_with(session);
    // Both custom pens answer as "Pen options"; take the first one, so a pen still follows it in
    // the same frame's loop range.
    h.query_all_by_label("Pen options")
        .min_by(|a, b| a.rect().left().total_cmp(&b.rect().left()))
        .expect("the two custom pens have chevrons")
        .click();
    h.run_steps(6);
    h.get_by_label("Delete Tool").click();
    h.run_steps(6);
    assert_eq!(h.state().session.draw_pens.len(), 7, "one custom pen was deleted");
    assert!(h.query_by_label("Delete Tool").is_none(), "the gallery still renders after the delete");
}
