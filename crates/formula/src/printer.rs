//! [`Expr`] → formula text (without `=`), A1 or R1C1, in the canonical dialect or a local one.
//!
//! A local dialect spells numbers with the region's decimal separator, separates arguments with
//! its list separator, writes array constants with its column and row separators and uses the
//! language's function names, booleans, errors, structured-reference items and R1C1 letters.
//! Defined names, LET/LAMBDA parameters, table and sheet names and quoted text are never
//! translated.

use std::borrow::Cow;
use std::fmt::Write;

use gridcraft_core::{CellRef, col_to_letters, number_to_text};
use gridcraft_locale::{Dialect, Language, TABLE_ITEM_ALL, TABLE_ITEM_DATA, TABLE_ITEM_HEADERS, TABLE_ITEM_THIS_ROW, TABLE_ITEM_TOTALS};

use crate::ast::*;
use crate::parser::{FILE_PREFIXES, KnownNames, UDF_PREFIX, bindable_bool, binds_after, is_bound, no_names};

/// Whether a sheet name needs quotes in a reference.
pub fn needs_quotes(name: &str) -> bool {
    needs_quotes_in(name, Dialect::INVARIANT.names)
}

/// Whether a sheet name needs quotes in a reference written in the language `names`.
fn needs_quotes_in(name: &str, names: &Language) -> bool {
    if name.is_empty() {
        return true;
    }
    let first = name.chars().next().unwrap_or('_');
    if first.is_ascii_digit() || name.chars().any(|c| !(c.is_alphanumeric() || c == '_' || c == '.')) {
        return true;
    }
    // Looks like a cell ref (`A1`, `R1C1`) or a boolean.
    gridcraft_core::CellRef::parse(name).is_some()
        || name.eq_ignore_ascii_case("TRUE")
        || name.eq_ignore_ascii_case("FALSE")
        || names.parse_bool(name).is_some()
}

pub fn quote_sheet(name: &str) -> String {
    quote_sheet_in(name, Dialect::INVARIANT.names)
}

fn quote_sheet_in(name: &str, names: &Language) -> String {
    if needs_quotes_in(name, names) { format!("'{}'", name.replace('\'', "''")) } else { name.to_string() }
}

fn sheet_prefix(s: &SheetSel, names: &Language) -> String {
    match s {
        SheetSel::Current => String::new(),
        SheetSel::Named(n) => format!("{}!", quote_sheet_in(n, names)),
        SheetSel::Span(a, b) => {
            let joined = format!("{a}:{b}");
            if needs_quotes_in(a, names) || needs_quotes_in(b, names) { format!("'{}'!", joined.replace('\'', "''")) } else { format!("{joined}!") }
        }
    }
}

fn anchor_a1(a: &Anchor) -> String {
    format!("{}{}{}{}", if a.col_abs { "$" } else { "" }, col_to_letters(a.col), if a.row_abs { "$" } else { "" }, a.row as u64 + 1)
}

pub fn reference_a1(r: &Reference) -> String {
    reference_a1_in(r, Dialect::INVARIANT.names)
}

fn reference_a1_in(r: &Reference, names: &Language) -> String {
    let body = match &r.kind {
        RefKind::Cell(a) => anchor_a1(a),
        RefKind::Range(a, b) => format!("{}:{}", anchor_a1(a), anchor_a1(b)),
        RefKind::Rows(r0, a0, r1, a1) => format!("{}{}:{}{}", if *a0 { "$" } else { "" }, r0 + 1, if *a1 { "$" } else { "" }, r1 + 1),
        RefKind::Cols(c0, a0, c1, a1) => {
            format!("{}{}:{}{}", if *a0 { "$" } else { "" }, col_to_letters(*c0), if *a1 { "$" } else { "" }, col_to_letters(*c1))
        }
    };
    format!("{}{}", sheet_prefix(&r.sheet, names), body)
}

fn r1c1_part(letter: char, v: u32, abs: bool, base: u32) -> String {
    if abs {
        format!("{letter}{}", v + 1)
    } else {
        let d = v as i64 - base as i64;
        if d == 0 { letter.to_string() } else { format!("{letter}[{d}]") }
    }
}

fn anchor_r1c1(a: &Anchor, at: CellRef, [row, col]: [char; 2]) -> String {
    format!("{}{}", r1c1_part(row, a.row, a.row_abs, at.row), r1c1_part(col, a.col, a.col_abs, at.col))
}

pub fn reference_r1c1(r: &Reference, at: CellRef) -> String {
    reference_r1c1_in(r, at, Dialect::INVARIANT.names)
}

fn reference_r1c1_in(r: &Reference, at: CellRef, names: &Language) -> String {
    let letters = names.r1c1;
    let [row, col] = letters;
    let body = match &r.kind {
        RefKind::Cell(a) => anchor_r1c1(a, at, letters),
        RefKind::Range(a, b) => format!("{}:{}", anchor_r1c1(a, at, letters), anchor_r1c1(b, at, letters)),
        RefKind::Rows(r0, a0, r1, a1) => {
            let (x, y) = (r1c1_part(row, *r0, *a0, at.row), r1c1_part(row, *r1, *a1, at.row));
            if x == y { x } else { format!("{x}:{y}") }
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            let (x, y) = (r1c1_part(col, *c0, *a0, at.col), r1c1_part(col, *c1, *a1, at.col));
            if x == y { x } else { format!("{x}:{y}") }
        }
    };
    format!("{}{}", sheet_prefix(&r.sheet, names), body)
}

/// A structured reference in dialect `d`: items from the language, parts joined with the region's
/// list separator.
pub fn struct_ref(s: &StructRef, d: &Dialect) -> String {
    let esc = |c: &str| c.replace('\'', "''").replace('[', "'[").replace(']', "']").replace('#', "'#");
    let item = |i: &StructItem| {
        let idx = match i {
            StructItem::All => TABLE_ITEM_ALL,
            StructItem::Data => TABLE_ITEM_DATA,
            StructItem::Headers => TABLE_ITEM_HEADERS,
            StructItem::Totals => TABLE_ITEM_TOTALS,
            StructItem::ThisRow => TABLE_ITEM_THIS_ROW,
        };
        d.names.table_items.get(idx).copied().unwrap_or("")
    };
    let sep = d.regional.list;
    let this_row = s.specifiers == [StructItem::ThisRow];
    let mut out = s.table.clone();
    let simple_col = |c: &str| !c.chars().any(|ch| matches!(ch, ' ' | '[' | ']' | '#' | '\'' | ',' | ':') || ch == sep);
    match (&s.col_start, &s.col_end) {
        (None, None) if s.specifiers.is_empty() => out.push_str("[]"),
        (None, None) if this_row => out.push_str("[@]"),
        (None, None) if s.specifiers.len() == 1 => {
            let _ = write!(out, "[{}]", s.specifiers.first().map(item).unwrap_or(""));
        }
        (Some(c), None) if s.specifiers.is_empty() => {
            let _ = write!(out, "[{}]", esc(c));
        }
        (Some(c), None) if this_row => {
            if simple_col(c) {
                let _ = write!(out, "[@{}]", esc(c));
            } else {
                let _ = write!(out, "[@[{}]]", esc(c));
            }
        }
        _ => {
            let mut parts: Vec<String> = s.specifiers.iter().map(|i| format!("[{}]", item(i))).collect();
            match (&s.col_start, &s.col_end) {
                (Some(a), Some(b)) => parts.push(format!("[{}]:[{}]", esc(a), esc(b))),
                (Some(a), None) => parts.push(format!("[{}]", esc(a))),
                _ => {}
            }
            let _ = write!(out, "[{}]", parts.join(&sep.to_string()));
        }
    }
    out
}

/// A number with the region's decimal separator.
fn number(n: f64, decimal: char) -> String {
    let t = number_to_text(n);
    if decimal == '.' { t } else { t.replace('.', &decimal.to_string()) }
}

/// The spelling of a function name in language `names`:
/// - a canonical built-in prints its local name;
/// - file-prefixed names (`_XLFN.FORECAST.ETS`) print the prefix in lower case;
/// - `_XLUDF.X` prints as plain `X` when `X` is a built-in this language does not know (so that
///   re-reading it in the same dialect gives back `_XLUDF.X`), and as `_xludf.X` otherwise.
fn call_name<'a>(name: &'a str, names: &Language) -> Cow<'a, str> {
    if let Some(local) = names.local_function(name) {
        return Cow::Borrowed(local);
    }
    let strip = |p: &str| name.get(..p.len()).filter(|h| h.eq_ignore_ascii_case(p)).and_then(|_| name.get(p.len()..));
    if let Some(rest) = strip(UDF_PREFIX) {
        if crate::catalog::is_builtin(rest) && names.canonical_function(rest).is_none() {
            return Cow::Borrowed(rest);
        }
        return Cow::Owned(format!("{}{rest}", UDF_PREFIX.to_ascii_lowercase()));
    }
    for p in FILE_PREFIXES {
        if let Some(rest) = strip(p) {
            return Cow::Owned(format!("{}{rest}", p.to_ascii_lowercase()));
        }
    }
    Cow::Borrowed(name)
}

/// Prints an expression in canonical A1 notation.
pub fn print(e: &Expr) -> String {
    print_local(e, &Dialect::INVARIANT)
}

/// Prints an expression in A1 notation written in dialect `d`.
pub fn print_local(e: &Expr, d: &Dialect) -> String {
    print_local_with(e, d, &no_names)
}

/// [`print_local`] for text read back with [`crate::parse_local_with`] and the same `known`
/// names: where a local function name or boolean is also a name in scope, the built-in is
/// written so that it still reads as the built-in (`_xlfn.SUM(…)`, `_xlfn.FALSE()`).
pub fn print_local_with(e: &Expr, d: &Dialect, known: KnownNames<'_>) -> String {
    let mut s = String::new();
    Printer { d, r1c1: None, known, bindings: Vec::new() }.expr(&mut s, e);
    s
}

/// Prints an expression in canonical R1C1 notation relative to `at`.
pub fn print_r1c1(e: &Expr, at: CellRef) -> String {
    print_r1c1_local(e, at, &Dialect::INVARIANT)
}

/// Prints an expression in R1C1 notation relative to `at`, written in dialect `d` (`L`/`C` in
/// Portuguese).
pub fn print_r1c1_local(e: &Expr, at: CellRef, d: &Dialect) -> String {
    let mut s = String::new();
    Printer { d, r1c1: Some(at), known: &no_names, bindings: Vec::new() }.expr(&mut s, e);
    s
}

struct Printer<'a> {
    d: &'a Dialect<'a>,
    r1c1: Option<CellRef>,
    known: KnownNames<'a>,
    /// Names bound by the LET/LAMBDA calls being written, innermost last.
    bindings: Vec<String>,
}

impl Printer<'_> {
    fn bound(&self, name: &str) -> bool {
        is_bound(name, &self.bindings, self.known)
    }

    /// The spelling of a call to `name`. When that spelling is also a name in scope the parser
    /// would call the name, so a built-in is written with its file prefix instead.
    fn call_name<'n>(&self, name: &'n str) -> Cow<'n, str> {
        let local = call_name(name, self.d.names);
        if !self.bound(&local) || local.to_ascii_uppercase() == name {
            return local;
        }
        if crate::catalog::is_builtin(name) { Cow::Owned(format!("_xlfn.{name}")) } else { call_name(name, Dialect::INVARIANT.names) }
    }

    fn args(&mut self, s: &mut String, function: &str, args: &[Expr]) {
        s.push('(');
        let saved = self.bindings.len();
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                s.push(self.d.regional.list);
            }
            self.expr(s, a);
            if let Some(n) = binds_after(function, args, i, i + 1 < args.len()) {
                self.bindings.push(n.to_string());
            }
        }
        self.bindings.truncate(saved);
        s.push(')');
    }

    /// Writes `e`. Left-nested chains (`1+1+…`, `A1%%%`, `f(1)(2)…`) can be arbitrarily deep, so
    /// the left spine is written with a loop; only right operands and arguments recurse, and
    /// those are bounded by the parser's nesting limit.
    fn expr(&mut self, s: &mut String, e: &Expr) {
        enum After<'a> {
            Binary(BinOp, &'a Expr),
            Percent,
            Invoke(&'a [Expr]),
        }
        let mut after: Vec<After<'_>> = Vec::new();
        let mut cur = e;
        loop {
            match cur {
                Expr::Binary(op, a, b) => {
                    after.push(After::Binary(*op, b));
                    cur = a;
                }
                Expr::Unary(UnOp::Percent, x) => {
                    after.push(After::Percent);
                    cur = x;
                }
                Expr::Invoke(c, args) => {
                    after.push(After::Invoke(args));
                    cur = c;
                }
                other => {
                    self.node(s, other);
                    break;
                }
            }
        }
        while let Some(a) = after.pop() {
            match a {
                After::Binary(op, b) => {
                    match op {
                        BinOp::Union => s.push(self.d.regional.list),
                        other => s.push_str(other.symbol()),
                    }
                    self.expr(s, b);
                }
                After::Percent => s.push('%'),
                After::Invoke(args) => self.args(s, "", args),
            }
        }
    }

    /// Writes a node that is not part of a left spine (its children are written by [`Self::expr`]).
    fn node(&mut self, s: &mut String, e: &Expr) {
        let d = self.d;
        match e {
            Expr::Number(n) => s.push_str(&number(*n, d.regional.decimal)),
            Expr::Text(t) => {
                s.push('"');
                s.push_str(&t.replace('"', "\"\""));
                s.push('"');
            }
            Expr::Bool(b) => {
                let local = d.names.bool_text(*b);
                if bindable_bool(local) && self.bound(local) {
                    // The TRUE() and FALSE() functions return the same value.
                    s.push_str(if *b { "_xlfn.TRUE()" } else { "_xlfn.FALSE()" });
                } else {
                    s.push_str(local);
                }
            }
            Expr::Error(err) => s.push_str(d.names.local_error(err.as_str())),
            Expr::Array(rows) => {
                s.push('{');
                for (ri, row) in rows.iter().enumerate() {
                    if ri > 0 {
                        s.push(d.regional.array_row);
                    }
                    for (ci, el) in row.iter().enumerate() {
                        if ci > 0 {
                            s.push(d.regional.array_col);
                        }
                        // An array constant holds no names: its booleans are always booleans.
                        match el {
                            Expr::Bool(b) => s.push_str(d.names.bool_text(*b)),
                            other => self.expr(s, other),
                        }
                    }
                }
                s.push('}');
            }
            Expr::Ref(r) => match self.r1c1 {
                Some(at) => s.push_str(&reference_r1c1_in(r, at, d.names)),
                None => s.push_str(&reference_a1_in(r, d.names)),
            },
            Expr::Name(n) => s.push_str(n),
            Expr::Struct(st) => s.push_str(&struct_ref(st, d)),
            Expr::Unary(op, x) => match op {
                UnOp::Neg => {
                    s.push('-');
                    self.expr(s, x);
                }
                UnOp::Plus => {
                    s.push('+');
                    self.expr(s, x);
                }
                UnOp::At => {
                    s.push('@');
                    self.expr(s, x);
                }
                UnOp::Percent => self.expr(s, e),
                UnOp::Spill => {
                    self.expr(s, x);
                    // A reference that became #REF! (its sheet or cell was deleted) stays `#REF!`.
                    if !matches!(**x, Expr::Error(_)) {
                        s.push('#');
                    }
                }
            },
            Expr::Binary(..) | Expr::Invoke(..) => self.expr(s, e),
            Expr::Call(name, args) => {
                s.push_str(&self.call_name(name));
                self.args(s, name, args);
            }
            Expr::Missing => {}
            Expr::Paren(x) => {
                s.push('(');
                self.expr(s, x);
                s.push(')');
            }
        }
    }
}
