//! Custom Sort (Data › Sort): sort levels with column, sort-on and order pickers.
//!
//! The dialog restores the grid selection it changed when it lets go of the grid (see
//! [`restore_selection`], called from `ref_edit::end_pick`) and keeps the worksheet it opened on.

use gridcraft_engine::Selection;
use gridcraft_engine::core::{CellRef, RangeRef, Value, col_to_letters};

use super::show::ok_cancel;
use super::{Dialog, Json, Sense, SheetApp, json, vec2};
use crate::icons::Icon;
use crate::theme::Tokens;

/// Commands that change which worksheet is shown or which worksheets exist: refused while Sort is
/// open, so Sort always acts on the worksheet it opened on.
const SHEET_COMMANDS: [&str; 8] =
    ["sheet.activate", "sheet.next", "sheet.previous", "sheet.move", "sheet.hide", "sheet.unhide", "home.insertSheet", "home.deleteSheet"];

/// What the dialog keeps besides its field values.
#[derive(Clone, Debug, Default)]
pub(super) struct State {
    /// The exact selection to restore when Sort closes, including ranges and the shift anchor.
    restore: Option<Selection>,
    /// The workbook and worksheet Sort opened on: the only one it may sort or restore a selection on.
    home: Option<(u64, usize)>,
    choices: Option<ChoicesCache>,
}

/// The column choices for one sheet revision, range and header setting.
#[derive(Clone, Debug)]
struct ChoicesCache {
    doc_uid: Option<u64>,
    sheet_index: Option<usize>,
    revision: Option<u64>,
    range: Option<String>,
    header: bool,
    choices: Vec<Choice>,
}

/// A column of the sorted range: its letters, its label and whether its values are numbers.
type Choice = (String, String, bool);

/// Opens Custom Sort on the range and header guess of `data.sort`, seeding "Sort by" from the active column (#1906).
pub(super) fn open(app: &mut SheetApp, params: &Json) -> Dialog {
    let restore = app.session.active().map(|doc| doc.selection.clone());
    let home = app.view_key();
    let (range, header) = gridcraft_engine::cmd::data::sort_context(&app.session, params).map_or((None, true), |(r, h)| (Some(r), h));
    let active_col = app.session.active().map(|doc| doc.selection.active.col);
    let first = match (range, active_col) {
        (Some(r), c) => col_to_letters(c.filter(|c| (r.start.col..=r.end.col).contains(c)).unwrap_or(r.start.col)),
        (None, c) => col_to_letters(c.unwrap_or(0)),
    };
    if header
        && let Some(r) = range
        && r.height() > 1
    {
        let _ = app.run("selection.set", json!({"range": body_of(r).a1()}));
    }
    let mut d =
        Dialog::custom("sort", "Sort", json!({"header": header, "range": range.map(|r| r.a1()), "levels": [{"column": first, "order": "asc"}]}));
    d.sort.restore = restore;
    d.sort.home = home;
    d
}

/// `r` without its header row.
fn body_of(r: RangeRef) -> RangeRef {
    RangeRef::new(CellRef::new(r.start.row.saturating_add(1), r.start.col), r.end)
}

/// Puts back the selection from before Sort opened (once), on the worksheet it opened on.
/// Runs whenever a dialog lets go of the grid.
pub(super) fn restore_selection(app: &mut SheetApp, d: &mut Dialog) {
    if d.name == "sort"
        && app.view_key() == d.sort.home
        && let Some(selection) = d.sort.restore.take()
        && let Some(doc) = app.session.active_mut()
    {
        doc.selection = selection;
    }
}

/// The reason `id` is refused while Sort is open, if it is.
pub(crate) fn refuse_sheet_change(app: &SheetApp, id: &str) -> Option<String> {
    (app.dialog.as_ref().is_some_and(|d| d.name == "sort") && SHEET_COMMANDS.contains(&id))
        .then(|| "Close the Sort dialog before switching or changing worksheets.".into())
}

/// The grid ignores the mouse while Sort is open, so the selection cannot change under the dialog.
pub(crate) fn grid_sense(app: &SheetApp) -> Sense {
    if app.dialog.as_ref().is_some_and(|d| d.name == "sort") { Sense::hover() } else { Sense::click_and_drag() }
}

/// Sort moves and resizes, with a minimum size that fits its controls.
pub(super) fn window<'a>(window: egui::Window<'a>, name: &str) -> egui::Window<'a> {
    if name == "sort" { window.resizable(true).min_width(480.0).min_height(260.0).default_width(500.0).default_height(320.0) } else { window }
}

/// The columns of `range` with their labels: header text, or `Column A`, `Column B`… without headers.
fn column_choices(app: &SheetApp, range: Option<RangeRef>, header: bool) -> Vec<Choice> {
    let Some(r) = range else { return Vec::new() };
    let sh = app.session.active().and_then(|doc| doc.wb.active());
    let body_start = r.start.row.saturating_add(u32::from(header));
    let cols = r.end.col.saturating_sub(r.start.col).saturating_add(1).max(1) as usize;
    // Total sampled cells are bounded across all columns, not per column.
    let rows_per_col = (200_000 / cols).max(1);
    (r.start.col..=r.end.col)
        .map(|c| {
            let letters = col_to_letters(c);
            let text = match (header, sh) {
                (true, Some(sh)) => sh.value(CellRef::new(r.start.row, c)).display().trim().to_string(),
                _ => String::new(),
            };
            let label = if text.is_empty() { format!("Column {letters}") } else { text };
            let numeric =
                sh.is_some_and(|sh| values_are_numeric((body_start..=r.end.row).take(rows_per_col).map(|row| sh.value(CellRef::new(row, c)))));
            (letters, label, numeric)
        })
        .collect()
}

/// The column choices, recomputed only when the sheet, range or header setting changed.
fn cached_choices(app: &SheetApp, d: &mut Dialog, range: Option<RangeRef>, header: bool) -> Vec<Choice> {
    let (doc_uid, sheet_index, revision) =
        app.session.active().map(|doc| (Some(doc.uid), Some(doc.wb.active_sheet()), Some(doc.revision))).unwrap_or_default();
    let range_key = range.map(|r| r.a1());
    if let Some(cache) = &d.sort.choices
        && cache.doc_uid == doc_uid
        && cache.sheet_index == sheet_index
        && cache.revision == revision
        && cache.range == range_key
        && cache.header == header
    {
        return cache.choices.clone();
    }
    let choices = column_choices(app, range, header);
    d.sort.choices = Some(ChoicesCache { doc_uid, sheet_index, revision, range: range_key, header, choices: choices.clone() });
    choices
}

/// Draws the sort-level editor: header toggle, level actions, the level rows and OK/Cancel; OK runs `data.sort`.
pub(super) fn sort(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let mut header = d.values.get("header").and_then(Json::as_bool).unwrap_or(true);
    let was_header = header;
    let range = d.values.get("range").and_then(Json::as_str).and_then(RangeRef::parse);
    let choices = cached_choices(app, d, range, header);
    let mut levels: Vec<Json> = d.values.get("levels").and_then(Json::as_array).cloned().unwrap_or_default();
    let mut selected = d.list_index.min(levels.len().saturating_sub(1));
    let t = Tokens::get(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        // New levels must use an unused column in the selected sort range.
        let used: Vec<&str> = levels.iter().filter_map(|l| l["column"].as_str()).collect();
        let next_column = choices.iter().map(|(l, _, _)| l).find(|l| !used.contains(&l.as_str())).cloned();
        if text_icon_button(ui, Icon::Plus, t.accent, "Add Level", levels.len() < 64 && next_column.is_some()).clicked()
            && let Some(next) = next_column
        {
            levels.push(json!({"column": next, "order": "asc"}));
            selected = levels.len() - 1;
        }
        if text_icon_button(ui, Icon::Close, t.danger, "Delete Level", !levels.is_empty()).clicked() && selected < levels.len() {
            levels.remove(selected);
            selected = selected.saturating_sub(1);
        }
        if text_icon_button(ui, Icon::Copy, t.text, "Copy Level", !levels.is_empty() && levels.len() < 64).clicked()
            && let Some(lv) = levels.get(selected).cloned()
        {
            levels.insert(selected + 1, lv);
            selected += 1;
        }
        if icon_button(ui, Icon::ChevronUp, t.text, "Move Up", selected > 0).clicked() {
            levels.swap(selected, selected - 1);
            selected -= 1;
        }
        if icon_button(ui, Icon::Chevron, t.text, "Move Down", selected + 1 < levels.len()).clicked() {
            levels.swap(selected, selected + 1);
            selected += 1;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.checkbox(&mut header, "My list has headers");
        });
    });
    d.values.insert("header".into(), json!(header));
    if header != was_header
        && let Some(r) = range
    {
        let shown = if header && r.height() > 1 { body_of(r) } else { r };
        let _ = app.run("selection.set", json!({"range": shown.a1()}));
    }
    ui.add_space(4.0);
    let table_height = (ui.available_height() - 48.0).max(60.0);
    egui::ScrollArea::vertical().max_height(table_height).min_scrolled_height(table_height).auto_shrink([false, false]).show(ui, |ui| {
        levels_table(ui, &mut levels, &mut selected, &choices);
    });
    d.list_index = selected;
    d.values.insert("levels".into(), Json::Array(levels.clone()));
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    if *confirm && !levels.is_empty() {
        let mut p = json!({"header": header, "keys": levels});
        if let Some(r) = d.values.get("range").and_then(Json::as_str) {
            p["range"] = json!(r);
        }
        // The range has no sheet name, so it must not be read against another worksheet.
        let failure = if app.view_key() != d.sort.home {
            Some("Return to the original worksheet to sort, or cancel.".to_string())
        } else {
            app.run("data.sort", p).err().map(|e| crate::clean_error(&e))
        };
        if let Some(message) = failure {
            app.message = Some((d.title.clone(), message));
            *confirm = false;
        }
    }
    if !open {
        *confirm = true;
    }
}

/// The selectable level rows: "Sort by"/"Then by" with the column, sort-on and order pickers.
fn levels_table(ui: &mut egui::Ui, levels: &mut [Json], selected: &mut usize, choices: &[Choice]) {
    let gap = ui.spacing().item_spacing.x;
    let label_w = 64.0;
    let row_h = 26.0;
    let rest = (ui.available_width() - 16.0 - label_w - 3.0 * gap).max(300.0);
    let w = rest / 3.0;
    let cell = |ui: &mut egui::Ui, w: f32, h: f32, add: &mut dyn FnMut(&mut egui::Ui)| {
        ui.allocate_ui_with_layout(vec2(w, h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(vec2(w, h));
            add(ui);
        });
    };
    ui.horizontal(|ui| {
        cell(ui, label_w + gap + w, 18.0, &mut |ui| {
            ui.strong("Column");
        });
        cell(ui, w, 18.0, &mut |ui| {
            ui.strong("Sort On");
        });
        cell(ui, w, 18.0, &mut |ui| {
            ui.strong("Order");
        });
    });
    let accent = Tokens::get(ui.ctx()).accent;
    for (i, lv) in levels.iter_mut().enumerate() {
        let row_rect = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), row_h));
        let row = ui.interact(row_rect, ui.scope_id().with(("sortRow", i)), Sense::click());
        if i == *selected {
            ui.painter().rect_filled(row_rect, 3.0, accent.gamma_multiply(0.18));
        } else if row.hovered() {
            ui.painter().rect_filled(row_rect, 3.0, accent.gamma_multiply(0.07));
        }
        if row.clicked() {
            *selected = i;
        }
        ui.allocate_ui_with_layout(row_rect.size(), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            cell(ui, label_w, row_h, &mut |ui| {
                ui.add(egui::Label::new(if i == 0 { "Sort by" } else { "Then by" }).selectable(false));
            });
            let mut col = lv["column"].as_str().unwrap_or("A").to_string();
            let shown = choices.iter().find(|(l, _, _)| *l == col).map(|(_, t, _)| t.clone()).unwrap_or_else(|| format!("Column {col}"));
            let r = craft_ui::ComboBox::from_id_salt(("col", i)).selected_text(shown).width(w).show_ui(ui, |ui| {
                for (letters, label, _) in choices {
                    ui.selectable_value(&mut col, letters.clone(), label);
                }
            });
            r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sort column"));
            if r.response.clicked() {
                *selected = i;
            }
            lv["column"] = json!(col);
            let mut by = lv["by"].as_str().unwrap_or("values").to_string();
            let by_label = match by.as_str() {
                "cellColor" => "Cell Color",
                "fontColor" => "Font Color",
                _ => "Cell Values",
            };
            let r = craft_ui::ComboBox::from_id_salt(("by", i)).selected_text(by_label).width(w).show_ui(ui, |ui| {
                ui.selectable_value(&mut by, "values".into(), "Cell Values");
                ui.selectable_value(&mut by, "cellColor".into(), "Cell Color");
                ui.selectable_value(&mut by, "fontColor".into(), "Font Color");
            });
            r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sort on"));
            if r.response.clicked() {
                *selected = i;
            }
            lv["by"] = json!(by);
            let numeric = choices.iter().find(|(l, _, _)| lv["column"].as_str() == Some(l.as_str())).is_some_and(|(_, _, n)| *n);
            let (asc_label, desc_label) = if numeric { ("Smallest to Largest", "Largest to Smallest") } else { ("A to Z", "Z to A") };
            let mut desc = lv["order"].as_str() == Some("desc");
            let order_label = if desc { desc_label } else { asc_label };
            let r = craft_ui::ComboBox::from_id_salt(("order", i)).selected_text(order_label).width(w).show_ui(ui, |ui| {
                ui.selectable_value(&mut desc, false, asc_label);
                ui.selectable_value(&mut desc, true, desc_label);
            });
            r.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Sort order"));
            if r.response.clicked() {
                *selected = i;
            }
            lv["order"] = json!(if desc { "desc" } else { "asc" });
        });
    }
}

/// A bordered icon-only button with a tooltip, greyed out when disabled.
fn icon_button(ui: &mut egui::Ui, icon: Icon, color: egui::Color32, tip: &str, enabled: bool) -> egui::Response {
    let color = if enabled { color } else { Tokens::get(ui.ctx()).text_disabled };
    let response = ui.add_enabled_ui(enabled, |ui| crate::widgets::icon_button(ui, icon, color, tip, vec2(24.0, 24.0))).inner;
    button_border(ui, response.rect);
    response
}

/// A bordered button with a coloured icon and a text label, greyed out when disabled.
fn text_icon_button(ui: &mut egui::Ui, icon: Icon, color: egui::Color32, label: &str, enabled: bool) -> egui::Response {
    let response = ui.add_enabled_ui(enabled, |ui| crate::widgets::small_button_accent(ui, icon, Some(color), label, label, false)).inner;
    button_border(ui, response.rect);
    response
}

/// Draws the visible border around a button's `rect`.
fn button_border(ui: &egui::Ui, rect: egui::Rect) {
    let t = Tokens::get(ui.ctx());
    ui.painter().rect_stroke(rect, 4.0, egui::Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
}

/// Whether the values are numbers (blanks aside), which picks "Smallest to Largest" over "A to Z".
fn values_are_numeric(values: impl IntoIterator<Item = Value>) -> bool {
    let mut has_number = false;
    for value in values {
        match value {
            Value::Empty => {}
            Value::Number(_) => has_number = true,
            _ => return false,
        }
    }
    has_number
}
