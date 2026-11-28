use egui::{Align2, Rect, pos2, vec2};
use gridcraft_engine::{
    DocState, cmd::data::list_items, core::CellRef, model::Sheet,
};

use crate::{SheetApp, grid::{Geo, editor::popup_dismissed}};

use super::{
    json,
};

/// Rows the list-validation dropdown shows before it scrolls (Excel shows 8).
const LIST_PICKER_MAX_ROWS: usize = 8;

const LIST_PICKER_ROW_H: f32 = 18.0;

/// Popup frame around the rows: 2 px inner margin and the window stroke on each side, rounded up.
const LIST_PICKER_CHROME: f32 = 8.0;

/// State of the open validation dropdown, resolved once when it opens — on the arrow
/// click (`interact`) or Alt+Down (`keymap`) — so `list_picker` does no validation scan
/// and no `list_items` call per frame: `list_items` evaluates the list's source formula,
/// far too slow to repeat every repaint (same rule as `GridState::pivot_fields`, #2113).
///
/// `PartialEq` compares `items` too; callers that only need "is the dropdown still open
/// on this cell" must compare `cell`, not the whole picker.
#[derive(PartialEq, Eq)]
pub struct ListPicker {
    /// The cell the dropdown opened on (the active cell): where the popup anchors.
    pub cell: CellRef,

    /// Top-left of the cell's merge — the only cell of a merge that holds a
    /// value, so it is the `cell.set` target; equals `cell` when unmerged.
    pub anchor: CellRef,

    /// The rule's list entries, computed once at open.
    pub items: Vec<String>,

    /// The highlighted the item row, moved by `input()` (arrows) and row hover.
    pub sel: usize,
}

impl ListPicker {

    /// Resolves the rule covering `c`, its items and the merge anchor.
    /// `None` when `c` has no list validation — the caller simply doesn't
    /// open the dropdown.
    pub fn open(d: &DocState, c: CellRef) -> Option<Self> {
        let sh = d.wb.active()?;
        // let dv = sh.validations.iter().find(|v| v.ranges.iter().any(|r| r.contains(c)))?;
        let dv = sh.validations.iter().find(|v| {
            v.kind == gridcraft_engine::model::ValidationKind::List
                && v.in_cell_dropdown
                && v.ranges.iter().any(|r| r.contains(c))
        })?;

        // A covered cell of a merge can't hold a value: writes go to its anchor.
        let anchor = sh.merge_at(c).map(|m| m.start).unwrap_or(c);
        // Resolve the items at the anchor — the cell the picked value is written to —  
        // so a relative list source offers the entries the anchor's rule checks.  
        let items = list_items(&d.wb, d.wb.active_sheet(), anchor, dv);
       Some(
           Self { cell: c, anchor, items, sel: 0 }
        )
    }

    /// Consumes the picker's keys for this frame — ArrowUp/ArrowDown move `sel`, Enter
    /// picks the highlighted item — and returns what the painter needs. The grid's
    /// `keyboard()` never sees them: its guard yields while `list_picker` is `Some`.
    /// Escape is not here on purpose: `popup_dismissed` handles it with `was_open`.
    pub fn input(&mut self, ui: &egui::Ui) -> PickerInput {
        let down = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        if down { self.sel = (self.sel + 1).min(self.items.len().saturating_sub(1)); }
        if up { self.sel = self.sel.saturating_sub(1); }
        let picked = enter.then(|| self.items.get(self.sel).cloned()).flatten();
        PickerInput { down, up, picked }
    }
}

/// What `ListPicker::input` consumed this frame: the arrow that moved
/// `sel` (the painter scrolls the highlighted row into view) and the
/// item Enter picked, if any.
pub struct PickerInput {
    pub down: bool,
    pub up: bool,
    pub picked: Option<String>,
}


/// The screen rect of `c` or of its whole merge: the single place that resolves
/// merges for the validation UI, so the arrow, the popup and the input message
/// all anchor to the same rect — a merged cell gets one dropdown, not one per
/// covered cell.
pub fn merge_rect(sh: &Sheet, geo: &Geo, c: CellRef) -> Rect {
    match sh.merge_at(c) {
        Some(m) => geo.range_rect(sh, m),
        None => geo.cell_rect(sh, c),
    }
}

/// The dropdown arrow beside a cell with an in-cell list validation.
/// Anchored to `merge_rect`, the rect of the whole merged range, so a merged
/// cell gets one arrow at its bottom-right corner — not one per covered cell.
/// `interact` reuses this rect for hit-testing, so the painted arrow and the
/// clickable area can never disagree.
/// `None` when the cell has no list validation with `in_cell_dropdown`, or is
/// scrolled under a frozen pane (`hidden_under_frozen` tests `c`, not the merge).
pub fn validation_arrow(sh: &Sheet, geo: &Geo, c: CellRef) -> Option<Rect> {
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(c)))?;
    if dv.kind != gridcraft_engine::model::ValidationKind::List || !dv.in_cell_dropdown || geo.hidden_under_frozen(sh, c) {
        return None;
    }
    let r = merge_rect(sh, geo, c);
    let s = r.height().clamp(14.0, 22.0);
    Some(Rect::from_min_size(pos2(r.right() + 1.0, r.bottom() - s), vec2(s, s)))
}

/// Popup height for `n` items: every item up to `LIST_PICKER_MAX_ROWS`, then a scrollbar.
pub(super) fn list_picker_height(n: usize, row_h: f32, gap: f32) -> f32 {
    let rows = n.clamp(1, LIST_PICKER_MAX_ROWS) as f32;
    rows * row_h + (rows - 1.0) * gap
}

/// Draws the validation list popup, choosing above/below and left/right placement for RTL,
/// and applies the picked item with `cell.set` on the merge anchor.
///
/// The picker is `take()`n out of `app.grid` — not cloned — so the `egui::Area` closure can
/// borrow `app` mutably for `run_or_alert` while `p` stays a local value: no `Vec<String>`
/// copy per frame. It is put back only if the popup stays open, which is also what persists
/// the `sel` moved by `input()` or by hover.
pub(super) fn list_picker(app: &mut SheetApp, ui: &mut egui::Ui, geo: &Geo, was_open: bool) {
    let Some(mut p) = app.grid.list_picker.take() else { return };
    let Some(d) = app.session.active() else { return };
    let Some(sh) = d.wb.sheet(d.wb.active_sheet()) else { return };

    // ↑/↓ move the highlight, Enter picks it; the grid's keyboard() already yielded these keys.
    let input = p.input(ui);

    // Rect of the whole merge (or the single cell): one popup anchored to the merged range.
    let r = merge_rect(sh, geo, p.cell);

    // Fit the popup to its rows (up to LIST_PICKER_MAX_ROWS); without a minimum the scroll area collapses to its 64 px default.
    let height = list_picker_height(p.items.len(), LIST_PICKER_ROW_H, ui.spacing().item_spacing.y);

    // Open below the cell, or above it when the rows would run past the bottom of the window (as Excel does).
    let screen = ui.ctx().content_rect();
    let above = r.bottom() + height + LIST_PICKER_CHROME > screen.bottom() && r.top() - height - LIST_PICKER_CHROME >= screen.top();
    let (at, pivot) = match (above, geo.rtl) {
        (false, false) => (geo.mirror_pos(r.left_bottom()), Align2::LEFT_TOP),
        (false, true) => (geo.mirror_pos(r.left_bottom()), Align2::RIGHT_TOP),
        (true, false) => (geo.mirror_pos(r.left_top()), Align2::LEFT_BOTTOM),
        (true, true) => (geo.mirror_pos(r.left_top()), Align2::RIGHT_BOTTOM),
    };
    let mut close = false;
    let response = egui::Area::new(egui::Id::unique("dv_list")).fixed_pos(at).pivot(pivot).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).inner_margin(2.0).show(ui, |ui| {
            ui.set_min_width(r.width().max(100.0));

            egui::ScrollArea::vertical().max_height(height).min_scrolled_height(height).show(ui, |ui| {
                // Hover takes the highlight only when the pointer moved this frame: a
                // stationary pointer over a row would undo ArrowDown/ArrowUp every repaint.
                let pointer_moved = ui.ctx().input(|i| {
                    i.events.iter().any(|e| matches!(e, egui::Event::PointerMoved(_)))
                });
                // Enter's choice is fixed at input time: a same-frame hover may move `sel`,  
                // but it must not change what Enter picked. Only an explicit click overrides.  
                let mut chosen = input.picked.as_deref();  
                for (i, it) in p.items.iter().enumerate() {
                    let row = ui.add(
                        egui::Button::selectable(i == p.sel, it.as_str())
                            .frame_when_inactive(false)
                            .min_size(vec2(r.width().max(100.0), LIST_PICKER_ROW_H)));
                    if (input.down || input.up) && i == p.sel && !ui.clip_rect().contains_rect(row.rect) {
                        row.scroll_to_me(Some(if input.down { egui::Align::Max } else { egui::Align::Min }));
                    }
                    if pointer_moved && row.hovered() {
                        p.sel = i;
                    }
                    
                    if row.clicked() {
                        chosen = Some(it.as_str());
                    }
                }
                if let Some(it) = chosen {
                    app.run_or_alert("cell.set", json!({"cell": p.anchor.a1(), "input": it}));
                    close = true;
                }
            });
        });
    });
    if close || popup_dismissed(&response.response, was_open) {
        app.grid.list_picker = None;
    } else {
        app.grid.list_picker = Some(p); // Save the item selected by the keyboard/hover
    }
}
