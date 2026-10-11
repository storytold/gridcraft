# gridcraft-cli

Headless GridCraft: inspect, convert, evaluate, script and serve MCP without a window. Errors go to stderr with a
non-zero exit code.

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
| `eval <formula> [--in FILE] [--sheet S] [--cell A1] [--locale TAG] [--json]` | `gridcraft-cli eval '=PMT(5%/12,360,-300000)'`; `gridcraft-cli eval '=SUM(B2:B9)' --in book.xlsx`; `gridcraft-cli eval --locale pt-BR '=SOMA(1,5;2)'` prints `3,5` |
| `cat <file> [--range R] [--sheet S] [--formulas] [--csv]` | `gridcraft-cli cat book.xlsx --range A1:F20` — aligned table of displayed values. |
| `run …` | see below |
| `commands [--json] [--search X]` | `gridcraft-cli commands --search border` — every command with its parameter docs. |
| `functions [--json] [--search X] [--category C] [--locale TAG]` | `gridcraft-cli functions --search lookup`; with `--locale pt-BR` each function is listed under its Portuguese name (`PROCV(...)`). `--json` carries `name` (canonical) and `localName`. |
| `mcp [--connect PORT] [--in FILE \| --sample NAME] [--locale TAG]` | MCP server on stdio ([mcp.md](mcp.md)). `--locale` sets the language of the headless session; it is rejected with `--connect` (change a running app with `app.setLocale`). |
| `send <port> <method> [json]` | `gridcraft-cli send 7979 engine.execute '{"command":"home.bold","params":{"range":"A1:C1"}}'` — one control-channel request to a running app (`gridcraft --control 7979`). |
| `version` | Prints `gridcraft-cli <version>`. |

## `run`

```sh
gridcraft-cli run [--in FILE | --sample budget|sales|grades] \
    [--cmd 'id={json}']... [--script steps.jsonl] \
    [--out FILE] [--print RANGE|used] [--sheet S] [--csv] [--formulas] [--locale TAG] [--quiet]
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

## Language and regional format (`--locale`)

`--locale TAG` (`pt-BR`, `de-DE`, `ja-JP`, `pt_BR.UTF-8`, a bare `pt`, …) gives the session that interface language,
the same formula language and the same regional format, as if the user had chosen it in Options › Language. An unknown
tag is an error; the choices are listed by the `app.languages` command (see
[control-protocol.md](control-protocol.md): `gridcraft-cli run --cmd app.languages` prints them as JSON).

- **`eval`**: the formula is written the way a user of that language types it: local function names, local argument
  separator, local decimal separator. The result is printed with the region's decimal separator, and booleans and
  errors in their local spelling. `--json` stays canonical (JSON numbers, canonical error literals).
  `gridcraft-cli eval --locale pt-BR '=SOMA(1,5;2)'` → `3,5`; `gridcraft-cli eval --locale pt-BR '=SUM(1;2)'` → `#NOME?`
  (English names are rejected in a local session, as in Excel).
- **`run`**: commands that take typed input accept `inputLocal` (`cell.set`, `range.fill`) and the `…Local` variants of
  formula and number-format parameters; `input` stays canonical. Displayed values (`--print`, `sheet.read` with
  `formatted`) follow the region; `--print --csv` separates fields with the region's list separator.
  The locale is applied before the workbook is created or opened, so a new book gets local sheet names (`Tabelle1` in
  de-DE) and `--in data.csv` is read in the region's format (`1,5` is 1.5 in de-DE).
- **Files**: XLSX is always written canonically. Text files follow the region: CSV is written with the region's list
  separator and local decimals and read the same way. Pass `delimiter` to `file.open`, `file.save`, `file.saveAs`,
  `file.saveBytes` or `file.exportCsv` (for example `--cmd 'file.save={"path":"out.csv","delimiter":","}'`) to fix the
  separator for automation.

```sh
gridcraft-cli run --locale pt-BR \
  --cmd 'cell.set={"cell":"A1","inputLocal":"=SOMA(1,5;2)"}' --print A1     # shows 3,5
```

Without `--locale` the CLI works in en-US, independent of the operating system.
