use super::*;
use crate::adjust::{Axis, Edit, adjust, rename_sheet, shift_relative};

fn rt(src: &str) -> String {
    print(&parse(src).unwrap_or_else(|e| panic!("{src}: {e}")))
}

#[test]
fn quoted_sheet_qualified_name_keeps_quotes() {
    assert_eq!(rt("'My Sheet'!Total"), "'My Sheet'!Total");
    assert_eq!(rt("'Data'!Total"), "Data!Total");
    assert_eq!(rt("'It''s'!Total+1"), "'It''s'!Total+1");
}

#[test]
fn sheet_names_excel_quotes_stay_quoted() {
    // Excel quotes R1C1-looking names, a leading `.`, cells within the sheet and booleans (#1458).
    for name in ["R", "C", "RC", "R1C", "RC1", "R1C1", "r2c3", "R1", "C1", "A1", "XFD1", "AB1048576", ".x", "TRUE", "false"] {
        assert_eq!(rt(&format!("'{name}'!A1")), format!("'{name}'!A1"), "{name}");
    }
    // Names Excel leaves bare.
    for name in ["XFE1", "AB1048577", "Sheet.1", "_x", "A1A", "\u{3a3}"] {
        assert_eq!(rt(&format!("'{name}'!A1")), format!("{name}!A1"), "{name}");
    }
}

#[test]
fn intersection_with_deleted_operand_reparses() {
    // Deleting the columns of an intersection operand prints `A1:B2 #REF!`; that text must parse back.
    assert_eq!(rt("A1:B2 #REF!"), "A1:B2 #REF!");
    assert_eq!(rt("#REF! A1:B2"), "#REF! A1:B2");
    assert_eq!(parse("A1:B2 #REF!"), parse("A1:B2 (#REF!)").map(|e| strip_paren(&e)));
}

fn strip_paren(e: &Expr) -> Expr {
    match e {
        Expr::Binary(op, a, b) => Expr::Binary(*op, Box::new(strip_paren(a)), Box::new(strip_paren(b))),
        Expr::Paren(x) => strip_paren(x),
        other => other.clone(),
    }
}

#[test]
fn roundtrips() {
    for f in [
        "1+2*3",
        "SUM(A1:B2)",
        "SUM($A$1:B$2,C3)",
        "IF(A1>0,\"yes\",\"no\")",
        "-2^2",
        "A1%",
        "Sheet2!A1+'My Sheet'!$B$3",
        "SUM(Sheet1:Sheet3!A1)",
        "SUM(A:A)",
        "SUM(3:5)",
        "{1,2;3,4}",
        "{-1,\"a\",TRUE,#N/A}",
        "VLOOKUP(A1,Table1[#All],2,FALSE)",
        "Table1[Sales]",
        "Table1[@Sales]",
        "Table1[[#Headers],[A]:[C]]",
        "\"a\"\"b\"&C1",
        "SUM((A1:A3,C1:C3))",
        "A1:C3 B2:B5",
        "LAMBDA(x,x+1)(2)",
        "F(1,,3)",
        "@A1:A10",
        "A1<>B1",
        "A1<=B1",
        "#REF!+1",
        "1E+20",
        "MyName*2",
        "Sheet1!MyName",
        "SUM(A1:B2) ",
    ] {
        let p = rt(f);
        assert_eq!(p, f.trim(), "round trip of {f}");
    }
}

#[test]
fn normalises() {
    assert_eq!(rt("=sum(a1:b2)"), "SUM(A1:B2)");
    assert_eq!(rt("= 1 + 2"), "1+2");
    assert_eq!(rt("_xlfn.XLOOKUP(1,A:A,B:B)"), "XLOOKUP(1,A:A,B:B)");
    assert_eq!(rt("_xlfn.GROUPBY(A1:A3,B1:B3,_xleta.SUM)"), "GROUPBY(A1:A3,B1:B3,SUM)");
    assert_eq!(rt("_XLUDF.concat(\"a\",\"b\")"), "_xludf.CONCAT(\"a\",\"b\")");
    assert_eq!(rt("true"), "TRUE");
    assert_eq!(rt("'Sheet1'!A1"), "Sheet1!A1");
}

#[test]
fn spill_operator() {
    for f in ["A1#", "Sheet1!$D$1#", "SUM(B2#)", "'My Sheet'!A1#*2", "ROWS(A1#)+1", "IF(A1,#N/A,#REF!)"] {
        assert_eq!(rt(f), f);
    }
    assert!(matches!(parse("A1#").unwrap(), Expr::Unary(UnOp::Spill, _)));
    // Files spell it ANCHORARRAY.
    assert_eq!(rt("_xlfn.ANCHORARRAY(Sheet1!$D$1)"), "Sheet1!$D$1#");
    assert_eq!(rt("SUM(ANCHORARRAY(B2))"), "SUM(B2#)");
    // Only a cell can spill.
    assert!(parse("A1:B2#").is_err());
    assert!(parse("Name#").is_err());
    // Deleting the anchor's sheet leaves #REF!.
    assert_eq!(print(&crate::adjust::delete_sheet(parse("SUM(Data!A1#)").unwrap(), "Data", &[])), "SUM(#REF!)");
}

#[test]
fn delete_sheet_shrinks_3d_span() {
    let order: Vec<String> = ["Sheet1", "Sheet2", "Sheet3", "Results"].iter().map(|s| s.to_string()).collect();
    let del = |f: &str, n: &str| print(&crate::adjust::delete_sheet(parse(f).unwrap(), n, &order));
    assert_eq!(del("SUM(Sheet1:Sheet3!A1)", "Sheet1"), "SUM(Sheet2:Sheet3!A1)");
    assert_eq!(del("SUM(Sheet1:Sheet3!A1)", "Sheet3"), "SUM(Sheet1:Sheet2!A1)");
    assert_eq!(del("SUM(Sheet3:Sheet1!A1)", "Sheet1"), "SUM(Sheet3:Sheet2!A1)");
    assert_eq!(del("SUM(Sheet1:Sheet3!A1)", "Sheet2"), "SUM(Sheet1:Sheet3!A1)");
    assert_eq!(del("SUM(Sheet1:Sheet2!A1)", "Sheet1"), "SUM(Sheet2!A1)");
    assert_eq!(del("Sheet1!A1+Sheet3!A1", "Sheet1"), "#REF!+Sheet3!A1");
}

#[test]
fn precedence() {
    let e = parse("1+2*3").unwrap();
    assert!(matches!(e, Expr::Binary(BinOp::Add, _, _)));
    let e = parse("-2^2").unwrap();
    assert!(matches!(e, Expr::Binary(BinOp::Pow, _, _)));
    let e = parse("1&2=\"12\"").unwrap();
    assert!(matches!(e, Expr::Binary(BinOp::Eq, _, _)));
    let e = parse("2^3^2").unwrap(); // left associative: (2^3)^2
    if let Expr::Binary(BinOp::Pow, l, _) = e {
        assert!(matches!(*l, Expr::Binary(BinOp::Pow, _, _)));
    } else {
        panic!()
    }
}

#[test]
fn errors() {
    for bad in ["1+", "SUM(1,2", "(1", "{1,2;3}", "\"abc", "A1:", "1 2", "{A1}", ")", "Sheet1!", "#FOO!"] {
        assert!(parse(bad).is_err(), "{bad} should fail");
    }
}

#[test]
fn no_panic_on_garbage() {
    let samples =
        ["(((((", "[[[", "'''", "!!!", "$$$", "=@@@", "A1:B2:C3:D4", "{,}", "{;}", "F(,,,,)", ":::", "1e", "1e+", ".", "..", "$A$", "XFD1048577"];
    for s in samples {
        let _ = parse(s);
    }
    let deep = "(".repeat(5000) + "1" + &")".repeat(5000);
    assert!(parse(&deep).is_err());
}

#[test]
fn references() {
    let e = parse("SUM(A1:B2)+Sheet2!C3*D4").unwrap();
    let refs: Vec<String> = e.references().iter().map(|r| printer::reference_a1(r)).collect();
    assert_eq!(refs, ["A1:B2", "Sheet2!C3", "D4"]);
}

#[test]
fn copy_shift() {
    let e = parse("A1+$A1+A$1+$A$1+SUM(B2:C3)").unwrap();
    assert_eq!(print(&shift_relative(e, 2, 1)), "B3+$A3+B$1+$A$1+SUM(C4:D5)");
    let e = parse("A1").unwrap();
    assert_eq!(print(&shift_relative(e, -1, 0)), "#REF!");
    let e = parse("SUM(A:A)").unwrap();
    assert_eq!(print(&shift_relative(e, 5, 1)), "SUM(B:B)");
}

#[test]
fn insert_delete() {
    let ins = Edit::Insert { axis: Axis::Rows, at: 2, count: 2 };
    let e = parse("A1+A3+SUM(A2:A5)+Other!A3").unwrap();
    assert_eq!(print(&adjust(e, "Sheet1", "Sheet1", &ins)), "A1+A5+SUM(A2:A7)+Other!A3");
    let del = Edit::Delete { axis: Axis::Rows, at: 2, count: 2 }; // rows 3-4
    let e = parse("A1+A3+SUM(A2:A5)+A6").unwrap();
    assert_eq!(print(&adjust(e, "Sheet1", "Sheet1", &del)), "A1+#REF!+SUM(A2:A3)+A4");
    let del = Edit::Delete { axis: Axis::Cols, at: 0, count: 1 };
    let e = parse("SUM(A1:C1)+B2").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &del)), "SUM(A1:B1)+A2");
    let e = parse("Sheet1!B2").unwrap();
    assert_eq!(print(&adjust(e, "Other", "Sheet1", &del)), "Sheet1!A2");
    let e = parse("SUM(A:A)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &del)), "SUM(#REF!)");
}

#[test]
fn insert_delete_cells() {
    let range = |a: &str| gridcraft_core::RangeRef::parse(a).unwrap();
    let adj = |f: &str, e: &Edit| print(&adjust(parse(f).unwrap(), "S", "S", e));
    // Insert A3:A4, shifting down: column A moves, other columns and wider ranges don't.
    let ins = Edit::InsertCells { axis: Axis::Rows, range: range("A3:A4") };
    assert_eq!(adj("SUM(A1:A5)+A2+A3+$A$9+B3+SUM(A1:B5)+SUM(A:A)", &ins), "SUM(A1:A7)+A2+A5+$A$11+B3+SUM(A1:B5)+SUM(A:A)");
    // Insert B2 shifting right.
    let ins = Edit::InsertCells { axis: Axis::Cols, range: range("B2") };
    assert_eq!(adj("A2+B2+C2+B3+SUM(A2:C2)", &ins), "A2+C2+D2+B3+SUM(A2:D2)");
    // Delete A3:A4 shifting up: references into it become #REF!, those below move up.
    let del = Edit::DeleteCells { axis: Axis::Rows, range: range("A3:A4") };
    assert_eq!(adj("A2+A3+A4+A5+B5+SUM(A1:A10)+SUM(A3:A4)+SUM(A1:B10)", &del), "A2+#REF!+#REF!+A3+B5+SUM(A1:A8)+SUM(#REF!)+SUM(A1:B10)");
    // Delete B1:C2 shifting left.
    let del = Edit::DeleteCells { axis: Axis::Cols, range: range("B1:C2") };
    assert_eq!(adj("A1+B1+D1+D2+D3+SUM(A1:E1)", &del), "A1+#REF!+B1+B2+D3+SUM(A1:C1)");
}

#[test]
fn moves_and_renames() {
    let mv = Edit::Move { from: gridcraft_core::RangeRef::parse("A1:B2").unwrap(), to_row: 9, to_col: 3 };
    let e = parse("A1+B2+C3+SUM(A1:B2)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &mv)), "D10+E11+C3+SUM(D10:E11)");
    let mv = Edit::Move { from: gridcraft_core::RangeRef::parse("B:C").unwrap(), to_row: 0, to_col: 4 };
    assert_eq!(print(&adjust(parse("SUM(B:B)+SUM($C:$C)+D1").unwrap(), "S", "S", &mv)), "SUM(E:E)+SUM($F:$F)+D1");
    let e = parse("Old!A1+'Old'!B2").unwrap();
    assert_eq!(print(&rename_sheet(e, "Old", "New Name")), "'New Name'!A1+'New Name'!B2");
}

#[test]
fn rename_sheet_updates_qualified_names() {
    let e = parse("Data!MyName+MyName+Other!MyName").unwrap();
    assert_eq!(print(&rename_sheet(e, "data", "Numbers")), "Numbers!MyName+MyName+Other!MyName");
    let e = parse("Data!MyName").unwrap();
    assert_eq!(print(&rename_sheet(e, "Data", "New Name")), "'New Name'!MyName");
    // Stored bare, like a parsed name, so the evaluator finds the sheet and a second rename still matches.
    let e = rename_sheet(parse("Data!MyName").unwrap(), "Data", "It's");
    assert_eq!(e, Expr::Name("It's!MyName".into()));
    assert_eq!(print(&rename_sheet(e, "It's", "Totals")), "Totals!MyName");
}

#[test]
fn moves_between_sheets() {
    let mv = Edit::MoveToSheet { from: gridcraft_core::RangeRef::parse("A1:B2").unwrap(), to_sheet: "New Name".into(), to_row: 9, to_col: 3 };
    for (host, input, expected) in [
        ("Source", "A1+$B$2+C3+SUM(A1:B2)", "'New Name'!D10+'New Name'!$E$11+C3+SUM('New Name'!D10:E11)"),
        ("Other", "A1+sOuRcE!$B2+Other!B2", "A1+'New Name'!$E11+Other!B2"),
        ("Source", "SUM(A1:C3)+SUM(Source:Other!A1)", "SUM(A1:C3)+SUM(Source:Other!A1)"),
    ] {
        assert_eq!(print(&adjust(parse(input).unwrap(), host, "Source", &mv)), expected);
    }
    let edge = Edit::MoveToSheet {
        from: gridcraft_core::RangeRef::parse("A1:B2").unwrap(),
        to_sheet: "New Name".into(),
        to_row: gridcraft_core::MAX_ROWS - 1,
        to_col: 3,
    };
    assert_eq!(print(&adjust(parse("B2").unwrap(), "Source", "Source", &edge)), "#REF!");
    for (from, input, row, col, expected) in [("A:A", "$A:$A", 0, 2, "Dest!$C:$C"), ("1:1", "$1:$1", 2, 0, "Dest!$3:$3")] {
        let mv = Edit::MoveToSheet { from: gridcraft_core::RangeRef::parse(from).unwrap(), to_sheet: "Dest".into(), to_row: row, to_col: col };
        assert_eq!(print(&adjust(parse(input).unwrap(), "Source", "Source", &mv)), expected);
    }
}

#[test]
fn r1c1() {
    let e = parse("A1+$B$2+SUM(C:C)").unwrap();
    assert_eq!(print_r1c1(&e, gridcraft_core::CellRef::new(1, 1)), "R[-1]C[-1]+R2C2+SUM(C[1])");
}

#[test]
fn spans() {
    let toks = lexer::tokenize("SUM(A1, B2)").unwrap();
    let a1 = toks.iter().find(|t| t.tok == lexer::Tok::Word("A1".into())).unwrap();
    assert_eq!((a1.start, a1.end), (4, 6));
}

#[test]
fn unbracketed_this_row_column() {
    // Writers such as xlsxwriter spell `[@Units]` as `[[#This Row],Units]`.
    let p = |f: &str| parse(f).unwrap_or_else(|e| panic!("{f}: {e}"));
    let Expr::Binary(BinOp::Mul, a, _) = p("[[#This Row],Units]*[[#This Row],Price]") else { panic!() };
    let Expr::Struct(s) = *a else { panic!() };
    assert_eq!((s.specifiers.as_slice(), s.col_start.as_deref()), ([StructItem::ThisRow].as_slice(), Some("Units")));
    assert_eq!(rt("[[#This Row],Units]*[[#This Row],Price]"), "[@Units]*[@Price]");
    assert_eq!(rt("Sales[[#This Row],Units]"), "Sales[@Units]");
    assert_eq!(rt("Sales[[#This Row], Units ]"), "Sales[@Units]");
    assert_eq!(rt("[[#This Row],Units]+[@[Unit Price]]+Sales[[#This Row],[Qty]]"), "[@Units]+[@[Unit Price]]+Sales[@Qty]");
    // Unexpected text between the brackets is an error, never silently dropped.
    for bad in [
        "[[#This Row]x]",
        "[[#This Row],[A]]x",
        "[[#Data],Col]",
        "[[#This Row],A:B]",
        "[x[#This Row]]",
        "[[#This Row],]x",
        "Sales[[#All],[A]]junk",
        "Sales[[A],[B],[C]]",
        "Sales[@[A]x]",
    ] {
        assert!(parse(bad).is_err(), "{bad} should fail");
    }
}

#[test]
fn struct_column_keeps_edge_spaces() {
    // The spaces are part of the column name: `Table1[Qty ]` names `Qty `, never `Qty`.
    let Expr::Struct(s) = parse("Table1[Qty ]").unwrap() else { panic!() };
    assert_eq!(s.col_start.as_deref(), Some("Qty "));
    let Expr::Struct(s) = parse("Table1[ Qty]").unwrap() else { panic!() };
    assert_eq!(s.col_start.as_deref(), Some(" Qty"));
    // Excel writes such a column in the double-bracket form.
    assert_eq!(rt("Table1[Qty ]"), "Table1[[Qty ]]");
    for f in ["Table1[[Qty ]]", "Table1[[ Sum for Q2 ]]", "Table1[[#Totals],[Qty ]]", "Table1[@[Qty ]]", "Table1[Unit Price]"] {
        assert_eq!(rt(f), f);
    }
}

#[test]
fn trim_references() {
    for f in [
        "SUM(A1:.E10)",
        "SUM(A1.:E10)",
        "SUM(A1.:.E10)",
        "SUM(A:.A)",
        "SUM($A.:.$C)",
        "ROWS(1:.5)",
        "ROWS($1.:$5)",
        "ROWS(1.:.5)",
        "Sheet2!$A$1:.B9",
        "Q1.2026!A1.:.B2",
        "'Q1 2026'!A1:.B2",
        "SUM(A1:.A9,B1.:B9)",
        "A1:.A9*2",
    ] {
        assert_eq!(rt(f), f, "round trip of {f}");
    }
    let Expr::Call(_, args) = parse("SUM(A1:.E10)").unwrap() else { panic!() };
    assert!(matches!(args.as_slice(), [Expr::Unary(UnOp::Trim(TrimMode::Trailing), r)] if matches!(**r, Expr::Ref(_))));
    // Decimals, row ranges and dotted names keep their meaning.
    assert_eq!(parse("1.5+.5").unwrap(), Expr::Binary(BinOp::Add, Box::new(Expr::Number(1.5)), Box::new(Expr::Number(0.5))));
    for f in ["SUM(1:5)", "Q1.2026!A1:B2", "my.name+1", "SUM(A1:B2)*1.5", "1.:5"] {
        assert_eq!(rt(f), f);
    }
    assert_eq!(rt("'Q1.2026'!A1:B2"), "Q1.2026!A1:B2");
    assert!(matches!(parse("Q1.2026!A1:B2").unwrap(), Expr::Ref(_)));
    // File call forms.
    assert_eq!(rt("SUM(_xlfn._TRO_TRAILING(A1:E10))"), "SUM(A1:.E10)");
    assert_eq!(rt("_xlfn._TRO_LEADING(A:A)"), "A.:A");
    assert_eq!(rt("_xlfn._TRO_ALL(Sheet1!A1:B2)"), "Sheet1!A1.:.B2");
    // Not a range: printed as the equivalent call.
    assert_eq!(rt("_xlfn._TRO_ALL(B2)"), "TRIMRANGE(B2,3,3)");
    for bad in ["A1.:", ".:A1", "A1:.:B2", "A1.:.:B2"] {
        assert!(parse(bad).is_err(), "{bad} should fail");
    }
    // R1C1 keeps the operator.
    assert_eq!(print_r1c1(&parse("SUM(A1:.B2)+ROWS(2.:2)").unwrap(), gridcraft_core::CellRef::new(0, 0)), "SUM(RC:.R[1]C[1])+ROWS(R[1].:R[1])");
}

#[test]
fn trim_references_adjust() {
    let ins = Edit::Insert { axis: Axis::Rows, at: 2, count: 2 };
    let e = parse("SUM(A1:.A5)+SUM(B:.B)+ROWS(2.:.4)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &ins)), "SUM(A1:.A7)+SUM(B:.B)+ROWS(2.:.6)");
    let del = Edit::Delete { axis: Axis::Cols, at: 0, count: 1 };
    let e = parse("SUM(A1.:C1)+SUM(A:.A)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &del)), "SUM(A1.:B1)+SUM(#REF!)");
    assert_eq!(print(&shift_relative(parse("SUM(A1.:.B2)").unwrap(), 1, 1)), "SUM(B2.:.C3)");
    let refs: Vec<String> = parse("SUM(Sheet2!A1:.B5)").unwrap().references().iter().map(|r| printer::reference_a1(r)).collect();
    assert_eq!(refs, ["Sheet2!A1:B5"]);
}

#[test]
fn prints_long_flat_chain_without_deep_recursion() {
    // Small stack: a recursive printer overflows long before 20000 levels.
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut e = Expr::Number(1.0);
            for _ in 0..20000 {
                e = Expr::Binary(BinOp::Add, Box::new(e), Box::new(Expr::Number(1.0)));
            }
            let text = print(&e);
            let r1c1 = print_r1c1(&e, gridcraft_core::CellRef { row: 0, col: 0 });
            // Dropping the deep tree recurses; leak it so only the printer is under test.
            std::mem::forget(e);
            (text, r1c1)
        })
        .expect("spawn");
    let (text, r1c1) = handle.join().expect("printer must not overflow the stack");
    assert_eq!(text, vec!["1"; 20001].join("+"));
    assert_eq!(r1c1, text);
}

#[test]
fn hostile_formulas_are_errors_not_stack_overflows() {
    // A small stack: parsing must fail cleanly instead of building trees deeper than the nesting
    // limit, which dropping, printing and evaluating walk recursively.
    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let percents = format!("1{}", "%".repeat(1_000));
            let invokes = format!("LAMBDA(x,x)(1){}", "(1)".repeat(1_000));
            let long_chain = format!("1{}", "+1".repeat(100_000));
            let huge_percents = format!("1{}", "%".repeat(1_000_000));
            (parse(&percents).is_err(), parse(&invokes).is_err(), parse(&long_chain).is_err(), parse(&huge_percents).is_err())
        })
        .expect("spawn");
    assert_eq!(handle.join().expect("parsing must not overflow the stack"), (true, true, true, true));
    // Within the limits these still parse.
    assert!(parse(&format!("1{}", "%".repeat(100))).is_ok());
    assert!(parse("LAMBDA(x,x)(1)(2)").is_ok());
}

#[test]
fn formulas_longer_than_excel_allows_are_refused() {
    let at_limit = format!("1{}", "+1".repeat((crate::MAX_FORMULA_LEN - 1) / 2));
    assert_eq!(at_limit.chars().count(), crate::MAX_FORMULA_LEN - 1);
    assert!(parse(&at_limit).is_ok());
    assert!(parse(&format!("={at_limit}")).is_ok(), "the leading = does not count");
    let over = format!("{at_limit}+1");
    let err = parse(&over).unwrap_err();
    assert!(err.msg.contains("8192"), "{}", err.msg);
    // Characters, not bytes: 8,000 two-byte characters in a text literal are within the limit.
    let text = format!("\"{}\"", "é".repeat(8_000));
    assert!(text.len() > crate::MAX_FORMULA_LEN);
    assert!(parse(&text).is_ok());
}
