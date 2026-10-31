//! Data-type, literal and operator-table tests for `gridcraft-dax`.

use gridcraft_dax::ast::{DateTimeLiteral, Expr, ExprKind, Literal};
use gridcraft_dax::types::{
    DataType, InferredType, PseudoType, TypeError, arithmetic_type, binary_type, comparison_coercion, comparison_compatible, comparison_type,
    infer_type, literal_type, numeric_matrix_order, unary_type,
};
use gridcraft_dax::{BinaryOp, Span, TokenKind, UnaryOp, parse_expression, print_expression, tokenize};

fn parse(src: &str) -> Expr {
    match parse_expression(src) {
        Ok(expr) => expr,
        Err(error) => panic!("{src:?} did not parse: {error}"),
    }
}

fn literal(src: &str) -> Literal {
    match parse(src).kind {
        ExprKind::Literal(literal) => literal,
        other => panic!("{src:?} is not a literal: {other:?}"),
    }
}

/// Parses, prints, re-parses and asserts the AST is unchanged. Returns the canonical text.
fn round_trip(src: &str) -> String {
    let parsed = parse(src);
    let printed = print_expression(&parsed);
    let reparsed = parse(&printed);
    assert_eq!(parsed, reparsed, "round trip changed the AST for {src:?} via {printed:?}");
    printed
}

fn infer(src: &str) -> Result<InferredType, TypeError> {
    infer_type(&parse(src))
}

fn data(data_type: DataType) -> InferredType {
    InferredType::Data(data_type)
}

fn arithmetic(op: BinaryOp, left: DataType, right: DataType) -> Result<InferredType, TypeError> {
    arithmetic_type(op, data(left), data(right))
}

#[test]
fn eight_scalar_types_and_metadata() {
    assert_eq!(DataType::ALL.map(DataType::name), ["Binary", "Boolean", "Currency", "DateTime", "Decimal", "Integer", "String", "Variant"]);

    for data_type in DataType::ALL {
        let metadata = data_type.metadata();
        assert_eq!(metadata.name, data_type.name());
        assert!(!metadata.representation.is_empty(), "{} representation", data_type.name());
        assert!(!metadata.range.is_empty(), "{} range", data_type.name());
        assert!(!metadata.precision.is_empty(), "{} precision", data_type.name());
        assert!(!metadata.notes.is_empty(), "{} notes", data_type.name());
        assert_eq!(data_type.has_literal(), metadata.literal_syntax.is_some());
        assert!(!metadata.name.is_empty());
    }

    assert_eq!(DataType::ALL.map(DataType::has_literal), [false, true, false, true, true, true, true, false], "literal availability per type");
    assert!(DataType::Binary.metadata().literal_syntax.is_none());
    assert!(DataType::Boolean.metadata().literal_syntax.is_some());
    assert!(DataType::Currency.metadata().literal_syntax.is_none());
    assert!(DataType::DateTime.metadata().literal_syntax.is_some());
    assert!(DataType::Decimal.metadata().literal_syntax.is_some());
    assert!(DataType::Integer.metadata().literal_syntax.is_some());
    assert!(DataType::String.metadata().literal_syntax.is_some());
    assert!(DataType::Variant.metadata().literal_syntax.is_none());

    assert!(DataType::Integer.is_numeric());
    assert!(DataType::Currency.is_numeric());
    assert!(DataType::Decimal.is_numeric());
    assert!(!DataType::Boolean.is_numeric());
    assert!(!DataType::DateTime.is_numeric());
}

#[test]
fn blank_and_table_are_pseudo_types_distinct_from_the_eight() {
    assert_eq!(PseudoType::ALL.map(PseudoType::name), ["BLANK", "Table"]);
    assert!(DataType::ALL.iter().all(|data_type| data_type.name() != "BLANK"));
    assert!(DataType::ALL.iter().all(|data_type| data_type.name() != "Table"));

    let blank = PseudoType::Blank.metadata();
    assert_eq!(blank.name, "BLANK");
    assert!(blank.literal_syntax.is_none(), "BLANK has no literal");
    assert!(blank.notes.contains("BLANK()"), "BLANK is a function: {}", blank.notes);

    let table = PseudoType::Table.metadata();
    assert_eq!(table.name, "Table");
    assert!(table.literal_syntax.is_some(), "table constructors exist");
    assert!(!table.representation.is_empty());

    assert_eq!(InferredType::from(PseudoType::Blank), InferredType::Blank);
    assert_eq!(InferredType::from(PseudoType::Table), InferredType::Table);
    assert_eq!(InferredType::Blank.name(), "BLANK");
    assert_eq!(InferredType::Table.name(), "Table");
    assert_eq!(InferredType::Unknown.name(), "unknown");
}

#[test]
fn literal_classification() {
    for (src, data_type) in [
        ("42", DataType::Integer),
        ("-42", DataType::Integer),
        ("3.14", DataType::Decimal),
        ("1e3", DataType::Decimal),
        ("dt\"2023-05-19\"", DataType::DateTime),
        ("\"text\"", DataType::String),
        ("TRUE", DataType::Boolean),
    ] {
        // `-42` is a unary expression, so inspect its operand before classifying.
        let expr = parse(src);
        let node = match &expr.kind {
            ExprKind::Unary { expr, .. } => expr.as_ref(),
            _ => &expr,
        };
        match &node.kind {
            ExprKind::Literal(value) => assert_eq!(literal_type(value), data_type, "{src}"),
            other => panic!("{src:?} did not contain a literal: {other:?}"),
        }
    }
}

#[test]
#[allow(clippy::approx_constant)] // 3.14 is the documented Decimal example, not PI.
fn integer_and_decimal_literals_are_distinct() {
    assert_eq!(literal("0"), Literal::Integer(0));
    assert_eq!(literal("42"), Literal::Integer(42));
    assert_eq!(literal("007"), Literal::Integer(7));
    assert_eq!(literal("9223372036854775807"), Literal::Integer(i64::MAX));

    for (src, value, canonical) in [
        ("3.14", 3.14, "3.14"),
        ("1e3", 1000.0, "1000.0"),
        ("1E3", 1000.0, "1000.0"),
        ("1.5E-3", 0.0015, "0.0015"),
        (".5", 0.5, "0.5"),
        ("1.", 1.0, "1.0"),
        ("1.e3", 1000.0, "1000.0"),
        ("0.000001", 0.000001, "1e-6"),
    ] {
        assert_eq!(literal(src), Literal::Decimal(value), "{src}");
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    // The sign is always the unary operator, never part of the numeric token.
    match parse("-42").kind {
        ExprKind::Unary { op: UnaryOp::Neg, expr } => {
            assert_eq!(expr.kind, ExprKind::Literal(Literal::Integer(42)));
        }
        other => panic!("expected unary minus, found {other:?}"),
    }
    match parse("+42").kind {
        ExprKind::Unary { op: UnaryOp::Plus, expr } => {
            assert_eq!(expr.kind, ExprKind::Literal(Literal::Integer(42)));
        }
        other => panic!("expected unary plus, found {other:?}"),
    }
}

#[test]
fn numeric_literal_overflow_is_a_structured_error() {
    for (src, needle) in [
        ("9223372036854775808", "integer literal is out of range"),
        ("99999999999999999999999999999999999999", "integer literal is out of range"),
        ("1e400", "decimal literal is out of range"),
    ] {
        let error = match parse_expression(src) {
            Ok(expr) => panic!("{src:?} unexpectedly parsed as {expr:?}"),
            Err(error) => error,
        };
        assert!(error.message.contains(needle), "{}: {}", src, error.message);
        assert_eq!(error.span, Span::new(0, src.len()), "{src}");
        assert!(error.to_string().contains(&error.span.start.to_string()));
    }

    // Limitation: the sign is a unary operator, so the unsigned magnitude of i64::MIN cannot be
    // written as an Integer literal. The parser reports the overflow instead of wrapping.
    assert!(parse_expression("-9223372036854775808").is_err());
}

#[test]
fn lexer_emits_typed_literal_tokens() {
    let tokens = match tokenize("1 2.5 dt\"2023-01-01\" \"x\" TRUE") {
        Ok(tokens) => tokens,
        Err(error) => panic!("tokenization failed: {error}"),
    };
    assert_eq!(tokens.first().map(|token| &token.kind), Some(&TokenKind::Integer(1)));
    assert_eq!(tokens.get(1).map(|token| &token.kind), Some(&TokenKind::Decimal(2.5)));
    assert!(matches!(
        tokens.get(2).map(|token| &token.kind),
        Some(TokenKind::DateTime(DateTimeLiteral { year: 2023, month: 1, day: 1, has_time: false, .. }))
    ));
    assert_eq!(tokens.get(3).map(|token| &token.kind), Some(&TokenKind::String("x".into())));
    assert_eq!(tokens.get(4).map(|token| &token.kind), Some(&TokenKind::Ident("TRUE".into())));
}

#[test]
fn datetime_literal_forms_are_accepted_and_canonicalised() {
    for (src, canonical) in [
        ("dt\"2023-05-19\"", "dt\"2023-05-19\""),
        ("dt\"2023-05-19T14:30:00\"", "dt\"2023-05-19T14:30:00\""),
        ("dt\"2023-05-19 14:30:00\"", "dt\"2023-05-19T14:30:00\""),
        ("dt\"2015-1-9T2:3:0\"", "dt\"2015-01-09T02:03:00\""),
        ("dt\"2023-5-9\"", "dt\"2023-05-09\""),
        ("dt\"2024-2-29\"", "dt\"2024-02-29\""),
        ("DT\"2023-5-19\"", "dt\"2023-05-19\""),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    match literal("dt\"2015-1-9T2:3:0\"") {
        Literal::DateTime(value) => {
            assert_eq!(value, DateTimeLiteral { year: 2015, month: 1, day: 9, hour: 2, minute: 3, second: 0, has_time: true })
        }
        other => panic!("expected a DateTime literal, found {other:?}"),
    }
}

#[test]
fn datetime_literal_rejects_invalid_forms() {
    for src in [
        // Timezone offsets and fractional seconds are not part of the DAX literal forms.
        "dt\"2023-05-19T14:30:00Z\"",
        "dt\"2023-05-19T14:30:00+02:00\"",
        "dt\"2023-05-19T14:30:00.123\"",
        // Time-only and incomplete values.
        "dt\"14:30:00\"",
        "dt\"2023-05-19T14:30\"",
        "dt\"2023-05\"",
        "dt\"20230519\"",
        "dt\"23-05-19\"",
        "dt\"2023-05-19T\"",
        // Invalid calendar fields.
        "dt\"2023-02-29\"",
        "dt\"1900-02-29\"",
        "dt\"2023-13-01\"",
        "dt\"2023-00-01\"",
        "dt\"2023-04-31\"",
        "dt\"2023-01-00\"",
        "dt\"2023-01-32\"",
        "dt\"0000-01-01\"",
        "dt\"2023-05-19T24:00:00\"",
        "dt\"2023-05-19T14:60:00\"",
        "dt\"2023-05-19T14:30:60\"",
        // Wrong case separator; whitespace between prefix and quote.
        "dt\"2023-05-19t14:30:00\"",
        "dt \"2023-05-19\"",
    ] {
        assert!(parse_expression(src).is_err(), "expected an error for {src:?}");
    }

    // `dt` alone is an ordinary identifier, not a literal.
    assert!(matches!(parse("dt").kind, ExprKind::Name(_)));
    let error = match parse_expression("dt\"2023-02-29\"") {
        Ok(expr) => panic!("invalid calendar date parsed as {expr:?}"),
        Err(error) => error,
    };
    assert!(error.message.contains("date/time literal"), "{}", error.message);
    assert_eq!(error.span, Span::new(0, "dt\"2023-02-29\"".len()));
}

#[test]
fn literal_printing_round_trips() {
    for src in [
        "0",
        "42",
        "3.14",
        "1e300",
        "1.2345678901234568e18",
        "dt\"2023-05-19\"",
        "dt\"2023-05-19T14:30:00\"",
        "\"a\"\"b\"",
        "\"\"",
        "TRUE",
        "FALSE",
        "TRUE()",
        "BLANK()",
        "1 + 2.5",
        "(1 + 2) * 3.0",
        "dt\"2023-05-19\" + 1",
    ] {
        let printed = round_trip(src);
        if let ExprKind::Literal(Literal::Decimal(value)) = parse(&printed).kind {
            assert!(value.is_finite(), "{src} printed a non-finite Decimal");
            assert!(printed.contains('.') || printed.contains('e') || printed.contains('E'), "Decimal text must re-lex as a Decimal: {printed}");
        }
    }
}

#[test]
fn boolean_literals_are_distinct_from_boolean_calls() {
    assert_eq!(literal("TRUE"), Literal::Boolean(true));
    assert_eq!(literal("FALSE"), Literal::Boolean(false));

    for (src, spelling) in [("TRUE()", "TRUE"), ("true()", "true"), ("FALSE()", "FALSE")] {
        match parse(src).kind {
            ExprKind::Call(call) => {
                assert!(call.args.is_empty());
                assert_eq!(call.name.text, spelling);
                assert_eq!(infer(src), Ok(data(DataType::Boolean)), "{src}");
            }
            other => panic!("{src:?} is not a call: {other:?}"),
        }
    }

    // `TRUE(1)` is still a call and is not a literal; its type is outside this crate's catalog.
    assert!(matches!(parse("TRUE(1)").kind, ExprKind::Call(_)));
    assert_eq!(infer("TRUE(1)"), Ok(InferredType::Unknown));
    assert_eq!(infer("FALSE(1)"), Ok(InferredType::Unknown));
}

#[test]
fn blank_is_a_call_not_a_literal() {
    match parse("BLANK()").kind {
        ExprKind::Call(call) => {
            assert_eq!(call.name.text, "BLANK");
            assert!(call.args.is_empty());
        }
        other => panic!("BLANK() is a call: {other:?}"),
    }
    assert_eq!(infer("BLANK()"), Ok(InferredType::Blank));
    assert!(matches!(parse("BLANK").kind, ExprKind::Name(_)));
    assert_eq!(infer("BLANK"), Ok(InferredType::Unknown));
}

#[test]
fn literal_operator_inference() {
    for (src, expected) in [
        ("1 + 2", data(DataType::Integer)),
        ("1 - 2", data(DataType::Integer)),
        ("1 * 2", data(DataType::Integer)),
        ("1 / 2", data(DataType::Decimal)),
        ("1.0 + 2", data(DataType::Decimal)),
        ("1 + 2.0", data(DataType::Decimal)),
        ("2 ^ 3", data(DataType::Decimal)),
        ("1 & 2", data(DataType::String)),
        // Excel: 12&34 is "1234"; the observed result is evaluator output, the type is String.
        ("12 & 34", data(DataType::String)),
        ("1 = 2", data(DataType::Boolean)),
        // Excel: "Report" = "REPORT" is TRUE (case-insensitive collation is evaluator work).
        ("\"Report\" = \"REPORT\"", data(DataType::Boolean)),
        ("1 <> 2", data(DataType::Boolean)),
        ("1 < 2", data(DataType::Boolean)),
        ("1 > 2", data(DataType::Boolean)),
        ("1 <= 2", data(DataType::Boolean)),
        ("1 >= 2", data(DataType::Boolean)),
        ("1 == 1", data(DataType::Boolean)),
        ("TRUE && FALSE", data(DataType::Boolean)),
        ("TRUE || FALSE", data(DataType::Boolean)),
        ("NOT TRUE", data(DataType::Boolean)),
        ("1 IN { 1 }", data(DataType::Boolean)),
        ("(1, \"a\") IN { (1, \"a\") }", data(DataType::Boolean)),
        ("TRUE() + 1", data(DataType::Integer)),
        ("-1", data(DataType::Integer)),
        ("+1", data(DataType::Integer)),
        ("-1.5", data(DataType::Decimal)),
        ("-TRUE()", data(DataType::Integer)),
        ("dt\"2023-05-19\" + 1", data(DataType::DateTime)),
        ("1 + dt\"2023-05-19\"", data(DataType::DateTime)),
        ("dt\"2023-05-19\" - 1", data(DataType::DateTime)),
        ("dt\"2023-05-19\" - dt\"2023-05-20\"", data(DataType::DateTime)),
    ] {
        assert_eq!(infer(src), Ok(expected), "{src}");
    }

    for src in ["[Amount]", "SUM(1, 2)", "VALUES(T[Col])", "\"22\" + 22", "[Amount] + 1", "[Amount] - 1"] {
        assert_eq!(infer(src), Ok(InferredType::Unknown), "{src}");
    }
}

#[test]
fn blank_arithmetic_keeps_blank_polymorphic() {
    for (src, expected) in [
        ("BLANK() + 5", data(DataType::Integer)),
        ("5 + BLANK()", data(DataType::Integer)),
        ("BLANK() + 2.5", data(DataType::Decimal)),
        ("BLANK() + TRUE()", data(DataType::Integer)),
        ("BLANK() + dt\"2023-05-19\"", data(DataType::DateTime)),
        ("BLANK() + BLANK()", InferredType::Blank),
        ("BLANK() - BLANK()", InferredType::Blank),
        ("BLANK() * 5", InferredType::Blank),
        ("5 * BLANK()", InferredType::Blank),
        ("BLANK() * BLANK()", InferredType::Blank),
        ("5 / BLANK()", data(DataType::Decimal)),
        ("0 / BLANK()", data(DataType::Decimal)),
        ("BLANK() / 5", InferredType::Blank),
        ("BLANK() / BLANK()", InferredType::Blank),
        ("-BLANK()", InferredType::Blank),
    ] {
        assert_eq!(infer(src), Ok(expected), "{src}");
    }

    assert_eq!(infer("BLANK() + [Amount]"), Ok(InferredType::Unknown));
    assert_eq!(infer("BLANK() * [Amount]"), Ok(InferredType::Blank));
}

#[test]
fn incompatible_comparisons_and_coercions() {
    for src in ["TRUE() = 1", "1 = TRUE()", "\"1\" = 1", "1 = \"1\"", "\"1\" > 0", "TRUE() == 1", "\"1\" == 1"] {
        let error = match infer(src) {
            Ok(inferred) => panic!("{src:?} unexpectedly inferred {inferred:?}"),
            Err(error) => error,
        };
        assert!(matches!(error, TypeError::IncompatibleOperands { .. }), "{src}: {error:?}");
        assert!(error.to_string().contains("cannot combine"), "{error}");
    }

    // BLANK is coerced to 0, "" or FALSE for `=`, and comparisons still yield Boolean.
    for src in ["BLANK() = 0", "BLANK() <> 0", "BLANK() = \"\"", "BLANK() = FALSE()", "BLANK() = BLANK()"] {
        assert_eq!(infer(src), Ok(data(DataType::Boolean)), "{src}");
    }

    // `==` is strict and does not coerce BLANK, but the comparison is still well-typed.
    for src in ["BLANK() == 0", "BLANK() == \"\"", "0 == BLANK()"] {
        assert_eq!(infer(src), Ok(data(DataType::Boolean)), "{src}");
    }

    assert_eq!(comparison_coercion(InferredType::Blank, data(DataType::Integer)), Ok(data(DataType::Integer)));
    assert_eq!(comparison_coercion(InferredType::Blank, data(DataType::String)), Ok(data(DataType::String)));
    assert_eq!(comparison_coercion(data(DataType::Boolean), InferredType::Blank), Ok(data(DataType::Boolean)));
    assert_eq!(comparison_coercion(InferredType::Blank, InferredType::Blank), Ok(InferredType::Blank));
    assert_eq!(comparison_coercion(data(DataType::Integer), data(DataType::Currency)), Ok(data(DataType::Currency)));
    assert_eq!(
        comparison_coercion(data(DataType::String), data(DataType::Integer)),
        Err(TypeError::IncompatibleTypes { left: data(DataType::String), right: data(DataType::Integer) })
    );
    assert_eq!(
        comparison_coercion(data(DataType::Boolean), data(DataType::Integer)),
        Err(TypeError::IncompatibleTypes { left: data(DataType::Boolean), right: data(DataType::Integer) })
    );
    assert_eq!(
        comparison_coercion(InferredType::Table, InferredType::Blank),
        Err(TypeError::IncompatibleTypes { left: InferredType::Table, right: InferredType::Blank })
    );

    assert_eq!(comparison_compatible(BinaryOp::StrictEq, InferredType::Blank, data(DataType::Integer)), Ok(()));
    assert_eq!(
        comparison_compatible(BinaryOp::Eq, data(DataType::String), data(DataType::Integer)),
        Err(TypeError::IncompatibleOperands { op: BinaryOp::Eq, left: data(DataType::String), right: data(DataType::Integer) })
    );
    assert_eq!(
        comparison_compatible(BinaryOp::Eq, InferredType::Table, data(DataType::Integer)),
        Err(TypeError::IncompatibleOperands { op: BinaryOp::Eq, left: InferredType::Table, right: data(DataType::Integer) })
    );
    assert_eq!(comparison_type(BinaryOp::Eq, data(DataType::Integer), data(DataType::Decimal)), Ok(DataType::Boolean));
}

#[test]
fn documented_arithmetic_result_matrices() {
    use DataType::{Currency, DateTime, Decimal, Integer};

    assert_eq!(numeric_matrix_order(), [Integer, Currency, Decimal, DateTime], "matrix order is Integer, Currency, Real, DateTime");

    for (op, left, right, expected) in [
        // Addition: DateTime wins; Currency promotes against Integer and Decimal.
        (BinaryOp::Add, Integer, Integer, Integer),
        (BinaryOp::Add, Integer, Currency, Currency),
        (BinaryOp::Add, Currency, Decimal, Decimal),
        (BinaryOp::Add, Integer, DateTime, DateTime),
        (BinaryOp::Add, DateTime, Integer, DateTime),
        (BinaryOp::Add, DateTime, DateTime, DateTime),
        // Subtraction: two dates are documented as DateTime.
        (BinaryOp::Sub, Integer, Integer, Integer),
        (BinaryOp::Sub, Integer, Currency, Currency),
        (BinaryOp::Sub, Integer, DateTime, Decimal),
        (BinaryOp::Sub, DateTime, Integer, DateTime),
        (BinaryOp::Sub, DateTime, DateTime, DateTime),
        // Multiplication reproduces the quirky published cells, including INT*DT -> INT.
        (BinaryOp::Mul, Integer, Integer, Integer),
        (BinaryOp::Mul, Integer, Currency, Currency),
        (BinaryOp::Mul, Currency, Currency, Decimal),
        (BinaryOp::Mul, Integer, DateTime, Integer),
        (BinaryOp::Mul, Currency, DateTime, Currency),
        (BinaryOp::Mul, Decimal, DateTime, Decimal),
        // Division is Decimal for Integer/Integer and keeps the published Currency cells.
        (BinaryOp::Div, Integer, Integer, Decimal),
        (BinaryOp::Div, Integer, Currency, Currency),
        (BinaryOp::Div, Currency, Currency, Decimal),
        (BinaryOp::Div, Currency, Integer, Currency),
        (BinaryOp::Div, DateTime, DateTime, Decimal),
        (BinaryOp::Div, DateTime, Currency, Decimal),
    ] {
        assert_eq!(arithmetic(op, left, right), Ok(data(expected)), "{op:?} {left:?} {right:?}");
    }

    // The multiplication table has no DateTime row; DateTime on the left is a type error.
    assert!(arithmetic(BinaryOp::Mul, DateTime, Integer).is_err());
    assert!(arithmetic(BinaryOp::Mul, DateTime, DateTime).is_err());
}

#[test]
fn documented_comparison_matrix_and_date_comparisons() {
    use DataType::{Currency, DateTime, Decimal, Integer};

    for (left, right, expected) in [
        (Integer, Integer, Integer),
        (Integer, Currency, Currency),
        (Currency, Currency, Currency),
        (Integer, Decimal, Decimal),
        (Integer, DateTime, Decimal),
        (Currency, DateTime, Decimal),
        (DateTime, DateTime, DateTime),
    ] {
        assert_eq!(comparison_coercion(data(left), data(right)), Ok(data(expected)), "{left:?} vs {right:?}");
    }

    for op in [BinaryOp::Eq, BinaryOp::Ne, BinaryOp::Lt, BinaryOp::Gt, BinaryOp::Le, BinaryOp::Ge, BinaryOp::StrictEq] {
        assert_eq!(comparison_type(op, data(DateTime), data(DateTime)), Ok(DataType::Boolean), "date vs date with {op:?}");
        assert_eq!(comparison_type(op, data(DateTime), data(Integer)), Ok(DataType::Boolean), "date vs number with {op:?}");
    }

    assert_eq!(infer("dt\"2023-05-19\" = dt\"2023-05-20\""), Ok(data(DataType::Boolean)));
    assert_eq!(infer("dt\"2023-05-19\" < 1"), Ok(data(DataType::Boolean)));
}

#[test]
fn variant_and_binary_have_no_literals_or_operators() {
    assert!(DataType::Variant.metadata().literal_syntax.is_none());
    assert!(DataType::Binary.metadata().literal_syntax.is_none());
    assert!(matches!(parse("VARIANT").kind, ExprKind::Name(_)));
    // Container children still participate in inference.
    assert!(infer("{ 1 + { 2 } }").is_err());
    assert!(infer("(1 + { 2 })").is_err());
    assert!(infer("SUM(1 + { 2 })").is_err());
    assert!(matches!(parse("BINARY").kind, ExprKind::Name(_)));
    assert_eq!(infer("VARIANT"), Ok(InferredType::Unknown));
    assert_eq!(infer("BINARY"), Ok(InferredType::Unknown));
}

#[test]
fn unary_and_binary_type_helpers_reject_non_scalars() {
    assert_eq!(unary_type(UnaryOp::Neg, data(DataType::Decimal)), Ok(data(DataType::Decimal)));
    assert_eq!(unary_type(UnaryOp::Neg, data(DataType::DateTime)), Ok(data(DataType::DateTime)));
    assert_eq!(unary_type(UnaryOp::Plus, data(DataType::Currency)), Ok(data(DataType::Currency)));
    assert!(unary_type(UnaryOp::Neg, InferredType::Table).is_err());
    assert!(unary_type(UnaryOp::Neg, data(DataType::Binary)).is_err());
    assert_eq!(
        unary_type(UnaryOp::Not, InferredType::Table),
        Err(TypeError::IncompatibleUnaryOperand { op: UnaryOp::Not, operand: InferredType::Table })
    );

    assert_eq!(binary_type(BinaryOp::Concat, data(DataType::Integer), data(DataType::Boolean)), Ok(data(DataType::String)));
    assert_eq!(binary_type(BinaryOp::And, InferredType::Blank, data(DataType::Boolean)), Ok(data(DataType::Boolean)));
    assert!(binary_type(BinaryOp::Concat, InferredType::Table, data(DataType::Integer)).is_err());
    assert!(binary_type(BinaryOp::In, data(DataType::Integer), data(DataType::Integer)).is_err());
    assert_eq!(binary_type(BinaryOp::In, data(DataType::Integer), InferredType::Table), Ok(data(DataType::Boolean)));
    assert_eq!(binary_type(BinaryOp::Eq, data(DataType::Boolean), data(DataType::Boolean)), Ok(data(DataType::Boolean)));
}
