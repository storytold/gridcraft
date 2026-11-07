//! BIFF8 parsed formulas ([MS-XLS] 2.2.2, 2.5.198): reverse-Polish `Ptg` tokens back to an
//! expression tree. Anything outside the common token set is reported as unsupported, and the
//! caller keeps the cell's cached value instead.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef};
use gridcraft_formula::{Anchor, BinOp, Expr, RefKind, Reference, SheetSel, UnOp};

use super::ftab::FTAB;

/// Formula token bytes decoded at most (Excel's limit is 1,800 per formula).
const MAX_RGCE: usize = 16 * 1024;
/// Values in one array constant at most.
const MAX_ARRAY: usize = 65_536;

/// Why a formula could not be turned back into text.
#[derive(Debug, PartialEq)]
pub(super) struct Unsupported(pub &'static str);

/// Where the formula lives: refs in cell formulas store coordinates; shared formulas store
/// row/column offsets for relative parts; names store offsets for 3-D refs too.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Cell,
    Shared,
    Name,
}

/// A supporting link ([MS-XLS] 2.4.271): only links to this workbook (and add-in function
/// names) are resolved; external workbooks are never followed.
#[derive(Default)]
pub(super) struct SupBook {
    pub internal: bool,
    pub addin: bool,
    pub names: Vec<String>,
}

/// XTI ([MS-XLS] 2.5.344): a supporting link and a range of its sheets.
#[derive(Clone, Copy)]
pub(super) struct Xti {
    pub book: u16,
    pub first: i16,
    pub last: i16,
}

/// Workbook-level tables formulas refer to.
#[derive(Default)]
pub(super) struct Links {
    /// Every sheet's name in BoundSheet8 order (including skipped chart/macro sheets).
    pub sheets: Vec<String>,
    /// Defined names (Lbl records), in order; `None` for names that can't be referenced.
    pub names: Vec<String>,
    /// Per defined name: whether the import skips it (its formula can't be decoded).
    pub skipped: Vec<bool>,
    /// Per defined name: whether it names a macro or a user-defined function (Lbl `fFunc` or
    /// `fProc`, [MS-XLS] 2.4.150). Never imported: the workbook's VBA project isn't read.
    pub macros: Vec<bool>,
    pub books: Vec<SupBook>,
    pub xti: Vec<Xti>,
}

pub(super) fn function(iftab: u16) -> Option<(&'static str, i8)> {
    FTAB.binary_search_by_key(&iftab, |(i, _, _)| *i).ok().and_then(|i| FTAB.get(i)).map(|(_, n, a)| (*n, *a))
}

fn error_code(code: u8) -> Option<CellError> {
    Some(match code {
        0x00 => CellError::Null,
        0x07 => CellError::Div0,
        0x0F => CellError::Value,
        0x17 => CellError::Ref,
        0x1D => CellError::Name,
        0x24 => CellError::Num,
        0x2A => CellError::NA,
        0x2B => CellError::GettingData,
        _ => return None,
    })
}

pub(super) fn cell_error(code: u8) -> CellError {
    error_code(code).unwrap_or(CellError::NA)
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
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([*s.first().ok_or(TRUNC)?, *s.get(1).ok_or(TRUNC)?]))
    }
    fn u32(&mut self) -> Result<u32, Unsupported> {
        Ok(u32::from(self.u16()?) | (u32::from(self.u16()?) << 16))
    }
    fn f64(&mut self) -> Result<f64, Unsupported> {
        let a: [u8; 8] = self.take(8)?.try_into().map_err(|_| TRUNC)?;
        Ok(f64::from_le_bytes(a))
    }
    fn chars(&mut self, cch: usize, high: bool) -> Result<String, Unsupported> {
        let units: Vec<u16> = if high {
            self.take(cch.checked_mul(2).ok_or(TRUNC)?)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
        } else {
            self.take(cch)?.iter().map(|&b| u16::from(b)).collect()
        };
        Ok(String::from_utf16_lossy(&units))
    }
}

/// A column field (ColRelU / ColRelNegU): the 14-bit column and the two relative flags.
fn col_field(v: u16) -> (u16, bool, bool) {
    (v & 0x3FFF, v & 0x4000 != 0, v & 0x8000 != 0)
}

/// One end of a reference. In `relative` (RgceLocRel) form the relative parts are offsets
/// from `base`, wrapped into the BIFF8 grid (65,536 × 256).
fn anchor(row: u16, col: u16, base: CellRef, relative: bool) -> Anchor {
    let (c, col_rel, row_rel) = col_field(col);
    let (r, c) = if relative {
        let r = if row_rel { (u32::from(row).wrapping_add(base.row)) & 0xFFFF } else { u32::from(row) };
        // A 14-bit signed offset; adding modulo 256 gives the same column either way.
        let c = if col_rel { (u32::from(c).wrapping_add(base.col)) & 0xFF } else { u32::from(c) & 0xFF };
        (r, c)
    } else {
        (u32::from(row), u32::from(c) & 0xFF)
    };
    Anchor { row: r, col: c, row_abs: !row_rel, col_abs: !col_rel }
}

/// An area; a range spanning every BIFF8 row (or column) becomes a whole-column (row) ref.
fn area(r0: u16, r1: u16, c0: u16, c1: u16, base: CellRef, relative: bool) -> RefKind {
    let a = anchor(r0, c0, base, relative);
    let b = anchor(r1, c1, base, relative);
    if a.row == 0 && b.row == 0xFFFF {
        return RefKind::Cols(a.col, a.col_abs, b.col, b.col_abs);
    }
    if a.col == 0 && b.col == 0xFF {
        return RefKind::Rows(a.row, a.row_abs, b.row, b.row_abs);
    }
    RefKind::Range(a, b)
}

impl Links {
    fn sheet_sel(&self, ixti: u16) -> Result<Option<SheetSel>, Unsupported> {
        let x = self.xti.get(usize::from(ixti)).ok_or(Unsupported("reference to a missing sheet link"))?;
        let book = self.books.get(usize::from(x.book)).ok_or(Unsupported("reference to a missing workbook link"))?;
        if !book.internal {
            return Err(Unsupported("references to other workbooks (external links)"));
        }
        let name = |i: i16| usize::try_from(i).ok().and_then(|i| self.sheets.get(i)).cloned();
        Ok(match (name(x.first), name(x.last)) {
            (Some(a), Some(_)) if x.first == x.last => Some(SheetSel::Named(a)),
            (Some(a), Some(b)) => Some(SheetSel::Span(a, b)),
            // A deleted sheet: the reference is #REF!.
            _ => None,
        })
    }

    fn extern_name(&self, ixti: u16, index: u32) -> Result<String, Unsupported> {
        let x = self.xti.get(usize::from(ixti)).ok_or(Unsupported("reference to a missing name link"))?;
        let book = self.books.get(usize::from(x.book)).ok_or(Unsupported("reference to a missing workbook link"))?;
        if !book.internal && !book.addin {
            return Err(Unsupported("names in other workbooks (external links)"));
        }
        let i = usize::try_from(index).ok().and_then(|i| i.checked_sub(1)).ok_or(Unsupported("invalid name index"))?;
        book.names.get(i).cloned().ok_or(Unsupported("reference to a missing name"))
    }
}

/// Strips the prefixes newer functions carry in files (`_xlfn.IFERROR`).
fn function_name(name: &str) -> String {
    let n = name.strip_prefix("_xlfn.").or_else(|| name.strip_prefix("_xlws.")).unwrap_or(name);
    n.to_ascii_uppercase()
}

/// Expression nesting at most, as in the formula parser: deeper trees would overflow the stack
/// when printed, evaluated or dropped (a hostile file can chain thousands of PtgParen tokens).
const MAX_DEPTH: usize = 256;

/// Operand stack entries: an expression and its nesting depth.
type Stack = Vec<(Expr, usize)>;

/// Pops an operand, recording its depth in `kids` (the deepest operand this token consumed).
fn pop(stack: &mut Stack, kids: &mut usize) -> Result<Expr, Unsupported> {
    let (e, d) = stack.pop().ok_or(Unsupported("malformed formula (operand missing)"))?;
    *kids = (*kids).max(d);
    Ok(e)
}

fn pop_n(stack: &mut Stack, n: usize, kids: &mut usize) -> Result<Vec<Expr>, Unsupported> {
    let at = stack.len().checked_sub(n).ok_or(Unsupported("malformed formula (argument missing)"))?;
    let popped = stack.split_off(at);
    *kids = popped.iter().map(|(_, d)| *d).fold(*kids, usize::max);
    Ok(popped.into_iter().map(|(e, _)| e).collect())
}

/// Pushes the result of a token one level above the operands it consumed.
fn push(stack: &mut Stack, kids: usize, e: Expr) -> Result<(), Unsupported> {
    let depth = kids.saturating_add(1);
    if depth > MAX_DEPTH {
        return Err(Unsupported("formula nested too deeply"));
    }
    stack.push((e, depth));
    Ok(())
}

/// Decodes `rgce` (tokens) with `rgcb` (the extra data for array constants) evaluated at
/// `base`. Returns the expression or why it can't be represented.
pub(super) fn decode(rgce: &[u8], rgcb: &[u8], base: CellRef, mode: Mode, links: &Links) -> Result<Expr, Unsupported> {
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
            0x17 => {
                let cch = usize::from(t.u8()?);
                let high = t.u8()? & 1 != 0;
                push(&mut stack, kids, Expr::Text(Arc::from(t.chars(cch, high)?)))?;
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
                if !n.is_finite() {
                    return Err(Unsupported("invalid number constant"));
                }
                push(&mut stack, kids, Expr::Number(n))?;
            }
            0x20 => {
                t.take(7)?;
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
                    // A function called by name (add-in or newer function): the name comes first.
                    if args.is_empty() {
                        return Err(Unsupported("malformed function call"));
                    }
                    let name = match args.remove(0) {
                        Expr::Name(n) => function_name(&n),
                        _ => return Err(Unsupported("function call through an expression")),
                    };
                    push(&mut stack, kids, Expr::Call(name, args))?;
                } else {
                    let (name, _) = function(tab & 0x7FFF).ok_or(Unsupported("unknown function"))?;
                    push(&mut stack, kids, Expr::Call(name.to_string(), args))?;
                }
            }
            0x23 => {
                let i = usize::try_from(t.u32()?).ok().and_then(|i| i.checked_sub(1));
                let name = i.and_then(|i| links.names.get(i)).filter(|n| !n.is_empty()).ok_or(Unsupported("reference to a missing defined name"))?;
                // A call to a VBA function (`=DIAGONALWERT(I11)`: PtgName to a macro name, then
                // PtgFuncVar 255) can't run here and would calculate to #NAME?: the cell keeps its
                // saved result, and a defined name that calls one is skipped like the others.
                if i.and_then(|i| links.macros.get(i)) == Some(&true) {
                    return Err(Unsupported("user-defined functions (VBA macros)"));
                }
                // A cell using a name the import drops would calculate to #NAME?: keep its
                // saved result instead, as for references to other workbooks.
                if mode != Mode::Name && i.and_then(|i| links.skipped.get(i)) == Some(&true) {
                    return Err(Unsupported("defined names GridCraft skipped"));
                }
                push(&mut stack, kids, Expr::Name(name.clone()))?;
            }
            0x24 | 0x2C => {
                let (r, c) = (t.u16()?, t.u16()?);
                let a = anchor(r, c, base, id == 0x2C);
                push(&mut stack, kids, Expr::Ref(Reference { sheet: SheetSel::Current, kind: RefKind::Cell(a) }))?;
            }
            0x25 | 0x2D => {
                let (r0, r1, c0, c1) = (t.u16()?, t.u16()?, t.u16()?, t.u16()?);
                let kind = area(r0, r1, c0, c1, base, id == 0x2D);
                push(&mut stack, kids, Expr::Ref(Reference { sheet: SheetSel::Current, kind }))?;
            }
            0x26 => {
                // PtgMemArea: a cache of the sub-expression that follows; skip its extra data.
                t.take(6)?;
                let n = usize::from(extra.u16()?);
                extra.take(n.checked_mul(8).ok_or(TRUNC)?)?;
            }
            0x27 | 0x28 => {
                t.take(6)?;
            }
            0x29 => {
                t.take(2)?;
            }
            0x2A => {
                t.take(4)?;
                push(&mut stack, kids, Expr::Error(CellError::Ref))?;
            }
            0x2B => {
                t.take(8)?;
                push(&mut stack, kids, Expr::Error(CellError::Ref))?;
            }
            0x39 => {
                let ixti = t.u16()?;
                let i = t.u32()?;
                push(&mut stack, kids, Expr::Name(links.extern_name(ixti, i)?))?;
            }
            0x3A => {
                let ixti = t.u16()?;
                let (r, c) = (t.u16()?, t.u16()?);
                let a = anchor(r, c, base, mode == Mode::Name);
                push(
                    &mut stack,
                    kids,
                    match links.sheet_sel(ixti)? {
                        Some(sheet) => Expr::Ref(Reference { sheet, kind: RefKind::Cell(a) }),
                        None => Expr::Error(CellError::Ref),
                    },
                )?;
            }
            0x3B => {
                let ixti = t.u16()?;
                let (r0, r1, c0, c1) = (t.u16()?, t.u16()?, t.u16()?, t.u16()?);
                let kind = area(r0, r1, c0, c1, base, mode == Mode::Name);
                push(
                    &mut stack,
                    kids,
                    match links.sheet_sel(ixti)? {
                        Some(sheet) => Expr::Ref(Reference { sheet, kind }),
                        None => Expr::Error(CellError::Ref),
                    },
                )?;
            }
            0x3C => {
                t.take(6)?;
                push(&mut stack, kids, Expr::Error(CellError::Ref))?;
            }
            0x3D => {
                t.take(10)?;
                push(&mut stack, kids, Expr::Error(CellError::Ref))?;
            }
            0x01 => return Err(Unsupported("nested shared or array formula reference")),
            0x02 => return Err(Unsupported("what-if data tables")),
            0x18 => return Err(Unsupported("natural-language formulas")),
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
    Ok(e)
}

/// PtgExtraArray ([MS-XLS] 2.5.198.59): an array constant's values in row-major order.
fn array_constant(b: &mut Bytes<'_>) -> Result<Expr, Unsupported> {
    let cols = usize::from(b.u8()?) + 1;
    let rows = usize::from(b.u16()?) + 1;
    if cols.saturating_mul(rows) > MAX_ARRAY {
        return Err(Unsupported("array constant too large"));
    }
    let mut out = Vec::with_capacity(rows);
    for _ in 0..rows {
        let mut row = Vec::with_capacity(cols);
        for _ in 0..cols {
            let kind = b.u8()?;
            row.push(match kind {
                0x00 => {
                    b.take(8)?;
                    Expr::Missing
                }
                0x01 => {
                    let n = b.f64()?;
                    if !n.is_finite() {
                        return Err(Unsupported("invalid number constant"));
                    }
                    Expr::Number(n)
                }
                0x02 => {
                    let cch = usize::from(b.u16()?);
                    let high = b.u8()? & 1 != 0;
                    Expr::Text(Arc::from(b.chars(cch, high)?))
                }
                0x04 => {
                    let v = b.u8()?;
                    b.take(7)?;
                    Expr::Bool(v != 0)
                }
                0x10 => {
                    let e = b.u8()?;
                    b.take(7)?;
                    Expr::Error(error_code(e).ok_or(Unsupported("unknown error constant"))?)
                }
                _ => return Err(Unsupported("unknown array constant value")),
            });
        }
        out.push(row);
    }
    Ok(Expr::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links() -> Links {
        Links {
            sheets: vec!["Sheet1".into(), "My Data".into()],
            names: vec!["Rate".into(), "MyUdf".into()],
            skipped: vec![false, false],
            macros: vec![false, true],
            books: vec![
                SupBook { internal: true, addin: false, names: vec![] },
                SupBook { internal: false, addin: true, names: vec!["_xlfn.IFERROR".into()] },
                SupBook::default(),
            ],
            xti: vec![
                Xti { book: 0, first: 0, last: 1 },
                Xti { book: 1, first: -2, last: -2 },
                Xti { book: 2, first: 0, last: 0 },
                Xti { book: 0, first: -1, last: -1 },
            ],
        }
    }

    fn text(rgce: &[u8], rgcb: &[u8]) -> Result<String, Unsupported> {
        decode(rgce, rgcb, CellRef::new(0, 0), Mode::Cell, &links()).map(|e| gridcraft_formula::print(&e))
    }

    #[test]
    fn decodes_common_tokens() {
        // {1,"a";TRUE,#N/A} via PtgArray + PtgExtraArray.
        let extra =
            [&[1u8, 1, 0][..], &[1], &1f64.to_le_bytes(), &[2, 1, 0, 0, b'a'], &[4, 1, 0, 0, 0, 0, 0, 0, 0], &[0x10, 0x2A, 0, 0, 0, 0, 0, 0, 0]]
                .concat();
        assert_eq!(text(&[0x60, 0, 0, 0, 0, 0, 0, 0], &extra).unwrap(), "{1,\"a\";TRUE,#N/A}");
        // SUM via PtgAttrSum over a 3-D span, whole column.
        let area3d = [&[0x3Bu8][..], &0u16.to_le_bytes(), &[0, 0, 0xFF, 0xFF, 1, 0, 1, 0]].concat();
        assert_eq!(text(&[area3d.as_slice(), &[0x19, 0x10, 0, 0]].concat(), &[]).unwrap(), "SUM('Sheet1:My Data'!$B:$B)");
        // IFERROR (a newer function) called by name through PtgNameX, with a missing argument.
        let namex = [&[0x39u8][..], &1u16.to_le_bytes(), &1u32.to_le_bytes()].concat();
        let call = [namex.as_slice(), &[0x1E, 5, 0, 0x16, 0x22, 3, 0xFF, 0]].concat();
        assert_eq!(text(&call, &[]).unwrap(), "IFERROR(5,)");
        // A VBA function called by name (PtgName to a macro name) is refused: it can't run here.
        let udf = [&[0x23u8][..], &2u32.to_le_bytes(), &[0x1E, 1, 0, 0x22, 2, 0xFF, 0]].concat();
        assert_eq!(text(&udf, &[]), Err(Unsupported("user-defined functions (VBA macros)")));
        // A deleted sheet becomes #REF!; the CHOOSE jump table is skipped.
        let ref_deleted = [&[0x3Au8][..], &3u16.to_le_bytes(), &[0, 0, 0, 0]].concat();
        let choose = [&[0x1Eu8, 1, 0][..], &[0x19, 0x04, 1, 0, 0, 0, 0, 0], ref_deleted.as_slice(), &[0x1E, 2, 0, 0x22, 3, 100, 0]].concat();
        assert_eq!(text(&choose, &[]).unwrap(), "CHOOSE(1,#REF!,2)");
        // External links, data tables and unbalanced stacks are refused, never guessed.
        let external = [&[0x3Au8][..], &2u16.to_le_bytes(), &[0, 0, 0, 0]].concat();
        assert!(text(&external, &[]).is_err());
        assert!(text(&[0x02, 0, 0, 0, 0], &[]).is_err());
        assert!(text(&[0x1E, 1, 0, 0x1E, 2, 0], &[]).is_err());
        assert!(text(&[0x03], &[]).is_err());
        assert!(text(&[0x21, 0xFF, 0x7F], &[]).is_err());
        assert!(text(&[0x60, 0, 0, 0, 0, 0, 0, 0], &[0xFF, 0xFF, 0xFF]).is_err());
    }

    #[test]
    fn deep_nesting_is_refused_not_a_stack_overflow() {
        // 1 followed by thousands of PtgParen / PtgUminus tokens: a hostile file used to build a
        // 16K-deep tree that overflowed the stack when printed or dropped.
        for op in [0x15u8, 0x13] {
            let mut rgce = vec![0x1E, 1, 0];
            rgce.resize(MAX_RGCE, op);
            assert_eq!(text(&rgce, &[]), Err(Unsupported("formula nested too deeply")));
        }
        // Binary chains nest too: 1+1+1+... left-associated.
        let mut chain = vec![0x1E, 1, 0];
        while chain.len() + 4 <= MAX_RGCE {
            chain.extend([0x1E, 1, 0, 0x03]);
        }
        assert!(text(&chain, &[]).is_err());
        // Nesting up to the limit still decodes.
        let mut ok = vec![0x1E, 1, 0];
        ok.resize(3 + MAX_DEPTH - 1, 0x15);
        assert!(text(&ok, &[]).is_ok());
    }
}
