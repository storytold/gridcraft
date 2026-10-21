use gridcraft_core::{CellRef, Value};
use gridcraft_model::Workbook;

use crate::{IoError, read_xml_table};

fn v(wb: &Workbook, r: u32, c: u32) -> Value {
    wb.sheet(0).unwrap().value(CellRef::new(r, c))
}

fn header(wb: &Workbook) -> Vec<String> {
    let sh = wb.sheet(0).unwrap();
    let ncols = sh.tables[0].columns.len() as u32;
    (0..ncols)
        .map(|c| match sh.value(CellRef::new(0, c)) {
            Value::Text(t) => t.to_string(),
            other => format!("{other:?}"),
        })
        .collect()
}

/// An invoice-shaped document written for this test: namespaced, pretty-printed, with two
/// repeated `det` items and fields that share a name under different parents.
const INVOICE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<nfeProc xmlns="http://example.test/nfe" versao="4.00">
  <NFe xmlns="http://example.test/nfe">
    <infNFe versao="4.00" Id="NFe123">
      <ide><cUF>26</cUF><nNF>1975</nNF><dhEmi>2026-09-30T14:18:05-03:00</dhEmi></ide>
      <emit><CNPJ>03987707000121</CNPJ><xNome>Seller</xNome></emit>
      <dest><xNome>Buyer &amp; Co</xNome></dest>
      <det nItem="1"><prod><cProd>A1</cProd><vProd>170.4300000000</vProd></prod></det>
      <det nItem="2"><prod><cProd>B2</cProd><vProd>20</vProd></prod></det>
      <chave>26260903987707000121550050000019751506435810</chave>
    </infNFe>
  </NFe>
</nfeProc>"#;

#[test]
fn repeated_element_makes_one_row_per_occurrence_with_the_parent_values_repeated() {
    let (wb, report) = read_xml_table(INVOICE.as_bytes()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let sh = wb.sheet(0).unwrap();
    // Header + two data rows.
    assert_eq!(sh.tables.len(), 1);
    let t = &sh.tables[0];
    assert_eq!((t.range.start.row, t.range.end.row), (0, 2));
    assert!(t.header_row && t.filter_button && t.banded_rows);

    let h = header(&wb);
    let col = |name: &str| h.iter().position(|n| n == name).unwrap_or_else(|| panic!("no column {name} in {h:?}")) as u32;
    // The parent's fields repeat on both rows; the item fields differ.
    assert_eq!(v(&wb, 1, col("cUF")), Value::Number(26.0));
    assert_eq!(v(&wb, 2, col("cUF")), Value::Number(26.0));
    assert_eq!(v(&wb, 1, col("cProd")), Value::text("A1"));
    assert_eq!(v(&wb, 2, col("cProd")), Value::text("B2"));
    assert_eq!(v(&wb, 1, col("nItem")), Value::Number(1.0));
    assert_eq!(v(&wb, 2, col("nItem")), Value::Number(2.0));
}

#[test]
fn headers_are_unique_in_document_order_and_namespaces_dropped() {
    let (wb, _) = read_xml_table(INVOICE.as_bytes()).unwrap();
    let h = header(&wb);
    // Attributes and elements in the order the document reaches them; `versao` and `xNome`
    // appear twice under different parents and get numbered, as Excel does.
    assert_eq!(&h[..3], ["versao", "versao2", "Id"]);
    assert!(h.contains(&"xNome".to_string()) && h.contains(&"xNome2".to_string()), "{h:?}");
    assert!(h.iter().all(|n| !n.contains(':') && !n.contains("xmlns")), "{h:?}");
    let mut sorted: Vec<_> = h.iter().map(|n| n.to_ascii_lowercase()).collect();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), h.len(), "duplicate headers: {h:?}");
}

#[test]
fn numbers_become_numbers_but_codes_and_keys_stay_text() {
    let (wb, _) = read_xml_table(INVOICE.as_bytes()).unwrap();
    let h = header(&wb);
    let col = |name: &str| h.iter().position(|n| n == name).unwrap() as u32;
    // A plain decimal is a number, trailing zeros and all.
    assert_eq!(v(&wb, 1, col("vProd")), Value::Number(170.43));
    assert_eq!(v(&wb, 2, col("vProd")), Value::Number(20.0));
    // A leading zero (CNPJ) and 44 digits (access key) would be corrupted by a number.
    assert_eq!(v(&wb, 1, col("CNPJ")), Value::text("03987707000121"));
    assert_eq!(v(&wb, 1, col("chave")), Value::text("26260903987707000121550050000019751506435810"));
    // Dates stay as written.
    assert_eq!(v(&wb, 1, col("dhEmi")), Value::text("2026-09-30T14:18:05-03:00"));
    // Entities are decoded.
    assert_eq!(v(&wb, 1, col("xNome2")), Value::text("Buyer & Co"));
}

#[test]
fn independent_repeated_elements_combine() {
    let xml = "<r><a>1</a><a>2</a><b>x</b><b>y</b><b>z</b></r>";
    let (wb, _) = read_xml_table(xml.as_bytes()).unwrap();
    let sh = wb.sheet(0).unwrap();
    assert_eq!(header(&wb), ["a", "b"]);
    // 2 x 3 pairings.
    assert_eq!(sh.tables[0].range.end.row, 6);
    assert_eq!(v(&wb, 1, 0), Value::Number(1.0));
    assert_eq!(v(&wb, 1, 1), Value::text("x"));
    assert_eq!(v(&wb, 6, 0), Value::Number(2.0));
    assert_eq!(v(&wb, 6, 1), Value::text("z"));
}

#[test]
fn flat_records_are_one_row_each() {
    let xml = "<people><p id=\"1\"><name>Ana</name></p><p id=\"2\"><name>Bia</name></p></people>";
    let (wb, _) = read_xml_table(xml.as_bytes()).unwrap();
    assert_eq!(header(&wb), ["id", "name"]);
    assert_eq!(wb.sheet(0).unwrap().tables[0].range.end.row, 2);
    assert_eq!(v(&wb, 2, 1), Value::text("Bia"));
}

#[test]
fn single_record_document_is_one_row() {
    let (wb, _) = read_xml_table(b"<a><b>1</b><c>t</c></a>").unwrap();
    assert_eq!(header(&wb), ["b", "c"]);
    assert_eq!(wb.sheet(0).unwrap().tables[0].range.end.row, 1);
}

#[test]
fn utf16_and_latin1_documents_are_read() {
    let text = "<a><n>Jos\u{e9}</n></a>";
    let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
    let (wb, _) = read_xml_table(&utf16).unwrap();
    assert_eq!(v(&wb, 1, 0), Value::text("Jos\u{e9}"));
}

#[test]
fn a_document_without_values_is_refused() {
    // An element with nothing in it still names a column; a file with no element at all does not.
    assert!(matches!(read_xml_table(b"not xml at all"), Err(IoError::Xml(_))));
    assert!(matches!(read_xml_table(b""), Err(IoError::Xml(_))));
}

#[test]
fn hostile_nesting_is_an_error_not_a_crash() {
    let deep = format!("{}x{}", "<a>".repeat(5000), "</a>".repeat(5000));
    assert!(read_xml_table(deep.as_bytes()).is_err());
}

#[test]
fn a_huge_cross_product_is_cut_with_a_warning() {
    // 1500 x 1500 pairings would be 2.25 million rows: past a sheet's rows.
    let mut xml = String::from("<r>");
    for i in 0..1500 {
        xml.push_str(&format!("<a>{i}</a>"));
    }
    for i in 0..1500 {
        xml.push_str(&format!("<b>{i}</b>"));
    }
    xml.push_str("</r>");
    let (wb, report) = read_xml_table(xml.as_bytes()).unwrap();
    assert!(!report.warnings.is_empty());
    assert!(wb.sheet(0).unwrap().tables[0].range.end.row < gridcraft_core::MAX_ROWS);
}

#[test]
fn a_long_list_of_nested_records_is_not_cut() {
    // 300k records, 7 values each, nested three levels deep: 2.1 million cells, well inside the
    // budget. Counting each level's rows again used to cut this down to a single row.
    let mut xml = String::from("<lote>");
    for i in 0..300_000 {
        xml.push_str(&format!(
            "<nf id=\"{i}\"><ide><n>{i}</n><d>x</d></ide><emit><a>V</a><b>W</b></emit><det><prod><c>{i}</c><q>1</q></prod></det></nf>"
        ));
    }
    xml.push_str("</lote>");
    let (wb, report) = read_xml_table(xml.as_bytes()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(wb.sheet(0).unwrap().tables[0].range.end.row, 300_000);
}

#[test]
fn many_occurrences_with_many_rows_each_are_cut_with_a_warning() {
    // 40 <g> each holding 1000 x 1000 pairings: 40 million rows if nothing bounded the group.
    let inner: String = format!("<g>{}{}</g>", "<a>1</a>".repeat(1000), "<b>2</b>".repeat(1000));
    let xml = format!("<r>{}</r>", inner.repeat(40));
    let (wb, report) = read_xml_table(xml.as_bytes()).unwrap();
    assert!(!report.warnings.is_empty());
    assert!(wb.sheet(0).unwrap().tables[0].range.end.row < gridcraft_core::MAX_ROWS);
}
