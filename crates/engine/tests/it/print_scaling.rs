//! Print-job scaling overrides are shared by preview, PDF export and native-print input.
use gridcraft_engine::Session;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Two contrasting print areas expose width-only and height-only fitting separately.
fn workbook() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("sheet.rename", json!({"name":"Tall"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"paper":"A4","printArea":"A1:H200","scale":42,"fitToPage":false})).unwrap();
    s.execute("home.insertSheet", json!({"name":"Wide"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"paper":"A4","printArea":"A1:AN20","scale":42,"fitToPage":false})).unwrap();
    s.execute("sheet.activate", json!({"sheet":"Tall"})).unwrap();
    s
}

/// Distinct printed bands for one sheet and axis, independent of the other axis's pages.
fn bands(preview: &Value, sheet: &str, axis: &str) -> BTreeSet<String> {
    preview["pageList"].as_array().unwrap().iter().filter(|p| p["sheet"] == sheet).map(|p| p[axis].as_str().unwrap().to_owned()).collect()
}

/// No Scaling overrides a saved percentage for this job without rewriting the workbook.
#[test]
fn print_scaling_none_resets_only_the_job() {
    let mut s = workbook();
    let before = serde_json::to_value(&s.doc().unwrap().wb).unwrap();
    let revision = s.doc().unwrap().revision;
    let dirty = s.doc().unwrap().is_dirty();
    let preview = s.execute("file.printPreview", json!({"sheets":"all","scaling":"none"})).unwrap();
    for p in preview["pageList"].as_array().unwrap() {
        assert_eq!(p["scale"], 100.0, "No Scaling must not retain the saved 42 percent");
    }
    assert_eq!(serde_json::to_value(&s.doc().unwrap().wb).unwrap(), before);
    assert_eq!(s.doc().unwrap().revision, revision);
    assert_eq!(s.doc().unwrap().is_dirty(), dirty);
}

/// Width, height and whole-sheet fitting constrain the intended axis on every printed sheet.
#[test]
fn print_scaling_axes_match_pdf_and_print_without_changing_page_setup() {
    let mut s = workbook();
    let before = serde_json::to_value(&s.doc().unwrap().wb).unwrap();
    for mode in ["pageSetup", "none", "columns", "rows", "sheet"] {
        let params = json!({"sheets":"all","scaling":mode});
        let p = s.execute("file.printPreview", params.clone()).unwrap();
        for sheet in ["Tall", "Wide"] {
            if mode == "columns" || mode == "sheet" {
                assert_eq!(bands(&p, sheet, "columns").len(), 1, "{mode}: {sheet}");
            }
            if mode == "rows" || mode == "sheet" {
                assert_eq!(bands(&p, sheet, "rows").len(), 1, "{mode}: {sheet}");
            }
        }
        if mode == "columns" {
            assert!(bands(&p, "Tall", "rows").len() > 1, "width fitting leaves tall sheets multipage");
        }
        if mode == "rows" {
            assert!(bands(&p, "Wide", "columns").len() > 1, "height fitting leaves wide sheets multipage");
        }
        if mode == "pageSetup" {
            assert!(p["pageList"].as_array().unwrap().iter().all(|page| page["scale"] == 42.0));
        }
        for command in ["file.exportPdf", "file.print"] {
            let pdf = s.execute(command, params.clone()).unwrap();
            assert_eq!(pdf["pages"], p["pages"], "{mode}: {command} matches preview");
            assert!(pdf["bytes"].as_u64().unwrap() > 100);
        }
        assert_eq!(serde_json::to_value(&s.doc().unwrap().wb).unwrap(), before);
    }
    let legacy = s.execute("file.printPreview", json!({"sheets":"all","fitToPage":true})).unwrap();
    assert_eq!(legacy["pages"], 2, "the existing fitToPage API remains compatible");
    let ranged = s.execute("file.printPreview", json!({"range":"A1:B100","scaling":"columns"})).unwrap();
    assert_eq!(bands(&ranged, "Tall", "columns"), BTreeSet::from(["A:B".into()]));
}

/// Invalid or conflicting scaling requests fail before changing any workbook state.
#[test]
fn print_scaling_rejects_invalid_and_ambiguous_modes() {
    let mut s = workbook();
    let before = serde_json::to_value(&s.doc().unwrap().wb).unwrap();
    for value in [json!(null), json!(false), json!(4), json!({}), json!("unknown")] {
        assert!(s.execute("file.printPreview", json!({"scaling":value})).is_err(), "{value}");
    }
    assert!(s.execute("file.printPreview", json!({"scaling":"columns","fitToPage":true})).is_err());
    assert_eq!(serde_json::to_value(&s.doc().unwrap().wb).unwrap(), before);
}

/// Saved fitting remains the default, while No Scaling restores manual breaks for this job.
#[test]
fn print_scaling_saved_fit_and_manual_breaks_remain_independent() {
    let mut s = workbook();
    s.execute("pageLayout.pageSetup", json!({"fitWidth":1,"fitHeight":1,"rowBreaks":[10],"colBreaks":["D"]})).unwrap();
    let before = serde_json::to_value(&s.doc().unwrap().wb).unwrap();
    let saved = s.execute("file.printPreview", json!({})).unwrap();
    assert_eq!(saved["pages"], 1);
    let inherited = s.execute("file.printPreview", json!({"scaling":"pageSetup"})).unwrap();
    assert_eq!(inherited, saved);
    let full = s.execute("file.printPreview", json!({"scaling":"none"})).unwrap();
    assert!(bands(&full, "Tall", "rows").contains("1:9"));
    assert!(bands(&full, "Tall", "columns").contains("A:C"));
    let fit = s.execute("file.printPreview", json!({"scaling":"sheet"})).unwrap();
    assert_eq!(fit["pages"], 1);
    assert_eq!(serde_json::to_value(&s.doc().unwrap().wb).unwrap(), before);
}
