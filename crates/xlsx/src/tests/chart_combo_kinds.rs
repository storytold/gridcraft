//! Importing a combination chart keeps each series' own group kind, the first group's included.

use gridcraft_model::ChartKind;

use crate::chart::read_chart;
use crate::xml::parse;

fn ser(i: usize) -> String {
    format!("<c:ser><c:idx val=\"{i}\"/><c:order val=\"{i}\"/><c:val><c:numRef><c:f>Data!$A$1:$A$5</c:f></c:numRef></c:val></c:ser>")
}

const AX: &str = "<c:axId val=\"1\"/><c:axId val=\"2\"/>";

fn bar(sers: &str) -> String {
    format!("<c:barChart><c:barDir val=\"col\"/><c:grouping val=\"clustered\"/>{sers}{AX}</c:barChart>")
}

fn line(sers: &str) -> String {
    format!("<c:lineChart><c:grouping val=\"standard\"/>{sers}{AX}</c:lineChart>")
}

fn kinds(first: &str, second: &str) -> Vec<Option<ChartKind>> {
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">\
         <c:chart><c:plotArea>{first}{second}</c:plotArea></c:chart></c:chartSpace>"
    );
    let root = parse(xml.as_bytes()).unwrap();
    let ch = read_chart(&root).unwrap();
    assert_eq!(ch.kind, ChartKind::Combo);
    ch.series.iter().map(|s| s.kind).collect()
}

#[test]
fn first_group_with_two_columns_stays_columns() {
    let k = kinds(&bar(&(ser(0) + &ser(1))), &line(&ser(2)));
    assert_eq!(k[0], Some(ChartKind::ColumnClustered));
    assert_eq!(k[1], Some(ChartKind::ColumnClustered));
    assert!(matches!(k[2], Some(ChartKind::Line | ChartKind::LineMarkers)));
}

#[test]
fn line_first_group_stays_a_line() {
    let k = kinds(&line(&ser(0)), &bar(&ser(1)));
    assert!(matches!(k[0], Some(ChartKind::Line | ChartKind::LineMarkers)));
    assert_eq!(k[1], Some(ChartKind::ColumnClustered));
}

#[test]
fn area_and_line_combo_round_trips_both_groups() {
    let area = format!("<c:areaChart><c:grouping val=\"standard\"/>{}{}</c:areaChart>{AX}", ser(0), ser(1));
    let xml = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">\
         <c:chart><c:plotArea>{area}{}</c:plotArea></c:chart></c:chartSpace>",
        line(&ser(2))
    );
    let root = parse(xml.as_bytes()).unwrap();
    let ch = read_chart(&root).unwrap();
    assert_eq!(ch.kind, ChartKind::Combo);
    assert_eq!(ch.series.len(), 3);
    let wb = gridcraft_model::Workbook::new();
    let written = crate::chart::write_chart(&wb, 0, &ch, "Book.xlsx");
    assert!(written.contains("<c:areaChart>"), "{written}");
    assert!(written.contains("<c:lineChart>"), "{written}");
    let rt = read_chart(&parse(written.as_bytes()).unwrap()).unwrap();
    assert_eq!(rt.kind, ChartKind::Combo);
    assert_eq!(rt.series.len(), 3);
    assert_eq!(rt.series[0].kind, Some(ChartKind::Area));
    assert_eq!(rt.series[1].kind, Some(ChartKind::Area));
    assert_eq!(rt.series[2].kind, Some(ChartKind::Line));

    // Combo chart with BarClustered + AreaStacked on primary axis and LineStacked on secondary axis.
    let bar_h = format!("<c:barChart><c:barDir val=\"bar\"/><c:grouping val=\"clustered\"/>{}{AX}</c:barChart>", ser(0));
    let area_st = format!("<c:areaChart><c:grouping val=\"stacked\"/>{}{AX}</c:areaChart>", ser(1));
    let line_sec = format!("<c:lineChart><c:grouping val=\"stacked\"/>{}<c:axId val=\"3\"/><c:axId val=\"4\"/></c:lineChart>", ser(2));
    let xml2 = format!(
        "<c:chartSpace xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">\
         <c:chart><c:plotArea>{bar_h}{area_st}{line_sec}<c:catAx><c:axId val=\"1\"/></c:catAx><c:valAx><c:axId val=\"2\"/></c:valAx><c:catAx><c:axId val=\"3\"/></c:catAx><c:valAx><c:axId val=\"4\"/></c:valAx></c:plotArea></c:chart></c:chartSpace>"
    );
    let ch2 = read_chart(&parse(xml2.as_bytes()).unwrap()).unwrap();
    assert!(ch2.series[2].secondary);
    let written2 = crate::chart::write_chart(&wb, 0, &ch2, "Book.xlsx");
    assert!(written2.contains("<c:barChart>") && written2.contains("<c:areaChart>") && written2.contains("<c:lineChart>"), "{written2}");
    let rt2 = read_chart(&parse(written2.as_bytes()).unwrap()).unwrap();
    assert_eq!(rt2.series[0].kind, Some(ChartKind::BarClustered));
    assert_eq!(rt2.series[1].kind, Some(ChartKind::AreaStacked));
    assert_eq!(rt2.series[2].kind, Some(ChartKind::LineStacked));
    assert!(rt2.series[2].secondary);
}

#[test]
fn chart_user_shapes_import_rel_and_abs_anchors_pictures_shapes_connectors_and_warn_on_skipped_pics() {
    let drawing = r#"<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <xdr:twoCellAnchor><xdr:from><xdr:col>1</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>1</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from><xdr:to><xdr:col>5</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>10</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>
        <xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="2" name="Chart 1"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr><xdr:xfrm/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rIdChart"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/>
      </xdr:twoCellAnchor>
    </xdr:wsDr>"#;
    let chart = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <c:chart><c:plotArea><c:lineChart><c:grouping val="standard"/><c:ser><c:idx val="0"/><c:val><c:numRef><c:f>Sheet1!$A$1:$A$3</c:f></c:numRef></c:val></c:ser></c:lineChart></c:plotArea></c:chart>
      <c:userShapes r:id="rIdUs"/>
    </c:chartSpace>"#;
    let user_shapes = r#"<cdr:userShapes xmlns:cdr="http://schemas.openxmlformats.org/drawingml/2006/chartDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
      <cdr:relSizeAnchor>
        <cdr:from><cdr:x>-0.25</cdr:x><cdr:y>0.6</cdr:y></cdr:from>
        <cdr:to><cdr:x>1.5</cdr:x><cdr:y>0.2</cdr:y></cdr:to>
        <cdr:pic><cdr:nvPicPr><cdr:cNvPr id="1" name="Badge" descr="ChartBadge"/><cdr:cNvPicPr/></cdr:nvPicPr><cdr:blipFill><a:blip r:embed="rIdImg"/></cdr:blipFill><cdr:spPr/></cdr:pic>
        <cdr:sp><cdr:nvSpPr><cdr:cNvPr id="2" name="Callout"/><cdr:cNvSpPr/></cdr:nvSpPr><cdr:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></cdr:spPr><cdr:txBody><a:bodyPr/><a:p><a:r><a:t>CalloutText</a:t></a:r></a:p></cdr:txBody></cdr:sp>
      </cdr:relSizeAnchor>
      <cdr:absSizeAnchor>
        <cdr:from><cdr:x>0.25</cdr:x><cdr:y>0.25</cdr:y></cdr:from>
        <cdr:ext cx="952500" cy="476250"/>
        <cdr:cxnSp><cdr:nvCxnSpPr><cdr:cNvPr id="3" name="Arrow"/><cdr:cNvCxnSpPr/></cdr:nvCxnSpPr><cdr:spPr><a:prstGeom prst="line"><a:avLst/></a:prstGeom></cdr:spPr></cdr:cxnSp>
        <cdr:pic><cdr:nvPicPr><cdr:cNvPr id="4" name="Ext"/><cdr:cNvPicPr/></cdr:nvPicPr><cdr:blipFill><a:blip r:embed="rIdExt"/></cdr:blipFill><cdr:spPr/></cdr:pic>
        <cdr:pic><cdr:nvPicPr><cdr:cNvPr id="5" name="Missing"/><cdr:cNvPicPr/></cdr:nvPicPr><cdr:blipFill><a:blip r:embed="rIdMissing"/></cdr:blipFill><cdr:spPr/></cdr:pic>
      </cdr:absSizeAnchor>
    </cdr:userShapes>"#;
    let srels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#;
    let drels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdChart" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="../charts/chart1.xml"/></Relationships>"#;
    let crels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdUs" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chartUserShapes" Target="../drawings/drawing2.xml"/></Relationships>"#;
    let usrels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/badge.png"/><Relationship Id="rIdExt" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.com/ext.png" TargetMode="External"/><Relationship Id="rIdMissing" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="../media/missing.png"/></Relationships>"#;
    let bytes = super::minimal(
        r#"<sheetData/><drawing r:id="rId1"/>"#,
        &[
            ("xl/worksheets/_rels/sheet1.xml.rels", srels),
            ("xl/drawings/drawing1.xml", drawing),
            ("xl/drawings/_rels/drawing1.xml.rels", drels),
            ("xl/charts/chart1.xml", chart),
            ("xl/charts/_rels/chart1.xml.rels", crels),
            ("xl/drawings/drawing2.xml", user_shapes),
            ("xl/drawings/_rels/drawing2.xml.rels", usrels),
            ("xl/media/badge.png", "PNGDATA"),
        ],
        "",
        "",
    );
    let (wb, rep) = crate::read_xlsx(&bytes).unwrap();
    let sh = wb.sheet(0).unwrap();
    assert_eq!(sh.charts.len(), 1);
    assert_eq!(sh.images.len(), 1);
    assert_eq!(sh.images[0].alt, "ChartBadge");
    assert_eq!(sh.images[0].anchor.height, 0.0, "to.y < from.y clamps height to 0");
    assert!(sh.images[0].anchor.width > 0.0, "from.x=-0.25..to.x=1.5 clamps to full chart width");
    assert_eq!(sh.shapes.len(), 2);
    assert_eq!(sh.shapes[0].text, "CalloutText");
    assert!((sh.shapes[1].anchor.width - 100.0).abs() < 1.0 && (sh.shapes[1].anchor.height - 50.0).abs() < 1.0);
    assert!(rep.warnings.iter().any(|w| w.contains("chart drawing was skipped")), "{:?}", rep.warnings);
    assert!(rep.warnings.iter().any(|w| w.contains("missing picture xl/media/missing.png")), "{:?}", rep.warnings);
}
