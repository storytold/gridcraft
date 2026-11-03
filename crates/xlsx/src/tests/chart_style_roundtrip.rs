//! Explicit chart styles survive complete XLSX serialization.

use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::{Anchor, Chart, ChartKind, LegendPos, Series, Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

#[test]
fn explicit_styles_survive_xlsx_save_and_reload() {
    // Start from the XLSX reader so this fixture follows new optional Chart fields.
    let template: Chart = crate::chart::read_chart(
        &crate::xml::parse(
            concat!(
                r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart">"#,
                "<c:chart><c:plotArea><c:barChart>",
                r#"<c:barDir val="col"/><c:grouping val="clustered"/>"#,
                r#"<c:ser><c:idx val="0"/><c:order val="0"/></c:ser>"#,
                "</c:barChart></c:plotArea></c:chart></c:chartSpace>",
            )
            .as_bytes(),
        )
        .unwrap(),
    )
    .unwrap();

    for kind in [ChartKind::ColumnClustered, ChartKind::BarClustered, ChartKind::Line] {
        for style in [1, 9, 48] {
            let mut wb = Workbook::default();
            let mut sheet = Sheet::new("Data");

            let mut chart = template.clone();
            chart.id = 1;
            chart.kind = kind;
            chart.anchor = Anchor { cell: CellRef::new(0, 3), width: 480.0, height: 300.0, ..Anchor::default() };
            chart.title = Some("Style roundtrip".into());
            chart.series = vec![Series {
                name: Some("Series 1".into()),
                categories: Some(r#"{"A","B","C"}"#.into()),
                values: "{10,20,30}".into(),
                bubble_sizes: None,
                color: None,
                secondary: false,
                kind: None,
                quartiles: Default::default(),
                smooth: false,
                trendlines: Vec::new(),
            }];
            chart.legend = LegendPos::Bottom;
            chart.data_labels = false;
            chart.gridlines = true;
            chart.style = style;
            chart.x_title = None;
            chart.y_title = None;
            chart.source = None;
            chart.by_rows = false;
            chart.pivot = None;
            sheet.charts.push(chart);

            wb.sheets = vec![Arc::new(sheet)];

            let bytes = write_xlsx(&wb).unwrap();
            let (reloaded, _) = read_xlsx(&bytes).unwrap();

            assert_eq!(reloaded.sheets[0].charts.len(), 1);
            assert_eq!(reloaded.sheets[0].charts[0].kind, kind);
            assert_eq!(reloaded.sheets[0].charts[0].style, style);

            let bytes2 = write_xlsx(&reloaded).unwrap();
            let (reloaded2, _) = read_xlsx(&bytes2).unwrap();

            assert_eq!(reloaded2.sheets[0].charts.len(), 1);
            assert_eq!(reloaded2.sheets[0].charts[0].kind, kind);
            assert_eq!(reloaded2.sheets[0].charts[0].style, style, "XLSX round-trip changed style {style} for {kind:?}");
        }
    }
}

/// Excel stores legacy styles 1 and 9 in mc:Fallback,
/// alongside c14 Choice values 101 and 109.
#[test]
fn excel_alternate_content_chart_styles_roundtrip() {
    use crate::chart::{read_chart, write_chart};
    use crate::xml::parse;

    for (kind, group) in [
        (
            ChartKind::ColumnClustered,
            r#"<c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/></c:ser></c:barChart>"#,
        ),
        (
            ChartKind::BarClustered,
            r#"<c:barChart><c:barDir val="bar"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/></c:ser></c:barChart>"#,
        ),
        (
            ChartKind::Line,
            r#"<c:lineChart><c:grouping val="standard"/><c:marker val="0"/><c:ser><c:idx val="0"/><c:order val="0"/></c:ser></c:lineChart>"#,
        ),
    ] {
        for (legacy_style, modern_style) in [(1, 101), (9, 109)] {
            let xml = format!(
                r#"<c:chartSpace
                    xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"
                    xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"
                    xmlns:c14="http://schemas.microsoft.com/office/drawing/2007/8/2/chart">
                    <mc:AlternateContent>
                        <mc:Choice Requires="c14">
                            <c14:style val="{modern_style}"/>
                        </mc:Choice>
                        <mc:Fallback>
                            <c:style val="{legacy_style}"/>
                        </mc:Fallback>
                    </mc:AlternateContent>
                    <c:chart>
                        <c:plotArea>{group}</c:plotArea>
                    </c:chart>
                </c:chartSpace>"#
            );

            let root = parse(xml.as_bytes()).unwrap();
            let chart = read_chart(&root).unwrap();

            assert_eq!(chart.kind, kind);
            assert_eq!(chart.style, legacy_style, "Excel AlternateContent style for {kind:?}");

            let saved = write_chart(&Workbook::default(), 0, &chart, "Book.xlsx");

            let reopened = read_chart(&parse(saved.as_bytes()).unwrap()).unwrap();

            assert_eq!(reopened.style, legacy_style, "Style changed when re-exporting {kind:?}");
        }
    }
}
