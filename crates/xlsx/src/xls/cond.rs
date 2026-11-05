//! Conditional formatting in `.xls` files: a CondFmt record names the ranges ([MS-XLS] 2.4.56),
//! and each CF record after it is one rule ([MS-XLS] 2.4.42): "cell value is …" with an
//! operator and one or two formulas, or a formula that is true; its look is a DXFN
//! ([MS-XLS] 2.5.81), where every attribute it doesn't set is left as the cell has it.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::{BorderLine, BorderStyle, CfOperator, Fill, PatternType, Style, Underline};

use super::records::Cursor;
use super::styles::{border_style, color, pattern};
use crate::IoError;

/// Ranges one CondFmt record lists at most.
pub(super) const MAX_CF_RANGES: usize = 4096;

/// A CondFmt record's ranges.
pub(super) fn condfmt(c: &mut Cursor<'_, '_>) -> Result<Vec<RangeRef>, IoError> {
    // Rule count, flags and the bounding range.
    c.skip(12)?;
    let n = usize::from(c.u16()?).min(MAX_CF_RANGES).min(c.remaining() / 8);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (r0, r1, c0, c1) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
        let (r0, r1) = (u32::from(r0.min(r1)), u32::from(r0.max(r1)));
        let (c0, c1) = (u32::from(c0.min(c1)), u32::from(c0.max(c1)));
        out.push(RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)));
    }
    Ok(out)
}

/// One CF record, before its formulas are decoded.
pub(super) struct Cf {
    /// 1: cell value compared; 2: a formula.
    pub kind: u8,
    pub op: Option<CfOperator>,
    pub style: Style,
    pub rgce1: Vec<u8>,
    pub rgce2: Vec<u8>,
}

/// Read a CF record: its kind, operator, look and the two formulas' tokens.
pub(super) fn cf(c: &mut Cursor<'_, '_>, palette: &[u32; 64], num_fmt: impl Fn(u16) -> Option<String>) -> Result<Cf, IoError> {
    let kind = c.u8()?;
    let op = match c.u8()? {
        1 => Some(CfOperator::Between),
        2 => Some(CfOperator::NotBetween),
        3 => Some(CfOperator::Equal),
        4 => Some(CfOperator::NotEqual),
        5 => Some(CfOperator::Greater),
        6 => Some(CfOperator::Less),
        7 => Some(CfOperator::GreaterOrEqual),
        8 => Some(CfOperator::LessOrEqual),
        _ => None,
    };
    let (cce1, cce2) = (usize::from(c.u16()?), usize::from(c.u16()?));
    let style = dxfn(c, palette, num_fmt)?;
    let rest = c.rest();
    let rgce1 = rest.get(..cce1).unwrap_or(&[]).to_vec();
    let rgce2 = rest.get(cce1..cce1.saturating_add(cce2)).unwrap_or(&[]).to_vec();
    Ok(Cf { kind, op, style, rgce1, rgce2 })
}

fn bit(flags: u32, n: u32) -> bool {
    flags & (1 << n) != 0
}

/// A DXFN: which blocks follow (number format, font, alignment, border, pattern, protection)
/// and, per attribute, whether it is left unset ("ninch" bits). Unset attributes stay default.
fn dxfn(c: &mut Cursor<'_, '_>, palette: &[u32; 64], num_fmt: impl Fn(u16) -> Option<String>) -> Result<Style, IoError> {
    let flags = c.u32()?;
    let flags2 = c.u16()?;
    let mut s = Style::default();
    if bit(flags, 25) {
        if flags2 & 1 != 0 {
            // DXFNumUsr: its size, then the format code.
            c.u16()?;
            let code = c.string()?;
            s.num_fmt = gridcraft_model::NumFmt::new(&code);
        } else {
            c.u8()?;
            if let Some(code) = num_fmt(u16::from(c.u8()?)) {
                s.num_fmt = gridcraft_model::NumFmt::new(&code);
            }
        }
    }
    if bit(flags, 26) {
        // DXFFntD, 118 bytes: name (64), height, options (italic 0x02, strikeout 0x80), weight,
        // script, underline, colour, then which of those are unset. Like an .xlsx differential
        // font, the face and size stay the cell's.
        let mut b = [0u8; 118];
        for slot in b.iter_mut() {
            *slot = c.u8()?;
        }
        let u32_at = |at: usize| b.get(at..at + 4).and_then(|x| <[u8; 4]>::try_from(x).ok()).map_or(u32::MAX, u32::from_le_bytes);
        let u16_at = |at: usize| b.get(at..at + 2).and_then(|x| <[u8; 2]>::try_from(x).ok()).map_or(0, u16::from_le_bytes);
        let (options, weight, underline, icv) = (u32_at(68), u16_at(72), b.get(76).copied().unwrap_or(0), u32_at(80));
        let (style_unset, underline_unset, weight_unset) = (u32_at(88), u32_at(96) != 0, u32_at(100) != 0);
        if !weight_unset {
            s.font.bold = weight >= 600;
        }
        if style_unset & 0x02 == 0 {
            s.font.italic = options & 0x02 != 0;
        }
        if style_unset & 0x80 == 0 {
            s.font.strike = options & 0x80 != 0;
        }
        if !underline_unset {
            s.font.underline = match underline {
                1 => Underline::Single,
                2 => Underline::Double,
                0x21 => Underline::SingleAccounting,
                0x22 => Underline::DoubleAccounting,
                _ => Underline::None,
            };
        }
        if let Ok(icv) = u16::try_from(icv) {
            s.font.color = color(icv, palette);
        }
    }
    if bit(flags, 27) {
        c.skip(8)?;
    }
    if bit(flags, 28) {
        let (a, b) = (c.u32()?, c.u32()?);
        let line = |unset: bool, dg: u32, icv: u32| {
            let style = border_style(dg & 0x0F);
            if unset || style == BorderStyle::None { BorderLine::default() } else { BorderLine { style, color: color((icv & 0x7F) as u16, palette) } }
        };
        s.borders.left = line(bit(flags, 10), a, a >> 16);
        s.borders.right = line(bit(flags, 11), a >> 4, a >> 23);
        s.borders.top = line(bit(flags, 12), a >> 8, b);
        s.borders.bottom = line(bit(flags, 13), a >> 12, b >> 7);
    }
    if bit(flags, 29) {
        let (p1, p2) = (c.u16()?, c.u16()?);
        let (pattern_unset, fore_unset, back_unset) = (bit(flags, 16), bit(flags, 17), bit(flags, 18));
        let (fore, back) = (color(p2 & 0x7F, palette), color((p2 >> 7) & 0x7F, palette));
        // An unset pattern with a colour is a solid fill; a solid fill shows its background
        // colour (the differential-format convention .xlsx shares).
        let kind = if pattern_unset { PatternType::Solid } else { pattern((p1 >> 10) & 0x3F) };
        if kind == PatternType::Solid {
            let shown = if back_unset { fore } else { back };
            if !(back_unset && fore_unset) {
                s.fill = Fill::solid(shown);
            }
        } else if kind != PatternType::None {
            s.fill = Fill { pattern: kind, fg: fore, bg: back };
        }
    }
    if bit(flags, 30) {
        c.skip(2)?;
    }
    Ok(s)
}
