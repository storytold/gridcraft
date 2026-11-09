//! The web build's client: the same API as the native one, with no processes. Every request
//! ends as [`Outcome::FailedToStart`], so cells show `#CONNECT!`.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::protocol::{Init, Input, SelfTest};
pub use crate::run::*;

/// Why a request can't run here.
pub const UNAVAILABLE: &str = "Python isn't available in the web version";

pub struct Client {
    limits: Limits,
    local: Option<RunResult>,
    next_id: u64,
}

impl Client {
    pub fn new(_transport: Box<dyn Transport>, limits: Limits) -> Client {
        Client { limits: limits.clamped(), local: None, next_id: 1 }
    }
    pub fn limits(&self) -> &Limits {
        &self.limits
    }
    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits.clamped();
    }
    pub fn starts(&self) -> u64 {
        0
    }
    pub fn busy(&self) -> bool {
        self.local.is_some()
    }
    pub fn alive(&self) -> bool {
        false
    }
    pub fn submit_render(&mut self, _at: [u32; 3], _width: u32, _height: u32) -> Option<u64> {
        None
    }

    pub fn take_touched(&mut self) -> Option<[u32; 3]> {
        None
    }

    pub fn self_test(&self) -> Option<&SelfTest> {
        None
    }
    pub fn stop(&mut self) -> Option<u64> {
        self.local.take().map(|r| r.id)
    }
    pub fn submit_run(&mut self, _code: &str, _inputs: BTreeMap<String, Input>) -> Result<u64, Busy> {
        self.refuse()
    }
    pub fn submit_cell(&mut self, _code: &str, _inputs: BTreeMap<String, Input>, _object: bool, _at: Option<[u32; 3]>) -> Result<u64, Busy> {
        self.refuse()
    }
    pub fn submit_init(&mut self, _code: &str, _init: Init) -> Result<u64, Busy> {
        self.refuse()
    }
    pub fn submit_reset(&mut self) -> Result<u64, Busy> {
        self.refuse()
    }
    pub fn poll(&mut self, _wait: Duration) -> Option<RunResult> {
        self.local.take()
    }
    pub fn wait(&mut self) -> Option<RunResult> {
        self.local.take()
    }
    pub fn run(&mut self, code: &str, inputs: BTreeMap<String, Input>) -> Result<Outcome, Busy> {
        self.submit_run(code, inputs)?;
        Ok(self.local.take().map(|r| r.outcome).unwrap_or_else(|| Outcome::FailedToStart(UNAVAILABLE.into())))
    }
    pub fn reset(&mut self) -> Result<Outcome, Busy> {
        self.submit_reset()?;
        Ok(self.local.take().map(|r| r.outcome).unwrap_or_else(|| Outcome::FailedToStart(UNAVAILABLE.into())))
    }
    fn refuse(&mut self) -> Result<u64, Busy> {
        if self.local.is_some() {
            return Err(Busy);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.local = Some(RunResult { id, outcome: Outcome::FailedToStart(UNAVAILABLE.into()) });
        Ok(id)
    }
}
