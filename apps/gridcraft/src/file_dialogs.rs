//! The system file dialogs (Open, Save As, Export, pictures), shown without blocking the frame
//! (#901, #385).
//!
//! The blocking `rfd::FileDialog` ran `-[NSSavePanel runModal]` inside winit's event handler. A
//! trackpad gesture or a drag in that nested run loop ran the closures winit had queued, winit's
//! re-entrancy panic fired inside an Objective-C block, where it can't unwind, and the app
//! aborted (winit #3992). Now the app's request is queued; the frame (which has the window)
//! starts an `rfd::AsyncFileDialog` parented to the window (a sheet on macOS, which returns at
//! once), and a helper thread waits for the answer, hands it to the app and wakes the UI. Same
//! approach as WordCraft's and PhotoCraft's file dialogs.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use gridcraft_ui_egui::Picked;

/// A dialog the app asked for.
enum Request {
    Open,
    Picture,
    /// Save under a suggested name; its extension picks the file type listed first.
    Save(String),
}

/// A request with the channel its answer goes back through.
type Queued = (Request, Sender<Option<String>>);

/// A dialog's answer, waited for off the UI thread.
type Picking = Pin<Box<dyn Future<Output = Option<rfd::FileHandle>> + Send>>;

/// Queues the app's requests (the `Services::pick_*` hooks hold one each).
#[derive(Clone)]
pub struct Asker(Sender<Queued>);

impl Asker {
    fn ask(&self, request: Request) -> Picked {
        let (answer, picked) = channel();
        // If the launcher is gone, the dropped answer reads as a cancel in the app.
        let _ = self.0.send((request, answer));
        picked
    }

    pub fn open(&self) -> Picked {
        self.ask(Request::Open)
    }

    pub fn picture(&self) -> Picked {
        self.ask(Request::Picture)
    }

    pub fn save(&self, suggested: &str) -> Picked {
        self.ask(Request::Save(suggested.to_string()))
    }
}

/// Shows the dialogs the app queued; lives in the eframe app, which has the window.
pub struct Launcher(Receiver<Queued>);

pub fn launcher() -> (Asker, Launcher) {
    let (queue, queued) = channel();
    (Asker(queue), Launcher(queued))
}

impl Launcher {
    /// Starts the queued dialogs with the window as their parent. Call after every pass that may
    /// ask for one.
    pub fn show(&self, ctx: &egui::Context, parent: &eframe::Frame) {
        while let Ok((request, answer)) = self.0.try_recv() {
            // Built here, on the UI thread: macOS shows the panel as a sheet on the window now.
            let dialog = rfd::AsyncFileDialog::new().set_parent(parent);
            let picking: Picking = match request {
                Request::Open => Box::pin(
                    dialog
                        .add_filter("Spreadsheets", &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "json"])
                        .add_filter("Excel Workbook", &["xlsx", "xlsm"])
                        .add_filter("Excel 97-2003 Workbook (import)", &["xls"])
                        .add_filter("Excel Binary Workbook (data import)", &["xlsb"])
                        .add_filter("OpenDocument Spreadsheet (data import)", &["ods"])
                        .add_filter("CSV", &["csv"])
                        .pick_file(),
                ),
                Request::Picture => Box::pin(dialog.add_filter("Pictures (PNG, JPEG)", &["png", "jpg", "jpeg"]).pick_file()),
                Request::Save(suggested) => Box::pin(save_dialog(dialog, &suggested).save_file()),
            };
            let ctx = ctx.clone();
            let waiting = std::thread::Builder::new().name("file dialog".into()).spawn(move || {
                let _ = answer.send(block_on(picking).and_then(|f| f.path().to_str().map(str::to_string)));
                ctx.request_repaint();
            });
            // The answer's sender went with the closure, so the app sees the dialog as cancelled.
            if let Err(e) = waiting {
                log::error!("couldn't wait for the file dialog: {e}");
            }
        }
    }
}

/// The Save dialog: the suggested name, with its type (an export asks for .pdf, .csv or .html)
/// listed first.
fn save_dialog(dialog: rfd::AsyncFileDialog, suggested: &str) -> rfd::AsyncFileDialog {
    let path = std::path::Path::new(suggested);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Book1").to_string();
    const TYPES: [(&str, &str); 4] =
        [("Excel Workbook (.xlsx)", "xlsx"), ("CSV UTF-8 (.csv)", "csv"), ("Web Page (.html)", "html"), ("PDF (.pdf)", "pdf")];
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).filter(|e| TYPES.iter().any(|(_, x)| x == e));
    let ext = ext.unwrap_or_else(|| "xlsx".to_string());
    let mut dialog = dialog.set_file_name(format!("{stem}.{ext}"));
    for (label, x) in TYPES.iter().filter(|(_, x)| *x == ext).chain(TYPES.iter().filter(|(_, x)| *x != ext)) {
        dialog = dialog.add_filter(*label, &[*x]);
    }
    dialog
}

/// Runs `future` to completion on this thread, sleeping until it is woken.
fn block_on<T>(future: impl Future<Output = T>) -> T {
    struct Unpark(std::thread::Thread);
    impl std::task::Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        // Spurious wake-ups just poll again.
        std::thread::park();
    }
}
