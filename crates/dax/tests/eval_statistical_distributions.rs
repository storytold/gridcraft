//! Evaluation tests for the statistical distributions, their inverses and LINEST/LINESTX.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Quantity".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Price".into(), data_type: Some("Double".into()) },
                ],
                // The last row has a BLANK Amount and must be skipped by the regressions.
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::Decimal(3.1), Scalar::Decimal(2.0)],
                    vec![Scalar::Integer(2), Scalar::Decimal(4.9), Scalar::Decimal(1.0)],
                    vec![Scalar::Integer(3), Scalar::Decimal(7.1), Scalar::Decimal(4.0)],
                    vec![Scalar::Integer(4), Scalar::Decimal(8.9), Scalar::Decimal(3.0)],
                    vec![Scalar::Integer(5), Scalar::Blank, Scalar::Decimal(6.0)],
                ],
            },
            ModelTable {
                name: "Fit".into(),
                columns: vec![
                    ModelColumn { name: "X1".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "X2".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Y".into(), data_type: Some("Double".into()) },
                ],
                rows: vec![
                    vec![Scalar::Decimal(1.0), Scalar::Decimal(2.0), Scalar::Decimal(2.5)],
                    vec![Scalar::Decimal(2.0), Scalar::Decimal(1.0), Scalar::Decimal(5.0)],
                    vec![Scalar::Decimal(3.0), Scalar::Decimal(4.0), Scalar::Decimal(7.4)],
                    vec![Scalar::Decimal(4.0), Scalar::Decimal(3.0), Scalar::Decimal(9.9)],
                    vec![Scalar::Decimal(5.0), Scalar::Decimal(6.0), Scalar::Decimal(12.4)],
                    vec![Scalar::Decimal(6.0), Scalar::Decimal(5.0), Scalar::Decimal(15.2)],
                ],
            },
        ],
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

fn eval_table(src: &str) -> Result<TableValue, String> {
    let model = model();
    let evaluator = Evaluator::new(&model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(&model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate_table(&bound).map_err(|error| error.to_string())
}

fn number(src: &str) -> f64 {
    match eval(src) {
        Ok(Scalar::Decimal(value)) => value,
        Ok(Scalar::Integer(value)) => value as f64,
        other => panic!("{src} did not evaluate to a number: {other:?}"),
    }
}

fn error(src: &str) -> String {
    match eval(src) {
        Err(message) => message,
        Ok(value) => panic!("{src} should have failed, got {value:?}"),
    }
}

/// Excel-compatible precision: relative 1e-9, with a tiny absolute floor for values near zero.
fn close(actual: f64, expected: f64) {
    let tolerance = 1e-9 * expected.abs().max(1e-12);
    assert!((actual - expected).abs() <= tolerance, "expected {expected}, got {actual}");
}

fn cell(table: &TableValue, name: &str) -> f64 {
    let index = table.column_index(name).unwrap_or_else(|| panic!("missing column {name}"));
    match table.rows.first().and_then(|row| row.get(index)) {
        Some(Scalar::Decimal(value)) => *value,
        Some(Scalar::Integer(value)) => *value as f64,
        other => panic!("column {name} is not numeric: {other:?}"),
    }
}

fn column_names(table: &TableValue) -> Vec<&str> {
    table.columns.iter().map(|column| column.name.as_str()).collect()
}

#[test]
fn beta_distribution_and_inverse() {
    close(number("BETA.DIST(0.5, 3.51, 4.02, TRUE)"), 0.577_938_368_273_035_9);
    close(number("BETA.DIST(0.5, 3.51, 4.02, FALSE)"), 2.076_672_782_936_026_5);
    close(number("BETA.DIST(2, 2, 3, TRUE, 0, 4)"), 0.6875);
    close(number("BETA.DIST(2, 2, 3, FALSE, 0, 4)"), 0.375);
    close(number("BETA.DIST(0.2, 0.5, 0.5, TRUE)"), 0.295_167_235_300_866_5);

    close(number("BETA.INV(0.5, 3.51, 4.02)"), 0.462_986_074_283_030_57);
    close(number("BETA.INV(0.75, 3.51, 4.02, 0, 10)"), 5.892_177_968_908_989);
    close(number("BETA.INV(0.975, 2, 5)"), 0.641_234_578_997_674_8);
    // Probability 1 is documented as valid for BETA.INV and returns the upper bound.
    close(number("BETA.INV(1, 2, 5)"), 1.0);
    close(number("BETA.INV(1, 2, 5, 10, 20)"), 20.0);
}

#[test]
fn chi_squared_distribution_and_inverse() {
    close(number("CHISQ.DIST(2.5, 10, TRUE)"), 0.009_124_279_218_395_273);
    close(number("CHISQ.DIST(2.5, 10, FALSE)"), 0.014_572_387_535_613_511);
    close(number("CHISQ.DIST(2, 2, TRUE)"), 0.632_120_558_828_557_7);
    close(number("CHISQ.DIST.RT(2.5, 10)"), 0.990_875_720_781_604_7);

    close(number("CHISQ.INV(0.95, 10)"), 18.307_038_053_275_146);
    close(number("CHISQ.INV(0.6, 2)"), 1.832_581_463_748_31);
    close(number("CHISQ.INV(0, 10)"), 0.0);
    close(number("CHISQ.INV.RT(0.05, 10)"), 18.307_038_053_275_15);
    close(number("CHISQ.INV.RT(0.01, 3)"), 11.344_866_730_144_368);
    close(number("CHISQ.INV.RT(1, 10)"), 0.0);

    // The distribution truncates degrees of freedom; the inverse rounds them.
    close(number("CHISQ.DIST(2.5, 10.9, TRUE)"), 0.009_124_279_218_395_273);
    close(number("CHISQ.INV(0.95, 10.4)"), 18.307_038_053_275_146);
}

#[test]
fn normal_distribution_and_inverse() {
    close(number("NORM.DIST(42, 40, 1.5, TRUE)"), 0.908_788_780_274_132_1);
    close(number("NORM.DIST(42, 40, 1.5, FALSE)"), 0.109_340_049_783_995_77);
    close(number("NORM.INV(0.95, 40, 1.5)"), 42.467_280_440_427_21);

    close(number("NORM.S.DIST(1.96, TRUE)"), 0.975_002_104_851_779_5);
    close(number("NORM.S.DIST(1.96, FALSE)"), 0.058_440_944_333_451_476);
    close(number("NORM.S.INV(0.975)"), 1.959_963_984_540_054);
    close(number("NORM.S.INV(0.01)"), -2.326_347_874_040_840_8);
}

#[test]
fn student_t_distribution_and_inverse() {
    close(number("T.DIST(2, 10, TRUE)"), 0.963_305_982_614_629_9);
    close(number("T.DIST(2, 10, FALSE)"), 0.061_145_766_321_218_174);
    close(number("T.DIST(-2, 10, TRUE)"), 0.036_694_017_385_370_18);
    close(number("T.DIST.2T(2, 10)"), 0.073_388_034_770_740_37);
    close(number("T.DIST.2T(0, 10)"), 1.0);
    close(number("T.DIST.RT(2, 10)"), 0.036_694_017_385_370_18);
    close(number("T.DIST.RT(-2, 10)"), 0.963_305_982_614_629_9);

    close(number("T.INV(0.75, 2)"), 0.816_496_580_927_726_1);
    close(number("T.INV(0.95, 10)"), 1.812_461_122_811_675_6);
    close(number("T.INV(0.01, 7)"), -2.997_951_566_868_528_6);
    close(number("T.INV.2T(0.05, 10)"), 2.228_138_851_986_275_3);
    close(number("T.INV.2T(1, 10)"), 0.0);
    close(number("T.DIST(2, 10.9, TRUE)"), 0.963_305_982_614_629_9);
}

#[test]
fn confidence_intervals_exponential_and_poisson() {
    close(number("CONFIDENCE.NORM(0.05, 2.5, 50)"), 0.692_951_912_174_838_9);
    close(number("CONFIDENCE.T(0.05, 2.5, 50)"), 0.710_492_138_739_324_6);
    close(number("CONFIDENCE.T(0.05, 1, 50)"), 0.284_196_855_495_729_87);

    close(number("EXPON.DIST(0.2, 10, TRUE)"), 0.864_664_716_763_387_3);
    close(number("EXPON.DIST(0.2, 10, FALSE)"), 1.353_352_832_366_127);
    close(number("EXPON.DIST(0, 10, TRUE)"), 0.0);

    close(number("POISSON.DIST(2, 5, TRUE)"), 0.124_652_019_483_081_08);
    close(number("POISSON.DIST(2, 5, FALSE)"), 0.084_224_337_488_568_32);
    close(number("POISSON.DIST(0, 0, TRUE)"), 1.0);
    close(number("POISSON.DIST(2.9, 5, TRUE)"), 0.124_652_019_483_081_08);
}

#[test]
fn linest_single_predictor() {
    let table = eval_table("LINEST(Sales[Amount], Sales[Quantity])").expect("LINEST evaluates");
    assert_eq!(table.rows.len(), 1);
    assert_eq!(
        column_names(&table),
        [
            "Slope1",
            "Intercept",
            "StandardErrorSlope1",
            "StandardErrorIntercept",
            "CoefficientOfDetermination",
            "StandardError",
            "FStatistic",
            "DegreesOfFreedom",
            "RegressionSumOfSquares",
            "ResidualSumOfSquares",
        ]
    );
    close(cell(&table, "Slope1"), 1.96);
    close(cell(&table, "Intercept"), 1.100_000_000_000_000_3);
    close(cell(&table, "StandardErrorSlope1"), 0.056_568_542_494_923_57);
    close(cell(&table, "StandardErrorIntercept"), 0.154_919_333_848_296_04);
    close(cell(&table, "CoefficientOfDetermination"), 0.998_336_798_336_798_3);
    close(cell(&table, "StandardError"), 0.126_491_106_406_734_67);
    close(cell(&table, "FStatistic"), 1_200.500_000_000_01);
    close(cell(&table, "DegreesOfFreedom"), 2.0);
    close(cell(&table, "RegressionSumOfSquares"), 19.208_000_000_000_002);
    close(cell(&table, "ResidualSumOfSquares"), 0.031_999_999_999_999_74);
}

#[test]
fn linest_without_intercept() {
    let table = eval_table("LINEST(Sales[Amount], Sales[Quantity], FALSE)").expect("LINEST evaluates");
    close(cell(&table, "Slope1"), 2.326_666_666_666_666_7);
    close(cell(&table, "Intercept"), 0.0);
    assert_eq!(table.rows[0][table.column_index("StandardErrorIntercept").expect("column")], Scalar::Blank);
    close(cell(&table, "CoefficientOfDetermination"), 0.994_862_370_334_068_5);
    close(cell(&table, "StandardError"), 0.528_730_134_903_956);
    close(cell(&table, "FStatistic"), 580.926_868_044_515_3);
    close(cell(&table, "DegreesOfFreedom"), 3.0);
    close(cell(&table, "RegressionSumOfSquares"), 162.401_333_333_333_35);
    close(cell(&table, "ResidualSumOfSquares"), 0.838_666_666_666_666_4);
}

#[test]
fn linest_multiple_predictors() {
    let table = eval_table("LINEST(Fit[Y], Fit[X1], Fit[X2])").expect("LINEST evaluates");
    assert_eq!(column_names(&table).len(), 12);
    assert_eq!(column_names(&table)[..3], ["Slope1", "Slope2", "Intercept"]);
    close(cell(&table, "Slope1"), 2.556_249_999_999_997_7);
    close(cell(&table, "Slope2"), -0.043_749_999_999_999_956);
    close(cell(&table, "Intercept"), -0.060_416_666_666_664_83);
    close(cell(&table, "StandardErrorSlope1"), 0.055_577_252_707_546_654);
    close(cell(&table, "StandardErrorSlope2"), 0.055_577_252_707_546_67);
    close(cell(&table, "StandardErrorIntercept"), 0.125_686_770_790_929_94);
    close(cell(&table, "CoefficientOfDetermination"), 0.999_542_838_299_658_3);
    close(cell(&table, "StandardError"), 0.130_170_827_931_777_2);
    close(cell(&table, "FStatistic"), 3_279.614_754_098_379_8);
    close(cell(&table, "DegreesOfFreedom"), 3.0);
    close(cell(&table, "RegressionSumOfSquares"), 111.1425);
    close(cell(&table, "ResidualSumOfSquares"), 0.050_833_333_333_333_036);
}

#[test]
fn linestx_matches_linest() {
    let from_columns = eval_table("LINEST(Sales[Amount], Sales[Quantity])").expect("LINEST");
    let from_expressions = eval_table("LINESTX(Sales, Sales[Amount], Sales[Quantity])").expect("LINESTX");
    assert_eq!(column_names(&from_columns), column_names(&from_expressions));
    for name in column_names(&from_columns) {
        let index = from_columns.column_index(name).expect("column");
        assert_eq!(from_columns.rows[0][index], from_expressions.rows[0][index], "{name}");
    }

    // A scaled X expression halves the slope but leaves the fit statistics unchanged.
    let scaled = eval_table("LINESTX(Sales, Sales[Amount], Sales[Quantity] * 2)").expect("LINESTX");
    close(cell(&scaled, "Slope1"), 0.98);
    close(cell(&scaled, "Intercept"), 1.100_000_000_000_001_2);
    close(cell(&scaled, "StandardErrorSlope1"), 0.028_284_271_247_461_815);
    close(cell(&scaled, "StandardErrorIntercept"), 0.154_919_333_848_296_2);
    close(cell(&scaled, "CoefficientOfDetermination"), 0.998_336_798_336_798_3);
    close(cell(&scaled, "FStatistic"), 1_200.500_000_000_007_3);

    let no_intercept = eval_table("LINESTX(Sales, Sales[Amount], Sales[Quantity], FALSE)").expect("LINESTX");
    close(cell(&no_intercept, "Slope1"), 2.326_666_666_666_666_7);
    close(cell(&no_intercept, "Intercept"), 0.0);
    assert_eq!(no_intercept.rows[0][no_intercept.column_index("StandardErrorIntercept").expect("column")], Scalar::Blank);
}

#[test]
fn domain_errors() {
    for (src, expected) in [
        ("BETA.DIST(0.5, 0, 2, TRUE)", "Alpha and Beta must be positive"),
        ("BETA.DIST(0.5, 2, -1, TRUE)", "Alpha and Beta must be positive"),
        ("BETA.DIST(1.5, 2, 3, TRUE)", "X must lie between A and B"),
        ("BETA.DIST(0.5, 2, 3, TRUE, 1, 1)", "A must be less than B"),
        ("BETA.INV(0, 2, 3)", "Probability"),
        ("BETA.INV(1.5, 2, 3)", "Probability"),
        ("CHISQ.DIST(-1, 5, TRUE)", "X must be non-negative"),
        ("CHISQ.DIST(1, 0, TRUE)", "Deg_freedom"),
        ("CHISQ.DIST.RT(-1, 5)", "X must be non-negative"),
        ("CHISQ.INV(-0.1, 5)", "Probability"),
        ("CHISQ.INV(1, 5)", "Probability"),
        ("CHISQ.INV.RT(0, 5)", "Probability"),
        ("CONFIDENCE.NORM(0, 1, 10)", "Alpha"),
        ("CONFIDENCE.NORM(1, 1, 10)", "Alpha"),
        ("CONFIDENCE.NORM(0.05, 0, 10)", "Standard_dev"),
        ("CONFIDENCE.NORM(0.05, 1, 0)", "Size"),
        ("CONFIDENCE.T(0.05, 1, 1)", "greater than 1"),
        ("EXPON.DIST(-1, 2, TRUE)", "X must be non-negative"),
        ("EXPON.DIST(1, 0, TRUE)", "Lambda"),
        ("NORM.DIST(0, 0, 0, TRUE)", "Standard_dev"),
        ("NORM.DIST(0, 0, -1, FALSE)", "Standard_dev"),
        ("NORM.INV(0, 0, 1)", "Probability"),
        ("NORM.INV(1, 0, 1)", "Probability"),
        ("NORM.S.INV(0)", "Probability"),
        ("POISSON.DIST(-1, 5, TRUE)", "X must be non-negative"),
        ("POISSON.DIST(1, -5, TRUE)", "Mean"),
        ("T.DIST(1, 0, TRUE)", "Deg_freedom"),
        ("T.DIST.2T(-1, 5)", "X must be non-negative"),
        ("T.DIST.2T(1, 0)", "Deg_freedom"),
        ("T.INV(0, 5)", "Probability"),
        ("T.INV(1, 5)", "Probability"),
        ("T.INV.2T(0, 5)", "Probability"),
    ] {
        let message = error(src);
        assert!(message.contains(expected), "{src}: expected {expected:?} in {message:?}");
    }

    // A BLANK bound is an error, not an omitted argument.
    let message = error("BETA.DIST(0.5, 2, 3, TRUE, BLANK())");
    assert!(message.contains("BLANK"), "{message}");

    // Column references from different tables cannot be combined.
    let message = eval_table("LINEST(Sales[Amount], Fit[X1])").expect_err("mismatched tables");
    assert!(message.contains("same table"), "{message}");
}
