//! Value-axis scaling ("nice numbers") and label formatting.

use gridcraft_core::{DateSystem, Value, number_to_text};
use gridcraft_numfmt::{NumberFormat, format_value};

/// Largest magnitude we plot; bigger values are clamped so ranges stay finite.
pub(crate) const MAX_ABS: f64 = 1e300;

/// A linear axis scale: `min..=max` with gridlines every `step`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl Scale {
    /// Tick values from `min` to `max` (at most 51).
    pub fn ticks(&self) -> Vec<f64> {
        let span = self.max - self.min;
        if !(span.is_finite() && self.step.is_finite() && self.step > 0.0 && span > 0.0) {
            return vec![self.min];
        }
        let n = ((span / self.step).round() as i64).clamp(1, 50);
        (0..=n)
            .map(|i| {
                let v = self.min + i as f64 * self.step;
                // Snap away accumulated float error.
                let snapped = (v / self.step).round() * self.step;
                if snapped.abs() < self.step * 1e-9 { 0.0 } else { snapped }
            })
            .collect()
    }
    /// Position of `v` along the axis, 0 at `min`, 1 at `max` (clamped to a sane band).
    pub fn frac(&self, v: f64) -> f32 {
        let span = self.max - self.min;
        if !(span.is_finite() && span > 0.0 && v.is_finite()) {
            return 0.0;
        }
        (((v - self.min) / span).clamp(-0.5, 1.5)) as f32
    }
}

pub(crate) fn clean(v: f64) -> f64 {
    if v.is_finite() { v.clamp(-MAX_ABS, MAX_ABS) } else { 0.0 }
}

/// Picks a "nice" scale (steps of 1, 2 or 5 × 10ⁿ, 4–9 gridlines) covering `lo..=hi`.
///
/// `force_zero` always includes 0 (columns, bars, areas). Otherwise 0 is still included when the
/// data sits reasonably close to it (min below 5/6 of max), like common spreadsheet behaviour.
/// A little headroom is added past the data so the extreme value doesn't touch the frame.
pub fn nice_scale(lo: f64, hi: f64, force_zero: bool) -> Scale {
    let (mut lo, mut hi) = (clean(lo), clean(hi));
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    if force_zero || (lo > 0.0 && lo < hi * 5.0 / 6.0) || lo == hi {
        lo = lo.min(0.0);
    }
    if force_zero || (hi < 0.0 && hi > lo * 5.0 / 6.0) || lo == hi {
        hi = hi.max(0.0);
    }
    if lo == hi {
        // Both zero.
        return Scale { min: 0.0, max: 1.0, step: 0.2 };
    }
    let range = hi - lo;
    if hi > 0.0 {
        hi += range * 0.05;
    }
    if lo < 0.0 {
        lo -= range * 0.05;
    }
    nice_steps(lo, hi)
}

/// Excel's automatic value axis for column, bar, line, area and combo charts, as observed in
/// Excel for Windows (Microsoft 365) with synthetic workbooks (#1688). Column and bar charts are
/// not forced to include 0. When every value is at least ⅚ of the largest one (95…100, or
/// 84…100), the axis leaves 0 out and starts half the data range below the smallest value,
/// rounded down to a step: 95…100 starts at 92, 84…100 at 75, and a stacked bar of start dates
/// 45200… plus a few days at 45180. 83…100 starts at 0. All-negative data mirrors this at the
/// top. `lo`/`hi` are the plotted values (stacked: the stacked positions, not the 0 a stack starts
/// from). The steps are [`nice_steps`]', which can differ from Excel's (95…100: Excel 92–101 by 1).
pub fn value_axis_scale(lo: f64, hi: f64) -> Scale {
    let (mut lo, mut hi) = (clean(lo), clean(hi));
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }
    if lo == hi {
        // One value: include 0, as `nice_scale` does.
        lo = lo.min(0.0);
        hi = hi.max(0.0);
        if lo == hi {
            return Scale { min: 0.0, max: 1.0, step: 0.2 };
        }
    }
    let span = hi - lo;
    let (padded_low, padded_high) = (lo > 0.0 && lo >= hi * 5.0 / 6.0, hi < 0.0 && hi <= lo * 5.0 / 6.0);
    if padded_low {
        lo -= span / 2.0;
    } else if padded_high {
        hi += span / 2.0;
    } else {
        lo = lo.min(0.0);
        hi = hi.max(0.0);
    }
    // 5% headroom past the end away from 0, of the data range when 0 is left out (95…100 → 100.25,
    // so 101) and of the whole axis otherwise (0…100 → 105, so 120), as `nice_scale` adds it.
    let room = if padded_low || padded_high { span } else { hi - lo };
    if hi > 0.0 {
        hi += room * 0.05;
    }
    if lo < 0.0 {
        lo -= room * 0.05;
    }
    nice_steps(lo, hi)
}

/// Steps of 1, 2 or 5 × 10ⁿ giving at most 8 intervals over `lo..=hi`, and the bounds rounded
/// out to them.
fn nice_steps(lo: f64, hi: f64) -> Scale {
    let range = hi - lo;
    if !range.is_finite() || range <= 0.0 {
        return Scale { min: 0.0, max: 1.0, step: 0.2 };
    }
    let e = (range / 8.0).log10().floor();
    let mut chosen = None;
    'outer: for k in [e - 1.0, e, e + 1.0, e + 2.0] {
        let p = 10f64.powf(k);
        for m in [1.0, 2.0, 5.0] {
            let s = m * p;
            if !(s.is_finite() && s > 0.0) {
                continue;
            }
            let intervals = (hi / s - 1e-9).ceil() - (lo / s + 1e-9).floor();
            if intervals <= 8.0 {
                chosen = Some(s);
                break 'outer;
            }
        }
    }
    let step = chosen.unwrap_or(range);
    let mut min = (lo / step + 1e-9).floor() * step;
    let mut max = (hi / step - 1e-9).ceil() * step;
    if min.abs() < step * 1e-9 {
        min = 0.0;
    }
    if max.abs() < step * 1e-9 {
        max = 0.0;
    }
    if !(min.is_finite() && max.is_finite()) || max <= min {
        return Scale { min: lo, max: hi, step: range };
    }
    Scale { min, max, step }
}

/// Exact fixed scale (100% stacked axes).
pub(crate) fn fixed_scale(min: f64, max: f64, step: f64) -> Scale {
    Scale { min, max, step }
}

/// A number format prepared once and reused for many labels.
pub(crate) struct Fmt {
    general: bool,
    fmt: NumberFormat,
}

impl Fmt {
    pub fn new(code: &str) -> Fmt {
        let t = code.trim();
        let general = t.is_empty() || t.eq_ignore_ascii_case("general");
        Fmt { general, fmt: NumberFormat::parse_cached(if general { "General" } else { t }) }
    }
    /// Formats `v`; `step` (axis labels) fixes the decimals for General.
    pub fn format(&self, v: f64, step: Option<f64>) -> String {
        let mut v = clean(v);
        if let Some(s) = step
            && s > 0.0
            && (v / s).abs() < 1e-9
        {
            v = 0.0;
        }
        if self.general {
            return general_text(v, step);
        }
        let mut t = format_value(&Value::Number(v), &self.fmt, DateSystem::D1900).text;
        if t.chars().count() > 40 {
            t = t.chars().take(40).collect();
        }
        t
    }
}

fn general_text(v: f64, step: Option<f64>) -> String {
    if v == 0.0 {
        return "0".into();
    }
    match step {
        Some(s) if s.is_finite() && s > 0.0 && v.abs() < 1e11 && v.abs() >= 1e-9 => {
            let d = (-s.log10().floor()).clamp(0.0, 10.0) as usize;
            let t = format!("{:.*}", d, v);
            if t.trim_start_matches('-').chars().all(|c| c == '0' || c == '.') { "0".into() } else { t }
        }
        _ => number_to_text(v),
    }
}

/// Formats a number with a format code (General shows up to the precision implied by `step`).
pub fn format_number(v: f64, code: &str, step: Option<f64>) -> String {
    Fmt::new(code).format(v, step)
}
