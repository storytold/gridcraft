//! Web table import (Data ▸ Get Data ▸ From Web).
//!
//! The engine stays network-free and wasm-clean: the host (desktop app, CLI) injects a
//! [`WebFetch`] into [`crate::Session::web_source`] that downloads a page; the engine
//! extracts the tables (pure HTML parsing, like the clipboard flavour in
//! [`crate::clip_html`]) and writes them to a sheet. Without a source, `data.fromWeb`
//! returns an error.

/// Rows imported by default (the sheet's row count minus the header row).
pub const DEFAULT_MAX_ROWS: usize = 1_048_575;
/// Columns imported at most (the sheet's column count).
pub const MAX_COLS: usize = 16_384;
/// Page size and download timeout, in bytes and seconds, when the caller gives none.
pub const DEFAULT_MAX_BYTES: usize = 16 * 1024 * 1024;
/// The largest page `extract_tables` accepts, decoded: a `DEFAULT_MAX_BYTES` download in a
/// single-byte charset can grow up to four bytes per character when decoded to UTF-8, so the
/// extractor's bound is the download cap times that, keeping the memory bound coordinated with
/// the fetch's own cap instead of rejecting exactly the pages a non-UTF-8 decode produces.
pub const MAX_DECODED_BYTES: usize = DEFAULT_MAX_BYTES * 4;
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;
/// Longest text a cell holds (Excel's limit).
pub const MAX_TEXT: usize = 32_767;
/// Cells a single imported table holds at most, however wide its `colspan`s or its padding
/// (`clip_html.rs` keeps the same cap for pasted HTML).
pub const MAX_TABLE_CELLS: usize = 1_000_000;
/// Tables a page holds at most. The download cap alone admits hundreds of thousands of empty
/// `<table></table>` repetitions, each a retained `WebTable`; this stops the read.
pub const MAX_TABLES: usize = 1_000;
/// Cells a page holds in all its tables together: 1,000 tables × wide `colspan` padding would
/// otherwise expand far past the download size before the table-count cap applies.
pub const MAX_PAGE_CELLS: usize = 4_000_000;

/// One page download: the URL and the caps the fetch must honour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebRequest {
    pub url: String,
    pub max_rows: usize,
    pub max_cols: usize,
    pub max_bytes: usize,
}

/// One table found on a page: its name, cell text row by row (padded to the widest row).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WebTable {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// Fetches a page and returns its tables. Implementations must honour the caps and never panic.
pub type WebFetch = std::sync::Arc<dyn Fn(&WebRequest) -> Result<Vec<WebTable>, String> + Send + Sync>;

/// The address a fetch is allowed to use: https, and no userinfo. A URL deserialized from a
/// saved workbook reaches the fetch path too, so every caller validates through here before a
/// `WebRequest` is built — a credential typed into the address (`user:pass@host`) would be sent
/// as basic auth and stored verbatim with the connection, and neither may happen.
pub fn validate_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if !url.starts_with("https://") {
        return Err("Only https:// addresses can be imported.".into());
    }
    let authority = url.trim_start_matches("https://").split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err("Web addresses with user and password (`user:pass@host`) can't be imported: the secret would be saved with the workbook.".into());
    }
    Ok(())
}

/// The tables of `html`, in document order. Nested tables are flattened into the cell text of
/// the enclosing table (like Excel's detection, a table inside a cell stays part of the cell).
/// `max_rows`/`max_cols` cap each table's size; longer tables are reported truncated. A page
/// holds at most [`MAX_TABLES`] tables, [`MAX_PAGE_CELLS`] cells across all of them, and each
/// table at most [`MAX_TABLE_CELLS`] cells: a hostile page cannot exhaust memory with
/// thousands of tables or `colspan` padding.
pub fn extract_tables(html: &str, max_rows: usize, max_cols: usize) -> Vec<WebTable> {
    let mut out: Vec<WebTable> = Vec::new();
    if html.len() > MAX_DECODED_BYTES {
        return out;
    }
    let mut rest = html;
    let mut current: Option<TableState> = None;
    let mut depth = 0usize;
    // The page-wide cell budget, shared by every table: what remains when the last table
    // closed is what the next one may take.
    let mut page_budget: usize = MAX_PAGE_CELLS;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else { break };
        let text = rest.get(..lt).unwrap_or_default();
        if let Some(t) = current.as_mut() {
            t.push_text(text);
        }
        rest = rest.get(lt..).unwrap_or_default();
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").and_then(|e| after.get(e + 3..)).unwrap_or_default();
            continue;
        }
        let Some(gt) = tag_end(rest) else { break };
        let tag = rest.get(1..gt).unwrap_or_default();
        rest = rest.get(gt + 1..).unwrap_or_default();
        let closing = tag.starts_with('/');
        let body = tag.trim_start_matches('/');
        let name_end = body.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(body.len());
        let name = body.get(..name_end).unwrap_or_default().to_ascii_lowercase();
        match (name.as_str(), closing) {
            ("style" | "script" | "noscript", false) => {
                let end = format!("</{name}");
                rest = find_ci(rest, &end).and_then(|i| rest.get(i..)).unwrap_or_default();
            }
            ("table", false) => {
                match current.as_mut() {
                    // A nested table: its markup stays part of the enclosing cell's text.
                    Some(t) => {
                        depth += 1;
                        t.push_text(&format!("<{tag}>"));
                    }
                    None => {
                        let id = attr(body, "id");
                        let name = id.filter(|s| !s.trim().is_empty()).map(String::from).unwrap_or_else(|| format!("Table {}", out.len() + 1));
                        current = Some(TableState::new(name, max_rows, max_cols, page_budget));
                        depth = 0;
                    }
                }
            }
            ("table", true) => {
                if depth > 0 {
                    depth -= 1;
                    if let Some(t) = current.as_mut() {
                        t.push_text(&format!("<{tag}>"));
                    }
                } else if let Some(mut t) = current.take() {
                    t.close();
                    page_budget = page_budget.saturating_sub(t.spent());
                    if !t.rows.is_empty() {
                        out.push(WebTable { name: t.name, rows: t.rows });
                    }
                    // A page of nothing but tiny tables is cut short at the aggregate cap,
                    // and a page of wide padded ones at the shared cell budget.
                    if out.len() >= MAX_TABLES || page_budget == 0 {
                        break;
                    }
                }
            }
            ("tr" | "td" | "th", false) => {
                if current.as_ref().is_some_and(|t| t.full()) {
                    continue;
                }
                match current.as_mut() {
                    // Inside a nested table the markup stays part of the enclosing cell's text.
                    Some(t) if depth > 0 => t.push_text(&format!("<{tag}>")),
                    Some(t) if name == "tr" => t.start_row(),
                    Some(t) => {
                        let colspan = attr(body, "colspan").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, max_cols);
                        let raw_row = attr(body, "rowspan").and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1);
                        let rowspan = if raw_row == 0 { None } else { Some(raw_row.clamp(1, max_rows)) };
                        t.start_cell(colspan, rowspan);
                    }
                    None => {}
                }
            }
            // A `rowspan="0"` cell covers the rest of its row group; the group ends here.
            ("thead" | "tbody" | "tfoot", true) => {
                if let Some(t) = current.as_mut() {
                    t.end_row_group();
                }
            }
            // Line breaks and block boundaries separate words in the cell text; `collapse_ws`
            // folds the result, so a `<br>` between two words reads as a space, as in Excel.
            ("br" | "p" | "div" | "li", _) => {
                if let Some(t) = current.as_mut() {
                    t.push_text(" ");
                }
            }
            _ => {}
        }
    }
    out
}

/// One `<table>` being read: cells accumulate text until the next `<td>`/`<th>`/`<tr>`.
struct TableState {
    name: String,
    max_rows: usize,
    max_cols: usize,
    rows: Vec<Vec<String>>,
    row: Vec<String>,
    cell: String,
    cell_open: bool,
    /// The open cell's `colspan` (>= 1) and `rowspan` (`None` for `rowspan="0"`: to this row group's end).
    cell_span: (usize, Option<usize>),
    /// Per column: the row index (exclusive) down to which a `rowspan` from above covers it.
    covered_until: Vec<usize>,
    /// Cells still allowed before the table is cut short (padding included).
    budget: usize,
    /// Cells this table has taken, so the page-wide budget can be updated when it closes.
    spent: usize,
}

impl TableState {
    /// A table whose cell budget is the least of its own cap, its size caps and the page's
    /// remaining share.
    fn new(name: String, max_rows: usize, max_cols: usize, page_budget: usize) -> Self {
        Self {
            name,
            max_rows,
            max_cols,
            rows: Vec::new(),
            row: Vec::new(),
            cell: String::new(),
            cell_open: false,
            cell_span: (1, Some(1)),
            covered_until: Vec::new(),
            budget: MAX_TABLE_CELLS.min(max_rows.saturating_mul(max_cols)).min(page_budget),
            spent: 0,
        }
    }

    /// Cells this table has taken from the budgets.
    fn spent(&self) -> usize {
        self.spent
    }

    /// Text between tags: part of the open cell, or nothing when none is.
    fn push_text(&mut self, text: &str) {
        if self.cell_open {
            self.cell.push_str(text);
        }
    }

    /// `<td>`/`<th>`: close the previous cell, open a new one with its spans.
    fn start_cell(&mut self, colspan: usize, rowspan: Option<usize>) {
        self.close_cell();
        self.cell_open = true;
        self.cell_span = (colspan, rowspan);
    }

    /// `<tr>`: close the previous cell and start the next row.
    fn start_row(&mut self) {
        self.close_cell();
        self.flush_row();
    }

    /// Charges one cell against the budget; false when it is spent (the table is then full).
    fn spend(&mut self) -> bool {
        match self.budget.checked_sub(1) {
            Some(left) => {
                self.budget = left;
                self.spent += 1;
                true
            }
            None => false,
        }
    }

    /// `</thead>`/`</tbody>`/`</tfoot>`: a `rowspan="0"` cell covers no further rows.
    fn end_row_group(&mut self) {
        // The row being built belongs to the group too (its last cell may still be open).
        let until = self.rows.len() + usize::from(!self.row.is_empty() || self.cell_open);
        for u in &mut self.covered_until {
            if *u == usize::MAX {
                *u = until;
            }
        }
    }

    /// Close the current cell into the current row: past the columns a `rowspan` above covers,
    /// then the cell, then an empty placeholder per extra `colspan` column.
    fn close_cell(&mut self) {
        if !self.cell_open {
            return;
        }
        self.cell_open = false;
        let (colspan, rowspan) = self.cell_span;
        let text = collapse_ws(&std::mem::take(&mut self.cell)).chars().take(MAX_TEXT).collect::<String>();
        // The row being built is the next one to flush: index rows.len().
        let r = self.rows.len();
        while self.covered_until.get(self.row.len()).is_some_and(|&until| until > r) {
            if !self.spend() || self.row.len() >= self.max_cols {
                return;
            }
            self.row.push(String::new());
        }
        let start = self.row.len();
        if self.spend() && self.row.len() < self.max_cols {
            self.row.push(text);
        }
        for _ in 1..colspan {
            if !self.spend() || self.row.len() >= self.max_cols {
                break;
            }
            self.row.push(String::new());
        }
        if let Some(span) = rowspan.filter(|s| *s > 1) {
            let until = r.saturating_add(span).min(self.max_rows);
            if self.covered_until.len() < self.row.len() {
                self.covered_until.resize(self.row.len(), 0);
            }
            for u in self.covered_until.get_mut(start..self.row.len()).into_iter().flatten() {
                *u = (*u).max(until);
            }
        } else if rowspan.is_none() {
            // rowspan="0": covers the rest of its row group (thead/tbody/tfoot), capped by the table.
            if self.covered_until.len() < self.row.len() {
                self.covered_until.resize(self.row.len(), 0);
            }
            for u in self.covered_until.get_mut(start..self.row.len()).into_iter().flatten() {
                *u = usize::MAX;
            }
        }
    }

    /// Take the built row into the table, or drop it when empty or past the row cap.
    fn flush_row(&mut self) {
        if self.row.is_empty() || self.rows.len() >= self.max_rows {
            self.row.clear();
            return;
        }
        self.rows.push(std::mem::take(&mut self.row));
    }

    /// The budget is spent: nothing more is read into this table.
    fn full(&self) -> bool {
        self.budget == 0
    }

    /// `</table>`: close the open cell and row, pad every row to the table's width (within the
    /// cell budget; a hostile table pads to no more than it paid for).
    fn close(&mut self) {
        self.close_cell();
        self.flush_row();
        let w = self.rows.iter().map(Vec::len).max().unwrap_or(0).min(self.max_cols);
        for r in &mut self.rows {
            let pad = w.saturating_sub(r.len()).min(self.budget);
            self.budget -= pad;
            self.spent += pad;
            r.resize(r.len() + pad, String::new());
        }
    }
}

/// Cell text as one line: runs of whitespace collapse to single spaces, entities decode.
fn collapse_ws(s: &str) -> String {
    let decoded = decode_entities(s).replace('\u{a0}', " ");
    let mut out = String::with_capacity(decoded.len());
    let mut in_ws = false;
    for ch in decoded.trim().chars() {
        if ch.is_whitespace() {
            in_ws = true;
        } else {
            if in_ws && !out.is_empty() {
                out.push(' ');
            }
            in_ws = false;
            out.push(ch);
        }
    }
    out
}

/// The common named and numeric HTML entities; unknown ones stay as written.
fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or_default());
        rest = rest.get(i..).unwrap_or_default();
        // Entities are short: look for `;` only in the first 32 bytes (byte-wise, so a UTF-8
        // boundary inside a multibyte character cannot fall back to scanning the whole rest).
        let end = rest.as_bytes().iter().take(32).position(|&b| b == b';').unwrap_or(rest.len());
        let ent = rest.get(..end.min(rest.len())).unwrap_or_default();
        let value = match ent.strip_prefix("&#") {
            Some(num) => num
                .strip_prefix('x')
                .map(|h| u32::from_str_radix(h.trim(), 16).ok())
                .unwrap_or_else(|| num.trim().parse::<u32>().ok())
                .and_then(char::from_u32),
            None => named_entity(ent.strip_prefix('&').unwrap_or(ent)),
        };
        match value {
            Some(ch) => {
                out.push(ch);
                rest = rest.get(end + 1..).unwrap_or_default();
            }
            None => {
                out.push('&');
                rest = rest.get(1..).unwrap_or_default();
            }
        }
    }
    out.push_str(rest);
    out
}

/// The character a named HTML entity stands for.
fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "bull" => '•',
        "middot" => '·',
        "deg" => '°',
        "euro" => '€',
        "pound" => '£',
        "yen" => '¥',
        "cent" => '¢',
        "sect" => '§',
        "para" => '¶',
        "times" => '×',
        "divide" => '÷',
        "plusmn" => '±',
        "frac12" => '½',
        "frac14" => '¼',
        "frac34" => '¾',
        "sup2" => '²',
        "sup3" => '³',
        _ => return None,
    })
}

/// The end of the tag starting at `rest` (`<tr class="x">` → index of `>`), tolerating quotes.
fn tag_end(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut quote = 0u8;
    for (i, &b) in bytes.iter().enumerate().skip(1) {
        match (quote, b) {
            (0, b'"') | (0, b'\'') => quote = b,
            (q, b) if q == b => quote = 0,
            (0, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// The value of `attr` in the tag body `body` ("id" of `<table id="x">`).
fn attr<'a>(body: &'a str, attr: &str) -> Option<&'a str> {
    let lower = body.to_ascii_lowercase();
    let needle = format!("{attr}=");
    let i = lower.match_indices(&needle).map(|(i, _)| i).find(|&i| i == 0 || lower.as_bytes()[i - 1].is_ascii_whitespace())?;
    let rest = body.get(i + needle.len()..)?;
    let rest = rest.trim_start();
    let q = rest.as_bytes().first()?;
    if *q == b'"' || *q == b'\'' {
        let rest = rest.get(1..)?;
        let end = rest.find(*q as char)?;
        Some(rest.get(..end).unwrap_or_default())
    } else {
        Some(rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or_default())
    }
}

/// Case-insensitive `find`.
fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    let h = haystack.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h[i..i + n.len()].eq_ignore_ascii_case(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// A long run of `&` with no `;` nearby is processed in linear time, not quadratically.
    fn a_run_of_bare_ampersands_stays_linear() {
        let ampersands = "&".repeat(8 * 1024 * 1024);
        let html = format!("<table><tr><td>{ampersands}</td></tr></table>");
        let t = extract_tables(&html, DEFAULT_MAX_ROWS, MAX_COLS);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].rows[0][0].len(), MAX_TEXT);
        assert!(t[0].rows[0][0].chars().all(|c| c == '&'));
    }

    #[test]
    /// A `<br>` (or a block tag) inside a cell separates words with a space, like Excel's
    /// import of the same cell — `New York<br>USA` must not join into `New YorkUSA`.
    fn a_line_break_inside_a_cell_separates_words() {
        let html = "<table><tr><td>A<br>B</td><td><p>C</p><div>D</div><li>E</li></td></tr></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t[0].rows, vec![vec!["A B", "C D E"]]);
    }

    #[test]
    /// The window scan is byte-safe: a multibyte character right after `&` cannot push the
    /// search past 32 bytes back onto the whole remaining text.
    fn the_entity_window_is_byte_safe() {
        let cell = "&\u{e9}".repeat(1_000);
        let html = format!("<table><tr><td>{cell}</td></tr></table>");
        let t = extract_tables(&html, DEFAULT_MAX_ROWS, MAX_COLS);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].rows[0][0].chars().filter(|c| *c == '&').count(), 1_000);
    }

    #[test]
    /// A named table takes its id for a name; others are numbered.
    fn extracts_named_and_unnamed_tables() {
        let html = "<p>intro</p><TABLE id=\"prices\"><tr><th>Sym</th><th>Price</th></tr>\
                    <tr><td>ABCD</td><td>12.50</td></tr></table>\
                    <table><tr><td>x</td><td>y &amp; z</td></tr></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].name, "prices");
        assert_eq!(t[0].rows, vec![vec!["Sym", "Price"], vec!["ABCD", "12.50"]]);
        assert_eq!(t[1].name, "Table 2");
        assert_eq!(t[1].rows, vec![vec!["x", "y & z"]]);
    }

    #[test]
    /// Rows are padded to the widest; unclosed cells and rows survive broken markup.
    fn ragged_rows_are_padded_and_spans_ignored() {
        let html = "<table><tr><td>a</td><td>b</td><td>c</td></tr><tr><td>d</td></tr></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t[0].rows, vec![vec!["a", "b", "c"], vec!["d", "", ""]]);
    }

    #[test]
    /// Cell text collapses whitespace and decodes entities; scripts never enter it.
    fn collapses_whitespace_decodes_entities_and_skips_scripts() {
        let html = "<table><tr><td>  Red&nbsp;&amp;&nbsp;Blue\n  Widget  </td>\
                    <script>evil(1<2)</script><td>&#65;&#x42;</td></tr></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t[0].rows[0], vec!["Red & Blue Widget", "AB"]);
    }

    #[test]
    /// The row and column caps truncate a too-large table instead of failing.
    fn honours_row_and_col_caps() {
        let html = "<table><tr><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr></table>";
        assert_eq!(extract_tables(html, 1, 100)[0].rows, vec![vec!["1", "2"]]);
        assert_eq!(extract_tables(html, 100, 1)[0].rows, vec![vec!["1"], vec!["3"]]);
    }

    #[test]
    /// Empty or broken markup yields no tables and never panics.
    fn empty_or_broken_markup_yields_nothing_without_panicking() {
        assert!(extract_tables("", 10, 10).is_empty());
        assert!(extract_tables("<table><tr><td>no end", 10, 10).is_empty());
        assert!(extract_tables("<table><tr><td a=\"b>c\">x</td></tr></table>", 10, 10)[0].rows[0][0].contains("x"));
        assert!(extract_tables("<table><!--<tr>--><tr><td>1</td></tr></table>", 10, 10)[0].rows == vec![vec!["1"]]);
    }

    #[test]
    /// `colspan` pads a row and `rowspan` covers cells below.
    fn colspan_and_rowspan_place_their_cells() {
        let html = "<table><tr><th colspan=\"2\">H</th><th>C</th></tr>\
                    <tr><td rowspan=\"2\">R</td><td>1</td><td>2</td></tr>\
                    <tr><td>3</td><td>4</td></tr></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t[0].rows, vec![vec!["H", "", "C"], vec!["R", "1", "2"], vec!["", "3", "4"],]);
    }

    #[test]
    /// `rowspan="0"` covers to the end of its row group.
    fn rowspan_zero_spans_its_own_row_group() {
        let html = "<table><thead><tr><th rowspan=\"0\">A</th><th>B</th></tr>\
                    <tr><th>C</th></tr></thead>\
                    <tbody><tr><td>x</td><td>y</td></tr></tbody></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t[0].rows, vec![vec!["A", "B"], vec!["", "C"], vec!["x", "y"],]);
    }

    #[test]
    /// Wide padded tables share one page-wide cell budget.
    fn wide_colspan_padding_across_many_tables_shares_a_page_budget() {
        // Each table pads one cell to 4,000 columns; without a page-wide budget 1,000 of
        // them retain ~4,000,000 padded cells.
        let html = "<table><tr><td colspan=\"4001\">x</td></tr></table>".repeat(1_000);
        let t = extract_tables(&html, DEFAULT_MAX_ROWS, MAX_COLS);
        let cells: usize = t.iter().map(|tb| tb.rows.iter().map(Vec::len).sum::<usize>()).sum();
        assert!(cells <= MAX_PAGE_CELLS, "cells = {cells}");
        assert!(t.len() <= MAX_TABLES);
    }

    #[test]
    /// Thousands of tiny tables are cut short at the aggregate cell cap.
    fn a_page_of_tiny_tables_is_capped_at_the_aggregate_limit() {
        let html = "<table><tr><td>x</td></tr></table>".repeat(5_000);
        let t = extract_tables(&html, 100, 100);
        assert_eq!(t.len(), MAX_TABLES);
    }

    #[test]
    /// A single hostile table cannot blow past its own cell budget.
    fn a_hostile_table_cannot_blow_the_cell_budget() {
        // One row, each cell spanning the whole column count: without a budget the padding
        // alone would be rows * cols.
        let html = "<table><tr><td colspan=\"10000\">a</td></tr></table>";
        let t = extract_tables(html, 5_000_000, 20_000);
        assert!(t[0].rows.len() <= 5_000_000);
        let cells: usize = t[0].rows.iter().map(Vec::len).sum();
        assert!(cells <= 1_000_000, "cells = {cells}");
    }

    #[test]
    /// Attribute names match whole, not as suffixes.
    fn attr_matches_whole_names_not_suffixes() {
        let html = "<table data-id=\"x\" id=\"prices\"><tr><td>1</td></tr></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t[0].name, "prices");
    }

    #[test]
    /// A nested table's markup stays in the enclosing cell's text.
    fn nested_table_text_stays_in_the_cell() {
        let html = "<table><tr><td>outer <table><tr><td>inner</td></tr></table> end</td></tr></table>";
        let t = extract_tables(html, 100, 100);
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].rows[0][0], "outer <table><tr><td>inner</table> end");
    }
}
