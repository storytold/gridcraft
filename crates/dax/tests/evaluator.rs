//! Core evaluator tests over an in-memory model.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, FilterContext, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Flag".into(), data_type: Some("Boolean".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::Integer(2), Scalar::Boolean(true)],
                    vec![Scalar::String("West".into()), Scalar::Integer(20), Scalar::Integer(3), Scalar::Boolean(false)],
                    vec![Scalar::String("East".into()), Scalar::Integer(30), Scalar::Integer(4), Scalar::Boolean(true)],
                ],
            },
            ModelTable { name: "Date".into(), columns: vec![ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) }], rows: vec![] },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "Region".into(),
            to_table: "Date".into(),
            to_column: "Date".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![
            Measure { name: "Total Sales".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure {
                name: "East Sales".into(),
                formula: "CALCULATE([Total Sales], Sales[Region] = \"East\")".into(),
                table: Some("Sales".into()),
                format: None,
                expr: None,
            },
            Measure { name: "Double Sales".into(), formula: "[Total Sales] * 2".into(), table: Some("Sales".into()), format: None, expr: None },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn eval(src: &str) -> Scalar {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression(src).expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    evaluator.evaluate(&bound).expect("evaluate")
}

fn eval_table(src: &str) -> TableValue {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression(src).expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    evaluator.evaluate_table(&bound).expect("table")
}

#[test]
fn aggregates_and_measures() {
    assert_eq!(eval("SUM(Sales[Amount])"), Scalar::Decimal(60.0));
    assert_eq!(eval("AVERAGE(Sales[Amount])"), Scalar::Decimal(20.0));
    assert_eq!(eval("COUNTROWS(Sales)"), Scalar::Integer(3));
    assert_eq!(eval("SUMX(Sales, Sales[Amount] * 2)"), Scalar::Decimal(120.0));
    assert_eq!(eval("[Total Sales]"), Scalar::Decimal(60.0));
    assert_eq!(eval("[East Sales]"), Scalar::Decimal(40.0));
    assert_eq!(eval("[Double Sales]"), Scalar::Decimal(120.0));
    assert_eq!(eval("CALCULATE([Total Sales], Sales[Region] = \"West\")"), Scalar::Decimal(20.0));
}

#[test]
fn logical_and_scalar_functions() {
    assert_eq!(eval("IF([Total Sales] > 50, \"big\", \"small\")"), Scalar::String("big".into()));
    assert_eq!(eval("SWITCH(2, 1, \"one\", 2, \"two\", \"other\")"), Scalar::String("two".into()));
    assert_eq!(eval("COALESCE(BLANK(), 3)"), Scalar::Integer(3));
    assert_eq!(eval("LEFT(\"abcd\", 2)"), Scalar::String("ab".into()));
    assert_eq!(eval("ROUND(1.234, 2)"), Scalar::Decimal(1.23));
    assert_eq!(eval("DATE(2024, 1, 2)"), Scalar::DateTime(45293.0));
    assert_eq!(eval("YEAR(DATE(2024, 1, 2))"), Scalar::Integer(2024));
    assert_eq!(eval("ISBLANK(BLANK())"), Scalar::Boolean(true));
}

#[test]
fn tables_filters_and_values() {
    let filtered = eval_table("FILTER(Sales, Sales[Amount] > 10)");
    assert_eq!(filtered.rows.len(), 2);
    let values = eval_table("VALUES(Sales[Region])");
    assert_eq!(values.rows.len(), 2);
}

#[test]
fn documented_keyword_arguments_bind() {
    let model = model();
    let binder = Binder::new(&model);
    for src in ["DATEADD('Date'[Date], -1, MONTH)", "CONVERT(1, INTEGER)"] {
        let expr = parse_expression(src).expect("parse");
        binder.bind_expression(&expr).expect("bind keyword argument");
    }
}

#[test]
fn filter_context_can_be_supplied() {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("[Total Sales]").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    let mut filters = FilterContext::default();
    filters.set(0, 0, [gridcraft_dax::ScalarKey::String("East".into())]);
    assert_eq!(evaluator.evaluate_with(&bound, filters).expect("eval"), Scalar::Decimal(40.0));
}

#[test]
fn pivot_groups_and_evaluates_measures() {
    let model = model();
    let fields = gridcraft_dax::pivot_fields(&model, &["Sales[Region]".into()]).expect("fields");
    let table = gridcraft_dax::pivot(&model, "Total Sales", &fields).expect("pivot");
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][0], Scalar::String("East".into()));
    assert_eq!(table.rows[0][1], Scalar::Decimal(40.0));
    assert_eq!(table.rows[1][0], Scalar::String("West".into()));
    assert_eq!(table.rows[1][1], Scalar::Decimal(20.0));
}

#[test]
fn iterators_resolve_rows_of_filtered_tables() {
    // The frame must carry the row values, not the position in the filtered table.
    assert_eq!(eval("SUMX(FILTER(Sales, Sales[Region] = \"West\"), Sales[Amount])"), Scalar::Decimal(20.0));
    // Context transition inside an iterator over a filtered table.
    assert_eq!(eval("SUMX(FILTER(Sales, Sales[Amount] > 10), [Total Sales])"), Scalar::Decimal(50.0));
}

#[test]
fn calculate_accepts_filter_table_arguments() {
    assert_eq!(eval("CALCULATE([Total Sales], FILTER(Sales, Sales[Amount] > 10))"), Scalar::Decimal(50.0));
    assert_eq!(eval("CALCULATE([Total Sales], Sales[Amount] >= 20)"), Scalar::Decimal(50.0));
    assert_eq!(eval("CALCULATE([Total Sales], 20 <= Sales[Amount])"), Scalar::Decimal(50.0));
    assert_eq!(eval("CALCULATE([Total Sales], Sales[Amount] < 20)"), Scalar::Decimal(10.0));
    assert_eq!(eval("CALCULATE([Total Sales], KEEPFILTERS(Sales[Region] = \"East\"))"), Scalar::Decimal(40.0));
}

#[test]
fn cyclic_measures_are_rejected_before_evaluation() {
    let mut model = model();
    model.measures.push(Measure { name: "Cycle A".into(), formula: "[Cycle B] + 1".into(), table: Some("Sales".into()), format: None, expr: None });
    model.measures.push(Measure { name: "Cycle B".into(), formula: "[Cycle A] + 1".into(), table: Some("Sales".into()), format: None, expr: None });
    model.parse_formulas().expect("model parses");
    let error = match Evaluator::new(&model) {
        Ok(_) => panic!("cyclic measures must be rejected"),
        Err(error) => error,
    };
    assert!(error.message.contains("Cycle"), "{error:?}");
}

#[test]
fn bidirectional_filters_do_not_recurse() {
    let mut model = model();
    model.tables.push(ModelTable {
        name: "Products".into(),
        columns: vec![ModelColumn { name: "Region".into(), data_type: Some("String".into()) }],
        rows: vec![vec![Scalar::String("East".into())], vec![Scalar::String("West".into())]],
    });
    model.relationships.push(Relationship {
        from_table: "Sales".into(),
        from_column: "Region".into(),
        to_table: "Products".into(),
        to_column: "Region".into(),
        bidirectional: true,
        inactive: false,
    });
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("CALCULATE(SUM(Sales[Amount]), Products[Region] = \"East\")").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound).expect("bidirectional filter"), Scalar::Decimal(40.0));

    model.relationships.last_mut().expect("relationship").inactive = true;
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound).expect("inactive relationship is ignored"), Scalar::Decimal(60.0));
}

#[test]
fn relatedtable_returns_rows_from_the_many_side() {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(10), Scalar::String("A".into())],
                    vec![Scalar::Integer(20), Scalar::String("A".into())],
                    vec![Scalar::Integer(30), Scalar::String("B".into())],
                ],
            },
            ModelTable {
                name: "Products".into(),
                columns: vec![ModelColumn { name: "Product".into(), data_type: Some("String".into()) }],
                rows: vec![vec![Scalar::String("A".into())], vec![Scalar::String("B".into())]],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "Product".into(),
            to_table: "Products".into(),
            to_column: "Product".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("SUMX(Products, COUNTROWS(RELATEDTABLE(Sales)))").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound).expect("relatedtable"), Scalar::Decimal(3.0));
}

#[test]
fn filters_propagate_across_several_relationships() {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                ],
                rows: vec![vec![Scalar::Integer(10), Scalar::String("A".into())], vec![Scalar::Integer(20), Scalar::String("B".into())]],
            },
            ModelTable {
                name: "Products".into(),
                columns: vec![
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("A".into()), Scalar::String("East".into())],
                    vec![Scalar::String("B".into()), Scalar::String("West".into())],
                ],
            },
            ModelTable {
                name: "Categories".into(),
                columns: vec![
                    ModelColumn { name: "Category".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::String("Group East".into())],
                    vec![Scalar::String("West".into()), Scalar::String("Group West".into())],
                ],
            },
        ],
        relationships: vec![
            Relationship {
                from_table: "Sales".into(),
                from_column: "Product".into(),
                to_table: "Products".into(),
                to_column: "Product".into(),
                bidirectional: false,
                inactive: false,
            },
            Relationship {
                from_table: "Products".into(),
                from_column: "Category".into(),
                to_table: "Categories".into(),
                to_column: "Category".into(),
                bidirectional: false,
                inactive: false,
            },
        ],
        measures: vec![Measure { name: "Total".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None }],
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("CALCULATE([Total], Categories[Name] = \"Group East\")").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound).expect("multi-hop filter"), Scalar::Decimal(10.0));
}

#[test]
fn bidirectional_filters_keep_orphan_rows() {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(10), Scalar::String("A".into()), Scalar::String("East".into())],
                    // No matching Products row: a bidirectional relationship must keep it.
                    vec![Scalar::Integer(20), Scalar::String("Orphan".into()), Scalar::String("East".into())],
                ],
            },
            ModelTable {
                name: "Products".into(),
                columns: vec![ModelColumn { name: "Product".into(), data_type: Some("String".into()) }],
                rows: vec![vec![Scalar::String("A".into())]],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "Product".into(),
            to_table: "Products".into(),
            to_column: "Product".into(),
            bidirectional: true,
            inactive: false,
        }],
        measures: vec![Measure { name: "Total".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None }],
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("CALCULATE([Total], Sales[Region] = \"East\")").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert_eq!(evaluator.evaluate(&bound).expect("bidirectional filter"), Scalar::Decimal(30.0));
}

#[test]
fn userelationship_selects_the_matching_relationship_by_column() {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "OrderDate".into(), data_type: Some("DateTime".into()) },
                    ModelColumn { name: "ShipDate".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(10), Scalar::DateTime(45292.0), Scalar::DateTime(45293.0)],
                    vec![Scalar::Integer(20), Scalar::DateTime(45293.0), Scalar::DateTime(45292.0)],
                ],
            },
            ModelTable {
                name: "Date".into(),
                columns: vec![ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) }],
                rows: vec![vec![Scalar::DateTime(45292.0)], vec![Scalar::DateTime(45293.0)]],
            },
        ],
        relationships: vec![
            Relationship {
                from_table: "Sales".into(),
                from_column: "OrderDate".into(),
                to_table: "Date".into(),
                to_column: "Date".into(),
                bidirectional: false,
                inactive: false,
            },
            Relationship {
                from_table: "Sales".into(),
                from_column: "ShipDate".into(),
                to_table: "Date".into(),
                to_column: "Date".into(),
                bidirectional: false,
                inactive: true,
            },
        ],
        measures: vec![Measure { name: "Total".into(), formula: "SUM(Sales[Amount])".into(), table: Some("Sales".into()), format: None, expr: None }],
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let evaluate = |src: &str| {
        let expr = parse_expression(src).expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        evaluator.evaluate(&bound).expect("evaluate")
    };
    // The active OrderDate relationship sees the Jan 2 order row.
    assert_eq!(evaluate("CALCULATE([Total], Date[Date] = DATE(2024,1,2))"), Scalar::Decimal(20.0));
    // USERELATIONSHIP picks ShipDate even though OrderDate is the first relationship in the model.
    assert_eq!(evaluate("CALCULATE([Total], USERELATIONSHIP(Sales[ShipDate], Date[Date]), Date[Date] = DATE(2024,1,2))"), Scalar::Decimal(10.0));
    assert_eq!(evaluate("CALCULATE([Total], USERELATIONSHIP(Sales[ShipDate], Date[Date]), Date[Date] = DATE(2024,1,1))"), Scalar::Decimal(20.0));
    // CROSSFILTER accepts both documented spellings and leaves the active relationship in place.
    assert_eq!(evaluate("CALCULATE([Total], CROSSFILTER(Sales[OrderDate], Date[Date], OneWay))"), Scalar::Decimal(30.0));
    assert_eq!(evaluate("CALCULATE([Total], CROSSFILTER(Sales[OrderDate], Date[Date], BOTH))"), Scalar::Decimal(30.0));
}

#[test]
fn calculate_filters_replace_the_target_column_and_map_by_name() {
    // The outer filter on Region must not suppress the candidates for the CALCULATE filter.
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let expr = parse_expression("CALCULATE([Total Sales], Sales[Region] = \"East\")").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    let mut filters = FilterContext::default();
    filters.set(0, 0, [gridcraft_dax::ScalarKey::String("West".into())]);
    assert_eq!(evaluator.evaluate_with(&bound, filters).expect("evaluate"), Scalar::Decimal(40.0));

    // VALUES(Sales[Amount]) exposes one column at position 0; it must filter Amount, not Region.
    assert_eq!(eval("CALCULATE([Total Sales], VALUES(Sales[Amount]))"), Scalar::Decimal(60.0));
}

#[test]
fn relationship_modifiers_apply_inside_calculate() {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Key".into(), data_type: Some("Int64".into()) },
                ],
                rows: vec![vec![Scalar::Integer(10), Scalar::Integer(1)], vec![Scalar::Integer(20), Scalar::Integer(2)]],
            },
            ModelTable {
                name: "Dim".into(),
                columns: vec![
                    ModelColumn { name: "Key".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                ],
                rows: vec![vec![Scalar::Integer(1), Scalar::String("A".into())], vec![Scalar::Integer(2), Scalar::String("B".into())]],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "Key".into(),
            to_table: "Dim".into(),
            to_column: "Key".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let evaluate = |src: &str| {
        let expr = parse_expression(src).expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        evaluator.evaluate(&bound).expect("evaluate")
    };
    // A Sales filter reaches Dim only when CROSSFILTER makes the relationship bidirectional.
    assert_eq!(evaluate("CALCULATE(COUNTROWS(Dim), Sales[Key] = 1)"), Scalar::Integer(2));
    assert_eq!(evaluate("CALCULATE(COUNTROWS(Dim), Sales[Key] = 1, CROSSFILTER(Sales[Key], Dim[Key], BOTH))"), Scalar::Integer(1));
    // The modifier itself is not a scalar.
    let expr = parse_expression("USERELATIONSHIP(Sales[Key], Dim[Key])").expect("parse");
    let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
    assert!(evaluator.evaluate(&bound).is_err(), "a modifier cannot be a standalone scalar");
}

#[test]
fn unbounded_scalar_functions_return_errors() {
    for formula in ["FACT(171)", "REPT(\"x\", 100000)"] {
        let model = model();
        let evaluator = Evaluator::new(&model).expect("evaluator");
        let expr = parse_expression(formula).expect("parse");
        let bound = Binder::new(&model).bind_expression(&expr).expect("bind");
        assert!(evaluator.evaluate(&bound).is_err(), "{formula} must fail");
    }
}

#[test]
fn scalar_json_round_trip_preserves_variants() {
    for value in [Scalar::Currency(123_456), Scalar::DateTime(45293.5), Scalar::Integer(7), Scalar::Decimal(1.5), Scalar::Blank] {
        let json = serde_json::to_value(&value).expect("serialize");
        let back: Scalar = serde_json::from_value(json.clone()).expect("deserialize");
        assert_eq!(back, value, "round trip through {json}");
    }
}

#[test]
fn scalar_keys_sort_numerically() {
    let mut keys: Vec<gridcraft_dax::ScalarKey> =
        [-2.5, -1.0, 0.0, 1.0, 2.5].into_iter().map(|value| gridcraft_dax::ScalarKey::from(&Scalar::Decimal(value))).collect();
    keys.sort();
    let back: Vec<f64> = keys.iter().map(|key| key.to_scalar().to_number().unwrap_or_default()).collect();
    assert_eq!(back, vec![-2.5, -1.0, 0.0, 1.0, 2.5]);
    assert_eq!(gridcraft_dax::ScalarKey::from(&Scalar::Decimal(-0.0)), gridcraft_dax::ScalarKey::from(&Scalar::Decimal(0.0)));
}

#[test]
fn pivot_cross_puts_column_groups_in_columns() {
    let model = model();
    let rows = gridcraft_dax::pivot_fields(&model, &["Sales[Region]".into()]).expect("rows");
    let columns = gridcraft_dax::pivot_fields(&model, &["Sales[Flag]".into()]).expect("columns");
    let table = gridcraft_dax::pivot_cross(&model, "Total Sales", &rows, &columns).expect("pivot");
    assert_eq!(table.columns.len(), 3);
    assert_eq!(table.rows.len(), 2);
    assert_eq!(table.rows[0][0], Scalar::String("East".into()));
    // FALSE sorts before TRUE; East has TRUE rows only (10 + 30).
    assert_eq!(table.rows[0][1], Scalar::Blank);
    assert_eq!(table.rows[0][2], Scalar::Decimal(40.0));
}
