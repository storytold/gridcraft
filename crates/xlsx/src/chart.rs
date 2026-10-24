//! DrawingML charts (`xl/charts/chartN.xml`): reading the common 2-D/3-D chart groups into the
//! model, and writing model charts (with cached series values) from scratch.

use std::fmt::Write as _;

use gridcraft_core::{RangeRef, Value, category_levels};
use gridcraft_formula::{Expr, SheetSel};
use gridcraft_model::{Chart, ChartKind, Color, DefinedName, LegendPos, LineDash, Series, Trendline, TrendlineKind, Workbook};

use crate::drawing::{dml_color, srgb};
use crate::xml::{El, esc, esc_attr, num};

// ---------------------------------------------------------------- reading

fn group_kind(g: &El) -> Option<ChartKind> {
    let grouping = g.child_val("grouping").unwrap_or("standard");
    let stacked = grouping == "stacked";
    let pct = grouping == "percentStacked";
    Some(match g.name.as_str() {
        "barChart" | "bar3DChart" => {
            let bar = g.child_val("barDir") == Some("bar");
            match (bar, stacked, pct) {
                (false, true, _) => ChartKind::ColumnStacked,
                (false, _, true) => ChartKind::ColumnStacked100,
                (false, _, _) => ChartKind::ColumnClustered,
                (true, true, _) => ChartKind::BarStacked,
                (true, _, true) => ChartKind::BarStacked100,
                (true, _, _) => ChartKind::BarClustered,
            }
        }
        "lineChart" | "line3DChart" => {
            if stacked || pct {
                ChartKind::LineStacked
            } else {
                let group_markers = g.child_val("marker").is_none_or(|v| v == "1" || v == "true");
                let all_none = g.kids("ser").all(|s| s.path(&["marker", "symbol"]).and_then(|m| m.attr("val")) == Some("none"));
                if !group_markers || (all_none && g.kids("ser").next().is_some()) { ChartKind::Line } else { ChartKind::LineMarkers }
            }
        }
        "pieChart" | "pie3DChart" | "ofPieChart" => ChartKind::Pie,
        "doughnutChart" => ChartKind::Doughnut,
        "areaChart" | "area3DChart" => {
            if stacked || pct {
                ChartKind::AreaStacked
            } else {
                ChartKind::Area
            }
        }
        "scatterChart" => {
            let no_lines = g.kids("ser").all(|s| s.path(&["spPr", "ln"]).is_some_and(|l| l.child("noFill").is_some()));
            if g.child_val("scatterStyle") == Some("marker") || (no_lines && g.kids("ser").next().is_some()) {
                ChartKind::Scatter
            } else {
                ChartKind::ScatterLines
            }
        }
        "bubbleChart" => ChartKind::Bubble,
        "radarChart" => ChartKind::Radar,
        "stockChart" => ChartKind::Stock,
        _ => return None,
    })
}

/// Excel's file notation for "this workbook" in front of a workbook-scoped defined name.
const THIS_BOOK: &str = "[0]!";

/// A chart reference (`<c:f>`) read from a file → model text: drops a leading `=` and the
/// `[0]!` in front of workbook-scoped names (`[0]!Sales` → `Sales`), which the formula parser
/// would take for a structured reference. External books (`[1]Sheet1!$A$1`), quoted sheet
/// names and string literals are kept as they are.
pub(crate) fn chart_ref_from_file(text: &str) -> String {
    let t = text.trim();
    let t = t.strip_prefix('=').unwrap_or(t);
    let mut out = String::with_capacity(t.len());
    let mut quote: Option<char> = None;
    // The last character kept: `[0]!` only counts at the start or after an operator,
    // never inside a structured reference (`T[[0]]`) or a longer name.
    let mut prev: Option<char> = None;
    let mut i = 0;
    while let Some(c) = t.get(i..).and_then(|r| r.chars().next()) {
        let rest = t.get(i..).unwrap_or("");
        if quote.is_none()
            && rest.starts_with(THIS_BOOK)
            && prev.is_none_or(|p| !(p.is_alphanumeric() || "_.\\[]'\"!$#".contains(p)))
            && rest.get(THIS_BOOK.len()..).and_then(|n| n.chars().next()).is_some_and(|n| n.is_alphabetic() || n == '_' || n == '\\')
        {
            i += THIS_BOOK.len();
            continue;
        }
        match (c, quote) {
            ('"' | '\'', None) => quote = Some(c),
            (q, Some(open)) if q == open => quote = None,
            _ => {}
        }
        out.push(c);
        prev = Some(c);
        i += c.len_utf8();
    }
    out
}

fn data_ref(e: Option<&El>) -> Option<String> {
    let e = e?;
    for k in ["numRef", "strRef", "multiLvlStrRef"] {
        if let Some(f) = e.child(k).and_then(|r| r.child("f")) {
            return Some(chart_ref_from_file(&f.text));
        }
    }
    // Literal values become an array constant.
    for k in ["numLit", "strLit"] {
        if let Some(l) = e.child(k) {
            let pts: Vec<String> = l
                .kids("pt")
                .filter_map(|p| p.child("v"))
                .map(|v| if k == "numLit" { v.text.trim().to_string() } else { format!("\"{}\"", v.text.replace('"', "\"\"")) })
                .collect();
            return Some(format!("{{{}}}", pts.join(",")));
        }
    }
    None
}

fn read_series(s: &El, automatic_accent: Option<Color>) -> Series {
    let name = s
        .child("tx")
        .and_then(|tx| tx.path(&["strRef", "f"]).map(|f| chart_ref_from_file(&f.text)).or_else(|| tx.child("v").map(|v| v.text.clone())));
    let sp = s.child("spPr");
    let color = sp
        .and_then(|p| p.child("solidFill"))
        .and_then(dml_color)
        .or_else(|| sp.and_then(|p| p.path(&["ln", "solidFill"])).and_then(dml_color))
        .or_else(|| s.path(&["marker", "spPr", "solidFill"]).and_then(dml_color))
        // Only a styled series recorded in our extension has a writer-generated accent.
        .filter(|color| Some(color) != automatic_accent.as_ref());
    Series {
        name,
        categories: data_ref(s.child("cat")).or_else(|| data_ref(s.child("xVal"))),
        values: data_ref(s.child("val")).or_else(|| data_ref(s.child("yVal"))).unwrap_or_default(),
        bubble_sizes: data_ref(s.child("bubbleSize")),
        color,
        secondary: false,
        kind: None,
        quartiles: Default::default(),
        smooth: bool_val(s, "smooth"),
        trendlines: s.kids("trendline").filter_map(read_trendline).collect(),
    }
}

const DASHES: [(LineDash, &str); 11] = [
    (LineDash::Solid, "solid"),
    (LineDash::Dot, "dot"),
    (LineDash::Dash, "dash"),
    (LineDash::LgDash, "lgDash"),
    (LineDash::DashDot, "dashDot"),
    (LineDash::LgDashDot, "lgDashDot"),
    (LineDash::LgDashDotDot, "lgDashDotDot"),
    (LineDash::SysDash, "sysDash"),
    (LineDash::SysDot, "sysDot"),
    (LineDash::SysDashDot, "sysDashDot"),
    (LineDash::SysDashDotDot, "sysDashDotDot"),
];

const EMU_PER_PT: f64 = 12_700.0;

/// A `CT_Boolean` child (`val` defaults to true).
fn bool_val(e: &El, tag: &str) -> bool {
    e.child(tag).is_some_and(|c| c.flag("val", true))
}

fn f64_val(e: &El, tag: &str) -> Option<f64> {
    e.child(tag).and_then(|c| c.attr_f64("val"))
}

/// `<c:trendline>` (ECMA-376 Part 1, 21.2.2.211). Unknown types are dropped.
fn read_trendline(t: &El) -> Option<Trendline> {
    let kind = match t.child_val("trendlineType").unwrap_or("linear") {
        "linear" => TrendlineKind::Linear,
        "exp" => TrendlineKind::Exponential,
        "log" => TrendlineKind::Logarithmic,
        "power" => TrendlineKind::Power,
        "poly" => TrendlineKind::Polynomial(t.child_val("order").and_then(|v| v.trim().parse::<u8>().ok()).unwrap_or(2).clamp(2, 6)),
        "movingAvg" => TrendlineKind::MovingAverage(t.child_val("period").and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(2).clamp(2, 255)),
        _ => return None,
    };
    let ln = t.path(&["spPr", "ln"]);
    Some(Trendline {
        kind,
        name: t.child("name").map(|n| n.text.clone()),
        forward: Trendline::clamp_forecast(f64_val(t, "forward").unwrap_or(0.0)),
        backward: Trendline::clamp_forecast(f64_val(t, "backward").unwrap_or(0.0)),
        intercept: f64_val(t, "intercept"),
        display_equation: bool_val(t, "dispEq"),
        display_r_squared: bool_val(t, "dispRSqr"),
        color: ln.and_then(|l| l.child("solidFill")).and_then(dml_color),
        width: ln.and_then(|l| l.attr_f64("w")).map(|w| (w / EMU_PER_PT) as f32).filter(|w| w.is_finite() && *w >= 0.0),
        dash: ln.and_then(|l| l.child("prstDash")).and_then(|d| d.attr("val")).and_then(|v| DASHES.iter().find(|(_, n)| *n == v).map(|(d, _)| *d)),
    })
}

fn rich_title(t: &El) -> Option<String> {
    let tx = t.child("tx")?;
    if let Some(rich) = tx.child("rich") {
        let paras: Vec<String> =
            rich.kids("p").map(|p| p.kids("r").filter_map(|r| r.child("t")).map(|t| t.text.as_str()).collect::<String>()).collect();
        let s = paras.join("\n");
        return if s.is_empty() { None } else { Some(s) };
    }
    if let Some(sr) = tx.child("strRef") {
        if let Some(v) = sr.path(&["strCache", "pt", "v"]) {
            return Some(v.text.clone());
        }
        return sr.child("f").map(|f| f.text.trim().to_string());
    }
    None
}

fn axis_ids(g: &El) -> Vec<String> {
    g.kids("axId").filter_map(|a| a.attr("val")).map(str::to_string).collect()
}

pub fn read_chart(root: &El) -> Option<Chart> {
    let chart = root.child("chart")?;
    let plot = chart.child("plotArea")?;
    let groups: Vec<(&El, ChartKind)> = plot.children.iter().filter_map(|g| group_kind(g).map(|k| (g, k))).collect();
    let (first, first_kind) = *groups.first()?;
    let first_axes = axis_ids(first);
    let custom_format = read_custom_format(root);
    let mut series = vec![];
    for (gi, (g, k)) in groups.iter().enumerate() {
        for s in g.kids("ser") {
            let automatic_accent = s
                .child_val("idx")
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|index| custom_format.as_ref().is_some_and(|format| format.series.contains_key(index)))
                .map(|index| Color::Theme(4 + (index % 6) as u8, 0));
            let mut ser = read_series(s, automatic_accent);
            if groups.len() > 1 {
                // Every series of a combo chart keeps its own group's kind, the first group's included.
                ser.kind = Some(*k);
            }
            if gi > 0 {
                ser.secondary = axis_ids(g) != first_axes;
            }
            series.push(ser);
        }
    }
    let kind = if groups.len() > 1 { ChartKind::Combo } else { first_kind };
    let show_val = |e: &El| e.child("dLbls").is_some_and(|d| bool_val(d, "showVal"));
    let data_labels = groups.iter().any(|(g, _)| show_val(g) || g.kids("ser").any(show_val));
    let val_axes: Vec<&El> = plot.kids("valAx").collect();
    let cat_ax = plot.child("catAx").or_else(|| plot.child("dateAx"));
    let scatter = matches!(first_kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble);
    let (x_ax, y_ax) = if scatter {
        let x = val_axes.iter().find(|a| matches!(a.child_val("axPos"), Some("b") | Some("t"))).copied();
        let y = val_axes.iter().find(|a| matches!(a.child_val("axPos"), Some("l") | Some("r"))).copied();
        (x, y)
    } else if kind != ChartKind::Combo && matches!(first_kind, ChartKind::BarClustered | ChartKind::BarStacked | ChartKind::BarStacked100) {
        (val_axes.first().copied(), cat_ax)
    } else {
        (cat_ax, val_axes.first().copied())
    };
    let legend = match chart.child("legend") {
        None => LegendPos::None,
        Some(l) => match l.child_val("legendPos").unwrap_or("r") {
            "b" => LegendPos::Bottom,
            "t" => LegendPos::Top,
            "l" => LegendPos::Left,
            _ => LegendPos::Right,
        },
    };
    let format = read_format(root, plot, &series);
    Some(Chart {
        id: 0,
        kind,
        anchor: Default::default(),
        title: chart.child("title").and_then(rich_title),
        series,
        legend,
        data_labels,
        gridlines: if scatter { y_ax } else { val_axes.first().copied() }.is_some_and(|a| a.child("majorGridlines").is_some()),
        style: root.child_val("style").and_then(|v| v.parse().ok()).unwrap_or(2),
        x_title: x_ax.and_then(|a| a.child("title")).and_then(rich_title),
        y_title: y_ax.and_then(|a| a.child("title")).and_then(rich_title),
        source: None,
        by_rows: false,
        pivot: root.path(&["pivotSource", "name"]).map(|n| strip_book(n.text.trim()).to_string()).filter(|n| !n.is_empty()),
        format,
    })
}

fn read_format(root: &El, plot: &El, series: &[Series]) -> gridcraft_model::ChartFormat {
    let scatter = plot
        .children
        .iter()
        .find_map(group_kind)
        .is_some_and(|kind| matches!(kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble));
    let mut format = read_custom_format(root).unwrap_or_else(|| gridcraft_model::ChartFormat {
        legend_overlay: root.path(&["chart", "legend"]).is_some_and(|l| bool_val(l, "overlay")),
        ..Default::default()
    });
    format.legend_overlay = root.path(&["chart", "legend"]).is_some_and(|legend| bool_val(legend, "overlay"));
    let mut seen_x = false;
    let mut seen_y = false;
    for a in plot.children.iter().filter(|a| matches!(a.name.as_str(), "catAx" | "dateAx" | "valAx")) {
        let x = matches!(a.child_val("axPos"), Some("b") | Some("t"));
        let seen = if x { &mut seen_x } else { &mut seen_y };
        if *seen {
            continue;
        }
        *seen = true;
        let options = if x { &mut format.x_axis } else { &mut format.y_axis };
        if let Some(scale) = a.child("scaling") {
            options.minimum = scale.child_val("min").and_then(|v| v.parse().ok()).or(options.minimum);
            options.maximum = scale.child_val("max").and_then(|v| v.parse().ok()).or(options.maximum);
            options.reversed = scale.child_val("orientation") == Some("maxMin");
            if scale.child("logBase").is_some() {
                options.logarithmic = true;
            }
        }
        options.major_unit = a.child_val("majorUnit").and_then(|v| v.parse().ok()).or(options.major_unit);
        options.major_tick = a.child_val("majorTickMark").filter(|v| matches!(*v, "in" | "out" | "cross")).map(str::to_string);
        options.minor_tick = a.child_val("minorTickMark").filter(|v| matches!(*v, "in" | "out" | "cross")).map(str::to_string);
        options.minor_unit = a.child_val("minorUnit").and_then(|v| v.parse().ok()).or(options.minor_unit);
        let surface = read_surface(a.child("spPr"));
        options.line.color = surface.border.filter(|color| *color != [217, 217, 217, 255]);
        options.line.width = surface.border_width;
        if a.path(&["spPr", "ln", "noFill"]).is_some() {
            options.line.width = Some(0.0);
        }
        let category_grid = if scatter { x } else { matches!(a.name.as_str(), "catAx" | "dateAx") };
        if let Some(grid) = a.child("majorGridlines") {
            let surface = read_surface(grid.child("spPr"));
            let style = gridcraft_model::ChartStroke {
                color: surface.border.filter(|color| *color != [217, 217, 217, 255]),
                width: surface.border_width.filter(|width| *width != 1.0),
            };
            if category_grid {
                format.vertical_gridline = style;
            } else {
                format.horizontal_gridline = style;
            }
        } else if category_grid {
            format.vertical_gridline = Default::default();
        } else {
            format.horizontal_gridline = Default::default();
        }
        options.number_format =
            a.child("numFmt").filter(|n| n.attr("sourceLinked") == Some("0")).and_then(|n| n.attr("formatCode")).map(str::to_string);
    }
    format.vertical_gridlines = plot
        .children
        .iter()
        .find(|axis| {
            if scatter {
                axis.name == "valAx" && matches!(axis.child_val("axPos"), Some("b" | "t"))
            } else {
                matches!(axis.name.as_str(), "catAx" | "dateAx")
            }
        })
        .is_some_and(|axis| axis.child("majorGridlines").is_some());
    // Extension keys are native c:idx values. Combo groups can put series in a
    // different XML order, which is the order used by the imported model.
    let custom_series = std::mem::take(&mut format.series);
    let mut series_index = 0;
    for group in plot.children.iter().filter(|g| group_kind(g).is_some()) {
        let Some(kind) = group_kind(group) else { continue };
        let bar_area = matches!(
            kind,
            ChartKind::ColumnClustered
                | ChartKind::ColumnStacked
                | ChartKind::ColumnStacked100
                | ChartKind::BarClustered
                | ChartKind::BarStacked
                | ChartKind::BarStacked100
                | ChartKind::Area
                | ChartKind::AreaStacked
        );
        for ser in group.kids("ser") {
            let custom = ser.child_val("idx").and_then(|value| value.parse::<usize>().ok()).and_then(|index| custom_series.get(&index));
            let ln = ser.path(&["spPr", "ln"]);
            let marker = ser.child("marker");
            let native_smooth = bool_val(ser, "smooth");
            let width = ln.and_then(|l| l.attr("w")).and_then(|w| w.parse::<f32>().ok()).map(|w| w / 12700.0);
            let series_color = series.get(series_index).and_then(|series| series.color.as_ref());
            let series_fill = ser.path(&["spPr", "solidFill"]);
            let series_fill_color = series_fill.and_then(dml_color);
            let fill_color = series_fill.and_then(read_rgba).filter(|fill| {
                custom.is_some_and(|style| style.fill_color.is_some()) || fill[3] != 255 || series_fill_color.as_ref() != series_color
            });
            let line_fill = ln.and_then(|line| line.child("solidFill"));
            let line_color = line_fill.and_then(read_rgba).filter(|fill| {
                bar_area
                    || custom.is_some_and(|style| style.line_color.is_some())
                    || fill[3] != 255
                    || line_fill.and_then(dml_color).as_ref() != series_color
            });
            let marker_no_fill = marker.and_then(|m| m.path(&["spPr", "noFill"])).is_some();
            let marker_fill_el = marker.and_then(|m| m.path(&["spPr", "solidFill"]));
            let marker_fill_color = marker_fill_el.and_then(dml_color);
            let marker_fill = marker_fill_el.and_then(read_rgba).filter(|fill| {
                custom.is_some_and(|style| style.marker_fill.is_some()) || fill[3] != 255 || marker_fill_color.as_ref() != series_color
            });
            let marker_border_fill = marker.and_then(|m| m.path(&["spPr", "ln", "solidFill"]));
            let marker_border_color = marker_border_fill.and_then(read_rgba).filter(|color| *color != [255, 255, 255, 255]);
            let marker_border_width = marker
                .and_then(|m| m.path(&["spPr", "ln"]))
                .and_then(|l| l.attr("w"))
                .and_then(|w| w.parse::<f32>().ok())
                .map(|w| w / 12700.0)
                .filter(|width| *width != 0.75 || marker_no_fill || marker_border_color.is_some());
            let default_marker = match kind {
                ChartKind::Line | ChartKind::LineStacked | ChartKind::ScatterLines => Some("none"),
                ChartKind::LineMarkers | ChartKind::Scatter => Some("circle"),
                _ => None,
            };
            let mut spec = gridcraft_model::ChartSeriesFormat {
                no_line: !matches!(kind, ChartKind::Scatter) && ln.is_some_and(|l| l.child("noFill").is_some()),
                dashed: ln.and_then(|l| l.child_val("prstDash")).is_some_and(|v| v != "solid"),
                marker: marker
                    .and_then(|m| m.child_val("symbol"))
                    .filter(|v| matches!(*v, "none" | "circle" | "square" | "diamond" | "triangle" | "x" | "plus" | "star" | "dash"))
                    .filter(|v| Some(*v) != default_marker)
                    .map(|v| if v == "x" { "cross".into() } else { v.into() }),
                marker_size: marker.and_then(|m| m.child_val("size")).and_then(|v| v.parse().ok()).filter(|size| {
                    // Size 7 is generated only for a styled series. Keep a native-only
                    // size 7, and any size explicitly recorded in our extension.
                    custom.is_some_and(|style| style.marker_size.is_some())
                        || ((*size != 7.0 || custom.is_none()) && (!matches!(kind, ChartKind::Scatter) || *size != 5.0))
                }),
                marker_no_fill,
                marker_fill,
                marker_border_color,
                marker_border_width,
                line_color: line_color.filter(|color| {
                    // Only a writer-generated border is automatic. A native-only dark
                    // border or an explicitly configured dark border must survive.
                    !bar_area || *color != [38, 38, 38, 255] || custom.is_none_or(|style| style.line_color.is_some())
                }),
                fill_color,
                ..Default::default()
            };
            if bar_area {
                // The bar/area writer uses border_width, not line_width. Keep the latter
                // as an extension-only field and normalize the automatic zero-width border.
                spec.line_width = custom.and_then(|style| style.line_width);
                spec.border_width = width.filter(|width| *width != 0.0 || custom.is_some_and(|style| style.border_width == Some(0.0)));
                if ln.is_some_and(|line| line.child("noFill").is_some()) {
                    // A hidden border has no native colour; retain its configured colour
                    // so showing the border later does not discard the user's choice.
                    spec.line_color = custom.and_then(|style| style.line_color);
                }
                if width == Some(0.0) {
                    spec.no_line = custom.is_some_and(|style| style.no_line);
                }
            } else if matches!(kind, ChartKind::Scatter) {
                // A marker-only scatter group encodes its structural no-line default in `spPr`.
                // Preserve its marker-only kind instead of turning that XML into a line override.
                spec.line_width = None;
            } else {
                spec.line_width = width.filter(|value| *value != 2.25 && !spec.no_line);
            }
            if custom.is_some_and(|style| style.smooth.is_some()) {
                spec.smooth = Some(native_smooth);
            }
            if spec != gridcraft_model::ChartSeriesFormat::default() {
                format.series.insert(series_index, spec);
            }
            series_index += 1;
        }
    }
    let bar_chart = plot.children.iter().find(|g| g.name == "barChart");
    format.gap_width = bar_chart.and_then(|group| group.child_val("gapWidth")).and_then(|s| s.parse::<f32>().ok()).filter(|v| *v != 150.0);
    format.overlap = bar_chart.and_then(|group| group.child_val("overlap")).and_then(|s| s.parse().ok());
    merge_native_surface(&mut format.plot_area, plot.child("spPr"));
    merge_native_surface(&mut format.chart_area, root.child("spPr"));
    format
}

fn read_rgba(fill: &El) -> Option<[u8; 4]> {
    let color = fill.child("srgbClr")?;
    let rgb = Color::from_hex(color.attr("val")?)?.resolve(&Default::default())?;
    let alpha = color.child_val("alpha").and_then(|v| v.parse::<f32>().ok()).unwrap_or(100000.0).clamp(0.0, 100000.0);
    Some([rgb[0], rgb[1], rgb[2], (alpha * 255.0 / 100000.0).round() as u8])
}

pub(crate) fn read_surface(sp: Option<&El>) -> gridcraft_model::ChartSurface {
    let mut surface = gridcraft_model::ChartSurface::default();
    if let Some(sp) = sp {
        surface.fill = sp.child("solidFill").and_then(read_rgba);
        surface.border = sp.path(&["ln", "solidFill"]).and_then(read_rgba);
        surface.border_width = sp.child("ln").and_then(|l| l.attr("w")).and_then(|v| v.parse::<f32>().ok()).map(|v| v / 12700.0);
        if sp.path(&["ln", "noFill"]).is_some() {
            surface.border_width = Some(0.0);
        }
        if sp.child("noFill").is_some() {
            surface.fill = Some([0, 0, 0, 0]);
        }
    }
    surface
}

/// Merges explicitly present native shape properties over a chart-format extension.
pub(crate) fn merge_native_surface(target: &mut gridcraft_model::ChartSurface, sp: Option<&El>) {
    let Some(sp) = sp else { return };
    let native = read_surface(Some(sp));
    if native.fill.is_some() || sp.child("noFill").is_some() {
        target.fill = native.fill;
    }
    if sp.child("ln").is_some() {
        if native.border.is_some() {
            target.border = native.border;
        }
        if native.border_width.is_some() {
            target.border_width = native.border_width;
        }
    }
}

fn rgba_xml(c: [u8; 4]) -> String {
    format!("<a:srgbClr val=\"{:02X}{:02X}{:02X}\"><a:alpha val=\"{}\"/></a:srgbClr>", c[0], c[1], c[2], (c[3] as u32 * 100000 + 127) / 255)
}

pub(crate) fn surface_xml(style: &gridcraft_model::ChartSurface, prefix: &str) -> String {
    if *style == Default::default() {
        return String::new();
    }
    let mut s = format!("<{prefix}:spPr>");
    if let Some(fill) = style.fill {
        let _ = write!(s, "<a:solidFill>{}</a:solidFill>", rgba_xml(fill));
    }
    if style.border.is_some() || style.border_width.is_some() {
        let w = style.border_width.unwrap_or(1.0);
        let _ = write!(s, "<a:ln w=\"{}\">", (w * 12700.0).round() as u32);
        if w == 0.0 {
            s.push_str("<a:noFill/>");
        } else {
            let _ = write!(s, "<a:solidFill>{}</a:solidFill>", rgba_xml(style.border.unwrap_or([217, 217, 217, 255])));
        }
        s.push_str("</a:ln>");
    }
    let _ = write!(s, "</{prefix}:spPr>");
    s
}

#[cfg(test)]
mod native_merge_tests {
    use super::*;

    #[test]
    fn omitted_native_axis_scale_values_preserve_extension_values() {
        let custom = gridcraft_model::ChartFormat {
            x_axis: gridcraft_model::ChartAxis {
                minimum: Some(2.0),
                maximum: Some(8.0),
                major_unit: Some(2.0),
                minor_unit: Some(0.5),
                logarithmic: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let root_xml = format!("<chartSpace>{}</chartSpace>", format_extension(&custom, "c"));
        let root = crate::xml::parse(root_xml.as_bytes()).unwrap();
        let plot =
            crate::xml::parse(b"<plotArea><catAx><axPos val=\"b\"/><scaling><orientation val=\"minMax\"/></scaling></catAx></plotArea>").unwrap();

        let loaded = read_format(&root, &plot, &[]);

        assert_eq!(loaded.x_axis, custom.x_axis);
    }

    #[test]
    fn native_surface_merges_only_components_present_in_xml() {
        let mut surface = gridcraft_model::ChartSurface { fill: Some([10, 20, 30, 255]), border: Some([40, 50, 60, 255]), border_width: Some(3.0) };
        let native = crate::xml::parse(b"<spPr><ln w=\"25400\"/></spPr>").unwrap();

        merge_native_surface(&mut surface, Some(&native));

        assert_eq!(surface.fill, Some([10, 20, 30, 255]));
        assert_eq!(surface.border, Some([40, 50, 60, 255]));
        assert_eq!(surface.border_width, Some(2.0));
    }

    #[test]
    fn native_no_fill_border_keeps_extension_color_and_hides_width() {
        let color = [40, 50, 60, 255];
        let mut surface = gridcraft_model::ChartSurface { border: Some(color), border_width: Some(3.0), ..Default::default() };
        let native = crate::xml::parse(b"<spPr><ln w=\"0\"><noFill/></ln></spPr>").unwrap();

        merge_native_surface(&mut surface, Some(&native));

        assert_eq!(surface.border, Some(color));
        assert_eq!(surface.border_width, Some(0.0));
    }
}

pub(crate) fn read_custom_format(root: &El) -> Option<gridcraft_model::ChartFormat> {
    let ext = root.child("extLst")?.kids("ext").find(|e| e.attr("uri") == Some("{CE01AC23-6FC7-453A-BE98-475249444346}"))?.child("format")?;
    serde_json::from_str(&ext.text).ok()
}

pub(crate) fn format_extension(format: &gridcraft_model::ChartFormat, prefix: &str) -> String {
    serde_json::to_string(format).map(|format|format!("<{prefix}:extLst><{prefix}:ext uri=\"{{CE01AC23-6FC7-453A-BE98-475249444346}}\"><gc:format xmlns:gc=\"urn:gridcraft:chart-format:1\">{}</gc:format></{prefix}:ext></{prefix}:extLst>",esc(&format))).unwrap_or_default()
}

// ---------------------------------------------------------------- writing

#[derive(Clone, Copy, PartialEq)]
enum G {
    Bar { bar: bool, grouping: &'static str },
    Line { stacked: bool, markers: bool },
    Pie,
    Doughnut,
    Area { stacked: bool },
    Scatter { lines: bool },
    Bubble,
    Radar,
    Stock,
}

fn group_for(k: ChartKind) -> G {
    match k {
        ChartKind::ColumnStacked => G::Bar { bar: false, grouping: "stacked" },
        ChartKind::ColumnStacked100 => G::Bar { bar: false, grouping: "percentStacked" },
        ChartKind::BarClustered => G::Bar { bar: true, grouping: "clustered" },
        ChartKind::BarStacked => G::Bar { bar: true, grouping: "stacked" },
        ChartKind::BarStacked100 => G::Bar { bar: true, grouping: "percentStacked" },
        ChartKind::Line => G::Line { stacked: false, markers: false },
        ChartKind::LineMarkers => G::Line { stacked: false, markers: true },
        ChartKind::LineStacked => G::Line { stacked: true, markers: false },
        ChartKind::Pie => G::Pie,
        ChartKind::Doughnut => G::Doughnut,
        ChartKind::Area => G::Area { stacked: false },
        ChartKind::AreaStacked => G::Area { stacked: true },
        ChartKind::Scatter => G::Scatter { lines: false },
        ChartKind::ScatterLines => G::Scatter { lines: true },
        ChartKind::Bubble => G::Bubble,
        ChartKind::Radar => G::Radar,
        ChartKind::Stock => G::Stock,
        // Combo charts start from clustered columns; chartex kinds have their own part.
        _ => G::Bar { bar: false, grouping: "clustered" },
    }
}

const MAX_CACHE: u64 = 100_000;

/// The sheet and range a same-workbook reference (`Sheet1!$B$2:$B$9`) names.
fn ref_target(wb: &Workbook, sheet_idx: usize, f: &str) -> Option<(usize, RangeRef)> {
    let e = gridcraft_formula::parse(f).ok()?;
    let Expr::Ref(r) = e else { return None };
    let si = match &r.sheet {
        SheetSel::Current => sheet_idx,
        SheetSel::Named(n) => wb.sheet_index(n)?,
        SheetSel::Span(..) => return None,
    };
    Some((si, r.range()))
}

/// Values of a same-workbook reference (`Sheet1!$B$2:$B$9`), row-major.
pub(crate) fn ref_values(wb: &Workbook, sheet_idx: usize, f: &str) -> Option<Vec<Value>> {
    let (si, mut range) = ref_target(wb, sheet_idx, f)?;
    let sheet = wb.sheet(si)?;
    // Whole columns/rows: cut the far end at the used range. The start stays put, so point indices keep matching the reference.
    if let Some(u) = sheet.used_range() {
        if range.is_full_cols() {
            range.end.row = range.end.row.min(u.end.row.max(range.start.row));
        }
        if range.is_full_rows() {
            range.end.col = range.end.col.min(u.end.col.max(range.start.col));
        }
    }
    if range.count() > MAX_CACHE {
        return None;
    }
    Some(range.iter().map(|c| sheet.value(c)).collect())
}

fn is_ref(f: &str) -> bool {
    matches!(gridcraft_formula::parse(f), Ok(Expr::Ref(_)))
}

/// A chart reference that is a name: its sheet when qualified (`Sheet1!Sales`), the bare
/// name, and the definition formulas would find from sheet `si` (none when undefined). `None`
/// when it isn't a name, or names a sheet the workbook doesn't have.
fn chart_name<'a>(wb: &'a Workbook, si: usize, f: &str) -> Option<(Option<usize>, String, Option<&'a DefinedName>)> {
    let Ok(Expr::Name(n)) = gridcraft_formula::parse(f.trim()) else { return None };
    match n.split_once('!') {
        Some((s, bare)) => {
            let i = wb.sheet_index(s.trim_matches('\''))?;
            Some((Some(i), bare.to_string(), wb.name(bare, i)))
        }
        None => {
            let def = wb.name(&n, si);
            Some((None, n, def))
        }
    }
}

/// A chart reference for `<c:f>`. Excel needs a name qualified there: with a bare `Name` it
/// draws the whole chart blank, with no repair prompt (observed in Excel for Windows,
/// Microsoft 365). A workbook-scoped name is written `[0]!Name`, as Excel writes it, a
/// sheet-scoped one `Sheet!Name`; an undefined name gets `[0]!`, as the file it came from had.
fn chart_ref_to_file(wb: &Workbook, si: usize, f: &str) -> String {
    let on_sheet = |i: usize, n: &str| wb.sheet(i).map(|sh| format!("{}!{n}", gridcraft_formula::quote_sheet(&sh.name)));
    match chart_name(wb, si, f) {
        Some((Some(i), n, _)) => on_sheet(i, &n).unwrap_or_else(|| crate::fmla::text_to_file(f)),
        Some((None, n, def)) => def.and_then(|d| d.scope).and_then(|i| on_sheet(i, &n)).unwrap_or_else(|| format!("{THIS_BOOK}{n}")),
        None => crate::fmla::text_to_file(f),
    }
}

/// A series name that refers to a defined name (as opposed to literal text that parses as one).
fn is_defined_name(wb: &Workbook, si: usize, f: &str) -> bool {
    chart_name(wb, si, f).is_some_and(|(_, _, d)| d.is_some())
}

fn num_data(tag: &str, f: Option<&str>, wb: &Workbook, si: usize) -> String {
    let f = f.unwrap_or("").trim();
    if f.is_empty() {
        return format!("<c:{tag}><c:numLit><c:ptCount val=\"0\"/></c:numLit></c:{tag}>");
    }
    if f.starts_with('{') {
        let vals: Vec<&str> = f.trim_matches(|c| c == '{' || c == '}').split([',', ';']).collect();
        let mut s = format!("<c:{tag}><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>", vals.len());
        for (i, v) in vals.iter().enumerate() {
            if let Ok(n) = v.trim().parse::<f64>() {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", num(n));
            }
        }
        let _ = write!(s, "</c:numLit></c:{tag}>");
        return s;
    }
    let mut s = format!("<c:{tag}><c:numRef><c:f>{}</c:f>", esc(&chart_ref_to_file(wb, si, f)));
    if let Some(vals) = ref_values(wb, si, f) {
        let _ = write!(s, "<c:numCache><c:formatCode>General</c:formatCode><c:ptCount val=\"{}\"/>", vals.len());
        for (i, v) in vals.iter().enumerate() {
            if let Value::Number(n) = v {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", num(*n));
            }
        }
        s.push_str("</c:numCache>");
    }
    let _ = write!(s, "</c:numRef></c:{tag}>");
    s
}

/// Splits the inside of an array constant on `,` and `;` outside double quotes; inside quotes `""` is a literal quote.
/// Unquoted items are trimmed; quoted ones keep the text between the quotes and drop the spaces around them.
fn split_array_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut in_quote = false;
    let mut chars = inner.chars().peekable();
    let finish = |cur: &mut String, quoted: &mut bool| {
        let text = std::mem::take(cur);
        let item = if *quoted { text } else { text.trim().to_string() };
        *quoted = false;
        item
    };
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quote && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => {
                if !quoted && cur.trim().is_empty() {
                    cur.clear();
                }
                in_quote = !in_quote;
                quoted = true;
            }
            ',' | ';' if !in_quote => items.push(finish(&mut cur, &mut quoted)),
            c if quoted && !in_quote && c.is_whitespace() => {}
            _ => cur.push(c),
        }
    }
    items.push(finish(&mut cur, &mut quoted));
    items
}

fn str_data(tag: &str, f: &str, wb: &Workbook, si: usize) -> String {
    let f = f.trim();
    if f.starts_with('{') {
        let vals = split_array_items(f.trim_matches(|c| c == '{' || c == '}'));
        let mut s = format!("<c:{tag}><c:strLit><c:ptCount val=\"{}\"/>", vals.len());
        for (i, v) in vals.iter().enumerate() {
            let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(v));
        }
        let _ = write!(s, "</c:strLit></c:{tag}>");
        return s;
    }
    let mut s = format!("<c:{tag}><c:strRef><c:f>{}</c:f>", esc(&chart_ref_to_file(wb, si, f)));
    if let Some(vals) = ref_values(wb, si, f) {
        let _ = write!(s, "<c:strCache><c:ptCount val=\"{}\"/>", vals.len());
        for (i, v) in vals.iter().enumerate() {
            if !v.is_empty() {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(&v.display()));
            }
        }
        s.push_str("</c:strCache>");
    }
    let _ = write!(s, "</c:strRef></c:{tag}>");
    s
}

/// `<c:cat>` of a series: a string reference, or a multi-level one (`multiLvlStrRef`) when the
/// category range has more columns than a column of values (or more rows than a row of values:
/// year beside month). Its cache lists the innermost level first, then each outer level with
/// only the cells that start a group, as Excel writes it.
fn cat_data(f: &str, values: &str, wb: &Workbook, si: usize) -> String {
    multi_level_cat(f.trim(), values, wb, si).unwrap_or_else(|| str_data("cat", f, wb, si))
}

fn multi_level_cat(f: &str, values: &str, wb: &Workbook, si: usize) -> Option<String> {
    let (sheet, range) = ref_target(wb, si, f)?;
    let value_range = ref_target(wb, si, values).map(|(_, r)| r);
    let levels = category_levels(&range, value_range.as_ref())?;
    if range.count() > MAX_CACHE {
        return None;
    }
    let sheet = wb.sheet(sheet)?;
    let (w, h) = (range.width() as usize, range.height() as usize);
    let text = |row: usize, col: usize| -> String {
        let v = range.start.offset(row as i64, col as i64).map(|c| sheet.value(c));
        v.filter(|v| !v.is_empty()).map(|v| v.display()).unwrap_or_default()
    };
    // The innermost level (nearest the values), then the outer ones.
    let (n, lvls): (usize, Vec<Vec<String>>) = if levels.by_columns {
        (h, (0..=levels.outer as usize).filter_map(|k| (w - 1).checked_sub(k)).map(|c| (0..h).map(|r| text(r, c)).collect()).collect())
    } else {
        (w, (0..=levels.outer as usize).filter_map(|k| (h - 1).checked_sub(k)).map(|r| (0..w).map(|c| text(r, c)).collect()).collect())
    };
    let mut s = format!("<c:cat><c:multiLvlStrRef><c:f>{}</c:f><c:multiLvlStrCache><c:ptCount val=\"{n}\"/>", esc(&crate::fmla::text_to_file(f)));
    for lvl in &lvls {
        s.push_str("<c:lvl>");
        for (i, v) in lvl.iter().enumerate() {
            if !v.is_empty() {
                let _ = write!(s, "<c:pt idx=\"{i}\"><c:v>{}</c:v></c:pt>", esc(v));
            }
        }
        s.push_str("</c:lvl>");
    }
    s.push_str("</c:multiLvlStrCache></c:multiLvlStrRef></c:cat>");
    Some(s)
}

fn series_xml(s: &Series, idx: usize, g: G, wb: &Workbook, si: usize, spec: Option<&gridcraft_model::ChartSeriesFormat>) -> String {
    let smooth = spec.and_then(|s| s.smooth).unwrap_or(s.smooth);
    let mut x = format!("<c:ser><c:idx val=\"{idx}\"/><c:order val=\"{idx}\"/>");
    if let Some(n) = &s.name {
        if is_ref(n) || is_defined_name(wb, si, n) {
            x.push_str(&str_data("tx", n, wb, si));
        } else {
            let lit = n.trim().strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(n);
            let _ = write!(x, "<c:tx><c:v>{}</c:v></c:tx>", esc(lit));
        }
    }
    let color = s.color.as_ref().and_then(|c: &Color| srgb(c, &wb.theme));
    let line_like = matches!(g, G::Line { .. } | G::Radar | G::Scatter { lines: true });
    if let Some(spec) = spec {
        let c = color.clone().unwrap_or_else(|| format!("<a:schemeClr val=\"accent{}\"/>", idx % 6 + 1));
        let width = if matches!(g, G::Bar { .. } | G::Area { .. }) { spec.border_width.unwrap_or(0.0) } else { spec.line_width.unwrap_or(2.25) };
        let fill = spec.fill_color.map(rgba_xml).unwrap_or_else(|| c.clone());
        let line = spec.line_color.map(rgba_xml).unwrap_or_else(|| c.clone());
        let _ = write!(x, "<c:spPr><a:solidFill>{fill}</a:solidFill><a:ln w=\"{}\">", (width * 12700.0).round() as u32);
        if spec.no_line || width == 0.0 || (g == (G::Scatter { lines: false }) && spec.line_width.is_none()) {
            x.push_str("<a:noFill/>");
        } else {
            if matches!(g, G::Bar { .. } | G::Area { .. }) && spec.line_color.is_none() {
                x.push_str("<a:solidFill><a:srgbClr val=\"262626\"/></a:solidFill>");
            } else {
                let _ = write!(x, "<a:solidFill>{line}</a:solidFill>");
            }
        }
        let _ = write!(x, "<a:prstDash val=\"{}\"/></a:ln></c:spPr>", if spec.dashed { "dash" } else { "solid" });
        if matches!(g, G::Line { .. } | G::Scatter { .. }) {
            // Styling a line must retain its marker-free default. Markers are opt-in on
            // ScatterLines, just as they are on a line chart without markers.
            let marker = spec.marker.as_deref().unwrap_or(if matches!(g, G::Line { markers: false, .. } | G::Scatter { lines: true }) {
                "none"
            } else {
                "circle"
            });
            let marker = match marker {
                "none" => "none",
                "circle" => "circle",
                "square" => "square",
                "diamond" => "diamond",
                "triangle" => "triangle",
                "cross" | "x" => "x",
                "plus" => "plus",
                "star" => "star",
                "dash" => "dash",
                _ => "circle",
            };
            let fill = if spec.marker_no_fill {
                "<a:noFill/>".into()
            } else {
                format!("<a:solidFill>{}</a:solidFill>", spec.marker_fill.map(rgba_xml).unwrap_or_else(|| c.clone()))
            };
            let border_width = spec
                .marker_border_width
                .or(spec.border_width)
                .unwrap_or(if spec.marker_border_color.is_some() || spec.marker_no_fill { 1.0 } else { 0.75 });
            let border = if border_width == 0.0 {
                "<a:noFill/>".into()
            } else {
                format!(
                    "<a:solidFill>{}</a:solidFill>",
                    rgba_xml(spec.marker_border_color.unwrap_or(if spec.marker_no_fill { [38, 38, 38, 255] } else { [255, 255, 255, 255] }))
                )
            };
            let _ = write!(
                x,
                "<c:marker><c:symbol val=\"{marker}\"/><c:size val=\"{}\"/><c:spPr>{fill}<a:ln w=\"{}\">{border}</a:ln></c:spPr></c:marker>",
                spec.marker_size.unwrap_or(7.0).round() as u32,
                (border_width * 12700.0).round() as u32
            );
        }
    } else {
        match (&color, line_like) {
            (Some(c), true) => {
                let _ = write!(x, "<c:spPr><a:ln w=\"28575\" cap=\"rnd\"><a:solidFill>{c}</a:solidFill><a:round/></a:ln></c:spPr>");
            }
            (Some(c), false) if g == (G::Scatter { lines: false }) => {
                let _ = write!(
                    x,
                    "<c:spPr><a:ln w=\"19050\"><a:noFill/></a:ln></c:spPr><c:marker><c:symbol val=\"circle\"/><c:size val=\"5\"/><c:spPr><a:solidFill>{c}</a:solidFill></c:spPr></c:marker>"
                );
            }
            (Some(c), false) => {
                let _ = write!(x, "<c:spPr><a:solidFill>{c}</a:solidFill></c:spPr>");
            }
            (None, _) if g == (G::Scatter { lines: false }) || g == G::Stock => x.push_str("<c:spPr><a:ln w=\"19050\"><a:noFill/></a:ln></c:spPr>"),
            _ => {}
        }
    }
    match g {
        G::Bar { .. } => x.push_str("<c:invertIfNegative val=\"0\"/>"),
        G::Line { markers: false, .. } | G::Scatter { lines: true } | G::Stock if spec.is_none() => {
            x.push_str("<c:marker><c:symbol val=\"none\"/></c:marker>")
        }
        G::Bubble => x.push_str("<c:invertIfNegative val=\"0\"/>"),
        _ => {}
    }
    // Trendlines sit between dLbls and the data in CT_LineSer, CT_BarSer, CT_AreaSer,
    // CT_ScatterSer and CT_BubbleSer; pie and radar series have none.
    if !matches!(g, G::Pie | G::Doughnut | G::Radar) {
        for t in &s.trendlines {
            x.push_str(&trendline_xml(t, wb));
        }
    }
    match g {
        G::Scatter { .. } | G::Bubble => {
            if let Some(c) = &s.categories {
                x.push_str(&num_data("xVal", Some(c), wb, si));
            }
            x.push_str(&num_data("yVal", Some(&s.values), wb, si));
            if g == G::Bubble {
                x.push_str(&num_data("bubbleSize", s.bubble_sizes.as_deref().or(Some(&s.values)), wb, si));
                x.push_str("<c:bubble3D val=\"0\"/>");
            } else {
                let _ = write!(x, "<c:smooth val=\"{}\"/>", smooth as u8);
            }
        }
        _ => {
            if let Some(c) = &s.categories {
                x.push_str(&cat_data(c, &s.values, wb, si));
            }
            x.push_str(&num_data("val", Some(&s.values), wb, si));
            if matches!(g, G::Line { .. }) {
                let _ = write!(x, "<c:smooth val=\"{}\"/>", smooth as u8);
            }
        }
    }
    x.push_str("</c:ser>");
    x
}

/// `<c:trendline>` with children in schema order: name, spPr, trendlineType, order, period,
/// forward, backward, intercept, dispRSqr, dispEq, trendlineLbl.
fn trendline_xml(t: &Trendline, wb: &Workbook) -> String {
    let mut x = String::from("<c:trendline>");
    if let Some(n) = &t.name {
        let _ = write!(x, "<c:name>{}</c:name>", esc(n));
    }
    let color = t.color.as_ref().and_then(|c| srgb(c, &wb.theme));
    if color.is_some() || t.width.is_some() || t.dash.is_some() {
        x.push_str("<c:spPr><a:ln");
        if let Some(w) = t.width.filter(|w| w.is_finite()) {
            // ST_LineWidth: 0..=20116800 EMU.
            let _ = write!(x, " w=\"{}\"", (f64::from(w) * EMU_PER_PT).round().clamp(0.0, 20_116_800.0) as i64);
        }
        x.push_str(" cap=\"rnd\">");
        if let Some(c) = &color {
            let _ = write!(x, "<a:solidFill>{c}</a:solidFill>");
        }
        if let Some(d) = t.dash.and_then(|d| DASHES.iter().find(|(k, _)| *k == d)) {
            let _ = write!(x, "<a:prstDash val=\"{}\"/>", d.1);
        }
        x.push_str("</a:ln></c:spPr>");
    }
    let ty = match t.kind {
        TrendlineKind::Linear => "linear",
        TrendlineKind::Exponential => "exp",
        TrendlineKind::Logarithmic => "log",
        TrendlineKind::Polynomial(_) => "poly",
        TrendlineKind::Power => "power",
        TrendlineKind::MovingAverage(_) => "movingAvg",
    };
    let _ = write!(x, "<c:trendlineType val=\"{ty}\"/>");
    match t.kind {
        TrendlineKind::Polynomial(o) => {
            let _ = write!(x, "<c:order val=\"{}\"/>", o.clamp(2, 6));
        }
        TrendlineKind::MovingAverage(p) => {
            let _ = write!(x, "<c:period val=\"{}\"/>", p.clamp(2, 255));
        }
        _ => {}
    }
    for (tag, v) in [("forward", t.forward), ("backward", t.backward)] {
        if v.is_finite() && v > 0.0 {
            let _ = write!(x, "<c:{tag} val=\"{}\"/>", num(v));
        }
    }
    if let Some(i) = t.intercept.filter(|i| i.is_finite()) {
        let _ = write!(x, "<c:intercept val=\"{}\"/>", num(i));
    }
    let _ = write!(x, "<c:dispRSqr val=\"{}\"/><c:dispEq val=\"{}\"/>", t.display_r_squared as u8, t.display_equation as u8);
    if t.display_r_squared || t.display_equation {
        x.push_str("<c:trendlineLbl><c:numFmt formatCode=\"General\" sourceLinked=\"0\"/></c:trendlineLbl>");
    }
    x.push_str("</c:trendline>");
    x
}

fn dlbls(show: bool) -> String {
    format!(
        "<c:dLbls><c:showLegendKey val=\"0\"/><c:showVal val=\"{}\"/><c:showCatName val=\"0\"/><c:showSerName val=\"0\"/><c:showPercent val=\"0\"/><c:showBubbleSize val=\"0\"/></c:dLbls>",
        show as u8
    )
}

fn group_xml(g: G, series: &[(usize, &Series)], ch: &Chart, axes: (u32, u32), wb: &Workbook, si: usize) -> String {
    let g = if g == (G::Scatter { lines: false })
        && series.iter().any(|(i, _)| ch.format.series.get(i).is_some_and(|s| s.line_width.is_some() && !s.no_line))
    {
        G::Scatter { lines: true }
    } else {
        g
    };
    let sers: String = series.iter().map(|(i, s)| series_xml(s, *i, g, wb, si, ch.format.series.get(i))).collect();
    let dl = dlbls(ch.data_labels);
    let ax = format!("<c:axId val=\"{}\"/><c:axId val=\"{}\"/>", axes.0, axes.1);
    match g {
        G::Bar { bar, grouping } => {
            let overlap = ch
                .format
                .overlap
                .map(|n| format!("<c:overlap val=\"{}\"/>", n.round() as i32))
                .unwrap_or_else(|| if grouping == "clustered" { String::new() } else { "<c:overlap val=\"100\"/>".into() });
            let gap = ch.format.gap_width.unwrap_or(150.0).round() as u32;
            format!(
                "<c:barChart><c:barDir val=\"{}\"/><c:grouping val=\"{grouping}\"/><c:varyColors val=\"0\"/>{sers}{dl}<c:gapWidth val=\"{gap}\"/>{overlap}{ax}</c:barChart>",
                if bar { "bar" } else { "col" }
            )
        }
        G::Line { stacked, .. } => format!(
            "<c:lineChart><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{sers}{dl}<c:marker val=\"1\"/>{ax}</c:lineChart>",
            if stacked { "stacked" } else { "standard" }
        ),
        G::Pie => format!("<c:pieChart><c:varyColors val=\"1\"/>{sers}{dl}<c:firstSliceAng val=\"0\"/></c:pieChart>"),
        G::Doughnut => {
            format!("<c:doughnutChart><c:varyColors val=\"1\"/>{sers}{dl}<c:firstSliceAng val=\"0\"/><c:holeSize val=\"50\"/></c:doughnutChart>")
        }
        G::Area { stacked } => format!(
            "<c:areaChart><c:grouping val=\"{}\"/><c:varyColors val=\"0\"/>{sers}{dl}{ax}</c:areaChart>",
            if stacked { "stacked" } else { "standard" }
        ),
        G::Scatter { .. } => format!("<c:scatterChart><c:scatterStyle val=\"lineMarker\"/><c:varyColors val=\"0\"/>{sers}{dl}{ax}</c:scatterChart>"),
        G::Bubble => {
            format!("<c:bubbleChart><c:varyColors val=\"0\"/>{sers}{dl}<c:bubbleScale val=\"100\"/><c:showNegBubbles val=\"0\"/>{ax}</c:bubbleChart>")
        }
        G::Radar => format!("<c:radarChart><c:radarStyle val=\"marker\"/><c:varyColors val=\"0\"/>{sers}{dl}{ax}</c:radarChart>"),
        G::Stock => format!("<c:stockChart>{sers}{dl}<c:hiLowLines/>{ax}</c:stockChart>"),
    }
}

fn title_xml(t: &str) -> String {
    let mut s = String::from("<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/>");
    for line in t.split('\n') {
        let _ = write!(s, "<a:p><a:pPr><a:defRPr/></a:pPr><a:r><a:t>{}</a:t></a:r></a:p>", esc(line));
    }
    s.push_str("</c:rich></c:tx><c:overlay val=\"0\"/></c:title>");
    s
}

struct Ax<'a> {
    cat: bool,
    id: u32,
    cross: u32,
    pos: &'a str,
    delete: bool,
    grid: bool,
    title: Option<&'a str>,
    crosses_max: bool,
}

fn axis_xml(a: &Ax<'_>, chart: &Chart) -> String {
    let automatic = gridcraft_model::ChartAxis::default();
    let options = if a.id > 2 {
        &automatic
    } else if matches!(a.pos, "b" | "t") {
        &chart.format.x_axis
    } else {
        &chart.format.y_axis
    };
    let mut s = format!("<c:{0}><c:axId val=\"{1}\"/><c:scaling>", if a.cat { "catAx" } else { "valAx" }, a.id,);
    if options.logarithmic && !a.cat {
        s.push_str("<c:logBase val=\"10\"/>");
    }
    let _ = write!(s, "<c:orientation val=\"{}\"/>", if options.reversed { "maxMin" } else { "minMax" });
    if !a.cat {
        if let Some(v) = options.maximum {
            let _ = write!(s, "<c:max val=\"{v}\"/>");
        }
        if let Some(v) = options.minimum {
            let _ = write!(s, "<c:min val=\"{v}\"/>");
        }
    }
    let _ = write!(s, "</c:scaling><c:delete val=\"{}\"/><c:axPos val=\"{}\"/>", a.delete as u8, a.pos);
    let scatter = matches!(chart.kind, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble);
    let category_grid = if scatter { matches!(a.pos, "b" | "t") } else { a.cat };
    if a.grid || (a.id <= 2 && category_grid && chart.format.vertical_gridlines) {
        let style = if category_grid { &chart.format.vertical_gridline } else { &chart.format.horizontal_gridline };
        let sp = surface_xml(&gridcraft_model::ChartSurface { border: style.color, border_width: style.width, ..Default::default() }, "c");
        if sp.is_empty() {
            s.push_str("<c:majorGridlines/>");
        } else {
            let _ = write!(s, "<c:majorGridlines>{sp}</c:majorGridlines>");
        }
    }
    if let Some(t) = a.title {
        s.push_str(&title_xml(t));
    }
    let _ = write!(
        s,
        "<c:numFmt formatCode=\"{}\" sourceLinked=\"{}\"/><c:majorTickMark val=\"{}\"/><c:minorTickMark val=\"{}\"/><c:tickLblPos val=\"nextTo\"/>",
        esc_attr(options.number_format.as_deref().unwrap_or("General")),
        if options.number_format.is_some() { 0 } else { 1 },
        esc_attr(options.major_tick.as_deref().unwrap_or("none")),
        esc_attr(options.minor_tick.as_deref().unwrap_or("none")),
    );
    s.push_str(&surface_xml(
        &gridcraft_model::ChartSurface { border: options.line.color, border_width: options.line.width, ..Default::default() },
        "c",
    ));
    let _ = write!(s, "<c:crossAx val=\"{}\"/><c:crosses val=\"{}\"/>", a.cross, if a.crosses_max { "max" } else { "autoZero" });
    if a.cat {
        s.push_str("<c:auto val=\"1\"/><c:lblAlgn val=\"ctr\"/><c:lblOffset val=\"100\"/><c:noMultiLvlLbl val=\"0\"/></c:catAx>");
    } else {
        s.push_str("<c:crossBetween val=\"between\"/>");
        if let Some(v) = options.major_unit.filter(|_| !options.logarithmic) {
            let _ = write!(s, "<c:majorUnit val=\"{v}\"/>");
        }
        if let Some(v) = options.minor_unit.filter(|_| !options.logarithmic) {
            let _ = write!(s, "<c:minorUnit val=\"{v}\"/>");
        }
        s.push_str("</c:valAx>");
    }
    s
}

/// A PivotChart link without the workbook in brackets: `[Book.xlsx]Sheet2!PT` → `Sheet2!PT`.
fn strip_book(link: &str) -> &str {
    link.strip_prefix('[').and_then(|r| r.split_once(']')).map_or(link, |(_, rest)| rest)
}

/// Chart part XML for a model chart on sheet `si`.
pub fn write_chart(wb: &Workbook, si: usize, ch: &Chart, book_name: &str) -> String {
    let axis_xml = |a: &Ax<'_>| axis_xml(a, ch);
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><c:roundedCorners val=\"0\"/>",
    );
    if (1..=48).contains(&ch.style) {
        let _ = write!(s, "<c:style val=\"{}\"/>", ch.style);
    }
    // A PivotChart names its PivotTable behind the workbook in brackets (`[Book.xlsx]Sheet2!PivotTable1`);
    // without the brackets Excel shows an ordinary chart.
    if let Some(link) = ch.pivot.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
        let link = strip_book(link);
        let _ = write!(s, "<c:pivotSource><c:name>[{}]{}</c:name><c:fmtId val=\"0\"/></c:pivotSource>", esc(book_name), esc(link));
    }
    s.push_str("<c:chart>");
    match &ch.title {
        Some(t) if !t.is_empty() => {
            s.push_str(&title_xml(t));
            s.push_str("<c:autoTitleDeleted val=\"0\"/>");
        }
        _ => s.push_str("<c:autoTitleDeleted val=\"1\"/>"),
    }
    s.push_str("<c:plotArea><c:layout/>");
    let all: Vec<(usize, &Series)> = ch.series.iter().enumerate().collect();
    let primary_g;
    let mut secondary: Option<G> = None;
    if ch.kind == ChartKind::Combo {
        // Series without a kind of their own: the first is a column, the others lines (as drawn).
        let kind = |i: usize, x: &Series| x.kind.filter(|k| *k != ChartKind::Combo).or((i > 0).then_some(ChartKind::Line));
        let is_line = |k: Option<ChartKind>| matches!(k.map(group_for), Some(G::Line { .. } | G::Area { .. } | G::Radar | G::Scatter { .. }));
        let bars: Vec<(usize, &Series)> = all.iter().filter(|(i, x)| !x.secondary && !is_line(kind(*i, x))).copied().collect();
        let lines: Vec<(usize, &Series)> = all.iter().filter(|(i, x)| !x.secondary && is_line(kind(*i, x))).copied().collect();
        let sec: Vec<(usize, &Series)> = all.iter().filter(|(_, x)| x.secondary).copied().collect();
        primary_g = G::Bar { bar: false, grouping: "clustered" };
        if !bars.is_empty() || (lines.is_empty() && sec.is_empty()) {
            s.push_str(&group_xml(primary_g, &bars, ch, (1, 2), wb, si));
        }
        if !lines.is_empty() {
            s.push_str(&group_xml(G::Line { stacked: false, markers: false }, &lines, ch, (1, 2), wb, si));
        }
        if !sec.is_empty() {
            let g = match sec.first().and_then(|(_, x)| x.kind).map(group_for) {
                Some(b @ G::Bar { .. }) => b,
                _ => G::Line { stacked: false, markers: false },
            };
            s.push_str(&group_xml(g, &sec, ch, (3, 4), wb, si));
            secondary = Some(g);
        }
    } else {
        primary_g = match group_for(ch.kind) {
            // A stock chart has 3 or 4 series (high-low-close, open-high-low-close).
            G::Stock if !(3..=4).contains(&ch.series.len()) => G::Line { stacked: false, markers: false },
            g => g,
        };
        s.push_str(&group_xml(primary_g, &all, ch, (1, 2), wb, si));
    }
    match primary_g {
        G::Pie | G::Doughnut => {}
        G::Scatter { .. } | G::Bubble => {
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 1,
                cross: 2,
                pos: "b",
                delete: false,
                grid: false,
                title: ch.x_title.as_deref(),
                crosses_max: false,
            }));
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 2,
                cross: 1,
                pos: "l",
                delete: false,
                grid: ch.gridlines,
                title: ch.y_title.as_deref(),
                crosses_max: false,
            }));
        }
        G::Bar { bar: true, .. } => {
            s.push_str(&axis_xml(&Ax {
                cat: true,
                id: 1,
                cross: 2,
                pos: "l",
                delete: false,
                grid: false,
                title: ch.y_title.as_deref(),
                crosses_max: false,
            }));
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 2,
                cross: 1,
                pos: "b",
                delete: false,
                grid: ch.gridlines,
                title: ch.x_title.as_deref(),
                crosses_max: false,
            }));
        }
        _ => {
            s.push_str(&axis_xml(&Ax {
                cat: true,
                id: 1,
                cross: 2,
                pos: "b",
                delete: false,
                grid: false,
                title: ch.x_title.as_deref(),
                crosses_max: false,
            }));
            s.push_str(&axis_xml(&Ax {
                cat: false,
                id: 2,
                cross: 1,
                pos: "l",
                delete: false,
                grid: ch.gridlines,
                title: ch.y_title.as_deref(),
                crosses_max: false,
            }));
        }
    }
    if secondary.is_some() {
        s.push_str(&axis_xml(&Ax { cat: true, id: 3, cross: 4, pos: "b", delete: true, grid: false, title: None, crosses_max: false }));
        s.push_str(&axis_xml(&Ax { cat: false, id: 4, cross: 3, pos: "r", delete: false, grid: false, title: None, crosses_max: true }));
    }
    s.push_str(&surface_xml(&ch.format.plot_area, "c"));
    s.push_str("</c:plotArea>");
    let pos = match ch.legend {
        LegendPos::None => None,
        LegendPos::Bottom => Some("b"),
        LegendPos::Top => Some("t"),
        LegendPos::Left => Some("l"),
        LegendPos::Right => Some("r"),
    };
    if let Some(p) = pos {
        let _ = write!(s, "<c:legend><c:legendPos val=\"{p}\"/><c:overlay val=\"{}\"/></c:legend>", u8::from(ch.format.legend_overlay));
    }
    s.push_str("<c:plotVisOnly val=\"1\"/><c:dispBlanksAs val=\"gap\"/></c:chart>");
    let mut chart_area = ch.format.chart_area.clone();
    chart_area.border_width = chart_area.border_width.or(ch.format.border_width);
    s.push_str(&surface_xml(&chart_area, "c"));
    s.push_str(&format_extension(&ch.format, "c"));
    s.push_str("</c:chartSpace>");
    s
}

#[cfg(test)]
mod marker_safety_tests {
    use super::*;

    #[test]
    fn custom_extension_marker_is_restricted_to_supported_xml_tokens() {
        let series = Series { values: "A1:A2".into(), ..Default::default() };
        let format = gridcraft_model::ChartSeriesFormat { marker: Some("circle\"/><c:evil/>".into()), ..Default::default() };
        let xml = series_xml(&series, 0, G::Line { stacked: false, markers: true }, &Workbook::default(), 0, Some(&format));
        assert!(xml.contains("<c:symbol val=\"circle\"/>"));
        assert!(!xml.contains("<c:evil/>"));
    }

    #[test]
    fn native_xlsx_format_overrides_extension_but_extension_keeps_gridcraft_only_fields() {
        let mut custom = gridcraft_model::ChartFormat::default();
        custom.x_axis.minimum = Some(5.0);
        custom.x_axis.maximum = Some(50.0);
        custom.legend_overlay = true;
        custom.text.insert("title".into(), gridcraft_model::ChartTextFormat { size: Some(22.0), ..Default::default() });
        custom.series.insert(0, gridcraft_model::ChartSeriesFormat { marker: Some("circle".into()), smooth: Some(false), ..Default::default() });
        let ext = format_extension(&custom, "c");
        let xml = format!(
            r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><c:chart><c:title><c:tx><c:rich><a:p><a:r><a:t>Chart</a:t></a:r></a:p></c:rich></c:tx></c:title><c:legend><c:legendPos val="b"/><c:overlay val="0"/></c:legend><c:plotArea><c:lineChart><c:ser><c:idx val="0"/><c:order val="0"/><c:spPr><a:ln w="25400"><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></a:ln></c:spPr><c:marker><c:symbol val="none"/></c:marker><c:smooth val="1"/></c:ser><c:axId val="1"/><c:axId val="2"/></c:lineChart><c:catAx><c:axPos val="b"/><c:scaling><c:min val="0"/><c:max val="75"/></c:scaling></c:catAx><c:valAx><c:axPos val="l"/><c:scaling><c:min val="0"/><c:max val="100"/></c:scaling></c:valAx></c:plotArea></c:chart>{ext}</c:chartSpace>"#
        );
        let root = crate::xml::parse(xml.as_bytes()).unwrap();
        let chart = read_chart(&root).unwrap();
        assert_eq!(chart.format.x_axis.minimum, Some(0.0));
        assert_eq!(chart.format.x_axis.maximum, Some(75.0));
        assert_eq!(chart.format.y_axis.maximum, Some(100.0));
        assert!(!chart.format.legend_overlay);
        assert_eq!(chart.format.text["title"].size, Some(22.0));
        assert_eq!(chart.format.series[&0].marker, None);
        assert_eq!(chart.format.series[&0].smooth, Some(true));
        assert_eq!(chart.format.series[&0].line_width, Some(2.0));
    }
}
