//! Export-time implicit-intersection analysis, without evaluating the workbook.
//!
//! A range is harmless in SUM, but not in SQRT or arithmetic. Keep the result's shape separate
//! from affected operations inside it: SUM(range * range) returns a scalar yet needs array
//! evaluation. Legacy array-aware contexts such as SUMPRODUCT already preserve those operations.

use gridcraft_core::CellRef;
use gridcraft_formula::{BinOp, Expr, SheetSel, StructItem, UnOp};
use gridcraft_model::{Formula, Workbook};

#[derive(Clone, Copy, Default)]
struct Shape {
    array: bool,
    affected: bool,
}

impl Shape {
    fn array() -> Self {
        Self { array: true, affected: false }
    }
}

pub(crate) fn needs_array(wb: &Workbook, sheet: usize, at: CellRef, formula: &Formula) -> bool {
    let Some(expr) = formula.expr_arc() else { return false };
    let mut scan = Scan { wb, at, remaining: 4096, locals: Vec::new() };
    let shape = scan.expr(&expr, sheet, false, 0);
    // An explicit `@` is written as `_xlfn.SINGLE`, which only dynamic-array formulas carry.
    let mut at = false;
    expr.walk(&mut |e| at |= matches!(e, Expr::Unary(UnOp::At, _)));
    shape.array || shape.affected || at
}

struct Scan<'a> {
    wb: &'a Workbook,
    at: CellRef,
    remaining: usize,
    locals: Vec<(String, Shape)>,
}

impl Scan<'_> {
    fn expr(&mut self, e: &Expr, sheet: usize, array_context: bool, depth: usize) -> Shape {
        // Names can be cyclic, and a short chain of repeated names can expand exponentially.
        // Preserve array evaluation when analysis cannot safely finish.
        if depth >= 64 || self.remaining == 0 {
            return Shape { array: true, affected: true };
        }
        self.remaining -= 1;
        match e {
            Expr::Ref(r) => Shape { array: !r.range().is_single() || matches!(r.sheet, SheetSel::Span(_, _)), affected: false },
            Expr::Array(rows) => Shape { array: rows.len() > 1 || rows.first().is_some_and(|r| r.len() > 1), affected: false },
            Expr::Name(n) => {
                if let Some((_, shape)) = self.locals.iter().rev().find(|(key, _)| key.eq_ignore_ascii_case(n)) {
                    return *shape;
                }
                let (scope, bare) = match n.rsplit_once('!') {
                    Some((s, name)) => (self.wb.sheet_index(&s.trim_matches('\'').replace("''", "'")).unwrap_or(sheet), name),
                    None => (sheet, n.as_str()),
                };
                if let Some(def) = self.wb.name(bare, scope) {
                    return match gridcraft_formula::parse(def.formula.trim_start_matches('=')) {
                        Ok(expr) => self.expr(&expr, def.scope.unwrap_or(scope), array_context, depth + 1),
                        Err(_) => Shape::array(),
                    };
                }
                Shape { array: self.wb.table(bare).is_some(), affected: false }
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
                Shape { array: !(one_row && one_col), affected: false }
            }
            Expr::Paren(x) => self.expr(x, sheet, array_context, depth + 1),
            Expr::Unary(op, x) => {
                let child = self.expr(x, sheet, array_context, depth + 1);
                match op {
                    UnOp::At => Shape { array: false, affected: child.affected },
                    // A trim reference is a reference operator, like `:`.
                    UnOp::Spill | UnOp::Trim(_) => Shape { array: true, affected: child.affected },
                    _ => Shape { array: child.array, affected: child.affected || (child.array && !array_context) },
                }
            }
            Expr::Binary(op, a, b) => {
                let a = self.expr(a, sheet, array_context, depth + 1);
                let b = self.expr(b, sheet, array_context, depth + 1);
                let reference = matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect);
                let array = reference || a.array || b.array;
                Shape { array, affected: a.affected || b.affected || (!reference && array && !array_context) }
            }
            Expr::Call(name, args) => self.call(name, args, sheet, array_context, depth + 1),
            Expr::Invoke(_, _) => Shape { array: true, affected: false },
            _ => Shape::default(),
        }
    }

    fn call(&mut self, name: &str, args: &[Expr], sheet: usize, array_context: bool, depth: usize) -> Shape {
        if name == "LET"
            && args.len() >= 3
            && let Some((last, bindings)) = args.split_last()
        {
            let saved = self.locals.len();
            let mut affected = false;
            for pair in bindings.as_chunks::<2>().0 {
                if let [Expr::Name(n), value] = pair {
                    let shape = self.expr(value, sheet, array_context, depth);
                    affected |= shape.affected;
                    self.locals.push((n.clone(), shape));
                }
            }
            let mut result = self.expr(last, sheet, array_context, depth);
            result.affected |= affected;
            self.locals.truncate(saved);
            return result;
        }
        let legacy_array =
            matches!(name, "SUMPRODUCT" | "MMULT" | "MINVERSE" | "MDETERM" | "TRANSPOSE" | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH");
        let mut affected = false;
        let mut lifted = false;
        let mut shapes = Vec::with_capacity(args.len().min(256));
        if args.len() > 255 {
            return Shape { array: true, affected: true };
        }
        for (i, arg) in args.iter().enumerate() {
            let context = array_context || legacy_array || (name == "INDEX" && i == 0);
            let shape = self.expr(arg, sheet, context, depth);
            affected |= shape.affected;
            if shape.array && scalar_arg(name, i) {
                lifted = true;
                affected |= !context;
            }
            shapes.push(shape);
        }
        let first_array = shapes.first().is_some_and(|s| s.array);
        let array = match name {
            "INDIRECT" => match args.first() {
                Some(Expr::Text(text)) if !matches!(args.get(1), Some(Expr::Bool(false))) => {
                    gridcraft_formula::parse(text).map(|e| self.expr(&e, sheet, array_context, depth).array).unwrap_or(true)
                }
                _ => true,
            },
            "OFFSET" => {
                // Omitted dimensions retain the source reference's shape; unknown dimensions
                // might become multiple cells on a later calculation.
                if args.len() <= 3 { first_array } else { !args.get(3).is_some_and(one) || args.get(4).map_or(first_array, |e| !one(e)) }
            }
            "INDEX" => {
                let row = args.get(1);
                let col = args.get(2);
                !row.is_some_and(positive) || col.is_some_and(|e| !positive(e)) || (col.is_none() && !args.first().is_some_and(single_column))
            }
            "ROW" | "COLUMN" => {
                affected |= first_array && !array_context;
                first_array
            }
            "IF" | "IFERROR" | "IFNA" | "IFS" | "SWITCH" | "CHOOSE" => shapes.iter().any(|s| s.array),
            "XLOOKUP" => {
                lifted
                    || match (args.get(1), args.get(2)) {
                        (Some(Expr::Ref(lookup)), Some(Expr::Ref(result))) => {
                            if lookup.range().width() == 1 {
                                result.range().width() > 1
                            } else {
                                result.range().height() > 1
                            }
                        }
                        _ => true,
                    }
            }
            "FILTER" | "SORT" | "SORTBY" | "UNIQUE" | "SEQUENCE" | "RANDARRAY" | "VSTACK" | "HSTACK" | "TOROW" | "TOCOL" | "WRAPROWS"
            | "WRAPCOLS" | "TAKE" | "DROP" | "CHOOSEROWS" | "CHOOSECOLS" | "EXPAND" | "GROUPBY" | "PIVOTBY" | "TRANSPOSE" | "MMULT" | "MINVERSE"
            | "FREQUENCY" | "LINEST" | "LOGEST" | "TREND" | "GROWTH" | "MAP" | "SCAN" | "BYROW" | "BYCOL" | "MAKEARRAY" | "ANCHORARRAY" => true,
            _ => lifted || self.wb.name(name, sheet).is_some(),
        };
        Shape { array, affected }
    }
}

fn positive(e: &Expr) -> bool {
    matches!(e, Expr::Number(n) if *n >= 1.0)
}

fn one(e: &Expr) -> bool {
    matches!(e, Expr::Number(n) if *n == 1.0)
}

fn single_column(e: &Expr) -> bool {
    matches!(e, Expr::Ref(r) if r.range().width() == 1)
}

/// Legacy reference/array-taking parameters. Other parameters consume a single value.
/// This is file-format policy, not a second evaluator: unknown results are never calculated.
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

/// Import-time legacy implicit intersection (issue #1636).
///
/// A plain `<f>` formula from an older file has no `@`, but a multi-cell reference in a
/// scalar position meant implicit intersection there (`ISBLANK(B:B)` at row 4 read `B4`).
/// Without it the reference lifts into an array that spills or fails with `#SPILL!`. Wrap
/// those references in `@`, which the evaluator already intersects and the exporter writes
/// as `_xlfn.SINGLE`, so the file round-trips as a modern explicit intersection.
///
/// Only references on an all-scalar path from the root are wrapped: anything under an
/// array-taking position (aggregates, `SUM(IF(…))`, array constructors such as `FILTER` or
/// `MAP`) keeps lifting, so intentional array formulas still spill. Modern spills are
/// stored as `t="array"` and never reach this rewrite. Deliberately untouched: names and
/// structured references (scope needs evaluation), `LAMBDA` bodies and invocations and
/// calls through `LET`-bound names (deferred evaluation), `LET` values (a bound array must
/// stay an array for `SUM(x)`), and the non-lifting single-reference heads (`FORMULATEXT`,
/// `TYPE`, `ROWS`, …), whose scalar result must not move. Defined-name `LAMBDA`s cannot be
/// seen here (names are read after sheets) and stay a known gap. Returns `None` when
/// nothing changed, so callers keep the original text byte-identical.
pub(crate) fn legacy_intersect(e: Expr) -> Option<Expr> {
    let mut rw = Rewriter { bound: Vec::new(), changed: false };
    let out = rw.expr(e, true, 0);
    rw.changed.then_some(out)
}

struct Rewriter {
    /// `LET`-bound names in scope: a call to one may invoke a `LAMBDA` over a range, which
    /// must keep spilling, so calls through these names are left alone.
    bound: Vec<String>,
    changed: bool,
}

/// Expression nesting the rewrite still descends into (hostile files nest deeply).
const MAX_REWRITE_DEPTH: usize = 64;

impl Rewriter {
    /// `scalar` is whether this position expects a single value: only there does a legacy
    /// multi-cell reference intersect. Anything else keeps its references untouched.
    fn expr(&mut self, e: Expr, scalar: bool, depth: usize) -> Expr {
        if !scalar || depth >= MAX_REWRITE_DEPTH {
            return e;
        }
        match e {
            Expr::Ref(r) if !r.range().is_single() && !matches!(r.sheet, SheetSel::Span(..)) => {
                self.changed = true;
                Expr::Unary(UnOp::At, Box::new(Expr::Ref(r)))
            }
            Expr::Paren(x) => Expr::Paren(Box::new(self.expr(*x, true, depth + 1))),
            Expr::Unary(op @ (UnOp::Neg | UnOp::Plus | UnOp::Percent), x) => Expr::Unary(op, Box::new(self.expr(*x, true, depth + 1))),
            Expr::Binary(op, a, b) if !matches!(op, BinOp::Range | BinOp::Union | BinOp::Intersect) => {
                Expr::Binary(op, Box::new(self.expr(*a, true, depth + 1)), Box::new(self.expr(*b, true, depth + 1)))
            }
            Expr::Call(name, args) => self.call(&name, args, depth + 1),
            // Array constants evaluate element-wise in every Excel; names, structured
            // references, explicit `@`/`#`/trim references and invocations keep their shape.
            _ => e,
        }
    }

    fn call(&mut self, name: &str, args: Vec<Expr>, depth: usize) -> Expr {
        if name == "LET" && args.len() >= 3 && args.len() % 2 == 1 {
            let mut args = args;
            if let Some(body) = args.pop() {
                // The values stay array-evaluated (a LET-bound array must keep spilling for
                // `SUM(x)`); only the body keeps the scalar context. Well-formed pairs lend
                // their names to the scope, like the export scan.
                let saved = self.bound.len();
                for pair in args.as_chunks::<2>().0 {
                    if let [Expr::Name(n), _] = pair {
                        self.bound.push(n.clone());
                    }
                }
                let body = self.expr(body, true, depth);
                self.bound.truncate(saved);
                args.push(body);
            }
            return Expr::Call(name.to_string(), args);
        }
        // Deferred or unknown-arity evaluation, and single-reference heads that never lift
        // (`TYPE(B:B)` is 64, `ROWS(B:B)` its row count: intersecting would move the answer).
        // Array constructors always spill, whatever their arguments hold (mirrors the export
        // scan's always-array list, which `scalar_arg` alone does not cover).
        if self.bound.iter().any(|b| b.eq_ignore_ascii_case(name))
            || matches!(
                name,
                "LAMBDA"
                    | "ISFORMULA"
                    | "FORMULATEXT"
                    | "SHEET"
                    | "SHEETS"
                    | "TYPE"
                    | "ERROR.TYPE"
                    | "FILTER"
                    | "SORT"
                    | "SORTBY"
                    | "UNIQUE"
                    | "SEQUENCE"
                    | "RANDARRAY"
                    | "VSTACK"
                    | "HSTACK"
                    | "TOROW"
                    | "TOCOL"
                    | "WRAPROWS"
                    | "WRAPCOLS"
                    | "TAKE"
                    | "DROP"
                    | "CHOOSEROWS"
                    | "CHOOSECOLS"
                    | "EXPAND"
                    | "GROUPBY"
                    | "PIVOTBY"
                    | "TRANSPOSE"
                    | "MMULT"
                    | "MINVERSE"
                    | "FREQUENCY"
                    | "LINEST"
                    | "LOGEST"
                    | "TREND"
                    | "GROWTH"
                    | "MAP"
                    | "SCAN"
                    | "BYROW"
                    | "BYCOL"
                    | "MAKEARRAY"
                    | "ANCHORARRAY"
            )
        {
            return Expr::Call(name.to_string(), args);
        }
        Expr::Call(name.to_string(), args.into_iter().enumerate().map(|(i, a)| self.expr(a, Self::arg_scalar(name, i), depth)).collect())
    }

    /// Whether argument `i` of `name` expects a single value. Mirrors [`scalar_arg`], except
    /// `ROW`/`COLUMN`, whose spilling result still intersected in legacy non-array formulas.
    fn arg_scalar(name: &str, i: usize) -> bool {
        name == "ROW" || name == "COLUMN" || scalar_arg(name, i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `legacy_intersect` on parsed `text`: the rewritten text, or `None` for unchanged.
    fn rewrite(text: &str) -> Option<String> {
        legacy_intersect(gridcraft_formula::parse(text).unwrap()).map(|e| gridcraft_formula::print(&e))
    }

    #[test]
    fn legacy_scalar_references_gain_at() {
        // The issue's formula: the whole-column test intersects at the formula's row.
        assert_eq!(rewrite("IF(ISBLANK(B:B),0,IF(B4=\"Thierry\",C4,0))").as_deref(), Some("IF(ISBLANK(@B:B),0,IF(B4=\"Thierry\",C4,0))"));
        for (before, after) in [
            ("ISBLANK(B:B)", "ISBLANK(@B:B)"),
            ("-B:B", "-@B:B"),
            ("B:B+1", "@B:B+1"),
            ("B:B&C:C", "@B:B&@C:C"),
            ("B:B", "@B:B"),
            ("B:B=C4", "@B:B=C4"),
            ("IF(B:B,1,0)", "IF(@B:B,1,0)"),
            ("XLOOKUP(B:B,A:A,C:C)", "XLOOKUP(@B:B,A:A,C:C)"),
            ("VLOOKUP(B:B,A:C,2)", "VLOOKUP(@B:B,A:C,2)"),
            ("MATCH(B:B,A:A,0)", "MATCH(@B:B,A:A,0)"),
            ("ROW(B:B)", "ROW(@B:B)"),
            ("CHOOSE(B:B,1,2)", "CHOOSE(@B:B,1,2)"),
            ("SUMIF(A:A,B:B,C:C)", "SUMIF(A:A,@B:B,C:C)"),
            ("LET(x,1,x+B:B)", "LET(x,1,x+@B:B)"),
            ("OFFSET(A1,B:B,0)", "OFFSET(A1,@B:B,0)"),
            ("MyName+B:B", "MyName+@B:B"),
        ] {
            assert_eq!(rewrite(before).as_deref(), Some(after), "{before}");
        }
    }

    #[test]
    fn legacy_array_positions_keep_lifting() {
        // Array-taking positions, array constructors and deferred evaluation stay dynamic.
        for text in [
            "SUM(B:B)",
            "SUM(IF(B:B=\"x\",1,0))",
            "SUMPRODUCT(B:B=C:C)",
            "FILTER(B:B,B:B>1)",
            "MAP(B:B,LAMBDA(x,x+1))",
            "SEQUENCE(B:B)",
            "AND(B:B)",
            "CONCAT(B:B)",
            "SUM(ROW(B:B))",
            "LET(x,B:B,SUM(x))",
            "LET(f,LAMBDA(x,x*2),f(B:B))",
            "LAMBDA(x,x+B:B)",
            "TYPE(B:B)",
            "ROWS(B:B)",
            "FORMULATEXT(B:B)",
            "B4",
            "B4+1",
            "SUM(A:A)+1",
        ] {
            assert_eq!(rewrite(text), None, "{text}");
        }
    }
}
