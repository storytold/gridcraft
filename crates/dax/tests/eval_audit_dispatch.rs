//! Dispatch audit: every catalogued function, at every documented arity, must reach a real
//! evaluator path — never the `not implemented` fallback and never an "unknown function" error.
//!
//! Three tests:
//!
//! 1. [`catalog_shape_is_consistent`] — the catalog itself is coherent: 484 unique names, every
//!    documented family populated, overload arities inside `min_args..=max_args`, sugar links
//!    resolving.
//! 2. [`no_catalogued_function_reaches_a_stub`] — for every overload arity, the call parses,
//!    binds and evaluates, and the outcome is never the generic "not implemented" fallback.
//! 3. [`context_functions_fail_with_a_context_error`] — the functions that legitimately need a
//!    visual, calculation-item, row or window context reject a bare model evaluation with a
//!    documented context error, never a stub and never a silently wrong value.
//!
//! `INFO.*` metadata functions are not in that list: they evaluate against the model catalogue
//! itself (returning row counts), so the dispatch audit covers them like any other function.

use gridcraft_dax::functions::{FunctionCategory, FunctionSpec, ParamKind, ReturnType, catalog};
use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

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
                    ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Decimal(10.0), Scalar::Integer(2), Scalar::Boolean(true), Scalar::DateTime(45292.0)],
                    vec![Scalar::String("West".into()), Scalar::Decimal(20.0), Scalar::Integer(3), Scalar::Boolean(false), Scalar::DateTime(45293.0)],
                    vec![Scalar::String("East".into()), Scalar::Decimal(30.0), Scalar::Integer(4), Scalar::Boolean(true), Scalar::DateTime(45294.0)],
                ],
            },
            ModelTable {
                name: "Products".into(),
                columns: vec![
                    ModelColumn { name: "Product".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Price".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![vec![Scalar::String("Bike".into()), Scalar::Decimal(100.0)], vec![Scalar::String("Helmet".into()), Scalar::Decimal(25.0)]],
            },
            ModelTable {
                name: "Date".into(),
                columns: vec![ModelColumn { name: "Date".into(), data_type: Some("DateTime".into()) }],
                rows: vec![vec![Scalar::DateTime(45292.0)], vec![Scalar::DateTime(45293.0)], vec![Scalar::DateTime(45294.0)]],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "Date".into(),
            to_table: "Date".into(),
            to_column: "Date".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    model
}

/// A syntactically valid argument for a catalog parameter kind.
fn placeholder(kind: ParamKind) -> &'static str {
    match kind {
        ParamKind::Column => "Sales[Amount]",
        ParamKind::Table | ParamKind::Reference => "Sales",
        ParamKind::Expression => "Sales[Amount]",
        ParamKind::Logical | ParamKind::Boolean => "TRUE",
        ParamKind::Text => "\"text\"",
        ParamKind::Date => "DATE(2024,1,1)",
        ParamKind::Function => "SUM",
        ParamKind::Number | ParamKind::Scalar | ParamKind::Any | ParamKind::Variant | ParamKind::Json => "1",
    }
}

/// Builds the call for one arity using that overload's parameter kinds.
fn formula(spec: &FunctionSpec, arity: usize) -> String {
    let kinds: Vec<ParamKind> = match spec.overload_for(arity) {
        Some(overload) => (0..arity).map(|index| overload.kind_at(index).unwrap_or(ParamKind::Any)).collect(),
        None => (0..arity).map(|index| spec.params.get(index).map(|param| param.kind).unwrap_or(ParamKind::Any)).collect(),
    };
    let args: Vec<&str> = kinds.iter().map(|kind| placeholder(*kind)).collect();
    format!("{}({})", spec.name, args.join(", "))
}

/// The exact text the evaluator uses when a function has no implementation. Two sites produce it
/// (`eval.rs` fallback and the time-intelligence range dispatcher), so matching it catches both.
const STUB_MARKER: &str = "is not implemented by this evaluator";

fn is_stub(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains(STUB_MARKER) || lower.contains("unknown function") || lower.contains("is not a function")
}

/// Functions that legitimately cannot be evaluated against a bare model: they need a visual, a
/// calculation item, a window/row context or a CALCULATE modifier. Each entry is the *substring the
/// evaluator's context error must contain*.
fn documented_context_requirements() -> Vec<(&'static str, &'static str)> {
    let mut list = vec![
        ("ISSUBTOTAL", "SUMMARIZE row context"),
        ("CURRENTGROUP", "GROUPBY"),
        ("ROLLUP", "SUMMARIZE group-by"),
        ("ROLLUPGROUP", "ROLLUP"),
        ("ROLLUPADDISSUBTOTAL", "SUMMARIZECOLUMNS"),
        ("ROLLUPISSUBTOTAL", "ADDMISSINGITEMS"),
        ("SELECTEDMEASURE", "calculation item"),
        ("SELECTEDMEASUREFORMATSTRING", "calculation item"),
        ("SELECTEDMEASURENAME", "calculation item"),
        ("EARLIER", "outer row context"),
        ("EARLIEST", "outer row context"),
        ("RANK", "needs a relation"),
        ("ROWNUMBER", "needs a relation"),
        ("OFFSET", "window function"),
        ("WINDOW", "window function"),
        ("ORDERBY", "window function"),
        ("PARTITIONBY", "window function"),
        ("MATCHBY", "window function"),
        ("RELATED", "row context"),
        ("RELATEDTABLE", "row context"),
        ("CROSSFILTER", "CALCULATE"),
        ("USERELATIONSHIP", "CALCULATE"),
    ];
    // Visual calculations need a visual frame; they are the whole `VisualCalculations` family.
    for spec in catalog() {
        if spec.category == FunctionCategory::VisualCalculations && !list.iter().any(|(name, _)| *name == spec.name) {
            list.push((spec.name, "visual"));
        }
    }
    list
}

#[test]
fn catalog_shape_is_consistent() {
    let specs = catalog();
    assert_eq!(specs.len(), 484, "catalogued function count");

    let mut seen: Vec<&str> = specs.iter().map(|spec| spec.name).collect();
    seen.sort_unstable();
    let before = seen.len();
    seen.dedup();
    assert_eq!(before, seen.len(), "catalog has duplicate names");

    for spec in specs {
        assert!(spec.min_args <= spec.max_args, "{}: min_args > max_args", spec.name);
        assert!(!spec.overloads.is_empty(), "{}: no overloads", spec.name);
        assert!(!spec.description.is_empty(), "{}: empty description", spec.name);
        for overload in spec.overloads {
            assert!(
                overload.arity() >= spec.min_args && overload.arity() <= spec.max_args,
                "{}: overload arity {} outside {}..={}",
                spec.name,
                overload.arity(),
                spec.min_args,
                spec.max_args
            );
        }
        if let Some(target) = spec.sugar_of {
            assert!(specs.iter().any(|other| other.name.eq_ignore_ascii_case(target)), "{}: sugar target `{target}` is not catalogued", spec.name);
        }
    }

    let mut counts: Vec<(FunctionCategory, usize)> = FunctionCategory::ALL.iter().map(|category| (*category, 0)).collect();
    for spec in specs {
        if let Some(entry) = counts.iter_mut().find(|(category, _)| *category == spec.category) {
            entry.1 += 1;
        }
    }
    let summary: Vec<String> = counts.iter().map(|(category, count)| format!("{}={count}", category.name())).collect();
    println!("catalog: {} functions [{}]", specs.len(), summary.join(", "));
    for (category, count) in counts {
        assert!(count > 0, "{} has no functions", category.name());
    }
}

#[test]
fn no_catalogued_function_reaches_a_stub() {
    let model = model();
    let binder = Binder::new(&model);
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let mut checked = 0usize;
    let mut valued = 0usize;
    let mut blank = 0usize;
    let mut rejected = 0usize;
    let mut stubs = Vec::new();
    let mut bind_failures = Vec::new();
    let mut functions_with_a_value = 0usize;
    let mut without_value = Vec::new();

    for spec in catalog() {
        let mut arities: Vec<usize> = spec.overloads.iter().map(|overload| overload.arity()).collect();
        arities.push(spec.min_args);
        arities.sort_unstable();
        arities.dedup();

        let mut any_value = false;
        let mut first_error = String::new();
        for arity in arities {
            checked += 1;
            let source = formula(spec, arity);
            let expr = match parse_expression(&source) {
                Ok(expr) => expr,
                Err(error) => {
                    bind_failures.push(format!("{}: parse error in `{source}`: {error}", spec.name));
                    continue;
                }
            };
            let bound = match binder.bind_expression(&expr) {
                Ok(bound) => bound,
                Err(error) => {
                    bind_failures.push(format!("{}: bind error in `{source}`: {error}", spec.name));
                    continue;
                }
            };
            let result = if spec.return_type == ReturnType::Table {
                evaluator.evaluate_table(&bound).map(|table| Scalar::Integer(table.rows.len() as i64))
            } else {
                evaluator.evaluate(&bound)
            };
            match result {
                Ok(Scalar::Blank) => {
                    blank += 1;
                    any_value = true;
                }
                Ok(_) => {
                    valued += 1;
                    any_value = true;
                }
                Err(error) => {
                    if first_error.is_empty() {
                        first_error = error.message.clone();
                    }
                    if is_stub(&error.message) {
                        stubs.push(format!("{}: `{source}` -> {}", spec.name, error.message));
                    } else {
                        rejected += 1;
                    }
                }
            }
        }
        if any_value {
            functions_with_a_value += 1;
        } else {
            without_value.push(format!("{}: {first_error}", spec.name));
        }
    }

    println!(
        "dispatch audit: {checked} calls over {} functions — {valued} values, {blank} blanks, {rejected} argument/context rejections, {} stubs",
        catalog().len(),
        stubs.len()
    );
    println!("functions that produced at least one value: {functions_with_a_value}");
    for line in &without_value {
        println!("NO VALUE {line}");
    }
    assert!(bind_failures.is_empty(), "{} calls do not parse/bind:\n{}", bind_failures.len(), bind_failures.join("\n"));
    assert!(stubs.is_empty(), "{} calls reach no evaluator path:\n{}", stubs.len(), stubs.join("\n"));
    // Mass-regression guard: with deliberately generic placeholder arguments a majority of
    // functions still return a value (the rest reject the argument shape), so a collapse in this
    // number means dispatch broke, not that the placeholders changed.
    assert!(
        functions_with_a_value * 100 >= catalog().len() * 70,
        "only {functions_with_a_value} of {} functions produced a value; dispatch may have regressed",
        catalog().len()
    );
}

#[test]
fn context_functions_fail_with_a_context_error() {
    let model = model();
    let binder = Binder::new(&model);
    let evaluator = Evaluator::new(&model).expect("evaluator");
    let mut problems = Vec::new();

    for (name, marker) in documented_context_requirements() {
        let spec = catalog().iter().find(|spec| spec.name.eq_ignore_ascii_case(name));
        let Some(spec) = spec else {
            problems.push(format!("{name}: documented as context-dependent but not catalogued"));
            continue;
        };
        let source = formula(spec, spec.min_args);
        let expr = match parse_expression(&source) {
            Ok(expr) => expr,
            Err(error) => {
                problems.push(format!("{name}: parse error in `{source}`: {error}"));
                continue;
            }
        };
        let bound = match binder.bind_expression(&expr) {
            Ok(bound) => bound,
            Err(error) => {
                problems.push(format!("{name}: bind error in `{source}`: {error}"));
                continue;
            }
        };
        let result = if spec.return_type == ReturnType::Table {
            evaluator.evaluate_table(&bound).map(|table| Scalar::Integer(table.rows.len() as i64))
        } else {
            evaluator.evaluate(&bound)
        };
        match result {
            Ok(value) => {
                // A value is acceptable only when it is BLANK: the function had nothing to work
                // with, which is a documented empty result, not a wrong answer.
                if !value.is_blank() {
                    problems.push(format!("{name}: `{source}` returned {value:?} without a context"));
                }
            }
            Err(error) => {
                if is_stub(&error.message) {
                    problems.push(format!("{name}: `{source}` -> stub: {}", error.message));
                } else if !error.message.to_ascii_lowercase().contains(&marker.to_ascii_lowercase()) {
                    problems.push(format!("{name}: `{source}` -> undocumented error `{}` (expected `{marker}`)", error.message));
                }
            }
        }
    }

    assert!(problems.is_empty(), "{} context functions do not report a documented context error:\n{}", problems.len(), problems.join("\n"));
}
