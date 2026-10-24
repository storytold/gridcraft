# Formula language

View → Interface language is the existing, persisted language preference. Choose **Español**
for Spanish formula entry and display, or **Français** for French formulas. On first run the preference follows the system language.
English and the other interface languages currently use the English formula table and separators.

French covers all 514 registered function names (including evaluator special forms and names
that Excel leaves unchanged). The table lives in `crates/formula/src/input/french.rs`.
For example, `=SI(VRAI;SOMME(1,5;2,25);0)` evaluates to 3.75. Function names follow Excel
French spellings, including `RECHERCHEX`, `NB.SI.ENS`, `ECARTYPE.PEARSON` and `AUJOURDHUI`.
French uses the same separators as Spanish below. Function descriptions and parameter names
keep their existing English fallback; this change translates function names and formula syntax.

Last updated: 2026-10-23 (correct conditional extrema names and French formula error literals).

`MAXIFS` and `MINIFS` use Excel's `MAX.SI.ENS` and `MIN.SI.ENS` names. Formula error
literals also use French spellings (`#VALEUR!`, `#NOM?`, `#NOMBRE!`, `#NUL!`,
`#PROPAGATION!`, `#CHARGEMENT_DONNEES`). English error spellings remain accepted in French
formulas; stored expressions and XLSX remain English. Quoted text and sheet names are unchanged.
These corrections are adapted from [PR #2310](https://github.com/storytold/gridcraft/pull/2310).
Its `crates/engine/tests/french_formula_names.rs` checks that every registered function and
special form has a table row, and that canonical and French names are unique. It checks
coverage and ambiguity, not the correctness of the French spelling.

| Spanish | Canonical English |
| --- | --- |
| `SUMA` | `SUM` |
| `PROMEDIO` | `AVERAGE` |
| `SI` | `IF` |
| `CONTAR` | `COUNT` |
| `SUMAR.SI` | `SUMIF` |
| `CONTAR.SI` | `COUNTIF` |
| `BUSCARV` | `VLOOKUP` |
| `HOY` | `TODAY` |
| `VERDADERO`, `FALSO` | `TRUE`, `FALSE` (literals or function calls) |

Names are case-insensitive. Spanish formulas use decimal commas (`1,5`), semicolon arguments
and reference unions, and backslashes between array columns (`{1\2;3\4}`). For example,
`=SI(VERDADERO;SUMA(1,5;2,25);0)` evaluates to 3.75. English mode retains comma arguments,
decimal points and comma array columns; it does not silently translate Spanish aliases.

The cell editor, formula bar, Show Formulas, autocomplete, the Formula ribbon, Insert Function, and argument hints
use the selected formula language. Common Spanish function help is translated; functions
without a Spanish entry retain their English names/help. General interface labels and errors
fall back to English where Spanish translations are not provided. An edit captures its language
when it starts so switching the preference cannot reinterpret unfinished input.

Workbook/sheet names, table names, LET bindings and LAMBDA parameters take precedence over
aliases. Text, sheet names and table column names are unchanged. When a localized spelling
would collide with a name, displayed formulas retain the canonical spelling to preserve meaning.

Stored formulas, XLSX import/export, engine commands, control/API calls, CLI evaluation and
clipboard exchange retain strict English syntax. This also applies to advanced dialogs such as
Name Manager and conditional-format rules. Localization happens at the cell/formula-bar editor
boundary, including filling the selected range with Ctrl+Enter; it does not change the workbook
format or teach the calculation engine ambiguous aliases.
