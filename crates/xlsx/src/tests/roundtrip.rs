//! Build a workbook using every feature, write it, read it back, compare.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};
use gridcraft_model::*;

use crate::package::Package;
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
    wb.structure_password = Some(gridcraft_model::PasswordHash::new_sha512("s3cret", b"0123456789abcdef"));
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
    s.view = gridcraft_model::SheetViewMode::PageLayout;
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
    s.edit_ranges = vec![
        EditRange { title: "Open & <free>".into(), ranges: vec![rr("A1:B2"), rr("D4")], password: None, security_descriptor: None },
        EditRange {
            title: "Old".into(),
            ranges: vec![rr("C1:C9")],
            password: Some(PasswordHash::Legacy(0x83AF)),
            security_descriptor: Some("O:WDG:WDD:(A;;CC;;;WD)".into()),
        },
        EditRange {
            title: "New".into(),
            ranges: vec![rr("E1")],
            password: Some(PasswordHash::new_sha512("pw", b"0123456789abcdef")),
            security_descriptor: None,
        },
    ];
    s.hyperlinks.insert(at("A2"), Hyperlink { target: "https://example.com/?a=1&b=2".into(), tooltip: Some("go".into()), external: true });
    s.hyperlinks.insert(at("A3"), Hyperlink { target: "'Data & Stuff'!B2".into(), tooltip: None, external: false });
    s.hyperlinks.insert(at("A4"), Hyperlink { target: "mailto:me@example.com".into(), tooltip: None, external: true });
    s.comments.insert(
        at("B2"),
        Comment {
            author: "Ann".into(),
            text: "Note & <stuff>\nline 2".into(),
            replies: vec![],
            threaded: false,
            resolved: false,
            visible: false,
            dates: vec![],
            note_box: None,
        },
    );
    s.comments.insert(
        at("C3"),
        Comment {
            author: "Bob".into(),
            text: "second".into(),
            replies: vec![],
            threaded: false,
            resolved: false,
            visible: true,
            dates: vec![],
            note_box: None,
        },
    );
    s.comments.insert(
        at("D4"),
        Comment {
            author: "Ann & Co".into(),
            text: "Threaded <one>\nsecond line".into(),
            replies: vec![("Bob".into(), "first reply".into()), ("Ann & Co".into(), "second reply".into())],
            threaded: true,
            resolved: true,
            visible: false,
            dates: vec!["2026-10-11T09:30:00.00".into(), String::new(), "2026-10-11T10:15:00.00".into()],
            note_box: None,
        },
    );
    s.comments.insert(
        at("E5"),
        Comment {
            author: "Cy".into(),
            text: "open thread".into(),
            replies: vec![],
            threaded: true,
            resolved: false,
            visible: false,
            dates: vec![],
            note_box: None,
        },
    );
    s.autofilter = Some(AutoFilter {
        range: rr("K1:N20"),
        criteria: vec![
            (0, FilterCriterion::Values { values: vec!["a".into(), "b".into()], blanks: true }),
            (1, FilterCriterion::Custom { a: (">=".into(), "5".into()), b: Some(("<".into(), "9".into())), and: true }),
            (2, FilterCriterion::Top10 { bottom: true, percent: false, count: 3 }),
            (3, FilterCriterion::FillColor(Color::Rgb(0xFF0000))),
        ],
    });
    s.protection = Some(SheetProtection {
        format_cells: true,
        sort: true,
        select_locked: true,
        select_unlocked: true,
        password: Some(gridcraft_model::PasswordHash::new_sha512("sheet pw", b"fedcba9876543210")),
        ..Default::default()
    });
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
        different_first: true,
        different_odd_even: true,
        first_header: "&CCover".into(),
        first_footer: String::new(),
        even_header: "&LEven &P".into(),
        even_footer: "&R&A".into(),
        hf_scale_with_doc: false,
        hf_align_with_margins: true,
    };
    s.sparklines.push(Sparkline { cell: at("J2"), source: "B2:B3".into(), kind: SparklineKind::WinLoss, color: Color::Rgb(0x112233), markers: true });
    s.tables.push(Table {
        id: 1,
        name: "Sales Table".into(),
        range: rr("P1:R5"),
        header_row: true,
        totals_row: true,
        columns: vec![
            TableColumn {
                name: "Region".into(),
                totals: TotalsFn::None,
                totals_label: Some("Total".into()),
                formula: None,
                unique_name: None,
                xml: None,
            },
            TableColumn { name: "Amount".into(), totals: TotalsFn::Sum, totals_label: None, formula: None, unique_name: None, xml: None },
            TableColumn {
                name: "Tax".into(),
                totals: TotalsFn::Custom,
                totals_label: Some("SUM(R2:R4)".into()),
                formula: Some("Q2*0.1".into()),
                unique_name: None,
                xml: None,
            },
        ],
        style: "TableStyleLight9".into(),
        banded_rows: true,
        banded_cols: true,
        first_col: true,
        last_col: false,
        filter_button: true,
        filter: vec![
            (0, FilterCriterion::Values { values: vec!["East".into()], blanks: false }),
            (1, FilterCriterion::Icon { set: "3TrafficLights1".into(), id: Some(1) }),
        ],
        table_type: None,
        connection_id: None,
    });
    let png = b"\x89PNG\r\n\x1a\n0000IHDRfake".to_vec();
    s.images.push(Image {
        id: 10,
        // A non-default anchor mode, to prove it survives an xlsx round-trip.
        anchor: Anchor { cell: at("C10"), dx: 5.0, dy: 4.0, width: 120.0, height: 80.0, mode: AnchorMode::MoveOnly },
        data: png.into(),
        mime: "image/png".into(),
        alt: "Logo \"x\"".into(),
    });
    s.shapes.push(Shape {
        id: 11,
        kind: ShapeKind::Ellipse,
        anchor: Anchor { cell: at("E10"), dx: 0.0, dy: 0.0, width: 64.0, height: 40.0, mode: AnchorMode::Absolute },
        fill: Color::Rgb(0x4472C4),
        line: Color::Rgb(0x000000),
        text: "Hi\nthere".into(),
        format: Default::default(),
    });
    s.shapes.push(Shape {
        id: 12,
        kind: ShapeKind::Arrow,
        anchor: Anchor { cell: at("E14"), dx: 0.0, dy: 0.0, width: 64.0, height: 0.0, ..Default::default() },
        fill: Color::Auto,
        line: Color::Rgb(0xFF0000),
        text: String::new(),
        format: Default::default(),
    });
    s.shapes.push(Shape {
        id: 13,
        kind: ShapeKind::TextBox,
        anchor: Anchor { cell: at("E16"), dx: 0.0, dy: 0.0, width: 64.0, height: 40.0, ..Default::default() },
        fill: Color::Auto,
        line: Color::Auto,
        text: "box".into(),
        format: ShapeFormat {
            no_line: true,
            font_size: Some(20.0),
            bold: true,
            italic: true,
            font: Some("Arial".into()),
            text_color: Some(Color::Rgb(0x112233)),
            text_outline: Some((Color::Rgb(0xFF0000), 1.5)),
            align: Some(ShapeTextAlign::Right),
            anchor: Some(ShapeTextAnchor::Bottom),
            rtl: true,
        },
    });
    let base_chart = Chart {
        id: 20,
        kind: ChartKind::ColumnClustered,
        anchor: Anchor { cell: at("J5"), dx: 0.0, dy: 0.0, width: 480.0, height: 288.0, ..Default::default() },
        title: Some("Sales".into()),
        series: vec![Series {
            name: Some("'Data & Stuff'!$B$1".into()),
            categories: Some("'Data & Stuff'!$A$2:$A$3".into()),
            values: "'Data & Stuff'!$B$2:$B$3".into(),
            bubble_sizes: None,
            color: Some(Color::Rgb(0xFF0000)),
            secondary: false,
            kind: None,
            quartiles: Default::default(),
            smooth: false,
            trendlines: Vec::new(),
        }],
        legend: LegendPos::Bottom,
        data_labels: true,
        gridlines: true,
        style: 2,
        x_title: Some("Region".into()),
        y_title: Some("Amount".into()),
        source: None,
        by_rows: false,
        pivot: None,
        cat_reverse: false,
        val_reverse: false,
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
        smooth: false,
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
    s3.comments.insert(
        at("A1"),
        Comment {
            author: "Bob".into(),
            text: "other sheet".into(),
            replies: vec![],
            threaded: true,
            resolved: false,
            visible: false,
            dates: vec![],
            note_box: None,
        },
    );
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
    assert_eq!(back.structure_password, wb.structure_password);
    assert!(back.structure_password.as_ref().unwrap().verify("s3cret").unwrap());
    assert_eq!(back.props, wb.props);
    assert_eq!(back.theme, wb.theme);
    assert_eq!(back.names, wb.names);
    assert_eq!(back.cell_styles.len(), 1);
    assert_eq!(back.sheets.len(), 3);
    assert_eq!(back.sheets[1].visibility, Visibility::Hidden);
    assert_eq!(back.sheets[2].visibility, Visibility::VeryHidden);
    assert_eq!(back.sheets[2].name, "2024");
    assert_eq!(back.sheets[2].comments, wb.sheets[2].comments);

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
    assert_eq!(b.view, gridcraft_model::SheetViewMode::PageLayout, "sheetView view=\"pageLayout\" round-trips");
    assert_eq!(b.cond_formats, a.cond_formats);
    assert_eq!(b.validations, a.validations);
    assert_eq!(b.hyperlinks, a.hyperlinks);
    assert_eq!(b.comments, a.comments);
    assert_eq!(b.autofilter, a.autofilter);
    assert_eq!(b.protection, a.protection);
    assert_eq!(b.edit_ranges, a.edit_ranges);
    assert_eq!(b.print, a.print);
    let mut sp = a.sparklines.clone();
    sp[0].source = "'Data & Stuff'!B2:B3".into();
    assert_eq!(b.sparklines, sp);
    // Tables: name sanitized, ids reassigned.
    let (ta, tb) = (&a.tables[0], &b.tables[0]);
    assert_eq!(tb.name, "Sales_Table");
    assert_eq!((tb.range, tb.header_row, tb.totals_row, &tb.columns, &tb.style), (ta.range, ta.header_row, ta.totals_row, &ta.columns, &ta.style));
    assert_eq!((tb.banded_rows, tb.banded_cols, tb.first_col, tb.last_col, tb.filter_button), (true, true, true, false, true));
    assert_eq!(tb.filter, ta.filter, "the table's filter criteria survive (SUBTOTAL tells filtered rows apart)");
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
    // The anchor mode (Excel's editAs) survives the round-trip.
    assert_eq!(ib.mode, AnchorMode::MoveOnly, "image anchor mode");
    assert_eq!(b.shapes[0].anchor.mode, AnchorMode::Absolute, "shape anchor mode");
    assert_eq!(b.shapes[1].anchor.mode, AnchorMode::MoveAndSize, "default anchor mode");
    assert_eq!(b.shapes.len(), 3);
    for (x, y) in a.shapes.iter().zip(&b.shapes) {
        assert_eq!((x.kind, &x.text, x.fill, x.line, &x.format), (y.kind, &y.text, y.fill, y.line, &y.format));
    }
    assert_eq!(b.charts.len(), a.charts.len());
    for (x, y) in a.charts.iter().zip(&b.charts) {
        assert_eq!(y.kind, x.kind, "chart kind");
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
fn iterative_calculation_settings_roundtrip() {
    let calc_pr = |wb: &Workbook| {
        let bytes = write_xlsx(wb).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let book = pkg.read_xml("xl/workbook.xml").unwrap().unwrap();
        let c = book.child("calcPr").unwrap();
        let attrs = ["calcMode", "iterate", "iterateCount", "iterateDelta", "fullPrecision"].map(|a| c.attr(a).map(str::to_string));
        (attrs, read_xlsx(&bytes).unwrap().0.calc)
    };
    let s = |v: &str| Some(v.to_string());
    // Defaults: nothing written.
    let mut wb = Workbook::new();
    assert_eq!(calc_pr(&wb), ([None, None, None, None, None], CalcSettings::default()));
    // On: the switch and both limits, beside the other calculation settings.
    wb.calc.mode = CalcMode::Manual;
    wb.calc.iterative = true;
    wb.calc.precision_as_displayed = true;
    let (attrs, back) = calc_pr(&wb);
    assert_eq!(attrs, [s("manual"), s("1"), s("100"), s("0.001"), s("0")]);
    assert_eq!(back, wb.calc);
    // Off: limits that differ from the defaults are kept for when it is switched on again.
    wb.calc.iterative = false;
    wb.calc.set_max_iterations(250);
    wb.calc.set_max_change(0.0005);
    let (attrs, back) = calc_pr(&wb);
    assert_eq!(attrs, [s("manual"), None, s("250"), s("0.0005"), s("0")]);
    assert_eq!(back, wb.calc);
    // Out-of-range values in a file are kept within Excel's limits.
    let xml = |attrs: &str| {
        format!(
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets><calcPr {attrs}/></workbook>"#
        )
    };
    for (attrs, count, change) in [
        (r#"iterate="1" iterateCount="0" iterateDelta="-2""#, 1, 2.0),
        (r#"iterate="true" iterateCount="99999" iterateDelta="NaN""#, CalcSettings::MAX_ITERATIONS, CalcSettings::DEFAULT_CHANGE),
    ] {
        let wbx = xml(attrs);
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let ws = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#;
        let bytes = super::make_zip(&[
            ("[Content_Types].xml", super::CT.as_bytes()),
            ("_rels/.rels", super::ROOT_RELS.as_bytes()),
            ("xl/workbook.xml", wbx.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/worksheets/sheet1.xml", ws.as_bytes()),
        ]);
        let calc = read_xlsx(&bytes).unwrap().0.calc;
        assert!(calc.iterative, "{attrs}");
        assert_eq!((calc.max_iterations, calc.max_change), (count, change), "{attrs}");
    }
}

#[test]
fn show_objects_roundtrip() {
    let saved = |wb: &Workbook| {
        let bytes = write_xlsx(wb).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let book = pkg.read_xml("xl/workbook.xml").unwrap().unwrap();
        let attr = book.child("workbookPr").and_then(|p| p.attr("showObjects")).map(str::to_string);
        (attr, read_xlsx(&bytes).unwrap().0.show_objects)
    };
    // Default: nothing written, All read back.
    let mut wb = Workbook::new();
    assert_eq!(saved(&wb), (None, ShowObjects::All));
    // Beside date1904, which shares the element.
    wb.date_system = DateSystem::D1904;
    for (show, attr) in [(ShowObjects::None, "none"), (ShowObjects::Placeholders, "placeholders")] {
        wb.show_objects = show;
        assert_eq!(saved(&wb), (Some(attr.to_string()), show));
        assert_eq!(read_xlsx(&write_xlsx(&wb).unwrap()).unwrap().0.date_system, DateSystem::D1904);
    }
    // A file written elsewhere: the attribute among the others Excel writes, or absent.
    let book = |pr: &str| {
        let wbx = format!(
            r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">{pr}<sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets></workbook>"#
        );
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#;
        let ws = r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData/></worksheet>"#;
        let bytes = super::make_zip(&[
            ("[Content_Types].xml", super::CT.as_bytes()),
            ("_rels/.rels", super::ROOT_RELS.as_bytes()),
            ("xl/workbook.xml", wbx.as_bytes()),
            ("xl/_rels/workbook.xml.rels", rels.as_bytes()),
            ("xl/worksheets/sheet1.xml", ws.as_bytes()),
        ]);
        read_xlsx(&bytes).unwrap().0
    };
    let hidden = book(r#"<workbookPr showObjects="none" defaultThemeVersion="166925"/>"#);
    assert_eq!(hidden.show_objects, ShowObjects::None);
    assert_eq!(saved(&hidden).0.as_deref(), Some("none"));
    for pr in [r#"<workbookPr defaultThemeVersion="166925"/>"#, "", r#"<workbookPr showObjects="bogus"/>"#] {
        let wb = book(pr);
        assert_eq!(wb.show_objects, ShowObjects::All, "{pr}");
        assert_eq!(saved(&wb).0, None, "{pr}: a file without the attribute stays without it");
    }
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
fn cell_text_carriage_returns_survive_xlsx_roundtrip() {
    let mut wb = Workbook::new();
    let values = [("A1", "Line 1\r\nLine 2"), ("A2", "Part A\rPart B"), ("A3", "Item 3\nItem 4"), ("A4", "literal _x000D_ marker")];
    let sheet = wb.sheet_mut(0).unwrap();
    for (address, text) in values {
        sheet.set_value(at(address), Value::text(text));
    }
    let mut formula = Cell::formula(Formula::new("A1"));
    formula.value = Value::text("Cached\r\ntext");
    sheet.cells.set(at("B1"), formula);
    sheet.comments.insert(
        at("A1"),
        Comment {
            author: "Tester".into(),
            text: "Note\r\ntext".into(),
            replies: vec![],
            threaded: false,
            resolved: false,
            visible: false,
            dates: vec![],
            note_box: None,
        },
    );

    let bytes = write_xlsx(&wb).unwrap();
    let mut package = Package::open(&bytes).unwrap();
    let shared_strings = String::from_utf8(package.read("xl/sharedStrings.xml").unwrap().unwrap()).unwrap();
    assert!(shared_strings.contains("Line 1_x000D_\nLine 2"), "CR must be encoded before XML parsing");
    assert!(shared_strings.contains("Part A_x000D_Part B"));
    assert!(!shared_strings.contains('\r'), "raw CR is normalized to LF by XML parsers");
    let sheet_xml = String::from_utf8(package.read("xl/worksheets/sheet1.xml").unwrap().unwrap()).unwrap();
    assert!(sheet_xml.contains("Cached_x000D_\ntext"), "formula caches also use ST_Xstring");

    let (back, _) = read_xlsx(&bytes).unwrap();
    for (address, text) in values {
        assert_eq!(back.sheets[0].value(at(address)), Value::text(text), "{address} changed on save/reopen");
    }
    assert_eq!(back.sheets[0].value(at("B1")), Value::text("Cached\r\ntext"));
    assert_eq!(back.sheets[0].comments.get(&at("A1")).map(|c| c.text.as_str()), Some("Note\r\ntext"));
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
            TableColumn { name: "X".into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None },
            TableColumn { name: "x".into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None },
        ],
        style: String::new(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
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

#[test]
fn vba_project_roundtrip() {
    // A VBA project blob is preserved byte-identically through write and read, and the package
    // carries the macro-enabled content type plus the vbaProject relationship.
    let vba = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1 fake OLE2 vbaProject".to_vec();
    let mut wb = Workbook::new();
    wb.vba_project = Some(vba.clone().into());
    let bytes = write_xlsx(&wb).unwrap();
    assert_eq!(write_xlsx(&wb).unwrap(), bytes, "output is deterministic");

    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
    let names: Vec<String> = (0..z.len()).map(|i| z.name_for_index(i).unwrap().to_string()).collect();
    assert!(names.iter().any(|x| x == "xl/vbaProject.bin"), "missing vbaProject.bin");
    let mut bin = vec![];
    z.by_name("xl/vbaProject.bin").unwrap().read_to_end(&mut bin).unwrap();
    assert_eq!(bin, vba, "vbaProject.bin written byte-identically");
    let mut ct = String::new();
    z.by_name("[Content_Types].xml").unwrap().read_to_string(&mut ct).unwrap();
    assert!(ct.contains("macroEnabled.main+xml"), "macro-enabled main content type: {ct}");
    assert!(ct.contains("/xl/vbaProject.bin"), "vbaProject content type override: {ct}");
    let mut rels = String::new();
    z.by_name("xl/_rels/workbook.xml.rels").unwrap().read_to_string(&mut rels).unwrap();
    assert!(rels.contains("relationships/vbaProject"), "vbaProject relationship: {rels}");

    let (back, rep) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.vba_project.as_deref(), Some(&vba), "vbaProject survives the read");
    assert!(rep.warnings.iter().any(|w| w.contains("not executed")), "{:?}", rep.warnings);

    // A workbook without macros stays macro-free and free of VBA warnings.
    let (plain, rp) = read_xlsx(&write_xlsx(&Workbook::new()).unwrap()).unwrap();
    assert_eq!(plain.vba_project, None);
    assert!(rp.warnings.iter().all(|w| !w.contains("VBA")), "{:?}", rp.warnings);
}

#[test]
fn vba_project_read_from_foreign_file() {
    // An .xlsm built by another producer is captured on read (not dropped with a plain warning).
    let vba: &[u8] = b"PK\x03\x04-not-really-but-opaque-bytes";
    let bytes = super::minimal(
        "<sheetData/>",
        &[("xl/vbaProject.bin", "ignored-as-text")],
        r#"<Relationship Id="rId9" Type="http://schemas.microsoft.com/office/2006/relationships/vbaProject" Target="vbaProject.bin"/>"#,
        "",
    );
    let (wb, rep) = read_xlsx(&bytes).unwrap();
    assert!(wb.vba_project.is_some(), "VBA blob captured");
    assert!(rep.warnings.iter().any(|w| w.contains("not executed")), "{:?}", rep.warnings);
    let _ = vba;
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
    let bytes = write_xlsx(&wb).unwrap();
    let (xml, _) = sheet1_xml(&bytes);
    assert!(!xml.contains("#SPILL!") && !xml.contains("#CALC!"), "{xml}");
    let mut pkg = Package::open(&bytes).unwrap();
    let sheet = pkg.read_xml("xl/worksheets/sheet1.xml").unwrap().unwrap();
    for (address, text, value) in [("C1", "A1:A3*2", "#VALUE!"), ("C2", "_xlfn._xlws.FILTER(A1:A3,A1:A3>5)", "#VALUE!"), ("C3", "NA()", "#N/A")] {
        let cell = sheet.child("sheetData").unwrap().kids("row").flat_map(|row| row.kids("c")).find(|cell| cell.attr("r") == Some(address)).unwrap();
        assert_eq!(cell.attr("t"), Some("e"));
        assert_eq!(cell.child("f").unwrap().text, text);
        assert_eq!(cell.child("v").unwrap().text, value);
    }
}

/// `CT_IconSet` needs one `cfvo` per icon (at least two); a rule without stored thresholds
/// gets Excel's defaults, one without the lower bound gets it prepended (#235).
#[test]
fn icon_sets_always_write_one_cfvo_per_icon() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    for (i, (set, thresholds)) in [
        ("3Arrows", vec![]),
        ("4Rating", vec![CfValueKind::Number(2.0), CfValueKind::Number(5.0), CfValueKind::Number(8.0)]),
        ("9Bogus", vec![CfValueKind::Percent(50.0)]),
    ]
    .into_iter()
    .enumerate()
    {
        s.cond_formats.push(CondFormat {
            ranges: vec![rr(&format!("A{}", i + 1))],
            rule: CfRule::IconSet { set: set.into(), thresholds, reverse: false, show_value: true },
            priority: i as u32 + 1,
            stop_if_true: false,
        });
    }
    let bytes = write_xlsx(&wb).unwrap();
    let (xml, _) = sheet1_xml(&bytes);
    assert!(
        xml.contains(
            "<iconSet iconSet=\"3Arrows\"><cfvo type=\"percent\" val=\"0\"/><cfvo type=\"percent\" val=\"33\"/><cfvo type=\"percent\" val=\"67\"/></iconSet>"
        ),
        "{xml}"
    );
    assert!(
        xml.contains("<iconSet iconSet=\"4Rating\"><cfvo type=\"percent\" val=\"0\"/><cfvo type=\"num\" val=\"2\"/><cfvo type=\"num\" val=\"5\"/><cfvo type=\"num\" val=\"8\"/></iconSet>"),
        "{xml}"
    );
    assert!(xml.contains("<iconSet iconSet=\"9Bogus\"><cfvo type=\"percent\" val=\"0\"/>"), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let rules: Vec<usize> = back
        .sheet(0)
        .unwrap()
        .cond_formats
        .iter()
        .map(|cf| match &cf.rule {
            CfRule::IconSet { thresholds, .. } => thresholds.len(),
            _ => 0,
        })
        .collect();
    assert_eq!(rules, [3, 4, 5]);
}

/// Files have no `@` operator: Excel writes `@x` as `_xlfn.SINGLE(x)` in a dynamic-array
/// formula, and reads it back as `@x`.
#[test]
fn implicit_intersection_is_written_as_single() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    for (r, n) in [(0, 1.0), (1, 3.0), (2, 5.0)] {
        s.set_value(CellRef::new(r, 0), Value::Number(n));
    }
    s.cells.set(at("D3"), Cell { value: Value::Number(10.0), ..Cell::formula(Formula::new("@A1:A3*2")) });
    s.cells.set(at("D4"), Cell { value: Value::Number(2.0), ..Cell::formula(Formula::new("A1*2")) });
    let bytes = write_xlsx(&wb).unwrap();
    let (xml, metadata) = sheet1_xml(&bytes);
    assert!(xml.contains("<c r=\"D3\" cm=\"1\"><f t=\"array\" ref=\"D3\">_xlfn.SINGLE(A1:A3)*2</f><v>10</v>"), "{xml}");
    assert!(xml.contains("<c r=\"D4\"><f>A1*2</f>"), "{xml}");
    assert!(!xml.contains('@'), "{xml}");
    assert!(metadata);
    let (back, _) = read_xlsx(&bytes).unwrap();
    let sh = back.sheet(0).unwrap();
    assert_eq!(sh.cell(at("D3")).and_then(|c| c.formula.as_ref()).map(|f| f.text.as_str()), Some("@A1:A3*2"));
    assert_eq!(sh.cell(at("D3")).and_then(|c| c.formula.as_ref()).and_then(|f| f.array), None);
}

#[test]
fn threaded_comments_write_persons_and_linked_legacy_notes() {
    let wb = sample();
    let bytes = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&bytes).unwrap();
    let text = |pkg: &mut Package, part: &str| String::from_utf8(pkg.read(part).unwrap().unwrap()).unwrap();
    // One person per author across the workbook, ids unique.
    let persons = text(&mut pkg, "xl/persons/person.xml");
    assert_eq!(persons.matches("<person ").count(), 3, "{persons}");
    assert!(persons.contains("displayName=\"Ann &amp; Co\""));
    let ct = text(&mut pkg, "[Content_Types].xml");
    assert!(ct.contains("application/vnd.ms-excel.threadedcomments+xml") && ct.contains("application/vnd.ms-excel.person+xml"));
    let tc = text(&mut pkg, "xl/threadedComments/threadedComment1.xml");
    assert_eq!(tc.matches("<threadedComment ").count(), 4, "{tc}");
    assert_eq!(tc.matches("parentId=").count(), 2);
    assert_eq!(tc.matches("done=\"1\"").count(), 1);
    // The legacy note of each thread points at it.
    let legacy = text(&mut pkg, "xl/comments1.xml");
    for id in tc.split("id=\"{").skip(1).filter_map(|s| s.split('}').next()).take(1) {
        assert!(legacy.contains(&format!("<author>tc={{{id}}}</author>")) && legacy.contains(&format!("xr:uid=\"{{{id}}}\"")), "{legacy}");
    }
}

#[test]
fn bad_comment_dates_are_left_out() {
    let mut wb = Workbook::new();
    let s = Arc::make_mut(&mut wb.sheets[0]);
    let c = Comment {
        author: "A".into(),
        text: "t".into(),
        replies: vec![],
        threaded: true,
        resolved: false,
        visible: false,
        dates: vec!["\"/><evil".into()],
        note_box: None,
    };
    s.comments.insert(at("A1"), c);
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let c = &back.sheets[0].comments[&at("A1")];
    assert!(c.threaded && c.dates.is_empty());
}

/// #378: the saved scroll position survives a round trip, with and without frozen panes (where it
/// is the scrolling pane's `topLeftCell`).
#[test]
fn scroll_position_round_trips_with_and_without_frozen_panes() {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().view_top_left = at("A31");
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheet(0).unwrap().view_top_left, at("A31"));

    let s = wb.sheet_mut(0).unwrap();
    s.freeze = Some((1, 0));
    s.view_top_left = at("C40");
    let bytes = write_xlsx(&wb).unwrap();
    let (xml, _) = sheet1_xml(&bytes);
    assert!(xml.contains(r#"<pane ySplit="1" topLeftCell="C40""#), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    assert_eq!(back.sheet(0).unwrap().view_top_left, at("C40"));
    assert_eq!(back.sheet(0).unwrap().freeze, Some((1, 0)));

    // A scroll position inside the frozen rows is written as the first scrolling row.
    wb.sheet_mut(0).unwrap().view_top_left = at("A1");
    let (xml, _) = sheet1_xml(&write_xlsx(&wb).unwrap());
    assert!(xml.contains(r#"topLeftCell="A2""#), "{xml}");
}

/// Excel stores LET/LAMBDA parameters as `_xlpm.name`; the model shows bare names.
#[test]
fn let_lambda_parameters_drop_xlpm_on_read() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.cells.set(at("A1"), Cell { value: Value::Number(2.0), ..Cell::formula(Formula::new("LET(x,1,x+1)")) });
    s.cells.set(at("A2"), Cell { value: Value::Number(6.0), ..Cell::formula(Formula::new("LAMBDA(a,a*2)(3)")) });
    let mut arr = Formula::new("MAP(B1:B2,LAMBDA(v,v+1))");
    arr.array = Some(rr("C1:C2"));
    s.cells.set(at("C1"), Cell::formula(arr));
    let bytes = write_xlsx(&wb).unwrap();
    let (xml, _) = sheet1_xml(&bytes);
    assert!(xml.contains("_xlfn.LET(_xlpm.x,1,_xlpm.x+1)"), "{xml}");
    assert!(xml.contains("_xlpm.v"), "{xml}");
    let (back, _) = read_xlsx(&bytes).unwrap();
    let sh = back.sheet(0).unwrap();
    let text = |a: &str| sh.cell(at(a)).and_then(|c| c.formula.as_ref()).map(|f| f.text.clone());
    assert_eq!(text("A1").as_deref(), Some("LET(x,1,x+1)"));
    assert_eq!(text("A2").as_deref(), Some("LAMBDA(a,a*2)(3)"));
    assert_eq!(text("C1").as_deref(), Some("MAP(B1:B2,LAMBDA(v,v+1))"));
}

/// A line break in a table column name (and its header cell) survives; a
/// leading backslash in a table name is legal and kept.
#[test]
fn table_newline_column_and_backslash_name_survive() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.set_value(at("A1"), Value::text("Handle\nBars"));
    s.set_value(at("B1"), Value::text("Name"));
    s.set_value(at("B2"), Value::text("x"));
    let col = |n: &str| TableColumn { name: n.into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None };
    s.tables.push(Table {
        id: 1,
        name: "\\_Prime.1".into(),
        range: rr("A1:B2"),
        header_row: true,
        totals_row: false,
        columns: vec![col("Handle\nBars"), col("Name")],
        style: String::new(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    s.cells.set(at("D1"), Cell::formula(Formula::new("COUNTA(\\_Prime.1[Name])")));
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    let s = back.sheet(0).unwrap();
    assert_eq!(s.tables[0].name, "\\_Prime.1");
    assert_eq!(s.tables[0].columns[0].name, "Handle\nBars");
    assert_eq!(s.value(at("A1")), Value::text("Handle\nBars"));
    assert_eq!(s.cell(at("D1")).and_then(|c| c.formula.as_ref()).map(|f| f.text.as_str()), Some("COUNTA(\\_Prime.1[Name])"));
    assert!(gridcraft_formula::parse("COUNTA(\\_Prime.1[Name])").is_ok(), "backslash table name parses as a structured reference");
}

/// A table that is only a header row and a totals row (Insert Table on one row, then Total
/// Row) is written like the header-only and one-data-row shapes, not dropped on save.
#[test]
fn header_and_totals_table_without_data_row_survives() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.tables.push(Table {
        id: 1,
        name: "Table1".into(),
        range: rr("A1:B2"),
        header_row: true,
        totals_row: true,
        columns: vec![
            TableColumn {
                name: "Name".into(),
                totals: TotalsFn::None,
                totals_label: Some("Total".into()),
                formula: None,
                unique_name: None,
                xml: None,
            },
            TableColumn { name: "Amount".into(), totals: TotalsFn::Sum, totals_label: None, formula: None, unique_name: None, xml: None },
        ],
        style: "TableStyleMedium2".into(),
        banded_rows: true,
        banded_cols: false,
        first_col: true,
        last_col: true,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    let (back, report) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let tables = &back.sheet(0).unwrap().tables;
    assert_eq!(tables.len(), 1);
    let t = &tables[0];
    assert_eq!((t.name.as_str(), t.range.a1(), t.header_row, t.totals_row), ("Table1", "A1:B2".to_string(), true, true));
    assert_eq!((t.style.as_str(), t.banded_rows, t.first_col, t.last_col), ("TableStyleMedium2", true, true, true));
    assert_eq!(t.columns[0].totals_label.as_deref(), Some("Total"));
    assert_eq!(t.columns[1].totals, TotalsFn::Sum);
    assert_eq!(back.sheet(0).unwrap().value(at("A1")), Value::text("Name"));
}

/// Templates and macro-enabled files differ from `.xlsx` only in the workbook part's content
/// type; a template saved with the workbook type is rejected by Excel.
#[test]
fn package_kind_sets_workbook_content_type() {
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().set_value(at("A1"), Value::Number(7.0));
    for (path, kind, ct) in [
        ("a.xlsx", crate::XlsxKind::Workbook, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"),
        ("a.XLTX", crate::XlsxKind::Template, "application/vnd.openxmlformats-officedocument.spreadsheetml.template.main+xml"),
        ("a.xlsm", crate::XlsxKind::MacroWorkbook, "application/vnd.ms-excel.sheet.macroEnabled.main+xml"),
        ("dir.x/a.xltm", crate::XlsxKind::MacroTemplate, "application/vnd.ms-excel.template.macroEnabled.main+xml"),
    ] {
        assert_eq!(crate::XlsxKind::from_path(path), kind);
        let bytes = crate::write_xlsx_as(&wb, kind).unwrap();
        let mut pkg = Package::open(&bytes).unwrap();
        let types = pkg.read_xml("[Content_Types].xml").unwrap().unwrap();
        let main: Vec<_> =
            types.kids("Override").filter(|e| e.attr("PartName") == Some("/xl/workbook.xml")).filter_map(|e| e.attr("ContentType")).collect();
        assert_eq!(main, [ct], "{path}");
        // Every kind reads back.
        let (back, report) = read_xlsx(&bytes).unwrap();
        assert!(report.warnings.is_empty(), "{path}: {:?}", report.warnings);
        assert_eq!(back.sheet(0).unwrap().value(at("A1")), Value::Number(7.0));
    }
    assert_eq!(crate::XlsxKind::from_path("noext"), crate::XlsxKind::Workbook);
}

/// The workbook's default font (`fonts[0]`, the Normal style's) is what every default-formatted
/// cell shows, typed or empty; saving keeps it instead of resetting it to Calibri 11 (#805).
#[test]
fn default_font_survives_a_round_trip() {
    let styles = |font0: &str| {
        format!(
            r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font>{font0}</font><font><b/><sz val="14"/><color theme="1"/><name val="Arial"/><family val="2"/></font></fonts>
            <fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
            <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
            <cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs>
            <cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
        )
    };
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    let body = r#"<sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>typed text</t></is></c><c r="B1" s="1"><v>1</v></c></row></sheetData>"#;
    let font0 = |bytes: &[u8]| {
        let mut pkg = Package::open(bytes).unwrap();
        let st = pkg.read_xml("xl/styles.xml").unwrap().unwrap();
        let f = st.child("fonts").unwrap().kids("font").next().unwrap().clone();
        let val = |n: &str| f.child(n).and_then(|e| e.attr("val")).map(str::to_string);
        (val("sz"), val("name"), val("family"), val("scheme"))
    };

    let file =
        super::minimal(body, &[("xl/styles.xml", &styles(r#"<sz val="10"/><color theme="1"/><name val="Arial"/><family val="2"/>"#))], rels, "");
    let (wb, _) = read_xlsx(&file).unwrap();
    let saved = write_xlsx(&wb).unwrap();
    assert_eq!(font0(&saved), (Some("10".into()), Some("Arial".into()), Some("2".into()), None));
    let (back, _) = read_xlsx(&saved).unwrap();
    let normal = &back.styles.get(StyleId::DEFAULT).font;
    assert_eq!((normal.name.as_str(), normal.size), ("Arial", 10.0), "empty cells and new input take the Normal font");
    let s = back.sheet(0).unwrap();
    assert_eq!(s.cell(at("A1")).unwrap().style, StyleId::DEFAULT);
    let b1 = &back.styles.get(s.cell(at("B1")).unwrap().style).font;
    assert!(b1.bold && b1.size == 14.0, "an explicit font is still its own: {b1:?}");

    // Excel's own Normal font follows the theme's body font; the scheme link survives too.
    let file = super::minimal(
        body,
        &[("xl/styles.xml", &styles(r#"<name val="Calibri"/><family val="2"/><color theme="1"/><sz val="12"/><scheme val="minor"/>"#))],
        rels,
        "",
    );
    let saved = write_xlsx(&read_xlsx(&file).unwrap().0).unwrap();
    assert_eq!(font0(&saved), (Some("12".into()), Some("Calibri".into()), Some("2".into()), Some("minor".into())));
}

fn one_font_workbook(font: &str) -> Vec<u8> {
    let styles = format!(
        r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="1"><font>{font}</font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#
    );
    let rels = r#"<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#;
    super::minimal("<sheetData/>", &[("xl/styles.xml", &styles)], rels, "")
}

#[test]
fn font_charset_and_family_roundtrip() {
    let file = one_font_workbook(r#"<name val="Traditional Arabic"/><family val="1"/><charset val="178"/>"#);
    let (wb, _) = read_xlsx(&file).unwrap();
    let saved = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let xml = String::from_utf8(pkg.read("xl/styles.xml").unwrap().unwrap()).unwrap();
    assert!(xml.contains("<name val=\"Traditional Arabic\"/><family val=\"1\"/><charset val=\"178\"/>"), "{xml}");

    let (back, _) = read_xlsx(&saved).unwrap();
    let font = &back.styles.get(StyleId::DEFAULT).font;
    let back_saved = write_xlsx(&back).unwrap();
    let mut pkg = Package::open(&back_saved).unwrap();
    let xml = String::from_utf8(pkg.read("xl/styles.xml").unwrap().unwrap()).unwrap();
    assert!(xml.contains("<name val=\"Traditional Arabic\"/><family val=\"1\"/><charset val=\"178\"/>"), "{xml}");
    assert_eq!(font.name, "Traditional Arabic");
    assert_eq!(font.family, Some(1));
    assert_eq!(font.charset, Some(178));
}

#[test]
fn fonts_without_charset_write_unchanged() {
    let file = one_font_workbook(r#"<name val="Arial"/>"#);
    let saved = write_xlsx(&read_xlsx(&file).unwrap().0).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let xml = String::from_utf8(pkg.read("xl/styles.xml").unwrap().unwrap()).unwrap();
    assert!(xml.contains("<name val=\"Arial\"/><family val=\"2\"/>"), "{xml}");
    assert!(!xml.contains("<charset"), "{xml}");
}

#[test]
fn out_of_range_family_is_ignored() {
    let file = one_font_workbook(r#"<name val="Arial"/><family val="15"/>"#);
    let (wb, _) = read_xlsx(&file).unwrap();
    assert_eq!(wb.styles.get(StyleId::DEFAULT).font.family, None);
    let saved = write_xlsx(&wb).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let xml = String::from_utf8(pkg.read("xl/styles.xml").unwrap().unwrap()).unwrap();
    assert!(xml.contains("<name val=\"Arial\"/><family val=\"2\"/>"), "{xml}");

    let mut set_directly = wb.clone();
    let wide = set_directly.styles.derive(StyleId::DEFAULT, |st| st.font.family = Some(200));
    set_directly.sheet_mut(0).unwrap().set_value(CellRef::new(0, 0), Value::text("x"));
    set_directly.sheet_mut(0).unwrap().set_style(CellRef::new(0, 0), wide);
    let saved = write_xlsx(&set_directly).unwrap();
    let mut pkg = Package::open(&saved).unwrap();
    let xml = String::from_utf8(pkg.read("xl/styles.xml").unwrap().unwrap()).unwrap();
    assert!(!xml.contains("<family val=\"200\"/>"), "{xml}");
}

#[test]
fn solid_data_bar_survives_with_sparklines_and_gradient_neighbours() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    s.set_value(at("A1"), Value::Number(1.0));
    let bar =
        |gradient: bool, max: CfValueKind| CfRule::DataBar { min: CfValueKind::Min, max, color: Color::Rgb(0x638EC6), gradient, show_value: true };
    let rules = [(true, "A1", CfValueKind::Max), (false, "B1", CfValueKind::Formula("$C$1".into())), (false, "D1:D3", CfValueKind::Number(9.0))];
    for (i, (gradient, range, max)) in rules.into_iter().enumerate() {
        s.cond_formats.push(CondFormat { ranges: vec![rr(range)], rule: bar(gradient, max), priority: i as u32 + 1, stop_if_true: false });
    }
    s.sparklines.push(Sparkline { cell: at("F1"), source: "A1:C1".into(), kind: SparklineKind::Line, color: Color::Rgb(0x376092), markers: false });
    let bytes = write_xlsx(&wb).unwrap();
    // x14 cfvo values are `xm:f` children; a `val` attribute there is not valid x14.
    let (xml, _) = sheet1_xml(&bytes);
    assert!(xml.contains("<x14:cfvo type=\"num\"><xm:f>9</xm:f></x14:cfvo>"), "{xml}");
    let (back, rep) = read_xlsx(&bytes).unwrap();
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
    let b = &back.sheets[0];
    assert_eq!(b.cond_formats, wb.sheets[0].cond_formats);
    assert_eq!(b.sparklines.len(), 1);
}

#[test]
fn doc_props_keep_boundary_whitespace() {
    let mut wb = Workbook::new();
    wb.props = DocProps {
        title: "  Title  ".into(),
        subject: "\n".into(),
        author: "\tMe\t".into(),
        company: " Co ".into(),
        keywords: " a, b ".into(),
        description: "\n  d\n".into(),
    };
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.props, wb.props);
}

/// Japanese paper sizes survive an XLSX save and reopen. IDs 12/13 are the JIS B4/B5; the
/// Japanese postcards and envelopes have their own IDs, and `B4 (ISO)` keeps ISO B4. The ISO B5/B6
/// names only exist in the engine: ECMA-376 has no `paperSize` id for them (only ISO B4), so they
/// are not part of the round-trip.
#[test]
fn japanese_paper_sizes_round_trip() {
    for paper in [
        "B4",
        "B5",
        "B6",
        "B4 (ISO)",
        "A6",
        "Japanese Postcard",
        "Japanese Double Postcard",
        "Japanese Envelope #3",
        "Japanese Envelope #4",
        "Japanese Envelope Kaku #2",
        "Japanese Envelope Kaku #3",
    ] {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).unwrap().print.paper = paper.into();
        let (back, report) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
        assert!(report.warnings.is_empty(), "{paper}: {:?}", report.warnings);
        assert_eq!(back.sheet(0).unwrap().print.paper, paper);
    }
    // An alias is written as the file's own name and read back as that name.
    let mut wb = Workbook::new();
    wb.sheet_mut(0).unwrap().print.paper = "B5 (JIS)".into();
    let (back, _) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert_eq!(back.sheet(0).unwrap().print.paper, "B5");
}

/// A table's dropdown filter lives in `sheet.autofilter` (the engine's only live filter state),
/// not in `Table::filter`. Writing used to suppress the sheet `<autoFilter>` because a table
/// intersects it and write the table part from `Table::filter`, which was empty: the criteria
/// were lost on every save, leaving the rows hidden with no filter to clear on reopen.
#[test]
fn a_table_filter_criteria_survive_a_round_trip() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    for (r, v) in [(0u32, "Region"), (1, "East"), (2, "West"), (3, "East")] {
        s.set_value(at(&format!("A{}", r + 1)), Value::text(v));
    }
    s.tables.push(Table {
        id: 1,
        name: "Sales".into(),
        range: rr("A1:A4"),
        header_row: true,
        totals_row: false,
        columns: vec![TableColumn { name: "Region".into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None }],
        style: "TableStyleMedium2".into(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    // The dropdown of a table's header cell installs the sheet AutoFilter over the table's
    // range, and `data.filterBy` stores its criteria there.
    s.autofilter =
        Some(AutoFilter { range: rr("A1:A4"), criteria: vec![(0, FilterCriterion::Values { values: vec!["East".into()], blanks: false })] });
    s.rows.entry(1).or_default().hidden = true;

    let (back, rep) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
    let b = back.sheet(0).unwrap();
    let t = b.tables.first().expect("the table survived");
    let (_, crit) = t.filter.first().expect("the table's filter criteria were kept on save");
    assert!(matches!(crit, FilterCriterion::Values { values, .. } if values == &["East".to_string()]), "the criteria were dropped: {:?}", t.filter);
    assert!(b.is_row_hidden(1), "the row the filter hid is still hidden");
    assert!(b.autofilter.is_none(), "a table's filter belongs to the table part, not the sheet");
}

/// Stored `colId` offsets are relative to the filter range's first column, so criteria of a filter
/// that starts right of the table's first column must not be folded into the table part, where
/// they would apply to the wrong column.
#[test]
fn a_partial_sheet_filter_is_not_folded_into_a_table() {
    let mut wb = Workbook::new();
    let s = wb.sheet_mut(0).unwrap();
    for (a, v) in [("A1", "Id"), ("B1", "Region"), ("A2", "1"), ("B2", "East"), ("A3", "2"), ("B3", "West")] {
        s.set_value(at(a), Value::text(v));
    }
    let column =
        |name: &str| TableColumn { name: name.into(), totals: TotalsFn::None, totals_label: None, formula: None, unique_name: None, xml: None };
    s.tables.push(Table {
        id: 1,
        name: "Sales".into(),
        range: rr("A1:B3"),
        header_row: true,
        totals_row: false,
        columns: vec![column("Id"), column("Region")],
        style: "TableStyleMedium2".into(),
        banded_rows: true,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: true,
        filter: vec![],
        table_type: None,
        connection_id: None,
    });
    s.autofilter =
        Some(AutoFilter { range: rr("B1:B3"), criteria: vec![(0, FilterCriterion::Values { values: vec!["East".into()], blanks: false })] });

    let (back, rep) = read_xlsx(&write_xlsx(&wb).unwrap()).unwrap();
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
    let b = back.sheet(0).unwrap();
    let t = b.tables.first().expect("the table survived");
    assert!(t.filter.is_empty(), "the criteria landed on the wrong table column: {:?}", t.filter);
    assert!(b.autofilter.is_none(), "a sheet filter inside a table would be invalid");
}

#[test]
fn mirrored_axis_orientation_round_trips() {
    let mk = |id: u32, kind: ChartKind, cell: &str, cat: bool, val: bool| Chart {
        id,
        kind,
        anchor: Anchor { cell: at(cell), width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series: vec![Series {
            name: Some("Data!$B$1".into()),
            categories: Some("Data!$A$1:$A$2".into()),
            values: "Data!$B$1:$B$2".into(),
            bubble_sizes: None,
            color: None,
            secondary: false,
            kind: None,
            smooth: false,
            trendlines: Vec::new(),
            quartiles: Default::default(),
        }],
        legend: LegendPos::Bottom,
        data_labels: false,
        gridlines: true,
        style: 2,
        x_title: None,
        y_title: None,
        source: None,
        by_rows: false,
        pivot: None,
        cat_reverse: cat,
        val_reverse: val,
    };
    let mut wb = Workbook::new();
    let mut s = Sheet::new("Data");
    s.set_value(at("A1"), Value::text("Q1"));
    s.set_value(at("A2"), Value::text("Q2"));
    s.set_value(at("B1"), Value::Number(10.0));
    s.set_value(at("B2"), Value::Number(20.0));
    // A column chart mirrors both axes; a scatter has no category axis, only its y axis (#1687).
    s.charts.push(mk(1, ChartKind::ColumnClustered, "F2", true, true));
    s.charts.push(mk(2, ChartKind::Scatter, "F20", false, true));
    wb.sheets = vec![Arc::new(s)];
    let bytes = write_xlsx(&wb).unwrap();

    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.clone())).unwrap();
    let mut part = |n: &str| {
        let mut out = String::new();
        std::io::Read::read_to_string(&mut z.by_name(n).unwrap_or_else(|_| panic!("{n}")), &mut out).unwrap();
        out
    };
    let col = part("xl/charts/chart1.xml");
    assert_eq!(col.matches("orientation val=\"maxMin\"").count(), 2, "{col}");
    assert!(!col.contains("orientation val=\"minMax\""), "{col}");
    let sc = part("xl/charts/chart2.xml");
    assert_eq!(sc.matches("orientation val=\"maxMin\"").count(), 1, "{sc}");
    assert_eq!(sc.matches("orientation val=\"minMax\"").count(), 1, "{sc}");

    let (back, rep) = read_xlsx(&bytes).unwrap();
    assert!(rep.warnings.is_empty(), "{:?}", rep.warnings);
    let charts = &back.sheets[0].charts;
    assert_eq!(charts.len(), 2);
    assert!(charts.iter().any(|c| c.kind == ChartKind::ColumnClustered && c.cat_reverse && c.val_reverse), "both directions lost");
    assert!(charts.iter().any(|c| c.kind == ChartKind::Scatter && !c.cat_reverse && c.val_reverse), "scatter y direction lost");
}
