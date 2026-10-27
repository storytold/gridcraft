//! Sheet features beyond cells: tables, conditional formatting, data validation, comments,
//! hyperlinks, charts, images, autofilter, print settings.

use gridcraft_core::{CellRef, RangeRef};
use serde::{Deserialize, Serialize};

use crate::style::{Color, Style};

mod pivot;
mod print;

pub use pivot::{PivotCalcField, PivotDateGroup, PivotField, PivotFilter, PivotFunc, PivotLayout, PivotShowAs, PivotSort, PivotTable, PivotValue};
pub use print::{EditRange, HfPage, LineInfo, Orientation, PrintSettings, SheetProtection, password_hash};

// ---------------------------------------------------------------- tables

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TotalsFn {
    #[default]
    None,
    Average,
    Count,
    CountNums,
    Max,
    Min,
    Sum,
    StdDev,
    Var,
    Custom,
}

impl TotalsFn {
    /// SUBTOTAL function number.
    pub fn subtotal_code(&self) -> Option<u32> {
        Some(match self {
            TotalsFn::Average => 101,
            TotalsFn::Count => 103,
            TotalsFn::CountNums => 102,
            TotalsFn::Max => 104,
            TotalsFn::Min => 105,
            TotalsFn::Sum => 109,
            TotalsFn::StdDev => 107,
            TotalsFn::Var => 110,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableColumn {
    pub name: String,
    pub totals: TotalsFn,
    /// Totals row label (first column) or custom formula.
    pub totals_label: Option<String>,
    /// Calculated column formula.
    pub formula: Option<String>,
    /// `uniqueName`: the name XML maps know the column by.
    #[serde(default)]
    pub unique_name: Option<String>,
    /// Binding to an element of an XML map. It moves with the column and goes when the column is
    /// deleted; inserted columns are unmapped.
    #[serde(default)]
    pub xml: Option<XmlColumnPr>,
}

/// A table column's binding to an XML map (`<xmlColumnPr>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct XmlColumnPr {
    pub map_id: u32,
    pub xpath: String,
    /// `xmlDataType` as written in the file (`string`, `boolean`, `integer`…).
    pub data_type: String,
    pub denormalized: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Table {
    pub id: u32,
    pub name: String,
    /// Whole table including header and totals rows.
    pub range: RangeRef,
    pub header_row: bool,
    pub totals_row: bool,
    pub columns: Vec<TableColumn>,
    pub style: String,
    pub banded_rows: bool,
    pub banded_cols: bool,
    pub first_col: bool,
    pub last_col: bool,
    pub filter_button: bool,
    /// Criteria of the table's AutoFilter (column offset within the table → criterion). With
    /// criteria present, hidden data rows are filtered out rather than hidden by hand, which
    /// SUBTOTAL distinguishes.
    #[serde(default)]
    pub filter: Vec<(u32, FilterCriterion)>,
    /// `tableType` when it isn't a plain worksheet table: `xml` for a table bound to an XML map.
    #[serde(default)]
    pub table_type: Option<String>,
    /// The connection (`xl/connections.xml`) an XML table is bound through.
    #[serde(default)]
    pub connection_id: Option<u32>,
}

impl Table {
    pub fn data_range(&self) -> Option<RangeRef> {
        let r0 = self.range.start.row + self.header_row as u32;
        let r1 = self.range.end.row.checked_sub(self.totals_row as u32)?;
        if r1 < r0 {
            return None;
        }
        Some(RangeRef::new(CellRef::new(r0, self.range.start.col), CellRef::new(r1, self.range.end.col)))
    }
    pub fn header_range(&self) -> Option<RangeRef> {
        self.header_row.then(|| RangeRef::new(self.range.start, CellRef::new(self.range.start.row, self.range.end.col)))
    }
    pub fn totals_range(&self) -> Option<RangeRef> {
        self.totals_row.then(|| RangeRef::new(CellRef::new(self.range.end.row, self.range.start.col), self.range.end))
    }
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name))
    }
}

// ---------------------------------------------------------------- conditional formatting

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CfOperator {
    Between,
    NotBetween,
    Equal,
    NotEqual,
    Greater,
    Less,
    GreaterOrEqual,
    LessOrEqual,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CfValueKind {
    Min,
    Max,
    Number(f64),
    Percent(f64),
    Percentile(f64),
    Formula(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CfRule {
    /// Cell value compared with one or two formulas.
    CellIs {
        op: CfOperator,
        a: String,
        b: Option<String>,
        style: Box<Style>,
    },
    /// Custom formula (relative to the top-left cell of the range).
    Expression {
        formula: String,
        style: Box<Style>,
    },
    ContainsText {
        text: String,
        style: Box<Style>,
    },
    NotContainsText {
        text: String,
        style: Box<Style>,
    },
    BeginsWith {
        text: String,
        style: Box<Style>,
    },
    EndsWith {
        text: String,
        style: Box<Style>,
    },
    Blanks {
        style: Box<Style>,
    },
    NoBlanks {
        style: Box<Style>,
    },
    Errors {
        style: Box<Style>,
    },
    NoErrors {
        style: Box<Style>,
    },
    Duplicate {
        style: Box<Style>,
    },
    Unique {
        style: Box<Style>,
    },
    /// Top/bottom N or N percent.
    Top10 {
        bottom: bool,
        percent: bool,
        rank: u32,
        style: Box<Style>,
    },
    AboveAverage {
        below: bool,
        equal: bool,
        std_dev: u8,
        style: Box<Style>,
    },
    /// `yesterday`, `today`, `tomorrow`, `last7Days`, `lastWeek`, `thisWeek`, `nextWeek`, `lastMonth`, `thisMonth`, `nextMonth`.
    TimePeriod {
        period: String,
        style: Box<Style>,
    },
    ColorScale {
        stops: Vec<(CfValueKind, Color)>,
    },
    DataBar {
        min: CfValueKind,
        max: CfValueKind,
        color: Color,
        gradient: bool,
        show_value: bool,
    },
    /// Icon set name (`3Arrows`, `3TrafficLights1`, `4Rating`, `5Quarters`…).
    IconSet {
        set: String,
        thresholds: Vec<CfValueKind>,
        reverse: bool,
        show_value: bool,
    },
}

/// Number of icons in an icon set (`3Arrows` → 3, `5Quarters` → 5), always 3 to 5.
pub fn icon_count(set: &str) -> usize {
    set.chars().next().and_then(|c| c.to_digit(10)).unwrap_or(3).clamp(3, 5) as usize
}

/// Excel's default icon-set thresholds: one per icon, the first being the lower bound,
/// then even percent steps (`0, 33, 67` for three icons).
pub fn default_icon_thresholds(set: &str) -> Vec<CfValueKind> {
    let k = icon_count(set);
    (0..k).map(|i| CfValueKind::Percent((i as f64 * 100.0 / k as f64).round())).collect()
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CondFormat {
    pub ranges: Vec<RangeRef>,
    pub rule: CfRule,
    pub priority: u32,
    pub stop_if_true: bool,
}

// ---------------------------------------------------------------- data validation

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationKind {
    #[default]
    Any,
    Whole,
    Decimal,
    List,
    Date,
    Time,
    TextLength,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorStyle {
    #[default]
    Stop,
    Warning,
    Information,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Validation {
    pub ranges: Vec<RangeRef>,
    pub kind: ValidationKind,
    pub op: CfOperator,
    /// Formula 1 (list source: `"a,b,c"` literal list or `=$A$1:$A$5`).
    pub f1: String,
    pub f2: Option<String>,
    pub allow_blank: bool,
    pub in_cell_dropdown: bool,
    pub input_title: String,
    pub input_message: String,
    pub show_input: bool,
    pub error_title: String,
    pub error_message: String,
    pub error_style: ErrorStyle,
    pub show_error: bool,
}

impl Default for Validation {
    fn default() -> Self {
        Validation {
            ranges: vec![],
            kind: ValidationKind::Any,
            op: CfOperator::Between,
            f1: String::new(),
            f2: None,
            allow_blank: true,
            in_cell_dropdown: true,
            input_title: String::new(),
            input_message: String::new(),
            show_input: true,
            error_title: String::new(),
            error_message: String::new(),
            error_style: ErrorStyle::Stop,
            show_error: true,
        }
    }
}

// ---------------------------------------------------------------- comments, links

/// A note (classic comment) or a threaded comment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comment {
    pub author: String,
    pub text: String,
    /// Replies of a threaded comment (author, text).
    pub replies: Vec<(String, String)>,
    pub threaded: bool,
    pub resolved: bool,
    pub visible: bool,
    /// Threaded comments: when the thread (first entry) and each reply were written, ISO 8601 as
    /// read from the file (`dT`); empty when unknown, e.g. for comments made here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dates: Vec<String>,
    /// Where the note's box sits and how big it is, as read from the file; `None` (a note made
    /// here, or a file without the shape) writes the default box next to the cell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_box: Option<NoteBox>,
}

/// A note box's place and size, from the note's VML shape: Excel's `<x:Anchor>` with its corners
/// stored relative to the note's cell, so the box moves with the note when rows or columns are
/// inserted or deleted, and the shape's width and height.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NoteBox {
    /// Top-left corner: columns and rows from the note's cell (negative = left of or above it),
    /// then the pixel offset into that column and row.
    pub from: NoteCorner,
    /// Bottom-right corner, the same way.
    pub to: NoteCorner,
    /// The shape's width and height in points (the VML style's `width`/`height`), when given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_pt: Option<(f64, f64)>,
}

/// One corner of a [`NoteBox`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteCorner {
    pub col: i64,
    pub col_px: u32,
    pub row: i64,
    pub row_px: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hyperlink {
    /// URL, `mailto:`, a file path (`Book.xlsx`, `Book.xlsx#Sheet1!A2`), or an in-workbook location
    /// like `Sheet2!A1`.
    pub target: String,
    pub tooltip: Option<String>,
    /// The target is outside this workbook (Excel's "Existing File or Web Page"): a relationship in
    /// the file, not a `location`. A bare file name can't be told from a defined name by its text.
    #[serde(default)]
    pub external: bool,
}

// ---------------------------------------------------------------- drawing objects

/// How a floating object is pinned to the grid when its cells move or resize. Mirrors Excel's
/// Format ▸ Size and Properties ▸ Properties, stored in the file as the anchor's `editAs`:
/// `twoCell` (absent `editAs`), `oneCell` and `absolute`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AnchorMode {
    /// Move and size with cells (Excel's default; `<xdr:twoCellAnchor>`).
    #[default]
    MoveAndSize,
    /// Move but don't size with cells (`<xdr:twoCellAnchor editAs="oneCell">` / `<xdr:oneCellAnchor>`).
    MoveOnly,
    /// Don't move or size with cells (`editAs="absolute"` / `<xdr:absoluteAnchor>`).
    Absolute,
}

/// Position of a floating object: top-left cell plus offset, size in points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub cell: CellRef,
    pub dx: f32,
    pub dy: f32,
    pub width: f32,
    pub height: f32,
    /// How the object follows its anchor cell when rows/columns move (sort, insert, delete).
    #[serde(default)]
    pub mode: AnchorMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ChartKind {
    #[default]
    ColumnClustered,
    ColumnStacked,
    ColumnStacked100,
    BarClustered,
    BarStacked,
    BarStacked100,
    Line,
    LineMarkers,
    LineStacked,
    Pie,
    Doughnut,
    Area,
    AreaStacked,
    AreaStacked100,
    Scatter,
    ScatterLines,
    Bubble,
    Radar,
    Histogram,
    Waterfall,
    Funnel,
    Treemap,
    Sunburst,
    BoxWhisker,
    Stock,
    Combo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegendPos {
    None,
    #[default]
    Bottom,
    Top,
    Left,
    Right,
}

/// Quartile calculation for a box & whisker series.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuartileMethod {
    #[default]
    Exclusive,
    Inclusive,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Series {
    /// Formula text for the series name (`Sheet1!$B$1`) or a literal.
    pub name: Option<String>,
    pub categories: Option<String>,
    pub values: String,
    /// X values for scatter / bubble sizes.
    pub bubble_sizes: Option<String>,
    pub color: Option<Color>,
    /// Combo charts: this series drawn as a line on the secondary axis.
    pub secondary: bool,
    pub kind: Option<ChartKind>,
    /// Draw line segments as a smoothed (spline) curve.
    #[serde(default)]
    pub smooth: bool,
    /// Box & whisker quartiles; Excel defaults to exclusive.
    #[serde(default)]
    pub quartiles: QuartileMethod,
    /// Trendlines fitted to this series (Excel allows several per series).
    #[serde(default)]
    pub trendlines: Vec<Trendline>,
}

/// Regression a trendline fits (`c:trendlineType`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrendlineKind {
    #[default]
    Linear,
    /// `y = c·e^(b·x)`.
    Exponential,
    /// `y = c·ln(x) + b`.
    Logarithmic,
    /// Polynomial of this order (2..=6).
    Polynomial(u8),
    /// `y = c·x^b`.
    Power,
    /// Average of this many consecutive points (2..=255).
    MovingAverage(u32),
}

/// DrawingML preset line dash (`a:prstDash`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineDash {
    #[default]
    Solid,
    Dot,
    Dash,
    LgDash,
    DashDot,
    LgDashDot,
    LgDashDotDot,
    SysDash,
    SysDot,
    SysDashDot,
    SysDashDotDot,
}

/// A chart trendline (Add Chart Element ▸ Trendline, Format Trendline pane).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Trendline {
    pub kind: TrendlineKind,
    /// Custom name shown in the legend; `None` = automatic ("Linear (Sales)").
    pub name: Option<String>,
    /// Forecast past the last point, in x units (categories on a category axis).
    pub forward: f64,
    /// Forecast before the first point, in x units.
    pub backward: f64,
    /// Set Intercept: fixed y at x = 0 (linear, exponential, polynomial).
    pub intercept: Option<f64>,
    pub display_equation: bool,
    pub display_r_squared: bool,
    /// Line colour; `None` = the series colour.
    pub color: Option<Color>,
    /// Line width in points; `None` = default.
    pub width: Option<f32>,
    /// Line dash; `None` = default.
    pub dash: Option<LineDash>,
}

impl Trendline {
    /// Longest forward / backward forecast kept from a file or a command, in x units. Excel's own
    /// limit is unconfirmed; the renderer additionally bounds the forecast by the plot's slot budget.
    pub const MAX_FORECAST: f64 = 100_000.0;

    /// A forecast distance as stored: finite, not negative (NaN and infinity become 0) and at most
    /// [`Self::MAX_FORECAST`].
    pub fn clamp_forecast(v: f64) -> f64 {
        if v.is_finite() && v > 0.0 { v.min(Self::MAX_FORECAST) } else { 0.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub id: u32,
    pub kind: ChartKind,
    pub anchor: Anchor,
    pub title: Option<String>,
    pub series: Vec<Series>,
    pub legend: LegendPos,
    pub data_labels: bool,
    pub gridlines: bool,
    pub style: u32,
    pub x_title: Option<String>,
    pub y_title: Option<String>,
    /// Source range when created from a selection (for Switch Row/Column).
    pub source: Option<String>,
    pub by_rows: bool,
    /// A PivotChart's PivotTable (`Sheet2!PivotTable1`): its series are rebuilt from the report
    /// whenever the PivotTable is refreshed or its layout changes.
    #[serde(default)]
    pub pivot: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Image {
    pub id: u32,
    pub anchor: Anchor,
    /// Encoded bytes (PNG/JPEG), base64 in JSON. Shared, so a copy of the sheet (an undo step)
    /// doesn't copy the picture.
    #[serde(with = "crate::b64::shared")]
    pub data: std::sync::Arc<Vec<u8>>,
    pub mime: String,
    pub alt: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    #[default]
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Triangle,
    Line,
    Arrow,
    TextBox,
    /// An icon from GridCraft's own icon set; `Shape::text` holds the icon name.
    Icon,
    /// A freehand ink stroke; `Shape::text` holds the points as `x,y;x,y;…` relative to the
    /// anchor (sheet points), `Shape::line` the ink colour and `fill` unused.
    Ink,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub id: u32,
    pub kind: ShapeKind,
    pub anchor: Anchor,
    pub fill: Color,
    pub line: Color,
    pub text: String,
    /// Outline visibility and text formatting (DrawingML `a:ln`, `a:bodyPr`, `a:pPr`, `a:rPr`).
    #[serde(default)]
    pub format: ShapeFormat,
}

/// Horizontal alignment of a shape's text (`a:pPr algn`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeTextAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Vertical anchor of a shape's text in its box (`a:bodyPr anchor`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeTextAnchor {
    #[default]
    Top,
    Middle,
    Bottom,
}

/// How a shape's outline and text look. The text properties apply to the whole text (the first
/// paragraph's and first run's properties when a file has several).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShapeFormat {
    /// No outline (`a:ln/a:noFill`), whatever `Shape::line` says.
    pub no_line: bool,
    /// Text size in points (`a:rPr sz` / 100); `None` is the default size.
    pub font_size: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    /// Typeface (`a:latin typeface`); `None` is the default font.
    pub font: Option<String>,
    /// Text fill colour; `None` is the default (black in text boxes).
    pub text_color: Option<Color>,
    /// Text outline (`a:rPr/a:ln`): colour and width in points.
    pub text_outline: Option<(Color, f32)>,
    /// `None`: the shape's default (left for text boxes, centred for other shapes).
    pub align: Option<ShapeTextAlign>,
    /// `None`: the shape's default (top for text boxes, middle for other shapes).
    pub anchor: Option<ShapeTextAnchor>,
    /// Paragraphs run right to left (`a:pPr rtl`).
    pub rtl: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SparklineKind {
    #[default]
    Line,
    Column,
    WinLoss,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sparkline {
    pub cell: CellRef,
    pub source: String,
    pub kind: SparklineKind,
    pub color: Color,
    pub markers: bool,
}

// ---------------------------------------------------------------- filter & sort

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FilterCriterion {
    /// Show rows whose display text is one of these (case-insensitive); `blanks` includes empty.
    Values {
        values: Vec<String>,
        blanks: bool,
    },
    /// Custom: one or two comparisons (`op` like `>=`, `=*abc*`), joined by and/or.
    Custom {
        a: (String, String),
        b: Option<(String, String)>,
        and: bool,
    },
    Top10 {
        bottom: bool,
        percent: bool,
        count: u32,
    },
    AboveAverage(bool),
    FillColor(Color),
    FontColor(Color),
    /// Rows whose cell shows icon `id` (0-based; `None` = no icon) of conditional-format icon set
    /// `set` (`iconFilter`). Kept from files; re-applying the filter leaves those rows as they are.
    Icon {
        set: String,
        id: Option<u32>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AutoFilter {
    pub range: RangeRef,
    /// Column offset within `range` → criterion.
    pub criteria: Vec<(u32, FilterCriterion)>,
}

/// A slicer: an on-sheet filter widget bound to one column of a table. Clicking its value tiles
/// sets that column's `FilterCriterion::Values` in the sheet's [`AutoFilter`] and re-applies it,
/// so a slicer is just a visual front-end over the existing filter machinery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Slicer {
    pub id: u32,
    /// Name of the table this slicer filters.
    pub table: String,
    /// Header name of the filtered column within the table.
    pub column: String,
    /// Caption shown in the slicer header (defaults to the column name).
    pub caption: String,
    pub anchor: Anchor,
    /// Selected display values. `None` means every value (no filter from this slicer); an empty
    /// list means nothing is selected (all rows hidden), matching Excel's "clear all buttons".
    pub selected: Option<Vec<String>>,
    /// Number of button columns in the layout (Excel default 1).
    pub columns: u32,
}

#[cfg(test)]
mod trendline_forecast_tests {
    use super::Trendline;

    #[test]
    fn clamp_forecast_bounds_hostile_values() {
        assert_eq!(Trendline::clamp_forecast(2.5), 2.5);
        assert_eq!(Trendline::clamp_forecast(0.0), 0.0);
        assert_eq!(Trendline::clamp_forecast(-4.0), 0.0);
        assert_eq!(Trendline::clamp_forecast(f64::NAN), 0.0);
        assert_eq!(Trendline::clamp_forecast(f64::INFINITY), 0.0);
        assert_eq!(Trendline::clamp_forecast(f64::NEG_INFINITY), 0.0);
        assert_eq!(Trendline::clamp_forecast(1e300), Trendline::MAX_FORECAST);
    }
}
