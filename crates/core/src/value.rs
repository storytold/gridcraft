//! Cell values and Excel's coercion and comparison rules.

use std::cmp::Ordering;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// Excel's error values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CellError {
    Null,
    Div0,
    Value,
    Ref,
    Name,
    Num,
    NA,
    GettingData,
    Spill,
    Calc,
    /// Circular reference that could not be resolved (shown as 0 by Excel; we keep it explicit).
    Circ,
}

impl CellError {
    pub const ALL: [CellError; 10] = [
        CellError::Null,
        CellError::Div0,
        CellError::Value,
        CellError::Ref,
        CellError::Name,
        CellError::Num,
        CellError::NA,
        CellError::GettingData,
        CellError::Spill,
        CellError::Calc,
    ];
    pub fn as_str(&self) -> &'static str {
        match self {
            CellError::Null => "#NULL!",
            CellError::Div0 => "#DIV/0!",
            CellError::Value => "#VALUE!",
            CellError::Ref => "#REF!",
            CellError::Name => "#NAME?",
            CellError::Num => "#NUM!",
            CellError::NA => "#N/A",
            CellError::GettingData => "#GETTING_DATA",
            CellError::Spill => "#SPILL!",
            CellError::Calc => "#CALC!",
            CellError::Circ => "#CIRC!",
        }
    }
    /// `ERROR.TYPE` number.
    pub fn code(&self) -> u32 {
        match self {
            CellError::Null => 1,
            CellError::Div0 => 2,
            CellError::Value => 3,
            CellError::Ref => 4,
            CellError::Name => 5,
            CellError::Num => 6,
            CellError::NA => 7,
            CellError::GettingData => 8,
            CellError::Spill => 9,
            CellError::Calc => 14,
            CellError::Circ => 15,
        }
    }
    pub fn parse(s: &str) -> Option<CellError> {
        let u = s.to_ascii_uppercase();
        CellError::ALL.iter().chain([CellError::Circ].iter()).find(|e| e.as_str() == u).copied()
    }
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A 2-D array of values (row-major). Always at least 1×1.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Array {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<Value>,
}

impl Array {
    pub fn new(rows: usize, cols: usize, data: Vec<Value>) -> Option<Array> {
        if rows == 0 || cols == 0 || rows.checked_mul(cols)? != data.len() {
            return None;
        }
        Some(Array { rows, cols, data })
    }
    pub fn filled(rows: usize, cols: usize, v: Value) -> Array {
        let (rows, cols) = (rows.max(1), cols.max(1));
        Array { rows, cols, data: vec![v; rows.saturating_mul(cols)] }
    }
    pub fn scalar(v: Value) -> Array {
        Array { rows: 1, cols: 1, data: vec![v] }
    }
    pub fn column(v: Vec<Value>) -> Array {
        if v.is_empty() {
            return Array::scalar(Value::Error(CellError::Calc));
        }
        Array { rows: v.len(), cols: 1, data: v }
    }
    pub fn row(v: Vec<Value>) -> Array {
        if v.is_empty() {
            return Array::scalar(Value::Error(CellError::Calc));
        }
        Array { rows: 1, cols: v.len(), data: v }
    }
    pub fn get(&self, r: usize, c: usize) -> Option<&Value> {
        if r >= self.rows || c >= self.cols {
            return None;
        }
        self.data.get(r * self.cols + c)
    }
    /// Excel's broadcasting read: a 1-row or 1-column array repeats along that axis; outside the
    /// array is `#N/A`.
    pub fn get_broadcast(&self, r: usize, c: usize) -> Value {
        let rr = if self.rows == 1 { 0 } else { r };
        let cc = if self.cols == 1 { 0 } else { c };
        self.get(rr, cc).cloned().unwrap_or(Value::Error(CellError::NA))
    }
    pub fn transpose(&self) -> Array {
        let mut data = Vec::with_capacity(self.data.len());
        for c in 0..self.cols {
            for r in 0..self.rows {
                data.push(self.get(r, c).cloned().unwrap_or(Value::Empty));
            }
        }
        Array { rows: self.cols, cols: self.rows, data }
    }
    pub fn iter(&self) -> impl Iterator<Item = &Value> {
        self.data.iter()
    }
}

/// A cell value or an intermediate formula result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "t", content = "v")]
pub enum Value {
    #[default]
    Empty,
    Number(f64),
    Text(Arc<str>),
    Bool(bool),
    Error(CellError),
    Array(Arc<Array>),
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::number(v)
    }
}
impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(Arc::from(v))
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(Arc::from(v))
    }
}
impl From<CellError> for Value {
    fn from(v: CellError) -> Self {
        Value::Error(v)
    }
}
impl From<Array> for Value {
    fn from(a: Array) -> Self {
        Value::Array(Arc::new(a))
    }
}

impl Value {
    /// A number, or `#NUM!` when it is NaN or infinite (Excel never stores those).
    pub fn number(v: f64) -> Value {
        if v.is_finite() { Value::Number(if v == 0.0 { 0.0 } else { v }) } else { Value::Error(CellError::Num) }
    }
    pub fn text(s: impl Into<Arc<str>>) -> Value {
        Value::Text(s.into())
    }
    pub fn is_empty(&self) -> bool {
        matches!(self, Value::Empty)
    }
    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Number(_))
    }
    pub fn is_text(&self) -> bool {
        matches!(self, Value::Text(_))
    }
    pub fn as_error(&self) -> Option<CellError> {
        if let Value::Error(e) = self { Some(*e) } else { None }
    }
    pub fn as_f64(&self) -> Option<f64> {
        if let Value::Number(n) = self { Some(*n) } else { None }
    }
    pub fn as_text(&self) -> Option<&str> {
        if let Value::Text(t) = self { Some(t) } else { None }
    }
    /// Top-left element of an array, the value itself otherwise.
    pub fn scalar(&self) -> Value {
        match self {
            Value::Array(a) => a.data.first().cloned().unwrap_or(Value::Empty),
            v => v.clone(),
        }
    }

    /// Coercion to a number as arithmetic does it: empty = 0, TRUE = 1, numeric text parses
    /// (including dates, times, percentages and currency), other text = `#VALUE!`.
    pub fn to_number(&self) -> Result<f64, CellError> {
        self.to_number_in(crate::DateSystem::D1900)
    }
    /// [`Value::to_number`] with date text read in the given workbook date system.
    pub fn to_number_in(&self, sys: crate::DateSystem) -> Result<f64, CellError> {
        match self {
            Value::Empty => Ok(0.0),
            Value::Number(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(t) => crate::parse::parse_number_text_in(t, sys).ok_or(CellError::Value),
            Value::Error(e) => Err(*e),
            Value::Array(a) => a.data.first().map(|v| v.to_number_in(sys)).unwrap_or(Ok(0.0)),
        }
    }
    /// Coercion to text as `&` does it.
    pub fn to_text(&self) -> Result<String, CellError> {
        match self {
            Value::Empty => Ok(String::new()),
            Value::Number(n) => Ok(number_to_text(*n)),
            Value::Bool(b) => Ok(if *b { "TRUE".into() } else { "FALSE".into() }),
            Value::Text(t) => Ok(t.to_string()),
            Value::Error(e) => Err(*e),
            Value::Array(a) => a.data.first().map(|v| v.to_text()).unwrap_or(Ok(String::new())),
        }
    }
    /// Coercion to a boolean as `IF` does it.
    pub fn to_bool(&self) -> Result<bool, CellError> {
        match self {
            Value::Empty => Ok(false),
            Value::Number(n) => Ok(*n != 0.0),
            Value::Bool(b) => Ok(*b),
            Value::Text(t) => {
                if t.eq_ignore_ascii_case("TRUE") {
                    Ok(true)
                } else if t.eq_ignore_ascii_case("FALSE") {
                    Ok(false)
                } else {
                    Err(CellError::Value)
                }
            }
            Value::Error(e) => Err(*e),
            Value::Array(a) => a.data.first().map(|v| v.to_bool()).unwrap_or(Ok(false)),
        }
    }
    /// Display text with General formatting (for logs, CSV and tests).
    pub fn display(&self) -> String {
        match self {
            Value::Error(e) => e.as_str().to_string(),
            v => v.to_text().unwrap_or_default(),
        }
    }
    /// Type rank for comparison and sorting: numbers < text < booleans < errors; empty sorts last.
    fn rank(&self) -> u8 {
        match self {
            Value::Number(_) => 0,
            Value::Text(_) => 1,
            Value::Bool(_) => 2,
            Value::Error(_) => 3,
            Value::Empty => 4,
            Value::Array(_) => 5,
        }
    }
}

/// Excel's comparison for `=`, `<`… operators. Text compares case-insensitively; empty acts as
/// 0, "" or FALSE depending on the other side. Errors are handled by the caller.
pub fn compare(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Empty, Value::Empty) => Ordering::Equal,
        (Value::Empty, Value::Number(n)) => compare_numbers(0.0, *n),
        (Value::Number(n), Value::Empty) => compare_numbers(*n, 0.0),
        (Value::Empty, Value::Text(t)) => "".cmp(&t.to_lowercase() as &str),
        (Value::Text(t), Value::Empty) => (t.to_lowercase() as String).as_str().cmp(""),
        (Value::Empty, Value::Bool(b)) => false.cmp(b),
        (Value::Bool(b), Value::Empty) => b.cmp(&false),
        (Value::Number(x), Value::Number(y)) => compare_numbers(*x, *y),
        (Value::Text(x), Value::Text(y)) => compare_text(x, y),
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Error(x), Value::Error(y)) => x.code().cmp(&y.code()),
        _ => a.rank().cmp(&b.rank()),
    }
}

/// Compares numbers the way Excel does: both are rounded to 15 significant digits first, so binary
/// noise such as `0.1+0.2` versus `0.3` counts as equal.
pub fn compare_numbers(a: f64, b: f64) -> Ordering {
    fn round15(x: f64) -> f64 {
        if !x.is_finite() || x == 0.0 { x } else { format!("{x:.14e}").parse().unwrap_or(x) }
    }
    if a == b {
        return Ordering::Equal;
    }
    // Numbers further apart than the 15th digit can't round together, and rounding keeps their
    // order: skip the text round trip (this is on the sort and lookup hot path).
    if (a - b).abs() > a.abs().max(b.abs()) * 1e-13 {
        return a.partial_cmp(&b).unwrap_or(Ordering::Equal);
    }
    round15(a).partial_cmp(&round15(b)).unwrap_or(Ordering::Equal)
}

/// Case-insensitive text order (Unicode lowercase).
pub fn compare_text(a: &str, b: &str) -> Ordering {
    let mut ai = a.chars().flat_map(char::to_lowercase);
    let mut bi = b.chars().flat_map(char::to_lowercase);
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x != y => return x.cmp(&y),
            _ => {}
        }
    }
}

/// Sorting order used by Sort (ascending): numbers, text, booleans, errors, then blanks last.
pub fn sort_compare(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Empty, Value::Empty) => Ordering::Equal,
        (Value::Empty, _) => Ordering::Greater,
        (_, Value::Empty) => Ordering::Less,
        (Value::Error(_), Value::Error(_)) => Ordering::Equal,
        _ => compare(a, b),
    }
}

/// Number → text the way Excel converts in `&`, CONCAT and TEXTJOIN: at most 15 significant
/// digits, rounded first; scientific from 1E+15, below 1E-9, and wherever the decimal would be
/// longer than 21 characters, not counting the sign (`0.` plus 19 decimals). Excel-verified only
/// for positive 15-digit mantissas: `0.0000692361330972509` stays decimal and
/// `1.28233995888454E-06` does not, as Excel caches them. The limit for shorter mantissas from
/// 1E-6 to 1E-9 (e.g. `0.0000099999999999999` decimal) and for negatives (sign excluded) are
/// inferred.
pub fn number_to_text(n: f64) -> String {
    if n == 0.0 || !n.is_finite() {
        return "0".into();
    }
    // Round to 15 significant digits first, then pick decimal or scientific from the rounded
    // magnitude, so 999999999999999.5 reads `1E+15`, never a 16-digit integer.
    let sci = format!("{:.14e}", n);
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let mag: i32 = exp.parse().unwrap_or(0);
    let sig = mant.bytes().filter(u8::is_ascii_digit).count() - mant.bytes().rev().take_while(|&b| b == b'0').count();
    // Characters after `0.` for |n| < 1: the leading zeros, then the significant digits.
    let decimals_below_one = -mag - 1 + sig as i32;
    if !(-9..15).contains(&mag) || decimals_below_one > 19 {
        return sci_text(n, 15);
    }
    // 15 significant digits, trailing zeros trimmed.
    let decimals = (14 - mag).clamp(0, 30) as usize;
    let s = format!("{:.*}", decimals, n);
    trim_decimal(&s)
}

/// Removes trailing zeros (and a trailing point) from a decimal string.
pub fn trim_decimal(s: &str) -> String {
    if s.contains('.') {
        let t = s.trim_end_matches('0').trim_end_matches('.');
        if t == "-0" { "0".into() } else { t.to_string() }
    } else {
        s.to_string()
    }
}

/// Scientific text like `1.23456789012346E+15` with up to `sig` significant digits.
pub fn sci_text(n: f64, sig: usize) -> String {
    let s = format!("{:.*e}", sig.saturating_sub(1), n);
    let Some((mant, exp)) = s.split_once('e') else { return s };
    let mant = trim_decimal(mant);
    let e: i32 = exp.parse().unwrap_or(0);
    format!("{}E{}{:02}", mant, if e < 0 { '-' } else { '+' }, e.abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_text() {
        assert_eq!(number_to_text(1.0), "1");
        assert_eq!(number_to_text(-2.5), "-2.5");
        assert_eq!(number_to_text(0.1 + 0.2), "0.3");
        assert_eq!(number_to_text(123456789012345.0), "123456789012345");
        assert_eq!(number_to_text(1e15), "1E+15");
        assert_eq!(number_to_text(1234567890123456.0), "1.23456789012346E+15");
        assert_eq!(number_to_text(0.0001), "0.0001");
        assert_eq!(number_to_text(1e-10), "1E-10");
        assert_eq!(number_to_text(1e15 - 0.5), "1E+15");
        assert_eq!(number_to_text(1e15 - 0.1), "1E+15");
        assert_eq!(number_to_text(-(1e15 - 0.5)), "-1E+15");
        assert_eq!(number_to_text(999999999999999.0), "999999999999999");
        assert_eq!(number_to_text(9.999999999999999e-10), "0.000000001");
        assert_eq!(number_to_text(-9.999999999999999e-10), "-0.000000001");
        assert_eq!(number_to_text(1.0 / 3.0), "0.333333333333333");
    }

    /// Excel switches to scientific when the 15-digit decimal would be longer than 21 characters
    /// (`0.` plus 19 decimals). Excel-verified: IMTAN, IMSECH, IMCSCH and IMEXP results cached by
    /// Excel for Mac in the `formulas` project's test.xlsx (ENGINEERING!S53, R47, S37, V37, V39).
    #[test]
    fn number_text_small_magnitudes() {
        assert_eq!(number_to_text(6.92361330972509e-5), "0.0000692361330972509");
        assert_eq!(number_to_text(2.41377759972715e-4), "0.000241377759972715");
        assert_eq!(number_to_text(1.28233995888454e-6), "1.28233995888454E-06");
        assert_eq!(number_to_text(-4.48088972789593e-6), "-4.48088972789593E-06");
        assert_eq!(number_to_text(5.98317105631764e-7), "5.98317105631764E-07");
        assert_eq!(number_to_text(-2.99158552811404e-7), "-2.99158552811404E-07");
        // Just below powers of ten: the 21-character limit, and the rounded magnitude.
        assert_eq!(number_to_text(9.99999999999999e-5), "0.0000999999999999999");
        assert_eq!(number_to_text(9.99999999999999e-6), "9.99999999999999E-06");
        assert_eq!(number_to_text(9.9999999999999e-6), "0.0000099999999999999");
        assert_eq!(number_to_text(9.99999999999999e-9), "9.99999999999999E-09");
        assert_eq!(number_to_text(-9.99999999999999e-9), "-9.99999999999999E-09");
        assert_eq!(number_to_text(9.99999999999999e-10), "9.99999999999999E-10");
        assert_eq!(number_to_text(9.999999999999999e-6), "0.00001");
        assert_eq!(number_to_text(0.00001), "0.00001");
        assert_eq!(number_to_text(0.000001), "0.000001");
        assert_eq!(number_to_text(1.5e-9), "0.0000000015");
        // Large side: 15 significant digits, scientific from 1E+15.
        assert_eq!(number_to_text(99999999999999.9), "99999999999999.9");
        assert_eq!(number_to_text(999999999999999.4), "999999999999999");
        assert_eq!(number_to_text(9999999999999998.0), "1E+16");
        assert_eq!(number_to_text(9.99999999999999e15), "9.99999999999999E+15");
    }

    #[test]
    fn coercion() {
        assert_eq!(Value::from("12").to_number(), Ok(12.0));
        assert_eq!(Value::from(" 1,234.5 ").to_number(), Ok(1234.5));
        assert_eq!(Value::from("50%").to_number(), Ok(0.5));
        assert_eq!(Value::from("abc").to_number(), Err(CellError::Value));
        assert_eq!(Value::Bool(true).to_number(), Ok(1.0));
        assert_eq!(Value::Empty.to_text(), Ok(String::new()));
        assert_eq!(Value::from("true").to_bool(), Ok(true));
        assert!(Value::number(f64::NAN).is_error());
    }

    #[test]
    fn comparisons() {
        assert_eq!(compare(&Value::from("abc"), &Value::from("ABC")), Ordering::Equal);
        assert_eq!(compare(&Value::Number(5.0), &Value::from("1")), Ordering::Less);
        assert_eq!(compare(&Value::from("z"), &Value::Bool(false)), Ordering::Less);
        assert_eq!(compare(&Value::Empty, &Value::Number(0.0)), Ordering::Equal);
        assert_eq!(sort_compare(&Value::Empty, &Value::Number(1.0)), Ordering::Greater);
    }

    #[test]
    fn errors() {
        for e in CellError::ALL {
            assert_eq!(CellError::parse(e.as_str()), Some(e));
        }
        assert_eq!(CellError::parse("#div/0!"), Some(CellError::Div0));
    }

    #[test]
    fn numbers_compare_at_15_significant_digits() {
        let num = Value::Number;
        assert_eq!(compare(&num(0.1 + 0.2), &num(0.3)), Ordering::Equal);
        assert_eq!(compare(&num(1.1 * 3.0), &num(3.3)), Ordering::Equal);
        assert_eq!(compare(&num(1.0), &num(1.0 + 1e-16)), Ordering::Equal);
        assert_ne!(compare(&num(0.1 + 0.2), &num(0.3)), Ordering::Greater);
        assert_eq!(compare(&num(1.0), &num(1.00000000000001)), Ordering::Less);
        assert_eq!(compare(&num(1.0), &num(2.0)), Ordering::Less);
    }

    #[test]
    fn arrays() {
        let a = Array::new(2, 2, vec![1.0.into(), 2.0.into(), 3.0.into(), 4.0.into()]).unwrap();
        assert_eq!(a.transpose().get(0, 1), Some(&Value::Number(3.0)));
        assert!(Array::new(2, 2, vec![]).is_none());
        let row = Array::row(vec![1.0.into(), 2.0.into()]);
        assert_eq!(row.get_broadcast(5, 1), Value::Number(2.0));
    }
}
