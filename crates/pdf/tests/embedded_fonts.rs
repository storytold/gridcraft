//! Embedded TrueType fonts for text WinAnsiEncoding lacks (Cyrillic for Ukrainian users).
//! The fonts are system fonts: the tests that need one skip when none is installed.

use std::sync::Mutex;

use gridcraft_pdf::{EmbeddedFont, Font, FontError, PdfDoc, clear_fonts, has_font, install_font, text_width};

/// The installed fonts are process-wide: tests that change them take turns.
static FONTS: Mutex<()> = Mutex::new(());

/// A TrueType font with Cyrillic from the usual system locations, if there is one.
fn system_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/liberation2/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/local/share/fonts/dejavu/DejaVuSans.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/Library/Fonts/Arial.ttf",
    ];
    CANDIDATES.iter().find_map(|p| std::fs::read(p).ok())
}

fn find(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

const UKRAINIAN: &str = "Привіт, Ґанок і їжак: 1 250,00 ₴";

#[test]
fn cyrillic_is_embedded_as_a_searchable_subset() {
    let _turn = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(data) = system_font() else {
        eprintln!("no system TrueType font with Cyrillic; skipped");
        return;
    };
    let full = data.len();
    let font = EmbeddedFont::from_bytes(data, 0).unwrap();
    assert!(font.glyph('Ї').is_some() && font.glyph('ґ').is_some());
    let cid_width = text_width(Font::Helvetica, 10.0, "Привіт");
    install_font(Font::Helvetica, font);
    assert!(has_font(Font::Helvetica) && has_font(Font::TimesBold), "other faces fall back to the sans regular font");
    // Widths come from the font now, not the full-width CID default.
    let width = text_width(Font::Helvetica, 10.0, "Привіт");
    assert!(width > 20.0 && width < cid_width, "{width} vs {cid_width}");

    let mut doc = PdfDoc::new();
    let page = doc.add_page(595.0, 842.0);
    page.text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], UKRAINIAN);
    page.text(72.0, 100.0, 12.0, Font::HelveticaBold, [0, 0, 0], "Підсумок");
    let pdf = doc.finish();
    clear_fonts();

    for needle in [&b"/CIDFontType2"[..], b"/FontFile2", b"/Identity-H", b"/ToUnicode", b"/CIDToGIDMap"] {
        assert!(find(&pdf, needle), "{}", String::from_utf8_lossy(needle));
    }
    // Latin text keeps the standard font; nothing went to the non-embedded CID font.
    assert!(find(&pdf, b"(, ) Tj") || find(&pdf, b"(, "), "Latin punctuation stays WinAnsi");
    assert!(!find(&pdf, b"STSong"));
    // A subset, not the whole font file.
    assert!(pdf.len() < full / 2, "{} bytes for a {full}-byte font", pdf.len());
    // The ToUnicode map lets viewers copy the text: `П` is U+041F.
    assert!(find(&pdf, b"<041F>"));
}

#[test]
fn without_a_font_cyrillic_uses_the_cid_font() {
    let _turn = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    clear_fonts();
    assert!(!has_font(Font::Helvetica));
    let mut doc = PdfDoc::new();
    doc.add_page(595.0, 842.0).text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], UKRAINIAN);
    let pdf = doc.finish();
    assert!(find(&pdf, b"STSong") && !find(&pdf, b"/FontFile2"));
}

#[test]
fn characters_the_font_lacks_still_reach_the_cid_font() {
    let _turn = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    let Some(data) = system_font() else { return };
    let font = EmbeddedFont::from_bytes(data, 0).unwrap();
    if font.glyph('中').is_some() {
        return;
    }
    install_font(Font::Helvetica, font);
    let mut doc = PdfDoc::new();
    doc.add_page(595.0, 842.0).text(72.0, 72.0, 12.0, Font::Helvetica, [0, 0, 0], "Київ 中文");
    let pdf = doc.finish();
    clear_fonts();
    assert!(find(&pdf, b"/FontFile2") && find(&pdf, b"STSong"));
}

#[test]
fn hostile_font_files_are_errors_not_panics() {
    assert_eq!(EmbeddedFont::from_bytes(Vec::new(), 0).unwrap_err(), FontError::Unreadable);
    assert_eq!(EmbeddedFont::from_bytes(b"not a font at all".to_vec(), 0).unwrap_err(), FontError::Unreadable);
    assert_eq!(EmbeddedFont::from_bytes(vec![0; 65 * 1024 * 1024], 0).unwrap_err(), FontError::TooLarge);
    let Some(data) = system_font() else { return };
    assert!(EmbeddedFont::from_bytes(data.clone(), 7).is_err(), "a plain .ttf has no font 7");
    // Every truncation and some corruptions either load or fail; a loaded one still embeds.
    let step = (data.len() / 97).max(1);
    for cut in (0..data.len()).step_by(step) {
        let mut broken = data.clone();
        broken.truncate(cut);
        exercise(broken);
        let mut flipped = data.clone();
        if let Some(b) = flipped.get_mut(cut) {
            *b ^= 0xFF;
        }
        exercise(flipped);
    }
}

/// Loads `data` and, if it loads, draws and saves Cyrillic with it in a document of its own.
fn exercise(data: Vec<u8>) {
    let Ok(font) = EmbeddedFont::from_bytes(data, 0) else { return };
    let _turn = FONTS.lock().unwrap_or_else(|e| e.into_inner());
    install_font(Font::Courier, font);
    let mut doc = PdfDoc::new();
    doc.add_page(595.0, 842.0).text(72.0, 72.0, 12.0, Font::Courier, [0, 0, 0], UKRAINIAN);
    let _ = doc.finish();
    clear_fonts();
}

/// `data` with its OS/2 `fsType` replaced (table checksums are not checked by the reader).
fn with_fs_type(mut data: Vec<u8>, fs_type: u16) -> Option<Vec<u8>> {
    let tables = usize::from(u16::from_be_bytes([*data.get(4)?, *data.get(5)?]));
    for i in 0..tables {
        let at = 12 + i * 16;
        if data.get(at..at + 4)? == b"OS/2" {
            let offset = u32::from_be_bytes(data.get(at + 8..at + 12)?.try_into().ok()?) as usize;
            data.get_mut(offset + 8..offset + 10)?.copy_from_slice(&fs_type.to_be_bytes());
            return Some(data);
        }
    }
    None
}

#[test]
fn licence_bits_that_forbid_subsetting_or_outlines_refuse_the_font() {
    let Some(data) = system_font() else {
        eprintln!("no system TrueType font with Cyrillic; skipped");
        return;
    };
    for (fs_type, allowed) in [(0x0000, true), (0x0004, true), (0x0008, true), (0x0002, false), (0x0100, false), (0x0200, false), (0x0108, false)] {
        let patched = with_fs_type(data.clone(), fs_type).expect("the font has an OS/2 table");
        let result = EmbeddedFont::from_bytes(patched, 0);
        if allowed {
            assert!(result.is_ok(), "fsType {fs_type:#06x} may be embedded");
        } else {
            assert!(matches!(result, Err(FontError::EmbeddingRestricted)), "fsType {fs_type:#06x} is refused");
        }
    }
}
