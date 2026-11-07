//! Show Values As: what a value cell shows in place of its plain aggregate.
//!
//! Most modes measure a cell against other cells of the report. The base-field modes (see
//! [`PivotShowAs`]) move along one row or column field: they swap the cell's item of that field
//! for another item and read the aggregate there. A cell whose path stops above the base field (a
//! subtotal of an outer field, a grand total) is a total *over* that field, so it has no item to
//! swap; each mode documents what such a cell shows.

use std::cell::RefCell;
use std::rc::Rc;

use super::items::FieldItems;
use super::output::{AggMap, ValSrc};
use super::{CalcSet, CellError, HashMap, PivotBaseItem, PivotField, PivotFunc, PivotShowAs, PivotValue, Source, Value, Workbook};

/// For each item path of an axis (`[]` for the top level), its child items in display order.
pub(super) type Siblings = HashMap<Vec<u32>, Vec<u32>>;

/// The aggregate of a value field at a pair of row and column item paths.
pub(super) struct ValueLookup<'a> {
    pub(super) wb: &'a Workbook,
    pub(super) source: &'a Source,
    pub(super) calc: &'a CalcSet,
    pub(super) vals: &'a [(ValSrc, &'a PivotValue)],
    pub(super) extra: &'a [usize],
    pub(super) aggs: &'a AggMap,
}

impl ValueLookup<'_> {
    /// `None` when no record falls in that cell.
    pub(super) fn get(&self, row_path: &[u32], col_path: &[u32], value_index: usize) -> Option<Value> {
        let (source, definition) = self.vals.get(value_index)?;
        let accumulators = self.aggs.get(&(row_path.to_vec(), col_path.to_vec()))?;
        match source {
            ValSrc::Col(_) => accumulators.get(value_index).map(|acc| acc.result(definition.func)),
            ValSrc::Calc(index) => {
                let sum = |name: &str| -> Value {
                    self.source
                        .col(name)
                        .and_then(|column| self.extra.iter().position(|extra_column| *extra_column == column))
                        .and_then(|index| accumulators.get(self.vals.len() + index))
                        .map(|acc| acc.result(PivotFunc::Sum))
                        .unwrap_or(Value::Number(0.0))
                };
                Some(self.calc.value(self.wb, *index, &sum))
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Axis {
    Rows,
    Columns,
}

/// One axis of the report: each field's items (outermost field first) and the item tree.
pub(super) struct AxisItems<'a> {
    pub(super) items: &'a [FieldItems],
    pub(super) siblings: &'a Siblings,
}

/// A value field's base field, resolved against the report: its axis and nesting depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BaseField {
    pub(super) axis: Axis,
    pub(super) depth: usize,
}

/// The base field of `value` in a report with these row and column fields: the named field
/// (searched in Rows, then Columns), else the first row field, else the first column field.
/// A name that is no longer in the report falls back the same way. `None` without axis fields.
pub(super) fn resolve_base_field(value: &PivotValue, rows: &[&PivotField], cols: &[&PivotField]) -> Option<BaseField> {
    let find = |fields: &[&PivotField], name: &str| fields.iter().position(|f| f.source_col.eq_ignore_ascii_case(name));
    let named = value.base_field.as_deref().and_then(|name| {
        find(rows, name)
            .map(|depth| BaseField { axis: Axis::Rows, depth })
            .or_else(|| find(cols, name).map(|depth| BaseField { axis: Axis::Columns, depth }))
    });
    let first = if rows.is_empty() { (!cols.is_empty()).then_some(Axis::Columns) } else { Some(Axis::Rows) };
    named.or(first.map(|axis| BaseField { axis, depth: 0 }))
}

/// A value cell: its row and column item paths and its value field.
#[derive(Clone, Copy)]
pub(super) struct CellAt<'a> {
    pub(super) row: &'a [u32],
    pub(super) col: &'a [u32],
    pub(super) value_index: usize,
}

impl CellAt<'_> {
    /// The cell's item path on an axis.
    fn path(&self, axis: Axis) -> &[u32] {
        match axis {
            Axis::Rows => self.row,
            Axis::Columns => self.col,
        }
    }

    /// The cell's (parent path, item) on the base field; `None` for a total over that field.
    fn own_item(&self, base: BaseField) -> Option<(&[u32], u32)> {
        let path = self.path(base.axis);
        Some((path.get(..base.depth)?, *path.get(base.depth)?))
    }

    /// The cell's row and column paths with its item of the base field replaced by `item`.
    fn with_item(&self, base: BaseField, item: u32) -> (Vec<u32>, Vec<u32>) {
        let (mut row, mut col) = (self.row.to_vec(), self.col.to_vec());
        let path = match base.axis {
            Axis::Rows => &mut row,
            Axis::Columns => &mut col,
        };
        if let Some(slot) = path.get_mut(base.depth) {
            *slot = item;
        }
        (row, col)
    }
}

/// The items of the base field around a cell, in display order: its item's siblings, with the
/// cell's other items kept. Running totals, ranks and base items read it; built once per run and
/// value field, it makes each cell a lookup instead of a pass over its run (quadratic in the
/// number of items).
struct Run {
    items: Vec<u32>,
    /// Item → its index in `items`.
    position: HashMap<u32, usize>,
    /// The running total up to and including each item (non-numbers count as 0).
    running: Vec<f64>,
    total: f64,
    /// The distinct numeric values of the run, ascending (for dense ranks).
    distinct: Vec<f64>,
    /// The named base item, when the value field has one and it is in the run.
    named: Option<u32>,
}

/// (row path, column path, value field) of a run, with the base field's item blanked out.
type RunKey = (Vec<u32>, Vec<u32>, usize);

/// Stands in for the base field's item in a [`RunKey`] (item ids are indices, far below it).
const ANY_ITEM: u32 = u32::MAX;

/// Computes what value cells show; one per report.
pub(super) struct ShowAs<'a> {
    pub(super) lookup: &'a ValueLookup<'a>,
    pub(super) rows: AxisItems<'a>,
    pub(super) cols: AxisItems<'a>,
    runs: RefCell<HashMap<RunKey, Rc<Run>>>,
}

impl<'a> ShowAs<'a> {
    /// One per report, over its value lookup and both axes; runs are built as cells need them.
    pub(super) fn new(lookup: &'a ValueLookup<'a>, rows: AxisItems<'a>, cols: AxisItems<'a>) -> Self {
        ShowAs { lookup, rows, cols, runs: RefCell::default() }
    }

    /// What the cell shows for `value` (whose base field is `base`), given its aggregate `raw`.
    pub(super) fn value(&self, value: &PivotValue, base: Option<BaseField>, raw: Value, cell: CellAt<'_>) -> Value {
        let at = |row: &[u32], col: &[u32]| self.lookup.get(row, col, cell.value_index);
        match value.show_as {
            PivotShowAs::Normal => raw,
            PivotShowAs::PercentOfGrandTotal => ratio(raw, at(&[], &[])),
            PivotShowAs::PercentOfColumnTotal => ratio(raw, at(&[], cell.col)),
            PivotShowAs::PercentOfRowTotal => ratio(raw, at(cell.row, &[])),
            PivotShowAs::PercentOfParentRowTotal => {
                // The parent of an outermost item, and of the grand total, is the whole report.
                let parent = cell.row.split_last().map_or(&[][..], |(_, parent)| parent);
                ratio(raw, at(parent, cell.col))
            }
            PivotShowAs::Index => self.index(raw, cell),
            PivotShowAs::PercentOf | PivotShowAs::DifferenceFrom | PivotShowAs::PercentDifferenceFrom => {
                self.against_base_item(value, base, raw, cell)
            }
            PivotShowAs::RunningTotal => match base {
                Some(base) => self.running_total(value, base, raw, cell, false),
                None => raw,
            },
            PivotShowAs::PercentRunningTotal => match base {
                Some(base) => self.running_total(value, base, raw, cell, true),
                None => raw,
            },
            PivotShowAs::Rank => base.map_or(Value::Empty, |base| self.rank(value, base, &raw, cell, true)),
            PivotShowAs::RankAscending => base.map_or(Value::Empty, |base| self.rank(value, base, &raw, cell, false)),
        }
    }

    /// The items and item tree of an axis.
    fn axis(&self, axis: Axis) -> &AxisItems<'_> {
        match axis {
            Axis::Rows => &self.rows,
            Axis::Columns => &self.cols,
        }
    }

    /// The run of the base field the cell is in, and the cell's index in it. `None` for a total
    /// over the base field.
    fn run(&self, value: &PivotValue, base: BaseField, cell: CellAt<'_>) -> Option<(Rc<Run>, usize)> {
        let (parent, own) = cell.own_item(base)?;
        let (row, col) = cell.with_item(base, ANY_ITEM);
        let key = (row, col, cell.value_index);
        let cached = self.runs.borrow().get(&key).cloned();
        let run = match cached {
            Some(run) => run,
            None => {
                let run = Rc::new(self.build_run(value, base, cell, self.axis(base.axis).siblings.get(parent)?));
                self.runs.borrow_mut().insert(key, Rc::clone(&run));
                run
            }
        };
        let index = *run.position.get(&own)?;
        Some((run, index))
    }

    /// Reads the run's totals once: each sibling's number, the running totals, the distinct values
    /// for ranks, and the named base item when the value field has one.
    fn build_run(&self, value: &PivotValue, base: BaseField, cell: CellAt<'_>, siblings: &[u32]) -> Run {
        let numbers: Vec<Option<f64>> = siblings
            .iter()
            .map(|item| {
                let (row, col) = cell.with_item(base, *item);
                self.lookup.get(&row, &col, cell.value_index).as_ref().and_then(Value::as_f64)
            })
            .collect();
        let mut total = 0.0;
        let running = numbers
            .iter()
            .map(|n| {
                total += n.unwrap_or(0.0);
                total
            })
            .collect();
        let mut distinct: Vec<f64> = numbers.iter().flatten().copied().collect();
        distinct.sort_by(f64::total_cmp);
        distinct.dedup();
        let named = match &value.base_item {
            PivotBaseItem::Named(label) => self
                .axis(base.axis)
                .items
                .get(base.depth)
                .and_then(|items| siblings.iter().copied().find(|item| items.label(*item).eq_ignore_ascii_case(label))),
            PivotBaseItem::Previous | PivotBaseItem::Next => None,
        };
        let position = siblings.iter().enumerate().map(|(i, item)| (*item, i)).collect();
        Run { items: siblings.to_vec(), position, running, total, distinct, named }
    }

    /// The base item of the cell. `None` when there is none: the cell is a total over the base
    /// field, it is the first (previous) or last (next) item, or the named item has no records
    /// beside it.
    fn base_item(&self, value: &PivotValue, base: BaseField, cell: CellAt<'_>) -> Option<u32> {
        let (run, index) = self.run(value, base, cell)?;
        match &value.base_item {
            PivotBaseItem::Previous => run.items.get(index.checked_sub(1)?).copied(),
            PivotBaseItem::Next => run.items.get(index.checked_add(1)?).copied(),
            PivotBaseItem::Named(_) => run.named,
        }
    }

    /// % Of, Difference From and % Difference From. A cell without a base item, or whose base item
    /// has no records there, is empty; so is the base item itself under the two differences (it
    /// would always be zero), while % Of shows it as 100%.
    fn against_base_item(&self, value: &PivotValue, base: Option<BaseField>, raw: Value, cell: CellAt<'_>) -> Value {
        let Some(base) = base else { return Value::Empty };
        let Some(item) = self.base_item(value, base, cell) else { return Value::Empty };
        let (row, col) = cell.with_item(base, item);
        let is_own = row == cell.row && col == cell.col;
        if is_own && value.show_as != PivotShowAs::PercentOf {
            return Value::Empty;
        }
        let Some(reference) = self.lookup.get(&row, &col, cell.value_index) else { return Value::Empty };
        let (own, [reference]) = match numbers(raw, [reference]) {
            Ok(n) => n,
            Err(shown) => return shown,
        };
        match value.show_as {
            PivotShowAs::PercentOf => divide(own, reference),
            PivotShowAs::DifferenceFrom => Value::number(own - reference),
            _ => divide(own - reference, reference),
        }
    }

    /// Running Total In (and its percentage): the sum over the base field's items up to the
    /// cell's own, the other items of the cell kept. A total over the base field shows its plain
    /// value (100% as a percentage): it is where the running total ends.
    fn running_total(&self, value: &PivotValue, base: BaseField, raw: Value, cell: CellAt<'_>, percent: bool) -> Value {
        if !raw.is_number() {
            return raw;
        }
        let Some((run, index)) = self.run(value, base, cell) else {
            return if percent { ratio(raw.clone(), Some(raw)) } else { raw };
        };
        let running = run.running.get(index).copied().unwrap_or(0.0);
        if percent { divide(running, run.total) } else { Value::number(running) }
    }

    /// Rank among the base field's items, the other items of the cell kept. Ranks are dense, as
    /// in Excel: ties share a rank and the next value takes the next one (5, 4, 3, 3 rank 1, 2,
    /// 3, 3). A total over the base field has no rank.
    fn rank(&self, value: &PivotValue, base: BaseField, raw: &Value, cell: CellAt<'_>, descending: bool) -> Value {
        let Some((run, _)) = self.run(value, base, cell) else { return Value::Empty };
        let Some(me) = raw.as_f64() else { return raw.clone() };
        let ahead =
            if descending { run.distinct.len() - run.distinct.partition_point(|n| *n <= me) } else { run.distinct.partition_point(|n| *n < me) };
        Value::number(ahead as f64 + 1.0)
    }

    /// Index: (value × grand total) / (row total × column total).
    fn index(&self, raw: Value, cell: CellAt<'_>) -> Value {
        let at = |row: &[u32], col: &[u32]| self.lookup.get(row, col, cell.value_index).unwrap_or(Value::Empty);
        match numbers(raw, [at(&[], &[]), at(cell.row, &[]), at(&[], cell.col)]) {
            Ok((own, [grand, row_total, col_total])) => divide(own * grand, row_total * col_total),
            Err(shown) => shown,
        }
    }
}

/// The number format a value field's cells get: the field's own, else a percentage for the ratio
/// modes, none for ranks and the index (plain numbers), else the source column's format
/// (`source`) for the functions that keep the source's units.
pub(super) fn display_format(value: &PivotValue, source: impl FnOnce() -> Option<String>) -> Option<String> {
    if let Some(format) = &value.number_format {
        return Some(format.clone());
    }
    match value.show_as {
        mode if mode.is_percentage() => Some("0.00%".into()),
        PivotShowAs::Rank | PivotShowAs::RankAscending | PivotShowAs::Index => None,
        _ => match value.func {
            PivotFunc::Count | PivotFunc::CountNumbers => None,
            _ => source().filter(|format| format != "General"),
        },
    }
}

/// `raw` and the other values as numbers, or what the cell shows instead: `raw` itself when it is
/// not a number (an error, or text from a calculated field), else the first error among the
/// others, else an empty cell when one of them is not a number.
fn numbers<const N: usize>(raw: Value, others: [Value; N]) -> Result<(f64, [f64; N]), Value> {
    let Value::Number(own) = raw else { return Err(raw) };
    let mut out = [0.0; N];
    for (slot, value) in out.iter_mut().zip(others) {
        *slot = match value {
            Value::Number(n) => n,
            Value::Error(e) => return Err(Value::Error(e)),
            _ => return Err(Value::Empty),
        };
    }
    Ok((own, out))
}

/// `numerator / denominator`, `#DIV/0!` for a zero denominator.
fn divide(numerator: f64, denominator: f64) -> Value {
    if denominator == 0.0 { Value::Error(CellError::Div0) } else { Value::number(numerator / denominator) }
}

/// `raw` over `total` for the percentage modes: errors pass through (the cell's own first),
/// `#DIV/0!` for a zero total, and a cell that is not a number (or has no total) shows `raw`.
fn ratio(raw: Value, total: Option<Value>) -> Value {
    match (raw, total) {
        (Value::Error(e), _) | (Value::Number(_), Some(Value::Error(e))) => Value::Error(e),
        (Value::Number(own), Some(Value::Number(total))) => divide(own, total),
        (raw, _) => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_prefers_the_cells_own_value_then_the_first_error() {
        assert_eq!(numbers(Value::Number(2.0), [Value::Number(3.0)]), Ok((2.0, [3.0])));
        assert_eq!(numbers(Value::text("x"), [Value::Error(CellError::NA)]), Err(Value::text("x")));
        assert_eq!(numbers(Value::Number(1.0), [Value::Empty, Value::Error(CellError::NA)]), Err(Value::Empty));
        assert_eq!(numbers(Value::Number(1.0), [Value::Error(CellError::Ref), Value::Empty]), Err(Value::Error(CellError::Ref)));
    }

    #[test]
    fn divide_by_zero_is_an_error_not_infinity() {
        assert_eq!(divide(1.0, 0.0), Value::Error(CellError::Div0));
        assert_eq!(divide(0.0, 0.0), Value::Error(CellError::Div0));
        assert_eq!(divide(1.0, 4.0), Value::Number(0.25));
    }

    #[test]
    fn ratio_passes_errors_and_non_numbers_through() {
        assert_eq!(ratio(Value::Number(1.0), Some(Value::Number(4.0))), Value::Number(0.25));
        assert_eq!(ratio(Value::Number(1.0), Some(Value::Number(0.0))), Value::Error(CellError::Div0));
        assert_eq!(ratio(Value::Error(CellError::NA), Some(Value::Error(CellError::Ref))), Value::Error(CellError::NA));
        assert_eq!(ratio(Value::Number(1.0), Some(Value::Error(CellError::Ref))), Value::Error(CellError::Ref));
        assert_eq!(ratio(Value::text("x"), Some(Value::Number(2.0))), Value::text("x"));
        assert_eq!(ratio(Value::Number(3.0), None), Value::Number(3.0));
    }

    fn field(name: &str) -> PivotField {
        PivotField { source_col: name.into(), ..PivotField::default() }
    }

    #[test]
    fn base_field_is_found_in_either_axis_and_defaults_to_the_first_field() {
        let (region, product, year) = (field("Region"), field("Product"), field("Year"));
        let rows = [&region, &product];
        let cols = [&year];
        let base = |name: Option<&str>| {
            let value = PivotValue { base_field: name.map(str::to_string), ..PivotValue::default() };
            resolve_base_field(&value, &rows, &cols)
        };
        assert_eq!(base(Some("product")), Some(BaseField { axis: Axis::Rows, depth: 1 }));
        assert_eq!(base(Some("YEAR")), Some(BaseField { axis: Axis::Columns, depth: 0 }));
        assert_eq!(base(None), Some(BaseField { axis: Axis::Rows, depth: 0 }));
        assert_eq!(base(Some("gone")), Some(BaseField { axis: Axis::Rows, depth: 0 }), "a stale name falls back");
        let none = PivotValue::default();
        assert_eq!(resolve_base_field(&none, &[], &cols), Some(BaseField { axis: Axis::Columns, depth: 0 }));
        assert_eq!(resolve_base_field(&none, &[], &[]), None);
    }

    #[test]
    fn a_cell_above_the_base_field_has_no_item_of_it() {
        let base = BaseField { axis: Axis::Rows, depth: 1 };
        let grand = CellAt { row: &[], col: &[], value_index: 0 };
        let subtotal = CellAt { row: &[4], col: &[], value_index: 0 };
        let leaf = CellAt { row: &[4, 7, 9], col: &[2], value_index: 0 };
        assert_eq!(grand.own_item(base), None);
        assert_eq!(subtotal.own_item(base), None);
        assert_eq!(leaf.own_item(base), Some((&[4][..], 7)));
        assert_eq!(leaf.with_item(base, 1), (vec![4, 1, 9], vec![2]));
        let on_columns = BaseField { axis: Axis::Columns, depth: 0 };
        assert_eq!(leaf.with_item(on_columns, 5), (vec![4, 7, 9], vec![5]));
        assert_eq!(grand.own_item(BaseField { axis: Axis::Rows, depth: usize::MAX }), None, "no overflow on a huge depth");
    }
}
