//! The tables of an HTML page (From Web's Navigator lists them as `Table 1`, `Table 2`…).
//!
//! A tolerant scanner, not a browser: it walks tags, keeps the text of `<td>`/`<th>` cells of
//! each `<table>` (nested tables become tables of their own), honours `colspan`/`rowspan` by
//! repeating the value, skips `<script>`/`<style>` and decodes character references. Every count
//! is bounded, so a hostile page costs at most a bounded amount of memory and time.

use gridcraft_core::Value;

use super::QTable;

/// Most tables taken from one page.
const MAX_TABLES: usize = 500;
/// Most rows and columns per table.
const MAX_ROWS: usize = 200_000;
const MAX_COLS: usize = 1_000;
/// Largest `colspan`/`rowspan` honoured.
const MAX_SPAN: usize = 1_000;
/// Longest cell text kept.
const MAX_CELL: usize = 32_767;
/// Most cells kept per table.
const MAX_TABLE_CELLS: usize = 5_000_000;
/// Most text kept from one page, in bytes (spans repeat text, so this bounds the copies).
const MAX_PAGE_BYTES: usize = 256 * 1024 * 1024;

/// One table of a page.
#[derive(Clone, Debug, Default)]
pub struct HtmlTable {
    /// `<caption>` text, else the `id`, if any (shown in the Navigator).
    pub caption: String,
    /// Rows of cells; `true` marks a header (`<th>`) cell.
    pub rows: Vec<Vec<(String, bool)>>,
}

impl HtmlTable {
    /// The table as a query table: a first row of `<th>` cells becomes the headers.
    pub fn to_qtable(&self) -> QTable {
        let width = self.rows.iter().map(Vec::len).max().unwrap_or(0).min(MAX_COLS);
        let header = self.rows.first().filter(|r| !r.is_empty() && r.iter().all(|(_, th)| *th));
        let mut columns: Vec<String> = match header {
            Some(h) => {
                (0..width).map(|i| h.get(i).map(|(t, _)| t.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| format!("Column{}", i + 1))).collect()
            }
            None => (1..=width).map(|i| format!("Column{i}")).collect(),
        };
        super::unique_names(&mut columns);
        let mut t = QTable::with_columns(columns);
        for r in self.rows.iter().skip(usize::from(header.is_some())) {
            t.push_row(r.iter().take(width).map(|(s, _)| if s.is_empty() { Value::Empty } else { Value::text(s.as_str()) }).collect());
        }
        t
    }
}

/// A pending `rowspan`: rows still covered, the text and whether it is a header cell.
type Span = Option<(usize, String, bool)>;

struct Open {
    table: HtmlTable,
    /// Pending rowspans by column.
    spans: Vec<Span>,
    row: Option<Vec<(String, bool)>>,
    cell: Option<(String, bool, usize, usize)>,
    caption: Option<String>,
    cells: usize,
}

impl Open {
    fn new(id: Option<String>) -> Open {
        Open { table: HtmlTable { caption: id.unwrap_or_default(), rows: vec![] }, spans: vec![], row: None, cell: None, caption: None, cells: 0 }
    }
    fn end_cell(&mut self) {
        let Some((text, th, colspan, rowspan)) = self.cell.take() else { return };
        let row = self.row.get_or_insert_with(Vec::new);
        let text = collapse(&text);
        for _ in 0..colspan {
            fill_spans(row, &mut self.spans);
            if row.len() >= MAX_COLS {
                break;
            }
            if rowspan > 1 {
                set_span(&mut self.spans, row.len(), Some((rowspan - 1, text.clone(), th)));
            }
            row.push((text.clone(), th));
        }
    }
    /// Ends the current row, keeping it while the page's `budget` of text bytes lasts.
    fn end_row(&mut self, budget: &mut usize) {
        self.end_cell();
        if let Some(mut row) = self.row.take() {
            fill_spans(&mut row, &mut self.spans);
            // Spans reaching past the last cell of this row.
            for col in row.len()..self.spans.len().min(MAX_COLS) {
                if let Some(Some((left, text, th))) = self.spans.get_mut(col).map(Option::take) {
                    while row.len() < col {
                        row.push((String::new(), false));
                    }
                    row.push((text.clone(), th));
                    if left > 1 {
                        set_span(&mut self.spans, col, Some((left - 1, text, th)));
                    }
                }
            }
            self.cells = self.cells.saturating_add(row.len());
            let bytes = row.iter().fold(0usize, |n, (t, _)| n.saturating_add(t.len() + 8));
            if self.table.rows.len() < MAX_ROWS && self.cells <= MAX_TABLE_CELLS && bytes <= *budget {
                *budget -= bytes;
                self.table.rows.push(row);
            }
        }
    }
}

fn set_span(spans: &mut Vec<Span>, col: usize, span: Span) {
    if col >= MAX_COLS {
        return;
    }
    if spans.len() <= col {
        spans.resize(col + 1, None);
    }
    if let Some(slot) = spans.get_mut(col) {
        *slot = span;
    }
}

/// Inserts the values of rowspans from rows above that cover the next column of `row`.
fn fill_spans(row: &mut Vec<(String, bool)>, spans: &mut Vec<Span>) {
    while let Some(Some((left, text, th))) = spans.get_mut(row.len()).map(Option::take) {
        let col = row.len();
        row.push((text.clone(), th));
        if left > 1 {
            set_span(spans, col, Some((left - 1, text, th)));
        }
    }
}

/// Collapses runs of whitespace to one space and trims, like rendered text.
fn collapse(s: &str) -> String {
    let mut out = String::with_capacity(s.len().min(MAX_CELL));
    let mut space = false;
    for c in s.trim().chars() {
        if out.len() >= MAX_CELL {
            break;
        }
        if c.is_whitespace() && c != '\u{a0}' {
            space = true;
            continue;
        }
        if space && !out.is_empty() {
            out.push(' ');
        }
        space = false;
        out.push(if c == '\u{a0}' { ' ' } else { c });
    }
    out
}

/// Decodes `&amp;`, `&#39;`, `&#x27;` and the common named references.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(rest.get(..i).unwrap_or(""));
        let tail = rest.get(i..).unwrap_or("");
        let end = tail.char_indices().take(12).find(|(_, c)| *c == ';').map(|(j, _)| j);
        let decoded = end.and_then(|j| {
            let name = tail.get(1..j)?;
            let c = if let Some(num) = name.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => num.parse::<u32>().ok()?,
                };
                char::from_u32(code).filter(|c| *c != '\0')?
            } else {
                match name {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    "nbsp" => '\u{a0}',
                    "copy" => '©',
                    "reg" => '®',
                    "euro" => '€',
                    "pound" => '£',
                    "yen" => '¥',
                    "cent" => '¢',
                    "mdash" => '—',
                    "ndash" => '–',
                    "hellip" => '…',
                    "laquo" => '«',
                    "raquo" => '»',
                    "deg" => '°',
                    "times" => '×',
                    _ => return None,
                }
            };
            Some((c, j + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = tail.get(len..).unwrap_or("");
            }
            None => {
                out.push('&');
                rest = tail.get(1..).unwrap_or("");
            }
        }
    }
    out.push_str(rest);
    out
}

/// A tag's lower-case name, whether it closes, and its attribute text.
fn tag_parts(inner: &str) -> (String, bool, &str) {
    let inner = inner.trim();
    let (closing, inner) = match inner.strip_prefix('/') {
        Some(r) => (true, r.trim_start()),
        None => (false, inner),
    };
    let end = inner.find(|c: char| c.is_whitespace() || c == '/' || c == '>').unwrap_or(inner.len());
    (inner.get(..end).unwrap_or("").to_ascii_lowercase(), closing, inner.get(end..).unwrap_or(""))
}

/// An attribute value from a tag's attribute text (`colspan="2"`, `id=x`).
fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower.get(from..)?.find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = at == 0 || lower.as_bytes().get(at - 1).is_some_and(|b| b.is_ascii_whitespace());
        let rest = attrs.get(at + name.len()..)?.trim_start();
        let Some(rest) = rest.strip_prefix('=').filter(|_| before_ok) else { continue };
        let rest = rest.trim_start();
        return Some(match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest.get(1..)?.split(q).next().unwrap_or(""),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        });
    }
    None
}

fn span(attrs: &str, name: &str) -> usize {
    attr(attrs, name).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(1).clamp(1, MAX_SPAN)
}

/// The tables of a page, in document order (a table's position is that of its `<table>` tag).
pub fn tables(html: &str) -> Vec<HtmlTable> {
    let mut done: Vec<(usize, HtmlTable)> = Vec::new();
    let mut stack: Vec<(usize, Open)> = Vec::new();
    let mut opened = 0usize;
    let mut budget = MAX_PAGE_BYTES;
    let mut pos = 0usize;
    let bytes_len = html.len();
    while pos < bytes_len {
        let Some(rest) = html.get(pos..) else { break };
        let Some(lt) = rest.find('<') else {
            push_text(&mut stack, rest);
            break;
        };
        push_text(&mut stack, rest.get(..lt).unwrap_or(""));
        let tag_start = pos + lt;
        let after = html.get(tag_start..).unwrap_or("");
        // Comments and doctype.
        if after.starts_with("<!--") {
            pos = after.find("-->").map_or(bytes_len, |e| tag_start + e + 3);
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let inner = after.get(1..gt).unwrap_or("");
        pos = tag_start + gt + 1;
        if inner.starts_with('!') || inner.starts_with('?') {
            continue;
        }
        let (name, closing, attrs) = tag_parts(inner);
        match (name.as_str(), closing) {
            ("script" | "style" | "noscript" | "template", false) => {
                let close = format!("</{name}");
                pos = match find_ci(html.get(pos..).unwrap_or(""), &close) {
                    Some(e) => pos + e,
                    None => bytes_len,
                };
            }
            ("table", false) => {
                if opened < MAX_TABLES && stack.len() < 64 {
                    stack.push((opened, Open::new(attr(attrs, "id").map(decode_entities))));
                }
                opened += 1;
            }
            ("table", true) => {
                if let Some((order, mut t)) = stack.pop() {
                    t.end_row(&mut budget);
                    if let Some(c) = t.caption.take().map(|c| collapse(&c)).filter(|c| !c.is_empty()) {
                        t.table.caption = c;
                    }
                    if !t.table.rows.is_empty() {
                        done.push((order, t.table));
                    }
                }
            }
            ("caption", false) => {
                if let Some((_, t)) = stack.last_mut() {
                    t.caption = Some(String::new());
                }
            }
            ("tr", false) => {
                if let Some((_, t)) = stack.last_mut() {
                    t.end_row(&mut budget);
                    t.row = Some(Vec::new());
                }
            }
            ("tr", true) => {
                if let Some((_, t)) = stack.last_mut() {
                    t.end_row(&mut budget);
                }
            }
            ("td" | "th", false) => {
                if let Some((_, t)) = stack.last_mut() {
                    t.end_cell();
                    t.cell = Some((String::new(), name == "th", span(attrs, "colspan"), span(attrs, "rowspan")));
                }
            }
            ("td" | "th", true) => {
                if let Some((_, t)) = stack.last_mut() {
                    t.end_cell();
                }
            }
            ("br", _) | ("p" | "div" | "li", true) => push_text(&mut stack, " "),
            ("thead" | "tbody" | "tfoot", _) => {
                if let Some((_, t)) = stack.last_mut() {
                    t.end_row(&mut budget);
                }
            }
            _ => {}
        }
    }
    // Unclosed tables still count.
    while let Some((order, mut t)) = stack.pop() {
        t.end_row(&mut budget);
        if !t.table.rows.is_empty() {
            done.push((order, t.table));
        }
    }
    done.sort_by_key(|(o, _)| *o);
    done.into_iter().map(|(_, t)| t).collect()
}

/// The first position of ASCII `needle` in `hay`, ignoring ASCII case, without copying `hay`.
fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| h.get(i..i + n.len()).is_some_and(|w| w.eq_ignore_ascii_case(n)))
}

fn push_text(stack: &mut [(usize, Open)], text: &str) {
    if text.is_empty() {
        return;
    }
    let Some((_, t)) = stack.last_mut() else { return };
    let text = decode_entities(text);
    if let Some(c) = t.caption.as_mut().filter(|_| t.cell.is_none() && t.row.is_none()) {
        if c.len() < MAX_CELL {
            c.push_str(&text);
        }
    } else if let Some((cell, ..)) = t.cell.as_mut()
        && cell.len() < MAX_CELL * 2
    {
        cell.push_str(&text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_with_headers_spans_and_entities() {
        let page = r#"<html><head><script>var t = "<table><tr><td>no</td></tr></table>";</script></head><body>
            <table id="prices"><caption>Fruit &amp; prices</caption>
              <thead><tr><th>Fruit</th><th>Price</th></tr></thead>
              <tbody><tr><td>Apple</td><td>1.20</td></tr>
              <tr><td>Pear &lt;ripe&gt;</td><td>&#36;2</td></tr>
              <tr><td colspan=2>Total</td></tr></tbody>
            </table>
            <table><tr><td rowspan="2">A</td><td>1</td></tr><tr><td>2</td></tr>
              <tr><td><table><tr><td>inner</td></tr></table></td><td>x</td></tr></table>
            <!-- <table><tr><td>commented</td></tr></table> -->"#;
        let ts = tables(page);
        assert_eq!(ts.len(), 3, "{ts:?}");
        assert_eq!(ts[0].caption, "Fruit & prices");
        let t = ts[0].to_qtable();
        assert_eq!(t.columns, vec!["Fruit", "Price"]);
        assert_eq!(t.rows[1], vec![Value::text("Pear <ripe>"), Value::text("$2")]);
        assert_eq!(t.rows[2], vec![Value::text("Total"), Value::text("Total")]);
        let t2 = ts[1].to_qtable();
        assert_eq!(t2.columns, vec!["Column1", "Column2"]);
        assert_eq!(t2.rows[1], vec![Value::text("A"), Value::text("2")]);
        assert_eq!(ts[2].rows, vec![vec![("inner".to_string(), false)]]);
    }

    #[test]
    fn hostile_markup_is_bounded() {
        let deep = "<table>".repeat(10_000) + &"<tr><td colspan=999999 rowspan=999999>x".repeat(50);
        let ts = tables(&deep);
        assert!(ts.len() <= MAX_TABLES);
        assert!(ts.iter().all(|t| t.rows.iter().all(|r| r.len() <= MAX_COLS)));
        assert!(!tables("<table><tr><td>&#xFFFFFFFF;&#0;&bogus;&").is_empty());
        assert!(tables("<").is_empty());
        assert!(tables("<table").is_empty());
    }
}
