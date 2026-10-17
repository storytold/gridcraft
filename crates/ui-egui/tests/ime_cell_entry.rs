//! IME composition can start directly on the selected cell, without F2 first.
use egui::{Context, Event, ImeEvent, RawInput, Rect, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn frame(ctx: &Context, app: &mut SheetApp, events: Vec<Event>) -> egui::FullOutput {
    let mut output =
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))), events, ..Default::default() }, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        });
    output.textures_delta.clear();
    output
}

fn app() -> (Context, SheetApp) {
    let ctx = Context::default();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "A1", "input": "original"})).unwrap();
    let mut app = SheetApp::new(session, Default::default());
    for _ in 0..4 {
        frame(&ctx, &mut app, vec![]);
    }
    (ctx, app)
}

fn preedit(text: &str) -> Event {
    let count = text.chars().count();
    Event::Ime(ImeEvent::Preedit { text: text.into(), active_range_chars: Some(count..count) })
}

fn cell(app: &SheetApp) -> String {
    app.session.active().unwrap().wb.active().unwrap().cell(CellRef::new(0, 0)).unwrap().input_text()
}

#[test]
fn selected_cell_enables_native_ime_before_an_editor_exists() {
    let (ctx, mut app) = app();
    let output = frame(&ctx, &mut app, vec![]);
    assert!(app.editor.is_none());
    let ime = output.platform_output.ime.expect("the focused grid must enable native IME");
    assert!(ime.rect.is_finite() && ime.cursor_rect.is_finite());
    assert!(app.grid.cells_rect.unwrap().intersects(ime.cursor_rect));
}

#[test]
fn korean_preedit_is_replaced_then_committed_once() {
    let (ctx, mut app) = app();
    let revision = app.session.active().unwrap().revision;
    for text in ["ㅎ", "하", "한"] {
        let output = frame(&ctx, &mut app, vec![preedit(text)]);
        assert!(!output.platform_output.ime.unwrap().should_interrupt_composition, "focus must not cancel the native composition");
        assert_eq!(app.editor.as_ref().map(|ed| ed.text.as_str()), Some(text));
        assert_eq!(cell(&app), "original", "composition must not change the workbook");
        assert_eq!(app.session.active().unwrap().revision, revision);
    }
    frame(&ctx, &mut app, vec![preedit(""), Event::Ime(ImeEvent::Commit("한".into()))]);
    frame(&ctx, &mut app, vec![preedit("ㄱ")]);
    frame(&ctx, &mut app, vec![preedit("그")]);
    frame(&ctx, &mut app, vec![preedit("글")]);
    frame(&ctx, &mut app, vec![preedit(""), Event::Ime(ImeEvent::Commit("글".into()))]);
    assert_eq!(app.editor.as_ref().unwrap().text, "한글");
    assert_eq!(cell(&app), "original");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(cell(&app), "한글");
}

#[test]
fn direct_ime_commit_starts_replacement_without_preedit() {
    let (ctx, mut app) = app();
    frame(&ctx, &mut app, vec![Event::Ime(ImeEvent::Commit("한국어".into()))]);
    assert_eq!(app.editor.as_ref().map(|ed| ed.text.as_str()), Some("한국어"));
    assert_eq!(cell(&app), "original");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(cell(&app), "한국어");
}

#[test]
fn empty_ime_events_do_not_start_replacing_a_cell() {
    let (ctx, mut app) = app();
    frame(&ctx, &mut app, vec![preedit(""), Event::Ime(ImeEvent::Commit(String::new()))]);
    assert!(app.editor.is_none());
    assert_eq!(cell(&app), "original");
}

#[test]
fn another_text_field_keeps_ime_without_starting_cell_entry() {
    let (ctx, mut app) = app();
    let name_box = egui::Id::new("gridcraft.name_box");
    ctx.memory_mut(|m| m.request_focus(name_box));
    frame(&ctx, &mut app, vec![preedit("한")]);
    assert!(app.editor.is_none(), "grid must not steal composition from the name box");
    assert_eq!(ctx.memory(|m| m.focused()), Some(name_box));
    assert_eq!(cell(&app), "original");
}

#[test]
fn committing_a_syllable_and_starting_the_next_in_one_frame_preserves_composition() {
    let (ctx, mut app) = app();
    frame(&ctx, &mut app, vec![preedit("한")]);
    let output = frame(&ctx, &mut app, vec![preedit(""), Event::Ime(ImeEvent::Commit("한".into())), preedit("ㄱ")]);
    assert_eq!(app.editor.as_ref().unwrap().text, "한ㄱ");
    assert!(!output.platform_output.ime.unwrap().should_interrupt_composition);
    frame(&ctx, &mut app, vec![preedit("글")]);
    frame(&ctx, &mut app, vec![preedit(""), Event::Ime(ImeEvent::Commit("글".into()))]);
    assert_eq!(app.editor.as_ref().unwrap().text, "한글");
}

#[test]
fn cancelling_preedit_leaves_no_uncommitted_syllable() {
    let (ctx, mut app) = app();
    let revision = app.session.active().unwrap().revision;
    frame(&ctx, &mut app, vec![preedit("한")]);
    frame(&ctx, &mut app, vec![preedit("")]);
    assert_eq!(app.editor.as_ref().unwrap().text, "");
    assert_eq!(cell(&app), "original");
    app.cancel_edit();
    assert_eq!(cell(&app), "original");
    assert_eq!(app.session.active().unwrap().revision, revision);
}

#[test]
fn ordinary_typing_still_inserts_once_with_caret_at_end() {
    let (ctx, mut app) = app();
    for text in ["a", "b", "c"] {
        frame(&ctx, &mut app, vec![Event::Text(text.into())]);
    }
    assert_eq!(app.editor.as_ref().unwrap().text, "abc");
    assert!(app.commit_edit(0, 0, false, false));
    assert_eq!(cell(&app), "abc");
}

#[test]
fn finished_cell_entry_leaves_grid_keyboard_shortcuts_available() {
    let (ctx, mut app) = app();
    frame(&ctx, &mut app, vec![Event::Ime(ImeEvent::Commit("한글".into()))]);
    assert!(app.commit_edit(0, 0, false, false));
    frame(&ctx, &mut app, vec![]);
    assert!(!ctx.text_edit_focused(), "an idle grid must not retain TextEdit state");
    frame(&ctx, &mut app, vec![preedit("새")]);
    frame(&ctx, &mut app, vec![preedit(""), Event::Ime(ImeEvent::Commit("새".into()))]);
    assert_eq!(app.editor.as_ref().unwrap().text, "새");
    assert_eq!(cell(&app), "한글");
}
