use super::*;

/// The aggregates that can fold a reference without building an array.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Agg {
    Sum,
    Count,
    Average,
}

impl Agg {
    pub(super) fn of(name: &str) -> Option<Agg> {
        match name {
            "SUM" => Some(Agg::Sum),
            "COUNT" => Some(Agg::Count),
            "AVERAGE" => Some(Agg::Average),
            _ => None,
        }
    }
}

/// What a folded aggregate carries from one value to the next.
#[derive(Default)]
pub(super) struct Acc {
    pub(super) sum: f64,
    pub(super) count: u64,
}

impl Acc {
    /// Takes a value found inside a reference: only numbers count. `Err` is an error value that
    /// ends `SUM` and `AVERAGE`; `COUNT` skips errors.
    pub(super) fn add_cell(&mut self, kind: Agg, v: &Value) -> Result<(), CellError> {
        match v {
            Value::Number(n) => {
                self.sum += n;
                self.count = self.count.saturating_add(1);
                Ok(())
            }
            Value::Error(e) if kind != Agg::Count => Err(*e),
            _ => Ok(()),
        }
    }

    pub(super) fn finish(&self, kind: Agg) -> Value {
        match kind {
            Agg::Sum => Value::number(self.sum),
            Agg::Count => Value::number(self.count as f64),
            Agg::Average if self.count == 0 => Value::Error(CellError::Div0),
            Agg::Average => Value::number(self.sum / self.count as f64),
        }
    }
}

// ---------------------------------------------------------------- operators

pub(super) fn map_unary(v: &Value, f: &dyn Fn(f64) -> Value, plus: bool, sys: gridcraft_core::DateSystem) -> Value {
    match v {
        Value::Array(a) => Value::from(Array { rows: a.rows, cols: a.cols, data: a.data.iter().map(|x| map_unary(x, f, plus, sys)).collect() }),
        Value::Error(e) => Value::Error(*e),
        Value::Text(_) if plus => v.clone(),
        other => match other.to_number_in(sys) {
            Ok(n) => f(n),
            Err(e) => Value::Error(e),
        },
    }
}

/// Element-wise combination with Excel's broadcasting.
pub fn zip2(a: &Value, b: &Value, f: impl Fn(&Value, &Value) -> Value) -> Value {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            let rows = broadcast_dim(x.rows, y.rows);
            let cols = broadcast_dim(x.cols, y.cols);
            let Some(n) = broadcast_len(rows, cols) else { return Value::Error(CellError::Num) };
            let mut data = Vec::with_capacity(n);
            for r in 0..rows {
                for c in 0..cols {
                    data.push(f(&x.get_broadcast(r, c), &y.get_broadcast(r, c)));
                }
            }
            Value::from(Array { rows, cols, data })
        }
        (Value::Array(x), s) => Value::from(Array { rows: x.rows, cols: x.cols, data: x.data.iter().map(|v| f(v, s)).collect() }),
        (s, Value::Array(y)) => Value::from(Array { rows: y.rows, cols: y.cols, data: y.data.iter().map(|v| f(s, v)).collect() }),
        (x, y) => f(x, y),
    }
}

/// Cell count of a `rows`×`cols` broadcast result, or `None` when it would exceed [`MAX_CELLS`].
/// Each operand passes its own size cap, but their broadcast product can be far larger
/// (`SEQUENCE(1048576)*SEQUENCE(1,16384)` is 17 billion cells), so it is checked before allocating.
pub(super) fn broadcast_len(rows: usize, cols: usize) -> Option<usize> {
    rows.checked_mul(cols).filter(|&n| n as u64 <= MAX_CELLS)
}

fn broadcast_dim(a: usize, b: usize) -> usize {
    if a == 1 {
        b
    } else if b == 1 {
        a
    } else {
        a.max(b)
    }
}

pub(super) fn zip3(c: &Array, t: &Value, f: &Value, g: impl Fn(&Value, &Value, &Value) -> Value) -> Value {
    let dims = |v: &Value| if let Value::Array(a) = v { (a.rows, a.cols) } else { (1, 1) };
    let (tr, tc) = dims(t);
    let (fr, fc) = dims(f);
    let rows = broadcast_dim(broadcast_dim(c.rows, tr), fr);
    let cols = broadcast_dim(broadcast_dim(c.cols, tc), fc);
    let Some(n) = broadcast_len(rows, cols) else { return Value::Error(CellError::Num) };
    let get = |v: &Value, r: usize, col: usize| if let Value::Array(a) = v { a.get_broadcast(r, col) } else { v.clone() };
    let mut data = Vec::with_capacity(n);
    for r in 0..rows {
        for col in 0..cols {
            data.push(g(&c.get_broadcast(r, col), &get(t, r, col), &get(f, r, col)));
        }
    }
    Value::from(Array { rows, cols, data })
}

/// One cell of a LAMBDA helper's result, or `None` for an array of more than one value: arrays
/// can't nest, so the whole helper gives `#CALC!`, as in Excel.
pub(super) fn single(v: Value) -> Option<Value> {
    match v {
        Value::Array(a) if a.rows * a.cols > 1 => None,
        v => Some(v.scalar()),
    }
}

pub fn binary_values(op: BinOp, x: &Value, y: &Value, sys: gridcraft_core::DateSystem) -> Value {
    zip2(x, y, |a, b| binary_scalar(op, a, b, sys))
}

fn binary_scalar(op: BinOp, a: &Value, b: &Value, sys: gridcraft_core::DateSystem) -> Value {
    if let Value::Error(e) = a {
        return Value::Error(*e);
    }
    if let Value::Error(e) = b {
        return Value::Error(*e);
    }
    let num = |f: fn(f64, f64) -> Value| match (a.to_number_in(sys), b.to_number_in(sys)) {
        (Ok(x), Ok(y)) => f(x, y),
        (Err(e), _) | (_, Err(e)) => Value::Error(e),
    };
    use std::cmp::Ordering::*;
    match op {
        BinOp::Add => num(|x, y| Value::number(x + y)),
        BinOp::Sub => num(|x, y| Value::number(x - y)),
        BinOp::Mul => num(|x, y| Value::number(x * y)),
        BinOp::Div => num(|x, y| if y == 0.0 { Value::Error(CellError::Div0) } else { Value::number(x / y) }),
        BinOp::Pow => num(|x, y| {
            if x == 0.0 && y == 0.0 {
                Value::Error(CellError::Num)
            } else if x == 0.0 && y < 0.0 {
                Value::Error(CellError::Div0)
            } else {
                Value::number(x.powf(y))
            }
        }),
        BinOp::Concat => match (a.to_text(), b.to_text()) {
            (Ok(x), Ok(y)) => {
                if x.len() + y.len() > 32767 * 4 {
                    Value::Error(CellError::Value)
                } else {
                    Value::text(x + &y)
                }
            }
            (Err(e), _) | (_, Err(e)) => Value::Error(e),
        },
        BinOp::Eq => Value::Bool(compare(a, b) == Equal),
        BinOp::Ne => Value::Bool(compare(a, b) != Equal),
        BinOp::Lt => Value::Bool(compare(a, b) == Less),
        BinOp::Gt => Value::Bool(compare(a, b) == Greater),
        BinOp::Le => Value::Bool(compare(a, b) != Greater),
        BinOp::Ge => Value::Bool(compare(a, b) != Less),
        BinOp::Range | BinOp::Union | BinOp::Intersect => Value::Error(CellError::Value),
    }
}
