//! Related pivot fields that must be **rejected**, never guessed and never panicked:
//! unrelated tables, two relationships between the same tables, a second chain (a cycle in the
//! relationship graph), inactive relationships and one-to-many paths.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::pivot_table::{PivotTable, PivotTableError};
use gridcraft_dax::value::Scalar;

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

fn sales() -> ModelTable {
    ModelTable {
        name: "Sales".into(),
        columns: columns(&[("ProductKey", "Int64"), ("AltKey", "Int64"), ("RegionKey", "Int64"), ("Amount", "Double")]),
        rows: vec![vec![Scalar::Integer(1), Scalar::Integer(1), Scalar::Integer(1), Scalar::Integer(10)]],
    }
}

fn product() -> ModelTable {
    ModelTable {
        name: "Product".into(),
        columns: columns(&[("ProductKey", "Int64"), ("AltKey", "Int64"), ("Name", "String"), ("RegionKey", "Int64")]),
        rows: vec![vec![Scalar::Integer(1), Scalar::Integer(1), text("Widget"), Scalar::Integer(1)]],
    }
}

fn region() -> ModelTable {
    ModelTable {
        name: "Region".into(),
        columns: columns(&[("RegionKey", "Int64"), ("Name", "String")]),
        rows: vec![vec![Scalar::Integer(1), text("North")]],
    }
}

fn other() -> ModelTable {
    ModelTable { name: "Other".into(), columns: columns(&[("Code", "String")]), rows: vec![vec![text("x")]] }
}

fn measure() -> Measure {
    Measure { name: "Total Sales".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None }
}

fn model(tables: Vec<ModelTable>, relationships: Vec<Relationship>) -> DataModel {
    let mut model = DataModel { tables, relationships, measures: vec![measure()] };
    model.parse_formulas().expect("model parses");
    model
}

#[test]
fn a_field_on_an_unrelated_table_is_rejected() {
    let model = model(vec![sales(), product(), other()], vec![relationship("Sales", "ProductKey", "Product", "ProductKey")]);

    let error = PivotTable::new("p", "Total Sales").with_row_fields(vec!["Other[Code]".into()]).render(&model).expect_err("unrelated");
    assert!(matches!(&error, PivotTableError::ForeignField { field, table } if field == "Code" && table == "Sales"), "{error:?}");
    assert!(error.to_string().contains("no active relationship"), "{error}");

    // A filter is bound the same way, so it is rejected too.
    let filter = PivotTable::new("p", "Total Sales")
        .with_filter(gridcraft_dax::pivot_table::PivotFilter::new("Other[Code]", [text("x")]))
        .render(&model)
        .expect_err("unrelated filter");
    assert!(matches!(filter, PivotTableError::ForeignField { .. }), "{filter:?}");
}

#[test]
fn two_relationships_between_the_same_tables_are_ambiguous() {
    // `Sales[ProductKey]` and `Sales[AltKey]` both point at `Product[ProductKey]`. A pivot field
    // names a table and a column, not a relationship, so the lookup cannot pick one.
    let model = model(
        vec![sales(), product()],
        vec![relationship("Sales", "ProductKey", "Product", "ProductKey"), relationship("Sales", "AltKey", "Product", "ProductKey")],
    );

    let error = PivotTable::new("p", "Total Sales").with_row_fields(vec!["Product[Name]".into()]).render(&model).expect_err("ambiguous");
    assert!(
        matches!(&error, PivotTableError::AmbiguousFieldPath { field, table, source_table, paths } if field == "Name" && table == "Product" && source_table == "Sales" && *paths == 2),
        "{error:?}"
    );
    assert!(error.to_string().contains("2 relationship chains"), "{error}");
}

#[test]
fn a_cycle_in_the_relationship_graph_is_rejected_as_ambiguous() {
    // Sales -> Product -> Region plus Sales -> Region: two chains reach Region, so the pivot cannot
    // tell which lookup the field means. The search is bounded and never loops.
    let mut product = product();
    product.columns = columns(&[("ProductKey", "Int64"), ("Name", "String"), ("RegionKey", "Int64")]);
    product.rows = vec![vec![Scalar::Integer(1), text("Widget"), Scalar::Integer(1)]];
    let sales = ModelTable {
        name: "Sales".into(),
        columns: columns(&[("ProductKey", "Int64"), ("RegionKey", "Int64"), ("Amount", "Double")]),
        rows: vec![vec![Scalar::Integer(1), Scalar::Integer(1), Scalar::Integer(10)]],
    };
    let model = model(
        vec![sales, product, region()],
        vec![
            relationship("Sales", "ProductKey", "Product", "ProductKey"),
            relationship("Product", "RegionKey", "Region", "RegionKey"),
            relationship("Sales", "RegionKey", "Region", "RegionKey"),
        ],
    );

    let error = PivotTable::new("p", "Total Sales").with_row_fields(vec!["Region[Name]".into()]).render(&model).expect_err("cycle");
    assert!(matches!(&error, PivotTableError::AmbiguousFieldPath { paths, .. } if *paths == 2), "{error:?}");
}

#[test]
fn an_inactive_relationship_is_rejected_and_named() {
    let mut inactive = relationship("Sales", "ProductKey", "Product", "ProductKey");
    inactive.inactive = true;
    let model = model(vec![sales(), product()], vec![inactive]);

    let error = PivotTable::new("p", "Total Sales").with_row_fields(vec!["Product[Name]".into()]).render(&model).expect_err("inactive");
    assert!(
        matches!(&error, PivotTableError::InactiveRelationship { field, table, source_table } if field == "Name" && table == "Product" && source_table == "Sales"),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("inactive relationship") && message.contains("USERELATIONSHIP"), "{message}");
}

#[test]
fn a_field_reachable_only_from_the_one_side_is_rejected() {
    // The pivot groups `Product` (source_table) and asks for a field on the many side, `Sales`.
    // One Product row reaches many Sales rows, so it has no single value to group by.
    let model = model(vec![sales(), product()], vec![relationship("Sales", "ProductKey", "Product", "ProductKey")]);

    let error = PivotTable::new("p", "Total Sales")
        .with_source_table("Product")
        .with_row_fields(vec!["Sales[Amount]".into()])
        .render(&model)
        .expect_err("one-to-many");
    assert!(
        matches!(&error, PivotTableError::FanOutField { field, table, source_table } if field == "Amount" && table == "Sales" && source_table == "Product"),
        "{error:?}"
    );
    assert!(error.to_string().contains("many-to-one"), "{error}");
}

#[test]
fn a_bidirectional_relationship_still_groups_by_lookup() {
    // Bidirectional only widens filter propagation; the lookup direction is unchanged, so the
    // star still groups and filters from the fact side.
    let mut both = relationship("Sales", "ProductKey", "Product", "ProductKey");
    both.bidirectional = true;
    let model = model(vec![sales(), product()], vec![both]);

    let table = PivotTable::new("p", "Total Sales").with_row_fields(vec!["Product[Name]".into()]).render(&model).expect("render");
    assert_eq!(table.rows, vec![vec![text("Widget"), Scalar::Decimal(10.0)]]);
}
