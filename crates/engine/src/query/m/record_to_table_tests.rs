//! Record.ToTable: ordered fields, held values, and the bounded evaluator contract.

use std::cell::Cell;
use std::rc::Rc;

use gridcraft_model::Workbook;
use gridcraft_model::query::ColType;

use super::eval::{Ev, FUEL, LIMIT};
use super::library;
use super::value::{Lazy, MRecord, MResult, MTable, MValue, equal};
use crate::query::access::FileEntry;
use crate::query::{DataAccess, EvalCx, FetchCache, MAX_ROWS};

struct NoAccess;

impl DataAccess for NoAccess {
    fn read_file(&self, _: &str, _: usize) -> Result<Vec<u8>, String> {
        Err("Record.ToTable tests cannot read external files.".into())
    }

    fn list_folder(&self, _: &str, _: usize) -> Result<Vec<FileEntry>, String> {
        Err("Record.ToTable tests cannot list external folders.".into())
    }
}

fn call(arguments: Vec<MValue>) -> MResult<MValue> {
    let wb = Workbook::new();
    let mut cache = FetchCache::default();
    let mut cx = EvalCx::new(&wb, &NoAccess, Err("No external sources in Record.ToTable tests.".into()), &mut cache);
    library::call(&mut Ev::new(&mut cx), "Record.ToTable", &[], arguments)
}

fn evaluate(formula: &str) -> MResult<MValue> {
    let wb = Workbook::new();
    let mut cache = FetchCache::default();
    let mut cx = EvalCx::new(&wb, &NoAccess, Err("No external sources in Record.ToTable tests.".into()), &mut cache);
    Ev::new(&mut cx).eval(&super::parser::parse(formula)?, &None)
}

fn table(value: MValue) -> Rc<MTable> {
    let MValue::Table(table) = value else { panic!("Expected a table, got {value:?}") };
    table
}

fn record(fields: Vec<(&str, MValue)>) -> MValue {
    MValue::record(MRecord::new(fields.into_iter().map(|(name, value)| (name.into(), value)).collect()))
}

#[test]
fn documented_example_preserves_field_order_and_values_through_the_evaluator() {
    // Public reference: https://learn.microsoft.com/en-us/powerquery-m/record-totable
    let got = table(evaluate(r#"Record.ToTable([OrderID=1, CustomerID=1, Item="Fishing rod", Price=100.0])"#).unwrap());
    assert_eq!(got.names.as_ref(), &[Rc::from("Name"), Rc::from("Value")]);
    assert_eq!(got.rows.len(), 4);
    for (row, name, value) in [
        (0, "OrderID", MValue::Number(1.0)),
        (1, "CustomerID", MValue::Number(1.0)),
        (2, "Item", MValue::text("Fishing rod")),
        (3, "Price", MValue::Number(100.0)),
    ] {
        assert!(equal(&got.rows[row][0], &MValue::text(name)));
        assert!(equal(&got.rows[row][1], &value));
    }
    assert!(super::unsupported("Record.ToTable([A=1])", &|_| false).unwrap().is_empty());
}

#[test]
fn empty_record_keeps_both_columns_and_the_existing_any_schema() {
    let got = table(evaluate("Record.ToTable([])").unwrap());
    assert_eq!(got.names.as_ref(), &[Rc::from("Name"), Rc::from("Value")]);
    assert!(got.rows.is_empty());
    assert_eq!(got.types, [ColType::Any, ColType::Any]);
    let MValue::Type(ty) = evaluate("Value.Type(Record.ToTable([]))").unwrap() else { panic!("Expected table type") };
    assert_eq!(ty.kind, "table");
}

#[test]
fn special_names_null_and_nested_values_are_preserved_without_text_projection() {
    let values = [
        MValue::Null,
        record(vec![("Nested", MValue::Number(3.0))]),
        MValue::list(vec![MValue::Number(4.0)]),
        MValue::table(MTable::new(vec!["Inner".into()])),
        MValue::builtin("Record.Field"),
    ];
    let names = ["", "Name with space", "ação", "a", "A"];
    let input = record(names.into_iter().zip(values.iter().cloned()).collect());
    let got = table(call(vec![input]).unwrap());
    assert_eq!(got.rows.len(), names.len());
    for (index, (name, value)) in names.iter().zip(values.iter()).enumerate() {
        assert!(equal(&got.rows[index][0], &MValue::text(name)));
        assert!(equal(&got.rows[index][1], value));
    }
    assert_eq!(got.types, [ColType::Any, ColType::Any]);
}

#[test]
fn non_records_null_and_wrong_argument_counts_fail() {
    for value in [
        MValue::Null,
        MValue::Number(7.0),
        MValue::text("not a record"),
        MValue::Logical(true),
        MValue::list(vec![]),
        MValue::table(MTable::new(vec![])),
    ] {
        let error = call(vec![value]).unwrap_err();
        assert_eq!(error.reason, "Expression.Error");
        assert!(error.message.contains("Record"), "{error}");
    }
    for arguments in [vec![], vec![record(vec![]), record(vec![])]] {
        let error = call(arguments).unwrap_err();
        assert_eq!(error.reason, "Expression.Error");
        assert!(error.message.contains("expects 1"), "{error}");
    }
}

#[test]
fn held_error_is_local_to_its_value_cell_and_try_does_not_erase_it() {
    for (formula, expected) in [
        (r#"Table.RowCount(Record.ToTable([Good=1, Bad=error "held"]))"#, 2.0),
        (r#"Record.ToTable([Good=1, Bad=error "held"]){0}[Value]"#, 1.0),
        (r#"try Record.ToTable([Good=1, Bad=error "held"]){1}[Value] otherwise 7"#, 7.0),
    ] {
        assert!(equal(&evaluate(formula).unwrap(), &MValue::Number(expected)), "{formula}");
    }
    let error = evaluate(r#"let T=try Record.ToTable([Bad=error Error.Record("Custom.Error","held")]) otherwise null in T{0}[Value]"#).unwrap_err();
    assert_eq!(error.reason, "Custom.Error");
    assert_eq!(error.message, "held");
}

#[test]
fn lazy_field_is_not_forced_by_conversion_or_names_and_is_evaluated_once_on_access() {
    let forced = Rc::new(Cell::new(0));
    let observed = forced.clone();
    let lazy = Lazy::value(Box::new(move |_| {
        observed.set(observed.get() + 1);
        Ok(MValue::Number(42.0))
    }));
    let wb = Workbook::new();
    let mut cache = FetchCache::default();
    let mut cx = EvalCx::new(&wb, &NoAccess, Err("No external sources in this test.".into()), &mut cache);
    let mut ev = Ev::new(&mut cx);
    let value = library::call(&mut ev, "Record.ToTable", &[], vec![record(vec![("Deferred", lazy)])]).unwrap();
    let got = table(value.clone());
    assert_eq!(forced.get(), 0);
    assert!(equal(&ev.field(&value, "Name", false).unwrap(), &MValue::list(vec![MValue::text("Deferred")])));
    assert!(equal(&library::call(&mut ev, "Table.RowCount", &[], vec![value]).unwrap(), &MValue::Number(1.0)));
    assert_eq!(forced.get(), 0);
    for _ in 0..2 {
        assert!(equal(&ev.force(got.rows[0][1].clone()).unwrap(), &MValue::Number(42.0)));
    }
    assert_eq!(forced.get(), 1);
}

#[test]
fn field_copy_pays_its_evaluator_budget_before_producing_rows() {
    let input = record(vec![("A", MValue::Null), ("B", MValue::Null), ("C", MValue::Null)]);
    for remaining in [2, 3] {
        let wb = Workbook::new();
        let mut cache = FetchCache::default();
        let mut cx = EvalCx::new(&wb, &NoAccess, Err("No external sources in this test.".into()), &mut cache);
        let mut ev = Ev::new(&mut cx);
        ev.tick(FUEL - remaining).unwrap();
        let result = library::call(&mut ev, "Record.ToTable", &[], vec![input.clone()]);
        if remaining < 3 {
            assert_eq!(result.unwrap_err().reason, LIMIT);
        } else {
            assert_eq!(table(result.unwrap()).rows.len(), 3);
            assert_eq!(ev.tick(1).unwrap_err().reason, LIMIT);
        }
    }
}

#[test]
fn row_limit_is_checked_before_field_shape_or_output_allocation() {
    // Repeated Rc names consume a bounded ~16 MiB on 64-bit targets; no output rows
    // or million-element values vector are built. The row cap must win first.
    let input = MValue::record(MRecord { names: vec![Rc::from("Field"); MAX_ROWS + 1].into(), values: vec![] });
    let error = call(vec![input]).unwrap_err();
    assert_eq!(error.reason, "Expression.Error");
    assert_eq!(error.message, format!("A table can hold at most {MAX_ROWS} rows here."));
}

#[test]
fn malformed_internal_records_fail_instead_of_losing_fields_or_panicking() {
    for (names, values) in [(vec![Rc::from("Missing")], vec![]), (vec![], vec![MValue::Null])] {
        let input = MValue::record(MRecord { names: names.into(), values });
        let error = call(vec![input]).unwrap_err();
        assert_eq!(error.reason, "Expression.Error");
        assert!(error.message.contains("inconsistent field names and values"), "{error}");
    }
}
