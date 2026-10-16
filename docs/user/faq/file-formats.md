---
title: Which file formats can GridCraft open?
description: GridCraft 0.6.1 opens .xlsx, .xlsm, .csv, .tsv and .txt fully, opens old .xls files and password-protected workbooks, and reads only the values of .xlsb and .ods.
order: 10
verified: 0.6.1
---

GridCraft 0.6.1 opens `.xlsx` files fully and saves to them. It also opens `.xlsm`, `.csv`, `.tsv` and `.txt`, old `.xls` files and password-protected workbooks. It reads only the saved values of `.xlsb` and `.ods` files.

| Format                          | Opens in 0.6.1                                                                 |
|---------------------------------|--------------------------------------------------------------------------------|
| `.xlsx` Excel workbook          | Yes. This is the format GridCraft saves to.                                    |
| `.xlsm` macro-enabled workbook  | Yes. Macros are kept when you save, but GridCraft does not run them.           |
| `.xls` Excel 97-2003            | Yes, without formatting. Opens as a new workbook: save it as `.xlsx`.          |
| `.xlsb` Excel binary workbook   | Values only: no formulas, formatting or charts.                                |
| `.ods` OpenDocument spreadsheet | Values only: no formulas, formatting or charts.                                |
| `.csv`, `.tsv`, `.txt`          | Yes.                                                                           |
| Password-protected workbooks    | Yes. GridCraft asks for the password.                                          |

## Old .xls files

GridCraft reads the sheets, values, formulas, number formats, merged cells, column widths, row heights and names of an `.xls` file. It does not read fonts, fills, borders, alignment, charts, pictures, comments, conditional formatting or data validation. The workbook opens without a file name, so **Save** asks where to save it as `.xlsx`. GridCraft cannot save `.xls` files.

GridCraft refuses, with a message, `.xls` files that are password-protected and files from Excel 95 or older.

## Password-protected workbooks

When you open a protected `.xlsx` or `.xlsm` file, a **Password** window asks for it. Type the password and click **OK**. If it is wrong, GridCraft says so and you can try again.

GridCraft cannot save a workbook with a password. The workbook opens without a file name, so it is not saved over the protected file, and the copy you save has no password. Some older kinds of protection are refused with a message.

## Workbooks with macros

When an `.xlsm` file contains macros, GridCraft shows "This workbook contains VBA macros. GridCraft keeps them intact when you save, but does not run them." Save it as `.xlsm` to keep the macros.

## Very large workbooks

A workbook of around 100 MB may fail to open. GridCraft limits one part of an `.xlsx` file to 512 MiB once unpacked, and a 74 MB file can unpack to more than that ([issue #175](https://github.com/storytold/gridcraft/issues/175)). This is still the case in 0.6.1.

## Related

[Open a workbook](../guides/open-a-workbook.md)
