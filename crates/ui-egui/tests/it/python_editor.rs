//! The Python editor window: its routes in and out, editing and applying the code through
//! `python.set`, the questions before unsaved code would be lost, the limit while typing, the
//! line-number gutter, multi-byte text, the control channel, and small windows.

use egui::{Event, Key, Modifiers, Pos2, Rect, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::python::MAX_FORMULA_CHARS;
use gridcraft_ui_egui::python_window::{self, APPLY, CANCEL, CLOSE, DISCARD, EDIT_IN_EDITOR, Pending, PyWindow};
use serde_json::json;

use crate::common::Harness;

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// B2 holds two lines of code.
fn with_code() -> Session {
    let mut s = blank();
    s.execute("python.set", json!({"cell": "B2", "code": "x = 1\nx + 1"})).unwrap();
    s
}

fn harness(session: Session) -> Harness {
    sized(session, 1200.0, 800.0)
}

fn sized(session: Session, w: f32, h: f32) -> Harness {
    crate::common::harness_with(crate::common::builder().with_size(vec2(w, h)), crate::common::app(session), 4)
}

fn press(key: Key, modifiers: Modifiers) -> [Event; 2] {
    [
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
    ]
}

fn send(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(3);
}

fn key(h: &mut Harness, key: Key, modifiers: Modifiers) {
    send(h, press(key, modifiers));
}

fn type_text(h: &mut Harness, text: &str) {
    send(h, [Event::Text(text.into())]);
}

fn win(h: &Harness) -> Option<PyWindow> {
    h.state().py_window.clone()
}

fn text(h: &Harness) -> String {
    win(h).map(|w| w.text).unwrap_or_default()
}

fn code(h: &Harness, cell: &str) -> Option<String> {
    let d = h.state().session.active()?;
    d.wb.active()?.cell(CellRef::parse(cell)?)?.py.as_ref().map(|p| p.code.clone())
}

fn ui(h: &mut Harness, p: serde_json::Value) -> Result<serde_json::Value, String> {
    let r = h.state_mut().run("ui.pythonEditor", p);
    h.run_steps(3);
    r
}

/// Puts the window's caret at the end of the code.
fn to_end(h: &mut Harness) {
    if let Some(w) = h.state_mut().py_window.as_mut() {
        w.caret = Some(w.text.chars().count());
    }
    h.run_steps(2);
}

fn select(h: &mut Harness, cell: &str) {
    h.state_mut().session.execute("selection.set", json!({"cell": cell})).unwrap();
    h.run_steps(2);
}

/// Clicks the widget labelled `label` inside `area`.
fn click_in(h: &mut Harness, area: Rect, label: &str) {
    let node = h.query_all_by_label(label).find(|n| area.contains(n.rect().center()));
    let Some(node) = node else { panic!("no {label:?} in {area:?}") };
    node.click();
    h.run_steps(3);
}

/// Clicks a button of the editor window.
fn click(h: &mut Harness, label: &str) {
    let area = win(h).and_then(|w| w.rect).expect("the window is on screen");
    click_in(h, area, label);
}

#[test]
fn the_window_opens_and_closes_by_each_route() {
    let mut h = harness(with_code());
    // The control channel, without a dialog.
    let r = ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    assert_eq!((r["open"].as_bool(), r["cell"].as_str()), (Some(true), Some("B2")));
    assert_eq!(text(&h), "x = 1\nx + 1");
    assert!(h.state().dialog.is_none() && h.state().message.is_none());
    assert!(h.query_all_by_label("Python Editor").count() >= 1, "the window shows");
    h.check_accessibility();
    assert_eq!(ui(&mut h, json!({"on": false})).unwrap()["open"], false);
    assert!(win(&h).is_none());
    // The formula bar's button, on a selected Python cell; Esc closes the window.
    select(&mut h, "B2");
    h.get_by_label(EDIT_IN_EDITOR).click();
    h.run_steps(3);
    assert_eq!(win(&h).map(|w| w.cell), Some(CellRef::new(1, 1)));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(win(&h).is_none(), "Esc closes an editor without changes");
    // The cell menu's entry; the Close button.
    h.state_mut().grid.context_menu = Some(Pos2::new(300.0, 300.0));
    h.run_steps(3);
    let menu = h.ctx.memory(|m| m.area_rect(egui::Id::unique("cell_context"))).expect("the cell menu");
    click_in(&mut h, menu, EDIT_IN_EDITOR);
    assert_eq!(win(&h).map(|w| w.cell), Some(CellRef::new(1, 1)));
    assert!(h.state().grid.context_menu.is_none(), "the menu closed");
    click(&mut h, CLOSE);
    assert!(win(&h).is_none());
    // No button and no menu entry on an ordinary cell.
    select(&mut h, "D4");
    assert!(h.query_by_label(EDIT_IN_EDITOR).is_none());
    // Toggling from the control channel: without `on` it opens, then closes.
    assert_eq!(ui(&mut h, json!({})).unwrap()["open"], true);
    assert_eq!(win(&h).map(|w| w.cell), Some(CellRef::new(3, 3)), "on the active cell");
    assert_eq!(ui(&mut h, json!({})).unwrap()["open"], false);
}

#[test]
fn python_mode_moves_its_edit_into_the_window() {
    let mut h = harness(with_code());
    select(&mut h, "B2");
    key(&mut h, Key::F2, Modifiers::NONE);
    assert!(h.state().editor.as_ref().is_some_and(|e| e.python));
    key(&mut h, Key::End, Modifiers::COMMAND);
    type_text(&mut h, " # more");
    h.get_by_label(EDIT_IN_EDITOR).click();
    h.run_steps(3);
    assert!(h.state().editor.is_none(), "the formula bar's edit ended");
    let w = win(&h).expect("open");
    assert_eq!(w.text, "x = 1\nx + 1 # more", "with the code as typed");
    assert!(w.dirty());
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1"), "nothing applied yet");
    // The keyboard is in the window: typing goes on where the caret was.
    type_text(&mut h, "!");
    assert_eq!(text(&h), "x = 1\nx + 1 # more!");
}

#[test]
fn enter_tab_and_ctrl_enter_or_apply_commit_through_python_set() {
    let mut h = harness(blank());
    select(&mut h, "C3");
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "for i in range(3):");
    key(&mut h, Key::Enter, Modifiers::NONE);
    type_text(&mut h, "print(i)");
    key(&mut h, Key::Enter, Modifiers::NONE);
    key(&mut h, Key::Tab, Modifiers::SHIFT);
    type_text(&mut h, "i");
    assert_eq!(text(&h), "for i in range(3):\n    print(i)\ni", "Enter keeps the indentation, Shift+Tab takes it back");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "C3").as_deref(), Some("for i in range(3):\n    print(i)\ni"), "Ctrl+Enter applied it");
    let w = win(&h).expect("the window stays open");
    assert!(!w.dirty());
    // Apply, after more typing.
    type_text(&mut h, " * 2");
    assert!(win(&h).is_some_and(|w| w.dirty()));
    assert!(h.query_by_label("Unsaved changes").is_some());
    click(&mut h, APPLY);
    assert_eq!(code(&h, "C3").as_deref(), Some("for i in range(3):\n    print(i)\ni * 2"));
    assert!(win(&h).is_some_and(|w| !w.dirty()));
    // An undo changes the cell: the window, with no changes of its own, shows it.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(text(&h), "for i in range(3):\n    print(i)\ni");
    // Each apply is one undo step, through python.set.
    let journal: Vec<String> = h.state().session.journal.iter().map(|(id, _)| id.clone()).collect();
    assert_eq!(journal.iter().filter(|id| *id == "python.set").count(), 2, "{journal:?}");
}

#[test]
fn esc_with_changes_asks_first() {
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, "\ny = 2");
    key(&mut h, Key::Escape, Modifiers::NONE);
    let w = win(&h).expect("still open");
    assert_eq!(w.pending, Some(Pending::Close), "it asks");
    assert!(h.query_by_label("The code for Sheet1!B2 has unsaved changes.").is_some());
    // Typing goes nowhere while it asks.
    type_text(&mut h, "zzz");
    assert_eq!(text(&h), "x = 1\nx + 1\ny = 2");
    assert_eq!(code(&h, "D1"), None);
    // Cancel (and Esc) go back to the code.
    click(&mut h, CANCEL);
    assert_eq!(win(&h).and_then(|w| w.pending), None);
    type_text(&mut h, "3");
    assert_eq!(text(&h), "x = 1\nx + 1\ny = 23", "the keyboard is back in the code");
    click(&mut h, CLOSE);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Close), "Close asks too");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(win(&h).and_then(|w| w.pending), None, "Esc answers Cancel");
    // Discard closes and changes nothing.
    key(&mut h, Key::Escape, Modifiers::NONE);
    click(&mut h, DISCARD);
    assert!(win(&h).is_none());
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1"));
    // Apply puts the code in, then closes.
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    type_text(&mut h, "# top\n");
    key(&mut h, Key::Escape, Modifiers::NONE);
    click(&mut h, APPLY);
    assert!(win(&h).is_none());
    assert_eq!(code(&h, "B2").as_deref(), Some("# top\nx = 1\nx + 1"));
}

#[test]
fn a_refused_commit_keeps_the_code_and_shows_the_message() {
    let mut s = blank();
    s.execute("insert.table", json!({"range": "A1:B3", "header": true})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "A1"})).unwrap();
    type_text(&mut h, "len('abc')");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    let w = win(&h).expect("still open");
    assert_eq!(w.text, "len('abc')", "the code is kept");
    assert!(w.dirty());
    let notice = python_window::state(h.state())["notice"].as_str().unwrap_or_default().to_string();
    assert!(notice.contains("table header"), "the command's message: {notice}");
    assert!(h.query_by_label(&notice).is_some(), "shown under the code");
    assert_eq!(code(&h, "A1"), None);
    assert!(h.state().message.is_none(), "in the window, not a message box over it");
    // Apply from the question refuses the same way and keeps the window open.
    key(&mut h, Key::Escape, Modifiers::NONE);
    click(&mut h, APPLY);
    assert!(win(&h).is_some_and(|w| w.text == "len('abc')" && w.pending.is_none()));
    // Typing again clears the message.
    type_text(&mut h, " ");
    assert_eq!(text(&h), "len('abc') ");
    assert_eq!(python_window::state(h.state())["notice"], serde_json::Value::Null);
}

/// The longest run of `c` that fits in a cell.
fn longest(c: char) -> String {
    let n = (0..=MAX_FORMULA_CHARS).rev().find(|n| python_window::fits(&c.to_string().repeat(*n))).unwrap_or(0);
    c.to_string().repeat(n)
}

#[test]
fn typing_and_pasting_past_the_limit_are_refused_with_a_message() {
    let mut h = harness(blank());
    ui(&mut h, json!({"on": true})).unwrap();
    let full = longest('y');
    let n = full.chars().count();
    let at = full[..n - 3].to_string();
    if let Some(w) = h.state_mut().py_window.as_mut() {
        w.text = at.clone();
        w.caret = Some(n - 3);
    }
    h.run_steps(2);
    // A paste that would go past the limit is refused whole.
    send(&mut h, [Event::Paste("abcd".into())]);
    assert_eq!(text(&h), at, "nothing of the paste went in");
    let notice = python_window::state(h.state())["notice"].as_str().unwrap_or_default().to_string();
    assert!(notice.contains(&MAX_FORMULA_CHARS.to_string()), "{notice}");
    assert!(h.query_by_label(&notice).is_some());
    // Typing up to the limit works, one more character doesn't; each keystroke on its own.
    send(&mut h, [Event::Text("a".into()), Event::Text("b".into()), Event::Text("c".into()), Event::Text("d".into())]);
    assert_eq!(text(&h), format!("{at}abc"), "three fit, the fourth is refused");
    assert!(python_window::fits(&text(&h)));
    // Enter (with indentation) past the limit is refused too; Backspace still works and clears
    // the message.
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(text(&h).chars().count(), n);
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(text(&h).chars().count(), n - 1);
    assert_eq!(python_window::state(h.state())["notice"], serde_json::Value::Null);
    // Quotes count twice in the formula: one more fits no longer.
    type_text(&mut h, "\"");
    assert_eq!(text(&h).chars().count(), n - 1, "a quote needs two places");
    // The applied code is what python.set accepts.
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "A1").map(|c| c.chars().count()), Some(n - 1));
}

#[test]
fn multi_byte_text_goes_through_edits_without_a_panic() {
    let mut h = harness(blank());
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "café = 'Crème brûlée'  # 日本語 \u{1F600}");
    key(&mut h, Key::Enter, Modifiers::NONE);
    type_text(&mut h, "if café:");
    key(&mut h, Key::Enter, Modifiers::NONE);
    type_text(&mut h, "x = '字'");
    assert_eq!(text(&h), "café = 'Crème brûlée'  # 日本語 \u{1F600}\nif café:\n    x = '字'");
    // Caret moves, selections, deletions and indents across the multi-byte characters.
    for (k, m) in [
        (Key::ArrowLeft, Modifiers::NONE),
        (Key::ArrowLeft, Modifiers::SHIFT),
        (Key::Backspace, Modifiers::NONE),
        (Key::ArrowUp, Modifiers::NONE),
        (Key::Home, Modifiers::NONE),
        (Key::Tab, Modifiers::NONE),
        (Key::ArrowUp, Modifiers::NONE),
        (Key::End, Modifiers::NONE),
        (Key::Backspace, Modifiers::NONE),
        (Key::Delete, Modifiers::NONE),
        (Key::A, Modifiers::COMMAND),
        (Key::Tab, Modifiers::NONE),
        (Key::Tab, Modifiers::SHIFT),
        (Key::ArrowRight, Modifiers::NONE),
    ] {
        key(&mut h, k, m);
    }
    // Backspace took the emoji, Delete the line break after it (joining the indented line).
    assert_eq!(text(&h), "café = 'Crème brûlée'  # 日本語     if café:\n    x = ''");
    // A paste with Windows line breaks keeps code's \n.
    send(&mut h, [Event::Paste("\r\né\r\n字".into())]);
    assert!(!text(&h).contains('\r'));
    assert!(text(&h).ends_with("\né\n字"), "{:?}", text(&h));
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "A1"), Some(text(&h)));
    // Reopened from the formula bar with the caret after the emoji (a char index, not a byte).
    ui(&mut h, json!({"on": false})).unwrap();
    select(&mut h, "B1");
    h.state_mut().toggle_python_mode();
    h.run_steps(3);
    type_text(&mut h, "s = '\u{1F600}é字'");
    h.get_by_label(EDIT_IN_EDITOR).click();
    h.run_steps(3);
    type_text(&mut h, "!");
    assert_eq!(text(&h), "s = '\u{1F600}é字'!");
}

/// Opens the editor on code of `lines` lines and returns its gutter's width.
fn gutter_for(lines: usize) -> f32 {
    let code = (1..=lines).map(|i| format!("v{i} = {i}")).collect::<Vec<_>>().join("\n");
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": code})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "A1"})).unwrap();
    let painted: Vec<String> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == lines.to_string() || t.galley.job.text == "1" => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    assert!(painted.contains(&"1".to_string()), "the first line's number is painted");
    win(&h).map(|w| w.gutter).unwrap_or(0.0)
}

#[test]
fn the_gutter_widens_with_the_digit_count() {
    let (g9, g10, g99, g100) = (gutter_for(9), gutter_for(10), gutter_for(99), gutter_for(100));
    assert!(g9 > 0.0);
    assert!(g10 > g9, "two digits: {g9} then {g10}");
    assert_eq!(g10, g99, "still two digits");
    assert!(g100 > g99, "three digits: {g99} then {g100}");
    let digit = (g100 - g99).max(g10 - g9);
    assert!((g10 - g9 - digit).abs() < 0.01 && (g100 - g99 - digit).abs() < 0.01, "one digit's width each time");
}

#[test]
fn only_the_visible_line_numbers_are_painted() {
    let code = (1..=300).map(|i| format!("v{i} = {i}")).collect::<Vec<_>>().join("\n");
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": code})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "A1"})).unwrap();
    let numbers = |h: &Harness| -> Vec<usize> {
        let area = win(h).and_then(|w| w.rect).unwrap();
        h.output()
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if area.contains(t.pos) => t.galley.job.text.parse::<usize>().ok(),
                _ => None,
            })
            .collect()
    };
    let shown = numbers(&h);
    assert!(shown.contains(&1) && shown.len() < 80, "the first rows only: {}", shown.len());
    // The view follows the caret to the end, and the numbers there are painted.
    to_end(&mut h);
    type_text(&mut h, " ");
    let shown = numbers(&h);
    assert!(shown.contains(&300) && !shown.contains(&1), "{shown:?}");
}

#[test]
fn another_cell_with_unsaved_changes_asks_first() {
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "A1"})).unwrap();
    type_text(&mut h, "a = 1");
    // The cell menu on B2 asks; Cancel keeps A1's code.
    select(&mut h, "B2");
    python_window::open_from_menu(h.state_mut());
    h.run_steps(3);
    let w = win(&h).unwrap();
    assert_eq!((w.cell, w.text.as_str()), (CellRef::new(0, 0), "a = 1"));
    assert!(matches!(w.pending, Some(Pending::Open { .. })), "it asks");
    click(&mut h, CANCEL);
    assert_eq!(win(&h).map(|w| (w.cell, w.text)), Some((CellRef::new(0, 0), "a = 1".to_string())));
    // Discard opens B2 and leaves A1 as it was.
    python_window::open_from_menu(h.state_mut());
    h.run_steps(3);
    click(&mut h, DISCARD);
    assert_eq!(win(&h).map(|w| (w.cell, w.text)), Some((CellRef::new(1, 1), "x = 1\nx + 1".to_string())));
    assert_eq!(code(&h, "A1"), None);
    // Apply puts B2's changes in, then opens on the formula bar's cell with its code.
    type_text(&mut h, "# b2\n");
    select(&mut h, "C3");
    h.state_mut().toggle_python_mode();
    h.run_steps(3);
    type_text(&mut h, "c = 3");
    h.get_by_label(EDIT_IN_EDITOR).click();
    h.run_steps(3);
    assert!(h.state().editor.is_some(), "the formula bar keeps its edit while the window asks");
    click(&mut h, APPLY);
    assert_eq!(code(&h, "B2").as_deref(), Some("# b2\nx = 1\nx + 1"));
    assert_eq!(win(&h).map(|w| (w.cell, w.text)), Some((CellRef::new(2, 2), "c = 3".to_string())));
    assert!(h.state().editor.is_none(), "and its edit moved into the window");
    // The control channel never asks: it refuses, unless forced.
    let e = ui(&mut h, json!({"on": true, "cell": "D4"})).unwrap_err();
    assert!(e.contains("unsaved changes to Sheet1!C3"), "{e}");
    assert!(win(&h).is_some_and(|w| w.cell == CellRef::new(2, 2) && w.pending.is_none()));
    assert!(h.state().dialog.is_none() && h.state().message.is_none());
    assert_eq!(ui(&mut h, json!({"on": true, "cell": "Sheet1!D4", "force": true})).unwrap()["cell"], "D4");
    assert_eq!(code(&h, "C3"), None, "force discarded the code");
    // The same cell again keeps the window's code.
    type_text(&mut h, "d");
    ui(&mut h, json!({"on": true, "cell": "D4"})).unwrap();
    assert_eq!(text(&h), "d");
}

#[test]
fn the_control_channel_opens_without_a_prompt_and_refuses_to_close_unsaved_code() {
    let mut h = harness(with_code());
    assert!(ui(&mut h, json!({"on": true, "cell": "Nope!A1"})).is_err());
    assert!(ui(&mut h, json!({"on": true, "cell": "A0"})).is_err());
    assert!(ui(&mut h, json!({"on": "yes"})).is_err());
    assert!(win(&h).is_none(), "a bad request opens nothing");
    ui(&mut h, json!({"on": true, "cell": "b2"})).unwrap();
    type_text(&mut h, "1");
    let e = ui(&mut h, json!({"on": false})).unwrap_err();
    assert!(e.contains("force"), "{e}");
    let state = python_window::state(h.state());
    assert_eq!((state["open"].as_bool(), state["dirty"].as_bool(), state["pending"].as_str()), (Some(true), Some(true), None));
    assert!(h.state().dialog.is_none() && h.state().message.is_none(), "no question, no dialog");
    let inspect = gridcraft_ui_egui::control::inspect(h.state(), &h.ctx);
    assert_eq!(inspect["pythonEditor"]["cell"], "B2");
    assert_eq!(ui(&mut h, json!({"on": false, "force": true})).unwrap()["open"], false);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1"));
    // Every hostile parameter is an error or ignored, never a panic.
    for p in [json!(null), json!([]), json!({"cell": 5}), json!({"cell": ""}), json!({"cell": "Sheet1!"}), json!({"force": "x"})] {
        let _ = ui(&mut h, p);
        let _ = ui(&mut h, json!({"on": false, "force": true}));
    }
}

#[test]
fn closing_or_leaving_the_workbook_asks_about_unsaved_code() {
    let mut h = harness(blank());
    let first = h.state().session.active().unwrap().uid;
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "a = 1");
    // A programmatic close is refused; File › Close asks in the window.
    assert!(h.state_mut().run("file.close", json!({})).unwrap_err().contains("Python editor"));
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(3);
    assert!(matches!(win(&h).and_then(|w| w.pending), Some(Pending::CloseWorkbook(_))));
    click(&mut h, CANCEL);
    assert_eq!(h.state().session.documents().len(), 1, "Cancel keeps the workbook");
    // Switching to another workbook asks too; Cancel goes back.
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Switched));
    let title = h.state().session.documents()[0].display_title();
    assert!(h.query_by_label(&format!("The code for Sheet1!A1 in {title} has unsaved changes.")).is_some());
    click(&mut h, CANCEL);
    assert_eq!(h.state().session.active().map(|d| d.uid), Some(first), "back to the code's workbook");
    // Apply after a switch puts the code in its own workbook, not the active one.
    h.state_mut().session.set_active(1);
    h.run_steps(3);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Switched));
    click(&mut h, APPLY);
    assert!(win(&h).is_none());
    assert_eq!(code(&h, "A1"), None, "the active workbook has no code");
    let first_code = h.state().session.documents()[0].wb.sheet(0).and_then(|s| s.cell(CellRef::new(0, 0))).and_then(|c| c.py.clone());
    assert_eq!(first_code.map(|p| p.code.clone()).as_deref(), Some("a = 1"));
    // An editor without changes closes with its workbook, without asking.
    h.state_mut().session.set_active(0);
    ui(&mut h, json!({"on": true})).unwrap();
    h.state_mut().run_or_alert("file.close", json!({"force": true}));
    h.run_steps(3);
    assert!(win(&h).is_none());
    assert_eq!(h.state().session.documents().len(), 1);
    // With changes, Discard goes on to close the workbook (which asks about its own changes).
    let current = h.state().session.active().unwrap().uid;
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "b");
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(3);
    click(&mut h, DISCARD);
    assert!(win(&h).is_none());
    assert!(h.state().session.documents().iter().all(|d| d.uid != current), "the workbook the code was for is closed");
}

#[test]
fn closing_one_of_a_workbooks_windows_keeps_the_editors_code_without_asking() {
    let mut h = harness(blank());
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "a = 1");
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(h.state().session.windows().len(), 2);
    // File › Close closes only the active window: nothing is asked, and the code stays.
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(3);
    assert_eq!(h.state().session.windows().len(), 1, "one window closed");
    assert_eq!(win(&h).map(|w| (w.text, w.pending)), Some(("a = 1".to_string(), None)));
    // The same programmatically; closing the workbook itself is still refused.
    h.state_mut().run("view.newWindow", json!({})).unwrap();
    h.run_steps(3);
    assert!(h.state_mut().run("file.close", json!({"document": true})).unwrap_err().contains("Python editor"));
    assert_eq!(h.state_mut().run("file.close", json!({})).unwrap()["document"], false);
    assert_eq!(win(&h).map(|w| w.text), Some("a = 1".to_string()), "the code is kept");
}

#[test]
fn quitting_with_unsaved_code_asks_first() {
    let mut h = harness(blank());
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "q");
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    h.run_steps(2);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Quit));
    assert!(!h.state().allow_close);
    click(&mut h, CANCEL);
    assert!(win(&h).is_some_and(|w| w.pending.is_none() && w.text == "q"));
    // Discard goes on with the quit: no workbook is unsaved, so the app closes.
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    h.run_steps(2);
    click(&mut h, DISCARD);
    assert!(win(&h).is_none());
    assert!(h.state().allow_close, "the quit went on");
}

fn control(h: &mut Harness, method: &str, p: serde_json::Value) -> serde_json::Value {
    let ctx = h.ctx.clone();
    match gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, method, &p) {
        gridcraft_ui_egui::control::Outcome::Done(v) => v,
        gridcraft_ui_egui::control::Outcome::Screenshot { .. } | gridcraft_ui_egui::control::Outcome::Hold(_) => json!(null),
    }
}

#[test]
fn app_quit_over_the_control_channel_refuses_unsaved_code() {
    let mut h = harness(blank());
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "q");
    // Programmatic calls never ask: the quit is refused, and the code stays.
    let r = control(&mut h, "app.quit", json!({}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(r["error"].as_str().unwrap_or_default().contains("Python editor has unsaved changes to Sheet1!A1"), "{r}");
    assert!(!h.state().allow_close);
    assert!(win(&h).is_some_and(|w| w.pending.is_none() && w.text == "q"), "nothing asks");
    // Once the code is discarded, the quit goes through.
    ui(&mut h, json!({"on": false, "force": true})).unwrap();
    let r = control(&mut h, "app.quit", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert!(h.state().allow_close);
}

#[test]
fn the_window_stays_inside_a_600_by_400_app() {
    for language in ["en", "de"] {
        let mut h = sized(with_code(), 600.0, 400.0);
        h.state_mut().run("app.language.set", json!({"language": language})).unwrap();
        ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
        type_text(&mut h, "y");
        h.run_steps(4);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0));
        let area = win(&h).and_then(|w| w.rect).unwrap();
        assert!(screen.contains_rect(area), "{language}: {area:?}");
        let lang = h.state().ui.language;
        for label in [APPLY, CLOSE] {
            let node = h.query_all_by_label(lang.tr(label)).find(|n| area.contains(n.rect().center())).expect(label);
            assert!(area.contains_rect(node.rect()), "{language}: {label} at {:?} in {area:?}", node.rect());
        }
        // The question's buttons too.
        key(&mut h, Key::Escape, Modifiers::NONE);
        for label in [APPLY, DISCARD, CANCEL] {
            let node = h.query_all_by_label(lang.tr(label)).find(|n| area.contains(n.rect().center())).expect(label);
            assert!(screen.contains_rect(node.rect()), "{language}: {label} at {:?}", node.rect());
        }
    }
    // Opened in a large app window, it shrinks with it.
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    let big = win(&h).and_then(|w| w.rect).unwrap();
    h.set_size(vec2(600.0, 400.0));
    h.run_steps(4);
    let small = win(&h).and_then(|w| w.rect).unwrap();
    assert!(small.height() < big.height());
    assert!(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0)).contains_rect(small), "{small:?}");
}

#[test]
#[allow(clippy::disallowed_methods)] // the clock is read off wasm, in a test
fn code_at_the_limit_stays_fast() {
    // About 8,000 characters in 200 lines.
    let mut lines = Vec::new();
    let mut i = 0;
    loop {
        let next = format!("v{i} = round(v{} * 1.05, 2) + {i}  # step", i.max(1) - 1);
        let candidate = if lines.is_empty() { next.clone() } else { format!("{}\n{next}", lines.join("\n")) };
        if !python_window::fits(&candidate) {
            break;
        }
        lines.push(next);
        i += 1;
    }
    let code = lines.join("\n");
    assert!(code.chars().count() > 7_500, "{}", code.chars().count());
    let mut s = blank();
    s.execute("python.set", json!({"cell": "A1", "code": code})).unwrap();
    let mut h = harness(s);
    let frames = |h: &mut Harness, n: u32, typing: bool| {
        let start = std::time::Instant::now();
        for _ in 0..n {
            if typing {
                h.input_mut().events.push(Event::Key {
                    key: Key::Backspace,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                });
            }
            h.run_steps(1);
        }
        start.elapsed().as_secs_f64() * 1000.0 / f64::from(n)
    };
    let base = frames(&mut h, 20, false);
    ui(&mut h, json!({"on": true, "cell": "A1"})).unwrap();
    to_end(&mut h);
    let idle = frames(&mut h, 20, false);
    let typing = frames(&mut h, 20, true);
    eprintln!(
        "python editor, {} chars, {} lines: {base:.1} ms a frame without it, {idle:.1} ms open, {typing:.1} ms typing",
        code.chars().count(),
        lines.len()
    );
    assert_eq!(text(&h).chars().count(), code.chars().count() - 20, "each frame took its Backspace");
    // A loose bound, for a debug build on a shared machine: it catches work that grows with the
    // square of the code, not a slow machine.
    assert!(typing < base + 250.0, "{typing} ms a frame");
}

#[test]
fn the_window_follows_its_sheet_and_never_writes_to_another() {
    let mut s = blank();
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("python.set", json!({"cell": "Data!B2", "code": "d = 1"})).unwrap();
    s.execute("python.set", json!({"cell": "Sheet1!B2", "code": "s = 1"})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "Data!B2"})).unwrap();
    assert_eq!(text(&h), "d = 1");
    let sheet_of = |h: &Harness| win(h).map(|w| (w.sheet, w.sheet_name)).unwrap();
    let data = sheet_of(&h).0;
    // Moved: followed by name.
    h.state_mut().run("sheet.move", json!({"sheet": "Data", "to": 1 - data})).unwrap();
    h.run_steps(3);
    assert_eq!(sheet_of(&h), (1 - data, "Data".to_string()));
    // Renamed: followed in place.
    h.state_mut().run("sheet.rename", json!({"sheet": "Data", "name": "Inputs"})).unwrap();
    h.run_steps(3);
    assert_eq!(sheet_of(&h).1, "Inputs");
    to_end(&mut h);
    type_text(&mut h, " + 1");
    click(&mut h, APPLY);
    let code_on = |h: &Harness, sheet: &str| {
        let d = h.state().session.active().unwrap();
        let sh = &d.wb.sheets[d.wb.sheet_index(sheet).unwrap()];
        sh.cell(CellRef::new(1, 1)).and_then(|c| c.py.as_ref().map(|p| p.code.clone()))
    };
    assert_eq!(code_on(&h, "Inputs").as_deref(), Some("d = 1 + 1"));
    assert_eq!(code_on(&h, "Sheet1").as_deref(), Some("s = 1"));
    // Deleted with unsaved code: Apply is refused rather than writing to the sheet in its place.
    type_text(&mut h, "!");
    h.state_mut().run("home.deleteSheet", json!({"sheet": "Inputs"})).unwrap();
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| w.dirty()), "the code is kept");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code_on(&h, "Sheet1").as_deref(), Some("s = 1"), "the other sheet is untouched");
    let notice = python_window::state(h.state())["notice"].as_str().unwrap_or_default().to_string();
    assert!(notice.contains("Inputs"), "{notice}");
    // A new sheet in its place is not taken for it either.
    h.state_mut().run("home.insertSheet", json!({"name": "Other"})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert!(
        h.state()
            .session
            .active()
            .unwrap()
            .wb
            .sheets
            .iter()
            .all(|sh| sh.cell(CellRef::new(1, 1)).and_then(|c| c.py.as_ref()).is_none_or(|p| p.code == "s = 1"))
    );
    h.state_mut().run("edit.undo", json!({})).unwrap();
    // Undoing the deletion brings the sheet back, and Apply works again.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    click(&mut h, APPLY);
    assert_eq!(code_on(&h, "Inputs").as_deref(), Some("d = 1 + 1!"));
    // Without unsaved code the window closes with its sheet.
    h.state_mut().run("home.deleteSheet", json!({"sheet": "Inputs"})).unwrap();
    h.run_steps(3);
    assert!(win(&h).is_none());
}

/// What `cell` holds on the active sheet, as the formula bar shows it.
fn input(h: &Harness, cell: &str) -> String {
    let d = h.state().session.active().unwrap();
    d.wb.active().unwrap().input_text(CellRef::parse(cell).unwrap())
}

/// The refusal shown when the window's cell changed under it.
fn stale_message(cell: &str) -> String {
    python_window::STALE.replace("{cell}", cell)
}

fn notice(h: &Harness) -> Option<String> {
    python_window::state(h.state())["notice"].as_str().map(str::to_string)
}

#[test]
fn a_row_inserted_above_the_cell_refuses_apply_rather_than_overwrite_what_moved_in() {
    let mut s = with_code();
    s.execute("cell.set", json!({"cell": "B1", "input": "keep"})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    // B2's code moves to B3, and B1's text to B2.
    h.state_mut().run("home.insertRows", json!({"rows": "1:1"})).unwrap();
    h.run_steps(3);
    assert_eq!((input(&h, "B2"), code(&h, "B3").as_deref()), ("keep".to_string(), Some("x = 1\nx + 1")));
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(input(&h, "B2"), "keep", "the text that moved to B2 is not replaced");
    assert_eq!(code(&h, "B2"), None);
    assert_eq!(code(&h, "B3").as_deref(), Some("x = 1\nx + 1"), "nor the code that moved away");
    let w = win(&h).expect("the window stays open");
    assert_eq!(w.text, "x = 1\nx + 1 + 2", "the code is kept");
    let message = stale_message("Sheet1!B2");
    assert_eq!(notice(&h).as_deref(), Some(message.as_str()));
    assert!(h.query_by_label(&message).is_some(), "shown in the window");
    assert!(h.state().message.is_none(), "not in a message box over it");
    // Apply from the question about unsaved changes refuses the same way.
    key(&mut h, Key::Escape, Modifiers::NONE);
    click(&mut h, APPLY);
    assert!(win(&h).is_some_and(|w| w.text == "x = 1\nx + 1 + 2" && w.pending.is_none()));
    assert_eq!(input(&h, "B2"), "keep");
    // The control channel doesn't take the address for the same cell: it refuses to replace the
    // unsaved code, and with `force` opens afresh on what B2 holds now.
    let e = ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap_err();
    assert!(e.contains("unsaved changes"), "{e}");
    assert!(win(&h).is_some_and(|w| w.text == "x = 1\nx + 1 + 2"));
    ui(&mut h, json!({"on": true, "cell": "B2", "force": true})).unwrap();
    type_text(&mut h, "y = 3");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("y = 3"), "opened again on the cell, Apply works");
}

#[test]
fn a_column_inserted_to_the_left_refuses_apply_rather_than_overwrite_a_formula() {
    let mut s = with_code();
    s.execute("cell.set", json!({"cell": "A2", "input": "=1+1"})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    h.state_mut().run("home.insertColumns", json!({"cols": "A:A"})).unwrap();
    h.run_steps(3);
    assert_eq!(input(&h, "B2"), "=1+1", "the formula moved to B2");
    click(&mut h, APPLY);
    assert_eq!(input(&h, "B2"), "=1+1", "and is not replaced");
    assert_eq!(code(&h, "C2").as_deref(), Some("x = 1\nx + 1"));
    assert!(win(&h).is_some_and(|w| w.text == "x = 1\nx + 1 + 2"));
    let message = stale_message("Sheet1!B2");
    assert!(h.query_by_label(&message).is_some(), "{:?}", notice(&h));
}

#[test]
fn code_changed_from_outside_refuses_apply() {
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "z = 9"})).unwrap();
    h.run_steps(3);
    // Before Apply, the window says the cell changed.
    let changed = python_window::CHANGED.replace("{cell}", "Sheet1!B2");
    assert!(h.query_by_label(&changed).is_some(), "the change is shown before Apply");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("z = 9"), "the other code is not replaced");
    assert_eq!(text(&h), "x = 1\nx + 1 + 2");
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
    // A changed return type counts too.
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    type_text(&mut h, "# note\n");
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "x = 1\nx + 1", "output": "object"})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1"));
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
}

#[test]
fn an_unrelated_edit_still_allows_apply() {
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    // Other cells, the cell's format, and a row inserted below it are not changes to the cell.
    h.state_mut().run("cell.set", json!({"cell": "D9", "input": "5"})).unwrap();
    h.state_mut().run("home.bold", json!({"range": "B2"})).unwrap();
    h.state_mut().run("home.insertRows", json!({"rows": "5:5"})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1 + 2"));
    assert_eq!(notice(&h), None);
    // A row inserted above it then is a change.
    type_text(&mut h, "!");
    h.state_mut().run("home.insertRows", json!({"rows": "1:1"})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2"), None);
    assert_eq!(code(&h, "B3").as_deref(), Some("x = 1\nx + 1 + 2"));
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
}

#[test]
fn apply_after_apply_works_because_each_apply_takes_the_cell_as_it_is_now() {
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    type_text(&mut h, " + 3");
    click(&mut h, APPLY);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1 + 2 + 3"));
    assert_eq!(notice(&h), None);
    // The reference is the code last applied: a change from outside after it is still caught.
    type_text(&mut h, " + 4");
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "x = 1\nx + 1 + 2"})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1 + 2"));
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
}

/// B2 holds code, formatted Hidden (and unlocked); the sheet is protected when `protected`.
fn hidden_code(protected: bool) -> Harness {
    let mut s = with_code();
    s.execute("home.formatCells", json!({"range": "B2", "style": {"protection": {"locked": false, "hidden": true}}})).unwrap();
    if protected {
        s.execute("review.protectSheet", json!({})).unwrap();
    }
    s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    harness(s)
}

#[test]
fn the_control_channel_refuses_hidden_code_on_a_protected_sheet() {
    let mut h = hidden_code(true);
    let e = ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap_err();
    assert!(e.contains("B2 are hidden because the sheet is protected"), "{e}");
    assert!(win(&h).is_none());
    // Without `cell` (the active cell) and without `on` too.
    assert!(ui(&mut h, json!({})).is_err());
    assert!(win(&h).is_none());
    assert!(h.state().dialog.is_none() && h.state().message.is_none(), "no dialog");
}

#[test]
fn the_formula_bar_offers_no_editor_button_on_hidden_code() {
    let mut h = hidden_code(true);
    assert!(h.query_by_label(EDIT_IN_EDITOR).is_none(), "no button");
    python_window::open_from_bar(h.state_mut());
    h.run_steps(3);
    assert!(win(&h).is_none(), "the route opens nothing");
    // Without protection the button is there.
    let h = hidden_code(false);
    assert!(h.query_by_label(EDIT_IN_EDITOR).is_some());
}

#[test]
fn the_cell_menu_offers_no_editor_entry_on_hidden_code() {
    let mut h = hidden_code(true);
    h.state_mut().grid.context_menu = Some(Pos2::new(300.0, 300.0));
    h.run_steps(3);
    let menu = h.ctx.memory(|m| m.area_rect(egui::Id::unique("cell_context"))).expect("the cell menu");
    assert!(h.query_all_by_label(EDIT_IN_EDITOR).all(|n| !menu.contains(n.rect().center())), "no entry in the menu");
    python_window::open_from_menu(h.state_mut());
    h.run_steps(3);
    assert!(win(&h).is_none(), "the route opens nothing");
}

#[test]
fn protecting_the_sheet_closes_the_window_on_hidden_code_and_keeps_typed_code_on_the_clipboard() {
    // Without changes, the window just closes.
    let mut h = hidden_code(false);
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    assert_eq!(text(&h), "x = 1\nx + 1");
    h.state_mut().session.execute("review.protectSheet", json!({})).unwrap();
    h.run_steps(3);
    assert!(win(&h).is_none(), "the code is out of view");
    assert!(h.query_by_label("x = 1\nx + 1").is_none());
    assert!(h.state().toast.is_none(), "nothing typed, nothing copied");
    // With typed code, it closes too, and the typed code goes to the clipboard.
    let mut h = hidden_code(false);
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " # typed");
    assert!(win(&h).is_some_and(|w| w.dirty()));
    h.state_mut().session.execute("review.protectSheet", json!({})).unwrap();
    h.step();
    let copied = h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "x = 1\nx + 1 # typed"));
    h.run_steps(2);
    assert!(win(&h).is_none(), "the code is out of view");
    assert!(copied, "the typed code is on the clipboard");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some(python_window::COPIED));
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1"), "nothing was applied");
}

#[test]
fn a_window_that_lost_its_sheet_still_closes_when_its_code_becomes_hidden() {
    let mut s = blank();
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("python.set", json!({"cell": "Data!B2", "code": "d = 1"})).unwrap();
    s.execute("home.formatCells", json!({"range": "Data!B2", "style": {"protection": {"locked": false, "hidden": true}}})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "Data!B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 1");
    // Copied, and then deleted: the window follows no sheet, and keeps its code.
    h.state_mut().run("sheet.move", json!({"sheet": "Data", "to": 0, "copy": true})).unwrap();
    h.state_mut().run("home.deleteSheet", json!({"sheet": "Data"})).unwrap();
    h.run_steps(3);
    assert!(win(&h).is_some_and(|w| w.text == "d = 1 + 1"), "the typed code stays");
    // Protecting the copy hides the code there: the window closes all the same.
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Data (2)"})).unwrap();
    h.state_mut().session.execute("review.protectSheet", json!({})).unwrap();
    h.step();
    let copied = h.output().platform_output.commands.iter().any(|c| matches!(c, egui::OutputCommand::CopyText(t) if t == "d = 1 + 1"));
    h.run_steps(2);
    assert!(win(&h).is_none(), "the code is out of view");
    assert!(copied, "the typed code is on the clipboard");
    assert_eq!(h.state().toast.as_ref().map(|t| t.0.as_str()), Some(python_window::COPIED));
}

#[test]
fn a_question_answered_after_the_cell_became_hidden_opens_nothing() {
    let mut s = with_code();
    s.execute("python.set", json!({"cell": "D4", "code": "y = 2"})).unwrap();
    s.execute("home.formatCells", json!({"range": "B2", "style": {"protection": {"locked": false, "hidden": true}}})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "D4"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 1");
    // Opening B2 from its menu asks first about D4's typed code.
    select(&mut h, "B2");
    python_window::open_from_menu(h.state_mut());
    h.run_steps(2);
    assert!(win(&h).is_some_and(|w| matches!(w.pending, Some(Pending::Open { .. }))), "it asks");
    // The sheet is protected while the question is open; then Discard.
    h.state_mut().session.execute("review.protectSheet", json!({})).unwrap();
    h.run_steps(2);
    let ctx = h.ctx.clone();
    python_window::answer(h.state_mut(), &ctx, python_window::Answer::Discard);
    assert!(win(&h).is_none(), "B2's code is not shown, not even for a frame");
    let m = h.state().message.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
    assert!(m.contains("B2 are hidden because the sheet is protected"), "{m}");
}

#[test]
fn apply_then_undo_then_typing_applies_to_the_same_cell() {
    let mut h = harness(with_code());
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1 + 2"));
    // Undo puts the old code back in the cell; the window, with nothing unsaved, shows it.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1"));
    assert_eq!(text(&h), "x = 1\nx + 1");
    to_end(&mut h);
    type_text(&mut h, " + 3");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(notice(&h), None);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1 + 3"), "Apply edits the code the window shows");
    // The same on a cell the window gave its first code: the undo empties it, Apply fills it again.
    ui(&mut h, json!({"on": true, "cell": "D4"})).unwrap();
    type_text(&mut h, "y = 1");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "D4").as_deref(), Some("y = 1"));
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!((code(&h, "D4"), text(&h)), (None, String::new()));
    type_text(&mut h, "z = 2");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(notice(&h), None);
    assert_eq!(code(&h, "D4").as_deref(), Some("z = 2"));
}

#[test]
fn after_an_undo_a_row_inserted_above_with_unsaved_typing_still_refuses_apply() {
    let mut s = with_code();
    s.execute("cell.set", json!({"cell": "B1", "input": "keep"})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    to_end(&mut h);
    type_text(&mut h, " + 3");
    h.state_mut().run("home.insertRows", json!({"rows": "1:1"})).unwrap();
    h.run_steps(3);
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(input(&h, "B2"), "keep", "the text that moved to B2 is not replaced");
    assert_eq!(code(&h, "B3").as_deref(), Some("x = 1\nx + 1"));
    assert_eq!(text(&h), "x = 1\nx + 1 + 3", "the code is kept");
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
}

/// B2 and B3 hold code; A2 and A3 sort them so that B3's row comes first.
fn two_python_cells(b3: &str) -> Harness {
    let mut s = with_code();
    s.execute("python.set", json!({"cell": "B3", "code": b3})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "2"})).unwrap();
    s.execute("cell.set", json!({"cell": "A3", "input": "1"})).unwrap();
    harness(s)
}

#[test]
fn a_sort_that_moves_another_python_cell_to_the_address_refuses_apply() {
    let mut h = two_python_cells("b = 3");
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    h.state_mut().run("data.sortAscending", json!({"range": "A2:B3", "column": "A"})).unwrap();
    h.run_steps(3);
    assert_eq!((code(&h, "B2").as_deref(), code(&h, "B3").as_deref()), (Some("b = 3"), Some("x = 1\nx + 1")), "the sort swapped the cells");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("b = 3"), "the cell that moved in is not replaced");
    assert_eq!(text(&h), "x = 1\nx + 1 + 2");
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
    // The same when the code that moved in is exactly the unsaved typing: the window takes it as
    // saved, but the typing was for the cell that moved away, so further typing is still refused.
    let mut h = two_python_cells("x = 1\nx + 1 + 2");
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    h.state_mut().run("data.sortAscending", json!({"range": "A2:B3", "column": "A"})).unwrap();
    h.run_steps(3);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1 + 2"));
    to_end(&mut h);
    type_text(&mut h, " + 4");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    assert_eq!(code(&h, "B2").as_deref(), Some("x = 1\nx + 1 + 2"), "the cell that moved in is not replaced");
    assert_eq!(code(&h, "B3").as_deref(), Some("x = 1\nx + 1"));
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
}

#[test]
fn the_question_closes_the_file_view_so_it_can_be_seen() {
    let mut h = harness(blank());
    ui(&mut h, json!({"on": true})).unwrap();
    type_text(&mut h, "a = 1");
    h.state_mut().run("ui.backstage", json!({"page": "info"})).unwrap();
    h.run_steps(2);
    assert!(h.state().backstage.is_some(), "the File view is open");
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(3);
    assert!(matches!(win(&h).and_then(|w| w.pending), Some(Pending::CloseWorkbook(_))), "it asks");
    assert!(h.state().backstage.is_none(), "the File view closes, so the question is in sight");
}

/// Runs `commands` as one script: one step, with no frame between them.
fn in_one_step(h: &mut Harness, commands: serde_json::Value) {
    let name = "Step";
    let _ = h.state_mut().session.execute("automate.deleteScript", json!({"name": name}));
    h.state_mut().session.execute("automate.newScript", json!({"name": name, "commands": commands})).unwrap();
    h.state_mut().session.execute("automate.runScript", json!({"name": name})).unwrap();
    h.run_steps(3);
}

#[test]
fn a_sheet_deleted_and_put_back_under_its_name_in_one_step_is_not_the_windows() {
    let mut s = blank();
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("python.set", json!({"cell": "Data!B2", "code": "d = 1"})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "Data!B2"})).unwrap();
    let place = win(&h).map(|w| w.sheet).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 1");
    // A macro deletes Data and puts a new Data in its place, with the same code at B2: the same
    // name, place and content, but not the window's sheet.
    in_one_step(
        &mut h,
        json!([
            {"command": "home.deleteSheet", "params": {"sheet": "Data"}},
            {"command": "home.insertSheet", "params": {"name": "Data", "before": place}},
            {"command": "python.set", "params": {"cell": "Data!B2", "code": "d = 1"}},
        ]),
    );
    let d = h.state().session.active().unwrap();
    assert_eq!(d.wb.sheet_index("Data"), Some(place), "the new Data is where the old one was");
    assert!(win(&h).is_some_and(|w| w.dirty() && w.text == "d = 1 + 1"), "the typed code is kept");
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    let code_on_data = |h: &Harness| {
        let d = h.state().session.active().unwrap();
        d.wb.sheet(place).and_then(|sh| sh.cell(CellRef::new(1, 1))).and_then(|c| c.py.as_ref().map(|p| p.code.clone()))
    };
    assert_eq!(code_on_data(&h).as_deref(), Some("d = 1"), "the new sheet is not written to");
    assert_eq!(notice(&h), Some(stale_message("Data!B2")));
    // Apply from the question about closing is refused the same way.
    h.state_mut().run_or_alert("file.close", json!({}));
    h.run_steps(3);
    click(&mut h, APPLY);
    assert_eq!(code_on_data(&h).as_deref(), Some("d = 1"));
    assert_eq!(h.state().session.documents().len(), 1, "the workbook stays open");
}

#[test]
fn a_rename_with_a_sheet_added_in_the_same_step_is_still_followed() {
    let mut s = blank();
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("python.set", json!({"cell": "Data!B2", "code": "d = 1"})).unwrap();
    let mut h = harness(s);
    ui(&mut h, json!({"on": true, "cell": "Data!B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 1");
    in_one_step(
        &mut h,
        json!([
            {"command": "sheet.rename", "params": {"sheet": "Data", "name": "Inputs"}},
            {"command": "home.insertSheet", "params": {"name": "Extra", "before": 0}},
        ]),
    );
    let d = h.state().session.active().unwrap();
    let inputs = d.wb.sheet_index("Inputs").unwrap();
    assert_eq!(win(&h).map(|w| (w.sheet, w.sheet_name)), Some((inputs, "Inputs".to_string())));
    key(&mut h, Key::Enter, Modifiers::COMMAND);
    let d = h.state().session.active().unwrap();
    let code = d.wb.sheet(inputs).and_then(|sh| sh.cell(CellRef::new(1, 1))).and_then(|c| c.py.as_ref().map(|p| p.code.clone()));
    assert_eq!(code.as_deref(), Some("d = 1 + 1"));
    assert_eq!(notice(&h), None);
}

#[test]
fn a_refused_apply_after_a_workbook_switch_goes_back_and_shows_why() {
    let mut h = harness(with_code());
    let first = h.state().session.active().unwrap().uid;
    ui(&mut h, json!({"on": true, "cell": "B2"})).unwrap();
    to_end(&mut h);
    type_text(&mut h, " + 2");
    // The cell changes from elsewhere, so Apply will be refused; then another workbook.
    h.state_mut().run("python.set", json!({"cell": "B2", "code": "z = 9"})).unwrap();
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(win(&h).and_then(|w| w.pending), Some(Pending::Switched));
    click(&mut h, APPLY);
    // Back on the code's workbook, with the question gone and the refusal on screen.
    assert_eq!(h.state().session.active().map(|d| d.uid), Some(first));
    assert_eq!(win(&h).and_then(|w| w.pending), None);
    assert_eq!(notice(&h), Some(stale_message("Sheet1!B2")));
    assert!(h.query_by_label(&stale_message("Sheet1!B2")).is_some(), "the refusal is shown");
    assert_eq!(code(&h, "B2").as_deref(), Some("z = 9"));
    assert_eq!(text(&h), "x = 1\nx + 1 + 2");
}
