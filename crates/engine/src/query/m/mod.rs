//! Power Query M: running the queries Excel keeps in a workbook (`ExcelQuery::formula`).
//!
//! The language follows Microsoft's public Power Query M language specification and function
//! reference: a lexer and parser ([`parser::parse`]), an evaluator with lazy `let` and record
//! fields ([`eval::Ev`]), and the part of the standard library the Power Query Editor writes
//! (sources, `Table.*`, and the scalar functions its steps use). Tables reuse the query engine's
//! sources, data access, type conversion and loading, so an Excel query refreshes like a
//! GridCraft one. A name GridCraft doesn't know is reported, and [`unsupported`] lists them
//! without running anything.

pub mod ast;
pub mod eval;
pub mod lexer;
pub mod library;
pub mod parser;
pub mod value;

use std::rc::Rc;

use gridcraft_core::{DateSystem, Value};
use gridcraft_model::query::ColType;

use super::{EvalCx, QResult, QTable};
use ast::{Expr, ListItem, TypeExpr};
use value::{MTable, MValue};

/// Longest text value, in bytes.
pub const MAX_TEXT: usize = 64 * 1024 * 1024;

/// What refreshing an Excel query produced: its table and how many cells held errors (loaded
/// blank, as Excel does).
#[derive(Debug)]
pub struct Refreshed {
    pub table: QTable,
    pub errors: usize,
}

/// Evaluates Excel's queries `names` (each once, sharing the queries they refer to).
pub fn evaluate_excel_queries(cx: &mut EvalCx<'_>, names: &[String]) -> Vec<QResult<Refreshed>> {
    let mut ev = eval::Ev::new(cx);
    let sys = ev.sys;
    names
        .iter()
        .map(|n| {
            let v = ev.query(n).map_err(|e| e.to_string())?;
            let t = match v {
                MValue::Table(t) => (*t).clone(),
                MValue::List(l) => {
                    let mut t = MTable::new(vec!["Column1".into()]);
                    for x in l.iter() {
                        t.push(vec![x.clone()]).map_err(|e| e.to_string())?;
                    }
                    t
                }
                MValue::Record(r) => {
                    let mut t = MTable::new(r.names.to_vec());
                    t.push(r.values.clone()).map_err(|e| e.to_string())?;
                    t
                }
                other => {
                    let mut t = MTable::new(vec![Rc::from(n.as_str())]);
                    t.push(vec![other]).map_err(|e| e.to_string())?;
                    t
                }
            };
            Ok(to_qtable(&t, sys))
        })
        .collect()
}

/// A worksheet-valued table read into M.
pub fn from_qtable(t: &QTable, sys: DateSystem) -> MTable {
    let names: Vec<Rc<str>> = t.columns.iter().map(|c| Rc::from(c.as_str())).collect();
    let rows = t
        .rows
        .iter()
        .map(|r| r.iter().enumerate().map(|(i, v)| MValue::from_cell(v, t.types.get(i).copied().unwrap_or_default(), sys)).collect())
        .collect();
    MTable { names: names.into(), types: t.types.clone(), rows }
}

/// An M table as worksheet values for loading; untyped columns holding only dates or times
/// are typed so they load with a date format.
pub fn to_qtable(t: &MTable, sys: DateSystem) -> Refreshed {
    let mut errors = 0usize;
    let mut out = QTable::with_columns(t.names.iter().map(|n| n.to_string()).collect());
    for (i, ty) in t.types.iter().enumerate() {
        let declared = if *ty == ColType::Any { inferred(t, i) } else { *ty };
        if let Some(slot) = out.types.get_mut(i) {
            *slot = declared;
        }
    }
    for r in &t.rows {
        let mut row: Vec<Value> = Vec::with_capacity(r.len());
        for v in r {
            if matches!(v, MValue::Error(_)) {
                errors += 1;
            }
            row.push(v.to_cell(sys));
        }
        out.push_row(row);
    }
    Refreshed { table: out, errors }
}

fn inferred(t: &MTable, col: usize) -> ColType {
    let mut kind: Option<ColType> = None;
    for v in t.rows.iter().filter_map(|r| r.get(col)).filter(|v| !v.is_null()).take(super::types::DETECT_ROWS) {
        let k = match v {
            MValue::Date(_) => ColType::Date,
            MValue::DateTime(_) | MValue::DateTimeZone(..) => ColType::DateTime,
            MValue::Time(_) => ColType::Time,
            _ => return ColType::Any,
        };
        kind = match kind {
            None => Some(k),
            Some(prev) if prev == k => Some(k),
            Some(ColType::Date | ColType::DateTime) if matches!(k, ColType::Date | ColType::DateTime) => Some(ColType::DateTime),
            Some(_) => return ColType::Any,
        };
    }
    kind.unwrap_or(ColType::Any)
}

/// The names a formula uses that GridCraft can't run: library functions it lacks and names
/// that are neither variables, other queries (`is_query`) nor library names. Errors when the
/// formula doesn't parse.
pub fn unsupported(formula: &str, is_query: &dyn Fn(&str) -> bool) -> Result<Vec<String>, value::MError> {
    let e = parser::parse(formula)?;
    let mut found: Vec<String> = Vec::new();
    let mut scope: Vec<Rc<str>> = Vec::new();
    walk(&e, &mut scope, &mut found, is_query, 0);
    found.sort();
    found.dedup();
    Ok(found)
}

fn walk(e: &Expr, scope: &mut Vec<Rc<str>>, found: &mut Vec<String>, is_query: &dyn Fn(&str) -> bool, depth: usize) {
    if depth > 400 {
        return;
    }
    let d = depth + 1;
    match e {
        Expr::Ident(n) => {
            if !scope.iter().any(|s| s == n) && !library::known(n) && !is_query(n) && found.len() < 1_000 {
                found.push(n.to_string());
            }
        }
        Expr::List(items) => {
            for it in items {
                match it {
                    ListItem::One(x) => walk(x, scope, found, is_query, d),
                    ListItem::Range(a, b) => {
                        walk(a, scope, found, is_query, d);
                        walk(b, scope, found, is_query, d);
                    }
                }
            }
        }
        Expr::Record(fields) | Expr::Let(fields, _) => {
            let mark = scope.len();
            scope.extend(fields.iter().map(|(n, _)| n.clone()));
            for (_, x) in fields.iter() {
                walk(x, scope, found, is_query, d);
            }
            if let Expr::Let(_, body) = e {
                walk(body, scope, found, is_query, d);
            }
            scope.truncate(mark);
        }
        Expr::Func(def) => {
            let mark = scope.len();
            scope.extend(def.params.iter().map(|p| p.name.clone()));
            walk(&def.body, scope, found, is_query, d);
            scope.truncate(mark);
        }
        Expr::If(a, b, c) => {
            for x in [a, b, c] {
                walk(x, scope, found, is_query, d);
            }
        }
        Expr::Call(f, args) => {
            walk(f, scope, found, is_query, d);
            for x in args {
                walk(x, scope, found, is_query, d);
            }
        }
        Expr::Field { target, .. } | Expr::Project { target, .. } => {
            if let Some(t) = target {
                walk(t, scope, found, is_query, d);
            }
        }
        Expr::Item { target, key, .. } => {
            walk(target, scope, found, is_query, d);
            walk(key, scope, found, is_query, d);
        }
        Expr::Unary(_, x) | Expr::Error(x) => walk(x, scope, found, is_query, d),
        Expr::Binary(_, a, b) | Expr::Meta(a, b) => {
            walk(a, scope, found, is_query, d);
            walk(b, scope, found, is_query, d);
        }
        Expr::Try { body, otherwise, catch } => {
            walk(body, scope, found, is_query, d);
            if let Some(o) = otherwise {
                walk(o, scope, found, is_query, d);
            }
            if let Some(f) = catch {
                let mark = scope.len();
                scope.extend(f.params.iter().map(|p| p.name.clone()));
                walk(&f.body, scope, found, is_query, d);
                scope.truncate(mark);
            }
        }
        Expr::Type(t) => walk_type(t, scope, found, is_query, d),
        Expr::Is(x, t) | Expr::As(x, t) => {
            walk(x, scope, found, is_query, d);
            walk_type(t, scope, found, is_query, d);
        }
        Expr::Null | Expr::Bool(_) | Expr::Num(_) | Expr::Text(_) | Expr::NotImplemented => {}
    }
}

fn walk_type(t: &TypeExpr, scope: &mut Vec<Rc<str>>, found: &mut Vec<String>, is_query: &dyn Fn(&str) -> bool, depth: usize) {
    if depth > 400 {
        return;
    }
    match t {
        TypeExpr::Expr(e) => walk(e, scope, found, is_query, depth + 1),
        TypeExpr::Nullable(x) | TypeExpr::List(x) => walk_type(x, scope, found, is_query, depth + 1),
        TypeExpr::Record(fs) | TypeExpr::Table(fs) => {
            for (_, x) in fs {
                walk_type(x, scope, found, is_query, depth + 1);
            }
        }
        TypeExpr::Prim(_) | TypeExpr::Function => {}
    }
}

#[cfg(test)]
mod tests;
