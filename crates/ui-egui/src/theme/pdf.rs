use super::font_setup::candidates;

/// Primary regular/bold and alternate regular/bold system faces for PDF text.
/// Prefer a face covering both Greek and Cyrillic; otherwise keep a face for each script.
/// Pass to `gridcraft_engine::pdf::set_unicode_font_loader` (the desktop app and the CLI do).
pub fn pdf_unicode_fonts() -> [Option<Vec<u8>>; 4] {
    let [[regular, regular_alt], [bold, bold_alt]] = ["cell", "cell-bold"].map(|role| {
        select_fallbacks(
            candidates(role)
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
        let Some(face) = gridcraft_engine::pdf::UnicodeFont::parse(bytes.clone()) else { continue };
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
pub(super) fn read_pdf_font(path: &str) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let limit = gridcraft_engine::pdf::MAX_FONT_BYTES as u64;
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

/// Whether a PDF fallback font has real Greek or Cyrillic letters, not just Latin: a capital and
/// a lowercase letter of each script. Greek: `Ω` (U+03A9) and `ω` (U+03C9), the last of each
/// case. Cyrillic: `Ж` (U+0416, a capital with no Latin look-alike, unlike `А`) and `џ`
/// (U+045F, the end of the U+0400–U+045F block), so a font with only the 33 Russian letters (the
/// JIS Gothic faces) doesn't qualify and Serbian, Macedonian, Ukrainian and Belarusian text gets
/// real glyphs too.
pub(super) fn pdf_font_covers_greek_or_cyrillic(f: &gridcraft_engine::pdf::UnicodeFont) -> bool {
    (f.has('Ω') && f.has('ω')) || (f.has('Ж') && f.has('џ'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::pdf::{UnicodeFont, test_support::tiny_ttf};

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
}
