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

/// How to print: A1, or R1C1 relative to a cell; with the relative parts of references moved
/// by a shift (a formula copied that far, see [`print_shifted`]).
#[derive(Clone, Copy)]
struct Print {
    r1c1: Option<CellRef>,
    shift: (i64, i64),
}

/// Prints an expression in A1 notation.
pub fn print(e: &Expr) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Print { r1c1: None, shift: (0, 0) });
    s
}

/// Prints an expression in R1C1 notation relative to `at`.
pub fn print_r1c1(e: &Expr, at: CellRef) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Print { r1c1: Some(at), shift: (0, 0) });
    s
}

/// Prints `e` copied `dr` rows and `dc` columns away, in A1 notation: the same text as
/// `print(&shift_relative(e, dr, dc))` (references pushed off the sheet print `#REF!`), without
/// building the moved expression.
pub fn print_shifted(e: &Expr, dr: i64, dc: i64) -> String {
    let mut s = String::new();
    write_expr(&mut s, e, Print { r1c1: None, shift: (dr, dc) });
    s
}

fn write_expr(s: &mut String, e: &Expr, p: Print) {
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
                    write_expr(s, el, p);
                }
            }
            s.push('}');
        }
        Expr::Ref(r) => {
            let moved;
            let r = if p.shift == (0, 0) {
                r
            } else {
                match crate::adjust::shift_ref(r, p.shift.0, p.shift.1) {
                    Some(x) => {
                        moved = x;
                        &moved
                    }
                    None => {
                        s.push_str(gridcraft_core::CellError::Ref.as_str());
                        return;
                    }
                }
            };
            match p.r1c1 {
                Some(at) => s.push_str(&reference_r1c1(r, at)),
                None => s.push_str(&reference_a1(r)),
            }
        }
        Expr::Name(n) => s.push_str(n),
        Expr::Struct(st) => s.push_str(&struct_ref(st)),
        Expr::Unary(op, x) => match op {
            UnOp::Neg => {
                s.push('-');
                write_expr(s, x, p);
            }
            UnOp::Plus => {
                s.push('+');
                write_expr(s, x, p);
            }
            UnOp::At => {
                s.push('@');
                write_expr(s, x, p);
            }
            UnOp::Percent => {
                write_expr(s, x, p);
                s.push('%');
            }
            UnOp::Spill => {
                write_expr(s, x, p);
                // A reference that became #REF! (its sheet or cell was deleted, or a copy moved it
                // off the sheet) stays `#REF!`.
                let off_sheet = p.shift != (0, 0) && matches!(&**x, Expr::Ref(r) if crate::adjust::shift_ref(r, p.shift.0, p.shift.1).is_none());
                if !matches!(**x, Expr::Error(_)) && !off_sheet {
                    s.push('#');
                }
            }
        },
        Expr::Binary(op, a, b) => {
            write_expr(s, a, p);
            s.push_str(op.symbol());
            write_expr(s, b, p);
        }
        Expr::Call(name, args) => {
            s.push_str(name);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, p);
            }
            s.push(')');
        }
        Expr::Invoke(c, args) => {
            write_expr(s, c, p);
            s.push('(');
            for (i, a) in args.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_expr(s, a, p);
            }
            s.push(')');
        }
        Expr::Missing => {}
        Expr::Paren(x) => {
            s.push('(');
            write_expr(s, x, p);
            s.push(')');
        }
    }
}
