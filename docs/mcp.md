# GridCraft MCP server

`gridcraft-cli mcp` exposes GridCraft to AI agents over the [Model Context Protocol](https://modelcontextprotocol.io):
newline-delimited JSON-RPC 2.0 on stdin/stdout (protocol `2025-06-18`; `2025-03-26` and `2024-11-05` clients are
answered in their own version). Logs go to stderr.

Two backends:

| Mode | Command | What it drives |
|---|---|---|
| Headless (default) | `gridcraft-cli mcp [--in book.xlsx \| --sample budget] [--locale pt-BR]` | An in-process engine session — no window. |
| Remote | `gridcraft-cli mcp --connect 7979` | A running app started with `gridcraft --control 7979` (TCP JSON lines on `127.0.0.1`). |

## Claude Code

```sh
claude mcp add gridcraft -- gridcraft-cli mcp
# or drive the desktop app you are looking at:
claude mcp add gridcraft-app -- gridcraft-cli mcp --connect 7979
```

JSON config (`.mcp.json`, Claude Desktop, other clients):

```json
{
  "mcpServers": {
    "gridcraft": {
      "command": "gridcraft-cli",
      "args": ["mcp"]
    }
  }
}
```

Use `"args": ["mcp", "--connect", "7979"]` for the desktop app, or `["mcp", "--in", "/path/book.xlsx"]` to start
with a file open.

## Tools

| Tool | Arguments | Does |
|---|---|---|
| `execute_command` | `command`, `params?` | Runs **any** engine command (`cell.set`, `home.bold`, `data.removeDuplicates`, `app.setLocale`, …). |
| `list_commands` | `search?` | Command ids, labels, ribbon paths, shortcuts and parameter docs. |
| `inspect_workbook` | — | Sheets, used ranges, tables, charts, names, selection, history. |
| `read_range` | `range?`, `sheet?`, `formulas?`, `formatted?`, `local?` | Rows of values (default: the used range); with `local`, formulas and values are shown in the session's language and region. |
| `write_range` | `range`, `values` (2-D), `local?`, `locale?` | `range.setValues`; strings starting with `=` are formulas; with `local` the strings are typed in the current language and region (`locale` overrides). |
| `set_cell` / `get_cell` | `cell` plus exactly one of `input` (canonical) or `inputLocal` (+ `locale?`) / `cell` | Enter one cell / read value, formula (`formula`, `formulaLocal`), display text, style. |
| `evaluate_formula` | exactly one of `formula` (canonical) or `formulaLocal` (+ `locale?`), `cell?` | Evaluates without changing the sheet. |
| `select` | `range`, `active?` | Sets the selection. |
| `format_range` | `range`, `bold?`, `italic?`, `underline?`, `fontName?`, `fontSize?`, `fontColor?`, `fillColor?`, `numberFormat?` or `numberFormatLocal?` (+ `locale?`), `horizontalAlign?`, `wrapText?`, `borders?` | Composes the `home.*` formatting commands. |
| `insert_chart` | `range`, `type`, `title?`, `subtype?`, `at?` | `insert.chart`. |
| `create_table` | `range`, `style?`, `header?`, `name?` | `insert.table`. |
| `sort_range` | `keys`, `range?`, `header?` | `data.sort`; keys are `"B"`, `"B desc"` or `{column, order}`. |
| `filter` | `column`, `range?`, `values?`, `custom?`, `top?`, `blanks?`, `clear?` | `data.filterBy` (turns AutoFilter on when needed). |
| `open_workbook` / `save_workbook` / `new_workbook` | `path` / `path?` / `sample?` | Files (`.xlsx`, `.csv`, `.tsv`, `.json`, `.html`). |
| `undo` / `redo` | `steps?` | History. |
| `screenshot` | `path` | PNG of the window (Remote only). |
| `list_functions` | `search?`, `category?` | Worksheet functions with `name` (canonical), `localName`, signature and description. |

## Language and regional format

Everything an agent sends or reads is **canonical** unless it says otherwise: `input`, `formula`, `numberFormat` use
English function names, `,` between arguments and `.` as the decimal point, so a workbook means the same thing in every
language. The user's language is reachable in three ways:

- `execute_command` with `app.getInternational` reports the session's languages, separators, date order and R1C1
  letters; `app.setLocale` changes them (`{"uiLanguage":"pt-BR","regionalFormat":"pt-BR"}`) and `app.languages` lists
  the choices. See [control-protocol.md](control-protocol.md).
- `set_cell` accepts `inputLocal` — the text as the user types it (`=SOMA(1,5;2)`, `1,5`, `10/10/2026`) — instead of
  `input`; `locale` overrides the session language for that one call. Giving both `input` and `inputLocal` is an
  error.
- Results carry the local spelling next to the canonical one: `get_cell` returns `formula` and `formulaLocal`,
  `list_functions` returns `name` and `localName`.

In a local session English function names are rejected like Excel does (`=SUM(1;2)` in pt-BR gives `#NOME?`), so use
`input` for canonical text and `inputLocal` for local text, never one in place of the other.

Results are one text block holding pretty JSON; failures set `isError: true` with a readable message. Unknown tools,
missing required arguments and wrong argument types are JSON-RPC `-32602` errors; malformed JSON is `-32700`; unknown
methods `-32601`.

## Resources

- `gridcraft://workbook` — `document.inspect` of the active workbook.
- `gridcraft://commands` — every command with its parameter docs.
- `gridcraft://functions` — every worksheet function.

`prompts/list` returns an empty list.

## Example session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}
→ {"jsonrpc":"2.0","method":"notifications/initialized"}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"write_range","arguments":{"range":"A1","values":[["Item","Cost"],["Rent",1200],["Food",400],["Total","=SUM(B2:B3)"]]}}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_range","arguments":{"range":"B4"}}}
← {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{\n  \"range\": \"B4\", ... \"values\": [[1600.0]]\n}"}],"isError":false}}
```

## Embedding

`gridcraft-mcp` is a library: `Server::new(Box::new(Headless::new()))` or
`Server::new(Box::new(Remote::connect(&control_addr("7979"))?))`, then `Server::serve(stdin, stdout)` or
`Server::handle_line(line) -> Option<String>`. Implement the `Backend` trait (`call(method, params)`) to drive another
host.
