use super::super::text_unicode::{TextRun, TextRunFont, draw_text_runs};
use super::*;

fn thai_catalog() -> Option<(FontCatalog, PdfFont)> {
    let catalog = FontCatalog::system();
    match catalog.face_for("Arial", false, false, "น้ำกุ้งผู้ใช้เก้าอี้ภาษาไทย") {
        Ok(font) => Some((catalog, font)),
        Err(error) => {
            assert!(std::env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI requires an embeddable Thai font: {error}");
            eprintln!("skipping Thai shaping regression; no embeddable Thai font: {error}");
            None
        }
    }
}

fn minimal_test_ttf() -> Vec<u8> {
    let mut head = vec![0; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mut maxp = vec![0; 6];
    maxp[0..4].copy_from_slice(&0x0000_5000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&1u16.to_be_bytes());
    let mut hhea = vec![0; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[34..36].copy_from_slice(&1u16.to_be_bytes());
    let hmtx = [0x01, 0xF4, 0, 0];
    let loca = [0, 0, 0, 0];
    let glyf: [u8; 0] = [];
    let cmap = [0, 0, 0, 1, 0, 3, 0, 1, 0, 0, 0, 12, 0, 4, 0, 0, 0, 24, 0, 0, 0, 2, 0, 0, 0, 0, 0xFF, 0xFF, 0, 0, 0xFF, 0xFF, 0, 1, 0, 0];
    let tables: [(&[u8; 4], &[u8]); 7] =
        [(b"head", &head), (b"maxp", &maxp), (b"hhea", &hhea), (b"hmtx", &hmtx), (b"loca", &loca), (b"glyf", &glyf), (b"cmap", &cmap)];
    let mut out = vec![0; 12 + tables.len() * 16];
    out[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    out[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
    let mut offset = out.len();
    for (index, (tag, table)) in tables.iter().enumerate() {
        while !offset.is_multiple_of(4) {
            out.push(0);
            offset += 1;
        }
        let record = 12 + index * 16;
        out[record..record + 4].copy_from_slice(*tag);
        out[record + 8..record + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[record + 12..record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
        out.extend_from_slice(table);
        offset = out.len();
    }
    out
}

fn narrow_ascii_test_ttf() -> Vec<u8> {
    narrow_test_ttf(32)
}

fn narrow_test_ttf(first: u16) -> Vec<u8> {
    let mut head = vec![0; 54];
    head[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());

    let mut maxp = vec![0; 32];
    maxp[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&96u16.to_be_bytes());

    let mut hhea = vec![0; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    hhea[34..36].copy_from_slice(&96u16.to_be_bytes());
    let mut hmtx = Vec::with_capacity(96 * 4);
    for _ in 0..96 {
        hmtx.extend_from_slice(&313u16.to_be_bytes());
        hmtx.extend_from_slice(&0i16.to_be_bytes());
    }
    let loca = vec![0; 97 * 2];
    let glyf: [u8; 0] = [];

    let mut format4 = Vec::with_capacity(222);
    format4.extend_from_slice(&4u16.to_be_bytes());
    format4.extend_from_slice(&222u16.to_be_bytes());
    format4.extend_from_slice(&0u16.to_be_bytes());
    format4.extend_from_slice(&4u16.to_be_bytes());
    format4.extend_from_slice(&4u16.to_be_bytes());
    format4.extend_from_slice(&1u16.to_be_bytes());
    format4.extend_from_slice(&0u16.to_be_bytes());
    format4.extend_from_slice(&(first + 94).to_be_bytes());
    format4.extend_from_slice(&u16::MAX.to_be_bytes());
    format4.extend_from_slice(&0u16.to_be_bytes());
    format4.extend_from_slice(&first.to_be_bytes());
    format4.extend_from_slice(&u16::MAX.to_be_bytes());
    format4.extend_from_slice(&0u16.to_be_bytes());
    format4.extend_from_slice(&1u16.to_be_bytes());
    format4.extend_from_slice(&4u16.to_be_bytes());
    format4.extend_from_slice(&0u16.to_be_bytes());
    for glyph in 1u16..=95 {
        format4.extend_from_slice(&glyph.to_be_bytes());
    }
    let mut cmap = vec![0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 20, 0, 3, 0, 1, 0, 0, 0, 20];
    cmap.extend_from_slice(&format4);

    // An undecodable Macintosh record precedes the usable Unicode family name, as in
    // some craft-fonts faces. It must not turn this distinct family into `Unknown`.
    let family: Vec<u8> = "GridCraftTestNarrow".encode_utf16().flat_map(u16::to_be_bytes).collect();
    let mut name = Vec::new();
    for value in [0u16, 2, 30, 1, 1, 0, 1, 1, 0, 3, 1, 0x0409, 1, family.len() as u16, 1] {
        name.extend_from_slice(&value.to_be_bytes());
    }
    name.push(b'x');
    name.extend_from_slice(&family);
    let tables: [(&[u8; 4], &[u8]); 8] = [
        (b"head", &head),
        (b"maxp", &maxp),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"loca", &loca),
        (b"glyf", &glyf),
        (b"cmap", &cmap),
        (b"name", &name),
    ];
    let mut out = vec![0; 12 + tables.len() * 16];
    out[0..4].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    out[4..6].copy_from_slice(&(tables.len() as u16).to_be_bytes());
    let mut offset = out.len();
    for (index, (tag, table)) in tables.iter().enumerate() {
        while !offset.is_multiple_of(4) {
            out.push(0);
            offset += 1;
        }
        let record = 12 + index * 16;
        out[record..record + 4].copy_from_slice(*tag);
        out[record + 8..record + 12].copy_from_slice(&(offset as u32).to_be_bytes());
        out[record + 12..record + 16].copy_from_slice(&(table.len() as u32).to_be_bytes());
        out.extend_from_slice(table);
        offset = out.len();
    }
    out
}

pub(super) fn has_actual_text(pdf: &[u8], text: &str) -> bool {
    let mut encoded = String::from("/ActualText <FEFF");
    for unit in text.encode_utf16() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{unit:04X}");
    }
    encoded.push('>');
    contains(pdf, &encoded)
}

fn pdf_num(value: f32) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    if rounded == rounded.trunc() {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded:.3}").trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[test]
fn rotated_font_runs_advance_along_the_shared_baseline() {
    let advance = 80.0;
    for (degrees, expected) in [
        (0.0_f32, (80.0, 0.0)),
        (30.0, (80.0 * 3.0_f32.sqrt() / 2.0, 40.0)),
        (-30.0, (80.0 * 3.0_f32.sqrt() / 2.0, -40.0)),
        (90.0, (0.0, 80.0)),
        (-90.0, (0.0, -80.0)),
    ] {
        let origin = advance_text_baseline(12.0, 25.0, advance, degrees.to_radians());
        assert!((origin.0 - (12.0 + expected.0)).abs() < 0.001, "x at {degrees} degrees: {origin:?}");
        assert!((origin.1 - (25.0 + expected.1)).abs() < 0.001, "y at {degrees} degrees: {origin:?}");
    }
    assert_eq!(advance_text_baseline(12.0, 25.0, advance, f32::NAN), (92.0, 25.0));
}

#[test]
fn word_boundary_search_uses_logarithmic_complete_line_probes() {
    let mut probes = 0usize;
    let fitting = fitting_candidate_count(8192, 6000.0, |count| {
        probes = probes.saturating_add(1);
        Ok(count as f32)
    })
    .unwrap();
    assert_eq!(fitting, 6000);
    assert!(probes <= 26, "binary search should shape only logarithmically many full-line candidates: {probes}");
}

#[test]
fn unavailable_latin_styles_use_standard_fonts_instead_of_another_face() {
    let mut catalog = FontCatalog::empty();
    catalog.add_font(std::sync::Arc::<[u8]>::from(narrow_ascii_test_ttf()), 0).unwrap();
    let regular = shaped_runs(&catalog, "GridCraftTestNarrow", false, false, "Regular matrix", 11.0).unwrap();
    assert!(regular.iter().all(|run| matches!(run.font, TextRunFont::Embedded(_))));
    for (bold, italic) in [(true, false), (false, true), (true, true)] {
        let runs = shaped_runs(&catalog, "GridCraftTestNarrow", bold, italic, "Regular matrix", 11.0).unwrap();
        let expected = Font::styled(Font::family_for("GridCraftTestNarrow"), bold, italic);
        assert!(runs.iter().all(|run| matches!(run.font, TextRunFont::Standard(font) if font == expected)));
    }
}

#[test]
fn long_unspaced_prefix_search_only_measures_line_sized_text() {
    for count in [9000, 32_000] {
        let text = "ก".repeat(count);
        let mut remaining = text.as_str();
        let mut measured_clusters = 0usize;
        let mut largest_probe = 0usize;
        let mut consumed = 0usize;
        while !remaining.is_empty() {
            let (end, clusters) = fitting_text_prefix(remaining, 8192, 20.0, |prefix| {
                let count = gridcraft_pdf::text_clusters(prefix).count();
                measured_clusters += count;
                largest_probe = largest_probe.max(count);
                Ok(count as f32)
            })
            .unwrap();
            assert!(end > 0 && clusters <= 20);
            consumed += clusters;
            remaining = &remaining[end..];
        }
        assert_eq!(consumed, count);
        assert!(largest_probe <= 32, "a narrow line must not shape thousands of remaining clusters");
        assert!(measured_clusters < count * 16, "search work must grow with consumed text: {measured_clusters}");
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::disallowed_methods)] // Native-only timing regression; WASM has no clock.
fn long_unspaced_thai_wrapping_preserves_text_and_reports_time() {
    let Some((catalog, font)) = thai_catalog() else { return };
    let width = shaped_width(&catalog, font.family(), false, false, &"ก".repeat(20), 11.0).unwrap() + 0.01;
    for count in [9000, 32_000] {
        let text = "ก".repeat(count);
        let start = std::time::Instant::now();
        let lines = wrap_shaped(&text, &catalog, font.family(), false, false, 11.0, width).unwrap();
        eprintln!("wrapped {count} Thai characters into {} lines in {:?}", lines.len(), start.elapsed());
        assert_eq!(lines.concat(), text);
        assert!(lines.len() > 1 && lines.len() <= 2000);
        let mut session = s();
        session.execute("cell.set", json!({"cell": "A1", "input": text})).unwrap();
        session.execute("home.wrapText", json!({"range": "A1", "on": true})).unwrap();
        session.execute("home.columnWidth", json!({"cols": "A:A", "chars": 20})).unwrap();
        let start = std::time::Instant::now();
        let (pdf, _) = build_pdf(&session, &json!({}), "file.exportPdf").unwrap();
        eprintln!("exported one wrapped cell with {count} Thai characters in {:?}", start.elapsed());
        assert!(contains(&pdf, "/FontFile2") && contains(&pdf, "/ToUnicode"));
    }
}

#[test]
fn wrapped_whitespace_and_explicit_newlines_are_preserved() {
    let catalog = FontCatalog::empty();
    let text = "  Regular   matrix\ttext\u{2003}more\n\n10/22  -  10/28 \n";
    let lines = wrap_shaped(text, &catalog, "Calibri", false, false, 11.0, f32::MAX).unwrap();
    assert_eq!(lines, text.split('\n').collect::<Vec<_>>());
    let width = shaped_width(&catalog, "Calibri", false, false, "Regular m", 11.0).unwrap();
    assert_eq!(wrap_shaped("Regular   matrix", &catalog, "Calibri", false, false, 11.0, width).unwrap(), ["Regular", "matrix"]);
    assert_eq!(wrap_shaped("Regular\tmatrix", &catalog, "Calibri", false, false, 11.0, width).unwrap(), ["Regular", "matrix"]);
    let paragraphs = "x\n".repeat(2100);
    assert_eq!(wrap_shaped(&paragraphs, &catalog, "Calibri", false, false, 11.0, width).unwrap().len(), 2000);
    let words = "word ".repeat(2100);
    let width = shaped_width(&catalog, "Calibri", false, false, "word", 11.0).unwrap() + 0.1;
    let lines = wrap_shaped(&words, &catalog, "Calibri", false, false, 11.0, width).unwrap();
    assert_eq!(lines.len(), 2000, "word-boundary breaks obey the same output budget as cluster and paragraph breaks");
    assert!(lines.iter().all(|line| line == "word"));
}

#[test]
fn thai_and_mixed_text_wrap_without_splitting_clusters() {
    let Some((catalog, font)) = thai_catalog() else { return };
    let glyphs = font.shape("น้ำ").unwrap();
    assert!(glyphs.iter().all(|glyph| glyph.glyph_id != 0));
    assert!(glyphs.iter().any(|glyph| glyph.x_offset != 0 || glyph.y_offset != 0), "Thai marks must actually be shaped");
    let text = "น้ำกุ้งผู้ใช้เก้าอี้".repeat(8);
    let width = shaped_width(&catalog, "Calibri", false, false, "น้ำกุ้ง", 11.0).unwrap();
    let lines = wrap_shaped(&text, &catalog, "Calibri", false, false, 11.0, width).unwrap();
    assert!(lines.len() > 1);
    assert_eq!(lines.concat(), text);
    let boundaries: std::collections::HashSet<usize> = gridcraft_pdf::text_clusters(&text).map(|(start, cluster)| start + cluster.len()).collect();
    let mut end = 0usize;
    for line in &lines {
        end += line.len();
        assert!(boundaries.contains(&end), "a wrapped line must end at a grapheme boundary");
        assert!(shaped_width(&catalog, "Calibri", false, false, line, 11.0).unwrap() <= width + 0.01);
    }
    let width = ["Regular", "ภาษาไทย", "matrix"]
        .iter()
        .map(|word| shaped_width(&catalog, "Calibri", false, false, word, 11.0).unwrap())
        .fold(0.0f32, f32::max)
        + 0.1;
    assert_eq!(wrap_shaped("Regular ภาษาไทย matrix", &catalog, "Calibri", false, false, 11.0, width).unwrap(), ["Regular", "ภาษาไทย", "matrix"]);
}

#[test]
fn serialized_mixed_font_runs_share_rotated_baselines() {
    let Some((catalog, embedded)) = thai_catalog() else { return };
    let latin_before = "ABC ";
    let thai = "ภาษาไทย";
    let latin_after = " XYZ";
    let size = 12.0;
    let actual_runs = shaped_runs(&catalog, "Calibri", false, false, "ABC ภาษาไทย XYZ", size).expect("sample text shapes into font runs");
    assert!(actual_runs.len() > 1, "the Thai/Latin sample must exercise multiple actual font runs");
    assert!(actual_runs.iter().any(|run| run.text.contains(thai)), "the Thai sample must be present in the shaped runs");
    let runs = || {
        vec![
            TextRun {
                font: TextRunFont::Standard(Font::Helvetica),
                text: latin_before.to_string(),
                width: gridcraft_pdf::text_width(Font::Helvetica, size, latin_before),
            },
            TextRun {
                font: TextRunFont::Embedded(embedded.clone()),
                text: thai.to_string(),
                width: embedded.text_width(thai, size).expect("validated font width"),
            },
            TextRun {
                font: TextRunFont::Standard(Font::Helvetica),
                text: latin_after.to_string(),
                width: gridcraft_pdf::text_width(Font::Helvetica, size, latin_after),
            },
        ]
    };
    let total_width: f32 = runs().iter().map(|run| run.width).sum();
    let first_width = gridcraft_pdf::text_width(Font::Helvetica, size, latin_before);
    let thai_width = embedded.text_width(thai, size).expect("validated font width");

    for degrees in [0.0_f32, 30.0, -30.0, 90.0, -90.0] {
        let angle = degrees.to_radians();
        let second_origin = advance_text_baseline(20.0, 40.0, first_width, angle);
        let third_origin = advance_text_baseline(second_origin.0, second_origin.1, thai_width, angle);
        let first_thai_glyph = embedded.shape(thai).expect("validated font shapes Thai").first().cloned().expect("Thai text has a visible glyph");
        let expected_thai_glyph = if first_thai_glyph.x_offset == 0 && first_thai_glyph.y_offset == 0 { Some(second_origin) } else { None };
        let mut doc = PdfDoc::new();
        let page = doc.add_page(200.0, 300.0);
        let width = draw_text_runs(page, runs(), 20.0, 40.0, size, [0, 0, 0], angle).expect("runs render");
        assert!((width - total_width).abs() < 0.001, "returned width stays on baseline at {degrees} degrees");
        let pdf = doc.finish();
        assert_eq!(
            pdf.windows(b"/ActualText".len()).filter(|window| *window == b"/ActualText").count(),
            1,
            "mixed font runs share one semantic source span"
        );
        assert!(!contains(&pdf, "3 Tr"), "semantic Unicode is not appended as invisible text");
        let expected_third = format!(
            "{} {} {} {} {} {} Tm",
            pdf_num(angle.sin_cos().1),
            pdf_num(-angle.sin()),
            pdf_num(angle.sin()),
            pdf_num(angle.cos()),
            pdf_num(third_origin.0),
            pdf_num(300.0 - third_origin.1),
        );
        assert!(contains(&pdf, &expected_third), "serialized third run origin at {degrees} degrees: {expected_third}");
        let expected_first_thai_glyph = expected_thai_glyph.map(|glyph_origin| {
            format!(
                "{} {} {} {} {} {} Tm <0001> Tj",
                pdf_num(angle.cos()),
                pdf_num(-angle.sin()),
                pdf_num(angle.sin()),
                pdf_num(angle.cos()),
                pdf_num(glyph_origin.0),
                pdf_num(300.0 - glyph_origin.1),
            )
        });
        if let Some(expected_first_thai_glyph) = expected_first_thai_glyph {
            assert!(contains(&pdf, &expected_first_thai_glyph), "serialized embedded glyph origin at {degrees} degrees: {expected_first_thai_glyph}");
        }
    }
}

#[test]
fn unicode_drawing_paths_use_supplied_faces_and_preserve_standard_latin() {
    let mut catalog = FontCatalog::empty();
    // An original generated face covers Greek, without depending on installed fonts.
    catalog.add_font(std::sync::Arc::<[u8]>::from(narrow_test_ttf(0x0391)), 0).unwrap();
    assert!(catalog.face_for("Arial", false, false, "Ω").is_ok());
    for text in ["Ω", "Latin € ?"] {
        for path in ["chart", "shape", "header", "footer"] {
            let pdf = if path == "chart" {
                let mut doc = PdfDoc::new();
                let pg = doc.add_page(200.0, 200.0);
                let prim = gridcraft_chart::Prim::Text {
                    x: 20.0,
                    y: 20.0,
                    text: text.to_string(),
                    size: 10.0,
                    color: [0, 0, 0, 255],
                    bold: false,
                    align: gridcraft_chart::HAlign::Center,
                    valign: gridcraft_chart::VAlign::Top,
                    rotation: 0.0,
                };
                draw_prims(pg, &catalog, &[prim], Xf { ox: 0.0, oy: 0.0, k: 1.0 }).unwrap();
                doc.finish()
            } else {
                let mut session = s();
                session.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
                if path == "shape" {
                    session.execute("insert.shape", json!({"kind": "textBox", "at": "A1", "width": 180, "height": 40, "text": text})).unwrap();
                } else {
                    session.execute("pageLayout.headerFooter", json!({"part": path, "section": "center", "text": text})).unwrap();
                }
                let document = session.doc().unwrap();
                let opts = PrintOptions::from_params(&json!({}), "file.exportPdf").unwrap();
                let lays = layouts(&document.wb, &opts);
                render_pdf_with_catalog(&document.wb, &lays, "Book1", "", &catalog).unwrap().0
            };
            if text == "Ω" {
                assert!(contains(&pdf, "/FontFile2"), "{path} must embed the supplied Unicode face");
                assert!(has_actual_text(&pdf, text), "{path} must retain the Unicode source text");
            } else {
                assert!(!contains(&pdf, "/FontFile2"), "{path} must keep the Standard-14 WinAnsi path");
                assert!(contains(&pdf, "/BaseFont /Helvetica"));
            }
        }
    }
}

#[test]
fn api_and_print_pipeline_embed_and_shape_thai_text() {
    let mut s = s();
    s.execute(
        "range.setValues",
        json!({"range": "A1", "values": [
            ["ภาษาไทย"], ["น้ำ"], ["กุ้ง"], ["ผู้ใช้"], ["เก้าอี้"], ["ป ป ญ ฐ"],
            ["ยอดรวม 1,234.50 บาท"], ["Thai mixed English with ภาษาไทย\nข้อความหลายบรรทัด และข้อความยาวสำหรับทดสอบการตัดบรรทัด"]
        ]}),
    )
    .unwrap();
    s.execute("home.columnWidth", json!({"cols": "A:A", "chars": 30})).unwrap();
    s.execute("home.fontName", json!({"range": "A1:A4", "name": "Arial"})).unwrap();
    s.execute("home.bold", json!({"range": "A2"})).unwrap();
    s.execute("home.italic", json!({"range": "A3"})).unwrap();
    s.execute("home.bold", json!({"range": "A4"})).unwrap();
    s.execute("home.italic", json!({"range": "A4"})).unwrap();
    s.execute("home.alignCenter", json!({"range": "A5"})).unwrap();
    s.execute("home.alignRight", json!({"range": "A6"})).unwrap();
    s.execute("home.wrapText", json!({"range": "A7", "on": true})).unwrap();
    s.execute("home.autofitRowHeight", json!({"rows": "7:7"})).unwrap();
    s.execute("home.wrapText", json!({"range": "A8", "on": true})).unwrap();
    s.execute("home.autofitRowHeight", json!({"rows": "8:8"})).unwrap();
    s.execute("cell.set", json!({"cell": "A70", "input": "ข้อความข้ามหน้า ภาษาไทย"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"printArea": "A1:A70", "fitToPage": false, "scale": 100})).unwrap();
    let has_thai_font = thai_catalog().is_some();
    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert!(pages >= 2, "Thai content beyond the first page must paginate");
    assert!(contains(&pdf, "/Subtype /Type0"));
    if has_thai_font {
        assert!(contains(&pdf, "/FontFile2"), "the Thai face is embedded when it is available");
        assert!(contains(&pdf, "/ToUnicode"));
    }
    assert!(contains(&pdf, "/ActualText <FEFF"), "the PDF retains semantic source text for marked Unicode text");
    let print = s.execute("file.print", json!({})).unwrap();
    let bytes = crate::io::base64_decode(print["base64"].as_str().unwrap()).unwrap();
    assert!(contains(&bytes, "/Subtype /Type0"));
    assert!(print["pages"].as_u64().unwrap_or(0) >= 2);
    assert_eq!(s.execute("file.printPreview", json!({})).unwrap()["pages"], print["pages"]);
    let d = s.doc().unwrap();
    let opts = PrintOptions::from_params(&json!({}), "file.exportPdf").unwrap();
    let lays = layouts(&d.wb, &opts);
    let (fallback, _) = render_pdf_with_catalog(&d.wb, &lays, "Book1", "", &FontCatalog::empty()).unwrap();
    assert!(!contains(&fallback, "/FontFile2"));
    assert!(!contains(&fallback, "/ToUnicode"), "the predefined CID fallback needs no embedded-font CMap");
    assert!(has_actual_text(&fallback, "ภาษาไทย"), "export without a Thai font preserves the logical text and succeeds");
    if let Some(path) = std::env::var_os("GRIDCRAFT_PDF_SAMPLE") {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, &pdf).unwrap();
    }
}

#[test]
fn engine_pdf_reuses_font_cids_across_repeated_glyphs_in_multiple_cells() {
    if thai_catalog().is_none() {
        return;
    }
    let mut s = s();
    let repeated = "ก".repeat(13_108);
    s.execute("range.setValues", json!({"range": "A1:A3", "values": [[repeated.clone()], [repeated.clone()], [repeated.clone()]]})).unwrap();
    s.execute("home.fontName", json!({"range": "A1:A3", "name": "Arial"})).unwrap();
    s.execute("range.setValues", json!({"range": "A4:A5", "values": [[repeated.clone()], [repeated.clone()]]})).unwrap();
    s.execute("home.fontName", json!({"range": "A4:A5", "name": "Arial"})).unwrap();
    s.execute("pageLayout.pageSetup", json!({"printArea": "A1:A5", "fitToPage": false, "scale": 100})).unwrap();

    let (pdf, pages) = pdf_of(&mut s, json!({}));
    assert_eq!(pages, 1, "the repeated-occurrence regression should fit on one exported page");
    assert!(contains(&pdf, "/CIDToGIDMap"));
    assert!(contains(&pdf, "/ToUnicode"));
    assert_eq!(pdf.windows(b"<0001> Tj".len()).filter(|window| *window == b"<0001> Tj").count(), 5 * 13_108);
    assert!(contains(&pdf, "/ActualText <FEFF"), "the engine output retains the logical Unicode text");
    if let Some(path) = std::env::var_os("GRIDCRAFT_CID_PDF_SAMPLE") {
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, &pdf).unwrap();
    }
}

#[test]
fn embedded_faces_are_subset_to_the_glyphs_a_document_draws() {
    let mut thai = s();
    thai.execute("cell.set", json!({"cell": "A1", "input": "ภาษาไทย"})).unwrap();
    let (pdf, _) = pdf_of(&mut thai, json!({}));
    eprintln!("one Thai cell: {} bytes", pdf.len());
    if thai_catalog().is_some() {
        assert!(contains(&pdf, "/FontFile2"));
        assert!(pdf.len() < 40_000, "one Thai cell embeds a subset, not the whole face: {} bytes", pdf.len());
        assert!(has_actual_text(&pdf, "ภาษาไทย"));
    }

    let mut arrow = s();
    arrow.execute("cell.set", json!({"cell": "A1", "input": "1"})).unwrap();
    arrow.execute("pageLayout.pageSetup", json!({"header": "&CTotal → 5"})).unwrap();
    let (pdf, _) = pdf_of(&mut arrow, json!({}));
    eprintln!("header \"Total → 5\": {} bytes", pdf.len());
    // Without an installed face covering the arrow, the header uses the predefined CID font.
    if contains(&pdf, "/FontFile2") {
        assert!(pdf.len() < 100_000, "one arrow embeds a subset, not a full system face: {} bytes", pdf.len());
    }
}

#[test]
fn supplied_pdf_font_size_errors_identify_the_limit() {
    const MIB: usize = 1024 * 1024;
    assert_eq!(checked_pdf_font_total(0, 32 * MIB), Ok(32 * MIB));
    assert_eq!(checked_pdf_font_total(32 * MIB, 32 * MIB), Ok(64 * MIB));
    assert_eq!(checked_pdf_font_total(0, 32 * MIB + 1), Err("a supplied PDF font exceeds the 32 MiB decoded limit"));
    assert_eq!(checked_pdf_font_total(64 * MIB, 1), Err("supplied PDF font data exceeds the 64 MiB total limit"));
    assert_eq!(checked_pdf_font_total(usize::MAX, 1), Err("supplied font data size overflow"));

    let session = s();
    let mut params = json!({"pdfFonts": [{"base64": "A".repeat(48 * MIB + 1)}]});
    let error = build_pdf(&session, &params, "file.exportPdf").unwrap_err().to_string();
    assert!(error.contains("48 MiB encoded"), "the pre-decode limit names the encoded size: {error}");
    if let Json::String(encoded) = &mut params["pdfFonts"][0]["base64"] {
        encoded.pop();
    }
    let error = build_pdf(&session, &params, "file.exportPdf").unwrap_err().to_string();
    assert!(error.contains("32 MiB decoded"), "an encoded string at the limit reaches the separate decoded-size check: {error}");
}

#[test]
fn corrupt_supplied_pdf_font_returns_a_command_error() {
    let mut s = s();
    let error = s.execute("file.exportPdf", json!({"pdfFonts": [{"base64": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}]})).unwrap_err().to_string();
    assert!(error.contains("unsupported font file type"), "unexpected font error: {error}");
}

#[test]
fn supplied_pdf_font_face_index_errors_are_command_errors_and_zero_exports() {
    let encoded = crate::io::base64_encode(&minimal_test_ttf());
    let mut s = s();
    s.execute("cell.set", json!({"cell": "A1", "input": "ordinary PDF export"})).unwrap();

    let valid = s.execute("file.exportPdf", json!({"pdfFonts": [{"base64": encoded, "faceIndex": 0}]})).expect("standalone face zero should export");
    let bytes = crate::io::base64_decode(valid["base64"].as_str().expect("successful export returns PDF base64")).expect("PDF base64 decodes");
    assert!(bytes.starts_with(b"%PDF-"));
    assert_eq!(valid["pages"].as_u64(), Some(1));

    let encoded = crate::io::base64_encode(&minimal_test_ttf());
    for index in [1u32, 99, u32::MAX] {
        let error = s
            .execute("file.exportPdf", json!({"pdfFonts": [{"base64": encoded, "faceIndex": index}]}))
            .expect_err("an invalid supplied face index must stop export instead of using another face")
            .to_string();
        assert!(error.contains(&index.to_string()), "command error should identify the requested face index: {error}");
        assert!(error.contains("not a collection"), "command error should describe why the index is invalid: {error}");
    }

    let encoded = crate::io::base64_encode(&minimal_test_ttf());
    for invalid in [json!(-1), json!(1.5), json!("1"), json!(u64::from(u32::MAX) + 1)] {
        let error = s
            .execute("file.exportPdf", json!({"pdfFonts": [{"base64": encoded, "faceIndex": invalid}]}))
            .expect_err("faceIndex preserves the non-negative 32-bit integer API contract")
            .to_string();
        assert!(error.contains("non-negative 32-bit integer"), "unexpected parameter validation error: {error}");
    }
}

#[test]
fn ascii_numeric_cells_use_selected_font_metrics_for_measurement_and_drawing() {
    let font_bytes = narrow_ascii_test_ttf();
    let mut catalog = FontCatalog::empty();
    catalog.add_font(std::sync::Arc::<[u8]>::from(font_bytes.clone()), 0).expect("generated test font is valid");
    let face = catalog.face_for("GridCraftTestNarrow", false, false, "5,000.00").expect("generated face covers formatted number");
    assert_eq!(face.family(), "GridCraftTestNarrow", "the first decodable family name identifies the supplied face");
    let expected_width = face.text_width("5,000.00", 22.0).expect("generated face shapes ASCII");
    assert!(expected_width < 100.0, "generated face's number width should remain finite and plausible: {expected_width}pt");

    let mut session = s();
    session.execute("range.setValues", json!({"range": "A1:B1", "values": [[5000, 123456789]]})).unwrap();
    session.execute("home.fontName", json!({"range": "A1:B1", "name": "GridCraftTestNarrow"})).unwrap();
    session.execute("home.fontSize", json!({"range": "A1:B1", "size": 22})).unwrap();
    session.execute("home.numberFormat", json!({"range": "A1", "code": "0,000.00"})).unwrap();
    session.execute("home.numberFormat", json!({"range": "B1", "code": "0.00"})).unwrap();
    session.execute("home.columnWidth", json!({"cols": "A:A", "chars": 20})).unwrap();
    session.execute("home.columnWidth", json!({"cols": "B:B", "chars": 3})).unwrap();
    let encoded = crate::io::base64_encode(&font_bytes);
    let response = session
        .execute("file.exportPdf", json!({"pdfFonts": [{"base64": encoded, "faceIndex": 0}]}))
        .expect("ASCII-only cell can export with a supplied face");
    let pdf = crate::io::base64_decode(response["base64"].as_str().expect("export returns bytes")).expect("base64 decodes");
    assert!(contains(&pdf, "/Subtype /Type0"), "the selected non-Standard-14 face must be embedded for ASCII-only values");
    assert!(contains(&pdf, "/ActualText <FEFF0035002C003000300030002E00300030>"), "PDF extraction retains the formatted source text");
    assert!(contains(&pdf, "FEFF0023"), "a number wider than its narrow column still overflows as hashes");
    assert!(!contains(&pdf, "FEFF003100320033003400350036003700380039002E00300030"), "the narrow cell does not claim to fit its long value");
    let pdf_width = (expected_width / 8.0 * 1000.0 / 22.0).round() as i32;
    assert!(contains(&pdf, &format!("/W [1 [{pdf_width}")), "PDF advance widths match the selected face measurement");
}

#[test]
fn unavailable_pdf_faces_use_standard_aliases_and_degrade_missing_unicode_without_rewriting_it() {
    let catalog = FontCatalog::empty();
    let ascii = shaped_runs(&catalog, "Calibri", false, false, "café 123", 12.0).expect("unavailable Latin family uses Standard-14");
    assert_eq!(ascii.len(), 1);
    assert!(matches!(&ascii[0].font, TextRunFont::Standard(Font::Helvetica)));
    assert_eq!(ascii[0].text, "café 123");

    let missing = shaped_runs(&catalog, "Calibri", false, false, "AไทยB", 12.0).expect("missing Thai fonts degrade to the Standard-14 CID fallback");
    assert_eq!(missing.iter().map(|run| run.text.as_str()).collect::<String>(), "AไทยB");
    assert!(missing.iter().all(|run| matches!(&run.font, TextRunFont::Standard(_))));
    assert!(shaped_width(&catalog, "Calibri", false, false, "AไทยB", 12.0).is_ok());
}

#[test]
fn shaped_wrapping_prefers_word_boundaries_and_chunks_long_paragraphs() {
    let catalog = FontCatalog::empty();
    let width = gridcraft_pdf::text_width(Font::Helvetica, 12.0, "The quick") + 8.0;
    let wrapped = wrap_shaped("The quick brown fox", &catalog, "Calibri", false, false, 12.0, width).expect("ASCII wraps without installed fonts");
    assert_eq!(wrapped.join(" "), "The quick brown fox");
    assert_eq!(wrapped.first().map(String::as_str), Some("The quick"));

    let long = "x".repeat(8193);
    let wrapped =
        wrap_shaped(&long, &catalog, "Calibri", false, false, 12.0, f32::MAX).expect("long paragraphs are split across bounded layout lines");
    assert_eq!(wrapped.concat(), long);
    assert_eq!(wrapped.len(), 2);
    assert!(wrapped.iter().all(|line| gridcraft_pdf::text_clusters(line).count() <= 8192));
}
