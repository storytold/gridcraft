# dBASE table import and export

GridCraft opens classic dBASE III/IV `.dbf` tables through File > Open and `file.open`
(including byte/base64 calls and the web app). Each table becomes a worksheet: row 1 contains
field names in their original order, followed by records not marked as deleted.

This is data import, not database editing. The workbook has no DBF save target; Save asks for
a destination. Save As XLSX retains imported cell types. Save As DBF creates a fresh data table;
it does not replace the workbook's save path or clear its unsaved changes.

## Supported data

- Classic 32-byte field descriptors, version markers `03`, `83` and `8B`.
- Character fields remain literal text, including leading spaces and text beginning with `=`.
  Trailing padding spaces are removed. Blank values become empty cells.
- Numeric and floating fields become finite double-precision numbers. Values with more than
  15 significant digits are kept as text, with a warning, to avoid rounding identifiers/decimals.
- Logical fields become booleans; blank/unknown logical values become empty cells.
- Valid dates from 1900 through 9999 become date-formatted serial values. Blank/zero dates
  become empty cells; invalid or earlier dates are kept as text, with a warning.
- OEM 437, 850, 852, 866, 865 and 861; Windows 1250, 1251, 1252, 1253 and 1254.
  A missing code-page marker defaults to Windows-1252 with a warning. Unsupported markers
  and invalid encoded text are errors. No locale or UTF-8 guessing is applied.

Memo fields require companion `.dbt` files and are currently refused, as are binary fields,
encrypted/incomplete tables, dBASE II/7 and Visual FoxPro-specific layouts. Index files and
other companions are never opened. This imports attribute data, not GIS geometry.

## Save As DBF

Unlike Excel, GridCraft can export an active sheet as a fresh dBASE III table. Choose
**File > Save As > dBASE III table**. The native file picker and web download both support it.
Programmatic callers use `file.exportDbf {path, sheet?}`, `file.saveAs {path}` or
`file.saveBytes {format: "dbf"}`.

The first used row supplies unique, case-insensitive field names: at most 10 ASCII letters,
digits or underscores, starting with a letter or underscore. Each remaining column must hold
one type, allowing blanks. Export uses Windows-1252 with checked encoding; text is limited to
254 bytes, numeric fields to 20 bytes and 15 decimal places, columns to 255 and records to
4,000 bytes. These are compatibility limits, not silent truncation rules. Dates use the
workbook's date system, must be real calendar dates from 1900 through 9999, and cannot include
times or use conditional date formats. Formula results are exported as values; errors, mixed types, unrepresentable text,
NUL characters, significant trailing spaces and numbers that do not fit cause an error before
any output file is written. Empty strings and blank cells become blank fields.

Numeric widths and the common decimal scale are inferred from actual values; exact shortest
representations are stored without rounding or unnecessary trailing digits. The importer may
preserve numbers with more than 15 significant digits as text, as described above. All-empty
columns use a one-byte character field. Export shares the reader's 5-million-field-value and
64 MiB decoded-text limits (including numbers the reader preserves as text); output is capped
at 512 MiB. The writer retains column metadata, a bounded format cache and its output buffer, not a second copy
of the cell table.

This is a new data export, not lossless database round-tripping: original field widths,
deleted records, indexes, memo relationships and workbook features are not preserved. Use a
new filename when exporting data belonging to another application or a GIS dataset. A DBF
export leaves the workbook's original path and unsaved state intact, just like CSV export.

## Bounds and verification

The reader validates header/descriptor lengths, field widths, record lengths and deletion
flags before building cells. Truncated files return an error. Declared work is capped at
5 million field values (including headers and deleted records); decoded cell text at 64 MiB;
the resulting sheet must fit the worksheet row limit including its header. These bounds
apply to decoded work, rather than imposing a blanket limit on compressed/input file size.

Number-format interpretation stays in the engine; the byte writer receives a cached
date/number classification without depending on the number-format crate.

Fixtures are constructed in Rust from original synthetic bytes. Tests cover typed values,
literal formulas, deleted/blank records, encodings, precision, dates, malformed/truncated
input, unsupported dialects and memo fields, engine imports and XLSX export. No private
or Excel-generated workbooks are committed. Additional writer/engine/UI tests cover typed
round-trips, headers and bounds, rejection without changing files, formula results, desktop
Save As and browser downloads in light/dark themes.

Excel 16.0.20326.20232 x64 was observed with an original synthetic DBF: Ctrl+S after an edit
shows a format warning; OK opens Save As, defaulting to XLSX after choosing a local folder.
Cancelling preserves edits. Saving as XLSX preserves the original DBF byte-for-byte.
GridCraft uses its existing import/Save As flow rather than duplicating Excel's warning dialog.
A synthetic DBF generated through GridCraft's desktop Save As was also opened in that Excel
build: its Name/Amount fields and Ada/42 record loaded without a repair or format warning.
This is a small interoperability check, not a real-world DBF corpus run.

References: [published table structure](https://www.vfphelp.com/help/html/465e7a94-51b7-4e0c-98f9-432864fe5bcc.htm),
[Excel's supported file formats](https://support.microsoft.com/en-gb/excel/file-formats-that-are-supported-in-excel).
