# Parquet tables

Opening `.parquet` imports a flat table as a new workbook. The first row contains
field names, followed by values. File contents take precedence over the extension.
The imported file has no workbook save target, so ordinary Save cannot overwrite it.

Save As > Parquet table, Export as Parquet, `file.exportParquet {path, sheet?}`,
`file.saveBytes {format: "parquet"}` and CLI `convert` export the active/selected
sheet's used rectangle. Its first row must contain nonempty unique text headers.
Each data column must contain one type (number, Boolean or text); blanks become
nulls. Formula results are written, not formulas. Number formats, date annotations,
original integer widths, decimal types, metadata, extra sheets and workbook objects
are not preserved. Dates/times export as their numeric spreadsheet serials.

## Supported input

Flat optional/required numeric, Boolean and UTF-8 fields; dates, millisecond and
microsecond times/timestamps are displayed using spreadsheet date/time formats.
Timestamps are displayed as stored (UTC for UTC-adjusted fields), without applying
a local timezone. Decimals and integers beyond exact binary64 integer precision
are kept as text, with an import notice. Formula-looking strings remain text.
NaN/infinities, nested/repeated/list/map fields, binary values, unknown logical types,
nanosecond timestamps, encrypted files and external column files are refused.
Trailing all-null rows do not extend a worksheet's used range; an import notice
reports their omission. Entirely null columns retain their header.

Uncompressed, Snappy, Gzip, Brotli and LZ4_RAW are supported on native and web.
ZSTD input is supported on native builds; the web build reports an unsupported
codec for ZSTD. Output uses Snappy on both, in batches of up to 4,096 rows.
No Python, database, full Arrow stack or background worker is installed.

## Resource bounds

The header leaves room for 1,048,575 data rows; at most 16,384 columns and
5,000,000 rectangular cells can be imported/exported at once. The decoder
preflights the compact-Thrift footer and page headers: at most 16 MiB metadata,
32 envelope nesting levels, 1,000,000 pages, 64 MiB per page and 512 MiB decoded page data.
Text cells are capped at 1 MiB and export text at 512 MiB. These are expansion
budgets, not a blanket small-file-size limit. Unsupported/oversized files fail
with a message rather than being silently truncated. A smaller source table can
be prepared in a separate file; opening never modifies the source.

Normal file opens transfer their owned input allocation to the decoder without a
second full-file copy, on native and web. The lower-level borrowed `read` /
`open_bytes` APIs still copy because their callers retain ownership. Both routes
use the same preflight limits, decoder and value conversions. Input files still
reside in memory; this is not a streaming dataset browser. Large tables may exceed
worksheet limits or practical memory before their compressed size looks large.

For native profiling without printing cell contents, run
`cargo run -p gridcraft-parquet --example inspect -- file.parquet --owned`.
Omit `--owned` to compare the borrowed-input path. The timer measures import after
reading the file; process peak memory includes the input and resulting worksheet.

## Verification

Synthetic Rust tests cover types, nulls, Unicode, formula-like text, compression,
multiple row groups, precise integers, dates/times, invalid schemas, resource limits,
malformed/truncated files and file command/save-state behavior. Private datasets
are used only for local checks and never committed.

Specification: https://parquet.apache.org/docs/file-format/
Implementation dependency: https://github.com/apache/arrow-rs/tree/main/parquet
