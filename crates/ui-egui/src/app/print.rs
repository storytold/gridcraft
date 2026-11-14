use super::{Json, SheetApp, json, now_ms, printing};

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

    /// Submit a job using missing options from the open Print page; explicit control options win.
    /// Null and empty-object parameters inherit the same selected print options.
    pub(super) fn submit_print(&mut self, mut params: Json) -> Result<Json, String> {
        if params.is_null() {
            params = json!({});
        }
        if let Some(bs) = &self.backstage
            && bs.page == crate::backstage::Page::Print
            && let Some(map) = params.as_object_mut()
        {
            map.entry("sheets").or_insert_with(|| json!(if bs.print_all { "all" } else { "active" }));
            if !map.contains_key("scaling") && !map.contains_key("fitToPage") {
                map.insert("scaling".into(), json!(bs.scaling.id()));
            }
        }
        if self.services.print_native.is_some() && !self.printing.ready() {
            return Err("Select an available printer in File > Print before submitting.".into());
        }
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
        Ok(json!({"printing": true, "bytes": len, "pages": pdf["pages"], "pending": self.services.print_native.is_some()}))
    }

    /// Terminal feedback also reaches the normal window when printing starts through control.
    /// Polls printing feedback. Called once per frame from [`Self::logic`]; the direct calls in
    /// `tests/native_print.rs` deliberately bypass the lifecycle to test this helper alone.
    pub fn poll_printing(&mut self, ctx: &egui::Context) {
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
