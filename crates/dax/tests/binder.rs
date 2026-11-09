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
fn measure_home_table_must_exist_in_the_model() {
    let json = r#"{
        "tables": [{"name":"Sales","columns":[{"name":"Amount","data_type":"Double"}]}],
        "measures": [{"name":"Total","formula":"SUM(Sales[Amount])","table":"Salez"}]
    }"#;
    let mut model: DataModel = serde_json::from_str(json).expect("json loads");
    let error = model.parse_formulas().expect_err("a measure on a missing table is rejected");
    assert!(error.message.contains("Salez"), "the error names the unknown table: {}", error.message);

    let valid = r#"{
        "tables": [{"name":"Sales","columns":[{"name":"Amount","data_type":"Double"}]}],
        "measures": [{"name":"Total","formula":"SUM(Sales[Amount])","table":"sales"}]
    }"#;
    let mut model: DataModel = serde_json::from_str(valid).expect("json loads");
    model.parse_formulas().expect("a home table matching ignoring case still validates");
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
    // A variable in scope must not turn a documented interval into a name. `DATEDIFF` catalogues
    // its interval as `Any` while `DATEADD`, `DATESINPERIOD` and `PARALLELPERIOD` catalogue theirs
    // as `Text`; the shadowing rule has to hold for both spellings, or a variable called `YEAR`
    // silently replaces the interval and the call no longer shifts dates.
    let calls: [(&str, usize, &[&str]); 4] = [
        ("DATEDIFF ( Sales[Amount], Sales[Amount], {interval} )", 2, &["DAY", "MONTH", "QUARTER", "YEAR"]),
        ("DATEADD ( Date[Date], 1, {interval} )", 2, &["DAY", "MONTH", "QUARTER", "YEAR"]),
        ("DATESINPERIOD ( Date[Date], Date[Date], -12, {interval} )", 3, &["DAY", "WEEK", "MONTH", "QUARTER", "YEAR"]),
        ("PARALLELPERIOD ( Date[Date], 1, {interval} )", 2, &["MONTH", "QUARTER", "YEAR"]),
    ];
    for (template, position, intervals) in calls {
        for interval in intervals {
            let call = template.replace("{interval}", interval);
            let body = match bind(&model, &format!("VAR {interval} = 1 RETURN {call}")) {
                BoundExpr::VarReturn { body, .. } => *body,
                other => panic!("{call}: expected a VAR block, got {other:?}"),
            };
            let args = match body {
                BoundExpr::Call { args, .. } => args,
                other => panic!("{call}: expected a call, got {other:?}"),
            };
            assert!(
                matches!(&args[position], BoundExpr::Keyword(word) if word.eq_ignore_ascii_case(interval)),
                "{call}: the interval must stay `{interval}`, got {:?}",
                args[position]
            );
        }
    }
    // The other direction: a word that is not a documented enumeration stays a name, so the
    // variable binds — `PARALLELPERIOD` has no `DAY` interval, and neither does a genuine text
    // parameter (RANKX's Ties).
    let bound = bind(&model, "VAR DAY = 1 RETURN PARALLELPERIOD ( Date[Date], 1, DAY )");
    assert!(format!("{bound:?}").contains("Variable"), "{bound:?}");
    let bound = bind(&model, "VAR Tertial = 1 RETURN DATEADD ( Date[Date], 1, Tertial )");
    assert!(format!("{bound:?}").contains("Variable"), "{bound:?}");
    let bound = bind(&model, "VAR DENSE = 1 RETURN RANKX ( Sales, Sales[Amount], 42, DENSE )");
    assert!(format!("{bound:?}").contains("Variable"), "{bound:?}");
}
#[test]
fn an_interval_keyword_is_not_shadowed_by_a_table_or_measure_name() {
    // A table or measure named like a documented interval must not capture the interval word.
    // `DATEADD`, `DATESINPERIOD` and `PARALLELPERIOD` catalogue their intervals as `Text`, so
    // the word reaches the name filter instead of the fixed-slot branch.
    let mut table_model = sample_model();
    table_model.tables.push(ModelTable {
        name: "Year".into(),
        columns: vec![ModelColumn { name: "Id".into(), data_type: Some("Int64".into()) }],
        rows: Vec::new(),
    });
    for formula in
        ["DATEADD ( Date[Date], 1, YEAR )", "DATESINPERIOD ( Date[Date], Date[Date], -12, YEAR )", "PARALLELPERIOD ( Date[Date], 1, YEAR )"]
    {
        match bind(&table_model, formula) {
            BoundExpr::Call { args, .. } => {
                let position = args.len() - 1;
                assert!(
                    matches!(&args[position], BoundExpr::Keyword(word) if word.eq_ignore_ascii_case("YEAR")),
                    "{formula}: the interval must stay `YEAR`, got {:?}",
                    args[position]
                );
            }
            other => panic!("{formula}: expected a call, got {other:?}"),
        }
    }
    let mut measure_model = sample_model();
    measure_model.measures.push(Measure { name: "Year".into(), formula: "SUM ( Sales[Amount] )".into(), table: None, format: None, expr: None });
    measure_model.parse_formulas().expect("model parses");
    match bind(&measure_model, "DATEADD ( Date[Date], 1, YEAR )") {
        BoundExpr::Call { args, .. } => {
            assert!(matches!(&args[2], BoundExpr::Keyword(word) if word.eq_ignore_ascii_case("YEAR")), "{:?}", args[2]);
        }
        other => panic!("expected a call, got {other:?}"),
    }
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

#[test]
fn a_datatable_type_word_is_never_a_table_name() {
    let mut model = sample_model();
    // A table called `INTEGER` must not capture DATATABLE's type word.
    model.tables.push(gridcraft_dax::model::ModelTable {
        name: "INTEGER".into(),
        columns: vec![gridcraft_dax::model::ModelColumn { name: "Id".into(), data_type: Some("Int64".into()) }],
        rows: Vec::new(),
    });
    match bind(&model, "DATATABLE ( \"Id\", INTEGER, { { 1 } } )") {
        BoundExpr::Call { args, .. } => assert!(matches!(&args[1], BoundExpr::Keyword(word) if word == "INTEGER"), "{:?}", args[1]),
        other => panic!("expected a call, got {other:?}"),
    }
}

#[test]
fn value_text_positions_do_not_bind_bare_words_as_keywords() {
    let model = sample_model();
    // `Text` parameters that take a value — not a documented enumeration — resolve a bare word
    // as a name. An undefined word is an unknown name, not silent text.
    for formula in [
        "LEFT ( Nme, 1 )",
        "RIGHT ( Nme, 1 )",
        "LEN ( Nme )",
        "CONTAINSSTRING ( Sales[Region], Nme )",
        "FORMAT ( Sales[Amount], Nme )",
        "NAMEOF ( Sales[Amount], Nme )",
        "EVALUATEANDLOG ( Sales[Amount], Nme )",
        "INFO.STORAGETABLES ( Nme, 1 )",
        "DATEADD ( Date[Date], 1, Fortnight )",
        "DATESINPERIOD ( Date[Date], Date[Date], -12, Fortnight )",
        "PARALLELPERIOD ( Date[Date], 1, Fortnight )",
    ] {
        let expr = parse_expression(formula).expect("parses");
        let error = Binder::new(&model).bind_expression(&expr).expect_err("an undefined word is not a keyword");
        assert!(error.message.contains("unknown name `Nme`") || error.message.contains("unknown name `Fortnight`"), "{formula}: {error:?}");
    }
    // A genuine name still binds in those positions: tables, measures and variables.
    let mut named = sample_model();
    named.tables.push(ModelTable {
        name: "Nme".into(),
        columns: vec![ModelColumn { name: "Id".into(), data_type: Some("Int64".into()) }],
        rows: vec![],
    });
    assert!(matches!(bind(&named, "LEFT ( Nme, 1 )"), BoundExpr::Call { .. }));
    let bound = bind(&model, "VAR Nme = \"ab\" RETURN LEFT ( Nme, 1 )");
    assert!(matches!(bound, BoundExpr::VarReturn { .. }), "{bound:?}");
}

#[test]
fn rank_ties_and_blanks_bind_their_documented_words() {
    let model = sample_model();
    // RANK's Ties and Blanks and ROWNUMBER's Blanks are documented bare words.
    for (formula, position, word) in [
        ("RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ) )", 0, "DENSE"),
        ("RANK ( SKIP, Sales, ORDERBY ( Sales[Amount] ) )", 0, "SKIP"),
        ("RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ), DEFAULT )", 3, "DEFAULT"),
        ("RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ), FIRST )", 3, "FIRST"),
        ("RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ), LAST )", 3, "LAST"),
        ("ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ), DEFAULT )", 2, "DEFAULT"),
        ("ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ), FIRST )", 2, "FIRST"),
        ("ROWNUMBER ( Sales, ORDERBY ( Sales[Amount] ), LAST )", 2, "LAST"),
    ] {
        match bind(&model, formula) {
            BoundExpr::Call { args, .. } => {
                assert!(matches!(&args[position], BoundExpr::Keyword(found) if found == word), "{formula}: {:?}", args[position]);
            }
            other => panic!("{formula}: expected a call, got {other:?}"),
        }
    }
    // A table named like one of those words must not capture the slot.
    let mut shadowed = sample_model();
    shadowed.tables.push(ModelTable {
        name: "DENSE".into(),
        columns: vec![ModelColumn { name: "Id".into(), data_type: Some("Int64".into()) }],
        rows: vec![],
    });
    match bind(&shadowed, "RANK ( DENSE, Sales, ORDERBY ( Sales[Amount] ) )") {
        BoundExpr::Call { args, .. } => assert!(matches!(&args[0], BoundExpr::Keyword(word) if word == "DENSE"), "{:?}", args[0]),
        other => panic!("expected a call, got {other:?}"),
    }
    // An undocumented word there is still a name, and therefore unknown here.
    let expr = parse_expression("RANK ( DENSEX, Sales, ORDERBY ( Sales[Amount] ) )").expect("parses");
    assert!(Binder::new(&model).bind_expression(&expr).is_err());
}
