use super::*;
use std::error::Error;
use std::io;

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

/// Parses the xref table back and checks each in-use entry points at `N 0 obj`.
fn check_xref(pdf: &[u8]) -> Result<usize, Box<dyn Error>> {
    let sx = rfind(pdf, b"startxref\n").ok_or_else(|| io::Error::other("missing startxref"))?;
    let rest = std::str::from_utf8(&pdf[sx + 10..])?;
    let off: usize = rest.lines().next().ok_or_else(|| io::Error::other("missing xref offset"))?.trim().parse()?;
    assert!(pdf[off..].starts_with(b"xref\n"), "startxref points at xref");
    let table = std::str::from_utf8(&pdf[off..sx])?;
    let mut lines = table.lines();
    assert_eq!(lines.next(), Some("xref"));
    let hdr: Vec<usize> =
        lines.next().ok_or_else(|| io::Error::other("missing xref header"))?.split(' ').map(str::parse).collect::<Result<_, _>>()?;
    assert_eq!(hdr.first(), Some(&0));
    let count = *hdr.get(1).ok_or_else(|| io::Error::other("missing xref count"))?;
    let mut checked = 0;
    for i in 0..count {
        let e = lines.next().ok_or_else(|| io::Error::other("missing xref entry"))?;
        assert_eq!(e.len(), 19, "entry `{e}` (20 bytes with newline)");
        let o: usize = e.get(..10).ok_or_else(|| io::Error::other("short xref offset"))?.parse()?;
        if e.trim_end().ends_with('n') {
            let want = format!("{i} 0 obj");
            assert!(pdf[o..].starts_with(want.as_bytes()), "object {i} at {o}");
            checked += 1;
        }
    }
    assert!(table.contains("trailer"));
    assert!(table.contains(&format!("/Size {count}")));
    Ok(checked)
}

#[test]
fn minimal_document() -> Result<(), Box<dyn Error>> {
    let pdf = PdfDoc::new().finish();
    assert!(pdf.starts_with(b"%PDF-1.7"));
    assert!(pdf.ends_with(b"%%EOF\n"));
    assert!(check_xref(&pdf)? >= 5);
    Ok(())
}

#[test]
fn drawing_and_text() -> Result<(), Box<dyn Error>> {
    let mut d = PdfDoc::new();
    d.info.title = "Report (Q1)".into();
    let p = d.add_page(612.0, 792.0);
    p.fill_rect(10.0, 10.0, 100.0, 20.0, [255, 255, 0]);
    p.rect_stroke(10.0, 10.0, 100.0, 20.0, 0.5, [0, 0, 0]);
    p.stroke_line(0.0, 0.0, 612.0, 792.0, 1.0, [255, 0, 0]);
    p.stroke_line_dashed(Point::new(0.0, 5.0), Point::new(100.0, 5.0), 1.0, [0, 0, 0], &[3.0, 2.0]);
    p.polygon(&[[0.0, 0.0], [10.0, 0.0], [5.0, 8.0]], Some([0, 0, 255]), Some(([0, 0, 0], 1.0)));
    p.circle(300.0, 300.0, 20.0, Some([0, 128, 0]), None);
    p.save();
    p.clip_rect(0.0, 0.0, 50.0, 50.0);
    p.text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], "Hello (world) \\ café €5");
    p.restore();
    p.text_rotated(Point::new(100.0, 100.0), 9.0, Font::TimesBold, [0, 0, 0], "Up", -std::f32::consts::FRAC_PI_2);
    let p2 = d.add_page(842.0, 595.0);
    p2.text(10.0, 20.0, 10.0, Font::CourierBold, [0, 0, 0], "Second");
    assert_eq!(d.page_count(), 2);
    let pdf = d.finish();
    check_xref(&pdf)?;
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
    Ok(())
}

#[test]
fn uri_and_page_links_are_real_pdf_annotations() -> Result<(), Box<dyn Error>> {
    let mut d = PdfDoc::new();
    let first = d.add_page(200.0, 300.0);
    first.link_uri(10.0, 20.0, 80.0, 30.0, "https://example.org/a(b)?q=£");
    first.link_page(Rect::new(100.0, 40.0, 50.0, 20.0), 1, Point::new(35.0, 60.0));
    first.link_uri(-20.0, -20.0, 40.0, 40.0, "mailto:a@example.org");
    first.link_uri(0.0, 0.0, f32::NAN, 10.0, "https://bad.example");
    first.link_uri(0.0, 0.0, 10.0, 10.0, "javascript:alert(1)");
    let second = d.add_page(100.0, 200.0);
    second.link_page(Rect::new(0.0, 0.0, 10.0, 10.0), 99, Point::new(0.0, 0.0));
    let pdf = d.finish();
    check_xref(&pdf)?;
    let s = String::from_utf8_lossy(&pdf);
    assert_eq!(s.matches("/Subtype /Link").count(), 3);
    assert!(s.contains("/Annots ["));
    assert!(s.contains("/Rect [10 250 90 280]"));
    assert!(s.contains("/Rect [0 280 20 300]"));
    assert!(s.contains("/URI (https://example.org/a\\(b\\)?q=%C2%A3)"));
    assert!(s.contains("/XYZ 35 140 null"));
    assert!(!s.contains("https://bad.example"));
    Ok(())
}

#[test]
fn images_with_alpha() -> Result<(), Box<dyn Error>> {
    let mut d = PdfDoc::new();
    let p = d.add_page(200.0, 200.0);
    let mut px = Vec::new();
    for i in 0..16u8 {
        px.extend_from_slice(&[i * 10, 0, 0, if i < 8 { 255 } else { 128 }]);
    }
    p.image_rgba(Rect::new(10.0, 10.0, 40.0, 40.0), &px, [4, 4]);
    p.image_rgba(Rect::new(10.0, 10.0, 40.0, 40.0), &[255; 16], [2, 2]); // opaque: no mask
    p.image_rgba(Rect::new(0.0, 0.0, 1.0, 1.0), &[1, 2, 3], [4, 4]); // too short: ignored
    let pdf = d.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"/SMask").is_some());
    assert!(find(&pdf, b"/Im1 Do").is_some());
    assert!(find(&pdf, b"/Im2 Do").is_some());
    assert!(find(&pdf, b"/Im3").is_none());
    Ok(())
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
fn hostile_inputs_never_panic() -> Result<(), Box<dyn Error>> {
    let mut d = PdfDoc::new();
    let p = d.add_page(f32::NAN, -5.0);
    p.text(f32::INFINITY, f32::NAN, 1e30, Font::Helvetica, [0, 0, 0], "x");
    p.fill_rect(0.0, 0.0, -1.0, 5.0, [0, 0, 0]);
    p.polygon(&[], None, None);
    p.circle(0.0, 0.0, -3.0, Some([0, 0, 0]), None);
    p.restore();
    p.restore();
    p.save();
    p.image_rgba(Rect::new(0.0, 0.0, 10.0, 10.0), &[], [u32::MAX, u32::MAX]);
    let pdf = d.finish();
    check_xref(&pdf)?;
    Ok(())
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
    p.stroke_line_dashed(Point::new(0.0, 0.0), Point::new(50.0, 0.0), 1.0, [0, 0, 0], &[0.0, 0.0]);
    p.polyline(&[[0.0, 0.0], [5.0, 5.0]], 1.0, [0, 0, 0], &[0.0, 0.0001, f32::NAN]);
    p.stroke_line_dashed(Point::new(0.0, 0.0), Point::new(50.0, 0.0), 1.0, [0, 0, 0], &[3.0, 2.0]);
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
        p.image_rgba(Rect::new(0.0, 0.0, w, h), &px, [2, 2]);
    }
    p.image_rgba(Rect::new(f32::NAN, 0.0, 10.0, 10.0), &px, [2, 2]);
    assert!(p.images.is_empty());
    assert!(p.content.is_empty());
}

#[test]
fn every_draw_method_survives_hostile_numbers() -> Result<(), Box<dyn Error>> {
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
            p.stroke_line_dashed(Point::new(a, b), Point::new(b, a), b, [1, 2, 3], &[a, b, 0.0]);
            p.polyline(&[[a, b], [b, a], [1.0, 1.0]], a, [1, 2, 3], &[b, a]);
            p.polygon(&[[a, b], [b, a], [1.0, 1.0]], Some([1, 2, 3]), Some(([4, 5, 6], a)));
            p.circle(a, b, b, Some([1, 2, 3]), Some(([4, 5, 6], a)));
            p.text(a, b, a, Font::Helvetica, [0, 0, 0], "x");
            p.text_rotated(Point::new(a, b), 10.0, Font::TimesRoman, [0, 0, 0], "y", b);
            p.image_rgba(Rect::new(a, b, b, a), &px, [2, 2]);
        }
    }
    p.stroke_line_dashed(Point::new(0.0, 0.0), Point::new(1.0, 1.0), 1.0, [0, 0, 0], &[0.0; 100]);
    let c = page_content(&d);
    assert!(!c.contains("NaN") && !c.contains("inf"));
    let pdf = d.finish();
    check_xref(&pdf)?;
    Ok(())
}

/// Decodes the `<FEFF…>` hex string following `key` in the file bytes.
fn utf16_entry(pdf: &[u8], key: &str) -> Result<String, Box<dyn Error>> {
    let at = find(pdf, key.as_bytes()).ok_or_else(|| io::Error::other("missing PDF info key"))? + key.len();
    let end = at + 9000.min(pdf.len() - at);
    let rest = std::str::from_utf8(pdf.get(at..end).ok_or_else(|| io::Error::other("short PDF info value"))?)?;
    let hex = rest.trim_start().strip_prefix("<FEFF").ok_or_else(|| io::Error::other("missing UTF-16 marker"))?;
    let end = hex.find('>').ok_or_else(|| io::Error::other("missing UTF-16 terminator"))?;
    let hex = hex.get(..end).ok_or_else(|| io::Error::other("invalid UTF-16 range"))?;
    let units: Vec<u16> = (0..hex.len()).step_by(4).map(|i| u16::from_str_radix(hex.get(i..i + 4).unwrap_or(""), 16)).collect::<Result<_, _>>()?;
    Ok(String::from_utf16(&units)?)
}

#[test]
fn info_strings_with_non_ascii_are_utf16be() -> Result<(), Box<dyn Error>> {
    let mut doc = PdfDoc::new();
    doc.add_page(200.0, 200.0);
    doc.info.title = "Übersicht €5 – “Q1” 日本 😀".into();
    doc.info.author = "Müller & Søn".into();
    let pdf = doc.finish();
    assert_eq!(utf16_entry(&pdf, "/Title")?, "Übersicht €5 – “Q1” 日本 😀");
    assert_eq!(utf16_entry(&pdf, "/Author")?, "Müller & Søn");
    Ok(())
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
fn info_strings_are_capped() -> Result<(), Box<dyn Error>> {
    let mut doc = PdfDoc::new();
    doc.add_page(200.0, 200.0);
    doc.info.title = "日".repeat(100_000);
    let pdf = doc.finish();
    assert_eq!(utf16_entry(&pdf, "/Title")?.chars().count(), MAX_INFO_CHARS);
    Ok(())
}

#[test]
fn cjk_text_is_not_replaced_by_question_marks() -> Result<(), Box<dyn Error>> {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(612.0, 792.0);
    p.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "签字清单");
    p.text(10.0, 40.0, 10.0, Font::Helvetica, [0, 0, 0], "No. 编号 2");
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"(????) Tj").is_none(), "CJK text written as question marks");
    assert!(find(&pdf, b"<7B7E5B576E055355> Tj").is_some());
    assert!(find(&pdf, b"(No. ) Tj").is_some());
    assert!(find(&pdf, b"<7F1653F7> Tj").is_some());
    assert!(find(&pdf, b"( 2) Tj").is_some());
    assert!(find(&pdf, b"/Encoding /UniGB-UTF16-H").is_some());
    // Measured as full-width glyphs, matching the CID font's default width.
    assert_eq!(text_width(Font::Helvetica, 10.0, "签字"), 20.0);
    Ok(())
}

use crate::test_support::tiny_ttf;

#[test]
fn truetype_parse_maps_characters_and_widths() {
    let f = UnicodeFont::parse(tiny_ttf(&['Α', 'β', 'Ж'], 640, 0)).unwrap();
    assert_eq!(f.glyph('Α'), 1);
    assert_eq!(f.glyph('β'), 2);
    assert_eq!(f.glyph('Ж'), 3);
    assert_eq!(f.glyph('Ω'), 0, "missing characters map to .notdef");
    assert_eq!(f.char_width('β'), 640);
    assert!(f.has('Α') && !f.has('Ω'));
}

#[test]
fn truetype_rejects_restricted_and_malformed_fonts() {
    assert!(UnicodeFont::parse(tiny_ttf(&['Α'], 500, 0x0002)).is_none(), "restricted licence");
    assert!(UnicodeFont::parse(b"OTTO\x00\x01".to_vec()).is_none());
    assert!(UnicodeFont::parse(Vec::new()).is_none());
    let good = tiny_ttf(&['Α', 'β'], 500, 0);
    // Every truncation is rejected or parsed safely; none panics.
    for len in 0..good.len() {
        let _ = UnicodeFont::parse(good[..len].to_vec()).map(|f| f.glyph('β'));
    }
    let mut flipped = good.clone();
    for i in 0..flipped.len() {
        flipped[i] ^= 0xFF;
        let _ = UnicodeFont::parse(flipped.clone()).map(|f| (f.glyph('Α'), f.char_width('β')));
        flipped[i] ^= 0xFF;
    }
}

#[test]
fn greek_text_embeds_a_unicode_font_instead_of_question_marks() -> Result<(), Box<dyn Error>> {
    let font = Arc::new(UnicodeFont::parse(tiny_ttf(&['Α', 'β', ' ', 'Q'], 600, 0)).unwrap());
    let mut doc = PdfDoc::default();
    doc.set_unicode_fonts([Some(font), None, None, None]).unwrap();
    let pg = doc.add_page(612.0, 792.0);
    pg.text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], "Αβ Q");
    pg.text(72.0, 90.0, 12.0, Font::HelveticaBold, [0, 0, 0], "Αβ");
    pg.text(72.0, 108.0, 12.0, Font::Helvetica, [0, 0, 0], "plain");
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"/U1 12 Tf").is_some(), "Greek uses the embedded font");
    assert!(find(&pdf, b"<0001000200030004> Tj").is_some(), "distinct Unicode character codes, not `?`");
    assert!(find(&pdf, b"(\x3f\x3f").is_none(), "no question marks");
    assert!(find(&pdf, b"/F1 12 Tf").is_some(), "Latin text keeps the standard font");
    assert!(find(&pdf, b"/Subtype /CIDFontType2").is_some());
    assert!(find(&pdf, b"/Encoding /Identity-H").is_some());
    assert!(find(&pdf, b"/FontFile2").is_some());
    assert!(find(&pdf, b"<0001> <0391>").is_some(), "ToUnicode keeps the text copyable");
    assert!(find(&pdf, b"<0002> <03B2>").is_some());
    assert!(find(&pdf, b"/U2").is_none(), "bold falls back to the regular face");
    Ok(())
}

#[test]
fn without_a_unicode_font_greek_uses_the_existing_cid_fallback() -> Result<(), Box<dyn Error>> {
    let mut doc = PdfDoc::default();
    doc.add_page(612.0, 792.0).text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], "Αβ");
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"<039103B2> Tj").is_some());
    assert!(find(&pdf, b"FontFile2").is_none());
    Ok(())
}

/// The distinct characters of `texts` in first-seen order: the character list for [`tiny_ttf`].
fn distinct_chars(texts: &[&str]) -> Vec<char> {
    let mut chars = Vec::new();
    for c in texts.iter().flat_map(|t| t.chars()) {
        if !chars.contains(&c) {
            chars.push(c);
        }
    }
    chars
}

/// Character codes follow first use within each face, independently of the font's glyph ids.
fn character_operand(chars: &[char], text: &str) -> String {
    let codes: String = text.chars().map(|c| format!("{:04X}", chars.iter().position(|x| *x == c).unwrap() + 1)).collect();
    format!("<{codes}> Tj")
}

#[test]
fn cyrillic_text_embeds_a_unicode_font_instead_of_question_marks() -> Result<(), Box<dyn Error>> {
    // Real Serbian (with the letters only Serbian Cyrillic has: Ђ ђ Љ љ Њ њ Џ џ Ћ ћ ј) and
    // Russian (a pangram, with ё), next to a Latin-only cell.
    let serbian = "Добар дан, Ђорђе! Љубав, њива, џеп, ћуп и ја.";
    let russian = "Съешь же ещё этих мягких французских булок";
    let chars = distinct_chars(&[serbian, russian]);
    let regular = Arc::new(UnicodeFont::parse(tiny_ttf(&chars, 600, 0)).unwrap());
    let bold = Arc::new(UnicodeFont::parse(tiny_ttf(&chars, 650, 0)).unwrap());
    let mut doc = PdfDoc::default();
    doc.set_unicode_fonts([Some(regular), Some(bold), None, None]).unwrap();
    let pg = doc.add_page(612.0, 792.0);
    pg.text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], serbian);
    pg.text(72.0, 90.0, 12.0, Font::HelveticaBold, [0, 0, 0], russian);
    pg.text(72.0, 108.0, 12.0, Font::Helvetica, [0, 0, 0], "Total 2026");
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"/U1 12 Tf").is_some(), "regular Cyrillic uses the embedded regular face");
    assert!(find(&pdf, b"/U2 12 Tf").is_some(), "bold Cyrillic uses the embedded bold face");
    assert!(find(&pdf, character_operand(&distinct_chars(&[serbian]), serbian).as_bytes()).is_some(), "Serbian uses Unicode character codes");
    assert!(find(&pdf, character_operand(&distinct_chars(&[russian]), russian).as_bytes()).is_some(), "Russian uses Unicode character codes");
    assert!(find(&pdf, b"(\x3f\x3f").is_none(), "no question marks");
    assert!(find(&pdf, b"/F1 12 Tf\n").is_some() && find(&pdf, b"(Total 2026) Tj").is_some(), "Latin keeps the standard font");
    assert_eq!(pdf.windows(10).filter(|w| w == b"/FontFile2").count(), 2, "regular and bold are both embedded");
    assert!(find(&pdf, b"[600]").is_some() && find(&pdf, b"[650]").is_some(), "each face carries its own widths");
    // ToUnicode keeps the text searchable and copyable: Ђ U+0402, ј U+0458, џ U+045F, я U+044F,
    // ё U+0451 map back from their glyphs.
    for c in ['Ђ', 'ј', 'џ', 'я', 'ё'] {
        let entry = format!("> <{:04X}>", c as u32);
        assert!(find(&pdf, entry.as_bytes()).is_some(), "ToUnicode entry for {c}: {entry}");
    }
    Ok(())
}

#[test]
fn without_a_unicode_font_cyrillic_uses_the_existing_cid_fallback() -> Result<(), Box<dyn Error>> {
    let mut doc = PdfDoc::default();
    doc.add_page(612.0, 792.0).text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], "Здраво");
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"<04170434044004300432043E> Tj").is_some());
    assert!(find(&pdf, b"FontFile2").is_none());
    Ok(())
}

#[test]
fn custom_faces_replace_latin_and_measure_all_four_styles() -> Result<(), Box<dyn Error>> {
    let faces: Vec<_> = [400, 600, 800, 1000].into_iter().map(|w| Arc::new(UnicodeFont::parse(tiny_ttf(&['A', 'Ω'], w, 0)).unwrap())).collect();
    let fonts = TextFonts::custom(faces[0].clone(), Some(faces[1].clone()), Some(faces[2].clone()), Some(faces[3].clone()));
    let mut doc = PdfDoc::default();
    doc.set_text_fonts(fonts).unwrap();
    let pg = doc.add_page(300.0, 300.0);
    for (i, font) in [Font::TimesRoman, Font::CourierBold, Font::HelveticaOblique, Font::TimesBoldItalic].into_iter().enumerate() {
        assert_eq!(pg.text_width(font, 10.0, "AA"), [8.0, 12.0, 16.0, 20.0][i]);
        pg.text(10.0, 20.0 + i as f32 * 20.0, 10.0, font, [0, 0, 0], "AΩ");
    }
    assert_eq!(doc.missing_glyph(), None);
    let bytes = doc.finish();
    check_xref(&bytes)?;
    for slot in 1..=4 {
        assert!(find(&bytes, format!("/U{slot} 10 Tf").as_bytes()).is_some());
    }
    assert_eq!(bytes.windows(10).filter(|w| *w == b"/FontFile2").count(), 4);
    assert!(find(&bytes, b"/Subtype /Type1").is_none());
    assert!(find(&bytes, b"<0001> <0041>").is_some());
    Ok(())
}

#[test]
fn custom_missing_styles_use_regular_and_missing_glyphs_are_reported() {
    let regular = Arc::new(UnicodeFont::parse(tiny_ttf(&['A'], 500, 0)).unwrap());
    let fonts = TextFonts::custom(regular, None, None, None);
    assert_eq!(fonts.text_width(Font::TimesBoldItalic, 10.0, "AA"), 10.0);
    let mut doc = PdfDoc::default();
    doc.set_text_fonts(fonts).unwrap();
    let pg = doc.add_page(100.0, 100.0);
    pg.text(10.0, 20.0, 10.0, Font::TimesBoldItalic, [0, 0, 0], "A");
    assert!(pg.content.contains("/U1 10 Tf"));
    pg.text(10.0, 40.0, 10.0, Font::Helvetica, [0, 0, 0], "Ω");
    assert_eq!(doc.missing_glyph(), Some('Ω'));
    assert!(UnicodeFont::parse(tiny_ttf(&['A'], 500, 0x0200)).is_none(), "bitmap-only embedding is forbidden");
}

#[test]
fn named_families_keep_independent_resources_styles_and_metrics_across_pages() -> Result<(), Box<dyn Error>> {
    let face = |advance| Arc::new(UnicodeFont::parse(tiny_ttf(&['A', ' '], advance, 0)).unwrap());
    let mut fonts = TextFonts::custom(face(300), None, None, None);
    fonts.add_family("Body Sans", face(400), Some(face(500)), None, None).unwrap();
    fonts.add_family("Heading Sans", face(800), None, Some(face(900)), None).unwrap();
    let body = fonts.font(" BODY SANS ", true, false);
    let heading = fonts.font("Heading Sans", false, true);
    assert_ne!(body, heading, "two sans families must not collapse to Helvetica");
    assert_eq!(fonts.text_width(body, 10.0, "AA"), 10.0);
    assert_eq!(fonts.text_width(heading, 10.0, "AA"), 18.0);
    assert_eq!(fonts.text_width(fonts.font("Heading Sans", true, true), 10.0, "AA"), 16.0, "missing named style uses its own regular face");
    assert_eq!(fonts.text_width(fonts.font("Unmapped", false, false), 10.0, "AA"), 6.0, "global font is the fallback");
    assert!(fonts.add_family("body sans", face(700), None, None, None).is_err(), "case-insensitive duplicate");
    let mut doc = PdfDoc::default();
    doc.set_text_fonts(fonts).unwrap();
    doc.add_page(300.0, 300.0).text(10.0, 20.0, 10.0, body, [0, 0, 0], "AA");
    let pg = doc.add_page(300.0, 300.0);
    pg.text(10.0, 20.0, 10.0, heading, [0, 0, 0], "AA");
    pg.text(10.0, 40.0, 10.0, body, [0, 0, 0], "AA");
    let bytes = doc.finish();
    check_xref(&bytes)?;
    assert!(find(&bytes, b"/U6 10 Tf").is_some());
    assert!(find(&bytes, b"/U11 10 Tf").is_some());
    assert_eq!(bytes.windows(10).filter(|w| *w == b"/FontFile2").count(), 2, "only used faces embed, once across pages");
    assert!(find(&bytes, b"[500]").is_some());
    assert!(find(&bytes, b"[900]").is_some());
    Ok(())
}

#[test]
fn named_families_bound_registration_and_detect_missing_glyphs() {
    let face = Arc::new(UnicodeFont::parse(tiny_ttf(&['A'], 500, 0)).unwrap());
    let mut fonts = TextFonts::default();
    for name in ["".to_string(), " ".to_string(), "A".repeat(256)] {
        assert!(fonts.add_family(&name, face.clone(), None, None, None).is_err());
    }
    for i in 0..MAX_FONT_FAMILIES {
        fonts.add_family(&format!("Family {i}"), face.clone(), None, None, None).unwrap();
    }
    assert!(fonts.add_family("too many", face, None, None, None).is_err());
    let mapped = fonts.font("Family 0", false, false);
    assert_eq!(fonts.missing_glyph(mapped, "Ω"), Some('Ω'));
    assert_eq!(fonts.missing_glyph(Font::Helvetica, "Ω"), None);
    assert_eq!(fonts.text_width(Font::Embedded(usize::MAX), 10.0, "A"), 6.0, "hostile handles cannot panic");
}

#[test]
fn shared_glyph_keeps_each_unicode_character_copyable_across_pages() -> Result<(), Box<dyn Error>> {
    let mut bytes = tiny_ttf(&['A', 'B', 'Ω'], 600, 0);
    let record = find(&bytes, b"cmap").unwrap();
    let cmap = u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
    // Four format-4 segments (A, B, Omega and sentinel): make B share A's glyph.
    let delta = cmap + 12 + 16 + 4 * 4 + 2;
    bytes[delta..delta + 2].copy_from_slice(&1u16.wrapping_sub('B' as u16).to_be_bytes());
    let font = Arc::new(UnicodeFont::parse(bytes).unwrap());
    assert_eq!(font.glyph('A'), font.glyph('B'));
    let mut doc = PdfDoc::default();
    doc.set_text_fonts(TextFonts::custom(font, None, None, None)).unwrap();
    doc.add_page(100.0, 100.0).text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "ABΩ");
    doc.add_page(100.0, 100.0).text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "BA");
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"> <0041>").is_some(), "A must survive Unicode extraction");
    assert!(find(&pdf, b"> <0042>").is_some(), "B must survive even though it shares A's glyph");
    assert!(find(&pdf, b"<0001> <0041>").is_some());
    assert!(find(&pdf, b"<0002> <0042>").is_some());
    assert!(find(&pdf, b"stream\n\0\0\0\x01\0\x01\0\x03\nendstream").is_some(), "two CIDs map to the shared glyph");
    Ok(())
}

#[test]
fn supplementary_characters_keep_utf16_surrogate_pair_extraction() -> Result<(), Box<dyn Error>> {
    let font = Arc::new(UnicodeFont::parse(tiny_ttf(&['A', 'Ω', '😀'], 600, 0)).unwrap());
    assert_eq!(font.glyph('😀'), 3, "format-12 font fixture covers supplementary Unicode");
    let mut doc = PdfDoc::default();
    doc.set_text_fonts(TextFonts::custom(font, None, None, None)).unwrap();
    doc.add_page(100.0, 100.0).text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "AΩ😀");
    assert_eq!(doc.missing_glyph(), None);
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, b"<000100020003> Tj").is_some());
    assert!(find(&pdf, b"<0003> <D83DDE00>").is_some(), "ToUnicode uses the UTF-16 surrogate pair");
    Ok(())
}

#[test]
fn unicode_alias_maps_are_bounded_per_page_and_across_pages() {
    let font = Arc::new(UnicodeFont::parse(tiny_ttf(&['A'], 600, 0)).unwrap());
    let fonts = TextFonts::custom(font, None, None, None);
    let chars: Vec<_> = (0x1000..=0x10FFFF).filter_map(char::from_u32).take(MAX_FONT_CHARACTERS + 1).collect();
    let mut doc = PdfDoc::default();
    doc.set_text_fonts(fonts.clone()).unwrap();
    let text: String = chars.iter().collect();
    doc.add_page(100.0, 100.0).text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], &text);
    assert_eq!(doc.pages[0].glyphs[0].len(), MAX_FONT_CHARACTERS);
    assert!(doc.text_limit_exceeded());
    let mut doc = PdfDoc::default();
    doc.set_text_fonts(fonts).unwrap();
    for chunk in chars.chunks(MAX_FONT_CHARACTERS / 2) {
        let text: String = chunk.iter().collect();
        doc.add_page(100.0, 100.0).text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], &text);
    }
    assert!(doc.pages.iter().take(2).all(|p| !p.text_limit_exceeded));
    assert!(doc.pages.last().unwrap().text_limit_exceeded);
    assert!(doc.text_limit_exceeded(), "the document-wide union also respects the cap");
}

#[test]
fn missing_glyph_checks_the_space_used_to_render_a_tab() {
    let face = Arc::new(UnicodeFont::parse(tiny_ttf(&['A'], 600, 0)).unwrap());
    let fonts = TextFonts::custom(face, None, None, None);
    assert_eq!(fonts.missing_glyph(Font::Helvetica, "A\tA"), Some(' '));
}

#[test]
fn separate_script_fallbacks_match_measurement_and_drawing_for_both_weights() -> Result<(), Box<dyn Error>> {
    let face = |chars: &[char], width| Arc::new(UnicodeFont::parse(tiny_ttf(chars, width, 0)).unwrap());
    let fonts =
        TextFonts::fallback([Some(face(&['Ω', 'ω'], 400)), Some(face(&['Ω', 'ω'], 500)), Some(face(&['Ж', 'џ'], 600)), Some(face(&['Ж', 'џ'], 700))]);
    assert!((fonts.text_width(Font::Helvetica, 10.0, "ΩЖAΩ") - 20.67).abs() < 0.001);
    assert_eq!(fonts.text_width(Font::HelveticaBold, 10.0, "ΩЖ"), 12.0);
    assert_eq!(fonts.text_width(Font::Helvetica, 10.0, "ΩЖΨ"), 20.0, "unavailable characters retain the CID fallback");
    let mut doc = PdfDoc::with_text_fonts(fonts);
    let pg = doc.add_page(100.0, 100.0);
    pg.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "ΩЖAΩ");
    pg.text_rotated(Point::new(10.0, 40.0), 10.0, Font::HelveticaBold, [0, 0, 0], "ΩЖ", std::f32::consts::FRAC_PI_2);
    let pdf = doc.finish();
    check_xref(&pdf)?;
    for entry in
        ["/U1 10 Tf", "/U2 10 Tf", "/U3 10 Tf", "/U4 10 Tf", "1 0 0 1 14 80 Tm", "1 0 0 1 20 80 Tm", "1 0 0 1 26.67 80 Tm", "0 -1 1 0 10 55 Tm"]
    {
        assert!(find(&pdf, entry.as_bytes()).is_some(), "missing {entry}");
    }
    assert_eq!(pdf.windows(10).filter(|w| *w == b"/FontFile2").count(), 4);
    assert!(find(&pdf, b"UniGB-UTF16-H").is_none(), "available scripts must use their embedded faces");
    assert!(find(&pdf, b"> <03A9>").is_some());
    assert!(find(&pdf, b"> <0416>").is_some());
    let mixed_weights = TextFonts::fallback([Some(face(&['Ω', 'Ж'], 900)), Some(face(&['Ω'], 500)), None, Some(face(&['Ж'], 700))]);
    assert_eq!(mixed_weights.text_width(Font::HelveticaBold, 10.0, "ΩЖ"), 12.0, "separate bold faces take precedence over a combined regular face");
    Ok(())
}

#[test]
fn changing_fonts_after_a_page_is_rejected_without_corrupting_metrics_or_glyphs() -> Result<(), Box<dyn Error>> {
    let first_bytes = tiny_ttf(&['A', 'Ω'], 500, 0);
    let second_bytes = tiny_ttf(&[' ', 'A', 'Ω'], 900, 0);
    let first = Arc::new(UnicodeFont::parse(first_bytes.clone()).unwrap());
    let second = Arc::new(UnicodeFont::parse(second_bytes.clone()).unwrap());
    assert_ne!(first.glyph('A'), second.glyph('A'));
    let mut doc = PdfDoc::with_text_fonts(TextFonts::custom(first, None, None, None));
    let pg = doc.add_page(100.0, 100.0);
    pg.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "AΩ");
    assert!(doc.set_text_fonts(TextFonts::custom(second.clone(), None, None, None)).is_err());
    assert!(doc.set_unicode_fonts([Some(second), None, None, None]).is_err(), "the legacy setter must enforce the same invariant");
    assert_eq!(doc.page_mut(0).unwrap().text_width(Font::Helvetica, 10.0, "AΩ"), 10.0);
    let pg = doc.add_page(100.0, 100.0);
    assert_eq!(pg.text_width(Font::Helvetica, 10.0, "AΩ"), 10.0, "new pages retain the original metrics");
    pg.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "ΩA");
    let pdf = doc.finish();
    check_xref(&pdf)?;
    assert!(find(&pdf, &first_bytes).is_some(), "the original face is embedded");
    assert!(find(&pdf, &second_bytes).is_none(), "the rejected face cannot replace the original resource");
    assert_eq!(pdf.windows(10).filter(|w| *w == b"/FontFile2").count(), 1);
    assert!(find(&pdf, b"<00010002> Tj").is_some());
    assert!(find(&pdf, b"<00020001> Tj").is_some());
    assert!(find(&pdf, b"<0001> <0041>").is_some());
    assert!(find(&pdf, b"<0002> <03A9>").is_some());
    Ok(())
}

#[test]
fn a_loader_registered_after_initial_lookups_supplies_cached_fonts_to_new_documents() -> Result<(), Box<dyn Error>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static LOADS: AtomicUsize = AtomicUsize::new(0);
    fn load() -> [Option<Vec<u8>>; 4] {
        LOADS.fetch_add(1, Ordering::SeqCst);
        [Some(tiny_ttf(&['Ω'], 500, 0)), Some(tiny_ttf(&['Ω'], 700, 0)), None, None]
    }
    // A local cache exercises registration order without changing process-wide test state.
    let cache = SystemFontCache::new();
    assert!(cache.unicode().iter().all(Option::is_none));
    assert_eq!(cache.text().text_width(Font::Helvetica, 10.0, "Ω"), 10.0);
    let mut existing = PdfDoc::with_text_fonts(cache.text().clone());
    existing.add_page(100.0, 100.0).text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "Ω");
    cache.loader.set(load).unwrap();
    let faces = cache.unicode();
    assert!(faces[..2].iter().all(Option::is_some));
    assert_eq!(cache.text().text_width(Font::Helvetica, 10.0, "Ω"), 5.0);
    assert_eq!(cache.text().text_width(Font::HelveticaBold, 10.0, "Ω"), 7.0);
    assert!(std::ptr::eq(cache.text(), cache.text()), "registered measurement faces are reused");
    assert!(std::ptr::eq(faces, cache.unicode()), "registered parsed faces are reused");
    assert_eq!(LOADS.load(Ordering::SeqCst), 1, "the registered loader runs once");
    assert!(cache.loader.set(|| [None, None, None, None]).is_err(), "first registration wins");
    let mut next = PdfDoc::with_text_fonts(cache.text().clone());
    next.add_page(100.0, 100.0).text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "Ω");
    let next_pdf = next.finish();
    check_xref(&next_pdf)?;
    assert!(find(&next_pdf, b"/FontFile2").is_some());
    let existing_pg = existing.add_page(100.0, 100.0);
    assert_eq!(existing_pg.text_width(Font::Helvetica, 10.0, "Ω"), 10.0, "existing documents retain their original faces");
    existing_pg.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], "Ω");
    let existing_pdf = existing.finish();
    check_xref(&existing_pdf)?;
    assert!(find(&existing_pdf, b"/FontFile2").is_none());
    Ok(())
}
