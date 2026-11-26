use std::sync::Arc;

use gridcraft_core::DateSystem;
use gridcraft_model::{Sheet, Workbook};

use crate::package::Package;
use crate::{read_xlsx, write_xlsx};

/// A macro-enabled workbook keeps the VBA code names of the workbook and its sheets: Excel binds
/// each sheet to its document module by code name, so losing them orphans the modules.
#[test]
fn vba_code_names_roundtrip() {
    let bytes = super::minimal(
        r#"<sheetPr codeName="ModuleData"><tabColor rgb="FF00FF00"/></sheetPr><sheetData/>"#,
        &[("xl/vbaProject.bin", "opaque")],
        r#"<Relationship Id="rId9" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/>"#,
        r#"<workbookPr codeName="ThisWorkbook" date1904="1"/>"#,
    );
    let (wb, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(wb.code_name.as_deref(), Some("ThisWorkbook"));
    assert_eq!(wb.sheet(0).unwrap().code_name.as_deref(), Some("ModuleData"));
    assert_eq!(wb.date_system, DateSystem::D1904);

    let saved = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let book = pkg.read_xml("xl/workbook.xml").unwrap().unwrap();
    let pr = book.child("workbookPr").unwrap();
    assert_eq!(pr.attr("codeName"), Some("ThisWorkbook"));
    assert!(pr.flag("date1904", false));
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    let pr = sheet.child("sheetPr").unwrap();
    assert_eq!(pr.attr("codeName"), Some("ModuleData"));
    assert!(pr.child("tabColor").is_some());

    let (back, _) = read_xlsx(&saved).unwrap();
    assert_eq!(back.code_name.as_deref(), Some("ThisWorkbook"));
    assert_eq!(back.sheet(0).unwrap().code_name.as_deref(), Some("ModuleData"));
}

#[test]
fn vba_code_names_follow_excel_character_rules() {
    for name in ["Data·Sheet", "·Sheet", "表", "\u{301}Data", "١Sheet", "A_09", &"a".repeat(31), &"表".repeat(31), &"😀".repeat(31)] {
        for workbook_name in [false, true] {
            let (book_name, sheet_name) = if workbook_name { (name, "ModuleData") } else { ("ThisWorkbook", name) };
            let bytes = super::minimal(
                &format!(r#"<sheetPr codeName="{sheet_name}"/><sheetData/>"#),
                &[],
                "",
                &format!(r#"<workbookPr codeName="{book_name}"/>"#),
            );
            let (wb, _) = read_xlsx(&bytes).unwrap();
            assert_eq!(wb.code_name.as_deref(), Some(book_name));
            assert_eq!(wb.sheet(0).unwrap().code_name.as_deref(), Some(sheet_name));

            for kind in [crate::XlsxKind::Workbook, crate::XlsxKind::MacroWorkbook, crate::XlsxKind::Template, crate::XlsxKind::MacroTemplate] {
                let saved = crate::write_xlsx_as(&wb, kind).unwrap();
                let mut pkg = Package::open(&saved).unwrap();
                let book = pkg.read_xml("xl/workbook.xml").unwrap().unwrap();
                let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
                assert_eq!(book.child("workbookPr").unwrap().attr("codeName"), Some(book_name));
                assert_eq!(sheet.child("sheetPr").unwrap().attr("codeName"), Some(sheet_name));
                let (back, _) = read_xlsx(&saved).unwrap();
                assert_eq!(back.code_name.as_deref(), Some(book_name));
                assert_eq!(back.sheet(0).unwrap().code_name.as_deref(), Some(sheet_name));
            }
        }
    }
}

#[test]
fn vba_code_names_detect_unicode_clashes() {
    for (first, second, clash) in [
        ("Σ", "ς", true),
        ("ς", "Σ", true),
        ("Σ", "σ", true),
        ("Dáta·", "DÁTA·", true),
        ("ß", "SS", true),
        ("SS", "ẞ", true),
        ("ſ", "S", true),
        ("ﬃ", "FFI", true),
        ("Ꭰ", "ꭰ", true),
        ("İ", "i\u{307}", true),
        ("I", "ı", false),
        ("İ", "i", false),
        ("é", "e", false),
    ] {
        let bytes =
            super::minimal(&format!(r#"<sheetPr codeName="{second}"/><sheetData/>"#), &[], "", &format!(r#"<workbookPr codeName="{first}"/>"#));
        let (mut wb, report) = read_xlsx(&bytes).unwrap();
        let warnings: Vec<&str> = report.warnings.iter().map(String::as_str).filter(|w| w.contains("VBA code name")).collect();
        if clash {
            assert_eq!(warnings, [format!("sheet \"Data\": duplicate VBA code name \"{second}\" was dropped")]);
        } else {
            assert!(warnings.is_empty(), "{warnings:?}");
        }
        let expected = (!clash).then_some(second);
        assert_eq!(wb.code_name.as_deref(), Some(first));
        assert_eq!(wb.sheet(0).unwrap().code_name.as_deref(), expected);
        wb.sheet_mut(0).unwrap().code_name = Some(second.into());
        let saved = write_xlsx(&wb).unwrap();
        let mut pkg = Package::open(&saved).unwrap();
        let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
        assert_eq!(sheet.child("sheetPr").and_then(|p| p.attr("codeName")), expected);
        let (back, report) = read_xlsx(&saved).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(back.code_name.as_deref(), Some(first));
        assert_eq!(back.sheet(0).unwrap().code_name.as_deref(), expected);
    }
}

#[test]
fn vba_code_names_ignore_invalid_conflicts_on_save() {
    let mut wb = Workbook::new();
    wb.code_name = Some("S".repeat(32));
    wb.sheet_mut(0).unwrap().code_name = wb.code_name.clone();
    let mut sheet = Sheet::new("Data");
    let valid_name = "ß".repeat(16);
    sheet.code_name = Some(valid_name.clone());
    wb.sheets.push(Arc::new(sheet));
    let saved = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet2.xml").unwrap().unwrap();
    assert_eq!(sheet.child("sheetPr").unwrap().attr("codeName"), Some(valid_name.as_str()));
    let (back, report) = read_xlsx(&saved).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(back.code_name.is_none());
    assert!(back.sheet(0).unwrap().code_name.is_none());
    assert_eq!(back.sheet(1).unwrap().code_name.as_deref(), Some(valid_name.as_str()));
}

#[test]
fn vba_code_names_are_valid_and_unique_on_save() {
    let mut book =
        format!(r#"<workbook xmlns="{}" xmlns:r="{}"><workbookPr codeName="ThisWorkbook"/><sheets>"#, crate::write::NS_MAIN, crate::write::NS_REL,);
    let mut rels = String::from(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let mut parts = Vec::new();
    for (i, (name, code)) in
        [("A", "Sheet1"), ("B", "SHEET1"), ("C", "thisworkbook"), ("D", "1st"), ("E", "Sheet_2"), ("F", "sheet1"), ("G", "Σ"), ("H", "ς")]
            .into_iter()
            .enumerate()
    {
        let id = i + 1;
        book.push_str(&format!(r#"<sheet name="{name}" sheetId="{id}" r:id="rId{id}"/>"#));
        rels.push_str(&format!(r#"<Relationship Id="rId{id}" Type="{}/worksheet" Target="worksheets/sheet{id}.xml"/>"#, crate::write::NS_REL));
        parts.push((
            format!("xl/worksheets/sheet{id}.xml"),
            format!(r#"<worksheet xmlns="{}"><sheetPr codeName="{code}"/><sheetData/></worksheet>"#, crate::write::NS_MAIN),
        ));
    }
    book.push_str("</sheets></workbook>");
    rels.push_str("</Relationships>");
    parts.extend([
        ("[Content_Types].xml".into(), super::CT.into()),
        ("_rels/.rels".into(), super::ROOT_RELS.into()),
        ("xl/workbook.xml".into(), book),
        ("xl/_rels/workbook.xml.rels".into(), rels),
    ]);
    let bytes = super::make_zip(&parts.iter().map(|(name, data)| (name.as_str(), data.as_bytes())).collect::<Vec<_>>());
    let (wb, report) = read_xlsx(&bytes).unwrap();
    let warnings: Vec<&str> = report.warnings.iter().map(String::as_str).filter(|w| w.contains("VBA code name")).collect();
    assert_eq!(warnings.len(), 5, "{warnings:?}");
    for warning in [
        r#"sheet "B": duplicate VBA code name "SHEET1" was dropped"#,
        r#"sheet "C": duplicate VBA code name "thisworkbook" was dropped"#,
        r#"sheet "D": invalid VBA code name "1st" was dropped"#,
        r#"sheet "F": duplicate VBA code name "sheet1" was dropped"#,
        r#"sheet "H": duplicate VBA code name "ς" was dropped"#,
    ] {
        assert!(warnings.contains(&warning), "{warnings:?}");
    }
    let expected = [Some("Sheet1"), None, None, None, Some("Sheet_2"), None, Some("Σ"), None];
    assert_eq!(wb.code_name.as_deref(), Some("ThisWorkbook"));
    assert_eq!(wb.sheets.iter().map(|s| s.code_name.as_deref()).collect::<Vec<_>>(), expected);
    let (back, report) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let codes: Vec<Option<&str>> = back.sheets.iter().map(|s| s.code_name.as_deref()).collect();
    assert_eq!(codes, expected);
    assert_eq!(back.code_name, wb.code_name);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let plain = write_xlsx(&Workbook::new()).unwrap();
    let mut pkg = Package::open(&plain).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    assert_eq!(sheet.child("sheetPr").and_then(|p| p.attr("codeName")), None);
}

#[test]
fn vba_code_names_rejected_on_import_are_reported() {
    for code in ["", "1st", "_Sheet", "Bad Name", "Bad.Name", "Bad&Name", "Bad\"Name", &"a".repeat(32), &"表".repeat(32)] {
        let escaped = crate::xml::esc_attr(code);
        let bytes =
            super::minimal(&format!(r#"<sheetPr codeName="{escaped}"/><sheetData/>"#), &[], "", &format!(r#"<workbookPr codeName="{escaped}"/>"#));
        let (wb, report) = read_xlsx(&bytes).unwrap();
        assert!(wb.code_name.is_none());
        assert!(wb.sheet(0).unwrap().code_name.is_none());
        let warnings: Vec<&str> = report.warnings.iter().map(String::as_str).filter(|w| w.contains("VBA code name")).collect();
        assert_eq!(
            warnings,
            [
                format!("workbook: invalid VBA code name \"{code}\" was dropped"),
                format!("sheet \"Data\": invalid VBA code name \"{code}\" was dropped")
            ]
        );
        let (back, report) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert!(back.code_name.is_none());
        assert!(back.sheet(0).unwrap().code_name.is_none());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }
}
