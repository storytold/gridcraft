//! In-memory columnar storage for a [`DataModel`], in the spirit of a VertiPaq-style core.
//!
//! The model metadata layer ([`crate::model`]) only describes tables, columns, relationships and
//! measures. This module materializes the optional inline rows as compressed columns:
//!
//! * every column has its own representation: integer, currency (scaled by 10,000), decimal,
//!   date/time, boolean, dictionary-encoded string, or an all-blank column with no value vector;
//! * every value column carries a blank/null bitmap, so BLANK is never confused with zero, an
//!   empty string or `FALSE`;
//! * string columns store a dictionary plus one code per row, so repeated labels are stored once;
//! * row order is preserved exactly, including duplicates and blanks;
//! * relationships get bidirectional key indexes (key value to row ids) for filter propagation.
//!
//! Table and column identifiers are the positions in the [`DataModel`], which makes them stable
//! for the lifetime of the [`ModelStorage`]. Every accessor is bounded: out-of-range table,
//! column and row lookups return `Option`/`Result` instead of panicking, and aggregates report
//! overflow rather than wrapping.
//!
//! ```
//! use gridcraft_dax::model::{ModelColumn, ModelTable, DataModel};
//! use gridcraft_dax::storage::ModelStorage;
//! use gridcraft_dax::value::Scalar;
//!
//! let model = DataModel {
//!     tables: vec![ModelTable {
//!         name: "Sales".into(),
//!         columns: vec![ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) }],
//!         rows: vec![vec![Scalar::Integer(2)], vec![Scalar::Blank], vec![Scalar::Integer(3)]],
//!     }],
//!     ..DataModel::default()
//! };
//! let storage = ModelStorage::from_model(&model)?;
//! let table = storage.table(0).expect("table exists");
//! let column = table.column_index("Amount").expect("column exists");
//! assert_eq!(table.columns()[column].data().kind(), gridcraft_dax::storage::ColumnKind::Integer);
//! assert_eq!(table.sum(column, &[0, 1, 2])?, Scalar::Integer(5));
//! # Ok::<(), gridcraft_dax::storage::StorageError>(())
//! ```

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use crate::model::DataModel;
use crate::value::{Scalar, ScalarKey};

/// A bitset used for column blank/null bitmaps.
///
/// Bit set means "this row is BLANK". Out-of-range accesses read as `false` and are ignored by
/// [`Bitmap::set`], so the bitmap itself can never panic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitmap {
    words: Vec<u64>,
    len: usize,
}

impl Bitmap {
    /// Allocates a bitmap covering `len` rows, with every bit clear.
    pub fn new(len: usize) -> Self {
        Bitmap { words: vec![0; len.div_ceil(64)], len }
    }

    /// The number of rows covered by the bitmap.
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` when the bitmap covers no rows.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the bit at `index` is set. Out-of-range indices read as `false`.
    pub fn get(&self, index: usize) -> bool {
        if index >= self.len {
            return false;
        }
        self.words.get(index / 64).is_some_and(|word| word & (1u64 << (index % 64)) != 0)
    }

    /// Sets the bit at `index`. Returns `false` when the index is out of range.
    pub fn set(&mut self, index: usize) -> bool {
        if index >= self.len {
            return false;
        }
        let Some(word) = self.words.get_mut(index / 64) else {
            return false;
        };
        *word |= 1u64 << (index % 64);
        true
    }

    /// The number of set bits.
    pub fn count_ones(&self) -> usize {
        self.words.iter().map(|word| word.count_ones() as usize).sum()
    }

    /// Iterates the set bit positions in ascending order.
    pub fn iter_set(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.len).filter(|index| self.get(*index))
    }
}

/// The physical representation of a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnKind {
    Integer,
    /// Currency stored as an integer scaled by 10,000, matching [`Scalar::Currency`].
    Currency,
    Decimal,
    /// A DateTime serial: whole days since 1899-12-30 plus a time fraction.
    DateTime,
    Boolean,
    /// Dictionary-encoded text.
    String,
    /// A column with no non-blank values; it has a length but no value vector.
    Blank,
}

impl ColumnKind {
    /// The model-facing name of the representation.
    pub fn name(self) -> &'static str {
        match self {
            ColumnKind::Integer => "Integer",
            ColumnKind::Currency => "Currency",
            ColumnKind::Decimal => "Decimal",
            ColumnKind::DateTime => "DateTime",
            ColumnKind::Boolean => "Boolean",
            ColumnKind::String => "String",
            ColumnKind::Blank => "Blank",
        }
    }

    /// Whether [`TableStorage::sum`] is defined for this representation.
    ///
    /// `SUM` works for integers, currency, decimals and date/time serials.
    pub fn is_numeric(self) -> bool {
        matches!(self, ColumnKind::Integer | ColumnKind::Currency | ColumnKind::Decimal | ColumnKind::DateTime)
    }
}

/// One column's compressed values.
///
/// Values are positional: row `n` is `values[n]` (or `codes[n]` for strings). A set bit in
/// `nulls` marks a blank row; blank rows keep a placeholder in the value/code vector so row order
/// is stable.
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnData {
    Integer {
        values: Vec<i64>,
        nulls: Bitmap,
    },
    Currency {
        values: Vec<i64>,
        nulls: Bitmap,
    },
    Decimal {
        values: Vec<f64>,
        nulls: Bitmap,
    },
    DateTime {
        values: Vec<f64>,
        nulls: Bitmap,
    },
    Boolean {
        values: Vec<bool>,
        nulls: Bitmap,
    },
    /// Dictionary-encoded text. `codes[row]` indexes `dictionary`; blank rows use code `0` and
    /// are masked by `nulls`.
    String {
        dictionary: Vec<String>,
        codes: Vec<u32>,
        nulls: Bitmap,
    },
    /// Every row is blank, so no vector is stored.
    Blank {
        len: usize,
    },
}

impl ColumnData {
    /// The number of rows covered by the column.
    pub fn len(&self) -> usize {
        match self {
            ColumnData::Integer { values, .. } | ColumnData::Currency { values, .. } => values.len(),
            ColumnData::Boolean { values, .. } => values.len(),
            ColumnData::Decimal { values, .. } | ColumnData::DateTime { values, .. } => values.len(),
            ColumnData::String { codes, .. } => codes.len(),
            ColumnData::Blank { len } => *len,
        }
    }

    /// `true` when the column covers no rows.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The physical representation of the column.
    pub fn kind(&self) -> ColumnKind {
        match self {
            ColumnData::Integer { .. } => ColumnKind::Integer,
            ColumnData::Currency { .. } => ColumnKind::Currency,
            ColumnData::Decimal { .. } => ColumnKind::Decimal,
            ColumnData::DateTime { .. } => ColumnKind::DateTime,
            ColumnData::Boolean { .. } => ColumnKind::Boolean,
            ColumnData::String { .. } => ColumnKind::String,
            ColumnData::Blank { .. } => ColumnKind::Blank,
        }
    }

    /// Whether `row` is blank. `None` when the row is out of range.
    pub fn is_blank_at(&self, row: usize) -> Option<bool> {
        if row >= self.len() {
            return None;
        }
        Some(match self {
            ColumnData::Integer { nulls, .. }
            | ColumnData::Currency { nulls, .. }
            | ColumnData::Decimal { nulls, .. }
            | ColumnData::DateTime { nulls, .. }
            | ColumnData::Boolean { nulls, .. }
            | ColumnData::String { nulls, .. } => nulls.get(row),
            ColumnData::Blank { .. } => true,
        })
    }

    /// The value at `row`, preserving blanks. `None` when the row is out of range.
    pub fn value(&self, row: usize) -> Option<Scalar> {
        match self {
            ColumnData::Integer { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(Scalar::Blank);
                }
                values.get(row).copied().map(Scalar::Integer)
            }
            ColumnData::Currency { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(Scalar::Blank);
                }
                values.get(row).copied().map(Scalar::Currency)
            }
            ColumnData::Decimal { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(Scalar::Blank);
                }
                values.get(row).copied().map(Scalar::Decimal)
            }
            ColumnData::DateTime { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(Scalar::Blank);
                }
                values.get(row).copied().map(Scalar::DateTime)
            }
            ColumnData::Boolean { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(Scalar::Blank);
                }
                values.get(row).copied().map(Scalar::Boolean)
            }
            ColumnData::String { dictionary, codes, nulls } => {
                if row >= codes.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(Scalar::Blank);
                }
                let code = usize::try_from(codes.get(row).copied()?).ok()?;
                dictionary.get(code).cloned().map(Scalar::String)
            }
            ColumnData::Blank { len } => {
                if row < *len {
                    Some(Scalar::Blank)
                } else {
                    None
                }
            }
        }
    }

    /// The blank/null bitmap, when the column stores values. An all-blank column has no bitmap.
    pub fn null_bitmap(&self) -> Option<&Bitmap> {
        match self {
            ColumnData::Integer { nulls, .. }
            | ColumnData::Currency { nulls, .. }
            | ColumnData::Decimal { nulls, .. }
            | ColumnData::DateTime { nulls, .. }
            | ColumnData::Boolean { nulls, .. }
            | ColumnData::String { nulls, .. } => Some(nulls),
            ColumnData::Blank { .. } => None,
        }
    }

    /// The number of blank rows.
    pub fn blank_count(&self) -> usize {
        match self {
            ColumnData::Blank { len } => *len,
            _ => self.null_bitmap().map_or(0, Bitmap::count_ones),
        }
    }

    /// The string dictionary, or an empty slice for non-string columns.
    pub fn dictionary(&self) -> &[String] {
        match self {
            ColumnData::String { dictionary, .. } => dictionary,
            _ => &[],
        }
    }

    /// The string codes (one per row), or an empty slice for non-string columns.
    pub fn codes(&self) -> &[u32] {
        match self {
            ColumnData::String { codes, .. } => codes,
            _ => &[],
        }
    }
}

/// A named column inside a [`TableStorage`].
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnStorage {
    name: String,
    data: ColumnData,
}

impl ColumnStorage {
    /// Creates a named column. The caller is responsible for matching table row counts; use
    /// [`TableStorage::new`] or [`TableStorage::from_rows`] for validated construction.
    pub fn new(name: impl Into<String>, data: ColumnData) -> Self {
        ColumnStorage { name: name.into(), data }
    }

    /// The column name, as declared by the model.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The compressed values.
    pub fn data(&self) -> &ColumnData {
        &self.data
    }

    /// The physical representation.
    pub fn kind(&self) -> ColumnKind {
        self.data.kind()
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// `true` when the column covers no rows.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// The value at `row`, or `None` when the row is out of range.
    pub fn value(&self, row: usize) -> Option<Scalar> {
        self.data.value(row)
    }

    /// Whether `row` is blank, or `None` when the row is out of range.
    pub fn is_blank_at(&self, row: usize) -> Option<bool> {
        self.data.is_blank_at(row)
    }
}

/// A table's columns plus its row count.
#[derive(Clone, Debug, PartialEq)]
pub struct TableStorage {
    name: String,
    row_count: usize,
    columns: Vec<ColumnStorage>,
}

impl TableStorage {
    /// Creates a table after checking that column names are unique (case-insensitively) and every
    /// column covers `row_count` rows.
    pub fn new(name: impl Into<String>, row_count: usize, columns: Vec<ColumnStorage>) -> Result<Self, StorageError> {
        let name = name.into();
        let mut names = BTreeSet::new();
        for column in &columns {
            if !names.insert(column.name().to_ascii_lowercase()) {
                return Err(StorageError::DuplicateColumn { table: name, column: column.name().to_string() });
            }
            if column.len() != row_count {
                return Err(StorageError::ColumnLengthMismatch {
                    table: name,
                    column: column.name().to_string(),
                    column_len: column.len(),
                    row_count,
                });
            }
        }
        Ok(TableStorage { name, row_count, columns })
    }

    /// Builds a table from row-oriented scalars, inferring each column's representation.
    ///
    /// Missing trailing cells in ragged rows are read as BLANK. The row count is the number of
    /// supplied rows; extra cells beyond `column_names.len()` are ignored.
    pub fn from_rows(name: impl Into<String>, column_names: Vec<String>, rows: Vec<Vec<Scalar>>) -> Result<Self, StorageError> {
        let name = name.into();
        let row_count = rows.len();
        let columns =
            column_names.into_iter().enumerate().map(|(index, column_name)| ColumnStorage::new(column_name, build_column(&rows, index))).collect();
        TableStorage::new(name, row_count, columns)
    }

    /// The model table name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The number of rows, including all-blank rows.
    pub fn row_count(&self) -> usize {
        self.row_count
    }

    /// `true` when the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    /// The columns in model order.
    pub fn columns(&self) -> &[ColumnStorage] {
        &self.columns
    }

    /// The column at `index`, or `None` when out of range.
    pub fn column(&self, index: usize) -> Option<&ColumnStorage> {
        self.columns.get(index)
    }

    /// The index of a column by name, ignoring case.
    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name))
    }

    /// The column with this name, ignoring case.
    pub fn column_by_name(&self, name: &str) -> Option<&ColumnStorage> {
        self.columns.iter().find(|column| column.name.eq_ignore_ascii_case(name))
    }

    /// The value at `(column, row)`, or `None` when either is out of range.
    pub fn value(&self, column: usize, row: usize) -> Option<Scalar> {
        self.column(column)?.value(row)
    }

    /// The value at `(column_name, row)`, ignoring column-name case.
    pub fn value_by_name(&self, column: &str, row: usize) -> Option<Scalar> {
        self.column_by_name(column)?.value(row)
    }

    /// Row indices whose value in `column` is one of `values`, in ascending row order.
    ///
    /// Matching is by [`ScalarKey`], so `ScalarKey::Blank` selects blank rows and duplicate keys
    /// are folded. String columns use the dictionary to test each distinct label once.
    pub fn filter_rows(&self, column: usize, values: &[ScalarKey]) -> Result<Vec<usize>, StorageError> {
        let storage = self.column_checked(column)?;
        let wanted: BTreeSet<ScalarKey> = values.iter().cloned().collect();
        let mut rows = Vec::new();
        match storage.data() {
            ColumnData::String { dictionary, codes, nulls } => {
                let wanted_text: BTreeSet<&str> = wanted
                    .iter()
                    .filter_map(|key| match key {
                        ScalarKey::String(text) => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let blank_wanted = wanted.contains(&ScalarKey::Blank);
                let mut allowed = vec![false; dictionary.len()];
                for (code, text) in dictionary.iter().enumerate() {
                    if wanted_text.contains(text.as_str())
                        && let Some(slot) = allowed.get_mut(code)
                    {
                        *slot = true;
                    }
                }
                for row in 0..self.row_count {
                    let matched = if nulls.get(row) {
                        blank_wanted
                    } else {
                        codes.get(row).and_then(|code| usize::try_from(*code).ok()).and_then(|code| allowed.get(code)).copied().unwrap_or(false)
                    };
                    if matched {
                        rows.push(row);
                    }
                }
            }
            data => {
                for row in 0..self.row_count {
                    if let Some(value) = data.value(row)
                        && wanted.contains(&ScalarKey::from(&value))
                    {
                        rows.push(row);
                    }
                }
            }
        }
        Ok(rows)
    }

    /// `SUM` over the selected rows, skipping blanks. An empty selection or an all-blank column
    /// sums to BLANK. Integer and currency overflow widens to a decimal instead of wrapping.
    pub fn sum(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let name = storage.name().to_string();
        match storage.data() {
            ColumnData::Blank { .. } => Ok(Scalar::Blank),
            ColumnData::Boolean { .. } | ColumnData::String { .. } => Err(StorageError::NotNumeric { table: self.name.clone(), column: name }),
            ColumnData::Integer { values, nulls } => {
                let mut total: i128 = 0;
                let mut any = false;
                for &row in rows {
                    if nulls.get(row) {
                        continue;
                    }
                    let Some(value) = values.get(row) else {
                        continue;
                    };
                    total = total
                        .checked_add(i128::from(*value))
                        .ok_or_else(|| StorageError::Overflow { table: self.name.clone(), column: name.clone() })?;
                    any = true;
                }
                if !any {
                    return Ok(Scalar::Blank);
                }
                Ok(match i64::try_from(total) {
                    Ok(value) => Scalar::Integer(value),
                    Err(_) => Scalar::Decimal(total as f64),
                })
            }
            ColumnData::Currency { values, nulls } => {
                let mut total: i128 = 0;
                let mut any = false;
                for &row in rows {
                    if nulls.get(row) {
                        continue;
                    }
                    let Some(value) = values.get(row) else {
                        continue;
                    };
                    total = total
                        .checked_add(i128::from(*value))
                        .ok_or_else(|| StorageError::Overflow { table: self.name.clone(), column: name.clone() })?;
                    any = true;
                }
                if !any {
                    return Ok(Scalar::Blank);
                }
                Ok(match i64::try_from(total) {
                    Ok(value) => Scalar::Currency(value),
                    Err(_) => Scalar::Decimal(total as f64 / 10_000.0),
                })
            }
            ColumnData::Decimal { values, nulls } | ColumnData::DateTime { values, nulls } => {
                let mut total = 0.0;
                let mut any = false;
                for &row in rows {
                    if nulls.get(row) {
                        continue;
                    }
                    let Some(value) = values.get(row) else {
                        continue;
                    };
                    total += value;
                    any = true;
                }
                if !any {
                    return Ok(Scalar::Blank);
                }
                if !total.is_finite() {
                    return Err(StorageError::Overflow { table: self.name.clone(), column: name });
                }
                Ok(Scalar::Decimal(total))
            }
        }
    }

    /// `COUNT` over the selected rows: the number of non-blank values.
    pub fn count(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let data = storage.data();
        let mut count: usize = 0;
        for &row in rows {
            if data.is_blank_at(row) == Some(false) {
                count += 1;
            }
        }
        Ok(Scalar::Integer(i64::try_from(count).unwrap_or(i64::MAX)))
    }

    /// `DISTINCTCOUNT` over the selected rows. BLANK counts as a distinct value, as in DAX.
    pub fn distinct_count(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let data = storage.data();
        let mut keys = BTreeSet::new();
        for &row in rows {
            if let Some(value) = data.value(row) {
                keys.insert(ScalarKey::from(&value));
            }
        }
        Ok(Scalar::Integer(i64::try_from(keys.len()).unwrap_or(i64::MAX)))
    }

    /// The smallest non-blank value over the selected rows, or BLANK when there is none.
    pub fn min(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        self.extremum(column, rows, Ordering::Less)
    }

    /// The largest non-blank value over the selected rows, or BLANK when there is none.
    pub fn max(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        self.extremum(column, rows, Ordering::Greater)
    }

    /// Dispatches to [`TableStorage::sum`], [`TableStorage::count`],
    /// [`TableStorage::distinct_count`], [`TableStorage::min`] or [`TableStorage::max`].
    pub fn aggregate(&self, column: usize, rows: &[usize], aggregate: Aggregate) -> Result<Scalar, StorageError> {
        match aggregate {
            Aggregate::Sum => self.sum(column, rows),
            Aggregate::Count => self.count(column, rows),
            Aggregate::DistinctCount => self.distinct_count(column, rows),
            Aggregate::Min => self.min(column, rows),
            Aggregate::Max => self.max(column, rows),
        }
    }

    fn extremum(&self, column: usize, rows: &[usize], want: Ordering) -> Result<Scalar, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let data = storage.data();
        let mut best: Option<Scalar> = None;
        for &row in rows {
            let Some(value) = data.value(row) else {
                continue;
            };
            if value.is_blank() {
                continue;
            }
            let replace = match &best {
                None => true,
                Some(current) => value.compare(current).is_some_and(|ordering| ordering == want),
            };
            if replace {
                best = Some(value);
            }
        }
        Ok(best.unwrap_or(Scalar::Blank))
    }

    fn column_checked(&self, index: usize) -> Result<&ColumnStorage, StorageError> {
        self.columns.get(index).ok_or_else(|| StorageError::ColumnOutOfRange { table: self.name.clone(), index })
    }

    fn check_rows(&self, column: &ColumnStorage, rows: &[usize]) -> Result<(), StorageError> {
        for &row in rows {
            if row >= self.row_count {
                return Err(StorageError::RowOutOfRange {
                    table: self.name.clone(),
                    column: column.name().to_string(),
                    row,
                    row_count: self.row_count,
                });
            }
        }
        Ok(())
    }
}

/// The aggregate functions supported by [`TableStorage::aggregate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Aggregate {
    Sum,
    /// Non-blank value count.
    Count,
    /// Distinct non-blank and blank value count.
    DistinctCount,
    Min,
    Max,
}

/// A single-column relationship plus its key indexes.
///
/// Both directions are indexed so filter propagation can go either way: `from_rows` maps a key
/// value to row ids in the `from` table (the many side in a typical star schema), and `to_rows`
/// maps a key value to row ids in the `to` table (the one side). Duplicate keys keep every row id
/// in ascending order, and blank keys are indexed like any other value.
#[derive(Clone, Debug, PartialEq)]
pub struct RelationshipIndex {
    pub from_table: usize,
    pub from_column: usize,
    pub to_table: usize,
    pub to_column: usize,
    pub bidirectional: bool,
    pub inactive: bool,
    /// Key value to row ids in the `from` table.
    pub from_rows: BTreeMap<ScalarKey, Vec<usize>>,
    /// Key value to row ids in the `to` table.
    pub to_rows: BTreeMap<ScalarKey, Vec<usize>>,
}

impl RelationshipIndex {
    /// Row ids in the `from` table whose key equals `key`; empty when there is no match.
    pub fn rows_from(&self, key: &ScalarKey) -> &[usize] {
        match self.from_rows.get(key) {
            Some(rows) => rows.as_slice(),
            None => &[],
        }
    }

    /// Row ids in the `to` table whose key equals `key`; empty when there is no match.
    pub fn rows_to(&self, key: &ScalarKey) -> &[usize] {
        match self.to_rows.get(key) {
            Some(rows) => rows.as_slice(),
            None => &[],
        }
    }

    /// The number of distinct key values on the `from` side.
    pub fn from_key_count(&self) -> usize {
        self.from_rows.len()
    }

    /// The number of distinct key values on the `to` side.
    pub fn to_key_count(&self) -> usize {
        self.to_rows.len()
    }
}

/// A [`DataModel`] materialized as columnar tables plus relationship indexes.
///
/// Table ids are the positions in `model.tables` and column ids the positions in
/// `model.tables[i].columns`, so both are stable and match the model's own lookup methods.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelStorage {
    tables: Vec<TableStorage>,
    relationships: Vec<RelationshipIndex>,
}

impl ModelStorage {
    /// Materializes every model table and relationship.
    ///
    /// Column representations are inferred from the stored values. Fails when the model has
    /// duplicate table names, duplicate column names inside a table, or a relationship naming a
    /// table or column that does not exist.
    pub fn from_model(model: &DataModel) -> Result<Self, StorageError> {
        let mut tables = Vec::with_capacity(model.tables.len());
        let mut names = BTreeSet::new();
        for table in &model.tables {
            if !names.insert(table.name.to_ascii_lowercase()) {
                return Err(StorageError::DuplicateTable { table: table.name.clone() });
            }
            let column_names = table.columns.iter().map(|column| column.name.clone()).collect();
            tables.push(TableStorage::from_rows(table.name.clone(), column_names, table.rows.clone())?);
        }

        let mut relationships = Vec::with_capacity(model.relationships.len());
        for relationship in &model.relationships {
            let from_table = table_index_by_name(&tables, &relationship.from_table)
                .ok_or_else(|| StorageError::UnknownTable { table: relationship.from_table.clone() })?;
            let to_table = table_index_by_name(&tables, &relationship.to_table)
                .ok_or_else(|| StorageError::UnknownTable { table: relationship.to_table.clone() })?;
            let (Some(from_storage), Some(to_storage)) = (tables.get(from_table), tables.get(to_table)) else {
                return Err(StorageError::UnknownTable { table: relationship.from_table.clone() });
            };
            let from_column = from_storage
                .column_index(&relationship.from_column)
                .ok_or_else(|| StorageError::UnknownColumn { table: relationship.from_table.clone(), column: relationship.from_column.clone() })?;
            let to_column = to_storage
                .column_index(&relationship.to_column)
                .ok_or_else(|| StorageError::UnknownColumn { table: relationship.to_table.clone(), column: relationship.to_column.clone() })?;
            relationships.push(RelationshipIndex {
                from_table,
                from_column,
                to_table,
                to_column,
                bidirectional: relationship.bidirectional,
                inactive: relationship.inactive,
                from_rows: build_key_index(from_storage, from_column),
                to_rows: build_key_index(to_storage, to_column),
            });
        }

        Ok(ModelStorage { tables, relationships })
    }

    /// The tables in model order.
    pub fn tables(&self) -> &[TableStorage] {
        &self.tables
    }

    /// The relationship indexes in model order.
    pub fn relationships(&self) -> &[RelationshipIndex] {
        &self.relationships
    }

    /// The table at `index`, or `None` when out of range.
    pub fn table(&self, index: usize) -> Option<&TableStorage> {
        self.tables.get(index)
    }

    /// The index of a table by name, ignoring case.
    pub fn table_index(&self, name: &str) -> Option<usize> {
        table_index_by_name(&self.tables, name)
    }

    /// The table with this name, ignoring case.
    pub fn table_by_name(&self, name: &str) -> Option<&TableStorage> {
        self.table_index(name).and_then(|index| self.tables.get(index))
    }

    /// The index of a column inside a table, ignoring case.
    pub fn column_index(&self, table: usize, name: &str) -> Option<usize> {
        self.tables.get(table)?.column_index(name)
    }

    /// A column inside a table, or `None` when either id is out of range.
    pub fn column(&self, table: usize, column: usize) -> Option<&ColumnStorage> {
        self.tables.get(table)?.column(column)
    }

    /// A column inside a table by name, ignoring case.
    pub fn column_by_name(&self, table: usize, name: &str) -> Option<&ColumnStorage> {
        self.tables.get(table)?.column_by_name(name)
    }

    /// A value at `(table, column, row)`, or `None` when any id is out of range.
    pub fn value(&self, table: usize, column: usize, row: usize) -> Option<Scalar> {
        self.tables.get(table)?.value(column, row)
    }

    /// The relationship with these endpoints, matched by table and column name, ignoring case.
    /// The `from` side must match exactly; the `to` side is compared for convenience.
    pub fn relationship(&self, from_table: &str, from_column: &str, to_table: &str, to_column: &str) -> Option<&RelationshipIndex> {
        self.relationships.iter().find(|relationship| {
            self.tables.get(relationship.from_table).is_some_and(|table| table.name().eq_ignore_ascii_case(from_table))
                && self.tables.get(relationship.to_table).is_some_and(|table| table.name().eq_ignore_ascii_case(to_table))
                && self
                    .tables
                    .get(relationship.from_table)
                    .and_then(|table| table.column(relationship.from_column))
                    .is_some_and(|column| column.name().eq_ignore_ascii_case(from_column))
                && self
                    .tables
                    .get(relationship.to_table)
                    .and_then(|table| table.column(relationship.to_column))
                    .is_some_and(|column| column.name().eq_ignore_ascii_case(to_column))
        })
    }

    /// Relationships whose `from` table is `table`.
    pub fn relationships_from(&self, table: usize) -> impl Iterator<Item = &RelationshipIndex> {
        self.relationships.iter().filter(move |relationship| relationship.from_table == table)
    }

    /// Relationships whose `to` table is `table`.
    pub fn relationships_to(&self, table: usize) -> impl Iterator<Item = &RelationshipIndex> {
        self.relationships.iter().filter(move |relationship| relationship.to_table == table)
    }

    /// The total number of materialized rows across all tables.
    pub fn total_rows(&self) -> usize {
        self.tables.iter().map(TableStorage::row_count).sum()
    }
}

/// A storage construction or lookup failure. All variants are recoverable: no storage call
/// panics on bad input.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    #[error("table `{table}` has no column at index {index}")]
    ColumnOutOfRange { table: String, index: usize },
    #[error("row {row} is out of range for column `{column}` of table `{table}` (row count {row_count})")]
    RowOutOfRange { table: String, column: String, row: usize, row_count: usize },
    #[error("column `{column}` of table `{table}` is not numeric")]
    NotNumeric { table: String, column: String },
    #[error("a sum over column `{column}` of table `{table}` overflowed")]
    Overflow { table: String, column: String },
    #[error("table `{table}` has a duplicate column named `{column}`")]
    DuplicateColumn { table: String, column: String },
    #[error("table `{table}` is defined more than once")]
    DuplicateTable { table: String },
    #[error("column `{column}` of table `{table}` has {column_len} values but the table has {row_count} rows")]
    ColumnLengthMismatch { table: String, column: String, column_len: usize, row_count: usize },
    #[error("table `{table}` does not exist")]
    UnknownTable { table: String },
    #[error("table `{table}` has no column named `{column}`")]
    UnknownColumn { table: String, column: String },
}

fn table_index_by_name(tables: &[TableStorage], name: &str) -> Option<usize> {
    tables.iter().position(|table| table.name().eq_ignore_ascii_case(name))
}

fn build_key_index(table: &TableStorage, column: usize) -> BTreeMap<ScalarKey, Vec<usize>> {
    let mut index: BTreeMap<ScalarKey, Vec<usize>> = BTreeMap::new();
    for row in 0..table.row_count() {
        if let Some(value) = table.value(column, row) {
            index.entry(ScalarKey::from(&value)).or_default().push(row);
        }
    }
    index
}

/// Infers the column representation from the non-blank values at `index`.
///
/// Homogeneous columns keep their exact type, so values round-trip bit-for-bit. Mixed columns
/// promote deterministically:
///
/// * any string, or booleans mixed with another family, become dictionary-encoded text via
///   [`Scalar::to_text`];
/// * integers, currency and decimals together become `Decimal` (currency converted to major
///   units, integers as `f64`);
/// * date/time mixed with numeric values becomes `Decimal` (serials);
/// * a column with no non-blank values is [`ColumnKind::Blank`].
fn infer_kind(rows: &[Vec<Scalar>], index: usize) -> ColumnKind {
    let mut found = false;
    let mut string = false;
    let mut boolean = false;
    let mut integer = false;
    let mut currency = false;
    let mut decimal = false;
    let mut datetime = false;
    for cells in rows {
        match cells.get(index) {
            None | Some(Scalar::Blank) => {}
            Some(Scalar::String(_)) => {
                string = true;
                found = true;
            }
            Some(Scalar::Boolean(_)) => {
                boolean = true;
                found = true;
            }
            Some(Scalar::Integer(_)) => {
                integer = true;
                found = true;
            }
            Some(Scalar::Currency(_)) => {
                currency = true;
                found = true;
            }
            Some(Scalar::Decimal(_)) => {
                decimal = true;
                found = true;
            }
            Some(Scalar::DateTime(_)) => {
                datetime = true;
                found = true;
            }
        }
    }
    if !found {
        return ColumnKind::Blank;
    }
    if string {
        return ColumnKind::String;
    }
    if boolean {
        if integer || currency || decimal || datetime {
            return ColumnKind::String;
        }
        return ColumnKind::Boolean;
    }
    if datetime {
        if integer || currency || decimal {
            return ColumnKind::Decimal;
        }
        return ColumnKind::DateTime;
    }
    if decimal || (integer && currency) {
        return ColumnKind::Decimal;
    }
    if currency {
        return ColumnKind::Currency;
    }
    ColumnKind::Integer
}

fn numeric_i64(value: &Scalar) -> i64 {
    value.to_number().map_or(0, |number| number as i64)
}

fn numeric_f64(value: &Scalar) -> f64 {
    value.to_number().unwrap_or(0.0)
}

fn string_code(dictionary: &mut Vec<String>, positions: &mut BTreeMap<String, u32>, text: &str) -> u32 {
    if let Some(code) = positions.get(text) {
        return *code;
    }
    let code = u32::try_from(dictionary.len()).unwrap_or(u32::MAX);
    dictionary.push(text.to_string());
    positions.insert(text.to_string(), code);
    code
}

/// Builds one column from row-oriented scalars, masking blanks with a null bitmap.
fn build_column(rows: &[Vec<Scalar>], index: usize) -> ColumnData {
    let len = rows.len();
    let kind = infer_kind(rows, index);
    match kind {
        ColumnKind::Blank => ColumnData::Blank { len },
        ColumnKind::Integer => {
            let mut values = Vec::with_capacity(len);
            let mut nulls = Bitmap::new(len);
            for (row, cells) in rows.iter().enumerate() {
                match cells.get(index) {
                    None | Some(Scalar::Blank) => {
                        values.push(0);
                        nulls.set(row);
                    }
                    Some(value) => values.push(numeric_i64(value)),
                }
            }
            ColumnData::Integer { values, nulls }
        }
        ColumnKind::Currency => {
            let mut values = Vec::with_capacity(len);
            let mut nulls = Bitmap::new(len);
            for (row, cells) in rows.iter().enumerate() {
                match cells.get(index) {
                    None | Some(Scalar::Blank) => {
                        values.push(0);
                        nulls.set(row);
                    }
                    // Currency is already an integer scaled by 10,000; other numeric scalars are
                    // scaled so the column keeps a single unit.
                    Some(Scalar::Currency(value)) => values.push(*value),
                    Some(value) => values.push(value.to_number().map_or(0, |number| (number * 10_000.0) as i64)),
                }
            }
            ColumnData::Currency { values, nulls }
        }
        ColumnKind::Decimal => {
            let mut values = Vec::with_capacity(len);
            let mut nulls = Bitmap::new(len);
            for (row, cells) in rows.iter().enumerate() {
                match cells.get(index) {
                    None | Some(Scalar::Blank) => {
                        values.push(0.0);
                        nulls.set(row);
                    }
                    Some(value) => values.push(numeric_f64(value)),
                }
            }
            ColumnData::Decimal { values, nulls }
        }
        ColumnKind::DateTime => {
            let mut values = Vec::with_capacity(len);
            let mut nulls = Bitmap::new(len);
            for (row, cells) in rows.iter().enumerate() {
                match cells.get(index) {
                    None | Some(Scalar::Blank) => {
                        values.push(0.0);
                        nulls.set(row);
                    }
                    Some(value) => values.push(numeric_f64(value)),
                }
            }
            ColumnData::DateTime { values, nulls }
        }
        ColumnKind::Boolean => {
            let mut values = Vec::with_capacity(len);
            let mut nulls = Bitmap::new(len);
            for (row, cells) in rows.iter().enumerate() {
                match cells.get(index) {
                    None | Some(Scalar::Blank) => {
                        values.push(false);
                        nulls.set(row);
                    }
                    Some(Scalar::Boolean(value)) => values.push(*value),
                    Some(value) => values.push(value.to_bool().unwrap_or(false)),
                }
            }
            ColumnData::Boolean { values, nulls }
        }
        ColumnKind::String => {
            let mut dictionary: Vec<String> = Vec::new();
            let mut positions: BTreeMap<String, u32> = BTreeMap::new();
            let mut codes = Vec::with_capacity(len);
            let mut nulls = Bitmap::new(len);
            for (row, cells) in rows.iter().enumerate() {
                match cells.get(index) {
                    None | Some(Scalar::Blank) => {
                        codes.push(0);
                        nulls.set(row);
                    }
                    Some(value) => {
                        let text = value.to_text();
                        codes.push(string_code(&mut dictionary, &mut positions, &text));
                    }
                }
            }
            ColumnData::String { dictionary, codes, nulls }
        }
    }
}
