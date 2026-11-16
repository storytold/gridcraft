use std::{
    env,
    fmt::Write as _,
    fs,
    path::{Component, Path, PathBuf},
};

const MAX_FONTS: usize = 128;
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

#[cfg_attr(test, allow(dead_code))]
fn main() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo:rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    println!("cargo:rerun-if-env-changed=GRIDCRAFT_REQUIRE_THAI_FONT");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");

    let wasm = env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "wasm32");
    let required = !wasm && env::var_os("CRAFT_FONTS_REQUIRED").is_some();
    let thai_required = !wasm && env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_some();
    let configured = if wasm { None } else { env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from) };
    let root = match configured {
        Some(path) => match path.canonicalize() {
            Ok(root) => Some(root),
            Err(error) if required || thai_required => return Err(format!("CRAFT_FONTS_DIR={}: {error}", path.display())),
            Err(error) => {
                println!("cargo:warning=building without craft-fonts: CRAFT_FONTS_DIR={}: {error}", path.display());
                None
            }
        },
        None if required || thai_required => return Err("CRAFT_FONTS_REQUIRED or GRIDCRAFT_REQUIRE_THAI_FONT needs CRAFT_FONTS_DIR".into()),
        None => None,
    };

    let arabic = match root.as_deref() {
        Some(root) => arabic_font_entries(root),
        None => Ok(String::new()),
    };
    let arabic = match arabic {
        Ok(entries) => entries,
        Err(error) if required => return Err(error),
        Err(error) => {
            println!("cargo:warning=building without bundled Arabic fonts: {error}");
            String::new()
        }
    };
    write_generated("arabic_fonts.rs", format!("pub(crate) static ARABIC_FONTS: &[(&str, &[u8])] = &[\n{arabic}];\n"))?;

    // The web UI owns its font budget. PDF faces are supplied per export through `pdfFonts`,
    // so this crate must not add the desktop font manifest to the WASM binary.
    let mut catalog = String::from("pub const CRAFT_FONTS: &[(&str, &[u8])] = &[];\n");
    if let Some(root) = root.as_deref() {
        match generated_catalog(root) {
            Ok(generated) => catalog = generated,
            Err(error) if required || thai_required => return Err(error),
            Err(error) => println!("cargo:warning=building without bundled TrueType PDF fonts: {error}"),
        }
    }
    write_generated("craft_fonts.rs", catalog)
}

fn write_generated(file: &str, contents: String) -> Result<(), String> {
    let out_dir = env::var_os("OUT_DIR").ok_or_else(|| "OUT_DIR is missing".to_string())?;
    let out = PathBuf::from(out_dir).join(file);
    fs::write(&out, contents).map_err(|error| format!("writing {}: {error}", out.display()))
}

/// Embed only Arabic fallback faces from the optional, separately licensed font bundle.
fn arabic_font_entries(root: &Path) -> Result<String, String> {
    use skrifa::MetadataProvider;

    let manifest = root.join("fonts/manifest.txt");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = String::from_utf8(read_font_input(&manifest, 1 << 20)?).map_err(|error| error.to_string())?;
    let mut out = String::new();
    let mut has_regular = false;
    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, _licence, licence_file, checksum, _source] = fields.as_slice() else {
            return Err(format!("malformed craft-fonts manifest line {}", line_number + 1));
        };
        if !scripts.split(',').any(|script| script.trim() == "Arab") {
            continue;
        }
        let path = checked_manifest_path(root, file, line_number + 1)?;
        let licence = checked_manifest_path(root, licence_file, line_number + 1)?;
        println!("cargo:rerun-if-changed={}", path.display());
        println!("cargo:rerun-if-changed={}", licence.display());
        let bytes = read_font_input(&path, MAX_FONT_BYTES)?;
        if sha256(&bytes) != *checksum {
            return Err(format!("checksum mismatch for {file}"));
        }
        let font = skrifa::FontRef::from_index(&bytes, 0).map_err(|error| format!("invalid Arabic font {file}: {error:?}"))?;
        if !"العربية١٢٣".chars().all(|character| font.charmap().map(character).is_some()) {
            return Err(format!("{file} lacks Arabic glyphs"));
        }
        writeln!(out, "    ({:?}, include_bytes!({:?})),", format!("craft:{family}:{style}"), path)
            .map_err(|error| format!("generating Arabic font catalog: {error}"))?;
        has_regular |= *style != "Bold";
    }
    if out.is_empty() {
        return Err("craft-fonts manifest contains no Arabic face; update the checkout".into());
    }
    if !has_regular {
        return Err("craft-fonts manifest has only bold Arabic faces; regular text needs a regular or variable face".into());
    }
    Ok(out)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn checked_manifest_path(root: &Path, relative: &str, line_number: usize) -> Result<PathBuf, String> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path.components().any(|part| matches!(part, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
    {
        return Err(format!("manifest line {line_number}: path must stay inside the checkout: {relative}"));
    }
    let canonical_root = root.canonicalize().map_err(|error| format!("{}: {error}", root.display()))?;
    let path = root.join(relative_path).canonicalize().map_err(|error| format!("{relative}: {error}"))?;
    if !path.starts_with(&canonical_root) {
        return Err(format!("manifest line {line_number}: path resolves outside the checkout: {relative}"));
    }
    Ok(path)
}

fn bundled_scripts(scripts: &str) -> bool {
    !scripts.is_empty() && scripts.split(',').all(|script| matches!(script.trim(), "Latn" | "Grek" | "Cyrl" | "Thai" | "Geor" | "Zyyy" | "Zinh"))
}

fn has_truetype_outlines(bytes: &[u8]) -> bool {
    let count = ttf_parser::fonts_in_collection(bytes).unwrap_or(1);
    (1..=128).contains(&count) && (0..count).all(|index| ttf_parser::Face::parse(bytes, index).is_ok_and(|face| face.tables().glyf.is_some()))
}

fn manifest_fonts(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let manifest = root.join("fonts/manifest.txt");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let contents = read_font_input(&manifest, 1 << 20)?;
    let contents = String::from_utf8(contents).map_err(|error| format!("{}: {error}", manifest.display()))?;
    let mut fonts = Vec::new();
    let mut total_bytes = 0u64;

    for (line_number, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split(" | ").map(str::trim).collect();
        let [_, _, file, scripts, _, licence_file, checksum, _] = fields.as_slice() else {
            return Err(format!("{}:{}: expected eight manifest fields separated by a pipe delimiter", manifest.display(), line_number + 1));
        };
        if !bundled_scripts(scripts) {
            continue;
        }
        let path = checked_manifest_path(root, file, line_number + 1)?;
        let licence = checked_manifest_path(root, licence_file, line_number + 1)?;
        println!("cargo:rerun-if-changed={}", path.display());
        println!("cargo:rerun-if-changed={}", licence.display());
        let bytes = read_font_input(&path, MAX_FONT_BYTES)?;
        if sha256(&bytes) != *checksum {
            return Err(format!("checksum mismatch for {file}"));
        }
        if !has_truetype_outlines(&bytes) {
            continue;
        }
        total_bytes = total_bytes.checked_add(bytes.len() as u64).ok_or_else(|| "craft-fonts total size overflow".to_string())?;
        if total_bytes > MAX_FONT_BYTES {
            return Err(format!("fonts in the manifest exceed the {MAX_FONT_BYTES}-byte build limit"));
        }
        fonts.push((file.to_string(), path));
        if fonts.len() > MAX_FONTS {
            return Err(format!("fonts/manifest.txt contains more than {MAX_FONTS} font files"));
        }
    }
    Ok(fonts)
}

fn generated_catalog(root: &Path) -> Result<String, String> {
    let mut generated = String::from("pub const CRAFT_FONTS: &[(&str, &[u8])] = &[\n");
    for (relative, path) in manifest_fonts(root)? {
        let absolute = path.to_string_lossy();
        writeln!(generated, "({relative:?}, include_bytes!({absolute:?})),").map_err(|error| format!("generating TrueType font catalog: {error}"))?;
    }
    generated.push_str("];\n");
    if env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_some() && !generated.contains("Thai") {
        return Err("craft-fonts manifest contains no Thai TrueType face".into());
    }
    Ok(generated)
}

fn read_font_input(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let file = fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds the font input size limit", path.display()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_bundle_excludes_unneeded_scripts_even_when_they_also_cover_latin() {
        for scripts in ["Thai", "Thai,Latn", "Geor", "Latn,Grek,Cyrl,Zyyy,Zinh"] {
            assert!(bundled_scripts(scripts), "{scripts}");
        }
        for scripts in ["", "Jpan,Latn", "Hans,Latn", "Arab", "Deva", "Unknown"] {
            assert!(!bundled_scripts(scripts), "{scripts}");
        }
        assert!(!has_truetype_outlines(b"OTTO"));
        assert!(!has_truetype_outlines(b"not a font"));
    }

    #[test]
    fn generated_bundle_only_contains_supported_truetype_faces() {
        let Some(root) = env::var_os("CRAFT_FONTS_DIR") else {
            assert!(env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_none(), "CI must exercise the font manifest filter");
            return;
        };
        let root = PathBuf::from(root).canonicalize().expect("craft-fonts directory must exist");
        let fonts = manifest_fonts(&root).expect("manifest fonts should validate");
        let generated = generated_catalog(&root).expect("font catalog should generate");
        for (relative, path) in fonts {
            assert!(has_truetype_outlines(&fs::read(path).expect("font should be readable")), "{relative}");
            assert!(generated.contains(&relative));
        }
        assert!(!generated.contains("NotoSansCJKsc-Regular.otf"));
        assert!(!generated.contains("NotoSansArabic.ttf"));
        if env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_some() {
            assert!(generated.contains("NotoSansThai.ttf"));
        }
    }
}
