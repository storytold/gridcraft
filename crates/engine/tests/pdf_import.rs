use gridcraft_core::{CellRef, Value};
use gridcraft_engine::{Session, Trust, pdf_import};
use gridcraft_pdf::{Font, PdfDoc, Point};
use serde_json::json;
use std::sync::{Arc, atomic::AtomicBool};
fn sample() -> Vec<u8> {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(400., 400.);
    for y in [40., 70., 100., 140.] {
        p.stroke_line_dashed(Point { x: 30., y }, Point { x: 330., y }, 0.5, [0, 0, 0], &[]);
    }
    for x in [30., 130., 230., 330.] {
        p.stroke_line_dashed(Point { x, y: 40. }, Point { x, y: 140. }, 0.5, [0, 0, 0], &[]);
    }
    for (x, y, text) in [
        (36., 60., "Code"),
        (136., 60., "Description"),
        (236., 60., "Amount"),
        (36., 90., "00123"),
        (136., 90., "Widgets"),
        (236., 90., "12.50"),
        (36., 118., "=1+1"),
        (136., 114., "First line"),
        (136., 130., "Second line"),
    ] {
        p.text(x, y, 10., Font::Helvetica, [0, 0, 0], text);
    }
    let p = doc.add_page(400., 400.);
    p.text(30., 50., 12., Font::Helvetica, [0, 0, 0], "Ordinary text, not a table.");
    doc.finish()
}
#[test]
fn detects_bordered_text_without_losing_empty_or_multiline_cells() {
    let bytes = Arc::new(sample());
    let result = pdf_import::inspect(bytes.clone(), 1, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(result.pages, 2);
    assert_eq!(result.tables.len(), 1);
    assert_eq!(
        result.tables[0].rows,
        vec![vec!["Code", "Description", "Amount"], vec!["00123", "Widgets", "12.50"], vec!["=1+1", "First line\nSecond line", ""]]
    );
    assert!(pdf_import::inspect(bytes, 2, Arc::new(AtomicBool::new(false))).unwrap().tables.is_empty());
}
#[test]
fn cancellation_malformed_and_invalid_pages_return_errors() {
    let data = Arc::new(sample());
    assert!(matches!(pdf_import::inspect(data.clone(), 1, Arc::new(AtomicBool::new(true))), Err(pdf_import::ImportError::Cancelled)));
    for page in [0, 3, u32::MAX] {
        assert!(pdf_import::inspect(data.clone(), page, Arc::new(AtomicBool::new(false))).is_err());
    }
    assert!(pdf_import::inspect(Arc::new(b"not a PDF".to_vec()), 1, Arc::new(AtomicBool::new(false))).is_err());
}
fn session() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}
#[test]
fn imports_text_as_a_table_with_one_undo_and_preserves_file_provenance() {
    let mut s = session();
    let before = s.active().unwrap().wb.clone();
    let out = s.execute("data.fromPdf", json!({"rows":[["Code","Price"],["00123","12.50"],["=1+1",""]]})).unwrap();
    assert_eq!(out["rows"], 3);
    let wb = &s.active().unwrap().wb;
    let sh = wb.active().unwrap();
    assert_eq!(sh.value(CellRef::new(1, 0)), Value::text("00123"));
    assert_eq!(sh.value(CellRef::new(2, 0)), Value::text("=1+1"));
    assert!(sh.cell(CellRef::new(2, 0)).is_none_or(|c| c.formula.is_none()));
    assert_eq!(sh.tables.len(), 1);
    assert_eq!(s.active().unwrap().trust, Trust::FromFile);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(*s.active().unwrap().wb, *before);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheets.len(), 2);
}
#[test]
fn number_conversion_is_explicit_and_bad_data_never_leaves_a_sheet_behind() {
    let mut s = session();
    let before = s.active().unwrap().wb.clone();
    for params in [
        json!({}),
        json!({"rows":[["A"],["B","C"]]}),
        json!({"rows":[["A"],["abc"]],"numericColumns":[0]}),
        json!({"rows":[["A"],["12"]],"numericColumns":[1]}),
        json!({"rows":[["A"]],"headers":"yes"}),
        json!({"rows":[["😀".repeat(16_384)]]}),
    ] {
        assert!(s.execute("data.fromPdf", params).is_err());
        assert_eq!(*s.active().unwrap().wb, *before);
    }
    s.execute("data.fromPdf", json!({"rows":[["Code","Price"],["00123","12.50"]],"numericColumns":[1]})).unwrap();
    assert_eq!(s.active().unwrap().wb.active().unwrap().value(CellRef::new(1, 1)), Value::Number(12.5));
}
#[test]
fn headers_can_be_added_and_imports_refuse_structure_protection() {
    let mut s = session();
    s.execute("data.fromPdf", json!({"rows":[["00123","12.50"]],"headers":false})).unwrap();
    let sh = s.active().unwrap().wb.active().unwrap();
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::text("Column1"));
    assert_eq!(sh.value(CellRef::new(1, 0)), Value::text("00123"));
    Arc::make_mut(&mut s.active_mut().unwrap().wb).protected_structure = true;
    let before = s.active().unwrap().wb.clone();
    assert!(s.execute("data.fromPdf", json!({"rows":[["x"]]})).is_err());
    assert_eq!(*s.active().unwrap().wb, *before);
}
#[test]
fn imported_literals_and_numbers_survive_xlsx_save_and_reopen() {
    let mut s = session();
    s.execute("data.fromPdf", json!({"rows":[["Code","Price"],["00123","12.50"],["=1+1","0"]],"numericColumns":[1]})).unwrap();
    let bytes = gridcraft_engine::io::save_bytes(&s.active().unwrap().wb, "Imported.xlsx").unwrap();
    let (wb, _) = gridcraft_engine::io::open_bytes("Imported.xlsx", &bytes).unwrap();
    let sh = wb.sheet(1).unwrap();
    assert_eq!(sh.value(CellRef::new(1, 0)), Value::text("00123"));
    assert_eq!(sh.value(CellRef::new(2, 0)), Value::text("=1+1"));
    assert_eq!(sh.value(CellRef::new(2, 1)), Value::Number(0.0));
    assert_eq!(sh.tables.len(), 1);
}

#[test]
fn file_preview_and_load_select_a_page_without_dialogs() {
    let path = std::env::temp_dir().join(format!("gridcraft-pdf-{}-file-preview.pdf", std::process::id()));
    std::fs::write(&path, sample()).unwrap();
    let mut s = session();
    let before = s.active().unwrap().wb.clone();
    let p = json!({"path":path.to_str().unwrap()});
    let preview = s.execute("data.pdfTables", p.clone()).unwrap();
    assert_eq!(preview["pages"], 2);
    assert_eq!(preview["tables"][0]["rows"][1][0], "00123");
    assert_eq!(*s.active().unwrap().wb, *before);
    assert!(s.execute("data.fromPdf", json!({"path":path.to_str().unwrap(),"page":2})).is_err());
    assert_eq!(*s.active().unwrap().wb, *before);
    s.execute("data.fromPdf", p).unwrap();
    assert_eq!(s.active().unwrap().wb.active().unwrap().value(CellRef::new(1, 0)), Value::text("00123"));
    std::fs::remove_file(path).unwrap();
}

// A second original PDF producer: plain PDF operators, independent of our PDF writer.
fn operator_pdf(content: &str, page_extra: &str) -> Vec<u8> {
    use std::io::Write;
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 200] {page_extra} /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
    ];
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(bytes.len());
        writeln!(bytes, "{} 0 obj\n{}\nendobj", i + 1, object).unwrap();
    }
    let xref = bytes.len();
    writeln!(bytes, "xref\n0 {}\n0000000000 65535 f ", offsets.len()).unwrap();
    for offset in &offsets[1..] {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    writeln!(bytes, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF", offsets.len()).unwrap();
    bytes
}
const OPERATORS: &str = "0.5 w 20 20 m 220 20 l 20 50 m 220 50 l 20 80 m 220 80 l 20 20 m 20 80 l 120 20 m 120 80 l 220 20 m 220 80 l S BT /F1 10 Tf 1 0 0 1 26 64 Tm (Code) Tj 1 0 0 1 126 64 Tm (Value) Tj 1 0 0 1 26 34 Tm (00123) Tj 1 0 0 1 126 34 Tm (=1+1) Tj ET";
#[test]
fn translated_scaled_cropped_content_keeps_cell_assignment() {
    for content in [OPERATORS.to_string(), format!("q 0.8 0 0 0.8 30 40 cm {OPERATORS} Q")] {
        let bytes = operator_pdf(&content, "/CropBox [10 10 390 190]");
        let p = pdf_import::inspect(Arc::new(bytes), 1, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(p.tables.len(), 1);
        assert_eq!(p.tables[0].rows, vec![vec!["Code", "Value"], vec!["00123", "=1+1"]]);
    }
}
#[test]
fn incomplete_borders_and_rotated_text_are_explicit_errors() {
    for (content, extra) in [(OPERATORS.replace("120 20 m 120 80 l", "120 20 m 120 50 l"), ""), (OPERATORS.into(), "/Rotate 90")] {
        assert!(pdf_import::inspect(Arc::new(operator_pdf(&content, extra)), 1, Arc::new(AtomicBool::new(false))).is_err());
    }
}
#[test]
fn filled_rectangle_borders_work_without_decoding_a_raster_page() {
    let content = "20 20 200 0.5 re f 20 50 200 0.5 re f 20 80 200 0.5 re f 20 20 0.5 60 re f 120 20 0.5 60 re f 220 20 0.5 60 re f";
    let p = pdf_import::inspect(Arc::new(operator_pdf(content, "")), 1, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(p.tables.len(), 1);
    assert_eq!(p.tables[0].rows, vec![vec!["", ""], vec!["", ""]]);
}

#[test]
fn images_inside_a_grid_are_refused_but_unrelated_logos_do_not_block_it() {
    for inside in [false, true] {
        let mut doc = PdfDoc::new();
        let p = doc.add_page(400., 400.);
        for y in [40., 70., 100.] {
            p.stroke_line(30., y, 230., y, 0.5, [0, 0, 0]);
        }
        for x in [30., 130., 230.] {
            p.stroke_line(x, 40., x, 100., 0.5, [0, 0, 0]);
        }
        let at = if inside { gridcraft_pdf::Rect::new(40., 50., 20., 10.) } else { gridcraft_pdf::Rect::new(300., 300., 20., 10.) };
        p.image_rgba(at, &[255, 0, 0, 255], [1, 1]);
        let result = pdf_import::inspect(Arc::new(doc.finish()), 1, Arc::new(AtomicBool::new(false)));
        if inside {
            assert!(matches!(result, Err(pdf_import::ImportError::Unsupported(_))));
        } else {
            assert_eq!(result.unwrap().tables.len(), 1);
        }
    }
}

#[test]
fn duplicate_or_blank_source_headers_are_not_silently_renamed() {
    for rows in [json!([["Code", "code"], ["A", "B"]]), json!([["Code", ""], ["A", "B"]])] {
        let mut s = session();
        let before = s.active().unwrap().wb.clone();
        assert!(s.execute("data.fromPdf", json!({"rows":rows})).is_err());
        assert_eq!(*s.active().unwrap().wb, *before);
        s.execute("data.fromPdf", json!({"rows":rows,"asTable":false})).unwrap();
        assert_eq!(
            s.active().unwrap().wb.active().unwrap().value(CellRef::new(0, 1)),
            if rows[0][1] == "" { Value::Empty } else { Value::text("code") }
        );
    }
}

#[test]
fn a_missing_outer_border_does_not_silently_drop_the_last_column() {
    let content = OPERATORS.replace("220 20 m 220 80 l", "");
    assert!(matches!(
        pdf_import::inspect(Arc::new(operator_pdf(&content, "")), 1, Arc::new(AtomicBool::new(false))),
        Err(pdf_import::ImportError::Unsupported(_))
    ));
}

#[test]
fn mapped_boundary_spaces_survive_extraction_loading_and_reopening() {
    let content = OPERATORS.replace("(00123)", "( 00123 )").replace("(=1+1)", "( =1+1 )");
    let preview = pdf_import::inspect(Arc::new(operator_pdf(&content, "")), 1, Arc::new(AtomicBool::new(false))).unwrap();
    assert_eq!(preview.tables[0].rows[1], vec![" 00123 ", " =1+1 "]);
    let mut s = session();
    s.execute("data.fromPdf", json!({"rows":preview.tables[0].rows})).unwrap();
    let bytes = gridcraft_engine::io::save_bytes(&s.active().unwrap().wb, "Spaces.xlsx").unwrap();
    let (wb, _) = gridcraft_engine::io::open_bytes("Spaces.xlsx", &bytes).unwrap();
    for (col, expected) in [" 00123 ", " =1+1 "].iter().enumerate() {
        let at = CellRef::new(1, col as u32);
        let sh = wb.sheet(1).unwrap();
        assert_eq!(sh.value(at), Value::text(*expected));
        assert!(sh.cell(at).unwrap().formula.is_none());
    }
}
