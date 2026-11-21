//! Defined names in recalculation: names as static precedents of the formulas that read them, and
//! the per-pass name cache and its limits.

use gridcraft_core::RangeRef;

use super::*;
use crate::eval::Area;

// Helpers.

/// Asserts that the formula at `at` on the first sheet is static and reads A1 and nothing else.
fn resolves_to_a1(t: &T, at: &str) {
    assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} resolves statically");
    assert_eq!(t.calc.graph.precedents_of((0, c(at))), vec![Area { sheet: 0, range: RangeRef::cell(c("A1")) }], "{at}");
}

/// A name of 1,024 areas (`wide_name`, four names of 256 single cells in C1:C1024) read from A1
/// down `wide` rows, and with `single` a one-cell name read from B1; the readers entered through
/// a full build, or with `at_once` in one edit (a paste) after it.
fn wide_readers(wide: u32, single: bool, at_once: bool) -> (T, Vec<(String, &'static str)>) {
    let mut t = T::new();
    for r in 1..=1024 {
        t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("C{r}")), Cell::value(Value::number(1.0)));
    }
    let union = |start: u32| format!("({})", (start..start + 256).map(|r| format!("Sheet1!$C${r}")).collect::<Vec<_>>().join(","));
    for (i, word) in ["first", "second", "third", "fourth"].iter().enumerate() {
        define(&mut t, &format!("{word}_part"), None, &union(i as u32 * 256 + 1));
    }
    define(&mut t, "wide_name", None, "SUM(first_part)+SUM(second_part)+SUM(third_part)+SUM(fourth_part)");
    define(&mut t, "one_cell", None, "Sheet1!$C$1");
    if at_once {
        t.calc.recalc_all(&mut t.wb);
    }
    let mut readers: Vec<(String, &str)> = (1..=wide).map(|r| (format!("A{r}"), "=wide_name")).collect();
    if single {
        readers.push(("B1".into(), "=one_cell"));
    }
    for (at, formula) in &readers {
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new(formula)));
    }
    if at_once {
        let keys: Vec<_> = readers.iter().map(|(at, _)| (0, c(at))).collect();
        t.calc.cells_changed(&mut t.wb, &keys);
    } else {
        t.calc.recalc_all(&mut t.wb);
    }
    (t, readers)
}

/// Sets a cell on sheet `sheet`, as `T::set` does on the first.
fn set_on(t: &mut T, sheet: usize, at: &str, input: &str) {
    let cell = if input.starts_with('=') {
        Cell::formula(Formula::new(input))
    } else {
        Cell::value(gridcraft_core::parse::parse_input(input, t.wb.date_system).value)
    };
    t.wb.sheet_mut(sheet).unwrap().cells.set(c(at), cell);
    t.calc.cells_changed(&mut t.wb, &[(sheet, c(at))]);
}

fn define(t: &mut T, name: &str, scope: Option<usize>, formula: &str) {
    t.wb.names.push(gridcraft_model::DefinedName { name: name.into(), scope, formula: formula.into(), comment: String::new(), hidden: false });
}

/// The deepest of the definitions `make(inner, k)` that the parser accepts, by `k`.
fn deepest(inner: &str, make: fn(&str, usize) -> String) -> String {
    let (mut low, mut high) = (0, gridcraft_formula::MAX_FORMULA_LEN);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if gridcraft_formula::parse_name(&make(inner, middle)).is_ok() {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    make(inner, low)
}

/// Four names of 256 single cells each (C1:C1024), and `quarters`, a LAMBDA whose body reads all
/// four and its argument: 1,024 areas.
fn quarters(t: &mut T) {
    for r in 1..=1024 {
        t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("C{r}")), Cell::value(Value::number(1.0)));
    }
    for (i, word) in ["first", "second", "third", "fourth"].iter().enumerate() {
        let start = i * 256 + 1;
        define(
            t,
            &format!("{word}_quarter"),
            None,
            &format!("({})", (start..start + 256).map(|r| format!("Sheet1!$C${r}")).collect::<Vec<_>>().join(",")),
        );
    }
    define(t, "quarters", None, "LAMBDA(given,SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)+given)");
}

// Resolution: a name's cells are the formula's precedents.

#[test]
fn names_are_static_precedents_not_dynamic() {
    // A formula that reads a defined name (a range, or a LAMBDA whose body reads a range) depends
    // on the cells the name resolves to, like a plain reference: it is NOT re-evaluated on every
    // recalc, and it IS re-evaluated when those cells change, so a column of LAMBDA calls is not
    // recomputed on every edit anywhere.
    let mut t = T::new();
    for (r, v) in ["10", "20", "30"].iter().enumerate() {
        t.set(&format!("A{}", r + 1), v);
    }
    for def in [("Keys", "Sheet1!$A$1:$A$3"), ("Pick", "LAMBDA(k,XLOOKUP(k,Keys,Keys,\"none\"))"), ("Deep", "LAMBDA(k,Pick(k))")] {
        define(&mut t, def.0, None, def.1);
    }
    t.calc.rebuild(&t.wb);
    t.set("C1", "=SUM(Keys)");
    t.set("C2", "=Pick(20)");
    t.set("C3", "=Deep(30)");
    t.set("C4", "=LET(x, Pick(20), x * 2)");
    assert_eq!(t.num("C1"), 60.0);
    assert_eq!(t.num("C2"), 20.0);
    assert_eq!(t.num("C3"), 30.0);
    assert_eq!(t.num("C4"), 40.0);
    for at in ["C1", "C2", "C3", "C4"] {
        assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} should be a static dependency of A1:A3");
    }
    // A LAMBDA parameter is a bound name, not a lookup: it must not make the call dynamic or
    // #NAME?. Unknown names and INDIRECT still do.
    t.set("D1", "=INDIRECT(\"A1\")");
    t.set("D2", "=NoSuchName+1");
    assert!(t.calc.graph.is_dynamic((0, c("D1"))));
    assert!(t.calc.graph.is_dynamic((0, c("D2"))));
    // Editing a cell the names resolve to re-evaluates the readers...
    t.set("A2", "25");
    assert_eq!(t.num("C1"), 65.0);
    assert_eq!(t.get("C2"), Value::text("none"));
    assert_eq!(t.get("C4"), Value::Error(CellError::Value), "\"none\" * 2");
    t.set("A3", "35");
    assert_eq!(t.get("C3"), Value::text("none"));
    // ...and editing an unrelated cell does not: only the two dynamic formulas (INDIRECT, and the
    // unknown name) are calculated again.
    t.set("F9", "1");
    assert_eq!(t.calc.last_recalc_cells, 2);
}

#[test]
fn every_reader_of_a_dynamic_name_stays_dynamic() {
    // Two formulas read a name built on INDIRECT in one build: the second finds the name already
    // resolved, and must take its dynamic flag with it, or it would go stale.
    let mut t = T::new();
    define(&mut t, "Moving", None, "INDIRECT(\"Sheet1!A1\")");
    for at in ["B1", "B2"] {
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new("=Moving")));
    }
    t.calc.recalc_all(&mut t.wb);
    assert!(t.calc.graph.is_dynamic((0, c("B1"))) && t.calc.graph.is_dynamic((0, c("B2"))));
    t.set("A1", "5");
    assert_eq!((t.num("B1"), t.num("B2")), (5.0, 5.0));
}

#[test]
fn a_name_with_relative_references_stays_dynamic_and_follows_its_cell() {
    // A relative reference in a name is read relative to the cell using it, so what it reads isn't
    // known from the definition: the reader stays dynamic. `Below` (A2 as defined from A1) used
    // in B1 reads B2.
    let mut t = T::new();
    define(&mut t, "Below", None, "Sheet1!A2");
    t.set("B2", "5");
    t.set("B1", "=Below");
    assert_eq!(t.num("B1"), 5.0);
    assert!(t.calc.graph.is_dynamic((0, c("B1"))));
    t.set("B2", "6");
    assert_eq!(t.num("B1"), 6.0);
}

#[test]
fn repeated_references_are_stored_once() {
    // A formula mentioning a cell three times depends on it once, and a name whose definition
    // mentions one cell 1,100 times contributes it once: within the 1,024-area cap, static.
    let mut t = T::new();
    t.set("A1", "2");
    t.set("B1", "=A1+A1+A1");
    assert_eq!(t.calc.graph.precedents_of((0, c("B1"))).len(), 1);
    define(&mut t, "same_cell", None, &vec!["$A$1"; 1100].join("+"));
    t.calc.rebuild(&t.wb);
    t.set("B2", "=same_cell");
    assert_eq!(t.num("B2"), 2200.0);
    assert!(!t.calc.graph.is_dynamic((0, c("B2"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("B2"))).len(), 1);
}

#[test]
fn repeated_references_keep_the_first_of_each_in_order() {
    // A formula's repeats are removed by comparison in a short list (up to 16 areas) and through a
    // set in a longer one: either way the first of each area stays, in the order written.
    let t = T::new();
    let cell = |at: &str| Area { sheet: 0, range: RangeRef::cell(c(at)) };
    let short = gridcraft_formula::parse("=SUM(C1,A1,C1,B1,A1)").unwrap();
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &short);
    assert!(!dynamic);
    assert_eq!(areas, vec![cell("C1"), cell("A1"), cell("B1")]);
    // Twenty cells of row 1, then three of them again: 23 areas, past the short list.
    let row: Vec<String> = (b'A'..=b'T').map(|col| format!("{}1", col as char)).collect();
    let long = gridcraft_formula::parse(&format!("=SUM({},C1,A1,T1)", row.join(","))).unwrap();
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &long);
    assert!(!dynamic);
    assert_eq!(areas, row.iter().map(|at| cell(at)).collect::<Vec<_>>());
}

// Scope: which name a formula reads, and on which sheet.

#[test]
fn sheet_scoped_names_used_from_another_sheet_are_dynamic() {
    // A LAMBDA's body runs against the calling cell's sheet. A sheet-scoped LAMBDA name passed by
    // name from another sheet (`MAP(5,Sheet1!Markup)` on Data) reads Data!A1, while its references
    // resolve against Sheet1 when the graph is built: a static edge to Sheet1!A1 would leave the
    // cell stale when Data!A1 changes. Used on its own sheet it stays static.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    define(&mut t, "Markup", Some(0), "LAMBDA(x,x+$A$1)");
    t.set("A1", "10");
    t.set("B1", "=Markup(1)");
    let data = |t: &mut T, at: &str, input: &str| {
        let cell = if input.starts_with('=') {
            Cell::formula(Formula::new(input))
        } else {
            Cell::value(gridcraft_core::parse::parse_input(input, t.wb.date_system).value)
        };
        t.wb.sheet_mut(1).unwrap().cells.set(c(at), cell);
        t.calc.cells_changed(&mut t.wb, &[(1, c(at))]);
    };
    data(&mut t, "A1", "1000");
    data(&mut t, "B1", "=MAP(5,Sheet1!Markup)");
    assert_eq!(t.num("B1"), 11.0);
    assert!(!t.calc.graph.is_dynamic((0, c("B1"))));
    assert!(t.calc.graph.is_dynamic((1, c("B1"))));
    let data_b1 = |t: &T| t.wb.sheet(1).unwrap().value(c("B1"));
    assert_eq!(data_b1(&t), Value::number(1005.0));
    data(&mut t, "A1", "2000");
    assert_eq!(data_b1(&t), Value::number(2005.0));
    // It stores no precedent: Sheet1!A1, where the name's references resolve, isn't what it reads,
    // and Trace Precedents would point there.
    assert!(t.calc.graph.precedents_of((1, c("B1"))).is_empty());
}

#[test]
fn a_workbook_name_resolves_on_each_sheet_that_uses_it() {
    // An unqualified reference in a workbook-wide name resolves on the sheet of the cell using it,
    // so one graph build resolving the name from two sheets keeps an answer per sheet.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    define(&mut t, "Base", None, "$A$1");
    for sheet in [0, 1] {
        t.wb.sheet_mut(sheet).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=Base")));
    }
    t.calc.recalc_all(&mut t.wb);
    assert!(!t.calc.graph.is_dynamic((1, c("B1"))));
    set_on(&mut t, 1, "A1", "5");
    assert_eq!(t.wb.sheet(1).unwrap().value(c("B1")), Value::number(5.0), "Data!B1 follows Data!A1");
    t.set("A1", "3");
    assert_eq!(t.num("B1"), 3.0, "Sheet1!B1 follows Sheet1!A1");
    assert_eq!(t.wb.sheet(1).unwrap().value(c("B1")), Value::number(5.0));
}

#[test]
fn a_name_qualified_by_a_sheet_with_spaces_or_a_bang_follows_its_own_cells() {
    // The parser stores `' Sales '!Data` as ` Sales !Data`: the walk of precedents splits it as the
    // evaluator does, at the last `!` with the sheet as is. On sheet `Sales`, `' Sales '!Data` is
    // the other sheet's name (dynamic, as any name scoped to another sheet), not `Sales`'s own
    // `Data`, whose cell would leave the reader stale; on sheet `Profit!Loss`,
    // `'Profit!Loss'!Total` is its own name and resolves statically rather than going dynamic.
    let mut t = T::new();
    for (sheet, name) in [("Sales", "Data"), (" Sales ", "Data"), ("Profit!Loss", "Total")] {
        t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new(sheet)));
        let scope = t.wb.sheets.len() - 1;
        define(&mut t, name, Some(scope), "$A$1");
        t.wb.sheet_mut(scope).unwrap().cells.set(c("A1"), Cell::value(Value::number(scope as f64)));
    }
    t.wb.sheet_mut(1).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=' Sales '!Data")));
    t.wb.sheet_mut(3).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("='Profit!Loss'!Total")));
    t.calc.recalc_all(&mut t.wb);
    let b1 = |t: &T, sheet: usize| t.wb.sheet(sheet).unwrap().value(c("B1"));
    assert_eq!((b1(&t, 1), b1(&t, 3)), (Value::number(2.0), Value::number(3.0)));
    assert!(!t.calc.graph.is_dynamic((3, c("B1"))), "'Profit!Loss'!Total resolves statically on its own sheet");
    set_on(&mut t, 2, "A1", "20");
    assert_eq!(b1(&t, 1), Value::number(20.0), "Sales!B1 follows ' Sales '!A1");
    set_on(&mut t, 3, "A1", "30");
    assert_eq!(b1(&t, 3), Value::number(30.0), "'Profit!Loss'!B1 follows 'Profit!Loss'!A1");
}

#[test]
fn a_sheet_name_comes_before_a_workbook_name_and_the_first_of_two_spellings_wins() {
    // The workbook-wide `Rate` is listed first, the sheet's own second: a formula on the sheet reads
    // the sheet's own, as Workbook::name finds it. Two workbook-wide names of one spelling: the
    // first one listed. The graph and the pass agree, and each reader follows its cell.
    let mut t = T::new();
    t.set("A1", "1");
    t.set("A2", "2");
    t.set("A3", "3");
    define(&mut t, "Rate", None, "Sheet1!$A$2");
    define(&mut t, "Rate", Some(0), "Sheet1!$A$1");
    define(&mut t, "Twice", None, "Sheet1!$A$3");
    define(&mut t, "twice", None, "Sheet1!$A$2");
    t.wb.sheet_mut(0).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=Rate")));
    t.wb.sheet_mut(0).unwrap().cells.set(c("B2"), Cell::formula(Formula::new("=TWICE")));
    t.calc.recalc_all(&mut t.wb);
    assert_eq!((t.num("B1"), t.num("B2")), (1.0, 3.0));
    t.set("A1", "10");
    t.set("A3", "30");
    assert_eq!((t.num("B1"), t.num("B2")), (10.0, 30.0));
    t.set("A2", "20");
    assert_eq!((t.num("B1"), t.num("B2")), (10.0, 30.0), "the names not read change nothing");
}

#[test]
fn a_sheet_name_with_apostrophes_is_resolved_as_the_evaluator_reads_it() {
    // A file may name sheets `x` and `x'`. `'x'''!Total`, read from `x`, is the `Total` of `x'`,
    // a name scoped to another sheet, so the formula is dynamic and follows that sheet's A1, as
    // the evaluator reads it; stripping every apostrophe would make it `x`'s own `Total`, static
    // and stale.
    let mut t = T::new();
    t.wb.sheets = vec![std::sync::Arc::new(gridcraft_model::Sheet::new("x")), std::sync::Arc::new(gridcraft_model::Sheet::new("x'"))];
    define(&mut t, "Total", Some(0), "$A$1");
    define(&mut t, "Total", Some(1), "$A$1");
    t.wb.sheet_mut(0).unwrap().cells.set(c("A1"), Cell::value(Value::number(1.0)));
    t.wb.sheet_mut(1).unwrap().cells.set(c("A1"), Cell::value(Value::number(5.0)));
    t.wb.sheet_mut(0).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("='x'''!Total")));
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("B1"), 5.0);
    t.wb.sheet_mut(1).unwrap().cells.set(c("A1"), Cell::value(Value::number(6.0)));
    t.calc.cells_changed(&mut t.wb, &[(1, c("A1"))]);
    assert_eq!(t.num("B1"), 6.0);
}

// Recursion and cycles.

#[test]
fn name_chains_and_cycles_stay_bounded() {
    // Names referring to names are followed to a depth bound; a cycle adds nothing and does not
    // hang the graph build.
    let mut t = T::new();
    t.set("A1", "7");
    define(&mut t, "Loop_first", None, "Loop_second+1");
    define(&mut t, "Loop_second", None, "Loop_first+1");
    define(&mut t, "Base", None, "Sheet1!$A$1");
    t.calc.rebuild(&t.wb);
    t.set("B1", "=Loop_first");
    t.set("B2", "=Base*2");
    assert_eq!(t.num("B2"), 14.0);
    assert!(!t.calc.graph.is_dynamic((0, c("B2"))));
    // The cycle can't be resolved to cells: evaluated every time, which reports it.
    assert!(t.calc.graph.is_dynamic((0, c("B1"))));
    t.set("A1", "8");
    assert_eq!(t.num("B2"), 16.0);
    // A chain is followed through NAME_DEPTH (8) names: a formula eight names from its cell
    // resolves to that cell, one nine names away stays dynamic when resolved on its own (a build
    // that resolved Tier_eight first would answer it from the cache), and both give the right value and
    // follow the cell.
    let words = ["one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
    for (i, w) in words.iter().enumerate() {
        let formula = if i == 0 { "Sheet1!$A$1".to_string() } else { format!("Tier_{}+0", words[i - 1]) };
        define(&mut t, &format!("Tier_{w}"), None, &formula);
    }
    t.calc.rebuild(&t.wb);
    t.set("B3", "=Tier_eight");
    t.set("B4", "=Tier_nine");
    assert_eq!(t.num("B3"), 8.0);
    assert_eq!(t.num("B4"), 8.0);
    assert!(!t.calc.graph.is_dynamic((0, c("B3"))), "eight names deep resolves to the cell");
    let p = t.calc.graph.precedents_of((0, c("B3")));
    assert_eq!(p.len(), 1, "{p:?}");
    assert!(p.contains(&Area { sheet: 0, range: RangeRef::cell(c("A1")) }), "{p:?}");
    assert!(t.calc.graph.is_dynamic((0, c("B4"))), "nine names deep is past the bound");
    t.set("A1", "9");
    assert_eq!(t.num("B3"), 9.0);
    assert_eq!(t.num("B4"), 9.0);
}

#[test]
fn recursive_lambda_names_stay_static() {
    // A LAMBDA name calling itself reads what its body reads, so its callers are static
    // dependencies of the argument and body cells, as directly, through a wrapper name, or at the
    // end of a chain whose self-call lands on the depth bound. Two names calling each other are a
    // cycle and stay dynamic; so does nothing else here.
    let mut t = T::new();
    let mut defs = vec![
        ("SumTo".to_string(), "LAMBDA(n,IF(n<=0,0,Sheet1!$C$1+SumTo(n-1)))".to_string()),
        ("Ping".into(), "LAMBDA(n,IF(n<=0,0,Pong(n-1)))".into()),
        ("Pong".into(), "LAMBDA(n,IF(n<=0,0,Ping(n-1)))".into()),
        ("Self".into(), "Self+Sheet1!$C$1".into()),
        ("Wrap_one".into(), "LAMBDA(n,SumTo(n))".into()),
    ];
    // Wrap_seven → … → Wrap_one → SumTo: SumTo is entered at depth 7, its self-call meets it at
    // the bound.
    let words = ["one", "two", "three", "four", "five", "six", "seven"];
    for i in 1..words.len() {
        defs.push((format!("Wrap_{}", words[i]), format!("LAMBDA(n,Wrap_{}(n))", words[i - 1])));
    }
    for (name, formula) in defs {
        define(&mut t, &name, None, &formula);
    }
    t.calc.rebuild(&t.wb);
    t.set("C1", "2");
    t.set("A2", "3");
    t.set("B1", "=SumTo(A2)");
    t.set("B2", "=Wrap_one(A2)");
    t.set("B3", "=Wrap_seven(A2)");
    t.set("B4", "=Ping(3)");
    t.set("B5", "=Self");
    for at in ["B1", "B2", "B3"] {
        assert_eq!(t.num(at), 6.0, "{at}");
    }
    assert_eq!(t.num("B4"), 0.0);
    // A name referring to itself without a LAMBDA never settles: the depth limit's `#NUM!`,
    // whatever C1 holds.
    assert_eq!(t.get("B5"), Value::Error(CellError::Num));
    let check_graph = |t: &T| {
        for at in ["B1", "B2", "B3", "B5"] {
            assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} should be static");
        }
        assert!(t.calc.graph.is_dynamic((0, c("B4"))), "mutual recursion stays dynamic");
        let p = t.calc.graph.precedents_of((0, c("B1")));
        assert_eq!(p.len(), 2, "{p:?}");
        for at in ["A2", "C1"] {
            assert!(p.contains(&Area { sheet: 0, range: RangeRef::cell(c(at)) }), "{at} in {p:?}");
        }
    };
    check_graph(&t);
    // One build resolving every formula through a shared cache gives the same answers.
    t.calc.rebuild(&t.wb);
    check_graph(&t);
    // The argument cell and the cell the body reads both recalculate the callers.
    t.set("A2", "4");
    for at in ["B1", "B2", "B3"] {
        assert_eq!(t.num(at), 8.0, "{at}");
    }
    t.set("C1", "5");
    for at in ["B1", "B2", "B3"] {
        assert_eq!(t.num(at), 20.0, "{at}");
    }
}

// Deep input: nesting and long chains, on small stacks.

#[test]
fn deep_formulas_at_the_parsers_limits_resolve_on_a_small_stack() {
    // One formula as deep as the parser allows resolves to its cells on a 1 MiB thread (the
    // Windows main thread): the walk keeps its own stack of steps.
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let mut t = T::new();
            t.set("A1", "7");
            // One formula nested as deep as the parser allows, each layer a parenthesis and an
            // operator. The evaluator's own depth limit answers the 250-layer one, so its value isn't
            // the point.
            t.set("B3", &format!("={}A1{}", "(".repeat(250), "+1)".repeat(250)));
            resolves_to_a1(&t, "B3");
            t.set("B4", &format!("={}A1{}", "(".repeat(129), "+1)".repeat(129)));
            assert_eq!(t.num("B4"), 136.0);
            resolves_to_a1(&t, "B4");
            // The shapes the parser counts without a level of their own (a postfix `%`, an invoked
            // result, a trim reference, a computed range, the spill operator and an array constant's
            // elements), at the parser's limit with one of them at the innermost or outermost point.
            // The parser also bounds the parentheses, unary and postfix operators and invocations
            // stacked on one path at 256 (#3813), so B5, B7 and B8 take as many parentheses as their
            // `%`s, trims and invocations leave.
            let parens = |inner: &str, n: usize| format!("{}{inner}{}", "(".repeat(n), ")".repeat(n));
            for (at, formula) in [
                ("B5", format!("={}%%", parens("A1", 254))),
                ("B6", format!("={}:B1:(C1)", parens("A1", 255))),
                ("B7", format!("=({}:.B5):.C5", parens("A1", 253))),
                ("B8", format!("=LAMBDA(x,{})(A1)(1)(1)", parens("x", 253))),
                ("B9", format!("={}", parens("A1#", 255))),
                ("B10", format!("=SUM(A1,{})", parens("{1,2}", 254))),
            ] {
                t.set(at, &formula);
                assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} resolves statically");
                assert!(!t.calc.graph.precedents_of((0, c(at))).is_empty(), "{at} has a precedent");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn deep_chains_of_names_resolve_on_a_small_stack() {
    // The walk keeps its own stack of steps and recurses only from one name into the next, so
    // nesting adds up across a chain of names without a bound: each formula below resolves to the
    // cell at the bottom, on a 1 MiB thread (the Windows main thread), with the right value.
    const WORDS: [&str; 8] = ["one", "two", "three", "four", "five", "six", "seven", "eight"];
    fn deep(levels: usize, inner: &str) -> String {
        format!("{}{}{}", "SUM(".repeat(levels), inner, ")".repeat(levels))
    }
    fn chain(t: &mut T, prefix: &str, count: usize, levels: usize) {
        for i in 0..count {
            let inner = if i == 0 { "Sheet1!$A$1".to_string() } else { format!("{prefix}_{}", WORDS[i - 1]) };
            define(t, &format!("{prefix}_{}", WORDS[i]), None, &deep(levels, &inner));
        }
        t.calc.rebuild(&t.wb);
    }
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let mut t = T::new();
            t.set("A1", "7");
            // Two names whose nesting adds up to 255 levels, and two whose nesting adds up to 256.
            for (prefix, outer, inner) in [("Edge", 127, 126), ("Past", 127, 127)] {
                define(&mut t, &format!("{prefix}_in"), None, &deep(inner, "Sheet1!$A$1"));
                define(&mut t, &format!("{prefix}_out"), None, &deep(outer, &format!("{prefix}_in")));
            }
            t.calc.rebuild(&t.wb);
            t.set("C1", "=Edge_out");
            t.set("C2", "=Past_out");
            assert_eq!((t.num("C1"), t.num("C2")), (7.0, 7.0));
            resolves_to_a1(&t, "C1");
            resolves_to_a1(&t, "C2");
            // 4 × 60 = 240 levels through four names, and 8 × 40 = 320 through eight.
            chain(&mut t, "Near", 4, 60);
            t.set("B1", "=Near_four");
            chain(&mut t, "Far", 8, 40);
            t.set("B2", "=Far_eight");
            assert_eq!((t.num("B1"), t.num("B2")), (7.0, 7.0));
            resolves_to_a1(&t, "B1");
            resolves_to_a1(&t, "B2");
            t.set("A1", "8");
            assert_eq!((t.num("B1"), t.num("B2")), (8.0, 8.0));
            // 8 × 250: a definition as deep as the parser allows, eight times over.
            chain(&mut t, "Vast", 8, 250);
            t.set("B1", "=Vast_eight");
            resolves_to_a1(&t, "B1");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn eight_names_each_as_deep_as_the_parser_allows_resolve_on_a_small_stack() {
    // Eight names, each a definition as deep or as long as the parser accepts around the next, read
    // by one formula on a 1 MiB thread (the Windows main thread): the walk visits every level of
    // every definition, recursing only from one name into the next, and resolves the formula to
    // the cells at the bottom. The deepest native use is the parse of the last definition on top of
    // seven names; in a mixed chain a trim-reference chain leads to a definition nested as deep as
    // the parser allows.
    // A shape's name, and its definition `k` levels deep around `inner`.
    type Shape = (&'static str, fn(&str, usize) -> String);
    let shapes: [Shape; 8] = [
        ("nested_sum", |inner, k| format!("{}{inner}{}", "SUM(".repeat(k), ")".repeat(k))),
        ("nested_lambda", |inner, k| format!("{}{inner}{}", "LAMBDA(x,".repeat(k), ")".repeat(k))),
        ("nested_let", |inner, k| format!("{}{inner}{}", "LET(x,1,".repeat(k), ")".repeat(k))),
        ("added_layers", |inner, k| format!("{}{inner}{}", "(".repeat(k), "+1)".repeat(k))),
        ("negated", |inner, k| format!("{}{inner}", "-".repeat(k))),
        ("trimmed", |inner, k| format!("{inner}{}", ":.$C$1".repeat(k))),
        ("ranged", |inner, k| format!("{inner}{}", ":($C$1)".repeat(k))),
        ("summed", |inner, k| format!("{inner}{}", "+1".repeat(k))),
    ];
    const WORDS: [&str; 8] = ["one", "two", "three", "four", "five", "six", "seven", "eight"];
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let mut t = T::new();
            t.set("A1", "7");
            t.set("C1", "1");
            // Each shape alone, and a chain taking the shapes in turn.
            let mut chains: Vec<(String, Vec<usize>)> = (0..shapes.len()).map(|k| (shapes[k].0.to_string(), vec![k; 8])).collect();
            chains.push(("mixed".into(), (0..8).map(|level| (level + 5) % shapes.len()).collect()));
            for (prefix, kinds) in &chains {
                for (level, kind) in kinds.iter().enumerate() {
                    let inner = if level == 0 { "Sheet1!$A$1".to_string() } else { format!("{prefix}_{}", WORDS[level - 1]) };
                    let formula = deepest(&inner, shapes[*kind].1);
                    assert!(formula.len() > inner.len() + 250, "{prefix} level {level} is deep: {}", formula.len());
                    define(&mut t, &format!("{prefix}_{}", WORDS[level]), None, &formula);
                }
            }
            t.calc.rebuild(&t.wb);
            for (row, (prefix, kinds)) in chains.iter().enumerate() {
                let at = format!("E{}", row + 1);
                t.set(&at, &format!("={prefix}_eight"));
                assert!(!t.calc.graph.is_dynamic((0, c(&at))), "{prefix}: resolves statically");
                // A range or trim reference anywhere in the chain covers the box from A1 to C1.
                let ranged = kinds.iter().any(|k| matches!(shapes[*k].0, "trimmed" | "ranged"));
                let expected = RangeRef::parse(if ranged { "A1:C1" } else { "A1" }).unwrap();
                assert_eq!(t.calc.graph.precedents_of((0, c(&at))), vec![Area { sheet: 0, range: expected }], "{prefix}");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_deep_run_of_parentheses_through_a_second_name_resolves() {
    // A definition that is a run of 250 parentheses, read through a second name that nests 20 more
    // levels, resolves statically to its cell.
    let mut t = T::new();
    t.set("A1", "7");
    for (name, formula) in [
        ("Wrapped", format!("{}Sheet1!$A$1{}", "(".repeat(250), ")".repeat(250))),
        ("Outer", format!("{}Wrapped{}", "SUM(".repeat(20), ")".repeat(20))),
    ] {
        define(&mut t, name, None, &formula);
    }
    t.calc.rebuild(&t.wb);
    t.set("B1", "=Outer");
    assert!(!t.calc.graph.is_dynamic((0, c("B1"))), "a run of 250 parentheses through a second name resolves statically");
    assert_eq!(t.calc.graph.precedents_of((0, c("B1"))), vec![Area { sheet: 0, range: RangeRef::cell(c("A1")) }]);
}

#[test]
fn nesting_past_what_the_parser_allows_resolves_on_a_small_stack() {
    // Trees built in code, deeper than the parser allows (unary minus, parentheses, `%`, a range
    // whose left operand is a range, an operator chain), resolve to their cell on a 256 KiB thread
    // without overflowing it: the walk's stack use doesn't grow with the nesting.
    use gridcraft_formula::{BinOp, Expr, UnOp};
    // Frees a tree one node at a time: the derived drop recurses once per level, more than the
    // thread's stack holds for these trees.
    fn dismantle(root: Expr) {
        let mut pending = vec![root];
        while let Some(e) = pending.pop() {
            match e {
                Expr::Unary(_, inner) | Expr::Paren(inner) => pending.push(*inner),
                Expr::Binary(_, left, right) => {
                    pending.push(*left);
                    pending.push(*right);
                }
                _ => {}
            }
        }
    }
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            type Wrap = (&'static str, fn(Expr) -> Expr);
            let wraps: [Wrap; 5] = [
                ("unary minus", |e| Expr::Unary(UnOp::Neg, Box::new(e))),
                ("parentheses", |e| Expr::Paren(Box::new(e))),
                ("percent", |e| Expr::Unary(UnOp::Percent, Box::new(e))),
                ("ranges", |e| Expr::Binary(BinOp::Range, Box::new(e), Box::new(Expr::Number(1.0)))),
                ("additions", |e| Expr::Binary(BinOp::Add, Box::new(e), Box::new(Expr::Number(1.0)))),
            ];
            for (kind, wrap) in wraps {
                for levels in [255, 256, 10_000, 100_000] {
                    let mut e = gridcraft_formula::parse("=A1").unwrap();
                    for _ in 0..levels {
                        e = wrap(e);
                    }
                    let (areas, found) = crate::eval::precedents(&Workbook::new(), 0, &e);
                    dismantle(e);
                    assert!(!found, "{levels} levels of {kind}");
                    assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::cell(c("A1")) }], "{levels} levels of {kind}");
                }
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_flat_operator_chain_as_long_as_a_formula_allows_resolves_on_a_small_stack() {
    // `=A1+1+1+…` with 4,096 operands: the walk's stack use doesn't grow with the chain, and the
    // formula resolves to its one cell on a 1 MiB thread (the Windows main thread).
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let mut t = T::new();
            t.set("A1", "1");
            t.set("B1", &format!("=A1{}", "+1".repeat(4_095)));
            assert_eq!(t.num("B1"), 4096.0);
            assert!(!t.calc.graph.is_dynamic((0, c("B1"))));
            assert_eq!(t.calc.graph.precedents_of((0, c("B1"))).len(), 1);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn a_hundred_nested_singles_resolve_statically() {
    // `SINGLE(1+…)` nested 100 deep is within what the parser accepts: the formula depends on A1
    // and nothing else, and follows it.
    let mut t = T::new();
    t.set("A1", "7");
    t.set("B1", &format!("={}A1{}", "SINGLE(1+".repeat(100), ")".repeat(100)));
    assert!(!t.calc.graph.is_dynamic((0, c("B1"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("B1"))), vec![Area { sheet: 0, range: RangeRef::cell(c("A1")) }]);
    assert_eq!(t.num("B1"), 107.0);
    t.set("A1", "8");
    assert_eq!(t.num("B1"), 108.0);
}

// Budgets: areas per name, per formula and per build, definition text per build.

#[test]
fn a_name_of_more_areas_than_the_cap_is_dynamic_and_still_follows_its_cells() {
    // A name's areas are stored for every formula reading it, so a name, or the names one formula
    // mentions, may bring at most MAX_NAME_AREAS (1,024) areas; past it the name or the formula is
    // dynamic. Both sides of the bound, and each reader follows a cell inside the names either way.
    let mut t = T::new();
    for r in 1..=1100 {
        t.set(&format!("C{r}"), "1");
    }
    let union = |rows: std::ops::RangeInclusive<u32>| format!("({})", rows.map(|r| format!("Sheet1!$C${r}")).collect::<Vec<_>>().join(","));
    // Four quarters of 256 single cells each: 1,024 areas.
    for (i, word) in ["first", "second", "third", "fourth"].iter().enumerate() {
        let start = i as u32 * 256 + 1;
        define(&mut t, &format!("{word}_quarter"), None, &union(start..=start + 255));
    }
    define(&mut t, "one_more", None, "Sheet1!$C$1025");
    // One name of exactly 1,024 areas, and one of 1,025.
    define(&mut t, "all_of_them", None, "SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)");
    define(&mut t, "one_too_many", None, "SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)+one_more");
    // A name around the one of 1,025: that one brings nothing and makes it dynamic, so a formula
    // reading it stores only the quarter it adds (without the cap per name, the 1,281 areas would
    // be past the cap per formula and the formula would store none).
    define(&mut t, "around_it", None, "one_too_many*0+SUM(first_quarter)");
    // A definition of 1,025 references of its own, written unqualified so it fits a name's length:
    // past the cap it brings nothing, and a name around it only its own areas (in an IF branch never
    // taken, since a union this long evaluates to #NUM!).
    define(&mut t, "direct_many", None, &format!("({})", (1..=1025).map(|r| format!("$C${r}")).collect::<Vec<_>>().join(",")));
    define(&mut t, "around_direct", None, "IF(TRUE,SUM(first_quarter),SUM(direct_many))");
    // A name mentioned again where its areas were already added adds nothing and counts nothing: a
    // definition mentioning the four quarters and one of them again holds its 1,024 cells, static,
    // and so does a formula mentioning a 1,024-area name twice.
    define(
        &mut t,
        "quarters_and_one_again",
        None,
        "SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)+SUM(first_quarter)",
    );
    t.calc.rebuild(&t.wb);
    t.set("A6", "=quarters_and_one_again");
    assert_eq!(t.num("A6"), 1280.0);
    assert!(!t.calc.graph.is_dynamic((0, c("A6"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("A6"))).len(), 1024);
    t.set("A8", "=all_of_them+all_of_them");
    assert_eq!(t.num("A8"), 2048.0);
    assert!(!t.calc.graph.is_dynamic((0, c("A8"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("A8"))).len(), 1024);
    t.set("A5", "=around_it");
    t.set("A7", "=around_direct");
    assert!(t.calc.graph.is_dynamic((0, c("A7"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("A7"))).len(), 256, "a name around a name of 1,025 references adds only its own areas");
    assert_eq!(t.num("A7"), 256.0);
    t.set("A1", "=SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)");
    t.set("A2", "=SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)+one_more");
    t.set("A3", "=all_of_them");
    t.set("A4", "=one_too_many");
    assert_eq!((t.num("A1"), t.num("A2"), t.num("A3"), t.num("A4")), (1024.0, 1025.0, 1024.0, 1025.0));
    let static_with = |t: &T, at: &str, n: usize| {
        assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} resolves statically");
        assert_eq!(t.calc.graph.precedents_of((0, c(at))).len(), n, "{at}");
    };
    static_with(&t, "A1", 1024);
    static_with(&t, "A3", 1024);
    assert!(t.calc.graph.is_dynamic((0, c("A2"))), "a formula taking 1,025 areas from names is dynamic");
    assert!(t.calc.graph.is_dynamic((0, c("A4"))), "a name of 1,025 areas is dynamic");
    assert!(t.calc.graph.precedents_of((0, c("A4"))).is_empty(), "and stores none of them");
    assert!(t.calc.graph.is_dynamic((0, c("A5"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("A5"))).len(), 256, "a name around it adds only its own areas");
    assert_eq!(t.num("A5"), 256.0);
    t.set("C700", "2");
    assert_eq!((t.num("A1"), t.num("A2"), t.num("A3"), t.num("A4")), (1025.0, 1026.0, 1025.0, 1026.0));
    t.set("C1025", "3");
    assert_eq!((t.num("A2"), t.num("A4")), (1028.0, 1028.0));
}

#[test]
fn a_name_is_resolved_after_the_arguments_of_its_call() {
    // Inside a definition, the areas a called name brings are counted against MAX_NAME_AREAS
    // (1,024) on top of what the definition holds so far, and the call's arguments come first:
    // `calls_quarters` holds C1 from its argument when the 1,024 areas of `quarters` arrive, one
    // past the bound, so it is dynamic and stores none (C1 among them is no exception). A formula
    // calling `quarters` itself counts only the areas it takes from names: 1,024, static.
    let mut t = T::new();
    quarters(&mut t);
    define(&mut t, "calls_quarters", None, "quarters(Sheet1!$C$1)");
    t.calc.rebuild(&t.wb);
    t.set("A1", "=calls_quarters");
    t.set("A2", "=quarters(C1)");
    assert_eq!((t.num("A1"), t.num("A2")), (1025.0, 1025.0));
    assert!(t.calc.graph.is_dynamic((0, c("A1"))), "the argument's area and the name's are one past the bound");
    assert!(t.calc.graph.precedents_of((0, c("A1"))).is_empty());
    assert!(!t.calc.graph.is_dynamic((0, c("A2"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("A2"))).len(), 1024);
    t.set("C5", "2");
    assert_eq!((t.num("A1"), t.num("A2")), (1026.0, 1026.0));
}

#[test]
fn the_formulas_of_one_build_take_at_most_a_million_areas_from_names() {
    // A name's areas are stored for every formula reading it. A name of 1,024 areas read from
    // 1,024 cells stores exactly the build's budget (1,048,576 areas): all static. One area more,
    // from a reader of a one-cell name, or one more reader of the wide name, and one reader is
    // dynamic; every reader still follows a cell inside the name. The same through a full build
    // and through one edit of all the readers at once (a paste).
    for at_once in [false, true] {
        for (wide, single, dynamic) in [(1024u32, false, 0usize), (1024, true, 1), (1025, false, 1)] {
            let (mut t, readers) = wide_readers(wide, single, at_once);
            let found = readers.iter().filter(|(at, _)| t.calc.graph.is_dynamic((0, c(at)))).count();
            assert_eq!(found, dynamic, "{wide} wide readers, one-cell reader {single}, in one edit {at_once}");
            t.set("C1", "2");
            assert!((1..=wide).all(|r| t.num(&format!("A{r}")) == 1025.0), "the wide readers follow C1");
            if single {
                assert_eq!(t.num("B1"), 2.0);
            }
        }
    }
}

#[test]
fn the_build_budget_on_areas_from_names_holds_across_edits() {
    // The build's budget on areas taken from names holds across edits: a full build at exactly the
    // budget, then one more reader entered on its own, is dynamic.
    let (mut t, _) = wide_readers(1024, false, false);
    t.set("B2", "=wide_name");
    assert!(t.calc.graph.is_dynamic((0, c("B2"))), "a reader entered after a build at the budget");
    assert_eq!(t.num("B2"), 1024.0);
    // It counts what the formulas hold, not how often they were entered: a reader entered again
    // gives its areas back first and stays static, and a reader cleared makes room for another.
    let (mut t, _) = wide_readers(1024, false, false);
    for _ in 0..3 {
        t.set("A1", "=wide_name");
    }
    assert!(!t.calc.graph.is_dynamic((0, c("A1"))), "a reader entered again at the budget");
    t.set("A2", "");
    t.set("B2", "=wide_name");
    assert!(!t.calc.graph.is_dynamic((0, c("B2"))), "a reader taking the place of a cleared one");
    t.set("B3", "=wide_name");
    assert!(t.calc.graph.is_dynamic((0, c("B3"))), "one reader more");
    t.set("C1", "2");
    assert_eq!((t.num("A1"), t.num("B2"), t.num("B3")), (1025.0, 1025.0, 1025.0));
    // A rebuild starts the charges afresh: A1 rewritten behind the graph's back (as undo puts back
    // a workbook) and rebuilt, then edited, has only its new charge to give back, so the room it
    // leaves takes exactly one reader.
    let (mut t, _) = wide_readers(1024, false, false);
    t.wb.sheet_mut(0).unwrap().cells.set(c("A1"), Cell::formula(Formula::new("=1")));
    t.calc.recalc_all(&mut t.wb);
    t.set("A1", "=2");
    t.set("B2", "=wide_name");
    t.set("B3", "=wide_name");
    assert!(!t.calc.graph.is_dynamic((0, c("B2"))), "the reader taking A1's place");
    assert!(t.calc.graph.is_dynamic((0, c("B3"))), "one reader more, after a rebuild");
}

#[test]
fn one_build_parses_at_most_four_mebibytes_of_definitions() {
    // A name used from a sheet is parsed for that sheet, so a build parses its definitions once per
    // sheet using them, up to 4 MiB in all; past that, further names are dynamic. 1,024 names of
    // exactly 4,096 characters are the budget: all static. One byte more (a name of one character)
    // or one name more and one is dynamic; every
    // reader follows its cell.
    let build = |names: usize, one_byte: bool| {
        let mut t = T::new();
        t.wb.sheet_mut(0).unwrap().cells.set(c("C10"), Cell::value(Value::number(3.0)));
        let formula = format!("Sheet1!$C$10{}", "+0".repeat(2_042));
        assert_eq!(formula.len(), 4096);
        for k in 0..names {
            define(&mut t, &format!("padded_name_{k}"), None, &formula);
            t.wb.sheet_mut(0).unwrap().cells.set(CellRef::new(k as u32, 0), Cell::formula(Formula::new(&format!("=padded_name_{k}"))));
        }
        if one_byte {
            define(&mut t, "tiny_name", None, "7");
            t.wb.sheet_mut(0).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=tiny_name")));
        }
        t.calc.recalc_all(&mut t.wb);
        t
    };
    for (names, one_byte, expected) in [(1024usize, false, 0usize), (1024, true, 1), (1025, false, 1)] {
        let mut t = build(names, one_byte);
        let dynamic = (0..names).filter(|k| t.calc.graph.is_dynamic((0, CellRef::new(*k as u32, 0)))).count()
            + usize::from(one_byte && t.calc.graph.is_dynamic((0, c("B1"))));
        assert_eq!(dynamic, expected, "{names} names, one byte more {one_byte}");
        t.set("C10", "4");
        assert!((0..names).all(|k| t.wb.sheet(0).unwrap().value(CellRef::new(k as u32, 0)) == Value::number(4.0)), "{names} names follow C10");
    }
}

#[test]
fn the_names_one_build_resolves_keep_at_most_a_million_areas() {
    // A name that only repeats another (`copy_k = wide_name`) is three bytes of text but keeps all
    // the areas of the name it repeats. The four quarters (256 areas each), `wide_name` (1,024),
    // as many copies (1,024 each) as fit and `tail_part`, a union of as many cells as are left,
    // keep exactly the build's budget, 1,048,576 areas, each name counting its entry too
    // (`entry_charge`): all static. One copy more, or a one-cell name read as well, and one reader
    // is dynamic; every reader still follows a cell inside the name.
    let charge = |name: &str| crate::eval::NameCache::entry_charge(name.len());
    let fixed = ["first_part", "second_part", "third_part", "fourth_part", "wide_name"].map(charge).iter().sum::<usize>() + 2 * 1024;
    let (mut copies, mut total) = (0usize, fixed);
    while total + charge(&format!("copy_{copies}")) + 1024 + charge("tail_part") < 1 << 20 {
        total += charge(&format!("copy_{copies}")) + 1024;
        copies += 1;
    }
    let tail = (1 << 20) - total - charge("tail_part");
    assert!((1..=1024).contains(&tail), "{tail} cells left for tail_part");
    let union = |start: u32, count: u32| format!("({})", (start..start + count).map(|r| format!("Sheet1!$C${r}")).collect::<Vec<_>>().join(","));
    let build = |copies: usize, single: bool| {
        let mut t = T::new();
        for r in 1..=1024 {
            t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("C{r}")), Cell::value(Value::number(1.0)));
        }
        for (i, word) in ["first", "second", "third", "fourth"].iter().enumerate() {
            define(&mut t, &format!("{word}_part"), None, &union(i as u32 * 256 + 1, 256));
        }
        define(&mut t, "wide_name", None, "SUM(first_part)+SUM(second_part)+SUM(third_part)+SUM(fourth_part)");
        define(&mut t, "tail_part", None, &union(1, tail as u32));
        define(&mut t, "one_cell", None, "Sheet1!$C$1");
        for k in 0..copies {
            define(&mut t, &format!("copy_{k}"), None, "wide_name");
        }
        let mut readers: Vec<(CellRef, String)> = (0..copies).map(|k| (CellRef::new(k as u32, 0), format!("=copy_{k}"))).collect();
        readers.push((c("B2"), "=SUM(tail_part)".into()));
        if single {
            readers.push((c("B1"), "=one_cell".into()));
        }
        for (at, formula) in &readers {
            t.wb.sheet_mut(0).unwrap().cells.set(*at, Cell::formula(Formula::new(formula)));
        }
        t.calc.recalc_all(&mut t.wb);
        (t, readers)
    };
    for (copies, single, expected) in [(copies, false, 0usize), (copies, true, 1), (copies + 1, false, 1)] {
        let (mut t, readers) = build(copies, single);
        let dynamic = readers.iter().filter(|(at, _)| t.calc.graph.is_dynamic((0, *at))).count();
        assert_eq!(dynamic, expected, "{copies} copies, one-cell reader {single}");
        t.set("C1", "2");
        assert!((0..copies).all(|k| t.wb.sheet(0).unwrap().value(CellRef::new(k as u32, 0)) == Value::number(1025.0)), "the copies follow C1");
        assert_eq!(t.num("B2"), tail as f64 + 1.0, "tail_part follows C1");
        if single {
            assert_eq!(t.num("B1"), 2.0);
        }
    }
}

// Pass cache: names parsed once per build or recalculation pass.

#[test]
fn names_mentioned_many_times_are_resolved_once() {
    // Eight names, each mentioning the previous one 20 times in a branch IF never takes; a cell
    // mentioning the last one 20 times. Followed afresh at every mention that is 20^8 expansions
    // for one formula (a workbook can make the graph build take forever); resolved once per name
    // and build, it is 8. The result is the same: a static dependency on the cell at the bottom.
    let mut t = T::new();
    t.set("A1", "7");
    let mut formula = "Sheet1!$A$1".to_string();
    for word in ["one", "two", "three", "four", "five", "six", "seven", "eight"] {
        define(&mut t, &format!("Rank_{word}"), None, &formula);
        let mentions = vec![format!("Rank_{word}"); 20].join(",");
        formula = format!("IF(TRUE,0,SUM({mentions}))");
    }
    t.calc.rebuild(&t.wb);
    t.set("B1", &format!("={formula}"));
    assert_eq!(t.num("B1"), 0.0);
    assert!(!t.calc.graph.is_dynamic((0, c("B1"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("B1"))), vec![Area { sheet: 0, range: RangeRef::cell(c("A1")) }]);
    let mut deps = Vec::new();
    t.calc.graph.dependents(0, c("A1"), &mut deps);
    assert!(deps.contains(&(0, c("B1"))));
}

#[test]
fn sheet_scoped_names_of_one_spelling_evaluate_per_sheet_in_one_pass() {
    // Two sheets with their own `Rate`: one pass calculating both readers looks each up from its
    // own sheet, whatever it parsed for the other.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    for (scope, value) in [(0, "1"), (1, "2")] {
        define(&mut t, "Rate", Some(scope), value);
        t.wb.sheet_mut(scope).unwrap().cells.set(c("A1"), Cell::formula(Formula::new("=Rate")));
    }
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("A1"), 1.0);
    assert_eq!(t.wb.sheet(1).unwrap().value(c("A1")), Value::number(2.0));
}

#[test]
fn a_one_shot_evaluation_finds_names_as_the_pass_does() {
    // `evaluate` (conditional formats, validation, filter criteria) looks names up without the
    // pass's index; it must pick the same one: the sheet's own `Rate` on Sheet1 though the
    // workbook-wide one is listed first, the workbook-wide one from Data, in any letter case, and
    // the first of two spellings.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.set("A1", "1");
    t.set("A2", "2");
    t.set("A3", "3");
    define(&mut t, "Rate", None, "Sheet1!$A$2");
    define(&mut t, "Rate", Some(0), "Sheet1!$A$1");
    define(&mut t, "Twice", None, "Sheet1!$A$3");
    define(&mut t, "twice", None, "Sheet1!$A$2");
    let at = c("C5");
    assert_eq!(crate::evaluate(&t.wb, 0, at, "=rATE"), Value::number(1.0), "the sheet's own name");
    assert_eq!(crate::evaluate(&t.wb, 1, at, "=rATE"), Value::number(2.0), "the workbook's name");
    assert_eq!(crate::evaluate(&t.wb, 1, at, "=TWICE"), Value::number(3.0), "the first spelling");
}

#[test]
fn a_sheet_scoped_name_read_from_another_sheet_keeps_its_scope_in_the_pass() {
    // `Price` belongs to Sheet1 and reads `$A$1` there. Two cells on Data read `Sheet1!Price`: the
    // second finds the definition already parsed in the pass, and must still read Sheet1's A1, not
    // Data's.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.set("A1", "5");
    t.wb.sheet_mut(1).unwrap().cells.set(c("A1"), Cell::value(Value::number(99.0)));
    define(&mut t, "Price", Some(0), "$A$1");
    for at in ["B1", "B2"] {
        t.wb.sheet_mut(1).unwrap().cells.set(c(at), Cell::formula(Formula::new("=Sheet1!Price")));
    }
    t.calc.recalc_all(&mut t.wb);
    let data = |t: &T, at: &str| t.wb.sheet(1).unwrap().value(c(at));
    assert_eq!((data(&t, "B1"), data(&t, "B2")), (Value::number(5.0), Value::number(5.0)));
}

#[test]
fn a_workbook_name_read_from_many_sheets_resolves_on_each_in_one_pass() {
    // A pass keeps one parsed copy of a workbook-wide name, shared by every sheet that reads it;
    // each sheet's unqualified references still resolve on that sheet.
    let mut t = T::new();
    for s in 1..40 {
        t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new(format!("Other{s}"))));
    }
    define(&mut t, "here", None, "$A$1*2");
    for s in 0..40 {
        t.wb.sheet_mut(s).unwrap().cells.set(c("A1"), Cell::value(Value::number(s as f64)));
        t.wb.sheet_mut(s).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=here")));
    }
    t.calc.recalc_all(&mut t.wb);
    for s in 0..40 {
        assert_eq!(t.wb.sheet(s).unwrap().value(c("B1")), Value::number(2.0 * s as f64), "sheet {s}");
    }
}

#[test]
fn every_use_of_a_name_is_charged_its_text_when_the_pass_keeps_it_parsed() {
    // Each use of a name costs the cell's work the length of its text, as it did when every use
    // parsed it again. The pass parses `Banner` once and keeps it; its uses must still be charged,
    // or a formula's result would depend on whether the name was kept. 8,002 bytes a use against
    // 256M units of work: 30,000 uses fit, 40,000 don't.
    let mut t = T::new();
    define(&mut t, "Banner", None, &format!("\"{}\"", "x".repeat(8_000)));
    t.calc.rebuild(&t.wb);
    t.set("A1", "=SUM(MAP(SEQUENCE(30000),LAMBDA(row,ISTEXT(Banner)*1)))");
    assert_eq!(t.num("A1"), 30_000.0);
    t.set("A2", "=SUM(MAP(SEQUENCE(40000),LAMBDA(row,ISTEXT(Banner)*1)))");
    assert_eq!(t.get("A2"), Value::Error(CellError::Calc));
}

// Range operands and bound names: LET variables, LAMBDA parameters, computed ranges.

#[test]
fn a_computed_range_counts_a_name_the_formula_already_took() {
    // `named_cell` is taken once by the formula, and again inside a range with a computed end, which
    // covers the cells from A1 to what INDEX returns: the range's box still reaches B5.
    let mut t = T::new();
    define(&mut t, "named_cell", None, "Sheet1!$B$5");
    t.set("B5", "2");
    t.set("A1", "1");
    t.set("D1", "=SUM(named_cell)+SUM(A1:INDEX(named_cell,1))");
    assert_eq!(t.num("D1"), 5.0);
    let p = t.calc.graph.precedents_of((0, c("D1")));
    assert!(p.contains(&Area { sheet: 0, range: RangeRef::parse("A1:B5").unwrap() }), "{p:?}");
    t.set("A3", "4");
    assert_eq!(t.num("D1"), 9.0);
}

#[test]
fn a_name_after_a_computed_range_counts_for_the_formula() {
    // `named_cell` is taken inside a range with a computed end first, then by the formula itself:
    // the formula's own list holds B5 as well as the box A1:B5.
    let mut t = T::new();
    define(&mut t, "named_cell", None, "Sheet1!$B$5");
    let e = gridcraft_formula::parse("=SUM(A1:INDEX(named_cell,1))+SUM(named_cell)").unwrap();
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
    assert!(!dynamic);
    assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::parse("A1:B5").unwrap() }, Area { sheet: 0, range: RangeRef::parse("B5").unwrap() }]);
}

#[test]
fn a_range_ending_in_a_let_variable_or_lambda_parameter_follows_the_cells_between() {
    // `$A$1:edge` with `edge` bound to `$B$5` covers A1:B5, as the evaluator reads it: editing A3
    // reaches each reader, whether `edge` is a LET variable, a variable bound to another one, the
    // reference INDEX picks from a LET variable, or a parameter of a LAMBDA invoked in place or of
    // a LAMBDA name. Each is dynamic.
    let mut t = T::new();
    for (at, v) in [("A1", "1"), ("A2", "2"), ("A3", "3"), ("A4", "4"), ("A5", "5"), ("B5", "10")] {
        t.set(at, v);
    }
    define(&mut t, "UpTo", None, "LAMBDA(edge,SUM(Sheet1!$A$1:edge))");
    t.calc.rebuild(&t.wb);
    let readers = [
        ("D1", "=LET(edge,$B$5,SUM($A$1:edge))"),
        ("D2", "=LET(first,$B$5,edge,first,SUM($A$1:(edge)))"),
        ("D3", "=LET(column,$B$1:$B$9,SUM($A$1:INDEX(column,5)))"),
        ("D4", "=LAMBDA(edge,SUM($A$1:edge))($B$5)"),
        ("D5", "=UpTo($B$5)"),
    ];
    for (at, formula) in readers {
        t.set(at, formula);
    }
    let dynamic = |t: &T| readers.map(|(at, _)| t.calc.graph.is_dynamic((0, c(at))));
    assert_eq!(dynamic(&t), [true; 5]);
    let values = |t: &T| readers.map(|(at, _)| t.num(at));
    assert_eq!(values(&t), [25.0; 5]);
    t.set("A3", "30");
    assert_eq!(values(&t), [52.0; 5], "A3 sits inside A1:B5");
}

#[test]
fn bound_names_in_unions_and_intersections_follow_their_cells() {
    // A bound name in a union or an intersection reads no more than its value's areas, which the
    // formula already depends on.
    let mut t = T::new();
    for (at, v) in [("A1", "1"), ("A3", "3"), ("B5", "10")] {
        t.set(at, v);
    }
    let readers = [("J1", "=LET(cell,$B$5,SUM(($A$1,cell)))"), ("J2", "=LET(block,$A$1:$B$5,SUM(block $A$3:$C$3))")];
    for (at, formula) in readers {
        t.set(at, formula);
    }
    let values = |t: &T| readers.map(|(at, _)| t.num(at));
    assert_eq!(values(&t), [11.0, 3.0]);
    t.set("A1", "100");
    t.set("A3", "30");
    t.set("B5", "20");
    assert_eq!(values(&t), [120.0, 30.0]);
}

#[test]
fn a_recursive_name_inside_its_own_range_follows_the_cells_between() {
    // `Grow(1)` returns `$A$1:Grow(0)`, and `Grow(0)` is `$B$5`: the cell sums A1:B5, so editing
    // B1, which neither reference names, reaches it. The self-call sits in a range's operand, so
    // the formula is dynamic.
    let mut t = T::new();
    for (at, v) in [("A1", "1"), ("A2", "2"), ("A3", "3"), ("A4", "4"), ("A5", "5"), ("B5", "10")] {
        t.set(at, v);
    }
    define(&mut t, "Grow", None, "LAMBDA(count,IF(count<=0,$B$5,$A$1:Grow(0)))");
    t.calc.rebuild(&t.wb);
    t.set("D1", "=SUM(Grow(1))");
    assert!(t.calc.graph.is_dynamic((0, c("D1"))));
    assert_eq!(t.num("D1"), 25.0);
    t.set("B1", "7");
    assert_eq!(t.num("D1"), 32.0, "B1 sits inside A1:B5");
}

#[test]
fn a_name_reading_a_variable_its_caller_binds_follows_the_callers_cells() {
    // The evaluator reads `Upward` with the caller's variables in scope, so its `edge` is the
    // caller's `$D$9`, not the name `edge` (`$B$2`): the cell sums A1:D9 and editing C1 reaches
    // it. So too where the caller is a LAMBDA name whose parameter is `edge`, and where a recursive
    // name calls itself inside a LET binding `edge`, and where the caller reads `Upward` through
    // `Onward`, which leaves free the `edge` that `Upward` leaves free. Callers that bind no name
    // `Upward` or `edge` leave free stay static, a LAMBDA name with its own parameter `edge` included.
    let mut t = T::new();
    for (at, v) in [("A1", "1"), ("B2", "2"), ("D9", "4")] {
        t.set(at, v);
    }
    define(&mut t, "edge", None, "$B$2");
    define(&mut t, "Upward", None, "SUM($A$1:edge)");
    define(&mut t, "Through", None, "LAMBDA(edge,Upward)");
    define(&mut t, "Climb", None, "LAMBDA(count,IF(count<=0,SUM($A$1:edge),LET(edge,$D$9,Climb(count-1))))");
    define(&mut t, "Total", None, "LAMBDA(edge,SUM(edge))");
    define(&mut t, "Onward", None, "Upward+0");
    t.calc.rebuild(&t.wb);
    let readers = [
        ("F1", "=LET(edge,$D$9,Upward)"),
        ("F2", "=Through($D$9)"),
        ("F3", "=Climb(1)"),
        ("F4", "=LET(other,$D$9,Upward)"),
        ("F5", "=LET(edge,$D$9,Total(edge))"),
        ("F6", "=LET(edge,$D$9,Onward)"),
    ];
    for (at, formula) in readers {
        t.set(at, formula);
    }
    let dynamic = |t: &T| readers.map(|(at, _)| t.calc.graph.is_dynamic((0, c(at))));
    assert_eq!(dynamic(&t), [true, true, true, false, false, true]);
    let values = |t: &T| readers.map(|(at, _)| t.num(at));
    assert_eq!(values(&t), [7.0, 7.0, 7.0, 3.0, 4.0, 7.0]);
    t.set("C1", "100");
    assert_eq!(values(&t), [107.0, 107.0, 107.0, 3.0, 4.0, 107.0], "C1 sits inside A1:D9, not A1:B2");
}

#[test]
fn a_variable_matching_a_name_in_another_case_of_any_script_is_the_variable() {
    // `Édge` and `édge` are one name to the evaluator, which reads the LET variable (`$D$9`) and
    // not the defined name (`$B$1`): the cell sums A1:D9, so editing C1 reaches it.
    let mut t = T::new();
    for (at, v) in [("A1", "1"), ("B1", "2"), ("D9", "4")] {
        t.set(at, v);
    }
    define(&mut t, "édge", None, "$B$1");
    t.calc.rebuild(&t.wb);
    t.set("F1", "=LET(Édge,$D$9,SUM($A$1:édge))");
    assert!(t.calc.graph.is_dynamic((0, c("F1"))));
    assert_eq!(t.num("F1"), 7.0);
    t.set("C1", "100");
    assert_eq!(t.num("F1"), 107.0, "C1 sits inside A1:D9");
}

#[test]
fn a_let_variable_is_bound_for_the_values_after_it_not_its_own() {
    // `edge` is also a defined name ($B$2). In `corner`'s value, after `edge` is bound, `$A$1:edge`
    // ends at the variable ($D$9): a bound name in a range's operand, dynamic, and the cell sums
    // A1:D9. In `edge`'s own value it is not bound yet: `$A$1:edge` ends at the defined name, a
    // static box A1:B2.
    let mut t = T::new();
    for (at, v) in [("A1", "1"), ("B2", "2"), ("C1", "3"), ("D9", "4")] {
        t.set(at, v);
    }
    define(&mut t, "edge", None, "Sheet1!$B$2");
    t.calc.rebuild(&t.wb);
    t.set("F1", "=LET(edge,$D$9,corner,$A$1:edge,SUM(corner))");
    t.set("F2", "=LET(edge,$A$1:edge,SUM(edge))");
    assert!(t.calc.graph.is_dynamic((0, c("F1"))));
    assert!(!t.calc.graph.is_dynamic((0, c("F2"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("F2"))), vec![Area { sheet: 0, range: RangeRef::parse("A1:B2").unwrap() }]);
    assert_eq!((t.num("F1"), t.num("F2")), (10.0, 3.0));
    t.set("C5", "100");
    assert_eq!((t.num("F1"), t.num("F2")), (110.0, 3.0), "C5 sits inside A1:D9, not A1:B2");
}

#[test]
fn a_let_variable_is_unbound_after_its_let() {
    // After the LET, `edge` is the defined name again: `$A$1:edge` there is the static box A1:B2.
    let mut t = T::new();
    define(&mut t, "edge", None, "Sheet1!$B$2");
    let e = gridcraft_formula::parse("=LET(edge,$D$9,edge)+SUM($A$1:edge)").unwrap();
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
    assert!(!dynamic);
    assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::parse("D9").unwrap() }, Area { sheet: 0, range: RangeRef::parse("A1:B2").unwrap() }]);
}

#[test]
fn an_odd_trailing_let_element_is_read_as_an_expression() {
    // `LET(x,1,trailing,2)` has a name where a value is missing: the walk reads `trailing` as an
    // expression with `x` bound, and `2` as the result. A bound name there reads nothing.
    let mut t = T::new();
    define(&mut t, "trailing", None, "Sheet1!$C$3");
    for (formula, cell) in [("=LET(x,1,trailing,2)", "C3"), ("=LET(x,$B$2,x,2)", "B2")] {
        let e = gridcraft_formula::parse(formula).unwrap();
        let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
        assert!(!dynamic, "{formula}");
        assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::parse(cell).unwrap() }], "{formula}");
    }
}

// Spill references.

#[test]
fn a_range_with_a_spill_reference_in_an_operand_follows_the_cells_between() {
    // `$A$1:Tail` with `Tail` the spill of G1 (`=SEQUENCE(3)`) covers A1:G3, not the box of A1 and
    // G1: editing B2 or M2, which no reference names, reaches each reader whose range covers it,
    // whether the spill reference is in the operand itself, behind a name, a name of a name, a
    // name holding the whole range, INDEX over the spill, or the body of a LAMBDA name. Each is
    // dynamic. A spill reference read whole, alone or in a union or intersection, stays static.
    let mut t = T::new();
    for row in 1..=10 {
        for column in ["A", "B", "C", "D", "E", "F", "H", "M", "Z"] {
            t.set(&format!("{column}{row}"), "1");
        }
    }
    t.set("G1", "=SEQUENCE(3)");
    define(&mut t, "Tail", None, "Sheet1!$G$1#");
    define(&mut t, "Outer", None, "Tail");
    define(&mut t, "Whole", None, "Sheet1!$A$1:Sheet1!$G$1#");
    define(&mut t, "Spanned", None, "Sheet1!$A$1:Outer");
    define(&mut t, "Box", None, "Sheet1!$A$1:Tail");
    define(&mut t, "SumSpill", None, "LAMBDA(x,SUM(Sheet1!$A$1:Sheet1!$G$1#)+x)");
    t.calc.rebuild(&t.wb);
    // A1:G3 holds 6 × 3 ones and 1 + 2 + 3; G1:Z3 holds 1 + 2 + 3 and three ones in each of H, M
    // and Z (the columns between are empty).
    let readers = [
        ("AD1", "=SUM(A1:Tail)", 24.0, 123.0),
        ("AD2", "=SUM(A1:(Tail))", 24.0, 123.0),
        ("AD3", "=SUM(Sheet1!A1:Tail)", 24.0, 123.0),
        ("AD4", "=SUM(Whole)", 24.0, 123.0),
        ("AD5", "=SUM(A1:Outer)", 24.0, 123.0),
        ("AD6", "=SUM(Spanned)", 24.0, 123.0),
        ("AD7", "=SUM((Box,Z1))", 25.0, 124.0),
        ("AD8", "=SUM(A1:INDEX(Tail,ROWS(Tail)))", 24.0, 123.0),
        ("AD9", "=SumSpill(0)", 24.0, 123.0),
        ("AD10", "=SUM(A1:(G1#))", 24.0, 123.0),
        ("AD11", "=SUM(A1:INDEX(G1#,ROWS(G1#)))", 24.0, 123.0),
        ("AD12", "=SUM(Tail:Z1)", 15.0, 114.0),
        ("AD13", "=SUM(G1#:Z1)", 15.0, 114.0),
        ("AD14", "=SUM((G1#):Z1)", 15.0, 114.0),
    ];
    let whole = [("AE1", "=SUM(G1#)", 6.0), ("AE2", "=SUM(Tail)", 6.0), ("AE3", "=SUM((B1:B3,Tail))", 9.0), ("AE4", "=SUM(Tail G1:H9)", 6.0)];
    for (at, formula, ..) in readers {
        t.set(at, formula);
    }
    for (at, formula, _) in whole {
        t.set(at, formula);
    }
    for (at, ..) in readers {
        assert!(t.calc.graph.is_dynamic((0, c(at))), "{at} should be dynamic");
    }
    for (at, ..) in whole {
        assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} should stay static");
    }
    assert_eq!(readers.map(|(at, ..)| t.num(at)), readers.map(|(_, _, before, _)| before));
    assert_eq!(whole.map(|(at, ..)| t.num(at)), whole.map(|(.., value)| value));
    t.set("B2", "100");
    t.set("M2", "100");
    assert_eq!(readers.map(|(at, ..)| t.num(at)), readers.map(|(.., after)| after), "B2 sits inside A1:G3, M2 inside G1:Z3");
}

#[test]
fn a_spill_name_read_whole_and_in_a_range_is_right_in_either_order() {
    // One build resolves `Tail` once and keeps it: the formula reading it whole is static and the
    // one reading it in a range's operand dynamic, whichever comes first.
    let mut t = T::new();
    t.set("G1", "=SEQUENCE(3)");
    define(&mut t, "Tail", None, "Sheet1!$G$1#");
    let whole = gridcraft_formula::parse("=SUM(Tail)").unwrap();
    let ranged = gridcraft_formula::parse("=SUM(A1:Tail)").unwrap();
    let mut names = crate::eval::NameCache::default();
    assert!(!crate::eval::precedents_with(&t.wb, 0, &whole, &mut names).1);
    assert!(crate::eval::precedents_with(&t.wb, 0, &ranged, &mut names).1);
    let mut names = crate::eval::NameCache::default();
    assert!(crate::eval::precedents_with(&t.wb, 0, &ranged, &mut names).1);
    assert!(!crate::eval::precedents_with(&t.wb, 0, &whole, &mut names).1);
}

// Database functions.

#[test]
fn a_name_calling_a_database_function_with_computed_criteria_follows_the_shifted_cells() {
    // A computed criterion (a formula under a label that names no field) is written for the first
    // record and shifted down for each record, so it reads cells no reference in the formula
    // names: here H3 for the second record. A name holding such a call (read by two formulas in
    // one build, the second finding it resolved), a call given its database and criteria as
    // names, and a call in a formula that also reads a name or a LET variable are dynamic, as
    // every formula calling a database function is, so they follow those cells. So are the
    // spellings whose prefix the parser leaves and the evaluator strips (`_xlws._xlfn.DSUM`).
    let mut t = T::new();
    for (at, input) in [("A1", "Amount"), ("A2", "1"), ("A3", "2"), ("C1", "Check"), ("C2", "=$H2>0"), ("H2", "1"), ("H3", "1")] {
        t.set(at, input);
    }
    define(&mut t, "Total", None, "DSUM(Sheet1!$A$1:$A$3,\"Amount\",Sheet1!$C$1:$C$2)");
    define(&mut t, "PrefixedTotal", None, "_xlws._xlfn.DSUM(Sheet1!$A$1:$A$3,\"Amount\",Sheet1!$C$1:$C$2)");
    define(&mut t, "Records", None, "Sheet1!$A$1:$A$3");
    define(&mut t, "Checks", None, "Sheet1!$C$1:$C$2");
    define(&mut t, "Adjustment", None, "0");
    let readers = [
        ("E1", "=Total"),
        ("E2", "=Total"),
        ("E3", "=DSUM(Records,\"Amount\",Checks)"),
        ("E4", "=DSUM($A$1:$A$3,\"Amount\",$C$1:$C$2)+Adjustment"),
        ("E5", "=LET(base,0,DSUM($A$1:$A$3,\"Amount\",$C$1:$C$2)+base)"),
        ("E6", "=PrefixedTotal"),
        ("E7", "=_xlfn._xlfn.DSUM($A$1:$A$3,\"Amount\",$C$1:$C$2)+Adjustment"),
    ];
    for (at, formula) in readers {
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new(formula)));
    }
    t.calc.recalc_all(&mut t.wb);
    let values = |t: &T| readers.map(|(at, _)| t.num(at));
    assert_eq!(values(&t), [3.0; 7]);
    t.set("H3", "-1");
    assert_eq!(values(&t), [1.0; 7], "the second record's criterion reads H3");
}

#[test]
fn a_volatile_function_under_a_prefix_the_parser_leaves_is_dynamic() {
    // The parser strips one `_xlfn.` or `_xlws.` prefix and the evaluator's registry the rest, so
    // `_xlws._xlfn.RAND()` draws a number at each calculation: the formula is dynamic, with a name
    // beside the call and without one.
    let mut t = T::new();
    define(&mut t, "Adjustment", None, "0");
    for formula in ["=_xlws._xlfn.RAND()", "=_xlfn._xlfn.NOW()+Adjustment"] {
        let e = gridcraft_formula::parse(formula).unwrap();
        assert!(crate::eval::precedents(&t.wb, 0, &e).1, "{formula}");
    }
}

#[test]
fn a_formula_calling_a_database_function_follows_the_cells_its_computed_criterion_shifts_to() {
    // The D-function formula mentions no name. Its criteria cell's formula, evaluated once per
    // record with its relative references shifted, reads H3 for the second record: with a name in
    // the criterion (`Limit`), with a LET variable, and with neither. Calling a database function
    // makes the formula dynamic, so editing H3 gives what a full recalculation gives.
    let criteria = ["=$H2>Limit", "=LET(limit,0,$H2>limit)", "=$H2>0"];
    // Each criterion with E1 before and after the edit, and whether E1 is dynamic.
    let seen = criteria.map(|criterion| {
        let mut t = T::new();
        define(&mut t, "Limit", None, "0");
        for (at, input) in [("A1", "Amount"), ("A2", "1"), ("A3", "2"), ("C1", "Check"), ("C2", criterion), ("H2", "1"), ("H3", "1")] {
            t.set(at, input);
        }
        t.wb.sheet_mut(0).unwrap().cells.set(c("E1"), Cell::formula(Formula::new("=DSUM($A$1:$A$3,\"Amount\",$C$1:$C$2)")));
        t.calc.recalc_all(&mut t.wb);
        let before = t.num("E1");
        t.set("H3", "-1");
        (criterion, before, t.num("E1"), t.calc.graph.is_dynamic((0, c("E1"))))
    });
    assert_eq!(seen, criteria.map(|criterion| (criterion, 3.0, 1.0, true)), "the second record's criterion reads H3");
}

#[test]
fn the_build_budget_on_free_names_holds_at_its_limit() {
    // The names one build resolves keep the names their definitions leave free, those of the names
    // they use included, and keep at most MAX_BUILD_FREE_NAMES (1,048,576) of them all together;
    // the name that would go past keeps none and is dynamic. `wide_free` mentions 511 constant
    // names (a definition is at most 8,192 characters), and each of 2,047 names `wide_user_k` =
    // `wide_free+0` keeps those and `wide_free`: 511 + 2,047 × 512 = 1,048,575 free names. The
    // last name read adds one (at the limit) or two (one past).
    let words: Vec<String> = (1..=511).map(|i| format!("free_name_{i}")).collect();
    for (last, past) in [("free_name_1", false), ("free_name_1+free_name_2", true)] {
        let mut t = T::new();
        for word in &words {
            define(&mut t, word, None, "1");
        }
        define(&mut t, "wide_free", None, &words.join("+"));
        for k in 1..=2047 {
            define(&mut t, &format!("wide_user_{k}"), None, "wide_free+0");
        }
        define(&mut t, "last_name", None, last);
        let mut names = crate::eval::NameCache::default();
        for k in 1..=2047 {
            let reader = gridcraft_formula::parse(&format!("=wide_user_{k}")).unwrap();
            assert!(!crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1, "{last}: wide_user_{k} is within the budget");
        }
        let reader = gridcraft_formula::parse("=last_name").unwrap();
        assert_eq!(crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1, past, "{last}");
    }
}

/// Three tiers of names: `leaves` names `leaf_<word>` reading C1 (the first) or holding 1, each of
/// `children` names `child_<word>` summing all the leaves, and `parents` names `parent_<word>`
/// summing all the children, the parents read from A1 down. Every name a definition mentions is a
/// free name of it, those of the names it uses included.
fn three_tiers(leaves: usize, children: usize, parents: usize) -> T {
    let words = |prefix: &str, count: usize| -> Vec<String> { (0..count).map(|k| format!("{prefix}_{}", word_for(k))).collect() };
    let mut t = T::new();
    t.wb.sheet_mut(0).unwrap().cells.set(c("C1"), Cell::value(Value::number(1.0)));
    let leaf_names = words("leaf", leaves);
    for (k, leaf) in leaf_names.iter().enumerate() {
        define(&mut t, leaf, None, if k == 0 { "Sheet1!$C$1" } else { "1" });
    }
    let child_names = words("child", children);
    for child in &child_names {
        define(&mut t, child, None, &leaf_names.join("+"));
    }
    for (row, parent) in words("parent", parents).iter().enumerate() {
        define(&mut t, parent, None, &child_names.join("+"));
        t.wb.sheet_mut(0).unwrap().cells.set(CellRef::new(row as u32, 0), Cell::formula(Formula::new(&format!("={parent}"))));
    }
    t
}

/// A word for `k` made of letters only (`k` = 0 is `alpha`, 26 is `alphabravo`), so no name made
/// with it reads as a cell reference.
fn word_for(k: usize) -> String {
    const LETTERS: [&str; 26] = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliett", "kilo", "lima", "mike", "november", "oscar",
        "papa", "quebec", "romeo", "sierra", "tango", "uniform", "victor", "whiskey", "xray", "yankee", "zulu",
    ];
    let mut word = String::new();
    let mut rest = k;
    loop {
        word.push_str(LETTERS[rest % 26]);
        rest /= 26;
        if rest == 0 {
            return word;
        }
        rest -= 1;
    }
}

#[test]
fn a_long_free_name_is_held_once_however_many_names_keep_it() {
    // `long_leaf` (4,000 characters) is a free name of `child_of_long` and of each of 1,000
    // parents, which mention it and `child_of_long`. The build keeps its spelling once, and each
    // summary a number for it: the bytes the free names hold are the two spellings met in
    // definitions plus one number per free name kept (one for the child, two for each parent), not
    // 1,001 copies of the long name.
    let long = format!("long_leaf{}", "q".repeat(3_991));
    assert_eq!(long.len(), 4_000);
    let mut t = T::new();
    define(&mut t, &long, None, "1");
    define(&mut t, "child_of_long", None, &long);
    for k in 0..1000 {
        define(&mut t, &format!("parent_{}", word_for(k)), None, &format!("child_of_long+{long}"));
    }
    let mut names = crate::eval::NameCache::default();
    for k in 0..1000 {
        let reader = gridcraft_formula::parse(&format!("=parent_{}", word_for(k))).unwrap();
        assert!(!crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1, "parent_{} is static", word_for(k));
    }
    let kept = 1 + 2 * 1000;
    assert_eq!(names.free_name_bytes(), long.len() + "child_of_long".len() + kept * std::mem::size_of::<usize>());
}

#[test]
fn free_names_shared_by_the_names_a_definition_uses_are_copied_once_per_use() {
    // Each of 4 parents sums 3 children, each child summing the same 5 leaves: a parent copies the
    // children's 5 free names once per child, 15 per parent and 60 in all, and the leaves (a
    // number or a cell) copy none.
    let t = three_tiers(5, 3, 4);
    let mut names = crate::eval::NameCache::default();
    for k in 0..4 {
        let reader = gridcraft_formula::parse(&format!("=parent_{}", word_for(k))).unwrap();
        assert!(!crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1);
    }
    assert_eq!(names.folded(), 4 * 3 * 5);
}

#[test]
fn a_name_used_twice_in_one_definition_hands_over_its_free_names_once() {
    // `twice` mentions `child_alpha` twice: its two free names are copied at the first mention
    // only, where its areas are taken, and the second adds nothing.
    let mut t = T::new();
    define(&mut t, "leaf_one", None, "1");
    define(&mut t, "leaf_two", None, "2");
    define(&mut t, "child_alpha", None, "leaf_one+leaf_two");
    define(&mut t, "twice", None, "child_alpha*child_alpha");
    let mut names = crate::eval::NameCache::default();
    let reader = gridcraft_formula::parse("=twice").unwrap();
    assert!(!crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1);
    assert_eq!(names.folded(), 2);
}

#[test]
fn the_build_budget_on_copied_free_names_holds_at_its_limit() {
    // Three parents of two children of four leaves copy 3 × 2 × 4 = 24 free names. With a budget
    // of exactly that, every parent is static; one less, and the copy that would pass it is not
    // made: one parent is dynamic, the build copies no more than its budget, and every parent
    // still follows C1, which the first leaf reads.
    let before = crate::eval::FOLD_BUDGET.with(std::cell::Cell::get);
    for (budget, dynamic) in [(24usize, 0usize), (23, 1)] {
        crate::eval::FOLD_BUDGET.with(|b| b.set(budget));
        let mut t = three_tiers(4, 2, 3);
        t.calc.recalc_all(&mut t.wb);
        let mut names = crate::eval::NameCache::default();
        for k in 0..3 {
            let reader = gridcraft_formula::parse(&format!("=parent_{}", word_for(k))).unwrap();
            crate::eval::precedents_with(&t.wb, 0, &reader, &mut names);
        }
        assert!(names.folded() <= budget, "budget {budget}: copied {}", names.folded());
        let found = (0..3).filter(|r| t.calc.graph.is_dynamic((0, CellRef::new(*r, 0)))).count();
        assert_eq!(found, dynamic, "budget {budget}");
        let values = |t: &T| (0..3).map(|r| t.wb.sheet(0).unwrap().value(CellRef::new(r, 0))).collect::<Vec<_>>();
        assert_eq!(values(&t), vec![Value::number(8.0); 3], "budget {budget}");
        t.set("C1", "11");
        assert_eq!(values(&t), vec![Value::number(28.0); 3], "budget {budget}: the parents follow C1");
    }
    crate::eval::FOLD_BUDGET.with(|b| b.set(before));
}

#[test]
fn a_span_of_sheets_counts_each_mention_against_a_names_areas() {
    // `twice_spanned` sums A1 over a span of sheets, mentioned twice: each mention adds one area
    // per sheet before repeats are removed, so 512 sheets fill a name's 1,024 areas exactly
    // (static) and 513 go past them (dynamic, the second mention adding none). Either way the
    // reader follows A1 of a sheet inside the span.
    for (sheets, dynamic) in [(512usize, false), (513, true)] {
        let mut t = T::new();
        let names: Vec<String> = (0..sheets).map(|k| format!("span_{}", word_for(k))).collect();
        for name in &names {
            let mut sheet = gridcraft_model::Sheet::new(name);
            sheet.cells.set(c("A1"), Cell::value(Value::number(1.0)));
            t.wb.sheets.push(std::sync::Arc::new(sheet));
        }
        let span = format!("{}:{}!$A$1", names[0], names[sheets - 1]);
        define(&mut t, "twice_spanned", None, &format!("SUM({span},{span})"));
        t.wb.sheet_mut(0).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=twice_spanned")));
        t.calc.recalc_all(&mut t.wb);
        assert_eq!(t.calc.graph.is_dynamic((0, c("B1"))), dynamic, "{sheets} sheets");
        assert_eq!(t.num("B1"), 2.0 * sheets as f64, "{sheets} sheets");
        set_on(&mut t, 200, "A1", "5");
        assert_eq!(t.num("B1"), 2.0 * sheets as f64 + 8.0, "{sheets} sheets: B1 follows A1 of a sheet in the span");
    }
}

#[test]
fn a_range_over_a_span_of_sheets_finds_each_box_by_its_sheet() {
    // `(first:last!A1):(first:last!B3)` over 300 sheets brings two areas per sheet into the range's
    // list, which closes into one box per sheet, A1:B3, in the order of the sheets. The first areas
    // scan the boxes while there are fewer than `SCANNED_UP_TO` (120 comparisons for 16), and each
    // area after that probes the index once: about 700 looks. A scan of the boxes made so far at
    // every area compares about 90,000.
    let sheets = 300;
    let mut t = T::new();
    let names: Vec<String> = (0..sheets).map(|k| format!("span_{}", word_for(k))).collect();
    for name in &names {
        t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new(name)));
    }
    let (first, last) = (&names[0], &names[sheets - 1]);
    let formula = gridcraft_formula::parse(&format!("=SUM(({first}:{last}!A1):({first}:{last}!B3))")).unwrap();
    crate::eval::BOXES_LOOKED_AT.with(|n| n.set(0));
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &formula);
    let looked_at = crate::eval::BOXES_LOOKED_AT.with(std::cell::Cell::get);
    assert!(!dynamic);
    let boxes: Vec<Area> = (1..=sheets).map(|sheet| Area { sheet, range: RangeRef::new(c("A1"), c("B3")) }).collect();
    assert_eq!(areas, boxes);
    let scanned = crate::eval::SCANNED_UP_TO;
    assert!(looked_at <= 2 * sheets + scanned * scanned, "looked {looked_at} times");
}

#[test]
fn a_range_operand_over_a_span_of_sheets_counts_each_mention_against_a_names_areas() {
    // A range operand over a span of 510 sheets, `(first:last!$A$1):$A$2`, closes into 511 boxes (one
    // per sheet of the span, and A2 of the sheet resolving the name), and each mention counts them
    // in full against the name's 1,024 areas, as a span of sheets written out does. Two mentions after C1 and C2 fill the 1,024 exactly (static); after C1, C2 and C3 they
    // go past by one (dynamic). The operands sit in an IF branch never taken, so the value is the
    // sum of the cells in front, which the reader follows either way.
    let sheets = 510;
    let names: Vec<String> = (0..sheets).map(|k| format!("span_{}", word_for(k))).collect();
    let operand = format!("({}:{}!$A$1):$A$2", names[0], names[sheets - 1]);
    for (front, value, dynamic) in [("Sheet1!$C$1+Sheet1!$C$2", 7.0, false), ("Sheet1!$C$1+Sheet1!$C$2+Sheet1!$C$3", 12.0, true)] {
        let mut t = T::new();
        for name in &names {
            let mut sheet = gridcraft_model::Sheet::new(name);
            sheet.cells.set(c("A1"), Cell::value(Value::number(1.0)));
            t.wb.sheets.push(std::sync::Arc::new(sheet));
        }
        t.set("C1", "3");
        t.set("C2", "4");
        t.set("C3", "5");
        define(&mut t, "spread", None, &format!("IF(TRUE,{front},SUM({operand},{operand}))"));
        t.set("B1", "=spread");
        t.calc.recalc_all(&mut t.wb);
        assert_eq!(t.calc.graph.is_dynamic((0, c("B1"))), dynamic, "{front}");
        assert_eq!(t.num("B1"), value, "{front}");
        t.set("C1", "10");
        assert_eq!(t.num("B1"), value + 7.0, "{front}: B1 follows C1");
    }
}

#[test]
fn a_reference_repeated_before_a_span_of_sheets_counts_as_it_does_after() {
    // A1 written 1,100 times and a span of 10 sheets hold 11 distinct areas in either order: the
    // repeats before the span are removed when the span would not fit with them, as they are after
    // collecting when the span comes first, so both names are static with the same 11 areas and
    // values. A name whose distinct areas pass 1,024 (1,015 cells and the 10 sheets) is dynamic.
    let mut t = T::new();
    let names: Vec<String> = (0..10).map(|k| format!("span_{}", word_for(k))).collect();
    for name in &names {
        let mut sheet = gridcraft_model::Sheet::new(name);
        sheet.cells.set(c("A1"), Cell::value(Value::number(1.0)));
        t.wb.sheets.push(std::sync::Arc::new(sheet));
    }
    t.set("A1", "2");
    for r in 1..=1015 {
        t.set(&format!("C{r}"), "1");
    }
    let span = format!("SUM({}:{}!$A$1)", names[0], names[9]);
    let repeats = vec!["$A$1"; 1100].join("+");
    let distinct = (1..=1015).map(|r| format!("$C${r}")).collect::<Vec<_>>().join("+");
    define(&mut t, "repeats_first", None, &format!("{repeats}+{span}"));
    define(&mut t, "span_first", None, &format!("{span}+{repeats}"));
    define(&mut t, "too_many_distinct", None, &format!("{distinct}+{span}"));
    t.set("B1", "=repeats_first");
    t.set("B2", "=span_first");
    t.set("B3", "=too_many_distinct");
    t.calc.recalc_all(&mut t.wb);
    for at in ["B1", "B2"] {
        assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} is static");
        assert_eq!(t.calc.graph.precedents_of((0, c(at))).len(), 11, "{at}");
        assert_eq!(t.num(at), 2210.0, "{at}");
    }
    assert!(t.calc.graph.is_dynamic((0, c("B3"))), "1,025 distinct areas are past the bound");
    assert_eq!(t.num("B3"), 1025.0);
    t.set("A1", "3");
    set_on(&mut t, 5, "A1", "4");
    assert_eq!((t.num("B1"), t.num("B2")), (3313.0, 3313.0), "both follow A1 here and on a sheet of the span");
    assert_eq!(t.num("B3"), 1028.0);
}

#[test]
fn a_name_bound_in_two_nested_scopes_stays_bound_until_the_outer_one_ends() {
    // `edge` is also a defined name ($B$2). Bound by the outer LET and again by the inner one, it
    // is still the outer variable after the inner LET ends: the formula reads D9 and C3 (the
    // values) and not B2, static. After the outer LET it is the defined name again (B2). A LAMBDA
    // parameter over a LET variable of the same name works the same way.
    let mut t = T::new();
    define(&mut t, "edge", None, "Sheet1!$B$2");
    let cell = |at: &str| Area { sheet: 0, range: RangeRef::parse(at).unwrap() };
    for (formula, expected) in [
        ("=LET(edge,$D$9,LET(edge,$C$3,edge)+edge)", vec![cell("D9"), cell("C3")]),
        ("=LET(edge,$D$9,LET(edge,$C$3,edge)+edge)+edge", vec![cell("D9"), cell("C3"), cell("B2")]),
        ("=LAMBDA(edge,LET(edge,$C$3,edge)+edge)", vec![cell("C3")]),
    ] {
        let e = gridcraft_formula::parse(formula).unwrap();
        let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
        assert!(!dynamic, "{formula}");
        assert_eq!(areas, expected, "{formula}");
    }
}

#[test]
fn a_name_used_before_and_after_its_let_binding_reads_the_defined_name_then_the_variable() {
    // In `before`'s value `edge` is not bound yet: `$A$1:edge` is the static box A1:B2. In the
    // result it is the variable, which reads nothing itself. Bound too early, `before` would hold a
    // bound name in a range's operand (dynamic); bound too late, the result would read B2.
    let mut t = T::new();
    define(&mut t, "edge", None, "Sheet1!$B$2");
    let e = gridcraft_formula::parse("=LET(before,SUM($A$1:edge),edge,$D$9,before+edge)").unwrap();
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
    assert!(!dynamic);
    assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::parse("A1:B2").unwrap() }, Area { sheet: 0, range: RangeRef::parse("D9").unwrap() }]);
}

#[test]
fn a_lookup_under_many_bound_names_compares_one_per_check() {
    // An uninvoked LAMBDA of 200 parameters whose body mentions `with_free` (which leaves `other`
    // free) 100 times: each mention checks whether the caller binds `other` with one lookup (and
    // whether `with_free` itself is bound with none, no bound name being spelled so). Compared one
    // by one with the bound names, that was 200 comparisons per mention, 20,000 in all.
    let mut t = T::new();
    define(&mut t, "other", None, "Sheet1!$B$1");
    define(&mut t, "with_free", None, "other");
    let parameters: Vec<String> = (0..200).map(|k| format!("param_{}", word_for(k))).collect();
    let body = vec!["with_free"; 100].join("+");
    let e = gridcraft_formula::parse(&format!("=LAMBDA({},{body})", parameters.join(","))).unwrap();
    crate::eval::BOUND_NAMES_COMPARED.with(|n| n.set(0));
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
    let compared = crate::eval::BOUND_NAMES_COMPARED.with(std::cell::Cell::get);
    assert!(!dynamic);
    assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::cell(c("B1")) }]);
    assert!(compared <= 2 * 100, "compared {compared} bound names for 100 mentions");
}

#[test]
fn a_recursive_name_under_the_same_bound_names_adds_them_at_its_first_self_call_only() {
    // A LAMBDA of 200 parameters calling itself 100 times under those parameters: the bound names
    // are added to its self-call set at the first call, and the other 99 find them unchanged.
    // Added at every call, that was 200 names per call, 20,000 in all.
    let mut t = T::new();
    let parameters: Vec<String> = (0..200).map(|k| format!("param_{}", word_for(k))).collect();
    let first = &parameters[0];
    let calls = vec![format!("deep_loop({first}-1)"); 100].join("+");
    define(&mut t, "deep_loop", None, &format!("LAMBDA({},IF({first}<=0,Sheet1!$C$1,{calls}))", parameters.join(",")));
    let e = gridcraft_formula::parse("=deep_loop(Sheet1!$A$1)").unwrap();
    crate::eval::BOUND_NAMES_COMPARED.with(|n| n.set(0));
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
    let compared = crate::eval::BOUND_NAMES_COMPARED.with(std::cell::Cell::get);
    assert!(!dynamic);
    assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::cell(c("A1")) }, Area { sheet: 0, range: RangeRef::cell(c("C1")) }]);
    assert!(compared <= 200 + 4 * 100, "compared {compared} bound names for 100 self-calls");
}

#[test]
fn a_recursive_name_notes_the_names_bound_at_each_self_call_whose_bound_names_changed() {
    // `walker` calls itself under `n`, then under `n` and the LET variable `other`, then under `n`
    // again. It leaves `other`, a defined name, free in its first branch, so the second call reads
    // the caller's `other`: dynamic. `stroller` binds `spare` instead, which it never leaves free:
    // static, reading the argument and `other`'s cell. `strider` makes the same three calls with no
    // LET: static.
    let mut t = T::new();
    define(&mut t, "other", None, "Sheet1!$C$2");
    define(&mut t, "walker", None, "LAMBDA(n,IF(n<=0,other,walker(n-1)+LET(other,Sheet1!$C$1,walker(n-2))+walker(n-3)))");
    define(&mut t, "stroller", None, "LAMBDA(n,IF(n<=0,other,stroller(n-1)+LET(spare,Sheet1!$C$1,stroller(n-2))+stroller(n-3)))");
    define(&mut t, "strider", None, "LAMBDA(n,IF(n<=0,other,strider(n-1)+strider(n-2)+strider(n-3)))");
    let cell = |at: &str| Area { sheet: 0, range: RangeRef::cell(c(at)) };
    for (formula, expected_dynamic, expected) in [
        ("=walker(Sheet1!$A$1)", true, vec![cell("A1"), cell("C2"), cell("C1")]),
        ("=stroller(Sheet1!$A$1)", false, vec![cell("A1"), cell("C2"), cell("C1")]),
        ("=strider(Sheet1!$A$1)", false, vec![cell("A1"), cell("C2")]),
    ] {
        let e = gridcraft_formula::parse(formula).unwrap();
        let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
        assert_eq!(dynamic, expected_dynamic, "{formula}");
        assert_eq!(areas, expected, "{formula}");
    }
}

#[test]
fn a_name_in_many_range_operands_is_boxed_once() {
    // `scattered` holds 300 single cells, D1:D300; `repeater` reads `scattered:$A$1` 200 times. Each
    // range operand takes the name's one box, which with A1 closes into A1:D300, so the 300 areas
    // are placed into a box once: about 300 looks (one box compared for each), and one more per
    // operand. Each operand placing all 300 areas into its own box looked about 60,000 times.
    let mut t = T::new();
    let cells = (1..=300).map(|r| format!("Sheet1!$D${r}")).collect::<Vec<_>>().join(",");
    define(&mut t, "scattered", None, &format!("({cells})"));
    define(&mut t, "repeater", None, &vec!["SUM(scattered:$A$1)"; 200].join("+"));
    let e = gridcraft_formula::parse("=repeater").unwrap();
    crate::eval::BOXES_LOOKED_AT.with(|n| n.set(0));
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
    let looked_at = crate::eval::BOXES_LOOKED_AT.with(std::cell::Cell::get);
    assert!(!dynamic);
    assert_eq!(areas, vec![Area { sheet: 0, range: RangeRef::parse("A1:D300").unwrap() }]);
    assert!(looked_at <= 300 + 2 * 200, "looked at {looked_at} boxes");
}

#[test]
fn a_name_in_range_operands_on_several_sheets_gives_the_boxes_of_its_areas() {
    // `spread` holds five cells on three sheets, `twin` two cells of Sheet1 (one box), `pair` one
    // cell on each of two sheets (two boxes, as many as its areas). Each range operand closes into
    // one box per sheet, in the order the sheets first appear, whether the names sit in the left or
    // right operand, in a chain of ranges, in a union with another name or a repeat of itself, in
    // the formula or in a definition.
    let mut t = T::new();
    for name in ["north", "south"] {
        t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new(name)));
    }
    define(&mut t, "spread", None, "(north!$B$2,Sheet1!$C$5,north!$D$9,south!$A$3,Sheet1!$E$1)");
    define(&mut t, "twin", None, "(Sheet1!$G$1,Sheet1!$G$2)");
    define(&mut t, "pair", None, "(south!$F$6,north!$A$1)");
    define(&mut t, "holder", None, "SUM(spread:$B$1)+SUM($H$9:spread)+SUM(twin:pair)+SUM(spread:$A$1:twin)+SUM((spread,twin,spread):$A$1)");
    let area = |sheet: usize, at: &str| Area { sheet, range: RangeRef::parse(at).unwrap() };
    let (north, south) = (1, 2);
    for (formula, expected) in [
        ("=SUM(spread:$B$1)", vec![area(north, "B2:D9"), area(0, "B1:E5"), area(south, "A3")]),
        ("=SUM($H$9:spread)", vec![area(0, "C1:H9"), area(north, "B2:D9"), area(south, "A3")]),
        ("=SUM(twin:pair)", vec![area(0, "G1:G2"), area(south, "F6"), area(north, "A1")]),
        ("=SUM(spread:$A$1:twin)", vec![area(north, "B2:D9"), area(0, "A1:G5"), area(south, "A3")]),
        ("=SUM((spread,twin,spread):$A$1)", vec![area(north, "B2:D9"), area(0, "A1:G5"), area(south, "A3")]),
        ("=SUM(spread:$A$1)+SUM(twin:$A$1)", vec![area(north, "B2:D9"), area(0, "A1:E5"), area(south, "A3"), area(0, "A1:G2")]),
        (
            "=holder",
            vec![
                area(north, "B2:D9"),
                area(0, "B1:E5"),
                area(south, "A3"),
                area(0, "C1:H9"),
                area(0, "G1:G2"),
                area(south, "F6"),
                area(north, "A1"),
                area(0, "A1:G5"),
            ],
        ),
    ] {
        let e = gridcraft_formula::parse(formula).unwrap();
        let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &e);
        assert!(!dynamic, "{formula}");
        assert_eq!(areas, expected, "{formula}");
    }
}

#[test]
fn a_name_held_as_its_boxes_counts_its_areas_when_repeats_are_removed() {
    // In one range operand of a definition: C1:C250 written out, a name of one column of cells,
    // then a span of 300 sheets. With `column_cells` (C1:C600) the operand holds 850 areas before
    // the span and 1,150 with it, so the repeats are removed, leaving 600, and the span fits (900):
    // static. With `longer_column` (C1:C750) 750 remain, and the span passes 1,024: dynamic. Were
    // the name counted as its one box when the repeats are removed, the second would fit as well.
    let mut t = T::new();
    let sheets: Vec<String> = (0..300).map(|k| format!("span_{}", word_for(k))).collect();
    for name in &sheets {
        t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new(name)));
    }
    let column = |rows: u32| (1..=rows).map(|r| format!("$C${r}")).collect::<Vec<_>>().join(",");
    define(&mut t, "column_cells", None, &format!("({})", column(600)));
    define(&mut t, "longer_column", None, &format!("({})", column(750)));
    let written = (1..=250).map(|r| format!("$C${r}")).collect::<Vec<_>>().join("+");
    let span = format!("SUM({}:{}!$A$1)", sheets[0], sheets[299]);
    define(&mut t, "fits_after_removal", None, &format!("SUM(({written}+column_cells+{span}):$B$1)"));
    define(&mut t, "past_after_removal", None, &format!("SUM(({written}+longer_column+{span}):$B$1)"));
    let fits = gridcraft_formula::parse("=fits_after_removal").unwrap();
    let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &fits);
    assert!(!dynamic);
    let mut expected = vec![Area { sheet: 0, range: RangeRef::parse("B1:C600").unwrap() }];
    expected.extend((1..=300).map(|sheet| Area { sheet, range: RangeRef::cell(c("A1")) }));
    assert_eq!(areas, expected);
    let past = gridcraft_formula::parse("=past_after_removal").unwrap();
    assert!(crate::eval::precedents(&t.wb, 0, &past).1, "750 distinct areas and a span of 300 sheets");
}

#[test]
fn a_span_repeated_after_many_references_sifts_only_the_areas_added_since() {
    // `crowded` reads C1:C1000 one by one, then a span of 10 sheets 40 times: 1,010 distinct areas,
    // static. From the third span on, each one would pass 1,024 areas with the repeats of the last,
    // so the repeats are removed first: looking only at the areas added since the last removal,
    // and adding no areas for a span the list already holds, that is the list once; looking at the
    // ten areas of each span again took about 1,400, and passing over the whole list again at each
    // span about 38,000.
    let mut t = T::new();
    let sheets: Vec<String> = (0..10).map(word_for).collect();
    for name in &sheets {
        let mut sheet = gridcraft_model::Sheet::new(name);
        sheet.cells.set(c("A1"), Cell::value(Value::number(1.0)));
        t.wb.sheets.push(std::sync::Arc::new(sheet));
    }
    for r in 1..=1000 {
        t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("C{r}")), Cell::value(Value::number(1.0)));
    }
    let cells = (1..=1000).map(|r| format!("$C${r}")).collect::<Vec<_>>().join("+");
    let spans = vec![format!("SUM({}:{}!$A$1)", sheets[0], sheets[9]); 40].join("+");
    define(&mut t, "crowded", None, &format!("{cells}+{spans}"));
    t.wb.sheet_mut(0).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=crowded")));
    crate::eval::AREAS_SIFTED.with(|n| n.set(0));
    t.calc.recalc_all(&mut t.wb);
    let sifted = crate::eval::AREAS_SIFTED.with(std::cell::Cell::get);
    assert!(!t.calc.graph.is_dynamic((0, c("B1"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("B1"))).len(), 1010);
    assert_eq!(t.num("B1"), 1400.0);
    assert!(sifted <= 1024, "looked at {sifted} areas");
    set_on(&mut t, 4, "A1", "3");
    assert_eq!(t.num("B1"), 1480.0, "B1 follows A1 of a sheet in the span");
}

#[test]
fn past_the_build_budget_on_free_names_none_are_copied_into_a_name_that_cannot_keep_them() {
    // As in `the_build_budget_on_free_names_holds_at_its_limit`: `wide_free` leaves 511 names free,
    // and 2,047 names `wide_user_k` = `wide_free+0` keep 1,048,575 free names, one under the
    // budget. Each of 100 further names `late_user_k` = `wide_free+0` would keep 512: it is dynamic,
    // as in that test, and the 511 free names of `wide_free` are not copied into it only to be
    // dropped.
    let words: Vec<String> = (1..=511).map(|i| format!("free_name_{i}")).collect();
    let mut t = T::new();
    for word in &words {
        define(&mut t, word, None, "1");
    }
    define(&mut t, "wide_free", None, &words.join("+"));
    for k in 1..=2047 {
        define(&mut t, &format!("wide_user_{k}"), None, "wide_free+0");
    }
    for k in 1..=100 {
        define(&mut t, &format!("late_user_{k}"), None, "wide_free+0");
    }
    let mut names = crate::eval::NameCache::default();
    for k in 1..=2047 {
        let reader = gridcraft_formula::parse(&format!("=wide_user_{k}")).unwrap();
        assert!(!crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1, "wide_user_{k} is within the budget");
    }
    let folded = names.folded();
    crate::eval::FREE_NAMES_COPIED.with(|n| n.set(0));
    for k in 1..=100 {
        let reader = gridcraft_formula::parse(&format!("=late_user_{k}")).unwrap();
        assert!(crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1, "late_user_{k} is past the budget");
    }
    assert_eq!(crate::eval::FREE_NAMES_COPIED.with(std::cell::Cell::get), 0);
    assert_eq!(names.folded() - folded, 100 * 511, "charged as if copied");
}

#[test]
fn a_span_repeated_after_a_removal_of_repeats_counts_as_held_when_a_name_joins_the_definition() {
    // A definition of 1,000 distinct cells, then the same span of 10 sheets three times, then
    // `ten_cells`, a name of 10 areas. The first span takes the list to 1,010 and the second to
    // 1,020 (both fit within 1,024); the third would take it to 1,030, so the repeats are removed
    // (1,010 areas, the span now known as held) and the third span is counted as 10 repeats
    // instead of added. `ten_cells` then joins a list holding 1,010 areas and 10 repeats: 1,030,
    // past 1,024, so the reader is dynamic. Without the repeats it would see 1,020 and be static.
    // Either way the reader follows its cells.
    let mut t = T::new();
    let names: Vec<String> = (0..10).map(|k| format!("span_{}", word_for(k))).collect();
    for name in &names {
        let mut sheet = gridcraft_model::Sheet::new(name);
        sheet.cells.set(c("A1"), Cell::value(Value::number(1.0)));
        t.wb.sheets.push(std::sync::Arc::new(sheet));
    }
    for r in 1..=1000 {
        t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("C{r}")), Cell::value(Value::number(1.0)));
    }
    for r in 1..=10 {
        t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("D{r}")), Cell::value(Value::number(2.0)));
    }
    let span = format!("SUM({}:{}!$A$1)", names[0], names[9]);
    let distinct = (1..=1000).map(|r| format!("$C${r}")).collect::<Vec<_>>().join("+");
    define(&mut t, "ten_cells", None, &format!("SUM({})", (1..=10).map(|r| format!("Sheet1!$D${r}")).collect::<Vec<_>>().join(",")));
    define(&mut t, "spans_repeated", None, &format!("{distinct}+{span}+{span}+{span}+ten_cells"));
    t.set("B1", "=spans_repeated");
    t.calc.recalc_all(&mut t.wb);
    assert!(t.calc.graph.is_dynamic((0, c("B1"))), "1,010 areas and 10 repeats held, 10 more are past the bound");
    assert_eq!(t.num("B1"), 1050.0);
    t.set("D1", "5");
    assert_eq!(t.num("B1"), 1053.0, "B1 follows D1");
}

// A formula mentioning a name whose own array covers cells it reads.

/// Asserts that `cells` hold the same values as a full recalculation of a copy of the workbook.
fn matches_full(t: &T, cells: &[&str], when: &str) {
    let mut full = t.wb.clone();
    Calc::new().recalc_all(&mut full);
    for at in cells {
        assert_eq!(t.get(at), full.sheet(0).unwrap().value(c(at)), "{at} {when}");
    }
}

/// `scale_row` reads C4:D4, D4 holds 2, and B4 reads `formula`.
fn scale_row_reader(formula: &str) -> T {
    let mut t = T::new();
    define(&mut t, "scale_row", None, "Sheet1!$C$4:$D$4");
    t.set("D4", "2");
    t.set("B4", formula);
    t
}

#[test]
fn a_name_reader_whose_array_covers_the_names_cells_matches_a_full_recalculation() {
    // B4's array [C4, D4] spills from B4 into C4, which `scale_row` reads. The formula stays
    // dynamic while it does: its second evaluation reads the C4 its first laid out, so B4 shows
    // D4 in an incremental pass and in a full one alike.
    let mut t = scale_row_reader("=IF(Sheet1!$A$1=0,scale_row+0,5)");
    let cells = ["B4", "C4", "D4"];
    assert!(t.calc.graph.is_dynamic((0, c("B4"))), "its array covers C4");
    for (input, shown) in [("2", 2.0), ("1", 1.0), ("7", 7.0)] {
        t.set("D4", input);
        assert_eq!(t.num("B4"), shown, "D4 = {input}");
        matches_full(&t, &cells, &format!("after D4 = {input}"));
    }
    // Resolved again (as for a rewritten hidden name in manual mode), it keeps the mark.
    t.calc.resolve_again(&t.wb, &[(0, c("B4"))]);
    assert!(t.calc.graph.is_dynamic((0, c("B4"))), "resolved again over the same array");
    // A single value spills nothing: the formula is static again.
    t.set("A1", "1");
    assert_eq!(t.num("B4"), 5.0);
    assert!(!t.calc.graph.is_dynamic((0, c("B4"))), "no array, nothing covered");
    t.set("A1", "0");
    assert!(t.calc.graph.is_dynamic((0, c("B4"))), "the array is back over C4");
    assert_eq!(t.num("B4"), 7.0);
    matches_full(&t, &cells, "with the array back");
    // The name moved to cells the array does not cover (a structural edit: full recalculation).
    t.set("F4", "3");
    t.set("G4", "4");
    t.wb.names.iter_mut().filter(|n| n.name == "scale_row").for_each(|n| n.formula = "Sheet1!$F$4:$G$4".into());
    t.calc.recalc_all(&mut t.wb);
    assert!(!t.calc.graph.is_dynamic((0, c("B4"))), "the array no longer covers the name's cells");
    assert_eq!((t.num("B4"), t.num("C4")), (3.0, 4.0));
    matches_full(&t, &cells, "after the name moved");
}

#[test]
fn a_reader_of_a_name_of_a_name_whose_array_covers_its_cells_matches_a_full_recalculation() {
    // `plus_one` adds 1 to `chain_row` (C1:D1); B1's array [C1+1, D1+1] spills into C1.
    let mut t = T::new();
    define(&mut t, "chain_row", None, "Sheet1!$C$1:$D$1");
    define(&mut t, "plus_one", None, "chain_row+1");
    t.set("D1", "2");
    t.set("B1", "=plus_one+0");
    assert!(t.calc.graph.is_dynamic((0, c("B1"))));
    for (input, shown) in [("2", 4.0), ("1", 3.0), ("7", 9.0)] {
        t.set("D1", input);
        assert_eq!(t.num("B1"), shown, "D1 = {input}");
        matches_full(&t, &["B1", "C1", "D1"], &format!("after D1 = {input}"));
    }
}

#[test]
fn a_reader_whose_array_covers_no_cell_it_reads_through_a_name_stays_static() {
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    define(&mut t, "price_row", None, "Sheet1!$F$1:$G$1");
    define(&mut t, "data_row", None, "Data!$C$4:$D$4");
    t.set("F1", "3");
    t.set("G1", "4");
    set_on(&mut t, 1, "D4", "6");
    // An array beside the name's cells.
    t.set("B1", "=price_row*2");
    // The same addresses as its array, on another sheet.
    t.set("B4", "=data_row+0");
    // Its own array read through a plain reference (left to #4470).
    t.set("D5", "2");
    t.set("B5", "=C5:D5+0");
    for at in ["B1", "B4", "B5"] {
        assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} stays static");
    }
    assert_eq!((t.num("B1"), t.num("C1")), (6.0, 8.0));
    assert_eq!((t.num("B4"), t.num("C4")), (0.0, 6.0));
}

#[test]
fn undo_and_redo_of_the_edit_that_laid_the_array_over_the_names_cells_rebuild_the_mark() {
    // Undo and redo put a workbook back and rebuild the graph without calculating: the mark comes
    // from the arrays the sheets hold.
    let mut t = scale_row_reader("=IF(Sheet1!$A$1=1,scale_row+0,5)");
    assert!(!t.calc.graph.is_dynamic((0, c("B4"))), "a single value");
    let before = t.wb.clone();
    t.set("A1", "1");
    assert!(t.calc.graph.is_dynamic((0, c("B4"))), "the array covers C4");
    let after = t.wb.clone();
    t.wb = before;
    t.calc.rebuild(&t.wb);
    assert!(!t.calc.graph.is_dynamic((0, c("B4"))), "undone: no array");
    t.wb = after;
    t.calc.rebuild(&t.wb);
    assert!(t.calc.graph.is_dynamic((0, c("B4"))), "redone: the array is back over C4");
    t.set("D4", "9");
    assert_eq!(t.num("B4"), 9.0);
    matches_full(&t, &["B4", "C4", "D4"], "after D4 = 9");
}

// Areas the evaluator cuts to the sheet's used range.

#[test]
fn a_name_of_a_whole_column_read_for_its_shape_follows_the_used_extent() {
    // The evaluator cuts a whole column, a whole row, or a block it would read past the bounds of
    // `read_range`, to the sheet's used range: what these readers see depends on cells outside the
    // areas they name. A value entered in Data!B20 only grows that range, and each reader holding a
    // name, a LET variable, a LAMBDA parameter or a LAMBDA name must follow it, as a full
    // recalculation does (B1 was 5 and is 20). A plain reference with no name stays static, as it
    // always was, and so does a reader of a name of a block the evaluator reads whole.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    for r in 1..=5 {
        set_on(&mut t, 1, &format!("A{r}"), &r.to_string());
    }
    for (name, formula) in [
        ("whole_input", "Data!$A:$A"),
        ("edge_block", "Data!$A$2:$A$1048576"),
        ("whole_row", "Data!$1:$1"),
        ("via_other", "whole_input"),
        ("column_rows", "LAMBDA(given,ROWS(Data!$A:$A+0)+given)"),
        ("bounded_input", "Data!$A$1:$A$20000"),
    ] {
        define(&mut t, name, None, formula);
    }
    let dynamic_readers = [
        ("B1", "=ROWS(ROW(whole_input))"),
        ("B2", "=COUNTBLANK(whole_input)"),
        ("B3", "=ROWS(edge_block+0)"),
        ("B4", "=LET(column,Data!$A:$A,ROWS(column+0))"),
        ("B5", "=ROWS(BYROW(Data!$A:$A,LAMBDA(row,row)))"),
        ("B6", "=COLUMNS(COLUMN(whole_row))"),
        ("B7", "=ROWS(via_other+0)"),
        ("B8", "=column_rows(0)"),
        ("B9", "=LAMBDA(column,ROWS(column+0))(Data!$A:$A)"),
    ];
    let static_readers = [("C1", "=SUM(Data!$A:$A)"), ("C2", "=ROWS(bounded_input+0)+SUM(bounded_input)")];
    for (at, formula) in dynamic_readers.iter().chain(&static_readers) {
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new(formula)));
    }
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("B1"), 5.0);
    set_on(&mut t, 1, "B20", "7");
    // Every reader is checked before anything is asserted, so a failure lists all the readers that
    // went stale or static, not only the first.
    let mut full = t.wb.clone();
    Calc::new().recalc_all(&mut full);
    let stale: Vec<String> = dynamic_readers
        .iter()
        .chain(&static_readers)
        .filter(|(at, _)| t.get(at) != full.sheet(0).unwrap().value(c(at)))
        .map(|(at, formula)| format!("{at} {formula}: {:?}, full {:?}", t.get(at), full.sheet(0).unwrap().value(c(at))))
        .collect();
    assert!(stale.is_empty(), "differ from a full recalculation after Data!B20 = 7: {stale:#?}");
    let not_dynamic: Vec<String> =
        dynamic_readers.iter().filter(|(at, _)| !t.calc.graph.is_dynamic((0, c(at)))).map(|(at, formula)| format!("{at} {formula}")).collect();
    assert!(not_dynamic.is_empty(), "static where they should be dynamic: {not_dynamic:#?}");
    for (at, formula) in static_readers {
        assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} {formula} stays static");
    }
    assert_eq!(t.num("B1"), 20.0, "the used range grew to row 20");
    assert_eq!(t.num("C2"), 20_015.0, "bounded_input is read whole: 20,000 rows, 1 to 5 summed");
}

#[test]
fn an_area_is_cut_by_the_same_rule_in_the_walk_and_the_evaluator() {
    // At each bound of `read_range` and one past it: 16,384 cells running to the sheet's edge are
    // read whole, 16,385 are cut; 1,048,576 cells short of the edge are read whole, 1,048,577 are
    // cut. A reader of a name of each is static where the area is read whole and dynamic where it
    // is cut, and after a value entered in Data!B1040000 grows the used range each matches a full
    // recalculation.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    for r in 1..=5 {
        set_on(&mut t, 1, &format!("A{r}"), &r.to_string());
    }
    let pairs = [
        ("at_edge_limit", "Data!$A$1032193:$A$1048576", false),
        ("past_edge_limit", "Data!$A$1032192:$A$1048576", true),
        ("at_block_limit", "Data!$A$1:$P$65536", false),
        ("past_block_limit", "Data!$A$1:$Q$61681", true),
    ];
    let readers = ["D1", "D2", "D3", "D4"];
    for ((name, formula, _), at) in pairs.iter().zip(readers) {
        define(&mut t, name, None, formula);
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new(&format!("=ROWS({name}+0)"))));
    }
    t.calc.recalc_all(&mut t.wb);
    for ((name, formula, cut), at) in pairs.iter().zip(readers) {
        let range = RangeRef::parse(formula.trim_start_matches("Data!")).unwrap();
        assert_eq!(crate::eval::may_be_cut(&range), *cut, "{name}");
        assert_eq!(t.calc.graph.is_dynamic((0, c(at))), *cut, "{at} reads {name}");
    }
    set_on(&mut t, 1, "B1040000", "7");
    assert_eq!((t.num("D2"), t.num("D4")), (7_809.0, 61_681.0), "the cut areas grew with the used range");
    matches_full(&t, &readers, "after Data!B1040000 = 7");
}

#[test]
fn a_name_is_charged_its_text_before_its_definition_is_parsed() {
    // Both definitions are past the parser's 8,192-character limit, so neither parses: a use that
    // gets as far as the parse evaluates to #NAME? (`ISTEXT(banner)` is FALSE). A use of `banner`
    // costs the cell about a million units of work (its text, and the call around it): 255 uses fit
    // in the cell's 268,435,456 units and 256 don't, so 254 leave between one and two million, less
    // than `cold_name`'s three million bytes. Charged before its parse, `cold_name` then makes the
    // cell #CALC! and is never handed to the parser (only `banner` was, its failed parse kept for the
    // pass); parsed before its charge, it would fail the parse and give #NAME? uncharged. After 200
    // uses the charge fits, the parse of `cold_name` is tried and fails, and the cell is #NAME?.
    let mut t = T::new();
    define(&mut t, "banner", None, &format!("\"{}\"", "x".repeat(1_000_000)));
    define(&mut t, "cold_name", None, &format!("\"{}\"", "y".repeat(2_999_998)));
    t.calc.rebuild(&t.wb);
    t.set("A3", "=SUM(MAP(SEQUENCE(255),LAMBDA(row,ISTEXT(banner)*1)))");
    t.set("A4", "=SUM(MAP(SEQUENCE(256),LAMBDA(row,ISTEXT(banner)*1)))");
    assert_eq!((t.get("A3"), t.get("A4")), (Value::number(0.0), Value::Error(CellError::Calc)), "255 uses fit, 256 don't");
    for (at, uses, shown, parsed) in [("A1", 254, CellError::Calc, 1), ("A2", 200, CellError::Name, 2)] {
        crate::recalc::NAMES_PARSED.with(|n| n.set(0));
        t.set(at, &format!("=SUM(MAP(SEQUENCE({uses}),LAMBDA(row,ISTEXT(banner)*1)))+LEN(cold_name)"));
        assert_eq!(t.get(at), Value::Error(shown), "{uses} uses");
        assert_eq!(crate::recalc::NAMES_PARSED.with(std::cell::Cell::get), parsed, "{uses} uses");
    }
}

#[test]
fn a_batch_gives_back_its_old_charges_before_it_resolves_a_new_reader() {
    // At the build's budget on areas from names, one batch clears a reader and enters a new one: the
    // new reader takes the cleared one's place and stays static, whichever comes first in the batch.
    for clear_first in [false, true] {
        let (mut t, _) = wide_readers(1024, false, false);
        t.wb.sheet_mut(0).unwrap().cells.set(c("B2"), Cell::formula(Formula::new("=wide_name")));
        t.wb.sheet_mut(0).unwrap().remove_cell(c("A2"));
        let keys = if clear_first { [(0, c("A2")), (0, c("B2"))] } else { [(0, c("B2")), (0, c("A2"))] };
        t.calc.cells_changed(&mut t.wb, &keys);
        assert!(!t.calc.graph.is_dynamic((0, c("B2"))), "clear first: {clear_first}");
        assert_eq!(t.num("B2"), 1024.0);
    }
}

#[test]
fn resolving_readers_again_gives_back_their_old_charges_first() {
    // In manual mode the readers of changed names are resolved again without a calculation
    // (`Calc::resolve_again`). At the build's budget on areas from names (1,023 readers of the
    // 1,024 areas of `wide_name`, and `shrinking_name` repeating it), `growing_name` takes those
    // 1,024 areas over from `shrinking_name`: its reader stays static whichever comes first in the
    // batch.
    for shrinking_first in [false, true] {
        let (mut t, _) = wide_readers(1023, false, false);
        t.wb.calc.mode = gridcraft_model::CalcMode::Manual;
        define(&mut t, "growing_name", None, "1");
        define(&mut t, "shrinking_name", None, "wide_name");
        for (at, formula) in [("B2", "=growing_name"), ("B3", "=shrinking_name")] {
            t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new(formula)));
        }
        t.calc.rebuild(&t.wb);
        assert!(!t.calc.graph.is_dynamic((0, c("B3"))), "the budget holds all the readers");
        for (name, formula) in [("growing_name", "wide_name"), ("shrinking_name", "1")] {
            t.wb.names.iter_mut().find(|n| n.name == name).unwrap().formula = formula.into();
        }
        let keys = if shrinking_first { [(0, c("B3")), (0, c("B2"))] } else { [(0, c("B2")), (0, c("B3"))] };
        t.calc.resolve_again(&t.wb, &keys);
        assert!(!t.calc.graph.is_dynamic((0, c("B2"))), "shrinking first: {shrinking_first}");
        assert_eq!(t.calc.graph.precedents_of((0, c("B2"))).len(), 1024, "shrinking first: {shrinking_first}");
    }
}

#[test]
fn a_subtotal_reading_a_name_follows_the_used_range_past_its_cell_budget() {
    // SUBTOTAL and AGGREGATE cut each reference argument to the used range and charge what is left
    // against a budget of 16,000,000 cells per call. `big_block` (1,040,000 cells) handed 17 times,
    // and `wide_blocks` (16 blocks of 1,048,576), fit while the used range is small and run out once
    // a value in JA70000 grows it: each reader below holds a name, a LET variable or a LAMBDA
    // parameter and calls SUBTOTAL, itself or in a name's definition, so it is dynamic and matches a
    // full recalculation. A SUBTOTAL over plain references stays static. The rule reaches further
    // than the budget needs: a SUBTOTAL over a plain reference beside a name (`rate_cell`) is
    // dynamic too. The readers sit right of the blocks, which end at column IV.
    let mut t = T::new();
    t.set("A1", "1");
    let block = "Sheet1!$A$1:$Z$40000";
    let seventeen = |of: &str| vec![of; 17].join(",");
    let column = |k: u32| {
        let n = k + 1;
        if n <= 26 {
            char::from(b'A' + n as u8 - 1).to_string()
        } else {
            format!("{}{}", char::from(b'A' + ((n - 1) / 26) as u8 - 1), char::from(b'A' + ((n - 1) % 26) as u8))
        }
    };
    let wide = (0..16).map(|k| format!("Sheet1!${}$1:${}$65536", column(16 * k), column(16 * k + 15))).collect::<Vec<_>>().join(",");
    for (name, formula) in [
        ("big_block", block.to_string()),
        ("wide_blocks", format!("({wide})")),
        ("subtotal_of", format!("LAMBDA(given,SUBTOTAL(9,{}))", seventeen("given"))),
        ("seventeen_sum", format!("SUBTOTAL(9,{})", seventeen(block))),
        ("rate_cell", "Sheet1!$JD$1".to_string()),
    ] {
        define(&mut t, name, None, &formula);
    }
    let dynamic_readers = [
        ("JC1", format!("=SUBTOTAL(9,{})", seventeen("big_block"))),
        ("JC2", "=SUBTOTAL(9,wide_blocks)".to_string()),
        ("JC3", format!("=LET(part,{block},SUBTOTAL(9,{}))", seventeen("part"))),
        ("JC4", format!("=subtotal_of({block})")),
        ("JC5", "=seventeen_sum".to_string()),
        ("JC6", "=SUBTOTAL(9,Sheet1!$A$1:$A$10)+rate_cell".to_string()),
    ];
    let static_reader = ("JC7", "=SUBTOTAL(9,Sheet1!$A$1:$A$10)");
    for (at, formula) in &dynamic_readers {
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new(formula)));
    }
    t.wb.sheet_mut(0).unwrap().cells.set(c(static_reader.0), Cell::formula(Formula::new(static_reader.1)));
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("JC1"), 17.0, "the budget holds while the used range is small");
    t.set("JA70000", "1");
    // Every reader is checked before anything is asserted, so a failure lists them all.
    let mut full = t.wb.clone();
    Calc::new().recalc_all(&mut full);
    let stale: Vec<String> = dynamic_readers
        .iter()
        .map(|(at, formula)| (*at, formula.as_str()))
        .chain([static_reader])
        .filter(|(at, _)| t.get(at) != full.sheet(0).unwrap().value(c(at)))
        .map(|(at, formula)| format!("{at} {:.60}: {:?}, full {:?}", formula, t.get(at), full.sheet(0).unwrap().value(c(at))))
        .collect();
    assert!(stale.is_empty(), "differ from a full recalculation after JA70000 = 1: {stale:#?}");
    let not_dynamic: Vec<&str> = dynamic_readers.iter().filter(|(at, _)| !t.calc.graph.is_dynamic((0, c(at)))).map(|(at, _)| *at).collect();
    assert!(not_dynamic.is_empty(), "static where they should be dynamic: {not_dynamic:?}");
    assert!(!t.calc.graph.is_dynamic((0, c(static_reader.0))), "plain references only");
    assert_eq!(t.get("JC1"), Value::Error(CellError::Num), "past the budget");
}

/// The names of the lookup tests: spellings differing in letter case, sheet-scoped names against
/// workbook-wide ones, non-ASCII names (compared with ASCII case folding only, as the scan does).
fn names_in_several_spellings(t: &mut T) {
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Second")));
    let names: [(&str, Option<usize>); 12] = [
        ("Rate", None),
        ("RATE", None),
        ("rate", Some(1)),
        ("Rate", Some(1)),
        ("Tax", Some(0)),
        ("tax", None),
        ("Größe", None),
        ("GRÖSSE", None),
        ("größe", Some(0)),
        ("only_second", Some(1)),
        ("Repeated", None),
        ("repeated", None),
    ];
    for (i, (name, scope)) in names.into_iter().enumerate() {
        define(t, name, scope, &i.to_string());
    }
}

#[test]
fn name_lookups_find_the_same_name_by_scan_and_by_index() {
    // A build or pass scans the names for its first `NAME_LOOKUPS_SCANNED` lookups and indexes them
    // at the next. Each query is asked as the lookup just below that count, at it, just past it
    // (the one building the index) and the one after, from each sheet and from a sheet that doesn't
    // exist: every answer is the scan's, the first name listed winning among same spellings.
    let mut t = T::new();
    names_in_several_spellings(&mut t);
    let scanned = crate::eval::NAME_LOOKUPS_SCANNED;
    let queries = ["rate", "RATE", "Rate", "tax", "TAX", "größe", "GRÖßE", "Größe", "GRÖSSE", "ONLY_SECOND", "repeated", "REPEATED", "missing"];
    for sheet in 0..3 {
        for query in queries {
            for asked_as in scanned - 1..=scanned + 2 {
                let mut lookup = crate::eval::NameLookup::default();
                crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
                for _ in 1..asked_as {
                    lookup.find(&t.wb, "absent", sheet);
                }
                let found = lookup.find(&t.wb, query, sheet);
                let indexed = crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get) == 1;
                assert_eq!(indexed, asked_as > scanned, "lookup {asked_as}");
                assert_eq!(found, t.wb.name_position(query, sheet), "{query} from sheet {sheet} as lookup {asked_as}");
            }
        }
    }
    // The scan's answers themselves: first listed wins, sheet scope before workbook scope.
    assert_eq!(t.wb.name_position("RATE", 0), Some(0));
    assert_eq!(t.wb.name_position("rate", 1), Some(2));
    assert_eq!(t.wb.name_position("tax", 0), Some(4));
    assert_eq!(t.wb.name_position("tax", 1), Some(5));
    assert_eq!(t.wb.name_position("größe", 0), Some(8));
    assert_eq!(t.wb.name_position("GRÖSSE", 1), Some(7));
    assert_eq!(t.wb.name_position("only_second", 0), None);
    assert_eq!(t.wb.name_position("REPEATED", 2), Some(10));
}

#[test]
fn a_build_or_pass_indexes_the_names_only_past_a_few_lookups() {
    // Many names defined; a formula reading `scanned` of them is built and evaluated by scans, with
    // no index. One reading one more builds the index once in the graph build and once in the pass.
    let scanned = crate::eval::NAME_LOOKUPS_SCANNED;
    let mut t = T::new();
    for k in 0..200 {
        define(&mut t, &format!("filler_{}", word_for(k)), None, "Sheet1!$Z$1");
    }
    for k in 0..scanned * 3 {
        define(&mut t, &format!("source_{}", word_for(k)), None, &format!("Sheet1!$C${}", k + 1));
        t.set(&format!("C{}", k + 1), &(k + 1).to_string());
    }
    t.calc.recalc_all(&mut t.wb);
    let reading = |count: usize| format!("={}", (0..count).map(|k| format!("source_{}", word_for(k))).collect::<Vec<_>>().join("+"));
    for (at, count, built) in [("A1", scanned, 0), ("A2", scanned + 1, 2)] {
        crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
        t.set(at, &reading(count));
        assert_eq!(t.num(at), (count * (count + 1) / 2) as f64, "{count} names");
        assert_eq!(crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get), built, "{count} lookups");
    }
    // In a build alone, through the precedent walk: none below the count, one past it.
    for (count, built) in [(scanned, 0), (scanned + 1, 1), (scanned * 3, 1)] {
        let formula = gridcraft_formula::parse(&reading(count)).unwrap();
        crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
        let (areas, dynamic) = crate::eval::precedents(&t.wb, 0, &formula);
        assert!(!dynamic && !areas.is_empty(), "{count} names");
        assert_eq!(crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get), built, "{count} lookups");
    }
}

/// The blocked-array case below, with `F6` reading `source`; returns the workbook after `G5` is
/// rewritten to read the cells `F6` would fill.
fn blocked_name_reader(source: &str) -> T {
    let mut t = T::new();
    for (at, input) in [("A7", "4"), ("A8", "2"), ("C7", "3"), ("B6", "9"), ("C6", "4"), ("D6", "9"), ("C4", "7")] {
        t.set(at, input);
    }
    define(&mut t, "rng_first", None, "Sheet1!$A$7:$C$8");
    define(&mut t, "Twice", None, "LAMBDA(x, x*2)");
    t.calc.recalc_all(&mut t.wb);
    t.set("G5", "=SEQUENCE(3)+7");
    t.set("F6", &format!("=MAP({source}, Twice)"));
    t.set("G5", "=B6:B7+C6:C7+D6:D7+F6:F7+C4");
    t
}

#[test]
fn a_blocked_array_reading_a_name_settles_as_with_direct_references() {
    // F6's array (F6:H7) is blocked by G5's, and G5's array reads F6:F7, the cells F6 would fill.
    // F6 reads its range through a name and is a static reader of the name's cells, so the two
    // settle: F6 shows #SPILL!, G5 carries that error and G6 adds what is on the sheet (3 + 7).
    // Main recalculates a name reader on every pass, so there F6 is evaluated round after round to
    // the round limit and G5:G6 end on 37 and 14, values the sheet never showed. This pins that an
    // edit and a full recalculation agree, and that the name gives what direct references give.
    let cells = ["F6", "F7", "G5", "G6", "G7", "H6", "H7"];
    let t = blocked_name_reader("rng_first");
    assert_eq!(t.get("F6"), Value::Error(CellError::Spill), "F6 is blocked by G5's array");
    assert_eq!(t.get("G5"), Value::Error(CellError::Spill), "G5 carries F6's error");
    assert_eq!(t.get("G6"), Value::number(10.0), "C7 + C4, with F7 empty");
    for at in ["F7", "G7", "H6", "H7"] {
        assert_eq!(t.get(at), Value::Empty, "{at}");
    }
    matches_full(&t, &cells, "after G5 is rewritten");
    let direct = blocked_name_reader("$A$7:$C$8");
    for at in cells {
        assert_eq!(t.get(at), direct.get(at), "{at}: the name against the direct reference");
    }
}

#[test]
fn a_formulas_own_bound_names_are_not_kept_by_the_build() {
    // A LET variable or LAMBDA parameter spelled as no free name of a definition is numbered for
    // its own walk only: three formulas binding their own names, with no defined names, leave the
    // build holding no spelling.
    let t = T::new();
    let mut names = crate::eval::NameCache::default();
    for formula in
        ["=LET(first_total,1,second_total,first_total+1,second_total)", "=LAMBDA(input_value,input_value*2)(3)", "=LET(Input_Value,4,input_value)"]
    {
        let reader = gridcraft_formula::parse(formula).unwrap();
        assert!(!crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1, "{formula}");
    }
    assert_eq!(names.free_name_bytes(), 0);
}

#[test]
fn a_bound_name_met_later_as_a_free_name_still_shadows_it() {
    // `LET(bound_value,…,uses_bound)` binds `bound_value` before the walk meets it free in
    // `uses_bound`'s definition: the evaluator reads the caller's variable (1+1, not 5+1), so the
    // formula is dynamic, in the walk that resolves `uses_bound` and in one sharing its cache.
    // A formula binding nothing reads the name and stays static.
    let mut t = T::new();
    define(&mut t, "bound_value", None, "5");
    define(&mut t, "uses_bound", None, "bound_value+1");
    let mut names = crate::eval::NameCache::default();
    for (formula, dynamic) in [("=LET(bound_value,1,uses_bound)", true), ("=LET(Bound_Value,1,uses_bound)", true), ("=uses_bound", false)] {
        let reader = gridcraft_formula::parse(formula).unwrap();
        assert_eq!(crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1, dynamic, "{formula}");
    }
    assert_eq!(names.free_name_bytes(), "bound_value".len() + std::mem::size_of::<usize>());
    t.set("A1", "=LET(bound_value,1,uses_bound)");
    t.set("A2", "=uses_bound");
    assert_eq!((t.num("A1"), t.num("A2")), (2.0, 6.0));
}

#[test]
fn the_build_budget_counts_each_name_resolved_for_a_sheet() {
    // Each name a build resolves for a sheet keeps an entry in its cache, which counts against
    // MAX_BUILD_NAME_AREAS as the areas its bytes would fill, whatever areas the name keeps.
    // Names of ten letters defined as 1 fill it at `budget` names: readers of that many are
    // static. `extra_reader` reads the last name defined: one already resolved (static) at the
    // budget, one more (dynamic) one past it, with the same values either way. `first_reader`
    // reads a name resolved before the budget ran out and stays static.
    let spelled = |k: usize| format!("named_{}", (0..4).map(|i| (b'a' + (k / 26usize.pow(i) % 26) as u8) as char).collect::<String>());
    let budget = (1 << 20) / crate::eval::NameCache::entry_charge(spelled(0).len());
    for (count, past) in [(budget, false), (budget + 1, true)] {
        let mut t = T::new();
        t.set("C1", "2");
        for k in 0..count {
            define(&mut t, &spelled(k), None, "1");
        }
        let words: Vec<String> = (0..budget).map(spelled).collect();
        let mut readers: Vec<(CellRef, String)> =
            words.chunks(512).enumerate().map(|(row, chunk)| (CellRef::new(row as u32, 0), format!("={}", chunk.join("+")))).collect();
        let (extra_reader, first_reader) = (c("B1"), c("B2"));
        readers.push((extra_reader, format!("={}*C1", spelled(count - 1))));
        readers.push((first_reader, format!("={}*C1", spelled(0))));
        for (at, formula) in &readers {
            t.wb.sheet_mut(0).unwrap().cells.set(*at, Cell::formula(Formula::new(formula)));
        }
        let keys: Vec<_> = readers.iter().map(|(at, _)| (0, *at)).collect();
        t.calc.cells_changed(&mut t.wb, &keys);
        let chunks = words.chunks(512).count();
        assert!((0..chunks).all(|row| !t.calc.graph.is_dynamic((0, CellRef::new(row as u32, 0)))), "{count}: the chunk readers are static");
        assert_eq!(t.calc.graph.is_dynamic((0, extra_reader)), past, "{count}");
        assert!(!t.calc.graph.is_dynamic((0, first_reader)), "{count}");
        assert_eq!(t.wb.sheet(0).unwrap().value(CellRef::new(0, 0)), Value::number(512.0));
        let values = |t: &T| (t.num("B1"), t.num("B2"));
        assert_eq!(values(&t), (2.0, 2.0), "{count}");
        t.set("C1", "3");
        assert_eq!(values(&t), (3.0, 3.0), "{count}");
        let mut names = crate::eval::NameCache::default();
        for (at, formula) in &readers {
            let reader = gridcraft_formula::parse(formula).unwrap();
            let dynamic = crate::eval::precedents_with(&t.wb, 0, &reader, &mut names).1;
            assert_eq!(dynamic, past && *at == extra_reader, "{count}: {at:?}");
        }
        assert_eq!(names.entries(), budget, "{count}: no entry past the budget");
    }
}
