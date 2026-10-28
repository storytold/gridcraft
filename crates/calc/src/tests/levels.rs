//! Recalculation planned in dependency levels: the plan's order, and the values it gives against a
//! calculation in row-major order.

use super::*;
use crate::Key;

impl T {
    /// Enters every cell, then recalculates the workbook at once: one pass over all of them.
    fn load(&mut self, cells: impl IntoIterator<Item = (String, String)>) {
        for (at, input) in cells {
            let cell = if input.starts_with('=') {
                Cell::formula(Formula::new(&input))
            } else {
                Cell::value(gridcraft_core::parse::parse_input(&input, self.wb.date_system).value)
            };
            self.wb.sheet_mut(0).unwrap().cells.set(c(&at), cell);
        }
        self.calc.recalc_all(&mut self.wb);
    }
    fn name(&mut self, name: &str, formula: &str) {
        self.wb.names.push(gridcraft_model::DefinedName {
            name: name.into(),
            scope: None,
            formula: formula.into(),
            comment: String::new(),
            hidden: false,
        });
    }
}

/// Every stored value, spill and circular reference of `a` and `b` is the same.
fn same_values(a: &T, b: &T, what: &str) {
    assert_eq!(a.wb.sheets.len(), b.wb.sheets.len(), "{what}: sheet count");
    for (i, (sa, sb)) in a.wb.sheets.iter().zip(&b.wb.sheets).enumerate() {
        let mut n = 0;
        for (at, cell) in sa.cells.iter() {
            assert_eq!(cell.value, sb.value(at), "{what}: sheet {i} {}", at.a1());
            n += 1;
        }
        assert_eq!(n, sb.cells.iter().count(), "{what}: sheet {i} cell count");
        assert_eq!(sa.spill, sb.spill, "{what}: sheet {i} spills");
    }
    let (mut ca, mut cb) = (a.calc.circular.clone(), b.calc.circular.clone());
    ca.sort();
    cb.sort();
    assert_eq!(ca, cb, "{what}: circular references");
}

#[test]
fn levels_put_readers_after_what_they_read() {
    let mut t = T::new();
    t.load(
        [("A1", "=1"), ("B1", "=A1+1"), ("C1", "=B1+1"), ("D1", "=SUM(A1:C1)"), ("E1", "=E1+1"), ("F1", "=E1"), ("G1", "=INDIRECT(\"A1\")")]
            .map(|(a, f)| (a.to_string(), f.to_string())),
    );
    let keys: Vec<Key> = ["A1", "B1", "C1", "D1", "E1", "F1", "G1"].iter().map(|a| (0, c(a))).collect();
    let plan = t.calc.graph.levels(&t.wb, &keys).unwrap();
    let levels: Vec<Vec<Key>> = plan.levels().map(|l| l.to_vec()).collect();
    // INDIRECT has no static edge: level 0 with A1.
    assert_eq!(levels, vec![vec![(0, c("A1")), (0, c("G1"))], vec![(0, c("B1"))], vec![(0, c("C1"))], vec![(0, c("D1"))]]);
    // The self-reading E1 and F1 behind it never reach a level.
    assert_eq!(plan.rest(), &[(0, c("E1")), (0, c("F1"))]);
}

#[test]
fn readers_of_a_spill_area_are_planned_after_its_anchor() {
    // B1..B600 read the cells A1 spills over, and Z1, as A1 does. With the spill range known,
    // they are planned after A1 and read its new array in the first round; the write-back then
    // queues them once more (the readers of a changed spill range), as before.
    let book = || {
        let mut t = T::new();
        t.set("Z1", "1");
        t.load((1..=600).map(|r| (format!("B{r}"), format!("=A{r}+Z1"))).chain([("A1".to_string(), "=SEQUENCE(600,1,Z1)".to_string())]));
        assert_eq!((t.num("B1"), t.num("B600")), (2.0, 601.0));
        t.set("Z1", "5");
        assert_eq!((t.num("B1"), t.num("B600")), (10.0, 609.0));
        t
    };
    assert_eq!(book().calc.last_recalc_cells, 601 + 600);
}

#[test]
fn a_chain_of_spill_areas_resolves_in_the_first_round() {
    // Ten spills in a chain, each reading the area (not the anchor) of the one before, laid out
    // so every reader comes before what it reads in row-major order. Nothing is calculated on
    // demand (the areas aren't formulas), so each write-back round settles one link when the
    // formulas run in row-major order: on one thread the 8-round limit leaves the chain stale
    // (A1 = 4193, three links short). Planned in levels, each reader waits for the anchor above
    // the area it reads, so the first round settles every link; the second runs the readers of
    // the new arrays once more and finds nothing changed.
    let mut t = T::new();
    let mut cells = vec![("K1".to_string(), "=SEQUENCE(600,1,Z1)".to_string()), ("Z1".to_string(), "1".to_string())];
    for (i, col) in "JIHGFEDCBA".chars().enumerate() {
        let prev = "KJIHGFEDCB".chars().nth(i).unwrap_or('K');
        cells.push((format!("{col}1"), format!("=SEQUENCE(600,1,MAX({prev}2:{prev}600))")));
    }
    t.load(cells);
    assert_eq!(t.num("A1"), 1.0 + 10.0 * 599.0);
    assert_eq!(t.num("A600"), 600.0 + 10.0 * 599.0);
    assert_eq!(t.calc.last_recalc_cells, 11 + 10);
}

#[test]
fn a_chain_of_spill_areas_laid_out_in_row_major_order_settles_as_on_main() {
    // Sixteen spills in row 1, A to P, each sized by how much of the one before it is filled
    // (its area, not its anchor), and each multiplied by a link of a chain in column Z that puts
    // A1 in the deepest level and P1 in the first. Run backwards, each spill is laid out one row
    // tall, which its reader (reading from row 2) doesn't read, so a write-back round would
    // reach only the next link and the round limit would leave the last spills one row tall.
    // Each reader waits for the anchor above the area it reads, so the levels run the chain
    // forwards and the first round settles it, as row-major order does.
    let mut t = T::new();
    let mut cells = vec![("Z100".to_string(), "=1".to_string())];
    for row in 101..=116 {
        cells.push((format!("Z{row}"), format!("=Z{}", row - 1)));
    }
    let columns = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P"];
    for (i, col) in columns.iter().enumerate() {
        let link = format!("Z{}", 116 - i);
        let formula = match i.checked_sub(1).and_then(|p| columns.get(p)) {
            Some(prev) => format!("=SEQUENCE(COUNT({prev}2:{prev}20)+1)*{link}"),
            None => format!("=SEQUENCE(5)*{link}"),
        };
        cells.push((format!("{col}1"), formula));
    }
    t.load(cells);
    // A1 spills five rows, so each reader counts four and spills five too.
    for col in columns {
        let height = (1..=30).take_while(|r| !matches!(t.get(&format!("{col}{r}")), Value::Empty)).count();
        assert_eq!(height, 5, "{col}1");
    }
}

#[test]
fn an_indirect_reader_of_a_spill_sized_by_another_spill_is_right_after_a_full_recalc() {
    // A1 spills three rows once Z2 is known; C1's height comes from A3, a cell A1 spills over,
    // and B1 reads C3 through INDIRECT, with no static link to either, so it is in the first
    // level, before both arrays. Formulas like B1 are calculated once more after the first round
    // that lays out an array, and again after a planned round in which an array changed, so B1
    // reads C3 once C1 has spilled over it.
    let mut t = T::new();
    t.load(
        [("Z1", "3"), ("Z2", "=Z1"), ("A1", "=SEQUENCE(Z2)"), ("B1", "=INDIRECT(\"C3\")"), ("C1", "=SEQUENCE(MAX(1,N(A3)))")]
            .map(|(at, input)| (at.to_string(), input.to_string())),
    );
    assert_eq!(t.num("C3"), 3.0);
    assert_eq!(t.num("B1"), 3.0, "after loading");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.num("B1"), 3.0, "after a second full recalc");
    t.set("Z1", "2");
    t.set("Z1", "3");
    assert_eq!(t.num("B1"), 3.0, "after two edits");
}

#[test]
fn formulas_over_filled_columns_dont_wait_for_the_formula_above_them() {
    // Totals at the top of two columns, each reading the other's column from row 3: the formula
    // above the area read can spill into it only if the cell under it and the top of the area
    // are both empty. Then each waits for the other and both come after the levels; with either
    // cell filled the spill would be blocked, neither waits, and nothing falls behind a loop.
    let plan_of = |filled: &[u32]| {
        let mut t = T::new();
        let mut cells = vec![("B1".to_string(), "=SUM(C3:C40)".to_string()), ("C1".to_string(), "=SUM(B3:B40)".to_string())];
        for &r in filled {
            cells.push((format!("B{r}"), r.to_string()));
            cells.push((format!("C{r}"), (2 * r).to_string()));
        }
        t.load(cells);
        let plan = t.calc.graph.levels(&t.wb, &[(0, c("B1")), (0, c("C1"))]).unwrap();
        (plan.rest().len(), t.num("B1"))
    };
    let below: Vec<u32> = (4..=40).collect();
    assert_eq!(plan_of(&below), (2, 2.0 * f64::from((4..=40).sum::<u32>())), "rows 2 and 3 empty: the two wait for each other");
    let under_filled: Vec<u32> = std::iter::once(2).chain(4..=40).collect();
    assert_eq!(plan_of(&under_filled).0, 0, "the cell under each total holds a value");
    let top_filled: Vec<u32> = (3..=40).collect();
    assert_eq!(plan_of(&top_filled).0, 0, "the top of each area holds a value");
}

#[test]
fn wide_readers_of_overlapping_ranges_are_leveled() {
    // Each reader in D sums A1:A1500 and A2:A1501, and each in E looks up in A1:C1501 and
    // A1:B1501, so a reader counts the formulas of A in both of its areas: more than will ever
    // complete. They are leveled after A all the same, not left for after the levels with what
    // reads them, where they would run on the main thread as if behind a cycle.
    let mut t = T::new();
    t.load(
        (1..=1501)
            .map(|r| (format!("A{r}"), "=ROW()".to_string()))
            .chain((1..=200).map(|r| (format!("D{r}"), "=SUM($A$1:$A$1500)+SUM($A$2:$A$1501)+ROW()".to_string())))
            .chain((1..=200).map(|r| (format!("E{r}"), "=VLOOKUP(ROW(),$A$1:$C$1501,1,FALSE)+VLOOKUP(ROW(),$A$1:$B$1501,1,FALSE)".to_string())))
            .chain([("G1".to_string(), "=D1+E1".to_string())]),
    );
    let total: f64 = (1..=1500).map(f64::from).sum();
    assert_eq!((t.num("D1"), t.num("E1"), t.num("G1")), (2.0 * total + 1501.0, 2.0, 2.0 * total + 1503.0));
    let keys: Vec<Key> = (1..=1501)
        .map(|r| (0, c(&format!("A{r}"))))
        .chain((1..=200).flat_map(|r| [(0, c(&format!("D{r}"))), (0, c(&format!("E{r}")))]))
        .chain([(0, c("G1"))])
        .collect();
    let plan = t.calc.graph.levels(&t.wb, &keys).unwrap();
    let sizes: Vec<usize> = plan.levels().map(<[Key]>::len).collect();
    assert_eq!((sizes, plan.rest().len()), (vec![1501, 400, 1], 0), "A, then the readers, then G1; nothing after the levels");
}

#[test]
fn a_long_chain_is_one_formula_per_level() {
    let n = 20_000;
    let mut t = T::new();
    t.load((1..n).map(|r| (format!("C{r}"), format!("=C{}+1", r + 1))).chain([(format!("C{n}"), "=1".to_string())]));
    assert_eq!(t.num("C1"), f64::from(n));
    assert!(t.calc.circular.is_empty());
    t.set(&format!("C{n}"), "=10");
    assert_eq!(t.num("C1"), f64::from(n + 9));
}

fn xorshift(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn a1(row: u32, col: u32) -> String {
    format!("{}{}", char::from(b'A' + col as u8), row + 1)
}

/// An acyclic sheet whose formulas read cells entered before them, placed anywhere (so row-major
/// order isn't a topological order) and mostly recent ones (so they read chains of one another),
/// directly, through ranges, defined names, INDIRECT and OFFSET. Returns the cells in entry order
/// and the names.
/// Cell inputs, as (address, input) pairs.
type Inputs = Vec<(String, String)>;

fn random_acyclic_sheet(seed: u64, rows: u32, cols: u32) -> (Inputs, Inputs) {
    let mut s = seed.max(1);
    let mut cells: Vec<(u32, u32)> = (0..rows).flat_map(|r| (0..cols).map(move |c| (r, c))).collect();
    for i in (1..cells.len()).rev() {
        let j = (xorshift(&mut s) % (i as u64 + 1)) as usize;
        cells.swap(i, j);
    }
    let rank: std::collections::HashMap<(u32, u32), usize> = cells.iter().enumerate().map(|(i, c)| (*c, i)).collect();
    let mut out = Vec::new();
    let mut names = Vec::new();
    for (i, &(r, c)) in cells.iter().enumerate() {
        if i < 20 || xorshift(&mut s).is_multiple_of(5) {
            out.push((a1(r, c), format!("{}", xorshift(&mut s) % 100)));
            continue;
        }
        let pick = |s: &mut u64| {
            let back = if xorshift(s).is_multiple_of(4) { xorshift(s) % i as u64 } else { 1 + xorshift(s) % 6 };
            cells[i - back.max(1) as usize]
        };
        let mut terms = Vec::new();
        for _ in 0..1 + xorshift(&mut s) % 3 {
            let (pr, pc) = pick(&mut s);
            // A column range ending at (pr, pc), if every cell in it was entered before.
            let top = pr.saturating_sub((xorshift(&mut s) % 4) as u32);
            let range = (top..=pr).all(|rr| rank.get(&(rr, pc)).is_some_and(|&k| k < i)).then(|| (a1(top, pc), a1(pr, pc)));
            terms.push(match xorshift(&mut s) % 7 {
                0 => format!("INDIRECT(\"{}\")", a1(pr, pc)),
                1 => {
                    let name = format!("Name_{}", names.len());
                    names.push((name.clone(), format!("Sheet1!${}${}", char::from(b'A' + pc as u8), pr + 1)));
                    name
                }
                2 => match &range {
                    Some((a, b)) => format!("SUM({a}:{b})"),
                    None => a1(pr, pc),
                },
                // A defined name over the range.
                3 => match &range {
                    Some(_) => {
                        let name = format!("Range_{}", names.len());
                        let col = char::from(b'A' + pc as u8);
                        names.push((name.clone(), format!("Sheet1!${col}${}:${col}${}", top + 1, pr + 1)));
                        format!("SUM({name})")
                    }
                    None => a1(pr, pc),
                },
                // OFFSET with a computed offset (a dynamic reference).
                4 => format!("N(OFFSET($A$1,{pr}+0*ROW(),{pc}))"),
                5 => match &range {
                    Some((a, b)) => format!("INDEX({a}:{b},1)"),
                    None => a1(pr, pc),
                },
                _ => a1(pr, pc),
            });
        }
        out.push((a1(r, c), format!("=MOD({},1000)", terms.join("+"))));
    }
    (out, names)
}

#[test]
fn a_plan_given_up_for_its_cost_gives_the_same_values_in_row_major_order() {
    // The budget is generous (a million units plus hundreds per formula); a test caps it at
    // nothing, so planning is given up at the first formula and the pass runs row-major with
    // the usual on-demand evaluation.
    for seed in [3u64, 11] {
        let (cells, names) = random_acyclic_sheet(seed * 104_729, 300, 8);
        let book = |cap: Option<usize>| {
            let mut t = T::new();
            t.calc.plan_budget = cap;
            for (name, formula) in &names {
                t.name(name, formula);
            }
            t.load(cells.iter().cloned());
            t
        };
        let planned = book(None);
        let keys: Vec<Key> = planned.wb.sheet(0).unwrap().cells.iter().filter(|(_, cell)| cell.formula.is_some()).map(|(at, _)| (0, at)).collect();
        assert!(planned.calc.graph.levels(&planned.wb, &keys).is_some());
        assert!(planned.calc.graph.levels_within(&planned.wb, &keys, Some(0)).is_none());
        let fallback = book(Some(0));
        same_values(&planned, &fallback, &format!("seed {seed}, no plan"));
    }
}

#[test]
fn levels_follow_precedents_across_sheets_and_whole_columns() {
    let mut t = T::new();
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    t.set("A1", "=1");
    t.wb.sheet_mut(1).unwrap().cells.set(c("B1"), Cell::formula(Formula::new("=SUM(Sheet1!A:A)")));
    t.wb.sheet_mut(0).unwrap().cells.set(c("C1"), Cell::formula(Formula::new("=Data!B1+SUM(1:1)")));
    t.calc.recalc_all(&mut t.wb);
    let keys = vec![(0, c("A1")), (1, c("B1")), (0, c("C1"))];
    let plan = t.calc.graph.levels(&t.wb, &keys).unwrap();
    let levels: Vec<Vec<Key>> = plan.levels().map(|l| l.to_vec()).collect();
    // C1 reads its own row: a cycle, never leveled.
    assert_eq!(levels, vec![vec![(0, c("A1"))], vec![(1, c("B1"))]]);
    assert_eq!(plan.rest(), &[(0, c("C1"))]);
    assert_eq!(t.get("C1"), Value::Error(CellError::Circ));
}

#[test]
fn a_wide_reader_waiting_only_on_a_spill_range_is_leveled_when_the_anchor_is_done() {
    // R reads 100 dirty formulas in column A (a wide reader, recounted per level) and part of
    // the area S spills over, not including S's own column. Once the A formulas are done R's
    // count is zero; it then waits only on S, whose completion moves no column R reads, so a
    // recount gated on moved columns alone would never release it.
    let mut t = T::new();
    t.set("Z1", "1");
    let mut cells: Vec<(String, String)> = (1..=100).map(|r| (format!("A{r}"), "=ROW()+Z1".to_string())).collect();
    cells.push(("C5".to_string(), "=SEQUENCE(6,4,A1)".to_string()));
    cells.push(("H1".to_string(), "=SUM(A1:A100)+SUM(E5:F10)".to_string()));
    t.load(cells);
    // Now C5 spills C5:F10, so the plan has the spill edge.
    let keys: Vec<Key> = (1..=100).map(|r| (0, c(&format!("A{r}")))).chain([(0, c("C5")), (0, c("H1"))]).collect();
    let plan = t.calc.graph.levels(&t.wb, &keys).unwrap();
    let levels: Vec<Vec<Key>> = plan.levels().map(|l| l.to_vec()).collect();
    assert!(plan.rest().is_empty(), "rest: {:?}", plan.rest());
    assert_eq!(levels.len(), 3);
    assert_eq!(levels[1], vec![(0, c("C5"))]);
    assert_eq!(levels[2], vec![(0, c("H1"))]);
    t.set("Z1", "2");
    let a: f64 = (1..=100).map(|r| f64::from(r) + 2.0).sum();
    // C5:F10 holds 3..26 row by row (A1 is 3 now); E and F of row r hold 5+4r and 6+4r.
    let spill: f64 = (0..6).map(|r| f64::from(11 + 8 * r)).sum();
    assert_eq!(t.num("H1"), a + spill);
}

#[test]
fn wide_readers_of_a_long_chains_end_cost_the_planner_little() {
    // A 20K running-total chain and 100 readers of its last rows: one level per link, and a
    // wide reader may only be counted again after as many completions as it had pending.
    let n = 20_000u32;
    let mut t = T::new();
    let mut cells: Vec<(String, String)> = Vec::new();
    cells.push(("H1".to_string(), "=1".to_string()));
    for r in 2..=n {
        cells.push((format!("H{r}"), format!("=H{}+1", r - 1)));
    }
    for i in 1..=100 {
        cells.push((format!("K{i}"), format!("=SUM(H{}:H{n})+ROW()", n - 100)));
    }
    t.load(cells);
    assert_eq!(t.num("K100"), (n - 100..=n).map(f64::from).sum::<f64>() + 100.0);
    let keys: Vec<Key> = (1..=n).map(|r| (0, c(&format!("H{r}")))).chain((1..=100).map(|i| (0, c(&format!("K{i}"))))).collect();
    let plan = t.calc.graph.levels(&t.wb, &keys).unwrap();
    assert_eq!(plan.levels().count(), n as usize + 1);
    assert!(plan.rest().is_empty());
    assert!(plan.work() < 12 * keys.len(), "{} index work for {} formulas", plan.work(), keys.len());
}
