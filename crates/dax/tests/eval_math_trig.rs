//! Evaluator tests for math and trigonometry functions.

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

fn assert_close(actual: Scalar, expected: f64) {
    match actual {
        Scalar::Decimal(value) => {
            let tolerance = 1e-9 * expected.abs().max(1.0);
            assert!((value - expected).abs() <= tolerance, "expected {expected}, got {value}");
        }
        other => panic!("expected Decimal({expected}), got {other:?}"),
    }
}

#[test]
fn inverse_trigonometry() {
    assert_close(eval("ACOS(0.5)"), std::f64::consts::FRAC_PI_3);
    assert_close(eval("ACOS(-1)"), std::f64::consts::PI);
    assert!(eval_result("ACOS(2)").is_err());
    assert!(eval_result("ACOS(-1.0001)").is_err());

    assert_close(eval("ASIN(0.5)"), std::f64::consts::FRAC_PI_6);
    assert_close(eval("ASIN(-1)"), -std::f64::consts::FRAC_PI_2);
    assert!(eval_result("ASIN(1.5)").is_err());

    assert_close(eval("ATAN(1)"), std::f64::consts::FRAC_PI_4);
    assert_close(eval("ATAN(0)"), 0.0);

    assert_close(eval("ACOT(1)"), std::f64::consts::FRAC_PI_4);
    assert_close(eval("ACOT(-1)"), 3.0 * std::f64::consts::FRAC_PI_4);
    assert_close(eval("ACOT(0)"), std::f64::consts::FRAC_PI_2);
}

#[test]
fn hyperbolic_and_inverse_hyperbolic() {
    assert_close(eval("ACOSH(2)"), 1.316_957_896_924_816_6);
    assert!(eval_result("ACOSH(0.5)").is_err());

    assert_close(eval("ASINH(1)"), 0.881_373_587_019_543);
    assert_close(eval("ATANH(0.5)"), 0.549_306_144_334_054_9);
    assert!(eval_result("ATANH(1)").is_err());
    assert!(eval_result("ATANH(-1)").is_err());

    assert_close(eval("ACOTH(2)"), 0.549_306_144_334_054_9);
    assert_close(eval("ACOTH(-2)"), -0.549_306_144_334_054_9);
    assert!(eval_result("ACOTH(1)").is_err());
    assert!(eval_result("ACOTH(-0.5)").is_err());

    assert_close(eval("COSH(1)"), 1.543_080_634_815_243_7);
    assert_close(eval("SINH(1)"), 1.175_201_193_643_801_4);
    assert_close(eval("TANH(1)"), 0.761_594_155_955_764_9);
    assert!(eval_result("SINH(1000)").is_err(), "overflow must be an error, not infinity");
}

#[test]
fn cotangent_family() {
    assert_close(eval("COT(PI() / 4)"), 1.0);
    assert_close(eval("COTH(1)"), 1.313_035_285_499_331_2);
    assert!(eval_result("COT(0)").is_err());
    assert!(eval_result("COTH(0)").is_err());
}

#[test]
fn even_and_odd_round_away_from_zero() {
    assert_eq!(eval("EVEN(3.2)"), Scalar::Integer(4));
    assert_eq!(eval("EVEN(-3.2)"), Scalar::Integer(-4));
    assert_eq!(eval("EVEN(2)"), Scalar::Integer(2));
    assert_eq!(eval("EVEN(-2)"), Scalar::Integer(-2));
    assert_eq!(eval("EVEN(0)"), Scalar::Integer(0));

    assert_eq!(eval("ODD(2.1)"), Scalar::Integer(3));
    assert_eq!(eval("ODD(-2.1)"), Scalar::Integer(-3));
    assert_eq!(eval("ODD(2)"), Scalar::Integer(3));
    assert_eq!(eval("ODD(0)"), Scalar::Integer(1));
    assert_eq!(eval("ODD(-2)"), Scalar::Integer(-3));
}

#[test]
fn mround_sign_and_multiples() {
    assert_close(eval("MROUND(7.3, 0.5)"), 7.5);
    assert_close(eval("MROUND(10, 3)"), 9.0);
    assert_close(eval("MROUND(-7.3, -0.5)"), -7.5);
    // dax.guide documents the binary floating-point midpoints 6.0 and 7.1.
    assert_close(eval("MROUND(6.05, 0.1)"), 6.0);
    assert_close(eval("MROUND(7.05, 0.1)"), 7.1);
    assert_close(eval("MROUND(10, 0)"), 0.0);
    assert!(eval_result("MROUND(7, -2)").is_err());
    assert!(eval_result("MROUND(-7, 2)").is_err());
}

#[test]
fn quotient_truncates_toward_zero() {
    assert_eq!(eval("QUOTIENT(7, 2)"), Scalar::Integer(3));
    assert_eq!(eval("QUOTIENT(-7, 2)"), Scalar::Integer(-3));
    assert_eq!(eval("QUOTIENT(7, -2)"), Scalar::Integer(-3));
    assert_eq!(eval("QUOTIENT(0, 2)"), Scalar::Integer(0));
    assert!(eval_result("QUOTIENT(7, 0)").is_err());
}

#[test]
fn sqrtpi_domain() {
    assert_close(eval("SQRTPI(2)"), 2.506_628_274_631_000_2);
    assert_close(eval("SQRTPI(0)"), 0.0);
    assert!(eval_result("SQRTPI(-1)").is_err());
}

#[test]
fn currency_rounds_to_four_decimals() {
    assert_eq!(eval("CURRENCY(10.5)"), Scalar::Currency(105_000));
    assert_eq!(eval("CURRENCY(3.66666)"), Scalar::Currency(36_667));
    assert_eq!(eval("CURRENCY(-3.66666)"), Scalar::Currency(-36_667));
    assert_eq!(eval("CURRENCY(3.0123456789)"), Scalar::Currency(30_123));
    assert_eq!(eval("CURRENCY(\"3.66666\")"), Scalar::Currency(36_667));
    assert_eq!(eval("CURRENCY(TRUE)"), Scalar::Currency(10_000));
    assert_eq!(eval("CURRENCY(FALSE)"), Scalar::Currency(0));
    assert!(eval_result("CURRENCY(\"not a number\")").is_err());
}

#[test]
fn random_functions_stay_in_range() {
    for _ in 0..64 {
        match eval("RAND()") {
            Scalar::Decimal(value) => assert!((0.0..1.0).contains(&value), "RAND out of range: {value}"),
            other => panic!("RAND returned {other:?}"),
        }
        match eval("RANDBETWEEN(1, 10)") {
            Scalar::Integer(value) => assert!((1..=10).contains(&value), "RANDBETWEEN out of range: {value}"),
            other => panic!("RANDBETWEEN returned {other:?}"),
        }
    }
    assert_eq!(eval("RANDBETWEEN(5, 5)"), Scalar::Integer(5));
    assert_eq!(eval("RANDBETWEEN(-3, -3)"), Scalar::Integer(-3));
    assert!(eval_result("RANDBETWEEN(10, 1)").is_err());
}

#[test]
fn convert_data_types() {
    assert_eq!(eval("CONVERT(32.34, INTEGER)"), Scalar::Integer(32));
    assert_eq!(eval("CONVERT(-32.34, INT64)"), Scalar::Integer(-32));
    assert_eq!(eval("CONVERT(\"123\", INTEGER)"), Scalar::Integer(123));
    assert_eq!(eval("CONVERT(1, DOUBLE)"), Scalar::Decimal(1.0));
    assert_eq!(eval("CONVERT(3.66666, CURRENCY)"), Scalar::Currency(36_667));
    assert_eq!(eval("CONVERT(3.66666, DECIMAL)"), Scalar::Currency(36_667));
    assert_eq!(eval("CONVERT(12345, DATETIME)"), Scalar::DateTime(12_345.0));
    assert_eq!(eval("CONVERT(\"1966-12-25\", DATETIME)"), Scalar::DateTime(24_466.0));
    assert_eq!(eval("CONVERT(TRUE, STRING)"), Scalar::String("TRUE".into()));
    assert_eq!(eval("CONVERT(\"TRUE\", BOOLEAN)"), Scalar::Boolean(true));
    assert_eq!(eval("CONVERT(0, LOGICAL)"), Scalar::Boolean(false));
    assert!(eval_result("CONVERT(\"abc\", INTEGER)").is_err());
    assert!(eval_result("CONVERT(1, NONSENSE)").is_err());
}
