use super::fidelity::value_show;
use super::*;

// ---------------------------------------------------------------- output

#[derive(Default)]
struct Totals {
    files: usize,
    opened: usize,
    failed: usize,
    formula_cells: usize,
    checked: usize,
    matched: usize,
    mismatched: usize,
    no_oracle: usize,
    volatile: usize,
    bool_as_number: usize,
    round_trip_failed: usize,
    round_trip_differing: usize,
    categories: BTreeMap<&'static str, usize>,
    functions: BTreeMap<String, FnStat>,
    signatures: BTreeMap<String, usize>,
}

fn totals(reports: &[FileReport]) -> Totals {
    let mut t = Totals { files: reports.len(), ..Default::default() };
    for r in reports {
        if r.error.is_some() {
            t.failed += 1;
            continue;
        }
        t.opened += 1;
        t.formula_cells += r.calc.formula_cells;
        t.checked += r.calc.checked;
        t.matched += r.calc.matched;
        t.mismatched += r.calc.mismatched;
        t.no_oracle += r.calc.no_oracle;
        t.volatile += r.calc.volatile;
        t.bool_as_number += r.calc.bool_as_number;
        if r.round_trip.error.is_some() {
            t.round_trip_failed += 1;
        }
        if !r.round_trip.is_clean() {
            t.round_trip_differing += 1;
        }
        for c in r.round_trip.differing() {
            *t.categories.entry(c).or_default() += 1;
        }
        for (n, s) in &r.calc.functions {
            t.functions.entry(n.clone()).or_default().add(s);
        }
        for (sig, n) in &r.calc.signatures {
            *t.signatures.entry(sig.clone()).or_default() += n;
        }
    }
    t
}

/// Functions with mismatches, worst first: most primary mismatches, then most mismatches.
fn ranked(functions: &BTreeMap<String, FnStat>) -> Vec<(&String, &FnStat)> {
    let mut v: Vec<(&String, &FnStat)> = functions.iter().filter(|(_, s)| s.mismatched > 0).collect();
    v.sort_by(|a, b| b.1.primary.cmp(&a.1.primary).then(b.1.mismatched.cmp(&a.1.mismatched)).then(a.0.cmp(b.0)));
    v
}

fn functions_json(functions: &BTreeMap<String, FnStat>) -> Json {
    let mut v: Vec<(&String, &FnStat)> = functions.iter().collect();
    v.sort_by(|a, b| b.1.primary.cmp(&a.1.primary).then(b.1.mismatched.cmp(&a.1.mismatched)).then(a.0.cmp(b.0)));
    Json::Array(v.into_iter().map(|(n, s)| json!({"name": n, "checked": s.checked, "mismatched": s.mismatched, "primary": s.primary})).collect())
}

pub fn report_json(reports: &[FileReport], opts: &Options) -> Json {
    let files: Vec<Json> = reports
        .iter()
        .map(|r| {
            let c = &r.calc;
            let mismatches: Vec<Json> = c
                .mismatches
                .iter()
                .map(|m| {
                    json!({
                        "cell": m.cell, "formula": m.formula, "expected": value_json(&m.expected), "actual": value_json(&m.actual),
                        "kind": m.kind, "functions": m.functions, "primary": m.primary,
                    })
                })
                .collect();
            let categories: serde_json::Map<String, Json> = r
                .round_trip
                .categories
                .iter()
                .map(|cat| (cat.name.to_string(), json!({"equal": cat.diffs == 0, "diffs": cat.diffs, "examples": cat.examples})))
                .collect();
            json!({
                "path": r.path,
                "opened": r.error.is_none(),
                "error": r.error,
                "warnings": r.warnings,
                "calc": {
                    "formulaCells": c.formula_cells, "checked": c.checked, "matched": c.matched, "mismatched": c.mismatched,
                    "noOracle": c.no_oracle, "staleCache": c.stale_cache, "volatile": c.volatile, "boolAsNumber": c.bool_as_number,
                    "mismatches": mismatches, "functions": functions_json(&c.functions),
                    "signatures": c.signatures,
                },
                "roundTrip": {"error": r.round_trip.error, "differing": r.round_trip.differing(), "categories": categories},
            })
        })
        .collect();
    let t = totals(reports);
    json!({
        "schema": SCHEMA,
        "tolerance": opts.tolerance,
        "files": files,
        "totals": {
            "files": t.files, "opened": t.opened, "failedToOpen": t.failed, "formulaCells": t.formula_cells, "checked": t.checked,
            "matched": t.matched, "mismatched": t.mismatched, "noOracle": t.no_oracle, "volatile": t.volatile, "boolAsNumber": t.bool_as_number,
            "roundTripFailed": t.round_trip_failed, "roundTripDiffering": t.round_trip_differing, "categoriesDiffering": t.categories,
        },
        "functions": functions_json(&t.functions),
        "signatures": t.signatures,
    })
}

/// One report line: line breaks and tabs shown escaped so that they stay visible as differences.
fn one_line(s: &str) -> String {
    s.replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

/// `…tail` of a long label.
fn fit(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w { s.to_string() } else { format!("…{}", s.chars().skip(n + 1 - w).collect::<String>()) }
}

fn print_file_reports(reports: &[FileReport]) {
    for r in reports {
        outln!("== {}", r.path);
        if let Some(e) = &r.error {
            outln!("  can't open: {e}");
            continue;
        }
        let c = &r.calc;
        outln!(
            "  formulas {}: {} checked, {} matched, {} mismatched, {} no oracle, {} volatile",
            c.formula_cells,
            c.checked,
            c.matched,
            c.mismatched,
            c.no_oracle,
            c.volatile
        );
        if c.stale_cache {
            outln!("  (the producer asked for a full recalculation on load: its cached values aren't results)");
        }
        if c.bool_as_number > 0 {
            outln!("  ({} matched TRUE/FALSE results were saved by the producer as 1/0, without t=\"b\")", c.bool_as_number);
        }
        for m in &c.mismatches {
            let tag = if m.primary { "mismatch" } else { "  (downstream)" };
            outln!(
                "  {tag} {}  {}  expected {}  got {}  [{}]",
                m.cell,
                one_line(&m.formula),
                value_show(&m.expected),
                value_show(&m.actual),
                m.functions.join(" ")
            );
        }
        if c.mismatched > c.mismatches.len() {
            outln!("  … {} more mismatches", c.mismatched - c.mismatches.len());
        }
        let rt = &r.round_trip;
        if let Some(e) = &rt.error {
            outln!("  round trip: {e}");
        }
        let differing: Vec<&Category> = rt.categories.iter().filter(|c| c.diffs > 0).collect();
        if differing.is_empty() && rt.error.is_none() {
            outln!("  round trip: all equal");
        }
        for cat in differing {
            outln!("  round trip {}: {} difference(s)", cat.name, cat.diffs);
            for e in &cat.examples {
                outln!("    {}", one_line(e));
            }
            if cat.diffs > cat.examples.len() {
                outln!("    … {} more", cat.diffs - cat.examples.len());
            }
        }
    }
}

fn print_file_table(reports: &[FileReport]) {
    let w = reports.iter().map(|r| r.path.chars().count()).max().unwrap_or(4).clamp(4, 50);
    outln!(
        "{:<w$}  {:>8}  {:>8}  {:>8}  {:>8}  {:>9}  {:>8}  round trip",
        "file",
        "formulas",
        "checked",
        "matched",
        "mismatch",
        "no-oracle",
        "volatile"
    );
    for r in reports {
        let label = fit(&r.path, w);
        if let Some(e) = &r.error {
            outln!("{label:<w$}  can't open: {}", one_line(&fit(e, 100)));
            continue;
        }
        let c = &r.calc;
        let rt = match (&r.round_trip.error, r.round_trip.differing()) {
            (Some(_), _) => "FAILED".to_string(),
            (None, d) if d.is_empty() => "ok".to_string(),
            (None, d) => d.join(","),
        };
        outln!(
            "{label:<w$}  {:>8}  {:>8}  {:>8}  {:>8}  {:>9}  {:>8}  {rt}",
            c.formula_cells,
            c.checked,
            c.matched,
            c.mismatched,
            c.no_oracle,
            c.volatile
        );
    }
}

fn print_rankings(t: &Totals, opts: &Options) {
    let top = ranked(&t.functions);
    if !top.is_empty() {
        outln!("");
        outln!("top mismatching functions (primary = no mismatching precedent):");
        outln!("  {:<20}  {:>8}  {:>10}  {:>8}", "function", "primary", "mismatched", "checked");
        for (n, s) in top.iter().take(opts.max_report.max(1)) {
            outln!("  {:<20}  {:>8}  {:>10}  {:>8}", n, s.primary, s.mismatched, s.checked);
        }
    }
    let mut sigs: Vec<(&String, &usize)> = t.signatures.iter().collect();
    sigs.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    if !sigs.is_empty() {
        outln!("");
        outln!("mismatch signatures (expected → GridCraft):");
        for (sig, n) in sigs.iter().take(opts.max_report.max(1)) {
            outln!("  {n:>8}  {sig}");
        }
    }
}

fn print_totals(t: &Totals) {
    outln!("");
    outln!("files: {} ({} opened, {} failed to open)", t.files, t.opened, t.failed);
    outln!(
        "formula cells: {} ({} checked: {} matched, {} mismatched; {} no oracle; {} volatile)",
        t.formula_cells,
        t.checked,
        t.matched,
        t.mismatched,
        t.no_oracle,
        t.volatile
    );
    if t.bool_as_number > 0 {
        outln!("booleans saved as 1/0 by the producer: {} (counted as matched)", t.bool_as_number);
    }
    let cats: Vec<String> = t.categories.iter().map(|(c, n)| format!("{c} {n}")).collect();
    outln!(
        "round trip: {} of {} files differ{}{}",
        t.round_trip_differing,
        t.opened,
        if t.round_trip_failed > 0 { format!(" ({} failed)", t.round_trip_failed) } else { String::new() },
        if cats.is_empty() { String::new() } else { format!("; categories (files): {}", cats.join(", ")) }
    );
}

pub(super) fn print_human(reports: &[FileReport], opts: &Options) {
    print_file_reports(reports);
    outln!("");
    print_file_table(reports);
    let t = totals(reports);
    print_rankings(&t, opts);
    print_totals(&t);
}
