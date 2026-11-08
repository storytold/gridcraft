# Decisions

## Office Scripts run on Bun, which is not shipped

Office Scripts are TypeScript, so GridCraft needs a TypeScript runtime that is not V8-in-Office.
Bun (MIT) is the runtime: it executes TypeScript directly, starts in milliseconds, and its MIT
licence is compatible with GridCraft's `MIT OR Apache-2.0`. It is treated like model weights,
not like a dependency: runtimes ship in the allowlist sense, but Bun itself is user-downloaded or
placed by hand, never committed, packaged or fetched without the user asking. Discovery is
`$GRIDCRAFT_BUN`, then `PATH`, then `<model root>/runtimes/bun`. A missing Bun disables Office
Scripts with the path it looked at and a download link; nothing else in the app changes. The web
build has no child processes and says so.

The alternative, embedding a JavaScript engine as a Rust dependency, was rejected: QuickJS-class
engines do not run TypeScript without a second transpiler, and a V8 binding would be a large
unreviewed native dependency for one feature.

The shim that defines the `ExcelScript` globals is a small TypeScript string embedded in
`gridcraft-script-bun` (`src/shim.rs`). It is runtime plumbing, not application logic: it makes no
decisions about a workbook and forwards every operation to the engine. The repository's
"Rust only" rule is about the application; the maintainer decision for #3527 allows this one
shim and Bun.

## The ExcelScript bridge is JSON lines over stdio

The runtime is a child process. The parent sends one JSON object, `{"script": "...", "limits":
{...}}`, on stdin; the shim then writes a request line for every workbook operation it needs
(`{"id": 7, "method": "Range.setValue", "params": {"range": "A1", "value": 42}}`) and blocks
reading one reply line. The engine turns the request into a normal command (`cell.set`,
`range.setValues`, `home.bold`, …) through `Session::execute`, so a script gets the same
journaling, protection, arrays, spills and undo as the ribbon.

Rejected alternatives: a file-based protocol (partial writes and cleanup), a socket (a second
listen path to secure, and wasm has none), and calling back through the control channel (a
loopback client inside the app, with its own authentication and timeout). Stdin/stdout is
synchronous, needs no ports or tokens, and its ordering is exactly the call order.

Bounds live on both sides: 100,000 characters of code, 10,000 calls, 30 s by default
(`timeoutMs` up to 600 s), 1 MiB of console output, 8 MiB per line and 16 MiB of runtime output.
The parent kills the child at the timeout; malformed lines and broken pipes are errors, not
panics. A run is an undo group: failure or timeout rolls back to the workbook and history as they
were before Run.

## Office Scripts are not Office Add-ins

Office Add-ins are web pages hosted by Office, using `Office.js` and `Excel.run`; they need a
manifest, an origin and a task pane, and they do not exist outside Office. The issue asks for
Office Scripts, whose model is a `main(workbook: ExcelScript.Workbook)` function over the
ExcelScript object model. GridCraft implements that subset and deliberately does not implement
`Excel.run`. Nothing here loads a web page or a foreign manifest.

## The Office Scripts store is a local folder of .osts files

GridCraft is offline-first: the script store is a folder on the local disk, by default
`<config>/scripts`, and `script.folder` can point it at any local folder, such as a synced
`OneDrive/Documents/Office Scripts` or a SharePoint-synced library. Excel requires scripts to be in
OneDrive/SharePoint (or associated with a workbook); pointing both at the same synced folder is
the interop path. No network call is made to list, save or run a script: GridCraft itself needs no
connection, but a script is native code and can reach the network if it tries (see the trust model
in [office-scripts.md](office-scripts.md)).

The transfer format is Office Scripts' `.osts` JSON (version 0.3.0): `body` with CRLF,
`description`, `noCodeMetadata`, `parameterInfo` and `apiInfo` as JSON strings, and unknown fields
kept. GridCraft's own record metadata is stored beside the body; it is stripped from an exported
file. Excel's script code is not part of the `.xlsx`: a workbook may carry an association and a
script button, which are workbook-side metadata and are left untouched by the xlsx pass-through.
GridCraft does not invent an in-workbook script format.

## The coverage report is generated, not claimed

`docs/office-scripts-coverage.md` comes from two checked-in lists: the ExcelScript names extracted
mechanically from Microsoft's public reference index (names only) and the members GridCraft
implements. `cargo xtask office-scripts-coverage` writes it, `--check` and a crate test fail when
it is stale, and the xtask step runs in `cargo xtask ci`. Enum values are `not-applicable`; every
other unimplemented member of the object-model classes is `not-yet` and returns a typed
`notSupported` error naming it. The plain data objects the bridge returns (`DataValidationRule`,
for one) are data, not classes: a member they do not carry reads as `undefined`. This keeps the
claim honest as the object model grows.

## Scripts live beside the settings, never in the workbook

A script is `<config>/scripts/<id>.json`; an id is `[a-z0-9-]`, made from the name, so a name
cannot address another file. Unlike the older `automate.*` scripts and macros, which are workbook
content and therefore travel with a file, Office Scripts belong to the application: opening a
workbook never brings code, so there is no trust prompt for them. Code is checked for balanced
strings, comments and brackets before it is saved or run; the runtime reports real grammar errors
with their line.

## Bound repeated XLSX column expansion (#1022)

Limit each worksheet to 1,000,000 nondefault column assignments across all `<cols>` blocks.
This allows more than 60 full-grid passes while bounding repeated work on overlapping spans;
counting only the final map size would not limit that work. Charge each clipped span before
inserting it, after the existing default-record skip. Exceeding the limit returns the existing
`IoError::TooLarge`, which aborts the import instead of silently dropping a partially read sheet.
Keep the current last-wins semantics rather than introduce interval storage or normalization.

## Formula localization uses the existing interface preference

PR #98 builds on the persisted `UiState.language` introduced by #31 and extended by #104.
Español selects the Spanish function table and list/decimal separators; there is no competing
locale setting. Formula syntax without a supplied translation table falls back to English.
The editor translates to canonical English before engine commands, and translates back only
for display. API, clipboard and XLSX syntax remain stable. Existing names override aliases.

The revision is based directly on main after #31/#104 and Portuguese interface support (#82)
merged. Those locales retain their existing formula fallback; this change adds only Spanish.

## Preserve array evaluation when exporting formulas (#64)

GridCraft evaluates ordinary formulas with array semantics even when the final result occupies one cell. XLSX export marks actual spills and formulas whose results may differ under legacy implicit intersection as dynamic, using the spill range or a single-cell array reference. Legacy CSE array ranges keep their existing representation.

The exporter inspects the parsed formula without evaluating it. It distinguishes range-taking functions such as `SUM` from scalar arguments and operators: `SUM(A1:A3)` and `A1*2` stay ordinary, while `SUM(A1:A3*B1:B3)` and `SQRT(A1:A3)` retain array evaluation. Legacy array-aware contexts such as `SUMPRODUCT` do not need an extra marker. Explicit `@` scalarizes its result without hiding array-sensitive operations inside it.

Names are resolved using workbook/sheet scope, and structured references distinguish one table-row cell from a column. Literal `INDIRECT` and `INDEX` arguments permit tighter classification; unresolved reference dimensions are conservatively treated as potentially multiple cells. Name/AST traversal is bounded, with array preservation as the fallback. This is an export classifier, not a claim of exact Excel metadata parity for every function or user-defined function. It stays within the format crate's existing dependencies and does not change calculation or import behavior.

This is a writer policy; interpreting implicit intersection in imported legacy formulas remains separate work. See Microsoft's [Formula vs. Formula2 documentation](https://learn.microsoft.com/en-us/office/vba/excel/concepts/cells-and-ranges/range-formula-vs-formula2).

## Static cell pictures

Store immutable picture payloads in `Sheet::cell_pictures`, a sparse map keyed by `CellRef`,
shared through `Arc`. Ordinary cells keep their original size. Content replacement removes the
map entry; copy, fill, sort and structural edits explicitly transport it, and undo shares bytes.
Serialize the map as address/payload pairs so JSON does not require struct-valued object keys.
Keep the scalar `#VALUE!` fallback used by XLSX consumers that cannot interpret rich images.
This first implementation does not extend formula evaluation to rich values or implement
`IMAGE()`; references and text-only exports use the fallback.

Write picture value metadata and existing dynamic-array cell metadata into the same workbook
metadata part, with independent index tables. Resolve input parts through their relationships.
Support embedded PNG/JPEG files without fetching external resources. See
[cell pictures](cell-pictures.md) for the supported user workflow and current limits.

Picture metadata is optional during XLSX import: part-size limits, record limits and relationship
read errors produce warnings and retain scalar cell fallbacks. The generic package size limits
remain unchanged. Rebased on main before revision; ODS/XLSB import PRs are still unmerged, and
`Package::read_limited` uses the same implementation as the incoming XLSB PR #101 to avoid
duplicate helpers when that work lands.

The UI cache uses a 64 MiB texture budget with at most 4,096 sparse entries. Images used in the
current or preceding frame are protected from eviction. If the visible set exceeds the budget,
new images show the existing placeholder until capacity is available; header-based admission
avoids repeatedly decoding overflow images. More than 128 small visible pictures now keep stable
texture handles instead of cycling through the cache every frame.

## Ctrl+wheel zoom steps along its own ladder, not Excel's 15-point steps

A Ctrl+wheel notch moves the zoom to the next step on `grid::WHEEL_ZOOM_LADDER`: 10, 12, 15, 18,
21, 25, 30, 35, 42, 50, 60, 70, 85, 100, 120, 140, 170, 200, 240, 280, 340, 400 (four steps per
doubling, 17–25% apart). A step less than 5% away is skipped, except 100% and the 10% / 400%
limits, which are never skipped: 121% out goes to 100%, and 99% or 101% towards 100% goes to 100%.
The wheel's turning is counted in notches (egui eases each notch in over several frames, and
fine-grained wheels send parts of one); a notch steps once 60% of it has turned. With animations
on, the zoom then glides to the step over about a tenth of a second (as egui eases a scroll notch),
and a notch turned during a glide steps on from the step it's going to. With View › Animations off
it goes to the step at once. Whether the zoom glide should have a setting of its own is left to the
maintainers.

Before, each frame multiplied the whole-number zoom by egui's eased factor and rounded, so a notch
out never undid a notch in (100 → 122 → 101 → 123 → 102). Keeping the zoom unrounded fixed that
for a wheel with exact notches, but not for one that sends uneven parts of a notch.

Excel (observed: Microsoft 365, Windows) steps 15 points a notch: 100 → 115 → 130, and from 112%
to 115% or 100%. That's a large relative step at low zoom and a small one near 400%; equal
relative steps keep each notch feeling the same, and the doublings land on 25, 50, 100, 200 and
400%. The status bar's Zoom In / Zoom Out buttons and Ctrl+Alt+= / Ctrl+Alt+- keep their 10%
steps, so the wheel and the buttons now step differently. Whether they should share one ladder
(this one, Excel's, or another) is left to the maintainers.

## Equations are laid out by RaTeX, behind a guard

GridCraft lays equations out with RaTeX (`ratex-parser`, `ratex-layout`, `ratex-types`,
`ratex-font`, MIT), pinned exactly at 0.1.14. The gate that chose it, rather than an in-house
LaTeX subset:

- **Source scan.** The five crates hold about 31,700 lines. Outside tests and the bundled
  command-line tools there are 95 sites that can panic (`unwrap`, `expect`, `panic!`,
  `unreachable!`), 87 of them in the parser. Most take an argument the parser has already
  counted (`args.into_iter().next().unwrap()`) or compile a fixed regular expression. There is
  no `unsafe`, no clock (`Instant::now`, `SystemTime`), no threads, and no file or environment
  access. Indexing is not counted; the fuzz run covers it.
- **Fuzzing.** 70,000 random token strings and 50,000 mutations of valid equations: no panic,
  no input slower than 5 ms. Self-referencing and exponential macros stop at the parser's
  1,000-expansion limit, which counts expansions and not their size: a definition whose
  argument doubles on every call never reaches it. The wrapper therefore refuses macro
  definitions (`\def`, `\newcommand`, `\let` and the like) before RaTeX sees the source, and
  caps the padded size of matrices at 2,500 cells. Nesting deeper than 32 is refused ("Recursion limit exceeded"), and
  the deepest accepted input needs under 256 KB of stack in a release build, well inside the
  smallest main-thread stack we run on (1 MB on Windows). Absurd sizes (`\rule{1e308em}{1em}`)
  lay out with huge dimensions, so the wrapper caps them.
- **wasm.** The crates build for `wasm32-unknown-unknown`.
- **Regular expressions.** `ratex-parser` pulls in `fancy-regex` (with `bit-set` 0.8 and a
  second `bit-vec`) for one module, its port of mhchem: 62 anchored patterns in
  `src/mhchem/data/patterns_regex.json` and a dozen in `src/mhchem/patterns.rs` tokenise the
  argument of `\ce{…}` and `\pu{…}`, and need lookahead, which the linear-time `regex` crate
  does not have (`regex` and `regex-lite` serve the parser's other, fixed patterns). Every
  pattern is compiled with `fancy-regex`'s default backtrack limit of 1,000,000 steps per
  match; past it the match is reported as no match, so the macro fails instead of hanging.
  The limit is not reachable from outside the crate. `gate.rs` times 38 sources of exactly
  4,096 bytes shaped to stress the parser and those patterns (nesting at the depth cap, runs of
  backslashes and braces, `(a+)+b`, wide and tall environments, chemistry such as `\ce{+-+-…}`,
  `\ce{111…}` and `\pu{kJ//…}`): the slowest takes 0.8 s in a debug build, with a 10 s bound
  per source.
- **Syntax.** Fractions, roots, scripts, big operators and limits, `\left`/`\middle`/`\right`,
  matrices (`matrix`, `pmatrix`, `bmatrix`, `vmatrix`, `cases`, `aligned`, `array`), accents,
  braces, arrows, `\mathbb`/`\mathcal`/`\mathfrak`, `\text`, colours and mhchem all lay out.

Because a stray panic is still possible, RaTeX runs only inside `gridcraft-math`, under
`catch_unwind`, with input capped at 4 KB and depth 32. Its output is converted to plain data and
cached on the shape; painting (egui and PDF) reads only that cache, so nothing in a paint path
can reach RaTeX. On the web build (`wasm32-unknown-unknown`) a panic aborts instead of unwinding,
so there the caps and the fuzzed input space are the protection. `crates/math/tests/gate.rs` re-runs a smaller, deterministic version of the
gate on every test run, so a version bump has to pass it. RaTeX positions glyphs with KaTeX
font metrics; GridCraft never bundles the KaTeX fonts, so glyphs are drawn with system fonts
and a glyph no font has is drawn as a `[?]` box.
