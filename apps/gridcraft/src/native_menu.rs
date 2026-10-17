//! macOS: the native menu bar (File, Edit, View, Insert, Format, Tools, Data, Window, Help), as
//! a desktop spreadsheet has on the Mac. Items run the same commands as the ribbon.

use std::str::FromStr;
use std::sync::mpsc::{Receiver, channel};

use gridcraft_ui_egui::{SheetApp, i18n::Language};
use muda::accelerator::Accelerator;
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use serde_json::{Value, json};

type Entry = (&'static str, &'static str, Option<&'static str>);

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
                ("Format as Table", "insert.table", None),
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
        ("Window", vec![("New Window", "view.newWindow", None), ("Next Sheet", "sheet.next", None), ("Previous Sheet", "sheet.previous", None)]),
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

enum Label {
    Item(MenuItem),
    Submenu(Submenu),
    Predefined(PredefinedMenuItem),
}

impl Label {
    fn set_text(&self, text: &str) {
        match self {
            Self::Item(item) => item.set_text(text),
            Self::Submenu(menu) => menu.set_text(text),
            Self::Predefined(item) => item.set_text(text),
        }
    }
}

pub struct NativeMenu {
    _menu: Menu,
    language: Language,
    labels: Vec<(&'static str, Label)>,
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

impl NativeMenu {
    pub fn install(ctx: &egui::Context, language: Language) -> NativeMenu {
        let menu = Menu::new();
        let app_menu = Submenu::new("GridCraft", true);
        let about = MenuItem::with_id("dialog:about", language.tr("About GridCraft"), true, None);
        let mut labels = vec![("About GridCraft", Label::Item(about.clone()))];
        let predefined = [
            ("Services", PredefinedMenuItem::services(Some(language.tr("Services")))),
            ("Hide GridCraft", PredefinedMenuItem::hide(Some(language.tr("Hide GridCraft")))),
            ("Hide Others", PredefinedMenuItem::hide_others(Some(language.tr("Hide Others")))),
            ("Show All", PredefinedMenuItem::show_all(Some(language.tr("Show All")))),
            ("Quit GridCraft", PredefinedMenuItem::quit(Some(language.tr("Quit GridCraft")))),
        ];
        let _ = app_menu.append_items(&[
            &about,
            &PredefinedMenuItem::separator(),
            &predefined[0].1,
            &PredefinedMenuItem::separator(),
            &predefined[1].1,
            &predefined[2].1,
            &predefined[3].1,
            &PredefinedMenuItem::separator(),
            &predefined[4].1,
        ]);
        labels.extend(predefined.into_iter().map(|(key, item)| (key, Label::Predefined(item))));
        let _ = menu.append(&app_menu);
        let mut theme_items = Vec::new();
        let mut window_menu = None;
        for (title, entries) in tree() {
            let sub = Submenu::new(language.tr(title), true);
            labels.push((title, Label::Submenu(sub.clone())));
            for (label, id, acc) in entries {
                if label == "-" {
                    let _ = sub.append(&PredefinedMenuItem::separator());
                    continue;
                }
                if id == "view.theme" {
                    let themes = Submenu::new(language.tr(label), true);
                    labels.push((label, Label::Submenu(themes.clone())));
                    for (mode, label) in [("system", "System"), ("light", "Light"), ("dark", "Dark")] {
                        let item = CheckMenuItem::with_id(format!("theme:{mode}"), language.tr(label), true, false, None);
                        let _ = themes.append(&item);
                        theme_items.push((mode, item));
                    }
                    let _ = sub.append(&themes);
                    continue;
                }
                let accel = acc.and_then(|a| Accelerator::from_str(a).ok());
                let item = MenuItem::with_id(id, language.tr(label), true, accel);
                labels.push((label, Label::Item(item.clone())));
                let _ = sub.append(&item);
            }
            let _ = menu.append(&sub);
            if title == "Window" {
                window_menu = Some(sub);
            }
        }
        menu.init_for_nsapp();
        // NSApp's windowsMenu is where macOS adds Move & Resize, Fill and Center, the targets of the
        // system tiling shortcuts (Fn+Ctrl+arrows, Fn+F). Only once the menu is NSApp's: muda looks
        // the submenu up in the installed main menu.
        if let Some(window) = window_menu {
            window.set_as_windows_menu_for_nsapp();
        }
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let _ = tx.send(e);
            ctx.request_repaint();
        }));
        NativeMenu { _menu: menu, language, labels, events: rx, theme_items }
    }

    pub fn poll(&mut self, app: &mut SheetApp, ctx: &egui::Context) {
        if self.language != app.ui.language {
            self.language = app.ui.language;
            for (key, label) in &self.labels {
                label.set_text(self.language.tr(key));
            }
            for ((_, item), key) in self.theme_items.iter().zip(["System", "Light", "Dark"]) {
                item.set_text(self.language.tr(key));
            }
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
    fn french_catalog_covers_every_native_menu_entry() {
        for (title, entries) in tree() {
            for key in std::iter::once(title).chain(entries.iter().map(|entry| entry.0)) {
                if !matches!(key, "-" | "Format" | "Normal" | "Zoom…") {
                    assert_ne!(Language::Fr.tr(key), key, "missing French menu label: {key}");
                }
            }
        }
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
}
