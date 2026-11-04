//! The outline band of grouped rows and columns (Data ▸ Group), as in Excel: a strip left of the
//! row headers and one above the column headers, one lane per outline level, with a bracket per
//! expanded group, a − / + button on its summary line (below / right of the detail, or above /
//! left of it when the sheet's outline settings say so) and the level buttons 1…n+1 in the
//! corner. The bands exist only while the sheet has an outline.
//!
//! Layout and hit-testing are pure functions of [`Geo`] in logical (left-to-right) coordinates,
//! so right-to-left sheets get them mirrored with the rest of the grid. Clicks run commands
//! (`data.hideDetail`, `data.showDetail`, `data.showLevels`), so they undo like any edit.

use egui::{Align2, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use gridcraft_engine::core::col_to_letters;
use gridcraft_engine::model::{LineInfo, Sheet};
use gridcraft_engine::outline::{self, OutlineGroup};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

use crate::grid::{DISPLAY_SCALE, Geo};
use crate::theme::{self, Tokens};

/// Width of one level's lane at 100% zoom (points).
const LANE: f32 = 15.0;

/// The outline of one direction (rows or columns): its deepest level and its groups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    pub levels: u8,
    pub groups: Vec<OutlineGroup>,
}

impl Axis {
    pub fn new(lines: &BTreeMap<u32, LineInfo>) -> Axis {
        Axis { levels: outline::max_level(lines), groups: outline::groups(lines) }
    }
    /// The outline of a sheet's rows (`rows`) or columns, with its summary lines where the
    /// sheet's outline settings put them.
    pub fn of(sh: &Sheet, rows: bool) -> Axis {
        let lines = if rows { &sh.rows } else { &sh.cols };
        Axis { levels: outline::max_level(lines), groups: outline::groups_with(lines, outline::summary_before(sh, rows)) }
    }
}

/// One lane's width at this zoom (`zoom` is the sheet zoom, 1.0 = 100%).
pub fn lane(zoom: f32) -> f32 {
    (LANE * zoom.max(0.75)).round()
}

/// Thickness of the band for an outline `levels` deep: a lane per level plus one for the level
/// button that shows everything. No outline, no band.
pub fn band(levels: u8, zoom: f32) -> f32 {
    if levels == 0 { 0.0 } else { (levels as f32 + 1.0) * lane(zoom) + 3.0 }
}

/// What a button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// A level button: show `rows` (or columns) down to this level.
    Level { rows: bool, level: u8 },
    /// A group's − (hide its detail) or + (show it) button.
    Toggle { rows: bool, group: OutlineGroup },
}

impl Action {
    /// The command (and its parameters) the button runs.
    pub fn command(&self) -> (&'static str, Json) {
        match *self {
            Action::Level { rows, level } => ("data.showLevels", json!({ if rows { "rows" } else { "cols" }: level })),
            Action::Toggle { rows, group } => {
                let id = if group.collapsed { "data.showDetail" } else { "data.hideDetail" };
                let (key, lines) = if rows {
                    ("rows", format!("{}:{}", group.start as u64 + 1, group.end as u64 + 1))
                } else {
                    ("cols", format!("{}:{}", col_to_letters(group.start), col_to_letters(group.end)))
                };
                (id, json!({ key: lines, "level": group.level }))
            }
        }
    }
    /// The button's label: the level digit, − or +.
    pub fn label(&self) -> String {
        match *self {
            Action::Level { level, .. } => level.to_string(),
            Action::Toggle { group, .. } => if group.collapsed { "+" } else { "\u{2212}" }.to_string(),
        }
    }
}

/// A button in the band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub action: Action,
}

/// An expanded group's bracket: a line along its lane from the first detail line to its button,
/// with a tick at the start (`tick` is `None` when the start is scrolled out of view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bracket {
    pub line: [Pos2; 2],
    pub tick: Option<[Pos2; 2]>,
}

/// Everything drawn in the bands this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub buttons: Vec<Button>,
    pub brackets: Vec<Bracket>,
}

/// The row band (left of the row headers) and the column band (above the column headers).
pub fn bands(geo: &Geo) -> (Rect, Rect) {
    let r = geo.rect;
    let rows = Rect::from_min_max(r.min, pos2(r.left() + geo.band_w, r.bottom()));
    let cols = Rect::from_min_max(r.min, pos2(r.right(), r.top() + geo.band_h));
    (rows, cols)
}

/// One direction's geometry, in (along, across) coordinates: along the lines, across the lanes.
struct Dir<'a> {
    rows: bool,
    geo: &'a Geo,
    sh: &'a Sheet,
    /// The cells' extent along, where the scrolling pane begins, and the first scrolling line.
    lo: f32,
    hi: f32,
    split: f32,
    frozen: u32,
    /// Lane `k`'s centre across (1-based).
    lane0: f32,
    /// Centre across, and along, of this direction's level buttons.
    level_along: f32,
}

impl Dir<'_> {
    /// Where the line starts on screen, and its size.
    fn start(&self, line: u32) -> f32 {
        if self.rows { self.geo.y(self.sh, line) } else { self.geo.x(self.sh, line) }
    }
    fn size(&self, line: u32) -> f32 {
        self.geo.z * if self.rows { self.sh.row_height(line) } else { self.sh.col_width(line) }
    }
    fn lane(&self, step: f32, k: u8) -> f32 {
        self.lane0 + (k as f32 - 0.5) * step
    }
    /// The visible extent of a line along: clipped to its pane, `None` when hidden or off screen.
    fn visible(&self, line: u32) -> Option<(f32, f32)> {
        let (a, size) = (self.start(line), self.size(line));
        let top = if line >= self.frozen { self.split } else { self.lo };
        let (a0, a1) = (a.max(top), (a + size).min(self.hi));
        (size > 0.0 && a1 > a0).then_some((a0, a1))
    }
    /// The first on-screen line in `start..=end`: the frozen ones first, then the scrolling ones from the line
    /// at the pane's edge, so a long group is not scanned from its far-off start.
    fn first_visible(&self, start: u32, end: u32) -> Option<(u32, (f32, f32))> {
        let at = if self.rows { self.geo.row_at(self.sh, self.split) } else { self.geo.col_at(self.sh, self.split) };
        let scrolled = start.max(self.frozen).max(at);
        (start..self.frozen.min(end.saturating_add(1))).chain(scrolled..=end).find_map(|l| self.visible(l).map(|v| (l, v)))
    }
    fn point(&self, along: f32, across: f32) -> Pos2 {
        if self.rows { pos2(across, along) } else { pos2(along, across) }
    }
    fn rect(&self, along: f32, across: f32, side: f32) -> Rect {
        Rect::from_center_size(self.point(along, across), vec2(side, side))
    }
}

/// The band's buttons and brackets for this frame.
/// The bracket for one group's detail, and its tick, on the lane `across`; `button` is the group's
/// summary button when it is on screen.
fn group_bracket(out: &mut Layout, dir: &Dir, grp: &OutlineGroup, button: Option<f32>, across: f32, side: f32) {
    let first = dir.first_visible(grp.start, grp.end);
    let Some((first_line, (a0, _))) = first else { return };
    if grp.summary_before {
        // From the button down (or across) to the last detail line, with the tick there.
        let a0 = button.map_or(a0, |mid| mid + side / 2.0);
        let end = dir.start(grp.end) + dir.size(grp.end);
        let a1 = end.min(dir.hi);
        if a1 <= a0 {
            return;
        }
        let tick = (end <= dir.hi).then(|| [dir.point(a1 - 0.5, across), dir.point(a1 - 0.5, across + side / 2.0)]);
        out.brackets.push(Bracket { line: [dir.point(a0, across), dir.point(a1 - 0.5, across)], tick });
        return;
    }
    let a1 = match button {
        Some(mid) => mid - side / 2.0,
        None => dir.start(grp.summary()).min(dir.hi),
    };
    if a1 <= a0 {
        return;
    }
    let start_shown = first_line == grp.start && (a0 - dir.start(grp.start)).abs() < 0.5;
    let tick = start_shown.then(|| [dir.point(a0 + 0.5, across), dir.point(a0 + 0.5, across + side / 2.0)]);
    out.brackets.push(Bracket { line: [dir.point(a0 + 0.5, across), dir.point(a1, across)], tick });
}

pub fn layout(geo: &Geo, sh: &Sheet) -> Layout {
    let mut out = Layout::default();
    let zoom = (geo.z / DISPLAY_SCALE).max(0.1);
    let step = lane(zoom);
    let side = (step - 4.0).max(7.0);
    let corner_h = if geo.header_h > 0.0 { geo.header_h } else { geo.band_h };
    let corner_w = if geo.header_w > 0.0 { geo.header_w } else { geo.band_w };
    let rows = Dir {
        rows: true,
        geo,
        sh,
        lo: geo.cells.top(),
        hi: geo.cells.bottom(),
        split: geo.cells.top() + geo.frozen_h,
        frozen: geo.fr,
        lane0: geo.rect.left() + 2.0,
        level_along: geo.cells.top() - corner_h / 2.0,
    };
    let cols = Dir {
        rows: false,
        geo,
        sh,
        lo: geo.cells.left(),
        hi: geo.cells.right(),
        split: geo.cells.left() + geo.frozen_w,
        frozen: geo.fc,
        lane0: geo.rect.top() + 2.0,
        level_along: geo.cells.left() - corner_w / 2.0,
    };
    for (dir, axis) in [(&rows, &geo.row_outline), (&cols, &geo.col_outline)] {
        if axis.levels == 0 {
            continue;
        }
        for k in 1..=axis.levels + 1 {
            out.buttons.push(Button { rect: dir.rect(dir.level_along, dir.lane(step, k), side), action: Action::Level { rows: dir.rows, level: k } });
        }
        for grp in &axis.groups {
            let across = dir.lane(step, grp.level);
            // The button on the summary line, if that line is on screen.
            let button = dir.visible(grp.summary()).map(|(a0, a1)| (a0 + a1) / 2.0).filter(|_| grp.has_summary());
            if let Some(mid) = button {
                out.buttons.push(Button { rect: dir.rect(mid, across, side), action: Action::Toggle { rows: dir.rows, group: *grp } });
            }
            if grp.collapsed {
                continue;
            }
            // The bracket, over the part of the detail that is on screen.
            group_bracket(&mut out, dir, grp, button, across, side);
        }
    }
    out
}

/// The button under `p` (logical coordinates), if any.
pub fn hit(geo: &Geo, sh: &Sheet, p: Pos2) -> Option<Action> {
    // Hidden symbols (or no outline) leave empty bands: a click on their edge is a cell click.
    if geo.band_w <= 0.0 && geo.band_h <= 0.0 {
        return None;
    }
    let (rows, cols) = bands(geo);
    if !rows.contains(p) && !cols.contains(p) {
        return None;
    }
    layout(geo, sh).buttons.into_iter().find(|b| b.rect.expand(1.5).contains(p)).map(|b| b.action)
}

/// Paints the bands (in logical coordinates, before the grid is mirrored for right-to-left).
pub fn paint(p: &Painter, geo: &Geo, sh: &Sheet, t: &Tokens) {
    if geo.band_w <= 0.0 && geo.band_h <= 0.0 {
        return;
    }
    let (rows, cols) = bands(geo);
    for band in [rows, cols] {
        if band.width() > 0.0 && band.height() > 0.0 {
            p.rect_filled(band, 0.0, t.window);
        }
    }
    if geo.band_w > 0.0 {
        p.line_segment([pos2(rows.right() - 0.5, rows.top()), pos2(rows.right() - 0.5, rows.bottom())], Stroke::new(1.0, t.header_line));
    }
    if geo.band_h > 0.0 {
        p.line_segment([pos2(cols.left(), cols.bottom() - 0.5), pos2(cols.right(), cols.bottom() - 0.5)], Stroke::new(1.0, t.header_line));
    }
    let l = layout(geo, sh);
    let ink = Stroke::new(1.0, t.header_text);
    // Brackets stay out of the corner, where the level buttons are.
    let row_clip = Rect::from_min_max(pos2(rows.left(), geo.cells.top()), rows.max);
    let col_clip = Rect::from_min_max(pos2(geo.cells.left(), cols.top()), cols.max);
    for b in &l.brackets {
        let clip = if b.line[0].x == b.line[1].x { row_clip } else { col_clip };
        let pc = p.with_clip_rect(clip);
        pc.line_segment(b.line, ink);
        if let Some(tick) = b.tick {
            pc.line_segment(tick, ink);
        }
    }
    // The outline levels grow with the zoom like the headings beside them.
    let font = theme::ui_font((10.0 * (geo.z / DISPLAY_SCALE)).max(7.5));
    for b in &l.buttons {
        let clip = match b.action {
            Action::Toggle { rows: true, .. } => row_clip,
            Action::Toggle { rows: false, .. } => col_clip,
            Action::Level { .. } => geo.rect,
        };
        let pc = p.with_clip_rect(clip);
        pc.rect_filled(b.rect, 1.0, t.header_bg);
        pc.rect_stroke(b.rect, 1.0, ink, StrokeKind::Inside);
        pc.text(b.rect.center(), Align2::CENTER_CENTER, b.action.label(), font.clone(), t.header_text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::core::CellRef;

    /// Rows 5:8 grouped with 6:7 inside, columns C:E grouped.
    fn sheet() -> Sheet {
        let mut sh = Sheet::new("Sheet1");
        for (r, level) in [(4, 1), (5, 2), (6, 2), (7, 1)] {
            sh.rows.entry(r).or_default().outline = level;
        }
        for c in 2..=4 {
            sh.cols.entry(c).or_default().outline = 1;
        }
        sh
    }

    fn geo(sh: &Sheet) -> Geo {
        Geo::new(sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0)), vec2(0.0, 0.0))
    }

    #[test]
    fn no_outline_no_band() {
        let sh = Sheet::new("Sheet1");
        let g = geo(&sh);
        assert_eq!((g.band_w, g.band_h), (0.0, 0.0));
        assert_eq!(layout(&g, &sh), Layout::default());
        assert_eq!(g.head_left(), g.rect.left());
    }

    #[test]
    fn long_groups_keep_their_bracket_past_4096_lines() {
        for rows in [true, false] {
            let mut sh = Sheet::new("S");
            let map = if rows { &mut sh.rows } else { &mut sh.cols };
            for i in 0..5000u32 {
                map.entry(i).or_default().outline = 1;
            }
            for line in [0u32, 4095, 4096, 4200, 4970] {
                let scroll = if rows { vec2(0.0, sh.row_top(line) as f32) } else { vec2(sh.col_left(line) as f32, 0.0) };
                let g = Geo::new(&sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)), scroll);
                let l = layout(&g, &sh);
                assert_eq!(l.brackets.len(), 1, "rows={rows} line={line}");
                assert_eq!(l.brackets[0].tick.is_some(), line == 0, "rows={rows} line={line}");
            }
            // Past the group's summary line nothing is left to bracket.
            let scroll = if rows { vec2(0.0, sh.row_top(5100) as f32) } else { vec2(sh.col_left(5100) as f32, 0.0) };
            let g = Geo::new(&sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0)), scroll);
            assert!(layout(&g, &sh).brackets.is_empty(), "rows={rows}");
        }
    }

    #[test]
    fn bands_push_the_headers_and_cells_in() {
        let sh = sheet();
        let g = geo(&sh);
        let plain = geo(&Sheet::new("Sheet1"));
        assert_eq!(g.band_w, band(2, 1.0));
        assert_eq!(g.band_h, band(1, 1.0));
        assert!(g.band_w > g.band_h && g.band_h > 0.0);
        assert_eq!(g.head_left(), g.rect.left() + g.band_w);
        assert_eq!(g.head_top(), g.rect.top() + g.band_h);
        assert_eq!(g.cells.left(), plain.cells.left() + g.band_w);
        assert_eq!(g.cells.top(), plain.cells.top() + g.band_h);
    }

    #[test]
    fn level_buttons_sit_in_the_corner_and_toggles_on_summary_lines() {
        let sh = sheet();
        let g = geo(&sh);
        let l = layout(&g, &sh);
        let levels: Vec<_> =
            l.buttons.iter().filter_map(|b| if let Action::Level { rows, level } = b.action { Some((rows, level)) } else { None }).collect();
        assert_eq!(levels, [(true, 1), (true, 2), (true, 3), (false, 1), (false, 2)]);
        // Row level buttons: side by side in the row band, level with the column headers.
        let row1 = l.buttons.iter().find(|b| b.action == Action::Level { rows: true, level: 1 }).unwrap().rect;
        let row2 = l.buttons.iter().find(|b| b.action == Action::Level { rows: true, level: 2 }).unwrap().rect;
        assert!(row1.right() < row2.left() && (row1.center().y - row2.center().y).abs() < 0.01);
        assert!(row1.center().y > g.head_top() && row1.center().y < g.cells.top() && row2.right() <= g.head_left());
        // The level-1 row group's − is on row 9 (index 8) in lane 1; the inner group's on row 8.
        let toggle = |rows: bool, level: u8| {
            l.buttons.iter().find(|b| matches!(b.action, Action::Toggle { rows: r, group } if r == rows && group.level == level)).unwrap()
        };
        let outer = toggle(true, 1);
        assert!(g.cell_rect(&sh, CellRef::new(8, 0)).y_range().contains(outer.rect.center().y));
        assert!((outer.rect.center().x - row1.center().x).abs() < 0.01);
        let inner = toggle(true, 2);
        assert!(g.cell_rect(&sh, CellRef::new(7, 0)).y_range().contains(inner.rect.center().y));
        assert!((inner.rect.center().x - row2.center().x).abs() < 0.01);
        // Column group C:E: its − is above column F.
        let col = toggle(false, 1);
        assert!(g.cell_rect(&sh, CellRef::new(0, 5)).x_range().contains(col.rect.center().x));
        assert!(col.rect.bottom() <= g.head_top());
        // Brackets: one per expanded group, from the first detail line to its button.
        assert_eq!(l.brackets.len(), 3);
        let b = l.brackets.iter().find(|b| (b.line[0].x - outer.rect.center().x).abs() < 0.01).unwrap();
        assert!((b.line[0].y - g.y(&sh, 4)).abs() < 1.0 && (b.line[1].y - outer.rect.top()).abs() < 0.01);
        assert!(b.tick.is_some());
        assert_eq!(outer.action.label(), "\u{2212}");
    }

    #[test]
    fn summaries_above_and_left_take_the_toggles_and_the_bracket_ticks_move_to_the_end() {
        let mut sh = sheet();
        sh.summary_below = false;
        sh.summary_right = false;
        let g = geo(&sh);
        let l = layout(&g, &sh);
        let toggle = |rows: bool, level: u8| {
            l.buttons.iter().find(|b| matches!(b.action, Action::Toggle { rows: r, group } if r == rows && group.level == level)).unwrap()
        };
        // Rows 5:8: the outer − is on row 4 (index 3), the inner one (rows 6:7) on row 5.
        let outer = toggle(true, 1);
        assert!(g.cell_rect(&sh, CellRef::new(3, 0)).y_range().contains(outer.rect.center().y));
        assert!(g.cell_rect(&sh, CellRef::new(4, 0)).y_range().contains(toggle(true, 2).rect.center().y));
        // Column group C:E: its − is above column B.
        let col = toggle(false, 1);
        assert!(g.cell_rect(&sh, CellRef::new(0, 1)).x_range().contains(col.rect.center().x));
        // The bracket runs from the button to the end of row 8, with the tick there.
        assert_eq!(l.brackets.len(), 3);
        let b = l.brackets.iter().find(|b| (b.line[0].x - outer.rect.center().x).abs() < 0.01).unwrap();
        let row8 = g.cell_rect(&sh, CellRef::new(7, 0));
        assert!((b.line[0].y - outer.rect.bottom()).abs() < 0.01 && (b.line[1].y - (row8.bottom() - 0.5)).abs() < 1.0);
        assert!(b.tick.is_some_and(|t| (t[0].y - b.line[1].y).abs() < 0.01 && t[1].x > t[0].x));
        // A group on the first row has no row above it: no toggle, still a bracket.
        let mut sh = Sheet::new("S");
        sh.summary_below = false;
        for r in 0..=2 {
            sh.rows.entry(r).or_default().outline = 1;
        }
        let g = geo(&sh);
        let l = layout(&g, &sh);
        assert!(!l.buttons.iter().any(|b| matches!(b.action, Action::Toggle { .. })));
        assert_eq!(l.brackets.len(), 1);
    }

    #[test]
    fn collapsed_groups_show_plus_and_no_bracket() {
        let mut sh = sheet();
        for r in 4..=7 {
            sh.rows.entry(r).or_default().hidden = true;
        }
        let g = geo(&sh);
        let l = layout(&g, &sh);
        let outer = l.buttons.iter().find(|b| matches!(b.action, Action::Toggle { rows: true, group } if group.level == 1)).unwrap();
        assert_eq!(outer.action.label(), "+");
        // The inner group's summary row is hidden with the rest: no button for it.
        assert!(!l.buttons.iter().any(|b| matches!(b.action, Action::Toggle { rows: true, group } if group.level == 2)));
        assert_eq!(l.brackets.len(), 1); // only the columns' bracket
        assert_eq!(outer.action.command(), ("data.showDetail", json!({"rows": "5:8", "level": 1})));
    }

    #[test]
    fn hit_finds_buttons_and_maps_them_to_commands() {
        let sh = sheet();
        let g = geo(&sh);
        let l = layout(&g, &sh);
        for b in &l.buttons {
            assert_eq!(hit(&g, &sh, b.rect.center()), Some(b.action));
        }
        assert_eq!(hit(&g, &sh, g.cells.center()), None);
        assert_eq!(hit(&g, &sh, pos2(g.rect.left() + 1.0, g.cells.bottom() - 2.0)), None);
        let col = l.buttons.iter().find(|b| matches!(b.action, Action::Toggle { rows: false, .. })).unwrap();
        assert_eq!(col.action.command(), ("data.hideDetail", json!({"cols": "C:E", "level": 1})));
        assert_eq!(Action::Level { rows: true, level: 2 }.command(), ("data.showLevels", json!({"rows": 2})));
        assert_eq!(Action::Level { rows: false, level: 1 }.command(), ("data.showLevels", json!({"cols": 1})));
    }

    #[test]
    fn groups_scrolled_under_frozen_rows_stay_out_of_their_band() {
        let mut sh = sheet();
        sh.freeze = Some((10, 0));
        // Rows 20:22 (summary row 23) scrolled under the ten frozen rows; rows 5:8 are frozen.
        for r in 19..=21 {
            sh.rows.entry(r).or_default().outline = 1;
        }
        let g = Geo::new(&sh, Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 700.0)), vec2(0.0, 14.0 * sh.default_row_height));
        let l = layout(&g, &sh);
        let split = g.cells.top() + g.frozen_h;
        let low = |grp: &OutlineGroup| grp.start == 19;
        assert!(!l.buttons.iter().any(|b| matches!(b.action, Action::Toggle { group, .. } if low(&group))));
        for b in l.buttons.iter().filter(|b| matches!(b.action, Action::Toggle { rows: true, group } if group.start >= 10)) {
            assert!(b.rect.center().y >= split);
        }
        assert!(l.buttons.iter().any(|b| matches!(b.action, Action::Toggle { rows: true, group } if group.start == 4)));
    }
}
