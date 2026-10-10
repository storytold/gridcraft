//! Formula helpers must stay in the worksheet while its editor scrolls.
use egui::{Context, Id, LayerId, Order, RawInput, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

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

fn frames(ctx: &Context, app: &mut SheetApp) {
    for _ in 0..4 {
        frame(ctx, app);
    }
}

fn visible(ctx: &Context, editor: &str) -> bool {
    ctx.memory(|m| m.areas().is_visible(&LayerId::new(Order::Tooltip, Id::new(editor).with("ac"))))
}

#[test]
fn cell_formula_helpers_hide_when_scrolled_out_and_return_with_the_editor() {
    for text in ["=SU", "=SUM("] {
        let ctx = Context::default();
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        frames(&ctx, &mut app);
        app.begin_edit(Some(text.into()), false);
        if text == "=SU" {
            app.editor.as_mut().unwrap().update_autocomplete(&["SUM".into(), "SUMIF".into()]);
        }
        frames(&ctx, &mut app);
        assert!(visible(&ctx, "gridcraft.cell_editor"), "visible cell shows {text} help");
        app.view_mut().unwrap().scroll.y = 400.0;
        frames(&ctx, &mut app);
        assert!(!visible(&ctx, "gridcraft.cell_editor"), "scrolled-out cell must hide {text} help");
        assert_eq!(app.editor.as_ref().unwrap().text, text, "scrolling preserves the edit");
        app.view_mut().unwrap().scroll.y = 0.0;
        frames(&ctx, &mut app);
        assert!(visible(&ctx, "gridcraft.cell_editor"), "scrolling back restores {text} help");
    }
}

#[test]
fn formula_bar_hint_remains_visible_when_the_worksheet_scrolls() {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    frames(&ctx, &mut app);
    app.begin_edit(Some("=SUM(".into()), true);
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.formula_bar"));
    app.view_mut().unwrap().scroll.y = 400.0;
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.formula_bar"));
}

#[test]
fn cell_argument_hint_is_clipped_to_the_worksheet() {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    frames(&ctx, &mut app);
    app.begin_edit(Some("=SUM(".into()), false);
    frames(&ctx, &mut app);
    app.view_mut().unwrap().scroll.y = 10.0;
    frames(&ctx, &mut app);
    let output = frame(&ctx, &mut app);
    let cells = app.grid.cells_rect.unwrap();
    let hint = output.shapes.iter().find(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "SUM(")).expect("argument hint painted");
    assert!(cells.contains_rect(hint.clip_rect), "helper must not paint over the headers: {:?}", hint.clip_rect);
}

#[test]
fn scrolled_out_cell_editor_still_handles_escape() {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Default::default());
    frames(&ctx, &mut app);
    app.begin_edit(Some("=SUM(".into()), false);
    frames(&ctx, &mut app);
    app.view_mut().unwrap().scroll.y = 400.0;
    frames(&ctx, &mut app);
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    assert!(app.editor.is_none(), "scrolling must not disable editor keys");
}

#[test]
fn cell_editor_and_helper_stay_below_the_frozen_top_row() {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    session.run("view.freezeTopRow", serde_json::json!({})).unwrap();
    session.run("selection.set", serde_json::json!({"range": "B5"})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    frames(&ctx, &mut app);
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let frozen_h = sh.row_height(0);
    let scroll = sh.row_top(4) as f32;
    app.begin_edit(Some("=SUM(".into()), false);
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.cell_editor"));
    app.view_mut().unwrap().scroll.y = scroll - frozen_h / 2.0;
    frames(&ctx, &mut app);
    let output = frame(&ctx, &mut app);
    let frozen_bottom = app.grid.cells_rect.unwrap().top() + frozen_h * gridcraft_ui_egui::grid::DISPLAY_SCALE;
    let editor = output
        .shapes
        .iter()
        .find(|s| s.clip_rect.bottom() > frozen_bottom && matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "=SUM("))
        .expect("editor painted");
    assert!(editor.clip_rect.top() >= frozen_bottom, "editor must be clipped below the frozen row: {:?}", editor.clip_rect);
    app.view_mut().unwrap().scroll.y = scroll;
    frames(&ctx, &mut app);
    assert!(!visible(&ctx, "gridcraft.cell_editor"), "cell hidden under the frozen row must hide its helper");
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(");
    app.view_mut().unwrap().scroll.y = 0.0;
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.cell_editor"), "scrolling back restores the helper");
}
