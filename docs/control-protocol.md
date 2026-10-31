# GridCraft control protocol

Start the desktop app with a control port:

```sh
gridcraft --control 7979            # or GRIDCRAFT_CONTROL_PORT=7979
```

## Authentication

The control port is reachable by every program on the machine, and by a web page that POSTs
`text/plain` to `127.0.0.1:7979`, so every connection must prove it knows the app's token
(#777). **There is no way to turn this off.**

- The app makes a token at start (256 random bits from the operating system, kept in memory) and
  writes it to a file only you can read: `$XDG_RUNTIME_DIR/gridcraft/control-<port>.token` (else
  `~/.cache/gridcraft/…`; Windows: `%USERPROFILE%\.gridcraft\…`; the folder is mode 0700 and the
  file 0600 on Unix). The path is printed on stderr. The token itself is never printed or logged.
- Or choose the token yourself: `GRIDCRAFT_CONTROL_TOKEN=…` (preferred) or `--control-token …`
  (visible in the process list), at least 16 characters. The app then writes no file.
- The first line of a connection must be
  `{"id":0,"method":"auth","params":{"token":"…"}}`; the reply is
  `{"id":0,"ok":true,"result":{"authenticated":true}}`. Any other first line (a wrong or missing
  token, another method, text that isn't JSON such as an HTTP request, no line within 10 s, or
  one longer than 4 KiB) gets `{"ok":false,"error":"authentication required"}` and the
  connection is closed. At most 16 connections may be waiting to send their token at once; past that a new
  connection is closed before anything is read. After that, a line that isn't JSON also closes the connection.
- `gridcraft-cli send`/`mcp --connect` and `gridcraft_mcp::Remote` do this themselves: they use
  `GRIDCRAFT_CONTROL_TOKEN` if set, else the token file of that port (so they work for the same
  user on the same machine without setup). Your own client reads the file or the variable.

## Requests

Then send newline-delimited JSON to `127.0.0.1:7979` (after the `auth` line). Every request line gets one reply line:

```json
{"id": 1, "method": "engine.execute", "params": {"command": "cell.set", "params": {"cell": "B2", "input": "=SUM(A1:A9)"}}}
{"id": 1, "ok": true, "result": {"cell": "B2", "value": "45"}}
```

Errors come back as `{"id": …, "ok": false, "error": "…"}`. Asynchronous work that
finishes later than the reply (a `ui.screenshot` capture) adds its own `request` token to
the reply so the client can match it; a capture that completes after its 8 s deadline returns
the timeout error and writes no file.

A line that is not a JSON object with a string `method` (an HTTP request, text that isn't JSON,
bytes that aren't UTF-8, a line over 64 MiB) gets one error reply and the server closes the
connection; nothing sent after it runs. Blank lines are skipped.

## Methods

| Method | Params | What it does |
|---|---|---|
| `engine.execute` | `{command, params, window?, doc?}` | Runs any engine command (see `engine.commands` or `gridcraft-cli commands`) in the active window, or in the window named by `window` (a window id) or `doc` (a workbook id: its most recent window). The active window stays in front afterwards unless the command itself switches windows. `edit.goTo`, `edit.goToSpecial`, `selection.set` and `selection.move` scroll the active cell into view, as Go To does in the window |
| `window.list` | | The open windows: `[{window, doc, index, title, dirty, active, hidden}]`. `title` is the workbook name, or "Book1:1", "Book1:2"… when a workbook has several windows; `index` is the workbook's position among the open workbooks |
| `engine.commands` | `{search?}` | Every command: id, label, ribbon path, shortcut, params doc, enabled; `search` keeps those whose id, label or ribbon path contains it (case-insensitive) |
| `engine.journal` | | Commands run so far (replayable). Passwords (`password`, `…Password` params) are left out, so a replayed Protect Sheet has no password. Capped at 10,000 entries and 64 MB (serialized); past either cap the oldest entries go |
| `document.inspect` | | Workbook summary: sheets, used ranges, tables, charts, names, selection, undo labels |
| `sheet.read` | `{range?, sheet?, formulas?, formatted?}` | Values (or formulas / displayed text) of a range |
| `cell.get` | `{cell?}` | One cell: value, formula, displayed text, style, merge, comment, link |
| `ui.inspect` | | UI state: ribbon tab, editor, open dialog, Backstage page, message, grid rect, scroll, perf, `windows` (as `window.list`) and `activeWindow` |
| `ui.click` / `ui.drag` / `ui.move` | `{x, y, toX?, toY?, button?, count?, shift?, cmd?, alt?}` | Real pointer input in window points; refused with an error if it would leave more than 10,000 synthetic events or 8 MiB of synthetic input queued (pointer, key, text and move events share this budget) |
| `ui.cell.click` / `ui.cell.drag` | `{cell}` / `{from, to}` | Pointer input at a cell's centre (works in point mode while editing); same shared queue limits |
| `ui.key` / `ui.text` | `{key, shift?, cmd?, alt?, ctrl?}` / `{text}` | Keyboard input (typing in the grid starts editing); same shared queue limits, and one `ui.text` string may be at most 1 MiB |
| `ui.edit.begin` / `ui.edit.text` / `ui.edit.commit` / `ui.edit.cancel` | `{text?}` / `{move?: down\|right\|up\|left\|none}` | Drive the cell editor |
| `ui.ribbon` | `{tab}` | Switch ribbon tab |
| `ui.dialog` / `ui.dialog.set` / `ui.dialog.confirm` / `ui.dialog.cancel` | `{name}` / `{field, value}` | Open, fill in and confirm dialogs; `confirm` runs the dialog's command with the same parameters as its OK button |
| `ui.backstage` (via `engine.execute`) | `{page?: home\|new\|open\|info\|saveAs\|print\|export\|options\|account\|close}` | Open the File tab's Backstage view on a page, or close it (`ui.inspect` reports the open page as `backstage`) |
| `ui.screenshot` | `{path?}` | PNG of the window (base64 when no path). Animations are settled first: the picture shows the final state. On the native app it works while hidden or occluded when the platform still reports the current content size (the request asks for a hidden paint); a minimized window has no capturable size, so its request stays queued until the window is restored and otherwise reports the 8 s timeout. In a **hidden browser tab** the web runner does not implement that paint request, so the capture waits until the tab is shown again or reports the 8 s timeout. Up to 4 outstanding requests; retained images are limited to 128 MiB each and 256 MiB in total (each request reserves the largest image the ±1-pixel rounding rule can retain), one capture's transient encoding working set to 256 MiB, a base64 reply to 64 MiB, and retained images plus one capture's encoding working set to 512 MiB; a request that would exceed a budget is refused without evicting earlier ones. See [Screenshot budgets](#screenshot-budgets). A resize/DPI/zoom/visibility/occlusion change during capture is retried within the bounded retry budget or reported instead of saving stale pixels; the identity intentionally includes zoom and occlusion even though the expected physical size may be unchanged, so a capture never crosses layout generations. Every reply — success, refusal or timeout — carries the request `token` as `request` |
| `ui.set` | `{dark?, formulaBar?, ribbonCollapsed?, animations?}` | UI preferences (`formulaBar` runs `view.formulaBar`) |
| `ui.resize` / `ui.focus` | `{width, height}` | Window control |
| `app.open` / `app.save` / `app.quit` | `{path, password?}` / `{path?}` | Files and lifetime. `password` opens a password-protected workbook; without it, such a workbook returns an error (never a dialog) |
| `agent.listen` | `{timeoutMs?, agent?}` | Wait for an Ask Agent request (`{id, instruction, selection, ...}`) |
| `agent.reply` | `{id, text, done?, error?}` | Progress note or final answer for an Ask Agent request |

### Screenshot budgets

`ui.screenshot` keeps up to four requests outstanding. Each admitted request reserves the largest
image the ±1-pixel rounding rule can retain: 128 MiB per image and 256 MiB in total across
requests, and a request that would exceed a budget is refused without evicting earlier ones.
Encoding is bounded separately: one capture's transient working set may be 256 MiB (the larger of
the retained image plus its RGBA copy, or, for a base64 reply, the image plus the PNG stream plus
the encoded text), the base64 reply itself may be 64 MiB, and retained images plus one capture's
encoding working set may be 512 MiB. Base64 text counts toward the encoding working set, so a
large pathless capture can be refused there or by its base64 length even when its retained image
fits. `path` writes the PNG to a file instead of returning base64 text and so avoids those two
base64 costs, but it does not bypass the per-image, aggregate, encoding-working-set or peak
limits.

Any other method name that is a command id (e.g. `home.bold`) runs that command. Its params may
include `window` or `doc` to run it in that window, as with `engine.execute`; the window commands
(`window.*`, `view.newWindow`, `view.hideWindow`, `view.unhideWindow`, `view.switchWindows`) read
them as their own parameters.

### Windows

Every open workbook has one or more windows; View › New Window (`view.newWindow`) adds another
on the same workbook, and saving from any of them saves that workbook. Each window has its own
selection, active sheet and scroll position; switching windows never marks the workbook as
changed. `window.activate {window}`, `view.switchWindows {window}` and `view.unhideWindow
{window}` bring a window forward; `window.close {window?, force?}` closes one (the workbook
closes with its last window, asking about unsaved changes unless `force`); `file.close` closes
the active window, or with `{document: true}` the workbook and all its windows.

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
`app.language.japanese` are shortcuts. `engine.commands` lists every code it takes (`en`, `es`,
`zh`, `ja`, `ko`, `ru`, `pt`, `de`, `cs`, `fr`, `sr`), as does the error for an unknown one. Only
interface chrome is translated, never command ids or document text. The choice is saved in `ui.json`; with nothing saved the desktop app follows the
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

## Ask Agent

Desktop Ask Agent hands selected cells and an instruction to an external agent over authenticated control.
- `agent.listen {timeoutMs?, agent?}` claims the oldest waiting request or returns `{"none": true}`.
- `agent.reply {id, text, done?, error?}` sends progress notes (`done: false`) or a final answer (`done: true`).
- The agent edits with standard commands tagged with `request`. All edits form ONE undo group in the app.
- Only the connection that claimed the request may edit while it is open (`NOT_OWNER`).
