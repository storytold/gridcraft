//! Conditional-format text work is shared, refusals are not inverted, and exports retain ordinary styling.

use gridcraft_core::{CellRef, Value};
use gridcraft_engine::Session;
use gridcraft_engine::cf::CfCache;
use gridcraft_engine::cmd::print::{PrintOptions, layouts, render_pdf};
use gridcraft_engine::model::Workbook;
use gridcraft_pdf::PdfDoc;
use serde_json::json;

/// Creates a blank workbook through the same session API as normal commands.
fn session() -> Session {
    let mut session = Session::new();
    session.new_workbook();
    session
}

/// Adds one visible text rule without relying on any default colour values.
fn text_rule(session: &mut Session, range: &str, kind: &str, needle: &str) {
    session.execute("home.conditionalFormat", json!({"range": range, "rule": {"type": kind, "text": needle, "preset": "lightRedFill"}})).unwrap();
}

/// Reports whether the rendering API actually supplied a conditional style.
fn styled(cache: &mut CfCache, workbook: &Workbook, sheet: usize, cell: &str) -> bool {
    cache.look(workbook, sheet, CellRef::parse(cell).unwrap()).and_then(|look| look.style).is_some()
}

/// Four distinct hostile cells consume the shared viewport allowance; other cells are cheap controls.
fn hostile_session() -> Session {
    let mut session = session();
    let long = "a".repeat(32_767);
    session
        .execute("range.setValues", json!({"range": "A1", "values": [["z", 7], [long], [long], [long], [long], ["z"], [null], ["=NA()"], ["=\"\""]]}))
        .unwrap();
    text_rule(&mut session, "A1:A9", "notContainsText", "a*b");
    session
        .execute(
            "home.conditionalFormat",
            json!({"range": "B1", "rule": {"type": "cellIs", "operator": "greaterThan", "value": "5", "preset": "lightRedFill"}}),
        )
        .unwrap();
    session
}

/// Visits distinct backtracking inputs instead of repeatedly hitting the same memoized refusal.
fn exhaust(cache: &mut CfCache, workbook: &Workbook) {
    for row in 2..=5 {
        assert!(!styled(cache, workbook, 0, &format!("A{row}")), "an exhausted NotContains search must not format A{row}");
    }
}

/// The allowance spans cells, and both known results and refusals stay stable when it runs out.
#[test]
fn text_work_is_shared_and_repeated_outcomes_stay_stable() {
    let session = hostile_session();
    let workbook = &session.doc().unwrap().wb;
    let mut cache = CfCache::new();
    assert!(styled(&mut cache, workbook, 0, "A1"), "a completed non-match activates NotContains");
    exhaust(&mut cache, workbook);
    assert!(!styled(&mut cache, workbook, 0, "A6"), "a fresh cell cannot renew the viewport allowance");
    assert!(styled(&mut CfCache::new(), workbook, 0, "A6"), "the same cheap non-match completes with a fresh allowance");
    assert!(styled(&mut cache, workbook, 0, "A1"), "a previously formatted title keeps its result after exhaustion");
    assert!(!styled(&mut cache, workbook, 0, "A2"), "a previous refusal stays refused");
}

/// Known blank/error outcomes and unrelated numeric rules do not depend on wildcard work credits.
#[test]
fn text_exhaustion_keeps_blank_error_and_non_text_rules_working() {
    let session = hostile_session();
    let workbook = &session.doc().unwrap().wb;
    let mut cache = CfCache::new();
    exhaust(&mut cache, workbook);
    assert!(!styled(&mut cache, workbook, 0, "A6"), "the wildcard bank is exhausted");
    for cell in ["A7", "A8", "A9"] {
        assert!(styled(&mut cache, workbook, 0, cell), "NotContains still resolves the known blank/error case {cell}");
    }
    assert!(styled(&mut cache, workbook, 0, "B1"), "a numeric condition remains effective");
    assert_eq!(workbook.sheet(0).unwrap().value(CellRef::parse("B1").unwrap()), Value::number(7.0));
}

/// Equal rule and cell indices on different sheets must not share a cached text result.
#[test]
fn text_result_keys_distinguish_sheets() {
    let mut session = session();
    session.execute("cell.set", json!({"cell": "A1", "input": "z"})).unwrap();
    text_rule(&mut session, "A1", "notContainsText", "a*b");
    session.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
    session.execute("cell.set", json!({"cell": "A1", "input": "ab"})).unwrap();
    text_rule(&mut session, "A1", "notContainsText", "a*b");
    let workbook = &session.doc().unwrap().wb;
    let mut cache = CfCache::new();
    assert!(styled(&mut cache, workbook, 0, "A1"));
    assert!(!styled(&mut cache, workbook, 1, "A1"));
    assert!(styled(&mut cache, workbook, 0, "A1"));
}

/// A maximum-length astral literal is valid input, including its full case-insensitive pattern.
#[test]
fn full_length_unicode_literal_fits_the_per_match_allowance() {
    let mut session = session();
    let text = "\u{10400}".repeat(32_767);
    let needle = "\u{10428}".repeat(32_767);
    session.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
    text_rule(&mut session, "A1", "containsText", &needle);
    assert!(styled(&mut CfCache::new(), &session.doc().unwrap().wb, 0, "A1"));
}

/// Uses the PDF writer itself to obtain its current RGB operator spelling.
fn fill_operator(color: [u8; 3]) -> String {
    let mut probe = PdfDoc::new();
    probe.add_page(10.0, 10.0).fill_rect(0.0, 0.0, 1.0, 1.0, color);
    let bytes = probe.finish();
    String::from_utf8_lossy(&bytes).lines().find(|line| line.ends_with(" rg")).unwrap().to_string()
}

/// Real multipage PDF output uses the sheet export allowance and repeats each styled title.
#[test]
fn multipage_pdf_keeps_text_fills_and_repeated_titles() {
    let mut session = session();
    let body = format!("ordinary{}", "a".repeat(3000));
    let mut rows = vec![json!(["ordinary"])];
    rows.extend((0..500).map(|_| json!([body])));
    session.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    text_rule(&mut session, "A1:A501", "containsText", "ordinary");
    session.execute("pageLayout.pageSetup", json!({"printArea": "A1:A501", "titleRows": "1:1"})).unwrap();
    let workbook = &session.doc().unwrap().wb;
    let mut viewport = CfCache::new();
    let viewport_matches = (1..=501).filter(|row| styled(&mut viewport, workbook, 0, &format!("A{row}"))).count();
    assert!(viewport_matches < 501, "the fixture exceeds one viewport's work allowance");
    let mut export = CfCache::for_export();
    let first = export.look(workbook, 0, CellRef::parse("A1").unwrap()).unwrap();
    let color = first.style.unwrap().fill.fg.resolve(&workbook.theme).unwrap();
    assert!((2..=501).all(|row| styled(&mut export, workbook, 0, &format!("A{row}"))), "ordinary text fits the larger export allowance");
    let layouts = layouts(workbook, &PrintOptions::default());
    assert_eq!(layouts.len(), 1);
    let (bytes, pages) = render_pdf(workbook, &layouts, "Conditional text report", "");
    assert!(pages > 1, "the fixture exercises repeated titles across pages");
    let pdf = String::from_utf8_lossy(&bytes);
    let operator = fill_operator(color);
    let fills = pdf.lines().filter(|line| *line == operator.as_str()).count();
    assert_eq!(fills, 500 + pages, "every body cell and each page's repeated title retain their fill");
    assert_eq!(pdf.matches("(ordinary) Tj").count(), pages, "the title is printed on each page");
}
