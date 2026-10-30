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
//! # Compression and vectorized reads
//!
//! A column that repeats compresses itself at construction time, always losslessly and only when
//! the encoding saves at least [`MIN_ENCODING_SAVING`] bytes:
//!
//! * integers, currency and booleans with long runs become [`ColumnData::RleInteger`],
//!   [`ColumnData::RleCurrency`] or [`ColumnData::RleBoolean`] — run values plus run offsets, so a
//!   row lookup is a binary search and `len()` is one read;
//! * text with few distinct labels becomes [`ColumnData::PackedString`], whose codes take
//!   `ceil(log2(labels))` bits per row; a constant label takes zero bits;
//! * decimals, date/times and high-entropy columns keep the plain vector — an alternative encoding
//!   that would not pay for its index is never chosen.
//!
//! Decoding is exact: [`ModelStorage::is_faithful`] still compares every cell with the model scalar,
//! so the evaluator's results are unchanged whatever encoding a column ends up using.
//!
//! [`TableStorage::for_each_value`] is the allocation-free scan: it yields [`ValueRef`] values
//! (text borrows the dictionary label) over a row-id slice. [`TableStorage::sum_batch`] and
//! [`TableStorage::sum_batches`] fold such slices in [`SCAN_BATCH_ROWS`]-row blocks without a
//! [`Scalar`] per cell, with exactly the result of [`TableStorage::sum`]; the sum counters and the
//! other aggregates read columns the same way. [`ModelStorage::footprint`] reports the encoded
//! against logical bytes of every column and of the model total.
//!
//! Table and column identifiers are the positions in the [`DataModel`], which makes them stable
//! for the lifetime of the [`ModelStorage`]. Every accessor is bounded: out-of-range table,
//! column and row lookups return `Option`/`Result` instead of panicking, and aggregates report
//! overflow rather than wrapping.
//!
//! # Evaluation substrate
//!
//! [`crate::eval::Evaluator`] materializes the model once through [`ModelStorage::from_model`]
//! and reads it for filtering, aggregates, `VALUES`/`DISTINCT`, relationship joins and pivot
//! grouping; it no longer rescans [`crate::model::ModelTable::rows`] on those paths. A storage
//! read is only used when [`ModelStorage::is_faithful`] holds, i.e. every table is rectangular
//! and every stored value equals the model scalar exactly. Models with ragged rows or columns
//! whose representation is promoted (mixed numeric families, booleans mixed with other values,
//! NaNs) keep the row-scan semantics instead.
//!
//! What the evaluator reads here:
//!
//! * [`TableStorage::filter_rows`] for column filters, applied per column and intersected;
//! * [`RelationshipIndex::rows_from`]/[`RelationshipIndex::rows_to`] to propagate those filters
//!   across relationships and to answer `RELATED`/`RELATEDTABLE`, so key columns are never
//!   rescanned;
//! * [`TableStorage::count`], [`TableStorage::distinct_count`], [`TableStorage::count_blank`],
//!   [`TableStorage::numeric_values`] and [`TableStorage::distinct_values`] for the aggregate fast
//!   paths, `VALUES`/`DISTINCT` and the statistical aggregates. The typed exact aggregates
//!   ([`TableStorage::sum`], [`TableStorage::min`], [`TableStorage::max`],
//!   [`TableStorage::aggregate`]) stay the API for typed consumers; the evaluator derives the six
//!   numeric DAX aggregates from [`TableStorage::numeric_values`] because DAX's `SUM`/`MIN`/`MAX`
//!   are defined over `f64` observations in this evaluator, not over an exact integer total.
//!
//! What still scans model rows: `FILTER`, `ADDCOLUMNS`, `TOPN` and every other function that
//! evaluates an expression per row, because those need a row context; plus any model that is not
//! faithful.
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

    /// The bytes this bitmap allocates.
    pub fn encoded_bytes(&self) -> usize {
        self.words.len() * std::mem::size_of::<u64>()
    }
}

/// The saving, in bytes, an alternative column encoding must reach before it replaces the plain
/// value vector.
///
/// Below it the extra decode indirection (a binary search or a bit unpack) costs more than the
/// bytes it saves, so tiny columns — model tables with a handful of rows — always keep the plain
/// representation. This is what makes the encoding choice stable for hand-written fixtures while
/// long columns still compress: a 100k-row column needs to save at least this many bytes, which a
/// low-cardinality column does by orders of magnitude.
pub const MIN_ENCODING_SAVING: usize = 64;

/// Word-packed string codes: `bits` bits per code, least-significant bit first inside each `u64`.
///
/// A dictionary-encoded text column with few distinct labels needs `ceil(log2(labels))` bits per
/// row instead of a full `u32`. [`ColumnData::PackedString`] uses this representation when it saves
/// at least [`MIN_ENCODING_SAVING`] bytes over a plain code vector. Every accessor is bounded:
/// out-of-range reads return `None`, and a malformed code index cannot panic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedCodes {
    bits: u8,
    len: usize,
    words: Vec<u64>,
}

impl PackedCodes {
    /// Packs `codes` at their smallest width. `None` only when the bit count overflows `usize`.
    ///
    /// An all-zero code vector packs to zero bits and no words, which is how a constant text
    /// column ends up costing no code bytes at all.
    pub fn pack(codes: &[u32]) -> Option<Self> {
        let max = codes.iter().copied().max().unwrap_or(0);
        let bits = (u32::BITS - max.leading_zeros()) as u8;
        let len = codes.len();
        let total_bits = len.checked_mul(usize::from(bits))?;
        let mut words = vec![0u64; total_bits.div_ceil(64)];
        if bits > 0 {
            let mask = code_mask(bits);
            for (index, code) in codes.iter().enumerate() {
                let offset = index.checked_mul(usize::from(bits))?;
                let word = offset / 64;
                let shift = offset % 64;
                let value = u64::from(*code) & mask;
                if let Some(slot) = words.get_mut(word) {
                    *slot |= value << shift;
                }
                let spill = shift + usize::from(bits);
                if spill > 64
                    && let Some(next) = words.get_mut(word + 1)
                {
                    // A code that straddles two words continues in the next one.
                    *next |= value >> (usize::from(bits) - (spill - 64));
                }
            }
        }
        Some(PackedCodes { bits, len, words })
    }

    /// The code at `index`, or `None` when out of range.
    pub fn get(&self, index: usize) -> Option<u32> {
        if index >= self.len {
            return None;
        }
        if self.bits == 0 {
            return Some(0);
        }
        let bits = usize::from(self.bits);
        let offset = index.checked_mul(bits)?;
        let word = offset / 64;
        let shift = offset % 64;
        let mut value = *self.words.get(word)? >> shift;
        let spill = shift + bits;
        if spill > 64 {
            let high = *self.words.get(word + 1)? & ((1u64 << (spill - 64)) - 1);
            value |= high << (bits - (spill - 64));
        }
        u32::try_from(value & code_mask(self.bits)).ok()
    }

    /// The number of codes covered.
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` when no codes are covered.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// How many bits each code occupies (`0` for a constant column).
    pub fn bits_per_code(&self) -> u8 {
        self.bits
    }

    /// The bytes this packed vector allocates.
    pub fn bytes(&self) -> usize {
        self.words.len() * std::mem::size_of::<u64>()
    }

    /// The raw backing words, for callers that want to move the vector without repacking.
    pub fn words(&self) -> &[u64] {
        &self.words
    }
}

fn code_mask(bits: u8) -> u64 {
    if bits >= 32 { u64::from(u32::MAX) } else { (1u64 << bits) - 1 }
}

/// A non-allocating view of one stored value, used by the vectorized scan API.
///
/// [`Scalar`] owns its text, so materializing a `Scalar` for every cell of a text column clones a
/// dictionary label per row. A [`ValueRef`] borrows the label instead, so
/// [`TableStorage::for_each_value`] and [`TableStorage::sum_batch`] touch no heap at all.
/// [`ValueRef::to_scalar`] materializes an owned value only when the caller actually needs one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ValueRef<'a> {
    Blank,
    Integer(i64),
    /// Currency scaled by 10,000.
    Currency(i64),
    Decimal(f64),
    DateTime(f64),
    Boolean(bool),
    Text(&'a str),
}

impl ValueRef<'_> {
    /// Whether this cell is BLANK.
    pub fn is_blank(&self) -> bool {
        matches!(self, ValueRef::Blank)
    }

    /// The owned scalar for this cell. Text is the only representation that allocates.
    pub fn to_scalar(&self) -> Scalar {
        match self {
            ValueRef::Blank => Scalar::Blank,
            ValueRef::Integer(value) => Scalar::Integer(*value),
            ValueRef::Currency(value) => Scalar::Currency(*value),
            ValueRef::Decimal(value) => Scalar::Decimal(*value),
            ValueRef::DateTime(value) => Scalar::DateTime(*value),
            ValueRef::Boolean(value) => Scalar::Boolean(*value),
            ValueRef::Text(text) => Scalar::String((*text).to_string()),
        }
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
/// Values are positional: row `n` is `values[n]` (or `codes[n]` for plain text, or the run that
/// contains `n` for a run-length column). A set bit in `nulls` marks a blank row; blank rows keep a
/// placeholder in the value/code vector (or inside a run) so row order is stable. The run-length and
/// packed variants are produced by the constructor's compression pass and decode to exactly the
/// same values as the plain ones; see the module documentation for the selection rule.
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
    /// Run-length encoded integers: run `i` covers rows `offsets[i]..offsets[i + 1]` and holds
    /// `values[i]`. `offsets` always ends with the row count, so `len()` is one read and row
    /// lookup is a binary search.
    RleInteger {
        values: Vec<i64>,
        offsets: Vec<usize>,
        nulls: Bitmap,
    },
    /// Run-length encoded currency (scaled by 10,000).
    RleCurrency {
        values: Vec<i64>,
        offsets: Vec<usize>,
        nulls: Bitmap,
    },
    /// Run-length encoded booleans.
    RleBoolean {
        values: Vec<bool>,
        offsets: Vec<usize>,
        nulls: Bitmap,
    },
    /// Dictionary-encoded text with bit-packed codes.
    PackedString {
        dictionary: Vec<String>,
        packed: PackedCodes,
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
            ColumnData::RleInteger { offsets, .. } | ColumnData::RleCurrency { offsets, .. } | ColumnData::RleBoolean { offsets, .. } => {
                offsets.last().copied().unwrap_or(0)
            }
            ColumnData::PackedString { packed, .. } => packed.len(),
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
            ColumnData::Integer { .. } | ColumnData::RleInteger { .. } => ColumnKind::Integer,
            ColumnData::Currency { .. } | ColumnData::RleCurrency { .. } => ColumnKind::Currency,
            ColumnData::Decimal { .. } => ColumnKind::Decimal,
            ColumnData::DateTime { .. } => ColumnKind::DateTime,
            ColumnData::Boolean { .. } | ColumnData::RleBoolean { .. } => ColumnKind::Boolean,
            ColumnData::String { .. } | ColumnData::PackedString { .. } => ColumnKind::String,
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
            | ColumnData::String { nulls, .. }
            | ColumnData::RleInteger { nulls, .. }
            | ColumnData::RleCurrency { nulls, .. }
            | ColumnData::RleBoolean { nulls, .. }
            | ColumnData::PackedString { nulls, .. } => nulls.get(row),
            ColumnData::Blank { .. } => true,
        })
    }

    /// The value at `row` as a borrowed view, preserving blanks. `None` when the row is out of
    /// range.
    ///
    /// This is the allocation-free read path: text cells borrow the dictionary label.
    pub fn value_ref(&self, row: usize) -> Option<ValueRef<'_>> {
        match self {
            ColumnData::Integer { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(row).copied().map(ValueRef::Integer)
            }
            ColumnData::Currency { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(row).copied().map(ValueRef::Currency)
            }
            ColumnData::Decimal { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(row).copied().map(ValueRef::Decimal)
            }
            ColumnData::DateTime { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(row).copied().map(ValueRef::DateTime)
            }
            ColumnData::Boolean { values, nulls } => {
                if row >= values.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(row).copied().map(ValueRef::Boolean)
            }
            ColumnData::RleInteger { values, offsets, nulls } => {
                let run = rle_run(offsets, row)?;
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(run).copied().map(ValueRef::Integer)
            }
            ColumnData::RleCurrency { values, offsets, nulls } => {
                let run = rle_run(offsets, row)?;
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(run).copied().map(ValueRef::Currency)
            }
            ColumnData::RleBoolean { values, offsets, nulls } => {
                let run = rle_run(offsets, row)?;
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                values.get(run).copied().map(ValueRef::Boolean)
            }
            ColumnData::String { dictionary, codes, nulls } => {
                if row >= codes.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                let code = usize::try_from(codes.get(row).copied()?).ok()?;
                dictionary.get(code).map(|label| ValueRef::Text(label.as_str()))
            }
            ColumnData::PackedString { dictionary, packed, nulls } => {
                if row >= packed.len() {
                    return None;
                }
                if nulls.get(row) {
                    return Some(ValueRef::Blank);
                }
                let code = usize::try_from(packed.get(row)?).ok()?;
                dictionary.get(code).map(|label| ValueRef::Text(label.as_str()))
            }
            ColumnData::Blank { len } => {
                if row < *len {
                    Some(ValueRef::Blank)
                } else {
                    None
                }
            }
        }
    }

    /// The value at `row`, preserving blanks. `None` when the row is out of range.
    pub fn value(&self, row: usize) -> Option<Scalar> {
        self.value_ref(row).map(|value| value.to_scalar())
    }

    /// Visits `rows` in order without allocating a [`Scalar`] per cell.
    ///
    /// Out-of-range rows are skipped. [`TableStorage::for_each_value`] is the checked entry point;
    /// this method is the raw column-level scan.
    pub fn visit_rows(&self, rows: &[usize], mut visit: impl FnMut(usize, ValueRef<'_>)) {
        for &row in rows {
            if let Some(value) = self.value_ref(row) {
                visit(row, value);
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
            | ColumnData::String { nulls, .. }
            | ColumnData::RleInteger { nulls, .. }
            | ColumnData::RleCurrency { nulls, .. }
            | ColumnData::RleBoolean { nulls, .. }
            | ColumnData::PackedString { nulls, .. } => Some(nulls),
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
            ColumnData::String { dictionary, .. } | ColumnData::PackedString { dictionary, .. } => dictionary,
            _ => &[],
        }
    }

    /// The plain string code vector, or an empty slice for non-string columns and for text columns
    /// whose codes are bit-packed.
    ///
    /// A packed column has no `&[u32]` to lend out; [`Self::code_at`] reads one code from any text
    /// representation.
    pub fn codes(&self) -> &[u32] {
        match self {
            ColumnData::String { codes, .. } => codes,
            _ => &[],
        }
    }

    /// The dictionary code at `row` for any text representation, or `None` when the column is not
    /// text or the row is out of range.
    pub fn code_at(&self, row: usize) -> Option<u32> {
        match self {
            ColumnData::String { codes, .. } => codes.get(row).copied(),
            ColumnData::PackedString { packed, .. } => packed.get(row),
            _ => None,
        }
    }

    /// The dictionary and null bitmap of a text column, for the paths that compare labels once
    /// instead of once per row.
    fn string_parts(&self) -> Option<(&[String], &Bitmap)> {
        match self {
            ColumnData::String { dictionary, nulls, .. } | ColumnData::PackedString { dictionary, nulls, .. } => Some((dictionary.as_slice(), nulls)),
            _ => None,
        }
    }

    /// The full-width payload of the column in bytes: one 8-byte numeric slot per row, one byte per
    /// boolean, or one 4-byte code per row plus the dictionary text for text columns. The null
    /// bitmap is not part of the logical size.
    pub fn logical_bytes(&self) -> usize {
        let rows = self.len();
        let dictionary: usize = self.dictionary().iter().map(String::len).sum();
        match self.kind() {
            ColumnKind::Integer | ColumnKind::Currency => rows.saturating_mul(std::mem::size_of::<i64>()),
            ColumnKind::Decimal | ColumnKind::DateTime => rows.saturating_mul(std::mem::size_of::<f64>()),
            ColumnKind::Boolean => rows,
            ColumnKind::String => rows.saturating_mul(std::mem::size_of::<u32>()).saturating_add(dictionary),
            ColumnKind::Blank => 0,
        }
    }

    /// The bytes this representation actually allocates: value/code storage, any encoding index,
    /// the dictionary text and the null bitmap.
    pub fn encoded_bytes(&self) -> usize {
        let dictionary: usize = self.dictionary().iter().map(String::len).sum();
        let bitmap = self.null_bitmap().map_or(0, Bitmap::encoded_bytes);
        let payload = match self {
            ColumnData::Integer { values, .. } | ColumnData::RleInteger { values, .. } => values.len().saturating_mul(std::mem::size_of::<i64>()),
            ColumnData::Currency { values, .. } | ColumnData::RleCurrency { values, .. } => values.len().saturating_mul(std::mem::size_of::<i64>()),
            ColumnData::Decimal { values, .. } | ColumnData::DateTime { values, .. } => values.len().saturating_mul(std::mem::size_of::<f64>()),
            ColumnData::Boolean { values, .. } | ColumnData::RleBoolean { values, .. } => values.len(),
            ColumnData::String { codes, .. } => codes.len().saturating_mul(std::mem::size_of::<u32>()),
            ColumnData::PackedString { packed, .. } => packed.bytes(),
            ColumnData::Blank { .. } => 0,
        };
        let offsets = match self {
            ColumnData::RleInteger { offsets, .. } | ColumnData::RleCurrency { offsets, .. } | ColumnData::RleBoolean { offsets, .. } => {
                offsets.len().saturating_mul(std::mem::size_of::<usize>())
            }
            _ => 0,
        };
        payload.saturating_add(offsets).saturating_add(dictionary).saturating_add(bitmap)
    }
}

/// The run containing `row`, or `None` when `row` is past the last offset.
///
/// The binary search (`partition_point`) makes random access logarithmic in the run count; the
/// scan loop keeps a cursor instead, so a sequential read is one comparison per row.
fn rle_run(offsets: &[usize], row: usize) -> Option<usize> {
    let runs = offsets.len().checked_sub(1)?;
    if row >= offsets.last().copied()? {
        return None;
    }
    let run = offsets.partition_point(|start| *start <= row).checked_sub(1)?;
    (run < runs).then_some(run)
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

    /// The encoded-vs-logical byte accounting of this column.
    pub fn footprint(&self) -> ColumnFootprint {
        ColumnFootprint { kind: self.kind(), rows: self.len(), logical_bytes: self.data.logical_bytes(), encoded_bytes: self.data.encoded_bytes() }
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

    /// The byte accounting of every column plus the row count.
    pub fn footprint(&self) -> TableFootprint {
        TableFootprint { table: self.name.clone(), rows: self.row_count, columns: self.columns.iter().map(ColumnStorage::footprint).collect() }
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
        if let Some((dictionary, nulls)) = storage.data().string_parts() {
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
                    storage
                        .data()
                        .code_at(row)
                        .and_then(|code| usize::try_from(code).ok())
                        .and_then(|code| allowed.get(code))
                        .copied()
                        .unwrap_or(false)
                };
                if matched {
                    rows.push(row);
                }
            }
            return Ok(rows);
        }
        let data = storage.data();
        for row in 0..self.row_count {
            if let Some(value) = data.value(row)
                && wanted.contains(&ScalarKey::from(&value))
            {
                rows.push(row);
            }
        }
        Ok(rows)
    }

    /// `SUM` over the selected rows, skipping blanks. An empty selection or an all-blank column
    /// sums to BLANK. Integer and currency overflow widens to a decimal instead of wrapping.
    pub fn sum(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        self.sum_batch(column, rows)
    }

    /// `SUM` over one row-id slice through the vectorized scan API: identical result to
    /// [`Self::sum`], one pass of raw values in [`SCAN_BATCH_ROWS`]-row blocks, and no
    /// `Option<Scalar>` allocation per cell.
    ///
    /// Run-length encoded columns read one run per block of rows rather than one value per row;
    /// integer and currency totals stay exact in `i128` and widen to a decimal on `i64` overflow.
    pub fn sum_batch(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let mut total = NumericTotal::default();
        fold_numeric(self, storage, rows, &mut total)?;
        finish_numeric_sum(self, storage, total)
    }

    /// [`Self::sum_batch`] for several row-id slices, matching the column representation and its
    /// overflow policy once.
    ///
    /// Pivot grouping sums one slice per group; this returns one scalar per slice in the same order.
    pub fn sum_batches(&self, column: usize, row_slices: &[&[usize]]) -> Result<Vec<Scalar>, StorageError> {
        let storage = self.column_checked(column)?;
        let mut totals = Vec::with_capacity(row_slices.len());
        for rows in row_slices {
            self.check_rows(storage, rows)?;
            let mut total = NumericTotal::default();
            fold_numeric(self, storage, rows, &mut total)?;
            totals.push(finish_numeric_sum(self, storage, total)?);
        }
        Ok(totals)
    }

    /// Visits `(row, value)` for every row id in `rows`, in the given order, without materializing
    /// a [`Scalar`] per cell.
    ///
    /// This is the vectorized read entry point for aggregates and renderers that only look at a
    /// value: text cells borrow the dictionary label, blanks come back as [`ValueRef::Blank`], and a
    /// row id outside the table is rejected before the scan starts.
    pub fn for_each_value(&self, column: usize, rows: &[usize], visit: impl FnMut(usize, ValueRef<'_>)) -> Result<(), StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        storage.data().visit_rows(rows, visit);
        Ok(())
    }

    /// `COUNT` over the selected rows: the number of non-blank values.
    ///
    /// This is a bitmap read per row — no value decode and no allocation — which is the whole cost
    /// of the aggregate.
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
    ///
    /// A dictionary-encoded column counts distinct codes instead of decoding a value per row, so
    /// each label is compared once whatever the row count.
    pub fn distinct_count(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        if let Some((dictionary, nulls)) = storage.data().string_parts() {
            let mut seen = vec![false; dictionary.len()];
            let mut distinct: usize = 0;
            let mut seen_blank = false;
            for &row in rows {
                if nulls.get(row) {
                    if !seen_blank {
                        seen_blank = true;
                        distinct += 1;
                    }
                    continue;
                }
                if let Some(code) = storage.data().code_at(row).and_then(|code| usize::try_from(code).ok())
                    && let Some(seen) = seen.get_mut(code)
                    && !*seen
                {
                    *seen = true;
                    distinct += 1;
                }
            }
            return Ok(Scalar::Integer(i64::try_from(distinct).unwrap_or(i64::MAX)));
        }
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

    /// The numeric values of `column` over `rows`, in row order, using DAX coercion.
    ///
    /// This is the observation list `SUM`, `AVERAGE`, `PRODUCT`, `MIN`, `MAX` and the variance
    /// family aggregate: blanks, booleans and text contribute nothing (BLANK is skipped,
    /// booleans and text are not numbers here even when Excel would coerce them), integers are
    /// read as `f64`, currency in major units and date/time serials unchanged. Non-finite values
    /// are kept; callers that need finite observations filter them.
    pub fn numeric_values(&self, column: usize, rows: &[usize]) -> Result<Vec<f64>, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let mut values = Vec::with_capacity(rows.len());
        // Every representation decodes through the borrowed view, so a run-length encoded column
        // and a plain one take the same path and a text cell never clones its label here.
        storage.data().visit_rows(rows, |_, value| match value {
            ValueRef::Integer(cell) => values.push(cell as f64),
            ValueRef::Currency(cell) => values.push(cell as f64 / 10_000.0),
            ValueRef::Decimal(cell) | ValueRef::DateTime(cell) => values.push(cell),
            // BLANK, logical and text values are not numeric observations in DAX.
            ValueRef::Blank | ValueRef::Boolean(_) | ValueRef::Text(_) => {}
        });
        Ok(values)
    }

    /// `COUNTBLANK` over the selected rows: blanks plus empty text.
    ///
    /// DAX counts an empty string as blank, so a dictionary-encoded column tests its labels as
    /// well as the null bitmap. Every other representation only counts null bits.
    pub fn count_blank(&self, column: usize, rows: &[usize]) -> Result<Scalar, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let data = storage.data();
        let mut count: usize = 0;
        if matches!(data, ColumnData::Blank { .. }) {
            count = rows.len();
        } else if let Some((dictionary, nulls)) = data.string_parts() {
            for &row in rows {
                let blank = nulls.get(row);
                let empty = !blank
                    && data
                        .code_at(row)
                        .and_then(|code| usize::try_from(code).ok())
                        .and_then(|code| dictionary.get(code))
                        .is_some_and(|label| label.is_empty());
                if blank || empty {
                    count += 1;
                }
            }
        } else {
            for &row in rows {
                if data.is_blank_at(row) == Some(true) {
                    count += 1;
                }
            }
        }
        Ok(Scalar::Integer(i64::try_from(count).unwrap_or(i64::MAX)))
    }

    /// The distinct values of `column` over `rows`, in first-occurrence row order.
    ///
    /// BLANK is a value of its own. A dictionary-encoded column compares each label once through
    /// its dictionary instead of decoding a value per row.
    pub fn distinct_values(&self, column: usize, rows: &[usize]) -> Result<Vec<Scalar>, StorageError> {
        let storage = self.column_checked(column)?;
        self.check_rows(storage, rows)?;
        let data = storage.data();
        let mut values = Vec::new();
        if let Some((dictionary, nulls)) = data.string_parts() {
            let mut seen = vec![false; dictionary.len()];
            let mut seen_blank = false;
            for &row in rows {
                if nulls.get(row) {
                    if !seen_blank {
                        seen_blank = true;
                        values.push(Scalar::Blank);
                    }
                    continue;
                }
                let Some(code) = data.code_at(row).and_then(|code| usize::try_from(code).ok()) else {
                    continue;
                };
                let Some(label) = dictionary.get(code) else {
                    continue;
                };
                if let Some(seen) = seen.get_mut(code)
                    && !*seen
                {
                    *seen = true;
                    values.push(Scalar::String(label.clone()));
                }
            }
        } else {
            let mut seen = BTreeSet::new();
            for &row in rows {
                let Some(value) = data.value(row) else {
                    continue;
                };
                if seen.insert(ScalarKey::from(&value)) {
                    values.push(value);
                }
            }
        }
        Ok(values)
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

/// The number of row ids one compact scan block covers in [`TableStorage::sum_batch`].
///
/// The batch is a fixed-size slice of the caller's row-id list: it bounds the working set of one
/// scan step without copying the selection, and it makes the traversal cache-friendly whatever the
/// column length.
pub const SCAN_BATCH_ROWS: usize = 1024;

/// The running total of a vectorized numeric scan.
///
/// Integers and currency accumulate exactly in `i128` (so an overflowing `i64` column widens
/// instead of wrapping), decimal and date/time values accumulate in `f64`, and `overflow` records an
/// `i128` overflow so the caller can report it once at the end of the scan.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct NumericTotal {
    integer: i128,
    decimal: f64,
    overflow: bool,
    any: bool,
}

impl NumericTotal {
    fn push(&mut self, value: ValueRef<'_>) {
        match value {
            ValueRef::Integer(cell) | ValueRef::Currency(cell) => match self.integer.checked_add(i128::from(cell)) {
                Some(total) => {
                    self.integer = total;
                    self.any = true;
                }
                None => self.overflow = true,
            },
            ValueRef::Decimal(cell) | ValueRef::DateTime(cell) => {
                self.decimal += cell;
                self.any = true;
            }
            ValueRef::Blank | ValueRef::Boolean(_) | ValueRef::Text(_) => {}
        }
    }

    fn is_valid(&self) -> bool {
        !self.overflow && self.decimal.is_finite()
    }
}

/// Folds `rows` of a numeric column into `total`, one [`SCAN_BATCH_ROWS`]-row block at a time.
fn fold_numeric(table: &TableStorage, storage: &ColumnStorage, rows: &[usize], total: &mut NumericTotal) -> Result<(), StorageError> {
    match storage.kind() {
        ColumnKind::Blank => return Ok(()),
        ColumnKind::Boolean | ColumnKind::String => {
            return Err(StorageError::NotNumeric { table: table.name.clone(), column: storage.name().to_string() });
        }
        _ => {}
    }
    for batch in rows.chunks(SCAN_BATCH_ROWS) {
        storage.data().visit_rows(batch, |_, value| total.push(value));
    }
    Ok(())
}

/// Converts a completed [`NumericTotal`] into the scalar `SUM` returns for this column kind.
///
/// The shape of the result is exactly what the previous per-row implementation produced: an integer
/// or currency total that does not fit `i64` becomes a decimal, an all-blank selection is BLANK, and
/// an `i128`/`f64` overflow is an error rather than a wrapped number.
fn finish_numeric_sum(table: &TableStorage, storage: &ColumnStorage, total: NumericTotal) -> Result<Scalar, StorageError> {
    if !total.is_valid() {
        return Err(StorageError::Overflow { table: table.name.clone(), column: storage.name().to_string() });
    }
    if !total.any {
        return Ok(Scalar::Blank);
    }
    Ok(match storage.kind() {
        ColumnKind::Integer => match i64::try_from(total.integer) {
            Ok(value) => Scalar::Integer(value),
            Err(_) => Scalar::Decimal(total.integer as f64),
        },
        ColumnKind::Currency => match i64::try_from(total.integer) {
            Ok(value) => Scalar::Currency(value),
            Err(_) => Scalar::Decimal(total.integer as f64 / 10_000.0),
        },
        _ => Scalar::Decimal(total.decimal),
    })
}

/// Byte accounting for one column: the logical full-width size of its values against what the
/// encoding actually allocates.
///
/// `logical_bytes` is the payload a plain column needs — one 8-byte slot per numeric row, one byte
/// per boolean, one 4-byte code per text row plus the dictionary text. `encoded_bytes` is what this
/// column really allocates: value, code and run storage, any encoding index, the dictionary text and
/// the null bitmap. Dictionary text is counted in both, so the ratio measures the value vector, not
/// the labels, and a high-entropy column can never look smaller than its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColumnFootprint {
    /// The column representation.
    pub kind: ColumnKind,
    /// The number of rows.
    pub rows: usize,
    /// The full-width value payload in bytes.
    pub logical_bytes: usize,
    /// What the representation allocates in bytes, null bitmap included.
    pub encoded_bytes: usize,
}

impl ColumnFootprint {
    /// The bytes saved relative to the logical payload.
    pub fn saved_bytes(&self) -> usize {
        self.logical_bytes.saturating_sub(self.encoded_bytes)
    }

    /// `encoded_bytes / logical_bytes`; `1.0` for an empty column.
    pub fn ratio(&self) -> f64 {
        if self.logical_bytes == 0 { 1.0 } else { self.encoded_bytes as f64 / self.logical_bytes as f64 }
    }
}

/// Byte accounting for one table: its row count and one [`ColumnFootprint`] per column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableFootprint {
    pub table: String,
    pub rows: usize,
    pub columns: Vec<ColumnFootprint>,
}

impl TableFootprint {
    /// The logical bytes of every column.
    pub fn logical_bytes(&self) -> usize {
        self.columns.iter().map(|column| column.logical_bytes).sum()
    }

    /// The encoded bytes of every column.
    pub fn encoded_bytes(&self) -> usize {
        self.columns.iter().map(|column| column.encoded_bytes).sum()
    }

    /// The bytes saved relative to the logical payload.
    pub fn saved_bytes(&self) -> usize {
        self.logical_bytes().saturating_sub(self.encoded_bytes())
    }
}

/// Byte accounting for a whole [`ModelStorage`]: per-table and per-column encoded versus logical
/// bytes, with the model total.
///
/// This is the storage-side companion of the evaluator's call counters
/// ([`crate::StorageStats`], which counts aggregates and scans). Keep the two apart: the counter
/// type belongs to evaluation and the footprint belongs to the store.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StorageFootprint {
    pub tables: Vec<TableFootprint>,
}

impl StorageFootprint {
    /// The logical bytes across every table.
    pub fn logical_bytes(&self) -> usize {
        self.tables.iter().map(TableFootprint::logical_bytes).sum()
    }

    /// The encoded bytes across every table.
    pub fn encoded_bytes(&self) -> usize {
        self.tables.iter().map(TableFootprint::encoded_bytes).sum()
    }

    /// The bytes saved relative to the logical payload.
    pub fn saved_bytes(&self) -> usize {
        self.logical_bytes().saturating_sub(self.encoded_bytes())
    }

    /// `encoded_bytes / logical_bytes`; `1.0` when the storage holds no values.
    pub fn ratio(&self) -> f64 {
        let logical = self.logical_bytes();
        if logical == 0 { 1.0 } else { self.encoded_bytes() as f64 / logical as f64 }
    }

    /// Every column footprint as `(table name, column index, footprint)` in model order.
    pub fn columns(&self) -> impl Iterator<Item = (&str, usize, &ColumnFootprint)> {
        self.tables.iter().flat_map(|table| {
            let name = table.table.as_str();
            table.columns.iter().enumerate().map(move |(index, column)| (name, index, column))
        })
    }
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
///
/// A storage is *faithful* when every table is rectangular (each row has exactly one value per
/// column) and every stored value equals the model scalar exactly. Faithful storage is a
/// transparent substitute for the model rows, which is what lets the evaluator answer
/// aggregation and filtering from columns without changing a single result. [`Self::is_faithful`]
/// reports it; when it is `false` the evaluator falls back to scanning the model rows.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelStorage {
    tables: Vec<TableStorage>,
    relationships: Vec<RelationshipIndex>,
    faithful: bool,
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
        let mut faithful = true;
        for table in &model.tables {
            if !names.insert(table.name.to_ascii_lowercase()) {
                return Err(StorageError::DuplicateTable { table: table.name.clone() });
            }
            let column_names = table.columns.iter().map(|column| column.name.clone()).collect();
            let storage = TableStorage::from_rows(table.name.clone(), column_names, table.rows.clone())?;
            faithful &= storage_round_trips(&storage, table);
            tables.push(storage);
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

        Ok(ModelStorage { tables, relationships, faithful })
    }

    /// Whether the storage is a lossless, rectangular copy of the model rows.
    ///
    /// `false` means some column was promoted (integers mixed with decimals, booleans mixed with
    /// other values), some row is ragged, or a value did not round-trip (NaN). The evaluator only
    /// answers from storage when this is `true`, so such models keep their row-scan results.
    pub fn is_faithful(&self) -> bool {
        self.faithful
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

    /// The encoded-vs-logical byte accounting of every table and column, with the model total.
    pub fn footprint(&self) -> StorageFootprint {
        StorageFootprint { tables: self.tables.iter().map(TableStorage::footprint).collect() }
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

/// Whether `storage` is a rectangular, value-for-value copy of `table`'s rows.
///
/// Ragged rows are blank-extended by [`TableStorage::from_rows`] and a promoted column rewrites
/// values, so neither round-trips: comparing every cell against the model scalar is what makes
/// the claim exact instead of a guess about [`infer_kind`].
fn storage_round_trips(storage: &TableStorage, table: &crate::model::ModelTable) -> bool {
    if storage.row_count() != table.rows.len() || storage.columns().len() != table.columns.len() {
        return false;
    }
    table.rows.iter().enumerate().all(|(row, cells)| {
        cells.len() == table.columns.len()
            && storage.columns().iter().enumerate().all(|(column, _)| storage.value(column, row).as_ref() == cells.get(column))
    })
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
///
/// The plain representation is built first and then the compression pass decides whether a run-length or
/// bit-packed encoding saves enough bytes to keep; the decoded values are identical either way.
fn build_column(rows: &[Vec<Scalar>], index: usize) -> ColumnData {
    let len = rows.len();
    let kind = infer_kind(rows, index);
    let data = match kind {
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
    };
    compress(data)
}

/// Rewrites a plain column into a run-length or bit-packed representation when that saves at least
/// [`MIN_ENCODING_SAVING`] bytes over the value vector.
///
/// The choice is per column and deterministic:
///
/// * integers, currency and booleans use run-length encoding when the run index plus the run values
///   is meaningfully smaller than one value per row — the constant and low-churn columns of a fact
///   table, where a whole block repeats;
/// * dictionary-encoded text uses bit-packed codes when `ceil(log2(labels))` bits per row beats a
///   `u32` code, which covers every column with fewer than 2^32 labels while still paying nothing
///   when the column is short;
/// * decimals and date/times stay plain: `f64` has no exact equality (NaN never repeats) and a run
///   index would cost more than the 8-byte values it replaces for typical spreads.
///
/// The null bitmap is untouched, so blanks decode exactly as before and
/// [`ModelStorage::is_faithful`] stays true for a model that was faithful before compression.
fn compress(data: ColumnData) -> ColumnData {
    match data {
        ColumnData::Integer { values, nulls } => match run_length_i64(&values) {
            Some((values, offsets)) => ColumnData::RleInteger { values, offsets, nulls },
            None => ColumnData::Integer { values, nulls },
        },
        ColumnData::Currency { values, nulls } => match run_length_i64(&values) {
            Some((values, offsets)) => ColumnData::RleCurrency { values, offsets, nulls },
            None => ColumnData::Currency { values, nulls },
        },
        ColumnData::Boolean { values, nulls } => match run_length_bool(&values) {
            Some((values, offsets)) => ColumnData::RleBoolean { values, offsets, nulls },
            None => ColumnData::Boolean { values, nulls },
        },
        ColumnData::String { dictionary, codes, nulls } => {
            let plain = codes.len().saturating_mul(std::mem::size_of::<u32>());
            match PackedCodes::pack(&codes) {
                Some(packed) if packed.bytes().saturating_add(MIN_ENCODING_SAVING) <= plain => ColumnData::PackedString { dictionary, packed, nulls },
                _ => ColumnData::String { dictionary, codes, nulls },
            }
        }
        other => other,
    }
}

/// Run-length encodes `values` into `(run values, run offsets)` when that saves enough bytes.
///
/// `offsets[i]` is the first row of run `i` and the last entry is the row count, which is what makes
/// `ColumnData::len` a single read and the run lookup a binary search.
fn run_length_i64(values: &[i64]) -> Option<(Vec<i64>, Vec<usize>)> {
    if values.is_empty() {
        return None;
    }
    let mut runs: Vec<i64> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    for (row, value) in values.iter().enumerate() {
        if runs.last() != Some(value) {
            runs.push(*value);
            offsets.push(row);
        }
    }
    offsets.push(values.len());
    let encoded = runs.len().checked_mul(std::mem::size_of::<i64>())?.checked_add(offsets.len().checked_mul(std::mem::size_of::<usize>())?)?;
    let plain = values.len().checked_mul(std::mem::size_of::<i64>())?;
    (encoded.saturating_add(MIN_ENCODING_SAVING) <= plain).then_some((runs, offsets))
}

/// Run-length encodes a boolean vector under the same size rule as [`run_length_i64`].
fn run_length_bool(values: &[bool]) -> Option<(Vec<bool>, Vec<usize>)> {
    if values.is_empty() {
        return None;
    }
    let mut runs: Vec<bool> = Vec::new();
    let mut offsets: Vec<usize> = Vec::new();
    for (row, value) in values.iter().enumerate() {
        if runs.last() != Some(value) {
            runs.push(*value);
            offsets.push(row);
        }
    }
    offsets.push(values.len());
    let encoded = runs.len().checked_add(offsets.len().checked_mul(std::mem::size_of::<usize>())?)?;
    let plain = values.len();
    (encoded.saturating_add(MIN_ENCODING_SAVING) <= plain).then_some((runs, offsets))
}
