use super::*;

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
