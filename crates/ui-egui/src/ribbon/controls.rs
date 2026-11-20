use super::{Rect, Sense, SheetApp, Stroke, StrokeKind, Style, Tokens, Ui, keytip_table, pos2, theme, vec2};

pub(super) fn sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 6.0), pos2(r.center().x, r.bottom() - 6.0)], Stroke::new(1.0, t.separator));
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
    let Some(rest) = app
        .keytips
        .prefix()
        .filter(|prefix| keytip_table::paints_badges(prefix))
        .and_then(|prefix| sequence.strip_prefix(prefix))
        .filter(|s| !s.is_empty())
    else {
        return;
    };
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

/// Per-control state of an [`editable_combo`]: the filter text, the highlighted row, the value
/// that row holds, and the item values it last saw (a list can change while its popup is open).
#[derive(Clone, Default)]
struct ComboState {
    text: String,
    sel: i32,
    was_open: bool,
    values: Vec<String>,
    /// The highlighted row's value, remembered by value rather than position: when the item
    /// list changes under an open popup, the rematch follows this value to its new position.
    highlight: Option<String>,
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
    items: &[(&str, &str)],
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
    items: &[(&str, &str)],
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

/// The rows of `items` a filter text shows: the indexes whose display contains the text,
/// ignoring case (an empty filter shows every row). This is the one definition of "filtered" —
/// the popup header builds its row list with it, and the selection rematch reads positions in
/// the list it returns, so the two can never disagree about which row a position means.
fn filtered_positions(items: &[(&str, &str)], filter: &str) -> Vec<usize> {
    let lower = filter.to_lowercase();
    items.iter().enumerate().filter(|(_, (_, d))| d.to_lowercase().contains(&lower)).map(|(i, _)| i).collect()
}

/// Where the highlight goes when a combo's item list changes under an open popup: a position in
/// `filtered` (the rows actually shown), never in the full list. The row highlighted before the
/// change wins, so a list growing under the user's navigation — the font scan landing — keeps
/// their row and only moves its position; when that value is no longer shown, the current value
/// takes over, and when neither is shown the highlight restarts at the first shown row.
fn rematch_selection(items: &[(&str, &str)], filtered: &[usize], highlight: Option<&str>, current_value: &str) -> i32 {
    let position_of = |wanted: &str| filtered.iter().position(|&i| items.get(i).is_some_and(|(v, _)| *v == wanted)).map(|p| p as i32);
    highlight.and_then(position_of).or_else(|| position_of(current_value)).unwrap_or(0)
}

/// Arrow and Enter keys (taken out of the input so a filter field never sees them), the filter
/// field when `text` is given, and the rows its text matches.
fn combo_head(ui: &mut Ui, text: Option<(&mut String, egui::Id)>, open_now: bool, sel: i32, items: &[(&str, &str)]) -> ComboHead {
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
    let mut filter = String::new();
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
        filter = text.clone();
    }
    let filtered = filtered_positions(items, &filter);
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

/// The rows of a combo popup. Returns the picked value, the highlighted row, and the filtered
/// row list the highlight is a position in (so the caller can remember the highlighted value).
fn combo_rows(ui: &mut Ui, head: ComboHead, items: &[(&str, &str)], row_w: f32) -> (Option<String>, i32, Vec<usize>) {
    let ComboHead { filtered, mut sel, down, up, enter } = head;
    let mut picked = None;
    if enter && !filtered.is_empty() {
        let idx = filtered[sel.clamp(0, filtered.len() as i32 - 1) as usize];
        picked = Some(items[idx].0.to_string());
    }
    for (pos, i) in filtered.iter().enumerate() {
        let (value, disp) = items[*i];
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
    (picked, sel, filtered)
}

/// Whether the item values a combo last saw differ from `items`: a different length, or any
/// value at the same position, compared in place without building the new list. Displays do
/// not count — a list whose previews changed but whose values did not needs no rematch.
fn values_changed(old: &[String], items: &[(&str, &str)]) -> bool {
    old.len() != items.len() || old.iter().zip(items.iter()).any(|(o, (v, _))| o.as_str() != *v)
}

/// What an open combo does with its state before it draws, given the item list as it now
/// stands. On a fresh open, the previous session is forgotten (filter, remembered highlight)
/// and the rematch starts from `current_value`; on a list change under an open popup, the
/// highlight is rematched (see [`rematch_selection`]) and the new values are stored. Returns
/// whether a repaint is needed: a fresh open's reset lands on the frame after the popup opens,
/// which usually has no input of its own, and without the request the stale filtered list
/// would stay on screen. A closed popup skips all of this — reopening rematches anyway — and
/// only records that it is closed.
fn sync_open_combo(st: &mut ComboState, items: &[(&str, &str)], current_value: &str, open_now: bool) -> bool {
    let mut repaint = false;
    if open_now {
        // Change detection without allocating: the item values are compared against the stored
        // ones in place, and the stored copy is only rebuilt when the list actually changed.
        let changed = values_changed(&st.values, items);
        if !st.was_open || changed {
            if !st.was_open {
                st.text = String::new();
                // A fresh popup has no navigation to preserve: the rematch starts from the
                // current value, not from wherever a previous popup session left its highlight.
                st.highlight = None;
                repaint = true;
            }
            // Rematch on a changed list too, not just on open: the font list grows when the
            // background font scan lands, and a highlight left at its old position would sit on
            // a different row, so Enter would apply that row instead. The position is computed
            // against the filtered rows (under the filter as it stands — a mid-open change does
            // not clear what the user typed), and the previously highlighted value wins over
            // the current value, so navigation in progress is not reset by the scan landing.
            let filtered = filtered_positions(items, &st.text);
            st.sel = rematch_selection(items, &filtered, st.highlight.as_deref(), current_value);
            if changed {
                st.values = items.iter().map(|(v, _)| (*v).to_owned()).collect();
            }
        }
    }
    st.was_open = open_now;
    repaint
}

/// The value the highlight points at: `sel` is a position in `filtered` (the rows shown), so
/// resolving it back goes through the filtered rows to the item. None when that position shows
/// nothing (no rows shown, or a stale position past the last shown row).
fn highlighted_value(items: &[(&str, &str)], filtered: &[usize], sel: i32) -> Option<String> {
    filtered.get(sel.max(0) as usize).and_then(|&i| items.get(i)).map(|(v, _)| (*v).to_string())
}

/// The engine behind [`editable_combo`] and [`list_combo`]: the popup drawn under the anchor,
/// the state it keeps across frames (a `ComboState` in the egui memory: the filter text, the
/// highlight and the item values it last saw) and the `ComboResult` the caller acts on. An open
/// popup whose item list changes — the font scan landing under the font combo — rematches its
/// highlight against the new list (see [`rematch_selection`]) instead of leaving it at a stale
/// position, so Enter still applies the row the user sees highlighted.
fn combo_popup(
    ui: &mut Ui,
    anchor: &egui::Response,
    salt: &'static str,
    current_value: &str,
    items: &[(&str, &str)],
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
    if sync_open_combo(&mut st, items, current_value, open_now) {
        ui.ctx().request_repaint();
    }
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
    if let Some((picked, sel, filtered)) = shown {
        st.sel = sel;
        // Remember the highlighted value for the next list change: `sel` is a position in the
        // filtered rows just shown, so resolve it back to the item it points at.
        st.highlight = highlighted_value(items, &filtered, sel);
        if let Some(value) = picked {
            result = ComboResult::Picked(value);
        }
    }
    ui.data_mut(|d| d.insert_temp(state_id, st));
    result
}

pub(super) fn menu_items(app: &mut SheetApp, ui: &mut Ui, items: &[(&str, &str, serde_json::Value)]) {
    for (label, id, p) in items {
        if *label == "-" {
            ui.separator();
            continue;
        }
        let lang = app.ui.language;
        let response = ui.add(egui::Button::new(lang.tr(label)).frame_when_inactive(false).min_size(vec2(220.0, 22.0)));
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
mod combo_rematch_tests {
    use super::*;

    #[test]
    fn the_rematch_position_is_in_the_filtered_list_not_the_full_one() {
        // Only "Meiryo" matches the filter, so its row is filtered position 0 — the old code
        // returned its full-list position (2), highlighting a row that is not even shown.
        let items = [("Alpha", "Alpha"), ("Beta", "Beta"), ("Meiryo", "Meiryo"), ("Zebra", "Zebra")];
        let filtered = filtered_positions(&items, "me");
        assert_eq!(filtered, [2]);
        assert_eq!(rematch_selection(&items, &filtered, None, "Meiryo"), 0);
    }

    #[test]
    fn rows_inserted_before_the_highlight_keep_it_on_the_same_value() {
        // The font scan lands while the popup is open: new families sort in before the row the
        // user had navigated to. The highlight follows the value to its new position, even
        // though the current value is a different row.
        let items = [("Arial", "Arial"), ("Beta", "Beta"), ("Cambria", "Cambria"), ("Meiryo", "Meiryo"), ("Zebra", "Zebra")];
        let filtered = filtered_positions(&items, "");
        assert_eq!(rematch_selection(&items, &filtered, Some("Meiryo"), "Beta"), 3);
    }

    #[test]
    fn a_highlight_that_is_gone_falls_back_to_the_current_value() {
        // Filtered position again: "Tahoma" is full-list index 2 but filtered position 1.
        let items = [("Arial", "Arial"), ("Meiryo", "Meiryo"), ("Tahoma", "Tahoma")];
        let filtered = filtered_positions(&items, "a");
        assert_eq!(filtered, [0, 2]);
        assert_eq!(rematch_selection(&items, &filtered, Some("Gone"), "Tahoma"), 1);
    }

    #[test]
    fn a_highlight_and_current_value_both_gone_restart_at_the_first_shown_row() {
        let items = [("Arial", "Arial"), ("Meiryo", "Meiryo"), ("Tahoma", "Tahoma")];
        let filtered = filtered_positions(&items, "");
        assert_eq!(rematch_selection(&items, &filtered, Some("Gone"), "Also Gone"), 0);
        // Nothing shown at all still yields a valid (clamped downstream) position.
        assert_eq!(rematch_selection(&items, &[], Some("Meiryo"), "Meiryo"), 0);
    }
}

#[cfg(test)]
mod combo_sync_tests {
    use super::*;

    /// A state as a previous popup session left it: a typed filter, a moved highlight, and
    /// the values of the (shorter) list that session saw.
    fn left_over_state() -> ComboState {
        ComboState { text: "me".to_string(), sel: 2, was_open: false, values: vec!["Alpha".into(), "Beta".into()], highlight: Some("Beta".into()) }
    }

    #[test]
    fn a_closed_popup_keeps_its_state_and_stale_values() {
        let items = [("Alpha", "Alpha"), ("Beta", "Beta"), ("Meiryo", "Meiryo")];
        let mut st = left_over_state();
        st.was_open = true;
        assert!(!sync_open_combo(&mut st, &items, "Alpha", false));
        // Nothing is rematched or stored while closed; reopening rematches anyway.
        assert_eq!(st.text, "me");
        assert_eq!(st.sel, 2);
        assert_eq!(st.values.len(), 2);
        assert!(!st.was_open);
    }

    #[test]
    fn a_fresh_open_forgets_the_previous_session_and_starts_from_the_current_value() {
        let items = [("Alpha", "Alpha"), ("Beta", "Beta"), ("Meiryo", "Meiryo")];
        let mut st = left_over_state();
        assert!(sync_open_combo(&mut st, &items, "Meiryo", true), "a fresh open asks for a repaint");
        assert_eq!(st.text, "");
        // Unfiltered now, so the current value's own position; the leftover highlight ("Beta",
        // position 1) must not win.
        assert_eq!(st.sel, 2);
        assert_eq!(st.values, ["Alpha".to_string(), "Beta".to_string(), "Meiryo".to_string()]);
        assert!(st.was_open);
    }

    #[test]
    fn an_unchanged_list_leaves_navigation_and_values_alone() {
        let items = [("Alpha", "Alpha"), ("Beta", "Beta"), ("Meiryo", "Meiryo")];
        let mut st = ComboState {
            sel: 2,
            was_open: true,
            values: vec!["Alpha".into(), "Beta".into(), "Meiryo".into()],
            highlight: None,
            ..Default::default()
        };
        assert!(!sync_open_combo(&mut st, &items, "Alpha", true));
        // A rematch would pull the highlight back to the current value (position 0); with no
        // list change none runs, so the position the user navigated to stays.
        assert_eq!(st.sel, 2);
        assert_eq!(st.values.len(), 3);
    }

    #[test]
    fn a_list_change_under_an_open_popup_follows_the_highlight_and_stores_the_new_values() {
        // The font scan lands: Cambria sorts in before the row the user had navigated to.
        let items = [("Arial", "Arial"), ("Beta", "Beta"), ("Cambria", "Cambria"), ("Meiryo", "Meiryo")];
        let mut st = ComboState {
            sel: 2,
            was_open: true,
            values: vec!["Arial".into(), "Beta".into(), "Meiryo".into()],
            highlight: Some("Meiryo".into()),
            ..Default::default()
        };
        assert!(!sync_open_combo(&mut st, &items, "Beta", true));
        assert_eq!(st.sel, 3);
        assert_eq!(st.values, ["Arial".to_string(), "Beta".to_string(), "Cambria".to_string(), "Meiryo".to_string()]);
    }

    #[test]
    fn a_list_change_with_the_highlight_gone_falls_back_to_the_current_value_in_the_filtered_rows() {
        let items = [("Arial", "Arial"), ("Meiryo", "Meiryo"), ("Tahoma", "Tahoma")];
        let mut st = ComboState { text: "a".into(), sel: 0, was_open: true, values: vec!["Arial".into()], highlight: Some("Gone".into()) };
        assert!(!sync_open_combo(&mut st, &items, "Tahoma", true));
        // The filter "a" shows Arial and Tahoma; Tahoma is filtered position 1, not 2.
        assert_eq!(st.sel, 1);
        assert_eq!(st.values.len(), 3);
    }

    #[test]
    fn values_changed_compares_values_only() {
        let old = vec!["Alpha".to_string(), "Beta".to_string()];
        assert!(!values_changed(&old, &[("Alpha", "Alpha — a new preview"), ("Beta", "Beta")]), "display-only changes do not count");
        assert!(values_changed(&old, &[("Alpha", "Alpha")]), "shorter");
        assert!(values_changed(&old, &[("Alpha", "Alpha"), ("Gamma", "Gamma")]), "longer");
        assert!(values_changed(&old, &[("Alpha", "Alpha"), ("Beta2", "Beta2")]), "a value replaced");
    }

    #[test]
    fn the_highlighted_value_resolves_through_the_filtered_rows() {
        let items = [("Alpha", "Alpha"), ("Beta", "Beta"), ("Meiryo", "Meiryo")];
        let filtered = filtered_positions(&items, "me");
        assert_eq!(highlighted_value(&items, &filtered, 0).as_deref(), Some("Meiryo"));
        assert_eq!(highlighted_value(&items, &filtered, 5), None);
        assert_eq!(highlighted_value(&items, &[], 0), None);
    }
}

#[cfg(test)]
mod combo_popup_tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    /// What the harness drives: the item values as they stand this frame (the test swaps them
    /// mid-scenario, as the font scan landing does), the combo's current value, and the pick
    /// the user made, if any.
    struct ComboDrive {
        values: Vec<String>,
        current: String,
        picked: Option<String>,
    }

    /// An editable combo on a plain anchor button, forced open, over the drive's values.
    fn combo_harness(values: &[&str], current: &str) -> egui_kittest::Harness<'static, ComboDrive> {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 400.0)).build_ui_state(
            |ui, drive: &mut ComboDrive| {
                if ui.ctx().cumulative_pass_nr() == 0 {
                    ui.ctx().set_fonts(theme::pinned_font_definitions());
                    return;
                }
                let resp = ui.add(egui::Button::new("Font name"));
                let items: Vec<(&str, &str)> = drive.values.iter().map(|v| (v.as_str(), v.as_str())).collect();
                if let ComboResult::Picked(value) = editable_combo(ui, &resp, "font", &drive.current, &items, true) {
                    drive.picked = Some(value);
                }
            },
            ComboDrive { values: values.iter().map(|v| v.to_string()).collect(), current: current.to_string(), picked: None },
        );
        h.run_steps(2);
        h
    }

    #[test]
    fn a_list_change_under_the_open_combo_keeps_enter_on_the_current_value() {
        // The review scenario, driven end to end: the combo opens over the standard names
        // only, the background font scan lands and the list grows under the open popup, and
        // Enter must still apply the current value — not the row that took its old position.
        let mut h = combo_harness(&["Arial", "Calibri", "Tahoma"], "Tahoma");
        assert!(h.query_by_label("Tahoma").is_some(), "the open popup draws its rows");
        assert_eq!(h.state().picked, None);

        h.state_mut().values = ["Arial", "Calibri", "Meiryo", "Tahoma", "Zebra"].iter().map(|v| v.to_string()).collect();
        h.run_steps(2);
        assert!(h.query_by_label("Meiryo").is_some(), "the grown list is what the popup now shows");
        assert_eq!(h.state().picked, None);

        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(2);
        assert_eq!(h.state().picked.as_deref(), Some("Tahoma"), "Enter applies the current value, not Meiryo at its stale position");
    }
}
