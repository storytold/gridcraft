//! Paper sizes by name: the names `PrintSettings::paper` holds, with their sizes. File formats
//! (paper ids or page sizes) and print layout all read this one table.

/// Paper names and sizes in points, width then height, as the paper is named: Ledger is
/// Tabloid turned on its side. Each size has one name, so a size reads back as the name it was
/// saved under. `B4`/`B5`/`B6` are the **JIS** sizes Japanese Excel uses (257×364, 182×257 and
/// 128×182 mm); the ISO B series is `B4 (ISO)`/`B5 (ISO)`/`B6 (ISO)`. The Japanese postcards and
/// envelopes are the JIS ones too.
pub const PAPERS: [(&str, f32, f32); 24] = [
    ("Letter", 612.0, 792.0),
    ("Legal", 612.0, 1008.0),
    ("A4", 595.28, 841.89),
    ("A3", 841.89, 1190.55),
    ("A5", 419.53, 595.28),
    ("A6", 297.64, 419.53),
    ("Tabloid", 792.0, 1224.0),
    ("Ledger", 1224.0, 792.0),
    ("Executive", 522.0, 756.0),
    ("Statement", 396.0, 612.0),
    ("B4", 728.50, 1031.81),
    ("B5", 515.91, 728.50),
    ("B6", 362.83, 515.91),
    ("B4 (ISO)", 708.66, 1000.63),
    ("B5 (ISO)", 498.90, 708.66),
    ("B6 (ISO)", 354.33, 498.90),
    ("Envelope #10", 297.0, 684.0),
    ("Envelope DL", 311.81, 623.62),
    ("Japanese Postcard", 283.46, 419.53),
    ("Japanese Double Postcard", 419.53, 566.93),
    ("Japanese Envelope #3", 340.16, 666.14),
    // 長形4号 is 90×205 mm; 90×225 mm is 長形40号, a different envelope, so it stays out.
    ("Japanese Envelope #4", 255.12, 581.10),
    ("Japanese Envelope Kaku #2", 680.31, 941.10),
    ("Japanese Envelope Kaku #3", 612.28, 785.20),
];

/// Other spellings of the names above, in [`key`] form.
const ALIASES: [(&str, &str); 13] = [
    // The B5 envelope is ISO B5 itself (176×250 mm): one size, one name.
    ("envelopeb5", "B5 (ISO)"),
    ("usletter", "Letter"),
    ("uslegal", "Legal"),
    ("11x17", "Tabloid"),
    ("b4jis", "B4"),
    ("b5jis", "B5"),
    ("b6jis", "B6"),
    ("postcard", "Japanese Postcard"),
    ("doublepostcard", "Japanese Double Postcard"),
    ("chou#3", "Japanese Envelope #3"),
    ("chou#4", "Japanese Envelope #4"),
    ("kaku#2", "Japanese Envelope Kaku #2"),
    ("kaku#3", "Japanese Envelope Kaku #3"),
];

/// A name as compared: spaces, hyphens, underscores and parentheses don't matter, nor does case
/// (`B5 (JIS)`, `b5-jis` and `b5jis` are one name).
fn key(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace([' ', '-', '_', '(', ')'], "")
}

/// The paper `name` stands for, under its table name.
pub fn canonical(name: &str) -> Option<&'static str> {
    // No paper name is long; a huge one (from a file or a command) is not worth folding.
    if name.len() > 64 {
        return None;
    }
    let k = key(name);
    PAPERS.iter().map(|(n, _, _)| *n).find(|n| key(n) == k).or_else(|| ALIASES.iter().find(|(a, _)| *a == k).map(|(_, n)| *n))
}

/// The size of paper `name` in points (width, height).
pub fn size_pt(name: &str) -> Option<(f32, f32)> {
    let n = canonical(name)?;
    PAPERS.iter().find(|(p, _, _)| *p == n).map(|(_, w, h)| (*w, *h))
}

/// The paper of a page `width` × `height` points, within 2 mm on each side: the nearest one as it
/// stands, else the nearest one turned. Some papers are less than 4 mm apart (Letter and Japanese
/// Envelope Kaku #3), so the nearest wins, not the first in the table.
///
/// The name is the paper as the page stands: a 17×11 in page is `Ledger`. A caller that also
/// records a landscape orientation should pass the size upright (narrow side first), since
/// print layout turns the named paper once more for landscape.
pub fn for_size(width: f32, height: f32) -> Option<&'static str> {
    const TOLERANCE: f32 = 5.67; // 2 mm
    let nearest = |w: f32, h: f32| {
        PAPERS
            .iter()
            .filter_map(|(n, pw, ph)| {
                let (dw, dh) = ((pw - w).abs(), (ph - h).abs());
                (dw < TOLERANCE && dh < TOLERANCE).then_some((dw + dh, *n))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, n)| n)
    };
    nearest(width, height).or_else(|| nearest(height, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_sizes() {
        assert_eq!(size_pt("b5-jis"), Some((515.91, 728.50)));
        assert_eq!(size_pt("US Letter"), Some((612.0, 792.0)));
        assert_eq!(size_pt("nonsense"), None);
        assert_eq!(for_size(1224.0, 792.0), Some("Ledger"));
        assert_eq!(for_size(792.0, 1224.0), Some("Tabloid"));
        assert_eq!(for_size(842.0, 595.0), Some("A4"));
        assert_eq!(canonical("ledger"), Some("Ledger"));
        assert_eq!(canonical(""), None);
        assert_eq!(canonical(&"A4".repeat(1000)), None);
    }

    #[test]
    fn every_size_reads_back_as_its_name() {
        for (name, w, h) in PAPERS {
            assert_eq!(for_size(w, h), Some(name), "{name}");
        }
        assert_eq!(canonical("Envelope B5"), Some("B5 (ISO)"));
    }

    /// Letter (612×792) and Japanese Envelope Kaku #3 (612.28×785.2) are within 2 mm of some
    /// of the same sizes: the nearer paper wins, whichever comes first in the table.
    #[test]
    fn for_size_picks_the_nearest_paper() {
        assert_eq!(for_size(612.2, 788.0), Some("Japanese Envelope Kaku #3"));
        assert_eq!(for_size(612.0, 790.0), Some("Letter"));
        assert_eq!(for_size(788.0, 612.2), Some("Japanese Envelope Kaku #3"));
        // A wide page that is a paper as it stands is not read as another paper turned.
        assert_eq!(for_size(1224.0, 792.0), Some("Ledger"));
        assert_eq!(for_size(1000.0, 10.0), None);
        assert_eq!(for_size(f32::NAN, 792.0), None);
        assert_eq!(for_size(f32::INFINITY, f32::INFINITY), None);
    }
}
