//! Conditional formatting evaluation: what extra style, data bar, colour scale or icon a cell
//! gets.

use std::collections::HashMap;

use gridcraft_core::{CellRef, RangeRef, Value, compare};
use gridcraft_model::{CfOperator, CfRule, CfValueKind, Color, CondFormat, PatternType, Sheet, Style, Underline, Workbook, default_icon_thresholds};

/// The conditional look of one cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CfLook {
    /// Differential style (only non-default parts apply).
    pub style: Option<Style>,
    /// Fill from a colour scale (RGB).
    pub scale_fill: Option<[u8; 3]>,
    /// Data bar geometry and colours.
    pub bar: Option<BarLook>,
    /// Icon: set name and index (0 = lowest).
    pub icon: Option<(String, usize)>,
    pub hide_value: bool,
}

/// One cell's data bar: how the UI paints it. `frac` is signed — the magnitude is the bar's
/// width as a fraction of the drawable width on its side of the axis (honouring the rule's
/// `limits`), the sign the side it grows to; `axis_frac` is where zero falls across the
/// drawable width (0 = left edge, 1 = right).
#[derive(Clone, Debug, PartialEq)]
pub struct BarLook {
    pub frac: f32,
    pub axis_frac: f32,
    pub color: [u8; 3],
    pub gradient: bool,
    pub border: Option<[u8; 3]>,
    pub negative_fill: [u8; 3],
    pub axis: Option<[u8; 3]>,
}

/// Precomputed statistics per rule (min/max/percentiles/top-N thresholds/duplicates).
#[derive(Default)]
pub struct CfCache {
    stats: HashMap<usize, RuleStats>,
}

#[derive(Default, Clone)]
struct RuleStats {
    sorted: Vec<f64>,
    avg: f64,
    std: f64,
    counts: HashMap<String, usize>,
}

/// Which statistics a rule reads. Scanning the rule's whole range for them costs O(range), so
/// rules that read none (cell value, formula, text, blanks, errors, dates) never scan.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum StatsNeed {
    /// Sorted numbers, their mean and deviation (top/bottom N, above average, scales, bars, icons).
    Numbers,
    /// How often each displayed text occurs (duplicate / unique values).
    Counts,
}

fn stats_need(rule: &CfRule) -> Option<StatsNeed> {
    match rule {
        CfRule::Duplicate { .. } | CfRule::Unique { .. } => Some(StatsNeed::Counts),
        CfRule::Top10 { .. } | CfRule::AboveAverage { .. } | CfRule::ColorScale { .. } | CfRule::DataBar { .. } | CfRule::IconSet { .. } => {
            Some(StatsNeed::Numbers)
        }
        _ => None,
    }
}

impl RuleStats {
    fn build(sh: &Sheet, ranges: &[RangeRef], need: StatsNeed) -> RuleStats {
        let (sorted, counts) = numbers_in(sh, ranges, need);
        let n = sorted.len().max(1) as f64;
        let avg = sorted.iter().sum::<f64>() / n;
        let std = (sorted.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / n).sqrt();
        RuleStats { sorted, avg, std, counts }
    }
}

#[cfg(test)]
thread_local! {
    /// Cells `numbers_in` visited on this thread (tests count the work a lookup does).
    static SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn numbers_in(sh: &Sheet, ranges: &[RangeRef], need: StatsNeed) -> (Vec<f64>, HashMap<String, usize>) {
    let mut nums = Vec::new();
    let mut counts = HashMap::new();
    for r in ranges {
        let r = match sh.used_range().and_then(|u| r.intersection(&u)) {
            Some(x) => x,
            None => continue,
        };
        for c in r.iter().take(2_000_000) {
            #[cfg(test)]
            SCANNED.with(|n| n.set(n.get() + 1));
            let v = sh.value(c);
            match need {
                StatsNeed::Numbers => {
                    if let Value::Number(n) = v {
                        nums.push(n);
                    }
                }
                StatsNeed::Counts => {
                    if !v.is_empty() {
                        *counts.entry(v.display().to_lowercase()).or_insert(0) += 1;
                    }
                }
            }
        }
    }
    nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (nums, counts)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p / 100.0).clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let a = sorted.get(lo).copied().unwrap_or(0.0);
    let b = sorted.get(hi).copied().unwrap_or(a);
    a + (b - a) * (rank - lo as f64)
}

/// Where a data bar's axis sits between the bounds: where zero falls, or the edge nearest zero
/// when the bounds do not span it (an all-negative range parks it at the right edge, measured
/// in #1159).
fn axis_position(lo: f64, hi: f64) -> f64 {
    if hi > lo {
        ((0.0 - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else if lo < 0.0 {
        1.0
    } else {
        0.0
    }
}

/// A data bar's share of its side, `n` between the bounds `lo`..`hi`. With bounds that span
/// zero the axis sits at zero and each bar is its value's share of its own side (the side's
/// extreme reaching from the axis to that side's edge). Otherwise the axis parks at the edge
/// nearest zero and the bar is the value's place between the bounds, measured from that edge —
/// Excel's (value − min) / (max − min), clamped, for Lowest/Number/Percent/Percentile/Formula
/// bounds alike; only an Automatic minimum pulls the lower bound to zero (see [`threshold`]).
/// Measured in Excel for Microsoft 365: Lowest–Highest over 10…20 gives 15 → 0.50 and Number
/// 3–8 gives 4 → 0.20 (#1158); Automatic over 10…20 gives 10 → 0.50, and over −10…0 the
/// axis-parked share gives −5 → 0.50 (#1159).
fn bar_share(n: f64, lo: f64, hi: f64) -> f64 {
    if lo < 0.0 && hi > 0.0 {
        if n < 0.0 { (n / lo).clamp(0.0, 1.0) } else { (n / hi).clamp(0.0, 1.0) }
    } else if hi <= 0.0 {
        // All non-positive: the share runs from the upper bound.
        if hi > lo { ((hi - n) / (hi - lo)).clamp(0.0, 1.0) } else { 0.0 }
    } else if hi > lo {
        ((n - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

fn threshold(wb: &Workbook, sheet: usize, at: CellRef, st: &RuleStats, k: &CfValueKind) -> f64 {
    let min = st.sorted.first().copied().unwrap_or(0.0);
    let max = st.sorted.last().copied().unwrap_or(0.0);
    match k {
        // A data bar's Automatic bounds: the minimum is pulled to zero unless the values go
        // lower (the gallery's bar starts at 0, or at the lowest value when that is negative);
        // the maximum is the highest value as-is.
        CfValueKind::AutoMin => min.min(0.0),
        CfValueKind::AutoMax => max,
        CfValueKind::Min => min,
        CfValueKind::Max => max,
        CfValueKind::Number(n) => *n,
        CfValueKind::Percent(p) => min + (max - min) * p / 100.0,
        CfValueKind::Percentile(p) => percentile(&st.sorted, *p),
        CfValueKind::Formula(f) => gridcraft_calc::evaluate(wb, sheet, at, f).to_number().unwrap_or(0.0),
    }
}

fn lerp(a: [u8; 3], b: [u8; 3], t: f64) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [
        (a[0] as f64 + (b[0] as f64 - a[0] as f64) * t).round() as u8,
        (a[1] as f64 + (b[1] as f64 - a[1] as f64) * t).round() as u8,
        (a[2] as f64 + (b[2] as f64 - a[2] as f64) * t).round() as u8,
    ]
}

pub use gridcraft_model::icon_count;

struct RuleEval<'a> {
    wb: &'a Workbook,
    sheet: usize,
    cell: CellRef,
    first: CellRef,
    value: &'a Value,
    text: &'a str,
    stats: &'a RuleStats,
}

fn simple_rule_style<'a>(ctx: &RuleEval<'_>, rule: &'a CfRule) -> Option<&'a Style> {
    let RuleEval { wb, sheet, cell, first, value, text, stats } = ctx;
    match rule {
        CfRule::CellIs { op, a, b, style } => {
            let eval = |f: &str| gridcraft_calc::evaluate(wb, *sheet, *cell, &relative(f, *first, *cell));
            let av = eval(a);
            let matches = match op {
                CfOperator::Between | CfOperator::NotBetween => {
                    let bv = b.as_deref().map(eval).unwrap_or_default();
                    let (lo, hi) = if compare(&av, &bv) == std::cmp::Ordering::Greater { (bv, av) } else { (av, bv) };
                    let within = compare(value, &lo) != std::cmp::Ordering::Less && compare(value, &hi) != std::cmp::Ordering::Greater;
                    if *op == CfOperator::Between { within } else { !within }
                }
                CfOperator::Equal => compare(value, &av) == std::cmp::Ordering::Equal,
                CfOperator::NotEqual => compare(value, &av) != std::cmp::Ordering::Equal,
                CfOperator::Greater => compare(value, &av) == std::cmp::Ordering::Greater,
                CfOperator::Less => compare(value, &av) == std::cmp::Ordering::Less,
                CfOperator::GreaterOrEqual => compare(value, &av) != std::cmp::Ordering::Less,
                CfOperator::LessOrEqual => compare(value, &av) != std::cmp::Ordering::Greater,
            };
            (matches && !value.is_error()).then_some(style.as_ref())
        }
        CfRule::Expression { formula, style } => {
            gridcraft_calc::evaluate(wb, *sheet, *cell, &relative(formula, *first, *cell)).to_bool().unwrap_or(false).then_some(style.as_ref())
        }
        CfRule::ContainsText { text: needle, style } => {
            (!value.is_error() && text.to_lowercase().contains(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::NotContainsText { text: needle, style } => {
            (value.is_error() || !text.to_lowercase().contains(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::BeginsWith { text: needle, style } => {
            (!value.is_error() && text.to_lowercase().starts_with(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::EndsWith { text: needle, style } => {
            (!value.is_error() && text.to_lowercase().ends_with(&needle.to_lowercase())).then_some(style.as_ref())
        }
        CfRule::Blanks { style } => (value.is_empty() || text.trim().is_empty()).then_some(style.as_ref()),
        CfRule::NoBlanks { style } => (!value.is_empty() && !text.trim().is_empty()).then_some(style.as_ref()),
        CfRule::Errors { style } => value.is_error().then_some(style.as_ref()),
        CfRule::NoErrors { style } => (!value.is_error()).then_some(style.as_ref()),
        CfRule::Duplicate { style } => {
            (!value.is_empty() && stats.counts.get(&text.to_lowercase()).copied().unwrap_or(0) > 1).then_some(style.as_ref())
        }
        CfRule::Unique { style } => {
            (!value.is_empty() && stats.counts.get(&text.to_lowercase()).copied().unwrap_or(0) == 1).then_some(style.as_ref())
        }
        _ => None,
    }
}

fn time_period_matches(wb: &Workbook, period: &str, serial: f64) -> bool {
    let today = gridcraft_core::date::serial_in_system(wb.date_system, gridcraft_calc::now_serial()).floor();
    let day = serial.floor();
    let date = |serial| gridcraft_core::date::datetime_from_serial(wb.date_system, serial);
    let week_start = today - date(today).map(|value| value.weekday as f64).unwrap_or(0.0);
    let (year, month) = date(today).map(|value| (value.year, value.month)).unwrap_or((2026, 1));
    let year_month = date(day).map(|value| (value.year, value.month));
    let month_matches = |offset: i32| {
        let shifted_month = month as i32 + offset;
        let shifted_year = year + (shifted_month - 1).div_euclid(12);
        let shifted_month = (shifted_month - 1).rem_euclid(12) + 1;
        year_month == Some((shifted_year, shifted_month as u32))
    };
    match period {
        "yesterday" => day == today - 1.0,
        "today" => day == today,
        "tomorrow" => day == today + 1.0,
        "last7Days" => day > today - 7.0 && day <= today,
        "lastWeek" => day >= week_start - 7.0 && day < week_start,
        "thisWeek" => day >= week_start && day < week_start + 7.0,
        "nextWeek" => day >= week_start + 7.0 && day < week_start + 14.0,
        "lastMonth" => month_matches(-1),
        "thisMonth" => month_matches(0),
        "nextMonth" => month_matches(1),
        _ => false,
    }
}

impl CfCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Conditional look of `c`, or `None` when no rule applies.
    pub fn look(&mut self, wb: &Workbook, sheet: usize, c: CellRef) -> Option<CfLook> {
        let sh = wb.sheet(sheet)?;
        if sh.cond_formats.is_empty() {
            return None;
        }
        let mut out = CfLook::default();
        let mut any = false;
        let mut rules: Vec<(usize, &CondFormat)> =
            sh.cond_formats.iter().enumerate().filter(|(_, cf)| cf.ranges.iter().any(|r| r.contains(c))).collect();
        rules.sort_by_key(|(_, cf)| cf.priority);
        let v = sh.value(c);
        let no_stats = RuleStats::default();
        for (idx, cf) in rules {
            let first = RangeRef::top_left(&cf.ranges).unwrap_or(c);
            let st: &RuleStats = match stats_need(&cf.rule) {
                Some(need) => &*self.stats.entry(idx).or_insert_with(|| RuleStats::build(sh, &cf.ranges, need)),
                None => &no_stats,
            };
            let text = v.display();
            let context = RuleEval { wb, sheet, cell: c, first, value: &v, text: &text, stats: st };
            let matched_style = simple_rule_style(&context, &cf.rule).or_else(|| match &cf.rule {
                CfRule::Top10 { bottom, percent, rank, style } => match v.as_f64() {
                    Some(n) if !st.sorted.is_empty() => {
                        let k = if *percent { ((st.sorted.len() as f64) * (*rank as f64) / 100.0).floor().max(1.0) as usize } else { *rank as usize };
                        let k = k.clamp(1, st.sorted.len());
                        let ok = if *bottom {
                            n <= st.sorted.get(k - 1).copied().unwrap_or(n)
                        } else {
                            n >= st.sorted.get(st.sorted.len() - k).copied().unwrap_or(n)
                        };
                        ok.then_some(style.as_ref())
                    }
                    _ => None,
                },
                CfRule::AboveAverage { below, equal, std_dev, style } => match v.as_f64() {
                    Some(n) => {
                        let thr = st.avg + if *below { -1.0 } else { 1.0 } * st.std * *std_dev as f64;
                        let ok = if *below { n < thr || (*equal && n == thr) } else { n > thr || (*equal && n == thr) };
                        ok.then_some(style.as_ref())
                    }
                    None => None,
                },
                CfRule::TimePeriod { period, style } => match v.as_f64() {
                    Some(n) if time_period_matches(wb, period, n) => Some(style.as_ref()),
                    _ => None,
                },
                CfRule::ColorScale { stops } => {
                    if let Some(n) = v.as_f64() {
                        let pts: Vec<(f64, [u8; 3])> = stops
                            .iter()
                            .map(|(k, col)| (threshold(wb, sheet, c, st, k), col.resolve(&wb.theme).unwrap_or([255, 255, 255])))
                            .collect();
                        let rgb = match pts.as_slice() {
                            [(a, ca), (b, cb)] => lerp(*ca, *cb, if b > a { (n - a) / (b - a) } else { 0.0 }),
                            [(a, ca), (m, cm), (b, cb)] => {
                                if n <= *m {
                                    lerp(*ca, *cm, if m > a { (n - a) / (m - a) } else { 0.0 })
                                } else {
                                    lerp(*cm, *cb, if b > m { (n - m) / (b - m) } else { 1.0 })
                                }
                            }
                            _ => [255, 255, 255],
                        };
                        if out.scale_fill.is_none() {
                            out.scale_fill = Some(rgb);
                            any = true;
                        }
                    }
                    None
                }
                CfRule::DataBar { min, max, color, gradient, show_value, border, negative_fill, negative_border, axis, limits } => {
                    if let Some(n) = v.as_f64() {
                        let lo = threshold(wb, sheet, c, st, min);
                        let hi = threshold(wb, sheet, c, st, max);
                        // The axis sits where zero falls between the bounds; bounds that don't
                        // span zero park it on the edge nearest zero.
                        let axis_pos = axis_position(lo, hi);
                        // Bar length. With bounds that span zero the axis sits at zero and each
                        // bar is its value's share of its own side (the side's extreme reaching
                        // from the axis to that side's edge). Otherwise the axis parks at the
                        // edge nearest zero and the bar is the value's place between the bounds,
                        // measured from that edge — Excel's (value − min) / (max − min), clamped,
                        // for Lowest/Number/Percent/Percentile/Formula bounds alike; only an
                        // Automatic minimum pulls the lower bound to zero (see `threshold`).
                        // Measured in Excel for Microsoft 365: Lowest–Highest over 10…20 gives
                        // 15 → 0.50 and Number 3–8 gives 4 → 0.20 (#1158); Automatic over 10…20
                        // gives 10 → 0.50 (#1159).
                        // The share is mapped into the rule's length limits, Excel's default
                        // 10/90 for a rule without an x14 extension.
                        let u = bar_share(n, lo, hi);
                        let (min_len, max_len) = limits.map(|(a, b)| (a.min(b) as f64 / 100.0, a.max(b) as f64 / 100.0)).unwrap_or((0.1, 0.9));
                        let mag = min_len + u * (max_len - min_len);
                        let finite = |x: f64| if x.is_finite() { x as f32 } else { 0.0 };
                        let col = color.resolve(&wb.theme).unwrap_or([0x63, 0x8E, 0xC6]);
                        // A negative bar takes its own border when the rule colours one,
                        // falling back to the positive border (Excel couples them by default).
                        let border_col = if n < 0.0 { negative_border.as_ref().or(border.as_ref()) } else { border.as_ref() };
                        if out.bar.is_none() {
                            out.bar = Some(BarLook {
                                frac: finite(if n < 0.0 { -mag } else { mag }),
                                axis_frac: finite(axis_pos),
                                color: col,
                                gradient: *gradient,
                                border: border_col.and_then(|c| c.resolve(&wb.theme)),
                                negative_fill: negative_fill
                                    .as_ref()
                                    .and_then(|c| c.resolve(&wb.theme))
                                    // No explicit fill (and `negativeBarColorSameAsPositive` was
                                    // resolved at read time): Excel's default red.
                                    .unwrap_or([255, 0, 0]),
                                axis: axis.as_ref().and_then(|c| c.resolve(&wb.theme)),
                            });
                            out.hide_value |= !show_value;
                            any = true;
                        }
                    }
                    None
                }
                CfRule::IconSet { set, thresholds, reverse, show_value, icons } => {
                    if let Some(n) = v.as_f64() {
                        let k = icon_count(set);
                        // One threshold per icon, the first being the lower bound (as in the file);
                        // also accept the lower bound left out, else Excel's defaults.
                        let defaults;
                        let lower = match thresholds.len() {
                            n if n == k => thresholds.get(1..).unwrap_or_default(),
                            n if n + 1 == k => thresholds.as_slice(),
                            _ => {
                                defaults = default_icon_thresholds(set);
                                defaults.get(1..).unwrap_or_default()
                            }
                        };
                        let cuts: Vec<f64> = lower.iter().map(|t| threshold(wb, sheet, c, st, t)).collect();
                        let mut idx = cuts.iter().filter(|cut| n >= **cut).count();
                        if *reverse {
                            idx = k - 1 - idx.min(k - 1);
                        }
                        if out.icon.is_none() {
                            // A custom icon for this slot (x14:cfIcon), which may come from
                            // another set; an unnamed choice falls back to the rule's own.
                            let icon = match icons.as_ref().and_then(|list| list.get(idx)) {
                                Some(choice) => (if choice.set.is_empty() { set.clone() } else { choice.set.clone() }, choice.id as usize),
                                None => (set.clone(), idx.min(k - 1)),
                            };
                            out.icon = Some(icon);
                            out.hide_value |= !show_value;
                            any = true;
                        }
                    }
                    None
                }
                _ => None,
            });
            if let Some(st) = matched_style {
                any = true;
                // Every matching rule applies; where two set the same part, the rule with the
                // higher priority (seen first) wins.
                match &mut out.style {
                    None => out.style = Some(st.clone()),
                    Some(prev) => layer_under(prev, st),
                }
                if cf.stop_if_true {
                    break;
                }
            }
        }
        any.then_some(out)
    }
}

/// Adds the parts `lower` sets that `style` leaves unset (differential styles: default parts
/// are unset).
fn layer_under(style: &mut Style, lower: &Style) {
    if style.fill.pattern == PatternType::None {
        style.fill = lower.fill;
    }
    if style.font.color == Color::Auto {
        style.font.color = lower.font.color;
    }
    style.font.bold |= lower.font.bold;
    style.font.italic |= lower.font.italic;
    style.font.strike |= lower.font.strike;
    if style.font.underline == Underline::None {
        style.font.underline = lower.font.underline;
    }
    let b = &mut style.borders;
    for (dst, src) in [
        (&mut b.left, lower.borders.left),
        (&mut b.right, lower.borders.right),
        (&mut b.top, lower.borders.top),
        (&mut b.bottom, lower.borders.bottom),
        (&mut b.diag_down, lower.borders.diag_down),
        (&mut b.diag_up, lower.borders.diag_up),
    ] {
        if dst.is_none() {
            *dst = src;
        }
    }
    if style.num_fmt.as_str() == "General" {
        style.num_fmt = lower.num_fmt.clone();
    }
}

/// Shifts a rule formula written for the top-left cell of the range to `c`.
fn relative(f: &str, first: CellRef, c: CellRef) -> String {
    if first == c {
        return f.to_string();
    }
    match gridcraft_formula::parse(f.trim_start_matches('=')) {
        Ok(e) => {
            gridcraft_formula::print(&gridcraft_formula::adjust::shift_relative(e, c.row as i64 - first.row as i64, c.col as i64 - first.col as i64))
        }
        Err(_) => f.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_model::IconChoice;
    use serde_json::json;

    use super::*;

    #[test]
    fn every_matching_rule_applies() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[200], [400], [510]]})).unwrap();
        let rule = |op: &str, value: &str, preset: &str| json!({"range": "A1:A3", "rule": {"type": "cellIs", "operator": op, "value": value, "preset": preset}});
        s.execute("home.conditionalFormat", rule("greaterThan", "300", "lightRedFill")).unwrap();
        s.execute("home.conditionalFormat", rule("greaterThan", "500", "redBorder")).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style);
        assert!(look("A1").is_none());
        let only_fill = look("A2").unwrap();
        assert_eq!(only_fill.fill.pattern, PatternType::Solid);
        assert!(only_fill.borders.top.is_none());
        let both = look("A3").unwrap();
        assert_eq!(both.fill, only_fill.fill);
        assert!(!both.borders.top.is_none());
    }

    #[test]
    fn cell_value_rules_on_blanks_and_errors() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3, "=NA()"], [null, 7], ["=NA()", null], [" ", null], [7, null]]})).unwrap();
        let rule = |range: &str, r: serde_json::Value| json!({"range": range, "rule": r});
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "cellIs", "operator": "less", "value": "5", "preset": "lightRedFill"})))
            .unwrap();
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "noBlanks", "preset": "redBorder"}))).unwrap();
        s.execute("home.conditionalFormat", rule("B1:B2", json!({"type": "cellIs", "operator": "greater", "value": "5", "preset": "lightRedFill"})))
            .unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style);
        let fill = |st: Option<Style>| st.is_some_and(|st| st.fill.pattern == PatternType::Solid);
        let border = |st: Option<Style>| st.is_some_and(|st| !st.borders.top.is_none());
        // Less than 5: 3 and the blank cell (as 0), not #N/A.
        assert_eq!([look("A1"), look("A2"), look("A3"), look("A5")].map(fill), [true, true, false, false]);
        // Greater than 5: 7, not #N/A.
        assert_eq!([look("B1"), look("B2")].map(fill), [false, true]);
        // No blanks: neither the empty cell nor the one holding a space.
        assert_eq!([look("A1"), look("A2"), look("A4")].map(border), [true, false, false]);
    }

    /// The statistics as every rule used to get them: one pass collecting both parts.
    fn all_stats_before(sh: &Sheet, ranges: &[RangeRef]) -> RuleStats {
        let mut nums = Vec::new();
        let mut counts = HashMap::new();
        for r in ranges {
            let Some(r) = sh.used_range().and_then(|u| r.intersection(&u)) else { continue };
            for c in r.iter() {
                let v = sh.value(c);
                if let Value::Number(n) = v {
                    nums.push(n);
                }
                if !v.is_empty() {
                    *counts.entry(v.display().to_lowercase()).or_insert(0) += 1;
                }
            }
        }
        nums.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = nums.len().max(1) as f64;
        let avg = nums.iter().sum::<f64>() / n;
        let std = (nums.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / n).sqrt();
        RuleStats { sorted: nums, avg, std, counts }
    }

    #[test]
    fn needed_statistics_equal_the_full_ones() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3, "x"], [7, "X"], [3, null], ["=NA()", 2.5], [" ", 9], [-1, "y"]]})).unwrap();
        let d = s.doc().unwrap();
        let sh = d.wb.sheet(0).unwrap();
        for ranges in ["A1:B6", "A1:A3", "B2:B9", "D1:E4", "A1:A2"].map(|r| vec![RangeRef::parse(r).unwrap()]) {
            let full = all_stats_before(sh, &ranges);
            let numbers = RuleStats::build(sh, &ranges, StatsNeed::Numbers);
            assert_eq!((numbers.sorted, numbers.avg.to_bits(), numbers.std.to_bits()), (full.sorted, full.avg.to_bits(), full.std.to_bits()));
            assert_eq!(RuleStats::build(sh, &ranges, StatsNeed::Counts).counts, full.counts);
        }
    }

    #[test]
    fn rules_that_read_statistics_still_match() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[1, "a"], [2, "A"], [3, "b"], [10, "c"], [2, null]]})).unwrap();
        let rule = |range: &str, r: serde_json::Value| json!({"range": range, "rule": r});
        s.execute("home.conditionalFormat", rule("B1:B5", json!({"type": "duplicate", "preset": "lightRedFill"}))).unwrap();
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "top10", "rank": 1, "preset": "redBorder"}))).unwrap();
        s.execute("home.conditionalFormat", rule("A1:A5", json!({"type": "colorScale"}))).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap());
        let fill = |l: Option<CfLook>| l.and_then(|l| l.style).is_some_and(|st| st.fill.pattern == PatternType::Solid);
        // Duplicates are case-insensitive texts: "a" and "A".
        assert_eq!(["B1", "B2", "B3", "B4", "B5"].map(|a| fill(look(a))), [true, true, false, false, false]);
        // The top value gets the border, the scale runs from the minimum to the maximum.
        assert!(look("A4").and_then(|l| l.style).is_some_and(|st| !st.borders.top.is_none()));
        assert!(look("A1").and_then(|l| l.style).is_none_or(|st| st.borders.top.is_none()));
        assert_eq!(look("A1").and_then(|l| l.scale_fill), Some([0xF8, 0x69, 0x6B]));
        assert_eq!(look("A4").and_then(|l| l.scale_fill), Some([0x63, 0xBE, 0x7B]));
    }

    /// Looking cells up does work in proportion to the cells asked for, not the rule's range: the
    /// statistics are only gathered for rules that read them, and once per rule.
    #[test]
    fn lookups_do_not_scan_the_range_for_rules_without_statistics() {
        let mut s = crate::Session::new();
        s.new_workbook();
        let rows: Vec<serde_json::Value> = (1..=10_000).map(|i| json!([i, i % 7])).collect();
        s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
        s.execute(
            "home.conditionalFormat",
            json!({"range": "A1:B10000", "rule": {"type": "expression", "formula": "=MOD(ROW(),2)=0", "preset": "lightRedFill"}}),
        )
        .unwrap();
        s.execute(
            "home.conditionalFormat",
            json!({"range": "A1:B10000", "rule": {"type": "cellIs", "operator": "greater", "value": "5", "preset": "redBorder"}}),
        )
        .unwrap();
        s.execute("home.conditionalFormat", json!({"range": "A1:B10000", "rule": {"type": "noBlanks", "preset": "redBorder"}})).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        SCANNED.with(|n| n.set(0));
        for r in 1..=50 {
            for col in ["A", "B"] {
                cache.look(&d.wb, 0, CellRef::parse(&format!("{col}{r}")).unwrap());
            }
        }
        assert_eq!(SCANNED.with(|n| n.get()), 0);
        // A rule that does read them scans its range once, however many cells are looked up.
        s.execute("home.conditionalFormat", json!({"range": "A1:A10000", "rule": {"type": "top10", "preset": "redBorder"}})).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        SCANNED.with(|n| n.set(0));
        for r in 1..=50 {
            cache.look(&d.wb, 0, CellRef::parse(&format!("A{r}")).unwrap());
        }
        assert_eq!(SCANNED.with(|n| n.get()), 10_000);
    }

    #[test]
    fn cell_value_operator_names() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[3], [7]]})).unwrap();
        let rule = |op: &str| json!({"range": "A1:A2", "rule": {"type": "cellIs", "operator": op, "value": "5", "preset": "lightRedFill"}});
        s.execute("home.conditionalFormat", rule("lessThan")).unwrap();
        assert!(s.execute("home.conditionalFormat", rule("smaller")).is_err());
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style).is_some();
        assert_eq!([look("A1"), look("A2")], [true, false]);
    }

    /// Installs `rule` over `range` straight into the model — the x14 extensions have no command
    /// parameters yet, so tests reach the sheet the way `review.rs` does.
    fn add_rule(s: &mut crate::Session, range: &str, rule: CfRule) {
        let i = s.doc().unwrap().wb.active_sheet();
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        std::sync::Arc::make_mut(&mut wb.sheets[i]).cond_formats.push(CondFormat {
            ranges: vec![RangeRef::parse(range).unwrap()],
            rule,
            priority: 1,
            stop_if_true: false,
        });
    }

    /// A min..max data bar in the classic blue; the x14 parts come as arguments.
    fn databar(border: Option<Color>, negative_fill: Option<Color>, negative_border: Option<Color>, limits: Option<(u8, u8)>) -> CfRule {
        CfRule::DataBar {
            min: CfValueKind::Min,
            max: CfValueKind::Max,
            color: Color::rgb(0x63, 0x8E, 0xC6),
            gradient: true,
            show_value: true,
            border,
            negative_fill,
            negative_border,
            axis: None,
            limits,
        }
    }

    #[test]
    fn negative_bars_draw_left_with_negative_fill() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[-5], [1], [10]]})).unwrap();
        s.execute("range.setValues", json!({"range": "B1", "values": [[-5], [1], [10]]})).unwrap();
        add_rule(&mut s, "A1:A3", databar(None, Some(Color::rgb(0xFF, 0x00, 0x00)), None, None));
        // Borders: the rule's colour for positive bars, the negative one for negative bars.
        add_rule(&mut s, "B1:B3", databar(Some(Color::rgb(0x11, 0x22, 0x33)), None, Some(Color::rgb(0x44, 0x55, 0x66)), None));
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut bar = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.bar);
        let a1 = bar("A1").expect("the negative cell has a bar");
        assert!(a1.frac < 0.0, "A1's bar grows left of the axis ({})", a1.frac);
        assert!((a1.frac.abs() - 0.9).abs() < 1e-4, "the most negative value reaches the maximum length on its side ({})", a1.frac);
        assert!(a1.axis_frac > 0.0 && a1.axis_frac < 1.0, "bounds spanning zero put the axis inside ({})", a1.axis_frac);
        assert_eq!(a1.negative_fill, [255, 0, 0], "an explicit negative fill is resolved");
        assert_eq!(a1.color, [0x63, 0x8E, 0xC6]);
        assert_eq!(a1.border, None, "no border colour, no border");
        let a3 = bar("A3").expect("the positive cell has a bar");
        assert!(a3.frac > 0.0, "A3's bar grows right of the axis ({})", a3.frac);
        assert!((a3.frac - 0.9).abs() < 1e-4, "the maximum value reaches the maximum length ({})", a3.frac);
        assert_eq!(a3.color, [0x63, 0x8E, 0xC6]);
        assert_eq!(a1.axis_frac, a3.axis_frac, "one rule, one axis");
        assert_eq!(bar("B1").unwrap().border, Some([0x44, 0x55, 0x66]), "the negative bar borders in negative_border");
        assert_eq!(bar("B3").unwrap().border, Some([0x11, 0x22, 0x33]), "the positive bar borders in border");
        assert_eq!(bar("B1").unwrap().negative_fill, [255, 0, 0], "no explicit negative fill: Excel's default red");
    }

    #[test]
    fn limits_scale_bar_width() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[-5], [1], [10]]})).unwrap();
        add_rule(&mut s, "A1:A3", databar(None, None, None, Some((10, 90))));
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut bar = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.bar);
        let a3 = bar("A3").expect("the max cell has a bar");
        assert!((a3.frac.abs() - 0.9).abs() < 1e-4, "the max value reaches the 90% limit ({})", a3.frac);
        let a2 = bar("A2").expect("the near-zero cell has a bar");
        assert!(a2.frac.abs() >= 0.1, "a near-zero value keeps at least the minimum length ({})", a2.frac);
        assert!((a2.axis_frac - 5.0 / 15.0).abs() < 1e-4, "the axis sits where zero falls ({})", a2.axis_frac);
        assert_eq!(bar("A1").unwrap().negative_fill, [255, 0, 0], "no explicit negative fill: Excel's default red");
    }

    #[test]
    fn degenerate_ranges_have_sane_geometry() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[-10], [-2]]})).unwrap();
        add_rule(&mut s, "A1:A2", databar(None, None, None, None));
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let bars: Vec<_> =
            ["A1", "A2"].iter().map(|a| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.bar).expect("a bar")).collect();
        assert!((bars[0].axis_frac - 1.0).abs() < 1e-4, "all-negative bounds park the axis at the right edge ({})", bars[0].axis_frac);
        for b in &bars {
            assert!(b.frac.is_finite() && b.axis_frac.is_finite(), "no NaN geometry");
            assert!((-1.0..=1.0).contains(&b.frac) && (0.0..=1.0).contains(&b.axis_frac));
            assert!(b.frac <= 0.0, "negative cells stay on the left side ({})", b.frac);
        }
        // One repeated value (hi == lo) keeps the old full-bar fallback, not a NaN.
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[7], [7]]})).unwrap();
        add_rule(&mut s, "A1:A2", databar(None, None, None, None));
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        for a in ["A1", "A2"] {
            let b = cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.bar).expect("a bar");
            assert!(b.frac.is_finite() && b.frac > 0.5 && b.frac <= 1.0, "a repeated value draws a full-length bar ({})", b.frac);
            assert_eq!(b.axis_frac, 0.0);
        }
    }

    #[test]
    fn bar_lengths_match_excel() {
        // Measured in Excel for Microsoft 365 (issues #1158 and #1159), tolerance 0.02: a data
        // bar's length is the value's place between its bounds — (value − min) / (max − min),
        // clamped — mapped into the rule's length limits (0–100 here, as Excel's own x14 rule
        // stores); only an Automatic minimum pulls the lower bound to zero, and bounds spanning
        // zero draw the axis at zero with each side scaled to its own extreme.
        let mut s = crate::Session::new();
        s.new_workbook();
        // A: Number 3–8 and Percent 20–80 over 0…10; B: Lowest–Highest over 10…20;
        // C: Automatic over 10…20; D: Automatic with an axis over −5…5; E: over −10…0.
        s.execute("range.setValues", json!({"range": "A1", "values": [[0],[1],[2],[3],[4],[5],[6],[7],[8],[9],[10]]})).unwrap();
        s.execute("range.setValues", json!({"range": "B1", "values": [[10],[11],[12],[13],[14],[15],[16],[17],[18],[19],[20]]})).unwrap();
        s.execute("range.setValues", json!({"range": "C1", "values": [[10],[11],[12],[13],[14],[15],[16],[17],[18],[19],[20]]})).unwrap();
        s.execute("range.setValues", json!({"range": "D1", "values": [[-5],[-4],[-3],[-2],[-1],[0],[1],[2],[3],[4],[5]]})).unwrap();
        s.execute("range.setValues", json!({"range": "E1", "values": [[-10],[-9],[-8],[-7],[-6],[-5],[-4],[-3],[-2],[-1],[0]]})).unwrap();
        let bar = |min, max| CfRule::DataBar {
            min,
            max,
            color: Color::rgb(0x63, 0x8E, 0xC6),
            gradient: true,
            show_value: true,
            border: None,
            negative_fill: None,
            negative_border: None,
            axis: None,
            limits: Some((0, 100)),
        };
        add_rule(&mut s, "A1:A11", bar(CfValueKind::Number(3.0), CfValueKind::Number(8.0)));
        // Percent 20–80 over 0…10: the percent bounds resolve against the range (2 and 8).
        s.execute("range.setValues", json!({"range": "F1", "values": [[0],[1],[2],[3],[4],[5],[6],[7],[8],[9],[10]]})).unwrap();
        add_rule(&mut s, "F1:F11", bar(CfValueKind::Percent(20.0), CfValueKind::Percent(80.0)));
        add_rule(&mut s, "B1:B11", bar(CfValueKind::Min, CfValueKind::Max));
        add_rule(&mut s, "C1:C11", bar(CfValueKind::AutoMin, CfValueKind::AutoMax));
        add_rule(&mut s, "D1:D11", bar(CfValueKind::AutoMin, CfValueKind::AutoMax));
        add_rule(&mut s, "E1:E11", bar(CfValueKind::AutoMin, CfValueKind::AutoMax));
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.bar);
        let frac = |b: &Option<BarLook>| b.as_ref().map(|b| b.frac);
        let close = |got: Option<f32>, want: f64, what: &str| {
            let got = got.unwrap_or(f32::NAN);
            assert!((f64::from(got) - want).abs() < 0.02, "{what}: {got} vs {want}");
        };
        // Number 3–8: the value's place between the bounds, not its share of the maximum.
        close(frac(&look("A4")), 0.0, "Number 3–8, value 3");
        close(frac(&look("A5")), 0.20, "Number 3–8, value 4");
        close(frac(&look("A7")), 0.60, "Number 3–8, value 6");
        close(frac(&look("A9")), 1.00, "Number 3–8, value 8");
        // Percent 20–80 over 0…10: the bounds are 2 and 8, so 3 sits one sixth of the way.
        close(frac(&look("F4")), 0.17, "Percent 20–80, value 3");
        close(frac(&look("F6")), 0.50, "Percent 20–80, value 5");
        // Lowest–Highest over 10…20: 15 sits half-way, not at 0.75 of the maximum.
        close(frac(&look("B1")), 0.0, "Lowest–Highest, value 10");
        close(frac(&look("B6")), 0.50, "Lowest–Highest, value 15");
        close(frac(&look("B11")), 1.00, "Lowest–Highest, value 20");
        // Automatic over 10…20: the minimum is pulled to zero, so the lowest value is half a bar.
        close(frac(&look("C1")), 0.50, "Automatic, value 10");
        close(frac(&look("C6")), 0.75, "Automatic, value 15");
        close(frac(&look("C11")), 1.00, "Automatic, value 20");
        // Automatic with bounds spanning zero: the axis at the middle, each side its own scale
        // (the reported frac is of its side's half, so −1 reaches from the axis to the left edge).
        let d1 = look("D1").expect("a bar");
        assert!((d1.axis_frac - 0.5).abs() < 0.02, "the axis sits at zero ({})", d1.axis_frac);
        close(frac(&look("D1")), -1.00, "axis, value −5");
        close(frac(&look("D5")), -0.20, "axis, value −1");
        close(frac(&look("D11")), 1.00, "axis, value 5");
        // All non-positive: the axis parks at the right edge and bars grow left from it.
        let e1 = look("E1").expect("a bar");
        assert!((e1.axis_frac - 1.0).abs() < 0.02, "the axis parks at the right edge ({})", e1.axis_frac);
        close(frac(&look("E1")), -1.00, "right-edge axis, value −10");
        close(frac(&look("E6")), -0.50, "right-edge axis, value −5");
    }

    #[test]
    fn custom_icons_resolve_per_slot() {
        let mut s = crate::Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [[-5], [1], [10]]})).unwrap();
        add_rule(
            &mut s,
            "A1:A3",
            CfRule::IconSet {
                set: "3Arrows".into(),
                thresholds: vec![],
                reverse: false,
                show_value: true,
                icons: Some(vec![
                    IconChoice { set: "3Triangles".into(), id: 2 },
                    IconChoice { set: "3Stars".into(), id: 1 },
                    IconChoice { set: "5Boxes".into(), id: 3 },
                ]),
            },
        );
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut icon = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.icon);
        assert_eq!(icon("A1"), Some(("3Triangles".to_string(), 2)), "the lowest slot takes the first custom icon");
        assert_eq!(icon("A2"), Some(("3Stars".to_string(), 1)));
        assert_eq!(icon("A3"), Some(("5Boxes".to_string(), 3)), "the highest slot takes the last custom icon");
    }

    #[test]
    fn text_rules_skip_error_values() {
        let mut s = crate::Session::new();
        s.new_workbook();
        let values = json!([["=NA()", "=NA()", "=1/0", "=NA()"], ["abc", "#x", "x!", "abc"], ["xyz", "y", "y", "xyz"]]);
        s.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
        let rule = |range: &str, kind: &str, text: &str| json!({"range": range, "rule": {"type": kind, "text": text, "preset": "lightRedFill"}});
        s.execute("home.conditionalFormat", rule("A1:A3", "containsText", "a")).unwrap();
        s.execute("home.conditionalFormat", rule("B1:B3", "beginsWith", "#")).unwrap();
        s.execute("home.conditionalFormat", rule("C1:C3", "endsWith", "!")).unwrap();
        s.execute("home.conditionalFormat", rule("D1:D3", "notContainsText", "a")).unwrap();
        let d = s.doc().unwrap();
        let mut cache = CfCache::new();
        let mut look = |a: &str| cache.look(&d.wb, 0, CellRef::parse(a).unwrap()).and_then(|l| l.style).is_some();
        // Contains "a": abc, never #N/A although its text holds an "a".
        assert_eq!([look("A1"), look("A2"), look("A3")], [false, true, false]);
        // Begins with "#": #x, never #N/A.
        assert_eq!([look("B1"), look("B2"), look("B3")], [false, true, false]);
        // Ends with "!": x!, never #DIV/0!.
        assert_eq!([look("C1"), look("C2"), look("C3")], [false, true, false]);
        // Does not contain "a": #N/A and xyz, not abc.
        assert_eq!([look("D1"), look("D2"), look("D3")], [true, false, true]);
    }
}
