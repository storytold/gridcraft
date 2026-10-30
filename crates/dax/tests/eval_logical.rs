//! Evaluator tests for bitwise integer operations and `IF.EAGER`.

use gridcraft_dax::model::DataModel;
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    DataModel { tables: vec![], relationships: vec![], measures: vec![] }
}

fn eval_result(source: &str) -> Result<Scalar, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(source).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn eval(source: &str) -> Scalar {
    eval_result(source).expect("expression evaluates")
}

#[test]
fn bitwise_and_or_xor() {
    assert_eq!(eval("BITAND(5, 3)"), Scalar::Integer(1));
    assert_eq!(eval("BITAND(13, 25)"), Scalar::Integer(9));
    assert_eq!(eval("BITAND(-1, 255)"), Scalar::Integer(255));
    assert_eq!(eval("BITAND(3.9, 1)"), Scalar::Integer(1));

    assert_eq!(eval("BITOR(5, 3)"), Scalar::Integer(7));
    assert_eq!(eval("BITOR(13, 25)"), Scalar::Integer(29));

    assert_eq!(eval("BITXOR(5, 3)"), Scalar::Integer(6));
    assert_eq!(eval("BITXOR(13, 25)"), Scalar::Integer(20));
    assert_eq!(eval("BITXOR(0, 0)"), Scalar::Integer(0));
}

#[test]
fn bitwise_rejects_out_of_range_values() {
    assert!(eval_result("BITAND(1e30, 1)").is_err());
    assert!(eval_result("BITOR(-1e30, 1)").is_err());
}

#[test]
fn if_eager_selects_a_branch() {
    assert_eq!(eval("IF.EAGER(TRUE, 1, 2)"), Scalar::Integer(1));
    assert_eq!(eval("IF.EAGER(FALSE, 1, 2)"), Scalar::Integer(2));
    assert_eq!(eval("IF.EAGER(1 = 1, \"yes\", \"no\")"), Scalar::String("yes".into()));
    assert_eq!(eval("IF.EAGER(FALSE, 1)"), Scalar::Blank);
}

#[test]
fn if_eager_evaluates_both_branches() {
    // The unselected branch is still evaluated, so its error propagates.
    assert!(eval_result("IF.EAGER(TRUE, 1, 1 / 0)").is_err());
    assert!(eval_result("IF.EAGER(FALSE, 1 / 0, 2)").is_err());
    assert!(eval_result("IF.EAGER(FALSE, 1 / 0)").is_err());
    // The condition itself is also evaluated eagerly.
    assert!(eval_result("IF.EAGER(1 / 0, 1, 2)").is_err());
    // A non-boolean condition is rejected rather than silently treated as FALSE.
    assert!(eval_result("IF.EAGER(\"maybe\", 1, 2)").is_err());
}
