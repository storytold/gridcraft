# Import bordered PDF tables

Data > From PDF opens a native file picker. Select a page and table, inspect the
preview, choose column types, then Load. A new worksheet is created; existing
sheets are never overwritten. Format as table creates an ordinary GridCraft table,
with an optional first header row. Blank/duplicate source headers are refused rather than silently renamed; disable table formatting or add generated headers to preserve them. The whole import is one undo step.

## Supported input

This first implementation detects complete rectangular grids made of horizontal
and vertical vector lines, containing selectable PDF text. It preserves empty
cells and multiline text. Separate tables on a page are listed in reading order.
Only the selected page is interpreted; page rotation and cropping use craft-pdf's
page transform. Interpretation runs on a cancelable desktop worker, not the UI
thread. No Python installation, remote service, OCR or page rasterization is used.

Scanned images, borderless tables, merged/incomplete grids, rotated table text,
nonrectangular clipping, replacement-text spans, masked/knockout content and
unsupported interpreter diagnostics are outside the first scope. Images that
intersect a detected table are refused rather than silently omitted. A page with
no supported grid returns an empty table list. No partial table is loaded after an
error, cancellation or exhausted budget. The web build remains supported, but its
file-picker flow does not yet support PDF table import.

## Preserve data deliberately

Every column starts as Text. Leading zeros, apostrophes and strings beginning with
`=` are data, not formulas. Choose Number explicitly for plain finite decimal
numbers; blank cells stay blank and the selected header row stays text. Ambiguous
localized numbers, dates and currency strings should remain Text. A conversion
error identifies its row and column and leaves the workbook unchanged.

The preview shows up to 20 rows and 20 columns. Long cells are shortened for preview only. Column type controls can address
all columns, and Load imports the complete selected table. Cancel discards the
preview without creating a sheet. A preview belongs to its original workbook
window and cannot silently load into another workbook.

## Automation

- `data.pdfTables {path, page: 1}`: inspect a one-based page, returning `page`,
  `pages` and `tables` (bounds plus text rows), without changing a workbook.
- `data.fromPdf {path, page: 1, table: 1, sheet?, asTable: true, headers: true,
  numericColumns: []}`: import a selected table into a fresh sheet. Page and table
  numbers are one-based; numeric column indexes are zero-based.
- `data.fromPdf {rows: [["Code", "Amount"], ["00123", "12.50"]],
  numericColumns: [1]}`: load an already confirmed text preview without rereading
  a file. It uses the same dimension/type validation and never evaluates formulas.

Missing inputs return errors, never dialogs. Macro file-read permissions apply
when a path is supplied. Imported data receives file-origin trust.

## Bounds

The shared reader's 512 MiB input limit applies before reading. Each page has
budgets of 10 seconds on desktop, 200,000 glyphs, 16,384 line segments, 8 MiB of decoded text
and 2,000,000 geometry operations. A preview/import contains at most 100,000 cells
and must fit worksheet dimensions. Cell text must fit the XLSX limit of 32,767 UTF-16 code units. Budget failures are explicit; they are not a
claim that every file below those limits can be interpreted.

Tests construct original PDFs in Rust rather than committing third-party samples.
They cover extraction, empty/multiline cells, literal preservation, explicit number
conversion, invalid input, cancellation, geometry budgets, one-step undo and XLSX
round-trip. UI tests exercise preview loading, cancellation and keyboard isolation.
