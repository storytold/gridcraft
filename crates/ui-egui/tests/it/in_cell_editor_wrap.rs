//! The in-cell editor of a Wrap Text cell keeps the column's width and wraps, growing down, as
//! Excel's does; it used to widen to one long line.
use egui::{Context, Id, RawInput, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

const TEXT: &str = "Since the 1960s, NASA has left hardware across the solar system, on the Moon, on Mars, and in deep space.";

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

/// The editor's text field rect while A1 (narrow, wrap on or off) is edited with [`TEXT`].
fn editor_rect(wrap: bool) -> Rect {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.columnWidth", json!({"cols": "A:A", "chars": 12})).unwrap();
    session.execute("home.wrapText", json!({"range": "A1", "on": wrap})).unwrap();
    let mut app = crate::common::app(session);
    frames(&ctx, &mut app);
    app.begin_edit(Some(TEXT.into()), false);
    frames(&ctx, &mut app);
    ctx.read_response(Id::unique("gridcraft.cell_editor")).expect("the in-cell editor is shown").rect
}

#[test]
fn a_wrap_text_cell_edits_wrapped_at_the_column_width() {
    let wrapped = editor_rect(true);
    let one_line = editor_rect(false);
    assert!(one_line.width() > 400.0, "without Wrap Text the editor widens to the text: {one_line:?}");
    assert!(wrapped.width() < 120.0, "with Wrap Text the editor keeps the column's width: {wrapped:?}");
    assert!(wrapped.height() > 4.0 * one_line.height(), "and grows down to show every line: {wrapped:?} vs {one_line:?}");
}

/// A short Wrap Text edit stays one line tall: the field doesn't reserve rows over the cells below.
#[test]
fn a_short_wrap_text_edit_stays_one_line_tall() {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let mut app = crate::common::app(session);
    frames(&ctx, &mut app);
    app.begin_edit(Some("short".into()), false);
    frames(&ctx, &mut app);
    let field = ctx.read_response(Id::unique("gridcraft.cell_editor")).expect("the in-cell editor is shown").rect;
    assert!(field.height() < 30.0, "a one-line edit doesn't reach over the rows below: {field:?}");
}

/// The editor is sized from the text wrapped where the field wraps it: the editor's width less its
/// 2 px inset a side, with no text margin (the field has no frame). Measured narrower, the editor
/// would count rows the field doesn't have and reach over the cells below.
#[test]
fn the_editor_measures_the_text_at_the_fields_wrap_width() {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.columnWidth", json!({"cols": "A:A", "chars": 12})).unwrap();
    session.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
    let mut app = crate::common::app(session);
    frames(&ctx, &mut app);
    app.begin_edit(Some(TEXT.into()), false);
    frames(&ctx, &mut app);
    let field = ctx.read_response(Id::unique("gridcraft.cell_editor")).expect("the in-cell editor is shown").rect;
    let sheet = app.session.active().unwrap().wb.active().unwrap().clone();
    let st = app.session.active().unwrap().wb.styles.get(sheet.style_id(gridcraft_engine::cell(0, 0))).clone();
    // The grid draws at its zoom: the cell is the field plus the 2 px inset a side, the font scales.
    let z = (field.width() + 4.0) / sheet.col_width(0);
    let font = egui::FontId::new(st.font.size * 96.0 / 72.0 * z, gridcraft_ui_egui::theme::cell_family(&st.font.name, st.font.bold, st.font.italic));
    let at =
        |w: f32| ctx.fonts_mut(|f| f.layout_job(egui::text::LayoutJob::simple(TEXT.into(), font.clone(), egui::Color32::PLACEHOLDER, w)).size().y);
    let (own, with_margin) = (at(field.width()), at(field.width() - 8.0));
    assert!(own != with_margin, "the test text must wrap differently 8 px narrower");
    assert!((field.height() - own).abs() < 1.0, "the field wraps at its whole width, no text margin: {} vs {own}", field.height());
}

/// An edit with line breaks (Alt+Enter) in a cell without Wrap Text shows every line: the field's
/// one row is a minimum, it grows with the text and never scrolls.
#[test]
fn an_unwrapped_multiline_edit_shows_every_line() {
    let field = |text: &str| {
        let ctx = Context::default();
        let mut session = Session::new();
        session.new_workbook();
        let mut app = crate::common::app(session);
        frames(&ctx, &mut app);
        app.begin_edit(Some(text.into()), false);
        frames(&ctx, &mut app);
        ctx.read_response(Id::unique("gridcraft.cell_editor")).expect("the in-cell editor is shown").rect
    };
    let one = field("one").height();
    let two = field("one\ntwo").height();
    let three = field("one\ntwo\nthree").height();
    let four = field("one\ntwo\nthree\nfour").height();
    // egui 0.37 gives a one-line TextEdit a widget-height floor, so compare the increments from
    // two lines up rather than a fraction of the one-line field.
    let line = three - two;
    assert!(line > 0.0, "a third line adds height: {two} then {three}");
    assert!((four - three - line).abs() < 1.0, "each extra line takes the same height: {two}, {three}, {four}");
    assert!(three > one + line / 2.0, "the third line is taller than the one-line floor: {one} then {three}");
    assert!(four < three + line + 1.0, "and no more rows are reserved: {three} then {four}");
}
