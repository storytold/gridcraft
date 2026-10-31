//! The worksheet grid: virtualised painting of only the visible cells, frozen panes,
//! headers, selection, fill handle, column/row resizing, text overflow, borders, conditional
//! formats, tables, comments, filters, sparklines, floating charts/pictures/shapes and the
//! in-cell editor.

use std::sync::Arc;

use egui::{Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::cf::{CfCache, CfLook};
use gridcraft_engine::core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value, col_to_letters};
use gridcraft_engine::model::{BorderLine, BorderStyle, Color, HAlign, LineIndex, PatternType, Sheet, Style, Underline, VAlign, Workbook};
use serde_json::json;

use crate::SheetApp;
use crate::editor::EditState;
use crate::theme::{self, Tokens};

/// A painted pane: its row span, column span and screen rectangle.
pub(crate) type PaneQuad = ((u32, u32), (u32, u32), Rect);
/// Cached Page Break Preview ranges, keyed by (doc uid, revision, sheet).
type PagesCache = ((u64, u64, usize), Vec<(RangeRef, u32)>);
/// Cached Show Details cells, keyed by (doc uid, revision, sheet, pivot).
type DetailsCache = ((u64, u64, usize, usize), Option<std::collections::HashSet<(u32, u32)>>);

mod cell_paint;
mod cursor;
mod editor;
mod geo;
mod interact;
mod keyboard;
mod menus;
mod quadrant;
mod selection;
mod show;
mod tables;

pub(crate) use cell_paint::paint_border;
#[cfg(test)]
pub(crate) use cell_paint::{cell_text_y, rotated_text_pos, wrapped_cell_text_y};
#[cfg(test)]
use cell_paint::{center_across_end, display_text, look, numfmt_color, painted_halign};
pub use editor::validation_arrow;
#[cfg(test)]
use editor::{editor_slack, editor_width, list_picker_height};
pub use geo::Geo;
pub use interact::{follow_link, freeze_handles, point_move};
pub(crate) use quadrant::{QuadrantPaint, paint_quadrant};
pub use selection::selection_fill;
pub(crate) use selection::{SelectionPaintInput, paint_selection};
pub(crate) use show::horizontal_scrollbar;
pub use show::show;
#[cfg(test)]
use tables::filter_table_range;
pub use tables::{TableHeading, filter_button_rect, heading_table, table_headings};

pub const COL_HEADER_H: f32 = 21.0;
/// Sheet units are 96-dpi pixels (as in XLSX); on screen one unit is drawn at this many points,
/// which matches desktop spreadsheet density (a default 20 px row is 16 pt tall).
pub const DISPLAY_SCALE: f32 = 0.8;

/// The zoom percentages a Ctrl+wheel notch steps between: four per doubling (about 20% apart),
/// so 25, 50, 100, 200 and 400 are on it. Excel steps 15 points a notch instead; the status bar's
/// Zoom In / Zoom Out buttons keep their 10% steps.
pub const WHEEL_ZOOM_LADDER: [u16; 22] = [10, 12, 15, 18, 21, 25, 30, 35, 42, 50, 60, 70, 85, 100, 120, 140, 170, 200, 240, 280, 340, 400];

/// Seconds without Ctrl+wheel input after which part of a notch turned so far is forgotten.
const WHEEL_ZOOM_IDLE: f64 = 0.75;

/// The zoom one Ctrl+wheel notch goes to from `zoom`: the next step on [`WHEEL_ZOOM_LADDER`] in
/// that direction, skipping a step less than 5% away (121% out goes to 100%, not 120%), but never
/// 100% or the 10% / 400% limits (99% or 101% towards 100% goes to 100%).
pub fn wheel_zoom_step(zoom: u16, zoom_in: bool) -> u16 {
    let zoom = zoom.clamp(10, 400);
    let z = u32::from(zoom);
    let far = |l: u32| if zoom_in { l * 100 >= z * 105 } else { l * 105 <= z * 100 };
    let mut ahead = WHEEL_ZOOM_LADDER.iter().copied().filter(|&l| if zoom_in { l > zoom } else { l < zoom }).collect::<Vec<_>>();
    if !zoom_in {
        ahead.reverse();
    }
    let limit = ahead.last().copied().unwrap_or(zoom);
    ahead.into_iter().find(|&l| l == 100 || far(u32::from(l))).unwrap_or(limit)
}

/// One frame of the zoom gliding from `from` to `to` percent: the share `t` (0–1) of the way,
/// measured as a ratio so zooming in and out feel alike, and at least one percent, so it arrives.
pub fn zoom_glide_step(from: u16, to: u16, t: f32) -> u16 {
    let (f, g) = (f32::from(from.max(1)), f32::from(to.max(1)));
    let next = (f * (g / f).powf(t.clamp(0.0, 1.0))).round();
    let next = if next.is_finite() { next.clamp(f32::from(from.min(to)), f32::from(from.max(to))) as u16 } else { to };
    match (next == from, from.cmp(&to)) {
        (true, std::cmp::Ordering::Less) => from + 1,
        (true, std::cmp::Ordering::Greater) => from - 1,
        _ => next,
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Drag {
    #[default]
    None,
    Select,
    Rows(u32),
    Cols(u32),
    /// Reordering one contiguous whole-column selection by its header.
    MoveCols {
        source: RangeRef,
        before: Option<u32>,
    },
    ResizeCol {
        col: u32,
        start: f32,
        orig: f32,
    },
    ResizeRow {
        row: u32,
        start: f32,
        orig: f32,
    },
    /// Dragging the fill handle; `menu` when the right button drags it (the fill menu opens on release).
    Fill {
        target: Option<RangeRef>,
        menu: bool,
    },
    /// Dragging a freeze handle: `cols` for the column bar, `to` the line count it would freeze.
    Freeze {
        cols: bool,
        to: Option<u32>,
    },
    /// Moving the selection by its border (cut & paste).
    Move {
        grab: CellRef,
        to: Option<CellRef>,
    },
    Object {
        kind: &'static str,
        id: u32,
        start: Pos2,
        orig: (CellRef, f32, f32, f32, f32),
        resize: bool,
    },
}

/// The fill a left-drag of the handle just made, which the Auto Fill Options button can redo
/// another way. It only applies while nothing else changed the workbook (`revision`) and the
/// filled cells are still selected.
#[derive(Clone, Debug, PartialEq)]
pub struct FillTag {
    pub src: RangeRef,
    pub target: RangeRef,
    pub uid: u64,
    pub sheet: usize,
    pub revision: u64,
    pub undo_len: usize,
}

/// Trace Precedents / Trace Dependents arrows: they stay on the cell they were traced from, on its
/// sheet, while the selection moves. They go away when anything changes the workbook
/// (`revision`): Excel removes them on edits to the traced formula and on inserted, deleted or
/// moved cells, this also on a new value in a precedent.
pub struct Trace {
    pub uid: u64,
    pub revision: u64,
    pub sheet: usize,
    pub cell: CellRef,
    /// The command's result: `{"sheet", "range"}` precedents or `{"sheet", "cell"}` dependents.
    pub items: serde_json::Value,
}

impl Trace {
    /// A trace of the active cell, just run.
    pub fn new(app: &SheetApp, items: serde_json::Value) -> Option<Trace> {
        app.session.active().map(|d| Trace { uid: d.uid, revision: d.revision, sheet: d.wb.active_sheet(), cell: d.selection.active, items })
    }
}

#[derive(Default)]
pub struct GridState {
    pub drag: Drag,
    pub ensure_visible: bool,
    /// With `ensure_visible`: scroll to the moving end of the selection instead of the active cell
    /// (Shift/Ctrl+Shift+arrows keep the active cell on the anchor while the far corner moves).
    pub reveal_moving_end: bool,
    /// With `ensure_visible`: the cell an arrow or paging key moved from (the moving end when
    /// extending), set only by those keys. A move from the scrolled rows or columns into the
    /// frozen ones scrolls them back to their top or left.
    pub reveal_from: Option<CellRef>,
    pub last_title: Option<String>,
    pub drag_select: bool,
    pub rect: Option<Rect>,
    pub cells_rect: Option<Rect>,
    /// Column and row of the clicked header cell, and where to open the menu.
    pub filter_menu: Option<(u32, u32, Pos2)>,
    pub context_menu: Option<Pos2>,
    /// The frame a right-click opened `context_menu` in: that click's own press doesn't close it.
    pub context_menu_frame: u64,
    /// After a right-drag of the fill handle: the source, the dragged range and where to open the menu.
    pub fill_menu: Option<(RangeRef, RangeRef, Pos2)>,
    /// After a left-drag of the fill handle: the Auto Fill Options button beside the filled cells.
    pub fill_tag: Option<FillTag>,
    /// The right-clicked chart, picture, shape or slicer: where to open its menu, its kind and id.
    pub object_menu: Option<(Pos2, &'static str, u32)>,
    pub marching_phase: f32,
    /// The selection outline on its way to a new selection.
    pub glide: crate::selection_glide::SelectionGlide,
    pub last_click: Option<(CellRef, f64)>,
    pub hover_cell: Option<CellRef>,
    pub last_autosave: f64,
    pub last_border: String,
    pub last_fill: String,
    pub last_font_color: String,
    pub trace: Option<Trace>,
    pub renaming_tab: Option<usize>,
    pub rename_text: String,
    pub list_picker: Option<CellRef>,
    /// Open task pane: comments, watch, selection, formatChart.
    pub pane: Option<String>,
    /// Ink stroke being drawn (sheet points).
    pub ink: Vec<[f32; 2]>,
    /// Page ranges for Page Break Preview, cached per (doc uid, revision, sheet).
    pub pages: Option<PagesCache>,
    /// The PivotTable Fields pane's `pivot.fields` answer, cached per (doc uid, revision, pivot):
    /// it scans the whole source list, far too slow to repeat every frame (#2113).
    pub pivot_fields: Option<((u64, u64, String), serde_json::Value)>,
    /// The cells of a PivotTable report where the cell menu offers Show Details, relative to the
    /// report's top-left and cached per (doc uid, revision, sheet, pivot): finding them computes
    /// the whole report. `None` when the report can't be computed or is out of date: every cell
    /// of it offers Show Details, and running it says why.
    pub show_details_cells: Option<DetailsCache>,
    pub header_menu: Option<(Pos2, bool)>,
    /// Page Layout view pagination, cached per (doc uid, revision, sheet, rows, columns).
    pub paper: Option<crate::paper_view::PaperCache>,
    /// Header or footer being edited in place (Page Layout view).
    pub hf_edit: Option<crate::paper_view::HfEdit>,
    /// A page margin being dragged on a ruler (Page Layout view); committed on release.
    pub ruler_drag: Option<crate::paper_view::RulerDrag>,
    /// Insert › Header & Footer: start editing the first page's header once Page Layout view is up.
    pub hf_start: bool,
    /// The grid geometry of the last frame (hit-testing for agents and tests).
    pub geo: Option<Geo>,
    /// The horizontal scrollbar's track in the sheet tab bar (hit-testing for tests).
    pub hscroll: Option<Rect>,
    /// The handle between the sheet tabs and the horizontal scrollbar (hit-testing for tests).
    pub tab_split: Option<Rect>,
    /// The share of the tab bar each workbook's tabs (and New sheet) take after the handle was
    /// dragged, by document uid. A workbook without one gives its tabs up to 60%.
    pub tab_shares: std::collections::HashMap<u64, f32>,
    /// Ctrl+wheel zoom in progress (see [`WheelZoom`]).
    pub wheel_zoom: Option<WheelZoom>,
    /// A one-finger touch drag in progress: it pans the sheet instead of selecting.
    pub touch_pan: bool,
    /// What the macro step editor shows on the grid (Check, debugging).
    pub macro_overlay: Option<crate::macro_editor::MacroOverlay>,
}

/// Ctrl+wheel zoom for one (doc uid, sheet). egui eases a notch in over several frames and
/// fine-grained wheels send parts of one, so the turning is counted in notches, and each whole
/// notch steps [`WHEEL_ZOOM_LADDER`]. With animations on the zoom then glides to that step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelZoom {
    pub place: (u64, usize),
    /// Turned so far towards the next step, in notches (+ in, − out).
    pub notches: f32,
    /// When `notches` last changed.
    pub time: f64,
    /// The step the zoom is gliding to, and the percentage set last on the way.
    pub glide: Option<(u16, u16)>,
}

impl WheelZoom {
    /// The state for `place` showing `zoom` percent: fresh for another document or sheet, and
    /// without its glide if the zoom was changed some other way since the glide set it.
    fn at(app: &SheetApp, place: (u64, usize), zoom: u16, now: f64) -> WheelZoom {
        let mut w = app.grid.wheel_zoom.filter(|w| w.place == place).unwrap_or(WheelZoom { place, notches: 0.0, time: now, glide: None });
        if w.glide.is_some_and(|(_, shown)| shown != zoom) {
            w.glide = None;
        }
        w
    }
}

/// Row and column heading labels grow with the zoom as the cells do, as in Excel (at 400% the
/// labels are four times their 100% size), and never get smaller than a readable 8.5 points.
pub(crate) fn heading_font_size(zoom: f32) -> f32 {
    (11.5 * (zoom / DISPLAY_SCALE)).max(8.5)
}

#[cfg(test)]
mod tests;
