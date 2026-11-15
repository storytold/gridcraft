use super::{
    AXIS, AXIS_TITLE_SIZE, Chart, ChartData, ChartKind, Ctx, DARK_TEXT, Fmt, GRID, HAlign, LABEL_SIZE, PI, Prim, Rgba, Scale, TEXT, VAlign, WHITE,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Bx {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) w: f32,
    pub(super) h: f32,
}

impl Bx {
    pub(super) fn right(&self) -> f32 {
        self.x + self.w
    }
    pub(super) fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub(super) fn ok(&self) -> bool {
        self.w.is_finite() && self.h.is_finite() && self.w > 4.0 && self.h > 4.0
    }
    pub(super) fn cx(&self) -> f32 {
        self.x + self.w / 2.0
    }
    pub(super) fn cy(&self) -> f32 {
        self.y + self.h / 2.0
    }
}

/// Text colour readable on `fill`.
pub(super) fn on(fill: Rgba) -> Rgba {
    let l = 0.299 * fill[0] as f32 + 0.587 * fill[1] as f32 + 0.114 * fill[2] as f32;
    if l < 150.0 { WHITE } else { DARK_TEXT }
}

pub(super) fn chart_title(chart: &Chart, data: &ChartData) -> Option<String> {
    match &chart.title {
        Some(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
        None if data.series.len() == 1 => data.series.first().map(|s| s.name.clone()).filter(|t| !t.is_empty()),
        None => None,
    }
}

pub(super) fn axis_titles(ctx: &mut Ctx, chart: &Chart, mut area: Bx) -> Bx {
    let lh = ctx.font_size("x_axis_title", AXIS_TITLE_SIZE).max(ctx.font_size("y_axis_title", AXIS_TITLE_SIZE)) * 1.4 + 2.0;
    if let Some(t) = chart.x_title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let t = ctx.fit(t, ctx.font_size("x_axis_title", AXIS_TITLE_SIZE), area.w);
        ctx.text_role("x_axis_title", area.cx(), area.bottom(), &t, AXIS_TITLE_SIZE, TEXT, HAlign::Center, VAlign::Bottom);
        area.h -= lh;
    }
    if let Some(t) = chart.y_title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let t = ctx.fit(t, ctx.font_size("y_axis_title", AXIS_TITLE_SIZE), area.h);
        if !t.is_empty() {
            let format = ctx.format.text_style("y_axis_title");
            ctx.out.push(Prim::Text {
                role: "y_axis_title".into(),
                size: format.size.unwrap_or(AXIS_TITLE_SIZE),
                color: format.color.unwrap_or(TEXT),
                bold: format.bold.unwrap_or(false),
                format,
                x: area.x,
                y: area.cy(),
                text: t,
                align: HAlign::Center,
                valign: VAlign::Top,
                rotation: -PI / 2.0,
            });
        }
        area.x += lh;
        area.w -= lh;
    }
    area
}

// ---------------------------------------------------------------- axes frame

pub(super) enum CatAxis<'a> {
    /// Category labels and, for a multi-level axis, the outer levels (nearest the labels first;
    /// one entry per category, non-empty where a group starts).
    Labels(&'a [String], &'a [Vec<String>]),
    /// A data table supplies the labels below a vertical chart. Its stub must clear the plot.
    Table(&'a [String], f32),
    Numeric(Scale, &'a Fmt),
}

/// Groups of an outer category level: `(first, end)` index pairs, a group starting at every
/// non-empty entry (and at the first category).
pub(super) fn level_groups(level: &[String], n: usize) -> Vec<(usize, usize)> {
    let mut starts: Vec<usize> = (0..n).filter(|&j| j == 0 || level.get(j).is_some_and(|t| !t.is_empty())).collect();
    starts.push(n);
    starts.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))).collect()
}

pub(super) struct ValAxis<'a> {
    pub(super) scale: Scale,
    pub(super) fmt: &'a Fmt,
}

impl ValAxis<'_> {
    fn labels(&self) -> Vec<(f64, String)> {
        self.scale.ticks().into_iter().map(|v| (v, self.fmt.format(v, self.scale.label_step()))).collect()
    }
}

/// Paints the plot fill/border and remembers its rectangle for positioned chart elements.
fn plot_surface(ctx: &mut Ctx, plot: Bx, chart: &Chart) {
    ctx.plot = Some(plot);
    let style = &chart.format.plot_area;
    let width = style.border_width.unwrap_or(if style.border.is_some() { 1.0 } else { 0.0 });
    ctx.out.push(Prim::Rect {
        x: plot.x,
        y: plot.y,
        w: plot.w,
        h: plot.h,
        fill: style.fill,
        stroke: (width > 0.0).then_some((style.border.unwrap_or(GRID), width)),
        radius: 0.0,
    });
}

fn tick(ctx: &mut Ctx, x: f32, y: f32, horizontal_axis: bool, kind: Option<&str>, style: &gridcraft_model::ChartStroke) {
    let color = style.color.unwrap_or(AXIS);
    let width = style.width.unwrap_or(0.75);
    if width <= 0.0 {
        return;
    }
    let (inside, outside) = match kind {
        Some("in") => (4.0, 0.0),
        Some("out") => (0.0, 4.0),
        Some("cross") => (3.0, 3.0),
        _ => return,
    };
    if horizontal_axis {
        ctx.vline(x, y - inside, y + outside, color, width);
    } else {
        ctx.hline(x - outside, x + inside, y, color, width);
    }
}

fn minor_ticks(ctx: &mut Ctx, scale: Scale, axis: &gridcraft_model::ChartAxis, plot: Bx, horizontal: bool) {
    let Some(kind) = axis.minor_tick.as_deref().filter(|v| *v != "none") else {
        return;
    };
    let mut values = Vec::new();
    if scale.logarithmic {
        let start = scale.min.max(f64::MIN_POSITIVE).log10().floor() as i32;
        let end = (scale.max.log10().ceil() as i32).min(start.saturating_add(40));
        for exponent in start..=end {
            for multiplier in 2..10 {
                let v = 10f64.powi(exponent) * multiplier as f64;
                if v > scale.min && v < scale.max {
                    values.push(v);
                }
            }
        }
    } else {
        let step = axis.minor_unit.unwrap_or(scale.step / 5.0);
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        let stride = ((scale.max - scale.min) / step / 250.0).ceil().max(1.0);
        for i in 0..250 {
            let v = scale.min + (1.0 + i as f64 * stride) * step;
            if v >= scale.max {
                break;
            }
            let k = (v - scale.min) / scale.step;
            if v > scale.min && (k - k.round()).abs() > 1e-7 {
                values.push(v);
            }
        }
    }
    for value in values.into_iter().take(250) {
        let f = scale.frac(value);
        let start = ctx.out.len();
        tick(
            ctx,
            if horizontal { plot.x + f * plot.w } else { plot.x },
            if horizontal { plot.bottom() } else { plot.bottom() - f * plot.h },
            horizontal,
            Some(kind),
            &axis.tick_line,
        );
        for primitive in &mut ctx.out[start..] {
            if let Prim::Line { pts, .. } = primitive {
                let center = if horizontal { plot.bottom() } else { plot.x };
                for p in pts {
                    let index = if horizontal { 1 } else { 0 };
                    p[index] = center + (p[index] - center) * 0.6;
                }
            }
        }
    }
}

/// Paints a vertical chart's category labels, group separators and category gridlines.
fn category_labels(ctx: &mut Ctx, chart: &Chart, plot: Bx, (l, outer): (&[String], &[Vec<String>]), reverse_categories: bool) {
    let lsx = ctx.font_size("x_axis", LABEL_SIZE);
    let ls = lsx.max(ctx.font_size("y_axis", LABEL_SIZE));
    let lh = ls * 1.4;
    let gx = chart.format.vertical_gridline.color.unwrap_or(GRID);
    let gwx = chart.format.vertical_gridline.width.unwrap_or(0.75);
    let ax = chart.format.x_axis.line.color.unwrap_or(AXIS);
    let awx = chart.format.x_axis.line.width.unwrap_or(0.75);
    let n = l.len().max(1);
    let cw = plot.w / n as f32;
    for j in (0..n.saturating_sub(1)).step_by(n.div_ceil(250).max(1)) {
        tick(ctx, plot.x + (j + 1) as f32 * cw, plot.bottom(), true, chart.format.x_axis.minor_tick.as_deref(), &chart.format.x_axis.tick_line);
    }
    let lw = l.iter().take(2000).map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max);
    let k = (((lw + 6.0) / cw.max(1e-3)).ceil() as usize).clamp(1, n);
    for (j, t) in l.iter().enumerate().step_by(k) {
        let t = ctx.fit(t, ls, cw * k as f32 - 2.0);
        let j = if reverse_categories { n - 1 - j } else { j };
        tick(ctx, plot.x + (j as f32 + 0.5) * cw, plot.bottom(), true, chart.format.x_axis.major_tick.as_deref(), &chart.format.x_axis.tick_line);
        ctx.text_role("x_axis", plot.x + (j as f32 + 0.5) * cw, plot.bottom() + 3.0, &t, lsx, TEXT, HAlign::Center, VAlign::Top);
    }
    if chart.format.vertical_gridlines {
        for j in (0..n).step_by(k) {
            let j = if reverse_categories { n - 1 - j } else { j };
            ctx.vline(plot.x + (j as f32 + 0.5) * cw, plot.y, plot.bottom(), gx, gwx);
        }
    }
    // Outer levels, one row each below the labels: a group's name centred under its
    // categories, and a separator at each group boundary down through the rows.
    for (i, level) in outer.iter().enumerate() {
        let top = plot.bottom() + 3.0 + lh * (i as f32 + 1.0);
        for (first, end) in level_groups(level, n) {
            let (start, stop) = if reverse_categories { (n - end, n - first) } else { (first, end) };
            let (x0, x1) = (plot.x + start as f32 * cw, plot.x + stop as f32 * cw);
            if let Some(t) = level.get(first).filter(|t| !t.is_empty()) {
                let t = ctx.fit(t, ls, x1 - x0 - 2.0);
                ctx.text_role("x_axis", (x0 + x1) / 2.0, top, &t, lsx, TEXT, HAlign::Center, VAlign::Top);
            }
            if first > 0 {
                let edge = if reverse_categories { x1 } else { x0 };
                ctx.vline(edge, plot.bottom(), top + lh - 2.0, ax, awx);
            }
        }
        let edge = if reverse_categories { plot.x } else { plot.right() };
        ctx.vline(edge, plot.bottom(), top + lh - 2.0, ax, awx);
    }
}

/// Paints the category axis of horizontal bar charts, including multilevel labels.
fn bar_category_labels(ctx: &mut Ctx, chart: &Chart, plot: Bx, labels: &[String], outer: &[Vec<String>], (lw, level_w): (f32, &[f32])) {
    let lsy = ctx.font_size("y_axis", LABEL_SIZE);
    let ls = lsy.max(ctx.font_size("x_axis", LABEL_SIZE));
    let lh = ls * 1.4;
    let ay = chart.format.y_axis.line.color.unwrap_or(AXIS);
    let awy = chart.format.y_axis.line.width.unwrap_or(0.75);
    let gy = chart.format.horizontal_gridline.color.unwrap_or(GRID);
    let gwy = chart.format.horizontal_gridline.width.unwrap_or(0.75);
    let n = labels.len().max(1);
    let ch = plot.h / n as f32;
    for j in (0..n.saturating_sub(1)).step_by(n.div_ceil(250).max(1)) {
        tick(ctx, plot.x, plot.bottom() - (j + 1) as f32 * ch, false, chart.format.y_axis.minor_tick.as_deref(), &chart.format.y_axis.tick_line);
    }
    let k = ((lh / ch.max(1e-3)).ceil() as usize).clamp(1, n);
    for (j, t) in labels.iter().enumerate().step_by(k) {
        let t = ctx.fit(t, ls, lw - 6.0);
        let j = if chart.format.y_axis.reversed { n - 1 - j } else { j };
        tick(ctx, plot.x, plot.bottom() - (j as f32 + 0.5) * ch, false, chart.format.y_axis.major_tick.as_deref(), &chart.format.y_axis.tick_line);
        ctx.text_role("y_axis", plot.x - 4.0, plot.bottom() - (j as f32 + 0.5) * ch, &t, lsy, TEXT, HAlign::Right, VAlign::Middle);
    }
    if chart.format.vertical_gridlines {
        for j in (0..n).step_by(k) {
            let j = if chart.format.y_axis.reversed { n - 1 - j } else { j };
            ctx.hline(plot.x, plot.right(), plot.bottom() - (j as f32 + 0.5) * ch, gy, gwy);
        }
    }
    // Outer levels: a group's name centred beside its categories, separators at the
    // boundaries running right to the axis.
    let mut x1 = plot.x - lw;
    for (level, w) in outer.iter().zip(level_w) {
        let x0 = x1 - w;
        for (first, end) in level_groups(level, n) {
            let (start, stop) = if chart.format.y_axis.reversed { (n - end, n - first) } else { (first, end) };
            let (y0, y1) = (plot.bottom() - stop as f32 * ch, plot.bottom() - start as f32 * ch);
            if let Some(t) = level.get(first).filter(|t| !t.is_empty()) {
                let t = ctx.fit(t, ls, w - 4.0);
                ctx.text_role("y_axis", (x0 + x1) / 2.0, (y0 + y1) / 2.0, &t, lsy, TEXT, HAlign::Center, VAlign::Middle);
            }
            if first > 0 {
                let edge = if chart.format.y_axis.reversed { y0 } else { y1 };
                ctx.hline(x0, plot.x, edge, ay, awy);
            }
        }
        let edge = if chart.format.y_axis.reversed { plot.bottom() } else { plot.y };
        ctx.hline(x0, plot.x, edge, ay, awy);
        x1 = x0;
    }
}

/// Lays out axes, ticks and gridlines using the chart's formatting and returns the plot rectangle.
pub(super) fn frame(ctx: &mut Ctx, area: Bx, cat: CatAxis, y: &ValAxis, y2: Option<&ValAxis>, horizontal: bool, chart: &Chart) -> Bx {
    let reverse_categories =
        chart.format.x_axis.reversed && !matches!(chart.kind, ChartKind::Histogram | ChartKind::Waterfall | ChartKind::BoxWhisker | ChartKind::Stock);
    let grid = chart.gridlines;
    let gx = chart.format.vertical_gridline.color.unwrap_or(GRID);
    let gwx = chart.format.vertical_gridline.width.unwrap_or(0.75);
    let gy = chart.format.horizontal_gridline.color.unwrap_or(GRID);
    let gwy = chart.format.horizontal_gridline.width.unwrap_or(0.75);
    let ax = chart.format.x_axis.line.color.unwrap_or(AXIS);
    let awx = chart.format.x_axis.line.width.unwrap_or(0.75);
    let ay = chart.format.y_axis.line.color.unwrap_or(AXIS);
    let awy = chart.format.y_axis.line.width.unwrap_or(0.75);
    let lsx = ctx.font_size("x_axis", LABEL_SIZE);
    let lsy = ctx.font_size("y_axis", LABEL_SIZE);
    let ls = lsx.max(lsy);
    let lh = ls * 1.4;
    let ylabels = y.labels();
    let stub = match &cat {
        CatAxis::Table(_, width) => *width,
        _ => 0.0,
    };
    let maxw = |ctx: &Ctx, l: &[(f64, String)]| l.iter().map(|(_, t)| ctx.tw(t, ls)).fold(0.0f32, f32::max);
    if !horizontal {
        let left = (maxw(ctx, &ylabels) + 6.0).min(area.w * 0.4).max(stub);
        let y2labels = y2.map(ValAxis::labels);
        let right = match (&y2labels, &cat) {
            (Some(l), _) => (maxw(ctx, l) + 6.0).min(area.w * 0.3),
            (None, CatAxis::Numeric(s, f)) => (ctx.tw(&f.format(s.max, s.label_step()), ls) / 2.0).min(area.w * 0.2),
            _ => 4.0,
        };
        let levels = match &cat {
            CatAxis::Labels(_, o) => o.len() as f32,
            CatAxis::Numeric(..) | CatAxis::Table(..) => 0.0,
        };
        let plot = Bx { x: area.x + left, y: area.y + lh * 0.5, w: area.w - left - right, h: area.h - lh * 1.5 - 4.0 - lh * levels };
        if !plot.ok() {
            return plot;
        }
        plot_surface(ctx, plot, chart);
        minor_ticks(ctx, y.scale, &chart.format.y_axis, plot, false);
        if chart.format.y_axis.line.color.is_some() || chart.format.y_axis.line.width.is_some() {
            ctx.vline(plot.x, plot.y, plot.bottom(), ay, awy);
        }
        for (v, t) in &ylabels {
            let yy = plot.bottom() - y.scale.frac(*v) * plot.h;
            tick(ctx, plot.x, yy, false, chart.format.y_axis.major_tick.as_deref(), &chart.format.y_axis.tick_line);
            if grid {
                ctx.hline(plot.x, plot.right(), yy, gy, gwy);
            }
            ctx.text_role("y_axis", plot.x - 4.0, yy, t, lsy, TEXT, HAlign::Right, VAlign::Middle);
        }
        if let (Some(a), Some(l)) = (y2, &y2labels) {
            for (v, t) in l {
                let yy = plot.bottom() - a.scale.frac(*v) * plot.h;
                ctx.text_role("y_axis", plot.right() + 4.0, yy, t, lsy, TEXT, HAlign::Left, VAlign::Middle);
            }
        }
        let zero = if y.scale.min <= 0.0 && y.scale.max >= 0.0 { 0.0 } else { y.scale.min };
        let y0 = plot.bottom() - y.scale.frac(zero) * plot.h;
        ctx.hline(plot.x, plot.right(), y0, ax, awx);
        match cat {
            CatAxis::Table(..) => {}
            CatAxis::Labels(l, outer) => category_labels(ctx, chart, plot, (l, outer), reverse_categories),
            CatAxis::Numeric(s, f) => {
                minor_ticks(ctx, s, &chart.format.x_axis, plot, true);
                let ticks = s.ticks();
                if chart.format.vertical_gridlines {
                    for v in &ticks {
                        ctx.vline(plot.x + s.frac(*v) * plot.w, plot.y, plot.bottom(), gx, gwx);
                    }
                }
                let labels: Vec<String> = ticks.iter().map(|v| f.format(*v, s.label_step())).collect();
                let lw = labels.iter().map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max);
                let slot = plot.w / ticks.len().max(1) as f32;
                let k = (((lw + 6.0) / slot.max(1e-3)).ceil() as usize).max(1);
                for (v, t) in ticks.iter().zip(&labels).step_by(k) {
                    let xx = plot.x + s.frac(*v) * plot.w;
                    tick(ctx, xx, plot.bottom(), true, chart.format.x_axis.major_tick.as_deref(), &chart.format.x_axis.tick_line);
                    ctx.text_role("x_axis", xx, plot.bottom() + 3.0, t, lsx, TEXT, HAlign::Center, VAlign::Top);
                }
                if s.min <= 0.0 && s.max >= 0.0 {
                    let xx = plot.x + s.frac(0.0) * plot.w;
                    ctx.vline(xx, plot.y, plot.bottom(), ay, awy);
                }
            }
        }
        plot
    } else {
        let (labels, outer): (Vec<String>, &[Vec<String>]) = match cat {
            CatAxis::Labels(l, o) => (l.to_vec(), o),
            CatAxis::Table(l, _) => (l.to_vec(), &[]),
            CatAxis::Numeric(..) => (vec![], &[]),
        };
        let lw = (labels.iter().take(2000).map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max) + 6.0).min(area.w * 0.35);
        // Outer levels stand in columns left of the labels, each as wide as its longest name.
        let level_w: Vec<f32> =
            outer.iter().map(|l| (l.iter().take(2000).map(|t| ctx.tw(t, ls)).fold(0.0f32, f32::max) + 6.0).min(area.w * 0.15)).collect();
        let levels_w: f32 = level_w.iter().sum();
        let right = ylabels.last().map(|(_, t)| ctx.tw(t, ls) / 2.0).unwrap_or(0.0).min(area.w * 0.2) + 2.0;
        let left = (lw + levels_w).max(stub);
        let plot = Bx { x: area.x + left, y: area.y + 2.0, w: area.w - left - right, h: area.h - lh - 6.0 };
        if !plot.ok() {
            return plot;
        }
        plot_surface(ctx, plot, chart);
        minor_ticks(ctx, y.scale, &chart.format.x_axis, plot, true);
        let vw = maxw(ctx, &ylabels);
        let slot = plot.w / ylabels.len().max(1) as f32;
        let k = (((vw + 6.0) / slot.max(1e-3)).ceil() as usize).max(1);
        for (i, (v, t)) in ylabels.iter().enumerate() {
            let xx = plot.x + y.scale.frac(*v) * plot.w;
            tick(ctx, xx, plot.bottom(), true, chart.format.x_axis.major_tick.as_deref(), &chart.format.x_axis.tick_line);
            if grid {
                ctx.vline(xx, plot.y, plot.bottom(), gx, gwx);
            }
            if i % k == 0 {
                ctx.text_role("x_axis", xx, plot.bottom() + 3.0, t, lsx, TEXT, HAlign::Center, VAlign::Top);
            }
        }
        let zero = if y.scale.min <= 0.0 && y.scale.max >= 0.0 { 0.0 } else { y.scale.min };
        let x0 = plot.x + y.scale.frac(zero) * plot.w;
        ctx.vline(x0, plot.y, plot.bottom(), ay, awy);
        bar_category_labels(ctx, chart, plot, &labels, outer, (lw, &level_w));
        plot
    }
}

pub(super) fn value_range(it: impl Iterator<Item = f64>) -> Option<(f64, f64)> {
    it.fold(None, |acc, v| match acc {
        None => Some((v, v)),
        Some((a, b)) => Some((a.min(v), b.max(v))),
    })
}
