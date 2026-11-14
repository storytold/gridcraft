//! IME composition at the grid level.
//!
//! With a CJK input method, the first keystroke of a composition must reach the input method,
//! not be written straight into the cell as raw text. The grid publishes `PlatformOutput::ime`
//! on every frame it owns keyboard input, so the OS delivers `ImeEvent`s; a `Commit` opens the
//! in-cell editor with the finished text. Plain ASCII keeps using `Event::Text` (no regression).

use egui::{Event, ImeEvent};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A warmed harness over `session`, with the grid holding keyboard input on the active cell.
///
/// The suite pins a font set with no Han face, so the harness draws tofu for the CJK commit
/// text: these tests check input routing, not glyph coverage.
fn fixture_with(session: Session) -> Harness {
    let builder = crate::common::builder().allow_missing_glyphs().with_size(egui::vec2(1200.0, 800.0));
    crate::common::harness_with(builder, crate::common::app(session), 4)
}

/// A warmed harness over a blank workbook, with the grid holding keyboard input on `A1`.
fn fixture() -> Harness {
    fixture_with(crate::support::blank_session())
}

/// A protected sheet with `active` selected, optionally unlocking one cell first.
fn protected_session(active: &str, unlock: Option<&str>) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    if let Some(cell) = unlock {
        s.execute("home.formatCells", json!({"range": cell, "style": {"protection": {"locked": false}}})).unwrap();
    }
    s.execute("review.protectSheet", json!({})).unwrap();
    s.execute("selection.set", json!({"cell": active})).unwrap();
    s
}

/// The text the in-cell editor currently holds, or `None` when no editor is open.
fn editor_text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|ed| ed.text.clone())
}

/// The grid must request IME on every frame it owns keyboard input: winit applies
/// `set_ime_allowed` at the end of the frame, so requesting it in response to the first key is
/// already too late. The commit tests below only exercise the event handler and would still
/// pass with `request_grid_ime` removed, so this asserts the actual fix.
#[test]
fn grid_publishes_ime_request_before_the_first_keystroke() {
    let mut h = fixture();
    h.run_steps(1);
    let ime = h.output().platform_output.ime.as_ref().expect("the focused grid must request IME");
    assert!(matches!(ime.purpose, egui::IMEPurpose::Normal), "cell text is ordinary input");
    assert!(ime.rect.width() > 0.0 && ime.rect.height() > 0.0, "the IME rect must cover the active cell");
    assert_eq!(ime.cursor_rect.min, ime.rect.min, "the caret sits at the cell's start");
    assert!(ime.context_generation > 0, "egui 0.37 needs a non-zero IME context generation");
}

/// When a dialog takes the keyboard, `PlatformOutput` must stop advertising a grid IME owner.
/// The output is rebuilt each frame, so an early return is what turns IME off.
#[test]
fn grid_withholds_ime_request_when_it_does_not_own_keyboard_input() {
    let mut h = fixture();
    h.state_mut().message = Some(("GridCraft".into(), "busy".into()));
    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_none(), "a message dialog must take IME ownership away from the grid");
}

/// A locked cell on a protected sheet cannot be edited, so inviting a composition there would
/// only produce text that `SheetApp::begin_edit` discards.
#[test]
fn locked_cell_on_a_protected_sheet_does_not_request_ime() {
    let mut h = fixture_with(protected_session("A1", None));
    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_none(), "a composition on a locked cell is always refused");
}

/// The protection guard must not disable IME for cells that are still editable.
#[test]
fn unlocked_cell_on_a_protected_sheet_still_requests_ime() {
    let mut h = fixture_with(protected_session("C2", Some("C2")));
    h.run_steps(1);
    assert!(h.output().platform_output.ime.is_some(), "unlocked cells stay editable on a protected sheet");
}

/// `IMEOutput::rect` is in screen coordinates, while `Geo::cell_rect` is in logical ones; on a
/// right-to-left sheet the two differ, and the candidate window must follow the drawn cell.
#[test]
fn right_to_left_sheet_publishes_mirrored_ime_rects() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("view.rightToLeft", json!({"on": true})).unwrap();
    s.execute("selection.set", json!({"cell": "B2"})).unwrap();
    let mut h = fixture_with(s);
    h.run_steps(1);

    let geo = h.state().grid.geo.clone().expect("the grid has been drawn");
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    let logical = geo.cell_rect(sheet, CellRef::parse("B2").unwrap());
    let expected = geo.mirror_rect(logical);
    assert!(geo.rtl, "the fixture must be right-to-left");
    assert!((expected.min.x - logical.min.x).abs() > 1.0, "the fixture must distinguish mirrored from logical x");

    let ime = h.output().platform_output.ime.as_ref().expect("the focused grid must request IME");
    assert!((ime.rect.min.x - expected.min.x).abs() < 0.01, "IME rect must use screen coordinates: got {:?}, expected {:?}", ime.rect, expected);
    assert!((ime.rect.max.x - expected.max.x).abs() < 0.01, "IME rect must use screen coordinates: got {:?}, expected {:?}", ime.rect, expected);
    assert!((ime.cursor_rect.min.x - ime.rect.min.x).abs() < 0.01, "the caret must sit at the mirrored cell's start");
}

/// A committed composition opens the editor with its text and leaves the caret after it.
#[test]
fn ime_commit_opens_editor_with_committed_text() {
    let mut h = fixture();
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("张".into())));
    h.run_steps(2);
    let ed = h.state().editor.as_ref().expect("an IME commit must open the in-cell editor");
    assert_eq!(ed.text, "张", "the committed text should seed the editor");
    assert_eq!(ed.caret, ed.text.chars().count(), "the caret should sit after the committed text");
}

/// A preedit is not text yet: it must not open the editor on its own, only the commit does.
#[test]
fn ime_preedit_does_not_seed_editor() {
    let mut h = fixture();
    h.input_mut().events.push(Event::Ime(ImeEvent::Preedit { text: "zh".into(), active_range_chars: Some(0..2) }));
    h.run_steps(2);
    assert!(h.state().editor.is_none(), "a preedit must not seed the editor on its own");
    h.input_mut().events.push(Event::Ime(ImeEvent::Commit("张".into())));
    h.run_steps(2);
    assert_eq!(editor_text(&h).as_deref(), Some("张"), "the commit after a preedit opens the editor");
}

/// Plain ASCII never composes, so `Event::Text` still seeds the editor exactly as before.
#[test]
fn ascii_text_still_seeds_editor_no_regression() {
    let mut h = fixture();
    h.input_mut().events.push(Event::Text("abc".into()));
    h.run_steps(2);
    assert_eq!(editor_text(&h).as_deref(), Some("abc"), "ASCII typing must still seed the editor");
}
