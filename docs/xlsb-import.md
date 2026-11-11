# Importing Excel binary workbooks

Open an `.xlsb` file with File → Open, drag it onto GridCraft, or pass it to the CLI.
GridCraft imports worksheet data into a new workbook and shows the conversion limits.

The import reads worksheet names, visibility, numbers, text, booleans, errors, merged cells
and cell formulas. A formula keeps its last saved result until it recalculates, and reads as
the same formula would from an `.xlsx` file saved by Excel: legacy formulas get Excel's
implicit `@`, and newer functions such as `CONCAT`, `XLOOKUP` and `LET` are read by name.
Shared formulas (filled down or across) become a formula in each cell; Ctrl+Shift+Enter arrays
keep their range, and dynamic arrays spill again when they recalculate.

Some formulas are not read yet and keep their **last saved results** as constants, with a
warning saying why: formulas using defined names, table references or other workbooks, and
Python cells whose code is stored outside the cell. Those results may be out of date if the source
application did not recalculate before saving.

Source formatting is omitted: dates, times, percentages and currencies initially display as
their underlying numbers. The workbook's date system is retained. Charts, images, defined
names, macros and other workbook features are not imported. GridCraft does not write XLSB files.

Save opens Save As and suggests a new `.xlsx` file. The source XLSB file is not used as the
workbook's save destination; requests to save to `.xlsb` return an error.

```sh
gridcraft-cli info source.xlsb
gridcraft-cli convert source.xlsb imported.xlsx
```

CLI import warnings go to stderr, leaving structured output on stdout available for scripts.
