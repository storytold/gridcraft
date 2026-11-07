//! Ctrl+click on a hyperlink cell follows the link (#1198), like a plain click does.

use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// A OneDrive folder link: `!`, `:`, `&`, `=`, `%` and a fragment must reach the system whole (#2085).
const SHARE: &str =
    "https://onedrive.live.com/:f:/g/personal/0123456789ABCDEF/EaBcDe?resid=0123456789ABCDEF%21123&ithint=folder&redeem=aHR0cHM6Ly9l#a=1&b=2";

/// A real file the local-link tests can point at.
fn local_file() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("gridcraft-ui-link-{}.txt", std::process::id()));
    std::fs::write(&path, "x").unwrap();
    path
}

fn harness() -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B6", "input": "file"})).unwrap();
    session.execute("insert.link", json!({"cell": "B6", "target": local_file().to_str().unwrap()})).unwrap();
    session.execute("cell.set", json!({"cell": "B8", "input": "=HYPERLINK(\"https://example.org/h\",\"go\")"})).unwrap();
    session.execute("cell.set", json!({"cell": "B12", "input": "share"})).unwrap();
    session.execute("insert.link", json!({"cell": "B12", "target": SHARE})).unwrap();
    session.execute("cell.set", json!({"cell": "B10", "input": "tool"})).unwrap();
    session.execute("insert.link", json!({"cell": "B10", "target": "/tmp/setup.exe"})).unwrap();
    session.execute("cell.set", json!({"cell": "B2", "input": "web"})).unwrap();
    session.execute("insert.link", json!({"cell": "B2", "target": "https://example.org/a"})).unwrap();
    session.execute("cell.set", json!({"cell": "B4", "input": "jump"})).unwrap();
    session.execute("insert.link", json!({"cell": "B4", "target": "Sheet1!F9"})).unwrap();
    show(session)
}

/// Renders a session's grid in a test harness.
fn show(session: Session) -> Harness {
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        crate::common::app(session),
    );
    h.run_steps(4);
    h
}

fn at_fraction(h: &Harness, cell: &str, fraction: f32) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let r = geo.cell_rect(sheet, CellRef::parse(cell).unwrap());
    egui::pos2(r.left() + r.width() * fraction, r.center().y)
}

fn click(h: &mut Harness, cell: &str, modifiers: Modifiers) {
    click_at(h, cell, 0.3, modifiers);
}

fn click_at(h: &mut Harness, cell: &str, fraction: f32, modifiers: Modifiers) {
    let pos = at_fraction(h, cell, fraction);
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: true, modifiers },
        Event::PointerButton { pos, button: PointerButton::Primary, pressed: false, modifiers },
    ]);
    h.run_steps(3);
    h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(1);
}

#[test]
fn ctrl_click_on_a_web_link_asks_to_open_it() {
    let mut h = harness();
    click(&mut h, "B2", Modifiers::COMMAND);
    let d = h.state().dialog.as_ref().expect("Ctrl+click on a web link opens the confirmation");
    assert_eq!(d.name, "openLink");
}

#[test]
fn ctrl_click_on_an_internal_link_goes_there() {
    let mut h = harness();
    click(&mut h, "B4", Modifiers::COMMAND);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "F9");
}

#[test]
fn plain_click_still_follows() {
    let mut h = harness();
    click(&mut h, "B4", Modifiers::NONE);
    assert_eq!(h.state().session.active().unwrap().selection.active.a1(), "F9");
}

#[test]
fn ctrl_click_beside_the_link_text_still_extends_the_selection() {
    let mut h = harness();
    click_at(&mut h, "B2", 0.9, Modifiers::NONE);
    click_at(&mut h, "B4", 0.9, Modifiers::COMMAND);
    let d = h.state().session.active().unwrap();
    let ranges: Vec<String> = d.selection.ranges.iter().map(|r| r.a1()).collect();
    assert_eq!(ranges, ["B2", "B4"], "the hyperlink cell joins the selection");
    assert!(h.state().dialog.is_none(), "no link was followed");
}

#[test]
fn ctrl_alt_click_on_the_link_text_still_extends_the_selection() {
    let mut h = harness();
    click_at(&mut h, "B2", 0.9, Modifiers::NONE);
    click_at(&mut h, "B4", 0.3, Modifiers { alt: true, ..Modifiers::COMMAND });
    let d = h.state().session.active().unwrap();
    let ranges: Vec<String> = d.selection.ranges.iter().map(|r| r.a1()).collect();
    assert_eq!(ranges, ["B2", "B4"], "Ctrl+Alt+click adds the hyperlink cell to the selection");
    assert_eq!(d.selection.active.a1(), "B4", "the link was not followed");
    assert!(h.state().dialog.is_none(), "no link dialog");
}

#[test]
/// A click on a local file link opens the confirmation dialog first.
fn a_click_on_a_local_file_link_asks_before_opening_it() {
    let mut h = harness();
    click(&mut h, "B6", Modifiers::NONE);
    let d = h.state().dialog.as_ref().expect("a link to a file on this computer asks first (#2596)");
    assert_eq!(d.name, "openLink");
    assert!(h.state().message.is_none());
}

#[test]
/// A click on a `=HYPERLINK()` cell follows its target.
fn a_click_on_a_hyperlink_formula_follows_it() {
    let mut h = harness();
    click(&mut h, "B8", Modifiers::NONE);
    let d = h.state().dialog.as_ref().expect("=HYPERLINK() cells follow their link on click (#2596)");
    assert_eq!(d.name, "openLink");
}

#[test]
/// A link to a program is refused with a message.
fn a_click_on_a_link_to_a_program_is_refused_with_a_message() {
    let mut h = harness();
    click(&mut h, "B10", Modifiers::NONE);
    assert!(h.state().dialog.is_none());
    assert!(h.state().message.as_ref().is_some_and(|(_, why)| why.contains("programs and scripts")), "{:?}", h.state().message);
}

#[test]
/// Confirming a folder share link hands the whole URL to the system.
fn confirming_a_folder_share_link_hands_the_whole_url_to_the_system() {
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let mut h = harness();
    let sink = opened.clone();
    h.state_mut().services.open_url = Some(Box::new(move |u: &str| sink.borrow_mut().push(u.to_string())));
    click(&mut h, "B12", Modifiers::NONE);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name.as_str()), Some("openLink"));
    h.get_by_label("   OK   ").click();
    h.run_steps(4);
    assert_eq!(*opened.borrow(), [SHARE]);
}

#[test]
/// A formula link to a sibling file opens the path resolved from the saved workbook's folder.
fn a_formula_link_to_a_sibling_file_opens_the_resolved_path_of_a_saved_workbook() {
    #[allow(clippy::disallowed_methods)] // native-only test: a unique temp dir name
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("gridcraft-ui-sibling-{}-{nonce}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("target.txt"), "x").unwrap();
    let mut session = Session::new();
    session.new_workbook();
    session.execute("cell.set", json!({"cell": "B8", "input": "=HYPERLINK(\"target.txt\",\"open\")"})).unwrap();
    session.execute("file.saveAs", json!({"path": dir.join("Book.xlsx").to_str().unwrap()})).unwrap();
    let opened = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let mut h = show(session);
    let sink = opened.clone();
    h.state_mut().services.open_url = Some(Box::new(move |u: &str| sink.borrow_mut().push(u.to_string())));
    click(&mut h, "B8", Modifiers::NONE);
    let d = h.state().dialog.as_ref().expect("a bare file name in =HYPERLINK() is a file link in a saved workbook");
    assert_eq!(d.name, "openLink");
    assert_eq!(d.values.get("local").and_then(serde_json::Value::as_bool), Some(true));
    // The host gets the resolved path (a temp dir may sit under a symlink, as on macOS).
    let real = std::fs::canonicalize(dir.join("target.txt")).unwrap().to_string_lossy().into_owned();
    let expected = real.strip_prefix("\\\\?\\").filter(|rest| rest.as_bytes().get(1) == Some(&b':')).map_or_else(|| real.clone(), str::to_owned);
    assert_eq!(d.values.get("url").and_then(serde_json::Value::as_str), Some(expected.as_str()));
    h.get_by_label("   OK   ").click();
    h.run_steps(4);
    assert_eq!(*opened.borrow(), [expected]);
    std::fs::remove_dir_all(dir).unwrap();
}
