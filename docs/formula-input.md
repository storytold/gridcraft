# Formula language

View → Interface language is the existing, persisted language preference. Choose **Español**
for Spanish or **Português (Brasil)** for Brazilian Portuguese formula entry and display. On first
run the preference follows the system language. English and the other interface languages
currently use the English formula table and separators.

## Brazilian Portuguese

Portuguese formulas use the Portuguese function names (`SOMA`, `MÉDIA`, `SE`, `PROCV`…),
`VERDADEIRO` and `FALSO`, decimal commas, semicolon arguments and backslashes between array
columns: `=SOMA(1;2,5)` is stored as `=SUM(1,2.5)` and shown again as `=SOMA(1;2,5)`. Names match
in any case, accents included (`média` is `MÉDIA`). A function without a Portuguese name keeps its
English one. Table specifiers are `[#Tudo]`, `[#Dados]`, `[#Cabeçalhos]`, `[#Totais]` and
`[#Esta linha]`, separated by `;` (`Tabela1[[#Cabeçalhos];[Qtd]]`). `VERDADEIRO` or `FALSO` typed
outside a formula enters the logical value. Common function help is in Portuguese.

## Spanish

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
