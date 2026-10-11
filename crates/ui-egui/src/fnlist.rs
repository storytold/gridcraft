//! The function list as the interface shows it: the engine's English metadata plus the names,
//! descriptions and parameter lists of the active language.
//!
//! Names come in two spellings. The *canonical* name (`SUM`) is what the engine and files use; the
//! *local* name (`SOMA` in pt-BR) is what the formula language accepts, so the editor, the
//! autocomplete and the Insert Function dialog only ever show and match local names.

use std::sync::LazyLock;

use gridcraft_locale::Locale;
use serde_json::Value as Json;

use crate::l10n::{Localizer, Tr};

/// One function of the engine's list.
#[derive(Clone, Debug, PartialEq)]
pub struct FunctionInfo {
    /// Canonical (English) name.
    pub name: String,
    /// Category id as the engine names it (`MathTrig`, `DateTime`, ...).
    pub category: String,
    /// English signature, `NAME(arg1, [arg2], ...)`.
    pub signature: String,
    /// English description.
    pub description: String,
}

fn text(f: &Json, key: &str) -> String {
    f.get(key).and_then(Json::as_str).unwrap_or_default().to_string()
}

/// Every function the engine lists, sorted as the engine lists them.
pub fn all() -> &'static [FunctionInfo] {
    static LIST: LazyLock<Vec<FunctionInfo>> = LazyLock::new(|| {
        gridcraft_engine::cmd::formulas::function_list()
            .iter()
            .map(|f| FunctionInfo {
                name: text(f, "name"),
                category: text(f, "category"),
                signature: text(f, "signature"),
                description: text(f, "description"),
            })
            .filter(|f| !f.name.is_empty())
            .collect()
    });
    &LIST
}

/// The function with canonical name `name` (case-insensitive).
pub fn find(name: &str) -> Option<&'static FunctionInfo> {
    all().iter().find(|f| f.name.eq_ignore_ascii_case(name))
}

/// The name of `f` in the formula language of `loc`; functions the language does not translate
/// keep their canonical name.
pub fn local_name(loc: &Locale, f: &FunctionInfo) -> String {
    loc.formula.local_function(&f.name).unwrap_or(&f.name).to_string()
}

/// The local names of all listed functions.
pub fn local_names(loc: &Locale) -> Vec<String> {
    all().iter().map(|f| local_name(loc, f)).collect()
}

/// The function a name typed in the formula language `loc` stands for. Only local names count: a
/// canonical English name in a localized formula is not a function, as in Excel.
pub fn resolve(loc: &Locale, typed: &str) -> Option<&'static FunctionInfo> {
    loc.formula.canonical_function(typed).and_then(find)
}

/// Short description in the interface language, the engine's English text when the language has
/// none.
pub fn description(l: &Localizer, f: &FunctionInfo) -> String {
    l.function_description(&f.name).map_or_else(|| f.description.clone(), |d| d.into_owned())
}

/// Parameter list in the interface language, in signature order (`[parameter]` keeps its brackets,
/// `...` marks repetition). Falls back to the English signature.
pub fn params(l: &Localizer, f: &FunctionInfo) -> Vec<String> {
    let local = l.function_args(&f.name);
    if !local.is_empty() {
        return local;
    }
    let inner = f.signature.split_once('(').map_or("", |(_, rest)| rest.trim_end_matches(')'));
    gridcraft_l10n::split_args(inner)
}

/// Signature as shown in the interface: local name, then the parameters in the interface
/// language separated by the region's list separator, `SOMA(núm1; [núm2]; ...)`.
pub fn signature(l: &Localizer, loc: &Locale, f: &FunctionInfo) -> String {
    let sep = format!("{} ", loc.regional.list);
    format!("{}({})", local_name(loc, f), params(l, f).join(&sep))
}

/// Display name of a category id of the engine's list.
pub fn category_label(l: &Localizer, category: &str) -> String {
    let key = format!("ui-fn-category-{category}");
    l.get(&key, &[]).map_or_else(|| category.to_string(), |c| c.into_owned())
}

/// Category ids in the order Insert Function offers them.
pub const CATEGORIES: &[&str] = &[
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
];

/// Label for the "all categories" choice of the Insert Function dialog.
pub fn all_categories(l: &Localizer) -> String {
    l.tr("All").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_in_the_formula_language_only() {
        let pt = gridcraft_locale::language("pt-BR").expect("pt-BR");
        let region = gridcraft_locale::region("pt-BR").expect("region");
        let loc = Locale::new(pt, pt, *region);
        assert_eq!(resolve(&loc, "SOMA").map(|f| f.name.as_str()), Some("SUM"));
        assert!(resolve(&loc, "SUM").is_none());
        let sum = find("sum").expect("SUM is listed");
        assert_eq!(local_name(&loc, sum), "SOMA");
        assert_eq!(local_name(&Locale::default(), sum), "SUM");
    }

    #[test]
    fn signatures_use_the_dialect_separator() {
        let pt = gridcraft_locale::language("pt-BR").expect("pt-BR");
        let region = gridcraft_locale::region("pt-BR").expect("region");
        let loc = Locale::new(pt, pt, *region);
        let l = Localizer::new("pt-BR");
        let f = find("IF").expect("IF is listed");
        let s = signature(&l, &loc, f);
        assert!(s.starts_with("SE("), "{s}");
        assert!(s.contains("; "), "{s}");
        let en = signature(&Localizer::new("en-US"), &Locale::default(), f);
        assert_eq!(en, f.signature);
    }

    #[test]
    fn every_listed_category_has_a_label_in_every_language() {
        for tag in gridcraft_l10n::available_languages() {
            let l = Localizer::new(tag);
            for c in CATEGORIES {
                assert!(l.has(&format!("ui-fn-category-{c}")), "{tag}: missing category {c}");
            }
        }
        for f in all() {
            assert!(CATEGORIES.contains(&f.category.as_str()), "category {} of {} is not in CATEGORIES", f.category, f.name);
        }
    }
}
