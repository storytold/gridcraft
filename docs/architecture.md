# Architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-11-05 · **Change:** minor (a save waits for Python cells: `engine::python::wait`, `ui_egui::save_wait`; earlier: dates in Python cells: `engine::python::dates`, `protocol::Stamp`, `PyCode::dates`; earlier: the indicators: `python_card::tag_state`, `python_ui::checks`, `engine::python::packages`; earlier: the app turns Python on: `ui_egui::python_ui`; earlier: the engine runs Python cells: `engine::python`; earlier: `python.clear`, Convert to Normal Cell; earlier: Python cells in the interface; earlier: Python mode in the formula bar; earlier: Python cells as data; earlier: equations: the `math` crate and the layout cache; earlier: recently used functions kept in the preferences; Function Library module; accent icons; earlier: the hidden names that hold macros) · **Target:** Microsoft Excel (Microsoft 365)

How GridCraft is built today. Rules for contributors are in [`AGENTS.md`](../AGENTS.md).

## Workspace and layering

An engine-first Cargo workspace. Layering is enforced by `cargo xtask layers`; nothing below L6
depends on egui, eframe, winit or rfd, so the UI crate is swappable. Line counts are Rust lines
including tests, 2026-10-10.

| Layer | Crate | Lines | Job |
|---|---|---|---|
| L0 | `core` | ~1.3k | Cell and range addresses (A1, R1C1, sheet-qualified; 1,048,576 × 16,384), `Value`, `CellError`, Excel coercion, 1900/1904 date serials, typed-input parsing |
| L0 | `numfmt` | ~2.1k | Excel number-format codes (sections, conditions, colours, dates, elapsed time, fractions, scientific, fill), General, `TEXT()`; en-US only |
| L1 | `formula` | ~1.7k | Tokenizer, parser, AST, printer (A1 and R1C1), reference adjustment for copy, fill and structural edits |
| L1 | `functions` | ~13.4k | The function library: `FnSpec` registry (name, arity, category, scalar args, signature, description) and pure implementations; automatic array lifting |
| L1 | `math` | ~1.1k | Equations: LaTeX laid out by RaTeX (MIT, pinned) into plain glyph, rule and path data, and LaTeX to Office Math; every entry point capped and panic-guarded |
| L2 | `model` | ~2.2k | Workbook, sheets, copy-on-write cell store (64-row bands behind `Arc`), interned styles, names, tables, CF, validation, comments, charts, shapes, images, sparklines, print settings, protection |
| L2 | `calc` | ~2.9k | Evaluator (references, names, tables, LET/LAMBDA, special forms like OFFSET/INDIRECT/SUBTOTAL) and dependency-graph recalculation with dynamic arrays, spills and cycle detection; single-threaded |
| L2 | `xlsx` | ~10.7k | XLSX read/write (transitional and strict read), CSV/TSV, XLSB and ODS data import, PivotTable caches, DrawingML charts and drawings; byte slices only (WASM-safe); lenient reader that reports warnings |
| L0 | `sandbox` | ~0.6k | Confined launcher for helper processes: Landlock and seccomp on Linux, `sandbox-exec` on macOS, refused elsewhere; checks the confinement before the program runs ([sandbox.md](sandbox.md)); used by the `python` crate and the apps |
| L2 | `python` | ~3.1k | Python in cells, off wasm (the web build has a stand-in that reports Python as unavailable): interpreter detection, the worker shim (`shim.py`, the one non-Rust source file, run by the user's Python), the JSON-lines protocol with its caps, the client with deadlines and kills, the process launch and a fake worker for tests; depends on `core` and, off wasm, `sandbox` |
| L2 | `pdf` | ~1k | Small PDF 1.7 writer (14 standard fonts, vectors, images) for print and export |
| L3 | `chart` | ~2.9k | Toolkit-free chart layout to drawing primitives, a rasterizer, sparklines |
| L5 | `engine` | ~17.7k | `Session`: open workbooks, selection, history (COW snapshots), clipboard, fill, sort, filter, PivotTables, what-if, print, file I/O (`io.rs`), and the command registry (`cmd/*`, 313 commands) with the Excel ribbon catalog (`catalog.rs`) |
| L6 | `ui-egui` | ~11.6k | Excel-style UI (title bar, QAT, ribbon, formula bar, virtualised grid, editor, tabs, status bar, dialogs, task panes, chart view) and the control channel |
| L6 | `mcp` | ~1.4k | MCP server over stdio (hand-written JSON-RPC), headless or bridged to a running app |
| L7 | `apps/gridcraft`, `apps/gridcraft-cli`, `apps/gridcraft-web` | ~2.2k | Desktop (eframe/wgpu), CLI (`info`, `convert`, `eval`, `cat`, `run`, `commands`, `functions`, `mcp`, `send`), web (WASM via trunk) |
| — | `xtask` | — | `ci`, `parity`, `assets`, `layers`, `wasm`, `version`, `stats` |

## Data flow

1. **Every action is a command.** The UI, CLI, control channel and MCP call
   `Session::execute(id, params)`. Commands are `CommandSpec { id, label, menu, shortcut, params,
   enabled, run, journal, undoable }`; ids follow Excel's ribbon (`home.bold`, `insert.chart`,
   `data.sortAscending`) plus primitives (`cell.set`, `selection.set`, `range.setValues`).
   `Session::execute` catches escaped panics and restores the workbook, its selection and its
   undo and redo history as they were before the command (never-crash guard); a group that was
   open before the command and that the command ended or rolled back stays ended and keeps a
   step from its start to the restored workbook, so Undo still returns to the state before it.
2. **Edits** change the model through copy-on-write bands; undo keeps the previous snapshot.
   Undo retains at most 100 steps and trims older steps when its estimated retained cell storage
   exceeds 128 MiB. The newest step remains undoable even if that step alone exceeds the budget.
   The estimate is a history policy, not a measure or hard cap of process memory.
3. **Recalc** marks dependents of changed cells dirty through the dependency graph (with range
   nodes for range dependents), evaluates in topological order, spills dynamic arrays and reports
   `#SPILL!`/cycles. Volatile functions are always dirty.
4. **Rendering**: the egui grid reads display values (number formats applied in `engine/display.rs`)
   for the visible window only; charts render through `crates/chart` primitives.
5. **Files**: `engine/io.rs` sniffs content (XLSX, XLSB, ODS) before trusting the extension and
   dispatches to `crates/xlsx`; imports that lose data return warnings shown to the user.

## UI building blocks

The panels in `crates/ui-egui` share a few pieces, so a new feature reuses them instead of
drawing its own:

- **Colours**: `theme::Tokens`, each with a light and a dark value. The sheet stays white in both
  themes, so colours drawn on it (`sheet_ref_colors`, `sheet_control_*`, icons through
  `icons::paint_on_sheet`) keep their light values. `theme::contrast_tests` holds every
  foreground/background pair to WCAG 2 (4.5:1 for text, 3:1 for glyphs) in both themes; a new
  token pair is added there. Icons (`icons.rs`, drawn in code) take their paper and category
  colours (`theme::Cat`) from the tokens; `icons::paint_accent` gives an icon an accent colour,
  drawn on its coloured part when it has one (the spine and glyph of `Icon::FnBook`).
- **Ribbon widgets**: `widgets.rs` has the buttons every tab is built from (`big_button`,
  `small_button`, `small_button_accent`, `split_button`, `big_split_button`, `toggle_button`),
  translated through `i18n` in one place. Split buttons return the main and arrow responses
  separately, with accessible names (`AutoSum`, `AutoSum options`); big and small buttons
  report their label (an icon-only one its tooltip) as their accessible name. The ribbon card
  scrolls a tab wider than the window, with arrows at its edges; `ribbon::RibbonOverflow` records the groups' size each frame,
  and a test keeps every tab within the card's three rows.
- **Function Library**: `function_library.rs` holds the function categories (engine id and
  label, in the Insert Function dialog's order), the function list built once from
  `cmd::formulas::function_list`, and the menus of Formulas › Function Library, including the
  AutoSum menu that Home › Editing shares. Recently Used and the Insert Function dialog's Most
  Recently Used category list what `formulas.recentlyUsed` returns.
- **Code editor**: `widgets::code_editor(ui, id, &mut text, Lang, max_lines)` (`code_editor.rs`)
  edits code in the monospace font with `Tokens::syntax` colours: Python (keywords, strings
  including triple-quoted ones, numbers, comments, builtins) and LaTeX (commands, scripts,
  comments). Tab indents (every selected line, Shift+Tab dedents), Enter keeps the line's
  indentation (one level more after a Python colon), both applied in input order with the typing
  around them: a pass can carry several events, and the editor's keys that follow an event the
  text field acts on (select all, an arrow) wait for the editor's next pass, which puts them
  ahead of its input, so the field never gets a Tab or an Enter of a focused editor; and it
  scrolls past `max_lines` with the caret in view. The tokenizer and the key edits are pure functions with tests, including
  hostile input.
- **Code editor marks**: `code_editor_marked` underlines a byte range in the error colour (the
  equation editor marks the position of a LaTeX error with it).
- **Formula bar**: grows with its content from one line to `formula_bar::MAX_LINES` (five), then
  scrolls; one line is Excel's 30 px unless the font's rows need more (`formula_bar::one_line`;
  the Windows UI font's row is 17.28 px), so every counted line fits whatever the font;
  `formula_bar::BarLayout` records where its parts went each frame, for tests.


## Engine building blocks for multi-command actions

Actions made of many commands (macros, a debugger that runs one step per call) use these parts of
`Session` and the engine:

- **Undo groups.** `Session::grouped(label, f)` runs `f` as one undo step and rolls every change
  back if `f` fails; `group_begin`, `group_end` and `group_rollback` do the same across calls.
  While a group is open on a document, its commands add no undo steps of their own and Undo and
  Redo are disabled there. Groups nest; the outermost one makes the step. A command that opens and
  closes its own groups still makes one undo step for itself, covering its edits inside and outside
  the groups. A command that ends a group open before it leaves the group its step and makes one
  of its own for what it changes after the end; a group a command opens and leaves open starts
  where the command did, but only when the command succeeds: a command that fails rolls back
  every group it opened, as `group_rollback` does. `Session::abort_groups(doc)`, and the command
  `automate.stopAction` on the active workbook, roll back the groups left open on a document (a
  debugger the person abandons, a caller that dropped its token), so Undo works there again. The
  command runs on its own: a step of a script or macro may not stop the action it is part of, and
  a recording leaves it out, so playing the recording back never stops the run. The
  history limit is applied when the outermost command finishes; until then the steps made inside
  it are bounded on their own.
- **Recently used functions** (`recent.rs`). `Prefs::recent_functions` holds the ten functions
  most recently entered in a formula, newest first. `Session::execute` updates it for every
  `cell.*` and `range.*` command it journals, from any source (interface, control channel, MCP,
  script). It reads at most 2,000 of the formulas a command enters, starting from the last one
  (the most recent), so a large paste costs no more than that; `formulas.recentlyUsed` returns
  it. It is kept with the other preferences (`prefs.json`, the browser's storage on the
  web) and read leniently, so a malformed value loses only this list. It does not read the
  journal back, so the journal's trimming does not affect it.
- **Recording buffer** (`record.rs`). `Session::start_recording` collects each successful
  top-level command with the document, sheet and selection it ran in, without secret parameters.
  Commands run by other commands are not recorded, nor is a menu-style call that only opened a
  dialog (the dialog's confirming call is the step; a run that changed the workbook before a step
  of it opened a dialog is recorded), and the buffer is independent of the journal
  (Automate › Record Actions still records through the journal).
- **Document trust.** `DocState::trust` is `Local` for workbooks made in the session and
  `FromFile` for workbooks opened from a file. `Trust` has no default: `DocState::new` takes it,
  so every way of making a document says where its content came from. It is also `FromFile` for
  workbooks that received a paste from one that writes formulas or validation: Paste, Paste
  Special › All, All Except Borders, Formulas, Formulas and Number Formats or Validation, and any
  linked paste other than column widths or notes. Values, formats, column widths and notes carry
  nothing active. Active content in a `FromFile` workbook asks before it runs. This is provenance,
  which no decision changes; `DocState::scripts_trusted` is the separate per-session consent for
  stored `automate.*` scripts (`automate.trustScripts`).
- **Workbook data in hidden names** (`cmd/stash.rs`). An item is split over hidden
  workbook-scope names `_gc_<key>_0`, `_gc_<key>_1`, … of string literals within Excel's 255- and
  8,192-character limits, so it survives an xlsx round trip. Characters XML can't carry (controls
  other than tab, line feed and carriage return, U+FFFE, U+FFFF) are stored as the noncharacter
  U+FDD0 and four hex digits, as is U+FDD0 itself, and read back as they were. `stash::items`
  and `stash::keys` list the items under a key prefix in one pass over the names, at most
  10,000 of them (`stash::MAX_ITEMS`) with a count of the rest, so a caller can say what it did
  not list. The older single-name `_sc_<key>` items are read as well.
  `formulas.defineName` and `formulas.deleteName` refuse a name the workbook holds such an item
  under (a hidden workbook-scope name of either shape; a visible one is the person's own), and
  `formulas.createFromSelection` skips such labels, unless `force: true` is passed.

The macro engine (`engine/src/macros/`, commands in `cmd/macros.rs`, described in
[`macros.md`](macros.md)) is built on these: a run is an undo group, the recorder reads the
recording buffer, the policy asks before a macro in a `FromFile` workbook runs, and xlsx keeps
macros as chunked hidden names (`_gc_macro_<n>_<k>`: macro `n`, chunk `k`).

## Python cells

A Python cell is a `Cell` whose `py` holds a `model::PyCode`: the code, the return type (an Excel
value or a Python object, the `0` or `1` of Excel's `PY(code, return_type)`), the last result and a
stale flag. Its `value` is the cached result, so formulas, display and saving read it like any
other cell. `model::python` has the text forms: `PY("…"&"…",n)` with literals of at most 255
characters (the function's syntax, which some files hold), the `_xlfn._xlws.` prefix, the
8,192-character limit measured on that formula (`PyCode::fits`), and the `xl("…")` literals in
code, which structural edits rewrite like formula references.

Only those edits rewrite them: inserting or deleting cells, rows and columns, a cut and paste, and
renaming or deleting a sheet. An edit that would make a cell's code longer than the limit (a
longer sheet name, `Z1` becoming `AA1`, a cut to another sheet that qualifies the references) is
refused and changes nothing, since code over the limit would be dropped to its value on the next
open. Copying, filling and sorting carry the code unchanged, so an `xl()`
reference acts as an absolute reference. A sort that moves a Python cell to another row leaves its
code reading the cells it named before (a formula moved by the same sort has its relative references
shifted), and references to the sorted cells from other cells, formulas and Python code alike, keep
naming the same positions.

Calc treats a Python cell as a graph node that reads nothing: a pass takes its kept result instead
of evaluating it, so an array result spills, is blocked and unblocked, and reaches its dependents
through the same code as a formula's. The xlsx reader and writer map Python cells to and from
`_xlfn._xlws.PY(index,n)` formulas with cached values and the workbook's Python part, which holds
the code and the environment (`xlsx::python_part`, [MS-XLSX] 2.3.10); `io::adopt_python` and `io::enforce_python_limits`
bound what a file can make the app hold (result size, spill ranges, total rebuilt cells, code
size). `python.set` stores code; where Python doesn't run (see Running Python cells below) new or
changed code shows `#BLOCKED!`. `python.output` switches a cell's return type; as with a new return type through `python.set`,
a cached result of the old type goes, so the cell has no result until it runs, unless the type asked for
is already in effect or the cell holds a picture with no choice. `python.clear`
(Convert to Normal Cell) removes the code and keeps the cell's value; the values its result spilled
become plain values (`Sheet::settle_spill`). A Python-only error (`#BLOCKED!`, `#BUSY!`,
`#PYTHON!`, `#TIMEOUT!`) is no result and no ordinary cell's value, so a cell that shows one is left
empty with its format. The cells that changed are touched so their readers recalculate. `cmd::python::clear_refusal` gives the reason the command would refuse (no Python
code, a table header, a locked cell or a spill over locked cells on a protected sheet), so the card
and the cell menu turn the action off where it would fail.

In ui-egui, Python mode is a state of the shared cell editor (`EditState::python`) that edits in
the formula bar only, through `code_editor::code_field`: the code editor's in-order key handling
(`apply_in_order_until`) with stop keys for commit, cancel and leaving the mode.
Point mode works there too: `EditState::can_point` accepts the caret where a value can start,
outside strings and comments: after `(`, `,`, `=` or an operator, after a keyword that takes a value
(`return`, `in`, `else`), or at the start of a line outside brackets, never after a completed
operand followed by a space, such as `value` or `f(a)`; `insert_ref` wraps the reference
the grid builds in an `xl("…")` call (`editor::python_xl`, which escapes it as a Python string), and
`pointed_text`/`pointed_range` read it back, so the grid's click, drag, header and arrow handling is
shared with formulas. A locked cell on a protected sheet is refused before a draft exists.
Committing a Python edit
(`commit_python_edit`) runs no command when the code is unchanged (no undo step), and none when
sheet protection locked the cell while the draft was open (the draft is dropped with the warning).
With all code deleted it runs `cell.set` with empty input on a Python cell, which clears it, and
nothing on any other cell. Any other commit runs `python.set`; when that refuses, the message
shows and the edit stays open. Drafts, Python or not, follow their sheet by its runtime identity
(`Sheet::uid`, `model::SheetUid`, found again by `EditState::sheet_in`), so a sheet inserted,
deleted or moved while one is open never takes it. After every command the open draft is pointed
at its sheet again, so it is drawn, pointed from and committed there, and its point-mode anchor
(held by sheet index) is let go when the sheet order changed. A draft is dropped then, with a
message, when its sheet is gone, or when it holds Python code (in Python mode, or begun on a Python
cell) that sheet protection now hides, as the Python editor window closes then.
`python_card.rs` holds the PY tag (painted in the grid's
logical coordinates, so the grid's mirroring places it on right-to-left sheets), the card
(`ui.pythonCard`) and the cell menu's return-type items. `python_card/table.rs` is the card's grid over
a `PyTable`: its column widths are worked out once per table, and only the cells in the scroll area's
viewport are laid out. `python_window.rs` is the Python editor
window: its own copy of one cell's code (by workbook uid, sheet and address) in `code_field`. It
follows its sheet by `Sheet::uid` (`model::SheetUid`), an identity that lives only while the app
runs: a new, inserted, copied or opened sheet gets a fresh one, and a rename, a move and the
copies undo keeps don't change it, so a sheet deleted and another of its name put in its place is
never taken for it. The code field has the line-number gutter (painted for the rows in view)
and an `accept` check that refuses an edit past `PyCode::fits` as it is typed or pasted. Apply
runs `python.set` in the window's workbook. The questions about unsaved code are a state of the window (`Pending`), so a
programmatic `ui.pythonEditor` never opens one; it refuses, or discards with `force`. With native
windows, where each window draws the app with its own workbook active, the editor is drawn only in
its workbook's window (`PyWindow::window`), and a question or an opening brings that window
forward.

## Running Python cells

`engine::python` runs Python cells in a session that has a `PythonCapability` from
`Session::set_python_capability`, a setter for the app. The app gives one; the CLI and MCP don't, so
there cells keep their saved values or show `#BLOCKED!` and the commands that run cells say why not.
No command, control method or file content can give a capability or choose the interpreter
(`no_command_turns_python_on_or_chooses_an_interpreter` walks the command catalog). A workbook made
in the app is on when the capability is; one opened from a file is off until
`Session::set_python_workbook_enabled` turns it on, and Python code pasted from a file turns a
workbook off again, even one the person had turned on, as other content from a file only changes its
`Trust`.

**In the app** (`ui_egui::python_ui`). The person's choices are a preference, `UiState::python`, off
by default; anything malformed in the saved file reads as off. `python_ui::tick`, from
`SheetApp::logic`, compares them with what the session was last given and hands over a new capability
when they changed, calls `python_poll(Duration::ZERO)`, and while `python_busy()` asks for a repaint
every 100 ms, so results appear without input and an idle app stays idle. The switches are widget
code only: `write_settings` and `set_workbook` refuse, and the widgets draw disabled, while synthetic
input from the control channel is queued or was just applied (`python_ui::locked`). So `ui.click`,
`ui.drag`, `ui.key` and `ui.text` can't reach them, and they have no key tips. That doesn't cover
someone who can edit the preferences file, or who operates the real mouse and keyboard or the
accessibility interface. With Python off nothing is started: the search for Pythons
(`engine::search_pythons`, on a thread of its own) lists paths without asking them anything.

The Python tab is a contextual tab like Table Design (`ribbon::contextual_tabs`, last, so rightmost):
`python_ui::tab_shows` is true on a Python cell, or while a draft or the editor window edits code. Its
key tip, `Y`, is in the static table, so `SheetApp::ribbon_keys` ignores a tab that isn't up.

The PY tag (`python_card::tag_state`, `paint_tag`) reads the cell's `busy` and `stale` flags and
`python_diagnostic`, a few lookups per cell in view. Both editors keep a `python_ui::checks::CodeChecks`:
`lint` runs once per text and package list, and its findings become `code_editor::Mark`s in chars, so a
mark from before this frame's edit can't split a character; a run's line is used only while the text is
the code that ran. `engine::python::packages` works out `packages.missing` from each piece of code
once per package list, keyed by its hash, so an edit reads the cell that changed, and an empty list reports
nothing.

The panes (`python_ui/panes.rs`) read `python_report`, `python_diagnostics` and `python_run_log`, and
each opens by a UI-only command (`ui.pythonEnvironment` and the like). Their lists draw only the rows
in view. What the interface asks every frame is cheap: `python_state` copies nothing, `python_running`
reads the request in flight, and `python_has_cells` is kept from what each change touched, so a
workbook without Python cells is scanned once.

A Python object's size and table come from the worker only with the Python Object return type, each
inside the protocol's caps (`MAX_SIZE_DIMS`, `MAX_TABLE_ROWS`, `MAX_TABLE_COLS`, `MAX_TABLE_TEXT_BYTES`,
`MAX_TABLE_BYTES`), and `Reply::parse` refuses a line that breaks one or whose shape doesn't match its
data. The worker walks at most what it sends: `iloc` for pandas, a slice for numpy and lists, stopping
at the budget in bytes, so a table of a million rows costs what one of 200 does. `runner::kept` writes the
cell's value from the type's name and the size (`object_label`), and keeps the table as
`PyOutput::table`, an `Arc<PyTable>` so that the undo history shares it, and `#[serde(skip)]` with
`from_run`, which says whether a result with no table never had one or a file lost it. `PyTable` also
holds the CSV and Markdown forms; `python.table` and `python.saveCsv` go no further than it, since a
whole object has no bound, and they report `cut` so that the interface can say so. `macros::policy::EXPORTS` lists
`python.saveCsv`, so a macro step with a `path` needs the permission, and it writes through
`io::write_file`, which refuses inside a script.

`PyCode::spill_lines` is the one Python setting of a cell that isn't its code or return type.
`PyCode::result_value` splits the kept text when it is on, so no run is needed and the text is there
to go back to; calculation lays out the result as any array. It is serialized in GridCraft JSON only. The
Python part of an xlsx file has no place for it, and a hidden name was rejected, since it would make
this workbook's Python data depend on a defined name another application shows and may drop.

Nothing blocks the caller, but for `file.save` with `wait` (below), which the interface never lets
through. A command only marks what is due, at the end of the outermost `execute`;
`Session::python_poll`, called each frame, takes at most one finished result, writes it through the
normal commit path with no undo step, recalculates and submits the next cell. Each open workbook has
one `Client`, made when first needed and dropped with the workbook, which stops its worker.

- **Order.** `python_order`: sheets in tab order, each row by row, as in Excel.
- **What is due** (`python/plan.rs`). The run state is a list of cells in run order. After a change
  it is updated from `CellStore::diff` and the spill maps, so a workbook without Python cells pays
  for comparing the bands the edit touched. Each cell has a fingerprint of its code, return type
  and inputs, and the one it last ran with; the first cell where they differ, or whose place in the
  order is new, is due, with every cell after it, since cells share variables. The hash leaves out
  what a reference spells and where the values are, so moving cells around runs nothing. Only a
  reference the change reaches is read again, and an area several cells read is read once.
- **Busy and stale.** `PyCode::busy` (never saved) makes a due cell show `#BUSY!` through any
  recalculation while its kept result, and what that result spilled, stay; a save writes the kept
  result. `PyCode::failed` (never saved) keeps the error of a run that left no result, for the same
  wait. Under manual calculation (`python.calculation`, or the workbook's own manual mode) due
  cells are marked stale instead, and keep their value until `python.run` or `python.runAll`.
- **Inputs** (`python/inputs.rs`). Each literal `xl("…")` is resolved by the parent as a formula's
  reference would be (`calc::evaluate_reference`): cells, ranges, a spill, other sheets, names and
  tables; a range running to the sheet's edge stops at the used range. Over
  `protocol::MAX_ARRAY_CELLS` cells in all, or with a reference that names nothing, the cell doesn't
  run (`#PYTHON!` with `xl.tooLarge`, `#REF!` with `xl.badReference`).
- **Dates** (`python/dates.rs`, `gridcraft_python::protocol::Stamp`). The worker never sees a serial
  number, since only the parent knows the workbook's date system: dates cross the protocol as capped
  text, `{"date":"2024-01-15"}`, and durations as days, and `Stamp::from_serial` and `Stamp::serial`
  convert through `core::date`. Text that names no date reads as `#VALUE!`; a date no serial holds is
  `#NUM!`. `inputs` asks `NumberFormat::date_shape` what each number's format shows, once per style,
  and the fingerprint counts the answer, so a new format runs what reads the cell; a row or column
  format under a spill counts too (`SheetChange::lines`), as a spilled value has no cell to change.
  A result's stamps become serials, and `PyCode::dates` holds what each place asks for and what runs
  wrote (never saved, and a shared slice, as a cell is copied whenever a flag on it changes).
  `dates::sync_formats` compares those with the cells' formats after each result, and `plan` calls it
  when a spill comes or goes, in the same way as a result: no undo step, and before the look at what
  changed, as what reads those cells goes by the formats. General can't say "no format", so `written`
  is what tells a format a run wrote, which a later result takes back, from one the person chose or
  took off.
- **Workers** (`python/runner.rs`). A cell only runs in a worker that has run every cell before it, so
  a fresh worker runs those first. A cell that stopped its worker (a limit, a crash, `python.stop`)
  doesn't run again until its code or inputs change or someone asks. After more than three lost
  workers in one go the cells still waiting are left stale. A result that changes what an earlier cell
  reads sends that cell back to run, for eight passes at most; past that those cells stay as they are,
  marked stale, so cells that feed each other end. A worker can also go between cells, and the client
  replaces it as it sends without saying so, so `runner::submit` compares `Client::starts` around the
  send and runs the cells before it first. On such a pass a cell shows `#BUSY!` only when its
  turn comes, and until then reads as what it last gave, so a formula over a later cell gives the
  earlier one its result rather than `#BUSY!`. A worker that doesn't start is tried three more
  times, as a thread starting another process can leak a descriptor that makes the launcher refuse.
- **Initialization** (`python/initialization.rs`). `Workbook::python_env`, an `Arc` so an undo step
  doesn't copy it, holds `model::PyEnvironment`: the code, `user_modified`, the GUID and, opaque to all
  but the xlsx crate, the raw XML around them. `python.initialization` sets it through `edit`, so it is
  one undo step. `DocRun::init` is its run state. `step` runs it before the next cell in any worker that
  hasn't (`init.in_worker`, which `worker_gone` clears), as a `Request::Run` with `init`, through the
  same `send` as a cell, so the deadline and the replay limit hold. `plan` compares the code and whose
  it is (`same_run`) along with the cells: a change, undo included, stops the worker and makes every cell
  due. A failure is the `init.failed` diagnostic kept in `DocRun::init`, a state of the workbook:
  `python_diagnostic` returns it for every cell without a copy per cell, `show_failure` writes `#PYTHON!`
  to all of them in one commit, and it holds until the code changes, someone asks cells to run, or a new
  package list arrives. The worker that failed goes with what it had done. `python.stop` sets
  `init.stopped`, which counts as manual calculation, so code that never ends doesn't start again with the
  next edit. Code past `MAX_INIT_CHARS` fails without a worker.
  **Whose code it is** (`protocol::Init`) decides what a failing line means, in the worker
  (`initialize` in `shim.py`), which runs code that isn't a person's a statement at a time. It leaves
  out imports of what it provides lazily (`np`, `pd`, `sm`, `plt`, `sns`). It skips an import
  that fails, a statement that hits a name such a line would have bound, and a top-level call on a module
  that fails; its reply's value lists the packages it went without, as rows of `module, line`, which
  become `init.skipped`. Anything else is the code's failure. A person's code is one `exec`, unless it
  imports `excel`, in which case only that module gets those rules. The static check of packages reads
  a person's code; other code is reported from what it actually lacked. The run log has a
  row for each run with `PythonRun::initialization`.
- **Waiting for a save** (`python/wait.rs`). `python_workbook_busy` says a workbook has a request out or
  cells queued; a cell stale under manual calculation is neither. A `PythonWait` is that question with a
  cap: `start_timeout` + `timeout` for each cell left + 1 s, at most `MAX_WAIT`, 120 s. Cells left are those
  queued or running, those before them that a fresh worker must run again, and initialization code that
  hasn't run; more of them during the wait add to the cap, and a chosen `waitSeconds` doesn't move. It
  blocks nothing, and takes no results: `python_poll` does that. It remembers the cells it saw queued, by
  sheet identity, because `turn_off` forgets their entries: `python_wait_unfinished` names those that are
  still Python cells and aren't `Clean`, or are unsettled or stopped. `file.save` and `file.saveAs` take
  `wait` and `waitSeconds` for a caller with no frames to keep going: `python_settle_for_save` polls in a
  bounded loop, inside the command, which has no undo step to confuse. A command inside another doesn't
  block, and the interface takes `wait` off before it runs a save (`SheetApp::run_command`).
- **What is said.** `Session::python_diagnostic` gives one diagnostic per cell, from the catalog in
  `gridcraft_python::diagnostics`; `python.diagnostics` lists them. Each workbook keeps its last 200
  runs (`python.runLog`), with printed output cut to 64 KiB.
- **The environment** (`python/environment.rs`). `Session::python_environment` reports the state,
  the sandbox, uv, the interpreter and the packages. Working it out starts Python, so it runs on a
  thread of its own; `python_poll` picks up the answer, and it runs again when the capability changes
  and on `python.refreshEnvironment`. While it is out, due cells wait showing `#BUSY!`. On the web and
  on systems without a sandbox the answer, `blocked.platform`, is there at once.

**A save that waits** (`ui_egui::save_wait`). `SheetApp::run_or_alert`, the way a person's commands
arrive, sends `file.save` and `file.saveAs` through `person_save_waits`, which enters the draft and then
starts a `SaveWait` if `python_wait_begin` finds something to wait for. A workbook has at most one: a
later save joins it, one entry per file, without touching its cap. `poll`, from `SheetApp::logic` after
`python_ui::tick`, ends the waits that are over, and a frame never blocks. The saves run in the workbook's
window through `SheetApp::show_window` and `Session::run`, with no `settle_drafts`, so a draft begun during
the wait stays one. `show_window` doesn't stamp the window as active, so a save behind the person can't
change the order `activate_recent` follows: a close after the save comes back to where they came from,
or leaves them where they are. Save Changes' Save sets `close_after`, and nothing closes unless the save
worked, left the workbook clean and no draft is open, and left no cell unfinished unless Save Now said so:
the message about those cells would go with the window, and their results with the worker. `must_ask_before_closing` makes `continue_quit` ask
about a workbook with a wait, except one whose save has been answered for, which carries the quit on when
it is done (`quit_waits`). `ask_before_closing_waiting_save` does the same for a clean workbook's close,
which the command itself wouldn't ask about. A wait is keyed by the workbook's `uid`, and one whose
workbook has gone answers its replies and ends without a save. The control channel runs through
`control::save_request`: `control_save` refuses what would need a dialog, and `Outcome::Hold` gives `hold`
the request's reply to send when the save has happened. AutoSave reads `python_workbook_busy` and
`SaveWaits::waiting` and doesn't start. The app's control server and `gridcraft_mcp::Remote` read
`reply_may_be_held`, so a client that gave up couldn't report a save still coming as a failure, or retry it.

The Python editor window also edits the initialization code (`python_window::Target::initialization`,
`PyWindow::initialization`), so the code, its checks, its questions and how it follows its workbook are
one path: Apply runs `python.initialization`, and there is no sheet to follow, so `stale` compares the
workbook's code with what the window loaded. `python_ui::checks::update_initialization` gives its problems, and the
window's strip shows the failure. A cell's editor leaves the failure's line out, since it counts in the other code.

## Sandbox

`crates/sandbox` runs a helper process confined. The parent spawns `launch_command`, which runs the
application's own executable with `--sandbox-launch` and the `Policy` (read paths, executable
paths) and a `Canary` path; both apps pass their arguments to `gridcraft_sandbox::handle_args`
first thing in `main`. The launcher confines itself (Linux: Landlock, then its seccomp filters) or
already runs confined (macOS: `sandbox-exec` with a deny-by-default profile built from the policy),
then must be refused reading the canary, writing, a TCP connection and starting a process before
it `exec`s the program; otherwise it exits with code 97 and the program never runs. Other systems
report `Support::Unsupported`. The crate has no `unsafe` code (the system calls are behind the
`landlock` and `seccompiler` crates). `gridcraft_python::process` runs cells' workers through it.
`gridcraft-cli sandbox-check` runs the same checks on a machine. Details: [sandbox.md](sandbox.md).

## Equations

An equation is a shape (`ShapeKind::Equation`) whose `text` is its LaTeX source; it selects,
opens and deletes as its own object kind (`ObjKind::Equation`). `gridcraft-math` is the only
code that calls the layout engine (RaTeX), and only through `layout`, `check` and `to_omml`,
which cap the source (4 KB, nesting 32) and run under `catch_unwind`. The result is cached on
the shape (`Shape::math`, never saved); `Workbook::refresh_equations` rebuilds stale layouts when
a document is created and on every commit, so painting only reads the cache and nothing in a
paint path can reach the layout engine. One call lays out at most 128 KB of source
(`MAX_EQUATION_LAYOUT_BYTES`); equations past it keep an error and show their source until edited,
so a file with thousands of equations opens in bounded time. The equation editor's preview and the gallery are the
only other callers, through the same guarded entry points. The gate that admitted RaTeX is
recorded in [`decisions.md`](decisions.md) and re-run by `crates/math/tests/gate.rs`.

`gridcraft-python` (L2, used by the engine and the CLI) holds the worker. `interp` finds an interpreter:
the recommended `~/.gridcraft/python` environment, the active virtual environment, uv's own Pythons
(newest first), conda, `python3` and `python` on `PATH`, then the usual install folders outside
`PATH`, absolute paths to existing files only, Python 3.11 or newer, each asked its version with `<python> -I -S -E -c
<script>` under a deadline and a bounded read. `process::ProcessTransport` starts `<python> -I -S -E
<shim>` with an empty environment (`OPENBLAS_NUM_THREADS=1` only) in a fresh private folder holding
a read-only copy of `shim.py` and an empty working directory; `process::Launch` is the seam where a
wrapper can change the command. The shim adds site-packages from `sysconfig` without `site`, keeps
one `__main__` for all cells until `reset`, answers `xl(ref)` from the inputs the parent sent, and
converts the last expression's value. The engine's cell runner and its check of the environment, and
`gridcraft-cli python-check`, start a worker through the sandbox (`ProcessTransport::confined`). `protocol`
is the JSON-lines protocol (`run {id, code, inputs}`, `reset {id}`; `ready {selfTest}` once, then
one `done {id, ok, value or error, stdout}` per request) with a byte or character cap on every field
and array shapes checked on read: every line from the worker is untrusted. `run` holds the limits of
a worker (time per request, extra time to start, memory, line cap), the typed outcome of a request
with the cell error each one shows (`#CONNECT!` when the worker could not start), and the
`Transport` trait through which workers start. `client::Client` runs one request at a time with an
absolute deadline set by the parent, through a reader thread (lines capped in bytes, a bounded
channel), a writer thread and a memory watcher (`/proc` on Linux, `ps` every 500 ms on macOS), with
each reply also reporting the worker's peak (`getrusage`), so that a spike between two reads counts
against the request that made it, and
kills the worker at the deadline, over the memory limit, on a crash, and at a line over the cap,
malformed JSON, a wrong id, an unsolicited line or a second reply; the next request starts a fresh
worker. `fake::FakeTransport` stands in for the interpreter in tests on every system; the web
build's client reports every request as failed to start.

## Agent control

- Desktop control channel: `gridcraft --control 7979`, JSON lines on localhost
  (`engine.execute`, `ui.screenshot`, pointer and keyboard input…), see
  [`control-protocol.md`](control-protocol.md).
- MCP: `gridcraft-cli mcp`, headless or `--connect` to a running app, see [`mcp.md`](mcp.md).
- CLI: [`cli.md`](cli.md).

## Platforms and builds

macOS universal, Windows x64/x86/arm64, Linux x86_64/aarch64/riscv64 (AppImage, Flatpak, deb,
rpm, tarball), FreeBSD, and web (WASM; WebGPU or WebGL2). Release workflows in
`.github/workflows/release.yml`; see [`releasing.md`](releasing.md).

## Known architectural gaps

- No threading in calc (see [hardware-parity.md](hardware-parity.md)).
- No string catalog or text shaping (see [localization-parity.md](localization-parity.md)).
- XLSX reader builds the whole workbook in memory; no streaming (#175).
- Unmodelled XLSX parts are not carried through a save.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-11-05 | minor | A save waits for Python cells: `engine::python::wait`, `ui_egui::save_wait`, `Outcome::Hold`, `control::reply_timeout` |
| 2026-11-05 | minor | A Python object's size and table: `protocol::Table`, `model::PyTable`, `PyOutput::table`, `PyCode::spill_lines`, `python_card::table` |
| 2026-11-05 | minor | Dates in Python cells: `engine::python::dates`, `protocol::Stamp`, `PyCode::dates` |
| 2026-11-05 | minor | Initialization code: `model::PyEnvironment`, `engine::python::initialization`, `protocol::Init` and the worker's `initialize`, the Python part's `environmentDefinition`, and the editor window's second subject |
| 2026-11-05 | minor | The engine runs Python cells: `engine::python`, `PythonCapability`, `python_poll` and the commands that run and report; `PyCode::busy`; `calc::evaluate_reference`. The app doesn't turn it on yet |
| 2026-11-02 | minor | `python.clear` (Convert to Normal Cell) and `cmd::python::clear_refusal` |
| 2026-11-02 | minor | Python cells in the interface: `python_card.rs` (PY tags, the card, the return-type items) and `python.output` |
| 2026-11-02 | minor | Python mode: `xl()` pointing through the shared point mode (`EditState::can_point`, `insert_ref`, `editor::python_xl`) |
| 2026-11-04 | minor | The `sandbox` crate (L0) and its launcher entry in both apps |
| 2026-11-02 | minor | Python mode in the formula bar: Python mode on the shared editor, `code_field` and its stop keys; drafts follow their sheet by `model::SheetUid` |
| 2026-11-02 | minor | Python cells as data: `model::PyCode`, the file forms, calc laying out kept results, the limits on open, and what copying, filling and sorting do to `xl()` references; edits that would push code past the limit are refused |
| 2026-11-01 | trivial | Equations: the layout cache answers for a source over the cap by its length alone, without hashing it |
| 2026-11-01 | minor | Equations: the `math` crate (L1), the layout cache and code editor marks |
| 2026-11-01 | minor | The Python editor window (`python_window.rs`) and its questions as state |
| 2026-11-01 | minor | Python cells in the interface: Python mode on the shared editor, `code_field` and its stop keys; drafts follow their sheet by `model::SheetUid` |
| 2026-10-31 | minor | Recently used functions (`recent.rs`, `Prefs::recent_functions`) behind `formulas.recentlyUsed` |
| 2026-10-31 | minor | Function Library module (`function_library.rs`); accent icons (`icons::paint_accent`, `widgets::small_button_accent`) |
| 2026-10-25 | trivial | The hidden names that hold macros are `_gc_macro_<n>_<k>` |
| 2026-10-24 | minor | Macro engine on the building blocks (`macros/`, `cmd/macros.rs`) |
| 2026-10-26 | minor | The `python` crate: the client, its threads, deadlines and kills, and the fake worker |
| 2026-10-26 | minor | The `python` crate: interpreter detection, the worker shim and the process launch |
| 2026-10-26 | minor | The `python` crate (L2): the worker protocol and its caps, limits and outcomes |
| 2026-10-24 | minor | Engine building blocks for multi-command actions: undo groups, recording buffer, document trust, chunked hidden-name storage |
| 2026-10-14 | minor | Document Undo's retained-cell-storage budget and latest-step guarantee |
| 2026-10-10 | major | Created from the code on main: crates, layers, data flow, agent control, known gaps |
