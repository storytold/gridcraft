# Architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-17 · **Change:** major (created from the code on main; supersedes the local-only plan/architecture.md for committed docs) · **Target:** Microsoft Excel (Microsoft 365)

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
   `Session::execute` catches escaped panics and restores the workbook (never-crash guard).
2. **Edits** change the model through copy-on-write bands; undo keeps the previous snapshot.
3. **Recalc** marks dependents of changed cells dirty through the dependency graph (with range
   nodes for range dependents), evaluates in topological order, spills dynamic arrays and reports
   `#SPILL!`/cycles. Volatile functions are always dirty.
4. **Rendering**: the egui grid reads display values (number formats applied in `engine/display.rs`)
   for the visible window only; charts render through `crates/chart` primitives.
5. **Files**: `engine/io.rs` sniffs content (XLSX, XLSB, ODS) before trusting the extension and
   dispatches to `crates/xlsx`; imports that lose data return warnings shown to the user.

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
| 2026-10-10 | major | Created from the code on main: crates, layers, data flow, agent control, known gaps |


## Large workbook imports (2026-10-16)

The asynchronous file inbox calls `Session::open_bytes` directly, avoiding a base64/JSON
round trip. It reports completed imports separately from the web engine's ready event.
Row fitting collects one index per stored row, rather than one per cell.

For XLSX inputs of at least 20 MiB and one million model cells, complete saved formula
results can be displayed initially while the dependency graph is built. Automatic calculation
settings are retained: edits still update dependents, and Calculate Now forces full evaluation.
A visible import notice identifies this cached opening mode. Files with missing cached values
or array/spill ranges that the reader must reconstruct retain the ordinary full recalculation.
Large worksheet parts discard retained expression trees after expanding shared formulas;
formula text and cached values remain, and expressions are parsed on demand.

This does not move import or recalculation off the UI thread. Hosts may show transfer progress
and import stages; smooth per-row progress still needs a yielding or worker-backed reader.


## Cooperative workbook opening (2026-10-17)

`core::work::WorkControl` supplies scheduling-neutral progress and cancellation checkpoints.
The XLSX reader yields between bounded decompression chunks and XML events, followed by row
height preparation, dependency construction and initial calculation. Preserved worksheet
fragments reuse the existing decompressed bytes. Synchronous engine/CLI callers poll the same
import implementation with an uninterrupted control; there is no second parser.

`file::prepare_open` owns the candidate document until completion; `Session::finish_import`
publishes it atomically. Cancelled or failed imports leave existing workbooks intact. The desktop
shell prepares on a worker thread; the browser shell yields through MessageChannel tasks on a
short time budget, keeping rendering and cancel messages serviceable without serializing a second
workbook model. The import UI disables document commands until publication. Same-origin hosts
can consume `gridcraft:progress` and send `gridcraft:cancel`; cancellation produces
`gridcraft:cancelled` rather than a startup failure.

This applies to ordinary XLSX/XLSM imports. Legacy readers, password decryption, individual
formula evaluations, and saving remain synchronous on the browser thread; this is not a general
background execution system. Memory limits and saved-result eligibility remain unchanged.


### Import progress presentation — 2026-10-17

Progress scopes carry a shared sheet name alongside the sheet ordinal/count. Decompression,
worksheet parsing and row fitting identify the current sheet in both the native progress window
and same-origin host messages (`sheetName`). Shared ownership avoids copying the name at every
checkpoint. The loading window explicitly paints the ordinary theme grid background instead of
leaving the viewport clear colour exposed. Generated light/dark UI checks cover background and
sheet-label rendering; parser tests verify names stay attached to scoped progress.
