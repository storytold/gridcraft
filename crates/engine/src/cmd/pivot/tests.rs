use gridcraft_core::{CellError, CellRef, Value};
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
fn inserting_or_deleting_lines_moves_the_pivot_with_its_cells() {
    let mut s = sales();
    s.execute("insert.pivotTable", json!({"source": "Sheet1!A1:E13", "destination": "Sheet1!H10", "rows": ["Region"], "values": ["Sales"]})).unwrap();
    assert_eq!(pt(&s).last_range.unwrap().a1(), "H10:I14");
    // Rows above the report (and below the source) move the report only.
    s.execute("home.insertRows", json!({"rows": "16:17"})).unwrap();
    s.execute("home.insertRows", json!({"rows": "6:7"})).unwrap();
    assert_eq!(text(&s, "H12"), "Row Labels");
    let l = s.execute("pivot.list", json!({})).unwrap();
    assert_eq!(l[0]["anchor"], "H12");
    assert_eq!(l[0]["range"], "H12:I16");
    assert_eq!(l[0]["source"], "Sheet1!A1:E15");
    // A row above the source moves both.
    s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
    let p = pt(&s);
    assert_eq!(p.source, "Sheet1!A2:E16");
    assert_eq!(p.anchor.a1(), "H13");
    s.execute("pivot.refresh", json!({})).unwrap();
    assert_eq!(pt(&s).last_range.unwrap().a1(), "H13:I17");
    assert_eq!(text(&s, "H13"), "Row Labels");
    assert_eq!(val(&s, "I17"), Value::Number(TOTAL));
    // Deleting a column left of the report moves it left.
    s.execute("home.deleteColumns", json!({"cols": "F:F"})).unwrap();
    let p = pt(&s);
    assert_eq!(p.anchor.a1(), "G13");
    assert_eq!(p.last_range.unwrap().a1(), "G13:H17");
    // Deleting rows above moves it up.
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
