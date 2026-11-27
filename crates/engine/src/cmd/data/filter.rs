use super::sort::sort_key;
use super::{
    AutoFilter, CellRef, EngineError, FilterCriterion, Json, Ordering, RangeRef, Result, Session, Sheet, Value, Workbook, bad, bool_param, col_param,
    data_target, edit, json,
};
use std::collections::HashMap;

// ---------------------------------------------------------------- filter

pub(super) fn toggle_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = data_target(s, p, "data.filter")?;
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    // The active cell belongs to the active sheet: on another sheet the table is the one at `range`.
    let at = if sheet == s.doc()?.wb.active_sheet() { s.doc()?.selection.active } else { r.start };
    let has = s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.autofilter.is_some());
    let in_table = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.table_at(at).map(|t| t.name.clone()));
    if let Some(name) = in_table {
        // An explicit range over part of the table is refused, as when installing a filter on it.
        if p.get("range").is_some()
            && let Some(sh) = s.doc()?.wb.sheet(sheet)
            && let Some(t) = sh.tables.iter().find(|t| t.name == name)
            && t.range.intersects(&r)
            && !(t.header_row
                && t.range.start.row == r.start.row
                && t.range.contains(r.start)
                && t.range.contains(r.end)
                && r.start.col == t.range.start.col
                && r.end.col == t.range.end.col
                && t.data_range().is_none_or(|data| r.end.row >= data.end.row))
        {
            return Err(bad("data.filter", "a filter range can't overlap a table; filter inside the table instead"));
        }
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            let filtered: Vec<u32> = sh.rows.keys().copied().filter(|r| sh.is_row_filtered(*r)).collect();
            let mut cleared = false;
            if let Some(t) = sh.tables.iter_mut().find(|t| t.name == name) {
                t.filter_button = !t.filter_button;
                if !t.filter_button {
                    t.filter.clear();
                    // The sheet AutoFilter hosted on this table's header row is the table's filter too.
                    cleared = sh.autofilter.as_ref().is_some_and(|af| af.range.start.row == t.range.start.row && af.range.intersects(&t.range));
                }
            }
            if cleared {
                sh.autofilter = None;
                for r in filtered {
                    if let Some(info) = sh.rows.get_mut(&r) {
                        info.hidden = false;
                    }
                }
                // Rows another table's criteria hide stay hidden: evaluate each such table's saved
                // criteria through the live filter, then hide the union once (a later table would
                // unhide what an earlier one excluded) and take the filter off again.
                let others: Vec<(RangeRef, Vec<(u32, FilterCriterion)>)> =
                    sh.tables.iter().filter(|t| !t.filter.is_empty()).map(|t| (t.range, t.filter.clone())).collect();
                let mut excluded: std::collections::HashSet<u32> = std::collections::HashSet::new();
                for (range, criteria) in others {
                    if let Some(sh) = cx.wb.sheet_mut(sheet) {
                        sh.autofilter = Some(AutoFilter { range, criteria });
                    }
                    apply_filter(&mut cx.wb, sheet);
                    if let Some(sh) = cx.wb.sheet(sheet) {
                        excluded.extend((range.start.row.saturating_add(1)..=range.end.row).filter(|r| sh.is_row_hidden(*r)));
                    }
                }
                if let Some(sh) = cx.wb.sheet_mut(sheet) {
                    sh.autofilter = None;
                    for r in excluded {
                        sh.rows.entry(r).or_default().hidden = true;
                    }
                }
                cx.structural = true;
            }
            Ok(Json::Null)
        });
    }
    if has {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            // Reveal only rows the filter hid; rows hidden by hand or by a collapsed group stay hidden.
            let filtered: Vec<u32> = sh.rows.keys().copied().filter(|r| sh.is_row_filtered(*r)).collect();
            sh.autofilter = None;
            for r in filtered {
                if let Some(info) = sh.rows.get_mut(&r) {
                    info.hidden = false;
                }
            }
            cx.structural = true;
            Ok(json!({"filter": false}))
        });
    }
    install_filter(s, p)
}

/// Turn a sheet AutoFilter on over `p.range`, or over the data range of the selection.
///
/// A sheet AutoFilter never overlaps a table, because a file can't hold one (Excel refuses
/// `Range("A1:C8").AutoFilter` over part of a table, and from a cell outside a table it leaves the
/// table's cells out of the region). Inside a table the filter covers the table from its header
/// row, which is saved as the table's own filter, and takes over the criteria the table already
/// had so there is one filter to save.
fn install_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, mut r) = data_target(s, p, "data.filter")?;
    {
        let d = s.doc()?;
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        if p.get("range").is_none() && sh.table_at(d.selection.active).is_none() && d.selection.current().is_single() {
            r = crate::selection::current_region_outside_tables(sh, d.selection.active);
        }
        // A selected header row filters the data below it (#201).
        if p.get("range").is_none() && r.height() == 1 {
            let region = match sh.table_at(r.start) {
                Some(t) => t.data_range().map(|dr| RangeRef::new(t.range.start, dr.end)).unwrap_or(t.range),
                None => crate::selection::current_region_outside_tables(sh, r.start),
            };
            if region.end.row > r.end.row {
                r = RangeRef::new(r.start, CellRef::new(region.end.row, r.end.col));
            }
        }
    }
    let Some(sh) = s.doc()?.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    // The table whose header row the filter starts on, when it lies inside that table.
    let host =
        sh.tables.iter().position(|t| t.header_row && t.range.start.row == r.start.row && t.range.contains(r.start) && t.range.contains(r.end));
    if host.is_none() && sh.tables.iter().any(|t| t.range.intersects(&r)) {
        return Err(bad("data.filter", "a filter range can't overlap a table; filter inside the table instead"));
    }
    // A narrower range would leave out the table's criteria on the columns it doesn't cover, and saving would drop them.
    if let Some(t) = host.and_then(|i| sh.tables.get(i)) {
        if t.data_range().is_some_and(|data| r.end.row < data.end.row) {
            return Err(bad("data.filter", "filter the table's full data range"));
        }
        let shift = r.start.col.saturating_sub(t.range.start.col);
        if t.filter.iter().any(|(o, _)| o.checked_sub(shift).is_none_or(|c| c >= r.width())) {
            return Err(bad("data.filter", "the table is filtered on a column this range leaves out; filter the table's full width"));
        }
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let mut criteria = vec![];
        if let Some(t) = host.and_then(|i| sh.tables.get_mut(i)) {
            // The criteria are saved with the table only while its filter buttons are on.
            t.filter_button = true;
            let shift = r.start.col.saturating_sub(t.range.start.col);
            criteria = std::mem::take(&mut t.filter).into_iter().filter_map(|(o, c)| Some((o.checked_sub(shift)?, c))).collect();
        }
        sh.autofilter = Some(AutoFilter { range: r, criteria });
        Ok(json!({"filter": true, "range": r.a1()}))
    })
}

pub(super) fn filter_by(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    let Some(sh) = s.doc()?.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let af = match &sh.autofilter {
        Some(a) => a.clone(),
        None => {
            install_filter(s, &json!({}))?;
            s.doc()?.wb.sheet(sheet).and_then(|sh| sh.autofilter.clone()).ok_or_else(|| EngineError::Other("no filter".into()))?
        }
    };
    let col = match col_param(p.get("column"), af.range.start.col) {
        Some(c) => c,
        // A number too large to be a column is outside the filter range, not a missing parameter.
        None if p.get("column").is_some_and(|v| v.as_u64().is_some()) => return Err(bad("data.filterBy", "column outside the filter range")),
        None => return Err(bad("data.filterBy", "missing `column`")),
    };
    if col < af.range.start.col || col > af.range.end.col {
        return Err(bad("data.filterBy", "column outside the filter range"));
    }
    let off = col - af.range.start.col;
    let crit = if bool_param(p, "clear").unwrap_or(false) {
        None
    } else if let Some(vals) = p.get("values").and_then(Json::as_array) {
        Some(FilterCriterion::Values {
            values: vals.iter().map(crate::cmd::edit::json_to_input).collect(),
            blanks: bool_param(p, "blanks").unwrap_or(false),
        })
    } else if let Some(c) = p.get("custom") {
        let a =
            (c.get("op").and_then(Json::as_str).unwrap_or("=").to_string(), c.get("value").map(crate::cmd::edit::json_to_input).unwrap_or_default());
        let b =
            c.get("op2").and_then(Json::as_str).map(|o| (o.to_string(), c.get("value2").map(crate::cmd::edit::json_to_input).unwrap_or_default()));
        Some(FilterCriterion::Custom { a, b, and: c.get("and").and_then(Json::as_bool).unwrap_or(true) })
    } else if let Some(t) = p.get("top") {
        Some(FilterCriterion::Top10 {
            bottom: t.get("bottom").and_then(Json::as_bool).unwrap_or(false),
            percent: t.get("percent").and_then(Json::as_bool).unwrap_or(false),
            count: t.get("count").and_then(Json::as_u64).unwrap_or(10) as u32,
        })
    } else {
        bool_param(p, "aboveAverage").map(FilterCriterion::AboveAverage)
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(a) = sh.autofilter.as_mut() {
            a.criteria.retain(|(o, _)| *o != off);
            if let Some(c) = crit.clone() {
                a.criteria.push((off, c));
            }
        }
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })?;
    let hidden = s.doc()?.wb.sheet(sheet).map(|sh| sh.rows.values().filter(|r| r.hidden).count()).unwrap_or(0);
    Ok(json!({"hiddenRows": hidden}))
}

/// Match value-list entries using the same Unicode case rules as custom filters.
fn matches_filter_value(a: &str, b: &str) -> bool {
    gridcraft_core::compare_text(a, b) == Ordering::Equal
}

/// Does a value pass a custom comparison (`>`, `=*abc*`, `<>x`…)?
fn custom_ok(v: &Value, text: &str, op: &str, crit: &str) -> bool {
    let num = gridcraft_core::parse::parse_number_text(crit);
    let cmp = match (v.as_f64(), num) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        // A numeric ordering comparison only matches numbers; blanks and text never do.
        (None, Some(_)) if matches!(op, "<" | "<=" | ">" | ">=") => None,
        _ => Some(gridcraft_core::compare_text(text, crit)),
    };
    let wild = || crate::cmd::edit::wildcard(&text.to_lowercase(), &crit.to_lowercase());
    match op {
        "=" => {
            if crit.contains(['*', '?']) {
                wild()
            } else {
                cmp == Some(Ordering::Equal)
            }
        }
        "<>" => {
            if crit.contains(['*', '?']) {
                !wild()
            } else {
                cmp != Some(Ordering::Equal)
            }
        }
        ">" => cmp == Some(Ordering::Greater),
        "<" => cmp == Some(Ordering::Less),
        ">=" => matches!(cmp, Some(Ordering::Greater | Ordering::Equal)),
        "<=" => matches!(cmp, Some(Ordering::Less | Ordering::Equal)),
        "beginsWith" => text.to_lowercase().starts_with(&crit.to_lowercase()),
        "endsWith" => text.to_lowercase().ends_with(&crit.to_lowercase()),
        "contains" => text.to_lowercase().contains(&crit.to_lowercase()),
        "notContains" => !text.to_lowercase().contains(&crit.to_lowercase()),
        _ => true,
    }
}

/// The filter's "(Blanks)" group: truly empty cells and zero-length text such as a `=""` result.
fn filter_blank(v: &Value) -> bool {
    v.is_empty() || matches!(v, Value::Text(t) if t.is_empty())
}

/// Excel grows an AutoFilter range when a value is entered in the row right below it, in one of its columns (#2342).
/// A table's own filter is left to the table, and a row typed after a blank row does not count.
/// A formula counts before it is calculated: this runs inside the edit, ahead of the recalc.
pub(crate) fn grow_autofilter(sh: &mut Sheet, at: CellRef) {
    // The AutoFilter checks come first: they are cheap, and most sheets have none.
    let Some(af) = sh.autofilter.as_ref() else { return };
    let r = af.range;
    if r.end.row.checked_add(1) != Some(at.row) || at.col < r.start.col || at.col > r.end.col || sh.tables.iter().any(|t| t.range == r) {
        return;
    }
    if !sh.cells.get(at).is_some_and(|c| !filter_blank(&c.value) || c.formula.is_some()) {
        return;
    }
    let grown = RangeRef::new(r.start, CellRef::new(at.row, r.end.col));
    if let Some(af) = sh.autofilter.as_mut() {
        af.range = grown;
    }
}

/// Statistics use the original range within the grid, regardless of the other active criteria.
fn filter_stats(sh: &Sheet, af: &AutoFilter) -> HashMap<usize, (Option<f64>, f64)> {
    let r = af.range;
    // Precompute per-criterion top10 threshold and average once, not per row.
    let mut col_stats = HashMap::new();
    for (i, (off, c)) in af.criteria.iter().enumerate() {
        if matches!(c, FilterCriterion::Top10 { .. } | FilterCriterion::AboveAverage(_)) {
            let Some(col) = r.start.col.checked_add(*off) else { continue };
            // Keep main's imported-range bound after extracting statistics from apply_filter.
            let mut nums: Vec<f64> = (r.start.row.saturating_add(1)..=r.end.row.min(gridcraft_core::MAX_ROWS - 1))
                .filter_map(|row| sh.value(CellRef::new(row, col)).as_f64())
                .collect();
            let avg = if nums.is_empty() { 0.0 } else { nums.iter().sum::<f64>() / nums.len() as f64 };
            let thr = match c {
                FilterCriterion::Top10 { bottom, percent, count } => {
                    nums.sort_by(|a, b| if *bottom { a.partial_cmp(b) } else { b.partial_cmp(a) }.unwrap_or(Ordering::Equal));
                    let k = if *percent { ((nums.len() as f64) * (*count as f64) / 100.0).ceil() as usize } else { *count as usize };
                    let k = k.clamp(1, nums.len().max(1));
                    nums.get(k - 1).copied()
                }
                _ => None,
            };
            col_stats.insert(i, (thr, avg));
        }
    }
    col_stats
}

/// Shared row and checkbox matching, with thresholds and Values lookups prepared once per query.
struct FilterMatcher<'a> {
    wb: &'a Workbook,
    sh: &'a Sheet,
    af: &'a AutoFilter,
    stats: HashMap<usize, (Option<f64>, f64)>,
    /// Selected value strings bucketed by Unicode lowercase for O(1) lookup, verified with `matches_filter_value`.
    values: HashMap<usize, HashMap<String, Vec<String>>>,
}

impl<'a> FilterMatcher<'a> {
    fn new(wb: &'a Workbook, sh: &'a Sheet, af: &'a AutoFilter) -> Self {
        let values = af
            .criteria
            .iter()
            .enumerate()
            .filter_map(|(i, (_, crit))| match crit {
                FilterCriterion::Values { values, .. } => {
                    let mut buckets: HashMap<String, Vec<String>> = HashMap::new();
                    for v in values {
                        buckets.entry(v.to_lowercase()).or_default().push(v.clone());
                    }
                    Some((i, buckets))
                }
                _ => None,
            })
            .collect();
        Self { wb, sh, af, stats: filter_stats(sh, af), values }
    }

    fn row_matches(&self, row: u32, excluded_offset: Option<u32>) -> bool {
        self.af.criteria.iter().enumerate().all(|(i, (off, crit))| Some(*off) == excluded_offset || self.criterion_matches(row, i, *off, crit))
    }

    fn criterion_matches(&self, row: u32, i: usize, off: u32, crit: &FilterCriterion) -> bool {
        let Some(col) = self.af.range.start.col.checked_add(off) else { return true };
        let c = CellRef::new(row, col);
        let v = self.sh.value(c);
        let text = crate::display::cell_text(self.wb, self.sh, c);
        match crit {
            FilterCriterion::Values { blanks, .. } => {
                if filter_blank(&v) {
                    *blanks
                } else {
                    self.values.get(&i).is_some_and(|buckets| {
                        let matches_in = |candidate: &str| {
                            buckets.get(&candidate.to_lowercase()).is_some_and(|list| list.iter().any(|x| matches_filter_value(x, candidate)))
                        };
                        matches_in(&text) || matches_in(&v.display())
                    })
                }
            }
            FilterCriterion::Custom { a, b, and } => {
                let x = custom_ok(&v, &text, &a.0, &a.1);
                match b {
                    Some(b) => {
                        let y = custom_ok(&v, &text, &b.0, &b.1);
                        if *and { x && y } else { x || y }
                    }
                    None => x,
                }
            }
            FilterCriterion::Top10 { bottom, .. } => match (v.as_f64(), self.stats.get(&i)) {
                (Some(n), Some((thr, _))) => {
                    let thr = thr.unwrap_or(n);
                    if *bottom { n <= thr } else { n >= thr }
                }
                _ => false,
            },
            FilterCriterion::AboveAverage(above) => match (v.as_f64(), self.stats.get(&i)) {
                (Some(n), Some((_, avg))) => {
                    if *above {
                        n > *avg
                    } else {
                        n < *avg
                    }
                }
                _ => false,
            },
            FilterCriterion::FillColor(col) => self.wb.styles.get(self.sh.style_id(c)).fill.fg == *col,
            FilterCriterion::FontColor(col) => self.wb.styles.get(self.sh.style_id(c)).font.color == *col,
            // Icon criteria are retained from files but cannot be evaluated yet.
            FilterCriterion::Icon { .. } => true,
        }
    }
}

/// Hides rows of the autofilter range that fail any criterion.
pub(crate) fn apply_filter(wb: &mut Workbook, sheet: usize) {
    let Some(sh) = wb.sheet(sheet) else { return };
    let Some(af) = sh.autofilter.clone() else { return };
    // An imported range can name rows past the grid; none of those exist to hide.
    let r = RangeRef::new(af.range.start, CellRef::new(af.range.end.row.min(gridcraft_core::MAX_ROWS - 1), af.range.end.col));
    let matcher = FilterMatcher::new(wb, sh, &af);
    // Reapplying a retained icon filter preserves visibility until icons can be evaluated.
    let has_icon = af.criteria.iter().any(|(_, c)| matches!(c, FilterCriterion::Icon { .. }));
    let hide: Vec<_> =
        (r.start.row + 1..=r.end.row).map(|row| (row, !matcher.row_matches(row, None) || (has_icon && sh.is_row_hidden(row)))).collect();
    let Some(shm) = wb.sheet_mut(sheet) else { return };
    for (row, h) in hide {
        if h {
            shm.rows.entry(row).or_default().hidden = true;
        } else if let Some(e) = shm.rows.get_mut(&row) {
            e.hidden = false;
        }
    }
}

pub(super) fn clear_filter(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(a) = sh.autofilter.as_mut() {
            a.criteria.clear();
        }
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

pub(super) fn reapply(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    edit(s, |cx| {
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Distinct display values from rows matching the other columns' criteria.
/// The current criterion only determines checkboxes, so its unchecked alternatives stay available.
pub fn filter_values(wb: &Workbook, sheet: usize, col: u32) -> Vec<(String, bool)> {
    column_filter_values(wb, sheet, col, true)
}

/// Applies edited eligible choices to the whole column's checkbox state, retaining choices hidden by other filters.
pub fn filter_selection(wb: &Workbook, sheet: usize, col: u32, choices: &[(String, bool)]) -> Vec<(String, bool)> {
    let mut edited = std::collections::BTreeMap::new();
    for (value, on) in choices {
        *edited.entry(value.to_lowercase()).or_insert(false) |= *on;
    }
    let mut selection = column_filter_values(wb, sheet, col, false);
    for (value, on) in &mut selection {
        if let Some(checked) = edited.get(&value.to_lowercase()) {
            *on = *checked;
        }
    }
    selection
}

fn column_filter_values(wb: &Workbook, sheet: usize, col: u32, eligible_only: bool) -> Vec<(String, bool)> {
    let Some(sh) = wb.sheet(sheet) else { return vec![] };
    let Some(af) = &sh.autofilter else { return vec![] };
    if col < af.range.start.col || col > af.range.end.col {
        return vec![];
    }
    let off = col - af.range.start.col;
    let selected = af.criteria.iter().enumerate().find(|(_, (o, _))| *o == off);
    let matcher = FilterMatcher::new(wb, sh, af);
    let mut seen = std::collections::BTreeMap::new();
    // Imported end rows cannot add choices beyond the grid either.
    for row in
        af.range.start.row.saturating_add(1)..=af.range.end.row.min(af.range.start.row.saturating_add(1_000_000)).min(gridcraft_core::MAX_ROWS - 1)
    {
        if eligible_only && !matcher.row_matches(row, Some(off)) {
            continue;
        }
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let blank = filter_blank(&v);
        let t = if blank { "(Blanks)".to_string() } else { crate::display::cell_text(wb, sh, c) };
        let on = selected.is_none_or(|(i, (off, crit))| matcher.criterion_matches(row, i, *off, crit));
        let key = if blank { sort_key(&Value::Empty) } else { sort_key(&v) };
        // One choice can represent rows with different colors; keep any matching row checked.
        *seen.entry((key, t)).or_insert(false) |= on;
    }
    let mut values: Vec<_> = seen.into_iter().collect();
    values.sort_by(|((a, ta), _), ((b, tb), _)| {
        a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| gridcraft_core::compare_text(&a.2, &b.2)).then_with(|| ta.cmp(tb))
    });
    values.into_iter().map(|((_, t), on)| (t, on)).collect()
}

#[cfg(test)]
mod unicode_filter_tests {
    use super::*;

    fn is_hidden(s: &Session, row: u32) -> bool {
        s.doc().unwrap().wb.active().unwrap().rows.get(&row).is_some_and(|info| info.hidden)
    }

    /// Unicode-equivalent labels stay visible and checked in value-list filters.
    #[test]
    fn unicode_value_filter_keeps_rows_and_dropdown_in_sync() {
        let mut s = Session::new();
        s.new_workbook();

        s.execute(
            "range.setValues",
            json!({
                "range": "A1",
                "values": [
                    ["Name"],
                    ["ÁRBOL"],
                    ["árbol"],
                    ["ÉCOLE"],
                    ["école"],
                    ["Pino"]
                ]
            }),
        )
        .unwrap();

        s.execute("data.filter", json!({"range": "A1:A6"})).unwrap();

        s.execute("data.filterBy", json!({"column": "A", "values": ["árbol"]})).unwrap();

        // Both uppercase and lowercase forms must match.
        for row in [1, 2] {
            assert!(!is_hidden(&s, row), "row {row} must remain visible");
        }
        for row in [3, 4, 5] {
            assert!(is_hidden(&s, row), "row {row} must be hidden");
        }

        // Both displayed variants must show as selected in the dropdown.
        let options = filter_values(&s.doc().unwrap().wb, 0, 0);
        for (label, expected) in [("ÁRBOL", true), ("árbol", true), ("ÉCOLE", false), ("école", false), ("Pino", false)] {
            let selected = options.iter().find(|(text, _)| text == label).map(|(_, on)| *on);

            assert_eq!(selected, Some(expected), "{label}");
        }

        // Another accented word uses the same comparison.
        s.execute("data.filterBy", json!({"column": "A", "values": ["école"]})).unwrap();

        for row in [1, 2, 5] {
            assert!(is_hidden(&s, row), "row {row} must be hidden");
        }
        for row in [3, 4] {
            assert!(!is_hidden(&s, row), "row {row} must remain visible");
        }

        // Custom filters and value-list filters must agree.
        s.execute(
            "data.filterBy",
            json!({
                "column": "A",
                "custom": {"op": "=", "value": "árbol"}
            }),
        )
        .unwrap();

        for row in [1, 2] {
            assert!(!is_hidden(&s, row));
        }
        for row in [3, 4, 5] {
            assert!(is_hidden(&s, row));
        }
    }

    /// Numeric values and the special Blanks group keep their existing behavior.
    #[test]
    fn unicode_value_filter_preserves_numbers_and_blanks() {
        let mut s = Session::new();
        s.new_workbook();

        s.execute(
            "range.setValues",
            json!({
                "range": "A1",
                "values": [
                    ["Value"],
                    [42],
                    [""],
                    ["Other"]
                ]
            }),
        )
        .unwrap();

        s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();

        s.execute(
            "data.filterBy",
            json!({
                "column": "A",
                "values": ["42"],
                "blanks": true
            }),
        )
        .unwrap();

        assert!(!is_hidden(&s, 1), "numeric value must match");
        assert!(!is_hidden(&s, 2), "blank must remain visible");
        assert!(is_hidden(&s, 3), "unselected text must be hidden");

        let options = filter_values(&s.doc().unwrap().wb, 0, 0);

        for (label, expected) in [("42", true), ("(Blanks)", true), ("Other", false)] {
            let selected = options.iter().find(|(text, _)| text == label).map(|(_, on)| *on);

            assert_eq!(selected, Some(expected), "{label}");
        }
    }
}
