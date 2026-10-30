# Architecture

> **Last reviewed:** 2026-10-29 · **Last updated:** 2026-10-29 · **Change:** minor (native seeded processes, shared-work controls and upstream macro integration) · **Target:** GridCraft native engine, CLI/MCP and WASM

How GridCraft is built today. Rules for contributors are in [`AGENTS.md`](../AGENTS.md).

The [actuarial extension](actuarial.md) lives at L1 in `functions::actuarial`.
Its 95 deterministic worksheet functions use the existing registry and
[explicit scientific-method contracts](actuarial-scientific.md). CLI/MCP and
workbook uncertainty reuse engine commands; no MC sampler is copied or exposed.
`ACT.*` names are GridCraft extensions excluded from Excel parity counts.

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
| L5 | `engine` | ~17.7k | `Session`: open workbooks, selection, history (COW snapshots), clipboard, fill, sort, filter, PivotTables, what-if, print, file I/O (`io.rs`), and the command registry (`cmd/*`, runtime commands discoverable through CLI/MCP) with the Excel ribbon catalog (`catalog.rs`) |
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

## Monte Carlo analysis

`data.monteCarlo` reuses `whatif::Trial` for seeded sampling over arbitrary workbook formulas.
Trials do not modify the source document; only the final static report becomes an undo step.
The engine owns random/replicated-LHS sampling, Gaussian dependence, stable summaries,
sampling-error diagnostics and sample-derived tornado sensitivity; the egui dialog
only edits command parameters and drives the shared `analysis.*` job lifecycle for progress
and cancellation. Synchronous callers retain `data.monteCarlo`. A neutral calc `WorkHook`
connects `functions::Ctx`/calc accounting to an engine-owned persistent meter, including nested
commands and uses atomic run counters. What-if trials alone debit the per-workbook reservation
bank through bounded prepaid blocks, keeping clock/locks out of the per-node hot path.
Ordinary commands stay independent of the analysis bank: queries have a 1B-unit cap and
edits/recalculation/imports a finite 10B cap, including file opening without a document. Nested
queries retain their own cap and charge the parent operation; document trust never raises it. Terminal work failure
uses the command rollback barrier; report publication stays one atomic undoable edit.
Imports recalculate a local document and check admission before replacing a blank or adding a
window. Conditional-formatting formulas have an independent document-owned renderer bank,
per-cell limit and bounded revision/identity/time-aware cache; painting does not consume the
analysis allowance. These runtime controls are shared by native and WASM front ends.
See [`monte-carlo.md`](monte-carlo.md) for limits and the
reproducibility contract. No plugin host or external runtime is introduced.

### Seeded worksheet simulation extension

The [Monte Carlo process contract](monte-carlo-processes.md) adds neutral scalar requests in L1
`Ctx::simulation` and a scoped L2 `SimulationHook` captured once per calculation pass. L5 owns
all numerical samplers, SplitMix64 streams, stationary AR1 paths, bounded caches and failure
latches. No functions-to-engine dependency or second RNG is introduced. Every trial drops its
sampling scope before publication/source recalculation. Volatile MC functions seed the dirty
closure even without a dependency on a sampled input.

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
| 2026-10-29 | minor | Seeded worksheet counts and stationary AR1; integrate upstream macros with analysis lifecycle guards |
| 2026-10-28 | minor | Bounded conditional-format rendering, safe file-open admission and integration with upstream Clippy conventions |
| 2026-10-28 | minor | Atomic work accounting, expiring prepaid analysis credit and independent ordinary-command limits |
| 2026-10-28 | minor | Shared analysis work accounting, atomic terminal rollback and portable progress/cancellation lifecycle |
| 2026-10-28 | minor | Clarify native GridCraft analysis and document Monte Carlo numerical contracts |
| 2026-10-27 | minor | Monte Carlo reuses isolated what-if trials; generic command, report and thin UI |
| 2026-10-25 | trivial | The hidden names that hold macros are `_gc_macro_<n>_<k>` |
| 2026-10-24 | minor | Macro engine on the building blocks (`macros/`, `cmd/macros.rs`) |
| 2026-10-24 | minor | Engine building blocks for multi-command actions: undo groups, recording buffer, document trust, chunked hidden-name storage |
| 2026-10-14 | minor | Document Undo's retained-cell-storage budget and latest-step guarantee |
| 2026-10-10 | major | Created from the code on main: crates, layers, data flow, agent control, known gaps |
