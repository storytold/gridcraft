use super::*;
use std::error::Error;
use std::io;

mod reference;
use reference::{OldPage, interpret};

#[test]
fn incremental_measurements_keep_whole_run_font_selection() {
    let face = |chars: &[char], width| Arc::new(UnicodeFont::parse(tiny_ttf(chars, width, 0)).unwrap());
    let primary = face(&[' ', '?', 'A', 'Ω'], 900);
    let alternate = face(&[' ', '?', 'A', 'Ω', 'Ж'], 300);
    let fallback = TextFonts::fallback([Some(primary.clone()), None, Some(alternate.clone()), None]);
    assert_eq!(fallback.measure_text(Font::Helvetica, "A").then(fallback.measure_text(Font::Helvetica, "Ω")).width(10.0), 18.0);
    assert_eq!(fallback.measure_text(Font::Helvetica, "AΩ").then(fallback.measure_text(Font::Helvetica, "Ж")).width(10.0), 9.0);
    let mut named = TextFonts::default();
    named.add_family("Body", primary.clone(), None, None, None).unwrap();
    let handle = named.font("Body", false, false);
    let sets = [
        TextFonts::default(),
        TextFonts::custom(primary.clone(), Some(alternate.clone()), None, None),
        named,
        fallback,
        TextFonts::fallback([Some(primary), None, Some(face(&['Ж'], 600)), Some(alternate)]),
    ];
    for fonts in sets {
        for font in [Font::Helvetica, Font::HelveticaBold, Font::TimesItalic, Font::Symbol, handle, Font::Embedded(999)] {
            for text in ["", "A A", "AΩ", "AΩЖ", "Ω Ж Ω", "A\tΩ\rЖ", "ΩΨЖ😀", "?A\0Ω"] {
                for split in text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())) {
                    let (left, right) = text.split_at(split);
                    let combined = fonts.measure_text(font, left).then(fonts.measure_text(font, right));
                    assert_eq!(combined.width(11.0), fonts.text_width(font, 11.0, text), "{font:?} {text:?} split {split}");
                }
            }
        }
    }
}

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

#[test]
fn symbol_encoding_survives_registered_unicode_fallbacks() {
    let face = Arc::new(UnicodeFont::parse(tiny_ttf(&['∑', 'α', '≤', '∞'], 900, 0)).unwrap());
    let fonts = TextFonts::fallback([Some(face), None, None, None]);
    assert_eq!(fonts.text_width(Font::Symbol, 10.0, "∑"), 7.13);
    assert_eq!(fonts.text_width(Font::Helvetica, 10.0, "α"), 9.0);
    let mut doc = PdfDoc::with_text_fonts(fonts);
    let p = doc.add_page(200.0, 200.0);
    p.text(10.0, 50.0, 12.0, Font::Symbol, [0, 0, 0], "∑α≤∞");
    p.text_rotated(Point::new(80.0, 80.0), 12.0, Font::Symbol, [0, 0, 0], "∑α≤∞", 0.5);
    p.text(10.0, 100.0, 12.0, Font::Helvetica, [0, 0, 0], "α");
    let pdf = doc.finish();
    let text = String::from_utf8_lossy(&pdf);
    assert_eq!(text.matches("/F13 12 Tf").count(), 2, "math keeps the Symbol resource");
    assert_eq!(text.matches("(\\345a\\243\\245) Tj").count(), 2);
    assert!(text.contains("/U1 12 Tf"), "ordinary Greek still uses its embedded fallback");
    assert_eq!(text.matches("/FontFile2").count(), 1);
    assert!(!text.contains("UniGB-UTF16-H"));
    check_xref(&pdf).unwrap();
}

#[test]
fn explicit_custom_faces_override_symbol_with_matching_metrics() {
    let bytes = tiny_ttf(&['∑', 'α', '≤', '∞', 'x'], 640, 0);
    let face = Arc::new(UnicodeFont::parse(bytes.clone()).unwrap());
    let fonts = TextFonts::custom(face, None, None, None);
    assert_eq!(fonts.text_width(Font::Symbol, 10.0, "∑"), 6.4);
    assert_eq!(fonts.missing_glyph(Font::Symbol, "∑α≤∞x"), None);
    let mut doc = PdfDoc::with_text_fonts(fonts);
    doc.add_page(200.0, 200.0).text(10.0, 50.0, 12.0, Font::Symbol, [0, 0, 0], "∑α≤∞x");
    let pdf = doc.finish();
    let text = String::from_utf8_lossy(&pdf);
    assert!(find(&pdf, &bytes).is_some());
    assert!(text.contains("/U1 12 Tf"));
    assert!(!text.contains("/BaseFont /Symbol"));
    assert!(text.contains("<0001> <2211>"));
    assert!(text.contains("<0002> <03B1>"));
    check_xref(&pdf).unwrap();
}

#[test]
fn embedded_text_preserves_cached_fill_through_shapes_text_and_restore() {
    let face = Arc::new(UnicodeFont::parse(tiny_ttf(&['A', 'Ω'], 600, 0)).unwrap());
    let mut named = TextFonts::default();
    named.add_family("Body", face.clone(), None, None, None).unwrap();
    let handle = named.font("Body", false, false);
    for (fonts, font) in [
        (named, handle),
        (TextFonts::custom(face.clone(), None, None, None), Font::Helvetica),
        (TextFonts::fallback([Some(face), None, None, None]), Font::Helvetica),
    ] {
        let mut doc = PdfDoc::with_text_fonts(fonts);
        let page = doc.add_page(200.0, 200.0);
        let red = [255, 0, 0];
        page.fill_rect(0.0, 0.0, 5.0, 5.0, red);
        page.save();
        page.text_rotated(Point::new(10.0, 20.0), 10.0, font, [0, 0, 255], "Ω", 0.5);
        page.fill_rect(0.0, 0.0, 5.0, 5.0, red);
        page.restore();
        page.fill_rect(0.0, 0.0, 5.0, 5.0, red);
        page.text(10.0, 40.0, 10.0, font, [0, 255, 0], "Ω");
        page.text(10.0, 60.0, 10.0, Font::Helvetica, red, "A");
        page.fill_rect(0.0, 0.0, 5.0, 5.0, red);
        let state = interpret(&page.content);
        let fills: Vec<_> = state.paints.iter().map(|paint| paint.fill.as_str()).collect();
        assert_eq!(fills, ["1 0 0", "0 0 1", "1 0 0", "1 0 0", "0 1 0", "1 0 0", "1 0 0"]);
        assert!(state.balanced);
        check_xref(&doc.finish()).unwrap();
    }
}

#[test]
fn streamed_named_fonts_share_images_and_keep_links_and_unicode_maps() -> Result<(), Box<dyn Error>> {
    let body_bytes = tiny_ttf(&['A', 'B', 'Ω'], 400, 0);
    let heading_bytes = tiny_ttf(&['A', 'B', 'Ω'], 800, 0);
    let mut fonts = TextFonts::default();
    fonts.add_family("Body", Arc::new(UnicodeFont::parse(body_bytes.clone()).unwrap()), None, None, None).unwrap();
    fonts.add_family("Heading", Arc::new(UnicodeFont::parse(heading_bytes.clone()).unwrap()), None, None, None).unwrap();
    let body = fonts.font("Body", false, false);
    let heading = fonts.font("Heading", false, false);
    let mut doc = PdfDoc::with_text_fonts(fonts);
    for i in 0..2 {
        let page = doc.add_page(200.0, 200.0);
        let font = if i == 0 { body } else { heading };
        assert_eq!(page.text_width(font, 10.0, "ABΩ"), if i == 0 { 12.0 } else { 24.0 });
        page.text(10.0, 20.0, 10.0, font, [0, 0, 0], "ABΩ");
        for x in [10.0, 30.0] {
            page.image_rgba(Rect::new(x, 30.0, 10.0, 10.0), &[10, 20, 30, 128], [1, 1]);
        }
        page.link_page(Rect::new(10.0, 10.0, 20.0, 10.0), 1 - i, Point::new(10.0, 20.0));
        page.link_uri(10.0, 50.0, 20.0, 10.0, "https://example.com/fonts");
        page.save(); // finish must balance this without losing font or image resources.
    }
    let pdf = doc.finish();
    check_xref(&pdf)?;
    let (objects, pages) = parse_pages(&pdf);
    assert_eq!(pages.len(), 2);
    let image_ids: Vec<_> = pages.iter().flat_map(|page| page.xobjects.iter().map(|(_, id)| *id)).collect();
    assert_eq!(image_ids.len(), 2);
    assert_eq!(image_ids[0], image_ids[1]);
    assert_eq!(decode_image(&objects, image_ids[0]), (1, 1, vec![10, 20, 30], Some(vec![128])));
    assert_eq!(objects.iter().filter(|obj| obj.dict.contains("/Subtype /Image")).count(), 2);
    let files: Vec<_> = objects.iter().filter_map(|obj| ref_of(&obj.dict, "/FontFile2")).collect();
    assert_eq!(files.len(), 2);
    assert_eq!(objects[files[0]].stream.as_deref(), Some(body_bytes.as_slice()));
    assert_eq!(objects[files[1]].stream.as_deref(), Some(heading_bytes.as_slice()));
    let cmaps: Vec<_> = objects.iter().filter_map(|obj| ref_of(&obj.dict, "/ToUnicode")).collect();
    assert_eq!(cmaps.len(), 2);
    for id in cmaps {
        assert!(find(objects[id].stream.as_ref().unwrap(), b"<0003> <03A9>").is_some());
    }
    assert!(pages[0].content.contains("/U5 10 Tf"));
    assert!(pages[1].content.contains("/U9 10 Tf"));
    assert!(pages.iter().all(|page| interpret(&page.content).balanced));
    assert_eq!(objects.iter().filter(|obj| obj.dict.contains("/Subtype /Link")).count(), 4);
    let page_ids: Vec<_> = objects.iter().enumerate().filter(|(_, obj)| obj.dict.contains("/Type /Page ")).map(|(id, _)| id).collect();
    for id in page_ids {
        assert!(objects.iter().any(|obj| obj.dict.contains(&format!("/Dest [{id} 0 R /XYZ 10 180 null]"))));
    }
    Ok(())
}
