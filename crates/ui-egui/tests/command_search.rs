//! The search box (Alt+Q): opening it, filtering, keyboard and mouse choice, recents, closing.

use egui::{Event, Key, Modifiers, PointerButton, os::OperatingSystem};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::command_search::SELF_IDS;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness() -> Harness {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_os(OperatingSystem::Windows).with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(s, Default::default()),
    );
    h.run_steps(4);
    h
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers }
}

fn events(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(2);
}

fn key(h: &mut Harness, k: Key) {
    events(h, [key_event(k, true, Modifiers::NONE), key_event(k, false, Modifiers::NONE)]);
}

fn type_text(h: &mut Harness, text: &str) {
    events(h, [Event::Text(text.into())]);
}

/// Alt held, Q pressed (with the "q" text some hosts send), then both released.
fn alt_q(h: &mut Harness) {
    events(h, [Event::ModifiersChanged(Modifiers::ALT), key_event(Key::AltLeft, true, Modifiers::ALT)]);
    events(h, [key_event(Key::Q, true, Modifiers::ALT), Event::Text("q".into())]);
    events(h, [key_event(Key::Q, false, Modifiers::ALT)]);
    events(h, [Event::ModifiersChanged(Modifiers::NONE), key_event(Key::AltLeft, false, Modifiers::NONE)]);
}

fn dialog(h: &Harness) -> Option<&str> {
    h.state().dialog.as_ref().map(|d| d.name())
}

fn search_text(h: &Harness) -> String {
    h.state().dialog.as_ref().map(|d| d.search.clone()).unwrap_or_default()
}

fn last_command(h: &Harness) -> Option<String> {
    h.state().session.journal.last().map(|(id, _)| id.clone())
}

fn bold(h: &Harness) -> bool {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse("A1").unwrap())).font.bold
}

fn ui_search(h: &mut Harness, query: &str) -> Vec<Json> {
    h.state_mut().run("ui.search", json!({"query": query})).unwrap().as_array().cloned().unwrap()
}

/// Opens the search, types `query`, and runs the match `down` rows below the first with Enter.
fn run_from_search(h: &mut Harness, query: &str, down: usize) {
    alt_q(h);
    type_text(h, query);
    for _ in 0..down {
        key(h, Key::ArrowDown);
    }
    key(h, Key::Enter);
}

#[test]
fn alt_q_opens_the_search_with_the_input_focused() {
    let mut h = harness();
    alt_q(&mut h);
    assert_eq!(dialog(&h), Some("commandSearch"));
    assert_eq!(h.state().keytips.prefix(), None, "the Alt+Q chord must not leave key tips armed");
    assert!(h.state().editor.is_none(), "the Q must not start a cell edit");
    assert_eq!(search_text(&h), "", "the Q of the chord must not be typed into the search");
    assert!(h.ctx.text_edit_focused(), "the search input has keyboard focus");
    type_text(&mut h, "bold");
    assert_eq!(search_text(&h), "bold", "typing goes straight into the search");
}

#[test]
fn typing_filters_and_enter_runs_the_highlighted_first_match() {
    let mut h = harness();
    let hits = ui_search(&mut h, "bold");
    assert_eq!(hits.first().and_then(|c| c["id"].as_str()), Some("home.bold"));
    assert!(hits.iter().all(|c| c["label"].as_str().unwrap().to_lowercase().contains("bold")
        || c["menu"].to_string().to_lowercase().contains("bold")
        || c["id"].as_str().unwrap().to_lowercase().contains("bold")));
    assert!(!bold(&h));
    run_from_search(&mut h, "bold", 0);
    assert!(h.state().dialog.is_none(), "running a command closes the search");
    assert_eq!(last_command(&h).as_deref(), Some("home.bold"));
    assert!(bold(&h));
}

#[test]
fn down_and_enter_run_the_second_result() {
    let mut h = harness();
    // Recents give a known order: italic (newest), then bold.
    run_from_search(&mut h, "bold", 0);
    run_from_search(&mut h, "italic", 0);
    assert!(bold(&h));
    assert_eq!(h.state().recent_commands(), ["home.italic".to_string(), "home.bold".to_string()]);
    let before = h.state().session.active().unwrap().selection.active;
    alt_q(&mut h);
    key(&mut h, Key::ArrowDown);
    assert_eq!(h.state().dialog.as_ref().unwrap().list_index, 1);
    assert_eq!(h.state().session.active().unwrap().selection.active, before, "Down moves the highlight, not the cell");
    key(&mut h, Key::Enter);
    assert!(h.state().dialog.is_none());
    assert_eq!(last_command(&h).as_deref(), Some("home.bold"));
    assert!(!bold(&h), "the second recent (Bold) toggled bold off again");
    // Up from the first row stays on the first row.
    alt_q(&mut h);
    key(&mut h, Key::ArrowUp);
    assert_eq!(h.state().dialog.as_ref().unwrap().list_index, 0);
}

#[test]
fn empty_input_lists_recently_run_commands() {
    let mut h = harness();
    assert!(ui_search(&mut h, "").is_empty(), "nothing run yet");
    run_from_search(&mut h, "bold", 0);
    let recent = ui_search(&mut h, "");
    assert_eq!(recent.first().and_then(|c| c["id"].as_str()), Some("home.bold"));
    alt_q(&mut h);
    let _ = h.get_by_label("Recently used");
    // Clicking the listed recent runs it.
    let label = format!("Bold    Home › Font    {}", recent[0]["shortcut"].as_str().unwrap());
    h.get_by_label(&label).click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == "home.bold").count(), 2);
}

#[test]
fn escape_and_a_click_outside_close_without_running_anything() {
    let mut h = harness();
    let ran = h.state().session.journal.len();
    alt_q(&mut h);
    type_text(&mut h, "bold");
    key(&mut h, Key::Escape);
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().session.journal.len(), ran, "Esc runs nothing");

    alt_q(&mut h);
    assert_eq!(dialog(&h), Some("commandSearch"));
    let pos = h.state().grid.cells_rect.unwrap().left_bottom() + egui::vec2(40.0, -40.0);
    events(
        &mut h,
        [
            Event::PointerMoved(pos),
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
            Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ],
    );
    assert!(h.state().dialog.is_none(), "a click outside closes the search");
    assert!(!bold(&h));
    assert!(h.state().session.journal.iter().skip(ran).all(|(id, _)| id.starts_with("selection.")), "the click only selects a cell");
}

#[test]
fn the_search_never_lists_or_records_itself() {
    let mut h = harness();
    for q in ["search", "command", "palette", "tell me", "ui.search", "commandSearch"] {
        let hits = ui_search(&mut h, q);
        assert!(hits.iter().all(|c| !SELF_IDS.contains(&c["id"].as_str().unwrap())), "{q}: {hits:?}");
    }
    assert!(h.state().dialog.is_none(), "ui.search never opens the search box");
    let r = h.state_mut().run("ui.search", json!({}));
    assert_eq!(r, Ok(json!([])));
    assert!(h.state().dialog.is_none());
}

#[test]
fn a_command_that_asks_for_its_dialog_opens_it_as_from_the_ribbon() {
    let mut h = harness();
    let hits = ui_search(&mut h, "format cells");
    let at = hits.iter().position(|c| c["id"] == "home.formatCells").unwrap();
    run_from_search(&mut h, "format cells", at);
    assert_eq!(dialog(&h), Some("formatCells"));
    key(&mut h, Key::Escape);
    assert!(h.state().dialog.is_none(), "Esc from that dialog returns to the sheet, not to the search");
}

#[test]
fn plain_alt_still_shows_key_tips_and_alt_then_q_opens_the_search() {
    let mut h = harness();
    events(&mut h, [Event::ModifiersChanged(Modifiers::ALT), key_event(Key::AltLeft, true, Modifiers::ALT)]);
    events(&mut h, [Event::ModifiersChanged(Modifiers::NONE), key_event(Key::AltLeft, false, Modifiers::NONE)]);
    assert_eq!(h.state().keytips.prefix(), Some(""), "a bare Alt tap arms key tips");
    assert!(h.state().dialog.is_none());
    events(&mut h, [key_event(Key::Q, true, Modifiers::NONE), Event::Text("q".into()), key_event(Key::Q, false, Modifiers::NONE)]);
    assert_eq!(dialog(&h), Some("commandSearch"), "Alt, then Q is the search box's key tip");
    assert_eq!(h.state().keytips.prefix(), None);
    assert_eq!(search_text(&h), "", "the key tip letter is not typed into the search");
    // Key tips still reach other commands: Alt, H, 1 is Bold.
    key(&mut h, Key::Escape);
    events(&mut h, [Event::ModifiersChanged(Modifiers::ALT), key_event(Key::AltLeft, true, Modifiers::ALT)]);
    events(&mut h, [Event::ModifiersChanged(Modifiers::NONE), key_event(Key::AltLeft, false, Modifiers::NONE)]);
    for k in [Key::H, Key::Num1] {
        events(&mut h, [key_event(k, true, Modifiers::NONE), Event::Text(k.name().to_ascii_lowercase()), key_event(k, false, Modifiers::NONE)]);
    }
    assert!(bold(&h));
}
