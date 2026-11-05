//! Value filters (Value Filters, Top 10): which items of a row or column field keep their place,
//! judged by their totals of one value field within each group of the fields outside it.
//!
//! Filtered items leave the report entirely, subtotals and grand totals included, as in Excel: the
//! report computes once per value filter (outermost field first, rows before columns), each pass
//! dropping the records of the items that fail, then once more for the report itself.

use super::{HashMap, HashSet, PivotTopOf, PivotValueRule};

/// The items (of one group) that pass `rule`, given each item's total; `None` for an item without
/// a numeric total, which never passes.
pub(super) fn passing(rule: PivotValueRule, totals: &[(u32, Option<f64>)]) -> HashSet<u32> {
    let numbered: Vec<(u32, f64)> = totals.iter().filter_map(|(item, total)| Some((*item, total.filter(|t| t.is_finite())?))).collect();
    let keep = |test: &dyn Fn(f64) -> bool| numbered.iter().filter(|(_, t)| test(*t)).map(|(item, _)| *item).collect();
    match rule {
        PivotValueRule::Top { count, of } => extremes(numbered.clone(), count, of, true),
        PivotValueRule::Bottom { count, of } => extremes(numbered.clone(), count, of, false),
        PivotValueRule::Equal(v) => keep(&|t| t == v),
        PivotValueRule::NotEqual(v) => keep(&|t| t != v),
        PivotValueRule::Greater(v) => keep(&|t| t > v),
        PivotValueRule::GreaterOrEqual(v) => keep(&|t| t >= v),
        PivotValueRule::Less(v) => keep(&|t| t < v),
        PivotValueRule::LessOrEqual(v) => keep(&|t| t <= v),
        PivotValueRule::Between(a, b) => keep(&|t| t >= a.min(b) && t <= a.max(b)),
        PivotValueRule::NotBetween(a, b) => keep(&|t| t < a.min(b) || t > a.max(b)),
    }
}

/// Top (`largest`) or Bottom: `count` items with every item tied with the last one, or the items
/// whose totals, from the most extreme, add up to `count` percent of the group's total or to
/// `count` (the item that reaches the target is kept).
fn extremes(mut items: Vec<(u32, f64)>, count: f64, of: PivotTopOf, largest: bool) -> HashSet<u32> {
    // Ties sort in item order, so equal totals are cut the same way every time.
    if !count.is_finite() || count <= 0.0 {
        return HashSet::new();
    }
    if largest {
        items.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    } else {
        items.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    }
    let take = match of {
        PivotTopOf::Items => {
            let n = (count.floor() as usize).min(items.len());
            let Some(&(_, cut)) = n.checked_sub(1).and_then(|last| items.get(last)) else { return HashSet::new() };
            items.iter().take_while(|(_, t)| if largest { *t >= cut } else { *t <= cut }).count()
        }
        PivotTopOf::Percent | PivotTopOf::Sum => {
            let target = if of == PivotTopOf::Percent { items.iter().map(|(_, t)| t).sum::<f64>() * count / 100.0 } else { count };
            let mut running = 0.0;
            items
                .iter()
                .take_while(|(_, t)| {
                    let before = running;
                    running += t;
                    before < target
                })
                .count()
        }
    };
    items.into_iter().take(take).map(|(item, _)| item).collect()
}

/// For each group (the item path of the outer fields), the field's items in it, from the item
/// paths of the records left.
pub(super) fn groups<'p>(paths: impl Iterator<Item = &'p [u32]>, depth: usize) -> HashMap<Vec<u32>, Vec<u32>> {
    let mut out: HashMap<Vec<u32>, Vec<u32>> = HashMap::new();
    let mut seen: HashSet<(Vec<u32>, u32)> = HashSet::new();
    for path in paths {
        let (Some(prefix), Some(&item)) = (path.get(..depth), path.get(depth)) else { continue };
        if seen.insert((prefix.to_vec(), item)) {
            out.entry(prefix.to_vec()).or_default().push(item);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[u32]) -> HashSet<u32> {
        items.iter().copied().collect()
    }

    /// Items 0..5 with totals 50, 40, 30, 30, 10 and item 5 without one.
    fn totals() -> Vec<(u32, Option<f64>)> {
        vec![(0, Some(50.0)), (1, Some(40.0)), (2, Some(30.0)), (3, Some(30.0)), (4, Some(10.0)), (5, None)]
    }

    #[test]
    fn top_and_bottom_items_keep_ties_at_the_cut() {
        let top = |n: f64| passing(PivotValueRule::Top { count: n, of: PivotTopOf::Items }, &totals());
        let bottom = |n: f64| passing(PivotValueRule::Bottom { count: n, of: PivotTopOf::Items }, &totals());
        assert_eq!(top(2.0), set(&[0, 1]));
        assert_eq!(top(3.0), set(&[0, 1, 2, 3]), "30 is tied at the cut");
        assert_eq!(top(2.9), set(&[0, 1]), "a count is whole items");
        assert_eq!(top(99.0), set(&[0, 1, 2, 3, 4]), "never the item without a total");
        assert_eq!(bottom(1.0), set(&[4]));
        assert_eq!(bottom(2.0), set(&[4, 2, 3]));
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 0.5] {
            assert!(top(bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn percent_and_sum_add_up_from_the_extreme() {
        // Total 160: 50% is 80, reached by 50 + 40.
        let top_pct = |p: f64| passing(PivotValueRule::Top { count: p, of: PivotTopOf::Percent }, &totals());
        assert_eq!(top_pct(50.0), set(&[0, 1]));
        assert_eq!(top_pct(31.25), set(&[0]), "exactly 50 of 160");
        assert_eq!(top_pct(31.3), set(&[0, 1]), "just past it takes the next item");
        assert_eq!(top_pct(100.0), set(&[0, 1, 2, 3, 4]));
        assert_eq!(passing(PivotValueRule::Top { count: 60.0, of: PivotTopOf::Sum }, &totals()), set(&[0, 1]));
        assert_eq!(passing(PivotValueRule::Bottom { count: 40.0, of: PivotTopOf::Sum }, &totals()), set(&[4, 2]));
    }

    #[test]
    fn comparisons_test_each_total() {
        let p = |rule| passing(rule, &totals());
        assert_eq!(p(PivotValueRule::Equal(30.0)), set(&[2, 3]));
        assert_eq!(p(PivotValueRule::NotEqual(30.0)), set(&[0, 1, 4]));
        assert_eq!(p(PivotValueRule::Greater(30.0)), set(&[0, 1]));
        assert_eq!(p(PivotValueRule::GreaterOrEqual(30.0)), set(&[0, 1, 2, 3]));
        assert_eq!(p(PivotValueRule::Less(30.0)), set(&[4]));
        assert_eq!(p(PivotValueRule::LessOrEqual(30.0)), set(&[2, 3, 4]));
        assert_eq!(p(PivotValueRule::Between(40.0, 30.0)), set(&[1, 2, 3]), "bounds in either order");
        assert_eq!(p(PivotValueRule::NotBetween(30.0, 40.0)), set(&[0, 4]));
        assert_eq!(p(PivotValueRule::Greater(f64::NAN)), set(&[]));
    }

    #[test]
    fn groups_collect_each_items_once_per_parent() {
        let paths: Vec<Vec<u32>> = vec![vec![0, 1], vec![0, 2], vec![0, 1], vec![1, 1], vec![2]];
        let g = groups(paths.iter().map(Vec::as_slice), 1);
        assert_eq!(g.get(&vec![0]), Some(&vec![1, 2]));
        assert_eq!(g.get(&vec![1]), Some(&vec![1]));
        assert_eq!(g.get(&vec![2]), None, "a path that stops above the field");
        let top = groups(paths.iter().map(Vec::as_slice), 0);
        assert_eq!(top.get(&vec![]), Some(&vec![0, 1, 2]));
    }
}
