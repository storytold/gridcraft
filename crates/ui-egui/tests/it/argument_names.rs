//! Ctrl+Shift+A (Control+Shift+A on a Mac) after a function name inserts its argument names, as
//! in Excel.
use egui::{Context, Event, Key, Modifiers, RawInput, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

fn frame(ctx: &Context, app: &mut SheetApp, time: f64, events: Vec<Event>) {
    let mut output = ctx.run_ui(
        RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), time: Some(time), events, ..Default::default() },
        |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
    );
    output.textures_delta.clear();
}

fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
}

fn ctrl_shift_a() -> Event {
    key(Key::A, Modifiers::CTRL | Modifiers::SHIFT)
}

/// A cell editor holding `text`, caret at the end.
fn editing(text: &str) -> (Context, SheetApp) {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    for t in 0..4 {
        frame(&ctx, &mut app, f64::from(t) * 0.01, vec![]);
    }
    app.begin_edit(Some(text.into()), false);
    for t in 4..8 {
        frame(&ctx, &mut app, f64::from(t) * 0.01, vec![]);
    }
    (ctx, app)
}

fn text(app: &SheetApp) -> &str {
    app.editor.as_ref().map(|e| e.text.as_str()).unwrap_or("<no editor>")
}

#[test]
fn after_the_opening_parenthesis_inserts_the_argument_names() {
    let (ctx, mut app) = editing("=SUMIF(");
    frame(&ctx, &mut app, 0.1, vec![ctrl_shift_a()]);
    frame(&ctx, &mut app, 0.11, vec![]);
    assert_eq!(text(&app), "=SUMIF(range,criteria,sum_range)");
}

#[test]
fn after_a_bare_function_name_also_adds_the_parentheses() {
    for (typed, expected) in
        [("=SUMIF", "=SUMIF(range,criteria,sum_range)"), ("=1+sumif", "=1+sumif(range,criteria,sum_range)"), ("=TODAY", "=TODAY()")]
    {
        let (ctx, mut app) = editing(typed);
        frame(&ctx, &mut app, 0.1, vec![ctrl_shift_a()]);
        frame(&ctx, &mut app, 0.11, vec![]);
        assert_eq!(text(&app), expected, "after {typed}");
    }
}

#[test]
fn anywhere_else_the_shortcut_changes_nothing() {
    for typed in ["=SUMIF(A1", "=SUMIF(A1,", "=NOTAFUNCTION", "=A1+", "=\"SUMIF(", "SUMIF("] {
        let (ctx, mut app) = editing(typed);
        frame(&ctx, &mut app, 0.1, vec![ctrl_shift_a()]);
        frame(&ctx, &mut app, 0.11, vec![]);
        assert_eq!(text(&app), typed, "after {typed}");
    }
}

#[test]
fn one_undo_takes_back_the_inserted_names_only() {
    let (ctx, mut app) = editing("=SUMI");
    // Typed just before the shortcut: the text edit's own undo has not settled on it yet.
    frame(&ctx, &mut app, 0.1, vec![Event::Text("F(".into())]);
    frame(&ctx, &mut app, 0.11, vec![]);
    assert_eq!(text(&app), "=SUMIF(");
    frame(&ctx, &mut app, 0.12, vec![ctrl_shift_a()]);
    frame(&ctx, &mut app, 0.13, vec![]);
    assert_eq!(text(&app), "=SUMIF(range,criteria,sum_range)");
    frame(&ctx, &mut app, 0.14, vec![key(Key::Z, Modifiers::COMMAND)]);
    frame(&ctx, &mut app, 0.15, vec![]);
    assert_eq!(text(&app), "=SUMIF(");
}
