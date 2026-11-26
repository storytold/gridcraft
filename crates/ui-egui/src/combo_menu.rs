//! The drop-down menu of the ribbon's combo fields (font name and size): a menu under a field that
//! scrolls its rows and can keep a header (the filter field) in place above them.

use egui::{Frame, Popup, PopupCloseBehavior, RectAlign, Response, ScrollArea, TextWrapMode, Ui};

/// A menu under a field that opens and closes on the field's clicks, closes on a click, and opens
/// below the field or above it, never beside it.
pub(crate) struct Menu<'a> {
    popup: Popup<'a>,
    field: egui::Rect,
    height: f32,
}

impl<'a> Menu<'a> {
    /// The menu under `field`, a widget in `ui`.
    pub(crate) fn new(ui: &Ui, field: &Response) -> Self {
        let flipped: &'static [RectAlign] = if ui.ctx().global_style().right_to_left { &[RectAlign::TOP_END] } else { &[RectAlign::TOP_START] };
        Self { popup: Popup::menu(field).align_alternatives(flipped), field: field.rect, height: ui.spacing().combo_height }
    }

    /// The most height of the menu's contents, inside its frame; the room on the field's roomier
    /// side caps it.
    pub(crate) fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Shows `add` in the menu while it is open, scrolling when it is taller than the menu.
    /// Returns what `add` returned, or `None` while the menu is closed.
    pub(crate) fn show_ui<R>(self, add: impl FnOnce(&mut Ui) -> R) -> Option<R> {
        self.show(|_| (), |ui, ()| add(ui))
    }

    /// Shows `header` at the top of the menu and `list` under it while the menu is open, and
    /// returns what `list` returned. `list` gets what `header` returned. Only `list` scrolls; the
    /// header stays in place.
    pub(crate) fn show_ui_with_header<H, R>(self, header: impl FnOnce(&mut Ui) -> H, list: impl FnOnce(&mut Ui, H) -> R) -> Option<R> {
        self.show(header, list)
    }

    fn show<H, R>(self, header: impl FnOnce(&mut Ui) -> H, list: impl FnOnce(&mut Ui, H) -> R) -> Option<R> {
        let Self { popup, field, height } = self;
        let ctx = popup.ctx().clone();
        let screen = ctx.content_rect();
        let room = (screen.bottom() - field.bottom()).max(field.top() - screen.top());
        let style = egui::style::StyleModifier::from(egui::containers::menu::menu_style);
        let mut popup_style = (*ctx.global_style()).clone();
        style.apply(&mut popup_style);
        let frame = Frame::popup(&popup_style).total_margin().sum().y;
        let height = height.min(room - frame).max(0.0);
        popup
            .width(field.width())
            .style(style)
            .close_behavior(PopupCloseBehavior::CloseOnClick)
            .show(|ui| {
                ui.set_min_width(ui.available_width());
                // egui offers the popup last pass's height; offer the full height so a list that
                // shrank (a filter with no matches) grows back.
                ui.set_max_height(height);
                let header = header(ui);
                // Labels widen a narrow menu rather than wrap early, as in egui's combo box.
                ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                ScrollArea::vertical().max_height(ui.available_height().max(0.0)).show(ui, |ui| list(ui, header)).inner
            })
            .map(|shown| shown.inner)
    }
}
