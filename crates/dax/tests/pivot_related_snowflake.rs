//! Pivot groups over a snowflake: `Sales -> Product -> Category`, two hops from the fact table.
//!
//! ```text
//! Sales (fact)                 Product (dimension)          Category (dimension)
//! ProductKey Amount            ProductKey Name       CategoryKey   CategoryKey Category
//!          1    100                     1 Widget            10            10 Hardware
//!          1     50                     2 Gadget            20            20 Software
//!          2     25                     4 Doohickey         10
//!          3     40   <- ProductKey 3 has no Product row (orphan key)
//! ```
//!
//! `Sales[ProductKey] -> Product[ProductKey]` and `Product[CategoryKey] -> Category[CategoryKey]`
//! are active, so `Category` is reachable from `Sales` only through `Product`.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::pivot_table::{PivotFilter, PivotLayout, PivotTable};
use gridcraft_dax::value::{Scalar, TableValue};

fn columns(names: &[(&str, &str)]) -> Vec<ModelColumn> {
    names.iter().map(|(name, kind)| ModelColumn { name: (*name).into(), data_type: Some((*kind).into()) }).collect()
}

fn relationship(from_table: &str, from_column: &str, to_table: &str, to_column: &str) -> Relationship {
    Relationship {
        from_table: from_table.into(),
        from_column: from_column.into(),
        to_table: to_table.into(),
        to_column: to_column.into(),
        bidirectional: false,
        inactive: false,
    }
}

fn text(value: &str) -> Scalar {
    Scalar::String(value.into())
}

fn int(value: i64) -> Scalar {
    Scalar::Integer(value)
}

fn dec(value: f64) -> Scalar {
    Scalar::Decimal(value)
}

fn header(table: &TableValue) -> Vec<String> {
    table.columns.iter().map(|column| column.name.clone()).collect()
}

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: columns(&[("ProductKey", "Int64"), ("Amount", "Double")]),
                rows: vec![vec![int(1), int(100)], vec![int(1), int(50)], vec![int(2), int(25)], vec![int(3), int(40)]],
            },
            ModelTable {
                name: "Product".into(),
                columns: columns(&[("ProductKey", "Int64"), ("Name", "String"), ("CategoryKey", "Int64")]),
                rows: vec![vec![int(1), text("Widget"), int(10)], vec![int(2), text("Gadget"), int(20)], vec![int(4), text("Doohickey"), int(10)]],
            },
            ModelTable {
                name: "Category".into(),
                columns: columns(&[("CategoryKey", "Int64"), ("Category", "String")]),
                rows: vec![vec![int(10), text("Hardware")], vec![int(20), text("Software")]],
            },
        ],
        relationships: vec![
            relationship("Sales", "ProductKey", "Product", "ProductKey"),
            relationship("Product", "CategoryKey", "Category", "CategoryKey"),
        ],
        measures: vec![Measure {
            name: "Total Sales".into(),
            formula: "SUM(Sales[Amount])".into(),
            table: Some("Sales".into()),
            format: None,
            expr: None,
        }],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn pivot() -> PivotTable {
    PivotTable::new("snowflake", "Total Sales")
}

#[test]
fn a_two_hop_related_field_groups_and_filters_through_both_hops() {
    let table = pivot().with_row_fields(vec!["Category[Category]".into()]).with_layout(PivotLayout::Tabular).render(&model()).expect("render");

    // Widget (key 1) is Hardware: 100 + 50. Gadget (key 2) is Software: 25. The orphan key 3
    // reaches no Product row, so it groups as BLANK and its cell stays empty.
    assert_eq!(header(&table), vec!["Category[Category]".to_string(), "Total Sales".to_string()]);
    assert_eq!(table.rows, vec![vec![Scalar::Blank, Scalar::Blank], vec![text("Hardware"), dec(150.0)], vec![text("Software"), dec(25.0)]]);
}

#[test]
fn one_hop_and_two_hop_row_fields_mix() {
    let table = pivot()
        .with_row_fields(vec!["Product[Name]".into(), "Category[Category]".into()])
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(header(&table), vec!["Product[Name]".to_string(), "Category[Category]".to_string(), "Total Sales".to_string()]);
    assert_eq!(
        table.rows,
        vec![
            vec![Scalar::Blank, Scalar::Blank, Scalar::Blank],
            vec![text("Gadget"), text("Software"), dec(25.0)],
            vec![text("Widget"), text("Hardware"), dec(150.0)],
        ]
    );
}

#[test]
fn a_dimension_row_without_fact_rows_is_not_a_group() {
    let table = pivot().with_row_fields(vec!["Product[Name]".into()]).with_layout(PivotLayout::Tabular).render(&model()).expect("render");

    // Doohickey (ProductKey 4) has no fact row; the cells come from the fact table, so it never
    // appears. The orphan key still does, as BLANK.
    assert_eq!(table.rows, vec![vec![Scalar::Blank, Scalar::Blank], vec![text("Gadget"), dec(25.0)], vec![text("Widget"), dec(150.0)]]);
}

#[test]
fn a_filter_two_hops_away_restricts_the_fact_table() {
    let filtered = pivot()
        .with_row_fields(vec!["Product[Name]".into()])
        .with_filter(PivotFilter::new("Category[Category]", [text("Software")]))
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    // Only Gadget is Software, and only the two Widget rows are Hardware.
    assert_eq!(header(&filtered), vec!["Product[Name]".to_string(), "Total Sales".to_string()]);
    assert_eq!(filtered.rows, vec![vec![text("Gadget"), dec(25.0)]]);

    let hardware = pivot()
        .with_row_fields(vec!["Category[Category]".into()])
        .with_filter(PivotFilter::new("Category[Category]", [text("Hardware")]))
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");
    assert_eq!(hardware.rows, vec![vec![text("Hardware"), dec(150.0)]]);
}

#[test]
fn the_grand_total_spans_the_fact_table_across_the_snowflake() {
    let table = pivot()
        .with_row_fields(vec!["Category[Category]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(
        table.rows,
        vec![
            vec![Scalar::Blank, Scalar::Blank],
            vec![text("Hardware"), dec(150.0)],
            vec![text("Software"), dec(25.0)],
            vec![text("Grand Total"), dec(215.0)],
        ]
    );
}
