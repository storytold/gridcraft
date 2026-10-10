# Architecture

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-10 · **Change:** minor (M12, #191: `locale` and `l10n` crates, local formulas and regional formats; line counts re-measured) · **Target:** Microsoft Excel (Microsoft 365)

How GridCraft is built today. Rules for contributors are in [`AGENTS.md`](../AGENTS.md).

## Workspace and layering

An engine-first Cargo workspace. Layering is enforced by `cargo xtask layers`; nothing below L6
depends on egui, eframe, winit or rfd, so the UI crate is swappable. Line counts are Rust lines
including tests, 2026-10-10.

| Layer | Crate | Lines | Job |
|---|---|---|---|
| L0 | `core` | ~1.7k | Cell and range addresses (A1, R1C1, sheet-qualified; 1,048,576 × 16,384), `Value`, `CellError`, Excel coercion, 1900/1904 date serials, typed-input parsing in the regional format |
| L0 | `numfmt` | ~3.2k | Excel number-format codes (sections, conditions, colours, dates, elapsed time, fractions, scientific, fill), General, `TEXT()`; regional separators, month and day names, localized format-code letters |
| L0 | `locale` | ~0.9k | Formula languages (function names, error values, booleans, keywords) and regional formats (separators, dates, month and day names, currency) for 12 languages, compiled from `data/*.toml` with sources per entry |
| L1 | `formula` | ~3.3k | Tokenizer, parser, AST, printer (A1 and R1C1, canonical and localized), reference adjustment for copy, fill and structural edits |
| L1 | `functions` | ~13.9k | The function library: `FnSpec` registry (name, arity, category, scalar args, signature, description) and pure implementations; automatic array lifting |
| L2 | `model` | ~2.2k | Workbook, sheets, copy-on-write cell store (64-row bands behind `Arc`), interned styles, names, tables, CF, validation, comments, charts, shapes, images, sparklines, print settings, protection |
| L2 | `calc` | ~3.0k | Evaluator (references, names, tables, LET/LAMBDA, special forms like OFFSET/INDIRECT/SUBTOTAL) and dependency-graph recalculation with dynamic arrays, spills and cycle detection; single-threaded |
| L2 | `xlsx` | ~10.9k | XLSX read/write (transitional and strict read), CSV/TSV, XLSB and ODS data import, PivotTable caches, DrawingML charts and drawings; byte slices only (WASM-safe); lenient reader that reports warnings |
| L2 | `pdf` | ~1k | Small PDF 1.7 writer (14 standard fonts, vectors, images) for print and export |
| L3 | `chart` | ~3.1k | Toolkit-free chart layout to drawing primitives, a rasterizer, sparklines |
| L4 | `l10n` | ~0.8k | Interface messages (Fluent, `locales/<tag>/`, embedded at build time) and localized keyboard shortcuts |
| L5 | `engine` | ~20.4k | `Session`: open workbooks, selection, history (COW snapshots), clipboard, fill, sort, filter, PivotTables, what-if, print, file I/O (`io.rs`), locale preferences, and the command registry (`cmd/*`, 319 commands) with the Excel ribbon catalog (`catalog.rs`) |
| L6 | `ui-egui` | ~16.0k | Excel-style UI (title bar, QAT, ribbon, formula bar, virtualised grid, editor, tabs, status bar, dialogs, task panes, chart view) and the control channel |
| L6 | `mcp` | ~1.6k | MCP server over stdio (hand-written JSON-RPC), headless or bridged to a running app |
| L7 | `apps/gridcraft`, `apps/gridcraft-cli`, `apps/gridcraft-web` | ~2.9k | Desktop (eframe/wgpu), CLI (`info`, `convert`, `eval`, `cat`, `run`, `commands`, `functions`, `mcp`, `send`), web (WASM via trunk) |
| — | `xtask` | — | `ci`, `parity`, `assets`, `layers`, `wasm`, `locales`, `version`, `stats` |

**Languages.** Workbooks, files and the journal hold canonical formulas (English function names,
`,` and `.`). The formula bar, commands with `inputLocal`/`formulaLocal` and CSV translate at the
boundary through the session's formula language and regional format (`locale` + `formula`);
interface text comes from `l10n`.

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
- No text shaping or right-to-left layout (see [localization-parity.md](localization-parity.md)).
- XLSX reader builds the whole workbook in memory; no streaming (#175).
- Unmodelled XLSX parts are not carried through a save.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-10 | minor | M12 (#191): `locale` (L0) and `l10n` (L4) crates, localized formulas and regional formats at the boundary; line counts and command count re-measured |
| 2026-10-10 | major | Created from the code on main: crates, layers, data flow, agent control, known gaps |
