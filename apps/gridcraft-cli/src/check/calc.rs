use super::*;

/// Functions whose result depends on the clock, a random generator or the environment.
const VOLATILE: [&str; 6] = ["RAND", "RANDARRAY", "RANDBETWEEN", "NOW", "TODAY", "INFO"];

// ---------------------------------------------------------------- calc oracle

fn functions_of(e: &Expr) -> Vec<String> {
    let mut set = BTreeSet::new();
    e.walk(&mut |x| {
        if let Expr::Call(n, _) = x {
            set.insert(n.clone());
        }
    });
    set.into_iter().collect()
}

fn is_volatile(e: &Expr, volatile_names: &HashSet<String>) -> bool {
    let mut v = false;
    e.walk(&mut |x| match x {
        Expr::Call(n, args) => {
            let filename = n == "CELL" && matches!(args.first(), Some(Expr::Text(t)) if t.eq_ignore_ascii_case("filename"));
            if filename || VOLATILE.contains(&n.as_str()) {
                v = true;
            }
        }
        Expr::Name(n) if volatile_names.contains(&n.to_ascii_uppercase()) => v = true,
        _ => {}
    });
    v
}

/// Upper-cased defined names whose formula is volatile, directly or through other names.
pub(super) fn volatile_names(wb: &Workbook) -> HashSet<String> {
    let parsed: Vec<(String, Option<Expr>)> =
        wb.names.iter().map(|n| (n.name.to_ascii_uppercase(), gridcraft_engine::formula::parse(&n.formula).ok())).collect();
    let mut set = HashSet::new();
    // Each round adds at least one name or stops, so `len + 1` rounds reach the fixed point.
    for _ in 0..=parsed.len() {
        let before = set.len();
        for (name, e) in &parsed {
            if !set.contains(name) && e.as_ref().is_some_and(|e| is_volatile(e, &set)) {
                set.insert(name.clone());
            }
        }
        if set.len() == before {
            break;
        }
    }
    set
}

/// The formula as the formula bar shows it.
pub(super) fn formula_text(cell: &Cell) -> String {
    cell.bar_text()
}

fn cell_name(wb: &Workbook, (sheet, c): Key) -> String {
    let name = wb.sheet(sheet).map(|s| s.name.as_str()).unwrap_or("?");
    format!("{}!{}", quote_sheet(name), c.a1())
}

/// Whether the producer cached GridCraft's logical result as the number 1 or 0. Excel types a
/// logical result `t="b"`; a writer that leaves the type out (`<c><f>TRUE()</f><v>1</v></c>`, as
/// Business Central's "Open in Excel" exports do) stores the same value, read back as a number.
pub fn bool_cached_as_number(expected: &Value, actual: &Value) -> bool {
    matches!((expected, actual), (Value::Number(n), Value::Bool(b)) if *n == if *b { 1.0 } else { 0.0 })
}

/// Whether GridCraft's `actual` agrees with the producer's `expected`.
pub fn values_match(expected: &Value, actual: &Value, tolerance: f64) -> bool {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => (a - b).abs() <= tolerance * a.abs().max(b.abs()).max(1.0),
        (Value::Text(a), Value::Text(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Error(a), Value::Error(b)) => a == b,
        _ => false,
    }
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Empty => "blank",
        Value::Number(_) => "number",
        Value::Text(_) => "text",
        Value::Bool(_) => "bool",
        Value::Error(e) => e.as_str(),
        Value::Array(_) => "array",
    }
}

fn mismatch_kind(expected: &Value, actual: &Value) -> &'static str {
    match (expected, actual) {
        (_, Value::Empty) => "blank",
        (Value::Number(_), Value::Number(_)) => "numeric",
        (Value::Text(_), Value::Text(_)) => "text",
        (Value::Bool(_), Value::Bool(_)) => "bool",
        (Value::Error(_), _) | (_, Value::Error(_)) => "error",
        _ => "type",
    }
}

/// Mismatched cells indexed by (sheet, column) → rows, for precedent-area queries.
struct MismatchIndex {
    cols: HashMap<(usize, u32), BTreeSet<u32>>,
    /// Columns with mismatches per sheet (for very wide areas).
    sheet_cols: HashMap<usize, BTreeSet<u32>>,
}

impl MismatchIndex {
    fn new(keys: &[Key]) -> MismatchIndex {
        let mut cols: HashMap<(usize, u32), BTreeSet<u32>> = HashMap::new();
        let mut sheet_cols: HashMap<usize, BTreeSet<u32>> = HashMap::new();
        for &(s, c) in keys {
            cols.entry((s, c.col)).or_default().insert(c.row);
            sheet_cols.entry(s).or_default().insert(c.col);
        }
        MismatchIndex { cols, sheet_cols }
    }
    /// Whether a mismatched cell other than `own` lies in the area.
    fn any_in(&self, sheet: usize, r0: u32, r1: u32, c0: u32, c1: u32, own: Key) -> bool {
        let Some(cols) = self.sheet_cols.get(&sheet) else { return false };
        cols.range(c0..=c1)
            .any(|&col| self.cols.get(&(sheet, col)).is_some_and(|rows| rows.range(r0..=r1).any(|&row| (sheet, CellRef::new(row, col)) != own)))
    }
}

/// Compares every formula cell of `cached` (as the producer saved it) with `fresh` (recalculated
/// by GridCraft). Without `trust_cache` nothing is compared (see [`CalcReport::stale_cache`]).
pub fn oracle(cached: &Workbook, fresh: &Workbook, calc: &Calc, trust_cache: bool, opts: &Options) -> CalcReport {
    let mut r = CalcReport { stale_cache: !trust_cache, ..Default::default() };
    let vnames = volatile_names(cached);
    let mut volatile: HashSet<Key> = HashSet::new();
    for (si, sh) in cached.sheets.iter().enumerate() {
        for (c, cell) in sh.cells.iter() {
            if let Some(f) = &cell.formula {
                r.formula_cells += 1;
                if f.expr_arc().is_some_and(|e| is_volatile(&e, &vnames)) {
                    volatile.insert((si, c));
                }
            }
        }
    }
    // Everything downstream of a volatile formula is volatile too; each key is pushed once.
    let mut stack: Vec<Key> = volatile.iter().copied().collect();
    let mut deps = Vec::new();
    while let Some((s, c)) = stack.pop() {
        deps.clear();
        calc.graph.dependents(s, c, &mut deps);
        if let Some(range) = fresh.sheet(s).and_then(|sh| sh.spill_ranges.get(&c)) {
            calc.graph.dependents_of_range(s, *range, &mut deps);
        }
        for k in deps.drain(..) {
            if volatile.insert(k) {
                stack.push(k);
            }
        }
    }

    let mut bad: Vec<(Key, Vec<String>)> = Vec::new();
    for (si, sh) in cached.sheets.iter().enumerate() {
        for (c, cell) in sh.cells.iter() {
            let Some(f) = &cell.formula else { continue };
            if volatile.contains(&(si, c)) {
                r.volatile += 1;
                continue;
            }
            if cell.value.is_empty() || !trust_cache {
                r.no_oracle += 1;
                continue;
            }
            r.checked += 1;
            let actual = fresh.sheet(si).and_then(|s| s.cell(c)).map(|x| x.value.scalar()).unwrap_or_default();
            let funcs = match f.expr_arc() {
                Some(e) => {
                    let names = functions_of(&e);
                    if names.is_empty() { vec!["(no function)".to_string()] } else { names }
                }
                None => vec!["(unparsed)".to_string()],
            };
            let bool_as_number = bool_cached_as_number(&cell.value, &actual);
            let ok = bool_as_number || values_match(&cell.value, &actual, opts.tolerance);
            if bool_as_number {
                r.bool_as_number += 1;
            }
            for n in &funcs {
                let st = r.functions.entry(n.clone()).or_default();
                st.checked += 1;
                if !ok {
                    st.mismatched += 1;
                }
            }
            if ok {
                r.matched += 1;
            } else {
                r.mismatched += 1;
                *r.signatures.entry(format!("{} → {}", type_name(&cell.value), type_name(&actual))).or_default() += 1;
                bad.push(((si, c), funcs));
            }
        }
    }

    // A mismatch is primary when none of its precedent cells mismatch.
    let keys: Vec<Key> = bad.iter().map(|(k, _)| *k).collect();
    let index = MismatchIndex::new(&keys);
    let mut listed: Vec<(bool, Key, Vec<String>)> = Vec::with_capacity(bad.len());
    for (k, funcs) in bad {
        let upstream = calc
            .graph
            .precedents_of(k)
            .iter()
            .any(|a| index.any_in(a.sheet, a.range.start.row, a.range.end.row, a.range.start.col, a.range.end.col, k));
        if !upstream {
            for n in &funcs {
                r.functions.entry(n.clone()).or_default().primary += 1;
            }
        }
        listed.push((!upstream, k, funcs));
    }
    // Primary mismatches first, each group in sheet then row-major order (already sorted).
    listed.sort_by_key(|(primary, _, _)| !*primary);
    for (primary, k, functions) in listed.into_iter().take(opts.max_report) {
        let cell = cached.sheet(k.0).and_then(|s| s.cell(k.1));
        let expected = cell.map(|c| c.value.clone()).unwrap_or_default();
        let actual = fresh.sheet(k.0).and_then(|s| s.cell(k.1)).map(|x| x.value.scalar()).unwrap_or_default();
        r.mismatches.push(Mismatch {
            cell: cell_name(cached, k),
            formula: cell.map(formula_text).unwrap_or_default(),
            kind: mismatch_kind(&expected, &actual),
            expected,
            actual,
            functions,
            primary,
        });
    }
    r
}
