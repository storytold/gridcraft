//! macOS: the native menu bar (File, Edit, View, Insert, Format, Tools, Data, Window, Help), as
//! a desktop spreadsheet has on the Mac. Items run the same commands as the ribbon.

use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::{OpenWindow, SheetApp};
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::{Value, json};

#[path = "native_menu_tree.rs"]
mod menu_tree;
use menu_tree::{APP_ITEMS, THEMES, tree};

/// A menu bar element whose text follows the interface language.
enum Labelled {
    Submenu(Submenu),
    Item(MenuItem),
    Check(CheckMenuItem),
}

impl Labelled {
    fn set_text(&self, text: &str) {
        match self {
            Self::Submenu(menu) => menu.set_text(text),
            Self::Item(item) => item.set_text(text),
            Self::Check(item) => item.set_text(text),
        }
    }
}

pub struct NativeMenu {
    _menu: Menu,
    events: Receiver<MenuEvent>,
    theme_items: Vec<(&'static str, CheckMenuItem)>,
    /// The language the bar is labelled in, and every labelled title or item with its English key.
    language: Language,
    labelled: Vec<(&'static str, Labelled)>,
    window_menu: Option<Submenu>,
    /// The Window menu's window items and what they last showed.
    window_items: Vec<CheckMenuItem>,
    windows: Vec<OpenWindow>,
}

/// Menu id of the Window menu item that brings workbook `doc` forward.
const WINDOW_ITEM: &str = "window:id:";

/// The Quit item: asks about each unsaved workbook in its window first
/// (`SheetApp::request_quit`), which AppKit's own Quit (terminate:) would skip.
const QUIT: &str = "app.quit";

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
    /// Installs the bar labelled in `language` (the interface language at launch).
    pub fn install(ctx: &egui::Context, language: Language) -> NativeMenu {
        let [about_label, quit_label] = APP_ITEMS;
        let menu = Menu::new();
        let app_menu = Submenu::new("GridCraft", true);
        let about = MenuItem::with_id("dialog:about", language.tr(about_label), true, None);
        let quit = MenuItem::with_id(QUIT, language.tr(quit_label), true, Accelerator::from_str("CMD+Q").ok());
        let _ = app_menu.append_items(&[
            &about,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &quit,
        ]);
        let mut labelled = vec![(about_label, Labelled::Item(about)), (quit_label, Labelled::Item(quit))];
        let _ = menu.append(&app_menu);
        let mut theme_items = Vec::new();
        let mut window_menu = None;
        for (title, entries) in tree() {
            let sub = Submenu::new(language.tr(title), true);
            for (label, id, acc) in entries {
                if label == "-" {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                    continue;
                }
                if id == "view.theme" {
                    let themes = Submenu::new(language.tr(label), true);
                    for (mode, theme_label) in THEMES {
                        let item = CheckMenuItem::with_id(format!("theme:{mode}"), language.tr(theme_label), true, false, None);
                        let _ = themes.append(&item);
                        theme_items.push((mode, item.clone()));
                        labelled.push((theme_label, Labelled::Check(item)));
                    }
                    let _ = sub.append(&themes);
                    labelled.push((label, Labelled::Submenu(themes)));
                    continue;
                }
                let accel = acc.and_then(|a| Accelerator::from_str(a).ok());
                let item = MenuItem::with_id(id, language.tr(label), true, accel);
                let _ = sub.append(&item);
                labelled.push((label, Labelled::Item(item)));
            }
            let _ = menu.append(&sub);
            if title == "Window" {
                window_menu = Some(sub.clone());
            }
            labelled.push((title, Labelled::Submenu(sub)));
        }
        menu.init_for_nsapp();
        // NSApp's windowsMenu is where macOS adds Move & Resize, Fill and Center, the targets of the
        // system tiling shortcuts (Fn+Ctrl+arrows, Fn+F). Only once the menu is NSApp's: muda looks
        // the submenu up in the installed main menu.
        if let Some(window) = &window_menu {
            window.set_as_windows_menu_for_nsapp();
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));
        NativeMenu { _menu: menu, events: rx, theme_items, language, labelled, window_menu, window_items: Vec::new(), windows: Vec::new() }
    }

    /// Relabels the bar when the interface language changes (View > Language, `app.language.set`).
    fn sync_language(&mut self, language: Language) {
        if language == self.language {
            return;
        }
        self.language = language;
        for (english, element) in &self.labelled {
            element.set_text(language.tr(english));
        }
    }

    /// Keeps the Window menu's list of open windows (title, unsaved bullet, active check) in
    /// step with the session.
    fn sync_windows(&mut self, app: &SheetApp) {
        let windows = app.open_windows();
        if windows != self.windows
            && let Some(menu) = &self.window_menu
        {
            let same_windows = windows.len() == self.windows.len() && windows.iter().zip(&self.windows).all(|(a, b)| a.window == b.window);
            if same_windows {
                for (item, w) in self.window_items.iter().zip(&windows) {
                    item.set_text(w.label());
                }
            } else {
                for item in self.window_items.drain(..) {
                    let _ = menu.remove(&item);
                }
                for w in &windows {
                    let item = CheckMenuItem::with_id(format!("{WINDOW_ITEM}{}", w.window), w.label(), true, w.active, None);
                    let _ = menu.append(&item);
                    self.window_items.push(item);
                }
            }
            self.windows = windows;
        }
        // AppKit flips a check item when it is chosen, so the active check is put back every poll.
        for (item, w) in self.window_items.iter().zip(&self.windows) {
            if item.is_checked() != w.active {
                item.set_checked(w.active);
            }
        }
    }

    /// Runs the chosen menu items. Commands go to the active window, the most recently focused;
    /// `viewport` is its egui viewport, for the items that act on its input.
    pub fn poll(&mut self, app: &mut SheetApp, ctx: &egui::Context, viewport: egui::ViewportId) {
        while let Ok(ev) = self.events.try_recv() {
            let id = ev.id.as_ref().to_string();
            if id == QUIT {
                app.request_quit(ctx);
                continue;
            }
            if let Some(window) = id.strip_prefix(WINDOW_ITEM).and_then(|w| w.parse::<u64>().ok()) {
                app.run_or_alert("view.switchWindows", json!({"window": window}));
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
                    ctx.send_viewport_cmd_to(viewport, command);
                    continue;
                }
                if id == "edit.undo" || id == "edit.redo" {
                    ctx.input_mut_for(viewport, |i| {
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
        self.sync_language(app.ui.language);
        self.sync_windows(app);
    }
}

#[cfg(test)]
mod tests {
    // No `use super::*`: tests/mac_menu_appkit.rs builds this file with cfg(test) but no test
    // harness, so the #[test] functions are left out and a module-level import would be unused.

    #[test]
    fn file_menu_has_export_as_pdf_dispatching_the_command() {
        let tree = super::tree();
        let (_, file) = tree.iter().find(|(title, _)| *title == "File").expect("File menu");
        let item = file.iter().find(|e| e.0 == "Export as PDF…").expect("Export as PDF… in the File menu");
        assert_eq!(item.1, "file.exportPdf");
        assert!(gridcraft_engine::find_command("file.exportPdf").is_some());
    }

    #[test]
    fn help_menu_checks_for_updates_through_the_registered_command() {
        let tree = super::tree();
        let (_, help) = tree.iter().find(|(title, _)| *title == "Help").expect("Help menu");
        let item = help.iter().find(|e| e.0 == "Check for Updates…").expect("Check for Updates… in the Help menu");
        assert_eq!(item.1, "app.checkForUpdates");
        let mut app = super::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
        assert!(app.run(item.1, serde_json::json!({})).is_ok());
    }

    #[test]
    fn theme_choices_use_the_shared_preference_command() {
        let mut app = super::SheetApp::new(gridcraft_engine::Session::new(), Default::default());
        for mode in ["dark", "system", "light"] {
            assert!(super::select_theme(&mut app, &format!("theme:{mode}")));
            assert_eq!(app.ui.theme_mode(), mode);
            assert!(app.message.is_none());
        }
        assert!(!super::select_theme(&mut app, "view.formulaBar"));
        assert_eq!(app.ui.theme_mode(), "light");
    }
}
