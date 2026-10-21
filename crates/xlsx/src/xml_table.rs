//! XML import as a table, the way Excel opens an `.xml` file with "As an XML table".
//!
//! Every distinct element or attribute that carries a value becomes a column (named after the
//! element, `name2`, `name3`… when the same name appears under different parents), in the order
//! the document first reaches it. A repeated element becomes one row per occurrence, and the
//! values around it (its parents' fields) are repeated on each of those rows. Independent
//! repeated elements combine, as in Excel. The result is a sheet with a header row and a table
//! over the data, with the filter buttons and banding Excel gives it.
//!
//! Hostile input is bounded: nesting is limited by the XML reader, and the number of rows and
//! cells (and columns) is capped, with a warning when the cap cuts the data.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{Cell, LineInfo, Sheet, Table, TableColumn, TotalsFn, Workbook};

use crate::xml::{El, parse};
use crate::{IoError, ReadReport};

/// Cells (not counting the header) read from one file.
const MAX_CELLS: usize = 10_000_000;
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

/// Reads an XML document as one table. See the module docs for how the document maps to cells.
pub fn read_xml_table(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let root = parse(bytes)?;
    let mut st = State::default();
    let path = format!("/{}", root.name);
    let rows = rows_of(&root, &path, &mut st);

    let mut report = ReadReport::default();
    if st.names.is_empty() {
        return Err(IoError::Format("the XML file has no values to show as a table".into()));
    }
    if st.truncated {
        report.warn("The XML file is too big to show completely; the rows beyond the limit were left out.");
    }

    let max_rows = (MAX_ROWS as usize).saturating_sub(1);
    let rows: Vec<Row> = rows.into_iter().take(max_rows).collect();
    let ncols = st.names.len().min(MAX_COLS as usize);
    if st.names.len() > ncols {
        report.warn("The XML file has more columns than a sheet holds; the last ones were left out.");
    }

    let mut sheet = Sheet::new("Sheet1");
    // Header: text, one cell per column.
    for (c, name) in st.names.iter().take(ncols).enumerate() {
        sheet.set_value(CellRef::new(0, c as u32), Value::text(name.as_str()));
    }
    let mut fit: Vec<usize> = st.names.iter().take(ncols).map(|n| n.chars().count() + 4).collect();
    for (r, row) in rows.iter().enumerate() {
        for (c, text) in row {
            if *c >= ncols {
                continue;
            }
            let pos = CellRef::new(r as u32 + 1, *c as u32);
            sheet.cells.set(pos, Cell::value(to_value(text)));
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

    let mut wb = Workbook::new();
    wb.sheets = vec![Arc::new(sheet)];
    Ok((wb, report))
}

/// The rows an element produces: its own attributes and text, combined with the rows of each
/// group of same-named children. Occurrences of one child name add rows; different child names
/// combine into every pairing.
fn rows_of(el: &El, path: &str, st: &mut State) -> Vec<Row> {
    let mut base: Row = vec![];
    for (k, v) in &el.attrs {
        let c = st.col(&format!("{path}/@{k}"), k);
        let v = v.trim();
        if !v.is_empty() {
            base.push((c, Arc::from(v)));
        }
    }
    if el.children.is_empty() {
        let text = el.text.trim();
        if !text.is_empty() || el.attrs.is_empty() {
            let c = st.col(path, &el.name);
            if !text.is_empty() {
                base.push((c, Arc::from(text)));
            }
        }
        return vec![base];
    }

    let mut result = vec![base];
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
            // sheet holds before the pairing below gets to cut them.
            for r in rows_of(inst, &child_path, st) {
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
        // Occurrences without any value add no row.
        group.retain(|r| !r.is_empty());
        if group.is_empty() {
            continue;
        }
        // The pairings are the only rows kept (the rows they came from are dropped), so the cell
        // budget bounds what they hold, not a running total over the levels of the document.
        let width = group.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let held = result.iter().map(Vec::len).max().unwrap_or(0);
        let max_pairs = (MAX_CELLS / (held + width).max(1)).min(MAX_ROWS as usize);
        let allowed = (max_pairs / result.len().max(1)).max(1);
        if group.len() > allowed {
            group.truncate(allowed);
            st.truncated = true;
        }
        let mut next: Vec<Row> = Vec::with_capacity(result.len().saturating_mul(group.len()).min(MAX_ROWS as usize));
        'pairs: for a in &result {
            for b in &group {
                if next.len() >= max_pairs {
                    st.truncated = true;
                    break 'pairs;
                }
                let mut row = a.clone();
                row.extend(b.iter().cloned());
                next.push(row);
            }
        }
        result = next;
        if st.truncated {
            break;
        }
    }
    result
}

/// A value read from the file: a plain decimal number is a number, anything else is text.
/// Numbers that only look numeric stay text: ones with leading zeros (CNPJ, CEP, product codes)
/// and ones past 15 digits (access keys), which a number would corrupt.
fn to_value(s: &str) -> Value {
    if is_plain_number(s)
        && let Ok(n) = s.parse::<f64>()
        && n.is_finite()
    {
        return Value::number(n);
    }
    Value::text(s)
}

fn is_plain_number(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = usize::from(b.first() == Some(&b'-'));
    let int_start = i;
    while b.get(i).is_some_and(u8::is_ascii_digit) {
        i += 1;
    }
    let int_len = i - int_start;
    if int_len == 0 || (int_len > 1 && b.get(int_start) == Some(&b'0')) {
        return false;
    }
    let mut digits = int_len;
    if i < b.len() {
        if b.get(i) != Some(&b'.') {
            return false;
        }
        i += 1;
        let frac_start = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == frac_start {
            return false;
        }
        digits += i - frac_start;
    }
    i == b.len() && digits <= 15
}
