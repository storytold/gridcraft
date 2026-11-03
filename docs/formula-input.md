# Formula language

View → Interface language is the existing, persisted language preference. Choose **Español**
for Spanish formula entry and display. On first run the preference follows the system language.
Choose **Português (Brasil)** for Brazilian Portuguese formula entry and display (see below).
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

## Brazilian Portuguese

**Português (Brasil)** uses the Brazilian function names below with the same separators as
Spanish: decimal commas, `;` between arguments and `\` between array columns. For example,
`=SE(VERDADEIRO;SOMA(1,5;2,25);0)` evaluates to 3.75. Accented names match in any case
(`média` is `MÉDIA`). Spanish aliases such as `SUMA` are not read in Portuguese, and English
names (`SUM`) keep working with Portuguese separators.

| Brazilian Portuguese | Canonical English |
| -------------------- | ----------------- |
| `SOMA`, `SOMASE`, `SOMASES`, `SOMARPRODUTO` | `SUM`, `SUMIF`, `SUMIFS`, `SUMPRODUCT` |
| `MÉDIA`, `MÁXIMO`, `MÍNIMO` | `AVERAGE`, `MAX`, `MIN` |
| `CONT.NÚM`, `CONT.VALORES`, `CONT.SE`, `CONT.SES` | `COUNT`, `COUNTA`, `COUNTIF`, `COUNTIFS` |
| `SE`, `SEERRO`, `E`, `OU`, `NÃO` | `IF`, `IFERROR`, `AND`, `OR`, `NOT` |
| `PROCV`, `PROCH`, `PROCX`, `ÍNDICE`, `CORRESP` | `VLOOKUP`, `HLOOKUP`, `XLOOKUP`, `INDEX`, `MATCH` |
| `ARRED`, `ARREDONDAR.PARA.CIMA`, `ARREDONDAR.PARA.BAIXO`, `TRUNCAR` | `ROUND`, `ROUNDUP`, `ROUNDDOWN`, `TRUNC` |
| `CONCATENAR`, `HOJE`, `AGORA` | `CONCATENATE`, `TODAY`, `NOW` |
| `VPL`, `TIR`, `PGTO`, `VP`, `VF` | `NPV`, `IRR`, `PMT`, `PV`, `FV` |
| `VERDADEIRO`, `FALSO` | `TRUE`, `FALSE` |

The editor, help, name-precedence and file-boundary rules above apply to Portuguese as they do
to Spanish.
