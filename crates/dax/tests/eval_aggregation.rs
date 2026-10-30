//! Evaluator tests for `AVERAGEA`, `MAXA`, `MINA` and `DISTINCTCOUNTNOBLANK`.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                ModelColumn { name: "Flag".into(), data_type: Some("Boolean".into()) },
                ModelColumn { name: "Note".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Empty".into(), data_type: Some("String".into()) },
            ],
            rows: vec![
                vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::Boolean(true), Scalar::String("5".into()), Scalar::Blank],
                vec![Scalar::String("West".into()), Scalar::Integer(20), Scalar::Boolean(false), Scalar::String(String::new()), Scalar::Blank],
                vec![Scalar::String("East".into()), Scalar::Blank, Scalar::Boolean(true), Scalar::String("abc".into()), Scalar::Blank],
                vec![Scalar::Blank, Scalar::Integer(40), Scalar::Boolean(false), Scalar::String("7".into()), Scalar::Blank],
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
fn averagea_counts_text_and_booleans() {
    // BLANK is an empty cell, not a zero: 10 + 20 + 40 over three non-blank rows.
    assert_eq!(value("AVERAGEA(Sales[Amount])"), Scalar::Decimal(70.0 / 3.0));
    // TRUE = 1, FALSE = 0: (1 + 0 + 1 + 0) / 4.
    assert_eq!(value("AVERAGEA(Sales[Flag])"), Scalar::Decimal(0.5));
    // "5" = 5, "" = 0, "abc" = 0, "7" = 7.
    assert_eq!(value("AVERAGEA(Sales[Note])"), Scalar::Decimal(3.0));
    // A column whose cells are all BLANK has no values to average.
    assert_eq!(value("AVERAGEA(Sales[Empty])"), Scalar::Blank);
}

#[test]
fn averagea_respects_the_filter_context_and_is_blank_for_no_rows() {
    // East rows are 10 and BLANK: the BLANK row is excluded.
    assert_eq!(value("CALCULATE(AVERAGEA(Sales[Amount]), Sales[Region] = \"East\")"), Scalar::Decimal(10.0));
    assert_eq!(value("CALCULATE(AVERAGEA(Sales[Amount]), Sales[Region] = \"Nowhere\")"), Scalar::Blank);
}

#[test]
fn maxa_and_mina_count_logical_values() {
    assert_eq!(value("MINA(Sales[Flag])"), Scalar::Decimal(0.0));
    assert_eq!(value("MAXA(Sales[Flag])"), Scalar::Decimal(1.0));
    // Text is not rankable, so every text value coerces to 0 before the comparison.
    assert_eq!(value("MINA(Sales[Note])"), Scalar::Decimal(0.0));
    assert_eq!(value("MAXA(Sales[Note])"), Scalar::Decimal(7.0));
    assert_eq!(value("MAXA(Sales[Amount])"), Scalar::Decimal(40.0));
    assert_eq!(value("MINA(Sales[Amount])"), Scalar::Decimal(10.0));
}

#[test]
fn maxa_and_mina_follow_the_filter_context() {
    assert_eq!(value("CALCULATE(MAXA(Sales[Amount]), Sales[Region] = \"East\")"), Scalar::Decimal(10.0));
    assert_eq!(value("CALCULATE(MINA(Sales[Amount]), Sales[Region] = \"West\")"), Scalar::Decimal(20.0));
    assert_eq!(value("CALCULATE(MAXA(Sales[Flag]), Sales[Region] = \"West\")"), Scalar::Decimal(0.0));
    assert_eq!(value("CALCULATE(MAXA(Sales[Amount]), Sales[Region] = \"Nowhere\")"), Scalar::Blank);
}

#[test]
fn distinctcountnoblank_excludes_blank() {
    // East, West and one BLANK row -> two distinct non-blank values.
    assert_eq!(value("DISTINCTCOUNTNOBLANK(Sales[Region])"), Scalar::Integer(2));
    // The plain DISTINCTCOUNT counts BLANK as a value.
    assert_eq!(value("DISTINCTCOUNT(Sales[Region])"), Scalar::Integer(3));
    assert_eq!(value("DISTINCTCOUNTNOBLANK(Sales[Amount])"), Scalar::Integer(3));
    assert_eq!(value("DISTINCTCOUNTNOBLANK(Sales[Empty])"), Scalar::Integer(0));
}

#[test]
fn distinctcountnoblank_respects_the_filter_context() {
    // Both East rows carry the same region, so the flag filter leaves one distinct value.
    assert_eq!(value("CALCULATE(DISTINCTCOUNTNOBLANK(Sales[Region]), Sales[Flag] = TRUE)"), Scalar::Integer(1));
    assert_eq!(value("CALCULATE(DISTINCTCOUNTNOBLANK(Sales[Region]), Sales[Region] = \"Nowhere\")"), Scalar::Integer(0));
}

#[test]
fn the_a_variants_still_require_a_column() {
    assert!(eval("AVERAGEA(1)").is_err());
    assert!(eval("MAXA(\"text\")").is_err());
    assert!(eval("DISTINCTCOUNTNOBLANK()").is_err());
}
