use super::sort::sort_key;
use super::{
    AutoFilter, CellRef, EngineError, FilterCriterion, Json, Ordering, RangeRef, Result, Session, Sheet, Value, Workbook, bad, bool_param, col_param,
    current_region, data_target, edit, json,
};
use crate::cf::CfCache;
use crate::cmd::format::color_param;
use gridcraft_model::{Color, PatternType, Style, Theme};

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
    // Without a filter yet, a table's filter is the table: the one at the active cell, else the
    // sheet's only one. The active cell's region would be wrong when it sits outside the table
    // (a report's title above it), and the column would then be "outside the filter range".
    // The only table is that filter when the column asked for is one of its columns and the active
    // cell isn't in a data region of its own beside the table (a header with rows below it).
    let active = s.doc()?.selection.active;
    let region = current_region(sh, active);
    let sole = if sh.tables.len() == 1 { sh.tables.first() } else { None }.filter(|t| {
        let in_table = col_param(p.get("column"), t.range.start.col).is_some_and(|c| c >= t.range.start.col && c <= t.range.end.col);
        in_table && (region.height() < 2 || region.intersects(&t.range))
    });
    let table = sh.table_at(active).or(sole);
    let table_range = table.map(|t| {
        let r = t.range;
        if t.totals_row && r.end.row > r.start.row { RangeRef::new(r.start, CellRef::new(r.end.row - 1, r.end.col)) } else { r }
    });
    let af = match (&sh.autofilter, table_range) {
        (Some(a), _) => a.clone(),
        (None, Some(r)) => {
            edit(s, |cx| {
                cx.sheet_mut(sheet)?.autofilter = Some(AutoFilter { range: r, criteria: vec![] });
                Ok(Json::Null)
            })?;
            AutoFilter { range: r, criteria: vec![] }
        }
        (None, None) => {
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
    } else if let Some(period) = p.get("period").and_then(Json::as_str) {
        // Date Filters › This Month and the like: the period's days as of today, kept as a fixed
        // range (it does not move on to next month by itself).
        let sys = s.doc()?.wb.date_system;
        let today = gridcraft_core::date::serial_in_system(sys, gridcraft_calc::now_serial());
        let (start, end) = date_period(period, sys, today).ok_or_else(|| bad("data.filterBy", format!("unknown period `{period}`")))?;
        Some(FilterCriterion::Custom { a: (">=".into(), start.to_string()), b: Some(("<".into(), end.to_string())), and: true })
    } else if let Some(c) = p.get("fillColor") {
        Some(FilterCriterion::FillColor(color_param(Some(c)).ok_or_else(|| bad("data.filterBy", "invalid `fillColor`"))?))
    } else if let Some(c) = p.get("fontColor") {
        Some(FilterCriterion::FontColor(color_param(Some(c)).ok_or_else(|| bad("data.filterBy", "invalid `fontColor`"))?))
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

/// A whole-day value (a date without a time) compared with a cell shown as a date compares the
/// cell's day: 31 March at 14:24 equals 31 March and is on or before it, also when the criterion
/// has a second condition or came from a file. `None` leaves the comparison exact: a value with
/// a time, a number that isn't shown as a date, or anything not a number.
fn by_day(v: &Value, crit: &str, shown_as_date: impl Fn() -> bool) -> Option<Value> {
    let n = v.as_f64().filter(|n| n.is_finite())?;
    let day = gridcraft_core::parse::parse_number_text(crit).filter(|d| d.is_finite() && d.fract() == 0.0)?;
    (matches!(v, Value::Number(_)) && day >= 0.0 && shown_as_date()).then(|| Value::Number(n.floor()))
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

/// The periods Date Filters offers, as `data.filterBy`'s `period` names them.
pub const DATE_PERIODS: &[&str] = &[
    "tomorrow",
    "today",
    "yesterday",
    "nextWeek",
    "thisWeek",
    "lastWeek",
    "nextMonth",
    "thisMonth",
    "lastMonth",
    "nextQuarter",
    "thisQuarter",
    "lastQuarter",
    "nextYear",
    "thisYear",
    "lastYear",
    "yearToDate",
];

/// The serials `[start, end)` a date period covers as of `today` (a serial in `sys`): weeks start
/// on Sunday, quarters in January, April, July and October; Year to Date runs to the end of today.
/// `None` for an unknown period or one outside the date system's range.
pub fn date_period(period: &str, sys: gridcraft_core::DateSystem, today: f64) -> Option<(f64, f64)> {
    use gridcraft_core::date::{datetime_from_serial, serial_from_ymd};
    let t = today.floor();
    let dt = datetime_from_serial(sys, t)?;
    let (y, m) = (i64::from(dt.year), i64::from(dt.month));
    let ymd = |y: i64, m: i64| serial_from_ymd(sys, y, m, 1);
    let week = t - f64::from(dt.weekday);
    let q = (m - 1) / 3 * 3 + 1;
    Some(match period {
        "tomorrow" => (t + 1.0, t + 2.0),
        "today" => (t, t + 1.0),
        "yesterday" => (t - 1.0, t),
        "nextWeek" => (week + 7.0, week + 14.0),
        "thisWeek" => (week, week + 7.0),
        "lastWeek" => (week - 7.0, week),
        "nextMonth" => (ymd(y, m + 1)?, ymd(y, m + 2)?),
        "thisMonth" => (ymd(y, m)?, ymd(y, m + 1)?),
        "lastMonth" => (ymd(y, m - 1)?, ymd(y, m)?),
        "nextQuarter" => (ymd(y, q + 3)?, ymd(y, q + 6)?),
        "thisQuarter" => (ymd(y, q)?, ymd(y, q + 3)?),
        "lastQuarter" => (ymd(y, q - 3)?, ymd(y, q)?),
        "nextYear" => (ymd(y + 1, 1)?, ymd(y + 2, 1)?),
        "thisYear" => (ymd(y, 1)?, ymd(y + 1, 1)?),
        "lastYear" => (ymd(y - 1, 1)?, ymd(y, 1)?),
        "yearToDate" => (ymd(y, 1)?, t + 1.0),
        _ => return None,
    })
}

/// The colour a cell's fill shows: `Auto` (no fill) unless it has a pattern.
pub(super) fn fill_color(st: &Style) -> Color {
    if st.fill.pattern == PatternType::None { Color::Auto } else { st.fill.fg }
}

/// The fill and font colour cell `c` shows, as the grid paints it: its own, under a conditional
/// format's fill and font colour where a rule sets them, and a colour scale's fill over both
/// (Filter and Sort by Color, and the colours the filter menu lists).
pub(super) fn shown_colors(wb: &Workbook, sheet: usize, sh: &Sheet, c: CellRef, cf: &mut CfCache) -> (Color, Color) {
    let st = wb.styles.get(sh.style_id(c));
    let (mut fill, mut font) = (fill_color(st), st.font.color);
    if let Some(look) = cf.look(wb, sheet, c) {
        if let Some(rule) = &look.style {
            if rule.fill.pattern != PatternType::None {
                fill = rule.fill.fg;
            }
            if rule.font.color != Color::Auto {
                font = rule.font.color;
            }
        }
        if let Some([r, g, b]) = look.scale_fill {
            fill = Color::rgb(r, g, b);
        }
    }
    (fill, font)
}

/// Two colours look the same: equal, or the same RGB once theme colours are resolved, so a
/// theme accent and the RGB it stands for match (filter and sort by colour).
pub(super) fn same_color(a: &Color, b: &Color, theme: &Theme) -> bool {
    a == b || a.resolve(theme).is_some_and(|x| b.resolve(theme) == Some(x))
}

/// What a column of the autofilter holds, for the filter menu: the kind of data (which decides
/// the sort labels and the Number / Text / Date Filters submenu), its header text, whether it is
/// filtered, and the cell and font colours its data cells use (Sort / Filter by Color).
#[derive(Clone, Debug, PartialEq)]
pub struct FilterColumn {
    pub kind: FilterKind,
    pub header: String,
    pub filtered: bool,
    pub fills: Vec<Color>,
    pub fonts: Vec<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Number,
    Text,
    Date,
}

/// At most this many distinct colours are listed per kind; a column painted in more is unusual
/// and the menu stays short.
const MAX_FILTER_COLORS: usize = 32;

/// Describes column `col` of the active autofilter on `sheet` (see [`FilterColumn`]); `None`
/// without an autofilter or for a column outside it. Text wins when a column holds more text
/// than numbers; numbers shown as dates make it a date column when they are most of its numbers.
pub fn filter_column(wb: &Workbook, sheet: usize, col: u32) -> Option<FilterColumn> {
    let sh = wb.sheet(sheet)?;
    let af = sh.autofilter.as_ref()?;
    if col < af.range.start.col || col > af.range.end.col {
        return None;
    }
    let off = col - af.range.start.col;
    let (mut numbers, mut dates, mut texts) = (0usize, 0usize, 0usize);
    let (mut fills, mut fonts): (Vec<Color>, Vec<Color>) = (vec![], vec![]);
    let add = |list: &mut Vec<Color>, c: Color| {
        if list.len() < MAX_FILTER_COLORS && !list.iter().any(|x| same_color(x, &c, &wb.theme)) {
            list.push(c);
        }
    };
    let last = af.range.end.row.min(af.range.start.row.saturating_add(1_000_000)).min(gridcraft_core::MAX_ROWS - 1);
    let mut cf = CfCache::new();
    for row in af.range.start.row + 1..=last {
        let c = CellRef::new(row, col);
        let st = wb.styles.get(sh.style_id(c));
        let (fill, font) = shown_colors(wb, sheet, sh, c, &mut cf);
        add(&mut fills, fill);
        add(&mut fonts, font);
        match sh.value(c) {
            Value::Number(_) => {
                numbers += 1;
                if crate::display::number_format(st.num_fmt.as_str()).is_date() {
                    dates += 1;
                }
            }
            Value::Text(t) if !t.is_empty() => texts += 1,
            _ => {}
        }
    }
    let kind = if texts > numbers || numbers == 0 {
        FilterKind::Text
    } else if dates * 2 > numbers {
        FilterKind::Date
    } else {
        FilterKind::Number
    };
    let header = crate::display::cell_text(wb, sh, CellRef::new(af.range.start.row, col));
    let filtered = af.criteria.iter().any(|(o, _)| *o == off);
    Some(FilterColumn { kind, header, filtered, fills, fonts })
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
    // An imported range can name rows past the grid; none of those exist to hide.
    let r = RangeRef::new(af.range.start, CellRef::new(af.range.end.row.min(gridcraft_core::MAX_ROWS - 1), af.range.end.col));
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
    let mut cf = CfCache::new();
    for row in r.start.row + 1..=r.end.row {
        let mut visible = true;
        for (i, (off, crit)) in af.criteria.iter().enumerate() {
            let c = CellRef::new(row, r.start.col + off);
            let v = sh.value(c);
            let text = crate::display::cell_text(wb, sh, c);
            let ok = match crit {
                FilterCriterion::Values { values, blanks } => {
                    if filter_blank(&v) {
                        *blanks
                    } else {
                        values.iter().any(|x| x.eq_ignore_ascii_case(&text) || x.eq_ignore_ascii_case(&v.display()))
                    }
                }
                FilterCriterion::Custom { a, b, and } => {
                    let shown_as_date = || crate::display::number_format(wb.styles.get(sh.style_id(c)).num_fmt.as_str()).is_date();
                    let ok = |op: &str, crit: &str| match by_day(&v, crit, shown_as_date) {
                        Some(day) => custom_ok(&day, &text, op, crit),
                        None => custom_ok(&v, &text, op, crit),
                    };
                    let x = ok(&a.0, &a.1);
                    match b {
                        Some(b) => {
                            let y = ok(&b.0, &b.1);
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
                FilterCriterion::FillColor(col) => same_color(&shown_colors(wb, sheet, sh, c, &mut cf).0, col, &wb.theme),
                FilterCriterion::FontColor(col) => same_color(&shown_colors(wb, sheet, sh, c, &mut cf).1, col, &wb.theme),
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

/// Distinct display values of a filter column (for the dropdown list).
pub fn filter_values(wb: &Workbook, sheet: usize, col: u32) -> Vec<(String, bool)> {
    let Some(sh) = wb.sheet(sheet) else { return vec![] };
    let Some(af) = &sh.autofilter else { return vec![] };
    let off = col.saturating_sub(af.range.start.col);
    let selected: Option<&FilterCriterion> = af.criteria.iter().find(|(o, _)| *o == off).map(|(_, c)| c);
    let mut seen = std::collections::BTreeMap::new();
    for row in af.range.start.row + 1..=af.range.end.row.min(af.range.start.row + 1_000_000) {
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let blank = filter_blank(&v);
        let t = if blank { "(Blanks)".to_string() } else { crate::display::cell_text(wb, sh, c) };
        let on = match selected {
            Some(FilterCriterion::Values { values, blanks }) => {
                if blank {
                    *blanks
                } else {
                    values.iter().any(|x| x.eq_ignore_ascii_case(&t))
                }
            }
            _ => true,
        };
        let key = if blank { sort_key(&Value::Empty) } else { sort_key(&v) };
        seen.entry((key, t)).or_insert(on);
    }
    let mut values: Vec<_> = seen.into_iter().collect();
    values.sort_by(|((a, ta), _), ((b, tb), _)| {
        a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then_with(|| gridcraft_core::compare_text(&a.2, &b.2)).then_with(|| ta.cmp(tb))
    });
    values.into_iter().map(|((_, t), on)| (t, on)).collect()
}
