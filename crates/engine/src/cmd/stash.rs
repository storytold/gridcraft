//! GridCraft data kept in hidden workbook-scope defined names, so that it travels inside the xlsx
//! file.
//!
//! An item `key` is stored as names `_gc_<key>_0`, `_gc_<key>_1`, …, each a formula of string
//! literals joined with `&`: `"…"&"…"`. Excel limits a string literal to 255 characters and a
//! formula to 8,192, so a long value is split over as many names as it needs and the file stays
//! one that Excel accepts. Older items (custom views, scripts, the watch list and scenarios) keep
//! one name each, `_sc_<key>`; reading accepts that form as well, so those items can move to this
//! format later, and a write of the same key replaces it.

use std::collections::{BTreeMap, HashSet};

use gridcraft_model::{DefinedName, Workbook};
use serde_json::Value as Json;

use crate::{EngineError, Result};

const PREFIX: &str = "_gc_";
const LEGACY_PREFIX: &str = "_sc_";
/// Characters (UTF-16 units, as Excel counts them) in one string literal.
const LITERAL_UNITS: usize = 255;
/// Characters in one name's formula.
const FORMULA_UNITS: usize = 8_192;
/// The longest key: `_gc_<key>_<k>` stays well inside Excel's 255-character names.
const MAX_KEY: usize = 200;
/// The longest item: 4,194,304 characters.
pub const MAX_ITEM_CHARS: usize = 4 * 1024 * 1024;

/// The item text stored under `key`, or `None` when there is none (or it is over the size limit).
pub fn get(wb: &Workbook, key: &str) -> Option<String> {
    let mut chunks: BTreeMap<u64, &str> = BTreeMap::new();
    let mut legacy = None;
    for n in wb.names.iter() {
        match item_of(n) {
            Some((k, Some(i))) if k.eq_ignore_ascii_case(key) => {
                chunks.entry(i).or_insert(&n.formula);
            }
            Some((k, None)) if k.eq_ignore_ascii_case(key) => legacy = legacy.or(Some(&n.formula)),
            _ => {}
        }
    }
    let mut out = String::new();
    if !chunks.contains_key(&0) {
        out = string_literals(legacy?);
    } else {
        // Chunks run 0, 1, 2, …; anything after a gap isn't part of the item.
        for (want, (i, f)) in chunks.iter().enumerate() {
            if *i != want as u64 {
                break;
            }
            out.push_str(&string_literals(f));
            if out.len() > MAX_ITEM_CHARS.saturating_mul(4) {
                return None;
            }
        }
    }
    (out.chars().count() <= MAX_ITEM_CHARS).then_some(out)
}

/// Stores `text` under `key`, replacing what was there (in either format). Empty text removes it.
pub fn set(wb: &mut Workbook, key: &str, text: &str) -> Result<()> {
    set_many(wb, &[(key.to_string(), text.to_string())])
}

/// Stores every `(key, text)` of `items` as [`set`] would, with the checks done before anything
/// changes and one pass over the names for all of them, so writing a thousand items into a
/// workbook with many thousands of names stays near-linear.
pub fn set_many(wb: &mut Workbook, items: &[(String, String)]) -> Result<()> {
    let mut planned = Vec::new();
    let mut keys: HashSet<String> = HashSet::new();
    for (key, text) in items {
        check_key(key)?;
        if text.chars().count() > MAX_ITEM_CHARS {
            return Err(EngineError::Other(format!(
                "The data is too large to keep in the workbook ({} MB at most).",
                MAX_ITEM_CHARS / (1024 * 1024)
            )));
        }
        keys.insert(key.to_ascii_lowercase());
        for (i, formula) in chunk_formulas(text).into_iter().enumerate() {
            planned.push(DefinedName { name: format!("{PREFIX}{key}_{i}"), scope: None, formula, comment: "GridCraft data".into(), hidden: true });
        }
    }
    // A visible name of an item's shape is someone's own (see `item_of`); a hidden name beside
    // it with the same name would be the one an xlsx file drops. Refuse before changing anything.
    let names: HashSet<String> = planned.iter().map(|n| n.name.to_ascii_lowercase()).collect();
    if let Some(n) = wb.names.iter().find(|n| n.scope.is_none() && !n.hidden && names.contains(&n.name.to_ascii_lowercase())) {
        return Err(EngineError::Other(format!(
            "The workbook has a defined name '{}', which GridCraft needs for its own data. Rename it first.",
            n.name
        )));
    }
    // What was there under these keys goes, in either format, then the new chunks come.
    wb.names.retain(|n| !item_of(n).is_some_and(|(k, _)| keys.contains(&k.to_ascii_lowercase())));
    wb.names.extend(planned);
    Ok(())
}

/// Removes the item stored under `key`.
pub fn remove(wb: &mut Workbook, key: &str) {
    wb.names.retain(|n| !item_of(n).is_some_and(|(k, _)| k.eq_ignore_ascii_case(key)));
}

/// The item under `key` as JSON (`None` when missing or not JSON).
pub fn get_json(wb: &Workbook, key: &str) -> Option<Json> {
    serde_json::from_str(&get(wb, key)?).ok()
}

pub fn set_json(wb: &mut Workbook, key: &str, v: &Json) -> Result<()> {
    set(wb, key, &v.to_string())
}

/// The keys of the items stored in the workbook that start with `prefix` (`"macro_"`), sorted.
pub fn keys(wb: &Workbook, prefix: &str) -> Vec<String> {
    let mut keys: Vec<String> = wb
        .names
        .iter()
        // A later chunk on its own is no item, as `get` reads it.
        .filter_map(|n| item_of(n).filter(|(_, chunk)| chunk.is_none_or(|i| i == 0)).map(|(k, _)| k.to_ascii_lowercase()))
        .filter(|k| k.starts_with(&prefix.to_ascii_lowercase()))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

/// The item a hidden name belongs to, lower-cased: `_gc_<key>_<k>` and `_sc_<key>` give `<key>`.
pub fn item_key(name: &str) -> Option<String> {
    parse_name(name).map(|(k, _)| k.to_ascii_lowercase())
}

/// The item a name holds a part of: a hidden workbook-scope `_gc_<key>_<k>` name, or a legacy
/// `_sc_<key>` one. A visible name of the `_gc_` shape is someone's own name, not an item: it is
/// neither read, nor listed, nor removed on write.
fn item_of(n: &DefinedName) -> Option<(&str, Option<u64>)> {
    if n.scope.is_some() {
        return None;
    }
    parse_name(&n.name).filter(|(_, chunk)| chunk.is_none() || n.hidden)
}

/// `_gc_<key>_<k>` → (`key`, `Some(k)`); `_sc_<key>` → (`key`, `None`).
fn parse_name(name: &str) -> Option<(&str, Option<u64>)> {
    if let Some(rest) = strip_prefix_ci(name, PREFIX) {
        let (key, k) = rest.rsplit_once('_')?;
        // Only the chunk number as `set` writes it: `_gc_watch_00` is someone else's name, not
        // chunk 0 under another spelling, so it is neither read nor in the way of `_gc_watch_0`.
        let canonical = !k.is_empty() && k.len() <= 9 && k.bytes().all(|b| b.is_ascii_digit()) && (k == "0" || !k.starts_with('0'));
        if key.is_empty() || !canonical {
            return None;
        }
        return k.parse().ok().map(|i| (key, Some(i)));
    }
    strip_prefix_ci(name, LEGACY_PREFIX).filter(|k| !k.is_empty()).map(|k| (k, None))
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| s.get(prefix.len()..)).flatten()
}

fn check_key(key: &str) -> Result<()> {
    let ok = !key.is_empty() && key.len() <= MAX_KEY && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.');
    if ok { Ok(()) } else { Err(EngineError::Other(format!("`{key}` can't name workbook data: use letters, digits, `_` and `.`"))) }
}

/// `text` as name formulas: string literals of at most 255 characters joined with `&`, each formula
/// at most 8,192 characters. Concatenating [`string_literals`] of each gives `text` back.
pub fn chunk_formulas(text: &str) -> Vec<String> {
    let mut formulas = Vec::new();
    let (mut formula, mut units) = (String::new(), 0usize);
    let (mut literal, mut lit_units) = (String::from("\""), 0usize);
    let mut flush_literal = |literal: &mut String, lit_units: &mut usize, formula: &mut String, units: &mut usize| {
        literal.push('"');
        let cost = literal.encode_utf16().count() + usize::from(!formula.is_empty());
        if *units + cost > FORMULA_UNITS && !formula.is_empty() {
            formulas.push(std::mem::take(formula));
            *units = 0;
        }
        if !formula.is_empty() {
            formula.push('&');
            *units += 1;
        }
        *units += literal.encode_utf16().count();
        formula.push_str(literal);
        *literal = String::from("\"");
        *lit_units = 0;
    };
    for ch in text.chars() {
        if lit_units + ch.len_utf16() > LITERAL_UNITS {
            flush_literal(&mut literal, &mut lit_units, &mut formula, &mut units);
        }
        lit_units += ch.len_utf16();
        if ch == '"' {
            literal.push('"');
        }
        literal.push(ch);
    }
    if lit_units > 0 {
        flush_literal(&mut literal, &mut lit_units, &mut formula, &mut units);
    }
    if !formula.is_empty() {
        formulas.push(formula);
    }
    formulas
}

/// Concatenates the string literals of a `"a"&"b""c"` formula (anything outside them is ignored).
pub fn string_literals(f: &str) -> String {
    let mut out = String::new();
    let mut it = f.chars().peekable();
    let mut in_str = false;
    while let Some(c) = it.next() {
        if !in_str {
            if c == '"' {
                in_str = true;
            }
            continue;
        }
        if c == '"' {
            if it.peek() == Some(&'"') {
                it.next();
                out.push('"');
            } else {
                in_str = false;
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(n: &str, formula: &str) -> DefinedName {
        DefinedName { name: n.into(), scope: None, formula: formula.into(), comment: String::new(), hidden: true }
    }

    fn excel_limits_hold(wb: &Workbook) {
        for n in &wb.names {
            assert!(n.formula.encode_utf16().count() <= FORMULA_UNITS, "{} is {} long", n.name, n.formula.len());
            // Every literal (quotes undoubled) is at most 255 characters.
            for lit in n.formula.split("\"&\"") {
                let inner = lit.trim_start_matches('"').trim_end_matches('"').replace("\"\"", "\"");
                assert!(inner.encode_utf16().count() <= LITERAL_UNITS, "{}", inner.len());
            }
        }
    }

    #[test]
    fn long_values_split_over_names_and_come_back() {
        let mut wb = Workbook::new();
        // Quotes double when written; characters outside the BMP count twice.
        let text: String = (0..30_000).map(|i| ['a', '"', 'é', '\u{1D11E}', '&', '\n'][i % 6]).collect();
        set(&mut wb, "big", &text).unwrap();
        let names: Vec<&str> = wb.names.iter().map(|n| n.name.as_str()).collect();
        assert!(names.len() > 5, "{names:?}");
        assert_eq!(names[0], "_gc_big_0");
        assert!(wb.names.iter().all(|n| n.hidden && n.scope.is_none()));
        excel_limits_hold(&wb);
        assert_eq!(get(&wb, "big").as_deref(), Some(text.as_str()));
        assert_eq!(get(&wb, "BIG").as_deref(), Some(text.as_str()), "names are case-insensitive");
        // Shorter again: the extra chunks go.
        set(&mut wb, "big", "short").unwrap();
        assert_eq!(wb.names.len(), 1);
        assert_eq!(wb.names[0].formula, "\"short\"");
        set(&mut wb, "big", "").unwrap();
        assert!(wb.names.is_empty());
        assert_eq!(get(&wb, "big"), None);
    }

    #[test]
    fn chunks_and_literals_respect_the_boundaries() {
        for len in [0, 1, 254, 255, 256, 8_000, 8_192, 8_193, 20_000] {
            let text = "q".repeat(len);
            let fs = chunk_formulas(&text);
            assert_eq!(fs.iter().map(|f| string_literals(f)).collect::<String>(), text, "{len}");
            assert!(fs.iter().all(|f| f.len() <= FORMULA_UNITS), "{len}");
        }
        // All quotes: each literal doubles in the formula.
        let text = "\"".repeat(9_000);
        let fs = chunk_formulas(&text);
        assert_eq!(fs.iter().map(|f| string_literals(f)).collect::<String>(), text);
        assert!(fs.iter().all(|f| f.len() <= FORMULA_UNITS));
    }

    #[test]
    fn the_old_single_name_still_reads_and_is_replaced_on_write() {
        let mut wb = Workbook::new();
        wb.names.push(name("_sc_scripts", "\"{\"\"A\"\":[1]}\""));
        wb.names.push(name("_sc_scenarios", "\"[1,\"&\"2]\""));
        assert_eq!(get_json(&wb, "scripts"), Some(serde_json::json!({"A": [1]})));
        assert_eq!(get_json(&wb, "scenarios"), Some(serde_json::json!([1, 2])));
        set_json(&mut wb, "scripts", &serde_json::json!({"B": []})).unwrap();
        let names: Vec<&str> = wb.names.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["_sc_scenarios", "_gc_scripts_0"]);
        assert_eq!(get_json(&wb, "scripts"), Some(serde_json::json!({"B": []})));
    }

    #[test]
    fn hostile_names_read_safely() {
        let mut wb = Workbook::new();
        // A gap: chunk 2 without chunk 1.
        wb.names.push(name("_gc_gap_0", "\"a\""));
        wb.names.push(name("_gc_gap_2", "\"c\""));
        assert_eq!(get(&wb, "gap").as_deref(), Some("a"));
        // Only a later chunk: no item, not listed, and the legacy name is still read.
        wb.names.push(name("_gc_orphan_2", "\"c\""));
        assert_eq!(get(&wb, "orphan"), None);
        assert_eq!(keys(&wb, "orphan"), Vec::<String>::new());
        wb.names.push(name("_sc_orphan", "\"old\""));
        assert_eq!(get(&wb, "orphan").as_deref(), Some("old"));
        assert_eq!(keys(&wb, "orphan"), ["orphan"]);
        // Chunks out of order, a duplicate, junk around the literals, an unterminated literal.
        wb.names.push(name("_gc_mix_1", "\"b\"&A1&"));
        wb.names.push(name("_gc_mix_0", "SUM(\"a\")"));
        wb.names.push(name("_gc_mix_0", "\"ignored\""));
        wb.names.push(name("_gc_mix_2", "\"c"));
        assert_eq!(get(&wb, "mix").as_deref(), Some("abc"));
        // Names that only look like chunks.
        for odd in ["_gc_", "_gc__0", "_gc_x_", "_gc_x_-1", "_gc_x_99999999999999999999", "_sc_", "_gcx_0"] {
            wb.names.push(name(odd, "\"zzz\""));
        }
        assert_eq!(get(&wb, "x"), None);
        assert_eq!(get(&wb, ""), None);
        // Not JSON, or JSON nested deeper than the parser allows.
        wb.names.push(name("_gc_notjson_0", "\"{\""));
        assert_eq!(get_json(&wb, "notjson"), None);
        let deep = format!("\"{}{}\"", "[".repeat(10_000), "]".repeat(10_000));
        wb.names.push(name("_gc_deep_0", &deep));
        assert_eq!(get_json(&wb, "deep"), None);
        // Sheet-scoped names are someone else's.
        wb.names.push(DefinedName { scope: Some(0), ..name("_gc_scoped_0", "\"s\"") });
        assert_eq!(get(&wb, "scoped"), None);
        // Bad keys are refused.
        for bad in ["", "has space", "quote\"", "a/b", &"k".repeat(201)] {
            assert!(set(&mut wb, bad, "v").is_err(), "{bad}");
        }
        assert!(set(&mut wb, "big", &"x".repeat(MAX_ITEM_CHARS + 1)).is_err());
    }

    #[test]
    fn oversized_items_are_not_read() {
        let mut wb = Workbook::new();
        let chunk = format!("\"{}\"", "x".repeat(250));
        let big = vec![chunk.as_str(); 32].join("&");
        for i in 0..(MAX_ITEM_CHARS / 8_000 + 2) {
            wb.names.push(name(&format!("_gc_huge_{i}"), &big));
        }
        assert_eq!(get(&wb, "huge"), None);
    }

    #[test]
    fn a_visible_name_of_the_item_shape_is_not_an_item() {
        // Someone's own visible `_gc_watch_0` is not storage: it is not read or listed, and
        // writing or removing the item leaves it alone.
        let mut wb = Workbook::new();
        let own = DefinedName { hidden: false, ..name("_gc_watch_0", "\"mine\"") };
        wb.names.push(own.clone());
        assert_eq!(get(&wb, "watch"), None);
        assert!(keys(&wb, "").is_empty());
        // Writing the item would put a hidden name of the same name beside it, and an xlsx file
        // keeps only one of them: refused, with the names as they were.
        let err = set(&mut wb, "watch", "theirs").unwrap_err().to_string();
        assert!(err.contains("_gc_watch_0"), "{err}");
        assert_eq!(wb.names, vec![own.clone()]);
        assert_eq!(get(&wb, "watch"), None);
        set(&mut wb, "other", "theirs").unwrap();
        assert_eq!(get(&wb, "other").as_deref(), Some("theirs"));
        remove(&mut wb, "watch");
        remove(&mut wb, "other");
        assert_eq!(wb.names, vec![own]);
    }

    #[test]
    fn a_chunk_number_with_leading_zeros_is_another_name() {
        // `_gc_watch_00` is not chunk 0 under another spelling: a hidden one is not read, and a
        // visible one is not in the way of writing `_gc_watch_0`.
        let mut wb = Workbook::new();
        wb.names.push(name("_gc_watch_00", "\"theirs\""));
        assert_eq!(get(&wb, "watch"), None);
        assert!(keys(&wb, "").is_empty());
        let own = DefinedName { hidden: false, ..name("_gc_watch_00", "\"mine\"") };
        wb.names.push(own.clone());
        set(&mut wb, "watch", "ours").unwrap();
        assert_eq!(get(&wb, "watch").as_deref(), Some("ours"));
        assert_eq!(wb.names.len(), 3, "both other names stay");
        remove(&mut wb, "watch");
        assert_eq!(wb.names, vec![name("_gc_watch_00", "\"theirs\""), own]);
    }

    #[test]
    fn keys_list_items_by_prefix() {
        let mut wb = Workbook::new();
        set(&mut wb, "macro_2", "b").unwrap();
        set(&mut wb, "macro_1", &"a".repeat(9_000)).unwrap();
        set(&mut wb, "scripts", "{}").unwrap();
        assert_eq!(keys(&wb, "macro_"), ["macro_1", "macro_2"]);
        assert_eq!(item_key("_gc_macro_1_3").as_deref(), Some("macro_1"));
        assert_eq!(item_key("_SC_Scripts").as_deref(), Some("scripts"));
        assert_eq!(item_key("Total"), None);
    }

    #[test]
    fn items_round_trip_through_xlsx() {
        let mut wb = Workbook::new();
        let text: String = (0..20_000).map(|i| ['x', '"', 'ü', '\u{1D11E}', ' '][i % 5]).collect();
        set(&mut wb, "blob", &text).unwrap();
        wb.names.push(name("_sc_watch", "\"[\"\"A1\"\"]\""));
        let bytes = gridcraft_xlsx::write_xlsx(&wb).unwrap();
        let (back, warnings) = crate::io::open_bytes("data.xlsx", &bytes).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(get(&back, "blob").as_deref(), Some(text.as_str()));
        assert_eq!(get_json(&back, "watch"), Some(serde_json::json!(["A1"])));
    }
}
