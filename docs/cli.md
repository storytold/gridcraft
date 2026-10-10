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
| `eval <formula> [--in FILE] [--sheet S] [--cell A1] [--json]` | `gridcraft-cli eval '=PMT(5%/12,360,-300000)'`; `gridcraft-cli eval '=SUM(B2:B9)' --in book.xlsx` |
| `cat <file> [--range R] [--sheet S] [--formulas] [--csv]` | `gridcraft-cli cat book.xlsx --range A1:F20` — aligned table of displayed values. |
| `run …` | see below |
| `commands [--json] [--search X]` | `gridcraft-cli commands --search border` — every command with its parameter docs. |
| `functions [--json] [--search X] [--category C]` | `gridcraft-cli functions --search lookup` |
| `mcp [--connect PORT] [--in FILE \| --sample NAME]` | MCP server on stdio ([mcp.md](mcp.md)). |
| `send <port> <method> [json]` | `gridcraft-cli send 7979 engine.execute '{"command":"home.bold","params":{"range":"A1:C1"}}'` — one control-channel request to a running app (`gridcraft --control 7979`). |
| `version` | Prints `gridcraft-cli <version>`. |

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
