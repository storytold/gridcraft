//! BIFF12 parsed formulas ([MS-XLSB] 2.2.2, 2.5.98): reverse-Polish `Ptg` tokens back to an
//! expression tree, for cell formulas. The tokens are BIFF8's, but nearly every operand differs
//! (32-bit rows, 14-bit columns, UTF-16 strings, its own array-constant codes, newer functions
//! called through hidden names), so this is its own decoder. It shares the function table, the
//! depth guards and the limits with `xls/formula.rs`. Anything outside the token set it knows is
//! reported as unsupported, and the caller keeps the cell's cached value instead.
//!
//! Names keep their file spelling (`_xlfn.CONCAT`, `_xlpm.x`): the caller passes the printed
//! text through `fmla::from_file`, as the `.xlsx` reader does.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS};
use gridcraft_formula::{Anchor, BinOp, Expr, RefKind, Reference, SheetSel, UnOp};

use crate::xls::formula::{MAX_ARRAY, MAX_RGCE, Stack, Unsupported, error_code, function, pop, pop_n, push};

/// A supporting link record ([MS-XLSB] 2.2.7.3), in the order of the workbook's externals block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Book {
    /// `BrtSupSelf`: this workbook.
    This,
    /// `BrtSupSame`: the caller's sheet (macro sheets and names only).
    Same,
    /// `BrtSupAddin`: add-in functions.
    Addin,
    /// `BrtSupBookSrc`: another workbook, DDE or OLE. Never followed.
    External,
}

/// Xti ([MS-XLSB] 2.5.173): a supporting link and a range of its sheets.
#[derive(Clone, Copy, Debug)]
pub(super) struct Xti {
    pub book: u32,
    pub first: i32,
    pub last: i32,
}

/// A `BrtName` as far as cell formulas need it.
#[derive(Clone, Debug)]
pub(super) struct Name {
    /// The name as written (`_xlfn.CONCAT`, `_xlpm.x`, `Rate`).
    pub text: String,
    /// `fFutureFunction`: a hidden name standing for a function newer than the file format.
    pub future: bool,
}

/// Workbook-level tables formulas refer to.
#[derive(Default)]
pub(super) struct Links {
    /// Every sheet's name in `BrtBundleSh` order, `None` for sheets the import skips.
    pub sheets: Vec<Option<String>>,
    /// Every `BrtName`, in order: `PtgName` indexes them from 1.
    pub names: Vec<Name>,
    pub books: Vec<Book>,
    pub xti: Vec<Xti>,
}

/// Whether a name stands for a LET parameter (`_xlpm.x`), which only the formula itself defines.
fn is_param(name: &str) -> bool {
    name.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("_xlpm."))
}

impl Links {
    /// The sheets a 3-D reference names, or `None` for a deleted sheet (the reference is `#REF!`).
    fn sheet_sel(&self, ixti: u16) -> Result<Option<SheetSel>, Unsupported> {
        let x = self.xti.get(usize::from(ixti)).ok_or(Unsupported("reference to a missing sheet link"))?;
        match usize::try_from(x.book).ok().and_then(|b| self.books.get(b)) {
            Some(Book::This) => {}
            Some(Book::External) => return Err(Unsupported("references to other workbooks (external links)")),
            Some(_) => return Err(Unsupported("references through add-in or same-sheet links")),
            None => return Err(Unsupported("reference to a missing workbook link")),
        }
        // -1: "the first sheet in the reference cannot be found" (a deleted sheet).
        if x.first == -1 || x.last == -1 {
            return Ok(None);
        }
        // -2 (the whole workbook) is for names, not cell references.
        let (Ok(first), Ok(last)) = (usize::try_from(x.first), usize::try_from(x.last)) else {
            return Err(Unsupported("reference to a missing sheet"));
        };
        // A span takes every sheet between its ends, so a skipped one anywhere in it changes
        // what the reference means.
        let span = self.sheets.get(first..=last).filter(|s| !s.is_empty()).ok_or(Unsupported("reference to a missing sheet"))?;
        if span.iter().any(Option::is_none) {
            return Err(Unsupported("references to sheets GridCraft skipped"));
        }
        let (Some(Some(a)), Some(Some(b))) = (span.first(), span.last()) else {
            return Err(Unsupported("reference to a missing sheet"));
        };
        Ok(Some(if first == last { SheetSel::Named(a.clone()) } else { SheetSel::Span(a.clone(), b.clone()) }))
    }
}

struct Bytes<'a> {
    b: &'a [u8],
    pos: usize,
}

const TRUNC: Unsupported = Unsupported("truncated formula");

impl Bytes<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], Unsupported> {
        let s = self.b.get(self.pos..self.pos.checked_add(n).ok_or(TRUNC)?).ok_or(TRUNC)?;
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, Unsupported> {
        self.take(1)?.first().copied().ok_or(TRUNC)
    }
    fn u16(&mut self) -> Result<u16, Unsupported> {
        let s: [u8; 2] = self.take(2)?.try_into().map_err(|_| TRUNC)?;
        Ok(u16::from_le_bytes(s))
    }
    fn u32(&mut self) -> Result<u32, Unsupported> {
        let s: [u8; 4] = self.take(4)?.try_into().map_err(|_| TRUNC)?;
        Ok(u32::from_le_bytes(s))
    }
    fn f64(&mut self) -> Result<f64, Unsupported> {
        let s: [u8; 8] = self.take(8)?.try_into().map_err(|_| TRUNC)?;
        let n = f64::from_le_bytes(s);
        if !n.is_finite() {
            return Err(Unsupported("invalid number constant"));
        }
        Ok(n)
    }
    /// `cch` UTF-16 code units; unpaired surrogates are refused, not replaced.
    fn chars(&mut self, cch: usize) -> Result<String, Unsupported> {
        let units = self.take(cch.checked_mul(2).ok_or(TRUNC)?)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c));
        char::decode_utf16(units).collect::<Result<String, _>>().map_err(|_| Unsupported("invalid text constant"))
    }
}

/// The last row and column of the grid: a range from 0 to these is a whole column or row.
const LAST_ROW: u32 = MAX_ROWS - 1;
const LAST_COL: u32 = MAX_COLS - 1;

/// Where a formula lives: a cell formula's references hold coordinates; a shared formula's
/// relative tokens (`PtgRefN`, `PtgAreaN`) hold offsets from the shared formula's first cell.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Cell,
    Shared(CellRef),
}

/// RgceLoc ([MS-XLSB] 2.5.98.91): a row and a `ColRelShort` (14-bit column, then the
/// column-relative and row-relative flags). In a cell formula the coordinates are absolute
/// whatever the flags say; the flags only decide where `$` goes.
fn anchor(row: u32, col: u16) -> Result<Anchor, Unsupported> {
    let c = u32::from(col & 0x3FFF);
    if row > LAST_ROW || c > LAST_COL {
        return Err(Unsupported("reference outside the grid"));
    }
    Ok(Anchor { row, col: c, row_abs: col & 0x8000 == 0, col_abs: col & 0x4000 == 0 })
}

/// RgceLocRel ([MS-XLSB] 2.5.98.92), in a shared formula: a relative row or column is an offset
/// from `base`, wrapping around the grid (1,048,576 rows, 16,384 columns), as Excel adds them.
/// An absolute one is a coordinate.
fn relative_anchor(row: u32, col: u16, base: CellRef) -> Result<Anchor, Unsupported> {
    let (row_rel, col_rel) = (col & 0x8000 != 0, col & 0x4000 != 0);
    let r = if row_rel { base.row.wrapping_add(row) % MAX_ROWS } else { row };
    let c = if col_rel { (base.col + u32::from(col & 0x3FFF)) % MAX_COLS } else { u32::from(col & 0x3FFF) };
    anchor(r, (c as u16) | (col & 0xC000))
}

fn loc(row: u32, col: u16, mode: Mode) -> Result<Anchor, Unsupported> {
    match mode {
        Mode::Cell => anchor(row, col),
        Mode::Shared(base) => relative_anchor(row, col, base),
    }
}

/// RgceArea: a range spanning every row (or column) is a whole-column (row) reference.
fn area(r0: u32, r1: u32, c0: u16, c1: u16) -> Result<RefKind, Unsupported> {
    area_of(anchor(r0, c0)?, anchor(r1, c1)?)
}

fn area_of(a: Anchor, b: Anchor) -> Result<RefKind, Unsupported> {
    Ok(if a.row == 0 && b.row == LAST_ROW {
        RefKind::Cols(a.col, a.col_abs, b.col, b.col_abs)
    } else if a.col == 0 && b.col == LAST_COL {
        RefKind::Rows(a.row, a.row_abs, b.row, b.row_abs)
    } else {
        RefKind::Range(a, b)
    })
}

fn reference(sheet: Option<SheetSel>, kind: RefKind) -> Expr {
    match sheet {
        Some(sheet) => Expr::Ref(Reference { sheet, kind }),
        None => Expr::Error(CellError::Ref),
    }
}

/// Decodes a formula's `rgce` (tokens) with its `rgcb` (extra data for array constants and
/// cached areas). Returns the expression, with names in their file spelling, or why it can't be
/// represented. A shared formula is decoded for its first cell (`Mode::Shared`): the other cells
/// shift its relative references, as `.xlsx` shared formulas are read.
pub(super) fn decode(rgce: &[u8], rgcb: &[u8], links: &Links, mode: Mode) -> Result<Expr, Unsupported> {
    if rgce.len() > MAX_RGCE {
        return Err(Unsupported("formula too long"));
    }
    let mut t = Bytes { b: rgce, pos: 0 };
    let mut extra = Bytes { b: rgcb, pos: 0 };
    let mut stack: Stack = Vec::new();
    while t.pos < rgce.len() {
        let ptg = t.u8()?;
        let mut kids = 0usize;
        // Operand classes (reference / value / array) share one token number.
        let id = if ptg & 0x60 != 0 { 0x20 | (ptg & 0x1F) } else { ptg };
        match id {
            0x03..=0x11 => {
                let op = match id {
                    0x03 => BinOp::Add,
                    0x04 => BinOp::Sub,
                    0x05 => BinOp::Mul,
                    0x06 => BinOp::Div,
                    0x07 => BinOp::Pow,
                    0x08 => BinOp::Concat,
                    0x09 => BinOp::Lt,
                    0x0A => BinOp::Le,
                    0x0B => BinOp::Eq,
                    0x0C => BinOp::Ge,
                    0x0D => BinOp::Gt,
                    0x0E => BinOp::Ne,
                    0x0F => BinOp::Intersect,
                    0x10 => BinOp::Union,
                    _ => BinOp::Range,
                };
                let b = pop(&mut stack, &mut kids)?;
                let a = pop(&mut stack, &mut kids)?;
                push(&mut stack, kids, Expr::Binary(op, Box::new(a), Box::new(b)))?;
            }
            0x12..=0x14 => {
                let op = match id {
                    0x12 => UnOp::Plus,
                    0x13 => UnOp::Neg,
                    _ => UnOp::Percent,
                };
                let x = pop(&mut stack, &mut kids)?;
                push(&mut stack, kids, Expr::Unary(op, Box::new(x)))?;
            }
            0x15 => {
                let x = pop(&mut stack, &mut kids)?;
                push(&mut stack, kids, Expr::Paren(Box::new(x)))?;
            }
            0x16 => push(&mut stack, kids, Expr::Missing)?,
            // PtgStr: cch (u16, at most 255) and always UTF-16, unlike BIFF8.
            0x17 => {
                let cch = usize::from(t.u16()?);
                if cch > 255 {
                    return Err(Unsupported("text constant too long"));
                }
                push(&mut stack, kids, Expr::Text(Arc::from(t.chars(cch)?)))?;
            }
            0x19 => {
                let kind = t.u8()?;
                if kind & 0x04 != 0 {
                    // PtgAttrChoose: jump table of cOffset + 1 entries.
                    let n = usize::from(t.u16()?) + 1;
                    t.take(n * 2)?;
                } else {
                    t.take(2)?;
                }
                if kind & 0x10 != 0 {
                    // PtgAttrSum: SUM of the single operand.
                    let x = pop(&mut stack, &mut kids)?;
                    push(&mut stack, kids, Expr::Call("SUM".into(), vec![x]))?;
                }
                // Semi (volatile), If, Goto, Space, Baxcel: no effect on the expression.
            }
            0x1C => {
                let e = t.u8()?;
                push(&mut stack, kids, Expr::Error(error_code(e).ok_or(Unsupported("unknown error constant"))?))?;
            }
            0x1D => push(&mut stack, kids, Expr::Bool(t.u8()? != 0))?,
            0x1E => push(&mut stack, kids, Expr::Number(f64::from(t.u16()?)))?,
            0x1F => {
                let n = t.f64()?;
                push(&mut stack, kids, Expr::Number(n))?;
            }
            0x20 => {
                // PtgArray: 14 unused bytes (Excel leaves junk in them); the values are in rgcb.
                t.take(14)?;
                push(&mut stack, kids, array_constant(&mut extra)?)?;
            }
            0x21 => {
                let iftab = t.u16()?;
                let (name, args) = function(iftab).ok_or(Unsupported("unknown function"))?;
                let n = usize::try_from(args).map_err(|_| Unsupported("unknown function argument count"))?;
                let args = pop_n(&mut stack, n, &mut kids)?;
                push(&mut stack, kids, Expr::Call(name.to_string(), args))?;
            }
            0x22 => {
                let n = usize::from(t.u8()?);
                let tab = t.u16()?;
                if tab & 0x8000 != 0 {
                    return Err(Unsupported("macro sheet commands"));
                }
                let mut args = pop_n(&mut stack, n, &mut kids)?;
                if tab & 0x7FFF == 0x00FF {
                    // A function called by name (a newer function, add-in or LAMBDA): the name
                    // comes first, as a PtgName to a hidden `_xlfn.` name for a newer function.
                    if args.is_empty() {
                        return Err(Unsupported("malformed function call"));
                    }
                    let name = match args.remove(0) {
                        Expr::Name(n) => n,
                        _ => return Err(Unsupported("function call through an expression")),
                    };
                    // `PY(index, …)` runs a script stored elsewhere in the package, which this
                    // import doesn't read. `PY("code", …)` carries its code and becomes a Python cell.
                    let py = name.rsplit('.').next().is_some_and(|n| n.eq_ignore_ascii_case("PY"));
                    if py && !args.first().is_some_and(literal_text) {
                        return Err(Unsupported("Python code stored outside the cell"));
                    }
                    push(&mut stack, kids, Expr::Call(name, args))?;
                } else {
                    let (name, _) = function(tab & 0x7FFF).ok_or(Unsupported("unknown function"))?;
                    push(&mut stack, kids, Expr::Call(name.to_string(), args))?;
                }
            }
            0x23 => {
                // PtgName: a 1-based index into every BrtName, hidden ones included.
                let i = usize::try_from(t.u32()?).ok().and_then(|i| i.checked_sub(1));
                let name = i.and_then(|i| links.names.get(i)).ok_or(Unsupported("reference to a missing defined name"))?;
                // Defined names are not imported from XLSB yet: a cell using one would calculate
                // to #NAME?, so it keeps its saved result. Newer functions and LET parameters are
                // hidden names that the formula text itself defines.
                if !name.future && !is_param(&name.text) {
                    return Err(Unsupported("defined names"));
                }
                push(&mut stack, kids, Expr::Name(name.text.clone()))?;
            }
            0x24 => {
                let (r, c) = (t.u32()?, t.u16()?);
                push(&mut stack, kids, Expr::Ref(Reference { sheet: SheetSel::Current, kind: RefKind::Cell(anchor(r, c)?) }))?;
            }
            0x25 => {
                let (r0, r1, c0, c1) = (t.u32()?, t.u32()?, t.u16()?, t.u16()?);
                push(&mut stack, kids, Expr::Ref(Reference { sheet: SheetSel::Current, kind: area(r0, r1, c0, c1)? }))?;
            }
            0x26 => {
                // PtgMemArea: a cache of the sub-expression that follows. Its rgcb entry
                // (PtgExtraMem) is a count and 16 bytes per cached area: skip both.
                t.take(6)?;
                let n = usize::try_from(extra.u32()?).map_err(|_| TRUNC)?;
                extra.take(n.checked_mul(16).ok_or(TRUNC)?)?;
            }
            // PtgMemErr, PtgMemNoMem: 6 bytes; PtgMemFunc: 2. The sub-expression follows them.
            0x27 | 0x28 => {
                t.take(6)?;
            }
            0x29 => {
                t.take(2)?;
            }
            0x2A => {
                t.take(6)?;
                push(&mut stack, kids, Expr::Error(CellError::Ref))?;
            }
            0x2B => {
                t.take(12)?;
                push(&mut stack, kids, Expr::Error(CellError::Ref))?;
            }
            0x39 => return Err(Unsupported("names in other workbooks or on other sheets")),
            0x3A => {
                let ixti = t.u16()?;
                let (r, c) = (t.u32()?, t.u16()?);
                let kind = RefKind::Cell(anchor(r, c)?);
                push(&mut stack, kids, reference(links.sheet_sel(ixti)?, kind))?;
            }
            0x3B => {
                let ixti = t.u16()?;
                let (r0, r1, c0, c1) = (t.u32()?, t.u32()?, t.u16()?, t.u16()?);
                let kind = area(r0, r1, c0, c1)?;
                push(&mut stack, kids, reference(links.sheet_sel(ixti)?, kind))?;
            }
            0x3C | 0x3D => {
                let ixti = t.u16()?;
                t.take(if id == 0x3C { 6 } else { 12 })?;
                // The sheet link must still be one GridCraft follows: an external #REF! stays cached.
                links.sheet_sel(ixti)?;
                push(&mut stack, kids, Expr::Error(CellError::Ref))?;
            }
            0x01 => return Err(Unsupported("the shared or array formula form")),
            0x02 => return Err(Unsupported("what-if data tables")),
            0x18 => return Err(Unsupported("structured table references")),
            0x2C => {
                let Mode::Shared(_) = mode else { return Err(Unsupported("relative references in a cell formula")) };
                let (r, c) = (t.u32()?, t.u16()?);
                push(&mut stack, kids, Expr::Ref(Reference { sheet: SheetSel::Current, kind: RefKind::Cell(loc(r, c, mode)?) }))?;
            }
            0x2D => {
                let Mode::Shared(_) = mode else { return Err(Unsupported("relative references in a cell formula")) };
                let (r0, r1, c0, c1) = (t.u32()?, t.u32()?, t.u16()?, t.u16()?);
                let kind = area_of(loc(r0, c0, mode)?, loc(r1, c1, mode)?)?;
                push(&mut stack, kids, Expr::Ref(Reference { sheet: SheetSel::Current, kind }))?;
            }
            _ => return Err(Unsupported("unknown formula token")),
        }
        if stack.len() > MAX_RGCE {
            return Err(Unsupported("formula too complex"));
        }
    }
    let (e, _) = stack.pop().ok_or(Unsupported("malformed formula (operand missing)"))?;
    if !stack.is_empty() {
        return Err(Unsupported("malformed formula (unused operands)"));
    }
    Ok(grouped(e, false))
}

/// Text written out in the formula: a string, or strings joined with `&` (how Python code
/// longer than a string constant is written).
fn literal_text(e: &Expr) -> bool {
    match e {
        Expr::Text(_) => true,
        Expr::Binary(BinOp::Concat, a, b) => literal_text(a) && literal_text(b),
        _ => false,
    }
}

/// How tightly an expression binds when printed, as the formula parser reads it: binary
/// operators by [`BinOp::precedence`], prefix `+`/`-` at 60 and `%` at 70 (`-2^2` is `(-2)^2`);
/// anything else is one operand.
fn binding(e: &Expr) -> u8 {
    match e {
        Expr::Binary(op, ..) => op.precedence(),
        Expr::Unary(UnOp::Neg | UnOp::Plus, _) => 60,
        Expr::Unary(UnOp::Percent, _) => 70,
        _ => u8::MAX,
    }
}

fn paren(e: Expr) -> Expr {
    Expr::Paren(Box::new(e))
}

/// The tree with the parentheses its text needs to mean the same. Tokens are reverse Polish, so
/// grouping lives in their order: Excel adds `PtgParen` wherever its text has parentheses, but
/// `1 2 + 3 *` (another writer's, or a crafted file's) would print as `1+2*3`. Binary operators
/// are left-associative; a union in a function's arguments needs parentheses of its own.
/// Recursion follows the decoded tree, which `push` bounds at `MAX_DEPTH` levels.
fn grouped(e: Expr, in_args: bool) -> Expr {
    match e {
        Expr::Binary(op, a, b) => {
            let (a, b) = (grouped(*a, in_args), grouped(*b, in_args));
            let a = if binding(&a) < op.precedence() { paren(a) } else { a };
            let b = if binding(&b) <= op.precedence() { paren(b) } else { b };
            let e = Expr::Binary(op, Box::new(a), Box::new(b));
            if in_args && op == BinOp::Union { paren(e) } else { e }
        }
        Expr::Unary(op, x) => {
            let x = grouped(*x, in_args);
            let needs = match op {
                // The parser reads `-1%` as `-(1%)`, Excel's tokens as `(-1)%`: the same value, so
                // a sign under `%` keeps Excel's spelling.
                UnOp::Neg | UnOp::Plus | UnOp::Percent => binding(&x) < 60,
                _ => false,
            };
            Expr::Unary(op, Box::new(if needs { paren(x) } else { x }))
        }
        Expr::Paren(x) => paren(grouped(*x, false)),
        Expr::Call(name, args) => Expr::Call(name, args.into_iter().map(|a| grouped(a, true)).collect()),
        other => other,
    }
}

/// PtgExtraArray ([MS-XLSB] 2.5.98.41): row and column counts, then `SerAr` values in row-major
/// order. The type codes are not BIFF8's: 0x00 is a number here, not a missing value.
fn array_constant(b: &mut Bytes<'_>) -> Result<Expr, Unsupported> {
    let rows = usize::try_from(b.u32()?).map_err(|_| TRUNC)?;
    let cols = usize::try_from(b.u32()?).map_err(|_| TRUNC)?;
    if rows == 0 || cols == 0 || rows.saturating_mul(cols) > MAX_ARRAY {
        return Err(Unsupported("array constant too large or empty"));
    }
    let mut out = Vec::with_capacity(rows);
    for _ in 0..rows {
        let mut row = Vec::with_capacity(cols);
        for _ in 0..cols {
            row.push(match b.u8()? {
                0x00 => Expr::Number(b.f64()?),
                0x01 => {
                    let cch = usize::from(b.u16()?);
                    if cch > 255 {
                        return Err(Unsupported("text constant too long"));
                    }
                    Expr::Text(Arc::from(b.chars(cch)?))
                }
                0x02 => Expr::Bool(b.u8()? != 0),
                0x04 => {
                    let e = b.u8()?;
                    b.take(3)?;
                    Expr::Error(error_code(e).ok_or(Unsupported("unknown error constant"))?)
                }
                _ => return Err(Unsupported("unknown array constant value")),
            });
        }
        out.push(row);
    }
    Ok(Expr::Array(out))
}
