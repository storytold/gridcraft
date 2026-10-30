# Architecture

> **Imported formula results update:** 2026-10-30 · Cache-preserving open, missing-result calculation and explicit recalculation build on #2308 by @mrzv. Saved results can be stale; structural edits still perform full recalculation. Readiness estimates unchanged.
>
> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-30 · **Change:** trivial (the hidden names that hold macros; earlier: macro engine on the engine building blocks) · **Target:** Microsoft Excel (Microsoft 365)

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
| L2 | `model` | ~2.2k | Workbook, sheets, copy-on-write cell store (64-row bands behind `Arc`), interned styles, names, tables, CF, validation, comments, charts, shapes, images, sparklines, print settings, protection |
| L2 | `calc` | ~2.9k | Evaluator (references, names, tables, LET/LAMBDA, special forms like OFFSET/INDIRECT/SUBTOTAL) and dependency-graph recalculation with dynamic arrays, spills and cycle detection; single-threaded |
| L2 | `xlsx` | ~10.7k | XLSX read/write (transitional and strict read), CSV/TSV, XLSB and ODS data import, PivotTable caches, DrawingML charts and drawings; byte slices only (WASM-safe); lenient reader that reports warnings |
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
   Read-only consumers can explicitly call `Calc::recalc_missing`: only formula cells with
   `Value::Empty` results are calculated, with nonempty cached results (including errors, empty
   strings, volatile functions and unknown UDFs) treated as fixed inputs. Formula metadata and
   cached anchors' existing spill values/ranges are retained. Missing formulas use the existing
   evaluator, including its function support and bounded spill/dynamic follow-up rounds;
   unparseable missing formulas return `#NAME?`. This neither validates saved results nor
   reconstructs missing spill data for cached anchors. The full dependency graph is restored
   before return, so later edits recalculate normally rather than continuing to trust caches.
   Cycles detected while evaluating missing formulas remain reported even when dynamic references
   make them invisible to the restored static graph.
4. **Rendering**: the egui grid reads display values (number formats applied in `engine/display.rs`)
   for the visible window only; charts render through `crates/chart` primitives.
5. **Files**: `engine/io.rs` sniffs content (XLSX, XLSB, ODS) before trusting the extension and
   dispatches to `crates/xlsx`; imports that lose data return warnings shown to the user.

### Imported formula results

`file.open` now uses `DocState::from_cached`: saved nonempty results are kept, missing
formula results are calculated with `Calc::recalc_missing`, and the full dependency graph
remains available for edits. The open response and import message warn that saved results
may be stale. For XLSX input, `file.open` deliberately retains saved nonempty results even
when `calcPr` requests `fullCalcOnLoad`; that file flag does not override this open policy.
Callers requiring a full recalculation on open must explicitly pass `recalculate: true`.
Calculate Now also performs the
existing full recalculation. Neither option makes unsupported functions calculable.
Unresolved function calls retain their explicit argument dependencies but are not made
volatile merely because the function is absent. Workbook-defined calls and supported
dynamic references still recalculate normally. Structural edits still use a full recalc.
XLSX import now retains the file's stored legacy/dynamic array follower values in
`Sheet::spill`, with bounded declared ranges in `spill_ranges`. They remain separate from
ordinary cells, so a later full recalculation can spill without colliding with imported caches.
Export walks stored spill rows and matching cached entries rather than expanding the
declared area, so sparse results beyond a million positions survive. Legacy-array edit
protection checks declared ranges even when a follower has no cached value. Formatting
and replacement of the entire array remain allowed. Clear Contents and Clear All validate
the selected footprint before editing, even when the cache has no ordinary follower cells.
A union of selected areas may cover the whole array; filtered-out rows are not cleared and
therefore cannot contribute to full coverage. Only stored values are retained;
absent values are not invented or reconstructed. Missing anchors with explicit legacy/dynamic
array metadata keep their saved followers when evaluation fails, yields no array or a new
array is blocked. A successful replacement array still removes obsolete followers; normal
full/incremental calculation retains its existing cleanup policy. The same retained range is
available to missing dependents during the calculation pass. A follower-only cache also
triggers the generic freshness notice. Accepted multi-cell ranges use their existing scan
count/area/work budgets independently of single-cell metadata saturation; the metadata map
can therefore contain at most twice the 65,536-range cap, with at most 65,536 multi-cell
ranges participating in follower membership checks.
This does not establish cache freshness or reconstruct absent cached spill data. Saved filename-dependent results, including `CELL("filename")`, can also retain a previous file name or an earlier cached error until explicit recalculation.

## UI building blocks

The panels in `crates/ui-egui` share a few pieces, so a new feature reuses them instead of
drawing its own:

- **Colours**: `theme::Tokens`, each with a light and a dark value. The sheet stays white in both
  themes, so colours drawn on it (`sheet_ref_colors`, `sheet_control_*`, icons through
  `icons::paint_on_sheet`) keep their light values. `theme::contrast_tests` holds every
  foreground/background pair to WCAG 2 (4.5:1 for text, 3:1 for glyphs) in both themes; a new
  token pair is added there. Icons (`icons.rs`, drawn in code) take their paper and category
  colours (`theme::Cat`) from the tokens.
- **Ribbon widgets**: `widgets.rs` has the buttons every tab is built from (`big_button`,
  `small_button`, `split_button`, `big_split_button`, `toggle_button`), translated through
  `i18n` in one place. Split buttons return the main and arrow responses separately, with
  accessible names (`AutoSum`, `AutoSum options`). The ribbon card scrolls a tab wider than the
  window, with arrows at its edges; `ribbon::RibbonOverflow` records the groups' size each frame,
  and a test keeps every tab within the card's three rows.
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
| 2026-10-30 | minor | Keep stored array followers when a missing anchor cannot produce a replacement; retain accepted multi-cell range metadata after single-cell metadata saturation; include follower-only caches in the freshness notice. |
| 2026-10-30 | minor | Reject partial Clear Contents/Clear All selections inside legacy arrays using declared ranges, including missing cached followers; allow whole-array selection unions and respect filter-hidden rows. |
| 2026-10-30 | minor | Export sparse array caches by stored entries without the million-cell cutoff; protect missing legacy-array followers using declared ranges, including when no follower cache exists. |
| 2026-10-30 | minor | Preserve XLSX legacy/dynamic array follower caches in spill storage and retain bounded ranges, so cached open and save/reopen do not lose stored array results; explicit full recalculation remains supported. |
| 2026-10-30 | trivial | Clarify that XLSX fullCalcOnLoad does not override cached open; explicit recalculate: true is required for full recalculation on open. |
| 2026-10-25 | trivial | The hidden names that hold macros are `_gc_macro_<n>_<k>` |
| 2026-10-24 | minor | Macro engine on the building blocks (`macros/`, `cmd/macros.rs`) |
| 2026-10-28 | minor | Cache-preserving imported formula open and explicit recalculation, building on #2308; synthetic lifecycle coverage, remaining structural-recalc limitation (#2770). |
| 2026-10-25 | minor | Retain freshly detected dynamic cycles when cache-preserving calculation restores the full dependency graph |
| 2026-10-24 | minor | Engine building blocks for multi-command actions: undo groups, recording buffer, document trust, chunked hidden-name storage |
| 2026-10-22 | minor | Add the opt-in `Calc::recalc_missing` API for cache-preserving read-only calculation; retain normal incremental editing afterward |
| 2026-10-14 | minor | Document Undo's retained-cell-storage budget and latest-step guarantee |
| 2026-10-10 | major | Created from the code on main: crates, layers, data flow, agent control, known gaps |
