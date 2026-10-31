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
}

impl PivotDateGroup {
    /// Finest first: days, months, quarters, years, then ungrouped.
    pub fn coarseness(self) -> u8 {
        match self {
            PivotDateGroup::Days => 0,
            PivotDateGroup::Months => 1,
            PivotDateGroup::Quarters => 2,
            PivotDateGroup::Years => 3,
            PivotDateGroup::None => 4,
        }
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
    /// For a field outside Rows and Columns: it filters the PivotTable without being a report
    /// filter (no filter cell above the table). Set by a slicer on a field the PivotTable doesn't
    /// show, and read from files as hidden items of such a field.
    pub hidden: bool,
    /// The date grouping `selected` is in, when it isn't the first Rows/Columns field's on the
    /// column (`None`): a field in no area keeps its cache field's grouping here (ECMA-376 puts
    /// `fieldGroup` on the cache field), and on a field grouped at several levels it names the
    /// level filtered. See [`PivotTable::filter_group`].
    pub date_group: PivotDateGroup,
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

impl PivotTable {
    fn axis(&self) -> impl Iterator<Item = &PivotField> {
        self.rows.iter().chain(self.columns.iter())
    }

    /// The grouping a filter on `col` gets when it names none: the first Rows/Columns field's.
    pub fn implied_group(&self, col: &str) -> PivotDateGroup {
        self.axis().find(|f| f.source_col.eq_ignore_ascii_case(col)).map(|f| f.date_group).unwrap_or_default()
    }

    /// The date grouping a filter's labels are in.
    pub fn filter_group(&self, f: &PivotFilter) -> PivotDateGroup {
        if f.date_group == PivotDateGroup::None { self.implied_group(&f.source_col) } else { f.date_group }
    }

    /// Clears each filter's `date_group` where the first Rows/Columns field implies it, so one
    /// filter has one representation.
    pub fn normalize_filter_groups(&mut self) {
        let implied: Vec<PivotDateGroup> = self.filters.iter().map(|f| self.implied_group(&f.source_col)).collect();
        for (f, g) in self.filters.iter_mut().zip(implied) {
            if f.date_group == g {
                f.date_group = PivotDateGroup::None;
            }
        }
    }

    /// The grouping of `col`'s base cache field: the finest of its Rows/Columns fields' and its
    /// filter's, as a cache groups the base field by the finest level. A slicer on `col` lists
    /// these items.
    pub fn field_group(&self, col: &str) -> PivotDateGroup {
        let filter = self.filters.iter().filter(|f| f.source_col.eq_ignore_ascii_case(col)).map(|f| self.filter_group(f));
        let axis = self.axis().filter(|f| f.source_col.eq_ignore_ascii_case(col)).map(|f| f.date_group);
        axis.chain(filter).min_by_key(|g| g.coarseness()).unwrap_or_default()
    }

    /// Sets the filter on `col` to `selected` (`None`: everything) with its labels in `group`,
    /// adding a hidden filter for a field in no area. Clearing a filter removes it, except a
    /// report filter and a filter that holds the field's grouping (none of its Rows/Columns
    /// fields has it), which keep `selected: None`.
    pub fn set_filter(&mut self, col: &str, selected: Option<Vec<String>>, group: PivotDateGroup) {
        let on_axis = self.axis().any(|f| f.source_col.eq_ignore_ascii_case(col));
        let date_group = if group == self.implied_group(col) { PivotDateGroup::None } else { group };
        let i = self.filters.iter().position(|f| f.source_col.eq_ignore_ascii_case(col));
        match (i, selected) {
            (Some(i), None) => {
                let Some(f) = self.filters.get(i) else { return };
                let grouping = f.date_group != PivotDateGroup::None
                    && !self.axis().any(|a| a.source_col.eq_ignore_ascii_case(col) && a.date_group == f.date_group);
                if grouping || !(on_axis || f.hidden) {
                    if let Some(f) = self.filters.get_mut(i) {
                        f.selected = None;
                    }
                } else {
                    self.filters.remove(i);
                }
            }
            (Some(i), Some(sel)) => {
                if let Some(f) = self.filters.get_mut(i) {
                    f.selected = Some(sel);
                    f.date_group = date_group;
                }
            }
            (None, Some(sel)) => self.filters.push(PivotFilter { source_col: col.to_string(), selected: Some(sel), hidden: !on_axis, date_group }),
            (None, None) => {}
        }
    }
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
