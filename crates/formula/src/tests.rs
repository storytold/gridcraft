use super::*;
use crate::adjust::{Axis, Edit, adjust, rename_sheet, shift_relative};

fn rt(src: &str) -> String {
    print(&parse(src).unwrap_or_else(|e| panic!("{src}: {e}")))
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
    assert_eq!(rt("true"), "TRUE");
    assert_eq!(rt("'Sheet1'!A1"), "Sheet1!A1");
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
fn moves_and_renames() {
    let mv = Edit::Move { from: gridcraft_core::RangeRef::parse("A1:B2").unwrap(), to_row: 9, to_col: 3 };
    let e = parse("A1+B2+C3+SUM(A1:B2)").unwrap();
    assert_eq!(print(&adjust(e, "S", "S", &mv)), "D10+E11+C3+SUM(D10:E11)");
    let e = parse("Old!A1+'Old'!B2").unwrap();
    assert_eq!(print(&rename_sheet(e, "Old", "New Name")), "'New Name'!A1+'New Name'!B2");
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
