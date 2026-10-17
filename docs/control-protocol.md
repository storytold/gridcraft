# GridCraft control protocol

Start the desktop app with a control port:

```sh
gridcraft --control 7979            # or GRIDCRAFT_CONTROL_PORT=7979
```

Then send newline-delimited JSON to `127.0.0.1:7979`. Every request line gets one reply line:

```json
{"id": 1, "method": "engine.execute", "params": {"command": "cell.set", "params": {"cell": "B2", "input": "=SUM(A1:A9)"}}}
{"id": 1, "ok": true, "result": {"cell": "B2", "value": "45"}}
```

Errors come back as `{"id": …, "ok": false, "error": "…"}`.

A line that is not a JSON object with a string `method` (an HTTP request, text that isn't JSON,
bytes that aren't UTF-8, a line over 64 MiB) gets one error reply and the server closes the
connection; nothing sent after it runs. Blank lines are skipped.

## Methods

| Method | Params | What it does |
|---|---|---|
| `engine.execute` | `{command, params}` | Runs any engine command (see `engine.commands` or `gridcraft-cli commands`) |
| `engine.commands` | `{search?}` | Every command: id, label, ribbon path, shortcut, params doc, enabled; `search` keeps those whose id, label or ribbon path contains it (case-insensitive) |
| `engine.journal` | | Commands run so far (replayable). Passwords (`password`, `…Password` params) are left out, so a replayed Protect Sheet has no password |
| `document.inspect` | | Workbook summary: sheets, used ranges, tables, charts, names, selection, undo labels |
| `sheet.read` | `{range?, sheet?, formulas?, formatted?}` | Values (or formulas / displayed text) of a range |
| `cell.get` | `{cell?}` | One cell: value, formula, displayed text, style, merge, comment, link |
| `ui.inspect` | | UI state: ribbon tab, editor, open dialog, Backstage page, message, grid rect, scroll, perf |
| `ui.click` / `ui.drag` / `ui.move` | `{x, y, toX?, toY?, button?, count?, shift?, cmd?, alt?}` | Real pointer input in window points |
| `ui.cell.click` / `ui.cell.drag` | `{cell}` / `{from, to}` | Pointer input at a cell's centre (works in point mode while editing) |
| `ui.key` / `ui.text` | `{key, shift?, cmd?, alt?, ctrl?}` / `{text}` | Keyboard input (typing in the grid starts editing) |
| `ui.edit.begin` / `ui.edit.text` / `ui.edit.commit` / `ui.edit.cancel` | `{text?}` / `{move?: down\|right\|up\|left\|none}` | Drive the cell editor |
| `ui.ribbon` | `{tab}` | Switch ribbon tab |
| `ui.dialog` / `ui.dialog.set` / `ui.dialog.confirm` / `ui.dialog.cancel` | `{name}` / `{field, value}` | Open, fill in and confirm dialogs |
| `ui.backstage` (via `engine.execute`) | `{page?: home\|new\|open\|info\|saveAs\|print\|export\|options\|close}` | Open the File tab's Backstage view on a page, or close it (`ui.inspect` reports the open page as `backstage`) |
| `ui.screenshot` | `{path?}` | PNG of the window (base64 when no path). Animations are settled first: the picture shows the final state |
| `ui.set` | `{dark?, formulaBar?, ribbonCollapsed?, animations?}` | UI preferences (`formulaBar` runs `view.formulaBar`) |
| `ui.resize` / `ui.focus` | `{width, height}` | Window control |
| `app.open` / `app.save` / `app.quit` | `{path, password?}` or `{name?, base64, password?}` / `{path?}` | Files and lifetime. `app.open` opens a file, or a workbook's bytes (base64; `name`, default `Book.xlsx`, gives its title and format). `password` opens a password-protected workbook; without it, such a workbook returns an error (never a dialog) |

Any other method name that is a command id (e.g. `home.bold`) runs that command.

Use `engine.execute` with `{command: "view.theme", params: {mode: "system"}}`
to choose the display theme; `mode` accepts `"system"`, `"light"`, or `"dark"`.
System follows live OS appearance changes and falls back to Light when the platform supplies no preference.
The saved choice remains System; new users still start in Light. The legacy
`view.darkMode` command and `ui.set {dark}` select a fixed Light or Dark theme.

`{command: "view.animations", params: {on: false}}` turns motion in the UI off (the selection
outline then jumps to a new selection instead of gliding there); without `on` it toggles. It is
the View ▸ Animations checkbox, saved with the other UI preferences and on by default.

`{command: "app.language.set", params: {language: "ja"}}` switches the interface language
(`code` is accepted as an alias for `language`); `app.language.english` and
`app.language.japanese` are shortcuts. Only interface chrome is translated, never command ids or
document text. The choice is saved in `ui.json`; with nothing saved the desktop app follows the
system language.

`{command: "file.open", params: {path: "Secret.xlsx", password: "…"}}` opens a password-protected
workbook (Agile or Standard encryption). Without `password` it returns an error containing
"is password-protected. Enter its password to open it." and never opens a dialog; a wrong
password returns "The password is incorrect." The password is left out of the journal and never
quoted in an error. The opened workbook has no save path, so Save asks where to save it, without
the password, and never replaces the encrypted original.

For a headless equivalent (no window), use `gridcraft-cli mcp` or `gridcraft-cli run`;
see [`mcp.md`](mcp.md) and [`cli.md`](cli.md).

### Native printing

Desktop `engine.commands {"search":"print"}` (and connected MCP `list_commands`)
lists `ui.printSettings` and `ui.printSubmit` with their parameter and result fields.

`engine.execute` can call `ui.printSettings` with `{}` to inspect the printer list, selected
printer, copies, duplex, grayscale, discovery/submission status and backend receipt/error.
`{"refresh":true}` starts background discovery; poll the same command until `discovering` is
false. Set `printer` to a discovered name, integer `copies` from 1 to 999, `duplex` to
`printerDefault`, `off`, `longEdge` or `shortEdge`, and boolean `grayscale`. Invalid updates are
rejected without changing earlier settings. `ui.printSubmit` explicitly submits the current
print-ready PDF without opening a dialog; inspect `ui.printSettings` until `busy` becomes false
and check `status`/`detail`. The browser keeps its own print hook. PDF export remains separate.
GridCraft accepts the existing `file.print` sheet/pagination params on `ui.printSubmit`; the engine `file.print` command returns the PDF without spooling.

## Web

The browser build has no port to listen on. Opened in an `<iframe>` with `?host=parent`, it takes
the same methods from the page that embeds it, as `postMessage` messages. Its
`gridcraft:ready` message (see `apps/gridcraft-web/src/host.rs` for the other messages) lists
`"control"` in `capabilities`; send requests after it, to the frame's window with your own origin
as the target origin:

```json
{"type": "gridcraft:control", "id": 1, "method": "engine.execute", "params": {"command": "cell.set", "params": {"cell": "B2", "input": "=SUM(A1:A9)"}}}
```

GridCraft runs the requests at its next frame, in the order they arrived, together with the
workbooks sent as `gridcraft:open`: a request sent after a workbook works on that workbook. Each
request is answered in that frame, on the parent window:

```json
{"type": "gridcraft:reply", "id": 1, "ok": true, "result": {"cell": "B2", "value": "45"}}
{"type": "gridcraft:reply", "id": 1, "ok": false, "error": "…"}
```

- `id` (a number or a string) is required and comes back in the reply; match replies by it.
  `params` is an object and may be left out. A message GridCraft can't read (no `id`, `method`
  not a string, `params` not an object, not representable as JSON) is answered with an error
  under its `id`, or `id: null` when it has no usable one.
- A control message holds at most 16 MiB of JSON text (UTF-8 bytes; one over 16 Mi UTF-16 code
  units is refused before GridCraft reads it). Binary values (an `ArrayBuffer` or typed array)
  are refused: send bytes as base64 strings. Send a workbook as `gridcraft:open` rather than
  `app.open {base64}`: its bytes don't count against the limit.
- While the page is visible, the next frame is the next display refresh. While it is hidden,
  browsers wake it with timers instead: about once a second, and in Chrome as rarely as once a
  minute after five minutes hidden, so answers take that long.
- Input methods (`ui.click`, `ui.key`, `ui.text`, `ui.drag`…) answer `ok` at once; their effect
  lands over the following frames (a drag takes several), so read the outcome with a later
  request.
- Only the parent window is heard, and only when its messages come from the origin of
  GridCraft's own URL (`location.origin`); replies are posted to that origin only. When that
  URL has no origin (`location.origin` is `"null"`: a scheme the browser gives no origin, a
  `data:` URL), the parent can't be checked, so host mode stays off and GridCraft posts
  `gridcraft:failed` saying why (a fixed text, posted to any origin). A custom scheme the web
  view gives an origin works (WKWebView gives `tauri://localhost` the origin `tauri://localhost`).
- In a frame sandboxed without `allow-same-origin`, GridCraft's document has an opaque origin
  while `location.origin` stays its URL's: the parent's messages only reach it when posted with
  the target origin `"*"`, and its messages reach the parent with `event.origin` `"null"`, so
  the parent has to recognise them by `event.source`.
- The channel gives the parent nothing it doesn't have: a same-origin parent can already reach
  into the frame (`iframe.contentWindow`) and run any code there. Other origins are never heard.
- There is no file system. `app.open` takes the workbook's bytes (`{name?, base64, password?}`);
  `file.saveBytes {format?}` returns `{name, base64, bytes}`; `app.save` hands the workbook over
  as `gridcraft:save`, as the Save button does, and from that moment it counts as saved (an edit
  after it makes it unsaved again); `file.exportPdf` without a `path` returns
  `{pages, bytes, base64}`: a requested PDF is in the reply, not also handed over as
  `gridcraft:save` (as it is when the user exports from a menu).
- `ui.screenshot` isn't available (it is answered with an error); `ui.resize`, `ui.focus` and
  `app.quit` do nothing in a browser tab.
