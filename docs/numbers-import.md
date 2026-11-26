# Importing Apple Numbers documents

Open a `.numbers` file with File → Open, drag it onto GridCraft, or pass it to the CLI. GridCraft
imports each table into its own worksheet of a new workbook and shows a warning describing the
conversion limits. A sheet with one table keeps the sheet's name; a sheet with several tables
names each worksheet `Sheet - Table`.

The import reads numbers, currency, text (including rich text, as plain text), booleans, dates,
times and durations. Formula cells become their **last saved results**, not live formulas. These
results may be out of date if the document was not recalculated before saving. Dates and times
are given a date format, and durations a `[h]:mm:ss` format.

This is data import, not full Numbers compatibility. Source formatting, formulas, merged cells,
charts, images, categories, form controls and other document features are not preserved.
GridCraft does not write Numbers files. Documents saved by very old Numbers versions (before the
`.iwa` format, in 2013) and password-protected documents are not read.

Save opens Save As and suggests a new `.xlsx` file. The source Numbers file is not used as the
workbook's save destination; requests to save to `.numbers` return an error.

```sh
gridcraft-cli info source.numbers
gridcraft-cli convert source.numbers imported.xlsx
```

CLI import warnings go to stderr, leaving structured output on stdout available for scripts.

## How it is read

A `.numbers` file is a ZIP package. Its `Index/*.iwa` parts are chunks of Snappy-compressed
Protocol Buffers messages. The reader decompresses them with its own bounded Snappy decoder,
follows the document → sheet → table → tile references and reads the packed cell storage. Only
the fields the import needs are read, by number, and the rest are skipped. The reader is written
from the public Snappy and Protocol Buffers format descriptions and from observing documents,
never from Apple's code. Hostile input is bounded: chunk and total decompressed sizes, object and
cell counts and the worksheet grid are all capped.
