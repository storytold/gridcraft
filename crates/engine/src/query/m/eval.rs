//! Evaluating M: environments (`let` and record fields are evaluated on first use, so unused
//! steps never run and steps may refer to later ones), operators, function calls, field and item
//! access, `try`, types, and names of other queries.
//!
//! Recursion is bounded ([`MAX_DEPTH`]) and so is the total work ([`FUEL`]): a hostile or
//! runaway formula ends in an error, never a crash or a hang.

use std::cell::RefCell;
use std::rc::Rc;

use gridcraft_core::DateSystem;

use super::super::EvalCx;
use super::ast::{BinOp, Expr, ListItem, TypeExpr, UnOp};
use super::library;
use super::value::{LazyState, MError, MFunc, MRecord, MResult, MTable, MType, MValue, equal, order};

/// Deepest nesting of evaluation (expressions inside expressions, calls inside calls).
pub const MAX_DEPTH: usize = 150;
/// Most expression evaluations one refresh may take.
pub const FUEL: u64 = 2_000_000_000;
/// Most items a list may hold.
pub const MAX_LIST: usize = super::super::MAX_ROWS;
/// The reason of errors that end the whole evaluation (limits), which `try` doesn't catch.
pub const LIMIT: &str = "Evaluation.Limit";

pub fn limit(msg: impl Into<String>) -> MError {
    MError::new(LIMIT, msg)
}

pub fn is_limit(e: &MError) -> bool {
    e.reason == LIMIT
}

/// A scope: names bound by a `let`, a record or a function call.
pub struct Env {
    names: Vec<Rc<str>>,
    slots: Vec<RefCell<Slot>>,
    parent: Option<Rc<Env>>,
}

enum Slot {
    Pending(Rc<Expr>),
    Running,
    Done(MResult<MValue>),
}

impl Env {
    fn values(parent: Option<Rc<Env>>, vars: Vec<(Rc<str>, MValue)>) -> Rc<Env> {
        let (names, slots) = vars.into_iter().map(|(n, v)| (n, RefCell::new(Slot::Done(Ok(v))))).unzip();
        Rc::new(Env { names, slots, parent })
    }
    fn pending(parent: Option<Rc<Env>>, binds: &[(Rc<str>, Rc<Expr>)]) -> Rc<Env> {
        let (names, slots) = binds.iter().map(|(n, e)| (n.clone(), RefCell::new(Slot::Pending(e.clone())))).unzip();
        Rc::new(Env { names, slots, parent })
    }
}

enum QState {
    Running,
    Done(MResult<MValue>),
}

/// The evaluator of one refresh: the evaluation context (workbook, data access, cache), the
/// values of queries already worked out, and the limits.
pub struct Ev<'e, 'a> {
    pub cx: &'e mut EvalCx<'a>,
    pub sys: DateSystem,
    depth: usize,
    fuel: u64,
    queries: Vec<(String, QState)>,
}

impl<'e, 'a> Ev<'e, 'a> {
    pub fn new(cx: &'e mut EvalCx<'a>) -> Ev<'e, 'a> {
        let sys = cx.wb.date_system;
        Ev { cx, sys, depth: 0, fuel: FUEL, queries: vec![] }
    }

    /// Spends work; errors once the budget is gone.
    pub fn tick(&mut self, n: u64) -> MResult<()> {
        match self.fuel.checked_sub(n) {
            Some(f) => {
                self.fuel = f;
                Ok(())
            }
            None => {
                self.fuel = 0;
                Err(limit("The query does too much work to finish (it may loop without end)."))
            }
        }
    }

    /// The value of the Excel query `name` (its M formula), worked out once per refresh.
    pub fn query(&mut self, name: &str) -> MResult<MValue> {
        let wb = self.cx.wb;
        let Some(q) =
            wb.excel_queries.iter().find(|q| q.name == name).or_else(|| wb.excel_queries.iter().find(|q| q.name.eq_ignore_ascii_case(name)))
        else {
            return Err(MError::expr(format!("The query '{name}' wasn't found.")));
        };
        if let Some((_, st)) = self.queries.iter().find(|(n, _)| *n == q.name) {
            return match st {
                QState::Running => {
                    Err(MError::expr(format!("A cyclic reference was encountered during evaluation: the query '{}' refers to itself.", q.name)))
                }
                QState::Done(r) => r.clone(),
            };
        }
        if self.queries.len() >= super::super::MAX_QUERY_DEPTH * 8 {
            return Err(limit("Too many queries refer to each other."));
        }
        self.queries.push((q.name.clone(), QState::Running));
        let r = super::parser::parse(&q.formula).and_then(|e| self.eval(&e, &None)).and_then(|v| self.force(v));
        if let Some(slot) = self.queries.iter_mut().find(|(n, _)| *n == q.name) {
            slot.1 = QState::Done(r.clone());
        }
        r
    }

    /// `v` with a lazy value worked out and a held error raised.
    pub fn force(&mut self, mut v: MValue) -> MResult<MValue> {
        for _ in 0..64 {
            match v {
                MValue::Error(e) => return Err((*e).clone()),
                MValue::Lazy(l) => {
                    let state = std::mem::replace(&mut *l.state.borrow_mut(), LazyState::Running);
                    let r = match state {
                        LazyState::Done(r) => r,
                        LazyState::Running => Err(MError::expr("A cyclic reference was encountered during evaluation.")),
                        LazyState::Pending(f) => f(self),
                    };
                    *l.state.borrow_mut() = LazyState::Done(r.clone());
                    v = r?;
                }
                other => return Ok(other),
            }
        }
        Err(MError::expr("A value refers to itself."))
    }

    /// Works out an expression (the result is never a held error or a lazy value).
    pub fn eval(&mut self, e: &Expr, env: &Option<Rc<Env>>) -> MResult<MValue> {
        self.tick(1)?;
        self.depth += 1;
        let r = if self.depth > MAX_DEPTH {
            Err(limit("The formula nests too deeply (or a function calls itself too often) to evaluate."))
        } else {
            self.eval_inner(e, env).and_then(|v| self.force(v))
        };
        self.depth -= 1;
        r
    }

    fn eval_inner(&mut self, e: &Expr, env: &Option<Rc<Env>>) -> MResult<MValue> {
        match e {
            Expr::Null => Ok(MValue::Null),
            Expr::Bool(b) => Ok(MValue::Logical(*b)),
            Expr::Num(n) => Ok(MValue::Number(*n)),
            Expr::Text(t) => Ok(MValue::Text(t.clone())),
            Expr::Ident(name) => self.lookup(name, env),
            Expr::List(items) => {
                let mut out = Vec::new();
                for it in items {
                    match it {
                        ListItem::One(x) => out.push(self.held(x, env)?),
                        ListItem::Range(a, b) => {
                            let (a, b) = (self.eval(a, env)?, self.eval(b, env)?);
                            // A range of characters: {"a".."z"}.
                            if let (MValue::Text(x), MValue::Text(y)) = (&a, &b) {
                                let one = |t: &str| {
                                    let mut cs = t.chars();
                                    match (cs.next(), cs.next()) {
                                        (Some(c), None) => Some(u32::from(c)),
                                        _ => None,
                                    }
                                };
                                let (Some(x), Some(y)) = (one(x), one(y)) else {
                                    return Err(MError::expr("A range of characters needs single characters at both ends."));
                                };
                                if y >= x {
                                    let count = (y - x + 1) as usize;
                                    if count > MAX_LIST.saturating_sub(out.len()) {
                                        return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
                                    }
                                    self.tick(count as u64)?;
                                    out.extend((x..=y).filter_map(char::from_u32).map(|c| MValue::text(c.to_string())));
                                }
                                continue;
                            }
                            let (MValue::Number(a), MValue::Number(b)) = (&a, &b) else {
                                return Err(MError::expr(format!("A list range needs numbers, not {} and {}.", a.kind(), b.kind())));
                            };
                            if a.fract() != 0.0 || b.fract() != 0.0 || !a.is_finite() || !b.is_finite() {
                                return Err(MError::expr("A list range needs whole numbers."));
                            }
                            if b >= a {
                                let count = b - a + 1.0;
                                if count > (MAX_LIST.saturating_sub(out.len())) as f64 {
                                    return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
                                }
                                self.tick(count as u64)?;
                                let mut x = *a;
                                while x <= *b {
                                    out.push(MValue::Number(x));
                                    x += 1.0;
                                }
                            }
                        }
                    }
                    if out.len() > MAX_LIST {
                        return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
                    }
                }
                Ok(MValue::list(out))
            }
            Expr::Record(fields) => {
                for (i, (n, _)) in fields.iter().enumerate() {
                    if fields.iter().take(i).any(|(m, _)| m == n) {
                        return Err(MError::expr(format!("The name '{n}' is defined more than once.")));
                    }
                }
                let scope = Env::pending(env.clone(), fields);
                let scope = Some(scope);
                let mut out = Vec::with_capacity(fields.len());
                for (n, _) in fields.iter() {
                    let v = match self.lookup(n, &scope) {
                        Ok(v) => v,
                        Err(e) if is_limit(&e) => return Err(e),
                        Err(e) => MValue::error(e),
                    };
                    out.push((n.clone(), v));
                }
                Ok(MValue::record(MRecord::new(out)))
            }
            Expr::Let(binds, body) => {
                for (i, (n, _)) in binds.iter().enumerate() {
                    if binds.iter().take(i).any(|(m, _)| m == n) {
                        return Err(MError::expr(format!("The variable '{n}' is defined more than once.")));
                    }
                }
                let scope = Some(Env::pending(env.clone(), binds));
                self.eval(body, &scope)
            }
            Expr::If(c, a, b) => match self.eval(c, env)? {
                MValue::Logical(true) => self.eval(a, env),
                MValue::Logical(false) => self.eval(b, env),
                other => Err(not_logical(&other)),
            },
            Expr::Func(def) => Ok(MValue::Function(Rc::new(MFunc::Closure { def: def.clone(), env: env.clone() }))),
            Expr::Call(f, args) => {
                let fv = self.eval(f, env)?;
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(a, env)?);
                }
                self.call(&fv, vals)
            }
            Expr::Field { target, name, optional } => {
                let t = match target {
                    Some(t) => self.eval(t, env)?,
                    None => self.lookup("_", env).map_err(|_| MError::expr(format!("[{name}] is used outside 'each' (there is no current row).")))?,
                };
                self.field(&t, name, *optional)
            }
            Expr::Project { target, names, optional } => {
                let t = match target {
                    Some(t) => self.eval(t, env)?,
                    None => self.lookup("_", env)?,
                };
                self.project(&t, names, *optional)
            }
            Expr::Item { target, key, optional } => {
                let t = self.eval(target, env)?;
                let k = self.eval(key, env)?;
                self.item(&t, &k, *optional)
            }
            Expr::Unary(op, x) => {
                let v = self.eval(x, env)?;
                unary(*op, v)
            }
            Expr::Binary(BinOp::And, l, r) => {
                let a = self.eval(l, env)?;
                match a {
                    MValue::Logical(false) => Ok(MValue::Logical(false)),
                    MValue::Logical(true) | MValue::Null => {
                        let b = self.eval(r, env)?;
                        match (&a, &b) {
                            (_, MValue::Logical(false)) => Ok(MValue::Logical(false)),
                            (MValue::Logical(true), MValue::Logical(true)) => Ok(MValue::Logical(true)),
                            (_, MValue::Logical(true) | MValue::Null) => Ok(MValue::Null),
                            _ => Err(not_logical(&b)),
                        }
                    }
                    other => Err(not_logical(&other)),
                }
            }
            Expr::Binary(BinOp::Or, l, r) => {
                let a = self.eval(l, env)?;
                match a {
                    MValue::Logical(true) => Ok(MValue::Logical(true)),
                    MValue::Logical(false) | MValue::Null => {
                        let b = self.eval(r, env)?;
                        match (&a, &b) {
                            (_, MValue::Logical(true)) => Ok(MValue::Logical(true)),
                            (MValue::Logical(false), MValue::Logical(false)) => Ok(MValue::Logical(false)),
                            (_, MValue::Logical(false) | MValue::Null) => Ok(MValue::Null),
                            _ => Err(not_logical(&b)),
                        }
                    }
                    other => Err(not_logical(&other)),
                }
            }
            Expr::Binary(BinOp::Coalesce, l, r) => match self.eval(l, env)? {
                MValue::Null => self.eval(r, env),
                v => Ok(v),
            },
            Expr::Binary(op, l, r) => {
                let a = self.eval(l, env)?;
                let b = self.eval(r, env)?;
                self.binary(*op, a, b)
            }
            Expr::Try { body, otherwise, catch } => {
                let r = self.eval(body, env);
                match r {
                    Err(e) if is_limit(&e) => Err(e),
                    Err(e) => match (otherwise, catch) {
                        (Some(o), _) => self.eval(o, env),
                        (None, Some(f)) => {
                            let fv = MValue::Function(Rc::new(MFunc::Closure { def: f.clone(), env: env.clone() }));
                            let args = if f.params.is_empty() { vec![] } else { vec![error_record(&e)] };
                            self.call(&fv, args)
                        }
                        (None, None) => {
                            Ok(MValue::record(MRecord::new(vec![("HasError".into(), MValue::Logical(true)), ("Error".into(), error_record(&e))])))
                        }
                    },
                    Ok(v) => match (otherwise, catch) {
                        (None, None) => Ok(MValue::record(MRecord::new(vec![("HasError".into(), MValue::Logical(false)), ("Value".into(), v)]))),
                        _ => Ok(v),
                    },
                }
            }
            Expr::Error(x) => {
                let v = self.eval(x, env)?;
                Err(raised(&v))
            }
            Expr::Meta(a, _) => self.eval(a, env),
            Expr::Type(t) => Ok(MValue::Type(Rc::new(self.eval_type(t, env)?))),
            Expr::Is(x, t) => {
                let v = self.eval(x, env)?;
                let t = self.eval_type(t, env)?;
                Ok(MValue::Logical(conforms(&v, &t)))
            }
            Expr::As(x, t) => {
                let v = self.eval(x, env)?;
                let t = self.eval_type(t, env)?;
                if conforms(&v, &t) {
                    Ok(v)
                } else {
                    Err(MError::expr(format!("We cannot convert a value of type {} to type {}.", v.kind(), t.kind)))
                }
            }
            Expr::NotImplemented => Err(MError::new("Expression.Error", "Not Implemented")),
        }
    }

    /// An expression's value to hold in a list: an error is kept in place (raised on use).
    fn held(&mut self, e: &Expr, env: &Option<Rc<Env>>) -> MResult<MValue> {
        match self.eval(e, env) {
            Ok(v) => Ok(v),
            Err(e) if is_limit(&e) => Err(e),
            Err(e) => Ok(MValue::error(e)),
        }
    }

    fn lookup(&mut self, name: &str, env: &Option<Rc<Env>>) -> MResult<MValue> {
        let mut scope = env.clone();
        while let Some(s) = scope {
            if let Some(i) = s.names.iter().position(|n| &**n == name) {
                let Some(cell) = s.slots.get(i) else { break };
                let pending = {
                    let mut slot = cell.borrow_mut();
                    match &*slot {
                        Slot::Done(r) => return r.clone(),
                        Slot::Running => return Err(MError::expr(format!("A cyclic reference was encountered during evaluation of '{name}'."))),
                        Slot::Pending(e) => {
                            let e = e.clone();
                            *slot = Slot::Running;
                            e
                        }
                    }
                };
                let r = self.eval(&pending, &Some(s.clone()));
                if let Some(cell) = s.slots.get(i) {
                    *cell.borrow_mut() = Slot::Done(r.clone());
                }
                return r;
            }
            scope = s.parent.clone();
        }
        if self.cx.wb.excel_queries.iter().any(|q| q.name == name) {
            return self.query(name);
        }
        if let Some(q) = self.cx.find(name) {
            let t = self.cx.evaluate(q, None).map_err(MError::source)?;
            return Ok(MValue::table(super::from_qtable(&t, self.sys)));
        }
        if let Some(v) = library::global(name) {
            return Ok(v);
        }
        if self.cx.wb.excel_queries.iter().any(|q| q.name.eq_ignore_ascii_case(name)) {
            return self.query(name);
        }
        Err(unknown_name(name))
    }

    /// Calls a function value.
    pub fn call(&mut self, f: &MValue, args: Vec<MValue>) -> MResult<MValue> {
        let MValue::Function(func) = f else {
            return Err(MError::expr(format!("We cannot convert a value of type {} to type Function.", f.kind())));
        };
        match &**func {
            MFunc::Closure { def, env } => {
                let required = def.params.iter().filter(|p| !p.optional).count();
                let max = def.params.len();
                if args.len() < required || args.len() > max {
                    return Err(MError::expr(format!(
                        "{} arguments were passed to a function which expects {}.",
                        args.len(),
                        if required == max { format!("{max}") } else { format!("between {required} and {max}") }
                    )));
                }
                let mut vars = Vec::with_capacity(max);
                let mut args = args.into_iter();
                for p in &def.params {
                    vars.push((p.name.clone(), args.next().unwrap_or(MValue::Null)));
                }
                let scope = Some(Env::values(env.clone(), vars));
                self.eval(&def.body, &scope)
            }
            MFunc::Builtin { name, bound } => {
                self.depth += 1;
                let r = if self.depth > MAX_DEPTH {
                    Err(limit("Functions call each other too deeply to evaluate."))
                } else {
                    library::call(self, name, bound, args).and_then(|v| self.force(v))
                };
                self.depth -= 1;
                r
            }
        }
    }

    /// Calls `f` with one argument (the `each` functions of table operations).
    pub fn call1(&mut self, f: &MValue, a: MValue) -> MResult<MValue> {
        self.call(f, vec![a])
    }

    pub fn field(&mut self, t: &MValue, name: &str, optional: bool) -> MResult<MValue> {
        match t {
            MValue::Record(r) => match r.get(name) {
                Some(v) => self.force(v.clone()),
                None if optional => Ok(MValue::Null),
                None => Err(MError::expr(format!("The field '{name}' of the record wasn't found."))),
            },
            MValue::Table(t) => match t.names.iter().position(|n| &**n == name) {
                Some(i) => Ok(MValue::list(t.rows.iter().map(|r| r.get(i).cloned().unwrap_or(MValue::Null)).collect())),
                None if optional => Ok(MValue::Null),
                None => Err(MError::expr(format!("The column '{name}' of the table wasn't found."))),
            },
            MValue::Null if optional => Ok(MValue::Null),
            other => Err(MError::expr(format!("We cannot apply field access to the type {}.", other.kind()))),
        }
    }

    fn project(&mut self, t: &MValue, names: &[Rc<str>], optional: bool) -> MResult<MValue> {
        match t {
            MValue::Record(r) => {
                let mut out = Vec::new();
                for n in names {
                    match r.get(n) {
                        Some(v) => out.push((n.clone(), v.clone())),
                        None if optional => out.push((n.clone(), MValue::Null)),
                        None => return Err(MError::expr(format!("The field '{n}' of the record wasn't found."))),
                    }
                }
                Ok(MValue::record(MRecord::new(out)))
            }
            MValue::Table(t) => {
                let cols: Vec<Option<usize>> = names.iter().map(|n| t.names.iter().position(|c| c == n)).collect();
                if !optional && let Some((n, _)) = names.iter().zip(&cols).find(|(_, c)| c.is_none()) {
                    return Err(MError::expr(format!("The column '{n}' of the table wasn't found.")));
                }
                let mut out = MTable::new(names.to_vec());
                for (j, c) in cols.iter().enumerate() {
                    if let (Some(i), Some(slot)) = (c, out.types.get_mut(j)) {
                        *slot = t.types.get(*i).copied().unwrap_or_default();
                    }
                }
                for r in &t.rows {
                    out.rows.push(cols.iter().map(|c| c.and_then(|i| r.get(i).cloned()).unwrap_or(MValue::Null)).collect());
                }
                Ok(MValue::table(out))
            }
            other => Err(MError::expr(format!("We cannot apply field access to the type {}.", other.kind()))),
        }
    }

    pub fn item(&mut self, t: &MValue, k: &MValue, optional: bool) -> MResult<MValue> {
        let too_few = || MError::expr("There weren't enough elements in the enumeration to complete the operation.");
        match (t, k) {
            (MValue::List(l), MValue::Number(n)) => {
                let i = index(*n)?;
                match l.get(i) {
                    Some(v) => self.force(v.clone()),
                    None if optional => Ok(MValue::Null),
                    None => Err(too_few()),
                }
            }
            (MValue::Table(t), MValue::Number(n)) => {
                let i = index(*n)?;
                match t.rows.get(i) {
                    Some(r) => Ok(MValue::record(t.row_record(r))),
                    None if optional => Ok(MValue::Null),
                    None => Err(too_few()),
                }
            }
            (MValue::Table(t), MValue::Record(key)) => {
                let mut cols = Vec::with_capacity(key.names.len());
                for n in key.names.iter() {
                    cols.push(t.col(n)?);
                }
                let mut found: Option<usize> = None;
                for (ri, r) in t.rows.iter().enumerate() {
                    let mut hit = true;
                    for (c, kv) in cols.iter().zip(key.values.iter()) {
                        let v = self.force(r.get(*c).cloned().unwrap_or(MValue::Null))?;
                        let kv = self.force(kv.clone())?;
                        if !equal(&v, &kv) {
                            hit = false;
                            break;
                        }
                    }
                    if hit {
                        if found.is_some() {
                            return Err(MError::expr("The key matched more than one row in the table."));
                        }
                        found = Some(ri);
                    }
                }
                match found.and_then(|i| t.rows.get(i)) {
                    Some(r) => Ok(MValue::record(t.row_record(r))),
                    None if optional => Ok(MValue::Null),
                    None => Err(MError::expr(format!("The key didn't match any rows in the table. Key: {}", key_text(key)))),
                }
            }
            (MValue::Null, _) if optional => Ok(MValue::Null),
            (t, k) => Err(MError::expr(format!("We cannot apply an item selector {{{}}} to a value of type {}.", k.kind(), t.kind()))),
        }
    }

    pub fn binary(&mut self, op: BinOp, a: MValue, b: MValue) -> MResult<MValue> {
        use MValue as V;
        let cant = |a: &V, b: &V| {
            let sym = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::Concat => "&",
                BinOp::Lt => "<",
                BinOp::Le => "<=",
                BinOp::Gt => ">",
                BinOp::Ge => ">=",
                _ => "?",
            };
            MError::expr(format!("We cannot apply operator {sym} to types {} and {}.", title(a.kind()), title(b.kind())))
        };
        match op {
            BinOp::Eq => return Ok(V::Logical(equal(&a, &b))),
            BinOp::Ne => return Ok(V::Logical(!equal(&a, &b))),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                if a.is_null() || b.is_null() {
                    return Ok(V::Null);
                }
                let o = order(&a, &b).ok_or_else(|| cant(&a, &b))?;
                return Ok(V::Logical(match op {
                    BinOp::Lt => o.is_lt(),
                    BinOp::Le => o.is_le(),
                    BinOp::Gt => o.is_gt(),
                    _ => o.is_ge(),
                }));
            }
            _ => {}
        }
        if op == BinOp::Concat {
            return match (&a, &b) {
                (V::Null, _) | (_, V::Null) => Ok(V::Null),
                (V::Text(x), V::Text(y)) => {
                    if x.len().saturating_add(y.len()) > super::MAX_TEXT {
                        return Err(limit("The text is too long."));
                    }
                    Ok(V::text(format!("{x}{y}")))
                }
                (V::List(x), V::List(y)) => {
                    if x.len().saturating_add(y.len()) > MAX_LIST {
                        return Err(limit(format!("A list can hold at most {MAX_LIST} items here.")));
                    }
                    Ok(V::list(x.iter().chain(y.iter()).cloned().collect()))
                }
                (V::Record(x), V::Record(y)) => {
                    let mut fields: Vec<(Rc<str>, V)> = x.names.iter().cloned().zip(x.values.iter().cloned()).collect();
                    for (n, v) in y.names.iter().zip(y.values.iter()) {
                        match fields.iter_mut().find(|(m, _)| m == n) {
                            Some(slot) => slot.1 = v.clone(),
                            None => fields.push((n.clone(), v.clone())),
                        }
                    }
                    Ok(V::record(MRecord::new(fields)))
                }
                (V::Table(_), V::Table(_)) => library::table::combine(self, &[a.clone(), b.clone()]),
                (V::Date(d), V::Time(t)) => Ok(V::DateTime(*d as f64 + t)),
                _ => Err(cant(&a, &b)),
            };
        }
        if a.is_null() || b.is_null() {
            return Ok(V::Null);
        }
        let r = match (op, &a, &b) {
            (BinOp::Add, V::Number(x), V::Number(y)) => V::Number(x + y),
            (BinOp::Sub, V::Number(x), V::Number(y)) => V::Number(x - y),
            (BinOp::Mul, V::Number(x), V::Number(y)) => V::Number(x * y),
            (BinOp::Div, V::Number(x), V::Number(y)) => V::Number(x / y),
            (BinOp::Add, V::Date(d), V::Duration(x)) | (BinOp::Add, V::Duration(x), V::Date(d)) => V::Date(add_days(*d, x.floor())?),
            (BinOp::Sub, V::Date(d), V::Duration(x)) => V::Date(add_days(*d, -x.floor())?),
            (BinOp::Add, V::DateTime(d), V::Duration(x)) | (BinOp::Add, V::Duration(x), V::DateTime(d)) => V::DateTime(d + x),
            (BinOp::Sub, V::DateTime(d), V::Duration(x)) => V::DateTime(d - x),
            (BinOp::Add, V::DateTimeZone(d, o), V::Duration(x)) => V::DateTimeZone(d + x, *o),
            (BinOp::Sub, V::DateTimeZone(d, o), V::Duration(x)) => V::DateTimeZone(d - x, *o),
            (BinOp::Add, V::Time(t), V::Duration(x)) => V::Time((t + x).rem_euclid(1.0)),
            (BinOp::Sub, V::Time(t), V::Duration(x)) => V::Time((t - x).rem_euclid(1.0)),
            (BinOp::Sub, V::Date(x), V::Date(y)) => V::Duration((x - y) as f64),
            (BinOp::Sub, V::DateTime(x), V::DateTime(y)) | (BinOp::Sub, V::Time(x), V::Time(y)) => V::Duration(x - y),
            (BinOp::Sub, V::DateTimeZone(x, ox), V::DateTimeZone(y, oy)) => {
                V::Duration((x - f64::from(*ox) / 1440.0) - (y - f64::from(*oy) / 1440.0))
            }
            (BinOp::Add, V::Duration(x), V::Duration(y)) => V::Duration(x + y),
            (BinOp::Sub, V::Duration(x), V::Duration(y)) => V::Duration(x - y),
            (BinOp::Mul, V::Duration(x), V::Number(n)) | (BinOp::Mul, V::Number(n), V::Duration(x)) => V::Duration(x * n),
            (BinOp::Div, V::Duration(x), V::Number(n)) => V::Duration(x / n),
            (BinOp::Div, V::Duration(x), V::Duration(y)) => V::Number(x / y),
            _ => return Err(cant(&a, &b)),
        };
        Ok(r)
    }

    pub fn eval_type(&mut self, t: &TypeExpr, env: &Option<Rc<Env>>) -> MResult<MType> {
        Ok(match t {
            TypeExpr::Prim(k) => MType::prim(k),
            TypeExpr::Nullable(inner) => MType { nullable: true, ..self.eval_type(inner, env)? },
            TypeExpr::List(_) => MType::prim("list"),
            TypeExpr::Record(fields) | TypeExpr::Table(fields) => {
                let mut fs = Vec::with_capacity(fields.len());
                for (n, ft) in fields {
                    fs.push((n.clone(), self.eval_type(ft, env)?));
                }
                MType { kind: if matches!(t, TypeExpr::Table(_)) { "table" } else { "record" }, nullable: false, facet: None, fields: fs }
            }
            TypeExpr::Function => MType::prim("function"),
            TypeExpr::Expr(e) => match self.eval(e, env)? {
                MValue::Type(t) => (*t).clone(),
                other => return Err(MError::expr(format!("We cannot convert a value of type {} to type Type.", other.kind()))),
            },
        })
    }
}

fn index(n: f64) -> MResult<usize> {
    if n < 0.0 || n.fract() != 0.0 || !n.is_finite() {
        return Err(MError::expr(format!("The index {n} isn't a whole number from 0.")));
    }
    Ok(if n > usize::MAX as f64 { usize::MAX } else { n as usize })
}

fn add_days(d: i64, n: f64) -> MResult<i64> {
    if !n.is_finite() || n.abs() > 4_000_000.0 {
        return Err(MError::expr("The date is out of range."));
    }
    let r = d.saturating_add(n as i64);
    super::value::days(i64::from(super::value::ymd(r).0), 1, 1).map(|_| r)
}

fn key_text(r: &MRecord) -> String {
    let parts: Vec<String> = r.names.iter().zip(r.values.iter()).map(|(n, v)| format!("{n}={}", super::library::show(v))).collect();
    format!("[{}]", parts.join(", "))
}

fn title(kind: &str) -> String {
    let mut c = kind.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn not_logical(v: &MValue) -> MError {
    MError::expr(format!("We cannot convert the value {} to type Logical.", super::library::show(v)))
}

fn unary(op: UnOp, v: MValue) -> MResult<MValue> {
    match (op, v) {
        (_, MValue::Null) => Ok(MValue::Null),
        (UnOp::Not, MValue::Logical(b)) => Ok(MValue::Logical(!b)),
        (UnOp::Minus, MValue::Number(n)) => Ok(MValue::Number(-n)),
        (UnOp::Minus, MValue::Duration(n)) => Ok(MValue::Duration(-n)),
        (UnOp::Plus, v @ (MValue::Number(_) | MValue::Duration(_))) => Ok(v),
        (UnOp::Not, v) => Err(not_logical(&v)),
        (_, v) => Err(MError::expr(format!("We cannot apply a sign to a value of type {}.", v.kind()))),
    }
}

/// The record `try` gives for an error (`[Reason, Message, Detail]`).
pub fn error_record(e: &MError) -> MValue {
    MValue::record(MRecord::new(vec![
        ("Reason".into(), MValue::text(&e.reason)),
        ("Message".into(), MValue::text(&e.message)),
        ("Detail".into(), MValue::Null),
    ]))
}

/// The error `error x` raises.
fn raised(v: &MValue) -> MError {
    match v {
        MValue::Text(t) => MError::expr(t.to_string()),
        MValue::Record(r) => {
            let text = |n: &str| match r.get(n) {
                Some(MValue::Text(t)) => Some(t.to_string()),
                _ => None,
            };
            MError { reason: text("Reason").unwrap_or_else(|| "Expression.Error".into()), message: text("Message").unwrap_or_default() }
        }
        other => MError::expr(super::library::show(other)),
    }
}

/// Whether `v` is of type `t` (its primitive kind; nullable types also take null).
pub fn conforms(v: &MValue, t: &MType) -> bool {
    match (t.kind, v) {
        ("any", _) => true,
        ("none", _) => false,
        ("anynonnull", v) => !v.is_null(),
        ("null", v) => v.is_null(),
        (_, MValue::Null) => t.nullable,
        (k, v) => k == v.kind(),
    }
}

fn unknown_name(name: &str) -> MError {
    let module = name.split('.').next().unwrap_or("");
    let library = name.starts_with('#') || (name.contains('.') && library::MODULES.contains(&module));
    if library {
        MError::expr(format!("The M function '{name}' isn't supported by GridCraft yet."))
    } else {
        MError::expr(format!("The name '{name}' wasn't recognized. Make sure it's spelled correctly."))
    }
}
