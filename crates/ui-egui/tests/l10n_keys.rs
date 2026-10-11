//! Every UI text the sources hand to the localizer has a message in the language files: for
//! `tr("English")`, `msg!("English")` and `tip(l, platform, "English", id)` the key is derived from the
//! English text and the en-US value must be that text; for `text("ui-…")` the key must exist. Each
//! key must also exist in pt-BR, so a new literal cannot ship untranslated.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use gridcraft_l10n::{Localizer, slug};

fn key(english: &str) -> String {
    format!("ui-{}", slug(&english.replace('…', " ellipsis ")))
}

/// Reads the Rust string literal that starts right after `start` (just past its opening quote).
fn literal(src: &str, start: usize) -> Option<String> {
    let mut out = String::new();
    let mut chars = src[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                '\\' => out.push('\\'),
                'u' => {
                    let mut hex = String::new();
                    for h in chars.by_ref() {
                        if h == '}' {
                            break;
                        }
                        if h != '{' {
                            hex.push(h);
                        }
                    }
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
    None
}

fn sources() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<PathBuf> =
        std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
    files.sort();
    files
}

struct Found {
    /// `(file, english)` of `tr`/`msg!`/`tip` literals.
    english: BTreeSet<(String, String)>,
    /// `(file, key)` of explicit `text("ui-…")` keys.
    keys: BTreeSet<(String, String)>,
}

fn scan() -> Found {
    let mut found = Found { english: BTreeSet::new(), keys: BTreeSet::new() };
    for path in sources() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name == "l10n.rs" {
            continue;
        }
        let full = std::fs::read_to_string(&path).unwrap();
        let src = full.split("#[cfg(test)]").next().unwrap();
        for (needle, explicit) in [(".tr(\"", false), ("msg!(\"", false), ("platform(), \"", false), (".text(\"ui-", true), ("text(\"ui-", true)] {
            let mut from = 0;
            while let Some(i) = src[from..].find(needle) {
                let at = from + i + needle.len();
                from = at;
                if explicit {
                    let Some(rest) = literal(src, at) else { continue };
                    found.keys.insert((name.clone(), format!("ui-{rest}")));
                } else if let Some(text) = literal(src, at) {
                    found.english.insert((name.clone(), text));
                }
            }
        }
        // Explicit keys passed as plain string constants (`label: "ui-dialogs-…"`).
        let mut from = 0;
        while let Some(i) = src[from..].find("\"ui-") {
            let at = from + i + 1;
            from = at;
            if let Some(k) = literal(src, at)
                && !k.ends_with('-')
                && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            {
                found.keys.insert((name.clone(), k));
            }
        }
    }
    found
}

/// Message ids defined by the `ui*.ftl` files of a language.
fn defined(tag: &str) -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales").join(tag);
    let mut keys = BTreeSet::new();
    for entry in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        let path = entry.path();
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        if !file.starts_with("ui") || path.extension().is_none_or(|x| x != "ftl") {
            continue;
        }
        for line in std::fs::read_to_string(&path).unwrap().lines() {
            if line.starts_with("ui-")
                && let Some((id, _)) = line.split_once('=')
            {
                keys.insert(id.trim().to_string());
            }
        }
    }
    keys
}

#[test]
fn literals_have_english_and_portuguese_messages() {
    let found = scan();
    let en = Localizer::new("en-US");
    let en_keys = defined("en-US");
    let pt_keys = defined("pt-BR");
    let mut problems = Vec::new();
    let mut by_key: BTreeMap<String, String> = BTreeMap::new();
    for (file, english) in &found.english {
        let k = key(english);
        if let Some(other) = by_key.insert(k.clone(), english.clone())
            && other != *english
        {
            problems.push(format!("{file}: `{english}` and `{other}` share the key {k}"));
        }
        match en.get(&k, &[]) {
            Some(value) if value == *english => {}
            Some(value) => problems.push(format!("{file}: {k} is `{value}` in en-US, the source says `{english}`")),
            None => problems.push(format!("{file}: missing en-US message {k} for `{english}`")),
        }
        if !pt_keys.contains(&k) {
            problems.push(format!("{file}: missing pt-BR message {k} for `{english}`"));
        }
    }
    for (file, k) in &found.keys {
        if !en_keys.contains(k) && !en.has(k) {
            problems.push(format!("{file}: missing en-US message {k}"));
        }
        if !pt_keys.contains(k) {
            problems.push(format!("{file}: missing pt-BR message {k}"));
        }
    }
    assert!(problems.is_empty(), "{} problems:\n{}", problems.len(), problems.join("\n"));
}

#[test]
fn function_categories_are_named() {
    let en = Localizer::new("en-US");
    let pt = defined("pt-BR");
    for id in [
        "Financial",
        "Logical",
        "Text",
        "DateTime",
        "Lookup",
        "MathTrig",
        "Statistical",
        "Engineering",
        "Information",
        "Database",
        "Compatibility",
        "Web",
        "Cube",
    ] {
        let k = format!("ui-fn-category-{id}");
        assert!(en.has(&k), "missing en-US {k}");
        assert!(pt.contains(&k), "missing pt-BR {k}");
    }
}
