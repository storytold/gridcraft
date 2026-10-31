# GridCraft MCP server

`gridcraft-cli mcp` exposes GridCraft to AI agents over the [Model Context Protocol](https://modelcontextprotocol.io):
newline-delimited JSON-RPC 2.0 on stdin/stdout (protocol `2025-06-18`; `2025-03-26` and `2024-11-05` clients are
answered in their own version). Logs go to stderr.

Two backends:

| Mode | Command | What it drives |
|---|---|---|
| Headless (default) | `gridcraft-cli mcp [--in book.xlsx \| --sample budget]` | An in-process engine session — no window. It can be [confined](#confining-an-agent) to folders and commands. |
| Remote | `gridcraft-cli mcp --connect 7979` | A running app started with `gridcraft --control 7979` (TCP JSON lines on `127.0.0.1`, authenticated with the app's token: it is read from the app's token file or `GRIDCRAFT_CONTROL_TOKEN`, see [`control-protocol.md`](control-protocol.md)). |

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

## Confining an agent

A headless server can be held to the folders and commands a task needs:

```sh
gridcraft-cli mcp --automation-read-root ~/work/in --automation-write-root ~/work/out \
    --allow-commands 'range.setValues, cell.get, sheet.read, document.inspect, home.*, file.save, file.saveAs'
```

- `--automation-read-root DIR`: workbooks, CSV imports and every other file a command reads must be inside `DIR`
  (`--in` too). `--automation-write-root DIR`: saves and exports must land inside `DIR`, in a folder that already
  exists (a save doesn't create folders); the write root itself and a folder are not files to write. Each root
  must be an existing folder. With one root given, the access the other would grant is denied: with only a read
  root the agent can't write a file at all. With neither root, files are not confined (`--allow-commands` alone
  holds the agent to commands only). Paths are resolved before they are checked: `..`, symbolic links and
  junctions that lead out are refused, and so are Windows device names (`NUL`, `COM1`…). Case is ignored on
  Windows.
- `--allow-commands LIST`: only the commands listed may run, by id (`home.bold`) or group (`home.*`), separated by
  commas, spaces or new lines; `#` starts a comment. `@FILE` reads the list from a file, which must be a
  regular file of at most 256 KiB. A pipe, socket or device found at the first check is refused without being
  opened; the check and the open are not atomic, so another program that swaps a FIFO in between can still
  make the open wait for a writer. The read stops at 256 KiB whatever the file claims, and a longer list is
  refused, never cut short. An unknown id or group, an
  empty list, or the flag without a value is an error at startup, so a typo never widens or silently narrows
  the list. The dedicated tools are held to
  it by the commands they run (`write_range` is `range.setValues`, `format_range` the `home.*` commands it
  composes, `inspect_workbook` is `document.inspect`, `read_range` is `sheet.read`…), and so are the steps of a
  stored script: `automate.runScript` refuses a script that holds an unlisted command before any step runs.
  `list_commands` shows only the commands listed.

A refused path or command is a tool error (`isError: true`) naming what was refused; nothing is read or written.
Each flag may be given once, any problem with them stops the server at startup, and they are an error with
`--connect`: a running app is not confined by them.

## Tools

| Tool | Arguments | Does |
|---|---|---|
| `execute_command` | `command`, `params?` | Runs **any** engine command (`cell.set`, `home.bold`, `data.removeDuplicates`, …). |
| `list_commands` | `search?` | Command ids, labels, ribbon paths, shortcuts and parameter docs. |
| `list_windows` | — | Open windows: window id, workbook (`doc`) id, title ("Book1", or "Book1:2" for a workbook's second window), unsaved changes, active, hidden. |
| `inspect_workbook` | — | Sheets, used ranges, tables, charts, names, selection, history. |
| `read_range` | `range?`, `sheet?`, `formulas?`, `formatted?` | Rows of values (default: the used range). |
| `write_range` | `range`, `values` (2-D) | `range.setValues`; strings starting with `=` are formulas. |
| `set_cell` / `get_cell` | `cell`, `input` / `cell` | Enter one cell / read value, formula, display text, style. |
| `evaluate_formula` | `formula`, `cell?` | Evaluates without changing the sheet. A formula that couldn't be entered in a cell (it doesn't parse, or a function gets the wrong number of arguments) is an error saying why. |
| `select` | `range`, `active?` | Sets the selection. |
| `format_range` | `range`, `bold?`, `italic?`, `underline?`, `fontName?`, `fontSize?`, `fontColor?`, `fillColor?`, `numberFormat?`, `horizontalAlign?`, `wrapText?`, `borders?` | Composes the `home.*` formatting commands. |
| `insert_chart` | `range`, `type`, `title?`, `subtype?`, `at?` | `insert.chart`. |
| `create_table` | `range`, `style?`, `header?`, `name?` | `insert.table`. |
| `sort_range` | `keys`, `range?`, `header?` | `data.sort`; keys are `"B"`, `"B desc"` or `{column, order}`. |
| `filter` | `column`, `range?`, `values?`, `custom?`, `top?`, `blanks?`, `clear?` | `data.filterBy` (turns AutoFilter on when needed). |
| `open_workbook` / `save_workbook` / `new_workbook` | `path` / `path?` / `sample?` | Files (`.xlsx`, `.csv`, `.tsv`, `.json`, `.html`). |
| `undo` / `redo` | `steps?` | History. |
| `screenshot` | `path` | PNG of the window (Remote only). |
| `list_functions` | `search?`, `category?` | Worksheet functions with signatures. |

Every tool that acts on a workbook (all but `list_commands`, `list_functions`, `list_windows`, `open_workbook`,
`new_workbook` and `screenshot`) also takes `window` or `doc`, an id from `list_windows`, and acts in that window
(for `doc`, the workbook's most recent window) without bringing it forward. Without them it acts in the active window.

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
