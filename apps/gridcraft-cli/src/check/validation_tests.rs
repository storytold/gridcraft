//! Data-validation comparisons retain rules, ranges and messages, including meaningful order.

#![allow(clippy::unwrap_used)]

use gridcraft_engine::core::RangeRef;
use gridcraft_engine::model::{CfOperator, ErrorStyle, Validation, ValidationKind};

use super::*;

fn rule(range: &str) -> Validation {
    Validation {
        ranges: vec![RangeRef::parse(range).unwrap()],
        kind: ValidationKind::Whole,
        f1: "1".into(),
        f2: Some("10".into()),
        input_title: "Enter a count".into(),
        input_message: "Use 1 through 10".into(),
        error_title: "Invalid count".into(),
        error_message: "Value is outside the range".into(),
        ..Default::default()
    }
}

fn book(rules: Vec<Validation>) -> Workbook {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().validations = rules;
    wb
}

fn validation_diff(before: &Workbook, after: &Workbook, max_report: usize) -> Category {
    diff_workbooks(before, after, &Options { max_report, ..Default::default() }).into_iter().find(|c| c.name == "validations").unwrap()
}

#[test]
fn same_count_rule_formula_range_options_and_messages_changes_are_detected() {
    let changes: &[(&str, fn(&mut Validation))] = &[
        ("kind", |v| v.kind = ValidationKind::Decimal),
        ("operator", |v| v.op = CfOperator::NotBetween),
        ("formula1", |v| v.f1 = "2".into()),
        ("formula2", |v| v.f2 = Some("20".into())),
        ("range", |v| v.ranges = vec![RangeRef::parse("B1:B10").unwrap()]),
        ("allow blank", |v| v.allow_blank = false),
        ("dropdown", |v| v.in_cell_dropdown = false),
        ("input title", |v| v.input_title = "Different title".into()),
        ("input message", |v| v.input_message = "Different message".into()),
        ("show input", |v| v.show_input = false),
        ("error title", |v| v.error_title = "Different error".into()),
        ("error message", |v| v.error_message = "Different error detail".into()),
        ("error style", |v| v.error_style = ErrorStyle::Warning),
        ("show error", |v| v.show_error = false),
    ];
    let original = rule("A1:A10");
    let before = book(vec![original.clone()]);
    for (name, change) in changes {
        let mut changed = original.clone();
        change(&mut changed);
        let category = validation_diff(&before, &book(vec![changed]), 20);
        assert!(category.diffs > 0, "{name} change was missed: {category:?}");
        assert!(category.examples.iter().any(|e| e.contains("Sheet1")), "missing sheet context: {category:?}");
    }
}

#[test]
fn identical_ordered_rules_are_clean_and_duplicate_multiplicity_is_retained() {
    let first = rule("A1:A10");
    let second = rule("C1:C10");
    let original = book(vec![first.clone(), first.clone(), second.clone()]);
    assert_eq!(validation_diff(&original, &original.clone(), 20).diffs, 0);
    let changed = book(vec![first, second.clone(), second]);
    assert!(validation_diff(&original, &changed, 20).diffs > 0, "equal total counts must not hide different duplicate multiplicities");
}

#[test]
fn overlapping_rule_order_and_relative_formula_base_order_are_preserved() {
    let first = rule("A1:A10");
    let mut second = first.clone();
    second.f1 = "5".into();
    let before = book(vec![first.clone(), second.clone()]);
    let after = book(vec![second, first]);
    assert!(validation_diff(&before, &after, 20).diffs > 0, "the first matching rule controls validation");

    let mut relative = rule("A1:A10");
    relative.kind = ValidationKind::Custom;
    relative.f1 = "A1>0".into();
    relative.f2 = None;
    relative.ranges.push(RangeRef::parse("C1:C10").unwrap());
    let mut reordered = relative.clone();
    reordered.ranges.reverse();
    assert!(validation_diff(&book(vec![relative]), &book(vec![reordered]), 20).diffs > 0, "the first range sets the relative formula base");
}

#[test]
fn actual_xlsx_roundtrip_uses_writer_equivalent_formula_and_operator_spellings() {
    let mut whole = rule("A1:A10");
    whole.f1 = " =1 ".into();
    whole.f2 = Some(" =10 ".into());
    whole.input_message = "Count <10> & \"units\"".into();
    let mut list_source = rule("B1:B10");
    list_source.kind = ValidationKind::List;
    list_source.op = CfOperator::Equal; // The writer omits operators for List/Custom/Any.
    list_source.f1 = " =$D$1:$D$3 ".into();
    list_source.f2 = Some(" ".into());
    let mut literal = rule("C1:C10");
    literal.kind = ValidationKind::List;
    literal.f1 = "\"Red,Blue & Green\"".into();
    literal.f2 = None;
    let mut custom = rule("E1:E10");
    custom.kind = ValidationKind::Custom;
    custom.op = CfOperator::Greater;
    custom.f1 = " =LEN(E1)>0 ".into();
    custom.f2 = None;
    let before = book(vec![whole, list_source, literal, custom]);
    let bytes = gridcraft_engine::io::save_bytes(&before, "validation.xlsx").unwrap();
    let (after, _) = gridcraft_engine::io::open_bytes("validation.xlsx", &bytes).unwrap();
    assert_eq!(after.sheet(0).unwrap().validations.len(), 4);
    let category = validation_diff(&before, &after, 20);
    assert_eq!(category.diffs, 0, "{category:?}");
    let report = check_bytes("validation.xlsx", &bytes, &Options::default());
    assert!(report.is_clean(), "{report:?}");
}

#[test]
fn literal_list_case_and_formula_reference_changes_are_not_normalized_away() {
    for (original, changed) in [("\"Red,Blue\"", "\"red,Blue\""), ("=$D$1:$D$3", "=$E$1:$E$3")] {
        let mut first = rule("A1:A10");
        first.kind = ValidationKind::List;
        first.f1 = original.into();
        first.f2 = None;
        let mut second = first.clone();
        second.f1 = changed.into();
        assert!(validation_diff(&book(vec![first]), &book(vec![second]), 20).diffs > 0);
    }
}

#[test]
fn formula_formatting_is_equivalent_but_literal_list_spaces_are_data() {
    let mut formula = rule("B1:B10");
    formula.kind = ValidationKind::Custom;
    formula.f1 = "=sum( A1 )".into();
    formula.f2 = None;
    let mut formatted = formula.clone();
    formatted.f1 = "SUM(A1)".into();
    assert_eq!(validation_diff(&book(vec![formula]), &book(vec![formatted]), 20).diffs, 0);

    let mut literal = rule("C1:C10");
    literal.kind = ValidationKind::List;
    literal.f1 = "\"Red, Blue\"".into();
    literal.f2 = None;
    let mut trimmed = literal.clone();
    trimmed.f1 = "\"Red,Blue\"".into();
    assert!(validation_diff(&book(vec![literal]), &book(vec![trimmed]), 20).diffs > 0, "spaces inside a list item are meaningful");
}

#[test]
fn report_limits_do_not_hide_total_differences() {
    let rules = vec![rule("A1:A10"), rule("C1:C10"), rule("E1:E10")];
    let mut changed = rules.clone();
    for validation in &mut changed {
        validation.f1 = "3".into();
    }
    let before = book(rules);
    let after = book(changed);
    let full = validation_diff(&before, &after, 20);
    assert!(full.diffs >= 3, "{full:?}");
    for limit in [0, 1] {
        let capped = validation_diff(&before, &after, limit);
        assert_eq!(capped.diffs, full.diffs);
        assert_eq!(capped.examples.len(), limit);
    }
}

#[test]
fn whole_worksheet_ranges_are_compared_without_expanding_cells() {
    let original = rule("A1:XFD1048576");
    let mut changed = original.clone();
    changed.error_message = "Changed message".into();
    let before = book(vec![original]);
    assert_eq!(validation_diff(&before, &before.clone(), 20).diffs, 0);
    assert!(validation_diff(&before, &book(vec![changed]), 1).diffs > 0);
}

#[test]
fn added_removed_and_writer_omitted_rules_remain_visible() {
    let existing = book(vec![rule("C7:C12")]);
    let empty = book(vec![]);
    for (before, after) in [(&existing, &empty), (&empty, &existing)] {
        let category = validation_diff(before, after, 20);
        assert_eq!(category.diffs, 1);
        assert!(category.examples[0].contains("Sheet1") && category.examples[0].contains("C7:C12"), "missing location: {category:?}");
    }

    let mut unranged = rule("A1");
    unranged.ranges.clear();
    let before = book(vec![unranged]);
    let bytes = gridcraft_engine::io::save_bytes(&before, "unranged.xlsx").unwrap();
    let (after, _) = gridcraft_engine::io::open_bytes("unranged.xlsx", &bytes).unwrap();
    assert!(after.sheet(0).unwrap().validations.is_empty(), "the writer omits rules without a range");
    assert_eq!(validation_diff(&before, &after, 20).diffs, 1, "normalization must not erase a missing rule");
}

#[test]
fn invalid_formula_fallback_keeps_distinct_text_and_reports_the_range() {
    for formula in ["SUM(", "AVERAGE("] {
        assert!(gridcraft_engine::formula::parse(formula).is_err(), "fixture must exercise fallback: {formula}");
    }
    let mut original = rule("D9:D12");
    original.kind = ValidationKind::Custom;
    original.f1 = "SUM(".into();
    original.f2 = None;
    let mut formatted = original.clone();
    formatted.f1 = " =SUM( ".into();
    let before = book(vec![original.clone()]);
    assert_eq!(validation_diff(&before, &book(vec![formatted]), 20).diffs, 0);

    let mut changed = original;
    changed.f1 = "AVERAGE(".into();
    let category = validation_diff(&before, &book(vec![changed]), 20);
    assert_eq!(category.diffs, 1);
    let description = &category.examples[0];
    assert!(description.contains("D9:D12") && description.contains("SUM(") && description.contains("AVERAGE("), "{description}");
}

#[test]
fn trim_reference_modes_survive_actual_xlsx_roundtrip() {
    for (reference, mode) in [("A1.:A10", 1), ("A1:.A10", 2), ("A1.:.A10", 3)] {
        let mut validation = rule("B1:B10");
        validation.kind = ValidationKind::Custom;
        validation.f1 = format!("SUM({reference})>0");
        validation.f2 = Some(format!("ROWS({reference})"));
        let before = book(vec![validation]);
        let bytes = gridcraft_engine::io::save_bytes(&before, "trim.xlsx").unwrap();
        let (after, _) = gridcraft_engine::io::open_bytes("trim.xlsx", &bytes).unwrap();
        let reopened = &after.sheet(0).unwrap().validations[0];
        assert_eq!(reopened.f1, format!("SUM(TRIMRANGE(A1:A10,{mode},{mode}))>0"));
        assert_eq!(reopened.f2, Some(format!("ROWS(TRIMRANGE(A1:A10,{mode},{mode}))")));
        let category = validation_diff(&before, &after, 20);
        assert_eq!(category.diffs, 0, "{reference}: {category:?}");
    }
}

#[test]
fn imported_trim_reference_calls_pass_the_complete_fidelity_check() {
    use std::io::{Cursor, Read, Write};

    let modes = [("A1.:A10", "LEADING", 1), ("A1:.A10", "TRAILING", 2), ("A1.:.A10", "ALL", 3)];
    let rules = modes
        .iter()
        .map(|(reference, _, _)| {
            let mut validation = rule("B1:B10");
            validation.kind = ValidationKind::Custom;
            validation.f1 = format!("SUM({reference})>0");
            validation.f2 = None;
            validation
        })
        .collect();
    let bytes = gridcraft_engine::io::save_bytes(&book(rules), "trim.xlsx").unwrap();
    // Construct supported file spellings that import as trim operators. A file produced
    // directly by save_bytes already contains TRIMRANGE and would miss this regression.
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut part = source.by_index(i).unwrap();
        let name = part.name().to_string();
        let mut data = Vec::new();
        part.read_to_end(&mut data).unwrap();
        if name == "xl/worksheets/sheet1.xml" {
            let mut xml = String::from_utf8(data).unwrap();
            for (_, suffix, mode) in modes {
                let call = format!("_xlfn.TRIMRANGE(A1:A10,{mode},{mode})");
                assert!(xml.contains(&call));
                xml = xml.replace(&call, &format!("_xlfn._TRO_{suffix}(A1:A10)"));
            }
            data = xml.into_bytes();
        }
        output.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        output.write_all(&data).unwrap();
    }
    let bytes = output.finish().unwrap().into_inner();
    let (imported, _) = gridcraft_engine::io::open_bytes("trim.xlsx", &bytes).unwrap();
    let validations = &imported.sheet(0).unwrap().validations;
    assert_eq!(validations.len(), modes.len());
    for (validation, (reference, _, _)) in validations.iter().zip(modes) {
        assert_eq!(validation.f1, format!("SUM({reference})>0"));
    }
    let report = check_bytes("trim.xlsx", &bytes, &Options::default());
    assert!(report.is_clean(), "{report:?}");
}

#[test]
fn different_trim_modes_and_ranges_remain_differences() {
    let mut original = rule("B1:B10");
    original.kind = ValidationKind::Custom;
    original.f1 = "SUM(A1:.A10)>0".into();
    original.f2 = None;
    let before = book(vec![original.clone()]);
    for formula in ["SUM(A1.:A10)>0", "SUM(A1.:.A10)>0", "SUM(TRIMRANGE(A1:A10,2,1))>0", "SUM(TRIMRANGE(A1:A20,2,2))>0"] {
        let mut changed = original.clone();
        changed.f1 = formula.into();
        let category = validation_diff(&before, &book(vec![changed]), 20);
        assert_eq!(category.diffs, 1, "{formula}: {category:?}");
    }
}
