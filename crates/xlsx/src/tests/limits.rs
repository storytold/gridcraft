//! Excel's limits as the bound of legitimate input, and the schema as the list of what a streamed
//! element may hold. A worksheet row of exactly the grid's width, with or without its extension
//! list, is read whole with nothing dropped; one cell more is dropped with a warning that says so. A
//! cell that repeats an address already read is dropped too, so nothing read is ever replaced.
//! Cell text and shared-string runs past Excel's length are kept whole, since cutting them would
//! lose the user's data at the next save, and warned about. Elements the schema does not know are
//! tolerated a few times and then dropped, never crowd out an element the schema allows, and are
//! named in a warning, since nothing reads them.

use gridcraft_core::{CellRef, MAX_CELL_CHARS, MAX_COLS, Value, col_to_letters};

use super::minimal;
use crate::read_xlsx;
use crate::xml::MAX_UNLISTED_CHILD_NAMES;

const SST_RELS: &str = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#;
const EXT_LST: &str = r#"<extLst><ext uri="{00000000-0000-0000-0000-000000000000}"/></extLst>"#;
const A1_IS_SHARED_STRING_0: &str = r#"<sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData>"#;

fn row_warning(n: u64) -> String {
    format!(
        "sheet \"Data\": dropped {n} element(s), counting what they held, that a row may not hold (more than {MAX_COLS} cells, a second extension list, unknown elements, or more elements than a part may hold)"
    )
}
fn string_warning(n: u64) -> String {
    format!(
        "shared strings: dropped {n} element(s), counting what they held, that a string may not hold (a second phoneticPr, unknown elements, or more elements than a part may hold)"
    )
}
fn repeat_warning(n: u64) -> String {
    format!("sheet \"Data\": dropped {n} cell(s), or a row's properties, where a cell or row was already read; the first one read is kept")
}
fn elsewhere_warning(n: u64) -> String {
    format!("sheet \"Data\": {n} cell(s) sat out of column order, or in another row's element, and were placed where their addresses say")
}
fn unread_row_warning(n: u64) -> String {
    format!("sheet \"Data\": {n} row element(s) GridCraft does not read (extension lists or unknown elements) were left out and will not be saved")
}
const LONG_TEXT: &str =
    "sheet \"Data\": 1 cell text(s) longer than Excel's 32767 characters are saved as they are; Excel may refuse or repair the saved file";

/// One row: `unknown` invented elements first, then `cells` cells from A1 on, then `ext_lsts`
/// extension lists, then `extra` cells addressed A2 with the value 999. Such a cell is inside the
/// grid and repeats no address already read, so neither the grid check nor the repeat check
/// catches it: only the row's cell limit can keep it out.
fn row_of(unknown: usize, cells: u32, ext_lsts: usize, extra: usize) -> String {
    let mut body = String::from("<sheetData><row r=\"1\">");
    for i in 0..unknown {
        body.push_str(&format!("<unknown{i}/>"));
    }
    for col in 0..cells {
        body.push_str(&format!("<c r=\"{}1\"><v>{}</v></c>", col_to_letters(col), col + 1));
    }
    body.push_str(&EXT_LST.repeat(ext_lsts));
    body.push_str(&"<c r=\"A2\"><v>999</v></c>".repeat(extra));
    body.push_str("</row></sheetData>");
    body
}

fn a1(wb: &gridcraft_model::Workbook) -> Value {
    wb.sheet(0).unwrap().value(CellRef::parse("A1").unwrap())
}

#[test]
fn a_row_as_wide_as_the_grid_is_read_whole_and_one_cell_more_is_dropped_with_a_warning() {
    // The full width, with and without the extLst.
    for ext_lsts in [0, 1] {
        let (wb, rep) = read_xlsx(&minimal(&row_of(0, MAX_COLS, ext_lsts, 0), &[], "", "")).unwrap();
        let s = wb.sheet(0).unwrap();
        assert_eq!(a1(&wb), Value::Number(1.0));
        assert_eq!(s.value(CellRef::parse("XFD1").unwrap()), Value::Number(MAX_COLS as f64));
        assert_eq!(s.cells.len(), MAX_COLS as usize);
        assert!(!rep.warnings.iter().any(|w| w.contains("dropped")), "extLst {ext_lsts}: {:?}", rep.warnings);
    }
    // One cell more than the width, with and without the extLst: dropped, A1 untouched, one
    // warning.
    for ext_lsts in [0, 1] {
        let (wb, rep) = read_xlsx(&minimal(&row_of(0, MAX_COLS, ext_lsts, 1), &[], "", "")).unwrap();
        let s = wb.sheet(0).unwrap();
        assert_eq!(value(&wb, "A2"), Value::Empty, "extLst {ext_lsts}: the cell past the row's width was kept");
        assert_eq!(s.value(CellRef::parse("XFD1").unwrap()), Value::Number(MAX_COLS as f64));
        assert_eq!(s.cells.len(), MAX_COLS as usize);
        assert!(rep.warnings.contains(&row_warning(2)), "extLst {ext_lsts}: {:?}", rep.warnings);
    }
    // A second extLst is one more than the schema allows: dropped with the same warning.
    let (wb, rep) = read_xlsx(&minimal(&row_of(0, 3, 2, 0), &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().cells.len(), 3);
    assert!(rep.warnings.contains(&row_warning(2)), "{:?}", rep.warnings);
}

#[test]
fn unknown_elements_in_a_row_are_tolerated_a_few_times_and_never_crowd_out_cells_or_the_extension_list() {
    // Exactly the tolerated number of unknown elements: nothing dropped, and the full row of cells
    // plus its extLst behind them are read whole.
    let (wb, rep) = read_xlsx(&minimal(&row_of(MAX_UNLISTED_CHILD_NAMES, MAX_COLS, 1, 0), &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().cells.len(), MAX_COLS as usize);
    assert!(!rep.warnings.iter().any(|w| w.contains("dropped")), "{:?}", rep.warnings);
    // Many more unknown elements: the extra ones are dropped and counted, the cells and the extLst
    // behind them are still read whole, and the cell past the width is still dropped.
    let (wb, rep) = read_xlsx(&minimal(&row_of(1000, MAX_COLS, 1, 1), &[], "", "")).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.cells.len(), MAX_COLS as usize);
    assert_eq!(value(&wb, "A2"), Value::Empty);
    assert_eq!(s.value(CellRef::parse("XFD1").unwrap()), Value::Number(MAX_COLS as f64));
    assert!(rep.warnings.contains(&row_warning(1000 - MAX_UNLISTED_CHILD_NAMES as u64 + 2)), "{:?}", rep.warnings);
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
    assert!(!rep.warnings.iter().any(|w| w.contains("dropped") || w.contains("longer than Excel's")), "{:?}", rep.warnings);
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
    // fills it), so it isn't saved and isn't counted.
    let in_array = r#"<sheetData><row r="1"><c r="A1"><f t="array" ref="A1:A2">1</f><v>1</v></c></row><row r="2"><c r="A2" t="s"><v>0</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(in_array, &[("xl/sharedStrings.xml", &plain(most + 1))], SST_RELS, "")).unwrap();
    assert_eq!(value(&wb, "A2"), Value::Empty);
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    // A long shared string no cell uses isn't saved, so it is not warned about; the cells using
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
    assert!(
        rep.warnings.contains(
            &"sheet \"Data\": 2 cell text(s) longer than Excel's 32767 characters are saved as they are; Excel may refuse or repair the saved file"
                .to_string()
        ),
        "{:?}",
        rep.warnings
    );
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
    // that parse again, so the cached text isn't what is saved and isn't counted.
    let formula = format!(r#"<sheetData><row r="1"><c r="A1" t="str"><f>REPT("a",40000)</f><v>{}</v></c></row></sheetData>"#, "a".repeat(most + 1));
    let (wb, rep) = read_xlsx(&minimal(&formula, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Text("a".repeat(most + 1).into()));
    assert!(!rep.warnings.iter().any(|w| w.contains("longer than Excel's")), "{:?}", rep.warnings);
    // A formula GridCraft can't parse isn't calculated, so its cached text is what is saved, and
    // it is counted.
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
}

fn value(wb: &gridcraft_model::Workbook, at: &str) -> Value {
    wb.sheet(0).unwrap().value(CellRef::parse(at).unwrap())
}

#[test]
fn a_cell_repeating_an_address_already_read_is_dropped_and_the_first_kept() {
    // Excel writes each cell once, in order, in its own row. A cell that repeats an address would
    // replace what was read there, so it is dropped and counted, and so are a repeated row's
    // properties where the first one had some. A cell out of column order, or in another row's element, lands
    // where its address says as long as nothing was read there, and an unaddressed cell takes
    // the column after the one before it, in its own row.
    let body = r#"<sheetData><row r="1" ht="20" customHeight="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c><c r="B1"><v>2</v></c><c r="A1"><v>999</v></c><c r="D7"><v>7</v></c><c><v>8</v></c></row><row r="2"><c r="A2"><v>20</v></c><c r="A1"><v>998</v></c></row><row r="1" ht="30" customHeight="1"><c r="A1"><v>997</v></c><c r="G1"><v>9</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    for (at, n) in [("A1", 1.0), ("B1", 2.0), ("C1", 3.0), ("D7", 7.0), ("E1", 8.0), ("A2", 20.0), ("G1", 9.0)] {
        assert_eq!(value(&wb, at), Value::Number(n), "{at}");
    }
    assert!(rep.warnings.contains(&repeat_warning(4)), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&elsewhere_warning(2)), "B1 out of order and D7 in row 1's element are named: {:?}", rep.warnings);
    let first = |body: &str| read_xlsx(&minimal(body, &[], "", "")).unwrap().0.sheet(0).unwrap().rows.get(&0).cloned();
    assert_eq!(
        wb.sheet(0).unwrap().rows.get(&0).cloned(),
        first(r#"<sheetData><row r="1" ht="20" customHeight="1"/></sheetData>"#),
        "the first row's height is kept"
    );
    // Properties a repeated row brings where the first had none replace nothing: kept, silently.
    let body = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row><row r="1" ht="30" customHeight="1"><c r="B1"><v>2</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().rows.get(&0).cloned(), first(r#"<sheetData><row r="1" ht="30" customHeight="1"/></sheetData>"#));
    assert_eq!(value(&wb, "B1"), Value::Number(2.0));
    assert!(!rep.warnings.iter().any(|w| w.contains("already read")), "{:?}", rep.warnings);
    // An unaddressed cell after a dropped repeat takes the column after the repeat, as on main.
    let body = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c><c r="A1"><v>999</v></c><c><v>8</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!((a1(&wb), value(&wb, "B1"), value(&wb, "C1")), (Value::Number(1.0), Value::Number(8.0), Value::Number(3.0)));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    // The same address twice in a row, the second straight after the first.
    let body = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="A1"><v>999</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Number(1.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    // A full row and then a cell in the next row's element naming A1: the 16,385th cell for row 1.
    let mut body = row_of(0, MAX_COLS, 0, 0);
    body = body.replace("</row></sheetData>", r#"</row><row r="2"><c r="A1"><v>999</v></c></row></sheetData>"#);
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Number(1.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
    // A sparse sheet in order says nothing.
    let body = r#"<sheetData><row r="1"><c r="A1"/><c r="C1"><v>3</v></c></row><row r="3"><c r="B3"><v>1</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(value(&wb, "B3"), Value::Number(1.0));
    assert!(!rep.warnings.iter().any(|w| w.contains("already read") || w.contains("out of column order")), "{:?}", rep.warnings);
    // A repeat of the furthest cell read, after a cell out of order: the shortcut past the furthest
    // cell must not let it through.
    let body =
        r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><c r="C1"><v>3</v></c><c r="B1"><v>2</v></c><c r="C1"><v>999</v></c></row></sheetData>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert_eq!(value(&wb, "C1"), Value::Number(3.0));
    assert!(rep.warnings.contains(&repeat_warning(1)), "{:?}", rep.warnings);
}

#[test]
fn a_row_past_the_element_cap_loses_only_what_it_holds_past_it() {
    // A row of cells each holding many children (which the row's own allowance doesn't limit)
    // fills the element cap: what it holds past the cap is dropped and counted, and the rows and
    // sheet content after it are still read.
    let mut body = String::from(r#"<sheetData><row r="1"><c r="A1"><v>5</v></c></row><row r="2">"#);
    let filler = "<x/>".repeat(62);
    for col in 0..MAX_COLS {
        body.push_str(&format!(r#"<c r="{}2"><v>1</v>{filler}</c>"#, col_to_letters(col)));
    }
    body.push_str(r#"</row><row r="3"><c r="A3"><v>7</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A5:B5"/></mergeCells>"#);
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Number(5.0));
    assert_eq!(value(&wb, "A2"), Value::Number(1.0), "the row keeps what it held before the cap");
    assert_eq!(value(&wb, "A3"), Value::Number(7.0), "the row after it is read");
    assert_eq!(wb.sheet(0).unwrap().merges.len(), 1, "the sheet content after the rows is read");
    assert!(!rep.warnings.iter().any(|w| w.contains("damaged")), "{:?}", rep.warnings);
    let prefix = "sheet \"Data\": dropped ";
    assert!(rep.warnings.iter().any(|w| w.starts_with(prefix) && w.contains("more elements than a part may hold")), "{:?}", rep.warnings);
}

#[test]
fn what_was_dropped_before_a_part_turned_out_damaged_is_still_said() {
    // A row holding two extension lists, then a part that ends inside a comment: the second list
    // was dropped before the error, and the warning says so beside the "damaged" one.
    let body = format!(r#"<sheetData><row r="1"><c r="A1"><v>1</v></c>{EXT_LST}{EXT_LST}</row><row r="2"><c r="A2"><v>2</v></c></row><!-- "#);
    let (_, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert!(rep.warnings.iter().any(|w| w.starts_with("sheet \"Data\" is damaged")), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&row_warning(2)), "{:?}", rep.warnings);
}

#[test]
fn row_elements_and_string_elements_nothing_reads_are_named() {
    // A row's extension list and the unknown elements a row or a string may hold are kept out of
    // the sheet by the reader, which doesn't read them: a warning says so, with the count.
    let (_, rep) = read_xlsx(&minimal(&row_of(2, 3, 1, 0), &[], "", "")).unwrap();
    assert!(rep.warnings.contains(&unread_row_warning(3)), "{:?}", rep.warnings);
    let (_, rep) = read_sst(sst_with(2, Some("hello"), 0, 0, 0));
    assert!(
        rep.warnings
            .contains(&"shared strings: 2 element(s) GridCraft does not read (unknown elements) were left out and will not be saved".to_string()),
        "{:?}",
        rep.warnings
    );
    // A plain row says nothing.
    let (_, rep) = read_xlsx(&minimal(&row_of(0, 3, 0, 0), &[], "", "")).unwrap();
    assert!(!rep.warnings.iter().any(|w| w.contains("does not read")), "{:?}", rep.warnings);
}

#[test]
fn a_cell_crossing_the_element_cap_is_dropped_with_what_it_holds_and_the_next_cell_is_read() {
    // One cell holding more elements than the cap: it is dropped with everything it holds, all of
    // it counted, and the count goes back to where it began, so the small cell after it is read.
    let mut body = String::from(r#"<sheetData><row r="2"><c r="A2"><v>1</v>"#);
    body.push_str(&"<x/>".repeat(1_000_001));
    body.push_str(r#"</c><c r="B2"><v>2</v></c></row></sheetData>"#);
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(value(&wb, "A2"), Value::Empty);
    assert_eq!(value(&wb, "B2"), Value::Number(2.0), "the cell after it is read");
    let count = rep
        .warnings
        .iter()
        .find_map(|w| w.strip_prefix("sheet \"Data\": dropped ").and_then(|r| r.split(' ').next()).and_then(|n| n.parse::<u64>().ok()))
        .unwrap_or(0);
    assert!(count > 1_000_000, "everything the cell held is counted: {:?}", rep.warnings);
}

#[test]
fn an_inline_string_crossing_the_element_cap_is_dropped_with_its_cell() {
    // The string's runs fill the cap, so the string is dropped at the cap; its cell goes with it
    // rather than falling back to the cell's own value text, and the cell after it is read.
    let mut body = String::from(r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><f>"formula"</f><v>fallback</v><is>"#);
    body.push_str(&"<t>a</t>".repeat(1_000_001));
    body.push_str(r#"</is></c><c r="B1"><v>2</v></c></row></sheetData>"#);
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(a1(&wb), Value::Empty, "{:?}", rep.warnings);
    assert!(wb.sheet(0).unwrap().cells.get(CellRef::new(0, 0)).is_none(), "no formula or style is kept for the cell");
    assert_eq!(value(&wb, "B1"), Value::Number(2.0), "the cell after it is read");
    let count = rep
        .warnings
        .iter()
        .find_map(|w| w.strip_prefix("sheet \"Data\": dropped ").and_then(|r| r.split(' ').next()).and_then(|n| n.parse::<u64>().ok()))
        .unwrap_or(0);
    assert!(count > 1_000_000, "everything the cell held is counted: {:?}", rep.warnings);
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
    assert!(text.len() > 400_000 && text.len() < 600_000, "{}", text.len());
    assert!(
        rep.warnings.iter().any(|w| w.starts_with("shared strings: 1 string(s) held more elements than a part may hold and were cut there")),
        "{:?}",
        rep.warnings
    );
    assert!(rep.warnings.contains(&LONG_TEXT.to_string()), "{:?}", rep.warnings);
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
    assert!(!rep.warnings.iter().any(|w| w.contains("cut there")), "{:?}", rep.warnings);
}

#[test]
fn a_child_dropped_for_its_allowance_gives_back_what_it_held() {
    // A second extension list holding a million elements is dropped for being one too many; the
    // count goes back to where it began, so the cell after it is read.
    let mut body = String::from(r#"<sheetData><row r="1"><c r="A1"><v>1</v></c><extLst/><extLst>"#);
    body.push_str(&"<ext/>".repeat(999_999));
    body.push_str(r#"</extLst><c r="B1"><v>2</v></c></row></sheetData>"#);
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    assert_eq!(value(&wb, "B1"), Value::Number(2.0), "{:?}", rep.warnings);
    assert!(rep.warnings.contains(&row_warning(1_000_000)), "{:?}", rep.warnings);
}
