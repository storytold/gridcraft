//! The native menu bar as data (File, Edit, View, Insert, Format, Tools, Data, Window, Help) and its
//! resolution into labels for one language. Only the macOS shell (`native_menu.rs`) builds real
//! menus from it; the model is plain data so it is tested on every platform.
//!
//! Labels come from the localizer: a command's own label (`commands.ftl`), a ribbon tab name
//! (`ribbon.ftl`) or, for the words that are neither, a `menu-*` message in `menu.ftl`.

use gridcraft_l10n::Localizer;

/// Where the text of an item comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Label {
    /// The label of this engine command (`commands.ftl`); it may differ from the item's action.
    Command(&'static str),
    /// A message of `menu.ftl`.
    Key(&'static str),
}

/// Where the text of a menu title comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Title {
    /// The ribbon tab with this English name (`ribbon.ftl`).
    Tab(&'static str),
    /// A message of `menu.ftl`.
    Key(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Separator,
    Item {
        label: Label,
        /// An engine command id, `dialog:<name>[:<arg>]` or `url:<address>`.
        action: &'static str,
        accel: Option<&'static str>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub title: Title,
    pub entries: Vec<Entry>,
}

/// An item that is labelled by its own command.
const fn cmd(id: &'static str, accel: Option<&'static str>) -> Entry {
    Entry::Item { label: Label::Command(id), action: id, accel }
}

/// An item whose action is `action` but whose text is the label of command `label`.
const fn like(label: &'static str, action: &'static str, accel: Option<&'static str>) -> Entry {
    Entry::Item { label: Label::Command(label), action, accel }
}

/// An item with its own `menu.ftl` text.
const fn key(key: &'static str, action: &'static str, accel: Option<&'static str>) -> Entry {
    Entry::Item { label: Label::Key(key), action, accel }
}

const SEP: Entry = Entry::Separator;

/// The menus after the application menu, in display order.
pub fn tree() -> Vec<Menu> {
    let menu = |title: Title, entries: Vec<Entry>| Menu { title, entries };
    vec![
        menu(
            Title::Tab("File"),
            vec![
                cmd("file.new", Some("CMD+N")),
                key("menu-file-new-from-sample", "dialog:start", None),
                cmd("file.open", Some("CMD+O")),
                SEP,
                cmd("file.close", Some("CMD+W")),
                cmd("file.save", Some("CMD+S")),
                cmd("file.saveAs", Some("CMD+SHIFT+S")),
                key("menu-file-export-csv", "dialog:saveCopy:csv", None),
                key("menu-file-save-web-page", "dialog:saveCopy:html", None),
                SEP,
                like("pageLayout.pageSetup", "dialog:pageSetup", None),
                cmd("file.print", Some("CMD+P")),
                SEP,
                key("menu-file-properties", "file.properties", None),
                SEP,
                cmd("file.options", None),
            ],
        ),
        menu(
            Title::Tab("Edit"),
            vec![
                cmd("edit.undo", None),
                cmd("edit.redo", None),
                SEP,
                cmd("edit.cut", None),
                cmd("edit.copy", None),
                cmd("edit.paste", None),
                like("edit.pasteSpecial", "dialog:pasteSpecial", Some("CTRL+CMD+V")),
                SEP,
                cmd("edit.fillDown", None),
                cmd("edit.fillRight", None),
                like("edit.fillSeries", "dialog:series", None),
                cmd("edit.flashFill", None),
                SEP,
                cmd("edit.clearAll", None),
                cmd("edit.clearFormats", None),
                cmd("edit.clearContents", None),
                SEP,
                key("menu-edit-delete", "dialog:deleteCells", None),
                cmd("home.deleteSheet", None),
                SEP,
                like("edit.find", "dialog:find", None),
                like("edit.replace", "dialog:find:replace", Some("CTRL+H")),
                like("edit.goTo", "dialog:goTo", Some("CTRL+G")),
            ],
        ),
        menu(
            Title::Tab("View"),
            vec![
                cmd("view.normal", None),
                cmd("view.pageLayout", None),
                cmd("view.pageBreakPreview", None),
                SEP,
                key("menu-view-formula-bar", "view.formulaBar", None),
                cmd("view.gridlines", None),
                cmd("view.headings", None),
                SEP,
                key("menu-view-zoom", "dialog:zoom", None),
                cmd("view.zoomToSelection", None),
                cmd("view.freezePanes", None),
                cmd("view.freezeTopRow", None),
                cmd("view.unfreezePanes", None),
                SEP,
                key("menu-view-display-theme", THEME_ACTION, None),
                key("menu-view-collapse-ribbon", "view.collapseRibbon", Some("CMD+ALT+R")),
            ],
        ),
        menu(
            Title::Tab("Insert"),
            vec![
                key("menu-insert-cells", "dialog:insertCells", None),
                key("menu-insert-rows", "home.insertRows", None),
                key("menu-insert-columns", "home.insertColumns", None),
                key("menu-insert-sheet", "home.insertSheet", Some("SHIFT+F11")),
                SEP,
                key("menu-insert-chart", "insert.recommendedCharts", None),
                key("menu-insert-sparklines", "dialog:sparkline", None),
                cmd("insert.pivotTable", None),
                cmd("insert.table", None),
                SEP,
                key("menu-insert-function", "dialog:insertFunction", Some("SHIFT+F3")),
                key("menu-insert-name", "dialog:defineName", None),
                like("review.newComment", "dialog:comment", None),
                like("review.newNote", "dialog:note", None),
                key("menu-insert-picture", "dialog:insertPicture", None),
                cmd("insert.textBox", None),
                key("menu-insert-link", "dialog:insertLink", None),
                cmd("insert.checkbox", None),
            ],
        ),
        menu(
            Title::Tab("Format"),
            vec![
                key("menu-format-cells", "dialog:formatCells", None),
                like("home.rowHeight", "dialog:rowHeight", None),
                cmd("home.autofitRowHeight", None),
                like("home.columnWidth", "dialog:columnWidth", None),
                cmd("home.autofitColumnWidth", None),
                SEP,
                cmd("home.hideRows", None),
                cmd("home.unhideRows", None),
                cmd("home.hideColumns", None),
                cmd("home.unhideColumns", None),
                SEP,
                key("menu-format-rename-sheet", "dialog:renameSheet", None),
                cmd("sheet.hide", None),
                cmd("sheet.unhide", None),
                SEP,
                key("menu-format-conditional-formatting", "dialog:manageRules", None),
                like("home.formatAsTable", "insert.table", None),
            ],
        ),
        menu(
            Title::Key("menu-title-tools"),
            vec![
                key("menu-tools-spelling", "dialog:spelling", None),
                like("review.workbookStatistics", "dialog:statistics", None),
                like("review.checkAccessibility", "dialog:accessibility", None),
                SEP,
                like("review.protectSheet", "dialog:protectSheet", None),
                cmd("review.protectWorkbook", None),
                SEP,
                like("data.goalSeek", "dialog:goalSeek", None),
                key("menu-tools-error-checking", "dialog:errorChecking", None),
                key("menu-tools-evaluate-formula", "dialog:evaluateFormula", None),
                SEP,
                cmd("automate.recordActions", None),
                key("menu-tools-command-palette", "dialog:commandSearch", Some("CMD+SHIFT+P")),
            ],
        ),
        menu(
            Title::Tab("Data"),
            vec![
                like("data.sort", "dialog:sort", None),
                cmd("data.sortAscending", None),
                cmd("data.sortDescending", None),
                cmd("data.filter", None),
                key("menu-data-clear-filter", "data.clearFilter", None),
                SEP,
                key("menu-data-text-to-columns", "dialog:textToColumns", None),
                key("menu-data-remove-duplicates", "dialog:removeDuplicates", None),
                like("data.validation", "dialog:dataValidation", None),
                SEP,
                cmd("data.group", None),
                cmd("data.ungroup", None),
                key("menu-data-subtotal", "dialog:subtotal", None),
                SEP,
                cmd("formulas.calculateNow", None),
            ],
        ),
        menu(Title::Tab("Window"), vec![cmd("view.newWindow", None), cmd("sheet.next", None), cmd("sheet.previous", None)]),
        menu(
            Title::Key("menu-title-help"),
            vec![
                key("menu-help-agent-control", "dialog:agents", None),
                key("menu-help-contributors", "dialog:about:Contributors", None),
                key("menu-help-website", "url:https://getartcraft.com/apps/gridcraft", None),
                key("menu-help-discord", "url:https://discord.gg/artcraft", None),
            ],
        ),
    ]
}

/// A menu item with its text in one language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedItem {
    pub label: String,
    pub action: &'static str,
    pub accel: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedEntry {
    Separator,
    Item(ResolvedItem),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedMenu {
    pub title: String,
    pub entries: Vec<ResolvedEntry>,
}

/// Text of the "About" item in the application menu.
pub const ABOUT_KEY: &str = "menu-app-about";
/// Action of the "About" item.
pub const ABOUT_ACTION: &str = "dialog:about";

/// Action of the "Display Theme" item, which holds a submenu with [`THEME_CHOICES`] instead of
/// running a command itself.
pub const THEME_ACTION: &str = "view.theme";
/// The Display Theme choices: the `mode` of `view.theme` and the `menu.ftl` message that names it.
pub const THEME_CHOICES: [(&str, &str); 3] = [("system", "menu-theme-system"), ("light", "menu-theme-light"), ("dark", "menu-theme-dark")];

fn label_text(loc: &Localizer, label: Label) -> String {
    match label {
        Label::Command(id) => loc.command_label(id).map_or_else(|| id.to_string(), |t| t.into_owned()),
        Label::Key(k) => loc.plain(k).into_owned(),
    }
}

fn title_text(loc: &Localizer, title: Title) -> String {
    match title {
        Title::Tab(english) => loc.ribbon_tab(english).map_or_else(|| english.to_string(), |t| t.into_owned()),
        Title::Key(k) => loc.plain(k).into_owned(),
    }
}

/// The menu bar in the language of `loc`.
pub fn resolve(loc: &Localizer) -> Vec<ResolvedMenu> {
    tree()
        .into_iter()
        .map(|m| ResolvedMenu {
            title: title_text(loc, m.title),
            entries: m
                .entries
                .into_iter()
                .map(|e| match e {
                    Entry::Separator => ResolvedEntry::Separator,
                    Entry::Item { label, action, accel } => ResolvedEntry::Item(ResolvedItem { label: label_text(loc, label), action, accel }),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [&str; 2] = ["en-US", "pt-BR"];

    fn items() -> Vec<(Label, &'static str)> {
        tree()
            .into_iter()
            .flat_map(|m| m.entries)
            .filter_map(|e| match e {
                Entry::Item { label, action, .. } => Some((label, action)),
                Entry::Separator => None,
            })
            .collect()
    }

    #[test]
    fn every_label_exists_in_the_reference_language() {
        let en = Localizer::new("en-US");
        for (label, action) in items() {
            match label {
                Label::Command(id) => assert!(en.command_label(id).is_some(), "{action}: no label for command {id}"),
                Label::Key(k) => assert!(en.get(k, &[]).is_some_and(|t| !t.is_empty()), "{action}: menu.ftl has no `{k}`"),
            }
        }
        for m in tree() {
            match m.title {
                Title::Tab(t) => assert!(en.ribbon_tab(t).is_some(), "no ribbon tab `{t}`"),
                Title::Key(k) => assert!(en.get(k, &[]).is_some(), "menu.ftl has no `{k}`"),
            }
        }
        assert!(en.get(ABOUT_KEY, &[]).is_some());
        for (mode, k) in THEME_CHOICES {
            assert!(en.get(k, &[]).is_some_and(|t| !t.is_empty()), "menu.ftl has no `{k}` for the {mode} theme");
        }
    }

    /// Commands the UI shell itself runs (`SheetApp::run_ui_command`): view toggles that are not
    /// saved in the workbook.
    const UI_COMMANDS: [&str; 3] = ["view.formulaBar", "view.collapseRibbon", THEME_ACTION];

    #[test]
    fn command_actions_are_engine_or_ui_commands() {
        for (_, action) in items() {
            if !action.starts_with("dialog:") && !action.starts_with("url:") && !UI_COMMANDS.contains(&action) {
                assert!(gridcraft_engine::find_command(action).is_some(), "`{action}` is not an engine command");
            }
        }
        for (label, _) in items() {
            if let Label::Command(id) = label {
                assert!(gridcraft_engine::find_command(id).is_some(), "label command `{id}` is not an engine command");
            }
        }
    }

    #[test]
    fn menus_are_not_empty_and_separators_are_not_doubled() {
        for tag in LANGS {
            for m in resolve(&Localizer::new(tag)) {
                assert!(!m.title.is_empty(), "{tag}: empty menu title");
                assert!(!matches!(m.entries.first(), Some(ResolvedEntry::Separator)), "{tag}/{}: leading separator", m.title);
                assert!(!matches!(m.entries.last(), Some(ResolvedEntry::Separator)), "{tag}/{}: trailing separator", m.title);
                for pair in m.entries.windows(2) {
                    assert!(!matches!(pair, [ResolvedEntry::Separator, ResolvedEntry::Separator]), "{tag}/{}: doubled separator", m.title);
                }
                for e in &m.entries {
                    if let ResolvedEntry::Item(i) = e {
                        assert!(!i.label.is_empty(), "{tag}/{}: empty label for {}", m.title, i.action);
                    }
                }
            }
        }
    }

    #[test]
    fn portuguese_menu_is_translated() {
        let en = resolve(&Localizer::new("en-US"));
        let pt = resolve(&Localizer::new("pt-BR"));
        assert_eq!(en.len(), pt.len());
        assert_ne!(en.first().map(|m| &m.title), pt.first().map(|m| &m.title), "the File menu keeps its English title");
        let differing = en.iter().zip(&pt).filter(|(a, b)| a.title != b.title).count();
        assert!(differing >= 5, "only {differing} menu titles differ between en-US and pt-BR");
        let item = |menus: &[ResolvedMenu], action: &str| {
            menus.iter().flat_map(|m| &m.entries).find_map(|e| match e {
                ResolvedEntry::Item(i) if i.action == action => Some(i.label.clone()),
                _ => None,
            })
        };
        assert_eq!(item(&en, "file.save").as_deref(), Some("Save"));
        assert_ne!(item(&pt, "file.save"), item(&en, "file.save"));
        assert_ne!(item(&pt, "dialog:start"), item(&en, "dialog:start"));
    }
}
