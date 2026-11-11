//! A PivotTable's source as the report reads it: the range a source names, its column names, and
//! the labels its items show. The engine computes reports from these; file writers state them,
//! so both name the same columns and items.

use std::collections::{HashMap, HashSet};

use gridcraft_core::date::{DateSystem, datetime_from_serial};
use gridcraft_core::{CellRef, RangeRef, Value, split_sheet};
use gridcraft_numfmt::{NumberFormat, format_value};

use crate::{PivotDateGroup, Workbook};

const INVALID: &str = "The reference isn't valid.";

/// Month names of grouped dates' items.
pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// The sheet and range (header row included) a source names: a table (without its totals row),
/// a range (whole columns or rows cut to the sheet's used range; one without a sheet is on
/// `default_sheet`), or a defined name holding either, followed a few levels deep.
pub fn resolve(wb: &Workbook, text: &str, default_sheet: usize) -> Result<(usize, RangeRef), String> {
    resolve_inner(wb, text, default_sheet, 0)
}

fn resolve_inner(wb: &Workbook, text: &str, default_sheet: usize, depth: u32) -> Result<(usize, RangeRef), String> {
    let t = text.trim().trim_start_matches('=').trim();
    if t.is_empty() || depth > 4 {
        return Err(INVALID.into());
    }
    if let Some((si, ti)) = wb.table(t)
        && let Some(tb) = wb.sheet(si).and_then(|s| s.tables.get(ti))
    {
        if !tb.header_row {
            return Err("The table needs a header row to be used as a PivotTable source.".into());
        }
        let end_row = tb.range.end.row.saturating_sub(tb.totals_row as u32).max(tb.range.start.row);
        return Ok((si, RangeRef::new(tb.range.start, CellRef::new(end_row, tb.range.end.col))));
    }
    let (sheet, body) = split_sheet(t);
    if let Some(r) = RangeRef::parse(body) {
        let si = match sheet {
            Some(n) => wb.sheet_index(&n).ok_or_else(|| INVALID.to_string())?,
            None => default_sheet,
        };
        let sh = wb.sheet(si).ok_or_else(|| INVALID.to_string())?;
        let r = if r.is_full_cols() || r.is_full_rows() {
            let used = sh.used_range().ok_or_else(|| INVALID.to_string())?;
            r.intersection(&used).ok_or_else(|| INVALID.to_string())?
        } else {
            r
        };
        return Ok((si, r));
    }
    if sheet.is_none()
        && let Some(n) = wb.name(t, default_sheet)
    {
        let f = n.formula.clone();
        return resolve_inner(wb, &f, default_sheet, depth + 1);
    }
    Err(INVALID.into())
}

/// The number format of a source column's items: its first record's.
pub fn column_format(wb: &Workbook, sheet: usize, range: RangeRef, col: u32) -> String {
    match wb.sheet(sheet) {
        Some(sh) if range.end.row > range.start.row => {
            wb.styles.get(sh.style_id(CellRef::new(range.start.row + 1, col))).num_fmt.as_str().to_string()
        }
        _ => "General".into(),
    }
}

/// The source's column names: each header as it shows, trimmed, with a repeated one numbered
/// (`Amount`, `Amount2`). An empty header is an error, as a PivotTable needs labelled columns.
pub fn headers(wb: &Workbook, sheet: usize, range: RangeRef) -> Result<Vec<String>, String> {
    let sh = wb.sheet(sheet).ok_or(INVALID)?;
    let mut out = Vec::new();
    // Names taken so far (lower-case), and the next suffix to try for each repeated header, so
    // that thousands of equal headers don't take cubic time.
    let mut taken: HashSet<String> = HashSet::new();
    let mut next_suffix: HashMap<String, usize> = HashMap::new();
    for c in range.start.col..=range.end.col {
        let at = CellRef::new(range.start.row, c);
        let fmt = NumberFormat::parse(wb.styles.get(sh.style_id(at)).num_fmt.as_str());
        let h = format_value(&sh.value(at).scalar(), &fmt, wb.date_system).plain().trim().to_string();
        if h.is_empty() {
            return Err(
                "The PivotTable field name is not valid. To create a PivotTable, you must use data that is organized as a list with labeled columns."
                    .into(),
            );
        }
        let key = h.to_ascii_lowercase();
        let mut n = h.clone();
        let mut k = next_suffix.get(&key).copied().unwrap_or(2);
        while taken.contains(&n.to_ascii_lowercase()) {
            n = format!("{h}{k}");
            k += 1;
        }
        next_suffix.insert(key, k);
        taken.insert(n.to_ascii_lowercase());
        out.push(n);
    }
    Ok(out)
}

/// A date serial's item under a date grouping: its label and sort key.
pub fn date_group_item(sys: DateSystem, n: f64, group: PivotDateGroup) -> Option<(String, f64)> {
    if group == PivotDateGroup::None {
        return None;
    }
    let dt = datetime_from_serial(sys, n)?;
    let mon = MONTHS.get(dt.month.saturating_sub(1) as usize).copied().unwrap_or("Jan");
    Some(match group {
        PivotDateGroup::Years => (dt.year.to_string(), dt.year as f64),
        PivotDateGroup::Quarters => {
            let q = (dt.month.saturating_sub(1)) / 3 + 1;
            (format!("Qtr{q}"), q as f64)
        }
        PivotDateGroup::Months => (mon.to_string(), dt.month as f64),
        _ => (format!("{}-{mon}", dt.day), (dt.month * 100 + dt.day) as f64),
    })
}

/// The labels a source column's items show: a grouped date's group, a number in the column's
/// format, `(blank)` for an empty cell, else the value's text.
pub struct ItemLabels {
    fmt: NumberFormat,
    group: PivotDateGroup,
    sys: DateSystem,
}

impl ItemLabels {
    /// The labels of column `col` of source `range` on `sheet`, under `group`.
    pub fn new(wb: &Workbook, sheet: usize, range: RangeRef, col: u32, group: PivotDateGroup) -> ItemLabels {
        ItemLabels { fmt: NumberFormat::parse(&column_format(wb, sheet, range, col)), group, sys: wb.date_system }
    }

    pub fn label(&self, v: &Value) -> String {
        item_label(v, self.group, &self.fmt, self.sys)
    }
}

/// The label an item shows (see [`ItemLabels`]), its column's format `fmt` parsed.
pub fn item_label(v: &Value, group: PivotDateGroup, fmt: &NumberFormat, sys: DateSystem) -> String {
    match v {
        Value::Empty => "(blank)".into(),
        Value::Number(n) => match date_group_item(sys, *n, group) {
            Some((label, _)) => label,
            None => format_value(v, fmt, sys).plain(),
        },
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => (if *b { "TRUE" } else { "FALSE" }).into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(_) => item_label(&v.scalar(), group, fmt, sys),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NumFmt, Style};

    fn book(rows: &[&[Value]]) -> Workbook {
        let mut wb = Workbook::new();
        if let Some(sh) = wb.sheet_mut(0) {
            for (r, row) in rows.iter().enumerate() {
                for (c, v) in row.iter().enumerate() {
                    sh.set_value(CellRef::new(r as u32, c as u32), v.clone());
                }
            }
        }
        wb
    }

    fn set_format(wb: &mut Workbook, at: &str, code: &str) {
        let id = wb.styles.intern(Style { num_fmt: NumFmt::new(code), ..Style::default() });
        let at = CellRef::parse(at).unwrap();
        wb.sheet_mut(0).unwrap().set_style(at, id);
    }

    #[test]
    fn headers_are_trimmed_numbered_and_formatted() {
        let mut wb = book(&[&[Value::text(" Amount "), Value::text("amount"), Value::Number(5.0), Value::text("Amount2")]]);
        set_format(&mut wb, "C1", "0.0* ");
        let r = RangeRef::parse("A1:D1").unwrap();
        assert_eq!(headers(&wb, 0, r).unwrap(), ["Amount", "amount2", "5.0", "Amount22"]);
        let empty = book(&[&[Value::text("A"), Value::text("  ")]]);
        assert!(headers(&empty, 0, RangeRef::parse("A1:B1").unwrap()).is_err());
        assert!(headers(&wb, 7, r).is_err());
    }

    #[test]
    fn item_labels_follow_the_column_format_and_grouping() {
        let mut wb = book(&[&[Value::text("When"), Value::text("Cost")], &[Value::Number(46037.0), Value::Number(3.5)]]);
        set_format(&mut wb, "B2", "$0.00");
        let r = RangeRef::parse("A1:B2").unwrap();
        assert_eq!(column_format(&wb, 0, r, 1), "$0.00");
        assert_eq!(column_format(&wb, 0, RangeRef::parse("A1:B1").unwrap(), 1), "General");
        let cost = ItemLabels::new(&wb, 0, r, 1, PivotDateGroup::None);
        assert_eq!(cost.label(&Value::Number(3.5)), "$3.50");
        assert_eq!(cost.label(&Value::Empty), "(blank)");
        assert_eq!(cost.label(&Value::Bool(true)), "TRUE");
        assert_eq!(cost.label(&Value::text("x")), "x");
        // 46037 is 15 Jan 2026.
        let when = ItemLabels::new(&wb, 0, r, 0, PivotDateGroup::Quarters);
        assert_eq!(when.label(&Value::Number(46037.0)), "Qtr1");
        assert_eq!(date_group_item(wb.date_system, 46037.0, PivotDateGroup::Months), Some(("Jan".into(), 1.0)));
        assert_eq!(date_group_item(wb.date_system, 46037.0, PivotDateGroup::Days), Some(("15-Jan".into(), 115.0)));
        assert_eq!(date_group_item(wb.date_system, 46037.0, PivotDateGroup::None), None);
        assert_eq!(date_group_item(wb.date_system, -1.0, PivotDateGroup::Years), None);
    }

    #[test]
    fn resolve_follows_tables_ranges_and_names() {
        let mut wb = book(&[&[Value::text("A")], &[Value::Number(1.0)], &[Value::Number(2.0)]]);
        assert_eq!(resolve(&wb, "=Sheet1!A1:A3", 0).unwrap(), (0, RangeRef::parse("A1:A3").unwrap()));
        assert_eq!(resolve(&wb, "A:A", 0).unwrap(), (0, RangeRef::parse("A1:A3").unwrap()));
        assert!(resolve(&wb, "Nope!A1:A3", 0).is_err());
        assert!(resolve(&wb, "", 0).is_err());
        // A name naming itself stops after a few levels.
        wb.names.push(crate::DefinedName { name: "Loop".into(), scope: None, formula: "Loop".into(), comment: String::new(), hidden: false });
        assert!(resolve(&wb, "Loop", 0).is_err());
    }
}
