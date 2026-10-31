//! Linked data type records (Stocks, Currencies) in a hand-written package, not an Excel file.

use gridcraft_core::{CellRef, Value};

use super::{CT, ROOT_RELS, make_zip};
use crate::{read_xlsx, write_xlsx};

fn at(value: &str) -> CellRef {
    CellRef::parse(value).unwrap()
}

const NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/richdata";

/// Two records (a currency pair and a stock with an extra field) and `=A1.Price` in B1.
fn package() -> Vec<u8> {
    let rich_rels = [
        ("rdRichValue", "rdrichvalue.xml", "http://schemas.microsoft.com/office/2017/06/relationships/rdRichValue"),
        ("rdRichValueStructure", "rdrichvaluestructure.xml", "http://schemas.microsoft.com/office/2017/06/relationships/rdRichValueStructure"),
    ]
    .iter()
    .enumerate()
    .map(|(i, (_, file, ty))| format!(r#"<Relationship Id="rIdR{i}" Type="{ty}" Target="richData/{file}"/>"#))
    .collect::<String>();
    let workbook = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let workbook_rels = format!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata" Target="metadata.xml"/>{rich_rels}</Relationships>"#
    );
    let sheet = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="e" vm="1"><v>#VALUE!</v></c><c r="B1" cm="1"><f t="array" aca="1" ref="B1" ca="1">_FV(A1,"Price")</f><v>1.0842</v></c></row><row r="2"><c r="A2" t="e" vm="2"><v>#VALUE!</v></c></row></sheetData></worksheet>"#;
    let metadata = format!(
        r#"<metadata xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:rv="{NS}"><metadataTypes count="2"><metadataType name="XLDAPR" cellMeta="1"/><metadataType name="XLRICHVALUE"/></metadataTypes><futureMetadata name="XLRICHVALUE" count="2"><bk><extLst><ext uri="{{3e2802c4-a4d2-4d8b-9148-e3be6c30e623}}"><rv:rvb i="1"/></ext></extLst></bk><bk><extLst><ext uri="{{3e2802c4-a4d2-4d8b-9148-e3be6c30e623}}"><rv:rvb i="3"/></ext></extLst></bk></futureMetadata><valueMetadata count="2"><bk><rc t="2" v="0"/></bk><bk><rc t="2" v="1"/></bk></valueMetadata></metadata>"#
    );
    let structures = format!(
        r#"<rvStructures xmlns="{NS}" count="3"><s t="_linkedentity"><k n="%cvi" t="r"/></s><s t="_linkedentitycore"><k n="_DisplayString" t="s"/><k n="Price" t="d"/><k n="_Icon" t="s"/><k n="_Format" t="spb"/></s><s t="_linkedentitycore"><k n="_DisplayString" t="s"/><k n="Price" t="d"/><k n="Market cap" t="d"/></s></rvStructures>"#
    );
    // Rich value 0 is the currency record and 1 its wrapper; 2 the stock record and 3 its wrapper.
    let values = format!(
        r#"<rvData xmlns="{NS}" count="4"><rv s="1"><v>EUR/USD</v><v>1.0842</v><v>Bank</v><v>0</v></rv><rv s="0"><v>0</v></rv><rv s="2"><v>Contoso &amp; Co</v><v>25.5</v><v>1.5e9</v></rv><rv s="0"><v>2</v></rv></rvData>"#
    );
    make_zip(&[
        ("[Content_Types].xml", CT.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ("xl/metadata.xml", metadata.as_bytes()),
        ("xl/richData/rdrichvalue.xml", values.as_bytes()),
        ("xl/richData/rdrichvaluestructure.xml", structures.as_bytes()),
    ])
}

fn check(wb: &gridcraft_model::Workbook) {
    let sheet = wb.sheet(0).unwrap();
    assert_eq!(sheet.value(at("A1")), Value::text("EUR/USD"));
    assert_eq!(sheet.value(at("A2")), Value::text("Contoso & Co"));
    let currency = sheet.linked_records.get(&at("A1")).unwrap();
    assert_eq!(currency.field("Price"), Some(Value::Number(1.0842)));
    // Only number, text and boolean fields are kept.
    assert_eq!(currency.fields.len(), 3);
    assert_eq!(sheet.linked_records.get(&at("A2")).unwrap().field("market cap"), Some(Value::Number(1.5e9)));
    let b1 = sheet.cells.get(at("B1")).unwrap();
    assert_eq!(b1.formula.as_ref().unwrap().text, "FIELDVALUE(A1,\"Price\")");
    assert_eq!(b1.value, Value::Number(1.0842));
}

#[test]
fn reads_linked_records_through_wrappers_and_keeps_the_field_formula() {
    let (wb, report) = read_xlsx(&package()).unwrap();
    // The hand-written package has no styles part; nothing else may be reported.
    assert!(report.warnings.iter().all(|w| w == "no styles part"), "{:?}", report.warnings);
    check(&wb);
}

#[test]
fn linked_records_survive_a_save_with_their_field_formula() {
    let (wb, _) = read_xlsx(&package()).unwrap();
    let saved = write_xlsx(&wb).unwrap();
    let (again, report) = read_xlsx(&saved).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    check(&again);
    let text = |name: &str| {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&saved)).unwrap();
        let mut out = String::new();
        std::io::Read::read_to_string(&mut zip.by_name(name).unwrap(), &mut out).unwrap();
        out
    };
    assert!(text("xl/worksheets/sheet1.xml").contains("_FV(A1,&quot;Price&quot;)") || text("xl/worksheets/sheet1.xml").contains("_FV(A1,\"Price\")"));
    assert!(text("xl/richData/rdrichvaluestructure.xml").contains("_linkedentity"));
}

#[test]
fn a_damaged_linked_reference_keeps_the_display_fallback_without_a_record() {
    let original = package();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&original)).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let mut bytes = vec![];
        std::io::Read::read_to_end(&mut file, &mut bytes).unwrap();
        if file.name() == "xl/richData/rdrichvalue.xml" {
            // The second wrapper points past the last rich value.
            bytes = String::from_utf8(bytes).unwrap().replace("<rv s=\"0\"><v>2</v></rv>", "<rv s=\"0\"><v>99</v></rv>").into_bytes();
        }
        parts.push((file.name().to_string(), bytes));
    }
    let broken = make_zip(&parts.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>());
    let (wb, _) = read_xlsx(&broken).unwrap();
    let sheet = wb.sheet(0).unwrap();
    assert!(sheet.linked_records.contains_key(&at("A1")));
    assert!(!sheet.linked_records.contains_key(&at("A2")));
}
