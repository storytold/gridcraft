# Moving or copying worksheets between workbooks

Open both workbooks, right-click the source sheet tab and choose **Move or Copy**.
Choose the destination under **To book**, select the sheet to insert before (or
**(move to end)**), and select **Create a copy** to leave the original in place.
**(new book)** creates an unsaved workbook containing the transferred sheet.
A conflicting sheet name receives a suffix such as `Report (2)`.

This first implementation transfers self-contained sheets. It keeps cell values,
formulas and array ranges; cell, row and column styles; row/column geometry; notes,
hyperlinks, merges, views, print settings, ordinary tables, charts, pictures, shapes,
sparklines, conditional formats and data validation. Sheet-local names and the
workbook names used by formulas are carried across. Styles are interned into the
destination, including the source Normal formatting of implicit empty cells.
Table identifiers are allocated in the destination workbook.

## Safety limits

The command reports an error without changing either workbook when it cannot
preserve a transfer safely. In particular:

- References to sheets or tables left behind need workbook links, which are not
  implemented here. This includes references in names, charts and validation rules.
- A move is refused if remaining source formulas or names depend on the moved sheet.
  A copy can still work because the original remains available.
- INDIRECT references cannot be checked and are refused.
- Conflicting workbook names or table names require resolving the conflict first.
- The workbooks must use the same theme and date system. A new workbook inherits
  these from the source.
- Workbook structure protection is respected, and a move cannot remove the source
  workbook's last visible worksheet.
- VBA, retained package content, XML/connection-bound tables, PivotTables,
  PivotCharts and slicers require further integration and are refused.
- Relative file hyperlinks are refused because their base folder would change.

These restrictions are explicit; formulas are never silently converted into values,
unsupported objects are never discarded, and references are not rebound to an
unrelated sheet or name with the same spelling in the destination.

## Undo and redo

A copy changes only the destination. A move changes both workbooks and has a linked
history entry: Undo or Redo from either workbook restores both sides together. If
the other workbook has newer edits, undo those first. If a participant has closed,
the linked operation is refused rather than partially restoring or deleting data.
Undo of a transfer to a new workbook leaves an empty unsaved workbook open.
Files are not saved automatically.

## Commands

`sheet.move` retains its existing within-workbook parameters. To transfer to another
open workbook, add `targetWorkbook` with that document's stable `uid`; use the string
`"new"` for a new workbook. `to` is the zero-based insertion position in the destination
(`sheets.len()` means append). `copy` defaults to false. `sourceWorkbook`, when supplied,
must match the active document uid and guards against a stale dialog.

Example: `sheet.move {"sheet":"Report","to":0,"copy":true,"targetWorkbook":42}`.
Call `view.switchWindows {}` to discover the open documents and their `uid` values.
Document ids are session identities, not filenames or indices.

## Reference and verification

The interaction follows Microsoft's public documentation:
https://support.microsoft.com/en-gb/excel/get-started/move-or-copy-worksheets-or-worksheet-data

Synthetic regression tests cover destination selection, stale destinations, local
formula rewriting, names, styles and implicit formatting, new workbooks, refusals,
and linked undo/redo. UI tests exercise the destination picker in light and dark
mode and cancellation. Broader cross-workbook linking is a separate feature.
