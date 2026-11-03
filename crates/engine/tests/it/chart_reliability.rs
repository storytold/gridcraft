//! Regression coverage for complete chart copy/edit/save/render workflows.

use gridcraft_engine::{
    Session, io,
    model::{Chart, Color, LineDash, Trendline, TrendlineKind},
};
use serde_json::json;

fn fixture() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "values":[
            ["Quarter","North","South"], ["Q1",2,3], ["Q2",4,5], ["Q3",6,8], ["Q4",8,12], ["Q5",10,17]
        ]}),
    )
    .unwrap();
    s.execute("insert.chart", json!({"range":"A1:C6","type":"line","title":"Audit","at":"E2"})).unwrap();
    s
}

fn last(s: &Session) -> &Chart {
    s.doc().unwrap().wb.active().unwrap().charts.last().unwrap()
}

/// A formatted chart keeps upstream data-table settings through duplication and undo.
#[test]
fn formatting_and_duplication_keep_data_table_settings() {
    let mut s = fixture();
    let id = last(&s).id;
    s.execute("chart.set", json!({"chart":id,"dataTable":true,"dataTableKeys":false,"format":{"y_axis":{"minimum":-5.0,"maximum":25.0}}})).unwrap();
    let original = last(&s).clone();
    assert!(!original.data_table.as_ref().unwrap().show_keys);
    s.execute("chart.duplicate", json!({"chart":id})).unwrap();
    assert_eq!(last(&s).data_table, original.data_table);
    assert_eq!(last(&s).format, original.format);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(last(&s), &original);
    s.execute("chart.set", json!({"chart":id,"type":"pie"})).unwrap();
    assert!(last(&s).data_table.is_none(), "unsupported chart kinds still remove data tables");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(last(&s), &original);
}

fn with_trends(s: &mut Session) -> Chart {
    let mut snapshot = last(s).clone();
    for (index, series) in snapshot.series.iter_mut().enumerate() {
        series.trendlines = [
            TrendlineKind::Linear,
            TrendlineKind::Polynomial(3),
            TrendlineKind::Exponential,
            TrendlineKind::Logarithmic,
            TrendlineKind::Power,
            TrendlineKind::MovingAverage(2),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| Trendline {
            kind,
            name: Some(format!("Fit {index}/{i}")),
            color: Some(Color::Rgb(0xA01030 + i as u32 * 0x100 + index as u32)),
            width: Some(2.5),
            dash: Some(LineDash::Dash),
            display_equation: true,
            display_r_squared: true,
            ..Default::default()
        })
        .collect();
    }
    s.execute("chart.paste", json!({"snapshot":snapshot})).unwrap();
    s.execute(
        "chart.set",
        json!({"format":{
            "legend_overlay":true,"legend_position":[0.1,0.2],"trendline_label_position":[0.4,0.1],
            "vertical_gridlines":true,"y_axis":{"minimum":1.0,"maximum":30.0,"major_unit":5.0,"minor_unit":1.0,"reversed":true},
            "x_axis":{"reversed":true},
            "chart_area":{"fill":[245,245,245,255],"border_width":2.0},"plot_area":{"fill":[235,240,250,255]},
            "text":{"chart":{"name":"DejaVu Sans","size":10.0,"italic":true,"color":[32,32,32,255]},"title":{"size":16.0,"bold":true}},
            "series":{"0":{"marker":"diamond","marker_size":8.0,"line_width":2.0,"line_color":[22,88,144,255]}}
        }}),
    )
    .unwrap();
    last(s).clone()
}

#[test]
fn trendlines_survive_duplicate_cross_sheet_paste_reopen_edit_and_image_export() {
    let mut s = fixture();
    let original = with_trends(&mut s);
    let duplicate_id = s.execute("chart.duplicate", json!({"chart":original.id})).unwrap()["chart"].as_u64().unwrap() as u32;
    let duplicate = last(&s).clone();
    assert_ne!(duplicate_id, original.id);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(last(&s), &original);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(last(&s), &duplicate);
    s.execute("home.insertSheet", json!({"name":"Copies"})).unwrap();
    s.execute("chart.paste", json!({"snapshot":original,"at":"C3"})).unwrap();
    let pasted = last(&s).clone();
    assert_ne!(pasted.id, original.id);
    assert_ne!(pasted.id, duplicate.id);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().charts.is_empty());
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(last(&s), &pasted);

    let saved = gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap();
    let mut reopened = Session::new();
    reopened.execute("file.open", json!({"name":"audit.xlsx","base64":io::base64_encode(&saved)})).unwrap();
    let c = last(&reopened).clone();
    // Native XML may spell out implicit border colors and marker defaults. Compare the
    // resulting drawing rather than requiring identical representations of those defaults.
    let wb = &reopened.doc().unwrap().wb;
    let draw = |chart| {
        gridcraft_chart::render(
            chart,
            &gridcraft_chart::resolve(wb, wb.active_sheet(), chart),
            c.anchor.width,
            c.anchor.height,
            &gridcraft_chart::ApproxMeasure,
        )
    };
    assert_eq!(draw(&c), draw(&original));
    for (actual, expected) in c.series.iter().zip(&original.series) {
        assert_eq!(actual.trendlines, expected.trendlines);
        assert_eq!(actual.values, expected.values);
        assert_eq!(actual.categories, expected.categories);
    }
    assert_eq!(c.series.len(), original.series.len());
    // An imported format must still pass the command validator when edited again.
    reopened.execute("chart.set", json!({"chart":c.id,"title":"Edited after reopen","format":c.format})).unwrap();
    let wb = &reopened.doc().unwrap().wb;
    let data = gridcraft_chart::resolve(wb, wb.active_sheet(), last(&reopened));
    assert_eq!(data.series[0].values, vec![Some(2.0), Some(4.0), Some(6.0), Some(8.0), Some(10.0)]);
    let prims = gridcraft_chart::render(last(&reopened), &data, c.anchor.width, c.anchor.height, &gridcraft_chart::ApproxMeasure);
    for trend in data.series.iter().flat_map(|series| &series.trendlines) {
        assert!(
            prims.iter().any(|p| matches!(p, gridcraft_chart::Prim::Line{color,width,..} if *color==trend.color && *width==2.5)),
            "missing {:?}",
            trend.spec.kind
        );
    }
    assert!(prims.iter().any(|p| matches!(p, gridcraft_chart::Prim::Text{role,text,..} if role=="trendline" && text.contains("R²"))));
    for transparent in [false, true] {
        let svg = reopened.execute("chart.export", json!({"chart":c.id,"format":"svg","scale":1.5,"transparent":transparent})).unwrap();
        let svg = String::from_utf8(io::base64_decode(svg["base64"].as_str().unwrap()).unwrap()).unwrap();
        assert!(svg.contains("Edited after reopen") && svg.contains("#a01030"));
        let png = match reopened.execute("chart.export", json!({"chart":c.id,"format":"png","scale":1.5,"transparent":transparent})) {
            Ok(png) => png,
            Err(gridcraft_engine::EngineError::BadParams { cmd, msg })
                if cmd == "chart.export" && msg == "PNG export needs installed fonts; use SVG on this platform" =>
            {
                // This is the native export contract on a fontless host, already covered by
                // the engine export test. SVG and model/render assertions above still run.
                eprintln!("fontless native host: chart.export returned its documented PNG error");
                continue;
            }
            Err(error) => panic!("unexpected PNG export failure: {error}"),
        };
        let png = image::load_from_memory(&io::base64_decode(png["base64"].as_str().unwrap()).unwrap()).unwrap().to_rgba8();
        assert_eq!(png.dimensions(), ((c.anchor.width * 1.5) as u32, (c.anchor.height * 1.5) as u32));
        assert_eq!(png.get_pixel(4, 4).0[3], if transparent { 0 } else { 255 });
    }
}

#[test]
fn copying_cells_and_removing_duplicates_keeps_chart_trendlines() {
    let mut s = fixture();
    let original = with_trends(&mut s);
    s.execute("edit.copy", json!({"range":"A2:C2"})).unwrap();
    s.execute("edit.paste", json!({"at":"A7"})).unwrap();
    let removed = s.execute("data.removeDuplicates", json!({"range":"A1:C7","header":true})).unwrap();
    assert_eq!(removed["removed"], 1);
    assert_eq!(last(&s), &original);
    let wb = &s.doc().unwrap().wb;
    let data = gridcraft_chart::resolve(wb, wb.active_sheet(), last(&s));
    assert_eq!(data.series[0].trendlines.len(), 6);
    assert!(
        gridcraft_chart::render(last(&s), &data, 480.0, 300.0, &gridcraft_chart::ApproxMeasure)
            .iter()
            .any(|p| matches!(p,gridcraft_chart::Prim::Text{role,..} if role=="trendline"))
    );
}

#[test]
fn changing_colors_on_a_large_pasted_chart_does_not_overflow() {
    for palette in ["monochrome", "accent6"] {
        let mut s = fixture();
        let mut snapshot = last(&s).clone();
        snapshot.series.resize(256, snapshot.series[0].clone());
        s.execute("chart.paste", json!({"snapshot":snapshot})).unwrap();
        let original = last(&s).clone();
        s.execute("chart.changeColors", json!({"chart":original.id,"palette":palette})).unwrap();
        assert!(last(&s).series.iter().all(|series| series.color.is_some()));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(last(&s), &original);
    }
}

#[test]
fn monochrome_is_per_chart_and_survives_duplicate_and_save() {
    let mut s = fixture();
    let first = last(&s).clone();
    s.execute("chart.duplicate", json!({"chart":first.id})).unwrap();
    let second = last(&s).clone();
    s.execute("chart.changeColors", json!({"chart":second.id,"palette":"monochrome"})).unwrap();
    let changed = last(&s).clone();
    assert_ne!(changed.series[0].color, second.series[0].color);
    assert_eq!(s.doc().unwrap().wb.active().unwrap().charts[0], first);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(last(&s), &second);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(last(&s), &changed);
    let wb = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap()).unwrap().0;
    assert_eq!(wb.active().unwrap().charts[0].series, first.series);
    let color = |chart: &Chart| chart.series.iter().map(|series| series.color.and_then(|color| color.resolve(&wb.theme))).collect::<Vec<_>>();
    assert_eq!(color(&wb.active().unwrap().charts[1]), color(&changed));
}

#[test]
fn chart_paste_keeps_valid_boundaries_and_rejects_malformed_snapshots_atomically() {
    let mut s = fixture();
    let original = last(&s).clone();
    for (style, order, forecast, expected_style, expected_order, expected_forecast) in [
        (0, 0, -1.0, 1, 2, 0.0),
        (1, 1, 0.0, 1, 2, 0.0),
        (16, 2, 2.5, 16, 2, 2.5),
        (17, 6, 100000.0, 16, 6, 100000.0),
        (u32::MAX, 7, 1e300, 16, 6, 100000.0),
        (1, 255, 0.0, 1, 6, 0.0),
    ] {
        let mut snapshot = original.clone();
        snapshot.style = style;
        snapshot.series[0].trendlines = vec![
            Trendline { kind: TrendlineKind::Polynomial(order), forward: forecast, backward: forecast, ..Default::default() },
            Trendline { kind: TrendlineKind::Exponential, forward: 2.5, ..Default::default() },
        ];
        s.execute("chart.paste", json!({"snapshot":snapshot})).unwrap();
        let chart = last(&s);
        assert_eq!(chart.style, expected_style);
        assert_eq!(chart.series[0].trendlines[0].kind, TrendlineKind::Polynomial(expected_order));
        assert_eq!(chart.series[0].trendlines[0].forward, expected_forecast);
        assert_eq!(chart.series[0].trendlines[0].backward, expected_forecast);
        assert_eq!(chart.series[0].trendlines[1], snapshot.series[0].trendlines[1]);
    }
    for snapshot in [
        json!(null),
        json!("invalid"),
        json!({}),
        {
            let mut value = serde_json::to_value(&original).unwrap();
            value["style"] = json!(-1);
            value
        },
        {
            let mut value = serde_json::to_value(&original).unwrap();
            value["series"][0]["trendlines"] = json!([{"kind":{"Polynomial":-1}}]);
            value
        },
    ] {
        let before = s.doc().unwrap().wb.clone();
        let history = s.doc().unwrap().undo.len();
        assert!(s.execute("chart.paste", json!({"snapshot":snapshot})).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
        assert_eq!(s.doc().unwrap().undo.len(), history);
    }
}
