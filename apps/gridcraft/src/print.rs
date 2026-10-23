//! Desktop spooler adapter. Discovery and submission never run on the UI thread.
use gridcraft_ui_egui::printing::{Discover, Duplex, Printer, Settings, Submit};

pub fn discover() -> Discover {
    Box::new(|| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new().name("print-discovery".into()).spawn(move || {
            let printers = craft_print::printers()
                .into_iter()
                .map(|printer| Printer { name: printer.name, label: printer.description, default: printer.default })
                .collect();
            let _ = sender.send(printers);
        });
        receiver
    })
}

// Keep submission workers reachable until normal app shutdown; dropping a JoinHandle would
// let process exit kill PowerShell/CUPS before it hands the job to the spooler.
static JOBS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(Vec::new());

/// How long application shutdown waits for submission workers in total. Long enough for a normal
/// handover (PowerShell or CUPS taking the job), short enough that a stalled driver or spooler
/// cannot keep a closed window's process alive.
pub const SHUTDOWN_DEADLINE: std::time::Duration = std::time::Duration::from_secs(25);

/// Waits for pending submission workers for at most [`SHUTDOWN_DEADLINE`].
pub fn finish_pending_jobs() {
    finish_pending_jobs_within(SHUTDOWN_DEADLINE);
}

/// Waits up to `deadline` in total (not per worker) for the pending submission workers. A worker
/// that finishes in time is joined; one still running at the deadline is left behind and logged,
/// never waited on, so shutdown cannot hang.
pub fn finish_pending_jobs_within(deadline: std::time::Duration) {
    let workers = std::mem::take(&mut *JOBS.lock().unwrap_or_else(|poison| poison.into_inner()));
    for worker in join_within(workers, deadline) {
        let name = worker.thread().name().unwrap_or("print worker").to_owned();
        // The handover may or may not have reached the queue: do not claim either way.
        log::warn!(
            "{name} was still running when the application shut down; its job may or may not have reached the print queue, so check the printer's queue before printing again"
        );
    }
}

/// Joins every worker that finishes before `deadline` (polling `is_finished`, so a join never blocks)
/// and returns the ones still running.
#[allow(clippy::disallowed_methods)] // the desktop app (native only): wasm never runs it
fn join_within(mut workers: Vec<std::thread::JoinHandle<()>>, deadline: std::time::Duration) -> Vec<std::thread::JoinHandle<()>> {
    let end = std::time::Instant::now() + deadline;
    loop {
        let (done, pending): (Vec<_>, Vec<_>) = workers.into_iter().partition(|worker| worker.is_finished());
        for worker in done {
            if worker.join().is_err() {
                log::error!("the print worker stopped unexpectedly during shutdown");
            }
        }
        workers = pending;
        let now = std::time::Instant::now();
        if workers.is_empty() || now >= end {
            return workers;
        }
        std::thread::sleep(std::time::Duration::from_millis(10).min(end - now));
    }
}

/// The spooler's receipt, led by the printer it went to (the receipt alone is often empty).
fn named_receipt(printer: &str, receipt: &str) -> String {
    match (printer.trim().is_empty(), receipt.trim().is_empty()) {
        (_, true) => printer.to_owned(),
        (true, false) => receipt.to_owned(),
        (false, false) => format!("{printer}: {receipt}"),
    }
}

/// The job for a local queue. A sheet's page size is the workbook's own (Page Setup's Letter by
/// default), not a paper the person picked on the Print page, so the job never demands it: it goes on
/// the queue's own default paper and is shrunk to fit when it is larger, instead of being refused
/// ("has no 13.33 x 7.50 inch paper") by a queue that does not list that size. The PDF is already
/// gray when Grayscale is ticked; `grayscale` is also the driver's monochrome hint.
fn local_job(settings: Settings) -> craft_print::Job {
    craft_print::Job {
        printer: Some(settings.printer),
        copies: settings.copies.clamp(1, 999),
        duplex: settings.duplex.map(|value| match value {
            Duplex::LongEdge => craft_print::Duplex::LongEdge,
            Duplex::ShortEdge => craft_print::Duplex::ShortEdge,
            Duplex::Off => craft_print::Duplex::Off,
        }),
        grayscale: settings.grayscale,
        scaling: craft_print::Scaling::ShrinkToFit,
        title: "GridCraft document".into(),
        ..Default::default()
    }
}

pub fn submit() -> Submit {
    // A job's steps (spool job started, each page, completion or failure) go to the log, so a job
    // that reached the queue and never printed can be traced afterwards.
    let _ = craft_print::set_trace(|line| log::info!("print: {line}"));
    Box::new(|bytes, settings: Settings| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new().name("print-submit".into()).spawn(move || {
            // The status names the printer: "Print job sent to the print queue: Office Laser".
            let printer = settings.printer.clone();
            let job = local_job(settings);
            let result = craft_print::submit(&bytes, &job).map(|receipt| named_receipt(&printer, &receipt)).map_err(|error| error.to_string());
            let _ = sender.send(result);
        });
        if let Ok(worker) = worker {
            let mut jobs = JOBS.lock().unwrap_or_else(|poison| poison.into_inner());
            jobs.retain(|job| !job.is_finished());
            jobs.push(worker);
        }
        receiver
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_receipt_names_the_printer() {
        assert_eq!(named_receipt("Office Laser", ""), "Office Laser");
        assert_eq!(named_receipt("Office Laser", "job 7"), "Office Laser: job 7");
        assert_eq!(named_receipt("", "job 7"), "job 7");
    }

    #[test]
    fn a_local_job_uses_the_queues_own_paper_and_carries_the_settings() {
        let job = local_job(Settings { printer: "Office Laser".into(), copies: 2, duplex: Some(Duplex::LongEdge), grayscale: true });
        assert_eq!(job.scaling, craft_print::Scaling::ShrinkToFit, "no exact paper is demanded of the queue");
        assert!(job.options.is_empty(), "no paper size or other driver option is sent");
        assert_eq!(job.printer.as_deref(), Some("Office Laser"));
        assert_eq!((job.copies, job.duplex, job.grayscale), (2, Some(craft_print::Duplex::LongEdge), true));
        assert_eq!(local_job(Settings { copies: 0, ..Default::default() }).copies, 1);
    }

    #[test]
    fn shutdown_waits_for_pending_submission_workers() {
        let (release, wait) = std::sync::mpsc::channel();
        let (completed, completion) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait.recv().unwrap();
            completed.send(()).unwrap();
        });
        JOBS.lock().unwrap().push(worker);
        let shutdown = std::thread::spawn(finish_pending_jobs);
        assert!(completion.try_recv().is_err(), "the print worker is still pending");
        release.send(()).unwrap();
        shutdown.join().unwrap();
        completion.recv().unwrap();
        assert!(JOBS.lock().unwrap().is_empty(), "workers were joined before shutdown returned");
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // native-only test: reads the clock
    fn a_stalled_worker_cannot_hold_shutdown_past_the_deadline() {
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let stalled = std::thread::spawn(move || {
            let _ = wait.recv();
        });
        let (done_sender, done_receiver) = std::sync::mpsc::channel();
        let finished = std::thread::spawn(move || done_sender.send(()).unwrap());
        done_receiver.recv().unwrap();
        while !finished.is_finished() {
            std::thread::yield_now();
        }
        let started = std::time::Instant::now();
        let left = join_within(vec![stalled, finished], std::time::Duration::from_millis(200));
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "the wait is bounded by the deadline");
        assert_eq!(left.len(), 1, "only the stalled worker is left behind; the completed handover was joined");
        assert!(!left[0].is_finished());
        release.send(()).unwrap();
        for worker in left {
            worker.join().unwrap();
        }
    }

    #[test]
    fn a_worker_finishing_inside_the_deadline_is_joined() {
        let worker = std::thread::spawn(|| std::thread::sleep(std::time::Duration::from_millis(50)));
        assert!(join_within(vec![worker], std::time::Duration::from_secs(10)).is_empty());
    }
}
