//! Hyperlink fidelity compares destinations and routing, not merely the number of links.

#![allow(clippy::unwrap_used)]

use gridcraft_engine::model::Hyperlink;

use super::*;

fn book(links: &[(&str, &str, Option<&str>, bool)]) -> Workbook {
    let mut wb = Workbook::new();
    for (cell, target, tooltip, external) in links {
        wb.sheet_mut(0)
            .unwrap()
            .hyperlinks
            .insert(CellRef::parse(cell).unwrap(), Hyperlink { target: (*target).into(), tooltip: tooltip.map(str::to_string), external: *external });
    }
    wb
}

fn links_diff(before: &Workbook, after: &Workbook, max_report: usize) -> Category {
    diff_workbooks(before, after, &Options { max_report, ..Default::default() }).into_iter().find(|c| c.name == "hyperlinks").unwrap()
}

#[test]
fn changing_a_destination_without_changing_link_count_is_detected() {
    let before = book(&[("A1", "https://example.com/original", None, true)]);
    let after = book(&[("A1", "https://example.com/replacement", None, true)]);
    assert!(links_diff(&before, &after, 20).diffs > 0);
}

#[test]
fn moving_a_link_to_another_cell_is_detected() {
    let before = book(&[("A1", "Sheet1!D7", None, false)]);
    let after = book(&[("B1", "Sheet1!D7", None, false)]);
    assert!(links_diff(&before, &after, 20).diffs > 0);
}

#[test]
fn changing_a_tooltip_without_changing_link_count_is_detected() {
    let before = book(&[("A1", "https://example.com/", Some("Original description"), true)]);
    let after = book(&[("A1", "https://example.com/", Some("Changed description"), true)]);
    assert!(links_diff(&before, &after, 20).diffs > 0);
}

#[test]
fn bare_file_target_and_workbook_location_have_different_routing() {
    // A bare name can mean either an external file or a name in the current workbook.
    let before = book(&[("A1", "Budget.xlsx", None, true)]);
    let after = book(&[("A1", "Budget.xlsx", None, false)]);
    assert!(links_diff(&before, &after, 20).diffs > 0);
}

#[test]
fn insertion_order_does_not_change_hyperlink_fidelity() {
    let first = ("A1", "Sheet1!D7", Some("Go to total"), false);
    let second = ("C2", "https://example.com/", None, true);
    assert_eq!(links_diff(&book(&[first, second]), &book(&[second, first]), 20).diffs, 0);
}

#[test]
fn writer_equivalent_locations_and_inferred_external_routes_are_clean() {
    let before = book(&[
        ("A1", "  #Sheet1!D7  ", None, false),
        ("A2", "https://example.com/page#section", None, false),
        ("A3", "mailto:test@example.com", None, false),
        ("A4", "file:///C:/Reports/Budget.xlsx#Sheet1!A1", None, false),
        ("A5", "\\\\server\\share\\Budget.xlsx", None, false),
        ("A6", "https://example.com/page#", None, true),
    ]);
    let after = book(&[
        ("A1", "Sheet1!D7", None, false),
        ("A2", "https://example.com/page#section", None, true),
        ("A3", "mailto:test@example.com", None, true),
        ("A4", "file:///C:/Reports/Budget.xlsx#Sheet1!A1", None, true),
        ("A5", "\\\\server\\share\\Budget.xlsx", None, true),
        ("A6", "https://example.com/page", None, true),
    ]);
    let category = links_diff(&before, &after, 20);
    assert_eq!(category.diffs, 0, "{category:?}");
}

#[test]
fn real_xlsx_round_trip_keeps_locations_external_fragments_and_escaped_tooltips() {
    let before = book(&[
        ("A1", "#Sheet1!D7", Some("Total <limit> & \"quoted\""), false),
        ("B2", "https://example.com/report?a=1&b=2#summary", Some("Web & report"), false),
        ("C3", "Budget.xlsx#'Annual Plan'!B4", Some("External file"), true),
        ("D4", "https://example.com/empty-fragment#", Some(""), true),
    ]);
    let bytes = gridcraft_engine::io::save_bytes(&before, "links.xlsx").unwrap();
    let (after, _) = gridcraft_engine::io::open_bytes("links.xlsx", &bytes).unwrap();
    let links = &after.sheet(0).unwrap().hyperlinks;
    let internal = &links[&CellRef::parse("A1").unwrap()];
    assert_eq!(internal.target, "Sheet1!D7");
    assert_eq!(internal.tooltip.as_deref(), Some("Total <limit> & \"quoted\""));
    assert!(!internal.external);
    let file = &links[&CellRef::parse("C3").unwrap()];
    assert_eq!(file.target, "Budget.xlsx#'Annual Plan'!B4");
    assert!(file.external);
    assert_eq!(links[&CellRef::parse("D4").unwrap()].tooltip.as_deref(), Some(""));
    let category = links_diff(&before, &after, 20);
    assert_eq!(category.diffs, 0, "{category:?}");
    let report = check_bytes("links.xlsx", &bytes, &Options::default());
    assert!(report.is_clean(), "{report:?}");
}

#[test]
fn capped_examples_still_count_all_changed_links() {
    let before = book(&[("A1", "Sheet1!D1", None, false), ("A2", "Sheet1!D2", None, false), ("A3", "Sheet1!D3", None, false)]);
    let after = book(&[("A1", "Sheet1!E1", None, false), ("A2", "Sheet1!E2", None, false), ("A3", "Sheet1!E3", None, false)]);
    let full = links_diff(&before, &after, 20);
    let capped = links_diff(&before, &after, 1);
    assert!(full.diffs >= 3, "{full:?}");
    assert_eq!(capped.diffs, full.diffs);
    assert_eq!(capped.examples.len(), 1);
}

#[test]
fn meaningful_target_and_tooltip_distinctions_are_not_normalized_away() {
    let cases = [
        ("https://example.com/Report", "https://example.com/report", None, None),
        ("https://example.com/a%2Fb", "https://example.com/a/b", None, None),
        ("https://example.com/page#first", "https://example.com/page#second", None, None),
        ("https://example.com/", "https://example.com/", None, Some("")),
    ];
    for (original, changed, original_tip, changed_tip) in cases {
        let before = book(&[("C7", original, original_tip, true)]);
        let after = book(&[("C7", changed, changed_tip, true)]);
        let category = links_diff(&before, &after, 20);
        assert_eq!(category.diffs, 1, "{original:?} {original_tip:?} versus {changed:?} {changed_tip:?}: {category:?}");
        let description = &category.examples[0];
        assert!(description.contains("C7"), "missing cell in {description}");
        if original == changed {
            assert!(description.to_ascii_lowercase().contains("tooltip"), "missing tooltip distinction in {description}");
        } else {
            assert!(description.contains(original) && description.contains(changed), "missing destinations in {description}");
        }
    }
}

#[test]
fn zero_example_limit_and_empty_target_entries_do_not_hide_changes() {
    let before = book(&[("A1", "Budget.xlsx", None, true), ("B2", "", None, false)]);
    let after = book(&[("A1", "Budget.xlsx", None, false)]);
    let full = links_diff(&before, &after, 20);
    let capped = links_diff(&before, &after, 0);
    assert_eq!(full.diffs, 2, "routing change and empty-target removal each matter: {full:?}");
    assert_eq!(capped.diffs, full.diffs);
    assert!(capped.examples.is_empty());
    let routing = full.examples.iter().find(|e| e.contains("A1")).unwrap();
    assert!(routing.contains("Budget.xlsx"));
    let description = routing.to_ascii_lowercase();
    assert!(description.contains("external") && description.contains("internal"), "missing routing distinction in {routing}");
    assert!(full.examples.iter().any(|e| e.contains("B2")), "missing removed cell in {full:?}");
}
