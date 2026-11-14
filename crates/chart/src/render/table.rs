//! The chart data table uses the same primitives as the plot, including in print/PDF output.

use super::frame::{Bx, CatAxis, level_groups, wrap_labels};
use super::legend::{Swatch, cat_label, draw_swatch, plot_kind};
use super::{AXIS, Chart, ChartData, ChartKind, Ctx, Fmt, HAlign, LABEL_SIZE, TEXT, VAlign};

pub(super) struct Layout {
    top: f32,
    row_height: f32,
    headers: usize,
    stub: f32,
    columns: usize,
    /// Rows the category headers take: long names wrap, as in Excel.
    cat_lines: usize,
}

/// Most lines a category header wraps to in a data table (Excel's real tables go to five).
const MAX_HEADER_LINES: usize = 5;

/// The category headers' text.
fn headers_of(data: &ChartData, columns: usize) -> Vec<String> {
    let n = data.categories.len().max(data.series.iter().map(|s| s.values.len()).max().unwrap_or(0));
    (0..columns).map(|j| if j < n { cat_label(data, j) } else { String::new() }).collect()
}

pub(super) fn category_axis<'a>(layout: &Option<Layout>, labels: &'a [String], outer: &'a [Vec<String>]) -> CatAxis<'a> {
    match layout {
        Some(t) => CatAxis::Table(labels, t.stub),
        None => CatAxis::Labels(labels, outer),
    }
}

/// Reserve at most half the available height. A table that cannot fit is explicitly labelled
/// instead of dropping rows or allocating a series × point-sized matrix.
/// `margins` are the widths the axes' labels take on the plot's left and right.
pub(super) fn reserve(ctx: &mut Ctx, chart: &Chart, data: &ChartData, mut area: Bx, columns: usize, margins: (f32, f32)) -> (Bx, Option<Layout>) {
    if chart.data_table.is_none() || !chart.supports_data_table() {
        return (area, None);
    }
    let headers = data.category_levels.len() + 1;
    let row_height = LABEL_SIZE * 1.4 + 4.0;
    let key_width = if chart.data_table.as_ref().is_some_and(|t| t.show_keys) { 22.0 } else { 0.0 };
    let names = data.series.iter().take(32).map(|s| ctx.tw(&s.name, LABEL_SIZE)).fold(0.0f32, f32::max);
    let stub = (names + key_width + 10.0).clamp(24.0, (area.w * 0.3).max(24.0));
    // The plot takes the width beside the stub or the value labels, whichever is wider, as the
    // frame lays it out, so a column is this wide.
    let left = margins.0.min(area.w * 0.4).max(stub);
    let column = (area.w - left - margins.1) / columns.max(1) as f32;
    let mut cat_lines = if columns == 0 || columns > 128 {
        1
    } else {
        wrap_labels(ctx, &headers_of(data, columns), LABEL_SIZE, column - 4.0, MAX_HEADER_LINES)
            .map_or(1, |w| w.iter().map(Vec::len).max().unwrap_or(1))
    };
    // Wrapped headers that would push the table past its share of the height go back to one line
    // (cut short) rather than lose the whole table.
    if row_height * (headers + cat_lines - 1 + data.series.len()) as f32 > area.h * 0.5 {
        cat_lines = 1;
    }
    let rows = headers + cat_lines - 1 + data.series.len();
    let height = row_height * rows as f32;
    if columns == 0 || columns > 128 || data.series.len() > 32 || height > area.h * 0.5 || (area.w * 0.6) / (columns as f32) < 18.0 {
        let message = ctx.fit("Data table needs more space", LABEL_SIZE, area.w);
        ctx.text(area.cx(), area.bottom(), &message, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Bottom));
        area.h -= row_height;
        return (area, None);
    }
    area.h -= height;
    (area, Some(Layout { top: area.bottom(), row_height, headers, stub, columns, cat_lines }))
}

pub(super) fn draw(ctx: &mut Ctx, chart: &Chart, data: &ChartData, plot: Bx, layout: Option<&Layout>) {
    let Some(layout) = layout else { return };
    let Some(settings) = &chart.data_table else { return };
    let x0 = plot.x - layout.stub;
    let top = layout.top;
    // The category headers take `cat_lines` rows; every row after them moves down.
    let extra = layout.cat_lines - 1;
    let rows = layout.headers + extra + data.series.len();
    let bottom = top + layout.row_height * rows as f32;
    let cw = plot.w / layout.columns as f32;
    // Outermost category level first, then the individual categories.
    for (i, level) in data.category_levels.iter().rev().enumerate() {
        for (first, end) in level_groups(level, layout.columns) {
            let text = level.get(first).map(String::as_str).unwrap_or("");
            let text = ctx.fit(text, LABEL_SIZE, (end - first) as f32 * cw - 4.0);
            let x = plot.x + (first + end) as f32 * cw / 2.0;
            ctx.text(x, top + (i as f32 + 0.5) * layout.row_height, &text, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Middle));
        }
    }
    let first_header = top + (layout.headers - 1) as f32 * layout.row_height;
    let labels = headers_of(data, layout.columns);
    let wrapped = if layout.cat_lines > 1 { wrap_labels(ctx, &labels, LABEL_SIZE, cw - 4.0, layout.cat_lines) } else { None };
    for (j, text) in labels.iter().enumerate() {
        let lines = match &wrapped {
            Some(w) => w[j].clone(),
            None => vec![ctx.fit(text, LABEL_SIZE, cw - 4.0)],
        };
        // Centred in the header block, whatever number of lines this one has.
        let block = layout.cat_lines as f32 * layout.row_height;
        let start = first_header + (block - lines.len() as f32 * layout.row_height) / 2.0;
        for (r, line) in lines.iter().enumerate() {
            let line = ctx.fit(line, LABEL_SIZE, cw - 4.0);
            let y = start + (r as f32 + 0.5) * layout.row_height;
            ctx.text(plot.x + (j as f32 + 0.5) * cw, y, &line, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Middle));
        }
    }
    for (i, series) in data.series.iter().enumerate() {
        let y = top + ((layout.headers + extra) as f32 + i as f32 + 0.5) * layout.row_height;
        let name_x = if settings.show_keys { x0 + 25.0 } else { x0 + 4.0 };
        if settings.show_keys {
            let swatch = match plot_kind(chart, series, i) {
                ChartKind::Line | ChartKind::LineStacked => Swatch::Line,
                ChartKind::LineMarkers => Swatch::LineMarker,
                _ => Swatch::Box,
            };
            draw_swatch(ctx, swatch, x0 + 4.0, y, 7.0, series.color);
        }
        let name = ctx.fit(&series.name, LABEL_SIZE, plot.x - name_x - 4.0);
        ctx.text(name_x, y, &name, LABEL_SIZE, TEXT, (HAlign::Left, VAlign::Middle));
        let fmt = Fmt::new(&series.number_format);
        for j in 0..layout.columns {
            let value = series.values.get(j).copied().flatten().map(|v| fmt.format(v, None)).unwrap_or_default();
            let text = ctx.fit(&value, LABEL_SIZE, cw - 4.0);
            ctx.text(plot.x + (j as f32 + 0.5) * cw, y, &text, LABEL_SIZE, TEXT, (HAlign::Center, VAlign::Middle));
        }
    }
    if settings.horizontal_border {
        // No line inside the category headers' block.
        let inside = |row: usize| row >= layout.headers && row < layout.headers + extra;
        for row in (1..rows).filter(|r| !inside(*r)) {
            ctx.hline(x0, plot.right(), top + row as f32 * layout.row_height, AXIS, 0.75);
        }
    }
    if settings.vertical_border {
        ctx.vline(plot.x, top, bottom, AXIS, 0.75);
        // The outer headers span groups; category dividers begin at the innermost header.
        for j in 1..layout.columns {
            ctx.vline(plot.x + j as f32 * cw, top + (layout.headers - 1) as f32 * layout.row_height, bottom, AXIS, 0.75);
        }
    }
    if settings.outline {
        ctx.hline(x0, plot.right(), top, AXIS, 0.75);
        ctx.hline(x0, plot.right(), bottom, AXIS, 0.75);
        ctx.vline(x0, top, bottom, AXIS, 0.75);
        ctx.vline(plot.right(), top, bottom, AXIS, 0.75);
    }
}
