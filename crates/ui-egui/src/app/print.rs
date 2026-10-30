use super::{Json, SheetApp, json, now_ms, printing};
use crate::{backstage, i18n};

impl SheetApp {
    /// Web: `file.exportPdf` without a path returns the PDF as base64; hand it to the browser as a download.
    pub(super) fn deliver_pdf(&self, result: &Json) {
        if result.get("path").is_some() {
            return;
        }
        if let Some(dl) = &self.services.download
            && let Some(b) = result.get("base64").and_then(Json::as_str).and_then(gridcraft_engine::io::base64_decode)
        {
            let title = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
            dl(&std::path::Path::new(&title).with_extension("pdf").to_string_lossy(), &b);
        }
    }

    /// Asks for a `.pdf` path with the save picker, then re-runs `file.exportPdf` with it. Other
    /// params (range, sheets, fitToPage) are kept. A cancelled picker does nothing.
    pub(super) fn export_pdf_with_picker(&mut self, mut params: Json) {
        if !self.commit_edit(0, 0, false, false) {
            return;
        }
        let title = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "Book1".into());
        let stem = std::path::Path::new(&title).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book1").to_string();
        let Some(pick) = &self.services.pick_save else { return };
        let Some(path) = pick(&format!("{stem}.pdf")) else { return };
        // The picker may suggest a workbook extension; the export writes a PDF.
        let path = std::path::Path::new(&path).with_extension("pdf").to_string_lossy().into_owned();
        match &mut params {
            Json::Object(map) => {
                map.insert("path".into(), Json::String(path));
            }
            other => *other = json!({"path": path}),
        }
        if let Err(e) = self.run("file.exportPdf", params) {
            self.alert(e);
        }
    }

    /// Builds the selected sheets' print-ready PDF and hands it to the desktop worker or the
    /// browser. Rendering errors use the usual alert; asynchronous spooler errors stay visible
    /// on the Print page. The engine command still returns PDF bytes without submitting.
    pub fn print_now(&mut self, params: Json) {
        if let Err(error) = self.submit_print(params) {
            self.alert(error);
        }
    }

    pub(super) fn submit_print(&mut self, mut params: Json) -> Result<Json, String> {
        require_params_object(&params, self.ui.language)?;
        if self.services.print_native.is_some() && !self.printing.ready() {
            return Err("Select an available printer in File > Print before submitting.".into());
        }
        // A driver may ignore the monochrome hint, so the PDF itself is gray when Grayscale is ticked.
        params["grayscale"] = json!(self.services.print_native.is_some() && self.printing.settings.grayscale);
        let pdf = self.run("file.print", params)?;
        let encoded = pdf.get("base64").and_then(Json::as_str).ok_or("The print command did not produce a PDF.")?;
        let bytes = gridcraft_engine::io::base64_decode(encoded).ok_or("Couldn't decode the generated PDF.")?;
        let len = bytes.len();
        if let Some(submit) = &self.services.print_native {
            self.printing.begin(submit(bytes, self.printing.settings.clone())).map_err(|_| "A print job is already being submitted.".to_owned())?;
            self.print_feedback_seen = None;
        } else if let Some(print) = &self.services.print {
            print(&bytes);
        } else {
            return Err("Printing is unavailable here; export a PDF instead.".into());
        }
        Ok(json!({"printing": true, "bytes": len, "pending": self.services.print_native.is_some()}))
    }

    /// Whether the network printer is the Print page's chosen destination.
    pub fn network_selected(&self) -> bool {
        self.network.selected && self.services.network_print.is_some()
    }

    /// Whether the pages print gray: the chosen destination's Grayscale choice (a queue's box, or the
    /// network printer's Color setting). The Print preview and the PDF that is sent both follow it.
    pub fn print_grayscale(&self) -> bool {
        if self.network_selected() {
            self.network.state.draft.request(None).color == Some(printing::net::ColorMode::Monochrome)
        } else {
            self.services.print_native.is_some() && self.printing.settings.grayscale
        }
    }

    /// Builds the selected sheets' print-ready PDF and sends it to the queried network printer on a
    /// worker, following the job. Needs the ticked confirmation (craft-ui spends it); returns the
    /// reason it did not start otherwise. `pages` and `media` describe the sheets sent.
    pub fn print_network(&mut self, mut params: Json, pages: Option<u32>, media: Option<&str>) -> Result<(), String> {
        require_params_object(&params, self.ui.language)?;
        let Some(net) = &self.services.network_print else { return Err(self.ui.language.tr("Network printing is unavailable here.").into()) };
        if self.network.state.draft.block((net.validate)(&self.network.state.draft.normalized_url()).is_ok(), true, net.formats).is_some() {
            return Err(self.ui.language.tr("Query the printer and tick the confirmation before printing.").into());
        }
        params["grayscale"] = json!(self.print_grayscale());
        let pdf = self.run("file.print", params)?;
        let encoded = pdf.get("base64").and_then(Json::as_str).ok_or("The print command did not produce a PDF.")?;
        let bytes = gridcraft_engine::io::base64_decode(encoded).ok_or("Couldn't decode the generated PDF.")?;
        let name = self.session.active().map(|d| d.display_title()).unwrap_or_else(|| "GridCraft document".into());
        let Some(net) = &self.services.network_print else { return Err(self.ui.language.tr("Network printing is unavailable here.").into()) };
        let address = self.network.state.draft.normalized_url();
        let send = (net.send)(printing::NetworkJob { name, pdf: std::sync::Arc::new(bytes), confirmed: self.network.state.draft.confirmed });
        self.network.state.draft.expected_pages = pages;
        // A paper size goes along only when the printer lists it: a printer that did not say what it
        // holds gets no media request, so the job cannot be refused for a size it never listed.
        let media = media.filter(|_| matches!(self.network.state.draft.media_state(), printing::Capability::Supported(_)));
        if self.network.state.start_job(&send, (net.validate)(&address).is_ok(), true, net.formats, media) {
            Ok(())
        } else {
            Err(self.ui.language.tr("The network printer did not start the job.").into())
        }
    }

    /// Collect the network destination's finished work (each frame, also while the Print page is
    /// closed). With the page closed the person still hears how the job went.
    fn poll_network(&mut self, ctx: &egui::Context) {
        let was_running = self.network.state.draft.running();
        self.network.state.poll(ctx);
        if !was_running || self.network.state.draft.running() {
            return;
        }
        let on_print_page = self.backstage.as_ref().is_some_and(|b| b.page == backstage::Page::Print);
        let labels = printing::network_labels(self.ui.language);
        let Some(text) = printing::network_notice(&self.network.state.draft.job, &labels) else { return };
        if on_print_page {
            return;
        }
        if matches!(self.network.state.draft.job, printing::net::JobView::Failed(_)) {
            self.alert(text);
        } else {
            self.toast = Some((text, now_ms()));
        }
    }

    /// Terminal feedback also reaches the normal window when printing starts through control.
    /// Polls printing feedback. Called once per frame from [`Self::logic`]; the direct calls in
    /// `tests/native_print.rs` deliberately bypass the lifecycle to test this helper alone.
    pub fn poll_printing(&mut self, ctx: &egui::Context) {
        self.poll_network(ctx);
        self.printing.poll(ctx);
        if self.printing.message == self.print_feedback_seen {
            return;
        }
        self.print_feedback_seen = self.printing.message.clone();
        let Some(message @ (printing::Feedback::Submitted(_) | printing::Feedback::Failed(_) | printing::Feedback::WorkerStopped)) =
            self.printing.message.clone()
        else {
            return;
        };
        let text = printing::feedback(&message, self.ui.language, |label| self.ui.language.tr(label).to_owned());
        if matches!(message, printing::Feedback::Submitted(_)) {
            self.toast = Some((text, now_ms()));
        } else {
            self.alert(text);
        }
    }
}

/// Print parameters are an object (`null` means none): the code adds keys to them, which would panic on
/// any other JSON value.
fn require_params_object(params: &Json, lang: i18n::Language) -> Result<(), String> {
    if params.is_null() || params.is_object() { Ok(()) } else { Err(lang.tr("Print parameters must be an object.").into()) }
}
