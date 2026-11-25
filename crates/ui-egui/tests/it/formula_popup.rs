//! Formula helpers must stay in the worksheet while its editor scrolls.
use egui::{Context, Id, LayerId, Order, RawInput, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
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

/// Reports whether the named autocomplete/helper tooltip layer is visible.
fn visible(ctx: &Context, editor: &str) -> bool {
    ctx.memory(|m| m.areas().is_visible(&LayerId::new(Order::Tooltip, Id::unique(editor).with("ac"))))
}

#[test]
/// Autocomplete and argument help hide when the edited cell scrolls out, the edit is preserved, and both return when it scrolls back.
fn cell_formula_helpers_hide_when_scrolled_out_and_return_with_the_editor() {
    for text in ["=SU", "=SUM("] {
        let ctx = Context::default();
        ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
        let mut session = Session::new();
        session.new_workbook();
        let mut app = crate::common::app(session);
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

/// A wrapped in-cell edit in a hidden row/column must not paint (or show its helper) over the
/// visible line; the edit survives and comes back when the row and column are shown again.
#[test]
fn hidden_row_and_column_hide_the_wrapped_editor_until_unhidden() {
    let ctx = Context::default();
    ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    let mut session = Session::new();
    session.new_workbook();
    session.run("home.wrapText", serde_json::json!({"range": "A2", "on": true})).unwrap();
    session.run("home.hideRows", serde_json::json!({"rows": "2:2"})).unwrap();
    session.run("home.hideColumns", serde_json::json!({"cols": "A:A"})).unwrap();
    session.run("selection.set", serde_json::json!({"range": "A2"})).unwrap();
    let mut app = crate::common::app(session);
    frames(&ctx, &mut app);
    app.begin_edit(Some("=SUM(".into()), false);
    frames(&ctx, &mut app);
    assert!(!visible(&ctx, "gridcraft.cell_editor"), "a hidden cell must not show its helper");
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(", "hiding the cell preserves the edit");
    let cells = app.grid.cells_rect.expect("the grid drew");
    assert!(!paints_visible_text(&frame(&ctx, &mut app), "=SUM(", cells), "a hidden cell must not paint its editor");

    app.session.run("home.unhideRows", serde_json::json!({"rows": "2:2"})).unwrap();
    app.session.run("home.unhideColumns", serde_json::json!({"cols": "A:A"})).unwrap();
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.cell_editor"), "unhiding the cell restores its helper");
    assert!(paints_visible_text(&frame(&ctx, &mut app), "=SUM(", cells), "unhiding the cell restores its editor");
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(", "unhiding the cell preserves the edit");
}

/// Whether `text` is painted with a non-empty clip that reaches the worksheet (a hidden cell's
/// editor may still be in the shape list, clipped to nothing; the formula bar shows the same text
/// above the grid and must not satisfy this check).
fn paints_visible_text(output: &egui::FullOutput, text: &str, cells: Rect) -> bool {
    output
        .shapes
        .iter()
        .any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == text && s.clip_rect.intersect(t.visual_bounding_rect()).intersect(cells).is_positive()))
}

#[test]
/// Formula-bar help stays visible while the sheet scrolls.
fn formula_bar_hint_remains_visible_when_the_worksheet_scrolls() {
    let ctx = Context::default();
    ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    frames(&ctx, &mut app);
    app.begin_edit(Some("=SUM(".into()), true);
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.formula_bar"));
    app.view_mut().unwrap().scroll.y = 400.0;
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.formula_bar"));
}

#[test]
/// Cell argument help is clipped to the worksheet instead of painting over the headers.
fn cell_argument_hint_is_clipped_to_the_worksheet() {
    let ctx = Context::default();
    ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
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
/// An editor for a scrolled-out cell still handles Esc.
fn scrolled_out_cell_editor_still_handles_escape() {
    let ctx = Context::default();
    ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
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
/// The editor and its helper clip below the frozen top row, stay hidden while the cell is under it, and return on scroll-back.
fn cell_editor_and_helper_stay_below_the_frozen_top_row() {
    let ctx = Context::default();
    ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    let mut session = Session::new();
    session.new_workbook();
    session.run("view.freezeTopRow", serde_json::json!({})).unwrap();
    session.run("selection.set", serde_json::json!({"range": "B5"})).unwrap();
    let mut app = crate::common::app(session);
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
    let geo = app.grid.geo.as_ref().expect("grid drew");
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let edited = gridcraft_engine::core::CellRef::new(4, 1);
    assert!(
        geo.hidden_under_frozen(sh, edited),
        "B5 must be scrolled fully under the frozen row: requested {scroll}, kept {:?}, geo scroll {:?}, cells {:?}, frozen_h {}, cell {:?}",
        app.view().scroll,
        geo.scroll,
        geo.cells,
        geo.frozen_h,
        geo.cell_rect(sh, edited)
    );
    assert!(!visible(&ctx, "gridcraft.cell_editor"), "cell hidden under the frozen row must hide its helper");
    assert_eq!(app.editor.as_ref().unwrap().text, "=SUM(");
    app.view_mut().unwrap().scroll.y = 0.0;
    frames(&ctx, &mut app);
    assert!(visible(&ctx, "gridcraft.cell_editor"), "scrolling back restores the helper");
}

/// The editor is drawn in screen coordinates, so its frozen-pane quadrant must be mirrored
/// before the visibility test and clip on a right-to-left sheet. Both panes are checked: using
/// the logical quadrant hides the frozen editor, and lets the scrolling editor's clip reach into
/// the frozen pane.
#[test]
fn frozen_column_editor_paints_in_both_panes_on_a_right_to_left_sheet() {
    for (cell, text) in [("A2", "frozen"), ("B2", "scrolling")] {
        let ctx = Context::default();
        ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
        let mut session = Session::new();
        session.new_workbook();
        session.run("view.rightToLeft", serde_json::json!({"on": true})).unwrap();
        session.run("view.freezeFirstColumn", serde_json::json!({})).unwrap();
        session.run("selection.set", serde_json::json!({"cell": cell})).unwrap();
        let mut app = crate::common::app(session);
        frames(&ctx, &mut app);
        app.begin_edit(Some(text.into()), false);
        frames(&ctx, &mut app);
        let output = frame(&ctx, &mut app);
        let geo = app.grid.geo.as_ref().expect("the grid drew");
        assert!(geo.rtl && geo.fc == 1, "the test needs a right-to-left frozen column");
        let sh = app.session.active().unwrap().wb.active().unwrap();
        let target = CellRef::parse(cell).unwrap();
        let screen_cell = geo.screen_cell_rect(sh, target);
        let split = pos2(geo.cells.left() + geo.frozen_w, geo.cells.top() + geo.frozen_h);
        let quadrant = Rect::from_min_max(
            pos2(if target.col >= geo.fc { split.x } else { geo.cells.left() }, if target.row >= geo.fr { split.y } else { geo.cells.top() }),
            pos2(if target.col >= geo.fc { geo.cells.right() } else { split.x }, if target.row >= geo.fr { geo.cells.bottom() } else { split.y }),
        );
        let screen_quadrant = geo.mirror_rect(quadrant);
        let (clipped, _) = output
            .shapes
            .iter()
            .find_map(|s| {
                let egui::Shape::Text(t) = &s.shape else { return None };
                (t.galley.text() == text && s.clip_rect.intersect(t.visual_bounding_rect()).intersect(screen_cell).is_positive()).then_some((s, t))
            })
            .expect("the editor text is painted inside its screen cell");
        assert!(
            screen_quadrant.contains_rect(clipped.clip_rect),
            "the editor of {cell} must stay inside its screen pane {screen_quadrant:?}, not {:?}",
            clipped.clip_rect
        );
    }
}

#[test]
/// The DATEDIF helper paints its signature and a wrapped unit description no wider than 400 points.
fn argument_hint_shows_the_function_description() {
    // #363: inside DATEDIF( the hint lists the units the third argument takes.
    let ctx = Context::default();
    ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    frames(&ctx, &mut app);
    app.begin_edit(Some("=DATEDIF(A1,B1,".into()), false);
    frames(&ctx, &mut app);
    let output = frame(&ctx, &mut app);
    let texts: Vec<&egui::epaint::TextShape> =
        output.shapes.iter().filter_map(|s| if let egui::Shape::Text(t) = &s.shape { Some(t) } else { None }).collect();
    assert!(texts.iter().any(|t| t.galley.text() == "DATEDIF("), "signature painted");
    let desc = texts.iter().find(|t| t.galley.text().contains("YM or YD")).expect("description with the units painted");
    assert!(desc.galley.rect.width() <= 400.0, "description wraps instead of widening the hint: {}", desc.galley.rect.width());
}
