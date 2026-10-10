//! Evaluates a parsed macro body against a [`Host`].

use std::collections::HashMap;

use crate::ast::{BinOp, CellExpr, Expr, Place, Stmt, UnOp, Value};

/// The thing that actually owns the spreadsheet: cell storage and a way to surface `MsgBox`.
/// Row/column are 0-based, matching the convention a spreadsheet engine normally uses internally
/// — the interpreter itself converts from VBA's 1-based `Cells(row, col)` and `"A1"`-style
/// `Range` text before calling in.
pub trait Host {
    fn get_cell(&self, sheet: Option<&str>, row: u32, col: u32) -> Value;
    fn set_cell(&mut self, sheet: Option<&str>, row: u32, col: u32, value: Value);
    fn msg_box(&mut self, text: &str);
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RunError {
    #[error("{0}")]
    Other(String),
}

pub fn exec_block(stmts: &[Stmt], host: &mut dyn Host) -> Result<(), RunError> {
    let mut interp = Interp { env: HashMap::new(), host };
    interp.block(stmts)
}

struct Interp<'h> {
    env: HashMap<String, Value>,
    host: &'h mut dyn Host,
}

impl Interp<'_> {
    fn block(&mut self, stmts: &[Stmt]) -> Result<(), RunError> {
        for s in stmts {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), RunError> {
        match s {
            Stmt::Dim(name) => {
                self.env.entry(name.to_ascii_lowercase()).or_insert(Value::Empty);
                Ok(())
            }
            Stmt::Assign(place, expr) => {
                let v = self.eval(expr)?;
                self.assign(place, v)
            }
            Stmt::If { branches, else_body } => {
                for (cond, body) in branches {
                    if self.eval(cond)?.truthy() {
                        return self.block(body);
                    }
                }
                self.block(else_body)
            }
            Stmt::For { var, start, end, step, body } => {
                let start = self.eval(start)?.as_number();
                let end = self.eval(end)?.as_number();
                let step = match step {
                    Some(e) => self.eval(e)?.as_number(),
                    None => 1.0,
                };
                if step == 0.0 {
                    return Err(RunError::Other("For loop Step is 0 — would never end".into()));
                }
                let key = var.to_ascii_lowercase();
                let mut i = start;
                // A real guard against runaway loops from a bad macro (e.g. `For i = 1 To 1E300`).
                let mut iterations = 0u32;
                while (step > 0.0 && i <= end) || (step < 0.0 && i >= end) {
                    self.env.insert(key.clone(), Value::Number(i));
                    self.block(body)?;
                    i += step;
                    iterations += 1;
                    if iterations > 1_000_000 {
                        return Err(RunError::Other("For loop exceeded 1,000,000 iterations".into()));
                    }
                }
                Ok(())
            }
            Stmt::MsgBox(e) => {
                let v = self.eval(e)?;
                self.host.msg_box(&v.display());
                Ok(())
            }
        }
    }

    fn assign(&mut self, place: &Place, v: Value) -> Result<(), RunError> {
        match place {
            Place::Var(name) => {
                self.env.insert(name.to_ascii_lowercase(), v);
                Ok(())
            }
            Place::Cell { sheet, cell } => {
                let sheet_name = match sheet {
                    Some(e) => Some(self.eval(e)?.display()),
                    None => None,
                };
                let (row, col) = self.resolve_cell(cell)?;
                self.host.set_cell(sheet_name.as_deref(), row, col, v);
                Ok(())
            }
        }
    }

    fn eval(&mut self, e: &Expr) -> Result<Value, RunError> {
        match e {
            Expr::Lit(v) => Ok(v.clone()),
            Expr::Var(name) => Ok(self.env.get(&name.to_ascii_lowercase()).cloned().unwrap_or(Value::Empty)),
            Expr::Unary(op, inner) => {
                let v = self.eval(inner)?;
                Ok(match op {
                    UnOp::Neg => Value::Number(-v.as_number()),
                    UnOp::Not => Value::Bool(!v.truthy()),
                })
            }
            Expr::Bin(op, l, r) => {
                let l = self.eval(l)?;
                let r = self.eval(r)?;
                Ok(self.bin(*op, l, r))
            }
            Expr::CellRead { sheet, cell } => {
                let sheet_name = match sheet {
                    Some(e) => Some(self.eval(e)?.display()),
                    None => None,
                };
                let (row, col) = self.resolve_cell(cell)?;
                Ok(self.host.get_cell(sheet_name.as_deref(), row, col))
            }
        }
    }

    fn bin(&self, op: BinOp, l: Value, r: Value) -> Value {
        match op {
            BinOp::Add => Value::Number(l.as_number() + r.as_number()),
            BinOp::Sub => Value::Number(l.as_number() - r.as_number()),
            BinOp::Mul => Value::Number(l.as_number() * r.as_number()),
            BinOp::Div => {
                let d = r.as_number();
                Value::Number(if d == 0.0 { f64::NAN } else { l.as_number() / d })
            }
            BinOp::Pow => Value::Number(l.as_number().powf(r.as_number())),
            BinOp::Concat => Value::Str(format!("{}{}", l.display(), r.display())),
            BinOp::Eq => Value::Bool(values_eq(&l, &r)),
            BinOp::Ne => Value::Bool(!values_eq(&l, &r)),
            BinOp::Lt => Value::Bool(compare(&l, &r).is_lt()),
            BinOp::Le => Value::Bool(compare(&l, &r).is_le()),
            BinOp::Gt => Value::Bool(compare(&l, &r).is_gt()),
            BinOp::Ge => Value::Bool(compare(&l, &r).is_ge()),
            BinOp::And => Value::Bool(l.truthy() && r.truthy()),
            BinOp::Or => Value::Bool(l.truthy() || r.truthy()),
        }
    }

    /// Resolves a `CellExpr` to a 0-based `(row, col)`, converting from VBA's 1-based `Cells`
    /// coordinates and parsing `"A1"`-style text for `Range`.
    fn resolve_cell(&mut self, cell: &CellExpr) -> Result<(u32, u32), RunError> {
        match cell {
            CellExpr::A1(e) => {
                let text = self.eval(e)?.display();
                a1_to_rc(&text).ok_or_else(|| RunError::Other(format!("`{text}` isn't a valid cell reference")))
            }
            CellExpr::RowCol(r, c) => {
                let row = self.eval(r)?.as_number();
                let col = self.eval(c)?.as_number();
                if row < 1.0 || col < 1.0 {
                    return Err(RunError::Other("Cells(row, col) is 1-based — row and column must each be at least 1".into()));
                }
                Ok((row as u32 - 1, col as u32 - 1))
            }
        }
    }
}

fn values_eq(l: &Value, r: &Value) -> bool {
    match (l, r) {
        (Value::Str(a), Value::Str(b)) => a.eq_ignore_ascii_case(b),
        _ => l.as_number() == r.as_number(),
    }
}

fn compare(l: &Value, r: &Value) -> std::cmp::Ordering {
    match (l, r) {
        (Value::Str(a), Value::Str(b)) => a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase()),
        _ => l.as_number().partial_cmp(&r.as_number()).unwrap_or(std::cmp::Ordering::Equal),
    }
}

/// Parses `"A1"`-style text (case-insensitive, `$` ignored) into 0-based `(row, col)`.
fn a1_to_rc(s: &str) -> Option<(u32, u32)> {
    let s: String = s.trim().chars().filter(|&c| c != '$').collect();
    let split = s.find(|c: char| c.is_ascii_digit())?;
    let (col_text, row_text) = s.split_at(split);
    if col_text.is_empty() || row_text.is_empty() || !col_text.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut col: u32 = 0;
    for c in col_text.chars() {
        col = col * 26 + (c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
    }
    let row: u32 = row_text.parse().ok()?;
    if col == 0 || row == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

/// Formats a number the way VBA's default `Variant`-to-string conversion roughly would: no
/// trailing `.0` for whole numbers, no scientific notation for ordinary magnitudes.
pub fn format_number(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n}");
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a1_parses() {
        assert_eq!(a1_to_rc("A1"), Some((0, 0)));
        assert_eq!(a1_to_rc("$B$2"), Some((1, 1)));
        assert_eq!(a1_to_rc("AA10"), Some((9, 26)));
        assert_eq!(a1_to_rc(""), None);
        assert_eq!(a1_to_rc("1A"), None);
    }
}
