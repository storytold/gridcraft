The workbook `/app/sales.xlsx` holds quarterly sales by region: a table with headers in `B4:I4`,
regions in `B5:B10`, quarters Q1–Q4 in columns `C:F`, and a total row in row 11.

Add an average column next to the table:

1. Put the header `Average` in `J4`.
2. In `J5:J10`, put a formula giving the average of that row's four quarters (`C:F`).
   Use live formulas, not typed numbers.
3. In `J11`, put a formula giving the average of `J5:J10`.

Save the result back to `/app/sales.xlsx`.

The `gridcraft-cli` tool is installed. `gridcraft-cli commands` lists the spreadsheet commands,
and `gridcraft-cli run --in FILE --cmd 'id={json}' --out FILE` runs them.
