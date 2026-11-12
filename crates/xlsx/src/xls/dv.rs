//! Data validation in `.xls` files: a DVal record ([MS-XLS] 2.4.96) starts a sheet's rules and
//! says how many follow, and each Dv record after it ([MS-XLS] 2.4.95) is one rule: its type,
//! operator, alert style, messages, up to two formulas (DVParsedFormula, 2.5.198.9) and the
//! ranges it covers (SqRefU, 2.5.247).
//!
//! The result matches what the `.xlsx` reader gives for the same workbook saved by Excel as
//! `.xlsx`. Excel itself reads exactly the DVal's count of Dv records, and none without a
//! DVal. It opens an unknown rule type as "any value", but refuses the whole file for an
//! unknown alert style or operator, or for a formula or range list that runs past its record;
//! here such a rule is skipped instead and the rest are kept.

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef};
use gridcraft_model::{CfOperator, ErrorStyle, Validation, ValidationKind};

use super::formula::{Links, Mode, decode};
use super::invalid;
use super::records::Cursor;
use crate::IoError;

/// The last row and column of the BIFF8 grid (65,536 × 256).
const LAST_ROW: u16 = 0xFFFF;
const LAST_COL: u16 = 0x00FF;

/// PtgStr: a string constant ([MS-XLS] 2.5.198.89).
const PTG_STR: u8 = 0x17;

/// The number of Dv records a DVal says follow it: flags (2 bytes), the input window's
/// position (4 + 4), the drop-down arrow's object id (4), then `idvMac` (4).
pub(super) fn dval(c: &mut Cursor<'_, '_>) -> Result<u32, IoError> {
    c.skip(14)?;
    c.u32()
}

/// One Dv record as stored, before its formulas are decoded.
pub(super) struct Dv {
    flags: u32,
    prompt_title: String,
    error_title: String,
    prompt: String,
    error: String,
    rgce1: Vec<u8>,
    rgce2: Vec<u8>,
    ranges: Vec<RangeRef>,
}

impl Dv {
    /// Characters held in the rule's four messages.
    pub fn text_len(&self) -> usize {
        [&self.prompt_title, &self.error_title, &self.prompt, &self.error].iter().map(|s| s.len()).sum()
    }
}

/// Reads a Dv record: flags, the four messages, the two formulas and the ranges.
pub(super) fn dv(c: &mut Cursor<'_, '_>) -> Result<Dv, IoError> {
    let flags = c.u32()?;
    let prompt_title = message(c)?;
    let error_title = message(c)?;
    let prompt = message(c)?;
    let error = message(c)?;
    let rgce1 = formula(c)?;
    let rgce2 = formula(c)?;
    let cref = usize::from(c.u16()?);
    // Each Ref8U is 8 bytes; a count the record can't hold makes Excel refuse the file.
    if cref.saturating_mul(8) > c.remaining() {
        return Err(invalid("data validation ranges run past the end of the record"));
    }
    let mut ranges = Vec::with_capacity(cref);
    for _ in 0..cref {
        let (r0, r1, c0, c1) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
        ranges.extend(range(r0, r1, c0, c1));
    }
    Ok(Dv { flags, prompt_title, error_title, prompt, error, rgce1, rgce2, ranges })
}

/// A title or message. Excel stores an empty one as a single NUL character.
fn message(c: &mut Cursor<'_, '_>) -> Result<String, IoError> {
    let s = c.string()?;
    Ok(if s == "\0" { String::new() } else { s })
}

/// A DVParsedFormula's tokens: their size, 2 unused bytes (which hold leftover data in
/// Excel's files, so they are never read), then the tokens.
fn formula(c: &mut Cursor<'_, '_>) -> Result<Vec<u8>, IoError> {
    let cce = usize::from(c.u16()?);
    c.skip(2)?;
    if cce > c.remaining() {
        return Err(invalid("data validation formula runs past the end of the record"));
    }
    (0..cce).map(|_| c.u8()).collect()
}

/// A Ref8U as a range. Columns past the BIFF8 grid are clamped to its last column, as Excel
/// does; a reversed range is dropped (Excel drops it too). A range spanning every row (or
/// column) of the BIFF8 grid is a whole column (row), as `.xlsx` stores the same rule.
fn range(r0: u16, r1: u16, c0: u16, c1: u16) -> Option<RangeRef> {
    let (c0, c1) = (c0.min(LAST_COL), c1.min(LAST_COL));
    if r0 > r1 || c0 > c1 {
        return None;
    }
    let (r0, r1) = if r0 == 0 && r1 == LAST_ROW { (0, MAX_ROWS - 1) } else { (u32::from(r0), u32::from(r1)) };
    let (c0, c1) = if c0 == 0 && c1 == LAST_COL { (0, MAX_COLS - 1) } else { (u32::from(c0), u32::from(c1)) };
    Some(RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
}

/// The model's rule for a Dv, or `None` when it has no ranges, when Excel would refuse it (an
/// alert style or operator outside the spec's values), or when a formula can't be decoded.
pub(super) fn validation(dv: Dv, links: &Links) -> Option<Validation> {
    // Relative references are offsets from the top-left cell of all the ranges, whatever order
    // they are listed in: the cell the engine evaluates the formulas from.
    let base = RangeRef::top_left(&dv.ranges)?;
    let f = dv.flags;
    let bit = |n: u32| f & (1 << n) != 0;
    let kind = match f & 0xF {
        1 => ValidationKind::Whole,
        2 => ValidationKind::Decimal,
        3 => ValidationKind::List,
        4 => ValidationKind::Date,
        5 => ValidationKind::Time,
        6 => ValidationKind::TextLength,
        7 => ValidationKind::Custom,
        // 0, and the undefined 8-15, which Excel opens as "any value".
        _ => ValidationKind::Any,
    };
    let error_style = match (f >> 4) & 0x7 {
        0 => ErrorStyle::Stop,
        1 => ErrorStyle::Warning,
        2 => ErrorStyle::Information,
        _ => return None,
    };
    // The operator is undefined for "any value", lists and custom formulas, whatever it holds.
    let compares = !matches!(kind, ValidationKind::Any | ValidationKind::List | ValidationKind::Custom);
    let op = if compares {
        match (f >> 20) & 0xF {
            0 => CfOperator::Between,
            1 => CfOperator::NotBetween,
            2 => CfOperator::Equal,
            3 => CfOperator::NotEqual,
            4 => CfOperator::Greater,
            5 => CfOperator::Less,
            6 => CfOperator::GreaterOrEqual,
            7 => CfOperator::LessOrEqual,
            _ => return None,
        }
    } else {
        CfOperator::Between
    };
    let text = |rgce: &[u8]| decode(rgce, &[], base, Mode::Cell, links).ok().map(|e| gridcraft_formula::print(&e));
    let f1 = match kind {
        // "Any value" has no formula.
        ValidationKind::Any => String::new(),
        _ if dv.rgce1.is_empty() => String::new(),
        ValidationKind::List if bit(7) => match literal_list(&dv.rgce1) {
            Some(list) => list,
            None => format!("={}", text(&dv.rgce1)?),
        },
        ValidationKind::List => format!("={}", text(&dv.rgce1)?),
        _ => text(&dv.rgce1)?,
    };
    // The upper bound of "between" and "not between".
    let f2 =
        if compares && matches!(op, CfOperator::Between | CfOperator::NotBetween) && !dv.rgce2.is_empty() { Some(text(&dv.rgce2)?) } else { None };
    Some(Validation {
        ranges: dv.ranges,
        kind,
        op,
        f1,
        f2,
        allow_blank: bit(8),
        // fSuppressCombo hides a list's in-cell arrow; it means nothing for other kinds.
        in_cell_dropdown: !(kind == ValidationKind::List && bit(9)),
        input_title: dv.prompt_title,
        input_message: dv.prompt,
        show_input: bit(18),
        error_title: dv.error_title,
        error_message: dv.error,
        error_style,
        show_error: bit(19),
    })
}

/// A literal list: one PtgStr holding the items separated by NUL characters. It is stored the
/// way `.xlsx` stores it: in quotes, the items separated by commas, each `"` doubled. `None`
/// when the formula isn't a lone PtgStr.
fn literal_list(rgce: &[u8]) -> Option<String> {
    let (&ptg, rest) = rgce.split_first()?;
    let (&cch, rest) = rest.split_first()?;
    let (&flags, chars) = rest.split_first()?;
    if ptg != PTG_STR {
        return None;
    }
    let cch = usize::from(cch);
    let s = if flags & 1 != 0 {
        let units: Vec<u16> = chars.as_chunks::<2>().0.iter().map(|u| u16::from_le_bytes(*u)).collect();
        if units.len() != cch || chars.len() != cch * 2 {
            return None;
        }
        String::from_utf16_lossy(&units)
    } else {
        if chars.len() != cch {
            return None;
        }
        chars.iter().map(|&b| char::from(b)).collect()
    };
    let items: Vec<String> = s.split('\0').map(|item| item.replace('"', "\"\"")).collect();
    Some(format!("\"{}\"", items.join(",")))
}
