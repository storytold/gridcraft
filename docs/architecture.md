# Architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-17 · **Change:** minor (Portuguese date names and Record.ToTable) · **Target:** Microsoft Excel (Microsoft 365)

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
| L6 | `external` | ~0.5k | Native data access for Get & Transform and WEBSERVICE: HTTP(S) (system TLS on Windows/macOS, rustls elsewhere) and ODBC on Windows; the desktop app and CLI inject it into the engine's `query::DataAccess` trait (never built for wasm) |
| L7 | `apps/gridcraft`, `apps/gridcraft-cli`, `apps/gridcraft-web` | ~2.2k | Desktop (eframe/wgpu), CLI (`info`, `convert`, `eval`, `cat`, `run`, `commands`, `functions`, `mcp`, `send`), web (WASM via trunk) |
| — | `xtask` | — | `ci`, `parity`, `assets`, `layers`, `wasm`, `version`, `stats` |

## Data flow

1. **Every action is a command.** The UI, CLI, control channel and MCP call
   `Session::execute(id, params)`. Commands are `CommandSpec { id, label, menu, shortcut, params,
   enabled, run, journal, undoable }`; ids follow Excel's ribbon (`home.bold`, `insert.chart`,
   `data.sortAscending`) plus primitives (`cell.set`, `selection.set`, `range.setValues`).
   `Session::execute` catches escaped panics and restores the workbook (never-crash guard).
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
6. **External data** (Get & Transform, WEBSERVICE): `engine/query` evaluates the workbook's
   queries (`model::query`: a source and typed steps) through the `DataAccess` trait the engine
   owns. Hosts inject an implementation (`Session::set_data_access`, the `external` crate);
   without one, files go through `std::fs` and web/ODBC are disabled with a reason. A document
   opened with external content stays blocked until `data.enableContent`. Excel's own queries
   (M formulas from the DataMashup part, `model::ExcelQuery`) run in `engine/query/m`: a lexer,
   parser and lazy evaluator for Power Query M over the same sources, data access and loading,
   with recursion depth and total work bounded.

## Power Query: one model, M

**Decision** (proposed 2026-10-15, to agree with the Get & Transform contributors): a query is
its M formula, as in Excel. Everything else is a view of it or a way to run and store it.

- **Model.** A query is a name, an M expression (normally a `let` whose bindings are the
  Applied Steps), its load target and refresh options. Excel's queries and GridCraft's are the
  same type (today: `model::ExcelQuery` beside `model::query::Query` with typed steps).
- **One engine.** Every query runs through `engine/query/m`. A transform is implemented once,
  as the `Table.*` (or other) library function the M calls; the typed-step code becomes those
  functions' implementation where it is the better one.
- **Steps are a codec.** Each Power Query Editor step is a pair: `StepOp → M` (the M Excel's editor
  writes for it) and `M → StepOp` (recognising that pattern in a binding). A recognised step opens
  its dialog; any other binding is edited as M (formula bar, Advanced Editor).
- **Storage is Excel's.** Queries are saved in the DataMashup part ([MS-QDEFF]) with
  `connections.xml` and query tables, so Excel lists and refreshes queries made in GridCraft;
  Excel's own parts are written back unchanged when nothing changed. GridCraft's JSON part
  (`customXml/gridcraftQueries.xml`) is read only to migrate older files.
- **The oracle is Excel's cache.** A workbook saved by Excel holds the values its last refresh
  loaded. Refreshing in GridCraft and comparing (values, row order, filtered rows, calculated
  columns) is the conformance test; Excel-authored Power Query workbooks live in the corpus repo
  (craftrules `standards/test-corpora.md`), never here.

Seams, so work splits without touching the same files:

| Area | Where | Unit of work |
|---|---|---|
| M language | `query/m/{lexer,parser,eval}.rs` | a language feature + tests |
| M library | `query/m/library/*`, one file per M module (`text`, `number`, `date`, `list`, `record`, `convert`, `splitter`, `table`, `source`) | one function: an entry in `FUNCTIONS`, its code, a test |
| Sources | `query/m/library/source.rs` over `query::DataAccess` | one connector |
| Step codec | per step, beside `query/mtext.rs` | one `StepOp` both ways + a round-trip test |
| Editor UI | `ui-egui/src/power_query.rs` | one dialog over its step codec |
| Storage | `xlsx` (DataMashup, connections, query tables) | one part, round trip + damaged input |

Adding an M function: add `(name, min, max)` to `library::FUNCTIONS`, implement it in its module's
`call`, add a test with M as the Power Query Editor writes it and a hostile case;
`every_library_function_dispatches` and the random-formula test cover dispatch and panics.

`Record.ToTable` copies field values without forcing them (held errors and lazy values stay as
they are) and charges the evaluator budget per field before building rows.

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
- Two query models until the Power Query decision above is carried out: GridCraft's typed steps
  (saved in its own JSON part, which Excel doesn't read) and Excel's M.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-17 | minor | Bounded Portuguese date names/standard formatting and Record.ToTable; no query-model, editor or storage change |
| 2026-10-15 | minor | Power Query direction: M is the one query model (steps as a codec, Excel's storage, Excel's cache as the oracle); seams for parallel work |
| 2026-10-15 | minor | The M interpreter (`engine/query/m`) for Excel's Power Query queries |
| 2026-10-14 | minor | Document Undo's retained-cell-storage budget and latest-step guarantee |
| 2026-10-14 | minor | Get & Transform data flow; the `external` crate (L6) |
| 2026-10-10 | major | Created from the code on main: crates, layers, data flow, agent control, known gaps |
