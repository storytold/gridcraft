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
fn averagea_counts_text_booleans_and_blanks() {
    // Every row counts, a BLANK cell as 0: 10 + 20 + 0 + 40 over four rows. This is the
    // documented DAX example (1, 20, "n/a", BLANK and TRUE average 22/5).
    assert_eq!(value("AVERAGEA(Sales[Amount])"), Scalar::Decimal(17.5));
    // TRUE = 1, FALSE = 0: (1 + 0 + 1 + 0) / 4.
    assert_eq!(value("AVERAGEA(Sales[Flag])"), Scalar::Decimal(0.5));
    // Any text counts as 0, numeric or not: "5", "", "abc" and "7" all contribute 0.
    assert_eq!(value("AVERAGEA(Sales[Note])"), Scalar::Decimal(0.0));
    // A column with rows but only BLANK cells averages 0, not BLANK.
    assert_eq!(value("AVERAGEA(Sales[Empty])"), Scalar::Decimal(0.0));
}

#[test]
fn averagea_respects_the_filter_context_and_is_blank_for_no_rows() {
    // East rows are 10 and BLANK: the BLANK row stays in the denominator.
    assert_eq!(value("CALCULATE(AVERAGEA(Sales[Amount]), Sales[Region] = \"East\")"), Scalar::Decimal(5.0));
    assert_eq!(value("CALCULATE(AVERAGEA(Sales[Amount]), Sales[Region] = \"Nowhere\")"), Scalar::Blank);
}

#[test]
fn maxa_and_mina_count_logical_values() {
    assert_eq!(value("MINA(Sales[Flag])"), Scalar::Decimal(0.0));
    assert_eq!(value("MAXA(Sales[Flag])"), Scalar::Decimal(1.0));
    // Text is not rankable, so every text value coerces to 0 before the comparison.
    assert_eq!(value("MINA(Sales[Note])"), Scalar::Decimal(0.0));
    assert_eq!(value("MAXA(Sales[Note])"), Scalar::Decimal(0.0));
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

#[test]
fn max_and_min_with_two_scalars_return_the_larger_or_smaller_value() {
    // The catalog's two-scalar overload, `MAX ( 10000, [Sales Amount] )`.
    assert_eq!(value("MAX(3, 5)"), Scalar::Integer(5));
    assert_eq!(value("MIN(3, 5)"), Scalar::Integer(3));
    assert_eq!(value("MAX(10000, SUM(Sales[Amount]))"), Scalar::Integer(10000));
    assert_eq!(value("MIN(1, SUM(Sales[Amount]))"), Scalar::Integer(1));
    // Text compares case-insensitively, like every DAX comparison.
    assert_eq!(value("MAX(\"a\", \"B\")"), Scalar::String("B".into()));
    assert_eq!(value("MIN(\"a\", \"B\")"), Scalar::String("a".into()));
    // One argument still aggregates the column, not the first cell.
    assert_eq!(value("MAX(Sales[Amount])"), Scalar::Decimal(40.0));
    assert_eq!(value("MIN(Sales[Amount])"), Scalar::Decimal(10.0));
}
