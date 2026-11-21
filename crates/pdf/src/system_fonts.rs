//! Toolkit-independent native PDF fallback loading and shared cell-font candidates.

fn home_fonts(file: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{home}/Library/Fonts/{file}")
}

fn in_dirs(dirs: &[&str], files: &[&str]) -> Vec<String> {
    dirs.iter().flat_map(|d| files.iter().map(move |f| format!("{d}/{f}"))).collect()
}

/// Installed cell-font candidates in preference order, shared by the desktop and PDF fallback.
/// Only system font folders are probed; application bundles are never searched.
pub fn system_cell_font_candidates(bold: bool, italic: bool) -> Vec<(String, u32)> {
    let mac_sup = "/System/Library/Fonts/Supplemental";
    let win = std::env::var("WINDIR").map(|w| format!("{w}\\Fonts")).unwrap_or_else(|_| "C:\\Windows\\Fonts".into());
    let lin = ["/usr/share/fonts/truetype", "/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/TTF", "/usr/share/fonts/chromeos"];
    let mut v = Vec::new();
    let mut add = |s: String, i: u32| v.push((s, i));

    let (carlito, calibri_win, calibri_mac, arial_mac, arial_win, lib, dejavu) = match (bold, italic) {
        (false, false) => {
            ("Carlito-Regular.ttf", "calibri.ttf", "Calibri.ttf", "Arial.ttf", "arial.ttf", "LiberationSans-Regular.ttf", "DejaVuSans.ttf")
        }
        (true, false) => (
            "Carlito-Bold.ttf",
            "calibrib.ttf",
            "Calibri Bold.ttf",
            "Arial Bold.ttf",
            "arialbd.ttf",
            "LiberationSans-Bold.ttf",
            "DejaVuSans-Bold.ttf",
        ),
        (false, true) => (
            "Carlito-Italic.ttf",
            "calibrii.ttf",
            "Calibri Italic.ttf",
            "Arial Italic.ttf",
            "ariali.ttf",
            "LiberationSans-Italic.ttf",
            "DejaVuSans-Oblique.ttf",
        ),
        _ => (
            "Carlito-BoldItalic.ttf",
            "calibriz.ttf",
            "Calibri Bold Italic.ttf",
            "Arial Bold Italic.ttf",
            "arialbi.ttf",
            "LiberationSans-BoldItalic.ttf",
            "DejaVuSans-BoldOblique.ttf",
        ),
    };
    // Calibri if the user has it installed system-wide (never from an Office bundle).
    add(format!("/Library/Fonts/{calibri_mac}"), 0);
    add(home_fonts(calibri_mac), 0);
    add(format!("{win}\\{calibri_win}"), 0);
    for p in in_dirs(&lin, &[&format!("crosextra/{carlito}"), &format!("carlito/{carlito}")]) {
        add(p, 0);
    }
    // Fedora keeps each family in its own package folder, as for the UI fonts (#54).
    add(format!("/usr/share/fonts/google-carlito-fonts/{carlito}"), 0);
    add(home_fonts(carlito), 0);
    add(format!("{mac_sup}/{arial_mac}"), 0);
    add(format!("{win}\\{arial_win}"), 0);
    for p in in_dirs(&lin, &[&format!("liberation/{lib}"), &format!("dejavu/{dejavu}")]) {
        add(p, 0);
    }
    add(format!("/usr/share/fonts/liberation-sans-fonts/{lib}"), 0);
    add(format!("/usr/share/fonts/dejavu-sans-fonts/{dejavu}"), 0);
    v
}

/// Primary regular/bold and alternate regular/bold system faces for PDF text.
/// Prefer a face covering both Greek and Cyrillic; otherwise keep a face for each script.
/// Pass to `crate::set_unicode_font_loader` (the desktop app and the CLI do).
pub fn pdf_unicode_fonts() -> [Option<Vec<u8>>; 4] {
    let [[regular, regular_alt], [bold, bold_alt]] = [false, true].map(|bold| {
        select_fallbacks(
            system_cell_font_candidates(bold, false)
                .into_iter()
                .filter(|(path, index)| *index == 0 && path.to_ascii_lowercase().ends_with(".ttf"))
                .filter_map(|(path, _)| read_pdf_font(&path)),
        )
    });
    [regular, bold, regular_alt, bold_alt]
}

fn select_fallbacks(fonts: impl IntoIterator<Item = Vec<u8>>) -> [Option<Vec<u8>>; 2] {
    let (mut greek, mut cyrillic) = (None, None);
    for bytes in fonts {
        let Some(face) = crate::UnicodeFont::parse(bytes.clone()) else { continue };
        if !pdf_font_covers_greek_or_cyrillic(&face) {
            continue;
        }
        let has_greek = face.has('Ω') && face.has('ω');
        let has_cyrillic = face.has('Ж') && face.has('џ');
        if has_greek && has_cyrillic {
            return [Some(bytes), None];
        }
        if has_greek && greek.is_none() {
            greek = Some(bytes);
        } else if has_cyrillic && cyrillic.is_none() {
            cyrillic = Some(bytes);
        }
    }
    [greek, cyrillic]
}

/// Bound the native fallback read before parsing or cloning bytes. A system-font symlink may
/// point at a device or a growing file, so metadata alone never limits the allocation.
#[cfg(not(target_arch = "wasm32"))]
fn read_pdf_font(path: &str) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let limit = crate::MAX_FONT_BYTES as u64;
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

#[cfg(target_arch = "wasm32")]
fn read_pdf_font(_path: &str) -> Option<Vec<u8>> {
    None
}

/// Whether a PDF fallback font has real Greek or Cyrillic letters, not just Latin: a capital and
/// a lowercase letter of each script. Greek: `Ω` (U+03A9) and `ω` (U+03C9), the last of each
/// case. Cyrillic: `Ж` (U+0416, a capital with no Latin look-alike, unlike `А`) and `џ`
/// (U+045F, the end of the U+0400–U+045F block), so a font with only the 33 Russian letters (the
/// JIS Gothic faces) doesn't qualify and Serbian, Macedonian, Ukrainian and Belarusian text gets
/// real glyphs too.
fn pdf_font_covers_greek_or_cyrillic(f: &crate::UnicodeFont) -> bool {
    (f.has('Ω') && f.has('ω')) || (f.has('Ж') && f.has('џ'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{UnicodeFont, test_support::tiny_ttf};

    #[test]
    fn a_combined_face_wins_over_earlier_single_script_faces() {
        let greek = tiny_ttf(&['Ω', 'ω'], 400, 0);
        let cyrillic = tiny_ttf(&['Ж', 'џ'], 500, 0);
        let both = tiny_ttf(&['Ω', 'ω', 'Ж', 'џ'], 600, 0);
        let selected = select_fallbacks([greek, cyrillic, both.clone()]);
        assert_eq!(selected, [Some(both), None]);
    }

    #[test]
    fn separate_script_faces_survive_either_candidate_order() {
        let greek = tiny_ttf(&['Ω', 'ω'], 400, 0);
        let cyrillic = tiny_ttf(&['Ж', 'џ'], 500, 0);
        for candidates in [[greek.clone(), cyrillic.clone()], [cyrillic.clone(), greek.clone()]] {
            let selected = select_fallbacks(candidates);
            assert_eq!(selected, [Some(greek.clone()), Some(cyrillic.clone())]);
            let [g, c] = selected.map(|b| UnicodeFont::parse(b.unwrap()).unwrap());
            assert!(g.has('Ω') && c.has('Ж'));
        }
        assert_eq!(select_fallbacks([vec![0; 16], tiny_ttf(&['A'], 500, 0), tiny_ttf(&['Ω', 'ω'], 500, 2)]), [None, None]);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pdf_font_reads_reject_oversized_and_non_regular_inputs() {
        let dir = std::env::temp_dir().join(format!("gridcraft-pdf-font-reader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("font.ttf");
        std::fs::write(&path, b"small input").unwrap();
        assert_eq!(read_pdf_font(path.to_str().unwrap()), Some(b"small input".to_vec()));
        assert!(read_pdf_font(dir.to_str().unwrap()).is_none());
        std::fs::File::create(&path).unwrap().set_len(crate::MAX_FONT_BYTES as u64 + 1).unwrap();
        assert!(read_pdf_font(path.to_str().unwrap()).is_none());
        #[cfg(unix)]
        assert!(read_pdf_font("/dev/zero").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pdf_unicode_fonts_set_cyrillic_in_real_glyphs() {
        // Real-font check, like `fallback_covers_its_script`: skips on a machine with no cell font
        // that has Cyrillic, fails if one is installed but printing would still lose the text.

        let samples = ["Добар дан, Ђорђе! Љубав, њива, џеп, ћуп", "Съешь же ещё этих мягких булок"];
        let has_cyrillic = |bold: bool| {
            system_cell_font_candidates(bold, false)
                .iter()
                .filter_map(|(p, _)| read_pdf_font(p))
                .filter_map(UnicodeFont::parse)
                .any(|f| f.has('Ж') && f.has('џ'))
        };
        let selected = pdf_unicode_fonts();
        for (i, bold) in [false, true].into_iter().enumerate() {
            if !has_cyrillic(bold) {
                continue; // no Cyrillic face for this role here; nothing to assert
            }
            let font = [i, i + 2]
                .into_iter()
                .filter_map(|slot| selected.get(slot)?.clone())
                .filter_map(UnicodeFont::parse)
                .find(|f| f.has('Ж') && f.has('џ'));
            let font = font.unwrap_or_else(|| panic!("a bold={bold} candidate has Cyrillic but none was picked for PDF"));
            assert!(pdf_font_covers_greek_or_cyrillic(&font));
            for text in samples {
                for c in text.chars().filter(|c| c.is_alphabetic()) {
                    assert!(font.has(c), "the bold={bold} PDF font lacks {c:?} (U+{:04X}) from {text:?}", c as u32);
                }
            }
        }
    }
}
