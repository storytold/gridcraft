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
    /// A decimal. A finite value is a JSON number; `NaN`/`Infinity` become `{"$decimal": "NaN"}`
    /// so they survive a round trip (`serde_json` writes a non-finite float as `null`, which would
    /// read back as BLANK).
    #[serde(with = "decimal_serde")]
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

mod decimals {
    /// The JSON text of a non-finite value, or `None` when it is finite.
    pub fn to_text(value: f64) -> Option<&'static str> {
        if value.is_nan() {
            Some("NaN")
        } else if value == f64::INFINITY {
            Some("Infinity")
        } else if value == f64::NEG_INFINITY {
            Some("-Infinity")
        } else {
            None
        }
    }

    /// The value of a JSON text produced by [`to_text`].
    pub fn from_text(text: &str) -> Option<f64> {
        match text {
            "NaN" => Some(f64::NAN),
            "Infinity" => Some(f64::INFINITY),
            "-Infinity" => Some(f64::NEG_INFINITY),
            _ => None,
        }
    }
}

mod decimal_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::decimals;

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Json {
        Number(f64),
        Tagged {
            #[serde(rename = "$decimal")]
            text: String,
        },
    }

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        match decimals::to_text(*value) {
            Some(text) => Json::Tagged { text: text.to_string() }.serialize(serializer),
            None => Json::Number(*value).serialize(serializer),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        match Json::deserialize(deserializer)? {
            Json::Number(value) => Ok(value),
            Json::Tagged { text } => decimals::from_text(&text).ok_or_else(|| serde::de::Error::custom("unknown non-finite decimal")),
        }
    }
}

mod datetime_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::decimals;

    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum Number {
        Finite(f64),
        Text(String),
    }

    #[derive(Serialize, Deserialize)]
    struct Tagged {
        #[serde(rename = "$dateTime")]
        value: Number,
    }

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        let value = match decimals::to_text(*value) {
            Some(text) => Number::Text(text.to_string()),
            None => Number::Finite(*value),
        };
        Tagged { value }.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        match Tagged::deserialize(deserializer)?.value {
            Number::Finite(value) => Ok(value),
            Number::Text(text) => decimals::from_text(&text).ok_or_else(|| serde::de::Error::custom("unknown non-finite serial")),
        }
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

    /// Whether the value is one of the four numeric variants, without coercing anything else.
    pub fn is_numeric(&self) -> bool {
        matches!(self, Scalar::Integer(_) | Scalar::Currency(_) | Scalar::Decimal(_) | Scalar::DateTime(_))
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
                // DAX prints zero as `0`, never `-0`: ordinary arithmetic (`ROUND ( -0.4, 0 )`,
                // `0 * -1`) produces a negative zero, and its sign must not reach the text.
                let value = if *value == 0.0 { 0.0 } else { *value };
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
            Scalar::Currency(value) => currency_text(*value, false),
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

    /// The storage/debug spelling: currency keeps all four fraction digits, where
    /// [`Scalar::to_dax_text`] prints DAX's shortened form. Both come from the same exact split.
    pub fn to_text(&self) -> String {
        match self {
            Scalar::Blank => String::new(),
            Scalar::Boolean(true) => "TRUE".into(),
            Scalar::Boolean(false) => "FALSE".into(),
            Scalar::Integer(value) => value.to_string(),
            Scalar::Currency(value) => currency_text(*value, true),
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
    ///
    /// `None` means the types are not comparable: text against a number or a logical, and any
    /// pair with NaN. Text is never coerced to a number here — arithmetic coerces numeric text
    /// (`to_number`), comparison does not — so the caller decides what an incomparable pair means
    /// (the evaluator maps it to FALSE for `=`/ordering and to an error where DAX raises one).
    pub fn compare(&self, other: &Scalar) -> Option<Ordering> {
        match (self, other) {
            (Scalar::Blank, Scalar::Blank) => Some(Ordering::Equal),
            // BLANK converts to the empty string when compared with text, so `BLANK () = ""` is
            // TRUE and `BLANK () <> ""` is FALSE, symmetrically.
            (Scalar::Blank, Scalar::String(b)) => Some(String::new().cmp(&b.to_lowercase())),
            (Scalar::String(a), Scalar::Blank) => Some(a.to_lowercase().cmp(&String::new())),
            (Scalar::String(a), Scalar::String(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
            (Scalar::String(_), _) | (_, Scalar::String(_)) => None,
            (Scalar::Boolean(a), Scalar::Boolean(b)) => Some(a.cmp(b)),
            // DAX does not compare a logical value with a number: `TRUE() = 1` and the `IN`,
            // `SWITCH` and `MIN`/`MAX` paths built on this ordering treat them as incomparable.
            // BLANK is not numeric, so the BLANK-converts-to-FALSE rule above is unaffected.
            (Scalar::Boolean(_), other) | (other, Scalar::Boolean(_)) if other.is_numeric() => None,
            // Two keys of the same integer variant compare exactly: above 2^53 the `f64` route
            // would report distinct values as equal, and `IN`/`=` must not.
            (Scalar::Integer(a), Scalar::Integer(b)) => Some(a.cmp(b)),
            (Scalar::Currency(a), Scalar::Currency(b)) => Some(a.cmp(b)),
            _ => numeric_compare(self, other),
        }
    }

    /// Identity for relationship joins and member filters: BLANK equals only BLANK and text is
    /// case-sensitive, while the numeric variants compare by value — a key can be `Integer` on one
    /// side and `Decimal` or `Currency` on the other, and the two must still join.
    pub fn strict_eq(&self, other: &Scalar) -> bool {
        match (self, other) {
            (Scalar::Blank, Scalar::Blank) => true,
            (Scalar::String(a), Scalar::String(b)) => a == b,
            (a, b) if a.is_numeric() && b.is_numeric() => {
                // Two keys of the same variant compare exactly, however large they are.
                if std::mem::discriminant(a) == std::mem::discriminant(b) {
                    return a == b;
                }
                match (exact_number(a), exact_number(b)) {
                    (Some(x), Some(y)) => x == y,
                    _ => false,
                }
            }
            _ => self == other,
        }
    }
}

/// A comparable/hashable key used for filter context values, relationship joins and dictionary
/// lookups.
///
/// Keys keep the exact spelling: they double as the reported key in integrity findings. Where DAX
/// compares values for *distinctness* instead of identity — `DISTINCTCOUNT`, `VALUES`/`DISTINCT`
/// — the text folds through [`distinct_key`], which is the model's case-insensitive collation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScalarKey {
    Blank,
    Boolean(bool),
    Integer(i64),
    Currency(i64),
    Decimal(u64),
    DateTime(u64),
    String(String),
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

/// Compares two numeric scalars.
///
/// An `Integer` against a `Decimal`/`DateTime` is compared through `i128` when the decimal is
/// integral, so a value above 2^53 is not rounded into its neighbour; every other pair uses the
/// `f64` coercion DAX applies.
fn numeric_compare(left: &Scalar, right: &Scalar) -> Option<Ordering> {
    fn exact(value: &Scalar) -> Option<i128> {
        match value {
            Scalar::Integer(number) => Some(i128::from(*number)),
            Scalar::Decimal(number) | Scalar::DateTime(number) if number.fract() == 0.0 && number.abs() < 9.2e18 => Some(*number as i128),
            _ => None,
        }
    }
    let integer_involved = matches!(left, Scalar::Integer(_) | Scalar::Currency(_)) || matches!(right, Scalar::Integer(_) | Scalar::Currency(_));
    if integer_involved && let (Some(a), Some(b)) = (exact(left), exact(right)) {
        return Some(a.cmp(&b));
    }
    left.to_number()?.partial_cmp(&right.to_number()?)
}

/// [`relationship_key_of`] for a value: the key a relationship index stores for it under the
/// numeric rule.
pub(crate) fn relationship_key_of_value(value: &Scalar) -> ScalarKey {
    relationship_key_of(&ScalarKey::from(value))
}

/// [`relationship_key`] for a key that is already normalized-except-for-variant.
pub(crate) fn relationship_key_of(key: &ScalarKey) -> ScalarKey {
    match key {
        ScalarKey::Integer(number) if number.unsigned_abs() <= (1 << 53) => ScalarKey::Decimal(order_bits(*number as f64)),
        ScalarKey::Currency(number) if number.unsigned_abs() <= (1 << 53) => {
            let major = *number as f64 / 10_000.0;
            // The division must land back on the stored amount. Past roughly 2^49 hundredths it
            // rounds, and two distinct currencies would then share one key.
            if (major * 10_000.0).round() as i64 == *number { ScalarKey::Decimal(order_bits(major)) } else { key.clone() }
        }
        // A NaN serial has no ordering and never joins, so it keeps its own variant instead of
        // folding onto a `Decimal` NaN with the same bits.
        ScalarKey::DateTime(serial) if !bits_order(*serial).is_nan() => ScalarKey::Decimal(*serial),
        other => other.clone(),
    }
}

/// Intersects two key sets under [`folded_key`], keeping the left-hand representatives.
///
/// Two filters that allow `"East"` and `"east"` accept the same rows, so their intersection must
/// not be empty; the same holds for `Integer(2)` and `Decimal(2.0)`.
pub(crate) fn fold_intersect(left: &BTreeSet<ScalarKey>, right: &BTreeSet<ScalarKey>) -> BTreeSet<ScalarKey> {
    let right: BTreeSet<ScalarKey> = right.iter().map(folded_key).collect();
    left.iter().filter(|key| right.contains(&folded_key(key))).cloned().collect()
}

/// Whether a key holds a NaN value, which never joins and is never a relationship key.
pub(crate) fn is_nan_key(key: &ScalarKey) -> bool {
    match key {
        ScalarKey::Decimal(bits) | ScalarKey::DateTime(bits) => bits_order(*bits).is_nan(),
        _ => false,
    }
}

/// The same key with text folded to lower case: the rule filter keys and the values they are
/// compared with share, so a filter set from `VALUES` matches every spelling of the value.
pub(crate) fn folded_key(key: &ScalarKey) -> ScalarKey {
    match key {
        ScalarKey::String(text) => ScalarKey::String(text.to_lowercase()),
        // Numbers fold to one variant too, so a filter key built from `Integer(3)` matches a
        // column cell stored as `Decimal(3.0)` (a promoted column) and vice versa.
        other => relationship_key_of(other),
    }
}

/// The key a distinctness comparison uses: text folds to lower case, the model collation, so `"A"`
/// and `"a"` are one value. Every other type keeps its exact key.
///
/// `DISTINCTCOUNT`, `DISTINCTCOUNTNOBLANK`, `VALUES`/`DISTINCT` and the storage dictionary all use
/// this, so a faithful model and a row-scanned one count the same values.
pub(crate) fn distinct_key(value: &Scalar) -> ScalarKey {
    // Text folds by case and numbers fold by value: a promoted column can hold `Integer(2)` and
    // `Decimal(2.0)` in the same cell family, and DAX counts that as one value. The relationship
    // rule keeps the conversions exact (and NaN distinct).
    folded_key(&ScalarKey::from(value))
}

/// The numeric value of a variant that an `f64` holds exactly.
///
/// `Integer` and `Currency` are compared through their exact representation only up to 2^53:
/// beyond it `9007199254740992` and `9007199254740993` share an `f64`, and a relationship must not
/// treat two distinct keys as the same one.
fn exact_number(value: &Scalar) -> Option<f64> {
    const LIMIT: i64 = 1 << 53;
    match value {
        Scalar::Integer(number) if number.unsigned_abs() <= LIMIT as u64 => Some(*number as f64),
        Scalar::Currency(number) if number.unsigned_abs() <= LIMIT as u64 => {
            let major = *number as f64 / 10_000.0;
            // The same round-trip rule the relationship key uses, so `strict_eq` and the join
            // indexes cannot disagree about an amount that does not survive the division.
            if (major * 10_000.0).round() as i64 == *number { Some(major) } else { None }
        }
        Scalar::Decimal(number) | Scalar::DateTime(number) => Some(*number),
        _ => None,
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

/// A `Currency` value — an integer scaled by 10,000 — as text.
///
/// The split stays in integers so every digit of the `i64` range survives; dividing through
/// `f64` rounds above 2^53 and can print the wrong minor units. `keep_zeros` picks the spelling:
/// `to_text` keeps all four fraction digits, while `to_dax_text` drops the trailing zeros DAX
/// does not print. Both spellings share this split so they cannot disagree on the value.
fn currency_text(value: i64, keep_zeros: bool) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.unsigned_abs();
    let (whole, fraction) = (magnitude / 10_000, magnitude % 10_000);
    let fraction = format!("{fraction:04}");
    let fraction = if keep_zeros { fraction.as_str() } else { fraction.trim_end_matches('0') };
    if fraction.is_empty() { format!("{sign}{whole}") } else { format!("{sign}{whole}.{fraction}") }
}

/// A `DateTime` serial as DAX prints it: `M/D/YYYY H:MM:SS AM/PM`.
fn datetime_text(serial: f64) -> String {
    use chrono::Datelike;

    // A non-finite serial is not a date: print the value itself instead of a fabricated one
    // (`NaN.floor() as i64` is 0, which would render as 1899-12-30 noon).
    if !serial.is_finite() {
        return serial.to_string();
    }
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
            let key: Vec<ScalarKey> = row.iter().map(distinct_key).collect();
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

    #[test]
    fn distinct_keys_treat_one_number_as_one_value() {
        assert_eq!(distinct_key(&Scalar::Integer(2)), distinct_key(&Scalar::Decimal(2.0)));
        assert_eq!(distinct_key(&Scalar::Currency(20_000)), distinct_key(&Scalar::Decimal(2.0)));
        assert_eq!(distinct_key(&Scalar::String("A".into())), distinct_key(&Scalar::String("a".into())));
        assert_ne!(distinct_key(&Scalar::Integer(2)), distinct_key(&Scalar::Integer(3)));
        // A non-finite serial prints as itself, not as a fabricated calendar date.
        assert_eq!(Scalar::DateTime(f64::NAN).to_dax_text(), "NaN");
    }

    #[test]
    fn negative_zero_prints_as_zero() {
        // Ordinary arithmetic reaches -0.0 — `ROUND ( -0.4, 0 )` and `0 * -1` both produce it —
        // and DAX prints zero, never `-0`, so `&` and the text functions must agree.
        assert_eq!(Scalar::Decimal(-0.0).to_dax_text(), "0");
        assert_eq!(Scalar::Decimal(0.0).to_dax_text(), "0");
        // A small negative fraction still prints its sign, so only a zero loses it.
        assert_eq!(Scalar::Decimal(-0.5).to_dax_text(), "-0.5");
    }

    #[test]
    fn currency_text_keeps_every_fixed_point_digit() {
        // `to_text` is the storage/debug spelling: exactly four fraction digits, sign included.
        assert_eq!(Scalar::Currency(0).to_text(), "0.0000");
        assert_eq!(Scalar::Currency(-123_450).to_text(), "-12.3450");
        // Leading zeros in the fraction stay significant (`1.0001`, not `1.1`).
        assert_eq!(Scalar::Currency(10_001).to_text(), "1.0001");
        // Dividing through f64 rounded these away: 2^53 + 1 lost its last minor unit and
        // `i64::MAX / 10_000` printed `.6250` instead of `.5807`.
        assert_eq!(Scalar::Currency(9_007_199_254_740_993).to_text(), "900719925474.0993");
        assert_eq!(Scalar::Currency(9_007_199_254_740_999).to_text(), "900719925474.0999");
        assert_eq!(Scalar::Currency(i64::MAX).to_text(), "922337203685477.5807");
        assert_eq!(Scalar::Currency(i64::MIN).to_text(), "-922337203685477.5808");

        // Both spellings now share one exact split, so they agree on every digit.
        for (value, dax) in [
            (0, "0"),
            (-123_450, "-12.345"),
            (10_001, "1.0001"),
            (9_007_199_254_740_993, "900719925474.0993"),
            (9_007_199_254_740_999, "900719925474.0999"),
            (i64::MAX, "922337203685477.5807"),
            (i64::MIN, "-922337203685477.5808"),
        ] {
            assert_eq!(Scalar::Currency(value).to_dax_text(), dax, "DAX text of {value}");
            assert_eq!(
                Scalar::Currency(value).to_text().trim_end_matches('0').trim_end_matches('.'),
                dax,
                "the two currency spellings disagree on {value}"
            );
        }
    }

    #[test]
    fn non_finite_values_survive_a_json_round_trip() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for scalar in [Scalar::Decimal(value), Scalar::DateTime(value)] {
                let json = serde_json::to_value(&scalar).expect("serialize");
                let back: Scalar = serde_json::from_value(json.clone()).expect("deserialize");
                assert_eq!(format!("{back:?}"), format!("{scalar:?}"), "round trip through {json}");
            }
        }
        // A finite decimal keeps its natural JSON number, and text is still text.
        assert_eq!(serde_json::to_value(Scalar::Decimal(1.5)).expect("serialize"), serde_json::json!(1.5));
        let text: Scalar = serde_json::from_value(serde_json::json!("NaN")).expect("deserialize");
        assert_eq!(text, Scalar::String("NaN".into()));
    }

    #[test]
    fn relationship_keys_normalize_only_what_is_exact() {
        // Integer and currency amounts fold onto the decimal key a decimal column uses.
        assert_eq!(relationship_key_of(&ScalarKey::Integer(2)), relationship_key_of(&ScalarKey::Decimal(order_bits(2.0))));
        assert_eq!(relationship_key_of(&ScalarKey::Currency(20_000)), relationship_key_of(&ScalarKey::Decimal(order_bits(2.0))));
        // A currency amount whose major-unit division cannot round-trip keeps its own key.
        let huge = 5_500_000_000_000_001_i64;
        assert_ne!(
            relationship_key_of(&ScalarKey::Currency(huge)),
            relationship_key_of(&ScalarKey::Currency(huge + 1)),
            "two distinct currency keys must not collapse"
        );
        // A NaN serial never joins, so it keeps its variant instead of matching a decimal NaN.
        assert_ne!(
            relationship_key_of(&ScalarKey::from(&Scalar::DateTime(f64::NAN))),
            relationship_key_of(&ScalarKey::from(&Scalar::Decimal(f64::NAN))),
        );
    }
}
