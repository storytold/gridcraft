//! Plural forms: which of a language's wordings a count takes.
//!
//! A message that counts something ("3 steps", "1 step") cannot be translated by looking up one
//! string: the noun, and often the verb, change with the number, and how they change is a property
//! of the language, not of the message. This module holds the two halves of that:
//!
//! * [`category`] turns a count into the language's plural category, following the CLDR plural
//!   rules for that language (public specification data, not code from any product);
//! * [`english`] gives the source wording for a category, since English is the original: the
//!   English text of a catalog key is its `one` form (that is why a key reads `{n} step`), and
//!   only the other categories are listed here.
//!
//! Every other language answers from its own catalog (see `ru::plural`) and falls back to English
//! when it has no form yet, so a partly translated language still reads correctly.
//!
//! ## How many forms a language needs
//!
//! Only the categories a rule can reach for whole counts need wording. For the languages GridCraft
//! offers today that is:
//!
//! | forms | categories | languages |
//! |-------|------------|-----------|
//! | 1 | other | Japanese, Chinese (both scripts), Korean, Vietnamese |
//! | 2 | one, other | English, German, Dutch, Swedish, Danish, Greek, Georgian, Turkish, Spanish, Italian, Portuguese, French |
//! | 3 | one, few, other | Serbian, Romanian, Lithuanian, Czech, Polish, Russian, Ukrainian |
//!
//! Russian and Ukrainian take a fourth, distinct form for a fraction ("1,5 шага"), which CLDR calls
//! `other`; Spanish, Italian, Portuguese and French likewise have a `many` form an exact million
//! takes. Both are listed where they belong and no count of whole things reaches them.
//!
//! A language may need more than four, which is why the enum names all six CLDR categories: Arabic
//! uses six (`zero one two few many other`), Welsh six, Irish five, Latvian `zero` instead of
//! `many`. None of those are offered yet; adding one means adding its rule here, its categories to
//! that language's plural table, and nothing else.

use super::Language;

/// A CLDR plural category: the six kinds of wording a language can distinguish. A language uses a
/// subset; the ones it uses are the ones its plural table lists.
///
/// `Zero` and `Two` are not reachable in any language GridCraft offers today (Arabic and Welsh are
/// the ones that need them), so they are allowed to go unconstructed here rather than left out:
/// a rule that needs them finds the name instead of a redesigned enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Category {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

/// The plural category `n` falls in for `language`, for a count of whole things.
///
/// Counts here are always whole and never negative (steps, cells, files), so the rules are written
/// for that case. A fractional count takes `other` in most languages and `few` in Russian ("1,5
/// шага"); no interface count is fractional today, and one that became so would pass
/// [`Category::Other`] from its own rule.
pub(crate) fn category(language: Language, n: u64) -> Category {
    use Category::{Few, Many, One, Other};
    let (last, last_two) = (n % 10, n % 100);
    match language {
        // No plural agreement at all: one wording for every count.
        Language::Ja | Language::Zh | Language::Ko | Language::Vi => Other,
        // 1 шаг, 2 шага, 5 шагов. 11–14 are the exception that takes the many form.
        Language::Ru | Language::Uk => {
            if last == 1 && last_two != 11 {
                One
            } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
                Few
            } else {
                Many
            }
        }
        // 1 krok, 2 kroki, 5 kroków: only an exact 1 is one, so 21 takes the few form.
        Language::Pl => {
            if n == 1 {
                One
            } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
                Few
            } else {
                Many
            }
        }
        // 1 krok, 2–4 kroky, 5 kroků.
        Language::Cs => match n {
            1 => One,
            2..=4 => Few,
            _ => Other,
        },
        // 1 fajl, 2 fajla, 5 fajlova: 5 and up share the other form.
        Language::Sr => {
            if last == 1 && last_two != 11 {
                One
            } else if (2..=4).contains(&last) && !(12..=14).contains(&last_two) {
                Few
            } else {
                Other
            }
        }
        // 1 pas, 2 pași, 20 de pași: 0 and 2–19 take the few form, so do 101–119.
        Language::Ro => {
            if n == 1 {
                One
            } else if n == 0 || (2..=19).contains(&last_two) {
                Few
            } else {
                Other
            }
        }
        // 1 page, 1,5 pages: a French or Portuguese count of 0 is singular, and an exact million
        // takes the many form ("1 million de pages").
        Language::Fr | Language::PtBr => {
            if n < 2 {
                One
            } else if n != 0 && n.is_multiple_of(1_000_000) {
                Many
            } else {
                Other
            }
        }
        // 1 página, 1 millón de páginas.
        Language::Es | Language::It => {
            if n == 1 {
                One
            } else if n != 0 && n.is_multiple_of(1_000_000) {
                Many
            } else {
                Other
            }
        }
        // English, German, Dutch, Swedish, Danish, Greek, Georgian, Turkish: singular only at 1.
        _ => {
            if n == 1 {
                One
            } else {
                Other
            }
        }
    }
}

/// The source wording of a counted message for `category`. The message's key is its English `one`
/// form, so `one` (and any other category English does not distinguish) answers `None` and the key
/// is used as it stands.
pub(super) fn english(key: &str, category: Category) -> Option<&'static str> {
    Some(match (key, category) {
        ("All done. We made {n} replacement.", Category::Other) => "All done. We made {n} replacements.",
        ("{name}    (script, {n} step)", Category::Other) => "{name}    (script, {n} steps)",
        (", {n} of them holds data (red)", Category::Other) => ", {n} of them hold data (red)",
        ("{n} step", Category::Other) => "{n} steps",
        ("Saved macro “{name}” ({n} step)", Category::Other) => "Saved macro “{name}” ({n} steps)",
        ("Ran macro “{name}” ({n} step)", Category::Other) => "Ran macro “{name}” ({n} steps)",
        ("Ran {n} step.", Category::Other) => "Ran {n} steps.",
        ("Check passed: writes {n} cell", Category::Other) => "Check passed: writes {n} cells",
        ("Finished: {n} step ran, one undo step.", Category::Other) => "Finished: {n} steps ran, one undo step.",
        ("Stopped after {n} step; the changes were kept.", Category::Other) => "Stopped after {n} steps; the changes were kept.",
        ("{n} cell found", Category::Other) => "{n} cells found",
        ("{n} item in this file was handled differently. First: {warning}", Category::Other) => {
            "{n} items in this file were handled differently. First: {warning}"
        }
        _ => return None,
    })
}

/// Every counted message the interface has: the English `one` wording with an `{n}` slot, which is
/// the key [`super::Language::tr_plural`] takes. A language whose forms are written keeps its
/// wording in its own catalog's plural table (Russian's is `ru::plural`), and the guard test in
/// `ru.rs` fails when a key here has no form there, so a new counted message cannot ship with a
/// silent English fallback in the one catalog that is written out.
#[cfg(test)]
pub(super) const PLURALS: &[&str] = &[
    ", {n} of them holds data (red)",
    "All done. We made {n} replacement.",
    "Check passed: writes {n} cell",
    "Finished: {n} step ran, one undo step.",
    "Ran macro \u{201c}{name}\u{201d} ({n} step)",
    "Saved macro \u{201c}{name}\u{201d} ({n} step)",
    "Stopped after {n} step; the changes were kept.",
    "{name}    (script, {n} step)",
    "{n} cell found",
    "{n} item in this file was handled differently. First: {warning}",
    "{n} step",
    "Ran {n} step.",
];

#[cfg(test)]
mod tests {
    use super::{Category, Language, category};
    use Category::{Few, Many, One, Other};

    /// The categories counts reach, in the order they first do: every small count, then the ones
    /// a language has a form of its own for (a million, which French, Portuguese, Spanish and
    /// Italian word as `many`).
    fn seen(language: Language) -> Vec<Category> {
        let mut out = Vec::new();
        for n in (0..=1_000u64).chain([1_000_000, 2_000_000, 1_000_001]) {
            let c = category(language, n);
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// The counts at which each language changes its wording, as its speakers write them.
    #[test]
    fn counts_take_the_form_the_language_uses() {
        let cases: &[(Language, &[(u64, Category)])] = &[
            (Language::En, &[(0, Other), (1, One), (2, Other), (1_000_000, Other)]),
            (Language::Ru, &[(0, Many), (1, One), (2, Few), (4, Few), (5, Many), (11, Many), (21, One), (22, Few), (111, Many)]),
            (Language::Uk, &[(1, One), (3, Few), (13, Many), (104, Few)]),
            (Language::Pl, &[(1, One), (2, Few), (5, Many), (21, Many), (22, Few), (112, Many)]),
            (Language::Cs, &[(1, One), (3, Few), (5, Other), (21, Other)]),
            (Language::Sr, &[(1, One), (4, Few), (5, Other), (21, One)]),
            (Language::Ro, &[(0, Few), (1, One), (19, Few), (20, Other), (101, Other), (119, Few)]),
            (Language::Fr, &[(0, One), (1, One), (2, Other), (1_000_000, Many)]),
            (Language::Es, &[(0, Other), (1, One), (2, Other), (2_000_000, Many)]),
            (Language::Ja, &[(0, Other), (1, Other), (7, Other)]),
        ];
        for &(language, counts) in cases {
            for &(n, want) in counts {
                assert_eq!(category(language, n), want, "{} with {n}", language.code());
            }
        }
    }

    /// Every language GridCraft offers has a rule, and reaches only as many categories as it has
    /// wording for.
    #[test]
    fn every_offered_language_reaches_the_expected_number_of_forms() {
        for language in Language::ALL {
            let categories = seen(language);
            let expected = match language {
                Language::Ja | Language::Zh | Language::Ko | Language::Vi => 1,
                // Serbian, Romanian, Lithuanian, Czech, Polish, Russian and Ukrainian word 1, 2–4
                // and 5+ differently; Russian and Ukrainian add a fraction form that no whole count
                // reaches, so three is what the counts above show.
                Language::Sr | Language::Ro | Language::Cs | Language::Pl | Language::Ru | Language::Uk => 3,
                // French, Portuguese, Spanish and Italian additionally have the million form.
                Language::Fr | Language::Es | Language::It | Language::PtBr => 3,
                _ => 2,
            };
            assert_eq!(categories.len(), expected, "{}: {categories:?}", language.code());
        }
    }

    /// The key is the English singular, so English answers only for the other categories.
    #[test]
    fn english_leaves_the_singular_to_the_key() {
        assert_eq!(super::english("{n} step", One), None);
        assert_eq!(super::english("{n} step", Other), Some("{n} steps"));
        assert_eq!(super::english("{n} cell found", Few), None);
        assert_eq!(super::english("{n} cell found", Other), Some("{n} cells found"));
    }
}
