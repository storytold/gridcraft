//! Compiles `data/languages/*.toml` and `data/regions/*.toml` into static tables, rejecting
//! incomplete or conflicting data.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LanguageFile {
    tag: String,
    native_name: String,
    dbcs: bool,
    bool_true: String,
    bool_false: String,
    r1c1: String,
    table_items: Vec<String>,
    /// Use another language's function table unchanged (CJK Excel keeps English names).
    #[serde(default)]
    functions_from: Option<String>,
    #[serde(default)]
    gaps: Vec<String>,
    #[serde(default)]
    provenance: BTreeMap<String, toml::Value>,
    errors: BTreeMap<String, String>,
    format: FormatFile,
    #[serde(default)]
    functions: BTreeMap<String, String>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    cell_types: BTreeMap<String, String>,
    info_types: BTreeMap<String, String>,
    content: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FormatFile {
    year: char,
    month: char,
    day: char,
    hour: char,
    minute: char,
    second: char,
    general: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegionFile {
    tag: String,
    native_name: String,
    lcid: u16,
    decimal: char,
    group: char,
    list: char,
    array_col: char,
    array_row: char,
    date_order: String,
    date_sep: char,
    time_sep: char,
    short_date: String,
    long_date: String,
    short_time: String,
    long_time: String,
    months: Vec<String>,
    months_abbr: Vec<String>,
    weekdays: Vec<String>,
    weekdays_abbr: Vec<String>,
    am_pm: Vec<String>,
    currency: String,
    currency_format: String,
    #[serde(default)]
    provenance: BTreeMap<String, toml::Value>,
}

fn read_dir_sorted(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| fail(&format!("cannot read {}: {e}", dir.display())))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    v.sort();
    v
}

fn fail(msg: &str) -> ! {
    eprintln!("gridcraft-locale data error: {msg}");
    std::process::exit(1)
}

fn ident(tag: &str) -> String {
    tag.replace('-', "_").to_ascii_uppercase()
}

fn upper(s: &str) -> String {
    s.chars().flat_map(char::to_uppercase).collect()
}

fn lit(s: &str) -> String {
    format!("{s:?}")
}

fn pairs(v: &[(String, String)]) -> String {
    let mut s = String::from("&[");
    for (a, b) in v {
        let _ = write!(s, "({}, {}),", lit(a), lit(b));
    }
    s.push(']');
    s
}

fn same_keys(what: &str, tag: &str, got: &BTreeMap<String, String>, want: &BTreeMap<String, String>) {
    for k in want.keys() {
        if !got.contains_key(k) {
            fail(&format!("{tag}: missing {what} `{k}`"));
        }
    }
    for k in got.keys() {
        if !want.contains_key(k) {
            fail(&format!("{tag}: unknown {what} `{k}` (not in en-US)"));
        }
    }
}

/// Fails when two values are equal ignoring case.
fn unique_values<'a>(tag: &str, what: &str, values: impl Iterator<Item = &'a String>) {
    let mut seen: HashMap<String, &str> = HashMap::new();
    for v in values {
        if let Some(prev) = seen.insert(upper(v.trim()), v) {
            fail(&format!("{tag}: {what} `{v}` is used twice (also `{prev}`)"));
        }
    }
}

/// Keyword tables accept the local or the canonical spelling, so neither may name another entry.
fn unique_keywords(tag: &str, what: &str, table: &BTreeMap<String, String>) {
    let mut owner: HashMap<String, &str> = HashMap::new();
    for (canonical, local) in table {
        for word in [canonical, local] {
            if let Some(prev) = owner.insert(upper(word.trim()), canonical)
                && prev != canonical
            {
                fail(&format!("{tag}: {what} `{word}` names both {prev} and {canonical}"));
            }
        }
    }
}

fn is_function_word(s: &str) -> bool {
    let mut cs = s.chars();
    cs.next().is_some_and(|c| c.is_alphabetic() || c == '_') && s.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=build.rs");

    // ---------------------------------------------------------------- languages
    let mut langs: Vec<LanguageFile> = Vec::new();
    for path in read_dir_sorted(&root.join("languages")) {
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
        let l: LanguageFile = toml::from_str(&text).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
        if path.file_stem().and_then(|s| s.to_str()) != Some(l.tag.as_str()) {
            fail(&format!("{}: file name must match tag `{}`", path.display(), l.tag));
        }
        langs.push(l);
    }
    // en-US first, then the others in tag order.
    langs.sort_by_key(|l| (l.tag != "en-US", l.tag.clone()));
    let Some(en) = langs.first().filter(|l| l.tag == "en-US") else { fail("data/languages/en-US.toml is required") };
    if en.functions_from.is_some() || !en.gaps.is_empty() || !en.aliases.is_empty() {
        fail("en-US: the canonical table cannot inherit, have gaps or aliases");
    }
    for (k, v) in &en.functions {
        if k != v || *k != k.to_ascii_uppercase() || !is_function_word(k) {
            fail(&format!("en-US: function `{k}` must be upper-case and map to itself"));
        }
    }
    // en-US is the canonical dialect files and the journal are written in: pin it.
    let en_format = &en.format;
    if (en.bool_true.as_str(), en.bool_false.as_str(), en.r1c1.as_str()) != ("TRUE", "FALSE", "RC")
        || en.table_items != ["#All", "#Data", "#Headers", "#Totals", "#This Row"]
        || en.errors.iter().any(|(k, v)| k != v)
        || (en_format.year, en_format.month, en_format.day, en_format.hour, en_format.minute, en_format.second) != ('y', 'm', 'd', 'h', 'm', 's')
        || en_format.general != "General"
    {
        fail("en-US: booleans, R1C1 letters, table items, errors and format words must keep their canonical spelling");
    }
    let en_functions = en.functions.clone();
    let en_errors = en.errors.clone();
    let en_content = en.content.clone();
    let en_cell = en.cell_types.clone();
    let en_info = en.info_types.clone();

    let mut out = String::new();
    let mut statics = Vec::new();
    for l in &langs {
        let tag = &l.tag;
        if l.tag != "en-US" && l.provenance.is_empty() {
            fail(&format!("{tag}: a [provenance] table is required"));
        }
        let functions: BTreeMap<String, String> = match &l.functions_from {
            Some(from) if from == "en-US" => {
                if !l.functions.is_empty() || !l.gaps.is_empty() {
                    fail(&format!("{tag}: functions_from excludes [functions] and gaps"));
                }
                en_functions.clone()
            }
            Some(other) => fail(&format!("{tag}: functions_from = `{other}` (only en-US is supported)")),
            None => {
                for g in &l.gaps {
                    if !en_functions.contains_key(g) {
                        fail(&format!("{tag}: gap `{g}` is not a canonical function"));
                    }
                    if l.functions.contains_key(g) {
                        fail(&format!("{tag}: `{g}` is both translated and a gap"));
                    }
                }
                let mut all = l.functions.clone();
                for g in &l.gaps {
                    all.insert(g.clone(), g.clone());
                }
                same_keys("function", tag, &all, &en_functions);
                all
            }
        };
        same_keys("error", tag, &l.errors, &en_errors);
        same_keys("content key", tag, &l.content, &en_content);
        same_keys("CELL type", tag, &l.cell_types, &en_cell);
        same_keys("INFO type", tag, &l.info_types, &en_info);
        if l.table_items.len() != 5 {
            fail(&format!("{tag}: table_items needs 5 entries"));
        }
        let r1c1: Vec<char> = l.r1c1.chars().collect();
        let r1c1_upper: Vec<char> = upper(&l.r1c1).chars().collect();
        if r1c1.len() != 2 || r1c1.iter().any(|c| !c.is_alphabetic()) || r1c1_upper.first() == r1c1_upper.get(1) {
            fail(&format!("{tag}: r1c1 must be two distinct letters"));
        }
        if !is_function_word(&l.bool_true) || !is_function_word(&l.bool_false) || upper(&l.bool_true) == upper(&l.bool_false) {
            fail(&format!("{tag}: booleans must be distinct words (letters, digits, `.`, `_`)"));
        }
        if l.native_name.trim().is_empty() || l.format.general.trim().is_empty() {
            fail(&format!("{tag}: native_name and format.general must not be empty"));
        }
        // Number-format codes are read through these letters: only month and minute may share
        // one (Excel tells them apart by context), and none may collide with another.
        let f = &l.format;
        let letters = [f.year, f.month, f.day, f.hour, f.second];
        if letters.iter().enumerate().any(|(i, a)| letters.iter().skip(i + 1).any(|b| a.to_lowercase().eq(b.to_lowercase())))
            || [f.year, f.day, f.hour, f.second].iter().any(|c| c.to_lowercase().eq(f.minute.to_lowercase()))
        {
            fail(&format!("{tag}: format letters must differ (only month and minute may share one)"));
        }
        for (c, e) in &l.errors {
            if !e.starts_with('#') {
                fail(&format!("{tag}: error `{c}` → `{e}` must start with #"));
            }
        }
        // Reverse-mapped values must be unambiguous (case-insensitive), or the lookups that go
        // from local text back to canonical would silently pick the first match.
        unique_values(tag, "error", l.errors.values());
        unique_values(tag, "table item", l.table_items.iter());
        if l.table_items.iter().any(|t| !t.starts_with('#')) {
            fail(&format!("{tag}: table items must start with #"));
        }
        unique_keywords(tag, "CELL type", &l.cell_types);
        unique_keywords(tag, "INFO type", &l.info_types);
        // Local names: unique, lexable, distinct from literals.
        let reserved: Vec<String> = [upper(&l.bool_true), upper(&l.bool_false)].into_iter().chain(l.errors.values().map(|e| upper(e))).collect();
        let mut by_local: HashMap<String, String> = HashMap::new();
        let entries = functions
            .iter()
            .map(|(c, loc)| (c.clone(), loc.clone(), "function"))
            .chain(l.aliases.iter().map(|(loc, c)| (c.clone(), loc.clone(), "alias")));
        for (canonical, local, what) in entries {
            if !en_functions.contains_key(&canonical) {
                fail(&format!("{tag}: {what} `{local}` maps to unknown function `{canonical}`"));
            }
            if !is_function_word(&local) {
                fail(&format!("{tag}: {what} `{local}` ({canonical}) is not a valid function name"));
            }
            let u = upper(&local);
            // TRUE() and FALSE() are spelled like the literals; any other overlap is a data error.
            let literal_function = (canonical == "TRUE" && u == upper(&l.bool_true)) || (canonical == "FALSE" && u == upper(&l.bool_false));
            if reserved.contains(&u) && !literal_function {
                fail(&format!("{tag}: {what} `{local}` collides with a boolean or error literal"));
            }
            if let Some(prev) = by_local.get(&u)
                && *prev != canonical
            {
                fail(&format!("{tag}: local name `{local}` is used by both {prev} and {canonical}"));
            }
            by_local.insert(u, canonical);
        }
        let mut by_local: Vec<(String, String)> = by_local.into_iter().collect();
        by_local.sort_by(|a, b| a.0.chars().cmp(b.0.chars()));
        let functions: Vec<(String, String)> = functions.into_iter().collect();
        let errors: Vec<(String, String)> = l.errors.clone().into_iter().collect();
        let cell: Vec<(String, String)> = l.cell_types.clone().into_iter().collect();
        let info: Vec<(String, String)> = l.info_types.clone().into_iter().collect();
        let content: Vec<(String, String)> = l.content.clone().into_iter().collect();
        let f = &l.format;
        let name = format!("LANG_{}", ident(tag));
        let _ = writeln!(
            out,
            "pub(crate) static {name}: Language = Language {{ tag: {}, native_name: {}, dbcs: {}, bool_true: {}, bool_false: {}, r1c1: [{:?}, {:?}], table_items: [{}], errors: {}, format: FormatWords {{ year: {:?}, month: {:?}, day: {:?}, hour: {:?}, minute: {:?}, second: {:?}, general: {} }}, functions: {}, by_local: {}, gaps: &[{}], cell_types: {}, info_types: {}, content: {} }};",
            lit(tag),
            lit(&l.native_name),
            l.dbcs,
            lit(&l.bool_true),
            lit(&l.bool_false),
            r1c1.first().copied().unwrap_or('R'),
            r1c1.get(1).copied().unwrap_or('C'),
            l.table_items.iter().map(|s| lit(s)).collect::<Vec<_>>().join(", "),
            pairs(&errors),
            f.year,
            f.month,
            f.day,
            f.hour,
            f.minute,
            f.second,
            lit(&f.general),
            pairs(&functions),
            pairs(&by_local),
            l.gaps.iter().map(|s| lit(s)).collect::<Vec<_>>().join(", "),
            pairs(&cell),
            pairs(&info),
            pairs(&content),
        );
        statics.push(name);
    }
    let _ = writeln!(
        out,
        "/// Every language, en-US first.\npub static LANGUAGES: &[&Language] = &[{}];",
        statics.iter().map(|s| format!("&{s}")).collect::<Vec<_>>().join(", ")
    );

    // ---------------------------------------------------------------- regions
    let mut regions: Vec<RegionFile> = Vec::new();
    for path in read_dir_sorted(&root.join("regions")) {
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
        let r: RegionFile = toml::from_str(&text).unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
        if path.file_stem().and_then(|s| s.to_str()) != Some(r.tag.as_str()) {
            fail(&format!("{}: file name must match tag `{}`", path.display(), r.tag));
        }
        regions.push(r);
    }
    regions.sort_by_key(|r| (r.tag != "en-US", r.tag.clone()));
    if regions.first().is_none_or(|r| r.tag != "en-US") {
        fail("data/regions/en-US.toml is required");
    }
    let mut rnames = Vec::new();
    let mut lcids = Vec::new();
    // The engine builds a locale from a language and the region of the same tag.
    let lang_tags: Vec<&str> = langs.iter().map(|l| l.tag.as_str()).collect();
    let region_tags: Vec<&str> = regions.iter().map(|r| r.tag.as_str()).collect();
    if lang_tags != region_tags {
        fail(&format!("languages {lang_tags:?} and regions {region_tags:?} must have the same tags"));
    }
    if regions.first().is_some_and(|r| (r.decimal, r.group, r.list, r.array_col, r.array_row) != ('.', ',', ',', ',', ';')) {
        fail("en-US region: the canonical separators are `.` decimal, `,` group, `,` list, `,` array columns, `;` array rows");
    }
    for r in &regions {
        let tag = &r.tag;
        if r.provenance.is_empty() || r.native_name.trim().is_empty() {
            fail(&format!("{tag}: a [provenance] table and a native_name are required"));
        }
        if r.decimal == r.list || r.decimal == r.group || r.array_col == r.decimal || r.array_col == r.array_row || r.array_row == r.decimal {
            fail(&format!("{tag}: conflicting separators"));
        }
        if r.months.len() != 12 || r.months_abbr.len() != 12 || r.weekdays.len() != 7 || r.weekdays_abbr.len() != 7 || r.am_pm.len() != 2 {
            fail(&format!("{tag}: needs 12 months, 7 weekdays and 2 AM/PM designators"));
        }
        if lcids.contains(&r.lcid) {
            fail(&format!("{tag}: duplicate lcid {}", r.lcid));
        }
        lcids.push(r.lcid);
        let order = match r.date_order.as_str() {
            "MDY" => "Mdy",
            "DMY" => "Dmy",
            "YMD" => "Ymd",
            o => fail(&format!("{tag}: date_order `{o}` (MDY, DMY or YMD)")),
        };
        let list = |v: &[String]| v.iter().map(|s| lit(s)).collect::<Vec<_>>().join(", ");
        let name = format!("REGION_{}", ident(tag));
        let _ = writeln!(
            out,
            "pub(crate) const {name}: Regional = Regional {{ tag: {}, native_name: {}, lcid: {}, decimal: {:?}, group: {:?}, list: {:?}, array_col: {:?}, array_row: {:?}, date_order: DateOrder::{order}, date_sep: {:?}, time_sep: {:?}, short_date: {}, long_date: {}, short_time: {}, long_time: {}, months: [{}], months_abbr: [{}], weekdays: [{}], weekdays_abbr: [{}], am_pm: [{}], currency: {}, currency_format: {} }};",
            lit(tag),
            lit(&r.native_name),
            r.lcid,
            r.decimal,
            r.group,
            r.list,
            r.array_col,
            r.array_row,
            r.date_sep,
            r.time_sep,
            lit(&r.short_date),
            lit(&r.long_date),
            lit(&r.short_time),
            lit(&r.long_time),
            list(&r.months),
            list(&r.months_abbr),
            list(&r.weekdays),
            list(&r.weekdays_abbr),
            list(&r.am_pm),
            lit(&r.currency),
            lit(&r.currency_format),
        );
        rnames.push(name);
    }
    let _ = writeln!(
        out,
        "/// Every region, en-US first.\npub static REGIONS: &[&Regional] = &[{}];",
        rnames.iter().map(|s| format!("&{s}")).collect::<Vec<_>>().join(", ")
    );

    let dest = Path::new(&std::env::var("OUT_DIR").unwrap_or_else(|_| fail("OUT_DIR not set"))).join("tables.rs");
    std::fs::write(&dest, out).unwrap_or_else(|e| fail(&format!("{}: {e}", dest.display())));
}
