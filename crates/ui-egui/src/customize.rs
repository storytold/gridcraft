//! Customize the Ribbon and the Quick Access Toolbar (File › Options in Excel; right-click on the
//! ribbon or the toolbar's … menu here). The user's layout is a [`RibbonLayout`] kept in
//! [`crate::UiState`] (`ui.json`): which tabs show and in what order, tabs and groups of their own
//! holding any commands, and the commands on the Quick Access Toolbar. Agents read and change it
//! with `ui.customizeRibbon` and `ui.quickAccessToolbar`, which never open a dialog.
//!
//! Everything read from `ui.json` or a command is hostile: unknown tabs and commands are dropped,
//! duplicates removed, labels shortened and every list capped ([`RibbonLayout::sanitize`]).

use egui::{Align2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::icons::{self, Icon};
use crate::ribbon::TABS;
use crate::theme::{self, Tokens};

pub const MAX_CUSTOM_TABS: usize = 32;
pub const MAX_GROUPS: usize = 64;
pub const MAX_GROUP_COMMANDS: usize = 64;
pub const MAX_QAT: usize = 64;
pub const MAX_LABEL: usize = 64;
/// The Quick Access Toolbar until the user changes it (after AutoSave and Home, which stay).
pub const DEFAULT_QAT: &[&str] = &["file.save", "edit.undo", "edit.redo"];
/// The toolbar's … menu: the commands Excel offers there to tick on and off.
pub const QAT_MENU: &[&str] = &[
    "file.new",
    "file.open",
    "file.save",
    "file.print",
    "edit.undo",
    "edit.redo",
    "data.sortAscending",
    "data.sortDescending",
    "formulas.calculateNow",
    "review.spelling",
];
/// Interface commands a toolbar or custom group may hold besides the engine's.
const UI_COMMANDS: &[(&str, &str, &str)] = &[("view.collapseRibbon", "Collapse Ribbon", "View"), ("view.darkMode", "Dark Mode", "View")];

/// The user's ribbon: built-in tabs shown, hidden and reordered, tabs and groups of their own, and
/// the Quick Access Toolbar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RibbonLayout {
    /// Tabs in the order chosen: built-in tab names and custom tab keys. A built-in tab missing
    /// from it (all of them, until the user reorders, and any tab a later version adds) keeps its
    /// place after the built-in tab before it.
    pub order: Vec<String>,
    /// Tabs not shown (built-in names or custom keys).
    pub hidden: Vec<String>,
    pub tabs: Vec<CustomTab>,
    /// Custom groups, each at the end of a built-in or custom tab, in the order shown.
    pub groups: Vec<CustomGroup>,
    /// Command ids on the Quick Access Toolbar, in order.
    pub qat: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTab {
    /// `custom1`, `custom2`…: what `order`, `hidden` and a group's `tab` name it by.
    pub key: String,
    pub label: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomGroup {
    /// The tab it is on: a built-in tab name or a custom tab key.
    pub tab: String,
    pub label: String,
    pub commands: Vec<String>,
}

impl Default for RibbonLayout {
    fn default() -> Self {
        RibbonLayout { order: vec![], hidden: vec![], tabs: vec![], groups: vec![], qat: DEFAULT_QAT.iter().map(|s| s.to_string()).collect() }
    }
}

/// Commands a button can't run: each needs a value that only its own ribbon control asks for (a
/// size, a colour, a name, a range…), so with no parameters it fails. They are left out of the
/// dialog, the right-click menus and saved layouts; `every_offered_command_runs_from_a_button`
/// finds any new one.
const NEEDS_INPUT: &[&str] = &[
    "arrange.bringForward",
    "arrange.rotate",
    "arrange.sendBackward",
    "automate.newScript",
    "chart.selectData",
    "chart.shapeOutline",
    "data.consolidate",
    "data.dataTable",
    "data.goalSeek",
    "edit.find",
    "edit.goTo",
    "edit.replace",
    "file.exportCsv",
    "file.exportHtml",
    "formulas.useInFormula",
    "home.columnWidth",
    "home.defaultWidth",
    "home.fillColor",
    "home.fontColor",
    "home.fontName",
    "home.fontSize",
    "home.newCellStyle",
    "home.numberFormat",
    "home.rowHeight",
    "insert.sparkline",
    "pivot.changeSource",
    "pivot.collapse",
    "pivot.group",
    "sheet.move",
    "sheet.rename",
    "table.resize",
    "view.zoom",
    "window.activate",
];

/// Why a toolbar or group can't hold command `id`.
fn refused(id: &str) -> String {
    if NEEDS_INPUT.contains(&id) {
        format!("{id:?} needs a value that only its ribbon control asks for, so no button can run it")
    } else {
        format!("unknown command {id:?}")
    }
}

/// Whether a toolbar or group may hold command `id`.
pub fn known_command(id: &str) -> bool {
    !NEEDS_INPUT.contains(&id) && (gridcraft_engine::find_command(id).is_some() || UI_COMMANDS.iter().any(|(u, ..)| *u == id))
}

fn label_text(label: &str, fallback: &str) -> String {
    let label: String = label.trim().chars().filter(|c| !c.is_control()).take(MAX_LABEL).collect();
    if label.is_empty() { fallback.to_string() } else { label }
}

fn commands(list: &[String], known: &dyn Fn(&str) -> bool, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for id in list {
        if out.len() >= cap {
            break;
        }
        if known(id) && !out.contains(id) {
            out.push(id.clone());
        }
    }
    out
}

impl RibbonLayout {
    /// Reads a layout from JSON, sanitised; anything it can't use is left out, never an error.
    pub fn from_json(v: Json) -> RibbonLayout {
        let mut layout: RibbonLayout = serde_json::from_value(v).unwrap_or_default();
        layout.sanitize(&known_command);
        layout
    }

    pub fn to_json(&self) -> Json {
        serde_json::to_value(self).unwrap_or(Json::Null)
    }

    pub fn is_custom(&self, key: &str) -> bool {
        self.tabs.iter().any(|t| t.key == key)
    }

    fn is_tab(&self, key: &str) -> bool {
        TABS.contains(&key) || self.is_custom(key)
    }

    /// Drops what doesn't fit: unknown tabs and commands, duplicates, empty or overlong labels,
    /// and anything past the caps.
    pub fn sanitize(&mut self, known: &dyn Fn(&str) -> bool) {
        let mut tabs: Vec<CustomTab> = Vec::new();
        for t in std::mem::take(&mut self.tabs) {
            let ok_key = t.key.strip_prefix("custom").is_some_and(|n| !n.is_empty() && n.len() <= 6 && n.bytes().all(|b| b.is_ascii_digit()));
            if tabs.len() < MAX_CUSTOM_TABS && ok_key && !tabs.iter().any(|x| x.key == t.key) {
                tabs.push(CustomTab { label: label_text(&t.label, "New Tab"), key: t.key });
            }
        }
        self.tabs = tabs;
        let order = std::mem::take(&mut self.order);
        for key in order {
            if self.is_tab(&key) && !self.order.contains(&key) {
                self.order.push(key);
            }
        }
        let hidden = std::mem::take(&mut self.hidden);
        for key in hidden {
            if self.is_tab(&key) && !self.hidden.contains(&key) {
                self.hidden.push(key);
            }
        }
        let groups = std::mem::take(&mut self.groups);
        for g in groups {
            if self.groups.len() < MAX_GROUPS && self.is_tab(&g.tab) {
                self.groups.push(CustomGroup {
                    label: label_text(&g.label, "New Group"),
                    commands: commands(&g.commands, known, MAX_GROUP_COMMANDS),
                    tab: g.tab,
                });
            }
        }
        self.qat = commands(&self.qat, known, MAX_QAT);
    }

    /// Every tab, shown or hidden, in the order the user put them in (see [`RibbonLayout::order`]).
    pub fn tab_order(&self) -> Vec<String> {
        let mut out: Vec<String> = self.order.iter().filter(|k| self.is_tab(k)).cloned().collect();
        for (i, tab) in TABS.iter().enumerate() {
            if out.iter().any(|k| k == tab) {
                continue;
            }
            // After the nearest built-in tab before it that is placed; first when there is none.
            let at = TABS[..i].iter().rev().find_map(|prev| out.iter().position(|k| k == prev)).map_or(0, |p| p + 1);
            out.insert(at, tab.to_string());
        }
        for t in &self.tabs {
            if !out.contains(&t.key) {
                out.push(t.key.clone());
            }
        }
        out
    }

    pub fn visible_tabs(&self) -> Vec<String> {
        self.tab_order().into_iter().filter(|k| !self.hidden.contains(k)).collect()
    }

    /// What a tab is called: a custom tab's label, or the built-in tab's name.
    pub fn tab_label<'a>(&'a self, key: &'a str) -> &'a str {
        self.tabs.iter().find(|t| t.key == key).map_or(key, |t| t.label.as_str())
    }

    /// Adds a custom tab after the others, with one empty group, and returns its key.
    pub fn add_tab(&mut self, label: &str) -> Option<String> {
        if self.tabs.len() >= MAX_CUSTOM_TABS || self.groups.len() >= MAX_GROUPS {
            return None;
        }
        let n = (1..).find(|n| !self.is_custom(&format!("custom{n}")))?;
        let key = format!("custom{n}");
        self.order = self.tab_order();
        self.order.push(key.clone());
        self.tabs.push(CustomTab { key: key.clone(), label: label_text(label, "New Tab") });
        self.groups.push(CustomGroup { tab: key.clone(), label: "New Group".into(), commands: vec![] });
        Some(key)
    }

    /// Adds an empty group at the end of tab `tab` and returns its index.
    pub fn add_group(&mut self, tab: &str, label: &str) -> Option<usize> {
        if self.groups.len() >= MAX_GROUPS || !self.is_tab(tab) {
            return None;
        }
        // After the tab's last group, so its groups stay together.
        let at = self.groups.iter().rposition(|g| g.tab == tab).map_or(self.groups.len(), |i| i + 1);
        self.groups.insert(at, CustomGroup { tab: tab.to_string(), label: label_text(label, "New Group"), commands: vec![] });
        Some(at)
    }

    /// Removes a custom tab and its groups. Built-in tabs can only be hidden.
    pub fn remove_tab(&mut self, key: &str) {
        if !self.is_custom(key) {
            return;
        }
        self.tabs.retain(|t| t.key != key);
        self.groups.retain(|g| g.tab != key);
        self.order.retain(|k| k != key);
        self.hidden.retain(|k| k != key);
    }

    pub fn set_hidden(&mut self, key: &str, hidden: bool) {
        self.hidden.retain(|k| k != key);
        if hidden && self.is_tab(key) {
            self.hidden.push(key.to_string());
        }
    }

    /// Moves a tab one place earlier (`-1`) or later (`1`).
    pub fn move_tab(&mut self, key: &str, delta: isize) {
        let mut order = self.tab_order();
        let Some(i) = order.iter().position(|k| k == key) else { return };
        let Some(j) = i.checked_add_signed(delta).filter(|j| *j < order.len()) else { return };
        order.swap(i, j);
        self.order = order;
    }

    /// Moves a group one place earlier or later among the groups of its tab.
    pub fn move_group(&mut self, index: usize, delta: isize) -> usize {
        let Some(tab) = self.groups.get(index).map(|g| g.tab.clone()) else { return index };
        let same: Vec<usize> = self.groups.iter().enumerate().filter(|(_, g)| g.tab == tab).map(|(i, _)| i).collect();
        let Some(pos) = same.iter().position(|i| *i == index) else { return index };
        let Some(other) = pos.checked_add_signed(delta).and_then(|p| same.get(p)).copied() else { return index };
        self.groups.swap(index, other);
        other
    }

    pub fn qat_add(&mut self, id: &str) -> bool {
        if self.qat.len() >= MAX_QAT || !known_command(id) || self.qat.iter().any(|q| q == id) {
            return false;
        }
        self.qat.push(id.to_string());
        true
    }

    pub fn qat_remove(&mut self, id: &str) {
        self.qat.retain(|q| q != id);
    }
}

/// `UiState::ribbon` from `ui.json`: a malformed value reads as the default layout instead of
/// failing the whole `UiState`, and what it holds is sanitised.
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<RibbonLayout, D::Error> {
    let v = Json::deserialize(d)?;
    Ok(RibbonLayout::from_json(v))
}

// ---------------------------------------------------------------- commands (agents)

fn string_list(v: &Json) -> Result<Vec<String>, String> {
    let Some(a) = v.as_array() else { return Err("expected a list of command ids".into()) };
    let ids: Vec<String> = a.iter().filter_map(Json::as_str).map(str::to_string).collect();
    if let Some(bad) = ids.iter().find(|id| !known_command(id)) {
        return Err(refused(bad));
    }
    Ok(ids)
}

/// `ui.quickAccessToolbar {commands?: [ids], add?: id, remove?: id, reset?: bool}` → `{commands}`.
/// With no parameters it reads the toolbar.
pub fn quick_access_command(app: &mut SheetApp, p: &Json) -> Result<Json, String> {
    let layout = &mut app.ui.ribbon;
    if p.get("reset").and_then(Json::as_bool) == Some(true) {
        layout.qat = RibbonLayout::default().qat;
    }
    if let Some(v) = p.get("commands") {
        let ids = string_list(v)?;
        if ids.len() > MAX_QAT {
            return Err(format!("the Quick Access Toolbar holds at most {MAX_QAT} commands"));
        }
        layout.qat = commands(&ids, &known_command, MAX_QAT);
    }
    if let Some(id) = p.get("add").and_then(Json::as_str) {
        if !known_command(id) {
            return Err(refused(id));
        }
        if !layout.qat.iter().any(|q| q == id) && !layout.qat_add(id) {
            return Err(format!("the Quick Access Toolbar holds at most {MAX_QAT} commands"));
        }
    }
    if let Some(id) = p.get("remove").and_then(Json::as_str) {
        layout.qat_remove(id);
    }
    Ok(json!({"commands": layout.qat}))
}

/// `ui.customizeRibbon {layout?, reset?: bool}` → `{layout, tabs}`: replaces the whole layout
/// (sanitised: what it couldn't use is left out of the `layout` returned) and returns it with the
/// tabs now shown, in order. With no parameters it reads them.
pub fn customize_command(app: &mut SheetApp, p: &Json) -> Result<Json, String> {
    if p.get("reset").and_then(Json::as_bool) == Some(true) {
        app.ui.ribbon = RibbonLayout::default();
    }
    if let Some(v) = p.get("layout") {
        if !v.is_object() {
            return Err("`layout` must be an object: {order, hidden, tabs, groups, qat}".into());
        }
        let qat = app.ui.ribbon.qat.clone();
        // Unlike a saved ui.json, a layout sent with a field of the wrong shape is an error and
        // leaves the ribbon as it was, rather than resetting it.
        let mut layout: RibbonLayout = serde_json::from_value(v.clone()).map_err(|e| format!("`layout` is malformed: {e}"))?;
        layout.sanitize(&known_command);
        if v.get("qat").is_none() {
            layout.qat = qat;
        }
        app.ui.ribbon = layout;
    }
    let tabs: Vec<Json> = app.ui.ribbon.visible_tabs().iter().map(|k| json!({"key": k, "label": app.ui.ribbon.tab_label(k)})).collect();
    Ok(json!({"layout": app.ui.ribbon.to_json(), "tabs": tabs}))
}

// ---------------------------------------------------------------- commands' looks

/// A command's label and the first entry of its menu path (its tab), English.
fn command_info(id: &str) -> Option<(&'static str, &'static str)> {
    if let Some(spec) = gridcraft_engine::find_command(id) {
        return Some((spec.label, spec.menu.first().copied().unwrap_or("")));
    }
    UI_COMMANDS.iter().find(|(u, ..)| *u == id).map(|(_, label, tab)| (*label, *tab))
}

fn command_shortcut(id: &str) -> Option<&'static str> {
    gridcraft_engine::find_command(id).and_then(|s| s.shortcut)
}

/// A shortcut as this platform writes it: ⌘⇧S on a Mac, Ctrl+Shift+S elsewhere.
fn shortcut_text(s: &str, mac: bool) -> String {
    if mac {
        s.split('+')
            .map(|k| match k {
                "Cmd" | "Ctrl" => "⌘",
                "Shift" => "⇧",
                "Alt" => "⌥",
                other => other,
            })
            .collect()
    } else {
        s.replace("Cmd", "Ctrl")
    }
}

/// The interface's icon for each command that has one; others get a lettered tile.
const COMMAND_ICONS: &[(&str, Icon)] = &[
    ("file.new", Icon::NewFile),
    ("file.open", Icon::OpenFolder),
    ("file.save", Icon::Save),
    ("file.saveAs", Icon::Save),
    ("file.print", Icon::Print),
    ("file.printPreview", Icon::Print),
    ("edit.undo", Icon::Undo),
    ("edit.redo", Icon::Redo),
    ("edit.copy", Icon::Copy),
    ("edit.cut", Icon::Cut),
    ("edit.paste", Icon::Paste),
    ("edit.pasteSpecial", Icon::Paste),
    ("edit.formatPainter", Icon::Brush),
    ("edit.fillDown", Icon::FillDown),
    ("edit.clearAll", Icon::Eraser),
    ("edit.clearFormats", Icon::Eraser),
    ("edit.clearContents", Icon::Eraser),
    ("home.bold", Icon::Bold),
    ("home.italic", Icon::Italic),
    ("home.underline", Icon::Underline),
    ("home.strikethrough", Icon::Strike),
    ("home.borders", Icon::Borders),
    ("home.alignLeft", Icon::AlignLeft),
    ("home.alignRight", Icon::AlignRight),
    ("home.alignTop", Icon::AlignTop),
    ("home.alignMiddle", Icon::AlignMiddle),
    ("home.alignBottom", Icon::AlignBottom),
    ("home.wrapText", Icon::Wrap),
    ("home.mergeCenter", Icon::Merge),
    ("home.mergeCells", Icon::Merge),
    ("home.mergeAcross", Icon::Merge),
    ("home.increaseIndent", Icon::IndentInc),
    ("home.decreaseIndent", Icon::IndentDec),
    ("home.orientation", Icon::Orientation),
    ("home.percent", Icon::Percent),
    ("home.comma", Icon::Comma),
    ("home.increaseDecimal", Icon::DecInc),
    ("home.decreaseDecimal", Icon::DecDec),
    ("home.conditionalFormat", Icon::CondFormat),
    ("home.formatAsTable", Icon::FormatTable),
    ("home.insertCells", Icon::Insert),
    ("home.insertRows", Icon::Insert),
    ("home.deleteCells", Icon::Delete),
    ("home.deleteRows", Icon::Delete),
    ("home.deleteColumns", Icon::Delete),
    ("insert.table", Icon::Table),
    ("insert.pivotTable", Icon::PivotTable),
    ("insert.chart", Icon::Chart),
    ("insert.recommendedCharts", Icon::Chart),
    ("insert.picture", Icon::Picture),
    ("insert.shape", Icon::Shapes),
    ("insert.textBox", Icon::TextBox),
    ("insert.link", Icon::Link),
    ("insert.symbol", Icon::Symbol),
    ("review.newComment", Icon::Comment),
    ("review.newNote", Icon::Note),
    ("review.spelling", Icon::Spell),
    ("review.protectSheet", Icon::Lock),
    ("review.protectWorkbook", Icon::Lock),
    ("data.sortAscending", Icon::SortAsc),
    ("data.sortDescending", Icon::SortDesc),
    ("data.sort", Icon::SortFilter),
    ("data.filter", Icon::Filter),
    ("data.removeDuplicates", Icon::Duplicates),
    ("data.textToColumns", Icon::TextColumns),
    ("data.validation", Icon::Validation),
    ("data.group", Icon::Group),
    ("data.ungroup", Icon::Ungroup),
    ("data.subtotal", Icon::Subtotal),
    ("formulas.autoSum", Icon::Sum),
    ("formulas.insertFunction", Icon::Function),
    ("formulas.nameManager", Icon::Name),
    ("formulas.calculateNow", Icon::Calc),
    ("formulas.calculateSheet", Icon::Calc),
    ("view.freezePanes", Icon::Freeze),
    ("view.freezeTopRow", Icon::Freeze),
    ("view.freezeFirstColumn", Icon::Freeze),
    ("view.zoomToSelection", Icon::Zoom),
    ("view.gridlines", Icon::Gridlines),
    ("view.normal", Icon::Normal),
    ("view.pageLayout", Icon::PageLayout),
    ("view.pageBreakPreview", Icon::PageBreak),
    ("view.darkMode", Icon::Appearance),
    ("pageLayout.margins", Icon::Margins),
    ("pageLayout.orientation", Icon::Orient),
    ("pageLayout.printArea", Icon::PrintArea),
    ("pageLayout.theme", Icon::Theme),
];

pub fn command_icon(id: &str) -> Option<Icon> {
    COMMAND_ICONS.iter().find(|(c, _)| *c == id).map(|(_, icon)| *icon)
}

/// Why command `id` can't run now, or `None` when it can.
fn disabled_reason(app: &SheetApp, id: &str) -> Option<String> {
    let spec = gridcraft_engine::find_command(id)?;
    (spec.enabled)(&app.session).err()
}

/// A toolbar or custom-group button for command `id`: its icon (or a lettered tile), its label
/// when `with_label`, a tip with the shortcut (and why it can't run, when it can't), and its name
/// and state for screen readers. Doesn't click while the command can't run.
/// `quiet` draws it in the title bar's dimmer tone.
fn command_button(app: &SheetApp, ui: &mut Ui, id: &str, with_label: bool, quiet: bool, height: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let lang = app.ui.language;
    let label = command_info(id).map_or(id, |(l, _)| l);
    let label = lang.tr(label).trim_end_matches('…').to_string();
    let reason = disabled_reason(app, id);
    let enabled = reason.is_none();
    let font = theme::ui_font(12.5);
    let text = with_label.then(|| ui.painter().layout_no_wrap(label.clone(), font.clone(), t.text));
    let w = 26.0 + text.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, height), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(rect, 4.0, t.pressed);
    } else if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, t.hover);
    }
    let color = if !enabled {
        t.text_disabled
    } else if quiet {
        t.text_dim
    } else {
        t.text
    };
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(16.0, 16.0));
    match command_icon(id) {
        Some(icon) => icons::paint(ui.painter(), icon_rect, icon, color),
        None => {
            let letter = label.chars().find(|c| c.is_alphanumeric()).unwrap_or('•').to_uppercase().to_string();
            ui.painter().rect_stroke(icon_rect, 3.0, Stroke::new(1.0, color), StrokeKind::Inside);
            ui.painter().text(icon_rect.center(), Align2::CENTER_CENTER, letter, theme::ui_bold(10.0), color);
        }
    }
    if let Some(g) = text {
        ui.painter().galley(pos2(rect.left() + 26.0, rect.center().y - g.size().y / 2.0), g, color);
    }
    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    let mut tip = label.clone();
    if let Some(s) = command_shortcut(id) {
        tip = format!("{tip} ({})", shortcut_text(s, mac));
    }
    if let Some(r) = &reason {
        tip = format!("{tip}\n{}", crate::clean_error(r));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
    resp.on_hover_text(tip)
}

/// Runs a command from the toolbar or a custom group, as the ribbon's own buttons run it.
fn run_from_toolbar(app: &mut SheetApp, ctx: &egui::Context, id: &str) {
    match id {
        "edit.copy" | "edit.cut" => app.copy_to_clipboard(ctx, id),
        "edit.paste" => app.paste_from_clipboard(),
        // File operations include the cell still being edited, as the File menu's do.
        "file.save" | "file.saveAs" | "file.print" | "file.printPreview" => {
            if app.commit_edit(0, 0, false, false) {
                app.run_or_alert(id, json!({}));
            }
        }
        // The grid draws tracer arrows from what these return, as the ribbon's buttons keep it.
        "formulas.tracePrecedents" | "formulas.traceDependents" => match app.run(id, json!({})) {
            Ok(r) => app.grid.trace = Some(r),
            Err(e) => app.alert(e),
        },
        "formulas.removeArrows" => app.grid.trace = None,
        _ => app.run_or_alert(id, json!({})),
    }
}

// ---------------------------------------------------------------- right-click menus

/// What a right-click menu asked for, carried to the end of the frame (the menus draw with the
/// app borrowed immutably).
#[derive(Clone, Debug, PartialEq)]
enum Request {
    AddQat(String),
    RemoveQat(String),
    Customize(&'static str),
    Collapse,
}

fn request_id() -> egui::Id {
    egui::Id::new("gridcraft.customize.requests")
}

fn request(ui: &Ui, r: Request) {
    ui.ctx().data_mut(|d| d.get_temp_mut_or_default::<Vec<Request>>(request_id()).push(r));
    ui.close();
}

/// Carries out what the right-click menus asked for this frame.
pub fn apply_requests(app: &mut SheetApp, ctx: &egui::Context) {
    let requests = ctx.data_mut(|d| d.remove_temp::<Vec<Request>>(request_id())).unwrap_or_default();
    for r in requests {
        match r {
            Request::AddQat(id) => {
                app.ui.ribbon.qat_add(&id);
            }
            Request::RemoveQat(id) => app.ui.ribbon.qat_remove(&id),
            Request::Customize(page) => app.open_dialog("customizeRibbon", json!({"page": page})),
            Request::Collapse => app.ui.ribbon_collapsed = !app.ui.ribbon_collapsed,
        }
    }
}

fn ribbon_items(app: &SheetApp, ui: &mut Ui) {
    let lang = app.ui.language;
    if ui.button(lang.tr("Customize Quick Access Toolbar…")).clicked() {
        request(ui, Request::Customize("qat"));
    }
    ui.separator();
    if ui.button(lang.tr("Customize the Ribbon…")).clicked() {
        request(ui, Request::Customize("ribbon"));
    }
    let collapse = if app.ui.ribbon_collapsed { "Pin the Ribbon" } else { "Collapse the Ribbon" };
    if ui.button(lang.tr(collapse)).clicked() {
        request(ui, Request::Collapse);
    }
}

/// The right-click menu of a ribbon control that runs command `id`: add it to (or remove it from)
/// the Quick Access Toolbar, and customise either.
pub fn control_menu(app: &SheetApp, ui: &mut Ui, id: &str) {
    let lang = app.ui.language;
    if known_command(id) {
        if app.ui.ribbon.qat.iter().any(|q| q == id) {
            if ui.button(lang.tr("Remove from Quick Access Toolbar")).clicked() {
                request(ui, Request::RemoveQat(id.to_string()));
            }
        } else {
            let full = app.ui.ribbon.qat.len() >= MAX_QAT;
            if ui.add_enabled(!full, egui::Button::new(lang.tr("Add to Quick Access Toolbar"))).clicked() {
                request(ui, Request::AddQat(id.to_string()));
            }
        }
    }
    ribbon_items(app, ui);
}

/// The right-click menu of the ribbon's empty space and its tabs.
pub fn ribbon_menu(app: &SheetApp, ui: &mut Ui) {
    ribbon_items(app, ui);
}

// ---------------------------------------------------------------- the Quick Access Toolbar

/// The Quick Access Toolbar's commands and its … menu, drawn in the title bar after AutoSave and
/// Home. Undo keeps its list of the steps it can undo.
pub fn quick_access(app: &mut SheetApp, ui: &mut Ui, height: f32) {
    let t = Tokens::get(ui.ctx());
    let ids = app.ui.ribbon.qat.clone();
    for id in &ids {
        let resp = command_button(app, ui, id, false, true, height);
        let lang = app.ui.language;
        resp.context_menu(|ui| {
            if ui.button(lang.tr("Remove from Quick Access Toolbar")).clicked() {
                request(ui, Request::RemoveQat(id.clone()));
            }
            ribbon_items(app, ui);
        });
        if resp.clicked() {
            run_from_toolbar(app, ui.ctx(), id);
        }
        if id == "edit.undo" {
            let undo_list = crate::widgets::icon_button(ui, Icon::Chevron, t.text_dim, "Undo list", vec2(14.0, height));
            egui::Popup::menu(&undo_list).show(|ui| {
                // Nothing to offer while Undo itself can't run (an undo group still open).
                let labels: Vec<String> = if gridcraft_engine::cmd::can_undo(&app.session).is_ok() {
                    app.session.active().map(|d| d.undo.iter().rev().take(20).map(|e| e.label.clone()).collect()).unwrap_or_default()
                } else {
                    Vec::new()
                };
                if labels.is_empty() {
                    ui.label(app.ui.language.tr("Can't Undo"));
                }
                for (i, l) in labels.iter().enumerate() {
                    if ui.button(format!("{} {l}", app.ui.language.tr("Undo"))).clicked() {
                        app.run_or_alert("edit.undo", json!({"steps": i + 1}));
                    }
                }
            });
        }
    }
    let more = crate::widgets::icon_button(ui, Icon::More, t.text_dim, "Customize Quick Access Toolbar", vec2(26.0, height));
    more.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, app.ui.language.tr("Customize Quick Access Toolbar")));
    egui::Popup::menu(&more).show(|ui| {
        let lang = app.ui.language;
        ui.label(egui::RichText::new(lang.tr("Customize Quick Access Toolbar")).strong());
        for id in QAT_MENU {
            let mut on = app.ui.ribbon.qat.iter().any(|q| q == id);
            let label = command_info(id).map_or(*id, |(l, _)| l);
            if ui.checkbox(&mut on, lang.tr(label)).changed() {
                if on {
                    app.ui.ribbon.qat_add(id);
                } else {
                    app.ui.ribbon.qat_remove(id);
                }
            }
        }
        ui.separator();
        if ui.button(lang.tr("More Commands…")).clicked() {
            app.open_dialog("customizeRibbon", json!({"page": "qat"}));
            ui.close();
        }
        if ui.button(lang.tr("Customize the Ribbon…")).clicked() {
            app.open_dialog("customizeRibbon", json!({"page": "ribbon"}));
            ui.close();
        }
    });
}

// ---------------------------------------------------------------- custom groups

/// The user's groups on tab `tab`, after its own controls: each command a labelled small button,
/// three to a column, with a divider before the group.
pub fn custom_groups(app: &mut SheetApp, ui: &mut Ui, tab: &str) {
    let groups: Vec<CustomGroup> = app.ui.ribbon.groups.iter().filter(|g| g.tab == tab).cloned().collect();
    let builtin = TABS.contains(&tab);
    for (n, g) in groups.iter().enumerate() {
        if builtin || n > 0 {
            divider(ui);
        }
        if g.commands.is_empty() {
            let t = Tokens::get(ui.ctx());
            let lang = app.ui.language;
            let (r, resp) = ui.allocate_exact_size(vec2(96.0, 72.0), Sense::click());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, &g.label, theme::ui_font(11.5), t.text_dim);
            resp.on_hover_text(lang.tr("Right-click to add commands with Customize the Ribbon")).context_menu(|ui| ribbon_menu(app, ui));
            continue;
        }
        for column in g.commands.chunks(3) {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for id in column {
                    let resp = command_button(app, ui, id, true, false, 23.0);
                    resp.context_menu(|ui| control_menu(app, ui, id));
                    if resp.clicked() {
                        run_from_toolbar(app, ui.ctx(), id);
                    }
                }
            });
        }
    }
}

fn divider(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 6.0), pos2(r.center().x, r.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

// ---------------------------------------------------------------- the dialog

/// The commands offered in the dialog: every command with a place in the menus that a button can
/// run, and the interface commands, as (id, English label, English tab), sorted by label.
fn choices() -> Vec<(&'static str, &'static str, &'static str)> {
    let mut v: Vec<(&'static str, &'static str, &'static str)> = gridcraft_engine::command_specs()
        .iter()
        .filter(|c| known_command(c.id))
        .filter_map(|c| c.menu.first().map(|tab| (c.id, c.label, *tab)))
        .collect();
    v.extend(UI_COMMANDS.iter().copied());
    v.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()).then(a.0.cmp(b.0)));
    v
}

/// What is selected in the dialog's right-hand list.
#[derive(Clone, Debug, PartialEq)]
enum Selected {
    None,
    Tab(String),
    Group(usize),
    Command(usize, usize),
    Qat(usize),
}

fn selected(d: &crate::dialogs::Dialog) -> Selected {
    let v = d.values.get("selected").and_then(Json::as_str).unwrap_or("");
    let num = |s: &str| s.parse::<usize>().ok();
    match v.split_once(':') {
        Some(("tab", k)) => Selected::Tab(k.to_string()),
        Some(("group", g)) => num(g).map_or(Selected::None, Selected::Group),
        Some(("command", gi)) => gi.split_once('/').and_then(|(g, i)| Some(Selected::Command(num(g)?, num(i)?))).unwrap_or(Selected::None),
        Some(("qat", i)) => num(i).map_or(Selected::None, Selected::Qat),
        _ => Selected::None,
    }
}

fn select(d: &mut crate::dialogs::Dialog, s: Selected) {
    let v = match s {
        Selected::None => String::new(),
        Selected::Tab(k) => format!("tab:{k}"),
        Selected::Group(g) => format!("group:{g}"),
        Selected::Command(g, i) => format!("command:{g}/{i}"),
        Selected::Qat(i) => format!("qat:{i}"),
    };
    d.values.insert("selected".into(), json!(v));
}

/// Customize Ribbon and Quick Access Toolbar, on two pages (Excel for Mac's Ribbon & Toolbar).
/// Every change applies at once, as File › Options does here.
pub fn dialog(app: &mut SheetApp, ui: &mut Ui, d: &mut crate::dialogs::Dialog, confirm: &mut bool) {
    let lang = app.ui.language;
    if d.tab.is_empty() {
        d.tab = if d.values.get("page").and_then(Json::as_str) == Some("qat") { "qat".into() } else { "ribbon".into() };
    }
    ui.horizontal(|ui| {
        for (page, label) in [("ribbon", "Ribbon"), ("qat", "Quick Access Toolbar")] {
            if ui.selectable_label(d.tab == page, lang.tr(label)).clicked() && d.tab != page {
                d.tab = page.into();
                select(d, Selected::None);
            }
        }
    });
    ui.separator();
    let all = choices();
    ui.horizontal_top(|ui| {
        // Left: choose a command.
        ui.vertical(|ui| {
            ui.set_width(250.0);
            ui.label(egui::RichText::new(lang.tr("Choose commands from:")).strong());
            let category = d.values.get("category").and_then(Json::as_str).unwrap_or("").to_string();
            let mut cats: Vec<&str> = vec![""];
            for (_, _, tab) in &all {
                if !cats.contains(tab) {
                    cats.push(tab);
                }
            }
            let shown = |c: &str| if c.is_empty() { lang.tr("All Commands").to_string() } else { lang.tr(c).to_string() };
            let mut pick_category = category.clone();
            egui::ComboBox::from_id_salt("customize.category").selected_text(shown(&category)).width(240.0).show_ui(ui, |ui| {
                for c in &cats {
                    ui.selectable_value(&mut pick_category, c.to_string(), shown(c));
                }
            });
            if pick_category != category {
                d.values.insert("category".into(), json!(pick_category));
            }
            ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(240.0).hint_text(lang.tr("Search")));
            let q = d.search.to_lowercase();
            let picked = d.values.get("pick").and_then(Json::as_str).unwrap_or("").to_string();
            egui::ScrollArea::vertical().id_salt("customize.commands").min_scrolled_height(300.0).max_height(300.0).show(ui, |ui| {
                for &(id, label, tab) in all.iter().filter(|e| {
                    let (id, label, tab) = **e;
                    (category.is_empty() || tab == category)
                        && (q.is_empty()
                            || lang.tr(label).to_lowercase().contains(&q)
                            || label.to_lowercase().contains(&q)
                            || id.to_lowercase().contains(&q))
                }) {
                    let r = ui.add(egui::Button::selectable(picked == id, lang.tr(label)).min_size(vec2(236.0, 20.0)));
                    if r.clicked() {
                        d.values.insert("pick".into(), json!(id));
                    }
                    r.on_hover_text(format!("{} · {id}", lang.tr(tab)));
                }
            });
        });
        // Middle: Add and Remove.
        ui.vertical(|ui| {
            ui.add_space(120.0);
            let picked = d.values.get("pick").and_then(Json::as_str).map(str::to_string);
            if ui.add_enabled(picked.is_some(), egui::Button::new(format!("{} >>", lang.tr("Add")))).clicked()
                && let Some(id) = picked
            {
                add(app, d, &id);
            }
            if ui.button(format!("<< {}", lang.tr("Remove"))).clicked() {
                remove(app, d);
            }
        });
        // Right: the ribbon's tabs and groups, or the toolbar.
        ui.vertical(|ui| {
            ui.set_width(260.0);
            if d.tab == "qat" {
                qat_page(app, ui, d);
            } else {
                ribbon_page(app, ui, d);
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.button(lang.tr("Reset")).on_hover_text(lang.tr("Put this page back as GridCraft has it")).clicked() {
            let qat = app.ui.ribbon.qat.clone();
            if d.tab == "qat" {
                app.ui.ribbon.qat = RibbonLayout::default().qat;
            } else {
                app.ui.ribbon = RibbonLayout { qat, ..RibbonLayout::default() };
                if !app.ui.ribbon.visible_tabs().contains(&app.ui.ribbon_tab) {
                    app.ui.ribbon_tab = "Home".into();
                }
            }
            select(d, Selected::None);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(lang.tr("Close")).clicked() {
                *confirm = true;
            }
        });
    });
}

fn add(app: &mut SheetApp, d: &mut crate::dialogs::Dialog, id: &str) {
    let layout = &mut app.ui.ribbon;
    if d.tab == "qat" {
        layout.qat_add(id);
        if let Some(i) = layout.qat.iter().position(|q| q == id) {
            select(d, Selected::Qat(i));
        }
        return;
    }
    // Commands go into a group of the user's: the selected one, or a new one on the selected tab.
    let group = match selected(d) {
        Selected::Group(g) | Selected::Command(g, _) => Some(g),
        Selected::Tab(tab) => {
            let last = layout.groups.iter().rposition(|g| g.tab == tab);
            if last.is_some() { last } else { layout.add_group(&tab, "New Group") }
        }
        _ => None,
    };
    let Some(g) = group.filter(|g| *g < layout.groups.len()) else {
        app.message = Some(("Customize the Ribbon".into(), "Select a tab or one of your groups to add the command to.".into()));
        return;
    };
    if let Some(grp) = layout.groups.get_mut(g)
        && grp.commands.len() < MAX_GROUP_COMMANDS
        && !grp.commands.iter().any(|c| c == id)
    {
        grp.commands.push(id.to_string());
        let i = grp.commands.len() - 1;
        select(d, Selected::Command(g, i));
    }
}

fn remove(app: &mut SheetApp, d: &mut crate::dialogs::Dialog) {
    let layout = &mut app.ui.ribbon;
    match selected(d) {
        Selected::Qat(i) if i < layout.qat.len() => {
            layout.qat.remove(i);
            select(d, if layout.qat.is_empty() { Selected::None } else { Selected::Qat(i.min(layout.qat.len() - 1)) });
        }
        Selected::Command(g, i) => {
            if let Some(grp) = layout.groups.get_mut(g)
                && i < grp.commands.len()
            {
                grp.commands.remove(i);
            }
            select(d, Selected::Group(g));
        }
        Selected::Group(g) if g < layout.groups.len() => {
            layout.groups.remove(g);
            select(d, Selected::None);
        }
        Selected::Tab(tab) if layout.is_custom(&tab) => {
            layout.remove_tab(&tab);
            if app.ui.ribbon_tab == tab {
                app.ui.ribbon_tab = "Home".into();
            }
            select(d, Selected::None);
        }
        _ => {}
    }
}

fn qat_page(app: &mut SheetApp, ui: &mut Ui, d: &mut crate::dialogs::Dialog) {
    let lang = app.ui.language;
    ui.label(egui::RichText::new(lang.tr("Quick Access Toolbar")).strong());
    let sel = selected(d);
    egui::ScrollArea::vertical().id_salt("customize.qat").min_scrolled_height(300.0).max_height(300.0).show(ui, |ui| {
        if app.ui.ribbon.qat.is_empty() {
            ui.label(egui::RichText::new(lang.tr("No commands: add some from the list on the left.")).weak());
        }
        for (i, id) in app.ui.ribbon.qat.iter().enumerate() {
            let label = command_info(id).map_or(id.as_str(), |(l, _)| l);
            if ui.add(egui::Button::selectable(sel == Selected::Qat(i), lang.tr(label)).min_size(vec2(240.0, 20.0))).clicked() {
                select(d, Selected::Qat(i));
            }
        }
    });
    ui.horizontal(|ui| {
        // Always there, greyed while nothing is selected, so the dialog keeps its size.
        let n = app.ui.ribbon.qat.len();
        let at = if let Selected::Qat(i) = sel { Some(i).filter(|i| *i < n) } else { None };
        if ui.add_enabled(at.is_some_and(|i| i > 0), egui::Button::new(lang.tr("Up"))).clicked()
            && let Some(i) = at
        {
            app.ui.ribbon.qat.swap(i, i - 1);
            select(d, Selected::Qat(i - 1));
        }
        if ui.add_enabled(at.is_some_and(|i| i + 1 < n), egui::Button::new(lang.tr("Down"))).clicked()
            && let Some(i) = at
        {
            app.ui.ribbon.qat.swap(i, i + 1);
            select(d, Selected::Qat(i + 1));
        }
    });
}

fn ribbon_page(app: &mut SheetApp, ui: &mut Ui, d: &mut crate::dialogs::Dialog) {
    let lang = app.ui.language;
    ui.label(egui::RichText::new(lang.tr("Customize the Ribbon:")).strong());
    let sel = selected(d);
    let order = app.ui.ribbon.tab_order();
    egui::ScrollArea::vertical().id_salt("customize.tabs").min_scrolled_height(300.0).max_height(300.0).show(ui, |ui| {
        for key in &order {
            ui.horizontal(|ui| {
                let mut shown = !app.ui.ribbon.hidden.contains(key);
                let name = if app.ui.ribbon.is_custom(key) { app.ui.ribbon.tab_label(key).to_string() } else { lang.tr(key).to_string() };
                let check = ui.checkbox(&mut shown, "");
                check.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, shown, format!("{} {name}", lang.tr("Show"))));
                if check.on_hover_text(lang.tr("Show this tab")).changed() {
                    app.ui.ribbon.set_hidden(key, !shown);
                }
                let label = if app.ui.ribbon.is_custom(key) { format!("{name} ({})", lang.tr("Custom")) } else { name };
                if ui.selectable_label(sel == Selected::Tab(key.clone()), label).clicked() {
                    select(d, Selected::Tab(key.clone()));
                }
            });
            let groups: Vec<(usize, CustomGroup)> =
                app.ui.ribbon.groups.iter().enumerate().filter(|(_, g)| &g.tab == key).map(|(i, g)| (i, g.clone())).collect();
            for (gi, g) in groups {
                ui.horizontal(|ui| {
                    ui.add_space(28.0);
                    if ui.selectable_label(sel == Selected::Group(gi), format!("{} ({})", g.label, lang.tr("Custom"))).clicked() {
                        select(d, Selected::Group(gi));
                    }
                });
                for (ci, id) in g.commands.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_space(48.0);
                        let label = command_info(id).map_or(id.as_str(), |(l, _)| l);
                        if ui.selectable_label(sel == Selected::Command(gi, ci), lang.tr(label)).clicked() {
                            select(d, Selected::Command(gi, ci));
                        }
                    });
                }
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button(lang.tr("New Tab")).clicked()
            && let Some(key) = app.ui.ribbon.add_tab("New Tab")
        {
            select(d, Selected::Tab(key));
        }
        let tab = match &sel {
            Selected::Tab(k) => Some(k.clone()),
            Selected::Group(g) | Selected::Command(g, _) => app.ui.ribbon.groups.get(*g).map(|g| g.tab.clone()),
            _ => None,
        };
        if ui.add_enabled(tab.is_some(), egui::Button::new(lang.tr("New Group"))).clicked()
            && let Some(tab) = tab
            && let Some(g) = app.ui.ribbon.add_group(&tab, "New Group")
        {
            select(d, Selected::Group(g));
        }
    });
    ui.horizontal(|ui| {
        let (up, down) = (ui.button(lang.tr("Up")).clicked(), ui.button(lang.tr("Down")).clicked());
        let delta = if up {
            -1
        } else if down {
            1
        } else {
            0
        };
        if delta != 0 {
            match sel.clone() {
                Selected::Tab(k) => app.ui.ribbon.move_tab(&k, delta),
                Selected::Group(g) => {
                    let to = app.ui.ribbon.move_group(g, delta);
                    select(d, Selected::Group(to));
                }
                Selected::Command(g, i) => {
                    if let Some(grp) = app.ui.ribbon.groups.get_mut(g)
                        && let Some(j) = i.checked_add_signed(delta).filter(|j| *j < grp.commands.len())
                    {
                        grp.commands.swap(i, j);
                        select(d, Selected::Command(g, j));
                    }
                }
                _ => {}
            }
        }
    });
    // Rename the selected custom tab or group in place.
    let current = match &sel {
        Selected::Tab(k) if app.ui.ribbon.is_custom(k) => Some(app.ui.ribbon.tab_label(k).to_string()),
        Selected::Group(g) => app.ui.ribbon.groups.get(*g).map(|g| g.label.clone()),
        _ => None,
    };
    // Always there, greyed unless one of the user's tabs or groups is selected.
    let renamable = current.is_some();
    // The field keeps what was typed from frame to frame (a trailing space, an empty field) while
    // the label takes it trimmed; leaving the field, or selecting something else, shows the label.
    let owner = d.values.get("selected").cloned().unwrap_or_default();
    let typed = d.values.get("renaming").filter(|o| **o == owner).and(d.values.get("rename")).and_then(Json::as_str);
    let mut name = typed.map(str::to_string).or(current).unwrap_or_default();
    {
        ui.horizontal(|ui| {
            ui.label(lang.tr("Name:"));
            let field = ui.add_enabled(renamable, egui::TextEdit::singleline(&mut name).char_limit(MAX_LABEL).desired_width(180.0));
            if field.lost_focus() {
                d.values.remove("rename");
                d.values.remove("renaming");
            }
            if field.changed() {
                d.values.insert("rename".into(), Json::String(name.clone()));
                d.values.insert("renaming".into(), owner);
                let name = label_text(&name, "");
                if !name.is_empty() {
                    match &sel {
                        Selected::Tab(k) => {
                            if let Some(t) = app.ui.ribbon.tabs.iter_mut().find(|t| &t.key == k) {
                                t.label = name;
                            }
                        }
                        Selected::Group(g) => {
                            if let Some(g) = app.ui.ribbon.groups.get_mut(*g) {
                                g.label = name;
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(id: &str) -> bool {
        known_command(id)
    }

    #[test]
    fn a_hostile_layout_is_sanitised() {
        let long = "x".repeat(500);
        let v = json!({
            "order": ["Data", "Nope", "Data", "custom1", "custom9"],
            "hidden": ["Draw", "Draw", "Bogus"],
            "tabs": [{"key": "custom1", "label": long}, {"key": "custom1", "label": "dup"}, {"key": "home", "label": "bad key"}, {"key": "custom", "label": "no number"}],
            "groups": [
                {"tab": "Home", "label": "  ", "commands": ["home.bold", "home.bold", "no.such", "edit.copy"]},
                {"tab": "Nowhere", "label": "g", "commands": []},
                {"tab": "custom1", "label": "Mine", "commands": (0..200).map(|_| "home.bold").collect::<Vec<_>>()}
            ],
            "qat": ["file.save", "file.save", "made.up", "edit.undo"]
        });
        let l = RibbonLayout::from_json(v);
        assert_eq!(l.tabs.len(), 1);
        assert_eq!(l.tabs[0].label.chars().count(), MAX_LABEL);
        assert_eq!(l.order, vec!["Data".to_string(), "custom1".to_string()]);
        assert_eq!(l.hidden, vec!["Draw".to_string()]);
        assert_eq!(l.groups.len(), 2);
        assert_eq!(l.groups[0].label, "New Group");
        assert_eq!(l.groups[0].commands, vec!["home.bold".to_string(), "edit.copy".to_string()]);
        assert_eq!(l.groups[1].commands, vec!["home.bold".to_string()]);
        assert_eq!(l.qat, vec!["file.save".to_string(), "edit.undo".to_string()]);
        // Not even JSON of the right shape: the default layout.
        assert_eq!(RibbonLayout::from_json(json!([1, 2, 3])), RibbonLayout::default());
        assert_eq!(RibbonLayout::from_json(json!({"qat": "file.save"})), RibbonLayout::default());
        // Caps hold.
        let mut big = RibbonLayout::default();
        for _ in 0..100 {
            big.add_tab("t");
        }
        assert!(big.tabs.len() <= MAX_CUSTOM_TABS && big.groups.len() <= MAX_GROUPS);
        big.sanitize(&known);
        assert!(big.tabs.len() <= MAX_CUSTOM_TABS);
    }

    #[test]
    fn built_in_tabs_keep_their_place_when_others_move() {
        let mut l = RibbonLayout::default();
        assert_eq!(l.tab_order(), TABS.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        // Moving Data first leaves the rest in their order; a tab the order doesn't name (one a
        // later version adds) goes after the built-in tab before it.
        // Data moved first; Automate missing from the order, as a tab a later version adds would be:
        // it goes after the built-in tab before it (View).
        l.order = ["Data", "Home", "Insert", "Draw", "Page Layout", "Formulas", "Review", "View"].iter().map(|s| s.to_string()).collect();
        let order = l.tab_order();
        assert_eq!(order.first().map(String::as_str), Some("Data"));
        assert_eq!(order.last().map(String::as_str), Some("Automate"), "{order:?}");
        assert_eq!(order.len(), TABS.len());
        l.order.clear();
        let key = l.add_tab("Mine").unwrap();
        assert_eq!(l.tab_order().last(), Some(&key));
        l.move_tab(&key, -1);
        assert_eq!(l.tab_order().iter().position(|k| k == &key), Some(TABS.len() - 1));
        l.set_hidden("Draw", true);
        assert!(!l.visible_tabs().contains(&"Draw".to_string()));
        assert_eq!(l.visible_tabs().len(), TABS.len());
        l.remove_tab(&key);
        assert!(l.tabs.is_empty() && l.groups.is_empty() && !l.order.contains(&key));
        // Built-in tabs can be hidden, not removed.
        l.remove_tab("Home");
        assert!(l.tab_order().contains(&"Home".to_string()));
    }

    #[test]
    fn groups_stay_with_their_tab_and_move_among_its_groups() {
        let mut l = RibbonLayout::default();
        let a = l.add_group("Home", "A").unwrap();
        let _ = l.add_group("Data", "D").unwrap();
        let b = l.add_group("Home", "B").unwrap();
        assert_eq!(l.groups.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(), vec!["A", "B", "D"]);
        assert_eq!((a, b), (0, 1));
        let to = l.move_group(1, -1);
        assert_eq!(to, 0);
        assert_eq!(l.groups.iter().map(|g| g.label.as_str()).collect::<Vec<_>>(), vec!["B", "A", "D"]);
        assert_eq!(l.move_group(1, 1), 1, "A is the last of Home's groups");
        assert!(l.add_group("Nowhere", "x").is_none());
    }

    #[test]
    fn the_toolbar_takes_known_commands_once() {
        let mut l = RibbonLayout::default();
        assert_eq!(l.qat, DEFAULT_QAT.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(l.qat_add("home.bold"));
        assert!(!l.qat_add("home.bold"));
        assert!(!l.qat_add("no.such.command"));
        l.qat_remove("file.save");
        assert_eq!(l.qat, vec!["edit.undo".to_string(), "edit.redo".to_string(), "home.bold".to_string()]);
        for id in QAT_MENU {
            assert!(known_command(id), "{id} is a command");
        }
    }

    /// A button runs its command with no parameters, so each command offered must run that way:
    /// it may say it can't run here (no table, nothing to undo), never that it was given too
    /// little. And each one left out still needs its input.
    #[test]
    fn every_offered_command_runs_from_a_button() {
        let app = || {
            let mut s = gridcraft_engine::Session::new();
            s.execute("file.new", json!({"sample": "sales"})).unwrap();
            crate::SheetApp::new(s, Default::default())
        };
        // These report what the sheet lacks through their parameters' error: Ink to Shape when
        // there is no ink stroke, Select Precedents and Dependents when the active cell has none.
        let nothing_here = ["draw.inkToShape", "formulas.selectPrecedents", "formulas.selectDependents"];
        let mut failed = Vec::new();
        for (id, ..) in choices() {
            if let Err(e) = app().run(id, json!({}))
                && e.starts_with("invalid parameters")
                && !nothing_here.contains(&id)
            {
                failed.push(format!("{id}: {e}"));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
        let mut runs = Vec::new();
        for id in NEEDS_INPUT {
            assert!(gridcraft_engine::find_command(id).is_some(), "{id} is a command");
            let r = app().run(id, json!({}));
            if !r.as_ref().is_err_and(|e| e.starts_with("invalid parameters")) {
                runs.push(format!("{id}: {r:?}"));
            }
        }
        assert!(runs.is_empty(), "these run with no input: {runs:#?}");
    }

    /// The toolbar's trace buttons draw their arrows and Remove Arrows takes them away, as the
    /// ribbon's buttons do.
    #[test]
    fn trace_buttons_on_the_toolbar_draw_and_remove_arrows() {
        let mut s = gridcraft_engine::Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "=A1*2"})).unwrap();
        s.execute("selection.set", json!({"range": "B1"})).unwrap();
        let mut app = crate::SheetApp::new(s, Default::default());
        let ctx = egui::Context::default();
        run_from_toolbar(&mut app, &ctx, "formulas.tracePrecedents");
        assert!(app.grid.trace.is_some(), "arrows to B1's precedents");
        run_from_toolbar(&mut app, &ctx, "formulas.removeArrows");
        assert!(app.grid.trace.is_none());
        run_from_toolbar(&mut app, &ctx, "formulas.traceDependents");
        assert!(app.grid.trace.is_some() || app.message.is_some(), "arrows, or a message saying there are none");
    }

    #[test]
    fn every_mapped_icon_names_a_command() {
        assert!(COMMAND_ICONS.len() > 50);
        for (id, _) in COMMAND_ICONS {
            assert!(known_command(id), "{id} is not a command");
        }
    }

    #[test]
    fn shortcuts_follow_the_platform() {
        assert_eq!(shortcut_text("Cmd+Shift+S", true), "⌘⇧S");
        assert_eq!(shortcut_text("Cmd+Shift+S", false), "Ctrl+Shift+S");
        assert_eq!(shortcut_text("F9", false), "F9");
    }

    #[test]
    fn a_saved_layout_round_trips_and_a_broken_one_spares_the_rest() {
        let mut ui = crate::UiState::default();
        ui.ribbon.qat_add("home.bold");
        let key = ui.ribbon.add_tab("Mine").unwrap();
        ui.ribbon_collapsed = true;
        let text = serde_json::to_string(&ui).unwrap();
        let back: crate::UiState = serde_json::from_str(&text).unwrap();
        assert_eq!(back.ribbon, ui.ribbon);
        assert!(back.ribbon.is_custom(&key));
        let broken = r#"{"ribbonCollapsed": true, "ribbon": {"tabs": 7, "qat": {"x": 1}}}"#;
        let back: crate::UiState = serde_json::from_str(broken).unwrap();
        assert!(back.ribbon_collapsed, "the rest of ui.json still reads");
        assert_eq!(back.ribbon, RibbonLayout::default());
    }
}
