//! A bounded look at a table-like Python object, kept on its cell for the card while the workbook is
//! open, and that look as CSV or Markdown. It is never saved: a file has no place for it, and it is
//! only ever as much as the worker sent (`gridcraft_python::protocol::Table` holds the caps).

use std::borrow::Cow;

use gridcraft_core::Value;

/// What the worker sent of a table-like object.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PyTable {
    /// The column names, one per column, or none when the object doesn't name its columns.
    pub columns: Vec<String>,
    pub rows: usize,
    pub cols: usize,
    /// `rows × cols` scalars in row order.
    pub data: Vec<Value>,
    /// The first column holds row labels, which `total_cols` doesn't count.
    pub row_labels: bool,
    /// The object's rows.
    pub total_rows: u64,
    /// The object's columns.
    pub total_cols: u64,
    /// A text here is shorter than in the object.
    pub text_cut: bool,
}

impl PyTable {
    /// The cell at `row`, `col`.
    pub fn cell(&self, row: usize, col: usize) -> Option<&Value> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        self.data.get(row.checked_mul(self.cols)?.checked_add(col)?)
    }

    /// The text of the cell at `row`, `col`: empty when there is no such cell.
    pub fn cell_text(&self, row: usize, col: usize) -> Cow<'_, str> {
        self.cell(row, col).map_or(Cow::Borrowed(""), value_text)
    }

    /// The columns that are the object's: without the row labels.
    pub fn data_cols(&self) -> usize {
        self.cols.saturating_sub(usize::from(self.row_labels))
    }

    /// Whether there is nothing to show: not even a column.
    pub fn is_empty(&self) -> bool {
        self.cols == 0
    }

    /// Whether the object has rows that aren't here.
    pub fn rows_cut(&self) -> bool {
        self.cols > 0 && (self.rows as u64) < self.total_rows
    }

    /// Whether the object has columns that aren't here.
    pub fn cols_cut(&self) -> bool {
        (self.data_cols() as u64) < self.total_cols
    }

    /// Whether anything of the object isn't here: rows, columns or the end of a long text. A person
    /// who copies or saves this must be told.
    pub fn cut(&self) -> bool {
        self.rows_cut() || self.cols_cut() || self.text_cut
    }

    /// The table as CSV, with a header row when its columns have names: RFC 4180, with CRLF after each
    /// row. A text a spreadsheet would run as a formula is guarded ([`csv_field`]). A table with no
    /// columns has no text.
    pub fn to_csv(&self) -> String {
        let mut out = String::new();
        if self.is_empty() {
            return out;
        }
        if !self.columns.is_empty() {
            for (col, name) in self.columns.iter().enumerate() {
                push_csv_field(&mut out, col, name, true, self.cols);
            }
            out.push_str("\r\n");
        }
        for row in 0..self.rows {
            for col in 0..self.cols {
                let text = self.cell_text(row, col);
                let is_text = matches!(self.cell(row, col), Some(Value::Text(_)));
                push_csv_field(&mut out, col, &text, is_text, self.cols);
            }
            out.push_str("\r\n");
        }
        out
    }

    /// The table as a Markdown pipe table. Markdown has no table without a header row, so a table
    /// whose columns have no names gets an empty one. A table with no columns has no text.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        if self.is_empty() {
            return out;
        }
        out.push('|');
        for col in 0..self.cols {
            let name = self.columns.get(col).map_or("", String::as_str);
            out.push(' ');
            push_markdown_text(&mut out, name);
            out.push_str(" |");
        }
        out.push_str("\n|");
        out.push_str(&" --- |".repeat(self.cols));
        out.push('\n');
        for row in 0..self.rows {
            out.push('|');
            for col in 0..self.cols {
                out.push(' ');
                push_markdown_text(&mut out, &self.cell_text(row, col));
                out.push_str(" |");
            }
            out.push('\n');
        }
        out
    }
}

/// A scalar as the table shows it: numbers in full, with no format, and booleans and errors as a
/// sheet writes them.
fn value_text(v: &Value) -> Cow<'_, str> {
    match v {
        Value::Empty | Value::Array(_) => Cow::Borrowed(""),
        Value::Number(n) => Cow::Owned(n.to_string()),
        Value::Text(t) => Cow::Borrowed(t.as_ref()),
        Value::Bool(true) => Cow::Borrowed("TRUE"),
        Value::Bool(false) => Cow::Borrowed("FALSE"),
        Value::Error(e) => Cow::Borrowed(e.as_str()),
    }
}

/// Whether a spreadsheet opening the file would take this text as a formula: it starts with `=`,
/// `+`, `-` or `@`, or with a tab or a carriage return that a reader may strip. Only a text: the
/// number -5 is a number.
fn csv_formula_like(text: &str) -> bool {
    text.starts_with(['=', '+', '-', '@', '\t', '\r'])
}

/// Appends one field. A text that could run as a formula is quoted with a `'` before it, the usual
/// guard: it stays readable, and no spreadsheet runs it. A column's only field, when empty, is
/// quoted too, as a blank line is no record to many readers.
fn push_csv_field(out: &mut String, col: usize, text: &str, is_text: bool, cols: usize) {
    if col > 0 {
        out.push(',');
    }
    let guarded = is_text && csv_formula_like(text);
    if !guarded && !text.contains([',', '"', '\n', '\r']) && !(text.is_empty() && cols == 1) {
        out.push_str(text);
        return;
    }
    out.push('"');
    if guarded {
        out.push('\'');
    }
    out.push_str(&text.replace('"', "\"\""));
    out.push('"');
}

/// Appends a cell's text: a `|` would end the cell and a line break the row, and a `\` before a
/// `|` would undo its escape, so those are escaped and line breaks become `<br>`.
fn push_markdown_text(out: &mut String, text: &str) {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '\n' => out.push_str("<br>"),
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push_str("<br>");
            }
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::CellError;

    fn table(columns: &[&str], rows: usize, cols: usize, data: Vec<Value>) -> PyTable {
        PyTable {
            columns: columns.iter().map(|c| c.to_string()).collect(),
            rows,
            cols,
            data,
            total_rows: rows as u64,
            total_cols: cols as u64,
            ..PyTable::default()
        }
    }

    fn one(text: &str) -> PyTable {
        table(&[], 1, 2, vec![Value::text(text), Value::Number(1.0)])
    }

    #[test]
    fn csv_quotes_what_it_must_and_nothing_else() {
        let t = table(
            &["a", "b,c"],
            3,
            2,
            vec![
                Value::Number(1.5),
                Value::text("plain text"),
                Value::Bool(true),
                Value::text("say \"hi\", now"),
                Value::Empty,
                Value::Error(CellError::NA),
            ],
        );
        assert_eq!(t.to_csv(), "a,\"b,c\"\r\n1.5,plain text\r\nTRUE,\"say \"\"hi\"\", now\"\r\n,#N/A\r\n");
        // Line breaks of every kind stay inside their field.
        assert_eq!(one("x\ny").to_csv(), "\"x\ny\",1\r\n");
        assert_eq!(one("x\r\ny").to_csv(), "\"x\r\ny\",1\r\n");
        assert_eq!(one("x\ry").to_csv(), "\"x\ry\",1\r\n");
        // A quote in the middle, and at the ends.
        assert_eq!(one("\"").to_csv(), "\"\"\"\",1\r\n");
        // A name of a single column that is empty, and a column of empty cells.
        let blank = table(&[""], 2, 1, vec![Value::Empty, Value::text("")]);
        assert_eq!(blank.to_csv(), "\"\"\r\n\"\"\r\n\"\"\r\n");
        assert_eq!(table(&["a"], 0, 1, vec![]).to_csv(), "a\r\n", "only the header");
    }

    #[test]
    fn csv_guards_a_text_a_spreadsheet_would_run() {
        for (text, want) in [
            ("=1+1", "\"'=1+1\",1\r\n"),
            ("=HYPERLINK(\"http://x\",\"y\")", "\"'=HYPERLINK(\"\"http://x\"\",\"\"y\"\")\",1\r\n"),
            ("+1", "\"'+1\",1\r\n"),
            ("-1", "\"'-1\",1\r\n"),
            ("@SUM(A1)", "\"'@SUM(A1)\",1\r\n"),
            ("\tx", "\"'\tx\",1\r\n"),
            ("\rx", "\"'\rx\",1\r\n"),
            ("\r\n=1", "\"'\r\n=1\",1\r\n"),
            ("=", "\"'=\",1\r\n"),
            ("=a,b", "\"'=a,b\",1\r\n"),
            // Only at the start.
            ("a=1", "a=1,1\r\n"),
            (" =1", " =1,1\r\n"),
            ("1-1", "1-1,1\r\n"),
            ("", ",1\r\n"),
        ] {
            assert_eq!(one(text).to_csv(), want, "{text:?}");
        }
        // A column's name is a text too.
        assert_eq!(table(&["=cmd"], 0, 1, vec![]).to_csv(), "\"'=cmd\"\r\n");
        // Numbers, booleans and errors aren't: -5 stays a number.
        let others = table(&[], 1, 3, vec![Value::Number(-5.0), Value::Bool(false), Value::Error(CellError::Value)]);
        assert_eq!(others.to_csv(), "-5,FALSE,#VALUE!\r\n");
    }

    #[test]
    fn markdown_escapes_what_would_break_a_row() {
        let t = table(&["a|b", "c"], 2, 2, vec![Value::Number(1.0), Value::text("x|y"), Value::Empty, Value::Bool(true)]);
        assert_eq!(t.to_markdown(), "| a\\|b | c |\n| --- | --- |\n| 1 | x\\|y |\n|  | TRUE |\n");
        let cell = |text: &str| table(&[], 1, 1, vec![Value::text(text)]).to_markdown();
        assert_eq!(cell("x\ny"), "|  |\n| --- |\n| x<br>y |\n", "with an empty header row");
        assert_eq!(cell("x\r\ny"), "|  |\n| --- |\n| x<br>y |\n");
        assert_eq!(cell("x\ry"), "|  |\n| --- |\n| x<br>y |\n");
        assert_eq!(cell("\n\n"), "|  |\n| --- |\n| <br><br> |\n");
        // A backslash before a pipe would undo the pipe's escape.
        assert_eq!(cell("a\\|b"), "|  |\n| --- |\n| a\\\\\\|b |\n");
        assert_eq!(cell("c:\\d"), "|  |\n| --- |\n| c:\\\\d |\n");
        // No formula guard here: Markdown runs nothing.
        assert_eq!(cell("=1+1"), "|  |\n| --- |\n| =1+1 |\n");
        // Always one line per row, whatever a cell holds.
        let lines = table(&["h\nh"], 1, 1, vec![Value::text("a\nb\r\nc|d")]).to_markdown();
        assert_eq!(lines.lines().count(), 3, "{lines}");
    }

    #[test]
    fn a_table_with_no_columns_has_no_text() {
        let none = table(&[], 3, 0, vec![]);
        assert!(none.is_empty());
        assert_eq!((none.to_csv(), none.to_markdown()), (String::new(), String::new()));
        assert!(!none.cut(), "there is nothing to have cut");
    }

    #[test]
    fn it_knows_what_it_left_out() {
        let whole = table(&["a"], 1, 1, vec![Value::Number(1.0)]);
        assert!(!whole.rows_cut() && !whole.cols_cut() && !whole.cut());
        assert!(PyTable { total_rows: 2, ..whole.clone() }.rows_cut());
        assert!(PyTable { total_cols: 2, ..whole.clone() }.cols_cut());
        assert!(PyTable { text_cut: true, ..whole.clone() }.cut());
        // Row labels aren't one of the object's columns.
        let labelled = PyTable { row_labels: true, cols: 2, total_cols: 1, ..whole.clone() };
        assert_eq!(labelled.data_cols(), 1);
        assert!(!labelled.cols_cut());
        assert!(PyTable { row_labels: true, cols: 2, total_cols: 2, ..whole.clone() }.cols_cut());
    }

    #[test]
    fn a_cell_past_the_shape_has_no_text() {
        let t = table(&[], 1, 2, vec![Value::text("a"), Value::text("b")]);
        assert_eq!(t.cell_text(0, 1), "b");
        assert_eq!(t.cell_text(0, 2), "", "not the next row's");
        assert_eq!(t.cell_text(1, 0), "");
        assert_eq!(t.cell_text(usize::MAX, usize::MAX), "");
        assert_eq!(t.cell(usize::MAX, 1), None);
    }
}
