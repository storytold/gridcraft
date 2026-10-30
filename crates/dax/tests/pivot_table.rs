//! Integration tests for the model-backed `PivotTable` object.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable};
use gridcraft_dax::pivot_table::{DEFAULT_MAX_CELLS, PivotFilter, PivotLayout, PivotTable, PivotTableError};
use gridcraft_dax::value::{Scalar, TableValue};

// Region   | Category | Amount | Quantity
// East     | Widget   |     10 |        2
// West     | Widget   |     20 |        3
// East     | Gadget   |     30 |        4
// East     | Widget   |      5 |        1
// (Blank)  | Gadget   |      7 |        1
fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
            ],
            rows: vec![
                vec![Scalar::String("East".into()), Scalar::String("Widget".into()), Scalar::Integer(10), Scalar::Integer(2)],
                vec![Scalar::String("West".into()), Scalar::String("Widget".into()), Scalar::Integer(20), Scalar::Integer(3)],
                vec![Scalar::String("East".into()), Scalar::String("Gadget".into()), Scalar::Integer(30), Scalar::Integer(4)],
                vec![Scalar::String("East".into()), Scalar::String("Widget".into()), Scalar::Integer(5), Scalar::Integer(1)],
                vec![Scalar::Blank, Scalar::String("Gadget".into()), Scalar::Integer(7), Scalar::Integer(1)],
            ],
        }],
        relationships: Vec::new(),
        measures: vec![
            Measure { name: "Total Sales".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure { name: "Total Quantity".into(), formula: "SUM(Sales[Quantity])".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure { name: "Homeless".into(), formula: "1".into(), table: None, format: None, expr: None },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn header(table: &TableValue) -> Vec<String> {
    table.columns.iter().map(|column| column.name.clone()).collect()
}

fn row(table: &TableValue, index: usize) -> Vec<Scalar> {
    table.rows.get(index).cloned().unwrap_or_default()
}

fn text(value: &str) -> Scalar {
    Scalar::String(value.into())
}

fn sales_pivot() -> PivotTable {
    PivotTable::new("Sales by region", "Total Sales")
}

#[test]
fn simple_rows_render_one_row_per_distinct_value() {
    let table = sales_pivot().with_row_fields(vec!["Sales[Region]".into()]).with_layout(PivotLayout::Tabular).render(&model()).expect("render");

    assert_eq!(header(&table), vec!["Region".to_string(), "Total Sales".to_string()]);
    assert_eq!(table.rows.len(), 3);
    // BLANK sorts first, then East (10 + 30 + 5), then West.
    assert_eq!(row(&table, 0), vec![Scalar::Blank, Scalar::Decimal(7.0)]);
    assert_eq!(row(&table, 1), vec![text("East"), Scalar::Decimal(45.0)]);
    assert_eq!(row(&table, 2), vec![text("West"), Scalar::Decimal(20.0)]);
    assert_eq!(table.lineage, Some(0));
}

#[test]
fn compact_layout_collapses_row_fields_and_labels_blanks() {
    let table = sales_pivot().with_row_fields(vec!["Sales[Region]".into()]).render(&model()).expect("render");

    assert_eq!(header(&table), vec!["Row Labels".to_string(), "Total Sales".to_string()]);
    assert_eq!(row(&table, 0), vec![text("(Blank)"), Scalar::Decimal(7.0)]);
    assert_eq!(row(&table, 1), vec![text("East"), Scalar::Decimal(45.0)]);
}

#[test]
fn row_and_column_cross_tab_puts_column_groups_in_columns() {
    let table = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_column_fields(vec!["Sales[Category]".into()])
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(header(&table), vec!["Region".to_string(), "Gadget".to_string(), "Widget".to_string()]);
    assert_eq!(table.rows.len(), 3);
    // BLANK has no Widget rows, so that cell stays blank rather than zero.
    assert_eq!(row(&table, 0), vec![Scalar::Blank, Scalar::Decimal(7.0), Scalar::Blank]);
    assert_eq!(row(&table, 1), vec![text("East"), Scalar::Decimal(30.0), Scalar::Decimal(15.0)]);
    assert_eq!(row(&table, 2), vec![text("West"), Scalar::Blank, Scalar::Decimal(20.0)]);
}

#[test]
fn several_values_produce_one_column_per_measure() {
    let table = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_values(vec!["Total Sales".into(), "Total Quantity".into()])
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(header(&table), vec!["Region".to_string(), "Total Sales".to_string(), "Total Quantity".to_string()]);
    assert_eq!(row(&table, 1), vec![text("East"), Scalar::Decimal(45.0), Scalar::Decimal(7.0)]);
}

#[test]
fn filters_keep_only_allowed_values() {
    let filtered = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_filter(PivotFilter::new("Sales[Region]", [text("East")]))
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(filtered.rows.len(), 1);
    assert_eq!(row(&filtered, 0), vec![text("East"), Scalar::Decimal(45.0)]);

    // Two allowed values, and BLANK can be allowed explicitly.
    let both = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_filter(PivotFilter::new("Sales[Region]", [text("East"), Scalar::Blank]))
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");
    assert_eq!(both.rows.len(), 2);
    assert_eq!(row(&both, 1), vec![text("East"), Scalar::Decimal(45.0)]);

    // Filters also apply to column groups and to the totals.
    let cross = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_column_fields(vec!["Sales[Category]".into()])
        .with_filter(PivotFilter::new("Sales[Category]", [text("Widget")]))
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");
    assert_eq!(header(&cross), vec!["Region".to_string(), "Widget".to_string(), "Grand Total".to_string()]);
    assert_eq!(cross.rows.len(), 3);
    assert_eq!(row(&cross, 0), vec![text("East"), Scalar::Decimal(15.0), Scalar::Decimal(15.0)]);
    assert_eq!(row(&cross, 1), vec![text("West"), Scalar::Decimal(20.0), Scalar::Decimal(20.0)]);
    assert_eq!(row(&cross, 2), vec![text("Grand Total"), Scalar::Decimal(35.0), Scalar::Decimal(35.0)]);

    // A filter that allows nothing yields no rows but keeps the header.
    let empty = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into()])
        .with_filter(PivotFilter::new("Sales[Region]", []))
        .render(&model())
        .expect("render");
    assert!(empty.rows.is_empty());
    assert_eq!(header(&empty), vec!["Row Labels".to_string(), "Total Sales".to_string()]);
}

#[test]
fn blank_column_groups_are_labeled() {
    let table = sales_pivot().with_column_fields(vec!["Sales[Region]".into()]).with_layout(PivotLayout::Tabular).render(&model()).expect("render");

    assert_eq!(header(&table), vec!["(Blank)".to_string(), "East".to_string(), "West".to_string()]);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(row(&table, 0), vec![Scalar::Decimal(7.0), Scalar::Decimal(45.0), Scalar::Decimal(20.0)]);
}

#[test]
fn blank_groups_are_their_own_group() {
    let table = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into(), "Sales[Category]".into()])
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(row(&table, 0), vec![Scalar::Blank, text("Gadget"), Scalar::Decimal(7.0)]);
}

#[test]
fn row_subtotals_and_grand_total() {
    let table = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into(), "Sales[Category]".into()])
        .with_row_subtotals(true)
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(header(&table), vec!["Region".to_string(), "Category".to_string(), "Total Sales".to_string()]);
    let rows = table.rows.clone();
    assert_eq!(rows.len(), 8);
    assert_eq!(rows[0], vec![Scalar::Blank, text("Gadget"), Scalar::Decimal(7.0)]);
    // Subtotal for the single BLANK region.
    assert_eq!(rows[1], vec![Scalar::Blank, Scalar::Blank, Scalar::Decimal(7.0)]);
    assert_eq!(rows[2], vec![text("East"), text("Gadget"), Scalar::Decimal(30.0)]);
    assert_eq!(rows[3], vec![text("East"), text("Widget"), Scalar::Decimal(15.0)]);
    // Subtotal for East (30 + 15).
    assert_eq!(rows[4], vec![text("East"), Scalar::Blank, Scalar::Decimal(45.0)]);
    assert_eq!(rows[5], vec![text("West"), text("Widget"), Scalar::Decimal(20.0)]);
    assert_eq!(rows[6], vec![text("West"), Scalar::Blank, Scalar::Decimal(20.0)]);
    // Grand total row: 7 + 30 + 15 + 20.
    assert_eq!(rows[7], vec![text("Grand Total"), Scalar::Blank, Scalar::Decimal(72.0)]);
}

#[test]
fn column_subtotals_and_grand_total_columns() {
    let table = sales_pivot()
        .with_column_fields(vec!["Sales[Category]".into(), "Sales[Region]".into()])
        .with_column_subtotals(true)
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(
        header(&table),
        vec![
            "Gadget | (Blank)".to_string(),
            "Gadget | East".to_string(),
            "Gadget Total".to_string(),
            "Widget | East".to_string(),
            "Widget | West".to_string(),
            "Widget Total".to_string(),
            "Grand Total".to_string(),
        ]
    );
    assert_eq!(table.rows.len(), 1);
    let row = row(&table, 0);
    assert_eq!(row[0], Scalar::Decimal(7.0));
    assert_eq!(row[1], Scalar::Decimal(30.0));
    assert_eq!(row[2], Scalar::Decimal(37.0));
    assert_eq!(row[3], Scalar::Decimal(15.0));
    assert_eq!(row[4], Scalar::Decimal(20.0));
    assert_eq!(row[5], Scalar::Decimal(35.0));
    assert_eq!(row[6], Scalar::Decimal(72.0));
}

#[test]
fn outline_layout_blanks_repeated_ancestors_and_names_subtotals() {
    let table = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into(), "Sales[Category]".into()])
        .with_row_subtotals(true)
        .with_layout(PivotLayout::Outline)
        .render(&model())
        .expect("render");

    let rows = table.rows.clone();
    assert_eq!(rows[0], vec![Scalar::Blank, text("Gadget"), Scalar::Decimal(7.0)]);
    // The subtotal of the BLANK region is named in its own column.
    assert_eq!(rows[1], vec![text("(Blank) Total"), Scalar::Blank, Scalar::Decimal(7.0)]);
    assert_eq!(rows[2], vec![text("East"), text("Gadget"), Scalar::Decimal(30.0)]);
    // "East" does not repeat on the second East leaf row.
    assert_eq!(rows[3], vec![Scalar::Blank, text("Widget"), Scalar::Decimal(15.0)]);
    assert_eq!(rows[4], vec![text("East Total"), Scalar::Blank, Scalar::Decimal(45.0)]);
}

#[test]
fn compact_layout_indents_nested_rows() {
    let table = sales_pivot()
        .with_row_fields(vec!["Sales[Region]".into(), "Sales[Category]".into()])
        .with_row_subtotals(true)
        .render(&model())
        .expect("render");

    assert_eq!(header(&table), vec!["Row Labels".to_string(), "Total Sales".to_string()]);
    assert_eq!(row(&table, 0), vec![text("  Gadget"), Scalar::Decimal(7.0)]);
    assert_eq!(row(&table, 1), vec![text("Region (Blank) Total"), Scalar::Decimal(7.0)]);
    assert_eq!(row(&table, 2), vec![text("  Gadget"), Scalar::Decimal(30.0)]);
    assert_eq!(row(&table, 3), vec![text("  Widget"), Scalar::Decimal(15.0)]);
    assert_eq!(row(&table, 4), vec![text("Region East Total"), Scalar::Decimal(45.0)]);
}

#[test]
fn grand_total_row_is_skipped_without_row_fields() {
    let table = sales_pivot().with_grand_totals(true).render(&model()).expect("render");
    assert_eq!(header(&table), vec!["Row Labels".to_string(), "Total Sales".to_string()]);
    assert_eq!(table.rows.len(), 1);
    assert_eq!(row(&table, 0), vec![text("Grand Total"), Scalar::Decimal(72.0)]);
}

#[test]
fn empty_model_reports_unknown_measure() {
    let error = sales_pivot().render(&DataModel::default()).expect_err("empty model");
    assert!(matches!(&error, PivotTableError::UnknownMeasure { measure, .. } if measure == "Total Sales"), "{error:?}");

    let no_source = PivotTable::default().render(&model()).expect_err("no source");
    assert!(matches!(no_source, PivotTableError::MissingSource { .. }), "{no_source:?}");

    let homeless = PivotTable::new("homeless", "Homeless").render(&model()).expect_err("no home table");
    assert!(matches!(homeless, PivotTableError::NoSourceTable { .. }), "{homeless:?}");
}

#[test]
fn invalid_fields_are_reported_not_panicked() {
    let unknown_table = PivotTable::new("p", "Total Sales").with_source_table("Missing").render(&model()).expect_err("unknown table");
    assert!(matches!(unknown_table, PivotTableError::UnknownTable(_)), "{unknown_table:?}");

    let unknown_column = sales_pivot().with_row_fields(vec!["Sales[Nope]".into()]).render(&model()).expect_err("unknown column");
    assert!(matches!(unknown_column, PivotTableError::Pivot(_)), "{unknown_column:?}");

    let foreign = sales_pivot().with_row_fields(vec!["Sales[Region]".into(), "Other[Region]".into()]).render(&model()).expect_err("foreign");
    // `Other[Region]` does not bind at all without that table.
    assert!(matches!(foreign, PivotTableError::Pivot(_)), "{foreign:?}");

    let two_tables = DataModel {
        tables: vec![
            model().tables[0].clone(),
            ModelTable { name: "Other".into(), columns: vec![ModelColumn { name: "Region".into(), data_type: None }], rows: Vec::new() },
        ],
        ..model()
    };
    let foreign = sales_pivot().with_row_fields(vec!["Sales[Region]".into(), "Other[Region]".into()]).render(&two_tables).expect_err("foreign");
    assert!(matches!(foreign, PivotTableError::ForeignField { .. }), "{foreign:?}");

    let bad_filter = sales_pivot().with_filter(PivotFilter::new("Sales[Nope]", [text("East")])).render(&model()).expect_err("bad filter");
    assert!(matches!(bad_filter, PivotTableError::Pivot(_)), "{bad_filter:?}");
}

#[test]
fn rendering_is_bounded_by_max_cells() {
    let pivot = sales_pivot().with_row_fields(vec!["Sales[Region]".into()]).with_max_cells(2);
    let error = pivot.render(&model()).expect_err("over budget");
    assert!(matches!(&error, PivotTableError::TooManyCells { limit: 2, .. }), "{error:?}");

    let default_pivot = sales_pivot().with_row_fields(vec!["Sales[Region]".into()]);
    assert_eq!(default_pivot.max_cells, DEFAULT_MAX_CELLS);
    assert!(default_pivot.render(&model()).is_ok());
}
