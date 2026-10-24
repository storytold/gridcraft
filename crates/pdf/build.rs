use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

const MAX_FONTS: usize = 128;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

// Keep the optional PDF bundle compact. CJK has the predefined CID fallback; Arabic and
// Devanagari layout/bidi are not supported. System and explicitly supplied faces stay available.
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
    let contents = fs::read_to_string(&manifest).map_err(|error| format!("{}: {error}", manifest.display()))?;
    let canonical_root = root.canonicalize().map_err(|error| format!("{}: {error}", root.display()))?;
    let mut fonts = Vec::new();
    let mut total_bytes = 0u64;

    for (line_number, line) in contents.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let Some(file) = fields.get(2).copied().filter(|_| fields.len() == 8) else {
            return Err(format!("{}:{}: expected eight manifest fields separated by a pipe delimiter", manifest.display(), line_number + 1));
        };
        let relative = Path::new(file);
        if relative.is_absolute()
            || relative.components().any(|part| matches!(part, Component::ParentDir | Component::RootDir | Component::Prefix(_)))
        {
            return Err(format!("{}:{}: font path must stay inside the checkout: {file}", manifest.display(), line_number + 1));
        }
        let path = root.join(relative).canonicalize().map_err(|error| format!("{file}: {error}"))?;
        if !path.starts_with(&canonical_root) {
            return Err(format!("{}:{}: font path resolves outside the checkout: {file}", manifest.display(), line_number + 1));
        }
        if !fields.get(3).is_some_and(|scripts| bundled_scripts(scripts)) {
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let size = fs::metadata(&path).map_err(|error| format!("{file}: {error}"))?.len();
        if size > MAX_TOTAL_BYTES {
            return Err(format!("{file} exceeds the {MAX_TOTAL_BYTES}-byte build limit"));
        }
        let bytes = fs::read(&path).map_err(|error| format!("{file}: {error}"))?;
        if !has_truetype_outlines(&bytes) {
            continue;
        }
        total_bytes = total_bytes.checked_add(size).ok_or_else(|| "craft-fonts total size overflow".to_string())?;
        if total_bytes > MAX_TOTAL_BYTES {
            return Err(format!("fonts in the manifest exceed the {MAX_TOTAL_BYTES}-byte build limit"));
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
        println!("cargo:rerun-if-changed={}", path.display());
        let absolute = path.to_string_lossy();
        generated.push_str(&format!("({relative:?}, include_bytes!({absolute:?})),\n"));
    }
    generated.push_str("];\n");
    Ok(generated)
}

#[cfg_attr(test, allow(dead_code))] // The integration harness also exercises this build script.
fn main() -> Result<(), String> {
    println!("cargo:rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo:rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_ARCH");

    // The web UI owns its font budget. PDF faces are supplied per export through `pdfFonts`,
    // so this crate must not add the whole desktop font manifest to the WASM binary.
    let wasm = env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "wasm32");
    let required = !wasm && env::var_os("CRAFT_FONTS_REQUIRED").is_some();
    let mut generated = String::from("pub const CRAFT_FONTS: &[(&str, &[u8])] = &[];\n");
    let configured = if wasm { None } else { env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from) };
    match configured {
        Some(configured) => match configured.canonicalize() {
            Ok(root) => match generated_catalog(&root) {
                Ok(catalog) => generated = catalog,
                Err(error) if required => return Err(format!("CRAFT_FONTS_DIR={}: {error}", root.display())),
                Err(error) => println!("cargo:warning=building without craft-fonts: {error}"),
            },
            Err(error) if required => return Err(format!("CRAFT_FONTS_DIR={}: {error}", configured.display())),
            Err(error) => println!("cargo:warning=building without craft-fonts: CRAFT_FONTS_DIR={}: {error}", configured.display()),
        },
        None if required => return Err("CRAFT_FONTS_REQUIRED is set but CRAFT_FONTS_DIR is not".to_string()),
        None => {}
    }

    let out_dir = env::var_os("OUT_DIR").ok_or_else(|| "OUT_DIR is missing".to_string())?;
    let out = PathBuf::from(out_dir).join("craft_fonts.rs");
    fs::write(&out, generated).map_err(|error| format!("{}: {error}", out.display()))?;
    Ok(())
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
        let root = PathBuf::from(root).canonicalize().unwrap();
        let fonts = manifest_fonts(&root).unwrap();
        let generated = generated_catalog(&root).unwrap();
        for (relative, path) in fonts {
            assert!(has_truetype_outlines(&fs::read(path).unwrap()), "{relative}");
            assert!(generated.contains(&relative));
        }
        assert!(!generated.contains("NotoSansCJKsc-Regular.otf"));
        assert!(!generated.contains("NotoSansArabic.ttf"));
        if env::var_os("GRIDCRAFT_REQUIRE_THAI_FONT").is_some() {
            assert!(generated.contains("NotoSansThai.ttf"));
        }
    }
}
