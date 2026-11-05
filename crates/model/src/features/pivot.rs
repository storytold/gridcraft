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
