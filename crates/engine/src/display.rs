//! How cells look as text: number formats applied, for the grid, clipboard, CSV and agents.

use std::collections::HashMap;
use std::sync::Mutex;

use gridcraft_core::{CellRef, Locale, RangeRef, Value};
use gridcraft_model::{Cell, Sheet, Workbook};
use gridcraft_numfmt::{Formatted, NumberFormat, format_value_in};

/// Parsed number formats are cached by code.
fn parsed(code: &str) -> NumberFormat {
    static CACHE: std::sync::LazyLock<Mutex<HashMap<String, NumberFormat>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    let mut m = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(f) = m.get(code) {
        return f.clone();
    }
    let f = NumberFormat::parse(code);
    if m.len() > 4096 {
        m.clear();
    }
    m.insert(code.to_string(), f.clone());
    f
}

pub fn number_format(code: &str) -> NumberFormat {
    parsed(code)
}

/// Formats a value with a format code (en-US).
pub fn format(v: &Value, code: &str, wb: &Workbook) -> Formatted {
    format_in(v, code, wb, Locale::EnUs)
}

/// Formats a value with a format code, as shown in `loc` (`1.234,56` in German).
pub fn format_in(v: &Value, code: &str, wb: &Workbook, loc: Locale) -> Formatted {
    format_value_in(v, &parsed(code), wb.date_system, loc)
}

/// The text a cell shows (full precision General; the grid narrows General to fit the column).
pub fn cell_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    cell_text_in(wb, sheet, c, Locale::EnUs)
}

/// The text a cell shows in `loc`.
pub fn cell_text_in(wb: &Workbook, sheet: &Sheet, c: CellRef, loc: Locale) -> String {
    let v = sheet.value(c);
    if sheet.show_formulas
        && let Some(f) = sheet.cell(c).and_then(|x| x.formula.as_ref())
    {
        return gridcraft_formula::locale::to_local(&format!("={}", f.text), loc);
    }
    let style = wb.styles.get(sheet.style_id(c));
    let f = format_in(&v, style.num_fmt.as_str(), wb, loc);
    match f.fill {
        Some((_, pos)) => {
            let mut t = f.text;
            if pos <= t.len() && t.is_char_boundary(pos) {
                t.insert(pos, ' ');
            }
            t
        }
        None => f.text,
    }
}

/// What the formula bar shows for a cell in `loc`, ready to be typed back: the formula as
/// written there, or the value (`1234,5`, `10.10.2026`, `12,5%`, `WAHR` in German). en-US shows
/// [`Cell::input_text`].
pub fn input_text_in(wb: &Workbook, cell: &Cell, loc: Locale) -> String {
    if loc.is_en() {
        return cell.input_text();
    }
    if let Some(f) = &cell.formula {
        return gridcraft_formula::locale::to_local(&format!("={}", f.text), loc);
    }
    match &cell.value {
        Value::Number(n) if n.is_finite() => {
            let nf = parsed(wb.styles.get(cell.style).num_fmt.as_str());
            if nf.is_date() && *n >= 0.0 {
                let code = if n.fract() == 0.0 {
                    "dd.mm.yyyy"
                } else if *n < 1.0 {
                    "hh:mm:ss"
                } else {
                    "dd.mm.yyyy hh:mm:ss"
                };
                return format_in(&cell.value, code, wb, loc).text;
            }
            if nf.is_percent() {
                return format!("{}%", loc.number_literal(&gridcraft_core::number_to_text(n * 100.0)));
            }
            loc.number_literal(&gridcraft_core::number_to_text(*n))
        }
        Value::Bool(b) => loc.bool_name(*b).to_string(),
        Value::Error(e) => loc.error_name(*e).to_string(),
        _ => cell.input_text(),
    }
}

/// Tab-separated text of a range, as the system clipboard gets it.
pub fn range_text(wb: &Workbook, sheet: usize, r: RangeRef) -> String {
    let Some(sh) = wb.sheet(sheet) else { return String::new() };
    let mut out = String::new();
    let r = if r.count() > 2_000_000 { RangeRef::new(r.start, r.start.offset_clamped(1999, 999)) } else { r };
    for row in r.start.row..=r.end.row {
        if sh.is_row_hidden(row) {
            continue;
        }
        let mut first = true;
        for col in r.start.col..=r.end.col {
            if !first {
                out.push('\t');
            }
            first = false;
            let t = cell_text(wb, sh, CellRef::new(row, col));
            if t.contains(['\t', '\n', '"']) {
                out.push('"');
                out.push_str(&t.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push('\n');
    }
    out
}

/// Status-bar statistics for a selection: average, count (non-empty), numerical count, min,
/// max, sum.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Stats {
    pub count: u64,
    pub numeric: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub average: Option<f64>,
}

pub fn stats(sheet: &Sheet, ranges: &[RangeRef]) -> Stats {
    let mut s = Stats { min: f64::INFINITY, max: f64::NEG_INFINITY, ..Default::default() };
    let mut seen = std::collections::HashSet::new();
    for r in ranges {
        let r = match sheet.used_range().and_then(|u| r.intersection(&u)) {
            Some(x) => x,
            None => continue,
        };
        let mut visit = |c: CellRef, v: &Value| {
            if !seen.insert(c) || sheet.is_row_hidden(c.row) {
                return;
            }
            if v.is_empty() {
                return;
            }
            s.count += 1;
            if let Value::Number(n) = v {
                s.numeric += 1;
                s.sum += n;
                s.min = s.min.min(*n);
                s.max = s.max.max(*n);
            }
        };
        for (c, cell) in sheet.cells.iter_range(r) {
            visit(c, &cell.value);
        }
        for (c, v) in sheet.spill.range(r.start..=r.end) {
            if r.contains(*c) {
                visit(*c, v);
            }
        }
    }
    if s.numeric > 0 {
        s.average = Some(s.sum / s.numeric as f64);
    } else {
        s.min = 0.0;
        s.max = 0.0;
    }
    s
}
