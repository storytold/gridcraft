//! Defined names in recalculation: names as static precedents of the formulas that read them, the
//! per-pass name cache and its limits, and the TEXT fast path.

use gridcraft_core::RangeRef;

use super::*;
use crate::eval::Area;

#[test]
fn names_are_static_precedents_not_dynamic() {
    // A formula that reads a defined name (a range, or a LAMBDA whose body reads a range) depends
    // on the cells the name resolves to, like a plain reference: it is NOT re-evaluated on every
    // recalc, and it IS re-evaluated when those cells change. Before this change any name made the
    // formula dynamic, so a column of LAMBDA calls was recomputed on every edit anywhere.
    let mut t = T::new();
    for (r, v) in ["10", "20", "30"].iter().enumerate() {
        t.set(&format!("A{}", r + 1), v);
    }
    for def in [("Keys", "Sheet1!$A$1:$A$3"), ("Pick", "LAMBDA(k,XLOOKUP(k,Keys,Keys,\"none\"))"), ("Deep", "LAMBDA(k,Pick(k))")] {
        t.wb.names.push(gridcraft_model::DefinedName {
            name: def.0.into(),
            scope: None,
            formula: def.1.into(),
            comment: String::new(),
            hidden: false,
        });
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
fn deep_formulas_through_a_chain_of_names_stay_within_the_nesting_bound() {
    // The walk recurses once per nesting level, through names too. One formula can't nest past
    // the parser's bound, but a chain of names can add up: within MAX_NESTING in total it resolves
    // to cells, past it the formula stays dynamic with the right value, and eight definitions each
    // nested as deep as the parser allows build the graph on a 1 MiB stack (the Windows main
    // thread) instead of overflowing it.
    const WORDS: [&str; 8] = ["one", "two", "three", "four", "five", "six", "seven", "eight"];
    fn deep(levels: usize, inner: &str) -> String {
        format!("{}{}{}", "SUM(".repeat(levels), inner, ")".repeat(levels))
    }
    fn chain(t: &mut T, prefix: &str, count: usize, levels: usize) {
        for i in 0..count {
            let inner = if i == 0 { "Sheet1!$A$1".to_string() } else { format!("{prefix}_{}", WORDS[i - 1]) };
            t.wb.names.push(gridcraft_model::DefinedName {
                name: format!("{prefix}_{}", WORDS[i]),
                scope: None,
                formula: deep(levels, &inner),
                comment: String::new(),
                hidden: false,
            });
        }
        t.calc.rebuild(&t.wb);
    }
    // The whole test runs on a 1 MiB thread (the Windows main thread), both sides of each bound.
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let mut t = T::new();
            t.set("A1", "7");
            // One formula nested as deep as the parser allows, each layer a parenthesis and an operator
            // (the parser counts the left operand at its expression's level, and so does the walk): static.
            // The evaluator's own depth limit answers the 250-layer one, so its value isn't the point.
            t.set("B3", &format!("={}A1{}", "(".repeat(250), "+1)".repeat(250)));
            assert!(!t.calc.graph.is_dynamic((0, c("B3"))), "250 layers of (x+1) in one formula resolve statically");
            t.set("B4", &format!("={}A1{}", "(".repeat(129), "+1)".repeat(129)));
            assert_eq!(t.num("B4"), 136.0);
            assert!(!t.calc.graph.is_dynamic((0, c("B4"))), "129 layers resolve statically");
            // The shapes the parser counts without a level of their own: a postfix `%`, an invoked
            // result, a trim reference, a computed range, the spill operator and an array constant's
            // elements each sit at their own expression's level, so a formula at the parser's limit with
            // one of them at the innermost or outermost point is still static.
            let parens = |inner: &str, n: usize| format!("{}{inner}{}", "(".repeat(n), ")".repeat(n));
            for (at, formula) in [
                ("B5", format!("={}%%", parens("A1", 255))),
                ("B6", format!("={}:B1:(C1)", parens("A1", 255))),
                ("B7", format!("=({}:.B5):.C5", parens("A1", 254))),
                ("B8", format!("=LAMBDA(x,{})(A1)(1)(1)", parens("x", 254))),
                ("B9", format!("={}", parens("A1#", 255))),
                ("B10", format!("=SUM(A1,{})", parens("{1,2}", 254))),
            ] {
                t.set(at, &formula);
                assert!(!t.calc.graph.is_dynamic((0, c(at))), "{at} resolves statically");
                assert!(!t.calc.graph.precedents_of((0, c(at))).is_empty(), "{at} has a precedent");
            }
            // Two names whose nesting adds up to exactly the bound, and two one past it: the last level
            // the parser allows resolves, the first it doesn't is dynamic.
            for (prefix, outer, inner) in [("Edge", 127, 126), ("Past", 127, 127)] {
                t.wb.names.push(gridcraft_model::DefinedName {
                    name: format!("{prefix}_in"),
                    scope: None,
                    formula: deep(inner, "Sheet1!$A$1"),
                    comment: String::new(),
                    hidden: false,
                });
                t.wb.names.push(gridcraft_model::DefinedName {
                    name: format!("{prefix}_out"),
                    scope: None,
                    formula: deep(outer, &format!("{prefix}_in")),
                    comment: String::new(),
                    hidden: false,
                });
            }
            t.calc.rebuild(&t.wb);
            t.set("C1", "=Edge_out");
            t.set("C2", "=Past_out");
            assert_eq!((t.num("C1"), t.num("C2")), (7.0, 7.0));
            assert!(!t.calc.graph.is_dynamic((0, c("C1"))), "255 levels through two names resolve statically");
            assert!(t.calc.graph.is_dynamic((0, c("C2"))), "256 levels through two names stay dynamic");
            // 4 × 60 = 240 levels and four names: within the bound, static.
            chain(&mut t, "Near", 4, 60);
            t.set("B1", "=Near_four");
            assert_eq!(t.num("B1"), 7.0);
            assert!(!t.calc.graph.is_dynamic((0, c("B1"))), "240 levels through four names resolve statically");
            // 8 × 40 = 320 levels: past the bound, dynamic, still right and following A1.
            chain(&mut t, "Far", 8, 40);
            t.set("B2", "=Far_eight");
            assert_eq!(t.num("B2"), 7.0);
            assert!(t.calc.graph.is_dynamic((0, c("B2"))), "320 levels through eight names stay dynamic");
            t.set("A1", "8");
            assert_eq!((t.num("B1"), t.num("B2")), (8.0, 8.0));
            // 8 × 250: a definition as deep as the parser allows, eight times over, on a 1 MiB stack.
            // Then eight definitions of 1,300 trim-reference links each (the parser counts a link as no
            // nesting at all, the walk recurses once or twice per link), and a long trim chain leading to
            // a definition as deep as the parser allows, which is parsed on top of the walk's deepest
            // frame. The budget before a definition is parsed keeps `Reach` on the stack: without it,
            // parsing `Nested` there overflows this thread. No formula within the length limit needs the
            // budget on the walk alone in this build (the long chains fit in 1 MiB without it); it bounds
            // the walk by the stack it measures, whatever a frame costs on other targets.
            chain(&mut t, "Vast", 8, 250);
            t.set("B1", "=Vast_eight");
            assert!(t.calc.graph.is_dynamic((0, c("B1"))));
            for i in 0..8 {
                let inner = if i == 0 { "Sheet1!$A$1".to_string() } else { format!("Trim_{}", WORDS[i - 1]) };
                t.wb.names.push(gridcraft_model::DefinedName {
                    name: format!("Trim_{}", WORDS[i]),
                    scope: None,
                    formula: format!("{inner}{}", ":.$C$1".repeat(1_300)),
                    comment: String::new(),
                    hidden: false,
                });
            }
            t.calc.rebuild(&t.wb);
            t.set("B2", "=Trim_eight");
            assert!(t.calc.graph.is_dynamic((0, c("B2"))), "eight names of 1,300 trim links stay dynamic");
            for (name, formula) in [
                ("Nested", format!("{}1{}", "LAMBDA(x,".repeat(255), ")".repeat(255))),
                ("Spine", format!("Nested{}", ":.1.5".repeat(1_637))),
                ("Reach", format!("Nested{}", ":.1.5".repeat(1_200))),
            ] {
                t.wb.names.push(gridcraft_model::DefinedName { name: name.into(), scope: None, formula, comment: String::new(), hidden: false });
            }
            t.calc.rebuild(&t.wb);
            t.set("B3", "=Spine");
            t.set("B4", "=Reach");
            assert!(t.calc.graph.is_dynamic((0, c("B3"))), "a deep definition at the end of a long chain stays dynamic (it nests past the bound wherever it is used); what this part checks is that the build ends at all");
            assert!(
                t.calc.graph.is_dynamic((0, c("B4"))),
                "likewise; without the budget before a definition is parsed, parsing it here overflows this thread before this line"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn name_chains_and_cycles_stay_bounded() {
    // Names referring to names are followed to a depth bound; a cycle adds nothing and does not
    // hang the graph build.
    let mut t = T::new();
    t.set("A1", "7");
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Loop_first".into(),
        scope: None,
        formula: "Loop_second+1".into(),
        comment: String::new(),
        hidden: false,
    });
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Loop_second".into(),
        scope: None,
        formula: "Loop_first+1".into(),
        comment: String::new(),
        hidden: false,
    });
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Base".into(),
        scope: None,
        formula: "Sheet1!$A$1".into(),
        comment: String::new(),
        hidden: false,
    });
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
        t.wb.names.push(gridcraft_model::DefinedName { name: format!("Tier_{w}"), scope: None, formula, comment: String::new(), hidden: false });
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
        t.wb.names.push(gridcraft_model::DefinedName { name, scope: None, formula, comment: String::new(), hidden: false });
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

#[test]
fn sheet_scoped_names_used_from_another_sheet_are_dynamic() {
    // A LAMBDA's body runs against the calling cell's sheet. A sheet-scoped LAMBDA name passed by
    // name from another sheet (`MAP(5,Sheet1!Markup)` on Data) reads Data!A1, while its references
    // resolve against Sheet1 when the graph is built: a static edge to Sheet1!A1 would leave the
    // cell stale when Data!A1 changes. Used on its own sheet it stays static.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Markup".into(),
        scope: Some(0),
        formula: "LAMBDA(x,x+$A$1)".into(),
        comment: String::new(),
        hidden: false,
    });
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
fn names_mentioned_many_times_are_resolved_once() {
    // Eight names, each mentioning the previous one 20 times in a branch IF never takes; a cell
    // mentioning the last one 20 times. Followed afresh at every mention that is 20^8 expansions
    // for one formula (a workbook can make the graph build take forever); resolved once per name
    // and build, it is 8. The result is the same: a static dependency on the cell at the bottom.
    let mut t = T::new();
    t.set("A1", "7");
    let mut formula = "Sheet1!$A$1".to_string();
    for word in ["one", "two", "three", "four", "five", "six", "seven", "eight"] {
        t.wb.names.push(gridcraft_model::DefinedName {
            name: format!("Rank_{word}"),
            scope: None,
            formula: formula.clone(),
            comment: String::new(),
            hidden: false,
        });
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
fn a_name_of_more_areas_than_the_cap_is_dynamic_and_still_follows_its_cells() {
    // A name's areas are stored for every formula reading it, so a name, or the names one formula
    // mentions, may bring at most MAX_NAME_AREAS (1,024) areas; past it the name or the formula is
    // dynamic, as every name was before names were resolved. Both sides of the bound, and each
    // reader follows a cell inside the names either way.
    let mut t = T::new();
    for r in 1..=1100 {
        t.set(&format!("C{r}"), "1");
    }
    let union = |rows: std::ops::RangeInclusive<u32>| format!("({})", rows.map(|r| format!("Sheet1!$C${r}")).collect::<Vec<_>>().join(","));
    let define = |t: &mut T, name: &str, formula: String| {
        t.wb.names.push(gridcraft_model::DefinedName { name: name.into(), scope: None, formula, comment: String::new(), hidden: false });
    };
    // Four quarters of 256 single cells each: 1,024 areas.
    for (i, word) in ["first", "second", "third", "fourth"].iter().enumerate() {
        let start = i as u32 * 256 + 1;
        define(&mut t, &format!("{word}_quarter"), union(start..=start + 255));
    }
    define(&mut t, "one_more", "Sheet1!$C$1025".into());
    // One name of exactly 1,024 areas, and one of 1,025.
    define(&mut t, "all_of_them", "SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)".into());
    define(&mut t, "one_too_many", "SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)+one_more".into());
    // A name around the one of 1,025: that one brings nothing and makes it dynamic, so a formula
    // reading it stores only the quarter it adds (without the cap per name, the 1,281 areas would
    // be past the cap per formula and the formula would store none).
    define(&mut t, "around_it", "one_too_many*0+SUM(first_quarter)".into());
    // A definition of 1,025 references of its own, written unqualified so it fits a name's length:
    // past the cap it brings nothing, and a name around it only its own areas (in an IF branch never taken, since a union this long evaluates to #NUM!).
    define(&mut t, "direct_many", format!("({})", (1..=1025).map(|r| format!("$C${r}")).collect::<Vec<_>>().join(",")));
    define(&mut t, "around_direct", "IF(TRUE,SUM(first_quarter),SUM(direct_many))".into());
    // A name mentioned again where its areas were already added adds nothing and counts nothing: a
    // definition mentioning the four quarters and one of them again holds its 1,024 cells, static,
    // and so does a formula mentioning a 1,024-area name twice.
    define(
        &mut t,
        "quarters_and_one_again",
        "SUM(first_quarter)+SUM(second_quarter)+SUM(third_quarter)+SUM(fourth_quarter)+SUM(first_quarter)".into(),
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

#[test]
fn a_name_with_relative_references_stays_dynamic_and_follows_its_cell() {
    // A relative reference in a name is read relative to the cell using it, so what it reads isn't
    // known from the definition: the reader stays dynamic. `Below` (A2 as defined from A1) used
    // in B1 reads B2.
    let mut t = T::new();
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "Below".into(),
        scope: None,
        formula: "Sheet1!A2".into(),
        comment: String::new(),
        hidden: false,
    });
    t.set("B2", "5");
    t.set("B1", "=Below");
    assert_eq!(t.num("B1"), 5.0);
    assert!(t.calc.graph.is_dynamic((0, c("B1"))));
    t.set("B2", "6");
    assert_eq!(t.num("B1"), 6.0);
}

#[test]
fn a_workbook_name_resolves_on_each_sheet_that_uses_it() {
    // An unqualified reference in a workbook-wide name resolves on the sheet of the cell using it,
    // so one graph build resolving the name from two sheets keeps an answer per sheet.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.wb.names.push(gridcraft_model::DefinedName { name: "Base".into(), scope: None, formula: "$A$1".into(), comment: String::new(), hidden: false });
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
fn sheet_scoped_names_of_one_spelling_evaluate_per_sheet_in_one_pass() {
    // Two sheets with their own `Rate`: one pass calculating both readers looks each up from its
    // own sheet, whatever it parsed for the other.
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    for (scope, value) in [(0, "1"), (1, "2")] {
        t.wb.names.push(gridcraft_model::DefinedName {
            name: "Rate".into(),
            scope: Some(scope),
            formula: value.into(),
            comment: String::new(),
            hidden: false,
        });
        t.wb.sheet_mut(scope).unwrap().cells.set(c("A1"), Cell::formula(Formula::new("=Rate")));
    }
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("A1"), 1.0);
    assert_eq!(t.wb.sheet(1).unwrap().value(c("A1")), Value::number(2.0));
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
        t.wb.names.push(gridcraft_model::DefinedName {
            name: name.into(),
            scope: Some(scope),
            formula: "$A$1".into(),
            comment: String::new(),
            hidden: false,
        });
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
fn text_with_an_array_of_codes_formats_the_value_by_each() {
    // TEXT converts a single format code once per call; an array of codes still formats by each.
    let mut t = T::new();
    t.set("A1", "=TEXT(1234.5,{\"0\",\"0.0\"})");
    assert_eq!(t.get("A1"), Value::text("1235"));
    assert_eq!(t.get("B1"), Value::text("1234.5"));
}

#[test]
fn a_flat_operator_chain_as_long_as_a_formula_allows_resolves_on_a_small_stack() {
    // `=A1+1+1+…` with 4,096 operands: the walk unwinds the chain's left spine in a loop, so its
    // stack use doesn't grow with the chain, and the formula resolves to its one cell on a 1 MiB
    // thread (the Windows main thread). Recursing once per operator, it ran past the walk's stack
    // budget and the formula was dynamic.
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
fn the_formulas_of_one_build_take_at_most_a_million_areas_from_names() {
    // A name's areas are stored for every formula reading it. A name of 1,024 areas read from
    // 1,024 cells stores exactly the build's budget (1,048,576 areas): all static. One area more,
    // from a reader of a one-cell name, or one more reader of the wide name, and one reader is
    // dynamic; every reader still follows a cell inside the name. The same through a full build
    // and through one edit of all the readers at once (a paste).
    let build = |wide: u32, single: bool, at_once: bool| {
        let mut t = T::new();
        for r in 1..=1024 {
            t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("C{r}")), Cell::value(Value::number(1.0)));
        }
        let union = |start: u32| format!("({})", (start..start + 256).map(|r| format!("Sheet1!$C${r}")).collect::<Vec<_>>().join(","));
        let mut define = |name: &str, formula: String| {
            t.wb.names.push(gridcraft_model::DefinedName { name: name.into(), scope: None, formula, comment: String::new(), hidden: false });
        };
        for (i, word) in ["first", "second", "third", "fourth"].iter().enumerate() {
            define(&format!("{word}_part"), union(i as u32 * 256 + 1));
        }
        define("wide_name", "SUM(first_part)+SUM(second_part)+SUM(third_part)+SUM(fourth_part)".into());
        define("one_cell", "Sheet1!$C$1".into());
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
    };
    // The budget holds across edits too: a full build at exactly the budget, then one more reader
    // entered on its own, is dynamic.
    let (mut t, _) = build(1024, false, false);
    t.set("B2", "=wide_name");
    assert!(t.calc.graph.is_dynamic((0, c("B2"))), "a reader entered after a build at the budget");
    assert_eq!(t.num("B2"), 1024.0);
    // It counts what the formulas hold, not how often they were entered: a reader entered again
    // gives its areas back first and stays static, and a reader cleared makes room for another.
    let (mut t, _) = build(1024, false, false);
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
    let (mut t, _) = build(1024, false, false);
    t.wb.sheet_mut(0).unwrap().cells.set(c("A1"), Cell::formula(Formula::new("=1")));
    t.calc.recalc_all(&mut t.wb);
    t.set("A1", "=2");
    t.set("B2", "=wide_name");
    t.set("B3", "=wide_name");
    assert!(!t.calc.graph.is_dynamic((0, c("B2"))), "the reader taking A1's place");
    assert!(t.calc.graph.is_dynamic((0, c("B3"))), "one reader more, after a rebuild");
    for at_once in [false, true] {
        for (wide, single, dynamic) in [(1024u32, false, 0usize), (1024, true, 1), (1025, false, 1)] {
            let (mut t, readers) = build(wide, single, at_once);
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
            t.wb.names.push(gridcraft_model::DefinedName {
                name: format!("padded_name_{k}"),
                scope: None,
                formula: formula.clone(),
                comment: String::new(),
                hidden: false,
            });
            t.wb.sheet_mut(0).unwrap().cells.set(CellRef::new(k as u32, 0), Cell::formula(Formula::new(&format!("=padded_name_{k}"))));
        }
        if one_byte {
            t.wb.names.push(gridcraft_model::DefinedName {
                name: "tiny_name".into(),
                scope: None,
                formula: "7".into(),
                comment: String::new(),
                hidden: false,
            });
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
    // the areas of the name it repeats. The four quarters (256 areas each), `wide_name` (1,024) and
    // 1,022 copies (1,024 each) keep exactly the build's budget, 1,048,576 areas: all static. One
    // copy more, or a one-cell name read as well, and one reader is dynamic; every reader still
    // follows a cell inside the name.
    let build = |copies: usize, single: bool| {
        let mut t = T::new();
        for r in 1..=1024 {
            t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("C{r}")), Cell::value(Value::number(1.0)));
        }
        let union = |start: u32| format!("({})", (start..start + 256).map(|r| format!("Sheet1!$C${r}")).collect::<Vec<_>>().join(","));
        let mut define = |name: String, formula: String| {
            t.wb.names.push(gridcraft_model::DefinedName { name, scope: None, formula, comment: String::new(), hidden: false });
        };
        for (i, word) in ["first", "second", "third", "fourth"].iter().enumerate() {
            define(format!("{word}_part"), union(i as u32 * 256 + 1));
        }
        define("wide_name".into(), "SUM(first_part)+SUM(second_part)+SUM(third_part)+SUM(fourth_part)".into());
        define("one_cell".into(), "Sheet1!$C$1".into());
        for k in 0..copies {
            define(format!("copy_{k}"), "wide_name".into());
        }
        let mut readers: Vec<(CellRef, String)> = (0..copies).map(|k| (CellRef::new(k as u32, 0), format!("=copy_{k}"))).collect();
        if single {
            readers.push((c("B1"), "=one_cell".into()));
        }
        for (at, formula) in &readers {
            t.wb.sheet_mut(0).unwrap().cells.set(*at, Cell::formula(Formula::new(formula)));
        }
        t.calc.recalc_all(&mut t.wb);
        (t, readers)
    };
    for (copies, single, expected) in [(1022usize, false, 0usize), (1022, true, 1), (1023, false, 1)] {
        let (mut t, readers) = build(copies, single);
        let dynamic = readers.iter().filter(|(at, _)| t.calc.graph.is_dynamic((0, *at))).count();
        assert_eq!(dynamic, expected, "{copies} copies, one-cell reader {single}");
        t.set("C1", "2");
        assert!((0..copies).all(|k| t.wb.sheet(0).unwrap().value(CellRef::new(k as u32, 0)) == Value::number(1025.0)), "the copies follow C1");
        if single {
            assert_eq!(t.num("B1"), 2.0);
        }
    }
}

fn define(t: &mut T, name: &str, scope: Option<usize>, formula: &str) {
    t.wb.names.push(gridcraft_model::DefinedName { name: name.into(), scope, formula: formula.into(), comment: String::new(), hidden: false });
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

#[test]
fn a_computed_range_counts_a_name_the_formula_already_took() {
    // `named_cell` is taken once by the formula, and again inside a range with a computed end, which
    // covers the cells from A1 to what INDEX returns: the range's box still reaches B5.
    let mut t = T::new();
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "named_cell".into(),
        scope: None,
        formula: "Sheet1!$B$5".into(),
        comment: String::new(),
        hidden: false,
    });
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
fn a_workbook_name_read_from_many_sheets_resolves_on_each_in_one_pass() {
    // A pass keeps one parsed copy of a workbook-wide name, shared by every sheet that reads it;
    // each sheet's unqualified references still resolve on that sheet.
    let mut t = T::new();
    for s in 1..40 {
        t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new(format!("Other{s}"))));
    }
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "here".into(),
        scope: None,
        formula: "$A$1*2".into(),
        comment: String::new(),
        hidden: false,
    });
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
fn repeated_references_are_stored_once() {
    // A formula mentioning a cell three times depends on it once, and a name whose definition
    // mentions one cell 1,100 times contributes it once: within the 1,024-area cap, static.
    let mut t = T::new();
    t.set("A1", "2");
    t.set("B1", "=A1+A1+A1");
    assert_eq!(t.calc.graph.precedents_of((0, c("B1"))).len(), 1);
    t.wb.names.push(gridcraft_model::DefinedName {
        name: "same_cell".into(),
        scope: None,
        formula: vec!["$A$1"; 1100].join("+"),
        comment: String::new(),
        hidden: false,
    });
    t.calc.rebuild(&t.wb);
    t.set("B2", "=same_cell");
    assert_eq!(t.num("B2"), 2200.0);
    assert!(!t.calc.graph.is_dynamic((0, c("B2"))));
    assert_eq!(t.calc.graph.precedents_of((0, c("B2"))).len(), 1);
}
