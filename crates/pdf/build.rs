use std::fmt::Write as _;

fn main() -> Result<(), String> {
    shaped_fonts()
}

/// Embed only the Arabic and Hebrew fallback faces from the optional, separately licensed font bundle.
fn shaped_fonts() -> Result<(), String> {
    use std::path::PathBuf;
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let required = std::env::var_os("CRAFT_FONTS_REQUIRED").is_some();
    let dir = std::env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from);
    if dir.is_none() && required {
        return Err("CRAFT_FONTS_REQUIRED needs CRAFT_FONTS_DIR".into());
    }
    let mut statics = String::new();
    for (name, script) in [("ARABIC_FONTS", Script::ARABIC), ("HEBREW_FONTS", Script::HEBREW)] {
        let entries = match dir.as_deref().map(|dir| font_entries(dir, &script)) {
            None => String::new(),
            Some(Ok(entries)) => entries,
            Some(Err(e)) if required => return Err(e),
            Some(Err(e)) => {
                println!("cargo::warning=building without bundled {} fonts: {e}", script.name);
                String::new()
            }
        };
        let _ = writeln!(statics, "pub(crate) static {name}: &[(&str, &[u8])] = &[\n{entries}];");
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("shaped_fonts.rs");
    std::fs::write(&out, statics).map_err(|e| format!("writing {}: {e}", out.display()))
}

/// A script whose faces are taken from the bundle: its manifest tag and characters each face must map.
struct Script {
    name: &'static str,
    tag: &'static str,
    sample: &'static str,
}

impl Script {
    const ARABIC: Self = Self { name: "Arabic", tag: "Arab", sample: "العربية١٢٣" };
    const HEBREW: Self = Self { name: "Hebrew", tag: "Hebr", sample: "עברית" };
}

fn font_entries(dir: &std::path::Path, script: &Script) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use skrifa::MetadataProvider;
    let name = script.name;
    let root = dir.canonicalize().map_err(|e| format!("craft-fonts directory: {e}"))?;
    let manifest = root.join("fonts/manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = String::from_utf8(read_font_input(&manifest, 1 << 20)?).map_err(|e| e.to_string())?;
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
        if !scripts.split(',').any(|tag| tag.trim() == script.tag) {
            continue;
        }
        let path = root.join(file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        let licence = root.join(licence_file).canonicalize().map_err(|e| format!("{licence_file}: {e}"))?;
        if !path.starts_with(&root) || !licence.starts_with(&root) {
            return Err("craft-fonts paths must stay inside the checkout".into());
        }
        println!("cargo::rerun-if-changed={}", path.display());
        println!("cargo::rerun-if-changed={}", licence.display());
        let bytes = read_font_input(&path, 64 << 20)?;
        if Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>() != *checksum {
            return Err(format!("checksum mismatch for {file}"));
        }
        let font = skrifa::FontRef::from_index(&bytes, 0).map_err(|e| format!("invalid {name} font {file}: {e}"))?;
        if !script.sample.chars().all(|ch| font.charmap().map(ch).is_some()) {
            return Err(format!("{file} lacks {name} glyphs"));
        }
        let _ = writeln!(out, "    ({:?}, include_bytes!({:?})),", format!("craft:{family}:{style}"), path);
        has_regular |= *style != "Bold";
    }
    if out.is_empty() {
        return Err(format!("craft-fonts manifest contains no {name} face; update the checkout"));
    }
    if !has_regular {
        return Err(format!("craft-fonts manifest has only bold {name} faces; regular text needs a regular or variable face"));
    }
    Ok(out)
}

fn read_font_input(path: &std::path::Path, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > limit {
        return Err(format!("{} exceeds the font input size limit", path.display()));
    }
    Ok(bytes)
}
