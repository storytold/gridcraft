//! Shared operational limits for isolated workbook analysis, independent of UI or transport.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, AtomicU64, Ordering},
};

use crate::{EngineError, Result, Workbook};

/// Maximum estimated work units consumed by one trial sequence, including preparation.
pub const MAX_RUN_WORK: u64 = 100_000_000;
/// Finite allowance for a read-only query, including queries against locally trusted books.
pub const MAX_COMMAND_WORK: u64 = 1_000_000_000;
/// Larger finite allowance for edits, full recalculation and explicit imports, never analysis credit.
pub const MAX_EDIT_WORK: u64 = 10_000_000_000;
/// Small charges share prepaid credit; large native kernels reserve their full cost first.
const WORK_CHUNK: u64 = 4096;
/// Estimated work units available per open workbook in a fixed sixty-second window.
pub const MAX_WINDOW_WORK: u64 = 300_000_000;
/// Analysis reports may not grow the active workbook beyond this stored-cell limit.
pub const MAX_REPORT_CELLS: usize = 2_000_000;
/// Includes source sheets, preventing unbounded small-report/chart accumulation.
pub const MAX_REPORT_SHEETS: usize = 128;
const WINDOW_MS: f64 = 60_000.0;

/// A cloneable cooperative cancellation signal. It never mutates the source workbook.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicU8>);
impl Cancellation {
    /// Requests cancellation at the next analysis checkpoint, not an unsafe thread interruption.
    pub fn cancel(&self) -> bool {
        match self.0.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => true,
            Err(state) => state == 1,
        }
    }
    /// Whether cancellation has been requested by the UI, transport or embedding caller.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire) == 1
    }
    /// Converts a cancellation request into a normal atomic-analysis error.
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() { Err(EngineError::Other("Analysis cancelled; source cells were preserved.".into())) } else { Ok(()) }
    }

    /// Establishes the atomic publication boundary; later cancellation requests return false.
    pub(crate) fn begin_commit(&self) -> Result<()> {
        self.0
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| EngineError::Other("Analysis cancelled or already publishing; no partial report was created.".into()))
    }

    /// Marks publication complete without allowing a later request to claim it was cancelled.
    pub(crate) fn complete(&self) {
        self.0.store(3, Ordering::Release);
    }
}

#[derive(Debug, Default)]
struct Window {
    start_ms: Option<f64>,
    used: u64,
}

/// Window state and its generation share one slow-path lock. Fast charges only read generation.
#[derive(Debug)]
struct Bank {
    window: Mutex<Window>,
    generation: AtomicU64,
    capacity: u64,
    #[cfg(test)]
    reservations: AtomicU64,
}

impl Default for Bank {
    /// Analysis uses the normal capacity; renderer construction changes only its own bank.
    fn default() -> Self {
        Self {
            window: Mutex::default(),
            generation: AtomicU64::default(),
            capacity: MAX_WINDOW_WORK,
            #[cfg(test)]
            reservations: AtomicU64::default(),
        }
    }
}

/// A budget shared by all what-if evaluators of an open workbook, including repeated requests.
/// Clones share reservations; unused credit is not refunded, and reopening creates a new bank.
#[derive(Clone, Debug, Default)]
pub struct AnalysisBudget(Arc<Bank>);
impl AnalysisBudget {
    /// A separate, smaller persistent window for workbook-authored paint expressions.
    pub(crate) fn rendering() -> Self {
        Self::with_capacity(1_000_000)
    }

    /// Keeps capacity construction inside the policy module; callers cannot raise a live bank.
    fn with_capacity(capacity: u64) -> Self {
        Self(Arc::new(Bank { capacity, ..Bank::default() }))
    }

    /// Reserves credit only on depletion/epoch changes, with one clock read per slow path.
    fn credit_for(&self, units: u64, credit: u64, generation: u64, future: u64) -> Result<(u64, u64)> {
        self.credit_for_at(units, credit, generation, future, gridcraft_core::date::unix_now_ms())
    }

    /// Discards credit from an observed old window, then reserves at most the run's real margin.
    /// Near bank exhaustion a smaller grant is valid whenever it covers this exact call.
    fn credit_for_at(&self, units: u64, credit: u64, generation: u64, future: u64, now: Option<f64>) -> Result<(u64, u64)> {
        let mut window = self.0.window.lock().map_err(|_| EngineError::Other("Analysis budget is unavailable.".into()))?;
        if refill(&mut window, now) {
            self.0.generation.fetch_add(1, Ordering::AcqRel);
        }
        let current = self.0.generation.load(Ordering::Acquire);
        let credit = if current == generation { credit } else { 0 };
        let needed = units.saturating_sub(credit);
        if needed == 0 {
            return Ok((credit - units, current));
        }
        let available = self.0.capacity.saturating_sub(window.used);
        if available < needed {
            return Err(EngineError::Other(
                "Shared analysis work limit reached for this workbook. Wait for the next 60-second window or reduce the model/iterations.".into(),
            ));
        }
        let grant = needed.max(WORK_CHUNK.min(needed.saturating_add(future))).min(available);
        window.used += grant; // grant <= available, so this sum is bounded by the bank capacity.
        #[cfg(test)]
        self.0.reservations.fetch_add(1, Ordering::Relaxed);
        Ok((credit.saturating_add(grant) - units, current))
    }

    /// Exact deterministic reservations for clock/window boundary regression tests.
    #[cfg(test)]
    fn spend_at(&self, units: u64, now: Option<f64>) -> Result<()> {
        self.credit_for_at(units, 0, 0, 0, now).map(|_| ())
    }

    /// Remaining unreserved work; querying observes rollover and expires old run credits.
    pub fn remaining(&self) -> Result<u64> {
        let mut window = self.0.window.lock().map_err(|_| EngineError::Other("Analysis budget is unavailable.".into()))?;
        if refill(&mut window, gridcraft_core::date::unix_now_ms()) {
            self.0.generation.fetch_add(1, Ordering::AcqRel);
        }
        Ok(self.0.capacity.saturating_sub(window.used))
    }
}

/// Refills only after an entire forward-moving window; unknown/backward clocks retain charges.
fn refill(window: &mut Window, now: Option<f64>) -> bool {
    let Some(now) = now.filter(|n| n.is_finite()) else { return false };
    match window.start_ms {
        Some(start) if now - start >= WINDOW_MS => {
            window.start_ms = Some(now);
            window.used = 0;
            true
        }
        None => {
            window.start_ms = Some(now);
            true
        }
        _ => false,
    }
}

/// Sticky failure codes keep the frequent checkpoint free of locks and error-string allocation.
const REFUSED_RUN: u8 = 1;
const REFUSED_SHARED: u8 = 2;
const REFUSED_STATE: u8 = 3;

/// Atomic counters cover exact consumed work; only prepaid analysis-credit renewal takes locks.
#[derive(Debug)]
struct RunState {
    budget: Option<AnalysisBudget>,
    spent: AtomicU64,
    limit: AtomicU64,
    credit: AtomicU64,
    credit_generation: AtomicU64,
    failure: AtomicU8,
    reserving: Mutex<()>,
}

/// One shared meter for trial bookkeeping, expression evaluation and native function kernels.
#[derive(Clone, Debug)]
pub(crate) struct Meter {
    pub cancellation: Cancellation,
    state: Arc<RunState>,
}
impl Meter {
    /// Starts analysis with a persistent run limit and the open workbook reservation bank.
    pub fn new(budget: AnalysisBudget) -> Self {
        Self::with_bank(Some(budget), MAX_RUN_WORK)
    }

    /// Finite query/local protection; command metadata selects the separate bulk allowance.
    pub fn ordinary() -> Self {
        Self::with_bank(None, MAX_COMMAND_WORK)
    }

    /// Selects policy from command metadata, never document trust. Nested mutations reuse their
    /// parent's meter; nested queries retain a local 1B ceiling as well as the parent's ceiling.
    pub fn command(mutating: bool, inherited: bool) -> Option<Self> {
        if mutating && inherited { None } else { Some(Self::with_bank(None, if mutating { MAX_EDIT_WORK } else { MAX_COMMAND_WORK })) }
    }

    /// A nested query charges both limits before work and propagates refusal to the parent latch.
    pub fn hook_with_parent(&self, parent: Option<gridcraft_calc::WorkHook>) -> gridcraft_calc::WorkHook {
        let own = self.hook();
        match parent {
            Some(parent) => gridcraft_calc::WorkHook::new(move |units| own.charge(units) && parent.charge(units)),
            None => own,
        }
    }

    /// Creates shared exact counters; the optional bank is used exclusively by analysis trials.
    fn with_bank(budget: Option<AnalysisBudget>, limit: u64) -> Self {
        Self {
            cancellation: Cancellation::default(),
            state: Arc::new(RunState {
                budget,
                spent: AtomicU64::new(0),
                limit: AtomicU64::new(limit),
                credit: AtomicU64::new(0),
                credit_generation: AtomicU64::new(0),
                failure: AtomicU8::new(0),
                reserving: Mutex::new(()),
            }),
        }
    }

    /// Configures a lower ceiling before the run starts, without resetting a terminal refusal.
    pub fn set_limit(&self, limit: u64) -> Result<()> {
        if limit == 0 || self.state.spent.load(Ordering::Relaxed) != 0 {
            return Err(EngineError::Other("Configure a positive work limit before starting the run.".into()));
        }
        self.state
            .limit
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |old| (limit <= old).then_some(limit))
            .map(|_| ())
            .map_err(|_| EngineError::Other("The configured work limit can only be lowered.".into()))
    }

    /// Zero-unit polling reads only cancellation/failure atomics, never a clock or mutex.
    pub fn check(&self) -> Result<()> {
        self.cancellation.check()?;
        match self.state.failure.load(Ordering::Acquire) {
            0 => Ok(()),
            REFUSED_RUN => Err(EngineError::Other("Work limit exceeded. Reduce the model or iteration count.".into())),
            REFUSED_SHARED => Err(EngineError::Other(
                "Shared analysis work limit reached or budget unavailable. Wait for the next 60-second window or reduce the model/iterations.".into(),
            )),
            _ => Err(EngineError::Other("Work meter is unavailable.".into())),
        }
    }

    /// Exact accepted work, distinct from the bank's conservative unrefunded reservations.
    pub fn spent(&self) -> Result<u64> {
        Ok(self.state.spent.load(Ordering::Relaxed))
    }

    /// Latches the first terminal reason, so IFERROR and zero-unit checks cannot hide a refusal.
    fn refuse(&self, reason: u8) -> Result<()> {
        let _ = self.state.failure.compare_exchange(0, reason, Ordering::AcqRel, Ordering::Acquire);
        self.check()
    }

    /// Consumes already-paid credit without locks or clock reads when its window is current.
    fn take_credit(&self, budget: &AnalysisBudget, units: u64) -> bool {
        self.state.credit_generation.load(Ordering::Acquire) == budget.0.generation.load(Ordering::Acquire)
            && self.state.credit.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |credit| credit.checked_sub(units)).is_ok()
    }

    /// Serializes only renewal, expires observed old-window credit and pays before native work.
    fn renew_credit(&self, budget: &AnalysisBudget, units: u64) -> Result<()> {
        let Ok(_renewal) = self.state.reserving.lock() else { return self.refuse(REFUSED_STATE) };
        self.check()?;
        if self.take_credit(budget, units) {
            return Ok(());
        }
        let credit = self.state.credit.swap(0, Ordering::AcqRel);
        let generation = self.state.credit_generation.load(Ordering::Acquire);
        let future = self.state.limit.load(Ordering::Relaxed).saturating_sub(self.state.spent.load(Ordering::Relaxed));
        match budget.credit_for(units, credit, generation, future) {
            Ok((remaining, generation)) => {
                self.state.credit.fetch_add(remaining, Ordering::Release);
                self.state.credit_generation.store(generation, Ordering::Release);
                Ok(())
            }
            Err(_) => self.refuse(REFUSED_SHARED),
        }
    }

    /// Charges exact run work before a call/allocation; small shared debits use prepaid credit.
    /// A 1,000-unit run can reserve at most 1,000 units, never an unconditional 4,096-unit block.
    pub fn charge(&self, units: u64) -> Result<()> {
        self.check()?;
        if units == 0 {
            return Ok(());
        }
        let limit = self.state.limit.load(Ordering::Relaxed);
        if self.state.spent.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| used.checked_add(units).filter(|next| *next <= limit)).is_err()
        {
            return self.refuse(REFUSED_RUN);
        }
        if let Some(budget) = &self.state.budget
            && !self.take_credit(budget, units)
            && let Err(error) = self.renew_credit(budget, units)
        {
            self.state.spent.fetch_sub(units, Ordering::Relaxed);
            return Err(error);
        }
        self.check()
    }

    /// Supplies lower-layer code a neutral callback, keeping engine policy/errors out of L1/L2.
    pub fn hook(&self) -> gridcraft_calc::WorkHook {
        let meter = self.clone();
        gridcraft_calc::WorkHook::new(move |units| meter.charge(units).is_ok())
    }
}

/// Checks report growth before trial allocation/publication, counting stored cells on all sheets.
/// Existing undo history remains governed by the engine's separate retained-byte/entry caps.
pub(crate) fn report_capacity(wb: &Workbook, additional_cells: usize) -> Result<()> {
    if wb.sheets.len() >= MAX_REPORT_SHEETS {
        return Err(EngineError::Other(format!(
            "Analysis report sheet limit reached ({MAX_REPORT_SHEETS} sheets including the source). Delete unneeded reports first."
        )));
    }
    let stored = wb.sheets.iter().fold(0usize, |n, sh| n.saturating_add(sh.cells.len()));
    if stored.saturating_add(additional_cells) > MAX_REPORT_CELLS {
        return Err(EngineError::Other(format!(
            "Analysis report storage limit exceeded ({MAX_REPORT_CELLS} stored cells including the source). Omit samples or delete unneeded reports."
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A conservative 50k prefix-sum bound fits bulk work while both entry policies stay finite.
    #[test]
    fn prefix_sum_bulk_allowance_is_larger_without_raising_the_query_cap() {
        let rows = 50_000u64;
        let bound = rows * (rows + 1) / 2 + 16 * rows;
        let bulk = Meter::command(true, false).unwrap();
        bulk.charge(bound).unwrap();
        assert_eq!(bulk.spent().unwrap(), bound);
        assert!(bulk.charge(MAX_EDIT_WORK - bound + 1).is_err());
        assert!(bulk.charge(0).is_err());
        let query = Meter::command(false, false).unwrap();
        assert!(query.charge(bound).is_err());
        assert!(query.charge(0).is_err());
        assert!(Meter::command(true, true).is_none(), "nested mutations must not reset the parent");
    }

    /// A nested query charges its parent and retains its own ceiling even inside a bulk scope.
    #[test]
    fn nested_queries_keep_their_ceiling_and_parent_accounting() {
        let parent = Meter::command(true, false).unwrap();
        let _outer = parent.hook().enter().unwrap();
        let query = Meter::command(false, true).unwrap();
        {
            let _inner = query.hook_with_parent(gridcraft_calc::WorkHook::current()).enter().unwrap();
            assert!(gridcraft_calc::WorkHook::current().unwrap().charge(64));
            assert!(!gridcraft_calc::WorkHook::current().unwrap().charge(MAX_COMMAND_WORK));
        }
        assert_eq!(parent.spent().unwrap(), 64);
        assert_eq!(query.spent().unwrap(), 64);
        assert!(!gridcraft_calc::WorkHook::allowed(), "a handled child refusal must stay terminal");
    }

    /// Cheap checkpoints amortize shared reservations and zero polling performs no renewal.
    #[test]
    fn atomic_charges_amortize_the_bank_and_preserve_exact_consumed_work() {
        let budget = AnalysisBudget::default();
        let meter = Meter::new(budget.clone());
        for _ in 0..10_000 {
            meter.charge(1).unwrap();
            meter.charge(0).unwrap();
        }
        assert_eq!(meter.spent().unwrap(), 10_000);
        assert_eq!(budget.0.reservations.load(Ordering::Relaxed), 3);
        assert_eq!(budget.0.window.lock().unwrap().used, 3 * WORK_CHUNK);
    }

    /// Small runs reserve their actual margin and can use the last units of a nearly full bank.
    #[test]
    fn small_limits_and_nearly_exhausted_shared_banks_do_not_require_a_full_chunk() {
        let budget = AnalysisBudget::default();
        let meter = Meter::new(budget.clone());
        meter.set_limit(1000).unwrap();
        meter.charge(1).unwrap();
        assert_eq!(budget.0.window.lock().unwrap().used, 1000);
        meter.charge(999).unwrap();
        assert_eq!(meter.spent().unwrap(), 1000);
        assert!(meter.charge(1).is_err());
        assert!(meter.charge(0).is_err());
        let almost_full = AnalysisBudget::default();
        almost_full.spend_at(MAX_WINDOW_WORK - 2, gridcraft_core::date::unix_now_ms()).unwrap();
        let last = Meter::new(almost_full);
        last.charge(1).unwrap();
        last.charge(1).unwrap();
        assert!(last.charge(1).is_err());
        assert_eq!(last.spent().unwrap(), 2);
    }

    /// An observed rollover expires prepaid old credit instead of bypassing the new window.
    #[test]
    fn prepaid_credit_expires_when_the_window_generation_rolls_over() {
        let budget = AnalysisBudget::default();
        let meter = Meter::new(budget.clone());
        meter.charge(1).unwrap();
        assert_eq!(meter.state.credit.load(Ordering::Relaxed), WORK_CHUNK - 1);
        let old_generation = meter.state.credit_generation.load(Ordering::Acquire);
        budget.0.window.lock().unwrap().start_ms = Some(gridcraft_core::date::unix_now_ms().unwrap() - WINDOW_MS - 1.0);
        assert_eq!(budget.remaining().unwrap(), MAX_WINDOW_WORK);
        assert_ne!(old_generation, budget.0.generation.load(Ordering::Acquire));
        meter.charge(1).unwrap();
        assert_eq!(budget.remaining().unwrap(), MAX_WINDOW_WORK - WORK_CHUNK);
        assert_eq!(meter.spent().unwrap(), 2);
        assert_eq!(meter.state.credit.load(Ordering::Relaxed), WORK_CHUNK - 1);
        // No explicit status query: renewal itself must observe rollover and discard old credit.
        let other_budget = AnalysisBudget::default();
        let other = Meter::new(other_budget.clone());
        other.charge(1).unwrap();
        other_budget.0.window.lock().unwrap().start_ms = Some(gridcraft_core::date::unix_now_ms().unwrap() - WINDOW_MS - 1.0);
        other.charge(WORK_CHUNK + 1).unwrap();
        assert_eq!(other_budget.remaining().unwrap(), MAX_WINDOW_WORK - WORK_CHUNK - 1);
        assert_eq!(other.spent().unwrap(), WORK_CHUNK + 2);
    }

    /// An indivisible native charge must be fully prepaid or refused before its kernel runs.
    #[test]
    fn large_native_charges_are_prepaid_exactly_and_share_a_sticky_clone() {
        let budget = AnalysisBudget::default();
        let meter = Meter::new(budget.clone());
        meter.set_limit(10_000).unwrap();
        meter.charge(7000).unwrap();
        assert_eq!(budget.remaining().unwrap(), MAX_WINDOW_WORK - 7000);
        assert!(meter.clone().charge(3001).is_err());
        assert!(meter.charge(0).is_err());
        assert_eq!(meter.spent().unwrap(), 7000);
    }

    /// Concurrent small calls cannot overdraw either the run counter or shared prepaid credit.
    #[test]
    fn concurrent_clones_share_exact_run_work_and_sticky_refusal() {
        let meter = Meter::new(AnalysisBudget::default());
        meter.set_limit(1000).unwrap();
        let mut threads = Vec::new();
        for _ in 0..4 {
            let copy = meter.clone();
            threads.push(std::thread::spawn(move || {
                for _ in 0..250 {
                    copy.charge(1).unwrap();
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(meter.spent().unwrap(), 1000);
        assert!(meter.charge(1).is_err());
        assert!(meter.clone().charge(0).is_err());
    }

    /// Repeated evaluators share a bank, including at rollback/unknown clock readings.
    #[test]
    fn shared_budget_refills_only_after_a_complete_forward_window() {
        let budget = AnalysisBudget::default();
        budget.spend_at(MAX_WINDOW_WORK, Some(1000.)).unwrap();
        assert!(budget.clone().spend_at(1, Some(1001.)).is_err());
        assert!(budget.spend_at(1, Some(0.)).is_err());
        assert!(budget.spend_at(1, None).is_err());
        assert!(budget.spend_at(1, Some(f64::NAN)).is_err());
        budget.spend_at(1, Some(61_000.)).unwrap();
    }
    /// Run limits cannot be raised and cancellation is observed before any extra charge.
    #[test]
    fn run_limits_and_cooperative_cancellation_are_enforced() {
        let meter = Meter::new(AnalysisBudget::default());
        meter.set_limit(10).unwrap();
        assert!(meter.set_limit(11).is_err());
        meter.charge(10).unwrap();
        assert!(meter.charge(1).is_err());
        assert!(meter.charge(0).is_err());
        assert!(meter.set_limit(MAX_RUN_WORK + 1).is_err());
        let next = Meter::new(AnalysisBudget::default());
        next.cancellation.clone().cancel();
        assert!(next.charge(1).is_err());
        assert_eq!(next.spent().unwrap(), 0);
    }

    /// Cancellation wins before publication; accepted publication rejects all later requests.
    #[test]
    fn cancellation_and_publication_have_one_atomic_boundary() {
        let cancelled = Cancellation::default();
        assert!(cancelled.cancel());
        assert!(cancelled.begin_commit().is_err());
        let publishing = Cancellation::default();
        publishing.begin_commit().unwrap();
        assert!(!publishing.cancel());
        assert!(!publishing.is_cancelled());
        publishing.complete();
        assert!(!publishing.cancel());
        assert!(publishing.check().is_ok());
    }
}
