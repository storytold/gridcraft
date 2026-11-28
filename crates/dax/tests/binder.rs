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

#[test]
fn documented_keyword_arguments_bind_instead_of_resolving_as_names() {
    let model = sample_model();
    // CROSSFILTER's direction words are documented enum values, including the two directional
    // spellings; none of them may be resolved as a model name.
    for direction in ["NONE", "ONE", "ONEWAY", "BOTH", "TWOWAY", "ONEWAY_LEFTFILTERSRIGHT", "ONEWAY_RIGHTFILTERSLEFT"] {
        let formula = format!("CROSSFILTER ( Sales[Region], Date[Date], {direction} )");
        match bind(&model, &formula) {
            BoundExpr::Call { args, .. } => {
                assert!(matches!(&args[2], BoundExpr::Keyword(word) if word.eq_ignore_ascii_case(direction)), "{direction}: {:?}", args[2]);
            }
            other => panic!("expected a call for {direction}, got {other:?}"),
        }
    }
    // RANK/ROWNUMBER's Reset argument takes the same three words at the documented position.
    for reset in ["NONE", "LOWESTPARENT", "HIGHESTPARENT"] {
        let rank =
            format!("RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ), DEFAULT, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Amount] ), {reset} )");
        assert!(matches!(bind(&model, &rank), BoundExpr::Call { .. }), "RANK should bind {reset}");
        let rownumber =
            format!("ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ), DEFAULT, PARTITIONBY ( Sales[Region] ), MATCHBY ( Sales[Amount] ), {reset} )");
        assert!(matches!(bind(&model, &rownumber), BoundExpr::Call { .. }), "ROWNUMBER should bind {reset}");
    }
    // RANKX's Order argument is `ASC`/`DESC`, not a name.
    match bind(&model, "RANKX ( Sales, Sales[Amount], 42, DESC )") {
        BoundExpr::Call { args, .. } => assert!(matches!(args[3], BoundExpr::Keyword(ref word) if word == "DESC"), "{:?}", args[3]),
        other => panic!("expected a call, got {other:?}"),
    }
}

#[test]
fn an_interval_keyword_is_not_shadowed_by_a_variable() {
    let model = sample_model();
    // A variable in scope must not turn the documented interval argument into a name: `YEAR` here
    // is DATEDIFF's interval, and the binder keeps it a keyword.
    let bound = bind(&model, "VAR YEAR = 1 RETURN DATEDIFF ( Sales[Amount], Sales[Amount], YEAR )");
    let debug = format!("{bound:?}");
    assert!(debug.contains("Keyword(\"YEAR\")"), "{debug}");
    // `DATEADD`'s interval is catalogued as text, but it is the same documented enumeration: a
    // variable holding another interval must not replace it.
    let bound = bind(&model, "VAR MONTH = \"DAY\" RETURN DATEADD ( Sales[Amount], 1, MONTH )");
    let debug = format!("{bound:?}");
    assert!(debug.contains("Keyword(\"MONTH\")"), "{debug}");
    // `DATESINPERIOD`'s fourth argument is the same documented enumeration, including `WEEK`,
    // which `DATEADD` does not accept.
    for interval in ["DAY", "WEEK", "MONTH", "QUARTER", "YEAR"] {
        let formula = format!("VAR {interval} = \"DAY\" RETURN DATESINPERIOD ( Date[Date], Date[Date], -1, {interval} )");
        let bound = bind(&model, &formula);
        let debug = format!("{bound:?}");
        assert!(debug.contains(&format!("Keyword(\"{interval}\")")), "{formula}: {debug}");
    }
    // A variable still wins where the parameter is documented as text: RANKX's Ties argument.
    let bound = bind(&model, "VAR DENSE = 1 RETURN RANKX ( Sales, Sales[Amount], 42, DENSE )");
    assert!(format!("{bound:?}").contains("Variable"), "{bound:?}");
}

#[test]
fn documented_order_keywords_bind_at_their_argument_positions() {
    let model = sample_model();
    // Each documented bare `ASC`/`DESC` slot binds as a keyword instead of failing as an unknown
    // name, and an undocumented word there is still a name (and therefore an unknown one).
    let calls = [
        ("RANKX ( Sales, Sales[Amount], 42, DESC )", 3),
        ("RANK.EQ ( 42, Sales[Amount], DESC )", 2),
        ("TOPNSKIP ( 10, 20, Sales, Sales[Amount], DESC )", 4),
        ("CONCATENATEX ( Sales, Sales[Amount], \", \", Sales[Amount], DESC )", 4),
        ("SAMPLE ( 10, Sales, Sales[Amount], DESC )", 3),
    ];
    for (src, position) in calls {
        match bind(&model, src) {
            BoundExpr::Call { args, .. } => {
                assert!(matches!(&args[position], BoundExpr::Keyword(word) if word == "DESC"), "{src}: {:?}", args[position]);
            }
            other => panic!("{src}: expected a call, got {other:?}"),
        }
    }
    // Every repeated `(orderBy, Order)` pair binds, not just the first one.
    for (src, positions) in [
        ("SAMPLE ( 10, Sales, Sales[Amount], DESC, Sales[Amount], ASC )", [3usize, 5usize]),
        ("TOPNSKIP ( 10, 20, Sales, Sales[Amount], DESC, Sales[Amount], ASC )", [4usize, 6usize]),
    ] {
        match bind(&model, src) {
            BoundExpr::Call { args, .. } => {
                assert!(matches!(&args[positions[0]], BoundExpr::Keyword(word) if word == "DESC"), "{src}: {:?}", args[positions[0]]);
                assert!(matches!(&args[positions[1]], BoundExpr::Keyword(word) if word == "ASC"), "{src}: {:?}", args[positions[1]]);
            }
            other => panic!("{src}: expected a call, got {other:?}"),
        }
    }
    // An unknown word in a keyword slot is still unknown.
    let expr = parse_expression("SAMPLE ( 10, Sales, Sales[Amount], SIDEWAYS )").expect("parse");
    assert!(Binder::new(&model).bind_expression(&expr).is_err());
}

#[test]
fn concatenate_x_binds_a_bare_order_word_in_every_pair() {
    let model = sample_model();
    // Every `(orderBy, Order)` pair takes a bare keyword: index 4 and index 6 here.
    let bound = bind(&model, "CONCATENATEX ( Sales, Sales[Region], \"|\", Sales[Region], DESC, Sales[Amount], ASC )");
    match bound {
        BoundExpr::Call { args, .. } => {
            assert!(matches!(&args[4], BoundExpr::Keyword(word) if word == "DESC"), "{:?}", args[4]);
            assert!(matches!(&args[6], BoundExpr::Keyword(word) if word == "ASC"), "{:?}", args[6]);
        }
        other => panic!("expected a call, got {other:?}"),
    }
    // A non-keyword in an order slot is still a name, and there is no such name here.
    let expr = parse_expression("CONCATENATEX ( Sales, Sales[Region], \"|\", Sales[Region], SIDEWAYS )").expect("parse");
    assert!(Binder::new(&model).bind_expression(&expr).is_err());
}
