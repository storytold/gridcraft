# Function parity: worksheet functions and formulas

> **Last reviewed:** 2026-10-15 · **Last updated:** 2026-10-21 · **Change:** minor (basis-0 securities functions count the end of February to a 31st as 31 days) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Excel's worksheet functions, category by category, against GridCraft's function library. Part of
[target-app-parity.md](target-app-parity.md); gaps feed [gaps.md](gaps.md).

## How this is measured

- **Excel's list (523):** Microsoft's "Excel functions (alphabetical)" support page, fetched and
  parsed on 2026-10-10 (506 rows with their category and the Excel version that introduced
  them), plus 14 text functions the page lists under combined entries (FIND/FINDB, LEFT/LEFTB,
  LEN/LENB, MID/MIDB, REPLACE/REPLACEB, RIGHT/RIGHTB, SEARCH/SEARCHB), BETA.INV, and COPILOT and
  PY (Microsoft 365 functions not on that page when fetched).
- **Ours (505):** every function spec in `crates/functions/src/*.rs` (`f!("NAME", …)`) plus the
- **Ours (503):** every function spec in `crates/functions/src/*.rs` (`f!("NAME", …)`) plus the
- **Ours (502):** every function spec in `crates/functions/src/*.rs` (`f!("NAME", …)`) plus the
- **Ours (502):** every function spec in `crates/functions/src/*.rs` (`f!("NAME", …)`) plus the
- **Ours (505):** every function spec in `crates/functions/src/*.rs` (`f!("NAME", …)`) plus the
  evaluator's special forms in `crates/calc/src/eval.rs` (`SPECIAL_FUNCTIONS`: IF, IFS, CHOOSE,
  OFFSET, INDIRECT, INDEX, SUBTOTAL, AGGREGATE, TEXT, LET, LAMBDA, MAP, REDUCE, SCAN, BYROW, BYCOL,
  MAKEARRAY…), excluding the test-only `ADD2`. Every one is an Excel name.
- **Not on Microsoft's list:** USDOLLAR, a legacy function Excel still evaluates (like DOLLAR),
  is in our library as a Compatibility function for files that use it. It is not counted above.
- **Working** excludes stubs that return an error by design: the 7 CUBE functions (`#N/A`, no
  cube connections) and WEBSERVICE and FILTERXML (`#VALUE!`). Excel for Mac also lacks
  WEBSERVICE and FILTERXML; Excel for Windows has them.
- The measurement is **presence**. Depth (edge cases matching Excel) is estimated below.

## Coverage

**Measured: 513 / 523 by name (98.1%); 504 / 523 working (96.4%).**

| Category | Excel | Ours | Working | % working | Missing |
|---|---|---|---|---|---|
| Statistical | 111 | 111 | 111 | 100% | — |
| Math and trigonometry | 80 | 79 | 79 | 99% | PERCENTOF |
| Financial | 55 | 55 | 55 | 100% | — |
| Engineering | 54 | 54 | 54 | 100% | — |
| Text | 50 | 48 | 48 | 96% | DETECTLANGUAGE, TRANSLATE |
| Lookup and reference | 40 | 36 | 36 | 90% | GETPIVOTDATA, IMAGE, RTD, TRIMRANGE |
| Compatibility | 40 | 40 | 40 | 100% | — |
| Date and time | 25 | 25 | 25 | 100% | — |
| Information | 22 | 21 | 21 | 95% | STOCKHISTORY |
| Logical | 19 | 19 | 19 | 100% | — |
| Database | 12 | 12 | 12 | 100% | — |
| Cube | 7 | 7 | 0 | 0% | (all 7 are `#N/A` stubs) |
| Web | 3 | 3 | 1 | 33% | (WEBSERVICE, FILTERXML are stubs; ENCODEURL works) |
| Add-in and Automation | 3 | 1 | 1 | 33% | CALL, REGISTER.ID |
| AI and Python | 2 | 0 | 0 | 0% | COPILOT, PY |
| **Total** | **523** | **505** | **496** | **94.8%** | 18 missing, 9 stubs |

By the Excel version that introduced them (working / total): before 2010 346 / 360, 2010
56 / 56, 2013 49 / 51, 2016 1 / 5, 2019 6 / 6, 2021 11 / 11, 2024 22 / 23, Microsoft 365 5 / 11.
Dynamic-array and LAMBDA-era functions (FILTER, SORT, UNIQUE, XLOOKUP, VSTACK, TAKE, GROUPBY,
PIVOTBY, REGEXTEST…) are all present.

FORECAST.ETS missing-point interpolation walks known points once, so filling a long gap takes
linear work in the completed timeline length (#1042). Interpolation values, nearest-value
endpoint handling, zero-fill mode and the existing 30% missing-point limit are unchanged.
This improves series preparation; it does not change the model-fitting or seasonality algorithms.

### Scope of what's missing

| Function(s) | In scope? | Estimate | Note |
|---|---|---|---|
| FORECAST.ETS family (4) | yes | 4–6 h | AAA exponential smoothing with seasonality detection; needed for Forecast Sheet too |
| ODDFPRICE, ODDFYIELD, ODDLPRICE, ODDLYIELD | yes | 2–4 h | Odd first/last period bond maths |
| GETPIVOTDATA | yes | 2–3 h | Needs the pivot engine's result cache; also the "Generate GetPivotData" option |
| IMAGE | yes | 3–5 h | Pictures in cells (see PR #105); fetching URLs needs a privacy decision |
| CUBE* (7), WEBSERVICE, FILTERXML | partly | 4–8 h | FILTERXML can work offline; CUBE needs an OLAP/data-model source |
| TRANSLATE, DETECTLANGUAGE, STOCKHISTORY, COPILOT, PY | no (cloud) | — | Microsoft cloud services; open equivalents only by owner decision |
| CALL, REGISTER.ID, RTD | no | — | Native DLL/COM calls; not portable or safe |

## Formula language

| Feature | Status |
|---|---|
| A1 and R1C1 references, absolute/relative/mixed, F4 cycling | done |
| 3-D references (`Sheet1:Sheet3!A1`) | done; deleting a span's end sheet keeps the rest (#140) |
| Structured references (`Table1[Col]`, `[#Totals]`, `@`; the unbracketed `[[#This Row],Col]` some writers emit) | done |
| Defined names (workbook and sheet scope), Create from Selection | done |
| Dynamic arrays, spill, `#SPILL!`, spill operator `A1#` | done; stale spills after a structural edit or a cleared blocker fixed (#133, #139; PRs #148, #157) |
| Recalculation after clearing large sparse ranges | every stored cell changed by Clear is reported to recalculation, without a million-coordinate cutoff; removed formula spills are cleared, and removing empty merges still unblocks spills (#1032) |
| SUBTOTAL / AGGREGATE input size | references trim to the used range; all areas, arrays and values of one call are charged against the 16-million-cell limit before any cell is read (skipped hidden, nested and error cells count), and `#NUM!` past it (#2175) |
| Implicit intersection `@`, legacy CSE array formulas | done; legacy array range fill (PR #156) |
| Implicit intersection `@`, legacy CSE array formulas | done, the entered range filled (PR #156) |
| Trim references `A1:.E10` (trailing), `A1.:E10` (leading), `A1.:.E10` (both), also on whole rows/columns (`A:.A`) | done; evaluated as `TRIMRANGE(range, code, code)`, adjusted on insert/delete; files store `_xlfn.TRIMRANGE(range,code,code)` (Excel's own file spelling unconfirmed in public docs), `_xlfn._TRO_ALL/_TRO_LEADING/_TRO_TRAILING(...)` read too |
| LET, LAMBDA, named LAMBDAs, ISOMITTED, MAP/REDUCE/SCAN/BYROW/BYCOL/MAKEARRAY | done |
| Iterative calculation for circular references | done: with Enable Iterative Calculation on (`formulas.calculationOptions` `{iterative, maxIterations, maxChange}`, saved as `<calcPr iterate iterateCount iterateDelta>`), loops are evaluated again from their previous values (blank = 0) until no cell on a loop changes by Maximum Change or Maximum Iterations is reached; off, cycles give `#CIRC!`. Differences: no Options dialog yet (command only); GridCraft recalculates on open, so a loop that never settles (`=A1+1`) advances again each time the file is opened |
| 15-significant-digit comparison semantics | done (#57): `=0.1+0.2=0.3` is TRUE, and MATCH and COUNTIF compare the same way. Arithmetic keeps full precision, as Excel does — `=(0.1+0.2)-0.3` is 5.55E-17 |
| External workbook references (`[Book.xlsx]Sheet1!A1`) | parsed; links dropped on open, no update |
| Localized function names and `;` separators | **missing** (#45); en-US only |
| Long flat operator chains (`=A1+A2+…`, `-`, `*`, `/`, `^`, `&`, comparisons) | done: no operand limit below Excel's 8,192-character formula limit (Excel, observed: `=1+1+…` with 4,096 operands is exactly 8,192 characters with the `=` and calculates; one more character is refused). Evaluated (#484), printed (#628), stored, copied, adjusted, saved and reopened without recursing once per operand, on a 1 MiB stack (#916); written to XLSX as plain formulas, not dynamic arrays; Spanish entry accepts them |
| Error values | the seven classic errors plus `#SPILL!`, `#CALC!`, `#GETTING_DATA` done; `#FIELD!`, `#BLOCKED!`, `#CONNECT!`, `#BUSY!`, `#UNKNOWN!`, `#PYTHON!` missing (minor: they come from data types, cloud and Python features) |

## Depth (estimated)

**~82% ready.** The library is broad and has 96 test functions in `crates/functions` with many
assertions each, but there is no conformance suite against Excel-computed results, and user
reports keep finding edge cases: MIN() with no arguments (#138), COUNTBLANK past the used range
(#137, and past 16,384 cells #1010, fixed 10-14 for bounded references), ROUND at 15 digits and DATE(1900,2,29) (fixed 10-09), SUMIF sum-range sizing (fixed
10-09). Statistical distributions and financial functions with iterative solvers (RATE, IRR,
XIRR, YIELD) are the highest-risk for last-digit differences.

TEXT() and cell display share `crates/numfmt`; checked against Excel-cached results in
number- and text-format test workbooks (352 TEXT cells), all match:
fraction sections with literal text (`#\:#=/=#`, `#_#/#`), zero fractions, zero-padded
denominators, `?` placeholders padding the thousands separator, and booleans through the text
section (`TEXT(TRUE,";;;\@@\@")` = `@TRUE@`).

`MEDIAN` and `AGGREGATE` function number 12 use an overflow-safe midpoint for even-sized
inputs, so large finite middle values do not become `#NUM!` merely because their sum overflows (#1579).
Sorting, argument coercion and error propagation are unchanged; this is a numerical correction
under the documented median rule, not an extreme-value comparison against desktop Excel.

Work to close (16–26 h in total): the in-scope missing functions (10–16 h with the table above),
an oracle corpus of Excel-computed results per function (owner runs Excel on generated
workbooks; results checked in as data, not Excel files) and fixing what it finds (6–10 h).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-21 | minor | Basis 0 (US 30/360) in the coupon, discount-security, ACCRINTM, PRICEMAT/YIELDMAT and AMOR* functions counts the end of February to a 31st as 31 days, as YEARFRAC does (COUPDAYBS(31 Mar 2008, 28 Feb 2011, 1, 0) = 31, was 30); ACCRINT and the odd-period functions keep their period count |
| 2026-10-21 | minor | SUBTOTAL and AGGREGATE charge every argument against the evaluator's 16-million-cell limit before scanning; a value in XFD1048576 no longer makes `SUBTOTAL(9,A:XFD)` read the whole sheet (#2175) |
| 2026-10-19 | minor | LET and LAMBDA names compare case-insensitively in any script, as in Excel: `=LET(dług,100,DŁUG*2)` is 200 (was #NAME?); also LET-bound lambdas called or passed by name (`żar(2)`, `MAP(…,Λ)`), `ISOMITTED`, and formula-bar alias translation (one shared `gridcraft_formula::same_name`, which matches `ß` with `ẞ` and keeps dotless `ı` apart from `i`) |
| 2026-10-19 | minor | Long flat operator chains (#916): 4,096 operands (Excel's 8,192-character limit) no longer overflow the stack when entered, filled, adjusted or saved; chains past 64 operands are no longer saved as dynamic-array formulas; Spanish entry no longer refuses chains past 256 operands |
| 2026-10-17 | minor | 15-significant-digit comparison moved from missing to done (#57), and the resolved caveats on the 3-D reference, dynamic-array and legacy-array rows cleared (#140, #133, #139, #156) |
| 2026-10-16 | minor | FORECAST.ETS gap interpolation uses a forward-only known-point scan instead of rescanning each gap for every missing value; numerical behavior and missing-point limits are preserved (#1042) |
| 2026-10-15 | minor | MEDIAN and AGGREGATE median use an overflow-safe midpoint (#1579); regressions cover finite extremes, tiny values and dependent recalculation |
| 2026-10-15 | minor | ADDRESS quotes a sheet name the way Excel does: names that read as an A1 cell (`A1`) or an R1C1 reference (`R1C1`, `RC`) and names starting with `.` get quotes; `TRUE` / `FALSE` stay bare (#1458) |
| 2026-10-13 | minor | Clear reports changed stored cells sparsely, including those beyond the first million coordinates; dependent formulas and removed spills update, while clearing empty merges still unblocks spills (#1032) |
| 2026-10-14 | minor | Bounded references up to 1,048,576 cells (`D3:AZ400`) keep their full shape instead of stopping at the used range, so `SUMPRODUCT(D3:AZ400*B3:B400,(A3:A400=x)*(D1:AZ1=y))` broadcasts like Excel and `ROWS`, `COUNTBLANK` and `--(r="")` count the blanks; references that run to the edge of the sheet (`A:A`) still stop at the used range (#1010) |
| 2026-10-12 | minor | USDOLLAR added as a Compatibility function (not on Microsoft's list, so the counts don't change); MIRR with no positive flow is -1, COUNTIF's bare `"<="` / `">="` skip empty strings, INDIRECT with `a1` = FALSE gives `#REF!` for A1-style text (#738) |
| 2026-10-11 | minor | TEXT / number formats fixed against Excel-cached results: fraction literals and zero fractions, `0` denominators zero padded, `?` pads the thousands separator, booleans use the text section (226 mismatches → 0 in 352 TEXT cells) |
| 2026-10-11 | minor | Iterative calculation: circular references iterate up to Maximum Iterations / Maximum Change when enabled |
| 2026-10-11 | minor | Trim-reference operators (`:.`, `.:`, `.:.`) and unbracketed `[[#This Row],Col]` done; dropped the stale "Scope of what's missing" rows for TRIMRANGE, PERCENTOF, BAHTTEXT and the trim references |
| 2026-10-11 | minor | Fixes from the cached-value oracle (`gridcraft-cli check`): MODE / MODE.SNGL / MODE.MULT give `#VALUE!` for text in a direct argument or single-cell reference; `VALUE("")` is `#VALUE!`; SUMPRODUCT with a text scalar argument is `#VALUE!`; ADDRESS leaves `[Book]Sheet` unquoted; VLOOKUP / HLOOKUP / LOOKUP / MATCH approximate match bisect like Excel on unsorted mixed-type data; XLOOKUP / XMATCH binary search compares across types in sort order; SUBTOTAL leaves out filtered rows for every function number (table and sheet AutoFilters) |
| 2026-10-11 | minor | ODDFPRICE, ODDFYIELD, ODDLPRICE, ODDLYIELD added: Financial 55 / 55; 505 / 523 by name, 496 working |
| 2026-10-11 | minor | PERCENTOF and EUROCONVERT added: 503 / 523 by name, 494 working |
| 2026-10-11 | minor | BAHTTEXT added: 502 / 523 by name, 493 working |
| 2026-10-11 | minor | TRIMRANGE added (the trim-reference operators not yet): 502 / 523 by name, 493 working |
| 2026-10-11 | minor | FORECAST.ETS, .CONFINT, .SEASONALITY, .STAT added (AAA exponential smoothing; Excel's optimiser is undocumented, so last digits on noisy data may differ): 505 / 523 by name, 496 working |
| 2026-10-10 | major | Created: 501 / 523 by name, 492 working, per category, against Microsoft's published list |
