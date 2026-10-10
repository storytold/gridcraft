use gridcraft_core::{CellError, CellRef, Value};
use gridcraft_model::{Cell, Formula, Workbook};

use crate::Calc;

fn c(s: &str) -> CellRef {
    CellRef::parse(s).unwrap()
}

struct T {
    wb: Workbook,
    calc: Calc,
}

impl T {
    fn new() -> T {
        T { wb: Workbook::new(), calc: Calc::new() }
    }
    fn set(&mut self, at: &str, input: &str) {
        let cell = if input.starts_with('=') {
            Cell::formula(Formula::new(input))
        } else {
            Cell::value(gridcraft_core::parse::parse_input(input, self.wb.date_system).value)
        };
        self.wb.sheet_mut(0).unwrap().cells.set(c(at), cell);
        self.calc.cells_changed(&mut self.wb, &[(0, c(at))]);
    }
    fn get(&self, at: &str) -> Value {
        self.wb.sheet(0).unwrap().value(c(at))
    }
    fn num(&self, at: &str) -> f64 {
        self.get(at).as_f64().unwrap_or_else(|| panic!("{at} = {:?}", self.get(at)))
    }
}

#[test]
fn spill_range_operator() {
    let mut t = T::new();
    t.set("A1", "3");
    t.set("B1", "=SEQUENCE(A1)");
    t.set("C1", "=SUM(B1#)");
    t.set("C2", "=ROWS(B1#)");
    assert_eq!((t.num("C1"), t.num("C2")), (6.0, 3.0));
    // Growing and shrinking the spill recalculates its users.
    t.set("A1", "5");
    assert_eq!((t.num("C1"), t.num("C2")), (15.0, 5.0));
    t.set("A1", "2");
    assert_eq!((t.num("C1"), t.num("C2")), (3.0, 2.0));
    // A blocked spill, or a cell that doesn't spill, gives #REF!.
    t.set("B2", "x");
    assert_eq!(t.get("C1"), Value::Error(CellError::Ref));
    t.set("B2", "");
    assert_eq!(t.num("C1"), 3.0);
    t.set("D1", "=SUM(A1#)");
    assert_eq!(t.get("D1"), Value::Error(CellError::Ref));
    // Through a defined name.
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Seq".into(),
        scope: None,
        formula: "Sheet1!$B$1#".into(),
        comment: String::new(),
        hidden: false,
    });
    t.set("E1", "=SUM(Seq)*10");
    assert_eq!(t.num("E1"), 30.0);
    t.set("A1", "4");
    assert_eq!(t.num("E1"), 100.0);
}

#[test]
fn arithmetic_and_dependencies() {
    let mut t = T::new();
    t.set("A1", "2");
    t.set("A2", "3");
    t.set("A3", "=A1*A2+1");
    assert_eq!(t.num("A3"), 7.0);
    t.set("A1", "10");
    assert_eq!(t.num("A3"), 31.0);
    t.set("B1", "=SUM(A1:A3)");
    assert_eq!(t.num("B1"), 44.0);
    t.set("A2", "0");
    assert_eq!(t.num("A3"), 1.0);
    assert_eq!(t.num("B1"), 11.0);
}

#[test]
fn chains_out_of_order() {
    let mut t = T::new();
    t.set("A5", "=A4+1");
    t.set("A4", "=A3+1");
    t.set("A3", "=A2+1");
    t.set("A2", "=A1+1");
    t.set("A1", "1");
    assert_eq!(t.num("A5"), 5.0);
}

#[test]
fn long_chain() {
    let mut t = T::new();
    t.set("A1", "1");
    for r in 2..=3000 {
        t.set(&format!("A{r}"), &format!("=A{}+1", r - 1));
    }
    assert_eq!(t.num("A3000"), 3000.0);
    t.set("A1", "10");
    assert_eq!(t.num("A3000"), 3009.0);
}

#[test]
fn errors_and_cycles() {
    let mut t = T::new();
    t.set("A1", "=1/0");
    assert_eq!(t.get("A1"), Value::Error(CellError::Div0));
    t.set("B1", "=B2");
    t.set("B2", "=B1");
    assert!(t.get("B1").is_error());
    t.set("C1", "=NOSUCHFN(1)");
    assert_eq!(t.get("C1"), Value::Error(CellError::Name));
    t.set("C2", "=1+");
    assert_eq!(t.get("C2"), Value::Error(CellError::Name));
}

#[test]
fn operators() {
    let mut t = T::new();
    t.set("A1", "=\"a\"&1&TRUE");
    assert_eq!(t.get("A1"), Value::from("a1TRUE"));
    t.set("A2", "=-2^2");
    assert_eq!(t.num("A2"), 4.0);
    t.set("A3", "=50%");
    assert_eq!(t.num("A3"), 0.5);
    t.set("A4", "=\"abc\"=\"ABC\"");
    assert_eq!(t.get("A4"), Value::Bool(true));
    t.set("A5", "=\"5\"+1");
    assert_eq!(t.num("A5"), 6.0);
    t.set("A6", "=\"x\"+1");
    assert_eq!(t.get("A6"), Value::Error(CellError::Value));
    t.set("A7", "=B99");
    assert_eq!(t.num("A7"), 0.0);
}

#[test]
fn blocked_spill_reads_as_spill_error() {
    // Calculated in the same pass, a formula over a blocked spill read the array that couldn't
    // spill.
    let mut t = T::new();
    let s = t.wb.sheet_mut(0).unwrap();
    for a in ["A1", "B1", "C1", "A2", "C2", "A3", "B3", "C3"] {
        s.cells.set(c(a), Cell::value(Value::Number(1.0)));
    }
    s.cells.set(c("B2"), Cell::formula(Formula::new("=SEQUENCE(2,2,9)")));
    s.cells.set(c("E1"), Cell::formula(Formula::new("=SUM(A1:C3)")));
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.get("B2"), Value::Error(CellError::Spill));
    assert_eq!(t.get("E1"), Value::Error(CellError::Spill));
    assert_eq!(t.num("C3"), 1.0);
}

#[test]
fn blocked_spill_read_through_offset() {
    // The reader's reference is only known while evaluating, so it isn't in the dependency graph.
    let mut t = T::new();
    t.set("E4", "5");
    t.set("E8", "=SUM(OFFSET(E2,0,0,2,2))");
    t.set("E3", "=SEQUENCE(2,2,3)");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.get("E3"), Value::Error(CellError::Spill));
    assert_eq!(t.get("E8"), Value::Error(CellError::Spill));
}

#[test]
fn spill_into_the_old_area_of_a_blocked_neighbour() {
    // C7's array becomes blocked by C8, whose array then takes over C7's old area.
    let mut t = T::new();
    t.set("C7", "=SEQUENCE(2,2)");
    t.set("C8", "=SEQUENCE(2,2)");
    assert_eq!(t.get("C7"), Value::Error(CellError::Spill));
    assert_eq!([t.num("C8"), t.num("D8"), t.num("C9"), t.num("D9")], [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn arrays_do_not_spill_in_tables() {
    // Excel: spilled array formulas aren't supported in tables.
    let mut t = T::new();
    t.wb.sheet_mut(0).unwrap().tables.push(gridcraft_model::Table {
        id: 1,
        name: "Table1".into(),
        range: gridcraft_core::RangeRef::parse("B3:C6").unwrap(),
        header_row: true,
        totals_row: false,
        columns: vec![],
        style: String::new(),
        banded_rows: false,
        banded_cols: false,
        first_col: false,
        last_col: false,
        filter_button: false,
    });
    t.set("C4", "=SEQUENCE(2)");
    t.set("E1", "=SEQUENCE(4)");
    t.set("H1", "=SUM(C4:C5)");
    assert_eq!(t.get("C4"), Value::Error(CellError::Spill));
    assert_eq!(t.get("C5"), Value::Empty);
    assert_eq!(t.get("H1"), Value::Error(CellError::Spill));
    assert_eq!(t.num("E4"), 4.0);
}

#[test]
fn ifs_and_lazy() {
    let mut t = T::new();
    t.set("A1", "5");
    t.set("B1", "=IF(A1>3,\"big\",1/0)");
    assert_eq!(t.get("B1"), Value::from("big"));
    t.set("B2", "=IFERROR(1/0,\"oops\")");
    assert_eq!(t.get("B2"), Value::from("oops"));
    t.set("B3", "=CHOOSE(2,\"a\",\"b\",\"c\")");
    assert_eq!(t.get("B3"), Value::from("b"));
    t.set("B4", "=SWITCH(A1,1,\"one\",5,\"five\",\"other\")");
    assert_eq!(t.get("B4"), Value::from("five"));
}

#[test]
fn references_functions() {
    let mut t = T::new();
    for (i, v) in [10, 20, 30, 40].iter().enumerate() {
        t.set(&format!("A{}", i + 1), &v.to_string());
    }
    t.set("B1", "=ROW(A3)");
    assert_eq!(t.num("B1"), 3.0);
    t.set("B2", "=ROWS(A1:A4)*COLUMNS(A1:C1)");
    assert_eq!(t.num("B2"), 12.0);
    t.set("B3", "=SUM(OFFSET(A1,1,0,2,1))");
    assert_eq!(t.num("B3"), 50.0);
    t.set("B4", "=INDIRECT(\"A\"&4)");
    assert_eq!(t.num("B4"), 40.0);
    t.set("B5", "=SUM(A1:INDEX(A1:A4,3))");
    assert_eq!(t.num("B5"), 60.0);
    t.set("B6", "=SUM(A:A)");
    assert_eq!(t.num("B6"), 100.0);
    t.set("A2", "25");
    assert_eq!(t.num("B3"), 55.0);
    assert_eq!(t.num("B6"), 105.0);
}

#[test]
fn spills() {
    let mut t = T::new();
    t.set("A1", "=SEQUENCE(3)");
    assert_eq!(t.num("A1"), 1.0);
    assert_eq!(t.num("A3"), 3.0);
    t.set("B1", "=SUM(A1:A3)");
    assert_eq!(t.num("B1"), 6.0);
    t.set("A2", "x");
    assert_eq!(t.get("A1"), Value::Error(CellError::Spill));
    t.set("A2", "");
    assert_eq!(t.num("A3"), 3.0);
    t.set("C1", "={1,2;3,4}*10");
    assert_eq!(t.num("D2"), 40.0);
}

#[test]
fn references_keep_their_size_past_the_used_range() {
    let mut t = T::new();
    t.set("A1", "5");
    t.set("A2", "6");
    t.set("C1", "=ROWS(A1:A10*1)");
    t.set("C2", "=SUMPRODUCT(--(A1:A10=\"\"))");
    t.set("C3", "=COUNTBLANK(A1:A10)");
    t.set("C4", "=COUNTIF(A1:A10,\"\")");
    t.set("C5", "=COLUMNS(TRANSPOSE(A1:A10))");
    for (at, want) in [("C1", 10.0), ("C2", 8.0), ("C3", 8.0), ("C4", 8.0), ("C5", 10.0)] {
        assert_eq!(t.num(at), want, "{at}");
    }
    // Whole columns are still read up to the used range.
    t.set("D1", "=ROWS(FILTER(A:A,A:A<>\"\"))");
    assert_eq!(t.num("D1"), 2.0);
}

#[test]
fn spilled_blank_cells_show_zero() {
    let mut t = T::new();
    t.set("A1", "5");
    t.set("A3", "7");
    t.set("C1", "=A1:A4");
    assert_eq!(t.get("C2"), Value::Number(0.0));
    assert_eq!(t.get("C4"), Value::Number(0.0));
    t.set("D1", "=COUNT(C1:C4)");
    assert_eq!(t.num("D1"), 4.0);
    t.set("E1", "=A2:A3");
    assert_eq!(t.get("E1"), Value::Number(0.0));
    t.set("F1", "=SORT(A1:A3)");
    assert_eq!(t.get("F3"), Value::Number(0.0));
}

#[test]
fn let_lambda() {
    let mut t = T::new();
    t.set("A1", "=LET(x,2,y,3,x*y)");
    assert_eq!(t.num("A1"), 6.0);
    t.set("A2", "=LAMBDA(a,b,a+b)(2,3)");
    assert_eq!(t.num("A2"), 5.0);
    t.set("A3", "=REDUCE(0,{1,2,3},LAMBDA(acc,v,acc+v))");
    assert_eq!(t.num("A3"), 6.0);
    t.set("A4", "=SUM(MAP({1,2,3},LAMBDA(v,v*v)))");
    assert_eq!(t.num("A4"), 14.0);
}

#[test]
fn names_and_sheets() {
    let mut t = T::new();
    t.wb.names.push(gridcraft_model::DefinedName { name: "Rate".into(), scope: None, formula: "0.5".into(), comment: String::new(), hidden: false });
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.wb.sheet_mut(1).unwrap().set_value(c("A1"), Value::Number(8.0));
    t.calc.rebuild(&t.wb);
    t.set("A1", "=Rate*Data!A1");
    assert_eq!(t.num("A1"), 4.0);
    t.wb.sheet_mut(1).unwrap().set_value(c("A1"), Value::Number(10.0));
    t.calc.cells_changed(&mut t.wb, &[(1, c("A1"))]);
    assert_eq!(t.num("A1"), 5.0);
}

#[test]
fn legacy_array_formulas_fill_their_range() {
    let mut t = T::new();
    t.set("A1", "1");
    t.set("A2", "2");
    let cse = |t: &mut T, at: &str, formula: &str, range: &str| {
        let mut f = Formula::new(formula);
        f.array = gridcraft_core::RangeRef::parse(range);
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(f));
        t.calc.cells_changed(&mut t.wb, &[(0, c(at))]);
    };
    // Past the result: #N/A. A single column repeats across the range.
    cse(&mut t, "C1", "=A1:A2*2", "C1:D3");
    cse(&mut t, "F1", "=A1:A2*2", "F1");
    cse(&mut t, "G1", "=5", "G1:G2");
    assert_eq!(t.num("C2"), 4.0);
    assert_eq!(t.num("D2"), 4.0);
    assert_eq!(t.get("C3"), Value::Error(CellError::NA));
    // A smaller range shows only part of the result and never spills.
    assert_eq!(t.num("F1"), 2.0);
    assert_eq!(t.get("F2"), Value::Empty);
    assert_eq!(t.num("G2"), 5.0);
    // A range too large to fill is an error, not an allocation of every cell.
    cse(&mut t, "H1", "=1", "H1:XFD1048576");
    assert_eq!(t.get("H1"), Value::Error(CellError::Num));
}

#[test]
fn text_function() {
    let mut t = T::new();
    t.set("A1", "=TEXT(1234.5,\"#,##0.00\")");
    assert_eq!(t.get("A1"), Value::from("1,234.50"));
}

#[test]
fn subtotal_skips_hidden() {
    let mut t = T::new();
    t.set("A1", "1");
    t.set("A2", "2");
    t.set("A3", "3");
    t.wb.sheet_mut(0).unwrap().rows.insert(1, gridcraft_model::LineInfo { hidden: true, ..Default::default() });
    t.set("B1", "=SUBTOTAL(109,A1:A3)");
    t.set("B2", "=SUBTOTAL(9,A1:A3)");
    assert_eq!(t.num("B1"), 4.0);
    assert_eq!(t.num("B2"), 6.0);
}

#[test]
fn recalc_all_after_load() {
    let mut t = T::new();
    let s = t.wb.sheet_mut(0).unwrap();
    s.cells.set(c("A1"), Cell::value(Value::Number(4.0)));
    s.cells.set(c("A2"), Cell::formula(Formula::new("=A1*2")));
    s.cells.set(c("A3"), Cell::formula(Formula::new("=A2+A1")));
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("A3"), 12.0);
}

#[test]
fn sumif_sum_range_takes_the_criteria_size() {
    // Excel adds up the range that starts at sum_range's top-left cell with the criteria range's
    // height and width, whatever size sum_range has.
    let mut t = T::new();
    for (r, (k, v)) in [("a", 1), ("b", 2), ("a", 3)].iter().enumerate() {
        t.set(&format!("A{}", r + 1), k);
        t.set(&format!("B{}", r + 1), &v.to_string());
    }
    t.set("D1", "=SUMIF(A1:A3,\"a\",B1)");
    assert_eq!(t.num("D1"), 4.0);
    t.set("D2", "=AVERAGEIF(A1:A3,\"a\",B1)");
    assert_eq!(t.num("D2"), 2.0);
    t.set("D3", "=SUMIF(A1:A3,\"a\",B1:B2)");
    assert_eq!(t.num("D3"), 4.0);
    t.set("D4", "=SUMIF(A1:A3,\"a\",B1:F1)");
    assert_eq!(t.num("D4"), 4.0);
    t.set("D5", "=SUMIF(A1:A3,\"a\",B1:B99)");
    assert_eq!(t.num("D5"), 4.0);
    t.set("D6", "=SUMIF(A:A,\"a\",B1)");
    assert_eq!(t.num("D6"), 4.0);
    t.set("D7", "=SUMIF(A1:A3,\"a\",$B$2)");
    assert_eq!(t.num("D7"), 2.0); // B2:B4: rows 1 and 3 match → B2 + B4 (empty)
    t.set("D8", "=SUMIF(A1:A3,\"a\")");
    assert_eq!(t.num("D8"), 0.0);
    // A horizontal criteria range resizes across.
    t.set("H1", "1");
    t.set("I1", "0");
    t.set("J1", "1");
    t.set("H2", "10");
    t.set("I2", "20");
    t.set("J2", "30");
    t.set("D9", "=SUMIF(H1:J1,1,H2)");
    assert_eq!(t.num("D9"), 40.0);
    // The resized cells are precedents: editing one recalculates.
    t.set("B3", "5");
    assert_eq!(t.num("D1"), 6.0);
    assert_eq!(t.num("D2"), 3.0);
    assert_eq!(t.num("D3"), 6.0);
    assert_eq!(t.num("D4"), 6.0);
    assert_eq!(t.num("D6"), 6.0);
    // SUMIFS and AVERAGEIFS don't resize: different sizes are #VALUE!.
    t.set("D11", "=SUMIFS(B1,A1:A3,\"a\")");
    assert_eq!(t.get("D11"), Value::Error(CellError::Value));
    t.set("D12", "=AVERAGEIFS(B1:B2,A1:A3,\"a\")");
    assert_eq!(t.get("D12"), Value::Error(CellError::Value));
    t.set("D13", "=SUMIFS(B1:B3,A1:A3,\"a\")");
    assert_eq!(t.num("D13"), 6.0);
}

#[test]
fn sumif_resized_sum_range_stops_at_the_sheet_edge() {
    let mut t = T::new();
    t.set("A1", "a");
    t.set("A2", "a");
    t.set("B1048575", "5");
    t.set("B1048576", "7");
    t.set("C1", "=SUMIF(A1:A2,\"a\",B1048576)");
    assert_eq!(t.num("C1"), 7.0);
    t.set("C2", "=SUMIF(A1:A2,\"a\",B1048575)");
    assert_eq!(t.num("C2"), 12.0);
    t.set("C3", "=SUMIF(A1:B1,\"a\",XFD1)");
    assert_eq!(t.num("C3"), 0.0);
}

#[test]
fn sumif_resized_sum_range_on_another_sheet_and_through_a_name() {
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Amounts".into(),
        scope: None,
        formula: "Data!$B$1".into(),
        comment: String::new(),
        hidden: false,
    });
    for (r, k) in ["a", "b", "a"].iter().enumerate() {
        t.set(&format!("A{}", r + 1), k);
        t.wb.sheet_mut(1).unwrap().set_value(CellRef::new(r as u32, 1), Value::Number((r + 1) as f64));
    }
    t.calc.rebuild(&t.wb);
    t.set("C1", "=SUMIF(A1:A3,\"a\",Data!B1)");
    assert_eq!(t.num("C1"), 4.0);
    t.set("C2", "=AVERAGEIF(A1:A3,\"a\",Amounts)");
    assert_eq!(t.num("C2"), 2.0);
    t.wb.sheet_mut(1).unwrap().set_value(c("B3"), Value::Number(9.0));
    t.calc.cells_changed(&mut t.wb, &[(1, c("B3"))]);
    assert_eq!(t.num("C1"), 10.0);
    assert_eq!(t.num("C2"), 5.0);
}

impl T {
    /// Stores formulas without recalculating, then recalculates everything at once (as loading
    /// a file does).
    fn load(&mut self, cells: impl IntoIterator<Item = (String, String)>) {
        for (at, f) in cells {
            self.wb.sheet_mut(0).unwrap().cells.set(c(&at), Cell::formula(Formula::new(&f)));
        }
        self.calc.recalc_all(&mut self.wb);
    }
}

#[test]
fn long_chains_compute_in_either_direction() {
    let n = 20_000;
    let mut t = T::new();
    // C1 = C2+1, C2 = C3+1, …: every formula reads the row below, the reverse of row order.
    t.load((1..n).map(|r| (format!("C{r}"), format!("=C{}+1", r + 1))).chain([(format!("C{n}"), "=1".to_string())]));
    assert_eq!(t.num("C1"), n as f64);
    assert!(t.calc.circular.is_empty());
    // An edit at the far end recalculates the whole chain.
    t.set(&format!("C{n}"), "=10");
    assert_eq!(t.num("C1"), (n + 9) as f64);
}

#[test]
fn long_chains_need_no_call_stack() {
    // Evaluation sets formulas aside on a heap stack instead of recursing, so a long reverse
    // chain through a defined name (no static precedents to order by) fits a tiny thread stack.
    let run = || {
        let n = 5_000;
        let mut t = T::new();
        t.wb.names.push(gridcraft_model::DefinedName {
            name: "Step".into(),
            scope: None,
            formula: "1".into(),
            comment: String::new(),
            hidden: false,
        });
        t.load((1..n).map(|r| (format!("A{r}"), format!("=A{}+Step", r + 1))).chain([(format!("A{n}"), "=Step".to_string())]));
        t.num("A1")
    };
    let a1 = std::thread::Builder::new().stack_size(512 * 1024).spawn(run).unwrap().join().unwrap();
    assert_eq!(a1, 5_000.0);
}

#[test]
fn cycles_are_circular_and_so_is_what_reads_them() {
    let mut t = T::new();
    t.set("A1", "=B1+1");
    t.set("B1", "=A1+1");
    assert!(!t.calc.circular.is_empty());
    t.set("C1", "=A1*2");
    t.set("D1", "=5");
    for at in ["A1", "B1", "C1"] {
        assert_eq!(t.get(at), Value::Error(CellError::Circ), "{at}");
    }
    assert_eq!(t.num("D1"), 5.0);
    // Breaking the cycle recalculates everything that was stuck behind it.
    t.set("B1", "=D1");
    assert_eq!((t.num("A1"), t.num("B1"), t.num("C1")), (6.0, 5.0, 12.0));
    assert!(t.calc.circular.is_empty());
    // A formula that reads itself.
    t.set("E1", "=E1+1");
    assert_eq!(t.get("E1"), Value::Error(CellError::Circ));
}

#[test]
fn ranges_over_formulas_wait_for_all_of_them() {
    // SUM reads a range of formulas below it that are dirty in the same pass.
    let mut t = T::new();
    t.load((2..=1001).map(|r| (format!("A{r}"), format!("=ROW()+B{r}"))).chain([("A1".to_string(), "=SUM(A2:A1001)".to_string())]));
    let want: f64 = (2..=1001).map(f64::from).sum();
    assert_eq!(t.num("A1"), want);
    t.set("B5", "100");
    assert_eq!(t.num("A1"), want + 100.0);
}

#[test]
fn indexed_lookups_match_scanning_ones() {
    // Each lookup appears three times over the same table, so the later copies go through the
    // shared range and its index (built on the second search): all copies must agree with what a
    // scan finds.
    let mut t = T::new();
    let mut cells: Vec<(String, String)> = (1..=100).map(|r| (format!("A{r}"), format!("=\"Key{r}\""))).collect();
    cells.extend([
        ("A5".to_string(), "=\"key3\"".to_string()),
        ("A10".to_string(), "=0.3".to_string()),
        ("A11".to_string(), "=TRUE".to_string()),
        ("A12".to_string(), "=7".to_string()),
    ]);
    cells.extend((1..=100).map(|r| (format!("B{r}"), format!("={r}"))));
    cells.extend((1..=100).map(|r| (format!("C{r}"), format!("={}", r * 10))));
    let cases = [
        ("=VLOOKUP(\"KEY3\",$A$1:$B$100,2,FALSE)", Value::Number(3.0)),
        ("=XLOOKUP(\"key3\",$A$1:$A$100,$B$1:$B$100,,0,-1)", Value::Number(5.0)),
        ("=MATCH(0.1+0.2,$A$1:$A$100,0)", Value::Number(10.0)),
        ("=MATCH(TRUE,$A$1:$A$100,0)", Value::Number(11.0)),
        ("=XMATCH(7,$A$1:$A$100)", Value::Number(12.0)),
        ("=MATCH(\"7\",$A$1:$A$100,0)", Value::Error(CellError::NA)),
        ("=VLOOKUP(\"Key2*\",$A$1:$B$100,2,FALSE)", Value::Number(2.0)),
        ("=MATCH(\"nope\",$A$1:$A$100,0)", Value::Error(CellError::NA)),
        ("=MATCH(255,$C$1:$C$100,1)", Value::Number(25.0)),
        ("=XLOOKUP(255,$C$1:$C$100,$B$1:$B$100,,1)", Value::Number(26.0)),
        ("=XMATCH(990,$C$1:$C$100,0,2)", Value::Number(99.0)),
        ("=VLOOKUP(5,$C$1:$C$100,1,TRUE)", Value::Error(CellError::NA)),
    ];
    for (i, (f, _)) in cases.iter().enumerate() {
        for col in ["E", "F", "G"] {
            cells.push((format!("{col}{}", i + 1), f.to_string()));
        }
    }
    t.load(cells);
    for (i, (f, want)) in cases.iter().enumerate() {
        for col in ["E", "F", "G"] {
            assert_eq!(&t.get(&format!("{col}{}", i + 1)), want, "{col}{}: {f}", i + 1);
        }
    }
    // An edit to the table: the next recalculation searches the new values.
    t.set("A3", "Moved");
    assert_eq!(t.num("E1"), 5.0);
    assert_eq!(t.num("G1"), 5.0);
}

#[test]
fn a_spill_landing_in_a_shared_range_replaces_it() {
    // F1:F3 read G2:G100, which G1's spill fills during the same recalculation; a range shared
    // before the spill landed must not be reused after it.
    let mut t = T::new();
    t.load([("G1".to_string(), "=SEQUENCE(100)".to_string())].into_iter().chain((1..=3).map(|r| (format!("F{r}"), "=SUM(G2:G100)".to_string()))));
    for r in 1..=3 {
        assert_eq!(t.num(&format!("F{r}")), 5049.0);
    }
}

/// A workbook of ~3,000 rows mixing what multi-threaded levels must get right: lookups over a
/// shared table, INDIRECT and a defined name reading cells of the same level (so workers hand
/// formulas back), spills and their readers, running sums, text, a cycle.
fn mixed_workbook(threads: u32) -> T {
    let n = 3_000;
    let mut t = T::new();
    t.wb.calc.threads = threads;
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Base".into(),
        scope: None,
        formula: "Sheet1!$B$7".into(),
        comment: String::new(),
        hidden: false,
    });
    let mut cells: Vec<(String, String)> = Vec::new();
    for r in 1..=n {
        cells.push((format!("A{r}"), format!("={}", (r * 37) % 1000)));
        cells.push((format!("B{r}"), format!("=A{r}*2+ROW()")));
        cells.push((format!("C{r}"), format!("=VLOOKUP({},$A$1:$B${n},2,FALSE)", (r * 37) % 1000)));
        cells.push((format!("D{r}"), "=INDIRECT(\"B\"&ROW())+Base".to_string()));
        cells.push((format!("E{r}"), format!("=IF(MOD(A{r},2)=0,\"even\"&A{r},LEN(\"odd\"&B{r}))")));
        cells.push((format!("F{r}"), if r == 1 { "=B1".to_string() } else { format!("=F{}+B{r}", r - 1) }));
        cells.push((format!("G{r}"), format!("=SUM($B$1:$B${n})/ROW()")));
    }
    cells.push(("J1".to_string(), "=SEQUENCE(50,1,A3)".to_string()));
    cells.push(("K1".to_string(), "=SUM(J1#)+COUNT(J2:J50)".to_string()));
    cells.push(("L1".to_string(), "=L2+1".to_string()));
    cells.push(("L2".to_string(), "=L1+1".to_string()));
    t.load(cells);
    t
}

#[test]
fn multi_threaded_recalc_matches_one_thread() {
    let one = mixed_workbook(1);
    let many = mixed_workbook(8);
    let (a, b) = (one.wb.sheet(0).unwrap(), many.wb.sheet(0).unwrap());
    let mut cells = 0;
    for (c, cell) in a.cells.iter() {
        assert_eq!(cell.value, b.value(c), "{}", c.a1());
        cells += 1;
    }
    assert_eq!(cells, b.cells.iter().count());
    assert_eq!(a.spill, b.spill);
    // Spot checks against values worked out by hand.
    assert_eq!(one.num("D10"), one.num("B10") + one.num("B7"));
    assert_eq!(one.num("C5"), one.num("B5"));
    assert_eq!(one.num("K1"), (0..50).map(|i| f64::from(i) + one.num("A3")).sum::<f64>() + 49.0);
    assert_eq!(many.get("L1"), Value::Error(CellError::Circ));
    // An edit recalculates thousands of formulas in large levels again.
    let mut many = many;
    let mut one = one;
    many.set("A1", "=999");
    one.set("A1", "=999");
    for (c, cell) in one.wb.sheet(0).unwrap().cells.iter() {
        assert_eq!(cell.value, many.wb.sheet(0).unwrap().value(c), "after edit {}", c.a1());
    }
}

#[test]
fn thread_count_follows_the_settings() {
    let mut s = gridcraft_model::CalcSettings { threads: 3, ..Default::default() };
    assert_eq!(crate::recalc::thread_count(&s), if cfg!(target_arch = "wasm32") { 1 } else { 3 });
    s.multi_threaded = false;
    assert_eq!(crate::recalc::thread_count(&s), 1);
    s.multi_threaded = true;
    s.threads = 0;
    assert!(crate::recalc::thread_count(&s) >= 1);
}

#[test]
fn clearing_a_far_blocker_unblocks_a_spill() {
    // The blocker is 79 rows below the anchor: further than the 64-cell window typed edits look
    // at, so only the index of blocked anchors finds it when the cell is cleared.
    let mut t = T::new();
    t.set("A1", "=SEQUENCE(100)");
    t.set("A80", "x");
    assert_eq!(t.get("A1"), Value::Error(CellError::Spill));
    t.set("A80", "");
    assert_eq!((t.num("A1"), t.num("A80")), (1.0, 80.0));
    // Blocked again, then the graph rebuilt from the workbook (as undo does): the index is
    // rebuilt from the cells' values.
    t.set("A90", "y");
    assert_eq!(t.get("A1"), Value::Error(CellError::Spill));
    t.calc.rebuild(&t.wb);
    t.set("A90", "");
    assert_eq!(t.num("A90"), 90.0);
}
