//! Original, synthetic XLSX regressions: built-in date IDs versus literal custom formats.
use std::io::{Cursor, Read, Write};
use std::sync::{Arc, Mutex};

use gridcraft_engine::display::{cell_text, cell_text_in, edit_text};
use gridcraft_engine::{
    Session,
    core::{CellRef, Value, date::DateSystem},
    model::{NumFmt, Workbook},
    numfmt::Locale,
};
use serde_json::json;

/// Address a zero-based row in the synthetic fixture's value column B.
fn at(row: u32) -> CellRef {
    CellRef::new(row, 1)
}

/// Build an original in-memory XLSX with built-in dates, custom aliases and numeric formats
/// using the requested date system; no Office-authored workbook is needed.
fn fixture(sys: DateSystem) -> Vec<u8> {
    let serial = gridcraft_engine::core::parse::parse_input("2026-03-04", sys).value.as_f64().unwrap();
    let styles = r##"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<numFmts count="4"><numFmt numFmtId="164" formatCode="m/d/yyyy"/>
<numFmt numFmtId="165" formatCode="dd.mm.yyyy"/>
<numFmt numFmtId="166" formatCode="m/d/yyyy h:mm"/>
<numFmt numFmtId="167" formatCode="#,##0.00&amp;quot; RON&amp;quot;"/></numFmts>
<fonts count="1"><font><sz val="11"/><name val="Carlito"/></font></fonts>
<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
<borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs>
<cellXfs count="9"><xf/><xf numFmtId="14"/><xf numFmtId="164"/><xf numFmtId="165"/>
<xf numFmtId="4"/><xf numFmtId="10"/><xf numFmtId="22"/><xf numFmtId="166"/><xf numFmtId="167"/></cellXfs>
</styleSheet>"##
        .replace("&amp;quot;", "&quot;");
    let mut rows = String::from(
        r#"<row r="1"><c r="A1" t="inlineStr"><is><t>Synthetic format</t></is></c><c r="B1" t="inlineStr"><is><t>Value</t></is></c></row>"#,
    );
    for (i, (label, style, value)) in [
        ("Built-in 14", 1, serial),
        ("Custom US date", 2, serial),
        ("Custom dotted date", 3, serial),
        ("Grouped number", 4, 1234567.89),
        ("Percentage", 5, 0.125),
        ("Built-in 22", 6, serial + 0.5),
        ("Custom US timestamp", 7, serial + 0.5),
        ("RON literal", 8, -1234.5),
    ]
    .into_iter()
    .enumerate()
    {
        let r = i + 2;
        rows.push_str(&format!(
            r#"<row r="{r}"><c r="A{r}" t="inlineStr"><is><t>{label}</t></is></c><c r="B{r}" s="{style}"><v>{value}</v></c></row>"#
        ));
    }
    rows.push_str(r#"<row r="10"><c r="A10" t="inlineStr"><is><t>Formula</t></is></c><c r="B10" s="4"><f>SUM(1.5,2.25)</f><v>3.75</v></c></row>"#);
    let sheet = format!(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cols><col min="1" max="1" width="30" customWidth="1"/><col min="2" max="2" width="25" customWidth="1"/></cols><sheetData>{rows}</sheetData></worksheet>"#
    );
    let year1904 = u8::from(sys == DateSystem::D1904);
    let workbook = format!(
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="{year1904}"/><sheets><sheet name="Synthetic" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let ct = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#;
    let root = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in [
        ("[Content_Types].xml", ct),
        ("_rels/.rels", root),
        ("xl/workbook.xml", &workbook),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/styles.xml", &styles),
        ("xl/worksheets/sheet1.xml", &sheet),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// Read the fixture cell through the locale-aware display path used by the grid.
fn shown(wb: &Workbook, row: u32, loc: &Locale) -> String {
    cell_text_in(wb, wb.sheet(0).unwrap(), at(row), loc)
}
/// Extract a named XML part from the synthetic XLSX for independent serialization checks.
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut text).unwrap();
    text
}

/// Built-in regional dates and literal custom codes survive three XLSX round trips
/// with both date systems, unchanged values and invariant formula/editor text.
#[test]
fn synthetic_xlsx_preserves_regional_builtins_and_literal_custom_dates() {
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        let source = fixture(sys);
        let (mut wb, report) = gridcraft_xlsx::read_xlsx(&source).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let ro = Locale::from_tag("ro-RO");
        let customized = Locale::from_tag("en_US@rg=sgzzzz").with_icu_short_date("dd.MM.y");
        let original = wb.sheet(0).unwrap().value(at(1));
        for _ in 0..3 {
            assert_eq!(wb.date_system, sys);
            assert_eq!(shown(&wb, 1, &ro), "04.03.2026");
            assert_eq!(shown(&wb, 1, &customized), "04.03.2026");
            assert_eq!(shown(&wb, 1, &Locale::EN_US), "3/4/2026");
            assert_eq!(shown(&wb, 2, &ro), "3/4/2026");
            assert_eq!(shown(&wb, 3, &Locale::EN_US), "04.03.2026");
            assert_eq!(shown(&wb, 4, &ro), "1.234.567,89");
            assert_eq!(shown(&wb, 5, &ro), "12,50%");
            assert_eq!(shown(&wb, 6, &ro), "04.03.2026 12:00");
            assert_eq!(shown(&wb, 7, &ro), "3/4/2026 12:00");
            assert_eq!(shown(&wb, 8, &ro), "-1.234,50 RON");
            assert_eq!(shown(&wb, 9, &ro), "3,75");
            let sh = wb.sheet(0).unwrap();
            assert_eq!(sh.value(at(1)), original);
            assert_eq!(sh.cell(at(9)).unwrap().formula.as_ref().unwrap().text, "SUM(1.5,2.25)");
            assert_eq!(cell_text(&wb, sh, at(1)), "3/4/2026");
            assert_eq!(edit_text(&wb, sh, at(1)), "3/4/2026");
            assert_eq!(wb.styles.get(sh.style_id(at(2))).num_fmt.as_str(), "m/d/yyyy");
            let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
            let xml = part(&bytes, "xl/styles.xml");
            assert!(xml.contains("<xf numFmtId=\"14\""));
            assert!(xml.contains("<xf numFmtId=\"22\""));
            assert!(xml.contains("formatCode=\"m/d/yyyy\""));
            let (back, report) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
            assert!(report.warnings.is_empty());
            wb = back;
        }
        // Optional, explicit development output. Never overwrite, remove or publish fixtures.
        if let Ok(dir) = std::env::var("GRIDCRAFT_SYNTHETIC_OUTPUT") {
            let name = if sys == DateSystem::D1900 { "synthetic-1900" } else { "synthetic-1904" };
            for (suffix, bytes) in [("input", source), ("roundtrip", gridcraft_xlsx::write_xlsx(&wb).unwrap())] {
                let path = std::path::Path::new(&dir).join(format!("{name}-{suffix}.xlsx"));
                std::fs::OpenOptions::new().write(true).create_new(true).open(path).unwrap().write_all(&bytes).unwrap();
            }
        }
    }
}

/// Marked custom short-date aliases retain their identity through style JSON and XLSX
/// while legacy string representations remain accepted.
#[test]
fn custom_short_date_aliases_survive_style_serialization_and_xlsx() {
    for code in ["m/d/yyyy", "M/D/YYYY", "m\\/d\\/yyyy", "m/d/yyyy h:mm"] {
        let nf = NumFmt::custom(code);
        assert!(nf.is_literal_short_date());
        let json = serde_json::to_string(&nf).unwrap();
        assert_eq!(serde_json::from_str::<NumFmt>(&json).unwrap(), nf);
        assert_ne!(nf, NumFmt::new(code));
        let mut wb = Workbook::new();
        let id = wb.styles.derive(Default::default(), |st| st.num_fmt = nf.clone());
        wb.sheet_mut(0)
            .unwrap()
            .cells
            .set(CellRef::new(0, 0), gridcraft_engine::model::Cell { value: Value::Number(45292.0), formula: None, style: id });
        let (back, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(back.styles.get(back.sheet(0).unwrap().style_id(CellRef::new(0, 0))).num_fmt, nf);
    }
    assert_eq!(serde_json::from_str::<NumFmt>("\"0.00\"").unwrap(), NumFmt::new("0.00"));
}

/// Romanian AutoFit measures localized text while command inputs and stored values stay invariant.
#[test]
fn romanian_autofit_and_commands_keep_numeric_values_and_invariant_input() {
    let mut s = Session::new();
    s.new_workbook();
    s.display_locale = Locale::from_tag("ro-RO");
    s.execute("range.setValues", json!({"range":"A1:A2","values":[[45292],[1234.5]]})).unwrap();
    s.execute("home.numberFormat", json!({"range":"A1","format":"Short Date"})).unwrap();
    s.execute("home.numberFormat", json!({"range":"A2","code":"#,##0.00"})).unwrap();
    s.execute("cell.set", json!({"cell":"A3","input":"=SUM(1.5,2.25)"})).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let seen = calls.clone();
    s.text_width = Some(Arc::new(move |text, _, _| {
        seen.lock().unwrap().push(text.to_string());
        text.len() as f32 * 8.0
    }));
    s.execute("home.autofitColumnWidth", json!({"cols":"A:A"})).unwrap();
    let measured = calls.lock().unwrap();
    assert!(measured.iter().any(|t| t == "01.01.2024"));
    assert!(measured.iter().any(|t| t == "1.234,50"));
    let wb = &s.active().unwrap().wb;
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(CellRef::new(2, 0)), Value::Number(3.75));
    assert_eq!(cell_text(wb, sh, CellRef::new(1, 0)), "1,234.50");
    let before = sh.value(CellRef::new(0, 0));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), before);
}

/// A font edit retains an imported literal date; an intentional date preset and Undo
/// switch and restore its identity through XLSX export.
#[test]
fn formatting_and_undo_preserve_an_imported_custom_date() {
    let (wb, _) = gridcraft_xlsx::read_xlsx(&fixture(DateSystem::D1900)).unwrap();
    let mut s = Session::new();
    s.new_workbook();
    s.active_mut().unwrap().wb = Arc::new(wb);
    s.display_locale = Locale::from_tag("ro-RO");
    s.execute("home.formatCells", json!({"range":"B3","style":{"font":{"bold":true}}})).unwrap();
    let check = |s: &Session, expected: &str| {
        let wb = &s.active().unwrap().wb;
        assert_eq!(shown(wb, 2, &s.display_locale), expected);
        assert_eq!(wb.sheet(0).unwrap().value(at(2)), wb.sheet(0).unwrap().value(at(1)));
    };
    check(&s, "3/4/2026");
    s.execute("home.numberFormat", json!({"range":"B3","format":"Short Date"})).unwrap();
    check(&s, "04.03.2026");
    s.execute("edit.undo", json!({})).unwrap();
    check(&s, "3/4/2026");
    let bytes = gridcraft_xlsx::write_xlsx(&s.active().unwrap().wb).unwrap();
    let (back, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    assert_eq!(shown(&back, 2, &s.display_locale), "3/4/2026");
}
