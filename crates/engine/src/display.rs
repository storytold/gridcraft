//! How cells look as text: number formats applied, for the grid, clipboard, CSV and agents.

use std::collections::HashMap;
use std::sync::Mutex;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Sheet, Workbook};
use gridcraft_numfmt::{Formatted, NumberFormat, format_value};

/// Parsed number formats are cached by code.
fn parsed(code: &str) -> NumberFormat {
    static CACHE: std::sync::LazyLock<Mutex<HashMap<String, NumberFormat>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    let lock = || CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(f) = lock().get(code) {
        return f.clone();
    }
    // Parse outside the lock, so a slow code never blocks the other threads' lookups.
    let f = NumberFormat::parse(code);
    let mut m = lock();
    if m.len() > 4096 {
        m.clear();
    }
    m.insert(code.to_string(), f.clone());
    f
}

pub fn number_format(code: &str) -> NumberFormat {
    parsed(code)
}

/// Formats a value with a format code.
pub fn format(v: &Value, code: &str, wb: &Workbook) -> Formatted {
    format_value(v, &parsed(code), wb.date_system)
}

/// Whether typing `t` back into a General cell would not give the text `t` again (it would parse
/// as a number, boolean, error, date or formula, or lose a leading apostrophe). Such text is shown
/// for editing with a leading `'`, as Excel does for text carrying a quote prefix, so that
/// committing the editor unchanged keeps the cell as text.
fn text_needs_quote_prefix(t: &str, sys: gridcraft_core::date::DateSystem) -> bool {
    if t.is_empty() {
        return false;
    }
    let mut chars = t.chars();
    let (first, second) = (chars.next(), chars.next());
    if matches!(first, Some('=' | '\'')) {
        return true;
    }
    if matches!(first, Some('+' | '-')) && second.is_some_and(|c| c.is_ascii_alphabetic() || c == '(') {
        return true;
    }
    !matches!(gridcraft_core::parse::parse_input(t, sys).value, Value::Text(ref x) if x.as_ref() == t)
}

/// What the cell editor and the formula bar start from. Same as `Cell::input_text`, except that a
/// constant number in a date or time format is shown as a date (as the grid shows it), not as its
/// serial. When the formatted text would not read back as the same date (for example
/// `dddd, mmmm d`), an ISO `yyyy-mm-dd` (with `hh:mm:ss` for a time of day; just `hh:mm:ss`
/// for a time alone) is used instead, and if even that would not read back, the serial.
pub fn edit_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    let raw = sheet.input_text(c);
    let Some(cell) = sheet.cell(c) else { return raw };
    if let Value::Text(t) = &cell.value
        && cell.formula.is_none()
        && wb.styles.get(cell.style).num_fmt.as_str() != "@"
        && text_needs_quote_prefix(t, wb.date_system)
    {
        return format!("'{raw}");
    }
    let Value::Number(n) = &cell.value else { return raw };
    let code = wb.styles.get(cell.style).num_fmt.as_str();
    if cell.formula.is_some() || !parsed(code).is_date() {
        return raw;
    }
    let reads_back = |s: &str| matches!(gridcraft_core::parse::parse_input(s, wb.date_system).value, Value::Number(m) if (m - n).abs() < 1e-9);
    let shown = format(&cell.value, code, wb).text;
    if reads_back(&shown) {
        return shown;
    }
    let Some(dt) = gridcraft_core::date::datetime_from_serial(wb.date_system, *n) else { return raw };
    let time = format!("{:02}:{:02}:{:02}", dt.hour, dt.minute, dt.second);
    let date = format!("{:04}-{:02}-{:02}", dt.year, dt.month, dt.day);
    let iso = if *n < 1.0 {
        time
    } else if n.fract() == 0.0 {
        date
    } else {
        format!("{date} {time}")
    };
    if reads_back(&iso) { iso } else { raw }
}

/// Whether `c` has a formula that must not be shown: its cell is formatted Hidden on a
/// protected sheet. A spilled cell follows its anchor.
pub fn formula_hidden(wb: &Workbook, sheet: &Sheet, c: CellRef) -> bool {
    if !sheet.is_protected() {
        return false;
    }
    let at = match sheet.cell(c) {
        Some(x) if x.formula.is_some() => c,
        _ => match sheet.spill_ranges.iter().find(|(_, r)| r.contains(c)) {
            Some((anchor, _)) => *anchor,
            None => return false,
        },
    };
    wb.styles.get(sheet.style_id(at)).protection.hidden
}

/// The text a cell shows (full precision General; the grid narrows General to fit the column).
pub fn cell_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    if sheet.show_formulas
        && let Some(f) = sheet.cell(c).and_then(|x| x.formula.as_ref())
        && !formula_hidden(wb, sheet, c)
    {
        return format!("={}", f.text);
    }
    value_text(wb, sheet, c)
}

/// A cell's value with its number format applied, independent of column width: no `#####`
/// and no General narrowing (Save As CSV/Text writes this). Formulas give their result.
pub fn value_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    let style = wb.styles.get(sheet.style_id(c));
    value_text_as(wb, &sheet.value(c), style.num_fmt.as_str())
}

/// [`value_text`] for a value and a format code given apart from a cell (print takes the code
/// from the cell's effective look, which a conditional format can override).
pub fn value_text_as(wb: &Workbook, v: &Value, code: &str) -> String {
    let f = format(v, code, wb);
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

/// The workbook's phonetic readings by the text they belong to, built once for a painting or
/// export pass so that looking up any number of cells does not scan `wb.phonetics` per cell
/// (View › Show › Furigana). The first entry for a text wins, as the single-cell scan does.
#[derive(Default)]
pub struct PhoneticIndex<'a> {
    readings: HashMap<&'a str, String>,
}

impl<'a> PhoneticIndex<'a> {
    pub fn new(wb: &'a Workbook) -> Self {
        let mut readings: HashMap<&'a str, String> = HashMap::with_capacity(wb.phonetics.len());
        for p in &wb.phonetics {
            // A text may have several entries; the first with runs is the one a per-cell scan finds.
            if p.runs.is_empty() {
                continue;
            }
            let reading = || p.runs.iter().map(|run| run.text.as_str()).collect();
            readings.entry(p.text.as_str()).or_insert_with(reading);
        }
        Self { readings }
    }

    /// [`phonetic_text`] for one cell from the index: the reading of the cell's string, if the
    /// workbook carries one. `None` for a cell without a string, a formula, or one whose string
    /// has no readings.
    pub fn reading(&self, sheet: &Sheet, c: CellRef) -> Option<&str> {
        if sheet.cell(c).is_some_and(|cell| cell.formula.is_some()) {
            return None;
        }
        let text = sheet.input_text(c);
        if text.is_empty() {
            return None;
        }
        self.readings.get(text.as_str()).filter(|r| !r.trim().is_empty()).map(String::as_str)
    }
}

/// The phonetic (furigana) reading of a cell's string, when the workbook carries one for its text:
/// the `<rPh>` runs joined in document order (XLSX keeps them per shared string, so the reading is
/// found by the cell's text). `None` for a cell without a string, a formula, or one whose string
/// has no readings.
///
/// This is what View › Show › Furigana draws above the cell text on screen and in PDF export; the
/// readings are never used to change the cell's value, formula or layout. A pass over many cells
/// builds [`PhoneticIndex`] once instead of calling this for each cell.
pub fn phonetic_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> Option<String> {
    PhoneticIndex::new(wb).reading(sheet, c).map(str::to_owned)
}

/// Tab-separated text of a range, as the system clipboard gets it.
pub fn range_text(wb: &Workbook, sheet: usize, r: RangeRef) -> String {
    let Some(sh) = wb.sheet(sheet) else { return String::new() };
    let mut out = String::new();
    let r = clipboard_range(r);
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

/// The existing plain-text clipboard limit, shared with its HTML representation.
pub(crate) fn clipboard_range(r: RangeRef) -> RangeRef {
    if r.count() > 2_000_000 { RangeRef::new(r.start, r.start.offset_clamped(1999, 999)) } else { r }
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

#[cfg(test)]
mod tests {
    use gridcraft_model::{PhoneticRun, PhoneticString};

    use super::*;

    fn string_book(text: &str, reading: Option<&[(&str, &str)]>) -> Workbook {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).expect("one sheet").set_value(CellRef::new(0, 0), Value::text(text));
        if let Some(runs) = reading {
            let mut start = 0u32;
            let runs = runs
                .iter()
                .map(|(base, reading)| {
                    let end = start + base.encode_utf16().count() as u32;
                    let run = PhoneticRun { start, end, text: (*reading).into() };
                    start = end;
                    run
                })
                .collect();
            wb.phonetics.push(PhoneticString { text: text.into(), runs, props: None });
        }
        wb
    }

    #[test]
    fn phonetic_text_joins_the_runs_of_the_cells_string() {
        let wb = string_book("東京都", Some(&[("東京", "トウキョウ"), ("都", "ト")]));
        let sh = wb.sheet(0).expect("one sheet");
        assert_eq!(phonetic_text(&wb, sh, CellRef::new(0, 0)).as_deref(), Some("トウキョウト"));
        // A cell without readings, a string without phonetic data and an empty cell have none.
        let plain = string_book("abc", None);
        assert_eq!(phonetic_text(&plain, plain.sheet(0).expect("one sheet"), CellRef::new(0, 0)), None);
        assert_eq!(phonetic_text(&plain, plain.sheet(0).expect("one sheet"), CellRef::new(1, 1)), None);
        // A formula cell's value is not a shared string, so it has no readings even when its text matches.
        let mut formula = string_book("東京都", Some(&[("東京", "トウキョウ")]));
        let f = gridcraft_model::Cell::formula(gridcraft_model::Formula::new("=A1"));
        formula.sheet_mut(0).expect("one sheet").set_cell(CellRef::new(0, 1), f);
        assert_eq!(phonetic_text(&formula, formula.sheet(0).expect("one sheet"), CellRef::new(0, 1)), None);
    }

    #[test]
    fn phonetic_index_keeps_the_first_entry_and_agrees_with_the_single_lookup() {
        let mut wb = string_book("東京都", Some(&[("東京", "トウキョウ")]));
        // A later entry for the same text must not replace the first, as the single-cell scan keeps it.
        wb.phonetics
            .push(PhoneticString { text: "東京都".into(), runs: vec![PhoneticRun { start: 0, end: 3, text: "ベツ".into() }], props: None });
        let index = PhoneticIndex::new(&wb);
        let sh = wb.sheet(0).expect("one sheet");
        assert_eq!(index.reading(sh, CellRef::new(0, 0)), Some("トウキョウ"));
        assert_eq!(phonetic_text(&wb, sh, CellRef::new(0, 0)).as_deref(), Some("トウキョウ"));
        // A first entry whose runs are blank still wins over a later readable one: both lookups
        // then report no reading, as the scan rejects the joined text.
        let mut blank = string_book("x", None);
        blank.phonetics.push(PhoneticString { text: "x".into(), runs: vec![PhoneticRun { start: 0, end: 1, text: " ".into() }], props: None });
        blank
            .phonetics
            .push(PhoneticString { text: "x".into(), runs: vec![PhoneticRun { start: 0, end: 1, text: "エックス".into() }], props: None });
        let index = PhoneticIndex::new(&blank);
        let sh = blank.sheet(0).expect("one sheet");
        assert_eq!(index.reading(sh, CellRef::new(0, 0)), None);
        assert_eq!(phonetic_text(&blank, sh, CellRef::new(0, 0)), None);
    }
}
