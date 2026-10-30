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
            Scalar::String(value) => value.trim().parse().ok(),
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Scalar::Blank => String::new(),
            Scalar::Boolean(true) => "TRUE".into(),
            Scalar::Boolean(false) => "FALSE".into(),
            Scalar::Integer(value) => value.to_string(),
            Scalar::Currency(value) => format!("{:.4}", *value as f64 / 10_000.0),
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

/// A comparable/hashable key used for filter context values.
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
