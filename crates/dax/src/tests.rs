use crate::ast::*;
use crate::lexer::TokenKind;
use crate::{MAX_DEPTH, MAX_INPUT_LEN, parse_expression, parse_query, print_expression, print_query, tokenize};

fn expression(src: &str) -> Expr {
    match parse_expression(src) {
        Ok(expr) => expr,
        Err(error) => panic!("{src:?} did not parse: {error}"),
    }
}

/// Parses, prints, re-parses and asserts the AST is unchanged. Returns the canonical text.
fn round_trip(src: &str) -> String {
    let parsed = expression(src);
    let printed = print_expression(&parsed);
    let reparsed = match parse_expression(&printed) {
        Ok(expr) => expr,
        Err(error) => panic!("canonical text {printed:?} did not parse: {error}"),
    };
    assert_eq!(parsed, reparsed, "round trip changed the AST for {src:?} via {printed:?}");
    printed
}

fn binary(expr: &Expr) -> (&BinaryOp, &Expr, &Expr) {
    match &expr.kind {
        ExprKind::Binary { op, left, right } => (op, left, right),
        other => panic!("expected a binary expression, found {other:?}"),
    }
}

#[test]
fn every_operator_spelling_and_canonical_text() {
    for (src, canonical) in [
        ("1 + 2", "1 + 2"),
        ("1 - 2", "1 - 2"),
        ("1 * 2", "1 * 2"),
        ("1 / 2", "1 / 2"),
        ("1 ^ 2", "1 ^ 2"),
        ("\"a\" & \"b\"", "\"a\" & \"b\""),
        ("1 = 2", "1 = 2"),
        ("1 == 2", "1 == 2"),
        ("1 <> 2", "1 <> 2"),
        ("1 < 2", "1 < 2"),
        ("1 > 2", "1 > 2"),
        ("1 <= 2", "1 <= 2"),
        ("1 >= 2", "1 >= 2"),
        ("TRUE && FALSE", "TRUE && FALSE"),
        ("TRUE || FALSE", "TRUE || FALSE"),
        ("NOT TRUE", "NOT TRUE"),
        ("1 IN { 1 }", "1 IN { 1 }"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }
}

#[test]
fn arithmetic_precedence_parentheses_and_associativity() {
    for (src, canonical) in [
        ("1 + 2 * 3", "1 + 2 * 3"),
        ("1 * 2 + 3", "1 * 2 + 3"),
        ("1 + 2 / 3", "1 + 2 / 3"),
        ("1 - 2 * 3", "1 - 2 * 3"),
        ("1 / 2 - 3", "1 / 2 - 3"),
        ("1 + 2 - 3", "1 + 2 - 3"),
        ("1 - 2 + 3", "1 - 2 + 3"),
        ("1 * 2 / 3", "1 * 2 / 3"),
        ("1 / 2 * 3", "1 / 2 * 3"),
        ("1 + 2 + 3", "1 + 2 + 3"),
        ("1 - 2 - 3", "1 - 2 - 3"),
        ("1 * 2 * 3", "1 * 2 * 3"),
        ("1 / 2 / 3", "1 / 2 / 3"),
        ("(1 + 2) * 3", "(1 + 2) * 3"),
        ("1 * (2 + 3)", "1 * (2 + 3)"),
        ("1 - (2 - 3)", "1 - (2 - 3)"),
        ("((1))", "1"),
        ("(1 + 2)", "1 + 2"),
        ("2 ^ 3 * 4", "2 ^ 3 * 4"),
        ("2 * 3 ^ 4", "2 * 3 ^ 4"),
        ("2 ^ 3 + 4", "2 ^ 3 + 4"),
        ("2 ^ 3 ^ 2", "2 ^ 3 ^ 2"),
        ("2 ^ (3 ^ 2)", "2 ^ 3 ^ 2"),
        ("(2 ^ 3) ^ 2", "(2 ^ 3) ^ 2"),
        ("-2 ^ 2", "-2 ^ 2"),
        ("(-2) ^ 2", "(-2) ^ 2"),
        ("2 ^ -3", "2 ^ (-3)"),
        ("2 ^ (-3)", "2 ^ (-3)"),
        ("-(2 * 3)", "-(2 * 3)"),
        ("-2 * 3", "-2 * 3"),
        ("+1 - 2", "+1 - 2"),
        ("1 + -2", "1 + -2"),
        ("1 - -2", "1 - -2"),
        ("- -2", "- -2"),
        ("- +2", "- +2"),
        ("+ -2", "+ -2"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    let expr = expression("-2 ^ 2");
    assert!(matches!(&expr.kind, ExprKind::Unary { op: UnaryOp::Neg, expr: inner }
            if matches!(&inner.kind, ExprKind::Binary { op: BinaryOp::Pow, .. })));

    let expr = expression("(-2) ^ 2");
    let (op, left, _) = binary(&expr);
    assert_eq!(*op, BinaryOp::Pow);
    assert!(matches!(&left.kind, ExprKind::Unary { op: UnaryOp::Neg, .. }));
}

#[test]
fn binary_operators_associate_left_except_exponentiation() {
    let expr = expression("1 - 2 - 3");
    let (op, left, right) = binary(&expr);
    assert_eq!(*op, BinaryOp::Sub);
    assert!(matches!(&left.kind, ExprKind::Binary { op: BinaryOp::Sub, .. }));
    assert!(matches!(&right.kind, ExprKind::Literal(Literal::Number(n)) if *n == 3.0));

    let expr = expression("2 ^ 3 ^ 2");
    let (op, left, right) = binary(&expr);
    assert_eq!(*op, BinaryOp::Pow);
    assert!(matches!(&left.kind, ExprKind::Literal(Literal::Number(n)) if *n == 2.0));
    assert!(matches!(&right.kind, ExprKind::Binary { op: BinaryOp::Pow, .. }));

    let expr = expression("1 / 2 / 3");
    let (op, left, _) = binary(&expr);
    assert_eq!(*op, BinaryOp::Div);
    assert!(matches!(&left.kind, ExprKind::Binary { op: BinaryOp::Div, .. }));

    let expr = expression("1 - (2 - 3)");
    let (op, _, right) = binary(&expr);
    assert_eq!(*op, BinaryOp::Sub);
    assert!(matches!(&right.kind, ExprKind::Binary { op: BinaryOp::Sub, .. }));
}

#[test]
fn comparison_and_concatenation_precedence() {
    for (src, canonical) in [
        ("1 = 2", "1 = 2"),
        ("1 == 2", "1 == 2"),
        ("1 <> 2", "1 <> 2"),
        ("1 < 2", "1 < 2"),
        ("1 > 2", "1 > 2"),
        ("1 <= 2", "1 <= 2"),
        ("1 >= 2", "1 >= 2"),
        ("1 = 2 == 3", "1 = 2 == 3"),
        ("\"a\" & \"b\" = \"ab\"", "\"a\" & \"b\" = \"ab\""),
        ("\"a\" & (\"b\" = \"c\")", "\"a\" & (\"b\" = \"c\")"),
        ("\"a\" & \"b\" & \"c\"", "\"a\" & \"b\" & \"c\""),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    let expr = expression("1 = 2");
    assert_eq!(*binary(&expr).0, BinaryOp::Eq);
    let expr = expression("1 == 2");
    assert_eq!(*binary(&expr).0, BinaryOp::StrictEq);
    let expr = expression("1 = 2 == 3");
    let (op, left, _) = binary(&expr);
    assert_eq!(*op, BinaryOp::StrictEq);
    assert!(matches!(&left.kind, ExprKind::Binary { op: BinaryOp::Eq, .. }));

    let expr = expression("\"a\" & \"b\" = \"ab\"");
    let (op, left, _) = binary(&expr);
    assert_eq!(*op, BinaryOp::Eq);
    assert!(matches!(&left.kind, ExprKind::Binary { op: BinaryOp::Concat, .. }));
}

#[test]
fn logical_operators_and_not_precedence() {
    for (src, canonical) in [
        ("NOT TRUE", "NOT TRUE"),
        ("not false", "NOT FALSE"),
        ("TRUE && FALSE", "TRUE && FALSE"),
        ("TRUE || FALSE", "TRUE || FALSE"),
        ("NOT TRUE || FALSE && TRUE", "NOT TRUE || FALSE && TRUE"),
        ("TRUE && FALSE || TRUE", "TRUE && FALSE || TRUE"),
        ("TRUE || FALSE && TRUE", "TRUE || FALSE && TRUE"),
        ("NOT TRUE = FALSE", "NOT TRUE = FALSE"),
        ("NOT (TRUE = FALSE)", "NOT TRUE = FALSE"),
        ("NOT TRUE && FALSE", "NOT TRUE && FALSE"),
        ("NOT (TRUE && FALSE)", "NOT (TRUE && FALSE)"),
        ("NOT NOT TRUE", "NOT NOT TRUE"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    let expr = expression("TRUE || FALSE && TRUE");
    let (op, left, right) = binary(&expr);
    assert_eq!(*op, BinaryOp::Or);
    assert!(matches!(&left.kind, ExprKind::Literal(Literal::Boolean(true))));
    assert!(matches!(&right.kind, ExprKind::Binary { op: BinaryOp::And, .. }));

    let expr = expression("NOT TRUE && FALSE");
    let (op, left, _) = binary(&expr);
    assert_eq!(*op, BinaryOp::And);
    assert!(matches!(&left.kind, ExprKind::Unary { op: UnaryOp::Not, .. }));

    let expr = expression("NOT TRUE = FALSE");
    assert!(matches!(&expr.kind, ExprKind::Unary { op: UnaryOp::Not, expr: inner }
        if matches!(&inner.kind, ExprKind::Binary { op: BinaryOp::Eq, .. })));

    let expr = expression("NOT (TRUE && FALSE)");
    assert!(matches!(&expr.kind, ExprKind::Unary { op: UnaryOp::Not, expr: inner }
        if matches!(&inner.kind, ExprKind::Binary { op: BinaryOp::And, .. })));
}

#[test]
fn in_operator_with_lists_and_table_expressions() {
    for (src, canonical) in [
        ("1 IN { 1 }", "1 IN { 1 }"),
        ("1 IN {1, 2, 3}", "1 IN { 1, 2, 3 }"),
        ("1 IN {}", "1 IN {}"),
        ("'Sales'[Region] IN { \"East\", \"West\" }", "'Sales'[Region] IN { \"East\", \"West\" }"),
        ("1 IN VALUES(T[Col])", "1 IN VALUES(T[Col])"),
        ("1 IN (FILTER(T, T[X] > 0))", "1 IN FILTER(T, T[X] > 0)"),
        ("1 IN { 1 } = TRUE", "1 IN { 1 } = TRUE"),
        ("2 ^ 3 IN { 8 }", "2 ^ 3 IN { 8 }"),
        ("1 + 2 IN { 3 }", "1 + 2 IN { 3 }"),
        ("-1 IN { 1 }", "-1 IN { 1 }"),
        ("NOT 1 IN { 1 }", "NOT 1 IN { 1 }"),
        ("1 IN { 1 } IN { TRUE }", "1 IN { 1 } IN { TRUE }"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    let expr = expression("1 IN { 1, 2 }");
    let (_, _, right) = binary(&expr);
    assert!(matches!(&right.kind, ExprKind::Table(items) if items.len() == 2));

    let expr = expression("1 IN VALUES(T[Col])");
    let (_, _, right) = binary(&expr);
    assert!(matches!(&right.kind, ExprKind::Call(_)));

    // `IN` binds tighter than `^`, comparisons and arithmetic in DAX's table.
    let expr = expression("2 ^ 3 IN { 8 }");
    let (op, left, right) = binary(&expr);
    assert_eq!(*op, BinaryOp::Pow);
    assert!(matches!(&left.kind, ExprKind::Literal(Literal::Number(n)) if *n == 2.0));
    assert!(matches!(&right.kind, ExprKind::Binary { op: BinaryOp::In, .. }));

    let expr = expression("1 + 2 IN { 3 }");
    let (op, left, right) = binary(&expr);
    assert_eq!(*op, BinaryOp::Add);
    assert!(matches!(&left.kind, ExprKind::Literal(Literal::Number(n)) if *n == 1.0));
    assert!(matches!(&right.kind, ExprKind::Binary { op: BinaryOp::In, .. }));

    let expr = expression("1 IN { 1 } = TRUE");
    let (op, left, _) = binary(&expr);
    assert_eq!(*op, BinaryOp::Eq);
    assert!(matches!(&left.kind, ExprKind::Binary { op: BinaryOp::In, .. }));
}

#[test]
fn quoted_and_bracketed_identifiers_preserve_escapes() {
    for (src, canonical) in [
        ("'My Table'[Sales Amount]", "'My Table'[Sales Amount]"),
        ("'It''s'[A]]B]", "'It''s'[A]]B]"),
        ("Sales[Amount]", "Sales[Amount]"),
        ("[Measure]", "[Measure]"),
        ("'Quoted Table'", "'Quoted Table'"),
        ("SUM('Sales'[Amount])", "SUM('Sales'[Amount])"),
        ("'Sales'[Amount] + 1", "'Sales'[Amount] + 1"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    let expr = expression("'It''s'[A]]B]");
    match expr.kind {
        ExprKind::Name(name) => {
            let table = match &name.table {
                Some(table) => table,
                None => panic!("expected a table qualifier"),
            };
            assert_eq!(table.text, "It's");
            assert_eq!(table.style, NameStyle::Quoted);
            assert_eq!(name.name.text, "A]B");
            assert_eq!(name.name.style, NameStyle::Bracketed);
        }
        other => panic!("expected a name, found {other:?}"),
    }
}

#[test]
fn strings_numbers_booleans_and_calls() {
    for (src, canonical) in [
        ("\"a\"\"b\"", "\"a\"\"b\""),
        ("\"\"", "\"\""),
        ("TRUE", "TRUE"),
        ("true", "TRUE"),
        ("False", "FALSE"),
        ("1", "1"),
        ("1.5", "1.5"),
        (".5", "0.5"),
        ("1.", "1"),
        ("1e3", "1000"),
        ("1.5E-3", "0.0015"),
        ("0.000001", "0.000001"),
        ("0", "0"),
        ("1000", "1000"),
        ("SUM()", "SUM()"),
        ("SUM(1, 2)", "SUM(1, 2)"),
        ("SUM(1, MAX(2, 3))", "SUM(1, MAX(2, 3))"),
        ("Sum(1, 2)", "Sum(1, 2)"),
        ("STDEV.P(1, 2)", "STDEV.P(1, 2)"),
        ("TRUE()", "TRUE()"),
        ("true()", "true()"),
        ("FALSE(1)", "FALSE(1)"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    match expression("\"a\"\"b\"").kind {
        ExprKind::Literal(Literal::String(text)) => assert_eq!(text, "a\"b"),
        other => panic!("expected a string literal, found {other:?}"),
    }
    assert!(matches!(expression("TRUE").kind, ExprKind::Literal(Literal::Boolean(true))));
    assert!(matches!(expression("FALSE").kind, ExprKind::Literal(Literal::Boolean(false))));
}

#[test]
fn table_and_row_constructors() {
    for (src, canonical) in [
        ("{}", "{}"),
        ("{1}", "{ 1 }"),
        ("{1, 2, 3}", "{ 1, 2, 3 }"),
        ("{(1, 2), (3, 4)}", "{ (1, 2), (3, 4) }"),
        ("{ (\"a\", TRUE), (\"b\", FALSE) }", "{ (\"a\", TRUE), (\"b\", FALSE) }"),
        ("(1, 2)", "(1, 2)"),
        ("(1, 2, 3)", "(1, 2, 3)"),
        ("((1, 2))", "(1, 2)"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }

    let expr = expression("{ (1, \"a\"), (2, \"b\") }");
    match expr.kind {
        ExprKind::Table(rows) => {
            assert_eq!(rows.len(), 2);
            assert!(rows.iter().all(|row| matches!(&row.kind, ExprKind::Tuple(items) if items.len() == 2)));
        }
        other => panic!("expected a table, found {other:?}"),
    }
}

#[test]
fn comments_are_skipped() {
    for (src, canonical) in [
        ("1 + /* comment */ 2", "1 + 2"),
        ("1 + // comment\n2", "1 + 2"),
        ("1 + -- comment\n2", "1 + 2"),
        ("/* lead */ 1 /* tail */", "1"),
        ("1--2", "1"),
        ("1 - -2", "1 - -2"),
    ] {
        assert_eq!(round_trip(src), canonical, "{src}");
    }
}

#[test]
fn query_parsing_and_printing() {
    let query = parse_query("EVALUATE SUM('Sales'[Amount])").unwrap();
    assert_eq!(print_query(&query), "EVALUATE SUM('Sales'[Amount])");
    assert_eq!(parse_query(&print_query(&query)).unwrap(), query);

    assert_eq!(print_query(&parse_query("evaluate 1 + 2 * 3").unwrap()), "EVALUATE 1 + 2 * 3");
    assert_eq!(print_query(&parse_query("/* c */ EVALUATE {1, 2}").unwrap()), "EVALUATE { 1, 2 }");

    for bad in ["", "1 + 2", "EVALUATE", "EVALUATE 1 2", "EVALUATE 1 +"] {
        assert!(parse_query(bad).is_err(), "expected a query error for {bad:?}");
    }
}

#[test]
fn leading_equals_is_accepted_for_expressions() {
    assert_eq!(round_trip("= 1 + 2"), "1 + 2");
    assert!(parse_expression("=").is_err());
}

#[test]
fn malformed_input_reports_errors() {
    for bad in [
        "", "   ", "(", "()", "(1,", "(1,)", "{1,", "{1, }", "1 +", "1 2", "1e", "1e+", "1.2.3", "123abc", "\"abc", "'abc", "[abc", "/* abc", "1 IN",
        "NOT", "1 == ", "1 & ", "=>", "1 => 2", "SUM(1,)", "SUM(1", "SUM(,1)", "1 ? 2", "=", "''", "1 | 2",
    ] {
        assert!(parse_expression(bad).is_err(), "expected an error for {bad:?}");
    }
}

#[test]
fn error_spans_point_at_the_problem() {
    let error = parse_expression("1 + )").unwrap_err();
    assert_eq!(error.span, Span::new(4, 5));
    assert!(error.to_string().contains("bytes 4..5"), "{error}");

    let error = parse_expression("\"abc").unwrap_err();
    assert_eq!(error.span, Span::new(0, 4));

    let error = parse_expression("1 + ").unwrap_err();
    assert_eq!(error.span, Span::new(4, 4));
}

#[test]
fn nesting_and_input_length_are_bounded() {
    let deep = format!("{}1{}", "(".repeat(MAX_DEPTH + 4), ")".repeat(MAX_DEPTH + 4));
    let error = parse_expression(&deep).unwrap_err();
    assert!(error.message.contains("nested too deeply"), "{}", error.message);

    let shallow = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(round_trip(&shallow), "1");

    let long = "1".repeat(MAX_INPUT_LEN + 1);
    let error = parse_expression(&long).unwrap_err();
    assert!(error.message.contains("too long"), "{}", error.message);

    let chain = vec!["1"; 1_000].join(" + ");
    assert!(chain.len() < MAX_INPUT_LEN);
    assert!(parse_expression(&chain).is_err(), "a 1,000-term chain exceeds the nesting limit");
    let short = vec!["1"; 200].join(" + ");
    assert_eq!(round_trip(&short), short);
    // A flat list of sibling expressions must not accumulate the depth of their internal
    // binary folds.
    let args = (0..300).map(|i| format!("{i} + 1")).collect::<Vec<_>>().join(", ");
    let call = format!("ROW({args})");
    assert!(call.len() < MAX_INPUT_LEN);
    assert!(parse_expression(&call).is_ok());
    // Parentheses disappear from the AST, so a grouped chain keeps its depth; adding more
    // folds around it must still hit the limit.
    let inner = vec!["1"; 200].join(" + ");
    let outer = format!("({inner}) + {}", vec!["1"; 100].join(" + "));
    assert!(outer.len() < MAX_INPUT_LEN);
    assert!(parse_expression(&outer).is_err(), "a grouped 300-term chain exceeds the nesting limit");
    let ok_inner = vec!["1"; 100].join(" + ");
    let ok_outer = format!("({ok_inner}) + {}", vec!["1"; 100].join(" + "));
    assert!(parse_expression(&ok_outer).is_ok());
}

#[test]
fn spans_are_byte_offsets_and_ignored_by_ast_equality() {
    let expr = expression("1 + 23");
    assert_eq!(expr.span, Span::new(0, 6));
    let (_, left, right) = binary(&expr);
    assert_eq!(left.span, Span::new(0, 1));
    assert_eq!(right.span, Span::new(4, 6));

    assert_eq!(expression("1 + 2"), expression("  1  +  2  "));
}

#[test]
fn lexer_tokens_keywords_and_arrow() {
    let tokens = tokenize("NOT true").unwrap();
    assert_eq!(tokens.first().map(|t| &t.kind), Some(&TokenKind::Not));
    assert_eq!(tokens.get(1).map(|t| &t.kind), Some(&TokenKind::Ident("true".into())));
    assert_eq!(tokens.get(2).map(|t| &t.kind), Some(&TokenKind::Eof));

    let tokens = tokenize("x => y").unwrap();
    assert_eq!(tokens.get(1).map(|t| &t.kind), Some(&TokenKind::Arrow));

    let tokens = tokenize("value in {1}").unwrap();
    assert_eq!(tokens.get(1).map(|t| &t.kind), Some(&TokenKind::In));
}
