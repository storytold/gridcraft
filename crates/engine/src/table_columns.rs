//! Table calculated-column creation, within the same transaction as the entered formula.
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_model::{Cell, Formula};

use crate::{EngineError, Result, cmd::Ctx};

/// Create an empty calculated column, or update a consistent one. A column with exceptions
/// keeps single-cell edits: its stored master formula and the other cells remain unchanged.
/// Called from single-cell entry, not imports or multi-cell writes.
pub(crate) fn fill_column(cx: &mut Ctx<'_>, sheet: usize, at: CellRef, previous: Option<&Formula>) -> Result<()> {
    let Some(sh) = cx.wb.sheet(sheet) else { return Ok(()) };
    let Some(formula) = sh.cell(at).and_then(|cell| cell.formula.clone()).filter(|f| f.array.is_none()) else { return Ok(()) };
    let Some((table_index, data)) = sh.tables.iter().enumerate().find_map(|(i, table)| table.data_range().filter(|r| r.contains(at)).map(|r| (i, r)))
    else {
        return Ok(());
    };
    let Some(column_index) = at.col.checked_sub(data.start.col).map(|i| i as usize) else { return Ok(()) };
    if sh.tables.get(table_index).and_then(|t| t.columns.get(column_index)).is_none() {
        return Ok(());
    }
    let column = RangeRef::new(CellRef::new(data.start.row, at.col), CellRef::new(data.end.row, at.col));
    if sh.cell_pictures.range(column.start..=column.end).any(|(c, _)| column.contains(*c) && *c != at) {
        return Ok(());
    }
    let empty = !sh.cells.iter_range(column).any(|(c, cell)| c != at && (cell.formula.is_some() || !cell.value.is_empty()));
    if !empty {
        let Some(master) = sh.tables.get(table_index).and_then(|t| t.columns.get(column_index)).and_then(|c| c.formula.as_deref()) else {
            return Ok(());
        };
        let Some(master) = Formula::new(master).expr() else { return Ok(()) };
        let invariant = gridcraft_formula::adjust::shift_relative(master.clone(), 1, 0) == master;
        let matches = |cell: Option<&Formula>, row: u32| {
            cell.is_some_and(|f| {
                f.array.is_none() && f.expr_arc().is_some_and(|e| if invariant { *e == master } else { *e == shifted(&master, data.start.row, row) })
            })
        };
        // The edited cell's old contents count too: replacing an exception is not permission
        // to overwrite the rest of the column. Blanks and constants are exceptions as well.
        if !matches(previous, at.row)
            || (data.start.row..=data.end.row)
                .any(|row| row != at.row && !matches(sh.cell(CellRef::new(row, at.col)).and_then(|c| c.formula.as_deref()), row))
        {
            return Ok(());
        }
        if matches(Some(&formula), at.row) {
            return Ok(());
        }
    }
    let Some(expr) = formula.expr() else { return Ok(()) };
    // Structured references and absolute references can share one parsed formula across every
    // row. Only relative row references require a distinct expression and text per destination.
    let row_invariant = gridcraft_formula::adjust::shift_relative(expr.clone(), 1, 0) == expr;
    // Bound work from file-controlled table dimensions and formula length. The allowance for
    // shifted addresses covers their growth to the workbook's maximum row/column indices.
    let per_row = if row_invariant { 0 } else { formula.text.len().saturating_add(64) as u64 };
    let generated_bytes = u64::from(column.height()).checked_mul(per_row);
    if generated_bytes.is_none_or(|n| n > 64 * 1024 * 1024) {
        return Err(EngineError::Other("The calculated column is too large to fill automatically. Fill a smaller range instead.".into()));
    }
    let first = Formula::from_expr(gridcraft_formula::adjust::shift_relative(expr.clone(), i64::from(data.start.row) - i64::from(at.row), 0));
    if let Some(col) = cx.wb.sheet_mut(sheet).and_then(|sh| sh.tables.get_mut(table_index)).and_then(|t| t.columns.get_mut(column_index)) {
        col.formula = Some(first.text);
    }
    for row in data.start.row..=data.end.row {
        if row == at.row {
            continue;
        }
        let c = CellRef::new(row, at.col);
        let shifted = if row_invariant {
            formula.clone()
        } else {
            Arc::new(Formula::from_expr(gridcraft_formula::adjust::shift_relative(expr.clone(), i64::from(row) - i64::from(at.row), 0)))
        };
        let sh = cx.sheet_mut(sheet)?;
        let style = sh.style_id(c);
        sh.set_cell(c, Cell { value: Value::Empty, formula: Some(shifted), style });
        cx.touch(sheet, c);
    }
    Ok(())
}

fn shifted(expr: &Expr, from: u32, to: u32) -> Expr {
    gridcraft_formula::adjust::shift_relative(expr.clone(), i64::from(to) - i64::from(from), 0)
}

/// The table's XLSX master is a formula too. Use the same rewrite as cell formulas, anchored
/// at the first data row, including when that cell is an exception or has been cleared.
pub(crate) fn rewrite_masters(wb: &mut gridcraft_model::Workbook, mut rewrite: impl FnMut(usize, CellRef, Expr) -> Expr) {
    for si in 0..wb.sheets.len() {
        let Some(sh) = wb.sheet_mut(si) else { continue };
        for table in &mut sh.tables {
            let Some(data) = table.data_range() else { continue };
            for (ci, column) in table.columns.iter_mut().enumerate() {
                let Some(text) = &column.formula else { continue };
                let Ok(expr) = gridcraft_formula::parse(text) else { continue };
                let Some(col) = u32::try_from(ci).ok().and_then(|c| data.start.col.checked_add(c)) else { continue };
                let result = gridcraft_formula::print(&rewrite(si, CellRef::new(data.start.row, col), expr));
                if result != *text {
                    column.formula = Some(result);
                }
            }
        }
    }
}
