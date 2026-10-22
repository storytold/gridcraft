//! Evaluator tests for the statistical aggregates: combinatorics, geometric mean, median,
//! percentiles, variance/standard deviation, ranking and SAMPLE.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::value::{Scalar, TableValue};
use gridcraft_dax::{Binder, Evaluator, parse_expression};

fn column(name: &str, data_type: &str) -> ModelColumn {
    ModelColumn { name: name.into(), data_type: Some(data_type.into()) }
}

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![column("Region", "String"), column("Amount", "Double"), column("Quantity", "Int64"), column("Flag", "Boolean")],
                rows: vec![
                    vec![Scalar::String("East".into()), Scalar::Integer(10), Scalar::Integer(2), Scalar::Boolean(true)],
                    vec![Scalar::String("West".into()), Scalar::Integer(20), Scalar::Integer(3), Scalar::Boolean(false)],
                    vec![Scalar::String("East".into()), Scalar::Integer(30), Scalar::Integer(4), Scalar::Boolean(true)],
                    vec![Scalar::String("West".into()), Scalar::Integer(40), Scalar::Integer(5), Scalar::Boolean(false)],
                ],
            },
            ModelTable {
                name: "Sparse".into(),
                columns: vec![column("Label", "String"), column("Value", "Double")],
                rows: vec![
                    vec![Scalar::String("a".into()), Scalar::Integer(2)],
                    vec![Scalar::String("b".into()), Scalar::Blank],
                    vec![Scalar::String("c".into()), Scalar::Integer(4)],
                    vec![Scalar::String("d".into()), Scalar::Blank],
                ],
            },
            ModelTable { name: "Nulls".into(), columns: vec![column("Value", "Double")], rows: vec![vec![Scalar::Blank], vec![Scalar::Blank]] },
            ModelTable { name: "Empty".into(), columns: vec![column("Value", "Double")], rows: vec![] },
            ModelTable { name: "Single".into(), columns: vec![column("Value", "Double")], rows: vec![vec![Scalar::Integer(5)]] },
            ModelTable {
                name: "Ties".into(),
                columns: vec![column("Value", "Int64")],
                rows: vec![
                    vec![Scalar::Integer(10)],
                    vec![Scalar::Integer(10)],
                    vec![Scalar::Integer(20)],
                    vec![Scalar::Integer(20)],
                    vec![Scalar::Integer(20)],
                ],
            },
        ],
        relationships: vec![],
        measures: vec![],
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

fn number(value: Scalar) -> f64 {
    match value {
        Scalar::Decimal(value) => value,
        Scalar::Integer(value) => value as f64,
        Scalar::Currency(value) => value as f64 / 10_000.0,
        other => panic!("expected a numeric scalar, got {other:?}"),
    }
}

fn assert_close(actual: Scalar, expected: f64) {
    let value = number(actual);
    let tolerance = 1e-9 * expected.abs().max(1.0);
    assert!((value - expected).abs() <= tolerance, "expected {expected}, got {value}");
}

fn assert_integer(actual: Scalar, expected: i64) {
    assert_eq!(actual, Scalar::Integer(expected));
}

// ---------------------------------------------------------------------------------------------
// COMBIN / COMBINA / PERMUT
// ---------------------------------------------------------------------------------------------

#[test]
fn combinatorics() {
    assert_integer(eval("COMBIN(8, 3)").expect("COMBIN"), 56);
    assert_integer(eval("COMBIN(8, 2)").expect("COMBIN"), 28);
    assert_integer(eval("COMBIN(8, 0)").expect("COMBIN"), 1);
    assert_integer(eval("COMBIN(8, 8)").expect("COMBIN"), 1);
    // Exact integer arithmetic far beyond f64's 2^53 integer range.
    assert_integer(eval("COMBIN(60, 30)").expect("COMBIN large"), 118_264_581_564_861_424);
    assert_integer(eval("COMBIN(52, 5)").expect("COMBIN 52 choose 5"), 2_598_960);
    // Arguments round to the nearest integer, as Excel does.
    assert_integer(eval("COMBIN(8.9, 3.9)").expect("COMBIN rounds"), 126);

    assert_integer(eval("COMBINA(8, 3)").expect("COMBINA"), 120);
    assert_integer(eval("COMBINA(4, 3)").expect("COMBINA"), 20);
    assert_integer(eval("COMBINA(10, 3)").expect("COMBINA"), 220);
    assert_integer(eval("COMBINA(0, 0)").expect("COMBINA both zero"), 1);
    assert_integer(eval("COMBINA(5, 0)").expect("COMBINA nothing chosen"), 1);

    assert_close(eval("PERMUT(8, 3)").expect("PERMUT"), 336.0);
    assert_close(eval("PERMUT(5, 0)").expect("PERMUT"), 1.0);
    assert_close(eval("PERMUT(5, 5)").expect("PERMUT"), 120.0);
    assert_close(eval("PERMUT(8.9, 3.9)").expect("PERMUT rounds"), 3024.0);
}

#[test]
fn combinatoric_domain_errors() {
    assert!(eval("COMBIN(-1, 0)").is_err());
    assert!(eval("COMBIN(2, 3)").is_err());
    assert!(eval("COMBIN(8, -1)").is_err());
    assert!(eval("COMBINA(2, 3)").is_err());
    assert!(eval("COMBINA(-1, 0)").is_err());
    assert!(eval("PERMUT(0, 0)").is_err());
    assert!(eval("PERMUT(3, 4)").is_err());
    assert!(eval("PERMUT(-3, 1)").is_err());
}

// ---------------------------------------------------------------------------------------------
// GEOMEAN / GEOMEANX
// ---------------------------------------------------------------------------------------------

#[test]
fn geometric_mean() {
    // (10 * 20 * 30 * 40)^(1/4)
    assert_close(eval("GEOMEAN(Sales[Amount])").expect("GEOMEAN"), 240_000_f64.powf(0.25));
    assert_close(eval("GEOMEAN(Sales[Amount])").expect("GEOMEAN"), 22.133_638_394_006_432);
    // Blanks are ignored: geometric mean of 2 and 4.
    assert_close(eval("GEOMEAN(Sparse[Value])").expect("GEOMEAN blanks"), 8_f64.sqrt());
    assert_close(eval("GEOMEANX(Sales, Sales[Amount])").expect("GEOMEANX"), 22.133_638_394_006_432);
    assert_close(eval("GEOMEANX(Sales, Sales[Amount] * 2)").expect("GEOMEANX expression"), 2.0 * 240_000_f64.powf(0.25));
    assert_close(eval("GEOMEANX(Sparse, Sparse[Value])").expect("GEOMEANX blanks"), 8_f64.sqrt());
    // Non-positive values do not contribute; a single positive value is its own mean.
    assert_close(eval("GEOMEANX(Sales, Sales[Amount] - 20)").expect("GEOMEANX non-positive"), (10.0 * 20.0_f64).powf(1.0 / 2.0));
}

// ---------------------------------------------------------------------------------------------
// MEDIAN / MEDIANX
// ---------------------------------------------------------------------------------------------

#[test]
fn median_and_medianx() {
    assert_eq!(eval("MEDIAN(Sales[Amount])").expect("MEDIAN"), Scalar::Decimal(25.0));
    assert_eq!(eval("MEDIANX(Sales, Sales[Amount])").expect("MEDIANX"), Scalar::Decimal(25.0));
    // MEDIAN ignores blanks: median of 2 and 4.
    assert_eq!(eval("MEDIAN(Sparse[Value])").expect("MEDIAN blanks"), Scalar::Decimal(3.0));
    // MEDIANX includes blanks: sorted [BLANK, BLANK, 2, 4] has 1.0 as its median.
    assert_eq!(eval("MEDIANX(Sparse, Sparse[Value])").expect("MEDIANX blanks"), Scalar::Decimal(1.0));
    assert_eq!(eval("MEDIAN(Single[Value])").expect("MEDIAN single"), Scalar::Decimal(5.0));
    assert_eq!(eval("MEDIANX(Sales, Sales[Amount] / 2)").expect("MEDIANX expression"), Scalar::Decimal(12.5));
}

// ---------------------------------------------------------------------------------------------
// Percentiles
// ---------------------------------------------------------------------------------------------

#[test]
fn percentile_inclusive() {
    assert_eq!(eval("PERCENTILE.INC(Sales[Amount], 0)").expect("PERCENTILE.INC min"), Scalar::Decimal(10.0));
    assert_eq!(eval("PERCENTILE.INC(Sales[Amount], 0.5)").expect("PERCENTILE.INC median"), Scalar::Decimal(25.0));
    assert_eq!(eval("PERCENTILE.INC(Sales[Amount], 1)").expect("PERCENTILE.INC max"), Scalar::Decimal(40.0));
    // rank = 0.25 * (4 - 1) = 0.75 -> 10 + 0.75 * (20 - 10)
    assert_eq!(eval("PERCENTILE.INC(Sales[Amount], 0.25)").expect("PERCENTILE.INC interpolation"), Scalar::Decimal(17.5));
    // PERCENTILE.INC ignores blanks in the column: values 2 and 4.
    assert_eq!(eval("PERCENTILE.INC(Sparse[Value], 0.5)").expect("PERCENTILE.INC blanks"), Scalar::Decimal(3.0));
    assert_eq!(eval("PERCENTILEX.INC(Sales, Sales[Amount], 0.25)").expect("PERCENTILEX.INC"), Scalar::Decimal(17.5));
    // PERCENTILEX.INC includes blanks: sorted [BLANK, BLANK, 2, 4], rank = 0.5 * 3 = 1.5.
    assert_eq!(eval("PERCENTILEX.INC(Sparse, Sparse[Value], 0.25)").expect("PERCENTILEX.INC blank position"), Scalar::Blank);
    assert_eq!(eval("PERCENTILEX.INC(Sparse, Sparse[Value], 0.5)").expect("PERCENTILEX.INC interpolation"), Scalar::Decimal(1.0));
}

#[test]
fn percentile_exclusive() {
    assert_eq!(eval("PERCENTILE.EXC(Sales[Amount], 0.25)").expect("PERCENTILE.EXC"), Scalar::Decimal(12.5));
    assert_eq!(eval("PERCENTILE.EXC(Sales[Amount], 0.5)").expect("PERCENTILE.EXC median"), Scalar::Decimal(25.0));
    assert_eq!(eval("PERCENTILE.EXC(Sales[Amount], 0.75)").expect("PERCENTILE.EXC"), Scalar::Decimal(37.5));
    // Boundaries 1/(n+1) and n/(n+1) are valid for n = 4.
    assert_eq!(eval("PERCENTILE.EXC(Sales[Amount], 0.2)").expect("PERCENTILE.EXC lower bound"), Scalar::Decimal(10.0));
    assert_eq!(eval("PERCENTILE.EXC(Sales[Amount], 0.8)").expect("PERCENTILE.EXC upper bound"), Scalar::Decimal(40.0));
    // Blanks are ignored by the column form: values 2 and 4.
    assert_eq!(eval("PERCENTILE.EXC(Sparse[Value], 0.5)").expect("PERCENTILE.EXC blanks"), Scalar::Decimal(3.0));
    // The iterator form counts blanks: sorted [BLANK, BLANK, 2, 4]; k = 0.6 -> rank 2 -> 2.
    assert_eq!(eval("PERCENTILEX.EXC(Sparse, Sparse[Value], 0.6)").expect("PERCENTILEX.EXC"), Scalar::Decimal(2.0));
    assert_eq!(eval("PERCENTILEX.EXC(Sparse, Sparse[Value], 0.8)").expect("PERCENTILEX.EXC upper"), Scalar::Decimal(4.0));
}

#[test]
fn percentile_errors() {
    assert!(eval("PERCENTILE.INC(Sales[Amount], -0.1)").is_err());
    assert!(eval("PERCENTILE.INC(Sales[Amount], 1.1)").is_err());
    // 0.1 < 1/(4+1) and 0.9 > 4/(4+1) are outside the exclusive range.
    assert!(eval("PERCENTILE.EXC(Sales[Amount], 0.1)").is_err());
    assert!(eval("PERCENTILE.EXC(Sales[Amount], 0.9)").is_err());
    assert!(eval("PERCENTILE.EXC(Single[Value], 0.5)").is_ok());
    assert!(eval("PERCENTILEX.EXC(Sales, Sales[Amount], 0.1)").is_err());
}

// ---------------------------------------------------------------------------------------------
// VAR / STDEV
// ---------------------------------------------------------------------------------------------

#[test]
fn variance_and_standard_deviation() {
    // Values 10, 20, 30, 40; mean 25; sum of squared deviations 500.
    assert_close(eval("VAR.P(Sales[Amount])").expect("VAR.P"), 125.0);
    assert_close(eval("VAR.S(Sales[Amount])").expect("VAR.S"), 500.0 / 3.0);
    assert_close(eval("STDEV.P(Sales[Amount])").expect("STDEV.P"), 125.0_f64.sqrt());
    assert_close(eval("STDEV.S(Sales[Amount])").expect("STDEV.S"), (500.0_f64 / 3.0).sqrt());
    assert_close(eval("VARX.P(Sales, Sales[Amount])").expect("VARX.P"), 125.0);
    assert_close(eval("VARX.S(Sales, Sales[Amount])").expect("VARX.S"), 500.0 / 3.0);
    assert_close(eval("STDEVX.P(Sales, Sales[Amount])").expect("STDEVX.P"), 125.0_f64.sqrt());
    assert_close(eval("STDEVX.S(Sales, Sales[Amount])").expect("STDEVX.S"), (500.0_f64 / 3.0).sqrt());
    // Blanks are ignored: numeric values 2 and 4.
    assert_close(eval("VAR.P(Sparse[Value])").expect("VAR.P blanks"), 1.0);
    assert_close(eval("VAR.S(Sparse[Value])").expect("VAR.S blanks"), 2.0);
    assert_close(eval("STDEVX.P(Sparse, Sparse[Value])").expect("STDEVX.P blanks"), 1.0);
    assert_close(eval("STDEVX.S(Sparse, Sparse[Value])").expect("STDEVX.S blanks"), 2.0_f64.sqrt());
}

#[test]
fn variance_requires_two_values() {
    assert!(eval("VAR.P(Single[Value])").is_err());
    assert!(eval("VAR.S(Single[Value])").is_err());
    assert!(eval("STDEV.P(Single[Value])").is_err());
    assert!(eval("STDEV.S(Single[Value])").is_err());
    assert!(eval("VARX.P(Empty, Empty[Value])").is_err());
    assert!(eval("STDEVX.S(Empty, Empty[Value])").is_err());
    assert!(eval("VAR.P(Nulls[Value])").is_err());
}

// ---------------------------------------------------------------------------------------------
// RANK.EQ
// ---------------------------------------------------------------------------------------------

#[test]
fn rank_eq() {
    assert_integer(eval("RANK.EQ(30, Sales[Amount])").expect("RANK.EQ descending"), 2);
    assert_integer(eval("RANK.EQ(40, Sales[Amount])").expect("RANK.EQ top"), 1);
    assert_integer(eval("RANK.EQ(10, Sales[Amount])").expect("RANK.EQ bottom"), 4);
    assert_integer(eval("RANK.EQ(10, Sales[Amount], 1)").expect("RANK.EQ ascending"), 1);
    assert_integer(eval("RANK.EQ(30, Sales[Amount], 1)").expect("RANK.EQ ascending"), 3);
    assert_integer(eval("RANK.EQ(30, Sales[Amount], 0)").expect("RANK.EQ explicit descending"), 2);
    // Ties share the top rank and the next rank skips the duplicates.
    assert_integer(eval("RANK.EQ(10, Ties[Value])").expect("RANK.EQ tie"), 4);
    assert_integer(eval("RANK.EQ(20, Ties[Value])").expect("RANK.EQ tied top"), 1);
    assert_integer(eval("RANK.EQ(20, Ties[Value], 1)").expect("RANK.EQ tie ascending"), 3);
    // A value that is not in the column, or a BLANK value, returns BLANK.
    assert_eq!(eval("RANK.EQ(25, Sales[Amount])").expect("RANK.EQ missing"), Scalar::Blank);
    assert_eq!(eval("RANK.EQ(BLANK(), Sales[Amount])").expect("RANK.EQ blank"), Scalar::Blank);
}

// ---------------------------------------------------------------------------------------------
// RANKX
// ---------------------------------------------------------------------------------------------

#[test]
fn rank_x() {
    assert_integer(eval("RANKX(Sales, Sales[Amount], 30)").expect("RANKX default"), 2);
    assert_integer(eval("RANKX(Sales, Sales[Amount], 30, 1)").expect("RANKX ascending"), 3);
    // A value that is not in the table is ranked as if it had been added.
    assert_integer(eval("RANKX(Sales, Sales[Amount], 25)").expect("RANKX missing value"), 3);
    assert_integer(eval("RANKX(Sales, Sales[Amount], 45)").expect("RANKX above maximum"), 1);
    // Skip (default) leaves gaps after ties; Dense does not.
    assert_integer(eval("RANKX(Ties, Ties[Value], 10)").expect("RANKX skip"), 4);
    assert_integer(eval("RANKX(Ties, Ties[Value], 10, 0, \"Dense\")").expect("RANKX dense"), 2);
    assert_integer(eval("RANKX(Ties, Ties[Value], 15, 1)").expect("RANKX ascending skip"), 3);
    assert_integer(eval("RANKX(Ties, Ties[Value], 15, 1, \"Dense\")").expect("RANKX ascending dense"), 2);
    assert_integer(eval("RANKX(Ties, Ties[Value], 15, 0)").expect("RANKX descending skip"), 4);
    // BLANK expression results are ranked as zero.
    assert_integer(eval("RANKX(Sparse, Sparse[Value], 0)").expect("RANKX blank as zero"), 3);
    assert!(eval("RANKX(Ties, Ties[Value], 10, 0, \"Middle\")").is_err());
    assert!(eval("RANKX(Ties, Ties[Value], 10, 2)").is_err());
}

// ---------------------------------------------------------------------------------------------
// SAMPLE
// ---------------------------------------------------------------------------------------------

fn sampled_amounts(source: &str) -> Vec<Scalar> {
    let table = eval_table(source).expect("SAMPLE table");
    table.rows.iter().filter_map(|row| row.get(1).cloned()).collect()
}

#[test]
fn sample() {
    // Descending by Amount: 40, 30, 20, 10. A size of 2 keeps the first and last rows.
    assert_eq!(sampled_amounts("SAMPLE(2, Sales, Sales[Amount])"), vec![Scalar::Integer(40), Scalar::Integer(10)]);
    assert_eq!(sampled_amounts("SAMPLE(3, Sales, Sales[Amount])"), vec![Scalar::Integer(40), Scalar::Integer(20), Scalar::Integer(10)]);
    assert_eq!(
        sampled_amounts("SAMPLE(4, Sales, Sales[Amount])"),
        vec![Scalar::Integer(40), Scalar::Integer(30), Scalar::Integer(20), Scalar::Integer(10)]
    );
    // Order 1 sorts ascending.
    assert_eq!(sampled_amounts("SAMPLE(2, Sales, Sales[Amount], 1)"), vec![Scalar::Integer(10), Scalar::Integer(40)]);
    // A sample larger than the table returns every row.
    assert_eq!(eval_table("SAMPLE(10, Sales, Sales[Amount])").expect("SAMPLE clamp").rows.len(), 4);
    // Size 0 or less returns an empty table.
    assert!(eval_table("SAMPLE(0, Sales, Sales[Amount])").expect("SAMPLE zero").rows.is_empty());
    assert!(eval_table("SAMPLE(-5, Sales, Sales[Amount])").expect("SAMPLE negative").rows.is_empty());
    // The result keeps the source columns.
    let table = eval_table("SAMPLE(2, Sales, Sales[Amount])").expect("SAMPLE columns");
    assert_eq!(table.columns.len(), 4);
}

// ---------------------------------------------------------------------------------------------
// Empty and all-blank inputs
// ---------------------------------------------------------------------------------------------

#[test]
fn empty_and_all_blank_inputs() {
    assert_eq!(eval("MEDIAN(Empty[Value])").expect("MEDIAN empty"), Scalar::Blank);
    assert_eq!(eval("MEDIANX(Empty, Empty[Value])").expect("MEDIANX empty"), Scalar::Blank);
    assert_eq!(eval("GEOMEAN(Empty[Value])").expect("GEOMEAN empty"), Scalar::Blank);
    assert_eq!(eval("GEOMEANX(Empty, Empty[Value])").expect("GEOMEANX empty"), Scalar::Blank);
    assert_eq!(eval("PERCENTILE.INC(Empty[Value], 0.5)").expect("PERCENTILE.INC empty"), Scalar::Blank);
    assert_eq!(eval("PERCENTILE.EXC(Empty[Value], 0.5)").expect("PERCENTILE.EXC empty"), Scalar::Blank);
    assert_eq!(eval("PERCENTILEX.INC(Empty, Empty[Value], 0.5)").expect("PERCENTILEX.INC empty"), Scalar::Blank);
    assert_eq!(eval("PERCENTILEX.EXC(Empty, Empty[Value], 0.5)").expect("PERCENTILEX.EXC empty"), Scalar::Blank);
    assert_eq!(eval("MEDIAN(Nulls[Value])").expect("MEDIAN all blank"), Scalar::Blank);
    assert_eq!(eval("MEDIANX(Nulls, Nulls[Value])").expect("MEDIANX all blank"), Scalar::Blank);
    assert_eq!(eval("GEOMEAN(Nulls[Value])").expect("GEOMEAN all blank"), Scalar::Blank);
    assert_eq!(eval("PERCENTILEX.INC(Nulls, Nulls[Value], 0.5)").expect("PERCENTILEX.INC all blank"), Scalar::Blank);
}

#[test]
fn column_arguments_are_required() {
    assert!(eval("MEDIAN(1)").is_err());
    assert!(eval("VAR.P(2)").is_err());
}
