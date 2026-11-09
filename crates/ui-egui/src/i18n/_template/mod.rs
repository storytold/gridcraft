//! Template for a language catalog. Copy this folder, name it after the language's ISO 639-1 code
//! (`i18n/de/` or the single file `i18n/de.rs` for a language that fits in one file), then wire it
//! in `i18n.rs`: a `Language` variant, its place in `Language::ALL`, `name`, `code`, `from_tag` and
//! one arm of `Language::tr`. `docs/localization.md` has the steps in order.
//!
//! Three rules hold for every catalog:
//!
//! - The key is the English text as the interface shows it, and translations keep every `{slot}`
//!   spelled the same. English is the source language: there is no English catalog, `tr` returns
//!   the key itself, so a variant of English is never written out.
//! - A label that means two things in the interface is keyed `context|label`; only the part after
//!   the `|` is shown. Translate the label, not the context.
//! - A counted message is not translated here but in [`plural`], one wording per plural category
//!   the language uses: 1 step, 2 steps is one key, not two.
//!
//! Copy nothing from another product: these strings are the interface's own, and a translation is
//! written from the English meaning, not from another spreadsheet's wording.

/// Every interface string this language translates, keyed by its English text.
///
/// Leaving a string out is allowed while a catalog is being built: the interface shows the English
/// there. `cargo xtask i18n-report` lists what the code asks for and this does not answer.
pub(super) fn translate(english: &str) -> Option<&'static str> {
    Some(match english {
        "Cancel" => "TODO: Cancel",
        "OK" => "TODO: OK",
        // Labels that need two translations carry their context before the `|`.
        "Home|Orientation" => "TODO: Orientation on the Home tab",
        _ => return None,
    })
}

/// The wording of a counted message per plural category, for a language that words counts
/// differently from English. A language with one form for every count answers `None` and the
/// English wording is used; a language with two forms is `One` and `Other`.
///
/// The categories are the CLDR ones, so the languages that need `Zero`, `Two` or `Few` write them
/// here without a change to the engine.
#[allow(dead_code)]
pub(super) fn plural(key: &str, category: super::plural::Category) -> Option<&'static str> {
    use super::plural::Category::{Few, Many, One, Other};
    Some(match (key, category) {
        ("{n} step", One) => "{n} TODO: step",
        ("{n} step", Few | Other) => "{n} TODO: steps",
        ("{n} step", Many) => "{n} TODO: many steps",
        _ => return None,
    })
}
