use super::calc::formula_text;
use super::package::{REBUILT_KINDS, relationship_kinds, validate_package};
use super::{
    BTreeSet, Category, Cell, CellError, CellRef, CfOperator, Diffs, Options, RoundTrip, Sheet, Validation, ValidationKind, Value, Workbook,
    number_text, quote_sheet,
};

// ---------------------------------------------------------------- round trip

/// Writes `wb` with GridCraft's XLSX writer, validates the package against itself and against
/// `source` (the file `wb` was read from), reads it back and compares.
pub fn round_trip(wb: &Workbook, source: &[u8], opts: &Options) -> RoundTrip {
    let bytes = match gridcraft_engine::io::save_bytes(wb, "roundtrip.xlsx") {
        Ok(b) => b,
        Err(e) => return RoundTrip { error: Some(format!("write failed: {e}")), categories: vec![] },
    };
    let mut package = Diffs::new("package", opts.max_report);
    match validate_package(&bytes) {
        Ok(issues) => issues.into_iter().for_each(|i| package.push(|| i)),
        Err(e) => package.push(|| e),
    }
    if let (Ok(before), Ok(after)) = (relationship_kinds(source), relationship_kinds(&bytes)) {
        for (key, kind) in &before {
            if !after.contains_key(key) && !REBUILT_KINDS.contains(&key.as_str()) {
                package.push(|| format!("lost relationship {kind}"));
            }
        }
    }
    let back = match gridcraft_engine::io::open_bytes("roundtrip.xlsx", &bytes) {
        Ok((b, _)) => b,
        Err(e) => return RoundTrip { error: Some(format!("reading the written file failed: {e}")), categories: vec![package.done()] },
    };
    let mut categories = diff_workbooks(wb, &back, opts);
    categories.push(package.done());
    RoundTrip { error: None, categories }
}

pub(super) fn value_show(v: &Value) -> String {
    match v {
        Value::Empty => "(blank)".to_string(),
        Value::Text(t) => serde_json::to_string(t.as_ref()).unwrap_or_default(),
        Value::Number(n) => number_text(*n),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Value::Error(e) => e.as_str().to_string(),
        Value::Array(_) => v.scalar().display(),
    }
}

fn opt_formula(c: Option<&Cell>) -> String {
    c.filter(|c| c.formula.is_some()).map(formula_text).unwrap_or_default()
}

/// Per-sheet comparison of two lists, reporting what one side has and the other lacks.
fn diff_sets(d: &mut Diffs, sheet: &str, a: &BTreeSet<String>, b: &BTreeSet<String>) {
    for x in a.difference(b) {
        d.push(|| format!("{sheet}: lost {x}"));
    }
    for x in b.difference(a) {
        d.push(|| format!("{sheet}: gained {x}"));
    }
}

/// A sheet's conditional-format rules in precedence order, as comparable text. The priority number
/// itself is left out, so renumbering without changing the order isn't a difference.
fn cond_format_list(s: &Sheet) -> Vec<String> {
    let mut cfs: Vec<_> = s.cond_formats.iter().collect();
    cfs.sort_by_key(|c| c.priority);
    cfs.iter().map(|c| format!("{:?} {:?} stopIfTrue={}", c.ranges, c.rule, c.stop_if_true)).collect()
}

fn diff_count(d: &mut Diffs, sheet: &str, a: usize, b: usize) {
    if a != b {
        d.push(|| format!("{sheet}: {a} → {b}"));
    }
}

/// The writer/reader canonicalize formula syntax, but literal list contents are data.
fn validation_formula(f: &str) -> String {
    let f = f.trim();
    if f.starts_with('"') {
        return f.to_owned();
    }
    let f = f.strip_prefix('=').unwrap_or(f).trim();
    gridcraft_engine::formula::parse(f)
        .map(|e| {
            // The XLSX writer spells trim references as equivalent TRIMRANGE calls.
            // Normalize inside expressions too, without equating different trim modes.
            use gridcraft_engine::formula::{Expr, UnOp};
            let e = e.map(&mut |e| match e {
                Expr::Unary(UnOp::Trim(mode), range) => {
                    let code = Expr::Number(f64::from(mode.code()));
                    Expr::Call("TRIMRANGE".into(), vec![*range, code.clone(), code])
                }
                other => other,
            });
            gridcraft_engine::formula::print(&e)
        })
        .unwrap_or_else(|_| f.to_owned())
}

fn canonical_validation(v: &Validation) -> Validation {
    let mut v = v.clone();
    v.f1 = validation_formula(&v.f1);
    v.f2 = v.f2.as_deref().map(validation_formula).filter(|f| !f.is_empty());
    // These validation kinds do not use an operator, and the XLSX writer omits it.
    if matches!(v.kind, ValidationKind::Any | ValidationKind::List | ValidationKind::Custom) {
        v.op = CfOperator::Between;
    }
    v
}

fn validation_description(v: Option<&Validation>) -> String {
    match v {
        Some(v) => {
            let ranges = v.ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(" ");
            format!("[{ranges}] {v:?}")
        }
        None => "(none)".into(),
    }
}

fn diff_validations(d: &mut Diffs, sheet: &str, a: &[Validation], b: &[Validation]) {
    // The first matching rule wins, and the first range anchors relative formulas. Keep both
    // orders and compare range metadata directly, never the potentially billions of cells.
    for i in 0..a.len().max(b.len()) {
        let (before, after) = (a.get(i), b.get(i));
        if before == after {
            continue;
        }
        if let (Some(x), Some(y)) = (before, after)
            && canonical_validation(x) == canonical_validation(y)
        {
            continue;
        }
        d.push(|| format!("{sheet}: validation {}: {} → {}", i + 1, validation_description(before), validation_description(after)));
    }
}

/// A value as the file format can hold it: `#SPILL!`/`#CALC!` are cached as `#VALUE!` and an
/// unresolved circular reference as 0 (what Excel writes), so those aren't round-trip losses.
fn as_cached(v: Value) -> Value {
    match v {
        Value::Error(CellError::Spill | CellError::Calc) => Value::Error(CellError::Value),
        Value::Error(CellError::Circ) => Value::Number(0.0),
        v => v,
    }
}

fn num_fmt<'a>(wb: &'a Workbook, c: Option<&Cell>) -> &'a str {
    wb.styles.get(c.map(|c| c.style).unwrap_or_default()).num_fmt.as_str()
}

/// Feature-by-feature comparison of a workbook (`a`) with what reading it back gave (`b`). Sheets
/// are paired by position.
pub fn diff_workbooks(a: &Workbook, b: &Workbook, opts: &Options) -> Vec<Category> {
    let m = opts.max_report;
    let mut sheets = Diffs::new("sheets", m);
    let (an, bn): (Vec<&str>, Vec<&str>) = (a.sheets.iter().map(|s| s.name.as_str()).collect(), b.sheets.iter().map(|s| s.name.as_str()).collect());
    if an != bn {
        sheets.push(|| format!("{an:?} → {bn:?}"));
    }
    let mut values = Diffs::new("values", m);
    let mut formulas = Diffs::new("formulas", m);
    let mut num_fmts = Diffs::new("numFmts", m);
    let mut merges = Diffs::new("merges", m);
    let mut tables = Diffs::new("tables", m);
    let mut cond_formats = Diffs::new("condFormats", m);
    let mut validations = Diffs::new("validations", m);
    let mut charts = Diffs::new("charts", m);
    let mut comments = Diffs::new("comments", m);
    let mut hyperlinks = Diffs::new("hyperlinks", m);
    let mut pivots = Diffs::new("pivots", m);
    for (sa, sb) in a.sheets.iter().zip(b.sheets.iter()) {
        let name = quote_sheet(&sa.name);
        let mut cell = |c: CellRef, ca: Option<&Cell>, cb: Option<&Cell>| {
            let cached = |x: Option<&Cell>| as_cached(x.map(|x| x.value.scalar()).unwrap_or_default());
            let (va, vb) = (cached(ca), cached(cb));
            if va != vb {
                values.push(|| format!("{name}!{}: {} → {}", c.a1(), value_show(&va), value_show(&vb)));
            }
            let (fa, fb) = (opt_formula(ca), opt_formula(cb));
            if fa != fb {
                formulas.push(|| format!("{name}!{}: {fa} → {fb}", c.a1()));
            }
            let (na, nb) = (num_fmt(a, ca), num_fmt(b, cb));
            if na != nb {
                num_fmts.push(|| format!("{name}!{}: {na} → {nb}", c.a1()));
            }
        };
        for (c, ca) in sa.cells.iter() {
            cell(c, Some(ca), sb.cell(c));
        }
        for (c, cb) in sb.cells.iter() {
            if sa.cell(c).is_none() {
                cell(c, None, Some(cb));
            }
        }
        let set = |f: &dyn Fn(&Sheet) -> Vec<String>, s: &Sheet| f(s).into_iter().collect::<BTreeSet<String>>();
        let merge_list = |s: &Sheet| -> Vec<String> { s.merges.iter().map(|r| r.a1()).collect() };
        diff_sets(&mut merges, &name, &set(&merge_list, sa), &set(&merge_list, sb));
        let table_list = |s: &Sheet| -> Vec<String> {
            s.tables
                .iter()
                .map(|t| format!("{} {} [{}]", t.name, t.range.a1(), t.columns.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")))
                .collect()
        };
        diff_sets(&mut tables, &name, &set(&table_list, sa), &set(&table_list, sb));
        let (cfa, cfb) = (cond_format_list(sa), cond_format_list(sb));
        if cfa.len() != cfb.len() {
            cond_formats.push(|| format!("{name}: {} → {}", cfa.len(), cfb.len()));
        } else if let Some((x, y)) = cfa.iter().zip(&cfb).find(|(x, y)| x != y) {
            cond_formats.push(|| format!("{name}: {x} → {y}"));
        }
        diff_validations(&mut validations, &name, &sa.validations, &sb.validations);
        let chart_list = |s: &Sheet| s.charts.iter().map(|c| format!("{:?} ({} series)", c.kind, c.series.len())).collect::<Vec<_>>();
        let (ca, cb) = (chart_list(sa), chart_list(sb));
        if ca != cb {
            charts.push(|| format!("{name}: {ca:?} → {cb:?}"));
        }
        diff_count(&mut comments, &name, sa.comments.len(), sb.comments.len());
        diff_count(&mut hyperlinks, &name, sa.hyperlinks.len(), sb.hyperlinks.len());
        diff_count(&mut pivots, &name, sa.pivots.len(), sb.pivots.len());
    }
    let mut names = Diffs::new("names", m);
    let name_list = |wb: &Workbook| -> BTreeSet<String> {
        wb.names
            .iter()
            .map(|n| {
                let scope = n.scope.map(|i| format!("{}!", quote_sheet(wb.sheet(i).map(|s| s.name.as_str()).unwrap_or("?")))).unwrap_or_default();
                format!("{scope}{}={}", n.name, n.formula)
            })
            .collect()
    };
    diff_sets(&mut names, "workbook", &name_list(a), &name_list(b));
    vec![
        sheets.done(),
        values.done(),
        formulas.done(),
        num_fmts.done(),
        merges.done(),
        names.done(),
        tables.done(),
        cond_formats.done(),
        validations.done(),
        charts.done(),
        comments.done(),
        hyperlinks.done(),
        pivots.done(),
    ]
}
