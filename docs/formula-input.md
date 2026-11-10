# Formula language

View → Interface language is the existing, persisted language preference. Choose **Español**
for Spanish formula entry and display. On first run the preference follows the system language.
English and the other interface languages currently use the English formula table and separators.

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

## Text read as a number

Arithmetic on text, `VALUE`, criteria and Paste Special's operations (Add, Multiply…) read numbers
the way the system's region writes them, as Excel does. On Windows the desktop app takes the decimal
and group separators and the currency from the regional format settings, which can differ from the
display language; elsewhere from the system's language and region tag, through the regions GridCraft
knows. With a decimal comma `"1,5"*1` is
1.5, `"1.000,5"*1` is 1000.5, `"1,0000"*1` is 1, and `"1.5"*1` and `"$1,5"*1` are `#VALUE!`. So a
workbook relying on such text can calculate differently on another system, also as in Excel.
The CLI and the engine's commands always read a decimal point, as do `NUMBERVALUE`, filters and
typed entry. Dates in text aren't affected: they still read month first.
