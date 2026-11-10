//! Runtime values for the DAX evaluator.
//!
//! Scalar values cover the DAX scalar types; tables are column-oriented metadata plus rows of
//! scalars. BLANK is a distinct value, never silently zero or the empty string.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// A DAX scalar value.
///
/// `Currency` and `DateTime` are plain numbers in DAX, so an untagged numeric representation
/// would read them back as `Integer`/`Decimal`. They serialize as tagged JSON objects instead:
/// `{"$currency": 1234}` (scaled by 10,000) and `{"$dateTime": 45292.5}`. Every other variant
/// keeps its natural JSON form, so existing model files and command output stay unchanged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scalar {
    /// BLANK. It serializes as `null` and is the first untagged match.
    Blank,
    Boolean(bool),
    Integer(i64),
    /// Currency is an integer scaled by 10,000.
    #[serde(with = "currency_serde")]
    Currency(i64),
    Decimal(f64),
    /// DateTime is an OLE Automation serial: whole days since 1899-12-30 plus a time fraction.
    #[serde(with = "datetime_serde")]
    DateTime(f64),
    String(String),
}

mod currency_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Tagged {
        #[serde(rename = "$currency")]
        value: i64,
    }

    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        Tagged { value: *value }.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        Ok(Tagged::deserialize(deserializer)?.value)
    }
}

mod datetime_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Tagged {
        #[serde(rename = "$dateTime")]
        value: f64,
    }

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        Tagged { value: *value }.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        Ok(Tagged::deserialize(deserializer)?.value)
    }
}

impl Scalar {
    pub fn is_blank(&self) -> bool {
        matches!(self, Scalar::Blank)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Scalar::Blank => "BLANK",
            Scalar::Boolean(_) => "Boolean",
            Scalar::Integer(_) => "Integer",
            Scalar::Currency(_) => "Currency",
            Scalar::Decimal(_) => "Decimal",
            Scalar::DateTime(_) => "DateTime",
            Scalar::String(_) => "String",
        }
    }

    /// DAX arithmetic coercion: BLANK is 0, booleans are 1/0, numeric text parses, dates are
    /// serials, and currency is divided by 10,000.
    pub fn to_number(&self) -> Option<f64> {
        match self {
            Scalar::Blank => Some(0.0),
            Scalar::Boolean(value) => Some(if *value { 1.0 } else { 0.0 }),
            Scalar::Integer(value) => Some(*value as f64),
            Scalar::Currency(value) => Some(*value as f64 / 10_000.0),
            Scalar::Decimal(value) | Scalar::DateTime(value) => Some(*value),
            // `f64::from_str` accepts "nan"/"inf"; DAX has no text form for those, so they are
            // not numbers here.
            Scalar::String(value) => value.trim().parse::<f64>().ok().filter(|number| number.is_finite()),
        }
    }

    /// DAX's implicit text conversion, used by `&` and the text functions: a whole-valued number
    /// prints without a decimal point (`10`, not `10.0`) and a date prints as its calendar text.
    /// [`Scalar::to_text`] stays the storage/debug spelling other callers rely on.
    pub fn to_dax_text(&self) -> String {
        match self {
            Scalar::Decimal(value) if value.is_finite() && value.fract() == 0.0 => {
                // DAX switches to exponent form for very large whole numbers instead of printing
                // every digit.
                if value.abs() >= 1.0e15 {
                    // Excel writes the exponent's sign: `1E+15`, not Rust's `1E15`.
                    let text = format!("{value:E}");
                    match text.split_once('E') {
                        Some((mantissa, exponent)) if !exponent.starts_with('-') => format!("{mantissa}E+{exponent}"),
                        _ => text,
                    }
                } else {
                    format!("{value:.0}")
                }
            }
            Scalar::Currency(value) => {
                // Integer arithmetic keeps every digit of the scaled range (about 19 significant
                // digits); an f64 round trip would lose the low ones.
                let sign = if *value < 0 { "-" } else { "" };
                let magnitude = value.unsigned_abs();
                let (whole, fraction) = (magnitude / 10_000, magnitude % 10_000);
                if fraction == 0 { format!("{sign}{whole}") } else { format!("{sign}{whole}.{}", format!("{fraction:04}").trim_end_matches('0')) }
            }
            Scalar::DateTime(serial) => datetime_text(*serial),
            // DAX prints at most 15 significant digits; Rust's shortest round-trip form can show
            // 17 (`0.30000000000000004`), so fractional decimals are rounded first.
            Scalar::Decimal(value) if value.is_finite() => {
                let rounded: f64 = format!("{value:.14e}").parse().unwrap_or(*value);
                rounded.to_string()
            }
            other => other.to_text(),
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Scalar::Blank => String::new(),
            Scalar::Boolean(true) => "TRUE".into(),
            Scalar::Boolean(false) => "FALSE".into(),
            Scalar::Integer(value) => value.to_string(),
            Scalar::Currency(value) => {
                // Integer arithmetic keeps every digit of the scaled range (about 19 significant
                // digits); an f64 round trip would lose the low ones. The four fractional digits
                // are the storage form, so they are kept as written.
                let sign = if *value < 0 { "-" } else { "" };
                let magnitude = value.unsigned_abs();
                format!("{sign}{}.{:04}", magnitude / 10_000, magnitude % 10_000)
            }
            Scalar::Decimal(value) => {
                if value.fract() == 0.0 {
                    format!("{value:.1}")
                } else {
                    value.to_string()
                }
            }
            Scalar::DateTime(value) => value.to_string(),
            Scalar::String(value) => value.clone(),
        }
    }

    pub fn to_bool(&self) -> Option<bool> {
        match self {
            Scalar::Blank => Some(false),
            Scalar::Boolean(value) => Some(*value),
            Scalar::Integer(value) => Some(*value != 0),
            Scalar::Currency(value) => Some(*value != 0),
            Scalar::Decimal(value) => Some(*value != 0.0),
            Scalar::DateTime(value) => Some(*value != 0.0),
            Scalar::String(value) if value.eq_ignore_ascii_case("true") => Some(true),
            Scalar::String(value) if value.eq_ignore_ascii_case("false") => Some(false),
            Scalar::String(_) => None,
        }
    }

    /// Comparison with DAX's blank coercion for `=`/ordering and case-insensitive strings.
    pub fn compare(&self, other: &Scalar) -> Option<Ordering> {
        match (self, other) {
            (Scalar::Blank, Scalar::Blank) => Some(Ordering::Equal),
            // BLANK converts to the empty string when compared with text, so `BLANK () = ""` is
            // TRUE and `BLANK () <> ""` is FALSE, symmetrically.
            (Scalar::Blank, Scalar::String(b)) => Some(String::new().cmp(&b.to_lowercase())),
            (Scalar::String(a), Scalar::Blank) => Some(a.to_lowercase().cmp(&String::new())),
            (Scalar::String(a), Scalar::String(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
            (Scalar::Boolean(a), Scalar::Boolean(b)) => Some(a.cmp(b)),
            _ => self.to_number()?.partial_cmp(&other.to_number()?),
        }
    }

    pub fn strict_eq(&self, other: &Scalar) -> bool {
        match (self, other) {
            (Scalar::Blank, Scalar::Blank) => true,
            (Scalar::String(a), Scalar::String(b)) => a == b,
            _ => self == other,
        }
    }
}

/// A comparable key used for filter context values.
///
/// Text compares case-insensitively, as DAX does: `"East"` and `"east"` are one key, so grouping
/// and filter maps do not split them. The original spelling is kept for output, and the first
/// spelling stored wins when equal keys collide.
#[derive(Clone, Debug)]
pub enum ScalarKey {
    Blank,
    Boolean(bool),
    Integer(i64),
    Currency(i64),
    Decimal(u64),
    DateTime(u64),
    String(String),
}

impl PartialEq for ScalarKey {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ScalarKey {}

impl PartialOrd for ScalarKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScalarKey {
    fn cmp(&self, other: &Self) -> Ordering {
        // The variant order is the derived one, so numeric keys keep sorting before text.
        fn rank(key: &ScalarKey) -> u8 {
            match key {
                ScalarKey::Blank => 0,
                ScalarKey::Boolean(_) => 1,
                ScalarKey::Integer(_) => 2,
                ScalarKey::Currency(_) => 3,
                ScalarKey::Decimal(_) => 4,
                ScalarKey::DateTime(_) => 5,
                ScalarKey::String(_) => 6,
            }
        }
        match (self, other) {
            (ScalarKey::String(left), ScalarKey::String(right)) => compare_text(left, right),
            (ScalarKey::Blank, ScalarKey::Blank) => Ordering::Equal,
            (ScalarKey::Boolean(left), ScalarKey::Boolean(right)) => left.cmp(right),
            (ScalarKey::Integer(left), ScalarKey::Integer(right)) => left.cmp(right),
            (ScalarKey::Currency(left), ScalarKey::Currency(right)) => left.cmp(right),
            (ScalarKey::Decimal(left), ScalarKey::Decimal(right)) => left.cmp(right),
            (ScalarKey::DateTime(left), ScalarKey::DateTime(right)) => left.cmp(right),
            _ => rank(self).cmp(&rank(other)),
        }
    }
}

/// Case-insensitive text order, matching how the engine compares text: ASCII folding first,
/// Unicode lowercasing only when the strings differ by more than ASCII case.
fn compare_text(left: &str, right: &str) -> Ordering {
    if left == right {
        return Ordering::Equal;
    }
    if left.is_ascii() && right.is_ascii() {
        return left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase());
    }
    left.to_lowercase().cmp(&right.to_lowercase())
}

impl From<&Scalar> for ScalarKey {
    fn from(value: &Scalar) -> Self {
        match value {
            Scalar::Blank => ScalarKey::Blank,
            Scalar::Boolean(value) => ScalarKey::Boolean(*value),
            Scalar::Integer(value) => ScalarKey::Integer(*value),
            Scalar::Currency(value) => ScalarKey::Currency(*value),
            Scalar::Decimal(value) => ScalarKey::Decimal(order_bits(*value)),
            Scalar::DateTime(value) => ScalarKey::DateTime(order_bits(*value)),
            Scalar::String(value) => ScalarKey::String(value.clone()),
        }
    }
}

/// Maps an `f64` to a `u64` whose unsigned order matches numeric order. `-0.0` and `0.0` map to
/// the same key, so `Ord` on [`ScalarKey`] sorts pivot groups numerically.
fn order_bits(value: f64) -> u64 {
    let value = if value == 0.0 { 0.0 } else { value };
    let bits = value.to_bits();
    if bits >> 63 == 1 { !bits } else { bits | (1 << 63) }
}

/// Inverse of [`order_bits`].
pub(crate) fn bits_order(bits: u64) -> f64 {
    let bits = if bits >> 63 == 1 { bits & !(1 << 63) } else { !bits };
    f64::from_bits(bits)
}

impl ScalarKey {
    /// Rebuilds the scalar that produced this key.
    pub fn to_scalar(&self) -> Scalar {
        match self {
            ScalarKey::Blank => Scalar::Blank,
            ScalarKey::Boolean(value) => Scalar::Boolean(*value),
            ScalarKey::Integer(value) => Scalar::Integer(*value),
            ScalarKey::Currency(value) => Scalar::Currency(*value),
            ScalarKey::Decimal(value) => Scalar::Decimal(bits_order(*value)),
            ScalarKey::DateTime(value) => Scalar::DateTime(bits_order(*value)),
            ScalarKey::String(value) => Scalar::String(value.clone()),
        }
    }
}

/// A `DateTime` serial as DAX prints it: `M/D/YYYY H:MM:SS AM/PM`.
fn datetime_text(serial: f64) -> String {
    use chrono::Datelike;
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30);
    let Some(epoch) = epoch else { return serial.to_string() };
    let mut seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    let mut days = serial.floor() as i64;
    if seconds >= 86_400 {
        seconds -= 86_400;
        days = days.saturating_add(1);
    }
    let Some(date) = chrono::Duration::try_days(days).and_then(|offset| epoch.checked_add_signed(offset)) else {
        return serial.to_string();
    };
    let (hour, minute, second) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    let meridiem = if hour < 12 { "AM" } else { "PM" };
    format!("{}/{}/{} {}:{minute:02}:{second:02} {meridiem}", date.month(), date.day(), date.year(), hour12)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableColumn {
    pub name: String,
}

/// A materialized table value.
#[derive(Clone, Debug, PartialEq)]
pub struct TableValue {
    pub columns: Vec<TableColumn>,
    pub rows: Vec<Vec<Scalar>>,
    /// The model table this value came from, when known. It lets row-context evaluation avoid
    /// guessing from column names.
    pub lineage: Option<usize>,
}

impl TableValue {
    pub fn new(columns: Vec<TableColumn>, rows: Vec<Vec<Scalar>>) -> Self {
        TableValue { columns, rows, lineage: None }
    }

    pub fn with_lineage(mut self, lineage: usize) -> Self {
        self.lineage = Some(lineage);
        self
    }

    /// Keeps the existing lineage when the input had one.
    pub fn with_lineage_if(mut self, lineage: Option<usize>) -> Self {
        self.lineage = lineage;
        self
    }

    /// Builds a table from rows whose column names are given.
    pub fn from_rows(columns: Vec<TableColumn>, rows: Vec<Vec<Scalar>>, lineage: Option<usize>) -> Self {
        TableValue { columns, rows, lineage }
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name))
    }

    pub fn column(&self, index: usize) -> Vec<Scalar> {
        self.rows.iter().filter_map(|row| row.get(index).cloned()).collect()
    }

    pub fn distinct_rows(&self) -> Self {
        let mut seen = BTreeSet::new();
        let mut rows = Vec::new();
        for row in &self.rows {
            let key: Vec<ScalarKey> = row.iter().map(ScalarKey::from).collect();
            if seen.insert(key) {
                rows.push(row.clone());
            }
        }
        TableValue { columns: self.columns.clone(), rows, lineage: self.lineage }
    }
}

/// A runtime evaluation error.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{message}")]
pub struct EvalError {
    pub message: String,
}

impl EvalError {
    pub fn new(message: impl Into<String>) -> Self {
        EvalError { message: message.into() }
    }

    pub fn not_implemented(name: &str) -> Self {
        EvalError::new(format!("DAX function `{name}` is not implemented by this evaluator"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `to_text` must print every digit of a large scaled currency, which an `f64` division drops
    /// for values above about 9e11 (the documented DAX currency range reaches about 9.2e14).
    #[test]
    fn currency_to_text_keeps_the_low_digits() {
        assert_eq!(Scalar::Currency(123_456_789_012_345_678).to_text(), "12345678901234.5678");
        assert_eq!(Scalar::Currency(2_000_000_000_000_001).to_text(), "200000000000.0001");
        assert_eq!(Scalar::Currency(-123_456_789_012_345_678).to_text(), "-12345678901234.5678");
        assert_eq!(Scalar::Currency(i64::MIN).to_text(), "-922337203685477.5808");
        assert_eq!(Scalar::Currency(i64::MAX).to_text(), "922337203685477.5807");
    }

    /// The four-decimal storage form is preserved, including trailing zeros and small values.
    #[test]
    fn currency_to_text_keeps_four_fractional_digits() {
        assert_eq!(Scalar::Currency(1).to_text(), "0.0001");
        assert_eq!(Scalar::Currency(10_000).to_text(), "1.0000");
        assert_eq!(Scalar::Currency(0).to_text(), "0.0000");
        assert_eq!(Scalar::Currency(-1).to_text(), "-0.0001");
    }
}
