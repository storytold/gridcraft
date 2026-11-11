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
        wb.sheet_mut(0).unwrap().cells.set(CellRef::new(0, 0), gridcraft_engine::model::Cell::styled(Value::Number(45292.0), id));
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

/// Explicit dates from a synthetic SYLK remain literal in a Romanian region and after XLSX saves.
#[test]
fn synthetic_sylk_keeps_literal_dates_after_xlsx_export() {
    let date = gridcraft_engine::core::parse::parse_input("2026-03-04", DateSystem::D1900).value.as_f64().unwrap();
    let loc = Locale::from_tag("ro-RO");
    let at = CellRef::new(0, 0);
    for (code, value, expected) in
        [("m/d/yyyy", date, "3/4/2026"), ("m/d/yyyy h:mm", date + 0.5, "3/4/2026 12:00"), ("dd.mm.yyyy", date, "04.03.2026")]
    {
        let source = format!("ID;PSynthetic\nP;P{code}\nF;P0;Y1;X1\nC;Y1;X1;K{value}\nE\n");
        let (mut wb, warnings) = gridcraft_engine::io::open_bytes("Synthetic.slk", source.as_bytes()).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        for _ in 0..3 {
            let sh = wb.sheet(0).unwrap();
            assert_eq!(cell_text_in(&wb, sh, at, &loc), expected, "{code}");
            assert_eq!(sh.value(at), Value::Number(value));
            assert_eq!(wb.styles.get(sh.style_id(at)).num_fmt.as_str(), code);
            let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
            let (back, report) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
            assert!(report.warnings.is_empty());
            wb = back;
        }
    }
}

/// The interactive boundary reads Romanian numbers/dates without changing command conventions
/// or stored values across XLSX export, in either Excel date system.
#[test]
fn interactive_romanian_entry_survives_xlsx_in_both_date_systems() {
    let loc = Locale::from_tag("ro-RO");
    for sys in [DateSystem::D1900, DateSystem::D1904] {
        let mut s = Session::new();
        s.new_workbook();
        Arc::make_mut(&mut s.active_mut().unwrap().wb).date_system = sys;
        let date = gridcraft_engine::core::parse::parse_input("2026-03-04", sys).value.as_f64().unwrap();
        let cases = [
            ("1,234", Value::Number(1.234)),
            ("1.234,56", Value::Number(1234.56)),
            ("-1.234,56", Value::Number(-1234.56)),
            ("12,5%", Value::Number(0.125)),
            ("1,2E3", Value::Number(1200.0)),
            ("04.03.2026", Value::Number(date)),
            ("04.03.2026 12:30", Value::Number(date + 12.5 / 24.0)),
            ("2026-03-04", Value::Number(date)),
            ("1.234.567", Value::Number(1234567.0)),
            ("1.002.024", Value::Number(1002024.0)),
            ("1.23", Value::text("1.23")),
            ("11.13.2026", Value::text("11.13.2026")),
        ];
        for (row, (input, expected)) in cases.iter().enumerate() {
            let cell = CellRef::new(row as u32, 0);
            s.run_interactive("cell.set", json!({"cell":cell.a1(),"input":input}), &loc).unwrap();
            assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(cell), *expected, "{input}");
        }
        let mut wb = (*s.active().unwrap().wb).clone();
        for _ in 0..3 {
            let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
            let (back, report) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
            assert!(report.warnings.is_empty());
            assert_eq!(back.date_system, sys);
            for (row, (_, expected)) in cases.iter().enumerate() {
                assert_eq!(back.sheet(0).unwrap().value(CellRef::new(row as u32, 0)), *expected);
            }
            wb = back;
        }
    }
}

/// Region is explicit and per command: CLI/headless entry, formulas and calls after a failed
/// interactive command retain their invariant parsing.
#[test]
fn interactive_region_never_leaks_into_headless_commands_or_formulas() {
    let mut s = Session::new();
    s.new_workbook();
    let ro = Locale::from_tag("ro-RO");
    s.display_locale = ro.clone();
    s.run_interactive("cell.set", json!({"cell":"A1","input":"1,234"}), &ro).unwrap();
    s.execute_headless("cell.set", json!({"cell":"A2","input":"1,234"})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(1.234));
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Number(1234.0));
    s.run_interactive("cell.set", json!({"cell":"A3","input":"=SUM(1.5,2.25)"}), &ro).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(2, 0)), Value::Number(3.75));
    assert!(s.run_interactive("cell.set", json!({"cell":"A4","input":"=1+"}), &ro).is_err());
    s.execute("cell.set", json!({"cell":"A4","input":"1,234"})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(3, 0)), Value::Number(1234.0));
    let (csv, _) = gridcraft_engine::io::open_bytes("Synthetic.csv", b"1.234\n").unwrap();
    assert_eq!(csv.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(1.234));
}

/// Ctrl+Enter respects each destination's Text format and remains one undoable edit.
#[test]
fn interactive_fill_and_external_paste_keep_text_and_numeric_destinations() {
    let mut s = Session::new();
    s.new_workbook();
    let ro = Locale::from_tag("ro-RO");
    s.execute("home.formatCells", json!({"range":"A1","style":{"num_fmt":"@"}})).unwrap();
    s.execute("selection.set", json!({"range":"A1:B2"})).unwrap();
    s.run_interactive("range.fill", json!({"input":"1,234"}), &ro).unwrap();
    let sh = s.active().unwrap().wb.sheet(0).unwrap();
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::text("1,234"));
    assert_eq!(sh.value(CellRef::new(0, 1)), Value::Number(1.234));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 1)), Value::Empty);
    s.run_interactive("edit.paste", json!({"at":"A1","text":"1,234\t04.03.2026\n1.234,56\t12,5%"}), &ro).unwrap();
    let sh = s.active().unwrap().wb.sheet(0).unwrap();
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::text("1,234"));
    assert_eq!(sh.value(CellRef::new(0, 1)), Value::Number(46085.0));
    assert_eq!(sh.value(CellRef::new(1, 0)), Value::Number(1234.56));
    assert_eq!(sh.value(CellRef::new(1, 1)), Value::Number(0.125));
    s.execute("selection.set", json!({"range":"A2"})).unwrap();
    let copy = s.execute("edit.copy", json!({})).unwrap();
    s.run_interactive("edit.paste", json!({"at":"D1","text":copy["text"]}), &ro).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 3)), Value::Number(1234.56));
}

/// F2 text uses full regional precision and preserves the order of explicit custom dates.
#[test]
fn interactive_editor_roundtrips_builtin_and_literal_date_formats() {
    let mut s = Session::new();
    s.new_workbook();
    let ro = Locale::from_tag("ro-RO");
    {
        let wb = Arc::make_mut(&mut s.active_mut().unwrap().wb);
        let custom = wb.styles.derive(Default::default(), |st| st.num_fmt = NumFmt::custom("m/d/yyyy"));
        wb.sheet_mut(0).unwrap().set_style(CellRef::new(0, 0), custom);
    }
    s.run_interactive("cell.set", json!({"cell":"A1","input":"3/4/2026"}), &ro).unwrap();
    s.run_interactive("cell.set", json!({"cell":"A2","input":"04.03.2026"}), &ro).unwrap();
    s.execute("cell.set", json!({"cell":"A3","input":"1.2345678901234"})).unwrap();
    s.execute("cell.set", json!({"cell":"A4","input":"'1,23"})).unwrap();
    let wb = &s.active().unwrap().wb;
    assert_eq!(gridcraft_engine::entry::editing_text(wb, 0, CellRef::new(0, 0), &ro), "3/4/2026");
    assert_eq!(gridcraft_engine::entry::editing_text(wb, 0, CellRef::new(1, 0), &ro), "04.03.2026");
    assert_eq!(gridcraft_engine::entry::editing_text(wb, 0, CellRef::new(2, 0), &ro), "1,2345678901234");
    assert_eq!(gridcraft_engine::entry::editing_text(wb, 0, CellRef::new(3, 0), &ro), "'1,23");
    assert_eq!(wb.sheet(0).unwrap().value(CellRef::new(0, 0)), wb.sheet(0).unwrap().value(CellRef::new(1, 0)));
}

/// A recorded interactive command is self-contained: headless replay retains its numeric region.
#[test]
fn recorded_interactive_entry_replays_with_its_explicit_numeric_region() {
    let mut s = Session::new();
    s.new_workbook();
    s.start_recording();
    s.run_interactive("cell.set", json!({"cell":"A1","input":"1,234"}), &Locale::from_tag("ro-RO")).unwrap();
    let recording = s.stop_recording().unwrap();
    let step = recording.steps.iter().find(|step| step.id == "cell.set").unwrap();
    assert_eq!(step.params["inputLocale"]["decimal"], ",");
    s.execute("cell.set", json!({"cell":"A1","input":"0"})).unwrap();
    s.execute_headless(&step.id, step.params.clone()).unwrap();
    assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(1.234));
}

/// Malformed explicit numeric conventions are refused before they can change a cell.
#[test]
fn invalid_interactive_snapshots_leave_values_unchanged() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("cell.set", json!({"cell":"A1","input":"7"})).unwrap();
    for locale in [
        json!(null),
        json!({"decimal":",","group":".","shortDate":"dd.mm.yyyy","percentPrefix":"yes"}),
        json!({"decimal":",","group":",","shortDate":"dd.mm.yyyy"}),
        json!({"decimal":"..","group":",","shortDate":"dd.mm.yyyy"}),
        json!({"decimal":",","group":".","shortDate":"d".repeat(256)}),
        json!({"decimal":",","group":"\n","shortDate":"dd.mm.yyyy"}),
        json!({"decimal":",","group":".","shortDate":"h:mm"}),
        json!({"decimal":",","group":".","shortDate":"dd/mm"}),
        json!({"decimal":",","group":".","shortDate":"dd-mmm-yyyy"}),
    ] {
        assert!(s.execute("cell.set", json!({"cell":"A1","input":"1,234","inputLocale":locale})).is_err());
        assert_eq!(s.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(7.0));
    }
}

/// Independent Romanian sessions exchange regional plain text; agent copies stay invariant.
#[test]
fn regional_clipboard_plain_text_keeps_decimal_values_between_sessions() {
    let ro = Locale::from_tag("ro-RO");
    let mut source = Session::new();
    source.new_workbook();
    source.display_locale = ro.clone();
    source.execute("cell.set", json!({"cell":"A1","input":"1.234"})).unwrap();
    let copied = source.run_interactive("edit.copy", json!({"range":"A1","html":true}), &ro).unwrap();
    let mut dest = Session::new();
    dest.new_workbook();
    dest.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"]}), &ro).unwrap();
    assert_eq!(dest.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(1.234));
    assert_eq!(copied["text"], "1,234\n");
    // Fresh en-US sessions prove the host parsing path and its plain-text ambiguity.
    let mut en_plain = Session::new();
    en_plain.new_workbook();
    en_plain.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"]}), &Locale::EN_US).unwrap();
    assert_eq!(en_plain.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(1234.0));
    let mut en_rich = Session::new();
    en_rich.new_workbook();
    en_rich.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"],"html":copied["html"]}), &Locale::EN_US).unwrap();
    assert_eq!(en_rich.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(1.234));
    // Separately, internal clipboard identity must survive a later region change.
    source.run_interactive("edit.paste", json!({"at":"B1","text":copied["text"]}), &Locale::EN_US).unwrap();
    assert_eq!(source.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 1)), Value::Number(1.234));
    assert_eq!(source.execute("edit.copy", json!({"range":"A1"})).unwrap()["text"], "1.234\n");
}

/// A rich copy carries its numeric region across sessions, even across date systems.
#[test]
fn regional_clipboard_html_keeps_numbers_percentages_and_calendar_dates() {
    for source_system in [DateSystem::D1900, DateSystem::D1904] {
        for target_system in [DateSystem::D1900, DateSystem::D1904] {
            for (source_locale, target_locale) in [("ro-RO", "en-US"), ("en-US", "ro-RO"), ("ro-RO", "ro-RO")] {
                let mut source = Session::new();
                source.new_workbook();
                Arc::make_mut(&mut source.active_mut().unwrap().wb).date_system = source_system;
                // The short date hides this time fraction, so text-only fallback cannot pass.
                let serial = gridcraft_engine::core::parse::parse_input("2026-03-04", source_system).value.as_f64().unwrap() + 0.625;
                source.execute("range.setValues", json!({"range":"A1:C1","values":[[1.2345678901234,0.125,serial]]})).unwrap();
                source.execute("home.formatCells", json!({"range":"B1","style":{"num_fmt":"0.00%"}})).unwrap();
                source.execute("home.numberFormat", json!({"range":"C1","format":"Short Date"})).unwrap();
                let copied = source.run_interactive("edit.copy", json!({"range":"A1:C1","html":true}), &Locale::from_tag(source_locale)).unwrap();
                let mut dest = Session::new();
                dest.new_workbook();
                Arc::make_mut(&mut dest.active_mut().unwrap().wb).date_system = target_system;
                dest.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"],"html":copied["html"]}), &Locale::from_tag(target_locale))
                    .unwrap();
                let sh = dest.active().unwrap().wb.sheet(0).unwrap();
                assert_eq!(sh.value(CellRef::new(0, 0)), Value::Number(1.2345678901234), "{source_locale} to {target_locale}");
                assert_eq!(sh.value(CellRef::new(0, 1)), Value::Number(0.125));
                let expected = Value::Number(gridcraft_engine::core::parse::parse_input("2026-03-04", target_system).value.as_f64().unwrap() + 0.625);
                assert_eq!(sh.value(CellRef::new(0, 2)), expected, "{source_locale} to {target_locale}, {source_system:?} to {target_system:?}");
            }
        }
    }
}

/// Stale HTML and malformed/oversized region metadata leave ordinary regional input intact.
#[test]
fn regional_clipboard_ignores_unmatched_or_invalid_html_region() {
    let valid = json!({"decimal":".","group":",","shortDate":"m/d/yyyy"});
    let invalid = json!({"decimal":".","group":".","shortDate":"m/d/yyyy"});
    let table = |region: &str, shown: &str| format!("<table data-gridcraft-region-v1=\"{region}\"><tr><td>{shown}</td></tr></table>");
    let encode = |v: &serde_json::Value| gridcraft_engine::io::base64_encode(v.to_string().as_bytes());
    for (html, field, expected) in [
        (table(&encode(&valid), "1,234"), "2,345", 2.345),
        (table(&encode(&invalid), "1.234"), "1.234", 1234.0),
        (table(&"A".repeat(5000), "1.234"), "1.234", 1234.0),
    ] {
        let mut dest = Session::new();
        dest.new_workbook();
        dest.run_interactive("edit.paste", json!({"at":"A1","text":field,"html":html}), &Locale::from_tag("ro-RO")).unwrap();
        assert_eq!(dest.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(expected));
    }
}

/// Exact HTML values survive display rounding, custom date order and different date systems.
#[test]
fn regional_clipboard_exact_values_respect_text_and_formula_destinations() {
    let ro = Locale::from_tag("ro-RO");
    let mut source = Session::new();
    source.new_workbook();
    let date = gridcraft_engine::core::parse::parse_input("2026-03-04", DateSystem::D1900).value.as_f64().unwrap();
    source.execute("range.setValues", json!({"range":"A1:D1","values":[[1.2345678901234,date,0.5,1.5]]})).unwrap();
    {
        let wb = Arc::make_mut(&mut source.active_mut().unwrap().wb);
        let custom = wb.styles.derive(Default::default(), |style| style.num_fmt = NumFmt::custom("m/d/yyyy"));
        wb.sheet_mut(0).unwrap().set_style(CellRef::new(0, 1), custom);
    }
    source.execute("home.formatCells", json!({"range":"C1","style":{"num_fmt":"h:mm"}})).unwrap();
    source.execute("home.formatCells", json!({"range":"D1","style":{"num_fmt":"[h]:mm"}})).unwrap();
    let copied = source.run_interactive("edit.copy", json!({"range":"A1:D1","html":true}), &ro).unwrap();
    let mut dest = Session::new();
    dest.new_workbook();
    Arc::make_mut(&mut dest.active_mut().unwrap().wb).date_system = DateSystem::D1904;
    dest.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"],"html":copied["html"]}), &ro).unwrap();
    let sh = dest.active().unwrap().wb.sheet(0).unwrap();
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::Number(1.2345678901234));
    assert_eq!(sh.value(CellRef::new(0, 1)), gridcraft_engine::core::parse::parse_input("2026-03-04", DateSystem::D1904).value);
    assert_eq!(sh.value(CellRef::new(0, 2)), Value::Number(0.5));
    assert_eq!(sh.value(CellRef::new(0, 3)), Value::Number(1.5));
    let one = source.run_interactive("edit.copy", json!({"range":"A1","html":true}), &ro).unwrap();
    dest.execute("home.formatCells", json!({"range":"A2","style":{"num_fmt":"@"}})).unwrap();
    dest.run_interactive("edit.paste", json!({"at":"A2","text":one["text"],"html":one["html"]}), &ro).unwrap();
    assert_eq!(dest.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::text(one["text"].as_str().unwrap().trim_end()));
    let region = gridcraft_engine::io::base64_encode(json!({"decimal":".","group":",","shortDate":"m/d/yyyy"}).to_string().as_bytes());
    let formula_html = format!("<table data-gridcraft-region-v1=\"{region}\"><tr><td data-gridcraft-number-v1=\"999\">=1+2</td></tr></table>");
    dest.run_interactive("edit.paste", json!({"at":"B2","text":"=1+2","html":formula_html}), &ro).unwrap();
    let cell = dest.active().unwrap().wb.sheet(0).unwrap().cell(CellRef::new(1, 1)).unwrap();
    assert!(cell.formula.is_some());
    assert_eq!(cell.value, Value::Number(3.0));
    for attributes in [
        "data-gridcraft-number-v1=\"999\"",
        "data-gridcraft-number-v1=\"NaN\"",
        "data-gridcraft-number-v1=\"1e999\"",
        "data-gridcraft-number-v1=\"999\" data-gridcraft-calendar-v1=\"invalid\"",
    ] {
        let html = format!("<table data-gridcraft-region-v1=\"{region}\"><tr><td {attributes}>1.234</td></tr></table>");
        dest.run_interactive("edit.paste", json!({"at":"C2","text":"1.234","html":html}), &ro).unwrap();
        assert_eq!(dest.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(1, 2)), Value::Number(1.234));
    }
}

/// Financial formats can hide zeros visually without turning their stored values into blanks.
#[test]
fn regional_clipboard_keeps_numeric_zero_when_format_shows_blank() {
    for range in ["A1", "A1:B1"] {
        let mut source = Session::new();
        source.new_workbook();
        source.execute("range.setValues", json!({"range":"A1:B1","values":[[0,1.234]]})).unwrap();
        source.execute("home.formatCells", json!({"range":"A1","style":{"num_fmt":"0;-0;;@"}})).unwrap();
        let copied = source.run_interactive("edit.copy", json!({"range":range,"html":true}), &Locale::from_tag("ro-RO")).unwrap();
        let mut dest = Session::new();
        dest.new_workbook();
        dest.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"],"html":copied["html"]}), &Locale::from_tag("ro-RO")).unwrap();
        assert_eq!(dest.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(0.0), "{range}");
        if range == "A1:B1" {
            assert_eq!(dest.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 1)), Value::Number(1.234));
        }
        dest.execute("home.formatCells", json!({"range":"A2","style":{"num_fmt":"@"}})).unwrap();
        dest.run_interactive("edit.paste", json!({"at":"A2","text":copied["text"],"html":copied["html"]}), &Locale::from_tag("ro-RO")).unwrap();
        assert_eq!(dest.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(1, 0)), Value::Empty);
    }
}

/// Rounded source formats validate exact values and preserve a currency's display in General.
#[test]
fn regional_clipboard_validates_rounded_number_metadata_and_currency_format() {
    let mut source = Session::new();
    source.new_workbook();
    source.execute("range.setValues", json!({"range":"A1:B1","values":[[1.234567,10000.125]]})).unwrap();
    source.execute("home.formatCells", json!({"range":"A1","style":{"num_fmt":"0.00"}})).unwrap();
    source.execute("home.formatCells", json!({"range":"B1","style":{"num_fmt":"\"RON \"#,##0.00"}})).unwrap();
    let copied = source.run_interactive("edit.copy", json!({"range":"A1:B1","html":true}), &Locale::from_tag("ro-RO")).unwrap();
    let mut dest = Session::new();
    dest.new_workbook();
    dest.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"],"html":copied["html"]}), &Locale::from_tag("ro-RO")).unwrap();
    let wb = &dest.active().unwrap().wb;
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(CellRef::new(0, 0)), Value::Number(1.234567));
    assert_eq!(sh.value(CellRef::new(0, 1)), Value::Number(10000.125));
    assert_eq!(cell_text_in(wb, sh, CellRef::new(0, 0), &Locale::from_tag("ro-RO")), "1,23");
    assert_eq!(cell_text_in(wb, sh, CellRef::new(0, 1), &Locale::from_tag("ro-RO")), "RON 10.000,13");
    let bad_html = copied["html"].as_str().unwrap().replacen("data-gridcraft-number-v1=\"1.234567\"", "data-gridcraft-number-v1=\"999\"", 1);
    assert_ne!(bad_html, copied["html"].as_str().unwrap());
    let mut clean = Session::new();
    clean.new_workbook();
    clean.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"],"html":bad_html}), &Locale::from_tag("ro-RO")).unwrap();
    assert_eq!(clean.active().unwrap().wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::Number(1.23));
}

/// Parser-inferred percent/date formats must not hide captured precision in a General target.
#[test]
fn regional_clipboard_captures_general_before_inferred_formatting() {
    let ro = Locale::from_tag("ro-RO");
    let mut source = Session::new();
    source.new_workbook();
    let date = gridcraft_engine::core::parse::parse_input("2026-03-04", DateSystem::D1900).value.as_f64().unwrap();
    source.execute("range.setValues", json!({"range":"A1:B1","values":[[0.1256789,date]]})).unwrap();
    source.execute("home.formatCells", json!({"range":"A1","style":{"num_fmt":"0.0000%"}})).unwrap();
    {
        let wb = Arc::make_mut(&mut source.active_mut().unwrap().wb);
        let custom = wb.styles.derive(Default::default(), |style| style.num_fmt = NumFmt::custom("m/d/yyyy"));
        wb.sheet_mut(0).unwrap().set_style(CellRef::new(0, 1), custom);
    }
    let copied = source.run_interactive("edit.copy", json!({"range":"A1:B1","html":true}), &ro).unwrap();
    let mut dest = Session::new();
    dest.new_workbook();
    Arc::make_mut(&mut dest.active_mut().unwrap().wb).date_system = DateSystem::D1904;
    dest.run_interactive("edit.paste", json!({"at":"A1","text":copied["text"],"html":copied["html"]}), &ro).unwrap();
    {
        let wb = &dest.active().unwrap().wb;
        let sh = wb.sheet(0).unwrap();
        assert_eq!(sh.value(CellRef::new(0, 0)), Value::Number(0.1256789));
        assert_eq!(wb.styles.get(sh.style_id(CellRef::new(0, 0))).num_fmt.as_str(), "0.0000%");
        assert_eq!(cell_text_in(wb, sh, CellRef::new(0, 0), &ro), "12,5679%");
        assert!(wb.styles.get(sh.style_id(CellRef::new(0, 1))).num_fmt.is_literal_short_date());
        assert_eq!(cell_text_in(wb, sh, CellRef::new(0, 1), &ro), "3/4/2026");
    }
    dest.execute("home.formatCells", json!({"range":"A2","style":{"num_fmt":"0.0%"}})).unwrap();
    dest.execute("home.formatCells", json!({"range":"B2","style":{"num_fmt":"yyyy-mm-dd"}})).unwrap();
    dest.run_interactive("edit.paste", json!({"at":"A2","text":copied["text"],"html":copied["html"]}), &ro).unwrap();
    let wb = &dest.active().unwrap().wb;
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.value(CellRef::new(1, 0)), Value::Number(0.1256789));
    assert_eq!(wb.styles.get(sh.style_id(CellRef::new(1, 0))).num_fmt.as_str(), "0.0%");
    assert_eq!(wb.styles.get(sh.style_id(CellRef::new(1, 1))).num_fmt.as_str(), "yyyy-mm-dd");
    assert_eq!(cell_text_in(wb, sh, CellRef::new(1, 1), &ro), "2026-03-04");
}

/// Exercise the actual UI command snapshot, percentage inference and unchanged regional entry.
#[test]
fn interactive_turkish_leading_percent_survives_the_command_boundary() {
    for (tag, input, expected) in [
        ("tr-TR", "%15", Value::Number(0.15)),
        ("tr-TR", "%15,5", Value::Number(0.155)),
        ("ro-RO", "15,5%", Value::Number(0.155)),
        ("en-US", "15.5%", Value::Number(0.155)),
        ("ro-RO", "%15", Value::text("%15")),
        ("en-US", "%15", Value::text("%15")),
        ("tr-TR", "1.5", Value::text("1.5")),
        ("tr-TR", "₺46,00", Value::text("₺46,00")),
    ] {
        let mut s = Session::new();
        s.new_workbook();
        let loc = Locale::from_tag(tag);
        s.run_interactive("cell.set", json!({"cell":"A1","input":input}), &loc).unwrap();
        let wb = &s.active().unwrap().wb;
        let sh = wb.sheet(0).unwrap();
        let at = CellRef::parse("A1").unwrap();
        assert_eq!(sh.value(at), expected, "{tag}: {input}");
        if matches!(expected, Value::Number(_)) {
            assert!(gridcraft_engine::numfmt::NumberFormat::parse(wb.styles.get(sh.style_id(at)).num_fmt.as_str()).is_percent());
        }
    }
}
