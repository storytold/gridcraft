//! Print > Network printer: the app's part of `craft_ui::printing::network`.
//!
//! The model, its rules and the widgets are craft-ui's; the printing is craft-print's, through a
//! private [`Registry`] that holds one backend with only the printer the person typed: WS-Print
//! ([`WsdBackend`]) or IPP ([`IppBackend`]), chosen from the address. This file supplies what a host
//! supplies: the address check, the worker that asks a printer what it lists, the worker that sends a
//! job and follows it, the mapping of craft-print's answers onto craft-ui's three-state options, and
//! nothing that words a message (the wording is `gridcraft_ui_egui::printing::network_labels`).
//!
//! * **Nothing connects on its own.** No discovery, no multicast. The registry asks the printer only
//!   when the person presses Query printer, and sends a document only after the ticked confirmation
//!   ([`UserConfirmed`], taken from the box and from nothing else).
//! * **Two phases.** The worker reports *accepted* as soon as the printer has the document, then
//!   follows the job with craft-print's `JobWatch`, reporting each new status until it ends. A
//!   document is sent once: a send whose outcome is unknown is shown with the request to check the
//!   printer, and nothing retries it.
//! * **Formats.** The document is the print-ready PDF. A printer that lists no PDF is refused before a
//!   job exists (`NoCommonFormat`): GridCraft has no PDF rasteriser to hand the registry as a
//!   converter.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use craft_print::ipp::{IppBackend, PrinterUri};
use craft_print::wsd::{PrinterUrl, WsdBackend};
use craft_print::{
    CancelToken, ColourMode, Document, Duplex, ErrorKind, Job, JobOptions, JobSource, JobState, JobStatus, JobWatch, Orientation, Phase, PrintError,
    PrinterDetails, PrinterState, Quality, Registry, Resolution, SubmitRequest, UserConfirmed, WaitPolicy, WatchOutcome, WatchStep,
};
use gridcraft_ui_egui::printing::net::{
    self, Caps, Failure, FailureKind, JobHandle, JobUpdate, Outcome, PrinterSummary, Progress, QueryPrinter, Report, Request, SendJob, Stage,
};
use gridcraft_ui_egui::printing::{Capability, Network, NetworkJob};

/// Any transport that can be shared with the worker threads (tests give an in-memory printer).
pub type Transport = Arc<dyn craft_print::wsd::adapter::Transport + Send + Sync>;

/// The media types the dialog can produce.
pub const FORMATS: &[&str] = &["application/pdf"];

const MAX_JOB_NAME: usize = 100;
/// How long a job is followed after the printer accepted it; it carries on at the printer after that.
const FOLLOW_FOR: Duration = Duration::from_secs(300);

/// Which protocol an address speaks: `ipp://` or a path under `/ipp` is IPP, anything else WS-Print.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Wsd,
    Ipp,
}

impl Protocol {
    pub fn of(address: &str) -> Self {
        let lower = address.trim().to_ascii_lowercase();
        if lower.starts_with("ipp://") || lower.starts_with("ipps://") {
            return Protocol::Ipp;
        }
        let rest = lower.split_once("://").map_or(lower.as_str(), |(_, rest)| rest);
        match rest.split_once('/') {
            Some((_, path)) if path == "ipp" || path.starts_with("ipp/") || path.starts_with("ipp?") => Protocol::Ipp,
            _ => Protocol::Wsd,
        }
    }

    const fn prefix(self) -> &'static str {
        match self {
            Protocol::Wsd => "wsd",
            Protocol::Ipp => "ipp",
        }
    }
}

fn policy(why: impl Into<String>) -> Failure {
    Failure::new(FailureKind::Policy, why)
}

/// The address checked against the network policy (a numeric loopback, link-local or private
/// address; no TLS). The error is what the dialog shows under the field.
fn validate(text: &str) -> Result<(), Failure> {
    match Protocol::of(text) {
        Protocol::Wsd => PrinterUrl::parse(text).map(|_| ()).map_err(|e| policy(e.to_string())),
        Protocol::Ipp => PrinterUri::parse(text).map(|_| ()).map_err(|e| policy(e.to_string())),
    }
}

/// One printer behind a private registry: its namespaced id and the registry that follows its jobs.
struct Open {
    registry: Registry,
    id: String,
}

/// Open the printer at `address`: nothing connects.
fn open(address: &str, transport: Option<Transport>) -> Result<Open, PrintError> {
    let protocol = Protocol::of(address);
    let registry = Registry::new();
    let (native, backend): (String, Box<dyn craft_print::backends::Backend>) = match protocol {
        Protocol::Wsd => {
            let mut backend = WsdBackend::new();
            if let Some(transport) = transport {
                backend = backend.with_transport(transport);
            }
            (backend.add(address)?, Box::new(backend))
        }
        Protocol::Ipp => {
            let mut backend = IppBackend::new();
            if let Some(transport) = transport {
                backend = backend.with_transport(transport);
            }
            (backend.add(address)?, Box::new(backend))
        }
    };
    registry.register(backend).map_err(|e| PrintError::new(ErrorKind::Failed, e.to_string()))?;
    Ok(Open { registry, id: format!("{}:{native}", protocol.prefix()) })
}

fn failure(e: &PrintError, open: bool) -> Failure {
    let kind = if e.native().is_some_and(|n| n.domain == "wsd-fault") {
        FailureKind::Fault
    } else if e.phase() == Some(Phase::Prepare) && e.kind() == ErrorKind::Failed {
        FailureKind::Encode
    } else {
        match e.kind() {
            ErrorKind::InvalidRequest if open => FailureKind::Policy,
            ErrorKind::InvalidRequest | ErrorKind::ConsentRequired => FailureKind::Invalid,
            ErrorKind::UnsupportedFormat => FailureKind::NoCommonFormat,
            ErrorKind::Unsupported => FailureKind::Unsupported,
            ErrorKind::Unavailable | ErrorKind::Timeout | ErrorKind::Busy | ErrorKind::Io => FailureKind::Transport,
            ErrorKind::Failed => FailureKind::Protocol,
            ErrorKind::Rejected | ErrorKind::NotFound => FailureKind::Fault,
            ErrorKind::Cancelled => FailureKind::Stopped,
            _ => FailureKind::Other,
        }
    };
    Failure::new(kind, e.message()).with_outcome_unknown(e.outcome_unknown())
}

fn state_name(state: PrinterState) -> &'static str {
    match state {
        PrinterState::Idle => "idle",
        PrinterState::Processing => "processing",
        PrinterState::Stopped => "stopped",
        PrinterState::Offline => "offline",
        _ => "unknown",
    }
}

/// What the registry knows of a printer, as craft-ui's three-state `Caps` (the mapping table in
/// craft-ui's README). A list the printer left out reads as unknown, so the control stays enabled
/// with a note and the value is sent as a hint; a printer that says it has no two-sided, no colour or
/// one copy reads as unsupported. Orientation and quality have no field in the details: they are
/// unknown unless the backend says it ignores the hint.
pub fn caps_of(details: &PrinterDetails) -> Caps {
    use craft_print::JobHint;
    let ignores = |hint: JobHint| details.honoured.as_ref().is_some_and(|list| !list.contains(&hint));
    let token = |t: &str| t.to_owned();
    let hinted = |hint: JobHint| if ignores(hint) { Capability::Unsupported } else { Capability::Unknown };
    Caps {
        formats: details.formats.iter().map(|f| f.mime().to_owned()).collect(),
        sides: match details.two_sided {
            None => Capability::Unknown,
            Some(true) => Capability::from_option_list(Some(&[
                token(net::Sides::OneSided.token()),
                token(net::Sides::LongEdge.token()),
                token(net::Sides::ShortEdge.token()),
            ])),
            Some(false) => Capability::from_option_list(Some(&[token(net::Sides::OneSided.token())])),
        },
        orientations: hinted(JobHint::Orientation),
        qualities: hinted(JobHint::Quality),
        color_modes: match details.colour {
            None => Capability::Unknown,
            Some(true) => Capability::from_option_list(Some(&[token(net::ColorMode::Color.token()), token(net::ColorMode::Monochrome.token())])),
            Some(false) => Capability::from_option_list(Some(&[token(net::ColorMode::Monochrome.token())])),
        },
        media_sizes: if details.paper.is_empty() { Capability::Unknown } else { Capability::from_option_list(Some(details.paper.as_slice())) },
        resolutions: if details.resolutions.is_empty() {
            Capability::Unknown
        } else {
            let list: Vec<net::Resolution> = details.resolutions.iter().map(|r| net::Resolution::new(r.x_dpi, r.y_dpi)).collect();
            Capability::from_option_list(Some(list.as_slice()))
        },
        copies: Capability::from_option_range(details.max_copies.map(|max| (1, max))),
    }
}

fn summary(details: &PrinterDetails) -> PrinterSummary {
    let (state, reasons) = details.status.as_ref().map_or((String::new(), Vec::new()), |s| (state_name(s.state).to_owned(), s.reasons.clone()));
    PrinterSummary { name: details.name.clone(), color: details.colour, state, queued_jobs: None, reasons, caps: caps_of(details) }
}

fn repaint(ctx: &Option<egui::Context>) {
    if let Some(ctx) = ctx {
        ctx.request_repaint();
    }
}

/// What the last query of each address read, so a job to that printer does not read it again.
type Queried = Arc<Mutex<Option<(String, PrinterDetails)>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The worker that asks the printer at the (normalised) address what it is and what it lists. It
/// sends no job.
fn query_printer(transport: Option<Transport>, ctx: Option<egui::Context>, queried: Queried) -> QueryPrinter {
    Box::new(move |address: &str| {
        let (tx, rx) = mpsc::channel();
        let (address, transport, ctx, queried) = (address.to_owned(), transport.clone(), ctx.clone(), Arc::clone(&queried));
        std::thread::spawn(move || {
            let result = match open(&address, transport) {
                Ok(printer) => printer
                    .registry
                    .printer_details(&printer.id)
                    .map(|details| {
                        let summary = summary(&details);
                        *lock(&queried) = Some((address.clone(), details));
                        summary
                    })
                    .map_err(|e| failure(&e, false)),
                Err(e) => Err(failure(&e, true)),
            };
            let _ = tx.send(result);
            repaint(&ctx);
        });
        rx
    })
}

fn options(request: &Request, job_name: &str) -> (Job, JobOptions) {
    let name: String = job_name.chars().filter(|c| !c.is_control()).take(MAX_JOB_NAME).collect();
    let mut job = Job::default();
    if !name.trim().is_empty() {
        job.title = name;
    }
    job.copies = request.copies.unwrap_or(1);
    job.duplex = request.sides.map(|s| match s {
        net::Sides::OneSided => Duplex::Off,
        net::Sides::LongEdge => Duplex::LongEdge,
        net::Sides::ShortEdge => Duplex::ShortEdge,
    });
    let mut options = JobOptions::new();
    if let Some(o) = request.orientation {
        options = options.with_orientation(match o {
            net::Orientation::Portrait => Orientation::Portrait,
            net::Orientation::Landscape => Orientation::Landscape,
            net::Orientation::ReversePortrait => Orientation::ReversePortrait,
            net::Orientation::ReverseLandscape => Orientation::ReverseLandscape,
        });
    }
    match request.quality {
        Some(net::Quality::Draft) => options = options.with_quality(Quality::Draft),
        Some(net::Quality::Normal) => options = options.with_quality(Quality::Normal),
        Some(net::Quality::High) => options = options.with_quality(Quality::High),
        Some(net::Quality::Photo) => options = options.with_quality(Quality::Photo),
        None => {}
    }
    if let Some(color) = request.color {
        options = options.with_colour(match color {
            net::ColorMode::Color => ColourMode::Colour,
            net::ColorMode::Monochrome => ColourMode::Mono,
        });
    }
    if let Some(r) = request.resolution {
        options = options.with_resolution(Resolution::new(r.x_dpi, r.y_dpi));
    }
    if let Some(media) = &request.media_size {
        options = options.with_media(media.as_str());
    }
    if let Some(pages) = request.expected_pages {
        options = options.with_expected_pages(pages);
    }
    (job, options)
}

fn progress(status: &JobStatus) -> Progress {
    Progress {
        stage: Stage::from_name(status.state.name()),
        sheets_done: status.sheets_done,
        sheets_total: status.sheets_total,
        notes: status.reasons.clone(),
    }
}

/// How following a job ended, as craft-ui's report. The document was accepted in every case, so a
/// watch that could not finish is "sent, outcome unknown", never a failure.
fn report(outcome: &WatchOutcome, printer: &str, warnings: &[String]) -> Report {
    let (kind, status) = match outcome {
        WatchOutcome::Terminal(s) => (
            match s.state {
                JobState::Completed if !s.warnings.is_empty() => Outcome::CompletedWithWarnings,
                JobState::Completed => Outcome::Completed,
                JobState::CompletedWithErrors => Outcome::CompletedWithErrors,
                JobState::Cancelled => Outcome::Canceled,
                JobState::Aborted => Outcome::Aborted,
                _ => Outcome::AcceptedUnknown,
            },
            Some(s),
        ),
        WatchOutcome::Forgotten { last } => (Outcome::Forgotten, last.as_ref()),
        WatchOutcome::TimedOut { last } => (Outcome::StillRunning, last.as_ref()),
        _ => (Outcome::AcceptedUnknown, None),
    };
    let mut notes: Vec<String> = status.map(|s| s.reasons.clone()).unwrap_or_default();
    notes.extend(warnings.iter().cloned());
    if let WatchOutcome::Failed(e) = outcome {
        notes.push(e.message().to_owned());
    }
    Report { outcome: kind, printer: printer.to_owned(), sheets: status.and_then(|s| s.sheets_done).map_or(0, u64::from), notes }
}

/// The worker that sends the print-ready `pdf` and follows the job. Each call makes a new job with
/// the person's `confirmed` (the attestation of the ticked box, never `interactive()`).
#[allow(clippy::too_many_arguments)]
fn send_job(
    transport: Option<Transport>,
    ctx: Option<egui::Context>,
    name: String,
    pdf: Arc<Vec<u8>>,
    queried: Queried,
    confirmed: UserConfirmed,
) -> SendJob {
    Box::new(move |address: &str, request: Request| {
        let (result_tx, result_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        let cancel = CancelToken::new();
        let (address, transport, ctx) = (address.to_owned(), transport.clone(), ctx.clone());
        let (name, pdf, worker_cancel) = (name.clone(), Arc::clone(&pdf), cancel.clone());
        let known = lock(&queried).as_ref().filter(|(a, _)| *a == address).map(|(_, d)| d.clone());
        std::thread::spawn(move || {
            let (job, options) = options(&request, &name);
            let result = run_job(&address, transport, (job, options), &pdf, known, confirmed, &worker_cancel, &update_tx, &ctx);
            let _ = result_tx.send(result);
            repaint(&ctx);
        });
        JobHandle::new(result_rx, Box::new(move || cancel.cancel())).with_updates(update_rx)
    })
}

#[allow(clippy::too_many_arguments)]
fn run_job(
    address: &str,
    transport: Option<Transport>,
    (job, options): (Job, JobOptions),
    pdf: &Arc<Vec<u8>>,
    known: Option<PrinterDetails>,
    confirmed: UserConfirmed,
    cancel: &CancelToken,
    updates: &mpsc::Sender<JobUpdate>,
    ctx: &Option<egui::Context>,
) -> Result<Report, Failure> {
    let printer = open(address, transport).map_err(|e| failure(&e, true))?;
    let printer_name = known.as_ref().map(|d| d.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| address.to_owned());
    let job = Job { printer: Some(printer.id.clone()), ..job };
    // What the person was shown is what the registry decides on: no second read for the format.
    let request = SubmitRequest::new(Document::pdf(pdf.as_slice()), job).with_options(options).with_consent(confirmed).with_cancel(cancel.clone());
    // Phase one: the printer accepts the document. A cancel before that sends nothing.
    let submitted = match printer.registry.submit_job_with_details(request, known) {
        Ok(submitted) => submitted,
        Err(e) if e.kind() == ErrorKind::Cancelled => {
            return Ok(Report { outcome: Outcome::Canceled, printer: String::new(), sheets: 0, notes: Vec::new() });
        }
        Err(e) => return Err(failure(&e, false)),
    };
    let notes = submitted.warnings.clone();
    let _ = updates.send(JobUpdate::Accepted(Progress { notes: notes.clone(), ..Progress::default() }));
    repaint(ctx);
    // Phase two: follow it. One cancel is sent if asked; the watch then carries on to the end state.
    let policy = WaitPolicy { interval: Some(Duration::from_millis(500)), ..WaitPolicy::default() }.with_max_wait(FOLLOW_FOR);
    let outcome = match JobWatch::for_submitted(&submitted, policy) {
        Some(mut watch) => {
            watch = watch.with_cancel(cancel.clone());
            let source: &dyn JobSource = &printer.registry;
            loop {
                match watch.tick(source) {
                    WatchStep::Pending { next_in } => std::thread::sleep(next_in.min(Duration::from_millis(100))),
                    WatchStep::Update(status) => {
                        let _ = updates.send(JobUpdate::Status(progress(&status)));
                        repaint(ctx);
                    }
                    WatchStep::Done(outcome) => break outcome,
                    _ => std::thread::sleep(Duration::from_millis(100)),
                }
            }
        }
        None => WatchOutcome::NoStatus,
    };
    Ok(report(&outcome, &printer_name, &notes))
}

/// A job that failed before it started: the handle already holds the failure.
fn refused(failure: Failure) -> SendJob {
    Box::new(move |_address: &str, _request: Request| {
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(Err(failure.clone()));
        JobHandle::new(rx, Box::new(|| {}))
    })
}

/// The services the Print page's network destination needs. `transport` replaces the plain-HTTP one
/// (tests; embedding).
pub fn service(ctx: Option<egui::Context>, transport: Option<Transport>) -> Network {
    let queried = Queried::default();
    let query = query_printer(transport.clone(), ctx.clone(), Arc::clone(&queried));
    let send = {
        let queried = Arc::clone(&queried);
        move |job: NetworkJob| match UserConfirmed::attest(Some(job.confirmed)) {
            // The attestation comes from the ticked box and nothing else.
            Ok(confirmed) => send_job(transport.clone(), ctx.clone(), job.name, job.pdf, Arc::clone(&queried), confirmed),
            Err(e) => refused(failure(&e, false)),
        }
    };
    Network { validate: Box::new(|text: &str| validate(text)), query, send: Box::new(send), formats: FORMATS }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_print::wsd::testing::{Ending, FakeConfig, FakePrinter, MemTransport};
    use serde_json::json;
    use std::time::Duration;

    const ADDRESS: &str = "http://127.0.0.1:5357/print";
    const WAIT: Duration = Duration::from_secs(20);

    fn transport(printer: &Arc<FakePrinter>) -> Transport {
        Arc::new(MemTransport(Arc::clone(printer)))
    }

    fn summary_of(network: &Network, address: &str) -> Result<PrinterSummary, Failure> {
        (network.query)(address).recv_timeout(WAIT).expect("the query answered")
    }

    fn finish(handle: JobHandle) -> (Vec<JobUpdate>, Result<Report, Failure>) {
        let result = handle.result.recv_timeout(WAIT).expect("the job ended");
        let updates = handle.updates.map(|rx| rx.try_iter().collect()).unwrap_or_default();
        (updates, result)
    }

    fn job(confirmed: bool) -> NetworkJob {
        NetworkJob { name: "Book1".into(), pdf: Arc::new(b"%PDF-1.7\n%%EOF\n".to_vec()), confirmed }
    }

    #[test]
    fn the_protocol_follows_the_address() {
        for (address, expected) in [
            ("http://192.168.1.50:5357/service", Protocol::Wsd),
            ("192.168.1.50:5357/0f1e2d3c", Protocol::Wsd),
            ("ipp://192.168.1.50/ipp/print", Protocol::Ipp),
            ("http://192.168.1.50:631/ipp/print", Protocol::Ipp),
            ("http://192.168.1.50/ipp", Protocol::Ipp),
            ("http://192.168.1.50/ippx", Protocol::Wsd),
        ] {
            assert_eq!(Protocol::of(address), expected, "{address}");
        }
    }

    #[test]
    fn only_numeric_local_addresses_are_accepted() {
        for ok in [ADDRESS, "http://192.168.1.50:5357/service", "http://169.254.1.2/x", "ipp://10.0.0.7/ipp/print"] {
            assert!(validate(ok).is_ok(), "{ok}");
        }
        for refused in ["http://printer.local/service", "http://8.8.8.8/service", "ipps://10.0.0.7/ipp/print", "http://example.com/ipp/print", ""] {
            let failure = validate(refused).expect_err(refused);
            assert_eq!(failure.kind, FailureKind::Policy, "{refused}");
        }
    }

    #[test]
    fn details_map_onto_the_three_state_options() {
        use craft_print::{DocumentFormat, JobHint};
        let unknown = caps_of(&PrinterDetails::default());
        assert_eq!(unknown.sides, Capability::Unknown);
        assert_eq!(unknown.color_modes, Capability::Unknown);
        assert_eq!(unknown.copies, Capability::Unknown);
        assert_eq!(unknown.orientations, Capability::Unknown);
        assert!(unknown.formats.is_empty(), "no formats: the printer is left to answer for itself");
        let mono_simplex = caps_of(&PrinterDetails {
            formats: vec![DocumentFormat::Pdf],
            two_sided: Some(false),
            colour: Some(false),
            max_copies: Some(1),
            honoured: Some(vec![JobHint::Copies]),
            ..PrinterDetails::default()
        });
        assert_eq!(mono_simplex.formats, vec!["application/pdf".to_owned()]);
        assert_eq!(mono_simplex.sides, Capability::from_option_list(Some(&["OneSided".to_owned()])));
        assert_eq!(mono_simplex.color_modes, Capability::from_option_list(Some(&["Monochrome".to_owned()])));
        assert_eq!(mono_simplex.copies, Capability::from_option_range(Some((1, 1))));
        assert_eq!(mono_simplex.orientations, Capability::Unsupported, "the backend says it ignores the hint");
        assert_eq!(mono_simplex.qualities, Capability::Unsupported);
        let duplex = caps_of(&PrinterDetails { two_sided: Some(true), colour: Some(true), max_copies: Some(10), ..PrinterDetails::default() });
        assert_eq!(duplex.sides.list().len(), 3);
        assert_eq!(duplex.color_modes.list().len(), 2);
        assert_eq!(duplex.copies, Capability::from_option_range(Some((1, 10))));
    }

    #[test]
    fn a_query_reads_the_printer_and_sends_no_job() {
        let printer = FakePrinter::new(FakeConfig::default());
        let network = service(None, Some(transport(&printer)));
        let summary = summary_of(&network, ADDRESS).expect("queried");
        assert!(summary.caps.formats.iter().any(|f| f == "application/pdf"), "{:?}", summary.caps.formats);
        assert_eq!(summary.color, Some(true));
        assert!(summary.caps.sides.list().iter().any(|s| s == "TwoSidedLongEdge"));
        assert_eq!(printer.jobs(), 0, "a query creates no job");
        assert!(!printer.actions().iter().any(|a| a.contains("CreatePrintJob") || a.contains("SendDocument")), "{:?}", printer.actions());
    }

    #[test]
    fn a_confirmed_job_is_sent_followed_and_reported_with_the_printers_name() {
        let printer = FakePrinter::new(FakeConfig::default());
        let network = service(None, Some(transport(&printer)));
        let summary = summary_of(&network, ADDRESS).expect("queried");
        let request = Request { copies: Some(2), sides: Some(net::Sides::LongEdge), expected_pages: Some(1), ..Request::default() };
        let (updates, result) = finish((network.send)(job(true))(ADDRESS, request));
        let report = result.expect("sent");
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(report.printer, summary.name, "the status names the printer the person queried");
        assert!(matches!(updates.first(), Some(JobUpdate::Accepted(_))), "accepted comes first");
        assert_eq!(printer.jobs(), 1);
        let stored = printer.job(1).expect("job 1");
        assert!(stored.document.as_deref().is_some_and(|d| d.starts_with(b"%PDF")), "the PDF arrived as it is");
        assert_eq!(stored.name, "Book1");
    }

    #[test]
    fn a_job_without_the_confirmation_is_refused_before_anything_is_sent() {
        let printer = FakePrinter::new(FakeConfig::default());
        let network = service(None, Some(transport(&printer)));
        let (_, result) = finish((network.send)(job(false))(ADDRESS, Request::default()));
        assert_eq!(result.expect_err("unconfirmed").kind, FailureKind::Invalid);
        assert!(printer.log().is_empty(), "nothing reached the printer");
    }

    #[test]
    fn a_printer_that_lists_no_pdf_is_refused_before_a_job_exists() {
        let printer = FakePrinter::new(FakeConfig { formats: vec!["image/pwg-raster"], ..FakeConfig::default() });
        let network = service(None, Some(transport(&printer)));
        let summary = summary_of(&network, ADDRESS).expect("queried");
        assert!(!summary.takes_any(network.formats), "craft-ui blocks Print for this printer");
        let (_, result) = finish((network.send)(job(true))(ADDRESS, Request::default()));
        assert_eq!(result.expect_err("no common format").kind, FailureKind::NoCommonFormat);
        assert_eq!(printer.jobs(), 0);
    }

    #[test]
    fn a_printer_that_aborts_the_job_is_reported_as_stopped() {
        let printer = FakePrinter::new(FakeConfig { ending: Ending::Aborts("AbortedBySystem"), ..FakeConfig::default() });
        let network = service(None, Some(transport(&printer)));
        summary_of(&network, ADDRESS).expect("queried");
        let (_, result) = finish((network.send)(job(true))(ADDRESS, Request::default()));
        assert_eq!(result.expect("accepted").outcome, Outcome::Aborted);
    }

    /// The Print page's whole path: the app builds the PDF of the workbook, craft-ui starts the job
    /// only after the query and the ticked confirmation, and the fake printer receives a PDF.
    #[test]
    fn the_app_prints_its_workbook_to_a_network_printer() {
        let printer = FakePrinter::new(FakeConfig::default());
        let services = gridcraft_ui_egui::Services { network_print: Some(service(None, Some(transport(&printer)))), ..Default::default() };
        let mut app = gridcraft_ui_egui::SheetApp::new(gridcraft_engine::Session::new(), services);
        app.run("file.new", json!({})).expect("a workbook");
        app.run("cell.set", json!({"cell": "A1", "input": "network print"})).expect("cell set");
        app.network.selected = true;
        let ctx = egui::Context::default();
        let wait = |app: &mut gridcraft_ui_egui::SheetApp, done: &dyn Fn(&gridcraft_ui_egui::SheetApp) -> bool| {
            // 20 ms a turn: the clock is off limits (it traps on wasm), so count the turns.
            for _ in 0..1000 {
                if done(app) {
                    return;
                }
                app.network.state.poll(&ctx);
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("timed out");
        };

        // Nothing starts before the printer was queried and the box ticked.
        app.network.state.draft.url = ADDRESS.into();
        assert!(app.print_network(json!({}), Some(1), None).is_err());
        assert_eq!(printer.jobs(), 0);

        let net = app.services.network_print.as_ref().expect("service");
        assert!(app.network.state.start_query(&net.query, (net.validate)(ADDRESS).is_ok()));
        wait(&mut app, &|a| !a.network.state.draft.querying());
        assert!(app.print_network(json!({}), Some(1), None).is_err(), "the confirmation is still missing");
        assert_eq!(printer.jobs(), 0);

        app.network.state.draft.confirmed = true;
        app.print_network(json!({}), Some(1), None).expect("started");
        assert!(!app.network.state.draft.confirmed, "the confirmation covers one print");
        wait(&mut app, &|a| !a.network.state.draft.running());
        assert!(
            matches!(&app.network.state.draft.job, net::JobView::Finished(r) if r.outcome == Outcome::Completed),
            "{:?}",
            app.network.state.draft.job
        );
        assert_eq!(printer.jobs(), 1);
        let stored = printer.job(1).expect("job 1");
        assert!(stored.document.as_deref().is_some_and(|d| d.starts_with(b"%PDF")));
    }
}
