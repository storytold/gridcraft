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
    let pdf = PdfDoc::new().finish();
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
    let pdf = d.finish();
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
    let pdf = d.finish();
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
    let pdf = d.finish();
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
    let pdf = d.finish();
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
    let pdf = d.finish();
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
    let pdf = doc.finish();
    assert_eq!(utf16_entry(&pdf, "/Title"), "Übersicht €5 – “Q1” 日本 😀");
    assert_eq!(utf16_entry(&pdf, "/Author"), "Müller & Søn");
}

#[test]
fn ascii_info_strings_stay_literal_and_escaped() {
    let mut doc = PdfDoc::new();
    doc.add_page(200.0, 200.0);
    doc.info.title = r"Q1 (draft) a\b".into();
    let pdf = doc.finish();
    assert!(find(&pdf, br"/Title (Q1 \(draft\) a\\b)").is_some());
    assert!(find(&pdf, b"/Producer (GridCraft PDF)").is_some());
}

#[test]
fn info_strings_are_capped() {
    let mut doc = PdfDoc::new();
    doc.add_page(200.0, 200.0);
    doc.info.title = "日".repeat(100_000);
    let pdf = doc.finish();
    assert_eq!(utf16_entry(&pdf, "/Title").chars().count(), MAX_INFO_CHARS);
}

#[test]
fn cjk_text_is_not_replaced_by_question_marks() {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(612.0, 792.0);
    p.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "签字清单");
    p.text(10.0, 40.0, 10.0, Font::Helvetica, [0, 0, 0], "No. 编号 2");
    let pdf = doc.finish();
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
fn latin_extended_letters_use_the_latin_font() {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(612.0, 792.0);
    p.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "tőrű ŐŰ");
    let c = page_content(&doc).to_string();
    assert!(!c.contains("<0151>") && !c.contains("<0171>"), "ő/ű set in the CJK font: {c}");
    // ő, ű, Ő, Ű are bytes 0xD1, 0xF1, 0xD0, 0xF0 of the Helvetica Latin Extended-A resource.
    // Resources: /F1../F12 the faces, /F13 the CID font, /F14../F25 their Latin Extended-A twins.
    let ext = "/F14";
    let expected =
        format!("/F1 10 Tf\n(t) Tj\n{ext} 10 Tf\n<D1> Tj\n/F1 10 Tf\n(r) Tj\n{ext} 10 Tf\n<F1> Tj\n/F1 10 Tf\n( ) Tj\n{ext} 10 Tf\n<D0F0> Tj\n");
    assert!(c.contains(&expected), "ő/ű not set through the Latin resource: {c}");
    let pdf = doc.finish();
    check_xref(&pdf);
    assert!(find(&pdf, CID_BASE_FONT.as_bytes()).is_none(), "CJK font used for Latin letters");
    for name in ["/ohungarumlaut", "/uhungarumlaut", "/Ohungarumlaut", "/Uhungarumlaut"] {
        assert!(find(&pdf, name.as_bytes()).is_some(), "missing glyph name {name}");
    }
    assert_eq!(text_width(Font::Helvetica, 10.0, "ő"), text_width(Font::Helvetica, 10.0, "o"));
    assert_eq!(text_width(Font::TimesBold, 10.0, "Ű"), text_width(Font::TimesBold, 10.0, "U"));
    assert_eq!(text_width(Font::Courier, 10.0, "ł"), 6.0);
}

/// Romanian: ă/Ă are Latin Extended-A, ș ț Ș Ț (comma below) are Latin Extended-B. All of them
/// are set in the Latin face, none in the CJK font (which has no glyph for them: printed boxes).
#[test]
fn romanian_letters_use_the_latin_font() {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(612.0, 792.0);
    p.text(10.0, 20.0, 10.0, Font::HelveticaBold, [0, 0, 0], "Comandă științe ȘȚ");
    let c = page_content(&doc).to_string();
    // HelveticaBold is /F2; its Latin Extended twin is /F15. ă = 0x83, ș ț = 0x02 0x04, Ș Ț = 0x01 0x03.
    let expected = "/F2 10 Tf\n(Comand) Tj\n/F15 10 Tf\n<83> Tj\n/F2 10 Tf\n( ) Tj\n/F15 10 Tf\n<02> Tj\n/F2 10 Tf\n(tiin) Tj\n/F15 10 Tf\n<04> Tj\n/F2 10 Tf\n(e ) Tj\n/F15 10 Tf\n<0103> Tj\n";
    assert!(c.contains(expected), "Romanian letters not set through the Latin resource: {c}");
    let pdf = doc.finish();
    check_xref(&pdf);
    assert!(find(&pdf, CID_BASE_FONT.as_bytes()).is_none(), "CJK font used for Romanian letters");
    // Measured like their base letters, not as a full-em CJK glyph.
    assert_eq!(text_width(Font::HelveticaBold, 10.0, "ș"), text_width(Font::HelveticaBold, 10.0, "s"));
    assert_eq!(text_width(Font::Helvetica, 10.0, "Ț"), text_width(Font::Helvetica, 10.0, "T"));
    // The letters next to the block stay out of it.
    for c in ['\u{0217}', '\u{021C}'] {
        assert_eq!(ext_code(c), None, "{c:?}");
    }
    // The resource carries the advances (a viewer's standard metrics have no `uni0219`): ș at
    // code 2 advances like s, ă at 0x83 like a, and unused codes by nothing.
    let widths: Vec<u16> = ext_widths(Font::HelveticaBold).split(' ').map(|w| w.parse().unwrap()).collect();
    assert_eq!(widths.len(), 255);
    assert_eq!(widths[2 - 1], Font::HelveticaBold.char_width('s'));
    assert_eq!(widths[0x83 - 1], Font::HelveticaBold.char_width('a'));
    assert_eq!(widths[0x20 - 1], 0);
    // Œ is WinAnsi (0x8C there), so its Latin Extended-A slot (0xD2) is unused.
    assert_eq!(widths[0xD2 - 1], 0);
    for code in 1..=255u8 {
        if let Some(c) = ext_char(code) {
            assert_eq!(ext_code(c), Some(code), "{c:?}");
        }
    }
}

#[test]
fn romanian_comma_below_and_cedilla_letters_stay_distinct() {
    // Romanian writes ș ț with a comma below (U+0218..U+021B); the older cedilla letters ş ţ
    // (U+015E, U+015F, U+0162, U+0163) are other characters and keep their own codes and glyphs.
    // The standard fonts name the T cedilla `Tcommaaccent`, which the AGL maps back to U+0162.
    let pairs = [('Ș', 'Ş', 'S', "Scedilla"), ('ș', 'ş', 's', "scedilla"), ('Ț', 'Ţ', 'T', "Tcommaaccent"), ('ț', 'ţ', 't', "tcommaaccent")];
    let widths: Vec<u16> = ext_widths(Font::Helvetica).split(' ').map(|w| w.parse().unwrap()).collect();
    for (i, (comma, cedilla, base, cedilla_glyph)) in pairs.into_iter().enumerate() {
        let code = ext_code(comma).unwrap();
        assert_eq!(usize::from(code), i + 1, "{comma:?}");
        let cedilla_code = ext_code(cedilla).unwrap();
        assert_ne!(code, cedilla_code, "{comma:?} {cedilla:?}");
        // Named by code point, which every viewer maps back to the letter (the AGL's
        // `Tcommaaccent` would map to the cedilla letter U+0162).
        assert_eq!(metrics::COMMA_BELOW.get(i).map(|e| e.0), Some(format!("uni{:04X}", comma as u32).as_str()));
        let cedilla_name = metrics::LATIN_EXT_A.get(usize::from(cedilla_code - 0x80)).map(|e| e.0);
        assert_eq!(cedilla_name, Some(cedilla_glyph), "{cedilla:?}");
        // Each advances like its base letter.
        assert_eq!(widths[usize::from(code) - 1], Font::Helvetica.char_width(base), "{comma:?}");
    }
    // Mixed with other Latin Extended-A letters and plain text, in one run of the same face.
    let mut doc = PdfDoc::new();
    let p = doc.add_page(612.0, 792.0);
    p.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "ăşșţțőŁ");
    let c = page_content(&doc).to_string();
    // ă 0x83, ş 0xDF, ș 0x02, ţ 0xE3, ț 0x04, ő 0xD1, Ł 0xC1, all in /F14 (Helvetica's twin).
    assert!(c.contains("/F14 10 Tf\n<83DF02E304D1C1> Tj\n"), "{c}");
    let pdf = doc.finish();
    check_xref(&pdf);
    assert!(find(&pdf, b"/Differences [1 /uni0218 /uni0219 /uni021A /uni021B 128 /Amacron").is_some());
}

#[test]
fn latin_extended_letters_use_each_faces_own_resource() {
    // Every face routed to a `/Differences` resource has the Latin glyphs; the symbolic standard
    // fonts (Symbol, ZapfDingbats) lack them and must never be in this list.
    for face in Font::ALL {
        let name = face.base_name();
        assert!(!name.starts_with("Symbol") && !name.starts_with("ZapfDingbats"), "{name}");
        let mut doc = PdfDoc::new();
        let p = doc.add_page(612.0, 792.0);
        p.text(10.0, 20.0, 10.0, face, [0, 0, 0], "ő");
        let ext = Font::ALL.len() + 2 + face.index();
        let c = page_content(&doc).to_string();
        assert!(c.contains(&format!("/F{ext} 10 Tf\n<D1> Tj\n")), "{name}: {c}");
        let pdf = doc.finish();
        check_xref(&pdf);
        let dict = format!("/BaseFont /{name} /FirstChar 1 /LastChar 255 /Widths [");
        assert!(find(&pdf, dict.as_bytes()).is_some(), "{name}: no widths for the Latin Extended resource");
        let enc = "/Encoding << /Type /Encoding /Differences [1 /uni0218 /uni0219 /uni021A /uni021B 128 /Amacron";
        assert!(find(&pdf, enc.as_bytes()).is_some(), "{name}: no Latin Extended-A encoding");
        assert!(find(&pdf, format!("/F{ext} ").as_bytes()).is_some(), "{name}: resource missing");
        assert!(find(&pdf, CID_BASE_FONT.as_bytes()).is_none(), "{name}: CJK font used");
    }
}
