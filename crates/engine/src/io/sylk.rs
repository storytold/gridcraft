//! SYLK (`.slk`) import: the symbolic link text format older spreadsheets and some legacy systems
//! export. One record per line, fields separated by `;` (`;;` is a literal semicolon):
//!
//! - `ID` starts the file; `E` ends it.
//! - `C;Y<row>;X<col>;K<value>;E<formula>` is a cell. `Y` and `X` may be left out to stay on the
//!   current row or column. `K` is a number, `"text"`, `TRUE`/`FALSE` or an error; `E` is a
//!   formula in R1C1 notation; `S;R<row>;C<col>` shares the formula of another cell.
//! - `P;P<code>` adds a number format to the file's list; `F;P<n>;Y<row>;X<col>` gives a cell the
//!   n-th format (0-based), and `F;W<first> <last> <width>` sets column widths in characters.
//!
//! Other records (fonts, options, window settings) are skipped. Formulas are converted to A1 with
//! the same relative and absolute references, as Excel does when it opens the file.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, Value, col_to_letters};
use gridcraft_model::{Cell, Formula, NumFmt, Sheet, Style, Workbook};

use crate::{EngineError, Result};

/// The largest SYLK file read, as for other text imports: past it the file is refused.
const MAX_BYTES: usize = 512 << 20;
/// Column-width records kept from one file; each can cover every column.
const MAX_WIDTH_RECORDS: usize = 1024;

/// Reads a SYLK file into a one-sheet workbook. Returns the workbook and its warnings.
pub fn read(name: &str, bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
    if bytes.len() > MAX_BYTES {
        return Err(EngineError::Other(format!("We can't open '{name}': the SYLK file is larger than {} MB.", MAX_BYTES >> 20)));
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        // Older exports are in a single-byte code page: keep each byte as its Latin-1 character.
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    if !text.starts_with("ID") {
        return Err(EngineError::Other(format!("We can't open '{name}': it isn't a SYLK file (it doesn't start with an ID record).")));
    }
    let mut sheet = Sheet::new("Sheet1");
    let mut formats: Vec<String> = Vec::new();
    // Cells' number formats by index into `formats`, applied once every format is known.
    let mut cell_formats: Vec<(CellRef, usize)> = Vec::new();
    let mut formulas: HashMap<CellRef, String> = HashMap::new();
    let mut widths: Vec<(u32, u32, f64)> = Vec::new();
    let (mut row, mut col) = (0u32, 0u32);
    let mut skipped = 0usize;
    for line in text.lines() {
        let fields = split_fields(line);
        let Some((kind, rest)) = fields.split_first() else { continue };
        match kind.as_str() {
            "C" => {
                let (mut value, mut formula, mut shared) = (None, None, None::<(Option<u32>, Option<u32>)>);
                let mut bad = false;
                for f in rest {
                    let (tag, arg) = f.split_at(f.chars().next().map_or(0, char::len_utf8));
                    match tag {
                        "Y" => match position(arg, MAX_ROWS) {
                            Some(r) => row = r,
                            None => bad = true,
                        },
                        "X" => match position(arg, MAX_COLS) {
                            Some(c) => col = c,
                            None => bad = true,
                        },
                        "K" => value = Some(parse_value(arg)),
                        "E" => formula = Some(arg.to_string()),
                        "S" => shared = Some((None, None)),
                        // In a shared-formula cell, R and C name the cell whose formula it shares.
                        "R" if shared.is_some() => shared = shared.map(|(_, c)| (position(arg, MAX_ROWS), c)),
                        "C" if shared.is_some() => shared = shared.map(|(r, _)| (r, position(arg, MAX_COLS))),
                        _ => {}
                    }
                }
                if bad {
                    skipped += 1;
                    continue;
                }
                let at = CellRef::new(row, col);
                if let Some((Some(r), Some(c))) = shared {
                    formula = formulas.get(&CellRef::new(r, c)).cloned();
                }
                let mut cell = sheet.cell(at).cloned().unwrap_or_else(|| Cell::value(Value::Empty));
                if let Some(v) = value {
                    cell.value = v;
                }
                if let Some(r1c1) = formula {
                    let a1 = r1c1_formula_to_a1(&r1c1, at);
                    let f = Formula::new(&a1);
                    if f.expr.is_some() {
                        cell.formula = Some(Arc::new(f));
                        formulas.insert(at, r1c1);
                    } else {
                        skipped += 1;
                    }
                }
                sheet.set_cell(at, cell);
            }
            "F" => {
                let mut format = None;
                for f in rest {
                    let (tag, arg) = f.split_at(f.chars().next().map_or(0, char::len_utf8));
                    match tag {
                        "Y" => row = position(arg, MAX_ROWS).unwrap_or(row),
                        "X" => col = position(arg, MAX_COLS).unwrap_or(col),
                        "P" => format = arg.parse::<usize>().ok(),
                        "W" => {
                            let mut parts = arg.split_whitespace();
                            if let (Some(first), Some(last), Some(width)) = (parts.next(), parts.next(), parts.next())
                                && let (Some(first), Some(last), Ok(width)) =
                                    (position(first, MAX_COLS), position(last, MAX_COLS), width.parse::<f64>())
                                && first <= last
                                && (0.0..=255.0).contains(&width)
                                // A file of width records can't make the import loop over every column
                                // again and again.
                                && widths.len() < MAX_WIDTH_RECORDS
                            {
                                widths.push((first, last, width));
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(format) = format {
                    cell_formats.push((CellRef::new(row, col), format));
                }
            }
            // Number formats only: `P;F…` and `P;E…` are fonts.
            "P" => {
                if let Some(code) = rest.first().and_then(|f| f.strip_prefix('P')) {
                    formats.push(code.to_string());
                }
            }
            "E" => break,
            _ => {}
        }
    }
    for (first, last, width) in widths {
        let size = crate::cmd::format::chars_to_points(width) as f32;
        for c in first..=last {
            sheet.cols.entry(c).or_default().size = Some(size);
        }
    }
    let mut wb = Workbook::new();
    let mut styles = HashMap::new();
    for (at, index) in cell_formats {
        let Some(code) = formats.get(index).filter(|code| !code.is_empty() && *code != "General") else { continue };
        let style = *styles.entry(index).or_insert_with(|| wb.styles.intern(Style { num_fmt: NumFmt::custom(code), ..Style::default() }));
        let mut cell = sheet.cell(at).cloned().unwrap_or_else(|| Cell::value(Value::Empty));
        cell.style = style;
        sheet.set_cell(at, cell);
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

/// A record's fields: `;` separates them and `;;` is a literal semicolon.
fn split_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.trim_end_matches('\r').chars().peekable();
    while let Some(c) = chars.next() {
        if c == ';' {
            if chars.peek() == Some(&';') {
                chars.next();
                if let Some(f) = fields.last_mut() {
                    f.push(';');
                }
            } else {
                fields.push(String::new());
            }
        } else if let Some(f) = fields.last_mut() {
            f.push(c);
        }
    }
    fields
}

/// A 1-based row or column number → 0-based, within `max`.
fn position(arg: &str, max: u32) -> Option<u32> {
    arg.trim().parse::<u32>().ok().filter(|n| (1..=max).contains(n)).map(|n| n - 1)
}

/// A `K` value: `"text"` (everything between the first and the last quote, as written), a
/// boolean, an error or a number.
fn parse_value(arg: &str) -> Value {
    if let Some(text) = arg.strip_prefix('"') {
        let text = text.strip_suffix('"').unwrap_or(text);
        return Value::Text(Arc::from(text));
    }
    if arg.eq_ignore_ascii_case("TRUE") || arg.eq_ignore_ascii_case("FALSE") {
        return Value::Bool(arg.eq_ignore_ascii_case("TRUE"));
    }
    if let Some(e) = CellError::parse(arg) {
        return Value::Error(e);
    }
    match arg.trim().parse::<f64>() {
        Ok(n) if n.is_finite() => Value::Number(n),
        _ => Value::Text(Arc::from(arg)),
    }
}

/// One part of an R1C1 reference: `R`/`C` followed by an absolute number, a bracketed offset or
/// nothing (the cell's own row or column).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    Absolute(u32),
    Relative(i64),
}

impl Part {
    /// The 0-based index and whether it is absolute, or `None` when it falls off the sheet.
    fn resolve(self, base: u32, max: u32) -> Option<(u32, bool)> {
        match self {
            Part::Absolute(n) => (1..=max).contains(&n).then(|| (n - 1, true)),
            Part::Relative(k) => i64::from(base).checked_add(k).and_then(|v| u32::try_from(v).ok()).filter(|v| *v < max).map(|v| (v, false)),
        }
    }
}

/// Parses the number after `R` or `C` at `i`: digits, `[offset]` or nothing. Returns the part and
/// the index after it, or `None` when the text isn't a reference part.
fn parse_part(chars: &[char], i: usize) -> Option<(Part, usize)> {
    match chars.get(i) {
        Some('[') => {
            let close = chars[i..].iter().position(|&c| c == ']')? + i;
            let inner: String = chars[i + 1..close].iter().collect();
            Some((Part::Relative(inner.parse().ok()?), close + 1))
        }
        Some(c) if c.is_ascii_digit() => {
            let end = chars[i..].iter().position(|c| !c.is_ascii_digit()).map_or(chars.len(), |p| p + i);
            let n: String = chars[i..end].iter().collect();
            Some((Part::Absolute(n.parse().unwrap_or(u32::MAX)), end))
        }
        _ => Some((Part::Relative(0), i)),
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '\\' | '?' | '$')
}

/// Rewrites the R1C1 references in a SYLK formula as A1 references for the cell at `at`:
/// `RC[-1]` → `B2`, `R1C1` → `$A$1`, `R[-2]C:R[-1]C` → `D2:D3`, a whole column `C4` → `$D:$D` and
/// a whole row `R3` → `$3:$3`. Text in quotes and quoted sheet names are left alone; a reference
/// off the sheet becomes `#REF!`.
pub(crate) fn r1c1_formula_to_a1(formula: &str, at: CellRef) -> String {
    let chars: Vec<char> = formula.chars().collect();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            // Copy the quoted run as it is; a doubled quote stays inside it.
            let mut j = i + 1;
            while j < chars.len() && !(chars[j] == c && chars.get(j + 1) != Some(&c)) {
                j += if chars[j] == c { 2 } else { 1 };
            }
            let stop = (j + 1).min(chars.len());
            out.extend(&chars[i..stop]);
            i = stop;
            continue;
        }
        let starts_word = i == 0 || !is_word_char(chars[i - 1]);
        if starts_word
            && matches!(c, 'R' | 'r' | 'C' | 'c')
            && let Some((text, next)) = reference_at(&chars, i, at)
        {
            out.push_str(&text);
            i = next;
            continue;
        }
        // Copy the rest of a word as it is, so `ROUND` or `Sheet2` isn't read as a reference.
        if is_word_char(c) {
            let end = (i..chars.len()).find(|&j| !is_word_char(chars[j])).unwrap_or(chars.len());
            out.extend(&chars[i..end]);
            i = end;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// The A1 text of the R1C1 reference starting at `i`, and the index after it, or `None` when the
/// word there isn't a reference (a function or a name).
fn reference_at(chars: &[char], i: usize, at: CellRef) -> Option<(String, usize)> {
    let mut j = i;
    let mut row = None;
    let mut col = None;
    if matches!(chars.get(j), Some('R' | 'r')) {
        let (part, next) = parse_part(chars, j + 1)?;
        row = Some(part);
        j = next;
    }
    if matches!(chars.get(j), Some('C' | 'c')) {
        let (part, next) = parse_part(chars, j + 1)?;
        col = Some(part);
        j = next;
    }
    if row.is_none() && col.is_none() {
        return None;
    }
    // A reference ends the word: `ROUND(`, `RATE` or `R2D2` are names.
    if chars.get(j).is_some_and(|&c| is_word_char(c) || c == '(' || c == '[' || c == '!') {
        return None;
    }
    let row_text = |p: Part| p.resolve(at.row, MAX_ROWS).map(|(r, abs)| format!("{}{}", if abs { "$" } else { "" }, r + 1));
    let col_text = |p: Part| p.resolve(at.col, MAX_COLS).map(|(c, abs)| format!("{}{}", if abs { "$" } else { "" }, col_to_letters(c)));
    // One side of a range (`R2:R4`, `C1:C3`) is written alone; a lone row or column is the whole of it.
    let prev = chars[..i].iter().rev().find(|c| !c.is_whitespace());
    let in_range = prev == Some(&':') || chars[j..].iter().find(|c| !c.is_whitespace()) == Some(&':');
    let text = match (row, col) {
        (Some(r), Some(c)) => row_text(r).zip(col_text(c)).map(|(r, c)| format!("{c}{r}")),
        (Some(r), None) => row_text(r).map(|r| if in_range { r } else { format!("{r}:{r}") }),
        (None, Some(c)) => col_text(c).map(|c| if in_range { c } else { format!("{c}:{c}") }),
        (None, None) => None,
    };
    Some((text.unwrap_or_else(|| "#REF!".into()), j))
}
