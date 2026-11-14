//! The one active session per file (ADR-0007 §1): a lease, held by the session that created it
//! and refreshed by every message of a connection that belongs to it.
//!
//! The lease is what stops two cabinets from opening two rooms over the same workbook. It is not
//! a lock on the document: operations are serialised by the room itself.

use std::collections::BTreeMap;

/// A live claim on a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lease {
    pub file_id: i64,
    pub session_id: String,
    pub owner: String,
    pub expires_at: u64,
}

/// Why a session cannot take the lease.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LeaseError {
    /// Another live session holds the file; `takeover` was not asked for.
    Held { session_id: String, owner: String },
}

/// Every live lease of the process.
#[derive(Default)]
pub struct Leases {
    by_file: BTreeMap<i64, Lease>,
}

impl Leases {
    pub fn new() -> Leases {
        Leases::default()
    }

    /// The live lease of `file`, dropping an expired one.
    pub fn live(&mut self, file_id: i64, now: u64) -> Option<Lease> {
        match self.by_file.get(&file_id) {
            Some(lease) if lease.expires_at > now => Some(lease.clone()),
            Some(_) => {
                self.by_file.remove(&file_id);
                None
            }
            None => None,
        }
    }

    /// Takes the lease for `session_id`. A live lease of another session is refused unless
    /// `takeover`; an expired one is replaced. The caller closes the replaced room itself.
    pub fn acquire(&mut self, file_id: i64, session_id: &str, owner: &str, now: u64, ttl: u64, takeover: bool) -> Result<Lease, LeaseError> {
        if let Some(current) = self.live(file_id, now)
            && current.session_id != session_id
            && !takeover
        {
            return Err(LeaseError::Held { session_id: current.session_id, owner: current.owner });
        }
        let lease = Lease { file_id, session_id: session_id.to_string(), owner: owner.to_string(), expires_at: now.saturating_add(ttl) };
        self.by_file.insert(file_id, lease.clone());
        Ok(lease)
    }

    /// Extends the lease of `session_id` on `file`, when it is the holder. Returns the refreshed
    /// lease, or `None` when the lease is gone or belongs to somebody else (the connection must
    /// then be closed).
    pub fn touch(&mut self, file_id: i64, session_id: &str, now: u64, ttl: u64) -> Option<Lease> {
        let lease = self.by_file.get_mut(&file_id)?;
        if lease.session_id != session_id {
            return None;
        }
        if lease.expires_at <= now {
            self.by_file.remove(&file_id);
            return None;
        }
        lease.expires_at = now.saturating_add(ttl);
        Some(lease.clone())
    }

    /// Drops the lease of one session, wherever it is.
    pub fn release(&mut self, session_id: &str) {
        self.by_file.retain(|_, lease| lease.session_id != session_id);
    }

    /// Every live lease, for `/metrics` and `/v1/sessions`.
    pub fn live_leases(&self, now: u64) -> Vec<Lease> {
        self.by_file.values().filter(|l| l.expires_at > now).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_live_session_per_file() {
        let mut leases = Leases::new();
        leases.acquire(7, "s-1", "ann", 100, 60, false).unwrap();
        assert_eq!(leases.acquire(7, "s-2", "bob", 100, 60, false).unwrap_err(), LeaseError::Held { session_id: "s-1".into(), owner: "ann".into() });
        assert_eq!(leases.acquire(7, "s-1", "ann", 100, 60, false).unwrap().expires_at, 160);
    }

    #[test]
    fn an_expired_lease_is_replaced() {
        let mut leases = Leases::new();
        leases.acquire(7, "s-1", "ann", 100, 60, false).unwrap();
        assert!(leases.live(7, 160).is_none());
        assert_eq!(leases.acquire(7, "s-2", "bob", 200, 60, false).unwrap().session_id, "s-2");
    }

    #[test]
    fn an_owner_takes_over_and_the_old_holder_cannot_refresh() {
        let mut leases = Leases::new();
        leases.acquire(7, "s-1", "ann", 100, 60, false).unwrap();
        leases.acquire(7, "s-2", "ann", 100, 60, true).unwrap();
        assert!(leases.touch(7, "s-1", 110, 60).is_none());
        assert_eq!(leases.touch(7, "s-2", 110, 60).unwrap().expires_at, 170);
    }

    #[test]
    fn release_frees_the_file() {
        let mut leases = Leases::new();
        leases.acquire(7, "s-1", "ann", 100, 60, false).unwrap();
        leases.release("s-1");
        assert!(leases.live(7, 100).is_none());
    }

    #[test]
    fn files_are_independent() {
        let mut leases = Leases::new();
        leases.acquire(7, "s-1", "ann", 100, 60, false).unwrap();
        assert_eq!(leases.acquire(8, "s-2", "bob", 100, 60, false).unwrap().file_id, 8);
    }
}
