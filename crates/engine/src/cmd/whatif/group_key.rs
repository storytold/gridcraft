use gridcraft_core::{CellError, Value};

/// Equality for Consolidate labels and Advanced Filter records. Number formats do not
/// change identity, and a number, text, or Boolean with the same rendering are distinct.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum GroupKey {
    Empty,
    Number(u64),
    Text(String),
    Bool(bool),
    Error(CellError),
}

impl GroupKey {
    pub(super) fn from_value(value: &Value) -> Self {
        match value.scalar() {
            Value::Empty | Value::Array(_) => Self::Empty,
            // Match numerical equality for signed zero before hashing the bits.
            Value::Number(n) => Self::Number(if n == 0.0 { 0 } else { n.to_bits() }),
            Value::Text(text) => Self::Text(text.to_lowercase()),
            Value::Bool(value) => Self::Bool(value),
            Value::Error(error) => Self::Error(error),
        }
    }
}
