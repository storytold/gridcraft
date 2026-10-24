//! Switching between open workbooks: the View tab's Switch Windows menu, the Ctrl+F6 and
//! Ctrl+Tab shortcuts, and what a switch keeps (each book's scroll and selection) and commits
//! (a text-box draft).

use egui::accesskit::Toggled;
use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers, vec2};
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::keymap::{Shortcut, lookup};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness_on(session: Session, os: OperatingSystem) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_os(os).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn harness(session: Session) -> Harness {
    harness_on(session, OperatingSystem::Nix)
}

/// Book1 (active) and Book2.
fn two_books() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.new_workbook();
    s.set_active(0);
    s
}

fn one_book() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL };
/// The Control key on a Mac.
const MAC_CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: false };
const MAC_CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..MAC_CTRL };

/// A key press as a US keyboard reports it.
fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn index(h: &Harness) -> usize {
    h.state().session.active_index()
}

fn active_cell(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.active.a1()
}

fn quiet(h: &Harness) -> bool {
    h.state().message.is_none() && h.state().toast.is_none()
}

fn open_switch_windows(h: &mut Harness) {
    h.state_mut().ui.ribbon_tab = "View".into();
    h.run_steps(4);
    h.get_by_label("Switch Windows").click();
    h.run_steps(4);
}

fn checked(h: &Harness, label: &str) -> bool {
    h.get_by_label(label).accesskit_node().toggled() == Some(Toggled::True)
}

#[test]
fn keymap_binds_the_window_shortcuts_and_leaves_backtick_to_the_native_menu() {
    let next = Some(Shortcut::Run("window.next", json!({})));
    let previous = Some(Shortcut::Run("window.previous", json!({})));
    for mac in [false, true] {
        let (ctrl, ctrl_shift) = if mac { (MAC_CTRL, MAC_CTRL_SHIFT) } else { (CTRL, CTRL_SHIFT) };
        assert_eq!(lookup(Key::F6, Some(Key::F6), ctrl, mac), next, "Ctrl+F6 (mac: {mac})");
        assert_eq!(lookup(Key::F6, Some(Key::F6), ctrl_shift, mac), previous, "Ctrl+Shift+F6 (mac: {mac})");
        assert_eq!(lookup(Key::Tab, Some(Key::Tab), ctrl, mac), next, "Ctrl+Tab (mac: {mac})");
        assert_eq!(lookup(Key::Tab, Some(Key::Tab), ctrl_shift, mac), previous, "Ctrl+Shift+Tab (mac: {mac})");
        // Plain and Shift+Tab stay cell navigation; F6 alone is not a shortcut.
        assert_eq!(lookup(Key::Tab, Some(Key::Tab), Modifiers::NONE, mac), None);
        assert_eq!(lookup(Key::Tab, Some(Key::Tab), Modifiers::SHIFT, mac), None);
        assert_eq!(lookup(Key::F6, Some(Key::F6), Modifiers::NONE, mac), None);
    }
    // macOS Cmd+` and Cmd+Shift+` are the native Window menu's accelerators, not keymap entries.
    assert_eq!(lookup(Key::Backtick, Some(Key::Backtick), Modifiers::COMMAND, true), None);
    assert_eq!(lookup(Key::Backtick, Some(Key::Backtick), Modifiers::COMMAND | Modifiers::SHIFT, true), None);
    // Show Formulas: Control+` on the Mac (Cmd+` is the next window), Ctrl+` elsewhere through
    // the plain shortcut table, so neither chord does two things.
    assert_eq!(lookup(Key::Backtick, Some(Key::Backtick), Modifiers::CTRL, true), Some(Shortcut::Run("formulas.showFormulas", json!({}))));
    assert_eq!(lookup(Key::Backtick, Some(Key::Backtick), Modifiers::CTRL, false), None);
}

#[test]
fn shortcuts_cycle_through_the_windows_and_wrap() {
    for (os, ctrl, ctrl_shift) in [(OperatingSystem::Nix, CTRL, CTRL_SHIFT), (OperatingSystem::Mac, MAC_CTRL, MAC_CTRL_SHIFT)] {
        let mut h = harness_on(two_books(), os);
        assert_eq!(index(&h), 0);
        assert_eq!(h.state().grid.last_title.as_deref(), Some("Book1"));
        key(&mut h, Key::F6, ctrl);
        assert_eq!(index(&h), 1, "Ctrl+F6 activates the next window ({os:?})");
        assert_eq!(h.state().grid.last_title.as_deref(), Some("Book2"), "the window title follows the switch ({os:?})");
        key(&mut h, Key::F6, ctrl);
        assert_eq!(index(&h), 0, "Ctrl+F6 wraps to the first window ({os:?})");
        key(&mut h, Key::F6, ctrl_shift);
        assert_eq!(index(&h), 1, "Ctrl+Shift+F6 wraps to the last window ({os:?})");
        key(&mut h, Key::F6, ctrl_shift);
        assert_eq!(index(&h), 0, "Ctrl+Shift+F6 activates the previous window ({os:?})");
        key(&mut h, Key::Tab, ctrl);
        assert_eq!(index(&h), 1, "Ctrl+Tab activates the next window ({os:?})");
        key(&mut h, Key::Tab, ctrl);
        assert_eq!(index(&h), 0, "Ctrl+Tab wraps ({os:?})");
        key(&mut h, Key::Tab, ctrl_shift);
        assert_eq!(index(&h), 1, "Ctrl+Shift+Tab wraps backwards ({os:?})");
        key(&mut h, Key::Tab, ctrl_shift);
        assert_eq!(index(&h), 0, "Ctrl+Shift+Tab activates the previous window ({os:?})");
        assert_eq!(h.state().grid.last_title.as_deref(), Some("Book1"));
        // Ctrl+Tab switched windows: it did not move the selection as Tab does.
        assert_eq!(active_cell(&h), "A1", "Ctrl+Tab is a window switch, not a Tab ({os:?})");
        assert!(quiet(&h), "no message or toast ({os:?})");
        assert!(h.state().editor.is_none());
    }
}

#[test]
fn with_one_book_the_window_shortcuts_do_nothing_quietly() {
    let mut h = harness(one_book());
    for (k, m) in [(Key::F6, CTRL), (Key::F6, CTRL_SHIFT), (Key::Tab, CTRL), (Key::Tab, CTRL_SHIFT)] {
        key(&mut h, k, m);
        assert_eq!(index(&h), 0);
        assert!(quiet(&h), "{k:?} with {m:?} must not raise a message or toast");
        assert_eq!(active_cell(&h), "A1", "{k:?} with {m:?} must not move the selection");
        assert!(h.state().editor.is_none());
    }
    // The menu still opens, with the one book checked.
    open_switch_windows(&mut h);
    assert!(checked(&h, "1 Book1"));
    assert!(quiet(&h));
}

#[test]
fn ribbon_switch_windows_menu_lists_the_books_and_activates_the_clicked_one() {
    let mut h = harness(two_books());
    open_switch_windows(&mut h);
    assert!(checked(&h, "1 Book1"), "the active book is checked");
    assert!(!checked(&h, "2 Book2"));
    h.get_by_label("2 Book2").click();
    h.run_steps(4);
    assert_eq!(index(&h), 1, "clicking an entry activates its book");
    assert_eq!(h.state().session.active().unwrap().display_title(), "Book2");
    assert!(quiet(&h));

    // Reopened, the check has moved; an unsaved book carries the title bar's marker.
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "edited"})).unwrap();
    open_switch_windows(&mut h);
    assert!(checked(&h, "2 Book2 •"), "the active, edited book is checked and marked");
    assert!(!checked(&h, "1 Book1"));
    h.get_by_label("1 Book1").click();
    h.run_steps(4);
    assert_eq!(index(&h), 0);

    // The keytip W, W opens the same menu.
    for k in [Key::F10, Key::W, Key::W] {
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
    assert!(checked(&h, "1 Book1"), "Alt, W, W opens the Switch Windows menu");
    assert!(quiet(&h));
}

#[test]
fn scroll_and_selection_of_each_book_survive_a_switch_away_and_back() {
    let mut h = harness(two_books());
    h.state_mut().run("selection.set", json!({"cell": "D7"})).unwrap();
    {
        let app = h.state_mut();
        let sh = app.session.active().unwrap().wb.active().unwrap().clone();
        app.view_mut().unwrap().scroll = vec2(sh.col_left(2) as f32, sh.row_top(24) as f32);
    }
    h.run_steps(2);
    let book1 = h.state().view().scroll;
    assert!(book1.y > 0.0 && book1.x > 0.0, "Book1 is scrolled");

    key(&mut h, Key::F6, CTRL);
    assert_eq!(index(&h), 1);
    assert_eq!(h.state().view().scroll, vec2(0.0, 0.0), "Book2 has its own scroll position");
    assert_eq!(active_cell(&h), "A1", "Book2 has its own selection");
    h.state_mut().run("selection.set", json!({"cell": "B3"})).unwrap();
    {
        let app = h.state_mut();
        let sh = app.session.active().unwrap().wb.active().unwrap().clone();
        app.view_mut().unwrap().scroll = vec2(0.0, sh.row_top(10) as f32);
    }
    h.run_steps(2);
    let book2 = h.state().view().scroll;
    assert!(book2.y > 0.0 && book2 != book1);

    key(&mut h, Key::F6, CTRL);
    assert_eq!(index(&h), 0);
    assert_eq!(h.state().view().scroll, book1, "Book1's scroll position is back");
    assert_eq!(active_cell(&h), "D7", "Book1's selection is back");

    key(&mut h, Key::Tab, CTRL);
    assert_eq!(index(&h), 1);
    assert_eq!(h.state().view().scroll, book2, "Book2's scroll position is back");
    assert_eq!(active_cell(&h), "B3", "Book2's selection is back");
}

#[test]
fn a_switch_commits_the_text_box_draft() {
    let mut h = harness(two_books());
    let id = h.state_mut().run("insert.textBox", json!({"at": "B2", "text": "Draft"})).unwrap()["shape"].as_u64().unwrap() as u32;
    h.run_steps(2);
    for (command, expected, text) in [("window.next", 1, "Committed by next"), ("window.previous", 1, "Committed by previous")] {
        let before = index(&h);
        assert!(h.state_mut().begin_text_box_edit(id));
        h.state_mut().text_box_editor.as_mut().unwrap().text = text.into();
        h.state_mut().run(command, json!({})).unwrap();
        assert_eq!(index(&h), expected, "{command} switches the window");
        assert!(h.state().text_box_editor.is_none(), "{command} ends the text-box edit");
        let docs = h.state().session.documents();
        let shape = docs[before].wb.sheet(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
        assert_eq!(shape.text, text, "{command} commits the draft to the book it was typed in");
        assert!(quiet(&h));
        if command == "window.next" {
            h.state_mut().run("window.previous", json!({})).unwrap();
            h.run_steps(2);
        }
    }
}

#[test]
fn a_switch_from_the_menu_commits_the_cell_being_edited_to_its_own_book() {
    // The in-cell editor isn't document-aware: left open, it would carry on in the other
    // workbook and Enter would write the text there.
    let mut h = harness(two_books());
    h.state_mut().begin_edit(Some("typed in book1".into()), false);
    assert!(h.state().editor.is_some());
    open_switch_windows(&mut h);
    h.get_by_label("2 Book2").click();
    h.run_steps(4);
    assert_eq!(index(&h), 1);
    assert!(h.state().editor.is_none(), "the entry was committed by the switch");
    let docs = h.state().session.documents();
    assert_eq!(docs[0].wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).display(), "typed in book1");
    assert!(docs[1].wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).is_empty());
    // The same through the command the Mac menu and the control channel use.
    h.state_mut().begin_edit(Some("more".into()), false);
    h.state_mut().run("window.activate", json!({"index": 0})).unwrap();
    h.run_steps(2);
    assert!(h.state().editor.is_none());
    assert_eq!(h.state().session.documents()[1].wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).display(), "more");
}

#[test]
fn a_switch_drops_the_object_selection_of_the_book_left_behind() {
    // Chart ids start at 1 in every workbook: Book1's selected chart 1 must not show as selected,
    // nor be deleted, in Book2.
    let mut h = harness(two_books());
    h.state_mut().run("cell.set", json!({"cell": "A1", "input": "3"})).unwrap();
    h.state_mut().run("insert.chart", json!({"range": "A1", "type": "column"})).unwrap();
    assert_eq!(h.state().selected_chart, Some(1), "inserting selects the chart");
    key(&mut h, Key::F6, CTRL);
    assert_eq!(index(&h), 1);
    assert_eq!(h.state().selected_chart, None, "Book2 shows no selection it never made");
    key(&mut h, Key::Delete, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(h.state().session.documents()[0].wb.sheet(0).unwrap().charts.len(), 1, "Book1's chart survives Delete in Book2");
}

#[test]
fn the_switch_windows_menu_scrolls_when_many_books_are_open() {
    let mut s = Session::new();
    for _ in 0..40 {
        s.new_workbook();
    }
    s.set_active(0);
    let mut h = harness(s);
    open_switch_windows(&mut h);
    let first = h.get_by_label("1 Book1").rect();
    // Bounded: the menu hangs below its button instead of being pushed up to the top edge to fit
    // forty rows, and the rows below the viewport come into reach by scrolling.
    assert!(first.top() > 30.0 && first.bottom() <= 800.0, "{first:?}");
    let before = h.get_by_label("40 Book40").rect();
    assert!(before.top() > first.top() + 400.0, "{before:?}: forty rows are laid out");
    h.input_mut().events.push(Event::PointerMoved(first.center()));
    h.input_mut().events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -3000.0),
        modifiers: Modifiers::NONE,
        phase: egui::TouchPhase::Move,
    });
    h.run_steps(3);
    let after = h.get_by_label("40 Book40").rect();
    assert!(after.top() < before.top() - 300.0 && after.bottom() <= 800.0, "{before:?} -> {after:?}: the list scrolls");
    h.get_by_label("40 Book40").click();
    h.run_steps(4);
    assert_eq!(index(&h), 39, "the last entry can be clicked");
}

#[test]
fn a_refused_cell_entry_keeps_the_switch_from_happening_and_says_why() {
    // A formula that doesn't parse can't be committed: the switch is refused with the reason,
    // the editor stays open in the same book, and the commit's own message is not replaced.
    let mut h = harness(two_books());
    h.state_mut().begin_edit(Some("=1+".into()), false);
    let r = h.state_mut().run("window.next", json!({}));
    assert!(r.is_err(), "{r:?}");
    assert!(r.unwrap_err().starts_with("finish the entry first"));
    assert_eq!(index(&h), 0);
    assert!(h.state().editor.is_some(), "the entry is still being edited");
    assert!(h.state().message.is_some(), "the reason is shown");
    // `file.new` while editing commits a good entry into the book being left, not the new one.
    h.state_mut().editor = None;
    h.state_mut().message = None;
    h.state_mut().begin_edit(Some("kept".into()), false);
    h.state_mut().run("file.new", json!({})).unwrap();
    h.run_steps(2);
    let docs = h.state().session.documents();
    assert_eq!(docs.len(), 3);
    assert_eq!(docs[0].wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).display(), "kept");
    assert!(docs[2].wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).is_empty());
}

#[test]
fn the_shortcuts_work_while_editing_and_commit_the_entry_to_its_own_book() {
    // The grid's keyboard handler stands back while an editor is open; the window shortcuts
    // still go through, and the switch commits the entry first, as it does from the menu.
    for (os, next, previous) in [(OperatingSystem::Nix, CTRL, CTRL_SHIFT), (OperatingSystem::Mac, MAC_CTRL, MAC_CTRL_SHIFT)] {
        let mut h = harness_on(two_books(), os);
        h.state_mut().begin_edit(Some("typed in book1".into()), false);
        key(&mut h, Key::F6, next);
        assert_eq!(index(&h), 1, "{os:?}: Ctrl+F6 switches while editing");
        assert!(h.state().editor.is_none(), "{os:?}: the entry was committed by the switch");
        assert!(quiet(&h), "{os:?}");
        h.state_mut().begin_edit(Some("typed in book2".into()), false);
        key(&mut h, Key::Tab, previous);
        assert_eq!(index(&h), 0, "{os:?}: Ctrl+Shift+Tab switches back while editing");
        assert!(h.state().editor.is_none(), "{os:?}");
        let docs = h.state().session.documents();
        assert_eq!(docs[0].wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).display(), "typed in book1", "{os:?}");
        assert_eq!(docs[1].wb.sheet(0).unwrap().value(gridcraft_engine::core::CellRef::new(0, 0)).display(), "typed in book2", "{os:?}");
        // A text-box draft, too.
        let id = h.state_mut().run("insert.textBox", json!({"at": "B2", "text": "Draft"})).unwrap()["shape"].as_u64().unwrap() as u32;
        h.run_steps(2);
        assert!(h.state_mut().begin_text_box_edit(id));
        h.state_mut().text_box_editor.as_mut().unwrap().text = "Committed by the shortcut".into();
        key(&mut h, Key::Tab, next);
        assert_eq!(index(&h), 1, "{os:?}: Ctrl+Tab switches while a text box is edited");
        assert!(h.state().text_box_editor.is_none(), "{os:?}");
        let docs = h.state().session.documents();
        let shape = docs[0].wb.sheet(0).unwrap().shapes.iter().find(|s| s.id == id).unwrap();
        assert_eq!(shape.text, "Committed by the shortcut", "{os:?}");
    }
}

#[test]
fn a_refused_entry_keeps_the_shortcut_from_switching_and_a_later_error_still_shows() {
    let mut h = harness(two_books());
    h.state_mut().begin_edit(Some("=1+".into()), false);
    key(&mut h, Key::F6, CTRL);
    assert_eq!(index(&h), 0, "the switch did not happen");
    assert!(h.state().editor.is_some(), "the entry is still being edited");
    let (_, why) = h.state().message.clone().expect("the reason is shown");
    assert!(!why.starts_with("finish the entry first"), "the entry's own message, not the wrapper: {why}");
    // A message box doesn't swallow the next error (the Mac menu and the control channel still
    // reach the app while one is up).
    h.state_mut().editor = None;
    h.state_mut().message = Some(("GridCraft".into(), "an earlier message".into()));
    h.state_mut().run_or_alert("window.activate", json!({"index": 9}));
    let (_, now) = h.state().message.clone().expect("a message is shown");
    assert_ne!(now, "an earlier message", "the new error replaced it");
}
