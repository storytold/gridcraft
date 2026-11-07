//! Per-point colours: Format Data Point overrides, vary colors by point and Invert if Negative.
//!
//! Precedence, as Excel draws it: a negative point of a series (or point) with Invert if
//! Negative takes the inverted fill (white, outlined, unless the series has a fill of its own
//! and an inverted colour), whatever its own fill; then the point's own fill; then, with vary
//! colors on and no series colour set, the palette colour of the point; then the series colour.

use crate::{ChartData, MarkerSymbol, PointStyle, Prim, Rgba, SeriesData};

const WHITE: Rgba = [0xFF, 0xFF, 0xFF, 0xFF];
/// Outline of a white inverted bar (GridCraft's own grey; Excel draws a dark outline).
const INVERT_OUTLINE: Rgba = [0x40, 0x40, 0x40, 0xFF];

/// The override for point `j`, if any.
pub(crate) fn point(s: &SeriesData, j: usize) -> Option<&PointStyle> {
    s.points.binary_search_by_key(&j, |p| p.index).ok().and_then(|i| s.points.get(i))
}

/// Whether the lines or markers of the series can differ from point to point.
pub(crate) fn varies(data: &ChartData, s: &SeriesData) -> bool {
    !s.points.is_empty() || (data.vary_colors && !s.explicit_color)
}

/// The colour point `j` gets before its own overrides: the palette colour under vary colors,
/// else the series colour.
pub(crate) fn auto_color(data: &ChartData, s: &SeriesData, j: usize) -> Rgba {
    if data.vary_colors && !s.explicit_color {
        j.checked_rem(data.palette.len()).and_then(|k| data.palette.get(k)).copied().unwrap_or(s.color)
    } else {
        s.color
    }
}

/// Fill and outline of bar, slice or bubble `j` holding `value`.
pub(crate) fn fill(data: &ChartData, s: &SeriesData, j: usize, value: Option<f64>) -> (Rgba, Option<(Rgba, f32)>) {
    let p = point(s, j);
    let invert = p.and_then(|p| p.invert_if_negative).unwrap_or(s.invert_if_negative);
    if invert && value.is_some_and(|v| v < 0.0) {
        let c = s.invert_color.unwrap_or(WHITE);
        return (c, (c == WHITE).then_some((INVERT_OUTLINE, 0.75)));
    }
    let c = p.and_then(|p| p.fill).unwrap_or_else(|| auto_color(data, s, j));
    (c, p.and_then(|p| p.line).map(|l| (l, 1.0)))
}

/// Colour of the line segment that ends at point `j`.
pub(crate) fn line(data: &ChartData, s: &SeriesData, j: usize) -> Rgba {
    point(s, j).and_then(|p| p.line).unwrap_or_else(|| auto_color(data, s, j))
}

/// Fill of marker `j` (its own, else its vary-colors or series colour).
pub(crate) fn marker_fill(data: &ChartData, s: &SeriesData, j: usize) -> Rgba {
    point(s, j).and_then(|p| p.marker_fill).unwrap_or_else(|| auto_color(data, s, j))
}

/// How marker `j` is drawn: `None` when the point hides its marker.
pub(crate) struct Mark {
    symbol: MarkerSymbol,
    r: f32,
    fill: Rgba,
    stroke: (Rgba, f32),
}

pub(crate) fn mark(data: &ChartData, s: &SeriesData, j: usize, r: f32) -> Option<Mark> {
    let p = point(s, j);
    let symbol = p.map(|p| p.marker_symbol).unwrap_or_default();
    if symbol == MarkerSymbol::None {
        return None;
    }
    // A point's line colour also outlines its marker (Format Data Point › Line sets both).
    let line = p.and_then(|p| p.marker_line.or(p.line));
    Some(Mark {
        symbol,
        r: p.and_then(|p| p.marker_size).map(|v| (v / 2.0).clamp(1.0, 36.0)).unwrap_or(r),
        fill: marker_fill(data, s, j),
        stroke: line.map(|c| (c, 1.0)).unwrap_or((WHITE, 0.75)),
    })
}

/// Draws a marker at `c`: circles for automatic and round symbols, polygons and strokes for
/// the others.
pub(crate) fn draw_mark(out: &mut Vec<Prim>, c: [f32; 2], m: &Mark) {
    let [x, y] = c;
    let r = m.r;
    let poly = |pts: Vec<[f32; 2]>| Prim::Polygon { pts, fill: m.fill, stroke: Some(m.stroke) };
    match m.symbol {
        MarkerSymbol::Square => out.push(poly(vec![[x - r, y - r], [x + r, y - r], [x + r, y + r], [x - r, y + r]])),
        MarkerSymbol::Diamond => out.push(poly(vec![[x, y - r * 1.2], [x + r * 1.2, y], [x, y + r * 1.2], [x - r * 1.2, y]])),
        MarkerSymbol::Triangle => out.push(poly(vec![[x, y - r * 1.2], [x + r * 1.1, y + r * 0.8], [x - r * 1.1, y + r * 0.8]])),
        MarkerSymbol::X | MarkerSymbol::Star | MarkerSymbol::Plus => {
            let (dx, dy) = if m.symbol == MarkerSymbol::Plus { (r, 0.0) } else { (r * 0.8, r * 0.8) };
            let color = m.fill;
            out.push(Prim::Line { pts: vec![[x - dx, y - dy], [x + dx, y + dy]], color, width: 1.5, dash: false });
            out.push(Prim::Line { pts: vec![[x - dy, y + dx], [x + dy, y - dx]], color, width: 1.5, dash: false });
            if m.symbol == MarkerSymbol::Star {
                out.push(Prim::Line { pts: vec![[x, y - r], [x, y + r]], color, width: 1.5, dash: false });
            }
        }
        MarkerSymbol::Dash => out.push(Prim::Line { pts: vec![[x - r, y], [x + r, y]], color: m.fill, width: 2.0, dash: false }),
        MarkerSymbol::Dot => out.push(Prim::Circle { cx: x, cy: y, r: (r * 0.5).max(1.0), fill: Some(m.fill), stroke: None }),
        _ => out.push(Prim::Circle { cx: x, cy: y, r, fill: Some(m.fill), stroke: Some(m.stroke) }),
    }
}

#[cfg(test)]
mod tests;
