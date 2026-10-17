#![allow(clippy::unwrap_used)]

use super::*;
use gridcraft_core::Value;
use gridcraft_model::{Cell, Formula};
use serde_json::json;

#[test]
fn cached_import_preserves_values_and_edits_update_dependents() {
    let mut wb = Workbook::new();
    let sh = wb.sheet_mut(0).unwrap();
    sh.cells.set(cell(0, 0), Cell::value(Value::Number(2.0)));
    let mut formula = Cell::formula(Formula::new("A1*3"));
    formula.value = Value::Number(999.0);
    sh.cells.set(cell(0, 1), formula);
    let doc = DocState::with_calculation(wb, None, "cached.xlsx".into(), true);
    assert!(!doc.is_dirty());
    assert_eq!(doc.wb.sheet(0).unwrap().value(cell(0, 1)), Value::Number(999.0));
    let mut session = Session::new();
    session.add_document(doc);
    session.execute("cell.set", json!({"cell":"A1","input":"4"})).unwrap();
    assert_eq!(session.doc().unwrap().wb.sheet(0).unwrap().value(cell(0, 1)), Value::Number(12.0));
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(session.doc().unwrap().wb.sheet(0).unwrap().value(cell(0, 0)), Value::Number(2.0));
}

#[test]
fn binary_import_is_equivalent_and_invalid_import_keeps_existing_document() {
    let mut source = Session::new();
    source.new_workbook();
    source.execute("cell.set", json!({"cell":"A1","input":"=1+2"})).unwrap();
    let result = source.execute("file.saveBytes", json!({})).unwrap();
    let bytes = io::base64_decode(result["base64"].as_str().unwrap()).unwrap();
    assert!(gridcraft_xlsx::read_xlsx(&bytes).unwrap().1.cached_results_complete);
    let mut target = Session::new();
    target.new_workbook();
    target.open_bytes("sample.xlsx", &bytes).unwrap();
    assert_eq!(target.documents().len(), 1);
    assert_eq!(target.doc().unwrap().wb.sheet(0).unwrap().value(cell(0, 0)), Value::Number(3.0));
    let uid = target.doc().unwrap().uid;
    assert!(target.open_bytes("broken.xlsx", b"PK\x03\x04broken").is_err());
    assert_eq!(target.doc().unwrap().uid, uid);
    assert_eq!(target.doc().unwrap().wb.sheet(0).unwrap().value(cell(0, 0)), Value::Number(3.0));
}
use std::io::Write as _;

fn workbook_with_sheet(sheet: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, xml) in [
        (
            "xl/workbook.xml",
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
        ),
        ("xl/worksheets/sheet1.xml", sheet),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn missing_caches_and_array_reconstruction_require_initial_calculation() {
    for (body, complete) in [
        (r#"<c r="A1"><f>1+2</f><v>3</v></c>"#, true),
        (r#"<c r="A1"><f>1+2</f></c>"#, false),
        (r#"<c r="A1"><f t="array" ref="A1:A2">ROW(A1:A2)</f><v>1</v></c>"#, false),
    ] {
        let sheet = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1">{body}</row></sheetData></worksheet>"#
        );
        let (_, report) = gridcraft_xlsx::read_xlsx(&workbook_with_sheet(&sheet)).unwrap();
        assert_eq!(report.cached_results_complete, complete);
    }
}

#[test]
fn large_sheet_defers_expression_retention_without_losing_formula_or_saved_value() {
    let padding = " ".repeat(20 * 1024 * 1024);
    let sheet = format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><!--{padding}--><sheetData><row r="1"><c r="A1"><f>1+2</f><v>123</v></c></row></sheetData></worksheet>"#
    );
    let (workbook, report) = gridcraft_xlsx::read_xlsx(&workbook_with_sheet(&sheet)).unwrap();
    assert!(report.cached_results_complete);
    let cell = workbook.sheet(0).unwrap().cell(cell(0, 0)).unwrap();
    assert_eq!(cell.value, Value::Number(123.0));
    let formula = cell.formula.as_ref().unwrap();
    assert_eq!(formula.text, "1+2");
    assert!(formula.expr.is_none());
    assert!(formula.expr_arc().is_some());
}

/// Force every checkpoint to cross an event-loop turn, without browser timing dependencies.
struct YieldingWork {
    stages: Vec<gridcraft_core::work::WorkStage>,
    cancel_at: Option<gridcraft_core::work::WorkStage>,
    named_stages: Vec<gridcraft_core::work::WorkStage>,
}
impl gridcraft_core::work::WorkControl for YieldingWork {
    async fn checkpoint(&mut self, progress: gridcraft_core::work::WorkProgress) -> std::result::Result<(), gridcraft_core::work::WorkCancelled> {
        self.stages.push(progress.stage);
        if let Some(name) = &progress.sheet_name {
            assert_eq!(name.as_ref(), "Sheet1");
            assert_eq!((progress.part, progress.parts), (1, 1));
            self.named_stages.push(progress.stage);
        }
        let mut yielded = false;
        std::future::poll_fn(|cx| {
            if yielded {
                std::task::Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                std::task::Poll::Pending
            }
        })
        .await;
        if self.cancel_at == Some(progress.stage) { Err(gridcraft_core::work::WorkCancelled) } else { Ok(()) }
    }
}
fn drive<T>(future: impl std::future::Future<Output = T>) -> (T, usize) {
    let mut future = std::pin::pin!(future);
    let mut turns = 0;
    loop {
        match future.as_mut().poll(&mut std::task::Context::from_waker(std::task::Waker::noop())) {
            std::task::Poll::Ready(value) => return (value, turns),
            std::task::Poll::Pending => {
                turns += 1;
                assert!(turns < 100_000);
            }
        }
    }
}
fn cooperative_fixture() -> Vec<u8> {
    let mut rows = String::new();
    for row in 1..=2048 {
        rows.push_str(&format!(r#"<row r="{row}"><c r="A{row}"><v>2</v></c><c r="B{row}"><f>A{row}*3</f><v>6</v></c></row>"#));
    }
    workbook_with_sheet(&format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{rows}</sheetData></worksheet>"#
    ))
}
#[test]
fn cooperative_import_yields_and_publishes_only_a_complete_document() {
    use gridcraft_core::work::WorkStage;
    let bytes = cooperative_fixture();
    let mut target = Session::new();
    target.new_workbook();
    target.execute("cell.set", json!({"cell":"A1","input":"91"})).unwrap();
    let uid = target.doc().unwrap().uid;
    let mut control = YieldingWork { stages: vec![], cancel_at: None, named_stages: vec![] };
    let (prepared, turns) = drive(cmd::file::prepare_open("synthetic.xlsx", &bytes, &json!({}), &mut control));
    assert!(turns > 10);
    assert!(control.named_stages.contains(&WorkStage::Worksheet));
    assert!(control.named_stages.contains(&WorkStage::RowHeights));
    for stage in [WorkStage::Archive, WorkStage::Worksheet, WorkStage::RowHeights, WorkStage::Dependencies, WorkStage::Calculation] {
        assert!(control.stages.contains(&stage));
    }
    assert_eq!(target.doc().unwrap().uid, uid);
    assert_eq!(target.doc().unwrap().wb.sheet(0).unwrap().value(cell(0, 0)), Value::Number(91.0));
    target.finish_import(prepared.unwrap()).unwrap();
    assert_eq!(target.documents().len(), 2);
    assert_eq!(target.doc().unwrap().wb.sheet(0).unwrap().value(cell(2047, 1)), Value::Number(6.0));
    let encoded = target.execute("file.saveBytes", json!({})).unwrap();
    let saved = io::base64_decode(encoded["base64"].as_str().unwrap()).unwrap();
    let (roundtrip, _) = gridcraft_xlsx::read_xlsx(&saved).unwrap();
    assert_eq!(roundtrip.sheet(0).unwrap().value(cell(2047, 1)), Value::Number(6.0));
}
#[test]
fn cancellation_is_an_error_at_every_import_phase_and_keeps_existing_work() {
    use gridcraft_core::work::WorkStage;
    let bytes = cooperative_fixture();
    let mut target = Session::new();
    target.new_workbook();
    target.execute("cell.set", json!({"cell":"A1","input":"91"})).unwrap();
    let uid = target.doc().unwrap().uid;
    for stage in [WorkStage::Archive, WorkStage::Worksheet, WorkStage::RowHeights, WorkStage::Dependencies, WorkStage::Calculation] {
        let mut control = YieldingWork { stages: vec![], cancel_at: Some(stage), named_stages: vec![] };
        let (result, _) = drive(cmd::file::prepare_open("synthetic.xlsx", &bytes, &json!({}), &mut control));
        assert!(matches!(result, Err(EngineError::Cancelled(_))), "{stage:?}");
        assert_eq!(target.documents().len(), 1);
        assert_eq!(target.doc().unwrap().uid, uid);
        assert_eq!(target.doc().unwrap().wb.sheet(0).unwrap().value(cell(0, 0)), Value::Number(91.0));
    }
}
