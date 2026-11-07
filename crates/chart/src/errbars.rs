//! Error bar lengths (Excel's Error Amount choices) worked out from a series' values.

use gridcraft_model::{ChartKind, ErrorAmount, ErrorBarSides};

use crate::ErrorBarData;

/// One point's error bar in data units along its direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ErrorSpan {
    /// Where the bar is centred: `None` = at the plotted point, `Some(mean)` for standard
    /// deviation bars (Excel centres those on the series mean).
    pub center: Option<f64>,
    /// Length below the centre (0 when the minus side is off).
    pub minus: f64,
    /// Length above the centre (0 when the plus side is off).
    pub plus: f64,
}

/// Whether series drawn as `k` show error bars (Excel offers them on 2-D column, bar, line,
/// area, scatter and bubble charts, stacked or not).
pub fn supports_error_bars(k: ChartKind) -> bool {
    use ChartKind::*;
    matches!(
        k,
        ColumnClustered
            | ColumnStacked
            | BarClustered
            | BarStacked
            | Line
            | LineMarkers
            | LineStacked
            | Area
            | AreaStacked
            | Scatter
            | ScatterLines
            | Bubble
    )
}

/// A non-negative, finite length (hostile file values become 0).
fn len(v: f64) -> f64 {
    if v.is_finite() { v.abs().min(1e300) } else { 0.0 }
}

/// Mean and sample standard deviation of the finite values (`None` for fewer than one value;
/// the deviation is 0 for a single value).
fn stats(vals: &[f64]) -> Option<(f64, f64, usize)> {
    let n = vals.len();
    if n == 0 {
        return None;
    }
    let mean = vals.iter().sum::<f64>() / n as f64;
    let sd = if n > 1 { (vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (n - 1) as f64).sqrt() } else { 0.0 };
    Some((mean, sd, n))
}

/// The error bar of every point of `values` (the series' values along the bar direction: y
/// for Y bars, x for X bars). `None` where the point has no value.
pub fn error_spans(eb: &ErrorBarData, values: &[Option<f64>]) -> Vec<Option<ErrorSpan>> {
    let finite: Vec<f64> = values.iter().flatten().copied().filter(|v| v.is_finite()).collect();
    let st = stats(&finite);
    let (plus_on, minus_on) = match eb.spec.sides {
        ErrorBarSides::Both => (true, true),
        ErrorBarSides::Plus => (true, false),
        ErrorBarSides::Minus => (false, true),
    };
    // A custom list with one value applies to every point.
    let custom = |list: &[Option<f64>], j: usize| -> f64 {
        let v = if list.len() == 1 { list.first() } else { list.get(j) };
        len(v.copied().flatten().unwrap_or(0.0))
    };
    values
        .iter()
        .enumerate()
        .map(|(j, v)| {
            let v = (*v).filter(|v| v.is_finite())?;
            let (center, lo, hi) = match &eb.spec.amount {
                ErrorAmount::Fixed(a) => (None, len(*a), len(*a)),
                ErrorAmount::Percentage(p) => {
                    let a = len(v * len(*p) / 100.0);
                    (None, a, a)
                }
                ErrorAmount::StdDev(m) => {
                    let (mean, sd, _) = st?;
                    let a = len(sd * len(*m));
                    (Some(mean), a, a)
                }
                ErrorAmount::StdErr => {
                    let (_, sd, n) = st?;
                    let a = len(sd / (n as f64).sqrt());
                    (None, a, a)
                }
                ErrorAmount::Custom { .. } => (None, custom(&eb.minus, j), custom(&eb.plus, j)),
            };
            Some(ErrorSpan { center, minus: if minus_on { lo } else { 0.0 }, plus: if plus_on { hi } else { 0.0 } })
        })
        .collect()
}

#[cfg(test)]
mod tests;
