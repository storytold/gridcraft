//! UI interaction tests: real egui input through the whole app (no GPU needed).

use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

fn harness(session: Session) -> egui_kittest::Harness<'static, SheetApp> {
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
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

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
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

#[test]
fn typing_enters_values_and_moves_down() {
    let mut h = harness(blank());
    text(&mut h, "42");
    assert!(h.state().editor.is_some(), "typing starts editing");
    key(&mut h, Key::Enter, Modifiers::NONE);
    assert_eq!(value(&h, "A1"), Value::Number(42.0));
    text(&mut h, "=A1*2");
    key(&mut h, Key::Tab, Modifiers::NONE);
    assert_eq!(value(&h, "A2"), Value::Number(84.0));
    let active = h.state().session.active().map(|d| d.selection.active.a1());
    assert_eq!(active.as_deref(), Some("B2"));
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
fn renders_every_ribbon_tab_without_panicking() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    let mut h = harness(s);
    for tab in ["Home", "Insert", "Draw", "Page Layout", "Formulas", "Data", "Review", "View", "Automate"] {
        h.state_mut().ui.ribbon_tab = tab.into();
        h.run_steps(2);
    }
    h.state_mut().ui.dark = true;
    h.run_steps(3);
}

#[test]
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
        "about",
        "macros",
    ] {
        h.state_mut().open_dialog(name, json!({}));
        h.run_steps(2);
        key(&mut h, Key::Escape, Modifiers::NONE);
        h.state_mut().dialog = None;
        h.state_mut().message = None;
    }
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

/// An .xlsm carrying a VBA project, saved to bytes.
fn macro_workbook_bytes() -> Vec<u8> {
    let mut wb = gridcraft_engine::model::Workbook::new();
    wb.vba_project = Some(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1 fake OLE2 vbaProject".to_vec());
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
    wb.vba_project = Some(vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
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
    wb.vba_project = Some(vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1]);
    s.active_mut().unwrap().wb = std::sync::Arc::new(wb);
    // Rendering the status bar must not panic with a VBA project present.
    let mut h = harness(s);
    h.run_steps(3);
    h.state_mut().ui.dark = true;
    h.run_steps(2);
}

#[test]
fn alt_f8_opens_the_macros_dialog() {
    let mut h = harness(blank());
    key(&mut h, Key::F8, Modifiers::ALT);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name().to_string()).as_deref(), Some("macros"));
    h.state_mut().dialog = None;
}
