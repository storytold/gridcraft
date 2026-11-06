# Importing OpenDocument spreadsheets

Open an `.ods` file with File → Open, drag it onto GridCraft, or pass it to the CLI. GridCraft
imports worksheet data into a new workbook and shows a warning describing the conversion limits.

The import reads sheets, numbers, text, booleans, supported dates/times, repeated cells/rows and
merged cells. Formula cells import as **live formulas**: the saved OpenFormula text is translated
to Excel syntax and the saved result becomes the cell's initial value, so formulas recalculate.
The translation covers the `of:=`, `oooc:=`, `msoxl:=` and bare `=` prefixes; `;` argument
separators, with string literals untouched; bracketed references (`[.A1]` becomes `A1`,
`[.$A$1:.B2]` becomes `$A$1:B2`, and a sheet's leading `$` is dropped); quoted sheet names
(`['Sheet 2'.A1]` becomes `'Sheet 2'!A1`); and `COM.MICROSOFT.X` function names, which become
`X`. Matrix formulas import as array formulas, and cells whose saved result is an error become
error values.

A formula that cannot be translated exactly (a reference to another file, the `~` and `!`
reference operators, error literals, `ORG.OPENOFFICE.*` or `com.sun.star.*` add-in functions,
column-only or row-only references, or anything that fails to parse) keeps its saved result as a
constant, and the import warns. Unsupported or missing values retain available display text and
produce a warning.

This is partial import, not full ODS compatibility. Source formatting, charts, images, names and
other workbook features are not preserved. GridCraft does not write ODS files.

Save opens Save As and suggests a new `.xlsx` file. The source ODS file is not used as the
workbook's save destination; requests to save to `.ods` return an error.

```sh
gridcraft-cli info source.ods
gridcraft-cli convert source.ods imported.xlsx
```

CLI import warnings go to stderr, leaving structured output on stdout available for scripts.
