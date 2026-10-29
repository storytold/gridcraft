//! #1161: contains-text rules use SEARCH wildcards; begins/ends-with rules stay literal.

use gridcraft_core::CellRef;
use gridcraft_engine::{Session, cf::CfCache};
use serde_json::json;

/// Evaluates actual conditional styles through session commands and a shared rendering cache.
fn formatted(kind: &str, needle: &str, values: &[&str]) -> Vec<bool> {
    let mut s = Session::new();
    s.new_workbook();
    let rows: Vec<_> = values.iter().map(|value| json!([value])).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute(
        "home.conditionalFormat",
        json!({"range": format!("A1:A{}", values.len()), "rule": {"type": kind, "text": needle, "preset": "lightRedFill"}}),
    )
    .unwrap();
    let d = s.doc().unwrap();
    let mut cache = CfCache::new();
    (1..=values.len()).map(|row| cache.look(&d.wb, 0, CellRef::parse(&format!("A{row}")).unwrap()).and_then(|look| look.style).is_some()).collect()
}

#[test]
/// Wildcard and escape behavior follows the examples recorded in issue #1161.
fn contains_and_not_contains_follow_search_wildcards() {
    let values = ["abc", "ABC", "Abcdef", "xabc", "a*c", "a?c", "abxc", "ab", "xyz", ""];
    for (needle, expected) in [
        ("a*c", [true, true, true, true, true, true, true, false, false, false]),
        ("a?c", [true, true, true, true, true, true, false, false, false, false]),
        ("a~*c", [false, false, false, false, true, false, false, false, false, false]),
        ("a~?c", [false, false, false, false, false, true, false, false, false, false]),
    ] {
        assert_eq!(formatted("containsText", needle, &values), expected, "contains {needle}");
        assert_eq!(formatted("notContainsText", needle, &values), expected.map(|matched| !matched), "not contains {needle}");
    }
}

#[test]
/// Blank and error cells never match Contains, including empty or all-star patterns.
fn contains_wildcards_handle_escapes_unicode_empty_and_errors() {
    for (needle, value, expected) in [
        ("~~", "a~b", true),
        ("~", "a~b", true),
        ("~", "ab", false),
        ("a~", "xa~y", true),
        ("~~?", "~β", true),
        ("ä?", "xÄβy", true),
        ("ä?", "Ä", false),
        ("*", "", false),
        ("?", "", false),
        ("", "", false),
        ("*", "=\"\"", false),
        ("", "=\"\"", false),
        ("", "abc", true),
        ("a*", "=NA()", false),
        ("*", "=1/0", false),
    ] {
        assert_eq!(formatted("containsText", needle, &[value]), [expected], "contains {needle:?} in {value:?}");
        assert_eq!(formatted("notContainsText", needle, &[value]), [!expected], "not contains {needle:?} in {value:?}");
    }
}

#[test]
/// Begins With and Ends With keep treating wildcard symbols as literal characters.
fn begins_and_ends_with_keep_wildcards_literal() {
    let values = ["a*c", "abc", "a?c", "xa*", "~abc", "abc~", "=NA()"];
    for (kind, needle, expected) in [
        ("beginsWith", "a*", [true, false, false, false, false, false, false]),
        ("beginsWith", "a?", [false, false, true, false, false, false, false]),
        ("beginsWith", "~", [false, false, false, false, true, false, false]),
        ("endsWith", "a*", [false, false, false, true, false, false, false]),
        ("endsWith", "?c", [false, false, true, false, false, false, false]),
        ("endsWith", "~", [false, false, false, false, false, true, false]),
    ] {
        assert_eq!(formatted(kind, needle, &values), expected, "{kind} {needle}");
    }
}

/// Excel's text-rule display formatting skips both truly blank and formula-empty cells.
#[test]
fn blank_and_formula_empty_cells_follow_the_excel_review_table() {
    let values = ["abc", "a*c", "", "=\"\"", "=NA()"];
    for needle in ["*", ""] {
        assert_eq!(formatted("containsText", needle, &values), [true, true, false, false, false]);
        assert_eq!(formatted("notContainsText", needle, &values), [false, false, true, true, true]);
    }
    assert_eq!(formatted("containsText", "?", &values), [true, true, false, false, false]);
    assert_eq!(formatted("containsText", "~*", &values), [false, true, false, false, false]);
}

/// Both sigma spellings and uppercase Greek match without changing Unicode wildcard cardinality.
#[test]
fn greek_sigma_forms_match_in_both_case_directions() {
    for (needle, text) in [("κοσμος", "ΚΟΣΜΟΣ"), ("ΚΟΣΜΟΣ", "κοσμος"), ("κοσμοσ", "κοσμος"), ("κοσμο?", "ΚΟΣΜΟΣ"), ("~ς", "Σ")]
    {
        assert_eq!(formatted("containsText", needle, &[text]), [true], "{needle:?} in {text:?}");
        assert_eq!(formatted("notContainsText", needle, &[text]), [false], "{needle:?} in {text:?}");
    }
    assert_eq!(formatted("containsText", "??", &["İ"]), [false]);
    assert_eq!(formatted("containsText", "?", &["İ"]), [true]);
    assert_eq!(formatted("containsText", "i?", &["İx"]), [false]);
    assert_eq!(formatted("notContainsText", "i?", &["İx"]), [true]);
}

/// Refusing expensive matching must never become a successful inverse text rule.
#[test]
fn exhausted_matching_never_applies_the_negative_style() {
    let text = "a".repeat(32_767);
    assert_eq!(formatted("containsText", "a*b", &[&text]), [false]);
    assert_eq!(formatted("notContainsText", "a*b", &[&text]), [false]);
}
