use super::draws::draws_random;
use super::graph::{Graph, HashMap, HashSet, Key};
use super::{Evaluator, Host, Workbook, prof_now, spill_end, stack_position};
use crate::eval::{NameIndex, NameLookup};
use crate::spill_index::{Cover, SpillIndex};
use gridcraft_core::{Array, CellError, CellRef, RangeRef, Value};
use gridcraft_formula::Expr;

/// Formulas a formula may calculate on demand one inside the other (each needing one another's
/// value before it is done). Each level takes stack, so a chain deeper than this is suspended and
/// calculated from the top instead (see [`PassHost::calculate`]): long dependency chains calculate
/// whatever order they come in, with bounded stack.
const MAX_NESTED: usize = 64;

/// Formula evaluations iterative calculation may spend on one recalculation (passes × formulas
/// evaluated again), however high Maximum Iterations is.
const MAX_ITERATION_WORK: usize = 10_000_000;
/// Most definition text, in bytes, whose parsed formulas a pass keeps. A parsed formula takes
/// about a hundred times its text, so a workbook of many long names would otherwise hold gigabytes
/// for the pass; past this, about 100 MB of trees, a name is parsed again at each use. A library
/// of LAMBDA names is a few kilobytes.
const MAX_PASS_NAME_TEXT: usize = 1 << 20;

#[cfg(test)]
thread_local! {
    /// Definitions [`PassHost`] parsed on this thread for the evaluator, which a test reads; a
    /// level's workers add theirs to the main thread's after the level.
    pub(crate) static NAMES_PARSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Probes of the per-cell and column indexes the look-ups of [`PassHost::mark_readers`]
    /// made on this thread (`Graph::uncharged_probes` of each area looked up), which a test
    /// reads. Marking runs on the main thread.
    pub(crate) static MARKING_PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Arrays [`first_covering`] checked on this thread for a cell several arrays cover, which a
    /// test reads.
    pub(crate) static COVER_CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Look-ups made on this thread only to note pairs (see `PassHost::notes_wanted`): of the
    /// arrays over a cell (`note_arrays_over`), and of those reaching into a block from outside
    /// (`read_range`, `note_stored_outside`), which a test reads.
    pub(crate) static ARRAYS_OVER_LOOKUPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Formula trees `PassHost::run_level` walked on this thread to count their name lookups,
    /// for want of a count in the graph (`Node::name_lookups`), which a test reads.
    pub(crate) static NAME_LOOKUP_WALKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Formulas a worker takes from a level at a time.
const CHUNK: usize = 32;

/// Note entries (reads and pairs) a worker takes at a time from what the level's workers may hold
/// between them, so that they don't all update one counter at every note. It bounds the entries
/// one worker holds unused while another finds none left and hands its formula back (see
/// `take_note_entry`).
const NOTE_BATCH: usize = 256;

/// Stack of a calculation thread: at least the main thread's on every desktop platform (8 MiB
/// on macOS and Linux, 1 MiB on Windows), so a formula nests no less deep on a worker than on
/// the main thread.
const WORKER_STACK: usize = 8 << 20;

/// A block's results by last row: its sum and count, or `None` when it can't be summed from constants.
type BlockSums = std::collections::BTreeMap<u32, Option<(f64, u64)>>;

/// What a planned round notes for `Calc::agrees_with_row_major`.
#[derive(Default)]
pub(super) struct Watch {
    /// When each formula of the round started and finished being calculated on the main thread,
    /// on one clock, in the order they finished. A worker's result merged into the level counts
    /// at its turn in the walk (see `run_level`): it reads what one thread would have read then.
    /// A list rather than a map: the check looks formulas up only when an array of the round
    /// changed (see `Calc::agrees_with_row_major`), so the map is built there, then.
    pub(super) times: Vec<(Key, u64, u64)>,
    /// What the dynamic formulas of the round read (workers' included): the formula, the sheet
    /// and the cells. Reads of formulas they calculate on demand belong to those formulas, whose
    /// references the graph lists. A read is joined to the formula's entry before it when the two
    /// make one rectangle (see `note_read`).
    pub(super) reads: Vec<(Key, usize, RangeRef)>,
    /// Entries `reads` and `unforced` may still take between them, from `Plan::note_limit` (the
    /// notes' own limit beside the budget rule of `index_budget`); on a worker, the entries it
    /// has taken from what the level's workers may hold between them (see `run_level`). An entry
    /// is charged before it is stored.
    pub(super) budget: usize,
    /// The round's reads and pairs needed more entries than `budget` allowed: `reads` and
    /// `unforced` were let go, and what the dynamic formulas read is unknown, so the round is kept
    /// only if none of its arrays changed.
    pub(super) unknown: bool,
    /// Each formula before an array's anchor in row-major order that read cells of the array laid
    /// out in the round other than through a block read holding the anchor (one cell, or a block
    /// the array reaches into from outside), with that anchor: such a read takes the cells without
    /// calculating the anchor first, so in row-major order it may take them before the array is
    /// laid out. A block read holding the anchor calculates every formula in the block, the anchor
    /// among them, before it takes a value (see `read_range`), in either order. The check looks at
    /// these pairs only for a reader before the anchor (see `Calc::agrees_with_row_major`). Each
    /// distinct pair takes one of the entries `budget` allows, as `reads` do, and goes with them
    /// when they run out; on a worker, they are kept for the result merged at its turn.
    ///
    /// Such a read is noted by the array's area as the reading host sees it: the new one when it
    /// has calculated the anchor, and the one the workbook stores for it while it has not (see
    /// `StoredAreas`), so that the pairs don't depend on the thread count. The views differ only
    /// by anchors one thread has calculated by the reader's turn (at their turn, or on demand
    /// inside a formula before the reader) and a worker has not. The main thread calculates such
    /// an anchor, or merges a worker's result for it, in the walk before the reader (see
    /// `run_level`); when its array reads otherwise than as stored, the readers of its new area
    /// and the dynamic formulas after it are calculated again on the main thread, and the planner
    /// holds the readers of its stored area back to a later level. So a reader whose worker
    /// result is kept met only arrays of those anchors that read as stored, whose two areas are
    /// the same.
    pub(super) unforced: Vec<(Key, Key)>,
    /// The pairs in `unforced`, so that a formula reading the same arrays again and again (in a
    /// loop, or alternately) notes each pair once. The two always hold the same pairs on the main
    /// thread (each place that adds to, drains or lets go of one does the same to the other), so
    /// `Calc::agrees_with_row_major` looks pairs up here.
    pub(super) paired: HashSet<(Key, Key)>,
}

pub(super) struct PassHost<'a> {
    wb: &'a Workbook,
    exprs: &'a HashMap<Key, std::sync::Arc<Expr>>,
    /// What the formulas and Python cells of the pass gave so far. One of them without a result
    /// is pending (see `is_pending`): it is calculated before it is read. A result is stored as
    /// the formula is done, with nothing read in between, so no separate set of the pending ones
    /// is kept.
    pub(super) results: HashMap<Key, Value>,
    in_progress: HashSet<Key>,
    /// New spills found during the pass: anchor → array.
    pub(super) spills: HashMap<Key, std::sync::Arc<Array>>,
    /// Where the areas in `spills` are, so a read finds the ones it meets without scanning them all.
    spill_index: SpillIndex,
    /// Used range per sheet: the workbook doesn't change during a pass, and working it out walks
    /// every row, so once per sheet rather than once per range read.
    used: HashMap<usize, Option<RangeRef>>,
    rng: &'a mut u64,
    /// Who held a spill before the calculation; unset: every formula did (reading order decides).
    pub(super) standing: Option<&'a super::Standing>,
    now: f64,
    cycle: bool,
    pub(super) cycles: Vec<Key>,
    pub(super) depth: usize,
    /// A formula needed at [`MAX_NESTED`] levels deep: the evaluations in progress are abandoned
    /// and it is calculated first.
    suspend: Option<Key>,
    /// Formulas whose evaluation was abandoned for another one, to be calculated again after it.
    /// They all need it, so reading one of them while it is calculated is a circular reference,
    /// as reading a formula in progress is.
    waiting: HashSet<Key>,
    /// The formulas abandoned since the last suspension.
    abandoned: Vec<Key>,
    /// Other formulas read at [`MAX_NESTED`] levels deep by the evaluations being abandoned. They
    /// are calculated along with the suspended one, rather than one per restart (a wide range
    /// read at the limit would restart the abandoned evaluations once per cell in it).
    also: Vec<Key>,
    /// Suspensions so far (tests check that a wide read at the limit needs one, not one per cell).
    #[cfg(test)]
    pub(super) suspensions: usize,
    /// Spilled values looked at by range reads (tests check that a read outside a large spill
    /// looks at none of it).
    #[cfg(test)]
    pub(super) spill_visits: usize,
    /// The formulas in `cycles`.
    cycle_seen: HashSet<Key>,
    /// Iterative calculation: a circular reference reads the cell's previous value instead of
    /// giving `#CIRC!`.
    pub(super) iterate: bool,
    /// Values from the previous iteration; cells not in it read their stored value.
    prev: HashMap<Key, Value>,
    /// Formulas being evaluated, innermost last (kept only when iterating).
    stack: Vec<Key>,
    /// Formulas found on a circular reference (when iterating).
    pub(super) members: HashSet<Key>,
    /// Results that read a previous value, directly or through other formulas.
    pub(super) tainted: HashSet<Key>,
    /// Whether the formula being evaluated has read a previous value.
    taint: bool,
    /// "Set precision as displayed" is on: results are rounded to their cells' formats before
    /// they are stored or read by other formulas.
    precision: Option<crate::DisplayedPrecision>,
    /// `stack_position()` where the outermost formula being calculated started.
    stack_top: usize,
    /// Whether an evaluation ran past the stack budget since the innermost formula started.
    out_of_stack: bool,
    /// Sum and count of blocks that hold only constants, for `range_sum`. Keyed by sheet, first
    /// cell and last column; each value maps a last row to the block's result (`None`: the block
    /// has a formula, a Python cell, an error value or a spilled value, so every longer one has too).
    /// A worker reads the main thread's first; what it adds is merged into the main thread's after
    /// the level (see `range_sum`).
    sums: HashMap<(usize, CellRef, u32), BlockSums>,
    /// Blocks `range_sum` walked in the store (a worker's are added to the main thread's after
    /// the level), for tests.
    #[cfg(test)]
    pub(super) block_walks: usize,
    /// The kept results of the Python cells in the pass: a Python cell isn't evaluated, its
    /// result is laid out (an array spills from the cell, as a formula's does). Each one is
    /// pending until it is laid out, so a formula that reads it first gets its new value. A
    /// worker holds none: it reads them through the main thread's host.
    python: HashMap<Key, Value>,
    /// Parsed defined names, by their place in the workbook's list: the workbook doesn't change
    /// during a pass, and a column of cells calling one LAMBDA name would otherwise parse that
    /// name's formula (and every name it calls) once per cell. On a worker, the definitions of
    /// `level_names` it has used, charged there.
    names: HashMap<usize, Option<std::sync::Arc<Expr>>>,
    /// Finds the workbook's names for the pass (see [`NameLookup`]); a level's workers read its
    /// index (see `run_level`).
    name_lookup: NameLookup,
    /// On a worker of a level that started without this pass's name index: the index the
    /// level's workers share, built by the first of them to need one, which the main thread
    /// adopts after the level (see `run_level`).
    shared_names: Option<&'a std::sync::OnceLock<NameIndex>>,
    /// Bytes of the definitions kept in `names`.
    names_text: usize,
    /// Set by `evaluate_expr` for a one-shot evaluation (a conditional format per painted cell, a
    /// validation, a filter criterion per row), which gets a fresh host at every call: `name_expr`
    /// then scans the workbook's names at every lookup instead of going through `name_lookup`,
    /// whose index would cost a String and a hash insert per defined name at each call.
    pub(super) one_shot: bool,
    /// On a worker: the parsed definitions the level's workers share (see [`LevelNames`]).
    level_names: Option<&'a LevelNames>,
    /// The most definition text the main thread and a level's workers kept at once.
    #[cfg(test)]
    pub(super) peak_names_text: usize,
    /// The main thread's host, when this one runs on a worker thread of a multi-threaded level:
    /// what it has calculated so far is read through it (see [`PassHost::worker`]).
    base: Option<&'a PassHost<'a>>,
    /// On a worker: the formula being evaluated read a formula that isn't done or a Python cell
    /// not laid out yet, drew a random number or found no entry left to note a read in, so it is
    /// handed back to the main thread (see [`PassHost::compute`]); `run_level` sets it too for a
    /// result whose array or text the worker can't keep. Its result is thrown away, so from then
    /// on the evaluation reads nothing: the reads return at once (a cell `#CALC!`, the error the
    /// blocking read gave; a block no values; a name, a spill range or the used range none)
    /// without touching the store, the copied blocks, the sums, the names or the notes, a read
    /// under way stops where it is, and the evaluator stops once the read, or the function that
    /// drew, returns (`Host::halted`), with that read's result dropped for its skip value (see
    /// `Evaluator::read`), so the error can't steer the evaluation into work one thread doesn't
    /// do; its loops over such reads, draws and calls end too. The rule and the one shape that
    /// keeps it are written at `gridcraft_functions::Halt`.
    pub(super) blocked: bool,
    /// Walks of the store (a block read, visited or summed) made while `blocked`, for tests.
    #[cfg(test)]
    pub(super) blocked_walks: usize,
    /// Volatile and dynamic-reference functions called while `blocked` (see `note_volatile`),
    /// for tests.
    #[cfg(test)]
    pub(super) blocked_calls: usize,
    /// Reads of cells and random draws asked of this host while `blocked`, and cell reads that
    /// gave anything but `#CALC!` with it set by the end of the read, for tests: none while the
    /// evaluation and its loops stop (see `blocked`).
    #[cfg(test)]
    pub(super) blocked_reads: usize,
    /// Formulas the workers calculated, those they then handed back included, for tests.
    #[cfg(test)]
    pub(super) worker_calculated: usize,
    /// On a worker: large blocks read twice in this level, with the text copied, so that
    /// reading them again clones reference counts this thread alone holds. Every thread reading
    /// the same lookup column clones (and drops) each cell's shared text otherwise, and those
    /// counters bounce between the cores: with eight threads that cost more than the calculation.
    /// Kept by sheet, so that an array laid out looks only at the blocks of its own sheet that
    /// can reach it (see `drop_copies`).
    ranges: HashMap<usize, SheetBlocks>,
    /// On a worker: blocks read once in this level; the second read copies them, so a block read
    /// by one formula only (a running total's, say) never pays for a copy. A block is noted only
    /// while the worker's share of the bytes it may keep has room for what the set grows by
    /// (`seen_bytes`); past it, the blocks read for the first time are not noted, so they are not
    /// copied, and the reads go to the store as one thread's do.
    pub(super) seen: HashSet<(usize, RangeRef)>,
    /// Bytes of the table behind `seen`, charged with `cached_bytes` against the worker's share.
    pub(super) seen_bytes: usize,
    /// Copied blocks looked at by arrays dropping the ones they land in, for tests.
    #[cfg(test)]
    pub(super) blocks_checked: usize,
    /// Bytes held in `ranges` and, on a worker, in the arrays and text results it keeps (see
    /// `run_level`), and the most this host may hold (its share of [`CACHE_BYTES_BUDGET`]).
    pub(super) cached_bytes: usize,
    cache_budget: usize,
    /// On a worker: its copied blocks made way for a result (see `run_level`), so it copies no
    /// more in this level.
    copies_dropped: bool,
    /// Levels calculated on worker threads, for tests.
    #[cfg(test)]
    pub(super) worker_levels: usize,
    /// Levels calculated on this thread because their workers could not run: the first one in a
    /// round logs a warning (tests read the count too).
    pub(super) fallbacks: usize,
    /// Formulas the workers handed back to this host (see [`Calc::handed_back`]), for tests.
    #[cfg(test)]
    pub(super) handed_back: usize,
    /// Levels in which formulas after an array laid out differently from what the workbook
    /// stores were calculated again on this thread (see [`Calc::relaid_levels`]), for tests.
    #[cfg(test)]
    pub(super) relaid_levels: usize,
    /// Formulas of levels run on workers that this host calculated at their turn (handed back,
    /// random, or with an array the workers' result can't stand with), for tests.
    #[cfg(test)]
    pub(super) on_main: usize,
    /// Of those, the ones whose worker result was thrown away for an array before them.
    #[cfg(test)]
    pub(super) redone: usize,
    /// The index work looking up the readers of arrays laid out differently from what the
    /// workbook stores may still cost in levels calculated on worker threads this round (see
    /// [`PassHost::mark_readers`]).
    pub(super) marks_left: usize,
    /// Levels in which those look-ups ran out of `marks_left`, for tests.
    #[cfg(test)]
    pub(super) marks_given_up: usize,
    /// While `recording`: formulas whose array this host laid out differently from what the
    /// workbook stores (in place, shape or values, or none where one is stored); see `run_level`.
    laid_out: Vec<Key>,
    recording: bool,
    /// The dependency graph, for the readers of an array's area (see `run_level`), and its
    /// formulas whose references are only known while evaluating (see `PassHost::dynamic`).
    graph: Option<&'a Graph>,
    /// Formulas a worker takes from a level at a time ([`CHUNK`]; tests use smaller chunks to
    /// spread the formulas of small workbooks over the workers).
    pub(super) chunk: usize,
    #[cfg(test)]
    pub(super) fail_workers: bool,
    /// The bytes a level's workers may keep, in place of [`CACHE_BYTES_BUDGET`].
    #[cfg(test)]
    pub(super) level_bytes: Option<usize>,
    /// The most bytes of arrays and text results the workers of one level handed in.
    #[cfg(test)]
    pub(super) peak_worker_bytes: usize,
    /// A planned round: what it notes in `watch_out` (see [`Watch`]).
    pub(super) watch: bool,
    pub(super) watch_out: Watch,
    /// On a worker: the noted reads the level's workers may still hold between them, from which
    /// it takes [`NOTE_BATCH`] at a time into `watch_out.budget` (see `run_level`).
    note_pool: Option<&'a std::sync::atomic::AtomicUsize>,
    /// The clock of `Watch::times`.
    clock: u64,
    /// While `watch`: the formulas being evaluated, innermost last, each with whether it is
    /// dynamic (its reads are noted).
    current: Vec<(Key, bool)>,
    /// While `watch`: the anchors of the arrays the last `read_range` met reaching into its block
    /// from outside, which a worker keeps with a copy of the block (see `SheetBlocks::outside`).
    outside_read: Vec<CellRef>,
    /// While `watch`, on the main thread: the areas the workbook stores for the arrays of the
    /// round's formulas (see `Watch::unforced`); a worker reads the main thread's.
    stored: StoredAreas,
    /// Reads a worker served from its copied blocks (tests).
    #[cfg(test)]
    pub(super) copies_read: usize,
}

/// A worker's copied blocks on one sheet, by first and last cell, and the same blocks by column
/// band and height: a block is listed in each band of [`BAND_COLS`] columns it spans, and in a
/// band, class `c` holds the blocks of `2^c` to `2^(c+1) - 1` rows, ordered by first cell (row
/// first). A block of class `c` meeting an area starts at most `2^(c+1) - 2` rows above it, so an
/// array finds the blocks it lands in among those of each class, in the bands its columns span,
/// starting from there down to its last row: one tall block makes it look at that block, not at
/// every short one within its height, and blocks in the same rows but other columns are not
/// looked at.
#[derive(Default)]
struct SheetBlocks {
    blocks: HashMap<(CellRef, CellRef), Vec<Value>>,
    by_height: std::collections::BTreeMap<(u32, u32), std::collections::BTreeSet<(CellRef, CellRef)>>,
    /// For a block that arrays reach into from outside, their anchors: a read served from the copy
    /// notes them as the read that made it did (see `Watch::unforced`).
    outside: HashMap<(CellRef, CellRef), Vec<CellRef>>,
}

impl SheetBlocks {
    /// The height class of a block of `rows` rows (see [`SheetBlocks`]).
    fn class(rows: u32) -> u32 {
        rows.max(1).ilog2()
    }

    /// The column bands `range` spans (see [`SheetBlocks`]).
    fn bands(range: RangeRef) -> std::ops::RangeInclusive<u32> {
        range.start.col / BAND_COLS..=range.end.col / BAND_COLS
    }

    fn insert(&mut self, range: RangeRef, copy: Vec<Value>, outside: Vec<CellRef>) {
        let class = SheetBlocks::class(range.height());
        for band in SheetBlocks::bands(range) {
            self.by_height.entry((band, class)).or_default().insert((range.start, range.end));
        }
        self.blocks.insert((range.start, range.end), copy);
        if !outside.is_empty() {
            self.outside.insert((range.start, range.end), outside);
        }
    }

    /// Takes the block out; its values, if it was held.
    fn remove(&mut self, (from, to): (CellRef, CellRef)) -> Option<Vec<Value>> {
        let range = RangeRef::new(from, to);
        let class = SheetBlocks::class(range.height());
        for band in SheetBlocks::bands(range) {
            if let Some(set) = self.by_height.get_mut(&(band, class)) {
                set.remove(&(from, to));
                if set.is_empty() {
                    self.by_height.remove(&(band, class));
                }
            }
        }
        self.outside.remove(&(from, to));
        self.blocks.remove(&(from, to))
    }
}

/// The areas the workbook stores for the arrays of a planned round's formulas and Python cells
/// (one cell or more besides the anchor), by sheet, with the last of their anchors on each sheet
/// in row-major order: a reader after it reads no stored array whose anchor comes after it.
#[derive(Default)]
struct StoredAreas {
    index: SpillIndex,
    last: HashMap<usize, CellRef>,
}

/// Blocks smaller than this aren't kept per worker: copying them costs more than it saves.
const MIN_CACHED_CELLS: usize = 64;
/// Columns in a band of a worker's copied blocks (see [`SheetBlocks`]). A speed switch, not a
/// limit: an array looks for the blocks it lands in among those of the bands its columns span,
/// so blocks in its rows but far to the side are not looked at (measured on 5b4ba488 without the
/// bands: 8,000 one-column blocks beside 100,000 one-cell arrays in one level made an edit on two
/// threads about five times as slow as on one), and which blocks it drops doesn't depend on it.
/// As wide as the smallest block kept ([`MIN_CACHED_CELLS`] cells) can be, so a block is listed in
/// at most two bands plus one for each 64 of its cells.
const BAND_COLS: u32 = 64;
/// Bytes the workers of a level keep, in copied blocks and in the arrays and text results they
/// hold until the main thread's walk (which may throw them away), each worker within an equal
/// share: text is counted in full (a text shared within one result once), so the budget counts bytes, not
/// cells (a cell can hold 32K characters). A result is charged once it is built, so a worker may
/// hold its share and the array or text it is building. Past its share a worker reads the blocks
/// from the store as one thread does; a result that would fit without its copies makes them go,
/// and one that doesn't fit at all is handed back with the rest of its chunk (see `run_level`).
///
/// The level is the largest array one formula may build from a reference or an operator,
/// [`MAX_CELLS`](crate::eval::MAX_CELLS) values: the workers of a level together keep no more than
/// one more formula's largest array (384,000,000 bytes where a value takes 24, as on 64-bit
/// targets).
const CACHE_BYTES_BUDGET: usize = crate::eval::MAX_CELLS as usize * std::mem::size_of::<Value>();

/// What a copied block costs to keep: the values, and the text they hold (with its
/// reference-count header).
fn block_bytes(block: &[Value]) -> usize {
    block.iter().map(|v| std::mem::size_of::<Value>() + if let Value::Text(t) = v { t.len() + 16 } else { 0 }).sum()
}

/// What a worker keeps for a formula's result: its array's values, its own value when that is
/// text, and the text they hold, each allocation once (with its reference-count header). Unlike a
/// copied block, which owns a text per cell, an array's values often share one
/// (`=IF(SEQUENCE(100),$A$1)` holds `$A$1`'s text once), and an array formula's value is its
/// first element. A result of another kind is not charged: it takes one entry of a fixed size, in
/// the worker's results and then in the level's merged ones, as the main thread keeps one for
/// every formula of the pass. A text is told apart by its address: a set of them, filled only for
/// a text that isn't the one before it or the first one met (a single text needs no set).
/// `None` once the count passes `room`, where it stops.
/// A text is charged at its full length for each result that holds it, also when the workbook's
/// stored cells or the worker's own copied block share it (a copied block is charged on its own
/// as well), so a level of results that only reference long stored texts can fill a worker's
/// share without allocating any.
fn held_bytes(value: Option<&Value>, array: Option<&Array>, room: usize) -> Option<usize> {
    let text = value.filter(|v| matches!(v, Value::Text(_)));
    let values = array.map_or(&[][..], |a| a.data.as_slice());
    if text.is_none() && values.is_empty() {
        return Some(0);
    }
    let mut bytes = std::mem::size_of::<Value>().saturating_mul(values.len().saturating_add(usize::from(text.is_some())));
    if bytes > room {
        return None;
    }
    // The texts met, told apart by address: the first, then a set of the others.
    let mut first = None;
    let mut seen: HashSet<usize> = HashSet::default();
    let mut last = None;
    for v in text.into_iter().chain(values) {
        if let Value::Text(t) = v {
            let at = std::sync::Arc::as_ptr(t).cast::<u8>().addr();
            let new = last != Some(at)
                && match first {
                    None => {
                        first = Some(at);
                        true
                    }
                    Some(f) => f != at && seen.insert(at),
                };
            if new {
                bytes = bytes.saturating_add(t.len().saturating_add(16));
                if bytes > room {
                    return None;
                }
            }
            last = Some(at);
        }
    }
    #[cfg(test)]
    assert_eq!(Some(bytes), held_bytes_with_one_set(value, array, room), "held bytes");
    Some(bytes)
}

/// [`held_bytes`] as it was written before the first text was kept apart from the set: every text
/// in one set. The tests compare the two at every call.
#[cfg(test)]
fn held_bytes_with_one_set(value: Option<&Value>, array: Option<&Array>, room: usize) -> Option<usize> {
    let text = value.filter(|v| matches!(v, Value::Text(_)));
    let values = array.map_or(&[][..], |a| a.data.as_slice());
    let mut bytes = std::mem::size_of::<Value>().saturating_mul(values.len().saturating_add(usize::from(text.is_some())));
    let mut seen: HashSet<usize> = HashSet::default();
    let mut last = None;
    for v in text.into_iter().chain(values) {
        if let Value::Text(t) = v {
            let at = std::sync::Arc::as_ptr(t).cast::<u8>().addr();
            if last != Some(at) && seen.insert(at) {
                bytes = bytes.saturating_add(t.len().saturating_add(16));
                if bytes > room {
                    return None;
                }
            }
            last = Some(at);
        }
    }
    Some(bytes)
}

impl<'a> PassHost<'a> {
    /// A host with nothing calculated, pending, kept or noted yet, on the defaults a pass starts
    /// from: [`PassHost::new`] and [`PassHost::worker`] start from it and set what differs, so a
    /// field is added in one place.
    fn blank(wb: &'a Workbook, exprs: &'a HashMap<Key, std::sync::Arc<Expr>>, graph: Option<&'a Graph>, rng: &'a mut u64, now: f64) -> Self {
        PassHost {
            wb,
            exprs,
            results: HashMap::default(),
            in_progress: HashSet::default(),
            spills: HashMap::default(),
            spill_index: SpillIndex::default(),
            used: HashMap::default(),
            rng,
            standing: None,
            now,
            cycle: false,
            cycles: vec![],
            depth: 0,
            suspend: None,
            waiting: HashSet::default(),
            abandoned: Vec::new(),
            also: Vec::new(),
            #[cfg(test)]
            suspensions: 0,
            #[cfg(test)]
            spill_visits: 0,
            cycle_seen: HashSet::default(),
            iterate: false,
            prev: HashMap::default(),
            stack: Vec::new(),
            members: HashSet::default(),
            tainted: HashSet::default(),
            taint: false,
            // Its own cache of parsed formats: rounding depends only on the workbook's styles.
            precision: wb.calc.precision_as_displayed.then(crate::DisplayedPrecision::new),
            stack_top: 0,
            out_of_stack: false,
            sums: HashMap::default(),
            python: HashMap::default(),
            names: HashMap::default(),
            name_lookup: NameLookup::default(),
            shared_names: None,
            names_text: 0,
            one_shot: false,
            level_names: None,
            #[cfg(test)]
            peak_names_text: 0,
            base: None,
            blocked: false,
            #[cfg(test)]
            blocked_walks: 0,
            #[cfg(test)]
            blocked_calls: 0,
            #[cfg(test)]
            blocked_reads: 0,
            #[cfg(test)]
            worker_calculated: 0,
            ranges: HashMap::default(),
            seen: HashSet::default(),
            seen_bytes: 0,
            #[cfg(test)]
            blocks_checked: 0,
            #[cfg(test)]
            block_walks: 0,
            cached_bytes: 0,
            cache_budget: 0,
            copies_dropped: false,
            #[cfg(test)]
            worker_levels: 0,
            fallbacks: 0,
            #[cfg(test)]
            handed_back: 0,
            #[cfg(test)]
            relaid_levels: 0,
            #[cfg(test)]
            on_main: 0,
            #[cfg(test)]
            redone: 0,
            marks_left: 0,
            #[cfg(test)]
            marks_given_up: 0,
            laid_out: Vec::new(),
            recording: false,
            graph,
            chunk: CHUNK,
            #[cfg(test)]
            fail_workers: false,
            #[cfg(test)]
            level_bytes: None,
            #[cfg(test)]
            peak_worker_bytes: 0,
            watch: false,
            watch_out: Watch::default(),
            note_pool: None,
            clock: 0,
            current: Vec::new(),
            outside_read: Vec::new(),
            stored: StoredAreas::default(),
            #[cfg(test)]
            copies_read: 0,
        }
    }

    pub(super) fn new(wb: &'a Workbook, exprs: &'a HashMap<Key, std::sync::Arc<Expr>>, graph: Option<&'a Graph>, rng: &'a mut u64, now: f64) -> Self {
        let mut host = PassHost::blank(wb, exprs, graph, rng, now);
        host.results = HashMap::with_capacity_and_hasher(exprs.len(), Default::default());
        host
    }

    /// Adds the dirty Python cells of the pass and their kept results: like the formulas, they
    /// are pending until laid out, so every read path calculates them before reading them.
    pub(super) fn with_python(mut self, python: HashMap<Key, Value>) -> Self {
        self.python = python;
        self
    }

    /// Indexes the areas the workbook stores for the arrays of the formulas and Python cells of a
    /// planned round (see `StoredAreas`): once a round, before anything is calculated, as the
    /// workbook doesn't change while it runs.
    pub(super) fn index_stored(&mut self) {
        let wb = self.wb;
        // The formulas and Python cells of the round (indexing an anchor again replaces its area).
        for k in self.exprs.keys().chain(self.python.keys()) {
            if let Some(&r) = wb.sheet(k.0).and_then(|s| s.spill_ranges.get(&k.1))
                && r.start != r.end
            {
                self.stored.index.insert_rect(*k, r, false);
                let last = self.stored.last.entry(k.0).or_insert(k.1);
                *last = (*last).max(k.1);
            }
        }
    }

    /// Whether the pass keeps the parsed definition of the name at `index` in the workbook's list.
    #[cfg(test)]
    pub(super) fn keeps_name(&self, index: usize) -> bool {
        self.names.contains_key(&index)
    }

    /// The blocks this worker holds copies of, by sheet and range.
    #[cfg(test)]
    pub(super) fn copied_blocks(&self) -> Vec<(usize, RangeRef)> {
        self.ranges.iter().flat_map(|(&sheet, held)| held.blocks.keys().map(move |&(from, to)| (sheet, RangeRef::new(from, to)))).collect()
    }

    /// A host for a worker thread of a multi-threaded level: it reads what this host has
    /// calculated so far and records its own results, to be merged back by
    /// [`PassHost::run_level`]. It never calculates a formula on demand (see
    /// [`PassHost::compute`]). Its own sums of constant blocks start
    /// empty (the main thread's are read through `base`), and the main thread's arrays are found
    /// through its own index (see `covering_spill`). It notes what dynamic formulas read (within
    /// what `run_level` lets the workers hold); when they ran is noted at the merge.
    pub(super) fn worker<'w>(&'w self, rng: &'w mut u64, cache_budget: usize) -> PassHost<'w> {
        let mut w = PassHost::blank(self.wb, self.exprs, self.graph, rng, self.now);
        // Which of two arrays gets the cells both want is decided as on the main thread.
        w.standing = self.standing;
        w.base = Some(self);
        w.cache_budget = cache_budget;
        w.watch = self.watch;
        w.watch_out.unknown = self.watch_out.unknown;
        w
    }

    /// Keeps a parsed definition for the rest of the pass while the definitions kept stay within
    /// [`MAX_PASS_NAME_TEXT`].
    fn keep_name(&mut self, i: usize, expr: Option<std::sync::Arc<Expr>>) {
        if self.names.contains_key(&i) {
            return;
        }
        let len = self.wb.names.get(i).map_or(0, |d| d.formula.len());
        let kept = self.names_text.saturating_add(len);
        if kept <= MAX_PASS_NAME_TEXT {
            self.names_text = kept;
            self.names.insert(i, expr);
        }
    }

    /// The result of `k` from this pass so far (a worker's own, or the main thread's).
    fn result(&self, k: &Key) -> Option<&Value> {
        self.results.get(k).or_else(|| self.base.and_then(|b| b.results.get(k)))
    }

    /// Whether `k` is a formula or Python cell of this pass: only such a cell is ever pending or
    /// has a result in it.
    fn in_pass(&self, k: &Key) -> bool {
        self.exprs.contains_key(k) || self.is_python(k)
    }

    /// Whether `k` is a formula or Python cell of this pass that isn't done yet: one without a
    /// result (on a worker, neither its own nor the main thread's). Its result is stored when it
    /// is done (`compute`, `python_result`, or a worker's merged at its turn in `run_level`), and
    /// taken away only where it is to be calculated again: by iterative calculation before each
    /// pass (`iterate_loops`), and by a worker for a result it can't keep, which it hands back.
    fn is_pending(&self, k: &Key) -> bool {
        self.in_pass(k) && self.result(k).is_none()
    }

    /// The array `k` spilled in this pass so far.
    fn spill(&self, k: &Key) -> Option<&std::sync::Arc<Array>> {
        self.spills.get(k).or_else(|| self.base.and_then(|b| b.spills.get(k)))
    }

    /// Whether `k` is a Python cell of this pass (a worker's are the main thread's).
    fn is_python(&self, k: &Key) -> bool {
        self.python.contains_key(k) || self.base.is_some_and(|b| b.python.contains_key(k))
    }

    /// Keeps `arr` as the array `k` spilled in this pass, indexed as `compute` keeps one.
    #[cfg(test)]
    pub(super) fn insert_spill(&mut self, k: Key, arr: std::sync::Arc<Array>) {
        self.spill_index.insert(k, arr.rows, arr.cols);
        self.spills.insert(k, arr);
    }

    /// The array of this pass that covers `c` (a worker's own, or the main thread's), found
    /// through each host's index; of several, the one anchored first in row-major order.
    fn covering_spill(&self, sheet: usize, c: CellRef) -> Option<(&Key, &std::sync::Arc<Array>)> {
        let own = first_covering(&self.spills, &self.spill_index, sheet, c);
        let theirs = self.base.and_then(|b| first_covering(&b.spills, &b.spill_index, sheet, c));
        match (own, theirs) {
            (Some(x), Some(y)) => Some(if y.0.1 < x.0.1 { y } else { x }),
            (x, y) => x.or(y),
        }
    }

    /// Whether an array of this pass (a worker's own, or the main thread's) meets `range`.
    fn spills_meet(&self, sheet: usize, range: RangeRef) -> bool {
        self.spill_index.meets_any(sheet, range) || self.base.is_some_and(|b| b.spill_index.meets_any(sheet, range))
    }
}

/// Of `spills` (indexed by `index`), the one covering `c` anchored first in row-major order.
fn first_covering<'s>(
    spills: &'s HashMap<Key, std::sync::Arc<Array>>,
    index: &SpillIndex,
    sheet: usize,
    c: CellRef,
) -> Option<(&'s Key, &'s std::sync::Arc<Array>)> {
    match index.covering(sheet, c) {
        Cover::None => None,
        Cover::One(a) => spills.get_key_value(&(sheet, a)),
        Cover::Many => {
            // The anchors come sorted, so the first that covers the cell is the one to read.
            let mut hits = Vec::new();
            index.meeting(sheet, RangeRef::new(c, c), &mut hits);
            hits.into_iter().find_map(|a| {
                #[cfg(test)]
                COVER_CHECKS.with(|n| n.set(n.get().saturating_add(1)));
                spills.get_key_value(&(sheet, a)).filter(|(_, arr)| spill_covers(a, arr, c))
            })
        }
    }
}

/// Adds to `out` the anchors other than `c` of the areas of `index` covering `c`.
fn anchors_over(index: &SpillIndex, sheet: usize, c: CellRef, out: &mut Vec<CellRef>) {
    match index.covering(sheet, c) {
        Cover::None => {}
        Cover::One(a) => {
            if a != c {
                out.push(a);
            }
        }
        Cover::Many => {
            let mut hits = Vec::new();
            index.meeting(sheet, RangeRef::new(c, c), &mut hits);
            out.extend(hits.into_iter().filter(|&a| a != c));
        }
    }
}

/// Whether an array at `anchor` covers `c`.
fn spill_covers(anchor: CellRef, arr: &Array, c: CellRef) -> bool {
    c.row >= anchor.row && c.col >= anchor.col && ((c.row - anchor.row) as usize) < arr.rows && ((c.col - anchor.col) as usize) < arr.cols
}

/// Whether an array result at `k` can't spill: it is in a table (a one-value result is shown
/// there, as in Excel), or its area runs off the sheet,
/// overlaps merged cells, holds other cells, or the spill of another formula that outranks it on
/// a cell they share (see `Standing`; unless that formula is recalculated in this pass, and so
/// may spill elsewhere now). A spill it outranks gives way to it when written back.
fn spill_blocked(host: &PassHost<'_>, (sheet, anchor): Key, a: &Array) -> bool {
    let Some(sh) = host.wb.sheet(sheet) else { return false };
    let end =
        CellRef::new(anchor.row.saturating_add((a.rows as u32).saturating_sub(1)), anchor.col.saturating_add((a.cols as u32).saturating_sub(1)));
    let range = RangeRef::new(anchor, end);
    let own = sh.spill_ranges.get(&anchor);
    let default = super::Standing::default();
    let standing = host.standing.unwrap_or(&default);
    let spilled_over = |c: CellRef| {
        !own.is_some_and(|o| o.contains(c))
            && sh.spill_ranges.iter().any(|(o, r)| {
                // A formula or Python cell recalculated in this pass lays its array out again.
                *o != anchor
                    && r.contains(c)
                    && !host.exprs.contains_key(&(sheet, *o))
                    && !host.is_python(&(sheet, *o))
                    && standing.outranks_at(((sheet, *o), Some(*r)), ((sheet, anchor), own.copied()), c)
            })
    };
    end.row >= gridcraft_core::MAX_ROWS
        || end.col >= gridcraft_core::MAX_COLS
        || sh.merges.iter().any(|m| m.intersects(&range))
        || (a.data.len() > 1 && sh.tables.iter().any(|t| t.range.contains(anchor)))
        || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (!cell.value.is_empty() || cell.formula.is_some()))
        || sh.spill.range(range.start..=range.end).any(|(c, _)| range.contains(*c) && spilled_over(*c))
}

/// Whether `arr`, anchored at `anchor`, would spill exactly what the sheet already holds over
/// `range`: the same area, nothing new in the way, and the same values.
pub(super) fn spills_as_before(sheet: &gridcraft_model::Sheet, anchor: CellRef, range: RangeRef, arr: &Array) -> bool {
    let end =
        CellRef::new(anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)), anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)));
    RangeRef::new(anchor, end) == range
        && !sheet.merges.iter().any(|m| m.intersects(&range))
        && (arr.data.len() <= 1 || !sheet.tables.iter().any(|t| t.range.contains(anchor)))
        && range
            .iter()
            .enumerate()
            .all(|(i, c)| c == anchor || (!sheet.cells.has(c) && sheet.spill.get(&c) == Some(arr.data.get(i).unwrap_or(&Value::Empty))))
}

/// The spilled values `sheet` stores inside `range`, passed to `f` until it returns false. Values
/// outside the range are not walked: with fewer rows in the range than spilled values, each row
/// is looked up on its own; otherwise one ordered scan visits each spilled value at most once.
/// Returns how many values were looked at.
pub(super) fn stored_spill_in<'s>(sheet: &'s gridcraft_model::Sheet, range: RangeRef, f: &mut dyn FnMut(CellRef, &'s Value) -> bool) -> usize {
    let mut seen = 0usize;
    if sheet.spill.is_empty() {
        return 0;
    }
    if (range.height() as usize) < sheet.spill.len() {
        for row in range.start.row..=range.end.row {
            for (c, v) in sheet.spill.range(CellRef::new(row, range.start.col)..=CellRef::new(row, range.end.col)) {
                seen += 1;
                if !f(*c, v) {
                    return seen;
                }
            }
        }
    } else {
        for (c, v) in sheet.spill.range(range.start..=range.end) {
            seen += 1;
            if range.contains(*c) && !f(*c, v) {
                return seen;
            }
        }
    }
    seen
}

/// Whether what `k` spills in this pass (`arr`, or nothing) reads as what the workbook stores
/// for it: an array laid out as before, or no array and no stored spill range. An array of one
/// cell is its anchor's value, which a reader gets from the formula itself wherever it is
/// calculated, so one laid out anew, or one stored and gone, changes nothing a reader sees.
pub(super) fn array_as_stored(wb: &Workbook, k: Key, arr: Option<&std::sync::Arc<Array>>) -> bool {
    match (wb.sheet(k.0).and_then(|s| s.spill_ranges.get(&k.1).map(|r| (s, *r))), arr) {
        (None, None) => true,
        (None, Some(a)) => a.rows == 1 && a.cols == 1,
        (Some((_, r)), None) => r.start == r.end,
        (Some((s, r)), Some(a)) => spills_as_before(s, k.1, r, a),
    }
}

impl PassHost<'_> {
    /// Calculates `k` (and the formulas it reads) with bounded stack: when [`Self::compute`] would
    /// go deeper than [`MAX_NESTED`], it suspends what is in progress, the formula it needed is
    /// calculated from here, then the suspended ones again (reading the results kept so far).
    pub(super) fn calculate(&mut self, k: Key) {
        // Formulas to calculate, innermost last, each with the ones abandoned to calculate the
        // next.
        let mut work: Vec<(Key, Vec<Key>)> = vec![(k, Vec::new())];
        while let Some(&(top, _)) = work.last() {
            self.compute(top);
            match self.suspend.take() {
                Some(next) => {
                    #[cfg(test)]
                    {
                        self.suspensions += 1;
                    }
                    let held = std::mem::take(&mut self.abandoned);
                    self.waiting.extend(held.iter().copied());
                    if let Some(entry) = work.last_mut() {
                        entry.1 = held;
                    }
                    // The other formulas read at the limit are calculated after `next`, before the
                    // held evaluations restart. Each has nothing held of its own, so the held ones
                    // are released only when the last of them is done.
                    let mut seen: HashSet<Key> = HashSet::default();
                    seen.insert(next);
                    for s in std::mem::take(&mut self.also) {
                        if seen.insert(s) {
                            work.push((s, Vec::new()));
                        }
                    }
                    work.push((next, Vec::new()));
                }
                None => {
                    work.pop();
                    if let Some(entry) = work.last_mut() {
                        for a in entry.1.drain(..) {
                            self.waiting.remove(&a);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn compute(&mut self, k: Key) -> Value {
        if let Some(v) = self.result(&k) {
            let v = v.clone();
            if self.iterate && self.tainted.contains(&k) {
                self.taint = true;
            }
            return v;
        }
        if let Some(result) = self.python.get(&k).cloned() {
            return self.python_result(k, result);
        }
        if self.base.is_some_and(|b| b.python.contains_key(&k)) {
            // A worker reads a Python cell the main thread hasn't laid out yet: the formula
            // reading it is handed back, as for a formula not done (the stored value may be stale).
            self.blocked = true;
            return Value::Error(CellError::Calc);
        }
        let Some(expr) = self.exprs.get(&k) else {
            return self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default();
        };
        if self.base.is_some() && self.depth > 0 {
            // A worker reads a formula that isn't done: the formula reading it is handed back to
            // the main thread rather than calculating it here (another worker may be on it, and
            // a chain of them would nest). The value is thrown away.
            self.blocked = true;
            return Value::Error(CellError::Calc);
        }
        if self.suspend.is_some() {
            // Unwinding to `calculate`: the value is thrown away. A formula read at the limit is
            // calculated with the suspended one, so the evaluations are not restarted for each.
            if self.depth >= MAX_NESTED && !self.in_progress.contains(&k) && !self.waiting.contains(&k) {
                self.also.push(k);
            }
            return Value::Error(CellError::Calc);
        }
        if self.in_progress.contains(&k) || self.waiting.contains(&k) {
            self.cycle = true;
            if self.iterate {
                return self.circular_read(k);
            }
            // Once each, though a suspended evaluation meets it again.
            if self.cycle_seen.insert(k) {
                self.cycles.push(k);
            }
            return Value::Error(CellError::Circ);
        }
        if self.depth >= MAX_NESTED {
            self.suspend = Some(k);
            return Value::Error(CellError::Calc);
        }
        if self.depth == 0 {
            self.stack_top = stack_position();
        }
        self.in_progress.insert(k);
        self.depth += 1;
        let outer_taint = std::mem::replace(&mut self.taint, false);
        if self.iterate {
            self.stack.push(k);
        }
        let expr = std::sync::Arc::clone(expr);
        let outer_out_of_stack = std::mem::replace(&mut self.out_of_stack, false);
        let started = self.tick();
        if self.watch {
            let dynamic = self.dynamic().is_some_and(|d| d.contains(&k));
            self.current.push((k, dynamic));
        }
        let v = {
            let mut ev = Evaluator::new(self, k.0, k.1);
            ev.value(&expr)
        };
        if self.watch {
            self.current.pop();
        }
        let out_of_stack = std::mem::replace(&mut self.out_of_stack, outer_out_of_stack);
        if out_of_stack && self.depth > 1 && self.suspend.is_none() {
            // It ran out of the stack left below the formulas that read it: it is calculated
            // from the top instead, as a formula needed at `MAX_NESTED` levels deep is.
            self.suspend = Some(k);
            self.depth -= 1;
            self.in_progress.remove(&k);
            if self.iterate {
                self.stack.pop();
            }
            self.taint = outer_taint;
            return Value::Error(CellError::Calc);
        }
        if self.suspend.is_some() {
            // Abandoned: `calculate` evaluates this formula again once the one it needs is done.
            self.depth -= 1;
            self.in_progress.remove(&k);
            self.abandoned.push(k);
            if self.iterate {
                self.stack.pop();
            }
            self.taint = outer_taint;
            return Value::Error(CellError::Calc);
        }
        if self.blocked {
            // A worker: the evaluation read a formula that isn't done, so `run_level` evaluates
            // it again on the main thread once the level is merged.
            self.depth -= 1;
            self.in_progress.remove(&k);
            self.taint = outer_taint;
            return Value::Error(CellError::Calc);
        }
        // A legacy array formula fills the range it was entered in.
        let legacy = self.wb.sheet(k.0).and_then(|s| s.cell(k.1)).and_then(|c| c.formula.as_ref()).and_then(|f| f.array);
        let v = match legacy {
            Some(range) => fit_to_range(v, range),
            None => v,
        };
        self.depth -= 1;
        self.in_progress.remove(&k);
        if self.iterate {
            self.stack.pop();
            if self.taint {
                self.tainted.insert(k);
            } else {
                self.tainted.remove(&k);
            }
        }
        self.taint |= outer_taint;
        // A blank cell shows as 0 in a formula's result, inside an array as well (`=A1:A3`,
        // FILTER or SORT of a range with blanks). GROUPBY and PIVOTBY lay out blank cells
        // of their own, which stay blank.
        let keep_blanks = matches!(&*expr, Expr::Call(n, _) if matches!(n.as_str(), "GROUPBY" | "PIVOTBY"));
        let v = match v {
            Value::Array(a) => {
                let a = if keep_blanks || !a.data.iter().any(|x| matches!(x, Value::Empty)) {
                    a
                } else {
                    let data = a.data.iter().map(|x| if matches!(x, Value::Empty) { Value::Number(0.0) } else { x.clone() }).collect();
                    Array::new(a.rows, a.cols, data).map(std::sync::Arc::new).unwrap_or(a)
                };
                // Precision as displayed: each element takes the format of the cell it lands in.
                let a = match self.precision.as_mut() {
                    Some(p) if a.data.iter().any(|x| matches!(x, Value::Number(_))) => {
                        let cols = a.cols.max(1);
                        let data = a
                            .data
                            .iter()
                            .enumerate()
                            .map(|(i, x)| {
                                let at = CellRef::new(k.1.row.saturating_add((i / cols) as u32), k.1.col.saturating_add((i % cols) as u32));
                                p.round_value(self.wb, k.0, at, x.clone())
                            })
                            .collect();
                        Array::new(a.rows, a.cols, data).map(std::sync::Arc::new).unwrap_or(a)
                    }
                    _ => a,
                };
                // A dynamic-array formula anchors a spill range even while its result is one
                // value -- a FILTER matching one row, a SORT or UNIQUE over one item -- so `C1#`
                // is the 1 x 1 range and `ROWS(C1#)` is 1, as in Excel (#1491). A formula that
                // returns a scalar never reaches this arm, so it does not become an anchor.
                // Copied blocks the new array lands in no longer hold what they read, so they go.
                self.drop_copies(k.0, RangeRef::new(k.1, spill_end(k.1, &a)));
                self.spill_index.insert(k, a.rows, a.cols);
                self.spills.insert(k, a.clone());
                a.data.first().cloned().unwrap_or_default()
            }
            Value::Empty => Value::Number(0.0),
            v @ Value::Number(_) => match self.precision.as_mut() {
                Some(p) => p.round_value(self.wb, k.0, k.1, v),
                None => v,
            },
            v => v,
        };
        // An array that can't spill is #SPILL! for the formulas evaluated after it in this pass too.
        let v = match self.spills.get(&k) {
            Some(a) if spill_blocked(self, k, a) => {
                self.spills.remove(&k);
                self.spill_index.remove(k);
                Value::Error(CellError::Spill)
            }
            _ => v,
        };
        self.results.insert(k, v.clone());
        self.note_time(k, started);
        if self.recording && !array_as_stored(self.wb, k, self.spills.get(&k)) {
            self.laid_out.push(k);
        }
        v
    }

    /// The graph's formulas whose references are only known while evaluating (INDIRECT, OFFSET,
    /// volatile): what they read is noted (see [`Watch::reads`]).
    fn dynamic(&self) -> Option<&HashSet<Key>> {
        self.graph.map(|g| &g.dynamic)
    }

    /// The next tick of the clock of `Watch::times` (on the main thread of a planned round).
    fn tick(&mut self) -> u64 {
        if self.watch && self.base.is_none() {
            self.clock = self.clock.saturating_add(1);
        }
        self.clock
    }

    /// Whether a note made now could be kept: in a planned round (`watch`), for a formula not
    /// handed back (`blocked`), while the round's reads are not unknown (`Watch::unknown`: then
    /// no note is kept, and the check reads none). Every place that does work only to make a
    /// note (a look-up, an anchor list, a time) asks this first; none of that work feeds a value,
    /// a budget or a hand-back.
    fn notes_wanted(&self) -> bool {
        self.watch && !self.blocked && !self.watch_out.unknown
    }

    /// Notes that `k` was calculated from `started` to now (see [`Watch::times`]).
    fn note_time(&mut self, k: Key, started: u64) {
        if self.notes_wanted() && self.base.is_none() {
            let done = self.tick();
            self.watch_out.times.push((k, started, done));
        }
    }

    /// Notes a read of `range` on `sheet` by the formula being evaluated, if it is dynamic (see
    /// [`Watch::reads`]). A read that makes one rectangle with the formula's entry before it
    /// (the same cells again, or the next ones down a column or along a row, as a loop over
    /// INDIRECT or OFFSET reads them) widens that entry, which then lists the same cells as the
    /// two would. Otherwise it takes one of the entries the round may note (`Watch::budget`)
    /// before it is stored; with none left, the reads noted so far are let go and the round's
    /// reads are unknown. On a worker, a formula being handed back notes nothing more (the main
    /// thread notes it at its turn), and one whose read finds no entry left is handed back.
    fn note_read(&mut self, sheet: usize, range: RangeRef) {
        let Some(&(k, true)) = self.current.last() else { return };
        if self.notes_wanted() {
            self.keep_note(k, sheet, range);
        }
    }

    /// Notes in `Watch::unforced` that the formula being evaluated, if `watch`, read cells of the
    /// array at `anchor` without calculating the anchor first, when it comes before the anchor
    /// in row-major order. A pair already noted is not noted again (see `Watch::paired`).
    fn note_unforced(&mut self, anchor: Key) {
        let Some(&(k, _)) = self.current.last() else { return };
        if k >= anchor || !self.notes_wanted() || self.watch_out.paired.contains(&(k, anchor)) {
            return;
        }
        if self.take_note_entry() {
            self.watch_out.unforced.push((k, anchor));
            self.watch_out.paired.insert((k, anchor));
        }
    }

    /// Notes in `Watch::unforced` the arrays over `c`, a cell the formula being evaluated reads on
    /// its own, if `watch`: each one laid out in this host's view that covers it (`laid`: there is
    /// one), and each formula of the round this view hasn't calculated yet whose stored area
    /// covers it. Of two arrays over one cell both are noted, though the read takes the value of
    /// one, so that what is noted doesn't depend on which of them this view has calculated.
    fn note_arrays_over(&mut self, sheet: usize, c: CellRef, laid: bool) {
        // Once the round's reads are unknown, `note_unforced` notes no pair: the arrays over the
        // cell are not looked up.
        if !self.notes_wanted() {
            return;
        }
        // An anchor comes before the cells of its array in row-major order, and a pair is noted
        // only for a reader before the anchor: a reader after `c` notes nothing.
        let Some(&(k, _)) = self.current.last() else { return };
        if k >= (sheet, c) {
            return;
        }
        #[cfg(test)]
        ARRAYS_OVER_LOOKUPS.with(|n| n.set(n.get().saturating_add(1)));
        let mut anchors = Vec::new();
        if laid {
            anchors_over(&self.spill_index, sheet, c, &mut anchors);
            if let Some(b) = self.base {
                anchors_over(&b.spill_index, sheet, c, &mut anchors);
            }
        }
        let stored = self.stored_areas();
        if stored.last.get(&sheet).is_some_and(|&last| k < (sheet, last)) {
            let mut held = Vec::new();
            anchors_over(&stored.index, sheet, c, &mut held);
            anchors.extend(held.into_iter().filter(|&a| self.is_pending(&(sheet, a))));
        }
        for a in anchors {
            self.note_unforced((sheet, a));
        }
    }

    /// On a worker: whether the block `key` was read before in this level (see `seen`). A block
    /// read for the first time is noted when the table has room for it, or when the worker's
    /// share of the bytes it may keep has room for the table to grow (by as much again as it
    /// holds, as a full hash table does), charged in `seen_bytes` once it has grown.
    fn read_before(&mut self, key: (usize, RangeRef)) -> bool {
        if self.seen.contains(&key) {
            return true;
        }
        let entry = std::mem::size_of::<(usize, RangeRef)>().saturating_add(1);
        let room = self.seen.capacity();
        if self.seen.len() >= room {
            let more = room.saturating_add(1).saturating_mul(entry);
            if self.cached_bytes.saturating_add(self.seen_bytes).saturating_add(more) > self.cache_budget {
                return false;
            }
        }
        self.seen.insert(key);
        let grown = self.seen.capacity().saturating_sub(room).saturating_mul(entry);
        if self.cached_bytes.saturating_add(self.seen_bytes).saturating_add(grown) > self.cache_budget {
            // The table grew by more than that (its first allocation takes several entries at
            // once): it goes, so that the share holds.
            self.seen = HashSet::default();
            self.seen_bytes = 0;
            return false;
        }
        self.seen_bytes = self.seen_bytes.saturating_add(grown);
        false
    }

    /// The stored areas of the round's arrays (see `StoredAreas`): on a worker, the main thread's.
    fn stored_areas(&self) -> &StoredAreas {
        self.base.map_or(&self.stored, |b| &b.stored)
    }

    /// Takes one of the entries the round may note (`Watch::budget`), on a worker from what the
    /// level's workers may hold between them, [`NOTE_BATCH`] at a time. With none left, `false`:
    /// on a worker the formula is handed back, on the main thread the notes so far are let go and
    /// the round's reads are unknown.
    fn take_note_entry(&mut self) -> bool {
        let watch = &mut self.watch_out;
        if watch.budget == 0
            && let Some(pool) = self.note_pool
        {
            use std::sync::atomic::Ordering::Relaxed;
            let mut left = pool.load(Relaxed);
            while left > 0 {
                let take = left.min(NOTE_BATCH);
                match pool.compare_exchange_weak(left, left.saturating_sub(take), Relaxed, Relaxed) {
                    Ok(_) => {
                        watch.budget = take;
                        break;
                    }
                    Err(now) => left = now,
                }
            }
        }
        if watch.budget == 0 {
            if self.note_pool.is_some() {
                // A worker: the level's workers hold all the entries they may between them. The
                // formula is handed back and noted on the main thread at its turn, where the
                // round's one count decides whether its reads are unknown (see `run_level`).
                self.blocked = true;
            } else {
                watch.unknown = true;
                watch.reads = Vec::new();
                watch.unforced = Vec::new();
                watch.paired = HashSet::default();
            }
            return false;
        }
        watch.budget -= 1;
        true
    }

    /// Notes that `k` read `range` on `sheet` (see [`PassHost::note_read`]): from its evaluation
    /// here, or at the merge of a worker's result. The range is noted in order, whichever way
    /// round its corners came, so that `joined` meets two ranges in order: `D300:D1` stored
    /// backward and joined with `D2` gave `D2:D2`, which lost the rest of the column.
    fn keep_note(&mut self, k: Key, sheet: usize, range: RangeRef) {
        if !self.notes_wanted() {
            return;
        }
        let range = RangeRef::new(range.start, range.end);
        let watch = &mut self.watch_out;
        if let Some(last) = watch.reads.last_mut()
            && last.0 == k
            && last.1 == sheet
            && let Some(both) = joined(last.2, range)
        {
            last.2 = both;
            return;
        }
        if self.take_note_entry() {
            self.watch_out.reads.push((k, sheet, range));
        }
    }

    /// Notes the reads of a worker's result kept at its turn (see `run_level`): the entries the
    /// worker ended with, one unit each, joined no further. The worker joined each read with the
    /// formula's entry before it as `keep_note` does, so these are the entries one thread ends
    /// with for the formula, and as a formula's entries only grow in number while it reads, they
    /// fit what is left here when one thread's would have fitted at each read. Joining them again
    /// could join entries one thread had to keep apart when it ran out (K1, then K3, then K2
    /// joining K3), and let a formula fit here that runs out on one thread.
    fn keep_worker_notes(&mut self, reads: &[(Key, usize, RangeRef)]) {
        if !self.notes_wanted() {
            return;
        }
        let watch = &mut self.watch_out;
        match watch.budget.checked_sub(reads.len()) {
            Some(left) => {
                watch.budget = left;
                watch.reads.extend_from_slice(reads);
            }
            None => {
                watch.unknown = true;
                watch.reads = Vec::new();
                watch.unforced = Vec::new();
                watch.paired = HashSet::default();
            }
        }
    }

    /// Notes `k`'s pairs of a worker's result kept at its turn (see `Watch::unforced`), one entry
    /// for each pair not noted already, as [`PassHost::keep_worker_notes`] notes its reads.
    fn keep_worker_pairs(&mut self, k: Key, anchors: Vec<Key>) {
        if !self.notes_wanted() {
            return;
        }
        let watch = &mut self.watch_out;
        let fresh: Vec<(Key, Key)> = anchors.into_iter().map(|a| (k, a)).filter(|&pair| watch.paired.insert(pair)).collect();
        match watch.budget.checked_sub(fresh.len()) {
            Some(left) => {
                watch.budget = left;
                watch.unforced.extend(fresh);
            }
            None => {
                watch.unknown = true;
                watch.reads = Vec::new();
                watch.unforced = Vec::new();
                watch.paired = HashSet::default();
            }
        }
    }

    /// Drops this worker's copied blocks on `sheet` that meet `area`, where an array was laid
    /// out. In each height class only the blocks starting from as far above the area as a block
    /// of that class can reach, down to its last row, are looked at (see [`SheetBlocks`]): any
    /// block starting higher ends above the area.
    fn drop_copies(&mut self, sheet: usize, area: RangeRef) {
        let Some(held) = self.ranges.get_mut(&sheet) else { return };
        let last = (CellRef::new(area.end.row, u32::MAX), CellRef::new(u32::MAX, u32::MAX));
        let mut met = Vec::new();
        let bands = SheetBlocks::bands(area);
        let area_band = *bands.start();
        for band in bands {
            for (&(_, class), set) in held.by_height.range((band, 0)..=(band, u32::MAX)) {
                let reach = u32::try_from((2u64 << class).saturating_sub(2)).unwrap_or(u32::MAX);
                let first = (CellRef::new(area.start.row.saturating_sub(reach), 0), CellRef::new(0, 0));
                for &(from, to) in set.range(first..=last) {
                    // A block in several of the area's bands is checked in the first of them.
                    if band != area_band.max(from.col / BAND_COLS) {
                        continue;
                    }
                    #[cfg(test)]
                    {
                        self.blocks_checked += 1;
                    }
                    if RangeRef::new(from, to).intersects(&area) {
                        met.push((from, to));
                    }
                }
            }
        }
        for block in met {
            if let Some(v) = held.remove(block) {
                self.cached_bytes = self.cached_bytes.saturating_sub(block_bytes(&v));
            }
        }
        if held.blocks.is_empty() {
            self.ranges.remove(&sheet);
        }
    }

    /// A Python cell's kept result as its value: an array spills from the cell unless it is
    /// blocked, malformed or larger than [`gridcraft_model::python::MAX_RESULT_CELLS`] (only a
    /// crafted file holds one), which shows `#SPILL!`.
    fn python_result(&mut self, k: Key, result: Value) -> Value {
        let v = match result {
            Value::Array(a) => {
                let sane = a.rows >= 1
                    && a.cols >= 1
                    && a.rows.checked_mul(a.cols).is_some_and(|n| n == a.data.len() && n <= gridcraft_model::python::MAX_RESULT_CELLS);
                if sane {
                    self.spill_index.insert(k, a.rows, a.cols);
                    self.spills.insert(k, a.clone());
                    a.data.first().cloned().unwrap_or_default()
                } else {
                    Value::Error(CellError::Spill)
                }
            }
            v => v,
        };
        let v = match self.spills.get(&k) {
            Some(a) if spill_blocked(self, k, a) => {
                self.spills.remove(&k);
                self.spill_index.remove(k);
                Value::Error(CellError::Spill)
            }
            _ => v,
        };
        self.results.insert(k, v.clone());
        let now = self.tick();
        self.note_time(k, now);
        // In a level run on workers, an array laid out unlike the stored one is met as a
        // formula's is (see `run_level`).
        if self.recording && !array_as_stored(self.wb, k, self.spills.get(&k)) {
            self.laid_out.push(k);
        }
        v
    }

    /// A formula's previous value for iterative calculation: from the last iteration, else the
    /// value it shows. Blank, and the `#CIRC!` of a loop calculated with iteration off, are 0.
    fn previous(&self, k: Key) -> Value {
        let v = match self.prev.get(&k) {
            Some(v) => v.clone(),
            None => self.wb.sheet(k.0).map(|s| s.value(k.1)).unwrap_or_default(),
        };
        if matches!(v, Value::Empty | Value::Error(CellError::Circ)) { Value::Number(0.0) } else { v }
    }

    /// Reading `k` while it is being evaluated, with iterative calculation on: its previous value.
    /// The formulas around the loop are the ones whose changes decide when iteration stops.
    fn circular_read(&mut self, k: Key) -> Value {
        self.taint = true;
        match self.stack.iter().rposition(|s| *s == k) {
            Some(i) => self.members.extend(self.stack.get(i..).unwrap_or_default().iter().copied()),
            // Too deep to follow: the formula itself is watched.
            None => {
                self.members.insert(k);
            }
        }
        self.previous(k)
    }

    /// Iterative calculation, after a first pass over `order` that met circular references. The
    /// formulas that read a previous value (the loops and what depends on them) are evaluated
    /// again in the same order, each pass starting from the last one's values and reading what it
    /// has already produced (Gauss-Seidel), until no formula on a loop changes by `max_change` or
    /// more, or after `max_iterations` passes in all. Returns the number of passes.
    pub(super) fn iterate_loops(&mut self, order: &[Key], max_iterations: u32, max_change: f64) -> u32 {
        let again: Vec<Key> = order.iter().copied().filter(|k| self.tainted.contains(k)).collect();
        let set: HashSet<Key> = again.iter().copied().collect();
        // Bounds the work whatever Maximum Iterations says.
        let max_passes = u32::try_from(MAX_ITERATION_WORK / again.len().max(1)).unwrap_or(u32::MAX).max(1);
        let mut passes = 1u32;
        while passes < max_iterations.min(max_passes) && !self.converged(max_change) {
            // Without their results they are pending again (see `is_pending`): a formula that
            // reads one before its turn calculates it first, as in the first pass.
            self.prev = again.iter().filter_map(|k| self.results.remove(k).map(|v| (*k, v))).collect();
            let gone: Vec<Key> = self.spills.keys().filter(|a| set.contains(a)).copied().collect();
            for a in gone {
                self.spills.remove(&a);
                self.spill_index.remove(a);
            }
            for k in &again {
                self.calculate(*k);
            }
            passes += 1;
        }
        passes
    }

    /// Whether every formula on a loop changed by less than `max_change` in the last pass (text,
    /// logical and error values: not at all).
    fn converged(&self, max_change: f64) -> bool {
        self.members.iter().all(|k| {
            let Some(new) = self.results.get(k) else { return true };
            match (new, &self.previous(*k)) {
                (Value::Number(a), Value::Number(b)) => {
                    let d = (a - b).abs();
                    d < max_change || d == 0.0
                }
                (new, old) => new == old,
            }
        })
    }

    /// Calculates one level of a [`Plan`]: formulas that read no other formula of the level
    /// (statically). A small level, or one thread, calculates them on this thread in order.
    /// Otherwise worker threads take the formulas in chunks, each evaluating with its own host
    /// over this one's results so far; a formula that turns out to read a formula not done yet
    /// (through INDIRECT, OFFSET, a spill-range operator on a pending anchor, a name whose
    /// definition reads one) is handed back, and formulas that draw random numbers stay here, so
    /// the one generator draws in order. The level is then walked in row-major order, the order
    /// one thread calculates it in: a worker's result is merged at its turn, and a formula that
    /// runs here is calculated at its turn with the usual on-demand machinery.
    ///
    /// A worker read what this host held when the workers started, and its own results so far.
    /// That differs from what one thread reads at the same turn only by the formulas before it
    /// in the level, which it doesn't read (the level's formulas don't read one another), and by
    /// their arrays where those differ from what the workbook stores (in place, shape or values,
    /// or lost). So whenever the walk meets such an array (merged, laid out here, or a worker's
    /// that is thrown away), the formulas after it whose references meet its new area (the
    /// planner already holds the readers of the stored one back to a later level), and every
    /// dynamic formula after it (INDIRECT, OFFSET, a volatile function: what they read
    /// isn't known), are calculated here at their turn instead of merged. The readers of a new
    /// area are looked up through the graph within `marks_left`; once a look-up runs out of it,
    /// every formula after that array in the level is calculated here at its turn. Every formula
    /// is calculated over the state one thread would have had at its turn, and a level costs one
    /// worker pass. If a thread can't start or fails, the level is calculated on this thread.
    ///
    /// The reads a kept worker result noted are charged to this host at its turn in the walk, and
    /// a formula calculated here is noted as it runs. A formula's notes, and their count against
    /// the limit, are the same whether it was calculated here or on a worker and merged: a read
    /// joins only its own formula's entry before it, and a join adds no entry, so a worker's
    /// entries for a formula are the ones this host would have ended with, and they are kept as
    /// they are (`keep_worker_notes`). The round's notes are then one thread's, one set per
    /// formula from the calculation that is kept, and so is whether they passed the limit
    /// (`Watch::unknown`), whatever the thread count and timing. A worker that finds no entry
    /// left among what the workers may hold (taken by results thrown away later, or by others
    /// for a moment) hands its formula back, which changes only the thread it is calculated on.
    pub(super) fn run_level(&mut self, level: &[Key], threads: usize, parallel_min: usize) {
        if threads <= 1 || level.len() < parallel_min {
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        let exprs = self.exprs;
        let wb = self.wb;
        #[cfg(test)]
        let fail = self.fail_workers;
        let t0 = prof_now();
        let chunk_size = self.chunk.max(1);
        // Formulas that draw random numbers stay here, and so do Python cells, which lay out a
        // kept result rather than evaluate. Too few left for the workers: none are started.
        let graph = self.graph;
        let parallel: Vec<Key> = level.iter().copied().filter(|k| exprs.get(k).is_some_and(|e| !draws_random(graph, *k, e))).collect();
        if parallel.is_empty() || parallel.len() < parallel_min {
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        // The workers find names through this host's index, built here once for the pass when the
        // level's formulas name more than the pass has left to scan (see `NameLookup::expect`),
        // rather than each building one. A level naming a few, or none, leaves a workbook of many
        // names unindexed: its workers scan for those few, and when the definitions they reach
        // name more, the first worker past its scans builds one index the others share
        // (`shared_names`), which this host adopts after the level. At most one index is built
        // in a round, however the lookups are spread over threads and levels. A workbook with no
        // defined names has nothing to index, and its formulas are not walked to count lookups.
        if self.name_lookup.index().is_none() && !wb.names.is_empty() {
            self.name_lookup.expect(wb, level_name_lookups(graph, exprs, &parallel));
        }
        let shared_names = std::sync::OnceLock::new();
        let workers = threads.min(parallel.len().div_ceil(chunk_size));
        // The workers the level starts share the cache budget.
        let level_bytes = CACHE_BYTES_BUDGET;
        #[cfg(test)]
        let level_bytes = self.level_bytes.unwrap_or(level_bytes);
        let cache_budget = level_bytes / workers.max(1);
        // The workers keep the names they parse in one store, within what this host has left.
        let level_names = LevelNames::new(MAX_PASS_NAME_TEXT.saturating_sub(self.names_text));
        // The workers note reads within what this host may still note (see `Watch::budget`),
        // between them: at most that many entries are held on the workers, whatever their number
        // (40 bytes each, and a span per formula that noted any). Each worker records which of
        // its entries are whose, and the walk below charges the entries of each result it keeps
        // to this host at its turn, as one thread would have noted them then.
        let note_pool = std::sync::atomic::AtomicUsize::new(self.watch_out.budget);
        let seed = *self.rng;
        let next = std::sync::atomic::AtomicUsize::new(0);
        let me: &PassHost<'_> = &*self;
        let work = || {
            #[cfg(test)]
            #[allow(clippy::panic)]
            if fail {
                panic!("worker failure injected by a test");
            }
            let mut rng = seed;
            let mut w = me.worker(&mut rng, cache_budget);
            w.note_pool = Some(&note_pool);
            w.shared_names = Some(&shared_names);
            w.level_names = Some(&level_names);
            let mut deferred = Vec::new();
            let mut spans = Vec::new();
            let mut taken = Vec::new();
            // About its share of the level, so that the map isn't grown again and again as it fills.
            w.results.reserve((parallel.len() / workers.max(1)).saturating_add(chunk_size));
            // Bytes of the results it keeps, within `cached_bytes` (the rest is its copies).
            let mut kept = 0usize;
            #[cfg(test)]
            let mut calculated = 0usize;
            loop {
                let i = next.fetch_add(chunk_size, std::sync::atomic::Ordering::Relaxed);
                let Some(chunk) = parallel.get(i..parallel.len().min(i.saturating_add(chunk_size))) else { break };
                if chunk.is_empty() {
                    break;
                }
                taken.push(i);
                // Set when a result doesn't fit in what this worker may still keep: the rest of the
                // chunk is handed back without being calculated. An array or text result among them
                // would not fit either and would be calculated again on the main thread; a number
                // would fit, but which they are is known only once they are calculated.
                let mut full = false;
                for &k in chunk {
                    if full {
                        deferred.push(k);
                        continue;
                    }
                    w.blocked = false;
                    let noted = w.watch_out.reads.len();
                    let paired = w.watch_out.unforced.len();
                    // A worker calculates no formula but `k` (see `compute`), so its arrays grow by
                    // one exactly when `k` lays one out.
                    let arrays = w.spills.len();
                    let value = w.compute(k);
                    #[cfg(test)]
                    {
                        calculated += 1;
                    }
                    let count = w.watch_out.reads.len().saturating_sub(noted);
                    // Its array and text are kept until the walk, which may throw them away, so
                    // they count against this worker's share of the bytes it may keep, with its
                    // copied blocks. When the copies are what leave no room, they go rather than
                    // the result (the store still holds what they copied, while a formula handed
                    // back is calculated twice), and the worker copies no more in this level. Past
                    // the share, the formula is handed back with neither: the main thread
                    // calculates it at its turn, as one thread does.
                    if !w.blocked {
                        let laid = w.spills.len() != arrays;
                        // Measured up to the room it would have without its copies: `None`, it
                        // doesn't fit even then.
                        let held = if laid || matches!(value, Value::Text(_)) {
                            let room = w.cache_budget.saturating_sub(kept);
                            held_bytes(Some(&value), w.spills.get(&k).filter(|_| laid).map(|a| &**a), room)
                        } else {
                            Some(0)
                        };
                        if held != Some(0) {
                            if let Some(held) = held
                                && w.cached_bytes.saturating_add(w.seen_bytes).saturating_add(held) > w.cache_budget
                                && (!w.ranges.is_empty() || w.seen_bytes > 0)
                            {
                                w.ranges.clear();
                                w.seen = HashSet::default();
                                w.seen_bytes = 0;
                                w.cached_bytes = kept;
                                w.copies_dropped = true;
                            }
                            if let Some(held) = held
                                && w.cached_bytes.saturating_add(w.seen_bytes).saturating_add(held) <= w.cache_budget
                            {
                                w.cached_bytes += held;
                                kept += held;
                            } else {
                                w.results.remove(&k);
                                if w.spills.remove(&k).is_some() {
                                    w.spill_index.remove(k);
                                }
                                w.blocked = true;
                                full = true;
                            }
                        }
                    }
                    if w.blocked {
                        // Handed back: the main thread notes it at its turn. What it noted here
                        // goes, and the entries it took serve the formulas after it.
                        deferred.push(k);
                        w.watch_out.reads.truncate(noted);
                        let mut pairs = 0usize;
                        let watch = &mut w.watch_out;
                        for pair in watch.unforced.drain(paired.min(watch.unforced.len())..) {
                            watch.paired.remove(&pair);
                            pairs += 1;
                        }
                        watch.budget = watch.budget.saturating_add(count).saturating_add(pairs);
                    } else if count > 0 {
                        spans.push((k, noted, count));
                    }
                }
            }
            // A worker never finds a cycle: reading a formula not done hands the reader back
            // before the in-progress check, so `w.cycles` stays empty.
            WorkerOut {
                reads: std::mem::take(&mut w.watch_out.reads),
                spans,
                unforced: std::mem::take(&mut w.watch_out.unforced),
                results: w.results,
                spills: w.spills,
                taken,
                deferred,
                used: w.used,
                sums: w.sums,
                #[cfg(test)]
                block_walks: w.block_walks,
                #[cfg(test)]
                copies_read: w.copies_read,
                #[cfg(test)]
                blocked_walks: w.blocked_walks,
                #[cfg(test)]
                blocked_calls: w.blocked_calls,
                #[cfg(test)]
                blocked_reads: w.blocked_reads,
                #[cfg(test)]
                calculated,
                // A worker thread starts with its own count at nothing.
                #[cfg(test)]
                names_parsed: NAMES_PARSED.with(std::cell::Cell::get),
                #[cfg(test)]
                name_indexes_built: crate::eval::NAME_INDEXES_BUILT.with(std::cell::Cell::get),
            }
        };
        let mut outs: Vec<WorkerOut> = Vec::with_capacity(threads);
        let mut failed = false;
        std::thread::scope(|s| {
            let mut handles = Vec::with_capacity(threads);
            for _ in 0..workers {
                match std::thread::Builder::new().name("gridcraft-calc".into()).stack_size(WORKER_STACK).spawn_scoped(s, work) {
                    Ok(h) => handles.push(h),
                    Err(_) => break,
                }
            }
            failed = handles.is_empty();
            for h in handles {
                match h.join() {
                    Ok(out) => outs.push(out),
                    Err(_) => failed = true,
                }
            }
        });
        if let Some(index) = shared_names.into_inner() {
            self.name_lookup.adopt(index);
        }
        // The definitions the workers parsed, kept for the levels after this one: they fit in
        // what this host had left.
        #[cfg(test)]
        {
            let held = self.names_text.saturating_add(level_names.text());
            self.peak_names_text = self.peak_names_text.max(held);
        }
        for (i, expr) in level_names.into_parsed() {
            self.keep_name(i, expr);
        }
        if failed {
            if self.fallbacks == 0 {
                log::warn!("calculation threads could not run; calculating on one thread");
            }
            self.fallbacks += 1;
            for &k in level {
                self.calculate(k);
            }
            return;
        }
        #[cfg(test)]
        {
            self.worker_levels += 1;
        }
        // What the workers found, and the formulas they handed back. Each worker's results stay in
        // its own map, found through the worker that took the formula's chunk (`owner`, by chunk
        // of `parallel`), rather than all being copied into one map.
        let mut results: Vec<HashMap<Key, Value>> = Vec::with_capacity(outs.len());
        let mut owner: Vec<Option<usize>> = vec![None; parallel.len().div_ceil(chunk_size)];
        let mut spills: HashMap<Key, std::sync::Arc<Array>> = HashMap::default();
        let mut handed = 0usize;
        // What each worker noted, and for each formula it has a result for, where its entries
        // are: (worker, first entry, entries).
        let mut worker_reads: Vec<Vec<(Key, usize, RangeRef)>> = Vec::with_capacity(outs.len());
        let mut noted: HashMap<Key, (usize, usize, usize)> = HashMap::default();
        // And the arrays each read without calculating the anchor (`Watch::unforced`).
        let mut unforced: HashMap<Key, Vec<Key>> = HashMap::default();
        #[cfg(test)]
        {
            let held: usize = outs
                .iter()
                .flat_map(|out| {
                    out.results.iter().map(|(k, v)| held_bytes(Some(v), out.spills.get(k).map(|a| &**a), usize::MAX).unwrap_or(usize::MAX))
                })
                .sum();
            self.peak_worker_bytes = self.peak_worker_bytes.max(held);
        }
        for (o, out) in outs.into_iter().enumerate() {
            for (reader, anchor) in out.unforced {
                unforced.entry(reader).or_default().push(anchor);
            }
            // A worker spills only for the formulas it has a result for (a formula it hands back
            // stops before its array is kept).
            spills.extend(out.spills);
            for &i in &out.taken {
                if let Some(slot) = owner.get_mut(i / chunk_size) {
                    *slot = Some(o);
                }
            }
            results.push(out.results);
            noted.extend(out.spans.into_iter().map(|(k, first, count)| (k, (o, first, count))));
            worker_reads.push(out.reads);
            handed += out.deferred.len();
            self.used.extend(out.used);
            // The block sums the workers worked out, for the levels after this one. A sum is the
            // same whichever thread worked it out (see `range_sum`), so the order of the merge
            // doesn't matter, and the main thread keeps one per block as it would alone. A block
            // summed only by a worker result thrown away in the walk below is kept as well, which
            // one thread wouldn't have summed: a fixed size per block.
            for (key, rows) in out.sums {
                let known = self.sums.entry(key).or_default();
                for (last, r) in rows {
                    known.entry(last).or_insert(r);
                }
            }
            #[cfg(test)]
            {
                self.block_walks += out.block_walks;
                self.copies_read += out.copies_read;
                self.blocked_walks += out.blocked_walks;
                self.blocked_calls += out.blocked_calls;
                self.blocked_reads += out.blocked_reads;
                self.worker_calculated += out.calculated;
                NAMES_PARSED.with(|n| n.set(n.get().saturating_add(out.names_parsed)));
                crate::eval::NAME_INDEXES_BUILT.with(|n| n.set(n.get().saturating_add(out.name_indexes_built)));
            }
        }
        #[cfg(test)]
        {
            self.handed_back += handed;
        }
        if std::env::var_os("GRIDCRAFT_PROFILE").is_some() {
            eprintln!("  level of {}: {} handed back, {:.1} ms on workers", level.len(), handed, prof_now() - t0);
        }
        // In row-major order: a formula calculated here (with on-demand reads, random draws and
        // arrays laid out as on one thread), or a worker's result merged in. `again`: the
        // formulas after an array laid out differently from what is stored that may have read its
        // cells; `every_dynamic`: such an array has been met, so every dynamic formula after it
        // runs here too. One of the places the stale-reader rule is enforced; see `Calc::run`.
        let mut again = Stale { readers: HashSet::default(), left: self.marks_left, all: false };
        let mut every_dynamic = false;
        #[cfg(test)]
        let mut redone = 0usize;
        // `parallel` is the level in its order without the formulas kept here, so the walk meets
        // its formulas one after the other: the next one's place in it.
        let mut position = 0usize;
        for &k in level {
            let worker_array = if spills.is_empty() { None } else { spills.remove(&k) };
            let worker_result = if parallel.get(position) == Some(&k) {
                let taken_by = owner.get(position / chunk_size).copied().flatten();
                position += 1;
                taken_by.and_then(|o| results.get_mut(o)).and_then(|r| r.remove(&k))
            } else {
                None
            };
            let thrown = |host: &Self, again: &mut Stale| {
                // A worker's array that is thrown away: the formulas after it on that worker may
                // have read it. (An array that differs from what is stored came from a state that
                // differed from one thread's, so an array laid out differently has been met before
                // it and the dynamic formulas after it are already being calculated again.)
                if let Some(a) = &worker_array
                    && !array_as_stored(wb, k, Some(a))
                {
                    host.mark_readers(k, Some(a), again, Marked::Thrown);
                }
            };
            if self.results.contains_key(&k) {
                // Calculated on demand by a formula before it.
                thrown(self, &mut again);
                continue;
            }
            let dynamic = self.dynamic().is_none_or(|d| d.contains(&k));
            let read_stale = again.holds(&k) || (every_dynamic && dynamic);
            // A formula handed back has no worker result.
            if read_stale || worker_result.is_none() {
                #[cfg(test)]
                if read_stale && worker_result.is_some() {
                    redone += 1;
                }
                thrown(self, &mut again);
                #[cfg(test)]
                {
                    self.on_main += 1;
                }
                self.laid_out.clear();
                self.recording = true;
                self.calculate(k);
                self.recording = false;
                for a in std::mem::take(&mut self.laid_out) {
                    self.mark_readers(a, self.spills.get(&a), &mut again, Marked::Kept);
                    every_dynamic = true;
                }
            } else if let Some(v) = worker_result {
                self.results.insert(k, v);
                // What it read, noted at its turn as one thread notes it, against the one count.
                if let Some(&(o, first, count)) = noted.get(&k)
                    && let Some(reads) = worker_reads.get(o).and_then(|r| r.get(first..first.saturating_add(count)))
                {
                    self.keep_worker_notes(reads);
                }
                if let Some(anchors) = unforced.remove(&k) {
                    self.keep_worker_pairs(k, anchors);
                }
                let turn = self.tick();
                self.note_time(k, turn);
                if let Some(a) = worker_array {
                    self.spill_index.insert(k, a.rows, a.cols);
                    self.spills.insert(k, a);
                }
                if !array_as_stored(wb, k, self.spills.get(&k)) {
                    self.mark_readers(k, self.spills.get(&k), &mut again, Marked::Kept);
                    every_dynamic = true;
                }
            }
        }
        self.marks_left = again.left;
        #[cfg(test)]
        {
            self.relaid_levels += usize::from(redone > 0);
            self.redone += redone;
            self.marks_given_up += usize::from(again.all);
        }
    }

    /// Adds to `again` the formulas whose references meet the area `k`'s array covers (`arr`).
    /// Not the area the workbook stores for it: the planner holds its readers back to a later
    /// level. Without the graph, none: then every formula counts as dynamic, and all of them
    /// after it run again anyway.
    ///
    /// The look-up is charged to `again.left` (the budget rule: see `index_budget`), and stops
    /// as soon as nothing is left. Its per-cell and column probes (`Graph::uncharged_probes`) are
    /// charged before it for an array that is thrown away (`Marked::Thrown`), as the planner
    /// charges them: such an array never reaches `write_back`, so a level could otherwise walk
    /// the whole per-cell index once for each. For an array that is kept (`Marked::Kept`) they
    /// are not: when the round is kept, `write_back` probes the cells of an area holding this
    /// one for each changed array. When the look-up runs out, or nothing is left before it,
    /// every formula after this array in the level is taken as stale (`again.all`): calculated
    /// here at its turn, as after `every_dynamic`, so the values are one thread's either way.
    fn mark_readers(&self, k: Key, arr: Option<&std::sync::Arc<Array>>, again: &mut Stale, marked: Marked) {
        let Some(graph) = self.graph else { return };
        let Some(a) = arr else { return };
        if again.all {
            return;
        }
        let area = RangeRef::new(k.1, spill_end(k.1, a));
        // The look-up's own unit, and for a thrown array the probes it doesn't charge as it goes.
        let ahead = match marked {
            Marked::Kept => 1,
            Marked::Thrown => graph.uncharged_probes(area).saturating_add(1),
        };
        let Some(mut left) = again.left.checked_sub(ahead) else {
            again.left = 0;
            again.all = true;
            return;
        };
        #[cfg(test)]
        MARKING_PROBES.with(|n| n.set(n.get().saturating_add(graph.uncharged_probes(area))));
        let mut deps = Vec::new();
        if graph.dependents_of_range_within(k.0, area, &mut deps, &mut left) {
            again.left = left;
            again.readers.extend(deps);
        } else {
            again.left = 0;
            again.all = true;
        }
    }
}

/// Whether the array `PassHost::mark_readers` looks up the readers of is kept (merged at its turn
/// or laid out on the main thread) or thrown away with its worker's result.
#[derive(Clone, Copy)]
enum Marked {
    Kept,
    Thrown,
}

/// The formulas of a level calculated on worker threads that may have read, before it was laid
/// out, an array laid out differently from what the workbook stores (see
/// [`PassHost::run_level`]).
struct Stale {
    /// Found through the graph (see [`PassHost::mark_readers`]).
    readers: HashSet<Key>,
    /// The index work the look-ups may still cost this round (see `PassHost::marks_left`).
    left: usize,
    /// A look-up ran out of `left` in this level: every formula after that is stale.
    all: bool,
}

impl Stale {
    fn holds(&self, k: &Key) -> bool {
        self.all || self.readers.contains(k)
    }
}

/// What a worker thread worked out on one level.
struct WorkerOut {
    /// What its dynamic formulas read (see [`Watch::reads`]), and for each formula it has a result
    /// for that noted any: the formula, its first entry in `reads` and how many it has.
    reads: Vec<(Key, usize, RangeRef)>,
    spans: Vec<(Key, usize, usize)>,
    /// Its `Watch::unforced` notes, kept for the results merged at their turn.
    unforced: Vec<(Key, Key)>,
    #[cfg(test)]
    copies_read: usize,
    results: HashMap<Key, Value>,
    spills: HashMap<Key, std::sync::Arc<Array>>,
    /// Where each chunk it took starts in the level's list for the workers (see `run_level`):
    /// its results are looked up in its own map by these.
    taken: Vec<usize>,
    /// Formulas that read a formula not done yet: the main thread calculates them.
    deferred: Vec<Key>,
    /// Definitions it parsed (see [`NAMES_PARSED`]), counted to the main thread after the level.
    #[cfg(test)]
    names_parsed: usize,
    /// Indexes of names it built (see `crate::eval::NAME_INDEXES_BUILT`), counted to the main
    /// thread after the level.
    #[cfg(test)]
    name_indexes_built: usize,
    used: HashMap<usize, Option<RangeRef>>,
    sums: HashMap<(usize, CellRef, u32), BlockSums>,
    #[cfg(test)]
    block_walks: usize,
    /// Walks it made for formulas it had decided to hand back (see `PassHost::blocked_walks`).
    #[cfg(test)]
    blocked_walks: usize,
    /// Volatile and dynamic-reference functions it called for them (see `blocked_calls`).
    #[cfg(test)]
    blocked_calls: usize,
    /// Reads and draws it was asked for them (see `blocked_reads`).
    #[cfg(test)]
    blocked_reads: usize,
    /// Formulas it calculated (see `worker_calculated`).
    #[cfg(test)]
    calculated: usize,
}

/// A defined name's definition, parsed (`None` when it doesn't parse).
fn parse_definition(text: &str) -> Option<std::sync::Arc<Expr>> {
    #[cfg(test)]
    NAMES_PARSED.with(|n| n.set(n.get().saturating_add(1)));
    gridcraft_formula::parse_name(text).ok().map(std::sync::Arc::new)
}

/// A parsed definition the workers of a level share: parsed by the first to need it, which the
/// others wait for.
type SharedDefinition = std::sync::Arc<std::sync::OnceLock<Option<std::sync::Arc<Expr>>>>;

/// The parsed definitions the workers of a level share, so that a name is parsed once in the
/// level whatever the thread count, and the workers together keep no more definition text than
/// the main thread has left of [`MAX_PASS_NAME_TEXT`]. A definition's text is charged before it
/// is parsed; past the budget, a worker parses it at each use, as one thread does past the
/// bound. Every parse of a text gives the same tree, so which worker parses a definition first,
/// or which definitions the budget takes when it runs out, changes the cost alone. The main
/// thread keeps them after the level (see `run_level`).
struct LevelNames {
    /// The bytes of definition text charged, and the definitions by their place in the list.
    kept: std::sync::Mutex<(usize, HashMap<usize, SharedDefinition>)>,
    budget: usize,
}

impl LevelNames {
    fn new(budget: usize) -> LevelNames {
        LevelNames { kept: std::sync::Mutex::new((0, HashMap::default())), budget }
    }

    /// The shared place of the definition at `position`, `len` bytes long, charged to the level
    /// when it is the first ask; `None` past the budget, or when a worker panicked holding the
    /// store (the level then falls back to the main thread).
    fn slot(&self, position: usize, len: usize) -> Option<SharedDefinition> {
        let mut kept = self.kept.lock().ok()?;
        let (text, slots) = &mut *kept;
        if let Some(slot) = slots.get(&position) {
            return Some(slot.clone());
        }
        *text = text.checked_add(len).filter(|t| *t <= self.budget)?;
        let slot = SharedDefinition::default();
        slots.insert(position, slot.clone());
        Some(slot)
    }

    /// The definition text charged.
    #[cfg(test)]
    fn text(&self) -> usize {
        self.kept.lock().map_or(0, |k| k.0)
    }

    /// The definitions parsed, by their place in the list.
    fn into_parsed(self) -> impl Iterator<Item = (usize, Option<std::sync::Arc<Expr>>)> {
        let slots = self.kept.into_inner().map(|k| k.1).unwrap_or_default();
        slots.into_iter().filter_map(|(i, slot)| slot.get().map(|expr| (i, expr.clone())))
    }
}

/// The name lookups the formulas at `keys` may make (see [`name_lookups`]), for
/// `NameLookup::expect` in `PassHost::run_level`. The count stops once it passes
/// `NAME_LOOKUPS_SCANNED`, more than a pass can have left to scan: past that `expect` builds the
/// index whatever the rest would add, so the formulas after it are not looked at. A formula's
/// count is its graph node's (`Node::name_lookups`, capped at one past it, which decides the
/// same), else a walk of its tree.
fn level_name_lookups(graph: Option<&Graph>, exprs: &HashMap<Key, std::sync::Arc<Expr>>, keys: &[Key]) -> usize {
    let cap = crate::eval::NAME_LOOKUPS_SCANNED;
    let mut lookups = 0usize;
    for k in keys {
        let Some(e) = exprs.get(k) else { continue };
        let walked = || {
            #[cfg(test)]
            NAME_LOOKUP_WALKS.with(|n| n.set(n.get().saturating_add(1)));
            name_lookups(e, cap - lookups)
        };
        lookups = lookups.saturating_add(counted_lookups(graph, *k, e).unwrap_or_else(walked));
        if lookups > cap {
            break;
        }
    }
    #[cfg(test)]
    {
        let all = keys.iter().filter_map(|k| exprs.get(k)).fold(0usize, |n, e| n.saturating_add(name_lookups(e, usize::MAX)));
        assert!(if all <= cap { lookups == all } else { lookups > cap }, "{lookups} name lookups counted of {all}");
    }
    lookups
}

/// [`name_lookups`] for the formula at `k` evaluated as `e`, as its graph node counted it when the
/// node was built from this same tree (see `Node::name_lookups`), else `None`.
fn counted_lookups(graph: Option<&Graph>, k: Key, e: &std::sync::Arc<Expr>) -> Option<usize> {
    let node = graph.and_then(|g| g.nodes.get(&k))?;
    let tree = node.formula.as_ref().and_then(|f| f.expr.as_ref())?;
    if !std::sync::Arc::ptr_eq(tree, e) {
        return None;
    }
    node.name_lookups.map(usize::from)
}

/// The places in `e` that may look a defined name up when it is evaluated once (see
/// `Evaluator::name`): names and calls of names not built in, and INDIRECT, which may read a name
/// from text. A name's definition is reached only through one of these, so a formula without any
/// looks no name up. A LET variable or LAMBDA parameter in scope is not one: the evaluator finds
/// it among the bindings before it looks at the workbook's names. Scopes as the precedent walk
/// (`Walk::collect`) and the evaluator bind them: a LET variable for the values after it and the
/// result, a LAMBDA's parameters for its body, names compared case-folded. The walk stops once
/// the count passes `stop_past`.
pub(crate) fn name_lookups(e: &Expr, stop_past: usize) -> usize {
    enum Step<'e> {
        Visit(&'e Expr),
        Bind(&'e str),
        Unbind(usize),
    }
    let mut found = 0usize;
    // The folded spellings in scope, innermost last, and how often each stands there.
    let mut stack: Vec<String> = Vec::new();
    let mut bound: HashMap<String, usize> = HashMap::default();
    let is_bound = |bound: &HashMap<String, usize>, n: &str| !bound.is_empty() && bound.contains_key(&gridcraft_formula::fold_name(n));
    let mut pending = vec![Step::Visit(e)];
    while let Some(step) = pending.pop() {
        if found > stop_past {
            break;
        }
        let x = match step {
            Step::Visit(x) => x,
            Step::Bind(n) => {
                let n = gridcraft_formula::fold_name(n);
                *bound.entry(n.clone()).or_insert(0) += 1;
                stack.push(n);
                continue;
            }
            Step::Unbind(count) => {
                for _ in 0..count {
                    let Some(n) = stack.pop() else { break };
                    if let Some(c) = bound.get_mut(&n) {
                        *c = c.saturating_sub(1);
                        if *c == 0 {
                            bound.remove(&n);
                        }
                    }
                }
                continue;
            }
        };
        match x {
            Expr::Name(n) => found = found.saturating_add(usize::from(!is_bound(&bound, n))),
            Expr::Call(n, args) if n == "LAMBDA" && !args.is_empty() => {
                // The parameters are bound for the body (the last argument) only.
                let (parameters, body) = args.split_at(args.len() - 1);
                let names: Vec<&str> = parameters.iter().filter_map(|p| if let Expr::Name(p) = p { Some(p.as_str()) } else { None }).collect();
                pending.push(Step::Unbind(names.len()));
                pending.extend(body.iter().map(Step::Visit));
                pending.extend(names.into_iter().rev().map(Step::Bind));
            }
            Expr::Call(n, args) if n == "LET" && args.len() >= 3 => {
                // Each variable is bound for the values after it and for the result (the last
                // argument); steps run last in, first out.
                let (pairs, result) = args.split_at(args.len() - 1);
                let variables = pairs.chunks(2).filter(|pair| matches!(pair, [Expr::Name(_), _])).count();
                pending.push(Step::Unbind(variables));
                pending.extend(result.iter().map(Step::Visit));
                for pair in pairs.chunks(2).rev() {
                    match pair {
                        [Expr::Name(variable), value] => {
                            pending.push(Step::Bind(variable));
                            pending.push(Step::Visit(value));
                        }
                        [other] => pending.push(Step::Visit(other)),
                        _ => {}
                    }
                }
            }
            Expr::Call(n, args) => {
                if n == "INDIRECT" || (!crate::eval::is_known_function(n) && !is_bound(&bound, n)) {
                    found = found.saturating_add(1);
                }
                pending.extend(args.iter().rev().map(Step::Visit));
            }
            Expr::Array(rows) => pending.extend(rows.iter().flatten().rev().map(Step::Visit)),
            Expr::Unary(_, a) | Expr::Paren(a) => pending.push(Step::Visit(a)),
            Expr::Binary(_, a, b) => {
                pending.push(Step::Visit(b));
                pending.push(Step::Visit(a));
            }
            Expr::Invoke(callee, args) => {
                pending.extend(args.iter().rev().map(Step::Visit));
                pending.push(Step::Visit(callee));
            }
            Expr::Number(_) | Expr::Text(_) | Expr::Bool(_) | Expr::Error(_) | Expr::Ref(_) | Expr::Struct(_) | Expr::Missing => {}
        }
    }
    found
}

/// A legacy (Ctrl+Shift+Enter) array formula's result fitted to the range it was entered in, as
/// Excel does: a single row or column repeats across the range, a single value fills it, cells
/// past the result show `#N/A`, and a result larger than the range is cut off.
fn fit_to_range(v: Value, range: RangeRef) -> Value {
    if range.count() > crate::eval::MAX_CELLS {
        return Value::Error(CellError::Num);
    }
    let (h, w) = (range.height() as usize, range.width() as usize);
    let Value::Array(a) = v else {
        return if h * w == 1 { v } else { Array::new(h, w, vec![v; h * w]).map(Value::from).unwrap_or(Value::Error(CellError::Value)) };
    };
    if h * w == 1 {
        return a.data.first().cloned().unwrap_or_default();
    }
    let mut data = Vec::with_capacity(h * w);
    for r in 0..h {
        for c in 0..w {
            let (r, c) = (if a.rows == 1 { 0 } else { r }, if a.cols == 1 { 0 } else { c });
            data.push(a.get(r, c).cloned().unwrap_or(Value::Error(CellError::NA)));
        }
    }
    Array::new(h, w, data).map(Value::from).unwrap_or(Value::Error(CellError::Value))
}

/// `a` and `b` as one rectangle holding the cells of both and no other: `a` when it holds `b`,
/// `b` when it holds `a`, or the two side by side over the same rows or columns.
fn joined(a: RangeRef, b: RangeRef) -> Option<RangeRef> {
    if a.contains_range(&b) {
        return Some(a);
    }
    if b.contains_range(&a) {
        return Some(b);
    }
    let next = |end: u32, start: u32| end.checked_add(1) == Some(start);
    let same_cols = (a.start.col, a.end.col) == (b.start.col, b.end.col);
    let same_rows = (a.start.row, a.end.row) == (b.start.row, b.end.row);
    let side_by_side = (same_cols && (next(a.end.row, b.start.row) || next(b.end.row, a.start.row)))
        || (same_rows && (next(a.end.col, b.start.col) || next(b.end.col, a.start.col)));
    side_by_side.then(|| a.union(&b))
}

impl Host for PassHost<'_> {
    fn workbook(&self) -> &Workbook {
        self.wb
    }
    fn stack_top(&self) -> Option<usize> {
        (self.depth > 1).then_some(self.stack_top)
    }
    fn stack_exhausted(&mut self) {
        self.out_of_stack = true;
    }
    /// See `blocked`.
    fn halted(&self) -> bool {
        self.blocked
    }
    /// A worker: only there is `blocked` set, and every draw sets it (see `random`).
    fn can_halt(&self) -> bool {
        self.base.is_some()
    }
    /// Counts, for tests, the calls an evaluation makes after its formula was handed back (see
    /// `blocked_calls`): none while it stops at once.
    fn note_volatile(&mut self) {
        #[cfg(test)]
        {
            self.blocked_calls += usize::from(self.blocked);
        }
    }
    /// Found through `name_lookup`, which scans the workbook's names for the pass's first lookups
    /// and then indexes them once, as they don't change while it runs; on a worker through the
    /// main thread's index, or without it by its own scans and then the index the level's
    /// workers share (see `run_level`); in a one-shot evaluation always by a scan (see
    /// `one_shot`).
    fn name_position(&mut self, name: &str, scope_sheet: usize) -> Option<usize> {
        // See `blocked`.
        if self.blocked {
            return None;
        }
        let wb = self.wb;
        if let Some(index) = self.base.and_then(|b| b.name_lookup.index()) {
            index.find(name, scope_sheet)
        } else if self.one_shot {
            // See `one_shot`.
            wb.name_position(name, scope_sheet)
        } else if let Some(shared) = self.shared_names {
            // On a worker, only when its level's formulas name a few (see `run_level`): its own
            // scans, and past them the index the level's workers share (a name whose definition
            // names many).
            self.name_lookup.find_shared(wb, name, scope_sheet, shared)
        } else {
            self.name_lookup.find(wb, name, scope_sheet)
        }
    }
    /// The parsed definition, cached for the pass: the workbook's names don't change while it runs.
    /// Parsed once per definition, whichever sheets ask for it, and kept for the pass while the
    /// definitions kept stay within [`MAX_PASS_NAME_TEXT`]; past that, parsed at each use. A worker
    /// reads the main thread's parsed names first, then the level's store (see [`LevelNames`]).
    /// The evaluator has charged the use before it asks.
    fn name_expr(&mut self, position: usize) -> Option<std::sync::Arc<Expr>> {
        if self.blocked {
            return None;
        }
        if let Some(expr) = self.names.get(&position).or_else(|| self.base.and_then(|b| b.names.get(&position))) {
            return expr.clone();
        }
        let def = self.wb.names.get(position)?;
        if let Some(level) = self.level_names {
            // Past what the level may keep, or its store unusable: parsed for this use alone.
            let Some(slot) = level.slot(position, def.formula.len()) else { return parse_definition(&def.formula) };
            let expr = slot.get_or_init(|| parse_definition(&def.formula)).clone();
            self.names.insert(position, expr.clone());
            return expr;
        }
        let expr = parse_definition(&def.formula);
        self.keep_name(position, expr.clone());
        expr
    }
    fn spill_range(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        if self.blocked {
            return None;
        }
        self.note_read(sheet, RangeRef::new(anchor, anchor));
        // On a worker, no entry left to note the read in.
        if self.blocked {
            return None;
        }
        let k = (sheet, anchor);
        if !self.in_pass(&k) {
            return self.wb.sheet(sheet)?.spill_ranges.get(&anchor).copied();
        }
        // Recalculated in this pass: its new array, unless something blocks it (#SPILL!).
        self.compute(k);
        let arr = self.spill(&k)?;
        let end = CellRef::new(
            anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)),
            anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)),
        );
        let range = RangeRef::new(anchor, end);
        #[cfg(test)]
        self.walked_blocked();
        let sh = self.wb.sheet(sheet)?;
        let obstructed = end.row >= gridcraft_core::MAX_ROWS
            || end.col >= gridcraft_core::MAX_COLS
            || sh.cells.iter_range(range).any(|(c, cell)| c != anchor && (cell.formula.is_some() || !cell.value.is_empty()))
            || sh.merges.iter().any(|m| m.intersects(&range));
        (!obstructed).then_some(range)
    }
    // A read by a dynamic formula of a planned round is noted (see `Watch::reads`); the reads
    // these make inside, cell by cell, are not noted again.
    fn cell_value(&mut self, sheet: usize, c: CellRef) -> Value {
        #[cfg(test)]
        self.read_blocked();
        self.note_read(sheet, RangeRef::new(c, c));
        let out = self.read_cell(sheet, c);
        // See `blocked_reads`: a read that handed the formula back gives `#CALC!`.
        #[cfg(test)]
        {
            self.blocked_reads += usize::from(self.blocked && out != Value::Error(CellError::Calc));
        }
        out
    }
    fn range_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        #[cfg(test)]
        self.read_blocked();
        self.note_read(sheet, range);
        self.read_values(sheet, range)
    }
    fn visit_range(&mut self, sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
        #[cfg(test)]
        self.read_blocked();
        self.note_read(sheet, range);
        self.visit_values(sheet, range, f)
    }
    fn range_sum(&mut self, sheet: usize, range: RangeRef) -> Option<(f64, u64)> {
        #[cfg(test)]
        self.read_blocked();
        self.note_read(sheet, range);
        self.sum_constants(sheet, range)
    }
    fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        if self.blocked {
            return None;
        }
        if let Some(u) = self.base.and_then(|b| b.used.get(&sheet)) {
            return *u;
        }
        let wb = self.wb;
        *self.used.entry(sheet).or_insert_with(|| wb.sheet(sheet).and_then(|s| s.used_range()))
    }
    fn now_serial(&self) -> f64 {
        self.now
    }
    fn random(&mut self) -> f64 {
        #[cfg(test)]
        self.read_blocked();
        if self.base.is_some() {
            // A worker: the formula is handed back to the main thread, which draws from the one
            // generator (a name's body can call RAND where `uses_random` doesn't look).
            self.blocked = true;
        }
        // xorshift64*
        let mut x = *self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        *self.rng = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    }
}

impl PassHost<'_> {
    /// Counts a walk of the store made while `blocked` (see `blocked_walks`): none is made while
    /// the early returns hold.
    #[cfg(test)]
    fn walked_blocked(&mut self) {
        self.blocked_walks += usize::from(self.blocked);
    }

    /// Counts a read or draw asked for while `blocked` (see `blocked_reads`).
    #[cfg(test)]
    fn read_blocked(&mut self) {
        self.blocked_reads += usize::from(self.blocked);
    }

    fn read_cell(&mut self, sheet: usize, c: CellRef) -> Value {
        // See `blocked`: the value the blocking read returned.
        if self.blocked {
            return Value::Error(CellError::Calc);
        }
        let k = (sheet, c);
        // A formula or Python cell of the pass: calculated first if pending, else its result
        // (`compute` looks). Any other cell has neither, and `in_pass` settles it: a look-up in the
        // pass's formulas and one in its Python cells (on a worker the main thread's too), which
        // are empty maps in a pass without Python cells. This is the hottest path of a pass (once
        // per cell of every range read), and most cells read are not formulas of the pass.
        if self.in_pass(&k) {
            return self.compute(k);
        }
        // No array laid out in this view: none covers the cell (`covering_spill` would look in
        // two empty indexes), and the only arrays to note are those of the stored areas.
        if self.spills.is_empty() && self.base.is_none_or(|b| b.spills.is_empty()) {
            if self.notes_wanted() && !self.stored_areas().last.is_empty() {
                self.note_arrays_over(sheet, c, false);
                // See `blocked`: a note that handed the formula back makes this the blocking read.
                if self.blocked {
                    return Value::Error(CellError::Calc);
                }
            }
            return self.wb.sheet(sheet).map(|s| s.value(c)).unwrap_or_default();
        }
        // A cell covered by a spill computed in this pass; of two, the one anchored first in
        // row-major order, whatever order the arrays are kept in. The write-back keeps that one
        // too, unless `Standing` gives the cell to the other (its spill held the cell before the
        // calculation, or it held a spill and the first held none): then it gives the cells to
        // the other and calculates their readers again.
        if let Some((&anchor, arr)) = self.covering_spill(sheet, c) {
            let (dr, dc) = ((c.row - anchor.1.row) as usize, (c.col - anchor.1.col) as usize);
            let tainted = self.iterate && self.tainted.contains(&anchor);
            let v = arr.get(dr, dc).cloned().unwrap_or_default();
            if tainted {
                self.taint = true;
            }
            // A cell of the array other than its anchor (a formula cell, calculated above): read
            // without calculating the anchor.
            self.note_arrays_over(sheet, c, true);
            if self.blocked {
                return Value::Error(CellError::Calc);
            }
            return v;
        }
        // Read as stored: under an array whose anchor this view hasn't calculated yet, without
        // calculating it.
        self.note_arrays_over(sheet, c, false);
        if self.blocked {
            return Value::Error(CellError::Calc);
        }
        self.wb.sheet(sheet).map(|s| s.value(c)).unwrap_or_default()
    }
    fn read_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        if self.base.is_some()
            && !self.blocked
            && let Some(held) = self.ranges.get(&sheet)
            && held.blocks.contains_key(&(range.start, range.end))
        {
            // The arrays reaching into the block from outside, noted for this reader as the read
            // that made the copy noted them (see `read_range`): only those anchored after it, the
            // ones `note_unforced` may note.
            let outside: Vec<CellRef> = match self.current.last() {
                Some(&(k, _)) if self.notes_wanted() => held
                    .outside
                    .get(&(range.start, range.end))
                    .map(|anchors| anchors.iter().copied().filter(|&a| k < (sheet, a)).collect())
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            for a in outside {
                self.note_unforced((sheet, a));
            }
            // See `blocked`: a note that handed the formula back leaves the read with no values,
            // and the copy is not cloned for it.
            if self.blocked {
                return Vec::new();
            }
            #[cfg(test)]
            {
                self.copies_read += 1;
            }
            return self.ranges.get(&sheet).and_then(|held| held.blocks.get(&(range.start, range.end))).cloned().unwrap_or_default();
        }
        let out = self.read_range(sheet, range);
        let outside = std::mem::take(&mut self.outside_read);
        // Copied on a worker, on the second read of the block, when no formula in it was
        // pending either time, within the worker's share of the byte budget.
        //
        // `read_before` runs before the test of `blocked` on purpose: a read that handed its
        // formula back still marks the block as read once, so the worker's next read of it (for a
        // later formula) copies it, as it would have had the formula not been handed back. Tested
        // the other way round, the block would be noted only at that next read and copied at the
        // one after (`copies_read` differs, the values don't). A read that starts with the
        // formula already handed back needs no test of its own: `read_range` then gives no
        // values, fewer than `MIN_CACHED_CELLS`, so nothing is marked or copied.
        if self.base.is_some() && !self.copies_dropped && out.len() >= MIN_CACHED_CELLS && self.read_before((sheet, range)) && !self.blocked {
            let bytes = block_bytes(&out);
            if self.cached_bytes.saturating_add(self.seen_bytes).saturating_add(bytes) <= self.cache_budget {
                let copy: Vec<Value> = out
                    .iter()
                    .map(|v| match v {
                        Value::Text(t) => Value::Text(std::sync::Arc::from(&**t)),
                        v => v.clone(),
                    })
                    .collect();
                self.cached_bytes += bytes;
                self.ranges.entry(sheet).or_default().insert(range, copy, outside);
            }
        }
        // A note at the end of the read (an array reaching into the block from outside) that
        // handed the formula back: the evaluator's fence drops the values (see `blocked`).
        out
    }
    fn visit_values(&mut self, sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
        // See `blocked`: walked, with nothing in it.
        if self.blocked {
            return true;
        }
        #[cfg(test)]
        self.walked_blocked();
        let wb = self.wb;
        let Some(sh) = wb.sheet(sheet) else { return false };
        // Formula and Python cells first, in row-major order like `range_values`: reading one
        // calculates it (a Python cell lays out its result) and the pass remembers the value, so
        // the walk below gets the same value again without this method holding any. A read that
        // blocks ends the walk, as the check above would have.
        for (c, cell) in sh.cells.iter_range(range) {
            if cell.formula.is_some() || cell.py.is_some() {
                let _ = self.read_cell(sheet, c);
                if self.blocked {
                    return true;
                }
            }
        }
        #[cfg(test)]
        self.walked_blocked();
        // Spilled values sit between the stored cells (and the formulas just calculated may have
        // produced some): leave such blocks to `range_values`.
        let arrays = self.spills_meet(sheet, range);
        let mut stored = false;
        let _seen = stored_spill_in(sh, range, &mut |_, _| {
            stored = true;
            false
        });
        #[cfg(test)]
        {
            self.spill_visits += _seen;
        }
        if arrays || stored {
            return false;
        }
        for (c, cell) in sh.cells.iter_range(range) {
            let go = if cell.formula.is_some() || cell.py.is_some() { f(&self.read_cell(sheet, c)) } else { f(&cell.value) };
            if !go {
                break;
            }
        }
        true
    }
    fn sum_constants(&mut self, sheet: usize, range: RangeRef) -> Option<(f64, u64)> {
        // See `blocked`.
        if self.blocked {
            return None;
        }
        #[cfg(test)]
        self.walked_blocked();
        // Constants can't change during a pass, so a block's result stays valid for it. A block
        // that a spill found in this pass meets (and iteration, which tracks what a read depends
        // on) goes the long way: on a worker, the main thread's spills too. A spill elsewhere on
        // the sheet doesn't, so the path doesn't depend on which arrays were laid out before.
        if self.iterate || self.spills_meet(sheet, range) {
            return None;
        }
        let sh = self.wb.sheet(sheet)?;
        let (key, last) = ((sheet, range.start, range.end.col), range.end.row);
        // A worker reads the main thread's sums too: a result depends only on the workbook, which
        // doesn't change during the pass, so it is the same whichever thread worked it out, and
        // the main thread's don't change while the workers run.
        let theirs = self.base.and_then(|b| b.sums.get(&key));
        if let Some(r) = theirs.and_then(|t| t.get(&last)) {
            return *r;
        }
        let known = self.sums.entry(key).or_default();
        if let Some(r) = known.get(&last) {
            return *r;
        }
        // Start from the longest shorter block of the same rows from the top: summing row-major
        // from there on is the same additions in the same order as one walk from the start.
        let own = known.range(..last).next_back();
        let shorter = match (own, theirs.and_then(|t| t.range(..last).next_back())) {
            (Some(a), Some(b)) => Some(if b.0 > a.0 { b } else { a }),
            (a, b) => a.or(b),
        };
        let (from, mut sum, mut count) = match shorter {
            Some((_, None)) => {
                known.insert(last, None);
                return None;
            }
            Some((&row, &Some((sum, count)))) => (row + 1, sum, count),
            None => (range.start.row, 0.0, 0),
        };
        #[cfg(test)]
        {
            self.block_walks += 1;
        }
        let rest = RangeRef::new(CellRef::new(from, range.start.col), range.end);
        let mut plain = sh.spill.range(rest.start..=rest.end).all(|(c, _)| !rest.contains(*c));
        if plain {
            for (_, cell) in sh.cells.iter_range(rest) {
                match &cell.value {
                    _ if cell.formula.is_some() || cell.py.is_some() => plain = false,
                    Value::Error(_) => plain = false,
                    Value::Number(n) => {
                        sum += n;
                        count = u64::saturating_add(count, 1);
                    }
                    _ => {}
                }
                if !plain {
                    break;
                }
            }
        }
        let result = plain.then_some((sum, count));
        known.insert(last, result);
        result
    }
    /// Values of a block, row-major, from the store, this pass's spills and the formulas
    /// calculated so far (see [`Host::range_values`]).
    fn read_range(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        // See `blocked`: no values, which the evaluator takes as an error.
        if self.blocked {
            return Vec::new();
        }
        #[cfg(test)]
        self.walked_blocked();
        let (h, w) = (range.height() as usize, range.width() as usize);
        let mut out = vec![Value::Empty; h * w];
        let Some(sh) = self.wb.sheet(sheet) else { return out };
        // Sparse walk: only stored cells; formula and Python cells that are (or were) dirty go
        // through `compute`, constants are copied straight from the store.
        let mut formulas: Vec<(usize, CellRef)> = Vec::new();
        for (c, cell) in sh.cells.iter_range(range) {
            let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
            if cell.formula.is_some() || cell.py.is_some() {
                formulas.push((i, c));
            } else if let Some(slot) = out.get_mut(i) {
                *slot = cell.value.clone();
            }
        }
        let _seen = stored_spill_in(sh, range, &mut |c, v| {
            let i = (c.row - range.start.row) as usize * w + (c.col - range.start.col) as usize;
            if let Some(slot) = out.get_mut(i)
                && slot.is_empty()
            {
                *slot = v.clone();
            }
            true
        });
        #[cfg(test)]
        {
            self.spill_visits += _seen;
        }
        for (i, c) in formulas {
            let v = self.read_cell(sheet, c);
            // A read that blocks: no values, as the check above gives.
            if self.blocked {
                return Vec::new();
            }
            if let Some(slot) = out.get_mut(i) {
                *slot = v;
            }
        }
        #[cfg(test)]
        self.walked_blocked();
        if !self.spills.is_empty() || self.base.is_some_and(|b| !b.spills.is_empty()) {
            // The arrays over the block, from this host's and (on a worker) the main thread's.
            let mut hits = Vec::new();
            self.spill_index.meeting(sheet, range, &mut hits);
            let mut arrays: Vec<(CellRef, &std::sync::Arc<Array>)> =
                hits.iter().filter_map(|a| self.spills.get(&(sheet, *a)).map(|arr| (*a, arr))).collect();
            if let Some(b) = self.base {
                let mut theirs = Vec::new();
                b.spill_index.meeting(sheet, range, &mut theirs);
                arrays.extend(theirs.iter().filter_map(|a| b.spills.get(&(sheet, *a)).map(|arr| (*a, arr))));
            }
            // Iterating: an array spilled by a formula that read a previous value.
            if self.iterate && hits.iter().any(|a| self.tainted.contains(&(sheet, *a))) {
                self.taint = true;
            }
            // Of two arrays over one cell the one anchored first in row-major order wins, as in
            // `cell_value`: it is laid down last here. Only the part of each array inside the
            // range is walked: a large array beside a small read costs nothing.
            arrays.sort_by_key(|x| std::cmp::Reverse(x.0));
            // The arrays reaching into the block from an anchor outside it: their cells are read
            // without this read calculating the anchor (one inside, it has, above).
            let outside: Vec<CellRef> = if self.notes_wanted() {
                #[cfg(test)]
                ARRAYS_OVER_LOOKUPS.with(|n| n.set(n.get().saturating_add(1)));
                arrays.iter().filter(|(a, arr)| !range.contains(*a) && arr.rows > 0 && arr.cols > 0).map(|(a, _)| *a).collect()
            } else {
                Vec::new()
            };
            #[cfg(test)]
            let mut visited = 0usize;
            for (anchor, arr) in arrays {
                if arr.rows == 0 || arr.cols == 0 {
                    continue;
                }
                let (r0, c0) = (range.start.row.max(anchor.row), range.start.col.max(anchor.col));
                let r1 = range.end.row.min(anchor.row.saturating_add((arr.rows as u32).saturating_sub(1)));
                let c1 = range.end.col.min(anchor.col.saturating_add((arr.cols as u32).saturating_sub(1)));
                for r in r0..=r1 {
                    for cc in c0..=c1 {
                        if (r, cc) != (anchor.row, anchor.col)
                            && let Some(slot) = out.get_mut((r - range.start.row) as usize * w + (cc - range.start.col) as usize)
                        {
                            *slot = arr.get((r - anchor.row) as usize, (cc - anchor.col) as usize).cloned().unwrap_or_default();
                        }
                        #[cfg(test)]
                        {
                            visited += 1;
                        }
                    }
                }
            }
            #[cfg(test)]
            {
                self.spill_visits += visited;
            }
            for &a in &outside {
                self.note_unforced((sheet, a));
            }
            self.outside_read = outside;
        }
        if self.notes_wanted() {
            self.note_stored_outside(sheet, range);
        }
        out
    }

    /// Notes in `Watch::unforced` the formulas of the round this view hasn't calculated yet whose
    /// stored area reaches into `range` from an anchor outside it, anchored after the formula
    /// reading it (see `note_arrays_over`), and adds them to `outside_read` for a copy of the
    /// block. A worker calculates its formulas in row-major order and none on demand, so a later
    /// reader of the copy comes after this one, and an anchor after that reader is still not
    /// calculated on the worker: the anchors it notes from the copy (those after it) are the ones
    /// a read of the store would note.
    fn note_stored_outside(&mut self, sheet: usize, range: RangeRef) {
        let Some(&(k, _)) = self.current.last() else { return };
        let stored = self.stored_areas();
        // An anchor reaching into the block is at or before its last cell in row-major order.
        if k >= (sheet, range.end) || !stored.last.get(&sheet).is_some_and(|&last| k < (sheet, last)) {
            return;
        }
        #[cfg(test)]
        ARRAYS_OVER_LOOKUPS.with(|n| n.set(n.get().saturating_add(1)));
        let mut held = Vec::new();
        stored.index.meeting(sheet, range, &mut held);
        held.retain(|&a| !range.contains(a) && k < (sheet, a) && self.is_pending(&(sheet, a)));
        for &a in &held {
            self.note_unforced((sheet, a));
        }
        self.outside_read.extend(held);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A noted read is kept as the range in order, whichever way round its corners came, so the
    /// next read of the same formula joins it as the cells both hold: `D300:D1` stored backward,
    /// then `D2`, is noted as `D1:D300`. Kept backward, the two were joined as `D2:D2`.
    #[test]
    fn a_read_the_wrong_way_round_is_noted_in_order_and_joined_whole() {
        let wb = Workbook::new();
        let exprs = HashMap::default();
        let mut rng = 1u64;
        let mut host = PassHost::new(&wb, &exprs, None, &mut rng, 0.0);
        host.watch = true;
        host.watch_out.budget = 4;
        let reader = (0, CellRef::new(0, 9));
        host.current.push((reader, true));
        let column = RangeRef::new(CellRef::new(0, 3), CellRef::new(299, 3));
        host.note_read(0, RangeRef { start: column.end, end: column.start });
        assert_eq!(host.watch_out.reads, vec![(reader, 0, column)], "the backward read alone");
        host.note_read(0, RangeRef::cell(CellRef::new(1, 3)));
        assert_eq!(host.watch_out.reads, vec![(reader, 0, column)], "D2 joined with it");
        assert_eq!(host.watch_out.budget, 3, "one entry taken");
    }
}
