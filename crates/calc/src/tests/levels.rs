//! Recalculation planned in dependency levels: the plan's order, and the values it gives against a
//! calculation in row-major order.

use super::*;
use crate::Key;
use crate::recalc::{NoPlan, Redo};

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

/// Every stored cell, value, spill and circular reference of `a` and `b` is the same.
fn same_values(a: &T, b: &T, what: &str) {
    if let Err(why) = first_difference(a, b) {
        panic!("{what}: {why}");
    }
}

/// The first difference [`same_values`] looks for: the same addresses hold cells in both, with
/// the same stored values, and the spills and circular references are the same.
fn first_difference(a: &T, b: &T) -> Result<(), String> {
    if a.wb.sheets.len() != b.wb.sheets.len() {
        return Err(format!("sheet count {} and {}", a.wb.sheets.len(), b.wb.sheets.len()));
    }
    for (i, (sa, sb)) in a.wb.sheets.iter().zip(&b.wb.sheets).enumerate() {
        let mut n = 0;
        for (at, cell) in sa.cells.iter() {
            let Some(other) = sb.cell(at) else { return Err(format!("sheet {i} {} holds a cell only in the first", at.a1())) };
            if cell.value != other.value {
                return Err(format!("sheet {i} {}: {:?} and {:?}", at.a1(), cell.value, other.value));
            }
            n += 1;
        }
        // Every address of `a` holds a cell in `b`: with the same count, they hold the same ones.
        let m = sb.cells.iter().count();
        if n != m {
            return Err(format!("sheet {i} cell count {n} and {m}"));
        }
        if sa.spill != sb.spill {
            return Err(format!("sheet {i} spills {:?} and {:?}", sa.spill, sb.spill));
        }
    }
    let (mut ca, mut cb) = (a.calc.circular.clone(), b.calc.circular.clone());
    ca.sort();
    cb.sort();
    if ca != cb {
        return Err(format!("circular references {ca:?} and {cb:?}"));
    }
    Ok(())
}

#[test]
fn the_comparison_finds_a_cell_held_at_another_address() {
    // One cell each, at different addresses: an empty styled cell at C3 in one, a number at D4 in
    // the other. The counts match, and C3 reads as empty in both, but the cells differ.
    let (mut one, mut other) = (T::new(), T::new());
    one.wb.sheet_mut(0).unwrap().cells.set(c("C3"), Cell::styled(Value::Empty, gridcraft_model::StyleId(1)));
    other.wb.sheet_mut(0).unwrap().cells.set(c("D4"), Cell::value(Value::Number(7.0)));
    assert_eq!(first_difference(&one, &other), Err("sheet 0 C3 holds a cell only in the first".to_string()));
    assert_eq!(first_difference(&one, &one), Ok(()));
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
fn a_chain_of_spill_areas_read_before_they_are_laid_out_stays_stale_as_in_row_major_order() {
    // Ten spills in a chain, each reading the area (not the anchor) of the one before, laid out
    // so every reader comes before what it reads in row-major order. Nothing is calculated on
    // demand (the areas aren't formulas), so each write-back round settles one link in row-major
    // order, and the 8-round limit leaves the chain three links short: A1 = 4193 where the
    // settled chain gives 5991. That is the value row-major order gives, and it is stale
    // (upstream issue #2320). The plan would settle the chain in one round, but a planned round
    // gives what the row-major round gives, so the planned run ends where row-major order ends.
    // A fix of #2320 on the row-major path flips this test to 5991.
    let mut t = T::new();
    let mut cells = vec![("K1".to_string(), "=SEQUENCE(600,1,Z1)".to_string()), ("Z1".to_string(), "1".to_string())];
    for (i, col) in "JIHGFEDCBA".chars().enumerate() {
        let prev = "KJIHGFEDCB".chars().nth(i).unwrap_or('K');
        cells.push((format!("{col}1"), format!("=SEQUENCE(600,1,MAX({prev}2:{prev}600))")));
    }
    t.load(cells.clone());
    assert_eq!(t.num("A1"), 4193.0, "row-major order's stale value (#2320)");
    assert_eq!(t.num("A600"), 4193.0 + 599.0);
    let mut row_major = T::new();
    row_major.calc.plan_budget = Some(0);
    row_major.load(cells);
    same_values(&t, &row_major, "against row-major order");
}

#[test]
fn a_chain_of_spill_areas_laid_out_in_row_major_order_settles_in_the_first_round() {
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
    assert_eq!(t.calc.rounds_redone_in_row_major, 0, "the levels settle the chain by themselves");
}

/// Sixteen anchors in column A, each spilling one row to the right, sized by how much of the row
/// above is filled (its area B:T, not its anchor), and multiplied by a link of a chain in column Z
/// that puts A1 in the deepest level and A16 in the first. `first` is A1's formula.
fn sideways_chain(first: &str) -> Vec<(String, String)> {
    sideways_chain_of(16, first)
}

/// The sideways chain with `links` anchors in column A and a Z chain from Z100 to Z(100 + links):
/// A1's link is Z(100 + links).
fn sideways_chain_of(links: u32, first: &str) -> Vec<(String, String)> {
    let mut cells = vec![("Z100".to_string(), "1".to_string())];
    for row in 101..=100 + links {
        cells.push((format!("Z{row}"), format!("=Z{}", row - 1)));
    }
    cells.push(("A1".to_string(), first.to_string()));
    for row in 2..=links {
        cells.push((format!("A{row}"), format!("=SEQUENCE(1,COUNT(B{}:T{})+1)*Z{}", row - 1, row - 1, 101 + links - row)));
    }
    cells
}

/// How many cells of row 1..=16 are filled from column A rightwards, up to T.
fn sideways_widths(t: &T) -> Vec<usize> {
    sideways_widths_of(t, 16)
}

/// How many cells of row 1..=`links` are filled from column A rightwards, up to T.
fn sideways_widths_of(t: &T, links: u32) -> Vec<usize> {
    let columns = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S", "T"];
    (1..=links).map(|r| columns.iter().take_while(|col| !matches!(t.get(&format!("{col}{r}")), Value::Empty)).count()).collect()
}

#[test]
fn a_chain_of_arrays_spilling_right_into_the_row_below_settles_in_a_row_major_round() {
    // A{r} reads B:T of the row above, which A{r-1} spills into from the left: not under any
    // formula of those columns, so the planner doesn't hold A{r} back, and the chain in Z puts
    // A16 first. Planned, each reader comes before the array it reads, which row-major order
    // calculates first: the round is calculated again in row-major order, which settles the
    // chain at once.
    let mut t = T::new();
    t.load(sideways_chain("=SEQUENCE(1,5)*Z116"));
    assert_eq!(sideways_widths(&t), vec![5; 16]);
    assert_eq!((t.num("A16"), t.num("E16")), (1.0, 5.0));
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order]);
}

#[test]
fn a_chain_of_arrays_spilling_right_into_the_row_below_settles_after_an_edit() {
    // The same chain with A1 sized by the end of the Z chain: one wide everywhere after loading,
    // then Z100 = 5 makes A1 five wide, and every row below follows.
    let mut t = T::new();
    t.load(sideways_chain("=SEQUENCE(1,Z116)"));
    assert_eq!(sideways_widths(&t), vec![1; 16], "after loading");
    t.set("Z100", "5");
    assert_eq!(sideways_widths(&t), vec![5; 16], "after the edit");
    assert_eq!((t.num("A16"), t.num("E16")), (5.0, 25.0));
}

#[test]
fn a_chain_of_arrays_spilling_down_right_into_a_corner_area_settles_in_a_row_major_round() {
    // Twelve anchors on a diagonal, three rows and columns apart (A1, D4, G7, ...). Each one is
    // three by three when the two by two block diagonally below its predecessor holds a number
    // (which only the predecessor's three by three array puts there), else one cell. That block
    // is in neither the predecessor's row nor its column, and nothing dirty stands above it, so
    // the planner doesn't hold the reader back; the chain in Z puts the first anchor last.
    let columns = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R"];
    let more = ["S", "T", "U", "V", "W", "X", "Y", "Z", "AA", "AB", "AC", "AD", "AE", "AF", "AG", "AH", "AI", "AJ"];
    let column = |i: usize| columns.iter().chain(more.iter()).nth(i).copied().unwrap();
    let anchor = |link: usize| (link * 3 + 1, link * 3);
    let mut t = T::new();
    let mut cells = vec![("Z100".to_string(), "1".to_string())];
    for row in 101..=112 {
        cells.push((format!("Z{row}"), format!("=Z{}", row - 1)));
    }
    for link in 0..12 {
        let (row, col) = anchor(link);
        let factor = format!("Z{}", 112 - link);
        let formula = match link.checked_sub(1).map(anchor) {
            None => format!("=SEQUENCE(3,3)*{factor}"),
            Some((above, left)) => {
                let block = format!("{}{}:{}{}", column(left + 1), above + 1, column(left + 2), above + 2);
                format!("=SEQUENCE(IF(COUNT({block})>0,3,1),IF(COUNT({block})>0,3,1))*{factor}")
            }
        };
        cells.push((format!("{}{row}", column(col)), formula));
    }
    t.load(cells);
    let filled: Vec<usize> = (0..12)
        .map(|link| {
            let (row, col) = anchor(link);
            (0..3)
                .flat_map(|dr| (0..3).map(move |dc| (row + dr, col + dc)))
                .filter(|&(r, c)| !matches!(t.get(&format!("{}{r}", column(c))), Value::Empty))
                .count()
        })
        .collect();
    assert_eq!(filled, vec![9; 12]);
    // The last anchor is at AH34: its corner, AJ36, is the ninth number.
    assert_eq!(t.num("AJ36"), 9.0);
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order]);
}

#[test]
fn one_reader_of_a_new_array_costs_one_small_round_not_the_first_round_again() {
    // A1:A2000 are formulas, E2 spills three columns and depends on A1, and H1 reads F3, a cell
    // only E2's array covers (to the right of its anchor, under no formula of column F). H1 is
    // planned before E2 and reads F3 empty; one more round with H1 alone settles it, as the
    // write-back rounds do in row-major order. The first round is not calculated twice.
    let mut t = T::new();
    let mut cells: Vec<(String, String)> = (1..=2000).map(|r| (format!("A{r}"), "=ROW()*2".to_string())).collect();
    cells.push(("E2".to_string(), "=SEQUENCE(5,3)+A1".to_string()));
    cells.push(("H1".to_string(), "=F3+1".to_string()));
    t.load(cells);
    assert_eq!(t.num("F3"), 7.0);
    assert_eq!(t.num("H1"), 8.0);
    assert_eq!(t.calc.formulas_calculated, 2000 + 1 + 1 + 1, "the A column, E2, H1, then H1 once more");
}

#[test]
fn an_indirect_reader_of_a_spill_sized_by_another_spill_stays_stale_after_edits_as_in_row_major_order() {
    // A1 spills three rows once Z2 is known; C1's height comes from A3, a cell A1 spills over,
    // and B1 reads C3 through INDIRECT, with no static link to either. Dynamic formulas like B1
    // run once more after the first round that lays out an array, as without a plan: that is
    // enough after loading and after a full recalculation, but after the two edits C1 spills over
    // C3 a round after B1's rerun, and B1 keeps 0. That is the value row-major order gives, and
    // it is stale (upstream issue #4469: Excel shows 3 at once); the planned run gives the same.
    // A fix of #4469 on the row-major path flips the last assertion to 3.
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
    assert_eq!(t.num("B1"), 0.0, "after two edits: row-major order's stale value (#4469)");
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
fn planned_random_draws_go_by_level_and_the_cells_computed_from_them_follow() {
    // The stated exception to row-major order's values: while a workbook is planned, random draws
    // go in level order, and the cells computed from the draws follow the values drawn.
    // A1 reads Z1, so it is a level after C1 and draws second when planned, first in row-major order.
    let cells = [("Z1", "=1"), ("A1", "=RAND()+Z1*0"), ("C1", "=RAND()"), ("D1", "=A1<C1")];
    let book = |plan_budget: Option<usize>| {
        let mut t = T::new();
        t.calc.plan_budget = plan_budget;
        t.load(cells.iter().map(|(at, input)| (at.to_string(), input.to_string())));
        t
    };
    let shown = |t: &T| (t.num("A1"), t.num("C1"), t.get("D1"));
    let planned = book(None);
    assert!(planned.calc.plans_made > 0, "the load was planned");
    let (a1, c1, d1) = shown(&planned);
    assert_ne!(a1, c1, "two draws");
    assert_eq!(d1, Value::Bool(a1 < c1), "planned: D1 compares the values A1 and C1 show");
    let row_major = book(Some(0));
    assert_eq!(row_major.calc.plans_made, 0, "row-major order: no plan");
    let (rm_a1, rm_c1, rm_d1) = shown(&row_major);
    assert_eq!((rm_a1, rm_c1), (c1, a1), "row-major order: the two draws change places");
    assert_eq!(rm_d1, Value::Bool(rm_a1 < rm_c1), "row-major order: D1 compares the values A1 and C1 show");
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

/// A loop the plan can't see: A1 reads B2, and B2 reads A1 only through `closing` (INDIRECT or
/// OFFSET) and C1 = 1 plainly. A plan would put B2 first; row-major order enters at A1. Iterative
/// calculation on with `iterations` at most; `budget` caps planning (`Some(0)`: row-major order).
fn hidden_loop(closing: &str, iterations: u32, budget: Option<usize>) -> T {
    let mut t = T::new();
    t.wb.calc.iterative = true;
    t.wb.calc.max_iterations = iterations;
    t.calc.plan_budget = budget;
    let cells = [("C1", "1"), ("A1", "=B2*0.9+1"), ("B2", closing)];
    t.load(cells.iter().map(|(a, f)| (a.to_string(), f.to_string())));
    t
}

const LOOP_THROUGH_INDIRECT: &str = "=INDIRECT(\"A1\")*0.9+C1";
const LOOP_THROUGH_OFFSET: &str = "=OFFSET(C1,0,-2)*0.9+C1";

#[test]
fn a_loop_through_a_dynamic_reference_is_entered_at_its_first_cell_in_row_major_order() {
    // One iteration shows where the loop is entered: A1 comes first and calculates B2 on
    // demand, which reads A1's previous value (0), so B2 = 1 and A1 = 1.9, as without a plan. Entered
    // at B2, the values would be the other way round.
    for closing in [LOOP_THROUGH_INDIRECT, LOOP_THROUGH_OFFSET] {
        let t = hidden_loop(closing, 1, None);
        assert_eq!((t.num("A1"), t.num("B2")), (1.9, 1.0), "{closing}");
    }
}

#[test]
fn a_loop_through_a_dynamic_reference_iterates_as_in_row_major_order_after_an_edit_and_a_full_recalc() {
    for closing in [LOOP_THROUGH_INDIRECT, LOOP_THROUGH_OFFSET] {
        let (mut t, mut row_major) = (hidden_loop(closing, 20, None), hidden_loop(closing, 20, Some(0)));
        same_values(&t, &row_major, &format!("{closing}, opened"));
        t.set("C1", "2");
        row_major.set("C1", "2");
        same_values(&t, &row_major, &format!("{closing}, C1 edited"));
        assert!((t.num("A1") - 14.664642651587922).abs() < 1e-9, "{closing}: A1 = {}", t.num("A1"));
        assert!((t.num("B2") - 15.182936279542135).abs() < 1e-9, "{closing}: B2 = {}", t.num("B2"));
        t.calc.recalc_all(&mut t.wb);
        row_major.calc.recalc_all(&mut row_major.wb);
        same_values(&t, &row_major, &format!("{closing}, full recalculation"));
    }
}

#[test]
fn iterative_calculation_makes_no_plan() {
    // The sideways chain's planned round is calculated again in row-major order
    // (`a_chain_of_arrays_spilling_right_into_the_row_below_settles_in_a_row_major_round`); with
    // iteration on there is no plan, so nothing is calculated twice, and the values are those
    // of a pass in row-major order.
    let book = |budget: Option<usize>| {
        let mut t = T::new();
        t.wb.calc.iterative = true;
        t.calc.plan_budget = budget;
        t.load(sideways_chain("=SEQUENCE(1,5)*Z116"));
        t
    };
    let (iterative, row_major) = (book(None), book(Some(0)));
    assert_eq!(iterative.calc.rounds_redone_in_row_major, 0, "no plan, no round calculated again");
    same_values(&iterative, &row_major, "iterative, against row-major order");
}

/// A seeded sheet of eight rows by six columns for iterative calculation: numbers, and formulas
/// reading cells anywhere in the grid directly, through INDIRECT and through OFFSET, so loops
/// close every way and some are hidden from the plan. Most are damped and settle; a formula
/// adding 1 to what it reads makes a loop that never does. Returns the cells and the cells
/// holding numbers.
fn random_iterative_sheet(seed: u64) -> (Inputs, Vec<String>) {
    let mut s = seed.max(1);
    let mut cells = Vec::new();
    let mut numbers = Vec::new();
    for row in 0..8 {
        for col in 0..6 {
            let roll = xorshift(&mut s) % 100;
            let at = a1(row, col);
            if roll < 20 {
                numbers.push(at.clone());
                cells.push((at, format!("{}", xorshift(&mut s) % 7)));
            } else if roll < 75 {
                let (ar, ac) = ((xorshift(&mut s) % 8) as u32, (xorshift(&mut s) % 6) as u32);
                let (a, b) = (a1(ar, ac), a1((xorshift(&mut s) % 8) as u32, (xorshift(&mut s) % 6) as u32));
                let formula = match xorshift(&mut s) % 7 {
                    0 => format!("=INDIRECT(\"{a}\")*0.5+1"),
                    1 => format!("=OFFSET($A$1,{ar},{ac})*0.5+{b}*0.25"),
                    2 => format!("={a}*0.5+1"),
                    3 => format!("=INDIRECT(\"{a}\")*0.25+{b}*0.5"),
                    4 => format!("={a}*0.25+{b}*0.25+1"),
                    5 => format!("=N(OFFSET($A$1,{ar},{ac}))*0.5+{b}*0.25"),
                    _ => format!("={a}+1"),
                };
                cells.push((at, formula));
            }
        }
    }
    (cells, numbers)
}

#[test]
fn random_iterative_workbooks_calculate_as_in_row_major_order() {
    // Seeded sheets with iterative calculation on, half of them stopped at 3 iterations (so
    // loops end unsettled) and half at 100: after opening, after each of three edits and after a
    // full recalculation, the default run matches a run in row-major order without a plan, in
    // values, spills and circular references. `GRIDCRAFT_ITERATIVE_FUZZ_CASES` soaks it over more.
    let cases: u64 = std::env::var("GRIDCRAFT_ITERATIVE_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    for case in 0..cases {
        let seed = (case + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let (cells, numbers) = random_iterative_sheet(seed);
        let mut s = seed.max(1);
        let edits: Vec<(String, String)> = (0..3)
            .map(|_| {
                let at = if numbers.is_empty() {
                    a1((xorshift(&mut s) % 8) as u32, (xorshift(&mut s) % 6) as u32)
                } else {
                    numbers[(xorshift(&mut s) % numbers.len() as u64) as usize].clone()
                };
                (at, format!("{}", 10 + xorshift(&mut s) % 9))
            })
            .collect();
        let book = |budget: Option<usize>| {
            let mut t = T::new();
            t.wb.calc.iterative = true;
            t.wb.calc.max_iterations = if case % 2 == 0 { 3 } else { 100 };
            t.calc.plan_budget = budget;
            t.load(cells.iter().cloned());
            t
        };
        let (mut planned, mut row_major) = (book(None), book(Some(0)));
        same_values(&planned, &row_major, &format!("case {case} ({cells:?}), opened"));
        for (at, value) in &edits {
            planned.set(at, value);
            row_major.set(at, value);
            same_values(&planned, &row_major, &format!("case {case} ({cells:?}), {at} = {value}"));
        }
        planned.calc.recalc_all(&mut planned.wb);
        row_major.calc.recalc_all(&mut row_major.wb);
        same_values(&planned, &row_major, &format!("case {case} ({cells:?}), full recalculation"));
    }
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
        assert!(planned.calc.graph.levels(&planned.wb, &keys).is_ok());
        assert_eq!(planned.calc.graph.levels_within(&planned.wb, &keys, Some(0)).err(), Some(NoPlan::Other));
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

#[test]
fn a_chain_of_arrays_read_by_counts_below_settles_in_row_major_order() {
    // The sideways chain, plus ten formulas below it counting everything rows 1 to 16 spill
    // into: the planned round has the links read before they are laid out, so it is calculated
    // again in row-major order, which settles the chain and the counts at once.
    let mut t = T::new();
    let mut cells = sideways_chain("=SEQUENCE(1,5)*Z116");
    for row in 20..=29u32 {
        cells.push((format!("V{row}"), "=COUNT($B$1:$T$16)+ROW()".to_string()));
    }
    t.load(cells);
    assert_eq!(sideways_widths(&t), vec![5; 16]);
    assert_eq!((t.num("A16"), t.num("E16")), (1.0, 5.0));
    for row in 20..=29u32 {
        assert_eq!(t.num(&format!("V{row}")), 64.0 + f64::from(row), "V{row}");
    }
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order]);
}

#[test]
fn a_chain_of_a_hundred_arrays_settles_in_one_row_major_round() {
    // The sideways chain with 100 links and nothing else: planned backwards, so the round is
    // calculated again in row-major order, which lays the links out in order.
    let mut t = T::new();
    t.load(sideways_chain_of(100, "=SEQUENCE(1,5)*Z200"));
    assert_eq!(sideways_widths_of(&t, 100), vec![5; 100]);
    assert_eq!((t.num("A100"), t.num("E100")), (1.0, 5.0));
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order]);
}

#[test]
fn arrays_that_never_settle_through_indirect_end_as_in_row_major_order() {
    // Three formulas in column A, each one cell wide when the two cells right of the one before it
    // (A30 for A10) hold a number and spilling three wide otherwise, in a loop: whatever one does,
    // the next undoes, so each round dirties a few of them again and the arrangement never
    // settles. A10 reads A30's cells through INDIRECT, a loop the planner can't see. Z5 puts A10
    // in a later level than A20, which reads B10:C10; 5,000 more formulas in column AZ make the
    // first round large. The pass takes at most 8 rounds, each ending as in row-major order.
    let cells = |row_major: bool| {
        let mut t = T::new();
        if row_major {
            t.calc.plan_budget = Some(0);
        }
        let mut cells: Vec<(String, String)> = (1..=5_000).map(|r| (format!("AZ{r}"), "=ROW()*2".to_string())).collect();
        cells.push(("Z5".to_string(), "=1".to_string()));
        cells.push(("A10".to_string(), "=IF(COUNT(INDIRECT(\"B30:C30\"))>0,1,SEQUENCE(1,3))+Z5*0".to_string()));
        cells.push(("A20".to_string(), "=IF(COUNT(B10:C10)>0,1,SEQUENCE(1,3))".to_string()));
        cells.push(("A30".to_string(), "=IF(COUNT(B20:C20)>0,1,SEQUENCE(1,3))".to_string()));
        t.load(cells);
        t
    };
    let t = cells(false);
    let rounds = t.calc.rounds_calculated;
    assert!(rounds <= 8, "{rounds} rounds");
    assert!(t.calc.rounds_redone_in_row_major > 0);
    same_values(&t, &cells(true), "against row-major order");
}

#[test]
fn planning_charges_the_wide_readers_a_spill_query_passes_over() {
    // 600 anchors in column A each store a two-cell array in A:B, and 600 readers in CZ read
    // D1:BP600, rows every array's query meets, columns none of them reaches. Each query looks at
    // every reader before the columns turn it down: 360,000 looks in all, which the budget is
    // charged for, so a sheet of many more of both gives planning up rather than squaring.
    let mut t = T::new();
    let mut cells: Vec<(String, String)> = (1..=600).map(|r| (format!("A{r}"), "=SEQUENCE(1,2)+$A$5000".to_string())).collect();
    cells.extend((1..=600).map(|r| (format!("CZ{r}"), "=SUM(D1:BP600)".to_string())));
    cells.push(("A5000".to_string(), "1".to_string()));
    t.load(cells);
    assert_eq!(t.num("B600"), 3.0);
    let dirty: Vec<Key> = (1..=600).map(|r| (0, c(&format!("A{r}")))).collect();
    let plan = t.calc.graph.levels(&t.wb, &dirty).unwrap();
    assert!(plan.work() > 600 * 600, "{} index work", plan.work());
    assert!(matches!(t.calc.graph.levels_within(&t.wb, &dirty, Some(600 * 600)), Err(NoPlan::Budget { .. })), "past the budget");
    t.set("A5000", "2");
    assert_eq!(t.num("B600"), 4.0);
}

/// A sheet named Data with nothing on it, and on the first sheet a chain of `links` formulas in
/// column A, each the one above plus one, and `readers` wide readers in column C, each summing
/// A1:A65, the chain's end and `areas` cells of Data, every other row of column B: areas that
/// meet no dirty column, which the planner walks at every recount of a wide reader.
fn wide_readers_of_empty_areas(links: u32, readers: u32, areas: u32, plan_budget: Option<usize>) -> (T, Vec<Key>) {
    let mut t = T::new();
    t.calc.plan_budget = plan_budget;
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    let far: String = (1..=areas).map(|r| format!("+Data!B{}", 2 * r)).collect();
    let mut cells = vec![("A1".to_string(), "1".to_string())];
    cells.extend((2..=links).map(|r| (format!("A{r}"), format!("=A{}+1", r - 1))));
    cells.extend((1..=readers).map(|r| (format!("C{r}"), format!("=SUM(A1:A65)+A{links}{far}"))));
    let mut dirty: Vec<Key> = cells.iter().filter(|(_, input)| input.starts_with('=')).map(|(at, _)| (0, c(at))).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    t.load(cells);
    (t, dirty)
}

#[test]
fn planning_charges_each_area_it_walks() {
    // The reader waits for the chain's end, so it is counted again at each of the last 35 or so
    // links, and each count walks its 700 areas of Data: the index work holds them. At that much
    // work the plan is made; one unit less, planning is given up.
    let (t, dirty) = wide_readers_of_empty_areas(100, 1, 700, None);
    let plan = t.calc.graph.levels(&t.wb, &dirty).unwrap();
    assert!(plan.work() > 20 * 700, "{} index work", plan.work());
    assert!(t.calc.graph.levels_within(&t.wb, &dirty, Some(plan.work())).is_ok(), "at the budget");
    assert!(matches!(t.calc.graph.levels_within(&t.wb, &dirty, Some(plan.work() - 1)), Err(NoPlan::Budget { .. })), "past the budget");
    let (row_major, _) = wide_readers_of_empty_areas(100, 1, 700, Some(0));
    same_values(&t, &row_major, "planned, against row-major order");
    let (past, _) = wide_readers_of_empty_areas(100, 1, 700, Some(plan.work() - 1));
    same_values(&past, &row_major, "past the budget, against row-major order");
    assert_eq!(t.num("C1"), 65.0 * 66.0 / 2.0 + 100.0);
}

#[test]
fn wide_readers_of_many_empty_areas_on_a_long_chain_give_planning_up() {
    // Ten readers of 700 areas each, counted again at nearly every one of 2,000 links: about 13
    // million areas walked, past the budget of 256 units a formula and 2^20 for 2,009 formulas,
    // so the pass runs in row-major order rather than planning for longer than calculating.
    let (t, dirty) = wide_readers_of_empty_areas(2000, 10, 700, None);
    assert!(matches!(t.calc.graph.levels(&t.wb, &dirty), Err(NoPlan::Budget { .. })), "planning given up");
    let (row_major, _) = wide_readers_of_empty_areas(2000, 10, 700, Some(0));
    same_values(&t, &row_major, "against row-major order");
    assert_eq!(t.num("C10"), 65.0 * 66.0 / 2.0 + 2000.0);
}

/// A3 holds 1; row 1 from column B: `readers` formulas reading A3; A2: a formula reading A3 and
/// 900 empty whole rows, each of which the planner walks across every dirty column of the sheet.
/// With the planner's index work capped at `plan_budget`.
fn whole_rows_book(readers: u32, plan_budget: Option<usize>) -> (T, Vec<Key>) {
    let mut t = T::new();
    t.calc.plan_budget = plan_budget;
    let rows: Vec<String> = (0..900).map(|i| 100 + 2 * i).map(|r| format!("{r}:{r}")).collect();
    let sums: Vec<String> = rows.chunks(200).map(|part| format!("SUM({})", part.join(","))).collect();
    let mut cells = vec![("A3".to_string(), "1".to_string()), ("A2".to_string(), format!("=$A$3+{}", sums.join("+")))];
    cells.extend((1..=readers).map(|col| (CellRef::new(0, col).a1(), "=$A$3+1".to_string())));
    let mut dirty: Vec<Key> = cells.iter().filter(|(_, input)| input.starts_with('=')).map(|(at, _)| (0, c(at))).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    t.load(cells);
    (t, dirty)
}

#[test]
fn planning_gives_up_within_one_area_of_its_budget() {
    // Each of A2's 900 whole rows costs a walk over the sheet's 4,001 dirty columns: about 3.6
    // million units for one pass over them, and planning makes three (the count, the release
    // lists, the anchors above). The budget is checked after each area, so planning gives up
    // at most one area's walk (the area and 4,001 columns) past it, not after all of them.
    let cap = 1_000_000;
    let (t, dirty) = whole_rows_book(4000, None);
    let planned = t.calc.graph.levels_within(&t.wb, &dirty, Some(cap));
    let Err(NoPlan::Budget { work }) = planned else { panic!("planning given up: {:?}", planned.map(|p| p.work())) };
    assert!(cap < work && work <= cap + 4_002, "{work} index work against a budget of {cap}");
}

#[test]
fn planning_charges_every_reader_a_spill_query_lists() {
    // A1 holds the array A1:B1000; each of D1:D200 reads B1, B2 ... B100 one by one, so the query
    // for the readers of A1's stored spill lists each of them a hundred times: 20,000 keys for
    // 200 readers. Planning pays for each key as the query lists it, before they are sorted, and
    // gives the plan up inside the query, as soon as the budget runs out: the work it reports is
    // one unit past the budget, and the work the query did is at most the budget and the one
    // reader listed whose unit was refused, not the 20,000 keys.
    let refs: Vec<String> = (1..=100).map(|r| format!("B{r}")).collect();
    let reader = format!("={}", refs.join("+"));
    let mut t = T::new();
    t.load(
        [("Z1".to_string(), "1".to_string()), ("A1".to_string(), "=SEQUENCE(1000,2)+Z1".to_string())]
            .into_iter()
            .chain((1..=200).map(|r| (format!("D{r}"), reader.clone()))),
    );
    assert_eq!(t.num("D200"), (1..=100).map(|r| f64::from(2 * r) + 1.0).sum::<f64>());
    let mut dirty: Vec<Key> = (1..=200).map(|r| (0, c(&format!("D{r}")))).chain([(0, c("A1"))]).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    let plan = t.calc.graph.levels(&t.wb, &dirty).unwrap();
    assert!(plan.work() >= 200 * 100, "{} index work for 20,000 keys listed", plan.work());
    let cap = 10_000;
    crate::recalc::RANGE_LOOK_UP_WORK.with(|w| w.set(0));
    let planned = t.calc.graph.levels_within(&t.wb, &dirty, Some(cap));
    let looked_up = crate::recalc::RANGE_LOOK_UP_WORK.with(std::cell::Cell::get);
    let Err(NoPlan::Budget { work }) = planned else { panic!("planning given up: {:?}", planned.map(|p| p.work())) };
    assert_eq!(work, cap + 1, "stopped at the budget");
    assert!(looked_up <= cap + 1, "the query stopped inside: {looked_up} units of work looked up");
}

#[test]
fn a_look_up_counts_the_pending_entries_of_an_index() {
    // Five readers of A1:A10 are all an index of column A holds: few enough to wait, unmerged,
    // in its pending set. Looking up A1 visits the five, and counts them; within a budget it is
    // charged for them and for the five readers it lists, and stops one unit short of that.
    let mut t = T::new();
    t.load((1..=5).map(|r| (format!("C{r}"), "=SUM(A1:A10)".to_string())));
    let at = gridcraft_core::RangeRef::new(c("A1"), c("A1"));
    let mut readers = Vec::new();
    assert_eq!(t.calc.graph.dependents_of_range(0, at, &mut readers), 5, "the pending entries visited");
    assert_eq!(readers.len(), 5);
    let (mut readers, mut left) = (Vec::new(), 10);
    assert!(t.calc.graph.dependents_of_range_within(0, at, &mut readers, &mut left), "five entries and five readers");
    assert_eq!((readers.len(), left), (5, 0));
    let (mut readers, mut left) = (Vec::new(), 9);
    assert!(!t.calc.graph.dependents_of_range_within(0, at, &mut readers, &mut left), "one unit short");
}

#[test]
fn a_budgeted_reader_look_up_stops_when_its_budget_runs_out() {
    // Forty readers of column A's first hundred rows, one by one in a column index, and one wide:
    // the look-up of A1 lists them all with enough left, charging each reader it lists, and stops
    // listing as soon as a unit is due and nothing is left.
    let mut t = T::new();
    let mut cells: Vec<(String, String)> = (1..=40).map(|r| (format!("C{r}"), "=SUM(A1:A100)".to_string())).collect();
    cells.push(("D1".to_string(), "=SUM(A1:BZ100)".to_string()));
    t.load(cells);
    let at = gridcraft_core::RangeRef::new(c("A1"), c("A1"));
    let mut readers = Vec::new();
    let mut left = usize::MAX;
    assert!(t.calc.graph.dependents_of_range_within(0, at, &mut readers, &mut left));
    let cost = usize::MAX - left;
    assert_eq!(readers.len(), 41, "every reader listed");
    assert!(cost > 41, "each reader charged, and the entries visited: {cost}");
    // Exactly what it costs: done. One less: stopped, part of the readers listed.
    let (mut readers, mut left) = (Vec::new(), cost);
    assert!(t.calc.graph.dependents_of_range_within(0, at, &mut readers, &mut left));
    assert_eq!((readers.len(), left), (41, 0));
    let (mut readers, mut left) = (Vec::new(), cost - 1);
    assert!(!t.calc.graph.dependents_of_range_within(0, at, &mut readers, &mut left));
    assert_eq!(left, 0, "{} listed", readers.len());
    // Three units: it stops after three readers at most (each costs one, after what it visits).
    let (mut readers, mut left) = (Vec::new(), 3);
    assert!(!t.calc.graph.dependents_of_range_within(0, at, &mut readers, &mut left));
    assert!(readers.len() <= 3, "stopped early: {} listed", readers.len());
    // The look-up without a budget lists the same readers and counts the entries it visits.
    let mut all = Vec::new();
    let visited = t.calc.graph.dependents_of_range(0, at, &mut all);
    assert_eq!((all.len(), visited), (41, cost - 41));
}

#[test]
fn planning_given_up_leaves_the_passes_after_it_in_row_major_order_until_a_formula_changes() {
    // Planning the edit's round passes its budget, which the next edit would spend again: the
    // passes after it make no plan until a formula is entered, as after a round calculated again.
    // Each pass gives row-major order's values.
    let (mut t, _) = whole_rows_book(4000, Some(1_000_000));
    let (mut row_major, _) = whole_rows_book(4000, Some(0));
    for (at, input) in [("A3", "2"), ("A3", "3")] {
        edit(&mut t, &[(at, input)]);
        edit(&mut row_major, &[(at, input)]);
        assert_eq!(t.calc.plans_made, 0, "{at} = {input}: no plan");
        assert!(t.calc.row_major_only, "{at} = {input}: planning given up");
        same_values(&t, &row_major, &format!("after {at} = {input}"));
    }
    // A formula entered: its own pass plans again (one formula, well within the budget).
    edit(&mut t, &[("A4", "=A3*2")]);
    edit(&mut row_major, &[("A4", "=A3*2")]);
    assert_eq!(t.calc.plans_made, 1, "a formula entered: planned");
    assert!(!t.calc.row_major_only, "a formula entered");
    same_values(&t, &row_major, "after A4 = A3*2");
    // The next edit of A3 plans again, and gives planning up again.
    edit(&mut t, &[("A3", "4")]);
    edit(&mut row_major, &[("A3", "4")]);
    assert_eq!(t.calc.plans_made, 0, "A3 = 4: no plan");
    assert!(t.calc.row_major_only, "A3 = 4: planning given up again");
    same_values(&t, &row_major, "after A3 = 4");
    assert_eq!((t.num("B1"), t.num("A4")), (5.0, 8.0));
}

#[test]
fn a_dynamic_reader_of_an_array_that_goes_without_a_new_one_stays_stale_as_in_row_major_order() {
    // D1 turns A1's array into a single value, and no other array is laid out in that round. C1
    // reads B1 through INDIRECT, and in row-major order it comes after A1 but reads B1 before the
    // write-back clears it: it keeps 2. The dynamic formulas run again only after the first round
    // that lays out an array, as without a plan, so the planned run keeps 2 too. That is the value
    // row-major order gives, and it is stale (upstream issue #4469: Excel shows 0); a fix of #4469
    // on the row-major path flips the last assertion to 0.
    let mut t = T::new();
    t.load(
        [("D1", "1"), ("A2", "=$D$1"), ("A1", "=IF(A2,SEQUENCE(1,2),0)"), ("C1", "=INDIRECT(\"B1\")+0")].map(|(a, b)| (a.to_string(), b.to_string())),
    );
    assert_eq!((t.num("B1"), t.num("C1")), (2.0, 2.0), "after load");
    t.set("D1", "0");
    assert_eq!((t.num("A1"), t.get("B1"), t.num("C1")), (0.0, Value::Empty, 2.0), "row-major order's stale value (#4469)");
}

/// D3 reads G1:H2 through OFFSET, which the plan doesn't see, and H2 reads D3 (statically): a
/// cycle closed through OFFSET. The plan puts D3 in a level before H2, but in row-major order
/// H2 comes first and enters the cycle, so D3 shows `#CIRC!` and H2 counts nothing but its 1.
/// `more`: further cells.
fn cycle_through_offset(more: &[(&str, &str)]) -> T {
    let mut t = T::new();
    let cells = [("A1", "1"), ("D3", "=SUM(OFFSET(G1,0,0,2,2))"), ("H2", "=COUNT(B2:F3)+1")];
    t.load(cells.iter().chain(more).map(|(a, f)| (a.to_string(), f.to_string())));
    t
}

/// The cycle of [`cycle_through_offset`] is entered where a pass in row-major order enters it,
/// and every cell, spill and the circular list are what a new full recalculation gives.
fn entered_as_in_row_major_order(t: &T, what: &str) {
    assert_eq!(t.get("D3"), Value::Error(CellError::Circ), "{what}: D3");
    assert_eq!(t.get("H2"), Value::Number(1.0), "{what}: H2");
    assert_eq!(t.calc.circular, vec![(0, c("H2"))], "{what}: circular references");
    let mut full = T { wb: t.wb.clone(), calc: Calc::new() };
    full.calc.recalc_all(&mut full.wb);
    same_values(t, &full, &format!("{what}, against a full recalculation"));
}

#[test]
fn a_cycle_closed_through_offset_is_entered_as_in_row_major_order_after_edits_and_full_recalcs() {
    // Found by a reviewer: an INDIRECT reader of A1 in G1, an array above the cycle, and a second
    // INDIRECT reader over it. Each edit of A1 planned D3 ahead of H2 and showed D3 = 2 * A1 + 1,
    // while a full recalculation showed `#CIRC!`.
    let more = [("G1", "=INDIRECT(\"A1\")*2"), ("J1", "=A1+1"), ("K1", "=SEQUENCE(2)"), ("H5", "=COUNT(INDIRECT(\"A1:H4\"))+J1")];
    let mut t = cycle_through_offset(&more);
    entered_as_in_row_major_order(&t, "opened");
    // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
    t.calc.row_major_only = false;
    t.set("A1", "2");
    entered_as_in_row_major_order(&t, "A1 edited");
    assert!(t.calc.rounds_redone_in_row_major > 0, "the edit met the cycle from a level");
    t.calc.recalc_all(&mut t.wb);
    entered_as_in_row_major_order(&t, "full recalculation");
    t.set("A1", "3");
    entered_as_in_row_major_order(&t, "A1 edited again");
    assert_eq!(t.num("G1"), 6.0);
}

#[test]
fn a_cycle_closed_through_offset_over_a_plain_formula_is_entered_as_in_row_major_order() {
    // G1 reads A1 plainly: the plan put D3 ahead of H2 at every calculation, so the open, the
    // edits and a full recalculation all agreed on D3 = 2 * A1 + 1, where row-major order shows
    // `#CIRC!`.
    let mut t = cycle_through_offset(&[("G1", "=A1*2")]);
    entered_as_in_row_major_order(&t, "opened");
    assert!(t.calc.rounds_redone_in_row_major > 0, "the open met the cycle from a level");
    t.set("A1", "2");
    entered_as_in_row_major_order(&t, "A1 edited");
    t.calc.recalc_all(&mut t.wb);
    entered_as_in_row_major_order(&t, "full recalculation");
    t.set("A1", "3");
    entered_as_in_row_major_order(&t, "A1 edited again");
}

#[test]
fn random_draws_in_a_round_calculated_again_come_as_in_row_major_order() {
    // The cycle of `cycle_through_offset`, with B1 and B9 drawing in the first level and J2 in
    // the second, before D3 meets the cycle. The round calculated again draws from the seed the
    // round started with, in row-major order (B1, J2, B9), so every cell matches a workbook
    // calculated without a plan, at each step.
    let cells = [
        ("A1", "1"),
        ("D3", "=SUM(OFFSET(G1,0,0,2,2))"),
        ("H2", "=COUNT(B2:F3)+1"),
        ("G1", "=A1*2"),
        ("B1", "=RAND()"),
        ("J2", "=RAND()+G1"),
        ("B9", "=RAND()*A1"),
    ];
    let book = |plan_budget: Option<usize>| {
        let mut t = T::new();
        t.calc.plan_budget = plan_budget;
        t.load(cells.iter().map(|(a, f)| (a.to_string(), f.to_string())));
        t
    };
    let (mut t, mut row_major) = (book(None), book(Some(0)));
    same_values(&t, &row_major, "opened");
    assert!(t.calc.rounds_redone_in_row_major > 0, "the open met the cycle from a level");
    for value in ["2", "3"] {
        // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
        t.calc.row_major_only = false;
        t.set("A1", value);
        row_major.set("A1", value);
        same_values(&t, &row_major, &format!("A1 = {value}"));
        assert!(t.calc.rounds_redone_in_row_major > 0, "A1 = {value}: the edit met the cycle from a level");
    }
    assert_eq!(t.get("D3"), Value::Error(CellError::Circ));
}

#[test]
fn a_round_whose_levels_meet_no_cycle_is_calculated_once() {
    // D3 reads through OFFSET without closing a cycle (H2 is a number), and Y1 and Y2 form a
    // plain cycle, which comes after the levels: no round is calculated again.
    let mut t = cycle_through_offset(&[("G1", "=A1*2"), ("Y1", "=Y2+1"), ("Y2", "=Y1+1")]);
    t.set("H2", "5");
    assert_eq!(t.num("D3"), 7.0);
    t.set("A1", "2");
    assert_eq!(t.num("D3"), 9.0);
    assert_eq!(t.get("Y1"), Value::Error(CellError::Circ));
    assert_eq!(t.calc.rounds_redone_in_row_major, 0, "edit");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.get("Y1"), Value::Error(CellError::Circ));
    assert!(!t.calc.circular.is_empty());
    assert_eq!(t.calc.rounds_redone_in_row_major, 0, "full recalculation");
}

// ---- Passes without a settled result end as in row-major order --------------------------------

/// `cells` loaded (a `Data!` prefix puts a cell on a second sheet), with the pass planned, or
/// calculated in row-major order without a plan (`row_major`), as the base calculation runs every
/// pass.
fn loop_book(cells: &[(&str, &str)], row_major: bool) -> T {
    let mut t = T::new();
    if row_major {
        t.calc.plan_budget = Some(0);
    }
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    for (at, input) in cells {
        put(&mut t, at, input);
    }
    t.calc.recalc_all(&mut t.wb);
    t
}

fn put(t: &mut T, at: &str, input: &str) -> Key {
    let (sheet, at) = at.split_once('!').map_or((0, at), |(_, at)| (1, at));
    let cell = if input.starts_with('=') {
        Cell::formula(Formula::new(input))
    } else {
        Cell::value(gridcraft_core::parse::parse_input(input, t.wb.date_system).value)
    };
    t.wb.sheet_mut(sheet).unwrap().cells.set(c(at), cell);
    (sheet, c(at))
}

/// Loads `cells`, makes each edit of `edits`, then recalculates everything twice, planned and in
/// row-major order: the values, spills and circular references are the same at every step.
/// Returns the planned workbook after loading.
fn ends_as_in_row_major(what: &str, cells: &[(&str, &str)], edits: &[(&str, &str)]) -> T {
    let mut planned = loop_book(cells, false);
    let mut rows = loop_book(cells, true);
    same_values(&planned, &rows, &format!("{what}, after loading"));
    let loaded = T { wb: planned.wb.clone(), calc: planned.calc.clone() };
    for (at, input) in edits {
        for t in [&mut planned, &mut rows] {
            let key = put(t, at, input);
            t.calc.cells_changed(&mut t.wb, &[key]);
        }
        same_values(&planned, &rows, &format!("{what}, after {at} = {input}"));
    }
    for time in 1..=2 {
        for t in [&mut planned, &mut rows] {
            t.calc.recalc_all(&mut t.wb);
        }
        same_values(&planned, &rows, &format!("{what}, full recalculation {time}"));
    }
    loaded
}

#[test]
fn an_array_sized_through_another_array_it_feeds_ends_as_in_row_major_order() {
    // A1 is as tall as C3 says, B3 as wide as B5 says, and B5 reads A1: B3 spills over C3, A1
    // spills down to A2, where a value blocks it. Each round undoes the last; the plan lays the
    // arrays out in a different order from row-major order.
    let cells = [("A1", "=SEQUENCE(MOD(C3,3)+1)"), ("A2", "x"), ("B3", "=SEQUENCE(1,MOD(B5,3)+1)"), ("B5", "=A1+1")];
    ends_as_in_row_major("blocked", &cells, &[("Y1", "1"), ("Y2", "2")]);
}

#[test]
fn an_array_feeding_itself_through_another_sheet_ends_as_in_row_major_order() {
    let cells = [("A1", "=SEQUENCE(1,MOD(C5,3)+1)"), ("Data!A1", "=Sheet1!B1*2"), ("C5", "=Data!A1-1")];
    ends_as_in_row_major("othersheet", &cells, &[("Y1", "1")]);
}

#[test]
fn an_array_reading_its_own_spill_ends_as_in_row_major_order() {
    // D11's width comes from B7, which B6 spills over, and its values from E11, a cell of its own
    // spill; C4 reads D11.
    let cells = [("B6", "=SEQUENCE(2)+D1"), ("D11", "=SEQUENCE(1,MOD(B7,3)+1)+E11"), ("C4", "=SEQUENCE(MOD(B2,2)+1)+D11")];
    let t = ends_as_in_row_major("own spill", &cells, &[("D1", "1"), ("Y1", "1")]);
    assert_eq!((t.num("D11"), t.num("E11"), t.num("F11"), t.num("C4")), (1.0, 2.0, 3.0, 2.0), "after loading, as in row-major order");
}

#[test]
fn a_loop_with_one_member_per_round_ends_as_in_row_major_order() {
    // A10, A20 and A30 each spill three wide unless the two cells right of the one before hold a
    // number: each round dirties one of them, so no round shows the loop as a whole, and Z5 puts
    // A10 after A20, so the plan's first round differs from row-major order.
    let cells = [
        ("Z5", "=1"),
        ("A10", "=IF(COUNT(B30:C30)>0,1,SEQUENCE(1,3))+Z5*0"),
        ("A20", "=IF(COUNT(B10:C10)>0,1,SEQUENCE(1,3))"),
        ("A30", "=IF(COUNT(B20:C20)>0,1,SEQUENCE(1,3))"),
    ];
    ends_as_in_row_major("one member per round", &cells, &[("Z5", "=2"), ("Y1", "1")]);
}

#[test]
fn a_loop_that_starts_in_a_later_round_ends_as_in_row_major_order() {
    // After Z1 = 3, E1 spills to G1 (it waits for Z2, a level after A3), and A3, which read G1
    // before it was laid out, spills three wide in the next round: then D5 reads C3 and narrows
    // A3, which widens again, and so on. In row-major order A3 reads G1 after E1 in the first
    // round, a round ahead of the plan, so that planned round is calculated again in row-major
    // order.
    let cells = [("Z1", "1"), ("Z2", "=Z1"), ("E1", "=SEQUENCE(1,Z2)"), ("A3", "=IF(G1+Z1*0=3,SEQUENCE(1,IF(N(D5)>0,1,3)),0)"), ("D5", "=N(C3)")];
    ends_as_in_row_major("later round", &cells, &[("Z1", "3"), ("Y1", "1"), ("Z1", "1"), ("Z1", "3")]);
}

#[test]
fn a_static_cycle_beside_spills_ends_as_in_row_major_order() {
    // Reduced from a generated workbook: C3 counts B1:D1, which C1 spills into, and C1 is sized
    // through OFFSET from C3.
    let cells = [
        ("F12", "=IFERROR(ROWS(G9#)+COLUMNS(G9#),-1)"),
        ("C3", "=COUNT(B1:D1)"),
        ("E2", "=IFERROR(SUM(D2#),-1)"),
        ("B1", "=IFERROR(SUM(C13#),-1)"),
        ("C1", "=SEQUENCE(MOD(N(OFFSET(C3,0,0)),3)+1,MOD(N(INDIRECT(\"F12\")),2)+1)"),
    ];
    ends_as_in_row_major("static cycle", &cells, &[("A8", "0"), ("Y1", "1")]);
}

#[test]
fn arrays_sized_through_offset_and_counts_end_as_in_row_major_order() {
    let cells = [
        ("E9", "=IF(COUNT(A14:G11)>1,SEQUENCE(1,2),1)"),
        ("C6", "=IF(COUNT(F3:F9)>1,SEQUENCE(1,2),1)"),
        ("D11", "=COUNT(D2:E7)"),
        ("B8", "=SUM(D11:D9)"),
        ("B12", "=SEQUENCE(MOD(N(OFFSET(B8,0,0)),3)+1,MOD(N(INDIRECT(\"E2\")),2)+1)"),
        ("D7", "=IFERROR(SUM(G12#),-1)"),
        ("F6", "0"),
        ("E2", "=IFERROR(ROWS(C7#)+COLUMNS(C7#),-1)"),
        ("B13", "=SEQUENCE(MOD(N(OFFSET(A7,0,0)),3)+1,MOD(N(INDIRECT(\"B12\")),2)+1)"),
    ];
    ends_as_in_row_major("offset and counts", &cells, &[("E4", "2"), ("Y1", "1")]);
}

#[test]
fn an_array_that_feeds_itself_ends_the_same_whatever_else_the_workbook_holds() {
    // The blocked loop with 0 to 64 unrelated formulas beside it: the unrelated formulas change the
    // plan's levels, not where the loop ends, which is where row-major order ends it.
    let mut ends = Vec::new();
    for filler in [0u32, 4, 8, 16, 32, 64] {
        let mut cells: Vec<(String, String)> = [("A1", "=SEQUENCE(MOD(C3,3)+1)"), ("A2", "x"), ("B3", "=SEQUENCE(1,MOD(B5,3)+1)"), ("B5", "=A1+1")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .to_vec();
        cells.extend((1..=filler).map(|r| (format!("Z{r}"), "=ROW()".to_string())));
        let refs: Vec<(&str, &str)> = cells.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let planned = loop_book(&refs, false);
        let rows = loop_book(&refs, true);
        same_values(&planned, &rows, &format!("{filler} filler formulas"));
        assert!(planned.calc.rounds_calculated <= 8, "{filler} filler formulas: {} rounds", planned.calc.rounds_calculated);
        ends.push(["A1", "A2", "A3", "B3", "C3", "D3", "B5"].map(|at| planned.get(at)));
    }
    assert!(ends.windows(2).all(|w| w[0] == w[1]), "{ends:?}");
}

#[test]
fn an_honest_chain_of_arrays_ends_as_in_row_major_order() {
    // The sideways chain, planned backwards: its rounds are calculated in row-major order, which
    // settles it, after loading and after an edit.
    let mut t = T::new();
    t.load(sideways_chain("=SEQUENCE(1,5)*Z116"));
    assert_eq!(sideways_widths(&t), vec![5; 16]);
    t.set("Z100", "2");
    assert_eq!(sideways_widths(&t), vec![5; 16], "after an edit");
    assert_eq!((t.num("A16"), t.num("E16")), (2.0, 10.0), "after an edit");
}

// ---- Dynamic formulas after an array goes ---------------------------------------------------

#[test]
fn a_formula_reading_its_own_spill_through_indirect_ends_as_in_row_major_order() {
    // A1 reads B1, a cell of its own spill, through INDIRECT. The first round lays out 1 and 2;
    // the dynamic formulas run once more after it, A1 reads its own 2 and gives 0, and the array
    // goes: planned or not, through full recalculations, A1 entered again and an unrelated edit.
    // That is the value row-major order gives, and it is stale (upstream issue #4470: Excel shows
    // its circular-reference warning, then 1 and 2 in A1:B1, and keeps them); a fix of #4470 on
    // the row-major path flips these assertions to 1 and 2.
    let cells = [("A1", "=IF(INDIRECT(\"B1\")=0,SEQUENCE(1,2),0)")];
    let formula = cells[0].1;
    let t = ends_as_in_row_major("own spill through INDIRECT", &cells, &[("A1", formula), ("A1", formula), ("Y1", "1")]);
    assert_eq!((t.num("A1"), t.get("B1")), (0.0, Value::Empty), "after loading: row-major order's stale value (#4470)");
    for row_major in [false, true] {
        let mut t = loop_book(&cells, row_major);
        for time in 1..=3 {
            t.calc.cells_changed(&mut t.wb, &[(0, c("A1"))]);
            assert_eq!((t.num("A1"), t.get("B1")), (0.0, Value::Empty), "row-major {row_major}, A1 touched {time} times");
        }
        t.calc.recalc_all(&mut t.wb);
        assert_eq!((t.num("A1"), t.get("B1")), (0.0, Value::Empty), "row-major {row_major}, full recalculation");
    }
}

#[test]
fn a_dynamic_reader_of_an_array_that_goes_keeps_the_stale_cells_planned_or_not() {
    // Z1 = 0 turns A1's three-row array into one value. C5 reads A2 and A3 through INDIRECT and
    // keeps 5, planned or not: the dynamic formulas run again only after the first round that
    // lays out an array. That is the value row-major order gives, and it is stale (upstream issue
    // #4469: Excel shows 0 at once); a fix of #4469 on the row-major path flips it to 0.
    let cells = [("Z1", "1"), ("A1", "=IF(Z1=1,SEQUENCE(3),0)"), ("C5", "=N(INDIRECT(\"A2\"))+N(INDIRECT(\"A3\"))")];
    for row_major in [false, true] {
        let mut t = loop_book(&cells, row_major);
        assert_eq!(t.num("C5"), 5.0, "row-major {row_major}, after loading");
        let key = put(&mut t, "Z1", "0");
        t.calc.cells_changed(&mut t.wb, &[key]);
        assert_eq!(t.num("C5"), 5.0, "row-major {row_major}, after Z1 = 0: row-major order's stale value (#4469)");
    }
}

#[test]
fn a_dynamic_reader_above_an_array_sized_in_a_later_round_ends_as_in_row_major_order() {
    // Reduced from a generated workbook: B7 is as wide as F11 says, a cell F10 spills into, so in
    // row-major order B7 grows in the second round, after A3 read C7 through INDIRECT in that
    // round. The dynamic formulas run again once, after the first round that laid out arrays, as
    // without a plan, so A3 keeps the empty C7 it read, planned or not. That is the value
    // row-major order gives, and it is stale (upstream issue #4469: Excel shows 2).
    let cells = [("F10", "=SEQUENCE(2)"), ("B7", "=SEQUENCE(1,MOD(F11,3)+1)"), ("A3", "=INDIRECT(\"C7\")+0")];
    let t = ends_as_in_row_major("dynamic reader above", &cells, &[("F10", "=SEQUENCE(2)+1"), ("Y1", "1")]);
    assert_eq!((t.num("C7"), t.num("A3")), (2.0, 0.0), "after loading");
}

#[test]
fn a_loop_through_indirect_runs_its_dynamic_formulas_as_without_a_plan() {
    // D5 adds B12 to its array, and B12 reads D6, a cell of that array, through INDIRECT: a loop
    // that grows by 2 every time it goes round. In row-major order D5 calculates B12 on demand,
    // so the two orders don't surely agree on when B12 read D6, and the round is calculated
    // again in row-major order. The dynamic formulas run again only once after the first round
    // that laid out arrays, as without a plan: one more time round for each round would leave
    // B12 at 14.
    let cells = [("D5", "=SEQUENCE(2)+B12"), ("B12", "=INDIRECT(\"D6\")")];
    let t = ends_as_in_row_major("loop through indirect", &cells, &[("Y1", "1")]);
    assert_eq!((t.num("B12"), t.num("D5"), t.num("D6")), (2.0, 3.0, 4.0), "after loading");
}

// ---- Planned rounds calculated again in row-major order --------------------------------------

#[test]
fn two_arrays_that_each_shut_the_other_out_end_as_in_row_major_order() {
    // A1 spills three wide unless A5's array fills B5:C5, and A5 likewise with B1:C1: either
    // layout settles. Z9 puts A1 in a later level than A5, so the plan lays A5 out first, while
    // row-major order lays out A1 first. The planned round is calculated again in row-major
    // order, which spills A1 and leaves A5 one cell.
    let cells = [("Z9", "=1"), ("A1", "=IF(COUNT(B5:C5)>0,1,SEQUENCE(1,3))+Z9*0"), ("A5", "=IF(COUNT(B1:C1)>0,1,SEQUENCE(1,3))")];
    let t = ends_as_in_row_major("shut out", &cells, &[("Z9", "=2"), ("Y1", "1")]);
    assert_eq!((t.num("A1"), t.num("B1"), t.num("C1"), t.num("A5")), (1.0, 2.0, 3.0, 1.0), "after loading");
    assert_eq!(t.get("B5"), Value::Empty, "after loading");
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "after loading");
}

#[test]
fn a_dynamic_formula_reading_a_changed_array_in_another_order_is_calculated_again() {
    // C1 reads A6, a cell of A5's new array, through INDIRECT; Z8 puts C1 in a later level than
    // A5, so the plan calculates it after the array is laid out, while row-major order reaches C1
    // first. The planned round is calculated again in row-major order.
    let cells = [("Z9", "=1"), ("Z8", "=Z9"), ("A5", "=SEQUENCE(3)*Z9"), ("C1", "=INDIRECT(\"A6\")+Z8*0")];
    let t = ends_as_in_row_major("dynamic reader", &cells, &[("Z9", "=2"), ("Y1", "1")]);
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "after loading");
    assert_eq!((t.num("A6"), t.num("C1")), (2.0, 2.0), "after loading");
}

#[test]
fn a_formula_reading_its_own_new_cells_has_its_round_calculated_again() {
    // D11 reads E11, a cell its own array covers, as in the own-spill loop above.
    let cells = [("Z9", "=1"), ("D11", "=SEQUENCE(1,3)+E11*Z9")];
    let t = ends_as_in_row_major("own cells", &cells, &[("Z9", "=2")]);
    assert_eq!(t.calc.redo_reasons, vec![Redo::OwnArea], "after loading");
}

#[test]
fn two_arrays_that_shut_each_other_out_through_indirect_end_as_in_row_major_order() {
    // The shut-out pair with A1 reading B5:C5 through INDIRECT: only what the host noted of A1's
    // reads shows that it read A5's new cells after they were laid out, where row-major order
    // reaches A1 first.
    let cells = [("Z9", "=1"), ("A1", "=IF(COUNT(INDIRECT(\"B5:C5\"))>0,1,SEQUENCE(1,3))+Z9*0"), ("A5", "=IF(COUNT(B1:C1)>0,1,SEQUENCE(1,3))")];
    let t = ends_as_in_row_major("shut out through INDIRECT", &cells, &[("Z9", "=2"), ("Y1", "1")]);
    assert_eq!((t.num("A1"), t.num("B1"), t.num("C1"), t.num("A5")), (1.0, 2.0, 3.0, 1.0), "after loading");
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "after loading");
}

// ---- What a planned round notes and checks stays within its index-work budget ----------------

/// `cells` (a `Data!` prefix puts a cell on a second sheet) loaded with the planner's index work
/// capped at `plan_budget` (`Some(0)`: every round in row-major order).
fn capped_book(cells: &[(&str, String)], plan_budget: Option<usize>) -> T {
    let mut t = T::new();
    t.calc.plan_budget = plan_budget;
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    for (at, input) in cells {
        put(&mut t, at, input);
    }
    t.calc.recalc_all(&mut t.wb);
    t
}

/// `cells` edited on `t`, one recalculation each.
fn edit(t: &mut T, cells: &[(&str, &str)]) {
    for (at, input) in cells {
        let key = put(t, at, input);
        t.calc.cells_changed(&mut t.wb, &[key]);
    }
}

/// Z1 sizes A1's array; D1 reads forty cells of column B, every other one, and A2, all through
/// INDIRECT, so no read joins the one before it; Y1 is read by nothing.
fn noted_reads_cells() -> Vec<(&'static str, String)> {
    let reads: Vec<String> = (1..=40).map(|i| format!("N(INDIRECT(\"B{}\"))", 2 * i - 1)).collect();
    vec![
        ("Z1", "1".to_string()),
        ("Y1", "1".to_string()),
        ("A1", "=SEQUENCE(Z1)".to_string()),
        ("D1", format!("={}+N(INDIRECT(\"A2\"))", reads.join("+"))),
    ]
}

#[test]
fn reads_past_the_rounds_budget_are_not_noted_and_a_round_with_a_changed_array_runs_in_row_major_order() {
    // An edit of Y1 recalculates D1 alone (the dynamic formulas are in every round). With as
    // many units as D1's reads take, each is noted and the round is kept; with one fewer, none is
    // kept, and the round still stands, as none of its arrays changed. An edit of Z1 changes A1's
    // array: with the reads not noted, the round is calculated again in row-major order.
    let cells = noted_reads_cells();
    let mut free = capped_book(&cells, None);
    edit(&mut free, &[("Y1", "2")]);
    let noted = match free.calc.watched.as_slice() {
        [(noted, false)] => *noted,
        other => panic!("one planned round, its reads noted: {other:?}"),
    };
    assert!(noted >= 41, "{noted} reads noted: forty in column B and A2, none joined");

    let mut at_limit = capped_book(&cells, None);
    at_limit.calc.note_cap = Some(noted);
    edit(&mut at_limit, &[("Y1", "2")]);
    assert_eq!(at_limit.calc.watched, vec![(noted, false)], "at the limit, every read is noted");
    assert_eq!(at_limit.calc.rounds_redone_in_row_major, 0, "at the limit");

    let mut past = capped_book(&cells, None);
    past.calc.note_cap = Some(noted - 1);
    let mut row_major = capped_book(&cells, Some(0));
    edit(&mut past, &[("Y1", "2")]);
    edit(&mut row_major, &[("Y1", "2")]);
    assert_eq!(past.calc.watched, vec![(0, true)], "one past the limit, the reads are let go");
    assert_eq!(past.calc.rounds_redone_in_row_major, 0, "no array changed: the planned round stands");
    same_values(&past, &row_major, "after Y1 = 2");

    edit(&mut past, &[("Z1", "3")]);
    edit(&mut row_major, &[("Z1", "3")]);
    assert_eq!(past.calc.watched.first(), Some(&(0, true)), "after Z1 = 3, the reads are let go");
    assert_eq!(past.calc.redo_reasons, vec![Redo::Budget], "A1's array changed: the round runs in row-major order");
    same_values(&past, &row_major, "after Z1 = 3");
    assert_eq!((past.num("A2"), past.num("D1")), (2.0, 2.0));
}

/// Row 1000 holds `columns` formulas from column B on; A1:A4 each read twenty rows of those
/// columns through OFFSET, every other row, so no read joins the one before it. On the Data
/// sheet, C150 lays out a new array that D150 reads, so checking the round walks the reads
/// through the dirty columns.
fn wide_reads_cells(columns: u32) -> Vec<(String, String)> {
    let mut cells = vec![("A999".to_string(), "1".to_string())];
    for col in 1..=columns {
        cells.push((CellRef::new(999, col).a1(), "=$A$999+COLUMN()".to_string()));
    }
    for row in 1..=4 {
        cells.push((format!("A{row}"), format!("=REDUCE(0,SEQUENCE(20),LAMBDA(acc,i,acc+SUM(OFFSET($A$1,2*i,1,1,{columns}))))+ROW()")));
    }
    cells.push(("Data!C150".to_string(), "=SEQUENCE(3)+Sheet1!$A$999".to_string()));
    cells.push(("Data!D150".to_string(), "=C151+Sheet1!$A$999".to_string()));
    cells
}

#[test]
fn a_wide_read_over_many_dirty_columns_is_charged_to_the_rounds_budget() {
    // Each of the eighty reads covers the 300 dirty columns of row 1000, where none of them is
    // read: the check walks them all, and pays for each. Within the round's budget the planned
    // round stands; with planning and the reads paid for but not the walk, the round is
    // calculated again in row-major order. The values are row-major order's either way.
    let owned = wide_reads_cells(300);
    let cells: Vec<(&str, String)> = owned.iter().map(|(at, input)| (at.as_str(), input.clone())).collect();
    let free = capped_book(&cells, None);
    let row_major = capped_book(&cells, Some(0));
    same_values(&free, &row_major, "within the budget");
    assert_eq!(free.calc.rounds_redone_in_row_major, 0, "within the budget: {:?}", free.calc.redo_reasons);
    assert!(free.calc.check_work >= 80 * 300, "each read pays for the columns it walks: {}", free.calc.check_work);

    let mut sorted: Vec<Key> = owned
        .iter()
        .filter(|(_, input)| input.starts_with('='))
        .map(|(at, _)| at.split_once('!').map_or_else(|| (0, c(at)), |(_, at)| (1, c(at))))
        .collect();
    sorted.sort_by_key(|(s, c)| (*s, c.row, c.col));
    let planning = free.calc.graph.levels(&free.wb, &sorted).map(|p| p.work()).unwrap_or(usize::MAX);
    let capped = capped_book(&cells, Some(planning + 1_000));
    same_values(&capped, &row_major, "with the walk past the budget");
    assert_eq!(capped.calc.redo_reasons, vec![Redo::Budget], "planning {planning}: the walk runs out of budget");
}

/// Rows of [`new_arrays_beside_wide_readers`].
const WIDE_READER_ROWS: u32 = 600;

/// A1:A600 each lay out a new two-cell array (A:B), and CZ1:CZ600 each read D1:BP600, wide, all
/// rows, beside them: every look-up of an array's readers visits the 600 wide readers and finds
/// none. One level, with the planner's index work (and so the round's budget) capped at
/// `plan_budget`.
fn new_arrays_beside_wide_readers(plan_budget: Option<usize>) -> T {
    let n = WIDE_READER_ROWS;
    let cells: Vec<(String, String)> =
        (1..=n).flat_map(|r| [(format!("A{r}"), "={1,2}".to_string()), (format!("CZ{r}"), format!("=COLUMNS(D1:BP{n})"))]).collect();
    let cells: Vec<(&str, String)> = cells.iter().map(|(at, input)| (at.as_str(), input.clone())).collect();
    capped_book(&cells, plan_budget)
}

#[test]
fn looking_up_the_readers_of_new_arrays_is_charged_to_the_rounds_budget() {
    // Checking the round looks up the readers of each new array: 600 look-ups over the 600 wide
    // readers, about 360,000 entries visited and none found. Within the round's budget the round
    // stands; with planning paid for but not the look-ups, it is calculated again in row-major
    // order. The values are row-major order's either way.
    let n = WIDE_READER_ROWS as usize;
    let row_major = new_arrays_beside_wide_readers(Some(0));
    assert_eq!((row_major.num("B600"), row_major.num("CZ600")), (2.0, 65.0));
    let free = new_arrays_beside_wide_readers(None);
    same_values(&free, &row_major, "within the budget");
    assert_eq!(free.calc.rounds_redone_in_row_major, 0, "within the budget: {:?}", free.calc.redo_reasons);
    assert!(free.calc.check_work >= n * n, "each look-up pays for what it visits: {}", free.calc.check_work);

    let capped = new_arrays_beside_wide_readers(Some(50_000));
    same_values(&capped, &row_major, "past the budget");
    assert_eq!(capped.calc.plans_made, 1, "planning fits the budget");
    assert_eq!(capped.calc.redo_reasons, vec![Redo::Budget], "the look-ups run out of budget");
}

/// The units of work range look-ups did on this thread while `step` ran (see
/// `RANGE_LOOK_UP_WORK`).
fn looked_up_in(step: impl FnOnce()) -> usize {
    crate::recalc::RANGE_LOOK_UP_WORK.with(|w| w.set(0));
    step();
    crate::recalc::RANGE_LOOK_UP_WORK.with(std::cell::Cell::get)
}

/// Z1 = 1 makes A1 (`=SEQUENCE(1,Z1)`) one number; the 600 formulas of column D read B1. An edit
/// of Z1 to 2 lays out a new array over A1:B1 in a round of A1 alone. With the planner's index
/// work capped at `plan_budget`.
fn a_new_array_read_by_many(plan_budget: Option<usize>) -> T {
    let cells: Vec<(String, String)> = [("Z1".to_string(), "1".to_string()), ("A1".to_string(), "=SEQUENCE(1,Z1)".to_string())]
        .into_iter()
        .chain((1..=600).map(|r| (format!("D{r}"), "=B1+0".to_string())))
        .collect();
    let cells: Vec<(&str, String)> = cells.iter().map(|(at, input)| (at.as_str(), input.clone())).collect();
    capped_book(&cells, plan_budget)
}

#[test]
fn a_check_whose_look_up_runs_out_stops_inside_it_and_calculates_the_round_again() {
    // Checking the round of the edit looks up B1's readers once: 600 of them, none in the round.
    // With the round's budget at 50, planning A1 fits and the look-up runs out after about fifty:
    // it stops there, and the round is calculated again in row-major order, the check having
    // charged no more than the budget, with row-major order's values.
    let cap = 50;
    let mut row_major = a_new_array_read_by_many(Some(0));
    let mut t = a_new_array_read_by_many(None);
    same_values(&t, &row_major, "after loading");
    assert!(!t.calc.row_major_only, "after loading: {:?}", t.calc.redo_reasons);
    t.calc.plan_budget = Some(cap);
    let planned = looked_up_in(|| edit(&mut t, &[("Z1", "2")]));
    let unplanned = looked_up_in(|| edit(&mut row_major, &[("Z1", "2")]));
    same_values(&t, &row_major, "after Z1 = 2");
    assert_eq!((t.num("B1"), t.num("D600")), (2.0, 2.0));
    assert_eq!(t.calc.plans_made, 1, "planning A1 fits the budget");
    assert_eq!(t.calc.redo_reasons, vec![Redo::Budget], "the look-up runs out");
    // Write-back's own look-ups of B1's 600 readers aren't budgeted: the row-major run of the
    // same edit makes them too, so what the planned run looks up past that is the check's, at
    // most its budget and the one reader whose unit it refused.
    assert!(planned <= unplanned + cap + 1, "the look-up stopped inside: {planned} units against {unplanned} in row-major order");
}

#[test]
fn a_reader_beside_an_array_but_in_neither_of_its_areas_keeps_the_round() {
    // A1 is a column of forty when Y1 is 1 and a row of ten otherwise. Z50 reads C10, inside the
    // rectangle around both areas but in neither, and comes before A1 in the plan (Y1 puts A1 a
    // level later) and after it in row-major order. What C10 holds doesn't depend on whether
    // A1's array is laid out yet, so the planned rounds stand.
    let cells: Vec<(&str, String)> = [("Z1", "1"), ("Y1", "=Z1"), ("C10", "7"), ("A1", "=IF(Y1=1,SEQUENCE(40),SEQUENCE(1,10))"), ("Z50", "=C10+Z1")]
        .map(|(at, input)| (at, input.to_string()))
        .to_vec();
    let mut planned = capped_book(&cells, None);
    let mut row_major = capped_book(&cells, Some(0));
    same_values(&planned, &row_major, "after loading");
    for value in ["2", "1", "2"] {
        edit(&mut planned, &[("Z1", value)]);
        edit(&mut row_major, &[("Z1", value)]);
        same_values(&planned, &row_major, &format!("after Z1 = {value}"));
        assert!(planned.calc.redo_reasons.is_empty(), "after Z1 = {value}: {:?}", planned.calc.redo_reasons);
    }
    assert_eq!((planned.num("J1"), planned.num("Z50")), (10.0, 9.0));
}

// ---- After a round calculated again ------------------------------------------------------------

/// The shut-out pair (its first round is calculated again in row-major order) and Y1, read by
/// nothing.
fn shut_out_cells() -> Vec<(&'static str, String)> {
    [("Z9", "=1"), ("Y1", "1"), ("A1", "=IF(COUNT(B5:C5)>0,1,SEQUENCE(1,3))+Z9*0"), ("A5", "=IF(COUNT(B1:C1)>0,1,SEQUENCE(1,3))")]
        .map(|(at, input)| (at, input.to_string()))
        .to_vec()
}

#[test]
fn a_redone_round_leaves_the_passes_after_it_in_row_major_order_until_a_formula_changes() {
    // Loading calculates the pair's first round again in row-major order: the passes after it
    // make no plan, through full recalculations and a value edit, and give row-major order's
    // values. A formula entered makes the next pass plan again.
    let cells = shut_out_cells();
    let mut t = capped_book(&cells, None);
    let mut row_major = capped_book(&cells, Some(0));
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "after loading");
    assert!(t.calc.row_major_only, "after loading");
    for time in 1..=2 {
        t.calc.recalc_all(&mut t.wb);
        row_major.calc.recalc_all(&mut row_major.wb);
        assert_eq!(t.calc.plans_made, 0, "full recalculation {time}: no plan");
        assert!(t.calc.row_major_only, "full recalculation {time}");
        same_values(&t, &row_major, &format!("full recalculation {time}"));
    }
    edit(&mut t, &[("Y1", "2")]);
    edit(&mut row_major, &[("Y1", "2")]);
    assert_eq!(t.calc.plans_made, 0, "a value edit: no plan");
    same_values(&t, &row_major, "after Y1 = 2");
    edit(&mut t, &[("Y2", "=Y1+1")]);
    edit(&mut row_major, &[("Y2", "=Y1+1")]);
    assert!(t.calc.plans_made > 0, "a formula entered: the pass plans again");
    assert!(!t.calc.row_major_only, "nothing calculated again since");
    same_values(&t, &row_major, "after Y2 = Y1+1");
}

#[test]
fn a_pass_with_no_redone_round_keeps_planning() {
    // A chain and an array whose readers wait for it: no round is calculated again, so every
    // pass plans.
    let cells: Vec<(&str, String)> =
        [("Z1", "1"), ("Z2", "=Z1*2"), ("A1", "=SEQUENCE(3)*Z2"), ("C1", "=SUM(A1#)")].map(|(at, input)| (at, input.to_string())).to_vec();
    let mut t = capped_book(&cells, None);
    for value in ["2", "3"] {
        edit(&mut t, &[("Z1", value)]);
        assert!(t.calc.plans_made > 0 && !t.calc.row_major_only && t.calc.redo_reasons.is_empty(), "after Z1 = {value}");
        t.calc.recalc_all(&mut t.wb);
        assert!(t.calc.plans_made > 0 && !t.calc.row_major_only, "full recalculation after Z1 = {value}");
    }
    assert_eq!(t.num("C1"), 36.0);
}

#[test]
fn a_rebuild_keeps_row_major_order_while_the_formulas_are_the_same() {
    // Loading calculates the shut-out pair's round again, so passes run in row-major order. A
    // rebuild that finds the same formulas with the same references keeps that, a value edited
    // included, and a new copy of a formula with the same text (as Sort and Undo put back); one
    // that finds a formula entered, or one replaced by another that reads the same cells, plans
    // again: through a rebuild alone (Sort, structural edits, Undo and Redo rebuild without
    // `cells_changed`), and in a full recalculation, which makes a plan.
    let mut cells = shut_out_cells();
    cells.push(("X1", "=Y1+1".to_string()));
    let mut t = capped_book(&cells, None);
    assert!(t.calc.row_major_only, "after loading");
    t.calc.rebuild(&t.wb);
    assert!(t.calc.row_major_only, "the same formulas");
    put(&mut t, "Y1", "2");
    t.calc.rebuild(&t.wb);
    assert!(t.calc.row_major_only, "a value edited");
    put(&mut t, "X1", "=Y1+1");
    t.calc.rebuild(&t.wb);
    assert!(t.calc.row_major_only, "a new copy of the same formula");
    put(&mut t, "X1", "=Y1*5");
    t.calc.rebuild(&t.wb);
    assert!(!t.calc.row_major_only, "a formula replaced by one that reads the same cell");

    let mut t = capped_book(&cells, None);
    assert!(t.calc.row_major_only, "after loading");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.calc.plans_made, 0, "the same formulas: no plan");
    put(&mut t, "X1", "=Y1*5");
    t.calc.recalc_all(&mut t.wb);
    assert!(t.calc.plans_made > 0, "a formula replaced by one that reads the same cell: a plan");
    assert_eq!(t.num("X1"), 5.0);

    let mut t = capped_book(&cells, None);
    put(&mut t, "Y2", "=Y1+1");
    t.calc.rebuild(&t.wb);
    assert!(!t.calc.row_major_only, "a formula entered");
}

#[test]
fn a_round_calculated_again_reads_the_clock_its_planned_try_read() {
    // The shut-out pair's first round is calculated again in row-major order, with B9 reading the
    // clock in it, which moves on by one at each reading. The clock is read once a round, as
    // without a plan: B9 shows the reading of its last round, whether that round was planned
    // first or not.
    let mut t = T::new();
    t.calc.ticking_now = Some(100.0);
    for (at, input) in shut_out_cells().iter().map(|(at, input)| (*at, input.as_str())).chain([("B9", "=NOW()")]) {
        put(&mut t, at, input);
    }
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order]);
    let rounds = t.calc.rounds_calculated as f64;
    assert_eq!(t.calc.ticking_now, Some(100.0 + rounds), "one reading for each of {rounds} rounds");
    assert_eq!(t.num("B9"), 100.0 + rounds - 1.0);
}

#[test]
fn a_dynamic_reader_of_a_chained_array_stays_stale_as_in_row_major_order() {
    // X1 feeds B2's start, and C5 reads C2, a cell of B2's array, through INDIRECT; A1 reads C5.
    // After Z1 = 2 the first round lays out X1's new values (and is kept), and the second has
    // B2, C5 and A1: row-major order reaches A1 first, which calculates C5 on demand before B2's
    // new array is laid out, so C5 and A1 keep 3 while C2 holds 4. That is the value row-major
    // order gives, and it is stale (upstream issue #4469); a fix of #4469 on the row-major path
    // flips it to 4. The plan reaches C5 after B2's array, so the check has to see that A1, which
    // row-major order calculates first, reaches C5 (`forcing_roots`), and send the round back.
    let cells: Vec<(&str, String)> =
        [("Z1", "1"), ("X1", "=SEQUENCE(2,1,Z1)"), ("B2", "=SEQUENCE(1,2,X2)"), ("C5", "=INDIRECT(\"C2\")+0"), ("A1", "=C5+0")]
            .map(|(at, input)| (at, input.to_string()))
            .to_vec();
    let mut planned = capped_book(&cells, None);
    let mut row_major = capped_book(&cells, Some(0));
    same_values(&planned, &row_major, "after loading");
    assert_eq!((planned.num("A1"), planned.num("C5"), planned.num("C2")), (3.0, 3.0, 3.0), "after loading");
    for (value, want) in [("2", (3.0, 3.0, 4.0)), ("3", (4.0, 4.0, 5.0))] {
        // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
        planned.calc.row_major_only = false;
        edit(&mut planned, &[("Z1", value)]);
        edit(&mut row_major, &[("Z1", value)]);
        assert!(planned.calc.plans_made > 0, "after Z1 = {value}: planned");
        same_values(&planned, &row_major, &format!("after Z1 = {value}"));
        assert_eq!((planned.num("A1"), planned.num("C5"), planned.num("C2")), want, "after Z1 = {value}: row-major order's values (#4469)");
    }
}
