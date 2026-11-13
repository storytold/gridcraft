//! Host-neutral scoped checkpoints. Lower layers never import engine/UI policy or errors.
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

thread_local! { static CURRENT: RefCell<Option<WorkHook>> = const { RefCell::new(None) }; }

/// A cloneable accounting/cancellation callback supplied by the embedding host.
#[derive(Clone)]
pub struct WorkHook {
    checkpoint: Arc<dyn Fn(u64) -> bool + Send + Sync>,
    failed: Arc<AtomicBool>,
}
impl WorkHook {
    /// Installs a policy callback; false means stop before performing charged work.
    pub fn new(checkpoint: impl Fn(u64) -> bool + Send + Sync + 'static) -> Self {
        Self { checkpoint: Arc::new(checkpoint), failed: Arc::new(AtomicBool::new(false)) }
    }
    /// Charges conservative work, including zero-unit persistent-failure checks.
    pub fn charge(&self, units: u64) -> bool {
        {
            if self.failed.load(Ordering::Acquire) {
                return false;
            }
            let allowed = (self.checkpoint)(units);
            if !allowed {
                self.failed.store(true, Ordering::Release);
            }
            allowed
        }
    }
    /// Clones the active thread's policy without retaining a borrow while calling user code.
    pub fn current() -> Option<Self> {
        CURRENT.with(|slot| slot.try_borrow().ok().and_then(|value| value.clone()))
    }
    /// Checks the active policy after evaluation, even when IFERROR swallowed a numeric error.
    pub fn allowed() -> bool {
        Self::current().is_none_or(|hook| hook.charge(0))
    }
    /// Enters a portable nested evaluation scope; callers must keep the returned guard alive.
    /// A busy internal slot is a refusal, never an unguarded fallback.
    pub fn enter(&self) -> Option<WorkScope> {
        CURRENT.with(|slot| {
            slot.try_borrow_mut().ok().map(|mut value| {
                let failed = value.as_ref().map_or_else(|| self.failed.clone(), |parent| parent.failed.clone());
                let source = self.clone();
                WorkScope {
                    // Keep this hook's own sticky refusal as well as the parent's latch.
                    previous: value.replace(Self { checkpoint: Arc::new(move |units| source.charge(units)), failed }),
                    thread_bound: PhantomData,
                }
            })
        })
    }

    /// Enters an optional, locally fallible scope without sharing its refusal latch with the
    /// enclosing operation. The enclosing policy is still polled for terminal refusal; the
    /// callback must explicitly forward positive work charges to it when composing policies.
    /// Use `enter` for work whose failure must stop the enclosing operation.
    pub fn enter_local(&self) -> Option<WorkScope> {
        CURRENT.with(|slot| {
            slot.try_borrow_mut().ok().map(|mut value| {
                let parent = value.clone();
                let source = self.clone();
                let local = Self {
                    checkpoint: Arc::new(move |units| parent.as_ref().is_none_or(|hook| hook.charge(0)) && source.charge(units)),
                    failed: self.failed.clone(),
                };
                WorkScope { previous: value.replace(local), thread_bound: PhantomData }
            })
        })
    }
}

impl std::fmt::Debug for WorkHook {
    /// Debug output never exposes callback captures or workbook data.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkHook(..)")
    }
}

/// Restores the preceding policy on all returns, including unwinding; guards stay on their thread.
#[must_use]
pub struct WorkScope {
    previous: Option<WorkHook>,
    thread_bound: PhantomData<Rc<()>>,
}
impl Drop for WorkScope {
    /// Restores the slot before dropping callbacks, allowing safe destructor/re-entrant evaluation.
    fn drop(&mut self) {
        let removed = CURRENT.with(|slot| slot.try_borrow_mut().ok().and_then(|mut value| std::mem::replace(&mut *value, self.previous.take())));
        drop(removed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    /// Re-entering a rejected child under a fresh parent must not reset either failure contract.
    #[test]
    fn failed_child_remains_failed_after_scope_reentry() {
        let calls = Arc::new(AtomicU64::new(0));
        let seen = calls.clone();
        let child = WorkHook::new(move |_| seen.fetch_add(1, Ordering::Relaxed) > 0);
        assert!(!child.charge(1));
        let parent = WorkHook::new(|_| true);
        let outer = parent.enter().unwrap();
        {
            let _inner = child.enter().unwrap();
            assert!(!WorkHook::allowed());
        }
        assert!(!WorkHook::allowed());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        drop(outer);
        assert!(WorkHook::current().is_none());
    }

    /// Optional child refusal remains sticky locally without poisoning the enclosing operation.
    #[test]
    fn local_refusal_does_not_poison_parent_or_reset_on_reentry() {
        let parent = WorkHook::new(|_| true);
        let _outer = parent.enter().unwrap();
        let child = WorkHook::new(|units| units == 0);
        {
            let _inner = child.enter_local().unwrap();
            assert!(!WorkHook::current().unwrap().charge(1));
            assert!(!WorkHook::allowed());
        }
        assert!(WorkHook::allowed());
        {
            let _inner = child.enter_local().unwrap();
            assert!(!WorkHook::allowed());
        }
        assert!(WorkHook::allowed());
    }

    /// A local scope still observes an actual enclosing refusal and cannot clear it on return.
    #[test]
    fn local_scope_observes_real_parent_refusal() {
        let parent = WorkHook::new(|units| units == 0);
        let _outer = parent.enter().unwrap();
        let child = WorkHook::new(|_| true);
        {
            let _inner = child.enter_local().unwrap();
            assert!(WorkHook::allowed());
            assert!(!parent.charge(1));
            assert!(!WorkHook::allowed());
        }
        assert!(!WorkHook::allowed());
    }
}
