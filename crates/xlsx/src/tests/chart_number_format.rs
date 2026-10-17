//! Number formats typed on chart axes and data labels (`c:numFmt sourceLinked="0"`): read from
//! Excel's layout, and written back so a round trip keeps them.

use std::sync::Arc;

use gridcraft_model::{Anchor, AxisNumberFormats, Chart, ChartKind, LegendPos, Series, Sheet, Workbook};

use crate::chart::read_chart;
use crate::xml::parse;
use crate::{read_xlsx, write_xlsx};

const NS: &str = "xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\"";

/// A column + line combo as Excel saves it, with the secondary value axis listed first.
fn excel_combo() -> String {
    format!(
        "<c:chartSpace {NS}><c:chart><c:plotArea>\
         <c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>\
         <c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
         <c:dLbls><c:numFmt formatCode=\"0;;;\" sourceLinked=\"0\"/><c:showVal val=\"1\"/></c:dLbls>\
         <c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val></c:ser>\
         <c:ser><c:idx val=\"1\"/><c:order val=\"1\"/>\
         <c:dLbls><c:numFmt formatCode=\"0.00\" sourceLinked=\"1\"/><c:showVal val=\"1\"/></c:dLbls>\
         <c:val><c:numRef><c:f>Data!$C$2:$C$5</c:f></c:numRef></c:val></c:ser>\
         <c:axId val=\"10\"/><c:axId val=\"20\"/></c:barChart>\
         <c:lineChart><c:grouping val=\"standard\"/>\
         <c:ser><c:idx val=\"2\"/><c:order val=\"2\"/>\
         <c:val><c:numRef><c:f>Data!$D$2:$D$5</c:f></c:numRef></c:val></c:ser>\
         <c:dLbls><c:numFmt formatCode=\"0.0%\" sourceLinked=\"0\"/><c:showVal val=\"1\"/></c:dLbls>\
         <c:axId val=\"30\"/><c:axId val=\"40\"/></c:lineChart>\
         <c:catAx><c:axId val=\"10\"/><c:numFmt formatCode=\"General\" sourceLinked=\"1\"/><c:crossAx val=\"20\"/></c:catAx>\
         <c:valAx><c:axId val=\"40\"/><c:numFmt formatCode=\"0%\" sourceLinked=\"0\"/><c:crossAx val=\"30\"/></c:valAx>\
         <c:valAx><c:axId val=\"20\"/><c:numFmt formatCode=\"[h]:mm\" sourceLinked=\"0\"/><c:crossAx val=\"10\"/></c:valAx>\
         <c:catAx><c:axId val=\"30\"/><c:delete val=\"1\"/><c:crossAx val=\"40\"/></c:catAx>\
         </c:plotArea></c:chart></c:chartSpace>"
    )
}

#[test]
fn typed_formats_are_read_and_linked_ones_are_not() {
    let ch = read_chart(&parse(excel_combo().as_bytes()).unwrap()).unwrap();
    assert_eq!(
        ch.axis_number_formats,
        AxisNumberFormats { x: None, y: Some("[h]:mm".into()), y2: Some("0%".into()) },
        "each value axis by the group that uses it, not by file order"
    );
    let labels: Vec<Option<&str>> = ch.series.iter().map(|s| s.label_number_format.as_deref()).collect();
    // Series 1 is linked to source; series 2 takes its group's labels.
    assert_eq!(labels, [Some("0;;;"), None, Some("0.0%")]);
}

fn read(plot: &str) -> Chart {
    read_chart(&parse(format!("<c:chartSpace {NS}><c:chart><c:plotArea>{plot}</c:plotArea></c:chart></c:chartSpace>").as_bytes()).unwrap()).unwrap()
}

#[test]
fn a_percentage_label_format_is_not_applied_to_values() {
    // Driver-Safety-Scorecard's doughnut: labels show percentages only, in `0.0%`. On the values
    // that format would read 3 as "300.0%".
    let ch = read(
        "<c:doughnutChart><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
         <c:dLbls><c:numFmt formatCode=\"0.0%\" sourceLinked=\"0\"/><c:showVal val=\"0\"/><c:showPercent val=\"1\"/></c:dLbls>\
         <c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val></c:ser>\
         <c:dLbls><c:numFmt formatCode=\"0.00%\" sourceLinked=\"0\"/><c:showVal val=\"1\"/></c:dLbls></c:doughnutChart>",
    );
    // The series' own labels win over the group's, so the group format doesn't step in either.
    assert_eq!(ch.series[0].label_number_format, None);
}

#[test]
fn a_format_typed_for_sizes_or_x_values_is_not_applied_to_values() {
    let labels = |group: &str, shows: &str| {
        let ch = read(&format!(
            "<c:{group}><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
             <c:dLbls><c:numFmt formatCode=\"0&quot; kg&quot;\" sourceLinked=\"0\"/>{shows}</c:dLbls>\
             <c:yVal><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:yVal></c:ser></c:{group}>"
        ));
        ch.series[0].label_number_format.clone()
    };
    let kg = Some("0\" kg\"".to_string());
    let off = "<c:showVal val=\"0\"/>";
    assert_eq!(labels("bubbleChart", &format!("{off}<c:showBubbleSize val=\"1\"/>")), None);
    assert_eq!(labels("scatterChart", &format!("{off}<c:showCatName val=\"1\"/>")), None);
    // Values shown next to the sizes take the format; category names on a column chart are text.
    assert_eq!(labels("bubbleChart", "<c:showVal val=\"1\"/><c:showBubbleSize val=\"1\"/>"), kg);
    assert_eq!(labels("barChart", &format!("{off}<c:showCatName val=\"1\"/>")), kg);
    // Labels showing nothing yet keep it for when values are switched on.
    assert_eq!(labels("bubbleChart", off), kg);
}

#[test]
fn a_hidden_value_axis_keeps_no_typed_format() {
    // ELD-Unverified-Logs: a 100% stacked chart whose deleted value axis says `#,##0`. Drawn, it
    // would label 0% to 100% as 0, 0, 0, 1, 1, 1.
    let ch = read(
        "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"percentStacked\"/>\
         <c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val></c:ser>\
         <c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
         <c:catAx><c:axId val=\"1\"/><c:crossAx val=\"2\"/></c:catAx>\
         <c:valAx><c:axId val=\"2\"/><c:delete val=\"1\"/><c:numFmt formatCode=\"#,##0\" sourceLinked=\"0\"/><c:crossAx val=\"1\"/></c:valAx>",
    );
    assert_eq!(ch.axis_number_formats, AxisNumberFormats::default());
}

#[test]
fn scatter_axes_are_the_groups_own_and_x_comes_first() {
    // openpyxl writes axPos "l" on both scatter axes; an unrelated value axis is listed first.
    let ch = read(
        "<c:scatterChart><c:ser><c:idx val=\"0\"/><c:order val=\"0\"/>\
         <c:xVal><c:numRef><c:f>Data!$A$2:$A$5</c:f></c:numRef></c:xVal><c:yVal><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:yVal></c:ser>\
         <c:axId val=\"10\"/><c:axId val=\"20\"/></c:scatterChart>\
         <c:valAx><c:axId val=\"99\"/><c:axPos val=\"b\"/><c:numFmt formatCode=\"@\" sourceLinked=\"0\"/><c:crossAx val=\"98\"/></c:valAx>\
         <c:valAx><c:axId val=\"10\"/><c:axPos val=\"l\"/><c:numFmt formatCode=\"0.0\" sourceLinked=\"0\"/><c:crossAx val=\"20\"/></c:valAx>\
         <c:valAx><c:axId val=\"20\"/><c:axPos val=\"l\"/><c:numFmt formatCode=\"0%\" sourceLinked=\"0\"/><c:crossAx val=\"10\"/></c:valAx>",
    );
    assert_eq!(ch.axis_number_formats, AxisNumberFormats { x: Some("0.0".into()), y: Some("0%".into()), y2: None });
}

#[test]
fn a_100_percent_line_or_area_axis_keeps_no_typed_format() {
    // The model reads 100% stacked line and area as plain stacked and draws absolute values, so
    // a `0.0%` typed for the 0%–100% axis would label them "2000000.0%". A column group on its
    // own axes keeps its format.
    for group in ["lineChart", "areaChart"] {
        let ch = read(&format!(
            "<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>\
             <c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val></c:ser>\
             <c:axId val=\"1\"/><c:axId val=\"2\"/></c:barChart>\
             <c:{group}><c:grouping val=\"percentStacked\"/>\
             <c:ser><c:idx val=\"1\"/><c:order val=\"1\"/><c:val><c:numRef><c:f>Data!$C$2:$C$5</c:f></c:numRef></c:val></c:ser>\
             <c:axId val=\"3\"/><c:axId val=\"4\"/></c:{group}>\
             <c:catAx><c:axId val=\"1\"/><c:crossAx val=\"2\"/></c:catAx>\
             <c:valAx><c:axId val=\"2\"/><c:numFmt formatCode=\"#,##0\" sourceLinked=\"0\"/><c:crossAx val=\"1\"/></c:valAx>\
             <c:catAx><c:axId val=\"3\"/><c:delete val=\"1\"/><c:crossAx val=\"4\"/></c:catAx>\
             <c:valAx><c:axId val=\"4\"/><c:numFmt formatCode=\"0.0%\" sourceLinked=\"0\"/><c:crossAx val=\"3\"/></c:valAx>"
        ));
        assert_eq!(ch.axis_number_formats, AxisNumberFormats { x: None, y: Some("#,##0".into()), y2: None }, "{group} on the secondary axis");
        let ch = read(&format!(
            "<c:{group}><c:grouping val=\"percentStacked\"/>\
             <c:ser><c:idx val=\"0\"/><c:order val=\"0\"/><c:val><c:numRef><c:f>Data!$B$2:$B$5</c:f></c:numRef></c:val></c:ser>\
             <c:axId val=\"1\"/><c:axId val=\"2\"/></c:{group}>\
             <c:catAx><c:axId val=\"1\"/><c:crossAx val=\"2\"/></c:catAx>\
             <c:valAx><c:axId val=\"2\"/><c:numFmt formatCode=\"0.0%\" sourceLinked=\"0\"/><c:crossAx val=\"1\"/></c:valAx>"
        ));
        assert_eq!(ch.axis_number_formats, AxisNumberFormats::default(), "{group} alone");
    }
}

fn series(values: &str, label: Option<&str>) -> Series {
    Series { values: values.into(), label_number_format: label.map(str::to_string), ..Series::default() }
}

fn chart(id: u32, kind: ChartKind, series: Vec<Series>, axes: AxisNumberFormats) -> Chart {
    Chart {
        id,
        kind,
        anchor: Anchor { width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series,
        legend: LegendPos::Bottom,
        data_labels: true,
        gridlines: true,
        style: 2,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
        axis_number_formats: axes,
        pivot_label_formats: Vec::new(),
    }
}

#[test]
fn typed_formats_round_trip() {
    let mut s = Sheet::new("Data");
    let mut secondary = series("Data!$C$2:$C$5", Some("0.0\" h\""));
    secondary.kind = Some(ChartKind::Line);
    secondary.secondary = true;
    let combo = chart(
        1,
        ChartKind::Combo,
        vec![series("Data!$B$2:$B$5", Some("0;;;")), secondary],
        AxisNumberFormats { x: None, y: Some("#,##0".into()), y2: Some("0%".into()) },
    );
    let scatter = chart(
        2,
        ChartKind::Scatter,
        vec![Series { categories: Some("Data!$A$2:$A$5".into()), ..series("Data!$B$2:$B$5", None) }],
        AxisNumberFormats { x: Some("[h]:mm".into()), y: Some("0.00E+00".into()), y2: None },
    );
    let plain = chart(3, ChartKind::Pie, vec![series("Data!$B$2:$B$5", Some("0.0%"))], AxisNumberFormats::default());
    s.charts = vec![combo, scatter, plain];
    let wb = Workbook { sheets: vec![Arc::new(s)], ..Workbook::default() };
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let before = &wb.sheets[0].charts;
    let after = &back.sheets[0].charts;
    assert_eq!(after.len(), 3);
    for (b, a) in before.iter().zip(after) {
        assert_eq!(a.axis_number_formats, b.axis_number_formats, "chart {}", b.id);
        let fmts = |c: &Chart| c.series.iter().map(|s| s.label_number_format.clone()).collect::<Vec<_>>();
        assert_eq!(fmts(a), fmts(b), "chart {}", b.id);
    }
}
