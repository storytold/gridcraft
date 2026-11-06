//! OpenFormula → Excel formula-text translation for ODS import.
//!
//! An ODS `table:formula` attribute holds OpenFormula text (ODF part 2), namespaced `of:`
//! (OASIS), `oooc:` (OpenOffice/LibreOffice) or — in Excel-authored content — `msoxl:`, which
//! is Excel syntax already. The first two dialects differ from Excel text mechanically (`;`
//! argument separators, bracketed relative references like `[.A1]` and `['Sheet 2'.$A1]`,
//! `COM.MICROSOFT.` prefixes on newer functions) and in ways that have no exact Excel spelling
//! (union `~` and intersection `!` operators, error literals, external-document references,
//! foreign add-in namespaces). A wrong guess would silently change what a formula computes, so
//! [`translate`] converts only what maps exactly and returns `None` for the rest: the caller
//! keeps the cell's cached constant rather than a mistranslation. A final
//! `gridcraft_formula::parse` gate makes every `Some` result a valid model formula.

use gridcraft_core::{MAX_COLS, MAX_ROWS};

/// Translates an ODS `table:formula` attribute value into Excel/model formula body text (no
/// leading `=`), or `None` when any construct has no exact Excel equivalent.
pub(crate) fn translate(text: &str, known: &dyn Fn(&str) -> bool) -> Option<String> {
    let (namespace, body) = split_prefix(text)?;
    if namespace == Some("msoxl") {
        return validated(body.to_string(), known);
    }
    validated(scan(body)?, known)
}

/// Splits an `of:=`, `oooc:=`, `msoxl:=` or bare `=` prefix into (namespace, body).
fn split_prefix(text: &str) -> Option<(Option<&'static str>, &str)> {
    for (ns, prefix) in [("of", "of:"), ("oooc", "oooc:"), ("msoxl", "msoxl:")] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return rest.strip_prefix('=').map(|body| (Some(ns), body));
        }
    }
    text.strip_prefix('=').map(|body| (None, body))
}

/// Returns the text only if it parses as a model formula whose every call names a function the
/// evaluator knows (a `UNKNOWN()` that only parses would open as a live formula and recalculate
/// to `#NAME?`, where the importer's contract keeps the source's cached constant for anything it
/// cannot guarantee to reproduce). The evaluator's complete contract, its special functions
/// included, decides — `gridcraft_functions::lookup` alone would reject the specials.
fn validated(text: String, known: &dyn Fn(&str) -> bool) -> Option<String> {
    // A call the caller's contract does not know cannot be guaranteed to recalculate to the
    // saved result, so the importer keeps the cache instead (its caller warns).
    let mut all_known = true;
    gridcraft_formula::parse(&text).ok()?.map(&mut |e| {
        if let gridcraft_formula::Expr::Call(n, _) = &e
            && !known(n)
        {
            all_known = false;
        }
        e
    });
    all_known.then_some(text)
}

/// Scans OpenFormula text in one pass, copying everything that is Excel syntax already and
/// translating what differs. Bytes ≥ 0x80 outside strings and sheet names are not part of the
/// grammar and bail.
fn scan(body: &str) -> Option<String> {
    let bytes = body.as_bytes();
    let mut out = String::with_capacity(body.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        i = match b {
            b'"' => string_at(body, i, &mut out)?,
            b'[' => reference_at(body, i, &mut out)?,
            b';' => {
                out.push(',');
                i + 1
            }
            // Union, intersection and error literals have no safe Excel spelling.
            b'~' | b'!' | b'#' => return None,
            b if is_word(b) => word_at(body, i, &mut out)?,
            b if b < 0x80 => {
                out.push(char::from(b));
                i + 1
            }
            _ => return None,
        };
    }
    Some(out)
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.'
}

/// Copies a maximal `[A-Za-z0-9_.]` run. A digit-initial run is a number literal (copied
/// verbatim; exponents like `1.5E3` stay one run); anything else is an identifier, which may
/// contain `.` only as the `COM.MICROSOFT.` prefix. A dot-initial run is not valid OpenFormula.
fn word_at(body: &str, start: usize, out: &mut String) -> Option<usize> {
    let bytes = body.as_bytes();
    let end = run_end(bytes, start, is_word);
    let word = body.get(start..end)?;
    let first = bytes.get(start).copied()?;
    if first.is_ascii_digit() {
        out.push_str(word);
        return Some(end);
    }
    if first == b'.' {
        return None;
    }
    let name = if let Some(rest) = strip_ms_prefix(word) {
        if rest.is_empty() || rest.contains('.') {
            return None;
        }
        rest
    } else if word.contains('.') {
        return None;
    } else {
        word
    };
    out.push_str(name);
    Some(end)
}

const MS_PREFIX: &str = "COM.MICROSOFT.";

/// Case-insensitively strips the `COM.MICROSOFT.` add-in prefix.
fn strip_ms_prefix(word: &str) -> Option<&str> {
    match word.get(..MS_PREFIX.len()) {
        Some(head) if head.eq_ignore_ascii_case(MS_PREFIX) => word.get(MS_PREFIX.len()..),
        _ => None,
    }
}

/// Copies a `"…"` string literal (`""` escapes) verbatim; `None` when unterminated.
fn string_at(body: &str, start: usize, out: &mut String) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut p = start + 1;
    loop {
        match bytes.get(p) {
            None => return None,
            Some(&b'"') => match bytes.get(p + 1) {
                Some(&b'"') => p += 2,
                _ => {
                    out.push_str(body.get(start..=p)?);
                    return Some(p + 1);
                }
            },
            Some(_) => p += 1,
        }
    }
}

/// Translates one `[…]` reference into `Sheet!A1:B2` form: an optional leading `$`
/// (sheet-absolute; Excel has no such marker, so it is dropped), an optional sheet part (a
/// `'…'`-quoted name with `''` escapes, or a bare `[A-Za-z0-9_]+` word) followed by `.`, or a
/// lone `.` for the same sheet; then `$?COL$?ROW` (1–3 letters, 1–7 digits, bounded to the
/// worksheet), optionally `:` and a second address (a leading `.` is allowed after the
/// colon). Anything else — a `#`
/// external-document part, column- or row-only addresses, a sheet on the second address —
/// bails.
fn reference_at(body: &str, start: usize, out: &mut String) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut p = start + 1;
    if bytes.get(p) == Some(&b'$') {
        p += 1;
    }
    let sheet = if bytes.get(p) == Some(&b'\'') {
        let (name, q) = quoted_sheet_at(body, p)?;
        if name.is_empty() || bytes.get(q) != Some(&b'.') {
            return None;
        }
        p = q + 1;
        Some(name)
    } else {
        let end = run_end(bytes, p, |b| b.is_ascii_alphanumeric() || b == b'_');
        if end == p || bytes.get(end) != Some(&b'.') {
            None
        } else {
            let name = body.get(p..end)?.to_string();
            p = end + 1;
            Some(name)
        }
    };
    if sheet.is_none() && bytes.get(p) != Some(&b'.') {
        return None; // same-sheet marker
    }
    if sheet.is_none() {
        p += 1;
    }
    let (first, q) = cell_at(body, p)?;
    let mut addr = first;
    p = q;
    if bytes.get(p) == Some(&b':') {
        p += 1;
        if bytes.get(p) == Some(&b'.') {
            p += 1;
        }
        let (second, q) = cell_at(body, p)?;
        addr.push(':');
        addr.push_str(&second);
        p = q;
    }
    if bytes.get(p) != Some(&b']') {
        return None;
    }
    if let Some(name) = &sheet {
        out.push_str(&excel_sheet(name));
        out.push('!');
    }
    out.push_str(&addr);
    Some(p + 1)
}

/// Parses a `'…'` sheet name (`''` escapes) at `start`; returns the unescaped name and the
/// position after the closing quote.
fn quoted_sheet_at(body: &str, start: usize) -> Option<(String, usize)> {
    let bytes = body.as_bytes();
    let mut name = String::new();
    let mut p = start + 1;
    loop {
        match bytes.get(p) {
            None => return None, // unterminated
            Some(&b'\'') => match bytes.get(p + 1) {
                Some(&b'\'') => {
                    name.push('\'');
                    p += 2;
                }
                _ => return Some((name, p + 1)),
            },
            Some(&b) if b < 0x80 => {
                name.push(char::from(b));
                p += 1;
            }
            Some(_) => {
                let ch = body.get(p..)?.chars().next()?;
                name.push(ch);
                p += ch.len_utf8();
            }
        }
    }
}

/// Parses `$?COL$?ROW` (1–3 letters, 1–7 digits) at `p`; returns the Excel spelling and the
/// position after it. The address must lie inside the worksheet (`MAX_COLS` columns by
/// `MAX_ROWS` rows): beyond it the model parser reads the word as a defined name, so the
/// translator bails and the caller keeps the cell's cached constant.
fn cell_at(body: &str, p: usize) -> Option<(String, usize)> {
    let bytes = body.as_bytes();
    let mut s = String::new();
    let mut q = p;
    if bytes.get(q) == Some(&b'$') {
        s.push('$');
        q += 1;
    }
    let col_end = run_end(bytes, q, |b| b.is_ascii_alphabetic());
    if !(1..=3).contains(&(col_end - q)) {
        return None;
    }
    // Letters are alphabetic, so each folds into 1..=26 and cannot overflow.
    let col = (q..col_end).try_fold(0u32, |n, i| {
        let letter = u32::from(bytes.get(i).copied()?.to_ascii_uppercase() - b'A' + 1);
        n.checked_mul(26)?.checked_add(letter)
    })?;
    if col > MAX_COLS {
        return None;
    }
    s.push_str(body.get(q..col_end)?);
    q = col_end;
    if bytes.get(q) == Some(&b'$') {
        s.push('$');
        q += 1;
    }
    let row_end = run_end(bytes, q, |b| b.is_ascii_digit());
    if !(1..=7).contains(&(row_end - q)) {
        return None;
    }
    let row = (q..row_end).try_fold(0u32, |n, i| {
        let digit = u32::from(bytes.get(i).copied()? - b'0');
        n.checked_mul(10)?.checked_add(digit)
    })?;
    if row > MAX_ROWS {
        return None;
    }
    s.push_str(body.get(q..row_end)?);
    Some((s, row_end))
}

/// Re-quotes a sheet name for Excel: a plain `[A-Za-z_][A-Za-z0-9_]*` word stays bare, anything
/// else is wrapped in `'…'` with `''` escapes.
fn excel_sheet(name: &str) -> String {
    let plain =
        name.chars().enumerate().all(|(i, c)| if i == 0 { c.is_ascii_alphabetic() || c == '_' } else { c.is_ascii_alphanumeric() || c == '_' });
    if plain { name.to_string() } else { format!("'{}'", name.replace('\'', "''")) }
}

/// Position after the maximal run of bytes matching `f`, starting at `from`.
fn run_end(bytes: &[u8], from: usize, f: impl Fn(u8) -> bool) -> usize {
    let mut p = from;
    while matches!(bytes.get(p), Some(&b) if f(b)) {
        p += 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_and_passthrough() {
        assert_eq!(translate("of:=SUM(A1:A9)", &|_| true), Some("SUM(A1:A9)".to_string()));
        assert_eq!(translate("oooc:=SUM(A1:A9)", &|_| true), Some("SUM(A1:A9)".to_string()));
        assert_eq!(translate("msoxl:=SUM(A1:A9)", &|_| true), Some("SUM(A1:A9)".to_string()));
        assert_eq!(translate("=SUM(A1:A9)", &|_| true), Some("SUM(A1:A9)".to_string()));
        assert_eq!(translate("of:=1", &|_| true), Some("1".to_string()));
        // The namespace without the `=` is not a formula prefix.
        assert_eq!(translate("of:1", &|_| true), None);
    }

    #[test]
    fn argument_separator_respects_strings() {
        assert_eq!(translate("of:=IF(A1>1;2;3)", &|_| true), Some("IF(A1>1,2,3)".to_string()));
        assert_eq!(translate("of:=CONCAT(\"a;b\";\"c\")", &|_| true), Some("CONCAT(\"a;b\",\"c\")".to_string()));
        assert_eq!(translate("of:=SUM(1.5;2)", &|_| true), Some("SUM(1.5,2)".to_string()));
    }

    #[test]
    fn bracketed_references() {
        assert_eq!(translate("of:=[.A1]", &|_| true), Some("A1".to_string()));
        assert_eq!(translate("of:=[.$A$1:.B2]", &|_| true), Some("$A$1:B2".to_string()));
        assert_eq!(translate("of:=['Sheet 2'.A1]", &|_| true), Some("'Sheet 2'!A1".to_string()));
        assert_eq!(translate("of:=[$S.$A1]", &|_| true), Some("S!$A1".to_string()));
        assert_eq!(translate("of:=['O''Brien.s'.A1]", &|_| true), Some("'O''Brien.s'!A1".to_string()));
        assert_eq!(translate("of:=['a#b'.A1]", &|_| true), Some("'a#b'!A1".to_string()));
        assert_eq!(translate("of:=[$.A1]", &|_| true), Some("A1".to_string()));
    }

    #[test]
    fn microsoft_function_names() {
        assert_eq!(translate("of:=COM.MICROSOFT.XLOOKUP(1;A1;B1)", &|_| true), Some("XLOOKUP(1,A1,B1)".to_string()));
        assert_eq!(translate("of:=com.microsoft.XLOOKUP(1;A1;B1)", &|_| true), Some("XLOOKUP(1,A1,B1)".to_string()));
    }

    #[test]
    fn bail_on_untranslatable() {
        assert_eq!(translate("of:=ORG.OPENOFFICE.FOO()", &|_| true), None);
        assert_eq!(translate("of:=com.sun.star.sheet.addin.X()", &|_| true), None);
        assert_eq!(translate("of:=[.A:.A]", &|_| true), None); // column-only range
        assert_eq!(translate("of:=[.A1]~[.B1]", &|_| true), None); // union operator
        assert_eq!(translate("of:=[.A1]![.B1]", &|_| true), None); // intersection operator
        assert_eq!(translate("of:=#REF!+1", &|_| true), None); // error literal
        assert_eq!(translate("of:=['file:///x'#S.A1]", &|_| true), None); // external document ref
        assert_eq!(translate("of:=\"unterminated", &|_| true), None); // unterminated string
        assert_eq!(translate("rq:=A1", &|_| true), None); // no known prefix
        assert_eq!(translate("of:=TRUE()", &|_| true), Some("TRUE()".to_string())); // must NOT bail
    }

    /// 1-based column number → letters (`1 → A`, `MAX_COLS → XFD`), so boundary cases are
    /// built from the worksheet constants instead of magic numbers.
    fn col_letters(mut n: u32) -> String {
        let mut s = String::new();
        while n > 0 {
            n -= 1;
            s.insert(0, char::from(b'A' + (n % 26) as u8));
            n /= 26;
        }
        s
    }

    /// An address beyond the worksheet must bail, not leak through the parse gate as a defined
    /// name (`#NAME?` after recalc); the caller keeps the cached constant instead.
    #[test]
    fn worksheet_bounds_bail_instead_of_becoming_names() {
        assert_eq!(translate("of:=[.ZZZ1]", &|_| true), None); // past the last column
        assert_eq!(translate(&format!("of:=[.{}1]", col_letters(MAX_COLS + 1)), &|_| true), None);
        assert_eq!(translate(&format!("of:=[.A{}]", MAX_ROWS + 1), &|_| true), None);
        assert_eq!(translate(&format!("of:=[.A1:.{}1]", col_letters(MAX_COLS + 1)), &|_| true), None); // range second endpoint
        // The last real cell stays translatable.
        let corner = format!("{}{}", col_letters(MAX_COLS), MAX_ROWS);
        assert_eq!(translate(&format!("of:=[.{corner}]"), &|_| true), Some(corner));
    }

    #[test]
    fn parse_gate() {
        assert_eq!(translate("of:=SUM(", &|_| true), None);
    }
}
