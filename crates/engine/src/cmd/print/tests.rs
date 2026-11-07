use serde_json::json;

use super::*;

fn s() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}
fn pdf_of(s: &mut Session, p: Json) -> (Vec<u8>, u64) {
    let r = s.execute("file.exportPdf", p).unwrap();
    let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
    (bytes, r["pages"].as_u64().unwrap())
}
fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn chart_data_table_prints_formatted_values_and_follows_recalculation() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Quarter", "Sales"], ["Q1", 1234], ["Q2", 5678]]})).unwrap();
    s.execute("home.numberFormat", json!({"range": "B2:B3", "format": "#,##0.00"})).unwrap();
    s.execute("insert.chart", json!({"range": "A1:B3", "at": "D2", "type": "column"})).unwrap();
    let (off, pages) = pdf_of(&mut s, json!({"fitToPage": true}));
    assert_eq!(pages, 1);
    let count = |bytes: &[u8], text: &str| String::from_utf8_lossy(bytes).matches(text).count();
    s.execute("chart.set", json!({"dataTable": true})).unwrap();
    let (on, _) = pdf_of(&mut s, json!({"fitToPage": true}));
    assert_eq!(count(&on, "(1,234.00) Tj"), count(&off, "(1,234.00) Tj") + 1);
    assert_eq!(count(&on, "(5,678.00) Tj"), count(&off, "(5,678.00) Tj") + 1);
    s.execute("cell.set", json!({"cell": "B2", "input": "=1000+111"})).unwrap();
    let (updated, _) = pdf_of(&mut s, json!({"fitToPage": true}));
    assert!(!contains(&updated, "(1,234.00) Tj"));
    assert!(count(&updated, "(1,111.00) Tj") >= 2, "cell and chart data table both print the new value");
}

#[test]
fn pdf_export_keeps_safe_links_on_their_printed_pages() {
    let mut s = s();
    s.execute("insert.link", json!({"cell": "A1", "target": "https://example.org/report"})).unwrap();
    s.execute("insert.link", json!({"cell": "A2", "target": "Sheet1!A60"})).unwrap();
    s.execute("insert.link", json!({"cell": "A3", "target": "file:///private/report"})).unwrap();
    s.execute("cell.set", json!({"cell": "A60", "input": "Destination"})).unwrap();
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert_eq!(pages, 2);
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(text.matches("/Subtype /Link").count(), 2);
    assert!(text.contains("/URI (https://example.org/report)"));
    assert!(text.contains("/Dest [") && text.contains("/XYZ "));
    assert!(!text.contains("/URI (file:"));

    // A link to a cell omitted by the selected print range must not point to a nonexistent
    // destination. The external link in the selected range remains clickable.
    let (cropped, _) = pdf_of(&mut s, json!({"range": "A1:A3"}));
    let cropped = String::from_utf8_lossy(&cropped);
    assert_eq!(cropped.matches("/Subtype /Link").count(), 1);
    assert!(!cropped.contains("/Dest ["));
}

#[test]
fn header_sections_split_join_and_field_names() {
    let spec = "&L&\"Arial,Bold\"Sales &&Co&CPage &P of &N&R&D";
    let parts = split_header_footer(spec);
    assert_eq!(parts, ["&\"Arial,Bold\"Sales &&Co".to_string(), "Page &P of &N".into(), "&D".into()]);
    assert_eq!(join_header_footer(&parts), spec);
    // Text before any section code is the centre section, as when printing.
    assert_eq!(split_header_footer("Report"), [String::new(), "Report".into(), String::new()]);
    assert_eq!(codes_to_fields("Page &P of &N, &A (&F) &&"), "Page &[Page] of &[Pages], &[Tab] (&[File]) &&");
    assert_eq!(fields_to_codes("Page &[Page] of &[Pages], &[Tab] (&[File]) &&"), "Page &P of &N, &A (&F) &&");
    // A dangling ampersand stays literal instead of eating the next section's code.
    assert_eq!(fields_to_codes("R&"), "R&&");
    let joined = join_header_footer(&[fields_to_codes("R&"), "x".into(), String::new()]);
    assert_eq!(split_header_footer(&joined)[0], "R&&");
    let cx = HfContext { page: 2, pages: 5, sheet: "Data".into(), ..Default::default() };
    let out = expand_header_footer(&join_header_footer(&[String::new(), fields_to_codes("&[Page] / &[Pages]"), fields_to_codes("&[Tab]")]), &cx);
    assert_eq!((out[1].text.as_str(), out[2].text.as_str()), ("2 / 5", "Data"));
    // Hostile input never panics.
    for t in ["&", "&\"", "&\"unterminated", "&&&", "&[Pag", "\u{1F600}&[Page]"] {
        let _ = join_header_footer(&split_header_footer(t));
        let _ = fields_to_codes(&codes_to_fields(t));
    }
}

#[test]
fn header_footer_date_follows_the_workbook_date_system() {
    let mut wb = Workbook::new();
    let date_1900 = hf_date_time(&wb).0;
    wb.date_system = gridcraft_core::DateSystem::D1904;
    assert_eq!(hf_date_time(&wb).0, date_1900);
}

#[test]
fn header_footer_section_edits_are_undoable_and_views_are_per_sheet() {
    let mut s = s();
    s.execute("pageLayout.headerFooter", json!({"header": "&LLeft&RRight"})).unwrap();
    let r = s.execute("pageLayout.headerFooter", json!({"part": "header", "section": "center", "text": "Page &[Page] of &[Pages]"})).unwrap();
    assert_eq!(r["header"], "&LLeft&CPage &P of &N&RRight");
    s.execute("pageLayout.headerFooter", json!({"part": "footer", "section": "right", "text": "&[Date]"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().print.footer, "&R&D");
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().print.header, "&LLeft&RRight");
    assert!(s.execute("pageLayout.headerFooter", json!({"part": "middle", "text": "x"})).is_err());
    assert!(s.execute("pageLayout.headerFooter", json!({"section": "top"})).is_err());
    let long = "x".repeat(1000);
    s.execute("pageLayout.headerFooter", json!({"part": "header", "section": "left", "text": long})).unwrap();
    assert!(s.doc().unwrap().wb.sheet(0).unwrap().print.header.chars().count() <= 255);
    // The view is per sheet and doesn't dirty a clean workbook.
    let mut fresh = Session::new();
    fresh.new_workbook();
    fresh.execute("home.insertSheet", json!({})).unwrap();
    fresh.doc_mut().unwrap().saved = fresh.doc().unwrap().wb.clone();
    fresh.execute("view.pageLayout", json!({"sheet": 0})).unwrap();
    let wb = &fresh.doc().unwrap().wb;
    assert_eq!((wb.sheet(0).unwrap().view, wb.sheet(1).unwrap().view), (SheetViewMode::PageLayout, SheetViewMode::Normal));
    assert!(!fresh.doc().unwrap().is_dirty(), "switching views is not an edit");
    s.execute("view.pageLayout", json!({"sheet": 0})).unwrap();
    s.execute("view.normal", json!({"sheet": 0})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().view, SheetViewMode::Normal);
    assert_eq!(s.execute("view.ruler", json!({"on": false})).unwrap()["on"], false);
    assert!(s.hide_ruler);
    assert_eq!(s.execute("view.ruler", json!({})).unwrap()["on"], true);
}

#[test]
fn page_count_300_rows_letter_portrait() {
    let mut s = s();
    let rows: Vec<Vec<Json>> = (1..=300).map(|i| vec![json!(i)]).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    // 684pt of printable height / 15pt rows = 45 rows per page → 7 pages.
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert_eq!(pages, 7);
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(contains(&pdf, "/Count 7"));
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    assert_eq!(pv["pages"], 7);
    assert_eq!(pv["pageList"][0]["rows"], "1:45");
    assert_eq!(pv["pageList"][1]["rows"], "46:90");
    assert_eq!(s.execute("file.printPreview", json!({"page": 7})).unwrap()["page"]["rows"], "271:300");
    assert!(s.execute("file.printPreview", json!({"page": 8})).is_err());
    // Landscape: 504pt / 15 = 33 rows per page → 10 pages.
    s.execute("pageLayout.pageSetup", json!({"orientation": "landscape"})).unwrap();
    assert_eq!(pdf_of(&mut s, json!({})).1, 10);
    // Fit to one page.
    assert_eq!(pdf_of(&mut s, json!({"fitToPage": true})).1, 1);
    s.execute("pageLayout.pageSetup", json!({"fitWidth": 1, "fitHeight": 2, "orientation": "portrait"})).unwrap();
    assert_eq!(pdf_of(&mut s, json!({})).1, 2);
    // Manual break and repeated title rows.
    s.execute("pageLayout.pageSetup", json!({"scale": 100, "rowBreaks": [11], "titleRows": "1:1"})).unwrap();
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    assert_eq!(pv["pageList"][0]["rows"], "1:10");
    // Title row takes one line on later pages: 44 body rows.
    assert_eq!(pv["pageList"][1]["rows"], "11:54");
}

#[test]
fn wide_sheet_goes_down_then_over() {
    let mut s = s();
    let rows: Vec<Vec<Json>> = (0..60).map(|r| (0..15).map(|c| json!(r * 100 + c)).collect()).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    // 511.2pt / 48pt = 10 columns per page; 45 rows per page.
    assert_eq!(pv["pages"], 4);
    assert_eq!(pv["pageList"][0]["range"], "A1:J45");
    assert_eq!(pv["pageList"][1]["range"], "A46:J60");
    assert_eq!(pv["pageList"][2]["range"], "K1:O45");
}

/// The ranges of the printed pages, in print order.
fn page_ranges(s: &mut Session) -> Vec<String> {
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    let list = pv["pageList"].as_array().unwrap();
    assert_eq!(pv["pages"], list.len());
    for (i, page) in list.iter().enumerate() {
        assert_eq!(page["page"], i + 1);
        assert_eq!(page["pageOrder"], s.doc().unwrap().wb.active().unwrap().print.page_order.id());
    }
    list.iter().map(|p| p["range"].as_str().unwrap().to_string()).collect()
}

/// The content streams of an exported PDF's pages, in page order (pages without pictures are the
/// file's only streams).
fn pdf_pages(s: &mut Session) -> Vec<String> {
    let (pdf, pages) = pdf_of(s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    let streams: Vec<String> = text.split("endstream").filter(|part| part.contains("stream\n")).map(str::to_string).collect();
    assert_eq!(streams.len() as u64, pages);
    streams
}

/// The page order measured in Excel: print area A1:D4 with a row break before row 3 and a column
/// break before column C, so four pages two tall and two wide, each filled and labelled in its
/// own colour, with `Page &P of &N` in the footer.
fn two_by_two_pages() -> Session {
    let mut s = s();
    for (range, color, label) in
        [("A1:B2", "#FF0000", "red"), ("A3:B4", "#00FF00", "green"), ("C1:D2", "#0000FF", "blue"), ("C3:D4", "#FFFF00", "yellow")]
    {
        s.execute("home.fillColor", json!({"range": range, "color": color})).unwrap();
        s.execute("cell.set", json!({"cell": &range[..2], "input": label})).unwrap();
    }
    s.execute("pageLayout.pageSetup", json!({"printArea": "A1:D4", "rowBreaks": [3], "colBreaks": ["C"], "footer": "Page &P of &N"})).unwrap();
    s
}

/// The colour of each printed page, read from its fill and label; each page's footer must
/// number it in print order.
fn printed_colours(s: &mut Session) -> Vec<&'static str> {
    let pages = pdf_pages(s);
    let n = pages.len();
    pages
        .iter()
        .enumerate()
        .map(|(i, page)| {
            assert!(page.contains(&format!("(Page {} of {n}) Tj", i + 1)), "page {}: {page}", i + 1);
            let found: Vec<&str> = [("red", "1 0 0 rg"), ("green", "0 1 0 rg"), ("blue", "0 0 1 rg"), ("yellow", "1 1 0 rg")]
                .into_iter()
                .filter(|(label, fill)| page.contains(&format!("({label}) Tj")) && page.contains(&format!("\n{fill}\n")))
                .map(|(label, _)| label)
                .collect();
            assert_eq!(found.len(), 1, "page {}: {found:?}", i + 1);
            found[0]
        })
        .collect()
}

#[test]
fn page_order_sets_the_sequence_of_the_pages_as_measured_in_excel() {
    let mut s = two_by_two_pages();
    // Down, then over is the default: down the first column of pages, then the next.
    assert_eq!(s.doc().unwrap().wb.active().unwrap().print.page_order, PageOrder::DownThenOver);
    assert_eq!(s.execute("pageLayout.pageSetup", json!({})).unwrap()["page_order"], "downThenOver");
    let down = ["A1:B2", "A3:B4", "C1:D2", "C3:D4"];
    assert_eq!(page_ranges(&mut s), down);
    assert_eq!(printed_colours(&mut s), ["red", "green", "blue", "yellow"]);
    // Over, then down: across the first row of pages, then the next.
    let r = s.execute("pageLayout.pageSetup", json!({"pageOrder": "overThenDown"})).unwrap();
    assert_eq!(r["page_order"], "overThenDown");
    let over = ["A1:B2", "C1:D2", "A3:B4", "C3:D4"];
    assert_eq!(page_ranges(&mut s), over);
    assert_eq!(printed_colours(&mut s), ["red", "blue", "green", "yellow"]);
    assert_eq!(s.execute("file.printPreview", json!({"page": 2})).unwrap()["page"]["range"], "C1:D2");
    // One undo step.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(page_ranges(&mut s), down);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(page_ranges(&mut s), over);
    // Anything but the two orders is refused and changes nothing, even next to a good value.
    for bad in [json!("sideways"), json!("OverThenDown"), json!(""), json!(2), json!(null), json!(true), json!(["downThenOver"])] {
        let e = s.execute("pageLayout.pageSetup", json!({"pageOrder": bad, "orientation": "landscape"})).unwrap_err();
        assert!(e.to_string().contains("pageOrder"), "{bad}: {e}");
    }
    let pr = &s.doc().unwrap().wb.active().unwrap().print;
    assert_eq!((pr.page_order, pr.orientation), (PageOrder::OverThenDown, Orientation::Portrait));
    assert!(s.take_ui_requests().is_empty(), "a programmatic call never opens a dialog");
    // First-page and odd/even headers go by the printed sequence too.
    s.execute(
        "pageLayout.headerFooter",
        json!({"header": "&COdd &P", "evenHeader": "&CEven &P", "firstHeader": "&CCover", "differentFirst": true, "differentOddEven": true}),
    )
    .unwrap();
    let pages = pdf_pages(&mut s);
    for (page, (header, label)) in pages.iter().zip([("Cover", "red"), ("Even 2", "blue"), ("Odd 3", "green"), ("Even 4", "yellow")]) {
        assert!(page.contains(&format!("({header}) Tj")) && page.contains(&format!("({label}) Tj")), "{header}: {page}");
    }
    s.execute("pageLayout.pageSetup", json!({"pageOrder": "downThenOver"})).unwrap();
    assert_eq!(page_ranges(&mut s), down);
    let pages = pdf_pages(&mut s);
    for (page, (header, label)) in pages.iter().zip([("Cover", "red"), ("Even 2", "green"), ("Odd 3", "blue"), ("Even 4", "yellow")]) {
        assert!(page.contains(&format!("({header}) Tj")) && page.contains(&format!("({label}) Tj")), "{header}: {page}");
    }
}

#[test]
fn page_order_without_breaks_and_with_print_titles() {
    let mut s = s();
    // Every cell shows its own address: 60 rows and 15 columns are two pages tall and two wide.
    let rows: Vec<Vec<Json>> = (1..=60).map(|r| (0..15).map(|c| json!(format!("{}{r}", gridcraft_core::col_to_letters(c)))).collect()).collect();
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    for titles in [false, true] {
        if titles {
            s.execute("pageLayout.pageSetup", json!({"titleRows": "1:1", "titleCols": "A:A"})).unwrap();
        }
        s.execute("pageLayout.pageSetup", json!({"pageOrder": "downThenOver"})).unwrap();
        let down = page_ranges(&mut s);
        if !titles {
            assert_eq!(down, ["A1:J45", "A46:J60", "K1:O45", "K46:O60"]);
        }
        s.execute("pageLayout.pageSetup", json!({"pageOrder": "overThenDown"})).unwrap();
        let over = page_ranges(&mut s);
        assert_eq!(over, [down[0].clone(), down[2].clone(), down[1].clone(), down[3].clone()], "titles: {titles}");
        // Each printed page starts with its own top-left cell; the title row and column (A1 is in
        // both) repeat on every page in either order.
        for (order, ranges) in [("overThenDown", &over), ("downThenOver", &down)] {
            s.execute("pageLayout.pageSetup", json!({"pageOrder": order})).unwrap();
            for (i, (page, range)) in pdf_pages(&mut s).iter().zip(ranges.iter()).enumerate() {
                let top_left = range.split(':').next().unwrap();
                assert!(page.contains(&format!("({top_left}) Tj")), "{order} page {}: no {top_left}", i + 1);
                assert_eq!(page.contains("(A1) Tj"), titles || i == 0, "{order} page {}, titles: {titles}", i + 1);
            }
        }
    }
}

#[test]
fn first_and_even_pages_print_their_own_headers() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["a"], ["b"], ["c"], ["d"], ["e"], ["f"]]})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"rowBreaks": [2, 4]})).unwrap();
    let r = s
        .execute(
            "pageLayout.headerFooter",
            json!({"header": "&COdd &P", "evenHeader": "&CEven &P", "firstHeader": "&CCover", "differentFirst": true, "differentOddEven": true}),
        )
        .unwrap();
    assert_eq!(r["firstHeader"], "&CCover");
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert_eq!(pages, 3);
    assert!(contains(&pdf, "(Cover) Tj") && contains(&pdf, "(Even 2) Tj") && contains(&pdf, "(Odd 3) Tj"));
    assert!(!contains(&pdf, "(Odd 1) Tj"), "the first page has its own header");
    // One section of the even-page footer, edited in place.
    s.execute("pageLayout.headerFooter", json!({"page": "even", "part": "footer", "section": "right", "text": "&[Tab]"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheet(0).unwrap().print.even_footer, "&R&A");
    assert!(s.execute("pageLayout.headerFooter", json!({"page": "last", "part": "header", "text": "x"})).is_err());
    // Turned off, every page prints the odd header again.
    s.execute("pageLayout.headerFooter", json!({"differentFirst": false, "differentOddEven": false})).unwrap();
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(contains(&pdf, "(Odd 1) Tj") && contains(&pdf, "(Odd 2) Tj") && !contains(&pdf, "(Cover) Tj"));
}

/// The y position of the first `(text) Tj` in a PDF (PDF space: larger is higher on the page).
fn text_y(pdf: &[u8], text: &str) -> Option<f32> {
    let s = String::from_utf8_lossy(pdf);
    let before = &s[..s.find(&format!("({text}) Tj"))?];
    let tm = before.rfind(" Tm")?;
    before[..tm].rsplit([' ', '\n']).next()?.parse().ok()
}

/// The x position and font size of the first `(text) Tj` in a PDF.
fn text_at(pdf: &[u8], text: &str) -> Option<(f32, f32)> {
    let s = String::from_utf8_lossy(pdf);
    let before = &s[..s.find(&format!("({text}) Tj"))?];
    let tf = before.rfind(" Tf")?;
    let size = before[..tf].rsplit(' ').next()?.parse().ok()?;
    let tm = before.rfind(" Tm")?;
    let mut nums = before[..tm].rsplit([' ', '\n']);
    let _y = nums.next()?;
    let x = nums.next()?.parse().ok()?;
    Some((x, size))
}

#[test]
fn headers_scale_with_the_sheet_and_align_with_its_margins_unless_turned_off() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"scale": 50, "left": 1.5, "right": 1.5})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&LLeft&C&14Middle"})).unwrap();
    // Both on (the default): the text prints at half size, from the 1.5-inch left margin.
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert_eq!(text_at(&pdf, "Left"), Some((108.0, 5.0)));
    assert_eq!(text_at(&pdf, "Middle").map(|t| t.1), Some(7.0));
    let r = s.execute("pageLayout.headerFooter", json!({"scaleWithDoc": false, "alignWithMargins": false})).unwrap();
    assert_eq!((&r["scaleWithDoc"], &r["alignWithMargins"]), (&json!(false), &json!(false)));
    // Both off: full size, 0.7 inch from the paper's left edge.
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert_eq!(text_at(&pdf, "Left"), Some((50.4, 10.0)));
    assert_eq!(text_at(&pdf, "Middle").map(|t| t.1), Some(14.0));
    s.execute("edit.undo", json!({})).unwrap();
    let pr = &s.doc().unwrap().wb.sheet(0).unwrap().print;
    assert!(pr.hf_scale_with_doc && pr.hf_align_with_margins);
}

#[test]
fn pdf_prints_cjk_text_instead_of_question_marks() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["签字清单", 2]]})).unwrap();
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(!contains(&pdf, "(????) Tj"), "CJK cell printed as (????) Tj");
    assert!(contains(&pdf, "<7B7E5B576E055355> Tj"));
    assert!(contains(&pdf, "(2) Tj"));
}

#[test]
fn pdf_contains_text_and_header_codes_expand() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Quarterly Report", null], ["Total", 1234.5]]})).unwrap();
    s.execute("home.bold", json!({"range": "A1"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"header": "&LLeft &A&CMiddle&RPage &P of &N", "footer": "&C&F", "gridlines": true, "headings": true}))
        .unwrap();
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert_eq!(pages, 1);
    assert!(contains(&pdf, "(Quarterly Report) Tj"));
    assert!(contains(&pdf, "(1234.5) Tj"));
    assert!(contains(&pdf, "(Left Sheet1) Tj"));
    assert!(contains(&pdf, "(Page 1 of 1) Tj"));
    assert!(contains(&pdf, "(Book1) Tj"));
    assert!(contains(&pdf, "/Helvetica-Bold"));
    let cx = HfContext {
        page: 3,
        pages: 9,
        sheet: "Data".into(),
        file: "b.xlsx".into(),
        date: "1/2/2026".into(),
        time: "9:00 AM".into(),
        ..Default::default()
    };
    let [l, c, r] = expand_header_footer("&L&\"Arial,Bold\"&A &D&C&14&BTitle && more&RP&P+1/&N &T", &cx);
    assert_eq!(l.text, "Data 1/2/2026");
    assert!(l.bold);
    assert_eq!(c.text, "Title & more");
    assert!(c.bold);
    assert_eq!(c.size, Some(14.0));
    assert_eq!(r.text, "P4/9 9:00 AM");
    assert_eq!(expand_header_footer("plain", &cx)[1].text, "plain");
    assert_eq!(expand_header_footer("&", &cx)[1].text, "");
    // A megabyte sheet name repeated by 5,000 `&A` codes stops at the section's 64 KiB.
    let long = HfContext { sheet: "S".repeat(1024 * 1024), ..cx };
    let [_, c, _] = expand_header_footer(&"&A".repeat(5_000), &long);
    assert!(c.text.is_empty(), "the name alone is over the limit");
    let short = HfContext { sheet: "S".repeat(1000), ..Default::default() };
    assert_eq!(expand_header_footer(&"&A".repeat(5_000), &short)[1].text.len(), 65_000);
}

#[test]
fn conditional_number_format_prints() {
    // #1163: a rule's number format replaces the cell's own, whether that is General or not;
    // cells the rule doesn't match keep theirs.
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [3], [4]]})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A3", "code": "0.0"})).unwrap();
    let rule = json!({"type": "cellIs", "operator": "greater", "value": "2", "style": {"num_fmt": "0.00"}});
    s.execute("home.conditionalFormat", json!({"range": "A1:A3", "rule": rule})).unwrap();
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(contains(&pdf, "(1) Tj"), "an unmatched cell keeps General");
    assert!(contains(&pdf, "(3.00) Tj") && !contains(&pdf, "(3) Tj"), "General cell under the rule");
    assert!(contains(&pdf, "(4.00) Tj") && !contains(&pdf, "(4.0) Tj"), "formatted cell under the rule");
}

#[test]
fn pdf_uses_indexed_number_format_palette_color() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "42"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A1", "format": "[Color4]0"})).unwrap();
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert_eq!(pages, 1);
    assert!(contains(&pdf, "0 1 0 rg"), "expected indexed Color4 green in PDF text stream");
}

#[test]
fn export_writes_file() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
    let dir = std::env::temp_dir().join(format!("gridcraft-print-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("out.pdf");
    let r = s.execute("file.exportPdf", json!({"path": path.to_str().unwrap()})).unwrap();
    assert_eq!(r["pages"], 1);
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(contains(&bytes, "(hello) Tj"));
    let _ = std::fs::remove_dir_all(&dir);
    let r = s.execute("file.print", json!({})).unwrap();
    assert!(r["base64"].as_str().unwrap().len() > 100);
}

#[test]
fn save_as_pdf_writes_a_pdf_and_keeps_the_workbook_file() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "B2", "input": "hello"})).unwrap();
    let dir = std::env::temp_dir().join(format!("gridcraft-save-pdf-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Out.PDF");
    let r = s.execute("file.saveAs", json!({"path": path.to_str().unwrap()})).unwrap();
    assert_eq!(r["pages"], 1);
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(contains(&bytes, "(hello) Tj"));
    // The PDF is a print, not the workbook's file: Save still asks where to put the workbook.
    let d = s.doc().unwrap();
    assert!(d.path.is_none() && d.is_dirty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rich_sheet_renders() {
    let mut s = Session::new();
    s.execute("file.new", json!({"sample": "sales"})).unwrap();
    s.execute("range.setValues", json!({"range": "H1", "values": [["A long piece of text that overflows into neighbours"]]})).unwrap();
    s.execute("cell.set", json!({"cell": "H3", "input": "Wrapped text across several lines in a narrow cell"})).unwrap();
    s.execute("home.wrapText", json!({"range": "H3"})).unwrap_or_default();
    s.execute("selection.set", json!({"range": "J5:K6"})).unwrap();
    s.execute("home.mergeCenter", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "J5", "input": "Merged"})).unwrap();
    s.execute("home.borders", json!({"range": "J5:K6", "preset": "outside", "style": "thick"})).unwrap_or_default();
    let (pdf, pages) = pdf_of(&mut s, json!({"sheets": "all"}));
    assert!(pages >= 1);
    assert!(contains(&pdf, "(Merged) Tj"));
    assert!(contains(&pdf, "(A long piece of text that overflows into neighbours) Tj"));
}

#[test]
fn charts_and_images_are_drawn() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Fruit", "Qty"], ["Apples", 3], ["Pears", 5]]})).unwrap();
    s.execute("insert.chart", json!({"range": "A1:B3", "type": "pie"})).unwrap_or_default();
    s.execute("insert.chart", json!({"range": "A1:B3"})).unwrap_or_default();
    // A 2×2 PNG with transparency.
    let mut png = Vec::new();
    let img = image::RgbaImage::from_raw(2, 2, vec![255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 0, 0, 0, 0]).unwrap();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let id = wb.next_object_id();
    wb.sheet_mut(0).unwrap().images.push(Image {
        id,
        anchor: Anchor { cell: CellRef::new(5, 5), dx: 0.0, dy: 0.0, width: 40.0, height: 40.0, mode: gridcraft_model::AnchorMode::MoveAndSize },
        data: png.into(),
        mime: "image/png".into(),
        alt: String::new(),
    });
    let charts = s.doc().unwrap().wb.active().unwrap().charts.len();
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(contains(&pdf, "/Subtype /Image"));
    assert!(contains(&pdf, "/SMask"));
    if charts > 0 {
        assert!(contains(&pdf, "(Apples) Tj") || contains(&pdf, "(Qty) Tj"));
    }
}

#[test]
fn page_setup_edits_settings() {
    let mut s = s();
    let r = s
        .execute(
            "pageLayout.pageSetup",
            json!({"paper": "A4", "orientation": "landscape", "margins": {"left": 1, "top": 0.5}, "right": 0.25, "centerH": true, "printArea": "A1:D20", "titleRows": "1:2", "titleCols": "A:A", "colBreaks": ["C"]}),
        )
        .unwrap();
    assert_eq!(r["paper"], "A4");
    assert_eq!(r["orientation"], "Landscape");
    assert_eq!(r["margins"][0], 1.0);
    assert_eq!(r["margins"][1], 0.25);
    assert_eq!(r["margins"][2], 0.5);
    assert_eq!(r["center_h"], true);
    assert_eq!(r["title_rows"], json!([0, 1]));
    assert_eq!(r["col_breaks"], json!([2]));
    let sh = s.doc().unwrap().wb.active().unwrap().print.clone();
    assert_eq!(sh.print_area, RangeRef::parse("A1:D20"));
    s.execute("pageLayout.pageSetup", json!({"printArea": ""})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().print.print_area, None);
    assert!(s.execute("pageLayout.pageSetup", json!({"paper": "Napkin"})).is_err());
    assert!(s.execute("pageLayout.pageSetup", json!({"orientation": "sideways"})).is_err());
    // Undoable as one step.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().print.print_area.is_some());
}

/// The Japanese paper sizes are the JIS ones: `B4`/`B5`/`B6` changed meaning from ISO, and the
/// ISO B series stays reachable by name. The postcards and envelopes are the JIS sizes too.
#[test]
fn japanese_paper_sizes_are_jis() {
    for (name, w, h) in [
        ("A4", 595.28, 841.89), // unchanged
        ("B4", 728.50, 1031.81),
        ("B5", 515.91, 728.50),
        ("B6", 362.83, 515.91),
        ("B4 (JIS)", 728.50, 1031.81),
        ("b5-jis", 515.91, 728.50),
        ("B4 (ISO)", 708.66, 1000.63),
        ("B5 (ISO)", 498.90, 708.66),
        ("B6 (ISO)", 354.33, 498.90),
        ("A6", 297.64, 419.53),
        ("Japanese Postcard", 283.46, 419.53),
        ("japanese double postcard", 419.53, 566.93),
        ("Japanese Envelope #3", 340.16, 666.14),
        ("chou #4", 255.12, 581.10),
        ("Japanese Envelope Kaku #2", 680.31, 941.10),
        ("kaku #3", 612.28, 785.20),
    ] {
        let got = paper_size(name);
        assert!((got.0 - w).abs() < 0.01 && (got.1 - h).abs() < 0.01, "{name}: {got:?}");
    }
    // 長形4号 is 90×205 mm (90×225 mm is 長形40号), so its height is 205 mm = 581.10 pt — not the
    // 225 mm it once had.
    let chou4 = paper_size("Japanese Envelope #4");
    assert_eq!((chou4.0, chou4.1), (255.12, 581.10));
    assert_eq!(chou4, paper_size("chou #4"));
    // Unknown names keep the old Letter fallback.
    assert_eq!(paper_size("Napkin"), (612.0, 792.0));
}

/// Page Setup's paper reaches the PDF page: the MediaBox is the paper's, A4 is unchanged, B5 is
/// JIS B5, and the Japanese postcard prints at 283.46×419.53 pt. Landscape swaps the sides and
/// fit-to-page keeps the paper size.
#[test]
fn page_setup_paper_sizes_reach_the_pdf_page() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x"]]})).unwrap();
    let close = |v: &Json, want: f64| (v.as_f64().unwrap() - want).abs() < 0.01;
    for (paper, w, h) in [
        ("A4", 595.28f64, 841.89f64),
        ("B5", 515.91, 728.50),
        ("Japanese Postcard", 283.46, 419.53),
        ("Japanese Double Postcard", 419.53, 566.93),
        ("Japanese Envelope Kaku #2", 680.31, 941.10),
    ] {
        s.execute("pageLayout.pageSetup", json!({"paper": paper, "orientation": "portrait"})).unwrap();
        let pv = s.execute("file.printPreview", json!({})).unwrap();
        let page = &pv["pageList"][0];
        assert_eq!(page["paper"], paper);
        assert!(close(&page["width"], w) && close(&page["height"], h), "{paper}: {page}");
        let (pdf, pages) = pdf_of(&mut s, json!({}));
        assert_eq!(pages, 1);
        let want = format!("/MediaBox [0 0 {w} {h}]");
        assert!(contains(&pdf, &want), "{paper}: wanted {want}");
    }
    // Landscape swaps the sides, and fit-to-page does not change the paper.
    s.execute("pageLayout.pageSetup", json!({"paper": "B5", "orientation": "landscape"})).unwrap();
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert_eq!(pages, 1);
    assert!(contains(&pdf, "/MediaBox [0 0 728.5 515.91]"));
    s.execute("pageLayout.pageSetup", json!({"paper": "B5", "orientation": "portrait"})).unwrap();
    let (pdf, _) = pdf_of(&mut s, json!({"fitToPage": true}));
    assert!(contains(&pdf, "/MediaBox [0 0 515.91 728.5]"));
    // `pageLayout.size` still takes any name, and an unknown one prints on Letter as before.
    s.execute("pageLayout.size", json!({"paper": "Napkin"})).unwrap();
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    assert!(close(&pv["pageList"][0]["width"], 612.0) && close(&pv["pageList"][0]["height"], 792.0));
    // Page Setup refuses an unknown name.
    assert!(s.execute("pageLayout.pageSetup", json!({"paper": "Napkin"})).is_err());
}

#[test]
fn hostile_settings_never_panic_and_cap_pages() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "XFD1048576", "input": "far"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "near"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"scale": 400, "margins": {"left": 20, "right": 20, "top": 20, "bottom": 20}})).unwrap();
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    assert!(pv["pages"].as_u64().unwrap() <= MAX_PAGES as u64);
    assert_eq!(pv["pages"].as_u64().unwrap(), MAX_PAGES as u64);
    let r = s.execute("file.exportPdf", json!({})).unwrap();
    assert_eq!(r["pages"], MAX_PAGES);
    // Over, then down stops at the cap as well, along the first row of pages.
    s.execute("pageLayout.pageSetup", json!({"pageOrder": "overThenDown"})).unwrap();
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    assert_eq!(pv["pages"].as_u64().unwrap(), MAX_PAGES as u64);
    assert_eq!(pv["pageList"][1]["rows"], pv["pageList"][0]["rows"], "the second page is beside the first");
    s.execute("pageLayout.pageSetup", json!({"fitWidth": 1, "fitHeight": 1, "titleRows": "1:1048576", "titleCols": "A:XFD", "headings": true}))
        .unwrap();
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    assert!(pv["pages"].as_u64().unwrap() <= MAX_PAGES as u64);
    assert!(s.execute("file.exportPdf", json!({"range": "A1:C3"})).is_ok());
    assert!(s.execute("file.exportPdf", json!({"range": "nope"})).is_err());
    assert!(s.execute("file.exportPdf", json!({"sheets": "some"})).is_err());
    // Empty workbook still prints one blank page.
    let mut e = Session::new();
    e.new_workbook();
    let r = e.execute("file.exportPdf", json!({})).unwrap();
    assert_eq!(r["pages"], 1);
    let r = e.execute("file.exportPdf", json!({"sheets": "all"})).unwrap();
    assert_eq!(r["pages"], 1);
    // Weird row sizes and huge fonts.
    s.execute("pageLayout.pageSetup", json!({"fitToPage": false, "scale": 10, "printArea": "A1:B3", "titleRows": "", "titleCols": ""})).unwrap();
    s.execute("home.fontSize", json!({"range": "A1", "size": 409})).unwrap_or_default();
    assert!(s.execute("file.exportPdf", json!({})).is_ok());
}

#[test]
fn export_pdf_without_a_path_returns_bytes_and_never_opens_a_dialog() {
    let mut s = s();
    for params in [json!({}), json!({"path": ""}), json!({"path": null})] {
        let r = s.execute("file.exportPdf", params).unwrap();
        assert!(r.get("path").is_none(), "no file is written without a path");
        let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }
    assert!(s.take_ui_requests().is_empty(), "a programmatic call never queues a dialog");
    // Headless callers (CLI, MCP) get the same bytes, not an error.
    assert!(s.execute_headless("file.exportPdf", json!({})).is_ok());
    // With no workbook open it is a clear error, not a panic.
    let mut none = Session::new();
    assert!(none.execute("file.exportPdf", json!({})).is_err());
}

#[test]
fn bands_unit() {
    let b = bands(0, 9, |_| 10.0, |_| 35.0, &[], 100);
    assert_eq!(b, vec![(0, 2), (3, 5), (6, 8), (9, 9)]);
    let b = bands(0, 9, |_| 10.0, |_| 35.0, &[5], 100);
    assert_eq!(b, vec![(0, 2), (3, 4), (5, 7), (8, 9)]);
    // A line larger than the page gets its own band.
    let b = bands(0, 2, |r| if r == 1 { 100.0 } else { 10.0 }, |_| 35.0, &[], 100);
    assert_eq!(b, vec![(0, 0), (1, 1), (2, 2)]);
    assert_eq!(bands(0, 1_000_000, |_| 10.0, |_| 5.0, &[], 3).len(), 3);
    let w = wrap_lines("the quick brown fox jumps", Font::Helvetica, 10.0, 50.0);
    assert!(w.len() >= 2);
    assert!(w.iter().all(|l| gridcraft_pdf::text_width(Font::Helvetica, 10.0, l) <= 50.0 || !l.contains(' ')));
    let w = wrap_lines("Supercalifragilistic", Font::Helvetica, 10.0, 20.0);
    assert!(w.len() > 2);
}

/// The wrapper `wrap_lines` replaced, re-measuring strings, as the reference for its lines.
fn wrap_lines_remeasuring(text: &str, font: Font, size: f32, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for word in para.split(' ') {
            let cand = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if line.is_empty() || gridcraft_pdf::text_width(font, size, &cand) <= width {
                line = cand;
            } else {
                out.push(std::mem::take(&mut line));
                line = word.to_string();
            }
            while gridcraft_pdf::text_width(font, size, &line) > width && line.chars().count() > 1 {
                let chars: Vec<char> = line.chars().collect();
                let (mut lo, mut hi) = (1usize, chars.len());
                while lo < hi {
                    let mid = lo + (hi - lo).div_ceil(2);
                    let cut: String = chars[..mid].iter().collect();
                    if gridcraft_pdf::text_width(font, size, &cut) <= width {
                        lo = mid;
                    } else {
                        hi = mid - 1;
                    }
                }
                if lo == chars.len() {
                    break;
                }
                out.push(chars[..lo].iter().collect());
                line = chars[lo..].iter().collect();
            }
            if out.len() > 2000 {
                return out;
            }
        }
        out.push(line);
    }
    out
}

#[test]
fn wrap_lines_matches_the_remeasuring_wrapper() {
    let texts = [
        "the quick brown fox jumps",
        "Supercalifragilistic",
        "a  b   c\n\nverylongwordthatneedsbreaking short\nend ",
        "Wide WWWWWWWWWW words, iiiiiiii narrow ones; mixed MMMMiiiiMMMM",
        "€ ü ß — unicode Ünïcödé text",
        "",
        " ",
    ];
    for text in texts {
        for font in [Font::Helvetica, Font::HelveticaBold, Font::Courier] {
            for (size, width) in [(10.0, 50.0), (10.0, 20.0), (11.0, 1.0), (8.0, 300.0), (12.0, 0.0)] {
                assert_eq!(wrap_lines(text, font, size, width), wrap_lines_remeasuring(text, font, size, width), "{text:?} {font:?} {size} {width}");
            }
        }
    }
}

#[test]
fn wrapping_a_long_word_in_a_narrow_box_stops_at_the_line_limit() {
    // One character per line: the old wrapper re-measured and copied the rest of the word for
    // each of the 100,000 lines before checking the limit.
    let word = "w".repeat(100_000);
    let lines = wrap_lines(&word, Font::Helvetica, 10.0, 1.0);
    assert!(lines.len() <= 2002, "{}", lines.len());
    assert!(lines.iter().all(|l| l == "w"));
    // A long paragraph in a very wide box is one line.
    let para = vec!["word"; 50_000].join(" ");
    assert_eq!(wrap_lines(&para, Font::Helvetica, 10.0, 1.0e9), vec![para.clone()]);
}

#[test]
fn center_across_spans_following_empty_center_across_cells() {
    let mut styles = StyleTable::default();
    let across = styles.intern(Style { align: Alignment { h: HAlign::CenterAcross, ..Alignment::default() }, ..Style::default() });
    let mut sh = Sheet::new("S");
    for col in 0..3 {
        sh.set_style(CellRef::new(0, col), across);
    }
    sh.set_value(CellRef::new(0, 0), Value::Text("Title".into()));
    let a1 = CellRef::new(0, 0);
    assert_eq!(center_across_width(&styles, &sh, a1, &[]), sh.col_width(1) + sh.col_width(2));
    sh.set_value(CellRef::new(0, 2), Value::Text("x".into()));
    assert_eq!(center_across_width(&styles, &sh, a1, &[]), sh.col_width(1));
    let merged = [RangeRef::new(CellRef::new(0, 1), CellRef::new(1, 1))];
    assert_eq!(center_across_width(&styles, &sh, a1, &merged), 0.0);
}

#[test]
fn rotated_wrapped_text_prints_rotated() {
    // Upright headers over narrow columns (mark sheets) combine rotation with Wrap Text.
    let mut s = s();
    let header = "Term 1 : Drama : Continuous Assessment";
    s.execute("range.setValues", json!({"range": "A1", "values": [[header, header]]})).unwrap();
    s.execute("home.wrapText", json!({"range": "A1:B1", "on": true})).unwrap();
    s.execute("home.orientation", json!({"range": "A1", "angle": "up"})).unwrap();
    s.execute("home.orientation", json!({"range": "B1", "angle": "down"})).unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:B", "chars": 6})).unwrap();
    s.execute("home.rowHeight", json!({"rows": "1:1", "height": 267})).unwrap();
    let (pdf, _) = pdf_of(&mut s, json!({}));
    let pdf = String::from_utf8_lossy(&pdf);
    // The page holds only these two cells, so every text block on it is a piece of a header.
    let blocks: Vec<&str> = pdf.split("BT\n").skip(1).filter(|b| b.contains(" Tj")).collect();
    let up = blocks.iter().filter(|b| b.contains("\n0 1 -1 0 ")).count();
    let down = blocks.iter().filter(|b| b.contains("\n0 -1 1 0 ")).count();
    // Every piece of both headers is rotated, each wrapped along the row height into a few long lines.
    assert_eq!(up + down, blocks.len(), "a header line printed flat: {blocks:?}");
    assert!((1..=3).contains(&up) && (1..=3).contains(&down), "up {up}, down {down}: {blocks:?}");
}

#[test]
fn checkbox_cells_print_as_boxes_not_true_false() {
    // #2972: a checkbox cell printed its value as TRUE or FALSE.
    let mut s = s();
    s.execute("insert.checkbox", json!({"range": "A1:A2"})).unwrap();
    s.execute("cell.toggleCheckbox", json!({"cell": "A2"})).unwrap();
    s.execute("cell.set", json!({"cell": "C1", "input": "TRUE"})).unwrap();
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(!contains(&pdf, "(FALSE) Tj"), "the unchecked box printed as FALSE");
    // Only the plain TRUE in C1 prints as text; the checked box is a filled box.
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(text.matches("(TRUE) Tj").count(), 1, "the checked box printed as TRUE");
    assert!(contains(&pdf, "0.063 0.486 0.255 rg"), "the checked box is not filled");
    assert!(contains(&pdf, "0.431 0.431 0.431 RG"), "the unchecked box is not outlined");
}

#[test]
fn an_equation_without_a_layout_prints_its_source_one_line_per_line() {
    use super::objects::{MAX_FALLBACK_LINES, fallback_lines};
    let mut s = s();
    s.execute("insert.equation", json!({"latex": "x", "cell": "B2"})).unwrap();
    // A file can carry a source the layout engine refuses: two lines here, in a frame 100 px
    // (75 pt) tall, which holds six 12 pt lines.
    let with_source = |text: &str| {
        let mut wb = (*s.doc().unwrap().wb).clone();
        if let Some(sh) = wb.sheet_mut(0) {
            for x in sh.shapes.iter_mut() {
                x.text = text.into();
                x.anchor.height = 100.0;
            }
        }
        let mut t = Session::new();
        t.add_document(crate::DocState::new(wb, None, "Book".into(), crate::Trust::FromFile));
        pdf_of(&mut t, json!({})).0
    };
    let pdf = with_source("frac{\nsecond line");
    assert!(contains(&pdf, "(frac{) Tj"), "the first line is its own text run");
    assert!(contains(&pdf, "(second line) Tj"), "and so is the second");
    assert!(!contains(&pdf, "(frac{?second line) Tj"), "the newline is not printed as ?");
    let (y1, y2) = (text_y(&pdf, "frac{").unwrap(), text_y(&pdf, "second line").unwrap());
    assert!(y1 > y2 && y1 - y2 < 13.0, "the second line sits one line below the first: {y1} {y2}");
    // Lines the frame cannot hold are not printed.
    let pdf = with_source(&"l\n".repeat(10_000));
    assert_eq!(String::from_utf8_lossy(&pdf).matches("(l) Tj").count(), 6);
    // Whatever the frame, the count is capped; a frame too low for one line still shows the first.
    let tall = "l\n".repeat(10_000);
    assert_eq!(fallback_lines(&tall, f32::INFINITY, 12.0).count(), MAX_FALLBACK_LINES);
    assert_eq!(fallback_lines(&tall, 1.0e9, 12.0).count(), MAX_FALLBACK_LINES);
    assert_eq!(fallback_lines("a\nb\nc", 30.0, 12.0).count(), 2);
    assert_eq!(fallback_lines("a\nb", 0.0, 12.0).count(), 1);
    assert_eq!(fallback_lines("a\nb", f32::NAN, 12.0).count(), 1);
}

#[test]
fn the_fallback_text_is_clipped_to_the_equation_frame() {
    // A long line of a source that does not lay out, in a narrow frame (40 by 100 px, 30 by
    // 75 pt): the lines are drawn inside a clip of the frame, nested in the block's, so they
    // cannot run over neighbours, and the clip is lifted again before the next object (a text
    // box below the equation in the same block) is drawn.
    let mut s = s();
    s.execute("insert.equation", json!({"latex": "x", "cell": "B2"})).unwrap();
    s.execute("insert.textBox", json!({"at": "B12", "text": "later box"})).unwrap();
    let mut wb = (*s.doc().unwrap().wb).clone();
    if let Some(sh) = wb.sheet_mut(0) {
        for x in sh.shapes.iter_mut().filter(|x| x.kind == gridcraft_model::ShapeKind::Equation) {
            x.text = format!("frac{{{}", "l".repeat(200));
            x.anchor.width = 40.0;
            x.anchor.height = 100.0;
        }
    }
    let mut t = Session::new();
    t.add_document(crate::DocState::new(wb, None, "Book".into(), crate::Trust::FromFile));
    let (pdf, _) = pdf_of(&mut t, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    let clip = text.find("30 75 re W n").expect("a clip the size of the frame");
    let run = text.find("(frac{llll").expect("the source line");
    let later = text.find("(later box) Tj").expect("the text box after the equation");
    assert!(clip < run && run < later, "clip, then the text, then the later object");
    // The net number of saved graphics states (`q` less `Q`) before an operator.
    let depth = |at: usize| text[..at].lines().filter(|l| *l == "q").count() as i64 - text[..at].lines().filter(|l| *l == "Q").count() as i64;
    assert_eq!(depth(run), depth(clip), "the text is drawn under the frame's clip");
    assert_eq!(depth(later), depth(clip) - 1, "the later object is drawn with the frame's state restored");
}

#[test]
#[allow(clippy::disallowed_methods)] // tests run only off wasm, where the clock exists
fn an_equation_source_of_a_megabyte_prints_a_bounded_excerpt() {
    // A file can carry a source far past `MAX_SOURCE_BYTES` (the drawing part may be 512 MiB). It
    // never lays out, so the fallback prints the first 4,096 bytes and does no work on the rest:
    // the PDF holds one run of that length, not the megabyte, and export takes bounded time.
    let mut s = s();
    s.execute("insert.equation", json!({"latex": "x", "cell": "B2"})).unwrap();
    let mut wb = (*s.doc().unwrap().wb).clone();
    if let Some(sh) = wb.sheet_mut(0) {
        for x in sh.shapes.iter_mut() {
            x.text = "l".repeat(1 << 20);
        }
    }
    let mut t = Session::new();
    t.add_document(crate::DocState::new(wb, None, "Book".into(), crate::Trust::FromFile));
    let start = std::time::Instant::now();
    let (pdf, _) = pdf_of(&mut t, json!({}));
    let took = start.elapsed();
    let text = String::from_utf8_lossy(&pdf);
    let longest = text.split(|c| c != 'l').map(str::len).max().unwrap_or(0);
    assert_eq!(longest, gridcraft_math::MAX_SOURCE_BYTES, "the run is the excerpt and no more");
    assert!(pdf.len() < 64 * 1024, "the PDF holds the excerpt, not the megabyte: {} bytes", pdf.len());
    assert!(took < std::time::Duration::from_secs(5), "{took:?}");
}

#[test]
fn equations_print_with_the_symbol_font() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("insert.equation", json!({"latex": "\\sum_{i=1}^{n} \\alpha_i \\le \\infty \\pm \\sqrt{x} \\times \\partial y", "cell": "B2"}))
        .unwrap();
    s.execute("insert.equation", json!({"latex": "\\mathbb{R} \\oplus \\text{\u{4E2D}}", "cell": "B8"})).unwrap();
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(contains(&pdf, "/BaseFont /Symbol >>"), "maths symbols use the Symbol font");
    assert!(contains(&pdf, "/BaseFont /Times-Italic"), "variables are italic");
    // The summation (0xE5) and alpha (0x61) codes in Symbol.
    assert!(contains(&pdf, "(\\345) Tj") && contains(&pdf, "(a) Tj"));
    // A glyph no standard font has is a [?] box.
    assert!(contains(&pdf, "(?) Tj"));
}

// ---------------------------------------------------------------- header/footer pictures (#1257)

/// A `w` × `h` PNG of one colour.
fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([20, 90, 160, 255]));
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

/// The place a shape id (`LH`, `CFFIRST`…) names.
fn hf_place(id: &str) -> (gridcraft_model::HfPage, bool, u8) {
    use gridcraft_model::HfPage;
    let (base, page) = match id {
        _ if id.ends_with("FIRST") => (&id[..2], HfPage::First),
        _ if id.ends_with("EVEN") => (&id[..2], HfPage::Even),
        _ => (id, HfPage::Odd),
    };
    let section = match &base[..1] {
        "L" => 0,
        "C" => 1,
        _ => 2,
    };
    (page, &base[1..2] == "F", section)
}

/// The size a VML-like style (`width:120pt;height:40pt`) gives, in points.
fn hf_size(style: &str) -> (f32, f32) {
    let get = |k: &str| style.split(';').find_map(|d| d.strip_prefix(k)).and_then(|v| v.trim_end_matches("pt").parse().ok()).unwrap_or(10.0);
    (get("width:"), get("height:"))
}

/// Gives the first sheet header/footer pictures, one per `(shape id, style, image)`.
fn keep_hf_pictures(s: &mut Session, shapes: &[(&str, &str, Vec<u8>)]) {
    let pics = shapes
        .iter()
        .map(|(id, style, data)| {
            let (page, footer, section) = hf_place(id);
            let (width, height) = hf_size(style);
            gridcraft_model::HfPicture {
                page,
                footer,
                section,
                width,
                height,
                data: std::sync::Arc::new(data.clone()),
                mime: "image/png".into(),
                title: String::new(),
                adjust: vec![],
            }
        })
        .collect();
    let d = s.doc_mut().unwrap();
    std::sync::Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap().print.hf_pictures = pics;
}

/// Kept header and footer pictures print at their `&G`, at their size, on the header and footer
/// margins.
#[test]
fn header_and_footer_pictures_print_where_their_g_is() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "Invoice"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&L&G&CInvoice", "footer": "&CPage &P &G"})).unwrap();
    keep_hf_pictures(&mut s, &[("LH", "width:120pt;height:40pt", png(30, 10)), ("CF", "width:80pt;height:20pt", png(8, 2))]);
    let (pdf, _) = pdf_of(&mut s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(text.matches("/Subtype /Image").count(), 2, "{text}");
    // Letter paper, default margins (left 0.7 in, header and footer 0.3 in): the letterhead's top
    // sits on the header margin, the footer picture's bottom on the footer margin.
    assert!(text.contains("q 120 0 0 40 50.4 730.4 cm /Sh"), "{text}");
    assert!(text.contains(" 21.6 cm /Sh") && text.contains("q 80 0 0 20 "), "{text}");
    assert!(text.contains("(Invoice) Tj") && text.contains("(Page 1 ) Tj"));
}

/// A kept picture prints only where its section has `&G`, and an unreadable or oversized image is
/// left out.
#[test]
fn header_pictures_need_their_g_and_a_readable_image() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    // No &G: the kept picture isn't printed.
    s.execute("pageLayout.headerFooter", json!({"header": "&LNo picture"})).unwrap();
    keep_hf_pictures(&mut s, &[("LH", "width:120pt;height:40pt", png(4, 4))]);
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(!contains(&pdf, "/Subtype /Image"));
    assert!(contains(&pdf, "(No picture) Tj"));
    // An image that isn't one, and a huge one: left out, the text still prints.
    s.execute("pageLayout.headerFooter", json!({"header": "&LBefore&G after&R&G"})).unwrap();
    keep_hf_pictures(
        &mut s,
        &[("LH", "width:10pt;height:10pt", b"\x89PNG\r\n\x1a\nbroken".to_vec()), ("RH", "width:10pt;height:10pt", png(9000, 1))],
    );
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(!contains(&pdf, "/Subtype /Image"));
    assert!(contains(&pdf, "(Before after) Tj"));
}

/// With Different First Page, the first-page picture prints on page 1 only.
#[test]
fn first_page_pictures_print_on_the_first_page_only() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "one"})).unwrap();
    s.execute("cell.set", json!({"cell": "A200", "input": "two"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&C&G", "firstHeader": "&C&G", "differentFirst": true})).unwrap();
    keep_hf_pictures(&mut s, &[("CHFIRST", "width:50pt;height:50pt", png(5, 5))]);
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert!(pages > 1);
    assert_eq!(String::from_utf8_lossy(&pdf).matches(" cm /Sh").count(), 1);
}

/// `expand_header_footer` records where in its section's text the first `&G` is.
#[test]
fn g_marks_where_the_picture_goes() {
    let cx = HfContext::default();
    let [l, c, r] = expand_header_footer("&LAB&GCD&G&C&G&RNone", &cx);
    assert_eq!((l.text.as_str(), l.picture), ("ABCD", Some(2)));
    assert_eq!((c.text.as_str(), c.picture), ("", Some(0)));
    assert_eq!(r.picture, None);
}

/// Like [`keep_hf_pictures`], but every picture shares the same image bytes.
fn keep_one_hf_image(s: &mut Session, ids: &[&str], style: &str, data: Vec<u8>) {
    keep_hf_pictures(s, &ids.iter().map(|id| (*id, style, Vec::new())).collect::<Vec<_>>());
    let data = std::sync::Arc::new(data);
    let d = s.doc_mut().unwrap();
    for p in &mut std::sync::Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap().print.hf_pictures {
        p.data = data.clone();
    }
}

/// A header picture on many pages is one image object the PDF draws on each page.
#[test]
fn a_header_picture_is_one_image_object_for_every_page() {
    let mut s = s();
    for r in [1, 100, 200] {
        s.execute("cell.set", json!({"cell": format!("A{r}"), "input": "x"})).unwrap();
    }
    // The same logo in all six sections.
    s.execute("pageLayout.headerFooter", json!({"header": "&L&G&C&G&R&G", "footer": "&L&G&C&G&R&G"})).unwrap();
    keep_one_hf_image(&mut s, &["LH", "CH", "RH", "LF", "CF", "RF"], "width:20pt;height:20pt", png(6, 6));
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    assert!(pages >= 3);
    assert_eq!(text.matches("/Subtype /Image").count(), 1, "decoded and written once");
    assert_eq!(text.matches("/Sh1 Do").count(), 6 * pages as usize, "drawn six times on every page");
}

/// Pictures no printed page shows (no `&G`, or a page kind not used) are not decoded.
#[test]
fn only_header_pictures_a_page_prints_are_decoded() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    // RH has no &G; the first-page and even-page pictures belong to options that are off.
    s.execute("pageLayout.headerFooter", json!({"header": "&L&G&RRight", "firstHeader": "&C&G", "evenHeader": "&C&G"})).unwrap();
    keep_hf_pictures(
        &mut s,
        &[
            ("LH", "width:20pt;height:20pt", png(4, 4)),
            ("RH", "width:20pt;height:20pt", png(5, 5)),
            ("CHFIRST", "width:20pt;height:20pt", png(6, 6)),
            ("CHEVEN", "width:20pt;height:20pt", png(7, 7)),
        ],
    );
    let (pdf, _) = pdf_of(&mut s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(text.matches("/Subtype /Image").count(), 1, "{text}");
    assert!(text.contains("/Width 4 /Height 4"));
}

/// Large pictures share one decoding budget per export: those past it are left out, the rest print.
#[test]
fn header_pictures_share_a_decoding_budget() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&L&G&C&G&R&G", "footer": "&C&G"})).unwrap();
    // Four distinct 4000 × 4000 pictures printed 22 in wide: decoded at 150 dpi (3300 px, about
    // 44 MB each), so the 128 MB budget takes three of them and leaves the fourth out.
    let big = |c: u8| {
        let img = image::RgbaImage::from_pixel(4000, 4000, image::Rgba([c, 0, 0, 255]));
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    };
    let style = "width:1584pt;height:1584pt";
    keep_hf_pictures(&mut s, &[("LH", style, big(10)), ("CH", style, big(20)), ("RH", style, big(30)), ("CF", style, big(40))]);
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert_eq!(String::from_utf8_lossy(&pdf).matches("/Subtype /Image").count(), 3);
}

// ---------------------------------------------------------------- header/footer pictures from a real XLSX package

/// A one-sheet XLSX package as Excel lays it out: `header`/`footer` text, a VML drawing named by
/// `<legacyDrawingHF>` with one picture-frame shape per `(shape id, style, image part)`, and the
/// image parts in `images` (a shape may name a part that isn't there).
fn hf_package(header: &str, footer: &str, shapes: &[(&str, &str, &str)], images: &[(&str, Vec<u8>)]) -> Vec<u8> {
    use std::io::Write;
    let esc = |t: &str| t.replace('&', "&amp;");
    let ns = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships""#;
    let rels = |items: &[(String, &str, String)]| {
        let mut s = String::from(r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
        for (id, kind, target) in items {
            s.push_str(&format!(
                r#"<Relationship Id="{id}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="{target}"/>"#
            ));
        }
        s + "</Relationships>"
    };
    let mut vml = String::from(
        r#"<xml xmlns:v="urn:schemas-microsoft-com:vml" xmlns:o="urn:schemas-microsoft-com:office:office"><o:shapelayout v:ext="edit"><o:idmap v:ext="edit" data="1"/></o:shapelayout>"#,
    );
    let mut vml_rels = vec![];
    for (i, (id, style, part)) in shapes.iter().enumerate() {
        vml.push_str(&format!(
            r##"<v:shape id="{id}" o:spid="_x0000_s{}" type="#_x0000_t75" style="{style}"><v:imagedata o:relid="rId{}" o:title="{id}"/></v:shape>"##,
            1025 + i,
            i + 1
        ));
        vml_rels.push((format!("rId{}", i + 1), "image", format!("../media/{part}")));
    }
    vml.push_str("</xml>");
    let sheet = format!(
        r#"<?xml version="1.0"?><worksheet {ns}><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Quote</t></is></c></row></sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/><headerFooter><oddHeader>{}</oddHeader><oddFooter>{}</oddFooter></headerFooter><legacyDrawingHF r:id="rId1"/></worksheet>"#,
        esc(header),
        esc(footer)
    );
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        (
            "[Content_Types].xml".into(),
            br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Default Extension="vml" ContentType="application/vnd.openxmlformats-officedocument.vmlDrawing"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec(),
        ),
        ("_rels/.rels".into(), rels(&[("rId1".into(), "officeDocument", "xl/workbook.xml".into())]).into_bytes()),
        ("xl/workbook.xml".into(), format!(r#"<?xml version="1.0"?><workbook {ns}><sheets><sheet name="Quote" sheetId="1" r:id="rId1"/></sheets></workbook>"#).into_bytes()),
        ("xl/_rels/workbook.xml.rels".into(), rels(&[("rId1".into(), "worksheet", "worksheets/sheet1.xml".into())]).into_bytes()),
        ("xl/worksheets/sheet1.xml".into(), sheet.into_bytes()),
        ("xl/worksheets/_rels/sheet1.xml.rels".into(), rels(&[("rId1".into(), "vmlDrawing", "../drawings/vmlDrawing1.vml".into())]).into_bytes()),
        ("xl/drawings/vmlDrawing1.vml".into(), vml.into_bytes()),
        ("xl/drawings/_rels/vmlDrawing1.vml.rels".into(), rels(&vml_rels).into_bytes()),
    ];
    parts.extend(images.iter().map(|(name, data)| (format!("xl/media/{name}"), data.clone())));
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in &parts {
        z.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap().into_inner()
}

/// A session with `bytes` opened as an XLSX file.
fn open_xlsx(bytes: &[u8]) -> Session {
    let (wb, _) = gridcraft_xlsx::read_xlsx(bytes).unwrap();
    let mut s = Session::new();
    s.add_document(crate::DocState::new(wb, None, "Quote.xlsx".into(), crate::Trust::FromFile));
    s
}

/// The `q w 0 0 h x y cm /…` placements of the images a PDF draws, as text.
fn image_placements(pdf: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(pdf);
    text.lines().filter(|l| l.starts_with("q ") && l.contains(" cm /")).map(|l| l.split(" cm ").next().unwrap_or("").to_string()).collect()
}

/// An XLSX with a logo in the left header and another picture in the right footer prints both at
/// their size and place, and prints the same after one and two saves.
#[test]
fn an_xlsx_letterhead_and_footer_picture_print_and_survive_a_save() {
    // A logo in the left header and a different picture in the right footer, as Excel saves them.
    let (logo, stamp) = (png(30, 10), png(6, 2));
    let bytes = hf_package(
        "&L&G&CQuote",
        "&R&G",
        &[("LH", "position:absolute;width:120pt;height:40pt;z-index:1", "image1.png"), ("RF", "width:60pt;height:20pt", "image2.png")],
        &[("image1.png", logo.clone()), ("image2.png", stamp.clone())],
    );
    let mut s = open_xlsx(&bytes);
    let (pdf, _) = pdf_of(&mut s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    // Two distinct pictures, each at its own size and place: the logo's top on the header margin
    // at the left margin (Letter, 0.7 in sides, 0.3 in header), the stamp's bottom on the footer
    // margin against the right margin.
    assert_eq!(text.matches("/Subtype /Image").count(), 2, "{text}");
    assert!(text.contains("/Width 30 /Height 10") && text.contains("/Width 6 /Height 2"), "{text}");
    assert_eq!(image_placements(&pdf), ["q 120 0 0 40 50.4 730.4", "q 60 0 0 20 501.6 21.6"]);
    assert!(text.contains("(Quote) Tj"));
    // Saved and opened again, the same pictures print in the same places (twice: what one save
    // wrote is what the next one keeps).
    let wb = s.doc().unwrap().wb.clone();
    let once = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (again, _) = gridcraft_xlsx::read_xlsx(&once).unwrap();
    let twice = gridcraft_xlsx::write_xlsx(&again).unwrap();
    for saved in [once, twice] {
        let mut t = open_xlsx(&saved);
        let (pdf2, _) = pdf_of(&mut t, json!({}));
        assert_eq!(image_placements(&pdf2), image_placements(&pdf));
        assert_eq!(String::from_utf8_lossy(&pdf2).matches("/Subtype /Image").count(), 2);
    }
}

/// `&LBefore&Gafter`: the picture sits right after "Before" and "after" right after the picture, on
/// one line.
#[test]
fn a_picture_sits_between_the_text_around_its_g() {
    let bytes = hf_package("&LBefore&Gafter", "", &[("LH", "width:20pt;height:10pt", "image1.png")], &[("image1.png", png(4, 2))]);
    let mut s = open_xlsx(&bytes);
    let (pdf, _) = pdf_of(&mut s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    // The x of a text run: the `… x y Tm` line of the text object that draws it.
    let run_x = |run: &str| -> f32 {
        let at = text.find(&format!("({run}) Tj")).unwrap_or_else(|| panic!("no {run} in {text}"));
        let tm = text[..at].rfind(" Tm").unwrap();
        let line = text[..tm].rsplit('\n').next().unwrap();
        line.split(' ').nth(4).unwrap().parse().unwrap()
    };
    let placed = image_placements(&pdf);
    assert_eq!(placed.len(), 1, "{text}");
    let image_x: f32 = placed[0].split(' ').nth(5).unwrap().parse().unwrap();
    let (before, after) = (run_x("Before"), run_x("after"));
    // One line: "Before", the picture right after it, then "after" right after the picture.
    let w = gridcraft_pdf::text_width(Font::styled(gridcraft_pdf::Family::Sans, false, false), 10.0, "Before");
    assert!((before - 50.4).abs() < 0.01, "{before}");
    assert!((image_x - (before + w)).abs() < 0.01, "picture at {image_x}, text ends at {}", before + w);
    assert!((after - (image_x + 20.0)).abs() < 0.01, "after at {after}");
}

/// A picture relationship naming a missing image part: the workbook opens and prints its text, the
/// drawing is reported as dropped, and the saved file has no dangling relationship.
#[test]
fn a_missing_picture_part_is_reported_and_the_rest_prints_and_saves() {
    // The logo's relationship names a part the package doesn't have; the footer picture is fine.
    let bytes = hf_package(
        "&L&G&CQuote",
        "&R&G",
        &[("LH", "width:120pt;height:40pt", "missing.png"), ("RF", "width:60pt;height:20pt", "image2.png")],
        &[("image2.png", png(6, 2))],
    );
    let (wb, report) = gridcraft_xlsx::read_xlsx(&bytes).expect("the workbook still opens");
    assert_eq!(wb.sheet(0).unwrap().print.header, "&L&G&CQuote");
    let mut s = Session::new();
    s.add_document(crate::DocState::new(wb, None, "Quote.xlsx".into(), crate::Trust::FromFile));
    let (pdf, _) = pdf_of(&mut s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    // The header and cell text print and nothing crashes. A drawing with a part missing isn't
    // kept (writing it back would leave a dangling relationship Excel offers to repair), so no
    // picture of it prints, and the report says so.
    assert!(text.contains("(Quote) Tj"));
    assert!(!text.contains("/Subtype /Image"), "{text}");
    assert!(
        report.warnings.iter().any(|w| w.contains("header and footer pictures were dropped") && w.contains("missing.png")),
        "{:?}",
        report.warnings
    );
    // Saving keeps the sheet and its header text, and opens again.
    let saved = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let (back, _) = gridcraft_xlsx::read_xlsx(&saved).unwrap();
    assert_eq!(back.sheet(0).unwrap().print.header, "&L&G&CQuote");
    assert_eq!(back.sheet(0).unwrap().print.footer, "&R&G");
    let mut t = Session::new();
    t.add_document(crate::DocState::new(back, None, "Quote.xlsx".into(), crate::Trust::FromFile));
    let (pdf2, _) = pdf_of(&mut t, json!({}));
    assert!(!contains(&pdf2, "/Subtype /Image"));
    // The saved package has no relationship to the missing part.
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(saved)).unwrap();
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).unwrap();
        let mut body = String::new();
        if f.name().ends_with(".rels") && std::io::Read::read_to_string(&mut f, &mut body).is_ok() {
            assert!(!body.contains("missing.png"), "{body}");
        }
    }
}

/// Only the page kinds the printed pages use are decoded: a one-page sheet with first and even
/// pages on decodes only its first-page picture.
#[test]
fn only_page_kinds_the_printed_pages_use_are_decoded() {
    let mut s = s();
    // One page, Different First Page and Different Odd & Even Pages on: that page is the first
    // page, so neither the odd-page nor the even-page picture prints.
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute(
        "pageLayout.headerFooter",
        json!({"header": "&L&G", "firstHeader": "&C&G", "evenHeader": "&R&G", "differentFirst": true, "differentOddEven": true}),
    )
    .unwrap();
    keep_hf_pictures(
        &mut s,
        &[
            ("LH", "width:20pt;height:20pt", png(4, 4)),
            ("CHFIRST", "width:20pt;height:20pt", png(5, 5)),
            ("RHEVEN", "width:20pt;height:20pt", png(6, 6)),
        ],
    );
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(pages, 1);
    assert_eq!(text.matches("/Subtype /Image").count(), 1, "{text}");
    assert!(text.contains("/Width 5 /Height 5"), "only the first-page picture");
}

/// The same picture on several sheets is decoded once and is one image object in the PDF.
#[test]
fn the_same_picture_on_several_sheets_is_one_image_object() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "one"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&L&G"})).unwrap();
    let logo = png(8, 4);
    keep_hf_pictures(&mut s, &[("LH", "width:40pt;height:20pt", logo.clone())]);
    // A second sheet with the same logo, read from its own part (bytes equal, not shared).
    s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "two"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&L&G"})).unwrap();
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let mut pictures = wb.sheet(0).unwrap().print.hf_pictures.clone();
    for p in &mut pictures {
        p.data = std::sync::Arc::new(p.data.to_vec());
    }
    let two = wb.sheet_index("Two").unwrap();
    wb.sheet_mut(two).unwrap().print.hf_pictures = pictures;
    let (pdf, pages) = pdf_of(&mut s, json!({"sheets": "all"}));
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(pages, 2);
    assert_eq!(text.matches("/Subtype /Image").count(), 1, "decoded and written once for both sheets");
    assert_eq!(text.matches(" cm /Sh1 Do").count(), 2, "drawn on both sheets' pages");
}

/// A picture over the budget at one size is still tried at a smaller size, while an unreadable one
/// is tried only once.
#[test]
fn a_picture_over_the_budget_at_one_size_still_prints_smaller() {
    use super::header_footer::HfImageCache;
    let mut doc = PdfDoc::new();
    let data: std::sync::Arc<Vec<u8>> = png(200, 200).into();
    // Room for 60 × 60 pixels: 200 × 200 doesn't fit, 50 × 50 does.
    let mut cache = HfImageCache::with_budget(60 * 60 * 4);
    assert!(cache.image(&mut doc, &data, (200, 200)).is_none(), "over the budget");
    assert!(cache.image(&mut doc, &data, (300, 300)).is_none(), "larger still: not tried again");
    assert_eq!(cache.tries(), 1);
    let small = cache.image(&mut doc, &data, (50, 50)).expect("a smaller size fits what is left");
    assert_eq!(cache.image(&mut doc, &data, (40, 40)), Some(small), "a smaller request reuses it");
    assert_eq!(cache.tries(), 2);
    // An image that can't be decoded is tried once, whatever the size.
    let broken: std::sync::Arc<Vec<u8>> = b"\x89PNG\r\n\x1a\nbroken".to_vec().into();
    assert!(cache.image(&mut doc, &broken, (100, 100)).is_none());
    assert!(cache.image(&mut doc, &broken, (10, 10)).is_none());
    assert_eq!(cache.tries(), 3);
}

/// A picture whose pixels at the printed size don't fit the budget is refused from its header
/// alone, before decoding.
#[test]
fn a_picture_over_the_budget_is_refused_before_decoding() {
    use super::header_footer::{Outcome, decode_limited};
    // Only the PNG header: its size is known, but decoding it would fail.
    let mut head = png(4000, 3000);
    head.truncate(33);
    let pixels = 4000 * 3000 * 4;
    assert_eq!(decode_limited(&head, 4000, 3000, pixels - 1).err(), Some(Outcome::OverBudget), "refused from the header alone");
    assert_eq!(decode_limited(&head, 4000, 3000, pixels).err(), Some(Outcome::Unreadable), "within the budget it is decoded, and fails");
    // The budget counts the size it prints at, not the source's.
    assert_eq!(decode_limited(&head, 400, 400, 400 * 300 * 4).err(), Some(Outcome::Unreadable));
    assert_eq!(decode_limited(&head, 400, 400, 400 * 300 * 4 - 1).err(), Some(Outcome::OverBudget));
    // A whole image right at the budget decodes.
    let (rgba, w, h) = decode_limited(&png(300, 200), 150, 150, 150 * 100 * 4).expect("fits exactly");
    assert_eq!((rgba.len(), w, h), (150 * 100 * 4, 150, 100));
}

/// The size checked against the budget is the size `thumbnail` gives, over awkward ratios.
#[test]
fn the_size_checked_against_the_budget_is_the_size_decoding_gives() {
    use super::header_footer::thumbnail_size;
    for (src, max) in [
        ((300, 200), (150, 150)),
        ((1001, 333), (97, 500)),
        ((7, 4000), (3, 101)),
        ((2049, 1365), (640, 427)),
        ((5, 5), (1, 1)),
        ((8192, 3), (100, 100)),
    ] {
        let img = image::DynamicImage::new_rgba8(src.0, src.1).thumbnail(max.0, max.1);
        assert_eq!(thumbnail_size(src, max), (img.width(), img.height()), "{src:?} into {max:?}");
    }
}

/// A picture smaller than its box keeps its own pixels, which serve a larger box too: the same
/// logo asked for at two sizes it fits in is decoded once and is one PDF image, while a picture
/// that was shrunk is decoded again for a larger box.
#[test]
fn a_picture_decoded_at_its_own_size_serves_every_larger_box() {
    use super::header_footer::HfImageCache;
    let mut doc = PdfDoc::new();
    let mut cache = HfImageCache::new();
    let logo: std::sync::Arc<Vec<u8>> = png(200, 100).into();
    let first = cache.image(&mut doc, &logo, (230, 230)).expect("decodes");
    assert_eq!(cache.image(&mut doc, &logo, (250, 250)), Some(first), "the same image for a larger box");
    assert_eq!(cache.tries(), 1);
    let big: std::sync::Arc<Vec<u8>> = png(400, 200).into();
    let small = cache.image(&mut doc, &big, (100, 100)).expect("decodes shrunk");
    let larger = cache.image(&mut doc, &big, (300, 300)).expect("decodes larger");
    assert_ne!(small, larger, "a shrunk picture is decoded again for a larger box");
    assert_eq!(cache.tries(), 3);
}

/// A section's lines split around its `&G`, and `&G` takes no room when there is no picture to
/// draw.
#[test]
fn section_lines_split_around_the_picture() {
    let cx = HfContext::default();
    let [l, _, r] = expand_header_footer("&LTop\nA&GB&RNo&G", &cx);
    let lines = section_lines(&l, true);
    assert_eq!(
        lines,
        [HfLine { before: "Top".into(), after: String::new(), picture: false }, HfLine { before: "A".into(), after: "B".into(), picture: true },]
    );
    // Without a picture to draw, &G takes no room.
    assert_eq!(section_lines(&l, false)[1], HfLine { before: "AB".into(), after: String::new(), picture: false });
    assert_eq!(section_lines(&r, true), [HfLine { before: "No".into(), after: String::new(), picture: true }]);
}

/// Each print preview page carries its page kind and its header and footer geometry.
#[test]
fn print_preview_pages_carry_their_header_footer_geometry() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("cell.set", json!({"cell": "A200", "input": "y"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&CTitle", "differentFirst": true, "differentOddEven": true})).unwrap();
    let pv = s.execute("file.printPreview", json!({})).unwrap();
    let pages = pv["pageList"].as_array().unwrap();
    assert!(pages.len() >= 3);
    let kinds: Vec<&str> = pages.iter().take(3).map(|p| p["hfPage"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["first", "even", "odd"]);
    // Letter, default margins: header and footer 0.3 in, sides 0.7 in, text at 100%.
    let p = &pages[0];
    let near = |v: &Json, want: f64| v.as_f64().is_some_and(|v| (v - want).abs() < 1e-3);
    assert!(near(&p["headerMargin"], 21.6) && near(&p["footerMargin"], 21.6), "{p}");
    assert!(near(&p["hfSides"][0], 50.4) && near(&p["hfSides"][1], 50.4), "{p}");
    assert!(near(&p["hfScale"], 1.0));
}

/// `layout_header_footer` places pictures and text where printing draws them, so the preview and
/// the PDF agree.
#[test]
fn header_footer_layout_is_what_printing_draws() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&L&G&CInvoice", "footer": "&CPage &P &G"})).unwrap();
    let d = s.doc().unwrap();
    let lay = layout_sheet(&d.wb, 0, &PrintOptions::default(), None).unwrap();
    let cx = HfContext { page: 1, pages: 1, ..HfContext::default() };
    let size = |footer: bool, section: usize| match (footer, section) {
        (false, 0) => Some((120.0, 40.0)),
        (true, 1) => Some((80.0, 20.0)),
        _ => None,
    };
    let items = layout_header_footer(&lay.hf_frame(), &lay.settings, gridcraft_model::HfPage::Odd, &cx, size);
    // The letterhead's top on the header margin, the footer picture's bottom on the footer
    // margin, as `header_and_footer_pictures_print_where_their_g_is` finds them in the PDF.
    let at = |x: f32, want: f32| (x - want).abs() < 1e-3;
    assert!(
        items.iter().any(|i| matches!(i, HfItem::Picture { x, y, width, height, footer: false, section: 0 }
            if at(*x, 50.4) && at(*y, 21.6) && at(*width, 120.0) && at(*height, 40.0))),
        "{items:?}"
    );
    assert!(
        items.iter().any(|i| matches!(i, HfItem::Picture { footer: true, section: 1, y, height, .. } if (y + height - (792.0 - 21.6)).abs() < 1e-3))
    );
    assert!(items.iter().any(|i| matches!(i, HfItem::Text { text, .. } if text == "Invoice")));
    assert!(items.iter().any(|i| matches!(i, HfItem::Text { text, .. } if text == "Page 1 ")));
    // Without pictures to draw, &G takes no room and nothing else is laid out.
    let plain = layout_header_footer(&lay.hf_frame(), &lay.settings, gridcraft_model::HfPage::Odd, &cx, |_, _| None);
    assert!(plain.iter().all(|i| matches!(i, HfItem::Text { .. })));
    assert_eq!(plain.len(), 2);
}

// ---------------------------------------------------------------- pageLayout.headerFooterPicture

/// A `w` × `h` PNG of one colour, in base64.
fn b64_png(w: u32, h: u32) -> String {
    crate::io::base64_encode(&png(w, h))
}

/// The first sheet's print settings.
fn print_of(s: &Session) -> PrintSettings {
    s.doc().unwrap().wb.sheet(0).unwrap().print.clone()
}

/// `pageLayout.headerFooterPicture` puts the picture in its section with `&G`, keeping its
/// proportions and fitting the margin, in one undo step.
#[test]
fn a_header_picture_is_inserted_with_its_g_and_fits_the_margin() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("pageLayout.headerFooter", json!({"header": "&CTitle"})).unwrap();
    let r = s.execute("pageLayout.headerFooterPicture", json!({"section": "center", "base64": b64_png(200, 100), "title": "Logo"})).unwrap();
    // 200 × 100 px is 150 × 75 pt; the default top margin leaves 0.45 in (32.4 pt) above the
    // header margin, so it shrinks to fit, keeping its proportions.
    let near = |v: &Json, want: f64| v.as_f64().is_some_and(|v| (v - want).abs() < 0.01);
    assert!(near(&r["width"], 64.8) && near(&r["height"], 32.4), "{r}");
    assert_eq!(r["header"], "&C&GTitle");
    let pr = print_of(&s);
    assert_eq!(pr.hf_pictures.len(), 1);
    assert_eq!((pr.hf_pictures[0].section, pr.hf_pictures[0].mime.as_str(), pr.hf_pictures[0].title.as_str()), (1, "image/png", "Logo"));
    let (pdf, _) = pdf_of(&mut s, json!({}));
    assert!(contains(&pdf, "/Subtype /Image") && contains(&pdf, "(Title) Tj"));
    // One undo step takes it all back.
    s.execute("edit.undo", json!({})).unwrap();
    let pr = print_of(&s);
    assert!(pr.hf_pictures.is_empty());
    assert_eq!(pr.header, "&CTitle");
}

/// A picture is sized as asked (one side keeps the proportions), replaces the one in its section,
/// can go on the first page only, and `remove` takes it and its `&G` away.
#[test]
fn header_pictures_are_sized_replaced_and_removed() {
    let mut s = s();
    // One side given: the other keeps the proportions.
    let r =
        s.execute("pageLayout.headerFooterPicture", json!({"part": "footer", "section": "right", "base64": b64_png(40, 20), "width": 60})).unwrap();
    assert_eq!((r["width"].as_f64(), r["height"].as_f64(), r["footer"].as_str()), (Some(60.0), Some(30.0), Some("&R&G")));
    // Both sides given; the second picture replaces the first, and `&G` isn't doubled.
    s.execute("pageLayout.headerFooterPicture", json!({"part": "footer", "section": "right", "base64": b64_png(5, 5), "width": 10, "height": 20}))
        .unwrap();
    let pr = print_of(&s);
    assert_eq!(pr.hf_pictures.len(), 1);
    assert_eq!((pr.hf_pictures[0].width, pr.hf_pictures[0].height), (10.0, 20.0));
    assert_eq!(pr.footer, "&R&G");
    // First-page pictures are their own.
    s.execute("pageLayout.headerFooterPicture", json!({"page": "first", "section": "left", "base64": b64_png(5, 5)})).unwrap();
    assert_eq!(print_of(&s).hf_pictures.len(), 2);
    assert_eq!(print_of(&s).first_header, "&L&G");
    // Removing takes the picture and its &G, and leaves the rest of the text.
    s.execute("pageLayout.headerFooter", json!({"part": "footer", "section": "right", "text": "&[Picture] Page &[Page] &&G"})).unwrap();
    s.execute("pageLayout.headerFooterPicture", json!({"part": "footer", "section": "right", "remove": true})).unwrap();
    let pr = print_of(&s);
    assert_eq!(pr.footer, "&R Page &P &&G");
    assert_eq!(pr.hf_pictures.len(), 1, "the first-page picture stays");
}

/// Editing a section so it loses `&G` drops that section's picture and only that one.
#[test]
fn erasing_the_picture_field_drops_its_picture() {
    let mut s = s();
    s.execute("pageLayout.headerFooterPicture", json!({"section": "left", "base64": b64_png(5, 5)})).unwrap();
    s.execute("pageLayout.headerFooterPicture", json!({"section": "right", "base64": b64_png(6, 6)})).unwrap();
    // Editing the left section without &[Picture] (as the in-place editor would commit it).
    s.execute("pageLayout.headerFooter", json!({"part": "header", "section": "left", "text": "Company"})).unwrap();
    let pr = print_of(&s);
    assert_eq!(pr.hf_pictures.len(), 1);
    assert_eq!(pr.hf_pictures[0].section, 2);
    assert_eq!(pr.header, "&LCompany&R&G");
}

/// Hostile input is refused (no picture, bad base64, not an image, a bad section, part or page, a
/// missing file, over 16 MB) and hostile sizes are clamped.
#[test]
fn header_picture_input_is_checked() {
    let mut s = s();
    let run = |s: &mut Session, p: Json| s.execute("pageLayout.headerFooterPicture", p);
    assert!(run(&mut s, json!({})).is_err(), "no picture");
    assert!(run(&mut s, json!({"base64": "%%%"})).is_err(), "bad base64");
    assert!(run(&mut s, json!({"base64": crate::io::base64_encode(b"not a picture")})).is_err());
    assert!(run(&mut s, json!({"base64": b64_png(5, 5), "section": "top"})).is_err());
    assert!(run(&mut s, json!({"base64": b64_png(5, 5), "part": "side"})).is_err());
    assert!(run(&mut s, json!({"base64": b64_png(5, 5), "page": "last"})).is_err());
    assert!(run(&mut s, json!({"path": "/no/such/file.png"})).is_err());
    let mut huge = png(5, 5);
    huge.resize(17 * 1024 * 1024, 0);
    assert!(run(&mut s, json!({"base64": crate::io::base64_encode(&huge)})).is_err(), "over 16 MB");
    // Hostile sizes are clamped.
    let r = run(&mut s, json!({"base64": b64_png(5, 5), "width": 1e30, "height": -4})).unwrap();
    assert_eq!(r["width"].as_f64(), Some(1584.0));
    assert!(print_of(&s).hf_pictures.iter().all(|p| p.width.is_finite() && p.height.is_finite() && p.height > 0.0));
}

/// A picture inserted here survives saving as XLSX and reading it back.
#[test]
fn an_inserted_header_picture_survives_saving_as_xlsx() {
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "x"})).unwrap();
    s.execute("pageLayout.headerFooterPicture", json!({"section": "left", "base64": b64_png(30, 10), "width": 90})).unwrap();
    let wb = s.doc().unwrap().wb.clone();
    let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
    let (back, rep) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    assert!(!rep.warnings.iter().any(|w| w.contains("header")), "{:?}", rep.warnings);
    let (a, b) = (&wb.sheet(0).unwrap().print, &back.sheet(0).unwrap().print);
    assert_eq!(b.header, "&L&G");
    assert_eq!(b.hf_pictures, a.hf_pictures);
}

/// A sheet that keeps a header/footer drawing from its file (one GridCraft couldn't model)
/// refuses new pictures: the file's drawing is written back as it was, and a sheet has one.
#[test]
fn a_sheet_keeping_its_files_header_drawing_refuses_new_pictures() {
    use gridcraft_model::{Preserved, PreservedXml};
    let mut s = s();
    let kept = Preserved {
        elements: vec![PreservedXml { slot: "legacyDrawingHF".into(), xml: "<legacyDrawingHF r:id=\"rId9\"/>".into() }],
        ..Preserved::default()
    };
    let d = s.doc_mut().unwrap();
    std::sync::Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap().preserved = Some(std::sync::Arc::new(kept));
    let err = s.execute("pageLayout.headerFooterPicture", json!({"section": "left", "base64": b64_png(5, 5)})).unwrap_err();
    assert!(err.to_string().contains("can't edit"), "{err}");
    let sh = s.doc().unwrap().wb.sheet(0).unwrap();
    assert!(sh.print.hf_pictures.is_empty() && !sh.print.header.contains("&G"));
}
