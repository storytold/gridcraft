# Office Scripts

GridCraft runs TypeScript (or JavaScript) programs against the open workbook through Microsoft's
**ExcelScript** object model. These are Office Scripts, not Office Add-ins: there is no `Office.js`,
no `Excel.run`, no task pane and no manifest. The runtime is [Bun](https://bun.sh) (MIT), which the
user downloads; it is never bundled or committed, the same policy as model weights.

GridCraft is local-first and runs scripts **offline**. Its script folder is a directory of `.osts`
files on disk. Excel only recognises scripts in OneDrive/SharePoint (or associated with the
workbook), so pointing GridCraft's folder at the same synced folder is the interop path:
`Documents/Office Scripts` in OneDrive, or a synced SharePoint library. Excel's script code is not
inside the `.xlsx`; a workbook may carry an association (and a script button) as workbook-side
metadata, and GridCraft preserves those parts unchanged through open/save instead of inventing an
in-workbook script format.

Saving in GridCraft writes a `.json` record in that folder, not an `.osts` file, so a script written
here reaches Excel only after **Export Script…** (`script.export`) writes the `.osts`; **Import
Script…** (`script.import`) brings an Excel script back. An `.osts` file dropped into the folder is
listed without an import step, but a saved record wins over an `.osts` with the same name or slug:
edits made later in Excel's editor do not appear until the file is imported again.

## Script storage and `.osts`

Scripts live in `<config>/scripts/` by default (Windows `%APPDATA%\GridCraft\scripts\`, macOS
`~/Library/Application Support/GridCraft/scripts/`, Linux/BSD
`$XDG_CONFIG_HOME/gridcraft/scripts/`). `script.folder` shows it, points it at another absolute
folder (created and write-tested) such as a synced `OneDrive/Documents/Office Scripts`, or resets
it with an empty path; the choice is kept in the preferences (per user, not per workbook).

Each script is stored as a GridCraft record (`<id>.json`) with its id, name, body, timestamps and,
for imported scripts, the `.osts` metadata. The Office transfer format is JSON version `0.3.0`:

```json
{ "version": "0.3.0", "body": "function main(workbook: ExcelScript.Workbook) { … }",
  "description": "…", "noCodeMetadata": "", "parameterInfo": "{\"version\":1,…}",
  "apiInfo": "{\"variant\":\"synchronous\",\"variantVersion\":2}" }
```

`parameterInfo` and `apiInfo` are JSON **strings**. `body` uses CRLF. `script.import {path}`
parses a file (bounded at 2 MiB, version `0.x`, a `main` function, syntax-checked) and keeps every
top-level field, including unknown ones, for `script.export {id, path}` to write back; `gridcraft`
metadata is stripped from an exported file. `script.osts` reports the format version, the parsed
`apiInfo`, and whether GridCraft honours it (only the synchronous variant exists). Malformed files
are structured errors, never a panic. Limits: 500 scripts, 120 characters per name, 100,000
characters of code.

`script.import` saves under a free name, so importing the same file twice makes a second record:
the first keeps the code it had. Run or delete the old record explicitly instead of assuming the
import replaced it.

## Finding Bun

1. `$GRIDCRAFT_BUN`; 2. `bun` on `PATH`; 3. `<model root>/runtimes/bun/bun(.exe)`, where the model
root is `$GRIDCRAFT_MODELS`, the platform settings folder's `models`, `models` beside the
executable, or `models` in the working directory.

On Windows, the npm `bun.cmd` shim is not used: it is a batch file, which a child process
cannot start directly. `$GRIDCRAFT_BUN` must point at `bun.exe` (`...\node_modules\bun\bin\bun.exe`),
and the `PATH` search looks for `bun.exe`, not `bun.cmd`.

Automate ▸ Office Scripts shows what was found and its version. The Automate tab's access key is
`U`, as in Excel. **Download Bun…** opens Bun's page
when the shell cannot fetch; **Import Bun…** copies an executable into the runtime folder and picks
it up without a restart. Without Bun, Run is disabled and `script.runtime` says what to install.

## The object model

The shim (`crates/script-bun/src/shim.rs`) defines these globals over a synchronous bridge. Members
not implemented throw a typed `OfficeScriptsError` whose message names the class and member
(`Range.getHyperlink is not supported…`), and the engine refuses the same way if one reaches it.

| Family | Backed by | Implemented members (summary) |
|---|---|---|
| `Workbook`, `Application` | `script.workbook`, `script.sheets`, `home.insertSheet` | active worksheet/cell, selected range(s), worksheets, names, add worksheet, dirty/name, application version through the workbook |
| `Worksheet` | `script.sheets`, `script.tables`, `script.charts`, `sheet.*` | getRange, getCell, getRangeByIndexes, getUsedRange, name/get/set, activate, delete, standard height/width, visibility, tables, charts |
| `Range` | `script.rangeInfo`, `range.setValues`, `home.*`, `edit.clear*`, `home.insert/deleteCells`, `home.merge*`, `selection.set` | values/text/formulas/number formats (get and set), address, dimensions, row/column index, getCell, offset, entire row/column, bounding rect, tables, data validation, format, insert/delete/clear/merge/unmerge/select |
| `RangeFormat`, `RangeFont`, `RangeFill`, `RangeBorder` | `script.rangeInfo` style facts; `home.bold/italic/underline/fontSize/fontName/fontColor/fillColor/numberFormat/home.drawBorder` | font bold/italic/underline/size/name/color/strikethrough, fill colour, number format, alignment reads, one border by side |
| `RangeAreas` | `script.selection` | areas, address (format applies to one area only, so it is `not-yet`) |
| `Table`, `TableColumn` | `script.tables`, `table.*` | range/header/total/body ranges, name, columns, column ranges/index/name, headers/totals, style, convert, delete |
| `Chart` | `script.charts`, `chart.set/changeType/delete` | type, name, left/top/width/height, setPosition, delete (active worksheet) |
| `Comment` | `cell.get`, `review.newComment/deleteComment` | content, author name, set content, delete |
| `DataValidation` | `script.validation`, `data.validation` | type, rule, error alert, prompt, ignore blanks (get and set), clear |
| `NamedItem` | `formulas.nameManager` | name, formula/refers-to, scope, comment, type, visible |
| global `console` | bridge `console.log` | log/info/warn/error, bounded |
|global `ExcelScript`|the shim's enum namespace|`BorderIndex`, `BorderLineStyle`, `BorderWeight`, `ChartType`, `ClearApplyTo`, `DataValidationType`/`DataValidationOperator`, `InsertShiftDirection`/`DeleteShiftDirection`, `SheetVisibility`; an unknown family or member throws `notSupported` |

Exact per-member status is generated: [office-scripts-coverage.md](office-scripts-coverage.md).
Run `cargo xtask office-scripts-coverage --check`; the test in `gridcraft-script` fails when the
report is stale. Sections not backed by the engine yet (events, tables of contents, slicers,
PivotTable members, images, conditional formatting objects, chart axes/series formatting,
`getUsedRange` variants beyond the worksheet, custom XML, bindings) are `not-yet` and throw the
typed error above.

## The bridge

The runtime is a child process. Its stdin/stdout carry one JSON object per line:

- parent → runtime: `{"script": "...", "limits": {"maxCalls": …, "maxOutputBytes": …}}`;
- runtime → parent: `{"id": 7, "method": "Range.setValue", "params": {"range": "A1", "value": 42}}`;
- parent → runtime: `{"id": 7, "result": null}` or `{"id": 7, "error": {"code": "engine", "message": "…"}}`;
- runtime → parent, once: `{"done": true}` after `main` returns. Output that ends without it — a
  crash, or a script that calls `process.exit` — fails the run and rolls the workbook back.

Each call becomes a normal engine command through `Session::execute`, so a script gets the same
journaling, protection, arrays, spills and undo as the ribbon. A run is an undo group: every call
is one step, and the whole run is **one undo step**. A syntax error, a runtime error, a refused
call or a timeout rolls the workbook and its undo history back. Scripts may `try`/`catch` a call's
error; an uncaught one fails the run.

Bounds: 100,000 characters of code, 10,000 calls, 30 s default (100 ms … 600 s with
`script.run {timeoutMs}`), 1 MiB of console output, 8 MiB per line, 16 MiB of runtime output. On
timeout the child is killed. A malformed line, a broken pipe or a crashed runtime is an error the
pane shows, never a panic. Errors carry the runtime's line when it reports one, and the pane
underlines that line.

## The trust model

Office Scripts run as native code: Bun is a full JavaScript runtime, so a script can read and write
files, start processes and reach the network. Excel sandboxes Office Scripts; GridCraft does not,
and cannot without a separate OS-level sandbox. What the shim does do:

- `process`, `Bun`, `require`, `fetch`, `module`, `exports` and `globalThis` are shadowed as
  `undefined` in the script's scope, so an ordinary script cannot reach them by accident. This is
  not a sandbox: `Function("return process")()` and a dynamic `import("node:fs")` still reach the
  real runtime.
- Bun starts with a cleared environment (`NO_COLOR` and the user's home folder, plus the
  directories Windows needs to start a process), so the user's tokens and settings are not in the
  script's `env`.
- The shim folder and file are created exclusively under the temporary folder, so a file planted
  there is never reused. On Unix the folder is set to `0700` and the file to `0600`; on Windows
  access depends on the temporary folder's ACL.

A script only runs when the person clicks Run (or an agent calls `script.run`). A script from
`script.import`, or from a custom scripts folder, has exactly the same access as one written in the
pane: treat a shared `.osts` like a macro from someone else.

## The recorder's command subset

**Record** turns the command journal (from where recording started to where it stopped; recording
controls are left out) into a script. It writes only what the bridge can replay:

| Command | Script |
|---|---|
| `cell.set` | `…getRange("A1").setValue(…)` |
| `range.setValues` | `…getRange("A1:B2").setValues(…)` |
| `home.bold` with `on` | `…getFormat().getFont().setBold(true)` |
| `home.numberFormat` | `…setNumberFormat("0.00")` |
| `home.insertSheet` | `workbook.addWorksheet("Name")` |
| `home.deleteSheet` | `workbook.getWorksheet("Name").delete()` |

Anything else becomes a comment naming the command and why it was skipped (including a command
that used the active selection without naming a range). A command targeting a sheet records the
sheet's name. The generated file is a valid `main(workbook)` Office Script and loads into the
editor, where it can be edited, run, saved and exported.

## Verification and limits

- `.osts` round trips preserve CRLF bodies, unknown fields and metadata; the editor and recorder
  emit a `main(workbook)` signature.
- A test records a sequence, replays the generated bridge calls through a fake host and checks the
  workbook matches; the real Bun run is an `#[ignore]` integration test (`GRIDCRAFT_BUN`).
- Excel needs a Microsoft 365 commercial/education account signed in for the Automate tab. When it
  is unavailable, Excel interop is verified by format round-trip and by pasting the exported body
  into Excel's editor; the PR states which path was used.
- The exported script was run inside Excel 16.0: the `.osts` was placed in
  `Documents\Office Scripts`, opened from Automate › View Scripts (Script details › Edit), its body
  pasted into the editor and Run. Excel's `Book1` then had `A1 = "from gridcraft excel"` in bold
  and a new `ExcelRun` sheet — the same result GridCraft produced, read back through Excel COM.
- Excel round-trip checked with Excel 16.0 COM: a workbook with values, a formula, number
  formats, a merge, a table, a comment, validation and a chart opened and saved by GridCraft
  re-opens in Excel without a repair prompt, and value/formula/number-format/table/comment/
  validation/merge/chart facts match. The only package part not carried was the optional
  `xl/calcChain.xml` (follow-up issue #3565).
- Known limits: no events, no `getUsedRange` outside the worksheet, no async API; chart and table
  operations are on the active worksheet; `setValue` on a multi-cell range sets the top-left cell
  (use `setValues`); runtime-reported line numbers can be off by the transpiler wrapper's few
  lines; the web build has no child processes and disables Office Scripts.

## Follow-up per object family

| Family | Next |
|---|---|
| `Range` | `getHyperlink`, `find`, `autoFill`, `copyFrom`, conditional-format objects, hidden rows/columns |
| `RangeFormat` | alignment setters, wrap/shrink/indent, column width/row height, protection, text orientation |
| `Worksheet` | tables/charts by name, used ranges of areas, page layout, freeze panes, protection |
| `Workbook` | tables/charts getters, comments list, active chart/slicer, calculation settings |
| `Table` | rows (add/delete), filters, sort, banded/highlight flags, resize |
| `Chart` | axes, series, titles, legend, plot area, data labels, styles |
| `Comment` | replies, resolved state, rich content, mentions |
| `DataValidation` | `getValid`/`getInvalidCells`, list helpers |
| Office-host-only families (bindings, custom XML, slicers, PivotTables, shapes/images, named sheet views) | out of scope until their engine models grow; each member stays a typed `notSupported` error |

## Commands

| Command | What it does |
|---|---|
| `script.list` | saved scripts |
| `script.save {name, code}` | saves (same name replaces), syntax-checked |
| `script.delete {id}` | removes a saved script |
| `script.run {id \| code, timeoutMs?}` | one undo step; nested commands journaled |
| `script.runtime` | found/missing, version, path, expected path, download URL |
| `script.record {on, name?}` | records the journal into a new script |
| `script.folder {path?}` | shows, sets or resets the scripts folder |
| `script.import {path, name?}` / `script.export {id, path}` | `.osts` in and out |
| `script.osts {id?}` | file format, API variant, unknown fields |
| `script.workbook`, `script.rangeInfo`, `script.sheets`, `script.selection`, `script.tables`, `script.charts`, `script.validation` | bridge-support queries |

`script.run` has no journal entry of its own. Programmatic calls never open a dialog. Commands are
automatically available to the CLI, the control channel and MCP.
