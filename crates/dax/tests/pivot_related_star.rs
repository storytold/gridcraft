//! Pivot groups over a star schema: one fact table and two dimensions of different cardinality.
//!
//! ```text
//! Sales (fact)                        Product (dimension, 3 rows)          Store (dimension, 2 rows)
//! ProductKey StoreKey Amount Quantity  ProductKey Name       Color          StoreKey City
//!          1       10    100        1           1 Widget     Red                  10 North
//!          1       20     50        2           2 Gadget     Blue                 20 South
//!          2       10     25        3           4 Doohickey  Red
//!          3       10     40        4   <- ProductKey 3 has no Product row (orphan key)
//!          2       20     10        1
//!          1       10     30        1
//! ```
//!
//! `Sales[ProductKey] -> Product[ProductKey]` and `Sales[StoreKey] -> Store[StoreKey]` are the two
//! active relationships.

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

fn measure(name: &str, formula: &str, table: &str) -> Measure {
    Measure { name: name.into(), formula: formula.into(), table: Some(table.into()), format: None, expr: None }
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
                columns: columns(&[("ProductKey", "Int64"), ("StoreKey", "Int64"), ("Amount", "Double"), ("Quantity", "Int64")]),
                rows: vec![
                    vec![int(1), int(10), int(100), int(1)],
                    vec![int(1), int(20), int(50), int(2)],
                    vec![int(2), int(10), int(25), int(3)],
                    vec![int(3), int(10), int(40), int(4)],
                    vec![int(2), int(20), int(10), int(1)],
                    vec![int(1), int(10), int(30), int(1)],
                ],
            },
            ModelTable {
                name: "Product".into(),
                columns: columns(&[("ProductKey", "Int64"), ("Name", "String"), ("Color", "String")]),
                rows: vec![
                    vec![int(1), text("Widget"), text("Red")],
                    vec![int(2), text("Gadget"), text("Blue")],
                    vec![int(4), text("Doohickey"), text("Red")],
                ],
            },
            ModelTable {
                name: "Store".into(),
                columns: columns(&[("StoreKey", "Int64"), ("City", "String")]),
                rows: vec![vec![int(10), text("North")], vec![int(20), text("South")]],
            },
        ],
        relationships: vec![relationship("Sales", "ProductKey", "Product", "ProductKey"), relationship("Sales", "StoreKey", "Store", "StoreKey")],
        measures: vec![measure("Total Sales", "SUM(Sales[Amount])", "Sales"), measure("Total Quantity", "SUM(Sales[Quantity])", "Sales")],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn pivot() -> PivotTable {
    PivotTable::new("star", "Total Sales")
}

#[test]
fn related_row_field_groups_by_the_dimension_value() {
    let table = pivot().with_row_fields(vec!["Product[Name]".into()]).with_layout(PivotLayout::Tabular).render(&model()).expect("render");

    // A related field is labeled with its table, so two dimensions with the same column name stay
    // apart. ProductKey 3 has no Product row: its fact row groups as BLANK, and the cell is BLANK
    // because the BLANK member selects no related row, so the relationship filter keeps nothing.
    assert_eq!(header(&table), vec!["Product[Name]".to_string(), "Total Sales".to_string()]);
    assert_eq!(table.rows, vec![vec![Scalar::Blank, Scalar::Blank], vec![text("Gadget"), dec(35.0)], vec![text("Widget"), dec(180.0)],]);
    assert_eq!(table.lineage, Some(0), "the rendered table keeps the source table's lineage");
}

#[test]
fn two_dimensions_cross_tabulate_with_two_measures() {
    let table = pivot()
        .with_row_fields(vec!["Product[Name]".into()])
        .with_column_fields(vec!["Store[City]".into()])
        .with_values(vec!["Total Sales".into(), "Total Quantity".into()])
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    // Label columns come first, then one column per column group and measure.
    assert_eq!(
        header(&table),
        vec![
            "Product[Name]".to_string(),
            "North | Total Sales".to_string(),
            "North | Total Quantity".to_string(),
            "South | Total Sales".to_string(),
            "South | Total Quantity".to_string(),
        ]
    );
    assert_eq!(
        table.rows,
        vec![
            vec![Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank],
            vec![text("Gadget"), dec(25.0), dec(3.0), dec(10.0), dec(1.0)],
            // Widget x North: 100 + 30, quantities 1 + 1.
            vec![text("Widget"), dec(130.0), dec(2.0), dec(50.0), dec(2.0)],
        ]
    );
}

#[test]
fn two_related_row_fields_order_blank_first_and_total_every_fact_row() {
    let table = pivot()
        .with_row_fields(vec!["Product[Name]".into(), "Store[City]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(header(&table), vec!["Product[Name]".to_string(), "Store[City]".to_string(), "Total Sales".to_string()]);
    assert_eq!(
        table.rows,
        vec![
            vec![Scalar::Blank, text("North"), Scalar::Blank],
            vec![text("Gadget"), text("North"), dec(25.0)],
            vec![text("Gadget"), text("South"), dec(10.0)],
            vec![text("Widget"), text("North"), dec(130.0)],
            vec![text("Widget"), text("South"), dec(50.0)],
            // The grand total constrains no dimension, so it is the whole fact table.
            vec![text("Grand Total"), Scalar::Blank, dec(255.0)],
        ]
    );
}

#[test]
fn a_filter_on_a_dimension_field_restricts_the_fact_table() {
    let filtered = pivot()
        .with_row_fields(vec!["Store[City]".into()])
        .with_filter(PivotFilter::new("Product[Name]", [text("Widget")]))
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    // Only Widget's fact rows (100 + 50 + 30) survive, so North is 130 and South is 50.
    assert_eq!(header(&filtered), vec!["Store[City]".to_string(), "Total Sales".to_string()]);
    assert_eq!(filtered.rows, vec![vec![text("North"), dec(130.0)], vec![text("South"), dec(50.0)]]);

    let by_store = pivot()
        .with_row_fields(vec!["Product[Name]".into()])
        .with_filter(PivotFilter::new("Store[City]", [text("North")]))
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");
    assert_eq!(by_store.rows, vec![vec![Scalar::Blank, Scalar::Blank], vec![text("Gadget"), dec(25.0)], vec![text("Widget"), dec(130.0)]]);

    // A filter that matches no dimension row keeps no fact row, but the header survives.
    let nothing = pivot()
        .with_row_fields(vec!["Product[Name]".into()])
        .with_filter(PivotFilter::new("Product[Name]", [text("Nope")]))
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");
    assert!(nothing.rows.is_empty());
    assert_eq!(header(&nothing), vec!["Product[Name]".to_string(), "Total Sales".to_string()]);
}

#[test]
fn same_table_fields_are_unchanged_by_the_relationships() {
    // Before related fields existed this grid was the only supported shape; it must not move.
    let table = pivot()
        .with_row_fields(vec!["Sales[ProductKey]".into()])
        .with_column_fields(vec!["Sales[StoreKey]".into()])
        .with_layout(PivotLayout::Tabular)
        .render(&model())
        .expect("render");

    assert_eq!(header(&table), vec!["ProductKey".to_string(), "10".to_string(), "20".to_string()]);
    assert_eq!(table.rows, vec![vec![int(1), dec(130.0), dec(50.0)], vec![int(2), dec(25.0), dec(10.0)], vec![int(3), dec(40.0), Scalar::Blank],]);
}

/// A model whose dimension repeats a key. The one side of a relationship must be unique; when it is
/// not, the pivot resolves each fact row to the **lowest** matching dimension row, so no fact row is
/// fanned out and no value is counted twice.
fn duplicate_key_model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: columns(&[("ProductKey", "Int64"), ("Amount", "Double")]),
                rows: vec![vec![int(1), int(10)], vec![int(1), int(20)], vec![int(2), int(5)]],
            },
            ModelTable {
                name: "Product".into(),
                columns: columns(&[("ProductKey", "Int64"), ("Name", "String")]),
                rows: vec![vec![int(1), text("Alpha")], vec![int(1), text("Beta")], vec![int(2), text("Gamma")]],
            },
        ],
        relationships: vec![relationship("Sales", "ProductKey", "Product", "ProductKey")],
        measures: vec![measure("Total Sales", "SUM(Sales[Amount])", "Sales")],
    };
    model.parse_formulas().expect("model parses");
    model
}

#[test]
fn a_duplicate_dimension_key_resolves_to_the_lowest_row_and_never_doubles_a_fact_row() {
    let table = PivotTable::new("duplicates", "Total Sales")
        .with_row_fields(vec!["Product[Name]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular)
        .render(&duplicate_key_model())
        .expect("render");

    // Both key-1 fact rows reach Product row 0 ("Alpha"), so Beta is no group at all and the two
    // rows are summed once: 10 + 20. The grand total proves nothing was duplicated.
    assert_eq!(header(&table), vec!["Product[Name]".to_string(), "Total Sales".to_string()]);
    assert_eq!(table.rows, vec![vec![text("Alpha"), dec(30.0)], vec![text("Gamma"), dec(5.0)], vec![text("Grand Total"), dec(35.0)]]);
}

#[test]
fn an_empty_dimension_table_groups_every_fact_row_as_blank() {
    let mut model = model();
    model.tables[1].rows.clear();

    let table = pivot()
        .with_row_fields(vec!["Product[Name]".into()])
        .with_grand_totals(true)
        .with_layout(PivotLayout::Tabular)
        .render(&model)
        .expect("render");

    // Every key is an orphan, so every fact row groups as BLANK. The relationship has no BLANK
    // member to attach them to, so the leaf cell is the empty-set measure (BLANK) while the grand
    // total, which constrains no dimension, still sums the fact table.
    assert_eq!(header(&table), vec!["Product[Name]".to_string(), "Total Sales".to_string()]);
    assert_eq!(table.rows, vec![vec![Scalar::Blank, Scalar::Blank], vec![text("Grand Total"), dec(255.0)]]);
}

#[test]
fn related_grouping_survives_an_unfaithful_model() {
    let mut model = model();
    // A boolean/integer mix promotes the column to a dictionary, so the columnar copy is lossless
    // only in the storage's own terms: the pivot must group from the model rows and follow the
    // relationship by key, with the same grid and with related filters still propagating.
    model.tables[1].columns.push(ModelColumn { name: "Rank".into(), data_type: Some("Boolean".into()) });
    model.tables[1].rows[0].push(Scalar::Boolean(true));
    model.tables[1].rows[1].push(int(2));
    model.tables[1].rows[2].push(Scalar::Boolean(false));

    let (table, stats) =
        pivot().with_row_fields(vec!["Product[Name]".into()]).with_layout(PivotLayout::Tabular).render_with_stats(&model).expect("render");
    assert_eq!(table.rows, vec![vec![Scalar::Blank, Scalar::Blank], vec![text("Gadget"), dec(35.0)], vec![text("Widget"), dec(180.0)]]);
    assert_eq!(stats.pivot_group_rows, 0, "grouping fell back to the model rows: {stats:?}");
    assert!(stats.row_scan_fallbacks > 0, "the unfaithful model must be reported: {stats:?}");

    let filtered = pivot()
        .with_row_fields(vec!["Store[City]".into()])
        .with_filter(PivotFilter::new("Product[Name]", [text("Widget")]))
        .with_layout(PivotLayout::Tabular)
        .render(&model)
        .expect("render");
    assert_eq!(filtered.rows, vec![vec![text("North"), dec(130.0)], vec![text("South"), dec(50.0)]]);
}
