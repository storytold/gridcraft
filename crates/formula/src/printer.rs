//! [`Expr`] → formula text (without `=`), A1 or R1C1.

use std::fmt::Write;

use gridcraft_core::{CellRef, number_to_text};

use crate::ast::{Anchor, BinOp, Expr, RefKind, Reference, SheetSel, StructItem, StructRef, UnOp};

pub use gridcraft_core::{quote_sheet, sheet_needs_quotes as needs_quotes};

fn sheet_prefix(s: &SheetSel) -> String {
    match s {
        SheetSel::Current => String::new(),
        SheetSel::Named(n) => format!("{}!", quote_sheet(n)),
        SheetSel::Span(a, b) => {
            let joined = format!("{a}:{b}");
            if needs_quotes(a) || needs_quotes(b) { format!("'{}'!", joined.replace('\'', "''")) } else { format!("{joined}!") }
        }
    }
}

fn push_anchor_a1(s: &mut String, a: &Anchor) {
    if a.col_abs {
        s.push('$');
    }
    push_col(s, a.col);
    if a.row_abs {
        s.push('$');
    }
    let _ = write!(s, "{}", a.row as u64 + 1);
}

/// Appends [`gridcraft_core::col_to_letters`]`(col)` without allocating.
fn push_col(s: &mut String, col: u32) {
    let mut letters = [0u8; 8];
    let mut i = letters.len();
    let mut n = col as u64 + 1;
    while n > 0 && i > 0 {
        i -= 1;
        if let Some(l) = letters.get_mut(i) {
            *l = b'A' + ((n - 1) % 26) as u8;
        }
        n = (n - 1) / 26;
    }
    for &l in letters.get(i..).unwrap_or_default() {
        s.push(char::from(l));
    }
}

pub fn reference_a1(r: &Reference) -> String {
    range_a1(r, ":")
}

/// A1 text with `sep` between the two ends of a range (`:` or a trim operator).
fn range_a1(r: &Reference, sep: &str) -> String {
    let mut s = String::new();
    push_range_a1(&mut s, r, sep, (0, 0));
    s
}

/// Appends [`range_a1`]`(r, sep)` to `s`, shifting relative coordinates by `(dr, dc)`.
fn push_range_a1(s: &mut String, r: &Reference, sep: &str, shift: (i64, i64)) {
    let kind = if shift == (0, 0) {
        r.kind.clone()
    } else {
        match crate::adjust::shift_ref_kind(&r.kind, shift.0, shift.1) {
            Some(k) => k,
            None => {
                s.push_str("#REF!");
                return;
            }
        }
    };
    match &r.sheet {
        SheetSel::Current => {}
        SheetSel::Named(n) => {
            s.push_str(&quote_sheet(n));
            s.push('!');
        }
        sheet @ SheetSel::Span(_, _) => s.push_str(&sheet_prefix(sheet)),
    }
    let dollar = |abs: bool| if abs { "$" } else { "" };
    match &kind {
        RefKind::Cell(a) => push_anchor_a1(s, a),
        RefKind::Range(a, b) => {
            push_anchor_a1(s, a);
            s.push_str(sep);
            push_anchor_a1(s, b);
        }
        RefKind::Rows(r0, a0, r1, a1) => {
            let _ = write!(s, "{}{}{sep}{}{}", dollar(*a0), r0 + 1, dollar(*a1), r1 + 1);
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            s.push_str(dollar(*a0));
            push_col(s, *c0);
            s.push_str(sep);
            s.push_str(dollar(*a1));
            push_col(s, *c1);
        }
    }
}

fn r1c1_part(letter: char, v: u32, abs: bool, base: u32) -> String {
    if abs {
        format!("{letter}{}", v + 1)
    } else {
        let d = v as i64 - base as i64;
        if d == 0 { letter.to_string() } else { format!("{letter}[{d}]") }
    }
}

fn anchor_r1c1(a: &Anchor, at: CellRef) -> String {
    format!("{}{}", r1c1_part('R', a.row, a.row_abs, at.row), r1c1_part('C', a.col, a.col_abs, at.col))
}

pub fn reference_r1c1(r: &Reference, at: CellRef) -> String {
    range_r1c1(r, at, ":")
}

/// R1C1 text with `sep` between the two ends of a range; a trim operator never collapses
/// `R1:R1` to `R1`.
fn range_r1c1(r: &Reference, at: CellRef, sep: &str) -> String {
    let body = match &r.kind {
        RefKind::Cell(a) => anchor_r1c1(a, at),
        RefKind::Range(a, b) => format!("{}{sep}{}", anchor_r1c1(a, at), anchor_r1c1(b, at)),
        RefKind::Rows(r0, a0, r1, a1) => {
            let (x, y) = (r1c1_part('R', *r0, *a0, at.row), r1c1_part('R', *r1, *a1, at.row));
            if x == y && sep == ":" { x } else { format!("{x}{sep}{y}") }
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            let (x, y) = (r1c1_part('C', *c0, *a0, at.col), r1c1_part('C', *c1, *a1, at.col));
            if x == y && sep == ":" { x } else { format!("{x}{sep}{y}") }
        }
    };
    format!("{}{}", sheet_prefix(&r.sheet), body)
}

pub fn struct_ref(s: &StructRef) -> String {
    write_struct_ref(s, true)
}

/// Structured reference as files spell it: `[@Col]` is only Excel's display shorthand, files
/// hold `[[#This Row],[Col]]` (and `[#This Row]` for `[@]`).
pub fn struct_ref_file(s: &StructRef) -> String {
    write_struct_ref(s, false)
}

/// Splits a sheet-qualified name at its last `!` outside brackets. A `!` inside brackets
/// belongs to a structured reference's column (`Table1[Done!]`, `Table1[#REF!]`).
fn sheet_split(n: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut escaped = false;
    let mut at = None;
    for (i, c) in n.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\'' if depth > 0 => escaped = true,
            '[' => depth = depth.saturating_add(1),
            ']' => depth = depth.saturating_sub(1),
            '!' if depth == 0 => at = Some(i),
            _ => {}
        }
    }
    at.and_then(|i| Some((n.get(..i)?, n.get(i + 1..)?)))
}

fn write_struct_ref(s: &StructRef, at_shorthand: bool) -> String {
    let esc = |c: &str| c.replace('\'', "''").replace('[', "'[").replace(']', "']").replace('#', "'#");
    let item = |i: &StructItem| match i {
        StructItem::All => "#All",
        StructItem::Data => "#Data",
        StructItem::Headers => "#Headers",
        StructItem::Totals => "#Totals",
        StructItem::ThisRow => "#This Row",
    };
    let this_row = at_shorthand && s.specifiers == [StructItem::ThisRow];
    let mut out = s.table.clone();
    let simple_col = |c: &str| !c.chars().any(|ch| matches!(ch, ' ' | '[' | ']' | '#' | '\'' | ',' | ':'));
    match (&s.col_start, &s.col_end) {
        (None, None) if s.specifiers.is_empty() => out.push_str("[]"),
        (None, None) if this_row => out.push_str("[@]"),
        (None, None) if s.specifiers.len() == 1 => {
            let _ = write!(out, "[{}]", s.specifiers.first().map(item).unwrap_or(""));
        }
        (Some(c), None) if s.specifiers.is_empty() => {
            // Names with spaces at their edges, or starting with `@` (this-row shorthand), use the double-bracket form.
            if c.trim() == c && !c.starts_with('@') {
                let _ = write!(out, "[{}]", esc(c));
            } else {
                let _ = write!(out, "[[{}]]", esc(c));
            }
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
            let _ = write!(out, "[{}]", parts.join(","));
        }
    }
    out
}

fn number(n: f64) -> String {
    let text = number_to_text(n);
    // 15 significant digits can round past f64::MAX, which no longer parses.
    if text.parse::<f64>().is_ok_and(f64::is_finite) {
        return text;
    }
    let exact = format!("{:E}", n);
    match exact.split_once('E') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}E+{e}"),
        _ => exact,
    }
}

/// Prints an expression in A1 notation.
pub fn print(e: &Expr) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, None, (0, 0));
    s
}

/// Prints an expression in A1 notation with relative references shifted by `(dr, dc)` into `out`,
/// without cloning the AST. Produces identical output to `print(&shift_relative(e.clone(), dr, dc))`.
pub fn write_shifted_into(out: &mut String, e: &Expr, dr: i64, dc: i64) {
    write_expr(out, e, None, (dr, dc));
}

/// Prints an expression in A1 notation with relative references shifted by `(dr, dc)`,
/// without cloning the AST. Produces identical output to `print(&shift_relative(e.clone(), dr, dc))`.
pub fn print_shifted(e: &Expr, dr: i64, dc: i64) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, None, (dr, dc));
    s
}

/// Prints an expression in R1C1 notation relative to `at`.
pub fn print_r1c1(e: &Expr, at: CellRef) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Some(at), (0, 0));
    s
}

fn write_expr(s: &mut String, e: &Expr, r1c1: Option<CellRef>, shift: (i64, i64)) {
    match e {
        Expr::Number(n) => s.push_str(&number(*n)),
        Expr::Text(t) => {
            s.push('"');
            s.push_str(&t.replace('"', "\"\""));
            s.push('"');
        }
        Expr::Bool(b) => s.push_str(if *b { "TRUE" } else { "FALSE" }),
        Expr::Error(err) => s.push_str(err.as_str()),
        Expr::Array(rows) => {
            s.push('{');
            for (ri, row) in rows.iter().enumerate() {
                if ri > 0 {
                    s.push(';');
                }
                for (ci, el) in row.iter().enumerate() {
                    if ci > 0 {
                        s.push(',');
                    }
                    write_expr(s, el, r1c1, shift);
                }
            }
            s.push('}');
        }
        Expr::Ref(r) => match r1c1 {
            Some(at) => s.push_str(&reference_r1c1(r, at)),
            None => push_range_a1(s, r, ":", shift),
        },
        Expr::Name(n) => match sheet_split(n) {
            // A sheet-qualified name is stored with the bare sheet name (`My Sheet!Total`): quote it so it parses back.
            Some((sheet, name)) => {
                s.push_str(&quote_sheet(sheet));
                s.push('!');
                s.push_str(name);
            }
            _ => s.push_str(n),
        },
        Expr::Struct(st) => s.push_str(&struct_ref(st)),
        Expr::Unary(op, x) => match op {
            UnOp::Neg => {
                s.push('-');
                write_expr(s, x, r1c1, shift);
            }
            UnOp::Plus => {
                s.push('+');
                write_expr(s, x, r1c1, shift);
            }
            UnOp::At => {
                s.push('@');
                write_expr(s, x, r1c1, shift);
            }
            UnOp::Percent => {
                write_expr(s, x, r1c1, shift);
                s.push('%');
            }
            UnOp::Spill => {
                let shifted_err = matches!(&**x, Expr::Error(_))
                    || (shift != (0, 0) && matches!(&**x, Expr::Ref(r) if crate::adjust::shift_ref_kind(&r.kind, shift.0, shift.1).is_none()));
                write_expr(s, x, r1c1, shift);
                // A reference that became #REF! (its sheet or cell was deleted) stays `#REF!`.
                if !shifted_err {
                    s.push('#');
                }
            }
            UnOp::Trim(mode) => match &**x {
                Expr::Ref(r) if !matches!(r.kind, RefKind::Cell(_)) => match r1c1 {
                    Some(at) => s.push_str(&range_r1c1(r, at, mode.symbol())),
                    None => push_range_a1(s, r, mode.symbol(), shift),
                },
                Expr::Binary(BinOp::Range, a, b) => {
                    write_expr(s, a, r1c1, shift);
                    s.push_str(mode.symbol());
                    write_expr(s, b, r1c1, shift);
                }
                // A reference that became #REF! stays `#REF!`.
                Expr::Error(_) => write_expr(s, x, r1c1, shift),
                // No operator form (e.g. a range shrunk to one cell): the equivalent call.
                other => {
                    if shift != (0, 0) && matches!(other, Expr::Ref(r) if crate::adjust::shift_ref_kind(&r.kind, shift.0, shift.1).is_none()) {
                        s.push_str("#REF!");
                    } else {
                        s.push_str("TRIMRANGE(");
                        write_expr(s, other, r1c1, shift);
                        let _ = write!(s, ",{0},{0})", mode.code());
                    }
                }
            },
        },
        Expr::Binary(..) => {
            // Walk the left spine iteratively: a flat chain such as `1+1+1+...` parses into a
            // tree one level deeper per operator, and recursing on it could exhaust the stack.
            let mut rest: Vec<(&BinOp, &Expr)> = Vec::new();
            let mut first = e;
            while let Expr::Binary(op, a, b) = first {
                rest.push((op, b));
                first = a;
            }
            write_expr(s, first, r1c1, shift);
            for (op, right) in rest.into_iter().rev() {
                s.push_str(op.symbol());
                write_expr(s, right, r1c1, shift);
            }
        }
        Expr::Call(name, args) => {
            s.push_str(name);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, r1c1, shift);
            }
            s.push(')');
        }
        Expr::Invoke(c, args) => {
            write_expr(s, c, r1c1, shift);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, r1c1, shift);
            }
            s.push(')');
        }
        Expr::Missing => {}
        Expr::Paren(x) => {
            s.push('(');
            write_expr(s, x, r1c1, shift);
            s.push(')');
        }
    }
}
