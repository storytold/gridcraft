//! Office 2016 chart types (histogram, box & whisker, waterfall, funnel, treemap, sunburst),
//! which files keep as `cx:chartSpace` "chartex" parts (`xl/charts/chartExN.xml`) rather than
//! DrawingML chart parts.

use std::fmt::Write as _;

use gridcraft_core::RangeRef;
use gridcraft_formula::{Expr, SheetSel};
use gridcraft_model::{Chart, ChartKind, LegendPos, Series, Workbook};

use crate::drawing::{dml_color, srgb};
use crate::xml::{El, esc, esc_attr};

pub const CT_CHARTEX: &str = "application/vnd.ms-office.chartex+xml";
pub const REL_CHARTEX: &str = "http://schemas.microsoft.com/office/2014/relationships/chartEx";
pub const CT_STYLE: &str = "application/vnd.ms-office.chartstyle+xml";
pub const CT_COLORS: &str = "application/vnd.ms-office.chartcolorstyle+xml";
pub const REL_STYLE: &str = "http://schemas.microsoft.com/office/2011/relationships/chartStyle";
pub const REL_COLORS: &str = "http://schemas.microsoft.com/office/2011/relationships/chartColorStyle";

/// Kinds written as chartex parts.
pub fn is_chartex(k: ChartKind) -> bool {
    matches!(k, ChartKind::Histogram | ChartKind::BoxWhisker | ChartKind::Waterfall | ChartKind::Funnel | ChartKind::Treemap | ChartKind::Sunburst)
}

/// The `mc:Choice` requirement for a kind's drawing frame: (prefix, namespace). Funnel charts
/// came in a later revision of the format.
pub fn requires(k: ChartKind) -> (&'static str, &'static str) {
    match k {
        ChartKind::Funnel => ("cx2", "http://schemas.microsoft.com/office/drawing/2015/10/21/chartex"),
        _ => ("cx1", "http://schemas.microsoft.com/office/drawing/2015/9/8/chartex"),
    }
}

// ---------------------------------------------------------------- reading

fn kind_of(layout: &str) -> Option<ChartKind> {
    Some(match layout {
        "clusteredColumn" | "paretoLine" => ChartKind::Histogram,
        "boxWhisker" => ChartKind::BoxWhisker,
        "waterfall" => ChartKind::Waterfall,
        "funnel" => ChartKind::Funnel,
        "treemap" => ChartKind::Treemap,
        "sunburst" => ChartKind::Sunburst,
        _ => return None,
    })
}

fn formula(dim: &El) -> Option<String> {
    let f = dim.child("f")?;
    let t = f.text.trim();
    Some(crate::fmla::from_file(t.strip_prefix('=').unwrap_or(t)))
}

fn text(tx: &El) -> Option<String> {
    if let Some(d) = tx.child("txData") {
        if let Some(f) = d.child("f") {
            let t = f.text.trim();
            return Some(t.strip_prefix('=').unwrap_or(t).to_string());
        }
        return d.child("v").map(|v| v.text.clone());
    }
    let rich = tx.child("rich")?;
    let paras: Vec<String> = rich.kids("p").map(|p| p.kids("r").filter_map(|r| r.child("t")).map(|t| t.text.as_str()).collect()).collect();
    let s = paras.join("\n");
    if s.is_empty() { None } else { Some(s) }
}

pub fn read_chartex(root: &El) -> Option<Chart> {
    let chart = root.child("chart")?;
    let plot = chart.child("plotArea")?;
    let region = plot.child("plotAreaRegion")?;
    // Data by id: (categories, values).
    let data: Vec<(String, Option<String>, Option<String>)> = root
        .child("chartData")
        .map(|cd| {
            cd.kids("data")
                .map(|d| {
                    let id = d.attr("id").unwrap_or("").to_string();
                    let cat = d.kids("strDim").chain(d.kids("numDim")).find(|x| matches!(x.attr("type"), Some("cat" | "x"))).and_then(formula);
                    let val = d.kids("numDim").find(|x| matches!(x.attr("type"), Some("val" | "size" | "y"))).and_then(formula);
                    (id, cat, val)
                })
                .collect()
        })
        .unwrap_or_default();
    let mut kind = None;
    let mut series = vec![];
    let mut data_labels = false;
    for s in region.kids("series") {
        let layout = s.attr("layoutId").unwrap_or("");
        // A Pareto line belongs to its histogram.
        if layout == "paretoLine" {
            continue;
        }
        let k = kind_of(layout)?;
        kind.get_or_insert(k);
        let id = s.child_val("dataId").unwrap_or("");
        let (cat, val) = data.iter().find(|(i, _, _)| i == id).map(|(_, c, v)| (c.clone(), v.clone())).unwrap_or_default();
        data_labels |= s.path(&["dataLabels", "visibility"]).is_some_and(|v| v.flag("value", false));
        series.push(Series {
            name: s.child("tx").and_then(text),
            categories: cat,
            values: val.unwrap_or_default(),
            // The default colour (the series' theme accent, which the writer spells out) isn't kept.
            color: s
                .path(&["spPr", "solidFill"])
                .and_then(dml_color)
                .filter(|c| *c != gridcraft_model::Color::Theme(4 + (series.len() % 6) as u8, 0)),
            ..Default::default()
        });
    }
    let legend = match chart.child("legend") {
        None => LegendPos::None,
        Some(l) => match l.attr("pos").unwrap_or("r") {
            "b" => LegendPos::Bottom,
            "t" => LegendPos::Top,
            "l" => LegendPos::Left,
            _ => LegendPos::Right,
        },
    };
    let axis_title = |id: &str| plot.kids("axis").find(|a| a.attr("id") == Some(id)).and_then(|a| a.path(&["title", "tx"])).and_then(text);
    Some(Chart {
        id: 0,
        kind: kind?,
        anchor: Default::default(),
        title: chart.path(&["title", "tx"]).and_then(text),
        series,
        legend,
        data_labels,
        gridlines: plot.kids("axis").any(|a| a.child("majorGridlines").is_some()),
        style: 2,
        x_title: axis_title("0"),
        y_title: axis_title("1"),
        source: None,
        by_rows: false,
    })
}

// ---------------------------------------------------------------- writing

/// Prefix of the hidden defined names chartex formulas go through: a chartex part refers to
/// its data by names (`_xlchart.v1.0`), which the workbook defines, not by references.
pub const NAME_PREFIX: &str = "_xlchart.v1.";

/// `<cx:f>` for a reference: a new hidden workbook name for it (collected in `names`, written
/// with the workbook's defined names). A reference along a row says so; otherwise each column
/// is one level.
fn f_xml(wb: &Workbook, si: usize, f: &str, names: &mut Vec<String>) -> String {
    let row = ref_range(wb, si, f).is_some_and(|(_, r)| r.height() == 1 && r.width() > 1);
    names.push(f.to_string());
    format!("<cx:f{}>{NAME_PREFIX}{}</cx:f>", if row { " dir=\"row\"" } else { "" }, names.len() - 1)
}

/// A reference's sheet and range (for the row/column direction).
fn ref_range(wb: &Workbook, si: usize, f: &str) -> Option<(usize, RangeRef)> {
    let Ok(Expr::Ref(r)) = gridcraft_formula::parse(f) else { return None };
    let sheet = match &r.sheet {
        SheetSel::Current => si,
        SheetSel::Named(n) => wb.sheet_index(n)?,
        SheetSel::Span(..) => return None,
    };
    Some((sheet, r.range()))
}

fn tx_xml(t: &str, wb: &Workbook, si: usize, names: &mut Vec<String>) -> String {
    if ref_range(wb, si, t).is_some() {
        let v = crate::chart::ref_values(wb, si, t).and_then(|v| v.into_iter().next()).map(|v| v.display()).unwrap_or_default();
        format!("<cx:tx><cx:txData>{}<cx:v>{}</cx:v></cx:txData></cx:tx>", f_xml(wb, si, t, names), esc(&v))
    } else {
        let lit = t.trim().strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(t);
        format!("<cx:tx><cx:txData><cx:v>{}</cx:v></cx:txData></cx:tx>", esc(lit))
    }
}

/// The chart style part a chartex part comes with: every element at its plain default.
pub fn style_xml() -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cs:chartStyle xmlns:cs=\"http://schemas.microsoft.com/office/drawing/2012/chartStyle\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" id=\"201\">",
    );
    for e in [
        "axisTitle",
        "categoryAxis",
        "chartArea",
        "dataLabel",
        "dataLabelCallout",
        "dataPoint",
        "dataPoint3D",
        "dataPointLine",
        "dataPointMarker",
        "dataPointMarkerLayout",
        "dataPointWireframe",
        "dataTable",
        "downBar",
        "dropLine",
        "errorBar",
        "floor",
        "gridlineMajor",
        "gridlineMinor",
        "hiLoLine",
        "leaderLine",
        "legend",
        "plotArea",
        "plotArea3D",
        "seriesAxis",
        "seriesLine",
        "title",
        "trendline",
        "trendlineLabel",
        "upBar",
        "valueAxis",
        "wall",
    ] {
        if e == "dataPointMarkerLayout" {
            s.push_str("<cs:dataPointMarkerLayout symbol=\"circle\" size=\"5\"/>");
        } else {
            let _ = write!(
                s,
                "<cs:{e}><cs:lnRef idx=\"0\"/><cs:fillRef idx=\"0\"/><cs:effectRef idx=\"0\"/><cs:fontRef idx=\"minor\"><a:schemeClr val=\"tx1\"/></cs:fontRef></cs:{e}>"
            );
        }
    }
    s.push_str("</cs:chartStyle>");
    s
}

/// The colour style part a chartex part comes with: the theme accents in turn.
pub fn colors_xml() -> String {
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cs:colorStyle xmlns:cs=\"http://schemas.microsoft.com/office/drawing/2012/chartStyle\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" meth=\"cycle\" id=\"10\">",
    );
    for i in 1..=6 {
        let _ = write!(s, "<a:schemeClr val=\"accent{i}\"/>");
    }
    s.push_str("<cs:variation/></cs:colorStyle>");
    s
}

/// A solid fill (and outline, for line-drawn parts like whiskers) as `cx:spPr`.
fn fill_xml(color: &str, line: bool) -> String {
    let ln = if line { format!("<a:ln w=\"9525\"><a:solidFill>{color}</a:solidFill></a:ln>") } else { String::new() };
    format!("<cx:spPr><a:solidFill>{color}</a:solidFill>{ln}</cx:spPr>")
}

/// Light grey for gridlines and the chart border: the text colour at a quarter strength.
const GREY: &str = "<a:schemeClr val=\"tx1\"><a:lumMod val=\"25000\"/><a:lumOff val=\"75000\"/></a:schemeClr>";

fn accent(i: usize) -> String {
    format!("<a:schemeClr val=\"accent{}\"/>", i % 6 + 1)
}

/// Chartex part XML for a model chart on sheet `si`. The references it uses are appended to
/// `names` (see [`NAME_PREFIX`]).
pub fn write_chartex(wb: &Workbook, si: usize, ch: &Chart, names: &mut Vec<String>) -> String {
    let hierarchy = matches!(ch.kind, ChartKind::Treemap | ChartKind::Sunburst);
    let mut s = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cx:chartSpace xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:cx=\"http://schemas.microsoft.com/office/drawing/2014/chartex\"><cx:chartData>",
    );
    for (i, ser) in ch.series.iter().enumerate() {
        let _ = write!(s, "<cx:data id=\"{i}\">");
        if let Some(c) = ser.categories.as_deref().filter(|_| ch.kind != ChartKind::Histogram) {
            let _ = write!(s, "<cx:strDim type=\"cat\">{}</cx:strDim>", f_xml(wb, si, c, names));
        }
        let _ = write!(s, "<cx:numDim type=\"{}\">{}</cx:numDim>", if hierarchy { "size" } else { "val" }, f_xml(wb, si, &ser.values, names));
        s.push_str("</cx:data>");
    }
    s.push_str("</cx:chartData><cx:chart>");
    if let Some(t) = ch.title.as_deref().filter(|t| !t.is_empty()) {
        let _ = write!(s, "<cx:title pos=\"t\" align=\"ctr\" overlay=\"0\"><cx:tx><cx:txData><cx:v>{}</cx:v></cx:txData></cx:tx></cx:title>", esc(t));
    }
    s.push_str("<cx:plotArea><cx:plotAreaRegion>");
    let layout = match ch.kind {
        ChartKind::BoxWhisker => "boxWhisker",
        ChartKind::Waterfall => "waterfall",
        ChartKind::Funnel => "funnel",
        ChartKind::Treemap => "treemap",
        ChartKind::Sunburst => "sunburst",
        _ => "clusteredColumn",
    };
    for (i, ser) in ch.series.iter().enumerate() {
        let _ = write!(s, "<cx:series layoutId=\"{layout}\" uniqueId=\"{{00000000-0000-0000-0000-{:08X}{i:04X}}}\">", ch.id);
        if let Some(n) = &ser.name {
            s.push_str(&tx_xml(n, wb, si, names));
        }
        // Fills are written out: they don't come from the style part. Waterfall decreases and
        // treemap/sunburst boxes get their own colours.
        let color = ser.color.as_ref().and_then(|c| srgb(c, &wb.theme)).unwrap_or_else(|| accent(i));
        s.push_str(&fill_xml(&color, ch.kind == ChartKind::BoxWhisker));
        let points = || crate::chart::ref_values(wb, si, &ser.values).unwrap_or_default().into_iter().take(10_000).enumerate();
        match ch.kind {
            ChartKind::Waterfall => {
                for (j, v) in points() {
                    if v.as_f64().is_some_and(|n| n < 0.0) {
                        let _ = write!(s, "<cx:dataPt idx=\"{j}\">{}</cx:dataPt>", fill_xml(&accent(i + 1), false));
                    }
                }
            }
            ChartKind::Treemap | ChartKind::Sunburst if ser.color.is_none() => {
                for (j, _) in points() {
                    let _ = write!(s, "<cx:dataPt idx=\"{j}\">{}</cx:dataPt>", fill_xml(&accent(j), false));
                }
            }
            _ => {}
        }
        if ch.data_labels {
            s.push_str("<cx:dataLabels><cx:visibility seriesName=\"0\" categoryName=\"0\" value=\"1\"/></cx:dataLabels>");
        }
        let _ = write!(s, "<cx:dataId val=\"{i}\"/>");
        match ch.kind {
            ChartKind::Histogram => s.push_str("<cx:layoutPr><cx:binning intervalClosed=\"r\"/></cx:layoutPr>"),
            ChartKind::BoxWhisker => s.push_str(
                "<cx:layoutPr><cx:visibility meanLine=\"0\" meanMarker=\"1\" nonoutliers=\"0\" outliers=\"1\"/><cx:statistics quartileMethod=\"exclusive\"/></cx:layoutPr>",
            ),
            ChartKind::Treemap => s.push_str("<cx:layoutPr><cx:parentLabelLayout val=\"overlapping\"/></cx:layoutPr>"),
            _ => {}
        }
        s.push_str("</cx:series>");
    }
    s.push_str("</cx:plotAreaRegion>");
    let gap = match ch.kind {
        ChartKind::Histogram => Some("0"),
        ChartKind::BoxWhisker => Some("1"),
        ChartKind::Waterfall => Some("0.5"),
        ChartKind::Funnel => Some("0.06"),
        _ => None,
    };
    let title = |t: Option<&str>| {
        t.filter(|t| !t.is_empty())
            .map(|t| format!("<cx:title><cx:tx><cx:txData><cx:v>{}</cx:v></cx:txData></cx:tx></cx:title>", esc(t)))
            .unwrap_or_default()
    };
    if let Some(gap) = gap {
        let _ =
            write!(s, "<cx:axis id=\"0\"><cx:catScaling gapWidth=\"{}\"/>{}<cx:tickLabels/></cx:axis>", esc_attr(gap), title(ch.x_title.as_deref()));
        if ch.kind != ChartKind::Funnel {
            let _ = write!(
                s,
                "<cx:axis id=\"1\"><cx:valScaling/>{}{}<cx:tickLabels/></cx:axis>",
                title(ch.y_title.as_deref()),
                if ch.gridlines {
                    format!("<cx:majorGridlines><cx:spPr><a:ln w=\"9525\"><a:solidFill>{GREY}</a:solidFill></a:ln></cx:spPr></cx:majorGridlines>")
                } else {
                    String::new()
                }
            );
        }
    }
    s.push_str("</cx:plotArea>");
    let pos = match ch.legend {
        LegendPos::None => None,
        LegendPos::Bottom => Some("b"),
        LegendPos::Top => Some("t"),
        LegendPos::Left => Some("l"),
        LegendPos::Right => Some("r"),
    };
    if let Some(p) = pos {
        let _ = write!(s, "<cx:legend pos=\"{p}\" align=\"ctr\" overlay=\"0\"/>");
    }
    // A white chart area with a light border, like other charts.
    let _ = write!(
        s,
        "</cx:chart><cx:spPr><a:solidFill><a:schemeClr val=\"bg1\"/></a:solidFill><a:ln w=\"9525\"><a:solidFill>{GREY}</a:solidFill></a:ln></cx:spPr></cx:chartSpace>"
    );
    s
}
