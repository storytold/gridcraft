//! Number formats whose meaning depends on the reader's locale are written with their code: the
//! currency formats 5–8 and the accounting formats with a currency, 42 and 44. Written as a bare
//! built-in id, LibreOffice and Excel in other locales show their own version (a 44 cell holding
//! -0.14 reads `$ (0.14)` in US Excel but `- 0.14` in LibreOffice).

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::*;

use crate::tables::builtin_format;
use crate::{read_xlsx, write_xlsx};

fn styles_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    zip.by_name("xl/styles.xml").unwrap().read_to_string(&mut s).unwrap();
    s
}

/// Writes one cell formatted with `code`; returns the `numFmtId` its cell format points at, the
/// `styles.xml` and the code read back.
fn round_trip(code: &str) -> (u32, String, String) {
    let mut wb = Workbook::new();
    let style = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
    let mut sheet = Sheet::new("Sheet1");
    sheet.cells.set(CellRef::new(0, 0), Cell { value: Value::Number(-0.14), formula: None, style });
    wb.sheets = vec![Arc::new(sheet)];
    let bytes = write_xlsx(&wb).unwrap();
    let xml = styles_xml(&bytes);
    let xfs = &xml[xml.find("<cellXfs").unwrap()..];
    let id = xfs
        .match_indices("numFmtId=\"")
        .map(|(i, m)| xfs[i + m.len()..].split('"').next().unwrap().parse::<u32>().unwrap())
        .find(|&id| id != 0)
        .unwrap();
    let (back, _) = read_xlsx(&bytes).unwrap();
    let st = back.styles.get(back.sheet(0).unwrap().cell(CellRef::new(0, 0)).unwrap().style);
    (id, xml, st.num_fmt.as_str().to_string())
}

#[test]
fn currency_and_accounting_formats_are_written_with_their_code() {
    for builtin in [5, 6, 7, 8, 42, 44] {
        let code = builtin_format(builtin).unwrap();
        let (id, xml, back) = round_trip(code);
        assert!(id >= 164, "format {builtin} written as built-in id {id}");
        assert!(xml.contains(&format!("<numFmt numFmtId=\"{id}\"")), "format {builtin}: no <numFmt> for id {id}");
        assert_eq!(back, code, "format {builtin}");
    }
}

#[test]
fn locale_independent_built_ins_keep_their_id() {
    for builtin in [3, 4, 10, 14, 41, 43, 49] {
        let (id, _, back) = round_trip(builtin_format(builtin).unwrap());
        assert_eq!(id, builtin);
        assert_eq!(back, builtin_format(builtin).unwrap());
    }
}

/// Original synthetic package: a date in a conditional differential number format.
fn dxf_date_round_trip(id: Option<u32>, code: &str, expected_code: &str, builtin: bool) {
    let attr = id.map(|id| format!(" numFmtId=\"{id}\"")).unwrap_or_default();
    let styles = format!(
        r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
        <fonts count="1"><font><sz val="11"/><name val="Carlito"/></font></fonts>
        <fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
        <borders count="1"><border/></borders><cellXfs count="1"><xf/></cellXfs>
        <dxfs count="1"><dxf><numFmt{attr} formatCode="{code}"/></dxf></dxfs></styleSheet>"#
    );
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let sheet = r#"<sheetData><row r="1"><c r="A1"><v>45292.5</v></c></row></sheetData>
        <conditionalFormatting sqref="A1"><cfRule type="cellIs" dxfId="0" priority="1" operator="greaterThan"><formula>0</formula></cfRule></conditionalFormatting>"#;
    let source = super::minimal(sheet, &[("xl/styles.xml", &styles)], rels, "");
    let (mut wb, _) = read_xlsx(&source).unwrap();
    for _ in 0..3 {
        let sh = wb.sheet(0).unwrap();
        assert_eq!(sh.value(CellRef::new(0, 0)), Value::Number(45292.5));
        let CfRule::CellIs { style, .. } = &sh.cond_formats[0].rule else { panic!("expected cellIs") };
        assert_eq!(style.num_fmt.as_str(), expected_code);
        assert_eq!(style.num_fmt.is_literal_short_date(), !builtin, "id={id:?}, code={code}");
        let out = write_xlsx(&wb).unwrap();
        let xml = styles_xml(&out);
        let root = crate::xml::parse(xml.as_bytes()).unwrap();
        let written = root.child("dxfs").unwrap().child("dxf").unwrap().child("numFmt").unwrap();
        let written_id = written.attr_u32("numFmtId").unwrap();
        if builtin {
            assert_eq!(Some(written_id), id);
        } else {
            assert!(written_id >= 164, "custom date collapsed to built-in {written_id}");
        }
        assert_eq!(written.attr("formatCode"), Some(expected_code));
        wb = read_xlsx(&out).unwrap().0;
    }
}

/// Explicit DXF short-date codes with IDs 14/22 retain built-in regional identity across round trips.
#[test]
fn dxf_builtin_short_dates_keep_regional_identity_and_builtin_ids() {
    for (id, code) in [(14, "m/d/yyyy"), (22, "m/d/yyyy h:mm")] {
        dxf_date_round_trip(Some(id), code, code, true);
    }
}

/// Custom or absent DXF IDs keep explicit short-date aliases literal across round trips.
#[test]
fn dxf_custom_and_missing_ids_keep_literal_short_dates() {
    for id in [Some(164), Some(165), None] {
        for code in ["m/d/yyyy", "m/d/yyyy h:mm"] {
            dxf_date_round_trip(id, code, code, false);
        }
    }
}

/// Standard short-date spellings normalize by built-in DXF ID, including case variants.
#[test]
fn dxf_standard_short_date_aliases_use_canonical_builtin_formats() {
    for (id, code, canonical) in [
        (14, "mm-dd-yy", "m/d/yyyy"),
        (14, "MM-DD-YY", "m/d/yyyy"),
        (14, "M/D/YYYY", "m/d/yyyy"),
        (22, "m/d/yy h:mm", "m/d/yyyy h:mm"),
        (22, "M/D/YY H:MM", "m/d/yyyy h:mm"),
    ] {
        dxf_date_round_trip(Some(id), code, canonical, true);
    }
}

/// Explicit custom standard-date aliases retain their exact code and custom ID on export.
#[test]
fn explicit_custom_standard_aliases_keep_their_code_and_custom_id() {
    for code in ["mm-dd-yy", "MM-DD-YY", "m/d/yy h:mm", "M/D/YY H:MM"] {
        let (id, _, back) = round_trip(code);
        assert!(id >= 164, "explicit custom alias collapsed to built-in {id}");
        assert_eq!(back, code);
    }
}
