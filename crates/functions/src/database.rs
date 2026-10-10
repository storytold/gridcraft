//! Database functions (DSUM, DCOUNT, DGET…): aggregate one field of the records of a table
//! that satisfy a criteria table.
//!
//! The database and the criteria are arrays whose first row holds column labels. Each criteria
//! row is a set of conditions that must all hold (AND); a record matches when any criteria row
//! holds (OR). An empty criteria cell matches anything. Plain text conditions match values that
//! begin with the text, as in Excel's advanced filter.

use std::borrow::Cow;

use gridcraft_core::parse::{parse_error_in, parse_number_text_in};
use gridcraft_core::{Array, CellError, Value, compare_text, number_to_text_in};

use crate::criteria::Criterion;
use crate::util::{A, R, as_array, has, num_val};
use crate::{Arg, Ctx, FnSpec};

/// Column index of a label in the header row.
fn find_column(ctx: &dyn Ctx, db: &Array, label: &str) -> Option<usize> {
    let label = label.trim();
    (0..db.cols).find(|&c| match db.get(0, c) {
        Some(Value::Text(t)) => compare_text(t.trim(), label).is_eq(),
        Some(Value::Number(n)) => number_to_text_in(*n, &ctx.locale().regional) == label,
        _ => false,
    })
}

/// Resolves the field argument: a label or a 1-based column number. `None` when omitted.
fn field_index(ctx: &dyn Ctx, db: &Array, a: &[Arg]) -> R<Option<usize>> {
    if !has(a, 1) {
        return Ok(None);
    }
    let v = a.get(1).map(|x| x.value.scalar()).unwrap_or(Value::Empty);
    match v {
        Value::Number(n) => {
            let i = n.trunc();
            if i < 1.0 || i > db.cols as f64 {
                return Err(CellError::Value);
            }
            Ok(Some(i as usize - 1))
        }
        Value::Text(t) => find_column(ctx, db, &t).map(Some).ok_or(CellError::Value),
        Value::Bool(_) => Err(CellError::Value),
        Value::Error(e) => Err(e),
        _ => Ok(None),
    }
}

/// One criteria cell as a matcher. Plain text becomes a "begins with" pattern.
fn cell_criterion(ctx: &dyn Ctx, v: &Value) -> Option<Criterion> {
    match v {
        Value::Empty => None,
        Value::Text(t) if t.is_empty() => None,
        Value::Text(t) => {
            let s: &str = t;
            let locale = ctx.locale();
            let has_op = s.starts_with(['=', '<', '>']);
            let plain_text = !has_op
                && parse_number_text_in(s, ctx.date_system(), &locale.regional).is_none()
                && locale.formula.parse_bool(s).is_none()
                && parse_error_in(s, locale.formula).is_none();
            if plain_text && !s.ends_with('*') { Some(Criterion::parse(ctx, &Value::from(format!("{s}*")))) } else { Some(Criterion::parse(ctx, v)) }
        }
        other => Some(Criterion::parse(ctx, other)),
    }
}

/// Rows (1-based within the database, excluding the header) of matching records.
fn matching_records(ctx: &dyn Ctx, db: &Array, crit: &Array) -> Vec<usize> {
    // Per criteria row: list of (database column or None when the label is unknown, criterion).
    let mut rows: Vec<Vec<(Option<usize>, Criterion)>> = Vec::new();
    for r in 1..crit.rows {
        let mut conds = Vec::new();
        for c in 0..crit.cols {
            let Some(cell) = crit.get(r, c) else { continue };
            let Some(cr) = cell_criterion(ctx, cell) else { continue };
            let col = match crit.get(0, c) {
                Some(Value::Text(t)) => find_column(ctx, db, t),
                Some(Value::Number(n)) => find_column(ctx, db, &number_to_text_in(*n, &ctx.locale().regional)),
                _ => None,
            };
            conds.push((col, cr));
        }
        rows.push(conds);
    }
    let all_match = rows.is_empty();
    (1..db.rows)
        .filter(|&r| {
            all_match
                || rows.iter().any(|conds| {
                    conds.iter().all(|(col, cr)| match col {
                        Some(c) => cr.matches(db.get(r, *c).unwrap_or(&Value::Empty)),
                        None => false,
                    })
                })
        })
        .collect()
}

struct Selection<'a> {
    db: Cow<'a, Array>,
    field: Option<usize>,
    records: Vec<usize>,
}

fn select<'a>(ctx: &dyn Ctx, a: &'a [Arg]) -> R<Selection<'a>> {
    let dbv = &a.first().ok_or(CellError::Value)?.value;
    let critv = &a.get(2).ok_or(CellError::Value)?.value;
    if let Value::Error(e) = dbv {
        return Err(*e);
    }
    if let Value::Error(e) = critv {
        return Err(*e);
    }
    let db = as_array(dbv);
    let crit = as_array(critv);
    let field = field_index(ctx, &db, a)?;
    let records = matching_records(ctx, &db, &crit);
    Ok(Selection { db, field, records })
}

/// Numbers of the field among matching records (field required).
fn field_numbers(c: &dyn Ctx, a: &[Arg]) -> R<Vec<f64>> {
    let s = select(c, a)?;
    let f = s.field.ok_or(CellError::Value)?;
    Ok(s.records.iter().filter_map(|&r| s.db.get(r, f).and_then(Value::as_f64)).collect())
}

fn dsum(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(field_numbers(c, a)?.iter().sum())
}

fn daverage(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let v = field_numbers(c, a)?;
    if v.is_empty() {
        return Err(CellError::Div0);
    }
    num_val(v.iter().sum::<f64>() / v.len() as f64)
}

fn dcount(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let s = select(c, a)?;
    let n = match s.field {
        Some(f) => s.records.iter().filter(|&&r| matches!(s.db.get(r, f), Some(Value::Number(_)))).count(),
        None => s.records.len(),
    };
    num_val(n as f64)
}

fn dcounta(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let s = select(c, a)?;
    let n = match s.field {
        Some(f) => s.records.iter().filter(|&&r| !matches!(s.db.get(r, f), None | Some(Value::Empty))).count(),
        None => s.records.len(),
    };
    num_val(n as f64)
}

fn dget(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let s = select(c, a)?;
    let f = s.field.ok_or(CellError::Value)?;
    match s.records.as_slice() {
        [] => Err(CellError::Value),
        [r] => Ok(s.db.get(*r, f).cloned().unwrap_or(Value::Empty)),
        _ => Err(CellError::Num),
    }
}

fn dmax(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let v = field_numbers(c, a)?;
    num_val(v.iter().copied().reduce(f64::max).unwrap_or(0.0))
}

fn dmin(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let v = field_numbers(c, a)?;
    num_val(v.iter().copied().reduce(f64::min).unwrap_or(0.0))
}

fn dproduct(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    let v = field_numbers(c, a)?;
    if v.is_empty() {
        return num_val(0.0);
    }
    num_val(v.iter().product())
}

fn variance(v: &[f64], sample: bool) -> R<f64> {
    let n = v.len() as f64;
    let min = if sample { 2.0 } else { 1.0 };
    if n < min {
        return Err(CellError::Div0);
    }
    let mean = v.iter().sum::<f64>() / n;
    let ss: f64 = v.iter().map(|x| (x - mean) * (x - mean)).sum();
    Ok(ss / if sample { n - 1.0 } else { n })
}

fn dvar(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(variance(&field_numbers(c, a)?, true)?)
}

fn dvarp(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(variance(&field_numbers(c, a)?, false)?)
}

fn dstdev(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(variance(&field_numbers(c, a)?, true)?.sqrt())
}

fn dstdevp(a: &[Arg], c: &mut dyn Ctx) -> R<Value> {
    num_val(variance(&field_numbers(c, a)?, false)?.sqrt())
}

pub(crate) fn specs() -> Vec<FnSpec> {
    vec![
        f!("DSUM", 3, 3, Database, A, "DSUM(database, field, criteria)", "Adds the numbers in a field of the records that meet the criteria.", dsum),
        f!(
            "DAVERAGE",
            3,
            3,
            Database,
            A,
            "DAVERAGE(database, field, criteria)",
            "Averages the numbers in a field of the records that meet the criteria.",
            daverage
        ),
        f!(
            "DCOUNT",
            3,
            3,
            Database,
            A,
            "DCOUNT(database, [field], criteria)",
            "Counts numeric cells in a field of the matching records (or the matching records).",
            dcount
        ),
        f!(
            "DCOUNTA",
            3,
            3,
            Database,
            A,
            "DCOUNTA(database, [field], criteria)",
            "Counts non-blank cells in a field of the matching records (or the matching records).",
            dcounta
        ),
        f!(
            "DGET",
            3,
            3,
            Database,
            A,
            "DGET(database, field, criteria)",
            "Returns the field value of the single record that meets the criteria.",
            dget
        ),
        f!("DMAX", 3, 3, Database, A, "DMAX(database, field, criteria)", "Largest number in a field of the matching records.", dmax),
        f!("DMIN", 3, 3, Database, A, "DMIN(database, field, criteria)", "Smallest number in a field of the matching records.", dmin),
        f!(
            "DPRODUCT",
            3,
            3,
            Database,
            A,
            "DPRODUCT(database, field, criteria)",
            "Multiplies the numbers in a field of the matching records.",
            dproduct
        ),
        f!(
            "DSTDEV",
            3,
            3,
            Database,
            A,
            "DSTDEV(database, field, criteria)",
            "Sample standard deviation of a field over the matching records.",
            dstdev
        ),
        f!(
            "DSTDEVP",
            3,
            3,
            Database,
            A,
            "DSTDEVP(database, field, criteria)",
            "Population standard deviation of a field over the matching records.",
            dstdevp
        ),
        f!("DVAR", 3, 3, Database, A, "DVAR(database, field, criteria)", "Sample variance of a field over the matching records.", dvar),
        f!("DVARP", 3, 3, Database, A, "DVARP(database, field, criteria)", "Population variance of a field over the matching records.", dvarp),
    ]
}

#[cfg(test)]
mod tests {
    use crate::util::testutil::*;
    use gridcraft_core::{CellError, Value};

    /// The classic orchard table.
    fn db() -> Value {
        arr(vec![
            vec![tv("Tree"), tv("Height"), tv("Age"), tv("Yield"), tv("Profit")],
            vec![tv("Apple"), nv(18.0), nv(20.0), nv(14.0), nv(105.0)],
            vec![tv("Pear"), nv(12.0), nv(12.0), nv(10.0), nv(96.0)],
            vec![tv("Cherry"), nv(13.0), nv(14.0), nv(9.0), nv(105.0)],
            vec![tv("Apple"), nv(14.0), nv(15.0), nv(10.0), nv(75.0)],
            vec![tv("Pear"), nv(9.0), nv(8.0), nv(8.0), nv(76.8)],
            vec![tv("Apple"), nv(8.0), nv(9.0), nv(6.0), nv(45.0)],
        ])
    }

    /// Apple with height > 10, OR any pear.
    fn crit() -> Value {
        arr(vec![
            vec![tv("Tree"), tv("Height"), tv("Age"), tv("Yield"), tv("Profit"), tv("Height")],
            vec![tv("=Apple"), tv(">10"), Value::Empty, Value::Empty, Value::Empty, tv("<16")],
            vec![tv("=Pear"), Value::Empty, Value::Empty, Value::Empty, Value::Empty, Value::Empty],
        ])
    }

    fn d(f: &str, field: crate::Arg) -> Value {
        ev(f, vec![rf(db()), field, rf(crit())])
    }

    #[test]
    fn orchard() {
        close(d("DCOUNT", t("Age")), 3.0);
        close(d("DCOUNTA", t("Profit")), 3.0);
        close(d("DMAX", t("Profit")), 96.0);
        close(d("DMIN", t("Profit")), 75.0);
        close(d("DSUM", t("Profit")), 247.8);
        close(d("DSUM", n(5.0)), 247.8);
        close(d("DPRODUCT", t("Yield")), 800.0);
        close(d("DAVERAGE", t("Yield")), 28.0 / 3.0);
        close_tol(d("DSTDEV", t("Yield")), 1.154700538, 1e-9);
        close_tol(d("DSTDEVP", t("Yield")), 0.942809042, 1e-9);
        close_tol(d("DVAR", t("Yield")), 1.333333333, 1e-9);
        close_tol(d("DVARP", t("Yield")), 0.888888889, 1e-9);
        close(d("DCOUNT", empty()), 3.0);
        is_err(d("DSUM", t("Nope")), CellError::Value);
        is_err(d("DSUM", n(9.0)), CellError::Value);
    }

    #[test]
    fn simple_criteria() {
        let c = |cells: Vec<Value>| arr(vec![vec![tv("Tree"), tv("Height")], cells]);
        close(ev("DSUM", vec![rf(db()), t("Profit"), rf(c(vec![tv("Apple"), Value::Empty]))]), 225.0);
        // Plain text matches values that begin with it.
        close(ev("DCOUNT", vec![rf(db()), t("Age"), rf(c(vec![tv("P"), Value::Empty]))]), 2.0);
        close(ev("DCOUNT", vec![rf(db()), t("Age"), rf(c(vec![tv("=P"), Value::Empty]))]), 0.0);
        close(ev("DSUM", vec![rf(db()), t("Yield"), rf(c(vec![Value::Empty, tv(">=13")]))]), 33.0);
        close(ev("DSUM", vec![rf(db()), t("Yield"), rf(c(vec![Value::Empty, nv(18.0)]))]), 14.0);
        close(ev("DSUM", vec![rf(db()), t("Yield"), rf(c(vec![Value::Empty, Value::Empty]))]), 57.0);
        close(ev("DSUM", vec![rf(db()), t("Yield"), rf(c(vec![tv("*e*"), tv("<10")]))]), 14.0);
        // Header only: every record matches.
        close(ev("DCOUNTA", vec![rf(db()), t("tree"), rf(arr(vec![vec![tv("Tree")]]))]), 6.0);
        // Unknown criteria label never matches.
        close(ev("DCOUNT", vec![rf(db()), t("Age"), rf(arr(vec![vec![tv("Color")], vec![tv("red")]]))]), 0.0);
    }

    #[test]
    fn dget_and_empty() {
        let c = |tree: &str| arr(vec![vec![tv("Tree")], vec![tv(tree)]]);
        close(ev("DGET", vec![rf(db()), t("Yield"), rf(c("=Cherry"))]), 9.0);
        is_err(ev("DGET", vec![rf(db()), t("Yield"), rf(c("=Apple"))]), CellError::Num);
        is_err(ev("DGET", vec![rf(db()), t("Yield"), rf(c("=Plum"))]), CellError::Value);
        is_err(ev("DAVERAGE", vec![rf(db()), t("Yield"), rf(c("=Plum"))]), CellError::Div0);
        close(ev("DMAX", vec![rf(db()), t("Yield"), rf(c("=Plum"))]), 0.0);
        close(ev("DPRODUCT", vec![rf(db()), t("Yield"), rf(c("=Plum"))]), 0.0);
        is_err(ev("DSTDEV", vec![rf(db()), t("Yield"), rf(c("=Cherry"))]), CellError::Div0);
        close(ev("DSTDEVP", vec![rf(db()), t("Yield"), rf(c("=Cherry"))]), 0.0);
        is_err(ev("DSUM", vec![e(CellError::Ref), t("Yield"), rf(c("=Cherry"))]), CellError::Ref);
        close(ev("DCOUNTA", vec![rf(db()), empty(), rf(c("=Pear"))]), 2.0);
    }
}
