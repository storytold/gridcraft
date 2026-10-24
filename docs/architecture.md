# Architecture

> **Last reviewed:** 2026-10-24 · **Last updated:** 2026-10-24 · **Change:** minor (compact PDF font bundle, exact Latin style fallback and bounded prefix search) · **Target:** Microsoft Excel (Microsoft 365)

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
| L2 | `pdf` | ~1k | PDF 1.7 writer (standard fonts, permitted TrueType embedding, HarfRust shaping and Unicode extraction, vectors, images) for print and export; independent of egui |
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
6. **PDF and Print**: `engine/cmd/print.rs` sends workbook text to the L0 `pdf` crate, which shapes
   Thai with HarfRust and embeds permitted TrueType faces. Native exports scan installed fonts and
   use the optional build-time `CRAFT_FONTS_DIR/fonts/manifest.txt`; Web/WASM accepts explicit
   `pdfFonts` bytes in the command and embeds no PDF font files at build time. Latin text embeds
   only the requested embeddable family and exact style; otherwise it uses a Standard-14 alias.
   The optional native PDF bundle includes TrueType outlines for Latin, Greek, Cyrillic, Thai
   and Georgian scripts; it excludes CJK and unsupported complex-script entries, even if they
   also cover Latin. At the pinned craft-fonts revision, this is two files (450,640 bytes), down
   from eight (41,874,180 bytes). System and explicitly supplied faces remain available.
   Wrapping omits separators at line breaks while retaining whitespace within lines and explicit
   newlines; over-wide words use exponential bracketing followed by binary search at grapheme
   boundaries, with at most 8,192 graphemes per layout line and 2,000 emitted lines. A missing script face degrades to the built-in
   CID fallback while preserving logical Unicode extraction. Invalid supplied font data returns an
   export error. CI pins craft-fonts with Noto Sans Thai and sets `GRIDCRAFT_REQUIRE_THAI_FONT=1`,
   so missing Thai coverage fails rather than silently skipping shaping tests. Tests accept a
   nearest-style Thai face when the requested bold/italic face is unavailable. A bounded catalog
   cache retains scalar coverage across lookup/font-byte eviction; only individually proven
   missing scalars bypass further font scans, and adding a supplied font invalidates that cache.
   `pdfFonts` input limits are 48 MiB encoded per entry, 32 MiB decoded per font and 64 MiB
   decoded in total, with a distinct error for each limit.
   Supplied faces prefer the typographic family name over a legacy family name. Chart labels,
   shapes, headers and footers consult the same catalog-backed text path for non-WinAnsi text;
   WinAnsi text retains Standard-14 metrics. This does not add bidi or Arabic/Devanagari support.
   Very wide wrapped lines with many short words can still require repeated complete-run
   shaping within the layout limits; adding independently shaped word widths would change
   contextual shaping and wrap boundaries.
   Unspaced text uses line-sized prefix probes instead of repeatedly shaping the remaining
   paragraph. A missing Thai face is tested with an empty catalog: the built-in CID fallback
   preserves `/ActualText` and needs no embedded-font `/ToUnicode` map.

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
  history limit is applied when the outermost command finishes; until then the steps made inside
  it are bounded on their own.
- **Recording buffer** (`record.rs`). `Session::start_recording` collects each successful
  top-level command with the document, sheet and selection it ran in, without secret parameters.
  Commands run by other commands are not recorded, nor is a menu-style call that only opened a
  dialog (the dialog's confirming call is the step), and the buffer is independent of the journal
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
  10,000 of them (`stash::MAX_ITEMS`). The older single-name `_sc_<key>` items are read as well.
  `formulas.defineName` and `formulas.deleteName` refuse a name the workbook holds such an item
  under (a hidden workbook-scope name of either shape; a visible one is the person's own), and
  `formulas.createFromSelection` skips such labels, unless `force: true` is passed.

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
- No complete string catalog or general complex-script layout: Arabic/Devanagari shaping and bidi
  remain unsupported; Thai shaping currently applies to PDF export only (see
  [localization-parity.md](localization-parity.md)).
- XLSX reader builds the whole workbook in memory; no streaming (#175).
- Unmodelled XLSX parts are not carried through a save.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-24 | minor | Filter the native PDF font bundle, retain exact Latin families/styles or Standard-14, and bracket unspaced text before binary search; test missing-font CMap behavior |
| 2026-10-24 | minor | Cache proven missing scalars independently of loaded faces; test nearest-style Thai fallback, font size limits, typographic family names and Unicode drawing paths |
| 2026-10-24 | minor | Adapt jpc7k1's word-wrap regression tests while retaining whitespace and bounded search; exercise a pinned Thai face in CI and omit desktop PDF fonts on WASM |
| 2026-10-24 | minor | PDF fallback preserves Standard-14 Latin, wraps at words, degrades when a script face is unavailable and loads optional fonts from the manifest |
| 2026-10-24 | minor | Engine building blocks for multi-command actions: undo groups, recording buffer, document trust, chunked hidden-name storage |
| 2026-10-23 | minor | PDF export embeds permitted TrueType faces and preserves Unicode extraction without coupling the PDF crate to egui |
| 2026-10-14 | minor | Document Undo's retained-cell-storage budget and latest-step guarantee |
| 2026-10-10 | major | Created from the code on main: crates, layers, data flow, agent control, known gaps |
