# Decisions

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

## Automatic Data Conversion: off means nothing is cut short or turned scientific

File › Options › Data copies Excel's Automatic Data Conversion list (enable all, remove leading
zeros, keep first 15 digits of long numbers, convert digits around "E"; Excel's fourth option,
continuous letters and numbers to a date, stops a conversion GridCraft never makes, so its place
holds a broader date option, below). The options apply to typing, pasting and opening CSV/text
files, as in Excel, and are an application preference (`Prefs::data_conversion`,
`app.dataConversion`), not workbook state. `app.dataConversion` is journaled but not undoable: it
changes what later typing gives, so a replayed journal needs it, and Undo takes back edits, not
settings.
Data validation reads typed input (typing, `data.validate`) under the same options, so a rule
checks what the cell would hold: with leading zeros kept, `0123` is text and a whole-number rule
refuses it. Circle Invalid Data checks what each cell holds, whatever the options say now, so
`0123` stored as text stays circled after the option is back on (formula cells are still judged
by their formula text, as before the options).
All default on, so nothing changes until someone opts out.

One deliberate difference: with "Keep first 15 digits of long numbers and display in scientific
notation" unchecked, Excel still converts 12–15 digit strings, which General format then shows as
`1.24412E+11`, and a CSV saved from Excel writes that text, so the digits are gone. In GridCraft,
unchecked means no number is cut short or shown in scientific notation: a number converts only if
General shows every digit of it as written (at most 11 characters, not below 1E-4); anything else
stays text exactly as written. A digit limit (default 11, up to 15) lets someone allow scientific
display for longer numbers; even at 15 no digit is lost. Core restates General's rule
(`general_shows_exactly`) because core can't depend on numfmt; a numfmt test checks the two agree.
The case that prompted it: CAD/CAM cut lists whose 12-character part codes are hex, all digits
(`124412140710`) or digits-E-digits (`1244121406E6`).

GridCraft adds one item to Excel's list, next to its counterpart "Remove leading zeros": *Remove
trailing zeros after the decimal separator*, on by default (today's behaviour). Off, a plain
number written with trailing decimal zeros (`720.0`, `3.10`, `.50`) gets a number format that shows
them (`0.0`, `0.00`, `#.00`), as `1,234.50` already gets `#,##0.00`. It stays a number; the grid
shows it as written; and since CSV export writes displayed values, the saved file has `720.0` again.
It is one of the conversions "Enable all" sets, so unchecking that keeps everything as written. As
with every automatic format, a cell that already has a format keeps it.

In the place of Excel's fourth option, GridCraft has *Convert text that looks like a date or time
to a date*, on by default (today's behaviour). Excel's box only keeps runs like `SEPT2` or
`12JAN2026` as text, which GridCraft never converts; Excel's other date and time conversions can't
be switched off, and GridCraft's couldn't either, so `2.6.70` (a version or part number) opened as
6 February 1970 and saved back as
`2/6/1970` (#506). Off, every date and time (`3/4/2026`, `2.6.70`, `4-Mar-2026`, `1/2`, `10:30`) stays
text, as written, when typed, pasted or opened; numbers and fractions (`1 1/2`) still convert, and
formulas still read date text (`=DATEVALUE("3/4/2026")`), as they read `0123` whatever the options
say. It is one of the conversions "Enable all" sets. A Text Import Wizard with a type per column
would be the finer tool; this is the switch for files that must come back as they were.

Paste also reads the clipboard's HTML, where a spreadsheet records the number behind a cell
(`x:num`, `sdval`; #257). That number doesn't override the options: a field they keep as text
(`0042`, `990000000000000019`, `2.6.70`) stays text even when the source recorded it as a number
(a date as its serial number), which for a long code may already have lost digits. The HTML still supplies the number for text the options
don't cover, like pasted currency (`Rp10,000.00`). JSON numbers and booleans are different, in
`range.setValues`, in `cell.set` and in the values a scenario stores: they're values, not text, so
the options don't apply to them.

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
  1,000-expansion limit. Nesting deeper than 32 is refused ("Recursion limit exceeded"), and
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
