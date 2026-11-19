//! SYLK (`.slk`) import. `craft_csv::sylk` reads the file (values, formulas rewritten from R1C1 to
//! A1, number formats, column widths); this module makes it a one-sheet workbook: formulas parsed
//! (a formula GridCraft can't parse is skipped and counted), error values recognised, formats
//! interned as styles and widths converted from characters.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

use craft_csv::sylk;
use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::{Cell, Formula, NumFmt, Sheet, Style, Workbook};

use crate::{EngineError, Result};

/// Reads a SYLK file into a one-sheet workbook. Returns the workbook and its warnings.
pub fn read(name: &str, bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
    let file = sylk::read(bytes).map_err(|e| match e {
        sylk::Error::TooLarge => EngineError::Other(format!("We can't open '{name}': the SYLK file is larger than {} MB.", sylk::MAX_BYTES >> 20)),
        _ => EngineError::Other(format!("We can't open '{name}': it isn't a SYLK file (it doesn't start with an ID record).")),
    })?;
    let mut wb = Workbook::new();
    let mut sheet = Sheet::new("Sheet1");
    let mut skipped = file.skipped;
    let mut styles = HashMap::new();
    for (&(row, col), c) in &file.cells {
        let value = match &c.value {
            sylk::Value::Number(n) => Value::Number(*n),
            sylk::Value::Text(s) => Value::Text(Arc::from(decode_sylk_escapes(s, false).as_ref())),
            sylk::Value::Bool(b) => Value::Bool(*b),
            sylk::Value::Error(s) => CellError::parse(s).map_or_else(|| Value::Text(Arc::from(s.as_str())), Value::Error),
            _ => Value::Empty,
        };
        let mut cell = Cell::value(value);
        if let Some(a1) = &c.formula {
            let decoded = decode_sylk_escapes(a1, true);
            let f = Formula::new(decoded.as_ref());
            if f.expr.is_some() {
                cell.formula = Some(Arc::new(f));
            } else {
                skipped += 1;
            }
        }
        if let Some(code) = &c.format {
            let decoded = decode_sylk_escapes(code, true);
            let id = if let Some(&id) = styles.get(decoded.as_ref()) {
                id
            } else {
                let id = wb.styles.intern(Style { num_fmt: NumFmt(Arc::from(decoded.as_ref())), ..Style::default() });
                styles.insert(decoded.into_owned(), id);
                id
            };
            cell.style = id;
        }
        sheet.set_cell(CellRef::new(row, col), cell);
    }
    for w in &file.column_widths {
        let size = crate::cmd::format::chars_to_points(w.width) as f32;
        for c in w.first..=w.last {
            sheet.cols.entry(c).or_default().size = Some(size);
        }
    }
    if let Some(first) = wb.sheet_mut(0) {
        sheet.name = std::mem::take(&mut first.name);
        *first = sheet;
    }
    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(format!("{name}: {skipped} SYLK records GridCraft couldn't read were skipped"));
    }
    Ok((wb, warnings))
}

/// Decodes SYLK 7-bit ISO/IEC 6937 escape sequences (`\x1bN<byte>`).
///
/// `quoted` says the string carries its own quotes as syntax (a formula or a number format)
/// rather than as text: there a `"` right after `\x1bN` with nothing after it is the quote that
/// closes the literal, so the incomplete escape is kept.
fn decode_sylk_escapes(s: &str, quoted: bool) -> Cow<'_, str> {
    if !s.contains('\x1b') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'N') {
            chars.next();
            let Some(code_ch) = chars.next() else {
                out.push('\x1b');
                out.push('N');
                break;
            };
            if !code_ch.is_ascii() || (code_ch as u8) < 0x20 {
                out.push('\x1b');
                out.push('N');
                out.push(code_ch);
                continue;
            }
            // A `"` here may be the quote that closes a formula's or format's literal, not the
            // payload for `¢`: keep the incomplete escape and let the quote close it.
            if quoted && code_ch == '"' && chars.peek().is_none() {
                out.push('\x1b');
                out.push('N');
                out.push(code_ch);
                continue;
            }
            let b = (code_ch as u8) | 0x80;
            if let Some((spacing, combining)) = iso6937_diacritic(b) {
                if let Some(&base) = chars.peek() {
                    if let Some(composed) = compose_iso6937(b, base) {
                        chars.next();
                        out.push(composed);
                    } else if base == ' ' {
                        chars.next();
                        out.push(spacing);
                    } else if base.is_ascii_alphabetic() {
                        chars.next();
                        out.push(base);
                        out.push(combining);
                    } else {
                        out.push(spacing);
                    }
                } else {
                    out.push(spacing);
                }
            } else if let Some(decoded) = iso6937_single(b) {
                out.push(decoded);
            } else {
                out.push('\x1b');
                out.push('N');
                out.push(code_ch);
            }
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
}

/// Maps an ISO/IEC 6937 non-spacing diacritic byte (`0xC1..=0xCF`) to `(spacing, combining)`.
fn iso6937_diacritic(b: u8) -> Option<(char, char)> {
    match b {
        0xC1 => Some(('`', '\u{0300}')),
        0xC2 => Some(('´', '\u{0301}')),
        0xC3 => Some(('^', '\u{0302}')),
        0xC4 => Some(('~', '\u{0303}')),
        0xC5 => Some(('¯', '\u{0304}')),
        0xC6 => Some(('˘', '\u{0306}')),
        0xC7 => Some(('˙', '\u{0307}')),
        0xC8 => Some(('¨', '\u{0308}')),
        0xCA => Some(('˚', '\u{030A}')),
        0xCB => Some(('¸', '\u{0327}')),
        0xCD => Some(('˝', '\u{030B}')),
        0xCE => Some(('˛', '\u{0328}')),
        0xCF => Some(('ˇ', '\u{030C}')),
        _ => None,
    }
}

/// Composes an ISO/IEC 6937 diacritic byte and ASCII base letter into a precomposed NFC char.
fn compose_iso6937(diacritic: u8, base: char) -> Option<char> {
    let ch = match (diacritic, base) {
        (0xC1, 'A') => 'À',
        (0xC1, 'E') => 'È',
        (0xC1, 'I') => 'Ì',
        (0xC1, 'O') => 'Ò',
        (0xC1, 'U') => 'Ù',
        (0xC1, 'a') => 'à',
        (0xC1, 'e') => 'è',
        (0xC1, 'i') => 'ì',
        (0xC1, 'o') => 'ò',
        (0xC1, 'u') => 'ù',
        (0xC2, 'A') => 'Á',
        (0xC2, 'C') => 'Ć',
        (0xC2, 'E') => 'É',
        (0xC2, 'I') => 'Í',
        (0xC2, 'L') => 'Ĺ',
        (0xC2, 'N') => 'Ń',
        (0xC2, 'O') => 'Ó',
        (0xC2, 'R') => 'Ŕ',
        (0xC2, 'S') => 'Ś',
        (0xC2, 'U') => 'Ú',
        (0xC2, 'Y') => 'Ý',
        (0xC2, 'Z') => 'Ź',
        (0xC2, 'a') => 'á',
        (0xC2, 'c') => 'ć',
        (0xC2, 'e') => 'é',
        (0xC2, 'i') => 'í',
        (0xC2, 'l') => 'ĺ',
        (0xC2, 'n') => 'ń',
        (0xC2, 'o') => 'ó',
        (0xC2, 'r') => 'ŕ',
        (0xC2, 's') => 'ś',
        (0xC2, 'u') => 'ú',
        (0xC2, 'y') => 'ý',
        (0xC2, 'z') => 'ź',
        (0xC3, 'A') => 'Â',
        (0xC3, 'C') => 'Ĉ',
        (0xC3, 'E') => 'Ê',
        (0xC3, 'G') => 'Ĝ',
        (0xC3, 'H') => 'Ĥ',
        (0xC3, 'I') => 'Î',
        (0xC3, 'J') => 'Ĵ',
        (0xC3, 'O') => 'Ô',
        (0xC3, 'S') => 'Ŝ',
        (0xC3, 'U') => 'Û',
        (0xC3, 'W') => 'Ŵ',
        (0xC3, 'Y') => 'Ŷ',
        (0xC3, 'a') => 'â',
        (0xC3, 'c') => 'ĉ',
        (0xC3, 'e') => 'ê',
        (0xC3, 'g') => 'ĝ',
        (0xC3, 'h') => 'ĥ',
        (0xC3, 'i') => 'î',
        (0xC3, 'j') => 'ĵ',
        (0xC3, 'o') => 'ô',
        (0xC3, 's') => 'ŝ',
        (0xC3, 'u') => 'û',
        (0xC3, 'w') => 'ŵ',
        (0xC3, 'y') => 'ŷ',
        (0xC4, 'A') => 'Ã',
        (0xC4, 'I') => 'Ĩ',
        (0xC4, 'N') => 'Ñ',
        (0xC4, 'O') => 'Õ',
        (0xC4, 'U') => 'Ũ',
        (0xC4, 'a') => 'ã',
        (0xC4, 'i') => 'ĩ',
        (0xC4, 'n') => 'ñ',
        (0xC4, 'o') => 'õ',
        (0xC4, 'u') => 'ũ',
        (0xC5, 'A') => 'Ā',
        (0xC5, 'E') => 'Ē',
        (0xC5, 'I') => 'Ī',
        (0xC5, 'O') => 'Ō',
        (0xC5, 'U') => 'Ū',
        (0xC5, 'a') => 'ā',
        (0xC5, 'e') => 'ē',
        (0xC5, 'i') => 'ī',
        (0xC5, 'o') => 'ō',
        (0xC5, 'u') => 'ū',
        (0xC6, 'A') => 'Ă',
        (0xC6, 'G') => 'Ğ',
        (0xC6, 'U') => 'Ŭ',
        (0xC6, 'a') => 'ă',
        (0xC6, 'g') => 'ğ',
        (0xC6, 'u') => 'ŭ',
        (0xC7, 'C') => 'Ċ',
        (0xC7, 'E') => 'Ė',
        (0xC7, 'G') => 'Ġ',
        (0xC7, 'I') => 'İ',
        (0xC7, 'Z') => 'Ż',
        (0xC7, 'c') => 'ċ',
        (0xC7, 'e') => 'ė',
        (0xC7, 'g') => 'ġ',
        (0xC7, 'z') => 'ż',
        (0xC8, 'A') => 'Ä',
        (0xC8, 'E') => 'Ë',
        (0xC8, 'I') => 'Ï',
        (0xC8, 'O') => 'Ö',
        (0xC8, 'U') => 'Ü',
        (0xC8, 'Y') => 'Ÿ',
        (0xC8, 'a') => 'ä',
        (0xC8, 'e') => 'ë',
        (0xC8, 'i') => 'ï',
        (0xC8, 'o') => 'ö',
        (0xC8, 'u') => 'ü',
        (0xC8, 'y') => 'ÿ',
        (0xCA, 'A') => 'Å',
        (0xCA, 'U') => 'Ů',
        (0xCA, 'a') => 'å',
        (0xCA, 'u') => 'ů',
        (0xCB, 'C') => 'Ç',
        (0xCB, 'G') => 'Ģ',
        (0xCB, 'K') => 'Ķ',
        (0xCB, 'L') => 'Ļ',
        (0xCB, 'N') => 'Ņ',
        (0xCB, 'R') => 'Ŗ',
        (0xCB, 'S') => 'Ş',
        (0xCB, 'T') => 'Ţ',
        (0xCB, 'c') => 'ç',
        (0xCB, 'g') => 'ģ',
        (0xCB, 'k') => 'ķ',
        (0xCB, 'l') => 'ļ',
        (0xCB, 'n') => 'ņ',
        (0xCB, 'r') => 'ŗ',
        (0xCB, 's') => 'ş',
        (0xCB, 't') => 'ţ',
        (0xCD, 'O') => 'Ő',
        (0xCD, 'U') => 'Ű',
        (0xCD, 'o') => 'ő',
        (0xCD, 'u') => 'ű',
        (0xCE, 'A') => 'Ą',
        (0xCE, 'E') => 'Ę',
        (0xCE, 'I') => 'Į',
        (0xCE, 'U') => 'Ų',
        (0xCE, 'a') => 'ą',
        (0xCE, 'e') => 'ę',
        (0xCE, 'i') => 'į',
        (0xCE, 'u') => 'ų',
        (0xCF, 'C') => 'Č',
        (0xCF, 'D') => 'Ď',
        (0xCF, 'E') => 'Ě',
        (0xCF, 'L') => 'Ľ',
        (0xCF, 'N') => 'Ň',
        (0xCF, 'R') => 'Ř',
        (0xCF, 'S') => 'Š',
        (0xCF, 'T') => 'Ť',
        (0xCF, 'Z') => 'Ž',
        (0xCF, 'c') => 'č',
        (0xCF, 'd') => 'ď',
        (0xCF, 'e') => 'ě',
        (0xCF, 'l') => 'ľ',
        (0xCF, 'n') => 'ň',
        (0xCF, 'r') => 'ř',
        (0xCF, 's') => 'š',
        (0xCF, 't') => 'ť',
        (0xCF, 'z') => 'ž',
        _ => return None,
    };
    Some(ch)
}

/// Maps a single-byte ISO/IEC 6937 GR character (`0xA0..=0xFF`, excluding diacritics). The codes
/// are the standard's: `0xE2` is `Đ` (U+0110), and `Ð` (U+00D0) has no ISO/IEC 6937 code.
fn iso6937_single(b: u8) -> Option<char> {
    let ch = match b {
        0xA0 => '\u{00A0}',
        0xA1 => '¡',
        0xA2 => '¢',
        0xA3 => '£',
        0xA4 => '€',
        0xA5 => '¥',
        0xA7 => '§',
        0xA8 => '¤',
        0xA9 => '‘',
        0xAA => '“',
        0xAB => '«',
        0xAC => '←',
        0xAD => '↑',
        0xAE => '→',
        0xAF => '↓',
        0xB0 => '°',
        0xB1 => '±',
        0xB2 => '²',
        0xB3 => '³',
        0xB4 => '×',
        0xB5 => 'µ',
        0xB6 => '¶',
        0xB7 => '·',
        0xB8 => '÷',
        0xB9 => '’',
        0xBA => '”',
        0xBB => '»',
        0xBC => '¼',
        0xBD => '½',
        0xBE => '¾',
        0xBF => '¿',
        0xD0 => '―',
        0xD1 => '¹',
        0xD2 => '®',
        0xD3 => '©',
        0xD4 => '™',
        0xD5 => '♪',
        0xD6 => '¬',
        0xD7 => '¦',
        0xDC => '⅛',
        0xDD => '⅜',
        0xDE => '⅝',
        0xDF => '⅞',
        0xE0 => 'Ω',
        0xE1 => 'Æ',
        0xE2 => 'Đ',
        0xE3 => 'ª',
        0xE4 => 'Ħ',
        0xE6 => 'Ĳ',
        0xE7 => 'Ŀ',
        0xE8 => 'Ł',
        0xE9 => 'Ø',
        0xEA => 'Œ',
        0xEB => 'º',
        0xEC => 'Þ',
        0xED => 'Ŧ',
        0xEE => 'Ŋ',
        0xEF => 'ŉ',
        0xF0 => 'ĸ',
        0xF1 => 'æ',
        0xF2 => 'đ',
        0xF3 => 'ð',
        0xF4 => 'ħ',
        0xF5 => 'ı',
        0xF6 => 'ĳ',
        0xF7 => 'ŀ',
        0xF8 => 'ł',
        0xF9 => 'ø',
        0xFA => 'œ',
        0xFB => 'ß',
        0xFC => 'þ',
        0xFD => 'ŧ',
        0xFE => 'ŋ',
        0xFF => '\u{00AD}',
        _ => return None,
    };
    Some(ch)
}
