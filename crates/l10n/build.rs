//! Embeds `locales/<tag>/*.ftl` and `locales/<tag>/keymap.toml` (repo root) into the crate with
//! `include_str!`, so adding a language or a message file needs no Rust change and works on wasm.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::exit;

fn fail(msg: &str) -> ! {
    println!("cargo::error={msg}");
    exit(1);
}

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| fail("CARGO_MANIFEST_DIR is not set"));
    let out = std::env::var("OUT_DIR").unwrap_or_else(|_| fail("OUT_DIR is not set"));
    let root = Path::new(&manifest).join("../../locales");
    println!("cargo::rerun-if-changed={}", root.display());

    let mut tags: Vec<String> = Vec::new();
    let entries = fs::read_dir(&root).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", root.display())));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| fail(&format!("cannot list {}: {e}", root.display())));
        if entry.path().is_dir() {
            tags.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    tags.sort();
    if !tags.iter().any(|t| t == "en-US") {
        fail("locales/en-US is required: it is the fallback language");
    }

    let mut code = String::new();
    let _ = writeln!(code, "const LANG_COUNT: usize = {};", tags.len());
    code.push_str("static LANGS: [LangSource; LANG_COUNT] = [\n");
    for tag in &tags {
        let dir = root.join(tag);
        let mut files: Vec<String> = Vec::new();
        let rd = fs::read_dir(&dir).unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", dir.display())));
        for f in rd {
            let f = f.unwrap_or_else(|e| fail(&format!("cannot list {}: {e}", dir.display())));
            let name = f.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix(".ftl") {
                files.push(stem.to_string());
            }
        }
        files.sort();
        let _ = writeln!(code, "    LangSource {{\n        tag: {tag:?},\n        files: &[");
        for stem in &files {
            let _ = writeln!(code, "            ({stem:?}, include_str!({:?})),", literal_path(&dir.join(format!("{stem}.ftl"))));
        }
        code.push_str("        ],\n");
        let keymap = dir.join("keymap.toml");
        if keymap.is_file() {
            let _ = writeln!(code, "        keymap: include_str!({:?}),", literal_path(&keymap));
        } else {
            code.push_str("        keymap: \"\",\n");
        }
        code.push_str("    },\n");
    }
    code.push_str("];\n");

    let dest = Path::new(&out).join("langs.rs");
    if let Err(e) = fs::write(&dest, code) {
        fail(&format!("cannot write {}: {e}", dest.display()));
    }
}

/// Path with forward slashes, valid inside a Rust string literal on every host.
fn literal_path(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}
