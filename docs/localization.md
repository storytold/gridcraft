# Adding an interface language

GridCraft's interface is written in English at its call sites, and every string that reaches the
screen goes through a key. **English is the source language**: there is no English catalog, because
`tr` returns the key itself. A language is a catalog that answers those keys, and the catalog is a
module named after the language's code — a folder per language, one file when the language fits in
one.

```
crates/ui-egui/src/i18n/
    rs                          the Language enum, tr, tr_plural
    plural.rs                   plural rules for every language, and their English wordings
    pseudo.rs                   the pseudo-locale: the interface accented, for finding misses
    ru.rs                       one language: a file, because it fits in one
    kk/
        mod.rs                  one language: a folder, when it needs several files
        functions.rs            (the same split es/es_functions already uses)
    _template/                  a copy of the shape a new language starts from
```

## The six steps

1. **Choose the code.** The ISO 639-1 code (`de`, `uk`, `kk`); `pt` is Brazilian Portuguese. That
   code is what `ui.json` saves and what `app.language.set` takes.
2. **Add the variant** in `crates/ui-egui/src/i18n.rs`: `Language`, its place in `Language::ALL`,
   `name` (the language's own name), `code`, `from_tag`, and one arm of `Language::tr`. Adding a
   variant is a compile error in every `match` that has to answer for it, which is the point.
3. **Copy `i18n/_template/`** to `i18n/<code>` (or `i18n/<code>.rs`) and add `mod <code>;` to
   `i18n.rs`, in alphabetical order with the others.
4. **Translate `translate`.** The key is the English text exactly as the interface shows it;
   `cargo xtask i18n-report` prints the strings the code asks for and the ones this catalog does
   not answer, so the work is a list, not a hunt. Leaving a string out is allowed at any point: the
   interface shows English there.
5. **Write the counted messages** in `plural`, for the categories the language uses (see below).
6. **Run the checks**: `cargo test -p gridcraft-ui-egui`, `cargo clippy -p gridcraft-ui-egui
   --all-targets -- -D warnings`, `cargo xtask i18n-report`. Then look at the interface with the
   language chosen, and with the pseudo-locale, which accents every string that went through a key
   and leaves plainly English words as they are.

## Keys

- A key is the English label: `tr("Cancel")`, `tr("Saved to {path}")`.
- **Slots** are named and stay as they are: `{path}`, `{n}`, `{name}`, `{error}`. Their contents are
  filled after translation, so word order is yours; the name is not.
- A label that means two things — the Home tab's "Orientation" and Page Layout's — is keyed
  `Home|Orientation`; only the part after `|` is shown, and only that part is translated.
- A whole error message with a template of its own lives in `i18n/error_templates.rs`.

## Counted messages

A message that counts something takes its count:

```rust
lang.tr_plural(n, "{n} step")            // 1 step / 2 steps / 5 steps
lang.tr_plural(cells, "Check passed: writes {n} cell")
```

`tr_plural` asks `plural::category` which plural category the count falls in for the language, then
looks for the wording in the language's `plural` table, then in the English wordings in
`plural.rs`, and fills `{n}`. Slots other than `{n}` are left for the call site.

The categories are the CLDR ones. How many forms a language needs:

| Forms | Languages |
|---|---|
| 1 (`other`) | Japanese, Chinese, Korean, Vietnamese |
| 2 (`one`, `other`) | English, German, Dutch, Greek, Turkish, Spanish, Italian, Portuguese, French |
| 3 (`one`, `few`, `many`/`other`) | Russian, Ukrainian, Polish, Czech, Serbian, Romanian |
| more | Arabic (6), Welsh (6), Irish (5), Latvian (3, but `zero` instead of `many`), Hebrew (2 with its own rule) |

French, Portuguese, Spanish and Italian also word an exact million differently ("1 million de
pages"); Russian and Ukrainian word a fraction as `few` ("1,5 шага"). Counts in the interface are
whole, so those forms are named but unreached today.

Every counted key the interface has is listed in `plural::PLURALS`. The Russian catalog words all
of them, and its tests fail when a key is added to the list without a Russian form — that is the
guard against a new count shipping in English everywhere.

## What is not translated

Command ids (`home.bold`), function names (`SUM`, `VLOOKUP`), formula syntax, sheet and file names,
paths, URLs, font names, keyboard key names as they appear on the keycaps, glyphs, and LaTeX in the
equation editor. The command *labels* are translated; their ids are not.

## The pseudo-locale

`app.language.set {"language": "pseudo"}` switches the interface to `pseudo.rs`: every string that
went through a key comes back accented, padded and bracketed (`⟦Çáñçéļ···⟧`), and every string that
did not stays plain English. It is not offered in the switcher. Checking a screen in it finds the
hardcoded words that no catalog can reach.
