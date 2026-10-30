//! craft-fonts build policy shared by `build.rs` and the UI crate.
//!
//! GridCraft stays a clean-room application: the shared repository may keep Adobe, Source Han and
//! Noto CJK faces for other consumers, but GridCraft never embeds them. The permitted faces are
//! matched by exact family and style, and their manifest script tags are asserted instead of
//! trusting the free-form parser in `docs/integration.md`.
//!
//! The file is compiled twice: by `build.rs` for the full parser/comparison/embedding path (via
//! `#[path]`), and by this crate, where `theme` only needs the barred-family filter and the web
//! allowlist. The rest of the policy is exercised by this file's own tests, so it is not dead
//! code to remove.
#![allow(dead_code)]

/// Families GridCraft must never embed: the repository asset policy excludes Adobe/Source faces
/// and the Noto CJK collection. There is deliberately no broad `Noto Sans` prefix: the permitted
/// `Noto Sans Symbols` and `Noto Sans Symbols 2` faces start with it.
pub const BARRED_PREFIXES: &[&str] = &["Adobe", "Source Han", "Source Sans", "Source Serif", "Noto Sans CJK", "Noto Serif CJK"];

/// The faces GridCraft's production font set relies on, with their exact manifest script tags.
/// Every release/acceptance build that sets `CRAFT_FONTS_REQUIRED=1` must contain all of them.
pub const REQUIRED_FACES: &[(&str, &str, &[&str])] = &[
    ("BIZ UDPGothic", "Regular", &["Jpan", "Latn"]),
    ("BIZ UDPGothic", "Bold", &["Jpan", "Latn"]),
    ("NanumGothic", "Regular", &["Hang", "Kore"]),
    ("Noto Sans Symbols", "Regular", &["Zsym"]),
    ("Noto Sans Symbols 2", "Regular", &["Zsym"]),
];

/// The native-only Simplified Chinese face from craft-fonts' separate
/// `fonts/simplified-chinese-manifest.txt` (the same opt-in DeckCraft uses). It is OFL and not
/// barred by the asset policy, but it never enters a web bundle; older native checkouts may omit
/// the manifest, so only `CRAFT_FONTS_REQUIRED=1` builds demand it.
pub const CHINESE_FACES: &[(&str, &str, &[&str])] = &[("LXGW WenKai Lite", "Regular", &["Hani", "Hans"])];

/// The explicit web allowlist. Browser builds embed only these faces; the Noto CJK collection and
/// the document-only faces stay off the wasm bundle. `Noto Sans Symbols` supplies `⌃`, and
/// `Noto Sans Symbols 2` supplies `✕`, `◀` and `✓`; `📋` still comes from egui's bundled emoji.
pub const WEB_ALLOWED: &[(&str, &str)] = &[
    ("BIZ UDPGothic", "Regular"),
    ("BIZ UDPGothic", "Bold"),
    ("NanumGothic", "Regular"),
    ("Noto Sans Symbols", "Regular"),
    ("Noto Sans Symbols 2", "Regular"),
];

/// One validated `fonts/manifest.txt` row. Only the fields GridCraft consumes are kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestRow {
    pub family: String,
    pub style: String,
    pub file: String,
    pub scripts: Vec<String>,
    pub sha256: String,
}

/// Whether the `CRAFT_FONTS_REQUIRED` value enables required mode. Only exactly `1` does:
/// `0`, an empty value and other words are not accepted as an implicit yes.
pub fn required_mode(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Whether `family` is barred by GridCraft's asset policy.
pub fn barred(family: &str) -> bool {
    BARRED_PREFIXES.iter().any(|prefix| family.starts_with(prefix))
}

/// Whether a web build may embed this exact family and style.
pub fn web_allowed(family: &str, style: &str) -> bool {
    WEB_ALLOWED.iter().any(|(f, s)| *f == family && *s == style)
}

/// The exact script tags a required face must declare.
pub fn required_scripts(family: &str, style: &str) -> Option<&'static [&'static str]> {
    REQUIRED_FACES.iter().chain(CHINESE_FACES.iter()).find(|(f, s, _)| *f == family && *s == style).map(|(_, _, scripts)| *scripts)
}

/// Parse the craft-fonts eight-field manifest. Comments and blank lines are skipped; every other
/// line must be complete and contain a valid SHA-256. A malformed row is an error, never a
/// silently ignored font.
pub fn parse_manifest(text: &str) -> Result<Vec<ManifestRow>, String> {
    let mut rows = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, licence, licence_file, sha256, source] = fields.as_slice() else {
            return Err(format!("manifest line {}: expected 8 fields separated by \" | \": {line}", number + 1));
        };
        if [family, style, file, scripts, licence, licence_file, sha256, source].iter().any(|f| f.is_empty()) {
            return Err(format!("manifest line {}: an empty field: {line}", number + 1));
        }
        if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("manifest line {}: sha256 must be 64 hex digits: {sha256}", number + 1));
        }
        let scripts: Vec<String> = scripts.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
        if scripts.is_empty() || scripts.iter().any(|tag| tag.len() != 4 || !tag.chars().all(|c| c.is_ascii_alphabetic())) {
            return Err(format!("manifest line {}: script tags must be ISO 15924 four-letter tags: {line}", number + 1));
        }
        rows.push(ManifestRow {
            family: (*family).to_string(),
            style: (*style).to_string(),
            file: (*file).to_string(),
            scripts,
            sha256: (*sha256).to_string(),
        });
    }
    Ok(rows)
}

/// Check the script tags of every face GridCraft knows. The craft-fonts parser accepts arbitrary
/// strings, so a face relabelled without the scripts GridCraft relies on is rejected here.
pub fn check_declared_scripts(rows: &[ManifestRow]) -> Result<(), String> {
    for row in rows {
        if let Some(expected) = required_scripts(&row.family, &row.style) {
            if row.scripts.iter().map(String::as_str).ne(expected.iter().copied()) {
                return Err(format!(
                    "{} {} must declare scripts {} but the manifest says {}",
                    row.family,
                    row.style,
                    expected.join(","),
                    row.scripts.join(",")
                ));
            }
        } else if REQUIRED_FACES.iter().chain(CHINESE_FACES.iter()).any(|(family, _, _)| *family == row.family) {
            return Err(format!("{} has an unexpected style {:?}; GridCraft only permits the exact required faces", row.family, row.style));
        }
    }
    Ok(())
}

/// Every required face must be present. Used only for `CRAFT_FONTS_REQUIRED=1` builds.
pub fn check_required(rows: &[ManifestRow]) -> Result<(), String> {
    let present = |family: &str, style: &str| rows.iter().any(|row| row.family == family && row.style == style);
    let missing: Vec<String> = REQUIRED_FACES.iter().filter(|(f, s, _)| !present(f, s)).map(|(f, s, _)| format!("{f} {s}")).collect();
    if missing.is_empty() { Ok(()) } else { Err(format!("the craft-fonts manifest is missing required faces: {}", missing.join(", "))) }
}

/// Every native-only Chinese face must be present. Used only for `CRAFT_FONTS_REQUIRED=1` native
/// builds; ordinary builds may use an older craft-fonts checkout without the opt-in manifest.
pub fn check_chinese_required(rows: &[ManifestRow]) -> Result<(), String> {
    let present = |family: &str, style: &str| rows.iter().any(|row| row.family == family && row.style == style);
    let missing: Vec<String> = CHINESE_FACES.iter().filter(|(f, s, _)| !present(f, s)).map(|(f, s, _)| format!("{f} {s}")).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("the simplified-Chinese craft-fonts manifest is missing required faces: {}", missing.join(", ")))
    }
}

/// The rows this build should embed, in manifest order. Barred families are always excluded; a
/// web build additionally keeps only [`WEB_ALLOWED`].
pub fn embeddable(rows: &[ManifestRow], wasm: bool) -> Vec<&ManifestRow> {
    rows.iter().filter(|row| !barred(&row.family) && (!wasm || web_allowed(&row.family, &row.style))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "NanumGothic | Regular | fonts/nanum-gothic/NanumGothic-Regular.ttf | Hang,Kore | OFL-1.1 | fonts/nanum-gothic/OFL.txt | 76f45ef4a6bcff344c837c95a7dcc26e017e38b5846d5ae0cdcb5b86be2e2d31 | https://example.invalid/nanum";

    #[test]
    /// Asserts the exact web allowlist and that every entry is unbarred, required and not a CJK collection face.
    fn the_web_allowlist_is_exact_and_never_barred() {
        assert_eq!(
            WEB_ALLOWED,
            &[
                ("BIZ UDPGothic", "Regular"),
                ("BIZ UDPGothic", "Bold"),
                ("NanumGothic", "Regular"),
                ("Noto Sans Symbols", "Regular"),
                ("Noto Sans Symbols 2", "Regular"),
            ]
        );
        for (family, _) in WEB_ALLOWED {
            assert!(!barred(family), "{family} must not be barred");
            assert!(REQUIRED_FACES.iter().any(|(f, _, _)| f == family), "{family} must be a required face");
        }
        assert!(!WEB_ALLOWED.iter().any(|(family, _)| family.contains("CJK")), "the web allowlist must contain no CJK collection face");
    }

    #[test]
    /// Adobe/Source and Noto CJK families are barred, while the permitted Noto Symbols, BIZ and Nanum faces are not.
    fn the_barred_prefixes_keep_the_permitted_noto_symbol_faces() {
        for family in ["Adobe Source Sans", "Source Han Sans", "Source Sans 3", "Source Serif 4", "Noto Sans CJK SC", "Noto Serif CJK JP"] {
            assert!(barred(family), "{family} must be barred");
        }
        for family in ["Noto Sans Symbols", "Noto Sans Symbols 2", "BIZ UDPGothic", "NanumGothic"] {
            assert!(!barred(family), "{family} must be permitted");
        }
    }

    #[test]
    /// Only the exact string `"1"` enables required mode; absent and all other values leave it off.
    fn only_the_value_one_enables_required_mode() {
        assert!(!required_mode(None));
        for value in ["", "0", "00", "01", "false", "true", "yes"] {
            assert!(!required_mode(Some(value)), "{value:?} must not enable required mode");
        }
        assert!(required_mode(Some("1")));
    }

    #[test]
    /// A complete eight-field row yields the expected family, style and script list.
    fn a_complete_manifest_row_parses() {
        let rows = parse_manifest(GOOD).unwrap_or_default();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.first().map(|r| (r.family.as_str(), r.style.as_str())), Some(("NanumGothic", "Regular")));
        assert_eq!(rows.first().map(|r| r.scripts.clone()), Some(vec!["Hang".to_string(), "Kore".to_string()]));
    }

    #[test]
    /// Missing fields, an empty file field, a bad hash and a non-ISO script tag are all rejected.
    fn malformed_rows_are_errors() {
        for bad in [
            "NanumGothic | Regular | fonts/x.ttf | Hang,Kore",
            "NanumGothic | Regular |  | Hang,Kore | OFL-1.1 | fonts/OFL.txt | 76f45ef4a6bcff344c837c95a7dcc26e017e38b5846d5ae0cdcb5b86be2e2d31 | https://example.invalid",
            "NanumGothic | Regular | fonts/x.ttf | Hang,Kore | OFL-1.1 | fonts/OFL.txt | notahash | https://example.invalid",
            "NanumGothic | Regular | fonts/x.ttf | Korean | OFL-1.1 | fonts/OFL.txt | 76f45ef4a6bcff344c837c95a7dcc26e017e38b5846d5ae0cdcb5b86be2e2d31 | https://example.invalid",
        ] {
            assert!(parse_manifest(bad).is_err(), "must reject {bad:?}");
        }
    }

    #[test]
    /// One row cannot satisfy every required face; script order and exact tags are checked, and a required family in an unlisted style is rejected.
    fn required_faces_and_script_tags_are_checked() {
        let rows = parse_manifest(GOOD).unwrap_or_default();
        assert!(check_required(&rows).is_err(), "one row cannot satisfy every required face");
        assert!(check_declared_scripts(&rows).is_ok());
        let relabelled = parse_manifest(&GOOD.replace("Hang,Kore", "Kore,Hang")).unwrap_or_default();
        assert!(check_declared_scripts(&relabelled).is_err(), "script order and exact tags are asserted");
        let unexpected_style = parse_manifest(&GOOD.replace("Regular", "Bold")).unwrap_or_default();
        assert!(check_declared_scripts(&unexpected_style).is_err(), "a required family in an unlisted style is rejected");
    }

    #[test]
    /// LXGW is permitted but native-only; its script order and style are checked, and a required native build must include it.
    fn the_native_chinese_face_is_permitted_and_checked() {
        let chinese = "LXGW WenKai Lite | Regular | fonts/lxgw-wenkai-lite/LXGWWenKaiLite-Regular.ttf | Hani,Hans | OFL-1.1 | fonts/lxgw-wenkai-lite/OFL.txt | f799a538a3355877e597a0ae04050e897932c9ae7689eb9227c2953c5dd549ea | https://example.invalid/lxgw";
        let rows = parse_manifest(chinese).unwrap_or_default();
        assert!(check_declared_scripts(&rows).is_ok());
        assert!(check_chinese_required(&rows).is_ok());
        assert!(!barred("LXGW WenKai Lite"), "the OFL Chinese face is permitted");
        assert!(!web_allowed("LXGW WenKai Lite", "Regular"), "the Chinese face is native-only");
        let relabelled = parse_manifest(&chinese.replace("Hani,Hans", "Hans,Hani")).unwrap_or_default();
        assert!(check_declared_scripts(&relabelled).is_err(), "the Chinese script tags are asserted");
        let unexpected_style = parse_manifest(&chinese.replace("Regular", "Bold")).unwrap_or_default();
        assert!(check_declared_scripts(&unexpected_style).is_err(), "an unlisted Chinese style is rejected");
        assert!(check_chinese_required(&[]).is_err(), "a required native build needs the Chinese face");
    }

    #[test]
    /// Native embedding excludes the barred CJK face; web embedding keeps only allowlisted faces.
    fn web_embedding_keeps_only_the_allowlist() {
        let manifest = format!(
            "{GOOD}\nNoto Sans CJK SC | Regular | fonts/cjk.otf | Hans,Latn | OFL-1.1 | fonts/OFL.txt | 76f45ef4a6bcff344c837c95a7dcc26e017e38b5846d5ae0cdcb5b86be2e2d31 | https://example.invalid/cjk"
        );
        let rows = parse_manifest(&manifest).unwrap_or_default();
        let native: Vec<&str> = embeddable(&rows, false).iter().map(|r| r.family.as_str()).collect();
        let web: Vec<&str> = embeddable(&rows, true).iter().map(|r| r.family.as_str()).collect();
        assert_eq!(native, ["NanumGothic"], "the barred CJK face is never embedded natively");
        assert_eq!(web, ["NanumGothic"], "the web build keeps only allowlisted faces");
    }
}
