use gridcraft_model::{Chart, ChartKind, LegendPos, Series, Sheet, Workbook};

#[test]
fn stock_close_values_keep_a_native_marker_through_repeated_roundtrips() {
    for count in [3, 4] {
        let mut wb = Workbook::new();
        let mut sheet = Sheet::new("Data");
        sheet.charts.push(Chart {
            id: 1,
            kind: ChartKind::Stock,
            anchor: Default::default(),
            title: None,
            series: (0..count)
                .map(|i| Series {
                    name: Some(format!("Series {i}")),
                    categories: Some("{\"One\",\"Two\",\"Three\"}".into()),
                    values: "{12,14,11}".into(),
                    ..Default::default()
                })
                .collect(),
            legend: LegendPos::Bottom,
            data_labels: false,
            data_table: None,
            gridlines: true,
            style: 2,
            x_title: None,
            y_title: None,
            source: None,
            by_rows: false,
            pivot: None,
            hole_size: 75,
            first_slice_angle: 0,
            format: Default::default(),
        });
        wb.sheets = vec![std::sync::Arc::new(sheet)];
        for cycle in 0..3 {
            let bytes = crate::write_xlsx(&wb).unwrap();
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            let mut xml = String::new();
            std::io::Read::read_to_string(&mut zip.by_name("xl/charts/chart1.xml").unwrap(), &mut xml).unwrap();
            let series: Vec<_> = xml.split("<c:ser>").skip(1).map(|s| s.split("</c:ser>").next().unwrap()).collect();
            assert_eq!(series.len(), count);
            for (i, ser) in series.iter().enumerate() {
                let expected = if i + 1 == count { "dash" } else { "none" };
                assert!(ser.contains(&format!("<c:symbol val=\"{expected}\"/>")), "{count} series, cycle {cycle}, index {i}: {ser}");
            }
            wb = crate::read_xlsx(&bytes).unwrap().0;
            assert_eq!(wb.active().unwrap().charts[0].kind, ChartKind::Stock);
        }
    }
}
