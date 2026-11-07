//! The notices of a workbook with Python cells in its custom document properties, and what is already
//! in that part.

use std::sync::Arc;

use gridcraft_core::Value;
use gridcraft_model::{PyCode, Sheet};

use super::python_pictures::{at, book, part_names, python_cell, repack, text, with_text_changed};
use crate::custom_props::{NOTICE, NOTICE_NAME, WARNING_NAME, WARNING_VALUE};
use crate::{read_xlsx, write_xlsx};

const NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/custom-properties";
const NS_VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
const CONTENT_TYPE: &str = "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const FMTID: &str = "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}";

fn python_workbook() -> Vec<u8> {
    let mut sheet = Sheet::new("Data");
    python_cell(&mut sheet, "A1", PyCode::new("1 + 1"), Value::Number(2.0));
    write_xlsx(&book(vec![sheet])).unwrap()
}

fn plain_workbook() -> Vec<u8> {
    write_xlsx(&book(vec![Sheet::new("Data")])).unwrap()
}

/// A package with this custom properties part, its relationship and its content type.
fn with_part(bytes: &[u8], part: &[u8]) -> Vec<u8> {
    if text(bytes, "_rels/.rels").contains(REL) {
        return repack(bytes, &[("docProps/custom.xml", part)]);
    }
    let bytes = with_text_changed(
        bytes,
        "_rels/.rels",
        "</Relationships>",
        &format!(r#"<Relationship Id="rId99" Type="{REL}" Target="docProps/custom.xml"/></Relationships>"#),
    );
    let bytes = with_text_changed(
        &bytes,
        "[Content_Types].xml",
        "</Types>",
        &format!(r#"<Override PartName="/docProps/custom.xml" ContentType="{CONTENT_TYPE}"/></Types>"#),
    );
    repack(&bytes, &[("docProps/custom.xml", part)])
}

/// (name, pid, type, value) of each property of the package's part, in order.
fn properties(bytes: &[u8]) -> Vec<(String, u32, String, String)> {
    let part = crate::xml::parse(text(bytes, "docProps/custom.xml").as_bytes()).unwrap();
    assert_eq!(part.name, "Properties");
    part.kids("property")
        .map(|p| {
            assert_eq!(p.attr("fmtid"), Some(FMTID));
            let value = p.children.first().unwrap();
            (p.attr("name").unwrap().to_string(), p.attr_u32("pid").unwrap(), value.name.clone(), value.text.clone())
        })
        .collect()
}

fn names(bytes: &[u8]) -> Vec<String> {
    properties(bytes).into_iter().map(|p| p.0).collect()
}

fn pids(bytes: &[u8]) -> Vec<u32> {
    properties(bytes).into_iter().map(|p| p.1).collect()
}

/// That the package has one custom properties part, one relationship to it and one content type.
fn assert_one_part(bytes: &[u8]) {
    let rels = text(bytes, "_rels/.rels");
    assert_eq!(rels.matches(REL).count(), 1, "{rels}");
    let types = text(bytes, "[Content_Types].xml");
    assert_eq!(types.matches(CONTENT_TYPE).count(), 1, "{types}");
    assert_eq!(types.matches("PartName=\"/docProps/custom.xml\"").count(), 1, "{types}");
    assert_eq!(part_names(bytes).iter().filter(|n| *n == "docProps/custom.xml").count(), 1);
}

fn assert_no_part(bytes: &[u8]) {
    assert!(!text(bytes, "_rels/.rels").contains(REL));
    assert!(!text(bytes, "[Content_Types].xml").contains(CONTENT_TYPE));
    assert!(!part_names(bytes).iter().any(|n| n == "docProps/custom.xml"));
}

/// The file saved again, as it was read.
fn saved_again(bytes: &[u8]) -> Vec<u8> {
    write_xlsx(&read_xlsx(bytes).unwrap().0).unwrap()
}

/// The same saved again with no Python cell left.
fn saved_without_python(bytes: &[u8]) -> Vec<u8> {
    let (mut wb, _) = read_xlsx(bytes).unwrap();
    Arc::make_mut(&mut wb.sheets[0]).cells.get_mut(at("A1")).unwrap().py = None;
    write_xlsx(&wb).unwrap()
}

#[test]
fn the_notices_are_within_excels_limit() {
    for text in [NOTICE_NAME, NOTICE, WARNING_NAME, WARNING_VALUE] {
        assert!(text.chars().count() <= 255, "{text}");
    }
}

#[test]
fn a_workbook_with_python_cells_has_the_notices_and_one_with_none_has_none() {
    let bytes = python_workbook();
    assert_one_part(&bytes);
    assert_eq!(
        properties(&bytes),
        [
            (NOTICE_NAME.to_string(), 2, "lpwstr".to_string(), NOTICE.to_string()),
            (WARNING_NAME.to_string(), 3, "bstr".to_string(), WARNING_VALUE.to_string())
        ]
    );
    let part = text(&bytes, "docProps/custom.xml");
    assert!(part.contains(&format!(r#"<Properties xmlns="{NS}" xmlns:vt="{NS_VT}">"#)), "{part}");
    assert!(part.contains("gridcraft:python:&lt;n&gt;:"), "the text holds what must be escaped: {part}");
    assert!(!part.contains("<n>"), "{part}");
    assert_no_part(&plain_workbook());
}

#[test]
fn they_are_written_again_and_not_taken_for_a_persons_own() {
    let bytes = python_workbook();
    let (wb, _) = read_xlsx(&bytes).unwrap();
    assert!(wb.preserved.as_deref().is_none_or(|p| p.parts.iter().all(|q| q.name != "docProps/custom.xml")), "not kept: {:?}", wb.preserved);
    let again = saved_again(&bytes);
    assert_one_part(&again);
    assert_eq!(properties(&again), properties(&bytes));
    assert_eq!(text(&again, "docProps/custom.xml"), text(&bytes, "docProps/custom.xml"));
}

#[test]
fn a_workbook_with_no_python_cells_left_loses_them() {
    let gone = saved_without_python(&python_workbook());
    assert_no_part(&gone);
    // What openpyxl leaves behind, in a file that has lost its code.
    let part = format!(
        r#"<Properties xmlns="{NS}"><property name="{NOTICE_NAME}" pid="2" fmtid="{FMTID}"><vt:lpwstr xmlns:vt="{NS_VT}">{}</vt:lpwstr></property></Properties>"#,
        crate::xml::esc(NOTICE)
    );
    assert_no_part(&saved_again(&with_part(&plain_workbook(), part.as_bytes())));
}

const MINE: &str =
    r#"<property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Client &amp; year"><vt:lpwstr>A &lt;b&gt;</vt:lpwstr></property>"#;
const THEIRS: &str = r#"<property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="5" name="Count"><vt:i4>7</vt:i4></property>"#;

fn persons_part() -> String {
    format!(r#"<?xml version="1.0"?><Properties xmlns="{NS}" xmlns:vt="{NS_VT}">{MINE}{THEIRS}</Properties>"#)
}

#[test]
fn the_notices_join_the_properties_a_file_already_has() {
    let bytes = saved_again(&with_part(&python_workbook(), persons_part().as_bytes()));
    assert_one_part(&bytes);
    assert_eq!(names(&bytes), ["Client & year", "Count", NOTICE_NAME, WARNING_NAME]);
    // They take the numbers that are free.
    assert_eq!(pids(&bytes), [2, 5, 3, 4]);
    let part = text(&bytes, "docProps/custom.xml");
    assert!(part.contains(&format!("{MINE}{THEIRS}")), "as they came: {part}");
    // Saved again, the same.
    let again = saved_again(&bytes);
    assert_one_part(&again);
    assert_eq!(text(&again, "docProps/custom.xml"), part);
}

#[test]
fn a_file_that_has_the_notices_keeps_one_of_each_wherever_they_were() {
    let older = format!(
        r#"<property fmtid="{FMTID}" pid="7" name="GridCraft: this workbook has Python cells. An older wording."><vt:bstr>x</vt:bstr></property>"#
    );
    let notice = format!(r#"<property fmtid="{FMTID}" pid="2" name="{NOTICE_NAME}"><vt:lpwstr>old text</vt:lpwstr></property>"#);
    let part = format!(r#"<Properties xmlns="{NS}" xmlns:vt="{NS_VT}">{older}{THEIRS}{notice}</Properties>"#);
    let bytes = saved_again(&with_part(&python_workbook(), part.as_bytes()));
    assert_one_part(&bytes);
    assert_eq!(names(&bytes), ["Count", NOTICE_NAME, WARNING_NAME]);
    assert_eq!(pids(&bytes), [5, 2, 3]);
    assert_eq!(properties(&bytes)[1].3, NOTICE, "from the workbook it has");
}

#[test]
fn the_persons_properties_stay_when_the_notices_go() {
    let bytes = saved_again(&with_part(&python_workbook(), persons_part().as_bytes()));
    let gone = saved_without_python(&bytes);
    assert_one_part(&gone);
    assert_eq!(names(&gone), ["Client & year", "Count"]);
    assert_eq!(pids(&gone), [2, 5]);
    // Without notices a part isn't touched at all.
    let plain = with_part(&plain_workbook(), persons_part().as_bytes());
    assert_eq!(text(&saved_again(&plain), "docProps/custom.xml"), persons_part());
}

#[test]
fn an_element_beside_the_notices_keeps_the_part_when_they_go() {
    use crate::custom_props::{Stripped, without_notices};
    let notice = format!(r#"<property fmtid="{{1}}" pid="2" name="{NOTICE_NAME}"><vt:lpwstr>x</vt:lpwstr></property>"#);
    match without_notices(format!("<Properties>{notice}<extra/></Properties>").as_bytes()) {
        Stripped::Part(part) => {
            let part = String::from_utf8(part).unwrap();
            assert!(part.contains("<extra/>") && !part.contains(NOTICE_NAME), "{part}");
        }
        _ => panic!("the part stays"),
    }
    // A comment or a processing instruction is something the file holds too.
    for kept in ["<!-- a note -->", "<?app x?>"] {
        match without_notices(format!("<Properties>{notice}{kept}</Properties>").as_bytes()) {
            Stripped::Part(part) => assert!(String::from_utf8(part).unwrap().contains(kept), "{kept}"),
            _ => panic!("{kept} keeps the part"),
        }
    }
    assert!(matches!(without_notices(format!("<Properties>\n{notice}\n</Properties>").as_bytes()), Stripped::Nothing));
}

#[test]
fn any_shape_of_root_takes_the_notices() {
    // Self-closed, as an empty list can be.
    let empty = format!(r#"<Properties xmlns="{NS}"/>"#);
    let bytes = saved_again(&with_part(&python_workbook(), empty.as_bytes()));
    assert_eq!(names(&bytes), [NOTICE_NAME, WARNING_NAME]);
    assert_one_part(&bytes);

    // No declaration of the types, as openpyxl writes it, and prefixes on everything.
    let bare = format!(
        r#"<Properties xmlns="{NS}"><property name="Count" pid="2" fmtid="{FMTID}"><vt:i4 xmlns:vt="{NS_VT}">7</vt:i4></property></Properties>"#
    );
    let prefixed = format!(
        r#"<o:Properties xmlns:o="{NS}" xmlns:t="{NS_VT}"><o:property name="Count" pid="2" fmtid="{FMTID}"><t:i4>7</t:i4></o:property></o:Properties>"#
    );
    let quoted = format!(
        r#"<Properties xmlns='{NS}' xmlns:vt='{NS_VT}'><property name='Count' pid='2' fmtid='{FMTID}'><vt:i4>7</vt:i4></property></Properties>"#
    );
    for part in [bare, prefixed, quoted] {
        let bytes = saved_again(&with_part(&python_workbook(), part.as_bytes()));
        assert_eq!(names(&bytes), ["Count", NOTICE_NAME, WARNING_NAME], "{part}");
        let written = text(&bytes, "docProps/custom.xml");
        assert!(written.contains(&format!(r#"<property xmlns="{NS}" fmtid="#)), "they declare what they need: {written}");
        assert!(written.contains(&format!(r#"<vt:bstr xmlns:vt="{NS_VT}">"#)), "{written}");
    }

    // UTF-16, whose declaration a rewrite doesn't keep.
    let utf8 = format!(r#"<?xml version="1.0" encoding="UTF-16"?><Properties xmlns="{NS}" xmlns:vt="{NS_VT}">{THEIRS}</Properties>"#);
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend(utf8.encode_utf16().flat_map(u16::to_le_bytes));
    let bytes = saved_again(&with_part(&python_workbook(), &utf16));
    assert_eq!(names(&bytes), ["Count", NOTICE_NAME, WARNING_NAME]);
    assert!(text(&bytes, "docProps/custom.xml").starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\""));
}

#[test]
fn a_damaged_part_stays_as_it_came_and_a_save_still_works() {
    for part in [
        "not xml".to_string(),
        format!(r#"<Properties xmlns="{NS}"><property name="a">"#),
        format!(r#"<Properties xmlns="{NS}"><property name="a"></wrong></Properties>"#),
        "<other/>".to_string(),
        String::new(),
    ] {
        let bytes = saved_again(&with_part(&python_workbook(), part.as_bytes()));
        assert_eq!(text(&bytes, "docProps/custom.xml"), part);
        assert_one_part(&bytes);
    }
    // Bytes that aren't text at all.
    let bytes = saved_again(&with_part(&python_workbook(), &[0xC0, 0xFF, 0x00, 0x80]));
    assert_one_part(&bytes);
}

#[test]
fn hostile_properties_are_numbered_around() {
    // Numbers the notices might take, one too large to read, and one that isn't a number.
    let part = format!(
        r#"<Properties xmlns="{NS}" xmlns:vt="{NS_VT}"><property pid="2" name="a"/><property pid="3" name="b"/><property pid="99999999999999999999" name="c"/><property pid="x" name="d"/><property name="e"/><comment/></Properties>"#
    );
    let bytes = saved_again(&with_part(&python_workbook(), part.as_bytes()));
    let written = text(&bytes, "docProps/custom.xml");
    assert!(written.contains(&format!(r#"pid="4" name="{NOTICE_NAME}""#)), "{written}");
    assert!(written.contains(&format!(r#"pid="5" name="{WARNING_NAME}""#)), "{written}");
    assert!(written.contains("<comment/>"), "what isn't a property stays too: {written}");
    // A name that only looks like ours is someone's.
    let like = format!(
        r#"<Properties xmlns="{NS}" xmlns:vt="{NS_VT}"><property pid="2" name="GridCraft.Python.Notice2"/><property pid="3" name="GridCraft: mine"/></Properties>"#
    );
    let bytes = saved_without_python(&saved_again(&with_part(&python_workbook(), like.as_bytes())));
    assert_eq!(text(&bytes, "docProps/custom.xml").matches("<property ").count(), 2);
}

#[test]
fn a_kept_part_with_that_name_for_another_reason_keeps_it() {
    // Reached as custom XML, it is kept under its name, and the notices need another: two parts of one
    // name would fail the save.
    let bytes =
        with_text_changed(&python_workbook(), "_rels/.rels", &format!(r#"<Relationship Id="rId4" Type="{REL}" Target="/docProps/custom.xml"/>"#), "");
    let bytes = with_text_changed(
        &bytes,
        "xl/_rels/workbook.xml.rels",
        "</Relationships>",
        r#"<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml" Target="../docProps/custom.xml"/></Relationships>"#,
    );
    let bytes = with_text_changed(&bytes, "[Content_Types].xml", CONTENT_TYPE, "application/xml");
    let bytes = saved_again(&repack(&bytes, &[("docProps/custom.xml", b"<a/>")]));
    assert_eq!(text(&bytes, "docProps/custom.xml"), "<a/>");
    let rels = text(&bytes, "_rels/.rels");
    assert!(rels.contains(REL), "{rels}");
    assert_eq!(part_names(&bytes).iter().filter(|n| n.starts_with("docProps/custom")).count(), 2);
}

#[test]
fn two_relationships_to_the_part_are_written_as_one() {
    let bytes = with_text_changed(
        &python_workbook(),
        "_rels/.rels",
        "</Relationships>",
        &format!(r#"<Relationship Id="rId99" Type="{REL}" Target="/docProps/custom.xml"/></Relationships>"#),
    );
    let bytes = saved_again(&repack(&bytes, &[("docProps/custom.xml", persons_part().as_bytes())]));
    assert_one_part(&bytes);
    assert_eq!(names(&bytes), ["Client & year", "Count", NOTICE_NAME, WARNING_NAME]);
}
