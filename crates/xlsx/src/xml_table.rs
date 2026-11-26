//! XML import as a table, the way Excel opens an `.xml` file with "As an XML table".
//!
//! Every distinct element or attribute that carries a value becomes a column (named after the
//! element, `name2`, `name3`… when the same name appears under different parents), in the order
//! the document first reaches it. A repeated element becomes one row per occurrence, and the
//! values around it (its parents' fields) are repeated on each of those rows. Independent
//! repeated elements stack, one after the other, as in Excel: `<p>1</p><p>2</p><q>a</q>` is three
//! rows. The result is a sheet with a header row and a table over the data, with the filter
//! buttons and banding Excel gives it.
//!
//! Types are decided per column, as Excel does: a column whose values are all numbers is
//! numeric, all `true`/`false` is logical, all dates (`2024-03-05`, `2024-03-05T10:00:00`) holds
//! dates, and any other column is text for every cell. A value that only looks like a number
//! (a leading zero, as in a tax id, or more than 15 digits, as in an access key) is not one, so
//! such a column stays text instead of being corrupted.
//!
//! Hostile input is bounded: the elements kept, nesting, the number of rows and cells (and
//! columns) are capped, with a warning when a cap cuts the data.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gridcraft_core::date::serial_from_ymd;
use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, LineInfo, NumFmt, Sheet, Style, StyleId, Table, TableColumn, TotalsFn, Workbook};

use crate::xml::{El, parse_with_node_limit};
use crate::{IoError, ReadReport};

/// Cells (not counting the header) read from one file.
const MAX_CELLS: usize = 10_000_000;
/// Elements an `.xml` file may hold. An XLSX part is held to far fewer; here the file is the data,
/// so a long list of records has to fit (a few hundred thousand invoices is some millions of
/// elements).
const MAX_NODES: usize = 5_000_000;
/// Most characters a column grows to when it fits its content.
const MAX_FIT_CHARS: usize = 50;
/// Data rows looked at to size a column.
const FIT_ROWS: usize = 200;

/// One output row: the values it holds, by column.
type Row = Vec<(usize, Arc<str>)>;

#[derive(Default)]
struct State {
    /// Column header names, in order of first appearance. Always unique (case-insensitive).
    names: Vec<String>,
    /// Column of a path such as `/nfeProc/NFe/infNFe/@Id`.
    index: HashMap<String, usize>,
    /// Lowercased names taken, with the next numeric suffix to try.
    taken: HashMap<String, usize>,
    /// Columns that have an empty element or attribute somewhere: Excel makes those text.
    empty: HashSet<usize>,
    truncated: bool,
}

impl State {
    /// The column for `path`, created with header `name` the first time it is seen.
    fn col(&mut self, path: &str, name: &str) -> usize {
        if let Some(&c) = self.index.get(path) {
            return c;
        }
        let base = if name.is_empty() { "Column" } else { name };
        let key = base.to_ascii_lowercase();
        let unique = match self.taken.get(&key).copied() {
            None => {
                self.taken.insert(key, 2);
                base.to_string()
            }
            Some(mut k) => loop {
                let candidate = format!("{base}{k}");
                k += 1;
                let ckey = candidate.to_ascii_lowercase();
                if !self.taken.contains_key(&ckey) {
                    self.taken.insert(ckey, 2);
                    self.taken.insert(key, k);
                    break candidate;
                }
            },
        };
        let c = self.names.len();
        self.names.push(unique);
        self.index.insert(path.to_string(), c);
        c
    }
}

/// How a column's values are stored.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Number,
    Bool,
    Date,
    DateTime,
    Text,
}

/// What every value of a column so far could be.
#[derive(Clone, Copy)]
struct Fit {
    number: bool,
    bool: bool,
    date: bool,
    time: bool,
    any: bool,
}

impl Fit {
    fn new() -> Fit {
        Fit { number: true, bool: true, date: true, time: false, any: false }
    }

    fn see(&mut self, v: &str) {
        self.any = true;
        self.number &= number(v).is_some();
        self.bool &= matches!(v, "true" | "false");
        if self.date {
            match date(v) {
                Some((_, time)) => self.time |= time,
                None => self.date = false,
            }
        }
    }

    fn kind(&self, has_empty: bool) -> Kind {
        if has_empty || !self.any {
            Kind::Text
        } else if self.number {
            Kind::Number
        } else if self.bool {
            Kind::Bool
        } else if self.date {
            if self.time { Kind::DateTime } else { Kind::Date }
        } else {
            Kind::Text
        }
    }
}

/// Reads an XML document as one table. See the module docs for how the document maps to cells.
pub fn read_xml_table(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let root = parse_with_node_limit(bytes, MAX_NODES)?;
    let mut st = State::default();
    let path = format!("/{}", root.name);
    let rows = rows_of(&root, &path, &mut st);

    let mut report = ReadReport::default();
    let mut wb = Workbook::new();
    // An empty document (no value anywhere) opens as a blank sheet, as in Excel.
    if rows.iter().all(Vec::is_empty) {
        wb.sheets = vec![Arc::new(Sheet::new("Sheet1"))];
        return Ok((wb, report));
    }
    let max_rows = (MAX_ROWS as usize).saturating_sub(1);
    // The header takes a row, so one data row fewer than a sheet's rows fits.
    if st.truncated || rows.len() > max_rows {
        report.warn("The XML file is too big to show completely; the rows beyond the limit were left out.");
    }
    let rows: Vec<Row> = rows.into_iter().take(max_rows).collect();
    let ncols = st.names.len().min(MAX_COLS as usize);
    if st.names.len() > ncols {
        report.warn("The XML file has more columns than a sheet holds; the last ones were left out.");
    }

    // The type of each column, from all of its values.
    let mut fits = vec![Fit::new(); ncols];
    for row in &rows {
        for (c, text) in row {
            if let Some(f) = fits.get_mut(*c) {
                f.see(text);
            }
        }
    }
    let kinds: Vec<Kind> = fits.iter().enumerate().map(|(c, f)| f.kind(st.empty.contains(&c))).collect();
    let mut style = |code: &str| wb.styles.intern(Style { num_fmt: NumFmt::new(code), ..Style::default() });
    let (text_style, date_style, time_style) = (style("@"), style("m/d/yyyy"), style("m/d/yyyy h:mm"));

    let mut sheet = Sheet::new("Sheet1");
    // Header: text, one cell per column.
    for (c, name) in st.names.iter().take(ncols).enumerate() {
        sheet.set_value(CellRef::new(0, c as u32), Value::text(name.as_str()));
    }
    let mut fit: Vec<usize> = st.names.iter().take(ncols).map(|n| n.chars().count() + 4).collect();
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row {
            let Some(kind) = kinds.get(*c).copied() else { continue };
            let pos = CellRef::new(r as u32 + 1, *c as u32);
            let (value, style) = match kind {
                Kind::Number => (number(text).map_or_else(|| Value::text(&**text), Value::number), StyleId::DEFAULT),
                Kind::Bool => (Value::Bool(&**text == "true"), StyleId::DEFAULT),
                Kind::Date => (date(text).map_or_else(|| Value::text(&**text), |(n, _)| Value::number(n)), date_style),
                Kind::DateTime => (date(text).map_or_else(|| Value::text(&**text), |(n, _)| Value::number(n)), time_style),
                Kind::Text => (Value::text(&**text), text_style),
            };
            sheet.cells.set(pos, Cell { value, formula: None, style, py: None });
            if r < FIT_ROWS
                && let Some(f) = fit.get_mut(*c)
            {
                *f = (*f).max(text.chars().count() + 1);
            }
        }
    }
    // Columns fit their content (width in pixels: 7 per character plus padding), as Excel's do.
    for (c, chars) in fit.iter().enumerate() {
        let width = (chars.clamp(&8, &MAX_FIT_CHARS) * 7 + 5) as f32;
        sheet.cols.insert(c as u32, LineInfo { size: Some(width), ..Default::default() });
    }

    if !rows.is_empty() {
        let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(rows.len() as u32, ncols as u32 - 1));
        sheet.tables.push(Table {
            id: 1,
            name: "Table1".into(),
            range,
            header_row: true,
            totals_row: false,
            columns: st
                .names
                .iter()
                .take(ncols)
                .map(|n| TableColumn { name: n.clone(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None })
                .collect(),
            style: "TableStyleMedium2".into(),
            banded_rows: true,
            banded_cols: false,
            first_col: false,
            last_col: false,
            filter_button: true,
            filter: vec![],
            table_type: None,
            connection_id: None,
        });
    }

    wb.sheets = vec![Arc::new(sheet)];
    Ok((wb, report))
}

/// The rows an element produces. Its own attributes and text, and the rows of every child that
/// is a single record, go on each row. Children that are several records (a repeated element, or
/// one holding a repeated element) each add their own rows, one group after the other.
fn rows_of(el: &El, path: &str, st: &mut State) -> Vec<Row> {
    let mut base: Row = vec![];
    for (k, v) in &el.attrs {
        let c = st.col(&format!("{path}/@{k}"), k);
        let v = v.trim();
        if v.is_empty() {
            st.empty.insert(c);
        } else {
            base.push((c, Arc::from(v)));
        }
    }
    if el.children.is_empty() {
        let text = el.text.trim();
        if !text.is_empty() || el.attrs.is_empty() {
            let c = st.col(path, &el.name);
            if text.is_empty() {
                st.empty.insert(c);
            } else {
                base.push((c, Arc::from(text)));
            }
        }
        return vec![base];
    }

    let mut single = base;
    let mut stacked: Vec<Vec<Row>> = vec![];
    let mut done: Vec<&str> = vec![];
    for child in &el.children {
        if done.contains(&child.name.as_str()) {
            continue;
        }
        done.push(child.name.as_str());
        let child_path = format!("{path}/{}", child.name);
        let mut group: Vec<Row> = vec![];
        let mut group_cells = 0usize;
        for inst in el.children.iter().filter(|c| c.name == child.name) {
            // Many occurrences, each with many rows of its own, must not pile up past what a
            // sheet holds.
            for r in rows_of(inst, &child_path, st) {
                if r.is_empty() {
                    continue; // an occurrence without any value adds no row
                }
                if group.len() >= MAX_ROWS as usize || group_cells.saturating_add(r.len()) > MAX_CELLS {
                    st.truncated = true;
                    break;
                }
                group_cells += r.len();
                group.push(r);
            }
            if st.truncated {
                break;
            }
        }
        match group.len() {
            0 => {}
            1 => single.extend(group.swap_remove(0)),
            _ => stacked.push(group),
        }
        if st.truncated {
            break;
        }
    }
    if stacked.is_empty() {
        return vec![single];
    }
    let mut out: Vec<Row> = vec![];
    let mut cells = 0usize;
    'groups: for group in stacked {
        for r in group {
            if out.len() >= MAX_ROWS as usize || cells.saturating_add(single.len() + r.len()) > MAX_CELLS {
                st.truncated = true;
                break 'groups;
            }
            cells += single.len() + r.len();
            let mut row = single.clone();
            row.extend(r);
            out.push(row);
        }
    }
    out
}

/// A plain decimal number: digits, optionally a fraction and an exponent (`2.50`, `1e3`). Not a
/// number: a leading zero (tax ids, postal codes, product codes), more than 15 digits (access
/// keys, which a number would corrupt), `.5`, `5%` or `1,5`.
fn number(s: &str) -> Option<f64> {
    let b = s.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let int_start = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let int_len = i - int_start;
    if int_len == 0 || (int_len > 1 && b.get(int_start) == Some(&b'0')) {
        return None;
    }
    let mut digits = int_len;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let frac_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == frac_start {
            return None;
        }
        digits += i - frac_start;
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        let exp_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == exp_start || i - exp_start > 3 {
            return None;
        }
    }
    if i != b.len() || digits > 15 {
        return None;
    }
    s.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// An XML Schema date (`2024-03-05`) or date-time (`2024-03-05T10:00:00`, optional fraction), as
/// a serial number, and whether it has a time. A time zone makes it text.
fn date(s: &str) -> Option<(f64, bool)> {
    let (d, t) = match s.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (s, None),
    };
    let mut parts = d.split('-');
    let (y, m, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || y.len() != 4 || m.len() != 2 || day.len() != 2 {
        return None;
    }
    let (y, m, day): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, day.parse().ok()?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days).contains(&day) {
        return None;
    }
    let serial = serial_from_ymd(DateSystem::D1900, y, m, day)?;
    let Some(t) = t else { return Some((serial, false)) };
    let (hms, frac) = match t.split_once('.') {
        Some((a, f)) if !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit()) => (a, f.parse::<f64>().ok()? / 10f64.powi(f.len() as i32)),
        Some(_) => return None,
        None => (t, 0.0),
    };
    let mut p = hms.split(':');
    let (h, mi, sec) = (p.next()?, p.next()?, p.next()?);
    if p.next().is_some() || h.len() != 2 || mi.len() != 2 || sec.len() != 2 {
        return None;
    }
    let (h, mi, sec): (u32, u32, u32) = (h.parse().ok()?, mi.parse().ok()?, sec.parse().ok()?);
    if h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    Some((serial + (f64::from(h) * 3600.0 + f64::from(mi) * 60.0 + f64::from(sec) + frac) / 86_400.0, true))
}
