use super::*;
use std::error::Error;
use std::io;
use std::sync::Arc;

mod reference;
use reference::{OldPage, interpret};

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
fn compiled_thai_fixture_is_embeddable_and_shapes_marks() {
    let bytes = crate::font::CRAFT_FONTS.iter().find(|(path, _)| *path == "fonts/noto-sans-thai/NotoSansThai.ttf").map(|(_, bytes)| *bytes);
    let Some(bytes) = bytes else {
        assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI must compile the pinned Noto Sans Thai fixture");
        return;
    };
    let font = PdfFont::from_bytes(std::sync::Arc::<[u8]>::from(bytes), 0).expect("the pinned OFL font permits TrueType embedding");
    assert_eq!(font.family(), "Noto Sans Thai");
    let glyphs = font.shape("น้ำกุ้งผู้ใช้เก้าอี้").unwrap();
    assert!(glyphs.iter().all(|glyph| glyph.glyph_id != 0));
    assert!(glyphs.iter().any(|glyph| glyph.x_offset != 0 || glyph.y_offset != 0), "the CI fixture must exercise Thai mark positioning");
}

#[test]
fn thai_text_is_not_silently_replaced_by_winansi_question_marks() {
    assert_eq!(win_ansi('น'), b'?', "baseline: the former standard-font path replaced Thai with '?' ");
    let catalog = FontCatalog::bundled();
    let font = match catalog.face_for("Arial", false, false, "น้ำ") {
        Ok(font) => font,
        Err(error) => {
            assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI requires an embeddable Thai font: {error}");
            eprintln!("skipping embedded Thai font regression: {error}");
            return;
        }
    };
    let shaped = font.shape("น้ำ").expect("a validated font must shape successfully");
    assert!(!shaped.is_empty());
    assert!(shaped.iter().any(|g| g.y_offset != 0 || g.x_offset != 0), "Thai tone/vowel marks should be positioned by OpenType shaping");
    let units_per_em = font.units_per_em;
    let source_maps = unicode_for_clusters("น้ำ", &shaped).expect("shaper clusters map to source text");
    let mut doc = PdfDoc::new();
    let page = doc.add_page(200.0, 200.0);
    let font_id = page.register_unicode_font(font);
    let angle = 0.3f32;
    page.text_unicode_rotated(10.0, 30.0, 12.0, font_id, [0, 0, 0], "น้ำ", angle).expect("valid Thai text should render");
    let pdf = doc.finish();
    check_xref(&pdf).expect("Unicode PDF has a valid xref table");
    assert!(find(&pdf, b"/Subtype /Type0").is_some());
    assert!(find(&pdf, b"/FontFile2").is_some());
    assert!(find(&pdf, b"/ToUnicode").is_some());
    assert!(
        find(&pdf, format!("/ActualText <FEFF{}>", utf16_hex("น้ำ")).as_bytes()).is_some(),
        "logical cluster text is attached to its visible glyphs"
    );
    assert!(!find(&pdf, b"3 Tr").is_some(), "no zero-width invisible semantic CIDs are appended");
    assert!(
        find(&pdf, format!("/ActualText <FEFF{}> >> BDC", utf16_hex("น้ำ")).as_bytes()).is_some(),
        "the visible run is tagged with the exact logical text"
    );
    for (index, source) in source_maps.iter().enumerate() {
        let mapping = format!("<{:04X}> <{}>", index + 1, utf16_hex(source));
        assert!(find(&pdf, mapping.as_bytes()).is_some(), "visible glyph CID must map its source cluster: {mapping}");
    }
    assert!(!find(&pdf, b"(???) Tj").is_some());

    // Check the actual PDF text matrices against HarfRust's upward-positive positions. This
    // catches the sign error in the serialized coordinates, including for rotated text.
    let page_height = 200.0f32;
    let scale = 12.0 / f32::from(units_per_em);
    let (sn, cs) = angle.sin_cos();
    let (mut pen_x, mut pen_y) = (0i64, 0i64);
    for (index, glyph) in shaped.iter().enumerate() {
        let dx = (pen_x as f32 + glyph.x_offset as f32) * scale;
        let dy = -(pen_y as f32 + glyph.y_offset as f32) * scale;
        let px = 10.0 + dx * cs - dy * sn;
        let py = 30.0 + dx * sn + dy * cs;
        let cid = index + 1;
        let expected = format!("{} {} {} {} {} {} Tm <{:04X}> Tj", n(cs), n(-sn), n(sn), n(cs), n(px), n(page_height - py), cid);
        assert!(find(&pdf, expected.as_bytes()).is_some(), "missing correctly positioned glyph matrix: {expected}");
        pen_x += i64::from(glyph.x_advance);
        pen_y += i64::from(glyph.y_advance);
    }
}

#[test]
fn unicode_glyph_origin_converts_both_vertical_signs_before_rotation() {
    // With a quarter-turn, an upward-positive shaped Y offset rotates into positive page X.
    let positive = unicode_glyph_origin(10.0, 30.0, 2, 5, 1, 3, 2.0, 0.0, 1.0);
    assert!((positive.0 - 26.0).abs() < 0.001);
    assert!((positive.1 - 36.0).abs() < 0.001);

    // A negative shaped Y offset moves down the page before rotation, hence negative page X.
    let negative = unicode_glyph_origin(10.0, 30.0, 2, 5, 1, -7, 2.0, 0.0, 1.0);
    assert!((negative.0 - 6.0).abs() < 0.001);
    assert!((negative.1 - 36.0).abs() < 0.001);
}

#[test]
fn missing_or_corrupt_unicode_fonts_return_errors_without_panicking() {
    assert!(PdfFont::from_bytes(Arc::<[u8]>::from([0u8; 24]), 0).is_err());
    let mut collection = vec![0u8; 16];
    collection[..4].copy_from_slice(b"ttcf");
    collection[8..12].copy_from_slice(&1u32.to_be_bytes());
    assert!(PdfFont::from_bytes(Arc::<[u8]>::from(collection), 1).is_err(), "wrong TTC face index must be rejected safely");
    let catalog = FontCatalog::empty();
    let error = catalog.face_for("Unavailable Family", false, false, "ภาษาไทย").err().unwrap_or_default();
    assert!(error.contains("no bundled or supplied embeddable font"));
}

#[test]
fn source_clusters_map_logical_unicode_once_and_long_runs_need_no_semantic_chunks() {
    let text = "a\u{301}bc";
    let shaped = [0, 0, 3, 4].map(|cluster| PositionedGlyph { glyph_id: 1, cluster, x_advance: 10, y_advance: 0, x_offset: 0, y_offset: 0 });
    assert_eq!(unicode_for_clusters(text, &shaped).unwrap(), ["a\u{301}", "", "b", "c"]);

    // A source cluster can expand to multiple positioned glyphs; it must extract exactly once.
    let expanded = [0, 0].map(|cluster| PositionedGlyph { glyph_id: 1, cluster, x_advance: 0, y_advance: 0, x_offset: 0, y_offset: 0 });
    assert_eq!(unicode_for_clusters("ก", &expanded).unwrap(), ["ก", ""]);

    // Runs longer than the old 256-unit semantic chunk map per source cluster, without adding
    // synthetic chunk origins or changing the Unicode order.
    let long_text = "a".repeat(300);
    let long_glyphs: Vec<PositionedGlyph> =
        (0..300).map(|cluster| PositionedGlyph { glyph_id: 1, cluster, x_advance: 1, y_advance: 0, x_offset: 0, y_offset: 0 }).collect();
    let mapped = unicode_for_clusters(&long_text, &long_glyphs).unwrap();
    assert_eq!(mapped.concat(), long_text);
    assert_eq!(mapped.len(), 300);
}

#[test]
fn repeated_visible_glyphs_exceeding_cid_space_are_accepted() {
    let catalog = FontCatalog::bundled();
    let font = match catalog.face_for("Arial", false, false, "a") {
        Ok(font) => font,
        Err(error) => {
            eprintln!("skipping repeated-glyph CID regression; no embeddable Latin font: {error}");
            return;
        }
    };
    let shaped = font.shape("a").expect("bundled font shapes a repeated Latin glyph");
    assert_eq!(shaped.len(), 1);
    let expected_glyph_id = u16::try_from(shaped[0].glyph_id).expect("font glyph ID fits TrueType CIDToGIDMap");
    let expected_width = shaped[0].x_advance;
    let mut doc = PdfDoc::new();
    let page = doc.add_page(612.0, 792.0);
    let font_id = page.register_unicode_font(font);
    let repeated = "a".repeat(700);
    for index in 0..100 {
        if let Err(error) = page.text_unicode(10.0, 20.0 + index as f32, 10.0, font_id, [0, 0, 0], &repeated) {
            panic!("failed after {index} successful draws / {} glyph occurrences: {error}", index * repeated.len());
        }
    }
    let mappings = page.unicode_fonts.get(&font_id).map_or(0, |used| used.glyphs.len());
    assert_eq!(mappings, 1, "70,000 glyph occurrences require one glyph/Unicode/width mapping");
    assert_eq!(page.content.matches("<0001> Tj").count(), 70_000, "all visible glyph occurrences remain in the content stream");
    let used = page.unicode_fonts.get(&font_id).expect("font resource remains registered");
    assert_eq!(used.cid_by_mapping.len(), 1);
    assert_eq!(used.glyphs[0].glyph_id, expected_glyph_id);
    assert_eq!(used.glyphs[0].unicode, "a");
    assert_eq!(used.glyphs[0].width, expected_width);
    let source_font = used.font.clone();
    let width_1000 = (expected_width as f32 * 1000.0 / f32::from(used.font.units_per_em.max(1))).round() as i32;
    let pdf = doc.finish();
    assert!(find(&pdf, b"/ToUnicode").is_some());
    assert!(find(&pdf, b"/CIDToGIDMap").is_some());
    assert_eq!(pdf.windows(b"1 beginbfchar".len()).filter(|window| *window == b"1 beginbfchar").count(), 1);
    assert!(find(&pdf, b"<0001> <0061>").is_some(), "CID 1 maps back to the visible source character");
    let descriptor = object_with(&pdf, "/Type /FontDescriptor");
    let embedded = object_stream(&pdf, reference(&descriptor, "/FontFile2"));
    let embedded_face = ttf_parser::Face::parse(embedded, 0).expect("embedded face remains a valid TrueType font");
    let cid_font = object_with(&pdf, "/Subtype /CIDFontType2");
    let cid_to_gid = object_stream(&pdf, reference(&cid_font, "/CIDToGIDMap"));
    let subset_glyph_id = u16::from_be_bytes([cid_to_gid[2], cid_to_gid[3]]);
    assert_ne!(subset_glyph_id, 0, "CID 1 maps to a visible glyph in the embedded face");
    let source_face = ttf_parser::Face::parse(&source_font.bytes, source_font.face_index).expect("source face remains valid");
    assert_eq!(outline(&embedded_face, subset_glyph_id), outline(&source_face, expected_glyph_id), "CID 1 retains the shaped source outline");
    assert!(find(&pdf, format!("/W [1 [{width_1000}]]").as_bytes()).is_some(), "the reused CID retains the shaped glyph width");
}

#[test]
fn cid_reuse_keeps_unicode_glyph_and_contextual_width_distinct() {
    let catalog = FontCatalog::bundled();
    let font = match catalog.face_for("Arial", false, false, "a") {
        Ok(font) => font,
        Err(error) => {
            eprintln!("skipping CID identity regression; no embeddable Latin font: {error}");
            return;
        }
    };
    let mut resource = PageUnicodeFont { font, glyphs: Vec::new(), cid_by_mapping: std::collections::HashMap::new() };
    assert_eq!(resource.intern_glyph(17, "a".into(), 500).unwrap(), 1);
    assert_eq!(resource.intern_glyph(17, "a".into(), 500).unwrap(), 1, "identical mappings reuse the CID");
    assert_eq!(resource.intern_glyph(17, "b".into(), 500).unwrap(), 2, "different source Unicode needs a different ToUnicode mapping");
    assert_eq!(resource.intern_glyph(17, "a".into(), 501).unwrap(), 3, "contextual advance needs a different width mapping");
    assert_eq!(resource.intern_glyph(18, "a".into(), 500).unwrap(), 4, "different outlines need different CIDToGIDMap entries");
    assert_eq!(resource.glyphs.iter().map(|entry| entry.glyph_id).collect::<Vec<_>>(), [17, 17, 17, 18]);
    assert_eq!(resource.glyphs.iter().map(|entry| entry.unicode.as_str()).collect::<Vec<_>>(), ["a", "b", "a", "a"]);
    assert_eq!(resource.glyphs.iter().map(|entry| entry.width).collect::<Vec<_>>(), [500, 500, 501, 500]);
}

#[test]
fn cid_resource_accepts_the_last_valid_cid_and_reports_true_mapping_exhaustion() {
    let catalog = FontCatalog::bundled();
    let font = match catalog.face_for("Arial", false, false, "a") {
        Ok(font) => font,
        Err(error) => {
            eprintln!("skipping CID boundary regression; no embeddable Latin font: {error}");
            return;
        }
    };
    let mut resource = PageUnicodeFont { font, glyphs: Vec::new(), cid_by_mapping: std::collections::HashMap::new() };
    for index in 0..MAX_UNICODE_CIDS {
        let cid = resource.intern_glyph(1, format!("u{index}"), 100).expect("all 65,535 usable CIDs can be allocated");
        assert_eq!(usize::from(cid), index + 1);
    }
    assert_eq!(resource.intern_glyph(1, "u0".into(), 100).unwrap(), 1, "a duplicate still reuses its CID when the resource is full");
    let error = resource.intern_glyph(1, "one-more".into(), 100).unwrap_err();
    assert!(error.contains("65535-mapping CID limit"), "exhaustion reports the supported mapping limit: {error}");
    assert_eq!(resource.glyphs.len(), MAX_UNICODE_CIDS);
}

#[test]
fn equal_font_names_do_not_merge_distinct_face_bytes() {
    let catalog = FontCatalog::bundled();
    let font = match catalog.face_for("Arial", false, false, "a") {
        Ok(font) => font,
        Err(error) => {
            eprintln!("skipping font-face identity regression; no embeddable Latin font: {error}");
            return;
        }
    };
    let mut distinct_face = font.clone();
    let mut bytes = distinct_face.bytes.to_vec();
    bytes.push(0);
    distinct_face.bytes = Arc::from(bytes);
    assert_eq!(font.postscript_name, distinct_face.postscript_name);
    assert_eq!(font.family, distinct_face.family);
    assert_ne!(font_key(&font), font_key(&distinct_face));

    let mut doc = PdfDoc::new();
    let page = doc.add_page(100.0, 100.0);
    let first = page.register_unicode_font(font);
    let second = page.register_unicode_font(distinct_face);
    assert_ne!(first, second, "equal names with different face bytes need distinct resources");
}

#[test]
fn bundled_thai_font_families_and_style_roles_shape_when_available() {
    let catalog = FontCatalog::bundled();
    let mut tested_families = 0;
    let available_families = ["Leelawadee UI", "Tahoma"].iter().filter(|family| catalog.has_family(family)).count();
    for family in ["Leelawadee UI", "Tahoma"] {
        if !catalog.has_family(family) {
            continue;
        }
        let mut found_role = false;
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let Ok(font) = catalog.face_for(family, bold, italic, "ผู้ใช้") else { continue };
            if !font.family().eq_ignore_ascii_case(family) || font.is_bold() != bold || font.is_italic() != italic {
                continue;
            }
            assert!(!font.shape("ผู้ใช้").unwrap_or_default().is_empty());
            found_role = true;
        }
        assert!(found_role, "bundled Thai family `{family}` has no tested style role covering Thai");
        tested_families += 1;
    }
    if tested_families == 0 {
        eprintln!("No bundled Leelawadee UI or Tahoma family; two-family Thai font checks skipped.");
    } else {
        assert_eq!(tested_families, available_families);
    }
}

#[test]
fn bundled_script_fallback_shapes_all_requested_style_roles_when_available() {
    let catalog = FontCatalog::bundled();
    if catalog.face_for("Arial", false, false, "ผู้ใช้").is_err() {
        assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI requires an embeddable Thai font for style fallback tests");
        eprintln!("No bundled embeddable Thai font; four-role fallback check skipped.");
        return;
    }
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let result = catalog.face_for("Arial", bold, italic, "ผู้ใช้");
        assert!(result.is_ok(), "missing {bold}/{italic} fallback role: {result:?}");
        if let Ok(font) = result {
            // A nearest-style face is valid when no exact bold/italic face exists.
            assert!(!font.shape("ผู้ใช้").unwrap_or_default().is_empty());
        }
    }
}

#[test]
fn regular_only_thai_font_serves_every_requested_style_role() {
    let bytes = crate::font::CRAFT_FONTS.iter().find(|(path, _)| *path == "fonts/noto-sans-thai/NotoSansThai.ttf").map(|(_, bytes)| *bytes);
    let Some(bytes) = bytes else {
        assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI must supply the regular-only Noto Sans Thai fixture");
        return;
    };
    let mut catalog = FontCatalog::empty();
    catalog.add_font(std::sync::Arc::<[u8]>::from(bytes), 0).unwrap();
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        assert_eq!(
            catalog.family_face_for("Noto Sans Thai", bold, italic, "น้ำผู้ใช้").is_ok(),
            !bold && !italic,
            "family-only resolution must not silently substitute an unavailable style"
        );
        let font = catalog.face_for("Arial", bold, italic, "น้ำผู้ใช้").expect("a regular face is a valid nearest-style fallback");
        assert_eq!(font.family(), "Noto Sans Thai");
        assert!(!font.is_bold() && !font.is_italic(), "this fixture must exercise a regular-only fallback");
        let glyphs = font.shape("น้ำผู้ใช้").unwrap();
        assert!(!glyphs.is_empty() && glyphs.iter().all(|glyph| glyph.glyph_id != 0));
        assert!(glyphs.iter().any(|glyph| glyph.x_offset != 0 || glyph.y_offset != 0));
    }
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
    assert_eq!(Font::family_for("Aptos Display"), Family::Sans);
    assert_eq!(Font::family_for("Times New Roman"), Family::Serif);
    assert_eq!(Font::family_for("Consolas"), Family::Mono);
    assert_eq!(Font::family_for("Courier New"), Family::Mono);
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

// ---------------------------------------------------------------------------------------------
// Writer cleanup: equivalence with the previous content writer, structure, and one test per
// behaviour change.
// ---------------------------------------------------------------------------------------------

/// Offsets of the in-use objects, by id, from the xref table.
fn xref_offsets(pdf: &[u8]) -> Vec<usize> {
    let sx = rfind(pdf, b"startxref\n").unwrap();
    let rest = std::str::from_utf8(&pdf[sx + 10..]).unwrap();
    let off: usize = rest.lines().next().unwrap().trim().parse().unwrap();
    let table = std::str::from_utf8(&pdf[off..sx]).unwrap();
    table.lines().skip(2).take_while(|l| l.len() == 18 || l.len() == 19).map(|e| e[..10].parse().unwrap()).collect()
}

struct Obj {
    dict: String,
    stream: Option<Vec<u8>>,
}

/// Parses every object of a document written by this crate, checking that each stream's
/// `/Length` matches its data and is followed by `endstream`/`endobj`.
fn parse_objects(pdf: &[u8]) -> Vec<Obj> {
    let offsets = xref_offsets(pdf);
    let mut objs = Vec::new();
    for (id, off) in offsets.iter().enumerate() {
        if id == 0 {
            objs.push(Obj { dict: String::new(), stream: None });
            continue;
        }
        let tail = &pdf[*off..];
        let head = format!("{id} 0 obj\n");
        assert!(tail.starts_with(head.as_bytes()), "object {id}");
        let body = &tail[head.len()..];
        let end = find(body, b"\nendobj\n").unwrap();
        match find(body, b"\nstream\n").filter(|s| *s < end) {
            Some(s) => {
                let dict = String::from_utf8(body[..s].to_vec()).unwrap();
                let l = dict.find("/Length ").unwrap() + 8;
                let len: usize = dict[l..].split(' ').next().unwrap().parse().unwrap();
                let data = &body[s + 8..];
                assert!(data[len..].starts_with(b"\nendstream\nendobj\n"), "object {id}: /Length {len} is wrong");
                objs.push(Obj { dict, stream: Some(data[..len].to_vec()) });
            }
            None => objs.push(Obj { dict: String::from_utf8(body[..end].to_vec()).unwrap(), stream: None }),
        }
    }
    objs
}

fn ref_of(dict: &str, key: &str) -> Option<usize> {
    let i = dict.find(key)? + key.len();
    dict[i..].trim_start().split(' ').next()?.parse().ok()
}

struct ParsedPage {
    media: String,
    content: String,
    /// `(Im name, object id)`.
    xobjects: Vec<(String, usize)>,
}

fn parse_pages(pdf: &[u8]) -> (Vec<Obj>, Vec<ParsedPage>) {
    let objs = parse_objects(pdf);
    let mut pages = Vec::new();
    for o in &objs {
        if !o.dict.contains("/Type /Page ") {
            continue;
        }
        let m = o.dict.find("/MediaBox [").unwrap() + 10;
        let media = o.dict[m..m + o.dict[m..].find(']').unwrap() + 1].to_string();
        let content_id = ref_of(&o.dict, "/Contents").unwrap();
        let content = String::from_utf8(objs[content_id].stream.clone().unwrap()).unwrap();
        let mut xobjects = Vec::new();
        if let Some(x) = o.dict.find("/XObject << ") {
            let rest = &o.dict[x + 11..];
            let rest = &rest[..rest.find(">>").unwrap()];
            let toks: Vec<&str> = rest.split_whitespace().collect();
            for c in toks.chunks(4) {
                xobjects.push((c[0].to_string(), c[1].parse().unwrap()));
            }
        }
        pages.push(ParsedPage { media, content, xobjects });
    }
    (objs, pages)
}

fn rle_decode(d: &[u8]) -> Vec<u8> {
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

/// Decodes an image XObject: `(width, height, rgb, alpha)`.
fn decode_image(objs: &[Obj], id: usize) -> (u32, u32, Vec<u8>, Option<Vec<u8>>) {
    let o = &objs[id];
    let w = ref_of(&o.dict, "/Width").unwrap() as u32;
    let h = ref_of(&o.dict, "/Height").unwrap() as u32;
    let rgb = rle_decode(o.stream.as_ref().unwrap());
    let alpha = ref_of(&o.dict, "/SMask").map(|m| rle_decode(objs[m].stream.as_ref().unwrap()));
    (w, h, rgb, alpha)
}

/// Deterministic pseudo-random numbers for the corpus.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn f(&mut self) -> f32 {
        self.below(600) as f32 + self.below(4) as f32 * 0.25
    }
}

const PALETTE: [Rgb; 4] = [[0, 0, 0], [255, 0, 0], [12, 200, 99], [255, 255, 255]];
const WIDTHS: [f32; 3] = [0.5, 1.0, 2.5];
const DASHES: [&[f32]; 3] = [&[], &[3.0, 2.0], &[1.0]];

/// A picture used by the corpus: `(rgba, w, h)`.
fn pictures() -> Vec<(Vec<u8>, u32, u32)> {
    let mut with_alpha = Vec::new();
    for i in 0..16u8 {
        with_alpha.extend_from_slice(&[i * 10, 0, 7, if i < 8 { 255 } else { 128 }]);
    }
    let opaque: Vec<u8> = (0..4 * 9).map(|i| if i % 4 == 3 { 255 } else { (i * 7) as u8 }).collect();
    let mut last_px = vec![200u8; 4 * 6];
    for px in last_px.chunks_mut(4) {
        px[3] = 255;
    }
    last_px[4 * 5 + 3] = 3; // only the very last pixel is translucent
    vec![(with_alpha, 4, 4), (opaque, 3, 3), (last_px, 3, 2)]
}

/// Applies a random operation to both writers.
fn random_op(rng: &mut Lcg, new: &mut Page, old: &mut OldPage, pics: &[(Vec<u8>, u32, u32)], depth: &mut usize) {
    let color = PALETTE[rng.below(4) as usize];
    let width = WIDTHS[rng.below(3) as usize];
    let dash = DASHES[rng.below(3) as usize];
    let (x, y, w, h) = (rng.f(), rng.f(), rng.f() / 4.0 + 1.0, rng.f() / 4.0 + 1.0);
    let pts = [[x, y], [x + w, y], [x + w, y + h]];
    let stroke = if rng.below(2) == 0 { Some((PALETTE[rng.below(4) as usize], WIDTHS[rng.below(3) as usize])) } else { None };
    let fill = if rng.below(2) == 0 { Some(color) } else { None };
    match rng.below(14) {
        0 | 1 => {
            new.fill_rect(x, y, w, h, color);
            old.fill_rect(x, y, w, h, color);
        }
        2 => {
            new.rect_stroke(x, y, w, h, width, color);
            old.rect_stroke(x, y, w, h, width, color);
        }
        3 => {
            new.stroke_line_dashed(Point::new(x, y), Point::new(x + w, y + h), width, color, dash);
            old.stroke_line_dashed(Point::new(x, y), Point::new(x + w, y + h), width, color, dash);
        }
        4 => {
            new.polyline(&pts, width, color, dash);
            old.polyline(&pts, width, color, dash);
        }
        5 => {
            new.polygon(&pts, fill, stroke);
            old.polygon(&pts, fill, stroke);
        }
        6 => {
            new.circle(x, y, w, fill, stroke);
            old.circle(x, y, w, fill, stroke);
        }
        7 | 8 => {
            let font = Font::ALL[rng.below(12) as usize];
            let angle = if rng.below(3) == 0 { 0.5 } else { 0.0 };
            new.text_rotated(Point::new(x, y), 10.0, font, color, "Hi (there)", angle);
            old.text_rotated(Point::new(x, y), 10.0, font, color, "Hi (there)", angle);
        }
        9 => {
            new.clip_rect(x, y, w, h);
            old.clip_rect(x, y, w, h);
        }
        10 if *depth < 20 => {
            new.save();
            old.save();
            *depth += 1;
        }
        11 if *depth > 0 => {
            new.restore();
            old.restore();
            *depth -= 1;
        }
        12 => {
            let (px, pw, ph) = &pics[rng.below(3) as usize];
            new.image_rgba(Rect::new(x, y, w, h), px, [*pw, *ph]);
            old.image(x, y, w, h);
        }
        _ => {
            new.text(x, y, 9.0, Font::Helvetica, color, "Plain");
            old.text_rotated(Point::new(x, y), 9.0, Font::Helvetica, color, "Plain", 0.0);
        }
    }
}

/// Builds the same random multi-page document with the new writer and the old content writer.
fn corpus_doc(seed: u64) -> (Vec<u8>, Vec<String>) {
    let mut rng = Lcg(seed);
    let pics = pictures();
    let mut doc = PdfDoc::new();
    let mut old_pages = Vec::new();
    for _ in 0..(1 + rng.below(4)) {
        let mut old = OldPage::new(792.0);
        let mut depth = 0usize;
        let page = doc.add_page(612.0, 792.0);
        for _ in 0..120 {
            random_op(&mut rng, page, &mut old, &pics, &mut depth);
        }
        while depth > 0 {
            page.restore();
            old.restore();
            depth -= 1;
        }
        old_pages.push(old.content);
    }
    (doc.finish(), old_pages)
}

#[test]
fn new_writer_is_equivalent_to_the_old_one() -> Result<(), Box<dyn Error>> {
    let mut style_new = 0;
    let mut style_old = 0;
    for seed in 1..=40u64 {
        let (pdf, old_pages) = corpus_doc(seed);
        check_xref(&pdf)?;
        let (_, pages) = parse_pages(&pdf);
        assert_eq!(pages.len(), old_pages.len());
        for (p, old) in pages.iter().zip(&old_pages) {
            assert_eq!(p.media, "[0 0 612 792]");
            let new = interpret(&p.content);
            let old = interpret(old);
            assert!(new.balanced && old.balanced, "q/Q balanced (seed {seed})");
            assert_eq!(new.paints.len(), old.paints.len(), "seed {seed}");
            for (i, (a, b)) in new.paints.iter().zip(&old.paints).enumerate() {
                // Round joins leaking from polylines into later rectangles is the old bug (B7),
                // compared separately in `join_does_not_leak_from_polylines`.
                let mut a = a.clone();
                let mut b = b.clone();
                a.join.clear();
                b.join.clear();
                assert_eq!(a, b, "seed {seed}, painting op {i}");
            }
            style_new += new.style_ops;
            style_old += old.style_ops;
        }
    }
    // Sanity: the corpus really contains style changes, and the new writer emits fewer.
    assert!(style_old > 1000, "corpus too small: {style_old}");
    assert!(style_new < style_old, "{style_new} vs {style_old}");
    Ok(())
}

#[test]
fn structure_is_valid_for_all_documents() -> Result<(), Box<dyn Error>> {
    for seed in 1..=40u64 {
        let (pdf, _) = corpus_doc(seed);
        check_xref(&pdf)?;
        parse_objects(&pdf); // every /Length matches its data
    }
    let empty = PdfDoc::new().finish();
    check_xref(&empty)?;
    parse_objects(&empty);
    Ok(())
}

#[test]
fn images_decode_to_their_pixels() {
    let pics = pictures();
    let mut d = PdfDoc::new();
    for _ in 0..2 {
        let p = d.add_page(300.0, 300.0);
        for (px, w, h) in &pics {
            p.image_rgba(Rect::new(1.0, 1.0, 20.0, 20.0), px, [*w, *h]);
        }
    }
    let pdf = d.finish();
    let (objs, pages) = parse_pages(&pdf);
    assert_eq!(pages.len(), 2);
    for page in &pages {
        assert_eq!(page.xobjects.len(), 3);
        for ((px, w, h), (_, id)) in pics.iter().zip(&page.xobjects) {
            let (dw, dh, rgb, alpha) = decode_image(&objs, *id);
            assert_eq!((dw, dh), (*w, *h));
            let want_rgb: Vec<u8> = px.chunks(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
            assert_eq!(rgb, want_rgb);
            let want_alpha: Vec<u8> = px.chunks(4).map(|c| c[3]).collect();
            match alpha {
                Some(a) => assert_eq!(a, want_alpha),
                None => assert!(want_alpha.iter().all(|a| *a == 255)),
            }
        }
    }
}

/// O6: no alpha plane (and no SMask object) for an opaque image; a single translucent pixel
/// anywhere (first, middle, last) produces the full plane.
#[test]
fn opaque_images_have_no_soft_mask() {
    let mut d = PdfDoc::new();
    let p = d.add_page(100.0, 100.0);
    p.image_rgba(Rect::new(0.0, 0.0, 10.0, 10.0), &[255; 4 * 12], [4, 3]);
    assert!(p.images[0].alpha.is_none());
    let pdf = d.finish();
    assert!(find(&pdf, b"/SMask").is_none());
    assert_eq!(pdf.windows(15).filter(|w| *w == b"/Subtype /Image").count(), 1);

    for translucent in [0usize, 5, 11] {
        let mut px = vec![255u8; 4 * 12];
        px[translucent * 4 + 3] = 7;
        let mut d = PdfDoc::new();
        d.add_page(100.0, 100.0).image_rgba(Rect::new(0.0, 0.0, 10.0, 10.0), &px, [4, 3]);
        let (objs, pages) = parse_pages(&d.finish());
        let (_, _, _, alpha) = decode_image(&objs, pages[0].xobjects[0].1);
        let mut want = vec![255u8; 12];
        want[translucent] = 7;
        assert_eq!(alpha, Some(want));
    }
}

/// O4: the same picture on five pages is stored once (image + mask), by every page.
#[test]
fn identical_images_are_stored_once() -> Result<(), Box<dyn Error>> {
    let pics = pictures();
    let (px, w, h) = &pics[0]; // has alpha: image object + mask object
    let mut d = PdfDoc::new();
    for _ in 0..5 {
        let p = d.add_page(200.0, 200.0);
        p.image_rgba(Rect::new(1.0, 1.0, 20.0, 20.0), px, [*w, *h]);
        p.image_rgba(Rect::new(30.0, 1.0, 20.0, 20.0), px, [*w, *h]); // twice on one page
    }
    let pdf = d.finish();
    check_xref(&pdf)?;
    assert_eq!(pdf.windows(15).filter(|w| *w == b"/Subtype /Image").count(), 2, "one image + one soft mask");
    let (_, pages) = parse_pages(&pdf);
    let ids: Vec<usize> = pages.iter().flat_map(|p| p.xobjects.iter().map(|x| x.1)).collect();
    assert_eq!(ids.len(), 5, "one XObject entry per page");
    assert!(ids.iter().all(|i| *i == ids[0]));
    for p in &pages {
        assert_eq!(p.content.matches(" Do").count(), 2);
    }
    Ok(())
}

/// O4: a picture that differs in one byte, or only in size, is not merged.
#[test]
fn different_images_are_not_merged() {
    let a = vec![50u8; 4 * 4];
    let mut b = a.clone();
    b[5] = 51;
    let mut d = PdfDoc::new();
    let p = d.add_page(100.0, 100.0);
    p.image_rgba(Rect::new(0.0, 0.0, 5.0, 5.0), &a, [4, 1]);
    p.image_rgba(Rect::new(0.0, 0.0, 5.0, 5.0), &b, [4, 1]);
    p.image_rgba(Rect::new(0.0, 0.0, 5.0, 5.0), &a, [2, 2]);
    d.add_page(100.0, 100.0).image_rgba(Rect::new(0.0, 0.0, 5.0, 5.0), &a, [4, 1]);
    let pdf = d.finish();
    assert_eq!(pdf.windows(15).filter(|w| *w == b"/Subtype /Image").count(), 6, "three distinct pictures, each with a soft mask (alpha 50)");
}

/// O4: many distinct pictures on one page are looked up by hash, not by comparing each new
/// picture with every earlier one (which made dense picture exports quadratic).
#[test]
fn many_distinct_images_on_one_page_are_not_compared_pairwise() {
    const N: u32 = 3000;
    let mut d = PdfDoc::new();
    let p = d.add_page(612.0, 792.0);
    for i in 0..N {
        let [a, b, c, _] = i.to_le_bytes();
        p.image_rgba(Rect::new(0.0, 0.0, 1.0, 1.0), &[a, b, c, 255], [1, 1]);
    }
    // A repeat of the first picture still shares its XObject.
    p.image_rgba(Rect::new(5.0, 5.0, 1.0, 1.0), &[0, 0, 0, 255], [1, 1]);
    assert_eq!(p.images.len(), N as usize);
    assert!(p.content.ends_with("/Im1 Do Q\n"), "the repeated picture reuses /Im1");
    assert!(p.image_comparisons <= 1, "{} pixel comparisons for {N} distinct pictures", p.image_comparisons);
    let pdf = d.finish();
    assert_eq!(pdf.windows(15).filter(|w| *w == b"/Subtype /Image").count(), N as usize);
}

/// O4: equal hashes alone never merge images; the pixels are compared.
#[test]
fn hash_collisions_do_not_merge_images() {
    let mut d = PdfDoc::new();
    for v in [10u8, 20] {
        d.add_page(100.0, 100.0).image_rgba(Rect::new(0.0, 0.0, 5.0, 5.0), &[v; 16], [2, 2]);
    }
    for p in 0..2 {
        d.page_mut(p).unwrap().images[0].hash = 42; // forced collision
    }
    let (objs, pages) = parse_pages(&d.finish());
    let ids: Vec<usize> = pages.iter().map(|p| p.xobjects[0].1).collect();
    assert_ne!(ids[0], ids[1]);
    assert_eq!(decode_image(&objs, ids[0]).2, vec![10; 12]);
    assert_eq!(decode_image(&objs, ids[1]).2, vec![20; 12]);
}

/// S5: the colour component table equals the old float formatting for every byte.
#[test]
fn colour_components_match_the_float_formatting() {
    for v in 0..=255u8 {
        let mut s = String::new();
        push_unit(&mut s, v);
        assert_eq!(s, n(v as f32 / 255.0), "byte {v}");
    }
    let mut s = String::new();
    push_rgb(&mut s, [255, 128, 0], true);
    assert_eq!(s, "1 0.502 0 RG");
}

/// S3: the obsolete /ProcSet entry is gone.
#[test]
fn no_procset() {
    let (pdf, _) = corpus_doc(3);
    assert!(find(&pdf, b"/ProcSet").is_none());
}

/// O3: a style that is already in effect is not emitted again.
#[test]
fn unchanged_style_is_emitted_once() {
    let mut d = PdfDoc::new();
    let p = d.add_page(200.0, 200.0);
    for i in 0..50 {
        p.rect_stroke(i as f32, 0.0, 5.0, 5.0, 1.5, [10, 20, 30]);
    }
    for i in 0..50 {
        p.stroke_line_dashed(Point::new(0.0, i as f32), Point::new(9.0, 9.0), 1.5, [10, 20, 30], &[2.0, 1.0]);
    }
    for _ in 0..50 {
        p.fill_rect(0.0, 0.0, 4.0, 4.0, [1, 2, 3]);
        p.text(5.0, 5.0, 9.0, Font::Helvetica, [1, 2, 3], "x");
    }
    let s = interpret(&p.content);
    assert_eq!(p.content.matches(" RG\n").count(), 1);
    assert_eq!(p.content.matches(" rg\n").count(), 1);
    assert_eq!(p.content.matches(" w\n").count(), 1);
    assert_eq!(p.content.matches(" d\n").count(), 2, "solid, then [2 1]");
    assert_eq!(s.style_ops, 5);
    assert_eq!(s.paints.len(), 50 * 4);
}

/// O3: the tracked style follows q/Q, and a new page starts from scratch.
#[test]
fn style_tracking_follows_save_restore_and_pages() {
    let red = [255, 0, 0];
    let blue = [0, 0, 255];
    let mut d = PdfDoc::new();
    let p = d.add_page(100.0, 100.0);
    p.fill_rect(0.0, 0.0, 5.0, 5.0, red);
    p.save();
    p.fill_rect(0.0, 0.0, 5.0, 5.0, red); // still red inside: nothing to emit
    p.fill_rect(0.0, 0.0, 5.0, 5.0, blue);
    p.restore();
    p.fill_rect(0.0, 0.0, 5.0, 5.0, red); // red again after Q without a new operator
    p.fill_rect(0.0, 0.0, 5.0, 5.0, blue); // but blue must be re-emitted
    let fills: Vec<String> = interpret(&p.content).paints.iter().map(|p| p.fill.clone()).collect();
    assert_eq!(fills, ["1 0 0", "1 0 0", "0 0 1", "1 0 0", "0 0 1"]);
    assert_eq!(p.content.matches(" rg\n").count(), 3, "red once, blue twice");
    let p2 = d.add_page(100.0, 100.0);
    p2.fill_rect(0.0, 0.0, 5.0, 5.0, red);
    assert_eq!(p2.content.matches(" rg\n").count(), 1, "a new page re-emits the colour");
}

/// B6: only 28 nested saves are written, the matching extra restores are no-ops.
#[test]
fn save_nesting_is_limited_to_28() -> Result<(), Box<dyn Error>> {
    let mut d = PdfDoc::new();
    let p = d.add_page(100.0, 100.0);
    for _ in 0..40 {
        p.save();
    }
    p.fill_rect(0.0, 0.0, 5.0, 5.0, [1, 1, 1]);
    assert_eq!(p.content.matches("q\n").count(), 28);
    // The 12 restores that match refused saves do nothing...
    for _ in 0..12 {
        p.restore();
    }
    assert_eq!(p.content.matches("Q\n").count(), 0);
    assert_eq!(interpret(&p.content).max_depth, 28);
    // ...the next 28 undo the accepted ones, and further restores are ignored.
    for _ in 0..40 {
        p.restore();
    }
    assert_eq!(p.content.matches("Q\n").count(), 28);
    let i = interpret(&p.content);
    assert!(i.balanced);
    // An image needs its own q: dropped at the limit, drawn otherwise.
    for _ in 0..28 {
        p.save();
    }
    p.image_rgba(Rect::new(0.0, 0.0, 5.0, 5.0), &[9; 16], [2, 2]);
    assert!(!p.content.contains(" Do"));
    p.restore();
    p.image_rgba(Rect::new(0.0, 0.0, 5.0, 5.0), &[9; 16], [2, 2]);
    assert!(p.content.contains(" Do"));
    let pdf = d.finish();
    check_xref(&pdf)?;
    Ok(())
}

/// B6: `finish()` closes what is still open, without exceeding the limit.
#[test]
fn finish_closes_open_saves() -> Result<(), Box<dyn Error>> {
    let mut d = PdfDoc::new();
    let p = d.add_page(100.0, 100.0);
    for _ in 0..33 {
        p.save();
    }
    let (_, pages) = parse_pages(&d.finish());
    let i = interpret(&pages[0].content);
    assert!(i.balanced);
    assert_eq!(i.qs, 28);
    Ok(())
}

/// B7: round joins set for a polyline do not leak into later rectangles, lines and circles.
#[test]
fn join_does_not_leak_from_polylines() {
    let mut d = PdfDoc::new();
    let p = d.add_page(200.0, 200.0);
    let pts = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]];
    p.rect_stroke(0.0, 0.0, 5.0, 5.0, 1.0, [0, 0, 0]);
    p.polyline(&pts, 1.0, [0, 0, 0], &[]);
    p.polyline(&pts, 1.0, [0, 0, 0], &[]);
    p.rect_stroke(0.0, 0.0, 5.0, 5.0, 1.0, [0, 0, 0]);
    p.stroke_line(0.0, 0.0, 9.0, 9.0, 1.0, [0, 0, 0]);
    p.polygon(&pts, None, Some(([0, 0, 0], 1.0)));
    p.circle(50.0, 50.0, 5.0, None, Some(([0, 0, 0], 1.0)));
    let joins: Vec<String> = interpret(&p.content).paints.iter().map(|p| p.join.clone()).collect();
    assert_eq!(joins, ["0", "1", "1", "0", "0", "1", "0"]);
    assert_eq!(p.content.matches(" j\n").count(), 4, "1, 0, 1, 0: only on change");
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
    // The writer emits style operators only on change: the two solid lines share one `[] 0 d`.
    let dashes: Vec<String> = interpret(c).paints.iter().map(|p| p.dash.clone()).collect();
    assert_eq!(dashes, ["", "", "3 2"]);
    assert_eq!(c.matches("[] 0 d").count(), 1);
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
    // Style operators are emitted only on change, so judge the width in effect for each stroke.
    let widths: Vec<String> = interpret(c).paints.iter().map(|p| p.width.clone()).collect();
    assert_eq!(widths, ["0", "0", "0", "0"]);
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

/// The decoded data of stream object `id`.
fn object_stream(pdf: &[u8], id: u32) -> &[u8] {
    let start = find(pdf, format!("\n{id} 0 obj\n").as_bytes()).unwrap();
    let object = &pdf[start..];
    let length_at = find(object, b"/Length ").unwrap() + 8;
    let length: usize = object[length_at..].iter().take_while(|b| b.is_ascii_digit()).map(|b| char::from(*b)).collect::<String>().parse().unwrap();
    let data = find(object, b">>\nstream\n").unwrap() + 10;
    &object[data..data + length]
}

/// The dictionary text of the object that contains `needle`.
fn object_with(pdf: &[u8], needle: &str) -> String {
    let at = find(pdf, needle.as_bytes()).unwrap_or_else(|| panic!("no object contains {needle}"));
    let start = rfind(&pdf[..at], b" 0 obj\n").unwrap();
    let end = at + find(&pdf[at..], b"\nendobj").unwrap();
    String::from_utf8_lossy(&pdf[start..end]).into_owned()
}

fn reference(dict: &str, key: &str) -> u32 {
    let at = dict.find(&format!("{key} ")).unwrap_or_else(|| panic!("{key} missing in {dict}"));
    dict[at + key.len()..].split_whitespace().next().unwrap().parse().unwrap()
}

/// The source text each CID of a ToUnicode CMap maps to.
fn to_unicode(cmap: &[u8]) -> std::collections::HashMap<u16, String> {
    let text = std::str::from_utf8(cmap).unwrap();
    let mut map = std::collections::HashMap::new();
    for line in text.lines().filter(|line| line.starts_with('<') && !line.starts_with("<0000> <FFFF>")) {
        let (cid, unicode) = line.split_once("> <").unwrap();
        let unicode = unicode.trim_end_matches('>');
        let units: Vec<u16> = (0..unicode.len()).step_by(4).map(|i| u16::from_str_radix(&unicode[i..i + 4], 16).unwrap()).collect();
        map.insert(u16::from_str_radix(&cid[1..], 16).unwrap(), String::from_utf16(&units).unwrap());
    }
    map
}

#[derive(Default)]
struct OutlineRecorder(Vec<String>);

impl ttf_parser::OutlineBuilder for OutlineRecorder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("M{x},{y}"));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push(format!("L{x},{y}"));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.push(format!("Q{x1},{y1},{x},{y}"));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.push(format!("C{x1},{y1},{x2},{y2},{x},{y}"));
    }
    fn close(&mut self) {
        self.0.push("Z".into());
    }
}

fn outline(face: &ttf_parser::Face, glyph: u16) -> Vec<String> {
    let mut recorder = OutlineRecorder::default();
    face.outline_glyph(ttf_parser::GlyphId(glyph), &mut recorder);
    recorder.0
}

/// The pinned Noto Sans Thai fixture when it is compiled in, else another bundled Thai face.
fn thai_face() -> Option<PdfFont> {
    if let Some((_, bytes)) = crate::font::CRAFT_FONTS.iter().find(|(path, _)| *path == "fonts/noto-sans-thai/NotoSansThai.ttf") {
        return Some(PdfFont::from_bytes(Arc::<[u8]>::from(*bytes), 0).unwrap());
    }
    match FontCatalog::bundled().face_for("Arial", false, false, "ภาษาไทย") {
        Ok(font) => Some(font),
        Err(error) => {
            assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI requires an embeddable Thai font: {error}");
            eprintln!("skipping Thai subset checks: {error}");
            None
        }
    }
}

/// Any embeddable face with a sample it covers.
fn any_face() -> Option<(PdfFont, &'static str)> {
    if let Some(font) = crate::font::CRAFT_FONTS
        .iter()
        .find(|(path, _)| *path == "fonts/noto-sans-thai/NotoSansThai.ttf")
        .and_then(|(_, bytes)| PdfFont::from_bytes(Arc::<[u8]>::from(*bytes), 0).ok())
    {
        return Some((font, "ภาษาไทย"));
    }
    match FontCatalog::bundled().face_for("Arial", false, false, "Total") {
        Ok(font) => Some((font, "Total")),
        Err(error) => {
            eprintln!("skipping font subset checks; no embeddable face: {error}");
            None
        }
    }
}

fn one_run_document(font: &PdfFont, text: &str) -> Vec<u8> {
    let mut doc = PdfDoc::new();
    let page = doc.add_page(200.0, 100.0);
    let id = page.register_unicode_font(font.clone());
    page.text_unicode(10.0, 30.0, 12.0, id, [0, 0, 0], text).unwrap();
    doc.finish()
}

#[test]
fn thai_and_symbol_faces_embed_subsets_that_keep_outlines_and_text() {
    const THAI: &str = "ภาษาไทย น้ำ ผู้ใช้";
    let Some(thai) = thai_face() else { return };
    let mut runs = vec![(thai, THAI)];
    match FontCatalog::bundled().face_for("Arial", false, false, "→") {
        Ok(arrow) => runs.push((arrow, "→")),
        Err(error) => eprintln!("no bundled or supplied face covers the arrow; checking the Thai face only: {error}"),
    }
    let mut doc = PdfDoc::new();
    let page = doc.add_page(300.0, 200.0);
    for (i, (font, text)) in runs.iter().enumerate() {
        let id = page.register_unicode_font(font.clone());
        page.text_unicode(10.0, 30.0 + 20.0 * i as f32, 12.0, id, [0, 0, 0], text).unwrap();
    }
    let pdf = doc.finish();
    check_xref(&pdf).expect("Unicode PDF has a valid xref table");
    let content_id = reference(&object_with(&pdf, "/Type /Page /Parent"), "/Contents");
    let content = std::str::from_utf8(object_stream(&pdf, content_id)).unwrap();
    // Every glyph is drawn as `... Tm <CID> Tj`, in run order.
    let mut cids = content.split("Tm <").skip(1).map(|rest| u16::from_str_radix(&rest[..4], 16).unwrap());

    for (font, text) in &runs {
        let name = pdf_font_name(font);
        let descriptor = object_with(&pdf, &format!("+{name} /Flags"));
        let tagged = descriptor.split("/FontName /").nth(1).unwrap().split(' ').next().unwrap().to_string();
        let (tag, _) = tagged.split_once('+').unwrap();
        assert!(tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()), "subset tag `{tag}`");

        let subset_bytes = object_stream(&pdf, reference(&descriptor, "/FontFile2"));
        let subset = ttf_parser::Face::parse(subset_bytes, 0).unwrap();
        let source = ttf_parser::Face::parse(&font.bytes, 0).unwrap();
        let shaped = font.shape(text).unwrap();
        let used: BTreeSet<u32> = shaped.iter().map(|g| g.glyph_id).collect();
        assert!(subset_bytes.len() * 10 < font.bytes.len(), "{name}: {} of {} bytes", subset_bytes.len(), font.bytes.len());
        assert!(usize::from(subset.number_of_glyphs()) <= used.len() + 1, "{name}: subset keeps only the drawn glyphs and .notdef");
        assert!(usize::from(source.number_of_glyphs()) > 10 * used.len(), "{name}: the source face is much larger than the text");

        let cid_font = object_with(&pdf, &format!("/Subtype /CIDFontType2 /BaseFont /{tagged} "));
        let cid_to_gid = object_stream(&pdf, reference(&cid_font, "/CIDToGIDMap"));
        let type0 = object_with(&pdf, &format!("/Subtype /Type0 /BaseFont /{tagged} "));
        let unicode = to_unicode(object_stream(&pdf, reference(&type0, "/ToUnicode")));
        let mut extracted = String::new();
        for glyph in &shaped {
            let cid = cids.next().unwrap();
            let at = usize::from(cid) * 2;
            let subset_glyph = u16::from_be_bytes([cid_to_gid[at], cid_to_gid[at + 1]]);
            let source_glyph = u16::try_from(glyph.glyph_id).unwrap();
            assert_ne!(subset_glyph, 0, "{name}: CID {cid} maps to a real glyph");
            assert_eq!(outline(&subset, subset_glyph), outline(&source, source_glyph), "{name}: CID {cid} draws its source outline");
            extracted.push_str(&unicode[&cid]);
        }
        assert_eq!(extracted, *text, "{name}: ToUnicode extracts the drawn text");
    }
}

#[test]
fn subset_tags_are_deterministic_and_follow_the_glyph_set() {
    let Some((font, text)) = any_face() else { return };
    let first = one_run_document(&font, text);
    assert_eq!(first, one_run_document(&font, text), "same input, same bytes");
    let tag = |pdf: &[u8]| {
        let descriptor = object_with(pdf, "/Type /FontDescriptor");
        descriptor.split("/FontName /").nth(1).unwrap().split('+').next().unwrap().to_string()
    };
    let shorter: String = text.chars().take(2).collect();
    assert_ne!(tag(&first), tag(&one_run_document(&font, &shorter)), "a different glyph set gets a different tag");
}

#[test]
fn faces_that_forbid_subsetting_or_fail_to_subset_are_embedded_whole() {
    let Some((mut font, text)) = any_face() else { return };
    font.fs_type |= FS_TYPE_NO_SUBSETTING;
    let pdf = one_run_document(&font, text);
    let descriptor = object_with(&pdf, "/Type /FontDescriptor");
    assert!(descriptor.contains(&format!("/FontName /{} ", pdf_font_name(&font))), "whole faces carry no subset tag: {descriptor}");
    assert_eq!(object_stream(&pdf, reference(&descriptor, "/FontFile2")), &font.bytes[..]);

    let mut broken = font.clone();
    broken.fs_type = 0;
    broken.bytes = Arc::from(vec![7u8; 64]);
    let (mut objects, mut next) = (Vec::new(), 1);
    let face = add_unicode_font_base(&broken, &BTreeSet::from([1, 2]), &mut objects, &mut next);
    assert!(face.remapper.is_none() && !face.base_font.contains('+'));
    assert_eq!(face.embedded_glyph_id(2), 2);
    assert!(objects.iter().any(|(_, body)| body.ends_with(&[[7u8; 64].as_slice(), b"\nendstream"].concat())));
}

#[test]
fn many_alternating_runs_share_one_text_matrix() {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(612.0, 792.0);
    p.text(10.0, 20.0, 10.0, Font::Helvetica, [0, 0, 0], &"a\u{4e2d}".repeat(2000));
    let pdf = doc.finish();
    let count = |needle: &[u8]| pdf.windows(needle.len()).filter(|w| *w == needle).count();
    assert_eq!(count(b" Tm\n"), 1, "the position is set once, before the first run");
    assert_eq!(count(b" Tf\n"), 4000, "every run selects its font");
    assert_eq!(count(b" Tj\n"), 4000);
}

#[test]
fn symbol_font_draws_maths_in_its_own_encoding() {
    let mut doc = PdfDoc::new();
    let p = doc.add_page(200.0, 200.0);
    p.text(10.0, 50.0, 12.0, Font::Symbol, [0, 0, 0], "\u{2211}\u{03B1}\u{2264}\u{221E}x");
    p.text(10.0, 80.0, 12.0, Font::TimesItalic, [0, 0, 0], "x");
    let pdf = doc.finish();
    let s = String::from_utf8_lossy(&pdf);
    assert!(s.contains("/BaseFont /Symbol >>"), "Symbol has no WinAnsi encoding");
    assert!(s.contains("/BaseFont /Times-Italic /Encoding /WinAnsiEncoding"));
    // The sum, alpha, less-or-equal and infinity codes; x is not in Symbol and becomes `?`.
    assert!(s.contains("(\\345a\\243\\245?) Tj"), "{s}");
    check_xref(&pdf).unwrap();
    assert!(Font::Symbol.can_encode('\u{2211}') && !Font::Symbol.can_encode('x'));
    assert!(Font::TimesRoman.can_encode('x') && !Font::TimesRoman.can_encode('\u{2211}'));
    assert_eq!(text_width(Font::Symbol, 10.0, "\u{2211}"), 7.13);
}
