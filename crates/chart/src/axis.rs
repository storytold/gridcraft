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
    pub logarithmic: bool,
    pub reversed: bool,
}

impl Scale {
    /// Tick values from `min` to `max` (at most 51).
    pub fn ticks(&self) -> Vec<f64> {
        if self.logarithmic {
            let first = self.min.log10().ceil() as i32;
            let last = (self.max.log10().floor() as i32).min(first.saturating_add(50));
            let ticks: Vec<_> = (first..=last).map(|e| 10f64.powi(e)).collect();
            return if ticks.is_empty() { vec![self.min, self.max] } else { ticks };
        }
        let span = self.max - self.min;
        if !(span.is_finite() && self.step.is_finite() && self.step > 0.0 && span > 0.0) {
            return vec![self.min];
        }
        let n = ((span / self.step + 1e-9).floor() as i64).clamp(0, 50);
        (0..=n)
            .map(|i| {
                let v = self.min + i as f64 * self.step;
                // Snap away accumulated float error.
                let snapped = v;
                if snapped.abs() < self.step * 1e-9 { 0.0 } else { snapped }
            })
            .collect()
    }
    /// Position of `v` along the axis, 0 at `min`, 1 at `max` (clamped to a sane band).
    pub fn frac(&self, v: f64) -> f32 {
        if self.logarithmic {
            if v <= 0.0 {
                return f32::NAN;
            }
            let f = ((v.log10() - self.min.log10()) / (self.max.log10() - self.min.log10())).clamp(-0.5, 1.5) as f32;
            return if self.reversed { 1.0 - f } else { f };
        }
        let span = self.max - self.min;
        if !(span.is_finite() && span > 0.0 && v.is_finite()) {
            return 0.0;
        }
        let f = (((v - self.min) / span).clamp(-0.5, 1.5)) as f32;
        if self.reversed { 1.0 - f } else { f }
    }

    /// Unclamped position of `v` on the axis. Use before geometric clipping so lines crossing
    /// manually bounded axes are clipped at the actual axis boundary, not at an arbitrary band.
    pub fn frac_raw(&self, v: f64) -> f32 {
        if self.logarithmic {
            if !(v > 0.0 && self.min > 0.0 && self.max > self.min && v.is_finite()) {
                return f32::NAN;
            }
            let f = ((v.log10() - self.min.log10()) / (self.max.log10() - self.min.log10())) as f32;
            return if self.reversed { 1.0 - f } else { f };
        }
        let span = self.max - self.min;
        if !(span.is_finite() && span > 0.0 && v.is_finite()) {
            return f32::NAN;
        }
        let f = ((v - self.min) / span) as f32;
        if self.reversed { 1.0 - f } else { f }
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
        return Scale { min: 0.0, max: 1.0, step: 0.2, logarithmic: false, reversed: false };
    }
    let range = hi - lo;
    if hi > 0.0 {
        hi += range * 0.05;
    }
    if lo < 0.0 {
        lo -= range * 0.05;
    }
    let range = hi - lo;
    if !range.is_finite() || range <= 0.0 {
        return Scale { min: 0.0, max: 1.0, step: 0.2, logarithmic: false, reversed: false };
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
        return Scale { min: lo, max: hi, step: range, logarithmic: false, reversed: false };
    }
    Scale { min, max, step, logarithmic: false, reversed: false }
}

/// Exact fixed scale (100% stacked axes).
pub(crate) fn fixed_scale(min: f64, max: f64, step: f64) -> Scale {
    Scale { min, max, step, logarithmic: false, reversed: false }
}

pub(crate) fn configured_scale(lo: f64, hi: f64, zero: bool, options: &gridcraft_model::ChartAxis) -> Scale {
    let mut scale = nice_scale(lo, hi, zero);
    if options.logarithmic {
        let hi = hi.max(1e-100);
        let lo = if lo > 0.0 { lo } else { hi / 1000.0 };
        scale.min = 10f64.powf(lo.log10().floor());
        scale.max = 10f64.powf(hi.log10().ceil());
        if scale.max <= scale.min {
            scale.max = scale.min * 10.0;
        }
    }
    if let Some(min) = options.minimum.filter(|v| v.is_finite() && (!options.logarithmic || *v > 0.0)) {
        scale.min = min;
    }
    if let Some(max) = options.maximum.filter(|v| v.is_finite() && (!options.logarithmic || *v > 0.0)) {
        scale.max = max;
    }
    if scale.max <= scale.min {
        if options.maximum.is_some() && options.minimum.is_none() {
            scale.min = if options.logarithmic { scale.max / 10.0 } else { scale.max - scale.max.abs().max(1.0) };
        } else {
            scale.max = if options.logarithmic { scale.min * 10.0 } else { scale.min + scale.min.abs().max(1.0) };
        }
    }
    scale.step = options.major_unit.filter(|v| v.is_finite() && *v > 0.0).unwrap_or_else(|| {
        if options.minimum.is_some() || options.maximum.is_some() { nice_scale(scale.min, scale.max, false).step } else { scale.step }
    });
    scale.logarithmic = options.logarithmic;
    scale.reversed = options.reversed;
    scale
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

#[cfg(test)]
mod override_tests {
    use super::*;
    use gridcraft_model::ChartAxis;

    #[test]
    fn manual_limits_units_and_reverse_use_the_same_coordinate_system() {
        let options = ChartAxis { minimum: Some(3.0), maximum: Some(23.0), major_unit: Some(5.0), reversed: true, ..Default::default() };
        let scale = configured_scale(-100.0, 100.0, false, &options);
        assert_eq!(scale.ticks(), vec![3.0, 8.0, 13.0, 18.0, 23.0]);
        assert_eq!(scale.frac(3.0), 1.0);
        assert_eq!(scale.frac(23.0), 0.0);
        assert_eq!(scale.frac_raw(43.0), -1.0);
        assert_eq!(scale.frac_raw(-17.0), 2.0);
    }

    #[test]
    fn raw_logarithmic_fraction_extrapolates_and_rejects_invalid_values() {
        let scale = Scale { min: 1.0, max: 100.0, step: 10.0, logarithmic: true, reversed: false };
        assert_eq!(scale.frac_raw(1000.0), 1.5);
        assert!(scale.frac_raw(0.0).is_nan());
    }

    #[test]
    fn logarithmic_positions_ticks_and_nonpositive_values() {
        let options = ChartAxis { minimum: Some(1.0), maximum: Some(1000.0), logarithmic: true, ..Default::default() };
        let scale = configured_scale(1.0, 1000.0, false, &options);
        assert_eq!(scale.ticks(), vec![1.0, 10.0, 100.0, 1000.0]);
        assert!((scale.frac(10.0) - 1.0 / 3.0).abs() < 1e-6);
        assert!(scale.frac(0.0).is_nan());
        assert!(scale.frac(-1.0).is_nan());
    }
}
