//! Excel keyboard shortcuts (crate::keymap) dispatched through real egui key events.

use egui::{Event, Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::BorderStyle;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::{Geo, SelectionMode};
use gridcraft_ui_egui::keymap::{Shortcut, lookup};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    crate::support::harness(session, 4)
}

fn book(selected: &str) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1.5, 2], [3, 4], [5, 6]]})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "=A1*2"})).unwrap();
    s.execute("selection.set", json!({"range": selected})).unwrap();
    s
}

/// A key press as a US keyboard reports it (the physical key is the unshifted one).
fn key(h: &mut Harness, k: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn style(h: &Harness, a1: &str) -> gridcraft_engine::model::Style {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(a1).unwrap())).clone()
}

/// Ctrl on Windows and Linux sets both `ctrl` and `command`.
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..CTRL };

#[test]
fn ctrl_shift_symbols_apply_number_formats_and_borders() {
    let mut h = harness(book("A1:B2"));
    key(&mut h, Key::Num4, CTRL_SHIFT);
    assert_eq!(style(&h, "B2").num_fmt.as_str(), "$#,##0.00_);($#,##0.00)");
    key(&mut h, Key::Num5, CTRL_SHIFT);
    assert_eq!(style(&h, "A1").num_fmt.as_str(), "0%");
    key(&mut h, Key::Backtick, CTRL_SHIFT);
    assert_eq!(style(&h, "A1").num_fmt.as_str(), "General");
    key(&mut h, Key::Num7, CTRL_SHIFT);
    assert!(style(&h, "A1").borders.top.style != BorderStyle::None && style(&h, "B2").borders.right.style != BorderStyle::None);
    key(&mut h, Key::Minus, CTRL_SHIFT);
    assert!(style(&h, "A1").borders.top.style == BorderStyle::None);
    // Ctrl+Shift+End extends from the anchor to the last used cell.
    key(&mut h, Key::End, CTRL_SHIFT);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "A1:C3");
}

#[test]
fn copy_formula_from_above_and_function_keys() {
    let mut h = harness(book("C2"));
    key(&mut h, Key::Quote, CTRL);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.clone()).as_deref(), Some("=A1*2"));
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().editor.is_none());
    let sheets = h.state().session.active().unwrap().wb.sheets.len();
    key(&mut h, Key::F11, Modifiers::SHIFT);
    assert_eq!(h.state().session.active().unwrap().wb.sheets.len(), sheets + 1);
    // Back on the data: Alt+F1 charts the current region, Ctrl+Alt+= zooms in.
    h.state_mut().session.run("sheet.previous", json!({})).unwrap();
    h.state_mut().session.run("selection.set", json!({"range": "A1:B3"})).unwrap();
    key(&mut h, Key::F1, Modifiers::ALT);
    assert_eq!(h.state().session.active().unwrap().wb.sheets[0].charts.len(), 1);
    key(&mut h, Key::Equals, Modifiers { alt: true, ..CTRL });
    assert_eq!(h.state().session.active().unwrap().wb.sheets[0].zoom, 110);
}

fn selection_a1(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.a1()
}

fn active_a1(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.active.a1()
}

fn mode(h: &Harness) -> SelectionMode {
    h.state().grid.selection_mode
}

#[test]
fn f8_extend_selection_extends_from_the_anchor_until_turned_off() {
    let mut h = harness(book("A1"));
    assert!(h.query_by_label("Extend Selection").is_none());
    key(&mut h, Key::F8, Modifiers::NONE);
    assert_eq!(mode(&h), SelectionMode::Extend);
    assert!(h.query_by_label("Extend Selection").is_some(), "the status bar shows the mode");
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "A1:B2");
    assert_eq!(active_a1(&h), "A1", "the active cell stays on the anchor");
    // Ctrl+arrow extends to the edge of the data, as Ctrl+Shift+arrow does.
    key(&mut h, Key::ArrowDown, CTRL);
    assert_eq!(selection_a1(&h), "A1:B3");
    // F8 again ends the mode: arrows move again.
    key(&mut h, Key::F8, Modifiers::NONE);
    assert_eq!(mode(&h), SelectionMode::Off);
    assert!(h.query_by_label("Extend Selection").is_none());
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "B1");
    // Esc ends it too, and keeps the extended selection.
    key(&mut h, Key::F8, Modifiers::NONE);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert_eq!(mode(&h), SelectionMode::Off);
    assert_eq!(selection_a1(&h), "B1:C1");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "B2");
}

#[test]
fn shift_f8_add_to_selection_builds_areas_with_arrow_keys() {
    let mut h = harness(book("A1:A2"));
    key(&mut h, Key::F8, Modifiers::SHIFT);
    assert_eq!(mode(&h), SelectionMode::Add);
    assert!(h.query_by_label("Add or Remove Selection").is_some(), "the status bar shows the mode");
    assert!(h.query_by_label("Extend Selection").is_none());
    // Arrows move the active cell and keep A1:A2; the next arrow moves the new cell again.
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "A1:A2,B1");
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "A1:A2,C1");
    assert_eq!(active_a1(&h), "C1");
    // Shift+arrow grows that area from it and keeps it.
    key(&mut h, Key::ArrowRight, Modifiers::SHIFT);
    assert_eq!(selection_a1(&h), "A1:A2,C1:D1");
    assert_eq!(active_a1(&h), "C1");
    // The next arrow starts a third area.
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "A1:A2,C1:D1,C3");
    // F8 switches to Extend mode, and Shift+F8 twice ends Add mode.
    key(&mut h, Key::F8, Modifiers::NONE);
    assert_eq!(mode(&h), SelectionMode::Extend);
    key(&mut h, Key::F8, Modifiers::SHIFT);
    assert_eq!(mode(&h), SelectionMode::Add);
    key(&mut h, Key::F8, Modifiers::SHIFT);
    assert_eq!(mode(&h), SelectionMode::Off);
    assert!(h.query_by_label("Add or Remove Selection").is_none());
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "D3", "with the mode off an arrow drops the areas");
}

#[test]
fn editing_a_cell_ends_the_selection_mode() {
    let mut h = harness(book("A1"));
    key(&mut h, Key::F8, Modifiers::NONE);
    key(&mut h, Key::F2, Modifiers::NONE);
    assert!(h.state().editor.is_some());
    assert_eq!(mode(&h), SelectionMode::Off);
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::F8, Modifiers::SHIFT);
    h.input_mut().events.push(Event::Text("7".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some(), "typing starts an edit");
    assert_eq!(mode(&h), SelectionMode::Off);
}

fn click(h: &mut Harness, a1: &str) {
    let at = {
        let app = h.state();
        let sheet = app.session.active().unwrap().wb.active().unwrap();
        Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cell_rect(sheet, CellRef::parse(a1).unwrap()).center()
    };
    h.input_mut().events.push(Event::PointerMoved(at));
    h.run_steps(1);
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(1);
}

#[test]
fn clicks_extend_or_add_in_the_selection_modes() {
    let mut h = harness(book("B2"));
    key(&mut h, Key::F8, Modifiers::NONE);
    click(&mut h, "D4");
    assert_eq!(selection_a1(&h), "B2:D4", "in Extend mode a click extends from the anchor");
    assert_eq!(mode(&h), SelectionMode::Extend, "the mode stays on");
    key(&mut h, Key::F8, Modifiers::SHIFT);
    click(&mut h, "F6");
    assert_eq!(selection_a1(&h), "B2:D4,F6", "in Add mode a click adds an area");
    // (Not a double-click: that would edit the cell.)
    h.state_mut().grid.last_click = None;
    click(&mut h, "F6");
    assert_eq!(selection_a1(&h), "B2:D4", "and a click on a selected area removes it");
    // An arrow after a click keeps the clicked areas.
    click(&mut h, "H8");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "B2:D4,H8,H9");
    // Moving back onto the kept H8 and off again keeps it.
    key(&mut h, Key::ArrowUp, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "B2:D4,H8");
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "B2:D4,H8,H9");
}

/// A point in the column header band, above `col`'s cells.
fn column_header_point(h: &Harness, col: &str) -> egui::Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let x = geo.cell_rect(sheet, CellRef::parse(&format!("{col}1")).unwrap()).center().x;
    egui::pos2(x, geo.head_top() + 4.0)
}

fn drag(h: &mut Harness, from: egui::Pos2, to: egui::Pos2) {
    h.input_mut().events.push(Event::PointerMoved(from));
    h.run_steps(1);
    h.input_mut().events.push(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.input_mut().events.push(Event::PointerMoved(to));
    h.run_steps(2);
    h.input_mut().events.push(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
}

#[test]
fn add_mode_header_drag_keeps_the_earlier_areas() {
    let mut h = harness(book("B2"));
    key(&mut h, Key::F8, Modifiers::SHIFT);
    click(&mut h, "F6");
    assert_eq!(selection_a1(&h), "B2,F6");
    h.state_mut().grid.last_click = None;
    let (from, to) = (column_header_point(&h, "H"), column_header_point(&h, "J"));
    drag(&mut h, from, to);
    assert_eq!(selection_a1(&h), "B2,F6,H:J", "a header drag in Add mode adds its columns and keeps the areas already selected");
}

#[test]
fn add_mode_header_drag_replaces_only_the_provisional_area() {
    let mut h = harness(book("B2"));
    key(&mut h, Key::F8, Modifiers::SHIFT);
    key(&mut h, Key::ArrowRight, Modifiers::NONE);
    assert_eq!(selection_a1(&h), "B2,C2");
    h.state_mut().grid.last_click = None;
    let (from, to) = (column_header_point(&h, "H"), column_header_point(&h, "J"));
    drag(&mut h, from, to);
    assert_eq!(selection_a1(&h), "B2,H:J", "the header drag replaces the provisional area while retaining earlier areas");
}

#[test]
fn add_mode_shift_home_and_end_keep_the_earlier_areas() {
    let mut home = harness(book("B2"));
    key(&mut home, Key::F8, Modifiers::SHIFT);
    click(&mut home, "F6");
    key(&mut home, Key::Home, Modifiers::SHIFT);
    assert_eq!(selection_a1(&home), "B2,A6:F6", "Shift+Home extends the active area and keeps the earlier area");

    let mut end = harness(book("B2"));
    key(&mut end, Key::F8, Modifiers::SHIFT);
    click(&mut end, "F6");
    key(&mut end, Key::End, CTRL_SHIFT);
    assert_eq!(selection_a1(&end), "B2,C3:F6", "Ctrl+Shift+End extends the active area and keeps the earlier area");
}

#[test]
fn selection_mode_command_sets_and_reports_without_a_dialog() {
    let mut h = harness(book("A1"));
    assert_eq!(h.state_mut().run("ui.selectionMode", json!({})), Ok(json!({"mode": "off"})));
    assert_eq!(h.state_mut().run("ui.selectionMode", json!({"mode": "extend"})), Ok(json!({"mode": "extend"})));
    h.run_steps(2);
    assert!(h.query_by_label("Extend Selection").is_some());
    assert_eq!(h.state_mut().run("ui.selectionMode", json!({"mode": "add"})), Ok(json!({"mode": "add"})));
    assert!(h.state_mut().run("ui.selectionMode", json!({"mode": "sideways"})).is_err());
    assert!(h.state_mut().run("ui.selectionMode", json!({"mode": 3})).is_err());
    assert_eq!(mode(&h), SelectionMode::Add, "a bad mode changes nothing");
    assert_eq!(h.state_mut().run("ui.selectionMode", json!({"mode": "off"})), Ok(json!({"mode": "off"})));
    assert!(h.state().dialog.is_none());
}

#[test]
fn windows_and_mac_variants() {
    let cmd = CTRL;
    let mac_cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    let ctrl = Modifiers::CTRL;
    let run = |id: &'static str| Some(Shortcut::Run(id, json!({})));
    assert_eq!(lookup(Key::L, Some(Key::L), cmd, false), run("insert.table"));
    assert_eq!(lookup(Key::L, Some(Key::L), mac_cmd, true), run("home.alignLeft"));
    assert_eq!(lookup(Key::E, Some(Key::E), mac_cmd, true), run("home.alignCenter"));
    assert_eq!(lookup(Key::E, Some(Key::E), ctrl, true), run("edit.flashFill"));
    assert_eq!(lookup(Key::Equals, Some(Key::Equals), Modifiers::ALT, false), run("formulas.autoSum"));
    // The shifted character doesn't matter: Ctrl+Shift+1 is the Number format wherever `!` is.
    assert_eq!(
        lookup(Key::Exclamationmark, Some(Key::Num1), CTRL_SHIFT, false),
        Some(Shortcut::Run("home.numberFormat", json!({"code": "#,##0.00"})))
    );
    // Navigation keys go by what they are, not where: the numpad's arrows and Page Down (Num
    // Lock off) report Num6 / Num3 as their physical keys.
    let alt_shift = Modifiers { alt: true, shift: true, ..Modifiers::NONE };
    assert_eq!(lookup(Key::ArrowRight, Some(Key::Num6), alt_shift, false), run("data.group"));
    assert_eq!(lookup(Key::ArrowLeft, Some(Key::Num4), alt_shift, false), run("data.ungroup"));
    assert_eq!(lookup(Key::PageDown, Some(Key::Num3), Modifiers::ALT, false), Some(Shortcut::ScreenRight));
    assert_eq!(lookup(Key::PageDown, Some(Key::Num3), CTRL_SHIFT, false), None);
    // Ctrl+. is Next Corner of Selection, with Control on a Mac too (#1442); Cmd+Shift+. stays
    // Increase Font Size.
    assert_eq!(lookup(Key::Period, Some(Key::Period), cmd, false), run("selection.nextCorner"));
    assert_eq!(lookup(Key::Period, Some(Key::Period), ctrl, true), run("selection.nextCorner"));
    assert_eq!(lookup(Key::Period, Some(Key::Period), Modifiers { shift: true, ..mac_cmd }, true), run("home.increaseFontSize"));
    // Plain keys stay with the grid (typing, navigation).
    assert_eq!(lookup(Key::A, Some(Key::A), Modifiers::NONE, false), None);
    assert_eq!(lookup(Key::F2, Some(Key::F2), Modifiers::NONE, false), None);
}

#[test]
fn space_selection_shortcuts_do_not_start_an_edit() {
    let mut h = harness(book("B2"));
    // Shift+Space: the key press, then the space the OS types with it.
    let shift = Modifiers::SHIFT;
    h.input_mut().events.push(Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: true, repeat: false, modifiers: shift });
    h.input_mut().events.push(Event::Text(" ".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_none(), "Shift+Space started an edit");
    assert_eq!(h.state().session.active().unwrap().selection.ranges[0].a1(), "2:2");
    h.state_mut().session.run("selection.set", json!({"range": "B2"})).unwrap();
    // Ctrl+Space selects the column, also without starting an edit.
    h.input_mut().events.push(Event::Key { key: Key::Space, physical_key: Some(Key::Space), pressed: true, repeat: false, modifiers: CTRL });
    h.input_mut().events.push(Event::Text(" ".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_none());
    assert_eq!(h.state().session.active().unwrap().selection.ranges[0].a1(), "B:B");
    // Plain typing still edits.
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
}

#[test]
fn space_shortcuts_per_platform() {
    let run = |id: &'static str| Some(Shortcut::Run(id, json!({})));
    let mac_cmd = Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE };
    let space = |m, mac| lookup(Key::Space, Some(Key::Space), m, mac);
    assert_eq!(space(Modifiers::SHIFT, false), run("selection.row"));
    assert_eq!(space(Modifiers::SHIFT, true), run("selection.row"));
    assert_eq!(space(CTRL, false), run("selection.column"));
    assert_eq!(space(Modifiers::CTRL, true), run("selection.column"));
    // Cmd+Space is Spotlight on macOS.
    assert_eq!(space(mac_cmd, true), None);
}

#[test]
/// Alt+Down opens the active header's filter menu in LTR and RTL without editing the workbook; typing goes to the search box and Esc dismisses it.
fn alt_down_opens_autofilter_headers_without_changing_the_workbook() {
    for rtl in [false, true] {
        for cell in ["A1", "B1"] {
            let mut session = book(cell);
            session.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
            session.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
            let mut h = harness(session);
            let before = h.state().session.active().unwrap().wb.clone();
            key(&mut h, Key::ArrowDown, Modifiers::ALT);
            let (column, _row, at) = h.state().grid.filter_menu.expect("Alt+Down must open the filter header menu");
            let d = h.state().session.active().unwrap();
            let active = CellRef::parse(cell).unwrap();
            assert_eq!(column, active.col);
            assert_eq!(d.selection.active, active);
            assert_eq!(d.wb, before);
            let sh = d.wb.active().unwrap();
            let geo = gridcraft_ui_egui::grid::Geo::new(sh, h.state().grid.rect.unwrap(), h.state().view().scroll);
            assert_eq!(at, geo.mirror_pos(gridcraft_ui_egui::grid::filter_button_rect(&geo, sh, active).center()));
            // Opening focuses the existing search box; typing must not edit the header.
            h.input_mut().events.push(Event::Text("1".into()));
            h.run_steps(2);
            assert_eq!(h.state().grid.filter_search, "1");
            assert!(h.state().editor.is_none());
            assert_eq!(h.state().session.active().unwrap().wb, before);
            key(&mut h, Key::Escape, Modifiers::NONE);
            assert!(h.state().grid.filter_menu.is_none());
        }
    }
}

#[test]
fn alt_down_keeps_validation_priority_and_plain_down_navigation() {
    let mut session = book("A1");
    session.execute("data.filter", json!({"range": "A1:B3"})).unwrap();
    session.execute("data.validation", json!({"range": "A1", "type": "list", "formula1": "\"First,Second\""})).unwrap();
    let mut h = harness(session);
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    assert_eq!(h.state().grid.list_picker, Some(CellRef::parse("A1").unwrap()));
    assert!(h.state().grid.filter_menu.is_none());
    key(&mut h, Key::Escape, Modifiers::NONE);
    key(&mut h, Key::ArrowDown, Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A2");
    assert!(h.state().grid.filter_menu.is_none());
    key(&mut h, Key::ArrowDown, Modifiers::ALT);
    assert!(h.state().grid.filter_menu.is_none(), "a data cell has no filter button");
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "A2");
}

#[test]
fn alt_down_only_opens_enabled_table_filter_buttons() {
    for enabled in [false, true] {
        let mut session = book("B1");
        session.execute("insert.table", json!({"range": "A1:B3"})).unwrap();
        if !enabled {
            session.execute("table.headerRow", json!({"on": false})).unwrap();
        }
        session.execute("selection.set", json!({"cell": "B1"})).unwrap();
        let mut h = harness(session);
        key(&mut h, Key::ArrowDown, Modifiers::ALT);
        assert_eq!(h.state().grid.filter_menu.map(|(column, _, _)| column), enabled.then_some(1));
        assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "B1");
    }
}

/// A key press as a non-US layout reports it: `typed` is the character the layout types (egui's
/// logical key), `at` the key's position named after a US keyboard.
fn key_on_layout(h: &mut Harness, typed: Key, at: Key, m: Modifiers) {
    h.input_mut().events.push(Event::Key { key: typed, physical_key: Some(at), pressed: true, repeat: false, modifiers: m });
    h.input_mut().events.push(Event::Key { key: typed, physical_key: Some(at), pressed: false, repeat: false, modifiers: m });
    h.run_steps(2);
}

fn editor_text(h: &Harness) -> Option<String> {
    h.state().editor.as_ref().map(|e| e.text.clone())
}

fn today(h: &Harness) -> String {
    let d = h.state().session.active().unwrap();
    let now = gridcraft_engine::core::date::serial_in_system(d.wb.date_system, gridcraft_engine::calc::now_serial());
    gridcraft_engine::display::format(&gridcraft_engine::core::Value::Number(now.floor()), "m/d/yyyy", &d.wb).text
}

#[test]
fn symbol_shortcuts_follow_the_character_the_layout_types() {
    // Croatian and German keyboards type `;` with Shift+, and `:` with Shift+. (US positions).
    // `+` is unshifted, on the US `=` key (Croatian) or `]` key (German); `'` is the US `-` key
    // (Croatian) or Shift+# on the US `\` key (German); Shift+0 types `=` on both.
    for (layout, plus_at, quote_at, quote_shift) in [("hr", Key::Equals, Key::Minus, false), ("de", Key::CloseBracket, Key::Backslash, true)] {
        let mut h = harness(book("C2"));
        let size = style(&h, "C2").font.size;
        // Ctrl+; (Ctrl+Shift+,): today's date, not Decrease Font Size.
        key_on_layout(&mut h, Key::Semicolon, Key::Comma, CTRL_SHIFT);
        assert_eq!(editor_text(&h), Some(today(&h)), "{layout}: Ctrl+;");
        assert_eq!(style(&h, "C2").font.size, size, "{layout}: Ctrl+; changed the font size");
        key(&mut h, Key::Escape, Modifiers::NONE);
        // Ctrl+: (Ctrl+Shift+.): the time, not Increase Font Size.
        key_on_layout(&mut h, Key::Colon, Key::Period, CTRL_SHIFT);
        let time = editor_text(&h).unwrap_or_default();
        assert!(time.contains(':') && (time.ends_with("AM") || time.ends_with("PM")), "{layout}: Ctrl+: gave {time:?}");
        assert_eq!(style(&h, "C2").font.size, size, "{layout}: Ctrl+: changed the font size");
        key(&mut h, Key::Escape, Modifiers::NONE);
        // Ctrl+': the formula of the cell above.
        key_on_layout(&mut h, Key::Quote, quote_at, if quote_shift { CTRL_SHIFT } else { CTRL });
        assert_eq!(editor_text(&h).as_deref(), Some("=A1*2"), "{layout}: Ctrl+'");
        key(&mut h, Key::Escape, Modifiers::NONE);
        // Ctrl++: Insert, as Ctrl+Shift+= on a US keyboard.
        key_on_layout(&mut h, Key::Plus, plus_at, CTRL);
        assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("insertCells"), "{layout}: Ctrl++");
        h.state_mut().dialog = None;
        // Digits go by position: Ctrl+Shift+0 unhides columns although it types `=`.
        h.state_mut().session.run("selection.set", json!({"range": "B:B"})).unwrap();
        h.state_mut().session.run("home.hideColumns", json!({})).unwrap();
        h.state_mut().session.run("selection.set", json!({"range": "A1:C1"})).unwrap();
        key_on_layout(&mut h, Key::Equals, Key::Num0, CTRL_SHIFT);
        assert!(h.state().dialog.is_none(), "{layout}: Ctrl+Shift+0 opened {:?}", h.state().dialog.as_ref().map(|d| d.name()));
        let d = h.state().session.active().unwrap();
        assert!(!d.wb.active().unwrap().is_col_hidden(1), "{layout}: Ctrl+Shift+0 didn't unhide column B");
    }
}

#[test]
fn date_and_time_shortcuts_on_azerty_and_czech_keyboards() {
    // French AZERTY types `;` and `:` unshifted on the US `,` and `.` keys; Czech QWERTZ types `;`
    // unshifted on the US backtick key and `:` with Shift+. (US positions).
    for (layout, semi_at, semi_m, colon_m) in [("fr", Key::Comma, CTRL, CTRL), ("cs", Key::Backtick, CTRL, CTRL_SHIFT)] {
        let mut h = harness(book("C2"));
        let size = style(&h, "C2").font.size;
        key_on_layout(&mut h, Key::Semicolon, semi_at, semi_m);
        assert_eq!(editor_text(&h), Some(today(&h)), "{layout}: Ctrl+;");
        key(&mut h, Key::Escape, Modifiers::NONE);
        key_on_layout(&mut h, Key::Colon, Key::Period, colon_m);
        let time = editor_text(&h).unwrap_or_default();
        assert!(time.contains(':') && (time.ends_with("AM") || time.ends_with("PM")), "{layout}: Ctrl+: gave {time:?}");
        assert_eq!(style(&h, "C2").font.size, size, "{layout}: the font size changed");
    }
}

#[test]
fn layout_keys_type_their_character_in_a_cell_being_edited() {
    // While a cell is edited the grid reads no shortcuts, so `;` and `:` (Shift+, and Shift+. on
    // Croatian and German keyboards) are typed, and nothing changes the font size.
    let mut h = harness(book("C2"));
    let size = style(&h, "C2").font.size;
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    for (typed, at, text) in [(Key::Semicolon, Key::Comma, ";"), (Key::Colon, Key::Period, ":")] {
        let m = Modifiers::SHIFT;
        h.input_mut().events.push(Event::Key { key: typed, physical_key: Some(at), pressed: true, repeat: false, modifiers: m });
        h.input_mut().events.push(Event::Text(text.into()));
        h.input_mut().events.push(Event::Key { key: typed, physical_key: Some(at), pressed: false, repeat: false, modifiers: m });
        h.run_steps(2);
    }
    assert_eq!(editor_text(&h).as_deref(), Some("x;:"));
    assert_eq!(style(&h, "C2").font.size, size);
}

#[test]
fn us_symbol_shortcuts_keep_their_keys() {
    let mut h = harness(book("C2"));
    // Ctrl+;: the date. Ctrl+Shift+; arrives as `:` on Windows and as `;` with Shift elsewhere.
    key(&mut h, Key::Semicolon, CTRL);
    assert_eq!(editor_text(&h), Some(today(&h)));
    key(&mut h, Key::Escape, Modifiers::NONE);
    for typed in [Key::Colon, Key::Semicolon] {
        key_on_layout(&mut h, typed, Key::Semicolon, CTRL_SHIFT);
        assert!(editor_text(&h).is_some_and(|t| t.ends_with("AM") || t.ends_with("PM")), "Ctrl+Shift+; as {typed:?}");
        key(&mut h, Key::Escape, Modifiers::NONE);
    }
    // Ctrl+Shift+, and Ctrl+Shift+. (`<` and `>` have no egui key) change the font size.
    let size = style(&h, "C2").font.size;
    key(&mut h, Key::Period, CTRL_SHIFT);
    assert!(style(&h, "C2").font.size > size);
    key(&mut h, Key::Comma, CTRL_SHIFT);
    assert_eq!(style(&h, "C2").font.size, size);
    // Ctrl+Shift+= arrives as `+`: Insert.
    key_on_layout(&mut h, Key::Plus, Key::Equals, CTRL_SHIFT);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("insertCells"));
    let run = |id: &'static str| Some(Shortcut::Run(id, json!({})));
    // The lookup itself: `;` typed with Shift is Ctrl+; (handled with the Ctrl letters), not the
    // key's position (Decrease Font Size).
    assert_eq!(lookup(Key::Semicolon, Some(Key::Comma), CTRL_SHIFT, false), None);
    assert_eq!(lookup(Key::Colon, Some(Key::Period), CTRL_SHIFT, false), Some(Shortcut::Time));
    assert_eq!(lookup(Key::Comma, Some(Key::Comma), CTRL_SHIFT, false), run("home.decreaseFontSize"));
    // A character egui has no key for (`č` on the US `;` key) leaves the position.
    assert_eq!(lookup(Key::Semicolon, Some(Key::Semicolon), CTRL_SHIFT, false), Some(Shortcut::Time));
}

#[test]
fn cmd_shift_semicolon_on_a_mac_does_not_insert_the_date() {
    // The Mac table has Cmd+; (the time) and Control+; (the date), and nothing for Cmd+Shift+;. It
    // arrives as `:` on the US `;` key, or on the US `.` key of a Croatian or German keyboard.
    let mut h = harness(book("C2"));
    h.ctx.set_os(egui::os::OperatingSystem::Mac);
    let cmd_shift = Modifiers { shift: true, mac_cmd: true, command: true, ..Modifiers::NONE };
    for at in [Key::Semicolon, Key::Period] {
        key_on_layout(&mut h, Key::Colon, at, cmd_shift);
        assert_eq!(editor_text(&h), None, "Cmd+Shift+; at {at:?}");
    }
    key(&mut h, Key::Semicolon, Modifiers { mac_cmd: true, command: true, ..Modifiers::NONE });
    assert!(editor_text(&h).is_some_and(|t| t.ends_with("AM") || t.ends_with("PM")), "Cmd+; is still the time");
}

/// #1443: Ctrl+Q (also with Shift or Alt, which is what AltGr+Q reports on Windows) must not
/// close the window and lose unsaved input; macOS keeps Cmd+Q as the app's Quit.
#[test]
fn ctrl_q_does_not_close_the_window_outside_macos() {
    let closes = |h: &mut Harness, k: Key, m: Modifiers| -> bool {
        h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m });
        h.input_mut().events.push(Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m });
        let mut closed = false;
        for _ in 0..3 {
            h.step();
            closed |= h.output().viewport_output.values().any(|v| v.commands.iter().any(|c| matches!(c, egui::ViewportCommand::Close)));
        }
        closed
    };
    let ctrl_alt = Modifiers { alt: true, ..CTRL };
    for os in [egui::os::OperatingSystem::Windows, egui::os::OperatingSystem::Nix] {
        let mut h = harness(book("A1"));
        h.ctx.set_os(os);
        h.run_steps(2);
        for (name, m) in [("Ctrl+Q", CTRL), ("Ctrl+Shift+Q", CTRL_SHIFT), ("Ctrl+Alt+Q", ctrl_alt)] {
            assert!(!closes(&mut h, Key::Q, m), "{os:?}: {name} closed the window");
        }
        assert!(h.ctx.options(|o| o.quit_shortcuts.is_empty()), "{os:?}: no quit shortcut left");
    }
    // macOS: Cmd+Q stays the app's Quit.
    let mut h = harness(book("A1"));
    // The harness's first frame already ran as the host OS; restore egui's default, as a real
    // macOS launch would have it.
    h.ctx.options_mut(|o| o.quit_shortcuts = vec![egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::Q)]);
    h.ctx.set_os(egui::os::OperatingSystem::Mac);
    h.run_steps(2);
    assert!(h.ctx.options(|o| !o.quit_shortcuts.is_empty()), "Cmd+Q is still egui's quit shortcut on macOS");
}

#[test]
fn ctrl_6_is_show_objects_and_nothing_else() {
    let show = Some(Shortcut::Run("view.showObjects", json!({})));
    let mac_control = Modifiers::CTRL;
    assert_eq!(lookup(Key::Num6, Some(Key::Num6), CTRL, false), show);
    assert_eq!(lookup(Key::Num6, Some(Key::Num6), mac_control, true), show);
    // Its neighbours keep theirs: Ctrl+Shift+6 (Ctrl+^) is Scientific, Cmd+6 on a Mac is free.
    assert_eq!(lookup(Key::Num6, Some(Key::Num6), CTRL_SHIFT, false), Some(Shortcut::Run("home.numberFormat", json!({"code": "0.00E+00"}))));
    assert_eq!(lookup(Key::Num6, Some(Key::Num6), Modifiers::MAC_CMD, true), None);
    // No other command claims Ctrl+6.
    let claims: Vec<&str> = Session::new().commands().iter().filter(|c| c.shortcut == Some("Ctrl+6")).map(|c| c.id).collect();
    assert_eq!(claims, ["view.showObjects"]);
}

/// Alt+= as Windows and Linux report it: the key press, then the `=` the OS types with it.
fn alt_equals(h: &mut Harness) {
    h.input_mut().events.push(Event::Key {
        key: Key::Equals,
        physical_key: Some(Key::Equals),
        pressed: true,
        repeat: false,
        modifiers: Modifiers::ALT,
    });
    h.input_mut().events.push(Event::Text("=".into()));
    h.run_steps(2);
}

#[test]
fn alt_equals_autosum_does_not_also_type_the_equals_sign() {
    let input = |h: &Harness, a1: &str| h.state().session.active().unwrap().wb.active().unwrap().input_text(CellRef::parse(a1).unwrap());
    // Column above, row to the left, and a block (totals below each column).
    for (selected, cell, formula) in [("A4", "A4", "=SUM(A1:A3)"), ("C3", "C3", "=SUM(A3:B3)"), ("A1:B3", "B4", "=SUM(B1:B3)")] {
        let mut h = harness(book(selected));
        alt_equals(&mut h);
        // One cell: the formula waits in the editor for Enter, as in Excel (#1834); a block gets
        // its totals at once. Either way without a stray `=`.
        let editing = h.state().editor.as_ref().map(|e| e.text.clone());
        assert_eq!(editing.as_deref(), (!selected.contains(':')).then_some(formula), "Alt+= on {selected}");
        key(&mut h, Key::Enter, Modifiers::NONE);
        assert!(h.state().message.is_none(), "Alt+= on {selected}: {:?}", h.state().message);
        assert_eq!(input(&h, cell), formula);
    }
    // Nothing to sum: AutoSum's own edit, `=SUM()`, not `=`.
    let mut h = harness(book("E10"));
    alt_equals(&mut h);
    assert_eq!(h.state().editor.as_ref().map(|e| e.text.clone()).as_deref(), Some("=SUM()"));
}

#[test]
fn ctrl_alt_letter_that_typed_text_is_not_select_all() {
    let mut h = harness(book("B2"));
    let ctrl_alt = Modifiers { alt: true, ..CTRL };
    h.input_mut().events.push(Event::Key { key: Key::A, physical_key: Some(Key::A), pressed: true, repeat: false, modifiers: ctrl_alt });
    h.input_mut().events.push(Event::Text("\u{e4}".into()));
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().selection.current().a1(), "B2");
    assert_eq!(h.state().editor.as_ref().unwrap().text, "\u{e4}");
}

/// Only plain Ctrl+Q toggles Quick Analysis; Ctrl+Shift+Q and Ctrl+Alt+Q (AltGr+Q) do not.
#[test]
fn only_plain_ctrl_q_toggles_quick_analysis() {
    let mut h = harness(book("A1:B3"));
    key(&mut h, Key::Q, CTRL_SHIFT);
    assert!(h.state().grid.quick_analysis.open.is_none(), "Ctrl+Shift+Q");
    key(&mut h, Key::Q, Modifiers { alt: true, ..CTRL });
    assert!(h.state().grid.quick_analysis.open.is_none(), "Ctrl+Alt+Q");
    key(&mut h, Key::Q, CTRL);
    assert!(h.state().grid.quick_analysis.open.is_some(), "Ctrl+Q");
}

/// Excel for Mac: Control+Q (ctrl without command) opens Quick Analysis, Command+Q is left to the
/// native Quit item, and a single cell still opens nothing without an alert.
#[test]
fn mac_control_q_toggles_quick_analysis_and_command_q_does_not() {
    const MAC_CONTROL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: false };
    const MAC_COMMAND: Modifiers = Modifiers { alt: false, ctrl: false, shift: false, mac_cmd: true, command: true };
    assert_eq!(lookup(Key::Q, Some(Key::Q), MAC_CONTROL, true), Some(Shortcut::QuickAnalysis));
    assert_eq!(lookup(Key::Q, Some(Key::Q), MAC_COMMAND, true), None);
    assert_eq!(lookup(Key::Q, Some(Key::Q), MAC_CONTROL, false), None, "Windows Ctrl+Q goes through ribbon::shortcut");
    let mac = |sel: &str| crate::common::os_harness(book(sel), egui::os::OperatingSystem::Mac);
    let mut h = mac("A1:B3");
    key(&mut h, Key::Q, MAC_COMMAND);
    assert!(h.state().grid.quick_analysis.open.is_none(), "Command+Q is Quit");
    key(&mut h, Key::Q, Modifiers { shift: true, ..MAC_CONTROL });
    assert!(h.state().grid.quick_analysis.open.is_none(), "Control+Shift+Q");
    key(&mut h, Key::Q, MAC_CONTROL);
    assert!(h.state().grid.quick_analysis.open.is_some(), "Control+Q opens");
    key(&mut h, Key::Q, MAC_CONTROL);
    assert!(h.state().grid.quick_analysis.open.is_none(), "Control+Q again closes");
    let mut h = mac("B2");
    key(&mut h, Key::Q, MAC_CONTROL);
    assert!(h.state().grid.quick_analysis.open.is_none());
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "no alert for a single cell");
}

#[test]
fn custom_shortcut_triggers_command_and_dialog() {
    let mut h = harness(book("A1:B2"));
    let mac = h.ctx.os() == egui::os::OperatingSystem::Mac;
    let combo = if mac { "Cmd+Shift+K" } else { "Ctrl+Shift+K" };
    let mods = if mac { Modifiers { mac_cmd: true, command: true, shift: true, ..Modifiers::NONE } } else { CTRL_SHIFT };
    h.state_mut().ui.shortcuts.insert(combo.into(), "dialog:customizeKeyboard".into());
    key(&mut h, Key::K, mods);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("customizeKeyboard"));
    h.state_mut().dialog = None;

    // Configurable commands like home.borders receive default parameters
    let border_combo = if mac { "Cmd+Shift+B" } else { "Ctrl+Shift+B" };
    h.state_mut().ui.shortcuts.insert(border_combo.into(), "home.borders".into());
    key(&mut h, Key::B, mods);
    assert!(style(&h, "B2").borders.bottom.style != BorderStyle::None);

    // view.zoom opens the zoom dialog without error
    let zoom_combo = if mac { "Cmd+Shift+Z" } else { "Ctrl+Shift+Z" };
    h.state_mut().ui.shortcuts.insert(zoom_combo.into(), "view.zoom".into());
    key(&mut h, Key::Z, mods);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("zoom"));
    h.state_mut().dialog = None;

    // Native copy event checks custom shortcuts before clipboard fallback
    let copy_combo = if mac { "Cmd+C" } else { "Ctrl+C" };
    h.state_mut().ui.shortcuts.insert(copy_combo.into(), "dialog:customizeKeyboard".into());
    h.input_mut().events.push(Event::Copy);
    h.run_steps(1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("customizeKeyboard"));

    // Disabled copy shortcut consumes event without alert or clipboard copy
    h.state_mut().dialog = None;
    h.state_mut().ui.shortcuts.insert(copy_combo.into(), "".into());
    h.input_mut().events.push(Event::Copy);
    h.run_steps(1);
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.is_none());

    // UI commands for shortcut management
    h.state_mut().run("ui.shortcut.assign", json!({"shortcut": combo, "command": "dialog:customizeKeyboard"})).unwrap();
    assert_eq!(h.state().ui.shortcuts.get(combo), Some(&"dialog:customizeKeyboard".to_string()));

    h.state_mut().run("ui.shortcut.remove", json!({"shortcut": combo, "command": "dialog:customizeKeyboard"})).unwrap();
    assert!(!h.state().ui.shortcuts.contains_key(combo));

    h.state_mut().run("ui.shortcut.assign", json!({"shortcut": combo, "command": "dialog:customizeKeyboard"})).unwrap();
    assert!(!h.state().ui.shortcuts.is_empty());
    h.state_mut().run("ui.shortcut.resetAll", json!({})).unwrap();
    assert!(h.state().ui.shortcuts.is_empty());
}
