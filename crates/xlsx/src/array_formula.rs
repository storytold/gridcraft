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
    shape.array || shape.affected
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
                    UnOp::Spill => Shape { array: true, affected: child.affected },
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
        _ => true,
    }
}
