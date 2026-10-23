//! Excel keyboard shortcuts (crate::keymap) dispatched through real egui key events.

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::BorderStyle;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::keymap::{Shortcut, lookup};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
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
            assert_eq!(h.ctx.data(|data| data.get_temp::<String>(egui::Id::new(("filter_search", column)))).as_deref(), Some("1"));
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
            session.execute("table.filterButton", json!({"on": false})).unwrap();
        }
        session.execute("selection.set", json!({"cell": "B1"})).unwrap();
        let mut h = harness(session);
        key(&mut h, Key::ArrowDown, Modifiers::ALT);
        assert_eq!(h.state().grid.filter_menu.map(|(column, _, _)| column), enabled.then_some(1));
        assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "B1");
    }
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
        assert_eq!(input(&h, cell), formula, "Alt+= on {selected}");
        assert!(h.state().editor.is_none(), "Alt+= on {selected} started an edit");
        // Enter moves on: there is no stray `=` edit to commit.
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
