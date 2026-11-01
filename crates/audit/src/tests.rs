//! Golden tests for `gridcraft-audit`. Every fixture is synthetic, written from
//! ECMA-376 in code; names are made up. No Excel-produced files are committed.

use std::io::{Cursor, Write};

use super::*;

const NS_X: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

fn zip(entries: &[(&str, String)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, text) in entries {
        z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(text.as_bytes()).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn content_types() -> String {
    r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="x"/><Default Extension="xml" ContentType="x"/><Override PartName="/xl/workbook.xml" ContentType="x"/></Types>"#.to_string()
}

fn root_rels() -> String {
    format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RT}officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="{RT}extended-properties" Target="docProps/app.xml"/></Relationships>"#
    )
}

fn workbook_rels(targets: &[(&str, &str)]) -> String {
    let mut rels = String::from(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    for (id, target) in targets {
        rels.push_str(&format!(r#"<Relationship Id="{id}" Type="{RT}worksheet" Target="{target}"/>"#));
    }
    rels.push_str("</Relationships>");
    rels
}

fn workbook(sheets: &str, extra: &str) -> String {
    format!(r#"<workbook xmlns="{NS_X}" xmlns:r="{NS_R}"><sheets>{sheets}</sheets>{extra}</workbook>"#)
}

fn sheet(name: &str, rid: &str, state: &str) -> String {
    let state_attr = if state.is_empty() { String::new() } else { format!(r#" state="{state}""#) };
    format!(r#"<sheet name="{name}" sheetId="1" r:id="{rid}"{state_attr}/>"#)
}

/// A minimal package: one visible sheet, no hidden content, no authors.
fn clean_fixture() -> Vec<u8> {
    let sheet_xml = format!(r#"<worksheet xmlns="{NS_X}"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#);
    zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", sheet_xml),
        (
            "docProps/core.xml",
            r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"/>"#.to_string(),
        ),
    ])
}

fn findings(pkg: &[u8]) -> Vec<AuditFinding> {
    audit_bytes(pkg).unwrap()
}

fn ids(fs: &[AuditFinding]) -> Vec<&str> {
    fs.iter().map(|f| f.id.as_str()).collect()
}

#[test]
fn clean_file_has_no_findings() {
    assert!(findings(&clean_fixture()).is_empty());
}

#[test]
fn finding_ids_are_stable() {
    let all = [
        (FindingKind::HiddenRow, "hidden/row"),
        (FindingKind::HiddenColumn, "hidden/column"),
        (FindingKind::HiddenSheet, "hidden/sheet"),
        (FindingKind::HiddenDefinedName, "hidden/defined-name"),
        (FindingKind::ExternalLink, "external-link"),
        (FindingKind::CommentAuthor, "author-pii/comment"),
        (FindingKind::ThreadedCommentAuthor, "author-pii/threaded-comment"),
        (FindingKind::CorePropertiesAuthor, "author-pii/core-properties"),
        (FindingKind::ExtendedProperties, "author-pii/extended-properties"),
        (FindingKind::PivotCache, "pivot-cache"),
        (FindingKind::VbaProject, "active-content/vba"),
        (FindingKind::OleEmbedding, "active-content/ole"),
        (FindingKind::OrphanPart, "package/orphan-part"),
    ];
    for (kind, id) in all {
        assert_eq!(kind.id(), id, "finding ID renamed");
    }
}

#[test]
fn hidden_rows_and_columns_found() {
    // Unusual prefix `x:`: matching is by namespace URI, not prefix.
    let sheet_xml = format!(
        r#"<x:worksheet xmlns:x="{NS_X}"><x:cols><x:col min="2" max="4" width="9" hidden="1" customWidth="1"/><x:col min="5" max="5" width="9"/></x:cols><x:sheetData><x:row r="1"><x:c r="A1"><x:v>1</x:v></x:c></x:row><x:row r="3" hidden="1"><x:c r="A3"><x:v>2</x:v></x:c></x:row><x:row r="4" hidden="0"><x:c r="A4"><x:v>3</x:v></x:c></x:row></x:sheetData></x:worksheet>"#
    );
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", sheet_xml),
    ]);
    let fs = findings(&pkg);
    let rows: Vec<_> = fs.iter().filter(|f| f.id == "hidden/row").collect();
    let cols: Vec<_> = fs.iter().filter(|f| f.id == "hidden/column").collect();
    assert_eq!(rows.len(), 1, "{fs:?}");
    assert!(rows[0].detail.contains('3'), "{rows:?}");
    assert!(rows[0].location.contains("sheet1.xml"), "{rows:?}");
    assert_eq!(cols.len(), 1, "{fs:?}");
    assert!(cols[0].detail.contains("B:D"), "{cols:?}");
    // hidden="0" is not flagged.
    assert!(!fs.iter().any(|f| f.detail.contains('4')), "{fs:?}");
}

#[test]
fn single_hidden_column_reports_letter() {
    let sheet_xml = format!(r#"<worksheet xmlns="{NS_X}"><cols><col min="1" max="1" hidden="true"/></cols><sheetData/></worksheet>"#);
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", sheet_xml),
    ]);
    let fs = findings(&pkg);
    let cols: Vec<_> = fs.iter().filter(|f| f.id == "hidden/column").collect();
    assert_eq!(cols.len(), 1);
    assert!(cols[0].detail.contains('A'), "{cols:?}");
}

#[test]
fn hidden_and_very_hidden_sheets_found() {
    let sheets = format!("{}{}{}", sheet("Budget", "rId1", ""), sheet("Salaries", "rId2", "hidden"), sheet("Keys", "rId3", "veryHidden"));
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheets, "")),
        (
            "xl/_rels/workbook.xml.rels",
            workbook_rels(&[("rId1", "worksheets/sheet1.xml"), ("rId2", "worksheets/sheet2.xml"), ("rId3", "worksheets/sheet3.xml")]),
        ),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("xl/worksheets/sheet2.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("xl/worksheets/sheet3.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
    ]);
    let fs = findings(&pkg);
    let hidden: Vec<_> = fs.iter().filter(|f| f.id == "hidden/sheet").collect();
    assert_eq!(hidden.len(), 2, "{fs:?}");
    assert!(hidden.iter().any(|f| f.location.contains("Salaries") && f.detail.contains("hidden") && !f.detail.contains("veryHidden")));
    assert!(hidden.iter().any(|f| f.location.contains("Keys") && f.detail.contains("veryHidden")));
}

#[test]
fn hidden_defined_name_found() {
    let wb = workbook(
        &sheet("Budget", "rId1", ""),
        r#"<definedNames><definedName name="Visible">Budget!$A$1</definedName><definedName name="Secret" hidden="1">Budget!$B$2</definedName></definedNames>"#,
    );
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", wb),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
    ]);
    let fs = findings(&pkg);
    let names: Vec<_> = fs.iter().filter(|f| f.id == "hidden/defined-name").collect();
    assert_eq!(names.len(), 1, "{fs:?}");
    assert!(names[0].detail.contains("Secret"), "{names:?}");
    assert_eq!(names[0].severity, "info");
}

#[test]
fn external_link_found() {
    let wb = workbook(&sheet("Budget", "rId1", ""), r#"<externalReferences><externalReference r:id="rId9"/></externalReferences>"#);
    let mut rels = workbook_rels(&[("rId1", "worksheets/sheet1.xml")]);
    rels = rels.replace(
        "</Relationships>",
        &format!(r#"<Relationship Id="rId9" Type="{RT}externalLink" Target="externalLinks/externalLink1.xml"/></Relationships>"#),
    );
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", wb),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("xl/externalLinks/externalLink1.xml", format!(r#"<externalLink xmlns="{NS_X}"/>"#)),
    ]);
    let fs = findings(&pkg);
    let links: Vec<_> = fs.iter().filter(|f| f.id == "external-link").collect();
    // One finding: the externalReference resolves to the same part, deduped.
    assert_eq!(links.len(), 1, "{fs:?}");
    assert!(links[0].detail.contains("externalLink1.xml"), "{links:?}");
}

#[test]
fn comment_authors_found() {
    let comments = format!(
        r#"<comments xmlns="{NS_X}"><authors><author>Quill Ramsey</author><author>Ona Vettel</author></authors><commentList><comment ref="B2" authorId="0"><text><t>Check this.</t></text></comment><comment ref="C3" authorId="1"><text><t>Agreed.</t></text></comment></commentList></comments>"#
    );
    let rels = workbook_rels(&[("rId1", "worksheets/sheet1.xml")]);
    let sheet_xml = format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#);
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", sheet_xml),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RT}comments" Target="../comments1.xml"/></Relationships>"#
            ),
        ),
        ("xl/comments1.xml", comments),
    ]);
    let fs = findings(&pkg);
    let authors: Vec<_> = fs.iter().filter(|f| f.id == "author-pii/comment").collect();
    assert_eq!(authors.len(), 2, "{fs:?}");
    assert!(authors.iter().any(|f| f.detail.contains("Quill Ramsey") && f.detail.contains("B2")));
    assert!(authors.iter().any(|f| f.detail.contains("Ona Vettel") && f.detail.contains("C3")));
}

#[test]
fn threaded_comment_authors_found() {
    let persons =
        format!(r#"<personList xmlns="{NS_X}"><person displayName="Ona Vettel" id="{{11111111-1111-1111-1111-111111111111}}"/></personList>"#);
    let threaded =
        r#"<ThreadedComments xmlns="http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments"><threadedComment ref="D4" personId="{11111111-1111-1111-1111-111111111111}"><text><t>Threaded note.</t></text></threadedComment></ThreadedComments>"#.to_string();
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("xl/persons/person.xml", persons),
        ("xl/threadedComments/threadedComment1.xml", threaded),
    ]);
    let fs = findings(&pkg);
    let authors: Vec<_> = fs.iter().filter(|f| f.id == "author-pii/threaded-comment").collect();
    assert_eq!(authors.len(), 1, "{fs:?}");
    assert!(authors[0].detail.contains("Ona Vettel") && authors[0].detail.contains("D4"), "{authors:?}");
}

#[test]
fn core_and_extended_properties_found() {
    let core = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Quill Ramsey</dc:creator><cp:lastModifiedBy>Ona Vettel</cp:lastModifiedBy></cp:coreProperties>"#;
    let app =
        r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Company>Acme Corp</Company></Properties>"#;
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("docProps/core.xml", core.to_string()),
        ("docProps/app.xml", app.to_string()),
    ]);
    let fs = findings(&pkg);
    let core_hits: Vec<_> = fs.iter().filter(|f| f.id == "author-pii/core-properties").collect();
    let ext_hits: Vec<_> = fs.iter().filter(|f| f.id == "author-pii/extended-properties").collect();
    assert_eq!(core_hits.len(), 2, "{fs:?}");
    assert!(core_hits.iter().any(|f| f.detail == "Quill Ramsey"));
    assert!(core_hits.iter().any(|f| f.detail == "Ona Vettel"));
    assert_eq!(ext_hits.len(), 1, "{fs:?}");
    assert_eq!(ext_hits[0].detail, "Acme Corp");
}

#[test]
fn pivot_cache_vba_and_embeddings_found() {
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("xl/pivotCache/pivotCacheDefinition1.xml", "<pivotCacheDefinition/>".to_string()),
        ("xl/vbaProject.bin", "fake".to_string()),
        ("xl/embeddings/oleObject1.bin", "fake".to_string()),
    ]);
    let fs = findings(&pkg);
    let kinds = ids(&fs);
    assert!(kinds.contains(&"pivot-cache"), "{kinds:?}");
    assert!(kinds.contains(&"active-content/vba"), "{kinds:?}");
    assert!(kinds.contains(&"active-content/ole"), "{kinds:?}");
}

#[test]
fn all_hidden_kinds_found_in_one_package() {
    // One hidden thing of each kind, in a single package: the proof that the
    // audit's finding classes are all live, not just individually tested.
    let sheets = format!("{}{}{}", sheet("Budget", "rId1", ""), sheet("Salaries", "rId2", "hidden"), sheet("Keys", "rId3", "veryHidden"));
    let wb = workbook(
        &sheets,
        r#"<definedNames><definedName name="Secret" hidden="1">Budget!$B$2</definedName></definedNames><externalReferences><externalReference r:id="rId9"/></externalReferences>"#,
    );
    let mut rels = workbook_rels(&[("rId1", "worksheets/sheet1.xml"), ("rId2", "worksheets/sheet2.xml"), ("rId3", "worksheets/sheet3.xml")]);
    rels = rels.replace(
        "</Relationships>",
        &format!(r#"<Relationship Id="rId9" Type="{RT}externalLink" Target="externalLinks/externalLink1.xml"/></Relationships>"#),
    );
    let sheet_xml = format!(
        r#"<worksheet xmlns="{NS_X}"><cols><col min="2" max="2" hidden="1"/></cols><sheetData><row r="3" hidden="1"><c r="A3"><v>2</v></c></row></sheetData></worksheet>"#
    );
    let comments = format!(
        r#"<comments xmlns="{NS_X}"><authors><author>Quill Ramsey</author></authors><commentList><comment ref="B2" authorId="0"><text><t>Check this.</t></text></comment></commentList></comments>"#
    );
    let persons =
        format!(r#"<personList xmlns="{NS_X}"><person displayName="Ona Vettel" id="{{11111111-1111-1111-1111-111111111111}}"/></personList>"#);
    let threaded =
        r#"<ThreadedComments xmlns="http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments"><threadedComment ref="D4" personId="{11111111-1111-1111-1111-111111111111}"><text><t>Threaded note.</t></text></threadedComment></ThreadedComments>"#.to_string();
    let core = r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:creator>Quill Ramsey</dc:creator></cp:coreProperties>"#;
    let app =
        r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Company>Acme Corp</Company></Properties>"#;
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", wb),
        ("xl/_rels/workbook.xml.rels", rels),
        ("xl/worksheets/sheet1.xml", sheet_xml),
        ("xl/worksheets/sheet2.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("xl/worksheets/sheet3.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RT}comments" Target="../comments1.xml"/></Relationships>"#
            ),
        ),
        ("xl/comments1.xml", comments),
        ("xl/persons/person.xml", persons),
        ("xl/threadedComments/threadedComment1.xml", threaded),
        ("docProps/core.xml", core.to_string()),
        ("docProps/app.xml", app.to_string()),
        ("xl/externalLinks/externalLink1.xml", format!(r#"<externalLink xmlns="{NS_X}"/>"#)),
        ("xl/pivotCache/pivotCacheDefinition1.xml", "<pivotCacheDefinition/>".to_string()),
        ("xl/vbaProject.bin", "fake".to_string()),
        ("xl/embeddings/oleObject1.bin", "fake".to_string()),
        ("stray.txt", "leftover".to_string()),
    ]);
    let fs = findings(&pkg);
    let kinds = ids(&fs);
    for expected in [
        "hidden/row",
        "hidden/column",
        "hidden/sheet",
        "hidden/defined-name",
        "external-link",
        "author-pii/comment",
        "author-pii/threaded-comment",
        "author-pii/core-properties",
        "author-pii/extended-properties",
        "pivot-cache",
        "active-content/vba",
        "active-content/ole",
        "package/orphan-part",
    ] {
        assert!(kinds.contains(&expected), "missing {expected} in {kinds:?}");
    }
}

#[test]
fn xml_deeper_than_importer_depth_is_an_error() {
    // Bounds match the xlsx importer (depth 256): nesting past it must fail
    // closed, never scan partially.
    let nested = "<g>".repeat(300) + &"</g>".repeat(300);
    let sheet_xml = format!(r#"<worksheet xmlns="{NS_X}"><sheetData>{nested}</sheetData></worksheet>"#);
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", sheet_xml),
    ]);
    assert!(matches!(audit_bytes(&pkg), Err(AuditError::Limit(_))), "{:?}", audit_bytes(&pkg).map(|_| ()));
}

#[test]
fn orphan_part_found() {
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("stray.txt", "leftover".to_string()),
    ]);
    let fs = findings(&pkg);
    let orphans: Vec<_> = fs.iter().filter(|f| f.id == "package/orphan-part").collect();
    assert_eq!(orphans.len(), 1, "{fs:?}");
    assert_eq!(orphans[0].location, "stray.txt");
}

#[test]
fn malformed_zip_is_an_error_not_a_panic() {
    let err = audit_bytes(b"this is not a zip").unwrap_err();
    assert!(matches!(err, AuditError::Zip(_)), "{err:?}");
}

#[test]
fn truncated_xml_is_an_error_not_a_silent_pass() {
    // Cut mid-tag: malformed XML must be an error, never a silent pass.
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", "<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><she".to_string()),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[])),
    ]);
    let err = audit_bytes(&pkg).unwrap_err();
    assert!(matches!(err, AuditError::Xml(_, _)), "{err:?}");
}

#[test]
fn missing_workbook_is_an_error() {
    let pkg = zip(&[("[Content_Types].xml", content_types()), ("_rels/.rels", root_rels())]);
    let err = audit_bytes(&pkg).unwrap_err();
    assert!(matches!(err, AuditError::Xml(_, _)), "{err:?}");
}

#[test]
fn findings_come_back_in_deterministic_order() {
    // Same content, different ZIP entry order, must produce identical reports.
    let parts = vec![
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData><row r="2" hidden="1"/></sheetData></worksheet>"#)),
        ("stray.txt", "x".to_string()),
    ];
    let a = findings(&zip(&parts));
    let mut reversed = parts.clone();
    reversed.reverse();
    let b = findings(&zip(&reversed));
    assert_eq!(a, b);
}

const NS_X_STRICT: &str = "http://purl.oclc.org/ooxml/spreadsheetml/main";
const NS_R_STRICT: &str = "http://purl.oclc.org/ooxml/officeDocument/relationships";

#[test]
fn strict_workbook_is_scanned_not_clean() {
    // A "Strict Open XML Spreadsheet" workbook must not pass as clean: the
    // Strict namespaces are understood, not skipped.
    let workbook = format!(
        r#"<workbook xmlns="{NS_X_STRICT}" xmlns:r="{NS_R_STRICT}"><sheets><sheet name="Secret" sheetId="1" r:id="rId1" state="veryHidden"/></sheets></workbook>"#
    );
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X_STRICT}"><sheetData/></worksheet>"#)),
    ]);
    let fs = findings(&pkg);
    assert!(fs.iter().any(|f| f.id == "hidden/sheet" && f.detail.contains("veryHidden")), "{fs:?}");
}

#[test]
fn unrecognized_workbook_namespace_is_an_error() {
    // Fail closed: an unknown workbook vocabulary must error, never scan
    // to a false clean result.
    let workbook = r#"<workbook xmlns="http://example.com/not-a-spreadsheet"><sheets/></workbook>"#.to_string();
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[])),
    ]);
    assert!(matches!(audit_bytes(&pkg), Err(AuditError::Xml(_, _))));
}

#[test]
fn entity_references_are_decoded() {
    // `&amp;` in an author name must come back as `&`, not vanish.
    let comments = format!(
        r#"<comments xmlns="{NS_X}"><authors><author>Smith &amp; Jones</author></authors><commentList><comment ref="B2" authorId="0"><text><t>Hi.</t></text></comment></commentList></comments>"#
    );
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RT}comments" Target="../comments1.xml"/></Relationships>"#
            ),
        ),
        ("xl/comments1.xml", comments),
    ]);
    let fs = findings(&pkg);
    let authors: Vec<_> = fs.iter().filter(|f| f.id == "author-pii/comment").collect();
    assert_eq!(authors.len(), 1, "{fs:?}");
    assert!(authors[0].detail.contains("Smith & Jones"), "{fs:?}");
}

#[test]
fn orphan_subtree_targets_stay_orphans() {
    // Relationships inside an unreachable subtree must not rescue their
    // targets: both the orphan part and what it points at are orphans.
    let orphan_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="related.bin"/></Relationships>"#;
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        ("xl/orphan.bin", "leftover".to_string()),
        ("xl/_rels/orphan.bin.rels", orphan_rels.to_string()),
        ("xl/related.bin", "rescued?".to_string()),
    ]);
    let fs = findings(&pkg);
    let orphans: Vec<_> = fs.iter().filter(|f| f.id == "package/orphan-part").map(|f| f.location.as_str()).collect();
    assert!(orphans.contains(&"xl/orphan.bin"), "{orphans:?}");
    assert!(orphans.contains(&"xl/related.bin"), "{orphans:?}");
}

#[test]
fn cased_worksheet_part_is_scanned() {
    // OPC part names are ASCII case-insensitive: a worksheet spelled
    // `xl/Worksheets/Sheet1.XML` must still be scanned, with its label.
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        (
            "xl/Worksheets/Sheet1.XML",
            format!(r#"<worksheet xmlns="{NS_X}"><sheetData><row r="2" hidden="1"><c r="A2"><v>1</v></c></row></sheetData></worksheet>"#),
        ),
    ]);
    let fs = findings(&pkg);
    let rows: Vec<_> = fs.iter().filter(|f| f.id == "hidden/row").collect();
    assert_eq!(rows.len(), 1, "{fs:?}");
    assert!(rows[0].location.contains("xl/Worksheets/Sheet1.XML"), "{fs:?}");
    assert!(rows[0].location.contains("(Budget)"), "{fs:?}");
    // The case-variant part is reachable, not an orphan.
    assert!(fs.iter().all(|f| f.id != "package/orphan-part"), "{fs:?}");
}

#[test]
fn cased_relationship_part_is_traversed() {
    // A case-variant relationship part (`_RELS/SHEET1.XML.RELS`) is still a
    // relationship part: its targets are discovered and reachable.
    let comments = format!(
        r#"<comments xmlns="{NS_X}"><authors><author>Wren Kessler</author></authors><commentList><comment ref="A1" authorId="0"><text><t>Note.</t></text></comment></commentList></comments>"#
    );
    let pkg = zip(&[
        ("[Content_Types].xml", content_types()),
        ("_rels/.rels", root_rels()),
        ("xl/workbook.xml", workbook(&sheet("Budget", "rId1", ""), "")),
        ("xl/_rels/workbook.xml.rels", workbook_rels(&[("rId1", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="{NS_X}"><sheetData/></worksheet>"#)),
        (
            "xl/worksheets/_rels/SHEET1.XML.RELS",
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RT}comments" Target="../comments1.xml"/></Relationships>"#
            ),
        ),
        ("xl/comments1.xml", comments),
    ]);
    let fs = findings(&pkg);
    let authors: Vec<_> = fs.iter().filter(|f| f.id == "author-pii/comment").collect();
    assert_eq!(authors.len(), 1, "{fs:?}");
    assert!(authors[0].detail.contains("Wren Kessler"), "{fs:?}");
    assert!(fs.iter().all(|f| f.id != "package/orphan-part"), "{fs:?}");
}
