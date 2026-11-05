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
///
/// The modes marked *base field* measure a cell against other items of one row or column field,
/// [`PivotValue::base_field`] (the first row field, else the first column field, when unset).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotShowAs {
    #[default]
    Normal,
    PercentOfGrandTotal,
    PercentOfColumnTotal,
    PercentOfRowTotal,
    /// The value over the subtotal of its parent row item (the grand total for an outermost item).
    PercentOfParentRowTotal,
    /// Base field: the value over the base item's value.
    PercentOf,
    /// Base field: the value minus the base item's value.
    DifferenceFrom,
    /// Base field: (value − base item's value) / base item's value.
    PercentDifferenceFrom,
    /// Base field: the sum of the value over the base field's items up to this one.
    RunningTotal,
    /// Base field: the running total over the base field's total.
    PercentRunningTotal,
    /// Base field: the rank among the base field's items, largest first (ties share a rank).
    Rank,
    /// Base field: the rank among the base field's items, smallest first.
    RankAscending,
    /// (value × grand total) / (row total × column total).
    Index,
}

impl PivotShowAs {
    /// Every mode, in the order Excel's Show Values As menu lists them.
    pub const ALL: [PivotShowAs; 13] = [
        Self::Normal,
        Self::PercentOfGrandTotal,
        Self::PercentOfColumnTotal,
        Self::PercentOfRowTotal,
        Self::PercentOf,
        Self::PercentOfParentRowTotal,
        Self::DifferenceFrom,
        Self::PercentDifferenceFrom,
        Self::RunningTotal,
        Self::PercentRunningTotal,
        Self::RankAscending,
        Self::Rank,
        Self::Index,
    ];

    /// Whether the mode reads [`PivotValue::base_field`].
    pub fn uses_base_field(self) -> bool {
        matches!(
            self,
            Self::PercentOf
                | Self::DifferenceFrom
                | Self::PercentDifferenceFrom
                | Self::RunningTotal
                | Self::PercentRunningTotal
                | Self::Rank
                | Self::RankAscending
        )
    }

    /// Whether the mode reads [`PivotValue::base_item`].
    pub fn uses_base_item(self) -> bool {
        matches!(self, Self::PercentOf | Self::DifferenceFrom | Self::PercentDifferenceFrom)
    }

    /// Whether the mode shows a ratio (formatted as a percentage unless the field has a format).
    pub fn is_percentage(self) -> bool {
        matches!(
            self,
            Self::PercentOfGrandTotal
                | Self::PercentOfColumnTotal
                | Self::PercentOfRowTotal
                | Self::PercentOfParentRowTotal
                | Self::PercentOf
                | Self::PercentDifferenceFrom
                | Self::PercentRunningTotal
        )
    }
}

/// The item of the base field a value is measured against (% Of, Difference From,
/// % Difference From).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotBaseItem {
    /// The item before the cell's own item ("(previous)"); the first item has no base.
    #[default]
    Previous,
    /// The item after the cell's own item ("(next)"); the last item has no base.
    Next,
    /// The item with this label (compared ignoring ASCII case).
    Named(String),
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
    /// The row or column field a base-field mode measures against (see [`PivotShowAs`]).
    pub base_field: Option<String>,
    /// The base field's item % Of and the Difference From modes measure against.
    pub base_item: PivotBaseItem,
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
