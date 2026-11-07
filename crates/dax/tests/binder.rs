//! Model metadata and name-binding tests.

use gridcraft_dax::binder::{BoundExpr, BoundVariable};
use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::{Binder, parse_expression};

fn sample_model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![],
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
            Measure { name: "Total Sales".into(), formula: "SUM ( Sales[Amount] )".into(), table: Some("Sales".into()), format: None, expr: None },
            Measure {
                name: "East Sales".into(),
                formula: "CALCULATE ( [Total Sales], Sales[Region] = \"East\" )".into(),
                table: Some("Sales".into()),
                format: None,
                expr: None,
            },
            Measure { name: "Cycle A".into(), formula: "[Cycle B]".into(), table: None, format: None, expr: None },
            Measure { name: "Cycle B".into(), formula: "[Cycle A]".into(), table: None, format: None, expr: None },
        ],
    };
    model.parse_formulas().expect("model parses");
    model
}

fn bind(model: &DataModel, formula: &str) -> BoundExpr {
    let expr = parse_expression(formula).expect("formula parses");
    Binder::new(model).bind_expression(&expr).expect("formula binds")
}

#[test]
fn binds_qualified_columns_tables_and_measures() {
    let model = sample_model();
    match bind(&model, "SUM ( Sales[Amount] )") {
        BoundExpr::Call { spec, args } => {
            assert_eq!(spec.name, "SUM");
            match &args[0] {
                BoundExpr::Column { table, column } => {
                    assert_eq!(model.tables[*table].name, "Sales");
                    assert_eq!(model.tables[*table].columns[*column].name, "Amount");
                }
                other => panic!("expected column, got {other:?}"),
            }
        }
        other => panic!("expected call, got {other:?}"),
    }
    assert!(matches!(bind(&model, "Sales"), BoundExpr::Table(0)));
    assert!(matches!(bind(&model, "[Total Sales]"), BoundExpr::Measure(0)));
    // A measure reference may carry its home table as a qualifier.
    assert!(matches!(bind(&model, "Sales[Total Sales]"), BoundExpr::Measure(0)));
    assert!(matches!(bind(&model, "[Amount]"), BoundExpr::UnqualifiedColumn { .. }));
}

#[test]
fn reports_unknown_names_and_bad_arities() {
    let model = sample_model();
    let error = |formula: &str| {
        let expr = parse_expression(formula).expect("parses");
        Binder::new(&model).bind_expression(&expr).expect_err("should fail")
    };
    assert!(error("Missing[Amount]").message.contains("unknown table"));
    assert!(error("Sales[Missing]").message.contains("unknown column"));
    // A measure qualifier has to name the measure's home table.
    assert!(error("Date[Total Sales]").message.contains("belongs to table `Sales`"));
    assert!(error("SUM ( )").message.contains("expects 1"));
    assert!(error("NOPE ( 1 )").message.contains("unknown DAX function"));
}

#[test]
fn binds_var_return_with_sequential_scopes() {
    let model = sample_model();
    let bound = bind(&model, "VAR x = 1 VAR y = x + 1 RETURN y");
    match bound {
        BoundExpr::VarReturn { vars, body } => {
            assert_eq!(vars.len(), 2);
            assert_eq!(vars[0].name, "x");
            assert_eq!(vars[1].name, "y");
            assert!(matches!(vars[0], BoundVariable { .. }));
            assert!(matches!(*body, BoundExpr::Variable { index: 1, .. }));
        }
        other => panic!("expected VAR block, got {other:?}"),
    }

    // A variable shadows a table of the same name.
    assert!(matches!(
        bind(&model, "VAR Sales = 1 RETURN Sales"),
        BoundExpr::VarReturn { body, .. } if matches!(*body, BoundExpr::Variable { .. })
    ));
}

#[test]
fn bracketed_names_bind_measures_before_variables() {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) }],
            rows: vec![],
        }],
        relationships: vec![],
        measures: vec![Measure {
            name: "Total".into(),
            formula: "SUM ( Sales[Amount] )".into(),
            table: Some("Sales".into()),
            format: None,
            expr: None,
        }],
    };
    model.parse_formulas().expect("model parses");

    // `[Total]` is a bracketed model reference, so it binds to the measure even though a
    // variable of the same name is in scope.
    match bind(&model, "VAR Total = 1 RETURN [Total]") {
        BoundExpr::VarReturn { vars, body } => {
            assert_eq!(vars.len(), 1);
            assert_eq!(vars[0].name, "Total");
            assert!(matches!(*body, BoundExpr::Measure(0)), "expected measure, got {body:?}");
        }
        other => panic!("expected VAR block, got {other:?}"),
    }

    // A bare name still resolves to the variable.
    assert!(matches!(
        bind(&model, "VAR Total = 1 RETURN Total"),
        BoundExpr::VarReturn { body, .. } if matches!(*body, BoundExpr::Variable { index: 0, .. })
    ));
}

#[test]
fn pathitem_type_names_bind_as_keywords() {
    let model = sample_model();
    for (formula, function, expected) in
        [("PATHITEM ( \"1|2\", 1, TEXT )", "PATHITEM", "TEXT"), ("PATHITEMREVERSE ( \"1|2\", 1, INTEGER )", "PATHITEMREVERSE", "INTEGER")]
    {
        match bind(&model, formula) {
            BoundExpr::Call { spec, args } => {
                assert_eq!(spec.name, function);
                assert_eq!(args.len(), 3);
                assert!(matches!(&args[2], BoundExpr::Keyword(keyword) if keyword == expected), "expected `{expected}` keyword, got {:?}", args[2]);
            }
            other => panic!("expected call, got {other:?}"),
        }
    }
}

#[test]
fn measures_and_dependencies_are_bound() {
    let model = sample_model();
    let binder = Binder::new(&model);
    let total = binder.bind_measure("Total Sales").expect("measure binds");
    assert!(total.dependencies.is_empty());
    let east = binder.bind_measure("East Sales").expect("measure binds");
    assert_eq!(east.dependencies, ["Total Sales".to_string()].into_iter().collect());

    let graph = binder.dependency_graph().expect_err("cycle is rejected");
    assert!(graph.message.contains("circular"));
}

#[test]
fn model_validation_rejects_duplicates_and_missing_relationship_columns() {
    let mut model = sample_model();
    model.tables.push(ModelTable { name: "sales".into(), columns: vec![], rows: vec![] });
    assert!(model.parse_formulas().expect_err("duplicate table").message.contains("duplicate"));

    let mut model = sample_model();
    model.relationships.push(Relationship {
        from_table: "Sales".into(),
        from_column: "Missing".into(),
        to_table: "Date".into(),
        to_column: "Date".into(),
        bidirectional: false,
        inactive: false,
    });
    assert!(model.parse_formulas().expect_err("missing column").message.contains("missing column"));
}

#[test]
fn model_json_deserializes_and_parses_measures() {
    let json = r#"{
        "tables": [{"name":"Sales","columns":[{"name":"Amount","data_type":"Double"}]}],
        "relationships": [],
        "measures": [{"name":"Total","formula":"SUM(Sales[Amount])","table":"Sales"}]
    }"#;
    let mut model: DataModel = serde_json::from_str(json).expect("json loads");
    model.parse_formulas().expect("model parses");
    assert!(model.measures[0].expr.is_some());
}
