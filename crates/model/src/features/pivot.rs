use gridcraft_core::Value;

use super::{CellRef, RangeRef};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- pivot tables

/// How a PivotTable lays out its row fields.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotLayout {
    /// All row fields in one column, inner items indented ("Row Labels" header).
    #[default]
    Compact,
    /// One column per row field; each item on its own row.
    Outline,
    /// One column per row field; inner items start on their parent's row, totals below.
    Tabular,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotSort {
    #[default]
    Asc,
    Desc,
    /// Source order (first appearance).
    None,
}

/// Grouping of a date field (values are date serials).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotDateGroup {
    #[default]
    None,
    Years,
    Quarters,
    Months,
    Days,
    /// The hour of the day ("12 AM" … "11 PM").
    Hours,
    /// The minute of the hour (":00" … ":59").
    Minutes,
    /// The second of the minute (":00" … ":59").
    Seconds,
}

/// Number grouping of a field (Group › Starting at, Ending at, By): its items are ranges `by`
/// wide from `start`; numbers below `start` or above `end` fall into a "<start" or ">end" item.
///
/// Only [`PivotNumberGroup::new`] makes one (deserializing goes through it too), so `start` and
/// `end` are finite with `start <= end`, `by` is positive and there are at most
/// [`PivotNumberGroup::MAX_RANGES`] ranges.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "NumberGroupFields")]
pub struct PivotNumberGroup {
    start: f64,
    end: f64,
    by: f64,
}

#[derive(Deserialize)]
struct NumberGroupFields {
    start: f64,
    end: f64,
    by: f64,
}

impl TryFrom<NumberGroupFields> for PivotNumberGroup {
    type Error = &'static str;
    /// Checks deserialized bounds the way [`PivotNumberGroup::new`] does.
    fn try_from(f: NumberGroupFields) -> Result<Self, Self::Error> {
        PivotNumberGroup::new(f.start, f.end, f.by).ok_or("invalid PivotTable number grouping")
    }
}

/// How many whole `by` steps fit in `span`, a quotient a hair under a whole number counting as
/// that number: 0.7 / 0.1 is 6.999999999999999 in binary floating point, and 0.7 belongs in the
/// "0.7-0.8" range.
fn steps(span: f64, by: f64) -> f64 {
    let q = span / by;
    let nearest = q.round();
    if (q - nearest).abs() <= 1e-9 * nearest.abs().max(1.0) { nearest } else { q.floor() }
}

/// The item a number falls in under a [`PivotNumberGroup`], in item order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NumberBucket {
    Below,
    /// The `n`th range, from 0.
    Range(u32),
    Above,
}

impl PivotNumberGroup {
    /// Most ranges one grouping makes (Excel's limit on a field's items).
    pub const MAX_RANGES: u32 = 1_048_576;

    /// `None` unless `start <= end` are finite and `by` is positive with at most
    /// [`Self::MAX_RANGES`] ranges between them.
    pub fn new(start: f64, end: f64, by: f64) -> Option<Self> {
        let ok = start.is_finite() && end.is_finite() && by.is_finite() && by > 0.0 && start <= end;
        let group = PivotNumberGroup { start, end, by };
        (ok && steps(end - start, by) < f64::from(Self::MAX_RANGES)).then_some(group)
    }

    /// Where the first range starts.
    pub fn start(&self) -> f64 {
        self.start
    }

    /// The largest number in a range; larger ones fall into the ">end" item.
    pub fn end(&self) -> f64 {
        self.end
    }

    /// The width of each range.
    pub fn by(&self) -> f64 {
        self.by
    }

    /// How many ranges lie between `start` and `end` (the last one holds `end`).
    pub fn ranges(&self) -> u32 {
        (steps(self.end - self.start, self.by) as u32).saturating_add(1).min(Self::MAX_RANGES)
    }

    /// The item `n` falls in.
    pub fn bucket(&self, n: f64) -> NumberBucket {
        if n < self.start {
            NumberBucket::Below
        } else if n > self.end {
            NumberBucket::Above
        } else {
            // `end` itself stays in the last range.
            NumberBucket::Range((steps(n - self.start, self.by) as u32).min(self.ranges() - 1))
        }
    }

    /// The caption of an item: "<0", "0-9" (whole-number ranges name their last whole number,
    /// as Excel does), "0-0.5", ">100".
    pub fn label(&self, bucket: NumberBucket) -> String {
        let text = |n: f64| gridcraft_core::number_to_text(n);
        match bucket {
            NumberBucket::Below => format!("<{}", text(self.start)),
            NumberBucket::Above => format!(">{}", text(self.end)),
            NumberBucket::Range(k) => {
                let low = self.start + f64::from(k) * self.by;
                let whole = self.start.fract() == 0.0 && self.by.fract() == 0.0;
                match (whole, self.by == 1.0) {
                    (true, true) => text(low),
                    (true, false) => format!("{}-{}", text(low), text(low + self.by - 1.0)),
                    (false, _) => format!("{}-{}", text(low), text(low + self.by)),
                }
            }
        }
    }

    /// Every item caption in order, from "<start" to ">end".
    pub fn labels(&self) -> Vec<String> {
        let mut out = vec![self.label(NumberBucket::Below)];
        out.extend((0..self.ranges()).map(|k| self.label(NumberBucket::Range(k))));
        out.push(self.label(NumberBucket::Above));
        out
    }
}

/// A field placed in the Rows or Columns area.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PivotField {
    /// Source column header name.
    pub source_col: String,
    pub sort: PivotSort,
    pub date_group: PivotDateGroup,
    /// Number grouping; when set it applies instead of `date_group` (setting one clears the other).
    pub number_group: Option<PivotNumberGroup>,
    /// Item labels whose detail is hidden (collapsed).
    pub collapsed_items: Vec<String>,
}

/// Summarize-values-by function.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotFunc {
    #[default]
    Sum,
    Count,
    Average,
    Max,
    Min,
    Product,
    CountNumbers,
    StdDev,
    StdDevP,
    Var,
    VarP,
}

/// Show-values-as transform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotShowAs {
    #[default]
    Normal,
    PercentOfGrandTotal,
    PercentOfColumnTotal,
    PercentOfRowTotal,
    /// Running total in the first row field.
    RunningTotal,
    /// Rank largest to smallest among sibling row items.
    Rank,
}

/// A field in the Values area.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PivotValue {
    pub source_col: String,
    pub func: PivotFunc,
    /// Caption, e.g. "Sum of Sales".
    pub name: String,
    pub show_as: PivotShowAs,
    pub number_format: Option<String>,
}

/// A report filter (or, for a field also in Rows/Columns, its hidden-items filter).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PivotFilter {
    pub source_col: String,
    /// Item labels shown; `None` = all.
    pub selected: Option<Vec<String>>,
}

/// A PivotTable: its definition plus where its output was last written. The output itself is
/// static cell values and styles on the sheet, rewritten on every refresh.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PivotTable {
    pub id: u32,
    pub name: String,
    /// Source: range formula text (`Sheet1!$A$1:$E$200`), a table name or a defined name.
    pub source: String,
    /// Top-left of the output body on the pivot's sheet.
    pub anchor: CellRef,
    pub rows: Vec<PivotField>,
    pub columns: Vec<PivotField>,
    pub values: Vec<PivotValue>,
    pub filters: Vec<PivotFilter>,
    pub layout: PivotLayout,
    /// PivotTable Options › Data › Enable show details: Show Details lists a value cell's
    /// source records only while this is on.
    pub enable_show_details: bool,
    /// Grand total for each row (the "Grand Total" column), like Excel's "On for Rows".
    pub grand_totals_rows: bool,
    /// Grand total for each column (the "Grand Total" row), like Excel's "On for Columns".
    pub grand_totals_cols: bool,
    pub subtotals: bool,
    /// Subtotals at the top of each group (Compact/Outline); otherwise below it.
    pub subtotals_top: bool,
    pub style: String,
    pub show_headers: bool,
    /// Area written by the last refresh.
    pub last_range: Option<RangeRef>,
    /// Calculated fields, usable in the Values area by name.
    pub calculated_fields: Vec<PivotCalcField>,
    /// Where GETPIVOTDATA finds the report's value cells, as of the last refresh. Shared between
    /// workbook clones (so editing the pivot's sheet doesn't copy it), set on refresh and saved
    /// with the report, so a reopened unrefreshed pivot still reads the report it stored. Only a
    /// pivot loaded without one (XLSX) builds it on first use, checked against the stored report.
    pub data_index: PivotLookup,
    /// The Σ Values pseudo-field (it exists with two or more value fields) sits in Rows rather
    /// than Columns (Excel's "values on rows").
    pub values_on_rows: bool,
    /// Position of Σ Values among the row fields (`None` = after the last). Only used with
    /// `values_on_rows`: in Columns Σ Values always comes after the last column field.
    pub values_position: Option<usize>,
}

/// A calculated field: a formula over other fields (`=Sales*0.1`, `='Unit Price'*Units`). As in
/// Excel, it is evaluated on the sums of the fields it names for each report cell, not per record.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PivotCalcField {
    pub name: String,
    /// Formula text, starting with `=`.
    pub formula: String,
}

/// One value cell of a computed PivotTable report: the value field, the row and column items
/// that locate it (fewer for subtotals, none for the grand total) and where the report shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PivotDataCell {
    /// The value field's caption ("Sum of Sales").
    pub data_field: String,
    /// The value field's source column ("Sales").
    pub source_col: String,
    pub items: Vec<PivotItemRef>,
    /// Row and column of the cell, counted from the report's top-left cell.
    pub row: u32,
    pub col: u32,
}

/// Builds a PivotTable's value cells from its source (`sheet` is the pivot's sheet). The engine
/// sets it; the model can't compute reports.
pub type PivotIndexer = fn(&crate::Workbook, usize, &PivotTable) -> Result<Vec<PivotDataCell>, String>;
pub static PIVOT_INDEXER: std::sync::OnceLock<PivotIndexer> = std::sync::OnceLock::new();

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// One part of a lookup key: length-prefixed, so no label text can imitate a separator.
fn key_part(out: &mut String, s: &str) {
    out.push_str(&s.len().to_string());
    out.push(':');
    out.push_str(s);
}

/// The lookup key of a value field with field/item labels (names ignore case and outer blanks).
/// Each item is `(field, label, is_bool)`: a Boolean item and a text item with the same label
/// are different items.
pub fn pivot_key(data_field: &str, items: &[(&str, &str, bool)]) -> String {
    let mut parts: Vec<String> = items
        .iter()
        .map(|(f, l, b)| {
            let mut p = String::new();
            key_part(&mut p, &norm(f));
            key_part(&mut p, &norm(l));
            p.push(if *b { 'b' } else { 't' });
            p
        })
        .collect();
    parts.sort_unstable();
    let mut key = String::new();
    key_part(&mut key, &norm(data_field));
    key.push_str(&parts.len().to_string());
    key.push('|');
    for p in parts {
        key.push_str(&p);
    }
    key
}

#[derive(Default)]
struct LookupInner {
    cells: std::sync::OnceLock<Vec<PivotDataCell>>,
    keys: std::sync::OnceLock<std::collections::HashMap<String, usize>>,
}

/// A PivotTable's GETPIVOTDATA index. Clones share it, so `Workbook` clones per command stay
/// cheap however large the report is. The cells are set on refresh or on first use; the hash of
/// them is built on the first lookup.
#[derive(Clone, Default)]
pub struct PivotLookup(std::sync::Arc<LookupInner>);

impl PivotLookup {
    /// The value cells, if they were set.
    pub fn cells(&self) -> Option<&[PivotDataCell]> {
        self.0.cells.get().map(Vec::as_slice)
    }

    /// Sets the value cells (the first set wins) and returns them.
    pub fn set_cells(&self, cells: Vec<PivotDataCell>) -> &[PivotDataCell] {
        self.0.cells.get_or_init(|| cells)
    }

    /// The cell with this value field and exactly these items, by hash.
    pub fn find(&self, data_field: &str, items: &[(&str, &str, bool)]) -> Option<&PivotDataCell> {
        let cells = self.0.cells.get()?;
        let keys = self.0.keys.get_or_init(|| {
            let mut m = std::collections::HashMap::with_capacity(cells.len());
            for (i, c) in cells.iter().enumerate() {
                let items: Vec<(&str, &str, bool)> =
                    c.items.iter().map(|it| (it.field.as_str(), it.label.as_str(), matches!(it.value, Value::Bool(_)))).collect();
                m.entry(pivot_key(&c.data_field, &items)).or_insert(i);
                m.entry(pivot_key(&c.source_col, &items)).or_insert(i);
            }
            m
        });
        keys.get(&pivot_key(data_field, items)).and_then(|i| cells.get(*i))
    }
}

impl std::fmt::Debug for PivotLookup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PivotLookup({} cells)", self.cells().map_or(0, <[PivotDataCell]>::len))
    }
}

/// Saved as the list of value cells (or nothing before the first refresh); the hash is rebuilt.
/// An empty list is never saved: it is no index, and restoring one would lock the shared lookup
/// against the lazy rebuild that a reopened workbook needs.
impl Serialize for PivotLookup {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.cells().filter(|cells| !cells.is_empty()).serialize(s)
    }
}

impl<'de> Deserialize<'de> for PivotLookup {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let l = PivotLookup::default();
        if let Some(cells) = Option::<Vec<PivotDataCell>>::deserialize(d)?
            && !cells.is_empty()
        {
            l.set_cells(cells);
        }
        Ok(l)
    }
}

/// The index is a cache, not part of a PivotTable's identity.
impl PartialEq for PivotLookup {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// A row or column field item: the field's source column, the item's label and its value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PivotItemRef {
    pub field: String,
    pub label: String,
    pub value: Value,
}

impl Default for PivotTable {
    fn default() -> Self {
        PivotTable {
            id: 0,
            name: "PivotTable1".into(),
            source: String::new(),
            anchor: CellRef::default(),
            rows: vec![],
            columns: vec![],
            values: vec![],
            filters: vec![],
            layout: PivotLayout::Compact,
            enable_show_details: true,
            grand_totals_rows: true,
            grand_totals_cols: true,
            subtotals: true,
            subtotals_top: true,
            style: "PivotStyleLight16".into(),
            show_headers: true,
            last_range: None,
            calculated_fields: vec![],
            data_index: PivotLookup::default(),
            values_on_rows: false,
            values_position: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_groups_refuse_what_would_misbehave() {
        assert!(PivotNumberGroup::new(0.0, 100.0, 10.0).is_some());
        assert!(PivotNumberGroup::new(5.0, 5.0, 1.0).is_some(), "one range");
        for (start, end, by) in [
            (0.0, 100.0, 0.0),
            (0.0, 100.0, -1.0),
            (10.0, 0.0, 1.0),
            (f64::NAN, 1.0, 1.0),
            (0.0, f64::INFINITY, 1.0),
            (0.0, 1.0, f64::NAN),
            (-1e308, 1e308, 1.0),
            (0.0, 2e6, 1.0),
        ] {
            assert_eq!(PivotNumberGroup::new(start, end, by), None, "{start} {end} {by}");
        }
        let bad: Result<PivotNumberGroup, _> = serde_json::from_str(r#"{"start":0,"end":10,"by":0}"#);
        assert!(bad.is_err(), "deserializing checks too");
        let good: PivotNumberGroup = serde_json::from_str(r#"{"start":0,"end":10,"by":5}"#).unwrap();
        assert_eq!(good, PivotNumberGroup::new(0.0, 10.0, 5.0).unwrap());
        assert_eq!(serde_json::to_string(&good).unwrap(), r#"{"start":0.0,"end":10.0,"by":5.0}"#);
    }

    #[test]
    fn numbers_fall_into_ranges_with_the_end_in_the_last() {
        let g = PivotNumberGroup::new(0.0, 100.0, 10.0).unwrap();
        assert_eq!(g.ranges(), 11, "0-9 … 90-99 and 100-109 for 100 itself");
        assert_eq!(g.bucket(-0.5), NumberBucket::Below);
        assert_eq!(g.bucket(0.0), NumberBucket::Range(0));
        assert_eq!(g.bucket(9.99), NumberBucket::Range(0));
        assert_eq!(g.bucket(10.0), NumberBucket::Range(1));
        assert_eq!(g.bucket(100.0), NumberBucket::Range(10));
        assert_eq!(g.bucket(100.5), NumberBucket::Above);
        let uneven = PivotNumberGroup::new(0.0, 25.0, 10.0).unwrap();
        assert_eq!(uneven.ranges(), 3);
        assert_eq!(uneven.bucket(25.0), NumberBucket::Range(2));
    }

    #[test]
    fn range_captions_read_like_excels() {
        let g = PivotNumberGroup::new(0.0, 30.0, 10.0).unwrap();
        assert_eq!(g.labels(), ["<0", "0-9", "10-19", "20-29", "30-39", ">30"]);
        let ones = PivotNumberGroup::new(1.0, 3.0, 1.0).unwrap();
        assert_eq!(ones.labels(), ["<1", "1", "2", "3", ">3"]);
        // Fractions: no float noise in the captions (0.1 × 3 is 0.30000000000000004), and a number
        // on a boundary starts the next range even when the division comes out a hair short.
        let tenths = PivotNumberGroup::new(0.0, 1.0, 0.1).unwrap();
        assert_eq!(tenths.ranges(), 11);
        assert_eq!(tenths.label(NumberBucket::Range(3)), "0.3-0.4");
        assert_eq!(tenths.bucket(0.7), NumberBucket::Range(7));
        assert_eq!(tenths.bucket(0.3), NumberBucket::Range(3));
        assert_eq!(tenths.bucket(0.6999), NumberBucket::Range(6));
        let negative = PivotNumberGroup::new(-20.0, 0.0, 10.0).unwrap();
        assert_eq!(negative.label(NumberBucket::Range(0)), "-20--11");
    }
}

#[cfg(test)]
mod pivot_lookup_tests {
    use super::*;

    fn cell(field: &str, label: &str, value: Value, row: u32) -> PivotDataCell {
        PivotDataCell {
            data_field: "Sum of Sales".into(),
            source_col: "Sales".into(),
            items: vec![PivotItemRef { field: field.into(), label: label.into(), value }],
            row,
            col: 1,
        }
    }

    #[test]
    fn labels_with_separator_characters_do_not_collide() {
        let l = PivotLookup::default();
        // Two different (field, label) pairs that a separator-joined key would make identical.
        l.set_cells(vec![cell("A\u{2}B", "C", Value::Text("C".into()), 1), cell("A", "B\u{2}C", Value::Text("B\u{2}C".into()), 2)]);
        assert_eq!(l.find("Sales", &[("A\u{2}B", "C", false)]).unwrap().row, 1);
        assert_eq!(l.find("Sales", &[("A", "B\u{2}C", false)]).unwrap().row, 2);
        // A label carrying the old pair separator cannot stand for two items.
        assert!(l.find("Sales", &[("A", "B", false), ("C", "D", false)]).is_none());
    }

    #[test]
    fn boolean_and_text_items_with_one_label_are_different_items() {
        let l = PivotLookup::default();
        l.set_cells(vec![cell("Flag", "TRUE", Value::Text("TRUE".into()), 1), cell("Flag", "TRUE", Value::Bool(true), 2)]);
        assert_eq!(l.find("Sales", &[("Flag", "TRUE", true)]).unwrap().row, 2);
        assert_eq!(l.find("Sales", &[("Flag", "TRUE", false)]).unwrap().row, 1);
    }

    #[test]
    fn index_round_trips_through_serde() {
        let l = PivotLookup::default();
        l.set_cells(vec![cell("Region", "East", Value::Text("East".into()), 1)]);
        let back: PivotLookup = serde_json::from_str(&serde_json::to_string(&l).unwrap()).unwrap();
        assert_eq!(back.find("Sales", &[("Region", "East", false)]).unwrap().row, 1);
        let none: PivotLookup = serde_json::from_str(&serde_json::to_string(&PivotLookup::default()).unwrap()).unwrap();
        assert!(none.cells().is_none());
    }

    #[test]
    fn an_empty_lookup_is_saved_as_no_index() {
        // An empty cell list is no index, so it must not be saved as an empty one: restoring an
        // empty index counts as an initialized lookup, which blocks the lazy rebuild.
        let l = PivotLookup::default();
        l.set_cells(vec![]);
        let saved = serde_json::to_string(&l).unwrap();
        assert_eq!(saved, "null", "an empty lookup saves no index: {saved}");
        let back: PivotLookup = serde_json::from_str("[]").unwrap();
        assert!(back.cells().is_none(), "an empty saved list restores no index");
    }
}
