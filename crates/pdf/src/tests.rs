use super::*;

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// Parses the xref table back and checks each in-use entry points at `N 0 obj`.
fn check_xref(pdf: &[u8]) -> usize {
    let sx = rfind(pdf, b"startxref\n").unwrap();
    let rest = std::str::from_utf8(&pdf[sx + 10..]).unwrap();
    let off: usize = rest.lines().next().unwrap().trim().parse().unwrap();
    assert!(pdf[off..].starts_with(b"xref\n"), "startxref points at xref");
    let table = std::str::from_utf8(&pdf[off..sx]).unwrap();
    let mut lines = table.lines();
    assert_eq!(lines.next(), Some("xref"));
    let hdr: Vec<usize> = lines.next().unwrap().split(' ').map(|x| x.parse().unwrap()).collect();
    assert_eq!(hdr[0], 0);
    let count = hdr[1];
    let mut checked = 0;
    for i in 0..count {
        let e = lines.next().unwrap();
        assert_eq!(e.len(), 19, "entry `{e}` (20 bytes with newline)");
        let o: usize = e[..10].parse().unwrap();
        if e.trim_end().ends_with('n') {
            let want = format!("{i} 0 obj");
            assert!(pdf[o..].starts_with(want.as_bytes()), "object {i} at {o}");
            checked += 1;
        }
    }
    assert!(table.contains("trailer"));
    assert!(table.contains(&format!("/Size {count}")));
    checked
}

#[test]
fn minimal_document() {
    let pdf = PdfDoc::new().finish().unwrap();
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    assert!(check_xref(&pdf) >= 5);
}

#[test]
fn drawing_and_text() {
    let mut d = PdfDoc::new();
    d.info.title = "Report (Q1)".into();
    let p = d.add_page(612.0, 792.0);
    p.fill_rect(10.0, 10.0, 100.0, 20.0, [255, 255, 0]);
    p.rect_stroke(10.0, 10.0, 100.0, 20.0, 0.5, [0, 0, 0]);
    p.stroke_line(0.0, 0.0, 612.0, 792.0, 1.0, [255, 0, 0]);
    p.stroke_line_dashed(0.0, 5.0, 100.0, 5.0, 1.0, [0, 0, 0], &[3.0, 2.0]);
    p.polygon(&[[0.0, 0.0], [10.0, 0.0], [5.0, 8.0]], Some([0, 0, 255]), Some(([0, 0, 0], 1.0)));
    p.circle(300.0, 300.0, 20.0, Some([0, 128, 0]), None);
    p.save();
    p.clip_rect(0.0, 0.0, 50.0, 50.0);
    p.text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], "Hello (world) \\ café €5");
    p.restore();
    p.text_rotated(100.0, 100.0, 9.0, Font::TimesBold, [0, 0, 0], "Up", -std::f32::consts::FRAC_PI_2);
    let p2 = d.add_page(842.0, 595.0);
    p2.text(10.0, 20.0, 10.0, Font::CourierBold, [0, 0, 0], "Second");
    assert_eq!(d.page_count(), 2);
    let pdf = d.finish().unwrap();
    check_xref(&pdf);
    assert!(find(&pdf, b"(Hello \\(world\\) \\\\ caf\\351 \\2005) Tj").is_some());
    assert!(find(&pdf, b"(Second) Tj").is_some());
    assert!(find(&pdf, b"/BaseFont /Helvetica /Encoding /WinAnsiEncoding").is_some());
    assert!(find(&pdf, b"/BaseFont /Courier-Bold").is_some());
    assert!(find(&pdf, b"/BaseFont /Times-Bold").is_some());
    assert!(find(&pdf, b"/Count 2").is_some());
    assert!(find(&pdf, b"/MediaBox [0 0 842 595]").is_some());
    assert!(find(&pdf, b"/Title (Report \\(Q1\\))").is_some());
    // Top-left origin: a line from (0,0) starts at the top of the page.
    assert!(find(&pdf, b"0 792 m 612 0 l S").is_some());
}

#[test]
fn uri_and_page_links_are_real_pdf_annotations() {
    let mut d = PdfDoc::new();
    let first = d.add_page(200.0, 300.0);
    first.link_uri(10.0, 20.0, 80.0, 30.0, "https://example.org/a(b)?q=£");
    first.link_page(100.0, 40.0, 50.0, 20.0, 1, 35.0, 60.0);
    first.link_uri(-20.0, -20.0, 40.0, 40.0, "mailto:a@example.org");
    first.link_uri(0.0, 0.0, f32::NAN, 10.0, "https://bad.example");
    first.link_uri(0.0, 0.0, 10.0, 10.0, "javascript:alert(1)");
    let second = d.add_page(100.0, 200.0);
    second.link_page(0.0, 0.0, 10.0, 10.0, 99, 0.0, 0.0);
    let pdf = d.finish().unwrap();
    check_xref(&pdf);
    let s = String::from_utf8_lossy(&pdf);
    assert_eq!(s.matches("/Subtype /Link").count(), 3);
    assert!(s.contains("/Annots ["));
    assert!(s.contains("/Rect [10 250 90 280]"));
    assert!(s.contains("/Rect [0 280 20 300]"));
    assert!(s.contains("/URI (https://example.org/a\\(b\\)?q=%C2%A3)"));
    assert!(s.contains("/XYZ 35 140 null"));
    assert!(!s.contains("https://bad.example"));
}

#[test]
fn images_with_alpha() {
    let mut d = PdfDoc::new();
    let p = d.add_page(200.0, 200.0);
    let mut px = Vec::new();
    for i in 0..16u8 {
        px.extend_from_slice(&[i * 10, 0, 0, if i < 8 { 255 } else { 128 }]);
    }
    p.image_rgba(10.0, 10.0, 40.0, 40.0, &px, 4, 4);
    p.image_rgba(10.0, 10.0, 40.0, 40.0, &[255; 16], 2, 2); // opaque: no mask
    p.image_rgba(0.0, 0.0, 1.0, 1.0, &[1, 2, 3], 4, 4); // too short: ignored
    let pdf = d.finish().unwrap();
    check_xref(&pdf);
    assert!(find(&pdf, b"/SMask").is_some());
    assert!(find(&pdf, b"/Im1 Do").is_some());
    assert!(find(&pdf, b"/Im2 Do").is_some());
    assert!(find(&pdf, b"/Im3").is_none());
}

#[test]
fn run_length_roundtrip() {
    fn decode(d: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < d.len() {
            let l = d[i];
            i += 1;
            if l == 128 {
                break;
            } else if l < 128 {
                out.extend_from_slice(&d[i..i + l as usize + 1]);
                i += l as usize + 1;
            } else {
                out.extend(std::iter::repeat_n(d[i], 257 - l as usize));
                i += 1;
            }
        }
        out
    }
    let cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![1],
        vec![1, 1],
        vec![1, 2, 3, 3, 3, 3, 4, 5, 5],
        (0..1000).map(|i| (i / 7) as u8).collect(),
        (0..1000).map(|i| (i * 31 % 251) as u8).collect(),
        vec![9; 300],
    ];
    for c in cases {
        assert_eq!(decode(&run_length(&c)), c);
    }
}

#[test]
fn widths() {
    assert!((text_width(Font::Helvetica, 10.0, "Hello") - 22.78).abs() < 0.01);
    assert!(text_width(Font::HelveticaBold, 10.0, "Hello") > text_width(Font::Helvetica, 10.0, "Hello"));
    assert_eq!(text_width(Font::Courier, 10.0, "abc"), 18.0);
    assert!(text_width(Font::TimesRoman, 12.0, "é") > 0.0);
    assert_eq!(text_width(Font::Helvetica, 10.0, ""), 0.0);
    assert_eq!(Font::family_for("Calibri"), Family::Sans);
    assert_eq!(Font::family_for("Times New Roman"), Family::Serif);
    assert_eq!(Font::family_for("Consolas"), Family::Mono);
    assert_eq!(Font::styled(Family::Serif, true, true), Font::TimesBoldItalic);
}

#[test]
fn hostile_inputs_never_panic() {
    let mut d = PdfDoc::new();
    let p = d.add_page(f32::NAN, -5.0);
    p.text(f32::INFINITY, f32::NAN, 1e30, Font::Helvetica, [0, 0, 0], "x");
    p.fill_rect(0.0, 0.0, -1.0, 5.0, [0, 0, 0]);
    p.polygon(&[], None, None);
    p.circle(0.0, 0.0, -3.0, Some([0, 0, 0]), None);
    p.restore();
    p.restore();
    p.save();
    p.image_rgba(0.0, 0.0, 10.0, 10.0, &[], u32::MAX, u32::MAX);
    let pdf = d.finish().unwrap();
    check_xref(&pdf);
}

fn page_content(d: &PdfDoc) -> &str {
    &d.pages[0].content
}

#[test]
fn text_width_does_not_overflow() {
    // 8,000,000 x 556 units exceeds u32::MAX: a u32 sum panics in debug builds.
    let long = "a".repeat(8_000_000);
    let w = text_width(Font::Helvetica, 10.0, &long);
    assert!(w.is_finite() && w > 0.0);
    assert!(w > text_width(Font::Helvetica, 10.0, &"a".repeat(7_000_000)));
}

#[test]
fn all_zero_dash_is_drawn_solid() {
    let mut d = PdfDoc::new();
    let p = d.add_page(100.0, 100.0);
    p.stroke_line_dashed(0.0, 0.0, 50.0, 0.0, 1.0, [0, 0, 0], &[0.0, 0.0]);
    p.polyline(&[[0.0, 0.0], [5.0, 5.0]], 1.0, [0, 0, 0], &[0.0, 0.0001, f32::NAN]);
    p.stroke_line_dashed(0.0, 0.0, 50.0, 0.0, 1.0, [0, 0, 0], &[3.0, 2.0]);
    let c = page_content(&d);
    assert_eq!(c.matches("[] 0 d").count(), 2);
    assert!(!c.contains("[0 0]"));
    assert!(c.contains("[3 2] 0 d"));
}

#[test]
fn negative_and_nan_stroke_widths_become_zero() {
    let mut d = PdfDoc::new();
    let p = d.add_page(100.0, 100.0);
    p.rect_stroke(1.0, 1.0, 10.0, 10.0, -3.0, [0, 0, 0]);
    p.stroke_line(0.0, 0.0, 5.0, 5.0, f32::NAN, [0, 0, 0]);
    p.polygon(&[[0.0, 0.0], [5.0, 0.0], [5.0, 5.0]], None, Some(([0, 0, 0], -1.0)));
    p.circle(20.0, 20.0, 5.0, None, Some(([0, 0, 0], f32::NEG_INFINITY)));
    let c = page_content(&d);
    assert_eq!(c.matches("\n0 w ").count(), 4);
    assert!(!c.contains("-1 w") && !c.contains("-3 w"));
}

#[test]
fn non_finite_image_size_stores_nothing() {
    let mut d = PdfDoc::new();
    let p = d.add_page(200.0, 200.0);
    let px = [255u8; 16];
    for (w, h) in [(f32::NAN, 10.0), (10.0, f32::NAN), (f32::INFINITY, 10.0), (-1.0, 10.0)] {
        p.image_rgba(0.0, 0.0, w, h, &px, 2, 2);
    }
    p.image_rgba(f32::NAN, 0.0, 10.0, 10.0, &px, 2, 2);
    assert!(p.images.is_empty());
    assert!(p.content.is_empty());
}

#[test]
fn every_draw_method_survives_hostile_numbers() {
    let bad = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -7.0, 0.0, 1.0e30, -1.0e30];
    let px = [200u8; 16];
    let mut d = PdfDoc::new();
    let p = d.add_page(300.0, 300.0);
    for &a in &bad {
        for &b in &bad {
            p.clip_rect(a, b, a, b);
            p.fill_rect(a, b, b, a, [1, 2, 3]);
            p.rect_stroke(a, b, a, b, a, [1, 2, 3]);
            p.stroke_line(a, b, b, a, a, [1, 2, 3]);
            p.stroke_line_dashed(a, b, b, a, b, [1, 2, 3], &[a, b, 0.0]);
            p.polyline(&[[a, b], [b, a], [1.0, 1.0]], a, [1, 2, 3], &[b, a]);
            p.polygon(&[[a, b], [b, a], [1.0, 1.0]], Some([1, 2, 3]), Some(([4, 5, 6], a)));
            p.circle(a, b, b, Some([1, 2, 3]), Some(([4, 5, 6], a)));
            p.text(a, b, a, Font::Helvetica, [0, 0, 0], "x");
            p.text_rotated(a, b, 10.0, Font::TimesRoman, [0, 0, 0], "y", b);
            p.image_rgba(a, b, b, a, &px, 2, 2);
        }
    }
    p.stroke_line_dashed(0.0, 0.0, 1.0, 1.0, 1.0, [0, 0, 0], &[0.0; 100]);
    let c = page_content(&d);
    assert!(!c.contains("NaN") && !c.contains("inf"));
    let pdf = d.finish().unwrap();
    check_xref(&pdf);
}

/// Decodes the `<FEFF…>` hex string following `key` in the file bytes.
fn utf16_entry(pdf: &[u8], key: &str) -> String {
    let at = find(pdf, key.as_bytes()).unwrap() + key.len();
    let rest = std::str::from_utf8(&pdf[at..at + 9000.min(pdf.len() - at)]).unwrap_or("");
    let hex = rest.trim_start().strip_prefix("<FEFF").unwrap();
    let hex = &hex[..hex.find('>').unwrap()];
    let units: Vec<u16> = (0..hex.len()).step_by(4).map(|i| u16::from_str_radix(&hex[i..i + 4], 16).unwrap()).collect();
    String::from_utf16(&units).unwrap()
}

#[test]
fn info_strings_with_non_ascii_are_utf16be() {
    let mut doc = PdfDoc::new();
    doc.add_page(200.0, 200.0);
    doc.info.title = "Übersicht €5 – “Q1” 日本 😀".into();
    doc.info.author = "Müller & Søn".into();
    let pdf = doc.finish().unwrap();
    assert_eq!(utf16_entry(&pdf, "/Title"), "Übersicht €5 – “Q1” 日本 😀");
    assert_eq!(utf16_entry(&pdf, "/Author"), "Müller & Søn");
}

#[test]
fn ascii_info_strings_stay_literal_and_escaped() {
    let mut doc = PdfDoc::new();
    doc.add_page(200.0, 200.0);
    doc.info.title = r"Q1 (draft) a\b".into();
    let pdf = doc.finish().unwrap();
    assert!(find(&pdf, br"/Title (Q1 \(draft\) a\\b)").is_some());
    assert!(find(&pdf, b"/Producer (GridCraft PDF)").is_some());
}

#[test]
fn info_strings_are_capped() {
    let mut doc = PdfDoc::new();
    doc.add_page(200.0, 200.0);
    doc.info.title = "日".repeat(100_000);
    let pdf = doc.finish().unwrap();
    assert_eq!(utf16_entry(&pdf, "/Title").chars().count(), MAX_INFO_CHARS);
}

#[test]
fn cjk_text_is_not_replaced_by_question_marks() {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(612.0, 792.0);
    p.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "签字清单");
    p.text(10.0, 40.0, 10.0, Font::Helvetica, [0, 0, 0], "No. 编号 2");
    let pdf = doc.finish().unwrap();
    check_xref(&pdf);
    assert!(find(&pdf, b"(????) Tj").is_none(), "CJK text written as question marks");
    assert!(find(&pdf, b"<7B7E5B576E055355> Tj").is_some());
    assert!(find(&pdf, b"(No. ) Tj").is_some());
    assert!(find(&pdf, b"<7F1653F7> Tj").is_some());
    assert!(find(&pdf, b"( 2) Tj").is_some());
    assert!(find(&pdf, b"/Encoding /UniGB-UTF16-H").is_some());
    // Measured as full-width glyphs, matching the CID font's default width.
    assert_eq!(text_width(Font::Helvetica, 10.0, "签字"), 20.0);
}

#[test]
fn arabic_is_shaped_and_preserved_or_export_reports_missing_font() {
    let text = "مَرْحَبًا 123 (ABC)";
    let mut doc = PdfDoc::new();
    let page = doc.add_page(600.0, 300.0);
    page.text(30.0, 70.0, 20.0, Font::Helvetica, [0, 0, 0], text);
    let layout = arabic::layout(Font::Helvetica, text);
    let Ok(layout) = layout else {
        let error = doc.finish().unwrap_err().to_string();
        assert!(error.contains("Arabic PDF"), "{error}");
        return;
    };
    assert!(layout.width > 0.0);
    assert!((text_width(Font::Helvetica, 20.0, text) - layout.width * 0.02).abs() < 0.001);
    let pdf = doc.finish().unwrap();
    check_xref(&pdf);
    assert_eq!(utf16_entry(&pdf, "/ActualText"), text);
    assert!(find(&pdf, b"/Subtype /Type3").is_some());
    assert!(find(&pdf, b"/ToUnicode").is_some());
    assert!(find(&pdf, b"beginbfchar").is_some());
    assert!(find(&pdf, b"(?) Tj").is_none());
    assert!(find(&pdf, b"/UniGB-UTF16-H").is_none(), "Arabic is never set in the CJK font");
}

#[test]
fn latin_inside_arabic_text_declares_its_page_font() {
    if arabic::layout(Font::Helvetica, "مرحبا").is_err() && arabic::ARABIC_FONTS.is_empty() {
        return; // Optional font input; required-font CI exercises this regression.
    }
    let mut doc = PdfDoc::new();
    doc.add_page(600.0, 300.0).text(30.0, 70.0, 20.0, Font::Helvetica, [0, 0, 0], "مرحبا\u{2028}عالم");
    let pdf = doc.finish().unwrap();
    check_xref(&pdf);
    assert!(find(&pdf, b"/F1 20 Tf").is_some(), "the line break is drawn as a Helvetica space");
    assert!(find(&pdf, b"/Font << /F1 ").is_some(), "the page declares the font its Latin run uses");
}

#[test]
fn cjk_inside_arabic_text_uses_the_cid_font() {
    if arabic::layout(Font::Helvetica, "مرحبا").is_err() && arabic::ARABIC_FONTS.is_empty() {
        return; // Optional font input; required-font CI exercises this regression.
    }
    let text = "مرحبا 编号";
    let mut doc = PdfDoc::new();
    doc.add_page(600.0, 300.0).text(30.0, 70.0, 20.0, Font::Helvetica, [0, 0, 0], text);
    let pdf = doc.finish().unwrap();
    check_xref(&pdf);
    assert!(find(&pdf, b"<7F1653F7> Tj").is_some(), "the CJK run is set in the CID font");
    assert!(find(&pdf, b"/Encoding /UniGB-UTF16-H").is_some(), "the page declares the CID font");
    assert_eq!(utf16_entry(&pdf, "/ActualText"), text);
    assert!(text_width(Font::Helvetica, 20.0, text) >= text_width(Font::Helvetica, 20.0, "مرحبا") + 40.0, "two full-width ideographs");
    let mut emoji = PdfDoc::new();
    emoji.add_page(600.0, 300.0).text(30.0, 70.0, 20.0, Font::Helvetica, [0, 0, 0], "مرحبا 😀");
    assert!(emoji.finish().is_err(), "characters no font covers still fail the export");
}

#[test]
fn oversized_arabic_text_is_an_error_instead_of_partial_pdf() {
    let mut doc = PdfDoc::new();
    doc.add_page(600.0, 300.0).text(0.0, 50.0, 10.0, Font::Helvetica, [0, 0, 0], &"ب".repeat(65_537));
    assert!(doc.finish().unwrap_err().to_string().contains("128 KiB"));
}

#[test]
fn arabic_joining_marks_styles_and_rotation() {
    if arabic::layout(Font::Helvetica, "مرحبا").is_err() && arabic::ARABIC_FONTS.is_empty() {
        return; // Optional font input; required-font CI exercises this regression.
    }
    for (text, expected) in [("مرحبا 123 (ABC)", "(ABC)"), ("مرحبا [ABC]", "[ABC]"), ("مرحبا <ABC>", "<ABC>")] {
        let mixed = arabic::layout(Font::Helvetica, text).unwrap();
        let latin: String = mixed
            .items
            .iter()
            .filter_map(|item| match item {
                arabic::Item::Latin { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        if !latin.is_empty() {
            assert_eq!(latin, expected);
        }
    }
    let mut metrics = PdfDoc::new();
    let (ascent, descent) = metrics.add_page(100.0, 100.0).text_vertical_extents(Font::Helvetica, 20.0, "كمثرى").unwrap();
    assert!(ascent > 0.0 && descent > 20.0 * 0.22, "Arabic descenders exceed the old Latin baseline allowance");
    assert!(
        metrics.page_mut(0).unwrap().text_vertical_extents(Font::Helvetica, 20.0, "مرحبا 123 (ABC)").is_some(),
        "spaces in variable fonts have empty outlines"
    );
    metrics.finish().unwrap();
    let joined = arabic::layout(Font::Helvetica, "سلام").unwrap();
    let separate_width: f32 = "سلام".chars().map(|c| text_width(Font::Helvetica, 1000.0, &c.to_string())).sum();
    assert!((joined.width - separate_width).abs() > 10.0, "joining changes advances");
    let marked = arabic::layout(Font::Helvetica, "مَرْحَبًا").unwrap();
    let plain = arabic::layout(Font::Helvetica, "مرحبا").unwrap();
    assert!(marked.items.len() > plain.items.len());
    assert!(marked.items.iter().any(|item| matches!(item, arabic::Item::Glyph { y, .. } if *y != 0.0)));
    let bold_available = match arabic::layout(Font::HelveticaBold, "مرحبا") {
        Ok(bold) if arabic::bold_face_is_synthetic() => {
            assert!(bold.width >= plain.width, "synthetic bold keeps the regular advances");
            true
        }
        Ok(bold) => {
            assert!(bold.width > plain.width, "bold metrics use the bold font instance");
            true
        }
        Err(e) => {
            assert!(arabic::ARABIC_FONTS.is_empty(), "a bundled face always serves bold text: {e:?}");
            false
        }
    };
    let mut regular = PdfDoc::new();
    regular.add_page(500.0, 300.0).text(30.0, 60.0, 20.0, Font::Helvetica, [0, 0, 0], "مرحبا");
    let regular = regular.finish().unwrap();
    let mut italic = PdfDoc::new();
    italic.add_page(500.0, 300.0).text(30.0, 60.0, 20.0, Font::HelveticaOblique, [0, 0, 0], "مرحبا");
    let italic = italic.finish().unwrap();
    assert_ne!(regular, italic, "italic shears the actual outlines");
    let rotated_font = if bold_available { Font::HelveticaBoldOblique } else { Font::HelveticaOblique };
    let mut rotated = PdfDoc::new();
    rotated.add_page(500.0, 300.0).text_rotated(100.0, 100.0, 20.0, rotated_font, [0, 0, 0], "مرحبا 123", -std::f32::consts::FRAC_PI_2);
    let rotated = rotated.finish().unwrap();
    check_xref(&rotated);
    assert_eq!(utf16_entry(&rotated, "/ActualText"), "مرحبا 123");
    assert!(find(&rotated, b"0 1 -1 0 100 200 cm").is_some());
}

#[test]
fn line_breaks_in_arabic_text_draw_as_spaces() {
    if arabic::layout(Font::Helvetica, "مرحبا").is_err() && arabic::ARABIC_FONTS.is_empty() {
        return; // Optional font input; required-font CI exercises this regression.
    }
    for text in ["مرحبا\r\nعالم", "مرحبا\u{2028}عالم", "مرحبا\u{2029}ABC\r"] {
        let layout = arabic::layout(Font::Helvetica, text).unwrap();
        for item in &layout.items {
            if let arabic::Item::Latin { text: drawn, .. } = item {
                assert!(!drawn.chars().any(|ch| matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')), "{drawn:?} from {text:?}");
            }
        }
    }
    let drawn = |text: &str| format!("{:?}", arabic::layout(Font::Helvetica, text).unwrap().items);
    assert_eq!(drawn("مرحبا\u{2029}عالم"), drawn("مرحبا عالم"), "a paragraph separator keeps right-to-left reading order");
}

#[test]
fn arabic_subsets_split_after_255_character_codes() {
    if arabic::layout(Font::Helvetica, "مرحبا").is_err() && arabic::ARABIC_FONTS.is_empty() {
        return; // Optional font input; required-font CI exercises this regression.
    }
    let mut doc = PdfDoc::new();
    let page = doc.add_page(600.0, 300.0);
    let marks = ['َ', 'ُ', 'ِ', 'ّ', 'ْ', 'ً', 'ٌ', 'ٍ'];
    for i in 0..260 {
        let text = format!("ب{}{}{}", marks[i % 8], marks[(i / 8) % 8], marks[(i / 64) % 8]);
        page.text(20.0, 50.0, 10.0, Font::Helvetica, [0, 0, 0], &text);
    }
    let pdf = doc.finish().unwrap();
    check_xref(&pdf);
    assert!(find(&pdf, b"/A2 ").is_some());
    assert!(find(&pdf, b"/LastChar 255").is_some());
    assert!(find(&pdf, b"<100> Tj").is_none(), "Type3 codes are always one byte");
}
