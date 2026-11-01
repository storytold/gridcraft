# Russian localization

This contribution builds on [#676](https://github.com/storytold/gridcraft/pull/676)
(@xmaks82, Russian interface) and [#1850](https://github.com/storytold/gridcraft/pull/1850)
(@dressky, Russian formulas). It adapts their translation data to current `main` rather than
restoring their older parser/editor/help implementations.

## Shared architecture

- `FormulaLocale` selects one `LocaleData`: function/error names, four separators and alias
  case folding. The lexer, parser, printer and editor share that data. Russian-specific alias
  folding and error spellings live alongside its function-name table.
- `Language::translation` exposes the existing catalogs without English fallback. Shared
  contextual-tab lookup and dynamic error matching use it; a language adds catalog rows rather
  than its own rendering implementation.
- Function help uses the existing English-keyed description/argument tables. Signatures come
  from canonical function metadata, keeping optional arguments and argument order consistent.
- Localization occurs at UI entry/display boundaries. Engine values, commands, clipboard and
  XLSX retain canonical English syntax. Workbook content and LET/LAMBDA name identity are not
  translated or folded as function aliases.

See [Adding another language](formula-input.md#adding-another-language) for extension points.

## Adaptation details

The Russian interface catalog incorporates #676, newer main-only entries and the previously
missing keys from the other current catalogs. New code routes Start/Save Changes text and ribbon
tab labels through the shared lookup. Coverage of catalog keys does not imply every UI literal
has already been wired to translation.

The formula table retains #1850's 199 pairs. `XLOOKUP` and `XMATCH` use a Latin final `X`
(`ПРОСМОТРX`, `ПОИСКПОЗX`), matching the public [Microsoft reference](https://support.microsoft.com/ru-ru/excel/look-up-values-with-vlookup-index-or-match).
The extra `е`/`ё` equivalence applies to aliases only. Current main's Unicode name matching and
iterative traversal of long flat formulas are retained. The table is tested in full for unique
canonical names, unique folded aliases and parse/print round trips.

Help for 23 common functions is migrated to `i18n/ru_functions.rs` using current English keys.
For example, IF's optional arguments follow the engine rather than #1850's hand-written Russian
signature. Functions without Russian help retain the English fallback.

Russian cell/formula error literals are in `input/ru.rs`. `#CALC!` is `#ВЫЧИС!`, as in the
public [LAMBDA reference](https://support.microsoft.com/ru-ru/excel/functions/lambda-function);
`#SPILL!` is `#ПЕРЕНОС!`, as in the [spill-error reference](https://support.microsoft.com/ru-ru/excel/how-to-correct-a-spill-error).
The internal circular-reference marker remains `#CIRC!`. Literal text resembling a localized
Boolean/error value keeps its type during editing.

Fixed diagnostics use the catalog. Shared dynamic templates cover selected parser, argument-count
and parameter errors; filenames, sheet names and other captured values are preserved. Unknown
system/engine messages remain unchanged. This is not a claim to translate every diagnostic.

## Remaining work

- Verify and extend the remaining built-in function names and help with native-speaker review.
- Integrate regional Russian number/date/currency formats with the shared region work.
- Translate remaining UI literals and diagnostics, coordinating with the other open localization
  PRs instead of adding competing per-language code paths.
- Localized formula syntax in advanced dialogs remains outside the cell-editor boundary.
