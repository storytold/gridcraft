//! macOS: the native menu bar (File, Edit, View, Insert, Format, Tools, Data, Window, Help), as
//! a desktop spreadsheet has on the Mac. Items run the same commands as the ribbon.
//!
//! The Window menu ends with the open workbooks, as Excel's does: one check item per visible
//! document ([`window_entries`]), the active one checked, kept in sync with the session on every
//! [`NativeMenu::poll`]. Today every document shares the one egui window, which is kept out of
//! AppKit's own list for the windows menu (it would show once more, titled after the active
//! document). A native-window-per-document implementation will map its NSWindows to the same
//! document uids, and this list is then macOS's own window list (the menu is NSApp's
//! `windowsMenu`).
//!
//! muda places and removes items by position and takes its own positions for AppKit's, while
//! AppKit adds items of its own to the windows menu (Move & Resize, Fill and Center). So the
//! Window submenu is never edited in place when the set of documents changes: a fresh submenu
//! (static items, separator, documents) replaces it in the menu bar, whose positions only muda
//! touches. Titles and check marks of listed documents change in place.

use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::{Value, json};

type Entry = (&'static str, &'static str, Option<&'static str>);

/// Cmd+` and Cmd+Shift+`: what macOS itself binds to "Cycle Through Windows". muda spells the
/// key as the character (`parse_named_code` accepts "`" and "BACKQUOTE").
const NEXT_WINDOW_ACCEL: &str = "CMD+`";
const PREVIOUS_WINDOW_ACCEL: &str = "CMD+SHIFT+`";

/// Menu ids of the per-document items: `window:<uid>`.
const WINDOW_ITEM_PREFIX: &str = "window:";

/// A window the Window menu lists: an open document whose window is not hidden.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowEntry {
    /// The document's uid (stable for the document's lifetime, unlike its index).
    pub uid: u64,
    /// The menu label: the title-bar text (file name or `BookN`, " — Edited" while unsaved).
    pub title: String,
    pub active: bool,
}

/// The windows to list, in document order, hidden ones (View › Hide) omitted.
pub(crate) fn window_entries(session: &Session) -> Vec<WindowEntry> {
    let active = session.active_index();
    session
        .documents()
        .iter()
        .enumerate()
        .filter(|(_, d)| !session.hidden_windows.contains(&d.uid))
        .map(|(i, d)| WindowEntry {
            uid: d.uid,
            title: format!("{}{}", d.display_title(), if d.is_dirty() { " — Edited" } else { "" }),
            active: i == active,
        })
        .collect()
}

fn window_item_id(uid: u64) -> String {
    format!("{WINDOW_ITEM_PREFIX}{uid}")
}

fn window_item_uid(id: &str) -> Option<u64> {
    id.strip_prefix(WINDOW_ITEM_PREFIX)?.parse().ok()
}

/// (label, command or `dialog:name`, accelerator). `-` is a separator.
fn tree() -> Vec<(&'static str, Vec<Entry>)> {
    vec![
        (
            "File",
            vec![
                ("New Workbook", "file.new", Some("CMD+N")),
                ("New from Sample…", "dialog:start", None),
                ("Open…", "file.open", Some("CMD+O")),
                ("-", "", None),
                ("Close", "file.close", Some("CMD+W")),
                ("Save", "file.save", Some("CMD+S")),
                ("Save As…", "file.saveAs", Some("CMD+SHIFT+S")),
                ("Export as PDF…", "file.exportPdf", None),
                ("Export as CSV…", "dialog:saveCopy:csv", None),
                ("Save as Web Page…", "dialog:saveCopy:html", None),
                ("-", "", None),
                ("Page Setup…", "dialog:pageSetup", None),
                ("Print…", "file.print", Some("CMD+P")),
                ("-", "", None),
                ("Properties…", "dialog:backstage:info", None),
            ],
        ),
        (
            "Edit",
            vec![
                ("Undo", "edit.undo", None),
                ("Redo", "edit.redo", None),
                ("-", "", None),
                ("Cut", "edit.cut", None),
                ("Copy", "edit.copy", None),
                ("Paste", "edit.paste", None),
                ("Paste Special…", "dialog:pasteSpecial", Some("CTRL+CMD+V")),
                ("-", "", None),
                ("Fill Down", "edit.fillDown", None),
                ("Fill Right", "edit.fillRight", None),
                ("Series…", "dialog:series", None),
                ("Flash Fill", "edit.flashFill", None),
                ("-", "", None),
                ("Clear All", "edit.clearAll", None),
                ("Clear Formats", "edit.clearFormats", None),
                ("Clear Contents", "edit.clearContents", None),
                ("-", "", None),
                ("Delete…", "dialog:deleteCells", None),
                ("Delete Sheet", "home.deleteSheet", None),
                ("-", "", None),
                ("Find…", "dialog:find", None),
                ("Replace…", "dialog:find:replace", Some("CTRL+H")),
                ("Go To…", "dialog:goTo", Some("CTRL+G")),
            ],
        ),
        (
            "View",
            vec![
                ("Normal", "view.normal", None),
                ("Page Layout", "view.pageLayout", None),
                ("Page Break Preview", "view.pageBreakPreview", None),
                ("-", "", None),
                ("Formula Bar", "view.formulaBar", None),
                ("Gridlines", "view.gridlines", None),
                ("Headings", "view.headings", None),
                ("-", "", None),
                ("Zoom…", "dialog:zoom", None),
                ("Zoom to Selection", "view.zoomToSelection", None),
                ("Freeze Panes", "view.freezePanes", None),
                ("Freeze Top Row", "view.freezeTopRow", None),
                ("Unfreeze Panes", "view.unfreezePanes", None),
                ("-", "", None),
                ("Display Theme", "view.theme", None),
                ("Collapse Ribbon", "view.collapseRibbon", Some("CMD+ALT+R")),
            ],
        ),
        (
            "Insert",
            vec![
                ("Cells…", "dialog:insertCells", None),
                ("Rows", "home.insertRows", None),
                ("Columns", "home.insertColumns", None),
                ("Sheet", "home.insertSheet", Some("SHIFT+F11")),
                ("-", "", None),
                ("Chart", "insert.recommendedCharts", None),
                ("Sparklines…", "dialog:sparkline", None),
                ("PivotTable", "insert.pivotTable", None),
                ("Table", "insert.table", None),
                ("-", "", None),
                ("Function…", "dialog:insertFunction", Some("SHIFT+F3")),
                ("Name…", "dialog:defineName", None),
                ("New Comment", "dialog:comment", None),
                ("New Note", "dialog:note", None),
                ("Picture…", "dialog:insertPicture", None),
                ("Text Box", "insert.textBox", None),
                ("Link…", "dialog:insertLink", None),
                ("Checkbox", "insert.checkbox", None),
            ],
        ),
        (
            "Format",
            vec![
                ("Cells…", "dialog:formatCells", None),
                ("Row Height…", "dialog:rowHeight", None),
                ("AutoFit Row Height", "home.autofitRowHeight", None),
                ("Column Width…", "dialog:columnWidth", None),
                ("AutoFit Column Width", "home.autofitColumnWidth", None),
                ("-", "", None),
                ("Hide Rows", "home.hideRows", None),
                ("Unhide Rows", "home.unhideRows", None),
                ("Hide Columns", "home.hideColumns", None),
                ("Unhide Columns", "home.unhideColumns", None),
                ("-", "", None),
                ("Rename Sheet…", "dialog:renameSheet", None),
                ("Hide Sheet", "sheet.hide", None),
                ("Unhide Sheet…", "sheet.unhide", None),
                ("-", "", None),
                ("Conditional Formatting…", "dialog:manageRules", None),
                ("Format as Table", "home.formatAsTable", None),
            ],
        ),
        (
            "Tools",
            vec![
                ("Spelling…", "dialog:spelling", None),
                ("Workbook Statistics", "dialog:statistics", None),
                ("Check Accessibility", "dialog:accessibility", None),
                ("-", "", None),
                ("Protect Sheet…", "dialog:protectSheet", None),
                ("Protect Workbook", "review.protectWorkbook", None),
                ("-", "", None),
                ("Goal Seek…", "dialog:goalSeek", None),
                ("Error Checking…", "dialog:errorChecking", None),
                ("Evaluate Formula…", "dialog:evaluateFormula", None),
                ("-", "", None),
                ("Record Actions", "automate.recordActions", None),
                ("Command Palette…", "dialog:commandSearch", Some("CMD+SHIFT+P")),
            ],
        ),
        (
            "Data",
            vec![
                ("Sort…", "dialog:sort", None),
                ("Sort A to Z", "data.sortAscending", None),
                ("Sort Z to A", "data.sortDescending", None),
                ("Filter", "data.filter", None),
                ("Clear Filter", "data.clearFilter", None),
                ("-", "", None),
                ("Text to Columns…", "dialog:textToColumns", None),
                ("Remove Duplicates…", "dialog:removeDuplicates", None),
                ("Data Validation…", "dialog:dataValidation", None),
                ("-", "", None),
                ("Group", "data.group", None),
                ("Ungroup", "data.ungroup", None),
                ("Show Detail", "data.showDetail", None),
                ("Hide Detail", "data.hideDetail", None),
                ("Subtotal…", "dialog:subtotal", None),
                ("-", "", None),
                ("Calculate Now", "formulas.calculateNow", None),
            ],
        ),
        (
            "Window",
            vec![
                ("Next Window", "window.next", Some(NEXT_WINDOW_ACCEL)),
                ("Previous Window", "window.previous", Some(PREVIOUS_WINDOW_ACCEL)),
                ("New Window", "view.newWindow", None),
                ("Next Sheet", "sheet.next", None),
                ("Previous Sheet", "sheet.previous", None),
            ],
        ),
        (
            "Help",
            vec![
                ("Agent Control (MCP)…", "dialog:agents", None),
                ("Contributors…", "dialog:about:Contributors", None),
                ("GridCraft on getartcraft.com", "url:https://getartcraft.com/apps/gridcraft", None),
                ("Join the ArtCraft Discord…", "url:https://discord.gg/artcraft", None),
            ],
        ),
    ]
}

pub struct NativeMenu {
    menu: Menu,
    events: Receiver<MenuEvent>,
    theme_items: Vec<(&'static str, CheckMenuItem)>,
    /// The Window submenu, its position in the menu bar, and the document items it ends with
    /// (uid, label, item), one per [`WindowEntry`].
    window_menu: Submenu,
    window_at: usize,
    window_items: Vec<(u64, String, CheckMenuItem)>,
}

/// The Window submenu: the static entries of [`tree`], then a separator and one check item per
/// listed document.
fn window_submenu(entries: &[WindowEntry]) -> (Submenu, Vec<(u64, String, CheckMenuItem)>) {
    let sub = Submenu::new("Window", true);
    for (label, id, acc) in tree().into_iter().find(|(t, _)| *t == "Window").map(|(_, e)| e).unwrap_or_default() {
        if label == "-" {
            let _ = sub.append(&PredefinedMenuItem::separator());
            continue;
        }
        let accel = acc.and_then(|a| Accelerator::from_str(a).ok());
        let _ = sub.append(&MenuItem::with_id(id, label, true, accel));
    }
    let mut items = Vec::with_capacity(entries.len());
    if !entries.is_empty() {
        let _ = sub.append(&PredefinedMenuItem::separator());
    }
    for w in entries {
        let item = CheckMenuItem::with_id(window_item_id(w.uid), &w.title, true, w.active, None);
        if sub.append(&item).is_ok() {
            items.push((w.uid, w.title.clone(), item));
        }
    }
    (sub, items)
}

/// Keeps the app's own window out of AppKit's list for the windows menu: the documents are
/// listed by [`NativeMenu::sync_windows`] instead, and the one egui window would show once more,
/// titled after the active document and always checked.
fn exclude_app_windows_from_windows_menu() {
    use objc2_app_kit::NSApplication;
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    for window in [app.mainWindow(), app.keyWindow()].into_iter().flatten() {
        if !window.isExcludedFromWindowsMenu() {
            window.setExcludedFromWindowsMenu(true);
        }
    }
}

fn select_theme(app: &mut SheetApp, id: &str) -> bool {
    let mode = match id {
        "theme:system" => "system",
        "theme:light" => "light",
        "theme:dark" => "dark",
        _ => return false,
    };
    app.run_or_alert("view.theme", json!({"mode": mode}));
    true
}

impl NativeMenu {
    pub fn install(ctx: &egui::Context) -> NativeMenu {
        let menu = Menu::new();
        let app_menu = Submenu::new("GridCraft", true);
        let about = MenuItem::with_id("dialog:about", "About GridCraft", true, None);
        let _ = app_menu.append_items(&[
            &about,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(None),
        ]);
        let _ = menu.append(&app_menu);
        let mut theme_items = Vec::new();
        let (window_menu, _) = window_submenu(&[]);
        let mut window_at = 0;
        for (title, entries) in tree() {
            if title == "Window" {
                window_at = menu.items().len();
                let _ = menu.append(&window_menu);
                continue;
            }
            let sub = Submenu::new(title, true);
            for (label, id, acc) in entries {
                if label == "-" {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                    continue;
                }
                if id == "view.theme" {
                    let themes = Submenu::new(label, true);
                    for (mode, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                        let item = CheckMenuItem::with_id(format!("theme:{mode}"), label, true, false, None);
                        let _ = themes.append(&item);
                        theme_items.push((mode, item));
                    }
                    let _ = sub.append(&themes);
                    continue;
                }
                let accel = acc.and_then(|a| Accelerator::from_str(a).ok());
                let item = MenuItem::with_id(id, label, true, accel);
                let _ = sub.append(&item);
            }
            let _ = menu.append(&sub);
        }
        menu.init_for_nsapp();
        // NSApp's windowsMenu is where macOS adds Move & Resize, Fill and Center, the targets of the
        // system tiling shortcuts (Fn+Ctrl+arrows, Fn+F). Only once the menu is NSApp's: muda looks
        // the submenu up in the installed main menu.
        window_menu.set_as_windows_menu_for_nsapp();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));
        NativeMenu { menu, events: rx, theme_items, window_menu, window_at, window_items: Vec::new() }
    }

    /// The (uid, label) pairs the Window menu lists, in order: what the last sync built. Read
    /// back by `tests/mac_menu_appkit.rs` against AppKit's view of the menu.
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn listed_windows(&self) -> Vec<(u64, String)> {
        self.window_items.iter().map(|(uid, title, _)| (*uid, title.clone())).collect()
    }

    /// Brings the Window menu's document list up to date with the session. Titles and check
    /// marks change in place; a changed set of documents replaces the whole submenu, never on an
    /// unchanged frame.
    fn sync_windows(&mut self, session: &Session) {
        let wanted = window_entries(session);
        let same_set = wanted.len() == self.window_items.len() && wanted.iter().zip(&self.window_items).all(|(w, (uid, _, _))| w.uid == *uid);
        if same_set {
            // The same documents: titles (a document saved or edited) and check marks change in
            // place, which needs no positions at all.
            for (w, (_, title, item)) in wanted.iter().zip(self.window_items.iter_mut()) {
                if *title != w.title {
                    item.set_text(&w.title);
                    *title = w.title.clone();
                }
                if item.is_checked() != w.active {
                    item.set_checked(w.active);
                }
            }
            return;
        }
        let (fresh, items) = window_submenu(&wanted);
        if self.menu.remove(&self.window_menu).is_err() {
            return;
        }
        if self.menu.insert(&fresh, self.window_at).is_err() {
            // Keep the menu the user has rather than none; the next change tries again.
            let _ = self.menu.insert(&self.window_menu, self.window_at);
            return;
        }
        fresh.set_as_windows_menu_for_nsapp();
        self.window_menu = fresh;
        self.window_items = items;
    }

    pub fn poll(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        exclude_app_windows_from_windows_menu();
        while let Ok(ev) = self.events.try_recv() {
            let id = ev.id.as_ref().to_string();
            if let Some(uid) = window_item_uid(&id) {
                // A click on a document closed since the menu was drawn is nothing to report.
                if app.session.document_index(uid).is_some() {
                    app.run_or_alert("window.activate", json!({"id": uid}));
                }
                continue;
            }
            // Text selection belongs to the inline editor, not the worksheet clipboard/undo.
            if app.text_box_editor.is_some() {
                let clipboard = match id.as_str() {
                    "edit.copy" => Some(egui::ViewportCommand::RequestCopy),
                    "edit.cut" => Some(egui::ViewportCommand::RequestCut),
                    "edit.paste" => Some(egui::ViewportCommand::RequestPaste),
                    _ => None,
                };
                if let Some(command) = clipboard {
                    ctx.send_viewport_cmd(command);
                    continue;
                }
                if id == "edit.undo" || id == "edit.redo" {
                    ctx.input_mut(|i| {
                        i.events.push(egui::Event::Key {
                            key: egui::Key::Z,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers { command: true, mac_cmd: true, shift: id == "edit.redo", ..Default::default() },
                        });
                    });
                    continue;
                }
            }
            if select_theme(app, &id) {
                continue;
            }
            if let Some(url) = id.strip_prefix("url:") {
                if let Some(open) = &app.services.open_url {
                    open(url);
                }
            } else if let Some(rest) = id.strip_prefix("dialog:") {
                let (name, arg) = rest.split_once(':').unwrap_or((rest, ""));
                let params: Value = match (name, arg) {
                    ("saveCopy", fmt) => json!({"format": fmt}),
                    ("find", "replace") => json!({"replace": true}),
                    ("note", _) => json!({"threaded": false}),
                    ("comment", _) => json!({"threaded": true}),
                    ("about", tab) => json!({"tab": tab}),
                    ("backstage", page) => json!({"page": page}),
                    _ => json!({}),
                };
                let name = if name == "note" { "comment" } else { name };
                app.open_dialog(name, params);
            } else if id == "edit.copy" || id == "edit.cut" {
                app.copy_to_clipboard(ctx, &id);
            } else if id == "edit.paste" {
                app.paste_from_clipboard();
            } else {
                app.run_or_alert(&id, json!({}));
            }
        }
        for (mode, item) in &self.theme_items {
            let checked = app.ui.theme_mode() == *mode;
            if item.is_checked() != checked {
                item.set_checked(checked);
            }
        }
        self.sync_windows(&app.session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_menu_has_export_as_pdf_dispatching_the_command() {
        let tree = tree();
        let (_, file) = tree.iter().find(|(title, _)| *title == "File").expect("File menu");
        let item = file.iter().find(|e| e.0 == "Export as PDF…").expect("Export as PDF… in the File menu");
        assert_eq!(item.1, "file.exportPdf");
        assert!(gridcraft_engine::find_command("file.exportPdf").is_some());
    }

    #[test]
    fn theme_choices_use_the_shared_preference_command() {
        let mut app = SheetApp::new(gridcraft_engine::Session::new(), Default::default());
        for mode in ["dark", "system", "light"] {
            assert!(select_theme(&mut app, &format!("theme:{mode}")));
            assert_eq!(app.ui.theme_mode(), mode);
            assert!(app.message.is_none());
        }
        assert!(!select_theme(&mut app, "view.formulaBar"));
        assert_eq!(app.ui.theme_mode(), "light");
    }

    #[test]
    fn window_entries_list_visible_documents_and_mark_the_active_one() {
        let mut app = SheetApp::new(gridcraft_engine::Session::new(), Default::default());
        assert!(window_entries(&app.session).is_empty());
        app.session.new_workbook();
        app.session.new_workbook();
        let uids: Vec<u64> = app.session.documents().iter().map(|d| d.uid).collect();
        assert_eq!(uids.len(), 2);
        assert_ne!(uids[0], uids[1]);

        let entries = window_entries(&app.session);
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].uid, entries[0].title.as_str(), entries[0].active), (uids[0], "Book1", false));
        assert_eq!((entries[1].uid, entries[1].title.as_str(), entries[1].active), (uids[1], "Book2", true));

        app.session.set_active(0);
        let active: Vec<bool> = window_entries(&app.session).iter().map(|e| e.active).collect();
        assert_eq!(active, [true, false]);

        // A dirty document carries the title bar's marking.
        app.run_or_alert("cell.set", json!({"cell": "A1", "input": "1"}));
        assert!(app.message.is_none());
        assert_eq!(window_entries(&app.session)[0].title, "Book1 — Edited");

        // A hidden window is left out; its uid is what hides it, not its index.
        app.session.hidden_windows.push(uids[1]);
        let entries = window_entries(&app.session);
        assert_eq!(entries.iter().map(|e| e.uid).collect::<Vec<_>>(), [uids[0]]);

        // Menu ids round-trip to the uid; anything else is not a window item.
        for uid in uids {
            assert_eq!(window_item_uid(&window_item_id(uid)), Some(uid));
        }
        assert_eq!(window_item_id(7), "window:7");
        assert_eq!(window_item_uid("window:"), None);
        assert_eq!(window_item_uid("window:Book1"), None);
        assert_eq!(window_item_uid("view.newWindow"), None);
    }

    #[test]
    fn window_menu_cycles_windows_with_the_system_shortcut() {
        let tree = tree();
        let (_, window) = tree.iter().find(|(title, _)| *title == "Window").expect("Window menu");
        let labels: Vec<&str> = window.iter().map(|e| e.0).collect();
        assert_eq!(labels, ["Next Window", "Previous Window", "New Window", "Next Sheet", "Previous Sheet"]);
        for (label, command, accel) in [
            ("Next Window", "window.next", Some(NEXT_WINDOW_ACCEL)),
            ("Previous Window", "window.previous", Some(PREVIOUS_WINDOW_ACCEL)),
            ("New Window", "view.newWindow", None),
        ] {
            let item = window.iter().find(|e| e.0 == label).expect(label);
            assert_eq!((item.1, item.2), (command, accel), "{label}");
            assert!(gridcraft_engine::find_command(command).is_some(), "{command} is a command");
        }
    }

    #[test]
    fn window_cycle_accelerators_parse_as_cmd_backquote() {
        use muda::accelerator::{Code, Modifiers};
        // Which spellings muda accepts for the key (the constants use the first that parses).
        let parsed: Vec<&str> = ["CMD+`", "CMD+Backquote", "CMD+Grave"].into_iter().filter(|s| Accelerator::from_str(s).is_ok()).collect();
        assert!(parsed.contains(&NEXT_WINDOW_ACCEL), "accepted spellings: {parsed:?}");
        assert_eq!(Accelerator::from_str(NEXT_WINDOW_ACCEL).ok(), Some(Accelerator::new(Modifiers::META, Code::Backquote)));
        assert_eq!(Accelerator::from_str(PREVIOUS_WINDOW_ACCEL).ok(), Some(Accelerator::new(Modifiers::META | Modifiers::SHIFT, Code::Backquote)));
    }
}
