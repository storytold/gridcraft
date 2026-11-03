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

/// An item of a field on the other axis that a sort by value takes its values from (e.g. the
/// "Apples" column). The field is named by source column and date grouping, so two levels
/// grouped from one date column (Years and Quarters) stay apart.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PivotScopeItem {
    pub source_col: String,
    pub date_group: PivotDateGroup,
    /// The item's label.
    pub item: String,
}

impl PivotScopeItem {
    /// Whether this names an item of `f`.
    pub fn is_of(&self, f: &PivotField) -> bool {
        self.date_group == f.date_group && self.source_col.eq_ignore_ascii_case(&f.source_col)
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
    /// Custom name shown in the field's header (Field Settings > Custom Name); `None` = the
    /// source column name.
    pub caption: Option<String>,
    /// Sort the items by this value field (its caption) instead of by their labels, in the
    /// `sort` direction (Sort > More Sort Options > by value). Ignored when `sort` is `None`.
    pub sort_by: Option<String>,
    /// Where `sort_by` takes its values: an item of each field on the other axis, outer field
    /// first, up to the first field it doesn't name (e.g. sort the rows by the "Apples" column).
    /// Empty, or naming an item that isn't there, = the grand total.
    pub sort_scope: Vec<PivotScopeItem>,
    /// Manual item order (item labels), used when `sort` is `None`; unlisted items follow in
    /// source order (date groups in date order).
    pub item_order: Vec<String>,
    /// Renamed items: (item label, caption shown instead).
    pub item_captions: Vec<(String, String)>,
    /// Subtotals of this field; `None` = the table's `subtotals`.
    pub subtotal: Option<bool>,
    /// Repeat the item labels on every row (Outline/Tabular, Repeat Item Labels).
    pub repeat_labels: bool,
    /// Number format of the item labels (Field Settings > Number Format); `None` = the source
    /// column's format.
    pub number_format: Option<String>,
}

impl PivotField {
    /// The header text: the custom name, else the source column name.
    pub fn header(&self) -> &str {
        self.caption.as_deref().unwrap_or(&self.source_col)
    }
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
    /// Header of the row labels in Compact layout; `None` = "Row Labels".
    pub row_header_caption: Option<String>,
    /// Header of the column labels in Compact layout; `None` = "Column Labels".
    pub col_header_caption: Option<String>,
    /// Caption of the Values field; `None` = "Values".
    pub data_caption: Option<String>,
    /// Caption of the grand totals; `None` = "Grand Total".
    pub grand_total_caption: Option<String>,
    /// Shown in value cells that summarize no records (For empty cells show); `None` = blank.
    pub empty_caption: Option<String>,
    /// Show the Values row: a header row with `data_caption` above the value captions
    /// (Outline/Tabular, several values and no column field). Excel's default is on.
    pub show_values_row: bool,
    /// Classic PivotTable layout (fields dragged in the grid): with one value field and no
    /// column field, the value caption sits above the row headers and the column reads "Total".
    pub classic_layout: bool,
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
            row_header_caption: None,
            col_header_caption: None,
            data_caption: None,
            grand_total_caption: None,
            empty_caption: None,
            show_values_row: true,
            classic_layout: false,
            calculated_fields: vec![],
            values_on_rows: false,
            values_position: None,
        }
    }
}
