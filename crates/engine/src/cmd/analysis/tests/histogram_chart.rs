use super::{Json, Value, json, s};
use gridcraft_model::{ChartKind, Workbook};

fn input() -> crate::Session {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [[1], [3], [3], [4], [5], [6], [8]]})).unwrap();
    s.execute("range.setValues", json!({"range": "B1", "values": [[2], [4], [6]]})).unwrap();
    s
}

fn resolved(wb: &Workbook, si: usize) -> gridcraft_chart::ChartData {
    gridcraft_chart::resolve(wb, si, &wb.sheets[si].charts[0])
}

#[test]
fn histogram_chart_output_is_opt_in_and_plots_counts_without_rebinning() {
    let mut s = input();
    let mut p = json!({"tool": "histogram", "range": "A1:A7", "bins": "B1:B3", "output": "E4"});
    let out = s.execute("data.analysisToolPak", p.clone()).unwrap();
    assert!(out.get("chart").is_none());
    assert!(s.active().unwrap().wb.sheets[0].charts.is_empty());
    s.execute("edit.undo", json!({})).unwrap();
    let before = s.active().unwrap().wb.clone();
    p["chartOutput"] = json!(true);
    let out = s.execute("data.analysisToolPak", p).unwrap();
    let d = s.active().unwrap();
    let c = &d.wb.sheets[0].charts[0];
    assert_eq!(out["chart"], c.id);
    assert_eq!(c.kind, ChartKind::ColumnClustered);
    assert_eq!(c.anchor.cell.a1(), "H4");
    let data = resolved(&d.wb, 0);
    assert_eq!(data.categories, ["2", "4", "6", "More"]);
    assert_eq!(data.series.len(), 1);
    assert_eq!(data.series[0].values, [Some(1.0), Some(3.0), Some(2.0), Some(1.0)]);
    assert_eq!(data.series[0].name, "Frequency");
    let with_chart = d.wb.clone();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb, before, "one Undo removes both chart and report");
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb, with_chart);
    // The analysis report is static; the chart links to its report cells.
    s.execute("cell.set", json!({"cell": "A1", "input": "99"})).unwrap();
    assert_eq!(resolved(&s.active().unwrap().wb, 0), data);
    s.execute("cell.set", json!({"cell": "F5", "input": "12"})).unwrap();
    assert_eq!(resolved(&s.active().unwrap().wb, 0).series[0].values[0], Some(12.0));
}

#[test]
fn histogram_chart_output_handles_pareto_and_cumulative_options_and_xlsx() {
    for pareto in [false, true] {
        for cumulative in [false, true] {
            let mut s = input();
            let before = s.active().unwrap().wb.clone();
            let out = s
                .execute(
                    "data.analysisToolPak",
                    json!({
                        "tool": "histogram", "range": "Sheet1!A1:A7", "bins": "Sheet1!B1:B3", "newSheet": "Hist's results",
                        "chartOutput": true, "pareto": pareto, "cumulative": cumulative,
                    }),
                )
                .unwrap();
            assert_eq!(out["sheet"], "Hist's results");
            let d = s.active().unwrap();
            assert_eq!(d.wb.active_sheet(), 1);
            let c = &d.wb.sheets[1].charts[0];
            assert_eq!(c.kind, if cumulative { ChartKind::Combo } else { ChartKind::ColumnClustered });
            assert_eq!(c.series.len(), if cumulative { 2 } else { 1 });
            assert!(c.series[0].values.starts_with("'Hist''s results'!"));
            let data = resolved(&d.wb, 1);
            assert_eq!(data.categories, if pareto { ["4", "6", "2", "More"] } else { ["2", "4", "6", "More"] });
            assert_eq!(
                data.series[0].values,
                if pareto { [Some(3.0), Some(2.0), Some(1.0), Some(1.0)] } else { [Some(1.0), Some(3.0), Some(2.0), Some(1.0)] }
            );
            if cumulative {
                assert!(data.series[1].secondary);
                assert_eq!(data.series[1].name, "Cumulative %");
                assert_eq!(data.series[1].number_format, "0.00%");
                assert_eq!(data.series[1].values.last(), Some(&Some(1.0)));
                assert_eq!(data.series[1].values[0], Some(if pareto { 3.0 / 7.0 } else { 1.0 / 7.0 }));
            }
            let bytes = gridcraft_xlsx::write_xlsx(&d.wb).unwrap();
            let (back, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
            assert_eq!(resolved(&back, 1), data, "chart series, categories and number formats survive XLSX");
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(s.active().unwrap().wb, before, "the new report sheet and chart undo together");
        }
    }
}

#[test]
fn histogram_chart_output_uses_labels_and_automatic_bins() {
    let mut s = s();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Measurements"], [1], [2], [3], [4]]})).unwrap();
    s.execute("data.analysisToolPak", json!({"tool": "Histogram", "range": "A1:A5", "labels": true, "chartOutput": true})).unwrap();
    let d = s.active().unwrap();
    let data = resolved(&d.wb, 1);
    assert_eq!(data.categories.last().map(String::as_str), Some("More"));
    assert_eq!(data.series[0].values.iter().flatten().sum::<f64>(), 4.0);
}

#[test]
fn histogram_chart_output_survives_repeated_row_column_switches() {
    for pareto in [false, true] {
        for cumulative in [false, true] {
            for reopen in [false, true] {
                let mut s = input();
                s.execute(
                    "data.analysisToolPak",
                    json!({"tool": "histogram", "range": "A1:A7", "bins": "B1:B3", "newSheet": "Hist's results",
                        "chartOutput": true, "pareto": pareto, "cumulative": cumulative}),
                )
                .unwrap();
                // Category text is editable, including the overflow bin in the sorted block.
                let bin_col = if pareto { if cumulative { "D" } else { "C" } } else { "A" };
                s.execute("cell.set", json!({"cell": format!("{bin_col}5"), "input": "Overflow"})).unwrap();
                if reopen {
                    let bytes = s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
                    s.execute("file.open", json!({"name": "histogram.xlsx", "base64": bytes["base64"]})).unwrap();
                    s.execute("sheet.activate", json!({"sheet": "Hist's results"})).unwrap();
                }
                let mut chart = s.active().unwrap().wb.sheets[1].charts[0].clone();
                // XLSX's ordinary column series inherit the chart kind rather than storing it.
                for se in &mut chart.series {
                    se.kind.get_or_insert(chart.kind);
                }
                let data = resolved(&s.active().unwrap().wb, 1);
                for _ in 0..2 {
                    s.execute("chart.switchRowColumn", json!({"chart": chart.id})).unwrap();
                    let switched = &s.active().unwrap().wb.sheets[1].charts[0];
                    assert!(switched.by_rows);
                    assert_eq!(switched.series.len(), 4, "one series per bin including More");
                    assert!(switched.series.iter().all(|se| se.name.is_some()));
                    s.execute("chart.switchRowColumn", json!({"chart": chart.id})).unwrap();
                    let restored = &s.active().unwrap().wb.sheets[1].charts[0];
                    assert!(!restored.by_rows);
                    assert_eq!(restored.kind, chart.kind);
                    assert_eq!(restored.series, chart.series, "restore numeric categories, series kinds and secondary axis");
                    assert_eq!(resolved(&s.active().unwrap().wb, 1), data);
                }
            }
        }
    }
}

#[test]
fn histogram_chart_switch_refuses_more_than_255_series_without_changes() {
    for bins in [254, 255] {
        let mut s = input();
        s.execute("range.setValues", json!({"range": "B1", "values": (0..bins).map(|i| vec![i]).collect::<Vec<_>>()})).unwrap();
        s.execute("data.analysisToolPak", json!({"tool": "histogram", "range": "A1:A7", "bins": format!("B1:B{bins}"), "chartOutput": true}))
            .unwrap();
        let before = s.active().unwrap().wb.clone();
        let history = s.execute("history.list", json!({})).unwrap();
        let out = s.execute("chart.switchRowColumn", json!({}));
        if bins == 254 {
            out.unwrap();
            assert_eq!(s.active().unwrap().wb.sheets[1].charts[0].series.len(), 255);
            assert_eq!(resolved(&s.active().unwrap().wb, 1).series.len(), 255);
        } else {
            assert!(out.unwrap_err().to_string().contains("255"));
            assert_eq!(s.active().unwrap().wb, before);
            assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
        }
    }
}

#[test]
fn histogram_report_column_chart_stays_columns_after_switching() {
    let mut s = input();
    s.execute("data.analysisToolPak", json!({"tool": "histogram", "range": "A1:A7", "bins": "B1:B3", "cumulative": true})).unwrap();
    s.execute("insert.chart", json!({"type": "column", "range": "A1:C5"})).unwrap();
    let before = resolved(&s.active().unwrap().wb, 1);
    assert_eq!(before.series.len(), 2);
    for _ in 0..2 {
        s.execute("chart.switchRowColumn", json!({})).unwrap();
        s.execute("chart.switchRowColumn", json!({})).unwrap();
        let d = s.active().unwrap();
        let c = &d.wb.sheets[1].charts[0];
        assert_eq!(c.kind, ChartKind::ColumnClustered);
        assert!(c.series.iter().all(|se| !se.secondary && se.kind.unwrap_or(c.kind) == ChartKind::ColumnClustered));
        assert_eq!(resolved(&d.wb, 1), before);
    }
}

#[test]
fn histogram_report_recognition_keeps_unrelated_numeric_columns() {
    for (values, range, series) in [
        (json!([["Bin", "Amount"], [1, 10], [2, 20], ["Overflow", 1]]), "A1:B4", 2),
        (json!([["Bin", "Frequency"], [1, "Pending"], [2, "Pending"], ["Overflow", "Pending"]]), "A1:B4", 2),
        (json!([["Bin", "Frequency", "Cumulative %"], [1, 10, 10], [2, 20, 30], ["Overflow", 1, 31]]), "A1:C4", 3),
    ] {
        let mut s = s();
        s.execute("range.setValues", json!({"range": "A1", "values": values})).unwrap();
        s.execute("insert.chart", json!({"type": "column", "range": range})).unwrap();
        let data = resolved(&s.active().unwrap().wb, 0);
        assert_eq!(data.series.len(), series, "unrelated data must retain its numeric Bin series");
        assert_eq!(data.series[0].values, [Some(1.0), Some(2.0), None]);
    }
}

fn refused(s: &mut crate::Session, params: Json) {
    let before = s.active().unwrap().wb.clone();
    let history = s.execute("history.list", json!({})).unwrap();
    assert!(s.execute("data.analysisToolPak", params).is_err());
    assert_eq!(s.active().unwrap().wb, before);
    assert_eq!(s.execute("history.list", json!({})).unwrap(), history);
}

#[test]
fn histogram_chart_output_refusals_preserve_report_charts_and_undo() {
    let mut s = input();
    let p = json!({"tool": "histogram", "range": "A1:A7", "bins": "B1:B3", "chartOutput": true, "output": "E1"});
    s.execute("cell.set", json!({"cell": "E1", "input": "Keep this"})).unwrap();
    refused(&mut s, p.clone());
    let mut overwrite = p.clone();
    overwrite["overwrite"] = json!(true);
    s.execute("data.analysisToolPak", overwrite).unwrap();
    assert_eq!(s.active().unwrap().wb.sheets[0].charts.len(), 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active().unwrap().wb.sheets[0].value(gridcraft_core::CellRef::parse("E1").unwrap()), Value::from("Keep this"));
    assert!(s.active().unwrap().wb.sheets[0].charts.is_empty());
    let mut edge = p.clone();
    edge["output"] = json!("XFC1048572");
    refused(&mut s, edge);
    s.execute("review.protectSheet", json!({})).unwrap();
    let mut protected = p;
    protected["output"] = json!("L1");
    refused(&mut s, protected);
}

#[test]
fn histogram_chart_output_places_near_edge_below_report_and_bounds_categories() {
    let mut s = input();
    s.execute("data.analysisToolPak", json!({"tool": "histogram", "range": "A1:A7", "bins": "B1:B3", "chartOutput": true, "output": "XFC1"}))
        .unwrap();
    assert_eq!(s.active().unwrap().wb.sheets[0].charts[0].anchor.cell.row, 6);
    s.execute("range.setValues", json!({"range": "C1", "values": (0..20_000).map(|i| vec![i]).collect::<Vec<_>>()})).unwrap();
    let p = json!({"tool": "histogram", "range": "A1:A7", "bins": "C1:C20000", "chartOutput": true, "newSheet": "Too many bins"});
    refused(&mut s, p);
}
