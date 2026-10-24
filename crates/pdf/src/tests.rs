use super::*;
use std::sync::Arc;

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
    let catalog = FontCatalog::system();
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
    check_xref(&pdf);
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
    assert!(error.contains("no installed embeddable font"));
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
    let catalog = FontCatalog::system();
    let font = match catalog.face_for("Arial", false, false, "a") {
        Ok(font) => font,
        Err(error) => {
            eprintln!("skipping repeated-glyph CID regression; no embeddable Latin font: {error}");
            return;
        }
    };
    let shaped = font.shape("a").expect("installed font shapes a repeated Latin glyph");
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
    let width_1000 = (expected_width as f32 * 1000.0 / f32::from(used.font.units_per_em.max(1))).round() as i32;
    let pdf = doc.finish();
    assert!(find(&pdf, b"/ToUnicode").is_some());
    assert!(find(&pdf, b"/CIDToGIDMap").is_some());
    assert_eq!(pdf.windows(b"1 beginbfchar".len()).filter(|window| *window == b"1 beginbfchar").count(), 1);
    assert!(find(&pdf, b"<0001> <0061>").is_some(), "CID 1 maps back to the visible source character");
    let cid_to_gid = [0, 0, (expected_glyph_id >> 8) as u8, expected_glyph_id as u8];
    assert!(find(&pdf, &cid_to_gid).is_some(), "CIDToGIDMap maps CID 1 to the shaped glyph ID");
    assert!(find(&pdf, format!("/W [1 [{width_1000}]]").as_bytes()).is_some(), "the reused CID retains the shaped glyph width");
}

#[test]
fn cid_reuse_keeps_unicode_glyph_and_contextual_width_distinct() {
    let catalog = FontCatalog::system();
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
    let catalog = FontCatalog::system();
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
    let catalog = FontCatalog::system();
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
fn installed_thai_font_families_and_style_roles_shape_when_available() {
    let catalog = FontCatalog::system();
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
        assert!(found_role, "installed Thai family `{family}` has no tested style role covering Thai");
        tested_families += 1;
    }
    if tested_families == 0 {
        eprintln!("No Leelawadee UI or Tahoma family installed; two-family Thai font checks skipped.");
    } else {
        assert_eq!(tested_families, available_families);
    }
}

#[test]
fn script_fallback_shapes_all_requested_style_roles_when_available() {
    let catalog = FontCatalog::system();
    if catalog.face_for("Arial", false, false, "ผู้ใช้").is_err() {
        assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI requires an embeddable Thai font for style fallback tests");
        eprintln!("No embeddable Thai font is installed; four-role fallback check skipped.");
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
    assert_eq!(Font::family_for("Aptos Display"), Family::Sans);
    assert_eq!(Font::family_for("Times New Roman"), Family::Serif);
    assert_eq!(Font::family_for("Consolas"), Family::Mono);
    assert_eq!(Font::family_for("Courier New"), Family::Mono);
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
