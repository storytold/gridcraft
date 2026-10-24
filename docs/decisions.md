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
## CSV: Excel's `sep=` first line (#1446)

A CSV file whose first line is `sep=` plus one character is split at that character when File ›
Open detects the delimiter, and the line isn't data. Observed in Excel 16.0 (build 17932, Windows)
opening synthetic files: spaces anywhere in the line are ignored (`sep= |`, ` sep=|  `), case
doesn't matter, the line may be quoted as a whole field, and a BOM before it is fine. `sep=` with
nothing after it (or only spaces) is dropped and nothing is split; a non-ASCII character is dropped
the same way. Two characters after `=` (a tab counts, `sep=|<tab>`), or the line anywhere but first,
is data. `sep="` splits at the quotes, so nothing is quoted. (#1446 measured `sep=` alone as data in
Excel for Microsoft 365, de-DE; this build drops it.)

A delimiter given by the caller wins and the line stays data, so a `.tsv`/`.txt` file (read with
tabs) keeps it. Saving: Excel writes its list separator and drops the line. GridCraft saves a CSV
with the separator it was read with, so it keeps a named `,` `;` or tab only when detection picks
it again on reopening; otherwise (`|`, or `sep=;` over text with as many commas) it saves commas,
which reopen in the same columns. A first row that would itself read as a `sep=` line (a lone
`sep=;` cell) is written behind a `sep=` line naming the delimiter, so it stays data. Excel writes
such a row bare (`"sep=;";`) and reopens the file garbled (observed: A1 `sep=;;`, row 2 in one cell).

From Text/CSV still guesses its own delimiter; making it detect like File › Open (and so honour
`sep=`) is another part of #1446.
