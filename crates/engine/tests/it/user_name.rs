//! File › Options › General › User name (`options.userName`, #1449): the name that signs new notes
//! and comments and becomes the author of new workbooks, kept with the preferences.

use gridcraft_engine::{Prefs, Session, core::CellRef};
use serde_json::{Value as Json, json};

fn user_name(s: &mut Session, params: Json) -> Result<String, String> {
    s.execute("options.userName", params).map(|r| r["name"].as_str().unwrap_or_default().to_string()).map_err(|e| e.to_string())
}

#[test]
fn the_user_name_is_reported_and_set_without_a_workbook() {
    let mut s = Session::new();
    assert!(s.active().is_none());
    assert_eq!(user_name(&mut s, json!({})), Ok("GridCraft User".into()), "the default, with no workbook open");
    assert_eq!(user_name(&mut s, json!({"name": null})), Ok("GridCraft User".into()), "a null name only reports");
    // Control characters become spaces and the ends are trimmed.
    assert_eq!(user_name(&mut s, json!({"name": "  Ann\tLee \n"})), Ok("Ann Lee".into()));
    assert_eq!(s.prefs.user_name, "Ann Lee");
    assert_eq!(user_name(&mut s, json!({"name": "Ann\u{0}Lee\u{7}"})), Ok("Ann Lee".into()));
    assert_eq!(user_name(&mut s, json!({})), Ok("Ann Lee".into()));
    assert!(s.journal.iter().all(|(id, _)| id != "options.userName"), "a preference, not a step of any workbook");
}

#[test]
fn a_blank_or_non_text_name_is_refused_and_changes_nothing() {
    let mut s = Session::new();
    s.new_workbook();
    user_name(&mut s, json!({"name": "Ann Lee"})).unwrap();
    for bad in [json!(""), json!("   "), json!(" \t\r\n "), json!("\u{0}\u{1b}"), json!(42), json!(true), json!(["Bob"]), json!({"name": "Bob"})] {
        let r = user_name(&mut s, json!({ "name": bad }));
        assert!(r.as_ref().is_err_and(|e| e.contains("`name`")), "{bad}: {r:?}");
        assert_eq!(s.prefs.user_name, "Ann Lee", "{bad} changes nothing");
    }
    let d = s.active().unwrap();
    assert!(!d.is_dirty() && d.undo.is_empty(), "the open workbook is neither changed nor given an undo step");
}

#[test]
fn a_long_name_is_cut_to_200_characters() {
    let mut s = Session::new();
    assert_eq!(user_name(&mut s, json!({"name": "x".repeat(500)})), Ok("x".repeat(200)));
    // Characters, not bytes.
    assert_eq!(user_name(&mut s, json!({"name": "é".repeat(500)})), Ok("é".repeat(200)));
    // A cut that ends on a space leaves no space at the end; leading spaces don't count.
    let name = format!("{}{} {}", " ".repeat(50), "a".repeat(199), "b".repeat(300));
    assert_eq!(user_name(&mut s, json!({ "name": name })), Ok("a".repeat(199)));
    assert_eq!(gridcraft_engine::cmd::file::MAX_USER_NAME_CHARS, 200);
}

#[test]
fn new_notes_comments_and_workbooks_take_the_name() {
    let mut s = Session::new();
    let first = s.new_workbook();
    assert_eq!(s.documents()[first].wb.props.author, "GridCraft User");
    user_name(&mut s, json!({"name": "  Ann\tLee \n"})).unwrap();
    assert_eq!(s.documents()[first].wb.props.author, "GridCraft User", "an open workbook keeps its author");

    s.execute("review.newNote", json!({"cell": "A1", "text": "a note"})).unwrap();
    s.execute("review.newComment", json!({"cell": "B1", "text": "a comment"})).unwrap();
    s.execute("review.replyComment", json!({"cell": "B1", "text": "a reply"})).unwrap();
    s.execute("review.newNote", json!({"cell": "C1", "text": "named", "author": "Bob"})).unwrap();
    let sheet = s.active().unwrap().wb.active().unwrap();
    let author = |cell: &str| sheet.comments.get(&CellRef::parse(cell).unwrap()).map(|c| c.author.clone());
    assert_eq!(author("A1").as_deref(), Some("Ann Lee"), "the note");
    assert_eq!(author("B1").as_deref(), Some("Ann Lee"), "the comment");
    assert_eq!(sheet.comments.get(&CellRef::parse("B1").unwrap()).unwrap().replies, vec![("Ann Lee".to_string(), "a reply".to_string())]);
    assert_eq!(author("C1").as_deref(), Some("Bob"), "an `author` given to the command still wins");

    let second = s.new_workbook();
    assert_eq!(s.documents()[second].wb.props.author, "Ann Lee", "the new workbook's author");
}

#[test]
fn the_preferences_file_keeps_the_name() {
    let mut s = Session::new();
    user_name(&mut s, json!({"name": "Ann Lee"})).unwrap();
    // The desktop writes `Session::prefs` to prefs.json and the web app to `localStorage`.
    let saved = serde_json::to_value(&s.prefs).unwrap();
    assert_eq!(saved["userName"], "Ann Lee");
    let mut next = Session::new();
    next.prefs = serde_json::from_value::<Prefs>(saved).unwrap();
    assert_eq!(user_name(&mut next, json!({})), Ok("Ann Lee".into()));
    let book = next.new_workbook();
    assert_eq!(next.documents()[book].wb.props.author, "Ann Lee");
}
