use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::{PivotLayout, PivotSort};
use serde_json::{Value as Json, json};

use crate::Session;

/// Region, Product, Date, Units, Sales — 12 records over 2026.
fn sales() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([
        ["Region", "Product", "Date", "Units", "Sales"],
        ["East", "Apples", "1/15/2026", 10, 100],
        ["West", "Apples", "2/10/2026", 5, 50],
        ["East", "Pears", "3/3/2026", 8, 120],
        ["North", "Pears", "4/20/2026", 2, 30],
        ["West", "Plums", "5/5/2026", 7, 70],
        ["East", "Plums", "6/30/2026", 4, 60],
        ["North", "Apples", "7/4/2026", 9, 90],
        ["West", "Pears", "8/8/2026", 3, 45],
        ["East", "Apples", "9/9/2026", 6, 60],
        ["North", "Plums", "10/10/2026", 1, 15],
        ["West", "Apples", "11/11/2026", 12, 120],
        ["East", "Pears", "12/12/2026", 5, 75]
    ]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s
}
const TOTAL: f64 = 835.0;

fn val(s: &Session, a: &str) -> Value {
    s.doc().unwrap().wb.active().unwrap().value(CellRef::parse(a).unwrap())
}
fn text(s: &Session, a: &str) -> String {
    val(s, a).display()
}
fn pt(s: &Session) -> gridcraft_model::PivotTable {
    s.doc().unwrap().wb.active().unwrap().pivots.last().unwrap().clone()
}
/// Finds the row (0-based) whose first report column holds `label`.
fn find_row(s: &Session, label: &str) -> Option<u32> {
    let p = pt(s);
    let r = p.last_range.unwrap();
    (r.start.row..=r.end.row).find(|row| val(s, &CellRef::new(*row, r.start.col).a1()).display() == label)
}

#[test]
fn insert_creates_placeholder_on_new_sheet() {
    let mut s = sales();
    let r = s.execute("insert.pivotTable", json!({})).unwrap();
    assert_eq!(r["name"], "PivotTable1");
    assert_eq!(r["anchor"], "A3");
    assert_eq!(r["fields"], json!(["Region", "Product", "Date", "Units", "Sales"]));
    let d = s.doc().unwrap();
    assert_eq!(d.wb.sheets.len(), 2);
    assert_eq!(d.wb.active_sheet(), 0, "new sheet goes before the current one");
    assert_eq!(d.wb.active().unwrap().name, "Sheet2");
    assert!(text(&s, "A3").starts_with("PivotTable1"));
    assert_eq!(pt(&s).source, "Sheet1!$A$1:$E$13");
    // One undo step removes it.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 1);
}

#[test]
fn region_by_product_sum_of_sales() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({})).unwrap();
    s.execute("pivot.addField", json!({"field": "region", "area": "rows"})).unwrap();
    s.execute("pivot.addField", json!({"field": "Product", "area": "columns"})).unwrap();
    let r = s.execute("pivot.addField", json!({"field": "Sales", "area": "values"})).unwrap();
    assert_eq!(r["range"], "A3:E8");
    assert_eq!(text(&s, "A3"), "Sum of Sales");
    assert_eq!(text(&s, "B3"), "Column Labels");
    assert_eq!(text(&s, "A4"), "Row Labels");
    assert_eq!(text(&s, "B4"), "Apples");
    assert_eq!(text(&s, "C4"), "Pears");
    assert_eq!(text(&s, "D4"), "Plums");
    assert_eq!(text(&s, "E4"), "Grand Total");
    assert_eq!(text(&s, "A5"), "East");
    assert_eq!(text(&s, "A6"), "North");
    assert_eq!(text(&s, "A7"), "West");
    assert_eq!(text(&s, "A8"), "Grand Total");
    assert_eq!(val(&s, "B5"), Value::Number(160.0));
    assert_eq!(val(&s, "C5"), Value::Number(195.0));
    assert_eq!(val(&s, "E5"), Value::Number(415.0));
    assert_eq!(val(&s, "E8"), Value::Number(TOTAL));
    // Column grand totals add up too.
    let sum: f64 = ["B8", "C8", "D8"].iter().map(|a| val(&s, a).as_f64().unwrap()).sum();
    assert_eq!(sum, TOTAL);
    let rows: f64 = ["E5", "E6", "E7"].iter().map(|a| val(&s, a).as_f64().unwrap()).sum();
    assert_eq!(rows, TOTAL);
    // Styles: header bold, grand total bold with a top border.
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert!(d.wb.styles.get(sh.style_id(CellRef::parse("A4").unwrap())).font.bold);
    let g = d.wb.styles.get(sh.style_id(CellRef::parse("B8").unwrap()));
    assert!(g.font.bold && !g.borders.top.is_none());
    // Listing.
    let l = s.execute("pivot.list", json!({})).unwrap();
    assert_eq!(l[0]["values"][0]["name"], "Sum of Sales");
}

#[test]
fn multiple_value_fields_make_a_values_dimension() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales", {"field": "Units", "func": "average"}]})).unwrap();
    assert_eq!(text(&s, "A3"), "Row Labels");
    assert_eq!(text(&s, "B3"), "Sum of Sales");
    assert_eq!(text(&s, "C3"), "Average of Units");
    assert_eq!(text(&s, "A7"), "Grand Total");
    assert_eq!(val(&s, "B7"), Value::Number(TOTAL));
    assert_eq!(val(&s, "C7"), Value::Number(72.0 / 12.0));
    // With a column field the values become the innermost column level.
    s.execute("pivot.addField", json!({"field": "Product", "area": "columns"})).unwrap();
    assert_eq!(text(&s, "B4"), "Apples");
    assert_eq!(text(&s, "B5"), "Sum of Sales");
    assert_eq!(text(&s, "C5"), "Average of Units");
    assert_eq!(text(&s, "D4"), "Pears");
    assert_eq!(text(&s, "H4"), "Total Sum of Sales");
    assert_eq!(text(&s, "I4"), "Total Average of Units");
    let last = pt(&s).last_range.unwrap();
    assert_eq!(val(&s, &CellRef::new(last.end.row, 7).a1()), Value::Number(TOTAL));
}

#[test]
fn date_grouping_by_quarter_and_month() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"]})).unwrap();
    // Ungrouped: one row per date, shown with the source date format.
    assert_eq!(pt(&s).last_range.unwrap().height(), 14);
    s.execute("pivot.group", json!({"field": "Date", "by": "quarters"})).unwrap();
    assert_eq!(text(&s, "A4"), "Qtr1");
    assert_eq!(text(&s, "A7"), "Qtr4");
    assert_eq!(val(&s, "B4"), Value::Number(270.0));
    assert_eq!(val(&s, "B5"), Value::Number(160.0));
    assert_eq!(val(&s, "B8"), Value::Number(TOTAL));
    s.execute("pivot.group", json!({"field": "Date", "by": "months"})).unwrap();
    assert_eq!(text(&s, "A4"), "Jan");
    assert_eq!(text(&s, "A15"), "Dec");
    s.execute("pivot.group", json!({"field": "Date", "by": "years"})).unwrap();
    assert_eq!(text(&s, "A4"), "2026");
    assert!(s.execute("pivot.group", json!({"field": "Region", "by": "years"})).is_err());
}

#[test]
fn filters_and_report_filter_rows() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
    // Report filter above the body; the body stays at A3.
    assert_eq!(text(&s, "A1"), "Region");
    assert_eq!(text(&s, "B1"), "(All)");
    assert_eq!(text(&s, "A3"), "Row Labels");
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    assert_eq!(text(&s, "B1"), "East");
    let gt = find_row(&s, "Grand Total").unwrap();
    assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(415.0));
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East", "west"]})).unwrap();
    assert_eq!(text(&s, "B1"), "(Multiple Items)");
    s.execute("pivot.filter", json!({"field": "Region", "selected": null})).unwrap();
    let gt = find_row(&s, "Grand Total").unwrap();
    assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(TOTAL));
    // Hidden items on a row field.
    s.execute("pivot.filter", json!({"field": "Product", "selected": ["Apples"]})).unwrap();
    assert_eq!(text(&s, "A4"), "Apples");
    assert_eq!(text(&s, "A5"), "Grand Total");
    assert_eq!(val(&s, "B5"), Value::Number(420.0));
    assert!(s.execute("pivot.filter", json!({"field": "Product", "selected": []})).is_err());
}

/// A selection of the most labels allowed (100,000) is looked up once per record, not scanned: the
/// report still filters case-insensitively.
#[test]
fn a_huge_filter_selection_is_matched_through_a_set() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
    let mut selected: Vec<String> = (0..99_999).map(|i| format!("Region {i}")).collect();
    selected.push("EAST".into());
    s.execute("pivot.filter", json!({"field": "Region", "selected": selected})).unwrap();
    let gt = find_row(&s, "Grand Total").unwrap();
    assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(415.0));
}

#[test]
fn show_values_as() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.valueSettings", json!({"field": "Sum of Sales", "showAs": "percentOfGrandTotal"})).unwrap();
    let east = val(&s, "B4").as_f64().unwrap();
    assert!((east - 415.0 / TOTAL).abs() < 1e-12);
    assert_eq!(val(&s, "B7"), Value::Number(1.0));
    assert_eq!(text(&s, "B7"), "1");
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("B4").unwrap())).num_fmt.as_str(), "0.00%");
    s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "runningTotal"})).unwrap();
    assert_eq!(val(&s, "B4"), Value::Number(415.0));
    assert_eq!(val(&s, "B5"), Value::Number(415.0 + 135.0));
    assert_eq!(val(&s, "B6"), Value::Number(TOTAL));
    s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "rank"})).unwrap();
    assert_eq!(val(&s, "B4"), Value::Number(1.0));
    assert_eq!(val(&s, "B5"), Value::Number(3.0));
    assert_eq!(val(&s, "B6"), Value::Number(2.0));
    s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "normal", "func": "max", "numberFormat": "#,##0"})).unwrap();
    assert_eq!(text(&s, "B3"), "Max of Sales");
    assert_eq!(val(&s, "B4"), Value::Number(120.0));
    assert!(s.execute("pivot.valueSettings", json!({"index": 0, "name": "Sales"})).is_err());
    s.execute("pivot.valueSettings", json!({"index": 0, "name": "Biggest sale"})).unwrap();
    assert_eq!(text(&s, "B3"), "Biggest sale");
}

/// Running Total over nested rows adds up each child's value across the top-level items, in the
/// order they appear: the same value as summing the plain values of the earlier items' rows.
#[test]
fn running_total_over_nested_rows_adds_up_across_the_top_level_items() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales"]})).unwrap();
    let r = pt(&s).last_range.unwrap();
    let rows: Vec<u32> = (r.start.row + 1..r.end.row).collect();
    let read = |s: &Session| -> Vec<(String, f64)> {
        rows.iter().map(|row| (text(s, &CellRef::new(*row, 0).a1()), val(s, &CellRef::new(*row, 1).a1()).as_f64().unwrap_or(0.0))).collect()
    };
    let before = read(&s);
    s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "runningTotal"})).unwrap();
    let after = read(&s);
    // Region rows carry the report's top-level items (not indented); the others are products.
    let indent = |s: &Session, row: u32| {
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        d.wb.styles.get(sh.style_id(CellRef::new(row, 0))).align.indent
    };
    let (mut regions, mut products) = (0.0, std::collections::HashMap::<String, f64>::new());
    for (i, row) in rows.iter().enumerate() {
        let name = &before[i].0;
        let total = if indent(&s, *row) == 0 {
            regions += before[i].1;
            regions
        } else {
            let t = products.entry(name.clone()).or_insert(0.0);
            *t += before[i].1;
            *t
        };
        assert!((after[i].1 - total).abs() < 1e-9, "row {name}: {} vs {total}", after[i].1);
    }
}

#[test]
fn compact_vs_tabular_shapes() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales"]})).unwrap();
    // Compact: one label column; parents carry subtotals at the top; children indented.
    let r = pt(&s).last_range.unwrap();
    assert_eq!(r.width(), 2);
    // header + 3 regions + 9 (region, product) pairs present + grand total
    assert_eq!(r.height(), 1 + 3 + 9 + 1);
    assert_eq!(text(&s, "A4"), "East");
    assert_eq!(val(&s, "B4"), Value::Number(415.0));
    assert_eq!(text(&s, "A5"), "Apples");
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("A5").unwrap())).align.indent, 1);
    assert!(d.wb.styles.get(sh.style_id(CellRef::parse("A4").unwrap())).font.bold);
    // Outline: one column per field.
    s.execute("pivot.layout", json!({"layout": "outline"})).unwrap();
    let r = pt(&s).last_range.unwrap();
    assert_eq!(r.width(), 3);
    assert_eq!(text(&s, "A3"), "Region");
    assert_eq!(text(&s, "B3"), "Product");
    assert_eq!(text(&s, "A4"), "East");
    assert_eq!(text(&s, "B5"), "Apples");
    // Tabular: first child on its parent's row; "East Total" rows below each group.
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    let r = pt(&s).last_range.unwrap();
    assert_eq!(r.width(), 3);
    assert_eq!(r.height(), 1 + 9 + 3 + 1);
    assert_eq!(text(&s, "A4"), "East");
    assert_eq!(text(&s, "B4"), "Apples");
    assert_eq!(text(&s, "A5"), "");
    assert_eq!(text(&s, "B5"), "Pears");
    assert_eq!(text(&s, "A7"), "East Total");
    assert_eq!(val(&s, "C7"), Value::Number(415.0));
    // No subtotals, no grand totals.
    s.execute("pivot.layout", json!({"subtotals": false, "grandTotalsCols": false})).unwrap();
    assert_eq!(pt(&s).last_range.unwrap().height(), 1 + 9);
    // Collapse East in compact layout: its children disappear.
    s.execute("pivot.layout", json!({"layout": "compact", "subtotals": "top"})).unwrap();
    s.execute("pivot.collapse", json!({"field": "Region", "items": ["East"]})).unwrap();
    assert_eq!(text(&s, "A4"), "East");
    assert_eq!(text(&s, "A5"), "North");
    s.execute("pivot.collapse", json!({"field": "Region", "collapse": false})).unwrap();
    assert_eq!(text(&s, "A5"), "Apples");
}

fn assert_values_on_rows_default(s: &Session) {
    let p = pt(s);
    assert!(p.values_on_rows);
    assert_eq!(p.values_position, None);
    let r = p.last_range.unwrap();
    assert_eq!(r.width(), 2);
    // header + 3 × (region + 2 values) + 2 grand totals
    assert_eq!(r.height(), 1 + 9 + 2);
    assert_eq!(text(s, "A4"), "East");
    assert_eq!(val(s, "B4"), Value::Empty);
    assert_eq!(text(s, "A5"), "Sum of Sales");
    assert_eq!(val(s, "B5"), Value::Number(415.0));
    assert_eq!(text(s, "A6"), "Sum of Units");
    assert_eq!(val(s, "B6"), Value::Number(33.0));
    assert_eq!(text(s, "A13"), "Total Sum of Sales");
    assert_eq!(val(s, "B13"), Value::Number(TOTAL));
    assert_eq!(text(s, "A14"), "Total Sum of Units");
    assert_eq!(val(s, "B14"), Value::Number(72.0));
}

fn assert_values_on_rows_tabular(s: &mut Session) {
    // Tabular, no column field: "Total" heads the data column, as Excel saves it.
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    assert_eq!(text(s, "A3"), "Region");
    assert_eq!(text(s, "B3"), "Values");
    assert_eq!(text(s, "C3"), "Total");
    // The Values field's caption heads its column, as in Excel's saved reports.
    edit_pt(s, |p| p.data_caption = Some("Data".into()));
    assert_eq!(text(s, "B3"), "Data");
    edit_pt(s, |p| p.data_caption = None);
}

fn assert_values_on_rows_position(s: &mut Session) {
    s.execute("pivot.layout", json!({"layout": "compact"})).unwrap();
    // Σ Values before Region: the value fields are the outer items.
    s.execute("pivot.moveField", json!({"field": "Values", "from": "rows", "to": "rows", "position": 0})).unwrap();
    assert_eq!(pt(s).values_position, Some(0));
    assert_eq!(text(s, "A4"), "Sum of Sales");
    assert_eq!(val(s, "B4"), Value::Empty);
    assert_eq!(text(s, "A5"), "East");
    assert_eq!(val(s, "B5"), Value::Number(415.0));
    assert_eq!(text(s, "A8"), "Sum of Units");
    assert_eq!(text(s, "A9"), "East");
    assert_eq!(val(s, "B9"), Value::Number(33.0));
    assert_eq!(text(s, "A12"), "Total Sum of Sales");
    assert_eq!(val(s, "B13"), Value::Number(72.0));
    // Adding a row field in front keeps Σ Values ahead of Region.
    s.execute("pivot.addField", json!({"field": "Product", "area": "rows", "position": 0})).unwrap();
    assert_eq!(pt(s).values_position, Some(1));
    s.execute("pivot.removeField", json!({"field": "Product", "area": "rows"})).unwrap();
    assert_eq!(pt(s).values_position, Some(0));
    // No row field: one line per value field and no grand total.
    s.execute("pivot.removeField", json!({"field": "Region", "area": "rows"})).unwrap();
    let r = pt(s).last_range.unwrap();
    assert_eq!(r.height(), 1 + 2);
    assert_eq!(text(s, "A4"), "Sum of Sales");
    assert_eq!(val(s, "B4"), Value::Number(TOTAL));
    assert_eq!(text(s, "A5"), "Sum of Units");
    assert_eq!(val(s, "B5"), Value::Number(72.0));
    // Back to Columns.
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "rows", "to": "columns"})).unwrap();
    assert!(!pt(s).values_on_rows);
    assert_eq!(text(s, "A3"), "Sum of Sales");
    assert_eq!(text(s, "B3"), "Sum of Units");
    assert!(s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "filters"})).is_err());
    assert!(s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "rows", "to": "columns"})).is_err());
}

#[test]
fn values_on_rows_layouts() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales", "Units"]})).unwrap();
    assert_eq!(text(&s, "C3"), "Sum of Units");
    // Σ Values after Region: each region holds one line per value field.
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows"})).unwrap();
    assert_values_on_rows_default(&s);
    assert_values_on_rows_tabular(&mut s);
    assert_values_on_rows_position(&mut s);
}

#[test]
fn collapsed_item_puts_values_in_the_values_column() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales", "Units"]})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows"})).unwrap();
    s.execute("pivot.collapse", json!({"field": "Region", "items": ["East"]})).unwrap();
    for layout in ["tabular", "outline"] {
        s.execute("pivot.layout", json!({"layout": layout})).unwrap();
        assert_eq!(text(&s, "C3"), "Values", "{layout}");
        let east = (4..20).find(|r| text(&s, &format!("A{r}")) == "East").unwrap();
        let first = if layout == "tabular" { east } else { east + 1 };
        assert_eq!(text(&s, &format!("B{first}")), "", "{layout}");
        assert_eq!(text(&s, &format!("C{first}")), "Sum of Sales", "{layout}");
        assert_eq!(val(&s, &format!("D{first}")), Value::Number(415.0), "{layout}");
        assert_eq!(text(&s, &format!("C{}", first + 1)), "Sum of Units", "{layout}");
    }
}

#[test]
fn values_stay_last_in_columns() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "columns": ["Region"], "values": ["Sales", "Units"]})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "columns", "position": 0})).unwrap();
    let p = pt(&s);
    assert!(!p.values_on_rows);
    assert_eq!(p.values_position, None);
    // Σ Values to Rows and back with a position: still last in Columns.
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows"})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "rows", "to": "columns", "position": 0})).unwrap();
    assert_eq!(pt(&s).values_position, None);
    assert_eq!(text(&s, "B5"), "Sum of Sales");
    assert_eq!(text(&s, "C5"), "Sum of Units");
}

#[test]
fn change_source_keeps_values_ahead_of_its_field() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales", "Units"]})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows", "position": 1})).unwrap();
    // The new source has no Region: Σ Values stays ahead of Product.
    s.execute("pivot.changeSource", json!({"source": "Sheet1!B1:E13"})).unwrap();
    let p = pt(&s);
    assert_eq!(p.rows.len(), 1);
    assert_eq!(p.values_position, Some(0));
    assert_eq!(text(&s, "A4"), "Sum of Sales");
    assert_eq!(text(&s, "A5"), "Apples");
}

fn assert_values_between_compact_rows(s: &Session) {
    // Compact: East, then per value field its products, then per-value subtotals of East.
    assert_eq!(text(s, "A4"), "East");
    assert_eq!(text(s, "A5"), "Sum of Sales");
    assert_eq!(text(s, "A6"), "Apples");
    assert_eq!(val(s, "B6"), Value::Number(160.0));
    assert_eq!(text(s, "A9"), "Sum of Units");
    assert_eq!(text(s, "A10"), "Apples");
    assert_eq!(val(s, "B10"), Value::Number(16.0));
    assert_eq!(text(s, "A13"), "East Sum of Sales");
    assert_eq!(val(s, "B13"), Value::Number(415.0));
    assert_eq!(text(s, "A14"), "East Sum of Units");
    assert_eq!(val(s, "B14"), Value::Number(33.0));
    let d = s.doc().unwrap();
    let sh = d.wb.active().unwrap();
    assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("A5").unwrap())).align.indent, 1);
    assert_eq!(d.wb.styles.get(sh.style_id(CellRef::parse("A6").unwrap())).align.indent, 2);
}

fn assert_values_between_outline(s: &mut Session) {
    // Outline: one column per level, "Values" heads the Σ Values column.
    s.execute("pivot.layout", json!({"layout": "outline"})).unwrap();
    assert_eq!(pt(s).last_range.unwrap().width(), 4);
    assert_eq!(text(s, "A3"), "Region");
    assert_eq!(text(s, "B3"), "Values");
    assert_eq!(text(s, "C3"), "Product");
    assert_eq!(text(s, "B5"), "Sum of Sales");
    assert_eq!(text(s, "C6"), "Apples");
    assert_eq!(val(s, "D6"), Value::Number(160.0));
}

fn assert_values_between_tabular(s: &mut Session) {
    // Tabular: labels start on their parent's row; totals per value field below each group.
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    assert_eq!(text(s, "A4"), "East");
    assert_eq!(text(s, "B4"), "Sum of Sales");
    assert_eq!(text(s, "C4"), "Apples");
    assert_eq!(val(s, "D4"), Value::Number(160.0));
    assert_eq!(text(s, "B7"), "Sum of Units");
    assert_eq!(text(s, "C7"), "Apples");
    assert_eq!(val(s, "D7"), Value::Number(16.0));
    assert_eq!(text(s, "A10"), "East Sum of Sales");
    assert_eq!(val(s, "D10"), Value::Number(415.0));
    assert_eq!(text(s, "A11"), "East Sum of Units");
    let r = pt(s).last_range.unwrap();
    let last = r.end.row + 1;
    assert_eq!(text(s, &format!("A{}", last - 1)), "Total Sum of Sales");
    assert_eq!(text(s, &format!("A{last}")), "Total Sum of Units");
    assert_eq!(val(s, &format!("D{last}")), Value::Number(72.0));
}

#[test]
fn values_on_rows_between_row_fields() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales", "Units"]})).unwrap();
    s.execute("pivot.moveField", json!({"field": "Σ Values", "from": "columns", "to": "rows", "position": 1})).unwrap();
    assert_values_between_compact_rows(&s);
    assert_values_between_outline(&mut s);
    assert_values_between_tabular(&mut s);
}

#[test]
fn refresh_after_source_edits() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "E2", "input": "1100"})).unwrap();
    // Refresh All picks the change up.
    let r = s.execute("data.refreshAll", json!({})).unwrap();
    assert_eq!(r["refreshed"], 1);
    let d = s.doc().unwrap();
    let si = d.wb.sheet_index("Sheet2").unwrap();
    let sh = d.wb.sheet(si).unwrap();
    assert_eq!(sh.value(CellRef::parse("B4").unwrap()), Value::Number(1415.0));
    assert_eq!(sh.value(CellRef::parse("B7").unwrap()), Value::Number(TOTAL + 1000.0));
    // pivot.refresh by name; change source to fewer rows.
    s.execute("pivot.changeSource", json!({"pivot": "PivotTable1", "source": "Sheet1!A1:E3"})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.sheet(si).unwrap();
    assert_eq!(sh.value(CellRef::parse("A6").unwrap()).display(), "Grand Total");
    assert_eq!(sh.value(CellRef::parse("B6").unwrap()), Value::Number(1150.0));
    // Old rows were cleared.
    assert_eq!(sh.value(CellRef::parse("A7").unwrap()), Value::Empty);
    s.execute("pivot.refresh", json!({"pivot": "PivotTable1"})).unwrap();
}

/// Edits the active sheet's last PivotTable in the model (settings only a file sets) and
/// refreshes it.
fn edit_pt(s: &mut Session, f: impl FnOnce(&mut gridcraft_model::PivotTable)) {
    let d = s.doc_mut().unwrap();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    let si = wb.active_sheet();
    f(wb.sheet_mut(si).unwrap().pivots.last_mut().unwrap());
    s.execute("pivot.refresh", json!({})).unwrap();
}

#[test]
fn sort_by_value_within_each_parent() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    // East 415, West 285, North 135.
    s.execute("pivot.sort", json!({"field": "Region", "order": "desc", "by": "Sum of Sales"})).unwrap();
    assert_eq!(pt(&s).rows[0].sort_by.as_deref(), Some("Sum of Sales"));
    assert_eq!([text(&s, "A4"), text(&s, "A5"), text(&s, "A6")], ["East", "West", "North"]);
    s.execute("pivot.sort", json!({"field": "Region", "order": "asc", "by": "Sales"})).unwrap();
    assert_eq!([text(&s, "A4"), text(&s, "A5"), text(&s, "A6")], ["North", "West", "East"]);
    assert!(s.execute("pivot.sort", json!({"field": "Region", "order": "asc", "by": "Nope"})).is_err());
    // The sort follows a renamed value field; sorting without `by` sorts by label again.
    s.execute("pivot.valueSettings", json!({"field": "Sum of Sales", "name": "Revenue"})).unwrap();
    assert_eq!(pt(&s).rows[0].sort_by.as_deref(), Some("Revenue"));
    assert_eq!(text(&s, "A4"), "North");
    s.execute("pivot.sort", json!({"field": "Region", "order": "desc"})).unwrap();
    assert_eq!(pt(&s).rows[0].sort_by, None);
    assert_eq!(text(&s, "A4"), "West");
    // Inner items sort within their parent; subtotals stay with their group.
    s.execute("pivot.addField", json!({"field": "Product", "area": "rows"})).unwrap();
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    s.execute("pivot.sort", json!({"field": "Region", "order": "desc", "by": "Revenue"})).unwrap();
    s.execute("pivot.sort", json!({"field": "Product", "order": "desc", "by": "Revenue"})).unwrap();
    // East: Pears 195, Apples 160, Plums 60.
    assert_eq!([text(&s, "A4"), text(&s, "B4"), text(&s, "B5"), text(&s, "B6"), text(&s, "A7")], ["East", "Pears", "Apples", "Plums", "East Total"]);
    assert_eq!(val(&s, "C4"), Value::Number(195.0));
    // West: Apples 170, Plums 70, Pears 45.
    assert_eq!([text(&s, "A8"), text(&s, "B8"), text(&s, "B9"), text(&s, "B10")], ["West", "Apples", "Plums", "Pears"]);
    // Columns sort by their row totals.
    s.execute("pivot.moveField", json!({"field": "Region", "from": "rows", "to": "columns"})).unwrap();
    assert_eq!([text(&s, "B4"), text(&s, "C4"), text(&s, "D4")], ["East", "West", "North"]);
}

#[test]
fn captions_renamed_items_and_manual_order() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    edit_pt(&mut s, |p| {
        p.row_header_caption = Some("Area".into());
        p.col_header_caption = Some("Fruit".into());
        p.grand_total_caption = Some("Everything".into());
        p.rows[0].item_captions = vec![("East".into(), "Eastern".into())];
        p.columns[0].sort = PivotSort::None;
        p.columns[0].item_order = vec!["Plums".into(), "Pears".into()];
    });
    assert_eq!(text(&s, "A4"), "Area");
    assert_eq!(text(&s, "B3"), "Fruit");
    // Listed items first, the rest after.
    assert_eq!([text(&s, "B4"), text(&s, "C4"), text(&s, "D4"), text(&s, "E4")], ["Plums", "Pears", "Apples", "Everything"]);
    assert_eq!(text(&s, "A5"), "Eastern");
    assert_eq!(text(&s, "A8"), "Everything");
    // A renamed item sorts by its label still, and a filter still names it by its label.
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    assert_eq!(text(&s, "A5"), "Eastern");
    assert_eq!(val(&s, "E5"), Value::Number(415.0));
    // Outline layout: the field's custom name heads its column.
    edit_pt(&mut s, |p| {
        p.layout = PivotLayout::Outline;
        p.rows[0].caption = Some("Sales area".into());
        p.columns[0].caption = Some("Kind".into());
    });
    assert_eq!(text(&s, "A4"), "Sales area");
    assert_eq!(text(&s, "B3"), "Kind");
}

#[test]
fn field_subtotals_repeat_labels_and_values_row() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    assert_eq!(text(&s, "A7"), "East Total");
    // Region without subtotals although the table has them; labels repeated on every row.
    edit_pt(&mut s, |p| {
        p.rows[0].subtotal = Some(false);
        p.rows[0].repeat_labels = true;
    });
    assert_eq!(pt(&s).last_range.unwrap().height(), 1 + 9 + 1);
    assert_eq!([text(&s, "A4"), text(&s, "A5"), text(&s, "A6"), text(&s, "A7")], ["East", "East", "East", "North"]);
    assert_eq!(text(&s, "B7"), "Apples");
    // Outline repeats the labels too.
    s.execute("pivot.layout", json!({"layout": "outline"})).unwrap();
    assert_eq!([text(&s, "A4"), text(&s, "B4"), text(&s, "A5"), text(&s, "B5")], ["East", "", "East", "Apples"]);
    // Two values without a column field: the Values row (Outline/Tabular) holds the caption.
    s.execute("pivot.removeField", json!({"field": "Product"})).unwrap();
    s.execute("pivot.addField", json!({"field": "Units", "area": "values"})).unwrap();
    assert_eq!(text(&s, "B3"), "Values");
    assert_eq!([text(&s, "A4"), text(&s, "B4"), text(&s, "C4")], ["Region", "Sum of Sales", "Sum of Units"]);
    edit_pt(&mut s, |p| p.data_caption = Some("Data".into()));
    assert_eq!(text(&s, "B3"), "Data");
    edit_pt(&mut s, |p| p.show_values_row = false);
    assert_eq!([text(&s, "A3"), text(&s, "B3")], ["Region", "Sum of Sales"]);
    // Compact layout never shows it.
    edit_pt(&mut s, |p| {
        p.show_values_row = true;
        p.layout = PivotLayout::Compact;
    });
    assert_eq!([text(&s, "A3"), text(&s, "B3")], ["Row Labels", "Sum of Sales"]);
}

#[test]
fn empty_cell_caption_classic_layout_and_date_order() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Units"], "values": ["Sales"]})).unwrap();
    // East has no record with 1 unit: blank, or the "For empty cells show" caption.
    assert_eq!([text(&s, "A5"), text(&s, "B4"), text(&s, "B5")], ["East", "1", ""]);
    edit_pt(&mut s, |p| p.empty_caption = Some("0".into()));
    assert_eq!(text(&s, "B5"), "0");
    // Classic layout, one value and no column field: the value caption above the row
    // headers and a "Total" column.
    s.execute("pivot.removeField", json!({"field": "Units"})).unwrap();
    edit_pt(&mut s, |p| {
        p.layout = PivotLayout::Tabular;
        p.classic_layout = true;
    });
    assert_eq!([text(&s, "A3"), text(&s, "A4"), text(&s, "B4"), text(&s, "A5")], ["Sum of Sales", "Region", "Total", "East"]);
    assert_eq!(pt(&s).last_range.unwrap().height(), 2 + 3 + 1);
    // A field number format applies to the item labels; date items sort after text, as in
    // Excel.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "C13", "input": "NA"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    s.execute("pivot.removeField", json!({"field": "Region"})).unwrap();
    s.execute("pivot.addField", json!({"field": "Date", "area": "rows"})).unwrap();
    edit_pt(&mut s, |p| p.rows[0].number_format = Some("m/d/yyyy".into()));
    assert_eq!(text(&s, "A5"), "NA");
}

#[test]
fn only_date_formatted_cells_sort_as_dates() {
    // The two mixed columns of ClosedXML's VariousDataTypesInTableColumns, in source order.
    // Excel caches each date-formatted cell as `<d>` and each General one as `<n>`, and its
    // saved items list numbers first and dates after errors, whatever the first cell is.
    let mixed = ["", "TRUE", "14.5", "10", "ABC", "=1/0", "8/10/2020", "16:30"];
    // 1 and 1.52… are 12/31/1899 and 12/31/1899 12:30, which typed input reads as text.
    let with_num = ["1", "1.5208333333333333", "12/24/2020", "16:30", "0.1", "45000", ""];
    let order = |inputs: &[&str], dates: Option<&str>| -> Vec<String> {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "K"})).unwrap();
        s.execute("cell.set", json!({"cell": "B1", "input": "V"})).unwrap();
        for (i, input) in inputs.iter().enumerate() {
            s.execute("cell.set", json!({"cell": format!("A{}", i + 2), "input": input})).unwrap();
            s.execute("cell.set", json!({"cell": format!("B{}", i + 2), "input": "1"})).unwrap();
        }
        if let Some(range) = dates {
            s.execute("home.formatCells", json!({"range": range, "style": {"num_fmt": "m/d/yyyy h:mm"}})).unwrap();
        }
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
        s.execute("insert.pivotTable", json!({"rows": ["K"], "values": ["V"]})).unwrap();
        edit_pt(&mut s, |p| p.rows[0].sort = PivotSort::Asc);
        let wb = &s.doc().unwrap().wb;
        let si = wb.active_sheet();
        let out = crate::pivot::compute(wb, wb.sheet(si).unwrap().pivots.last().unwrap(), si).unwrap();
        out.grid.iter().skip(1).filter_map(|r| r.first().map(|c| c.value.display())).filter(|l| l != "Grand Total").collect()
    };
    // Excel's saved <items> order for each field.
    let excel_mixed = ["10", "14.5", "ABC", "TRUE", "#DIV/0!", "0.6875", "44053", "(blank)"];
    let excel_with_num = ["0.1", "45000", "0.6875", "1", "1.52083333333333", "44189", "(blank)"];
    assert_eq!(order(&mixed, None), excel_mixed);
    assert_eq!(order(&with_num, Some("A2:A3")), excel_with_num);
    // One number in a date cell and in a General cell is one item, which sorts as a number
    // whichever cell comes first.
    let number = ["5", "44053", "ABC"];
    assert_eq!(order(&["5", "8/10/2020", "44053", "ABC"], None), number);
    assert_eq!(order(&["5", "44053", "8/10/2020", "ABC"], None), number);
}

#[test]
fn a_general_cell_filtered_out_still_makes_its_number_sort_as_a_number() {
    // 3/15/2023 is 45000: the General 45000 in row 3 makes that item a number, as in Excel's
    // cache, even when the report filter leaves only the date cell.
    let order = |filter: bool| -> Vec<String> {
        let mut s = Session::new();
        s.new_workbook();
        for (cell, input) in [("A1", "K"), ("B1", "V"), ("C1", "R"), ("A2", "3/15/2023"), ("A3", "45000"), ("A4", "ABC")] {
            s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
        }
        for (row, r) in [(2, "a"), (3, "b"), (4, "a")] {
            s.execute("cell.set", json!({"cell": format!("B{row}"), "input": "1"})).unwrap();
            s.execute("cell.set", json!({"cell": format!("C{row}"), "input": r})).unwrap();
        }
        s.execute("selection.set", json!({"range": "A1"})).unwrap();
        s.execute("insert.pivotTable", json!({"rows": ["K"], "values": ["V"], "filters": ["R"]})).unwrap();
        s.execute("pivot.sort", json!({"field": "K", "order": "asc"})).unwrap();
        if filter {
            s.execute("pivot.filter", json!({"field": "R", "selected": ["a"]})).unwrap();
        }
        let wb = &s.doc().unwrap().wb;
        let si = wb.active_sheet();
        let out = crate::pivot::compute(wb, wb.sheet(si).unwrap().pivots.last().unwrap(), si).unwrap();
        out.grid.iter().filter_map(|r| r.first().map(|c| c.value.display())).filter(|l| l == "45000" || l == "ABC").collect()
    };
    assert_eq!(order(false), ["45000", "ABC"]);
    assert_eq!(order(true), ["45000", "ABC"]);
}

#[test]
fn dates_sort_after_logicals_and_errors() {
    // Excel's ascending item order: numbers, text, logicals, errors, dates, blanks.
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "C11", "input": "TRUE"})).unwrap();
    s.execute("cell.set", json!({"cell": "C12", "input": "=1/0"})).unwrap();
    s.execute("cell.set", json!({"cell": "C13", "input": "NA"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"]})).unwrap();
    edit_pt(&mut s, |p| {
        p.rows[0].number_format = None;
        p.rows[0].sort = PivotSort::Asc;
    });
    let p = pt(&s);
    let r = p.last_range.unwrap();
    let labels: Vec<String> = (r.start.row + 1..=r.end.row).map(|row| text(&s, &CellRef::new(row, r.start.col).a1())).collect();
    // 46037 is 1/15/2026, the first date.
    assert_eq!(labels[..4], ["NA", "TRUE", "#DIV/0!", "46037"]);
}

fn pivot_chart(s: &Session) -> gridcraft_model::Chart {
    s.doc().unwrap().wb.active().unwrap().charts.last().unwrap().clone()
}
fn plotted(s: &Session) -> Vec<Vec<Option<f64>>> {
    let d = s.doc().unwrap();
    let data = gridcraft_chart::resolve(&d.wb, d.wb.active_sheet(), &pivot_chart(s));
    data.series.into_iter().map(|x| x.values).collect()
}

#[test]
fn pivot_chart_follows_the_report() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    // Any cell of the report picks the PivotTable.
    let r = s.execute("insert.pivotChart", json!({"pivot": "B5"})).unwrap();
    assert_eq!(r["series"], 1);
    let ch = pivot_chart(&s);
    assert_eq!(ch.pivot.as_deref(), Some("Sheet2!PivotTable1"));
    assert_eq!(ch.title.as_deref(), Some("Total"));
    assert_eq!(ch.series[0].values, "Sheet2!$B$4:$B$6");
    assert_eq!(ch.series[0].categories.as_deref(), Some("Sheet2!$A$4:$A$6"));
    assert_eq!(plotted(&s), vec![vec![Some(415.0), Some(135.0), Some(285.0)]]);
    // A column field: one series per item, rebound by the layout change.
    s.execute("pivot.addField", json!({"field": "Product", "area": "columns"})).unwrap();
    let ch = pivot_chart(&s);
    let names: Vec<_> = ch.series.iter().map(|x| x.name.clone().unwrap_or_default()).collect();
    assert_eq!(names, vec!["\"Apples\"", "\"Pears\"", "\"Plums\""]);
    assert_eq!(plotted(&s)[0], vec![Some(160.0), Some(90.0), Some(170.0)]);
    // Nested row fields: the innermost items only, as a union around the parent rows.
    s.execute("pivot.moveField", json!({"field": "Product", "from": "columns", "to": "rows"})).unwrap();
    let ch = pivot_chart(&s);
    assert!(ch.series[0].values.starts_with('('), "{}", ch.series[0].values);
    let pts = plotted(&s);
    assert_eq!(pts[0].len(), 9);
    assert_eq!(pts[0].iter().flatten().sum::<f64>(), TOTAL);
    // Survives an XLSX round trip as a PivotChart.
    let wb = (*s.doc().unwrap().wb).clone();
    let (back, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
    let si = back.sheet_index("Sheet2").unwrap();
    assert_eq!(back.sheet(si).unwrap().charts.first().and_then(|c| c.pivot.clone()).as_deref(), Some("Sheet2!PivotTable1"));
    // Renaming the sheet keeps the link; a refresh still rebinds.
    s.execute("sheet.rename", json!({"name": "Report"})).unwrap();
    assert_eq!(pivot_chart(&s).pivot.as_deref(), Some("Report!PivotTable1"));
    s.execute("pivot.moveField", json!({"field": "Region", "from": "rows", "to": "columns"})).unwrap();
    assert_eq!(pivot_chart(&s).series.len(), 3);
    // Deleting the PivotTable leaves an ordinary chart.
    s.execute("pivot.delete", json!({})).unwrap();
    assert_eq!(pivot_chart(&s).pivot, None);
}

#[test]
fn pivot_chart_errors() {
    let mut s = sales();
    // No PivotTable yet.
    assert!(s.execute("insert.pivotChart", json!({})).is_err());
    s.execute("insert.pivotTable", json!({"rows": ["Region"]})).unwrap();
    // Not in the report; no values.
    assert!(s.execute("insert.pivotChart", json!({"pivot": "Z99"})).is_err());
    assert!(s.execute("insert.pivotChart", json!({})).unwrap_err().to_string().contains("Values"));
    s.execute("pivot.addField", json!({"field": "Units", "area": "values"})).unwrap();
    let r = s.execute("insert.pivotChart", json!({"pivot": "PivotTable1", "type": "line", "title": null})).unwrap();
    assert_eq!(r["series"], 1);
    let ch = pivot_chart(&s);
    assert_eq!(ch.kind, gridcraft_model::ChartKind::Line);
    assert_eq!(ch.title, None);
    // One undo step.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.doc().unwrap().wb.active().unwrap().charts.is_empty());
}

#[test]
fn a_calculated_field_cannot_use_distinct_count() {
    // It is computed from the report's totals, so there are no per-record values to count.
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Price", "formula": "Sales / Units"})).unwrap();
    assert_eq!(text(&s, "C3"), "Sum of Price");
    let before = s.doc().unwrap().wb.clone();
    let set = s.execute("pivot.valueSettings", json!({"field": "Price", "func": "distinctCount"}));
    assert!(set.unwrap_err().to_string().contains("Distinct Count"));
    assert_eq!(s.doc().unwrap().wb, before, "a rejected change leaves the workbook as it was");
    s.execute("pivot.removeField", json!({"field": "Price", "area": "values"})).unwrap();
    let add = s.execute("pivot.addField", json!({"field": "Price", "area": "values", "func": "distinctCount"}));
    assert!(add.unwrap_err().to_string().contains("Distinct Count"));
    // A source field still takes it, and a calculated field keeps its other functions.
    s.execute("pivot.addField", json!({"field": "Product", "area": "values", "func": "distinctCount"})).unwrap();
    s.execute("pivot.addField", json!({"field": "Price", "area": "values", "func": "sum"})).unwrap();
}

#[test]
fn calculated_field_on_sums() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    let r = s.execute("pivot.calculatedField", json!({"name": "Price", "formula": "Sales / Units"})).unwrap();
    assert_eq!(r["formula"], "=Sales / Units");
    assert_eq!(text(&s, "C3"), "Sum of Price");
    assert_eq!(text(&s, "A4"), "East");
    // East: 415 sales over 33 units, from the sums.
    let east = val(&s, "C4").as_f64().unwrap();
    assert!((east - 415.0 / 33.0).abs() < 1e-12, "{east}");
    // The grand total applies the formula to the totals, not the sum of the rows.
    assert_eq!(text(&s, "A7"), "Grand Total");
    let total = val(&s, "C7").as_f64().unwrap();
    assert!((total - TOTAL / 72.0).abs() < 1e-12, "{total}");
    // Quoted names, functions, and another calculated field.
    s.execute("pivot.calculatedField", json!({"name": "Rounded", "formula": "=ROUND('Price'*2,1)"})).unwrap();
    assert_eq!(val(&s, "D4"), Value::Number(25.2));
    // Division by zero shows #DIV/0! cells.
    s.execute("pivot.calculatedField", json!({"name": "Zero", "formula": "=Sales/(Units-Units)"})).unwrap();
    assert_eq!(val(&s, "E4"), Value::Error(CellError::Div0));
    // Change and delete.
    s.execute("pivot.calculatedField", json!({"name": "zero", "formula": "=Units*2"})).unwrap();
    assert_eq!(val(&s, "E4"), Value::Number(66.0));
    s.execute("pivot.calculatedField", json!({"name": "Zero", "delete": true})).unwrap();
    assert_eq!(pt(&s).values.len(), 3);
    assert_eq!(pt(&s).calculated_fields.len(), 2);
    // The field list shows calculated fields.
    let f = s.execute("pivot.fields", json!({})).unwrap();
    assert!(f["fields"].as_array().unwrap().iter().any(|x| x["name"] == "Price" && x["calculated"] == true));
}

#[test]
fn calculated_field_errors() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    let mut e = |p: Json| s.execute("pivot.calculatedField", p).unwrap_err().to_string();
    assert!(e(json!({"name": "Bad", "formula": "=Sales*Bogus"})).contains("Bogus"));
    assert!(e(json!({"name": "Bad", "formula": "=A1*2"})).contains("cells"));
    assert!(e(json!({"name": "Bad", "formula": "=Sales*("})).contains("error"));
    assert!(e(json!({"name": "Sales", "formula": "=1"})).contains("already exists"));
    assert!(e(json!({"name": "Bad"})).contains("formula"));
    assert!(e(json!({"name": "Gone", "delete": true})).contains("Gone"));
    // A cycle is refused.
    s.execute("pivot.calculatedField", json!({"name": "X", "formula": "=Sales"})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Y", "formula": "=X*2"})).unwrap();
    let err = s.execute("pivot.calculatedField", json!({"name": "X", "formula": "=Y"})).unwrap_err().to_string();
    assert!(err.contains("itself"), "{err}");
    // Calculated fields only go to Values; nothing changed after the failures.
    assert!(s.execute("pivot.addField", json!({"field": "Y", "area": "rows"})).is_err());
    assert_eq!(pt(&s).calculated_fields.len(), 2);
    // Without a name the dialog opens (menu-style call).
    s.ui_requests.clear();
    s.execute("pivot.calculatedField", json!({})).unwrap();
    assert!(matches!(s.ui_requests.last(), Some(crate::UiRequest::Dialog(n, _)) if n == "pivotCalculatedField"));
}

#[test]
fn getpivotdata_reads_the_report() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    assert_eq!(pt(&s).last_range.unwrap().a1(), "A3:E8");
    let gp = |s: &mut Session, cell: &str, f: &str| {
        s.execute("cell.set", json!({"cell": cell, "input": f})).unwrap();
        val(s, cell)
    };
    // Item cell, row subtotal by any cell of the report, grand total; names and items ignore case.
    assert_eq!(gp(&mut s, "G1", r#"=GETPIVOTDATA("Sales",$A$3,"Region","East","Product","Apples")"#), Value::Number(160.0));
    assert_eq!(gp(&mut s, "G2", r#"=GETPIVOTDATA("sales",C6,"region","east")"#), Value::Number(415.0));
    assert_eq!(gp(&mut s, "G3", r#"=GETPIVOTDATA("Sum of Sales",A3)"#), Value::Number(TOTAL));
    assert_eq!(gp(&mut s, "G4", r#"=GETPIVOTDATA("Sales",A3,"Product","Pears")"#), Value::Number(270.0));
    // Not in the report: unknown item, value field, field, unpaired field, not a pivot cell.
    for (cell, f) in [
        ("H1", r#"=GETPIVOTDATA("Sales",A3,"Region","South")"#),
        ("H2", r#"=GETPIVOTDATA("Units",A3)"#),
        ("H3", r#"=GETPIVOTDATA("Sales",A3,"Date","East")"#),
        ("H4", r#"=GETPIVOTDATA("Sales",A3,"Region")"#),
        ("H5", r#"=GETPIVOTDATA("Sales",Z99)"#),
        ("H6", r#"=GETPIVOTDATA("Sales",A3,"Region","East","Region","East")"#),
    ] {
        assert_eq!(gp(&mut s, cell, f), Value::Error(CellError::Ref), "{f}");
    }
    assert_eq!(gp(&mut s, "H7", r#"=GETPIVOTDATA("Sales",A3,"Region",NA())"#), Value::Error(CellError::NA));
    // A refresh after a source edit updates the result.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "E2", "input": "1100"})).unwrap();
    s.execute("data.refreshAll", json!({})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(val(&s, "G1"), Value::Number(1160.0));
    // Opening a saved workbook starts without an index (files don't carry it); the first
    // GETPIVOTDATA builds it.
    let wb = (*s.doc().unwrap().wb).clone();
    let (back, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
    s.add_document(crate::DocState::new(back, None, "Reopened".into(), crate::Trust::FromFile));
    let d = s.doc().unwrap();
    let sh = d.wb.sheet(d.wb.sheet_index("Sheet2").unwrap()).unwrap();
    assert!(sh.pivots.first().unwrap().data_index.cells().is_some());
    assert_eq!(sh.value(CellRef::parse("G1").unwrap()), Value::Number(1160.0));
    assert_eq!(sh.value(CellRef::parse("G3").unwrap()), Value::Number(TOTAL + 1000.0));
}

#[test]
fn getpivotdata_reads_the_displayed_report() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    // The source changes after the last refresh, then the workbook is saved and reopened:
    // the report still shows the old numbers, and the formula agrees with the cell.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "E2", "input": "1100"})).unwrap();
    let wb = (*s.doc().unwrap().wb).clone();
    let (back, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
    s.add_document(crate::DocState::new(back, None, "Reopened".into(), crate::Trust::FromFile));
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    let r = pt(&s).last_range.unwrap();
    let at = r.start.a1();
    s.execute("cell.set", json!({"cell": "K1", "input": format!(r#"=GETPIVOTDATA("Sales",{at},"Region","East")"#)})).unwrap();
    let d = s.doc().unwrap();
    let sh = d.wb.sheet(d.wb.sheet_index("Sheet2").unwrap()).unwrap();
    let east = (r.start.row..=r.end.row).find(|row| sh.value(CellRef::new(*row, r.start.col)) == Value::Text("East".into())).unwrap();
    let shown = sh.value(CellRef::new(east, r.start.col + 1));
    assert_eq!(shown, Value::Number(415.0));
    assert_eq!(val(&s, "K1"), shown);
}

/// `=GETPIVOTDATA("Sales", <report>, "Region", <region>)` evaluated in K1 of the active sheet.
fn region_sales(s: &mut Session, region: &str) -> Value {
    let at = pt(s).last_range.unwrap().start.a1();
    s.execute("cell.set", json!({"cell": "K1", "input": format!(r#"=GETPIVOTDATA("Sales",{at},"Region","{region}")"#)})).unwrap();
    val(s, "K1")
}

fn reopen(s: &mut Session) {
    let wb = (*s.doc().unwrap().wb).clone();
    let (back, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
    s.add_document(crate::DocState::new(back, None, "Reopened".into(), crate::Trust::FromFile));
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
}

#[test]
fn getpivotdata_retries_after_a_lazy_build_fails() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    // A pivot read from a file carries no index, so the first lookup builds one.
    reopen(&mut s);
    assert!(pt(&s).data_index.cells().is_none());
    // The source changes, so the stored report is stale and the build finds nothing to trust.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Alpha"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(region_sales(&mut s, "West"), Value::Error(CellError::Ref));
    assert!(pt(&s).data_index.cells().is_none(), "the failed build caches nothing");
    // The lookup is shared, so caching a failure as an empty index would leave every state that
    // shares it #REF! for good. Restoring the source lets the lookup build the index.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "East"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(region_sales(&mut s, "West"), Value::Number(285.0));
    assert!(pt(&s).data_index.cells().is_some(), "the lookup rebuilt after the failure");
}

#[test]
fn getpivotdata_saves_no_index_when_the_lazy_build_fails() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    // A pivot read from a file carries no index; a source change makes the stored report stale,
    // so the lazy build fails and the lookup is #REF!.
    reopen(&mut s);
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Alpha"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(region_sales(&mut s, "West"), Value::Error(CellError::Ref));
    // Saving the pivot must save no index at all, not an empty one: restoring an empty index
    // would count as an index and lock the reopened pivot against the lazy rebuild.
    let json = serde_json::to_string(&pt(&s)).unwrap();
    let back: gridcraft_model::PivotTable = serde_json::from_str(&json).unwrap();
    assert!(json.contains(r#""data_index":null"#), "a failed build saves no index, not an empty one: {json}");
    assert!(back.data_index.cells().is_none());
    // Reopened with the source restored, the lookup builds the index.
    reopen(&mut s);
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "East"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(region_sales(&mut s, "West"), Value::Number(285.0));
}

#[test]
fn getpivotdata_keeps_the_refreshed_report_when_an_item_is_inserted() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    assert_eq!(region_sales(&mut s, "West"), Value::Number(285.0));
    // A new first item in the source shifts every item of a fresh report down one row.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Alpha"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    // Before the refresh the report still shows the old items, and the lookup reads that report.
    assert_eq!(region_sales(&mut s, "West"), Value::Number(285.0));
    assert_eq!(region_sales(&mut s, "Alpha"), Value::Error(CellError::Ref));
    // Saved and reopened unrefreshed: the stored report no longer matches the source, so there
    // is no index to trust (never the shifted cell of a different item).
    reopen(&mut s);
    assert_eq!(region_sales(&mut s, "West"), Value::Error(CellError::Ref));
    assert_eq!(region_sales(&mut s, "Alpha"), Value::Error(CellError::Ref));
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("data.refreshAll", json!({})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(region_sales(&mut s, "Alpha"), Value::Number(100.0));
    assert_eq!(region_sales(&mut s, "West"), Value::Number(285.0));
    assert_eq!(region_sales(&mut s, "East"), Value::Number(315.0));
}

#[test]
fn getpivotdata_keeps_the_refreshed_report_when_an_item_is_removed() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    assert_eq!(region_sales(&mut s, "North"), Value::Number(135.0));
    // Every North row becomes East: North leaves the report and West moves up.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    for a in ["A5", "A8", "A11"] {
        s.execute("cell.set", json!({"cell": a, "input": "East"})).unwrap();
    }
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(region_sales(&mut s, "North"), Value::Number(135.0));
    assert_eq!(region_sales(&mut s, "West"), Value::Number(285.0));
    reopen(&mut s);
    assert_eq!(region_sales(&mut s, "North"), Value::Error(CellError::Ref));
    assert_eq!(region_sales(&mut s, "West"), Value::Error(CellError::Ref));
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("data.refreshAll", json!({})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(region_sales(&mut s, "North"), Value::Error(CellError::Ref));
    assert_eq!(region_sales(&mut s, "West"), Value::Number(285.0));
    assert_eq!(region_sales(&mut s, "East"), Value::Number(550.0));
}

#[test]
fn getpivotdata_index_is_saved_with_an_unrefreshed_pivot() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "A2", "input": "Alpha"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    // The pivot as saved by the native format: the index of the stored report comes back with
    // it even though the source has since changed.
    let back: gridcraft_model::PivotTable = serde_json::from_str(&serde_json::to_string(&pt(&s)).unwrap()).unwrap();
    let cells = back.data_index.cells().expect("index saved");
    assert_eq!(cells, pt(&s).data_index.cells().unwrap());
    let west = back.data_index.find("Sales", &[("Region", "West", false)]).unwrap();
    assert_eq!((west.row, west.col), (3, 1));
}

#[test]
fn getpivotdata_calculated_field() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Double", "formula": "=Sales*2"})).unwrap();
    let at = pt(&s).last_range.unwrap().start.a1();
    s.execute("cell.set", json!({"cell": "K1", "input": format!(r#"=GETPIVOTDATA("Double",{at},"Region","East")"#)})).unwrap();
    assert_eq!(val(&s, "K1"), Value::Number(830.0));
}

#[test]
fn getpivotdata_report_filter() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
    let at = pt(&s).last_range.unwrap().start.a1();
    let f = |region: &str| format!(r#"=GETPIVOTDATA("Sales",{at},"Region","{region}","Product","Apples")"#);
    // With every item shown, naming the filter field is #REF!.
    s.execute("cell.set", json!({"cell": "H1", "input": f("East")})).unwrap();
    assert_eq!(val(&s, "H1"), Value::Error(CellError::Ref));
    // Filtered to East: East matches, West doesn't.
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    assert_eq!(val(&s, "H1"), Value::Number(160.0));
    s.execute("cell.set", json!({"cell": "H2", "input": f("West")})).unwrap();
    assert_eq!(val(&s, "H2"), Value::Error(CellError::Ref));
}

/// A workbook whose Sheet1 holds `rows` under a two-column header, with a PivotTable of the first
/// column (rows) and the second (sum) on a new sheet; the report sheet is active.
fn two_column_pivot(header: [&str; 2], rows: Json, key_format: Option<&str>) -> Session {
    let mut s = Session::new();
    s.new_workbook();
    let n = rows.as_array().map_or(0, Vec::len);
    let mut all = vec![json!(header)];
    all.extend(rows.as_array().cloned().unwrap_or_default());
    s.execute("range.setValues", json!({"range": "A1", "values": all})).unwrap();
    if let Some(code) = key_format {
        s.execute("home.numberFormat", json!({"range": format!("A2:A{}", n + 1), "code": code})).unwrap();
    }
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": [header[0]], "values": [header[1]]})).unwrap();
    s
}

/// The value the report shows for the row item `label` (one value field: the report's last column).
fn shown(s: &Session, label: &str) -> Value {
    let r = pt(s).last_range.unwrap();
    let row = find_row(s, label).unwrap_or_else(|| panic!("{label} is not in the report"));
    val(s, &CellRef::new(row, r.end.col).a1())
}

/// Inserting or deleting rows or columns inside a report keeps GETPIVOTDATA on the requested
/// item's cell (the stored offsets move with the cells), and undo restores the old positions.
#[test]
fn getpivotdata_follows_rows_and_columns_edited_inside_the_report() {
    let mut s = two_column_pivot(["Region", "Sales"], json!([["East", 10], ["North", 30], ["West", 20]]), None);
    assert_eq!(pt(&s).last_range.unwrap().start.a1(), "A3");
    s.execute("cell.set", json!({"cell": "K1", "input": r#"=GETPIVOTDATA("Sales",A3,"Region","West")"#})).unwrap();
    s.execute("cell.set", json!({"cell": "K2", "input": r#"=GETPIVOTDATA("Sales",A3,"Region","North")"#})).unwrap();
    assert_eq!(val(&s, "K1"), Value::Number(20.0));
    // Row 4 splits the header from the items; column B splits the labels from the values (and
    // moves the formulas from K to L).
    for (cmd, lines, west, north) in
        [("home.insertRows", json!({"rows": "4:4"}), "K1", "K2"), ("home.insertColumns", json!({"cols": "B:B"}), "L1", "L2")]
    {
        s.execute(cmd, lines).unwrap();
        assert_eq!(shown(&s, "West"), Value::Number(20.0), "{cmd}");
        assert_eq!(val(&s, west), shown(&s, "West"), "{cmd}");
        assert_eq!(val(&s, north), shown(&s, "North"), "{cmd}");
    }
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(pt(&s).last_range.unwrap().start.a1(), "A3");
    s.execute("cell.set", json!({"cell": "K3", "input": r#"=GETPIVOTDATA("Sales",A3,"Region","West")"#})).unwrap();
    assert_eq!(val(&s, "K3"), Value::Number(20.0), "undo restores the report and its offsets");
    // Deleting North's row moves West up; North itself is gone from the report.
    let north = find_row(&s, "North").unwrap() + 1;
    s.execute("home.deleteRows", json!({"rows": format!("{north}:{north}")})).unwrap();
    s.execute("cell.set", json!({"cell": "K1", "input": r#"=GETPIVOTDATA("Sales",A3,"Region","West")"#})).unwrap();
    s.execute("cell.set", json!({"cell": "K2", "input": r#"=GETPIVOTDATA("Sales",A3,"Region","North")"#})).unwrap();
    assert_eq!(val(&s, "K1"), Value::Number(20.0));
    assert_eq!(val(&s, "K1"), shown(&s, "West"));
    assert_eq!(val(&s, "K2"), Value::Error(CellError::Ref), "a deleted value cell is not read from its old place");
    // Lines inserted at the report's first line move the whole report, and lines right after it
    // leave it alone; either way the offsets stay what they were.
    for rows in ["3:3", "9:9"] {
        s.execute("home.insertRows", json!({"rows": rows})).unwrap();
        let at = pt(&s).last_range.unwrap().start.a1();
        s.execute("cell.set", json!({"cell": "K1", "input": format!(r#"=GETPIVOTDATA("Sales",{at},"Region","West")"#)})).unwrap();
        assert_eq!(val(&s, "K1"), Value::Number(20.0), "insert {rows}");
        assert_eq!(val(&s, "K1"), shown(&s, "West"), "insert {rows}");
    }
}

/// Numeric items whose formatted labels collide are told apart by value: an exact numeric item
/// wins over another item that only displays the same label.
#[test]
fn getpivotdata_matches_numeric_items_by_value_before_label() {
    let mut s = two_column_pivot(["Key", "Sales"], json!([[0.6, 10], [1, 20]]), Some("0"));
    let p = pt(&s);
    let labels: Vec<&str> = p.data_index.cells().unwrap().iter().flat_map(|c| c.items.iter().map(|i| i.label.as_str())).collect();
    assert_eq!(labels, ["1", "1"], "both items display as 1");
    s.execute("cell.set", json!({"cell": "K1", "input": r#"=GETPIVOTDATA("Sales",A3,"Key",1)"#})).unwrap();
    s.execute("cell.set", json!({"cell": "K2", "input": r#"=GETPIVOTDATA("Sales",A3,"Key",0.6)"#})).unwrap();
    assert_eq!(val(&s, "K1"), Value::Number(20.0));
    assert_eq!(val(&s, "K2"), Value::Number(10.0));
    // Dates: two times on one day show the same date label; each serial finds its own item.
    let mut s = two_column_pivot(["Day", "Sales"], json!([[45000, 10], [45000.25, 20]]), Some("m/d/yyyy"));
    s.execute("cell.set", json!({"cell": "K1", "input": r#"=GETPIVOTDATA("Sales",A3,"Day",45000.25)"#})).unwrap();
    s.execute("cell.set", json!({"cell": "K2", "input": r#"=GETPIVOTDATA("Sales",A3,"Day",45000)"#})).unwrap();
    assert_eq!(val(&s, "K1"), Value::Number(20.0));
    assert_eq!(val(&s, "K2"), Value::Number(10.0));
}

#[test]
fn replace_protection() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "H5", "input": "keep me"})).unwrap();
    let e = s.execute("insert.pivotTable", json!({"destination": "Sheet1!H3", "rows": ["Region"], "values": ["Sales"]})).unwrap_err().to_string();
    assert!(e.contains("There's already data in Sheet1!H5"), "{e}");
    assert_eq!(s.doc().unwrap().wb.active().unwrap().pivots.len(), 0);
    // Growing into data is refused as well.
    s.execute("insert.pivotTable", json!({"destination": "Sheet1!H3"})).unwrap();
    assert!(s.execute("pivot.addField", json!({"field": "Region", "area": "rows"})).is_err());
    s.execute("pivot.addField", json!({"field": "Region", "area": "rows", "replace": true})).unwrap();
    assert_eq!(text(&s, "H5"), "North");
    // A second pivot can't overlap the first.
    let e = s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "destination": "Sheet1!H6", "replace": true})).unwrap_err().to_string();
    assert!(e.contains("overlap"), "{e}");
    // Nor its own source data.
    let e = s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "destination": "Sheet1!B2", "replace": true})).unwrap_err().to_string();
    assert!(e.contains("source data"), "{e}");
    // Delete clears the report.
    s.execute("pivot.delete", json!({})).unwrap();
    assert_eq!(val(&s, "H5"), Value::Empty);
    assert!(s.doc().unwrap().wb.active().unwrap().pivots.is_empty());
}

#[test]
fn fields_move_remove_sort() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    let f = s.execute("pivot.fields", json!({})).unwrap();
    assert_eq!(f["fields"][0]["name"], "Region");
    assert_eq!(f["fields"][0]["samples"], json!(["East", "North", "West"]));
    assert_eq!(f["fields"][0]["areas"], json!(["rows"]));
    assert_eq!(f["fields"][4]["numeric"], true);
    assert_eq!(f["fields"][2]["date"], true);
    s.execute("pivot.sort", json!({"field": "Region", "order": "desc"})).unwrap();
    assert_eq!(text(&s, "A4"), "West");
    s.execute("pivot.moveField", json!({"field": "Region", "from": "rows", "to": "columns"})).unwrap();
    assert_eq!(text(&s, "B4"), "West");
    s.execute("pivot.removeField", json!({"field": "Sum of Sales", "area": "values"})).unwrap();
    s.execute("pivot.addField", json!({"field": "Region", "area": "values"})).unwrap();
    assert_eq!(pt(&s).values[0].name, "Count of Region");
    s.execute("pivot.removeField", json!({"field": "Region"})).unwrap();
    assert!(pt(&s).columns.is_empty() && pt(&s).values.is_empty());
    assert!(text(&s, "A3").contains("choose fields"));
    assert!(s.execute("pivot.addField", json!({"field": "Nope", "area": "rows"})).is_err());
    assert!(s.execute("pivot.addField", json!({"field": "Region", "area": "sideways"})).is_err());
}

#[test]
fn recommended_and_summarize() {
    let mut s = sales();
    let r = s.execute("insert.recommendedPivotTables", json!({})).unwrap();
    let list = r["suggestions"].as_array().unwrap();
    assert!((3..=5).contains(&list.len()), "{list:?}");
    assert_eq!(list[0]["title"], "Sum of Units by Region");
    let r = s.execute("insert.recommendedPivotTables", json!({"apply": 2})).unwrap();
    assert!(r["suggestion"]["title"].as_str().unwrap().contains("quarters"));
    assert_eq!(text(&s, "A4"), "Qtr1");
    // From a table.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("insert.table", json!({"range": "A1:E13"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    let r = s.execute("table.summarizePivot", json!({"rows": ["Product"], "values": ["Units"]})).unwrap();
    assert_eq!(pt(&s).source, "Table1");
    assert_eq!(r["name"], "PivotTable2");
    let gt = find_row(&s, "Grand Total").unwrap();
    assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(72.0));
}

#[test]
fn repeated_headers_get_numbered_names() {
    let mut s = Session::new();
    s.new_workbook();
    let mut header = vec![json!("a"), json!("A"), json!("a"), json!("a2")];
    // Thousands of equal headers (a row of check boxes, say) used to take cubic time; this would
    // hang rather than fail a wall-clock check, which is flaky on a loaded machine.
    header.extend((0..3000).map(|_| json!("x")));
    let data: Vec<_> = (0..header.len()).map(|i| json!(i)).collect();
    let end = CellRef::new(1, header.len() as u32 - 1).a1();
    s.execute("range.setValues", json!({"range": "A1", "values": [header, data]})).unwrap();
    let src = crate::pivot::read_source(&s.doc().unwrap().wb, &format!("Sheet1!A1:{end}"), 0).unwrap();
    assert_eq!(&src.headers[..4], ["a", "A2", "a3", "a22"]);
    assert_eq!(src.headers[4], "x");
    assert_eq!(src.headers.last().map(String::as_str), Some("x3000"));
    // Table column names follow the same rule.
    s.execute("insert.table", json!({"range": format!("A1:{end}"), "header": true})).unwrap();
    let text = |s: &mut Session, a: &str| s.execute("cell.get", json!({"cell": a})).unwrap()["text"].clone();
    assert_eq!(text(&mut s, "B1"), "A2");
    let last = CellRef::new(0, 3003).a1();
    assert_eq!(text(&mut s, &last), "x3000");
}

#[test]
fn hostile_params_never_panic() {
    let mut s = sales();
    let cmds = [
        "insert.pivotTable",
        "insert.recommendedPivotTables",
        "table.summarizePivot",
        "pivot.fields",
        "pivot.fieldList",
        "pivot.list",
        "pivot.addField",
        "pivot.removeField",
        "pivot.moveField",
        "pivot.valueSettings",
        "pivot.filter",
        "pivot.sort",
        "pivot.group",
        "pivot.collapse",
        "pivot.layout",
        "pivot.grandTotals",
        "pivot.subtotals",
        "pivot.reportLayout",
        "pivot.style",
        "pivot.refresh",
        "data.refreshAll",
        "pivot.changeSource",
        "pivot.delete",
        "pivot.showDetails",
        "pivot.options",
    ];
    let hostile = [
        json!({}),
        json!([1, 2]),
        json!({"pivot": 99999, "field": 5, "area": null}),
        json!({"source": "ZZZ999999999", "destination": "Nope!A1"}),
        json!({"source": "A:XFD", "destination": "Sheet1!XFD1048576"}),
        json!({"source": "''!A1", "destination": "!"}),
        json!({"field": "Sales", "area": "values", "func": "bogus", "position": 4294967295u64}),
        json!({"field": "Region", "selected": [null, 1, {"x": 1}], "index": 18446744073709551615u64, "showAs": "rank"}),
        json!({"apply": 99, "layout": "weird", "subtotals": 7, "style": "x".repeat(500)}),
        json!({"field": "Date", "by": "decades", "items": "nope", "order": "sideways"}),
    ];
    for c in cmds {
        for h in &hostile {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let spec = crate::find_command(c).unwrap();
                let _ = (spec.run)(&mut s, h);
            }));
            assert!(r.is_ok(), "{c} panicked on {h}");
        }
    }
    // And on a real pivot.
    s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "rows": ["Region"], "values": ["Sales"]})).unwrap();
    for c in cmds {
        for h in &hostile {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let spec = crate::find_command(c).unwrap();
                let _ = (spec.run)(&mut s, h);
            }));
            assert!(r.is_ok(), "{c} panicked on {h}");
        }
    }
}

#[test]
fn empty_header_is_refused() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["Region", ""], ["East", 1]]})).unwrap();
    let e = s.execute("insert.pivotTable", json!({"source": "A1:B2"})).unwrap_err().to_string();
    assert!(e.contains("field name is not valid"), "{e}");
    assert!(s.execute("insert.pivotTable", json!({"source": "NoSuchTable"})).is_err());
}

#[test]
fn too_many_value_columns_are_refused() {
    let mut s = Session::new();
    s.new_workbook();
    let mut records: Vec<Json> = vec![json!(["Cat", "Sub", "V"])];
    for r in 0..64 {
        for c in 0..64 {
            records.push(json!([format!("c{r}"), format!("s{c}"), 1.0]));
        }
    }
    s.execute("range.setValues", json!({"range": "A1", "values": records})).unwrap();
    let req = |n: usize| {
        let values: Vec<Json> = (0..n).map(|_| json!({"field": "V"})).collect();
        json!({"source": "A1:C4097", "rows": ["Cat"], "columns": ["Sub"], "values": values})
    };
    let e = s.execute("insert.pivotTable", req(256)).unwrap_err().to_string();
    assert!(e.contains("will not fit"), "{e}");
    assert!(s.execute("insert.pivotTable", req(128)).is_ok());
}

#[test]
fn collapsed_column_items_count_as_one_column_group() {
    let mut s = Session::new();
    s.new_workbook();
    let mut records: Vec<Json> = vec![json!(["Grp", "Sub", "V"])];
    records.extend((0..64).map(|c| json!(["g", format!("s{c}"), 1.0])));
    s.execute("range.setValues", json!({"range": "A1", "values": records})).unwrap();
    let values: Vec<Json> = (0..256).map(|_| json!({"field": "V"})).collect();
    s.execute("insert.pivotTable", json!({"source": "A1:C65", "columns": ["Grp"], "values": values})).unwrap();
    s.execute("pivot.collapse", json!({"field": "Grp"})).unwrap();
    // 64 Sub items under the collapsed "g" show as one column group: (1 + grand total) x 256 columns fit.
    s.execute("pivot.addField", json!({"field": "Sub", "area": "columns"})).unwrap();
}

#[test]
fn design_subtotals_replace_field_settings() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    // A file's field can override the table default (`defaultSubtotal`).
    edit_pt(&mut s, |p| {
        p.subtotals = false;
        p.rows[0].subtotal = Some(true);
    });
    assert!(find_row(&s, "East Total").is_some());
    // Design › Subtotals › Do Not Show Subtotals applies to every field.
    s.execute("pivot.subtotals", json!({"mode": "none"})).unwrap();
    assert!(pt(&s).rows.iter().all(|f| f.subtotal.is_none()));
    assert_eq!(find_row(&s, "East Total"), None);
    edit_pt(&mut s, |p| p.rows[0].subtotal = Some(false));
    s.execute("pivot.subtotals", json!({"mode": "bottom"})).unwrap();
    assert!(find_row(&s, "East Total").is_some());
}

#[test]
fn numeric_empty_cell_caption_is_a_number() {
    let mut s = sales();
    s.execute("range.setValues", json!({"range": "A14", "values": [["South", "Kiwis", "1/1/2026", 1, 10]]})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    // South has no Apples: the cell shows the caption, a number as Excel stores it.
    edit_pt(&mut s, |p| p.empty_caption = Some("0".into()));
    let south = find_row(&s, "South").unwrap();
    let apples = CellRef::new(south, 1);
    assert_eq!(text(&s, &CellRef::new(south - 3, 1).a1()), "Apples");
    assert_eq!(val(&s, &apples.a1()), Value::Number(0.0));
    edit_pt(&mut s, |p| p.empty_caption = Some("n/a".into()));
    assert_eq!(val(&s, &apples.a1()), Value::text("n/a"));
}

/// Saves the active workbook, applies `patch` to the first PivotTable part and reopens it.
fn reopen_patched(s: &Session, patch: impl Fn(&str) -> String) -> (gridcraft_model::Workbook, Vec<String>) {
    reopen_patched_part(s, "xl/pivotTables/pivotTable1.xml", patch)
}
/// Saves the active workbook, applies `patch` to `part` and reopens it.
fn reopen_patched_part(s: &Session, part: &str, patch: impl Fn(&str) -> String) -> (gridcraft_model::Workbook, Vec<String>) {
    use std::io::{Read, Write};
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "t.xlsx").unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().unwrap().to_string();
        let mut b = Vec::new();
        f.read_to_end(&mut b).unwrap();
        if name == part {
            b = patch(&String::from_utf8(b).unwrap()).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&b).unwrap();
    }
    let bytes = out.finish().unwrap().into_inner();
    crate::io::open_bytes("t.xlsx", &bytes).unwrap()
}

#[test]
fn sort_by_value_in_a_column_keeps_its_scope() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.sort", json!({"field": "Region", "order": "desc", "by": "Sum of Sales"})).unwrap();
    // Excel's "sort by the values in the Apples column": the scope also names Product's
    // item 0 (Apples). Apples: East 160, West 170, North 90; totals East 415, West 285.
    let one = "<references count=\"1\"><reference field=\"4294967294\" count=\"1\" selected=\"0\"><x v=\"0\"/></reference>";
    let values_ref = "<reference field=\"4294967294\" count=\"1\" selected=\"0\"><x v=\"0\"/></reference>";
    let two = format!("<references count=\"2\">{values_ref}<reference field=\"1\" count=\"1\" selected=\"0\"><x v=\"0\"/></reference>");
    let (wb, _) = reopen_patched(&s, |x| {
        assert!(x.contains(one), "{x}");
        x.replace(one, &two)
    });
    let si = wb.active_sheet();
    let p = wb.sheet(si).unwrap().pivots.last().unwrap();
    let apples = gridcraft_model::PivotScopeItem { source_col: "Product".into(), item: "Apples".into(), ..Default::default() };
    assert_eq!(p.rows[0].sort_scope, vec![apples]);
    let out = crate::pivot::compute(&wb, p, si).unwrap();
    let labels: Vec<String> = out.grid.iter().filter_map(|r| r.first()).map(|c| c.value.display()).collect();
    let order: Vec<&str> = labels.iter().map(String::as_str).filter(|l| ["East", "West", "North"].contains(l)).collect();
    assert_eq!(order, ["West", "East", "North"]);
    // Saving writes the scope back.
    let again = crate::io::save_bytes(&wb, "t.xlsx").unwrap();
    let (back, _) = crate::io::open_bytes("t.xlsx", &again).unwrap();
    assert_eq!(back.sheet(si).unwrap().pivots.last().unwrap().rows[0].sort_scope, p.rows[0].sort_scope);
    // A scope GridCraft can't represent (two Product items) loads as a sort by label, with
    // a warning, instead of as a sort by the grand total.
    let bad = format!("<references count=\"2\">{values_ref}<reference field=\"1\" count=\"2\" selected=\"0\"><x v=\"0\"/><x v=\"1\"/></reference>");
    let (wb2, warnings) = reopen_patched(&s, |x| x.replace(one, &bad));
    let p2 = wb2.sheet(wb2.active_sheet()).unwrap().pivots.last().unwrap();
    assert_eq!(p2.rows[0].sort_by, None);
    assert!(p2.rows[0].sort_scope.is_empty());
    assert!(warnings.iter().any(|w| w.contains("scope")), "{warnings:?}");
}

/// Saves and reopens a workbook.
fn reopen_wb(wb: &gridcraft_model::Workbook) -> gridcraft_model::Workbook {
    let bytes = crate::io::save_bytes(wb, "t.xlsx").unwrap();
    crate::io::open_bytes("t.xlsx", &bytes).unwrap().0
}
/// The labels in the report's first column, as computed from `wb`, keeping only `keep`.
fn first_col(wb: &gridcraft_model::Workbook, keep: &[&str]) -> Vec<String> {
    let si = wb.active_sheet();
    let p = wb.sheet(si).unwrap().pivots.last().unwrap();
    let out = crate::pivot::compute(wb, p, si).unwrap();
    out.grid.iter().filter_map(|r| r.first()).map(|c| c.value.display()).filter(|l| keep.contains(&l.as_str())).collect()
}
const VALUES_REF: &str = "<reference field=\"4294967294\" count=\"1\" selected=\"0\"><x v=\"0\"/></reference>";
/// The Values-only sort scope the writer emits, as a `<references>` opening.
fn values_only() -> String {
    format!("<references count=\"1\">{VALUES_REF}")
}

#[test]
fn sort_scope_tells_date_levels_of_one_column_apart() {
    use gridcraft_model::{PivotDateGroup, PivotField};
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Date"], "values": ["Sales"]})).unwrap();
    edit_pt(&mut s, |p| {
        let date = p.columns[0].clone();
        p.columns =
            vec![PivotField { date_group: PivotDateGroup::Years, ..date.clone() }, PivotField { date_group: PivotDateGroup::Quarters, ..date }];
    });
    s.execute("pivot.sort", json!({"field": "Region", "order": "desc", "by": "Sum of Sales"})).unwrap();
    // Sort by the values in the 2026 › Qtr2 column: Years is cache field 5 (item 1 = 2026,
    // after "<1/15/2026"), Quarters the Date field 2 (item 2 = Qtr2). Qtr2 holds West 70,
    // East 60, North 30; the totals are East 415, West 285, North 135.
    let refs = format!(
        "<references count=\"3\">{VALUES_REF}<reference field=\"5\" count=\"1\" selected=\"0\"><x v=\"1\"/></reference><reference field=\"2\" count=\"1\" selected=\"0\"><x v=\"2\"/></reference>"
    );
    let (wb, _) = reopen_patched(&s, |x| {
        assert!(x.contains(&values_only()), "{x}");
        x.replace(&values_only(), &refs)
    });
    let regions = ["East", "West", "North"];
    assert_eq!(first_col(&wb, &regions), ["West", "East", "North"]);
    // Saving writes both references back.
    let back = reopen_wb(&wb);
    let scope = |wb: &gridcraft_model::Workbook| wb.sheet(wb.active_sheet()).unwrap().pivots.last().unwrap().rows[0].sort_scope.clone();
    assert_eq!(scope(&back).len(), 2);
    assert_eq!(scope(&back), scope(&wb));
    assert_eq!(first_col(&back, &regions), ["West", "East", "North"]);
}

#[test]
fn custom_subtotal_functions_keep_the_subtotal_rows() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Units"]})).unwrap();
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    // Field Settings › Subtotals › Custom (Sum and Count) on Product, as Excel stores it.
    let (wb, warnings) = reopen_patched(&s, |x| {
        let start = x.match_indices("<pivotField ").nth(1).unwrap().0;
        let end = start + x[start..].find("</pivotField>").unwrap();
        let field = x[start..end]
            .replacen("<pivotField ", "<pivotField defaultSubtotal=\"0\" sumSubtotal=\"1\" countSubtotal=\"1\" ", 1)
            .replace("<item t=\"default\"/>", "<item t=\"sum\"/><item t=\"count\"/>");
        assert!(field.contains("t=\"sum\""), "{field}");
        format!("{}{field}{}", &x[..start], &x[end..])
    });
    let p = wb.sheet(wb.active_sheet()).unwrap().pivots.last().unwrap();
    assert_ne!(p.rows[1].subtotal, Some(false));
    // Shown with the automatic subtotal, with a warning.
    let plain = reopen_wb(&s.doc().unwrap().wb);
    let all = |wb: &gridcraft_model::Workbook| {
        let si = wb.active_sheet();
        let out = crate::pivot::compute(wb, wb.sheet(si).unwrap().pivots.last().unwrap(), si).unwrap();
        out.grid.iter().map(|r| r.iter().map(|c| c.value.display()).collect::<Vec<_>>()).collect::<Vec<_>>()
    };
    assert_eq!(all(&wb), all(&plain));
    assert!(warnings.iter().any(|w| w.contains("subtotal")), "{warnings:?}");
}

#[test]
fn custom_subtotals_on_the_first_field_keep_the_table_default() {
    use std::io::Read;
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region", "Product"], "values": ["Units"]})).unwrap();
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    // Custom Sum subtotal on Region (cache field 0, the first axis field), no xpdl extension.
    let (wb, _) = reopen_patched(&s, |x| {
        assert!(!x.contains("pivotTableDefinition16"), "{x}");
        let start = x.find("<pivotField ").unwrap();
        let end = start + x[start..].find("</pivotField>").unwrap();
        let field = x[start..end]
            .replacen("<pivotField ", "<pivotField defaultSubtotal=\"0\" sumSubtotal=\"1\" ", 1)
            .replace("<item t=\"default\"/>", "<item t=\"sum\"/>");
        assert!(field.contains("t=\"sum\""), "{field}");
        format!("{}{field}{}", &x[..start], &x[end..])
    });
    let p = wb.sheet(wb.active_sheet()).unwrap().pivots.last().unwrap();
    assert!(p.subtotals);
    assert_eq!([p.rows[0].subtotal, p.rows[1].subtotal], [None, None]);
    // Saving doesn't add a table default of "off" the file never had.
    let bytes = crate::io::save_bytes(&wb, "t.xlsx").unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    z.by_name("xl/pivotTables/pivotTable1.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert!(!xml.contains("EnabledSubtotalsDefault=\"0\""), "{xml}");
}

#[test]
fn sort_scope_on_a_vanished_item_sorts_as_after_saving() {
    let mut s = sales();
    s.execute("range.setValues", json!({"range": "A14", "values": [["South", "Kiwis", "1/1/2026", 1, 10]]})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.sort", json!({"field": "Region", "order": "desc", "by": "Sum of Sales"})).unwrap();
    // Sorted by the values in the Kiwis column (Product item 3)…
    let refs = format!("<references count=\"2\">{VALUES_REF}<reference field=\"1\" count=\"1\" selected=\"0\"><x v=\"3\"/></reference>");
    let (mut wb, _) = reopen_patched(&s, |x| x.replace(&values_only(), &refs));
    assert_eq!(wb.sheet(wb.active_sheet()).unwrap().pivots.last().unwrap().rows[0].sort_scope.len(), 1);
    // …then Kiwis leaves the source: the rows sort by the grand total, before and after saving.
    let src = wb.sheets.iter().position(|sh| sh.name == "Sheet1").unwrap();
    wb.sheet_mut(src).unwrap().set_value(CellRef::parse("B14").unwrap(), Value::text("Apples"));
    let regions = ["East", "West", "North", "South"];
    let total_order = ["East", "West", "North", "South"];
    assert_eq!(first_col(&wb, &regions), total_order);
    assert_eq!(first_col(&reopen_wb(&wb), &regions), total_order);
}

#[test]
fn date_groups_not_sorted_are_in_date_order() {
    let mut s = sales();
    // The first record is in Qtr4.
    s.execute("cell.set", json!({"cell": "C2", "input": "12/1/2026"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.group", json!({"field": "Date", "by": "quarters"})).unwrap();
    s.execute("pivot.sort", json!({"field": "Date", "order": "none"})).unwrap();
    let quarters = ["Qtr1", "Qtr2", "Qtr3", "Qtr4"];
    assert_eq!(first_col(&s.doc().unwrap().wb, &quarters), quarters);
    let back = reopen_wb(&s.doc().unwrap().wb);
    assert_eq!(first_col(&back, &quarters), quarters);
    // A reordered list still wins; items it doesn't name follow in date order.
    let mut wb = back;
    let si = wb.active_sheet();
    wb.sheet_mut(si).unwrap().pivots.last_mut().unwrap().rows[0].item_order = vec!["Qtr3".into()];
    assert_eq!(first_col(&wb, &quarters), ["Qtr3", "Qtr1", "Qtr2", "Qtr4"]);
    assert_eq!(first_col(&reopen_wb(&wb), &quarters), ["Qtr3", "Qtr1", "Qtr2", "Qtr4"]);
}

#[test]
fn manual_order_of_time_items_from_an_excel_cache() {
    // Excel caches times as `<d v="1899-12-30T08:00:00"/>`: those items must name the
    // report's 8:00 and 9:30, or a refresh puts the blank ahead of them.
    let mut s = Session::new();
    s.new_workbook();
    for (cell, input) in [("A1", "Shift"), ("B1", "Sales"), ("A2", "8:00"), ("B2", "10"), ("A3", "9:30"), ("B3", "20"), ("B4", "5")] {
        s.execute("cell.set", json!({"cell": cell, "input": input})).unwrap();
    }
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Shift"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.sort", json!({"field": "Shift", "order": "none"})).unwrap();
    // The report's first column: times as minutes, and the blank item.
    let shifts = |wb: &gridcraft_model::Workbook| -> Vec<String> {
        let si = wb.active_sheet();
        let out = crate::pivot::compute(wb, wb.sheet(si).unwrap().pivots.last().unwrap(), si).unwrap();
        out.grid
            .iter()
            .filter_map(|r| match &r.first()?.value {
                Value::Number(n) => Some(format!("{}", (n * 1440.0).round())),
                v if v.display() == "(blank)" => Some(v.display()),
                _ => None,
            })
            .collect()
    };
    let order = ["480", "570", "(blank)"];
    assert_eq!(shifts(&s.doc().unwrap().wb), order);
    let excel_items = "<d v=\"1899-12-30T08:00:00\"/><d v=\"1899-12-30T09:30:00\"/><m/>";
    let (back, _) = reopen_patched_part(&s, "xl/pivotCache/pivotCacheDefinition1.xml", |x| {
        let ours = "<n v=\"0.3333333333333333\"/><n v=\"0.3958333333333333\"/><m/>";
        assert!(x.contains(ours), "{x}");
        x.replace(ours, excel_items)
    });
    assert_eq!(shifts(&back), order);
    // Saving writes that order back.
    assert_eq!(shifts(&reopen_wb(&back)), order);
}

#[test]
fn inserting_or_deleting_lines_moves_the_pivot_with_its_cells() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "destination": "Sheet1!H10", "rows": ["Region"], "values": ["Sales"]})).unwrap();
    assert_eq!(pt(&s).last_range.unwrap().a1(), "H10:I14");
    s.execute("home.insertRows", json!({"rows": "16:17"})).unwrap();
    s.execute("home.insertRows", json!({"rows": "6:7"})).unwrap();
    assert_eq!(text(&s, "H12"), "Row Labels");
    let l = s.execute("pivot.list", json!({})).unwrap();
    assert_eq!(l[0]["anchor"], "H12");
    assert_eq!(l[0]["range"], "H12:I16");
    assert_eq!(l[0]["source"], "Sheet1!A1:E15");
    s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
    let p = pt(&s);
    assert_eq!(p.source, "Sheet1!A2:E16");
    assert_eq!(p.anchor.a1(), "H13");
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(pt(&s).last_range.unwrap().a1(), "H13:I17");
    assert_eq!(text(&s, "H13"), "Row Labels");
    assert_eq!(val(&s, "I17"), Value::Number(TOTAL));
    s.execute("home.deleteColumns", json!({"cols": "F:F"})).unwrap();
    let p = pt(&s);
    assert_eq!(p.anchor.a1(), "G13");
    assert_eq!(p.last_range.unwrap().a1(), "G13:H17");
    s.execute("home.deleteRows", json!({"rows": "1:1"})).unwrap();
    assert_eq!(pt(&s).source, "Sheet1!A1:E15");
    assert_eq!(pt(&s).anchor.a1(), "G12");
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(text(&s, "G12"), "Row Labels");
    assert_eq!(val(&s, "H16"), Value::Number(TOTAL));
}

#[test]
fn inserting_rows_on_the_source_sheet_moves_another_sheets_pivot_source() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("home.insertRows", json!({"rows": "1:2"})).unwrap();
    let l = s.execute("pivot.list", json!({})).unwrap();
    assert_eq!(l[0]["source"], "Sheet1!A3:E15");
    assert_eq!(l[0]["anchor"], "A3");
    s.execute("pivot.refresh", json!({"pivot": "PivotTable1"})).unwrap();
}

/// The active sheet's first five columns as shown, down to the first blank row.
fn sheet_rows(s: &mut Session) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for r in 0..30u32 {
        let row: Vec<String> = (0..5)
            .map(|c| s.execute("cell.get", json!({"cell": CellRef::new(r, c).a1()})).unwrap()["text"].as_str().unwrap_or("").to_string())
            .collect();
        if row.iter().all(String::is_empty) {
            break;
        }
        out.push(row);
    }
    out
}

#[test]
fn show_details_lists_a_value_cells_records_on_a_new_sheet() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    // B5 is East × Apples (100 + 60); the active cell is the default.
    s.execute("selection.set", json!({"range": "B5"})).unwrap();
    let r = s.execute("pivot.showDetails", json!({})).unwrap();
    assert_eq!(r["records"], 2);
    let d = s.doc().unwrap();
    assert_eq!(d.wb.sheets.len(), 3);
    assert_eq!(d.wb.active_sheet(), 0, "the new sheet goes before the PivotTable's");
    assert_eq!(d.wb.active().unwrap().name, "Sheet3");
    assert_eq!(
        sheet_rows(&mut s),
        vec![
            vec!["Region", "Product", "Date", "Units", "Sales"],
            vec!["East", "Apples", "1/15/2026", "10", "100"],
            vec!["East", "Apples", "9/9/2026", "6", "60"],
        ]
    );
    // The dates fit their column instead of showing ####.
    let sh = s.doc().unwrap().wb.active().unwrap().clone();
    assert!(sh.col_width(2) > sh.col_width(5), "{} vs {}", sh.col_width(2), sh.col_width(5));
    // One undo step takes the sheet away.
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    assert_eq!(val(&s, "B5"), Value::Number(160.0));
}

#[test]
fn show_details_on_a_total_takes_every_record_it_adds_up() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    let records = |s: &mut Session, cell: &str| {
        let n = s.execute("pivot.showDetails", json!({"cell": cell})).unwrap()["records"].as_u64().unwrap();
        s.execute("edit.undo", json!({})).unwrap();
        n
    };
    assert_eq!(records(&mut s, "E5"), 5, "East's row total");
    assert_eq!(records(&mut s, "C8"), 4, "Pears' column total");
    assert_eq!(records(&mut s, "E8"), 12, "the grand total");
    // A subtotal at the top of its group, in a two-level row axis.
    s.execute("pivot.moveField", json!({"field": "Product", "from": "columns", "to": "rows"})).unwrap();
    assert_eq!(text(&s, "A4"), "East");
    assert_eq!(records(&mut s, "B4"), 5);
    assert_eq!(text(&s, "A6"), "Pears");
    assert_eq!(records(&mut s, "B6"), 2);
}

#[test]
fn show_details_leaves_out_the_records_the_report_filters_out() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Product", "selected": ["Apples"]})).unwrap();
    let gt = find_row(&s, "Grand Total").unwrap();
    let r = s.execute("pivot.showDetails", json!({"cell": CellRef::new(gt, 1).a1()})).unwrap();
    assert_eq!(r["records"], 2);
    let rows = sheet_rows(&mut s);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().skip(1).all(|row| row[0] == "East" && row[1] == "Apples"), "{rows:?}");
}

#[test]
fn show_details_refuses_a_report_its_source_has_moved_on_from() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    // B5 still shows East × Apples (160), but the first East record is now Central's.
    s.execute("range.setValues", json!({"range": "Sheet1!A2", "values": [["Central"]]})).unwrap();
    assert_eq!(val(&s, "B5"), Value::Number(160.0));
    let e = s.execute("pivot.showDetails", json!({"cell": "B5"})).unwrap_err().to_string();
    assert!(e.contains("out of date"), "{e}");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    // After a refresh East × Apples (now B6) has the one record left.
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(text(&s, "A6"), "East");
    assert_eq!(s.execute("pivot.showDetails", json!({"cell": "B6"})).unwrap()["records"], 1);
}

#[test]
fn show_details_refuses_a_report_that_would_now_be_shorter() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    // North goes away: the source now gives one row fewer, so row 8 is no longer the Grand Total.
    s.execute("range.setValues", json!({"range": "Sheet1!A5", "values": [["East"]]})).unwrap();
    s.execute("range.setValues", json!({"range": "Sheet1!A8", "values": [["East"]]})).unwrap();
    s.execute("range.setValues", json!({"range": "Sheet1!A11", "values": [["East"]]})).unwrap();
    assert_eq!(text(&s, "A8"), "Grand Total");
    let e = s.execute("pivot.showDetails", json!({"cell": "E8"})).unwrap_err().to_string();
    assert!(e.contains("out of date"), "{e}");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
}

#[test]
fn show_details_needs_a_value_cell() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    for cell in ["A3", "B3", "A4", "B4", "A5", "A8", "H20"] {
        assert!(s.execute("pivot.showDetails", json!({"cell": cell})).is_err(), "{cell}");
    }
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
}

#[test]
fn enable_show_details_off_stops_show_details() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    assert_eq!(s.execute("pivot.options", json!({})).unwrap()["enableShowDetails"], true);
    s.execute("pivot.options", json!({"enableShowDetails": false})).unwrap();
    assert!(!pt(&s).enable_show_details);
    assert_eq!(s.execute("pivot.fields", json!({})).unwrap()["enableShowDetails"], false);
    let e = s.execute("pivot.showDetails", json!({"cell": "B5"})).unwrap_err().to_string();
    assert_eq!(e, "Show Details is turned off for this PivotTable.");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
    // A label still reports that it is not a value cell.
    let e = s.execute("pivot.showDetails", json!({"cell": "A5"})).unwrap_err().to_string();
    assert!(!e.contains("turned off"), "{e}");
    // Setting the same value again adds no undo step.
    let after = s.doc().unwrap().revision;
    s.execute("pivot.options", json!({"enableShowDetails": false})).unwrap();
    assert_eq!(s.doc().unwrap().revision, after);
    assert!(s.execute("pivot.options", json!({"enableShowDetails": "no"})).is_err());
    // One undo turns the option back on.
    s.execute("edit.undo", json!({})).unwrap();
    assert!(pt(&s).enable_show_details);
    assert_eq!(s.execute("pivot.showDetails", json!({"cell": "B5"})).unwrap()["records"], 2);
}

#[test]
fn enable_show_details_off_is_reported_before_an_out_of_date_report() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.options", json!({"enableShowDetails": false})).unwrap();
    // The source changes after the report was written, so the report is out of date.
    s.execute("range.setValues", json!({"range": "Sheet1!A2", "values": [["Central"]]})).unwrap();
    let e = s.execute("pivot.showDetails", json!({"cell": "B5"})).unwrap_err().to_string();
    assert!(e.contains("turned off") && !e.contains("out of date"), "{e}");
    assert_eq!(s.doc().unwrap().wb.sheets.len(), 2);
}

#[test]
fn detail_cells_are_the_cells_show_details_takes() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    for layout in 0..2 {
        if layout == 1 {
            // A two-level row axis, with subtotals at the top of each group.
            s.execute("pivot.moveField", json!({"field": "Product", "from": "columns", "to": "rows"})).unwrap();
        }
        let d = s.doc().unwrap();
        let si = d.wb.active_sheet();
        let pt = &d.wb.active().unwrap().pivots[0];
        let r = pt.last_range.unwrap();
        let cells = crate::pivot::detail_cells(&d.wb, pt, si).unwrap();
        let mut values = 0;
        for row in 0..r.height() {
            for col in 0..r.width() {
                let one = crate::pivot::detail_records(&d.wb, pt, si, row, col).unwrap();
                assert_eq!(cells.contains(&(row, col)), one.is_some(), "layout {layout}, cell ({row}, {col})");
                values += usize::from(one.is_some());
            }
        }
        assert!(values > 4, "layout {layout} has value cells");
    }
    // An out-of-date report answers as Show Details does.
    s.execute("range.setValues", json!({"range": "Sheet1!A2", "values": [["Central"]]})).unwrap();
    let d = s.doc().unwrap();
    let pt = &d.wb.active().unwrap().pivots[0];
    assert!(crate::pivot::detail_cells(&d.wb, pt, d.wb.active_sheet()).unwrap_err().contains("out of date"));
}

/// Measurement, not a check: `perf stat -e instructions:u` of this test with `PIVOT_BENCH_EDITS=0`
/// and `=20` (and `PIVOT_BENCH_REOPEN=0/1`) gives the cost of edits on the report's sheet and of
/// opening a saved workbook, with the setup subtracted. `PIVOT_BENCH_ROWS` is the source size.
#[test]
#[ignore = "measurement only"]
fn bench_edit_on_sheet_with_large_report() {
    let env = |k: &str, d: usize| std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d);
    let (n, edits) = (env("PIVOT_BENCH_ROWS", 100_000), env("PIVOT_BENCH_EDITS", 0));
    let mut s = Session::new();
    s.new_workbook();
    let mut rows = vec![json!(["Id", "Product", "Sales"])];
    rows.extend((0..n).map(|i| {
        let (id, product) = (format!("id{i:07}"), ["Apples", "Pears", "Plums"][i % 3]);
        json!([id, product, i])
    }));
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Id"], "columns": ["Product"], "values": ["Sales"]})).unwrap();
    for i in 0..edits {
        s.execute("cell.set", json!({"cell": "K1", "input": i.to_string()})).unwrap();
    }
    if env("PIVOT_BENCH_REOPEN", 0) == 1 {
        let wb = (*s.doc().unwrap().wb).clone();
        let (back, _) = gridcraft_xlsx::read_xlsx(&gridcraft_xlsx::write_xlsx(&wb).unwrap()).unwrap();
        s.add_document(crate::DocState::new(back, None, "Reopened".into(), crate::Trust::FromFile));
    }
}

#[test]
fn rank_ties_are_dense_and_count_numbers_is_captioned_count_of() {
    // #1448 items 10 and 12: Excel ranks 5, 4, 3, 3, 4 as 1, 2, 3, 3, 2 (GridCraft gave 1, 2, 4,
    // 4, 2), and captions a Count Numbers field "Count of Val".
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([["Key", "Val"], ["A", 5], ["B", 4], ["C", 3], ["D", 3], ["E", 4]]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!$A$1:$B$6", "destination": "Sheet1!D3", "rows": ["Key"], "values": ["Val"]})).unwrap();
    s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "rank"})).unwrap();
    let ranks: Vec<Value> = ["E4", "E5", "E6", "E7", "E8"].iter().map(|a| val(&s, a)).collect();
    assert_eq!(ranks, [1.0, 2.0, 3.0, 3.0, 2.0].map(Value::Number));
    s.execute("pivot.valueSettings", json!({"index": 0, "showAs": "normal", "func": "countNumbers"})).unwrap();
    assert_eq!(text(&s, "E3"), "Count of Val");
}

#[test]
fn legacy_count_numbers_caption_follows_the_function() {
    // Workbooks saved before Count Numbers was captioned "Count of" carry "Count Numbers of Val";
    // that is still an automatic caption, so it follows a new function. A custom name stays.
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([["Key", "Val"], ["A", 5], ["B", 4]]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!$A$1:$B$3", "destination": "Sheet1!D3", "rows": ["Key"], "values": ["Val"]})).unwrap();
    for (legacy, after) in [("Count Numbers of Val", "Sum of Val"), ("Count Numbers of Val2", "Sum of Val"), ("Numbers counted", "Numbers counted")] {
        s.execute("pivot.valueSettings", json!({"index": 0, "func": "countNumbers", "name": legacy})).unwrap();
        assert_eq!(text(&s, "E3"), legacy);
        s.execute("pivot.valueSettings", json!({"index": 0, "func": "sum"})).unwrap();
        assert_eq!(text(&s, "E3"), after, "{legacy}");
    }
}

/// The grand total of the report on the active sheet (column `col` of the Grand Total row).
fn grand_total(s: &Session, col: u32) -> Value {
    let gt = find_row(s, "Grand Total").unwrap();
    val(s, &CellRef::new(gt, col).a1())
}

#[test]
fn table_source_leaves_out_its_totals_row() {
    let mut s = sales();
    s.execute("insert.table", json!({"range": "A1:E13", "name": "SalesTable"})).unwrap();
    s.execute("table.totalRow", json!({"table": "SalesTable", "on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"source": "SalesTable", "rows": ["Region"], "values": ["Sales"]})).unwrap();
    // Three regions, no "Total" item, and the totals row's sum is not counted again.
    assert_eq!(find_row(&s, "Total"), None);
    assert_eq!(grand_total(&s, 1), Value::Number(TOTAL));
    assert_eq!(pt(&s).last_range.unwrap().height(), 5);
}

#[test]
fn sheet_scoped_and_chained_names_resolve() {
    let mut s = sales();
    s.execute("formulas.defineName", json!({"name": "Data", "refersTo": "=Sheet1!$A$1:$E$13", "scope": "Sheet1"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Alias", "refersTo": "=Data"})).unwrap();
    for (source, at) in [("Data", "H1"), ("Alias", "H20")] {
        s.execute("insert.pivotTable", json!({"source": source, "destination": format!("Sheet1!{at}"), "rows": ["Region"], "values": ["Sales"]}))
            .unwrap();
        assert_eq!(pt(&s).source, source);
        let r = pt(&s).last_range.unwrap();
        assert_eq!(val(&s, &CellRef::new(r.end.row, r.start.col + 1).a1()), Value::Number(TOTAL), "{source}");
    }
}

#[test]
fn repeated_and_padded_headers_keep_their_fields() {
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([[" Region ", "Sales", "sales"], ["East", 1, 10], ["West", 2, 20], ["East", 3, 30]]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    let r =
        s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:C4", "rows": ["Region"], "values": ["sales2"], "filters": ["Sales"]})).unwrap();
    assert_eq!(r["fields"], json!(["Region", "Sales", "sales2"]));
    // The page field on the first `Sales` stays above the report and filters by its own column.
    assert_eq!(text(&s, "A1"), "Sales");
    s.execute("pivot.filter", json!({"field": "Sales", "selected": ["1", "3"]})).unwrap();
    assert_eq!(text(&s, "B1"), "(Multiple Items)");
    assert_eq!(grand_total(&s, 1), Value::Number(40.0));
}

#[test]
fn formatted_page_items_are_chosen_by_their_labels() {
    let mut s = sales();
    s.execute("home.numberFormat", json!({"range": "E2:E13", "code": "$#,##0.00"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Units"], "filters": ["Sales"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Sales", "selected": ["$120.00"]})).unwrap();
    assert_eq!(text(&s, "B1"), "$120.00");
    // East Pears (8) and West Apples (12).
    assert_eq!(grand_total(&s, 1), Value::Number(20.0));
    s.execute("pivot.filter", json!({"field": "Sales", "selected": ["$120.00", "$60.00"]})).unwrap();
    assert_eq!(text(&s, "B1"), "(Multiple Items)");
    assert_eq!(grand_total(&s, 1), Value::Number(30.0));
}

#[test]
fn headers_read_values_while_formulas_show() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Reg\"&\"ion\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    let r = s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    assert_eq!(r["fields"][0], "Region");
}

#[test]
fn accounting_items_keep_their_labels_without_the_fill() {
    let mut s = sales();
    s.execute("home.numberFormat", json!({"range": "E2:E13", "code": "_($* #,##0_);_($* (#,##0);_($* \"-\"_);_(@_)"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Units"], "filters": ["Sales"]})).unwrap();
    // The page item is labelled without a space where the fill is, and a selection stored by
    // that label (as files and earlier versions hold it) still matches.
    s.execute("pivot.filter", json!({"field": "Sales", "selected": [" $120 "]})).unwrap();
    assert_eq!(text(&s, "B1"), " $120 ");
    assert_eq!(grand_total(&s, 1), Value::Number(20.0));
}

/// Older GridCraft read a header as the sheet showed it, so a PivotTable made while Show Formulas
/// was on saved a formula header by its formula text (`="Region"`). After that workbook is saved
/// and reopened, the field and its filter are under the header's value from the moment it opens.
#[test]
fn a_field_saved_under_a_formula_header_text_survives_reopen_and_refresh() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    // East: 100 + 120 + 60 + 60 + 75.
    assert_eq!(grand_total(&s, 1), Value::Number(415.0));
    // The PivotTable as an older GridCraft saved it, named by the header as the sheet showed it.
    let mut wb = (*s.doc().unwrap().wb).clone();
    let si = wb.active_sheet();
    let saved = wb.sheet_mut(si).unwrap().pivots.last_mut().unwrap();
    saved.filters[0].source_col = "=\"Region\"".into();
    saved.names_from_values = false;
    let bytes = crate::io::save_bytes(&wb, "book.json").unwrap();
    let (back, _) = crate::io::open_bytes("book.json", &bytes).unwrap();
    s.add_document(crate::DocState::new(back, None, "Reopened".into(), crate::Trust::FromFile));
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    assert_eq!(pt(&s).filters[0].source_col, "Region", "named by value as soon as it opens");
    assert!(pt(&s).names_from_values);
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    assert_eq!(p.filters[0].source_col, "Region");
    assert_eq!(p.filters[0].selected, Some(vec!["East".to_string()]));
    assert_eq!(grand_total(&s, 1), Value::Number(415.0));
}

/// A formula header `="Region"` beside a text header `Region`: an older GridCraft saved the fields
/// as `="Region"` and `Region`; now the columns are `Region` and `Region2`. Each saved field keeps
/// its own column, even the one whose old name is also a current header. A PivotTable already
/// saved with the current names keeps them.
#[test]
fn older_fields_keep_their_columns_when_an_old_name_is_also_a_current_header() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "Region"})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"], "filters": ["Region2"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region2", "selected": ["Apples"]})).unwrap();
    // Apples: 100 + 50 + 90 + 60 + 120.
    assert_eq!(grand_total(&s, 1), Value::Number(420.0));
    // Saved with the current names: a refresh keeps them.
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!((pt(&s).rows[0].source_col.as_str(), pt(&s).filters[0].source_col.as_str()), ("Region", "Region2"));
    assert_eq!(grand_total(&s, 1), Value::Number(420.0));
    // As an older GridCraft saved it: named by the headers as the sheet showed them.
    let mut wb = (*s.doc().unwrap().wb).clone();
    let si = wb.active_sheet();
    let saved = wb.sheet_mut(si).unwrap().pivots.last_mut().unwrap();
    saved.rows[0].source_col = "=\"Region\"".into();
    saved.filters[0].source_col = "Region".into();
    saved.names_from_values = false;
    let bytes = crate::io::save_bytes(&wb, "book.json").unwrap();
    let (back, _) = crate::io::open_bytes("book.json", &bytes).unwrap();
    s.add_document(crate::DocState::new(back, None, "Reopened".into(), crate::Trust::FromFile));
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    assert_eq!((p.rows[0].source_col.as_str(), p.filters[0].source_col.as_str()), ("Region", "Region2"));
    assert_eq!(p.filters[0].selected, Some(vec!["Apples".to_string()]));
    assert_eq!(grand_total(&s, 1), Value::Number(420.0));
}

/// The active PivotTable as an older GridCraft saved it (no `names_from_values` entry), with `edit`
/// applied, saved as JSON and reopened; its sheet is made active.
fn reopen_as_older(s: &mut Session, edit: impl FnOnce(&mut gridcraft_model::PivotTable)) {
    let mut wb = (*s.doc().unwrap().wb).clone();
    let si = wb.active_sheet();
    let sheet = wb.sheet(si).unwrap().name.clone();
    let saved = wb.sheet_mut(si).unwrap().pivots.last_mut().unwrap();
    edit(saved);
    saved.names_from_values = false;
    let bytes = crate::io::save_bytes(&wb, "book.json").unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("names_from_values"), "an older save has no entry");
    let (back, _) = crate::io::open_bytes("book.json", &bytes).unwrap();
    s.add_document(crate::DocState::new(back, None, "Reopened".into(), crate::Trust::FromFile));
    s.execute("sheet.activate", json!({"sheet": sheet})).unwrap();
}

/// A field saved under a formula header's text keeps its column even when Show Formulas is turned
/// off before the refresh: the older name is matched whatever the sheet shows now.
#[test]
fn an_older_formula_header_field_survives_show_formulas_turned_off() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    reopen_as_older(&mut s, |pt| pt.filters[0].source_col = "=\"Region\"".into());
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": false})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    assert_eq!(p.filters[0].source_col, "Region");
    assert!(p.names_from_values);
    assert_eq!(p.filters[0].selected, Some(vec!["East".to_string()]));
    assert_eq!(grand_total(&s, 1), Value::Number(415.0));
}

/// A formula header `="Region"` (A) beside a text header `Region` (B), and an older PivotTable
/// with only a filter named `Region`: that name was B's under Show Formulas and A's without. The
/// workbook was saved with Show Formulas on, so the filter is B's: it reads `Region2` now.
#[test]
fn an_older_name_both_layouts_place_is_read_as_the_sheet_was_saved() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "Region"})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"], "filters": ["Region2"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region2", "selected": ["Apples"]})).unwrap();
    // Apples (column B): 100 + 50 + 90 + 60 + 120.
    assert_eq!(grand_total(&s, 1), Value::Number(420.0));
    reopen_as_older(&mut s, |pt| pt.filters[0].source_col = "Region".into());
    let p = pt(&s);
    assert_eq!(p.filters[0].source_col, "Region2");
    assert!(p.names_from_values);
    // Turning Show Formulas off afterwards changes nothing: the names are settled.
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": false})).unwrap();
    s.execute("sheet.activate", json!({"sheet": "Sheet2"})).unwrap();
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(pt(&s).filters[0].source_col, "Region2");
    assert_eq!(grand_total(&s, 1), Value::Number(420.0));
}

/// A calculated field in Values isn't a source column, so it doesn't stop the older fields
/// beside it from finding their columns, and it keeps its own name.
#[test]
fn a_calculated_field_does_not_block_the_older_names() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Margin", "formula": "=Sales*0.1"})).unwrap();
    assert!(pt(&s).values.iter().any(|v| v.source_col == "Margin"));
    reopen_as_older(&mut s, |pt| pt.rows[0].source_col = "=\"Region\"".into());
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    assert_eq!(p.rows[0].source_col, "Region");
    assert!(p.values.iter().any(|v| v.source_col == "Margin"));
    assert!(p.names_from_values);
}

/// An older PivotTable with a source field saved as `="Margin"` (a formula header under Show
/// Formulas) and a calculated field `Margin`: the header now reads `Margin` too. Excel doesn't let
/// the two share a name, so the calculated field becomes `Margin2` and the source field takes the
/// column's name. Each value field keeps what it computed.
#[test]
fn an_older_source_field_does_not_take_a_calculated_fields_name() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "D1", "input": "=\"Margin\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    reopen_as_older(&mut s, |pt| {
        let sales = pt.values[0].clone();
        pt.calculated_fields.push(gridcraft_model::PivotCalcField { name: "Margin".into(), formula: "=Sales*0.1".into() });
        pt.values.push(gridcraft_model::PivotValue { source_col: "Margin".into(), name: "Sum of Margin".into(), ..sales.clone() });
        pt.values.push(gridcraft_model::PivotValue { source_col: "=\"Margin\"".into(), name: "Sum of Units".into(), ..sales });
    });
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    let names: Vec<&str> = p.values.iter().map(|v| v.source_col.as_str()).collect();
    assert_eq!(names, ["Sales", "Margin2", "Margin"]);
    assert_eq!(p.calculated_fields[0].name, "Margin2");
    assert!(p.names_from_values);
    // Sales, then the calculated field (a tenth of Sales), not the Units column (72), and the
    // Units column.
    assert_eq!(grand_total(&s, 1), Value::Number(835.0));
    assert_eq!(grand_total(&s, 2), Value::Number(83.5));
    assert_eq!(grand_total(&s, 3), Value::Number(72.0));
    // The field list shows the same names: the Units column `Margin` in Values, beside the
    // calculated `Margin2`.
    let r = s.execute("pivot.fields", json!({})).unwrap();
    let fields = r["fields"].as_array().unwrap();
    let areas = |name: &str, calculated: bool| {
        fields.iter().find(|f| f["name"] == name && f["calculated"].as_bool().unwrap_or(false) == calculated).map(|f| f["areas"].clone())
    };
    assert_eq!(areas("Margin", false), Some(json!(["values"])));
    assert_eq!(areas("Margin2", true), Some(json!(["values"])));
}

/// A calculated field's formula names a column as the sheet showed it: in an older PivotTable
/// with a formula header `="Amount"` (A) beside a text header `Amount` (B), `=Amount*2` read B.
/// Now A reads `Amount` and B `Amount2`: the value field takes `Amount` and the formula
/// `=Amount2*2`, so each still reads the column it read.
#[test]
fn a_calculated_formula_keeps_the_column_it_named() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x", "Amount", "Key"], [100, 5, "K"]]})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Amount\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Key"], "values": ["Amount"]})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Twice", "formula": "=Amount2*2"})).unwrap();
    reopen_as_older(&mut s, |pt| {
        pt.values[0].source_col = "=\"Amount\"".into();
        pt.calculated_fields[0].formula = "=Amount*2".into();
    });
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    assert!(p.names_from_values);
    assert_eq!(p.values[0].source_col, "Amount");
    assert_eq!(p.calculated_fields[0].formula, "=Amount2*2");
    // Column A (100), and twice column B (10), as the PivotTable was saved.
    assert_eq!(grand_total(&s, 1), Value::Number(100.0));
    assert_eq!(grand_total(&s, 2), Value::Number(10.0));
}

/// When both ways of reading the headers put the saved fields on the same columns, the sheet's
/// Show Formulas setting picks the layout a calculated field's formula reads, as it does when
/// they differ. With Show Formulas off, `Amount` is column A (100), so `Twice` is 200, not twice
/// column B (10).
#[test]
fn equal_placements_follow_show_formulas_for_calculated_formulas() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x", "Amount", "Key"], [100, 5, "K"]]})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Amount\""})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Key"], "values": ["Key"]})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Twice", "formula": "=Amount*2"})).unwrap();
    reopen_as_older(&mut s, |_| {});
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(grand_total(&s, 2), Value::Number(200.0));
}

/// Show Details reads the records the report is computed from: an older filter `Region`, which
/// filters column B (`Region2` now), takes B's Apples records, and the refreshed report is current.
#[test]
fn show_details_on_an_older_pivottable_takes_the_records_it_shows() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "Region"})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"], "filters": ["Region2"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region2", "selected": ["Apples"]})).unwrap();
    reopen_as_older(&mut s, |pt| pt.filters[0].source_col = "Region".into());
    s.execute("pivot.refresh", json!({})).unwrap();
    let gt = find_row(&s, "Grand Total").unwrap();
    let col = pt(&s).last_range.unwrap().start.col + 1;
    let r = s.execute("pivot.showDetails", json!({"cell": CellRef::new(gt, col).a1()})).unwrap();
    assert_eq!(r["records"], 5, "the five Apples records");
}

/// The field list shows an older PivotTable's fields as its report is computed. With a formula
/// header `="Region"` (A) beside a text header `Region` (B) and Show Formulas on, a filter saved
/// as `Region` filters column B, which reads `Region2` now, so the list puts `Region2` under
/// Filters, and removing it by that name works.
#[test]
fn the_field_list_shows_an_older_pivottable_as_it_is_computed() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "Region"})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Date"], "values": ["Sales"], "filters": ["Region2"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region2", "selected": ["Apples"]})).unwrap();
    reopen_as_older(&mut s, |pt| pt.filters[0].source_col = "Region".into());
    let r = s.execute("pivot.fields", json!({})).unwrap();
    let areas = |name: &str| r["fields"].as_array().unwrap().iter().find(|f| f["name"] == name).unwrap()["areas"].clone();
    assert_eq!(areas("Region"), json!([]));
    assert_eq!(areas("Region2"), json!(["filters"]));
    assert_eq!(r["filters"][0]["field"], "Region2");
    s.execute("pivot.removeField", json!({"field": "Region2", "area": "filters"})).unwrap();
    assert!(pt(&s).filters.is_empty());
    assert_eq!(grand_total(&s, 1), Value::Number(835.0));
}

/// An edit to an older PivotTable works with the names its report and field list show: adding a
/// field neither drops the older field beside it nor turns a calculated formula onto another column.
#[test]
fn an_edit_to_an_older_pivottable_keeps_its_fields_and_formulas() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x", "Amount", "Key"], [100, 5, "K"]]})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Amount\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Key"], "values": ["Amount"]})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Twice", "formula": "=Amount2*2"})).unwrap();
    reopen_as_older(&mut s, |pt| {
        pt.values[0].source_col = "=\"Amount\"".into();
        pt.calculated_fields[0].formula = "=Amount*2".into();
    });
    s.execute("pivot.addField", json!({"field": "Amount2", "area": "values"})).unwrap();
    let names: Vec<String> = pt(&s).values.iter().map(|v| v.source_col.clone()).collect();
    assert_eq!(names, ["Amount", "Twice", "Amount2"]);
    // Column A (100), twice column B (10), column B (5).
    assert_eq!(grand_total(&s, 1), Value::Number(100.0));
    assert_eq!(grand_total(&s, 2), Value::Number(10.0));
    assert_eq!(grand_total(&s, 3), Value::Number(5.0));
}

/// An older PivotTable whose source couldn't be read as the workbook opened (another header was
/// blank) is renamed as soon as it can be: by the edit that fills the header in, so a refresh
/// without reopening keeps the field, and a file saved without a refresh names it too. Undoing
/// the edit undoes both, and redoing it brings both back.
#[test]
fn an_older_pivottable_is_renamed_once_its_source_can_be_read() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("cell.set", json!({"cell": "Sheet1!D1", "input": ""})).unwrap();
    reopen_as_older(&mut s, |pt| pt.rows[0].source_col = "=\"Region\"".into());
    let state = |s: &Session| (pt(s).rows[0].source_col.clone(), pt(s).names_from_values);
    let (older, renamed) = (("=\"Region\"".to_string(), false), ("Region".to_string(), true));
    assert_eq!(state(&s), older);
    assert!(s.execute("pivot.refresh", json!({})).is_err(), "a blank header can't be read");
    // An edit elsewhere leaves it waiting.
    s.execute("cell.set", json!({"cell": "Sheet1!K1", "input": "x"})).unwrap();
    assert_eq!(state(&s), older);
    s.execute("cell.set", json!({"cell": "Sheet1!D1", "input": "Units"})).unwrap();
    assert_eq!(state(&s), renamed);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(state(&s), older);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(state(&s), renamed);
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();
    let (back, _) = crate::io::open_bytes("book.xlsx", &bytes).unwrap();
    let p = back.sheets.iter().flat_map(|sh| sh.pivots.iter()).next().unwrap();
    assert_eq!(p.rows.iter().map(|f| f.source_col.as_str()).collect::<Vec<_>>(), ["Region"]);
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(state(&s), renamed);
    assert!(find_row(&s, "East").is_some(), "the field stays in the report");
    assert_eq!(grand_total(&s, 1), Value::Number(TOTAL));
}

/// A header that gets its value from a recalculation, not an edit, is read too.
#[test]
fn an_older_pivottable_is_renamed_after_a_recalculation() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("cell.set", json!({"cell": "Sheet1!D1", "input": "=Sheet1!K1&\"\""})).unwrap();
    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    reopen_as_older(&mut s, |pt| pt.rows[0].source_col = "=\"Region\"".into());
    s.execute("cell.set", json!({"cell": "Sheet1!K1", "input": "Units"})).unwrap();
    assert!(!pt(&s).names_from_values);
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert_eq!((pt(&s).rows[0].source_col.as_str(), pt(&s).names_from_values), ("Region", true));
}

/// The names calculated fields' formulas use count in choosing how the headers were read. Saved
/// with Show Formulas off, `Key` fits both ways, but only Show Formulas gives `="Amount"` (A) and
/// `Amount` (B) their own columns, so the difference is still A - B.
#[test]
fn calculated_formula_names_help_choose_the_header_layout() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("range.setValues", json!({"range": "A1", "values": [["x", "Amount", "Key"], [100, 5, "K"]]})).unwrap();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Amount\""})).unwrap();
    s.execute("selection.set", json!({"range": "A1"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Key"], "values": ["Key"]})).unwrap();
    s.execute("pivot.calculatedField", json!({"name": "Net", "formula": "=Amount-Amount2"})).unwrap();
    reopen_as_older(&mut s, |pt| pt.calculated_fields[0].formula = "='=\"Amount\"'-Amount".into());
    assert_eq!(pt(&s).calculated_fields[0].formula, "=Amount-Amount2");
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(grand_total(&s, 2), Value::Number(95.0));
}

/// GETPIVOTDATA finds an older report's values by the new names without a refresh: the saved
/// index describes the report as stored, so its names change with the fields. (The report's own
/// header still shows the old caption, so the index couldn't be rebuilt from it.)
#[test]
fn getpivotdata_reads_an_older_report_by_its_new_names() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    s.execute("pivot.layout", json!({"layout": "tabular"})).unwrap();
    let area = pt(&s).last_range.unwrap();
    s.execute("cell.set", json!({"cell": area.start.a1(), "input": "'=\"Region\""})).unwrap();
    reopen_as_older(&mut s, |pt| {
        pt.rows[0].source_col = "=\"Region\"".into();
        let mut cells = pt.data_index.cells().unwrap().to_vec();
        cells.iter_mut().flat_map(|c| &mut c.items).for_each(|i| i.field = "=\"Region\"".into());
        pt.data_index = gridcraft_model::PivotLookup::default();
        pt.data_index.set_cells(cells);
    });
    let f = format!("=GETPIVOTDATA(\"Sales\",{},\"Region\",\"East\")", area.start.a1());
    let r = s.execute("formulas.evaluate", json!({"formula": f, "cell": "Z1"})).unwrap();
    // East: 100 + 120 + 60 + 60 + 75.
    assert!(r.to_string().contains("415"), "{r}");
}

/// An older PivotTable saved as XLSX keeps its fields: the file names them as the cache does.
#[test]
fn an_older_pivottable_keeps_its_fields_through_xlsx() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    reopen_as_older(&mut s, |pt| pt.rows[0].source_col = "=\"Region\"".into());
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();
    let (back, _) = crate::io::open_bytes("book.xlsx", &bytes).unwrap();
    let p = back.sheets.iter().flat_map(|sh| sh.pivots.iter()).next().unwrap();
    let rows: Vec<&str> = p.rows.iter().map(|f| f.source_col.as_str()).collect();
    assert_eq!(rows, ["Region"]);
}

/// A header the engine names through its number format (`2026.0`) is the name the XLSX cache
/// gives it too, so a field on it survives saving as XLSX.
#[test]
fn a_formatted_header_keeps_its_field_through_xlsx() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "2026"})).unwrap();
    s.execute("home.numberFormat", json!({"range": "A1", "code": "0.0"})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["2026.0"], "values": ["Sales"]})).unwrap();
    assert_eq!(pt(&s).rows[0].source_col, "2026.0");
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();
    let (back, _) = crate::io::open_bytes("book.xlsx", &bytes).unwrap();
    let p = back.sheets.iter().flat_map(|sh| sh.pivots.iter()).next().unwrap();
    let rows: Vec<&str> = p.rows.iter().map(|f| f.source_col.as_str()).collect();
    assert_eq!(rows, ["2026.0"]);
}

#[test]
fn distinct_count_counts_each_different_value_once_per_cell_and_in_the_totals() {
    // #3336: Alice, Bob, Alice, Charlie is 3 distinct customers, where Count says 4. Case does not
    // make a name different, and an empty cell is not a value.
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([
        ["Region", "Customer"],
        ["East", "Alice"],
        ["East", "Bob"],
        ["East", "alice"],
        ["East", null],
        ["West", "Alice"],
        ["West", "Charlie"]
    ]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    let make = json!({"source": "Sheet1!$A$1:$B$7", "destination": "Sheet1!D3", "rows": ["Region"], "values": ["Customer"]});
    s.execute("insert.pivotTable", make).unwrap();
    s.execute("pivot.valueSettings", json!({"index": 0, "func": "count"})).unwrap();
    let counts: Vec<Value> = ["E4", "E5", "E6"].iter().map(|a| val(&s, a)).collect();
    assert_eq!(counts, [3.0, 2.0, 5.0].map(Value::Number), "Count counts every non-empty cell");
    s.execute("pivot.valueSettings", json!({"index": 0, "func": "distinctCount"})).unwrap();
    assert_eq!(text(&s, "E3"), "Distinct Count of Customer");
    let distinct: Vec<Value> = ["E4", "E5", "E6"].iter().map(|a| val(&s, a)).collect();
    assert_eq!(distinct, [2.0, 2.0, 3.0].map(Value::Number), "East has Alice and Bob, West Alice and Charlie, 3 in all");
}

#[test]
fn distinct_count_survives_an_xlsx_round_trip_and_a_refresh() {
    // #3385: saving wrote Distinct Count as Count, so Alice, Bob, Alice became 3 after reopening
    // and refreshing. The sheet keeps the computed distinct counts for other readers too.
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([["Region", "Customer"], ["East", "Alice"], ["East", "Bob"], ["East", "alice"], ["West", "Alice"], ["West", "Charlie"]]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    let make = json!({"source": "Sheet1!$A$1:$B$6", "destination": "Sheet1!D3", "rows": ["Region"], "values": ["Customer"]});
    s.execute("insert.pivotTable", make).unwrap();
    s.execute("pivot.valueSettings", json!({"index": 0, "func": "distinctCount"})).unwrap();
    let cells = ["E4", "E5", "E6"];
    let expected = [2.0, 2.0, 3.0].map(Value::Number);
    assert_eq!(cells.map(|a| val(&s, a)), expected);
    let bytes = crate::io::save_bytes(&s.doc().unwrap().wb, "book.xlsx").unwrap();

    // The written sheet holds the distinct counts, and the cache doesn't ask for a refresh on open.
    let (written, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sheet = written.sheet(0).unwrap();
    assert_eq!(cells.map(|a| sheet.value(CellRef::parse(a).unwrap())), expected);
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut cache = String::new();
    std::io::Read::read_to_string(&mut z.by_name("xl/pivotCache/pivotCacheDefinition1.xml").unwrap(), &mut cache).unwrap();
    assert!(!cache.contains("refreshOnLoad"), "{cache}");

    let mut back = Session::new();
    back.execute("file.open", json!({"name": "book.xlsx", "base64": crate::io::base64_encode(&bytes)})).unwrap();
    assert_eq!(pt(&back).values[0].func, gridcraft_model::PivotFunc::DistinctCount);
    back.execute("pivot.refresh", json!({"pivot": "PivotTable1"})).unwrap();
    assert_eq!(text(&back, "E3"), "Distinct Count of Customer");
    assert_eq!(cells.map(|a| val(&back, a)), expected, "a refresh after reopening still counts distinct customers");
}

#[test]
fn saving_a_distinct_count_pivot_as_xlsx_says_only_gridcraft_reads_it() {
    // #3385: the file stores a plain Count; Excel's Refresh turns it into a count, so the user is told.
    let notices =
        |s: &mut Session| s.take_ui_requests().iter().filter(|r| matches!(r, crate::UiRequest::Message(m) if m.contains("Distinct Count"))).count();
    let dir = std::env::temp_dir().join(format!("gridcraft-distinct-notice-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Session::new();
    s.new_workbook();
    let rows = json!([["Region", "Customer"], ["East", "Alice"], ["East", "Bob"], ["West", "Alice"]]);
    s.execute("range.setValues", json!({"range": "A1", "values": rows})).unwrap();
    let make = json!({"source": "Sheet1!$A$1:$B$4", "destination": "Sheet1!D3", "rows": ["Region"], "values": ["Customer"]});
    s.execute("insert.pivotTable", make).unwrap();
    s.take_ui_requests();

    s.execute("file.saveAs", json!({"path": dir.join("plain.xlsx")})).unwrap();
    assert_eq!(notices(&mut s), 0, "a plain Count is saved as it is");

    s.execute("pivot.valueSettings", json!({"index": 0, "func": "distinctCount"})).unwrap();
    s.execute("file.saveBytes", json!({"format": "csv"})).unwrap();
    s.execute("file.saveAs", json!({"path": dir.join("book.csv")})).unwrap();
    assert_eq!(notices(&mut s), 0, "only an .xlsx file stores the marker");
    s.execute("file.saveAs", json!({"path": dir.join("book.xlsx")})).unwrap();
    assert_eq!(notices(&mut s), 1);
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(notices(&mut s), 0, "saving the same file again does not repeat it");
    // The path is known now, but a Distinct Count added later was never disclosed.
    s.execute("insert.pivotTable", json!({"source": "Sheet1!$A$1:$B$4", "destination": "Sheet1!H3", "rows": ["Region"], "values": ["Customer"]}))
        .unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(notices(&mut s), 0, "a plain Count adds nothing to disclose");
    s.execute("pivot.valueSettings", json!({"index": 0, "func": "distinctCount"})).unwrap();
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(notices(&mut s), 1, "a new Distinct Count field is disclosed on an ordinary Save to the same path");
    s.execute("file.save", json!({})).unwrap();
    assert_eq!(notices(&mut s), 0, "and only once");
    s.execute("file.saveBytes", json!({"format": "xlsx"})).unwrap();
    assert_eq!(notices(&mut s), 1, "a download is an export too");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A defined name with a table's name doesn't stop a PivotTable's table source following a rename
/// of the table: a bare source binds to the table first.
#[test]
fn a_table_rename_moves_a_pivot_source_a_defined_name_shadows() {
    let mut s = sales();
    s.execute("insert.table", json!({"range": "A1:E13"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    s.execute("table.summarizePivot", json!({"rows": ["Product"], "values": ["Units"]})).unwrap();
    assert_eq!(pt(&s).source, "Table1");
    s.execute("formulas.defineName", json!({"name": "Table1", "refersTo": "=Sheet1!$A$1:$B$3"})).unwrap();
    s.execute("table.rename", json!({"table": "Table1", "name": "Data"})).unwrap();
    assert_eq!(pt(&s).source, "Data");
    s.execute("pivot.refresh", json!({})).unwrap();
    let gt = find_row(&s, "Grand Total").unwrap();
    assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(72.0));
}

/// A PivotTable source that is a table's name reads the table before a defined name of that name,
/// so renaming the defined name leaves it on the table; a source that is the defined name's own
/// follows the rename.
#[test]
fn renaming_a_defined_name_leaves_a_table_source_on_its_table() {
    let mut s = sales();
    s.execute("insert.table", json!({"range": "A1:E13", "name": "Data"})).unwrap();
    s.execute("formulas.defineName", json!({"name": "Data", "refersTo": "=Sheet1!$A$1:$E$3"})).unwrap();
    s.execute("selection.set", json!({"range": "B2"})).unwrap();
    s.execute("table.summarizePivot", json!({"rows": ["Product"], "values": ["Units"]})).unwrap();
    assert_eq!(pt(&s).source, "Data");
    s.execute("formulas.defineName", json!({"name": "Choices", "oldName": "Data", "refersTo": "=Sheet1!$A$1:$E$3"})).unwrap();
    assert_eq!(pt(&s).source, "Data");
    s.execute("pivot.refresh", json!({})).unwrap();
    let gt = find_row(&s, "Grand Total").unwrap();
    assert_eq!(val(&s, &CellRef::new(gt, 1).a1()), Value::Number(72.0));
}

/// A field saved under a formula header's text, saved and reopened with Show Formulas off the
/// whole time, takes the column's name now.
#[test]
fn an_older_formula_header_field_saved_with_show_formulas_off_takes_its_column() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Product"], "values": ["Sales"], "filters": ["Region"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Region", "selected": ["East"]})).unwrap();
    assert_eq!(grand_total(&s, 1), Value::Number(415.0));
    reopen_as_older(&mut s, |pt| pt.filters[0].source_col = "=\"Region\"".into());
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    assert_eq!(p.filters[0].source_col, "Region");
    assert_eq!(p.filters[0].selected, Some(vec!["East".to_string()]));
    assert_eq!(grand_total(&s, 1), Value::Number(415.0));
}

/// Neither way of showing the headers places every saved field: `="Region"` is only A's name
/// with Show Formulas on, and `Product` only B's (a formula header) with it off. Each name takes
/// the column the way that places it gives, so both fields keep their columns.
#[test]
fn each_older_name_takes_the_column_whichever_way_places_it() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=\"Prod\"&\"uct\""})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"], "filters": ["Product"]})).unwrap();
    s.execute("pivot.filter", json!({"field": "Product", "selected": ["Apples"]})).unwrap();
    assert_eq!(grand_total(&s, 1), Value::Number(420.0));
    reopen_as_older(&mut s, |pt| pt.rows[0].source_col = "=\"Region\"".into());
    s.execute("pivot.refresh", json!({})).unwrap();
    let p = pt(&s);
    assert_eq!((p.rows[0].source_col.as_str(), p.filters[0].source_col.as_str()), ("Region", "Product"));
    assert_eq!(p.filters[0].selected, Some(vec!["Apples".to_string()]));
    assert_eq!(grand_total(&s, 1), Value::Number(420.0));
}

/// Two saved names that the two ways of showing the headers would both put on column A: the way
/// that places more of the fields wins, and the other name is left as it is rather than reading
/// A a second time.
#[test]
fn an_older_name_the_other_way_would_put_on_a_taken_column_is_left() {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("cell.set", json!({"cell": "B1", "input": "=\"Prod\"&\"uct\""})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"], "filters": ["Product"]})).unwrap();
    reopen_as_older(&mut s, |pt| {
        // `Region` and `Product` are A's and B's names with Show Formulas off; `="Region"` is
        // A's only with it on.
        let mut col = pt.rows[0].clone();
        col.source_col = "=\"Region\"".into();
        pt.columns.push(col);
    });
    let p = pt(&s);
    assert_eq!(p.rows[0].source_col, "Region");
    assert_eq!(p.filters[0].source_col, "Product");
    assert_eq!(p.columns[0].source_col, "=\"Region\"", "A is already the row field's");
    assert!(p.names_from_values);
}

/// A book an older GridCraft saved with a GETPIVOTDATA formula for `field` at Sheet1!K2, its
/// Region header a formula, a PivotTable index under the old name, and `blank` a cleared header.
fn older_with_getpivotdata(field: &str, blank: &str) -> Session {
    let mut s = sales();
    s.execute("cell.set", json!({"cell": "A1", "input": "=\"Region\""})).unwrap();
    s.execute("formulas.showFormulas", json!({"on": true})).unwrap();
    s.execute("insert.pivotTable", json!({"rows": ["Region"], "values": ["Sales"]})).unwrap();
    let (at, sheet) = (pt(&s).last_range.unwrap().start.a1(), s.doc().unwrap().wb.active().unwrap().name.clone());
    let f = format!("=GETPIVOTDATA(\"Sales\",{sheet}!{at},\"{}\",\"East\")", field.replace('"', "\"\""));
    s.execute("cell.set", json!({"cell": "Sheet1!K2", "input": f})).unwrap();
    if !blank.is_empty() {
        s.execute("cell.set", json!({"cell": blank, "input": ""})).unwrap();
    }
    reopen_as_older(&mut s, |pt| {
        pt.rows[0].source_col = "=\"Region\"".into();
        let mut cells = pt.data_index.cells().unwrap().to_vec();
        cells.iter_mut().flat_map(|c| &mut c.items).for_each(|i| i.field = "=\"Region\"".into());
        pt.data_index = gridcraft_model::PivotLookup::default();
        pt.data_index.set_cells(cells);
    });
    s
}

fn k2(s: &Session) -> Value {
    let wb = &s.doc().unwrap().wb;
    wb.sheet(wb.sheet_index("Sheet1").unwrap()).unwrap().value(CellRef::parse("K2").unwrap())
}

/// The edit that lets an older PivotTable be renamed also recalculates the GETPIVOTDATA formulas
/// that read its report, so no published value comes from the old names.
#[test]
fn getpivotdata_is_recalculated_when_an_edit_renames_an_older_pivottable() {
    for (blank, fix) in [("", ""), ("Sheet1!D1", "Units")] {
        let mut s = older_with_getpivotdata("Region", blank);
        if !fix.is_empty() {
            assert_eq!(k2(&s), Value::Error(CellError::Ref));
            s.execute("cell.set", json!({"cell": blank, "input": fix})).unwrap();
        }
        assert!(pt(&s).names_from_values);
        assert_eq!(k2(&s), Value::Number(415.0), "blank header: {blank:?}");
        if !fix.is_empty() {
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!((k2(&s), pt(&s).names_from_values), (Value::Error(CellError::Ref), false));
            s.execute("edit.redo", json!({})).unwrap();
            assert_eq!((k2(&s), pt(&s).names_from_values), (Value::Number(415.0), true));
        }
    }
}

/// A control linked to a cell that reads the renamed PivotTable shows the cell's final value.
#[test]
fn a_linked_control_follows_the_recalculation_after_an_older_pivottable_is_renamed() {
    let mut s = older_with_getpivotdata("Region", "Sheet1!D1");
    s.execute("sheet.activate", json!({"sheet": "Sheet1"})).unwrap();
    s.execute("cell.set", json!({"cell": "L2", "input": "=IFERROR(K2,1)"})).unwrap();
    s.execute("insert.spinButton", json!({"at": "N2", "link": "L2", "max": 1000})).unwrap();
    let shown = |s: &Session| s.doc().unwrap().wb.active().unwrap().controls[0].value;
    assert_eq!(shown(&s), 1);
    s.execute("cell.set", json!({"cell": "D1", "input": "Units"})).unwrap();
    assert_eq!((k2(&s), shown(&s)), (Value::Number(415.0), 415));
}

/// Under manual calculation, the Calculate Now that lets it be renamed does the same.
#[test]
fn getpivotdata_is_recalculated_when_calculate_now_renames_an_older_pivottable() {
    let mut s = older_with_getpivotdata("Region", "Sheet1!D1");
    s.execute("formulas.calculationOptions", json!({"mode": "manual"})).unwrap();
    s.execute("cell.set", json!({"cell": "Sheet1!D1", "input": "=Sheet1!K5&\"\""})).unwrap();
    s.execute("cell.set", json!({"cell": "Sheet1!K5", "input": "Units"})).unwrap();
    assert!(!pt(&s).names_from_values);
    s.execute("formulas.calculateNow", json!({})).unwrap();
    assert!(pt(&s).names_from_values);
    assert_eq!(k2(&s), Value::Number(415.0));
}

/// GETPIVOTDATA knows a field by its current name only: a formula that spells the name an older
/// GridCraft showed is not rewritten, and reads as an unknown field once the PivotTable is renamed.
#[test]
fn getpivotdata_does_not_know_an_older_pivottables_old_names() {
    let s = older_with_getpivotdata("=\"Region\"", "");
    assert!(pt(&s).names_from_values);
    assert_eq!(k2(&s), Value::Error(CellError::Ref));
}

/// A waiting PivotTable is renamed by the headers it first finds readable: a mistyped header costs
/// the field that named it, unless the field is named by the header's plain text.
#[test]
fn a_mistyped_header_costs_a_waiting_older_pivottable_only_that_formula_header() {
    for (header, typo, right, kept) in [("Sheet1!D1", "Unts", "Units", true), ("Sheet1!A1", "=\"Regoin\"", "=\"Region\"", false)] {
        let mut s = older_with_getpivotdata("Region", header);
        assert!(!pt(&s).names_from_values);
        s.execute("cell.set", json!({"cell": header, "input": typo})).unwrap();
        assert!(pt(&s).names_from_values);
        s.execute("cell.set", json!({"cell": header, "input": right})).unwrap();
        s.execute("pivot.refresh", json!({})).unwrap();
        assert_eq!(find_row(&s, "East").is_some(), kept, "{header}");
    }
    // Undoing the mistyped header lets the PivotTable wait again.
    let mut s = older_with_getpivotdata("Region", "Sheet1!A1");
    s.execute("cell.set", json!({"cell": "Sheet1!A1", "input": "=\"Regoin\""})).unwrap();
    s.execute("edit.undo", json!({})).unwrap();
    s.execute("cell.set", json!({"cell": "Sheet1!A1", "input": "=\"Region\""})).unwrap();
    s.execute("pivot.refresh", json!({})).unwrap();
    assert!(find_row(&s, "East").is_some());
}
