//! Evaluator tests for the text functions in `eval/text.rs`.

use gridcraft_dax::model::{DataModel, Measure, ModelColumn, ModelTable};
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Binder, Evaluator, FilterContext, parse_expression};

/// Excel serial for 2024-01-05, 2024-02-10 and 2024-03-15.
const JAN_5_2024: f64 = 45296.0;
const FEB_10_2024: f64 = 45332.0;
const MAR_15_2024: f64 = 45366.0;

fn model() -> DataModel {
    let mut model = DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Double".into()) },
                    ModelColumn { name: "Code".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Flag".into(), data_type: Some("Boolean".into()) },
                    ModelColumn { name: "When".into(), data_type: Some("DateTime".into()) },
                ],
                rows: vec![
                    vec![
                        Scalar::String("East".into()),
                        Scalar::Decimal(10.5),
                        Scalar::String("A-1".into()),
                        Scalar::Boolean(true),
                        Scalar::DateTime(JAN_5_2024),
                    ],
                    vec![
                        Scalar::String("West".into()),
                        Scalar::Decimal(20.25),
                        Scalar::String("B,2".into()),
                        Scalar::Boolean(false),
                        Scalar::DateTime(FEB_10_2024),
                    ],
                    vec![
                        Scalar::String("East".into()),
                        Scalar::Decimal(30.0),
                        Scalar::String("C\"3".into()),
                        Scalar::Boolean(true),
                        Scalar::DateTime(MAR_15_2024),
                    ],
                ],
            },
            ModelTable {
                name: "Mixed".into(),
                columns: vec![
                    ModelColumn { name: "Label".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Key".into(), data_type: None },
                ],
                rows: vec![
                    vec![Scalar::String("ten".into()), Scalar::Integer(10)],
                    vec![Scalar::String("two-text".into()), Scalar::String("2".into())],
                    vec![Scalar::String("three".into()), Scalar::Integer(3)],
                    vec![Scalar::String("blank".into()), Scalar::Blank],
                ],
            },
            ModelTable {
                name: "Items".into(),
                columns: vec![
                    ModelColumn { name: "Label".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Qty".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Note".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![Scalar::String("Alpha".into()), Scalar::Integer(3), Scalar::String("plain".into())],
                    vec![Scalar::String("Beta".into()), Scalar::Integer(1), Scalar::String("with, comma".into())],
                    vec![Scalar::String("Gamma".into()), Scalar::Integer(2), Scalar::String("with \"quote\"".into())],
                    vec![Scalar::String("Delta".into()), Scalar::Integer(4), Scalar::Blank],
                ],
            },
            ModelTable {
                name: "Clock".into(),
                columns: vec![ModelColumn { name: "At".into(), data_type: Some("DateTime".into()) }],
                // 2024-01-05 23:59:59.914: the day fraction rounds up to the next midnight.
                rows: vec![vec![Scalar::DateTime(45296.999999)]],
            },
        ],
        relationships: vec![],
        measures: vec![
            Measure {
                name: "Region List".into(),
                formula: "CONCATENATEX(Sales, Sales[Region], \",\")".into(),
                table: Some("Sales".into()),
                format: None,
                expr: None,
            },
            Measure {
                name: "Sales CSV".into(),
                formula: "TOCSV(Sales, 1, \",\", TRUE)".into(),
                table: Some("Sales".into()),
                format: None,
                expr: None,
            },
        ],
    };
    model.parse_formulas().expect("model formulas parse");
    model
}

fn eval_with(model: &DataModel, src: &str) -> Result<Scalar, String> {
    let evaluator = Evaluator::new(model).map_err(|error| error.to_string())?;
    let expr = parse_expression(src).map_err(|error| error.to_string())?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| error.to_string())?;
    evaluator.evaluate(&bound).map_err(|error| error.to_string())
}

fn eval(src: &str) -> Scalar {
    match eval_with(&model(), src) {
        Ok(value) => value,
        Err(error) => panic!("{src} failed: {error}"),
    }
}

fn eval_error(src: &str) -> String {
    match eval_with(&model(), src) {
        Ok(value) => panic!("{src} should fail, got {value:?}"),
        Err(error) => error,
    }
}

fn eval_measure(name: &str) -> Scalar {
    let model = model();
    let evaluator = Evaluator::new(&model).expect("evaluator");
    evaluator.evaluate_measure(name, FilterContext::default()).expect("measure evaluates")
}

fn text(src: &str) -> String {
    match eval(src) {
        Scalar::String(value) => value,
        other => panic!("{src} should return text, got {other:?}"),
    }
}

#[test]
fn combine_values_joins_with_delimiter() {
    assert_eq!(text(r##"COMBINEVALUES("-", "Contoso", "2020", "Q1")"##), "Contoso-2020-Q1");
    assert_eq!(text(r##"COMBINEVALUES(" | ", "a", 1, TRUE)"##), "a | 1 | TRUE");
    // BLANK coerces to the empty string and keeps its position between delimiters.
    assert_eq!(text(r##"COMBINEVALUES("|", "a", BLANK(), 2)"##), "a||2");
}

#[test]
fn concatenate_x_iterates_and_sorts() {
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], ",")"##), "Alpha,Beta,Gamma,Delta");
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label] & ":" & Items[Qty], ";")"##), "Alpha:3;Beta:1;Gamma:2;Delta:4");
    // Ascending is the default when an order-by expression is supplied.
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], "-", Items[Qty])"##), "Beta-Gamma-Alpha-Delta");
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], "-", Items[Qty], TRUE)"##), "Beta-Gamma-Alpha-Delta");
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], "|", Items[Qty], FALSE)"##), "Delta|Alpha|Gamma|Beta");
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], "|", Items[Qty], 0)"##), "Delta|Alpha|Gamma|Beta");
    // No delimiter means a plain concatenation.
    assert_eq!(text("CONCATENATEX(Items, Items[Label])"), "AlphaBetaGammaDelta");
    // An empty table yields BLANK.
    assert_eq!(eval("CONCATENATEX(FILTER(Items, Items[Qty] > 10), Items[Label], \",\")"), Scalar::Blank);
    // The function works in a measure, where the table is filtered by the filter context.
    assert_eq!(eval_measure("Region List"), Scalar::String("East,West,East".into()));
}

#[test]
fn concatenate_x_orders_by_text() {
    // Ascending string order: Alpha < Beta < Delta < Gamma.
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], ",", Items[Label])"##), "Alpha,Beta,Delta,Gamma");
    // BLANK sort keys go last ascending and first descending. Text comparison is case
    // insensitive, so a space sorts before a comma.
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], ",", Items[Note])"##), "Alpha,Gamma,Beta,Delta");
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], ",", Items[Note], 0)"##), "Delta,Beta,Gamma,Alpha");
}

#[test]
fn concatenate_x_orders_mixed_keys_deterministically() {
    // Numbers sort before text (and text after numbers) regardless of how the values print, so
    // the comparator stays transitive; BLANK stays last ascending and first descending. The
    // numeric key 10 versus the text key "2" is what the old text fallback ordered backwards.
    assert_eq!(text(r##"CONCATENATEX(Mixed, Mixed[Label], ",", Mixed[Key])"##), "three,ten,two-text,blank");
    assert_eq!(text(r##"CONCATENATEX(Mixed, Mixed[Label], ",", Mixed[Key], 0)"##), "blank,two-text,ten,three");
}

#[test]
fn concatenate_x_orders_by_every_pair() {
    // The two East rows tie on Region, so the second key (Amount, descending) decides them;
    // ignoring it left them in source order (A-1 before C"3).
    assert_eq!(text(r##"CONCATENATEX(Sales, Sales[Code], ",", Sales[Region], TRUE, Sales[Amount], FALSE)"##), "C\"3,A-1,B,2");
    // Each pair keeps its own direction: East/West descending, then Amount ascending.
    assert_eq!(text(r##"CONCATENATEX(Sales, Sales[Code], "|", Sales[Region], "DESC", Sales[Amount], "ASC")"##), "B,2|A-1|C\"3");
    // A Boolean pair direction applies per pair as well: Qty > 2 descending keeps the TRUE rows
    // first, then Label ascending breaks each group's ties.
    assert_eq!(text(r##"CONCATENATEX(Items, Items[Label], ",", Items[Qty] > 2, FALSE, Items[Label], TRUE)"##), "Alpha,Delta,Beta,Gamma");
    // The documented spelling is a bare ASC/DESC word, one per order slot.
    assert_eq!(text(r##"CONCATENATEX(Sales, Sales[Code], "|", Sales[Region], DESC, Sales[Amount], ASC)"##), "B,2|A-1|C\"3");
    let error = eval_error(r##"CONCATENATEX(Sales, Sales[Code], ",", Sales[Region], "SIDEWAYS")"##);
    assert!(error.contains("CONCATENATEX order must be ASC, DESC or a Boolean"), "{error}");
}

#[test]
fn fixed_rounds_and_groups() {
    assert_eq!(text("FIXED(384518.16, 2, FALSE)"), "384,518.16");
    assert_eq!(text("FIXED(4392768.29, 0, TRUE)"), "4392768");
    assert_eq!(text("FIXED(6741548.73, -2)"), "6,741,500");
    assert_eq!(text("FIXED(1234.5)"), "1,234.50");
    assert_eq!(text("FIXED(2.5, BLANK())"), "2.50");
    assert_eq!(text("FIXED(-1234.5678, 2, TRUE)"), "-1234.57");
    assert_eq!(text("FIXED(150, -2)"), "200");
    // Half away from zero: 99 rounds up to one hundred, and 49 stays zero.
    assert_eq!(text("FIXED(99, -2)"), "100");
    assert_eq!(text("FIXED(49, -2)"), "0");
    // Excel rounds half away from zero on the decimal digits the user supplied.
    assert_eq!(text("FIXED(2.675, 2)"), "2.68");
    assert_eq!(text("FIXED(-2.675, 2)"), "-2.68");
    assert_eq!(text("FIXED(0, 0)"), "0");
    // A negative value that rounds to zero prints without a sign: Excel rounds, then formats.
    assert_eq!(text("FIXED(-0.004, 2)"), "0.00");
    assert_eq!(text("FIXED(-49, -2)"), "0");
    assert_eq!(text("FIXED(-0.4, 0)"), "0");
    assert!(eval_error("FIXED(1, 200)").contains("decimals"));
}

#[test]
fn format_predefined_numeric_names() {
    assert_eq!(text(r##"FORMAT(12345.67, "General Number")"##), "12345.67");
    // Excel shows 15 significant digits: the binary sum of 0.1 and 0.2 reads as 0.3, not as Rust's
    // shortest round-trip form 0.30000000000000004. `General` and an empty pattern share the path.
    assert_eq!(text(r##"FORMAT(0.1 + 0.2, "General Number")"##), "0.3");
    assert_eq!(text(r##"FORMAT(0.1 + 0.2, "General")"##), "0.3");
    assert_eq!(text(r##"FORMAT(0.1 + 0.2, "")"##), "0.3");
    assert_eq!(text(r##"FORMAT(12345.67, "Currency")"##), "$12,345.67");
    assert_eq!(text(r##"FORMAT(12345.67, "Fixed")"##), "12345.67");
    assert_eq!(text(r##"FORMAT(12345.67, "Standard")"##), "12,345.67");
    assert_eq!(text(r##"FORMAT(0.742, "Percent")"##), "74.20%");
    assert_eq!(text(r##"FORMAT(12345.67, "Scientific")"##), "1.23E+04");
    assert_eq!(text(r##"FORMAT(TRUE, "Yes/No")"##), "Yes");
    assert_eq!(text(r##"FORMAT(FALSE, "Yes/No")"##), "No");
    assert_eq!(text(r##"FORMAT(FALSE, "True/False")"##), "False");
    assert_eq!(text(r##"FORMAT(TRUE, "On/Off")"##), "On");
    assert_eq!(text(r##"FORMAT(TRUE, "General")"##), "TRUE");
}

#[test]
fn format_custom_numeric_patterns() {
    assert_eq!(text(r##"FORMAT(1234.567, "$#,0.00")"##), "$1,234.57");
    assert_eq!(text(r##"FORMAT(1234.567, "#,##0")"##), "1,235");
    assert_eq!(text(r##"FORMAT(0.5, "0.00")"##), "0.50");
    assert_eq!(text(r##"FORMAT(0.5, "#.##")"##), ".5");
    assert_eq!(text(r##"FORMAT(1234567.891, "#,##0.00")"##), "1,234,567.89");
    assert_eq!(text(r##"FORMAT(0.125, "0.0%")"##), "12.5%");
    assert_eq!(text(r##"FORMAT(12, "0 ""units""")"##), "12 units");
    assert_eq!(text(r##"FORMAT(1234.5, "#,##0,")"##), "1");
    assert_eq!(text(r##"FORMAT(1234.567, "0.00E+00")"##), "1.23E+03");
    assert_eq!(text(r##"FORMAT(0.000123, "0.00E+00")"##), "1.23E-04");
    // Literal-only sections are printed verbatim.
    assert_eq!(text(r##"FORMAT(42, """N/A""")"##), "N/A");
    // Rounding happens on the 15 significant decimal digits, as in Excel.
    assert_eq!(text(r##"FORMAT(2.675, "0.00")"##), "2.68");
    assert_eq!(text(r##"FORMAT(1.005, "0.00")"##), "1.01");
}

#[test]
fn format_sections_and_signs() {
    assert_eq!(text(r##"FORMAT(-1234.5, "#,##0.00;(#,##0.00)")"##), "(1,234.50)");
    assert_eq!(text(r##"FORMAT(1234.5, "#,##0.00;(#,##0.00)")"##), "1,234.50");
    assert_eq!(text(r##"FORMAT(0, "0.00;-#,##0.00;""zero""")"##), "zero");
    assert_eq!(text(r##"FORMAT(-5, "0.00")"##), "-5.00");
    assert_eq!(text(r##"FORMAT(-5, "$0.00")"##), "-$5.00");
    // Missing sections fall back to the positive section.
    assert_eq!(text(r##"FORMAT(-5, "0.00;")"##), "-5.00");
}

#[test]
fn format_text_and_blank_values() {
    assert_eq!(text(r##"FORMAT(BLANK(), "0.00")"##), "");
    assert_eq!(text(r##"FORMAT("abc", "0.00")"##), "abc");
    assert_eq!(text(r##"FORMAT("abc", "0;0;0;@x")"##), "abcx");
}

#[test]
fn find_and_search_count_characters_and_honour_the_start() {
    // Positions are 1-based character indices, not byte offsets: `é` is one character.
    assert_eq!(eval("FIND(\"b\", \"éb\")"), Scalar::Integer(2));
    assert_eq!(eval("FIND(\"b\", \"abcb\")"), Scalar::Integer(2));
    // A later match after `start` is found: the first `a` is at 1, the one at or after 2 is at 3.
    assert_eq!(eval("FIND(\"a\", \"aXa\", 2)"), Scalar::Integer(3));
    assert_eq!(eval("FIND(\"X\", \"aXa\", 2)"), Scalar::Integer(2));
    assert_eq!(eval("FIND(\"a\", \"aXa\", 3)"), Scalar::Integer(3));
    // DAX raises an error for text that is not there unless `NotFoundValue` supplies the answer.
    assert!(eval_error("FIND(\"z\", \"aXa\")").contains("did not find"));
    assert_eq!(eval("FIND(\"z\", \"aXa\", 1, BLANK())"), Scalar::Blank);
    assert_eq!(eval("FIND(\"z\", \"aXa\", 1, -1)"), Scalar::Integer(-1));
    // `SEARCH` ignores case, including for a character whose lowercase is longer than the byte
    // it came from.
    assert_eq!(eval("SEARCH(\"x\", \"aXa\")"), Scalar::Integer(2));
    assert_eq!(eval("SEARCH(\"ÉTÉ\", \"un été chaud\")"), Scalar::Integer(4));
}

#[test]
fn replace_uses_character_positions() {
    assert_eq!(text(r##"REPLACE("DAX is so cool!", 11, 4, "fantastic")"##), "DAX is so fantastic!");
    assert_eq!(text(r##"REPLACE("123-45-6789", 1, 3, "XXX")"##), "XXX-45-6789");
    // Characters, not UTF-8 bytes.
    assert_eq!(text(r##"REPLACE("héllo", 2, 1, "e")"##), "hello");
    assert_eq!(text(r##"REPLACE("a😀b", 2, 1, "X")"##), "aXb");
    // Zero (or BLANK) replacement length inserts at the position.
    assert_eq!(text(r##"REPLACE("abc", 2, 0, "X")"##), "aXbc");
    assert_eq!(text(r##"REPLACE("abc", 2, BLANK(), "X")"##), "aXbc");
    // The replacement may be longer than the removed run.
    assert_eq!(text(r##"REPLACE("abc", 2, 1, "LONG")"##), "aLONGc");
    // A start past the end appends, and an over-long count truncates at the end.
    assert_eq!(text(r##"REPLACE("abc", 10, 1, "X")"##), "abcX");
    assert_eq!(text(r##"REPLACE("abc", 2, 10, "XY")"##), "aXY");
    assert_eq!(text(r##"REPLACE("", 1, 0, "x")"##), "x");
    assert!(eval_error(r##"REPLACE("abc", 0, 1, "x")"##).contains("StartPosition"));
    assert!(eval_error(r##"REPLACE("abc", 1, -1, "x")"##).contains("NumberOfCharacters"));
}

#[test]
fn unicode_returns_the_first_code_point() {
    assert_eq!(eval(r##"UNICODE("A")"##), Scalar::Integer(65));
    assert_eq!(eval(r##"UNICODE("abc")"##), Scalar::Integer(97));
    assert_eq!(eval(r##"UNICODE("é")"##), Scalar::Integer(233));
    assert_eq!(eval(r##"UNICODE("😀x")"##), Scalar::Integer(128_512));
    assert!(eval_error(r##"UNICODE("")"##).contains("UNICODE"));
    assert!(eval_error("UNICODE(BLANK())").contains("UNICODE"));
}

#[test]
fn to_csv_serializes_with_headers_and_escaping() {
    assert_eq!(text("TOCSV(Items, 2)"), "Label,Qty,Note\r\nAlpha,3,plain\r\nBeta,1,\"with, comma\"");
    assert_eq!(text(r##"TOCSV(Items, -1, "|", FALSE)"##), "Alpha|3|plain\r\nBeta|1|with, comma\r\nGamma|2|\"with \"\"quote\"\"\"\r\nDelta|4|");
    assert_eq!(text("TOCSV(Items, 0)"), "Label,Qty,Note");
    // An empty table still prints the header row.
    assert_eq!(text("TOCSV(FILTER(Items, Items[Qty] > 99))"), "Label,Qty,Note");
    assert_eq!(text("TOCSV(FILTER(Items, Items[Qty] > 99), 10, \",\", FALSE)"), "");
    assert_eq!(text("TOCSV(Sales, 1)"), "Region,Amount,Code,Flag,When\r\nEast,10.5,A-1,true,2024-01-05T00:00:00");
    assert!(eval_error(r##"TOCSV(Items, 1, "")"##).contains("Delimiter"));
    assert_eq!(eval_measure("Sales CSV"), eval("TOCSV(Sales, 1, \",\", TRUE)"));
}

#[test]
fn to_json_serializes_with_stable_order_and_escaping() {
    assert_eq!(text("TOJSON(Items, 1)"), "{\"header\":[\"Label\",\"Qty\",\"Note\"],\"rowCount\":4,\"data\":[[\"Alpha\",3,\"plain\"]]}");
    assert_eq!(
        text("TOJSON(Items, -1)"),
        "{\"header\":[\"Label\",\"Qty\",\"Note\"],\"rowCount\":4,\"data\":[[\"Alpha\",3,\"plain\"],[\"Beta\",1,\"with, comma\"],[\"Gamma\",2,\"with \\\"quote\\\"\"],[\"Delta\",4,null]]}"
    );
    assert_eq!(text("TOJSON(Items, 0)"), "{\"header\":[\"Label\",\"Qty\",\"Note\"],\"rowCount\":4,\"data\":[]}");
    assert_eq!(
        text("TOJSON(Sales, 1)"),
        "{\"header\":[\"Region\",\"Amount\",\"Code\",\"Flag\",\"When\"],\"rowCount\":3,\"data\":[[\"East\",10.5,\"A-1\",true,\"2024-01-05T00:00:00\"]]}"
    );
    // A derived table keeps its column order and rowCount reflects the filtered rows.
    assert_eq!(
        text("TOJSON(FILTER(Items, Items[Qty] > 2), -1)"),
        "{\"header\":[\"Label\",\"Qty\",\"Note\"],\"rowCount\":2,\"data\":[[\"Alpha\",3,\"plain\"],[\"Delta\",4,null]]}"
    );
    // The engine produces no result for an empty table.
    assert_eq!(eval("TOJSON(FILTER(Items, Items[Qty] > 99))"), Scalar::Blank);
}

#[test]
fn format_and_serialization_carry_midnight() {
    // A day fraction that rounds to 24:00:00 belongs to the next day, so the date tokens roll over
    // with the time instead of printing the previous day at midnight.
    assert_eq!(text(r##"FORMAT(DATE(2019, 3, 28) + 0.999999, "m/d/yyyy h:nn:ss")"##), "3/29/2019 0:00:00");
    // The ISO timestamp used by TOCSV and TOJSON carries the same day.
    assert_eq!(text("TOCSV(Clock, 1)"), "At\r\n2024-01-06T00:00:00");
    assert_eq!(text("TOJSON(Clock, 1)"), "{\"header\":[\"At\"],\"rowCount\":1,\"data\":[[\"2024-01-06T00:00:00\"]]}");
}

#[test]
fn concatenation_uses_dax_text_conversion() {
    assert_eq!(text("1 & 2"), "12");
    // A whole-valued number has no trailing `.0`; a fractional one keeps its digits.
    assert_eq!(text("10.0 & \"x\""), "10x");
    assert_eq!(text("1.5 & \"x\""), "1.5x");
    // A date prints as calendar text, not as its serial.
    assert_eq!(text("DATE(2019, 3, 28) & \"\""), "3/28/2019 12:00:00 AM");
    assert_eq!(text("COMBINEVALUES(\"-\", 10.0, \"Q1\")"), "10-Q1");
}

#[test]
fn fixed_rounds_half_away_from_zero_at_negative_precision() {
    assert_eq!(text("FIXED(50, -2)"), "100");
    assert_eq!(text("FIXED(6, -1)"), "10");
    // Below half rounds down, and a number shorter than the precision stays 0.
    assert_eq!(text("FIXED(49, -2)"), "0");
    assert_eq!(text("FIXED(4, -1)"), "0");
}

#[test]
fn trim_collapses_internal_spaces_but_only_ascii_space() {
    // Microsoft Learn, TRIM: "Removes all spaces from text except for single spaces between
    // words." Only character 32 counts, as the catalog states.
    assert_eq!(eval(r#"TRIM("  DAX is          awesome !!!    ")"#), Scalar::String("DAX is awesome !!!".into()));
    assert_eq!(eval(r#"TRIM("  a  b  ")"#), Scalar::String("a b".into()));
    assert_eq!(eval(r#"TRIM("a")"#), Scalar::String("a".into()));
    assert_eq!(eval(r#"TRIM("   ")"#), Scalar::String(String::new()));
    // A tab and a non-breaking space are not spaces for TRIM.
    assert_eq!(eval("TRIM(\"\ta\t\")"), Scalar::String("\ta\t".into()));
    assert_eq!(eval("TRIM(\"a\u{00A0} b\")"), Scalar::String("a\u{00A0} b".into()));
}

#[test]
fn format_dates_and_times() {
    // `FORMAT` is this family's; the only date/time function used here is `DATE`, which comes from
    // core. Time-of-day operands are written out as day fractions below, so `TIME` is not needed.
    assert_eq!(text(r##"FORMAT(DATE(2019, 3, 28), "yyyy-mm-dd")"##), "2019-03-28");
    assert_eq!(text(r##"FORMAT(DATE(2019, 3, 28), "m/d/yyyy")"##), "3/28/2019");
    assert_eq!(text(r##"FORMAT(DATE(2019, 3, 28), "Long Date")"##), "Thursday, March 28, 2019");
    assert_eq!(text(r##"FORMAT(DATE(2019, 3, 28), "ddd, mmm d, yyyy")"##), "Thu, Mar 28, 2019");
    assert_eq!(text(r##"FORMAT(DATE(2020, 6, 25), "yyyy \Qq")"##), "2020 Q2");
    // 13:05:09 and 00:05:09 as fractions of a day: written out so this test stays inside the
    // text family (`TIME` belongs to the date/time family, and a family test may only use core
    // plus its own module).
    let afternoon = (13.0 * 3600.0 + 5.0 * 60.0 + 9.0) / 86_400.0;
    let just_after_midnight = (5.0 * 60.0 + 9.0) / 86_400.0;
    assert_eq!(text(&format!(r#"FORMAT(DATE(2019, 3, 28) + {afternoon}, "hh:nn:ss")"#)), "13:05:09");
    // `mm` is minutes after hours and before seconds, month otherwise.
    assert_eq!(text(&format!(r#"FORMAT(DATE(2019, 3, 28) + {afternoon}, "hh:mm:ss")"#)), "13:05:09");
    assert_eq!(text(&format!(r#"FORMAT(DATE(2019, 3, 28) + {afternoon}, "mm:ss")"#)), "05:09");
    assert_eq!(text(&format!(r#"FORMAT(DATE(2019, 3, 28) + {afternoon}, "h:nn AM/PM")"#)), "1:05 PM");
    assert_eq!(text(&format!(r#"FORMAT(DATE(2019, 3, 28) + {just_after_midnight}, "h:nn AM/PM")"#)), "12:05 AM");
    // A date value with a numeric pattern is formatted as its serial number.
    assert_eq!(text(r##"FORMAT(DATE(2019, 3, 28), "0.00")"##), "43552.00");
    // No format (or a BLANK format) uses the general date form for dates.
    assert_eq!(text(r##"FORMAT(DATE(2019, 3, 28), BLANK())"##), "3/28/2019 12:00:00 AM");
}
