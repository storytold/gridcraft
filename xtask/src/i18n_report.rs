//! `cargo xtask i18n-report`: the interface strings each language doesn't translate yet, read from
//! the catalogs in `crates/ui-egui/src/i18n/`. A string a catalog lacks shows in English, so the
//! report always succeeds; `cargo xtask ci` runs it first so the list shows in the gate log.
//!
//! The reference set is every key any catalog translates. A catalog is a source file with a
//! `fn translate`; its keys are the string literals that start a match arm (`"key" => …`) or open a
//! table row (`("key", …)`). With the `engine` feature it also lists function help rows keyed by
//! English the function catalog no longer has: those show the English help.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// Missing keys listed per language; the rest are counted.
const LIST_CAP: usize = 50;

pub fn run(root: &Path) -> Result<(), String> {
    let dir = root.join("crates/ui-egui/src/i18n");
    let mut catalogs = BTreeMap::new();
    let mut help = BTreeMap::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) => {
            println!("i18n-report: can't read {}: {e}", dir.display());
            return Ok(());
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(std::ffi::OsStr::to_str).map(str::to_owned) else { continue };
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("rs") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&path) else { continue };
        if let Some(language) = stem.strip_suffix("_functions") {
            help.insert(language.to_owned(), catalog_keys(&source));
        } else if source.contains("fn translate(") {
            catalogs.insert(stem, catalog_keys(&source));
        }
    }
    print!("{}", render(&catalogs));
    print!("{}", function_help(&help));
    Ok(())
}

/// Per language: how many reference keys it lacks, then the first [`LIST_CAP`] of them.
pub fn render(catalogs: &BTreeMap<String, BTreeSet<String>>) -> String {
    let reference: BTreeSet<&String> = catalogs.values().flatten().collect();
    let mut out =
        format!("i18n-report: {} interface strings across {} catalogs; a string a catalog lacks shows in English\n", reference.len(), catalogs.len());
    for (language, keys) in catalogs {
        let missing: Vec<&&String> = reference.iter().filter(|key| !keys.contains(key.as_str())).collect();
        let _ = writeln!(out, "  {language}: {} missing", missing.len());
        list(&mut out, missing.iter().map(|key| key.as_str()), missing.len());
    }
    out
}

fn list<'a>(out: &mut String, keys: impl Iterator<Item = &'a str>, count: usize) {
    for key in keys.take(LIST_CAP) {
        let _ = writeln!(out, "      {key:?}");
    }
    if count > LIST_CAP {
        let _ = writeln!(out, "      … and {} more", count - LIST_CAP);
    }
}

/// Function help rows whose English the function catalog no longer has (an English description
/// reworded, an argument renamed).
#[cfg(feature = "engine")]
fn function_help(help: &BTreeMap<String, BTreeSet<String>>) -> String {
    let catalog = gridcraft_engine::cmd::formulas::function_list();
    let mut known: BTreeSet<&str> = catalog.iter().filter_map(|f| f["description"].as_str()).collect();
    known.extend(catalog.iter().filter_map(|f| f["signature"].as_str()).flat_map(argument_names));
    let mut out = String::new();
    for (language, keys) in help {
        let stale: Vec<&str> = keys.iter().map(String::as_str).filter(|key| !known.contains(key)).collect();
        let _ = writeln!(out, "  {language} function help: {} rows, {} keyed by English the function catalog no longer has", keys.len(), stale.len());
        list(&mut out, stale.iter().copied(), stale.len());
    }
    out
}

#[cfg(not(feature = "engine"))]
fn function_help(_: &BTreeMap<String, BTreeSet<String>>) -> String {
    String::new()
}

/// `SUM(number1, [number2], ...)` -> `number`, `number`: brackets, index and `...` dropped.
#[cfg(feature = "engine")]
fn argument_names(signature: &str) -> Vec<&str> {
    let Some(inner) = signature.split_once('(').and_then(|(_, rest)| rest.strip_suffix(')')) else { return Vec::new() };
    inner
        .split(", ")
        .map(|arg| {
            let arg = arg.strip_prefix('[').unwrap_or(arg);
            let arg = arg.strip_suffix(']').unwrap_or(arg);
            arg.trim_end_matches(|c: char| c.is_ascii_digit())
        })
        .filter(|name| !name.is_empty() && *name != "...")
        .collect()
}

/// The keys of one catalog source: string literals that start a match arm or open a table row.
/// Comments and the test module are skipped.
pub fn catalog_keys(source: &str) -> BTreeSet<String> {
    let source = source.split("#[cfg(test)]").next().unwrap_or(source);
    let mut keys = BTreeSet::new();
    for (value, start, end) in literals(source) {
        let before = source.get(..start).unwrap_or("").trim_end();
        let after = source.get(end..).unwrap_or("").trim_start();
        if after.starts_with("=>") || (before.ends_with('(') && after.starts_with(',')) {
            keys.insert(value);
        }
    }
    keys
}

/// The character a `\u{...}` escape stands for, if it names a valid code point.
fn unicode_escape(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Option<char> {
    let mut hex = String::new();
    for (_, c) in chars.by_ref() {
        match c {
            '}' => break,
            '{' => {}
            c => hex.push(c),
        }
    }
    u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32)
}

/// The value of a string literal whose opening quote was just read, and the byte offset past
/// its closing quote (`len`, the source's length, when it is never closed).
fn string_literal(chars: &mut std::iter::Peekable<std::str::CharIndices<'_>>, len: usize) -> (String, usize) {
    let mut value = String::new();
    let mut end = len;
    while let Some((j, c)) = chars.next() {
        match c {
            '"' => {
                end = j + 1;
                break;
            }
            '\\' => match chars.next().map(|(_, c)| c) {
                Some('n') => value.push('\n'),
                Some('t') => value.push('\t'),
                Some('r') => value.push('\r'),
                Some('0') => value.push('\0'),
                Some(c @ ('\\' | '"' | '\'')) => value.push(c),
                Some('u') => {
                    if let Some(c) = unicode_escape(chars) {
                        value.push(c);
                    }
                }
                // A line continuation skips the newline and the next line's indentation.
                Some('\n') => {
                    while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                        chars.next();
                    }
                }
                _ => {}
            },
            c => value.push(c),
        }
    }
    (value, end)
}

/// Every string literal outside `//` comments: its decoded value, and the byte range from its
/// opening quote to just past its closing one.
fn literals(source: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut chars = source.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '/' && source.get(i..).is_some_and(|rest| rest.starts_with("//")) {
            for (_, c) in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
            continue;
        }
        if c != '"' {
            continue;
        }
        let (value, end) = string_literal(&mut chars, source.len());
        out.push((value, i, end));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_match_arms_and_table_rows_and_lists_what_each_catalog_lacks() {
        let matched = r#"
pub(super) fn translate(text: &str) -> Option<&'static str> {
    Some(match text {
        "Paste" => "Einfügen", // "Commented" => "x",
        "Line\nbreak \"quoted\"" => {
            "Zeile"
        }
        _ => return None,
    })
}
#[cfg(test)]
mod tests { const T: &[(&str, &str)] = &[("Test only", "x")]; }
"#;
        let table = r#"
pub(super) const CORE: &[(&str, &str)] = &[
    ("Paste", "貼り付け"),
    (
        "Long \u{2026}",
        "長い",
    ),
];
pub(super) fn translate(text: &str) -> Option<&'static str> { None }
"#;
        let de = catalog_keys(matched);
        assert_eq!(de.iter().map(String::as_str).collect::<Vec<_>>(), ["Line\nbreak \"quoted\"", "Paste"]);
        let ja = catalog_keys(table);
        assert_eq!(ja.iter().map(String::as_str).collect::<Vec<_>>(), ["Long …", "Paste"]);

        let report = render(&BTreeMap::from([("de".to_owned(), de), ("ja".to_owned(), ja)]));
        assert!(report.starts_with("i18n-report: 3 interface strings across 2 catalogs"), "{report}");
        assert!(report.contains("  de: 1 missing\n      \"Long …\"\n"), "{report}");
        assert!(report.contains("  ja: 1 missing\n      \"Line\\nbreak \\\"quoted\\\"\"\n"), "{report}");
    }
}
