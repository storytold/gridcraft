//! Legacy (pre-dynamic-array) cell formulas and implicit intersection, without evaluating.
//!
//! Excel evaluates a cell formula saved without dynamic-array metadata (no `cm` attribute) the
//! legacy way: a reference that may cover several cells, used where one value is expected, is
//! intersected with the formula's row or column, and a multi-valued result shows its top-left
//! value. Dynamic-array Excel displays such a formula with the implicit intersection operator `@`
//! inserted where that happens (Microsoft support, "Implicit intersection operator: @"). The
//! reader does the same ([`apply_legacy`]), so the dynamic-array evaluator computes the legacy
//! result; the writer ([`export`]) saves a formula that is the legacy reading of some text as
//! that text, without `@`, and gives every other formula dynamic-array metadata.
//!
//! A range is harmless in SUM, but not in SQRT or arithmetic: `SUM(A1:A3*B1:B3)` intersects both
//! operands. Legacy array-aware parameters such as SUMPRODUCT's evaluate what is inside as
//! arrays, so nothing there is intersected, except IF's condition and IFERROR's and IFNA's
//! arguments ([`Param::Single`]). This is file-format policy, not a second evaluator: unknown
//! results are never calculated.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_formula::{BinOp, Expr, SheetSel, StructItem, UnOp};
use gridcraft_model::{Formula, Workbook};

use crate::FunctionInfo;

/// What an expression produces, as far as implicit intersection is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// One value, or a reference to one cell.
    Scalar,
    /// A reference that may cover several cells.
    Ref,
    /// Several values that are not a reference.
    Array,
}

impl Kind {
    fn join(self, other: Kind) -> Kind {
        match (self, other) {
            (Kind::Array, _) | (_, Kind::Array) => Kind::Array,
            (Kind::Ref, _) | (_, Kind::Ref) => Kind::Ref,
            _ => Kind::Scalar,
        }
    }
}

/// How a position consumes its operand in a legacy formula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ctx {
    /// One value: a multi-cell reference is intersected.
    Value,
    /// A reference or array as such (SUM's arguments); nested operations still take values.
    Ref,
    /// Array evaluation of everything inside (SUMPRODUCT's arguments): nothing is intersected.
    Array,
}

/// A function parameter's legacy evaluation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Param {
    Value,
    /// One value even inside array evaluation: IF's condition and IFERROR's and IFNA's
    /// arguments. Legacy Excel evaluates them as arrays inside SUMPRODUCT only in a
    /// Ctrl+Shift+Enter formula, which this analysis never sees. A name's definition is still
    /// evaluated as an array throughout ([`Legacy::kind_of`]).
    Single,
    Ref,
    Array,
    /// The parameter is evaluated like the call itself (IF's and CHOOSE's results).
    Pass,
    /// A value read from the top-left cell of a reference, never intersected (T: Excel caches
    /// `T(H12:J12)` as T of H12 in a formula outside columns H:J). The function lifts over the
    /// cells, so the legacy result is the top-left one.
    First,
    /// An array parameter of a dynamic-array-era function: whether Excel evaluates a legacy
    /// formula there as an array is not established, so such formulas are written as dynamic.
    Uncertain,
}

/// How the writer saves a formula that neither spills nor is a legacy CSE array.
pub(crate) enum Export {
    /// As it is, without dynamic-array metadata.
    Unchanged,
    /// As this expression (the formula without its implicit `@`), without dynamic-array metadata.
    Legacy(Expr),
    /// With dynamic-array metadata: legacy evaluation would differ.
    Dynamic,
}

pub(crate) fn export(wb: &Workbook, sheet: usize, at: CellRef, formula: &Formula, functions: FunctionInfo) -> Export {
    let Some(expr) = formula.expr_arc() else { return Export::Unchanged };
    let mut explicit = false;
    expr.walk(&mut |e| explicit |= matches!(e, Expr::Unary(UnOp::At, _)));
    let mut legacy = Legacy::new(wb, at, functions);
    if !explicit {
        legacy.formula(expr.as_ref().clone(), sheet);
        return if !legacy.certain {
            Export::Dynamic
        } else if !legacy.complete || legacy.inserted == 0 {
            // An analysis that stopped at its limits leaves a legacy formula as it was read;
            // saving it keeps it legacy.
            Export::Unchanged
        } else {
            Export::Dynamic
        };
    }
    let Some(plain) = strip(&expr) else { return Export::Dynamic };
    // The formula is the legacy reading of `plain` when every `@` in it is one the analysis of
    // `plain` inserts, or one of a single value, and the analysis inserts no other.
    legacy.explicit = true;
    legacy.formula(expr.as_ref().clone(), sheet);
    if legacy.certain && legacy.complete && !legacy.foreign && legacy.inserted == 0 { Export::Legacy(plain) } else { Export::Dynamic }
}

/// Rewrites the legacy cell formulas at `cells` (model sheet index, positions) to their
/// dynamic-array reading, with `@` where Excel intersects. Runs once names and tables are loaded.
pub(crate) fn apply_legacy(wb: &mut Workbook, cells: Vec<(usize, Vec<CellRef>)>, functions: FunctionInfo) {
    for (si, positions) in cells {
        let Some(sheet) = wb.sheet(si) else { continue };
        let mut changed = Vec::new();
        for at in positions {
            let Some(formula) = sheet.cell(at).and_then(|c| c.formula.as_deref()) else { continue };
            if let Some(f) = legacy(wb, si, at, formula, functions) {
                changed.push((at, f));
            }
        }
        if changed.is_empty() {
            continue;
        }
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        for (at, f) in changed {
            if let Some(cell) = sheet.cells.get_mut(at) {
                cell.formula = Some(Arc::new(f));
            }
        }
    }
}

/// The dynamic-array reading of a legacy formula, when it differs.
fn legacy(wb: &Workbook, sheet: usize, at: CellRef, formula: &Formula, functions: FunctionInfo) -> Option<Formula> {
    if formula.array.is_some() {
        return None;
    }
    let expr = formula.expr_arc()?;
    let mut legacy = Legacy::new(wb, at, functions);
    let out = legacy.formula(expr.as_ref().clone(), sheet);
    // Text round trip: the stored expression is exactly what the text parses to. An analysis
    // that stopped early leaves the formula as it was read.
    (legacy.inserted > 0 && legacy.certain && legacy.complete).then(|| Formula::new(&gridcraft_formula::print(&out)))
}

/// `e` without its `@`, when its text (the printer writes only the parentheses the expression
/// has) reads back as the same expression. Every `@` goes with the parentheses [`at`] adds, as a
/// legacy formula's reading needs (a union there is always inside the file's own parentheses).
/// An `@` of a bare union has no such text, so its formula is saved as a dynamic one.
fn strip(e: &Expr) -> Option<Expr> {
    let plain = remove_at(e.clone());
    gridcraft_formula::parse(&gridcraft_formula::print(&plain)).is_ok_and(|p| p == plain).then_some(plain)
}

/// Removes every `@` and the parentheses [`at`] adds.
fn remove_at(e: Expr) -> Expr {
    e.map(&mut |x| match x {
        Expr::Unary(UnOp::At, inner) => unwrap_at(*inner),
        other => other,
    })
}

/// The operand of an `@` without the parentheses [`at`] adds.
fn unwrap_at(e: Expr) -> Expr {
    match e {
        Expr::Paren(p) if !atomic(&p) => *p,
        other => other,
    }
}

/// `@e`, parenthesized unless `e` prints as one operand. An operand already in parentheses (the
/// file's own) gets a second pair, so [`remove_at`] can tell them from the ones added here.
fn at(e: Expr) -> Expr {
    let e = if atomic(&e) { e } else { Expr::Paren(Box::new(e)) };
    Expr::Unary(UnOp::At, Box::new(e))
}

fn atomic(e: &Expr) -> bool {
    matches!(e, Expr::Ref(_) | Expr::Name(_) | Expr::Struct(_) | Expr::Call(..) | Expr::Array(_))
}

/// Nesting the analysis follows: Excel allows 64 levels of nested functions, and parentheses
/// and names add levels. Operator chains do not count (see [`Legacy::binary`]).
const MAX_DEPTH: usize = 128;

struct Legacy<'a> {
    wb: &'a Workbook,
    at: CellRef,
    functions: FunctionInfo,
    remaining: usize,
    locals: Vec<(String, Kind)>,
    /// Kinds of the names analysed so far, by upper-case name and scope; `None` while a name's
    /// definition is being analysed (a cycle).
    names: HashMap<(String, usize), Option<Kind>>,
    /// How many `@` were inserted.
    inserted: usize,
    /// The analysis covered only established legacy semantics.
    certain: bool,
    /// The analysis finished within its nesting and size limits.
    complete: bool,
    /// Analysing a formula that has its own `@` (see [`Legacy::explicit_at`]).
    explicit: bool,
    /// An explicit `@` is neither implied by legacy evaluation nor of a single value.
    foreign: bool,
    /// Analysing an expression whose rewrite is discarded (see [`Legacy::kind_of`]).
    discarded: bool,
}

impl<'a> Legacy<'a> {
    fn new(wb: &'a Workbook, at: CellRef, functions: FunctionInfo) -> Self {
        Legacy {
            wb,
            at,
            functions,
            remaining: 16384,
            locals: Vec::new(),
            names: HashMap::new(),
            inserted: 0,
            certain: true,
            complete: true,
            explicit: false,
            foreign: false,
            discarded: false,
        }
    }

    /// The kind of an expression analysed only for its kind (a name's definition, INDIRECT's
    /// text), evaluated as legacy Excel evaluates a name: as an array throughout, IF's condition
    /// included. Its rewrite is discarded, so it inserts no `@` into the formula.
    fn kind_of(&mut self, e: Expr, sheet: usize, depth: usize) -> Kind {
        let outer = (std::mem::replace(&mut self.discarded, true), std::mem::replace(&mut self.explicit, false), self.inserted);
        let (_, kind) = self.expr(e, sheet, Ctx::Array, depth);
        (self.discarded, self.explicit, self.inserted) = outer;
        kind
    }

    /// The whole cell formula: a multi-valued result shows its top-left value.
    fn formula(&mut self, e: Expr, sheet: usize) -> Expr {
        let e = match e {
            Expr::Unary(UnOp::At, x) if self.explicit => return self.explicit_at(*x, sheet, Ctx::Value, 0, true).0,
            e => e,
        };
        let (e, kind) = self.expr(e, sheet, Ctx::Value, 0);
        if kind == Kind::Array && self.certain {
            self.inserted += 1;
            at(e)
        } else {
            e
        }
    }

    /// An `@` already in the formula, at a position the analysis reads in `ctx`: analysed as its
    /// operand alone. The `@` is the one legacy evaluation implies when the analysis puts it back
    /// (that insertion is then not counted), or redundant when the operand is a single value;
    /// any other `@` makes the formula a dynamic-array one (`foreign`).
    fn explicit_at(&mut self, x: Expr, sheet: usize, ctx: Ctx, depth: usize, top: bool) -> (Expr, Kind) {
        let operand = unwrap_at(x);
        let nested = matches!(operand, Expr::Unary(UnOp::At, _));
        let before = self.inserted;
        let (y, kind) = self.expr(operand, sheet, ctx, depth + 1);
        if !nested && self.inserted > before && matches!(y, Expr::Unary(UnOp::At, _)) {
            self.inserted -= 1;
            return (y, Kind::Scalar);
        }
        if top && kind == Kind::Array {
            return (at(y), Kind::Scalar);
        }
        if kind != Kind::Scalar {
            self.foreign = true;
        }
        (Expr::Unary(UnOp::At, Box::new(y)), Kind::Scalar)
    }

    fn expr(&mut self, e: Expr, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        // Names can be cyclic or nest deeply; their kinds are remembered, so repeated names
        // cost one analysis each.
        if depth >= MAX_DEPTH || self.remaining == 0 {
            self.complete = false;
            return (e, Kind::Array);
        }
        self.remaining -= 1;
        let (e, kind) = self.node(e, sheet, ctx, depth);
        if ctx == Ctx::Value && kind == Kind::Ref {
            self.inserted += 1;
            (at(e), Kind::Scalar)
        } else {
            (e, kind)
        }
    }

    fn node(&mut self, e: Expr, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        let inner = |wanted: Ctx| if ctx == Ctx::Array { Ctx::Array } else { wanted };
        match e {
            Expr::Ref(r) => {
                let kind = if r.range().is_single() && !matches!(r.sheet, SheetSel::Span(_, _)) { Kind::Scalar } else { Kind::Ref };
                (Expr::Ref(r), kind)
            }
            Expr::Array(rows) => {
                let kind = if rows.len() > 1 || rows.first().is_some_and(|r| r.len() > 1) { Kind::Array } else { Kind::Scalar };
                (Expr::Array(rows), kind)
            }
            Expr::Name(n) => {
                let kind = self.name(&n, sheet, depth);
                (Expr::Name(n), kind)
            }
            Expr::Struct(r) => {
                let table = if r.table.is_empty() {
                    self.wb.sheet(sheet).and_then(|s| s.table_at(self.at))
                } else {
                    self.wb.table(&r.table).and_then(|(si, ti)| self.wb.sheet(si).and_then(|s| s.tables.get(ti)))
                };
                let one_row = r.specifiers.iter().any(|s| matches!(s, StructItem::ThisRow | StructItem::Headers | StructItem::Totals));
                let one_col = match (&r.col_start, &r.col_end) {
                    (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                    (Some(_), None) => true,
                    _ => table.is_some_and(|t| t.columns.len() == 1),
                };
                (Expr::Struct(r), if one_row && one_col { Kind::Scalar } else { Kind::Ref })
            }
            Expr::Paren(x) => {
                let (x, kind) = self.expr(*x, sheet, ctx, depth + 1);
                (Expr::Paren(Box::new(x)), kind)
            }
            Expr::Unary(UnOp::At, x) if self.explicit => self.explicit_at(*x, sheet, ctx, depth, false),
            Expr::Unary(UnOp::At, x) => {
                let (x, _) = self.expr(*x, sheet, inner(Ctx::Ref), depth + 1);
                (Expr::Unary(UnOp::At, Box::new(x)), Kind::Scalar)
            }
            // A trim reference is a reference operator, like `:`.
            Expr::Unary(op @ (UnOp::Spill | UnOp::Trim(_)), x) => {
                let (x, _) = self.expr(*x, sheet, inner(Ctx::Ref), depth + 1);
                (Expr::Unary(op, Box::new(x)), Kind::Ref)
            }
            Expr::Unary(op, x) => {
                let (x, kind) = self.expr(*x, sheet, inner(Ctx::Value), depth + 1);
                (Expr::Unary(op, Box::new(x)), if kind == Kind::Scalar { Kind::Scalar } else { Kind::Array })
            }
            Expr::Binary(..) => self.binary(e, sheet, ctx, depth),
            Expr::Call(name, args) => self.call(name, args, sheet, ctx, depth + 1),
            Expr::Invoke(f, args) => {
                self.certain = false;
                (Expr::Invoke(f, args), Kind::Array)
            }
            other => (other, Kind::Scalar),
        }
    }

    fn name(&mut self, n: &str, sheet: usize, depth: usize) -> Kind {
        if let Some((_, kind)) = self.locals.iter().rev().find(|(key, _)| key.eq_ignore_ascii_case(n)) {
            return *kind;
        }
        let (scope, bare) = match n.rsplit_once('!') {
            Some((s, name)) => (self.wb.sheet_index(&s.trim_matches('\'').replace("''", "'")).unwrap_or(sheet), name),
            None => (sheet, n),
        };
        if let Some(def) = self.wb.name(bare, scope) {
            let key = (bare.to_ascii_uppercase(), scope);
            match self.names.get(&key) {
                Some(Some(kind)) => return *kind,
                Some(None) => {
                    // A name defined in terms of itself has no legacy value to keep.
                    self.certain = false;
                    return Kind::Array;
                }
                None => {}
            }
            let Ok(expr) = gridcraft_formula::parse(def.formula.trim_start_matches('=')) else { return Kind::Array };
            self.names.insert(key.clone(), None);
            // A name's own formula does not see the formula's LET variables.
            let locals = std::mem::take(&mut self.locals);
            let kind = self.kind_of(expr, def.scope.unwrap_or(scope), depth + 1);
            self.locals = locals;
            self.names.insert(key, Some(kind));
            return kind;
        }
        if self.wb.table(bare).is_some() { Kind::Ref } else { Kind::Scalar }
    }

    fn call(&mut self, name: String, args: Vec<Expr>, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        if name == "LET" && args.len() >= 3 {
            return self.let_(args, sheet, ctx, depth);
        }
        let lambda_name = !(self.functions.is_builtin)(&name) && !host_function(&name) && self.wb.name(&name, sheet).is_some();
        if args.len() > 255
            || lambda_name
            || matches!(name.as_str(), "LAMBDA" | "MAP" | "REDUCE" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" | "ISOMITTED")
        {
            self.certain = false;
            return (Expr::Call(name, args), Kind::Array);
        }
        let mut lifted = false;
        let mut kinds = Vec::with_capacity(args.len());
        let mut out = Vec::with_capacity(args.len());
        for (i, arg) in args.into_iter().enumerate() {
            let param = param(&name, i, self.functions);
            let arg_ctx = match param {
                Param::Value | Param::Single => Ctx::Value,
                Param::Ref | Param::First => Ctx::Ref,
                Param::Pass => ctx,
                Param::Array => Ctx::Array,
                Param::Uncertain => {
                    self.certain = false;
                    Ctx::Array
                }
            };
            let arg_ctx = if ctx == Ctx::Array && (param != Param::Single || self.discarded) { Ctx::Array } else { arg_ctx };
            let (arg, kind) = self.expr(arg, sheet, arg_ctx, depth);
            // A value parameter given several values evaluates the function once per value.
            lifted |= match param {
                Param::Value | Param::Single => kind == Kind::Array || (kind == Kind::Ref && arg_ctx == Ctx::Array),
                Param::First => kind != Kind::Scalar,
                _ => false,
            };
            kinds.push(kind);
            out.push(arg);
        }
        let first = kinds.first().copied().unwrap_or(Kind::Scalar);
        let kind = match name.as_str() {
            "IF" | "CHOOSE" if !lifted => kinds.iter().skip(1).fold(Kind::Scalar, |k, x| k.join(*x)),
            "ROW" | "COLUMN" => {
                if first == Kind::Scalar {
                    Kind::Scalar
                } else if ctx == Ctx::Array {
                    Kind::Array
                } else {
                    // Outside array evaluation, ROW and COLUMN of a range give its first row or column.
                    self.inserted += 1;
                    return (at(Expr::Call(name, out)), Kind::Scalar);
                }
            }
            "INDIRECT" => match out.first() {
                Some(Expr::Text(text)) if !matches!(out.get(1), Some(Expr::Bool(false))) => match gridcraft_formula::parse(text) {
                    Ok(e) => {
                        let kind = self.kind_of(e, sheet, depth);
                        if kind == Kind::Scalar { Kind::Scalar } else { Kind::Ref }
                    }
                    Err(_) => Kind::Ref,
                },
                _ => Kind::Ref,
            },
            "OFFSET" => {
                // Omitted dimensions keep the source reference's shape; unknown dimensions might
                // become several cells on a later calculation.
                let first_multi = first != Kind::Scalar;
                let multi = if out.len() <= 3 { first_multi } else { !out.get(3).is_some_and(one) || out.get(4).map_or(first_multi, |e| !one(e)) };
                if lifted {
                    Kind::Array
                } else if multi {
                    Kind::Ref
                } else {
                    Kind::Scalar
                }
            }
            "INDEX" => {
                let row = out.get(1);
                let col = out.get(2);
                let multi = !row.is_some_and(positive) || col.is_some_and(|e| !positive(e)) || (col.is_none() && !out.first().is_some_and(one_line));
                if multi {
                    if first == Kind::Array { Kind::Array } else { Kind::Ref }
                } else if lifted {
                    Kind::Array
                } else {
                    Kind::Scalar
                }
            }
            "XLOOKUP" if !lifted => match (out.get(1), out.get(2)) {
                (Some(Expr::Ref(lookup)), Some(Expr::Ref(result))) => {
                    let multi = if lookup.range().width() == 1 { result.range().width() > 1 } else { result.range().height() > 1 };
                    if multi { Kind::Ref } else { Kind::Scalar }
                }
                _ => Kind::Ref,
            },
            "ANCHORARRAY" => Kind::Ref,
            "FILTER" | "SORT" | "SORTBY" | "UNIQUE" | "SEQUENCE" | "RANDARRAY" | "VSTACK" | "HSTACK" | "TOROW" | "TOCOL" | "WRAPROWS"
            | "WRAPCOLS" | "TAKE" | "DROP" | "CHOOSEROWS" | "CHOOSECOLS" | "EXPAND" | "GROUPBY" | "PIVOTBY" | "TRANSPOSE" | "MMULT" | "MINVERSE"
            | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH" => Kind::Array,
            // A user-defined function may return a range or an array.
            n if user_function(n, self.functions) => Kind::Array,
            _ if lifted => Kind::Array,
            _ => Kind::Scalar,
        };
        (Expr::Call(name, out), kind)
    }

    /// A binary operation. `A1+A2+…+A70` parses as a left-nested chain as deep as it is long, so
    /// the chain's left operands are walked iteratively, at the same depth; each operation still
    /// costs one node of the budget.
    fn binary(&mut self, e: Expr, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        let operand = |op: BinOp, node: Ctx| {
            if node == Ctx::Array {
                Ctx::Array
            } else if matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect) {
                Ctx::Ref
            } else {
                Ctx::Value
            }
        };
        // (operator, right operand, the operation's context), outermost first.
        let mut spine = Vec::new();
        let mut cur = e;
        let mut cur_ctx = ctx;
        while let Expr::Binary(op, a, b) = cur {
            if !spine.is_empty() {
                // The caller's `expr` counted the outermost operation.
                if self.remaining == 0 {
                    self.complete = false;
                } else {
                    self.remaining -= 1;
                }
            }
            spine.push((op, *b, cur_ctx));
            cur_ctx = operand(op, cur_ctx);
            cur = *a;
        }
        let (mut acc, mut kind) = self.expr(cur, sheet, cur_ctx, depth);
        for (i, (op, b, node_ctx)) in spine.into_iter().enumerate().rev() {
            let (b, kb) = self.expr(b, sheet, operand(op, node_ctx), depth);
            let node = Expr::Binary(op, Box::new(acc), Box::new(b));
            let k = if matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect) {
                Kind::Ref
            } else if kind == Kind::Scalar && kb == Kind::Scalar {
                Kind::Scalar
            } else {
                Kind::Array
            };
            // An inner operation is an operand: `expr` would intersect it there.
            (acc, kind) = if i > 0 && node_ctx == Ctx::Value && k == Kind::Ref {
                self.inserted += 1;
                (at(node), Kind::Scalar)
            } else {
                (node, k)
            };
        }
        (acc, kind)
    }

    fn let_(&mut self, args: Vec<Expr>, sheet: usize, ctx: Ctx, depth: usize) -> (Expr, Kind) {
        let saved = self.locals.len();
        let count = args.len();
        let mut out = Vec::with_capacity(count);
        let mut pending: Option<String> = None;
        let mut kind = Kind::Scalar;
        for (i, arg) in args.into_iter().enumerate() {
            if i + 1 == count {
                let (body, k) = self.expr(arg, sheet, ctx, depth);
                out.push(body);
                kind = k;
            } else if i % 2 == 0 {
                pending = match &arg {
                    Expr::Name(n) => Some(n.clone()),
                    _ => None,
                };
                out.push(arg);
            } else {
                let value_ctx = if ctx == Ctx::Array { Ctx::Array } else { Ctx::Ref };
                let (value, k) = self.expr(arg, sheet, value_ctx, depth);
                if let Some(n) = pending.take() {
                    self.locals.push((n, k));
                }
                out.push(value);
            }
        }
        self.locals.truncate(saved);
        (Expr::Call("LET".into(), out), kind)
    }
}

fn positive(e: &Expr) -> bool {
    matches!(e, Expr::Number(n) if *n >= 1.0)
}

fn one(e: &Expr) -> bool {
    matches!(e, Expr::Number(n) if *n == 1.0)
}

/// A one-column or one-row reference: INDEX with a single index picks one cell of it.
fn one_line(e: &Expr) -> bool {
    matches!(e, Expr::Ref(r) if r.range().width() == 1 || r.range().height() == 1)
}

/// Functions the calculation engine evaluates itself, whose parameters are described here
/// rather than by the function library's lifting metadata.
fn host_function(name: &str) -> bool {
    matches!(
        name,
        "IF" | "IFS"
            | "IFERROR"
            | "IFNA"
            | "CHOOSE"
            | "SWITCH"
            | "ROW"
            | "COLUMN"
            | "ROWS"
            | "COLUMNS"
            | "AREAS"
            | "ISREF"
            | "ISFORMULA"
            | "FORMULATEXT"
            | "SHEET"
            | "SHEETS"
            | "OFFSET"
            | "INDIRECT"
            | "INDEX"
            | "CELL"
            | "TRIMRANGE"
            | "SUBTOTAL"
            | "AGGREGATE"
            | "TEXT"
            | "LET"
            | "LAMBDA"
            | "ISOMITTED"
            | "MAP"
            | "REDUCE"
            | "SCAN"
            | "BYROW"
            | "BYCOL"
            | "MAKEARRAY"
            | "ANCHORARRAY"
            | "GETPIVOTDATA"
    )
}

/// Not a built-in worksheet function: an add-in or VBA function.
fn user_function(name: &str, functions: FunctionInfo) -> bool {
    !host_function(name) && !(functions.is_builtin)(name)
}

fn param(name: &str, i: usize, functions: FunctionInfo) -> Param {
    match name {
        "IF" if i == 0 => Param::Single,
        "IFERROR" | "IFNA" => Param::Single,
        "CHOOSE" if i == 0 => Param::Value,
        "IF" | "CHOOSE" => Param::Pass,
        "SUMPRODUCT" | "MMULT" | "MINVERSE" | "MDETERM" | "TRANSPOSE" | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH" => Param::Array,
        "INDEX" if i == 0 => Param::Array,
        // LOOKUP's vectors and AGGREGATE's array form evaluate their arguments as arrays.
        "LOOKUP" if i > 0 => Param::Array,
        "AGGREGATE" if i == 2 => Param::Array,
        "ROW" | "COLUMN" | "ROWS" | "COLUMNS" | "AREAS" | "ISREF" | "ISFORMULA" | "FORMULATEXT" | "SHEET" | "SHEETS" | "TRIMRANGE"
        | "ANCHORARRAY" => Param::Ref,
        "CELL" | "GETPIVOTDATA" if i == 1 => Param::Ref,
        "T" => Param::First,
        "FILTER" | "SORT" | "SORTBY" | "UNIQUE" | "VSTACK" | "HSTACK" | "TOROW" | "TOCOL" | "WRAPROWS" | "WRAPCOLS" | "TAKE" | "DROP"
        | "CHOOSEROWS" | "CHOOSECOLS" | "EXPAND" | "GROUPBY" | "PIVOTBY"
            if !scalar_arg(name, i) =>
        {
            Param::Uncertain
        }
        n if user_function(n, functions) => Param::Ref,
        n if scalar_arg(n, i) && (host_function(n) || (functions.is_scalar_arg)(n, i)) => Param::Value,
        _ => Param::Ref,
    }
}

/// Legacy reference/array-taking parameters. Other parameters consume a single value; the
/// function library's lifting metadata refines this for the functions it implements.
fn scalar_arg(name: &str, i: usize) -> bool {
    match name {
        "SUM" | "AVERAGE" | "AVERAGEA" | "MIN" | "MINA" | "MAX" | "MAXA" | "COUNT" | "COUNTA" | "COUNTBLANK" | "PRODUCT" | "SUMSQ" | "MEDIAN"
        | "MODE" | "MODE.SNGL" | "STDEV" | "STDEV.S" | "STDEV.P" | "STDEVP" | "VAR" | "VAR.S" | "VAR.P" | "VARP" | "AVEDEV" | "DEVSQ" | "GEOMEAN"
        | "HARMEAN" | "KURT" | "SKEW" | "SKEW.P" | "AND" | "OR" | "XOR" | "SUMPRODUCT" | "SUMX2MY2" | "SUMX2PY2" | "SUMXMY2" | "CORREL" | "COVAR"
        | "COVARIANCE.P" | "COVARIANCE.S" | "PEARSON" | "RSQ" | "SLOPE" | "INTERCEPT" | "STEYX" | "ROWS" | "COLUMNS" | "AREAS" | "ROW" | "COLUMN"
        | "TRANSPOSE" | "ISREF" | "MMULT" | "MINVERSE" | "MDETERM" | "FREQUENCY" | "CONCAT" | "VSTACK" | "HSTACK" | "ANCHORARRAY" => false,
        "SUMIF" | "AVERAGEIF" => i == 1,
        "COUNTIF" => i != 0,
        "SUMIFS" | "AVERAGEIFS" | "MINIFS" | "MAXIFS" => i > 0 && i.is_multiple_of(2),
        "COUNTIFS" => i % 2 == 1,
        "INDEX" | "LARGE" | "SMALL" | "PERCENTILE" | "PERCENTILE.INC" | "PERCENTILE.EXC" | "QUARTILE" | "QUARTILE.INC" | "QUARTILE.EXC" | "IRR"
        | "MIRR" | "RANK" | "RANK.EQ" | "RANK.AVG" => i != if name.starts_with("RANK") { 1 } else { 0 },
        "VLOOKUP" | "HLOOKUP" | "MATCH" | "XMATCH" => i != 1,
        "LOOKUP" | "NPV" | "XNPV" | "SUBTOTAL" => i == 0,
        "XLOOKUP" => i != 1 && i != 2,
        "XIRR" => i > 1,
        "OFFSET" => i != 0,
        "AGGREGATE" | "TEXTJOIN" => i < 2,
        "FILTER" | "SORTBY" => i > 1,
        "SORT" | "UNIQUE" | "TAKE" | "DROP" | "CHOOSECOLS" | "CHOOSEROWS" | "TOROW" | "TOCOL" | "WRAPROWS" | "WRAPCOLS" | "EXPAND" => i != 0,
        "LINEST" | "LOGEST" | "TREND" | "GROWTH" => i > 1,
        "FORECAST" | "FORECAST.LINEAR" => i == 0,
        "FORECAST.ETS" | "FORECAST.ETS.CONFINT" => i != 1 && i != 2,
        "FORECAST.ETS.SEASONALITY" | "FORECAST.ETS.STAT" => i > 1,
        _ => true,
    }
}
