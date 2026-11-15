//! Recalculation planned in dependency levels, and wide levels calculated on worker threads:
//! the plan's order, what one thread and many give, and what a worker hands back to the main thread.

use super::*;
use crate::Key;
use crate::recalc::{NoPlan, Redo};

// ---- Multi-threaded recalculation ----------------------------------------------------------

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
        self.wb.names_mut().push(gridcraft_model::DefinedName {
            name: name.into(),
            scope: None,
            formula: formula.into(),
            comment: String::new(),
            hidden: false,
        });
    }
    /// Calculation threads: 1 follows the same plan on the main thread alone, the oracle the
    /// threaded runs match; `full_recalc_matches_entering_cells_one_by_one_at_any_thread_count`
    /// checks a planned full recalculation against the cells entered one by one (each edit
    /// planned on its own). One thread plans here although the workbook can't draw random
    /// numbers, which it doesn't outside tests (see `Calc::run`), so that its decisions can be
    /// compared with several threads'; `planning_as_shipped` turns that off.
    fn threads(mut self, n: u32) -> T {
        self.wb.calc.multi_threaded = n > 1;
        self.wb.calc.threads = n;
        // Whatever `GRIDCRAFT_CALC_THREADS` says and however many processors there are.
        self.calc.threads = Some(n as usize);
        self.calc.plan_one_thread = true;
        self
    }
    /// Planning as outside tests: no plan on one thread while the workbook can't draw random
    /// numbers.
    fn planning_as_shipped(mut self) -> T {
        self.calc.plan_one_thread = false;
        self
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
        if sa.spill_ranges != sb.spill_ranges {
            return Err(format!("sheet {i} spill ranges {:?} and {:?}", sa.spill_ranges, sb.spill_ranges));
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

/// Rows of the mixed workbook: enough for levels of a thousand formulas, which the default
/// `PARALLEL_MIN` puts on worker threads, without the test taking a minute.
const MIXED_ROWS: u32 = 1_000;

/// A workbook with a bit of everything, in levels wide enough to be threaded: lookups over a
/// range of formulas, INDIRECT readers of their own row (with a static edge through a name, so
/// they sit in a level after the rows they read), a defined name, a running-sum chain (one formula per level), whole-range sums,
/// a spill with readers of its range, a cycle and formulas behind it, and a second sheet whose
/// formulas read the first's and are read back by it.
fn mixed_workbook(threads: u32) -> T {
    let n = MIXED_ROWS;
    let mut t = T::new().threads(threads);
    // `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
    t.calc.parallel_min = 512;
    t.name("Base", "Sheet1!$B$7");
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    for r in 1..=600u32 {
        let data = t.wb.sheet_mut(1).unwrap();
        data.cells.set(c(&format!("A{r}")), Cell::formula(Formula::new(&format!("=SUM(Sheet1!$B$1:$B${n})+ROW()"))));
        data.cells.set(c(&format!("B{r}")), Cell::formula(Formula::new(&format!("=VLOOKUP(Sheet1!A{r},Sheet1!$A$1:$C${n},3,FALSE)"))));
    }
    let mut cells: Vec<(String, String)> = Vec::new();
    for r in 1..=n {
        cells.push((format!("A{r}"), format!("={}", (r * 37) % 1000)));
        cells.push((format!("B{r}"), format!("=A{r}*2+ROW()")));
        cells.push((format!("C{r}"), format!("=VLOOKUP({},$A$1:$B${n},2,FALSE)", (r * 37) % 1000)));
        cells.push((format!("D{r}"), "=INDIRECT(\"B\"&ROW())+Base".to_string()));
        cells.push((format!("E{r}"), format!("=IF(MOD(A{r},2)=0,\"even\"&A{r},LEN(\"odd\"&B{r}))")));
        cells.push((format!("F{r}"), if r == 1 { "=B1".to_string() } else { format!("=F{}+B{r}", r - 1) }));
        cells.push((format!("G{r}"), format!("=SUM($B$1:$B${n})/ROW()")));
        if r <= 600 {
            cells.push((format!("M{r}"), format!("=SUM(J1#)+A{r}")));
            cells.push((format!("N{r}"), "=L1+ROW()".to_string()));
            cells.push((format!("H{r}"), format!("=Data!A{r}+Data!B{r}")));
        }
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
    let mut one = mixed_workbook(1);
    let mut many = mixed_workbook(8);
    same_values(&one, &many, "after load");
    assert!(
        many.calc.worker_levels > 0 && one.calc.worker_levels == 0,
        "levels on workers: {} and {}",
        many.calc.worker_levels,
        one.calc.worker_levels
    );
    let data = many.wb.sheet(1).unwrap();
    assert_eq!(data.value(c("B600")), many.get("C600"));
    assert_eq!(many.num("H600"), data.value(c("A600")).as_f64().unwrap() + data.value(c("B600")).as_f64().unwrap());
    // Spot checks against values worked out by hand.
    assert_eq!(one.num("D10"), one.num("B10") + one.num("B7"));
    assert_eq!(one.num("C5"), one.num("B5"));
    let spill_sum = (0..50).map(|i| f64::from(i) + one.num("A3")).sum::<f64>();
    assert_eq!(one.num("K1"), spill_sum + 49.0);
    assert_eq!(many.num("M600"), spill_sum + many.num("A600"));
    assert_eq!(many.num(&format!("F{MIXED_ROWS}")), (1..=MIXED_ROWS).map(|r| many.num(&format!("B{r}"))).sum::<f64>());
    assert_eq!(many.get("L1"), Value::Error(CellError::Circ));
    assert_eq!(many.get("N600"), Value::Error(CellError::Circ));
    // Edits recalculate thousands of formulas in wide levels again: a lookup table entry, the
    // name's target, the spill's start.
    for (at, input) in [("A1", "=999"), ("B7", "=-1"), ("A3", "=100")] {
        one.set(at, input);
        many.set(at, input);
        same_values(&one, &many, &format!("after {at} = {input}"));
        assert!(many.calc.worker_levels > 0, "after {at} = {input}: no level on workers");
    }
    assert_eq!(many.num("K1"), (0..50).map(|i| f64::from(i) + 100.0).sum::<f64>() + 49.0);
}

#[test]
fn thread_count_follows_the_settings() {
    if std::env::var_os("GRIDCRAFT_CALC_THREADS").is_some() {
        eprintln!("skipped: GRIDCRAFT_CALC_THREADS overrides the settings");
        return;
    }
    // At least one processor and at most Excel's 64, whatever the machine has.
    assert_eq!([0, 1, 4, 63, 64, 65, 1024].map(crate::recalc::processors_used), [1, 1, 4, 63, 64, 64, 64]);
    // Never more than the processors present (a workbook's count travels with the file).
    let cores = crate::recalc::processors_used(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));
    let mut s = gridcraft_model::CalcSettings { threads: 3, ..Default::default() };
    assert_eq!(crate::recalc::thread_count(&s), if cfg!(target_arch = "wasm32") { 1 } else { 3.min(cores) });
    s.multi_threaded = false;
    assert_eq!(crate::recalc::thread_count(&s), 1);
    s.multi_threaded = true;
    s.threads = 0;
    assert_eq!(crate::recalc::thread_count(&s), if cfg!(target_arch = "wasm32") { 1 } else { cores });
    s.threads = 1000;
    assert!(crate::recalc::thread_count(&s) <= cores);
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
    // they are planned after A1 and read its new array in the first round, as one thread does
    // in row-major order; the write-back then queues them once more either way (the readers of
    // a changed spill range), so the counts match too.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.set("Z1", "1");
        t.load((1..=600).map(|r| (format!("B{r}"), format!("=A{r}+Z1"))).chain([("A1".to_string(), "=SEQUENCE(600,1,Z1)".to_string())]));
        assert_eq!((t.num("B1"), t.num("B600")), (2.0, 601.0));
        t.set("Z1", "5");
        assert_eq!((t.num("B1"), t.num("B600")), (10.0, 609.0));
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "spill readers");
    assert_eq!(one.calc.last_recalc_cells, 601 + 600);
    assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells);
    // The plan itself: with A1's spill range known, every B sits in a level after A1 (without
    // the spill edge the readers of A2..A600 would share A1's level and the values would still
    // come right through the write-back, so the counts alone don't show the edge).
    let keys: Vec<Key> = (1..=600).map(|r| (0, c(&format!("B{r}")))).chain([(0, c("A1"))]).collect();
    let plan = one.calc.graph.levels(&one.wb, &keys).unwrap();
    let levels: Vec<Vec<Key>> = plan.levels().map(|l| l.to_vec()).collect();
    let level_of = |k: Key| levels.iter().position(|l| l.contains(&k));
    let anchor = level_of((0, c("A1"))).unwrap();
    for r in 1..=600 {
        let b = level_of((0, c(&format!("B{r}")))).unwrap_or_else(|| panic!("B{r} not leveled: rest {:?}", plan.rest()));
        assert!(b > anchor, "B{r} in level {b}, A1 in level {anchor}");
    }
}

#[test]
fn a_chain_of_spill_areas_read_before_they_are_laid_out_stays_stale_as_in_row_major_order() {
    // Ten spills in a chain, each reading the area (not the anchor) of the one before, laid out
    // so every reader comes before what it reads in row-major order. Nothing is calculated on
    // demand (the areas aren't formulas), so each write-back round settles one link in row-major
    // order, and the 8-round limit leaves the chain three links short: A1 = 4193 where the
    // settled chain gives 5991. That is the value row-major order gives, and it is stale
    // (upstream issue #2320). The plan would settle the chain in one round, but a planned round
    // gives what the row-major round gives, so the planned and threaded runs end where row-major
    // order ends. A fix of #2320 on the row-major path flips this test to 5991.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells = vec![("K1".to_string(), "=SEQUENCE(600,1,Z1)".to_string()), ("Z1".to_string(), "1".to_string())];
        for (i, col) in "JIHGFEDCBA".chars().enumerate() {
            let prev = "KJIHGFEDCB".chars().nth(i).unwrap_or('K');
            cells.push((format!("{col}1"), format!("=SEQUENCE(600,1,MAX({prev}2:{prev}600))")));
        }
        t.load(cells.clone());
        assert_eq!(t.num("A1"), 4193.0, "{threads} threads: row-major order's stale value (#2320)");
        assert_eq!(t.num("A600"), 4193.0 + 599.0, "{threads} threads");
        let mut row_major = T::new().threads(threads);
        row_major.calc.plan_budget = Some(0);
        row_major.load(cells);
        same_values(&t, &row_major, &format!("{threads} threads"));
    }
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
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
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
            assert_eq!(height, 5, "{col}1 on {threads} threads");
        }
        assert_eq!(t.calc.rounds_redone_in_row_major, 0, "{threads} threads: the levels settle the chain by themselves");
    }
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
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(sideways_chain("=SEQUENCE(1,5)*Z116"));
        assert_eq!(sideways_widths(&t), vec![5; 16], "{threads} threads");
        assert_eq!((t.num("A16"), t.num("E16")), (1.0, 5.0), "{threads} threads");
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{threads} threads");
    }
}

#[test]
fn a_chain_of_arrays_spilling_right_into_the_row_below_settles_after_an_edit() {
    // The same chain with A1 sized by the end of the Z chain: one wide everywhere after loading,
    // then Z100 = 5 makes A1 five wide, and every row below follows.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(sideways_chain("=SEQUENCE(1,Z116)"));
        assert_eq!(sideways_widths(&t), vec![1; 16], "after loading, {threads} threads");
        t.set("Z100", "5");
        assert_eq!(sideways_widths(&t), vec![5; 16], "after the edit, {threads} threads");
        assert_eq!((t.num("A16"), t.num("E16")), (5.0, 25.0), "{threads} threads");
    }
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
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
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
        assert_eq!(filled, vec![9; 12], "{threads} threads");
        // The last anchor is at AH34: its corner, AJ36, is the ninth number.
        assert_eq!(t.num("AJ36"), 9.0, "{threads} threads");
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{threads} threads");
    }
}

#[test]
fn one_reader_of_a_new_array_costs_one_small_round_not_the_first_round_again() {
    // A1:A2000 are formulas, E2 spills three columns and depends on A1, and H1 reads F3, a cell
    // only E2's array covers (to the right of its anchor, under no formula of column F). H1 is
    // planned before E2 and reads F3 empty; one more round with H1 alone settles it, as the
    // write-back rounds do in row-major order. The first round is not calculated twice.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (1..=2000).map(|r| (format!("A{r}"), "=ROW()*2".to_string())).collect();
        cells.push(("E2".to_string(), "=SEQUENCE(5,3)+A1".to_string()));
        cells.push(("H1".to_string(), "=F3+1".to_string()));
        t.load(cells);
        assert_eq!(t.num("F3"), 7.0, "{threads} threads");
        assert_eq!(t.num("H1"), 8.0, "{threads} threads");
        assert_eq!(t.calc.formulas_calculated, 2000 + 1 + 1 + 1, "{threads} threads: the A column, E2, H1, then H1 once more");
    }
}

#[test]
fn an_indirect_reader_of_a_spill_sized_by_another_spill_stays_stale_after_edits_as_in_row_major_order() {
    // A1 spills three rows once Z2 is known; C1's height comes from A3, a cell A1 spills over,
    // and B1 reads C3 through INDIRECT, with no static link to either. Dynamic formulas like B1
    // run once more after the first round that lays out an array, as without a plan: that is
    // enough after loading and after a full recalculation, but after the two edits C1 spills over
    // C3 a round after B1's rerun, and B1 keeps 0. That is the value row-major order gives, and
    // it is stale (upstream issue #4469: Excel shows 3 at once); the planned and threaded runs
    // give the same. A fix of #4469 on the row-major path flips the last assertion to 3.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            [("Z1", "3"), ("Z2", "=Z1"), ("A1", "=SEQUENCE(Z2)"), ("B1", "=INDIRECT(\"C3\")"), ("C1", "=SEQUENCE(MAX(1,N(A3)))")]
                .map(|(at, input)| (at.to_string(), input.to_string())),
        );
        assert_eq!(t.num("C3"), 3.0, "{threads} threads");
        assert_eq!(t.num("B1"), 3.0, "after loading, {threads} threads");
        t.calc.recalc_all(&mut t.wb);
        assert_eq!(t.num("B1"), 3.0, "after a second full recalc, {threads} threads");
        t.set("Z1", "2");
        t.set("Z1", "3");
        assert_eq!(t.num("B1"), 0.0, "after two edits, {threads} threads: row-major order's stale value (#4469)");
    }
}

#[test]
fn formulas_handed_back_by_workers_are_right_and_counted_once() {
    // Each C reads a B through INDIRECT: no static edge, so C shares B's level. C1 reads B600,
    // which another worker has or nobody has yet; a worker never sees another's results, so it
    // hands C1 back to the main thread, which calculates it once the level is merged. Each
    // formula counts once however it was reached.
    let mut t = T::new().threads(8);
    // `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
    t.calc.parallel_min = 512;
    t.load((1..=600).flat_map(|r| [(format!("B{r}"), "=ROW()*3".to_string()), (format!("C{r}"), "=INDIRECT(\"B\"&(601-ROW()))+1".to_string())]));
    for r in [1u32, 300, 600] {
        assert_eq!(t.num(&format!("C{r}")), f64::from((601 - r) * 3 + 1));
    }
    assert_eq!(t.calc.last_recalc_cells, 1200);
    assert!(t.calc.circular.is_empty());
    assert!(t.calc.worker_levels > 0);
    assert!(t.calc.handed_back > 0, "no formula was handed back");
}

#[test]
fn each_worker_result_is_found_in_the_results_of_the_worker_that_took_its_chunk() {
    // One level of 900 formulas in chunks of three, spread over two or four workers. Every fifth
    // C reads a B through INDIRECT, which a worker that hasn't calculated that B itself hands
    // back (the main thread calculates it at its turn, and the B it reads on demand if that B's
    // turn hasn't come); a random D stays on the main thread. The walk finds each other result
    // in the results of the worker whose chunk held it: a result not found there would be
    // calculated on the main thread again. With no array in the level, the formulas calculated
    // on the main thread at their turn are the ones handed back and the random ones, and the
    // values are one thread's.
    let book = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(3);
        let mut cells = Vec::new();
        for r in 1..=300u32 {
            cells.push((format!("A{r}"), r.to_string()));
            cells.push((format!("B{r}"), format!("=A{r}*3")));
            let reader = if r % 5 == 0 { "=INDIRECT(\"B\"&(301-ROW()))+1".to_string() } else { format!("=A{r}+1") };
            cells.push((format!("C{r}"), reader));
            cells.push((format!("E{r}"), format!("=A{r}&\"x\"")));
        }
        cells.push(("D1".to_string(), "=RAND()".to_string()));
        t.load(cells);
        t
    };
    let one = book(1);
    for threads in [2u32, 4] {
        let t = book(threads);
        let what = format!("{threads} threads");
        assert!(t.calc.worker_levels > 0 && t.calc.handed_back > 0, "{what}: the level ran on workers and some readers were handed back");
        assert_eq!(t.calc.rounds_redone_in_row_major, 0, "{what}: the planned round was kept");
        assert_eq!(t.calc.level_formulas_on_main, t.calc.handed_back + 1, "{what}: on the main thread, the formulas handed back and D1");
        same_values(&t, &one, &what);
    }
}

#[test]
fn random_draws_come_from_one_generator_in_order_on_any_thread_count() {
    // RAND directly (kept on the main thread by its formula) and through a LAMBDA name (handed
    // back when the draw happens): the same sequence as one thread, no duplicates.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.name("Roll", "LAMBDA(RAND())");
        t.load((1..=600).map(|r| (format!("A{r}"), "=Roll()".to_string())).chain((1..=600).map(|r| (format!("B{r}"), "=RAND()".to_string()))));
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "RAND");
    assert!(many.calc.worker_levels > 0 && many.calc.handed_back > 0, "the level ran on workers and the Roll() cells were handed back");
    let mut seen = std::collections::HashSet::new();
    for r in 1..=600 {
        for col in ["A", "B"] {
            let v = many.num(&format!("{col}{r}"));
            assert!((0.0..1.0).contains(&v), "{col}{r} = {v}");
            assert!(seen.insert(v.to_bits()), "{col}{r} repeats {v}");
        }
    }
}

#[test]
fn planned_random_draws_go_by_level_and_the_cells_computed_from_them_follow() {
    // The stated exception to row-major order's values: while a workbook is planned, random draws
    // go in level order, and the cells computed from the draws follow the values drawn.
    // A1 reads Z1, so it is a level after C1 and draws second when planned, first in row-major order.
    let cells = [("Z1", "=1"), ("A1", "=RAND()+Z1*0"), ("C1", "=RAND()"), ("D1", "=A1<C1")];
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        t.load(cells.iter().map(|(at, input)| (at.to_string(), input.to_string())));
        t
    };
    let shown = |t: &T| (t.num("A1"), t.num("C1"), t.get("D1"));
    let planned = book(1, None);
    assert!(planned.calc.plans_made > 0, "the load was planned");
    let (a1, c1, d1) = shown(&planned);
    assert_ne!(a1, c1, "two draws");
    assert_eq!(d1, Value::Bool(a1 < c1), "planned: D1 compares the values A1 and C1 show");
    for threads in [2, 4] {
        let t = book(threads, None);
        assert!(t.calc.plans_made > 0, "{threads} threads: the load was planned");
        assert_eq!(shown(&t), (a1, c1, d1.clone()), "{threads} threads: the same draws as on one thread");
    }
    let row_major = book(1, Some(0));
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
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load((1..n).map(|r| (format!("C{r}"), format!("=C{}+1", r + 1))).chain([(format!("C{n}"), "=1".to_string())]));
        assert_eq!(t.num("C1"), f64::from(n), "{threads} threads");
        assert!(t.calc.circular.is_empty());
        let keys: Vec<Key> = (1..=n).map(|r| (0, c(&format!("C{r}")))).collect();
        let plan = t.calc.graph.levels(&t.wb, &keys).unwrap();
        assert_eq!((plan.levels().count(), plan.rest().len()), (n as usize, 0), "{threads} threads: one level per formula");
        t.set(&format!("C{n}"), "=10");
        assert_eq!(t.num("C1"), f64::from(n + 9), "{threads} threads");
    }
}

#[test]
fn iterative_calculation_ignores_the_thread_count() {
    let book = |threads| {
        let mut t = T::new().threads(threads);
        // `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
        t.calc.parallel_min = 512;
        t.wb.calc.iterative = true;
        t.wb.calc.max_iterations = 50;
        // The cycles alone would never make a level; the plain formulas in C are a wide level
        // that eight threads would calculate on workers if iterative calculation didn't force one.
        t.load(
            (1..=600)
                .map(|r| (format!("A{r}"), format!("=B{r}+1")))
                .chain((1..=600).map(|r| (format!("B{r}"), format!("=A{r}*0.5"))))
                .chain((1..=600).map(|r| (format!("C{r}"), "=ROW()*2".to_string()))),
        );
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "iterative");
    assert!((one.num("A600") - 2.0).abs() < 0.01);
    assert_eq!(many.num("C600"), 1200.0);
    assert_eq!((one.calc.worker_levels, many.calc.worker_levels), (0, 0), "iterative calculation runs on one thread");
}

/// A loop the plan can't see: A1 reads B2, and B2 reads A1 only through `closing` (INDIRECT or
/// OFFSET) and C1 = 1 plainly. A plan would put B2 first; row-major order enters at A1. Iterative
/// calculation on with `iterations` at most; `budget` caps planning (`Some(0)`: row-major order).
/// On four threads, planning on one thread too and levels of any size on the workers: without
/// iteration, these rounds would be planned.
fn hidden_loop(closing: &str, iterations: u32, budget: Option<usize>) -> T {
    let mut t = T::new().threads(4);
    t.calc.parallel_min = 1;
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
    // of a pass in row-major order. Four threads, planning on one thread too and levels of any
    // size on the workers: without iteration, this round is planned.
    let book = |iterative: bool, budget: Option<usize>| {
        let mut t = T::new().threads(4);
        t.calc.parallel_min = 1;
        t.wb.calc.iterative = iterative;
        t.calc.plan_budget = budget;
        t.load(sideways_chain("=SEQUENCE(1,5)*Z116"));
        t
    };
    assert!(book(false, None).calc.plans_made > 0, "iteration off: planned");
    let (iterative, row_major) = (book(true, None), book(true, Some(0)));
    assert_eq!(iterative.calc.plans_made, 0, "no plan");
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
            let at = cell_name(row, col);
            if roll < 20 {
                numbers.push(at.clone());
                cells.push((at, format!("{}", xorshift(&mut s) % 7)));
            } else if roll < 75 {
                let (ar, ac) = ((xorshift(&mut s) % 8) as u32, (xorshift(&mut s) % 6) as u32);
                let (a, b) = (cell_name(ar, ac), cell_name((xorshift(&mut s) % 8) as u32, (xorshift(&mut s) % 6) as u32));
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
    // full recalculation, the default run matches a run in row-major order without a plan, and
    // a run on four threads matches one thread, in values, spills and circular references.
    // `GRIDCRAFT_ITERATIVE_FUZZ_CASES` soaks it over more.
    let cases: u64 = std::env::var("GRIDCRAFT_ITERATIVE_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    for case in 0..cases {
        let seed = (case + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let (cells, numbers) = random_iterative_sheet(seed);
        let mut s = seed.max(1);
        let edits: Vec<(String, String)> = (0..3)
            .map(|_| {
                let at = if numbers.is_empty() {
                    cell_name((xorshift(&mut s) % 8) as u32, (xorshift(&mut s) % 6) as u32)
                } else {
                    numbers[(xorshift(&mut s) % numbers.len() as u64) as usize].clone()
                };
                (at, format!("{}", 10 + xorshift(&mut s) % 9))
            })
            .collect();
        let book = |budget: Option<usize>, threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            t.wb.calc.iterative = true;
            t.wb.calc.max_iterations = if case % 2 == 0 { 3 } else { 100 };
            t.calc.plan_budget = budget;
            t.load(cells.iter().cloned());
            t
        };
        let mut books = [(book(None, 1), "one thread"), (book(Some(0), 1), "row-major order"), (book(None, 4), "four threads")];
        let agree = |books: &[(T, &str)], step: &str| {
            for (other, label) in &books[1..] {
                same_values(&books[0].0, other, &format!("case {case} ({cells:?}), {step}, {label}"));
            }
        };
        agree(&books, "opened");
        for (at, value) in &edits {
            for (t, _) in &mut books {
                t.set(at, value);
            }
            agree(&books, &format!("{at} = {value}"));
        }
        for (t, _) in &mut books {
            t.calc.recalc_all(&mut t.wb);
        }
        agree(&books, "full recalculation");
    }
}

fn xorshift(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn cell_name(row: u32, col: u32) -> String {
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
            out.push((cell_name(r, c), format!("{}", xorshift(&mut s) % 100)));
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
            let range = (top..=pr).all(|rr| rank.get(&(rr, pc)).is_some_and(|&k| k < i)).then(|| (cell_name(top, pc), cell_name(pr, pc)));
            terms.push(match xorshift(&mut s) % 7 {
                0 => format!("INDIRECT(\"{}\")", cell_name(pr, pc)),
                1 => {
                    let name = format!("Name_{}", names.len());
                    names.push((name.clone(), format!("Sheet1!${}${}", char::from(b'A' + pc as u8), pr + 1)));
                    name
                }
                2 => match &range {
                    Some((a, b)) => format!("SUM({a}:{b})"),
                    None => cell_name(pr, pc),
                },
                // A defined name over the range.
                3 => match &range {
                    Some(_) => {
                        let name = format!("Range_{}", names.len());
                        let col = char::from(b'A' + pc as u8);
                        names.push((name.clone(), format!("Sheet1!${col}${}:${col}${}", top + 1, pr + 1)));
                        format!("SUM({name})")
                    }
                    None => cell_name(pr, pc),
                },
                // OFFSET with a computed offset (a dynamic reference).
                4 => format!("N(OFFSET($A$1,{pr}+0*ROW(),{pc}))"),
                5 => match &range {
                    Some((a, b)) => format!("INDEX({a}:{b},1)"),
                    None => cell_name(pr, pc),
                },
                _ => cell_name(pr, pc),
            });
        }
        out.push((cell_name(r, c), format!("=MOD({},1000)", terms.join("+"))));
    }
    (out, names)
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
fn a_plan_given_up_for_its_cost_gives_the_same_values_in_row_major_order() {
    // The budget is generous (a million units plus hundreds per formula); a test caps it at
    // nothing, so planning is given up at the first formula and the pass runs row-major with
    // the usual on-demand evaluation, on any thread count.
    for seed in [3u64, 11] {
        let (cells, names) = random_acyclic_sheet(seed * 104_729, 300, 8);
        let book = |cap: Option<usize>, threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            t.calc.plan_budget = cap;
            for (name, formula) in &names {
                t.name(name, formula);
            }
            t.load(cells.iter().cloned());
            t
        };
        let planned = book(None, 1);
        let keys: Vec<Key> = planned.wb.sheet(0).unwrap().cells.iter().filter(|(_, cell)| cell.formula.is_some()).map(|(at, _)| (0, at)).collect();
        assert!(planned.calc.graph.levels(&planned.wb, &keys).is_ok());
        assert_eq!(planned.calc.graph.levels_within(&planned.wb, &keys, Some(0)).err(), Some(NoPlan::Other));
        for threads in [1, 8] {
            let fallback = book(Some(0), threads);
            same_values(&planned, &fallback, &format!("seed {seed}, no plan, {threads} threads"));
            assert_eq!(fallback.calc.worker_levels, 0, "seed {seed}: nothing is threaded without a plan");
        }
    }
}

#[test]
fn the_generated_defined_names_are_names_not_references() {
    // A name such as `N5` or `R7` is read as the cell in column N or R, so the generated sheets
    // would read empty cells and test no name precedents at all; Excel refuses a name in A1 or
    // R1C1 form (`R`, `C`, `RC`, `R2C3`) outright.
    let r1c1 = |n: &str| {
        let digits = |s: &str| s.bytes().take_while(u8::is_ascii_digit).count();
        let mut rest = n;
        if let Some(r) = rest.strip_prefix(['R', 'r']) {
            rest = r.get(digits(r)..).unwrap_or("");
        }
        if let Some(c) = rest.strip_prefix(['C', 'c']) {
            rest = c.get(digits(c)..).unwrap_or("");
        }
        rest.is_empty()
    };
    for seed in 1..=8 {
        let (_, names) = random_acyclic_sheet(seed, 12, 8);
        assert!(!names.is_empty(), "seed {seed} defines names");
        for (name, _) in names {
            let reference = gridcraft_core::CellRef::parse(&name).is_some() || gridcraft_core::RangeRef::parse(&name).is_some() || r1c1(&name);
            assert!(!reference, "`{name}` would be read as a reference, not a name");
        }
    }
}

#[test]
fn full_recalc_matches_entering_cells_one_by_one_at_any_thread_count() {
    // `GRIDCRAFT_ORDER_FUZZ_SEEDS=50` soaks it.
    let seeds: u64 = std::env::var("GRIDCRAFT_ORDER_FUZZ_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    for seed in 1..=seeds {
        let (cells, names) = random_acyclic_sheet(seed * 7919, 400, 8);
        let book = |threads: u32| {
            let mut t = T::new().threads(threads);
            // Every level threaded, whatever `GRIDCRAFT_PARALLEL_MIN` says.
            t.calc.parallel_min = 1;
            for (name, formula) in &names {
                t.name(name, formula);
            }
            t
        };
        // Reference: entered in order, each formula reading only finished cells.
        let mut want = book(1);
        for (at, input) in &cells {
            want.set(at, input);
        }
        for threads in [1, 8] {
            let mut t = book(threads);
            t.load(cells.iter().cloned());
            let wrong: Vec<_> =
                cells.iter().filter(|(at, _)| t.get(at) != want.get(at)).map(|(at, f)| (at, f, t.get(at), want.get(at))).take(5).collect();
            assert!(wrong.is_empty(), "seed {seed}, {threads} threads: {wrong:?}");
            assert!(t.calc.circular.is_empty(), "seed {seed}, {threads} threads: circular");
            // The levels really ran on workers.
            assert!(threads == 1 || t.calc.worker_levels > 0, "seed {seed}: no level on workers");
        }
    }
}

#[test]
fn manual_mode_evaluates_only_the_edited_formulas_on_any_thread_count() {
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load((1..=600).flat_map(|r| [(format!("A{r}"), format!("={r}")), (format!("B{r}"), format!("=A{r}*2"))]));
        t.wb.calc.mode = gridcraft_model::CalcMode::Manual;
        // Entering 600 formulas at once in manual mode evaluates those and nothing else.
        for r in 1..=600 {
            t.wb.sheet_mut(0).unwrap().cells.set(c(&format!("A{r}")), Cell::formula(Formula::new(&format!("={}", r + 1000))));
        }
        let keys: Vec<Key> = (1..=600).map(|r| (0, c(&format!("A{r}")))).collect();
        t.calc.cells_changed(&mut t.wb, &keys);
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "manual");
    assert_eq!((many.num("A600"), many.num("B600")), (1600.0, 1200.0));
    assert_eq!(many.calc.last_recalc_cells, 600);
    assert!(many.calc.worker_levels > 0, "the edited formulas ran on workers");
}

/// A workbook with cycles reached in several ways: the error each cell shows, and the circular
/// list, must not depend on the thread count (every pass is planned the same way).
#[test]
fn cycles_land_on_the_same_cells_on_any_thread_count() {
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = Vec::new();
        for r in 1..=600 {
            cells.push((format!("A{r}"), format!("=B{r}+1")));
            cells.push((format!("B{r}"), "=INDIRECT(\"A\"&ROW())*2".to_string()));
            cells.push((format!("C{r}"), format!("=A{r}+SUM(D:D)")));
            cells.push((format!("D{r}"), format!("=C{r}+ROW()")));
            cells.push((format!("E{r}"), "=ROW()*3".to_string()));
        }
        t.load(cells);
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "cycles");
    assert_eq!(many.get("A5"), Value::Error(CellError::Circ));
    assert_eq!(many.get("D5"), Value::Error(CellError::Circ));
    assert_eq!(many.num("E600"), 1800.0);
    assert!(!many.calc.circular.is_empty());
    assert!(many.calc.worker_levels > 0, "levels ran on workers");
}

#[test]
fn a_spill_made_on_one_worker_is_read_by_formulas_on_the_same_level() {
    // Entered together, the anchor has no spill range yet, and its array goes sideways, into
    // cells no dirty formula sits above, so the planner can't hold its readers back: they share
    // its level. A1's worker result lays an array out where none is stored, so its readers after
    // it are calculated again on the main thread at their turn, with the array in sight (one
    // level with formulas calculated again): the readers are right in the first round. The
    // write-back runs them once more, as it runs the readers of any array laid out anew, on any
    // thread count.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        // `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
        t.calc.parallel_min = 512;
        t.set("Z1", "10");
        t.load((1..=600).map(|r| (format!("G{r}"), "=SUM(B1:E1)+ROW()".to_string())).chain([("A1".to_string(), "=SEQUENCE(1,5,Z1)".to_string())]));
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "spill read on the same level");
    assert_eq!(many.num("G600"), 11.0 + 12.0 + 13.0 + 14.0 + 600.0);
    assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells);
    assert_eq!((one.calc.last_recalc_cells, many.calc.relaid_levels), (1201, 1), "the readers twice, A1 once; one level run again");
}

#[test]
fn two_anchors_colliding_in_a_wide_level_resolve_as_on_one_thread() {
    // Workers take the level in row-major chunks of 32: A1 is the first formula and A50 the
    // fifty-first, so the two anchors are in different chunks, usually on different workers.
    // Either way their collision is resolved in the write-back, from the values alone.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            (1..=600)
                .map(|r| (format!("F{r}"), "=ROW()".to_string()))
                .chain([("A1".to_string(), "=SEQUENCE(100)".to_string()), ("A50".to_string(), "=SEQUENCE(100)".to_string())]),
        );
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "colliding anchors");
    assert!(many.calc.worker_levels > 0);
    // A1's area holds A50's formula, so A1 can't spill; A50's area is free.
    assert_eq!(many.get("A1"), Value::Error(CellError::Spill));
    assert_eq!((many.num("A50"), many.num("A149")), (1.0, 100.0));
}

#[test]
fn a_worker_leaves_cells_to_the_spill_that_held_them_as_on_one_thread() {
    // E10 spills along row 10; F8, first in reading order, is entered afterwards and wants F10.
    // The spill that already holds the cells keeps them (`Standing`): F8 shows #SPILL! in the
    // pass, on a worker as on the main thread, and the cells under it don't change, so F8 is the
    // only formula calculated. A worker deciding by reading order alone laid F8's array out, and
    // the write-back blocked it and calculated the readers of its area again.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            (1..=40).map(|r| (format!("H{r}"), "=SUM($F$9:$F$12)+ROW()".to_string())).chain([("E10".to_string(), "=SEQUENCE(1,3)*10".to_string())]),
        );
        t.set("F8", "=SEQUENCE(5)");
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "a new array over a spill that holds its cells");
    assert!(many.calc.worker_levels > 0);
    assert_eq!(many.get("F8"), Value::Error(CellError::Spill));
    assert_eq!((many.num("F10"), many.num("H1")), (20.0, 21.0));
    assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "F8 alone");
    assert_eq!(one.calc.last_recalc_cells, 1);
}

#[test]
fn a_spill_range_read_on_a_worker_before_its_anchor_is_done_is_handed_back() {
    // `A700#` written in a formula is a static edge, so its readers would be planned after the
    // anchor. Read through a name that INDIRECT names, nothing is known statically: the readers
    // share the anchor's level, and the anchor is that level's last formula, so every worker
    // asks for a spill range that isn't there yet and hands the reader back to the main thread.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        // `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
        t.calc.parallel_min = 512;
        t.name("Seq", "Sheet1!$A$700#");
        t.load(
            (1..=600)
                .map(|r| (format!("C{r}"), "=ROWS(INDIRECT(\"Seq\"))+COLUMNS(INDIRECT(\"Seq\"))".to_string()))
                .chain([("A700".to_string(), "=SEQUENCE(7)".to_string())]),
        );
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "spill range on the same level");
    assert_eq!(many.num("C600"), 8.0);
    assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells);
    assert_eq!(many.calc.handed_back, 600, "every reader met the anchor before it was done");
    let keys: Vec<Key> = (1..=600).map(|r| (0, c(&format!("C{r}")))).chain([(0, c("A700"))]).collect();
    let plan = one.calc.graph.levels(&one.wb, &keys).unwrap();
    assert_eq!(plan.levels().count(), 1, "the anchor and its readers share a level: {:?}", plan.rest());
}

#[test]
fn a_spill_landing_in_a_block_a_worker_has_read_ends_as_on_one_thread() {
    // One worker (two threads, three formulas, one chunk) calculates A1, A2, A3 in order: A1
    // reads the 100-cell block B2:CW2 (empty), A2 spills over it, A3 reads the block again. A2's
    // array is new, so A3, which reads its area, is calculated again on the main thread at its
    // turn and what it read on the worker is never kept, and the write-back runs both readers
    // again: the values and the cell count match one thread. A worker's copy of a block is
    // dropped by its own spill as well, which this test doesn't reach (A3 is calculated again
    // anyway); `recalc::tests::a_workers_copied_block_is_dropped_by_its_own_spill` checks it
    // directly.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load([("A1", "=SUM(B2:CW2)"), ("A2", "=SEQUENCE(1,200)"), ("A3", "=SUM(B2:CW2)")].map(|(a, f)| (a.to_string(), f.to_string())));
        t
    };
    let (one, two) = (book(1), book(2));
    same_values(&one, &two, "copied block under a spill");
    let want: f64 = (2..=101).map(f64::from).sum();
    assert_eq!((two.num("A1"), two.num("A3")), (want, want));
    assert_eq!(two.calc.last_recalc_cells, one.calc.last_recalc_cells);
    assert_eq!(two.calc.relaid_levels, 1, "A3 calculated again after A2's array");
}

#[test]
fn random_formulas_in_a_wide_level_are_planned_for_the_main_thread() {
    // A worker can't draw from the one generator, so a random formula on a worker would be handed
    // back; `run_level` keeps them for the main thread instead, and the rest of the level still
    // runs on workers, with nothing handed back or fallen back.
    let mut t = T::new().threads(8);
    t.calc.parallel_min = 1;
    t.load((1..=600).flat_map(|r| [(format!("A{r}"), "=RAND()".to_string()), (format!("B{r}"), "=ROW()".to_string())]));
    assert!(t.calc.worker_levels > 0, "the plain half of the level ran on workers");
    assert_eq!((t.calc.worker_fallbacks, t.calc.handed_back), (0, 0), "random formulas never reach a worker");
    assert_eq!(t.num("B600"), 600.0);
    let mut seen = std::collections::HashSet::new();
    for r in 1..=600 {
        let v = t.num(&format!("A{r}"));
        assert!((0.0..1.0).contains(&v) && seen.insert(v.to_bits()), "A{r} = {v}");
    }
}

#[test]
fn prefixed_random_formulas_in_a_wide_level_are_planned_for_the_main_thread() {
    // The parser leaves a prefix on a function's name (`_xlws._xlfn.RAND()` reaches the planner
    // as `_XLFN.RAND`) and the registry strips it, so these draw as RAND, RANDARRAY and
    // RANDBETWEEN do: kept for the main thread, never handed back, values one thread's.
    for random in ["=_xlws._xlfn.RAND()", "=_xlfn._xlfn.RANDARRAY(1,1)", "=_xlfn._xlfn.RANDBETWEEN(1,1000000)"] {
        let book = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            t.load((1..=600).flat_map(|r| [(format!("A{r}"), random.to_string()), (format!("B{r}"), "=ROW()".to_string())]));
            t
        };
        let (one, four) = (book(1), book(4));
        assert!(four.calc.worker_levels > 0, "{random}: the plain half of the level ran on workers");
        assert_eq!((four.calc.worker_fallbacks, four.calc.handed_back), (0, 0), "{random}: random formulas never reach a worker");
        same_values(&four, &one, random);
        assert_eq!(four.num("B600"), 600.0);
        // Drawn, not one value repeated (RANDBETWEEN may repeat a few of a million).
        let seen: std::collections::HashSet<u64> = (1..=600).map(|r| four.num(&format!("A{r}")).to_bits()).collect();
        assert!(seen.len() > 500, "{random}: {} distinct values", seen.len());
    }
}

#[test]
fn a_wide_level_of_random_formulas_and_few_others_starts_no_workers() {
    // 600 formulas drawing random numbers and three others make a level of 603, past the 512 a
    // level needs for workers; but the random ones stay on the main thread, and three formulas
    // are too few to start threads for. Values and draws are one thread's.
    let book = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 512;
        let mut cells: Vec<(String, String)> = vec![("A1".to_string(), "1".to_string())];
        cells.extend((2..=601).map(|r| (format!("B{r}"), "=RAND()+$A$1".to_string())));
        cells.extend((2..=4).map(|r| (format!("C{r}"), "=$A$1+1".to_string())));
        t.load(cells);
        t
    };
    let mut one = book(1);
    one.set("A1", "2");
    for threads in [2, 4] {
        let mut t = book(threads);
        t.set("A1", "2");
        assert_eq!(t.calc.worker_levels, 0, "{threads} threads: no workers for three formulas");
        same_values(&t, &one, &format!("{threads} threads"));
    }
    assert_eq!(one.num("C4"), 3.0);
}

#[test]
fn a_failing_worker_falls_back_to_the_main_thread() {
    let book = |fail| {
        let mut t = T::new().threads(8);
        t.calc.parallel_min = 1;
        t.calc.fail_workers = fail;
        t.load((1..=600).flat_map(|r| [(format!("A{r}"), format!("={r}")), (format!("B{r}"), format!("=A{r}*2+SUM(A1:A3)"))]));
        t
    };
    let (ok, failed) = (book(false), book(true));
    same_values(&ok, &failed, "worker failure");
    assert_eq!(failed.num("B600"), 1206.0);
    assert_eq!(ok.calc.worker_fallbacks, 0);
    assert!(ok.calc.worker_levels > 0, "the second level ran on workers");
    assert_eq!(ok.calc.handed_back, 0, "the second level reads the first through the main host, so nothing is handed back");
    assert_eq!(failed.calc.worker_fallbacks, 2, "one fallback per threaded level");
}

#[test]
fn legacy_array_formulas_fill_their_range_on_workers() {
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.set("A1", "1");
        t.set("A2", "2");
        for r in 0..40u32 {
            let col = format!("{}{}", char::from(b'C' + (r % 20) as u8), (r / 20) * 4 + 1);
            let mut f = Formula::new("=A1:A2*2");
            f.array = gridcraft_core::RangeRef::parse(&format!("{col}:{}{}", char::from(b'C' + (r % 20) as u8), (r / 20) * 4 + 3));
            t.wb.sheet_mut(0).unwrap().cells.set(c(&col), Cell::formula(f));
        }
        t.calc.recalc_all(&mut t.wb);
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "legacy arrays");
    assert!(many.calc.worker_levels > 0, "the level ran on workers");
    assert_eq!((many.num("C2"), many.get("C3")), (4.0, Value::Error(CellError::NA)));
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
fn random_draws_in_a_level_come_in_row_major_order_on_any_thread_count() {
    // Odd rows draw, even rows don't, and the level is released in a mixed order: the draws
    // still come row by row, on one thread (sorted level) and on many (the main thread's turn),
    // checked against the generator itself.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (1..=600).map(|r| (format!("A{r}"), "=ROW()".to_string())).collect();
        for r in 1..=600 {
            let f = if r % 2 == 1 { format!("=A{}+RAND()", 601 - r) } else { format!("=A{}*2", 601 - r) };
            cells.push((format!("B{r}"), f));
        }
        t.load(cells);
        t
    };
    let one = book(1);
    // The generator `Calc` starts from (xorshift64*), drawn as `Host::random` draws.
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut draw = move || {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        (state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    };
    for r in (1..=600).step_by(2) {
        assert_eq!(one.num(&format!("B{r}")), f64::from(601 - r) + draw(), "one thread: row {r} draws in row order");
    }
    for threads in [2, 8] {
        same_values(&one, &book(threads), &format!("{threads} threads"));
    }
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
fn a_chain_of_new_spills_read_across_workers_matches_one_thread() {
    // Each anchor, in row 1, spills one column more than the cell it reads from the previous
    // anchor's new array; the workbook stores no spill ranges yet and no dirty formula sits above
    // the cells read, so the planner can't hold the readers back and the whole chain shares one
    // level. A reader in another worker's chunk reads the stored (empty) cells where one thread,
    // in row-major order, reads the array: each reader of a new array is calculated again on the
    // main thread at its turn, after the array, as one thread calculates it. One formula per
    // chunk, so that the anchors land on different workers (the default chunk holds all twelve,
    // row 1 coming first); run several times, since which worker takes which chunk varies from
    // run to run.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(1);
        let mut cells: Vec<(String, String)> = (2..=1_201).map(|r| (format!("B{r}"), "=ROW()".to_string())).collect();
        cells.push(("A1".to_string(), "=SEQUENCE(1,2)".to_string()));
        for k in 1..=11u32 {
            let read = CellRef::new(0, 20 * (k - 1) + k);
            cells.push((CellRef::new(0, 20 * k).to_string(), format!("=SEQUENCE(1,{read}+1)")));
        }
        t.load(cells);
        t
    };
    let one = build(1);
    for k in 0..=11u32 {
        assert_eq!(one.num(&CellRef::new(0, 20 * k + k + 1).to_string()), f64::from(k + 2), "one thread: anchor {k} spills {} columns", k + 2);
    }
    for run in 0..5 {
        let many = build(8);
        same_values(&one, &many, &format!("spill chain across workers, run {run}"));
        assert!(many.calc.worker_levels > 0, "run {run}: the level ran on workers");
        assert_eq!(many.calc.relaid_levels, 1, "run {run}: readers calculated again after a new array");
    }
}

#[test]
fn formulas_before_a_new_array_read_the_stored_cells_on_any_thread_count() {
    // A random formula and a formula handed back are calculated on the main thread after the
    // workers; sorting before an anchor whose array is new, they must read the cells the workbook
    // stores, as one thread had them at that point, not the array the workers merged. Here B1
    // reads D105, a cell of C105's new array, which goes sideways (no dirty formula sits above
    // D105 to hold B1 back): on one thread it is empty when B1 runs, so B1 is 0 and A2's array
    // keeps B2; with the array in sight B1 would spill over B2 instead and A2 would lose the
    // collision.
    for (name, first) in [("random", "=IF(D105=2,SEQUENCE(2)+0*RAND(),0)"), ("handed back", "=IF(INDIRECT(\"Z1\")+D105=3,SEQUENCE(2),0)")] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (3..=99).map(|r| (format!("A{r}"), "=ROW()".to_string())).collect();
            cells.extend((2..=700).map(|r| (format!("F{r}"), "=ROW()".to_string())));
            cells.push(("B1".to_string(), first.to_string()));
            cells.push(("A2".to_string(), "=SEQUENCE(1,2)".to_string()));
            cells.push(("C105".to_string(), "=SEQUENCE(1,3)".to_string()));
            cells.push(("Z1".to_string(), "=ROW()".to_string()));
            t.load(cells);
            t
        };
        let one = build(1);
        assert_eq!(one.num("B2"), 2.0, "{name}: one thread, A2's array keeps B2");
        for run in 0..4 {
            let many = build(8);
            same_values(&one, &many, &format!("{name} formula before a new array, run {run}"));
            assert!(many.calc.worker_levels > 0, "{name} run {run}: the level ran on workers");
            if name == "handed back" {
                assert!(many.calc.handed_back >= 1, "run {run}: B1 was handed back");
            } else {
                assert!(many.calc.level_formulas_on_main >= 1, "run {run}: B1 ran on the main thread");
            }
        }
    }
}

#[test]
fn an_array_laid_out_after_the_workers_runs_its_readers_again() {
    // After the load every array is stored. The edit grows A1's array by a row, and A1 is random
    // (or handed back), so no worker lays it out: it is laid out on the main thread after the
    // workers, and D100, after it in row-major order, had read the stored (empty) A3 on its
    // worker where one thread, after A1, reads 3 and spills into B101's stored array, which
    // blocks it. No worker lays out a changed array here, so only the main thread's layout can
    // have D100 calculated again.
    for (name, anchor) in [("random", "=SEQUENCE(Z1+1)+0*RAND()"), ("handed back", "=SEQUENCE(INDIRECT(\"Z1\")+1)")] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
            cells.push(("Z1".to_string(), "1".to_string()));
            cells.push(("A1".to_string(), anchor.to_string()));
            cells.push(("D100".to_string(), "=IF(INDIRECT(\"A3\")=3,SEQUENCE(2),0)".to_string()));
            cells.push(("B101".to_string(), "=SEQUENCE(1,3)".to_string()));
            t.load(cells);
            assert_eq!((t.num("A2"), t.num("D100"), t.num("D101")), (2.0, 0.0, 3.0), "{name}, {threads} threads: after load");
            t.set("Z1", "2");
            t
        };
        let one = build(1);
        assert_eq!(
            (one.num("A3"), one.get("D100"), one.num("D101")),
            (3.0, Value::Error(CellError::Spill), 3.0),
            "{name}: one thread, D100 can't spill over B101's array"
        );
        for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2), (8, 3)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name} anchor laid out after the workers, {threads} threads, run {run}"));
            assert!(
                many.calc.worker_levels > 0 && many.calc.relaid_levels == 1,
                "{name} run {run}: the level ran on workers and the formulas after the array were calculated again"
            );
        }
    }
}

#[test]
fn an_array_laid_out_on_demand_is_seen_by_the_formulas_after_the_reader() {
    // B1 reads C60 through INDIRECT (no static edge, so they share a level, and a worker hands
    // B1 back), and one thread lays C60's changed array out right there, before the formulas
    // between B1 and C60 run: D30 then sees C61 at 4 and spills, into B31's stored array, which
    // blocks it. The workers calculated D30 with C61 still 2, and C60 is random, so no worker
    // lays out a changed array: D30 is calculated again on the main thread at its turn, for the
    // array laid out on demand before it.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
        cells.push(("Z1".to_string(), "1".to_string()));
        cells.push(("B1".to_string(), "=INDIRECT(\"C60\")".to_string()));
        cells.push(("D30".to_string(), "=IF(INDIRECT(\"C61\")=4,SEQUENCE(2),0)".to_string()));
        cells.push(("B31".to_string(), "=SEQUENCE(1,3)".to_string()));
        cells.push(("C60".to_string(), "=SEQUENCE(2)*Z1+0*RAND()".to_string()));
        t.load(cells);
        assert_eq!((t.num("B1"), t.num("D30")), (1.0, 0.0), "{threads} threads: after load");
        // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
        t.calc.row_major_only = false;
        t.set("Z1", "2");
        t
    };
    let one = build(1);
    assert_eq!(
        (one.num("B1"), one.num("C61"), one.get("D30")),
        (2.0, 4.0, Value::Error(CellError::Spill)),
        "one thread: D30 spills and B31's array blocks it"
    );
    for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2), (8, 3)] {
        let many = build(threads);
        same_values(&one, &many, &format!("array laid out on demand, {threads} threads, run {run}"));
        assert!(
            many.calc.worker_levels > 0 && many.calc.handed_back >= 1 && many.calc.relaid_levels == 1,
            "run {run}: the level ran on workers, B1 was handed back, and D30 was calculated again"
        );
    }
}

#[test]
fn a_result_read_on_demand_before_an_array_laid_out_after_the_workers_matches_one_thread() {
    // B5 reads D30 through INDIRECT; a worker hands B5 back, and on the main thread B5 reads the
    // worker's result for D30. One thread calculates D30 on demand right there, before C10's
    // array (C10 is random, so laid out after the workers), so D30 reads C11 as stored and both
    // are run again in the next round, where D30 spills into B31's stored array and is blocked.
    // The level does the same: D30, calculated on demand before C10, keeps that result (its
    // worker result is thrown away), and nothing after C10 is calculated again.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
        cells.push(("Z1".to_string(), "1".to_string()));
        cells.push(("B5".to_string(), "=INDIRECT(\"D30\")".to_string()));
        cells.push(("C10".to_string(), "=SEQUENCE(2)*Z1+0*RAND()".to_string()));
        cells.push(("D30".to_string(), "=IF(INDIRECT(\"C11\")=4,SEQUENCE(2),0)".to_string()));
        cells.push(("B31".to_string(), "=SEQUENCE(1,3)".to_string()));
        t.load(cells);
        assert_eq!((t.num("B5"), t.num("D30")), (0.0, 0.0), "{threads} threads: after load");
        // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
        t.calc.row_major_only = false;
        t.set("Z1", "2");
        t
    };
    let one = build(1);
    assert_eq!((one.get("B5"), one.get("D30"), one.num("D31")), (Value::Error(CellError::Spill), Value::Error(CellError::Spill), 3.0), "one thread");
    for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2), (8, 3)] {
        let many = build(threads);
        same_values(&one, &many, &format!("result read on demand before an array laid out after the workers, {threads} threads, run {run}"));
        assert!(
            many.calc.worker_levels > 0 && many.calc.handed_back >= 1 && many.calc.relaid_levels == 0,
            "run {run}: the level ran on workers, B5 was handed back, nothing calculated again"
        );
    }
}

#[test]
fn an_array_with_new_values_in_its_old_place_is_a_new_array_to_a_dynamic_reader() {
    // A1's array keeps its place and shape when Z1 changes, so the planner holds its static
    // readers back, but D100 reads A2 through INDIRECT and shares the level: on another worker it
    // read the stored value where one thread, after A1, read the new one and let D100 spill into
    // B101's stored array, which blocks it. Laid out by a worker, which has the level calculated
    // again from the workers' side, or on the main thread when A1 is random as well, where only
    // the main thread's layout can.
    for (name, anchor) in [("worker", "=SEQUENCE(2)+Z1"), ("main thread", "=SEQUENCE(2)+Z1+0*RAND()")] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=SUMPRODUCT(SEQUENCE(20000))+ROW()+Z1".to_string())).collect();
            cells.push(("Z1".to_string(), "1".to_string()));
            cells.push(("A1".to_string(), anchor.to_string()));
            cells.push(("D100".to_string(), "=IF(INDIRECT(\"A2\")=13,SEQUENCE(2),0)".to_string()));
            cells.push(("B101".to_string(), "=SEQUENCE(1,3)".to_string()));
            t.load(cells);
            t.set("Z1", "11");
            t
        };
        let one = build(1);
        assert_eq!(
            (one.num("A2"), one.get("D100")),
            (13.0, Value::Error(CellError::Spill)),
            "{name}: one thread, D100 spills and B101's array blocks it"
        );
        for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2), (8, 3)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name}-laid array with new values, {threads} threads, run {run}"));
            assert!(
                many.calc.worker_levels > 0 && many.calc.relaid_levels == 1,
                "{name} run {run}: the level ran on workers and the formulas after the array were calculated again"
            );
        }
    }
}

#[test]
fn arrays_with_new_values_in_their_old_place_leave_the_workers_results_that_cant_read_them() {
    // Forty arrays in row 3 keep their place and shape when BZ1 changes, with new values. Their
    // static readers in row 10 read a spilled cell and BZ1, so they are calculated in the same
    // round and the planner holds them back by the stored area alone, to the next level, where
    // they read the new values (the write-back runs them again after the round, to the same
    // values, as it runs the readers of any array laid out anew). The one dynamic formula of the
    // level, B2, comes before the arrays: nothing after them could read their new values, so the
    // workers' results stand and nothing is calculated again, with the first array laid out by a
    // worker or, random, on the main thread (the only formula calculated there, in both rounds:
    // random, it is dynamic, and dynamic formulas run once more after a round that lays an array
    // out). B2 read C4 as it was, as on one thread. The values and the formulas calculated are
    // one thread's.
    for (name, random, on_main) in [("worker", "", 0), ("main thread", "+0*RAND()", 2)] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (20..=99).map(|r| (format!("F{r}"), "=ROW()+$BZ$1".to_string())).collect();
            cells.push(("BZ1".to_string(), "1".to_string()));
            cells.push(("B2".to_string(), "=INDIRECT(\"C4\")*0+BZ1".to_string()));
            for col in 2..42 {
                let anchor = CellRef::new(2, col).a1();
                let spilled = CellRef::new(3, col).a1();
                let extra = if col == 2 { random } else { "" };
                cells.push((anchor, format!("=SEQUENCE(2)+$BZ$1{extra}")));
                cells.push((CellRef::new(9, col).a1(), format!("={spilled}*2+$BZ$1")));
            }
            cells.push(("H60".to_string(), "=INDIRECT(\"BZ2\")+F20".to_string()));
            t.load(cells);
            t.set("BZ1", "11");
            t
        };
        let one = build(1);
        assert_eq!((one.num("C4"), one.num("C10"), one.num("B2")), (13.0, 37.0, 11.0), "{name}: one thread");
        for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name}: arrays with new values in their old place, {threads} threads, run {run}"));
            assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "{name} run {run}: the same formulas calculated");
            assert_eq!(many.calc.level_formulas_on_main, on_main, "{name} run {run}: formulas calculated on the main thread");
            assert!(
                many.calc.worker_levels > 0 && many.calc.relaid_levels == 0 && many.calc.level_formulas_redone == 0,
                "{name} run {run}: nothing calculated again: {} levels, {} with formulas run again, {} formulas",
                many.calc.worker_levels,
                many.calc.relaid_levels,
                many.calc.level_formulas_redone
            );
        }
    }
}

#[test]
fn a_formula_after_a_thrown_away_worker_array_reads_as_on_one_thread() {
    // A1 is random, so it runs on the main thread, and lays out a new array over A1:B1. A3 reads
    // B1, so it is calculated again at its turn: with B1 at 2 it is a one-cell array. On its
    // worker, with B1 still empty, it spilled three columns, and D3, next in the same chunk, read
    // C3 from that array: 3 there, where one thread finds C3 empty. The worker's array is thrown
    // away with A3's result, so D3, which reads its area, is calculated again too. The arrays go
    // sideways, so no dirty formula above a cell read holds its reader back.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            [("A1", "=SEQUENCE(1,2)+0*RAND()"), ("A3", "=SEQUENCE(1,IF(B1=2,1,3))"), ("D3", "=C3+1")].map(|(a, f)| (a.to_string(), f.to_string())),
        );
        t
    };
    let one = build(1);
    assert_eq!((one.num("B1"), one.num("A3"), one.num("D3")), (2.0, 1.0, 1.0), "one thread");
    for (threads, run) in [(2, 0), (8, 0), (8, 1)] {
        let many = build(threads);
        same_values(&one, &many, &format!("formula after a thrown-away worker array, {threads} threads, run {run}"));
        assert!(many.calc.worker_levels > 0 && many.calc.level_formulas_redone >= 2, "run {run}: A3 and D3 calculated again");
    }
}

#[test]
fn a_reader_after_an_array_made_on_demand_reads_as_on_one_thread() {
    // A1 is random, so it runs on the main thread, and lays out A1:B1. A5 reads B10 through
    // INDIRECT, so a worker hands it back, and on the main thread it calculates B10 on demand,
    // after A1: with B1 at 2 that is a one-cell array. On its worker B10 had spilled three
    // columns with B1 still empty, and E10, next on that worker, read D10 from it: 3 there, where
    // one thread finds D10 empty. B10's worker array is thrown away when its turn comes (it was
    // calculated on demand already), so E10, which reads its area, is calculated again. The
    // arrays go sideways, so no dirty formula above a cell read holds its reader back.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            [("A1", "=SEQUENCE(1,2)+0*RAND()"), ("A5", "=INDIRECT(\"B10\")+0"), ("B10", "=SEQUENCE(1,IF(B1=2,1,3))"), ("E10", "=D10+1")]
                .map(|(a, f)| (a.to_string(), f.to_string())),
        );
        t
    };
    let one = build(1);
    assert_eq!((one.num("B10"), one.num("E10")), (1.0, 1.0), "one thread");
    for (threads, run) in [(2, 0), (2, 1), (8, 0), (8, 1)] {
        let many = build(threads);
        same_values(&one, &many, &format!("reader after an array made on demand, {threads} threads, run {run}"));
        assert!(many.calc.handed_back >= 1 && many.calc.level_formulas_redone >= 1, "run {run}: A5 handed back, E10 calculated again");
    }
}

#[test]
fn random_draws_stay_in_order_around_a_worker_pass_run_again() {
    // B5 and C10 draw random numbers, so both run on the main thread, in row-major order: B5
    // gets the first draw and C10 the second, as on one thread. C10's new array has no reader
    // in the level, so nothing after it is calculated again. The draws are compared value for
    // value.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
        cells.push(("B5".to_string(), "=RAND()".to_string()));
        cells.push(("C10".to_string(), "=SEQUENCE(2)+RAND()".to_string()));
        t.load(cells);
        t
    };
    let one = build(1);
    assert_ne!(one.num("B5"), one.num("C10") - 1.0, "one thread: two different draws");
    for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2)] {
        let many = build(threads);
        same_values(&one, &many, &format!("random draws around an array laid out after the workers, {threads} threads, run {run}"));
        assert!(
            many.calc.worker_levels > 0 && many.calc.relaid_levels == 0 && many.calc.level_formulas_on_main == 2,
            "run {run}: the level ran on workers, B5 and C10 on the main thread, nothing calculated again"
        );
    }
}

#[test]
fn a_volatile_array_does_not_run_the_dynamic_formulas_to_the_round_limit() {
    // RANDARRAY lays out new values every time it runs. Dynamic formulas run once more after the
    // first round that lays an array out, not after every one: an edit beside a RANDARRAY costs
    // a couple of evaluations, not eight rounds of them.
    let mut t = T::new();
    t.set("A1", "=RANDARRAY(3)");
    t.set("B1", "=ROW()");
    t.set("C1", "5");
    assert!(t.calc.last_recalc_cells <= 2, "the edit and at most the volatile array, not rounds of them: {}", t.calc.last_recalc_cells);
}

#[test]
fn a_dynamic_reader_after_a_new_array_sees_it_on_any_thread_count() {
    // D100 reads a cell of C1's new array through INDIRECT: no static edge, so it isn't held back
    // and isn't blocked either (the cell is no formula); on another worker it read the stored,
    // empty cell where one thread, after C1, read the array and let D100 spill, taking a cell
    // B101's array wanted. The slow fillers spread the level's chunks over the workers.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("A{r}"), "=SUMPRODUCT(SEQUENCE(20000))+ROW()".to_string())).collect();
        cells.push(("C1".to_string(), "=SEQUENCE(2)".to_string()));
        cells.push(("D100".to_string(), "=IF(INDIRECT(\"C2\")=2,SEQUENCE(2),0)".to_string()));
        cells.push(("B101".to_string(), "=SEQUENCE(1,3)".to_string()));
        t.load(cells);
        t
    };
    let one = build(1);
    assert_eq!(one.num("D101"), 2.0, "one thread: D100 spills");
    for run in 0..4 {
        let many = build(8);
        same_values(&one, &many, &format!("dynamic reader after a new array, run {run}"));
        assert!(many.calc.worker_levels > 0, "run {run}: the level ran on workers");
    }
}

#[test]
fn two_new_spills_overlapping_in_a_wide_level_resolve_as_on_one_thread() {
    // Two anchors whose new arrays would overlap (B1 down, A2 across), with readers of the cell
    // they contend for: whichever worker calculated which, the write-back resolves the collision
    // from the stored cells alone, as one thread does. (The write-back alone settles this one;
    // the test guards that threads never change which array wins.)
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            (1..=600)
                .map(|r| (format!("D{r}"), "=B2+ROW()".to_string()))
                .chain([("B1".to_string(), "=SEQUENCE(3)".to_string()), ("A2".to_string(), "=SEQUENCE(1,3)".to_string())]),
        );
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "overlapping new spills");
    assert!(many.calc.worker_levels > 0);
}

#[test]
fn a_read_under_two_arrays_checks_only_the_arrays_over_the_cell() {
    // B11:D13 and C10:E12 overlap in C11:D12, among 300 two-cell arrays of row 1. A reader of D12
    // reads C10's array (anchored first in row-major order, and kept by the write-back) after
    // checking the arrays the index has over D12, not every array of the pass.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (0..300).map(|col| (CellRef::new(0, col).a1(), "=SEQUENCE(2)".to_string())).collect();
        cells.push(("B11".to_string(), "=SEQUENCE(3,3)".to_string()));
        cells.push(("C10".to_string(), "=SEQUENCE(3,3)".to_string()));
        cells.extend((100..700).map(|r| (format!("A{r}"), "=D12+1".to_string())));
        crate::recalc::COVER_CHECKS.with(|n| n.set(0));
        t.load(cells);
        (t, crate::recalc::COVER_CHECKS.with(std::cell::Cell::get))
    };
    let (one, checks) = book(1);
    assert_eq!((one.num("D12"), one.num("A100"), one.get("B11")), (8.0, 9.0, Value::Error(CellError::Spill)));
    assert!(checks > 0 && checks <= 2 * 600, "{checks} arrays checked for 600 reads");
    let (many, _) = book(4);
    same_values(&one, &many, "reads under two arrays");
}

#[test]
fn a_result_read_after_an_array_laid_out_in_the_same_calculation_matches_one_thread() {
    // B1 reads C60 and then D30 through INDIRECT, so the workers hand it back. On one thread B1
    // lays C60's new array out and then calculates D30 on demand, which sees C61 and spills,
    // taking D31 from B31's array: B1 is 2 and B31 can't spill. The workers calculated D30 with
    // C61 still empty. B1 runs on the main thread at its turn, before C60's, and calculates C60
    // and then D30 on demand, as one thread does, so D30 sees the array; their worker results are
    // thrown away and nothing is calculated again.
    for (name, anchor) in [("plain", "=SEQUENCE(2)"), ("random", "=SEQUENCE(2)+0*RAND()")] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
            cells.push(("B1".to_string(), "=INDIRECT(\"C60\")+INDIRECT(\"D30\")".to_string()));
            cells.push(("D30".to_string(), "=IF(C61=2,SEQUENCE(2),0)".to_string()));
            cells.push(("B31".to_string(), "=SEQUENCE(1,3)".to_string()));
            cells.push(("C60".to_string(), anchor.to_string()));
            t.load(cells);
            t
        };
        let one = build(1);
        assert_eq!(
            (one.num("B1"), one.num("D30"), one.num("D31"), one.get("B31")),
            (2.0, 1.0, 2.0, Value::Error(CellError::Spill)),
            "{name}: one thread"
        );
        for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2), (8, 3)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name} array laid out inside the calculation that reads a result, {threads} threads, run {run}"));
            assert!(
                many.calc.worker_levels > 0 && many.calc.relaid_levels == 0,
                "{name} run {run}: the level ran on workers, nothing calculated again"
            );
        }
    }
}

#[test]
fn two_arrays_over_one_cell_read_as_the_write_back_keeps_them_on_any_thread_count() {
    // Two arrays of one pass can cover the same cell until the write-back blocks the one anchored
    // later in row-major order. A formula reading the cell meanwhile reads the one that will be
    // kept, whatever order the pass holds its arrays in (the threads change it): C16 reads B12
    // through OFFSET and D16 through INDIRECT, which B11's array and A12's both cover, and B11's
    // is kept; E16 read C12 from A12's array, which the write-back blocks, so the write-back
    // runs it again. Then a reduced random workbook that once differed by the choice (its
    // formulas form a static cycle, calculated after the levels). The write-back runs the
    // readers again whatever the pick, so the rounds correct the transient value in both: this
    // test holds the threads to one thread's result, and
    // `recalc::tests::two_arrays_over_one_cell_read_as_the_one_anchored_first` checks the pick.
    let overlap = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (1..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
        cells.push(("B11".to_string(), "=SEQUENCE(2)".to_string()));
        cells.push(("A12".to_string(), "=SEQUENCE(1,3)".to_string()));
        cells.push(("C16".to_string(), "=OFFSET(B11,1,0)+0".to_string()));
        cells.push(("D16".to_string(), "=INDIRECT(\"B12\")+0".to_string()));
        // C12 is A12's alone: read as 3 in the pass, empty once the write-back blocks A12.
        cells.push(("E16".to_string(), "=INDIRECT(\"C12\")+0".to_string()));
        t.load(cells);
        t
    };
    let reduced = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            [
                ("D7", "=IF(INDIRECT(\"D12\")=0,SEQUENCE(2),0)"),
                ("D10", "=SEQUENCE(1,MOD(E4,3)+1)+A14"),
                ("D12", "=IF(B14=1,SEQUENCE(1,2),0)"),
                ("C13", "=IF(INDIRECT(\"A12\")=0,SEQUENCE(2),0)"),
                ("A14", "=IF(D12>0,SEQUENCE(2),0)"),
                ("B14", "=SEQUENCE(1,MOD(A7,3)+1)+D10"),
                ("E14", "=SEQUENCE(1,MOD(C14,3)+1)+D7"),
            ]
            .map(|(a, f)| (a.to_string(), f.to_string())),
        );
        t.set("A7", "1");
        t
    };
    let (one, one_reduced) = (overlap(1), reduced(1));
    assert_eq!(
        (one.num("C16"), one.num("D16"), one.num("B12"), one.get("A12")),
        (2.0, 2.0, 2.0, Value::Error(CellError::Spill)),
        "one thread: B11's array is kept"
    );
    assert_eq!((one.num("E16"), one.get("C12")), (0.0, Value::Empty), "one thread: a reader of the blocked array's cell is run again");
    for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2)] {
        same_values(&one, &overlap(threads), &format!("two arrays over one cell, {threads} threads, run {run}"));
        same_values(&one_reduced, &reduced(threads), &format!("reduced random workbook, {threads} threads, run {run}"));
    }
}

#[test]
fn a_read_under_two_arrays_takes_the_first_anchored_when_the_pass_ends_at_its_round_limit() {
    // Two arrays of one pass cover a cell and the pass ends at its round limit or, with iterative
    // calculation on, at Maximum Iterations, without settling: the read of the array anchored
    // first in row-major order stays stored (this test takes the round limit). Before this
    // change the pick followed hash-map order. C55's array and B56's both cover C56 during the pass;
    // the write-back keeps C55's. Data!A56 reads C56 in a range (`read_range`), Data!A57 alone
    // (`read_cell`), and each keeps what C55's array held there.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
        let data = t.wb.sheet_mut(1).unwrap();
        data.cells.set(c("A56"), Cell::formula(Formula::new("=SUM(Sheet1!A56:C56)")));
        data.cells.set(c("A57"), Cell::formula(Formula::new("=Sheet1!C56")));
        t.load(
            [
                ("F4", "=IF(INDIRECT(\"F51\")=0,SEQUENCE(1,2),0)"),
                ("F17", "1"),
                ("D19", "=SUM(F2:F4)+F2"),
                ("C55", "=SEQUENCE(COUNT(A58:A59)+1)"),
                ("B56", "=SEQUENCE(1,MOD(F17,3)+1)+D19"),
                ("A58", "=COUNT(INDIRECT(\"D27:E30\"))"),
            ]
            .map(|(a, f)| (a.to_string(), f.to_string())),
        );
        t
    };
    let (one, threaded, row_major) = (book(1, None), book(4, None), book(1, Some(0)));
    for (t, what) in [(&one, "one thread"), (&threaded, "four threads"), (&row_major, "no plan")] {
        let data = |at: &str| t.wb.sheet(1).unwrap().value(c(at));
        assert_eq!((data("A56"), data("A57")), (Value::Number(4.0), Value::Number(2.0)), "{what}: the reads of C56");
        assert_eq!((t.get("B56"), t.num("C56")), (Value::Error(CellError::Spill), 2.0), "{what}: B56 is blocked, C56 is C55's");
    }
    same_values(&one, &threaded, "four threads");
    same_values(&one, &row_major, "no plan");
}

#[test]
fn random_spill_workbooks_calculate_the_same_on_any_thread_count() {
    // Small random workbooks of arrays whose shape depends on other cells, readers through
    // INDIRECT and OFFSET, random draws and plain fillers, loaded and then edited: each comes out
    // the same on one thread and on two or eight, in values, spills, circular references and the
    // number of formulas calculated by the edit. A failing case prints its cells;
    // `GRIDCRAFT_SPILL_FUZZ_CASES` soaks it over more.
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let cell = |n: u64| format!("{}{}", char::from(b'A' + (n % 6) as u8), 1 + (n / 6) % 12);
    let cases: u32 = std::env::var("GRIDCRAFT_SPILL_FUZZ_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(150);
    for case in 0..cases {
        let mut cells: Vec<(String, String)> = Vec::new();
        for _ in 0..4 + (next() % 9) as usize {
            let (at, a, b) = (cell(next()), cell(next()), cell(next()));
            let f = match next() % 9 {
                0 => format!("=IF({a}=0,SEQUENCE(2),0)"),
                1 => format!("=IF(INDIRECT(\"{a}\")=0,SEQUENCE(1,2),0)"),
                2 => format!("=SEQUENCE(1,MOD({a},3)+1)+{b}"),
                3 => format!("=OFFSET({a},1,0)+0"),
                4 => format!("=INDIRECT(\"{a}\")+{b}"),
                5 => format!("=SEQUENCE(MOD({a},2)+1)+0*RAND()"),
                6 => format!("={a}+ROW()"),
                7 => format!("=IF({a}=0,SEQUENCE(2),0)+0*RAND()"),
                _ => format!("=SEQUENCE(2)+{a}"),
            };
            cells.push((at, f));
        }
        cells.extend((1..=100).map(|r| (format!("K{r}"), "=ROW()".to_string())));
        let edit = (cell(next()), (next() % 3).to_string());
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            // Every other case one formula per chunk, so the few formulas that interact land on
            // different workers rather than in one worker's first chunk.
            if case % 2 == 1 {
                t.calc.chunk = Some(1);
            }
            t.load(cells.iter().cloned());
            t.set(&edit.0, &edit.1);
            t
        };
        let one = build(1);
        for threads in [2, 8] {
            let many = build(threads);
            same_values(&one, &many, &format!("case {case} ({:?}, then {edit:?}), {threads} threads", &cells[..cells.len() - 100]));
            assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "case {case}: formulas calculated by the edit, {threads} threads");
        }
    }
}

#[test]
fn a_result_read_on_demand_after_the_workers_matches_one_thread() {
    // J150 reads A201 through INDIRECT, a cell of A200's stored array, and A200 is in a later
    // level (through G1), so J150 reads the stored value, which the edit makes stale: A200 keeps
    // a single value now and its array goes. B5 reads J150 and then A200 through INDIRECT: one
    // thread calculates J150 on demand there, then A200. On eight threads a worker calculated
    // J150 at its own place in the level; B5, handed back, makes A200's array go after the
    // workers, and the formulas after B5 that could read it are calculated again at their turn,
    // as one thread would. The values and the formulas calculated are one thread's.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
        cells.push(("Z1".to_string(), "3".to_string()));
        cells.push(("G1".to_string(), "=Z1+0".to_string()));
        cells.push(("A200".to_string(), "=IF(Z1=1,5,SEQUENCE(3)+G1)".to_string()));
        cells.push(("J150".to_string(), "=INDIRECT(\"A201\")+0".to_string()));
        cells.push(("B5".to_string(), "=INDIRECT(\"J150\")+INDIRECT(\"A200\")".to_string()));
        // Dynamic too, after A200's place in the level.
        cells.push(("H300".to_string(), "=INDIRECT(\"Z1\")+0".to_string()));
        // Dynamic, before B5, read by nothing.
        cells.push(("C3".to_string(), "=INDIRECT(\"A201\")+0".to_string()));
        t.load(cells);
        assert_eq!((t.num("J150"), t.num("B5")), (5.0, 9.0), "{threads} threads: after load");
        // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
        t.calc.row_major_only = false;
        t.set("Z1", "1");
        t
    };
    let one = build(1);
    assert_eq!(one.get("A201"), Value::Empty, "one thread: A200's array went");
    for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2)] {
        let many = build(threads);
        same_values(&one, &many, &format!("result read on demand after the workers, {threads} threads, run {run}"));
        assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "{threads} threads, run {run}: the same formulas run again");
        assert!(many.calc.worker_levels > 0 && many.calc.handed_back >= 1, "{threads} threads, run {run}: B5 was handed back");
    }
}

#[test]
fn an_array_lost_on_the_main_thread_after_the_workers_runs_its_readers_again() {
    // B2's array goes when Z1 is 1, and B2 is random (or handed back), so it is calculated on
    // the main thread after the workers. D3 reads B3 through INDIRECT after B2 in row-major
    // order, a dynamic formula after an array that went: its worker result is thrown away and it
    // is calculated again at its turn, as a reader is after an array laid out anew. (One thread
    // reads B3's stored value there, 2, and keeps it after the write-back clears B3, as without a
    // plan: the dynamic formulas run again only after the first round that lays out an array.
    // That value is stale, upstream issue #4469; a fix of #4469 on the row-major path flips it
    // to 0.)
    for (name, anchor) in [("random", "=IF(Z1=1,5+0*RAND(),SEQUENCE(3)+0*RAND())"), ("handed back", "=IF(INDIRECT(\"H5\")=1,5,SEQUENCE(3))")] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
            cells.push(("Z1".to_string(), "3".to_string()));
            cells.push(("H5".to_string(), "=INDIRECT(\"Z1\")+0".to_string()));
            cells.push(("B2".to_string(), anchor.to_string()));
            cells.push(("D3".to_string(), "=INDIRECT(\"B3\")+0".to_string()));
            t.load(cells);
            assert_eq!(t.num("D3"), 2.0, "{name}, {threads} threads: after load");
            t.set("Z1", "1");
            t
        };
        let one = build(1);
        assert_eq!(
            (one.num("B2"), one.get("B3"), one.num("D3")),
            (5.0, Value::Empty, 2.0),
            "{name}: one thread, row-major order's stale value (#4469)"
        );
        for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2), (8, 3)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name} array lost after the workers, {threads} threads, run {run}"));
            assert!(
                many.calc.worker_levels > 0 && many.calc.relaid_levels == 1,
                "{name} run {run}: the level ran on workers and the formulas after the array were calculated again"
            );
        }
    }
}

#[test]
fn a_value_typed_into_a_spill_area_is_read_as_typed() {
    // Typing into a cell of a stored spill area blocks the array (its anchor shows #SPILL!); the
    // stored spill value under the typed cell goes at the write-back. Until then a reader of
    // the typed cell, through INDIRECT, OFFSET or a range, reads what was typed, after the
    // anchor is done (D11) or before it (D1), on one thread or many.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            [
                ("C6", "=SEQUENCE(2)"),
                ("D1", "=INDIRECT(\"C7\")+0"),
                ("D11", "=INDIRECT(\"C7\")+0"),
                ("E11", "=OFFSET(C6,1,0)+0"),
                ("F11", "=SUM(C7:C8)"),
            ]
            .map(|(a, f)| (a.to_string(), f.to_string())),
        );
        assert_eq!((t.num("D1"), t.num("D11"), t.num("E11"), t.num("F11")), (2.0, 2.0, 2.0, 2.0), "{threads} threads: after load");
        t.set("C7", "3");
        assert_eq!(t.get("C6"), Value::Error(CellError::Spill), "{threads} threads: the typed value blocks the array");
        assert_eq!(
            (t.num("D1"), t.num("D11"), t.num("E11"), t.num("F11")),
            (3.0, 3.0, 3.0, 3.0),
            "{threads} threads: every reader sees the typed value"
        );
    }
}

#[test]
fn cycles_reached_through_dynamic_readers_land_the_same_on_any_thread_count() {
    // Two workbooks from a random fuzzer, reduced: a cycle an OFFSET or INDIRECT reader closes
    // across levels, so one thread meets it on demand from an earlier level than the formula's
    // own. The cells and the circular list come out the same on any thread count: which member
    // of a cycle closed through INDIRECT shows the error, and which the list names, is what one
    // thread does, and the test holds the threads to it.
    for (name, cells) in [
        (
            "an OFFSET reader",
            vec![
                ("D9", "=3"),
                ("D10", "=COUNT(INDIRECT(\"D6:G1\"))"),
                ("E7", "=OFFSET(D9,1,0)+0"),
                ("G1", "=SUM(E11:E1)"),
                ("B12", "=SEQUENCE(2)+B8"),
            ],
        ),
        (
            "an INDIRECT self-reader",
            vec![
                ("G6", "=B6+ROW()"),
                ("C5", "=SEQUENCE(1,MOD(E11,3)+1)+G5"),
                ("G12", "=COUNT(INDIRECT(\"E3:A8\"))"),
                ("D11", "=COUNT(INDIRECT(\"G9:D12\"))"),
                ("A4", "=INDIRECT(\"D11\")+G6"),
                ("G5", "=SEQUENCE(1,MOD(D11,3)+1)+D1"),
            ],
        ),
    ] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            t.load(cells.iter().map(|(a, f)| (a.to_string(), f.to_string())));
            t
        };
        let one = build(1);
        assert!(!one.calc.circular.is_empty(), "{name}: one thread finds the cycle");
        for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name}, {threads} threads, run {run}"));
            assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "{name}, {threads} threads, run {run}: the same formulas run again");
            assert!(
                many.calc.worker_levels > 0 && many.calc.handed_back >= 1,
                "{name}, {threads} threads, run {run}: levels ran on workers and a reader was handed back"
            );
        }
    }
}

/// D3 reads G1:H2 through OFFSET, which the plan doesn't see, and H2 reads D3 (statically): a
/// cycle closed through OFFSET. The plan puts D3 in a level before H2, but in row-major order
/// H2 comes first and enters the cycle, so D3 shows `#CIRC!` and H2 counts nothing but its 1.
/// `more`: further cells. One thread, or four with levels of any size, one formula at a time.
fn cycle_through_offset(threads: u32, more: &[(&str, &str)]) -> T {
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
    t.calc.chunk = Some(1);
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
    full.calc.threads = t.calc.threads;
    full.calc.parallel_min = t.calc.parallel_min;
    full.calc.chunk = t.calc.chunk;
    full.calc.recalc_all(&mut full.wb);
    same_values(t, &full, &format!("{what}, against a full recalculation"));
}

#[test]
fn a_cycle_closed_through_offset_is_entered_as_in_row_major_order_after_edits_and_full_recalcs() {
    // The cycle through OFFSET with an INDIRECT reader of A1 in G1, an array above the cycle, and
    // a second INDIRECT reader over it: an edit of A1 that plans D3 ahead of H2 must still show
    // `#CIRC!` where a full recalculation does, not D3 = 2 * A1 + 1, on one thread and on four.
    let more = [("G1", "=INDIRECT(\"A1\")*2"), ("J1", "=A1+1"), ("K1", "=SEQUENCE(2)"), ("H5", "=COUNT(INDIRECT(\"A1:H4\"))+J1")];
    for threads in [1, 4] {
        let mut t = cycle_through_offset(threads, &more);
        entered_as_in_row_major_order(&t, &format!("{threads} threads, opened"));
        // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
        t.calc.row_major_only = false;
        t.set("A1", "2");
        entered_as_in_row_major_order(&t, &format!("{threads} threads, A1 edited"));
        assert!(t.calc.rounds_redone_in_row_major > 0, "{threads} threads: the edit met the cycle from a level");
        t.calc.recalc_all(&mut t.wb);
        entered_as_in_row_major_order(&t, &format!("{threads} threads, full recalculation"));
        t.set("A1", "3");
        entered_as_in_row_major_order(&t, &format!("{threads} threads, A1 edited again"));
        assert_eq!(t.num("G1"), 6.0, "{threads} threads");
    }
}

#[test]
fn a_cycle_closed_through_offset_over_a_plain_formula_is_entered_as_in_row_major_order() {
    // G1 reads A1 plainly: the plan put D3 ahead of H2 at every calculation, so the open, the
    // edits and a full recalculation all agreed on D3 = 2 * A1 + 1, where row-major order shows
    // `#CIRC!`.
    for threads in [1, 4] {
        let mut t = cycle_through_offset(threads, &[("G1", "=A1*2")]);
        entered_as_in_row_major_order(&t, &format!("{threads} threads, opened"));
        assert!(t.calc.rounds_redone_in_row_major > 0, "{threads} threads: the open met the cycle from a level");
        t.set("A1", "2");
        entered_as_in_row_major_order(&t, &format!("{threads} threads, A1 edited"));
        t.calc.recalc_all(&mut t.wb);
        entered_as_in_row_major_order(&t, &format!("{threads} threads, full recalculation"));
        t.set("A1", "3");
        entered_as_in_row_major_order(&t, &format!("{threads} threads, A1 edited again"));
    }
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
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(1);
        t.calc.plan_budget = plan_budget;
        t.load(cells.iter().map(|(a, f)| (a.to_string(), f.to_string())));
        t
    };
    for threads in [1, 4] {
        let (mut t, mut row_major) = (book(threads, None), book(1, Some(0)));
        same_values(&t, &row_major, &format!("{threads} threads, opened"));
        assert!(t.calc.rounds_redone_in_row_major > 0, "{threads} threads: the open met the cycle from a level");
        for value in ["2", "3"] {
            // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
            t.calc.row_major_only = false;
            t.set("A1", value);
            row_major.set("A1", value);
            same_values(&t, &row_major, &format!("{threads} threads, A1 = {value}"));
            assert!(t.calc.rounds_redone_in_row_major > 0, "{threads} threads, A1 = {value}: the edit met the cycle from a level");
        }
        assert_eq!(t.get("D3"), Value::Error(CellError::Circ), "{threads} threads");
    }
}

#[test]
fn a_round_whose_levels_meet_no_cycle_is_calculated_once() {
    // D3 reads through OFFSET without closing a cycle (H2 is a number), and Y1 and Y2 form a
    // plain cycle, which comes after the levels: no round is calculated again.
    for threads in [1, 4] {
        let mut t = cycle_through_offset(threads, &[("G1", "=A1*2"), ("Y1", "=Y2+1"), ("Y2", "=Y1+1")]);
        t.set("H2", "5");
        assert_eq!(t.num("D3"), 7.0, "{threads} threads");
        t.set("A1", "2");
        assert_eq!(t.num("D3"), 9.0, "{threads} threads");
        assert_eq!(t.get("Y1"), Value::Error(CellError::Circ), "{threads} threads");
        assert_eq!(t.calc.rounds_redone_in_row_major, 0, "{threads} threads: edit");
        t.calc.recalc_all(&mut t.wb);
        assert_eq!(t.get("Y1"), Value::Error(CellError::Circ), "{threads} threads");
        assert!(!t.calc.circular.is_empty(), "{threads} threads");
        assert_eq!(t.calc.rounds_redone_in_row_major, 0, "{threads} threads: full recalculation");
    }
}

#[test]
fn a_one_cell_array_that_goes_does_not_run_its_readers_again() {
    // A50 stores a one-cell array after the load and returns a plain number once Z1 is 1. The
    // array was the anchor's own value, which a reader gets from the formula wherever it is
    // calculated, so it going changes nothing a reader saw: no formula after it in the level is
    // calculated again, and no dynamic formula is run again after the round.
    for (name, anchor) in [("worker", "=IF(Z1=1,5,SEQUENCE(1)*2)"), ("main thread", "=IF(Z1=1,5,SEQUENCE(1)*2)+0*RAND()")] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()+Z1".to_string())).collect();
            cells.push(("Z1".to_string(), "0".to_string()));
            cells.push(("A50".to_string(), anchor.to_string()));
            cells.push(("B5".to_string(), "=INDIRECT(\"A50\")+Z1".to_string()));
            cells.push(("B70".to_string(), "=INDIRECT(\"A50\")+Z1".to_string()));
            // Dynamic too, after A50, but not reading it: a worker's result for it stands.
            cells.push(("B80".to_string(), "=INDIRECT(\"Z1\")+1".to_string()));
            t.load(cells);
            assert_eq!(t.wb.sheet(0).unwrap().spill_ranges.len(), 1, "{name}: A50 stores its one-cell array");
            t.set("Z1", "1");
            t
        };
        let one = build(1);
        assert_eq!((one.num("A50"), one.num("B5"), one.num("B70"), one.num("B80")), (5.0, 6.0, 6.0, 2.0), "{name}: one thread");
        assert_eq!(one.calc.last_recalc_cells, 102, "{name}: the 98 fillers, A50, B5, B70 and B80, once each");
        for (threads, run) in [(2, 0), (8, 0), (8, 1)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name} one-cell array gone, {threads} threads, run {run}"));
            assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "{name} run {run}: the same formulas calculated");
            assert_eq!((many.calc.relaid_levels, many.calc.level_formulas_redone), (0, 0), "{name} run {run}: nothing after A50 calculated again");
            assert!(many.calc.worker_levels > 0, "{name} run {run}: the level ran on workers");
        }
    }
}

#[test]
fn a_one_cell_array_laid_out_anew_does_not_run_the_level_again() {
    // A formula whose result is an array of one cell anchors a spill range (`A50#` is a range),
    // but the cell is the formula's own, so a reader gets the same value wherever the formula is
    // calculated: laid out anew by a worker, or on the main thread after the workers, it is no
    // reason to calculate the level again, which would put a sheet of such formulas on one
    // thread at every first calculation. The handed-back reader of the range gets it merged.
    for (name, anchor, count) in [("worker", "=SEQUENCE(1)*2", 107), ("main thread", "=SEQUENCE(1)*2+0*RAND()", 108)] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (2..=99).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
            cells.push(("A50".to_string(), anchor.to_string()));
            cells.push(("B5".to_string(), "=INDIRECT(\"A50\")+0".to_string()));
            cells.push(("B6".to_string(), "=SUM(INDIRECT(\"One\"))".to_string()));
            cells.push(("B7".to_string(), "=A50+1".to_string()));
            // Dynamic, after A50, not reading it: a worker's result for it stands.
            cells.push(("B80".to_string(), "=INDIRECT(\"F2\")+1".to_string()));
            t.name("One", "Sheet1!$A$50#");
            t.load(cells);
            t
        };
        let one = build(1);
        assert_eq!((one.num("B5"), one.num("B6"), one.num("B7")), (2.0, 2.0, 3.0), "{name}: one thread");
        for (threads, run) in [(2, 0), (8, 0), (8, 1), (8, 2)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{name} one-cell array, {threads} threads, run {run}"));
            assert!(
                many.calc.worker_levels > 0 && many.calc.handed_back >= 1,
                "{name} run {run}: the level ran on workers and the readers were handed back"
            );
            assert_eq!((many.calc.relaid_levels, many.calc.level_formulas_redone), (0, 0), "{name} run {run}: nothing after A50 calculated again");
            assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "{name} run {run}: the same formulas calculated");
        }
        // Each formula once; B7 again after the round, since it reads A50's cell, which is A50's
        // new spill range; and the dynamic formulas once more, as after any round that lays an
        // array out: B5, B6 and B80, and A50 itself when it draws a random number.
        assert_eq!(one.calc.last_recalc_cells, count, "{name}: 103 formulas, B7 again, and the dynamic ones again");
    }
}

#[test]
fn new_arrays_cost_one_worker_pass_and_only_their_readers_run_again() {
    // Each anchor lays out a new array (nothing is stored before the load), on a worker. The
    // readers in C read the arrays through INDIRECT, so after the first anchor every dynamic
    // formula of the level is calculated again on the main thread at its turn, while the 400
    // fillers keep their worker results: one worker pass per level however many arrays it lays
    // out, and only the readers are calculated twice. They run once more after the round, as
    // dynamic formulas do after a round that lays an array out, on the workers again: two levels
    // in all.
    for anchors in [3, 12] {
        let build = |threads: u32| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let mut cells: Vec<(String, String)> = (1..=400).map(|r| (format!("F{r}"), "=ROW()".to_string())).collect();
            for i in 0..anchors {
                cells.push((format!("A{}", 10 + 20 * i), "=SEQUENCE(2)".to_string()));
                cells.push((format!("C{}", 15 + 20 * i), format!("=INDIRECT(\"A{}\")+0", 11 + 20 * i)));
            }
            t.load(cells);
            t
        };
        let one = build(1);
        for i in 0..anchors {
            assert_eq!(one.num(&format!("C{}", 15 + 20 * i)), 2.0, "one thread: reader {i} sees the array");
        }
        for (threads, run) in [(2, 0), (8, 0), (8, 1)] {
            let many = build(threads);
            same_values(&one, &many, &format!("{anchors} anchors, {threads} threads, run {run}"));
            assert_eq!(many.calc.last_recalc_cells, one.calc.last_recalc_cells, "{anchors} anchors, run {run}: the same formulas calculated");
            assert_eq!(
                (many.calc.worker_levels, many.calc.relaid_levels, many.calc.level_formulas_redone, many.calc.level_formulas_on_main),
                (2, 1, anchors, anchors),
                "{anchors} anchors, {threads} threads, run {run}: levels, levels with readers run again, readers run again, formulas on the main thread"
            );
        }
    }
}

#[test]
fn a_recursive_lambda_on_a_worker_stops_at_the_same_call_as_on_one_thread() {
    // The recursions of the test above, 64 of them in one level calculated on worker threads: a
    // worker's stack holds the stack budget an evaluation may use (`STACK_BUDGET`), so each
    // stops at the same call there as on one thread (a stack smaller than that overflows).
    let tail = |n: u32| format!("=LET(F,LAMBDA(me,n,IF(n=0,0,me(me,n-1))),F(F,{n}))");
    let wrapped = |n: u32| format!("=LET(W,LAMBDA(f,k,f(f,k)),F,LAMBDA(me,n,IF(n<=0,0,1+W(me,n-1))),F(F,{n}))");
    let kinds = [tail(5459), tail(5460), wrapped(69), wrapped(70), "=CNT(131)".to_string()];
    let want = [Value::Number(0.0), Value::Error(CellError::Num), Value::Number(69.0), Value::Number(70.0), Value::Number(131.0)];
    let book = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.name("CNT", "=LAMBDA(n,IF(n=0,0,1+CNT(n-1)))");
        t.load((1..=64u32).map(|r| (format!("A{r}"), kinds[(r as usize - 1) % kinds.len()].clone())));
        t
    };
    let (one, many) = (book(1), book(8));
    same_values(&one, &many, "recursion on workers");
    assert!(many.calc.worker_levels > 0 && many.calc.handed_back == 0, "the recursions ran on workers");
    for r in 1..=64u32 {
        assert_eq!(many.get(&format!("A{r}")), want[(r as usize - 1) % want.len()], "A{r}");
    }
}

#[test]
fn a_python_cell_read_on_a_worker_before_it_is_laid_out_is_read_after_it() {
    // B1 shows 1, its kept result is 7. The readers reach it through INDIRECT, so the plan can't
    // put them after it: on a worker, a reader that meets it before the main thread has laid it
    // out is handed back rather than reading the 1 the cell still shows.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load((1..=600).map(|r| (format!("A{r}"), "=INDIRECT(\"B1\")+0".to_string())));
        plant_python(&mut t, "B1", Value::Number(1.0), Value::Number(7.0));
        t.calc.recalc_all(&mut t.wb);
        assert_eq!(t.num("B1"), 7.0, "{threads} threads");
        for r in 1..=600 {
            assert_eq!(t.num(&format!("A{r}")), 7.0, "A{r}, {threads} threads");
        }
        if threads > 1 {
            assert!(t.calc.worker_levels > 0, "the readers ran on workers");
        }
    }
}

#[test]
fn a_python_array_laid_out_anew_is_read_by_the_formulas_after_it_in_its_level() {
    // As `a_spill_landing_in_a_block_a_worker_has_read_ends_as_on_one_thread`, with a Python cell
    // spilling: A1 and A3 read B2:CW2, A2 is a Python cell with no spill stored whose kept result
    // is a row of 200. The three share a level; one worker reads the block before A2 is laid out,
    // so A3 is calculated again on the main thread at its turn, after A2's new array.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load([("A1", "=SUM(B2:CW2)"), ("A3", "=SUM(B2:CW2)")].map(|(a, f)| (a.to_string(), f.to_string())));
        let row =
            Value::Array(std::sync::Arc::new(gridcraft_core::Array::new(1, 200, (1..=200).map(|n| Value::Number(f64::from(n))).collect()).unwrap()));
        plant_python(&mut t, "A2", Value::Number(1.0), row);
        t.calc.recalc_all(&mut t.wb);
        t
    };
    let (one, two) = (book(1), book(2));
    same_values(&one, &two, "a Python array laid out anew");
    let want: f64 = (2..=101).map(f64::from).sum();
    assert_eq!((two.num("A1"), two.num("A3")), (want, want));
    assert_eq!(two.calc.last_recalc_cells, one.calc.last_recalc_cells);
    assert_eq!(two.calc.relaid_levels, 1, "A3 calculated again after A2's array");
}

#[test]
fn precision_as_displayed_rounds_results_calculated_on_workers() {
    // "Set precision as displayed" rounds each result to its cell's format. A worker rounds as the
    // main thread does: 600 formulas of one level, each formatted `0.00`, give the same rounded
    // values on one thread and on eight.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load((1..=600).map(|r| (format!("A{r}"), format!("={r}/3"))));
        let id = t.wb.styles.intern(gridcraft_model::Style { num_fmt: gridcraft_model::NumFmt::new("0.00"), ..Default::default() });
        for r in 1..=600 {
            t.wb.sheet_mut(0).unwrap().set_style(c(&format!("A{r}")), id);
        }
        t.wb.calc.precision_as_displayed = true;
        t.calc.recalc_all(&mut t.wb);
        for r in 1..=600 {
            let rounded = (f64::from(r) / 3.0 * 100.0).round() / 100.0;
            assert_eq!(t.num(&format!("A{r}")), rounded, "A{r}, {threads} threads");
        }
        if threads > 1 {
            assert!(t.calc.worker_levels > 0, "the formulas ran on workers");
        }
    }
}

#[test]
fn the_workers_of_a_level_keep_no_more_parsed_names_than_the_pass_may() {
    // A pass keeps at most 1 MiB of parsed definition text (`MAX_PASS_NAME_TEXT`); past that, a
    // name is parsed again at each use. 250 names of about 4 KB each come to just under it, and 64
    // formulas of one level read all of them: the workers of the level share that budget rather
    // than each keeping a mebibyte of its own.
    let ones = vec!["1"; 2000].join("+");
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        for i in 0..250 {
            t.wb.names_mut().push(gridcraft_model::DefinedName {
                name: format!("Addend_{i}"),
                scope: None,
                formula: ones.clone(),
                comment: String::new(),
                hidden: false,
            });
        }
        let sum = format!("=SUM({})", (0..250).map(|i| format!("Addend_{i}")).collect::<Vec<_>>().join(","));
        t.load((1..=64).map(|r| (format!("A{r}"), sum.clone())));
        for r in 1..=64 {
            assert_eq!(t.num(&format!("A{r}")), 500_000.0, "A{r}, {threads} threads");
        }
        assert!(t.calc.peak_names_text <= 1 << 20, "{threads} threads kept {} bytes at once", t.calc.peak_names_text);
        if threads > 1 {
            assert!(t.calc.worker_levels > 0, "the formulas ran on workers");
            assert!(t.calc.peak_names_text > 0, "the workers kept names");
        }
    }
}

#[test]
fn the_workers_of_a_level_parse_each_name_once_between_them() {
    // 38 names of about 8 KB each, 304 KB in all, more than a quarter of the pass's 1 MiB, read
    // by each of 64 formulas of one level on four workers: each name is parsed once in the
    // level, as on one thread. With 140 such names, 1.1 MB, the parsed text the workers keep
    // stays within the pass's 1 MiB, and the names past it are parsed at each use.
    let mut body = String::from("IF(TRUE,1,SUM(1");
    for i in 1..3990 {
        body.push_str(if i % 250 == 0 { ")+SUM(1" } else { ",1" });
    }
    body.push_str("))");
    assert!(body.len() > 7_900 && body.len() < 8_192, "{}", body.len());
    let book = |threads, count: usize| {
        let names: Vec<String> = (0..count).map(|i| format!("Addend_{i}")).collect();
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(1);
        for name in &names {
            t.name(name, &body);
        }
        let sum = format!("={}+$Z$1", names.join("+"));
        crate::recalc::NAMES_PARSED.with(|n| n.set(0));
        t.load((1..=64).map(|r| (format!("A{r}"), sum.clone())));
        (t, crate::recalc::NAMES_PARSED.with(std::cell::Cell::get))
    };
    let (one, parsed) = book(1, 38);
    assert_eq!((one.num("A1"), parsed), (38.0, 38));
    let (many, parsed) = book(4, 38);
    assert!(many.calc.worker_levels > 0, "the formulas ran on workers");
    assert_eq!(parsed, 38, "each name parsed once between the workers");
    assert!(many.calc.peak_names_text >= 38 * 7_900, "{}", many.calc.peak_names_text);
    same_values(&one, &many, "names read on workers");
    let (one, _) = book(1, 140);
    let (many, parsed) = book(4, 140);
    assert_eq!(one.num("A64"), 140.0);
    assert!(many.calc.peak_names_text <= 1 << 20 && many.calc.peak_names_text > (1 << 20) - 8_192, "{}", many.calc.peak_names_text);
    assert!(parsed > 140, "{parsed}: the names past the budget are parsed at each use");
    same_values(&one, &many, "more names than the pass keeps, read on workers");
}

#[test]
fn a_chain_of_arrays_read_by_counts_below_settles_in_row_major_order() {
    // The sideways chain, plus ten formulas below it counting everything rows 1 to 16 spill
    // into: the planned round has the links read before they are laid out, so it is calculated
    // again in row-major order, which settles the chain and the counts at once.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells = sideways_chain("=SEQUENCE(1,5)*Z116");
        for row in 20..=29u32 {
            cells.push((format!("V{row}"), "=COUNT($B$1:$T$16)+ROW()".to_string()));
        }
        t.load(cells);
        assert_eq!(sideways_widths(&t), vec![5; 16], "{threads} threads");
        assert_eq!((t.num("A16"), t.num("E16")), (1.0, 5.0), "{threads} threads");
        for row in 20..=29u32 {
            assert_eq!(t.num(&format!("V{row}")), 64.0 + f64::from(row), "V{row}, {threads} threads");
        }
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{threads} threads");
    }
}

#[test]
fn a_chain_of_a_hundred_arrays_settles_in_one_row_major_round() {
    // The sideways chain with 100 links and nothing else: planned backwards, so the round is
    // calculated again in row-major order, which lays the links out in order.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(sideways_chain_of(100, "=SEQUENCE(1,5)*Z200"));
        assert_eq!(sideways_widths_of(&t, 100), vec![5; 100], "{threads} threads");
        assert_eq!((t.num("A100"), t.num("E100")), (1.0, 5.0), "{threads} threads");
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{threads} threads");
    }
}

#[test]
fn arrays_that_never_settle_through_indirect_end_as_in_row_major_order() {
    // Three formulas in column A, each one cell wide when the two cells right of the one before it
    // (A30 for A10) hold a number and spilling three wide otherwise, in a loop: whatever one does,
    // the next undoes, so each round dirties a few of them again and the arrangement never
    // settles. A10 reads A30's cells through INDIRECT, a loop the planner can't see. Z5 puts A10
    // in a later level than A20, which reads B10:C10; 5,000 more formulas in column AZ make the
    // first round large. The pass takes at most 8 rounds, each ending as in row-major order.
    let cells = |row_major: bool, threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
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
    for threads in [1, 8] {
        let t = cells(false, threads);
        let rounds = t.calc.rounds_calculated;
        assert!(rounds <= 8, "{rounds} rounds on {threads} threads");
        assert!(t.calc.rounds_redone_in_row_major > 0, "{threads} threads");
        same_values(&t, &cells(true, threads), &format!("{threads} threads"));
    }
}

#[test]
fn planning_charges_the_wide_readers_a_spill_query_passes_over() {
    // 600 anchors in column A each store a two-cell array in A:B, and 600 readers in CZ read
    // D1:BP600, rows every array's query meets, columns none of them reaches. Each query looks at
    // every reader before the columns turn it down: 360,000 looks in all, which the budget is
    // charged for, so a sheet of many more of both gives planning up rather than squaring.
    let mut t = T::new().threads(1);
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
    let mut t = T::new().threads(1);
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

/// A chain from K1 through M1:Q1 and S1 (the dirty columns M to Q and S); S5 sums S1:Z1, which
/// starts at the last dirty column, and S6 sums A1:M1, which ends at the first; `readers` wide
/// readers from S10 read Q1 and 400 areas beside the dirty columns, none meeting one: narrow
/// (three columns) and wide (ten or more) on each side. With the planner's index work capped at
/// `plan_budget`.
fn areas_beside_dirty_columns(readers: u32, plan_budget: Option<usize>) -> (T, Vec<Key>) {
    let mut t = T::new().threads(1);
    t.calc.plan_budget = plan_budget;
    let mut cells: Vec<(String, String)> = vec![("K1".to_string(), "1".to_string()), ("M1".to_string(), "=K1+1".to_string())];
    cells.extend(["N1", "O1", "P1", "Q1"].iter().zip(["M1", "N1", "O1", "P1"]).map(|(at, from)| (at.to_string(), format!("={from}+1"))));
    cells.push(("S1".to_string(), "=Q1*2".to_string()));
    cells.push(("S5".to_string(), "=SUM(S1:Z1)".to_string()));
    cells.push(("S6".to_string(), "=SUM(A1:M1)".to_string()));
    let beside: String = (1..=100).map(|r| format!("+SUM(A{r}:C{r})+SUM(A{r}:J{r})+SUM(T{r}:V{r})+SUM(T{r}:AF{r})")).collect();
    cells.extend((10..10 + readers).map(|r| (format!("S{r}"), format!("=Q1{beside}"))));
    let mut dirty: Vec<Key> = cells.iter().filter(|(_, input)| input.starts_with('=')).map(|(at, _)| (0, c(at))).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    t.load(cells);
    (t, dirty)
}

#[test]
fn areas_beside_the_dirty_columns_are_charged_as_walked_and_meeting_ones_wait() {
    // The readers' areas meet no dirty column: planning visits a formula's areas once, for the
    // count, and charges the release lists and the anchors above what that visit cost, area by
    // area where the budget may run out, so the plan costs the same work and is given up at the
    // same points of a smaller budget as with a walk over the areas for each. An area
    // reaching the first or the last dirty column still makes its reader wait.
    let (t, dirty) = areas_beside_dirty_columns(20, None);
    let plan = t.calc.graph.levels(&t.wb, &dirty).unwrap();
    let level_of = |at: &str| plan.levels().position(|level| level.contains(&(0, c(at)))).unwrap();
    assert!(level_of("S5") > level_of("S1"), "S5 reads S1");
    assert!(level_of("S6") > level_of("M1"), "S6 reads M1");
    assert!(level_of("S10") > level_of("Q1"), "S10 reads Q1");
    let given_up_at: Vec<usize> = [100, 1_000, 10_000, plan.work() / 2, plan.work() - 1]
        .iter()
        .map(|&cap| match t.calc.graph.levels_within(&t.wb, &dirty, Some(cap)) {
            Err(NoPlan::Budget { work }) => work,
            other => panic!("a cap of {cap}: {:?}", other.map(|p| p.work())),
        })
        .collect();
    // The work and the points recorded before the planner dismissed such areas once.
    assert_eq!(plan.work(), 60_222);
    assert_eq!(given_up_at, [101, 1_001, 10_002, 30_114, 60_222]);
    assert!(t.calc.graph.levels_within(&t.wb, &dirty, Some(plan.work())).is_ok(), "at the budget");
    // Through the calculation's budget knob: planned at that work, row-major order one unit short.
    let (row_major, _) = areas_beside_dirty_columns(20, Some(0));
    let (at, _) = areas_beside_dirty_columns(20, Some(plan.work()));
    assert_eq!(at.calc.plans_made, 1, "planned at the budget");
    assert!(at.calc.redo_reasons.is_empty() && !at.calc.row_major_only, "{:?}", at.calc.redo_reasons);
    let (past, _) = areas_beside_dirty_columns(20, Some(plan.work() - 1));
    assert_eq!(past.calc.plans_made, 0, "given up one unit short");
    assert!(past.calc.row_major_only);
    same_values(&at, &row_major, "planned, against row-major order");
    same_values(&past, &row_major, "past the budget, against row-major order");
    assert_eq!(at.num("S6"), 3.0);
    assert_eq!(at.num("S5"), 12.0);
}

/// Chains in C1:C40 and D1:D40, and in column E a formula at E1 and E40 only (the dirty columns C
/// to E), read from D60:D64 through several areas in column C, areas reaching past the first and
/// the last dirty column and beside them, a whole column, a row of one cell's height, an area
/// under E1 with empty cells between them, and a wide reader of more than `EXACT` formulas.
fn areas_across_dirty_columns() -> (T, Vec<Key>) {
    let mut t = T::new().threads(1);
    let mut cells = vec![("A1".to_string(), "1".to_string()), ("C1".to_string(), "=$A$1+1".to_string())];
    cells.extend((2..=40).map(|r| (format!("C{r}"), format!("=C{}+1", r - 1))));
    cells.extend((1..=40).map(|r| (format!("D{r}"), format!("=C{r}*2"))));
    cells.push(("E1".to_string(), "=D1+1".to_string()));
    cells.push(("E40".to_string(), "=D40+1".to_string()));
    cells.extend(
        [
            ("D60", "=SUM(C1:C3)+SUM(C5:C7)+SUM(C9:C80)"),
            ("D61", "=SUM(B2:D4)+SUM(E3:H5)+SUM(A1:B5)+SUM(F1:H9)"),
            ("D62", "=SUM(C:C)+SUM(C4:E4)"),
            ("D63", "=SUM(E5:E6)+1"),
            ("D64", "=SUM(C1:E40)+SUM(C1:C40)"),
        ]
        .map(|(at, input)| (at.to_string(), input.to_string())),
    );
    let mut dirty: Vec<Key> = cells.iter().filter(|(_, input)| input.starts_with('=')).map(|(at, _)| (0, c(at))).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    t.load(cells);
    (t, dirty)
}

#[test]
fn areas_across_the_dirty_columns_give_the_levels_and_work_recorded() {
    // The levels, the index work and every point planning gives up at under a smaller budget, as
    // recorded before the planner visited each area once.
    let (t, dirty) = areas_across_dirty_columns();
    let plan = t.calc.graph.levels(&t.wb, &dirty).unwrap();
    let levels: Vec<Vec<String>> = plan.levels().map(|l| l.iter().map(|k| k.1.a1()).collect()).collect();
    let given_up_at: std::collections::BTreeSet<usize> = (1..plan.work())
        .map(|cap| match t.calc.graph.levels_within(&t.wb, &dirty, Some(cap)) {
            Err(NoPlan::Budget { work }) => work,
            other => panic!("a cap of {cap}: {:?}", other.map(|p| p.work())),
        })
        .collect();
    assert_eq!(plan.work(), 929);
    assert!(plan.rest().is_empty());
    let mut expected: Vec<Vec<String>> = vec![vec!["C1".into()], vec!["D1".into(), "C2".into()], vec!["E1".into(), "D2".into(), "C3".into()]];
    expected.push(vec!["D3".into(), "C4".into(), "D63".into()]);
    expected.push(vec!["D4".into(), "C5".into()]);
    expected.push(vec!["D5".into(), "C6".into(), "D61".into()]);
    expected.extend((6..=39).map(|r| vec![format!("D{r}"), format!("C{}", r + 1)]));
    expected.push(vec!["D40".into(), "D60".into(), "D62".into()]);
    expected.push(vec!["E40".into()]);
    expected.push(vec!["D64".into()]);
    assert_eq!(levels, expected);
    // Every point (537 of them, summing to 263,995), and those of the readers' walks in full.
    assert_eq!((given_up_at.len(), given_up_at.iter().sum::<usize>()), (537, 263_995));
    let readers: Vec<usize> = given_up_at.range(575..753).copied().collect();
    let recorded = [
        575, 577, 579, 617, 619, 621, 623, 625, 627, 629, 633, 635, 638, 642, 648, 652, 654, 657, 661, 665, 667, 670, 674, 676, 680, 722, 724, 728,
        730, 734, 736, 738, 740, 744, 746, 750, 752,
    ];
    assert_eq!(readers, recorded);
}

#[test]
fn spill_readers_are_planned_alike_from_sorted_and_unsorted_dirty_formulas() {
    // A1 spills over A1:J10; K1:K10 read its cells in column E, which holds no formula, so only
    // the readers of A1's stored spill range put them after A1 (no dirty formula above their
    // areas does); L1:L10 read the K beside them, and M1 the whole range. Planned from the dirty
    // formulas in row-major order (placed by a search) and in reverse (placed through a map): the
    // same levels, each K and M1 after A1, and the same work.
    let mut t = T::new().threads(1);
    t.set("Z1", "1");
    let mut cells = vec![("A1".to_string(), "=SEQUENCE(10,10,Z1)".to_string()), ("M1".to_string(), "=SUM(A1#)".to_string())];
    cells.extend((1..=10).map(|r| (format!("K{r}"), format!("=E{r}+Z1"))));
    cells.extend((1..=10).map(|r| (format!("L{r}"), format!("=K{r}*2"))));
    let mut sorted: Vec<Key> = cells.iter().map(|(at, _)| (0, c(at))).collect();
    sorted.sort_by_key(|(s, at)| (*s, at.row, at.col));
    t.load(cells);
    let reversed: Vec<Key> = sorted.iter().rev().copied().collect();
    fn level_sets<'p>(levels: impl Iterator<Item = &'p [Key]>) -> Vec<Vec<Key>> {
        levels
            .map(|l| {
                let mut l = l.to_vec();
                l.sort_by_key(|(s, at)| (*s, at.row, at.col));
                l
            })
            .collect()
    }
    let in_order = t.calc.graph.levels(&t.wb, &sorted).unwrap();
    let in_reverse = t.calc.graph.levels(&t.wb, &reversed).unwrap();
    assert_eq!(level_sets(in_order.levels()), level_sets(in_reverse.levels()));
    assert_eq!(in_order.work(), in_reverse.work());
    assert!(in_order.rest().is_empty() && in_reverse.rest().is_empty());
    let levels = level_sets(in_order.levels());
    let level_of = |at: &str| levels.iter().position(|l| l.contains(&(0, c(at)))).unwrap();
    for r in 1..=10 {
        assert!(level_of(&format!("K{r}")) > level_of("A1"), "K{r} after A1");
        assert!(level_of(&format!("L{r}")) > level_of(&format!("K{r}")), "L{r} after K{r}");
    }
    assert!(level_of("M1") > level_of("A1"), "M1 after A1");
    assert_eq!((t.num("E3"), t.num("K3")), (25.0, 26.0));
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
/// With the planner's index work capped at `plan_budget`, on four threads with planning as
/// shipped, whatever `GRIDCRAFT_CALC_THREADS` says and however many processors there are, and
/// `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
fn whole_rows_book(readers: u32, plan_budget: Option<usize>) -> (T, Vec<Key>) {
    let mut t = T::new().threads(4).planning_as_shipped();
    t.calc.parallel_min = 512;
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
    // million units for one pass over them, and planning charges three (the count, the release
    // lists, the anchors above). The budget is checked after each area, so planning gives up
    // at most one area's walk (the area and 4,001 columns) past it, not after all of them.
    let cap = 1_000_000;
    let (t, dirty) = whole_rows_book(4000, None);
    let planned = t.calc.graph.levels_within(&t.wb, &dirty, Some(cap));
    let Err(NoPlan::Budget { work }) = planned else { panic!("planning given up: {:?}", planned.map(|p| p.work())) };
    assert!(cap < work && work <= cap + 4_002, "{work} index work against a budget of {cap}");
}

#[test]
fn planning_keeps_no_more_spans_than_its_limit_whatever_the_areas_meet() {
    // A2's 900 whole rows meet each of the sheet's 4,001 dirty columns: planning holds the spans
    // of the first 65 (`KEPT_SPANS`, 64, and one that marks them not kept) and visits the areas
    // again, rather than keep one for each column of each row, a store that grew with the work
    // the budget allows.
    let spans_peak = |step: &dyn Fn()| {
        crate::recalc::PLAN_SPANS_PEAK.with(|p| p.set(0));
        step();
        crate::recalc::PLAN_SPANS_PEAK.with(std::cell::Cell::get)
    };
    let (t, dirty) = whole_rows_book(4000, None);
    let peak = spans_peak(&|| assert!(matches!(t.calc.graph.levels(&t.wb, &dirty), Err(NoPlan::Budget { .. })), "planning given up"));
    assert_eq!(peak, 65, "spans held for the columns A2's rows meet");
    // Past the limit, the areas visited again give the release lists and the anchors above: A5
    // reads the 100 formulas of row 1, too many for a release list, and D5 reads row 3, which
    // holds no formula, and B1, whose release list it is on and which would spill into row 3.
    let mut t = T::new();
    let mut cells =
        vec![("A3".to_string(), "1".to_string()), ("A5".to_string(), "=SUM(1:1)".to_string()), ("D5".to_string(), "=SUM(3:3)+B1".to_string())];
    cells.extend((1..=100).map(|col| (CellRef::new(0, col).a1(), "=$A$3+1".to_string())));
    let mut dirty: Vec<Key> = cells.iter().filter(|(_, input)| input.starts_with('=')).map(|(at, _)| (0, c(at))).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    t.load(cells);
    let peak = spans_peak(&|| {
        let plan = t.calc.graph.levels(&t.wb, &dirty).unwrap();
        let levels: Vec<Vec<String>> = plan.levels().map(|l| l.iter().map(|k| k.1.a1()).collect()).collect();
        assert_eq!(levels.len(), 2, "{levels:?}");
        assert_eq!(levels.get(1), Some(&vec!["A5".to_string(), "D5".to_string()]), "after the row they read");
    });
    assert_eq!(peak, 65, "spans held for 101 columns met");
    assert_eq!((t.num("A5"), t.num("D5")), (200.0, 3.0));
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
fn a_walk_of_the_cell_index_is_charged_by_its_capacity_after_removals() {
    // Ten thousand formulas that each read one cell of column A, all but one then cleared in one
    // batch: the per-cell index holds one entry, but its table keeps the room for ten thousand,
    // which a look-up over more than 4,096 cells walks, so the charge for it counts that room.
    let wide = gridcraft_core::RangeRef::new(c("C1"), c("C5000"));
    let mut small = T::new();
    small.load([("B1".to_string(), "=A1".to_string())]);
    assert!(small.calc.graph.uncharged_probes(wide) < 100, "{}", small.calc.graph.uncharged_probes(wide));
    let mut t = T::new();
    t.load((1..=10_000).map(|r| (format!("B{r}"), format!("=A{r}"))));
    let cleared: Vec<Key> = (2..=10_000).map(|r| (0, c(&format!("B{r}")))).collect();
    for (_, at) in &cleared {
        t.wb.sheet_mut(0).unwrap().cells.remove(*at);
    }
    t.calc.cells_changed(&mut t.wb, &cleared);
    let mut readers = Vec::new();
    t.calc.graph.dependents_of_range(0, gridcraft_core::RangeRef::new(c("A1"), c("A10000")), &mut readers);
    assert_eq!(readers, vec![(0, c("B1"))], "one reader of column A left");
    let charged = t.calc.graph.uncharged_probes(wide);
    assert!(charged >= 10_000, "{charged} probes charged for a walk of a table sized for 10,000 entries");
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
    // A formula entered: its own pass plans again (one formula, well within the budget). A round
    // that small is planned only when a level of one formula may go to worker threads
    // (`parallel_min` of 1, whatever `GRIDCRAFT_PARALLEL_MIN` says).
    t.calc.parallel_min = 1;
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
    // that lays out an array, as without a plan, so the planned run keeps 2 too, on one thread as
    // on many. That is the value row-major order gives, and it is stale (upstream issue #4469:
    // Excel shows 0); a fix of #4469 on the row-major path flips the last assertion to 0.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(
            [("D1", "1"), ("A2", "=$D$1"), ("A1", "=IF(A2,SEQUENCE(1,2),0)"), ("C1", "=INDIRECT(\"B1\")+0")]
                .map(|(a, b)| (a.to_string(), b.to_string())),
        );
        assert_eq!((t.num("B1"), t.num("C1")), (2.0, 2.0), "{threads} threads: after load");
        t.set("D1", "0");
        t
    };
    let one = build(1);
    assert_eq!((one.num("A1"), one.get("B1"), one.num("C1")), (0.0, Value::Empty, 2.0), "one thread: row-major order's stale value (#4469)");
    for threads in [2, 8] {
        same_values(&one, &build(threads), &format!("{threads} threads"));
    }
}

#[test]
fn getpivotdata_readers_on_four_threads_match_one_thread() {
    // A report of three value cells in A1:B4 with its lookup index, and 1,200 readers whose item
    // comes from a formula of the same pass, directly or through INDIRECT (a dynamic reader, its
    // item read where another worker may not have calculated it yet). Every level runs on workers.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let items = ["north", "south", "west"];
        let sheet = t.wb.sheet_mut(0).unwrap();
        sheet.cells.set(c("A1"), Cell::value(Value::Text("Key".into())));
        sheet.cells.set(c("B1"), Cell::value(Value::Text("Sum of Sales".into())));
        for (i, item) in items.iter().enumerate() {
            sheet.cells.set(c(&format!("A{}", i + 2)), Cell::value(Value::Text((*item).into())));
            sheet.cells.set(c(&format!("B{}", i + 2)), Cell::value(Value::number((i as f64 + 1.0) * 100.0)));
        }
        let pivot = gridcraft_model::PivotTable {
            rows: vec![gridcraft_model::PivotField { source_col: "Key".into(), ..Default::default() }],
            last_range: Some(gridcraft_core::RangeRef::parse("A1:B4").unwrap()),
            ..Default::default()
        };
        pivot.data_index.set_cells(
            items
                .iter()
                .enumerate()
                .map(|(i, item)| gridcraft_model::PivotDataCell {
                    data_field: "Sum of Sales".into(),
                    source_col: "Sales".into(),
                    items: vec![gridcraft_model::PivotItemRef { field: "Key".into(), label: (*item).into(), value: Value::Text((*item).into()) }],
                    row: i as u32 + 1,
                    col: 1,
                })
                .collect(),
        );
        sheet.pivots.push(pivot);
        t.load((1..=1200).flat_map(|r| {
            [
                (format!("C{r}"), r#"=INDEX({"north","south","west"},MOD(ROW(),3)+1)"#.to_string()),
                (format!("D{r}"), format!(r#"=GETPIVOTDATA("Sales",$A$1,"Key",C{r})*ROW()"#)),
                (format!("E{r}"), r#"=GETPIVOTDATA("Sales",$A$1,"Key",INDIRECT("C"&(1201-ROW())))"#.to_string()),
                (format!("F{r}"), format!("=D{r}+E{r}")),
            ]
        }));
        t
    };
    let (mut one, mut many) = (book(1), book(4));
    same_values(&one, &many, "after load");
    assert!(many.calc.worker_levels > 0, "no level ran on workers");
    // Row 3: MOD(3,3)+1 picks north (100); E3 reads C1198, MOD(1198,3)+1 picks south (200).
    assert_eq!((one.num("D3"), one.num("E3"), one.num("F3")), (300.0, 200.0, 500.0));
    // An edit of a report value cell recalculates the readers through the report.
    for t in [&mut one, &mut many] {
        t.set("B2", "1000");
    }
    same_values(&one, &many, "after B2 = 1000");
    assert_eq!(one.num("D3"), 3000.0);
}

#[test]
fn a_block_of_constants_is_summed_at_most_once_per_worker_however_many_levels_read_it() {
    // Six levels of 600 formulas, each summing A1:A1000 (constants) plus the cell to its left.
    // The first level's workers each sum the block once at most; the levels after it read the
    // sums merged into the main thread's. A worker that started each level without them would
    // sum the block again in every level, at least six times in all.
    let book = |threads| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 512;
        let columns = ["C", "D", "E", "F", "G", "H"];
        let mut cells: Vec<(String, String)> = (1..=1000).map(|r| (format!("A{r}"), "1".to_string())).collect();
        for (i, col) in columns.iter().enumerate() {
            for r in 1..=600 {
                let left = i.checked_sub(1).and_then(|p| columns.get(p)).map(|p| format!("+{p}{r}")).unwrap_or_default();
                cells.push((format!("{col}{r}"), format!("=SUM($A$1:$A$1000){left}")));
            }
        }
        t.load(cells);
        t
    };
    let (one, four) = (book(1), book(4));
    same_values(&one, &four, "six levels summing one block");
    assert_eq!(four.num("H600"), 6000.0);
    assert_eq!(four.calc.worker_levels, 6);
    assert_eq!(one.calc.block_walks, 1, "one thread");
    assert!(four.calc.block_walks <= 4, "four threads summed the block {} times over six levels", four.calc.block_walks);
}

#[test]
fn summed_blocks_that_change_during_a_pass_read_the_same_on_four_threads_as_on_one() {
    // B1:B990 hold constants, and A996 spills two cells from the second level, over B996: the
    // first level's sums of B1:B1000 (column D) were worked out without it, and column E sums
    // the block again after it. G1:G990 hold constants but for G5, a formula of the second
    // level, and column F sums them after E. Every level of 600 runs on workers with four
    // threads. An edit of B1 changes the block, the spill and every sum; a full recalculation
    // of the edited workbook gives the same.
    let book = |threads, b1: &str| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 512;
        let mut cells = vec![
            ("B1".to_string(), b1.to_string()),
            ("C1".to_string(), "=$B$1*10".to_string()),
            ("A996".to_string(), "=SEQUENCE(1,2,C1)".to_string()),
            ("G5".to_string(), "=D1*0+7".to_string()),
        ];
        cells.extend((2..=990).map(|r| (format!("B{r}"), "1".to_string())));
        cells.extend((1..=990).filter(|r| *r != 5).map(|r| (format!("G{r}"), "2".to_string())));
        for r in 1..=600 {
            cells.push((format!("D{r}"), "=SUM($B$1:$B$1000)+ROW()".to_string()));
            cells.push((format!("E{r}"), format!("=SUM($B$1:$B$1000)+D{r}+$A$996*0")));
            cells.push((format!("F{r}"), format!("=SUM($G$1:$G$1000)+E{r}")));
        }
        t.load(cells);
        t
    };
    let (mut one, mut four) = (book(1, "1"), book(4, "1"));
    same_values(&one, &four, "after load");
    assert!(four.calc.worker_levels > 0, "no level ran on workers");
    // B1:B1000 = 1 + 989 + B996 (C1 + 1 = 11).
    assert_eq!((four.num("B996"), four.num("D1"), four.num("E1")), (11.0, 1002.0, 2003.0));
    assert_eq!(four.num("F600"), 989.0 * 2.0 + 7.0 + 1001.0 * 2.0 + 600.0);
    for t in [&mut one, &mut four] {
        t.set("B1", "5");
    }
    same_values(&one, &four, "after B1 = 5");
    // 5 + 989 + 51.
    assert_eq!((four.num("B996"), four.num("D1"), four.num("E1")), (51.0, 1046.0, 2091.0));
    let full = book(4, "5");
    same_values(&four, &full, "the edit against a full recalculation");
}

// ---- Passes without a settled result end as in row-major order --------------------------------

/// `cells` loaded (a `Data!` prefix puts a cell on a second sheet), with the pass planned on
/// `threads` threads, or calculated in row-major order without a plan (`row_major`), as the base
/// calculation runs every pass.
fn loop_book(cells: &[(&str, &str)], threads: u32, row_major: bool) -> T {
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
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

/// Loads `cells`, makes each edit of `edits`, then recalculates everything twice, planned on one
/// thread and on four, and in row-major order: the values, spills and circular references are the
/// same at every step. Returns the planned workbook on one thread after loading.
fn ends_as_in_row_major(what: &str, cells: &[(&str, &str)], edits: &[(&str, &str)]) -> T {
    let mut loaded = None;
    for threads in [1, 4] {
        let mut planned = loop_book(cells, threads, false);
        let mut rows = loop_book(cells, threads, true);
        same_values(&planned, &rows, &format!("{what}, {threads} threads, after loading"));
        if loaded.is_none() {
            loaded = Some(T { wb: planned.wb.clone(), calc: planned.calc.clone() });
        }
        for (at, input) in edits {
            for t in [&mut planned, &mut rows] {
                let key = put(t, at, input);
                t.calc.cells_changed(&mut t.wb, &[key]);
            }
            same_values(&planned, &rows, &format!("{what}, {threads} threads, after {at} = {input}"));
        }
        for time in 1..=2 {
            for t in [&mut planned, &mut rows] {
                t.calc.recalc_all(&mut t.wb);
            }
            same_values(&planned, &rows, &format!("{what}, {threads} threads, full recalculation {time}"));
        }
    }
    loaded.unwrap()
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
        let planned = loop_book(&refs, 1, false);
        let rows = loop_book(&refs, 1, true);
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
    for threads in [1, 4] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(sideways_chain("=SEQUENCE(1,5)*Z116"));
        assert_eq!(sideways_widths(&t), vec![5; 16], "{threads} threads");
        t.set("Z100", "2");
        assert_eq!(sideways_widths(&t), vec![5; 16], "{threads} threads, after an edit");
        assert_eq!((t.num("A16"), t.num("E16")), (2.0, 10.0), "{threads} threads, after an edit");
    }
}

// ---- Dynamic formulas after an array goes ---------------------------------------------------

#[test]
fn a_formula_reading_its_own_spill_through_indirect_ends_as_in_row_major_order() {
    // A1 reads B1, a cell of its own spill, through INDIRECT. The first round lays out 1 and 2;
    // the dynamic formulas run once more after it, A1 reads its own 2 and gives 0, and the array
    // goes: planned or not, on any thread count, through full recalculations, A1 entered again and
    // an unrelated edit. That is the value row-major order gives, and it is stale (upstream issue
    // #4470: Excel shows its circular-reference warning, then 1 and 2 in A1:B1, and keeps them); a
    // fix of #4470 on the row-major path flips these assertions to 1 and 2.
    let cells = [("A1", "=IF(INDIRECT(\"B1\")=0,SEQUENCE(1,2),0)")];
    let formula = cells[0].1;
    let t = ends_as_in_row_major("own spill through INDIRECT", &cells, &[("A1", formula), ("A1", formula), ("Y1", "1")]);
    assert_eq!((t.num("A1"), t.get("B1")), (0.0, Value::Empty), "after loading: row-major order's stale value (#4470)");
    for row_major in [false, true] {
        for threads in [1, 4] {
            let mut t = loop_book(&cells, threads, row_major);
            for time in 1..=3 {
                t.calc.cells_changed(&mut t.wb, &[(0, c("A1"))]);
                assert_eq!((t.num("A1"), t.get("B1")), (0.0, Value::Empty), "row-major {row_major}, {threads} threads, A1 touched {time} times");
            }
            t.calc.recalc_all(&mut t.wb);
            assert_eq!((t.num("A1"), t.get("B1")), (0.0, Value::Empty), "row-major {row_major}, {threads} threads, full recalculation");
        }
    }
}

#[test]
fn a_dynamic_reader_of_an_array_that_goes_keeps_the_stale_cells_planned_or_not() {
    // Z1 = 0 turns A1's three-row array into one value. C5 reads A2 and A3 through INDIRECT and
    // keeps 5, planned or not, on any thread count: the dynamic formulas run again only after the
    // first round that lays out an array. That is the value row-major order gives, and it is stale
    // (upstream issue #4469: Excel shows 0 at once); a fix of #4469 on the row-major path flips it
    // to 0.
    let cells = [("Z1", "1"), ("A1", "=IF(Z1=1,SEQUENCE(3),0)"), ("C5", "=N(INDIRECT(\"A2\"))+N(INDIRECT(\"A3\"))")];
    for row_major in [false, true] {
        for threads in [1, 4] {
            let mut t = loop_book(&cells, threads, row_major);
            assert_eq!(t.num("C5"), 5.0, "row-major {row_major}, {threads} threads, after loading");
            let key = put(&mut t, "Z1", "0");
            t.calc.cells_changed(&mut t.wb, &[key]);
            assert_eq!(t.num("C5"), 5.0, "row-major {row_major}, {threads} threads, after Z1 = 0: row-major order's stale value (#4469)");
        }
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

/// The shut-out pair and the blocked loop, with `filler` unrelated formulas in column AA, loaded
/// on `threads` threads at the usual threshold for worker threads, planned or in row-major order.
fn loops_beside_filler(filler: u32, threads: u32, row_major: bool) -> T {
    let mut t = T::new().threads(threads);
    // `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
    t.calc.parallel_min = 512;
    if row_major {
        t.calc.plan_budget = Some(0);
    }
    let mut cells: Vec<(String, String)> = [
        ("Z9", "=1"),
        ("G1", "=IF(COUNT(H5:I5)>0,1,SEQUENCE(1,3))+Z9*0"),
        ("G5", "=IF(COUNT(H1:I1)>0,1,SEQUENCE(1,3))"),
        ("A1", "=SEQUENCE(MOD(C3,3)+1)"),
        ("A2", "x"),
        ("B3", "=SEQUENCE(1,MOD(B5,3)+1)"),
        ("B5", "=A1+1"),
    ]
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .to_vec();
    cells.extend((1..=filler).map(|r| (format!("AA{r}"), "=ROW()+Z9".to_string())));
    t.load(cells);
    t
}

#[test]
fn filler_formulas_change_no_value_of_the_loops_beside_them() {
    // 0 to 600 unrelated formulas, on one thread and four: past 512 the level they share with
    // the loops' formulas runs on worker threads. The loops end the same every time, as in
    // row-major order.
    let looked = ["A1", "A2", "A3", "B3", "C3", "D3", "B5", "G1", "H1", "I1", "G5", "H5", "I5"];
    let mut ends = Vec::new();
    for filler in [0u32, 4, 64, 511, 600] {
        for threads in [1, 4] {
            let planned = loops_beside_filler(filler, threads, false);
            same_values(&planned, &loops_beside_filler(filler, threads, true), &format!("{filler} filler formulas, {threads} threads"));
            if filler == 600 && threads == 4 {
                assert!(planned.calc.worker_levels > 0, "600 filler formulas run on worker threads");
            }
            ends.push(looked.map(|at| planned.get(at)));
        }
    }
    assert!(ends.windows(2).all(|w| w[0] == w[1]), "{ends:?}");
}

/// A 600-row array read through its spill operator by 600 formulas, a formula reading a cell
/// through INDIRECT and one reading a name defined nowhere (both dynamic), on four threads at
/// the usual threshold.
fn arrays_beside_dynamic_formulas() -> T {
    let mut t = T::new().threads(4);
    // `PARALLEL_MIN`'s default, whatever `GRIDCRAFT_PARALLEL_MIN` says.
    t.calc.parallel_min = 512;
    let mut cells: Vec<(String, String)> =
        [("Y1", "1"), ("Z1", "5"), ("A1", "=SEQUENCE(600)*Y1"), ("D1", "=INDIRECT(\"Z1\")+1"), ("E1", "=IFERROR(missing_rate,0)+1")]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .to_vec();
    cells.extend((1..=600).map(|r| (format!("B{r}"), "=SUM(A1#)+ROW()".to_string())));
    t.load(cells);
    t
}

#[test]
fn dynamic_formulas_that_read_no_changed_array_keep_the_plan_and_threads() {
    // On loading the array is new, and the dynamic formulas are in every round, but they read
    // nothing it covers, and its readers read it through its anchor: the planned rounds stand.
    let mut t = arrays_beside_dynamic_formulas();
    assert_eq!(t.calc.rounds_redone_in_row_major, 0, "after loading: {:?}", t.calc.redo_reasons);
    assert!(t.calc.worker_levels > 0, "after loading, the readers run on worker threads");
    assert_eq!((t.num("B1"), t.num("D1"), t.num("E1")), (180_300.0 + 1.0, 6.0, 1.0));
    t.set("Y1", "2");
    assert_eq!(t.calc.rounds_redone_in_row_major, 0, "after Y1 = 2: {:?}", t.calc.redo_reasons);
    assert!(t.calc.worker_levels > 0, "after Y1 = 2, the readers run on worker threads");
    assert_eq!(t.num("B600"), 360_600.0 + 600.0);
    t.set("Z1", "7");
    assert_eq!((t.calc.rounds_redone_in_row_major, t.num("D1")), (0, 8.0), "after Z1 = 7");
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

/// `cells` (a `Data!` prefix puts a cell on a second sheet) loaded on `threads` threads, every
/// level on worker threads past one, with the planner's index work capped at `plan_budget`
/// (`Some(0)`: every round in row-major order).
fn capped_book(cells: &[(&str, String)], threads: u32, plan_budget: Option<usize>) -> T {
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
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
    // array: with the reads not noted, the round is calculated again in row-major order. On four
    // threads D1 is noted on a worker, the only one its level needs, within the whole budget.
    let cells = noted_reads_cells();
    for threads in [1, 4] {
        let mut free = capped_book(&cells, threads, None);
        edit(&mut free, &[("Y1", "2")]);
        let noted = match free.calc.watched.as_slice() {
            [(noted, false)] => *noted,
            other => panic!("{threads} threads: one planned round, its reads noted: {other:?}"),
        };
        assert!(noted >= 41, "{threads} threads: {noted} reads noted: forty in column B and A2, none joined");

        let mut at_limit = capped_book(&cells, threads, None);
        at_limit.calc.note_cap = Some(noted);
        edit(&mut at_limit, &[("Y1", "2")]);
        assert_eq!(at_limit.calc.watched, vec![(noted, false)], "{threads} threads: at the limit, every read is noted");
        assert_eq!(at_limit.calc.rounds_redone_in_row_major, 0, "{threads} threads: at the limit");

        let mut past = capped_book(&cells, threads, None);
        past.calc.note_cap = Some(noted - 1);
        let mut row_major = capped_book(&cells, threads, Some(0));
        edit(&mut past, &[("Y1", "2")]);
        edit(&mut row_major, &[("Y1", "2")]);
        assert_eq!(past.calc.watched, vec![(0, true)], "{threads} threads: one past the limit, the reads are let go");
        assert_eq!(past.calc.rounds_redone_in_row_major, 0, "{threads} threads: no array changed, the planned round stands");
        same_values(&past, &row_major, &format!("{threads} threads, after Y1 = 2"));

        edit(&mut past, &[("Z1", "3")]);
        edit(&mut row_major, &[("Z1", "3")]);
        assert_eq!(past.calc.watched.first(), Some(&(0, true)), "{threads} threads: after Z1 = 3, the reads are let go");
        assert_eq!(past.calc.redo_reasons, vec![Redo::Budget], "{threads} threads: A1's array changed, the round runs in row-major order");
        same_values(&past, &row_major, &format!("{threads} threads, after Z1 = 3"));
        assert_eq!((past.num("A2"), past.num("D1")), (2.0, 2.0), "{threads} threads");
    }
}

/// A1 holds 3; `array_at` lays out an array of A1's size; D1:D200 read through INDIRECT the cell
/// of column E beside them, or (`chain`) F one row up, a formula of the same level, which a worker
/// may not have done yet; G1:G5 draw random numbers. On `threads` threads, every level of eight
/// formulas or more on workers, ten at a time (one for the chain, so that the readers are spread
/// over the workers and handed back), with the reads a round notes capped at `cap`.
fn noting_book(threads: u32, array_at: &str, chain: bool, cap: Option<usize>) -> T {
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 8;
    t.calc.chunk = Some(if chain { 1 } else { 10 });
    let mut cells = vec![("A1".to_string(), "3".to_string()), (array_at.to_string(), "=SEQUENCE($A$1)".to_string())];
    for r in 1..=200u32 {
        cells.push((format!("E{r}"), r.to_string()));
        if chain {
            cells.push((format!("D{r}"), "=$A$1+INDIRECT(\"F\"&MAX(1,ROW()-1))".to_string()));
            cells.push((format!("F{r}"), "=$A$1*2".to_string()));
        } else {
            cells.push((format!("D{r}"), "=INDIRECT(\"E\"&ROW())+$A$1".to_string()));
        }
    }
    cells.extend((1..=5).map(|r| (format!("G{r}"), "=RAND()+$A$1".to_string())));
    t.load(cells);
    t.calc.note_cap = cap;
    t
}

#[test]
fn the_reads_a_round_notes_and_whether_they_fit_are_one_threads_on_any_thread_count() {
    // A1 = 5 grows the array, so a round whose reads aren't all noted is calculated again in
    // row-major order. With the cap at what one thread notes in the edit's first round, the round
    // is kept; one below, it is calculated again. One, two and four threads note the same reads
    // and keep or redo the same rounds, in every run, with one thread's values and draws: with the
    // array after the readers, before them (their worker results are thrown away and they are
    // calculated again on the main thread), and with a chain of readers the workers hand back.
    let layouts = [("array after the readers", "B300", false), ("array before the readers", "B1", false), ("a chain handed back", "B300", true)];
    for (layout, array_at, chain) in layouts {
        let mut free = noting_book(1, array_at, chain, None);
        edit(&mut free, &[("A1", "5")]);
        let total = match free.calc.watched.first() {
            Some(&(noted, false)) if noted > 0 => noted,
            other => panic!("{layout}: the first round's reads noted: {other:?}"),
        };
        for (cap, redone) in [(total, vec![]), (total - 1, vec![Redo::Budget])] {
            let mut one = noting_book(1, array_at, chain, Some(cap));
            edit(&mut one, &[("A1", "5")]);
            assert_eq!(one.calc.redo_reasons, redone, "{layout}, cap {cap} of {total}, one thread");
            same_values(&one, &free, &format!("{layout}, cap {cap} of {total}, one thread"));
            let (mut workers, mut handed) = (0, 0);
            for threads in [1, 2, 4] {
                for run in 1..=20 {
                    let mut t = noting_book(threads, array_at, chain, Some(cap));
                    edit(&mut t, &[("A1", "5")]);
                    let what = format!("{layout}, cap {cap} of {total}, {threads} threads, run {run}");
                    assert_eq!(t.calc.watched, one.calc.watched, "{what}: the reads noted, round by round");
                    assert_eq!(t.calc.redo_reasons, one.calc.redo_reasons, "{what}");
                    same_values(&t, &one, &what);
                    workers += t.calc.worker_levels;
                    handed += t.calc.handed_back;
                }
            }
            assert!(workers > 0, "{layout}, cap {cap}: the readers ran on workers");
            assert!(!chain || handed > 0, "{layout}, cap {cap}: the workers handed readers back");
        }
    }
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
    let mut sorted: Vec<Key> = owned
        .iter()
        .filter(|(_, input)| input.starts_with('='))
        .map(|(at, _)| at.split_once('!').map_or_else(|| (0, c(at)), |(_, at)| (1, c(at))))
        .collect();
    sorted.sort_by_key(|(s, c)| (*s, c.row, c.col));
    for threads in [1, 4] {
        let free = capped_book(&cells, threads, None);
        let row_major = capped_book(&cells, threads, Some(0));
        same_values(&free, &row_major, &format!("{threads} threads, within the budget"));
        assert_eq!(free.calc.rounds_redone_in_row_major, 0, "{threads} threads, within the budget: {:?}", free.calc.redo_reasons);
        assert!(free.calc.check_work >= 80 * 300, "{threads} threads: each read pays for the columns it walks: {}", free.calc.check_work);

        let planning = free.calc.graph.levels(&free.wb, &sorted).map(|p| p.work()).unwrap_or(usize::MAX);
        let capped = capped_book(&cells, threads, Some(planning + 1_000));
        same_values(&capped, &row_major, &format!("{threads} threads, with the walk past the budget"));
        assert_eq!(capped.calc.redo_reasons, vec![Redo::Budget], "{threads} threads, planning {planning}: the walk runs out of budget");
    }
}

/// Rows of [`new_arrays_beside_wide_readers`].
const WIDE_READER_ROWS: u32 = 600;

/// A1:A600 each lay out a new two-cell array (A:B) with `array`, and CZ1:CZ600 each read
/// D1:BP600, wide, all rows, beside them: every look-up of an array's readers visits the 600 wide
/// readers and finds none. Z1 holds 1. One level, on `threads` threads, every level on workers,
/// with the planner's index work (and so the round's budget) capped at `plan_budget`.
fn new_arrays_beside_wide_readers(array: &str, threads: u32, plan_budget: Option<usize>) -> T {
    let n = WIDE_READER_ROWS;
    let cells: Vec<(String, String)> = (1..=n)
        .flat_map(|r| [(format!("A{r}"), array.to_string()), (format!("CZ{r}"), format!("=COLUMNS(D1:BP{n})"))])
        .chain([("Z1".to_string(), "1".to_string())])
        .collect();
    let cells: Vec<(&str, String)> = cells.iter().map(|(at, input)| (at.as_str(), input.clone())).collect();
    capped_book(&cells, threads, plan_budget)
}

#[test]
fn looking_up_the_readers_of_new_arrays_is_charged_to_the_rounds_budget() {
    // Checking the round looks up the readers of each new array: 600 look-ups over the 600 wide
    // readers, about 360,000 entries visited and none found. Within the round's budget the round
    // stands; with planning paid for but not the look-ups, it is calculated again in row-major
    // order, on one thread as on two and four. On worker threads, the level looks up the same
    // readers as it merges the arrays, on a count of its own: past it, the formulas after the
    // array that passed it are calculated on the main thread at their turn. The values are
    // row-major order's every time.
    let n = WIDE_READER_ROWS as usize;
    let row_major = new_arrays_beside_wide_readers("={1,2}", 1, Some(0));
    assert_eq!((row_major.num("B600"), row_major.num("CZ600")), (2.0, 65.0));
    for threads in [1, 2, 4] {
        let free = new_arrays_beside_wide_readers("={1,2}", threads, None);
        same_values(&free, &row_major, &format!("{threads} threads, within the budget"));
        assert_eq!(free.calc.rounds_redone_in_row_major, 0, "{threads} threads, within the budget: {:?}", free.calc.redo_reasons);
        assert!(free.calc.check_work >= n * n, "{threads} threads: each look-up pays for what it visits: {}", free.calc.check_work);
        assert_eq!(free.calc.marking_given_up, 0, "{threads} threads, within the budget");
        assert_eq!(free.calc.worker_levels > 0, threads > 1, "{threads} threads");

        let capped = new_arrays_beside_wide_readers("={1,2}", threads, Some(50_000));
        same_values(&capped, &row_major, &format!("{threads} threads, past the budget"));
        assert_eq!(capped.calc.plans_made, 1, "{threads} threads: planning fits the budget");
        assert_eq!(capped.calc.redo_reasons, vec![Redo::Budget], "{threads} threads: the look-ups run out of budget");
        // One thread makes no look-ups as it calculates; on workers, the formulas after the array
        // whose look-up passed the count are calculated on the main thread.
        assert_eq!(capped.calc.marking_given_up, usize::from(threads > 1), "{threads} threads");
        assert_eq!(
            capped.calc.level_formulas_on_main > 0,
            threads > 1,
            "{threads} threads: {} on the main thread",
            capped.calc.level_formulas_on_main
        );
    }
}

/// A1:A600 each lay out a new two-cell array (A:B), and CZ1:CZ600 each sum D9000:Z100000 through
/// INDIRECT, a block below and beside the arrays that Z100000 (holding 1) brings into the used
/// range. Every level on workers, with the planner's index work capped at `plan_budget`.
fn new_arrays_beside_far_reads(threads: u32, plan_budget: Option<usize>) -> T {
    let n = WIDE_READER_ROWS;
    let cells: Vec<(String, String)> = (1..=n)
        .flat_map(|r| [(format!("A{r}"), "={1,2}".to_string()), (format!("CZ{r}"), "=SUM(INDIRECT(\"D9000:Z100000\"))".to_string())])
        .chain([("Z100000".to_string(), "1".to_string())])
        .collect();
    let cells: Vec<(&str, String)> = cells.iter().map(|(at, input)| (at.as_str(), input.clone())).collect();
    capped_book(&cells, threads, plan_budget)
}

#[test]
fn a_noted_read_pays_for_every_array_its_look_up_inspects() {
    // Each of the 600 noted reads meets none of the 600 new arrays, yet its look-up of the arrays
    // it meets inspects every one of them (the block is wider than the arrays are many): about
    // 360,000 units for the check, where a unit per look-up and per array met would be 600. Within
    // the round's budget the round stands; with planning paid for but not those look-ups, it is
    // calculated again in row-major order. The values are row-major order's every time.
    let n = WIDE_READER_ROWS as usize;
    let row_major = new_arrays_beside_far_reads(1, Some(0));
    assert_eq!((row_major.num("B600"), row_major.num("CZ600")), (2.0, 1.0));
    for threads in [1, 2, 4] {
        let free = new_arrays_beside_far_reads(threads, None);
        same_values(&free, &row_major, &format!("{threads} threads, within the budget"));
        assert_eq!(free.calc.rounds_redone_in_row_major, 0, "{threads} threads, within the budget: {:?}", free.calc.redo_reasons);
        assert!(!free.calc.watched.is_empty() && free.calc.watched.iter().all(|w| *w == (n, false)), "{threads} threads: {:?}", free.calc.watched);
        assert!(free.calc.check_work >= n * n, "{threads} threads: each look-up pays for what it inspects: {}", free.calc.check_work);

        let capped = new_arrays_beside_far_reads(threads, Some(50_000));
        same_values(&capped, &row_major, &format!("{threads} threads, past the budget"));
        assert_eq!(capped.calc.plans_made, 1, "{threads} threads: planning fits the budget");
        assert_eq!(capped.calc.redo_reasons, vec![Redo::Budget], "{threads} threads: the look-ups run out of budget");
    }
}

#[test]
fn a_level_whose_look_ups_run_out_calculates_the_rest_on_the_main_thread_and_keeps_the_round() {
    // The arrays read Z1 through OFFSET, so after the first one every other is calculated again on
    // the main thread at its turn: the level looks up the readers of the worker's array it throws
    // away and of the one laid out, twice what checking the round looks up. With the round's
    // budget at one and a half times what the check costs, the check fits and the round stands,
    // while the level's look-ups run out and the formulas after that are calculated on the main
    // thread: on one thread, two and four, with row-major order's values.
    let array = "=OFFSET($Z$1,0,0)*{1,2}";
    let n = WIDE_READER_ROWS as usize;
    let row_major = new_arrays_beside_wide_readers(array, 1, Some(0));
    assert_eq!((row_major.num("B600"), row_major.num("CZ600")), (2.0, 65.0));
    let check = new_arrays_beside_wide_readers(array, 1, None).calc.check_work;
    assert!(check >= n * n, "the check pays for what its look-ups visit: {check}");
    let cap = check + check / 2;
    for threads in [1, 2, 4] {
        let t = new_arrays_beside_wide_readers(array, threads, Some(cap));
        same_values(&t, &row_major, &format!("{threads} threads"));
        assert!(t.calc.plans_made > 0, "{threads} threads: planned within {cap}");
        assert!(t.calc.redo_reasons.is_empty(), "{threads} threads: the check fits: {:?}", t.calc.redo_reasons);
        assert_eq!(t.calc.marking_given_up, usize::from(threads > 1), "{threads} threads: the level's look-ups run out");
    }
}

/// The units of work range look-ups did on this thread while `step` ran (see
/// `RANGE_LOOK_UP_WORK`).
fn looked_up_in(step: impl FnOnce()) -> usize {
    crate::recalc::RANGE_LOOK_UP_WORK.with(|w| w.set(0));
    step();
    crate::recalc::RANGE_LOOK_UP_WORK.with(std::cell::Cell::get)
}

/// Z1 = 1 makes A1 (`=SEQUENCE(1,Z1)`) one number; the 600 formulas of column D read B1. An edit
/// of Z1 to 2 lays out a new array over A1:B1 in a round of A1 alone. On `threads` threads, every
/// level on workers, with the planner's index work capped at `plan_budget`.
fn a_new_array_read_by_many(threads: u32, plan_budget: Option<usize>) -> T {
    let cells: Vec<(String, String)> = [("Z1".to_string(), "1".to_string()), ("A1".to_string(), "=SEQUENCE(1,Z1)".to_string())]
        .into_iter()
        .chain((1..=600).map(|r| (format!("D{r}"), "=B1+0".to_string())))
        .collect();
    let cells: Vec<(&str, String)> = cells.iter().map(|(at, input)| (at.as_str(), input.clone())).collect();
    capped_book(&cells, threads, plan_budget)
}

#[test]
fn a_check_whose_look_up_runs_out_stops_inside_it_and_calculates_the_round_again() {
    // Checking the round of the edit looks up B1's readers once: 600 of them, none in the round.
    // With the round's budget at 50, planning A1 fits and the look-up runs out after about fifty:
    // it stops there, and the round is calculated again in row-major order, the check having
    // charged no more than the budget. On worker threads, the level's own look-up of A1:B1 runs
    // out as well. On one thread, two and four, with row-major order's values.
    let cap = 50;
    for threads in [1, 2, 4] {
        let mut row_major = a_new_array_read_by_many(threads, Some(0));
        let mut t = a_new_array_read_by_many(threads, None);
        same_values(&t, &row_major, &format!("{threads} threads, after loading"));
        assert!(!t.calc.row_major_only, "{threads} threads, after loading: {:?}", t.calc.redo_reasons);
        t.calc.plan_budget = Some(cap);
        let planned = looked_up_in(|| edit(&mut t, &[("Z1", "2")]));
        let unplanned = looked_up_in(|| edit(&mut row_major, &[("Z1", "2")]));
        same_values(&t, &row_major, &format!("{threads} threads, after Z1 = 2"));
        assert_eq!((t.num("B1"), t.num("D600")), (2.0, 2.0), "{threads} threads");
        assert_eq!(t.calc.plans_made, 1, "{threads} threads: planning A1 fits the budget");
        assert_eq!(t.calc.redo_reasons, vec![Redo::Budget], "{threads} threads: the look-up runs out");
        // Write-back's own look-ups of B1's 600 readers aren't budgeted: the row-major run of the
        // same edit makes them too, so what the planned run looks up past that is the check's and,
        // on workers, the level's, each at most its budget and the one reader whose unit it refused.
        let budgeted = if threads > 1 { 2 } else { 1 };
        assert!(
            planned <= unplanned + budgeted * (cap + 1),
            "{threads} threads: the look-ups stopped inside: {planned} units against {unplanned} in row-major order"
        );
        assert_eq!(t.calc.marking_given_up, usize::from(threads > 1), "{threads} threads");
    }
}

/// A1 and A2 lay out two-cell arrays (A:B) that read Z1 through OFFSET; the 200 formulas of
/// column D read B1 and the 200 of column E read B2. After loading, the passes plan (as after a
/// formula edit). On `threads` threads, every level on workers, with the planner's index work
/// capped at `plan_budget`.
fn two_arrays_read_by_many(threads: u32, plan_budget: Option<usize>) -> T {
    let mut cells: Vec<(String, String)> = vec![("Z1".to_string(), "1".to_string())];
    for row in 1..=2 {
        cells.push((format!("A{row}"), "=OFFSET($Z$1,0,0)*{1,2}".to_string()));
    }
    for r in 1..=200 {
        cells.push((format!("D{r}"), "=B1+0".to_string()));
        cells.push((format!("E{r}"), "=B2+0".to_string()));
    }
    let cells: Vec<(&str, String)> = cells.iter().map(|(at, input)| (at.as_str(), input.clone())).collect();
    let mut t = capped_book(&cells, threads, plan_budget);
    t.calc.row_major_only = false;
    t
}

#[test]
fn reader_marking_that_runs_out_inside_a_look_up_calculates_the_rest_on_the_main_thread() {
    // Z1 = 2 recalculates A1 and A2, one level, nothing else. Checking the round looks up the 200
    // readers of B1 and those of B2. On workers, A2 reads OFFSET, so once A1's new array is laid
    // out A2 is calculated again on the main thread: the level looks up the readers of A1's
    // array, of the worker's A2 it throws away and of the one laid out, one and a half times
    // what the check looks up. With the round's budget at one and a quarter times what the check
    // costs, the check fits and the round stands, while the level's last look-up runs out inside
    // and takes the fallback (the rest of the level on the main thread, here none): on two
    // threads and four, with row-major order's values and the round kept as on one thread.
    let check = {
        let mut t = two_arrays_read_by_many(1, None);
        edit(&mut t, &[("Z1", "2")]);
        t.calc.check_work
    };
    assert!(check >= 400, "the check pays for the readers it lists: {check}");
    let cap = check + check / 4;
    let mut row_major = two_arrays_read_by_many(1, Some(0));
    edit(&mut row_major, &[("Z1", "2")]);
    assert_eq!((row_major.num("B2"), row_major.num("D200"), row_major.num("E200")), (4.0, 4.0, 4.0));
    let mut one = two_arrays_read_by_many(1, None);
    one.calc.plan_budget = Some(cap);
    let one_work = looked_up_in(|| edit(&mut one, &[("Z1", "2")]));
    same_values(&one, &row_major, "one thread");
    assert!(one.calc.plans_made > 0, "one thread: planned within {cap}");
    assert!(one.calc.redo_reasons.is_empty(), "one thread: the check fits: {:?}", one.calc.redo_reasons);
    for threads in [2, 4] {
        let mut t = two_arrays_read_by_many(threads, None);
        t.calc.plan_budget = Some(cap);
        let planned = looked_up_in(|| edit(&mut t, &[("Z1", "2")]));
        same_values(&t, &row_major, &format!("{threads} threads"));
        assert!(t.calc.plans_made > 0, "{threads} threads: planned within {cap}");
        assert_eq!(t.calc.redo_reasons, one.calc.redo_reasons, "{threads} threads: the round kept as on one thread");
        assert!(t.calc.worker_levels > 0, "{threads} threads: the level on workers");
        assert_eq!(t.calc.marking_given_up, 1, "{threads} threads: the level's last look-up runs out");
        // The one-thread run of the same edit makes every other look-up (planning's, the check's,
        // write-back's), so what this run looks up past it is the level's, at most its budget and
        // the one reader whose unit it refused.
        assert!(planned <= one_work + cap + 1, "{threads} threads: the look-up stopped inside: {planned} units against {one_work} on one thread");
    }
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
    for threads in [1, 4] {
        let mut planned = capped_book(&cells, threads, None);
        let mut row_major = capped_book(&cells, threads, Some(0));
        same_values(&planned, &row_major, &format!("{threads} threads, after loading"));
        for value in ["2", "1", "2"] {
            edit(&mut planned, &[("Z1", value)]);
            edit(&mut row_major, &[("Z1", value)]);
            same_values(&planned, &row_major, &format!("{threads} threads, after Z1 = {value}"));
            assert!(planned.calc.redo_reasons.is_empty(), "{threads} threads, after Z1 = {value}: {:?}", planned.calc.redo_reasons);
        }
        assert_eq!((planned.num("J1"), planned.num("Z50")), (10.0, 9.0), "{threads} threads");
    }
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
    for threads in [1, 4] {
        let mut t = capped_book(&cells, threads, None);
        let mut row_major = capped_book(&cells, threads, Some(0));
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{threads} threads, after loading");
        assert!(t.calc.row_major_only, "{threads} threads, after loading");
        for time in 1..=2 {
            t.calc.recalc_all(&mut t.wb);
            row_major.calc.recalc_all(&mut row_major.wb);
            assert_eq!(t.calc.plans_made, 0, "{threads} threads, full recalculation {time}: no plan");
            assert!(t.calc.row_major_only, "{threads} threads, full recalculation {time}");
            same_values(&t, &row_major, &format!("{threads} threads, full recalculation {time}"));
        }
        edit(&mut t, &[("Y1", "2")]);
        edit(&mut row_major, &[("Y1", "2")]);
        assert_eq!(t.calc.plans_made, 0, "{threads} threads: a value edit, no plan");
        same_values(&t, &row_major, &format!("{threads} threads, after Y1 = 2"));
        edit(&mut t, &[("Y2", "=Y1+1")]);
        edit(&mut row_major, &[("Y2", "=Y1+1")]);
        assert!(t.calc.plans_made > 0, "{threads} threads: a formula entered, the pass plans again");
        assert!(!t.calc.row_major_only, "{threads} threads: nothing calculated again since");
        same_values(&t, &row_major, &format!("{threads} threads, after Y2 = Y1+1"));
    }
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
    let mut t = capped_book(&cells, 1, None);
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

    let mut t = capped_book(&cells, 1, None);
    assert!(t.calc.row_major_only, "after loading");
    t.calc.recalc_all(&mut t.wb);
    assert_eq!(t.calc.plans_made, 0, "the same formulas: no plan");
    put(&mut t, "X1", "=Y1*5");
    t.calc.recalc_all(&mut t.wb);
    assert!(t.calc.plans_made > 0, "a formula replaced by one that reads the same cell: a plan");
    assert_eq!(t.num("X1"), 5.0);

    let mut t = capped_book(&cells, 1, None);
    put(&mut t, "Y2", "=Y1+1");
    t.calc.rebuild(&t.wb);
    assert!(!t.calc.row_major_only, "a formula entered");
}

#[test]
fn resolving_formulas_again_lets_the_next_pass_plan() {
    // Loading calculates the shut-out pair's round again, so passes run in row-major order.
    // Resolving a formula's precedents again (`Calc::resolve_again`, as for the readers of a name
    // rewritten in manual mode) lets that go, with the formula's text unchanged: the next full
    // recalculation makes a plan.
    let mut t = capped_book(&shut_out_cells(), 1, None);
    assert!(t.calc.row_major_only, "after loading");
    t.calc.resolve_again(&t.wb, &[(0, c("A5"))]);
    assert!(!t.calc.row_major_only, "resolved again");
    t.calc.recalc_all(&mut t.wb);
    assert!(t.calc.plans_made > 0, "the next pass plans");
}

#[test]
fn a_round_calculated_again_reads_the_clock_its_planned_try_read() {
    // The shut-out pair's first round is calculated again in row-major order, with B9 reading the
    // clock in it, which moves on by one at each reading. The clock is read once a round, as
    // without a plan: B9 shows the reading of its last round, whether that round was planned
    // first or not.
    for threads in [1, 4] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.ticking_now = Some(100.0);
        for (at, input) in shut_out_cells().iter().map(|(at, input)| (*at, input.as_str())).chain([("B9", "=NOW()")]) {
            put(&mut t, at, input);
        }
        t.calc.recalc_all(&mut t.wb);
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{threads} threads");
        let rounds = t.calc.rounds_calculated as f64;
        assert_eq!(t.calc.ticking_now, Some(100.0 + rounds), "{threads} threads: one reading for each of {rounds} rounds");
        assert_eq!(t.num("B9"), 100.0 + rounds - 1.0, "{threads} threads");
    }
}

#[test]
fn a_pass_with_no_redone_round_keeps_planning() {
    // A chain and an array whose readers wait for it: no round is calculated again, so every
    // pass plans.
    let cells: Vec<(&str, String)> =
        [("Z1", "1"), ("Z2", "=Z1*2"), ("A1", "=SEQUENCE(3)*Z2"), ("C1", "=SUM(A1#)")].map(|(at, input)| (at, input.to_string())).to_vec();
    for threads in [1, 4] {
        let mut t = capped_book(&cells, threads, None);
        for value in ["2", "3"] {
            edit(&mut t, &[("Z1", value)]);
            assert!(t.calc.plans_made > 0 && !t.calc.row_major_only && t.calc.redo_reasons.is_empty(), "{threads} threads, after Z1 = {value}");
            t.calc.recalc_all(&mut t.wb);
            assert!(t.calc.plans_made > 0 && !t.calc.row_major_only, "{threads} threads, full recalculation after Z1 = {value}");
        }
        assert_eq!(t.num("C1"), 36.0, "{threads} threads");
    }
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
    for threads in [1, 4] {
        let mut planned = capped_book(&cells, threads, None);
        let mut row_major = capped_book(&cells, threads, Some(0));
        same_values(&planned, &row_major, &format!("{threads} threads, after loading"));
        assert_eq!((planned.num("A1"), planned.num("C5"), planned.num("C2")), (3.0, 3.0, 3.0), "{threads} threads, after loading");
        for (value, want) in [("2", (3.0, 3.0, 4.0)), ("3", (4.0, 4.0, 5.0))] {
            // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
            planned.calc.row_major_only = false;
            edit(&mut planned, &[("Z1", value)]);
            edit(&mut row_major, &[("Z1", value)]);
            assert!(planned.calc.plans_made > 0, "{threads} threads, after Z1 = {value}: planned");
            same_values(&planned, &row_major, &format!("{threads} threads, after Z1 = {value}"));
            assert_eq!(
                (planned.num("A1"), planned.num("C5"), planned.num("C2")),
                want,
                "{threads} threads, after Z1 = {value}: row-major order's values (#4469)"
            );
        }
    }
}

// ---- Defined names read on worker threads --------------------------------------------------

/// How the formulas of `priced_book` read the prices.
#[derive(Clone, Copy, Debug)]
enum Pricing {
    /// Every formula through names, in mixed letter case, a LAMBDA name and a sheet-qualified name.
    ByName,
    /// The first ten rows of column A through a name, the rest as numbers.
    FewByName,
    /// Every formula of column A through INDIRECT of a name's text.
    ByIndirect,
    /// Every formula as numbers: the names are defined and read by nothing.
    ByNumber,
}

/// 301 names: `price_K` for the workbook, then `PRICE_K` (the first listed wins), then for an even
/// K `price_K` scoped to Sheet1 and `Price_K` after it, for K up to 99, each naming a cell of
/// Data's columns V to Y; and `double_it`, a LAMBDA name. From Sheet1, `price_K` is ten times K,
/// plus two for an even K; from Data, ten times K. Needs the Data sheet.
fn define_prices(t: &mut T) {
    let define = |t: &mut T, name: String, scope: Option<usize>, column: &str, value: u32, k: u32| {
        let at = format!("{column}{}", k + 1);
        put(t, &format!("Data!{at}"), &value.to_string());
        let formula = format!("Data!${column}${}", k + 1);
        t.wb.names_mut().push(gridcraft_model::DefinedName { name, scope, formula, comment: String::new(), hidden: false });
    };
    for k in 0..100 {
        define(t, format!("price_{k}"), None, "Y", k * 10, k);
        define(t, format!("PRICE_{k}"), None, "X", k * 10 + 1, k);
    }
    for k in (0..100).step_by(2) {
        define(t, format!("price_{k}"), Some(0), "W", k * 10 + 2, k);
        define(t, format!("Price_{k}"), Some(0), "V", k * 10 + 3, k);
    }
    t.name("double_it", "LAMBDA(x,x*2)");
}

/// 600 rows of formulas on Sheet1 and on Data reading the prices of `define_prices` as `pricing`
/// says, plus Sheet1's Z1 (0): one level of 2,400 formulas, every level threaded.
fn priced_book(threads: u32, plan_budget: Option<usize>, pricing: Pricing) -> T {
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
    t.calc.plan_budget = plan_budget;
    t.wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Data")));
    define_prices(&mut t);
    put(&mut t, "Z1", "0");
    for row in 1..=600u32 {
        let k = row % 100;
        let price = |name: &str| match pricing {
            Pricing::ByName => name.to_string(),
            Pricing::FewByName if row <= 10 => name.to_string(),
            _ => format!("{}", k * 10),
        };
        let first = match pricing {
            Pricing::ByIndirect => format!("=INDIRECT(\"price_{k}\")+$Z$1"),
            _ => format!("={}+$Z$1", price(&format!("pRiCe_{k}"))),
        };
        put(&mut t, &format!("A{row}"), &first);
        let second = match pricing {
            Pricing::ByName => format!("=DOUBLE_IT(Price_{k})+$Z$1"),
            _ => format!("={}*2+$Z$1", k * 10),
        };
        put(&mut t, &format!("B{row}"), &second);
        let data = match pricing {
            Pricing::ByName => (format!("PRICE_{k}"), format!("Sheet1!price_{k}")),
            _ => (format!("{}", k * 10), format!("{}", k * 10)),
        };
        put(&mut t, &format!("Data!A{row}"), &format!("={}+Sheet1!$Z$1", data.0));
        put(&mut t, &format!("Data!B{row}"), &format!("={}+Sheet1!$Z$1", data.1));
    }
    t.calc.recalc_all(&mut t.wb);
    t
}

#[test]
fn workers_find_defined_names_as_one_thread_does_through_one_shared_index() {
    // A worker finds a name where the main thread would: the first listed among spellings that
    // differ in letter case, a name scoped to the sheet before the workbook's. The pass's lookups
    // past the first few go through one index of the names, which the workers share: one built for
    // the pass on any thread count, none when the level names a few names or none (the workers
    // scan for those few).
    for pricing in [Pricing::ByName, Pricing::FewByName, Pricing::ByIndirect, Pricing::ByNumber] {
        let mut row_major = priced_book(1, Some(0), pricing);
        let loaded = priced_book(1, Some(0), pricing);
        edit(&mut row_major, &[("Z1", "5")]);
        for threads in [1, 2, 4] {
            let what = format!("{pricing:?} on {threads} threads");
            crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
            let mut planned = priced_book(threads, None, pricing);
            let built_loading = crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get);
            same_values(&planned, &loaded, &format!("{what}, after loading"));
            crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
            edit(&mut planned, &[("Z1", "5")]);
            let built_editing = crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get);
            same_values(&planned, &row_major, &format!("{what}, after Z1 = 5"));
            assert!(threads == 1 || planned.calc.worker_levels > 0, "{what}: the level ran on workers");
            // Loading builds the graph too, which indexes the names it resolves past the first few.
            let (loading, editing) = match pricing {
                Pricing::ByName => (2, 1),
                Pricing::ByIndirect => (1, 1),
                Pricing::FewByName | Pricing::ByNumber => (0, 0),
            };
            assert_eq!((built_loading, built_editing), (loading, editing), "{what}: indexes built loading and editing");
        }
        // Values worked out by hand, from the row-major run.
        let data = row_major.wb.sheet(1).unwrap();
        for row in [1u32, 2, 10, 11, 100, 599] {
            let k = f64::from(row % 100);
            let named = matches!(pricing, Pricing::ByName | Pricing::ByIndirect) || (matches!(pricing, Pricing::FewByName) && row <= 10);
            let from_sheet = if named && row % 2 == 0 { k * 10.0 + 2.0 } else { k * 10.0 };
            assert_eq!(row_major.num(&format!("A{row}")), from_sheet + 5.0, "{pricing:?}: A{row}");
            if matches!(pricing, Pricing::ByName) {
                assert_eq!(row_major.num(&format!("B{row}")), from_sheet * 2.0 + 5.0, "B{row}");
                assert_eq!(data.value(c(&format!("A{row}"))), Value::Number(k * 10.0 + 5.0), "Data!A{row}");
                assert_eq!(data.value(c(&format!("B{row}"))), Value::Number(from_sheet + 5.0), "Data!B{row}");
            }
        }
    }
}

/// The cells of `a_kept_worker_result_counts_its_reads_against_the_cap_as_one_thread_does`, with
/// `extra` beside them, loaded on `threads` threads, `cap` noted reads allowed after the load, then
/// Z1 = 3, which grows M5's array.
fn worker_notes_book(threads: u32, extra: &[(&str, &str)], cap: usize) -> T {
    let cells = [
        ("Z1", "1"),
        ("A1", "=N(INDIRECT(\"J10\"))+0*RAND()"),
        ("A2", "=N(INDIRECT(\"K1\"))+N(INDIRECT(\"K3\"))+N(INDIRECT(\"K2\"))"),
        ("M5", "=SEQUENCE($Z$1)"),
    ];
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
    t.calc.chunk = Some(1);
    t.load(cells.iter().chain(extra).map(|(at, input)| (at.to_string(), input.to_string())));
    t.calc.note_cap = Some(cap);
    edit(&mut t, &[("Z1", "3")]);
    t
}

#[test]
fn a_kept_worker_result_counts_its_reads_against_the_cap_as_one_thread_does() {
    // With two noted reads allowed, A1 (random, so calculated on the main thread) notes J10, and
    // A2 then reads K1, K3 and K2. One thread notes K1 and has nothing left for K3, which doesn't
    // join it: the round's reads are unknown. A worker notes K1 and K2:K3 (K2 joins K3), the same
    // two entries one thread would end with; kept at A2's turn they don't fit either, and must not
    // be joined again into K1:K3. Z1 = 3 grows M5's array, so a round whose reads are unknown is
    // calculated again in row-major order: on one, two and four threads alike.
    let one = worker_notes_book(1, &[], 2);
    assert_eq!(one.calc.watched.first(), Some(&(0, true)), "one thread: K3 finds no entry left");
    assert_eq!(one.calc.redo_reasons, vec![Redo::Budget], "one thread");
    assert!(one.calc.row_major_only, "one thread");
    for threads in [2, 4] {
        let mut workers = 0;
        for run in 1..=20 {
            let t = worker_notes_book(threads, &[], 2);
            let what = format!("{threads} threads, run {run}");
            assert_eq!(t.calc.watched, one.calc.watched, "{what}: the reads noted, round by round");
            assert_eq!(t.calc.redo_reasons, one.calc.redo_reasons, "{what}");
            assert_eq!(t.calc.row_major_only, one.calc.row_major_only, "{what}");
            same_values(&t, &one, &what);
            workers += t.calc.worker_levels;
        }
        assert!(workers > 0, "{threads} threads: A2 ran on a worker");
    }
    // What the hold changes: two random formulas in different levels, C1 reading C2, so the plan
    // draws C2 first and row-major order C1. With three reads allowed, one thread notes J10, K1
    // and K2:K3, and C1's read of C2 finds none left: the round is calculated again and the passes
    // after it run in row-major order. Joined into K1:K3 again, a worker's entries would leave
    // room for C2, the round would be kept, and the next pass (Y1, read by nothing, so only the
    // volatile formulas run) would draw in the plan's order: other values from the same seed.
    let random = [("C1", "=RAND()+C2"), ("C2", "=RAND()")];
    let mut one = worker_notes_book(1, &random, 3);
    assert_eq!(one.calc.redo_reasons, vec![Redo::Budget], "one thread, with the random formulas");
    assert!(one.calc.row_major_only, "one thread, with the random formulas");
    edit(&mut one, &[("Y1", "5")]);
    for threads in [2, 4] {
        for run in 1..=20 {
            let mut t = worker_notes_book(threads, &random, 3);
            let what = format!("{threads} threads, run {run}, with the random formulas");
            assert_eq!(t.calc.redo_reasons, vec![Redo::Budget], "{what}");
            assert!(t.calc.row_major_only, "{what}");
            edit(&mut t, &[("Y1", "5")]);
            assert_eq!(t.calc.plans_made, one.calc.plans_made, "{what}: the next pass");
            same_values(&t, &one, &format!("{what}, after Y1 = 5"));
        }
    }
}

#[test]
fn marking_the_readers_of_thrown_arrays_charges_their_per_cell_probes_first() {
    // A1 draws a random number, so it is calculated on the main thread, and lays out A1:B1. The
    // 400 formulas of row 1 read B1 through INDIRECT: a worker reads it empty and lays out 4,097
    // cells, an array the merge throws away (A1's array came before it) and calculates again
    // without one. Looking up its readers walks the whole per-cell index, which 20,000 one-cell
    // readers fill; one thread looks nothing up. Charged before each look-up, the walks stay
    // within the round's allowance, and marking gives up (the rest of the level runs on the main
    // thread) rather than walking it 400 times.
    let fillers = 20_000u32;
    let mut cells = vec![("A1".to_string(), "=SEQUENCE(1,2)+0*RAND()".to_string())];
    cells.extend((0..400).map(|i| (CellRef::new(0, 2 + i).a1(), "=IF(INDIRECT(\"B1\")=2,0,SEQUENCE(4097))".to_string())));
    cells.extend((0..fillers).map(|r| (CellRef::new(r, 700).a1(), format!("={}", CellRef::new(r, 701).a1()))));
    let book = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        for (at, input) in &cells {
            t.wb.sheet_mut(0).unwrap().cells.set(c(at), Cell::formula(Formula::new(input)));
        }
        crate::recalc::MARKING_PROBES.with(|n| n.set(0));
        t.calc.recalc_all(&mut t.wb);
        let probes = crate::recalc::MARKING_PROBES.with(std::cell::Cell::get);
        (t, probes)
    };
    let (one, none) = book(1);
    assert_eq!(none, 0, "one thread marks nothing");
    assert_eq!(one.num("C1"), 0.0);
    // What each round's marking may spend: the round's index-work budget, every formula dirty.
    let mut dirty: Vec<Key> = cells.iter().map(|(at, _)| (0, c(at))).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    let allowance = one.calc.graph.levels(&one.wb, &dirty).map(|p| p.budget()).unwrap_or(0);
    assert!(allowance > 0 && allowance < 400 * fillers as usize, "{allowance}: less than a walk of the index for each array");
    for threads in [2, 4] {
        let (t, probes) = book(threads);
        let what = format!("{threads} threads");
        assert!(probes <= allowance + fillers as usize + 2, "{what}: {probes} probes in marking, {allowance} allowed");
        assert!(t.calc.marking_given_up > 0, "{what}: marking gave up");
        assert!(t.calc.worker_levels > 0, "{what}: the level ran on workers");
        assert_eq!(t.calc.watched, one.calc.watched, "{what}");
        assert_eq!(t.calc.redo_reasons, one.calc.redo_reasons, "{what}");
        assert_eq!(t.calc.row_major_only, one.calc.row_major_only, "{what}");
        same_values(&t, &one, &what);
    }
}

#[test]
fn let_variables_are_not_counted_as_name_lookups_for_the_workers() {
    // A workbook of forty defined names and 600 formulas whose only names are a LET variable: the
    // evaluator finds `step` among the bindings, never among the workbook's names, so no index of
    // them is built for the workers, as none is on one thread.
    let words = ["amber", "basil", "cedar", "dune", "ember", "fern", "glade", "heath", "inlet", "jade"];
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        for word in words {
            for suffix in ["_rate", "_cost", "_tax", "_fee"] {
                t.name(&format!("{word}{suffix}"), "=1");
            }
        }
        let mut cells = vec![("Z1".to_string(), "1".to_string())];
        cells.extend((1..=600).map(|r| (format!("A{r}"), "=LET(step,$Z$1,step+ROW())".to_string())));
        t.load(cells);
        crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
        edit(&mut t, &[("Z1", "5")]);
        let built = crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get);
        (t, built)
    };
    let (row_major, _) = book(1, Some(0));
    assert_eq!(row_major.num("A600"), 605.0);
    for threads in [1, 4] {
        let (t, built) = book(threads, None);
        assert_eq!(built, 0, "{threads} threads: no name index built in the pass");
        assert!(threads == 1 || t.calc.worker_levels > 0, "{threads} threads: the level ran on workers");
        same_values(&t, &row_major, &format!("{threads} threads"));
    }
}

#[test]
fn a_worker_reads_nothing_more_for_a_formula_it_hands_back() {
    // Each formula of column D reads the F cell of its row through INDIRECT. One thread calculates
    // F first, so ISERROR is false and the sums are never read. A worker finds F not done, which
    // hands D back with `#CALC!` from that read: the branch that sums 5,000 cells must read
    // nothing, by any path (a block summed, visited or read).
    let book = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(1);
        let mut cells = vec![("A1".to_string(), "3".to_string())];
        cells.extend((1..=5000).map(|r| (format!("ZZ{r}"), format!("{r}"))));
        cells.extend(
            (1..=600)
                .map(|r| (format!("D{r}"), "=IF(ISERROR(INDIRECT(\"F\"&ROW())),SUM($ZZ$1:$ZZ$5000)+SUMPRODUCT($ZZ$1:$ZZ$5000),0)+$A$1".to_string())),
        );
        cells.extend((1..=600).map(|r| (format!("F{r}"), "=$A$1*2".to_string())));
        t.load(cells);
        edit(&mut t, &[("A1", "4")]);
        t
    };
    let one = book(1);
    assert_eq!((one.num("D600"), one.num("F600")), (4.0, 8.0));
    for threads in [2, 4] {
        let t = book(threads);
        let what = format!("{threads} threads");
        assert!(t.calc.handed_back > 0, "{what}: the workers handed formulas back");
        assert_eq!(t.calc.blocked_walks, 0, "{what}: walks for formulas handed back");
        same_values(&t, &one, &what);
    }
}

#[test]
fn a_worker_stops_a_read_once_it_hands_the_formula_back() {
    // In the second level, column D sums column F through INDIRECT, and column I reads F:H
    // through INDIRECT as values; column F is in that level too, so a worker meets a formula of F
    // not done partway through the block and hands the reader back. H1 spills down H from the
    // first level, so the values read lay an array over the block. Neither read walks on, or
    // lays the array over, once the reader is handed back. Then, with no reads left to note, a
    // worker's first read hands its formula back, and what it would have read is not walked:
    // F1:F600 read as values (column M), after SUM's first argument walked (column N), and summed
    // (column O); and in a book of its own (the main thread runs out of entries in the first
    // level of the other, and notes nothing after), the area of `$H$1#` (column J). Whichever
    // read hands a formula back, the evaluation reads nothing after it: not the rest of
    // SUBTOTAL's cells (column B), and not the cell each formula reads last, after a read of F
    // through implicit intersection (column C), TRIMRANGE (column L), INDIRECT alone (column Q),
    // CELL (columns R and S), or any of the above; and not the rest of the cells INDEX picks one
    // at a time for an array of positions (column T), nor the rest of a union's cells, summed
    // (column U) or flattened into a function's argument (column V), nor in the calls of a
    // LAMBDA that REDUCE (column W) or BYROW (column X) makes after the one whose read blocked.
    let reads = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells =
            vec![("A1".to_string(), "3".to_string()), ("E1".to_string(), "=$A$1".to_string()), ("H1".to_string(), "=SEQUENCE(600)+$A$1".to_string())];
        for r in 1..=600 {
            cells.push((format!("B{r}"), "=SUBTOTAL(9,INDIRECT(\"F1:F600\"))+$E$1".to_string()));
            cells.push((format!("C{r}"), "=@INDIRECT(\"F1:F600\")+$E$1".to_string()));
            cells.push((format!("D{r}"), "=SUM(INDIRECT(\"F1:F600\"))+$E$1".to_string()));
            cells.push((format!("F{r}"), "=$E$1*2".to_string()));
            cells.push((format!("I{r}"), "=SUMPRODUCT(INDIRECT(\"F1:H600\"))+$E$1".to_string()));
            cells.push((format!("R{r}"), "=N(CELL(\"contents\",INDIRECT(\"F\"&(ROW()+1))))+$E$1".to_string()));
            cells.push((format!("S{r}"), "=LEN(CELL(\"type\",INDIRECT(\"F\"&(ROW()+1))))+$E$1".to_string()));
            cells.push((format!("L{r}"), "=SUM(TRIMRANGE(INDIRECT(\"F1:F600\")))+$E$1".to_string()));
            cells.push((format!("Q{r}"), "=INDIRECT(\"F\"&(ROW()+1))+$E$1".to_string()));
            cells.push((format!("T{r}"), "=SUM(INDEX(INDIRECT(\"F1:F600\"),SEQUENCE(600)))+$E$1".to_string()));
            let union = "LET(a,(INDIRECT(\"F1\"),INDIRECT(\"F2\")),b,(a,a),c,(b,b),d,(c,c),e,(d,d),(e,e))";
            cells.push((format!("U{r}"), format!("=SUM({union})+$E$1")));
            cells.push((format!("V{r}"), format!("=MAX({union})+$E$1")));
            cells.push((format!("W{r}"), "=REDUCE(0,SEQUENCE(100),LAMBDA(a,i,a+INDIRECT(\"F\"&i)))+$E$1".to_string()));
            cells.push((format!("X{r}"), "=SUM(BYROW(SEQUENCE(100),LAMBDA(i,INDIRECT(\"F\"&i))))+$E$1".to_string()));
        }
        t.load(cells);
        edit(&mut t, &[("A1", "4")]);
        t
    };
    let spill_operator = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells = vec![("A1".to_string(), "3".to_string()), ("H1".to_string(), "=SEQUENCE(600)+$A$1".to_string())];
        cells.extend((1..=600).map(|r| (format!("J{r}"), "=ROWS($H$1#)+N(INDIRECT(\"K\"&ROW()))".to_string())));
        t.load(cells);
        t.calc.note_cap = Some(0);
        edit(&mut t, &[("A1", "4")]);
        t
    };
    let spill_reference = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells = vec![("A1".to_string(), "3".to_string())];
        cells.extend((1..=600).map(|r| (format!("F{r}"), r.to_string())));
        cells.extend((1..=600).map(|r| (format!("M{r}"), "=SUMPRODUCT($F$1:$F$600)+$A$1+N(INDIRECT(\"P1\"))".to_string())));
        cells.extend((1..=600).map(|r| (format!("N{r}"), "=SUM(1,$F$1:$F$600)+$A$1+N(INDIRECT(\"P1\"))".to_string())));
        cells.extend((1..=600).map(|r| (format!("O{r}"), "=SUM($F$1:$F$600)+$A$1+N(INDIRECT(\"P1\"))".to_string())));
        t.load(cells);
        t.calc.note_cap = Some(0);
        edit(&mut t, &[("A1", "4")]);
        t
    };
    let (one_reads, one_operator, one_reference) = (reads(1), spill_operator(1), spill_reference(1));
    assert_eq!((one_reads.num("D1"), one_reads.num("I1")), (4804.0, 4804.0 + 600.0 * 4.0 + 180_300.0));
    assert_eq!((one_reads.num("B1"), one_reads.num("C1"), one_reads.num("L1")), (4804.0, 12.0, 4804.0));
    assert_eq!((one_reads.num("Q1"), one_reads.num("R1"), one_reads.num("S1")), (12.0, 12.0, 5.0));
    assert_eq!((one_reads.num("T1"), one_reads.num("U1"), one_reads.num("V1")), (4804.0, 516.0, 12.0));
    assert_eq!((one_reads.num("W1"), one_reads.num("X1")), (804.0, 804.0));
    assert_eq!((one_operator.num("J600"), one_reference.num("M600"), one_reference.num("N600")), (600.0, 180_304.0, 180_305.0));
    assert_eq!(one_reference.num("O600"), 180_304.0);
    for threads in [2, 4] {
        let books = [
            ("block reads", reads(threads), &one_reads),
            ("spill operator", spill_operator(threads), &one_operator),
            ("spill reference", spill_reference(threads), &one_reference),
        ];
        for (name, t, one) in books {
            let what = format!("{name}, {threads} threads");
            assert!(t.calc.worker_levels > 0 && t.calc.handed_back > 0, "{what}: the workers handed formulas back");
            assert_eq!(t.calc.blocked_walks, 0, "{what}: walks for formulas handed back");
            assert_eq!(t.calc.blocked_reads, 0, "{what}: reads for formulas handed back");
            same_values(&t, one, &what);
        }
    }
}

#[test]
fn a_worker_stops_drawing_once_it_hands_the_formula_back() {
    // `noise` draws 2,500 random numbers, and the 600 formulas reach it through the name, so they
    // are calculated on the workers. A worker's first draw hands its formula back, as the main
    // thread draws from the one generator; the array function must not go on drawing the rest
    // (nor a lifted RANDBETWEEN calling itself once per element), nor the formula go on to its
    // next RAND. The main thread then draws for each formula at its turn, as one thread does.
    let book = |threads: u32, definition: &str| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.name("noise", definition);
        t.load((1..=600).map(|r| (format!("C{r}"), "=SUM(noise)*0+ROW()".to_string())));
        t
    };
    for definition in ["=RANDARRAY(50,50)", "=RANDARRAY(50,50,1,6,TRUE)", "=RANDBETWEEN(SEQUENCE(50,50),5000)", "=RAND()+RAND()"] {
        let one = book(1, definition);
        assert_eq!(one.num("C600"), 600.0, "{definition}");
        for threads in [2, 4] {
            let t = book(threads, definition);
            let what = format!("{definition}, {threads} threads");
            assert!(t.calc.worker_levels > 0 && t.calc.handed_back > 0, "{what}: the workers handed formulas back");
            assert_eq!(t.calc.blocked_reads, 0, "{what}: draws after a formula was handed back");
            same_values(&t, &one, &what);
        }
    }
}

#[test]
fn a_worker_stops_evaluating_a_formula_once_it_hands_it_back() {
    // A1 draws a random number, so it stays on the main thread. Each formula of column C reads it
    // through INDIRECT: one thread calculates A1 first and never takes IFERROR's fallback, while
    // a worker finds A1 not done, hands the formula back, and must not go on to the fallback (a
    // large SEQUENCE, and NOW and OFFSET, which the count sees) on the error that read gave. The
    // formulas of column D read B1 and are calculated on the workers, which go on after the read.
    let book = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells = vec![("A1".to_string(), "=1+0*RAND()".to_string()), ("B1".to_string(), "5".to_string())];
        cells.extend((1..=600).map(|r| (format!("C{r}"), "=IFERROR(INDIRECT(\"A1\")+0,SUM(SEQUENCE(100000))+NOW()+OFFSET(Z1,0,0))".to_string())));
        cells.extend((1..=600).map(|r| (format!("D{r}"), "=$B$1+ROW()".to_string())));
        t.load(cells);
        t
    };
    let one = book(1);
    assert_eq!((one.num("C600"), one.num("D600")), (1.0, 605.0));
    for threads in [2, 4] {
        let t = book(threads);
        let what = format!("{threads} threads");
        assert!(t.calc.worker_levels > 0 && t.calc.handed_back > 0, "{what}: the workers handed formulas back");
        assert_eq!(t.calc.blocked_calls, 0, "{what}: calls after a formula was handed back");
        same_values(&t, &one, &what);
    }
}

/// Ten formulas in row 1 from C, each `formula`, on `threads` taking `chunk` formulas at a time (a
/// chunk of 100: one worker), with `level_bytes` for what the level's workers may keep. `edited`:
/// loaded first, then Z1 = 2 recalculates them with the budget; otherwise the load does.
fn held_bytes_book(threads: u32, chunk: usize, formula: &str, level_bytes: Option<usize>, edited: bool) -> T {
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
    t.calc.chunk = Some(chunk);
    let mut cells = vec![("Z1".to_string(), "1".to_string()), ("A1".to_string(), "=SEQUENCE(1,2)+0*RAND()".to_string())];
    cells.extend((0..10).map(|i| (CellRef::new(0, 2 + i).a1(), formula.to_string())));
    if !edited {
        t.calc.level_bytes = level_bytes;
    }
    t.load(cells);
    if edited {
        t.calc.level_bytes = level_bytes;
        edit(&mut t, &[("Z1", "2")]);
    }
    t
}

#[test]
fn a_worker_hands_back_a_formula_whose_array_or_text_passes_what_the_level_may_keep() {
    // The arrays of the first case are kept at the merge (Z1 = 2 changes their values, not their
    // shape); those of the second are thrown away there (A1, random, lays out A1:B1 first on the
    // main thread), as is the text of the third. The hundred values of each array of the fourth
    // share one text, which the worker holds, and is charged, once; those of the fifth alternate
    // between two texts, each charged once. First one worker takes the ten formulas (a chunk of
    // 100), so it has the level's whole share: at the bytes of all ten it keeps them; one byte
    // less, it hands the last one back, without its result or its array, and the main thread
    // calculates it at its turn. Then the formulas go one per chunk to two or four workers, each
    // with its share: at ten results' bytes each, they keep all ten; at one byte less than one
    // result's, they hand all ten back, however the formulas fall to them. Either way the values
    // are one thread's, and the arrays and text of the results the workers hand in, added up over
    // the level (`peak_worker_bytes`, which leaves out their copied blocks), stay within the
    // level's bytes.
    let numbers = 100 * std::mem::size_of::<Value>();
    let text = std::mem::size_of::<Value>() + 1000 + 16;
    let cases = [
        ("kept arrays", "=SEQUENCE(100)+$Z$1", numbers, true),
        ("thrown arrays", "=IF(INDIRECT(\"B1\")=2,0,SEQUENCE(100))", numbers, false),
        ("thrown text", "=IF(INDIRECT(\"B1\")=2,0,REPT(\"x\",1000))", text, false),
        ("shared text", "=IF(SEQUENCE(100),REPT(\"x\",1000))", numbers + text, false),
        ("two shared texts", "=IF(ISODD(SEQUENCE(100)),REPT(\"x\",1000),REPT(\"y\",1000))", numbers + text + 1000 + 16, false),
    ];
    for (name, formula, each, edited) in cases {
        let one = held_bytes_book(1, 100, formula, None, edited);
        let mut runs = vec![(4u32, 100, 10 * each, 0), (4, 100, 10 * each - 1, 1)];
        for threads in [2u32, 4] {
            let workers = threads as usize;
            runs.extend([(threads, 1, workers * 10 * each, 0), (threads, 1, workers * (each - 1), 10)]);
        }
        for (threads, chunk, level_bytes, handed) in runs {
            let t = held_bytes_book(threads, chunk, formula, Some(level_bytes), edited);
            let what = format!("{name}, {threads} threads, chunks of {chunk}, {level_bytes} bytes");
            assert!(t.calc.worker_levels > 0, "{what}: the level ran on workers");
            assert_eq!(t.calc.handed_back, handed, "{what}: handed back");
            assert!(t.calc.peak_worker_bytes <= level_bytes, "{what}: {} bytes held", t.calc.peak_worker_bytes);
            same_values(&t, &one, &what);
        }
    }
}

#[test]
fn each_worker_keeps_no_more_than_its_share_of_what_the_level_may_keep() {
    // Eight formulas, one per chunk, so that all the level's workers (two or four) start and
    // split what it may keep between them. At two arrays' bytes less one, no worker's share holds
    // one array: every formula is handed back, however they fall to the workers, and the main
    // thread calculates them at their turn.
    let each = 100 * std::mem::size_of::<Value>();
    let book = |threads: u32, level_bytes: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(1);
        t.calc.level_bytes = level_bytes;
        let mut cells = vec![("Z1".to_string(), "1".to_string())];
        cells.extend((0..8).map(|i| (CellRef::new(0, 2 + i).a1(), "=SEQUENCE(100)+$Z$1".to_string())));
        t.load(cells);
        t
    };
    let one = book(1, None);
    for threads in [2, 4] {
        let t = book(threads, Some(2 * each - 1));
        let what = format!("{threads} threads");
        assert!(t.calc.worker_levels > 0, "{what}: the level ran on workers");
        assert_eq!(t.calc.handed_back, 8, "{what}: handed back");
        same_values(&t, &one, &what);
    }
}

#[test]
fn a_worker_hands_back_the_rest_of_its_chunk_uncalculated_once_a_result_does_not_fit() {
    // One worker takes the ten formulas (a chunk of 100). Four arrays fit in what the level may
    // keep; the fifth doesn't, so the worker hands it back, and the five after it without
    // calculating them: their arrays would only be thrown away. The main thread calculates the
    // six at their turn.
    let each = 100 * std::mem::size_of::<Value>();
    let one = held_bytes_book(1, 100, "=SEQUENCE(100)+$Z$1", None, false);
    for threads in [2, 4] {
        let t = held_bytes_book(threads, 100, "=SEQUENCE(100)+$Z$1", Some(5 * each - 1), false);
        let what = format!("{threads} threads");
        assert!(t.calc.worker_levels > 0, "{what}: the level ran on workers");
        assert_eq!(t.calc.handed_back, 6, "{what}: handed back");
        assert_eq!(t.calc.worker_calculated, 5, "{what}: calculated on the worker");
        // The four kept results are found in that worker's results at their turn: the main
        // thread calculates the six and A1 (random).
        assert_eq!(t.calc.level_formulas_on_main, 7, "{what}: calculated on the main thread");
        same_values(&t, &one, &what);
    }
}

/// The bytes a worker's record of the blocks it read once takes for its first entries (see
/// `PassHost::seen`), charged with its copies: the byte budgets below leave room for it.
fn first_read_record_bytes() -> usize {
    let mut record: std::collections::HashSet<(usize, gridcraft_core::RangeRef)> = std::collections::HashSet::new();
    record.insert((0, gridcraft_core::RangeRef::new(gridcraft_core::CellRef::new(0, 0), gridcraft_core::CellRef::new(0, 0))));
    record.capacity() * (std::mem::size_of::<(usize, gridcraft_core::RangeRef)>() + 1)
}

#[test]
fn a_workers_copies_make_way_for_a_result_that_fits_without_them() {
    // One worker takes the ten formulas (a chunk of 100), each an array of 100 numbers and a
    // read of Y1:Y200, which the worker copies at its second read (two arrays' bytes). The level
    // may keep seven arrays' bytes: the copy and five arrays fill it. The sixth array fits once
    // the copy goes, so it goes, and the formulas after it read the block from the store. The
    // seventh fits too; the eighth doesn't, and is handed back, the two after it uncalculated.
    let each = 100 * std::mem::size_of::<Value>();
    let book = |threads: u32, level_bytes: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(100);
        t.calc.level_bytes = level_bytes;
        let mut cells: Vec<(String, String)> = (1..=200).map(|r| (format!("Y{r}"), r.to_string())).collect();
        cells.extend((0..10).map(|i| (CellRef::new(0, 2 + i).a1(), "=SEQUENCE(100)+SUMPRODUCT($Y$1:$Y$200)".to_string())));
        t.load(cells);
        t
    };
    let one = book(1, None);
    for threads in [2, 4] {
        let t = book(threads, Some(7 * each + first_read_record_bytes()));
        let what = format!("{threads} threads");
        assert!(t.calc.worker_levels > 0, "{what}: the level ran on workers");
        assert_eq!(t.calc.handed_back, 3, "{what}: handed back");
        assert_eq!(t.calc.worker_calculated, 8, "{what}: calculated on the worker");
        assert_eq!(t.calc.copies_read, 4, "{what}: reads served from the copy");
        same_values(&t, &one, &what);
    }
}

#[test]
fn a_worker_copies_no_more_once_its_copies_made_way() {
    // One worker takes the ten formulas (a chunk of 100), each reading X1:X200 and Y1:Y200, which
    // it copies at its second read (two arrays' bytes each). The level may keep four arrays'
    // bytes, all taken by the copies when the third formula's array comes, so they go. The rest
    // read the blocks from the store: two reads served from the copies (the third formula's), and
    // no copy made again, though one of them would fit beside that array.
    let each = 100 * std::mem::size_of::<Value>();
    let book = |threads: u32, level_bytes: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(100);
        t.calc.level_bytes = level_bytes;
        let mut cells: Vec<(String, String)> =
            (1..=200).flat_map(|r| [(format!("X{r}"), r.to_string()), (format!("Y{r}"), (2 * r).to_string())]).collect();
        let reads = "SUMPRODUCT($X$1:$X$200)+SUMPRODUCT($Y$1:$Y$200)";
        cells.extend((0..10).map(|i| (CellRef::new(0, 2 + i).a1(), if i == 2 { format!("=SEQUENCE(100)+{reads}") } else { format!("={reads}") })));
        t.load(cells);
        t
    };
    let one = book(1, None);
    let t = book(4, Some(4 * each + first_read_record_bytes()));
    assert!(t.calc.worker_levels > 0, "the level ran on workers");
    assert_eq!(t.calc.handed_back, 0, "handed back");
    assert_eq!(t.calc.copies_read, 2, "reads served from the copies");
    same_values(&t, &one, "four threads");
}

#[test]
fn a_result_that_does_not_fit_without_the_copies_leaves_them() {
    // Thirty formulas taken three at a time: two in three read AX1:AX200, which a worker copies at
    // its second read (two arrays' bytes), and every third is an array of five arrays' bytes, more
    // than a worker may keep (four), so it is handed back. The copy stays, as dropping it would not
    // make room, and the worker reads it in the chunks it takes after (at least one worker takes
    // more than one of the ten).
    let each = 100 * std::mem::size_of::<Value>();
    let book = |threads: u32, level_bytes: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(3);
        t.calc.level_bytes = level_bytes;
        let mut cells: Vec<(String, String)> = (1..=200).map(|r| (format!("AX{r}"), r.to_string())).collect();
        cells.extend((0..30).map(|i| {
            let formula = if i % 3 == 2 { "=SEQUENCE(500)+$Z$1" } else { "=SUMPRODUCT($AX$1:$AX$200)+$Z$1" };
            (CellRef::new(0, 2 + i).a1(), formula.to_string())
        }));
        t.load(cells);
        t
    };
    let one = book(1, None);
    for threads in [2u32, 4] {
        let t = book(threads, Some(threads as usize * 4 * each));
        let what = format!("{threads} threads");
        assert!(t.calc.worker_levels > 0, "{what}: the level ran on workers");
        assert_eq!(t.calc.handed_back, 10, "{what}: handed back");
        assert!(t.calc.copies_read > 0, "{what}: reads served from the copy");
        same_values(&t, &one, &what);
    }
}

/// Seventeen one-word names, each a number, and `Alias`, their sum: one evaluation of `Alias`
/// looks up eighteen names, past the scans a lookup makes before it needs an index.
fn aliased_names(t: &mut T) {
    let words = [
        "apple", "berry", "cherry", "damson", "elder", "fig", "grape", "hazel", "iris", "juniper", "kiwi", "lemon", "mango", "nutmeg", "olive",
        "peach", "quince",
    ];
    for (i, word) in words.iter().enumerate() {
        t.name(word, &format!("={}", i + 1));
    }
    t.name("Alias", &format!("={}", words.join("+")));
}

#[test]
fn the_workers_of_a_round_build_at_most_one_name_index_between_them() {
    // Sixteen formulas name only `Alias` in their text, few enough that no index is built before
    // the workers start; each evaluation reaches eighteen names. One worker past its scans builds
    // the index the others then read; in a chain of seventeen levels the main thread takes it over
    // after the first, and the later levels read it. One index in the pass on any thread count.
    let fan = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 16;
        t.calc.chunk = Some(1);
        t.calc.plan_budget = plan_budget;
        aliased_names(&mut t);
        let mut cells = vec![("Z1".to_string(), "1".to_string())];
        cells.extend((1..=16).map(|r| (format!("A{r}"), "=Alias+$Z$1".to_string())));
        t.load(cells);
        crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
        edit(&mut t, &[("Z1", "2")]);
        let built = crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get);
        (t, built)
    };
    let chain = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        aliased_names(&mut t);
        let mut cells = vec![("Z1".to_string(), "1".to_string()), ("C1".to_string(), "=Alias+$Z$1".to_string())];
        cells.extend((2..=17).map(|k| (format!("C{k}"), format!("=C{}+Alias", k - 1))));
        t.load(cells);
        crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(0));
        edit(&mut t, &[("Z1", "2")]);
        let built = crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get);
        (t, built)
    };
    let (fan_row_major, _) = fan(1, Some(0));
    let (chain_row_major, _) = chain(1, Some(0));
    assert_eq!(fan_row_major.num("A16"), 153.0 + 2.0);
    assert_eq!(chain_row_major.num("C17"), 153.0 * 17.0 + 2.0);
    for threads in [1, 2, 4] {
        for run in 1..=5 {
            let what = format!("{threads} threads, run {run}");
            let (t, built) = fan(threads, None);
            assert_eq!(built, 1, "{what}: indexes built for sixteen formulas in one level");
            assert!(threads == 1 || t.calc.worker_levels > 0, "{what}: the level ran on workers");
            same_values(&t, &fan_row_major, &what);
            let (t, built) = chain(threads, None);
            assert!(built <= 1, "{what}: {built} indexes built over seventeen levels");
            assert!(threads == 1 || t.calc.worker_levels >= 17, "{what}: the levels ran on workers");
            same_values(&t, &chain_row_major, &format!("{what}, the chain"));
        }
    }
}

#[test]
fn planning_tests_its_budget_at_each_entry_it_releases() {
    // 64 formulas in A1:A64, and 2,000 readers in column B each summing all of them: every reader
    // gets an exact release list, so completing the first level releases 128,000 entries, a unit
    // each. With a cap in the middle of that wave, planning gives up at the entry that passes it,
    // one unit over, not at the end of the wave, 64,000 units later.
    let book = |plan_budget: Option<usize>| {
        let mut t = T::new().threads(1);
        t.calc.plan_budget = plan_budget;
        let mut cells: Vec<(String, String)> = (1..=64).map(|r| (format!("A{r}"), "=$Z$1+ROW()".to_string())).collect();
        cells.extend((1..=2000).map(|r| (format!("B{r}"), "=SUM($A$1:$A$64)".to_string())));
        cells.push(("Z1".to_string(), "1".to_string()));
        t.load(cells);
        t
    };
    let t = book(None);
    let mut dirty: Vec<Key> = (1..=64).map(|r| (0, c(&format!("A{r}")))).chain((1..=2000).map(|r| (0, c(&format!("B{r}"))))).collect();
    dirty.sort_by_key(|(s, at)| (*s, at.row, at.col));
    let plan = t.calc.graph.levels(&t.wb, &dirty).unwrap();
    let released = 64 * 2000;
    assert!(plan.work() > released, "{} index work", plan.work());
    let cap = plan.work() - released / 2;
    let past = t.calc.graph.levels_within(&t.wb, &dirty, Some(cap));
    assert!(matches!(past, Err(NoPlan::Budget { work }) if work == cap + 1), "{past:?} with a cap of {cap}");
    assert!(t.calc.graph.levels_within(&t.wb, &dirty, Some(plan.work())).is_ok(), "at the budget");
    // Through a pass: no plan, row-major order from then on, the values a row-major run gives.
    let mut capped = book(Some(cap));
    let mut row_major = book(Some(0));
    capped.calc.row_major_only = false;
    edit(&mut capped, &[("Z1", "2")]);
    edit(&mut row_major, &[("Z1", "2")]);
    assert!(capped.calc.row_major_only, "planning gave up");
    same_values(&capped, &row_major, "past the budget, against row-major order");
    assert_eq!(capped.num("B2000"), 64.0 * 65.0 / 2.0 + 128.0);
}

#[test]
fn a_read_under_an_unchanged_array_and_a_new_one_meeting_it_ends_as_in_row_major_order() {
    // B1's array (B1:B3) comes out as stored, so it is not among the arrays whose readers the
    // check looks up; A2's new one (A2:C2) meets it at B2, which B1 keeps (it held the cell). D3
    // reads B2 through INDIRECT. In the edit's second round (G1's array changed, so E1 and A2 run
    // again, B1 through E1, and the dynamic formulas once more) the plan lays out A2's array
    // before D3 and B1's after it (B1 waits for E1): D3 would read 11 from A2's array, where
    // row-major order lays B1's out first and D3 reads 2 from it. The round is sent back because
    // the two arrays meet. No dynamic formula runs again after that round, so a value kept from
    // the planned round would stay.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        t.load(
            [
                ("Z1", "1"),
                ("G1", "=SEQUENCE(2,1,Z1)"),
                ("E1", "=G2"),
                ("B1", "=SEQUENCE(3)+0*E1"),
                ("A2", "=SEQUENCE(1,3,10+0*G2)"),
                ("D3", "=INDIRECT(\"B2\")+0"),
            ]
            .map(|(at, input)| (at.to_string(), input.to_string())),
        );
        t.calc.row_major_only = false;
        edit(&mut t, &[("Z1", "2")]);
        t
    };
    let row_major = book(1, Some(0));
    assert_eq!((row_major.num("D3"), row_major.num("B2"), row_major.get("A2")), (2.0, 2.0, Value::Error(CellError::Spill)));
    for threads in [1, 4] {
        let t = book(threads, None);
        assert!(t.calc.redo_reasons.contains(&Redo::Overlap), "{threads} threads: {:?}", t.calc.redo_reasons);
        same_values(&t, &row_major, &format!("{threads} threads"));
    }
}

// ---- Readers before an array's anchor that read its cells through a block holding it ----

/// Cells to load: address and input.
type Layout<'a> = &'a [(&'a str, &'a str)];

/// `cells` loaded on `threads` threads with every level on worker threads, and the same cells
/// loaded in row-major order (no plan), which the first must match.
fn loaded_beside_row_major(cells: Layout<'_>, threads: u32) -> (T, T) {
    let owned = || cells.iter().map(|(at, input)| (at.to_string(), input.to_string()));
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
    t.load(owned());
    let mut row_major = T::new().threads(threads);
    row_major.calc.plan_budget = Some(0);
    row_major.load(owned());
    (t, row_major)
}

#[test]
fn a_reader_before_an_anchor_reading_a_block_that_holds_it_is_kept() {
    // Each reader comes before the array's anchor in row-major order and reads cells of the new
    // array, which the plan lays out before it. Its block holds the anchor, so in row-major order
    // the read calculates the anchor before it takes a value too: the reader reads the array laid
    // out in both orders, and the planned round is kept.
    let layouts: [(Layout<'_>, &str, f64); 4] = [
        (&[("B1", "=SUM(D5:D10)"), ("D5", "=SEQUENCE(3)")], "B1", 6.0),
        (&[("B1", "=SUM(D:D)"), ("D5", "=SEQUENCE(3)")], "B1", 6.0),
        (&[("A5", "=VLOOKUP(3,D5:E10,2,FALSE)"), ("D5", "=SEQUENCE(3,2)")], "A5", 4.0),
        (&[("A1", "=COUNTA(D:D)"), ("D2", "=SEQUENCE(4)")], "A1", 4.0),
    ];
    for (cells, reader, value) in layouts {
        for threads in [1, 4] {
            let (t, row_major) = loaded_beside_row_major(cells, threads);
            let what = format!("{cells:?} on {threads} threads");
            assert_eq!(t.num(reader), value, "{what}");
            same_values(&t, &row_major, &what);
            assert!(t.calc.redo_reasons.is_empty(), "{what}: {:?}", t.calc.redo_reasons);
            assert!(t.calc.plans_made > 0 && t.calc.plans_made == t.calc.rounds_calculated, "{what}: every round planned and kept");
            assert!(!t.calc.row_major_only, "{what}: no hold on the next edit");
            if threads > 1 {
                assert!(t.calc.worker_levels > 0, "{what}: no level on workers");
            }
        }
    }
}

#[test]
fn a_reader_before_an_anchor_reading_its_cells_without_it_is_sent_back() {
    // The same readers, reading cells of the array without calculating the anchor: a block that
    // starts below it, one cell through INDEX, one cell read before a block that holds the
    // anchor. In row-major order they read those cells before the array is laid out, so the
    // planned round, which lays it out before them, is calculated again in row-major order.
    let layouts: [Layout<'_>; 3] = [
        &[("B1", "=SUM(D6:D10)"), ("D5", "=SEQUENCE(3)")],
        &[("B1", "=INDEX(D5:D10,2)"), ("D5", "=SEQUENCE(3)")],
        &[("B1", "=D6+SUM(D5:D10)"), ("D5", "=SEQUENCE(3)")],
    ];
    for cells in layouts {
        for threads in [1, 4] {
            let (t, row_major) = loaded_beside_row_major(cells, threads);
            let what = format!("{cells:?} on {threads} threads");
            same_values(&t, &row_major, &what);
            assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{what}");
            assert!(t.calc.row_major_only, "{what}");
            if threads > 1 {
                assert!(t.calc.worker_levels > 0, "{what}: no level on workers");
            }
        }
    }
}

#[test]
fn readers_after_an_anchor_reading_into_its_array_from_outside_use_worker_copies() {
    // A 400 x 3 array anchored at A2, and a thousand readers after it of the columns it fills
    // to the right of its anchor (as lookups over a FILTER's columns read them): their blocks
    // don't hold the anchor. They read Z1 too (times nothing), so each edit of Z1 calculates
    // them in the round that lays the new array out. Readers after the anchor note no pair (the
    // check looks at none for them), so the workers serve the repeated blocks from their copies,
    // and the edits are kept.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        let mut cells = vec![("Z1".to_string(), "1".to_string()), ("A2".to_string(), "=SEQUENCE(400,3,Z1)".to_string())];
        for r in 2..=1001 {
            let block = if r % 2 == 0 { "$B$2:$B$401" } else { "$B$2:$C$401" };
            cells.push((format!("E{r}"), format!("=SUM({block})+ROW()+$Z$1*0")));
        }
        t.load(cells);
        t.calc.row_major_only = false;
        t
    };
    let mut row_major = book(4, Some(0));
    let mut t = book(4, None);
    for z in ["2", "3", "4"] {
        edit(&mut row_major, &[("Z1", z)]);
        edit(&mut t, &[("Z1", z)]);
        let what = format!("Z1 = {z}");
        same_values(&t, &row_major, &what);
        assert!(t.calc.redo_reasons.is_empty(), "{what}: {:?}", t.calc.redo_reasons);
        assert!(t.calc.plans_made > 0 && !t.calc.row_major_only, "{what}");
        assert!(t.calc.worker_levels > 0, "{what}: no level on workers");
        assert!(t.calc.copies_read > 0, "{what}: no read served from a worker's copy");
        assert_eq!(t.calc.unforced_noted, 0, "{what}: pairs noted for readers after the anchor");
    }
}

#[test]
fn a_reader_before_an_anchor_served_from_a_worker_copy_notes_its_pair() {
    // Five readers above A10's array read B10:B109, which it fills from outside the block, and Z1,
    // so they are calculated in the round that lays the new array out, after it. One
    // worker calculates them in turn: the first reads the block, the second copies it, the rest
    // are served from the copy. Each is before the anchor and notes the array it read without
    // calculating the anchor, the ones served from the copy as the read that made it did, and
    // the edit is calculated again in row-major order.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        let mut cells = vec![("Z1".to_string(), "1".to_string()), ("A10".to_string(), "=SEQUENCE(100,2,Z1)".to_string())];
        cells.extend((1..=5).map(|r| (format!("C{r}"), "=SUM($B$10:$B$109)+ROW()+$Z$1*0".to_string())));
        t.load(cells);
        t.calc.row_major_only = false;
        edit(&mut t, &[("Z1", "2")]);
        t
    };
    let row_major = book(2, Some(0));
    let t = book(2, None);
    same_values(&t, &row_major, "2 threads");
    assert_eq!(t.calc.redo_reasons, vec![Redo::Order]);
    assert!(t.calc.copies_read >= 3, "{} reads served from a copy", t.calc.copies_read);
    assert_eq!(t.calc.unforced_noted, 5, "one pair for each reader");
}

#[test]
fn pairs_past_the_note_limit_send_the_round_back_for_its_budget() {
    // The pairs take entries from the reads a round may note (`Plan::note_limit`). With none
    // allowed, a reader before an anchor that reads the array from outside its block makes the
    // round's reads unknown, and the round is calculated again in row-major order for its
    // budget; a reader whose block holds the anchor notes nothing, and its round is kept.
    let layouts: [(Layout<'_>, Vec<Redo>); 2] =
        [(&[("B1", "=SUM(D6:D10)"), ("D5", "=SEQUENCE(3)")], vec![Redo::Budget]), (&[("B1", "=SUM(D5:D10)"), ("D5", "=SEQUENCE(3)")], vec![])];
    for (cells, want) in layouts {
        for threads in [1, 4] {
            let owned = || cells.iter().map(|(at, input)| (at.to_string(), input.to_string()));
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            t.calc.note_cap = Some(0);
            t.load(owned());
            let mut row_major = T::new().threads(threads);
            row_major.calc.plan_budget = Some(0);
            row_major.load(owned());
            let what = format!("{cells:?} on {threads} threads");
            same_values(&t, &row_major, &what);
            assert_eq!(t.calc.redo_reasons, want, "{what}");
        }
    }
}

#[test]
fn reads_after_the_notes_ran_out_look_up_no_arrays() {
    // B1 comes before the array at D5 and reads D6, one of its cells, three thousand times. With
    // no entry to note in, the first pair makes the round's reads unknown; the reads after it
    // look up no arrays over the cell (no pair could be noted), and the round gives what row-major
    // order gives. With entries to spare every read looks the arrays up.
    let cells = [("B1", "=SUM(MAP(SEQUENCE(3000),LAMBDA(repeat,D6)))+RAND()*0"), ("D5", "=SEQUENCE(3)")];
    let owned = || cells.iter().map(|(at, input)| (at.to_string(), input.to_string()));
    let lookups = |cap: Option<usize>| {
        let mut t = T::new().threads(1);
        t.calc.note_cap = cap;
        crate::recalc::ARRAYS_OVER_LOOKUPS.with(|n| n.set(0));
        t.load(owned());
        (t, crate::recalc::ARRAYS_OVER_LOOKUPS.with(std::cell::Cell::get))
    };
    let (spare, looked_up_spare) = lookups(None);
    let (none_left, looked_up) = lookups(Some(0));
    let mut row_major = T::new().threads(1);
    row_major.calc.plan_budget = Some(0);
    row_major.load(owned());
    same_values(&none_left, &row_major, "no entries");
    same_values(&spare, &row_major, "entries to spare");
    assert!(looked_up_spare >= 3_000, "{looked_up_spare} look-ups with entries to spare");
    assert!(looked_up <= 1, "{looked_up} look-ups with no entries");
    assert!(none_left.calc.plans_made > 0, "the round was planned");
}

#[test]
fn a_formula_reading_two_arrays_again_and_again_notes_each_pair_once() {
    // B1 comes before the arrays at D5 and E5 and reads their cells below the anchors, as a
    // block that holds neither: two pairs, however often it reads them. Reading the block twice,
    // or a few thousand times in a loop (the arrays alternately, by anchor), takes two of the
    // three entries the round may note, so the round is kept (the arrays don't change; H20's
    // does, so unknown reads would send it back for its budget).
    for reader in ["=SUM(D6:E8)+SUM(D6:E8)+$Q$1*0", "=SUM(MAP(SEQUENCE(3000),LAMBDA(repeat,SUM(D6:E8))))+$Q$1*0"] {
        for threads in [1, 4] {
            let book = |plan_budget: Option<usize>| {
                let mut t = T::new().threads(threads);
                t.calc.parallel_min = 1;
                t.calc.note_cap = Some(3);
                t.calc.plan_budget = plan_budget;
                t.load(
                    [("Q1", "1"), ("B1", reader), ("D5", "=SEQUENCE(3)+$Q$1*0"), ("E5", "=SEQUENCE(3)+$Q$1*0"), ("H20", "=SEQUENCE($Q$1)")]
                        .map(|(at, input)| (at.to_string(), input.to_string())),
                );
                t.calc.row_major_only = false;
                edit(&mut t, &[("Q1", "2")]);
                t
            };
            let row_major = book(Some(0));
            let t = book(None);
            let what = format!("{reader} on {threads} threads");
            same_values(&t, &row_major, &what);
            assert!(t.calc.redo_reasons.is_empty(), "{what}: {:?}", t.calc.redo_reasons);
            assert_eq!(t.calc.unforced_noted, 2, "{what}: one pair for each array");
            assert!(t.calc.plans_made > 0 && !t.calc.row_major_only, "{what}");
        }
    }
}

#[test]
fn a_cell_under_two_arrays_read_on_its_own_notes_a_pair_for_each() {
    // C1's array runs down over C2 and B2's across it. A1 refers to both anchors, so the plan
    // puts it after them, and reads C2 alone through INDIRECT, before both anchors in row-major
    // order: one pair for each of the two arrays over that cell, though the read takes the value
    // of one. The two arrays meet, so the round is calculated again in row-major order, and the
    // values are row-major order's on one thread and on four.
    for threads in [1, 4] {
        let book = |plan_budget: Option<usize>| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            t.calc.plan_budget = plan_budget;
            t.load(
                [
                    ("Q1", "1"),
                    ("A1", "=N(INDIRECT(\"C2\"))+0*IFERROR(B2,0)+0*IFERROR(C1,0)"),
                    ("C1", "=SEQUENCE(3,1,Q1)"),
                    ("B2", "=SEQUENCE(1,3,10*Q1)"),
                ]
                .map(|(at, input)| (at.to_string(), input.to_string())),
            );
            t.calc.row_major_only = false;
            edit(&mut t, &[("Q1", "2")]);
            t
        };
        let row_major = book(Some(0));
        let t = book(None);
        let what = format!("{threads} threads");
        same_values(&t, &row_major, &what);
        assert!(t.calc.plans_made > 0, "{what}: planned");
        assert_eq!(t.calc.unforced_noted, 2, "{what}: one pair for each array over C2");
        assert_eq!(t.calc.redo_reasons, [Redo::Overlap], "{what}");
    }
}

#[test]
fn single_cell_reads_give_the_same_values_and_pairs_on_any_thread_count() {
    // One-cell reads, each by its own path: B1 reads E6 of D5's stored array through INDIRECT
    // (so the plan doesn't hold it back) before D5 is calculated, with no array laid out yet on
    // one thread; B2 reads it so after G1's array is laid out; C3 reads it by reference, held
    // back until D5's array is laid out; A10 reads it after D5; A11 reads a constant. Each edit
    // of Z1 changes D5's array, so the three readers before D5 note a pair each. The values are
    // those of row-major order, and the pairs the same on one, two and four threads.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(1);
        t.calc.plan_budget = plan_budget;
        t.load(
            [
                ("Z1", "1"),
                ("W1", "7"),
                ("G1", "=SEQUENCE(1,2,Z1)"),
                ("B1", "=N(INDIRECT(\"E6\"))+$Z$1*0"),
                ("B2", "=N(INDIRECT(\"E6\"))+$Z$1*0"),
                ("C3", "=E6+$Z$1*0"),
                ("D5", "=SEQUENCE(3,2,Z1)"),
                ("A10", "=E6*10+$Z$1*0"),
                ("A11", "=W1*2+$Z$1*0"),
            ]
            .map(|(at, input)| (at.to_string(), input.to_string())),
        );
        t
    };
    let mut row_major = book(1, Some(0));
    let mut books: Vec<(u32, T)> = [1, 2, 4].into_iter().map(|threads| (threads, book(threads, None))).collect();
    for z in ["2", "3", "4"] {
        edit(&mut row_major, &[("Z1", z)]);
        let z: f64 = z.parse().unwrap();
        // D5:E7 holds z, z+1, ..., z+5 by rows: E6 is z+3.
        assert_eq!(row_major.num("B1"), z + 3.0, "Z1 = {z}");
        assert_eq!(row_major.num("C3"), z + 3.0, "Z1 = {z}");
        assert_eq!(row_major.num("A10"), (z + 3.0) * 10.0, "Z1 = {z}");
        assert_eq!(row_major.num("A11"), 14.0, "Z1 = {z}");
        let mut pairs = Vec::new();
        for (threads, t) in &mut books {
            t.calc.row_major_only = false;
            edit(t, &[("Z1", &z.to_string())]);
            let what = format!("Z1 = {z} on {threads} threads");
            same_values(t, &row_major, &what);
            assert!(t.calc.plans_made > 0, "{what}: planned");
            if *threads > 1 {
                assert!(t.calc.worker_levels > 0, "{what}: no level on workers");
            }
            pairs.push(t.calc.unforced_noted);
        }
        assert_eq!(pairs, [3, 3, 3], "Z1 = {z}: one pair for each reader before the anchor, on 1, 2 and 4 threads");
    }
}

#[test]
fn a_formula_handed_back_returns_the_entries_its_pairs_took() {
    // The round may note two entries. On one worker B1 notes the arrays at D5 and E5 and runs
    // out at F5's, so it is handed back; the two entries its pairs took go back to the worker,
    // and B2 after it notes its two pairs from them and is kept there. Without them B2 would
    // find none left and be handed back too.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.note_cap = Some(2);
        t.calc.plan_budget = plan_budget;
        t.load(
            [
                ("Q1", "1"),
                ("B1", "=SUM(D6:F8)+$Q$1*0"),
                ("B2", "=SUM(D6:E8)+$Q$1*0"),
                ("D5", "=SEQUENCE(3)+$Q$1*0"),
                ("E5", "=SEQUENCE(3)+$Q$1*0"),
                ("F5", "=SEQUENCE(3)+$Q$1*0"),
            ]
            .map(|(at, input)| (at.to_string(), input.to_string())),
        );
        t.calc.row_major_only = false;
        edit(&mut t, &[("Q1", "2")]);
        t
    };
    let row_major = book(2, Some(0));
    let t = book(2, None);
    same_values(&t, &row_major, "2 threads");
    assert!(t.calc.worker_levels > 0, "no level on workers");
    assert_eq!(t.calc.handed_back, 1, "only B1 was handed back");
}

#[test]
fn a_reader_after_an_anchor_calculated_early_through_one_before_it_is_sent_back() {
    // E20 reads B12, a cell of A10's array, without the anchor; it comes after A10 in row-major
    // order and notes no pair. C1, before A10, reads E20: row-major order calculates E20 inside
    // C1, before A10, and E20 reads B12 as stored, where the plan calculates it after A10's new
    // array. The check decides a reader after the anchor by the row-major order, which sends
    // the round back.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        t.load(
            [("Z1", "1"), ("A10", "=SEQUENCE(5,2,Z1)"), ("C1", "=E20+$Z$1*0"), ("E20", "=B12+$Z$1*0")]
                .map(|(at, input)| (at.to_string(), input.to_string())),
        );
        t.calc.row_major_only = false;
        edit(&mut t, &[("Z1", "2")]);
        t
    };
    let row_major = book(1, Some(0));
    for threads in [1, 4] {
        let t = book(threads, None);
        let what = format!("{threads} threads");
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{what}");
        same_values(&t, &row_major, &what);
    }
}

#[test]
fn a_reader_of_a_cell_an_array_no_longer_covers_is_kept() {
    // The array at D5 shrinks from three rows to two. B1 reads D7, which only its stored spill
    // covered: in the pass that cell reads as stored before the array is laid out and after, so
    // the order doesn't matter to B1 and the round is kept; the next round reads it cleared.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        t.load([("Q1", "3"), ("B1", "=D7+$Q$1*0"), ("D5", "=SEQUENCE($Q$1)")].map(|(at, input)| (at.to_string(), input.to_string())));
        t.calc.row_major_only = false;
        edit(&mut t, &[("Q1", "2")]);
        t
    };
    let row_major = book(1, Some(0));
    assert_eq!(row_major.num("B1"), 0.0);
    for threads in [1, 4] {
        let t = book(threads, None);
        let what = format!("{threads} threads");
        same_values(&t, &row_major, &what);
        assert!(t.calc.redo_reasons.is_empty(), "{what}: {:?}", t.calc.redo_reasons);
        assert!(t.calc.plans_made > 0 && !t.calc.row_major_only, "{what}");
    }
}

#[test]
fn a_spill_reference_read_through_a_name_is_noted_for_the_check() {
    // A1 reads X5's spill range through INDIRECT and the name Seq, and W9, so the plan puts it
    // after W9 and after A3; in row-major order it comes first and calculates X5 on demand,
    // laying the array out before A3 reads Y5. The plan lays it out after A3 (A3 doesn't wait
    // for X5): A3 reads Y5 before the array in the plan and after it in row-major order. The
    // check sees that only through the read of the spill range noted for A1
    // (`PassHost::spill_range`), and sends the round back.
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        t.name("Seq", "Sheet1!$X$5#");
        t.load(
            [("Q1", "1"), ("A1", "=COLUMNS(INDIRECT(\"Seq\"))+W9+$Q$1*0"), ("A3", "=Y5+$Q$1*0"), ("X5", "=SEQUENCE(1,$Q$1)"), ("W9", "=$Q$1+1")]
                .map(|(at, input)| (at.to_string(), input.to_string())),
        );
        t.calc.row_major_only = false;
        edit(&mut t, &[("Q1", "3")]);
        t
    };
    let row_major = book(1, Some(0));
    assert_eq!((row_major.num("A1"), row_major.num("A3")), (7.0, 2.0));
    for threads in [1, 4] {
        let t = book(threads, None);
        let what = format!("{threads} threads");
        assert_eq!(t.calc.redo_reasons, vec![Redo::Order], "{what}");
        same_values(&t, &row_major, &what);
    }
}

/// `cells` loaded (`{RxC}=formula` enters a legacy array formula over R rows and C columns from
/// its cell), then `steps` (a cell and its input, or `full` for a full recalculation), with the
/// hold on row-major order lifted before each, on `threads` with worker chunks of one formula:
/// the workbook after each step, with the reasons its rounds were sent back and the pairs noted.
fn pairs_by_step(
    cells: Layout<'_>,
    steps: Layout<'_>,
    threads: u32,
    note_cap: Option<usize>,
    plan_budget: Option<usize>,
) -> Vec<(T, Vec<Redo>, usize)> {
    let mut t = T::new().threads(threads);
    t.calc.parallel_min = 1;
    t.calc.chunk = Some(1);
    t.calc.note_cap = note_cap;
    t.calc.plan_budget = plan_budget;
    for &(at, input) in cells {
        let cell = match input.strip_prefix('{').and_then(|rest| rest.split_once('}')) {
            Some((size, formula)) => {
                let (rows, cols) = size.split_once('x').unwrap();
                let (rows, cols): (u32, u32) = (rows.parse().unwrap(), cols.parse().unwrap());
                let mut f = Formula::new(formula);
                f.array = Some(gridcraft_core::RangeRef::new(c(at), gridcraft_core::CellRef::new(c(at).row + rows - 1, c(at).col + cols - 1)));
                Cell::formula(f)
            }
            None if input.starts_with('=') => Cell::formula(Formula::new(input)),
            None => Cell::value(gridcraft_core::parse::parse_input(input, t.wb.date_system).value),
        };
        t.wb.sheet_mut(0).unwrap().cells.set(c(at), cell);
    }
    t.calc.recalc_all(&mut t.wb);
    let mut out = Vec::new();
    for &(at, input) in steps {
        t.calc.row_major_only = false;
        if at == "full" {
            t.calc.recalc_all(&mut t.wb);
        } else {
            edit(&mut t, &[(at, input)]);
        }
        let reasons = t.calc.redo_reasons.clone();
        let pairs = t.calc.unforced_noted;
        out.push((T { wb: t.wb.clone(), calc: t.calc.clone() }, reasons, pairs));
    }
    out
}

#[test]
fn pairs_are_noted_the_same_on_any_thread_count() {
    // B2 reads A3 through INDIRECT, so on one thread it calculates A3 on demand, before C2 reads
    // B3:C4, a block A3's array reaches into from outside: C2 reads the array laid out and notes
    // the pair. On a worker B2 is handed back and C2 reads A3's stored spill, the anchor not yet
    // calculated in its view; it notes the pair by the stored area, so the round's pairs, and
    // the entries they take of the two the round may note, don't depend on the thread count or
    // on which worker ran first. D5's array changes, so the notes decide the round.
    let first: Layout<'_> =
        &[("E1", "1"), ("B2", "=N(INDIRECT(\"A3\"))"), ("C2", "=SUM(INDIRECT(\"B3:C4\"))"), ("A3", "=SEQUENCE(1,2)+E1*0"), ("D5", "=SEQUENCE(E1+1)")];
    let first_steps: Layout<'_> = &[("E1", "2"), ("E1", "3"), ("E1", "1")];
    // C2 reads a block A3's array reaches into from outside, or one cell of a legacy array
    // entered over A3:C3, and B2 is edited to read three cells through INDIRECT. The last edits
    // of E1 leave every array as stored.
    let later_steps: Layout<'_> = &[
        ("E1", "0"),
        ("full", ""),
        ("B2", "=N(INDIRECT(\"A1\"))+N(INDIRECT(\"A3\"))+N(INDIRECT(\"A2\"))"),
        ("E1", "2"),
        ("E1", "0"),
        ("E1", "0"),
        ("full", ""),
    ];
    let block: Layout<'_> = &[("E1", "1"), ("C2", "=SUM(INDIRECT(\"B3:C4\"))"), ("A3", "=SEQUENCE(E1+1,2)")];
    let legacy: Layout<'_> = &[("E1", "1"), ("C2", "=N(INDIRECT(\"C3\"))+0"), ("A3", "{1x3}=SEQUENCE(1,E1+1)")];
    let books: [(&str, Layout<'_>, Layout<'_>, Option<usize>); 3] =
        [("block, two entries", first, first_steps, Some(2)), ("block", block, later_steps, None), ("legacy cell", legacy, later_steps, None)];
    for (name, cells, steps, note_cap) in books {
        let row_major = pairs_by_step(cells, steps, 1, note_cap, Some(0));
        let one = pairs_by_step(cells, steps, 1, note_cap, None);
        for (i, ((o, _, _), (r, _, _))) in one.iter().zip(&row_major).enumerate() {
            same_values(o, r, &format!("{name}: step {i} on 1 thread"));
        }
        for threads in [2, 4] {
            for repeat in 0..20 {
                let many = pairs_by_step(cells, steps, threads, note_cap, None);
                for (i, ((m, m_redo, m_pairs), ((_, o_redo, o_pairs), (r, _, _)))) in many.iter().zip(one.iter().zip(&row_major)).enumerate() {
                    let what = format!("{name}: step {i} on {threads} threads, run {repeat}");
                    same_values(m, r, &what);
                    assert_eq!(m_redo, o_redo, "{what}: rounds sent back");
                    assert_eq!(m_pairs, o_pairs, "{what}: pairs noted");
                }
            }
        }
        let pairs: Vec<usize> = one.iter().map(|x| x.2).collect();
        let redo: Vec<&Vec<Redo>> = one.iter().map(|x| &x.1).collect();
        if note_cap == Some(2) {
            // B2's read, C2's read and C2's pair take three entries, past the two: each edit is
            // sent back for its budget, on any thread count.
            assert!(redo.iter().all(|r| **r == vec![Redo::Budget]), "{name}: {redo:?}");
        } else {
            assert!(pairs.iter().any(|&n| n > 0), "{name}: no pair noted");
        }
    }
}

// ---- One thread without random draws ------------------------------------------------------

/// A step of a script run on several thread counts: a cell entered, a defined name's definition
/// changed with the graph rebuilt and everything calculated (as the name commands do), or changed
/// with only the dynamic formulas calculated (as a hidden name's update does when no formula
/// mentions the name).
enum Edit {
    Cell(&'static str, &'static str),
    NameRebuilt(&'static str, &'static str),
    NameHidden(&'static str, &'static str),
}

impl T {
    fn define(&mut self, name: &str, formula: &str) {
        if let Some(n) = self.wb.names_mut().iter_mut().find(|n| n.name == name) {
            n.formula = formula.into();
        }
    }
    fn apply(&mut self, edit: &Edit) {
        match *edit {
            Edit::Cell(at, input) => self.set(at, input),
            Edit::NameRebuilt(name, formula) => {
                self.define(name, formula);
                self.calc.recalc_all(&mut self.wb);
            }
            Edit::NameHidden(name, formula) => {
                self.define(name, formula);
                self.calc.cells_changed(&mut self.wb, &[]);
            }
        }
    }
}

/// D3 reads G1:H1 through OFFSET while A1 is 1, and G1:H2 while it is 2, which closes a loop with
/// H2: a value edit of A1 sends a planned round back and sets the hold (`row_major_only`) without
/// changing a formula. K1 and M1 read the cell the name `Pick` gives through INDIRECT, which no
/// formula's references see; K1 also reads K9, so a plan calculates it a level after M1, while
/// row-major order calculates it first.
fn toggled_cycle(threads: u32) -> T {
    let mut t = T::new().threads(threads).planning_as_shipped();
    t.calc.parallel_min = 1;
    t.calc.chunk = Some(1);
    t.name("Pick", "Sheet1!$Z$1");
    let mut cells: Vec<(String, String)> = vec![
        ("A1".into(), "1".into()),
        ("D3".into(), "=SUM(OFFSET(G1,0,0,A1,2))".into()),
        ("H2".into(), "=COUNT(B2:F3)+1".into()),
        ("K9".into(), "=A1+1".into()),
        ("K1".into(), "=N(INDIRECT(\"Pick\"))+K9*0".into()),
        ("M1".into(), "=N(INDIRECT(\"Pick\"))".into()),
    ];
    cells.extend((1..=20).map(|r| (format!("Z{r}"), format!("{}", r * 7))));
    t.load(cells);
    t
}

#[test]
fn one_thread_makes_no_plan_while_the_workbook_cannot_draw_and_matches_several_threads() {
    let script = [
        Edit::Cell("A1", "2"),
        Edit::Cell("A1", "1"),
        Edit::Cell("K9", "=A1+5"),
        Edit::Cell("B5", "=SEQUENCE(A1+2)"),
        Edit::Cell("A1", "2"),
        Edit::NameRebuilt("Pick", "Sheet1!$Z$2"),
        Edit::NameHidden("Pick", "Sheet1!$Z$3"),
    ];
    let mut books: Vec<T> = [1, 2, 4].into_iter().map(toggled_cycle).collect();
    let mut several_held = false;
    for (step, edit) in std::iter::once(None).chain(script.iter().map(Some)).enumerate() {
        for t in &mut books {
            if let Some(edit) = edit {
                t.apply(edit);
            }
        }
        let (one, rest) = books.split_first().unwrap();
        assert_eq!(one.calc.plans_made, 0, "step {step}: one thread planned");
        assert!(!one.calc.row_major_only, "step {step}: one thread set the hold");
        for many in rest {
            same_values(one, many, &format!("step {step}: one thread and {:?}", many.calc.threads));
            several_held |= many.calc.row_major_only;
        }
        if step == 0 {
            assert!(rest.iter().all(|t| t.calc.plans_made > 0), "several threads plan the load");
        }
    }
    assert!(several_held, "several threads never held: the script doesn't send a round back");
}

#[test]
fn random_draws_match_on_one_two_and_four_threads_as_the_workbook_starts_and_stops_drawing() {
    // Each step on 1, 2 and 4 threads, as outside tests: rounds sent back while the workbook can't
    // draw (A1 = 2), so several threads hold and one doesn't; then `Pick` made to draw through a
    // rebuild that finds the same formulas (the hold survives it), a formula edit, `Pick` back,
    // the first random cell formula, RANDARRAY and a reader of it, the draws removed again, and
    // `Pick` made to draw through a hidden-name update.
    let draw = "OFFSET(Sheet1!$Z$1,RANDBETWEEN(0,19),0)";
    let script = [
        Edit::Cell("A1", "2"),
        Edit::Cell("A1", "1"),
        Edit::NameRebuilt("Pick", draw),
        Edit::Cell("K9", "=A1+3"),
        Edit::Cell("A1", "2"),
        Edit::Cell("A1", "1"),
        Edit::NameRebuilt("Pick", "Sheet1!$Z$1"),
        Edit::Cell("A1", "2"),
        Edit::Cell("A1", "1"),
        Edit::Cell("R1", "=RANDBETWEEN(1,1000)+K9*0"),
        Edit::Cell("T1", "=RAND()"),
        Edit::Cell("P1", "=RANDARRAY(3)+K9*0"),
        Edit::Cell("Q1", "=SUM(P1#)"),
        Edit::Cell("A1", "2"),
        Edit::Cell("A1", "1"),
        Edit::NameHidden("Pick", draw),
        Edit::Cell("A1", "3"),
        Edit::NameHidden("Pick", "Sheet1!$Z$1"),
        Edit::Cell("R1", "5"),
        Edit::Cell("T1", ""),
        Edit::Cell("P1", "=SEQUENCE(3)"),
        Edit::Cell("A1", "2"),
        Edit::Cell("A1", "1"),
        Edit::NameHidden("Pick", draw),
        Edit::Cell("A1", "4"),
    ];
    let mut books: Vec<T> = [1, 2, 4].into_iter().map(toggled_cycle).collect();
    let mut picks = std::collections::HashSet::new();
    let mut one_planned_draws = false;
    for (step, edit) in script.iter().enumerate() {
        // The hold before the step, to show the script reaches the case it is about.
        let held: Vec<bool> = books.iter().map(|t| t.calc.row_major_only).collect();
        for t in &mut books {
            t.apply(edit);
        }
        let (one, rest) = books.split_first().unwrap();
        for many in rest {
            same_values(one, many, &format!("step {step}: one thread and {:?}", many.calc.threads));
        }
        if step == 2 {
            assert_eq!(held, [false, true, true], "before `Pick` draws, several threads hold and one doesn't");
            one_planned_draws = one.calc.plans_made > 0;
        }
        if step == 23 {
            assert_eq!(held, [false, true, true], "before `Pick` draws again, several threads hold and one doesn't");
        }
        if matches!(step, 2 | 3 | 15 | 16 | 23 | 24) {
            picks.insert((one.num("K1") as u64, one.num("M1") as u64));
        }
        if step == 22 {
            assert_eq!(one.calc.plans_made, 0, "the draws removed, one thread plans no more");
        }
    }
    assert!(picks.len() > 1, "`Pick` drew the same cells at every step: {picks:?}");
    assert!(one_planned_draws, "one thread plans once the workbook can draw");
}

#[test]
fn a_function_that_draws_passed_by_name_counts_as_a_draw() {
    // RANDARRAY given to MAKEARRAY by name draws without a call of it in the tree: one thread
    // plans for it as several do, and the two cells draw in the same order on any thread count.
    let book = |threads: u32| {
        let mut t = T::new().threads(threads).planning_as_shipped();
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(1);
        let cells = [("K9", "=E1+1"), ("A1", "=SUM(MAKEARRAY(1,1,RANDARRAY))+K9*0"), ("C1", "=SUM(MAKEARRAY(1,1,RANDARRAY))")];
        t.load(cells.map(|(a, f)| (a.to_string(), f.to_string())));
        t
    };
    let one = book(1);
    assert_ne!(one.num("A1"), one.num("C1"), "two draws");
    for threads in [2, 4] {
        let many = book(threads);
        same_values(&one, &many, &format!("{threads} threads"));
        // A kept plan: C1 draws before A1, a level after it, where row-major order has A1 first.
        assert_eq!(many.calc.rounds_redone_in_row_major, 0, "{threads} threads");
    }
    assert!(one.calc.plans_made > 0, "one thread planned the draws");
}

#[test]
fn a_round_too_small_for_worker_threads_makes_no_plan_while_the_workbook_cannot_draw() {
    // Four threads, as outside tests, and a level needs four formulas to go to worker threads.
    // Editing A1 dirties its readers in column B, and nothing else. Three can't make a level for
    // the workers: while the workbook can't draw random numbers, their round runs in row-major
    // order without a plan, as on one thread. Four can, and their round is planned. A reader that
    // mentions RAND (without drawing) makes the workbook able to draw, and then three are planned
    // too. Each matches the row-major oracle.
    let book = |readers: u32, rand: bool, plan_budget: Option<usize>| {
        let mut t = T::new().threads(4).planning_as_shipped();
        t.calc.parallel_min = 4;
        t.calc.plan_budget = plan_budget;
        let mut cells = vec![("A1".to_string(), "1".to_string())];
        cells.extend((1..=readers).map(|r| {
            let draw = if rand && r == 1 { "+IF(FALSE,RAND(),0)" } else { "" };
            (format!("B{r}"), format!("=$A$1+ROW(){draw}"))
        }));
        t.load(cells);
        t
    };
    for (readers, rand, plans) in [(3, false, 0), (4, false, 1), (3, true, 1), (4, true, 1)] {
        let what = format!("{readers} readers, RAND mentioned: {rand}");
        let mut t = book(readers, rand, None);
        let mut row_major = book(readers, rand, Some(0));
        edit(&mut t, &[("A1", "2")]);
        edit(&mut row_major, &[("A1", "2")]);
        assert_eq!((t.calc.rounds_calculated, t.calc.formulas_calculated), (1, readers as usize), "{what}: one round of the readers");
        assert_eq!(t.calc.plans_made, plans, "{what}: plans made");
        same_values(&t, &row_major, &what);
        assert_eq!(t.num("B3"), 5.0, "{what}");
    }
}

#[test]
fn the_search_for_rand_answers_as_comparing_every_four_bytes() {
    // The search through names' texts skips blocks of 32 bytes with no `R` or `r`; it answers
    // as comparing each four bytes ignoring ASCII case, as lower-casing ASCII and as lower-casing
    // Unicode then searching for `rand` do.
    let check = |text: &str, expected: Option<bool>| {
        let windows = text.as_bytes().windows(4).any(|w| w.eq_ignore_ascii_case(b"RAND"));
        let ascii = text.to_ascii_lowercase().contains("rand");
        let unicode = text.to_lowercase().contains("rand");
        let found = crate::recalc::mentions_rand(text);
        assert_eq!(found, windows, "{text:?}: against the four-byte comparison");
        assert_eq!(found, ascii, "{text:?}: against ASCII lower-casing");
        assert_eq!(found, unicode, "{text:?}: against Unicode lower-casing");
        if let Some(expected) = expected {
            assert_eq!(found, expected, "{text:?}");
        }
    };
    let cases: &[(&str, bool)] = &[
        ("", false),
        ("RAND()", true),
        ("1+RAND()*2", true),
        ("1+RAND", true),
        ("rand", true),
        ("RaNd", true),
        ("rAND", true),
        ("=1+RAN", false),
        ("SUM(A1)+R", false),
        ("R", false),
        ("RRAND", true),
        ("RARAND", true),
        ("RANRAND", true),
        ("BRAND", true),
        ("OPERAND", true),
        ("RÄND", false),
        ("ÄRAND", true),
        ("RAND€", true),
        ("R€AND", false),
        ("RA€ND", false),
        ("\u{1D11E}RAND", true),
        ("RAN\u{1D11E}D", false),
        ("\u{212A}RAND\u{212A}", true),
        ("RA\u{212A}ND", false),
        ("ıRAND", true),
        ("RAıND", false),
        ("İrand", true),
        ("RAİND", false),
        ("rANİ", false),
        ("ŉRAND", true),
        ("RAŉD", false),
        ("Rẚnd", false),
    ];
    for (text, expected) in cases {
        check(text, Some(*expected));
    }
    // Long texts without a match, `R` and `Ran` included, then one ending in a match.
    let quiet = "IF(OR(A1>0,B2<3),VLOOKUP(Data!$A$1,Range,2,FALSE),0)+".repeat(2_000);
    check(&quiet, Some(false));
    check(&format!("{quiet}Ran"), Some(false));
    check(&format!("{quiet}rand"), Some(true));
    // A match or three letters of one at each offset around the blocks' edges.
    for offset in 0..100 {
        let pad = "x".repeat(offset);
        check(&format!("{pad}RAND"), Some(true));
        check(&format!("{pad}RAND{pad}"), Some(true));
        check(&format!("{pad}RAN"), Some(false));
        check(&format!("{pad}éRAN"), Some(false));
        check(&format!("{pad}é{pad}rand"), Some(true));
    }
    // Pseudo-random texts over the letters of RAND in both cases, a filler and a two-byte letter.
    let alphabet = ['R', 'a', 'N', 'd', 'r', 'A', 'n', 'D', 'x', 'é'];
    let mut state = 0x9E37_79B9_7F4A_7C15_u64;
    let (mut matched, mut missed) = (0, 0);
    for _ in 0..5_000 {
        let len = (xorshift(&mut state) % 90) as usize;
        let text: String = (0..len).map(|_| alphabet[(xorshift(&mut state) % alphabet.len() as u64) as usize]).collect();
        check(&text, None);
        if crate::recalc::mentions_rand(&text) {
            matched += 1;
        } else {
            missed += 1;
        }
    }
    assert!(matched > 100 && missed > 100, "matched {matched}, missed {missed}");
}

#[test]
fn a_read_whose_notes_hand_the_formula_back_serves_nothing_from_the_copy() {
    // Row 10 holds A10's array, laid out in the first level. In the second, one worker takes the
    // three readers of C9:C200 (a chunk of 100), whose cell C10 the array reaches into from
    // outside. Each reader comes before the anchor, so each read notes the pair, and the round
    // may note two: the third reader's read, served from the copy the second made, finds no entry
    // left and hands its formula back. That read gives no values, and none is counted as served
    // from the copy.
    let book = |threads: u32, cap: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.chunk = Some(100);
        t.calc.note_cap = cap;
        let mut cells: Vec<(String, String)> = (11..=200).map(|r| (format!("C{r}"), r.to_string())).collect();
        cells.push(("A10".to_string(), "=SEQUENCE(1,5)".to_string()));
        for r in 1..=3 {
            cells.push((format!("B{r}"), "=SUMPRODUCT($C$9:$C$200)+$A$10*0".to_string()));
        }
        t.load(cells);
        t
    };
    let one = book(1, None);
    let t = book(2, Some(2));
    assert!(t.calc.worker_levels > 0, "the level ran on workers");
    assert_eq!(t.calc.handed_back, 1, "handed back");
    assert_eq!(t.calc.copies_read, 0, "reads served from the copy");
    assert_eq!(t.calc.blocked_reads, 0, "cell reads and draws asked for after the hand-back");
    same_values(&t, &one, "two threads");
    // With one entry, the first reader's read takes it, and the second and third readers' reads
    // find none left at their ends and hand their formulas back: no copy is made for either, so
    // none is served, and the formulas ask for no cell read after it. That the fence drops the
    // values of such a read is pinned by `eval::fence::tests::a_read_that_hands_the_formula_back_passes_nothing_on`.
    let t = book(2, Some(1));
    assert!(t.calc.worker_levels > 0, "one entry: the level ran on workers");
    assert!(t.calc.handed_back > 0, "one entry: handed back");
    assert_eq!(t.calc.copies_read, 0, "one entry: reads served from the copy");
    assert_eq!(t.calc.blocked_reads, 0, "one entry: cell reads and draws asked for after the hand-back");
    same_values(&t, &one, "two threads, one entry");
}

#[test]
fn a_cell_read_whose_note_hands_the_formula_back_gives_calc() {
    // Row 10 holds A10's array, laid out in the first level. In the second, a worker takes the
    // three readers of C10, a cell of the array each reads on its own through INDIRECT before
    // the anchor, so each read notes the cell and then the pair. The round may note one entry:
    // the reads find none left for the pair and hand their formulas back (all three here). Those
    // reads give `#CALC!`, not the array's value.
    let book = |threads: u32, cap: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.note_cap = cap;
        let mut cells = vec![("A10".to_string(), "=SEQUENCE(1,5)".to_string())];
        for r in 1..=3 {
            cells.push((format!("B{r}"), "=N(INDIRECT(\"C10\"))+$A$10*0".to_string()));
        }
        t.load(cells);
        t
    };
    let one = book(1, None);
    let t = book(2, Some(1));
    assert!(t.calc.worker_levels > 0, "the level ran on workers");
    assert_eq!(t.calc.handed_back, 3, "handed back");
    assert_eq!(t.calc.blocked_reads, 0, "cell reads that gave a value after the hand-back");
    same_values(&t, &one, "two threads, one entry");
}

#[test]
fn a_stored_cell_read_whose_note_hands_the_formula_back_gives_calc() {
    // A10's array was laid out by the load and is stored over A10:E10. Z1's edit makes A10 and
    // the three readers of C10 dirty again in one level, with no array laid out in it yet (no
    // `extra` cell) or with X1's laid out in the level before (`extra`). Each reader comes
    // before the anchor, which is still pending when the reader runs, so its read of C10 as
    // stored notes the cell and then the pair. The round may note one entry: the reads find none
    // left for the pair and hand their formulas back. Those reads give `#CALC!`, not the value
    // stored in C10.
    for extra in [false, true] {
        let book = |threads: u32, cap: Option<usize>| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            let after = if extra { "$X$1" } else { "$Z$1" };
            let mut cells = vec![("Z1".to_string(), "1".to_string()), ("A10".to_string(), format!("=SEQUENCE(1,5)+{after}*0"))];
            if extra {
                cells.push(("X1".to_string(), "=SEQUENCE(1,2)+$Z$1*0".to_string()));
            }
            for r in 1..=3 {
                cells.push((format!("B{r}"), format!("=N(INDIRECT(\"C10\"))+{after}*0")));
            }
            t.load(cells);
            // As after a formula edit, so that the edit's pass plans (see `Calc::row_major_only`).
            t.calc.row_major_only = false;
            t.calc.note_cap = cap;
            t.set("Z1", "2");
            t
        };
        let what = if extra { "an array laid out before the level" } else { "no array laid out" };
        let one = book(1, None);
        let t = book(2, Some(1));
        assert!(t.calc.worker_levels > 0, "{what}: the level ran on workers");
        assert!(t.calc.handed_back > 0, "{what}: handed back");
        assert_eq!(t.calc.blocked_reads, 0, "{what}: cell reads that gave a value after the hand-back");
        same_values(&t, &one, what);
    }
}

#[test]
fn a_python_array_stored_over_a_cell_read_before_its_anchor_is_noted() {
    // C1, a Python cell, laid out C1:C3 in the last pass, and is calculated again with a new
    // result. A1 reads C2 alone through INDIRECT, so the plan doesn't hold it back, and comes
    // before C1 in row-major order: the pair is noted from the area the workbook stores for C1's
    // array, as for a formula's. The values are row-major order's.
    for threads in [1, 4] {
        let book = |plan_budget: Option<usize>| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            t.calc.chunk = Some(1);
            t.calc.plan_budget = plan_budget;
            t.load([("Z1", "1"), ("A1", "=N(INDIRECT(\"C2\"))+$Z$1*0")].map(|(at, input)| (at.to_string(), input.to_string())));
            let python = plant_python(&mut t, "C1", Value::Number(1.0), column(&[1.0, 2.0, 3.0]));
            t.calc.cells_changed(&mut t.wb, &[python]);
            t.calc.row_major_only = false;
            let python = plant_python(&mut t, "C1", Value::Number(1.0), column(&[4.0, 5.0, 6.0]));
            let z = put(&mut t, "Z1", "2");
            t.calc.cells_changed(&mut t.wb, &[python, z]);
            t
        };
        let row_major = book(Some(0));
        let t = book(None);
        let what = format!("{threads} threads");
        same_values(&t, &row_major, &what);
        assert_eq!(t.num("C2"), 5.0, "{what}: laid out anew");
        assert!(t.calc.plans_made > 0, "{what}: planned");
        assert_eq!(t.calc.unforced_noted, 1, "{what}: the pair of A1 and C1's stored array");
    }
}

#[test]
fn a_sum_of_constants_does_not_depend_on_an_array_elsewhere_on_the_sheet() {
    // B1 adds up a block of constants too large to walk within a formula's work; the remembered
    // sum gives 1. A1's array and A9's are on the same sheet, outside the block: whether one is
    // laid out before B1 (A1 first in row-major order, A9 in an earlier level) changes nothing,
    // on one thread or four, planned or in row-major order.
    let books: [&[(&str, &str)]; 3] = [
        &[("A1", "=SEQUENCE(2)"), ("B1", "=SUM(C2:XFC1048575)"), ("XFC1048575", "1")],
        &[("A8", "=1+1"), ("A1", "=SEQUENCE(A8)"), ("B1", "=SUM(C2:XFC1048575)"), ("XFC1048575", "1")],
        &[("A9", "=SEQUENCE(2)"), ("A8", "=1+1"), ("B1", "=SUM(C2:XFC1048575)+A8*0"), ("XFC1048575", "1")],
    ];
    for cells in books {
        for (threads, budget) in [(1, Some(0)), (1, None), (4, None)] {
            let mut t = T::new().threads(threads);
            t.calc.plan_budget = budget;
            t.calc.parallel_min = 1;
            t.calc.chunk = Some(1);
            t.load(cells.iter().map(|(a, b)| (a.to_string(), b.to_string())));
            assert_eq!(t.get("B1"), Value::Number(1.0), "{cells:?}: {threads} threads, plan budget {budget:?}");
        }
    }
}

#[test]
fn block_reads_after_the_notes_ran_out_look_up_no_arrays() {
    // B1 comes before the array at D5 and reads D6:D7, two of its cells, as a block that doesn't
    // hold the anchor, three thousand times: when loading, with the array laid out in the round,
    // and after an edit of Q1, with its stored area too. With no entry to note in, the first
    // pair makes the round's reads unknown; the reads after it look up no arrays reaching into
    // the block (no pair could be noted), and the round gives what row-major order gives. With
    // entries to spare every read looks them up.
    let cells = [("Q1", "1"), ("B1", "=SUM(MAP(SEQUENCE(3000),LAMBDA(repeat,SUM(D6:D7))))+RAND()*0+$Q$1*0"), ("D5", "=SEQUENCE(3)+$Q$1*0")];
    let owned = || cells.iter().map(|(at, input)| (at.to_string(), input.to_string()));
    let lookups = |cap: Option<usize>| {
        let mut t = T::new().threads(1);
        t.calc.note_cap = cap;
        crate::recalc::ARRAYS_OVER_LOOKUPS.with(|n| n.set(0));
        t.load(owned());
        let loading = crate::recalc::ARRAYS_OVER_LOOKUPS.with(std::cell::Cell::get);
        t.calc.row_major_only = false;
        crate::recalc::ARRAYS_OVER_LOOKUPS.with(|n| n.set(0));
        edit(&mut t, &[("Q1", "2")]);
        (t, loading, crate::recalc::ARRAYS_OVER_LOOKUPS.with(std::cell::Cell::get))
    };
    let (spare, loading_spare, edit_spare) = lookups(None);
    let (none_left, loading, edited) = lookups(Some(0));
    let mut row_major = T::new().threads(1);
    row_major.calc.plan_budget = Some(0);
    row_major.load(owned());
    edit(&mut row_major, &[("Q1", "2")]);
    same_values(&none_left, &row_major, "no entries");
    same_values(&spare, &row_major, "entries to spare");
    assert!(loading_spare >= 3_000 && edit_spare >= 6_000, "{loading_spare} and {edit_spare} look-ups with entries to spare");
    assert!(loading <= 1 && edited <= 2, "{loading} and {edited} look-ups with no entries");
    assert!(none_left.calc.plans_made > 0, "the round was planned");
}

#[test]
fn two_arrays_stored_across_each_other_send_the_round_back() {
    // A2's array is stored as A2:C2 and B1's as B1:B3, crossing at B2, which holds 2 for both: a
    // state no calculation lays out (one of them would be #SPILL!), but one the xlsx reader keeps
    // for Python spill areas, which it doesn't check. The edit of Q1 lays both out again as
    // stored, so no array of the round changed, yet they meet: the round goes back to row-major
    // order. The check finds that only because it looks for arrays that meet before it returns for
    // a round in which no array changed (see `Calc::agrees_with_row_major`).
    let book = |threads: u32, plan_budget: Option<usize>| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.calc.plan_budget = plan_budget;
        let sheet = t.wb.sheet_mut(0).unwrap();
        sheet.cells.set(c("Q1"), Cell::value(Value::Number(1.0)));
        for (at, input) in [("A2", "=SEQUENCE(1,3)+0*Q1"), ("B1", "=SEQUENCE(3)+0*Q1")] {
            let mut cell = Cell::formula(Formula::new(input));
            cell.value = Value::Number(1.0);
            sheet.cells.set(c(at), cell);
        }
        for (at, v) in [("B2", 2.0), ("C2", 3.0), ("B3", 3.0)] {
            sheet.spill.insert(c(at), Value::Number(v));
        }
        sheet.spill_ranges.insert(c("A2"), gridcraft_core::RangeRef::new(c("A2"), c("C2")));
        sheet.spill_ranges.insert(c("B1"), gridcraft_core::RangeRef::new(c("B1"), c("B3")));
        // As after opening the file.
        t.calc.rebuild(&t.wb);
        t.calc.row_major_only = false;
        edit(&mut t, &[("Q1", "2")]);
        t
    };
    for threads in [1, 4] {
        let row_major = book(threads, Some(0));
        let t = book(threads, None);
        let what = format!("{threads} threads");
        assert!(t.calc.plans_made > 0, "{what}: planned");
        assert_eq!(t.calc.redo_reasons, [Redo::Overlap], "{what}");
        same_values(&t, &row_major, &what);
    }
}

#[test]
fn the_names_texts_are_not_looked_through_while_a_formula_may_draw() {
    // Whether a defined name may draw is asked only when no formula of the graph may: with
    // `=RAND()` in A1 a pass doesn't look through the names' texts, without it each pass does.
    for (with_draw, scans) in [(true, 0), (false, 1)] {
        let mut t = T::new();
        t.name("Rate", "Sheet1!$B$1*2");
        let mut cells = vec![("B1".to_string(), "3".to_string()), ("C1".to_string(), "=Rate+1".to_string())];
        if with_draw {
            cells.push(("A1".to_string(), "=RAND()".to_string()));
        }
        t.load(cells);
        crate::recalc::NAME_TEXT_SCANS.with(|n| n.set(0));
        edit(&mut t, &[("B1", "4")]);
        assert_eq!(t.num("C1"), 9.0, "drawing formula {with_draw}");
        assert_eq!(crate::recalc::NAME_TEXT_SCANS.with(|n| n.get()), scans, "drawing formula {with_draw}: passes that looked through the names");
    }
}

#[test]
fn an_array_counting_its_own_spill_area_ends_as_in_row_major_order() {
    // A1 counts cells of its own spill area, so each round it would grow by a row: a loop through
    // spill cells, which main doesn't report (upstream issue #4470; Excel reports a formula reading
    // its own spill as a circular reference). Planned and threaded runs end as row-major order
    // ends it, main's result: two rows, no circular reference listed.
    let t = ends_as_in_row_major("own spill count", &[("A1", "=SEQUENCE(COUNT(A2:A10)+2)")], &[("Y1", "1")]);
    let spill = t.wb.sheet(0).unwrap().spill_ranges.get(&c("A1")).copied();
    assert_eq!(spill.map(|r| r.height()), Some(2), "{spill:?}");
    assert!(t.calc.circular.is_empty(), "{:?}", t.calc.circular);
}

#[test]
fn two_arrays_sized_by_each_others_spill_areas_end_as_in_row_major_order() {
    // B1's height counts D1's spilled cells and D1's counts B1's, so they never settle: a loop
    // through spill cells, which main doesn't report (upstream issue #4470). Planned and threaded
    // runs end as row-major order ends it, main's result: five rows each, no circular reference
    // listed.
    let cells = [("B1", "=SEQUENCE(COUNT(D2:D100)+1)"), ("D1", "=SEQUENCE(COUNT(B2:B100)+2)")];
    let t = ends_as_in_row_major("sized by each other", &cells, &[("Y1", "1")]);
    let sheet = t.wb.sheet(0).unwrap();
    for at in ["B1", "D1"] {
        let spill = sheet.spill_ranges.get(&c(at)).copied();
        assert_eq!(spill.map(|r| r.height()), Some(5), "{at}: {spill:?}");
    }
    assert!(t.calc.circular.is_empty(), "{:?}", t.calc.circular);
}

#[test]
fn a_level_takes_its_formulas_name_lookups_from_the_graph() {
    // A defined name read by nothing, and a level of forty cheap formulas on worker threads: the
    // count of name lookups the level's workers may make comes from the formulas' graph nodes,
    // counted by the precedent walk that built them, so no formula tree is walked for it at the
    // edit, whether the name was defined before the formulas were built or after.
    for threads in [2, 4] {
        for name_first in [true, false] {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            if name_first {
                t.name("Tax_rate", "0.1");
            }
            t.load((1..=40).map(|r| (format!("A{r}"), "=$B$1+ROW()".to_string())).chain([("B1".to_string(), "1".to_string())]));
            if !name_first {
                t.name("Tax_rate", "0.1");
            }
            crate::recalc::NAME_LOOKUP_WALKS.with(|n| n.set(0));
            edit(&mut t, &[("B1", "2")]);
            let what = format!("{threads} threads, name defined first {name_first}");
            assert_eq!(t.num("A3"), 5.0, "{what}");
            assert!(t.calc.worker_levels > 0, "{what}: the level ran on worker threads");
            let walks = crate::recalc::NAME_LOOKUP_WALKS.with(|n| n.get());
            assert_eq!(walks, 0, "{what}: trees walked to count name lookups");
        }
    }
}

#[test]
fn a_dynamic_formula_after_an_array_laid_out_anew_in_its_level_reads_it_laid_out() {
    // The edit of Z1 changes G1's array in the first round; E1 reads G2, a cell of it, so E1 runs
    // in the second round, with B1, whose array grows from two rows to three through E1, and D1,
    // which reads B1's cells through INDIRECT and reads E1 too, so it shares B1's level and comes
    // after it in row-major order. D1 reads B3, the row B1's array gains, outside the area the
    // workbook stores for it, so a worker reads it as empty. That round is the last in which the
    // dynamic formulas run again, so a value D1 kept from before B1's array was laid out would
    // stay. B1 is kept from a worker (`=SEQUENCE(E1)`), or calculated on the main thread, as a
    // formula that draws a random number is (`=SEQUENCE(E1+0*RAND())`): either way D1 is
    // calculated again on the main thread at its turn and reads 3, as in row-major order, which
    // the count of formulas calculated again shows whichever worker took D1.
    for b1 in ["=SEQUENCE(E1)", "=SEQUENCE(E1+0*RAND())"] {
        let book = |plan_budget: Option<usize>| {
            let mut t = T::new().threads(4);
            t.calc.parallel_min = 1;
            t.calc.chunk = Some(1);
            t.calc.plan_budget = plan_budget;
            t.load(
                [("Z1", "1"), ("G1", "=SEQUENCE(2,1,Z1)"), ("E1", "=G2"), ("B1", b1), ("D1", "=N(INDIRECT(\"B3\"))+0*E1")]
                    .map(|(at, input)| (at.to_string(), input.to_string())),
            );
            t.calc.row_major_only = false;
            edit(&mut t, &[("Z1", "2")]);
            t
        };
        let row_major = book(Some(0));
        let t = book(None);
        assert_eq!(row_major.num("D1"), 3.0, "{b1}: row-major order");
        assert!(t.calc.plans_made > 0, "{b1}: planned");
        // D1's worker result is thrown away whichever worker took it: when the worker that laid
        // B1's array out took D1 too, D1 read B3 there, and its value alone would not show it.
        assert_eq!(t.calc.level_formulas_redone, 1, "{b1}: D1 calculated again at its turn");
        same_values(&t, &row_major, b1);
    }
}

/// A report whose stored output range holds its corners the wrong way round, as a JSON workbook
/// can (the file is read as stored), read by GETPIVOTDATA in a planned round beside an array and
/// a reader of its spill: the round is planned, on worker threads with four, and gives what a
/// row-major run gives (and main: `#REF!`, then 4 and 5). The planner's column walk overflowed
/// on the backward area, and in a build without overflow checks the order check's look-ups
/// panicked instead.
#[test]
fn a_report_range_stored_the_wrong_way_round_plans_as_in_row_major_order() {
    for threads in [1u32, 4] {
        let run = |row_major: bool| {
            let mut t = T::new().threads(threads);
            t.calc.parallel_min = 1;
            if row_major {
                t.calc.plan_budget = Some(0);
            }
            let backward = gridcraft_core::RangeRef { start: c("F6"), end: c("A1") };
            let report = gridcraft_model::PivotTable { last_range: Some(backward), ..Default::default() };
            t.wb.sheet_mut(0).unwrap().pivots.push(report);
            let cells = [("Q1", "1"), ("J1", "=GETPIVOTDATA(\"Sales\",A1:F6)"), ("D10", "=SEQUENCE(3)+Q1"), ("H20", "=D11+1")];
            let filler = (1..=40).map(|r| (format!("K{r}"), "=ROW()".to_string()));
            t.load(cells.map(|(a, b)| (a.to_string(), b.to_string())).into_iter().chain(filler));
            let loaded = (t.get("J1"), t.num("H20"), t.calc.worker_levels);
            t.set("Q1", "2");
            (t, loaded)
        };
        let ((planned, loaded), (row_major, _)) = (run(false), run(true));
        assert_eq!((loaded.0, loaded.1), (Value::Error(CellError::Ref), 4.0), "{threads} threads");
        assert_eq!(loaded.2 > 0, threads > 1, "{threads} threads: levels on workers at load");
        assert_eq!((planned.get("J1"), planned.num("H20")), (Value::Error(CellError::Ref), 5.0), "{threads} threads");
        same_values(&planned, &row_major, &format!("{threads} threads"));
    }
}
