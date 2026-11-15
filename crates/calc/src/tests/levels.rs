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
        self.wb.names.push(gridcraft_model::DefinedName {
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
    /// planned on its own).
    fn threads(mut self, n: u32) -> T {
        self.wb.calc.multi_threaded = n > 1;
        self.wb.calc.threads = n;
        // Whatever `GRIDCRAFT_CALC_THREADS` says and however many processors there are.
        self.calc.threads = Some(n as usize);
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
    // Never more than the processors present (a workbook's count travels with the file).
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(64);
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
    // Found by a reviewer: an INDIRECT reader of A1 in G1, an array above the cycle, and a second
    // INDIRECT reader over it. Each edit of A1 planned D3 ahead of H2 and showed D3 = 2 * A1 + 1,
    // while a full recalculation showed `#CIRC!`.
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
    let kinds = [tail(139), tail(140), wrapped(69), wrapped(70), "=CNT(131)".to_string()];
    let want = [Value::Number(0.0), Value::Error(CellError::Num), Value::Number(69.0), Value::Error(CellError::Num), Value::Number(131.0)];
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
            t.wb.names.push(gridcraft_model::DefinedName {
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
        t.wb.names.push(gridcraft_model::DefinedName { name, scope, formula, comment: String::new(), hidden: false });
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
