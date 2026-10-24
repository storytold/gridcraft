# gridcraft-cli

Headless GridCraft: inspect, convert, evaluate, script and serve MCP without a window. Errors go to stderr with a
non-zero exit code.

`.xls` (Excel 97-2003, BIFF8) files are imported with values, formulas, number formats, merges,
column widths, row heights and defined names; fonts, fills, borders, charts and other objects are
not imported yet. Import warnings go to stderr. Convert to `.xlsx` to save; `.xls` output is not
supported.

`.xlsb` files support [worksheet data import](xlsb-import.md). Formula cells use their last saved
values; source formatting and other workbook features are omitted. Import warnings go to stderr.
Convert to `.xlsx` to save the imported workbook; XLSB output is not supported.

`.ods` files support [worksheet data import](ods-import.md). Formula cells use their last saved
values; source formatting and other workbook features are omitted. Import warnings go to stderr.
Convert to `.xlsx` to save the imported workbook; ODS output is not supported.

```sh
cargo install --path apps/gridcraft-cli     # or: cargo run -p gridcraft-cli -- <subcommand>
```

| Subcommand | Example |
|---|---|
| `info <file> [--json]` | `gridcraft-cli info book.xlsx` — sheets, used ranges, cell counts, tables, charts, names. |
| `convert <in> <out> [--sheet NAME]` | `gridcraft-cli convert book.xlsx data.csv --sheet Sales` — output format from the extension: `.xlsx`, `.csv`, `.tsv`, `.json`, `.html`. |
| `eval <formula> [--in FILE] [--sheet S] [--cell A1] [--json]` | `gridcraft-cli eval '=PMT(5%/12,360,-300000)'`; `gridcraft-cli eval '=SUM(B2:B9)' --in book.xlsx` — formulas in English with `,` between arguments. A formula that couldn't be entered in a cell (it doesn't parse, or a function gets the wrong number of arguments) is an error saying why; an unknown function is `#NAME?`, as in Excel. |
| `cat <file> [--range R] [--sheet S] [--formulas] [--csv]` | `gridcraft-cli cat book.xlsx --range A1:F20` — aligned table of displayed values. |
| `check <file-or-dir>... [--json] [--tolerance 1e-9] [--max-report N]` | `gridcraft-cli check corpus/` — calc oracle and XLSX round-trip fidelity, see below. |
| `run …` | see below |
| `commands [--json] [--search X]` | `gridcraft-cli commands --search border` — every command with its parameter docs. |
| `functions [--json] [--search X] [--category C]` | `gridcraft-cli functions --search lookup` |
| `mcp [--connect PORT] [--in FILE \| --sample NAME]` | MCP server on stdio ([mcp.md](mcp.md)). |
| `send <port> <method> [json]` | `gridcraft-cli send 7979 engine.execute '{"command":"home.bold","params":{"range":"A1:C1"}}'` — one control-channel request to a running app (`gridcraft --control 7979`). |
| `version` | Prints `gridcraft-cli <version>`. |

## Region for date-text formulas

`info`, `convert`, `eval`, `cat`, `run`, `check` and local `mcp` accept `--region TAG`
(for example `it-IT`, `en-GB` or `ja-JP`). The tag is validated and applied before a
workbook's first recalculation. Omit it for en-US; it is independent of interface language.
Tags are case-insensitive and accept `_` in place of `-`. Unknown tags are usage errors.

```sh
gridcraft-cli eval '=DATEVALUE("13/01/2026")' --region it-IT
gridcraft-cli eval '=MONTH("13/01/2026")' --region it-IT
gridcraft-cli run --region it-IT --in book.xlsx --cmd 'data.refreshAll' --print A1:G7
gridcraft-cli check book.xlsx --region it-IT --json
gridcraft --region it-IT book.xlsx
```

The native flag overrides the saved region preference before sample, blank or file workbooks
are created. The CLI always starts with explicit flags or en-US. `check` applies the region
to both its oracle recalculation and the workbook read back for round-trip comparison.
Region is an application preference and is not serialized into XLSX. Formula names and
programmatic cell/range input keep their existing canonical en-US syntax. Date/time
text in implicit arguments such as `YEAR`, `MONTH`, `DAY` and holiday ranges follows
the selected region. Numeric serial text retains en-US coercion: for example,
`HOUR("46035.5")` remains 12 under `it-IT`.

`mcp --connect` rejects `--region`: configure the running application before it opens the
file instead. A connected client must not silently replace that application's preference.

## `run`

```sh
gridcraft-cli run [--in FILE | --sample budget|sales|grades] \
    [--cmd 'id={json}']... [--script steps.jsonl] \
    [--out FILE] [--print RANGE|used] [--sheet S] [--csv] [--formulas] [--quiet]
```

Opens a file (or a sample, or a blank workbook), runs `--cmd` and `--script` steps in the order given, then saves
`--out` and prints each `--print` range. Each non-null command result is printed as a JSON line unless `--quiet`.
The first failing step stops the run with a non-zero exit.

A `--cmd` is `id`, `id={json}` or `id {json}`. A script has one command per line in the same forms, or as JSON objects
`{"command": "cell.set", "params": {...}}`; blank lines and `#` comments are skipped.

```sh
gridcraft-cli run --in book.xlsx \
  --cmd 'home.bold={"range":"A1:C1"}' \
  --cmd 'cell.set={"cell":"D2","input":"=B2*C2"}' \
  --cmd 'edit.fillDown={"range":"D2:D20"}' \
  --out book.xlsx --print A1:D5
```

Command ids and parameters: `gridcraft-cli commands`.

## `check`

```sh
gridcraft-cli check <file-or-dir>... [--json] [--tolerance 1e-9] [--max-report 20]
```

Measures calculation correctness and file fidelity on any set of `.xlsx`/`.xlsm` files (directories are searched
recursively). Nothing is written next to the inputs.

**Calc oracle.** A file saved by Excel (or another calculating app) stores the app's own result beside every formula.
`check` reads each file keeping those cached values, recalculates every formula with GridCraft and compares:
numbers within a relative `--tolerance` (absolute when the magnitude is below 1), errors by kind, text and logicals
exactly. Skipped, and counted separately:

- *no oracle*: formulas saved without a value, and every formula of a file whose producer asked for a full
  recalculation on load (`fullCalcOnLoad`: libraries that don't calculate, such as xlsxwriter, store 0 or nothing).
- *volatile*: `RAND`, `RANDARRAY`, `RANDBETWEEN`, `NOW`, `TODAY`, `INFO`, `CELL("filename")`, names that use them, and
  every formula that reads such a cell (through the dependency graph).

A logical result the producer cached as the number 1 or 0, without `t="b"` (Business Central's "Open in Excel"
exports write `<c><f>TRUE()</f><v>1</v></c>`), holds the same value: it matches GridCraft's TRUE or FALSE and is also
counted as *booleans saved as 1/0* (`boolAsNumber`). Excel types logical results, so on files it saved this count
should be 0; a non-zero count there would point at a formula where GridCraft returns a logical and Excel a number.

Each mismatch lists the cell, formula, expected (cached) and actual (GridCraft) values and the functions the formula
calls. Mismatches roll up by function: *checked* (formula cells calling it), *mismatched*, and *primary* (mismatched
while none of the cell's precedents mismatch — the likely culprit, as opposed to a downstream effect). They also roll
up by *signature*, `expected type → actual type` (e.g. `number → #SPILL!`), which often groups one root cause.

**Round trip.** The recalculated workbook is written with GridCraft's XLSX writer, the package is validated (every
part has a content type, every content-type override and internal relationship target exists, relationship ids are
unique: what Excel would "repair"), compared with the source (no kind of relationship it had, such as `xmlMaps`, is
missing; a calculation chain doesn't count, Excel rebuilds it, nor do a link to a place in the workbook and a
relationship to an empty part, such as a comments part without comments) and read back. Categories compared: `package`, `sheets` (names and order),
`values`, `formulas`, `numFmts` (per cell), `merges`, `names`, `tables` (name, range, columns), `condFormats` (rules
per sheet), `validations`, `charts` (kinds and series counts), `comments`, `hyperlinks`, `pivots`. `#SPILL!`/`#CALC!`
compare equal to `#VALUE!` and `#CIRC!` to 0, as that's what the file format stores.
unique: what Excel would "repair") and read back. Categories compared: `package`, `sheets` (names and order),
`values`, `formulas`, `numFmts` (per cell), `merges`, `names`, `tables` (name, range, columns), `condFormats` (each
sheet's rules in priority order, priority numbers aside), `validations`, `charts` (kinds and series counts), `comments`,
`hyperlinks`, `pivots`. `#SPILL!`/`#CALC!` compare equal to `#VALUE!` and `#CIRC!` to 0, as
that's what the file format stores.

Files that fail to open are reported with GridCraft's error and don't stop the run. Unreadable directories,
directory-entry/metadata errors, and subtrees or files omitted by the depth (32) and file (100,000) limits appear
as failed path reports too, in both human and JSON output. Intentional hidden/lock-file exclusions remain ignored.
JSON `totals.files` includes failed discovery paths; `totals.opened` counts only workbooks that opened successfully.
An empty scan fails with a diagnostic. Exit code: 0 when at least one file was checked, discovery was complete, and every file
opened with no mismatch and no round-trip difference, 1 otherwise, 2 on a usage error. `--max-report` caps the
examples listed per file and category (counts are always complete).

The human report prints each file's mismatches and differing categories, a summary table, the most mismatching
functions, mismatch signatures and totals. `--json` prints one object (schema `gridcraft-check/1`; new fields may be
added, a changed meaning bumps the version):

```jsonc
{
  "schema": "gridcraft-check/1",
  "tolerance": 1e-9,
  "files": [{
    "path": "book.xlsx",
    "opened": true,
    "error": null,                     // GridCraft's message when the file can't be opened
    "warnings": ["..."],               // import warnings
    "calc": {
      "formulaCells": 3, "checked": 2, "matched": 1, "mismatched": 1, "noOracle": 1, "volatile": 0,
      "staleCache": false,             // fullCalcOnLoad: nothing compared
      "boolAsNumber": 0,               // matched TRUE/FALSE results the producer saved as 1/0 (no t="b")
      "mismatches": [{"cell": "Sheet1!B1", "formula": "=SUM(A1:A2)", "expected": 7, "actual": 6,
                      "kind": "numeric",  // numeric | text | bool | error | blank (GridCraft empty) | type
                      "functions": ["SUM"], "primary": true}],
      "functions": [{"name": "SUM", "checked": 1, "mismatched": 1, "primary": 1}],
      "signatures": {"number → number": 1}
    },
    "roundTrip": {
      "error": null,                   // write or read-back failure
      "differing": ["values"],
      "categories": {"values": {"equal": false, "diffs": 1, "examples": ["Sheet1!A1: 2 → 3"]}, "...": {}}
    }
  }],
  "totals": {"files": 1, "opened": 1, "failedToOpen": 0, "formulaCells": 3, "checked": 2, "matched": 1,
             "mismatched": 1, "noOracle": 1, "volatile": 0, "boolAsNumber": 0, "roundTripFailed": 0, "roundTripDiffering": 1,
             "categoriesDiffering": {"values": 1}},
  "functions": [{"name": "SUM", "checked": 1, "mismatched": 1, "primary": 1}],  // all files, most mismatched first
  "signatures": {"number → number": 1}
}
```

Values use the same JSON shapes as `eval --json`: numbers, strings, booleans, `null` (empty) and
`{"error": "#N/A"}`. Mismatch lists are capped by `--max-report`; the counts are not.
