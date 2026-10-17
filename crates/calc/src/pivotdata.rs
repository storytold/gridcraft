//! GETPIVOTDATA: a value from a PivotTable report, found by its value field and the row and
//! column items that locate it (Excel's documented semantics). The engine keeps each
//! PivotTable's computed value cells in `PivotTable::data_cells`.

use gridcraft_core::{CellError, Value};
use gridcraft_formula::Expr;
use gridcraft_model::{PivotDataCell, PivotItemRef, PivotTable};

use crate::eval::{Ev, Evaluator};

/// Excel allows 126 field/item pairs.
const MAX_PAIRS: usize = 126;

fn same_text(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// A number matches an item with that value (dates as serials) or with that label; anything
/// else matches the label as text.
fn item_matches(item: &PivotItemRef, v: &Value) -> bool {
    if let (Value::Number(n), Value::Number(x)) = (v, &item.value)
        && n == x
    {
        return true;
    }
    if let Value::Bool(b) = v {
        return item.value == Value::Bool(*b);
    }
    v.to_text().is_ok_and(|t| same_text(&t, &item.label))
}

fn is_axis_field(pt: &PivotTable, field: &str) -> bool {
    pt.rows.iter().chain(pt.columns.iter()).any(|f| same_text(&f.source_col, field))
}

/// The value cell whose value field and items are exactly those asked for.
fn find<'a>(pt: &'a PivotTable, data_field: &str, pairs: &[(String, Value)]) -> Option<&'a PivotDataCell> {
    pt.data_cells.iter().find(|cell| {
        (same_text(&cell.data_field, data_field) || same_text(&cell.source_col, data_field))
            && cell.items.len() == pairs.len()
            && pairs.iter().all(|(f, v)| cell.items.iter().any(|it| same_text(&it.field, f) && item_matches(it, v)))
    })
}

impl Evaluator<'_> {
    pub(crate) fn getpivotdata(&mut self, args: &[Expr]) -> Ev {
        // A refresh changes the report without touching the formula's references.
        self.host.note_volatile();
        match self.pivot_value(args) {
            Ok(v) => Ev::V(v),
            Err(e) => Ev::V(Value::Error(e)),
        }
    }

    fn scalar_arg(&mut self, e: &Expr) -> Result<Value, CellError> {
        match self.value(e).scalar() {
            Value::Error(e) => Err(e),
            v => Ok(v),
        }
    }

    fn pivot_value(&mut self, args: &[Expr]) -> Result<Value, CellError> {
        let (Some(field_expr), Some(table_expr)) = (args.first(), args.get(1)) else { return Err(CellError::Value) };
        let rest = args.get(2..).unwrap_or_default();
        if rest.len() % 2 != 0 || rest.len() / 2 > MAX_PAIRS {
            return Err(CellError::Ref);
        }
        let data_field = self.scalar_arg(field_expr)?.to_text()?;
        let area = match self.eval(table_expr) {
            Ev::R(areas) => areas.first().copied().ok_or(CellError::Ref)?,
            Ev::V(Value::Error(e)) => return Err(e),
            _ => return Err(CellError::Ref),
        };
        let mut pairs: Vec<(String, Value)> = Vec::with_capacity(rest.len() / 2);
        for pair in rest.chunks(2) {
            let (Some(f), Some(i)) = (pair.first(), pair.get(1)) else { return Err(CellError::Ref) };
            let field = self.scalar_arg(f)?.to_text()?;
            let item = self.scalar_arg(i)?;
            if pairs.iter().any(|(g, _)| same_text(g, &field)) {
                return Err(CellError::Ref);
            }
            pairs.push((field, item));
        }
        let wb = self.host.workbook();
        let sheet = wb.sheet(area.sheet).ok_or(CellError::Ref)?;
        let pt = sheet.pivots.iter().find(|p| p.last_range.is_some_and(|r| r.intersects(&area.range))).ok_or(CellError::Ref)?;
        // A report filter field may be named when the report shows exactly that one item.
        let mut axis_pairs = Vec::with_capacity(pairs.len());
        for (field, item) in pairs {
            if is_axis_field(pt, &field) {
                axis_pairs.push((field, item));
                continue;
            }
            let filter = pt.filters.iter().find(|f| same_text(&f.source_col, &field)).ok_or(CellError::Ref)?;
            let shown = match filter.selected.as_deref() {
                Some([one]) => item.to_text().is_ok_and(|t| same_text(&t, one)),
                _ => false,
            };
            if !shown {
                return Err(CellError::Ref);
            }
        }
        find(pt, &data_field, &axis_pairs).map(|c| c.value.clone()).ok_or(CellError::Ref)
    }
}
