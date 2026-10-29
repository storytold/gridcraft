use std::collections::BTreeSet;

use gridcraft_model::{DateGroupItem, DateGrouping};

use super::sort::sort_key;
use super::{
    AutoFilter, CellRef, EngineError, FilterCriterion, Json, Ordering, RangeRef, Result, Session, Sheet, Value, Workbook, bad, bool_param, col_param,
    current_region, data_target, edit, json,
};

// ---------------------------------------------------------------- filter

pub(super) fn toggle_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = data_target(s, p, "data.filter")?;
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    // The active cell belongs to the active sheet: on another sheet the table is the one at `range`.
    let at = if sheet == s.doc()?.wb.active_sheet() { s.doc()?.selection.active } else { r.start };
    let has = s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.autofilter.is_some());
    let in_table = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.table_at(at).map(|t| t.name.clone()));
    if let Some(name) = in_table {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            if let Some(t) = sh.tables.iter_mut().find(|t| t.name == name) {
                t.filter_button = !t.filter_button;
                if !t.filter_button {
                    t.filter.clear();
                }
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
fn install_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, mut r) = data_target(s, p, "data.filter")?;
    // A selected header row filters the data below it (#201).
    if p.get("range").is_none() && r.height() == 1 {
        let d = s.doc()?;
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        let below = current_region(sh, r.start).end.row;
        if below > r.end.row {
            r = RangeRef::new(r.start, CellRef::new(below, r.end.col));
        }
    }
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.autofilter = Some(AutoFilter { range: r, criteria: vec![] });
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
    // A null `values` or `dates` is the same as leaving it out.
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let crit = if bool_param(p, "clear").unwrap_or(false) {
        None
    } else if given("values").is_some()
        || given("dates").is_some()
        // `blanks: true` on its own keeps the empty cells (it used to clear the filter instead).
        || (bool_param(p, "blanks") == Some(true) && ["custom", "top", "aboveAverage"].iter().all(|k| given(k).is_none()))
    {
        let vals = match given("values") {
            Some(v) => v.as_array().ok_or_else(|| bad("data.filterBy", "`values` must be a list"))?.as_slice(),
            None => &[],
        };
        let dates = match given("dates") {
            Some(d) => {
                d.as_array().ok_or_else(|| bad("data.filterBy", "`dates` must be a list"))?.iter().map(date_item_param).collect::<Result<Vec<_>>>()?
            }
            None => vec![],
        };
        // Values and date groups can share one filter, as Excel saves a text ticked together with a
        // month: a cell passes by either.
        let values: Vec<String> = vals.iter().map(crate::cmd::edit::json_to_input).collect();
        Some(FilterCriterion::Values { values, blanks: bool_param(p, "blanks").unwrap_or(false), dates })
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

/// One `dates` entry of `data.filterBy`: `{year: 2026, month: 3}` is March 2026; the deepest
/// field given is the grouping.
fn date_item_param(j: &Json) -> Result<DateGroupItem> {
    let err = || bad("data.filterBy", "a `dates` entry needs a year and valid month, day, hour, minute and second");
    // A misspelt key (`months`) would otherwise widen the group without a word.
    const KEYS: [&str; 6] = ["year", "month", "day", "hour", "minute", "second"];
    match j.as_object() {
        Some(o) if o.keys().all(|k| KEYS.contains(&k.as_str())) => {}
        _ => return Err(err()),
    }
    // Months and days start at 1; hours, minutes and seconds at 0.
    let field = |name: &str, min: u64, max: u64| -> Result<Option<u32>> {
        match j.get(name) {
            None => Ok(None),
            Some(v) => v.as_u64().filter(|n| (min..=max).contains(n)).map(|n| Some(n as u32)).ok_or_else(err),
        }
    };
    let year = field("year", 0, 9999)?.ok_or_else(err)?;
    let fields = [field("month", 1, 12)?, field("day", 1, 31)?, field("hour", 0, 23)?, field("minute", 0, 59)?, field("second", 0, 59)?];
    // Each field needs the ones above it (a day without a month means nothing).
    let depth = fields.iter().take_while(|f| f.is_some()).count();
    if fields.iter().skip(depth).any(Option::is_some) {
        return Err(err());
    }
    let get = |i: usize| fields.get(i).copied().flatten().unwrap_or(0);
    Ok(DateGroupItem {
        grouping: DateGrouping::ALL.get(depth).copied().unwrap_or(DateGrouping::Second),
        year: year as i32,
        month: get(0),
        day: get(1),
        hour: get(2),
        minute: get(3),
        second: get(4),
    })
}

/// The date and time a cell shows, when it is a number in a format that shows a calendar date.
/// Times and durations (`h:mm`, `[h]:mm`) stay in the plain value list, as in Excel, and so do
/// serials below 1 in the 1900 system (serial 0 is the fictitious 1900-01-00); the 1904 system's
/// serial 0 is the real 1904-01-01. The serial is rounded to the second the cell shows, so a
/// value a hair before midnight is filed under the day it displays.
pub fn cell_date(wb: &Workbook, sh: &Sheet, c: CellRef) -> Option<gridcraft_core::date::DateTime> {
    let n = sh.value(c).as_f64()?;
    let first = match wb.date_system {
        gridcraft_core::DateSystem::D1900 => 1.0,
        gridcraft_core::DateSystem::D1904 => 0.0,
    };
    // `kind` reads the first section, the one a positive number uses: `0.00;yyyy-mm-dd` is no date.
    let fmt = crate::display::number_format(wb.styles.get(sh.style_id(c)).num_fmt.as_str());
    if !n.is_finite() || n < first || fmt.kind() != gridcraft_numfmt::FormatKind::Date {
        return None;
    }
    gridcraft_core::date::datetime_from_serial(wb.date_system, (n * 86_400.0).round() / 86_400.0)
}

/// A criterion's date groups, looked up by each level of a cell's date: at most six lookups per
/// cell however many groups a file lists.
pub(super) struct DateSet(std::collections::HashSet<DateGroupItem>);

impl DateSet {
    pub(super) fn new(items: &[DateGroupItem]) -> DateSet {
        DateSet(items.iter().map(DateGroupItem::normalized).collect())
    }

    pub(super) fn contains(&self, dt: &gridcraft_core::date::DateTime) -> bool {
        DateGrouping::ALL.iter().any(|g| self.0.contains(&DateGroupItem::of(dt, *g)))
    }
}

/// A value-list criterion, prepared once per filter instead of once per row.
struct ValueFilter {
    /// The checked values, ASCII-lowercased (the list matches case-insensitively).
    values: std::collections::HashSet<String>,
    blanks: bool,
    dates: DateSet,
    has_dates: bool,
}

impl ValueFilter {
    fn new(values: &[String], blanks: bool, dates: &[DateGroupItem]) -> ValueFilter {
        ValueFilter {
            values: values.iter().map(|v| v.to_ascii_lowercase()).collect(),
            blanks,
            dates: DateSet::new(dates),
            has_dates: !dates.is_empty(),
        }
    }

    fn of(c: &FilterCriterion) -> Option<ValueFilter> {
        match c {
            FilterCriterion::Values { values, blanks, dates } => Some(ValueFilter::new(values, *blanks, dates)),
            _ => None,
        }
    }

    /// Does a cell pass? `date` gives the cell's [`cell_date`], asked only when needed. With date
    /// groups a date cell passes by its date alone, as the dropdown's tree shows it, and any other
    /// cell by its text (a filter can hold both, as Excel saves them); without date groups a date
    /// passes by the text it shows, so filters that list dates as text (older files, scripts) keep
    /// working. A date's serial number does not count as its text.
    fn pass(&self, v: &Value, text: &str, date: impl Fn() -> Option<gridcraft_core::date::DateTime>) -> bool {
        if filter_blank(v) {
            return self.blanks;
        }
        if self.has_dates
            && let Some(d) = date()
        {
            return self.dates.contains(&d);
        }
        self.values.contains(&text.to_ascii_lowercase()) || (self.values.contains(&v.display().to_ascii_lowercase()) && date().is_none())
    }
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
    if !sh.cells.get(at).is_some_and(|c| !filter_blank(&c.value) || c.formula.is_some()) {
        return;
    }
    let Some(af) = sh.autofilter.as_ref() else { return };
    let r = af.range;
    if r.end.row.checked_add(1) != Some(at.row) || at.col < r.start.col || at.col > r.end.col || sh.tables.iter().any(|t| t.range == r) {
        return;
    }
    let grown = RangeRef::new(r.start, CellRef::new(at.row, r.end.col));
    if let Some(af) = sh.autofilter.as_mut() {
        af.range = grown;
    }
}

/// Hides rows of the autofilter range that fail any criterion.
pub(crate) fn apply_filter(wb: &mut Workbook, sheet: usize) {
    let Some(sh) = wb.sheet(sheet) else { return };
    let Some(af) = sh.autofilter.clone() else { return };
    let r = af.range;
    let mut hide = Vec::new();
    // Precompute per-criterion top10 threshold and average once, not per row.
    let mut col_stats: std::collections::HashMap<usize, (Option<f64>, f64)> = std::collections::HashMap::new();
    for (i, (off, c)) in af.criteria.iter().enumerate() {
        if matches!(c, FilterCriterion::Top10 { .. } | FilterCriterion::AboveAverage(_)) {
            let col = r.start.col + off;
            let mut nums: Vec<f64> = (r.start.row + 1..=r.end.row).filter_map(|row| sh.value(CellRef::new(row, col)).as_f64()).collect();
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
    let value_filters: Vec<Option<ValueFilter>> = af.criteria.iter().map(|(_, c)| ValueFilter::of(c)).collect();
    for row in r.start.row + 1..=r.end.row {
        let mut visible = true;
        for (i, (off, crit)) in af.criteria.iter().enumerate() {
            let c = CellRef::new(row, r.start.col + off);
            let v = sh.value(c);
            let text = crate::display::cell_text(wb, sh, c);
            let ok = match crit {
                FilterCriterion::Values { .. } => match value_filters.get(i).and_then(Option::as_ref) {
                    Some(f) => f.pass(&v, &text, || cell_date(wb, sh, c)),
                    None => true,
                },
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
                FilterCriterion::Top10 { bottom, .. } => match (v.as_f64(), col_stats.get(&i)) {
                    (Some(n), Some((thr, _))) => {
                        let thr = thr.unwrap_or(n);
                        if *bottom { n <= thr } else { n >= thr }
                    }
                    _ => false,
                },
                FilterCriterion::AboveAverage(above) => match (v.as_f64(), col_stats.get(&i)) {
                    (Some(n), Some((_, avg))) => {
                        if *above {
                            n > *avg
                        } else {
                            n < *avg
                        }
                    }
                    _ => false,
                },
                FilterCriterion::FillColor(col) => wb.styles.get(sh.style_id(c)).fill.fg == *col,
                FilterCriterion::FontColor(col) => wb.styles.get(sh.style_id(c)).font.color == *col,
                // Icons are not evaluated here: the row keeps its current visibility.
                FilterCriterion::Icon { .. } => !sh.is_row_hidden(row),
            };
            if !ok {
                visible = false;
                break;
            }
        }
        hide.push((row, !visible));
    }
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
    edit(s, |cx| {
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// A node of the dropdown's date tree: a year, a month of it, a day… down to the seconds when the
/// column has times. Leaves carry the check state; a parent is checked when all its leaves are.
#[derive(Clone, Debug, PartialEq)]
pub struct DateNode {
    pub item: DateGroupItem,
    /// `2026`, `March`, `05`; hours `14`, minutes and seconds `:05`.
    pub label: String,
    /// What the search box matches on a leaf, lowercased: the labels from the year down, then the
    /// cells' display texts.
    pub search: String,
    pub checked: bool,
    pub children: Vec<DateNode>,
    /// Leaves under this node (1 for a leaf).
    pub leaves: usize,
    /// Leaves under this node that the search shows, and how many of those are checked. Set by
    /// [`FilterList::refresh`]; a box is checked when `shown_on == shown`, mixed when between.
    pub shown: usize,
    pub shown_on: usize,
}

impl DateNode {
    /// Does the search (lowercased) show this leaf?
    pub fn matches(&self, needle: &str) -> bool {
        needle.is_empty() || self.search.contains(needle)
    }

    /// Checks or unchecks the leaves under this node that the search shows; call
    /// [`FilterList::refresh`] afterwards.
    pub fn set_shown(&mut self, needle: &str, on: bool) {
        if self.children.is_empty() {
            if self.matches(needle) {
                self.checked = on;
            }
        } else {
            for c in &mut self.children {
                c.set_shown(needle, on);
            }
        }
    }

    fn refresh(&mut self, needle: &str) {
        if self.children.is_empty() {
            let shown = self.matches(needle);
            (self.shown, self.shown_on) = (usize::from(shown), usize::from(shown && self.checked));
        } else {
            (self.shown, self.shown_on) = (0, 0);
            for c in &mut self.children {
                c.refresh(needle);
                self.shown += c.shown;
                self.shown_on += c.shown_on;
            }
        }
    }
}

/// What the filter dropdown lists: the date tree for date cells, and the other values flat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FilterList {
    pub dates: Vec<DateNode>,
    pub values: Vec<(String, bool)>,
}

impl FilterList {
    /// Recounts the date tree's `shown` and `shown_on` for a search (lowercased). One pass over the
    /// tree: run it when the search or a check changes, not on every frame.
    pub fn refresh(&mut self, needle: &str) {
        for n in &mut self.dates {
            n.refresh(needle);
        }
    }
}

fn date_label(item: &DateGroupItem) -> String {
    match item.grouping {
        DateGrouping::Year => item.year.to_string(),
        DateGrouping::Month => {
            gridcraft_core::date::MONTHS.get(item.month.saturating_sub(1) as usize).map_or_else(|| item.month.to_string(), |m| m.to_string())
        }
        DateGrouping::Day => format!("{:02}", item.day),
        DateGrouping::Hour => format!("{:02}", item.hour),
        DateGrouping::Minute => format!(":{:02}", item.minute),
        DateGrouping::Second => format!(":{:02}", item.second),
    }
}

/// Groups sorted leaves into nodes at `depth`, recursing to `leaf`; `path` holds the labels above.
fn date_nodes(leaves: &[(DateGroupItem, BTreeSet<String>, bool)], depth: usize, leaf: usize, path: &str) -> Vec<DateNode> {
    let Some(&grouping) = DateGrouping::ALL.get(depth) else { return vec![] };
    let mut out: Vec<DateNode> = vec![];
    let mut start = 0;
    while let Some((first, ..)) = leaves.get(start) {
        let key = DateGroupItem::of(&date_time_of(first), grouping);
        let len = leaves.iter().skip(start).take_while(|(it, ..)| DateGroupItem::of(&date_time_of(it), grouping) == key).count().max(1);
        let group = leaves.get(start..start + len).unwrap_or_default();
        let label = date_label(&key);
        let path = format!("{path} {label}");
        let node = if depth >= leaf {
            let mut search = path.clone();
            for t in group.iter().flat_map(|(_, t, _)| t.iter()) {
                search.push('\n');
                search.push_str(t);
            }
            let checked = group.iter().any(|(.., on)| *on);
            DateNode {
                item: key,
                label,
                search: search.to_lowercase(),
                checked,
                children: vec![],
                leaves: 1,
                shown: 1,
                shown_on: usize::from(checked),
            }
        } else {
            let children = date_nodes(group, depth + 1, leaf, &path);
            let sum = |f: fn(&DateNode) -> usize| children.iter().map(f).sum::<usize>();
            let (leaves, shown_on) = (sum(|c| c.leaves), sum(|c| c.shown_on));
            DateNode { item: key, label, search: String::new(), checked: false, leaves, shown: leaves, shown_on, children }
        };
        out.push(node);
        start += len;
    }
    out
}

fn date_time_of(it: &DateGroupItem) -> gridcraft_core::date::DateTime {
    gridcraft_core::date::DateTime {
        year: it.year,
        month: it.month,
        day: it.day,
        hour: it.hour,
        minute: it.minute,
        second: it.second,
        ..Default::default()
    }
}

/// The filter dropdown's list for a column: date cells as a Year › Month › Day tree (› Hour ›
/// Minute › Second when any has a time), everything else as a flat list of distinct texts with
/// `(Blanks)` for empty cells and empty text.
pub fn filter_list(wb: &Workbook, sheet: usize, col: u32) -> FilterList {
    let Some(sh) = wb.sheet(sheet) else { return FilterList::default() };
    let Some(af) = &sh.autofilter else { return FilterList::default() };
    let off = col.saturating_sub(af.range.start.col);
    let selected: Option<&FilterCriterion> = af.criteria.iter().find(|(o, _)| *o == off).map(|(_, c)| c);
    let filter = selected.and_then(ValueFilter::of);
    let mut seen = std::collections::BTreeMap::new();
    // Every distinct text of a date stays searchable; a set keeps the inserts cheap however many there are.
    let mut dates: std::collections::BTreeMap<DateGroupItem, (BTreeSet<String>, bool)> = std::collections::BTreeMap::new();
    let mut timed = false;
    for row in af.range.start.row + 1..=af.range.end.row.min(af.range.start.row + 1_000_000) {
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let date = cell_date(wb, sh, c);
        let blank = filter_blank(&v);
        let t = if blank { "(Blanks)".to_string() } else { crate::display::cell_text(wb, sh, c) };
        let on = filter.as_ref().is_none_or(|f| f.pass(&v, &t, || date));
        match date {
            Some(d) => {
                timed |= d.hour != 0 || d.minute != 0 || d.second != 0;
                let e = dates.entry(DateGroupItem::of(&d, DateGrouping::Second)).or_insert_with(|| (BTreeSet::new(), false));
                e.0.insert(t);
                e.1 |= on;
            }
            None => {
                let key = if blank { sort_key(&Value::Empty) } else { sort_key(&v) };
                seen.entry((key, t)).or_insert(on);
            }
        }
    }
    // Leaves are days, or seconds when the column has times.
    let leaf = if timed { 5 } else { 2 };
    let leaf_grouping = DateGrouping::ALL.get(leaf).copied().unwrap_or(DateGrouping::Day);
    let mut leaves: Vec<(DateGroupItem, BTreeSet<String>, bool)> = vec![];
    for (item, (texts, on)) in dates {
        let key = DateGroupItem::of(&date_time_of(&item), leaf_grouping);
        match leaves.last_mut() {
            Some(last) if DateGroupItem::of(&date_time_of(&last.0), leaf_grouping) == key => {
                last.1.extend(texts);
                last.2 |= on;
            }
            _ => leaves.push((key, texts, on)),
        }
    }
    FilterList { dates: date_nodes(&leaves, 0, leaf, ""), values: seen.into_iter().map(|((_, t), on)| (t, on)).collect() }
}

/// The fewest `dates` that select the checked leaves the search shows (counts from
/// [`FilterList::refresh`]): a year whose every leaf is shown and checked is one item, a partly
/// selected one lists its selected months, and so on.
pub fn checked_date_items(nodes: &[DateNode]) -> Vec<DateGroupItem> {
    let mut out = vec![];
    for n in nodes {
        if n.shown_on == n.leaves {
            out.push(n.item);
        } else if n.shown_on > 0 {
            out.extend(checked_date_items(&n.children));
        }
    }
    out
}
