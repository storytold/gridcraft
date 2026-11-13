//! Integration-style tests: round trips, hand-written fixtures, hostile input and CSV.

mod array_export;
mod cellmetadata;
mod chart_book_names;
mod chart_bool_default;
mod chart_combo_kinds;
mod chart_data_table;
mod chart_ref_offsets;
mod controls_tests;
mod csv_tests;
mod encrypted_tests;
mod equation_tests;
mod fixtures;
mod google_sheets_dummy;
mod ink_roundtrip;
mod literal_category_roundtrip;
mod locale_numfmt;
mod malformed;
mod multilevel_category_roundtrip;
mod note_box;
mod ods_tests;
mod page_order;
mod passthrough_tests;
mod phonetic_tests;
mod pivot_tests;
mod richdata_tests;
mod roundtrip;
mod shape_text_rtl;
mod shared_media;
mod sheet_data_pin;
mod sheet_names;
mod smooth_roundtrip;
mod trendline_roundtrip;
mod xls_dv_tests;
mod xls_style_tests;
mod xls_tests;
mod xlsb_tests;
mod zip_order;

use std::io::Write;

/// Builds a zip package from (name, content) pairs.
pub fn make_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

pub const CT: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/></Types>"#;
pub const ROOT_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

/// A minimal one-sheet package with the given sheet XML body (inside `<worksheet>`), plus
/// optional extra parts and workbook relationships.
pub fn minimal(sheet_body: &str, extra_parts: &[(&str, &str)], wb_extra_rels: &str, wb_extra: &str) -> Vec<u8> {
    let wb = format!(
        r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{wb_extra}<sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let rels = format!(
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>{wb_extra_rels}</Relationships>"#
    );
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{sheet_body}</worksheet>"#
    );
    let mut parts: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wb.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
    ];
    for (n, c) in extra_parts {
        parts.push((n, c.as_bytes()));
    }
    make_zip(&parts)
}

#[test]
/// Verifies that a cross-sheet `.xlsx` table slicer (`x15:tableSlicerCache`) imports cleanly without skipped-object warnings.
fn cross_sheet_table_slicer_imports_without_skipped_object_warning() {
    let wb_xml = r#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Dashboard" sheetId="1" r:id="rId1"/><sheet name="Tracker" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
    let wb_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.microsoft.com/office/2007/relationships/slicerCache" Target="slicerCaches/slicerCache1.xml"/></Relationships>"#;
    let sheet1 = r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData/><drawing r:id="rId1"/></worksheet>"#;
    let sheet1_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/><Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2007/relationships/slicer" Target="../slicers/slicer1.xml"/></Relationships>"#;
    let drawing1 = r#"<?xml version="1.0"?><xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:sle="http://schemas.microsoft.com/office/drawing/2010/slicer"><xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>2</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>4</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>10</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to><xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="10" name="Date for Slicer_1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/></xdr:xfrm><a:graphic><a:graphicData uri="http://schemas.microsoft.com/office/drawing/2010/slicer"><sle:slicer name="Date for Slicer_1"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor></xdr:wsDr>"#;
    let slicer1 = r#"<?xml version="1.0"?><slicers xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><slicer name="Date for Slicer_1" cache="SlicerCache_Table_3_Col_36" caption="Date for Slicer" columnCount="2"/></slicers>"#;
    let slicer_cache1 = r#"<?xml version="1.0"?><slicerCacheDefinition xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" xmlns:x15="http://schemas.microsoft.com/office/spreadsheetml/2010/11/main" name="SlicerCache_Table_3_Col_36" sourceName="Date for Slicer"><extLst><ext uri="{2F2917AC-EB37-4324-AD4E-5DD8C200BD13}"><x15:tableSlicerCache tableId="3" column="36"/></ext></extLst></slicerCacheDefinition>"#;
    let sheet2 = r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Date for Slicer</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>2026-Q1</t></is></c></row></sheetData><tableParts count="1"><tablePart r:id="rId1"/></tableParts></worksheet>"#;
    let sheet2_rels = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/table3.xml"/></Relationships>"#;
    let table3 = r#"<?xml version="1.0"?><table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" id="3" name="Table_3" displayName="Table_3" ref="A1:A2"><tableColumns count="1"><tableColumn id="36" name="Date for Slicer"/></tableColumns></table>"#;
    let zip = make_zip(&[
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wb_xml.as_bytes()),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet1_rels.as_bytes()),
        ("xl/drawings/drawing1.xml", drawing1.as_bytes()),
        ("xl/slicers/slicer1.xml", slicer1.as_bytes()),
        ("xl/slicerCaches/slicerCache1.xml", slicer_cache1.as_bytes()),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
        ("xl/worksheets/_rels/sheet2.xml.rels", sheet2_rels.as_bytes()),
        ("xl/tables/table3.xml", table3.as_bytes()),
    ]);
    let (wb, report) = crate::read_xlsx(&zip).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("skipped")), "expected no skipped warnings, got: {:?}", report.warnings);
    assert_eq!(wb.sheets[0].slicers.len(), 1);
    let sl = &wb.sheets[0].slicers[0];
    assert_eq!(sl.table, "Table_3");
    assert_eq!(sl.column, "Date for Slicer");
    assert_eq!(sl.caption, "Date for Slicer");
    assert_eq!(sl.columns, 2);
    assert_eq!(sl.anchor.cell, gridcraft_core::CellRef::new(2, 1));
    assert!(sl.anchor.width > 0.0 && sl.anchor.height > 0.0);

    // 2. `sourceName` fallback when `<x15:tableSlicerCache>` is absent resolves the table and column by header name.
    let cache_by_source = r#"<?xml version="1.0"?><slicerCacheDefinition xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" name="SlicerCache_Table_3_Col_36" sourceName="Date for Slicer"/>"#;
    let zip_fallback = make_zip(&[
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wb_xml.as_bytes()),
        ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        ("xl/worksheets/_rels/sheet1.xml.rels", sheet1_rels.as_bytes()),
        ("xl/drawings/drawing1.xml", drawing1.as_bytes()),
        ("xl/slicers/slicer1.xml", slicer1.as_bytes()),
        ("xl/slicerCaches/slicerCache1.xml", cache_by_source.as_bytes()),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
        ("xl/worksheets/_rels/sheet2.xml.rels", sheet2_rels.as_bytes()),
        ("xl/tables/table3.xml", table3.as_bytes()),
    ]);
    let (wb_fb, _) = crate::read_xlsx(&zip_fallback).unwrap();
    assert_eq!(wb_fb.sheets[0].slicers.len(), 1);
    assert_eq!((wb_fb.sheets[0].slicers[0].table.as_str(), wb_fb.sheets[0].slicers[0].column.as_str()), ("Table_3", "Date for Slicer"));

    // 3. Negative cases: a missing cache, an unknown table/column, or a mismatched frame name when multiple slicer definitions exist attaches nothing.
    let slicer_two_defs = r#"<?xml version="1.0"?><slicers xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main"><slicer name="Other_1" cache="SlicerCache_Table_3_Col_36" caption="Other"/><slicer name="Other_2" cache="SlicerCache_Table_3_Col_36" caption="Other 2"/></slicers>"#;
    for (slicers_xml, cache_xml) in [
        (slicer_two_defs, slicer_cache1),
        (
            slicer1,
            r#"<?xml version="1.0"?><slicerCacheDefinition xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" name="UnrelatedCache" sourceName="MissingCol"/>"#,
        ),
        (
            slicer1,
            r#"<?xml version="1.0"?><slicerCacheDefinition xmlns="http://schemas.microsoft.com/office/spreadsheetml/2009/9/main" name="SlicerCache_Table_3_Col_36" sourceName="MissingCol"/>"#,
        ),
    ] {
        let zip_neg = make_zip(&[
            ("[Content_Types].xml", CT.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("xl/workbook.xml", wb_xml.as_bytes()),
            ("xl/_rels/workbook.xml.rels", wb_rels.as_bytes()),
            ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
            ("xl/worksheets/_rels/sheet1.xml.rels", sheet1_rels.as_bytes()),
            ("xl/drawings/drawing1.xml", drawing1.as_bytes()),
            ("xl/slicers/slicer1.xml", slicers_xml.as_bytes()),
            ("xl/slicerCaches/slicerCache1.xml", cache_xml.as_bytes()),
            ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
            ("xl/worksheets/_rels/sheet2.xml.rels", sheet2_rels.as_bytes()),
            ("xl/tables/table3.xml", table3.as_bytes()),
        ]);
        let (wb_neg, _) = crate::read_xlsx(&zip_neg).unwrap();
        assert!(wb_neg.sheets[0].slicers.is_empty(), "mismatched or unresolvable slicer must not attach");
    }
}
