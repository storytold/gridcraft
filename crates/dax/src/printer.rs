//! Canonical DAX printer: [`Expr`] / [`Query`] → text.
//!
//! Printing is iterative (an explicit work stack), so long operator chains — which parse into
//! deep trees — cannot overflow the stack. Parentheses are emitted only where precedence or
//! associativity requires them; re-parsing the result yields the same AST (spans ignored).

use std::fmt::Write as _;

use crate::ast::{Call, Expr, ExprKind, Literal, Name, NameRef, NameStyle, Query, UnaryOp};

/// Prints an expression in canonical form.
pub fn print_expression(expr: &Expr) -> String {
    let mut out = String::new();
    write_expr(&mut out, expr, 0);
    out
}

/// Prints a query in canonical form: `EVALUATE <expr>`.
pub fn print_query(query: &Query) -> String {
    let mut out = String::from("EVALUATE ");
    write_expr(&mut out, &query.evaluate, 0);
    out
}

fn write_quoted(out: &mut String, text: &str, quote: char) {
    out.push(quote);
    for c in text.chars() {
        if c == quote {
            out.push(quote);
        }
        out.push(c);
    }
    out.push(quote);
}

fn write_bracketed(out: &mut String, text: &str) {
    out.push('[');
    for c in text.chars() {
        if c == ']' {
            out.push(']');
        }
        out.push(c);
    }
    out.push(']');
}

fn write_name(out: &mut String, name: &Name) {
    match name.style {
        NameStyle::Bare => out.push_str(&name.text),
        NameStyle::Quoted => write_quoted(out, &name.text, '\''),
        NameStyle::Bracketed => write_bracketed(out, &name.text),
    }
}

fn write_name_ref(out: &mut String, name: &NameRef) {
    if let Some(table) = &name.table {
        write_name(out, table);
    }
    write_name(out, &name.name);
}

fn write_literal(out: &mut String, literal: &Literal) {
    match literal {
        Literal::Integer(n) => {
            let _ = write!(out, "{n}");
        }
        Literal::Decimal(n) => {
            // `Debug` keeps a decimal point (`1000.0`) or an exponent (`1e300`) for every finite
            // value, so the printed text re-lexes as a Decimal instead of an Integer.
            let _ = write!(out, "{n:?}");
        }
        Literal::DateTime(value) => {
            let _ = write!(out, "dt\"{value}\"");
        }
        Literal::String(s) => write_quoted(out, s, '"'),
        Literal::Boolean(true) => out.push_str("TRUE"),
        Literal::Boolean(false) => out.push_str("FALSE"),
    }
}

fn write_call_head(out: &mut String, call: &Call) {
    write_name(out, &call.name);
    out.push('(');
}

/// One element of the printer's work stack. Expressions are expanded here rather than by
/// recursion; text tasks cover operators and separators.
enum Task<'a> {
    Expr(&'a Expr, u8),
    Text(&'static str),
}

fn write_expr(out: &mut String, root: &Expr, root_min_bp: u8) {
    let mut stack: Vec<Task<'_>> = vec![Task::Expr(root, root_min_bp)];

    while let Some(task) = stack.pop() {
        match task {
            Task::Text(text) => out.push_str(text),
            Task::Expr(expr, min_bp) => match &expr.kind {
                ExprKind::Literal(literal) => write_literal(out, literal),
                ExprKind::Name(name) => write_name_ref(out, name),
                ExprKind::Unary { op, expr } => {
                    let bp = op.precedence();
                    let parens = bp < min_bp;
                    if parens {
                        out.push('(');
                        stack.push(Task::Text(")"));
                    }
                    stack.push(Task::Expr(expr, bp));
                    if matches!(op, UnaryOp::Plus | UnaryOp::Neg) && matches!(expr.kind, ExprKind::Unary { op: UnaryOp::Plus | UnaryOp::Neg, .. }) {
                        // `--` would start a line comment; separate repeated signs.
                        stack.push(Task::Text(" "));
                    }
                    stack.push(Task::Text(match op {
                        UnaryOp::Plus => "+",
                        UnaryOp::Neg => "-",
                        UnaryOp::Not => "NOT ",
                    }));
                }
                ExprKind::Binary { op, left, right } => {
                    let bp = op.precedence();
                    let parens = bp < min_bp;
                    if parens {
                        out.push('(');
                        stack.push(Task::Text(")"));
                    }
                    let (left_min, right_min) = if op.is_right_associative() { (bp + 1, bp) } else { (bp, bp + 1) };
                    stack.push(Task::Expr(right, right_min));
                    stack.push(Task::Text(" "));
                    stack.push(Task::Text(op.symbol()));
                    stack.push(Task::Text(" "));
                    stack.push(Task::Expr(left, left_min));
                }
                ExprKind::Call(call) => {
                    write_call_head(out, call);
                    if call.args.is_empty() {
                        out.push(')');
                    } else {
                        stack.push(Task::Text(")"));
                        for (index, arg) in call.args.iter().enumerate().rev() {
                            stack.push(Task::Expr(arg, 0));
                            if index > 0 {
                                stack.push(Task::Text(", "));
                            }
                        }
                    }
                }
                ExprKind::Table(items) => {
                    out.push('{');
                    if items.is_empty() {
                        out.push('}');
                    } else {
                        out.push(' ');
                        stack.push(Task::Text(" }"));
                        for (index, item) in items.iter().enumerate().rev() {
                            stack.push(Task::Expr(item, 0));
                            if index > 0 {
                                stack.push(Task::Text(", "));
                            }
                        }
                    }
                }
                ExprKind::Tuple(items) => {
                    out.push('(');
                    if items.is_empty() {
                        out.push(')');
                    } else {
                        stack.push(Task::Text(")"));
                        for (index, item) in items.iter().enumerate().rev() {
                            stack.push(Task::Expr(item, 0));
                            if index > 0 {
                                stack.push(Task::Text(", "));
                            }
                        }
                    }
                }
            },
        }
    }
}
