# Formula language

View → Interface language is the existing, persisted language preference, and it also picks the
formula language, as in Excel: function names follow the display language. On first run the
preference follows the system language. Interface languages not listed below use English
formulas.

| Interface | Function names (e.g.) | Local names | Booleans | Errors (e.g.) | Arguments / decimal | Array columns / rows | Table specifiers (e.g.) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| English | `SUM`, `IF`, `VLOOKUP` | — | `TRUE`, `FALSE` | `#VALUE!`, `#N/A` | `,` / `.` | `,` / `;` | `#Headers`, `#This Row` |
| Español | `SUMA`, `SI`, `BUSCARV` | 10 | `VERDADERO`, `FALSO` | `#¡VALOR!`, `#N/D` | `;` / `,` | `\` / `;` | `#Encabezados`, `#Esta fila` |

The Spanish function names:

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
| `#¡VALOR!`, `#N/D`, `#¿NOMBRE?`… | `#VALUE!`, `#N/A`, `#NAME?`… |
| `[#Todo]`, `[#Datos]`, `[#Encabezados]`, `[#Totales]`, `[#Esta fila]` | `[#All]`, `[#Data]`, `[#Headers]`, `[#Totals]`, `[#This Row]` |

Names, booleans, errors and table specifiers are case-insensitive in any script. Spanish formulas
use decimal commas (`1,5`), semicolon arguments and reference unions, and backslashes between
array columns (`{1\2;3\4}`). For example, `=SI(VERDADERO;SUMA(1,5;2,25);0)` evaluates to 3.75.
English mode retains comma arguments, decimal points and comma array columns; it does not
silently translate Spanish aliases. English names, booleans and errors are accepted in Spanish
too. A worksheet function passed by name (`BYROW(A1:B3;SUMA)`) is spelled like its call.

A formula language is a data file, `crates/formula/src/input/<language>.rs`: its function names,
booleans, errors, table specifiers and separators, each word with its source. Adding a language
means adding that file and one `FormulaLocale` variant; the lexer, parser and printer read every
language through the same table.

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
