//! AutoSave writes on a worker thread so a big workbook never freezes the window.
//!
//! The frame only takes an `Arc` snapshot of the workbook ([`gridcraft_engine::autosave::SaveJob`]);
//! the worker serialises and writes it, then wakes the window, and the next frame marks the
//! document clean if it hasn't changed since. Measured before this change, the synchronous save
//! held the frame for 0.3 to 1 s on 50k formula cells, about 2 s on 200k and 8 s or more on 1M.

use gridcraft_engine::autosave::SaveJob;

/// A save running on the worker thread.
pub struct InFlight {
    pub job: SaveJob,
    pub started: f64,
    rx: std::sync::mpsc::Receiver<Result<usize, String>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl InFlight {
    /// Starts `job` on a worker thread that wakes `ctx` when it is done. `None` when no thread
    /// could be started (the browser has none): the caller saves on this thread instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn start(job: SaveJob, ctx: &egui::Context) -> Option<InFlight> {
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = job.clone();
        let ctx = ctx.clone();
        let handle = std::thread::Builder::new()
            .name("autosave".into())
            .spawn(move || {
                let result = run(&worker);
                let _ = tx.send(result);
                ctx.request_repaint();
            })
            .map_err(|e| log::warn!("AutoSave could not start a worker thread, saving in the frame instead: {e}"))
            .ok()?;
        Some(InFlight { job, started: crate::monotonic_ms(), rx, handle: Some(handle) })
    }

    #[cfg(target_arch = "wasm32")]
    pub fn start(_job: SaveJob, _ctx: &egui::Context) -> Option<InFlight> {
        None
    }

    /// The result, once the worker has finished; `None` while it is still writing.
    pub fn poll(&self) -> Option<Result<usize, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("AutoSave stopped before it finished".into())),
        }
    }

    /// Waits for the worker to finish (before a manual save, open or close of a file).
    pub fn wait(mut self) -> Result<usize, String> {
        let r = self.rx.recv().unwrap_or_else(|_| Err("AutoSave stopped before it finished".into()));
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        r
    }
}

impl Drop for InFlight {
    /// Quitting while a save is being written waits for it: the file is never left half-replaced
    /// and the temporary file is cleaned up.
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Runs a job; a panic while serialising becomes an error instead of ending the worker silently.
pub fn run(job: &SaveJob) -> Result<usize, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job.run())) {
        Ok(r) => r.map_err(|e| e.to_string()),
        Err(_) => Err("AutoSave failed unexpectedly".into()),
    }
}
