//! Locale tables for GridCraft, following Excel's split between the display language and the
//! regional format:
//!
//! - [`Language`]: localized function names, boolean and error literals, structured-reference
//!   items, R1C1 letters, number-format letters, `CELL`/`INFO` keywords and the default names the
//!   app gives to things it creates (`Sheet1`, `Grand Total`).
//! - [`Regional`]: decimal, thousands, list and array separators, date order and names, AM/PM
//!   designators and currency.
//!
//! The tables are compiled from `data/languages/*.toml` and `data/regions/*.toml` by `build.rs`,
//! which rejects incomplete or conflicting data. Stored text (files, journal, `Formula.text`)
//! always uses the invariant en-US forms; localized forms exist only at the edges.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::cmp::Ordering;
use std::fmt;

/// Order of day, month and year in typed and displayed dates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DateOrder {
    Mdy,
    Dmy,
    Ymd,
}

/// Number-format letters of a language (`yyyy` is `aaaa` in Portuguese, `JJJJ` in German) and the
/// local name of the General format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatWords {
    pub year: char,
    pub month: char,
    pub day: char,
    pub hour: char,
    pub minute: char,
    pub second: char,
    pub general: &'static str,
}

/// Item order of [`Language::table_items`].
pub const TABLE_ITEM_ALL: usize = 0;
pub const TABLE_ITEM_DATA: usize = 1;
pub const TABLE_ITEM_HEADERS: usize = 2;
pub const TABLE_ITEM_TOTALS: usize = 3;
pub const TABLE_ITEM_THIS_ROW: usize = 4;

/// A display language: everything Excel translates in formulas and app-created content.
pub struct Language {
    /// BCP 47 tag, e.g. `pt-BR`.
    pub tag: &'static str,
    /// Name in the language itself, e.g. `Português (Brasil)`.
    pub native_name: &'static str,
    /// Double-byte character set language (Japanese, Korean, Chinese): the `*B` text functions
    /// count full-width characters as two bytes.
    pub dbcs: bool,
    pub bool_true: &'static str,
    pub bool_false: &'static str,
    /// Row and column letters of R1C1 references (`['L', 'C']` in Portuguese).
    pub r1c1: [char; 2],
    /// `#All`, `#Data`, `#Headers`, `#Totals`, `#This Row` (see the `TABLE_ITEM_*` indices).
    pub table_items: [&'static str; 5],
    /// Canonical error literal → local literal (`#N/A` → `#N/D`).
    pub errors: &'static [(&'static str, &'static str)],
    pub format: FormatWords,
    /// Canonical function name → local name, sorted by canonical name.
    pub functions: &'static [(&'static str, &'static str)],
    /// Upper-cased local name (including aliases) → canonical name, sorted by local name.
    pub by_local: &'static [(&'static str, &'static str)],
    /// Canonical names whose local name is not documented; they keep the canonical spelling.
    pub gaps: &'static [&'static str],
    /// `CELL` info types, canonical (lower-case) → local.
    pub cell_types: &'static [(&'static str, &'static str)],
    /// `INFO` types, canonical (lower-case) → local.
    pub info_types: &'static [(&'static str, &'static str)],
    /// Default names for app-created content, key → text (`sheet` → `Planilha`).
    pub content: &'static [(&'static str, &'static str)],
}

impl fmt::Debug for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Language").field("tag", &self.tag).finish_non_exhaustive()
    }
}

impl PartialEq for Language {
    fn eq(&self, other: &Self) -> bool {
        self.tag == other.tag
    }
}

/// Compares an already upper-cased key with `s` upper-cased on the fly (no allocation).
fn cmp_upper(key: &str, s: &str) -> Ordering {
    key.chars().cmp(s.chars().flat_map(char::to_uppercase))
}

/// Unicode case-insensitive equality (`é` matches `É`), accents significant.
pub fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.chars().flat_map(char::to_uppercase).eq(b.chars().flat_map(char::to_uppercase))
}

/// Whether `s` starts with `prefix`, case-insensitively; returns the byte length matched in `s`.
fn prefix_ignore_case(s: &str, prefix: &str) -> Option<usize> {
    let mut it = s.char_indices();
    let mut want = prefix.chars().flat_map(char::to_uppercase).peekable();
    let mut end = 0;
    while want.peek().is_some() {
        let (i, c) = it.next()?;
        for u in c.to_uppercase() {
            if want.next() != Some(u) {
                return None;
            }
        }
        end = i + c.len_utf8();
    }
    Some(end)
}

impl Language {
    /// Local name of a canonical function (`SUM` → `SOMA`); `None` when unknown.
    pub fn local_function(&self, canonical: &str) -> Option<&'static str> {
        self.functions
            .binary_search_by(|(k, _)| k.bytes().cmp(canonical.bytes().map(|b| b.to_ascii_uppercase())))
            .ok()
            .and_then(|i| self.functions.get(i))
            .map(|(_, l)| *l)
    }

    /// Canonical name of a local function name, Unicode case-insensitive (`soma` → `SUM`).
    pub fn canonical_function(&self, local: &str) -> Option<&'static str> {
        self.by_local.binary_search_by(|(k, _)| cmp_upper(k, local)).ok().and_then(|i| self.by_local.get(i)).map(|(_, c)| *c)
    }

    /// Local spelling of a canonical error literal; the canonical one when not translated.
    pub fn local_error<'a>(&self, canonical: &'a str) -> &'a str {
        self.errors.iter().find(|(c, _)| c.eq_ignore_ascii_case(canonical)).map_or(canonical, |(_, l)| *l)
    }

    /// Canonical error literal of a local one (case-insensitive).
    pub fn canonical_error(&self, local: &str) -> Option<&'static str> {
        self.errors.iter().find(|(_, l)| eq_ignore_case(l, local)).map(|(c, _)| *c)
    }

    /// The longest local error literal at the start of `s`: `(canonical, bytes consumed)`.
    pub fn error_prefix(&self, s: &str) -> Option<(&'static str, usize)> {
        self.errors.iter().filter_map(|(c, l)| prefix_ignore_case(s, l).map(|n| (*c, n))).max_by_key(|(_, n)| *n)
    }

    /// `TRUE`/`FALSE` in this language, case-insensitive.
    pub fn parse_bool(&self, word: &str) -> Option<bool> {
        if eq_ignore_case(word, self.bool_true) {
            Some(true)
        } else if eq_ignore_case(word, self.bool_false) {
            Some(false)
        } else {
            None
        }
    }

    pub fn bool_text(&self, b: bool) -> &'static str {
        if b { self.bool_true } else { self.bool_false }
    }

    /// Index (`TABLE_ITEM_*`) of a structured-reference item such as `#Tudo`, case-insensitive.
    pub fn table_item(&self, local: &str) -> Option<usize> {
        let t = local.trim();
        self.table_items.iter().position(|i| eq_ignore_case(i, t))
    }

    /// Default text for app-created content (`sheet`, `grand_total`…); falls back to en-US, then
    /// to the key itself.
    pub fn content<'a>(&self, key: &'a str) -> &'a str {
        let find = |l: &Language| l.content.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        find(self).or_else(|| find(&LANG_EN_US)).unwrap_or(key)
    }

    /// Canonical `CELL` info type of a local or canonical keyword (case-insensitive).
    pub fn canonical_cell_type(&self, word: &str) -> Option<&'static str> {
        keyword(self.cell_types, word)
    }

    /// Canonical `INFO` type of a local or canonical keyword (case-insensitive).
    pub fn canonical_info_type(&self, word: &str) -> Option<&'static str> {
        keyword(self.info_types, word)
    }
}

fn keyword(table: &'static [(&'static str, &'static str)], word: &str) -> Option<&'static str> {
    let w = word.trim();
    table.iter().find(|(c, l)| eq_ignore_case(l, w) || eq_ignore_case(c, w)).map(|(c, _)| *c)
}

/// Regional format: separators, dates, names and currency. Codes are canonical (en-US letters).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Regional {
    pub tag: &'static str,
    pub native_name: &'static str,
    /// Windows locale id, as used by `[$-416]` in format codes.
    pub lcid: u16,
    pub decimal: char,
    pub group: char,
    /// Function-argument and union separator.
    pub list: char,
    /// Column and row separators inside array constants.
    pub array_col: char,
    pub array_row: char,
    pub date_order: DateOrder,
    pub date_sep: char,
    pub time_sep: char,
    pub short_date: &'static str,
    pub long_date: &'static str,
    pub short_time: &'static str,
    pub long_time: &'static str,
    pub months: [&'static str; 12],
    pub months_abbr: [&'static str; 12],
    /// Sunday first.
    pub weekdays: [&'static str; 7],
    pub weekdays_abbr: [&'static str; 7],
    pub am_pm: [&'static str; 2],
    pub currency: &'static str,
    /// Canonical code of the region's currency format.
    pub currency_format: &'static str,
}

impl Regional {
    /// The region with explicit decimal and thousands separators (Excel's "Use system
    /// separators" off). The list and array separators keep the region's values unless they
    /// would collide with the decimal separator (the array column separator may equal the
    /// thousands separator, as in en-US and de-DE).
    pub fn with_separators(self, decimal: char, group: char) -> Regional {
        let group = if group == decimal { if decimal == ',' { '.' } else { ',' } } else { group };
        let list = [self.list, ';', ','].into_iter().find(|c| *c != decimal).unwrap_or(';');
        let array_row = [self.array_row, ';', '.', ','].into_iter().find(|c| *c != decimal).unwrap_or(';');
        let array_col = [self.array_col, ',', '\\', '.'].into_iter().find(|c| *c != decimal && *c != array_row).unwrap_or('\\');
        Regional { decimal, group, list, array_col, array_row, ..self }
    }

    /// Whether `c` is the thousands separator (any space counts when the separator is a space).
    pub fn is_group_char(&self, c: char) -> bool {
        c == self.group || (self.group.is_whitespace() && matches!(c, ' ' | '\u{a0}' | '\u{202f}'))
    }
}

/// The effective locale: interface language, formula language and regional format.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Locale {
    /// Interface language: messages, default content names, DBCS, shortcuts.
    pub ui: &'static Language,
    /// Formula language: function names, booleans, errors, table items, R1C1 and format letters.
    pub formula: &'static Language,
    pub regional: Regional,
}

impl Locale {
    pub fn new(ui: &'static Language, formula: &'static Language, regional: Regional) -> Locale {
        Locale { ui, formula, regional }
    }

    /// The formula dialect: names from the formula language, separators from the region.
    pub fn dialect(&self) -> Dialect<'_> {
        Dialect { names: self.formula, regional: &self.regional }
    }

    /// en-US interface, formulas and region.
    pub fn is_invariant(&self) -> bool {
        *self == INVARIANT
    }
}

impl Default for Locale {
    fn default() -> Self {
        INVARIANT
    }
}

/// en-US everywhere: what files, the journal and programmatic calls use.
pub static INVARIANT: Locale = Locale { ui: &LANG_EN_US, formula: &LANG_EN_US, regional: REGION_EN_US };

/// How formulas are spelled: names from a language, separators from a region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dialect<'a> {
    pub names: &'a Language,
    pub regional: &'a Regional,
}

impl Dialect<'static> {
    /// The canonical (file) dialect.
    pub const INVARIANT: Dialect<'static> = Dialect { names: &LANG_EN_US, regional: &REGION_EN_US };
}

impl Dialect<'_> {
    pub fn is_invariant(&self) -> bool {
        self.names.tag == LANG_EN_US.tag && *self.regional == REGION_EN_US
    }
}

include!(concat!(env!("OUT_DIR"), "/tables.rs"));

/// The language with this exact tag (case-insensitive).
pub fn language(tag: &str) -> Option<&'static Language> {
    LANGUAGES.iter().copied().find(|l| l.tag.eq_ignore_ascii_case(tag))
}

/// The region with this exact tag (case-insensitive).
pub fn region(tag: &str) -> Option<&'static Regional> {
    REGIONS.iter().copied().find(|r| r.tag.eq_ignore_ascii_case(tag))
}

/// The region with this Windows locale id.
pub fn region_by_lcid(lcid: u16) -> Option<&'static Regional> {
    REGIONS.iter().copied().find(|r| r.lcid == lcid)
}

/// Splits `pt_BR.UTF-8`, `pt-br@euro`, `zh-Hant-TW` into a lower-case language and subtags.
fn split_tag(requested: &str) -> (String, Vec<String>) {
    let base = requested.trim().split(['.', '@']).next().unwrap_or("").replace('_', "-");
    let mut parts = base.split('-').filter(|p| !p.is_empty()).map(str::to_ascii_lowercase);
    let lang = parts.next().unwrap_or_default();
    (lang, parts.collect())
}

/// CLDR `parentLocales`: the Portuguese regions whose parent is European Portuguese (`pt_PT`).
const PT_PT_CHILDREN: &[&str] = &["ao", "ch", "cv", "fr", "gq", "gw", "lu", "mo", "mz", "st", "tl"];

/// Index of the best of `available` for an OS or browser locale (`pt_BR.UTF-8`, `pt-AO`,
/// `zh-Hant-HK`, `de-AT`), following CLDR: an exact language-region match; European Portuguese for
/// the regions CLDR parents to `pt_PT` (Brazilian otherwise); Traditional Chinese (`zh-TW`) for
/// `Hant` or TW/HK/MO, Simplified (`zh-CN`) otherwise, an explicit script winning over the region;
/// then the first entry of the same language. `None` for an empty, `C`/`POSIX` or absent language.
///
/// The single rule for UI language, formula language and region, so they never disagree on a tag.
pub fn negotiate_index<T>(requested: &str, available: &[T], tag: impl Fn(&T) -> &str) -> Option<usize> {
    let (lang, rest) = split_tag(requested);
    if lang.is_empty() || lang == "c" || lang == "posix" {
        return None;
    }
    let find = |want: &str| available.iter().position(|x| tag(x).eq_ignore_ascii_case(want));
    let has = |subs: &[&str]| rest.iter().any(|p| subs.contains(&p.as_str()));
    let parent = match lang.as_str() {
        "zh" if has(&["hans"]) => Some("zh-CN"),
        "zh" if has(&["hant", "tw", "hk", "mo"]) => Some("zh-TW"),
        "zh" => Some("zh-CN"),
        "pt" if has(PT_PT_CHILDREN) => Some("pt-PT"),
        "pt" => Some("pt-BR"),
        _ => None,
    };
    // Chinese is decided by script, so `zh-Hans-TW` is not taken as `zh-TW`.
    let exact = if lang == "zh" {
        None
    } else {
        rest.iter().find_map(|sub| {
            available.iter().position(|x| tag(x).split_once('-').is_some_and(|(l, r)| l.eq_ignore_ascii_case(&lang) && r.eq_ignore_ascii_case(sub)))
        })
    };
    exact
        .or_else(|| parent.and_then(find))
        .or_else(|| available.iter().position(|x| tag(x).split('-').next().is_some_and(|l| l.eq_ignore_ascii_case(&lang))))
}

fn negotiate<T: 'static>(requested: &str, all: &'static [&'static T], tag: fn(&T) -> &'static str, fallback: &'static T) -> &'static T {
    negotiate_index(requested, all, |x| tag(x)).and_then(|i| all.get(i)).copied().unwrap_or(fallback)
}

/// Best language for an OS or browser locale (`pt_BR.UTF-8`, `pt`, `de-AT`); en-US otherwise.
pub fn negotiate_language(requested: &str) -> &'static Language {
    negotiate(requested, LANGUAGES, |l| l.tag, &LANG_EN_US)
}

/// Best region for an OS or browser locale; en-US otherwise.
pub fn negotiate_region(requested: &str) -> &'static Regional {
    negotiate(requested, REGIONS, |r| r.tag, &REGION_EN_US)
}

#[cfg(test)]
mod tests;
