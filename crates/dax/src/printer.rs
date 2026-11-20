//! Canonical DAX printer: [`Expr`] / [`Query`] → text.
//!
//! Printing is iterative (an explicit work stack), so long operator chains — which parse into
//! deep trees — cannot overflow the stack. Parentheses are emitted only where precedence or
//! associativity requires them; re-parsing the result yields the same AST (spans ignored).

use std::fmt::Write as _;

use crate::ast::{
    Axis, Call, Definition, Evaluate, Expr, ExprKind, FunctionParam, Literal, Name, NameRef, NameStyle, Query, StartValue, UnaryOp, VisualShape,
};

/// An expression-level `VAR ... RETURN ...` binds looser than every operator, so it needs
/// parentheses whenever it is an operand rather than a whole expression.
const VAR_RETURN_BP: u8 = 1;

/// Prints an expression in canonical form.
pub fn print_expression(expr: &Expr) -> String {
    let mut out = String::new();
    write_expr(&mut out, expr, 0);
    out
}

/// Prints a query in canonical form.
///
/// `DEFINE` entries are indented one level; `EVALUATE` is printed on one line for the simple
/// `EVALUATE <expression>` form and followed by indented `ORDER BY`/`START AT` clauses when they
/// exist. A visual shape prints its axes, groups, axis order and `DENSIFY` indented under the
/// `TABLE` definition. Re-parsing the result yields the same [`Query`] (spans ignored).
pub fn print_query(query: &Query) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(define) = &query.define {
        lines.push("DEFINE".into());
        for item in &define.items {
            push_definition(&mut lines, item);
        }
    }
    for evaluate in &query.evaluates {
        push_evaluate(&mut lines, evaluate);
    }
    lines.join("\n")
}

fn print_name(name: &Name) -> String {
    let mut out = String::new();
    write_name(&mut out, name);
    out
}

fn print_name_ref(name: &NameRef) -> String {
    let mut out = String::new();
    write_name_ref(&mut out, name);
    out
}

fn push_definition(lines: &mut Vec<String>, definition: &Definition) {
    match definition {
        Definition::Measure(def) => lines.push(format!("    MEASURE {} = {}", print_name_ref(&def.name), print_expression(&def.expr))),
        Definition::Column(def) => lines.push(format!("    COLUMN {} = {}", print_name_ref(&def.name), print_expression(&def.expr))),
        Definition::Var(def) => lines.push(format!("    VAR {} = {}", print_name(&def.name), print_expression(&def.expr))),
        Definition::MParameter(def) => {
            lines.push(format!("    MPARAMETER {} = {}", print_name(&def.name), print_expression(&def.expr)));
        }
        Definition::Function(def) => {
            let params: Vec<String> = def.params.iter().map(print_function_param).collect();
            lines.push(format!("    FUNCTION {} = ({}) => {}", print_name(&def.name), params.join(", "), print_expression(&def.body)));
        }
        Definition::Table(def) => {
            let mut head = format!("    TABLE {} = {}", print_name(&def.name), print_expression(&def.expr));
            match &def.visual_shape {
                None => lines.push(head),
                Some(shape) => {
                    head.push_str(" WITH VISUAL SHAPE");
                    lines.push(head);
                    push_visual_shape(lines, shape);
                }
            }
        }
    }
}

fn print_function_param(param: &FunctionParam) -> String {
    let mut out = print_name(&param.name);
    let mut annotations: Vec<&'static str> = Vec::new();
    if let Some(ty) = param.ty {
        annotations.push(ty.keyword());
    }
    if let Some(subtype) = param.subtype {
        annotations.push(subtype.keyword());
    }
    if let Some(passing) = param.passing {
        annotations.push(passing.keyword());
    }
    if !annotations.is_empty() {
        out.push_str(" : ");
        out.push_str(&annotations.join(" "));
    }
    if let Some(default) = &param.default {
        out.push_str(" = ");
        out.push_str(&print_expression(default));
    }
    out
}

fn push_visual_shape(lines: &mut Vec<String>, shape: &VisualShape) {
    for axis in &shape.axes {
        push_axis(lines, axis);
    }
    if let Some(densify) = &shape.densify {
        let mut line = String::from("        DENSIFY ");
        write_quoted(&mut line, densify, '"');
        lines.push(line);
    }
}

fn push_axis(lines: &mut Vec<String>, axis: &Axis) {
    lines.push(format!("        AXIS {}", axis.kind.keyword()));
    for group in &axis.groups {
        let columns: Vec<String> = group.columns.iter().map(print_name_ref).collect();
        lines.push(format!("            GROUP {} TOTAL {}", columns.join(", "), print_name_ref(&group.total)));
    }
    lines.push("            ORDER BY".into());
    for (index, item) in axis.order_by.iter().enumerate() {
        let mut line = format!("                {} {}", print_name_ref(&item.column), item.direction.keyword());
        if index + 1 < axis.order_by.len() {
            line.push(',');
        }
        lines.push(line);
    }
}

fn push_evaluate(lines: &mut Vec<String>, evaluate: &Evaluate) {
    lines.push(format!("EVALUATE {}", print_expression(&evaluate.table)));
    if !evaluate.order_by.is_empty() {
        lines.push("ORDER BY".into());
        for (index, item) in evaluate.order_by.iter().enumerate() {
            let mut line = format!("    {} {}", print_expression(&item.expr), item.direction.keyword());
            if index + 1 < evaluate.order_by.len() {
                line.push(',');
            }
            lines.push(line);
        }
    }
    if !evaluate.start_at.is_empty() {
        let mut line = String::from("START AT ");
        for (index, value) in evaluate.start_at.iter().enumerate() {
            if index > 0 {
                line.push_str(", ");
            }
            write_start_value(&mut line, value);
        }
        lines.push(line);
    }
}

fn write_start_value(out: &mut String, value: &StartValue) {
    match value {
        StartValue::Literal(literal) => write_literal(out, literal),
        StartValue::Parameter(name) => {
            out.push('@');
            out.push_str(&name.text);
        }
    }
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
    Name(&'a Name),
}

fn write_expr(out: &mut String, root: &Expr, root_min_bp: u8) {
    let mut stack: Vec<Task<'_>> = vec![Task::Expr(root, root_min_bp)];

    while let Some(task) = stack.pop() {
        match task {
            Task::Text(text) => out.push_str(text),
            Task::Name(name) => write_name(out, name),
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
                ExprKind::VarReturn { vars, body } => {
                    let parens = VAR_RETURN_BP < min_bp;
                    if parens {
                        out.push('(');
                        stack.push(Task::Text(")"));
                    }
                    stack.push(Task::Expr(body, 0));
                    stack.push(Task::Text("RETURN "));
                    for var in vars.iter().rev() {
                        stack.push(Task::Text(" "));
                        stack.push(Task::Expr(&var.expr, 0));
                        stack.push(Task::Text(" = "));
                        stack.push(Task::Name(&var.name));
                        stack.push(Task::Text("VAR "));
                    }
                }
            },
        }
    }
}
