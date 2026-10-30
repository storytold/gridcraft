//! Evaluator tests for `PATHITEMREVERSE`.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Account".into(),
            columns: vec![
                ModelColumn { name: "AccountKey".into(), data_type: Some("Int64".into()) },
                ModelColumn { name: "AccountPath".into(), data_type: Some("String".into()) },
            ],
            rows: vec![
                vec![Scalar::Integer(1), Scalar::String("1|2|3".into())],
                vec![Scalar::Integer(4), Scalar::String("4|5".into())],
                vec![Scalar::Integer(7), Scalar::String("7".into())],
            ],
        }],
        relationships: Vec::new(),
        measures: Vec::new(),
    };
    model.parse_formulas().expect("model parses");
    model
}

fn eval(src: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn value(src: &str) -> Scalar {
    match eval(src) {
        Ok(value) => value,
        Err(error) => panic!("{src} failed: {error}"),
    }
}

#[test]
fn counts_backwards_from_the_last_item() {
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", 1)"), Scalar::String("764".into()));
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", 2)"), Scalar::String("789".into()));
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", 3)"), Scalar::String("456".into()));
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", 4)"), Scalar::String("123".into()));
    // `PATHITEM 1` is the opposite end of the same path.
    assert_eq!(value("PATHITEM(\"123|456|789|764\", 1)"), Scalar::String("123".into()));
}

#[test]
fn a_single_item_path_has_one_position() {
    assert_eq!(value("PATHITEMREVERSE(\"42\", 1)"), Scalar::String("42".into()));
    assert_eq!(value("PATHITEMREVERSE(\"42\", 2)"), Scalar::Blank);
}

#[test]
fn out_of_range_positions_are_blank() {
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", 5)"), Scalar::Blank);
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", 100)"), Scalar::Blank);
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", 0)"), Scalar::Blank);
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", -2)"), Scalar::Blank);
    // BLANK coerces to 0, which is below the first reverse position.
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789|764\", BLANK())"), Scalar::Blank);
}

#[test]
fn the_optional_type_selects_text_or_integer() {
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789\", 1, 0)"), Scalar::String("789".into()));
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789\", 1, 1)"), Scalar::Integer(789));
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789\", 3, 1)"), Scalar::Integer(123));
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789\", 1, \"TEXT\")"), Scalar::String("789".into()));
    assert_eq!(value("PATHITEMREVERSE(\"123|456|789\", 2, \"INTEGER\")"), Scalar::Integer(456));
}

#[test]
fn an_invalid_type_or_non_numeric_item_is_an_error() {
    assert!(eval("PATHITEMREVERSE(\"123|456|789\", 1, 2)").is_err());
    assert!(eval("PATHITEMREVERSE(\"a|b\", 1, 1)").is_err());
    // The out-of-range item is BLANK before any integer conversion happens.
    assert_eq!(value("PATHITEMREVERSE(\"a|b\", 9, 1)"), Scalar::Blank);
}

#[test]
fn resolves_a_path_column_in_row_context() {
    // The right-most AccountKey of every path: 3, 5 and 7.
    assert_eq!(value("MAXX(Account, PATHITEMREVERSE(Account[AccountPath], 1, 1))"), Scalar::Decimal(7.0));
    assert_eq!(value("SUMX(Account, PATHITEMREVERSE(Account[AccountPath], 1, 1))"), Scalar::Decimal(15.0));
}
