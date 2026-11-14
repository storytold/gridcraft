use super::{Area, CellError, Evaluator, Expr, Value, resized_sum_range};

impl Evaluator<'_> {
    /// Fast path for one literal text equality over constant reference ranges. No expression is
    /// evaluated speculatively: a declined call takes the original path in its original order.
    pub(super) fn indexed_criteria(&mut self, name: &str, args: &[Expr]) -> Option<Value> {
        let (range_at, criterion_at, target_at, resize) = match (name, args.len()) {
            ("COUNTIF" | "COUNTIFS", 2) => (0, 1, 0, false),
            ("SUMIF" | "AVERAGEIF", 2) => (0, 1, 0, false),
            ("SUMIF" | "AVERAGEIF", 3) => (0, 1, 2, true),
            ("SUMIFS" | "AVERAGEIFS" | "MINIFS" | "MAXIFS", 3) => (1, 2, 0, false),
            _ => return None,
        };
        let mut range = self.criteria_area(args.get(range_at)?)?;
        let mut target = self.criteria_area(args.get(target_at)?)?;
        if resize {
            target.range = resized_sum_range(target.range, range.range.height(), range.range.width());
        } else if range.range.height() != target.range.height() || range.range.width() != target.range.width() {
            return None;
        }
        // As the ordinary path reads them: SUMIF's sum range already takes the criteria's size.
        let (given_range, given_target) = (range, target);
        range = self.materialized_area(&range).ok()?;
        target = self.materialized_area(&target).ok()?;
        if range.range.height() != target.range.height() || range.range.width() != target.range.width() {
            return None;
        }
        let criterion = match args.get(criterion_at)? {
            Expr::Text(t) => Value::Text(t.clone()),
            e @ Expr::Ref(_) => self.host.constant_value(self.criteria_area(e)?)?,
            _ => return None,
        };
        let key = gridcraft_functions::TextCriteriaIndex::key(&criterion)?;
        let index = self.host.text_criteria_index(range, target)?;
        // Count the work the ordinary path does, so the formula runs out (#CALC!) where it would:
        // a unit per argument evaluated and for the criterion handed over, and each multi-cell
        // range's cells once read and once handed over (a single cell is only handed over).
        let handed = |given: Area, read: Area| {
            if given.range.is_single() { 1 } else { 2 * u64::from(read.range.height()) * u64::from(read.range.width()) }
        };
        let mut work = (args.len() as u64) + 1 + handed(given_range, range);
        if target_at != range_at {
            work += handed(given_target, target);
        }
        if !self.spend(work) {
            return Some(Value::Error(CellError::Calc));
        }
        Some(index.aggregate(name, &key))
    }

    fn criteria_area(&self, e: &Expr) -> Option<Area> {
        let Expr::Ref(r) = e else { return None };
        let areas = self.reference_areas(r).ok()?;
        match areas.as_slice() {
            [area] => Some(*area),
            _ => None,
        }
    }
}
