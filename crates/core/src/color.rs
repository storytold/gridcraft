//! Shared legacy indexed colours used by XLSX and number-format rendering.

/// The 64-entry legacy indexed colour palette, stored as `RRGGBB`.
pub const INDEXED: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, 0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00,
    0xFF00FF, 0x00FFFF, 0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, 0x9999FF, 0x993366, 0xFFFFCC, 0xCCFFFF,
    0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, 0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080, 0x0000FF, 0x00CCFF, 0xCCFFFF,
    0xCCFFCC, 0xFFFF99, 0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, 0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600, 0x666699, 0x969696,
    0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333,
];

/// Resolve an Excel number-format color index (`Color1` through `Color56`).
pub fn indexed_color(index: u8) -> Option<[u8; 3]> {
    if !(1..=56).contains(&index) {
        return None;
    }
    let slot = index.checked_add(7)?;
    let rgb = *INDEXED.get(slot as usize)?;
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
}

/// The `ColorIndex` (1 to 56) of the palette colour nearest to `rgb` by distance in RGB, the
/// lowest index on a tie: how Excel snaps a colour given as RGB to the palette (a gridline
/// colour of `123456` becomes 49, `003366`; `FF0100` becomes 3, `FF0000`).
pub fn nearest_indexed(rgb: [u8; 3]) -> u8 {
    let mut best = (u32::MAX, 1);
    for index in 1..=56 {
        let Some(c) = indexed_color(index) else { continue };
        let d: u32 = c.iter().zip(rgb).map(|(&a, b)| u32::from(a.abs_diff(b)).pow(2)).sum();
        if d < best.0 {
            best = (d, index);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_snaps_to_the_nearest_palette_colour() {
        // Measured in Excel: `GridlineColor` 123456 reads back as ColorIndex 49, FF0100 as 3.
        assert_eq!(nearest_indexed([0x12, 0x34, 0x56]), 49);
        assert_eq!(indexed_color(49), Some([0x00, 0x33, 0x66]));
        assert_eq!(nearest_indexed([0xFF, 0x01, 0x00]), 3);
        // Every palette colour finds itself, or the first index holding the same colour.
        for index in 1..=56 {
            let rgb = indexed_color(index).unwrap();
            let found = nearest_indexed(rgb);
            assert!(found <= index && indexed_color(found) == Some(rgb), "{index} → {found}");
        }
        assert_eq!(nearest_indexed([0, 0, 0xFF]), 5, "0000FF is both 5 and 32: the lower wins");
        assert_eq!(indexed_color(0), None);
        assert_eq!(indexed_color(57), None);
    }
}
