use super::{Icon, Rect, Sense, SheetApp, Stroke, StrokeKind, Style, Tokens, Ui, keytip_table, pos2, theme, vec2};

/// Height of the controls row in the ribbon card; group captions sit below it.
pub(super) const GROUP_H: f32 = 62.0;
/// Height of a caption line under each group. Groups carry no visible caption (their names stay
/// in the code and for assistive technology), so the line takes no room.
pub(super) const CAPTION_H: f32 = 0.0;

/// Pitch of a two-row group (Font, Alignment, Number): each row is `ROW_H` tall with its controls
/// centred, and the pair sits centred in the group.
const ROW_H: f32 = 26.0;
const ROW_GAP: f32 = 4.0;

/// Lays out a two-row group: rows built with `ui.horizontal` inside get the design's 32 px pitch
/// and the pair is centred vertically in the card.
pub(crate) fn two_rows<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().interact_size.y = ROW_H;
        ui.spacing_mut().item_spacing.y = ROW_GAP;
        ui.add_space(((GROUP_H - 2.0 * ROW_H - ROW_GAP) / 2.0).max(0.0));
        add(ui)
    })
    .inner
}

fn group_start_id() -> egui::Id {
    egui::Id::unique("ribbon_group_start")
}

/// Marks where the first group of a tab begins, so the captions can be centred under their groups.
pub(super) fn begin_groups(ui: &Ui) {
    let x = ui.cursor().min.x;
    ui.ctx().data_mut(|d| d.insert_temp(group_start_id(), x));
}

/// Paints `caption` centred under the group that spans from the last group start to `end`.
fn paint_caption(ui: &Ui, caption: &str, end: f32) {
    if CAPTION_H <= 0.0 {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let start = ui.ctx().data(|d| d.get_temp::<f32>(group_start_id())).unwrap_or(end);
    let top = ui.min_rect().top();
    let label = crate::i18n::current(ui.ctx()).tr(caption);
    let center = pos2((start + end) / 2.0, top + GROUP_H + CAPTION_H / 2.0 + 1.0);
    ui.painter().text(center, egui::Align2::CENTER_CENTER, label, theme::ui_font(11.0), t.text_dim);
}

/// Ends a ribbon group: its caption under it and a separator line before the next group.
pub(crate) fn sep(ui: &mut Ui, caption: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, GROUP_H), Sense::hover());
    paint_caption(ui, caption, r.left());
    let bottom = r.top() + GROUP_H + CAPTION_H;
    ui.painter().line_segment([pos2(r.center().x, r.top() + 4.0), pos2(r.center().x, bottom - 2.0)], Stroke::new(1.0, t.separator));
    ui.ctx().data_mut(|d| d.insert_temp(group_start_id(), r.right()));
}

/// Ends a tab's last group: its caption, without a separator after it.
pub(super) fn end_group(ui: &mut Ui, caption: &str) {
    let end = ui.min_rect().right();
    paint_caption(ui, caption, end);
}

pub(super) fn active_style(app: &SheetApp) -> Style {
    app.session
        .active()
        .and_then(|d| {
            let sh = d.wb.active()?;
            Some(d.wb.styles.get(sh.style_id(d.selection.active)).clone())
        })
        .unwrap_or_default()
}

pub(super) fn theme_colors(app: &SheetApp) -> [u32; 12] {
    app.session.active().map(|d| d.wb.theme.colors).unwrap_or(gridcraft_engine::model::Theme::default().colors)
}

/// Runs a command from a ribbon click (menu-style: no params may open a dialog).
pub(super) fn act(app: &mut SheetApp, id: &str, params: serde_json::Value) {
    app.run_or_alert(id, params);
}

/// Keytips are painted, not focusable widgets: the grid retains keyboard focus.
pub(super) fn keytip(app: &SheetApp, ui: &Ui, sequence: &str, at: egui::Pos2) {
    let Some(rest) = app.keytips.prefix().and_then(|prefix| sequence.strip_prefix(prefix)).filter(|s| !s.is_empty()) else { return };
    let t = Tokens::get(ui.ctx());
    let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("ribbon_keytips"))).with_clip_rect(ui.clip_rect());
    let text = painter.layout_no_wrap(rest.into(), theme::ui_bold(10.5), t.text);
    let rect = Rect::from_center_size(at, text.size() + vec2(7.0, 3.0));
    painter.rect_filled(rect, 2.0, t.input_bg);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
    painter.galley(rect.min + vec2(3.5, 1.5), text, t.text);
}

/// Paint the badge for a control that runs command `id`, when the armed prefix matches.
pub(super) fn control_keytip(app: &SheetApp, ui: &Ui, response: &egui::Response, id: &str) {
    if let Some(sequence) = keytip_table::control_badge(app.keytips.prefix().unwrap_or_default(), id) {
        keytip(app, ui, sequence, response.rect.center_bottom());
    }
}

/// Paint the single badge for a menu anchor whose popup items run `targets`.
pub(super) fn anchor_keytip(app: &SheetApp, ui: &Ui, response: &egui::Response, targets: &[&str]) {
    if let Some(sequence) = keytip_table::anchor_badge(app.keytips.prefix().unwrap_or_default(), targets) {
        keytip(app, ui, sequence, response.rect.center_bottom());
    }
}

/// The `on` value for a ribbon checkbox command. A click has already flipped the checkbox bool, so
/// it is sent as is; a key tip leaves the bool untouched, so the command gets the opposite.
pub(super) fn checkbox_on(clicked: bool, shown: bool) -> bool {
    shown == clicked
}

/// Consume a one-shot keytip request for control `id` (set by `Action::Control`).
pub(super) fn invoke(app: &mut SheetApp, id: &str) -> bool {
    if app.ui.keytip_invoke == Some(id) {
        app.ui.keytip_invoke = None;
        true
    } else {
        false
    }
}

/// Force-open command for a popup: either a keytip invoke (open it) or a click (toggle).
pub(super) fn popup_command(forced: bool, clicked: bool) -> Option<egui::SetOpenCommand> {
    if forced { Some(egui::SetOpenCommand::Bool(true)) } else { clicked.then_some(egui::SetOpenCommand::Toggle) }
}

/// Per-control state of an [`editable_combo`]: the filter text and highlighted row.
#[derive(Clone, Default)]
struct ComboState {
    text: String,
    sel: i32,
    was_open: bool,
}

/// What the user did with an [`editable_combo`].
pub(crate) enum ComboResult {
    None,
    /// A row was applied: clicked or Enter on the highlighted row.
    Picked(String),
}

/// Excel-style dropdown: a filter field (type to narrow the list) plus arrow/Enter selection.
/// `items` are `(value, display)`; the value is what comes back. When `forced` (keytip invoke)
/// or clicked, the popup opens, focuses the filter field, and selects its text.
pub(crate) fn editable_combo(
    ui: &mut Ui,
    anchor: &egui::Response,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
    forced: bool,
) -> ComboResult {
    combo_popup(ui, anchor, salt, current_value, items, ComboPopupOptions { forced, filter: true, under: None })
}

/// The dropdown of [`editable_combo`] without the filter field: the whole list, picked by click or
/// arrows and Enter. It drops directly below `under` and is as wide as it, like Excel's Font Size
/// list under its box.
pub(super) fn list_combo(
    ui: &mut Ui,
    anchor: &egui::Response,
    under: Rect,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
) -> ComboResult {
    combo_popup(ui, anchor, salt, current_value, items, ComboPopupOptions { forced: false, filter: false, under: Some(under) })
}

/// What the header of a combo popup hands to its list: the rows to show and the highlighted one.
struct ComboHead {
    filtered: Vec<usize>,
    sel: i32,
    down: bool,
    up: bool,
    enter: bool,
}

struct ComboPopupOptions {
    forced: bool,
    filter: bool,
    under: Option<Rect>,
}

/// Arrow and Enter keys (taken out of the input so a filter field never sees them), the filter
/// field when `text` is given, and the rows its text matches.
fn combo_head(ui: &mut Ui, text: Option<(&mut String, egui::Id)>, open_now: bool, sel: i32, items: &[(&str, String)]) -> ComboHead {
    let (mut down, mut up, mut enter) = (false, false, false);
    ui.input(|i| {
        down = i.key_pressed(egui::Key::ArrowDown);
        up = i.key_pressed(egui::Key::ArrowUp);
        enter = i.key_pressed(egui::Key::Enter);
    });
    if down || up || enter {
        ui.ctx().input_mut(|i| {
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::ArrowDown | egui::Key::ArrowUp | egui::Key::Enter, pressed: true, .. }));
        });
    }
    let mut lower = String::new();
    if let Some((text, text_id)) = text {
        let te = ui.add(egui::TextEdit::singleline(text).id(text_id).desired_width(220.0).hint_text("Type to filter…"));
        if open_now && !te.has_focus() {
            ui.ctx().memory_mut(|m| m.request_focus(text_id));
            // Select all once focus lands so typing replaces the filter.
            if let Some(mut s) = egui::text_edit::TextEditState::load(ui.ctx(), text_id) {
                let len = text.chars().count();
                s.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(len))));
                s.store(ui.ctx(), text_id);
            }
        }
        ui.separator();
        lower = text.to_lowercase();
    }
    let filtered: Vec<usize> = items.iter().enumerate().filter(|(_, (_, d))| d.to_lowercase().contains(&lower)).map(|(i, _)| i).collect();
    let mut sel = sel;
    if !filtered.is_empty() {
        let n = filtered.len() as i32;
        sel = sel.clamp(0, n - 1);
        if down {
            sel = (sel + 1).rem_euclid(n);
        }
        if up {
            sel = (sel - 1).rem_euclid(n);
        }
    }
    ComboHead { filtered, sel, down, up, enter }
}

/// The rows of a combo popup. Returns the picked value and the highlighted row.
fn combo_rows(ui: &mut Ui, head: ComboHead, items: &[(&str, String)], row_w: f32) -> (Option<String>, i32) {
    let ComboHead { filtered, mut sel, down, up, enter } = head;
    let mut picked = None;
    if enter && !filtered.is_empty() {
        let idx = filtered[sel.clamp(0, filtered.len() as i32 - 1) as usize];
        picked = Some(items[idx].0.to_string());
    }
    for (pos, i) in filtered.iter().enumerate() {
        let (value, disp) = &items[*i];
        let row = ui.add(egui::Button::selectable(pos as i32 == sel, egui::RichText::new(disp)).min_size(vec2(row_w, 20.0)));
        if (down || up) && pos as i32 == sel && !ui.clip_rect().contains_rect(row.rect) {
            row.scroll_to_me(Some(if down { egui::Align::Max } else { egui::Align::Min }));
        }
        if row.clicked() {
            picked = Some(value.to_string());
        }
        if row.hovered() {
            sel = pos as i32;
        }
    }
    if picked.is_some() {
        ui.close();
    }
    (picked, sel)
}

fn combo_popup(
    ui: &mut Ui,
    anchor: &egui::Response,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, String)],
    options: ComboPopupOptions,
) -> ComboResult {
    let ComboPopupOptions { forced, filter, under } = options;
    let popup_id = egui::Popup::default_response_id(anchor);
    let state_id = anchor.id.with(salt).with("__combo_state");
    let mut st = ui.data_mut(|d| d.get_temp::<ComboState>(state_id)).unwrap_or_default();
    if forced {
        egui::Popup::open_id(ui.ctx(), popup_id);
    }
    let open_now = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if open_now && !st.was_open {
        st.text = String::new();
        st.sel = items.iter().position(|(v, _)| *v == current_value).map(|i| i as i32).unwrap_or(0);
        // The reset lands on the frame AFTER the popup opens, which usually has no input of
        // its own — without a repaint request the stale filtered list stays on screen.
        ui.ctx().request_repaint();
    }
    st.was_open = open_now;
    // Below `under` and as wide as it, or below the anchor itself.
    let mut field = anchor.clone();
    if let Some(r) = under {
        field.rect = r;
    }
    let row_w = under.map_or(220.0, |r| r.width() - 12.0);
    // Nine rows as today, plus the filter field and its separator, plus the popup frame.
    let button_h = ui.spacing().interact_size.y.max(20.0);
    let spacing = ui.spacing().item_spacing.y;
    let max_rows = 9.0_f32;
    let mut height = max_rows * button_h + (max_rows - 1.0) * spacing + egui::Frame::popup(ui.style()).total_margin().sum().y;
    if filter {
        height += ui.spacing().interact_size.y + 4.0 * spacing + 1.0;
    }
    let menu = craft_ui::combo::Menu::new(ui, &field).height(height);
    let text_id = anchor.id.with(salt).with("__combo_text");
    let sel0 = st.sel;
    let shown = if filter {
        menu.show_ui_with_header(
            |ui| combo_head(ui, Some((&mut st.text, text_id)), open_now, sel0, items),
            |ui, head| combo_rows(ui, head, items, row_w),
        )
    } else {
        menu.show_ui(|ui| {
            let head = combo_head(ui, None, open_now, sel0, items);
            combo_rows(ui, head, items, row_w)
        })
    };
    let mut result = ComboResult::None;
    if let Some((picked, sel)) = shown {
        st.sel = sel;
        if let Some(value) = picked {
            result = ComboResult::Picked(value);
        }
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
    result
}

/// The icon a menu item shows in its icon column: the command's own ribbon icon where it has one.
pub(super) fn menu_icon(id: &str, p: &serde_json::Value) -> Option<Icon> {
    let has = |key: &str| p.get(key).is_some();
    Some(match id {
        "edit.paste" | "dialog:pasteSpecial" => Icon::Paste,
        "edit.pasteSpecial" if has("link") => Icon::Link,
        "edit.pasteSpecial" if has("transpose") => Icon::Orientation,
        "edit.pasteSpecial" => match p.get("what").and_then(serde_json::Value::as_str) {
            Some("formulas") => Icon::Fx,
            Some("formats") => Icon::Brush,
            _ => Icon::Paste,
        },
        "home.underline" if p.get("style").and_then(serde_json::Value::as_str) != Some("double") => Icon::Underline,
        "home.mergeCenter" => Icon::Merge,
        "dialog:insertCells" | "home.insertRows" | "home.insertColumns" => Icon::Insert,
        "home.insertSheet" => Icon::Sheet,
        "dialog:deleteCells" | "home.deleteRows" | "home.deleteColumns" | "home.deleteSheet" => Icon::DeleteCells,
        "dialog:formatCells" if p.get("tab").is_none() => Icon::Format,
        "dialog:protectSheet" => Icon::ProtectSheet,
        "home.lockCell" => Icon::Lock,
        "dialog:renameSheet" | "dialog:moveSheet" => Icon::Sheet,
        "edit.fillDown" => Icon::FillDown,
        "edit.flashFill" => Icon::FlashFill,
        "edit.clearAll" | "edit.clearContents" => Icon::Eraser,
        "edit.clearFormats" => Icon::Brush,
        "edit.clearComments" => Icon::DeleteComment,
        "edit.clearHyperlinks" => Icon::Link,
        "data.sortAscending" => Icon::SortAsc,
        "data.sortDescending" => Icon::SortDesc,
        "dialog:sort" => Icon::SortCustom,
        "data.filter" => Icon::Filter,
        "data.clearFilter" => Icon::ClearFilter,
        "data.reapply" => Icon::Reapply,
        "dialog:find" => Icon::Find,
        "home.clearRules" => Icon::Eraser,
        "dialog:dataValidation" => Icon::Validation,
        "dialog:goalSeek" | "data.scenarioManager" | "data.dataTable" => Icon::WhatIf,
        "view.freezePanes" | "view.freezeTopRow" | "view.freezeFirstColumn" => Icon::Freeze,
        "pageLayout.printArea" if !has("clear") => Icon::PrintArea,
        "pageLayout.breaks" if has("insert") => Icon::Breaks,
        "dialog:pageSetup" => Icon::Margins,
        "dialog:comment" => Icon::Note,
        "review.showAllNotes" => Icon::ShowComments,
        _ => return None,
    })
}

/// The keyboard shortcut a menu item shows at its right: one GridCraft binds for that exact
/// command, written for this platform as the ribbon tooltips write them.
pub(super) fn menu_shortcut(id: &str, p: &serde_json::Value) -> Option<&'static str> {
    let plain = p.as_object().is_none_or(|o| o.is_empty());
    let mac = cfg!(target_os = "macos");
    let pick = |m: &'static str, other: &'static str| Some(if mac { m } else { other });
    match id {
        "edit.paste" => pick("⌘V", "Ctrl+V"),
        "dialog:find" if p.get("replace").is_some() => pick("⌘H", "Ctrl+H"),
        "dialog:find" => pick("⌘F", "Ctrl+F"),
        "dialog:goTo" => pick("⌘G", "Ctrl+G"),
        "dialog:formatCells" if plain => pick("⌘1", "Ctrl+1"),
        "edit.fillDown" => pick("⌘D", "Ctrl+D"),
        "edit.fillRight" => pick("⌘R", "Ctrl+R"),
        "edit.flashFill" => pick("⌘E", "Ctrl+E"),
        "edit.clearContents" => Some("Delete"),
        "data.filter" => pick("⌘⇧F", "Ctrl+Shift+L"),
        "home.hideRows" => pick("⌘9", "Ctrl+9"),
        "home.hideColumns" => pick("⌘0", "Ctrl+0"),
        "home.unhideRows" => pick("⌘⇧9", "Ctrl+Shift+9"),
        "home.unhideColumns" => pick("⌘⇧0", "Ctrl+Shift+0"),
        "home.underline" if p.get("style").and_then(serde_json::Value::as_str) == Some("single") => pick("⌘U", "Ctrl+U"),
        _ => None,
    }
}

/// A shortcut the interface fonts can draw (a host without a macOS symbol face gets the
/// spelled-out keys rather than tofu).
pub(super) fn drawable_shortcut(ui: &Ui, keys: &str) -> String {
    if keys.chars().any(|c| theme::symbol_fallback(c).is_some()) { theme::available_text(ui, keys) } else { keys.to_string() }
}

pub(super) fn menu_items(app: &mut SheetApp, ui: &mut Ui, items: &[(&str, &str, serde_json::Value)]) {
    for (label, id, p) in items {
        if *label == "-" {
            crate::widgets::menu_gap(ui);
            continue;
        }
        let lang = app.ui.language;
        let keys = menu_shortcut(id, p).map(|k| drawable_shortcut(ui, k));
        let response = crate::widgets::menu_row(ui, menu_icon(id, p), lang.tr(label), keys.as_deref());
        if let Some(sequence) = keytip_table::item_letter(app.keytips.prefix().unwrap_or_default(), id) {
            keytip(app, ui, sequence, pos2(response.rect.right() - 10.0, response.rect.center().y));
        }
        if response.clicked() {
            if let Some(d) = id.strip_prefix("dialog:") {
                app.open_dialog(d, p.clone());
            } else if *id == "edit.paste" {
                app.paste_from_clipboard();
            } else {
                act(app, id, p.clone());
            }
        }
    }
}

#[cfg(test)]
mod checkbox_on_tests {
    use super::*;

    #[test]
    fn click_sends_the_new_checkbox_value_and_key_tip_the_opposite() {
        // Click: the checkbox bool already holds the new choice.
        assert!(!checkbox_on(true, false));
        assert!(checkbox_on(true, true));
        // Key tip: the bool still holds the current setting, so toggle it.
        assert!(checkbox_on(false, false));
        assert!(!checkbox_on(false, true));
    }
}

#[cfg(test)]
mod menu_decoration_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn menu_items_show_only_shortcuts_gridcraft_binds_for_that_exact_command() {
        assert!(menu_shortcut("edit.fillDown", &json!({})).is_some());
        if !cfg!(target_os = "macos") {
            assert_eq!(menu_shortcut("dialog:find", &json!({"replace": true})), Some("Ctrl+H"));
            assert_eq!(menu_shortcut("dialog:formatCells", &json!({})), Some("Ctrl+1"));
        }
        // Format Cells opened on a tab is not what Ctrl+1 does.
        assert_eq!(menu_shortcut("dialog:formatCells", &json!({"tab": "Alignment"})), None);
        assert_eq!(menu_shortcut("home.underline", &json!({"style": "double"})), None);
        assert_eq!(menu_shortcut("pageLayout.margins", &json!({"preset": "wide"})), None);
    }

    #[test]
    fn menu_items_take_their_commands_icons() {
        assert_eq!(menu_icon("data.sortAscending", &json!({})), Some(Icon::SortAsc));
        assert_eq!(menu_icon("edit.pasteSpecial", &json!({"what": "formulas"})), Some(Icon::Fx));
        assert_eq!(menu_icon("edit.pasteSpecial", &json!({"what": "all", "link": true})), Some(Icon::Link));
        assert_eq!(menu_icon("home.insertSheet", &json!({})), Some(Icon::Sheet));
        assert_eq!(menu_icon("edit.goToSpecial", &json!({"kind": "notes"})), None);
    }
}
