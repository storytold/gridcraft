use super::{Rect, Sense, SheetApp, Stroke, StrokeKind, Style, Tokens, Ui, keytip_table, pos2, theme, vec2};

pub(super) fn sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(13.0, 72.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 6.0), pos2(r.center().x, r.bottom() - 6.0)], Stroke::new(1.0, t.separator));
}

/// Height of a ribbon group's control area and of the caption band under it; together they fill
/// the ribbon card so every group's caption lines up along the bottom, as in Excel.
pub(super) const GROUP_CONTROLS_H: f32 = 72.0;
pub(super) const GROUP_CAPTION_H: f32 = 16.0;
/// Width of the dialog launcher's box in the caption band's bottom-right corner.
pub(super) const GROUP_LAUNCHER_W: f32 = 15.0;
/// Padding a caption band keeps on each side when the group has no launcher.
const GROUP_CAPTION_PAD: f32 = 12.0;

/// A group's width: its controls' width, or its caption's plus the padding the caption band
/// needs. A launcher sits over the band's right-hand corner, and the caption is centred, so with
/// one the caption needs the launcher's width clear on each side too (#1795). The caption always
/// fits, so a group never shrinks below what its name needs in a narrow window.
pub(super) fn group_width(controls_w: f32, caption_w: f32, launcher: bool) -> f32 {
    let pad = if launcher { 2.0 * GROUP_LAUNCHER_W } else { GROUP_CAPTION_PAD };
    controls_w.max(caption_w + pad)
}

/// An Excel-style ribbon group: the controls packed top-left over a caption band carrying the
/// group's name, with Excel's dialog launcher in the bottom-right corner where Excel has one.
/// Every group is the same height, so the captions line up. A missing feature takes no space at
/// all: the cluster is simply narrower and everything after it moves left (#1655, #1764).
pub(super) fn group(
    app: &mut SheetApp,
    ui: &mut Ui,
    title: &str,
    launcher: Option<(&str, serde_json::Value)>,
    add: impl FnOnce(&mut SheetApp, &mut Ui),
) {
    let t = Tokens::get(ui.ctx());
    ui.vertical(|ui| {
        let row = ui.horizontal(|ui| add(app, ui));
        if row.response.rect.height() < GROUP_CONTROLS_H {
            ui.add_space(GROUP_CONTROLS_H - row.response.rect.height());
        }
        // The caption sits centred under the controls; a caption wider than them widens the group,
        // leaving the launcher's corner clear.
        let label = app.ui.language.tr(title).to_string();
        let galley = ui.painter().layout_no_wrap(label, theme::ui_font(11.0), t.text_dim);
        let w = group_width(row.response.rect.width(), galley.size().x, launcher.is_some());
        let (cap, _) = ui.allocate_exact_size(vec2(w, GROUP_CAPTION_H), Sense::hover());
        ui.painter().galley(cap.center() - galley.size() / 2.0, galley, t.text_dim);
        if let Some((dialog, params)) = launcher {
            // Excel's dialog launcher: a small box with an arrow, in the group's bottom-right.
            let lr = Rect::from_min_size(pos2(cap.right() - GROUP_LAUNCHER_W, cap.center().y - 7.5), vec2(GROUP_LAUNCHER_W, GROUP_LAUNCHER_W));
            let resp = ui.interact(lr, ui.scope_id().with(("group_launcher", title)), Sense::click());
            // A painted box carries no label of its own, so name it for screen readers.
            let name = format!("{} dialog launcher", app.ui.language.tr(title));
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), name.as_str()));
            if resp.hovered() {
                ui.painter().rect_filled(lr.shrink(1.0), 3.0, t.hover);
            }
            let box_r = lr.shrink(3.5);
            let p = ui.painter();
            p.rect_stroke(box_r, 1.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
            p.line_segment([box_r.right_top(), box_r.left_bottom()], Stroke::new(1.0, t.text_dim));
            p.line_segment([box_r.left_bottom(), box_r.left_bottom() + vec2(3.5, 0.0)], Stroke::new(1.0, t.text_dim));
            p.line_segment([box_r.left_bottom(), box_r.left_bottom() + vec2(0.0, -3.5)], Stroke::new(1.0, t.text_dim));
            if resp.on_hover_text(app.ui.language.tr("Dialog launcher")).clicked() {
                app.open_dialog(dialog, params.clone());
            }
        }
    });
}

/// A short divider between the clusters inside one group, as Excel draws in Font and Number.
pub(super) fn inner_sep(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 24.0), Sense::hover());
    ui.painter().line_segment([pos2(r.center().x, r.top() + 2.0), pos2(r.center().x, r.bottom() - 2.0)], Stroke::new(1.0, t.separator));
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

#[cfg(test)]
mod group_width_tests {
    use super::*;

    #[test]
    fn a_launcher_leaves_its_corner_clear_of_the_caption() {
        // A caption wider than the controls is centred, so the launcher's width must stay free on
        // each side of it; otherwise the box covers the text (#1795).
        let w = group_width(40.0, 100.0, true);
        assert_eq!(w, 100.0 + 2.0 * GROUP_LAUNCHER_W);
        assert_eq!((w - 100.0) / 2.0, GROUP_LAUNCHER_W, "the caption clears the launcher");
        // Without a launcher the band keeps its smaller padding.
        assert_eq!(group_width(40.0, 100.0, false), 112.0);
        // Controls wider than the caption still set the width, and the centred caption clears
        // the launcher's corner there too.
        let w = group_width(200.0, 100.0, true);
        assert_eq!(w, 200.0);
        assert!(w / 2.0 + 50.0 <= w - GROUP_LAUNCHER_W);
    }
}
