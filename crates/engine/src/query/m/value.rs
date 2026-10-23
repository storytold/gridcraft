//! M values: the primitive kinds (null, logical, number, text, date, datetime, datetimezone,
//! time, duration, binary), lists, records, tables, functions and types, and conversions to and
//! from worksheet values.
//!
//! Dates are days since 1970-01-01 (proleptic Gregorian), date-times the same with a fraction,
//! times and durations fractions of a day: worksheet serials are these plus a fixed offset.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;
use std::sync::Arc;

use gridcraft_core::date::{civil_from_days, days_from_civil};
use gridcraft_core::{CellError, DateSystem, Value};
use gridcraft_model::query::ColType;

use super::ast::FuncDef;
use super::eval::Env;

/// Days from the 1900 date system's serial 0 (1899-12-30) to 1970-01-01.
const SERIAL_1970: f64 = 25_569.0;
/// Days between the 1900 and 1904 date systems' origins.
const DAYS_1904: f64 = 1_462.0;

/// An M error: its reason (`Expression.Error`, `DataFormat.Error`…) and message.
#[derive(Clone, Debug, PartialEq)]
pub struct MError {
    pub reason: String,
    pub message: String,
}

impl MError {
    pub fn new(reason: &str, message: impl Into<String>) -> MError {
        MError { reason: reason.to_string(), message: message.into() }
    }
    pub fn expr(message: impl Into<String>) -> MError {
        MError::new("Expression.Error", message)
    }
    pub fn format(message: impl Into<String>) -> MError {
        MError::new("DataFormat.Error", message)
    }
    pub fn source(message: impl Into<String>) -> MError {
        MError::new("DataSource.Error", message)
    }
}

impl std::fmt::Display for MError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.reason, self.message)
    }
}

pub type MResult<T> = Result<T, MError>;

#[derive(Clone, Debug)]
pub enum MValue {
    Null,
    Logical(bool),
    Number(f64),
    Text(Rc<str>),
    /// Days since 1970-01-01.
    Date(i64),
    /// Days since 1970-01-01, with the time of day as the fraction.
    DateTime(f64),
    /// Local date-time and its offset from UTC in minutes.
    DateTimeZone(f64, i32),
    /// Fraction of a day.
    Time(f64),
    /// Days.
    Duration(f64),
    Binary(Rc<Bin>),
    List(Rc<Vec<MValue>>),
    Record(Rc<MRecord>),
    Table(Rc<MTable>),
    Function(Rc<MFunc>),
    Type(Rc<MType>),
    /// An error held in a list item, record field or table cell: raised when the value is used.
    Error(Rc<MError>),
    /// A value worked out on first use (a navigation table's `Data`, a file's `Content`).
    Lazy(Rc<Lazy>),
}

/// Bytes and where they came from (a content type and URL or path help read them).
#[derive(Debug)]
pub struct Bin {
    pub bytes: Arc<[u8]>,
    pub content_type: Option<String>,
    pub origin: String,
}

#[derive(Clone, Debug)]
pub struct MRecord {
    pub names: Rc<[Rc<str>]>,
    pub values: Vec<MValue>,
}

impl MRecord {
    pub fn new(fields: Vec<(Rc<str>, MValue)>) -> MRecord {
        let (names, values): (Vec<Rc<str>>, Vec<MValue>) = fields.into_iter().unzip();
        MRecord { names: names.into(), values }
    }
    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| &**n == name)
    }
    pub fn get(&self, name: &str) -> Option<&MValue> {
        self.index(name).and_then(|i| self.values.get(i))
    }
}

/// Most cells (rows × columns) one table may hold: a short formula could otherwise ask for more
/// memory than the machine has (a million rows of 16,000 columns).
pub const MAX_CELLS: usize = 64 * 1024 * 1024;

/// Refuses a table of `rows` × `width` cells past `MAX_CELLS`. The error ends the refresh, as the
/// other limits do (`try` doesn't catch it).
pub fn check_cells(rows: usize, width: usize) -> MResult<()> {
    if rows.saturating_mul(width.max(1)) > MAX_CELLS {
        return Err(super::eval::limit(format!("A table can hold at most {MAX_CELLS} cells here.")));
    }
    Ok(())
}

/// A table: column names, the type each column is known to have, and rows as wide as the
/// columns.
#[derive(Clone, Debug, Default)]
pub struct MTable {
    pub names: Rc<[Rc<str>]>,
    pub types: Vec<ColType>,
    pub rows: Vec<Vec<MValue>>,
}

impl MTable {
    pub fn new(names: Vec<Rc<str>>) -> MTable {
        let n = names.len();
        MTable { names: names.into(), types: vec![ColType::Any; n], rows: vec![] }
    }
    pub fn col(&self, name: &str) -> MResult<usize> {
        self.names.iter().position(|n| &**n == name).ok_or_else(|| MError::expr(format!("The column '{name}' of the table wasn't found.")))
    }
    pub fn width(&self) -> usize {
        self.names.len()
    }
    pub fn row_record(&self, row: &[MValue]) -> MRecord {
        MRecord { names: self.names.clone(), values: row.to_vec() }
    }
    /// Adds a row, padded or cut to the table's width; rows past `MAX_ROWS` are refused, and so
    /// is a row that takes the table past `MAX_CELLS`.
    pub fn push(&mut self, mut row: Vec<MValue>) -> MResult<()> {
        if self.rows.len() >= super::super::MAX_ROWS {
            return Err(MError::expr(format!("A table can hold at most {} rows here.", super::super::MAX_ROWS)));
        }
        check_cells(self.rows.len() + 1, self.width())?;
        row.resize(self.width(), MValue::Null);
        self.rows.push(row);
        Ok(())
    }
}

pub enum MFunc {
    Closure {
        def: Rc<FuncDef>,
        env: Option<Rc<Env>>,
    },
    /// A library function, with arguments already bound (`Splitter.SplitTextByDelimiter(",")`
    /// returns one).
    Builtin {
        name: Rc<str>,
        bound: Vec<MValue>,
    },
}

impl std::fmt::Debug for MFunc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MFunc::Closure { .. } => write!(f, "function"),
            MFunc::Builtin { name, .. } => write!(f, "{name}"),
        }
    }
}

/// A type value. Facets (`Int64.Type`, `Currency.Type`, `Percentage.Type`) tell Change Type
/// which column type to give.
#[derive(Clone, Debug, PartialEq)]
pub struct MType {
    pub kind: &'static str,
    pub nullable: bool,
    pub facet: Option<&'static str>,
    /// Columns of a table type, fields of a record type.
    pub fields: Vec<(Rc<str>, MType)>,
}

impl MType {
    pub fn prim(kind: &'static str) -> MType {
        MType { kind, nullable: false, facet: None, fields: vec![] }
    }
    pub fn facet(kind: &'static str, facet: &'static str) -> MType {
        MType { kind, nullable: false, facet: Some(facet), fields: vec![] }
    }
    /// The column type Change Type gives for this type.
    pub fn col_type(&self) -> ColType {
        match (self.kind, self.facet) {
            ("number", Some("Int64.Type" | "Int32.Type" | "Int16.Type" | "Int8.Type" | "Byte.Type")) => ColType::Whole,
            ("number", Some("Currency.Type")) => ColType::Currency,
            ("number", Some("Percentage.Type")) => ColType::Percentage,
            ("number", _) => ColType::Number,
            ("text", _) => ColType::Text,
            ("date", _) => ColType::Date,
            ("datetime" | "datetimezone", _) => ColType::DateTime,
            ("time", _) => ColType::Time,
            ("logical", _) => ColType::Logical,
            _ => ColType::Any,
        }
    }
    /// The type a column of `ColType` has.
    pub fn of_col(t: ColType) -> MType {
        match t {
            ColType::Any => MType::prim("any"),
            ColType::Text => MType::prim("text"),
            ColType::Number => MType::prim("number"),
            ColType::Currency => MType::facet("number", "Currency.Type"),
            ColType::Whole => MType::facet("number", "Int64.Type"),
            ColType::Percentage => MType::facet("number", "Percentage.Type"),
            ColType::Date => MType::prim("date"),
            ColType::DateTime => MType::prim("datetime"),
            ColType::Time => MType::prim("time"),
            ColType::Logical => MType::prim("logical"),
        }
    }
}

/// A value worked out on first use.
pub struct Lazy {
    pub state: RefCell<LazyState>,
}

pub type LazyFn = Box<dyn FnOnce(&mut super::eval::Ev<'_, '_>) -> MResult<MValue>>;

pub enum LazyState {
    Pending(LazyFn),
    Running,
    Done(MResult<MValue>),
}

impl std::fmt::Debug for Lazy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "lazy")
    }
}

impl Lazy {
    pub fn value(f: LazyFn) -> MValue {
        MValue::Lazy(Rc::new(Lazy { state: RefCell::new(LazyState::Pending(f)) }))
    }
}

impl MValue {
    pub fn text(s: impl AsRef<str>) -> MValue {
        MValue::Text(Rc::from(s.as_ref()))
    }
    pub fn list(items: Vec<MValue>) -> MValue {
        MValue::List(Rc::new(items))
    }
    pub fn table(t: MTable) -> MValue {
        MValue::Table(Rc::new(t))
    }
    pub fn record(r: MRecord) -> MValue {
        MValue::Record(Rc::new(r))
    }
    pub fn error(e: MError) -> MValue {
        MValue::Error(Rc::new(e))
    }
    pub fn builtin(name: &str) -> MValue {
        MValue::Function(Rc::new(MFunc::Builtin { name: name.into(), bound: vec![] }))
    }
    pub fn bound(name: &str, bound: Vec<MValue>) -> MValue {
        MValue::Function(Rc::new(MFunc::Builtin { name: name.into(), bound }))
    }

    /// The value's kind as M names it (`text`, `table`…), for messages.
    pub fn kind(&self) -> &'static str {
        match self {
            MValue::Null => "null",
            MValue::Logical(_) => "logical",
            MValue::Number(_) => "number",
            MValue::Text(_) => "text",
            MValue::Date(_) => "date",
            MValue::DateTime(_) => "datetime",
            MValue::DateTimeZone(..) => "datetimezone",
            MValue::Time(_) => "time",
            MValue::Duration(_) => "duration",
            MValue::Binary(_) => "binary",
            MValue::List(_) => "list",
            MValue::Record(_) => "record",
            MValue::Table(_) => "table",
            MValue::Function(_) => "function",
            MValue::Type(_) => "type",
            MValue::Error(_) => "error",
            MValue::Lazy(_) => "value",
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, MValue::Null)
    }

    /// The value as worksheet cell content (loading a query): dates become serials in `sys`,
    /// nested values their kind's name, errors blank.
    pub fn to_cell(&self, sys: DateSystem) -> Value {
        let serial = |days: f64| {
            let s = days + SERIAL_1970;
            if sys == DateSystem::D1904 { s - DAYS_1904 } else { s }
        };
        match self {
            MValue::Null | MValue::Error(_) | MValue::Lazy(_) => Value::Empty,
            MValue::Logical(b) => Value::Bool(*b),
            MValue::Number(n) if n.is_finite() => Value::number(*n),
            MValue::Number(n) if n.is_nan() => Value::text("NaN"),
            MValue::Number(n) => Value::text(if *n > 0.0 { "Infinity" } else { "-Infinity" }),
            MValue::Text(t) => Value::text(&**t),
            MValue::Date(d) => Value::number(serial(*d as f64)),
            MValue::DateTime(d) | MValue::DateTimeZone(d, _) => Value::number(serial(*d)),
            MValue::Time(t) | MValue::Duration(t) => Value::number(*t),
            MValue::Binary(_) => Value::text("Binary"),
            MValue::List(_) => Value::text("List"),
            MValue::Record(_) => Value::text("Record"),
            MValue::Table(_) => Value::text("Table"),
            MValue::Function(_) => Value::text("Function"),
            MValue::Type(_) => Value::text("Type"),
        }
    }

    /// A worksheet value read into M, typed by its column (`Date` columns hold serials).
    pub fn from_cell(v: &Value, ty: ColType, sys: DateSystem) -> MValue {
        let days = |s: f64| if sys == DateSystem::D1904 { s + DAYS_1904 - SERIAL_1970 } else { s - SERIAL_1970 };
        match v {
            Value::Empty => MValue::Null,
            Value::Bool(b) => MValue::Logical(*b),
            Value::Text(t) => MValue::Text(Rc::from(&**t)),
            Value::Number(n) => match ty {
                ColType::Date => MValue::Date(days(*n).floor() as i64),
                ColType::DateTime => MValue::DateTime(days(*n)),
                ColType::Time => MValue::Time(n.rem_euclid(1.0)),
                _ => MValue::Number(*n),
            },
            Value::Error(e) => MValue::error(MError::format(format!("The cell holds the error {}.", e.as_str()))),
            Value::Array(_) => MValue::error(MError::format("The cell holds an array.")),
        }
    }
}

// ---------------------------------------------------------------- dates

pub fn ymd(days: i64) -> (i32, u32, u32) {
    civil_from_days(days)
}

pub fn days(y: i64, m: i64, d: i64) -> MResult<i64> {
    if !(1..=9999).contains(&y) || !(1..=12).contains(&m) {
        return Err(MError::expr(format!("#date({y}, {m}, {d}) isn't a valid date.")));
    }
    let (y32, m32) = (y as i32, m as u32);
    let dim = gridcraft_core::date::days_in_month(y32, m32);
    if d < 1 || d > i64::from(dim) {
        return Err(MError::expr(format!("#date({y}, {m}, {d}) isn't a valid date.")));
    }
    Ok(days_from_civil(y32, m32, d as u32))
}

/// Text for a date in the culture's short date form (`en-US`: `1/31/2026`).
pub fn date_text(d: i64, dmy: bool) -> String {
    let (y, m, dd) = ymd(d);
    if dmy { format!("{dd:02}.{m:02}.{y}") } else { format!("{m}/{dd}/{y}") }
}

pub fn time_text(t: f64) -> String {
    // Seconds are cut, not rounded (as .NET shows them); a hair is added for float error.
    let total = ((t.rem_euclid(1.0) * 86_400.0) + 1.0e-6).floor() as i64;
    let (h, mi, s) = (total / 3600 % 24, total / 60 % 60, total % 60);
    let (h12, ap) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{h12}:{mi:02}:{s:02} {ap}")
}

pub fn duration_text(d: f64) -> String {
    let neg = d < 0.0;
    let total = (d.abs() * 86_400.0).round() as i64;
    let (days, h, mi, s) = (total / 86_400, total / 3600 % 24, total / 60 % 60, total % 60);
    format!("{}{days}.{h:02}:{mi:02}:{s:02}", if neg { "-" } else { "" })
}

// ---------------------------------------------------------------- equality and order

/// M's `=`: values of different kinds are unequal; lists, records and tables compare by
/// content.
pub fn equal(a: &MValue, b: &MValue) -> bool {
    match (a, b) {
        (MValue::Null, MValue::Null) => true,
        (MValue::Logical(x), MValue::Logical(y)) => x == y,
        (MValue::Number(x), MValue::Number(y)) => x == y,
        (MValue::Text(x), MValue::Text(y)) => x == y,
        (MValue::Date(x), MValue::Date(y)) => x == y,
        (MValue::DateTime(x), MValue::DateTime(y)) | (MValue::Time(x), MValue::Time(y)) | (MValue::Duration(x), MValue::Duration(y)) => {
            (x - y).abs() < 1e-9
        }
        (MValue::DateTimeZone(x, ox), MValue::DateTimeZone(y, oy)) => (x - f64::from(*ox) / 1440.0 - (y - f64::from(*oy) / 1440.0)).abs() < 1e-9,
        (MValue::Binary(x), MValue::Binary(y)) => x.bytes == y.bytes,
        (MValue::List(x), MValue::List(y)) => x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| equal(a, b)),
        (MValue::Record(x), MValue::Record(y)) => {
            x.names.len() == y.names.len() && x.names.iter().zip(x.values.iter()).all(|(n, v)| y.get(n).is_some_and(|w| equal(v, w)))
        }
        (MValue::Table(x), MValue::Table(y)) => {
            x.names == y.names
                && x.rows.len() == y.rows.len()
                && x.rows.iter().zip(y.rows.iter()).all(|(r, s)| r.iter().zip(s.iter()).all(|(a, b)| equal(a, b)))
        }
        (MValue::Type(x), MValue::Type(y)) => x == y,
        (MValue::Function(x), MValue::Function(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// The order of `<`, `>` for values of the same kind; `None` when they can't be compared.
pub fn order(a: &MValue, b: &MValue) -> Option<Ordering> {
    match (a, b) {
        (MValue::Number(x), MValue::Number(y)) => x.partial_cmp(y),
        (MValue::Text(x), MValue::Text(y)) => Some(x.cmp(y)),
        (MValue::Logical(x), MValue::Logical(y)) => Some(x.cmp(y)),
        (MValue::Date(x), MValue::Date(y)) => Some(x.cmp(y)),
        (MValue::DateTime(x), MValue::DateTime(y)) | (MValue::Time(x), MValue::Time(y)) | (MValue::Duration(x), MValue::Duration(y)) => {
            x.partial_cmp(y)
        }
        (MValue::DateTimeZone(x, ox), MValue::DateTimeZone(y, oy)) => (x - f64::from(*ox) / 1440.0).partial_cmp(&(y - f64::from(*oy) / 1440.0)),
        _ => None,
    }
}

/// A total order for sorting (Table.Sort, List.Sort): null first, then by kind, then by value.
pub fn sort_order(a: &MValue, b: &MValue) -> Ordering {
    fn rank(v: &MValue) -> u8 {
        match v {
            MValue::Null => 0,
            MValue::Logical(_) => 1,
            MValue::Number(_) => 2,
            MValue::Time(_) => 3,
            MValue::Date(_) => 4,
            MValue::DateTime(_) => 5,
            MValue::DateTimeZone(..) => 6,
            MValue::Duration(_) => 7,
            MValue::Text(_) => 8,
            _ => 9,
        }
    }
    order(a, b).unwrap_or_else(|| rank(a).cmp(&rank(b)))
}

/// A hashable form of a value for distinct, group and join keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Null,
    Bool(bool),
    Num(u64),
    Text(Rc<str>),
    Date(i64),
    Time(u64),
    Other(String),
}

pub fn key(v: &MValue) -> Key {
    let bits = |n: f64| if n == 0.0 { 0 } else { n.to_bits() };
    match v {
        MValue::Null => Key::Null,
        MValue::Logical(b) => Key::Bool(*b),
        MValue::Number(n) => Key::Num(bits(*n)),
        MValue::Text(t) => Key::Text(t.clone()),
        MValue::Date(d) => Key::Date(*d),
        MValue::DateTime(d) | MValue::Time(d) | MValue::Duration(d) | MValue::DateTimeZone(d, _) => Key::Time(bits(*d)),
        MValue::List(l) => Key::Other(format!("{:?}", l.iter().map(key).collect::<Vec<_>>())),
        MValue::Record(r) => Key::Other(format!("{:?}{:?}", r.names, r.values.iter().map(key).collect::<Vec<_>>())),
        other => Key::Other(other.kind().to_string()),
    }
}

pub fn cell_error(e: &CellError) -> MError {
    MError::format(format!("The cell holds the error {}.", e.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip_through_cells() {
        let d = days(2026, 10, 15).unwrap();
        let cell = MValue::Date(d).to_cell(DateSystem::D1900);
        assert_eq!(cell, Value::number(46_310.0));
        assert!(matches!(MValue::from_cell(&cell, ColType::Date, DateSystem::D1900), MValue::Date(x) if x == d));
        let c1904 = MValue::Date(d).to_cell(DateSystem::D1904);
        assert!(matches!(MValue::from_cell(&c1904, ColType::Date, DateSystem::D1904), MValue::Date(x) if x == d));
        assert_eq!(date_text(d, false), "10/15/2026");
        assert!(days(2026, 2, 29).is_err());
        assert!(days(0, 1, 1).is_err());
        assert_eq!(time_text(0.5), "12:00:00 PM");
        assert_eq!(duration_text(1.5), "1.12:00:00");
    }
}
