//! Access modes (JD Sheets P2-MODE-02, -04, -05, -06, -07).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use gridcraft_engine::Session;
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

use super::*;

fn app(author: &str) -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.session.prefs.user_name = author.into();
    app
}

fn wb(app: &SheetApp) -> Arc<Workbook> {
    app.session.active().unwrap().wb.clone()
}

/// Runs a command the way the ribbon and menus do, then ends the frame.
fn menu(app: &mut SheetApp, gate: &mut Gate, id: &str, params: serde_json::Value) -> bool {
    let _ = app.run(id, params);
    gate.enforce(app)
}

/// Runs a command the way the grid and keyboard shortcuts do (straight to the engine).
fn key(app: &mut SheetApp, gate: &mut Gate, id: &str, params: serde_json::Value) -> bool {
    let _ = app.session.run(id, params);
    gate.enforce(app)
}

fn comment_at(app: &SheetApp, a1: &str) -> Option<Comment> {
    let at = gridcraft_engine::core::CellRef::parse(a1).unwrap();
    wb(app).sheets[0].comments.get(&at).cloned()
}

#[test]
fn modes_parse() {
    assert_eq!(Mode::parse("view"), Some(Mode::View));
    assert_eq!(Mode::parse("comment"), Some(Mode::Comment));
    assert_eq!(Mode::parse("edit"), Some(Mode::Edit));
    assert_eq!(Mode::parse("VIEW"), None);
    assert!(!Mode::View.saves() && Mode::Comment.saves() && Mode::Edit.saves());
}

#[test]
fn view_mode_puts_back_every_change_whatever_the_path() {
    let mut app = app("Asha");
    let mut gate = Gate::new(Mode::View);
    assert!(!gate.enforce(&mut app));
    let start = wb(&app);
    let changes: Vec<(&str, serde_json::Value)> = vec![
        ("cell.set", json!({ "cell": "A1", "input": "typed" })),
        ("home.bold", json!({ "range": "A1" })),
        ("home.insertSheet", json!({})),
        ("review.newComment", json!({ "cell": "B2", "text": "hi", "author": "Asha" })),
        ("range.setValues", json!({ "range": "A1:B1", "values": [["a", "b"]] })),
    ];
    for (id, params) in &changes {
        assert!(menu(&mut app, &mut gate, id, params.clone()), "{id} by menu");
        assert_eq!(*wb(&app), *start, "{id} by menu");
        assert!(key(&mut app, &mut gate, id, params.clone()), "{id} by key");
        assert_eq!(*wb(&app), *start, "{id} by key");
    }
    // Undo cannot bring a change back either, and nothing counts as unsaved.
    let _ = app.session.run("edit.undo", json!({}));
    gate.enforce(&mut app);
    assert_eq!(*wb(&app), *start);
    assert!(!app.session.active().unwrap().is_dirty());
    assert_eq!(app.toast.as_ref().map(|(text, _)| text.as_str()), Some(VIEW_NOTICE));
}

#[test]
fn looking_around_is_never_put_back() {
    let mut app = app("Asha");
    let _ = app.session.run("home.insertSheet", json!({}));
    let mut gate = Gate::new(Mode::View);
    gate.enforce(&mut app);
    for (id, params) in
        [("sheet.activate", json!({ "sheet": 0 })), ("selection.set", json!({ "range": "C7" })), ("sheet.activate", json!({ "sheet": 1 }))]
    {
        assert!(!key(&mut app, &mut gate, id, params), "{id}");
    }
    assert_eq!(wb(&app).active_sheet(), 1);
    assert!(app.toast.is_none());
}

#[test]
fn comment_mode_keeps_own_comments_only() {
    let mut app = app("Meera");
    let _ = app.session.run("review.newComment", json!({ "cell": "A1", "text": "Meera's", "author": "Meera" }));
    app.session.prefs.user_name = "Asha".into();
    let mut gate = Gate::new(Mode::Comment);
    gate.enforce(&mut app);

    // Own comments and notes: kept.
    assert!(!menu(&mut app, &mut gate, "review.newComment", json!({ "cell": "B2", "text": "Mine" })));
    assert_eq!(comment_at(&app, "B2").map(|c| c.author), Some("Asha".into()));
    assert!(!menu(&mut app, &mut gate, "review.newNote", json!({ "cell": "C3", "text": "Note" })));
    assert!(comment_at(&app, "C3").is_some());
    // A reply to someone else's thread: kept.
    assert!(!menu(&mut app, &mut gate, "review.replyComment", json!({ "cell": "A1", "text": "Agreed" })));
    assert_eq!(comment_at(&app, "A1").unwrap().replies.len(), 1);
    // Deleting someone else's comment: put back.
    assert!(menu(&mut app, &mut gate, "review.deleteComment", json!({ "cell": "A1" })));
    assert_eq!(comment_at(&app, "A1").map(|c| c.text), Some("Meera's".into()));
    // Deleting your own: kept.
    assert!(!menu(&mut app, &mut gate, "review.deleteComment", json!({ "cell": "B2" })));
    assert!(comment_at(&app, "B2").is_none());
    // Content: put back, with the notice for comment mode.
    let before = wb(&app);
    assert!(key(&mut app, &mut gate, "cell.set", json!({ "cell": "D4", "input": "no" })));
    assert_eq!(*wb(&app), *before);
    assert_eq!(app.toast.as_ref().map(|(text, _)| text.as_str()), Some(COMMENT_NOTICE));
}

#[test]
fn edit_mode_is_gridcraft_as_it_is() {
    let mut app = app("Asha");
    let mut gate = Gate::new(Mode::Edit);
    for (id, params) in [("cell.set", json!({ "cell": "A1", "input": "typed" })), ("home.insertSheet", json!({})), ("edit.undo", json!({}))] {
        assert!(!menu(&mut app, &mut gate, id, params), "{id}");
    }
    assert!(app.session.active().unwrap().is_dirty());
    assert!(app.toast.is_none());
}

#[test]
fn switching_mode_takes_effect_at_once_and_keeps_the_view() {
    let mut app = app("Asha");
    let mut gate = Gate::new(Mode::Edit);
    assert!(!menu(&mut app, &mut gate, "cell.set", json!({ "cell": "A1", "input": "mine" })));
    let _ = app.session.run("selection.set", json!({ "range": "B5" }));
    gate.set_mode(Mode::View);
    gate.enforce(&mut app);
    let edited = wb(&app);
    assert!(menu(&mut app, &mut gate, "cell.set", json!({ "cell": "A1", "input": "other" })));
    assert_eq!(*wb(&app), *edited, "the work done in edit mode stays");
    assert_eq!(app.session.active().unwrap().selection.active.a1(), "B5");
    gate.set_mode(Mode::Edit);
    assert!(!menu(&mut app, &mut gate, "cell.set", json!({ "cell": "A1", "input": "again" })));
}
