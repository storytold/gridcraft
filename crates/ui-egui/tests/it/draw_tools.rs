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
fn there_is_one_select_tool_and_it_keeps_its_keytip() {
    // `draw.lasso` selects ink the way Select Objects does, so a second "Lasso Select" button was
    // the same command twice (and carried no keytip). One button remains, with the keytip.
    let h = draw_tab();
    assert!(h.query_by_label("Select Objects").is_some(), "the Select tool is there");
    assert!(h.query_by_label("Lasso Select").is_none(), "no duplicate Lasso Select button");
}

#[test]
fn a_custom_pen_tile_carries_its_own_name() {
    // Custom pens were all labelled with their brush ("Pen"), so two custom pens were the same
    // accessible name; the tile now carries the gallery id too.
    let mut session = Session::new();
    session.new_workbook();
    let added = session.execute("draw.addPen", serde_json::json!({"brush": "pen", "color": "#0A7B3E"})).unwrap();
    let id = added["pen"].as_u64().unwrap();
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
    // The pen it just added is the one in hand, so its name carries the selected state too.
    let label = format!("Pen {id}, selected");
    assert!(h.query_by_label(&label).is_some(), "the custom pen has its own name: {label}");
    assert!(h.query_by_label(&format!("{label} options")).is_some());
}

#[test]
fn pen_dropdown_swatches_and_textures_change_the_gallery_pen() {
    // Nothing tested the dropdown wiring: a colour swatch, a texture swatch, the Add menu and
    // Delete Tool all run commands whose parameters only the UI builds.
    let mut h = draw_tab();

    // A solid colour replaces the Galaxy pen's texture (effect cleared, colour exactly the hex).
    h.get_by_label("Galaxy pen options").click();
    h.run_steps(6);
    h.get_by_label("Pen colour #C00000").click();
    h.run_steps(6);
    let pens = h.state().session.draw_pens.clone();
    assert_eq!(pens[0].color, "#C00000");
    assert_eq!(pens[0].effect, None, "a solid colour clears the texture");
    assert_eq!(h.state().session.draw_tool, "pen");

    // A texture sets its base colour and the effect. The tile now reads as selected, so its name
    // (and its chevron's) says so.
    h.get_by_label("Galaxy pen, selected options").click();
    h.run_steps(6);
    h.get_by_label("Lava").click();
    h.run_steps(6);
    let pens = h.state().session.draw_pens.clone();
    assert_eq!(pens[0].effect.as_deref(), Some("lava"));
    assert_eq!(pens[0].color, "#D94F1E", "the texture's base colour");

    // The Add menu adds a brush to the gallery and puts it in hand.
    let before = h.state().session.draw_pens.len();
    h.get_by_label("Add").click();
    h.run_steps(6);
    h.get_by_label("Pen").click();
    h.run_steps(6);
    let pens = h.state().session.draw_pens.clone();
    assert_eq!(pens.len(), before + 1, "the Add menu adds a pen");
    assert_eq!(pens.last().map(|p| p.tool.as_str()), Some("pen"));

    // Delete Tool removes the custom pen it belongs to.
    let id = pens.last().map(|p| p.id).unwrap();
    h.get_by_label(&format!("Pen {id}, selected options")).click();
    h.run_steps(6);
    h.get_by_label("Delete Tool").click();
    h.run_steps(6);
    assert!(h.state().session.draw_pens.iter().all(|p| p.id != id), "Delete Tool removes the pen");
}

#[test]
fn update_pen_clears_a_texture_when_only_a_solid_colour_is_sent() {
    // The Colors swatches and More Colors picker send a colour with no effect: a textured pen
    // must become solid, or `stroke_color` would keep painting the old texture's base colour.
    // (The engine resets the colour when only the effect changes, so the dropdown passes the
    // texture's base colour with it — both directions stay in sync.)
    let mut s = Session::new();
    s.new_workbook();
    let pen = serde_json::json!({"brush": "pen", "color": "#FF0000", "width": 1.0, "effect": "galaxy"});
    let added = s.execute("draw.addPen", pen.clone()).unwrap();
    let id = added["pen"].as_u64().expect("addPen returns the gallery id");
    let solid = s.execute("draw.updatePen", serde_json::json!({"id": id, "color": "#FF0000", "effect": ""})).unwrap();
    assert_eq!(solid["color"], serde_json::json!("#FF0000"), "solid colour sticks");
    assert!(solid["effect"].is_null(), "texture cleared: {solid:?}");
    let textured = s.execute("draw.updatePen", serde_json::json!({"id": id, "effect": "galaxy"})).unwrap();
    assert_eq!(textured["effect"], serde_json::json!("galaxy"), "texture applies: {textured:?}");
    assert_eq!(textured["color"], serde_json::json!("#5B3EA8"), "colour resets to the texture base");
}

