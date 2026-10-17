//! Cooperative checkpoints for long-running operations. The caller owns scheduling and cancellation.
use std::future::{Future, ready};
use std::task::{Context, Poll, Waker};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkStage {
    #[default]
    Archive,
    SharedStrings,
    Worksheet,
    RowHeights,
    Dependencies,
    Calculation,
    Finishing,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkProgress {
    pub stage: WorkStage,
    pub completed: u64,
    pub total: u64,
    pub part: u32,
    pub parts: u32,
    pub sheet_name: Option<std::sync::Arc<str>>,
}
impl WorkProgress {
    pub fn new(stage: WorkStage, completed: u64, total: u64) -> Self {
        Self { stage, completed, total, part: 0, parts: 0, sheet_name: None }
    }
}
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Opening cancelled")]
pub struct WorkCancelled;

pub trait WorkControl {
    fn checkpoint(&mut self, progress: WorkProgress) -> impl Future<Output = Result<(), WorkCancelled>>;
}
pub struct Uninterrupted;
impl WorkControl for Uninterrupted {
    fn checkpoint(&mut self, _: WorkProgress) -> impl Future<Output = Result<(), WorkCancelled>> {
        ready(Ok(()))
    }
}
/// Poll a synchronous operation whose checkpoints never yield. A pending future is an internal error.
pub fn run_ready<T>(future: impl Future<Output = T>) -> Option<T> {
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(value) => Some(value),
        Poll::Pending => None,
    }
}
pub struct ProgressScope<'a, C> {
    pub control: &'a mut C,
    pub part: u32,
    pub parts: u32,
    pub sheet_name: std::sync::Arc<str>,
}
impl<C: WorkControl> WorkControl for ProgressScope<'_, C> {
    fn checkpoint(&mut self, mut progress: WorkProgress) -> impl Future<Output = Result<(), WorkCancelled>> {
        progress.part = self.part;
        progress.parts = self.parts;
        progress.sheet_name = Some(self.sheet_name.clone());
        self.control.checkpoint(progress)
    }
}
