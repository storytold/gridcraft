//! Fixed lookup tables: the legacy indexed colour palette, paper sizes, functions that need the
//! `_xlfn.` prefix in files, and unit conversions.

/// Id to write for a format code when it is a locale-independent built-in of
/// [`gridcraft_numfmt::builtin_format`] (currency formats 5–8 are written as custom formats
/// because their meaning depends on the reader's locale). The code must match exactly.
pub fn builtin_id(code: &str) -> Option<u32> {
    gridcraft_numfmt::builtin_id(code).filter(|id| !(5..=8).contains(id) && gridcraft_numfmt::builtin_format(*id) == Some(code))
}

/// The 64-entry legacy palette (`indexed` colours), RRGGBB.
pub const INDEXED: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, // 0-7
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, // 8-15
    0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, // 16-23
    0x9999FF, 0x993366, 0xFFFFCC, 0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, // 24-31
    0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080, 0x0000FF, // 32-39
    0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, // 40-47
    0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600, 0x666699, 0x969696, // 48-55
    0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333, // 56-63
];

/// Paper sizes (`pageSetup paperSize`) we name.
const PAPERS: [(u32, &str); 14] = [
    (1, "Letter"),
    (3, "Tabloid"),
    (4, "Ledger"),
    (5, "Legal"),
    (6, "Statement"),
    (7, "Executive"),
    (8, "A3"),
    (9, "A4"),
    (11, "A5"),
    (12, "B4"),
    (13, "B5"),
    (20, "Envelope #10"),
    (27, "Envelope DL"),
    (34, "Envelope B5"),
];

pub fn paper_name(id: u32) -> Option<&'static str> {
    PAPERS.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
}

pub fn paper_id(name: &str) -> Option<u32> {
    PAPERS.iter().find(|(_, n)| n.eq_ignore_ascii_case(name.trim())).map(|(i, _)| *i)
}

// ---------------------------------------------------------------- units

/// Column width as stored in a file (characters including padding) → model width (px).
pub fn col_width_to_px(w: f64) -> f32 {
    if !w.is_finite() || w <= 0.0 {
        return 0.0;
    }
    let w = w.min(255.0);
    (((256.0 * w + (128.0f64 / 7.0).trunc()) / 256.0) * 7.0).trunc() as f32
}

/// Model width (px) → stored column width.
pub fn px_to_col_width(px: f32) -> f64 {
    let px = (px.max(0.0) as f64).round().min(1790.0);
    let back = |w: f64| col_width_to_px(w) as f64;
    let chars = if px > 12.0 { ((px - 5.0) / 7.0 * 100.0 + 0.5).trunc() / 100.0 } else { px / 12.0 };
    let w = ((chars * 7.0 + 5.0) / 7.0 * 256.0).trunc() / 256.0;
    if back(w) == px {
        return w;
    }
    // Smallest stored width that reads back as `px`.
    let s = (px * 256.0 / 7.0 - 18.0).ceil().max(0.0);
    s / 256.0
}

/// Row height in typographic points → model px.
pub fn pt_to_px(pt: f64) -> f32 {
    if !pt.is_finite() || pt <= 0.0 { 0.0 } else { (pt.min(409.5) * 4.0 / 3.0) as f32 }
}

pub fn px_to_pt(px: f32) -> f64 {
    ((px.max(0.0) as f64) * 0.75 * 100.0).round() / 100.0
}

/// EMU per px at 96 dpi.
pub const EMU_PER_PX: f64 = 9525.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widths() {
        assert_eq!(col_width_to_px(9.140625), 64.0);
        assert_eq!(px_to_col_width(64.0), 9.140625);
        for px in 0..400 {
            assert_eq!(col_width_to_px(px_to_col_width(px as f32)), px as f32, "px {px}");
        }
        assert_eq!(pt_to_px(15.0), 20.0);
        assert_eq!(px_to_pt(20.0), 15.0);
    }

    #[test]
    fn functions() {
        assert_eq!(gridcraft_numfmt::builtin_format(14), Some("m/d/yyyy"));
        assert_eq!(builtin_id("0.00%"), Some(10));
        assert_eq!(builtin_id("M/D/YYYY"), None, "only exact codes get a built-in id");
        assert_eq!(builtin_id(gridcraft_numfmt::builtin_format(5).unwrap()), None);
        assert_eq!(paper_name(9), Some("A4"));
        assert_eq!(paper_id("a4"), Some(9));
    }
}
