use super::{Align2, Color32, Rect, Sense, SheetApp, Stroke, StrokeKind, Style, Tokens, Ui, keytip_table, pos2, theme, vec2};

/// The ribbon card's group-label band: Excel names every group under its buttons, in a band of
/// this height below them.
pub const RIBBON_LABEL_H: f32 = 18.0;

/// One group's name and the rect its label was painted in, in screen coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupLabel {
    /// The group's Excel name (translated).
    pub label: String,
    /// What was painted: `label`, shortened with an ellipsis when the group is narrower.
    pub painted: String,
    pub rect: Rect,
}

/// The group names painted in the card's label band this frame, recorded for the tests.
#[derive(Clone, Debug)]
pub struct RibbonLabels {
    /// The band the names are painted in, below the buttons.
    pub band: Rect,
    pub labels: Vec<GroupLabel>,
}

impl Default for RibbonLabels {
    fn default() -> Self {
        Self { band: Rect::NOTHING, labels: Vec::new() }
    }
}

impl RibbonLabels {
    /// Returns the identifier for the recorded labels.
    fn id() -> egui::Id {
        egui::Id::unique("ribbon_group_labels")
    }

    /// Last frame's labels.
    pub fn get(ctx: &egui::Context) -> RibbonLabels {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }
}

/// A group's span while its tab is drawn: the name of the group that ended at `right` and where
/// it started, both in screen coordinates; `separator` is set when a group separator ends it.
#[derive(Clone, Debug)]
struct GroupSpan {
    label: String,
    left: f32,
    right: f32,
    separator: bool,
}

fn spans_id() -> egui::Id {
    egui::Id::unique("ribbon_group_spans")
}

fn start_id() -> egui::Id {
    egui::Id::unique("ribbon_group_start")
}

/// Starts a tab's groups at the cursor. Called by the ribbon before the active tab draws its
/// groups.
///
/// Group tracking is per frame and single-shot: `begin_groups` resets the spans and the start x,
/// each tab's `sep`/`group_end` calls append one span apiece, and `paint_group_labels` consumes
/// the spans and clears `start_id`. The three calls MUST run in that order inside `show()` on
/// every frame that draws a tab; `sep`/`group_end` MUST NOT be called anywhere else, since an
/// out-of-order call writes `start_id` and leaves stale state until the next `begin_groups`.
pub(super) fn begin_groups(ui: &Ui) {
    let already = ui.ctx().data(|d| d.get_temp::<Vec<GroupSpan>>(spans_id()).is_some());
    debug_assert!(!already, "begin_groups ran twice without paint_group_labels consuming the spans");
    ui.ctx().data_mut(|d| {
        d.insert_temp(spans_id(), Vec::<GroupSpan>::new());
        d.insert_temp(start_id(), ui.cursor().min.x);
    });
}

/// Records that the group named `label` ends at `right` and the next one starts there. Only valid
/// between `begin_groups` and `paint_group_labels` while a tab draws inside `show()` (see the
/// invariant on `begin_groups`).
fn end_group(ui: &Ui, label: &str, right: f32, separator: bool) {
    let left = ui.ctx().data(|d| d.get_temp::<f32>(start_id())).unwrap_or(right);
    let label = crate::i18n::current(ui.ctx()).tr(label).to_owned();
    ui.ctx().data_mut(|d| {
        let Some(mut spans) = d.get_temp::<Vec<GroupSpan>>(spans_id()) else {
            debug_assert!(false, "sep/group_end called outside show(): begin_groups did not start group tracking");
            return;
        };
        spans.push(GroupSpan { label, left, right, separator });
        d.insert_temp(spans_id(), spans);
        d.insert_temp(start_id(), right);
    });
}

/// The separator after a group, named with that group's Excel label. The gap is laid out here;
/// the line itself is painted with the labels, so it can run through the label band (a painter
/// inside the scroll area is clipped to the buttons' height).
///
/// Only valid while a tab draws between `begin_groups` and `paint_group_labels` in `show()`.
pub(crate) fn sep(ui: &mut Ui, label: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    end_group(ui, label, r.center().x, true);
}

/// Ends the last group of a tab, after which no separator is drawn.
///
/// Only valid while a tab draws between `begin_groups` and `paint_group_labels` in `show()`.
pub(crate) fn group_end(ui: &mut Ui, label: &str) {
    end_group(ui, label, ui.cursor().min.x, false);
}

/// Paints the separators (from the buttons into the label band) and each group's name centred in
/// the band, clipped to the card's content viewport so a scrolled group is cut at the card's edge
/// as Excel's is. Returns what was painted.
pub(super) fn paint_group_labels(ui: &Ui, content: Rect, band: Rect, viewport: Rect) -> RibbonLabels {
    let t = Tokens::get(ui.ctx());
    let spans = ui.ctx().data_mut(|d| d.remove_temp::<Vec<GroupSpan>>(spans_id())).unwrap_or_default();
    ui.ctx().data_mut(|d| d.remove_temp::<f32>(start_id()));
    let line_top = content.top() + 6.0;
    let line_bottom = band.bottom() - 2.0;
    let lines = ui.painter().with_clip_rect(Rect::from_min_max(pos2(viewport.left(), line_top), pos2(viewport.right(), line_bottom)));
    let clip = Rect::from_min_max(pos2(viewport.left(), band.top()), pos2(viewport.right(), band.bottom()));
    let painter = ui.painter().with_clip_rect(clip);
    let font = theme::ui_font(11.0);
    let labels: Vec<GroupLabel> = spans
        .into_iter()
        .map(|s| {
            if s.separator {
                lines.line_segment([pos2(s.right, line_top), pos2(s.right, line_bottom)], Stroke::new(1.0, t.separator));
            }
            let painted = fit_label(&painter, &s.label, &font, (s.right - s.left - 8.0).max(0.0));
            let rect = Rect::from_min_max(pos2(s.left, band.top()), pos2(s.right, band.bottom()));
            painter.text(rect.center(), Align2::CENTER_CENTER, painted.clone(), font.clone(), t.text_dim);
            GroupLabel { label: s.label, painted, rect }
        })
        .collect();
    let snapshot = RibbonLabels { band, labels };
    ui.ctx().data_mut(|d| d.insert_temp(RibbonLabels::id(), snapshot.clone()));
    snapshot
}

/// `label`, shortened with an ellipsis until it fits `max_w` (or empty when nothing fits), as
/// Excel shortens a group name wider than its group.
fn fit_label(painter: &egui::Painter, label: &str, font: &egui::FontId, max_w: f32) -> String {
    let width = |text: &str| painter.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE).size().x;
    if width(label) <= max_w {
        return label.to_owned();
    }
    let mut text = label.to_owned();
    while !text.is_empty() && width(&format!("{text}…")) > max_w {
        text.pop();
    }
    if text.is_empty() { String::new() } else { format!("{text}…") }
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
