//! Interface languages. Strings in the code stay English and are the lookup keys; a catalog per
//! language (`*.tsv`, format in `de.tsv`) maps them to display text when the UI is drawn. Command
//! ids, workbook contents, formulas, file names, the control channel, the CLI and MCP never see
//! translated text, so scripts and agents are unaffected. A string without a translation shows
//! in English, so coverage can grow one entry at a time.
//!
//! The `language` setting is `auto` by default: the UI follows the operating system's language
//! (Windows display language, macOS preferred languages, `LANG`/`LC_*` elsewhere, the browser on
//! the web) when there is a catalog for it, and English otherwise. View ▸ Language picks one by
//! hand; `ui.language` does the same for scripts and agents.
//!
//! This is the system the other Crafting Apps use (PdfCraft, PhotoCraft, WordCraft:
//! `crates/ui-egui/src/i18n`), so the catalog format and habits carry over between them.
//!
//! # Adding a language
//! 1. Add `xx.tsv` next to `de.tsv` (copy its header; translate from the *meaning* of the English
//!    text, clean-room: never copy Microsoft Excel's or another product's localisation. Feature
//!    names may use the terms established in that language, as `de.tsv` does).
//! 2. Add one row to [`LANGUAGES`].
//!
//! The language menu, the system-language match and the catalog tests pick it up from there.
//!
//! # Looking strings up
//! - [`tl!`](crate::tl) / [`t`]: a string in the current language. [`tr`]: in a given one.
//! - [`tip`]: a tooltip such as `Bold (⌘B)`: the text is translated, the shortcut kept.
//! - [`location`]: a ribbon path such as `["Home", "Font"]`, segment by segment.
//! - [`t_at`]: a label at a ribbon location, for the few English words a language translates two
//!   ways.
//! - [`fmt`]: fill `{name}` placeholders after a lookup; translators may reorder them.

mod catalog;

use std::cell::Cell;
use std::sync::OnceLock;

use catalog::Catalog;

/// The `language` setting that follows the system.
pub const AUTO: &str = "auto";

/// One supported interface language.
pub struct LangInfo {
    /// BCP 47 code, lowercase (`de`). Also the `language` setting's value.
    pub code: &'static str,
    /// The language's name in itself, shown in the language menu.
    pub name: &'static str,
    /// Catalog file contents (empty for the built-in English).
    pub source: &'static str,
    catalog: OnceLock<Catalog>,
}

/// The registry. English first: it is the fallback and the source language.
pub static LANGUAGES: [LangInfo; 2] = [
    LangInfo { code: "en", name: "English", source: "", catalog: OnceLock::new() },
    // German; `de-DE`, `de-AT`, `de-CH` and the other regions resolve here.
    LangInfo { code: "de", name: "Deutsch", source: include_str!("de.tsv"), catalog: OnceLock::new() },
];

impl LangInfo {
    fn catalog(&self) -> &Catalog {
        self.catalog.get_or_init(|| {
            let (catalog, errors) = Catalog::parse(self.source);
            for error in errors {
                log::warn!("{} interface catalog: {error}; that entry shows in English", self.code);
            }
            catalog
        })
    }
}

/// A language the UI can be shown in (a handle into [`LANGUAGES`]).
#[derive(Clone, Copy)]
pub struct Lang(&'static LangInfo);

impl std::fmt::Debug for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Lang({})", self.0.code)
    }
}

impl PartialEq for Lang {
    fn eq(&self, other: &Self) -> bool {
        self.0.code == other.0.code
    }
}

impl Eq for Lang {}

impl Lang {
    pub const EN: Lang = Lang(&LANGUAGES[0]);

    pub fn code(self) -> &'static str {
        self.0.code
    }

    pub fn name(self) -> &'static str {
        self.0.name
    }

    /// A language by its exact code (any case).
    pub fn from_code(code: &str) -> Option<Lang> {
        LANGUAGES.iter().find(|l| l.code.eq_ignore_ascii_case(code)).map(Lang)
    }

    /// Resolve the `language` setting: a language code, or [`AUTO`] (and anything unknown, e.g. a
    /// code from a newer version) to follow the system.
    pub fn from_pref(pref: &str) -> Lang {
        Lang::from_code(pref).unwrap_or_else(system_lang)
    }

    /// Every registered language.
    pub fn all() -> impl Iterator<Item = Lang> {
        LANGUAGES.iter().map(Lang)
    }

    fn catalog(self) -> &'static Catalog {
        self.0.catalog()
    }
}

/// The canonical `language` setting for user input: [`AUTO`] or a registered code (any case).
/// `None` for anything else, so callers keep the current setting.
pub fn normalize_pref(pref: &str) -> Option<&'static str> {
    if pref.eq_ignore_ascii_case(AUTO) {
        return Some(AUTO);
    }
    Lang::from_code(pref).map(Lang::code)
}

/// Candidate codes for a locale tag, most specific first: `de_AT.UTF-8` → `de-at`, `de`;
/// `zh_TW` → `zh-tw`, `zh-hant`, `zh`.
fn candidates(tag: &str) -> Vec<String> {
    let base = tag.split(['.', '@']).next().unwrap_or("").replace('_', "-").to_ascii_lowercase();
    let parts: Vec<&str> = base.split('-').filter(|p| !p.is_empty()).collect();
    let Some(&primary) = parts.first() else { return Vec::new() };
    let mut out = Vec::new();
    for n in (1..=parts.len()).rev() {
        out.push(parts.get(..n).unwrap_or_default().join("-"));
    }
    if primary == "zh" && !parts.iter().any(|p| matches!(*p, "hans" | "hant")) {
        // Chinese by region when no script is given.
        let script = if parts.iter().any(|p| matches!(*p, "tw" | "hk" | "mo")) { "zh-hant" } else { "zh-hans" };
        out.insert(out.len().saturating_sub(1), script.to_string());
    }
    out
}

/// The registered language for a locale tag such as `de_DE.UTF-8` or `de-AT`; `None` when there
/// is no catalog for it. `C`/`POSIX` mean English.
pub fn lang_from_tag(tag: &str) -> Option<Lang> {
    let cands = candidates(tag);
    if matches!(cands.first().map(String::as_str), Some("c" | "posix")) {
        return Some(Lang::EN);
    }
    cands.iter().find_map(|c| Lang::from_code(c))
}

/// The first of the system's preferred languages GridCraft has, in the user's order: someone who
/// prefers French, then German, gets German rather than English.
pub fn first_supported<'a>(tags: impl IntoIterator<Item = &'a str>) -> Option<Lang> {
    tags.into_iter().find_map(lang_from_tag)
}

/// The system language (looked up once). English when it can't be determined or isn't supported.
pub fn system_lang() -> Lang {
    // Tests drive the UI by its English labels whatever the machine's language is.
    if cfg!(test) {
        return Lang::EN;
    }
    static SYSTEM: OnceLock<Lang> = OnceLock::new();
    *SYSTEM.get_or_init(detect_system_lang)
}

fn detect_system_lang() -> Lang {
    // An explicit override, then the platform's preferred UI languages (Windows display language,
    // macOS preferred languages, `LC_ALL`/`LC_MESSAGES`/`LANG` on Unix, the browser on the web).
    if let Some(l) = std::env::var("GRIDCRAFT_LANGUAGE").ok().and_then(|v| lang_from_tag(&v)) {
        return l;
    }
    let tags: Vec<String> = sys_locale::get_locales().take(16).collect();
    first_supported(tags.iter().map(String::as_str)).unwrap_or(Lang::EN)
}

thread_local! {
    // Per thread: parallel test harnesses must not change each other's language.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// Set the language the UI is drawn in (the app does this every frame from its setting).
pub fn set_current(lang: Lang) {
    CURRENT.set(lang);
}

/// The language the UI is drawn in.
pub fn current() -> Lang {
    CURRENT.get()
}

/// How numbers, dates and formulas are shown and typed in the current language, as in Excel
/// (German: `1.234,56`, `10.10.2026`, `=SUMME(A1;B1)`). Files, scripts and agents stay en-US.
pub fn number_locale() -> gridcraft_engine::core::Locale {
    gridcraft_engine::core::Locale::from_tag(current().code())
}

/// Does `lang` have a catalog entry for this string? (English never does: it is the source.)
pub fn has(lang: Lang, s: &str) -> bool {
    lang.catalog().plain(s).is_some()
}

/// `s` in the current language ([`tr`] with [`current`]).
pub fn t(s: &str) -> &str {
    tr(current(), s)
}

/// `s` in `lang`; strings without a translation come back unchanged.
pub fn tr(lang: Lang, s: &str) -> &str {
    lang.catalog().plain(s).unwrap_or(s)
}

/// Does this parenthesised tail name a keyboard shortcut (`⌘B`, `⇧F9`, `Ctrl+B`, `F2`)?
fn is_shortcut(inner: &str) -> bool {
    inner.contains(['⌘', '⇧', '⌥', '⌃'])
        || inner.starts_with("Ctrl")
        || inner.starts_with("Alt")
        || inner.starts_with("Shift")
        || (inner.starts_with('F') && inner.len() <= 3 && inner.chars().skip(1).all(|c| c.is_ascii_digit()))
}

/// A tooltip or label in the current language. A trailing shortcut such as ` (⌘B)` stays as
/// written and only the text before it is looked up, so `Bold (⌘B)` needs only `Bold`.
pub fn tip(s: &str) -> String {
    let lang = current();
    if let Some(full) = lang.catalog().plain(s) {
        return full.to_string();
    }
    if let Some((text, rest)) = s.rsplit_once(" (")
        && let Some(inner) = rest.strip_suffix(')')
        && is_shortcut(inner)
    {
        return format!("{} ({inner})", tr(lang, text));
    }
    s.to_string()
}

/// A ribbon path (`["Home", "Font"]`) in the current language, joined with ` › `.
pub fn location(path: &[&str]) -> String {
    path.iter().map(|s| t(s)).collect::<Vec<_>>().join(" › ")
}

/// `s` as labelled at a ribbon path: a catalog entry scoped to the path's last segment
/// (`Outline › Group`) wins over the plain one. Only the few English words a language needs to
/// translate two ways have scoped entries.
pub fn t_at(path: &[&str], s: &str) -> String {
    let lang = current();
    if let Some(group) = path.last()
        && let Some(scoped) = lang.catalog().plain(&format!("{group} › {s}"))
    {
        return scoped.to_string();
    }
    tr(lang, s).to_string()
}

/// Fill `{name}` placeholders in one pass. Unknown placeholders stay as written, and inserted
/// values are never read as templates again, so a file name containing `{n}` stays intact.
pub fn fmt(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some((before, after_open)) = rest.split_once('{') {
        out.push_str(before);
        let Some((name, after_close)) = after_open.split_once('}') else {
            out.push('{');
            out.push_str(after_open);
            return out;
        };
        match args.iter().find(|(key, _)| *key == name) {
            Some((_, value)) => out.push_str(value),
            None => {
                out.push('{');
                out.push_str(name);
                out.push('}');
            }
        }
        rest = after_close;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests;
