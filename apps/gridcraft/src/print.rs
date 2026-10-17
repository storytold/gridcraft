//! Desktop spooler adapter. Discovery and submission never run on the UI thread.
use gridcraft_ui_egui::printing::{Discover, Duplex, Printer, Settings, Submit};

pub fn discover() -> Discover {
    Box::new(|| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let _ = std::thread::Builder::new().name("print-discovery".into()).spawn(move || {
            let printers = craft_print::printers().into_iter().map(|printer| Printer { name: printer.name, default: printer.default }).collect();
            let _ = sender.send(printers);
        });
        receiver
    })
}

// Keep submission workers reachable until normal app shutdown; dropping a JoinHandle would
// let process exit kill PowerShell/CUPS before it hands the job to the spooler.
static JOBS: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(Vec::new());

pub fn finish_pending_jobs() {
    let workers = std::mem::take(&mut *JOBS.lock().unwrap_or_else(|poison| poison.into_inner()));
    for worker in workers {
        if worker.join().is_err() {
            log::error!("the print worker stopped unexpectedly during shutdown");
        }
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
}
