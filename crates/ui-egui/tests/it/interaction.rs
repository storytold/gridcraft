//! UI interaction tests: real egui input through the whole app (no GPU needed).

use egui::{Event, Key, Modifiers, PointerButton, accesskit::Role};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

/// Builds the standard interaction harness around the caller's session with four warm-up passes.
fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::harness(session, 4)
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

#[test]
fn vertical_scrollbar_pages_and_drags_without_changing_selection() {
    let mut h = harness(blank());
    let grid = h.state().grid.rect.unwrap();
    let cells = h.state().grid.cells_rect.unwrap();
    let p = egui::pos2(grid.right() + 7.0, cells.bottom() - 20.0);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(h.state().view().scroll.y > 0.0, "clicking the scrollbar track scrolls down");
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));

    h.state_mut().view_mut().unwrap().scroll.y = 0.0;
    h.run_steps(2);
    let start = egui::pos2(grid.right() + 7.0, cells.top() + 8.0);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.input_mut().events.push(Event::PointerButton { pos: start, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let end = start + egui::vec2(0.0, 80.0);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.run_steps(2);
    let dragged = h.state().view().scroll.y;
    assert!(dragged > 0.0, "dragging the thumb scrolls down");
    h.run_steps(3);
    assert_eq!(h.state().view().scroll.y, dragged, "holding the thumb still must not keep scrolling");
    h.input_mut().events.push(Event::PointerButton { pos: end, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));
}

/// The first column painted right of the frozen ones.
fn first_scrolled_col(h: &egui_kittest::Harness<'static, SheetApp>) -> u32 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = app.grid.geo.as_ref().unwrap();
    geo.col_at(sheet, geo.cells.left() + geo.frozen_w + 0.5)
}

fn click(h: &mut egui_kittest::Harness<'static, SheetApp>, p: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn horizontal_scrollbar_pages_and_drags_across_a_wide_sheet() {
    let mut wb = gridcraft_engine::model::Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    for c in 0..26 {
        sh.set_value(CellRef::new(0, c), Value::text(format!("col {}", c + 1)));
    }
    // Column A frozen: a page is the scrolling part only.
    sh.freeze = Some((0, 1));
    let mut s = Session::new();
    s.add_document(gridcraft_engine::DocState::new(wb, None, "Book1".into(), gridcraft_engine::Trust::Local));
    let mut h = harness(s);
    let track = h.state().grid.hscroll.expect("the tab bar has a horizontal scrollbar");
    assert!(track.width() >= 200.0, "the scrollbar gets a usable share of the tab bar: {track:?}");
    assert_eq!(first_scrolled_col(&h), 1);

    let page = {
        let geo = h.state().grid.geo.as_ref().unwrap();
        (geo.cells.width() - geo.frozen_w) / geo.z
    };
    click(&mut h, egui::pos2(track.right() - 6.0, track.center().y));
    let scrolled = h.state().view().scroll.x;
    assert!((scrolled - page).abs() < 0.5, "clicking the track right of the thumb scrolls one page: {scrolled} vs {page}");
    assert!(first_scrolled_col(&h) > 1);
    let geo = h.state().grid.geo.clone().unwrap();
    let sheet = h.state().session.active().unwrap().wb.active().unwrap().clone();
    assert_eq!(geo.x(&sheet, 0), geo.cells.left(), "the frozen column stays put");
    click(&mut h, egui::pos2(track.right() - 6.0, track.center().y));
    let further = h.state().view().scroll.x;
    click(&mut h, egui::pos2(track.left() + 2.0, track.center().y));
    assert!(h.state().view().scroll.x < further, "clicking left of the thumb scrolls back");

    h.state_mut().view_mut().unwrap().scroll.x = 0.0;
    h.run_steps(2);
    let start = egui::pos2(track.left() + 6.0, track.center().y);
    h.input_mut().events.push(Event::PointerMoved(start));
    h.input_mut().events.push(Event::PointerButton { pos: start, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(2);
    let end = start + egui::vec2(120.0, 0.0);
    h.input_mut().events.push(Event::PointerMoved(end));
    h.run_steps(2);
    let dragged = h.state().view().scroll.x;
    assert!(first_scrolled_col(&h) > 1, "dragging the thumb right changes the first visible column");
    h.run_steps(3);
    assert_eq!(h.state().view().scroll.x, dragged, "holding the thumb still must not keep scrolling");
    h.input_mut().events.push(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));
}

fn key(h: &mut egui_kittest::Harness<'static, SheetApp>, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn text(h: &mut egui_kittest::Harness<'static, SheetApp>, t: &str) {
    h.input_mut().events.push(Event::Text(t.into()));
    h.run_steps(2);
}

fn value(h: &egui_kittest::Harness<'static, SheetApp>, a: &str) -> Value {
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(CellRef::parse(a).unwrap_or_default()))).unwrap_or_default()
}

fn click_cell(h: &mut egui_kittest::Harness<'static, SheetApp>, cell: &str) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).center();
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

fn click_chart(h: &mut egui_kittest::Harness<'static, SheetApp>, id: u32) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let anchor = sheet.charts.iter().find(|chart| chart.id == id).unwrap().anchor;
    let pos = egui::pos2(geo.x(sheet, anchor.cell.col) + anchor.dx * geo.z, geo.y(sheet, anchor.cell.row) + anchor.dy * geo.z)
        + egui::vec2(anchor.width, anchor.height) * geo.z / 2.0;
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
    ]);
    h.run_steps(2);
}

#[test]
fn keyboard_navigation_does_not_replay_the_last_pointer_click() {
    let mut h = harness(blank());
    click_cell(&mut h, "B2");
    for (key_code, modifiers, expected) in [
        (Key::Enter, Modifiers::NONE, "B3"),
        (Key::Enter, Modifiers::NONE, "B4"),
        (Key::Enter, Modifiers::NONE, "B5"),
        (Key::Enter, Modifiers::SHIFT, "B4"),
        (Key::Tab, Modifiers::NONE, "C4"),
        // #911: Enter after a Tab goes back to the column the Tab started from.
        (Key::Enter, Modifiers::NONE, "B5"),
    ] {
        key(&mut h, key_code, modifiers);
        let selection = &h.state().session.active().unwrap().selection;
        assert_eq!(selection.active.a1(), expected);
        assert_eq!(selection.anchor, selection.active);
        assert!(h.state().editor.is_none(), "navigation must not start editing at the old pointer location");
    }
    // A subsequent real pointer click must still select the clicked cell.
    click_cell(&mut h, "D6");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "D6");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "D7");
}

#[test]
fn space_toggles_the_active_checkbox_without_clicking_the_hovered_cell() {
    let mut session = blank();
    session.execute("insert.checkbox", json!({"range": "A1:B1"})).unwrap();
    let mut h = harness(session);
    click_cell(&mut h, "A1");
    assert_eq!(value(&h, "A1"), Value::Bool(true));
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    key(&mut h, Key::Space, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "B1");
    assert_eq!(value(&h, "A1"), Value::Bool(true), "the old pointer location must remain unchanged");
    assert_eq!(value(&h, "B1"), Value::Bool(true));
    assert!(h.state().editor.is_none());
}

#[test]
/// Name Box Enter selects A1; the next grid Enter moves down to A2.
fn name_box_enter_goes_to_the_typed_cell_not_the_one_below() {
    // #247: typing A1 in the Name Box and pressing Enter selected A2; the grid replayed the Enter.
    let mut h = harness(blank());
    h.state_mut().session.run("selection.set", json!({"cell": "C5"})).unwrap();
    h.run_steps(2);
    h.ctx.memory_mut(|m| m.request_focus(egui::Id::unique("gridcraft.name_box")));
    h.run_steps(2);
    h.input_mut().events.push(Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND });
    h.input_mut().events.push(Event::Text("A1".into()));
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(0, 0));
    assert!(h.state().name_box.is_none());
    // The grid has the focus back: a plain Enter there still moves down.
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active, CellRef::new(1, 0));
}

#[test]
fn name_box_shows_a_name_defined_on_a_sheet_with_an_apostrophe() {
    // #1458: the quotes of `'Bob''s'!$A$1:$B$2` were taken off without reading `''` as `'`.
    let mut h = harness(blank());
    let session = &mut h.state_mut().session;
    session.run("home.insertSheet", json!({"name": "Bob's"})).unwrap();
    session.run("sheet.activate", json!({"sheet": "Bob's"})).unwrap();
    session.run("formulas.defineName", json!({"name": "Box", "refersTo": "='Bob''s'!$A$1:$B$2"})).unwrap();
    session.run("selection.set", json!({"range": "A1:B2"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().name_box_text(), "Box");
}

#[test]
/// Typing and Enter commit 42 in A1; `=A1*2` and Tab commit 84 in A2 and move to B2, with accessibility checks.
fn typing_enters_values_and_moves_down() {
    let mut h = harness(blank());
    text(&mut h, "42");
    assert!(h.state().editor.is_some(), "typing starts editing");
    h.check_accessibility();
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Number(42.0));
    text(&mut h, "=A1*2");
    h.check_accessibility();
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(value(&h, "A2"), Value::Number(84.0));
    let active = h.state().session.active().map(|d| d.selection.active.a1());
    assert_eq!(active.as_deref(), Some("B2"));
}

#[test]
fn enter_after_tabs_returns_to_the_column_where_the_tabs_started() {
    // #911: Excel moves to the next row in the column the run of Tabs started from.
    let mut h = harness(blank());
    for t in ["a", "b"] {
        text(&mut h, t);
        key(&mut h, Key::Tab, Modifiers::NONE);
    }
    text(&mut h, "c");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "C1"), Value::from("c"));
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A2");
    // The next Enter is an ordinary one again.
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A3");
}

#[test]
fn moving_with_an_arrow_ends_the_run_of_tabs() {
    let mut h = harness(blank());
    key(&mut h, Key::Tab, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "C2");
}

#[test]
fn an_arrow_that_commits_typing_ends_the_run_of_tabs() {
    // Excel 365 from B2: the arrow is an ordinary move, neither one more Tab (Right) nor an Enter
    // back to the starting column (Down).
    for (arrow, after_enter) in [(Key::ArrowRight, "D3"), (Key::ArrowDown, "C4")] {
        let mut s = blank();
        s.execute("selection.set", json!({"cell": "B2"})).unwrap();
        let mut h = harness(s);
        text(&mut h, "a");
        key(&mut h, Key::Tab, Modifiers::NONE);
        text(&mut h, "b");
        key(&mut h, arrow, Modifiers::NONE);
        assert_eq!(value(&h, "C2"), Value::from("b"), "{arrow:?}");
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert_eq!(h.state().session.active().unwrap().selection.active.a1(), after_enter, "{arrow:?}");
    }
}

#[test]
fn arrows_escape_and_undo() {
    let mut h = harness(blank());
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(h.state().session.active().map(|d| d.selection.active.a1()).as_deref(), Some("B2"));
    text(&mut h, "hello");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), Value::Empty);
    text(&mut h, "x");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "B2"), Value::from("x"));
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(value(&h, "B2"), Value::Empty);
}

#[test]
fn shift_arrows_in_point_mode_extend_from_the_pointed_cell() {
    let mut s = blank();
    s.execute("selection.set", json!({"cell": "A11"})).unwrap();
    let mut h = harness(s);
    text(&mut h, "=SUM(");
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    let edited = |h: &egui_kittest::Harness<'static, SheetApp>| h.state().editor.as_ref().map(|e| e.text.clone());
    assert_eq!(edited(&h).as_deref(), Some("=SUM(A10"));
    // Shift is held down, as on a real keyboard: the backend reports it before the arrow keys.
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    for expected in ["=SUM(A9:A10", "=SUM(A8:A10", "=SUM(A7:A10"] {
        key(&mut h, Key::ArrowUp, Modifiers::SHIFT);
        assert_eq!(edited(&h).as_deref(), Some(expected));
    }
    key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(edited(&h).as_deref(), Some("=SUM(A8:A10"), "Shift+Down shrinks back towards the anchor");
}

#[test]
fn shortcuts_format_and_extend() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1, 2], [3, 4]]})).unwrap();
    let mut h = harness(s);
    key(&mut h, Key::ArrowRight, Modifiers::SHIFT);
    key(&mut h, Key::ArrowDown, Modifiers::SHIFT);
    assert_eq!(h.state().session.active().map(|d| d.selection.a1()).as_deref(), Some("A1:B2"));
    key(&mut h, Key::B, Modifiers::COMMAND);
    let bold = h.state_mut().session.run("cell.get", json!({"cell": "B2"})).unwrap()["style"]["font"]["bold"].clone();
    assert_eq!(bold, json!(true));
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Empty);
}

#[test]
fn delete_key_removes_the_selected_chart() {
    let mut session = blank();
    session.execute("range.setValues", json!({"range": "A1:B2", "values": [[1, 2], [3, 4]]})).unwrap();
    let id = session.execute("insert.chart", json!({"range": "A1:B2", "at": "D2"})).unwrap()["chart"].as_u64().unwrap() as u32;
    let mut h = harness(session);

    click_cell(&mut h, "C10");
    click_chart(&mut h, id);
    assert_eq!(h.state().selected_chart, Some(id));

    key(&mut h, Key::Delete, Modifiers::NONE);

    assert!(h.state().session.active().unwrap().wb.active().unwrap().charts.is_empty());
    assert_eq!(h.state().selected_chart, None);
    assert_eq!(value(&h, "A1"), Value::Number(1.0));
}

#[test]
fn inserted_chart_is_selected_so_delete_removes_it() {
    // #668, #256: after Insert › Chart the selection stayed on the source cells, so Delete
    // cleared the data and left the chart behind.
    let mut session = blank();
    session.execute("range.setValues", json!({"range": "A1:B2", "values": [[1, 2], [3, 4]]})).unwrap();
    session.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
    let mut h = harness(session);

    let id = h.state_mut().run("insert.chart", json!({"type": "column"})).unwrap()["chart"].as_u64().unwrap() as u32;
    h.run_steps(2);
    assert_eq!(h.state().selected_chart, Some(id), "the new chart is selected");

    key(&mut h, Key::Delete, Modifiers::NONE);

    assert!(h.state().session.active().unwrap().wb.active().unwrap().charts.is_empty());
    assert_eq!(value(&h, "A1"), Value::Number(1.0));
}

#[test]
fn insert_column_or_bar_menu_inserts_a_hundred_percent_stacked_bar() {
    // Insert › Column or Bar (keytip N, C1) lists the column and bar types of the All Charts list.
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Quarter", "North", "South"], ["Q1", 10, 30], ["Q2", 30, 10]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:C3"})).unwrap();
    let mut h = harness(s);
    key(&mut h, Key::F10, Modifiers::NONE);
    for (k, t) in [(Key::N, "n"), (Key::C, "c"), (Key::Num1, "1")] {
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
        h.input_mut().events.push(Event::Text(t.into()));
        h.input_mut().events.push(Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    h.run_steps(2);
    for label in ["Clustered Column", "Stacked Column", "100% Stacked Column", "Clustered Bar", "Stacked Bar", "100% Stacked Bar"] {
        assert!(h.query_by_role_and_label(egui::accesskit::Role::Button, label).is_some(), "the menu lists {label}");
    }
    h.get_by_role_and_label(egui::accesskit::Role::Button, "100% Stacked Bar").click();
    h.run_steps(2);
    let kinds: Vec<_> = h.state().session.active().unwrap().wb.active().unwrap().charts.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [gridcraft_engine::model::ChartKind::BarStacked100]);
}

#[test]
/// Every main ribbon tab and dark mode render with accessibility checks.
fn renders_every_ribbon_tab_without_panicking() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    let mut h = harness(s);
    for tab in ["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"] {
        h.state_mut().ui.ribbon_tab = tab.into();
        h.run_steps(2);
        h.check_accessibility();
    }
    h.state_mut().ui.dark = true;
    h.run_steps(3);
    h.check_accessibility();
}

#[test]
/// Language commands are listed and every available locale renders and persists; CJK locales are omitted when craft fonts are absent.
fn language_switch_renders_the_ribbon_and_persists() {
    let mut h = harness(blank());
    // English by default whatever the host's locale: only the desktop app reads the system language.
    assert_eq!(h.state().ui.language, Language::En);
    // The language commands are listed with the other UI commands.
    let ctx = h.ctx.clone();
    let gridcraft_ui_egui::control::Outcome::Done(listed) = gridcraft_ui_egui::control::handle(h.state_mut(), &ctx, "engine.commands", &json!({}))
    else {
        panic!("engine.commands returns a response");
    };
    let ids: Vec<&str> = listed["result"].as_array().into_iter().flatten().filter_map(|c| c["id"].as_str()).collect();
    for id in ["app.language.set", "app.language.english", "app.language.japanese"] {
        assert!(ids.contains(&id), "engine.commands lists {id}");
    }
    // A minimal CI image has egui's bundled faces but no Japanese/Korean/Chinese face. Those
    // catalogs are exercised when the pinned craft-fonts input is present; skipping them here is
    // the documented no-font gap, not a missing-glyph or accessibility opt-out.
    let languages: Vec<Language> = Language::ALL
        .into_iter()
        .filter(|l| gridcraft_ui_egui::theme::craft_fonts_loaded() || !matches!(l, Language::Ja | Language::Ko | Language::Zh))
        .collect();
    for language in languages {
        h.state_mut().run("app.language.set", json!({"language": language.code()})).unwrap();
        h.run_steps(2); // every available locale renders the ribbon without panicking
        h.check_accessibility();
        assert_eq!(h.state().ui.language, language);
        let restored: gridcraft_ui_egui::UiState = serde_json::from_str(&serde_json::to_string(&h.state().ui).unwrap()).unwrap();
        assert_eq!(restored.language, language);
    }
    h.state_mut().run("app.language.set", json!({"language": "en"})).unwrap();
    assert_eq!(h.state().ui.language, Language::En);
}

#[test]
/// The PivotTable Fields pane and its field menus switch to Spanish.
fn pivot_fields_pane_follows_the_interface_language() {
    // #858: the PivotTable Fields pane and its field menus stayed in English after switching to Spanish.
    let mut h = harness(blank());
    h.state_mut().run("range.setValues", json!({"range": "A1:B3", "values": [["Region", "Sales"], ["North", 10], ["South", 20]]})).unwrap();
    h.state_mut().run("insert.pivotTable", json!({"source": "Sheet1!$A$1:$B$3", "rows": ["Region"]})).unwrap();
    h.state_mut().run("app.language.set", json!({"language": "es"})).unwrap();
    h.run_steps(4);
    h.check_accessibility();
    let es = |english| Language::Es.tr(english);
    for english in ["PivotTable Fields", "Rows", "Values", "Refresh", "Calculated Field…"] {
        assert!(h.query_by_label(es(english)).is_some(), "the pane shows `{}`", es(english));
    }
    if es("PivotTable Fields") != "PivotTable Fields" {
        assert!(h.query_by_label("PivotTable Fields").is_none());
    }
    h.get_by_label("Region ▾").click();
    h.run_steps(2);
    h.check_accessibility();
    for english in ["Move to Column Labels", "Group by Years", "Remove Field"] {
        assert!(h.query_by_label(es(english)).is_some(), "the field menu shows `{}`", es(english));
    }
    if es("Remove Field") != "Remove Field" {
        assert!(h.query_by_label("Remove Field").is_none());
    }
}

#[test]
fn pivot_fields_pane_reuses_its_field_list_until_the_workbook_changes() {
    // #2113: the pane ran `pivot.fields`, a scan of the whole source list, on every frame.
    let mut h = harness(blank());
    h.state_mut().run("range.setValues", json!({"range": "A1:B3", "values": [["Region", "Sales"], ["North", 10], ["South", 20]]})).unwrap();
    h.state_mut().run("insert.pivotTable", json!({"source": "Sheet1!$A$1:$B$3", "rows": ["Region"], "destination": "Sheet1!D1"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Sales").is_some(), "the pane lists the source fields");
    let (key, mut info) = h.state().grid.pivot_fields.clone().expect("the pane keeps its field list");
    info["fields"][1]["name"] = json!("Cached");
    h.state_mut().grid.pivot_fields = Some((key, info));
    h.run_steps(2);
    assert!(h.query_by_label("Cached").is_some(), "repaints reuse the kept list instead of scanning the source again");
    h.state_mut().run("cell.set", json!({"cell": "B1", "input": "Revenue"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Cached").is_none(), "an edit drops the kept list");
    assert!(h.query_by_label("Revenue").is_some(), "the pane shows the edited source");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Sales").is_some(), "undo brings the previous field list back");
}

#[test]
fn format_cells_draws_its_labels_in_the_interface_language() {
    let mut h = harness(blank());
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.state_mut().open_dialog("formatCells", json!({"tab": "Number"}));
    h.run_steps(2);
    let scientific = Language::De.tr("Scientific");
    assert!(h.query_by_label(scientific).is_some(), "the Number tab's categories are translated");
    if scientific != "Scientific" {
        assert!(h.query_by_label("Scientific").is_none(), "no English category is left");
    }
    h.get_by_label(scientific).click();
    h.run_steps(2);
    let style = &h.state().dialog.as_ref().unwrap().values["style"];
    assert_eq!(style["num_fmt"], json!("0.00E+00"), "a translated category still applies its code");
    h.state_mut().dialog = None;
    h.state_mut().run("app.language.set", json!({"language": "en"})).unwrap();
    h.state_mut().open_dialog("formatCells", json!({"tab": "Number"}));
    h.run_steps(2);
    assert!(h.query_by_label("Scientific").is_some(), "English is unchanged");
}

#[test]
fn name_manager_new_and_edit_open_the_name_dialog_and_come_back() {
    // #238: New… in the Name Manager did nothing (the manager put itself back over the dialog it
    // had just opened).
    let mut h = harness(blank());
    h.state_mut().open_dialog("nameManager", json!({}));
    h.run_steps(4);
    h.get_by_label("New…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.title.as_str()), Some("New Name"));
    if let Some(d) = h.state_mut().dialog.as_mut() {
        d.values.insert("name".into(), json!("Total"));
        d.values.insert("refersTo".into(), json!("=Sheet1!$A$1"));
    }
    h.run_steps(1);
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(2);
    let names = |h: &egui_kittest::Harness<'static, SheetApp>| {
        h.state().session.active().unwrap().wb.names.iter().map(|n| n.name.clone()).collect::<Vec<_>>()
    };
    assert_eq!(names(&h), ["Total"]);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("nameManager"), "OK returns to the Name Manager");

    h.get_by_label("Edit…").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.title.as_str()), Some("Edit Name"));
    if let Some(d) = h.state_mut().dialog.as_mut() {
        d.values.insert("name".into(), json!("GrandTotal"));
    }
    h.run_steps(1);
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(2);
    assert_eq!(names(&h), ["GrandTotal"], "Edit renames instead of adding a second name");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("nameManager"));
}

#[test]
fn recommended_charts_dialog_inserts_the_chosen_chart() {
    // #215: Recommended Charts opens a dialog of chart types for the data instead of inserting a
    // column chart straight away; OK inserts the one picked.
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Fruit", "Sales"], ["Apples", 5], ["Pears", 3], ["Plums", 2]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:B4"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("recommendedCharts", json!({}));
    h.run_steps(4);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("recommendedCharts"));
    h.get_by_label("Pie").click();
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let kinds: Vec<_> = h.state().session.active().unwrap().wb.active().unwrap().charts.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [gridcraft_engine::model::ChartKind::Pie]);
    assert!(h.state().selected_chart.is_some(), "the new chart is selected");

    // Without numbers there is nothing to recommend: a message, no dialog.
    h.state_mut().session.run("selection.set", json!({"range": "A1:A4"})).unwrap();
    h.state_mut().open_dialog("recommendedCharts", json!({}));
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.is_some());
}

#[test]
/// Selecting 100% Stacked Area in the All Charts list inserts that chart type.
fn all_charts_inserts_a_hundred_percent_stacked_area() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Quarter", "North", "South"], ["Q1", 10, 30], ["Q2", 30, 10]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:C3"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("recommendedCharts", json!({"tab": "All Charts"}));
    h.run_steps(4);
    // The entry sits below the list's visible height: scroll the list with the wheel first.
    let first = h.get_by_label("Pie").rect().center();
    h.input_mut().events.push(Event::PointerMoved(first));
    h.input_mut().events.push(Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        source: egui::MouseWheelSource::Unknown,
        delta: egui::vec2(0.0, -10.0),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(20);
    h.get_by_label("100% Stacked Area").click();
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let kinds: Vec<_> = h.state().session.active().unwrap().wb.active().unwrap().charts.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [gridcraft_engine::model::ChartKind::AreaStacked100]);
}

#[test]
fn recommended_charts_command_without_a_type_opens_the_dialog() {
    // #1399: the ribbon button and the NR keytip ran the command directly and inserted the first
    // recommendation (a scatter plot) instead of showing the chart picker.
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Fruit", "Sales"], ["Apples", 5], ["Pears", 3], ["Plums", 2]]})).unwrap();
    s.execute("selection.set", json!({"range": "A1:B4"})).unwrap();
    let mut h = harness(s);
    h.state_mut().run_or_alert("insert.recommendedCharts", json!({}));
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("recommendedCharts"));
    assert!(h.state().session.active().unwrap().wb.active().unwrap().charts.is_empty(), "no chart is inserted before one is picked");
}

#[test]
fn select_data_dialog_sets_the_category_axis_labels() {
    // #287: Chart Design › Select Data edits the Horizontal (Category) Axis Labels and removes the
    // series the X values were plotted as.
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Year", "Sales"], [2020, 5], [2021, 7], [2022, 9]]})).unwrap();
    let id = s.execute("insert.chart", json!({"range": "A1:B4", "type": "line"})).unwrap()["chart"].as_u64().unwrap();
    let mut h = harness(s);
    h.state_mut().selected_chart = Some(id as u32);
    h.state_mut().open_dialog("selectData", json!({"chart": id}));
    h.run_steps(2);
    let d = h.state_mut().dialog.as_mut().unwrap();
    assert_eq!(d.title, "Select Data Source");
    assert_eq!(d.values["names"], json!(["Year", "Sales"]));
    d.values.insert("categories".into(), json!("=A2:A4"));
    d.values.insert("remove".into(), json!([0]));
    h.run_steps(1);
    key(&mut h, Key::Enter, Modifiers::NONE);
    h.run_steps(2);
    assert!(h.state().dialog.is_none());
    let chart = h.state().session.active().unwrap().wb.active().unwrap().charts[0].clone();
    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].categories.as_deref(), Some("Sheet1!$A$2:$A$4"));
    assert_eq!(chart.series[0].values, "Sheet1!$B$2:$B$4");
}

#[test]
/// Each listed dialog opens, passes accessibility and closes.
fn dialogs_open_and_close() {
    let mut h = harness(blank());
    for name in [
        "formatCells",
        "insertFunction",
        "find",
        "sort",
        "nameManager",
        "dataValidation",
        "pasteSpecial",
        "commandSearch",
        "spelling",
        "goTo",
        "pivotCalculatedField",
        "about",
        "macros",
        "recordMacro",
        "macroEditor",
        "analysisToolPak",
        "solver",
    ] {
        h.state_mut().open_dialog(name, json!({}));
        h.run_steps(2);
        h.check_accessibility();
        key(&mut h, Key::Escape, Modifiers::NONE);
        h.state_mut().dialog = None;
        h.state_mut().message = None;
    }
}

#[test]
fn data_analysis_dialog_runs_a_tool_and_asks_before_overwriting() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [2], [3], [4], [5]]})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("analysisToolPak", json!({}));
    h.state_mut().dialog.as_mut().unwrap().values.insert("pick".into(), json!("descriptive"));
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.title.clone()).as_deref(), Some("Descriptive Statistics"));
    let fill = |h: &mut egui_kittest::Harness<'static, SheetApp>| {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("range".into(), json!("A1:A5"));
        d.values.insert("outputTo".into(), json!("range"));
        d.values.insert("output".into(), json!("C1"));
    };
    fill(&mut h);
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().dialog.is_none(), "OK runs the tool and closes the dialog");
    assert_eq!(value(&h, "C3"), Value::from("Mean"));
    assert_eq!(value(&h, "D3"), Value::Number(3.0));
    // A second report into the same cells asks before replacing them (opened with a tool, the
    // dialog starts at that tool's form).
    h.state_mut().open_dialog("analysisToolPak", json!({"tool": "descriptive"}));
    fill(&mut h);
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    let d = h.state().dialog.as_ref().expect("the dialog stays open to ask");
    assert_eq!(d.values.get("askOverwrite"), Some(&json!(true)));
    assert!(h.state().message.is_none());
}

#[test]
fn solver_dialog_loads_the_saved_model_and_results_can_restore() {
    let mut s = blank();
    for (c, v) in [("A1", "0"), ("A2", "0"), ("B1", "=3*A1+5*A2"), ("B2", "=2*A2"), ("B3", "=3*A1+2*A2")] {
        s.execute("cell.set", json!({"cell": c, "input": v})).unwrap();
    }
    let r = s
        .execute(
            "data.solver",
            json!({"objective": "B1", "variables": "A1:A2", "constraints": ["A1 <= 4", "B2 <= 12", "B3 <= 18"], "method": "simplex"}),
        )
        .unwrap();
    assert_eq!(s.active().unwrap().wb.active().unwrap().value(CellRef::parse("B1").unwrap()), Value::Number(36.0));
    let mut h = harness(s);
    h.state_mut().open_dialog("solver", json!({}));
    h.run_steps(2);
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!(d.values.get("objective"), Some(&json!("B1")));
    assert_eq!(d.values.get("method"), Some(&json!("simplex")));
    assert_eq!(d.values.get("constraints").and_then(|c| c.as_array()).map(Vec::len), Some(3));
    h.state_mut().dialog = None;
    // Solver Results › Restore Original Values undoes the solution.
    h.state_mut().open_dialog("solverResults", r);
    h.state_mut().dialog.as_mut().unwrap().values.insert("restore".into(), json!(true));
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert_eq!(value(&h, "A1"), Value::Number(0.0));
    assert_eq!(value(&h, "B1"), Value::Number(0.0));
}

#[test]
fn solver_results_write_the_chosen_reports() {
    let mut s = blank();
    for (c, v) in [("A1", "0"), ("A2", "0"), ("B1", "=3*A1+5*A2"), ("B2", "=2*A2"), ("B3", "=3*A1+2*A2")] {
        s.execute("cell.set", json!({"cell": c, "input": v})).unwrap();
    }
    let r = s
        .execute(
            "data.solver",
            json!({"objective": "B1", "variables": "A1:A2", "constraints": ["A1 <= 4", "B2 <= 12", "B3 <= 18"], "method": "simplex"}),
        )
        .unwrap();
    let mut h = harness(s);
    // Restore Original Values with the Answer and Limits reports: the cells go back, the reports stay.
    h.state_mut().open_dialog("solverResults", r);
    for key in ["restore", "answerReport", "limitsReport"] {
        h.state_mut().dialog.as_mut().unwrap().values.insert(key.into(), json!(true));
    }
    h.run_steps(2);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.is_none(), "{:?}", h.state().message);
    let names: Vec<String> = h.state().session.active().unwrap().wb.sheets.iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, ["Answer Report 1", "Limits Report 1", "Sheet1"]);
    assert_eq!(value(&h, "B1"), Value::Number(0.0));
}

#[test]
fn about_renders_every_tab() {
    let mut h = harness(blank());
    for tab in ["About", "Contributors", "Models"] {
        h.state_mut().open_dialog("about", json!({"tab": tab}));
        h.run_steps(2);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.tab.clone()).as_deref(), Some(tab));
        h.state_mut().dialog = None;
    }
}

#[test]
fn column_autocomplete_completes_on_enter() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["North"], ["East"], ["Eastern"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A4"})).unwrap();
    let mut h = harness(s);
    text(&mut h, "No");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A4"), Value::from("North"));
    text(&mut h, "Ea"); // ambiguous: East / Eastern
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A5"), Value::from("Ea"));
}

fn completion(h: &egui_kittest::Harness<'static, SheetApp>) -> (String, Option<String>) {
    let ed = h.state().editor.as_ref().expect("still editing");
    (ed.text.clone(), ed.completion.clone())
}

#[test]
fn column_autocomplete_backspace_and_escape_drop_only_the_proposal() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Apple"], ["Banana"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A3"})).unwrap();
    let mut h = harness(s);
    text(&mut h, "ap");
    assert_eq!(completion(&h), ("ap".into(), Some("Apple".into())), "\"ple\" is proposed after the caret");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A3"), Value::from("Apple"), "Enter accepts it, with the entry's own case");
    text(&mut h, "ba");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(value(&h, "A4"), Value::from("Banana"), "Tab accepts it too");

    // Backspace removes the proposal, not the typed "a".
    h.state_mut().run("selection.set", json!({"cell": "A5"})).unwrap();
    h.run_steps(2);
    text(&mut h, "ba");
    key(&mut h, Key::Backspace, Modifiers::NONE);
    assert_eq!(completion(&h), ("ba".into(), None));
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A5"), Value::from("ba"));

    // Escape dismisses the proposal and keeps the edit; a second Escape cancels it.
    text(&mut h, "ap");
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(completion(&h), ("ap".into(), None));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().editor.is_none());
    assert_eq!(value(&h, "A6"), Value::Empty);
}

#[test]
fn column_autocomplete_skips_ambiguous_prefixes_and_numbers() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Apricot"], ["Apple"], ["12 Main St"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A4"})).unwrap();
    let mut h = harness(s);
    text(&mut h, "ap");
    assert_eq!(completion(&h), ("ap".into(), None), "Apricot and Apple both match");
    text(&mut h, "p");
    assert_eq!(completion(&h), ("app".into(), Some("Apple".into())), "one more letter settles it");
    h.state_mut().cancel_edit();
    text(&mut h, "12");
    assert_eq!(completion(&h), ("12".into(), None), "a number is not completed");
}

#[test]
fn column_autocomplete_folds_case_beyond_ascii() {
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Ébène"], ["Ébano"], ["ébano"], ["STRAẞENBAHN"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A5"})).unwrap();
    let mut h = harness(s);
    text(&mut h, "éba");
    assert_eq!(completion(&h), ("éba".into(), Some("ébano".into())), "Ébano and ébano are one entry, Ébène doesn't match");
    h.state_mut().cancel_edit();
    text(&mut h, "éb");
    assert_eq!(completion(&h).1, None, "Ébène and Ébano differ past the prefix");
    h.state_mut().cancel_edit();
    // "ß" is two bytes, "ẞ" three: the proposed rest starts after the fifth character, not the sixth byte.
    text(&mut h, "straß");
    assert_eq!(completion(&h), ("straß".into(), Some("STRAẞENBAHN".into())));
    let painted = h.output().shapes.iter().any(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.job.text == "ENBAHN"));
    assert!(painted, "the rest \"ENBAHN\" is drawn after the caret");
}

#[test]
fn column_autocomplete_can_be_turned_off() {
    assert!(gridcraft_ui_egui::UiState::default().autocomplete);
    let old: gridcraft_ui_egui::UiState = serde_json::from_str(r#"{"dark": true}"#).unwrap();
    assert!(old.autocomplete, "a ui.json written before the setting existed keeps it on");

    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [["North"]]})).unwrap();
    s.execute("selection.set", json!({"cell": "A2"})).unwrap();
    let mut h = harness(s);
    assert_eq!(h.state_mut().run("options.autoComplete", json!({"on": false})), Ok(json!({"on": false})));
    assert!(h.state().dialog.is_none());
    text(&mut h, "No");
    assert_eq!(completion(&h), ("No".into(), None));
    text(&mut h, "r");
    assert_eq!(completion(&h), ("Nor".into(), None), "typing on doesn't bring it back");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A2"), Value::from("Nor"));
    assert_eq!(h.state_mut().run("options.autoComplete", json!({})), Ok(json!({"on": true})), "no parameters: it toggles");
}

/// An .xlsm carrying a VBA project, saved to bytes.
fn macro_workbook_bytes() -> Vec<u8> {
    let mut wb = gridcraft_engine::model::Workbook::new();
    wb.vba_project = Some(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1 fake OLE2 vbaProject".to_vec().into());
    gridcraft_engine::io::save_bytes(&wb, "macros.xlsm").unwrap()
}

#[test]
fn opening_a_macro_workbook_shows_a_notice() {
    // The reader's "macros kept but not executed" warning must reach a dismissible info bar,
    // not be silently dropped (issue #6 follow-up: Excel-style message bar).
    let mut h = harness(blank());
    let b64 = gridcraft_engine::io::base64_encode(&macro_workbook_bytes());
    let res = h.state_mut().session.run("file.open", json!({"name": "macros.xlsm", "base64": b64})).unwrap();
    assert!(
        res.get("warnings").and_then(|w| w.as_array()).is_some_and(|a| a.iter().any(|m| m.as_str().is_some_and(|s| s.contains("macros")))),
        "engine reports the macro warning: {res}"
    );
    // The UI-side handler turns that open result into the notice (open_path does this in the app).
    h.state_mut().show_open_warnings(&res);
    let n = h.state().notice.clone().expect("a notice was shown");
    assert!(n.text.contains("VBA"), "{}", n.text);
    assert_eq!(n.kind, gridcraft_ui_egui::NoticeKind::Warning);
    assert_eq!(n.action.as_ref().map(|a| a.1.as_str()), Some("macros"), "action opens the Macros dialog");

    // The notice bar renders without panic; `ui.notice` surfaces it; dismissing clears it.
    h.run_steps(2);
    let got = h.state_mut().run("ui.notice", json!({})).unwrap();
    assert_eq!(got.get("kind").and_then(|k| k.as_str()), Some("warning"));
    h.state_mut().run("ui.notice.dismiss", json!({})).unwrap();
    h.run_steps(2);
    assert!(h.state().notice.is_none());
}

#[test]
fn dropped_macro_warning_never_promises_to_keep_macros() {
    let mut h = harness(blank());
    for w in ["macros (VBA project) reference a missing part and were dropped", "macros (VBA project) could not be read and were dropped: bad zip"] {
        h.state_mut().show_open_warnings(&json!({"warnings": [w]}));
        let n = h.state().notice.clone().unwrap();
        assert_eq!(n.kind, gridcraft_ui_egui::NoticeKind::Warning);
        assert!(n.text.contains("dropped") && !n.text.contains("keeps them"), "{}", n.text);
    }
    h.state_mut().show_open_warnings(&json!({"warnings": ["macros (VBA project) were kept but not executed"]}));
    assert!(h.state().notice.clone().unwrap().text.contains("keeps them when you save as .xlsm"));
}

#[test]
fn plain_open_warning_becomes_an_info_notice() {
    let mut h = harness(blank());
    h.state_mut().show_open_warnings(&json!({"warnings": ["links to external workbooks are not supported"]}));
    let n = h.state().notice.clone().unwrap();
    assert_eq!(n.kind, gridcraft_ui_egui::NoticeKind::Info);
    assert!(n.action.is_none());
    assert!(n.text.contains("external"));
    // No warnings → no notice, no panic.
    h.state_mut().show_open_warnings(&json!({"warnings": []}));
    assert!(h.state().notice.is_none());
}

#[test]
fn macros_dialog_lists_vba_and_scripts() {
    let mut s = blank();
    let mut wb = gridcraft_engine::model::Workbook::new();
    wb.vba_project = Some(vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1].into());
    s.active_mut().unwrap().wb = std::sync::Arc::new(wb);
    let mut h = harness(s);
    h.state_mut().open_dialog("macros", json!({}));
    h.run_steps(3);
    let d = h.state().dialog.as_ref().expect("macros dialog open");
    assert_eq!(d.name(), "macros");
    let items = d.result.as_ref().and_then(|r| r.as_array()).cloned().unwrap_or_default();
    assert!(items.iter().any(|i| i.get("kind").and_then(|k| k.as_str()) == Some("vba")), "{items:?}");
    h.state_mut().dialog = None;
}

#[test]
fn status_bar_shows_macro_badge() {
    let mut s = blank();
    let mut wb = gridcraft_engine::model::Workbook::new();
    wb.vba_project = Some(vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1].into());
    s.active_mut().unwrap().wb = std::sync::Arc::new(wb);
    // Rendering the status bar must not panic with a VBA project present.
    let mut h = harness(s);
    h.run_steps(3);
    h.state_mut().ui.dark = true;
    h.run_steps(2);
}

#[test]
fn status_bar_names_a_circular_reference_by_its_address() {
    let mut s = blank();
    s.execute("home.insertSheet", json!({"name": "Bob's Q1"})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "=B2+1"})).unwrap();
    let h = harness(s);
    assert!(h.query_by_label("Circular References: B2").is_some(), "the status bar names the cell on the active sheet without its sheet");
}

#[test]
fn status_bar_names_no_cell_when_every_circular_reference_is_on_another_sheet() {
    let mut s = blank();
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "=B2+1"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    let mut h = harness(s);
    assert!(h.query_by_label("Circular References").is_some(), "the status bar still reports circular references");
    assert!(h.query_by_label("Circular References: B2").is_none(), "no cell address from another sheet");
    h.state_mut().session.execute("sheet.activate", json!({"sheet": "Data"})).unwrap();
    h.run_steps(2);
    assert!(h.query_by_label("Circular References: B2").is_some(), "the active sheet's loop is named");
}

#[test]
fn status_bar_names_a_circular_reference_on_the_active_sheet() {
    let mut s = blank();
    s.execute("cell.set", json!({"cell": "A1", "input": "=A1+1"})).unwrap();
    s.execute("home.insertSheet", json!({"name": "Data"})).unwrap();
    s.execute("cell.set", json!({"cell": "B2", "input": "=B2+1"})).unwrap();
    s.execute("formulas.calculateNow", json!({})).unwrap();
    let circular = s.active().unwrap().calc.circular.clone();
    assert_eq!(circular, vec![(0, CellRef::new(0, 0)), (1, CellRef::new(1, 1))], "Sheet1's loop is listed first");
    let h = harness(s);
    assert!(h.query_by_label("Circular References: B2").is_some(), "the status bar names the loop on the active sheet");
    assert!(h.query_by_label("Circular References: A1").is_none(), "not the first loop listed");
}

#[test]
fn alt_f8_opens_the_macros_dialog() {
    let mut h = harness(blank());
    key(&mut h, Key::F8, Modifiers::ALT);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("macros"));
    h.state_mut().dialog = None;
}

#[test]
fn general_numbers_fit_by_the_cells_own_font() {
    // 9pt marks in narrow columns (a stored width of 4, i.e. 28px; common in mark sheets): "100" fits.
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[100, 123456789]]})).unwrap();
    s.execute("home.fontSize", json!({"range": "A1:B1", "size": 9})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "width": 28})).unwrap();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let h = harness(s);
    let texts: Vec<String> = h
        .output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    assert!(texts.iter().any(|t| t == "100"), "100 drawn in full: {texts:?}");
    // A number that really doesn't fit still shows #### (once: only B1).
    assert_eq!(texts.iter().filter(|t| t.starts_with('#')).count(), 1, "{texts:?}");
}

fn text_shapes(shape: &egui::Shape, clip: egui::Rect, out: &mut Vec<(egui::epaint::TextShape, egui::Rect)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.clone(), clip)),
        egui::Shape::Vec(v) => v.iter().for_each(|s| text_shapes(s, clip, out)),
        _ => {}
    }
}

#[test]
fn rotated_wrapped_text_is_drawn_rotated_inside_its_cell() {
    // Narrow columns with tall, upright headers (mark sheets): rotation and Wrap Text together.
    let header = "Term 1 : Drama : Continuous Assessment";
    let mut s = blank();
    s.execute("range.setValues", json!({"range": "A1", "values": [[header, header]]})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1:B1", "on": true})).unwrap();
    s.execute("home.orientation", json!({"range": "A1", "angle": "up"})).unwrap();
    s.execute("home.orientation", json!({"range": "B1", "angle": "down"})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "chars": 6})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1:1", "height": 267})).unwrap();
    s.execute("selection.set", json!({"cell": "C3"})).unwrap();
    let h = harness(s);
    let mut found = Vec::new();
    for c in &h.output().shapes {
        text_shapes(&c.shape, c.clip_rect, &mut found);
    }
    let rotated: Vec<_> = found.iter().filter(|(t, _)| t.galley.job.text == header).collect();
    assert_eq!(rotated.len(), 2, "both headers drawn as one text block each");
    for (t, clip) in rotated {
        assert!((t.angle.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "drawn at ±90°, got {}", t.angle);
        // Wrapped along the row height into a couple of long lines, not across the narrow column.
        assert!(t.galley.rows.len() <= 3, "{} lines", t.galley.rows.len());
        let bb = t.visual_bounding_rect();
        assert!(bb.height() > bb.width(), "upright, not flat: {bb:?}");
        assert!(clip.expand(1.0).contains_rect(bb), "inside its cell: {bb:?} not in {clip:?}");
        // Upright text never spills into the neighbouring columns: the clip is the cell itself.
        assert!(clip.width() < 70.0, "clip spans one 6-character column, got {clip:?}");
    }
}

#[test]
fn undo_after_backspace_restores_the_cleared_cell() {
    // #1855: Backspace clears the cell and opens an empty editor; committing it must not add a
    // second, empty undo step, so one Ctrl+Z brings the value back and leaves other edits alone.
    let mut h = harness(blank());
    text(&mut h, "3200");
    key(&mut h, Key::Enter, Modifiers::NONE);
    click_cell(&mut h, "C5");
    text(&mut h, "11750");
    key(&mut h, Key::Enter, Modifiers::NONE);
    click_cell(&mut h, "A1");
    key(&mut h, Key::Backspace, Modifiers::NONE);
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Empty);
    key(&mut h, Key::Z, Modifiers::COMMAND);
    assert_eq!(value(&h, "A1"), Value::Number(3200.0));
    assert_eq!(value(&h, "C5"), Value::Number(11750.0));
}

#[test]
fn header_footer_dialog_opens_on_the_saved_header_and_footer() {
    // #523: the dialog started from a fixed empty header and `&CPage &P of &N`, so OK replaced the sheet's text.
    let mut s = blank();
    s.execute("pageLayout.headerFooter", json!({"header": "&LLeft", "footer": "&CConfidential"})).unwrap();
    let mut h = harness(s);
    h.state_mut().open_dialog("headerFooter", json!({}));
    h.run_steps(2);
    let d = h.state_mut().dialog.as_mut().unwrap();
    assert_eq!(d.values["header"], json!("&LLeft"));
    assert_eq!(d.values["footer"], json!("&CConfidential"));
    d.values.insert("header".into(), json!("&CNew"));
    let vals = json!(d.values.clone());
    h.state_mut().run("pageLayout.headerFooter", vals).unwrap();
    let pr = &h.state().session.active().unwrap().wb.active().unwrap().print;
    assert_eq!((pr.header.as_str(), pr.footer.as_str()), ("&CNew", "&CConfidential"));
}

/// A two-table model: the Data View's table tabs, Table Properties and Create Relationship all
/// need two tables to exercise.
fn power_pivot_fixture(tag: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gridcraft-power-pivot-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("model.json");
    std::fs::write(
        &path,
        r#"{
            "tables": [
                {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Amount","data_type":"Double"}],
                 "rows":[["East",10],["West",20],["East",30]]},
                {"name":"Regions","columns":[{"name":"Region","data_type":"String"},{"name":"Manager","data_type":"String"}],
                 "rows":[["East","Ana"],["West","Bo"]]}
            ],
            "relationships": [],
            "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
        }"#,
    )
    .unwrap();
    (dir, path)
}

/// Clicks the last button with this label. The Power Pivot window is painted over the sheet's own
/// ribbon, so labels such as "File" and "Home" exist twice; the dialog's copy comes later. The
/// click goes through accesskit so a floating overlay whose position was laid out this frame is
/// still hit correctly.
fn click_button(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    let node = h.get_all_by_role_and_label(Role::Button, label).last().unwrap_or_else(|| panic!("no button labelled {label:?}"));
    node.click_accesskit();
    h.run_steps(2);
}

/// Clicks the first button with this label, e.g. the Home ribbon's "Diagram View" rather than the
/// footer's copy of the same switch.
fn click_first_button(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    let node = h.get_all_by_role_and_label(Role::Button, label).next().unwrap_or_else(|| panic!("no button labelled {label:?}"));
    node.click_accesskit();
    h.run_steps(2);
}

/// Whether any node carries this exact label, as `query_by_label` panics when the same label is
/// painted more than once (the dialog's footer and ribbon repeat Data View).
fn seen(h: &egui_kittest::Harness<'static, SheetApp>, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

/// Puts a formula in the Data View formula bar and commits it with Enter. Excel's layout has no
/// Evaluate button: the ✓ button and Enter both run `dataModel.evaluate` for the formula bar.
fn enter_formula(h: &mut egui_kittest::Harness<'static, SheetApp>, formula: &str) {
    h.state_mut().dialog.as_mut().expect("the Power Pivot dialog is open").values.insert("formula".into(), json!(formula));
    h.run_steps(1);
    let field = h.get_all_by_role(Role::TextInput).last().expect("the Data View formula bar has a text field").rect();
    h.get_all_by_role(Role::TextInput).find(|node| node.rect() == field).expect("formula field").focus();
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(3);
}

#[allow(clippy::cognitive_complexity)] // One end-to-end Power Pivot walkthrough: load, measure, relationship.
#[test]
fn power_pivot_window_loads_a_model_and_edits_measures_and_relationships() {
    let (dir, path) = power_pivot_fixture("ui");
    let mut h = harness(blank());
    h.state_mut().open_dialog("powerPivot", json!({}));
    h.run_steps(3);

    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("powerPivot"));
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["tab"], json!("Home"), "Excel's Power Pivot opens on Home");
        assert_eq!(d.values["view"], json!("Data"), "the Data View is the default canvas");
    }
    assert!(seen(&h, "Load a model and choose a table tab to see its rows."), "the Data View starts empty");
    assert!(seen(&h, "No table tabs"));
    assert!(seen(&h, "Add Measure"), "the Calculation Area is part of the Data View");
    for tab in ["File", "Home", "Design", "Advanced"] {
        assert!(h.query_all_by_role_and_label(Role::Button, tab).next().is_some(), "the {tab} tab is in the strip");
    }
    assert!(!seen(&h, "Diagram"), "Diagram View is not a tab");
    assert!(!seen(&h, "Pivot Preview"), "the old Pivot Preview surface is gone");
    assert!(!seen(&h, "Pivot Result"));

    // Load through Home › Existing Connections: the model path now lives in the overlay.
    click_button(&mut h, "Existing Connections");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["overlay"], json!("existingConnections"));
    h.state_mut().dialog.as_mut().unwrap().values.insert("path".into(), json!(path.to_string_lossy()));
    h.run_steps(1);
    click_button(&mut h, "Load");
    h.run_steps(3);
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!(""), "a successful load closes the overlay");
        assert_eq!(d.values["model"]["loaded"], json!(true));
        assert_eq!(d.values.get("error").and_then(|e| e.as_str()), Some(""));
        assert_eq!(d.values["tableIndex"], json!(0));
        assert_eq!(d.values["tableData"]["columns"], json!(["Region", "Amount"]));
        assert_eq!(d.values["tableData"]["rows"].as_array().map(Vec::len), Some(3), "the Data View fetched the first table's rows");
    }
    for label in ["Data View", "Sales", "Regions", "Region", "Amount", "Add Measure", "Total Sales"] {
        assert!(seen(&h, label), "{label} is visible in the loaded Data View");
    }
    let name_box = h.get_all_by_role(Role::ComboBox).last().expect("the Data View formula bar has a name box");
    assert_eq!(name_box.value().as_deref(), Some("[Region]"), "the formula bar's name box names the selected column");
    assert!(!seen(&h, "Pivot Preview"));

    // Each table tab runs a table evaluation (`Table` + `table: true`) behind the Data View grid.
    click_button(&mut h, "Regions");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["tableIndex"], json!(1));
        assert_eq!(d.values["tableData"]["columns"], json!(["Region", "Manager"]));
        assert_eq!(d.values["tableData"]["rows"].as_array().map(Vec::len), Some(2));
    }
    assert!(seen(&h, "Manager"), "the new table's headers replace the old ones");
    click_button(&mut h, "Sales");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["tableData"]["rows"].as_array().map(Vec::len), Some(3));

    // Scalar evaluation through the formula bar (Enter, not an Evaluate button).
    enter_formula(&mut h, "SUM(Sales[Amount])");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["resultKind"], json!("Scalar"));
        assert_eq!(d.values["resultText"], json!("60"));
        assert_eq!(d.result.as_ref().and_then(|r| r.get("value")).and_then(|v| v.as_f64()), Some(60.0));
    }
    assert!(seen(&h, "Scalar: 60"), "the formula bar renders the result");

    // Design and Advanced keep the Data View canvas below their ribbons.
    click_button(&mut h, "Design");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["tab"], json!("Design"));
        assert_eq!(d.values["view"], json!("Data"));
    }
    assert!(seen(&h, "Region") && seen(&h, "Add Measure"), "Design keeps the grid and Calculation Area");
    assert_eq!(h.get_all_by_role(Role::ComboBox).last().and_then(|node| node.value()).as_deref(), Some("[Region]"), "Design keeps the formula bar");
    assert!(!seen(&h, "Diagram"));
    click_button(&mut h, "Advanced");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["tab"], json!("Advanced"));
    assert!(seen(&h, "Region") && seen(&h, "Add Measure"), "Advanced keeps the Data View canvas");
    assert_eq!(h.get_all_by_role(Role::ComboBox).last().and_then(|node| node.value()).as_deref(), Some("[Region]"), "Advanced keeps the formula bar");
    assert!(seen(&h, "Show Implicit Measures"), "the Advanced ribbon is active");

    // File is a backstage inside the window: its entries replace the canvas.
    click_button(&mut h, "File");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["tab"], json!("File"));
        assert_eq!(d.tab, "File", "the generic dialog tab mirrors the in-window strip");
    }
    for entry in ["Open Model…", "Existing Connections", "Switch to Advanced Mode", "Close", "Exit"] {
        assert!(seen(&h, entry), "File lists {entry}");
    }
    assert!(!seen(&h, "Add Measure"), "the File menu replaces the Data View canvas");
    click_button(&mut h, "Switch to Advanced Mode");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["tab"], json!("Advanced"));
    assert!(seen(&h, "Add Measure"), "leaving File restores the canvas");

    // Home › Diagram View is a view, not a tab; the footer switches back.
    click_button(&mut h, "Home");
    click_first_button(&mut h, "Diagram View");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["tab"], json!("Home"));
        assert_eq!(d.values["view"], json!("Diagram"));
        assert!(!seen(&h, "Add Measure"), "the Diagram View replaces the Data View canvas");
    }
    assert!(seen(&h, "Model Diagram"));
    assert!(h.query_all_by_role_and_label(Role::Label, "Sales").next().is_some(), "the diagram draws a card for Sales");
    assert!(h.query_all_by_role_and_label(Role::Label, "Regions").next().is_some(), "the diagram draws a card for Regions");
    assert!(seen(&h, "Manager"), "the cards list their columns");
    assert!(seen(&h, "This model exposes no relationships; showing table cards only."));
    click_button(&mut h, "Data View");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["view"], json!("Data"));
        assert!(!seen(&h, "Model Diagram"));
        assert!(seen(&h, "Add Measure"));
    }

    // Add Measure writes through dataModel.addMeasure; the Calculation Area lists the result.
    click_button(&mut h, "Add Measure");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!("addMeasure"));
        assert_eq!(d.values["measureDialogMode"], json!("Add"));
    }
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("newMeasureName".into(), json!("Double Sales"));
        d.values.insert("newMeasureTable".into(), json!("Sales"));
        d.values.insert("newMeasureFormula".into(), json!("SUM(Sales[Amount]) * 2"));
        d.values.insert("newMeasureFormat".into(), json!("Currency"));
    }
    h.run_steps(1);
    click_button(&mut h, "Add");
    h.run_steps(2);
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!(""), "Add closes on success");
        let measures = d.values["model"]["measures"].as_array().expect("status carries the measure list");
        assert_eq!(measures.len(), 2);
        assert_eq!(measures[1]["name"], json!("Double Sales"));
        assert_eq!(measures[1]["formula"], json!("SUM(Sales[Amount]) * 2"));
        assert_eq!(measures[1]["format"], json!("Currency"));
        assert_eq!(d.values["measureIndex"], json!(1), "the new measure is selected in the Calculation Area");
        assert_eq!(d.values["formula"], json!("[Double Sales]"));
    }
    assert!(seen(&h, "Double Sales") && seen(&h, "Currency"), "the Calculation Area lists the new measure");
    enter_formula(&mut h, "[Double Sales]");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["resultKind"], json!("Measure"));
        assert_eq!(d.values["resultText"], json!("$120.00"), "the measure's Currency format applies to evaluate output");
    }

    // Update Measure edits the same engine measure and clears a blank format.
    click_button(&mut h, "Update");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!("addMeasure"));
        assert_eq!(d.values["measureDialogMode"], json!("Update"));
    }
    assert!(seen(&h, "Update Measure"), "the overlay title says Update Measure");
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("newMeasureFormula".into(), json!("SUM(Sales[Amount]) * 3"));
        d.values.insert("newMeasureFormat".into(), json!(""));
    }
    h.run_steps(1);
    click_button(&mut h, "Update");
    h.run_steps(2);
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!(""));
        let measures = d.values["model"]["measures"].as_array().unwrap();
        assert_eq!(measures[1]["formula"], json!("SUM(Sales[Amount]) * 3"));
        assert!(measures[1]["format"].is_null(), "a blank format clears the measure format");
    }
    enter_formula(&mut h, "[Double Sales]");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["resultText"], json!("180"));

    // Delete Measure removes it through dataModel.deleteMeasure.
    click_button(&mut h, "Delete");
    h.run_steps(2);
    {
        let d = h.state().dialog.as_ref().unwrap();
        let measures = d.values["model"]["measures"].as_array().unwrap();
        assert_eq!(measures.len(), 1, "Delete removed the measure");
        assert_eq!(measures[0]["name"], json!("Total Sales"));
        assert_eq!(d.values["measureIndex"], json!(-1));
        assert_eq!(d.values["formula"], json!(""));
    }
    assert!(!seen(&h, "Double Sales"), "the Calculation Area dropped the deleted measure");

    // Create Relationship writes through dataModel.addRelationship; Manage Relationships lists it.
    click_button(&mut h, "Design");
    click_button(&mut h, "Create Relationship");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!("createRelationship"));
        assert_eq!(d.values["relFromTable"], json!("Sales"), "the dialog defaults to the first two tables");
        assert_eq!(d.values["relToTable"], json!("Regions"));
    }
    {
        let d = h.state_mut().dialog.as_mut().unwrap();
        d.values.insert("relFromTable".into(), json!("Sales"));
        d.values.insert("relFromColumn".into(), json!("Region"));
        d.values.insert("relToTable".into(), json!("Regions"));
        d.values.insert("relToColumn".into(), json!("Region"));
        d.values.insert("relBidirectional".into(), json!(true));
    }
    h.run_steps(1);
    click_button(&mut h, "Create");
    h.run_steps(2);
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!(""), "Create closes on success");
        let relationships = d.values["model"]["relationships"].as_array().unwrap();
        assert_eq!(relationships.len(), 1);
        assert_eq!(relationships[0]["from_table"], json!("Sales"));
        assert_eq!(relationships[0]["from_column"], json!("Region"));
        assert_eq!(relationships[0]["to_table"], json!("Regions"));
        assert_eq!(relationships[0]["to_column"], json!("Region"));
        assert_eq!(relationships[0]["bidirectional"], json!(true));
    }

    click_button(&mut h, "Manage Relationships");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["overlay"], json!("manageRelationships"));
    for label in ["Relationships", "1 defined", "Sales[Region]", "Regions[Region]", "Both"] {
        assert!(seen(&h, label), "Manage Relationships lists {label}");
    }
    click_button(&mut h, "Sales[Region]");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["relIndex"], json!(0), "selecting a row enables Delete");
    click_button(&mut h, "Delete");
    h.run_steps(2);
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values["overlay"], json!("manageRelationships"), "Manage Relationships stays open after Delete");
        assert_eq!(d.values["model"]["relationships"].as_array().map(Vec::len), Some(0));
    }
    assert!(seen(&h, "No relationships are exposed by this model."), "the list is empty after Delete");
    click_button(&mut h, "Close");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["overlay"], json!(""));

    // Table Properties: the table name, row/column counts, and each column's model type.
    click_button(&mut h, "Table Properties");
    assert_eq!(h.state().dialog.as_ref().unwrap().values["overlay"], json!("tableProperties"));
    for label in ["Table name", "Sales", "Rows", "3", "Columns", "2", "Column", "Type", "String", "Double"] {
        assert!(seen(&h, label), "Table Properties shows {label}");
    }
    click_button(&mut h, "Close");

    // Engine errors replace the result and leave the dialog open.
    click_button(&mut h, "Home");
    enter_formula(&mut h, "SUM(Missing[Amount])");
    let error = {
        let d = h.state().dialog.as_ref().unwrap();
        assert!(d.result.is_none());
        assert_eq!(d.name(), "powerPivot", "the dialog stays open after an engine error");
        d.values.get("error").and_then(|e| e.as_str()).unwrap_or("").to_string()
    };
    assert!(!error.is_empty(), "the engine's error is recorded");
    assert!(seen(&h, &error), "the Data View renders the error");
    assert!(!seen(&h, "Scalar: 60"), "a failed evaluation clears the previous result");

    h.state_mut().dialog = None;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn power_pivot_function_library_inserts_parseable_templates() {
    let (dir, path) = power_pivot_fixture("functions");
    let mut session = blank();
    session.run("dataModel.load", json!({"path": path.to_string_lossy()})).unwrap();
    let mut h = harness(session);
    h.state_mut().open_dialog("powerPivot", json!({}));
    h.run_steps(3);

    // The Function Library moved to Design › Insert Function; its search box filters the templates.
    click_button(&mut h, "Design");
    click_button(&mut h, "Insert Function");
    assert!(seen(&h, "Function Picker"), "the Function Library opens");

    let scalar_cases = [
        ("SUM", "SUM(Sales[Amount])", Some(60.0)),
        ("SUMX", "SUMX(Sales, Sales[Amount])", Some(60.0)),
        ("ROUND", "ROUND(1.234, 2)", None),
        ("CALCULATE", "CALCULATE([Total Sales], Sales[Region] = \"\")", None),
        ("DATE", "DATE(2024, 1, 1)", None),
    ];
    for (name, expected, value) in scalar_cases {
        if !seen(&h, "Function Picker") {
            click_button(&mut h, "Insert Function");
        }
        h.state_mut().dialog.as_mut().unwrap().values.insert("fnSearch".into(), json!(name));
        h.run_steps(1);
        click_button(&mut h, name);
        {
            let d = h.state().dialog.as_ref().unwrap();
            assert_eq!(d.values.get("formula").and_then(|f| f.as_str()), Some(expected), "{name} inserts its template");
            assert_eq!(d.values.get("showFunctions").and_then(|f| f.as_bool()), Some(false), "choosing a function closes the picker");
        }
        enter_formula(&mut h, expected);
        {
            let d = h.state().dialog.as_ref().unwrap();
            let error = d.values.get("error").and_then(|e| e.as_str()).unwrap_or("");
            assert!(error.is_empty(), "{name} template evaluates: {error}");
            if let Some(value) = value {
                assert_eq!(d.result.as_ref().and_then(|r| r.get("value")).and_then(|v| v.as_f64()), Some(value));
            }
        }
    }

    // IF evaluates to a value rather than a number.
    click_button(&mut h, "Insert Function");
    h.state_mut().dialog.as_mut().unwrap().values.insert("fnSearch".into(), json!("IF"));
    h.run_steps(1);
    click_button(&mut h, "IF");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values.get("formula").and_then(|f| f.as_str()), Some("IF(TRUE(), 1, 0)"));
    }
    enter_formula(&mut h, "IF(TRUE(), 1, 0)");
    {
        let d = h.state().dialog.as_ref().unwrap();
        assert_eq!(d.values.get("error").and_then(|e| e.as_str()), Some(""));
        assert_eq!(d.result.as_ref().and_then(|r| r.get("value")).and_then(|v| v.as_i64()), Some(1));
    }

    // Table templates: the Excel layout has no table switch in the formula bar; the Data View
    // loads table expressions through `dataModel.evaluate` with `table: true`, so verify the
    // inserted templates through that same engine path.
    let table_cases = [
        ("FILTER", "FILTER(Sales, Sales[Amount] > 0)"),
        ("TOPN", "TOPN(10, Sales, Sales[Amount], 0)"),
        ("ADDCOLUMNS", "ADDCOLUMNS(Sales, \"Value\", Sales[Amount])"),
        ("DATATABLE", "DATATABLE(\"Value\", INTEGER, {{1}, {2}})"),
    ];
    for (name, expected) in table_cases {
        click_button(&mut h, "Insert Function");
        h.state_mut().dialog.as_mut().unwrap().values.insert("fnSearch".into(), json!(name));
        h.run_steps(1);
        click_button(&mut h, name);
        assert_eq!(h.state().dialog.as_ref().unwrap().values.get("formula").and_then(|f| f.as_str()), Some(expected), "{name} inserts its template");
        let result = h
            .state_mut()
            .session
            .run("dataModel.evaluate", json!({"formula": expected, "table": true}))
            .unwrap_or_else(|error| panic!("{name} template evaluates as a table: {error}"));
        assert!(result.get("columns").and_then(|c| c.as_array()).is_some(), "{name} returns a table");
    }

    h.state_mut().dialog = None;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn power_pivot_open_model_browses_a_json_file() {
    let (dir, path) = power_pivot_fixture("browse");
    let bytes = std::fs::read(&path).unwrap();
    type Requests = std::sync::Arc<std::sync::Mutex<Vec<gridcraft_ui_egui::Inbox>>>;
    let requests: Requests = std::sync::Arc::default();
    let send = requests.clone();
    let mut h = crate::support::services_harness(
        gridcraft_ui_egui::Services {
            // The browser host: no filesystem, files arrive through a picker-owned inbox.
            open_async: Some(Box::new(|| {})),
            pick_model_async: Some(Box::new(move |inbox| send.lock().unwrap().push(inbox))),
            ..Default::default()
        },
        4,
    );
    h.state_mut().open_dialog("powerPivot", json!({}));
    h.run_steps(3);
    click_button(&mut h, "File");
    click_button(&mut h, "Open Model…");
    h.run_steps(2);
    click_button(&mut h, "Browse…");
    h.run_steps(2);
    assert_eq!(requests.lock().unwrap().len(), 1, "Browse… starts the browser picker");

    // Canceling the dialog drops its inbox: the late pick cannot load a model later.
    click_button(&mut h, "Cancel");
    h.run_steps(2);
    let canceled = requests.lock().unwrap().pop().unwrap();
    click_button(&mut h, "Open Model…");
    h.run_steps(2);
    canceled.lock().unwrap().push(("late.json".into(), bytes.clone()));
    h.run_steps(3);
    assert_eq!(h.state_mut().session.run("dataModel.status", json!({})).unwrap()["loaded"], json!(false));

    // A picked file reaches dataModel.load as JSON text (wasm has no path to read).
    click_button(&mut h, "Browse…");
    h.run_steps(2);
    requests.lock().unwrap().pop().unwrap().lock().unwrap().push(("model.json".into(), bytes));
    h.run_steps(3);
    let status = h.state_mut().session.run("dataModel.status", json!({})).unwrap();
    assert_eq!(status["loaded"], json!(true));
    assert_eq!(status["tables"][0]["name"], json!("Sales"));
    let d = h.state().dialog.as_ref().unwrap();
    assert_eq!(d.values["overlay"], json!(""), "a loaded model closes the overlay");
    assert_eq!(d.values["modelName"], json!("model.json"));
    assert_eq!(d.values["tableData"]["columns"], json!(["Region", "Amount"]), "the Data View fetched the loaded table");
    h.state_mut().dialog = None;
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn power_pivot_open_model_browses_a_path_on_native() {
    let (dir, path) = power_pivot_fixture("browse-native");
    let chosen = path.to_string_lossy().to_string();
    let mut h = crate::support::services_harness(
        gridcraft_ui_egui::Services { pick_open: Some(Box::new(move || Some(chosen.clone()))), ..Default::default() },
        4,
    );
    h.state_mut().open_dialog("powerPivot", json!({}));
    h.run_steps(3);
    click_button(&mut h, "File");
    click_button(&mut h, "Open Model…");
    h.run_steps(2);
    click_button(&mut h, "Browse…");
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["path"], json!(path.to_string_lossy()));
    assert!(!h.state().dialog.as_ref().unwrap().values.contains_key("modelJson"), "the native picker fills the path, not JSON text");
    click_button(&mut h, "Load");
    h.run_steps(3);
    assert_eq!(h.state_mut().session.run("dataModel.status", json!({})).unwrap()["loaded"], json!(true));
    h.state_mut().dialog = None;
    let _ = std::fs::remove_dir_all(&dir);
}
