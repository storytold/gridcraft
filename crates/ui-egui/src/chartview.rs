//! Floating objects on the sheet: charts (rendered by `gridcraft-chart` into primitives that
//! we paint with egui), pictures, shapes, equations, slicers and form controls (`form_controls.rs`).

use std::sync::Arc;

use egui::epaint::{PathShape, PathStroke};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, pos2, vec2};
use gridcraft_chart::{HAlign, Prim, VAlign};
use gridcraft_engine::model::{Anchor, ObjKind, ObjRef, ShapeKind, Sheet, Workbook};

use crate::SheetApp;
use crate::grid::Geo;
use crate::theme::{self, Tokens};

fn rgba(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn text_font(format: &gridcraft_engine::model::ChartTextFormat, size: f32, bold: bool) -> FontId {
    if format.name.is_some() || format.italic.unwrap_or(false) {
        FontId::new(
            size,
            theme::cell_family(format.name.as_deref().unwrap_or(gridcraft_engine::model::DEFAULT_FONT), bold, format.italic.unwrap_or(false)),
        )
    } else if bold {
        theme::ui_bold(size)
    } else {
        theme::ui_font(size)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
/// Supported selection groups; a series remains one editable group even when its markers get individual handles.
pub(crate) enum ChartElement {
    #[default]
    ChartArea,
    PlotArea,
    Title,
    XAxis,
    YAxis,
    XAxisTitle,
    YAxisTitle,
    Gridlines,
    Legend,
    DataLabels,
    TrendlineText,
    Series(usize),
}

impl ChartElement {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::ChartArea => "Chart Area",
            Self::PlotArea => "Plot Area",
            Self::Title => "Chart Title",
            Self::XAxis => "Horizontal Axis",
            Self::YAxis => "Vertical Axis",
            Self::XAxisTitle => "Horizontal Axis Title",
            Self::YAxisTitle => "Vertical Axis Title",
            Self::Gridlines => "Gridlines",
            Self::Legend => "Legend",
            Self::DataLabels => "Data Labels",
            Self::TrendlineText => "Trendline Labels",
            Self::Series(_) => "Data Series",
        }
    }
}

/// Reads transient chart-element selection without changing the worksheet's data selection.
pub(crate) fn selected_element(ctx: &egui::Context, id: u32) -> ChartElement {
    ctx.data_mut(|d| d.get_temp(egui::Id::unique(("chart_element", id)))).unwrap_or_default()
}

/// Changes the selected group and invalidates geometry cached for the previous group.
pub(crate) fn select_element(ctx: &egui::Context, id: u32, element: ChartElement) {
    if selected_element(ctx, id) != element {
        ctx.data_mut(|d| d.remove::<(ChartElement, Rect)>(egui::Id::unique(("chart_element_bounds", id))));
    }
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("chart_element", id)), element));
}

/// Bounds from this frame's primitives, shared by hit testing and selection painting.
fn element_bounds(
    painter: &Painter,
    chart: &gridcraft_engine::model::Chart,
    data: &gridcraft_chart::ChartData,
    prims: &[Prim],
    element: ChartElement,
    fallback: Rect,
) -> Rect {
    if matches!(element, ChartElement::Series(_) | ChartElement::Gridlines) {
        let colors = match element {
            ChartElement::Series(i) => {
                let mut colors = data.series.get(i).map(|s| vec![s.color]).unwrap_or_default();
                if let Some(style) = chart.format.series.get(&i) {
                    colors.extend([style.fill_color, style.line_color, style.marker_fill, style.marker_border_color].into_iter().flatten());
                }
                colors
            }
            _ => [Some([0xD9, 0xD9, 0xD9, 0xFF]), chart.format.horizontal_gridline.color, chart.format.vertical_gridline.color]
                .into_iter()
                .flatten()
                .collect(),
        };
        prims.iter().fold(Rect::NOTHING, |all, primitive| {
            let (color, rect) = match primitive {
                Prim::Line { pts, color, .. } => (*color, points_bounds(pts)),
                Prim::Polygon { pts, fill, stroke } => (if fill[3] == 0 { stroke.map(|s| s.0).unwrap_or(*fill) } else { *fill }, points_bounds(pts)),
                Prim::Circle { cx, cy, r, fill, stroke } => {
                    (fill.or_else(|| stroke.map(|s| s.0)).unwrap_or_default(), Rect::from_center_size(pos2(*cx, *cy), vec2(r * 2.0, r * 2.0)))
                }
                Prim::Rect { x, y, w, h, fill: Some(fill), .. } => (*fill, Rect::from_min_size(pos2(*x, *y), vec2(*w, *h))),
                _ => return all,
            };
            if colors.iter().any(|c| c[..3] == color[..3]) { all.union(rect) } else { all }
        })
    } else if matches!(
        element,
        ChartElement::Legend
            | ChartElement::TrendlineText
            | ChartElement::Title
            | ChartElement::XAxis
            | ChartElement::YAxis
            | ChartElement::XAxisTitle
            | ChartElement::YAxisTitle
            | ChartElement::DataLabels
    ) {
        let role = match element {
            ChartElement::Legend => "legend",
            ChartElement::TrendlineText => "trendline",
            ChartElement::Title => "title",
            ChartElement::XAxis => "x_axis",
            ChartElement::YAxis => "y_axis",
            ChartElement::XAxisTitle => "x_axis_title",
            ChartElement::YAxisTitle => "y_axis_title",
            _ => "data_labels",
        };
        let text_group = prims.iter().fold(Rect::NOTHING, |group, primitive| match primitive {
            Prim::Text { x, y, text, size, bold, format, align, valign, rotation, role: text_role, .. } if text_role == role => {
                let text_size = painter.layout_no_wrap(text.clone(), text_font(format, *size, *bold), Color32::BLACK).size();
                let ax = match align {
                    HAlign::Left => 0.0,
                    HAlign::Center => 0.5,
                    HAlign::Right => 1.0,
                };
                let ay = match valign {
                    VAlign::Top => 0.0,
                    VAlign::Middle => 0.5,
                    VAlign::Bottom => 1.0,
                };
                let local = Rect::from_min_size(pos2(-text_size.x * ax, -text_size.y * ay), text_size);
                let (sin, cos) = rotation.sin_cos();
                [local.left_top(), local.right_top(), local.right_bottom(), local.left_bottom()]
                    .into_iter()
                    .fold(Rect::NOTHING, |rect, point| {
                        let point = pos2(*x + point.x * cos - point.y * sin, *y + point.x * sin + point.y * cos);
                        rect.union(Rect::from_min_max(point, point))
                    })
                    .union(group)
            }
            _ => group,
        });
        if element == ChartElement::Legend && text_group != Rect::NOTHING {
            prims.iter().fold(text_group, |group, primitive| {
                let swatch = match primitive {
                    Prim::Rect { x, y, w, h, .. } if *w <= 24.0 && *h <= 24.0 => Some(Rect::from_min_size(pos2(*x, *y), vec2(*w, *h))),
                    Prim::Circle { cx, cy, r, .. } if *r <= 12.0 => Some(Rect::from_center_size(pos2(*cx, *cy), vec2(*r * 2.0, *r * 2.0))),
                    Prim::Line { pts, .. } | Prim::Polygon { pts, .. } => {
                        let rect = points_bounds(pts);
                        (rect.width() <= 24.0 && rect.height() <= 24.0).then_some(rect)
                    }
                    _ => None,
                };
                swatch.filter(|rect| text_group.expand(20.0).intersects(*rect)).map_or(group, |rect| group.union(rect))
            })
        } else if text_group == Rect::NOTHING {
            fallback
        } else {
            text_group
        }
    } else if element == ChartElement::PlotArea {
        prims
            .iter()
            .find_map(|primitive| match primitive {
                Prim::Rect { x, y, w, h, fill, .. }
                    if *fill == chart.format.plot_area.fill
                        && *w < chart.anchor.width
                        && *w > chart.anchor.width * 0.3
                        && *h > chart.anchor.height * 0.2 =>
                {
                    Some(Rect::from_min_size(pos2(*x, *y), vec2(*w, *h)))
                }
                _ => None,
            })
            .unwrap_or(fallback)
    } else {
        fallback
    }
}

fn refresh_element_bounds(
    painter: &Painter,
    chart: &gridcraft_engine::model::Chart,
    data: &gridcraft_chart::ChartData,
    prims: &[Prim],
    element: ChartElement,
    previous: Rect,
) -> Rect {
    let bounds = element_bounds(painter, chart, data, prims, element, previous);
    painter.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(("chart_element_bounds", chart.id)), (element, bounds)));
    bounds
}

/// Returns the selected element's current-frame bounds, populating the cache when selection came
/// from the keyboard or the format-pane combo rather than a pointer hit test.
fn selected_element_bounds(
    painter: &Painter,
    chart: &gridcraft_engine::model::Chart,
    data: &gridcraft_chart::ChartData,
    prims: &[Prim],
) -> Option<(ChartElement, Rect)> {
    let element = selected_element(painter.ctx(), chart.id);
    let id = egui::Id::unique(("chart_element_bounds", chart.id));
    let cached = painter
        .ctx()
        .data_mut(|d| d.get_temp::<(ChartElement, Rect)>(id))
        .filter(|(cached_element, rect)| *cached_element == element && rect.is_finite());
    if let Some(cached) = cached {
        return Some(cached);
    }
    let bounds = element_bounds(painter, chart, data, prims, element, Rect::NOTHING);
    if !bounds.is_finite() {
        return None;
    }
    painter.ctx().data_mut(|d| d.insert_temp(id, (element, bounds)));
    Some((element, bounds))
}

/// Hit test the primitives actually painted, including point markers, bars, lines and labels.
pub(crate) fn element_at(ui: &egui::Ui, wb: &Workbook, si: usize, chart: &gridcraft_engine::model::Chart, rect: Rect, point: Pos2) -> ChartElement {
    let z = rect.width() / chart.anchor.width.max(1.0);
    let data = gridcraft_chart::resolve(wb, si, chart);
    let prims = gridcraft_chart::render(chart, &data, chart.anchor.width, chart.anchor.height, &EguiMeasure { painter: ui.painter(), z });
    let p = (point - rect.min) / z.max(0.01);
    let near = |pts: &[[f32; 2]]| {
        pts.windows(2).any(|w| {
            let a = egui::vec2(w[0][0], w[0][1]);
            let b = egui::vec2(w[1][0], w[1][1]);
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_sq().max(0.001)).clamp(0.0, 1.0);
            (p - a - ab * t).length() <= 5.0 / z.max(0.25)
        })
    };
    let polygon_contains = |pts: &[[f32; 2]]| {
        let mut inside = false;
        for (a, b) in pts.iter().zip(pts.iter().cycle().skip(1)).take(pts.len()) {
            if (a[1] > p.y) != (b[1] > p.y) && p.x < (b[0] - a[0]) * (p.y - a[1]) / (b[1] - a[1]) + a[0] {
                inside = !inside;
            }
        }
        inside
    };
    for prim in prims.iter().rev() {
        let bounds = match prim {
            Prim::Rect { x, y, w, h, .. } => Rect::from_min_size(pos2(*x, *y), vec2(*w, *h)),
            Prim::Circle { cx, cy, r, .. } => Rect::from_center_size(pos2(*cx, *cy), vec2(*r * 2.0, *r * 2.0)),
            Prim::Line { pts, .. } | Prim::Polygon { pts, .. } => {
                pts.iter().fold(Rect::NOTHING, |r, p| r.union(Rect::from_min_max(pos2(p[0], p[1]), pos2(p[0], p[1]))))
            }
            Prim::Text { x, y, text, size, bold, format, align, valign, rotation, .. } => {
                let sz = ui.painter().layout_no_wrap(text.clone(), text_font(format, *size, *bold), Color32::BLACK).size();
                let ax = match align {
                    HAlign::Left => 0.0,
                    HAlign::Center => 0.5,
                    HAlign::Right => 1.0,
                };
                let ay = match valign {
                    VAlign::Top => 0.0,
                    VAlign::Middle => 0.5,
                    VAlign::Bottom => 1.0,
                };
                let local = Rect::from_min_size(pos2(-sz.x * ax, -sz.y * ay), sz);
                let (s, c) = rotation.sin_cos();
                [local.left_top(), local.right_top(), local.right_bottom(), local.left_bottom()].into_iter().fold(Rect::NOTHING, |r, p| {
                    let p = pos2(*x + p.x * c - p.y * s, *y + p.x * s + p.y * c);
                    r.union(Rect::from_min_max(p, p))
                })
            }
            _ => Rect::NOTHING,
        };
        let remember = |element| {
            let bounds = element_bounds(ui.painter(), chart, &data, &prims, element, bounds);
            select_element(ui.ctx(), chart.id, element);
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::unique(("chart_element_bounds", chart.id)), (element, bounds)));
            element
        };
        let color = match prim {
            Prim::Text { x, y, text, size, bold, format, align, valign, rotation, role, .. } => {
                let font = text_font(format, *size, *bold);
                let sz = ui.painter().layout_no_wrap(text.clone(), font, Color32::BLACK).size();
                let ax = match align {
                    HAlign::Left => 0.0,
                    HAlign::Center => 0.5,
                    HAlign::Right => 1.0,
                };
                let ay = match valign {
                    VAlign::Top => 0.0,
                    VAlign::Middle => 0.5,
                    VAlign::Bottom => 1.0,
                };
                let delta = p - vec2(*x, *y);
                let (s, c) = rotation.sin_cos();
                let local = pos2(delta.x * c + delta.y * s, -delta.x * s + delta.y * c);
                if !Rect::from_min_size(pos2(-sz.x * ax, -sz.y * ay), sz).expand(3.0).contains(local) {
                    continue;
                }
                let element = match role.as_str() {
                    "title" => ChartElement::Title,
                    "legend" => ChartElement::Legend,
                    "x_axis" => ChartElement::XAxis,
                    "y_axis" => ChartElement::YAxis,
                    "x_axis_title" => ChartElement::XAxisTitle,
                    "y_axis_title" => ChartElement::YAxisTitle,
                    "trendline" => ChartElement::TrendlineText,
                    "data_table" => continue,
                    _ => ChartElement::DataLabels,
                };
                return remember(element);
            }
            Prim::Circle { cx, cy, r, fill, stroke } if (p - vec2(*cx, *cy)).length() <= r + 4.0 => fill.or_else(|| stroke.map(|(c, _)| c)),
            Prim::Rect { x, y, w, h, fill, .. }
                if *fill == chart.format.plot_area.fill
                    && *w < chart.anchor.width
                    && *w > chart.anchor.width * 0.3
                    && *h > chart.anchor.height * 0.2
                    && Rect::from_min_size(pos2(*x, *y), vec2(*w, *h)).contains(p.to_pos2()) =>
            {
                return remember(ChartElement::PlotArea);
            }
            Prim::Rect { x, y, w, h, fill, .. }
                if *w < chart.anchor.width * 0.95 && Rect::from_min_size(pos2(*x, *y), vec2(*w, *h)).contains(p.to_pos2()) =>
            {
                *fill
            }
            Prim::Polygon { pts, fill, stroke } if near(pts) || polygon_contains(pts) => {
                if fill[3] == 0 {
                    stroke.map(|(c, _)| c)
                } else {
                    Some(*fill)
                }
            }
            Prim::Line { pts, color, .. } if near(pts) => {
                if *color == [0xD9, 0xD9, 0xD9, 0xFF]
                    || [chart.format.horizontal_gridline.color, chart.format.vertical_gridline.color].contains(&Some(*color))
                {
                    return remember(ChartElement::Gridlines);
                }
                if *color == [0xBF, 0xBF, 0xBF, 0xFF]
                    || [
                        chart.format.x_axis.line.color,
                        chart.format.y_axis.line.color,
                        chart.format.x_axis.tick_line.color,
                        chart.format.y_axis.tick_line.color,
                    ]
                    .contains(&Some(*color))
                {
                    return remember(if pts.first().zip(pts.last()).is_some_and(|(a, b)| (b[0] - a[0]).abs() > (b[1] - a[1]).abs()) {
                        ChartElement::XAxis
                    } else {
                        ChartElement::YAxis
                    });
                }
                Some(*color)
            }
            _ => None,
        };
        if let Some(color) = color
            && let Some(i) = data.series.iter().enumerate().rposition(|(i, s)| {
                s.color[..3] == color[..3]
                    || chart.format.series.get(&i).is_some_and(|s| {
                        [s.fill_color, s.line_color, s.marker_fill, s.marker_border_color].into_iter().flatten().any(|c| c[..3] == color[..3])
                    })
            })
        {
            return remember(ChartElement::Series(i));
        }
    }
    ChartElement::ChartArea
}

fn points_bounds(points: &[[f32; 2]]) -> Rect {
    points.iter().fold(Rect::NOTHING, |bounds, p| bounds.union(Rect::from_min_max(pos2(p[0], p[1]), pos2(p[0], p[1]))))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum SeriesSelectionMark {
    Shape(Rect),
    Point(Pos2),
}

/// Returns per-datum geometry for the active series rather than one confusing frame around its
/// entire plot-area extent. Chart primitives do not carry series IDs, so their rendered colors
/// are matched against the series palette and any explicit formatting overrides.
fn series_selection_marks(prims: &[Prim], colors: &[[u8; 4]]) -> Vec<SeriesSelectionMark> {
    let matches = |color: [u8; 4]| colors.iter().any(|candidate| candidate[..3] == color[..3]);
    let mut marks = Vec::new();
    for prim in prims {
        match prim {
            Prim::Rect { x, y, w, h, fill: Some(fill), .. } if matches(*fill) => {
                marks.push(SeriesSelectionMark::Shape(Rect::from_min_size(pos2(*x, *y), vec2(*w, *h))));
            }
            Prim::Circle { cx, cy, fill, stroke, .. } if fill.is_some_and(matches) || stroke.is_some_and(|(color, _)| matches(color)) => {
                marks.push(SeriesSelectionMark::Point(pos2(*cx, *cy)));
            }
            Prim::Polygon { pts, fill, stroke } if matches(*fill) || stroke.is_some_and(|(color, _)| matches(color)) => {
                // Small polygons are marker glyphs (triangles/diamonds); large ones are filled
                // series such as areas, whose smoothed paths can contain many interpolated points.
                let bounds = points_bounds(pts);
                if bounds.width() <= 24.0 && bounds.height() <= 24.0 {
                    marks.push(SeriesSelectionMark::Point(bounds.center()));
                }
            }
            _ => {}
        }
    }
    marks
}

fn paint_series_selection(p: &Painter, marks: &[SeriesSelectionMark], origin: Pos2, z: f32) {
    let accent = Tokens::get(p.ctx()).accent;
    for mark in marks {
        match mark {
            SeriesSelectionMark::Shape(bounds) => {
                let bounds = Rect::from_min_max(origin + bounds.min.to_vec2() * z, origin + bounds.max.to_vec2() * z).expand(2.0);
                p.rect_stroke(bounds, 0.0, Stroke::new(1.5, accent), StrokeKind::Outside);
            }
            SeriesSelectionMark::Point(point) => {
                let center = origin + point.to_vec2() * z;
                let bounds = Rect::from_center_size(center, vec2(7.0, 7.0));
                p.rect_filled(bounds, 1.0, Color32::WHITE);
                p.rect_stroke(bounds, 1.0, Stroke::new(1.25, accent), StrokeKind::Outside);
            }
        }
    }
}

struct EguiMeasure<'a> {
    painter: &'a Painter,
    z: f32,
}

impl gridcraft_chart::Measure for EguiMeasure<'_> {
    fn text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let font = if bold { theme::ui_bold(size * self.z) } else { theme::ui_font(size * self.z) };
        self.painter.layout_no_wrap(text.to_string(), font, Color32::BLACK).size().x / self.z.max(0.01)
    }
}

/// Paints `chart` (not necessarily on the sheet: the Recommended Charts preview) scaled to fit
/// `rect`.
pub fn paint_chart(p: &Painter, rect: Rect, wb: &Workbook, sheet: usize, chart: &gridcraft_engine::model::Chart) {
    let (w, h) = (chart.anchor.width.max(1.0), chart.anchor.height.max(1.0));
    let z = (rect.width() / w).min(rect.height() / h);
    if !(z.is_finite() && z > 0.0) {
        return;
    }
    let origin = rect.center() - vec2(w * z, h * z) / 2.0;
    let data = gridcraft_chart::resolve(wb, sheet, chart);
    let m = EguiMeasure { painter: p, z };
    let prims = gridcraft_chart::render(chart, &data, w, h, &m);
    paint_prims(&p.with_clip_rect(rect.intersect(p.clip_rect())), origin, &prims, z);
}

/// Paints chart primitives at `origin`, scaled by `z`.
pub fn paint_prims(p: &Painter, origin: Pos2, prims: &[Prim], z: f32) {
    let pt = |x: f32, y: f32| pos2(origin.x + x * z, origin.y + y * z);
    for prim in prims {
        match prim {
            Prim::Rect { x, y, w, h, fill, stroke, radius } => {
                let r = Rect::from_min_size(pt(*x, *y), vec2(w * z, h * z));
                if let Some(f) = fill {
                    p.rect_filled(r, radius * z, rgba(*f));
                }
                if let Some((c, wd)) = stroke {
                    p.rect_stroke(r, radius * z, Stroke::new(wd * z.max(0.5), rgba(*c)), StrokeKind::Inside);
                }
            }
            Prim::Line { pts, color, width, dash } => {
                let v: Vec<Pos2> = pts.iter().map(|q| pt(q[0], q[1])).collect();
                if v.len() < 2 {
                    continue;
                }
                if *dash {
                    p.extend(egui::Shape::dashed_line(&v, Stroke::new(width * z.max(0.5), rgba(*color)), 4.0 * z, 3.0 * z));
                } else {
                    p.add(PathShape::line(v, PathStroke::new(width * z.max(0.5), rgba(*color))));
                }
            }
            Prim::Polygon { pts, fill, stroke } => {
                let v: Vec<Pos2> = pts.iter().map(|q| pt(q[0], q[1])).collect();
                if v.len() < 3 {
                    continue;
                }
                // Polygons may be concave (areas): triangulate as a fan from the first point
                // when convex, else draw through a mesh of the closed path.
                let shape = PathShape { points: v.clone(), closed: true, fill: rgba(*fill), stroke: PathStroke::NONE };
                if is_convex(&v) {
                    p.add(shape);
                } else {
                    p.add(egui::Shape::mesh(area_mesh(&v, rgba(*fill))));
                }
                if let Some((c, w)) = stroke {
                    p.add(PathShape::closed_line(v, PathStroke::new(w * z.max(0.5), rgba(*c))));
                }
            }
            Prim::Wedge { cx, cy, r_outer, r_inner, a0, a1, fill, stroke } => {
                let c = pt(*cx, *cy);
                let steps = (((a1 - a0).abs() / 0.05).ceil() as usize).clamp(2, 400);
                let at = |a: f32, r: f32| pos2(c.x + r * z * a.sin(), c.y - r * z * a.cos());
                let mut mesh = egui::Mesh::default();
                let col = rgba(*fill);
                for i in 0..=steps {
                    let a = a0 + (a1 - a0) * i as f32 / steps as f32;
                    mesh.colored_vertex(at(a, *r_outer), col);
                    mesh.colored_vertex(if *r_inner > 0.0 { at(a, *r_inner) } else { c }, col);
                }
                for i in 0..steps as u32 {
                    let b = i * 2;
                    mesh.add_triangle(b, b + 1, b + 2);
                    mesh.add_triangle(b + 1, b + 3, b + 2);
                }
                p.add(egui::Shape::mesh(mesh));
                if let Some((sc, w)) = stroke {
                    let mut outline: Vec<Pos2> = (0..=steps).map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, *r_outer)).collect();
                    if *r_inner > 0.0 {
                        outline.extend((0..=steps).rev().map(|i| at(a0 + (a1 - a0) * i as f32 / steps as f32, *r_inner)));
                    } else {
                        outline.push(c);
                    }
                    p.add(PathShape::closed_line(outline, PathStroke::new(w * z.max(0.5), rgba(*sc))));
                }
            }
            Prim::Circle { cx, cy, r, fill, stroke } => {
                if let Some(f) = fill {
                    p.circle_filled(pt(*cx, *cy), r * z, rgba(*f));
                }
                if let Some((c, w)) = stroke {
                    p.circle_stroke(pt(*cx, *cy), r * z, Stroke::new(w * z.max(0.5), rgba(*c)));
                }
            }
            Prim::Text { x, y, text, size, color, bold, align, valign, rotation, format, .. } => {
                let font = text_font(format, size * z, *bold);
                let galley = p.layout_no_wrap(text.clone(), font, rgba(*color));
                let sz = galley.size();
                let ax = match align {
                    HAlign::Left => 0.0,
                    HAlign::Center => 0.5,
                    HAlign::Right => 1.0,
                };
                let ay = match valign {
                    VAlign::Top => 0.0,
                    VAlign::Middle => 0.5,
                    VAlign::Bottom => 1.0,
                };
                let anchor = pt(*x, *y);
                if *rotation == 0.0 {
                    p.galley(pos2(anchor.x - sz.x * ax, anchor.y - sz.y * ay), galley, rgba(*color));
                } else {
                    // Rotate about the anchor: offset the galley origin by the rotated alignment.
                    let (s, c) = rotation.sin_cos();
                    let off = vec2(-sz.x * ax, -sz.y * ay);
                    let rot = vec2(off.x * c - off.y * s, off.x * s + off.y * c);
                    let mut shape = egui::epaint::TextShape::new(anchor + rot, galley, rgba(*color));
                    shape.angle = *rotation;
                    p.add(shape);
                }
            }
        }
    }
}

fn is_convex(v: &[Pos2]) -> bool {
    let n = v.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let a = v[i];
        let b = v[(i + 1) % n];
        let c = v[(i + 2) % n];
        let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);
        if cross.abs() > 1e-3 {
            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }
    }
    true
}

/// Area charts produce "mountain" polygons whose bottom edge is a straight baseline: fill them
/// as vertical strips (exact for such shapes, acceptable for others).
fn area_mesh(v: &[Pos2], col: Color32) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let n = v.len();
    if n < 3 {
        return mesh;
    }
    let half = n / 2;
    for i in 0..half.saturating_sub(1) {
        let a = v[i];
        let b = v[i + 1];
        let c = v[n - 2 - i];
        let d = v[n - 1 - i];
        let base = mesh.vertices.len() as u32;
        for q in [a, b, c, d] {
            mesh.colored_vertex(q, col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}

/// A shape's text with its formatting: size, bold/italic, typeface, colour, outline, horizontal
/// alignment and vertical anchor. Shapes other than text boxes default to white text centred in
/// the shape; text boxes to black text at the top left.
fn paint_shape_text(p: &Painter, wb: &Workbook, sp: &gridcraft_engine::model::Shape, r: Rect, z: f32) {
    use gridcraft_engine::model::{ShapeTextAlign, ShapeTextAnchor};
    let f = &sp.format;
    let word_art = sp.kind == ShapeKind::WordArt;
    let text_box = sp.kind == ShapeKind::TextBox;
    let align = f.align.unwrap_or(if text_box { ShapeTextAlign::Left } else { ShapeTextAlign::Center });
    let anchor = f.anchor.unwrap_or(if text_box { ShapeTextAnchor::Top } else { ShapeTextAnchor::Middle });
    let default_color = if text_box || word_art { Color32::BLACK } else { Color32::WHITE };
    let color = f.text_color.and_then(|c| c.resolve(&wb.theme)).map(theme::color32).unwrap_or(default_color);
    // Default 14 px (as before); a set size is in points.
    let size = f.font_size.map_or(14.0, |pt| pt * 96.0 / 72.0).clamp(1.0, 2000.0) * z;
    let family = theme::cell_family(f.font.as_deref().unwrap_or(""), f.bold, f.italic);
    let inset = 4.0;
    let width = (r.width() - 2.0 * inset).max(1.0);
    let mut job = egui::text::LayoutJob::simple(sp.text.clone(), FontId::new(size, family), color, width);
    job.halign = match align {
        ShapeTextAlign::Left => egui::Align::LEFT,
        ShapeTextAlign::Center => egui::Align::Center,
        ShapeTextAlign::Right => egui::Align::RIGHT,
    };
    let g = p.layout_job(job);
    let x = match align {
        ShapeTextAlign::Left => r.left() + inset,
        ShapeTextAlign::Center => r.center().x,
        ShapeTextAlign::Right => r.right() - inset,
    };
    let y = match anchor {
        ShapeTextAnchor::Top => r.top() + inset,
        ShapeTextAnchor::Middle => r.center().y - g.size().y / 2.0,
        ShapeTextAnchor::Bottom => r.bottom() - inset - g.size().y,
    };
    let pos = pos2(x, y);
    let clip = p.with_clip_rect(r.intersect(p.clip_rect()));
    // WordArt's drop shadow: the text again, offset down and right, under everything else.
    if let Some(sc) = f.text_shadow.and_then(|c| c.resolve(&wb.theme)).map(theme::color32) {
        let d = (size * 0.05).clamp(1.0, 24.0);
        clip.galley_with_override_text_color(pos + vec2(d, d), g.clone(), sc);
    }
    // egui has no stroked text: the outline is the text drawn in the outline colour, offset in
    // eight directions by the outline width, under the filled text.
    if let Some((c, w)) = f.text_outline
        && let Some(oc) = c.resolve(&wb.theme).map(theme::color32)
    {
        let d = (w * 96.0 / 72.0 * z / 2.0).clamp(0.5, 20.0);
        for (dx, dy) in [(-1.0, -1.0), (0.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (0.0, 1.0), (1.0, 1.0)] {
            clip.galley_with_override_text_color(pos + vec2(dx * d, dy * d), g.clone(), oc);
        }
    }
    clip.galley(pos, g, color);
}

/// Resolves and renders `ch` (chart `ci` of sheet `si`), at zoom `z`.
fn render_chart(p: &Painter, z: f32, wb: &Workbook, si: usize, ch: &gridcraft_engine::model::Chart) -> Vec<Prim> {
    #[cfg(test)]
    tests::RENDERS.with(|c| c.set(c.get() + 1));
    let data = gridcraft_chart::resolve(wb, si, ch);
    let m = EguiMeasure { painter: p, z };
    gridcraft_chart::render(ch, &data, ch.anchor.width, ch.anchor.height, &m)
}

/// The primitives of chart `ci` of sheet `si`: from the frame cache when the workbook, zoom and
/// screen scale are those it was rendered for, else rendered now (see [`crate::chartcache`]).
fn chart_prims(app: &SheetApp, p: &Painter, z: f32, wb: &Arc<Workbook>, si: usize, ci: usize, ch: &gridcraft_engine::model::Chart) -> Arc<[Prim]> {
    // Only the active document's own workbook is cached: anything else cannot be tied to a uid.
    let Some(d) = app.session.active().filter(|d| Arc::ptr_eq(&d.wb, wb)) else {
        return render_chart(p, z, wb, si, ch).into();
    };
    // A series that calls NOW or RAND, directly or through a defined name, must be evaluated
    // again on every paint.
    if gridcraft_chart::has_volatile_series(wb, si, ch) {
        return render_chart(p, z, wb, si, ch).into();
    }
    let mut cache = app.chart_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.get(d.uid, wb, si, ci, (z, p.ctx().pixels_per_point()), || render_chart(p, z, wb, si, ch))
}

pub(crate) fn anchor_rect(geo: &Geo, sh: &Sheet, a: &Anchor) -> Rect {
    let x = geo.x(sh, a.cell.col) + a.dx * geo.z;
    let y = geo.y(sh, a.cell.row) + a.dy * geo.z;
    Rect::from_min_size(pos2(x, y), vec2(a.width * geo.z, a.height.max(1.0) * geo.z))
}

/// The shapes one floating object painted (indices into the painter's layer) and its logical
/// rectangle, so a right-to-left sheet can move it as a whole instead of mirroring it.
pub(crate) struct PaintedObject {
    pub shapes: std::ops::Range<usize>,
    pub rect: Rect,
}

/// Ctrl+6 (Options › Advanced › "For objects, show: Nothing"): objects are neither drawn nor
/// clickable. Placeholders, which Excel no longer offers, show the objects.
pub(crate) fn objects_hidden(wb: &Workbook) -> bool {
    wb.show_objects == gridcraft_engine::model::ShowObjects::None
}

pub(crate) fn paint_objects(app: &SheetApp, p: &Painter, geo: &Geo, wb: &Arc<Workbook>, si: usize, sh: &Sheet) -> Vec<PaintedObject> {
    let mut painted = Vec::new();
    if objects_hidden(wb) {
        return painted;
    }
    // Source outlines stay in cell space beneath every floating object.
    if let Some(ch) = sh.charts.iter().find(|ch| app.selected_chart() == Some(ch.id))
        && anchor_rect(geo, sh, &ch.anchor).intersects(p.clip_rect())
    {
        for s in &ch.series {
            if let Ok(gridcraft_engine::formula::Expr::Ref(rf)) = gridcraft_engine::formula::parse(&s.values)
                && rf.sheet_name().is_none_or(|n| n.eq_ignore_ascii_case(&sh.name))
            {
                let rr = geo.range_rect(sh, rf.range());
                let color = Tokens::get(p.ctx()).ref_color(0, true);
                for pane in geo.range_panes(rf.range()) {
                    let pp = p.with_clip_rect(pane.intersect(p.clip_rect()));
                    pp.rect_stroke(rr, 0.0, Stroke::new(1.5, color), StrokeKind::Inside);
                }
            }
        }
    }
    let mut done = |start: egui::layers::ShapeIdx, rect: Rect| painted.push(PaintedObject { shapes: start.0..p.add(egui::Shape::Noop).0, rect });
    // The grid paints the active workbook: its pictures are cached under it.
    let doc = app.session.active().map_or(0, |d| d.uid);
    for sp in &sh.shapes {
        let r = anchor_rect(geo, sh, &sp.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let start = p.add(egui::Shape::Noop);
        let mut label = true;
        let fill = sp.fill.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::TRANSPARENT);
        let line =
            if sp.format.no_line { Color32::TRANSPARENT } else { sp.line.resolve(&wb.theme).map(theme::color32).unwrap_or(Color32::DARK_GRAY) };
        match sp.kind {
            // WordArt is only its text (no fill or outline of its own).
            ShapeKind::WordArt => {}
            ShapeKind::Rectangle | ShapeKind::TextBox => {
                p.rect_filled(r, 0.0, fill);
                p.rect_stroke(r, 0.0, Stroke::new(1.0, line), StrokeKind::Inside);
            }
            ShapeKind::RoundedRectangle => {
                p.rect_filled(r, 10.0 * geo.z, fill);
                p.rect_stroke(r, 10.0 * geo.z, Stroke::new(1.0, line), StrokeKind::Inside);
            }
            ShapeKind::Ellipse => {
                let pts: Vec<Pos2> = (0..64)
                    .map(|i| {
                        let a = i as f32 / 64.0 * std::f32::consts::TAU;
                        pos2(r.center().x + r.width() / 2.0 * a.cos(), r.center().y + r.height() / 2.0 * a.sin())
                    })
                    .collect();
                p.add(PathShape::convex_polygon(pts, fill, Stroke::new(1.0, line)));
            }
            ShapeKind::Triangle => {
                p.add(PathShape::convex_polygon(vec![pos2(r.center().x, r.top()), r.right_bottom(), r.left_bottom()], fill, Stroke::new(1.0, line)));
            }
            ShapeKind::Ink => {
                let pt = gridcraft_engine::cmd::draw::ink_width(sp);
                let w = pt * geo.z;
                let pts: Vec<Pos2> =
                    gridcraft_engine::cmd::draw::ink_points(sp).iter().map(|q| pos2(r.left() + q[0] * geo.z, r.top() + q[1] * geo.z)).collect();
                if pts.len() >= 2 {
                    let col = ink_color(line, pt);
                    p.add(PathShape::line(pts, PathStroke::new(w, col)));
                }
                label = false;
            }
            ShapeKind::Icon => {
                if let Some(icon) = crate::icons::from_name(&sp.text) {
                    let t = Tokens::get(p.ctx());
                    crate::icons::paint_on_sheet(p, r, icon, if fill == Color32::TRANSPARENT { t.cell_text } else { fill });
                }
                label = false;
            }
            ShapeKind::Equation => {
                crate::equation::paint_shape(p, r, sp, wb, geo.z);
                label = false;
            }
            ShapeKind::Line | ShapeKind::Arrow => {
                p.line_segment([r.left_top(), r.right_bottom()], Stroke::new(2.0, line));
                if sp.kind == ShapeKind::Arrow {
                    let d = (r.right_bottom() - r.left_top()).normalized();
                    let n = vec2(-d.y, d.x);
                    let tip = r.right_bottom();
                    p.add(PathShape::convex_polygon(vec![tip, tip - d * 10.0 + n * 5.0, tip - d * 10.0 - n * 5.0], line, Stroke::NONE));
                }
            }
        }
        if label && !sp.text.is_empty() && !app.editing_text_box(sp.id) {
            paint_shape_text(p, wb, sp, r, geo.z);
        }
        done(start, r);
    }
    for im in &sh.images {
        let r = anchor_rect(geo, sh, &im.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let start = p.add(egui::Shape::Noop);
        if let Some(tex) = crate::floating_pictures::texture(p.ctx(), doc, im, r) {
            p.image(tex.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        } else {
            p.rect_filled(r, 0.0, Tokens::get(p.ctx()).sheet_track);
        }
        done(start, r);
    }
    for (ci, ch) in sh.charts.iter().enumerate() {
        let r = anchor_rect(geo, sh, &ch.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let start = p.add(egui::Shape::Noop);
        // Soft shadow + white card.
        p.rect_filled(r.translate(vec2(0.0, 1.5)), 2.0, Color32::from_black_alpha(18));
        // The renderer owns the area fill, including transparency.
        let prims = chart_prims(app, p, geo.z, wb, si, ci, ch);
        paint_prims(&p.with_clip_rect(r.intersect(p.clip_rect())), r.min, &prims, geo.z);
        if app.selected_chart() == Some(ch.id) {
            let data = gridcraft_chart::resolve(wb, si, ch);
            if let Some((element, bounds)) = selected_element_bounds(p, ch, &data, &prims) {
                let bounds = refresh_element_bounds(p, ch, &data, &prims, element, bounds);
                let overlay = p.with_clip_rect(r);
                if let ChartElement::Series(index) = element {
                    let mut colors = data.series.get(index).map(|series| vec![series.color]).unwrap_or_default();
                    if let Some(format) = ch.format.series.get(&index) {
                        colors.extend([format.fill_color, format.line_color, format.marker_fill, format.marker_border_color].into_iter().flatten());
                    }
                    let marks = series_selection_marks(&prims, &colors);
                    paint_series_selection(&overlay, &marks, r.min, geo.z);
                } else {
                    let bounds = Rect::from_min_max(r.min + bounds.min.to_vec2() * geo.z, r.min + bounds.max.to_vec2() * geo.z).expand(3.0);
                    overlay.rect_stroke(bounds, 0.0, Stroke::new(1.5, crate::theme::Tokens::get(p.ctx()).accent), StrokeKind::Outside);
                }
            }
        }
        // The source-data highlight below is in cell space: mirrored with the cells.
        done(start, r);
    }
    paint_slicers(p, geo, wb, si, sh);
    // Form controls sit above every other object.
    let hover = p.ctx().input(|i| i.pointer.hover_pos()).map(|h| geo.mirror_pos(h)).filter(|h| geo.in_cells(*h));
    for c in &sh.controls {
        let r = anchor_rect(geo, sh, &c.anchor);
        // A slider's value shows beside it while it is dragged.
        if !r.expand(24.0).intersects(p.clip_rect()) {
            continue;
        }
        let start = p.add(egui::Shape::Noop);
        crate::form_controls::paint(p, r, c, geo.z, crate::form_controls::look(app, geo, c, r, hover));
        done(start, r);
    }
    // The selected object's frame and handles go over every object, and move with it on a
    // right-to-left sheet.
    if let Some(r) = app.selected_object.and_then(|o| object_rect(geo, sh, o))
        && r.intersects(p.clip_rect())
    {
        let start = p.add(egui::Shape::Noop);
        selection_frame(p, r);
        done(start, r);
    }
    painted
}

/// Geometry of a slicer's parts within its screen rect.
pub struct SlicerLayout {
    pub header: Rect,
    /// Clear-filter button in the header.
    pub clear: Rect,
    /// One rect per value tile, in order.
    pub tiles: Vec<Rect>,
}

pub fn slicer_layout(rect: Rect, z: f32, n: usize, columns: u32) -> SlicerLayout {
    let header_h = 26.0 * z;
    let pad = 6.0 * z;
    let gap = 3.0 * z;
    let tile_h = 22.0 * z;
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), header_h));
    let cb = 16.0 * z;
    let clear = Rect::from_min_size(pos2(header.right() - pad - cb, header.top() + (header_h - cb) / 2.0), vec2(cb, cb));
    let cols = columns.max(1) as f32;
    let inner_left = rect.left() + pad;
    let inner_top = header.bottom() + pad;
    let inner_w = (rect.width() - 2.0 * pad).max(1.0);
    let tile_w = ((inner_w - (cols - 1.0) * gap) / cols).max(1.0);
    let mut tiles = Vec::with_capacity(n);
    for i in 0..n {
        let col = (i as f32) % cols;
        let row = (i as f32 / cols).floor();
        let x = inner_left + col * (tile_w + gap);
        let y = inner_top + row * (tile_h + gap);
        tiles.push(Rect::from_min_size(pos2(x, y), vec2(tile_w, tile_h)));
    }
    SlicerLayout { header, clear, tiles }
}

fn paint_slicers(p: &Painter, geo: &Geo, wb: &Workbook, si: usize, sh: &Sheet) {
    let tk = theme::Tokens::get(p.ctx());
    for sl in &sh.slicers {
        let r = anchor_rect(geo, sh, &sl.anchor);
        if !r.intersects(p.clip_rect()) {
            continue;
        }
        let items = gridcraft_engine::cmd::slicer::slicer_items(wb, si, sl);
        let lay = slicer_layout(r, geo.z, items.len(), sl.columns);
        let radius = 4.0 * geo.z;
        // Card + soft shadow.
        p.rect_filled(r.translate(vec2(0.0, 1.5)), radius, Color32::from_black_alpha(18));
        p.rect_filled(r, radius, tk.window);
        p.rect_stroke(r, radius, Stroke::new(1.0, tk.separator), StrokeKind::Inside);
        // Header.
        p.rect_filled(lay.header, radius, tk.accent_soft);
        p.rect_filled(Rect::from_min_max(pos2(lay.header.left(), lay.header.bottom() - radius), lay.header.max), 0.0, tk.accent_soft);
        let cap = if sl.caption.is_empty() { sl.column.clone() } else { sl.caption.clone() };
        p.text(lay.header.left_center() + vec2(8.0 * geo.z, 0.0), Align2::LEFT_CENTER, cap, theme::ui_bold(12.5 * geo.z), tk.accent_dark);
        // Clear-filter button (tinted when a filter is active).
        let filtered = sl.selected.is_some();
        if let Some(icon) = crate::icons::from_name("filter") {
            crate::icons::paint(p, lay.clear, icon, if filtered { tk.accent } else { tk.text_dim });
        }
        // Value tiles.
        for ((val, on), tr) in items.iter().zip(&lay.tiles) {
            if !r.contains(tr.left_top()) {
                continue; // overflowed the card; skip (no scroll yet)
            }
            let (fill, fg) = if *on { (tk.accent, tk.on_accent) } else { (tk.hover, tk.text_dim) };
            p.rect_filled(*tr, 3.0 * geo.z, fill);
            let cp = p.with_clip_rect(tr.intersect(p.clip_rect()));
            cp.text(tr.left_center() + vec2(6.0 * geo.z, 0.0), Align2::LEFT_CENTER, val, theme::ui_font(12.0 * geo.z), fg);
        }
    }
}

/// What a click inside a slicer hit.
pub enum SlicerAction {
    Toggle(String),
    Clear,
}

/// A click on a slicer's value tile or clear button, if any. A click elsewhere in a slicer returns
/// `None` so the caller can treat it as a select/drag on the object.
pub fn slicer_click(geo: &Geo, sh: &Sheet, wb: &Workbook, si: usize, p: Pos2) -> Option<(u32, SlicerAction)> {
    if !geo.cells.contains(p) || objects_hidden(wb) {
        return None;
    }
    for sl in sh.slicers.iter().rev() {
        let r = anchor_rect(geo, sh, &sl.anchor);
        if !r.contains(p) {
            continue;
        }
        let items = gridcraft_engine::cmd::slicer::slicer_items(wb, si, sl);
        let lay = slicer_layout(r, geo.z, items.len(), sl.columns);
        if lay.clear.contains(p) {
            return Some((sl.id, SlicerAction::Clear));
        }
        for ((val, _on), tr) in items.iter().zip(&lay.tiles) {
            if r.contains(tr.left_top()) && tr.contains(p) {
                return Some((sl.id, SlicerAction::Toggle(val.clone())));
            }
        }
        return None; // inside this (topmost) slicer, but on its header/background → select/drag
    }
    None
}

fn selection_frame(p: &Painter, r: Rect) {
    let t = Tokens::get(p.ctx());
    p.rect_stroke(r, 0.0, Stroke::new(1.0, t.sheet_control_border), StrokeKind::Outside);
    for h in crate::objects::Handle::ALL {
        let c = h.pos(r);
        p.circle_filled(c, 4.0, t.sheet_control_bg);
        p.circle_stroke(c, 4.0, Stroke::new(1.0, t.sheet_control_border));
    }
}

/// Where object `o` sits on the sheet (logical coordinates), when it exists on `sh`.
pub fn object_rect(geo: &Geo, sh: &Sheet, o: ObjRef) -> Option<Rect> {
    sh.object_anchor(o).map(|a| anchor_rect(geo, sh, &a))
}

/// The resize handle of the selected object under `p`, with the object and its rect. Handles
/// reach a little outside their object, so they win over [`hit`], but like it only over the cell
/// area: a handle that lies under the headers, the formula bar or a scroll bar leaves the press
/// to them. Nothing while objects are hidden.
pub fn selected_handle(app: &SheetApp, geo: &Geo, sh: &Sheet, p: Pos2) -> Option<(ObjRef, Rect, crate::objects::Handle)> {
    if !geo.cells.contains(p) || app.session.active().is_some_and(|d| objects_hidden(&d.wb)) {
        return None;
    }
    let o = app.selected_object?;
    let r = object_rect(geo, sh, o)?;
    crate::objects::handle_at(r, p).map(|h| (o, r, h))
}

/// The topmost object under `p` and its rect: form controls paint on top, so they are hit first,
/// then slicers, charts, pictures and shapes (equations included), each from the top of its stack.
/// Nothing is hit outside the cell area or while objects are hidden.
pub fn hit(app: &SheetApp, geo: &Geo, sh: &Sheet, p: Pos2) -> Option<(ObjRef, Rect)> {
    if !geo.cells.contains(p) || app.session.active().is_some_and(|d| objects_hidden(&d.wb)) {
        return None;
    }
    for c in sh.controls.iter().rev() {
        let r = anchor_rect(geo, sh, &c.anchor);
        if r.contains(p) {
            return Some((ObjRef::new(ObjKind::Control, c.id), r));
        }
    }
    for sl in sh.slicers.iter().rev() {
        let r = anchor_rect(geo, sh, &sl.anchor);
        if r.contains(p) {
            return Some((ObjRef::new(ObjKind::Slicer, sl.id), r));
        }
    }
    for ch in sh.charts.iter().rev() {
        let r = anchor_rect(geo, sh, &ch.anchor);
        if r.expand(4.0).contains(p) {
            return Some((ObjRef::new(ObjKind::Chart, ch.id), r));
        }
    }
    for im in sh.images.iter().rev() {
        let r = anchor_rect(geo, sh, &im.anchor);
        if r.contains(p) {
            return Some((ObjRef::new(ObjKind::Image, im.id), r));
        }
    }
    for s in sh.shapes.iter().rev() {
        let r = anchor_rect(geo, sh, &s.anchor);
        if r.expand(3.0).contains(p) {
            let kind = if s.kind == ShapeKind::Equation { ObjKind::Equation } else { ObjKind::Shape };
            return Some((ObjRef::new(kind, s.id), r));
        }
    }
    None
}

/// The colour ink is painted in: a highlighter (a stroke over 8 pt wide) is translucent, by its
/// width in points, so the live pen stroke and the committed ink agree at every zoom.
pub(crate) fn ink_color(line: Color32, width_pt: f32) -> Color32 {
    if width_pt > 8.0 { line.gamma_multiply(0.45) } else { line }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::Cell;

    use gridcraft_chart::Measure;

    use super::*;

    thread_local! {
        /// Charts resolved and rendered on this thread (a test runs on its own thread).
        pub(crate) static RENDERS: Cell<usize> = const { Cell::new(0) };
    }

    /// The production measure is not monotone for labels with a line break: egui lays the text out
    /// line by line, so ending a prefix with `\n…` can be narrower than the shorter `iii…`. The
    /// chart's label fitting must not assume otherwise (#1152).
    #[test]
    fn egui_measure_shrinks_when_a_prefix_ends_in_a_line_break() {
        let ctx = egui::Context::default();
        ctx.set_fonts(theme::pinned_font_definitions());
        let mut checked = false;
        // Fonts are ready from the second pass on.
        for _ in 0..2 {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                let m = EguiMeasure { painter: ui.painter(), z: 1.0 };
                let (short, long) = (m.text_width("iii…", 11.0, false), m.text_width("iiii\n…", 11.0, false));
                checked = long < short;
            });
            out.textures_delta.clear();
        }
        assert!(checked);
    }
}

#[cfg(test)]
mod selection_tests {
    use super::{Prim, SeriesSelectionMark, series_selection_marks};

    #[test]
    fn selected_legend_bounds_follow_overlay_layout_without_another_hit() {
        use super::*;
        use gridcraft_engine::Session;
        use serde_json::json;
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range":"A1","values":[["X","Measured","Comparison"],[1,2,3],[2,4,5],[3,6,7]]})).unwrap();
        session.execute("insert.chart", json!({"type":"line","range":"A1:C4"})).unwrap();
        let wb = &session.doc().unwrap().wb;
        let mut chart = wb.active().unwrap().charts[0].clone();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let data = gridcraft_chart::resolve(wb, 0, &chart);
            let render = |chart: &gridcraft_engine::model::Chart| {
                gridcraft_chart::render(chart, &data, chart.anchor.width, chart.anchor.height, &EguiMeasure { painter: ui.painter(), z: 1.0 })
            };
            let before = element_bounds(ui.painter(), &chart, &data, &render(&chart), ChartElement::Legend, Rect::NOTHING);
            assert!(before.is_finite());
            select_element(&ctx, chart.id, ChartElement::Legend);
            ctx.data_mut(|d| d.insert_temp(egui::Id::unique(("chart_element_bounds", chart.id)), (ChartElement::Legend, before)));
            chart.format.legend_overlay = true;
            chart.format.legend_position = Some([0.2, 0.25]);
            let prims = render(&chart);
            let after = refresh_element_bounds(ui.painter(), &chart, &data, &prims, ChartElement::Legend, before);
            assert!(after.is_finite());
            assert!((after.center() - before.center()).length() > 20.0);
            assert_eq!(selected_element(&ctx, chart.id), ChartElement::Legend);
            assert_eq!(ctx.data_mut(|d| d.get_temp::<(ChartElement, Rect)>(egui::Id::unique(("chart_element_bounds", chart.id)))).unwrap().1, after);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn keyboard_selected_element_populates_bounds_without_hit_testing() {
        use super::*;
        use gridcraft_engine::Session;
        use serde_json::json;
        let mut session = Session::new();
        session.new_workbook();
        session.execute("range.setValues", json!({"range":"A1","values":[["X","Measured","Comparison"],[1,2,3],[2,4,5],[3,6,7]]})).unwrap();
        session.execute("insert.chart", json!({"type":"line","range":"A1:C4"})).unwrap();
        let wb = &session.doc().unwrap().wb;
        let chart = &wb.active().unwrap().charts[0];
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let data = gridcraft_chart::resolve(wb, 0, chart);
            let prims =
                gridcraft_chart::render(chart, &data, chart.anchor.width, chart.anchor.height, &EguiMeasure { painter: ui.painter(), z: 1.0 });
            select_element(&ctx, chart.id, ChartElement::Legend);
            assert!(ctx.data_mut(|d| d.get_temp::<(ChartElement, Rect)>(egui::Id::unique(("chart_element_bounds", chart.id)))).is_none());
            let (selected, bounds) = selected_element_bounds(ui.painter(), chart, &data, &prims).unwrap();
            assert_eq!(selected, ChartElement::Legend);
            assert!(bounds.is_finite());
            assert_eq!(
                ctx.data_mut(|d| d.get_temp::<(ChartElement, Rect)>(egui::Id::unique(("chart_element_bounds", chart.id)))).unwrap(),
                (selected, bounds)
            );
        });
        output.textures_delta.clear();
    }

    #[test]
    fn series_selection_marks_bars_and_explicit_points_but_not_line_segments() {
        let color = [30, 100, 180, 255];
        let prims = [
            Prim::Rect { x: 2.0, y: 3.0, w: 8.0, h: 12.0, fill: Some(color), stroke: None, radius: 0.0 },
            Prim::Rect { x: 14.0, y: 5.0, w: 8.0, h: 10.0, fill: Some(color), stroke: None, radius: 0.0 },
            Prim::Line { pts: vec![[1.0, 30.0], [10.0, 20.0], [20.0, 25.0]], color, width: 1.5, dash: false },
            Prim::Circle { cx: 10.0, cy: 20.0, r: 3.0, fill: Some(color), stroke: None },
            Prim::Circle { cx: 20.0, cy: 25.0, r: 3.0, fill: Some(color), stroke: None },
        ];

        let marks = series_selection_marks(&prims, &[color]);

        assert_eq!(marks.len(), 4);
        assert!(matches!(marks[0], SeriesSelectionMark::Shape(_)));
        assert!(matches!(marks[1], SeriesSelectionMark::Shape(_)));
        assert_eq!(marks[2..], [SeriesSelectionMark::Point(egui::pos2(10.0, 20.0)), SeriesSelectionMark::Point(egui::pos2(20.0, 25.0)),]);
    }
}
