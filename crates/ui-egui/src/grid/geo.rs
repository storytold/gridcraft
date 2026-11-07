use super::{Arc, COL_HEADER_H, CellRef, DISPLAY_SCALE, LineIndex, MAX_COLS, MAX_ROWS, Pos2, RangeRef, Rect, Sheet, pos2};

/// Geometry of the grid for one frame (handles zoom and frozen panes).
#[derive(Clone)]
pub struct Geo {
    pub rect: Rect,
    pub cells: Rect,
    pub z: f32,
    pub header_w: f32,
    pub header_h: f32,
    /// The outline bands (grouped rows / columns): left of the row headers and above the column
    /// headers; 0 without an outline (see `outline`).
    pub band_w: f32,
    pub band_h: f32,
    pub row_outline: Arc<crate::outline::Axis>,
    pub col_outline: Arc<crate::outline::Axis>,
    pub fr: u32,
    pub fc: u32,
    pub frozen_w: f32,
    pub frozen_h: f32,
    pub scroll: egui::Vec2,
    /// Sheet Right-to-Left: everything is laid out left-to-right as usual (these "logical"
    /// coordinates are what `x`, `cell_rect`, `col_at`… return) and mirrored across `rect`'s
    /// vertical centre line on screen: painted shapes by `mirror_painted`, widgets by
    /// `mirror_rect`/`mirror_pos`, and pointer input back by `mirror_pos` (an involution).
    pub rtl: bool,
    /// Prefix sums over custom row heights / column widths, built once per frame.
    pub(super) ri: std::sync::Arc<LineIndex>,
    pub(super) ci: std::sync::Arc<LineIndex>,
    /// Page Layout view: cells sit on pages (`scroll` is then in page-layout units).
    pub paper: Option<Arc<gridcraft_engine::paper::PageGrid>>,
}

/// The line an arrow of a scrollbar goes to from `first`, which is partly scrolled out or not.
fn one_line(first: u32, partly_out: bool, forward: bool, frozen: u32, end: u32, shown: impl Fn(u32) -> bool) -> u32 {
    if forward {
        return (first + 1..end).find(|l| shown(*l)).unwrap_or(first);
    }
    if partly_out {
        return first;
    }
    (frozen..first).rev().find(|l| shown(*l)).unwrap_or(frozen)
}

impl Geo {
    pub fn new(sh: &Sheet, rect: Rect, scroll: egui::Vec2) -> Geo {
        let zoom = (sh.zoom as f32 / 100.0).clamp(0.1, 4.0);
        let z = zoom * DISPLAY_SCALE;
        let header_h = if sh.show_headings { (COL_HEADER_H * zoom.max(0.75)).round() } else { 0.0 };
        let ri = std::sync::Arc::new(LineIndex::new(&sh.rows, sh.default_row_height, MAX_ROWS));
        let ci = std::sync::Arc::new(LineIndex::new(&sh.cols, sh.default_col_width, MAX_COLS));
        let last_row = ri.at((scroll.y + rect.height() / z) as f64 + 200.0) + 1;
        let digits = (last_row as f32).log10().floor() as i32 + 1;
        let header_w = if sh.show_headings { ((digits.max(2) as f32 * 7.0 + 14.0) * zoom.max(0.75)).round() } else { 0.0 };
        let (fr, fc) = sh.freeze.unwrap_or((0, 0));
        let frozen_w = (ci.start(fc) as f32) * z;
        let frozen_h = (ri.start(fr) as f32) * z;
        let row_outline = Arc::new(crate::outline::Axis::of(sh, true));
        let col_outline = Arc::new(crate::outline::Axis::of(sh, false));
        // Show Outline Symbols off: no bands (the outline's hidden lines stay hidden).
        let symbols = sh.show_outline_symbols;
        let mut band_w = if symbols { crate::outline::band(row_outline.levels, zoom) } else { 0.0 };
        let mut band_h = if symbols { crate::outline::band(col_outline.levels, zoom) } else { 0.0 };
        // Without headings the level buttons still need a corner to sit in.
        if symbols && row_outline.levels > 0 && header_h == 0.0 {
            band_h = band_h.max(crate::outline::lane(zoom) + 3.0);
        }
        if symbols && col_outline.levels > 0 && header_w == 0.0 {
            band_w = band_w.max(crate::outline::lane(zoom) + 3.0);
        }
        let cells = Rect::from_min_max(pos2(rect.left() + band_w + header_w, rect.top() + band_h + header_h), rect.max);
        Geo {
            rect,
            cells,
            z,
            header_w,
            header_h,
            band_w,
            band_h,
            row_outline,
            col_outline,
            fr,
            fc,
            frozen_w,
            frozen_h,
            scroll,
            rtl: sh.right_to_left,
            ri,
            ci,
            paper: None,
        }
    }
    /// Left edge of the row headers (right of the outline band).
    pub fn head_left(&self) -> f32 {
        self.cells.left() - self.header_w
    }
    /// Top edge of the column headers (below the outline band).
    pub fn head_top(&self) -> f32 {
        self.cells.top() - self.header_h
    }
    /// Page Layout view geometry: `rect` excludes the rulers; cells are drawn at the print scale
    /// on pages laid out by `grid`; frozen panes don't apply (as in Excel).
    pub fn paged(sh: &Sheet, rect: Rect, scroll: egui::Vec2, grid: Arc<gridcraft_engine::paper::PageGrid>) -> Geo {
        let mut g = Geo::new(sh, rect, scroll);
        let zoom = (sh.zoom as f32 / 100.0).clamp(0.1, 4.0);
        g.z = zoom * DISPLAY_SCALE * grid.scale.clamp(0.1, 4.0);
        (g.fr, g.fc, g.frozen_w, g.frozen_h) = (0, 0, 0.0, 0.0);
        g.paper = Some(grid);
        g
    }
    /// The point is over cells (in Page Layout view: on a page, inside its margins).
    pub fn in_cells(&self, p: Pos2) -> bool {
        if !self.cells.contains(p) {
            return false;
        }
        match &self.paper {
            Some(pg) => pg.on_cells(
                self.scroll.x as f64 + ((p.x - self.cells.left()) / self.z) as f64,
                self.scroll.y as f64 + ((p.y - self.cells.top()) / self.z) as f64,
            ),
            None => true,
        }
    }
    /// Scroll offset (in `scroll` units) at which `row` reaches the top of the scrolled area.
    pub fn scroll_top_of(&self, sh: &Sheet, row: u32) -> f32 {
        match &self.paper {
            Some(pg) => pg.row_y(row) as f32,
            None => (sh.row_top(row) - sh.row_top(self.fr)) as f32,
        }
    }
    /// Scroll offset (in `scroll` units) at which `col` reaches the left of the scrolled area.
    pub fn scroll_left_of(&self, sh: &Sheet, col: u32) -> f32 {
        match &self.paper {
            Some(pg) => pg.col_x(col) as f32,
            None => (sh.col_left(col) - sh.col_left(self.fc)) as f32,
        }
    }
    /// The scroll that brings the next column to the left of the scrolled area, or back to the start
    /// of the one partly scrolled out and then the one before it. Hidden columns are skipped.
    pub fn scroll_one_col(&self, sh: &Sheet, scroll: f32, forward: bool) -> f32 {
        if self.paper.is_some() {
            return scroll + if forward { sh.default_col_width } else { -(sh.default_col_width) };
        }
        let first = self.ci.at(scroll as f64 + self.ci.start(self.fc));
        let target = one_line(first, scroll > self.scroll_left_of(sh, first) + 0.01, forward, self.fc, MAX_COLS, |c| sh.col_width(c) > 0.0);
        self.scroll_left_of(sh, target)
    }
    /// `scroll_one_col` for rows.
    pub fn scroll_one_row(&self, sh: &Sheet, scroll: f32, forward: bool) -> f32 {
        if self.paper.is_some() {
            return scroll + if forward { sh.default_row_height } else { -(sh.default_row_height) };
        }
        let first = self.ri.at(scroll as f64 + self.ri.start(self.fr));
        let target = one_line(first, scroll > self.scroll_top_of(sh, first) + 0.01, forward, self.fr, MAX_ROWS, |r| sh.row_height(r) > 0.0);
        self.scroll_top_of(sh, target)
    }
    /// Screen x for a logical x (and back): identity unless the sheet is right-to-left.
    pub fn mirror_x(&self, x: f32) -> f32 {
        if self.rtl { self.rect.left() + self.rect.right() - x } else { x }
    }
    pub fn mirror_pos(&self, p: Pos2) -> Pos2 {
        pos2(self.mirror_x(p.x), p.y)
    }
    pub fn mirror_rect(&self, r: Rect) -> Rect {
        if self.rtl { Rect::from_min_max(pos2(self.mirror_x(r.right()), r.top()), pos2(self.mirror_x(r.left()), r.bottom())) } else { r }
    }
    /// On-screen rectangle of a cell (`cell_rect` mirrored for right-to-left sheets).
    pub fn screen_cell_rect(&self, sh: &Sheet, c: CellRef) -> Rect {
        self.mirror_rect(self.cell_rect(sh, c))
    }
    pub fn x(&self, _sh: &Sheet, col: u32) -> f32 {
        if let Some(pg) = &self.paper {
            return self.cells.left() + (pg.col_x(col) - self.scroll.x as f64) as f32 * self.z;
        }
        if col < self.fc {
            self.cells.left() + self.ci.start(col) as f32 * self.z
        } else {
            self.cells.left() + self.frozen_w + (self.ci.start(col) as f32 - self.ci.start(self.fc) as f32 - self.scroll.x) * self.z
        }
    }
    pub fn y(&self, _sh: &Sheet, row: u32) -> f32 {
        if let Some(pg) = &self.paper {
            return self.cells.top() + (pg.row_y(row) - self.scroll.y as f64) as f32 * self.z;
        }
        if row < self.fr {
            self.cells.top() + self.ri.start(row) as f32 * self.z
        } else {
            self.cells.top() + self.frozen_h + (self.ri.start(row) as f32 - self.ri.start(self.fr) as f32 - self.scroll.y) * self.z
        }
    }
    /// Screen x of `col`'s right edge. Not `x(col + 1)`: across the freeze line (and between
    /// pages) the next column is laid out elsewhere (#2544).
    pub fn x_end(&self, sh: &Sheet, col: u32) -> f32 {
        self.x(sh, col) + sh.col_width(col) * self.z
    }
    /// Screen y of `row`'s bottom edge (see `x_end`).
    pub fn y_end(&self, sh: &Sheet, row: u32) -> f32 {
        self.y(sh, row) + sh.row_height(row) * self.z
    }
    /// Columns `a` and `b` are on the same side of the freeze line, so they scroll together.
    pub fn same_col_pane(&self, a: u32, b: u32) -> bool {
        (a < self.fc) == (b < self.fc)
    }
    pub fn cell_rect(&self, sh: &Sheet, c: CellRef) -> Rect {
        Rect::from_min_max(pos2(self.x(sh, c.col), self.y(sh, c.row)), pos2(self.x_end(sh, c.col), self.y_end(sh, c.row)))
    }
    pub fn range_rect(&self, sh: &Sheet, r: RangeRef) -> Rect {
        let a = self.cell_rect(sh, r.start);
        let b = self.cell_rect(sh, r.end);
        Rect::from_min_max(a.min, b.max)
    }
    pub fn col_at(&self, _sh: &Sheet, x: f32) -> u32 {
        if let Some(pg) = &self.paper {
            return pg.col_at(self.scroll.x as f64 + ((x - self.cells.left()) / self.z) as f64);
        }
        let dx = (x - self.cells.left()) / self.z;
        if dx < self.frozen_w / self.z {
            self.ci.at(dx.max(0.0) as f64)
        } else {
            self.ci.at(self.ci.start(self.fc) + self.scroll.x as f64 + (dx - self.frozen_w / self.z) as f64)
        }
    }
    pub fn row_at(&self, _sh: &Sheet, y: f32) -> u32 {
        if let Some(pg) = &self.paper {
            return pg.row_at(self.scroll.y as f64 + ((y - self.cells.top()) / self.z) as f64);
        }
        let dy = (y - self.cells.top()) / self.z;
        if dy < self.frozen_h / self.z {
            self.ri.at(dy.max(0.0) as f64)
        } else {
            self.ri.at(self.ri.start(self.fr) + self.scroll.y as f64 + (dy - self.frozen_h / self.z) as f64)
        }
    }
    pub fn cell_at(&self, sh: &Sheet, p: Pos2) -> CellRef {
        CellRef::new(self.row_at(sh, p.y), self.col_at(sh, p.x))
    }
    /// Visible scrolled rows (from the first visible row after the frozen ones).
    pub(super) fn scroll_rows(&self, _sh: &Sheet) -> (u32, u32) {
        let top = self.ri.start(self.fr) + self.scroll.y as f64;
        let r0 = self.ri.at(top).max(self.fr);
        let r1 = self.ri.at(top + ((self.cells.height() - self.frozen_h) / self.z) as f64).min(MAX_ROWS - 1);
        (r0, r1)
    }
    pub(super) fn scroll_cols(&self, _sh: &Sheet) -> (u32, u32) {
        let left = self.ci.start(self.fc) + self.scroll.x as f64;
        let c0 = self.ci.at(left).max(self.fc);
        let c1 = self.ci.at(left + ((self.cells.width() - self.frozen_w) / self.z) as f64).min(MAX_COLS - 1);
        (c0, c1)
    }
    /// Screen areas of the panes (frozen rows/columns or the scrolling part) that hold part of `r`.
    /// The panes share one coordinate system, so a range that is not clipped to these is also drawn
    /// over the frozen rows/columns when it is scrolled under them.
    pub fn range_panes(&self, r: RangeRef) -> Vec<Rect> {
        let split = pos2(self.cells.left() + self.frozen_w, self.cells.top() + self.frozen_h);
        let mut xs = Vec::new();
        if r.start.col < self.fc {
            xs.push((self.cells.left(), split.x));
        }
        if r.end.col >= self.fc {
            xs.push((split.x, self.cells.right()));
        }
        let mut ys = Vec::new();
        if r.start.row < self.fr {
            ys.push((self.cells.top(), split.y));
        }
        if r.end.row >= self.fr {
            ys.push((split.y, self.cells.bottom()));
        }
        ys.iter().flat_map(|&(y0, y1)| xs.iter().map(move |&(x0, x1)| Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)))).collect()
    }
    /// True when cell `c` is in the scrolling pane but scrolled entirely under the frozen rows or columns.
    pub fn hidden_under_frozen(&self, sh: &Sheet, c: CellRef) -> bool {
        let r = self.cell_rect(sh, c);
        (self.fc > 0 && c.col >= self.fc && r.right() <= self.cells.left() + self.frozen_w)
            || (self.fr > 0 && c.row >= self.fr && r.bottom() <= self.cells.top() + self.frozen_h)
    }
}
