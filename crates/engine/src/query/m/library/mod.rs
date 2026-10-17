//! The M standard library GridCraft supports: names of functions and constants, argument
//! checking, and dispatch to the modules (`table`, `source`, `convert`, `text`, `number`, `date`, `list`, `record`, `splitter`).

pub mod convert;
pub mod date;
pub mod list;
pub mod number;
pub mod record;
pub mod source;
pub mod splitter;
pub mod table;
pub mod text;

use std::rc::Rc;

use super::eval::Ev;
use super::value::{MError, MFunc, MRecord, MResult, MTable, MType, MValue, date_text, duration_text, time_text};

/// Library modules: a dotted name in one of these that GridCraft lacks is reported as an
/// unsupported M function (rather than an unknown name).
pub const MODULES: &[&str] = &[
    "Access",
    "ActiveDirectory",
    "AnalysisServices",
    "AzureStorage",
    "Binary",
    "BinaryEncoding",
    "BinaryOccurrence",
    "Byte",
    "Character",
    "Combiner",
    "Comparer",
    "Compression",
    "Csv",
    "CsvStyle",
    "Cube",
    "Culture",
    "Currency",
    "DB2",
    "Date",
    "DateTime",
    "DateTimeZone",
    "Day",
    "Decimal",
    "Diagnostics",
    "Double",
    "Duration",
    "Embedded",
    "Error",
    "Essbase",
    "Excel",
    "Exchange",
    "Expression",
    "File",
    "Folder",
    "Function",
    "GroupKind",
    "Guid",
    "Hdfs",
    "HdInsight",
    "Html",
    "Identity",
    "Int16",
    "Int32",
    "Int64",
    "Int8",
    "ItemExpression",
    "JoinAlgorithm",
    "JoinKind",
    "JoinSide",
    "Json",
    "Lines",
    "List",
    "Logical",
    "MissingField",
    "MySQL",
    "Number",
    "OData",
    "Odbc",
    "OleDb",
    "Oracle",
    "Occurrence",
    "Order",
    "Percentage",
    "PercentileMode",
    "PostgreSQL",
    "Precision",
    "QuoteStyle",
    "Record",
    "RelativePosition",
    "Replacer",
    "RoundingMode",
    "RowExpression",
    "Salesforce",
    "SapBusinessWarehouse",
    "SapHana",
    "SharePoint",
    "Single",
    "Splitter",
    "Sql",
    "Sybase",
    "Table",
    "Teradata",
    "Text",
    "TextEncoding",
    "Time",
    "TraceLevel",
    "Type",
    "Uri",
    "Value",
    "Variable",
    "Web",
    "WebAction",
    "Xml",
];

/// Every library function GridCraft runs, with its fewest and most arguments.
pub const FUNCTIONS: &[(&str, usize, usize)] = &[
    // sources and binaries
    ("Excel.CurrentWorkbook", 0, 0),
    ("Excel.Workbook", 1, 3),
    ("File.Contents", 1, 2),
    ("Csv.Document", 1, 5),
    ("Web.Contents", 1, 2),
    ("Web.Page", 1, 2),
    ("Json.Document", 1, 2),
    ("Folder.Files", 1, 2),
    ("OData.Feed", 1, 3),
    ("Odbc.Query", 2, 3),
    ("Text.FromBinary", 1, 2),
    ("Binary.FromText", 1, 2),
    ("Binary.Decompress", 2, 2),
    ("Binary.Buffer", 1, 1),
    ("Lines.FromBinary", 1, 4),
    ("#binary", 1, 1),
    // tables
    ("#table", 2, 2),
    ("Table.FromRows", 1, 2),
    ("Table.FromRecords", 1, 3),
    ("Table.FromList", 1, 5),
    ("Table.FromColumns", 1, 2),
    ("Table.PromoteHeaders", 1, 2),
    ("Table.DemoteHeaders", 1, 1),
    ("Table.TransformColumnTypes", 2, 3),
    ("Table.SelectColumns", 2, 3),
    ("Table.RemoveColumns", 2, 3),
    ("Table.RenameColumns", 2, 3),
    ("Table.ReorderColumns", 2, 3),
    ("Table.DuplicateColumn", 3, 4),
    ("Table.FirstN", 2, 2),
    ("Table.LastN", 2, 2),
    ("Table.Skip", 1, 2),
    ("Table.RemoveFirstN", 1, 2),
    ("Table.RemoveLastN", 1, 2),
    ("Table.Range", 2, 3),
    ("Table.RemoveRows", 2, 3),
    ("Table.AlternateRows", 4, 4),
    ("Table.ReverseRows", 1, 1),
    ("Table.Distinct", 1, 2),
    ("Table.RemoveRowsWithErrors", 1, 2),
    ("Table.SelectRowsWithErrors", 1, 2),
    ("Table.SelectRows", 2, 2),
    ("Table.Sort", 2, 2),
    ("Table.ReplaceValue", 5, 5),
    ("Table.ReplaceErrorValues", 2, 2),
    ("Table.SplitColumn", 3, 6),
    ("Table.TransformColumns", 2, 4),
    ("Table.TransformColumnNames", 2, 3),
    ("Table.FillDown", 2, 2),
    ("Table.FillUp", 2, 2),
    ("Table.Group", 3, 5),
    ("Table.UnpivotOtherColumns", 4, 4),
    ("Table.Unpivot", 4, 4),
    ("Table.Pivot", 4, 5),
    ("Table.Transpose", 1, 2),
    ("Table.NestedJoin", 6, 8),
    ("Table.ExpandTableColumn", 3, 4),
    ("Table.ExpandRecordColumn", 3, 4),
    ("Table.ExpandListColumn", 2, 2),
    ("Table.Combine", 1, 2),
    ("Table.AddIndexColumn", 2, 5),
    ("Table.AddColumn", 3, 4),
    ("Table.CombineColumns", 4, 4),
    ("Table.ColumnNames", 1, 1),
    ("Table.RowCount", 1, 1),
    ("Table.ColumnCount", 1, 1),
    ("Table.Column", 2, 2),
    ("Table.ToRows", 1, 1),
    ("Table.ToRecords", 1, 1),
    ("Table.First", 1, 2),
    ("Table.Last", 1, 2),
    ("Table.IsEmpty", 1, 1),
    ("Table.Buffer", 1, 2),
    // splitters, combiners, replacers, comparers
    ("Splitter.SplitTextByDelimiter", 1, 3),
    ("Splitter.SplitTextByEachDelimiter", 1, 3),
    ("Splitter.SplitTextByPositions", 1, 2),
    ("Splitter.SplitTextByLengths", 1, 2),
    ("Splitter.SplitByNothing", 0, 0),
    ("Combiner.CombineTextByDelimiter", 1, 2),
    ("Replacer.ReplaceText", 3, 3),
    ("Replacer.ReplaceValue", 3, 3),
    ("Comparer.Ordinal", 2, 2),
    ("Comparer.OrdinalIgnoreCase", 2, 2),
    // lists
    ("List.Sum", 1, 2),
    ("List.Min", 1, 3),
    ("List.Max", 1, 3),
    ("List.Average", 1, 2),
    ("List.Median", 1, 2),
    ("List.Count", 1, 1),
    ("List.NonNullCount", 1, 1),
    ("List.Distinct", 1, 2),
    ("List.First", 1, 2),
    ("List.Last", 1, 2),
    ("List.Select", 2, 2),
    ("List.Transform", 2, 2),
    ("List.Contains", 2, 3),
    ("List.Combine", 1, 1),
    ("List.RemoveNulls", 1, 1),
    ("List.Sort", 1, 2),
    ("List.Reverse", 1, 1),
    ("List.Numbers", 2, 3),
    ("List.Buffer", 1, 1),
    ("List.IsEmpty", 1, 1),
    ("List.PositionOf", 2, 4),
    // text
    ("Text.From", 1, 2),
    ("Text.Upper", 1, 2),
    ("Text.Lower", 1, 2),
    ("Text.Proper", 1, 2),
    ("Text.Trim", 1, 2),
    ("Text.Clean", 1, 1),
    ("Text.Contains", 2, 3),
    ("Text.StartsWith", 2, 3),
    ("Text.EndsWith", 2, 3),
    ("Text.Length", 1, 1),
    ("Text.Replace", 3, 3),
    ("Text.Combine", 1, 2),
    ("Text.Split", 2, 2),
    ("Text.Start", 2, 2),
    ("Text.End", 2, 2),
    ("Text.Middle", 2, 3),
    ("Text.BeforeDelimiter", 2, 3),
    ("Text.AfterDelimiter", 2, 3),
    ("Text.BetweenDelimiters", 3, 5),
    // numbers, dates, logicals, records, errors
    ("Number.From", 1, 2),
    ("Number.FromText", 1, 2),
    ("Number.Round", 1, 3),
    ("Number.RoundUp", 1, 2),
    ("Number.RoundDown", 1, 2),
    ("Number.Abs", 1, 1),
    ("Number.Mod", 2, 3),
    ("Number.IntegerDivide", 2, 3),
    ("Number.Power", 2, 2),
    ("Number.Sqrt", 1, 1),
    ("Date.From", 1, 2),
    ("Date.Year", 1, 1),
    ("Date.Month", 1, 1),
    ("Date.Day", 1, 1),
    ("Date.AddDays", 2, 2),
    ("DateTime.Date", 1, 1),
    ("DateTime.Time", 1, 1),
    ("DateTime.From", 1, 2),
    ("DateTime.LocalNow", 0, 0),
    ("#date", 3, 3),
    ("#datetime", 6, 6),
    ("#datetimezone", 8, 8),
    ("#time", 3, 3),
    ("#duration", 4, 4),
    ("Logical.From", 1, 2),
    ("Record.Field", 2, 2),
    ("Record.FieldNames", 1, 1),
    ("Record.FieldValues", 1, 1),
    ("Record.HasFields", 2, 2),
    ("Error.Record", 1, 3),
    ("List.FirstN", 2, 2),
    ("List.LastN", 1, 2),
    ("List.Skip", 1, 2),
    ("List.RemoveFirstN", 1, 2),
    ("List.RemoveLastN", 1, 2),
    ("List.Range", 2, 3),
    ("List.RemoveItems", 2, 2),
    ("List.ContainsAny", 2, 3),
    ("List.ContainsAll", 2, 3),
    ("List.Product", 1, 2),
    ("List.StandardDeviation", 1, 1),
    ("List.Mode", 1, 2),
    ("List.Union", 1, 2),
    ("List.Intersect", 1, 2),
    ("List.Difference", 2, 3),
    ("List.Positions", 1, 1),
    ("List.Zip", 1, 1),
    ("List.Repeat", 2, 2),
    ("List.Single", 1, 1),
    ("List.Split", 2, 2),
    ("List.Dates", 3, 3),
    ("List.ReplaceValue", 4, 4),
    ("List.Accumulate", 3, 3),
    ("List.Generate", 3, 4),
    ("Int64.From", 1, 3),
    ("Int32.From", 1, 3),
    ("Int16.From", 1, 3),
    ("Int8.From", 1, 3),
    ("Byte.From", 1, 3),
    ("Currency.From", 1, 3),
    ("Double.From", 1, 2),
    ("Single.From", 1, 2),
    ("Decimal.From", 1, 2),
    ("Percentage.From", 1, 2),
    ("Logical.ToText", 1, 1),
    ("Logical.FromText", 1, 1),
    ("Value.Is", 2, 2),
    ("Value.Type", 1, 1),
    ("Value.Equals", 2, 3),
    ("Number.ToText", 1, 3),
    ("Number.IsEven", 1, 1),
    ("Number.IsOdd", 1, 1),
    ("Number.IsNaN", 1, 1),
    ("Number.Sign", 1, 1),
    ("Number.Exp", 1, 1),
    ("Number.Ln", 1, 1),
    ("Number.Log", 1, 2),
    ("Number.Log10", 1, 1),
    ("Number.Factorial", 1, 1),
    ("Number.RoundTowardZero", 1, 2),
    ("Number.RoundAwayFromZero", 1, 2),
    ("Number.Sin", 1, 1),
    ("Number.Cos", 1, 1),
    ("Number.Tan", 1, 1),
    ("Number.Asin", 1, 1),
    ("Number.Acos", 1, 1),
    ("Number.Atan", 1, 1),
    ("Number.Atan2", 2, 2),
    ("Date.AddMonths", 2, 2),
    ("Date.AddYears", 2, 2),
    ("Date.AddQuarters", 2, 2),
    ("Date.AddWeeks", 2, 2),
    ("Date.StartOfDay", 1, 1),
    ("Date.EndOfDay", 1, 1),
    ("Date.StartOfWeek", 1, 2),
    ("Date.EndOfWeek", 1, 2),
    ("Date.StartOfMonth", 1, 1),
    ("Date.EndOfMonth", 1, 1),
    ("Date.StartOfQuarter", 1, 1),
    ("Date.EndOfQuarter", 1, 1),
    ("Date.StartOfYear", 1, 1),
    ("Date.EndOfYear", 1, 1),
    ("Date.DayOfWeek", 1, 2),
    ("Date.DayOfWeekName", 1, 2),
    ("Date.MonthName", 1, 2),
    ("Date.QuarterOfYear", 1, 1),
    ("Date.DayOfYear", 1, 1),
    ("Date.DaysInMonth", 1, 1),
    ("Date.WeekOfYear", 1, 2),
    ("Date.WeekOfMonth", 1, 2),
    ("Date.IsLeapYear", 1, 1),
    ("Date.ToText", 1, 3),
    ("DateTime.ToText", 1, 3),
    ("DateTimeZone.ToText", 1, 3),
    ("Time.ToText", 1, 3),
    ("Date.FromText", 1, 2),
    ("DateTime.FromText", 1, 2),
    ("Time.FromText", 1, 2),
    ("Time.From", 1, 2),
    ("Time.Hour", 1, 1),
    ("Time.Minute", 1, 1),
    ("Time.Second", 1, 1),
    ("DateTime.FixedLocalNow", 0, 0),
    ("DateTimeZone.UtcNow", 0, 0),
    ("DateTimeZone.FixedUtcNow", 0, 0),
    ("Duration.Days", 1, 1),
    ("Duration.Hours", 1, 1),
    ("Duration.Minutes", 1, 1),
    ("Duration.Seconds", 1, 1),
    ("Duration.TotalDays", 1, 1),
    ("Duration.TotalHours", 1, 1),
    ("Duration.TotalMinutes", 1, 1),
    ("Duration.TotalSeconds", 1, 1),
    ("Duration.From", 1, 1),
    ("Duration.ToText", 1, 2),
    ("Date.IsInCurrentDay", 1, 1),
    ("Date.IsInCurrentWeek", 1, 1),
    ("Date.IsInCurrentMonth", 1, 1),
    ("Date.IsInCurrentQuarter", 1, 1),
    ("Date.IsInCurrentYear", 1, 1),
    ("Date.IsInPreviousDay", 1, 1),
    ("Date.IsInPreviousWeek", 1, 1),
    ("Date.IsInPreviousMonth", 1, 1),
    ("Date.IsInPreviousQuarter", 1, 1),
    ("Date.IsInPreviousYear", 1, 1),
    ("Date.IsInNextDay", 1, 1),
    ("Date.IsInNextWeek", 1, 1),
    ("Date.IsInNextMonth", 1, 1),
    ("Date.IsInNextQuarter", 1, 1),
    ("Date.IsInNextYear", 1, 1),
    ("Date.IsInPreviousNDays", 2, 2),
    ("Date.IsInPreviousNWeeks", 2, 2),
    ("Date.IsInPreviousNMonths", 2, 2),
    ("Date.IsInPreviousNQuarters", 2, 2),
    ("Date.IsInPreviousNYears", 2, 2),
    ("Date.IsInNextNDays", 2, 2),
    ("Date.IsInNextNWeeks", 2, 2),
    ("Date.IsInNextNMonths", 2, 2),
    ("Date.IsInNextNQuarters", 2, 2),
    ("Date.IsInNextNYears", 2, 2),
    ("Date.IsInYearToDate", 1, 1),
    ("Text.TrimStart", 1, 2),
    ("Text.TrimEnd", 1, 2),
    ("Text.PadStart", 2, 3),
    ("Text.PadEnd", 2, 3),
    ("Text.Range", 2, 3),
    ("Text.RemoveRange", 2, 3),
    ("Text.Insert", 3, 3),
    ("Text.ReplaceRange", 4, 4),
    ("Text.Remove", 2, 2),
    ("Text.Select", 2, 2),
    ("Text.PositionOf", 2, 4),
    ("Text.PositionOfAny", 2, 3),
    ("Text.Repeat", 2, 2),
    ("Text.Reverse", 1, 1),
    ("Text.At", 2, 2),
    ("Text.ToList", 1, 1),
    ("Text.SplitAny", 2, 2),
    ("Text.Format", 2, 3),
    ("Character.FromNumber", 1, 1),
    ("Character.ToNumber", 1, 1),
];

/// Library constants (enumerations and type values).
fn constant(name: &str) -> Option<MValue> {
    let n = |x: f64| Some(MValue::Number(x));
    let ty = |t: MType| Some(MValue::Type(Rc::new(t)));
    match name {
        "Order.Ascending"
        | "JoinKind.Inner"
        | "QuoteStyle.None"
        | "MissingField.Error"
        | "Compression.GZip"
        | "BinaryEncoding.Base64"
        | "ExtraValues.List"
        | "GroupKind.Local"
        | "Occurrence.First"
        | "RoundingMode.Up"
        | "Day.Sunday"
        | "RelativePosition.FromStart" => n(0.0),
        "Order.Descending"
        | "JoinKind.LeftOuter"
        | "QuoteStyle.Csv"
        | "MissingField.Ignore"
        | "Compression.Deflate"
        | "BinaryEncoding.Hex"
        | "ExtraValues.Error"
        | "GroupKind.Global"
        | "Occurrence.Last"
        | "RoundingMode.Down"
        | "Day.Monday"
        | "RelativePosition.FromEnd" => n(1.0),
        "JoinKind.RightOuter" | "MissingField.UseNull" | "ExtraValues.Ignore" | "Occurrence.All" | "RoundingMode.AwayFromZero" | "Day.Tuesday" => {
            n(2.0)
        }
        "JoinKind.FullOuter" | "RoundingMode.TowardZero" | "Day.Wednesday" => n(3.0),
        "JoinKind.LeftAnti" | "RoundingMode.ToEven" | "Day.Thursday" => n(4.0),
        "JoinKind.RightAnti" | "Day.Friday" => n(5.0),
        "Day.Saturday" => n(6.0),
        "TextEncoding.Utf8" => n(65001.0),
        "TextEncoding.Utf16" | "TextEncoding.Unicode" => n(1200.0),
        "TextEncoding.BigEndianUnicode" => n(1201.0),
        "TextEncoding.Windows" => n(1252.0),
        "TextEncoding.Ascii" => n(20127.0),
        "Culture.Current" => Some(MValue::text("en-US")),
        "Number.PI" => n(std::f64::consts::PI),
        "Number.E" => n(std::f64::consts::E),
        "Number.Epsilon" => n(f64::from_bits(1)),
        "Number.MaxValue" => n(f64::MAX),
        "Number.MinValue" => n(f64::MIN),
        "Number.NaN" => n(f64::NAN),
        "Number.PositiveInfinity" => n(f64::INFINITY),
        "Number.NegativeInfinity" => n(f64::NEG_INFINITY),
        "Int64.Type" | "Int32.Type" | "Int16.Type" | "Int8.Type" | "Byte.Type" | "Currency.Type" | "Percentage.Type" => {
            let facet = FACETS.iter().find(|f| **f == name).copied();
            ty(MType { kind: "number", nullable: false, facet, fields: vec![] })
        }
        "Number.Type" | "Double.Type" | "Single.Type" | "Decimal.Type" => ty(MType::prim("number")),
        "Text.Type" => ty(MType::prim("text")),
        "Date.Type" => ty(MType::prim("date")),
        "DateTime.Type" => ty(MType::prim("datetime")),
        "DateTimeZone.Type" => ty(MType::prim("datetimezone")),
        "Time.Type" => ty(MType::prim("time")),
        "Duration.Type" => ty(MType::prim("duration")),
        "Logical.Type" => ty(MType::prim("logical")),
        "Binary.Type" => ty(MType::prim("binary")),
        "List.Type" => ty(MType::prim("list")),
        "Record.Type" => ty(MType::prim("record")),
        "Table.Type" => ty(MType::prim("table")),
        "Function.Type" => ty(MType::prim("function")),
        "Any.Type" => ty(MType::prim("any")),
        "None.Type" => ty(MType::prim("none")),
        "Null.Type" => ty(MType::prim("null")),
        "Type.Type" => ty(MType::prim("type")),
        _ => None,
    }
}

const FACETS: &[&str] = &["Int64.Type", "Int32.Type", "Int16.Type", "Int8.Type", "Byte.Type", "Currency.Type", "Percentage.Type"];

/// A library name's value: a constant or a function.
pub fn global(name: &str) -> Option<MValue> {
    constant(name).or_else(|| FUNCTIONS.iter().find(|(n, _, _)| *n == name).map(|_| MValue::builtin(name)))
}

/// Whether GridCraft knows the library name.
pub fn known(name: &str) -> bool {
    global(name).is_some()
}

/// Calls the library function `name` (with `bound` arguments first).
pub fn call(ev: &mut Ev<'_, '_>, name: &str, bound: &[MValue], args: Vec<MValue>) -> MResult<MValue> {
    let mut all = bound.to_vec();
    all.extend(args);
    // Bound functions (splitters, combiners) check their own call; library entries are checked
    // here.
    if bound.is_empty()
        && let Some((_, min, max)) = FUNCTIONS.iter().find(|(n, _, _)| *n == name)
        && (all.len() < *min || all.len() > *max)
    {
        return Err(MError::expr(format!(
            "{} arguments were passed to {name}, which expects {}.",
            all.len(),
            if min == max { format!("{min}") } else { format!("between {min} and {max}") }
        )));
    }
    let a = Args { name, v: all };
    if let Some(r) = table::call(ev, name, &a)? {
        return Ok(r);
    }
    if let Some(r) = source::call(ev, name, &a)? {
        return Ok(r);
    }
    for module in [convert::call, text::call, number::call, date::call, list::call, record::call, splitter::call] {
        if let Some(r) = module(ev, name, &a)? {
            return Ok(r);
        }
    }
    Err(MError::expr(format!("The M function '{name}' isn't supported by GridCraft yet.")))
}

/// A function's arguments, with conversions that fail with Power Query's messages.
pub struct Args<'n> {
    pub name: &'n str,
    pub v: Vec<MValue>,
}

const NULL: MValue = MValue::Null;

pub fn cant(v: &MValue, to: &str) -> MError {
    MError::expr(format!("We cannot convert the value {} to type {to}.", show(v)))
}

impl Args<'_> {
    pub fn at(&self, i: usize) -> &MValue {
        self.v.get(i).unwrap_or(&NULL)
    }
    /// Present and not null.
    pub fn has(&self, i: usize) -> bool {
        !self.at(i).is_null()
    }
    pub fn table(&self, i: usize) -> MResult<Rc<MTable>> {
        match self.at(i) {
            MValue::Table(t) => Ok(t.clone()),
            v => Err(cant(v, "Table")),
        }
    }
    pub fn list(&self, i: usize) -> MResult<Rc<Vec<MValue>>> {
        match self.at(i) {
            MValue::List(l) => Ok(l.clone()),
            v => Err(cant(v, "List")),
        }
    }
    pub fn record(&self, i: usize) -> MResult<Rc<MRecord>> {
        match self.at(i) {
            MValue::Record(r) => Ok(r.clone()),
            v => Err(cant(v, "Record")),
        }
    }
    pub fn opt_record(&self, i: usize) -> MResult<Option<Rc<MRecord>>> {
        if self.has(i) { self.record(i).map(Some) } else { Ok(None) }
    }
    pub fn text(&self, i: usize) -> MResult<Rc<str>> {
        match self.at(i) {
            MValue::Text(t) => Ok(t.clone()),
            v => Err(cant(v, "Text")),
        }
    }
    pub fn opt_text(&self, i: usize) -> MResult<Option<Rc<str>>> {
        if self.has(i) { self.text(i).map(Some) } else { Ok(None) }
    }
    pub fn num(&self, i: usize) -> MResult<f64> {
        match self.at(i) {
            MValue::Number(n) => Ok(*n),
            v => Err(cant(v, "Number")),
        }
    }
    pub fn opt_num(&self, i: usize) -> MResult<Option<f64>> {
        if self.has(i) { self.num(i).map(Some) } else { Ok(None) }
    }
    /// A count: a whole number from 0 (capped to `usize`).
    pub fn count(&self, i: usize) -> MResult<usize> {
        let n = self.num(i)?;
        if n < 0.0 || n.fract() != 0.0 || n.is_nan() {
            return Err(MError::expr(format!("The count {} isn't a whole number from 0.", show(self.at(i)))));
        }
        Ok(if n > usize::MAX as f64 { usize::MAX } else { n as usize })
    }
    pub fn func(&self, i: usize) -> MResult<MValue> {
        match self.at(i) {
            v @ MValue::Function(_) => Ok(v.clone()),
            v => Err(cant(v, "Function")),
        }
    }
    pub fn logical(&self, i: usize) -> MResult<Option<bool>> {
        match self.at(i) {
            MValue::Null => Ok(None),
            MValue::Logical(b) => Ok(Some(*b)),
            v => Err(cant(v, "Logical")),
        }
    }
    /// One name or a list of names.
    pub fn names(&self, i: usize) -> MResult<Vec<Rc<str>>> {
        names_of(self.at(i))
    }
    pub fn int(&self, i: usize, default: i64) -> MResult<i64> {
        match self.at(i) {
            MValue::Null => Ok(default),
            MValue::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() < 9.0e15 => Ok(*n as i64),
            v => Err(cant(v, "Number")),
        }
    }
}

/// A text, or a list of texts (a list item may itself be `{name, …}`, as in sort and distinct
/// criteria: its first item is the name).
pub fn names_of(v: &MValue) -> MResult<Vec<Rc<str>>> {
    match v {
        MValue::Text(t) => Ok(vec![t.clone()]),
        MValue::List(l) => l
            .iter()
            .map(|x| match x {
                MValue::Text(t) => Ok(t.clone()),
                MValue::List(inner) => match inner.first() {
                    Some(MValue::Text(t)) => Ok(t.clone()),
                    _ => Err(cant(x, "Text")),
                },
                other => Err(cant(other, "Text")),
            })
            .collect(),
        other => Err(cant(other, "List")),
    }
}

/// A short rendering of a value for messages (`"abc"`, `5`, `#date(2026, 1, 31)`, `[Table]`).
pub fn show(v: &MValue) -> String {
    match v {
        MValue::Null => "null".into(),
        MValue::Logical(b) => b.to_string(),
        MValue::Number(n) => convert::number_text(*n),
        MValue::Text(t) => {
            let short: String = t.chars().take(60).collect();
            format!("\"{short}{}\"", if t.chars().count() > 60 { "…" } else { "" })
        }
        MValue::Date(d) => {
            let (y, m, dd) = super::value::ymd(*d);
            format!("#date({y}, {m}, {dd})")
        }
        MValue::DateTime(d) => format!("{} {}", date_text(d.floor() as i64, false), time_text(d.rem_euclid(1.0))),
        MValue::DateTimeZone(d, _) => format!("{} {}", date_text(d.floor() as i64, false), time_text(d.rem_euclid(1.0))),
        MValue::Time(t) => time_text(*t),
        MValue::Duration(d) => duration_text(*d),
        MValue::Binary(_) => "[Binary]".into(),
        MValue::List(_) => "[List]".into(),
        MValue::Record(_) => "[Record]".into(),
        MValue::Table(_) => "[Table]".into(),
        MValue::Function(f) => match &**f {
            MFunc::Builtin { name, .. } => format!("[Function {name}]"),
            MFunc::Closure { .. } => "[Function]".into(),
        },
        MValue::Type(t) => format!("[Type {}]", t.kind),
        MValue::Error(e) => format!("[Error {}]", e.message),
        MValue::Lazy(_) => "[Value]".into(),
    }
}

// ---------------------------------------------------------------- helpers the modules share

pub fn long_text(s: String) -> MResult<MValue> {
    if s.len() > super::MAX_TEXT {
        return Err(super::eval::limit("The text is too long."));
    }
    Ok(MValue::text(s))
}

pub fn is_ignore_case(v: &MValue) -> bool {
    matches!(v, MValue::Function(f) if matches!(&**f, MFunc::Builtin { name, .. } if &**name == "Comparer.OrdinalIgnoreCase"))
}

pub fn folded(v: &MValue, fold: bool) -> MValue {
    match v {
        MValue::Text(t) if fold => MValue::text(t.to_lowercase()),
        v => v.clone(),
    }
}

pub fn folded_key(v: &MValue, fold: bool) -> super::value::Key {
    super::value::key(&folded(v, fold))
}

/// A list's items worked out (a held error is raised).
pub fn forced(ev: &mut Ev<'_, '_>, l: &[MValue]) -> MResult<Vec<MValue>> {
    let mut out = Vec::with_capacity(l.len());
    for v in l {
        out.push(ev.force(v.clone())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_library_name_resolves() {
        for (name, min, max) in FUNCTIONS {
            assert!(min <= max, "{name}");
            assert!(global(name).is_some(), "{name}");
            let module = name.trim_start_matches('#').split('.').next().unwrap_or("");
            assert!(name.starts_with('#') || MODULES.contains(&module), "{name}");
        }
        assert!(global("Int64.Type").is_some());
        assert!(global("Table.NoSuchThing").is_none());
    }
}
