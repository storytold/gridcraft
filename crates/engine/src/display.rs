//! How cells look as text: number formats applied, for the grid, clipboard, CSV and agents.

use std::collections::HashMap;
use std::sync::Mutex;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::{Sheet, Workbook};
use gridcraft_numfmt::{Formatted, NumberFormat, format_value, format_value_full};

/// Parsed number formats are cached by code.
pub(crate) fn parsed(code: &str) -> NumberFormat {
    static CACHE: std::sync::LazyLock<Mutex<HashMap<String, NumberFormat>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    // A code of more bytes than 4 × the character limit can't be within it: it shows as General
    // without being hashed or scanned again for every cell that shares it.
    if code.len() > 4 * gridcraft_numfmt::MAX_FORMAT_CODE_CHARS {
        return NumberFormat::parse("General");
    }
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

/// The parsed format of `code`; one longer than the character limit is a General format that keeps
/// the code it was made from. The engine itself formats through `parsed`, which doesn't copy it.
pub fn number_format(code: &str) -> NumberFormat {
    if code.len() > 4 * gridcraft_numfmt::MAX_FORMAT_CODE_CHARS {
        return NumberFormat::parse(code);
    }
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
    let first = t.chars().next();
    if matches!(first, Some('=' | '\'')) {
        return true;
    }
    // A `+` or `-` entry that reads as a formula becomes one (`+5+4`, `-abc`); one that doesn't
    // stays text (`--`, `+49 170 1234567`), as `input_to_cell` decides it.
    if matches!(first, Some('+' | '-'))
        && gridcraft_core::parse::starts_formula(t)
        && crate::cmd::edit::parse_typed_formula(t).is_ok_and(|e| crate::cmd::edit::check_arg_counts(&e).is_ok())
    {
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

/// Whether `c` has a formula or Python code that must not be shown: its cell is formatted
/// Hidden on a protected sheet. A spilled cell follows its anchor.
pub fn formula_hidden(wb: &Workbook, sheet: &Sheet, c: CellRef) -> bool {
    if !sheet.is_protected() {
        return false;
    }
    let at = match sheet.cell(c) {
        Some(x) if x.formula.is_some() || x.py.is_some() => c,
        _ => match sheet.spill_ranges.iter().find(|(_, r)| r.contains(c)) {
            Some((anchor, _)) => *anchor,
            None => return false,
        },
    };
    wb.styles.get(sheet.style_id(at)).protection.hidden
}

/// The text a cell shows (General up to the 11-character cell width; the grid narrows General
/// further to fit the column).
pub fn cell_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    if sheet.show_formulas
        && let Some(f) = sheet.cell(c).and_then(|x| x.formula.as_ref())
        && !formula_hidden(wb, sheet, c)
    {
        return format!("={}", f.text);
    }
    shown_text(wb, sheet, c)
}

/// Like [`cell_text`], reusing a caller-owned [`crate::cf::CfCache`] across a range so conditional
/// formatting `<dxf><numFmt>` overrides are evaluated efficiently.
pub fn cell_text_with_cf(wb: &Workbook, sheet: usize, sh: &Sheet, c: CellRef, cf: &mut crate::cf::CfCache) -> String {
    if sh.show_formulas
        && let Some(f) = sh.cell(c).and_then(|x| x.formula.as_ref())
        && !formula_hidden(wb, sh, c)
    {
        return format!("={}", f.text);
    }
    let code = cf.effective_num_fmt(wb, sheet, sh, c);
    value_text_as(wb, &sh.value(c), &code)
}

/// Effective number format code of `c` on `sheet`, resolving any matching conditional formatting
/// `<dxf><numFmt>` override when `sheet` belongs to `wb`.
fn effective_code<'a>(wb: &'a Workbook, sheet: &'a Sheet, c: CellRef) -> std::borrow::Cow<'a, str> {
    let base = wb.styles.get(sheet.style_id(c)).num_fmt.as_str();
    if crate::cf::has_num_fmt_rule(sheet, c)
        && let Some(si) = wb.sheets.iter().position(|s| std::ptr::eq(s.as_ref(), sheet))
    {
        let mut cf = crate::cf::CfCache::new();
        return cf.effective_num_fmt(wb, si, sheet, c);
    }
    std::borrow::Cow::Borrowed(base)
}

/// A cell's value with its number format applied, independent of column width: no `#####`
/// and no General narrowing (Save As CSV/Text writes this). Formulas give their result. A number
/// shown as General (also through a coloured or sectioned General, or a Text format) is written
/// as [`format_value_full`] writes it, never with an exponent: `&`'s 15 significant digits below
/// 1E+15 (`124412140710`, not `1.24412E+11`), the shortest round-trip digits from 1E+15 (every
/// integer below 1e16 exactly). Excel writes the cell display.
pub fn value_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    let code = effective_code(wb, sheet, c);
    with_fill_space(format_value_full(&sheet.value(c), &parsed(&code), wb.date_system))
}

/// Like [`value_text`], reusing a caller-owned [`crate::cf::CfCache`] across a range so conditional
/// formatting `<dxf><numFmt>` overrides are evaluated efficiently.
pub fn value_text_with_cf(wb: &Workbook, sheet: usize, sh: &Sheet, c: CellRef, cf: &mut crate::cf::CfCache) -> String {
    let code = cf.effective_num_fmt(wb, sheet, sh, c);
    with_fill_space(format_value_full(&sh.value(c), &parsed(&code), wb.date_system))
}

/// A cell's value through its number format, General at the 11-character cell width.
fn shown_text(wb: &Workbook, sheet: &Sheet, c: CellRef) -> String {
    let code = effective_code(wb, sheet, c);
    value_text_as(wb, &sheet.value(c), &code)
}

/// A value through a format code given apart from a cell, as a cell shows it (General at the
/// 11-character cell width). Print takes the code from the cell's effective look, which a
/// conditional format can override.
pub fn value_text_as(wb: &Workbook, v: &Value, code: &str) -> String {
    with_fill_space(format(v, code, wb))
}

/// The formatted text with a single space where a `*x` repeat fill would expand.
fn with_fill_space(f: Formatted) -> String {
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

/// Tab-separated text of a range, as the system clipboard gets it.
pub fn range_text(wb: &Workbook, sheet: usize, r: RangeRef) -> String {
    let Some(sh) = wb.sheet(sheet) else { return String::new() };
    let mut out = String::new();
    let mut cf = crate::cf::CfCache::new();
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
            let t = cell_text_with_cf(wb, sheet, sh, CellRef::new(row, col), &mut cf);
            // Like Excel, only text with a tab or line break is quoted (its quotes doubled);
            // `say "hi"` is copied as it is (#2225).
            if t.contains(['\t', '\n', '\r']) {
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
    use super::*;

    /// A format code far past Excel's 255 characters shows its values as General, for every cell
    /// that shares it, without being hashed into the cache each time.
    #[test]
    fn an_overlong_code_formats_as_general() {
        let wb = Workbook::new();
        let code = format!("0.00{}", "\"x\"".repeat(400_000));
        for _ in 0..1_000 {
            assert_eq!(format(&Value::Number(1.5), &code, &wb).text, "1.5");
        }
        assert_eq!(format(&Value::Number(1.5), "0.00", &wb).text, "1.50");
    }

    /// The public `number_format` keeps the code of an overlong format; it is General, but not the
    /// same format as a real `General`.
    #[test]
    fn number_format_keeps_an_overlong_code() {
        let code = format!("0.00{}", "\"x\"".repeat(2_000));
        let f = number_format(&code);
        assert!(f.is_general());
        assert_eq!(f.code(), code);
        assert_ne!(f, number_format("General"));
    }

    #[test]
    fn conditional_format_dxf_num_fmt_overrides_display_and_range_text() {
        use gridcraft_model::{CfOperator, CfRule, CondFormat, NumFmt, Style};

        let mut wb = Workbook::new();
        let base_style = wb.styles.intern(Style { num_fmt: NumFmt::new("0.0000"), ..Style::default() });
        let sh = wb.sheet_mut(0).unwrap();
        // Duration serials: 30s, 15m, 2h 10m, and a value overriding back to General.
        for (row, val) in [(0, 30.0 / 86_400.0), (1, 15.0 / 1440.0), (2, (2.0 * 60.0 + 10.0) / 1440.0), (3, 5.0)] {
            let c = CellRef::new(row, 0);
            sh.set_value(c, Value::Number(val));
            sh.set_style(c, base_style);
        }
        let r_dur = vec![RangeRef::new(CellRef::new(0, 0), CellRef::new(2, 0))];
        sh.cond_formats.push(CondFormat {
            ranges: r_dur.clone(),
            priority: 1,
            stop_if_true: true,
            rule: CfRule::CellIs {
                op: CfOperator::Less,
                a: "0.0006944".into(),
                b: None,
                style: Box::new(Style { num_fmt: NumFmt::new("s\"s\""), ..Style::default() }),
            },
        });
        sh.cond_formats.push(CondFormat {
            ranges: r_dur.clone(),
            priority: 2,
            stop_if_true: true,
            rule: CfRule::CellIs {
                op: CfOperator::Less,
                a: "0.04166".into(),
                b: None,
                style: Box::new(Style { num_fmt: NumFmt::new("[m]\"m\""), ..Style::default() }),
            },
        });
        sh.cond_formats.push(CondFormat {
            ranges: r_dur,
            priority: 3,
            stop_if_true: true,
            rule: CfRule::CellIs {
                op: CfOperator::GreaterOrEqual,
                a: "0.04166".into(),
                b: None,
                style: Box::new(Style { num_fmt: NumFmt::new("[h]\"h, \"m\"m\""), ..Style::default() }),
            },
        });
        let mut gen_style = Style::default();
        gen_style.dxf_stated.num_fmt = true;
        sh.cond_formats.push(CondFormat {
            ranges: vec![RangeRef::cell(CellRef::new(3, 0))],
            priority: 4,
            stop_if_true: true,
            rule: CfRule::CellIs { op: CfOperator::Equal, a: "5".into(), b: None, style: Box::new(gen_style) },
        });

        let sh = wb.sheet(0).unwrap();
        assert_eq!(cell_text(&wb, sh, CellRef::new(0, 0)), "30s");
        assert_eq!(cell_text(&wb, sh, CellRef::new(1, 0)), "15m");
        assert_eq!(cell_text(&wb, sh, CellRef::new(2, 0)), "2h, 10m");
        assert_eq!(cell_text(&wb, sh, CellRef::new(3, 0)), "5");
        assert_eq!(value_text(&wb, sh, CellRef::new(2, 0)), "2h, 10m");
        assert_eq!(range_text(&wb, 0, RangeRef::new(CellRef::new(0, 0), CellRef::new(3, 0))), "30s\n15m\n2h, 10m\n5\n");
    }

    #[test]
    fn shared_cf_cache_scans_rule_stats_once_across_loops() {
        use gridcraft_model::{CfRule, CondFormat, NumFmt, Style};
        let mut wb = Workbook::new();
        let sh = wb.sheet_mut(0).unwrap();
        for row in 0..40 {
            sh.set_value(CellRef::new(row, 0), Value::Number((row + 1) as f64));
        }
        sh.cond_formats.push(CondFormat {
            ranges: vec![RangeRef::new(CellRef::new(0, 0), CellRef::new(39, 0))],
            priority: 1,
            stop_if_true: false,
            rule: CfRule::Top10 {
                bottom: false,
                percent: false,
                rank: 5,
                style: Box::new(Style { num_fmt: NumFmt::new("0.00"), ..Style::default() }),
            },
        });
        sh.autofilter = Some(gridcraft_model::AutoFilter { range: RangeRef::new(CellRef::new(0, 0), CellRef::new(39, 0)), criteria: vec![] });

        let sh_ref = wb.sheet(0).unwrap();
        crate::cf::reset_scanned();
        let mut cf = crate::cf::CfCache::new();
        for row in 0..40 {
            let c = CellRef::new(row, 0);
            let t = cell_text_with_cf(&wb, 0, sh_ref, c, &mut cf);
            let v = value_text_with_cf(&wb, 0, sh_ref, c, &mut cf);
            if row >= 35 {
                assert_eq!(t, format!("{}.00", row + 1));
                assert_eq!(v, format!("{}.00", row + 1));
            } else {
                assert_eq!(t, format!("{}", row + 1));
            }
        }
        assert_eq!(crate::cf::scanned_count(), 40, "shared CfCache must build Top10 stats once");

        crate::cf::reset_scanned();
        let vals = crate::cmd::data::filter_values(&wb, 0, 0);
        assert_eq!(vals.len(), 39);
        assert_eq!(crate::cf::scanned_count(), 40, "filter_values must reuse one CfCache across all rows");
    }

    #[test]
    fn explicit_general_dxf_num_fmt_roundtrips_and_overrides_percent_base_format() {
        use gridcraft_model::{CfOperator, CfRule, CondFormat, NumFmt, Style};

        let mut wb = Workbook::new();
        let pct = wb.styles.intern(Style { num_fmt: NumFmt::new("0.00%"), ..Style::default() });
        let mut explicit_general = Style::default();
        explicit_general.dxf_stated.num_fmt = true;
        let mut bold_only = Style::default();
        bold_only.font.bold = true;

        {
            let sh = wb.sheet_mut(0).unwrap();
            for row in 0..2 {
                let c = CellRef::new(row, 0);
                sh.set_value(c, Value::Number(0.25));
                sh.set_style(c, pct);
            }
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::cell(CellRef::new(0, 0))],
                priority: 1,
                stop_if_true: false,
                rule: CfRule::CellIs { op: CfOperator::Greater, a: "0".into(), b: None, style: Box::new(explicit_general) },
            });
            sh.cond_formats.push(CondFormat {
                ranges: vec![RangeRef::cell(CellRef::new(1, 0))],
                priority: 2,
                stop_if_true: false,
                rule: CfRule::CellIs { op: CfOperator::Greater, a: "0".into(), b: None, style: Box::new(bold_only) },
            });
        }

        let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
        let (reopened, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        let sh = reopened.sheet(0).unwrap();
        let st_gen = sh.cond_formats[0].rule.style().unwrap();
        let st_bold = sh.cond_formats[1].rule.style().unwrap();
        assert!(crate::cf::style_sets_num_fmt(st_gen), "explicit General dxf numFmt must satisfy style_sets_num_fmt");
        assert!(!crate::cf::style_sets_num_fmt(st_bold), "bold-only dxf must not claim to set numFmt");
        assert_eq!(cell_text(&reopened, sh, CellRef::new(0, 0)), "0.25");
        assert_eq!(value_text(&reopened, sh, CellRef::new(0, 0)), "0.25");
        assert_eq!(cell_text(&reopened, sh, CellRef::new(1, 0)), "25.00%");
    }
}
