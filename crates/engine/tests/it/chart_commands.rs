//! Chart editing and clipboard/export contracts shared by the UI, CLI and MCP.

use std::sync::Arc;

use gridcraft_engine::{Session, core::CellRef, io, model::Chart};
use serde_json::{Value as Json, json};

fn source() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute(
        "range.setValues",
        json!({"range":"A1", "values":[
            [null,"North","South","East","West"],
            ["Jan",1,2,3,4], ["Feb",2,3,4,5], ["Mar",3,4,5,6],
            ["Apr",4,5,6,7], ["May",5,6,7,8]
        ]}),
    )
    .unwrap();
    s.execute("insert.chart", json!({"range":"A1:E6","type":"column","title":"Regional sales","at":"G2"})).unwrap();
    assert_eq!(chart(&s).series.len(), 4);
    s
}

fn chart(s: &Session) -> &Chart {
    s.doc().unwrap().wb.active().unwrap().charts.last().unwrap()
}

fn styled(s: &mut Session) {
    s.execute(
        "chart.set",
        json!({"format":{
            "legend_overlay":true,"legend_position":[0.3,0.2],"trendline_label_position":[0.2,0.6],
            "vertical_gridlines":true,"y_axis":{"minimum":0.0,"maximum":20.0},
            "text":{"chart":{"name":"Times New Roman","size":14.0,"italic":true},"title":{"bold":true}},
            "series":{"0":{"line_width":1.0},"1":{"line_width":2.0},"2":{"line_width":3.0},"3":{"line_width":4.0}}
        }}),
    )
    .unwrap();
}

fn bytes(out: &Json) -> Vec<u8> {
    let bytes = io::base64_decode(out["base64"].as_str().unwrap()).unwrap();
    assert_eq!(out["bytes"].as_u64(), Some(bytes.len() as u64));
    bytes
}

#[test]
fn insert_chart_appearance_is_one_undo_step_and_replays_with_undo_and_redo() {
    let mut s = source();
    let original = chart(&s).clone();
    let before = s.doc().unwrap().wb.clone();
    let history = s.doc().unwrap().undo.len();
    let journal = s.journal.len();
    let params = json!({
        "range":"A1:E6", "type":"column", "style":99, "legend":"top", "dataLabels":true, "gridlines":false,
        "format":{"border_width":2.5,"vertical_gridlines":true,"text":{"title":{"size":18.0,"bold":true}}}
    });
    let (_, preview) = gridcraft_engine::cmd::insert::build_chart(&s, &params).unwrap();
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), history);
    let result = s.execute("insert.chart", params.clone()).unwrap();
    assert_eq!(result["chart"].as_u64(), Some(u64::from(preview.id)));
    assert_eq!(chart(&s), &preview);
    assert_eq!(preview.style, 16);
    assert_eq!(preview.legend, gridcraft_engine::model::LegendPos::Top);
    assert!(preview.data_labels && !preview.gridlines);
    assert_eq!(preview.format, serde_json::from_value::<gridcraft_engine::model::ChartFormat>(params["format"].clone()).unwrap());
    assert_eq!(s.doc().unwrap().undo.len(), history + 1);
    assert_eq!(s.journal.len(), journal + 1);
    assert_eq!(s.journal.last().unwrap(), &("insert.chart".into(), params));

    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().charts.as_slice(), std::slice::from_ref(&original));
    let mut replay = Session::new();
    replay.new_workbook();
    for (id, params) in &s.journal {
        replay.execute(id, params.clone()).unwrap();
    }
    assert_eq!(replay.doc().unwrap().wb.active().unwrap().charts, [original]);

    s.execute("edit.redo", json!({})).unwrap();
    replay.execute("edit.redo", json!({})).unwrap();
    assert_eq!(chart(&s), &preview);
    assert_eq!(replay.doc().unwrap().wb.active().unwrap().charts, s.doc().unwrap().wb.active().unwrap().charts);
}

#[test]
fn insert_chart_appearance_matches_chart_set_parsing_and_defaults() {
    for (legend, style) in [("none", 0_u32), ("bottom", 1), ("top", 16), ("left", 17), ("right", u32::MAX), ("unknown", 2)] {
        let mut inserted = source();
        let mut updated = source();
        let params = json!({"legend":legend,"style":style,"dataLabels":true,"gridlines":false,"format":{"border_width":2.0}});
        let mut insertion = params.clone();
        insertion["range"] = json!("A1:E6");
        inserted.execute("insert.chart", insertion).unwrap();
        updated.execute("insert.chart", json!({"range":"A1:E6"})).unwrap();
        updated.execute("chart.set", params).unwrap();
        assert_eq!(chart(&inserted), chart(&updated), "{legend}, {style}");
    }
    let s = source();
    for kind in ["column", "pie", "doughnut", "treemap", "sunburst", "funnel"] {
        let (_, c) = gridcraft_engine::cmd::insert::build_chart(&s, &json!({"range":"A1:E6","type":kind})).unwrap();
        let pie = matches!(kind, "pie" | "doughnut");
        assert_eq!(c.legend, if pie { gridcraft_engine::model::LegendPos::Right } else { gridcraft_engine::model::LegendPos::Bottom });
        assert_eq!(c.gridlines, kind == "column");
        assert!(!c.data_labels);
        assert_eq!(c.style, 1);
        assert_eq!(c.format, Default::default());
    }
}

#[test]
fn insert_chart_invalid_format_preserves_workbook_history_redo_and_journal() {
    let mut s = source();
    s.execute("cell.set", json!({"cell":"G10","input":"42"})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    let history = s.doc().unwrap().undo.len();
    let redo = s.doc().unwrap().redo.clone();
    let revision = s.doc().unwrap().revision;
    let journal = s.journal.clone();
    for format in [
        Json::Null,
        json!("not a format"),
        json!({"text":{"title":{"size":0}}}),
        json!({"legend_position":[2,0]}),
        json!({"y_axis":{"minimum":30,"maximum":20}}),
        json!({"series":{"0":{"marker":"bogus"}}}),
    ] {
        let params = json!({"range":"A1:E6","style":16,"legend":"none","dataLabels":true,"gridlines":false,"format":format});
        assert!(gridcraft_engine::cmd::insert::build_chart(&s, &params).is_err(), "{params}");
        assert!(s.execute("insert.chart", params.clone()).is_err(), "{params}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb), "{params}");
        assert_eq!(s.doc().unwrap().undo.len(), history);
        assert_eq!(s.doc().unwrap().redo.len(), redo.len());
        assert!(s.doc().unwrap().redo.iter().zip(&redo).all(|(a, b)| Arc::ptr_eq(&a.wb, &b.wb)));
        assert_eq!(s.doc().unwrap().revision, revision);
        assert_eq!(s.journal, journal);
    }
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("G10").unwrap()).as_f64(), Some(42.0));
}

#[test]
fn duplicate_preserves_chart_and_upstream_series_properties_as_one_undo_step() {
    let mut s = source();
    styled(&mut s);
    s.execute("chart.set", json!({"quartiles":"inclusive"})).unwrap();
    s.execute("chart.addElement", json!({"element":"trendline","series":1,"type":"polynomial","order":3,"displayEquation":true})).unwrap();
    let original = chart(&s).clone();
    let before = s.doc().unwrap().undo.len();
    let result = s.execute("chart.duplicate", json!({"chart":original.id})).unwrap();
    let mut expected = original.clone();
    expected.id = result["chart"].as_u64().unwrap() as u32;
    expected.anchor.dx += 24.0;
    expected.anchor.dy += 24.0;
    assert_ne!(expected.id, original.id);
    assert_eq!(chart(&s), &expected);
    assert_eq!(s.doc().unwrap().undo.len(), before + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().charts, [original]);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(chart(&s), &expected);

    let wb = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&s.doc().unwrap().wb).unwrap()).unwrap().0;
    let reopened = wb.active().unwrap().charts.last().unwrap();
    assert_eq!(reopened.format, expected.format);
    assert_eq!(reopened.series.len(), expected.series.len());
    for (actual, expected) in reopened.series.iter().zip(&expected.series) {
        assert_eq!(actual.values, expected.values);
        assert_eq!(actual.categories, expected.categories);
        assert_eq!(actual.trendlines, expected.trendlines);
    }
}

#[test]
fn paste_places_snapshot_on_active_sheet_and_keeps_source_references() {
    let mut s = source();
    styled(&mut s);
    let original = chart(&s).clone();
    s.execute("home.insertSheet", json!({"name":"Chart copies"})).unwrap();
    let before = s.doc().unwrap().undo.len();
    let result = s.execute("chart.paste", json!({"snapshot":original,"at":"C4"})).unwrap();
    let mut expected = original.clone();
    expected.id = result["chart"].as_u64().unwrap() as u32;
    expected.anchor.cell = CellRef::parse("C4").unwrap();
    expected.anchor.dx = 0.0;
    expected.anchor.dy = 0.0;
    assert_ne!(expected.id, original.id);
    assert_eq!(chart(&s), &expected);
    assert_eq!(s.doc().unwrap().undo.len(), before + 1);
    let d = s.doc().unwrap();
    let data = gridcraft_chart::resolve(&d.wb, d.wb.active_sheet, chart(&s));
    assert_eq!(data.series[0].values, [Some(1.0), Some(2.0), Some(3.0), Some(4.0), Some(5.0)]);
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().charts.is_empty());
}

#[test]
fn paste_rejects_invalid_format_and_anchor_without_changing_workbook() {
    let mut s = source();
    let snapshot = serde_json::to_value(chart(&s)).unwrap();
    let invalid = [
        ("/anchor/width", json!(0)),
        ("/anchor/dx", json!(1e20)),
        ("/anchor/cell/row", json!(u32::MAX)),
        ("/format/y_axis/minimum", json!(30)),
        ("/format/legend_position", json!([2, 0])),
        ("/format/text", json!({"title":{"size":145}})),
        ("/format/series", json!({"0":{"marker":"bogus"}})),
        ("/series/0/bubble_sizes", json!("x".repeat(4097))),
    ];
    for (pointer, value) in invalid {
        let mut bad = snapshot.clone();
        if pointer == "/format/y_axis/minimum" {
            bad["format"]["y_axis"]["maximum"] = json!(20);
        }
        *bad.pointer_mut(pointer).unwrap() = value;
        let before = s.doc().unwrap().wb.clone();
        let history = s.doc().unwrap().undo.len();
        assert!(s.execute("chart.paste", json!({"snapshot":bad})).is_err(), "{pointer}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb), "{pointer}");
        assert_eq!(s.doc().unwrap().undo.len(), history);
    }
    assert!(s.execute("chart.paste", json!({})).is_err());
    assert!(s.execute("chart.paste", json!({"snapshot":snapshot,"at":"not a cell"})).is_err());
}

#[test]
fn export_returns_svg_and_png_without_history_or_journal_changes() {
    let mut s = source();
    styled(&mut s);
    s.execute("chart.set", json!({"width":160,"height":120})).unwrap();
    let before = s.doc().unwrap().wb.clone();
    let history = s.doc().unwrap().undo.len();
    let journal = s.journal.len();
    let result = s.execute("chart.export", json!({"format":"svg","scale":1.5,"transparent":true})).unwrap();
    assert_eq!(result["mime"], "image/svg+xml");
    let svg = String::from_utf8(bytes(&result)).unwrap();
    for text in ["<svg", "width=\"240\"", "height=\"180\"", "Regional sales", "Times New Roman", "font-style=\"italic\""] {
        assert!(svg.contains(text), "{text}: {svg}");
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        match s.execute("chart.export", json!({})) {
            Ok(result) => {
                assert_eq!(result["mime"], "image/png");
                let bytes = bytes(&result);
                assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
                assert_eq!(image::load_from_memory(&bytes).unwrap().into_rgba8().dimensions(), (320, 240));
            }
            Err(gridcraft_engine::EngineError::BadParams { cmd, msg })
                if cmd == "chart.export" && msg == "PNG export needs installed fonts; use SVG on this platform" =>
            {
                eprintln!("PNG assertions skipped: the native host has no system fonts; SVG and non-mutation assertions still run.");
            }
            Err(error) => panic!("unexpected chart PNG export error: {error}"),
        }
    }
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    assert_eq!(s.doc().unwrap().undo.len(), history);
    assert_eq!(s.journal.len(), journal);
}

#[test]
fn export_rejects_invalid_format_scale_and_chart() {
    let mut s = source();
    for params in [json!({"format":"exe"}), json!({"scale":0}), json!({"scale":9}), json!({"scale":1e100}), json!({"chart":99999})] {
        let before = s.doc().unwrap().wb.clone();
        assert!(s.execute("chart.export", params.clone()).is_err(), "{params}");
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    }
}

#[test]
fn deleting_series_remaps_styles_and_keeps_the_last_series() {
    let mut s = source();
    styled(&mut s);
    let original = chart(&s).clone();
    s.execute("chart.selectData", json!({"removeSeries":[0,2,2,99]})).unwrap();
    let current = chart(&s);
    assert_eq!(current.series, [original.series[1].clone(), original.series[3].clone()]);
    assert_eq!(current.format.series.len(), 2);
    assert_eq!(current.format.series.get(&0), original.format.series.get(&1));
    assert_eq!(current.format.series.get(&1), original.format.series.get(&3));
    assert_eq!(current.format.text, original.format.text);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(chart(&s), &original);
    s.execute("chart.selectData", json!({"removeSeries":[0,1,2,3]})).unwrap();
    let last = chart(&s).clone();
    assert_eq!(last.series.len(), 1);
    assert_eq!(last.format.series.get(&0), original.format.series.get(&0));
    s.execute("chart.selectData", json!({"removeSeries":[0]})).unwrap();
    assert_eq!(chart(&s), &last);
}

#[test]
fn rebuilding_series_resets_only_series_formats() {
    for (command, params) in [("chart.switchRowColumn", json!({})), ("chart.selectData", json!({"range":"A1:D6"}))] {
        let mut s = source();
        styled(&mut s);
        let original = chart(&s).clone();
        s.execute(command, params).unwrap();
        let mut expected = original.format.clone();
        expected.series.clear();
        assert_eq!(chart(&s).format, expected, "{command}");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(chart(&s), &original);
    }
}

#[test]
fn format_selection_and_element_deletion_preserve_whole_chart_until_explicit_delete() {
    let mut s = source();
    styled(&mut s);
    let original = chart(&s).clone();
    s.execute("chart.formatSelection", json!({"series":1,"color":"#123456"})).unwrap();
    assert_eq!(chart(&s).series[1].color, gridcraft_engine::model::Color::from_hex("#123456"));
    assert_eq!(chart(&s).format, original.format);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(chart(&s), &original);
    for params in [json!({"title":""}), json!({"legend":"none"}), json!({"xTitle":null}), json!({"yTitle":null}), json!({"gridlines":false})] {
        s.execute("chart.set", params).unwrap();
        assert_eq!(s.doc().unwrap().wb.active().unwrap().charts.len(), 1);
        assert_eq!(chart(&s).format, original.format);
    }
    let before_delete = chart(&s).clone();
    s.execute("chart.delete", json!({"chart":original.id})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().charts.is_empty());
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(chart(&s), &before_delete);
}

#[test]
fn chart_text_format_reaches_html_and_pdf() {
    let mut s = source();
    styled(&mut s);
    let out = s.execute("file.exportPdf", json!({})).unwrap();
    let pdf = io::base64_decode(out["base64"].as_str().unwrap()).unwrap();
    let pdf = String::from_utf8_lossy(&pdf);
    assert!(pdf.contains("/BaseFont /Times-BoldItalic"), "chart title font missing");
    assert!(pdf.contains("Regional sales"));

    s.execute("chart.set", json!({"format":{"text":{"title":{"name":"A \" & <Serif>","italic":true}}}})).unwrap();
    let html = io::to_html(&s.doc().unwrap().wb, 0).unwrap();
    assert!(html.contains("font-family=\"A &quot; &amp; &lt;Serif&gt;\""), "chart font must be escaped in HTML");
    assert!(html.contains("font-style=\"italic\""));
}
