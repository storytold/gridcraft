use super::*;
use gridcraft_model::ChartDataTable;

#[test]
fn percent_stacked_line_table_keys_use_lines_and_optional_markers() {
    let color = [1, 2, 3, 255];
    for (kind, markers) in [(ChartKind::LineStacked100, false), (ChartKind::LineStacked100Markers, true)] {
        let mut c = chart(kind);
        c.legend = LegendPos::None;
        c.data_labels = false;
        c.data_table = Some(ChartDataTable::default());
        c.format.text.insert("data_labels".into(), gridcraft_model::ChartTextFormat { size: Some(20.0), ..Default::default() });
        let data = ChartData {
            categories: vec!["Q1".into(), "Q2".into()],
            series: vec![series("Measured", &[2.0, 4.0], color, kind)],
            ..Default::default()
        };
        let p = render(&c, &data, 600.0, 400.0, &ApproxMeasure);
        let y = p
            .iter()
            .find_map(|p| match p {
                Prim::Text { text, y, .. } if text == "Measured" => Some(*y),
                _ => None,
            })
            .unwrap();
        assert!(
            p.iter().any(|p| matches!(p, Prim::Line { pts, color: c, .. }
            if *c == color && pts.len() == 2 && pts.iter().all(|point| (point[1] - y).abs() < 0.01))),
            "{kind:?}"
        );
        assert!(
            !p.iter().any(|p| matches!(p, Prim::Rect { y: top, h, fill: Some(c), .. }
            if *c == color && (top + h / 2.0 - y).abs() < 0.01)),
            "table key must not be a box"
        );
        assert_eq!(
            p.iter().any(|p| matches!(p, Prim::Circle { cy, fill: Some(c), .. }
            if *c == color && (*cy - y).abs() < 0.01)),
            markers,
            "{kind:?}"
        );
    }
}

#[test]
fn tables_draw_source_values_for_every_supported_chart_kind() {
    for kind in ALL_KINDS {
        let mut c = chart(kind);
        c.legend = LegendPos::None;
        c.data_labels = false;
        c.data_table = Some(ChartDataTable::default());
        let supported = c.supports_data_table();
        let d = sample(kind);
        let prims = render(&c, &d, 900.0, 500.0, &ApproxMeasure);
        assert_sane(&prims, 900.0, 500.0);
        let labels = texts(&prims);
        // Source amounts, including on 100% stacked charts, not shares or running totals.
        if supported {
            let table_sizes = prims
                .iter()
                .filter_map(|p| match p {
                    Prim::Text { size, role, .. } if role == "data_table" => Some(*size),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert!(!table_sizes.is_empty(), "{kind:?}: data-table text has a dedicated rendering role");
            assert!(
                table_sizes.iter().all(|size| (*size - 9.0).abs() < f32::EPSILON),
                "data-label font settings must not resize table text: {kind:?}: {table_sizes:?}"
            );
            for value in ["$12,000", "-$4,000", "$38,000", "$61,870"] {
                assert!(labels.contains(&value), "{kind:?}: missing {value}");
            }
        } else {
            c.data_table = None;
            assert_eq!(prims, render(&c, &d, 900.0, 500.0, &ApproxMeasure));
        }
    }
}

#[test]
fn table_columns_align_with_plot_and_keep_missing_values_blank() {
    let mut c = chart(ChartKind::ColumnClustered);
    c.title = Some(String::new());
    c.x_title = None;
    c.y_title = None;
    c.legend = LegendPos::None;
    c.data_labels = false;
    c.data_table = Some(ChartDataTable::default());
    let color = [1, 2, 3, 255];
    let mut s = series("Rate", &[0.25, 0.0, 0.75], color, c.kind);
    s.values[1] = None;
    s.number_format = "0.0%".into();
    let d = ChartData { categories: ["Jan", "Feb", "Mar"].map(String::from).to_vec(), series: vec![s], ..Default::default() };
    let p = render(&c, &d, 600.0, 400.0, &ApproxMeasure);
    let at = |text: &str| -> (f32, f32) {
        p.iter()
            .find_map(|p| match p {
                Prim::Text { text: t, x, y, .. } if t == text => Some((*x, *y)),
                _ => None,
            })
            .unwrap()
    };
    let (x1, y1) = at("25.0%");
    let (x3, y3) = at("75.0%");
    let header_y = at("Jan").1;
    assert!(y1 > header_y && (y1 - y3).abs() < 0.01);
    assert_eq!(at("Jan").0, x1);
    assert_eq!(at("Mar").0, x3);
    assert_eq!(texts(&p).iter().filter(|t| **t == "Jan").count(), 1, "the table replaces category-axis labels");
    let centers: Vec<f32> = p
        .iter()
        .filter_map(|p| match p {
            Prim::Rect { x, y, w, fill: Some(f), .. } if *f == color && *y < header_y => Some(x + w / 2.0),
            _ => None,
        })
        .collect();
    assert_eq!(centers.len(), 2);
    assert!((centers[0] - x1).abs() < 0.01 && (centers[1] - x3).abs() < 0.01);
    let cells: Vec<&str> = p
        .iter()
        .filter_map(|p| match p {
            Prim::Text { y, text, .. } if (*y - y1).abs() < 0.01 => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(cells, ["Rate", "25.0%", "75.0%"], "blank cells are not zeroes");
    assert!(
        p.iter().any(|p| matches!(p, Prim::Rect { y, fill: Some(f), .. } if *f == color && *y > header_y)),
        "legend key independent of hidden chart legend"
    );
    c.data_table = Some(ChartDataTable { show_keys: false, ..Default::default() });
    let off = render(&c, &d, 600.0, 400.0, &ApproxMeasure);
    assert!(!off.iter().any(|p| matches!(p, Prim::Rect { y, fill: Some(f), .. } if *f == color && *y > header_y)));
}

#[test]
fn reversed_category_order_keeps_data_table_labels_and_values_aligned() {
    let mut c = chart(ChartKind::ColumnClustered);
    c.title = None;
    c.x_title = None;
    c.y_title = None;
    c.legend = LegendPos::None;
    c.data_labels = false;
    c.format.x_axis.reversed = true;
    c.data_table = Some(ChartDataTable::default());
    let mut values = series("Values", &[11.0, 22.0, 33.0], [1, 2, 3, 255], c.kind);
    values.number_format = "0".into();
    let data = ChartData { categories: ["Alpha", "Beta", "Gamma"].map(String::from).to_vec(), series: vec![values], ..Default::default() };
    let prims = render(&c, &data, 600.0, 400.0, &ApproxMeasure);
    let x = |wanted: &str| {
        prims
            .iter()
            .find_map(|primitive| match primitive {
                Prim::Text { text, x, .. } if text == wanted => Some(*x),
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing table text {wanted:?}"))
    };
    assert!((x("Gamma") - x("33")).abs() < 0.01);
    assert!((x("Beta") - x("22")).abs() < 0.01);
    assert!((x("Alpha") - x("11")).abs() < 0.01);
}

#[test]
fn table_borders_are_independent_and_multilevel_headers_are_kept() {
    let mut c = chart(ChartKind::Line);
    c.legend = LegendPos::None;
    c.data_labels = false;
    let mut d = sample(c.kind);
    d.category_levels = vec![vec!["2025".into(), "".into(), "".into(), "2026".into(), "".into(), "".into()]];
    for (h, v, outline, want) in [(false, false, false, 0), (true, false, false, 4), (false, true, false, 6), (false, false, true, 4)] {
        c.data_table = Some(ChartDataTable { horizontal_border: h, vertical_border: v, outline, show_keys: false });
        let p = render(&c, &d, 900.0, 600.0, &ApproxMeasure);
        let year_y = p
            .iter()
            .find_map(|p| match p {
                Prim::Text { text, y, .. } if text == "2025" => Some(*y),
                _ => None,
            })
            .unwrap();
        assert!(texts(&p).contains(&"2026"));
        let lines = p.iter().filter(|p| matches!(p, Prim::Line { pts, .. } if pts.iter().all(|p| p[1] > year_y - 9.0))).count();
        assert_eq!(lines, want, "{h} {v} {outline}");
        assert_sane(&p, 900.0, 600.0);
    }
}

#[test]
fn dense_and_tiny_tables_are_bounded_and_explicit() {
    let mut c = chart(ChartKind::ColumnClustered);
    c.data_table = Some(ChartDataTable::default());
    let mut d = sample(c.kind);
    d.categories = (0..20_000).map(|i| i.to_string()).collect();
    let p = render(&c, &d, 600.0, 400.0, &ApproxMeasure);
    assert!(texts(&p).contains(&"Data table needs more space"));
    assert_sane(&p, 600.0, 400.0);
    d.categories.clear();
    for (w, h) in [(1.0, 1.0), (10.0, 10.0), (40.0, 35.0), (200.0, 100.0)] {
        assert_sane(&render(&c, &d, w, h, &ApproxMeasure), w, h);
    }
}
