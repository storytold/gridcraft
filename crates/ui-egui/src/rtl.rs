//! Mirrored window layout for a right-to-left interface language (Arabic).
//!
//! egui has no global direction, but `ui.horizontal` runs right to left wherever the enclosing
//! layout prefers it ([`egui::Layout::prefer_right_to_left`]): a right-to-left row, or a column
//! whose items are aligned to the right. So direction is inherited from a root: each panel,
//! window and menu body is wrapped in [`root`], columns are opened with [`vertical`] (egui's
//! `ui.vertical` always resets to left-aligned), and "far side" clusters use [`far_side`].
//! Widgets that paint their own parts ask [`flows`] where their leading edge is.
//!
//! Areas egui can only lay out from the left (a `Grid`, a horizontally scrolling strip) run in an
//! [`island`]: left to right inside, set against the leading edge. Everything here follows the
//! enclosing layout rather than the language, so a left-to-right interface is never touched.
//!
//! The sheet itself is not mirrored by the interface language: its direction belongs to the sheet
//! (Page Layout ▸ Sheet Right-to-Left), as does the direction of cell text.

use egui::{Align, Align2, Context, InnerResponse, Layout, Rect, Ui, UiBuilder, pos2, vec2};

/// Is the interface language written right to left?
pub fn on(ctx: &Context) -> bool {
    crate::i18n::current(ctx).rtl()
}

/// Is `ui` part of the mirrored window: a right-to-left language, in an area that lays its content
/// out from the right (so rows opened in it run right to left)? Never in a left-to-right
/// language, whose far-side clusters also fill from the right but are not mirrored.
pub fn flows(ui: &Ui) -> bool {
    on(ui.ctx()) && ui.layout().prefer_right_to_left()
}

/// Run a container's body (a panel, window, popup or menu, which egui starts left-aligned) in the
/// interface's direction. In a left-to-right language this is just `add`.
pub fn root<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !on(ui.ctx()) || flows(ui) {
        return add(ui);
    }
    ui.scope_builder(UiBuilder::new().layout(Layout::top_down(Align::Max)), add).inner
}

/// `ui.vertical`, aligned to the side rows start from: a column inside a right-to-left area keeps
/// its content on the right and its rows right to left.
pub fn vertical<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    if flows(ui) { ui.scope_builder(UiBuilder::new().layout(Layout::top_down(Align::Max)), add) } else { ui.vertical(add) }
}

/// The layout for a row's items that belong at its far end (the side rows end on): filled from
/// that end inwards.
pub fn far_side(ui: &Ui, align: Align) -> Layout {
    if flows(ui) { Layout::left_to_right(align) } else { Layout::right_to_left(align) }
}

/// A row layout in the direction of `ui`, for `allocate_ui_with_layout` and `with_layout`.
pub fn row_layout(ui: &Ui, align: Align) -> Layout {
    if flows(ui) { Layout::right_to_left(align) } else { Layout::left_to_right(align) }
}

/// The x coordinate `inset` points in from the leading edge of `rect` (left, or right when `rtl`).
pub fn lead_x(rtl: bool, rect: Rect, inset: f32) -> f32 {
    if rtl { rect.max.x - inset } else { rect.min.x + inset }
}

/// The x coordinate `inset` points in from the trailing edge of `rect`.
pub fn trail_x(rtl: bool, rect: Rect, inset: f32) -> f32 {
    lead_x(!rtl, rect, inset)
}

/// Text anchored at its leading edge and vertically centred: left-centre, or right-centre when `rtl`.
pub fn lead_center(rtl: bool) -> Align2 {
    if rtl { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER }
}

/// Text anchored at its leading edge and top.
pub fn lead_top(rtl: bool) -> Align2 {
    if rtl { Align2::RIGHT_TOP } else { Align2::LEFT_TOP }
}

/// Text anchored at its trailing edge and vertically centred.
pub fn trail_center(rtl: bool) -> Align2 {
    lead_center(!rtl)
}

/// Where to put something `width` wide so that it starts `inset` in from the leading edge of
/// `rect`: its left x.
pub fn lead_left(rtl: bool, rect: Rect, inset: f32, width: f32) -> f32 {
    if rtl { rect.max.x - inset - width } else { rect.min.x + inset }
}

/// `egui::Popup::menu`, with its items aligned to the side reading starts from.
pub fn menu(response: &egui::Response) -> egui::Popup<'_> {
    let popup = egui::Popup::menu(response);
    if on(&response.ctx) { popup.layout(menu_layout()) } else { popup }
}

/// `egui::Popup::context_menu`, with its items aligned to the side reading starts from.
pub fn context_menu_popup(response: &egui::Response) -> egui::Popup<'_> {
    let popup = egui::Popup::context_menu(response);
    if on(&response.ctx) { popup.layout(menu_layout()) } else { popup }
}

/// `Response::context_menu`, with its items aligned to the side reading starts from.
pub fn context_menu(response: &egui::Response, add: impl FnOnce(&mut Ui)) {
    context_menu_popup(response).show(add);
}

/// A right-to-left menu's layout: items against the right edge. Not justified, because egui
/// starts a full-width button's text at its left.
fn menu_layout() -> Layout {
    Layout::top_down(Align::Max)
}

/// Run `add` in a left-to-right island inside a right-to-left area, for content egui can only
/// lay out from the left (a `Grid`, a horizontally scrolling strip). The island is set against the
/// leading (right) edge using the width it had last frame. Outside a right-to-left area this is
/// just `add`.
pub fn island<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !flows(ui) {
        return add(ui);
    }
    // The island gets all the room there is, so its content takes its natural width; it starts
    // where content of last frame's width would end at the leading edge. A change of width shows
    // one frame late, so that frame asks for another.
    let key = ui.next_auto_id().with("rtl_island_width");
    let known = ui.data(|d| d.get_temp::<f32>(key));
    let room = ui.available_rect_before_wrap();
    let available = if room.width().is_finite() { room.width().max(0.0) } else { 0.0 };
    let width = known.unwrap_or(available);
    let left = room.max.x - width;
    if !left.is_finite() {
        return add(ui);
    }
    let max_rect = Rect::from_min_max(pos2(left, room.min.y), pos2(left + available.max(width), room.max.y));
    let shown = ui.scope_builder(UiBuilder::new().max_rect(max_rect).layout(Layout::top_down(Align::Min)), add);
    let now = shown.response.rect.width();
    if now.is_finite() && known.is_none_or(|w| (w - now).abs() > 0.5) {
        ui.data_mut(|d| d.insert_temp(key, now));
        ui.ctx().request_repaint();
    }
    shown.inner
}

/// A horizontally scrolling row of items (the ribbon, the sheet tabs): `add` fills it like a
/// `ui.horizontal`. Mirrored, the items start at the right edge. A scroll area's content is
/// unbounded, which a right-to-left row cannot start from, so the row is given a width (the
/// view's, or last frame's content if wider) and the scroll position is kept as a distance from
/// the right, where reading starts. Left to right this is a plain scroll area.
///
/// `target` asks for a scroll position (in the usual distance from the left); the area is returned
/// so the caller can read where it ended up. A row that `fill`s its view starts at the view's right
/// edge however little it holds (the ribbon); one that does not is as wide as its items (the
/// sheet tabs, which sit beside a button that follows them).
pub fn scroll_row(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    area: egui::ScrollArea,
    target: Option<f32>,
    fill: bool,
    add: impl FnOnce(&mut Ui),
) -> egui::scroll_area::ScrollAreaOutput<()> {
    if !on(ui.ctx()) {
        let area = match target {
            Some(x) => area.horizontal_scroll_offset(x),
            None => area,
        };
        return area.show(ui, |ui| {
            ui.horizontal(add);
        });
    }
    let row_id = ui.scope_id().with(("rtl_scroll_row", salt));
    let (content_w, mut from_right) = ui.data(|d| d.get_temp::<(f32, f32)>(row_id)).unwrap_or((0.0, 0.0));
    let room = ui.available_width();
    // A row that does not fill its view takes the width of its items (all there is while they are
    // unknown, or too wide to fit): the area is not left to shrink, because it would shrink towards
    // the left, away from where the row starts.
    let view_w = if fill || content_w <= 0.0 { room } else { content_w.min(room) };
    let over = (content_w - view_w).max(0.0);
    if let Some(x) = target {
        from_right = (over - x).clamp(0.0, over);
    }
    let mut measured = content_w;
    let height = ui.available_size_before_wrap().y;
    let scroll = ui
        .allocate_ui_with_layout(vec2(view_w, height), Layout::top_down(Align::Min), |ui| {
            area.horizontal_scroll_offset((over - from_right).max(0.0)).show(ui, |ui| {
                let size = vec2(if fill { content_w.max(view_w) } else { content_w }, ui.available_size_before_wrap().y);
                measured = ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), add).response.rect.width();
            })
        })
        .inner;
    let over_now = (scroll.content_size.x - scroll.inner_rect.width()).max(0.0);
    let from_right = (over_now - scroll.state.clamped_offset().x).max(0.0).min(over_now);
    let measured = if measured.is_finite() { measured } else { content_w };
    if (measured - content_w).abs() > 0.5 {
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(row_id, (measured, from_right)));
    scroll
}

/// `grid.show(ui, add)`. egui's `Grid` cannot be laid out from the right (it places cells from a
/// left edge that a right-to-left parent does not have, and says so with a debug assertion), so
/// inside a right-to-left area it is drawn in an [`island`]. Its columns keep their order.
pub fn grid<R>(ui: &mut Ui, grid: egui::Grid, add: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    island(ui, |ui| grid.show(ui, add))
}

/// A flat menu row at least `min_size` wide. egui starts a full-width button's text at its left,
/// so a mirrored row puts its text at the right end (`right_text`) instead.
pub fn item<'a>(ui: &Ui, text: &'a str, min_size: egui::Vec2) -> egui::Button<'a> {
    let button = if flows(ui) { egui::Button::new("").right_text(text) } else { egui::Button::new(text) };
    button.frame_when_inactive(false).min_size(min_size)
}

/// `ui.checkbox`, with the box on the side reading starts from: egui always draws it on the left.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    if !flows(ui) {
        return ui.checkbox(checked, text);
    }
    let text = text.into();
    // The box carries no text of its own: name it for assistive technology.
    let name = text.text().to_owned();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = ui.spacing().icon_spacing;
        let mut mark = ui.checkbox(checked, "").accessible_name(name);
        let label = ui.add(egui::Label::new(text).selectable(false).sense(egui::Sense::click()));
        if label.clicked() {
            *checked = !*checked;
            mark.mark_changed();
        }
        mark.union(label)
    })
    .inner
}

/// `ui.radio_value`, with the button on the side reading starts from.
pub fn radio_value<V: PartialEq>(ui: &mut Ui, current: &mut V, alternative: V, text: impl Into<egui::WidgetText>) -> egui::Response {
    if !flows(ui) {
        return ui.radio_value(current, alternative, text);
    }
    let text = text.into();
    let name = text.text().to_owned();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = ui.spacing().icon_spacing;
        let mut mark = ui.radio(*current == alternative, "").accessible_name(name);
        let label = ui.add(egui::Label::new(text).selectable(false).sense(egui::Sense::click()));
        if (mark.clicked() || label.clicked()) && *current != alternative {
            *current = alternative;
            mark.mark_changed();
        }
        mark.union(label)
    })
    .inner
}

/// `ui.radio`, with the button on the side reading starts from.
pub fn radio(ui: &mut Ui, selected: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    if !flows(ui) {
        return ui.radio(selected, text);
    }
    let text = text.into();
    let name = text.text().to_owned();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = ui.spacing().icon_spacing;
        let mark = ui.radio(selected, "").accessible_name(name);
        let label = ui.add(egui::Label::new(text).selectable(false).sense(egui::Sense::click()));
        mark.union(label)
    })
    .inner
}
