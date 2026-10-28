//! Recalculation planned in dependency levels, and wide levels calculated on worker threads:
//! the plan's order, what one thread and many give, and what a worker hands back to the main thread.

use super::*;
use crate::Key;

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
        assert_eq!(sa.spill_ranges, sb.spill_ranges, "{what}: sheet {i} spill ranges");
    }
    let (mut ca, mut cb) = (a.calc.circular.clone(), b.calc.circular.clone());
    ca.sort();
    cb.sort();
    assert_eq!(ca, cb, "{what}: circular references");
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
fn a_chain_of_spill_areas_resolves_in_the_first_round() {
    // Ten spills in a chain, each reading the area (not the anchor) of the one before, laid out
    // so every reader comes before what it reads in row-major order. Nothing is calculated on
    // demand (the areas aren't formulas), so each write-back round settles one link when the
    // formulas run in row-major order: on one thread the 8-round limit leaves the chain stale
    // (A1 = 4193, three links short). Planned in levels, each reader waits for the anchor above
    // the area it reads, so the first round settles every link; the second runs the readers of
    // the new arrays once more and finds nothing changed.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells = vec![("K1".to_string(), "=SEQUENCE(600,1,Z1)".to_string()), ("Z1".to_string(), "1".to_string())];
        for (i, col) in "JIHGFEDCBA".chars().enumerate() {
            let prev = "KJIHGFEDCB".chars().nth(i).unwrap_or('K');
            cells.push((format!("{col}1"), format!("=SEQUENCE(600,1,MAX({prev}2:{prev}600))")));
        }
        t.load(cells);
        assert_eq!(t.num("A1"), 1.0 + 10.0 * 599.0, "{threads} threads");
        assert_eq!(t.num("A600"), 600.0 + 10.0 * 599.0, "{threads} threads");
        assert_eq!(t.calc.last_recalc_cells, 11 + 10, "{threads} threads");
    }
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
fn a_chain_of_arrays_spilling_right_into_the_row_below_settles_in_extension_rounds() {
    // A{r} reads B:T of the row above, which A{r-1} spills into from the left: not under any
    // formula of those columns, so the planner doesn't hold A{r} back, and the chain in Z puts
    // A16 first. Each write-back round settles one link, and the 8-round limit would leave rows 9
    // to 16 one wide. The planned round finds an array grown over cells that a formula of an
    // earlier level read, so the rounds after it don't count toward the 8 while together they
    // calculate no more formulas than the first round did: fifteen more rounds settle the chain.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(sideways_chain("=SEQUENCE(1,5)*Z116"));
        assert_eq!(sideways_widths(&t), vec![5; 16], "{threads} threads");
        assert_eq!((t.num("A16"), t.num("E16")), (1.0, 5.0), "{threads} threads");
        assert_eq!((t.calc.extension_rounds, t.calc.row_major_fallbacks), (15, 0), "{threads} threads");
    }
}

#[test]
fn a_chain_of_arrays_spilling_right_into_the_row_below_settles_after_an_edit() {
    // The same chain with A1 sized by the end of the Z chain: one wide everywhere after loading,
    // then Z100 = 5 makes A1 five wide, and every row below follows in the rounds after the
    // edit's first, which don't count toward the 8.
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
fn a_chain_of_arrays_spilling_down_right_into_a_corner_area_settles_in_extension_rounds() {
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
        assert_eq!((t.calc.extension_rounds, t.calc.row_major_fallbacks), (11, 0), "{threads} threads");
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
fn an_indirect_reader_of_a_spill_sized_by_another_spill_is_right_after_a_full_recalc() {
    // A1 spills three rows once Z2 is known; C1's height comes from A3, a cell A1 spills over,
    // and B1 reads C3 through INDIRECT, with no static link to either, so it is in the first
    // level, before both arrays. Formulas like B1 are calculated once more after the first round
    // that lays out an array, and again after a planned round in which an array changed, so B1
    // reads C3 once C1 has spilled over it.
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
        assert_eq!(t.num("B1"), 3.0, "after two edits, {threads} threads");
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
        assert!(planned.calc.graph.levels(&planned.wb, &keys).is_some());
        assert!(planned.calc.graph.levels_within(&planned.wb, &keys, Some(0)).is_none());
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
    // level. A reader in another worker's chunk saw the stored
    // (empty) cells where one thread, in row-major order, saw the array: each reader of a new
    // array is calculated again on the main thread at its turn, after the array, as one thread
    // calculates it. Run several times, since the old result depended on which worker took which
    // chunk.
    let build = |threads: u32| {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
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
    // reads B3's stored value there, 2, which the write-back clears afterwards.)
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
        assert_eq!((one.num("B2"), one.get("B3"), one.num("D3")), (5.0, Value::Empty, 2.0), "{name}: one thread");
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
fn a_chain_of_arrays_longer_than_the_extension_rounds_allow_settles_in_row_major_order() {
    // The sideways chain, plus ten formulas below it counting everything rows 1 to 16 spill
    // into: each of those reads every link, so each extension round calculates them again with
    // the next link, and after three such rounds the next would calculate more formulas than the
    // first round did (16 links, 16 Z links, 10 counts). The pass then calculates the first
    // round's formulas once in row-major order, which settles the rest of the chain.
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
        assert_eq!((t.calc.extension_rounds, t.calc.row_major_fallbacks), (3, 1), "{threads} threads");
    }
}

#[test]
fn a_chain_of_a_hundred_arrays_settles_in_the_row_major_round_after_the_extension_rounds() {
    // The sideways chain with 100 links and nothing else: the first round calculates 200
    // formulas (100 links, 100 Z links), and each extension round settles one link with one
    // formula but is charged a 64th of that, rounded up past it: 4. Fifty extension rounds use up
    // the budget, and the round in row-major order settles the other links at once.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        t.load(sideways_chain_of(100, "=SEQUENCE(1,5)*Z200"));
        assert_eq!(sideways_widths_of(&t, 100), vec![5; 100], "{threads} threads");
        assert_eq!((t.num("A100"), t.num("E100")), (1.0, 5.0), "{threads} threads");
        assert_eq!((t.calc.extension_rounds, t.calc.row_major_fallbacks), (50, 1), "{threads} threads");
    }
}

#[test]
fn arrays_that_never_settle_stop_after_a_bounded_number_of_extension_rounds() {
    // Three formulas in column A, each one cell wide when the two cells right of the one before it
    // (A30 for A10) hold a number and spilling three wide otherwise, in a loop: whatever one does,
    // the next undoes, so each round dirties a few of them again and the arrangement never
    // settles. Z5 puts A10 in a later level than A20, which reads B10:C10, so the first planned
    // round lays an array out over cells an earlier level read, and the rounds after it are
    // extension rounds. 5,000 more formulas in column AZ make the first round large. Charged by
    // the formulas they calculate alone, the extension rounds would go on for about as many rounds
    // as the first round had formulas (5,004). Each is charged at least 5,004 / 64 + 1 = 79, so
    // there are at most 63 of them, then the round in row-major order, and at most 7 more counted
    // rounds: with at most 8 counted rounds before the extension rounds, no more than 8 + 64 + 8
    // rounds in all.
    for threads in [1, 8] {
        let mut t = T::new().threads(threads);
        t.calc.parallel_min = 1;
        let mut cells: Vec<(String, String)> = (1..=5_000).map(|r| (format!("AZ{r}"), "=ROW()*2".to_string())).collect();
        cells.push(("Z5".to_string(), "=1".to_string()));
        cells.push(("A10".to_string(), "=IF(COUNT(B30:C30)>0,1,SEQUENCE(1,3))+Z5*0".to_string()));
        cells.push(("A20".to_string(), "=IF(COUNT(B10:C10)>0,1,SEQUENCE(1,3))".to_string()));
        cells.push(("A30".to_string(), "=IF(COUNT(B20:C20)>0,1,SEQUENCE(1,3))".to_string()));
        t.load(cells);
        let (rounds, extension) = (t.calc.rounds_calculated, t.calc.extension_rounds);
        assert!(extension <= 64, "{rounds} rounds, {extension} of them extension rounds, on {threads} threads");
        assert!(rounds <= 8 + 64 + 8, "{rounds} rounds, {extension} of them extension rounds, on {threads} threads");
        assert_eq!(t.calc.row_major_fallbacks, 1, "{threads} threads");
    }
}
