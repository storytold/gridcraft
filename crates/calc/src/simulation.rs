//! Host-neutral simulation context. Sampling algorithms and RNG ownership stay in the engine.
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::Arc};

use gridcraft_core::{CellRef, Value};
pub use gridcraft_functions::SimulationRequest;

thread_local! { static CURRENT: RefCell<Option<SimulationHook>> = const { RefCell::new(None) }; }

/// The physical formula cell requesting a draw; independent of dependency evaluation order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimulationSite {
    pub sheet: usize,
    pub cell: CellRef,
}

type SampleCallback = dyn for<'a> Fn(SimulationSite, SimulationRequest<'a>) -> Value + Send + Sync;

/// A scoped, cloneable callback supplied by an embedding simulation host.
#[derive(Clone)]
pub struct SimulationHook(Arc<SampleCallback>);

impl SimulationHook {
    /// Creates a callback without introducing an RNG or a dependency on engine policy.
    pub fn new(callback: impl for<'a> Fn(SimulationSite, SimulationRequest<'a>) -> Value + Send + Sync + 'static) -> Self {
        Self(Arc::new(callback))
    }

    /// Captures the current context once per recalculation pass, without holding a borrow.
    pub fn current() -> Option<Self> {
        CURRENT.with(|slot| slot.try_borrow().ok().and_then(|value| value.clone()))
    }

    /// Dispatches a scalar request with its cell identity to the supplied host.
    pub fn sample(&self, site: SimulationSite, request: SimulationRequest<'_>) -> Value {
        (self.0)(site, request)
    }

    /// Enters a thread-bound context; retaining the guard restores the preceding context on exit.
    /// A busy slot refuses entry, so a simulation cannot silently fall back to ordinary mode.
    pub fn enter(&self) -> Option<SimulationScope> {
        CURRENT.with(|slot| {
            slot.try_borrow_mut().ok().map(|mut value| SimulationScope { previous: value.replace(self.clone()), thread_bound: PhantomData })
        })
    }
}

impl std::fmt::Debug for SimulationHook {
    /// Does not expose callback captures, seeds or workbook data.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SimulationHook(..)")
    }
}

/// Keeps a simulation context on its originating thread and restores its predecessor on drop.
#[must_use]
pub struct SimulationScope {
    previous: Option<SimulationHook>,
    thread_bound: PhantomData<Rc<()>>,
}

impl Drop for SimulationScope {
    /// Drops callbacks only after releasing the slot borrow, including during unwinding.
    fn drop(&mut self) {
        let removed = CURRENT.with(|slot| slot.try_borrow_mut().ok().and_then(|mut value| std::mem::replace(&mut *value, self.previous.take())));
        drop(removed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A nested host is visible only within its guard and leaves ordinary evaluation untouched.
    #[test]
    fn scoped_hosts_restore_previous_sampling_context() {
        assert!(SimulationHook::current().is_none());
        let site = SimulationSite { sheet: 2, cell: CellRef::new(3, 4) };
        let first = SimulationHook::new(|s, _| Value::Number(s.cell.row as f64));
        let outer = first.enter().unwrap();
        assert_eq!(SimulationHook::current().unwrap().sample(site, SimulationRequest::Active), Value::Number(3.0));
        {
            let second = SimulationHook::new(|s, _| Value::Number(s.cell.col as f64));
            let _inner = second.enter().unwrap();
            assert_eq!(SimulationHook::current().unwrap().sample(site, SimulationRequest::Active), Value::Number(4.0));
        }
        assert_eq!(SimulationHook::current().unwrap().sample(site, SimulationRequest::Active), Value::Number(3.0));
        drop(outer);
        assert!(SimulationHook::current().is_none());
    }
}
