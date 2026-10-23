//! Fonts from the optional craft-fonts build input (`CRAFT_FONTS_DIR`; see `build.rs`). Only the
//! Indian-script faces are embedded. The table is empty in a build without craft-fonts, and every
//! user of it works when it is.

/// One embedded craft-fonts face.
pub struct CraftFont {
    pub family: &'static str,
    pub style: &'static str,
    /// ISO 15924 scripts the font is for, e.g. `"Deva"`.
    pub scripts: &'static [&'static str],
    pub bytes: &'static [u8],
}

include!(concat!(env!("OUT_DIR"), "/craft_fonts.rs"));

/// Whether an embedded face covers `script` (ISO 15924).
pub fn covers(script: &str) -> bool {
    CRAFT_FONTS.iter().any(|f| f.scripts.contains(&script))
}
