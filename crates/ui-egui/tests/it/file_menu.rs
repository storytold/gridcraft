//! The File tab opens the Backstage view, which exposes the file operations without a native menu.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_engine::{Session, core::CellRef, core::Value};
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::{Services, SheetApp};

fn harness(services: Services) -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::services_harness(services, 4)
}

/// Opens File (the Backstage view) and clicks a navigation entry or page button.
fn choose(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    if h.state().backstage.is_none() {
        h.get_by_label("File").click();
        h.run_steps(3);
    }
    click(h, label);
}

fn click(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(3);
}

fn page(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<&'static str> {
    h.state().backstage.as_ref().map(|b| b.page.id())
}

#[test]
fn new_and_open_from_the_file_tab_keep_the_ribbon_as_it_was() {
    let opened = Arc::new(Mutex::new(0));
    let capture = opened.clone();
    let mut h = harness(Services {
        pick_open: Some(Box::new(move || {
            *capture.lock().unwrap() += 1;
            None
        })),
        ..Default::default()
    });
    h.state_mut().ui.ribbon_tab = "Data".into();
    h.state_mut().ui.ribbon_collapsed = true;
    h.run_steps(2);
    h.get_by_label("File").click();
    h.run_steps(3);
    assert_eq!(page(&h), Some("home"), "the File tab opens the Backstage view on Home");
    choose(&mut h, "New");
    assert_eq!(page(&h), Some("new"));
    click(&mut h, "Blank workbook");
    assert_eq!(h.state().session.documents().len(), 2);
    assert!(h.state().backstage.is_none(), "creating a workbook returns to the sheet");
    choose(&mut h, "Open");
    click(&mut h, "Browse…");
    assert_eq!(*opened.lock().unwrap(), 1, "Browse invokes the platform file picker");
    assert_eq!(page(&h), Some("open"), "a cancelled pick stays on File › Open");
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(h.state().backstage.is_none(), "Esc returns to the sheet");
    assert_eq!(h.state().ui.ribbon_tab, "Data");
    assert!(h.state().ui.ribbon_collapsed);
}

#[test]
fn save_actions_include_the_cell_draft_and_close_keeps_the_unsaved_prompt() {
    let downloads = Arc::new(Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    let capture = downloads.clone();
    let mut h = harness(Services {
        download: Some(Box::new(move |name, bytes| capture.lock().unwrap().push((name.to_string(), bytes.to_vec())))),
        ..Default::default()
    });
    h.input_mut().events.push(egui::Event::Text("123".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    choose(&mut h, "Save");
    assert!(h.state().editor.is_none(), "opening File commits the cell draft");
    assert_eq!(page(&h), Some("saveAs"), "a workbook without a file saves through Save As");
    click(&mut h, "Download (.xlsx)");
    assert!(h.state().backstage.is_none());
    choose(&mut h, "Save As");
    click(&mut h, "Download (.xlsx)");
    choose(&mut h, "Export");
    click(&mut h, "CSV (Comma delimited)");
    let saved = downloads.lock().unwrap();
    assert_eq!(saved.len(), 3);
    for (_, bytes) in saved.iter().take(2) {
        let (wb, _) = gridcraft_engine::io::open_bytes("saved.xlsx", bytes).unwrap();
        assert_eq!(wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0));
    }
    assert!(saved[2].0.ends_with(".csv"));
    assert!(String::from_utf8_lossy(&saved[2].1).contains("123"));
    drop(saved);
    choose(&mut h, "Close");
    assert!(h.state().backstage.is_none());
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0));
}

#[test]
/// Options opens, named Backstage pages are reachable and accessible, an invalid page errors, and both close and Back leave Backstage.
fn file_options_opens_the_options_page_and_pages_are_reachable_by_command() {
    let mut h = harness(Services::default());
    let r = h.state_mut().run("file.options", serde_json::json!({})).unwrap();
    assert_eq!(r["calculation"]["mode"], "automatic");
    h.run_steps(3);
    h.check_accessibility();
    assert_eq!(page(&h), Some("options"));
    click(&mut h, "Info");
    h.check_accessibility();
    assert_eq!(page(&h), Some("info"));
    for id in ["print", "export", "saveAs", "new", "open", "home"] {
        h.state_mut().run("ui.backstage", serde_json::json!({"page": id})).unwrap();
        h.run_steps(2);
        h.check_accessibility();
        assert_eq!(page(&h), Some(id));
    }
    assert!(h.state_mut().run("ui.backstage", serde_json::json!({"page": "feedback"})).is_err());
    h.state_mut().run("ui.backstage", serde_json::json!({"page": "close"})).unwrap();
    assert!(h.state().backstage.is_none());
    // The back arrow returns to the sheet too.
    h.state_mut().run("ui.backstage", serde_json::json!({})).unwrap();
    h.run_steps(2);
    click(&mut h, "Back");
    h.check_accessibility();
    assert!(h.state().backstage.is_none());
}

#[test]
fn translated_options_headings_show_no_context_prefix() {
    for language in [Language::Fr, Language::Sr, Language::Nl, Language::Ro] {
        let mut h = harness(Services::default());
        h.state_mut().run("app.language.set", serde_json::json!({"language": language.code()})).unwrap();
        h.state_mut().run("file.options", serde_json::json!({})).unwrap();
        h.run_steps(3);
        assert_eq!(page(&h), Some("options"));
        for key in ["Options|General", "Options|Save"] {
            let heading = language.tr(key);
            assert!(!heading.contains('|'), "{language:?}: {key} shows its context: {heading}");
            assert!(h.query_all_by_label(heading).next().is_some(), "{language:?}: no {heading} heading");
        }
        // Page Layout › Sheet Options: the Gridlines and Headings "View" checkboxes had the same
        // prefix. The View tab may share the word, so count at least the two checkboxes.
        h.state_mut().run("ui.backstage", serde_json::json!({"page": "close"})).unwrap();
        h.state_mut().ui.ribbon_tab = "Page Layout".into();
        h.run_steps(3);
        let view = language.tr("Sheet Options|View");
        assert!(!view.contains('|'), "{language:?}: Sheet Options|View shows its context: {view}");
        assert!(h.query_all_by_label(view).count() >= 2, "{language:?}: the Sheet Options checkboxes don't read {view}");
    }
}

#[test]
fn file_account_shows_the_version_and_opens_about() {
    let mut h = harness(Services::default());
    choose(&mut h, "Account");
    assert_eq!(page(&h), Some("account"));
    assert!(h.state().dialog.is_none());
    // The About dialog also shows the version; it must not be open yet.
    h.get_by_label(env!("CARGO_PKG_VERSION"));
    h.get_by_label(env!("CARGO_PKG_LICENSE"));
    click(&mut h, "About GridCraft");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("about"));
    assert_eq!(page(&h), Some("account"), "About opens over the File view, like Protect Workbook on Info");
}

#[test]
fn export_as_pdf_is_delivered_as_a_download() {
    let files = Arc::new(Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    let capture = files.clone();
    let mut h = harness(Services {
        download: Some(Box::new(move |name, bytes| capture.lock().unwrap().push((name.to_string(), bytes.to_vec())))),
        ..Default::default()
    });
    h.state_mut().run("file.exportPdf", serde_json::json!({})).unwrap();
    let got = files.lock().unwrap();
    assert_eq!(got.len(), 1, "Export as PDF hands a file to the browser");
    assert!(got[0].0.ends_with(".pdf"));
    assert!(got[0].1.starts_with(b"%PDF-"));
}

#[test]
fn save_as_offers_pdf_and_leaves_the_workbook_file_alone() {
    let dir = std::env::temp_dir().join(format!("gridcraft-save-as-type-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let (capture, target) = (asked.clone(), dir.clone());
    let mut h = harness(Services {
        pick_save: Some(Box::new(move |suggested| {
            capture.lock().unwrap().push(suggested.to_string());
            Some(target.join("Report").to_string_lossy().into_owned())
        })),
        ..Default::default()
    });
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    choose(&mut h, "Save As");
    h.state_mut().backstage.as_mut().unwrap().save_format = "pdf";
    h.run_steps(2);
    click(&mut h, "Browse…");
    assert!(asked.lock().unwrap().first().is_some_and(|s| s.ends_with(".pdf")), "the picker is offered a .pdf name");
    assert!(std::fs::read(dir.join("Report.pdf")).unwrap().starts_with(b"%PDF-"));
    let d = h.state().session.active().unwrap();
    assert!(d.path.is_none() && d.is_dirty(), "a PDF is a copy, not the workbook's file");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The window's close button (or Alt+F4, or the OS quitting the app): steps until the request
/// has been handled and returns the viewport commands the app answered with.
fn request_window_close(h: &mut egui_kittest::Harness<'static, SheetApp>) -> Vec<egui::ViewportCommand> {
    h.input_mut().viewports.entry(egui::ViewportId::ROOT).or_default().events.push(egui::ViewportEvent::Close);
    step_commands(h)
}

/// Runs a few frames and gathers the commands sent to the window.
fn step_commands(h: &mut egui_kittest::Harness<'static, SheetApp>) -> Vec<egui::ViewportCommand> {
    let mut out = Vec::new();
    for _ in 0..3 {
        h.step();
        out.extend(h.output().viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.commands.clone()).unwrap_or_default());
    }
    out
}

fn click_commands(h: &mut egui_kittest::Harness<'static, SheetApp>, label: &str) -> Vec<egui::ViewportCommand> {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    step_commands(h)
}

#[test]
fn closing_the_window_asks_about_every_unsaved_workbook() {
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    h.state_mut().run("file.new", serde_json::json!({})).unwrap();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "B2", "input": "7"})).unwrap();
    h.run_steps(2);

    let sent = request_window_close(&mut h);
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "unsaved changes hold the window open: {sent:?}");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    let sent = click_commands(&mut h, "Cancel");
    assert!(!sent.contains(&egui::ViewportCommand::Close), "Cancel keeps the app open");
    assert!(h.state().dialog.is_none());
    assert_eq!(h.state().session.documents().len(), 2, "Cancel keeps both workbooks");

    request_window_close(&mut h);
    let sent = click_commands(&mut h, "Don't Save");
    assert!(!sent.contains(&egui::ViewportCommand::Close), "the other workbook is still unsaved");
    assert_eq!(h.state().session.documents().len(), 1);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"), "then it asks about the next one");
    let sent = click_commands(&mut h, "Don't Save");
    assert!(sent.contains(&egui::ViewportCommand::Close), "once every workbook is answered the window closes: {sent:?}");
    let sent = request_window_close(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::CancelClose), "the close that follows is let through");
}

#[test]
fn closing_the_window_can_save_first_and_a_saved_workbook_closes_at_once() {
    let dir = std::env::temp_dir().join(format!("gridcraft-close-window-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Book.xlsx").to_string_lossy().into_owned();
    let mut h = harness(Services::default());
    h.state_mut().run("file.save", serde_json::json!({"path": &path})).unwrap();
    h.run_steps(2);
    let sent = request_window_close(&mut h);
    assert!(!sent.contains(&egui::ViewportCommand::CancelClose), "nothing unsaved: the window just closes");
    assert!(h.state().dialog.is_none());

    // A cell still being typed is an unsaved change too.
    h.input_mut().events.push(egui::Event::Text("123".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    let sent = request_window_close(&mut h);
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "{sent:?}");
    let sent = click_commands(&mut h, "Save");
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    let (wb, _) = gridcraft_engine::io::open_bytes("Book.xlsx", &std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(wb.active().unwrap().value(CellRef::new(0, 0)), Value::Number(123.0), "Save wrote the change before closing");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `data.csv` holding `a,b` / `1,2`, opened by path (a double-click), in a folder of its own.
fn open_csv(test: &str) -> (std::path::PathBuf, String, egui_kittest::Harness<'static, SheetApp>) {
    let dir = std::env::temp_dir().join(format!("gridcraft-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("data.csv").to_string_lossy().into_owned();
    std::fs::write(&path, "a,b\r\n1,2\r\n").unwrap();
    let mut h = harness(Services::default());
    h.state_mut().run("file.open", serde_json::json!({"path": &path})).unwrap();
    h.run_steps(2);
    (dir, path, h)
}

fn is_open(h: &egui_kittest::Harness<'static, SheetApp>, path: &str) -> bool {
    h.state().session.documents().iter().any(|d| d.path.as_deref() == Some(path))
}

/// #2607: a CSV keeps its workbook "edited" after a save, since the file holds only the active
/// sheet's values; Save in the close question still writes it and closes it, instead of asking again
/// on every close. Bounded to three rounds: on the bug each round asks again and never closes.
#[test]
fn closing_the_window_saves_a_csv_and_closes_instead_of_asking_again() {
    let (dir, path, mut h) = open_csv("close-csv-window");
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A2", "input": "5"})).unwrap();
    h.state_mut().run("file.save", serde_json::json!({})).unwrap();
    h.run_steps(2);
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("a,b\r\n5,2"), "Ctrl+S wrote the CSV");
    assert!(h.state().session.active().unwrap().is_dirty(), "a CSV save still leaves the workbook edited");

    let (mut asked, mut closed) = (0, false);
    for _ in 0..3 {
        let sent = request_window_close(&mut h);
        if !sent.contains(&egui::ViewportCommand::CancelClose) {
            closed = true;
            break;
        }
        assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
        asked += 1;
        if click_commands(&mut h, "Save").contains(&egui::ViewportCommand::Close) {
            closed = true;
            break;
        }
    }
    assert!(closed, "Save never closed the window: asked {asked} times");
    assert_eq!(asked, 1, "asked once, and Save closed the window");
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("a,b\r\n5,2"), "the CSV holds the change");
    let _ = std::fs::remove_dir_all(&dir);
}

/// File › Close on an edited CSV: Cancel keeps it open and unwritten; Save writes it and closes it.
#[test]
fn file_close_then_save_writes_a_csv_and_closes_it() {
    let (dir, path, mut h) = open_csv("close-csv-file");
    h.state_mut().run("cell.set", serde_json::json!({"cell": "B2", "input": "7"})).unwrap();
    h.run_steps(2);
    h.state_mut().run_or_alert("file.close", serde_json::json!({}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    click(&mut h, "Cancel");
    assert!(is_open(&h, &path), "Cancel keeps the workbook open");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a,b\r\n1,2\r\n", "Cancel writes nothing");

    h.state_mut().run_or_alert("file.close", serde_json::json!({}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    click(&mut h, "Save");
    assert!(!is_open(&h, &path), "Save wrote the CSV and closed the workbook");
    assert!(h.state().dialog.is_none() && h.state().message.is_none());
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("a,b\r\n1,7"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A CSV holds one sheet. With a second sheet, Save in the close question still writes the active
/// sheet, but the workbook stays open and says why, so the other sheet isn't lost without asking;
/// Don't Save then closes it. Quitting stops there too.
#[test]
fn save_keeps_a_csv_with_more_sheets_open_and_says_why() {
    let (dir, path, mut h) = open_csv("close-csv-sheets");
    h.state_mut().run("home.insertSheet", serde_json::json!({})).unwrap();
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "kept"})).unwrap();
    h.run_steps(2);
    assert_eq!(h.state().session.active().unwrap().wb.sheets.len(), 2);

    let sent = request_window_close(&mut h);
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "{sent:?}");
    let sent = click_commands(&mut h, "Save");
    assert!(!sent.contains(&egui::ViewportCommand::Close), "the quit stops: {sent:?}");
    assert!(is_open(&h, &path), "the workbook stays open");
    assert!(h.state().dialog.is_none(), "the question is answered");
    let message = h.state().message.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
    assert!(message.contains("only the active sheet"), "{message:?}");
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("kept"), "the active sheet was written");
    h.state_mut().message = None;

    h.state_mut().run_or_alert("file.close", serde_json::json!({}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    click(&mut h, "Don't Save");
    assert!(!is_open(&h, &path), "Don't Save closes it");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A workbook that was never saved has no file for Save to write: it asks where, and stays open.
#[test]
fn file_close_then_save_on_a_new_workbook_asks_where_and_stays_open() {
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "1"})).unwrap();
    h.run_steps(2);
    h.state_mut().run_or_alert("file.close", serde_json::json!({}));
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    click(&mut h, "Save");
    assert_eq!(h.state().session.documents().len(), 1, "still open");
    assert!(h.state().session.active().unwrap().is_dirty());
    assert!(h.state().message.is_none());
}

#[test]
fn quitting_with_one_of_two_workbooks_unsaved_asks_once() {
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    h.state_mut().run("file.new", serde_json::json!({})).unwrap();
    h.run_steps(2);

    let sent = request_window_close(&mut h);
    assert!(sent.contains(&egui::ViewportCommand::CancelClose), "{sent:?}");
    assert_eq!(h.state().dialog.as_ref().map(|d| d.name()), Some("saveChanges"));
    assert_eq!(h.state().session.active_index(), 0, "the unsaved workbook comes forward");
    let sent = click_commands(&mut h, "Cancel");
    assert!(!sent.contains(&egui::ViewportCommand::Close), "Cancel stops the quit");
    assert_eq!(h.state().session.documents().len(), 2, "Cancel keeps both workbooks");

    request_window_close(&mut h);
    let sent = click_commands(&mut h, "Don't Save");
    assert!(sent.contains(&egui::ViewportCommand::Close), "the saved workbook isn't asked about: {sent:?}");
    assert!(h.state().dialog.is_none());
}

/// Book1 has a cell being typed when File › New runs (the macOS menu runs the command directly).
fn typing_in_book1_then_new(h: &mut egui_kittest::Harness<'static, SheetApp>) -> u64 {
    h.input_mut().events.push(egui::Event::Text("hello".into()));
    h.run_steps(2);
    assert!(h.state().editor.is_some());
    let book1 = h.state().session.doc().unwrap().uid;
    h.state_mut().run_or_alert("file.new", serde_json::json!({}));
    h.run_steps(3);
    book1
}

fn a1(h: &egui_kittest::Harness<'static, SheetApp>, doc: usize) -> Value {
    h.state().session.documents()[doc].wb.active().unwrap().value(CellRef::new(0, 0))
}

#[test]
fn new_workbook_keeps_the_typed_cell_in_the_first_and_switch_windows_brings_it_back() {
    let mut h = harness(Services::default());
    let book1 = typing_in_book1_then_new(&mut h);
    assert!(h.state().editor.is_none(), "the draft doesn't follow into Book2");
    assert_eq!(h.state().session.documents().len(), 2);
    assert_eq!(h.state().session.active_index(), 1);
    assert_eq!(a1(&h, 0), Value::text("hello"), "the draft is committed into Book1, as Enter would");
    assert!(h.state().session.documents()[0].is_dirty(), "so quitting asks about Book1");
    assert_eq!(a1(&h, 1), Value::Empty);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(a1(&h, 1), Value::Empty, "Enter in Book2 writes nothing left over");

    // The key tip or command search opens the View tab's list; Book1 shows its unsaved bullet.
    h.state_mut().run_or_alert("view.switchWindows", serde_json::json!({}));
    h.run_steps(3);
    assert_eq!(h.state().ui.ribbon_tab, "View");
    h.get_by_label_contains("Book1 •").click();
    h.run_steps(3);
    assert_eq!(h.state().session.doc().unwrap().uid, book1);
    assert_eq!(h.state().session.documents().len(), 2);
    assert!(h.state().message.is_none());
}

#[test]
fn a_draft_its_workbook_refuses_stops_the_switch_and_stays_open() {
    let mut h = harness(Services::default());
    let book1 = typing_in_book1_then_new(&mut h);
    let book2 = h.state().session.doc().unwrap().uid;
    h.state_mut().session.execute("data.validation", serde_json::json!({"range": "A1", "type": "list", "formula1": "\"Red,Green\""})).unwrap();
    h.input_mut().events.push(egui::Event::Text("Purple".into()));
    h.run_steps(2);
    h.state_mut().run_or_alert("view.switchWindows", serde_json::json!({"doc": book1}));
    h.run_steps(2);
    assert_eq!(h.state().session.doc().unwrap().uid, book2, "the switch doesn't happen");
    assert_eq!(h.state().editor.as_ref().map(|ed| ed.text.as_str()), Some("Purple"), "the draft stays open to fix");
    assert_eq!(a1(&h, 1), Value::Empty, "Book2 is unchanged");
    assert!(h.state().message.is_some(), "the refusal is reported");

    // Esc drops the draft; then the switch goes through.
    h.state_mut().message = None;
    h.state_mut().cancel_edit();
    h.state_mut().run_or_alert("view.switchWindows", serde_json::json!({"doc": book1}));
    h.run_steps(2);
    assert_eq!(h.state().session.doc().unwrap().uid, book1);
    assert!(h.state().message.is_none());
}

#[test]
fn an_untouched_f2_draft_leaves_the_workbook_clean_on_a_file_command() {
    let dir = std::env::temp_dir().join(format!("gridcraft-f2-draft-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Book.xlsx").to_string_lossy().into_owned();
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "=1+2"})).unwrap();
    h.state_mut().run("file.save", serde_json::json!({"path": &path})).unwrap();
    h.run_steps(2);
    let undo = h.state().session.doc().unwrap().undo.len();
    h.key_press(egui::Key::F2);
    h.run_steps(2);
    assert_eq!(h.state().editor.as_ref().map(|ed| ed.text.as_str()), Some("=1+2"));
    h.state_mut().run_or_alert("file.new", serde_json::json!({}));
    h.run_steps(2);
    assert!(h.state().editor.is_none());
    let book1 = &h.state().session.documents()[0];
    assert!(!book1.is_dirty(), "nothing was typed, so nothing changed");
    assert_eq!(book1.undo.len(), undo, "and no undo step was added");
    let _ = std::fs::remove_dir_all(&dir);
}

fn pdf_picker_harness(dir: &std::path::Path) -> (Arc<Mutex<Vec<String>>>, egui_kittest::Harness<'static, SheetApp>) {
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let (capture, target) = (asked.clone(), dir.to_path_buf());
    let h = harness(Services {
        pick_save: Some(Box::new(move |suggested| {
            capture.lock().unwrap().push(suggested.to_string());
            Some(target.join("Report.xlsx").to_string_lossy().into_owned())
        })),
        ..Default::default()
    });
    (asked, h)
}

#[test]
fn the_palette_export_as_pdf_asks_where_to_save() {
    let dir = std::env::temp_dir().join(format!("gridcraft-export-pdf-palette-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (asked, mut h) = pdf_picker_harness(&dir);
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    // The palette and menus go through run_or_alert.
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({}));
    assert!(asked.lock().unwrap().first().is_some_and(|s| s.ends_with(".pdf")), "the picker is offered a .pdf name");
    assert!(std::fs::read(dir.join("Report.pdf")).unwrap().starts_with(b"%PDF-"), "the PDF is written, whatever extension the picker returned");
    assert!(h.state().message.is_none());
    let d = h.state().session.active().unwrap();
    assert!(d.path.is_none(), "a PDF is a copy, not the workbook's file");
    // Other params survive the round trip through the picker.
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({"range": "nope"}));
    assert!(h.state().message.is_some(), "a bad range is reported, not swallowed");
    // A path in the params skips the picker.
    let before = asked.lock().unwrap().len();
    let named = dir.join("Named.pdf");
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({"path": named.to_string_lossy()}));
    assert_eq!(asked.lock().unwrap().len(), before);
    assert!(named.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cancelled_export_pdf_picker_writes_nothing() {
    let mut h = harness(Services { pick_save: Some(Box::new(|_| None)), ..Default::default() });
    h.state_mut().run_or_alert("file.exportPdf", serde_json::json!({}));
    assert!(h.state().message.is_none());
    assert!(h.state_mut().session.take_ui_requests().is_empty());
}

/// Options › Formulas: Maximum Iterations and Maximum Change, with the checkbox that enables them.
#[test]
fn options_sets_maximum_iterations_and_change_only_while_iteration_is_on() {
    use egui::accesskit::Role;
    let mut session = Session::new();
    session.new_workbook();
    // Tall enough that the Formulas section is on screen without scrolling.
    let mut h = crate::common::builder().with_size(egui::vec2(1200.0, 1400.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        crate::common::prepare(SheetApp::new(session, Services::default())),
    );
    h.run_steps(4);
    h.state_mut().run("file.options", serde_json::json!({})).unwrap();
    h.run_steps(3);
    let calc = |h: &egui_kittest::Harness<'static, SheetApp>| h.state().session.active().unwrap().wb.calc.clone();
    // Resolve the field by its accessible name in both the disabled and enabled states.
    let field = |h: &egui_kittest::Harness<'static, SheetApp>, label: &str| h.get_by_role_and_label(Role::TextInput, label).rect();
    let disabled = |h: &egui_kittest::Harness<'static, SheetApp>, rect: egui::Rect| {
        h.get_all_by_role(Role::TextInput).find(|n| n.rect() == rect).unwrap().accesskit_node().is_disabled()
    };
    let retype = |h: &mut egui_kittest::Harness<'static, SheetApp>, rect: egui::Rect, text: &str| {
        h.input_mut().events.push(egui::Event::PointerMoved(rect.center()));
        h.run_steps(1);
        h.get_all_by_role(Role::TextInput).find(|n| n.rect() == rect).unwrap().click();
        h.run_steps(2);
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run_steps(1);
        for c in text.chars() {
            h.input_mut().events.push(egui::Event::Text(c.to_string()));
            h.run_steps(2);
        }
        h.key_press(egui::Key::Enter);
        h.run_steps(2);
    };

    // Iteration is off: the two fields show the defaults, greyed out.
    let (iterations, change) = (field(&h, "Maximum Iterations:"), field(&h, "Maximum Change:"));
    assert!(disabled(&h, iterations) && disabled(&h, change), "the fields are disabled while iteration is off");
    assert_eq!(h.get_all_by_role(Role::TextInput).find(|n| n.rect() == iterations).unwrap().value().as_deref(), Some("100"));
    assert_eq!(h.get_all_by_role(Role::TextInput).find(|n| n.rect() == change).unwrap().value().as_deref(), Some("0.001"));

    h.get_by_role_and_label(Role::CheckBox, "Enable iterative calculation").click();
    h.run_steps(3);
    assert!(calc(&h).iterative);
    let (iterations, change) = (field(&h, "Maximum Iterations:"), field(&h, "Maximum Change:"));
    assert!(!disabled(&h, iterations) && !disabled(&h, change));

    retype(&mut h, iterations, "500");
    assert_eq!(calc(&h).max_iterations, 500);
    retype(&mut h, change, "0.01");
    assert_eq!(calc(&h).max_change, 0.01);
    assert!(h.state().message.is_none());

    // Out-of-range entries are refused with a message and change nothing (the command alone would clamp them).
    for bad in ["0", "32768", "12.5", "lots"] {
        retype(&mut h, iterations, bad);
        assert_eq!(calc(&h).max_iterations, 500, "{bad}");
        assert!(h.state_mut().message.take().is_some(), "{bad} is reported");
        h.run_steps(2);
    }
    retype(&mut h, iterations, "32767");
    assert_eq!(calc(&h).max_iterations, 32767);
    for bad in ["-1", "x"] {
        retype(&mut h, change, bad);
        assert_eq!(calc(&h).max_change, 0.01, "{bad}");
        assert!(h.state_mut().message.take().is_some(), "{bad} is reported");
        h.run_steps(2);
    }
    retype(&mut h, change, "0");
    assert_eq!(calc(&h).max_change, 0.0);

    // Undo puts the old value back in the field.
    h.state_mut().run("edit.undo", serde_json::json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(calc(&h).max_change, 0.01);
    let change = field(&h, "Maximum Change:");
    assert_eq!(h.get_all_by_role(Role::TextInput).find(|n| n.rect() == change).unwrap().value().as_deref(), Some("0.01"));

    // A value typed but not confirmed with Enter is dropped when the checkbox switches iteration off.
    let iterations = field(&h, "Maximum Iterations:");
    h.input_mut().events.push(egui::Event::PointerMoved(iterations.center()));
    h.run_steps(1);
    h.get_all_by_role(Role::TextInput).find(|n| n.rect() == iterations).unwrap().click();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.input_mut().events.push(egui::Event::Text("777".into()));
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Enable iterative calculation").click();
    h.run_steps(3);
    assert!(!calc(&h).iterative);
    assert_eq!(calc(&h).max_iterations, 32767, "an unconfirmed entry is not committed once iteration is off");
    assert!(h.state().message.is_none());

    // Spanish accepts a decimal comma in Maximum Change, and still a dot.
    h.get_by_role_and_label(Role::CheckBox, "Enable iterative calculation").click();
    h.run_steps(3);
    h.state_mut().run("app.language.set", serde_json::json!({"language": Language::Es.code()})).unwrap();
    h.state_mut().run("file.options", serde_json::json!({})).unwrap();
    h.run_steps(3);
    let change = field(&h, "Cambio máximo:");
    retype(&mut h, change, "0,02");
    assert_eq!(calc(&h).max_change, 0.02);
    retype(&mut h, change, "0.5");
    assert_eq!(calc(&h).max_change, 0.5);
    assert!(h.state().message.is_none());
}

/// Share › Export as Web Page (and the native File menu's Save as Web Page) asks for a name in its
/// own dialog. A bare name has no folder: written as typed it lands in the process's working
/// directory, where nobody looks for it (#1212). The save picker chooses the folder instead.
#[test]
fn save_a_copy_with_a_bare_name_asks_where_to_save_it() {
    let dir = std::env::temp_dir().join(format!("gridcraft-save-copy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let asked = Arc::new(Mutex::new(Vec::<String>::new()));
    let (capture, target) = (asked.clone(), dir.clone());
    let mut h = harness(Services {
        pick_save: Some(Box::new(move |suggested| {
            capture.lock().unwrap().push(suggested.to_string());
            Some(target.join("Chosen.html").to_string_lossy().into_owned())
        })),
        ..Default::default()
    });
    h.state_mut().run("cell.set", serde_json::json!({"cell": "A1", "input": "42"})).unwrap();
    let save_copy = |h: &mut egui_kittest::Harness<'static, SheetApp>, path: &str| {
        h.state_mut().open_dialog("saveCopy", serde_json::json!({"format": "html"}));
        h.run_steps(3);
        h.state_mut().dialog.as_mut().unwrap().values.insert("path".into(), serde_json::json!(path));
        h.run_steps(2);
        h.get_by_label("   OK   ").click();
        h.run_steps(3);
    };
    let name = format!("gridcraft-save-copy-{}", std::process::id());
    save_copy(&mut h, &name);
    let stray = std::path::PathBuf::from(format!("{name}.html"));
    let written_in_cwd = stray.exists();
    let _ = std::fs::remove_file(&stray);
    assert!(!written_in_cwd, "a bare name is not written to the working directory");
    assert_eq!(*asked.lock().unwrap(), vec![format!("{name}.html")], "the picker is offered the typed name");
    assert!(std::fs::read_to_string(dir.join("Chosen.html")).unwrap().contains("42"), "the page is written where the picker says");
    assert!(h.state().message.is_none());
    // A full path skips the picker; a write that fails is reported, not swallowed.
    save_copy(&mut h, &dir.join("Named.html").to_string_lossy());
    assert!(dir.join("Named.html").exists());
    save_copy(&mut h, &dir.join("missing").join("Lost.html").to_string_lossy());
    assert!(h.state().message.is_some(), "a failed write is reported");
    assert_eq!(asked.lock().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_window_shows_the_same_workbook_and_a_draft_settles_in_its_own_window() {
    let mut h = harness(Services::default());
    h.state_mut().run("selection.set", serde_json::json!({"range": "B2"})).unwrap();
    let first = h.state().session.active_window().unwrap();
    h.state_mut().run_or_alert("view.newWindow", serde_json::json!({}));
    h.run_steps(2);
    let second = h.state().session.active_window().unwrap();
    let book = h.state().session.doc().unwrap().uid;
    assert_ne!(first, second);
    assert_eq!(h.state().session.documents().len(), 1, "one workbook, two windows");
    let titles: Vec<String> = h.state().open_windows().iter().map(|w| w.title.clone()).collect();
    assert_eq!(titles, ["Book1:1", "Book1:2"]);

    // A draft typed in window 2 is entered in window 2's cell before window 1 comes forward.
    h.state_mut().run("selection.set", serde_json::json!({"range": "D4"})).unwrap();
    h.input_mut().events.push(egui::Event::Text("typed".into()));
    h.run_steps(2);
    h.state_mut().run_or_alert("view.switchWindows", serde_json::json!({"window": first}));
    h.run_steps(2);
    assert!(h.state().editor.is_none(), "the draft doesn't follow into window 1");
    assert_eq!(h.state().session.active_window(), Some(first));
    let d = h.state().session.doc().unwrap();
    assert_eq!(d.wb.active().unwrap().value(CellRef::parse("D4").unwrap()), Value::text("typed"));
    assert_eq!(d.selection.active, CellRef::parse("B2").unwrap(), "window 1 keeps its own selection");

    // Quitting asks once about the workbook, whichever window shows it.
    request_window_close(&mut h);
    let sent = click_commands(&mut h, "Don't Save");
    assert!(sent.contains(&egui::ViewportCommand::Close), "{sent:?}");
    assert!(h.state().session.document_index(book).is_none(), "both windows closed with the workbook");
}

#[test]
fn control_requests_route_to_a_window_and_list_windows() {
    use gridcraft_ui_egui::control::{Outcome, handle};
    let mut h = harness(Services::default());
    let first = h.state().session.active_window().unwrap();
    h.state_mut().run_or_alert("file.new", serde_json::json!({}));
    h.run_steps(2);
    let ctx = h.ctx.clone();
    let call = |h: &mut egui_kittest::Harness<'static, SheetApp>, method: &str, p: serde_json::Value| match handle(h.state_mut(), &ctx, method, &p) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => serde_json::Value::Null,
    };
    let r = call(&mut h, "engine.execute", serde_json::json!({"command": "cell.set", "params": {"cell": "A1", "input": "one"}, "window": first}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(a1(&h, 0), Value::text("one"));
    assert_eq!(a1(&h, 1), Value::Empty);
    assert_eq!(h.state().session.active_index(), 1, "Book2 stays in front");
    let r = call(&mut h, "cell.get", serde_json::json!({"cell": "A1", "window": first}));
    assert!(r["result"].to_string().contains("one"), "{r}");

    let r = call(&mut h, "window.list", serde_json::json!({}));
    let rows = r["result"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{r}");
    assert_eq!((rows[0]["window"].as_u64(), rows[0]["title"].as_str(), rows[0]["dirty"].as_bool()), (Some(first), Some("Book1"), Some(true)));
    assert_eq!(rows[1]["active"].as_bool(), Some(true));
    let r = call(&mut h, "engine.execute", serde_json::json!({"command": "cell.get", "params": {"cell": "A1"}, "window": 999}));
    assert_eq!(r["ok"], false, "{r}");
}
