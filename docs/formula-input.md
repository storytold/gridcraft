# Formula language

View → Interface language is the existing, persisted language preference. Choose **Español** or
**Português (Brasil)** for localized formula entry and display. On first run the preference follows
the system language. English and languages without a formula table use English names and separators.

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

Brazilian Portuguese covers all 514 canonical function names currently recognized by GridCraft.
Names that Excel keeps unchanged, such as `ABS`, `COS`, `LET` and `LAMBDA`, remain unchanged.

| Brazilian Portuguese | Canonical English |
| --- | --- |
| `SOMA` | `SUM` |
| `MÉDIA` | `AVERAGE` |
| `SE` | `IF` |
| `CONT.NÚM` | `COUNT` |
| `SOMASE`, `SOMASES` | `SUMIF`, `SUMIFS` |
| `CONT.SE`, `CONT.SES` | `COUNTIF`, `COUNTIFS` |
| `PROCV`, `PROCX` | `VLOOKUP`, `XLOOKUP` |
| `HOJE` | `TODAY` |
| `VERDADEIRO`, `FALSO` | `TRUE`, `FALSE` (literals or function calls) |

Names are case-insensitive. Spanish and Brazilian Portuguese formulas use decimal commas (`1,5`), semicolon arguments
and reference unions, and backslashes between array columns (`{1\2;3\4}`). For example,
`=SI(VERDADERO;SUMA(1,5;2,25);0)` and `=SE(VERDADEIRO;SOMA(1,5;2,25);0)` evaluate to 3.75.
English mode retains comma arguments, decimal points and comma array columns; it does not silently
translate localized aliases.

The cell editor, formula bar, Show Formulas, autocomplete, the Formula ribbon, Insert Function, and argument hints
use the selected formula language. Common Spanish function help is translated; Portuguese
signatures show the localized function name while parameter names and descriptions currently
fall back to English. General interface labels and errors
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
