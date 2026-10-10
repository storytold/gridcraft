//! `cargo xtask locales`: coverage report and consistency gate for the localization data.
//!
//! * Function names (`gridcraft_locale::LANGUAGES`): per language, how many catalog functions are
//!   translated, spelled like English, aliased or undocumented (gaps).
//! * Message files (`locales/<tag>/*.ftl`, read through `gridcraft_l10n`'s file layout): per file,
//!   the keys of the language against the `en-US` reference.
//! * Keyboard shortcuts (`locales/<tag>/keymap.toml`): entries per language.
//!
//! The command fails on: problems reported by `gridcraft_l10n::load_problems()`, message keys that a
//! language lacks or adds against `en-US`, a `cmd-*-label` key or a keymap entry that is not an
//! engine command, an engine command without an `en-US` label, and catalog functions that the
//! `en-US` language data does not list. Missing translations of other languages are reported, not
//! failed: the build of the locale data already rejects structural errors.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use fluent_syntax::ast::Entry;
use gridcraft_l10n::{FALLBACK_TAG, command_key};
use gridcraft_locale::{LANGUAGES, Language, eq_ignore_case};

/// Message keys of one `.ftl` source, in file order (terms are not messages).
pub fn ftl_keys(source: &str) -> Result<Vec<String>, String> {
    match fluent_syntax::parser::parse(source) {
        Ok(resource) => Ok(message_keys(&resource.body)),
        Err((_, errors)) => Err(format!("{} syntax error(s)", errors.len())),
    }
}

fn message_keys(body: &[Entry<&str>]) -> Vec<String> {
    body.iter()
        .filter_map(|e| match e {
            Entry::Message(m) => Some(m.id.name.to_string()),
            _ => None,
        })
        .collect()
}

/// Command ids named by the `[shortcuts."<id>"]` tables of a `keymap.toml` source.
pub fn keymap_ids(source: &str) -> Result<Vec<String>, String> {
    let table: toml::Table = toml::from_str(source).map_err(|e| e.to_string())?;
    Ok(table.get("shortcuts").and_then(toml::Value::as_table).map(|t| t.keys().cloned().collect()).unwrap_or_default())
}

/// The command id a `cmd-<id with '.' replaced by '-'>-label` key stands for, matched against the
/// engine ids (the replacement is not reversible, so the ids are the dictionary).
pub fn command_for_key<'a>(key: &str, ids: &'a [String]) -> Option<&'a str> {
    ids.iter().map(String::as_str).find(|id| command_key(id) == key)
}

/// Whether `key` has the shape of a command label.
pub fn is_command_key(key: &str) -> bool {
    key.starts_with("cmd-") && key.ends_with("-label")
}

/// Function-name coverage of one language.
#[derive(Debug, PartialEq, Eq)]
pub struct FunctionRow {
    pub tag: &'static str,
    pub native_name: &'static str,
    pub total: usize,
    pub translated: usize,
    pub same_as_english: usize,
    pub aliases: usize,
    pub gaps: usize,
}

/// Coverage of `lang` against the canonical `catalog` names.
pub fn function_row(lang: &Language, catalog: &[&str]) -> FunctionRow {
    let (mut translated, mut same, mut gaps) = (0, 0, 0);
    for name in catalog {
        match lang.local_function(name) {
            None => gaps += 1,
            Some(_) if lang.gaps.iter().any(|g| g.eq_ignore_ascii_case(name)) => gaps += 1,
            Some(local) if eq_ignore_case(local, name) => same += 1,
            Some(_) => translated += 1,
        }
    }
    FunctionRow {
        tag: lang.tag,
        native_name: lang.native_name,
        total: catalog.len(),
        translated,
        same_as_english: same,
        aliases: lang.by_local.len().saturating_sub(lang.functions.len()),
        gaps,
    }
}

/// Message keys per file of one language directory.
#[derive(Debug, Default)]
pub struct LangFiles {
    pub tag: String,
    /// File stem (`commands`) → keys.
    pub files: BTreeMap<String, Vec<String>>,
    /// Command ids of `keymap.toml` (empty without the file).
    pub keymap: Vec<String>,
}

/// Reads `locales/<tag>/` below `locales`.
pub fn read_lang(locales: &Path, tag: &str) -> Result<LangFiles, String> {
    let dir = locales.join(tag);
    let mut out = LangFiles { tag: tag.to_string(), ..LangFiles::default() };
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        if let Some(stem) = name.strip_suffix(".ftl") {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let keys = ftl_keys(&text).map_err(|e| format!("{tag}/{name}: {e}"))?;
            out.files.insert(stem.to_string(), keys);
        } else if name == "keymap.toml" {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            out.keymap = keymap_ids(&text).map_err(|e| format!("{tag}/{name}: {e}"))?;
        }
    }
    Ok(out)
}

/// Keys a language lacks (`missing`) or adds (`extra`) against the reference, per file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct KeyDiff {
    pub missing: Vec<String>,
    pub extra: Vec<String>,
}

/// Differences of `lang` against `reference`, as `file: key` strings. A file the language lacks
/// counts all its reference keys as missing; a file only the language has counts all its keys as
/// extra.
pub fn diff_keys(reference: &LangFiles, lang: &LangFiles) -> KeyDiff {
    let mut diff = KeyDiff::default();
    let stems: BTreeSet<&String> = reference.files.keys().chain(lang.files.keys()).collect();
    for stem in stems {
        let want: BTreeSet<&String> = reference.files.get(stem).map(|k| k.iter().collect()).unwrap_or_default();
        let have: BTreeSet<&String> = lang.files.get(stem).map(|k| k.iter().collect()).unwrap_or_default();
        diff.missing.extend(want.difference(&have).map(|k| format!("{stem}: {k}")));
        diff.extra.extend(have.difference(&want).map(|k| format!("{stem}: {k}")));
    }
    diff
}

/// Everything the report prints and the gate checks.
pub struct Report {
    pub functions: Vec<FunctionRow>,
    pub reference: LangFiles,
    pub languages: Vec<(LangFiles, KeyDiff)>,
    pub commands_total: usize,
    pub errors: Vec<String>,
}

/// Collects the report from the repository at `root`; `engine_ids` are the engine command ids.
pub fn collect(root: &Path, engine_ids: &[String]) -> Result<Report, String> {
    let mut errors: Vec<String> = Vec::new();
    errors.extend(gridcraft_l10n::load_problems().into_iter().map(|p| format!("l10n: {p}")));

    let catalog: Vec<&str> = gridcraft_formula::catalog::BUILTINS.iter().map(|(n, _)| *n).collect();
    let functions: Vec<FunctionRow> = LANGUAGES.iter().map(|l| function_row(l, &catalog)).collect();
    if let Some(en) = functions.iter().find(|r| r.tag == FALLBACK_TAG)
        && en.gaps > 0
    {
        errors.push(format!("{FALLBACK_TAG}: {} catalog function(s) missing from crates/locale/data/languages/{FALLBACK_TAG}.toml", en.gaps));
    }

    let locales = root.join("locales");
    let mut tags: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&locales).map_err(|e| format!("{}: {e}", locales.display()))? {
        let path = entry.map_err(|e| format!("{}: {e}", locales.display()))?.path();
        if path.is_dir()
            && let Some(name) = path.file_name().and_then(|n| n.to_str())
        {
            tags.push(name.to_string());
        }
    }
    tags.sort();
    if !tags.iter().any(|t| t == FALLBACK_TAG) {
        return Err(format!("{}: the {FALLBACK_TAG} reference directory is missing", locales.display()));
    }
    let embedded = gridcraft_l10n::available_languages();
    for tag in &tags {
        if !embedded.contains(&tag.as_str()) {
            errors.push(format!("locales/{tag} is not embedded in gridcraft-l10n (stale build?)"));
        }
    }

    let reference = read_lang(&locales, FALLBACK_TAG)?;
    let mut languages = Vec::new();
    for tag in tags.iter().filter(|t| t.as_str() != FALLBACK_TAG) {
        let lang = read_lang(&locales, tag)?;
        let diff = diff_keys(&reference, &lang);
        for k in &diff.missing {
            errors.push(format!("{tag}: missing key {k}"));
        }
        for k in &diff.extra {
            errors.push(format!("{tag}: key {k} does not exist in {FALLBACK_TAG}"));
        }
        languages.push((lang, diff));
    }

    let ids: BTreeSet<&str> = engine_ids.iter().map(String::as_str).collect();
    let en_commands: BTreeSet<&String> = reference.files.get("commands").map(|k| k.iter().collect()).unwrap_or_default();
    for id in engine_ids {
        if !en_commands.contains(&command_key(id)) {
            errors.push(format!("{FALLBACK_TAG}: engine command `{id}` has no label (`{}` in commands.ftl)", command_key(id)));
        }
    }
    for lang in std::iter::once(&reference).chain(languages.iter().map(|(l, _)| l)) {
        for key in lang.files.values().flatten().filter(|k| is_command_key(k)) {
            if command_for_key(key, engine_ids).is_none() {
                errors.push(format!("{}: `{key}` is not the label of an engine command", lang.tag));
            }
        }
        for id in &lang.keymap {
            if !ids.contains(id.as_str()) {
                errors.push(format!("{}/keymap.toml: `{id}` is not an engine command", lang.tag));
            }
        }
    }
    Ok(Report { functions, reference, languages, commands_total: engine_ids.len(), errors })
}

/// The printable report.
pub fn render(r: &Report) -> String {
    let mut s = String::new();
    s += "Function names (catalog functions: translated / spelled like English / gaps; aliases are extra accepted spellings)\n\n";
    s += &format!("{:<8} {:<24} {:>5} {:>10} {:>8} {:>8} {:>5}\n", "tag", "language", "total", "translated", "english", "aliases", "gaps");
    for f in &r.functions {
        s += &format!(
            "{:<8} {:<24} {:>5} {:>10} {:>8} {:>8} {:>5}\n",
            f.tag, f.native_name, f.total, f.translated, f.same_as_english, f.aliases, f.gaps
        );
    }
    s += "\nMessage files (keys per file; * = differs from en-US) and keyboard shortcuts\n\n";
    let stems: Vec<&String> = r.reference.files.keys().collect();
    let mut header = format!("{:<8}", "tag");
    for stem in &stems {
        let _ = write!(header, " {:>14}", stem);
    }
    let _ = write!(header, " {:>9} {:>9}", "labels", "keymap");
    s += &header;
    s.push('\n');
    let row = |lang: &LangFiles, diff: Option<&KeyDiff>| -> String {
        let mut line = format!("{:<8}", lang.tag);
        for stem in &stems {
            let have = lang.files.get(*stem).map_or(0, Vec::len);
            let want = r.reference.files.get(*stem).map_or(0, Vec::len);
            let mark = if diff.is_some() && have != want { "*" } else { "" };
            let _ = write!(line, " {:>14}", format!("{have}/{want}{mark}"));
        }
        let labels = lang.files.get("commands").map_or(0, |k| k.iter().filter(|k| is_command_key(k)).count());
        let _ = write!(line, " {:>9} {:>9}", format!("{labels}/{}", r.commands_total), lang.keymap.len());
        line
    };
    s += &row(&r.reference, None);
    s.push('\n');
    for (lang, diff) in &r.languages {
        s += &row(lang, Some(diff));
        s.push('\n');
    }
    s
}

/// `cargo xtask locales`.
pub fn run(root: &Path) -> Result<(), String> {
    let ids: Vec<String> = gridcraft_engine::Session::new().commands().into_iter().map(|c| c.id.to_string()).collect();
    let report = collect(root, &ids)?;
    print!("{}", render(&report));
    println!();
    if report.errors.is_empty() {
        println!("OK: {} language(s) with function data, {} with message files, no problems.", report.functions.len(), report.languages.len() + 1);
        Ok(())
    } else {
        println!("{} problem(s):", report.errors.len());
        for e in &report.errors {
            println!("  - {e}");
        }
        Err(format!("{} localization problem(s)", report.errors.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(tag: &str, entries: &[(&str, &[&str])]) -> LangFiles {
        LangFiles {
            tag: tag.into(),
            files: entries.iter().map(|(s, k)| ((*s).to_string(), k.iter().map(|x| (*x).to_string()).collect())).collect(),
            keymap: vec![],
        }
    }

    #[test]
    fn reads_message_keys_and_ignores_terms_and_comments() {
        let keys = ftl_keys("# comment\nhello = Hi\n-brand = X\nbye = Bye\n    .attr = a\n").unwrap();
        assert_eq!(keys, ["hello", "bye"]);
        assert!(ftl_keys("broken = { $x\n").is_err());
    }

    #[test]
    fn reads_keymap_ids() {
        let ids =
            keymap_ids("[shortcuts]\n\"file.new\" = { keys = \"Cmd+N\", source = \"x\" }\n\"edit.copy\" = { keys = \"Cmd+C\", source = \"x\" }\n")
                .unwrap();
        assert_eq!(ids, ["edit.copy", "file.new"]);
        assert!(keymap_ids("").unwrap().is_empty());
        assert!(keymap_ids("[shortcuts").is_err());
    }

    #[test]
    fn command_keys_map_back_to_engine_ids() {
        let ids = vec!["file.saveAs".to_string(), "home.bold".to_string()];
        assert_eq!(command_for_key("cmd-file-saveAs-label", &ids), Some("file.saveAs"));
        assert_eq!(command_for_key("cmd-file-nope-label", &ids), None);
        assert!(is_command_key("cmd-home-bold-label"));
        assert!(!is_command_key("ribbon-tab-home"));
    }

    #[test]
    fn diffs_missing_and_extra_keys_per_file() {
        let en = files("en-US", &[("ui", &["a", "b"]), ("menu", &["m"])]);
        let pt = files("pt-BR", &[("ui", &["a", "c"]), ("other", &["z"])]);
        let d = diff_keys(&en, &pt);
        assert_eq!(d.missing, ["menu: m", "ui: b"]);
        assert_eq!(d.extra, ["other: z", "ui: c"]);
        assert_eq!(diff_keys(&en, &en), KeyDiff::default());
    }

    #[test]
    fn function_rows_count_translated_english_and_gaps() {
        let en = gridcraft_locale::language("en-US").unwrap();
        let row = function_row(en, &["SUM", "IF", "NO.SUCH.FUNCTION"]);
        assert_eq!((row.total, row.translated, row.same_as_english, row.gaps), (3, 0, 2, 1));
    }

    #[test]
    fn report_lists_every_file_and_language() {
        let en = files("en-US", &[("commands", &["cmd-file-new-label"]), ("ui", &["a"])]);
        let pt = files("pt-BR", &[("commands", &["cmd-file-new-label"])]);
        let diff = diff_keys(&en, &pt);
        let report = Report { functions: vec![], reference: en, languages: vec![(pt, diff)], commands_total: 1, errors: vec![] };
        let text = render(&report);
        assert!(text.contains("commands") && text.contains("ui"), "{text}");
        assert!(text.contains("0/1*"), "a short file is flagged: {text}");
        assert!(text.contains("1/1"), "{text}");
    }
}
