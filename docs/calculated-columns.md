# Calculated table columns

> **Last reviewed:** 2026-10-17 · **Last updated:** 2026-10-17

This implements only the calculated-column portion of #934, through `cell.set` and the shared cell-entry UI. It does not complete AutoCorrect, grow tables automatically, or change Power Query refresh.

- Enter a formula in an otherwise empty table data column to fill above and below the entry, within the data range.
- Edit a consistent calculated column to update every row and its stored master formula in one undoable transaction.
- Existing constants, blanks and differing formulas are exceptions. In a column with exceptions, a formula edit affects only the entered cell and keeps the stored master formula.
- Headers and totals are excluded. Existing cell formats are retained. Protection errors roll back the entire operation.
- File > Options > Fill formulas in tables to create calculated columns controls creation and updates. The engine command is `options.fillTableFormulas {"on":false}`; no argument toggles it. The preference is separate from workbook data and defaults on for old settings files.
- Imports, paste and multi-cell writes do not trigger this single-cell entry behavior. No new rows or columns are added by this feature.

The master formula is anchored at the first data row using the existing table-column model and XLSX reader/writer. Structured and row-invariant formulas share one parsed allocation. Relative A1 references are shifted with the existing formula adjuster. A 64 MiB budget bounds *newly generated, distinct formula text*, not table size or input file size; shared formulas do not incur a per-row text budget. Exceeding it returns an error without partial changes.

## Behavior references

- [Microsoft: Use calculated columns in an Excel table](https://support.microsoft.com/en-gb/excel/use-calculated-columns-in-an-excel-table)
- [Microsoft: Detect formula errors in Excel](https://support.microsoft.com/en-us/excel/detect-formula-errors-in-excel), especially inconsistent calculated-column formulas and exceptions.

Automated regression coverage includes middle-row creation, A1 and structured references, consistent updates, exceptions, option persistence, protection rollback, undo/redo, XLSX round trip, dependency recalculation and 10,000-row sharing. Test results and any measured timings belong in the PR validation section after the checks complete. A manual Excel check confirmed that a formula edit updates a consistent column, entering 999 creates a single-cell exception, and a later formula edit changes only its own cell. Excel flags the exceptions as inconsistent formulas; this change does not add that warning UI. Master formulas also follow header/table renames and structural reference edits.
