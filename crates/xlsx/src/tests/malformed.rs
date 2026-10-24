//! Hostile and broken input: must return an error or warnings, never panic.

use std::io::Write;

use super::{make_zip, minimal};
use crate::{IoError, read_xlsx, write_xlsx};

#[test]
fn garbage_and_truncation() {
    assert!(read_xlsx(b"").is_err());
    assert!(read_xlsx(b"hello world").is_err());
    assert!(read_xlsx(b"PK\x03\x04garbage").is_err());
    let good = write_xlsx(&gridcraft_model::Workbook::new()).unwrap();
    for n in [10, 100, good.len() / 2, good.len() - 10] {
        let _ = read_xlsx(&good[..n]);
    }
    // Flip bytes throughout a valid file.
    for i in (0..good.len()).step_by(97) {
        let mut b = good.clone();
        b[i] ^= 0xFF;
        let _ = read_xlsx(&b);
    }
}

#[test]
fn missing_parts() {
    // No workbook at all.
    let z = make_zip(&[("[Content_Types].xml", super::CT.as_bytes())]);
    assert!(matches!(read_xlsx(&z), Err(IoError::Format(_))));
    // Workbook pointing at a missing sheet part.
    let wb = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="r"><sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/nope.xml"/></Relationships>"#;
    let z = make_zip(&[
        ("_rels/.rels", super::ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", wb.as_bytes()),
        ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
    ]);
    let (wb, rep) = read_xlsx(&z).unwrap();
    assert_eq!(wb.sheets.len(), 1);
    assert!(!rep.warnings.is_empty());
    // No root rels: falls back to xl/workbook.xml; no sheets → one empty sheet.
    let wb2 = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#;
    let z = make_zip(&[("xl/workbook.xml", wb2.as_bytes())]);
    let (wb, rep) = read_xlsx(&z).unwrap();
    assert_eq!(wb.sheets.len(), 1);
    assert!(rep.warnings.iter().any(|w| w.contains("no worksheets")));
}

#[test]
fn huge_coordinates_and_dimensions() {
    let body = r#"<dimension ref="A1:XFD1048576"/><cols><col min="1" max="4000000000" width="1e308"/><col min="0" max="0"/></cols>
      <sheetData><row r="4294967295"><c r="A1"><v>1</v></c></row><row r="2000000"><c r="A2000000"><v>1</v></c></row>
      <row r="3"><c r="ZZZZ3"><v>1</v></c><c r="XFE3"><v>1</v></c><c r="XFD3"><v>7</v></c><c r="B3" s="99999"><v>nan</v></c><c r="C3" t="s"><v>-1</v></c><c r="D3" t="d"><v>9999-99-99</v></c></row>
      <row r="0"/></sheetData>
      <mergeCells><mergeCell ref="A1:XFD1048576"/><mergeCell ref="bogus"/></mergeCells>
      <hyperlinks><hyperlink ref="A1:XFD1048576" location="x"/></hyperlinks>
      <sheetViews><sheetView zoomScale="99999"><pane xSplit="1e9" ySplit="-5" state="frozen"/></sheetView></sheetViews>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    let s = wb.sheet(0).unwrap();
    assert_eq!(s.value(gridcraft_core::CellRef::parse("XFD3").unwrap()), gridcraft_core::Value::Number(7.0));
    assert!(s.cells.len() <= 4);
    assert!(s.hyperlinks.len() <= 10_000);
    assert_eq!(s.zoom, 400);
    assert!(rep.warnings.iter().any(|w| w.contains("dropped")));
    // Still writable.
    let out = write_xlsx(&wb).unwrap();
    read_xlsx(&out).unwrap();
}

#[test]
fn deeply_nested_and_broken_xml() {
    let mut deep = String::from("<sheetData/><extLst>");
    for _ in 0..5000 {
        deep.push_str("<x>");
    }
    let (wb, rep) = read_xlsx(&minimal(&deep, &[], "", "")).unwrap();
    assert_eq!(wb.sheets.len(), 1);
    assert!(rep.warnings.iter().any(|w| w.contains("could not be read") || w.contains("damaged")));
    // Truncated sheet XML keeps earlier rows.
    let (wb, _) = read_xlsx(&minimal(r#"<sheetData><row r="1"><c r="A1"><v>5</v></c></row><row r="2"><c r="A2"><v"#, &[], "", "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().value(gridcraft_core::CellRef::new(0, 0)), gridcraft_core::Value::Number(5.0));
    // Broken optional parts are warnings.
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>"#;
    let (_, rep) = read_xlsx(&minimal(
        "<sheetData/>",
        &[("xl/styles.xml", "<styleSheet><<<"), ("xl/theme/theme1.xml", "\u{0}\u{1}"), ("xl/sharedStrings.xml", "<sst><si><t>a</t></si><si><t>")],
        rels,
        "",
    ))
    .unwrap();
    assert!(!rep.warnings.is_empty());
    // Hostile drawing / table / comment references.
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../../../../etc/passwd"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/table" Target="../tables/t.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="../c.xml"/></Relationships>"#;
    let (_, rep) = read_xlsx(&minimal(
        r#"<sheetData/><drawing r:id="rId1"/><tableParts><tablePart r:id="rId2"/><tablePart r:id="rId9"/></tableParts>"#,
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", srels),
            ("xl/tables/t.xml", "<table ref='garbage'/>"),
            ("xl/c.xml", "<comments><commentList><comment ref='ZZZZZ99999999'/></commentList></comments>"),
        ],
        "",
        "",
    ))
    .unwrap();
    assert!(!rep.warnings.is_empty());
}

#[test]
fn zip_bomb_is_rejected() {
    // A part that inflates beyond the per-part cap: 600 MB of zeros compresses very well, but
    // building it takes a while, so test the cap via a declared size instead using a smaller
    // stand-in limit check on the reader path.
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
    z.start_file("xl/workbook.xml", opts).unwrap();
    let chunk = vec![b' '; 1 << 20];
    for _ in 0..520 {
        z.write_all(&chunk).unwrap();
    }
    let bytes = z.finish().unwrap().into_inner();
    assert!(bytes.len() < 10 << 20);
    assert!(matches!(read_xlsx(&bytes), Err(IoError::TooLarge(_))));
}

/// Threaded comments, persons and note VML from a hostile file: odd links and values are dropped,
/// broken parts are skipped, nothing panics.
#[test]
fn hostile_threaded_comments_persons_and_note_vml() {
    use gridcraft_core::CellRef;
    use gridcraft_model::{Comment, Workbook};
    let mut wb = Workbook::new();
    let s = std::sync::Arc::make_mut(&mut wb.sheets[0]);
    let c = |threaded| Comment {
        author: "A".into(),
        text: "t".into(),
        replies: vec![],
        threaded,
        resolved: false,
        visible: false,
        dates: vec![],
        note_box: None,
    };
    s.comments.insert(CellRef::new(0, 0), c(true));
    s.comments.insert(CellRef::new(1, 1), c(false));
    let good = write_xlsx(&wb).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(good)).unwrap();
    let mut parts: Vec<(String, Vec<u8>)> = vec![];
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut d = vec![];
        std::io::Read::read_to_end(&mut f, &mut d).unwrap();
        parts.push((f.name().to_string(), d));
    }
    let ns = r#"xmlns="http://schemas.microsoft.com/office/spreadsheetml/2018/threadedcomments""#;
    let deep = "<v:group>".repeat(300) + &"</v:group>".repeat(300);
    let threaded = format!(
        r#"<ThreadedComments {ns}><threadedComment ref="A1" dT="not a date" personId="{{nobody}}" id="{{T}}"><text>root</text></threadedComment><threadedComment ref="A1" personId="x" id="{{R}}" parentId="{{missing}}"><text>orphan</text></threadedComment><threadedComment ref="A1" personId="x" id="{{RR}}" parentId="{{R}}"><text>reply to orphan</text></threadedComment><threadedComment ref="ZZZZZ99999999" personId="x" id="{{B}}"><text>off sheet</text></threadedComment><threadedComment personId="x" id="{{N}}"><text>no ref</text></threadedComment></ThreadedComments>"#
    );
    let vml = format!(
        r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:x="urn:schemas-microsoft-com:office:excel"><v:shape><x:ClientData ObjectType="Note"><x:Row>4294967295</x:Row><x:Column>-1</x:Column><x:Visible/></x:ClientData></v:shape><v:shape><x:ClientData ObjectType="Note"><x:Row>1</x:Row><x:Column>1</x:Column><x:Visible/></x:ClientData></v:shape>{deep}</xml>"#
    );
    for (name, bytes) in &mut parts {
        if name.starts_with("xl/threadedComments/") {
            *bytes = threaded.clone().into_bytes();
        } else if name.starts_with("xl/drawings/vmlDrawing") {
            *bytes = vml.clone().into_bytes();
        } else if name == "xl/persons/person.xml" {
            *bytes = b"<personList><person".to_vec();
        }
    }
    let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let (back, _) = read_xlsx(&super::make_zip(&refs)).unwrap();
    let sheet = &back.sheets[0];
    let a1 = &sheet.comments[&CellRef::new(0, 0)];
    assert!(a1.threaded && a1.replies.is_empty() && a1.author.is_empty());
    assert_eq!(a1.text, "root");
    assert_eq!(sheet.comments.len(), 2);
    // Too deep for the parser: the VML is skipped, so the note keeps its default.
    assert!(!sheet.comments[&CellRef::new(1, 1)].visible);
    // A VML that parses shows the note.
    for (name, bytes) in &mut parts {
        if name.starts_with("xl/drawings/vmlDrawing") {
            *bytes = vml.replace(&deep, "").into_bytes();
        }
    }
    let refs: Vec<(&str, &[u8])> = parts.iter().map(|(n, d)| (n.as_str(), d.as_slice())).collect();
    let (back, _) = read_xlsx(&super::make_zip(&refs)).unwrap();
    assert!(back.sheets[0].comments[&CellRef::new(1, 1)].visible);
}

/// Allow Edit Ranges from a hostile file: bad references are dropped and a password that can't be
/// read keeps the range locked instead of opening it.
#[test]
fn hostile_protected_ranges_stay_locked() {
    let body = r#"<sheetData/><protectedRanges><protectedRange name="bad ref" sqref="ZZZZZ1 A0"/><protectedRange name="hex" sqref="A1" password="XYZ"/><protectedRange name="spin" sqref="B1" algorithmName="SHA-512" hashValue="AAAA" saltValue="AAAA" spinCount="4294967295"/><protectedRange name="half" sqref="C1" hashValue="AAAA"/><protectedRange name="md5" sqref="D1" algorithmName="MD5" hashValue="AAAA" saltValue="AAAA" spinCount="1"/></protectedRanges>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    let ranges = &wb.sheets[0].edit_ranges;
    assert_eq!(ranges.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["hex", "spin", "half", "md5"]);
    for r in ranges {
        let h = r.password.as_ref().unwrap();
        assert!(!matches!(h.verify(""), Ok(true)), "{}", r.title);
        assert!(!matches!(h.verify("password"), Ok(true)), "{}", r.title);
    }
}

#[test]
fn an_overlong_number_format_code_does_not_stop_the_open() {
    // A format code far over the 255-character cap: the file still opens, the format code is kept as
    // stored (display falls back to General) and the cell keeps its value.
    let code = format!("# ?/{}", "0".repeat(65_536));
    let styles = format!(
        r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="{code}"/></numFmts>
      <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="164"/></cellXfs></styleSheet>"#
    );
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let body = r#"<sheetData><row r="1"><c r="A1" s="1"><v>0.5</v></c></row></sheetData>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[("xl/styles.xml", &styles)], rels, "")).unwrap();
    let s = wb.sheet(0).unwrap();
    let cell = s.cell(gridcraft_core::CellRef::parse("A1").unwrap()).unwrap();
    assert_eq!(s.value(gridcraft_core::CellRef::parse("A1").unwrap()), gridcraft_core::Value::Number(0.5));
    assert_eq!(wb.styles.get(cell.style).num_fmt.as_str(), code);
}

#[test]
fn hyperlinks_past_the_cell_cap_are_reported() {
    // A styles part, so the only possible warning is the hyperlink one.
    let parts = [("xl/styles.xml", r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"/>"#)];
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let body = r#"<sheetData/><hyperlinks><hyperlink ref="A1:A12000" location="x"/></hyperlinks>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &parts, rels, "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().hyperlinks.len(), 10_000);
    assert!(rep.warnings.iter().any(|w| w.contains("hyperlinks past the first 10000 cells were dropped")), "{:?}", rep.warnings);
    // A small list reads in full with no warning.
    let body = r#"<sheetData/><hyperlinks><hyperlink ref="A1:A3" location="x"/></hyperlinks>"#;
    let (wb, rep) = read_xlsx(&minimal(body, &parts, rels, "")).unwrap();
    assert_eq!(wb.sheet(0).unwrap().hyperlinks.len(), 3);
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
}

/// Formula text built to overflow a recursive parser or tree walk (a million `%`, a 200,000-term
/// sum): the cells keep their formula text and cached value, nothing overflows the stack.
#[test]
fn hostile_formula_text_does_not_overflow_the_stack() {
    let percents = format!("1{}", "%".repeat(1_000_000));
    let chain = format!("1{}", "+1".repeat(100_000));
    let body = format!(r#"<sheetData><row r="1"><c r="A1"><f>{percents}</f><v>3</v></c><c r="B1"><f>{chain}</f><v>4</v></c></row></sheetData>"#);
    let names = format!(r#"<definedNames><definedName name="Deep">{percents}</definedName></definedNames>"#);
    let bytes = minimal(&body, &[], "", &names);
    let handle = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || {
            let (wb, _) = read_xlsx(&bytes).expect("readable");
            let s = wb.sheet(0).expect("sheet");
            let a1 = s.cell(gridcraft_core::CellRef::new(0, 0)).and_then(|c| c.formula.clone()).expect("formula kept");
            let b1 = s.cell(gridcraft_core::CellRef::new(0, 1)).and_then(|c| c.formula.clone()).expect("formula kept");
            let kept = (a1.expr.is_none(), b1.expr.is_none(), a1.text.len(), s.value(gridcraft_core::CellRef::new(0, 0)));
            assert_eq!(wb.names.first().map(|n| n.formula.len()), Some(1_000_001));
            let out = write_xlsx(&wb).expect("writable");
            drop(wb);
            (kept, !out.is_empty())
        })
        .expect("spawn");
    let ((a_unparsed, b_unparsed, a_len, a_value), written) = handle.join().expect("no stack overflow");
    assert!(a_unparsed && b_unparsed && written);
    assert_eq!(a_len, 1_000_001);
    assert_eq!(a_value, gridcraft_core::Value::Number(3.0));
}

/// #2173: a hyperlink is copied into every cell it covers, so oversized targets are dropped,
/// ScreenTips are cut to 255 characters and the cell cap holds across `<hyperlinks>` blocks.
#[test]
fn hyperlink_copies_are_bounded() {
    let long = "A".repeat(20_000);
    let tip = "t".repeat(20_000);
    let body = format!(
        r#"<sheetData/><hyperlinks><hyperlink ref="A1:A10000" location="{long}" tooltip="{tip}"/><hyperlink ref="B1:B3" location="Data!C1" tooltip="{tip}"/></hyperlinks>"#
    );
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    let links = &wb.sheets[0].hyperlinks;
    assert_eq!(links.len(), 3);
    assert!(links.values().all(|l| l.target == "Data!C1" && l.tooltip.as_ref().map(|t| t.chars().count()) == Some(255)));
    assert!(rep.warnings.iter().any(|w| w.contains("longer than")), "{:?}", rep.warnings);

    let blocks: String =
        ["A", "B", "C"].iter().map(|c| format!(r#"<hyperlinks><hyperlink ref="{c}1:{c}8000" location="Data!A1"/></hyperlinks>"#)).collect();
    let (wb, _) = read_xlsx(&minimal(&format!("<sheetData/>{blocks}"), &[], "", "")).unwrap();
    assert_eq!(wb.sheets[0].hyperlinks.len(), 10_000);
}

/// #2173: every rule of a conditional-format block copies the block's ranges; the copies are
/// capped per sheet.
#[test]
fn conditional_format_range_copies_are_bounded() {
    let sq = (1..=10_000).map(|r| format!("A{r}")).collect::<Vec<_>>().join(" ");
    let rules = r#"<cfRule type="containsBlanks" priority="1"/>"#.repeat(500);
    let body = format!(r#"<sheetData/><conditionalFormatting sqref="{sq}">{rules}</conditionalFormatting>"#);
    let (wb, rep) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    let cfs = &wb.sheets[0].cond_formats;
    // The first rule plus 100 copies of 10,000 ranges.
    assert_eq!(cfs.len(), 101);
    assert!(cfs.iter().all(|c| c.ranges.len() == 10_000));
    assert!(rep.warnings.iter().any(|w| w.contains("too many ranges")), "{:?}", rep.warnings);
}

/// #2173: cell formats share one copy of each custom number format (even an over-long one, which
/// is kept as stored) and font names are cut to 255 characters.
#[test]
fn style_copies_are_bounded() {
    let code = "0".repeat(50_000);
    let font = "F".repeat(100_000);
    let xfs = r#"<xf numFmtId="164" fontId="0"/>"#.repeat(10_000);
    let styles = format!(
        r#"<styleSheet><numFmts><numFmt numFmtId="164" formatCode="{code}"/><numFmt numFmtId="165" formatCode="0.000&quot;x&quot;"/></numFmts><fonts><font><name val="{font}"/></font></fonts><cellXfs>{xfs}<xf numFmtId="165"/><xf numFmtId="165"/></cellXfs><dxfs><dxf><numFmt numFmtId="166" formatCode="{code}"/></dxf></dxfs></styleSheet>"#
    );
    let st = crate::styles::read_styles(&crate::xml::parse(styles.as_bytes()).unwrap(), &[]);
    assert_eq!(st.xfs.len(), 10_002);
    let first = &st.xfs[0].num_fmt.0;
    assert_eq!(first.len(), 50_000);
    assert!(st.xfs[..10_000].iter().all(|s| std::sync::Arc::ptr_eq(&s.num_fmt.0, first) && s.font.name.chars().count() == 255));
    assert_eq!(st.xfs[10_000].num_fmt.as_str(), "0.000\"x\"");
    assert!(std::sync::Arc::ptr_eq(&st.xfs[10_000].num_fmt.0, &st.xfs[10_001].num_fmt.0));
    assert_eq!(st.dxfs[0].num_fmt.as_str(), code);

    // The same styles part read through the package.
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let sheet = r#"<sheetData><row r="1"><c r="A1" s="1"><v>1</v></c></row></sheetData>"#;
    let (wb, _) = read_xlsx(&minimal(sheet, &[("xl/styles.xml", &styles)], rels, "")).unwrap();
    assert_eq!(wb.sheets.len(), 1);
}

/// #2173: a shared formula's master is copied into every dependent; an over-long master keeps
/// the dependents' cached values instead of copying it into each of them.
#[test]
fn shared_formula_copies_are_bounded() {
    use gridcraft_core::{CellRef, Value};
    let master = format!("SUM({}1)", "1,".repeat(500_000));
    let mut rows = format!(r#"<row r="1"><c r="A1"><f t="shared" si="0" ref="A1:A20000">{master}</f><v>1</v></c></row>"#);
    for r in 2..=20_000 {
        rows.push_str(&format!(r#"<row r="{r}"><c r="A{r}"><f t="shared" si="0"/><v>7</v></c></row>"#));
    }
    let (wb, rep) = read_xlsx(&minimal(&format!("<sheetData>{rows}</sheetData>"), &[], "", "")).unwrap();
    let cells = &wb.sheets[0].cells;
    for r in [1, 9_999, 19_999] {
        let c = cells.get(CellRef::new(r, 0)).unwrap();
        assert!(c.formula.is_none());
        assert_eq!(c.value, Value::Number(7.0));
    }
    assert!(rep.warnings.iter().any(|w| w.contains("shared formula is longer")), "{:?}", rep.warnings);
}

#[test]
fn repeated_column_ranges_have_one_worksheet_work_budget() {
    // 62 * 16,384 = 1,015,808 assignments, while the map holds at most 16,384 columns.
    // This stays small even on the unbounded reader, but exceeds the one-million work cap.
    let col = r#"<col min="1" max="16384" width="10" customWidth="1"/>"#;
    for (first, second) in [(62, 0), (31, 31)] {
        let body = format!("<cols>{}</cols><cols>{}</cols><sheetData/>", col.repeat(first), col.repeat(second));
        assert!(matches!(read_xlsx(&minimal(&body, &[], "", "")), Err(IoError::TooLarge(_))), "blocks: {first}, {second}");
    }
}

#[test]
fn column_work_budget_counts_only_clipped_nondefault_assignments() {
    let full = r#"<col min="1" max="16384" width="10" customWidth="1"/>"#;
    // Default-only records and invalid coordinates do not expand into column assignments.
    let noise = format!(
        "{}{}",
        r#"<col min="1" max="16384" hidden="0" style="0"/>"#.repeat(62),
        r#"<col min="0" max="16384" width="20"/>
        <col min="16385" max="4294967295" width="20"/>
        <col min="5" max="4" width="20"/>
        <col min="bad" max="16384" width="20"/>"#
    );
    // The last declaration clips to 576 columns: 61 * 16,384 + 576 = 1,000,000.
    let cols = format!("{noise}{}<col min=\"15809\" max=\"4294967295\" width=\"20\"/>", full.repeat(61));
    let body = format!("<cols>{cols}</cols><sheetData/>");
    let (wb, _) = read_xlsx(&minimal(&body, &[], "", "")).unwrap();
    let sheet = wb.sheet(0).unwrap();
    assert_eq!(sheet.cols.len(), 16_384);
    assert_eq!(sheet.cols.get(&15_807).unwrap().size, Some(crate::tables::col_width_to_px(10.0)));
    assert_eq!(sheet.cols.get(&15_808).unwrap().size, Some(crate::tables::col_width_to_px(20.0)));
    assert_eq!(sheet.cols.get(&16_383).unwrap().size, Some(crate::tables::col_width_to_px(20.0)));

    let body = format!("<cols>{cols}<col min=\"1\" max=\"1\" hidden=\"1\"/></cols><sheetData/>");
    assert!(matches!(read_xlsx(&minimal(&body, &[], "", "")), Err(IoError::TooLarge(_))));
}

#[test]
fn ordinary_overlapping_column_ranges_keep_last_declaration() {
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
      <cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="2"/></cellXfs></styleSheet>"#;
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let body = r#"<cols><col min="1" max="3" width="10" hidden="1" style="1"/></cols>
      <cols><col min="2" max="4" width="20" hidden="0" style="0"/>
      <col min="5" max="5" hidden="1" style="1"/>
      <col min="1" max="5"/></cols><sheetData/>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[("xl/styles.xml", styles)], rels, "")).unwrap();
    let sheet = wb.sheet(0).unwrap();
    assert_eq!(sheet.cols.len(), 5);
    let first = sheet.cols.get(&0).unwrap();
    assert_eq!(first.size, Some(crate::tables::col_width_to_px(10.0)));
    assert!(first.hidden);
    assert_eq!(wb.styles.get(first.style.unwrap()).num_fmt.as_str(), "0.00");
    for col in 1..4 {
        let info = sheet.cols.get(&col).unwrap();
        assert_eq!(info.size, Some(crate::tables::col_width_to_px(20.0)));
        assert!(!info.hidden);
        assert_eq!(info.style, None);
    }
    let last = sheet.cols.get(&4).unwrap();
    assert_eq!(last.size, None);
    assert!(last.hidden);
    assert_eq!(wb.styles.get(last.style.unwrap()).num_fmt.as_str(), "0.00");
}

/// Show Outline Symbols off (`showOutlineSymbols="0"`) is read and written back.
#[test]
fn outline_symbols_flag_round_trips() {
    let body = r#"<sheetViews><sheetView showOutlineSymbols="0" workbookViewId="0"/></sheetViews><sheetData/>"#;
    let (wb, _) = read_xlsx(&minimal(body, &[], "", "")).unwrap();
    assert!(!wb.sheet(0).unwrap().show_outline_symbols);
    let (back, _) = read_xlsx(&crate::write_xlsx(&wb).unwrap()).unwrap();
    assert!(!back.sheet(0).unwrap().show_outline_symbols);
    // Absent means shown.
    let (wb, _) = read_xlsx(&minimal("<sheetData/>", &[], "", "")).unwrap();
    assert!(wb.sheet(0).unwrap().show_outline_symbols);
}
