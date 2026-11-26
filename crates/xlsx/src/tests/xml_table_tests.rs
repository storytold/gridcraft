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
      <emit><CNPJ>01234567000195</CNPJ><xNome>Seller</xNome></emit>
      <dest><xNome>Buyer &amp; Co</xNome></dest>
      <det nItem="1"><prod><cProd>A1</cProd><vProd>170.4300000000</vProd></prod></det>
      <det nItem="2"><prod><cProd>B2</cProd><vProd>20</vProd></prod></det>
      <chave>26260901234567000195550050000019751506435810</chave>
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
    assert_eq!(v(&wb, 1, col("CNPJ")), Value::text("01234567000195"));
    assert_eq!(v(&wb, 1, col("chave")), Value::text("26260901234567000195550050000019751506435810"));
    // Dates stay as written.
    assert_eq!(v(&wb, 1, col("dhEmi")), Value::text("2026-09-30T14:18:05-03:00"));
    // Entities are decoded.
    assert_eq!(v(&wb, 1, col("xNome2")), Value::text("Buyer & Co"));
}

#[test]
fn independent_repeated_elements_stack_one_after_the_other() {
    // As in Excel: a's rows, then b's rows, with blanks where the other group has its values.
    let xml = "<r><a>1</a><a>2</a><b>x</b><b>y</b><b>z</b></r>";
    let (wb, _) = read_xml_table(xml.as_bytes()).unwrap();
    let sh = wb.sheet(0).unwrap();
    assert_eq!(header(&wb), ["a", "b"]);
    assert_eq!(sh.tables[0].range.end.row, 5);
    assert_eq!((v(&wb, 1, 0), v(&wb, 1, 1)), (Value::Number(1.0), Value::Empty));
    assert_eq!((v(&wb, 2, 0), v(&wb, 2, 1)), (Value::Number(2.0), Value::Empty));
    assert_eq!((v(&wb, 3, 0), v(&wb, 3, 1)), (Value::Empty, Value::text("x")));
    assert_eq!((v(&wb, 5, 0), v(&wb, 5, 1)), (Value::Empty, Value::text("z")));
}

#[test]
fn single_values_go_on_every_row_of_the_stacked_groups() {
    let xml = "<r><id>7</id><a>1</a><a>2</a><b>x</b><b>y</b></r>";
    let (wb, _) = read_xml_table(xml.as_bytes()).unwrap();
    assert_eq!(header(&wb), ["id", "a", "b"]);
    assert_eq!(wb.sheet(0).unwrap().tables[0].range.end.row, 4);
    for r in 1..=4 {
        assert_eq!(v(&wb, r, 0), Value::Number(7.0), "row {r}");
    }
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
fn a_file_that_is_not_xml_is_refused() {
    // Not well formed, or not XML at all: refused.
    assert!(matches!(read_xml_table(b"not xml at all"), Err(IoError::Xml(_))));
    assert!(matches!(read_xml_table(b""), Err(IoError::Xml(_))));
}

#[test]
fn hostile_nesting_is_an_error_not_a_crash() {
    let deep = format!("{}x{}", "<a>".repeat(5000), "</a>".repeat(5000));
    assert!(read_xml_table(deep.as_bytes()).is_err());
}

#[test]
fn a_list_longer_than_a_sheet_is_cut_with_a_warning() {
    let xml = format!("<r>{}</r>", "<a>1</a>".repeat(gridcraft_core::MAX_ROWS as usize + 10));
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
fn each_record_stacks_its_own_repeated_children() {
    let rec = |id: u32| format!("<o><id>{id}</id><a>1</a><a>2</a><b>x</b><b>y</b></o>");
    let xml = format!("<r>{}{}</r>", rec(1), rec(2));
    let (wb, _) = read_xml_table(xml.as_bytes()).unwrap();
    // Two rows of a and two of b for each of the two records.
    assert_eq!(wb.sheet(0).unwrap().tables[0].range.end.row, 8);
    assert_eq!(
        (v(&wb, 1, 0), v(&wb, 4, 0), v(&wb, 5, 0), v(&wb, 8, 0)),
        (Value::Number(1.0), Value::Number(1.0), Value::Number(2.0), Value::Number(2.0))
    );
}

#[test]
fn a_document_with_no_values_opens_as_a_blank_sheet() {
    for xml in ["<root/>", "<root></root>", "<a><b/><c></c></a>"] {
        let (wb, report) = read_xml_table(xml.as_bytes()).unwrap();
        let sh = wb.sheet(0).unwrap();
        assert!(sh.tables.is_empty() && sh.used_range().is_none(), "{xml}");
        assert!(report.warnings.is_empty(), "{xml}");
    }
}

#[test]
fn a_column_is_numeric_only_when_all_of_its_values_are_numbers() {
    let xml = "<r><i><n>2.50</n><m>2.50</m><k>1e3</k></i><i><n>3</n><m>x</m><k>-4</k></i><i><n></n><m>1</m><k>5%</k></i></r>";
    let (wb, _) = read_xml_table(xml.as_bytes()).unwrap();
    // n has an empty element (text, every cell); m has a word; k has 5% and 1,5-like text.
    assert_eq!(v(&wb, 1, 0), Value::text("2.50"));
    assert_eq!(v(&wb, 2, 0), Value::text("3"));
    assert_eq!(v(&wb, 1, 1), Value::text("2.50"));
    assert_eq!(v(&wb, 1, 2), Value::text("1e3"));
    let (wb, _) = read_xml_table(b"<r><i><k>1e3</k><p>1,5</p></i><i><k>-4</k><p>.5</p></i></r>").unwrap();
    assert_eq!((v(&wb, 1, 0), v(&wb, 2, 0)), (Value::Number(1000.0), Value::Number(-4.0)));
    assert_eq!((v(&wb, 1, 1), v(&wb, 2, 1)), (Value::text("1,5"), Value::text(".5")));
}

#[test]
fn text_columns_are_formatted_as_text() {
    let (wb, _) = read_xml_table(b"<r><i><c>007</c><n>1</n></i><i><c>008</c><n>2</n></i></r>").unwrap();
    let sh = wb.sheet(0).unwrap();
    let fmt = |r: u32, c: u32| wb.styles.get(sh.style_id(CellRef::new(r, c))).num_fmt.as_str().to_string();
    assert_eq!(fmt(1, 0), "@");
    assert_eq!(fmt(1, 1), "General");
    assert_eq!(v(&wb, 1, 0), Value::text("007"));
}

#[test]
fn logical_and_date_columns_are_typed() {
    let xml = "<r><i><b>true</b><d>2024-03-05</d><t>2024-03-05T10:30:00</t><z>2024-03-05T10:30:00-03:00</z></i><i><b>false</b><d>2024-03-06</d><t>2024-03-05T12:00:00</t><z>2024-03-06</z></i></r>";
    let (wb, _) = read_xml_table(xml.as_bytes()).unwrap();
    let sh = wb.sheet(0).unwrap();
    let fmt = |r: u32, c: u32| wb.styles.get(sh.style_id(CellRef::new(r, c))).num_fmt.as_str().to_string();
    assert_eq!((v(&wb, 1, 0), v(&wb, 2, 0)), (Value::Bool(true), Value::Bool(false)));
    // 2024-03-05 is serial 45356; the time of day is a fraction of a day.
    assert_eq!((v(&wb, 1, 1), v(&wb, 2, 1)), (Value::Number(45356.0), Value::Number(45357.0)));
    assert_eq!(fmt(1, 1), "m/d/yyyy");
    assert_eq!(v(&wb, 1, 2), Value::Number(45356.0 + 10.5 / 24.0));
    assert_eq!(fmt(1, 2), "m/d/yyyy h:mm");
    // A time zone, or a column mixing forms Excel doesn't read, stays text.
    assert_eq!(v(&wb, 1, 3), Value::text("2024-03-05T10:30:00-03:00"));
    // An impossible day is not a date.
    let (wb, _) = read_xml_table(b"<r><i><d>2024-02-30</d></i><i><d>2024-03-01</d></i></r>").unwrap();
    assert_eq!(v(&wb, 1, 0), Value::text("2024-02-30"));
}

/// Entities a document declares are not expanded (no "billion laughs"), and an external one is
/// never read: only the five predefined entities and character references become text.
#[test]
fn declared_entities_are_not_expanded_and_external_ones_are_not_read() {
    let doc = r#"<?xml version="1.0"?>
<!DOCTYPE r [<!ENTITY a "aaaaaaaaaa"><!ENTITY b "&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;"><!ENTITY x SYSTEM "file:///etc/hostname">]>
<r><v>&b;</v><w>&x;</w><z>&amp; &#65;</z></r>"#;
    let (wb, _) = read_xml_table(doc.as_bytes()).unwrap();
    let cells: Vec<String> = (0..3)
        .map(|c| match v(&wb, 1, c) {
            Value::Text(t) => t.to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(cells[0], "&b;");
    assert_eq!(cells[1], "&x;");
    assert_eq!(cells[2], "& A");
}
