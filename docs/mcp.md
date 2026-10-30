# GridCraft MCP server

`gridcraft-cli mcp` exposes GridCraft to AI agents over the [Model Context Protocol](https://modelcontextprotocol.io):
newline-delimited JSON-RPC 2.0 on stdin/stdout (protocol `2025-06-18`; `2025-03-26` and `2024-11-05` clients are
answered in their own version). Logs go to stderr.

Two backends:

| Mode | Command | What it drives |
|---|---|---|
| Headless (default) | `gridcraft-cli mcp [--in book.xlsx \| --sample budget]` | An in-process engine session — no window. |
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

## Restrict file access to a folder

Use the in-process server with read and write folders chosen by the operator. For
example, keep inputs readable and allow outputs only in a nested directory:

```sh
gridcraft-cli mcp --automation-read-root /path/to/project --automation-write-root /path/to/project/_artcraft
```

Both folders must already exist. Give the same folder to both options if read and
write access should have the same boundary; sibling folders also work. The CLI
uses the separate root flags from DeckCraft and VectorCraft, without a `--root`
alias. Relative paths keep their working-directory-based meaning.

When either access root is supplied, an omitted read or write root grants no
access of that kind. With no root options the existing unrestricted behaviour is
retained; the startup message and MCP initialization say which policy is active.
Relative paths are relative to the server's working directory, and still have to
resolve inside the corresponding root. Roots must exist and be directories.
Duplicate root flags and empty roots are errors. Both `--automation-read-root DIR`
and `--automation-read-root=DIR` are accepted (likewise for the write root).

The checks apply before `--in FILE` is opened and at the engine's file I/O
boundary: workbook open/save, CSV and text import, picture import, CSV/HTML/PDF
export, `file.print` with a path, implicit Save to the workbook's current path,
and the temporary path used for atomic saving. `execute_command` and nested
commands use the same checks. Returning workbook/PDF bytes without writing a path
still works in read-only mode. Resolving symlinks and comparing whole path
components rejects traversal outside the root and similarly named sibling
folders. Canonical components are compared exactly; Unicode case folding never
widens a root. Dangling links are refused.

Root flags cannot be combined with `--connect`: the desktop control channel does
not enforce these roots, so the CLI refuses that configuration rather than
advertising a restriction it cannot provide. This is a file policy for the
headless MCP server, not an OS sandbox. Native roots capture open directory handles
from the operator-selected paths before resolving their canonical names. A root
name swapped during initialization cannot replace the captured directory authority.
After resolving and checking a path, reads open it relative to the retained read
root. Saves pin the authorized parent directory, then create a fresh temporary
file exclusively and rename it through that same handle. Replacing a checked
path component with a symlink cannot redirect these operations outside the root.
A pre-existing `.sctmp` file or symlink is left untouched and does not block a save.
The parent directory is an authorized object: a later rename of that directory
does not revoke its open handle. Hard links, other local programs' own access,
and a process-wide OS sandbox remain outside this document-I/O policy.

The canonical path policy follows the implementation accepted in
[DeckCraft #247](https://github.com/storytold/deckcraft/pull/247); the additional
native handle-based I/O uses `cap-std`, with native-only `getrandom` temporary names.
On macOS and Linux the component-containment rule is equivalent. On Windows GridCraft
compares canonical components exactly rather than applying Unicode lowercasing,
so distinct directories on case-sensitive volumes cannot be conflated. A casing
mismatch can conservatively refuse access. Windows path display also preserves
non-Unicode paths and strips only recognised verbatim disk/UNC prefixes. Native
Windows validation is still needed.

Example MCP client configuration:

```json
{
  "mcpServers": {
    "gridcraft": {
      "command": "/path/to/gridcraft-cli",
      "args": ["mcp", "--automation-read-root", "/path/to/project", "--automation-write-root", "/path/to/project/_artcraft"]
    }
  }
}
```

This implementation adapts Isaac Levine's [PR #480](https://github.com/storytold/gridcraft/pull/480) and additionally checks the
initial `--in` file and the atomic-save temporary path, and covers the current
command layout. Related issues: #445 and #2172.
