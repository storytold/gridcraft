//! Excel's limits as the bound of legitimate input, and the schema as the list of what a shared
//! string may hold. A worksheet cell that repeats an address already read is read again, as
//! before, and a warning counts the repeats the reader sees (see `read_craft_row`); cells out of
//! column order or in another row's element are placed where their addresses say, and counted.
//! Cell text and shared-string runs past Excel's length are kept whole, since cutting them would
//! lose the user's data at the next save, and warned about. Elements the schema does not know in a
//! shared string are tolerated a few times and then dropped, never crowd out an element the schema
//! allows, and are named in a warning, since nothing reads them.

use gridcraft_core::{CellRef, MAX_CELL_CHARS, MAX_COLS, Value, col_to_letters};

use super::minimal;
use crate::read_xlsx;
use crate::xml::{MAX_UNLISTED_CHILD_NAMES, MAX_XML_NODES};

const SST_RELS: &str = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#;
const A1_IS_SHARED_STRING_0: &str = r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData>"#;

fn string_warning(n: u64) -> String {
    format!(
        "shared strings: dropped {n} element(s), counting what they held, that a string may not hold (a second phoneticPr, unknown elements, or more elements than a part may hold)"
    )
}
fn repeat_warning(n: u64) -> String {
    format!("sheet \"Data\": {n} cell(s), or a row's properties, repeated an address or row already read; the later ones were read again")
}
fn elsewhere_warning(n: u64) -> String {
    format!("sheet \"Data\": {n} cell(s) sat out of column order, or in another row's element, and were placed where their addresses say")
}
const CUT: &str = "shared strings: 1 string(s) were being read when the part reached its element limit; each lost, whole, the parts that did not fit within that limit";
const LONG_TEXT: &str = "sheet \"Data\": 1 cell text(s) longer than Excel's limit of 32767 UTF-16 units were read; they were not cut to that length";
fn long_warning(n: u64) -> String {
    format!("sheet \"Data\": {n} cell text(s) longer than Excel's limit of 32767 UTF-16 units were read; they were not cut to that length")
}

/// One row: `cells` cells from A1 on, then `extra` cells addressed A2, valued 1000, 1001 and on.
fn row_of(cells: u32, extra: usize) -> String {
    let mut body = String::from("<sheetData><row r=\"1\">");
    for col in 0..cells {
        body.push_str(&format!("<c r=\"{}1\"><v>{}</v></c>", col_to_letters(col), col + 1));
    }
    for n in 0..extra {
        body.push_str(&format!("<c r=\"A2\"><v>{}</v></c>", 1000 + n));
    }
    body.push_str("</row></sheetData>");
    body
}

fn a1(wb: &gridcraft_model::Workbook) -> Value {
    wb.sheet(0).unwrap().value(CellRef::parse("A1").unwrap())
}

#[test]
fn a_row_element_with_more_cells_than_the_grid_is_wide_is_placed_as_before() {
    // The full width, read whole with nothing said.
    let (wb, rep) = read_xlsx(&minimal(&row_of(MAX_COLS, 0), &[], "", "")).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(a1(&wb), Value::Number(1.0));
    assert_eq!(s.value(CellRef::parse("XFD1").unwrap()), Value::Number(MAX_COLS as f64));
    assert_eq!(s.cells.len(), MAX_COLS as usize);
    assert!(!rep.warnings.iter().any(|w| w.contains("dropped") || w.contains("already read") || w.contains("column order")), "{:?}", rep.warnings);
    // Two cells more, both addressed A2, in row 1's element: each address is inside the grid, so
    // both are placed, the last one read kept at A2. The first is named for sitting in another
    // row's element, the second for repeating it.
    let (wb, rep) = read_xlsx(&minimal(&row_of(MAX_COLS, 2), &[], "", "")).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(value(&wb, "A2"), Value::Number(1001.0));
    assert_eq!(s.value(CellRef::parse("XFD1").unwrap()), Value::Number(MAX_COLS as f64));
    assert_eq!(s.cells.len(), MAX_COLS as usize + 1);
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&elsewhere_warning(1)), "{:?}", rep.warnings);
    // Without addresses, cells take the next column: those past column XFD are outside the grid
    // and dropped by the row reader, with its own warning.
    let body = format!("<sheetData><row r=\"1\">{}</row></sheetData>", "<c><v>1</v></c>".repeat(MAX_COLS as usize + 2));
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().cells.len(), MAX_COLS as usize);
    assert!(
        rep.warnings.iter().any(|w| w == "sheet \"Data\": 2 cells or rows outside the 1,048,576 × 16,384 grid were dropped"),
        "{:?}",
        rep.warnings
    );
}

/// A shared-string table with one string: `unknown` invented elements, then the plain text if
/// asked, `runs` one-character rich-text runs, `phonetic` one-character phonetic runs, and `props`
/// phonetic properties elements.
fn sst_with(unknown: usize, plain: Option<&str>, runs: usize, phonetic: usize, props: usize) -> String {
    let mut si = String::from("<si>");
    for i in 0..unknown {
        si.push_str(&format!("<unknown{i}/>"));
    }
    if let Some(t) = plain {
        si.push_str(&format!("<t>{t}</t>"));
    }
    si.push_str(&"<r><t>a</t></r>".repeat(runs));
    si.push_str(&"<rPh sb=\"0\" eb=\"1\"><t>b</t></rPh>".repeat(phonetic));
    si.push_str(&"<phoneticPr fontId=\"0\"/>".repeat(props));
    si.push_str("</si>");
    format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="1" uniqueCount="1">{si}</sst>"#)
}

fn read_sst(sst: String) -> (gridcraft_model::Workbook, crate::ReadReport) {
    read_xlsx(&minimal(A1_IS_SHARED_STRING_0, &[("xl/sharedStrings.xml", &sst)], SST_RELS, "")).unwrap()
}

#[test]
fn a_shared_string_keeps_its_runs_whole_past_excels_length_and_drops_only_what_the_schema_forbids() {
    let most = MAX_CELL_CHARS as usize;
    // Everything the schema allows, with as many runs as a cell has characters: read whole, no
    // warning.
    let (wb, rep) = read_sst(sst_with(0, Some(""), most, most, 1));
    assert_eq!(a1(&wb), Value::Text("a".repeat(most).into()));
    assert!(
        !rep.warnings.iter().any(|w| w.starts_with("shared strings") || w.contains("dropped") || w.contains("longer than Excel's")),
        "{:?}",
        rep.warnings
    );
    // One run more than Excel's length: kept whole (32,768 characters), with the long-text warning
    // and no drop.
    let (wb, rep) = read_sst(sst_with(0, None, most + 1, 0, 0));
    assert_eq!(a1(&wb), Value::Text("a".repeat(most + 1).into()));
    assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("dropped")), "{:?}", rep.warnings);
    // A second phonetic properties element is one more than the schema allows: dropped with the
    // warning. A second text is text, kept and joined to the first as in an inline string.
    let two_of_each = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>first</t><t>second</t><phoneticPr fontId="0"/><phoneticPr fontId="1"/></si></sst>"#;
    let (wb, rep) = read_sst(two_of_each.to_string());
    assert_eq!(a1(&wb), Value::Text("firstsecond".into()));
    assert!(rep.warnings.contains(&string_warning(1)), "{:?}", rep.warnings);
    // Unknown elements before the text are tolerated a few times and never crowd the text out.
    let (wb, rep) = read_sst(sst_with(MAX_UNLISTED_CHILD_NAMES, Some("hello"), 0, 0, 1));
    assert_eq!(a1(&wb), Value::Text("hello".into()));
    assert!(!rep.warnings.iter().any(|w| w.contains("dropped")), "{:?}", rep.warnings);
    let (wb, rep) = read_sst(sst_with(1000, Some("hello"), 2, 1, 1));
    assert_eq!(a1(&wb), Value::Text("helloaa".into()));
    assert!(rep.warnings.contains(&string_warning(1000 - MAX_UNLISTED_CHILD_NAMES as u64)), "{:?}", rep.warnings);
}

#[test]
fn cell_text_longer_than_excel_allows_is_kept_whole_with_a_warning() {
    let most = MAX_CELL_CHARS as usize;
    let plain =
        |n: usize| format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>{}</t></si></sst>"#, "a".repeat(n));
    // A shared string of exactly the limit: no warning. One character more: kept whole, warned.
    let (wb, rep) = read_sst(plain(most));
    assert_eq!(a1(&wb), Value::Text("a".repeat(most).into()));
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    let (wb, rep) = read_sst(plain(most + 1));
    assert_eq!(a1(&wb), Value::Text("a".repeat(most + 1).into()));
    assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
    // A long shared string in a cell inside another formula's array isn't kept there (the array
    // fills it), so it isn't counted.
    let in_array = r#"<sheetData><row r="1"><c r="A1"><f t="array" ref="A1:A2">1</f><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>0</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(in_array, &[("xl/sharedStrings.xml", &plain(most + 1))], SST_RELS, "")).unwrap();
    assert_eq!(value(&wb, "A2"), Value::Empty);
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    // A long shared string no cell uses is not warned about; the cells using
    // one are counted, each.
    let two = |cells: &str| {
        let sst = format!(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>short</t></si><si><t>{}</t></si></sst>"#,
            "a".repeat(most + 1)
        );
        read_xlsx(&minimal(&format!("<sheetData><row r=\"1\">{cells}</row></sheetData>"), &[("xl/sharedStrings.xml", &sst)], SST_RELS, "")).unwrap()
    };
    let (wb, rep) = two(r#"<c r="A1" t="s"><v>0</v></c>"#);
    assert_eq!(a1(&wb), Value::Text("short".into()));
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    let (_, rep) = two(r#"<c r="A1" t="s"><v>1</v></c><c r="B1" t="s"><v>1</v></c>"#);
    assert!(rep.warnings.contains(&long_warning(2)), "{:?}", rep.warnings);
    // Inline strings (in `is`, or in `v`) in the sheet itself: the
    // same, exactly at the limit and one past it.
    let in_sheet = |n: usize| {
        let text = "a".repeat(n);
        [
            format!(r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{text}</t></is></c></row></sheetData>"#),
            format!(r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><v>{text}</v></c></row></sheetData>"#),
        ]
    };
    for body in in_sheet(most + 1) {
        let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
        assert_eq!(a1(&wb), Value::Text("a".repeat(most + 1).into()));
        assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
    }
    for body in in_sheet(most) {
        let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
        assert_eq!(a1(&wb), Value::Text("a".repeat(most).into()));
        assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    }
    // A formula's cached result is read as it is, but opening a workbook calculates the formulas
    // that parse again, so the cached text isn't kept and isn't counted.
    let formula = format!(r#"<sheetData><row r="1"><c r="A1" t="str"><f>REPT("a",40000)</f><v>{}</v></c></row></sheetData>"#, "a".repeat(most + 1));
    let (wb, rep) = read_xlsx(&minimal(&formula, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Text("a".repeat(most + 1).into()));
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    // A formula GridCraft can't parse isn't calculated, so its cached text is kept, and it is
    // counted.
    let unparsed = format!(r#"<sheetData><row r="1"><c r="A1" t="str"><f>1+</f><v>{}</v></c></row></sheetData>"#, "a".repeat(most + 1));
    let (wb, rep) = read_xlsx(&minimal(&unparsed, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Text("a".repeat(most + 1).into()));
    assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
    // The limit is counted as Excel counts, in UTF-16 units: 16,384 characters outside the Basic
    // Multilingual Plane are 32,768 units, one over, and 16,383 are within it.
    let astral =
        |n: usize| format!(r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{}</t></is></c></row></sheetData>"#, "\u{1F600}".repeat(n));
    let (wb, rep) = read_xlsx(&minimal(&astral(most / 2 + 1), &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Text("\u{1F600}".repeat(most / 2 + 1).into()));
    assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
    let (_, rep) = read_xlsx(&minimal(&astral(most / 2), &[], "", "")).unwrap();
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    // At the limit exactly, nothing is said.
    let body = format!(r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{}</t></is></c></row></sheetData>"#, "a".repeat(most));
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Text("a".repeat(most).into()));
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    // A shared string longer than the limit in bytes but not in UTF-16 units: 20,000 `€` are
    // 60,000 bytes and 20,000 units, so the cell using it is not counted.
    let (wb, rep) =
        read_sst(format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>{}</t></si></sst>"#, "€".repeat(20_000)));
    assert_eq!(a1(&wb), Value::Text("€".repeat(20_000).into()));
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
}

#[test]
fn a_long_text_is_counted_when_read_even_where_a_later_cell_replaces_it() {
    // The long-text warning counts the long texts read, as each cell is read: a later cell at the
    // same address replaces the text but does not take it off the count.
    let long = "a".repeat(MAX_CELL_CHARS as usize + 1);
    let long_cell = format!(r#"<c r="A1" t="inlineStr"><is><t>{long}</t></is></c>"#);
    let plain_cell = r#"<c r="A1"><v>1</v></c>"#;
    let read = |cells: &str| read_xlsx(&minimal(&format!(r#"<sheetData><row r="1">{cells}</row></sheetData>"#), &[], "", "")).unwrap();
    // Long, then plain: one long text read, the plain value kept.
    let (wb, rep) = read(&format!("{long_cell}{plain_cell}"));
    assert_eq!(a1(&wb), Value::Number(1.0));
    assert!(rep.warnings.contains(&long_warning(1)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    // Long, then long: two read. Plain, then long: one.
    for (cells, n) in [(format!("{long_cell}{long_cell}"), 2), (format!("{plain_cell}{long_cell}"), 1)] {
        let (wb, rep) = read(&cells);
        assert_eq!(a1(&wb), Value::Text(long.as_str().into()));
        assert!(rep.warnings.contains(&long_warning(n)), "{n}: {:?}", rep.warnings);
        assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    }
    // The same through a long shared string, with another cell between the two A1s.
    let sst = format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>{long}</t></si></sst>"#);
    let body = r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1"><v>2</v></c><c r="A1"><v>1</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[("xl/sharedStrings.xml", &sst)], SST_RELS, "")).unwrap();
    assert_eq!(a1(&wb), Value::Number(1.0));
    assert!(rep.warnings.contains(&long_warning(1)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
}

#[test]
fn twenty_thousand_copies_of_a_cell_using_a_long_shared_string_are_each_counted() {
    // Twenty thousand copies of A1, each using one shared string, of 32,000 characters (within
    // Excel's length) and then of one past it: each repeat is counted, and on the long string
    // each copy read is counted as a long text.
    let cells = format!(r#"<sheetData><row r="1">{}</row></sheetData>"#, r#"<c r="A1" t="s"><v>0</v></c>"#.repeat(20_000));
    for (len, long) in [(32_000, 0), (MAX_CELL_CHARS as usize + 1, 20_000)] {
        let sst = format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>{}</t></si></sst>"#, "a".repeat(len));
        let (wb, rep) = read_xlsx(&minimal(&cells, &[("xl/sharedStrings.xml", &sst)], SST_RELS, "")).unwrap();
        assert_eq!(a1(&wb), Value::Text("a".repeat(len).into()));
        assert!(rep.warnings.contains(&repeat_warning(19_999)), "{:?}", rep.warnings);
        assert_eq!(rep.warnings.contains(&long_warning(long)), long > 0, "{:?}", rep.warnings);
        assert!(long > 0 || !rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    }
}

fn value(wb: &gridcraft_model::Workbook, at: &str) -> Value {
    wb.sheet(0).unwrap().value(CellRef::parse(at).unwrap())
}

#[test]
fn cells_in_the_same_columns_of_the_next_row_past_the_first_sixty_four_columns_are_not_repeats() {
    // The columns a row element held are cleared when a row of another number begins, all of
    // them: CV and XFD lie past the first 64 columns, so row 2's cells there are new, not repeats.
    let body = r#"<sheetData><row r="1"><c r="CV1"><v>1</v></c><c r="XFD1"><v>2</v></c></row><row r="2"><c r="CV2"><v>3</v></c><c r="XFD2"><v>4</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    for (at, n) in [("CV1", 1.0), ("XFD1", 2.0), ("CV2", 3.0), ("XFD2", 4.0)] {
        assert_eq!(value(&wb, at), Value::Number(n), "{at}");
    }
    assert!(!rep.warnings.iter().any(|w| w.contains("already read") || w.contains("column order")), "{:?}", rep.warnings);
}

#[test]
fn a_cell_repeating_an_address_already_read_replaces_it_with_a_warning() {
    // Excel writes each cell once, in order, in its own row. A cell that repeats an address
    // is read over what was read there, as before, and is counted, and so are a repeated row's
    // properties where the first one had some. A cell out of column order, or in
    // another row's element, lands where its address says, and an unaddressed cell takes the
    // column after the one before it, in its own row.
    let body = r#"<sheetData><row r="1" ht="20" customHeight="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c><c r="B1"><v>2</v></c><c r="A1"><v>999</v></c><c r="D7"><v>7</v></c><c><v>8</v></c></row><row r="2"><c r="A2"><v>20</v></c><c r="A1"><v>998</v></c></row><row r="1" ht="30" customHeight="1"><c r="A1"><v>997</v></c><c r="G1"><v>9</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    for (at, n) in [("A1", 997.0), ("B1", 2.0), ("C1", 3.0), ("D7", 7.0), ("E1", 8.0), ("A2", 20.0), ("G1", 9.0)] {
        assert_eq!(value(&wb, at), Value::Number(n), "{at}");
    }
    // Three repeats of A1 and the repeated row 1's height.
    assert!(rep.warnings.contains(&repeat_warning(4)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&elsewhere_warning(2)), "B1 out of order and D7 in row 1's element are named: {:?}", rep.warnings);
    let row_one = |body: &str| read_xlsx(&minimal(body, &[], "", "")).unwrap().0.sheet(0).unwrap().rows.get(&0).cloned();
    let second_height = row_one(r#"<sheetData><row r="1" ht="30" customHeight="1"/></sheetData>"#);
    assert!(second_height.is_some() && second_height != row_one(r#"<sheetData><row r="1" ht="20" customHeight="1"/></sheetData>"#));
    assert_eq!(wb.sheet(0).unwrap().rows.get(&0).cloned(), second_height, "the last row 1's height is kept");
    // Properties a repeated row brings where the first had none replace nothing: kept, silently.
    let body = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="1" ht="30" customHeight="1"><c r="B1"><v>2</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().rows.get(&0).cloned(), second_height);
    assert_eq!(value(&wb, "B1"), Value::Number(2.0));
    assert!(!rep.warnings.iter().any(|w| w.contains("already read") || w.contains("column order")), "{:?}", rep.warnings);
    // An unaddressed cell after a repeat takes the column after the repeat: B1, left of C1, so
    // out of column order.
    let body = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c><c r="A1"><v>999</v></c><c><v>8</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!((a1(&wb), value(&wb, "B1"), value(&wb, "C1")), (Value::Number(999.0), Value::Number(8.0), Value::Number(3.0)));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&elsewhere_warning(1)), "{:?}", rep.warnings);
    // A full row and then a cell in the next row's element naming A1.
    let body = row_of(MAX_COLS, 0).replace("</row></sheetData>", r#"</row><row r="2"><c r="A1"><v>999</v></c></row></sheetData>"#);
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Number(999.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    // A sparse sheet in order says nothing.
    let body = r#"<sheetData><row r="1"><c r="A1"/><c r="C1"><v>3</v></c></row><row r="3"><c r="B3"><v>1</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(value(&wb, "B3"), Value::Number(1.0));
    assert!(!rep.warnings.iter().any(|w| w.contains("already read") || w.contains("out of column order")), "{:?}", rep.warnings);
    // A repeat of the furthest cell read, after a cell out of order: the shortcut past the furthest
    // cell must not miss it.
    let body =
        r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c><c r="B1"><v>2</v></c><c r="C1"><v>999</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(value(&wb, "C1"), Value::Number(999.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&elsewhere_warning(1)), "{:?}", rep.warnings);
    // A cell naming another row than the one it sits in, repeating nothing: placed where its
    // address says and named.
    let body = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B5"><v>5</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!((a1(&wb), value(&wb, "B5")), (Value::Number(1.0), Value::Number(5.0)));
    assert!(rep.warnings.contains(&elsewhere_warning(1)), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("already read")), "{:?}", rep.warnings);
}

#[test]
fn a_blank_cell_repeated_in_a_later_element_of_the_same_row_is_a_repeat() {
    // A blank cell without a style stores nothing; repeated in a second row element for row 1,
    // right after the first, it is a column that row's elements already held, and the furthest
    // cell read.
    let body = r#"<sheetData><row r="1"><c r="A1"/></row><row r="1"><c r="A1"/></row></sheetData>"#;
    let (_, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("out of column order")), "{:?}", rep.warnings);
}

#[test]
fn a_cell_repeating_a_cell_the_reader_stores_nothing_for_in_the_elements_for_its_row_is_a_repeat() {
    // A blank cell without a style, and a value inside another formula's array, leave nothing
    // stored. Repeated in the same row element after a later cell, each is counted as a repeat,
    // not as a cell out of column order.
    for body in [
        r#"<sheetData><row r="1"><c r="A1"/><c r="B1"><v>2</v></c><c r="A1"/></row></sheetData>"#,
        r#"<sheetData><row r="1"><c r="A1"><f t="array" ref="A1:B1">1+1</f><v>2</v></c><c r="B1"><v>2</v></c><c r="C1"><v>3</v></c><c r="B1"><v>2</v></c></row></sheetData>"#,
    ] {
        let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
        assert_eq!(value(&wb, "C1"), if body.contains("C1") { Value::Number(3.0) } else { Value::Empty });
        assert!(rep.warnings.contains(&repeat_warning(1)), "{body}: {:?}", rep.warnings);
        assert!(!rep.warnings.iter().any(|w| w.contains("column order")), "{body}: {:?}", rep.warnings);
    }
    // In a later element for the same row, right after the first, in column order, it is a repeat
    // too: the elements for one row in a row share the columns they held.
    let body = r#"<sheetData><row r="1"><c r="A1"/><c r="B1"><v>2</v></c></row><row r="1"><c r="A1"/></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!((a1(&wb), value(&wb, "B1")), (Value::Empty, Value::Number(2.0)));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("column order")), "{:?}", rep.warnings);
    // The same for a value inside another formula's array.
    let body = r#"<sheetData><row r="1"><c r="A1"><f t="array" ref="A1:B1">1+1</f><v>2</v></c><c r="B1"><v>2</v></c><c r="C1"><v>3</v></c></row><row r="1"><c r="B1"><v>5</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(value(&wb, "C1"), Value::Number(3.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("column order")), "{:?}", rep.warnings);
    // An element for another row between the two copies clears the columns held, so the copy in
    // the third element, in column order and not the furthest cell read, repeating a cell the
    // reader stored nothing for, replaces nothing and is not counted, in either warning.
    let body = r#"<sheetData><row r="1"><c r="A1"/><c r="B1"><v>2</v></c></row><row r="2"><c r="A2"><v>3</v></c></row><row r="1"><c r="A1"/></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!((a1(&wb), value(&wb, "B1"), value(&wb, "A2")), (Value::Empty, Value::Number(2.0), Value::Number(3.0)));
    assert!(!rep.warnings.iter().any(|w| w.contains("already read") || w.contains("column order")), "{:?}", rep.warnings);
}

#[test]
fn each_cell_left_of_the_rightmost_one_read_in_its_row_element_is_out_of_column_order() {
    // Each cell is placed where its address says; every one not to the right of the rightmost
    // cell read before it in its row element is counted.
    for (body, n) in [
        (r#"<sheetData><row r="1"><c r="C1"><v>3</v></c><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row></sheetData>"#, 2),
        (r#"<sheetData><row r="1"><c r="D1"><v>4</v></c><c r="B1"><v>2</v></c><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c></row></sheetData>"#, 3),
    ] {
        let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
        for (at, v) in [("A1", 1.0), ("B1", 2.0), ("C1", 3.0)] {
            assert_eq!(value(&wb, at), Value::Number(v), "{at}");
        }
        assert!(rep.warnings.contains(&elsewhere_warning(n)), "{n}: {:?}", rep.warnings);
        assert!(!rep.warnings.iter().any(|w| w.contains("already read")), "{:?}", rep.warnings);
    }
}

fn formula(wb: &gridcraft_model::Workbook, at: &str) -> Option<String> {
    wb.sheet(0).unwrap().cell(CellRef::parse(at).unwrap()).and_then(|c| c.formula.as_ref()).map(|f| f.text.clone())
}

#[test]
fn a_repeated_cell_keeps_the_last_ones_formula_value_and_style() {
    let shared = |first: &str, second: &str| {
        format!(r#"<sheetData><row r="1">{first}{second}</row><row r="2"><c r="A2"><f t="shared" si="0"/><v>1</v></c></row></sheetData>"#)
    };
    let plain = r#"<c r="A1"><v>5</v></c>"#;
    let master = r#"<c r="A1"><f t="shared" ref="A1:A2" si="0">B1+1</f><v>1</v></c>"#;
    // A plain A1, then a second A1 that is the master of a shared formula A2 uses: both formulas
    // are live.
    let (wb, rep) = read_xlsx(&minimal(&shared(plain, master), &[], "", "")).unwrap();
    assert_eq!((formula(&wb, "A1").as_deref(), formula(&wb, "A2").as_deref()), (Some("B1+1"), Some("B2+1")));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    // The master first, then a plain A1: A1 keeps the plain value, and A2 still gets its formula.
    let (wb, rep) = read_xlsx(&minimal(&shared(master, plain), &[], "", "")).unwrap();
    assert_eq!((a1(&wb), formula(&wb, "A1")), (Value::Number(5.0), None));
    assert_eq!(formula(&wb, "A2").as_deref(), Some("B2+1"));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    // An empty cell with a style is a stored cell; a value after it at its address replaces it,
    // with the warning, also when another cell came between them.
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="10" applyNumberFormat="1"/></cellXfs></styleSheet>"#;
    let style_rels =
        r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let styled = |cells: &str| {
        read_xlsx(&minimal(&format!(r#"<sheetData><row r="1">{cells}</row></sheetData>"#), &[("xl/styles.xml", styles)], style_rels, "")).unwrap()
    };
    let a1_format = |wb: &gridcraft_model::Workbook| {
        wb.sheet(0).unwrap().cell(CellRef::parse("A1").unwrap()).map(|c| wb.styles.get(c.style).num_fmt.as_str().to_owned())
    };
    let (wb, _) = styled(r#"<c r="A1" s="1"/>"#);
    assert_eq!(a1_format(&wb).as_deref(), Some("0.00%"), "an empty styled cell is stored with its style");
    for cells in [r#"<c r="A1" s="1"/><c r="A1"><v>7</v></c>"#, r#"<c r="A1" s="1"/><c r="B1"><v>2</v></c><c r="A1"><v>7</v></c>"#] {
        let (wb, rep) = styled(cells);
        assert_eq!(a1(&wb), Value::Number(7.0), "{cells}");
        assert_eq!(a1_format(&wb).as_deref(), Some("General"), "{cells}: the last A1 has no style");
        assert!(rep.warnings.contains(&repeat_warning(1)), "{cells}: {:?}", rep.warnings);
        assert!(!rep.warnings.iter().any(|w| w.contains("column order")), "{cells}: {:?}", rep.warnings);
    }
    // An empty cell without a style isn't stored; a value straight after it at its address is
    // kept, and counted as a repeat, not as out of order.
    let body = r#"<sheetData><row r="1"><c r="A1"/><c r="A1"><v>7</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Number(7.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("column order")), "{:?}", rep.warnings);
    // As many copies of A1 as the grid has columns, then B1: the last A1 and B1 are kept.
    let body = format!(
        r#"<sheetData><row r="1">{}<c r="B1"><v>2</v></c></row></sheetData>"#,
        (0..MAX_COLS).map(|n| format!(r#"<c r="A1"><v>{n}</v></c>"#)).collect::<String>()
    );
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!((a1(&wb), value(&wb, "B1")), (Value::Number(f64::from(MAX_COLS - 1)), Value::Number(2.0)));
    assert!(rep.warnings.contains(&repeat_warning(u64::from(MAX_COLS) - 1)), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("column order")), "{:?}", rep.warnings);
}

#[test]
fn a_row_number_repeated_after_an_earlier_row_keeps_the_last_rows_cells_and_height() {
    // Rows 3, 1 and 3 again: row 1 comes after row 3, which is out of order but repeats nothing,
    // and the second row 3 repeats both a cell and the row's height. The last A3 and the last
    // height are kept, B3 from the repeat fills an empty address, and only the repeats are
    // counted. Nothing sits out of column order or in another row's element.
    let body = r#"<sheetData><row r="3" ht="20" customHeight="1"><c r="A3"><v>1</v></c><c r="C3"><v>3</v></c></row><row r="1"><c r="A1"><v>10</v></c></row><row r="3" ht="30" customHeight="1"><c r="A3"><v>999</v></c><c r="B3"><v>2</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    for (at, n) in [("A3", 999.0), ("B3", 2.0), ("C3", 3.0), ("A1", 10.0)] {
        assert_eq!(value(&wb, at), Value::Number(n), "{at}");
    }
    let third = |body: &str| read_xlsx(&minimal(body, &[], "", "")).unwrap().0.sheet(0).unwrap().rows.get(&2).cloned();
    let last_height = third(r#"<sheetData><row r="3" ht="30" customHeight="1"/></sheetData>"#);
    assert!(last_height.is_some() && last_height != third(r#"<sheetData><row r="3" ht="20" customHeight="1"/></sheetData>"#));
    assert_eq!(wb.sheet(0).unwrap().rows.get(&2).cloned(), last_height, "the last row 3's height is kept");
    assert!(rep.warnings.contains(&repeat_warning(2)), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("out of column order")), "{:?}", rep.warnings);
}

#[test]
fn what_was_repeated_before_a_part_turned_out_damaged_is_still_said() {
    // A repeated cell in the first row, then a part that ends inside a comment: the repeat was
    // read in a row before the damage, and the warning says so beside the "damaged" one.
    let body = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="A1"><v>999</v></c></row><row r="2"><c r="A2"><v>2</v></c></row><!-- "#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert!(rep.warnings.iter().any(|w| w.starts_with("sheet \"Data\" is damaged")), "{:?}", rep.warnings);
    assert_eq!(a1(&wb), Value::Number(999.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
}

#[test]
fn string_elements_nothing_reads_are_named() {
    // The unknown elements a string may hold are kept out of the table by the reader, which
    // doesn't read them: a warning says so, with the count.
    let (_, rep) = read_sst(sst_with(2, Some("hello"), 0, 0, 0));
    assert!(
        rep.warnings
            .contains(&"shared strings: 2 element(s) GridCraft does not read (unknown elements) were left out and will not be saved".to_string()),
        "{:?}",
        rep.warnings
    );
    // A plain string says nothing.
    let (_, rep) = read_sst(sst_with(0, Some("hello"), 0, 0, 0));
    assert!(!rep.warnings.iter().any(|w| w.starts_with("shared strings")), "{:?}", rep.warnings);
}

#[test]
fn what_was_dropped_before_a_string_table_turned_out_damaged_is_still_said() {
    let sst = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>a</t><phoneticPr fontId="0"/><phoneticPr fontId="1"/></si><!-- "#;
    let (_, rep) = read_sst(sst.to_string());
    assert!(rep.warnings.iter().any(|w| w.starts_with("shared strings are damaged")), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&string_warning(1)), "{:?}", rep.warnings);
}

#[test]
fn a_shared_string_cut_at_the_element_cap_is_said_to_be_cut_and_still_too_long() {
    // A string of more runs than a part may hold elements keeps what came before the cap; the
    // warning says it was cut, and the text A1 keeps, still longer than Excel allows, is named in
    // the long-text warning.
    let sst =
        format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si>{}</si></sst>"#, "<r><t>a</t></r>".repeat(600_000));
    let (wb, rep) = read_sst(sst);
    let Value::Text(text) = a1(&wb) else { panic!("A1 is text") };
    // 500,000 runs of two elements each fill the cap of 1,000,000; the run that crosses it is lost.
    assert_eq!(text.len(), 500_000);
    assert!(rep.warnings.contains(&CUT.to_string()), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
}

#[test]
fn a_shared_string_cut_at_the_element_cap_keeps_the_text_after_the_part_that_crossed_it() {
    // The run between "head" and "tail" holds a million elements and crosses the cap: it is
    // dropped whole with what it holds, the count goes back to where it began, and the run after
    // it fits again, so the string keeps the text on both sides of it. The warning says so.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>head</t><r><t>middle</t>{}</r><r><t>tail</t></r></si></sst>"#,
        "<x/>".repeat(1_000_001)
    );
    let (wb, rep) = read_sst(sst);
    assert_eq!(a1(&wb), Value::Text("headtail".into()));
    assert!(rep.warnings.contains(&CUT.to_string()), "{:?}", rep.warnings);
}

#[test]
fn a_run_that_lost_part_of_its_text_at_the_element_cap_is_dropped_whole() {
    // The middle run's text holds a million and one elements: the one that crosses the cap is
    // dropped, so the `t` holding it, and the run holding that, go whole too ("middle" is not
    // kept as "midle" or "mid"), counted with what they held: the two past the cap, the 999,999
    // before them in `b`, and `b`, `t` and `r` themselves. The runs on both sides are kept.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>head</t><r><t>mid<b>{}</b>dle</t></r><r><t>tail</t></r></si></sst>"#,
        "<x/>".repeat(1_000_001)
    );
    let (wb, rep) = read_sst(sst);
    assert_eq!(a1(&wb), Value::Text("headtail".into()));
    assert!(rep.warnings.contains(&string_warning(1_000_004)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&CUT.to_string()), "{:?}", rep.warnings);
}

#[test]
fn a_string_whose_refused_child_crosses_the_element_cap_is_not_said_to_be_cut() {
    // A second phoneticPr is one more than a string may hold, and it holds a million elements: it
    // is dropped whole for its allowance, and nothing the string keeps is lost, so the string is
    // whole (with its run after it) and its length is what the warning speaks of, not a cut.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>{}</t><phoneticPr fontId="0"/><phoneticPr fontId="1">{}</phoneticPr><r><t>tail</t></r></si></sst>"#,
        "a".repeat(40_000),
        "<x/>".repeat(1_000_001)
    );
    let (wb, rep) = read_sst(sst);
    let Value::Text(text) = a1(&wb) else { panic!("A1 is text") };
    assert_eq!(text.len(), 40_004);
    assert!(text.ends_with("tail"));
    assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("reached its element limit")), "{:?}", rep.warnings);
}

#[test]
fn a_string_that_loses_only_an_unknown_element_at_the_element_cap_is_not_said_to_be_cut() {
    // The unknown element holds a million elements and crosses the cap: it is dropped whole and
    // counted, and since the reader reads nothing in it, the string keeps its text on both sides
    // and is not said to be cut.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>head</t><unknown>{}</unknown><r><t>tail</t></r></si></sst>"#,
        "<x/>".repeat(1_000_001)
    );
    let (wb, rep) = read_sst(sst);
    assert_eq!(a1(&wb), Value::Text("headtail".into()));
    assert!(rep.warnings.iter().any(|w| w.starts_with("shared strings: dropped ")), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("reached its element limit")), "{:?}", rep.warnings);
}

#[test]
fn a_shared_string_that_lost_all_its_text_at_the_element_cap_counts_as_missing() {
    // The second string's only text holds a million elements and crosses the cap: it is dropped
    // whole, and the string lost text and kept none. Its cell reads as one naming a string the
    // table doesn't hold, as on main (empty, with that warning), not as an empty text; the strings
    // on both sides keep their text at their own indexes.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>alpha</t></si><si><t>beta{}</t></si><si><t>gamma</t></si></sst>"#,
        "<z/>".repeat(1_000_000)
    );
    let body = r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[("xl/sharedStrings.xml", &sst)], SST_RELS, "")).unwrap();
    assert_eq!((value(&wb, "A1"), value(&wb, "B1"), value(&wb, "C1")), (Value::Text("alpha".into()), Value::Empty, Value::Text("gamma".into())));
    assert!(wb.sheet(0).unwrap().cell(CellRef::parse("B1").unwrap()).is_none(), "B1 holds no cell");
    assert!(rep.warnings.contains(&"some cells refer to missing shared strings".to_string()), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&format!("{CUT}; 1 of them kept no text and count as missing strings")), "{:?}", rep.warnings);
}

/// The phonetic runs kept for the string in A1, as (start, end, reading).
fn a1_runs(wb: &gridcraft_model::Workbook) -> Vec<(u32, u32, String)> {
    wb.phonetics.iter().flat_map(|p| p.runs.iter().map(|r| (r.start, r.end, r.text.clone()))).collect()
}

/// The cut warning's ending for `n` phonetic runs dropped from strings that lost text.
fn unplaced_runs(n: u64) -> String {
    format!("; {n} phonetic run(s) in strings that lost text were dropped, since their offsets point into the text as it was")
}

#[test]
fn a_shared_string_cut_at_the_element_cap_that_kept_no_text_counts_as_missing_also_with_an_empty_run() {
    // The middle string keeps an empty run, then loses its text at the cap: it kept no text, so it
    // counts as missing, as one that kept nothing does. The same string under the cap, an empty
    // run alone, reads as empty text, as on main.
    let body = r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c></row></sheetData>"#;
    for (middle, cut) in [(format!("<si><r/><t>beta{}</t></si>", "<z/>".repeat(1_000_000)), true), ("<si><r/></si>".to_string(), false)] {
        let sst = format!(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>alpha</t></si>{middle}<si><t>gamma</t></si></sst>"#
        );
        let (wb, rep) = read_xlsx(&minimal(body, &[("xl/sharedStrings.xml", &sst)], SST_RELS, "")).unwrap();
        let expected = if cut { Value::Empty } else { Value::Text("".into()) };
        assert_eq!((value(&wb, "A1"), value(&wb, "B1"), value(&wb, "C1")), (Value::Text("alpha".into()), expected, Value::Text("gamma".into())));
        assert_eq!(wb.sheet(0).unwrap().cell(CellRef::parse("B1").unwrap()).is_none(), cut, "{:?}", rep.warnings);
        assert_eq!(rep.warnings.contains(&"some cells refer to missing shared strings".to_string()), cut, "{:?}", rep.warnings);
        let cut_warnings: Vec<&String> = rep.warnings.iter().filter(|w| w.contains("reached its element limit")).collect();
        let expected_cut = cut.then(|| format!("{CUT}; 1 of them kept no text and count as missing strings"));
        assert_eq!(cut_warnings, expected_cut.iter().collect::<Vec<_>>(), "{:?}", rep.warnings);
    }
}

#[test]
fn a_shared_string_with_empty_text_that_lost_only_a_phonetic_run_stays_empty_text() {
    // The string's text is empty in the file, and its only phonetic run crosses the cap: no text
    // was lost, so it reads as empty text, said to be cut, not as a missing string.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t/><rPh sb="0" eb="1"><t>reading</t>{}</rPh></si></sst>"#,
        "<x/>".repeat(1_000_001)
    );
    let (wb, rep) = read_sst(sst);
    assert_eq!(a1(&wb), Value::Text("".into()));
    assert!(rep.warnings.contains(&CUT.to_string()), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("missing shared strings")), "{:?}", rep.warnings);
}

#[test]
fn a_shared_string_that_lost_text_at_the_element_cap_drops_its_phonetic_runs() {
    // "middle" crosses the cap and is lost, so the string keeps "headtail". Its phonetic run over
    // offsets 4 to 8 annotated "midd" and would now land on "tail": it is dropped and counted under
    // the cut warning with the other run, whose offsets lie past the text, not as runs with bad
    // offsets, and a save writes no reading. The same run in a string that is not cut is kept.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>head</t><r><t>middle</t>{}</r><r><t>tail</t></r><rPh sb="4" eb="8"><t>ミドル</t></rPh><rPh sb="20" eb="30"><t>ソト</t></rPh></si></sst>"#,
        "<x/>".repeat(1_000_001)
    );
    let (wb, rep) = read_sst(sst);
    assert_eq!(a1(&wb), Value::Text("headtail".into()));
    assert!(a1_runs(&wb).is_empty(), "{:?}", wb.phonetics);
    assert!(rep.warnings.contains(&format!("{CUT}{}", unplaced_runs(2))), "{:?}", rep.warnings);
    assert!(!rep.warnings.iter().any(|w| w.contains("bad sb/eb")), "{:?}", rep.warnings);
    let (saved, _) = read_xlsx(&crate::write_xlsx(&wb).unwrap()).unwrap();
    assert!(a1_runs(&saved).is_empty(), "{:?}", saved.phonetics);
    let whole = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>headtail</t><rPh sb="4" eb="8"><t>ミドル</t></rPh></si></sst>"#;
    let (wb, _) = read_sst(whole.to_string());
    assert_eq!(a1_runs(&wb), [(4, 8, "ミドル".to_string())]);
}

#[test]
fn a_shared_string_that_lost_only_phonetic_parts_keeps_its_text_and_its_other_runs() {
    // One string loses its second phonetic run at the cap, another its phonetic properties: no
    // text was lost, so each keeps its text and its first run, and the cut warning says nothing of
    // dropped runs.
    let many = "<x/>".repeat(1_000_001);
    let first = r#"<t>headtail</t><rPh sb="0" eb="4"><t>ヘッド</t></rPh>"#;
    for lost in [format!(r#"<rPh sb="4" eb="8"><t>テイル</t>{many}</rPh>"#), format!(r#"<phoneticPr fontId="1">{many}</phoneticPr>"#)] {
        let sst = format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si>{first}{lost}</si></sst>"#);
        let (wb, rep) = read_sst(sst);
        assert_eq!(a1(&wb), Value::Text("headtail".into()));
        assert_eq!(a1_runs(&wb), [(0, 4, "ヘッド".to_string())]);
        assert!(wb.phonetics.iter().all(|p| p.props.is_none()), "{:?}", wb.phonetics);
        assert!(rep.warnings.contains(&CUT.to_string()), "{:?}", rep.warnings);
    }
}

#[test]
fn a_shared_string_that_lost_text_files_no_phonetic_data_under_its_shortened_text() {
    // The cut string keeps "head" after "middle" is lost at the cap; its phonetic properties go
    // with its runs, so nothing is filed under "head" for it, and the intact string with the same
    // text keeps its reading through a save, in either order, with no conflict between the two.
    let cut = format!(r#"<si><t>head</t><r><t>middle</t>{}</r><phoneticPr fontId="1"/></si>"#, "<x/>".repeat(1_000_001));
    let intact = r#"<si><t>head</t><rPh sb="0" eb="4"><t>ヘッド</t></rPh></si>"#;
    let body = r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c></row></sheetData>"#;
    for (first, second) in [(cut.as_str(), intact), (intact, cut.as_str())] {
        let sst = format!(r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{first}{second}</sst>"#);
        let (wb, rep) = read_xlsx(&minimal(body, &[("xl/sharedStrings.xml", &sst)], SST_RELS, "")).unwrap();
        assert_eq!((value(&wb, "A1"), value(&wb, "B1")), (Value::Text("head".into()), Value::Text("head".into())));
        assert!(!rep.warnings.iter().any(|w| w.contains("same text")), "{:?}", rep.warnings);
        assert!(rep.warnings.contains(&format!("{CUT}; 1 of them dropped their phonetic properties with the text they lost")), "{:?}", rep.warnings);
        assert_eq!(a1_runs(&wb), [(0, 4, "ヘッド".to_string())]);
        assert!(wb.phonetics.iter().all(|p| p.props.is_none()), "{:?}", wb.phonetics);
        let (saved, _) = read_xlsx(&crate::write_xlsx(&wb).unwrap()).unwrap();
        assert_eq!(a1_runs(&saved), [(0, 4, "ヘッド".to_string())]);
    }
}

#[test]
fn the_phonetic_runs_of_a_string_that_lost_all_its_text_are_counted() {
    // The string's phonetic runs come before its only text, which crosses the cap: it counts as
    // missing, and both runs, the damaged one too, are counted as dropped with the cut.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><rPh sb="0" eb="2"><t>ベ</t></rPh><rPh sb="1"/><t>beta{}</t></si></sst>"#,
        "<z/>".repeat(1_000_000)
    );
    let (wb, rep) = read_sst(sst);
    assert_eq!(a1(&wb), Value::Empty);
    assert!(rep.warnings.contains(&format!("{CUT}; 1 of them kept no text and count as missing strings{}", unplaced_runs(2))), "{:?}", rep.warnings);
    assert!(wb.phonetics.is_empty(), "{:?}", wb.phonetics);
}

#[test]
fn a_shared_string_at_the_element_cap_and_one_past_it() {
    // The string holds `head`, a run of `tail` stuffed with fill elements, and a phonetic run: 5
    // elements besides the fill. With the cap's worth in all, it is whole; with one more, the
    // phonetic run is the part that goes past the cap (the text is whole, so its other parts are
    // kept); with three more, the run of `tail` goes, and the phonetic run, kept after it, is
    // dropped with the text it pointed into.
    let reading = || vec![(0, 4, "ヘッド".to_string())];
    for (fill, text, runs, warning) in [
        (MAX_XML_NODES - 5, "headtail", reading(), None),
        (MAX_XML_NODES - 4, "headtail", vec![], Some(CUT.to_string())),
        (MAX_XML_NODES - 2, "head", vec![], Some(format!("{CUT}{}", unplaced_runs(1)))),
    ] {
        let sst = format!(
            r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>head</t><r><t>tail</t>{}</r><rPh sb="0" eb="4"><t>ヘッド</t></rPh></si></sst>"#,
            "<x/>".repeat(fill)
        );
        let (wb, rep) = read_sst(sst);
        assert_eq!(a1(&wb), Value::Text(text.into()), "{fill}");
        assert_eq!(a1_runs(&wb), runs, "{fill}");
        let cut: Vec<&String> = rep.warnings.iter().filter(|w| w.contains("reached its element limit")).collect();
        assert_eq!(cut, warning.iter().collect::<Vec<_>>(), "{fill}");
    }
}

#[test]
fn a_string_that_loses_an_empty_t_at_the_element_cap_is_taken_to_have_lost_text_by_the_by_name_rule() {
    // The string's text, a phonetic run and empty runs fill the cap exactly, so the empty `t`
    // after them (schema-valid, holding no character) is dropped at it. Which children a cut
    // string lost is known by name only, and a lost `t` counts as lost text whatever it held: the
    // phonetic run is dropped with the cut warning, although "abc" is whole and the run still
    // points at the right character.
    let sst = format!(
        r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><t>abc</t><rPh sb="0" eb="1"><t>ア</t></rPh>{}<t/></si></sst>"#,
        "<r/>".repeat(MAX_XML_NODES - 3)
    );
    let (wb, rep) = read_sst(sst);
    assert_eq!(a1(&wb), Value::Text("abc".into()));
    assert_eq!(a1_runs(&wb), []);
    assert!(rep.warnings.contains(&format!("{CUT}{}", unplaced_runs(1))), "{:?}", rep.warnings);
}

#[test]
fn run_formatting_and_unknown_elements_inside_a_run_are_left_out_without_a_warning_as_on_main() {
    // Only the direct children of a string are counted by name: the run's formatting and an
    // unknown element inside a run are read with it and not kept, with no warning, as on main,
    // and a save writes the text alone.
    let sst = r#"<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><si><r><rPr><b/></rPr><t>bold</t></r><r><t>x</t><zz/></r></si></sst>"#;
    let (wb, rep) = read_sst(sst.to_string());
    assert_eq!(a1(&wb), Value::Text("boldx".into()));
    assert!(rep.warnings.iter().all(|w| !w.starts_with("shared strings")), "{:?}", rep.warnings);
    let saved = crate::write_xlsx(&wb).unwrap();
    let mut part = String::new();
    std::io::Read::read_to_string(
        &mut zip::ZipArchive::new(std::io::Cursor::new(saved)).unwrap().by_name("xl/sharedStrings.xml").unwrap(),
        &mut part,
    )
    .unwrap();
    assert!(part.contains("<si><t>boldx</t></si>"), "{part}");
    assert!(!part.contains("rPr") && !part.contains("zz"), "{part}");
}

/// The count a warning made with `warning` reports, whatever it is, or 0 when there is none; the
/// warning is found by its text around the count.
fn warned_count(warnings: &[String], warning: fn(u64) -> String) -> u64 {
    const MARKER: u64 = 987_654_321;
    let template = warning(MARKER);
    let (before, after) = template.split_once(&MARKER.to_string()).expect("the warning shows its count");
    let mut found = warnings.iter().filter_map(|w| w.strip_prefix(before)?.strip_suffix(after));
    let count = found.next().map_or(0, |n| n.parse().unwrap_or_else(|_| panic!("a count in {n:?}")));
    assert!(found.next().is_none(), "one such warning: {warnings:?}");
    count
}

#[test]
fn a_warned_count_is_read_whatever_its_size() {
    assert_eq!(warned_count(&[repeat_warning(9)], repeat_warning), 9);
    assert_eq!(warned_count(&[repeat_warning(1), elsewhere_warning(12_345)], elsewhere_warning), 12_345);
    assert_eq!(warned_count(&[elsewhere_warning(3)], repeat_warning), 0);
}

#[test]
fn every_short_sequence_of_cells_counts_repeats_and_misplaced_cells_as_the_plain_rule_says() {
    // Every sequence of up to three cells over columns A and B and rows 1 and 2, each a value or
    // a blank cell (which stores nothing, and removes a stored value), split across one, two or
    // three row elements, each for row 1 or 2. The counts must equal a plain reading of the rule:
    // a cell of its element's own row at a column held already by that element, or by the
    // elements for the same row right before it, repeats it; otherwise a cell not past the
    // furthest one read repeats it when it is that one or a cell is stored there; a cell that
    // doesn't repeat is misplaced when it names another row or isn't to the right of the rightmost
    // cell read in its element.
    type Cell = (u32, u32, bool);
    fn reference(elements: &[(u32, Vec<Cell>)]) -> (u64, u64) {
        let (mut repeated, mut elsewhere) = (0, 0);
        let mut furthest: Option<(u32, u32)> = None;
        let mut stored = std::collections::BTreeSet::new();
        let mut held_cols = std::collections::BTreeSet::new();
        let mut previous_row = None;
        for (r, cells) in elements {
            if previous_row != Some(*r) {
                held_cols.clear();
                previous_row = Some(*r);
            }
            let mut rightmost: Option<u32> = None;
            for &(row, col, value) in cells {
                let pos = (row, col);
                let past = furthest.is_none_or(|f| pos > f);
                let held = row == *r && !held_cols.insert(col);
                if held || !past && (furthest == Some(pos) || stored.contains(&pos)) {
                    repeated += 1;
                } else if row != *r || rightmost.is_some_and(|m| col <= m) {
                    elsewhere += 1;
                }
                if past {
                    furthest = Some(pos);
                }
                rightmost = rightmost.max(Some(col));
                if value {
                    stored.insert(pos);
                } else {
                    stored.remove(&pos);
                }
            }
        }
        (repeated, elsewhere)
    }
    let options: Vec<Cell> = (1..=2).flat_map(|row| (0..2).flat_map(move |col| [(row, col, true), (row, col, false)])).collect();
    let mut sequences: Vec<Vec<Cell>> = vec![Vec::new()];
    let (mut checked, mut repeats_seen, mut uncounted_after_other_row) = (0, 0, 0);
    for _ in 0..3 {
        sequences = sequences.iter().flat_map(|s| options.iter().map(move |o| [s.as_slice(), &[*o]].concat())).collect();
        for cells in &sequences {
            // Every way to cut the cells into one to three non-empty runs, each with a row number.
            let n = cells.len();
            let mut cuts: Vec<Vec<usize>> = vec![vec![]];
            cuts.extend((1..n).map(|cut| vec![cut]));
            cuts.extend((1..n).flat_map(|first| (first + 1..n).map(move |second| vec![first, second])));
            for cut in cuts {
                let bounds: Vec<usize> = [0].into_iter().chain(cut.iter().copied()).chain([n]).collect();
                let runs: Vec<&[Cell]> = bounds.windows(2).map(|w| &cells[w[0]..w[1]]).collect();
                for numbers in 0..(1u32 << runs.len()) {
                    let elements: Vec<(u32, Vec<Cell>)> = runs.iter().enumerate().map(|(i, run)| (1 + ((numbers >> i) & 1), run.to_vec())).collect();
                    let mut body = String::from("<sheetData>");
                    for (r, cells) in &elements {
                        body.push_str(&format!("<row r=\"{r}\">"));
                        for &(row, col, value) in cells {
                            let at = format!("{}{row}", col_to_letters(col));
                            body.push_str(&if value { format!("<c r=\"{at}\"><v>1</v></c>") } else { format!("<c r=\"{at}\"/>") });
                        }
                        body.push_str("</row>");
                    }
                    body.push_str("</sheetData>");
                    let (_, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
                    let counted = (warned_count(&rep.warnings, repeat_warning), warned_count(&rep.warnings, elsewhere_warning));
                    let expected = reference(&elements);
                    assert_eq!(counted, expected, "{body}: {:?}", rep.warnings);
                    checked += 1;
                    repeats_seen += u64::from(expected.0 > 0);
                    // Row 1, row 2, row 1 with a blank cell of the outer row repeated across: the case
                    // left uncounted.
                    let blank_repeated_across = elements.len() == 3
                        && elements[0].0 == elements[2].0
                        && elements[1].0 != elements[0].0
                        && match (elements[0].1.as_slice(), elements[2].1.as_slice()) {
                            ([(row, col, false)], [(last_row, last_col, false)]) => (row, col) == (last_row, last_col) && *row == elements[0].0,
                            _ => false,
                        };
                    if blank_repeated_across && expected == (0, 0) {
                        uncounted_after_other_row += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 8 * 2 + 64 * 6 + 512 * 18);
    assert!(repeats_seen > 1000 && uncounted_after_other_row > 0, "{repeats_seen} {uncounted_after_other_row}");
}
