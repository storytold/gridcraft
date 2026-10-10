//! Worksheets.

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use serde::{Deserialize, Serialize};

use crate::cell::{Cell, CellPicture};
use crate::features::*;
use crate::store::CellStore;
use crate::style::{Color, StyleId};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    #[default]
    Visible,
    Hidden,
    VeryHidden,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    pub cells: CellStore,
    /// Static pictures are sparse rich content. Ordinary cells carry no picture pointer.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "picture_map")]
    pub cell_pictures: BTreeMap<CellRef, Arc<CellPicture>>,
    /// Values spilled by dynamic-array formulas (maintained by calc), keyed by cell.
    #[serde(skip)]
    pub spill: BTreeMap<CellRef, Value>,
    /// Anchor → spill range.
    #[serde(skip)]
    pub spill_ranges: BTreeMap<CellRef, RangeRef>,
    pub cols: BTreeMap<u32, LineInfo>,
    pub rows: BTreeMap<u32, LineInfo>,
    /// Default column width and row height in points.
    pub default_col_width: f32,
    pub default_row_height: f32,
    pub merges: Vec<RangeRef>,
    /// Frozen rows and columns (top/left).
    pub freeze: Option<(u32, u32)>,
    pub tab_color: Option<Color>,
    pub visibility: Visibility,
    pub show_gridlines: bool,
    pub show_headings: bool,
    pub show_formulas: bool,
    pub show_zeros: bool,
    pub right_to_left: bool,
    pub zoom: u16,
    pub cond_formats: Vec<CondFormat>,
    pub validations: Vec<Validation>,
    pub tables: Vec<Table>,
    pub autofilter: Option<AutoFilter>,
    #[serde(with = "crate::cellmap")]
    pub comments: BTreeMap<CellRef, Comment>,
    #[serde(with = "crate::cellmap")]
    pub hyperlinks: BTreeMap<CellRef, Hyperlink>,
    pub charts: Vec<Chart>,
    pub images: Vec<Image>,
    pub shapes: Vec<Shape>,
    pub sparklines: Vec<Sparkline>,
    /// PivotTables whose output lives on this sheet.
    #[serde(default)]
    pub pivots: Vec<PivotTable>,
    pub protection: Option<SheetProtection>,
    pub print: PrintSettings,
    /// Last selection and scroll position, saved with the file.
    pub view_active: CellRef,
    pub view_top_left: CellRef,
}

/// Default width of a column (points) and height of a row (points) for the default font.
pub const DEFAULT_COL_WIDTH: f32 = 64.0;
pub const DEFAULT_ROW_HEIGHT: f32 = 20.0;

impl Sheet {
    pub fn new(name: impl Into<String>) -> Sheet {
        Sheet {
            name: name.into(),
            cells: CellStore::new(),
            cell_pictures: BTreeMap::new(),
            spill: BTreeMap::new(),
            spill_ranges: BTreeMap::new(),
            cols: BTreeMap::new(),
            rows: BTreeMap::new(),
            default_col_width: DEFAULT_COL_WIDTH,
            default_row_height: DEFAULT_ROW_HEIGHT,
            merges: vec![],
            freeze: None,
            tab_color: None,
            visibility: Visibility::Visible,
            show_gridlines: true,
            show_headings: true,
            show_formulas: false,
            show_zeros: true,
            right_to_left: false,
            zoom: 100,
            cond_formats: vec![],
            validations: vec![],
            tables: vec![],
            autofilter: None,
            comments: BTreeMap::new(),
            hyperlinks: BTreeMap::new(),
            charts: vec![],
            images: vec![],
            shapes: vec![],
            sparklines: vec![],
            pivots: vec![],
            protection: None,
            print: PrintSettings::default(),
            view_active: CellRef::default(),
            view_top_left: CellRef::default(),
        }
    }

    pub fn cell(&self, c: CellRef) -> Option<&Cell> {
        self.cells.get(c)
    }
    /// Displayed value of a cell: its own value, or a spilled value.
    pub fn value(&self, c: CellRef) -> Value {
        match self.cells.get(c) {
            Some(cell) if !cell.value.is_empty() || cell.formula.is_some() => cell.value.clone(),
            _ => self.spill.get(&c).cloned().unwrap_or(Value::Empty),
        }
    }
    pub fn value_ref(&self, c: CellRef) -> Option<&Value> {
        match self.cells.get(c) {
            Some(cell) if !cell.value.is_empty() || cell.formula.is_some() => Some(&cell.value),
            _ => self.spill.get(&c),
        }
    }
    pub fn style_id(&self, c: CellRef) -> StyleId {
        if let Some(cell) = self.cells.get(c) {
            return cell.style;
        }
        if let Some(s) = self.rows.get(&c.row).and_then(|r| r.style) {
            return s;
        }
        self.cols.get(&c.col).and_then(|r| r.style).unwrap_or_default()
    }
    pub fn set_value(&mut self, c: CellRef, v: Value) {
        let style = self.style_id(c);
        self.set_cell(c, Cell { value: v, formula: None, style });
    }
    /// Replaces content, removing any picture at this address. Style-only edits use set_style.
    pub fn set_cell(&mut self, c: CellRef, cell: Cell) {
        self.cell_pictures.remove(&c);
        self.cells.set(c, cell);
    }
    pub fn remove_cell(&mut self, c: CellRef) -> Option<Cell> {
        self.cell_pictures.remove(&c);
        self.cells.remove(c)
    }
    /// Removes content in a range; returns scalar cells, with pictures removed from the side map.
    pub fn take_cells(&mut self, range: RangeRef) -> Vec<(CellRef, Cell)> {
        self.cell_pictures.retain(|c, _| !range.contains(*c));
        self.cells.take_range(range)
    }
    /// Pictures retain a scalar fallback for calculation and unsupported consumers.
    pub fn set_picture(&mut self, c: CellRef, picture: Arc<CellPicture>) {
        let style = self.style_id(c);
        self.cells.set(c, Cell { value: Value::Error(gridcraft_core::CellError::Value), formula: None, style });
        self.cell_pictures.insert(c, picture);
    }
    pub fn input_text(&self, c: CellRef) -> String {
        if self.cell_pictures.contains_key(&c) { String::new() } else { self.cell(c).map(Cell::input_text).unwrap_or_default() }
    }
    pub fn set_style(&mut self, c: CellRef, style: StyleId) {
        let mut cell = self.cells.get(c).cloned().unwrap_or_default();
        cell.style = style;
        self.cells.set(c, cell);
    }

    // ---- geometry

    pub fn col_width(&self, col: u32) -> f32 {
        match self.cols.get(&col) {
            Some(i) if i.hidden => 0.0,
            Some(LineInfo { size: Some(w), .. }) => *w,
            _ => self.default_col_width,
        }
    }
    pub fn row_height(&self, row: u32) -> f32 {
        match self.rows.get(&row) {
            Some(i) if i.hidden => 0.0,
            Some(LineInfo { size: Some(h), .. }) => *h,
            _ => self.default_row_height,
        }
    }
    pub fn is_row_hidden(&self, row: u32) -> bool {
        self.rows.get(&row).is_some_and(|r| r.hidden)
    }
    pub fn is_col_hidden(&self, col: u32) -> bool {
        self.cols.get(&col).is_some_and(|r| r.hidden)
    }
    /// Y offset (points) of the top of `row`.
    pub fn row_top(&self, row: u32) -> f64 {
        let mut y = row as f64 * self.default_row_height as f64;
        for (r, info) in self.rows.range(..row) {
            let _ = r;
            let h = if info.hidden { 0.0 } else { info.size.unwrap_or(self.default_row_height) };
            y += h as f64 - self.default_row_height as f64;
        }
        y
    }
    pub fn col_left(&self, col: u32) -> f64 {
        let mut x = col as f64 * self.default_col_width as f64;
        for (_, info) in self.cols.range(..col) {
            let w = if info.hidden { 0.0 } else { info.size.unwrap_or(self.default_col_width) };
            x += w as f64 - self.default_col_width as f64;
        }
        x
    }
    /// Row containing y (points from the top), clamped.
    pub fn row_at(&self, y: f64) -> u32 {
        line_at(y, self.default_row_height as f64, &self.rows, gridcraft_core::MAX_ROWS)
    }
    pub fn col_at(&self, x: f64) -> u32 {
        line_at(x, self.default_col_width as f64, &self.cols, gridcraft_core::MAX_COLS)
    }

    /// The merged range containing `c`, if any.
    pub fn merge_at(&self, c: CellRef) -> Option<RangeRef> {
        self.merges.iter().find(|m| m.contains(c)).copied()
    }
    pub fn table_at(&self, c: CellRef) -> Option<&Table> {
        self.tables.iter().find(|t| t.range.contains(c))
    }
    pub fn used_range(&self) -> Option<RangeRef> {
        let mut r = self.cells.used_range();
        for sr in self.spill_ranges.values() {
            r = Some(r.map_or(*sr, |x| x.union(sr)));
        }
        r
    }
    pub fn is_protected(&self) -> bool {
        self.protection.is_some()
    }
}

/// Prefix sums over the custom-sized lines of an axis: O(log n) position ↔ line lookups even
/// when every row has an explicit height (common in imported files).
#[derive(Clone, Debug, Default)]
pub struct LineIndex {
    default: f64,
    max: u32,
    /// (line, start position of that line, its size), sorted by line.
    lines: Vec<(u32, f64, f64)>,
}

impl LineIndex {
    pub fn new(custom: &BTreeMap<u32, LineInfo>, default: f32, max: u32) -> LineIndex {
        let default = default as f64;
        let mut lines = Vec::with_capacity(custom.len());
        let mut pos = 0.0;
        let mut prev = 0u32;
        for (&line, info) in custom {
            pos += (line - prev) as f64 * default;
            let size = if info.hidden { 0.0 } else { info.size.map_or(default, |s| s as f64) };
            lines.push((line, pos, size));
            pos += size;
            prev = line + 1;
        }
        LineIndex { default, max, lines }
    }
    /// Start position of `line`.
    pub fn start(&self, line: u32) -> f64 {
        match self.lines.binary_search_by(|e| e.0.cmp(&line)) {
            Ok(i) => self.lines.get(i).map_or(0.0, |e| e.1),
            Err(0) => line as f64 * self.default,
            Err(i) => match self.lines.get(i - 1) {
                Some(&(l, p, s)) => p + s + (line - l - 1) as f64 * self.default,
                None => line as f64 * self.default,
            },
        }
    }
    /// The line containing `pos`.
    pub fn at(&self, pos: f64) -> u32 {
        if pos <= 0.0 {
            return 0;
        }
        // Last custom line starting at or before pos.
        let i = self.lines.partition_point(|e| e.1 <= pos);
        let (base_line, base_pos) = match i.checked_sub(1).and_then(|k| self.lines.get(k)) {
            Some(&(l, p, s)) => {
                if pos < p + s {
                    return l.min(self.max - 1);
                }
                (l + 1, p + s)
            }
            None => (0, 0.0),
        };
        if self.default <= 0.0 {
            return base_line.min(self.max - 1);
        }
        let k = ((pos - base_pos) / self.default).floor();
        let r = base_line as f64 + k;
        if r >= self.max as f64 { self.max - 1 } else { r as u32 }
    }
}

/// Inverse of the cumulative size along an axis with sparse custom sizes.
fn line_at(pos: f64, default: f64, custom: &BTreeMap<u32, LineInfo>, max: u32) -> u32 {
    if pos <= 0.0 {
        return 0;
    }
    let mut start_pos = 0.0; // position of line `idx`
    let mut idx: u32 = 0;
    for (&line, info) in custom.iter() {
        // Lines idx..line have default size.
        let span = (line - idx) as f64 * default;
        if pos < start_pos + span {
            let k = ((pos - start_pos) / default).floor() as u32;
            return (idx + k).min(max - 1);
        }
        start_pos += span;
        let size = if info.hidden { 0.0 } else { info.size.map_or(default, |s| s as f64) };
        if pos < start_pos + size {
            return line.min(max - 1);
        }
        start_pos += size;
        idx = line + 1;
    }
    if default <= 0.0 {
        return idx.min(max - 1);
    }
    let k = ((pos - start_pos) / default).floor();
    let r = idx as f64 + k;
    if r >= max as f64 { max - 1 } else { r as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_index_matches_linear() {
        let mut s = Sheet::new("S");
        for r in (0..5000).step_by(3) {
            s.rows.insert(r, LineInfo { size: Some(10.0 + (r % 7) as f32), hidden: r % 11 == 0, ..Default::default() });
        }
        let idx = LineIndex::new(&s.rows, s.default_row_height, gridcraft_core::MAX_ROWS);
        for r in [0u32, 1, 2, 3, 4, 100, 999, 4998, 5000, 6000] {
            assert!((idx.start(r) - s.row_top(r)).abs() < 1e-6, "start {r}");
        }
        for y in [0.0, 5.0, 33.3, 1000.0, 47_000.0, 90_000.0] {
            assert_eq!(idx.at(y), s.row_at(y), "at {y}");
        }
    }

    #[test]
    fn geometry() {
        let mut s = Sheet::new("S");
        assert_eq!(s.row_top(3), 60.0);
        s.rows.insert(1, LineInfo { size: Some(40.0), ..Default::default() });
        s.rows.insert(2, LineInfo { hidden: true, ..Default::default() });
        assert_eq!(s.row_top(1), 20.0);
        assert_eq!(s.row_top(2), 60.0);
        assert_eq!(s.row_top(3), 60.0);
        assert_eq!(s.row_top(4), 80.0);
        assert_eq!(s.row_at(10.0), 0);
        assert_eq!(s.row_at(30.0), 1);
        assert_eq!(s.row_at(59.0), 1);
        assert_eq!(s.row_at(61.0), 3);
        assert_eq!(s.row_at(85.0), 4);
        assert_eq!(s.col_at(1e12), gridcraft_core::MAX_COLS - 1);
        assert_eq!(s.col_at(-5.0), 0);
        assert_eq!(s.col_left(2), 128.0);
    }
}

// CellRef is a struct, so encode the sparse map as entries for JSON as well as other formats.
mod picture_map {
    use super::*;
    use serde::{Deserializer, Serializer};
    pub fn serialize<S: Serializer>(map: &BTreeMap<CellRef, Arc<CellPicture>>, serializer: S) -> Result<S::Ok, S::Error> {
        map.iter().collect::<Vec<_>>().serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<CellRef, Arc<CellPicture>>, D::Error> {
        Ok(Vec::<(CellRef, Arc<CellPicture>)>::deserialize(deserializer)?.into_iter().collect())
    }
}
