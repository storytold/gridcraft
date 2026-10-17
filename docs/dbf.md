# dBASE table import

GridCraft opens classic dBASE III/IV `.dbf` tables through File > Open and `file.open`
(including byte/base64 calls and the web app). Each table becomes a worksheet: row 1 contains
field names in their original order, followed by records not marked as deleted.

This is data import, not database editing. The workbook has no DBF save target; Save asks for
a destination. Save As XLSX retains imported cell types. DBF export is not supported.

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

## Bounds and verification

The reader validates header/descriptor lengths, field widths, record lengths and deletion
flags before building cells. Truncated files return an error. Declared work is capped at
5 million field values (including headers and deleted records); decoded cell text at 64 MiB;
the resulting sheet must fit the worksheet row limit including its header. These bounds
apply to decoded work, rather than imposing a blanket limit on compressed/input file size.

Fixtures are constructed in Rust from original synthetic bytes. Tests cover typed values,
literal formulas, deleted/blank records, encodings, precision, dates, malformed/truncated
input, unsupported dialects and memo fields, engine imports and XLSX export. No private
or Excel-generated workbooks are committed. External Excel comparisons and a real-world
DBF corpus have not been performed.

References: [published table structure](https://www.vfphelp.com/help/html/465e7a94-51b7-4e0c-98f9-432864fe5bcc.htm),
[Excel's supported file formats](https://support.microsoft.com/en-gb/excel/file-formats-that-are-supported-in-excel).
