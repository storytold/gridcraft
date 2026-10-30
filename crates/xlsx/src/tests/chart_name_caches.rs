//! A series that goes through a defined name is saved with the cache Excel writes for it: the
//! values of the cells the name stands for and their number format (observed in Excel for
//! Windows, Microsoft 365: charts over `[0]!Name` carry the same `numCache`/`strCache` as charts
//! over the plain reference, dynamic `OFFSET` names included).

use std::io::Read;
use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Anchor, Chart, ChartKind, DefinedName, LegendPos, NumFmt, Series, Sheet, Style, Workbook};

use crate::write_xlsx;

fn name(n: &str, formula: &str) -> DefinedName {
    DefinedName { name: n.into(), scope: None, formula: formula.into(), comment: String::new(), hidden: false }
}

fn chart_xml(cats: &str, vals: &str, names: Vec<DefinedName>) -> String {
    let mut wb = Workbook::default();
    let date = wb.styles.intern(Style { num_fmt: NumFmt(Arc::from("m/d/yyyy")), ..Style::default() });
    let money = wb.styles.intern(Style { num_fmt: NumFmt(Arc::from("0.00")), ..Style::default() });
    let mut s = Sheet::new("Sheet1");
    for (i, (d, v)) in [(45296.0, 10.0), (45327.0, 20.0), (45356.0, 30.0)].iter().enumerate() {
        let r = i as u32 + 1;
        s.set_value(CellRef::new(r, 0), Value::Number(*d));
        s.set_style(CellRef::new(r, 0), date);
        s.set_value(CellRef::new(r, 1), Value::Number(*v));
        s.set_style(CellRef::new(r, 1), money);
        s.set_value(CellRef::new(r + 5, 0), Value::from(["a", "b", "c"][i]));
    }
    s.charts.push(Chart {
        id: 1,
        kind: ChartKind::ColumnClustered,
        anchor: Anchor { cell: CellRef::new(0, 4), width: 480.0, height: 300.0, ..Anchor::default() },
        title: None,
        series: vec![Series {
            name: None,
            categories: Some(cats.into()),
            values: vals.into(),
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
    });
    wb.sheets = vec![Arc::new(s)];
    wb.names = names;
    let bytes = write_xlsx(&wb).unwrap();
    let mut xml = String::new();
    zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap().by_name("xl/charts/chart1.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

fn part<'a>(xml: &'a str, tag: &str) -> &'a str {
    let open = format!("<c:{tag}>");
    let start = xml.find(&open).unwrap() + open.len();
    let end = xml[start..].find(&format!("</c:{tag}>")).unwrap() + start;
    &xml[start..end]
}

const DATE_CACHE: &str = "<c:numCache><c:formatCode>m/d/yyyy</c:formatCode><c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>45296</c:v></c:pt><c:pt idx=\"1\"><c:v>45327</c:v></c:pt><c:pt idx=\"2\"><c:v>45356</c:v></c:pt></c:numCache>";
const VAL_CACHE: &str = "<c:numCache><c:formatCode>0.00</c:formatCode><c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>10</c:v></c:pt><c:pt idx=\"1\"><c:v>20</c:v></c:pt><c:pt idx=\"2\"><c:v>30</c:v></c:pt></c:numCache>";

#[test]
fn plain_references_carry_the_source_format() {
    let xml = chart_xml("Sheet1!$A$2:$A$4", "Sheet1!$B$2:$B$4", vec![]);
    assert_eq!(part(&xml, "cat"), format!("<c:numRef><c:f>Sheet1!$A$2:$A$4</c:f>{DATE_CACHE}</c:numRef>"));
    assert_eq!(part(&xml, "val"), format!("<c:numRef><c:f>Sheet1!$B$2:$B$4</c:f>{VAL_CACHE}</c:numRef>"));
}

#[test]
fn static_names_are_cached_like_the_plain_reference() {
    let names = vec![name("Cats", "Sheet1!$A$2:$A$4"), name("Vals", "Sheet1!$B$2:$B$4")];
    let xml = chart_xml("Cats", "Vals", names);
    assert_eq!(part(&xml, "cat"), format!("<c:numRef><c:f>[0]!Cats</c:f>{DATE_CACHE}</c:numRef>"));
    assert_eq!(part(&xml, "val"), format!("<c:numRef><c:f>[0]!Vals</c:f>{VAL_CACHE}</c:numRef>"));
}

#[test]
fn names_of_names_and_offsets_are_cached() {
    let names = vec![
        name("Base", "Sheet1!$A$2:$A$4"),
        name("Cats", "Base"),
        name("DynV", "OFFSET(Sheet1!$B$2,0,0,3,1)"),
        name("Shifted", "OFFSET(Cats,0,1)"),
    ];
    let xml = chart_xml("Cats", "DynV", names.clone());
    assert_eq!(part(&xml, "cat"), format!("<c:numRef><c:f>[0]!Cats</c:f>{DATE_CACHE}</c:numRef>"));
    assert_eq!(part(&xml, "val"), format!("<c:numRef><c:f>[0]!DynV</c:f>{VAL_CACHE}</c:numRef>"));
    // OFFSET of a name: the cells one column over.
    assert_eq!(part(&chart_xml("Cats", "Shifted", names), "val"), format!("<c:numRef><c:f>[0]!Shifted</c:f>{VAL_CACHE}</c:numRef>"));
}

#[test]
fn text_categories_through_a_name_get_a_string_cache() {
    let xml = chart_xml("Txt", "Sheet1!$B$2:$B$4", vec![name("Txt", "Sheet1!$A$7:$A$9")]);
    assert_eq!(
        part(&xml, "cat"),
        "<c:strRef><c:f>[0]!Txt</c:f><c:strCache><c:ptCount val=\"3\"/><c:pt idx=\"0\"><c:v>a</c:v></c:pt><c:pt idx=\"1\"><c:v>b</c:v></c:pt><c:pt idx=\"2\"><c:v>c</c:v></c:pt></c:strCache></c:strRef>"
    );
}

#[test]
fn names_that_cannot_be_resolved_stay_without_a_cache() {
    // Name chains that loop, calculated sizes and undefined names: no cache, and no hang.
    let names = vec![name("A", "B"), name("B", "A"), name("Calc", "OFFSET(Sheet1!$B$2,0,0,COUNT(Sheet1!$B:$B),1)")];
    for n in ["A", "Calc", "Missing"] {
        let xml = chart_xml(n, n, names.clone());
        assert!(!xml.contains("<c:numCache>") && !xml.contains("<c:strCache>"), "{n}: {xml}");
        assert!(xml.contains(&format!("<c:f>[0]!{n}</c:f>")), "{n}: {xml}");
    }
}
