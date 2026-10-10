//! macOS: the native menu bar (File, Edit, View, Insert, Format, Tools, Data, Window, Help), as
//! a desktop spreadsheet has on the Mac. Items run the same commands as the ribbon.
//!
//! The menus and their labels are described in `menu_model` and rendered in the interface language
//! of the session; they are rebuilt whenever that language changes (`app.setLocale`, Options ›
//! Language, a changed system language at start).

use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use gridcraft_l10n::Localizer;
use gridcraft_ui_egui::SheetApp;
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::{Value, json};

use crate::menu_model::{self, ResolvedEntry};

pub struct NativeMenu {
    /// The installed menu bar; replaced as a whole when the language changes.
    _menu: Menu,
    /// Interface language the menu bar was built for.
    tag: &'static str,
    events: Receiver<MenuEvent>,
    theme_items: Vec<(&'static str, CheckMenuItem)>,
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

/// Builds the menu bar for `loc` and makes it the application's main menu. Also returns the check
/// items of the Display Theme submenu, which `NativeMenu::poll` keeps in step with the preference.
fn build(loc: &Localizer) -> (Menu, Vec<(&'static str, CheckMenuItem)>) {
    let menu = Menu::new();
    let app_menu = Submenu::new("GridCraft", true);
    let about = MenuItem::with_id(menu_model::ABOUT_ACTION, loc.plain(menu_model::ABOUT_KEY), true, None);
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
    for m in menu_model::resolve(loc) {
        let sub = Submenu::new(m.title, true);
        for entry in m.entries {
            match entry {
                ResolvedEntry::Separator => {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                }
                ResolvedEntry::Item(item) if item.action == menu_model::THEME_ACTION => {
                    let themes = Submenu::new(item.label, true);
                    for (mode, key) in menu_model::THEME_CHOICES {
                        let choice = CheckMenuItem::with_id(format!("theme:{mode}"), loc.plain(key), true, false, None);
                        let _ = themes.append(&choice);
                        theme_items.push((mode, choice));
                    }
                    let _ = sub.append(&themes);
                }
                ResolvedEntry::Item(item) => {
                    let accel = item.accel.and_then(|a| Accelerator::from_str(a).ok());
                    let _ = sub.append(&MenuItem::with_id(item.action, item.label, true, accel));
                }
            }
        }
        let _ = menu.append(&sub);
    }
    menu.init_for_nsapp();
    (menu, theme_items)
}

impl NativeMenu {
    /// Installs the menu bar for the interface language of `app`'s session.
    pub fn install(ctx: &egui::Context, app: &SheetApp) -> NativeMenu {
        let loc = Localizer::new(app.session.locale().ui.tag);
        let (menu, theme_items) = build(&loc);
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));
        NativeMenu { _menu: menu, tag: loc.tag(), events: rx, theme_items }
    }

    /// Rebuilds the menu bar when the interface language changed, then runs the chosen items.
    pub fn poll(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        let loc = Localizer::new(app.session.locale().ui.tag);
        if loc.tag() != self.tag {
            (self._menu, self.theme_items) = build(&loc);
            self.tag = loc.tag();
        }
        while let Ok(ev) = self.events.try_recv() {
            let id = ev.id.as_ref().to_string();
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
                    _ => json!({}),
                };
                let name = if name == "note" { "comment" } else { name };
                app.open_dialog(name, params);
            } else if id == "edit.copy" || id == "edit.cut" {
                app.copy_to_clipboard(ctx, &id);
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
