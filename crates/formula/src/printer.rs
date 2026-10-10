//! [`Expr`] → formula text (without `=`), A1 or R1C1.

use std::fmt::Write;

use gridcraft_core::{CellRef, col_to_letters, number_to_text};

use crate::ast::*;

/// Whether a sheet name needs quotes in a reference.
pub fn needs_quotes(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    let first = name.chars().next().unwrap_or('_');
    if first.is_ascii_digit() || name.chars().any(|c| !(c.is_alphanumeric() || c == '_' || c == '.')) {
        return true;
    }
    // Looks like a cell ref (`A1`, `R1C1`) or a boolean.
    gridcraft_core::CellRef::parse(name).is_some() || name.eq_ignore_ascii_case("TRUE") || name.eq_ignore_ascii_case("FALSE")
}

pub fn quote_sheet(name: &str) -> String {
    if needs_quotes(name) { format!("'{}'", name.replace('\'', "''")) } else { name.to_string() }
}

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

fn anchor_a1(a: &Anchor) -> String {
    format!("{}{}{}{}", if a.col_abs { "$" } else { "" }, col_to_letters(a.col), if a.row_abs { "$" } else { "" }, a.row as u64 + 1)
}

pub fn reference_a1(r: &Reference) -> String {
    let body = match &r.kind {
        RefKind::Cell(a) => anchor_a1(a),
        RefKind::Range(a, b) => format!("{}:{}", anchor_a1(a), anchor_a1(b)),
        RefKind::Rows(r0, a0, r1, a1) => format!("{}{}:{}{}", if *a0 { "$" } else { "" }, r0 + 1, if *a1 { "$" } else { "" }, r1 + 1),
        RefKind::Cols(c0, a0, c1, a1) => {
            format!("{}{}:{}{}", if *a0 { "$" } else { "" }, col_to_letters(*c0), if *a1 { "$" } else { "" }, col_to_letters(*c1))
        }
    };
    format!("{}{}", sheet_prefix(&r.sheet), body)
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
    let body = match &r.kind {
        RefKind::Cell(a) => anchor_r1c1(a, at),
        RefKind::Range(a, b) => format!("{}:{}", anchor_r1c1(a, at), anchor_r1c1(b, at)),
        RefKind::Rows(r0, a0, r1, a1) => {
            let (x, y) = (r1c1_part('R', *r0, *a0, at.row), r1c1_part('R', *r1, *a1, at.row));
            if x == y { x } else { format!("{x}:{y}") }
        }
        RefKind::Cols(c0, a0, c1, a1) => {
            let (x, y) = (r1c1_part('C', *c0, *a0, at.col), r1c1_part('C', *c1, *a1, at.col));
            if x == y { x } else { format!("{x}:{y}") }
        }
    };
    format!("{}{}", sheet_prefix(&r.sheet), body)
}

pub fn struct_ref(s: &StructRef) -> String {
    let esc = |c: &str| c.replace('\'', "''").replace('[', "'[").replace(']', "']").replace('#', "'#");
    let item = |i: &StructItem| match i {
        StructItem::All => "#All",
        StructItem::Data => "#Data",
        StructItem::Headers => "#Headers",
        StructItem::Totals => "#Totals",
        StructItem::ThisRow => "#This Row",
    };
    let this_row = s.specifiers == [StructItem::ThisRow];
    let mut out = s.table.clone();
    let simple_col = |c: &str| !c.chars().any(|ch| matches!(ch, ' ' | '[' | ']' | '#' | '\'' | ',' | ':'));
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
            let _ = write!(out, "[{}]", parts.join(","));
        }
    }
    out
}

fn number(n: f64) -> String {
    number_to_text(n)
}

/// Prints an expression in A1 notation.
pub fn print(e: &Expr) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, None);
    s
}

/// Prints an expression in R1C1 notation relative to `at`.
pub fn print_r1c1(e: &Expr, at: CellRef) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Some(at));
    s
}

fn write_expr(s: &mut String, e: &Expr, r1c1: Option<CellRef>) {
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
                    write_expr(s, el, r1c1);
                }
            }
            s.push('}');
        }
        Expr::Ref(r) => match r1c1 {
            Some(at) => s.push_str(&reference_r1c1(r, at)),
            None => s.push_str(&reference_a1(r)),
        },
        Expr::Name(n) => s.push_str(n),
        Expr::Struct(st) => s.push_str(&struct_ref(st)),
        Expr::Unary(op, x) => match op {
            UnOp::Neg => {
                s.push('-');
                write_expr(s, x, r1c1);
            }
            UnOp::Plus => {
                s.push('+');
                write_expr(s, x, r1c1);
            }
            UnOp::At => {
                s.push('@');
                write_expr(s, x, r1c1);
            }
            UnOp::Percent => {
                write_expr(s, x, r1c1);
                s.push('%');
            }
            UnOp::Spill => {
                write_expr(s, x, r1c1);
                // A reference that became #REF! (its sheet or cell was deleted) stays `#REF!`.
                if !matches!(**x, Expr::Error(_)) {
                    s.push('#');
                }
            }
        },
        Expr::Binary(op, a, b) => {
            write_expr(s, a, r1c1);
            s.push_str(op.symbol());
            write_expr(s, b, r1c1);
        }
        Expr::Call(name, args) => {
            s.push_str(name);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, r1c1);
            }
            s.push(')');
        }
        Expr::Invoke(c, args) => {
            write_expr(s, c, r1c1);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, r1c1);
            }
            s.push(')');
        }
        Expr::Missing => {}
        Expr::Paren(x) => {
            s.push('(');
            write_expr(s, x, r1c1);
            s.push(')');
        }
    }
}
