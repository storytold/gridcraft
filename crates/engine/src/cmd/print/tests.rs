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
fn header_font_runs_capture_styles_fields_and_bounded_expansion() {
    let cx = HfContext { page: 3, sheet: "Ω".into(), ..Default::default() };
    let sections = expand_header_footer("&C&\"Greek,Bold Italic\"&20&A&\"Latin,Regular\"&10A&P&BZ&IY&LLeft", &cx);
    let runs = &sections[1].runs;
    assert_eq!(sections[1].text, "ΩA3ZY");
    assert_eq!(runs.len(), 4);
    assert_eq!(
        (runs[0].text.as_str(), runs[0].font.as_deref(), runs[0].bold, runs[0].italic, runs[0].size),
        ("Ω", Some("Greek"), true, true, Some(20.0))
    );
    assert_eq!(
        (runs[1].text.as_str(), runs[1].font.as_deref(), runs[1].bold, runs[1].italic, runs[1].size),
        ("A3", Some("Latin"), false, false, Some(10.0))
    );
    assert!(runs[2].bold && !runs[2].italic);
    assert!(runs[3].bold && runs[3].italic);
    assert_eq!(sections[0].runs[0].text, "Left");
    let huge = HfContext { sheet: "Ω".repeat(100_000), ..Default::default() };
    let expanded = expand_header_footer("&A&A&A", &huge);
    assert_eq!(expanded[1].text.len(), 40_000);
    assert_eq!(expanded[1].runs.len(), 1);
    assert_eq!(expanded[1].runs[0].text, expanded[1].text);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn header_and_footer_font_switches_measure_and_align_each_run() {
    let dir = std::env::temp_dir().join(format!("gridcraft-header-font-runs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let greek = dir.join("greek.ttf");
    let greek_bold = dir.join("greek-bold.ttf");
    let latin = dir.join("latin.ttf");
    for (path, chars, width) in [(&greek, vec!['Ω'], 400), (&greek_bold, vec!['Ω'], 700), (&latin, vec!['A'], 300)] {
        std::fs::write(path, gridcraft_pdf::test_support::tiny_ttf(&chars, width, 0)).unwrap();
    }
    let mut s = s();
    for part in ["header", "footer"] {
        for section in ["&L", "&C", "&R"] {
            let spec = format!("{section}&\"Greek,Bold\"&20Ω&\"Latin,Regular\"&10A");
            let mut setup = json!({"scale": 100, "fitToPage": false, "header": "", "footer": ""});
            setup[part] = json!(spec);
            s.execute("pageLayout.pageSetup", setup).unwrap();
            let lay = layouts(&s.doc().unwrap().wb, &PrintOptions::default()).remove(0);
            let (left, right) = lay.hf_sides();
            let expected = match section {
                "&L" => left,
                "&C" => (lay.page_w - 17.0) / 2.0,
                _ => lay.page_w - right - 17.0,
            };
            for command in ["file.exportPdf", "file.print"] {
                let r = s
                    .execute_headless(
                        command,
                        json!({"fontFamilies": {"Greek": {"regular": greek, "bold": greek_bold}, "Latin": {"regular": latin}}}),
                    )
                    .unwrap();
                let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
                let pdf = String::from_utf8_lossy(&bytes);
                let runs: Vec<_> = pdf.split("BT\n").filter(|b| b.contains("/U6 20 Tf") || b.contains("/U9 10 Tf")).collect();
                assert_eq!(runs.len(), 2, "each header/footer character must use its own mapped face");
                for (run, x) in runs.iter().zip([expected, expected + 14.0]) {
                    let tm = run.lines().find(|line| line.ends_with(" Tm")).unwrap();
                    let actual: f32 = tm.split_whitespace().nth(4).unwrap().parse().unwrap();
                    assert!((actual - x).abs() < 0.01, "{part} {section}: expected {x}, got {actual}");
                }
                assert_eq!(bytes.windows(10).filter(|w| *w == b"/FontFile2").count(), 2);
            }
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
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
fn custom_font_widths_control_line_wrapping() {
    let regular = std::sync::Arc::new(UnicodeFont::parse(gridcraft_pdf::test_support::tiny_ttf(&['A', ' '], 1000, 0)).unwrap());
    let fonts = TextFonts::custom(regular, None, None, None);
    assert_eq!(wrap_lines(&fonts, "AA AA", Font::Helvetica, 10.0, 35.0), ["AA", "AA"]);
    assert_eq!(wrap_lines(&TextFonts::default(), "AA AA", Font::Helvetica, 10.0, 35.0), ["AA AA"]);
    assert_eq!(wrap_lines(&fonts, "AAAA", Font::HelveticaBoldOblique, 10.0, 25.0), ["AA", "AA"]);
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn custom_pdf_fonts_are_per_export_and_fail_before_writing() {
    let dir = std::env::temp_dir().join(format!("gridcraft-custom-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let font = dir.join("regular.ttf");
    let out = dir.join("out.pdf");
    std::fs::write(&font, gridcraft_pdf::test_support::tiny_ttf(&['A', ' '], 400, 0)).unwrap();
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "A"})).unwrap();
    for cmd in ["file.exportPdf", "file.print"] {
        let r = s.execute_headless(cmd, json!({"fonts": {"regular": font}})).unwrap();
        let bytes = crate::io::base64_decode(r["base64"].as_str().unwrap()).unwrap();
        assert!(bytes.windows(10).any(|w| w == b"/FontFile2"));
    }
    let (plain, _) = pdf_of(&mut s, json!({}));
    assert!(!plain.windows(10).any(|w| w == b"/FontFile2"), "custom faces must not leak into the next export");
    for fonts in [
        json!(null),
        json!([]),
        json!({}),
        json!({"regular": 7}),
        json!({"regular": ""}),
        json!({"regular": font, "typo": font}),
        json!({"regular": font, "bold": false}),
    ] {
        assert!(s.execute_headless("file.exportPdf", json!({"path": out, "fonts": fonts})).is_err(), "{fonts}");
        assert!(!out.exists());
    }
    s.execute("cell.set", json!({"cell": "A1", "input": "Ω"})).unwrap();
    let err = s.execute_headless("file.exportPdf", json!({"path": out, "fonts": {"regular": font}})).unwrap_err().to_string();
    assert!(err.contains("U+03A9"), "{err}");
    assert!(!out.exists());
    for bytes in [b"not a font".to_vec(), gridcraft_pdf::test_support::tiny_ttf(&['A'], 500, 2)] {
        std::fs::write(&font, bytes).unwrap();
        assert!(s.execute_headless("file.exportPdf", json!({"path": out, "fonts": {"regular": font}})).is_err());
        assert!(!out.exists());
    }
    // An oversized sparse file is refused within the font reader's own cap.
    std::fs::File::create(&font).unwrap().set_len(MAX_FONT_BYTES as u64 + 1).unwrap();
    let err = s.execute_headless("file.exportPdf", json!({"fonts": {"regular": font}})).unwrap_err().to_string();
    assert!(err.contains("limit"), "{err}");
    let sandbox = crate::io::ScriptSandbox::enter();
    let err = s.execute_headless("file.exportPdf", json!({"fonts": {"regular": font}})).unwrap_err().to_string();
    assert!(err.contains("can't read or write files"), "{err}");
    drop(sandbox);
    assert!(s.take_ui_requests().is_empty());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn xlsx_pdf_export_preserves_named_families_and_styles() {
    let dir = std::env::temp_dir().join(format!("gridcraft-family-fonts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut paths = Vec::new();
    for width in [400, 500, 800, 900] {
        let path = dir.join(format!("face-{width}.ttf"));
        std::fs::write(&path, gridcraft_pdf::test_support::tiny_ttf(&['A'], width, 0)).unwrap();
        paths.push(path);
    }
    let families = json!({"Body Sans": {"regular": paths[0], "bold": paths[1]}, "Heading Sans": {"regular": paths[2], "italic": paths[3]}});
    let mut source = s();
    source.execute("range.setValues", json!({"range": "A1:B2", "values": [["A", "A"], ["A", "A"]]})).unwrap();
    source.execute("home.fontName", json!({"range": "A1:A2", "name": "Body Sans"})).unwrap();
    source.execute("home.fontName", json!({"range": "B1:B2", "name": "Heading Sans"})).unwrap();
    source.execute("home.bold", json!({"range": "A2", "on": true})).unwrap();
    source.execute("home.italic", json!({"range": "B2", "on": true})).unwrap();
    source.execute("pageLayout.pageSetup", json!({"header": "&C&\"Heading Sans,Italic\"A"})).unwrap();
    let book = dir.join("mixed.xlsx");
    std::fs::write(&book, crate::io::save_bytes(&source.doc().unwrap().wb, "mixed.xlsx").unwrap()).unwrap();
    let mut session = Session::new();
    session.execute_headless("file.open", json!({"path": book})).unwrap();
    for cmd in ["file.exportPdf", "file.print"] {
        let result = session.execute_headless(cmd, json!({"fontFamilies": families})).unwrap();
        let bytes = crate::io::base64_decode(result["base64"].as_str().unwrap()).unwrap();
        assert_eq!(bytes.windows(10).filter(|w| *w == b"/FontFile2").count(), 4);
        for slot in [5, 6, 9, 11] {
            assert!(bytes.windows(format!("/U{slot} ").len()).any(|w| w == format!("/U{slot} ").as_bytes()), "face {slot} must survive XLSX import");
        }
    }
    let fonts = export_fonts(&json!({"fontFamilies": families}), "file.exportPdf").unwrap();
    assert_eq!(wrap_lines(&fonts, "AAAA", fonts.font("Body Sans", false, false), 10.0, 20.0), ["AAAA"]);
    assert_eq!(wrap_lines(&fonts, "AAAA", fonts.font("Heading Sans", false, false), 10.0, 20.0), ["AA", "AA"]);
    let (default, _) = pdf_of(&mut session, json!({}));
    assert!(!default.windows(10).any(|w| w == b"/FontFile2"));
    let out = dir.join("keep.pdf");
    std::fs::write(&out, b"existing output").unwrap();
    session.execute("cell.set", json!({"cell": "A1", "input": "Ω"})).unwrap();
    let error = session.execute_headless("file.exportPdf", json!({"path": out, "fontFamilies": families})).unwrap_err().to_string();
    assert!(error.contains("U+03A9"), "{error}");
    for value in [
        json!(null),
        json!([]),
        json!({"Body": {}}),
        json!({" ": {"regular": paths[0]}}),
        json!({"Body": {"regular": paths[0]}, "body": {"regular": paths[0]}}),
    ] {
        assert!(session.execute_headless("file.exportPdf", json!({"path": out, "fontFamilies": value})).is_err());
    }
    let mut many = serde_json::Map::new();
    for i in 0..=MAX_FONT_FAMILIES {
        many.insert(format!("Family {i}"), json!({"regular": paths[0]}));
    }
    assert!(export_fonts(&json!({"fontFamilies": many}), "file.exportPdf").unwrap_err().to_string().contains("32"));
    let mut budget = 1;
    assert!(export_faces(&json!({"regular": paths[0]}), "fontFamilies.Body", "file.exportPdf", &mut budget).is_err());
    assert_eq!(std::fs::read(out).unwrap(), b"existing output");
    std::fs::remove_dir_all(dir).unwrap();
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
    let w = wrap_lines(&TextFonts::default(), "the quick brown fox jumps", Font::Helvetica, 10.0, 50.0);
    assert!(w.len() >= 2);
    assert!(w.iter().all(|l| gridcraft_pdf::text_width(Font::Helvetica, 10.0, l) <= 50.0 || !l.contains(' ')));
    let w = wrap_lines(&TextFonts::default(), "Supercalifragilistic", Font::Helvetica, 10.0, 20.0);
    assert!(w.len() > 2);
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
