use super::MAX_LAMBDA_CALLS;
use super::array::{broadcast_len, single};
use super::special::is_special;
use super::{Array, CellError, Ev, Evaluator, Expr, Lambda, MAX_CELLS, MAX_TAIL_CALLS, Tail, Value, err};
use std::sync::Arc;

impl<'h> Evaluator<'h> {
    pub fn apply_lambda(&mut self, l: &Lambda, args: &[Expr]) -> Ev {
        match self.bind_args(l, args) {
            Ok(bound) => self.call_lambda_values(l, bound),
            Err(e) => e,
        }
    }

    /// The arguments of a call to `l`, evaluated in the current scope.
    fn bind_args(&mut self, l: &Lambda, args: &[Expr]) -> Result<Vec<(String, Ev)>, Ev> {
        // Too many or too few argument positions is #VALUE!. An explicitly empty position, as in
        // `(1,)`, is `Expr::Missing` and still counts as a position (it binds as omitted). `f()`
        // parses to no arguments, which for a one-parameter lambda can also be read as one empty
        // position, so that call still binds the parameter as omitted.
        let positions_ok = args.len() == l.params.len() || (args.is_empty() && l.params.len() == 1);
        if !positions_ok {
            return Err(err(CellError::Value));
        }
        let mut bound = Vec::with_capacity(l.params.len());
        for (i, p) in l.params.iter().enumerate() {
            let v = match args.get(i) {
                Some(Expr::Missing) | None => Ev::V(Value::Empty),
                Some(e) => self.eval(e),
            };
            bound.push((p.clone(), v));
        }
        Ok(bound)
    }

    /// Follows `e` through the positions whose value is the value of a LAMBDA call (`IF` branches,
    /// parentheses), and returns the call with its arguments evaluated instead of making it, so
    /// that `call_lambda_values` can loop. Anything else is evaluated here.
    fn tail_step(&mut self, e: &Expr) -> Tail {
        match e {
            Expr::Paren(x) => self.tail_step(x),
            Expr::Call(name, args) if name == "IF" && (2..=3).contains(&args.len()) => {
                let c = self.arg_val(args, 0).unwrap_or_default();
                if let Value::Array(arr) = &c {
                    return Tail::Done(self.if_elementwise(arr, args));
                }
                match c.to_bool() {
                    Ok(true) => self.tail_step_branch(args.get(1)),
                    Ok(false) => match args.get(2) {
                        Some(_) => self.tail_step_branch(args.get(2)),
                        None => Tail::Done(Ev::V(Value::Bool(false))),
                    },
                    Err(x) => Tail::Done(err(x)),
                }
            }
            Expr::Call(name, args) if gridcraft_functions::lookup(name).is_none() && !is_special(name) => match self.name(name) {
                Ev::L(l) => self.tail_call(l, args),
                Ev::V(Value::Error(x)) => Tail::Done(err(x)),
                _ => Tail::Done(err(CellError::Name)),
            },
            Expr::Invoke(callee, args) => match self.eval(callee) {
                Ev::L(l) => self.tail_call(l, args),
                Ev::V(Value::Error(x)) => Tail::Done(err(x)),
                _ => Tail::Done(err(CellError::Value)),
            },
            _ => Tail::Done(self.eval(e)),
        }
    }

    /// An `IF` branch; an omitted one (`IF(c,,1)`) is 0.
    fn tail_step_branch(&mut self, branch: Option<&Expr>) -> Tail {
        match branch {
            Some(Expr::Missing) | None => Tail::Done(Ev::V(Value::Number(0.0))),
            Some(b) => self.tail_step(b),
        }
    }

    fn tail_call(&mut self, l: Arc<Lambda>, args: &[Expr]) -> Tail {
        match self.bind_args(&l, args) {
            Ok(bound) => Tail::Call(l, bound),
            Err(e) => Tail::Done(e),
        }
    }

    pub(super) fn call_lambda_values(&mut self, l: &Lambda, bound: Vec<(String, Ev)>) -> Ev {
        if self.calls >= MAX_LAMBDA_CALLS {
            return err(CellError::Num);
        }
        // #2382: one charge per invocation, so a cell's total work stays bounded however the
        // lambdas nest. Foregrounding the charge here means every path into a body (direct
        // `(2,3)` application, `MAP`/`REDUCE`/`BYROW` helpers, D-function criteria) meters.
        self.fuel = match self.fuel.checked_sub(1) {
            Some(f) => f,
            None => return err(CellError::Calc),
        };
        self.calls += 1;
        let saved = std::mem::replace(&mut self.env, l.env.clone());
        self.env.extend(bound);
        // A call in tail position replaces this one instead of nesting under it, so a tail recursion
        // uses no more native stack at 5,000 deep than at 5. Each link still spends fuel.
        let mut tails = 0;
        let mut step = self.tail_step(&l.body);
        let r = loop {
            match step {
                Tail::Done(v) => break v,
                Tail::Call(next, bound) => {
                    tails += 1;
                    if tails >= MAX_TAIL_CALLS {
                        break err(CellError::Num);
                    }
                    self.fuel = match self.fuel.checked_sub(1) {
                        Some(f) => f,
                        None => break err(CellError::Calc),
                    };
                    self.env.clear();
                    self.env.extend(next.env.iter().cloned());
                    self.env.extend(bound);
                    step = self.tail_step(&next.body);
                }
            }
        };
        self.env = saved;
        self.calls -= 1;
        r
    }

    pub(super) fn let_(&mut self, args: &[Expr]) -> Ev {
        if args.len() < 3 || args.len().is_multiple_of(2) {
            return err(CellError::Value);
        }
        let base = self.env.len();
        for pair in args[..args.len() - 1].chunks(2) {
            let [Expr::Name(n), v] = pair else {
                self.env.truncate(base);
                return err(CellError::Value);
            };
            let val = self.eval(v);
            self.env.push((n.clone(), val));
        }
        let r = match args.last() {
            Some(body) => self.eval(body),
            None => err(CellError::Value),
        };
        self.env.truncate(base);
        r
    }

    /// The function argument of a LAMBDA helper; a function given by name takes `arity`
    /// arguments.
    fn lambda_arg(&mut self, e: Option<&Expr>, arity: usize) -> Option<Arc<Lambda>> {
        if let Some(f) = e.and_then(|x| self.function_by_name(x)) {
            let params: Vec<String> = (0..arity).map(|i| format!("_xleta.{i}")).collect();
            let body = Expr::Call(f, params.iter().map(|p| Expr::Name(p.clone())).collect());
            return Some(Arc::new(Lambda { params, body, env: Vec::new() }));
        }
        match e.map(|x| self.eval(x)) {
            Some(Ev::L(l)) => Some(l),
            _ => None,
        }
    }

    pub(super) fn higher_order(&mut self, name: &str, args: &[Expr]) -> Ev {
        let as_array = |v: Value| -> Arc<Array> {
            match v {
                Value::Array(a) => a,
                other => Arc::new(Array::scalar(other)),
            }
        };
        match name {
            "MAP" => {
                if args.len() < 2 {
                    return err(CellError::Value);
                }
                let Some(l) = self.lambda_arg(args.last(), args.len() - 1) else { return err(CellError::Value) };
                let arrays: Vec<Arc<Array>> = args[..args.len() - 1].iter().map(|a| as_array(self.value(a))).collect();
                let rows = arrays.iter().map(|a| a.rows).max().unwrap_or(1);
                let cols = arrays.iter().map(|a| a.cols).max().unwrap_or(1);
                let Some(n) = broadcast_len(rows, cols) else { return err(CellError::Num) };
                // The LAMBDA takes one parameter per array.
                if l.params.len() != arrays.len() {
                    return err(CellError::Value);
                }
                let mut out = Vec::with_capacity(n);
                for r in 0..rows {
                    for c in 0..cols {
                        let bound = l.params.iter().zip(arrays.iter()).map(|(p, a)| (p.clone(), Ev::V(a.get_broadcast(r, c)))).collect();
                        let v = self.call_lambda_values(&l, bound);
                        let Some(one) = single(self.deref(v)) else { return err(CellError::Calc) };
                        out.push(one);
                    }
                }
                Ev::V(Array::new(rows, cols, out).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
            }
            "REDUCE" | "SCAN" => {
                if args.len() != 3 {
                    return err(CellError::Value);
                }
                let mut acc = self.value(&args[0]);
                let arr = as_array(self.value(&args[1]));
                let Some(l) = self.lambda_arg(args.get(2), 2) else { return err(CellError::Value) };
                if l.params.len() != 2 {
                    return err(CellError::Value);
                }
                let mut scan = Vec::with_capacity(arr.data.len());
                for v in arr.data.iter() {
                    let bound = vec![(l.params[0].clone(), Ev::V(acc.clone())), (l.params[1].clone(), Ev::V(v.clone()))];
                    let r = self.call_lambda_values(&l, bound);
                    acc = self.deref(r);
                    if name == "SCAN" {
                        let Some(one) = single(acc.clone()) else { return err(CellError::Calc) };
                        scan.push(one);
                    }
                }
                if name == "SCAN" {
                    Ev::V(Array::new(arr.rows, arr.cols, scan).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
                } else {
                    Ev::V(acc)
                }
            }
            "BYROW" | "BYCOL" => {
                if args.len() != 2 {
                    return err(CellError::Value);
                }
                let arr = as_array(self.value(&args[0]));
                let Some(l) = self.lambda_arg(args.get(1), 1) else { return err(CellError::Value) };
                let Some(p) = l.params.first().cloned() else { return err(CellError::Value) };
                if l.params.len() != 1 {
                    return err(CellError::Value);
                }
                let by_row = name == "BYROW";
                let n = if by_row { arr.rows } else { arr.cols };
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let slice: Vec<Value> = if by_row {
                        (0..arr.cols).map(|c| arr.get(i, c).cloned().unwrap_or_default()).collect()
                    } else {
                        (0..arr.rows).map(|r| arr.get(r, i).cloned().unwrap_or_default()).collect()
                    };
                    let a = if by_row { Array::row(slice) } else { Array::column(slice) };
                    let r = self.call_lambda_values(&l, vec![(p.clone(), Ev::V(a.into()))]);
                    let Some(one) = single(self.deref(r)) else { return err(CellError::Calc) };
                    out.push(one);
                }
                let a = if by_row { Array::column(out) } else { Array::row(out) };
                Ev::V(a.into())
            }
            "MAKEARRAY" => {
                if args.len() != 3 {
                    return err(CellError::Value);
                }
                let (rows, cols) = match (self.arg_num(args, 0, 0.0), self.arg_num(args, 1, 0.0)) {
                    (Ok(r), Ok(c)) => (r.trunc(), c.trunc()),
                    (Err(e), _) | (_, Err(e)) => return err(e),
                };
                if rows < 1.0 || cols < 1.0 || rows * cols > MAX_CELLS as f64 {
                    return err(CellError::Value);
                }
                let Some(l) = self.lambda_arg(args.get(2), 2) else { return err(CellError::Value) };
                if l.params.len() != 2 {
                    return err(CellError::Value);
                }
                let (rows, cols) = (rows as usize, cols as usize);
                let mut out = Vec::with_capacity(rows * cols);
                for r in 0..rows {
                    for c in 0..cols {
                        let bound = vec![
                            (l.params[0].clone(), Ev::V(Value::number(r as f64 + 1.0))),
                            (l.params[1].clone(), Ev::V(Value::number(c as f64 + 1.0))),
                        ];
                        let v = self.call_lambda_values(&l, bound);
                        let Some(one) = single(self.deref(v)) else { return err(CellError::Calc) };
                        out.push(one);
                    }
                }
                Ev::V(Array::new(rows, cols, out).map(Value::from).unwrap_or(Value::Error(CellError::Calc)))
            }
            _ => err(CellError::Name),
        }
    }
}
