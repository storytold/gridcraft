//! GridCraft core types: cell addresses, values and Excel's coercion rules, error values,
//! date serials and typed-input parsing. No dependencies on the rest of the workspace.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod addr;
pub mod date;
pub mod parse;
pub mod value;

pub use addr::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, col_to_letters, letters_to_col};
pub use date::DateSystem;
pub use value::{Array, CellError, Value, compare, compare_numbers, compare_text, number_to_text, sort_compare};

/// Does `c` belong to a script that capitalises the first letter of words (PROPER, Flash Fill's
/// proper case)? Georgian doesn't: Unicode maps Mkhedruli to Mtavruli capitals, but Mtavruli is
/// only for all-caps text, so UPPER still gives Mtavruli.
pub fn capitalizes(c: char) -> bool {
    !matches!(c, '\u{10A0}'..='\u{10FF}' | '\u{1C90}'..='\u{1CBF}' | '\u{2D00}'..='\u{2D2F}')
}
