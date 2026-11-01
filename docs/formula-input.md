# Formula language

View → Interface language is the existing, persisted language preference. Choose **Español**
for Spanish formula entry and display. On first run the preference follows the system language.
Choose **Русский** for Russian formula entry and display. English and the remaining interface
languages currently use the English formula table and separators.

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

## Russian

Russian uses the same decimal comma, semicolon arguments/reference unions and backslash array
columns as Spanish. The 199-name table in `crates/formula/src/input/ru.rs`, adapted from #1850,
includes `СУММ`, `СРЗНАЧ`, `ЕСЛИ`, `ВПР`, `СЧЁТЕСЛИ`, `ПРОСМОТРX` and `ПОИСКПОЗX` (the final
`X` in the last two is Latin). Unlisted functions retain their English names. English function
names are also accepted with Russian separators. Alias lookup ignores case and accepts `е` for
`ё`; that extra equivalence does not apply to workbook names or LET/LAMBDA bindings.

`=ЕСЛИ(ИСТИНА;СУММ(1,5;2,25);0)` is stored as `IF(TRUE,SUM(1.5,2.25),0)`.
Russian formula literals include `#ДЕЛ/0!`, `#ЗНАЧ!`, `#ССЫЛКА!`, `#ИМЯ?`, `#Н/Д`,
`#ПЕРЕНОС!` and `#ВЫЧИС!`; canonical English errors remain accepted. Error constants and
ИСТИНА/ЛОЖЬ also work as direct cell input. Literal text stays text during an unchanged edit.
The grid, formula bar and editor localize error values; the engine, API and XLSX retain canonical
error values and formula syntax. The internal circular-reference marker remains `#CIRC!`.

Function descriptions for 23 common functions and argument labels live in
`crates/ui-egui/src/i18n/ru_functions.rs`, keyed by the current English catalog. Signatures are
assembled from the canonical metadata, preserving optional arguments. Missing help falls back
to English. Regional number/date display and localized syntax in advanced dialogs remain
separate work.

## Adding another language

Formula syntax is described by `LocaleData` in `crates/formula/src/input.rs`, with each locale's data in `input/<language>.rs`: function aliases,
error spellings, the decimal/list/array-column/array-row separators, and the alias case-folding
function. Add the locale's data and enum selection there, then connect the interface language
in `Language::formula_locale`. The lexer, parser, printer, completion and value editor consume
the shared methods; they must not gain per-language branches. Alias folding is separate from
`same_name`, which defines workbook and LET/LAMBDA name identity.

Interface translations remain in `i18n/<language>.rs`. Optional function descriptions and
argument names use the existing English-keyed `<language>_functions.rs` tables. Ribbon tab
names can use `Tab|name`; `Language::tr_tab` falls back to the ordinary label translation.

Dynamic error matching is shared in `i18n/error_templates.rs`. To translate an existing message
shape, add its template key to the language catalog. A new engine message shape needs one shared
template and a regression test generated through the real command/parser boundary. Placeholders
are substituted once, preserving braces and other text inside user-provided data. Built-in
function names and signatures use that language's existing formula/help tables. Unknown
messages or untranslated templates retain the original text.

Every new locale should test alias collisions, case rules, defined/LET/LAMBDA names, all four
separators, error literals, Unicode editing, and UI/XLSX round trips. Changes to shared machinery
must keep the existing English and Spanish cases passing too.
