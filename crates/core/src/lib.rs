//! GridCraft core types: cell addresses, values and Excel's coercion rules, error values,
//! date serials and typed-input parsing. Depends only on `gridcraft-locale` (separators, names).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod addr;
pub mod date;
pub mod parse;
pub mod value;

pub use addr::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, col_to_letters, letters_to_col};
pub use date::DateSystem;
pub use value::{Array, CellError, Value, compare, compare_numbers, compare_text, number_to_text, number_to_text_in, sort_compare};
