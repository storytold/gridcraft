---
title: Write formulas
description: Type a formula in a cell, pick cells with the mouse, find a function, and total a column with AutoSum.
order: 30
verified: 0.6.1
---

Select a cell, type `=` and your formula, then press Enter. The cell shows the result and the formula bar shows the formula.

## Type a formula

1. Click the cell.
2. Type `=` followed by the formula, for example `=SUM(B2:B9)`. Your typing also appears in the formula bar above the grid.
3. Press Enter to keep it. To throw it away, click the **Cancel** button next to the Name Box. The **Enter** button next to it keeps the formula too.

While you type a function's arguments, a hint under the cell shows them with a short description of the function.

If a formula gives a function too few or too many arguments, GridCraft does not enter it. It shows how to call the function instead, for example "DATEDIF has too few arguments. Use DATEDIF(start_date, end_date, unit)", and leaves the cell as it was.

To edit a formula later, select the cell and click in the formula bar. The **Expand formula bar** button at its right end makes the bar taller for long formulas.

## Pick cells with the mouse

While you type a formula, right after `=`, `(`, `,` or an operator such as `+`, `-`, `*`, `/` or `&`, click a cell to put its reference in the formula. The arrow keys do the same. You do not have to type `B2` yourself.

## Find a function

1. Click the **Insert Function** button (the fx button) next to the formula bar. You can also click **Insert Function** on the **Formulas** tab, which shows the same window.
2. In the **Formula Builder** window, type a word such as `lookup`, `average` or `date` into **Search:**.
3. Click a function. Its arguments and a short description appear under the list.
4. Click **OK**. GridCraft starts the formula with the function's name and an opening bracket, ready for its arguments.

The **Formulas** tab also has a button per group of functions: **Financial**, **Logical**, **Text**, **Date & Time**, **Lookup & Reference**, **Math & Trig** and **More Functions**. Click a function in the list to insert it.

## Add up a column

1. On the **Formulas** tab, select the empty cell under your numbers.
2. Click **AutoSum**. GridCraft puts a `SUM` formula there. It looks for numbers directly above the cell, and if there are none, to its left.

If you select a block of cells first, **AutoSum** puts a total under each column of the block.

## Jump to a cell or name a range

The Name Box is the small box at the left of the formula bar. Type a cell or range, such as `D12`, and press Enter to go there. Type a name that does not exist yet and press Enter to give the selected cells that name. Click its arrow to see your names.

## See every formula

Click **Show Formulas** on the **Formulas** tab to show formulas in the cells of the current sheet instead of their results. Click it again to switch back.

## Next

- [Make a chart](make-a-chart.md)
