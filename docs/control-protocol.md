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
| `engine.execute` | `{command, params, window?, doc?, wait?, waitSeconds?}` | Runs any engine command (see `engine.commands` or `gridcraft-cli commands`) in the active window, or in the window named by `window` (a window id) or `doc` (a workbook id: its most recent window). The active window stays in front afterwards unless the command itself switches windows. `edit.goTo`, `edit.goToSpecial`, `selection.set` and `selection.move` scroll the active cell into view, as Go To does in the window. `wait` and `waitSeconds` hold the reply until the workbook's Python is idle: see [Saving and waiting for Python cells](#saving-and-waiting-for-python-cells) |
| `window.list` | | The open windows: `[{window, doc, index, title, dirty, active, hidden}]`. `title` is the workbook name, or "Book1:1", "Book1:2"… when a workbook has several windows; `index` is the workbook's position among the open workbooks |
| `engine.commands` | `{search?}` | Every command: id, label, ribbon path, shortcut, params doc, enabled; `search` keeps those whose id, label or ribbon path contains it (case-insensitive) |
| `engine.journal` | | Commands run so far (replayable). Passwords (`password`, `…Password` params) are left out, so a replayed Protect Sheet has no password. Capped at 10,000 entries and 64 MB (serialized); past either cap the oldest entries go |
| `document.inspect` | | Workbook summary: sheets, used ranges, tables, charts, names, selection, undo labels |
| `sheet.read` | `{range?, sheet?, formulas?, formatted?}` | Values (or formulas / displayed text) of a range |
| `cell.get` | `{cell?}` | One cell: value, formula, displayed text, style, merge, comment, link |
| `ui.inspect` | | UI state: the saves waiting for Python cells (`saveWaits`: see [Saving and waiting for Python cells](#saving-and-waiting-for-python-cells)), ribbon tab, editor (`python` is true in Python mode; a draft in Python mode or of a Python cell's code has `text` null (a draft typed over a Python cell holds only what was typed and is shown) and `hidden` true while that cell's code is hidden on a protected sheet, or once the draft's sheet is gone; the sheet is found by its identity, whatever was inserted, deleted or moved before it), the Python editor window (`pythonEditor`: open, cell, text, dirty, pending question; `text` is null and `hidden` true while the cell's code is hidden on a protected sheet, even before the next frame closes the window), open dialog, Backstage page, message, selected object, grid rect, scroll, perf, `windows` (as `window.list`) and `activeWindow` |
| `ui.object.select` | `{id?, kind?}` | Selects a floating object on the active sheet and replies `{selected, rect}`, the rect in window points; with no `id` it clears the selection and replies `null` |
| `ui.click` / `ui.drag` / `ui.move` | `{x, y, toX?, toY?, button?, count?, shift?, cmd?, alt?}` | Real pointer input in window points; refused with an error if it would leave more than 10,000 synthetic events or 8 MiB of synthetic input queued (pointer, key, text and move events share this budget) |
| `ui.cell.click` / `ui.cell.drag` | `{cell}` / `{from, to}` | Pointer input at a cell's centre (works in point mode while editing); same shared queue limits |
| `ui.key` / `ui.text` | `{key, shift?, cmd?, alt?, ctrl?}` / `{text}` | Keyboard input (typing in the grid starts editing); same shared queue limits, and one `ui.text` string may be at most 1 MiB |
| `ui.edit.begin` / `ui.edit.text` / `ui.edit.commit` / `ui.edit.cancel` | `{text?}` / `{move?: down\|right\|up\|left\|none}` | Drive the cell editor |
| `ui.ribbon` | `{tab}` | Switch ribbon tab |
| `ui.dialog` / `ui.dialog.set` / `ui.dialog.confirm` / `ui.dialog.cancel` | `{name}` / `{field, value}` | Open, fill in and confirm dialogs; `confirm` runs the dialog's command with the same parameters as its OK button. `confirm` applies to a dialog whose OK runs a command (Solver, Data Analysis, the form dialogs); a dialog that presses its own OK, such as Goal Seek, answers `this dialog has no OK command; drive it with its own commands` and is driven with that command (`data.goalSeek`). That command does not go back to the dialog's sheet: after another sheet was shown, read the fields (`ui.inspect` reports the dialog's `values`), `cancel` the dialog (which shows its sheet again), then run `data.goalSeek` with those values, so references without a sheet name mean the dialog's sheet. Once the dialog's own sheet is deleted, `cancel` leaves the sheet shown as it is and replies `{"sheetDeleted": true}` (the sheet is found by identity, so a new sheet given its name does not count); the script then stops instead of running `data.goalSeek`, whose references without a sheet name would mean the sheet shown. A dialog with reference boxes (RefEdit) goes back to its own sheet, the one shown when it was opened (a sheet shown later, before any pick, does not take its place): `cancel` puts the selection from before picking back; `confirm` puts it back before the command runs (so references without a sheet name mean the dialog's sheet). Both fail, with the dialog left open, when a text-box draft on the picked sheet cannot be saved (its worksheet is no longer the active one), and `confirm` also once the dialog's sheet was deleted; the window's own Cancel closes the dialog in that case and shows the reason. |
| `ui.pythonMode` (via `engine.execute`) | `{on?}` | Python mode on the active cell, in the formula bar (an edit in progress keeps its text); `on: false` turns it off; without `on` it switches. A start that is refused (a locked cell on a protected sheet, a cell or draft inside another cell's spill, code hidden on a protected sheet) is an error with the reason. Opens no dialog |
| `ui.pythonEditor` (via `engine.execute`) | `{on?, cell?, force?}` | Open the Python editor window on `cell` (`A1` or `Sheet2!B3`; default: the active cell; a cell inside a Python cell's spill opens its Python cell), or close it with `on: false`; without `on` it opens or closes. Opens no dialog and never asks: with unsaved code in the window it refuses to close it or open it on another cell, unless `force: true` discards the code. Refused on a Hidden cell of a protected sheet, whose code stays out of view. Replies with the window's state. `file.close`, and `window.close` on the workbook's last window, are refused while the window has unsaved code for the workbook; `app.quit` is refused while it has unsaved code |
| `ui.pythonInitialization` (via `engine.execute`) | `{on?, force?}` | Open the Python editor window on the workbook's initialization code, or close it with `on: false`; without `on` it opens or closes. Opens no dialog and never asks, as `ui.pythonEditor`. Replies with the window's state, with `initialization: true` and no `sheet` or `cell` |
| `ui.pythonCard` (via `engine.execute`) | `{on?}` | Open the card of the active cell's Python cell (its cached result's type and preview), or close it with `on: false`. Opening it is an error when the active cell isn't a Python cell, or while a cell is being edited; closing it always succeeds |
| `ui.saveWait` (via `engine.execute`) | `{action?: saveNow\|cancel}` | The active workbook's save waiting for Python cells: `saveNow` saves without waiting any longer, `cancel` ends the wait with no save. Replies as `saveWaits` in `ui.inspect` has it, or `{waiting: false}`. See [Saving and waiting for Python cells](#saving-and-waiting-for-python-cells) |
| `ui.pythonEnvironment`, `ui.pythonPackages`, `ui.pythonProblems`, `ui.pythonRunLog`, `ui.pythonPermissions` (via `engine.execute`) | `{on?}` | Open or close a Python pane, in place of any other pane; without `on` it switches. Replies `{open}`; `ui.inspect` reports them. Opening a pane is safe from any caller: none of them turns Python on |
| `ui.backstage` (via `engine.execute`) | `{page?: home\|new\|open\|info\|saveAs\|print\|export\|options\|account\|close, section?: general\|python}` | `section` picks File › Options' page. Open the File tab's Backstage view on a page, or close it (`ui.inspect` reports the open page as `backstage`) |
| `ui.screenshot` | `{path?}` | PNG of the window (base64 when no path). Animations are settled first: the picture shows the final state. On the native app it works while hidden or occluded when the platform still reports the current content size (the request asks for a hidden paint); a minimized window has no capturable size, so its request stays queued until the window is restored and otherwise reports the 8 s timeout. In a **hidden browser tab** the web runner does not implement that paint request, so the capture waits until the tab is shown again or reports the 8 s timeout. Up to 4 outstanding requests; retained images are limited to 128 MiB each and 256 MiB in total (each request reserves the largest image the ±1-pixel rounding rule can retain), one capture's transient encoding working set to 256 MiB, a base64 reply to 64 MiB, and retained images plus one capture's encoding working set to 512 MiB; a request that would exceed a budget is refused without evicting earlier ones. See [Screenshot budgets](#screenshot-budgets). A resize/DPI/zoom/visibility/occlusion change during capture is retried within the bounded retry budget or reported instead of saving stale pixels; the identity intentionally includes zoom and occlusion even though the expected physical size may be unchanged, so a capture never crosses layout generations. Every reply — success, refusal or timeout — carries the request `token` as `request` |
| `ui.set` | `{dark?, formulaBar?, ribbonCollapsed?, animations?}` | UI preferences (`formulaBar` runs `view.formulaBar`) |
| `ui.resize` / `ui.focus` | `{width, height}` | Window control |
| `app.open` / `app.save` / `app.quit` | `{path, password?}` / `{path?, wait?, waitSeconds?}` | Files and lifetime. `password` opens a password-protected workbook; without it, such a workbook returns an error (never a dialog). `app.save` is `file.save`. `app.quit` doesn't ask about unsaved workbooks; it is refused while the Python editor window has unsaved code, and while a save is waiting for Python cells |

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

`ui.inspect` reports the selected floating object as `selectedObject: {kind, id}` (or `null`),
where `kind` is `chart`, `image`, `shape`, `equation`, `slicer` or `control` (a form button, slider or spin
button); the same names go in the `kind` parameter
of `object.delete`, `object.move`, `object.setAnchorMode` and the `arrange.*` commands. An
equation is a shape too, so its id works under `shape`; a plain shape's id given as `equation`,
a `kind` that is present but not one of these names (`null` included), or an id of no object of
the kind, is an error. `arrange.align` and `arrange.group` check every id in `ids` the same way;
with fewer than two ids they change nothing.
Form controls are worked with `control.setValue` and `control.press`, and listed by `control.list`.
`object.move` and `chart.set` cap `width` and `height` at 10,000 points, or at the object's
current size when a file made it larger: such an object keeps its size when it is moved and
shrinks by what a resize asks, but does not grow further.
`selectedChart` remains for existing clients: the id when the selection is a chart, otherwise
`null`. With an object selected, `ui.key {key: "Delete"}` (or Backspace) deletes the object
instead of clearing cells, and Escape clears the selection.

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

## Python cells

`engine.execute` reaches the `python.*` commands: `python.set`, `python.output`, `python.run`,
`python.runAll`, `python.stop`, `python.reset`, `python.initialization`, `python.calculation`,
`python.table`, `python.saveCsv`, `python.diagnostics`, `python.runLog`, `python.clearRunLog`,
`python.environment` and `python.refreshEnvironment`. None opens a dialog.

Python runs only after a person turned it on in File › Options › Python, and a workbook from a file
only after a person enabled it. Nothing here can do either, or choose the interpreter or the limits:
they aren't commands, `ui.set` has no such field, and those widgets refuse the input `ui.click`,
`ui.drag`, `ui.key` and `ui.text` send, whatever they are aimed at. Until then these commands report
why Python doesn't run. That doesn't cover someone who can edit the preferences file on disk, or who
operates the real mouse and keyboard or the system's accessibility interface.

`python.initialization {code?}` is the workbook's initialization code, which runs before its cells in
a fresh worker. With `code` it sets it, as one undo step, and every cell runs again from the top; it is
refused while the workbook's structure is protected, and over 65,536 characters. Without `code` it only
reports. It replies `{code, userModified, fits, diagnostic?}`. `userModified` is false for what the
application that wrote a file put there, where what can't be imported is skipped and reported as
`init.skipped` rather than stopping the cells. `fits` is false for a file's code cut at the limit, which
doesn't run. `diagnostic` is the `init.failed` every cell shows when it failed, with Python's message as
`detail` and its `line`. `python.environment` has that diagnostic too, as `initialization`, and
`python.diagnostics` lists it, and `init.skipped` with the module as `detail` and the name to install as
`install`, once each with `initialization: true` and no `cell`. In `python.runLog` a run of it has
`initialization: true` and no `sheet` or `cell`.

A Python Object cell's value names the object and its size (`DataFrame 6x4`), and `cell.get` has the
`python.typeName` (`DataFrame`) and, for a table-like object, `python.table`, a summary as below.
`python.table {cell?, sheet?, format?: json|csv|markdown}` gives the table the cell keeps from its last
run: `{cell, rows, cols, totalRows, totalCols, rowLabels, textCut, cut}` with `{columns, data}` for
`json` (the default), and `{text}` for `csv` or `markdown`. It holds at most 200 rows, 30 columns and
1 MiB of text. `totalCols` doesn't count the row labels, and `cut` is true when anything of the object
isn't there. A cell with no table is an error: a flat list and a dict have none, and a file doesn't keep
one, so a cell opened from a file must run first.

`python.saveCsv {cell?, sheet?, path}` writes that table's CSV, adding `.csv` to a path without it, and
replies `{cell, path, bytes, rows, cols, totalRows, totalCols, rowLabels, textCut, cut}`. Without a path
it is an error. A macro step needs the macro's permission, as for the other commands that write a file,
and a script can't write files.

`python.output {cell?, mode?, lines?: cell|rows}` also says what a text result with line breaks does: it
stays in the cell (`cell`, the default) or spills one line per row (`rows`), with no run. With `lines` and
no `mode` the return type stays. `cell.get` reports it as `python.lines`. GridCraft JSON keeps it; an xlsx
file doesn't.

`ui.pythonInitialization {on?, force?}` opens the Python editor window on that code, and closes it; it
replies as `ui.pythonEditor` does, with `initialization: true`, `title: "Initialization"` and no `sheet`
or `cell`, and never asks: with unsaved code it refuses, unless `force` discards them. `ui.inspect`
reports the same state as `pythonEditor`.

`ui.inspect` reports `python`: `{state: off|notSetUp|ready|running|blocked, reason?, enabled,
workbookEnabled, hasCells, enableBar, calculation, interpreter?, busy, unfinished, running? {sheet, cell, ms} or
{initialization: true, ms}, initialization? {userModified, fits, chars, failed? {detail, line}, skipped},
findings: {formulaBar, editor}, optionsPage, panes: {environment, packages, problems, runLog, permissions}}`. `state` is what the
Python tab's button shows; `reason` is a diagnostic's id from `python.diagnostics`' vocabulary.
`busy` says Python is working on any workbook, or the check of Python hasn't answered; `unfinished`
names the active workbook's cells that are running or queued, as `Sheet1!B2`.
`findings` lists what the checks found in the code being edited in the formula bar's Python mode and
in the editor window: `{kind, severity, message, detail, line?, start?, end?}`, the last run's
diagnostic first when there is one, then the static checks in order of position, with `start` and
`end` byte offsets into the current text. They come from the current text, and `line` and the offsets
of the last run only while the text is the code that ran. A list is empty when that editor isn't open
or its code is hidden on a protected sheet. Static checks need no Python, so they work with Python off
and with no sandbox; a missing package is only reported once the package list is known.
`python.diagnostics` also reports `packages.missing` with the module as `detail`, the first cell that
imports it as `cell` and the name to install as `install`, and `packages.noPandas`.

The Python tab is contextual: `ui.ribbon {tab: "Python"}` shows it only while the active cell is a
Python cell or Python code is being edited, and otherwise the ribbon goes back to Home. Its panes open
by their commands either way.

### Saving and waiting for Python cells

`file.save` and `file.saveAs` through the control channel, also as `app.save` or called by their
ids, never show a dialog: a workbook with no file and no `path`, and `file.saveAs` with no `path`,
is the error ``missing `path`: programmatic calls never open dialogs``.

Without `wait` a save happens at once, whatever is running. A cell still running or queued stores
its last result; one that never had a result stores none, and its cached value in the file is
`#VALUE!`, as the format has no code for `#BUSY!`. The code is always kept. The reply lists those
cells as `unfinished` when there are any.

`wait: true` holds the reply until the workbook's running and queued cells have finished, then
saves. A cell stale under manual calculation isn't one of them. The wait ends by itself after the
time Python may need to start, plus the time limit for each cell left, plus one second, and never
after 120 seconds. `waitSeconds: n` makes the bound `n` seconds, cut to 120; `wait: false` means no
wait whatever `waitSeconds` says. Anything else in these two fields is an error, and nothing is saved.
They go in the save's own `params`, or beside `command` in `engine.execute`.

```json
{"id": 2, "method": "engine.execute", "params": {"command": "file.save", "params": {"path": "/tmp/book.xlsx", "wait": true}}}
{"id": 2, "ok": true, "result": {"path": "/tmp/book.xlsx", "bytes": 5120, "unfinished": [], "unfinishedCount": 0}}
```

`unfinished` names the cells that have no result of their own at the moment of the save, in run
order, at most 1,000 of them, and `unfinishedCount` says how many there are. They were still running
when the bound came, or `python.stop` or Python being turned off left them, or a worker kept going
away. The save happens either way, unless it fails, which is an `ok: false` reply with the workbook
as it was.

While a reply is held the window keeps working and other requests go on, on other connections: a
connection answers its requests in order. Nothing shows in the window for a programmatic wait. A
second save of the workbook joins the wait that is there and doesn't start it over, and a shorter
`waitSeconds` of its own still holds. Each file is written once, in the order asked for, and a reply
comes for each request. If the workbook closes first, or someone cancels the wait, the reply is an
error saying so, and nothing is written. `app.quit` is refused meanwhile.

`engine.execute` of any other command takes `wait` and `waitSeconds` beside `command`: the command
runs, then the reply holds until Python is idle in the workbook it ran in, with the same bound, and
the result has `unfinished` and `unfinishedCount`. `value`, in a result that has `cell` and `value`,
is what the cell holds at that moment. A command that fails answers at once. When the workbook was
closed meanwhile the result has `workbookClosed: true`.

`ui.inspect` reports `saveWaits`: `[{waiting, doc, ms, capMs, prompt, saves, closeAfter, quitting,
unfinished, unfinishedCount}]`, one for each workbook with a save waiting. `prompt` says a person asked,
so the bar shows; `saves` lists the files it will write; `closeAfter` says the workbook closes after
(the Save of Save Changes), and `quitting` that the quit goes on from there. `ui.saveWait` ends a wait
from a script, as the bar's Save Now and Cancel do.

A client must wait for a held reply: the app's control server, `gridcraft-cli send` and
`gridcraft_mcp::Remote` allow the 120 seconds on top of their usual timeouts for a request that asks
to wait (`control::reply_timeout`, `python::reply_may_be_held`). The CLI and MCP without an app never
run Python, so `wait` changes nothing there but adding an empty `unfinished` to the reply.
