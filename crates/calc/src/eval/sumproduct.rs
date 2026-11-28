use std::sync::Arc;

use gridcraft_core::{Array, CellError, DateSystem, Value};
use gridcraft_formula::{BinOp, Expr, UnOp};

use super::array::{binary_scalar, map_unary};
use super::{Ev, Evaluator, err};

/// Minimum column height where fused/indexed `SUMPRODUCT` evaluation pays off.
const MIN_FUSED_ROWS: usize = 64;
/// Maximum bounded integer magnitude tracked as overflow-free for arithmetic of comparisons.
const MAX_SAFE_BOUND: u32 = 1_000_000;

/// Error-freedom classification of an [`SpNode`] across all rows `0..H`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ErrorSafety {
    /// May produce `Value::Error` on some row (division, error value, text in arithmetic, or
    /// unbounded arithmetic that could overflow `f64::MAX`).
    Unsafe,
    /// Guaranteed never to produce `Value::Error` on any row (all numeric outputs are finite).
    SafeFinite,
    /// Guaranteed never to produce `Value::Error`, and numeric values have `|x| <= bound`.
    SafeBounded(u32),
}

enum SpNode {
    Scalar(Value),
    Col(Arc<Array>),
    Unary(UnOp, Box<SpNode>),
    Binary(BinOp, Box<SpNode>, Box<SpNode>),
}

struct CompiledArg {
    node: SpNode,
    rows: usize,
}

impl SpNode {
    fn safety(&self, in_arith: bool, sys: DateSystem) -> ErrorSafety {
        match self {
            SpNode::Scalar(v) => {
                if v.is_error() {
                    return ErrorSafety::Unsafe;
                }
                if !in_arith {
                    return ErrorSafety::SafeFinite;
                }
                match v.to_number_in(sys) {
                    Ok(n) if n.is_finite() => {
                        let a = n.abs();
                        if a <= 1_000.0 { ErrorSafety::SafeBounded(a.ceil() as u32) } else { ErrorSafety::SafeFinite }
                    }
                    _ => ErrorSafety::Unsafe,
                }
            }
            SpNode::Col(arr) => {
                if arr.has_errors() {
                    return ErrorSafety::Unsafe;
                }
                if in_arith && arr.has_text() {
                    return ErrorSafety::Unsafe;
                }
                ErrorSafety::SafeFinite
            }
            SpNode::Unary(_, inner) => inner.safety(true, sys),
            SpNode::Binary(op, l, r) => match op {
                BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                    if l.safety(false, sys) != ErrorSafety::Unsafe && r.safety(false, sys) != ErrorSafety::Unsafe {
                        ErrorSafety::SafeBounded(1)
                    } else {
                        ErrorSafety::Unsafe
                    }
                }
                BinOp::Add | BinOp::Sub => match (l.safety(true, sys), r.safety(true, sys)) {
                    (ErrorSafety::SafeBounded(a), ErrorSafety::SafeBounded(b)) if a.checked_add(b).is_some_and(|s| s <= MAX_SAFE_BOUND) => {
                        ErrorSafety::SafeBounded(a + b)
                    }
                    _ => ErrorSafety::Unsafe,
                },
                BinOp::Mul => match (l.safety(true, sys), r.safety(true, sys)) {
                    (ErrorSafety::SafeBounded(a), ErrorSafety::SafeBounded(b)) if a.checked_mul(b).is_some_and(|p| p <= MAX_SAFE_BOUND) => {
                        ErrorSafety::SafeBounded(a * b)
                    }
                    (ErrorSafety::SafeBounded(b), ErrorSafety::SafeFinite) | (ErrorSafety::SafeFinite, ErrorSafety::SafeBounded(b)) if b <= 1 => {
                        ErrorSafety::SafeFinite
                    }
                    _ => ErrorSafety::Unsafe,
                },
                _ => ErrorSafety::Unsafe,
            },
        }
    }

    fn eval_row(&self, row: usize, sys: DateSystem) -> Value {
        match self {
            SpNode::Scalar(v) => v.clone(),
            SpNode::Col(arr) => {
                let idx = if arr.rows == 1 { 0 } else { row };
                arr.data.get(idx).cloned().unwrap_or(Value::Empty)
            }
            SpNode::Unary(op, inner) => {
                let v = inner.eval_row(row, sys);
                map_unary(
                    &v,
                    &|n| match op {
                        UnOp::Neg => Value::number(-n),
                        UnOp::Plus => Value::number(n),
                        UnOp::Percent => Value::number(n / 100.0),
                        UnOp::At | UnOp::Spill | UnOp::Trim(_) => Value::number(n),
                    },
                    *op == UnOp::Plus,
                    sys,
                )
            }
            SpNode::Binary(op, l, r) => {
                let a = l.eval_row(row, sys);
                let b = r.eval_row(row, sys);
                binary_scalar(*op, &a, &b, sys)
            }
        }
    }

    /// Collects the shortest indexed candidate row slice from top-level multiplicative `col = key`
    /// factors. A row absent from that slice is guaranteed to evaluate this factor to `0` (`FALSE`),
    /// so when every argument is error-free (`safety != Unsafe`), non-candidate rows contribute `0`.
    fn collect_candidates<'a>(&'a self, height: usize, best: &mut Option<&'a [u32]>) {
        if best.is_some_and(<[u32]>::is_empty) {
            return;
        }
        match self {
            SpNode::Unary(UnOp::Neg | UnOp::Plus | UnOp::Percent, inner) => {
                inner.collect_candidates(height, best);
            }
            SpNode::Binary(BinOp::Mul, l, r) => {
                l.collect_candidates(height, best);
                r.collect_candidates(height, best);
            }
            SpNode::Binary(BinOp::Eq, l, r) => {
                let pair = match (l.as_ref(), r.as_ref()) {
                    (SpNode::Col(col), SpNode::Scalar(key)) if col.rows == height => Some((col, key)),
                    (SpNode::Scalar(key), SpNode::Col(col)) if col.rows == height => Some((col, key)),
                    _ => None,
                };
                let Some((col, key)) = pair else { return };
                let pos = match key {
                    Value::Text(t) if !t.is_empty() => col.text_positions(t),
                    Value::Number(n) if *n != 0.0 && n.is_finite() => col.criteria_number_positions(*n),
                    Value::Empty | Value::Bool(false) => col.empty_eq_positions(),
                    Value::Text(_) => col.empty_eq_positions(),
                    Value::Number(n) if *n == 0.0 => col.empty_eq_positions(),
                    _ => None,
                };
                if let Some(slice) = pos
                    && best.is_none_or(|prev| slice.len() < prev.len())
                {
                    *best = Some(slice);
                }
            }
            _ => {}
        }
    }
}

/// Pure structural pre-check: whether `e` can be compiled into an [`SpNode`] without calling any
/// arbitrary function or volatile construct (so a fallback to `builtin` never double-evaluates).
fn can_fuse(e: &Expr) -> bool {
    match e {
        Expr::Paren(inner) => can_fuse(inner),
        Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Missing => true,
        Expr::Ref(_) | Expr::Struct(_) => true,
        Expr::Unary(UnOp::Neg | UnOp::Plus | UnOp::Percent, inner) => can_fuse(inner),
        Expr::Binary(
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge,
            l,
            r,
        ) => can_fuse(l) && can_fuse(r),
        _ => false,
    }
}

/// Whether `e` contains a range or structured reference that can yield a column array.
fn has_range_ref(e: &Expr) -> bool {
    match e {
        Expr::Paren(inner) | Expr::Unary(_, inner) => has_range_ref(inner),
        Expr::Binary(_, l, r) => has_range_ref(l) || has_range_ref(r),
        Expr::Ref(r) => !r.range().is_single(),
        Expr::Struct(_) => true,
        _ => false,
    }
}

impl Evaluator<'_> {
    /// Fused zero-allocation and indexed candidate-row fast path for `SUMPRODUCT` over 1-column
    /// arrays (`>= 64` rows). Falls back to [`Evaluator::builtin`] for 2D arrays, mismatched
    /// inner broadcast dimensions, small ranges, or arbitrary function subexpressions.
    pub(super) fn sumproduct_call(&mut self, args: &[Expr]) -> Ev {
        if args.is_empty() || !args.iter().all(can_fuse) || !args.iter().any(has_range_ref) {
            return self.builtin("SUMPRODUCT", args);
        }
        let mut compiled = Vec::with_capacity(args.len());
        for a in args {
            let Some(arg) = self.compile_sp_node(a) else {
                return self.builtin("SUMPRODUCT", args);
            };
            compiled.push(arg);
        }
        let Some(first) = compiled.first() else {
            return err(CellError::Value);
        };
        let height = first.rows;
        if compiled.iter().any(|a| a.rows != height) {
            return err(CellError::Value);
        }
        if height < MIN_FUSED_ROWS {
            return self.builtin("SUMPRODUCT", args);
        }
        let work = (args.len() as u64).saturating_mul(height as u64);
        if !self.spend(work) {
            return err(CellError::Calc);
        }
        let sys = self.host.workbook().date_system;
        let all_error_free = compiled.iter().all(|a| a.node.safety(false, sys) != ErrorSafety::Unsafe);
        let mut candidates: Option<&[u32]> = None;
        if all_error_free {
            for a in &compiled {
                a.node.collect_candidates(height, &mut candidates);
            }
        }
        let mut total = 0.0;
        if let Some(rows) = candidates {
            for &r in rows {
                match eval_sumproduct_row(&compiled, r as usize, sys) {
                    Ok(p) => total += p,
                    Err(e) => return err(e),
                }
            }
        } else {
            for r in 0..height {
                match eval_sumproduct_row(&compiled, r, sys) {
                    Ok(p) => total += p,
                    Err(e) => return err(e),
                }
            }
        }
        Ev::V(Value::number(total))
    }

    fn compile_sp_node(&mut self, e: &Expr) -> Option<CompiledArg> {
        match e {
            Expr::Paren(inner) => self.compile_sp_node(inner),
            Expr::Number(n) => Some(CompiledArg { node: SpNode::Scalar(Value::number(*n)), rows: 1 }),
            Expr::Text(t) => Some(CompiledArg { node: SpNode::Scalar(Value::Text(t.clone())), rows: 1 }),
            Expr::Bool(b) => Some(CompiledArg { node: SpNode::Scalar(Value::Bool(*b)), rows: 1 }),
            Expr::Error(x) => Some(CompiledArg { node: SpNode::Scalar(Value::Error(*x)), rows: 1 }),
            Expr::Missing => Some(CompiledArg { node: SpNode::Scalar(Value::Empty), rows: 1 }),
            Expr::Ref(_) | Expr::Struct(_) => {
                let ev = self.eval(e);
                match self.deref(ev) {
                    Value::Array(arr) => {
                        if arr.cols != 1 {
                            return None;
                        }
                        let rows = arr.rows;
                        Some(CompiledArg { node: SpNode::Col(arr), rows })
                    }
                    v => Some(CompiledArg { node: SpNode::Scalar(v), rows: 1 }),
                }
            }
            Expr::Unary(op, inner) => {
                let child = self.compile_sp_node(inner)?;
                Some(CompiledArg { node: SpNode::Unary(*op, Box::new(child.node)), rows: child.rows })
            }
            Expr::Binary(op, l, r) => {
                let left = self.compile_sp_node(l)?;
                let right = self.compile_sp_node(r)?;
                let rows = if left.rows == right.rows {
                    left.rows
                } else if left.rows == 1 {
                    right.rows
                } else if right.rows == 1 {
                    left.rows
                } else {
                    // Mismatched >1 heights pad shorter array with #N/A in Excel broadcast.
                    return None;
                };
                Some(CompiledArg { node: SpNode::Binary(*op, Box::new(left.node), Box::new(right.node)), rows })
            }
            _ => None,
        }
    }
}

fn eval_sumproduct_row(args: &[CompiledArg], row: usize, sys: DateSystem) -> Result<f64, CellError> {
    let mut p = 1.0;
    for arg in args {
        match arg.node.eval_row(row, sys) {
            Value::Number(n) => p *= n,
            Value::Error(e) => return Err(e),
            _ => p = 0.0,
        }
    }
    Ok(p)
}
