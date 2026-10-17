//! Get & Transform (Power Query) queries kept with a workbook: where the data comes from, the
//! ordered transformation steps (Applied Steps), where the result is loaded and how it refreshes.
//!
//! The model only holds definitions and the facts of the last refresh; `gridcraft-engine`
//! evaluates them. Steps are a typed list rather than M code: the engine shows an M-like text for
//! transparency, but nothing here is ever parsed or executed as a script.

use serde::{Deserialize, Serialize};

/// Most queries a workbook keeps.
pub const MAX_QUERIES: usize = 256;
/// Most applied steps one query keeps.
pub const MAX_STEPS: usize = 256;
/// Longest query or step name, in characters.
pub const MAX_NAME: usize = 80;

/// One query (Data › Queries & Connections lists them).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    /// Unique within the workbook (case-insensitive), as in Excel.
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The `Source` step.
    pub source: QuerySource,
    /// The steps after `Source`, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<QueryStep>,
    #[serde(default)]
    pub load: QueryLoad,
    #[serde(default)]
    pub refresh: RefreshOptions,
    /// What the last refresh produced.
    #[serde(default)]
    pub status: QueryStatus,
}

impl Query {
    pub fn new(name: impl Into<String>, source: QuerySource) -> Query {
        Query {
            name: name.into(),
            description: String::new(),
            source,
            steps: vec![],
            load: QueryLoad::default(),
            refresh: RefreshOptions::default(),
            status: QueryStatus::default(),
        }
    }
}

/// Where a query reads its data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum QuerySource {
    /// From Text/CSV: a delimited text file. `delimiter` `None` detects it (comma, semicolon, tab
    /// or pipe); `encoding` is the File Origin (`utf-8`, `utf-16`, `windows-1252`; `None` detects).
    Csv {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delimiter: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encoding: Option<String>,
    },
    /// Delimited text held in the query itself (what `data.getData {text}` imports, and Enter
    /// Data).
    Text {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        delimiter: Option<String>,
    },
    /// From Excel Workbook: a sheet, table or defined name of another workbook.
    Workbook {
        path: String,
        item: String,
        #[serde(default)]
        item_kind: WorkbookItemKind,
    },
    /// From XML: the repeating element `item` (a `/`-separated element path; `None` picks the
    /// first table found).
    Xml {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item: Option<String>,
    },
    /// From JSON: the array at `item` (a `.`-separated property path; `None` uses the document
    /// itself, or the first array of records in it).
    Json {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item: Option<String>,
    },
    /// From Folder: one row per file (subfolders included). With `combine`, the files' contents
    /// (CSV/text, JSON or XML, chosen by `extension` or the first file) are appended instead.
    Folder {
        path: String,
        #[serde(default)]
        combine: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extension: Option<String>,
    },
    /// From Web: an HTTP(S) GET. HTML pages offer their tables (`item` = `Table 1`, `Table 2`…);
    /// CSV, JSON and XML responses are read like the files.
    Web {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item: Option<String>,
    },
    /// From OData Feed: the JSON feed at `url`; at a service root, `item` names the entity set.
    #[serde(rename = "odata")]
    OData {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item: Option<String>,
    },
    /// From ODBC (and From SQL Server Database / From Microsoft Access Database through their ODBC
    /// drivers): a table or a SQL statement.
    Odbc {
        connection: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        table: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sql: Option<String>,
    },
    /// From Table/Range: a table or defined name of this workbook (`Excel.CurrentWorkbook()`).
    CurrentWorkbook { name: String },
    /// Another query of this workbook (Reference, and the Combine Queries results).
    Query { name: String },
    /// Blank Query: an empty table.
    Blank,
}

impl QuerySource {
    /// The file, folder, URL or connection the source reads from outside the workbook, if any.
    pub fn location(&self) -> Option<&str> {
        match self {
            QuerySource::Csv { path, .. }
            | QuerySource::Workbook { path, .. }
            | QuerySource::Xml { path, .. }
            | QuerySource::Json { path, .. }
            | QuerySource::Folder { path, .. } => Some(path),
            QuerySource::Web { url, .. } | QuerySource::OData { url, .. } => Some(url),
            QuerySource::Odbc { connection, .. } => Some(connection),
            QuerySource::Text { .. } | QuerySource::CurrentWorkbook { .. } | QuerySource::Query { .. } | QuerySource::Blank => None,
        }
    }

    /// Whether evaluating the source reaches outside the workbook (files, the network, a
    /// database): such queries stay disabled until external content is enabled.
    pub fn is_external(&self) -> bool {
        self.location().is_some()
    }
}

/// What a From Excel Workbook source picks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WorkbookItemKind {
    #[default]
    Sheet,
    Table,
    DefinedName,
}

/// One applied step: its name in Applied Steps and what it does.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct QueryStep {
    /// Shown in Applied Steps ("Promoted Headers", "Removed Columns1"). Empty = named after the
    /// operation when the step is added.
    #[serde(default)]
    pub name: String,
    #[serde(flatten)]
    pub op: StepOp,
}

/// A column's data type (the type icon in the Power Query Editor's column header).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColType {
    #[default]
    Any,
    Text,
    /// Decimal Number.
    Number,
    /// Currency (Fixed Decimal Number).
    Currency,
    /// Whole Number.
    Whole,
    Percentage,
    Date,
    DateTime,
    Time,
    /// True/False.
    Logical,
}

/// A literal in a step (filter values, replacements, conditional-column outputs). `null` is
/// written as an absent value (`Option::None`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Literal {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// A comparison used by Filter Rows and Conditional Column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FilterOp {
    #[default]
    Equals,
    NotEquals,
    BeginsWith,
    NotBeginsWith,
    EndsWith,
    NotEndsWith,
    Contains,
    NotContains,
    GreaterThan,
    GreaterOrEqual,
    LessThan,
    LessOrEqual,
    /// The value is null (blank).
    IsNull,
    IsNotNull,
}

/// One condition of a Filter Rows step.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FilterCondition {
    pub op: FilterOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Literal>,
}

/// A Sort step key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SortKey {
    pub column: String,
    #[serde(default)]
    pub descending: bool,
}

/// A column and the type Change Type gives it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnType {
    pub column: String,
    #[serde(rename = "type")]
    pub col_type: ColType,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rename {
    pub from: String,
    pub to: String,
}

/// Text transforms (Transform › Format).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextFn {
    Lower,
    Upper,
    /// Capitalize Each Word.
    Proper,
    Trim,
    Clean,
}

/// Split Column by Delimiter: where to split.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SplitMode {
    /// Each occurrence of the delimiter.
    #[default]
    Each,
    /// Left-most delimiter only.
    LeftMost,
    /// Right-most delimiter only.
    RightMost,
}

/// Group By aggregations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AggOp {
    /// Count Rows.
    #[default]
    Count,
    CountDistinct,
    Sum,
    Average,
    Median,
    Min,
    Max,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aggregation {
    /// New column name.
    pub name: String,
    #[serde(default)]
    pub op: AggOp,
    /// The column aggregated (not needed for Count Rows).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

/// Merge Queries join kinds, as the Merge dialog lists them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JoinKind {
    /// Left Outer (all from first, matching from second).
    #[default]
    LeftOuter,
    RightOuter,
    FullOuter,
    Inner,
    LeftAnti,
    RightAnti,
}

/// What a Conditional Column rule (or its Else) outputs: a value or another column's value.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Output {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Literal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<String>,
}

/// One If / Else If rule of a Conditional Column.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConditionalRule {
    pub column: String,
    #[serde(default)]
    pub op: FilterOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Literal>,
    #[serde(default)]
    pub output: Output,
}

/// The operation of an applied step. Column names are matched exactly (as in Power Query). In
/// JSON a step is flat (`{"name": "Removed Columns", "op": "removeColumns", "columns": […]}`), so
/// a new column's name is `newColumn` (`name` is the step's).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum StepOp {
    /// Use First Row as Headers.
    PromoteHeaders,
    /// Use Headers as First Row.
    DemoteHeaders,
    /// Change Type; `locale` (`en-US`, `de-DE`…) is Using Locale's culture for text it parses.
    ChangeType {
        columns: Vec<ColumnType>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        locale: Option<String>,
    },
    /// Choose Columns (Remove Other Columns): keeps these, in this order.
    ChooseColumns {
        columns: Vec<String>,
    },
    RemoveColumns {
        columns: Vec<String>,
    },
    RenameColumns {
        renames: Vec<Rename>,
    },
    /// Moves columns into this order (unlisted columns follow).
    ReorderColumns {
        columns: Vec<String>,
    },
    /// Duplicate Column: a copy of `column` named `newColumn` (empty: `Column - Copy`).
    DuplicateColumn {
        column: String,
        #[serde(rename = "newColumn", default)]
        name: String,
    },
    KeepTopRows {
        count: u64,
    },
    KeepBottomRows {
        count: u64,
    },
    /// Keep Range of Rows: `count` rows starting at row `first` (1-based).
    KeepRange {
        first: u64,
        count: u64,
    },
    RemoveTopRows {
        count: u64,
    },
    RemoveBottomRows {
        count: u64,
    },
    /// Remove Duplicates over these columns (empty: all columns).
    RemoveDuplicates {
        #[serde(default)]
        columns: Vec<String>,
    },
    /// Remove Blank Rows: rows whose every value is null or empty text.
    RemoveBlankRows,
    /// Remove Errors in these columns (empty: any column).
    RemoveErrors {
        #[serde(default)]
        columns: Vec<String>,
    },
    /// Filter Rows: keep rows whose value is in `values` (a checked list; `null` for blanks), and
    /// that meet `conditions` (all of them, or any with `or`).
    FilterRows {
        column: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        values: Option<Vec<Option<Literal>>>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        conditions: Vec<FilterCondition>,
        #[serde(default)]
        or: bool,
    },
    Sort {
        keys: Vec<SortKey>,
    },
    /// Replace Values: text inside text values (or whole values with `whole`, and always for
    /// numbers, logicals and null).
    ReplaceValues {
        columns: Vec<String>,
        #[serde(default)]
        find: Option<Literal>,
        #[serde(default)]
        replace: Option<Literal>,
        #[serde(default)]
        whole: bool,
    },
    /// Split Column › By Delimiter. `count` caps the number of new columns (Each mode).
    SplitColumn {
        column: String,
        delimiter: String,
        #[serde(default)]
        mode: SplitMode,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<u32>,
    },
    /// Format › lowercase, UPPERCASE, Capitalize Each Word, Trim, Clean.
    TransformText {
        columns: Vec<String>,
        function: TextFn,
    },
    FillDown {
        columns: Vec<String>,
    },
    FillUp {
        columns: Vec<String>,
    },
    GroupBy {
        keys: Vec<String>,
        aggregations: Vec<Aggregation>,
    },
    /// Unpivot Columns: the listed columns become Attribute/Value rows.
    Unpivot {
        columns: Vec<String>,
        #[serde(default = "attribute_name")]
        attribute: String,
        #[serde(default = "value_name")]
        value: String,
    },
    Transpose,
    /// Merge Queries, then expand the other query's `columns` (empty: all but its keys), named
    /// `prefix.column` when a prefix is given.
    MergeQueries {
        query: String,
        left: Vec<String>,
        right: Vec<String>,
        #[serde(default)]
        join: JoinKind,
        #[serde(default)]
        columns: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        prefix: Option<String>,
    },
    /// Append Queries: these queries' rows follow (columns matched by name).
    AppendQueries {
        queries: Vec<String>,
    },
    /// Add Column › Index Column.
    AddIndexColumn {
        #[serde(rename = "newColumn", default = "index_name")]
        name: String,
        #[serde(default)]
        start: i64,
        #[serde(default = "one")]
        step: i64,
    },
    /// Add Column › Conditional Column: the first rule that holds gives the value, else
    /// `otherwise`.
    AddConditionalColumn {
        #[serde(rename = "newColumn")]
        name: String,
        rules: Vec<ConditionalRule>,
        #[serde(default)]
        otherwise: Output,
    },
}

fn attribute_name() -> String {
    "Attribute".into()
}
fn value_name() -> String {
    "Value".into()
}
fn index_name() -> String {
    "Index".into()
}
fn one() -> i64 {
    1
}

impl StepOp {
    /// The step name Power Query gives a new step of this kind.
    pub fn default_name(&self) -> &'static str {
        match self {
            StepOp::PromoteHeaders => "Promoted Headers",
            StepOp::DemoteHeaders => "Demoted Headers",
            StepOp::ChangeType { locale: Some(_), .. } => "Changed Type with Locale",
            StepOp::ChangeType { .. } => "Changed Type",
            StepOp::ChooseColumns { .. } => "Removed Other Columns",
            StepOp::RemoveColumns { .. } => "Removed Columns",
            StepOp::RenameColumns { .. } => "Renamed Columns",
            StepOp::ReorderColumns { .. } => "Reordered Columns",
            StepOp::DuplicateColumn { .. } => "Duplicated Column",
            StepOp::KeepTopRows { .. } => "Kept First Rows",
            StepOp::KeepBottomRows { .. } => "Kept Last Rows",
            StepOp::KeepRange { .. } => "Kept Range of Rows",
            StepOp::RemoveTopRows { .. } => "Removed Top Rows",
            StepOp::RemoveBottomRows { .. } => "Removed Bottom Rows",
            StepOp::RemoveDuplicates { .. } => "Removed Duplicates",
            StepOp::RemoveBlankRows => "Removed Blank Rows",
            StepOp::RemoveErrors { .. } => "Removed Errors",
            StepOp::FilterRows { .. } => "Filtered Rows",
            StepOp::Sort { .. } => "Sorted Rows",
            StepOp::ReplaceValues { .. } => "Replaced Value",
            StepOp::SplitColumn { .. } => "Split Column by Delimiter",
            StepOp::TransformText { function: TextFn::Lower, .. } => "Lowercased Text",
            StepOp::TransformText { function: TextFn::Upper, .. } => "Uppercased Text",
            StepOp::TransformText { function: TextFn::Proper, .. } => "Capitalized Each Word",
            StepOp::TransformText { function: TextFn::Trim, .. } => "Trimmed Text",
            StepOp::TransformText { function: TextFn::Clean, .. } => "Cleaned Text",
            StepOp::FillDown { .. } => "Filled Down",
            StepOp::FillUp { .. } => "Filled Up",
            StepOp::GroupBy { .. } => "Grouped Rows",
            StepOp::Unpivot { .. } => "Unpivoted Columns",
            StepOp::Transpose => "Transposed Table",
            StepOp::MergeQueries { .. } => "Merged Queries",
            StepOp::AppendQueries { .. } => "Appended Query",
            StepOp::AddIndexColumn { .. } => "Added Index",
            StepOp::AddConditionalColumn { .. } => "Added Conditional Column",
        }
    }

    /// Other queries this step reads.
    pub fn referenced_queries(&self) -> Vec<&str> {
        match self {
            StepOp::MergeQueries { query, .. } => vec![query.as_str()],
            StepOp::AppendQueries { queries } => queries.iter().map(String::as_str).collect(),
            _ => vec![],
        }
    }
}

/// Close & Load To…: where the result goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadTo {
    /// A table on a worksheet.
    #[default]
    Table,
    /// Only Create Connection: the query is kept and refreshable but loads nothing.
    ConnectionOnly,
}

/// Where a query's result is loaded, and the table holding it once loaded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryLoad {
    #[serde(default)]
    pub to: LoadTo,
    /// Existing worksheet to load into; `None` = a new worksheet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// Top-left cell on `sheet` (`A1` when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    /// The table the result was loaded into (kept while it exists; refresh rewrites it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

/// Query Properties › Usage › Refresh control.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshOptions {
    /// Enable background refresh (kept for Excel; GridCraft refreshes in the foreground).
    #[serde(default = "yes")]
    pub background: bool,
    /// Refresh every N minutes while the workbook is open (0 = off).
    #[serde(default)]
    pub every_minutes: u32,
    /// Refresh data when opening the file (once external content is enabled).
    #[serde(default)]
    pub on_open: bool,
    /// Include in Refresh All.
    #[serde(default = "yes")]
    pub in_refresh_all: bool,
}

fn yes() -> bool {
    true
}

impl Default for RefreshOptions {
    fn default() -> Self {
        RefreshOptions { background: true, every_minutes: 0, on_open: false, in_refresh_all: true }
    }
}

/// The outcome of the last refresh (rows loaded, or the error), as Queries & Connections shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Largest M formula text kept for one Excel query, in bytes.
pub const MAX_M_TEXT: usize = 4 * 1024 * 1024;

/// A Power Query query authored in Excel: its M formula as Excel stored it (the `DataMashup` part)
/// and where it loads. GridCraft has no M interpreter, so these are listed and shown read-only and
/// never refreshed; Excel's own parts are kept byte for byte when the workbook is saved. Read from
/// the file, never written: not in JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExcelQuery {
    pub name: String,
    /// The query's M expression (the text after `shared Name =` in `Section1.m`).
    pub formula: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The connection (`xl/connections.xml`) that reads the query, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The table the query loads into, and its sheet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    /// Also loaded to the Data Model.
    pub data_model: bool,
    /// Rows of the last load, and when it happened (as Excel recorded them).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refreshed: Option<String>,
    /// Excel's Refresh data when opening the file.
    pub refresh_on_open: bool,
    /// Why the last refresh in GridCraft failed, if it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ExcelQuery {
    /// `table`, `dataModel` or `connectionOnly`.
    pub fn load_kind(&self) -> &'static str {
        if self.table.is_some() {
            "table"
        } else if self.data_model {
            "dataModel"
        } else {
            "connectionOnly"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_read_and_write_as_flat_json() {
        let q = Query {
            steps: vec![
                QueryStep { name: "Promoted Headers".into(), op: StepOp::PromoteHeaders },
                QueryStep {
                    name: String::new(),
                    op: StepOp::FilterRows {
                        column: "Region".into(),
                        values: Some(vec![Some(Literal::Text("North".into())), None]),
                        conditions: vec![FilterCondition { op: FilterOp::GreaterThan, value: Some(Literal::Number(3.0)) }],
                        or: false,
                    },
                },
            ],
            ..Query::new("Sales", QuerySource::Csv { path: "sales.csv".into(), delimiter: None, encoding: None })
        };
        let text = serde_json::to_string(&q).unwrap();
        assert!(text.contains(r#""op":"promoteHeaders""#), "{text}");
        assert!(text.contains(r#""kind":"csv""#), "{text}");
        let back: Query = serde_json::from_str(&text).unwrap();
        assert_eq!(back, q);
        // Minimal hand-written steps (no name, defaults) read too.
        let step: QueryStep = serde_json::from_str(r#"{"op":"addIndexColumn"}"#).unwrap();
        assert_eq!(step.op, StepOp::AddIndexColumn { name: "Index".into(), start: 0, step: 1 });
        assert!(serde_json::from_str::<QueryStep>(r#"{"op":"noSuchStep"}"#).is_err());
    }
}
