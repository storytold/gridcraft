//! References into linked workbooks, as typed and as shown.
//!
//! Files and the model only hold `[n]` (`[1]Sheet1!A1`, `[1]!Rate`), the position of the link in
//! `Workbook::external_books`. People type and read the linked file instead: its name
//! (`[Source.xlsx]Sheet1!A1`, `Source.xlsx!Rate`) or its path
//! (`'C:\links\[Source.xlsx]Sheet1'!A1`, `'C:\links\Source.xlsx'!Rate`).

use gridcraft_formula::{Expr, SheetSel};

use crate::workbook::{ExternalBook, ExternalName, ExternalSheet, Workbook};

impl Workbook {
    /// The link `[n]` (1-based) to the workbook at `path`: the link to that path or, for a bare
    /// file name, to a file of that name; otherwise a new link, which holds no values until the
    /// file is read (references into it are `#REF!`, as in Excel when the file is missing).
    pub fn link_book(&mut self, path: &str) -> usize {
        let bare = !path.contains(['\\', '/']);
        // `C:/links/Source.xlsx` is the same file as `C:\links\Source.xlsx`.
        let same = |a: &str, b: &str| a.replace('/', "\\").eq_ignore_ascii_case(&b.replace('/', "\\"));
        let found = self
            .external_books
            .iter()
            .position(|b| !b.path.is_empty() && if bare { b.file_name().eq_ignore_ascii_case(path) } else { same(&b.path, path) });
        let i = found.unwrap_or_else(|| {
            self.external_books.push(ExternalBook { path: path.to_string(), added: true, ..Default::default() });
            self.external_books.len() - 1
        });
        i + 1
    }

    /// References to another workbook by file name or path, as typed, made references through
    /// its link (`'C:\links\[Source.xlsx]Sheet1'!A1` → `[1]Sheet1!A1`, `Source.xlsx!Rate` →
    /// `[1]!Rate`), adding the link when there is none to that file.
    pub fn link_typed_books(&mut self, e: Expr) -> Expr {
        e.map(&mut |e| match e {
            Expr::Ref(mut r) => {
                r.sheet = match r.sheet {
                    SheetSel::Named(s) => SheetSel::Named(self.linked_sheet(&s, None).unwrap_or(s)),
                    SheetSel::Span(a, b) => SheetSel::Span(self.linked_sheet(&a, Some(&b)).unwrap_or(a), b),
                    other => other,
                };
                Expr::Ref(r)
            }
            Expr::Name(n) => Expr::Name(self.linked_name(&n).unwrap_or(n)),
            other => other,
        })
    }

    /// `[n]` written out as the linked file, as the formula bar and the Name Manager show it
    /// (`[1]Sheet1!A1` → `'C:\links\[Source.xlsx]Sheet1'!A1`).
    pub fn show_linked_books(&self, e: Expr) -> Expr {
        e.map(&mut |e| match e {
            Expr::Ref(mut r) => {
                r.sheet = match r.sheet {
                    SheetSel::Named(s) => SheetSel::Named(self.shown_sheet(&s).unwrap_or(s)),
                    SheetSel::Span(a, b) => SheetSel::Span(self.shown_sheet(&a).unwrap_or(a), b),
                    other => other,
                };
                Expr::Ref(r)
            }
            Expr::Name(n) => Expr::Name(self.shown_name(&n).unwrap_or(n)),
            other => other,
        })
    }

    /// [`Workbook::link_typed_books`] on formula text (without `=`); text that doesn't parse is
    /// returned as it is.
    pub fn link_typed_text(&mut self, text: &str) -> String {
        match gridcraft_formula::parse(text) {
            Ok(e) => gridcraft_formula::print(&self.link_typed_books(e)),
            Err(_) => text.to_string(),
        }
    }

    /// [`Workbook::show_linked_books`] on formula text (without `=`).
    pub fn shown_formula(&self, text: &str) -> String {
        if self.external_books.iter().all(|b| b.path.is_empty()) {
            return text.to_string();
        }
        match gridcraft_formula::parse(text) {
            Ok(e) => gridcraft_formula::print(&self.show_linked_books(e)),
            Err(_) => text.to_string(),
        }
    }

    /// `C:\links\[Source.xlsx]Sheet1` → `[1]Sheet1`, adding the sheet (and `last`, the end of a
    /// sheet range) to the link when it doesn't know it.
    fn linked_sheet(&mut self, typed: &str, last: Option<&str>) -> Option<String> {
        let (path, sheet) = typed_book(typed)?;
        let n = self.link_book(&path);
        let book = self.external_books.get_mut(n - 1)?;
        for s in std::iter::once(sheet).chain(last) {
            if book.sheet_index(s).is_none() {
                book.sheets.push(ExternalSheet { name: s.to_string(), refresh_error: true, ..Default::default() });
            }
        }
        Some(format!("[{n}]{sheet}"))
    }

    /// `Source.xlsx!Rate` → `[1]!Rate`, `C:\links\[Source.xlsx]Sheet1!Rate` → `[1]Sheet1!Rate`,
    /// adding the name to the link when it doesn't know it.
    fn linked_name(&mut self, typed: &str) -> Option<String> {
        let (qualifier, name) = typed.rsplit_once('!')?;
        let (path, sheet) = match typed_book(qualifier) {
            Some((path, sheet)) => (path, sheet),
            None if self.sheet_index(qualifier).is_none() && looks_like_file(qualifier) => (qualifier.to_string(), ""),
            None => return None,
        };
        let n = self.link_book(&path);
        let book = self.external_books.get_mut(n - 1)?;
        let scope = if sheet.is_empty() {
            None
        } else {
            Some(book.sheet_index(sheet).unwrap_or_else(|| {
                book.sheets.push(ExternalSheet { name: sheet.to_string(), refresh_error: true, ..Default::default() });
                book.sheets.len() - 1
            }))
        };
        if !book.names.iter().any(|x| x.sheet == scope && x.name.eq_ignore_ascii_case(name)) {
            book.names.push(ExternalName { name: name.to_string(), sheet: scope, refers_to: String::new() });
        }
        Some(format!("[{n}]{sheet}!{name}"))
    }

    /// The linked book `[n]` that starts `s`, with the rest of `s`.
    fn book_of<'a>(&self, s: &'a str) -> Option<(&ExternalBook, &'a str)> {
        let (n, rest) = s.strip_prefix('[')?.split_once(']')?;
        let n: usize = n.parse().ok()?;
        let book = self.external_books.get(n.checked_sub(1)?)?;
        (!book.path.is_empty()).then_some((book, rest))
    }

    /// `[1]Sheet1` → `C:\links\[Source.xlsx]Sheet1`.
    fn shown_sheet(&self, s: &str) -> Option<String> {
        let (book, sheet) = self.book_of(s)?;
        Some(format!("{}[{}]{sheet}", book.folder(), book.file_name()))
    }

    /// `[1]!Rate` → `C:\links\Source.xlsx!Rate`, `[1]Sheet1!Rate` →
    /// `C:\links\[Source.xlsx]Sheet1!Rate`: bare, as a parsed name is; the printer quotes them.
    fn shown_name(&self, s: &str) -> Option<String> {
        let (book, rest) = self.book_of(s)?;
        let (sheet, name) = rest.split_once('!')?;
        let qualifier = if sheet.is_empty() { book.path.clone() } else { format!("{}[{}]{sheet}", book.folder(), book.file_name()) };
        Some(format!("{qualifier}!{name}"))
    }
}

/// `C:\links\[Source.xlsx]Sheet1` → (`C:\links\Source.xlsx`, `Sheet1`). `None` for `[1]Sheet1`,
/// already a link's number, and for anything without a file name in brackets (sheet names
/// can't hold brackets).
fn typed_book(s: &str) -> Option<(String, &str)> {
    let (folder, rest) = s.split_once('[')?;
    let (file, sheet) = rest.split_once(']')?;
    if file.is_empty() || (folder.is_empty() && file.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    Some((format!("{folder}{file}"), sheet))
}

/// Whether the qualifier of `Source.xlsx!Rate` names a file: it has a folder or an extension
/// (and isn't a linked sheet, `[1]Data.v2`).
fn looks_like_file(q: &str) -> bool {
    if q.contains(['[', ']']) {
        return false;
    }
    let file = q.rsplit(['\\', '/']).next().unwrap_or(q);
    q.len() != file.len()
        || file.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && !ext.is_empty() && ext.chars().all(|c| c.is_ascii_alphanumeric()))
}
