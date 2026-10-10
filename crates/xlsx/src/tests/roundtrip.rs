//! Build a workbook using every feature, write it, read it back, compare.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};
use gridcraft_model::*;

use crate::{read_xlsx, write_xlsx};

fn at(a: &str) -> CellRef {
    CellRef::parse(a).unwrap()
}
fn rr(a: &str) -> RangeRef {
    RangeRef::parse(a).unwrap()
}

fn sample() -> Workbook {
    let mut wb = Workbook::new();
    wb.date_system = DateSystem::D1904;
    wb.calc = CalcSettings { mode: CalcMode::Manual, iterative: true, max_iterations: 42, max_change: 0.5, precision_as_displayed: true };
    wb.protected_structure = true;
    wb.props = DocProps {
        title: "T & <x>".into(),
        subject: "S".into(),
        author: "Me".into(),
        company: "Co".into(),
        keywords: "k".into(),
        description: "d".into(),
    };
    wb.theme.colors[4] = 0x112233;
    wb.theme.minor_font = "Aptos".into();

    let bold = wb.styles.derive(StyleId::DEFAULT, |s| {
        s.font.bold = true;
        s.font.color = Color::Theme(4, 400);
        s.fill = Fill::solid(Color::Rgb(0xFFEE00));
        s.borders.bottom = BorderLine { style: BorderStyle::Double, color: Color::Rgb(0xFF0000) };
        s.borders.diag_down = BorderLine::thin();
        s.align = Alignment { h: HAlign::Center, v: VAlign::Top, wrap: true, shrink: false, indent: 1, rotation: -30 };
        s.protection.locked = false;
    });
    let money = wb.styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new("#,##0.00 \"€\""));
    let pct = wb.styles.derive(StyleId::DEFAULT, |s| {
        s.num_fmt = NumFmt::new("0.00%");
        s.font = Font {
            name: "Arial".into(),
            size: 9.5,
            italic: true,
            underline: Underline::SingleAccounting,
            strike: true,
            vert: VertAlign::Subscript,
            ..Font::default()
        };
        s.fill = Fill { pattern: PatternType::LightTrellis, fg: Color::Theme(5, -250), bg: Color::Rgb(0x00FF00) };
    });
    let red = Style {
        font: Font { color: Color::Rgb(0x9C0006), bold: true, ..Font::default() },
        fill: Fill::solid(Color::Rgb(0xFFC7CE)),
        ..Style::default()
    };

    let mut s = Sheet::new("Data & Stuff");
    let set = |s: &mut Sheet, a: &str, v: Value, style: StyleId| s.cells.set(at(a), Cell { value: v, formula: None, style });
    set(&mut s, "A1", Value::text("Region"), bold);
    set(&mut s, "B1", Value::text("Amount"), bold);
    set(&mut s, "C1", Value::text("  spaced\nline _x0041_ \u{1} "), StyleId::DEFAULT);
    set(&mut s, "A2", Value::text("North"), StyleId::DEFAULT);
    set(&mut s, "B2", Value::Number(1234.5), money);
    set(&mut s, "A3", Value::text("South"), StyleId::DEFAULT);
    set(&mut s, "B3", Value::Number(-0.000123), pct);
    set(&mut s, "D1", Value::Bool(true), StyleId::DEFAULT);
    set(&mut s, "D2", Value::Error(CellError::NA), StyleId::DEFAULT);
    set(&mut s, "D3", Value::Number(1e300), StyleId::DEFAULT);
    set(&mut s, "E5", Value::Empty, bold);
    let mut f = Cell::formula(Formula::new("=SUM(B2:B3)*2"));
    f.value = Value::Number(2468.9);
    s.cells.set(at("B4"), f);
    let mut f = Cell::formula(Formula::new("XLOOKUP(\"North\",A2:A3,B2:B3)&TEXTJOIN(\",\",TRUE,A2:A3)"));
    f.value = Value::text("x");
    s.cells.set(at("C4"), f);
    let mut f = Cell::formula(Formula::new("ISNUMBER(B2)"));
    f.value = Value::Bool(true);
    s.cells.set(at("D4"), f);
    let mut f = Cell::formula(Formula::new("1/0"));
    f.value = Value::Error(CellError::Div0);
    s.cells.set(at("E4"), f);
    s.cells.set(at("F4"), Cell::formula(Formula::new("NOW()")));
    let mut arr = Formula::new("B2:B3*2");
    arr.array = Some(rr("G2:G3"));
    let mut c = Cell::formula(arr);
    c.value = Value::Number(2469.0);
    s.cells.set(at("G2"), c);
    // A dynamic-array formula with spilled values.
    let mut dynf = Cell::formula(Formula::new("SEQUENCE(3)"));
    dynf.value = Value::Number(1.0);
    s.cells.set(at("H1"), dynf);
    s.spill_ranges.insert(at("H1"), rr("H1:H3"));
    s.spill.insert(at("H2"), Value::Number(2.0));
    s.spill.insert(at("H3"), Value::Number(3.0));

    s.cols.insert(0, LineInfo { size: Some(100.0), ..Default::default() });
    s.cols.insert(1, LineInfo { size: Some(100.0), ..Default::default() });
    s.cols.insert(3, LineInfo { hidden: true, outline: 1, ..Default::default() });
    s.cols.insert(5, LineInfo { style: Some(money), ..Default::default() });
    s.rows.insert(0, LineInfo { size: Some(30.0), ..Default::default() });
    s.rows.insert(9, LineInfo { hidden: true, outline: 2, collapsed: true, ..Default::default() });
    s.rows.insert(10, LineInfo { style: Some(bold), ..Default::default() });
    s.default_row_height = 24.0;
    s.default_col_width = 72.0;
    s.merges.push(rr("A20:C21"));
    s.freeze = Some((1, 2));
    s.tab_color = Some(Color::Rgb(0x00B050));
    s.show_gridlines = false;
    s.show_headings = false;
    s.show_zeros = false;
    s.right_to_left = true;
    s.zoom = 125;
    s.view_active = at("C7");

    let red_b = Box::new(red.clone());
    let cfs = vec![
        CfRule::CellIs { op: CfOperator::Between, a: "1".into(), b: Some("10".into()), style: red_b.clone() },
        CfRule::Expression { formula: "MOD(ROW(),2)=0".into(), style: red_b.clone() },
        CfRule::ContainsText { text: "a\"b".into(), style: red_b.clone() },
        CfRule::NotContainsText { text: "zz".into(), style: red_b.clone() },
        CfRule::BeginsWith { text: "No".into(), style: red_b.clone() },
        CfRule::EndsWith { text: "th".into(), style: red_b.clone() },
        CfRule::Blanks { style: red_b.clone() },
        CfRule::NoBlanks { style: red_b.clone() },
        CfRule::Errors { style: red_b.clone() },
        CfRule::NoErrors { style: red_b.clone() },
        CfRule::Duplicate { style: red_b.clone() },
        CfRule::Unique { style: red_b.clone() },
        CfRule::Top10 { bottom: true, percent: true, rank: 5, style: red_b.clone() },
        CfRule::AboveAverage { below: true, equal: true, std_dev: 1, style: red_b.clone() },
        CfRule::TimePeriod { period: "last7Days".into(), style: red_b.clone() },
        CfRule::ColorScale {
            stops: vec![
                (CfValueKind::Min, Color::Rgb(0xF8696B)),
                (CfValueKind::Percentile(50.0), Color::Rgb(0xFFEB84)),
                (CfValueKind::Max, Color::Rgb(0x63BE7B)),
            ],
        },
        CfRule::DataBar {
            min: CfValueKind::Number(0.0),
            max: CfValueKind::Formula("$B$9".into()),
            color: Color::Rgb(0x638EC6),
            gradient: true,
            show_value: false,
        },
        CfRule::IconSet {
            set: "3Arrows".into(),
            thresholds: vec![CfValueKind::Percent(0.0), CfValueKind::Percent(33.0), CfValueKind::Percent(67.0)],
            reverse: true,
            show_value: false,
        },
    ];
    for (i, rule) in cfs.into_iter().enumerate() {
        s.cond_formats.push(CondFormat { ranges: vec![rr("B2:B10"), rr("D2")], rule, priority: i as u32 + 1, stop_if_true: i == 0 });
    }
    s.validations.push(Validation {
        ranges: vec![rr("A2:A10")],
        kind: ValidationKind::List,
        f1: "\"North,South\"".into(),
        input_title: "Pick".into(),
        input_message: "One".into(),
        ..Default::default()
    });
    s.validations.push(Validation {
        ranges: vec![rr("I1")],
        kind: ValidationKind::List,
        f1: "=$A$2:$A$3".into(),
        in_cell_dropdown: false,
        ..Default::default()
    });
    s.validations.push(Validation {
        ranges: vec![rr("B2:B10")],
        kind: ValidationKind::Decimal,
        op: CfOperator::GreaterOrEqual,
        f1: "0".into(),
        error_style: ErrorStyle::Information,
        error_title: "Bad".into(),
        error_message: "No".into(),
        allow_blank: false,
        ..Default::default()
    });
    s.hyperlinks.insert(at("A2"), Hyperlink { target: "https://example.com/?a=1&b=2".into(), tooltip: Some("go".into()) });
    s.hyperlinks.insert(at("A3"), Hyperlink { target: "'Data & Stuff'!B2".into(), tooltip: None });
    s.hyperlinks.insert(at("A4"), Hyperlink { target: "mailto:me@example.com".into(), tooltip: None });
    s.comments.insert(
        at("B2"),
        Comment { author: "Ann".into(), text: "Note & <stuff>\nline 2".into(), replies: vec![], threaded: false, resolved: false, visible: false },
    );
    s.comments
        .insert(at("C3"), Comment { author: "Bob".into(), text: "second".into(), replies: vec![], threaded: false, resolved: false, visible: false });
    s.autofilter = Some(AutoFilter {
        range: rr("K1:N20"),
        criteria: vec![
            (0, FilterCriterion::Values { values: vec!["a".into(), "b".into()], blanks: true }),
            (1, FilterCriterion::Custom { a: (">=".into(), "5".into()), b: Some(("<".into(), "9".into())), and: true }),
            (2, FilterCriterion::Top10 { bottom: true, percent: false, count: 3 }),
            (3, FilterCriterion::FillColor(Color::Rgb(0xFF0000))),
        ],
    });
    s.protection = Some(SheetProtection { format_cells: true, sort: true, select_locked: true, select_unlocked: true, ..Default::default() });
    s.print = PrintSettings {
        orientation: Orientation::Landscape,
        paper: "A4".into(),
        margins: [0.5, 0.6, 0.7, 0.8, 0.2, 0.25],
        print_area: Some(rr("A1:H30")),
        title_rows: Some((0, 1)),
        title_cols: Some((0, 0)),
        scale: 80,
        fit_to: Some((1, 0)),
        gridlines: true,
        headings: true,
        center_h: true,
        center_v: false,
        header: "&L&D&RPage &P".into(),
        footer: "&C&F".into(),
        row_breaks: vec![15, 30],
        col_breaks: vec![4],
    };
    s.sparklines.push(Sparkline { cell: at("J2"), source: "B2:B3".into(), kind: SparklineKind::WinLoss, color: Color::Rgb(0x112233), markers: true });
    s.tables.push(Table {
        id: 1,
        name: "Sales Table".into(),
        range: rr("P1:R5"),
        header_row: true,
        totals_row: true,
        columns: vec![
            TableColumn { name: "Region".into(), totals: TotalsFn::None, totals_label: Some("Total".into()), formula: None },
            TableColumn { name: "Amount".into(), totals: TotalsFn::Sum, totals_label: None, formula: None },
            TableColumn { name: "Tax".into(), totals: TotalsFn::Custom, totals_label: Some("SUM(R2:R4)".into()), formula: Some("Q2*0.1".into()) },
        ],
        style: "TableStyleLight9".into(),
        banded_rows: true,
        banded_cols: true,
        first_col: true,
        last_col: false,
        filter_button: true,
    });
    let png = b"\x89PNG\r\n\x1a\n0000IHDRfake".to_vec();
    s.images.push(Image {
        id: 10,
        anchor: Anchor { cell: at("C10"), dx: 5.0, dy: 4.0, width: 120.0, height: 80.0 },
        data: png,
        mime: "image/png".into(),
        alt: "Logo \"x\"".into(),
    });
    s.shapes.push(Shape {
        id: 11,
        kind: ShapeKind::Ellipse,
        anchor: Anchor { cell: at("E10"), dx: 0.0, dy: 0.0, width: 64.0, height: 40.0 },
        fill: Color::Rgb(0x4472C4),
        line: Color::Rgb(0x000000),
        text: "Hi\nthere".into(),
    });
    s.shapes.push(Shape {
        id: 12,
        kind: ShapeKind::Arrow,
        anchor: Anchor { cell: at("E14"), dx: 0.0, dy: 0.0, width: 64.0, height: 0.0 },
        fill: Color::Auto,
        line: Color::Rgb(0xFF0000),
        text: String::new(),
    });
    s.shapes.push(Shape {
        id: 13,
        kind: ShapeKind::TextBox,
        anchor: Anchor { cell: at("E16"), dx: 0.0, dy: 0.0, width: 64.0, height: 40.0 },
        fill: Color::Auto,
        line: Color::Auto,
        text: "box".into(),
    });
    let base_chart = Chart {
        id: 20,
        kind: ChartKind::ColumnClustered,
        anchor: Anchor { cell: at("J5"), dx: 0.0, dy: 0.0, width: 480.0, height: 288.0 },
        title: Some("Sales".into()),
        series: vec![Series {
            name: Some("'Data & Stuff'!$B$1".into()),
            categories: Some("'Data & Stuff'!$A$2:$A$3".into()),
            values: "'Data & Stuff'!$B$2:$B$3".into(),
            bubble_sizes: None,
            color: Some(Color::Rgb(0xFF0000)),
            secondary: false,
            kind: None,
        }],
        legend: LegendPos::Bottom,
        data_labels: true,
        gridlines: true,
        style: 2,
        x_title: Some("Region".into()),
        y_title: Some("Amount".into()),
        source: None,
        by_rows: false,
    };
    for kind in [
        ChartKind::ColumnClustered,
        ChartKind::ColumnStacked,
        ChartKind::ColumnStacked100,
        ChartKind::BarClustered,
        ChartKind::BarStacked,
        ChartKind::BarStacked100,
        ChartKind::Line,
        ChartKind::LineMarkers,
        ChartKind::LineStacked,
        ChartKind::Pie,
        ChartKind::Doughnut,
        ChartKind::Area,
        ChartKind::AreaStacked,
        ChartKind::Scatter,
        ChartKind::ScatterLines,
        ChartKind::Bubble,
        ChartKind::Radar,
    ] {
        s.charts.push(Chart { kind, ..base_chart.clone() });
    }
    let mut combo = Chart { kind: ChartKind::Combo, legend: LegendPos::Right, ..base_chart.clone() };
    combo.series.push(Series {
        name: Some("Literal".into()),
        kind: Some(ChartKind::Line),
        secondary: true,
        color: None,
        ..base_chart.series[0].clone()
    });
    s.charts.push(combo);
    s.charts.push(Chart { kind: ChartKind::Waterfall, legend: LegendPos::None, title: None, ..base_chart.clone() });

    wb.sheets = vec![Arc::new(s)];
    let mut s2 = Sheet::new("Hidden");
    s2.visibility = Visibility::Hidden;
    s2.set_value(at("A1"), Value::Number(5.0));
    wb.sheets.push(Arc::new(s2));
    let mut s3 = Sheet::new("2024");
    s3.visibility = Visibility::VeryHidden;
    wb.sheets.push(Arc::new(s3));
    wb.names.push(DefinedName { name: "Rate".into(), scope: None, formula: "0.07".into(), comment: "c".into(), hidden: false });
    wb.names.push(DefinedName { name: "Local".into(), scope: Some(1), formula: "Hidden!$A$1".into(), comment: String::new(), hidden: true });
    wb.names.push(DefinedName {
        name: "Pick".into(),
        scope: None,
        formula: "XLOOKUP(1,Hidden!A:A,Hidden!A:A)".into(),
        comment: String::new(),
        hidden: false,
    });
    wb.cell_styles.push(("Good".into(), red));
    wb
}

#[test]
fn full_roundtrip() {
    let wb = sample();
    let bytes = write_xlsx(&wb).unwrap();
    assert_eq!(write_xlsx(&wb).unwrap(), bytes, "output is deterministic");
    let (back, rep) = read_xlsx(&bytes).unwrap();
    assert!(rep.warnings.iter().all(|w| !w.contains("skipped") || w.contains("chart")), "{:?}", rep.warnings);

    assert_eq!(back.date_system, wb.date_system);
    assert_eq!(back.calc, wb.calc);
    assert!(back.protected_structure);
    assert_eq!(back.props, wb.props);
    assert_eq!(back.theme, wb.theme);
    assert_eq!(back.names, wb.names);
    assert_eq!(back.cell_styles.len(), 1);
    assert_eq!(back.sheets.len(), 3);
    assert_eq!(back.sheets[1].visibility, Visibility::Hidden);
    assert_eq!(back.sheets[2].visibility, Visibility::VeryHidden);
    assert_eq!(back.sheets[2].name, "2024");

    let (a, b) = (wb.sheet(0).unwrap(), back.sheet(0).unwrap());
    assert_eq!(b.name, a.name);
    // Cells: values, formulas and resolved styles.
    let skip = [at("H2"), at("H3")];
    for (c, cell) in a.cells.iter() {
        let got = b.cell(c).unwrap_or_else(|| panic!("missing {c}"));
        assert_eq!(got.value, cell.value, "value at {c}");
        assert_eq!(got.formula.as_ref().map(|f| (&f.text, f.array)), cell.formula.as_ref().map(|f| (&f.text, f.array)), "formula at {c}");
        assert_eq!(back.styles.get(got.style), wb.styles.get(cell.style), "style at {c}");
    }
    for (c, _) in b.cells.iter() {
        assert!(a.cell(c).is_some() || skip.contains(&c) || a.tables.iter().any(|t| t.range.contains(c)), "extra {c}");
    }
    // Spilled cells are written as cached values and dropped again on read.
    assert!(b.cell(at("H2")).is_none());
    assert_eq!(b.cols, a.cols);
    assert_eq!(b.rows, a.rows);
    assert_eq!(b.default_row_height, a.default_row_height);
    assert_eq!(b.default_col_width, a.default_col_width);
    assert_eq!(b.merges, a.merges);
    assert_eq!(b.freeze, a.freeze);
    assert_eq!(b.tab_color, a.tab_color);
    assert_eq!((b.show_gridlines, b.show_headings, b.show_zeros, b.right_to_left, b.zoom), (false, false, false, true, 125));
    assert_eq!(b.view_active, a.view_active);
    assert_eq!(b.cond_formats, a.cond_formats);
    assert_eq!(b.validations, a.validations);
    assert_eq!(b.hyperlinks, a.hyperlinks);
    assert_eq!(b.comments, a.comments);
    assert_eq!(b.autofilter, a.autofilter);
    assert_eq!(b.protection, a.protection);
    assert_eq!(b.print, a.print);
    let mut sp = a.sparklines.clone();
    sp[0].source = "'Data & Stuff'!B2:B3".into();
    assert_eq!(b.sparklines, sp);
    // Tables: name sanitized, ids reassigned.
    let (ta, tb) = (&a.tables[0], &b.tables[0]);
    assert_eq!(tb.name, "Sales_Table");
    assert_eq!((tb.range, tb.header_row, tb.totals_row, &tb.columns, &tb.style), (ta.range, ta.header_row, ta.totals_row, &ta.columns, &ta.style));
    assert_eq!((tb.banded_rows, tb.banded_cols, tb.first_col, tb.last_col, tb.filter_button), (true, true, true, false, true));
    assert_eq!(b.value(at("P1")), Value::text("Region"), "header cells carry the column names");
    // Drawings.
    assert_eq!(b.images.len(), 1);
    assert_eq!(b.images[0].data, a.images[0].data);
    assert_eq!(b.images[0].alt, a.images[0].alt);
    let close = |x: f32, y: f32| (x - y).abs() < 0.01;
    let (ia, ib) = (a.images[0].anchor, b.images[0].anchor);
    assert!(
        ia.cell == ib.cell && close(ia.dx, ib.dx) && close(ia.dy, ib.dy) && close(ia.width, ib.width) && close(ia.height, ib.height),
        "{ia:?} {ib:?}"
    );
    assert_eq!(b.shapes.len(), 3);
    for (x, y) in a.shapes.iter().zip(&b.shapes) {
        assert_eq!((x.kind, &x.text, x.fill, x.line), (y.kind, &y.text, y.fill, y.line));
    }
    assert_eq!(b.charts.len(), a.charts.len());
    for (x, y) in a.charts.iter().zip(&b.charts) {
        let expect_kind = if x.kind == ChartKind::Waterfall { ChartKind::ColumnClustered } else { x.kind };
        assert_eq!(y.kind, expect_kind, "chart kind");
        assert_eq!(y.title, x.title);
        assert_eq!(y.legend, x.legend);
        assert_eq!(y.data_labels, x.data_labels);
        if !matches!(x.kind, ChartKind::Pie | ChartKind::Doughnut) {
            assert_eq!(y.gridlines, x.gridlines);
            assert_eq!((&y.x_title, &y.y_title), (&x.x_title, &x.y_title));
        }
        assert_eq!(y.series.len(), x.series.len());
        for (sx, sy) in x.series.iter().zip(&y.series) {
            assert_eq!(sy.name, sx.name);
            assert_eq!(sy.categories, sx.categories);
            assert_eq!(sy.values, sx.values);
            assert_eq!(sy.color, sx.color);
            assert_eq!(sy.secondary, sx.secondary);
        }
        assert!(close(y.anchor.width, x.anchor.width) && close(y.anchor.height, x.anchor.height));
    }
    assert_eq!(back.sheet(1).unwrap().value(at("A1")), Value::Number(5.0));
}

#[test]
fn package_structure() {
    let bytes = write_xlsx(&sample()).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let names: Vec<String> = (0..z.len()).map(|i| z.name_for_index(i).unwrap().to_string()).collect();
    assert_eq!(names[0], "[Content_Types].xml");
    for n in [
        "_rels/.rels",
        "docProps/core.xml",
        "docProps/app.xml",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/styles.xml",
        "xl/theme/theme1.xml",
        "xl/sharedStrings.xml",
        "xl/metadata.xml",
        "xl/worksheets/sheet1.xml",
        "xl/worksheets/_rels/sheet1.xml.rels",
        "xl/tables/table1.xml",
        "xl/comments1.xml",
        "xl/drawings/vmlDrawing1.vml",
        "xl/drawings/drawing1.xml",
        "xl/drawings/_rels/drawing1.xml.rels",
        "xl/charts/chart1.xml",
        "xl/media/image1.png",
    ] {
        assert!(names.iter().any(|x| x == n), "missing {n}");
    }
    // Every part parses as XML; every override names an existing part; every internal
    // relationship target exists.
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for n in &names {
        let mut f = z.by_name(n).unwrap();
        let mut d = vec![];
        f.read_to_end(&mut d).unwrap();
        if n.ends_with(".xml") || n.ends_with(".rels") || n.ends_with(".vml") {
            let t = String::from_utf8(d).unwrap();
            crate::xml::parse(t.as_bytes()).unwrap_or_else(|e| panic!("{n}: {e}"));
            texts.insert(n.clone(), t);
        }
    }
    let ct = crate::xml::parse(texts["[Content_Types].xml"].as_bytes()).unwrap();
    for o in ct.kids("Override") {
        let p = o.attr("PartName").unwrap().trim_start_matches('/');
        assert!(names.iter().any(|x| x == p), "override for missing {p}");
    }
    for (n, t) in &texts {
        if !n.ends_with(".rels") {
            continue;
        }
        let src = n.replace("_rels/", "").trim_end_matches(".rels").to_string();
        let dir = crate::package::split_dir(&src).0;
        for r in crate::xml::parse(t.as_bytes()).unwrap().kids("Relationship") {
            if r.attr("TargetMode") == Some("External") {
                continue;
            }
            let target = crate::package::resolve(dir, r.attr("Target").unwrap());
            assert!(names.contains(&target), "{n}: dangling {target}");
        }
    }
    // Newer functions carry the _xlfn. prefix in the file.
    assert!(texts["xl/worksheets/sheet1.xml"].contains("_xlfn.XLOOKUP("));
    assert!(texts["xl/worksheets/sheet1.xml"].contains("cm=\"1\""));
    assert!(texts["xl/workbook.xml"].contains("_xlfn.XLOOKUP("));
    assert!(texts["xl/workbook.xml"].contains("fullCalcOnLoad"));
}

#[test]
fn empty_workbook_roundtrip() {
    let wb = Workbook::new();
    let bytes = write_xlsx(&wb).unwrap();
    let (back, rep) = read_xlsx(&bytes).unwrap();
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
    assert_eq!(back.sheets.len(), 1);
    assert_eq!(back.sheets[0].name, "Sheet1");
    assert_eq!(back.styles.len(), 1);
    assert_eq!(back.sheets[0].default_col_width, 64.0);
    assert_eq!(back.sheets[0].default_row_height, 20.0);
    assert!(crate::sniff(&bytes) == crate::Format::Xlsx);
}

#[test]
fn writing_repairs_invalid_models() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.visibility = Visibility::Hidden;
    // A table whose header cells disagree with its columns, with duplicate column names.
    s.set_value(at("A1"), Value::Number(1.0));
    s.tables.push(Table {
        id: 1,
        name: "1bad name".into(),
        range: rr("A1:C3"),
        header_row: true,
        totals_row: false,
        columns: vec![
            TableColumn { name: "X".into(), totals: TotalsFn::None, totals_label: None, formula: None },
            TableColumn { name: "x".into(), totals: TotalsFn::None, totals_label: None, formula: None },
        ],
        style: String::new(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
    });
    s.cells.set(at("B2"), Cell { value: Value::Number(1.0), formula: None, style: StyleId(9999) });
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let s = back.sheet(0).unwrap();
    assert_eq!(s.visibility, Visibility::Visible, "at least one sheet stays visible");
    let names: Vec<&str> = s.tables[0].columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["X", "x2", "Column3"]);
    assert_eq!(s.value(at("A1")), Value::text("X"));
    assert_eq!(s.tables[0].name, "Table1");
    assert_eq!(s.cell(at("B2")).unwrap().style, StyleId::DEFAULT);
}

fn sheet1_xml(bytes: &[u8]) -> (String, bool) {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut s).unwrap();
    let metadata = z.by_name("xl/metadata.xml").is_ok();
    (s, metadata)
}

/// Excel refuses to open a file whose cached values include `#SPILL!` or `#CALC!`.
#[test]
fn spill_and_calc_errors_are_cached_as_value_errors() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.set_value(at("A1"), Value::Number(1.0));
    s.cells.set(at("C1"), Cell { value: Value::Error(CellError::Spill), ..Cell::formula(Formula::new("A1:A3*2")) });
    s.cells.set(at("C2"), Cell { value: Value::Error(CellError::Calc), ..Cell::formula(Formula::new("FILTER(A1:A3,A1:A3>5)")) });
    s.cells.set(at("C3"), Cell { value: Value::Error(CellError::NA), ..Cell::formula(Formula::new("NA()")) });
    let (xml, _) = sheet1_xml(&write_xlsx(&wb).unwrap());
    assert!(!xml.contains("#SPILL!") && !xml.contains("#CALC!"), "{xml}");
    assert!(xml.contains(r#"<f>A1:A3*2</f><v>#VALUE!</v></c>"#), "{xml}");
    assert!(xml.contains(r#"<f>_xlfn._xlws.FILTER(A1:A3,A1:A3&gt;5)</f><v>#VALUE!</v></c>"#), "{xml}");
    assert!(xml.contains(r#"<c r="C3" t="e"><f>NA()</f><v>#N/A</v></c>"#), "{xml}");
}
