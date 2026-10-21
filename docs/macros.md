# Macros

> **Last updated:** 2026-10-21 · **Change:** minor (the macro interface: Automate › Macros, the Macros and Record Macro dialogs, the step editor, shortcuts)

GridCraft macros are typed, declarative lists of commands, not VBA. Every step runs a GridCraft
command (the same ids the ribbon, CLI and MCP use) or one of three bounded control steps. A macro
declares the inputs it works on, runs as one undo step, rolls all workbook changes back on an
error (a file it already wrote stays), checks where its results will land before it writes them,
and does not touch files without permission.
Agents run macros with `macro.run` like any other command.

This page describes the engine, its commands and the macro interface in the window.

## A macro

```json
{
  "name": "Totals",
  "description": "Adds a total next to the amounts",
  "inputs": [{"name": "Input", "header": true, "cols": [2, 2], "colTypes": ["text", "number"]}],
  "output": {"anchor": "Input.right(2).cell(1,2)", "onConflict": "ask"},
  "steps": [
    {"type": "command", "id": "cell.set", "params": {"cell": "Output", "input": "Total"}},
    {"type": "command", "id": "cell.set", "params": {"cell": "Output.offset(1,0)", "input": "=SUM({{Input.data.col(2)}})"}},
    {"type": "repeat", "over": "Input.data", "of": "rows", "max": 1000, "body": [
      {"type": "if", "cell": "Row.cell(1,2)", "op": "gt", "value": 100,
       "then": [{"type": "command", "id": "home.bold", "params": {"range": "Row"}}]}
    ]},
    {"type": "stop", "message": "Done."}
  ],
  "permissions": []
}
```

- **Steps**: `command` (`id`, `params`), `repeat` (each row or column of `over`, at most `max`
  times), `if` (`cell` compared with `value` by `eq`, `ne`, `gt`, `ge`, `lt`, `le`, `empty`,
  `notEmpty` or `contains`; `then` and `else` lists) and `stop` (`message`; `rollback: true`
  undoes the run). Every step can carry `enabled`, `breakpoint` and `note`. The short form
  `{"command": "cell.set", "params": {…}}` is accepted for a command step.
- **Inputs**: `kind` (`cell` or `range`), `rows` and `cols` as `[min, max]` (0 for no maximum;
  rows exclude the header), `header`, `colTypes` (`number`, `text`, `date`, `bool`, `any`),
  `nonEmpty` and `defaultRef`. A run binds each input to the range it is given, its default, or
  (for the first input) the selection, and refuses with messages that name the cells:
  "Input column 2 (Amount) must hold numbers; C4 holds text."
- **Limits**: 1,000 macros a workbook, 10,000 steps a macro, Repeat and If nested 4 deep,
  100,000 steps executed per run, and 60 seconds a run, footprint checks included (`timeLimit`
  to change; the web build has no clock, so there only the step limit applies).

## Tokens

References in step parameters can be tokens, worked out when the step runs:

| Token | Means |
|---|---|
| `Input`, or another input's name | the range the input is bound to |
| `Output` | the output rule's anchor |
| `Row`, `Column` | the current row or column of the Repeat around the step |
| `Name:Total` | a defined name, or a table's data |

Methods narrow or move a range and chain: `col(2)` or `col("Amount")`, `row(1)`, `cols(2..3)`,
`rows(2..)`, `cell(r,c)` (relative to the top-left cell; `cell(0,1)` is the cell above),
`offset(r,c)`, `resize(h,w)`, `data` (without the header row), `below(n)` and `right(n)`.
`col("Amount")` looks the name up in an input's header row, directly or after `data`; on a range
without a header row it is refused. A defined name can hold periods: `Name:Sales.Q1` is the name
`Sales.Q1`, and `Name:Sales.Q1.cell(1,1)` its first cell, because the longest name or table that
exists is used and methods follow it.

Write a token as the whole value of a reference parameter (`cell`, `range`, `target`, keys ending
in `Cell` or `Range`…), as `{"$ref": "Input.col(2)"}` anywhere, or as `{{Input.col(2)}}` inside
any string, such as a formula. Plain A1 references stay absolute; a `Name:` anchor follows
inserted rows. A reference with a sheet name is never a token, so `Output!A1` is a cell on a sheet
called Output, even when a token or input has that name.

## Running

`macro.run {name, inputs?, onConflict?, overwrite?, approve?, timeLimit?}`:

1. **Policy.** File commands other than Properties, exports and printing (such as Open, Save
   and Close), app, window (including New Window, Hide and Unhide), automate and macro commands,
   and Undo and Redo, never run in a macro. File › Properties runs, and so do exports and
   printing (`file.exportPdf`, `exportCsv`, `exportHtml`, `print`, `printPreview`, `saveBytes`)
   that only produce bytes. Reading or writing a file (`data.getData`, `data.fromTextCsv`,
   `insert.picture` or `pageLayout.background` with a `path`, or an export or print with a
   `path`) needs the command in the macro's `permissions`. Any `path` other than `null` counts,
   including a blank one and a token, and the policy is checked again on the parameters as they
   are when the step runs. A macro that touches files, or that came with a workbook opened from
   a file, runs once the person approves it: the call fails with `approve: …` and runs with
   `approve: true`. Approval lasts for the session, until the macro changes.
2. **Footprint.** The macro first runs in dry-run mode on the real workbook, and everything is put
   back. Cells it would write outside its inputs that hold data, and sheets and objects it would
   delete, are conflicts, handled by `onConflict` (or the macro's `output.onConflict`): `ask` (the
   call fails with `confirm: …`; `overwrite: true` answers it), `fail`, `overwrite`, `shiftDown` or
   `shiftRight` (Insert Cells makes room, then the footprint is checked again). Steps that touch
   files, and Solver, end the dry run early; the footprint then says it is partial, and unless
   the rule is `overwrite`, the run checks every step as it goes. A step that writes over cells,
   sheets or objects that held data when the run started stops the run and rolls the workbook
   back (a file already written stays): it asks (`confirm: …`) under `ask` and fails otherwise,
   since shifting can't make room that late.
3. **Run.** One undo step, "Run Macro: Totals". Any error rolls the whole run back and names the
   step: `Step 3.1 (cell.set): …` (paths are 1-based; `2.1` is inside a Repeat, `4.then.1` inside
   an If).

`macro.check {name, inputs?, timeLimit?}` reports the bound inputs, the output anchor, any
approval needed and the footprint, without changing anything.

## Debugging

`macro.debug {action, …}` with `start` (`name`, `inputs`, `breakpoints`), `step`, `continue`,
`runTo` (`step`: a path), `stop` (`keep: true` keeps the changes) and `state`. `breakpoints` can
be given with any action; `breakpointsOnly: true` makes the list replace the steps' own flags. The
run stays one undo step; while it is open, edits of the workbooks it touched (its own, and any
other its steps changed) are refused with "Finish or stop the macro first", also when another
command makes them (stopping Record Actions saves a script), and other workbooks stay
editable. Each answer gives the next step with its parameters as they would run, the parameters
of the step that just ran, and the cells changed so far.

## Recording

`macro.record` toggles recording (`relative: false` for absolute references). When it starts,
`name`, `shortcut` and `description` give the macro its name, Ctrl+Shift shortcut letter
(`"Ctrl+Shift+K"` or `"K"`) and description; they are checked at once, so a name that is taken,
too long or holds line breaks, or a letter that GridCraft or another macro uses, is refused
before anything is recorded. Without a name the macro is called `Macro1`, `Macro2` and so on. A
`name` or `description` given when stopping wins over the one given when starting; a name that
another macro has by then gets a suffix (`Tidy (2)`), so stopping never replaces a macro. The macro is saved in the workbook where recording started, even if
another one is active; if that workbook was closed, stopping ends the recording without saving it.
When saving the recording fails (a name the check refuses, a workbook at its macro limit, a
macro being debugged in that workbook), recording goes on, so stopping again keeps what was
recorded. A recording keeps up to 100,000 commands and a macro up to 10,000 steps; steps past
either limit are left out and counted in `dropped`. With relative references the selection when
recording starts is the example input: references in and near it on its sheet become tokens (on
another sheet `C4` stays that sheet's `C4`), and the input's shape, header and column types are
inferred. Toggles such as Bold are recorded with the state they set (`"on": true`), so playing
them back sets them rather than flipping them. Commands a macro may not run are left out and
listed in the result, also from the steps of a macro or script run while recording; a macro run
while recording is replaced by its steps when it has no inputs, no output and only plain commands
(otherwise it is left out and listed), and an Automate script run by the script's steps.

GridCraft keeps the Ctrl+Shift letters it uses itself on some platform: F, G, H, J, K, L, R, S, T
and Z (`macros::RESERVED_SHORTCUTS`). A shortcut a file brings with it on one of these letters is
kept but does not run the macro.

## Other commands and storage

`macro.list`, `macro.get`, `macro.save {macro, replace?}`, `macro.delete` and `macro.rename
{name, to}`. Saving, renaming and deleting are undoable.

Macros are saved in the workbook: as a field in GridCraft's JSON format, and in xlsx as hidden
defined names (`_gc_macro_<n>_<k>`, the macro's JSON split into string literals within Excel's
limits), which other spreadsheet applications keep but do not run.

Each macro carries its format version, `v` (1 in this version; a missing `v` reads as 1). When a
file is opened, a macro that is damaged, over the limits or in a newer format (a `v` above 1) is
left out with one warning, and the rest of the workbook and its other macros open. A newer
macro is never read in part, so a save can't write it back with its newer fields missing; a
macro that was left out is not in the saved file.

## In the window

- **Automate › Macros group.** Macros opens the Macros dialog. Record Macro opens the Record
  Macro dialog (name, Ctrl+Shift shortcut letter, description, Use Relative References); OK
  starts recording and stays off while the name or the letter cannot be used, with the reason
  under the fields. While recording, the button reads Stop Recording, the status bar shows a red
  dot, "Recording · N steps" and Stop, and Use Relative References is locked. Run Macro runs the
  macro run last (or the only one); its menu lists the recent macros, then every macro. New
  Macro opens the step editor on a new macro.
- **Macros dialog** (Automate › Macros, View › Macros, Alt+F8). It lists the workbook's macros
  (with their shortcuts), then its Automate scripts, and says when the file carries a VBA project.
  Run runs the selected macro on the selection or the selected script; Step Into, Edit and Create
  open the step editor; Delete asks first; Options sets a macro's shortcut letter and description. The dialog keeps the result of `view.macros`,
  as before macros existed.
- **Step editor.** A resizable window that works on a draft and saves it with `macro.save`. The
  step list shows each step's number, a breakpoint dot in the gutter (click to set or clear), a
  category icon, a summary, an error marker and an enabled checkbox; Repeat bodies and If branches
  are indented under their step, and rows drag to reorder within their list. Add Step opens a
  command picker (control steps first; commands a macro may never run are left out); Duplicate and
  Delete act on the selected step. The inspector's Details tab edits the selected step, with
  reference boxes that take a typed reference or a selection on the grid and switch between a
  token relative to the example input (Rel) and an absolute reference (Abs); the Inputs tab sets
  each input's shape, header and column types, and Use current selection as example infers them;
  the Output tab sets the anchor and what to do when results would land on data.
- **Debug bar.** Check dry-runs the macro and colours the grid: inputs blue, the cells it writes
  green, the cells among them that hold data red. Run runs it as one undo step. Step, Continue and
  Run to Here start the debugger when needed (`macro.debug` with the editor's breakpoints and
  `breakpointsOnly: true`, so a dot cleared while paused takes effect at once); the current step is
  selected, its parameters are shown as they will run, and its target is outlined on the grid.
  Stop asks whether to keep the changes so far or undo them. While a macro is debugged the status
  bar says "Debugging" with the macro's name, and Show reopens the editor.
- **Questions.** A macro that needs approval offers Run; a macro whose output would land on data
  offers Overwrite, Shift Down and Shift Right. Each button runs the command again with
  `approve: true` or the chosen `onConflict`.
- **Shortcuts.** Ctrl+Shift+letter (⌘⇧letter on a Mac) on the grid runs the macro with that
  letter. The letters GridCraft uses itself keep their own meaning.

Commands with empty parameters never open these dialogs: `macro.record {}` starts recording
under the next free name, and the dialogs are opened only from the ribbon, the menus and keys.

## Automate scripts and VBA

Automate scripts (`automate.recordActions`, `newScript`, `allScripts`, `runScript`,
`deleteScript`) are an older, separate feature. They keep their commands, parameters, storage
(`_sc_scripts`) and results, and View › Macros (`view.macros`) still lists them. Macros neither
read nor change them; a workbook can hold a script and a macro with the same name.

A VBA project in an `.xlsm` or `.xltm` file is preserved on save and never run. GridCraft macros
are stored apart from it.
