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

## Methods

| Method | Params | What it does |
|---|---|---|
| `engine.execute` | `{command, params}` | Runs any engine command (see `engine.commands` or `gridcraft-cli commands`) |
| `engine.commands` | `{search?}` | Every command: id, label, ribbon path, shortcut, params doc, enabled; `search` keeps those whose id, label or ribbon path contains it (case-insensitive) |
| `engine.journal` | | Commands run so far (replayable) |
| `document.inspect` | | Workbook summary: sheets, used ranges, tables, charts, names, selection, undo labels |
| `sheet.read` | `{range?, sheet?, formulas?, formatted?}` | Values (or formulas / displayed text) of a range |
| `cell.get` | `{cell?}` | One cell: value, formula, displayed text, style, merge, comment, link |
| `ui.inspect` | | UI state: ribbon tab, editor, open dialog, message, grid rect, scroll, perf |
| `ui.click` / `ui.drag` / `ui.move` | `{x, y, toX?, toY?, button?, count?, shift?, cmd?, alt?}` | Real pointer input in window points |
| `ui.cell.click` / `ui.cell.drag` | `{cell}` / `{from, to}` | Pointer input at a cell's centre (works in point mode while editing) |
| `ui.key` / `ui.text` | `{key, shift?, cmd?, alt?, ctrl?}` / `{text}` | Keyboard input (typing in the grid starts editing) |
| `ui.edit.begin` / `ui.edit.text` / `ui.edit.commit` / `ui.edit.cancel` | `{text?}` / `{move?: down\|right\|up\|left\|none}` | Drive the cell editor |
| `ui.ribbon` | `{tab}` | Switch ribbon tab |
| `ui.dialog` / `ui.dialog.set` / `ui.dialog.confirm` / `ui.dialog.cancel` | `{name}` / `{field, value}` | Open, fill in and confirm dialogs |
| `ui.screenshot` | `{path?}` | PNG of the window (base64 when no path) |
| `ui.set` | `{dark?, formulaBar?, ribbonCollapsed?}` | UI preferences |
| `ui.resize` / `ui.focus` | `{width, height}` | Window control |
| `app.open` / `app.save` / `app.quit` | `{path}` / `{path?}` | Files and lifetime |

Any other method name that is a command id (e.g. `home.bold`) runs that command.

Use `engine.execute` with `{command: "view.theme", params: {mode: "system"}}`
to choose the display theme; `mode` accepts `"system"`, `"light"`, or `"dark"`.
System follows live OS appearance changes and falls back to Light when the platform supplies no preference.
The saved choice remains System; new users still start in Light. The legacy
`view.darkMode` command and `ui.set {dark}` select a fixed Light or Dark theme.

## Language and regional format

The interface language, the formula language and the regional format are session preferences. Formulas, number
formats and `input` params are **canonical** (English function names, `,` between arguments, `.` as decimal point;
this is also what XLSX files, the journal and `engine.journal` hold); the `…Local` variants carry the text as a user of
the current language types and sees it.

| Command | Params | Result |
|---|---|---|
| `app.setLocale` | `{uiLanguage?, formulaLanguage?, regionalFormat?, useSystemSeparators?, decimalSeparator?, thousandsSeparator?}` | The `app.getInternational` payload after the change. `uiLanguage` and `regionalFormat` are `"system"` or a tag (`pt-BR`); `formulaLanguage` is `"followUi"` or `"en-US"`; the separators are one character and apply when `useSystemSeparators` is false. Applies at once: every open workbook recalculates (automatic mode) or marks its cells pending (manual mode) and the UI reloads. No dialog, not undoable. Omitted params keep their value; an invalid value changes nothing. |
| `app.getInternational` | | `{uiLanguage, formulaLanguage, regionalFormat}` (resolved tags), `decimal`, `thousands`, `list`, `arrayColumn`, `arrayRow`, `dateOrder`, `dateSeparator`, `timeSeparator`, `shortDate`, `longDate`, `r1c1: [row, column]`, `boolTrue`, `boolFalse`, `currency` and `prefs` (the six raw preferences). |
| `app.languages` | | `{languages: [{tag, nativeName, functionsTranslated}], regions: [{tag, nativeName}]}`. `functionsTranslated` is false for languages that keep English function names (ja-JP, ko-KR, zh-CN, zh-TW). |

Commands that take or return text a user types or sees:

- `cell.set`, `range.fill`: `inputLocal` (typed text, e.g. `=SOMA(1,5;2)`, `1,5`, `10/10/2026`) instead of `input`; the
  optional `locale` tag overrides the session language and region for that call. `inputLocal` follows the same sheet
  protection as `input`: a locked cell on a protected sheet is refused with the `protected-sheet` error code.
- `cell.get`, `document.inspect`, and the commands that list names, conditional formats and validations: `formula` and
  `formulaLocal`, `numberFormat` and `numberFormatLocal`; the formula-taking commands accept the `…Local` params.
- A formula that is formatted Hidden on a protected sheet is shown in neither spelling: `formula` and `formulaLocal`
  are `null` in `cell.get`, `formulas.watchWindow` and `formulas.errorChecking`, `input` and `inputLocal` hold the
  value instead, `sheet.read {formulas: true}` returns the value, `formulas.evaluateFormula {cell}` is refused, and
  `edit.find` / `edit.replace` do not look inside it.
- Number-format commands accept `numberFormatLocal` (`#.##0,00`, `dd/mm/aaaa`).
- `formulas.functions`: each item has `name` (canonical) and `localName`.

The desktop app detects the system language at every start; the choice made with `app.setLocale` is saved in
`prefs.json` (`~/.config/gridcraft/` on Linux, `~/Library/Application Support/GridCraft/` on macOS, `%APPDATA%\GridCraft\`
on Windows) and in the browser's `localStorage` (`gridcraft.prefs`) for the web build.

The UI command `app.language.set` accepts `{language: "ja"}` (or the `code` alias) and delegates to
`app.setLocale {uiLanguage: "ja-JP"}`. Short codes `en`, `pt`, `zh`, `ja`, `ko` and `ru` select
`en-US`, `pt-BR`, `zh-CN`, `ja-JP`, `ko-KR` and `ru-RU`; full tags and regional variants negotiate
within a supported language. Unknown languages are rejected. `app.language.english` and
`app.language.japanese` use the same path. The result is the `app.setLocale` payload plus `language`
(the resolved tag). These commands change only the interface preference; the formula language
continues to follow it when `formulaLanguage` is `"followUi"`. Existing document text and canonical
command ids remain unchanged. Language preferences are saved in `prefs.json`, not `ui.json`.

## Data import and displayed exports

`file.open` supports ODS and XLSB data import, including packages detected by content rather than
their extension. Imported formula caches become constant values; formulas and number formats
stored in GridCraft remain canonical. The returned `warnings` describe import limitations and are
also shown by the UI. An imported document has no save target; Save asks for a new destination.
ODS and XLSB are import-only formats. Password-protected workbooks return `encrypted-workbook`,
legacy .xls files return `legacy-workbook`, import failures return `import-failed`, and attempts to
save as ODS or XLSB return `import-only-format`.

`edit.copy {html: true}` returns an HTML table alongside plain text when within the clipboard limits.
Both use the workbook’s current locale and displayed cell text; the internal clipboard retains
canonical formulas and values. HTML file exports use the same displayed-cell path.

`shape.setText {id, text}` edits a text box without interpreting its text as a formula. It is undoable,
refuses protected sheets (`protected-sheet`) and caps text at 32,767 characters (`text-box-too-long`).
`object.setAnchorMode {kind: "chart"|"image"|"shape", id, mode: "moveAndSize"|"moveOnly"|"absolute"}`
controls whether an object moves and/or resizes with cells. Selection-pane entries include `anchorMode`.
`chart.switchRowColumn {chart?}` switches series orientation, clipping whole-row/column source ranges
to used cells and returning `chart-range-too-large` if the remaining range exceeds the chart limit.

For a headless equivalent (no window), use `gridcraft-cli mcp` or `gridcraft-cli run`;
see [`mcp.md`](mcp.md) and [`cli.md`](cli.md).
