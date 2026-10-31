//! Optional provider saves are UI operations; engine and control-channel calls stay local.

use crate::file_providers::{Capture, Command, Host};
use crate::{SheetApp, json};

impl SheetApp {
    pub(crate) fn choose_provider_save(&mut self, save_as: bool) -> bool {
        let Some(host) = self.services.file_providers.clone().filter(|host| host.enabled()) else { return false };
        if let Err(error) = self.settle_drafts() {
            self.alert(error);
            return true;
        }
        self.store_view_state();
        let Some(doc) = self.session.active() else { return true };
        let name = host.filename(doc.uid).unwrap_or_else(|| {
            let title = doc.display_title();
            let stem = std::path::Path::new(&title).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book");
            let extension = if doc.wb.vba_project.is_some() { "xlsm" } else { "xlsx" };
            format!("{stem}.{extension}")
        });
        if let Err(error) = host.choose(doc.uid, doc.revision, name, save_as) {
            self.alert(error);
        }
        true
    }

    pub(crate) fn provider_tick(&mut self) {
        let Some(host) = self.services.file_providers.clone() else { return };
        for command in host.take_commands() {
            match command {
                command @ Command::Save { .. } => self.provider_save(&host, command),
                command @ Command::Result { .. } => self.provider_result(&host, command),
                command @ Command::Open { .. } => self.provider_open(&host, command),
            }
        }
        let ids: Vec<u64> = self.session.documents().iter().map(|doc| doc.uid).collect();
        let documents = self
            .session
            .documents()
            .iter()
            .map(|doc| {
                json!({"id":doc.uid.to_string(), "title":doc.display_title(), "revision":doc.revision,
            "dirty":doc.is_dirty(), "provider":host.bound(doc.uid), "pending":host.pending_for(doc.uid)})
            })
            .collect::<Vec<_>>();
        host.publish_documents(json!(documents), &ids);
    }

    fn provider_mark_saved(&mut self, id: u64, revision: u64) {
        let _ = self.session.with_target(gridcraft_engine::Target::Doc(id), |session| {
            if let Some(doc) = session.active_mut()
                && doc.revision == revision
            {
                doc.saved = doc.wb.clone();
            }
        });
    }
    fn provider_save(&mut self, host: &Host, command: Command) {
        let Command::Save { request, doc, destination, name, save_as } = command else { return };
        let encoded = self
            .session
            .documents()
            .iter()
            .find(|state| state.uid == doc)
            .ok_or_else(|| "The workbook was closed before saving".to_string())
            .and_then(|state| {
                let extension = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                if !matches!(extension.as_str(), "xlsx" | "xlsm" | "json") {
                    return Err("Save requires a workbook package (.xlsx, .xlsm, .json). Use Export for a sheet copy.".into());
                }
                if state.wb.vba_project.is_some() && extension == "xlsx" {
                    return Err("This workbook contains VBA. Choose .xlsm to preserve it.".into());
                }
                let bytes = gridcraft_engine::io::save_bytes(&state.wb, &name).map_err(|error| error.to_string())?;
                Ok((state.revision, bytes))
            });
        match encoded {
            Ok((revision, bytes)) if destination == "provider" => {
                if let Err(error) = host.capture(&request, Capture { doc, revision, name, save_as }, bytes) {
                    host.failed(&request, &error);
                    self.alert(error);
                }
            }
            Ok((revision, bytes)) => {
                let result = self
                    .services
                    .provider_download
                    .as_ref()
                    .ok_or_else(|| "No device download service is available".to_string())
                    .and_then(|download| download(&name, &bytes));
                match result {
                    Ok(()) => {
                        if !host.bound(doc) {
                            self.provider_mark_saved(doc, revision);
                        }
                        host.finish_device(&request);
                        self.toast = Some((format!("Downloaded {name}"), crate::now_ms()));
                    }
                    Err(error) => {
                        host.failed(&request, &error);
                        self.alert(error);
                    }
                }
            }
            Err(error) => {
                host.failed(&request, &error);
                self.alert(error);
            }
        }
    }
    fn provider_result(&mut self, host: &Host, command: Command) {
        let Command::Result { capture, resource, status } = command else { return };
        if status == "saved" && self.session.documents().iter().any(|doc| doc.uid == capture.doc) {
            let aliases = host.bind(&capture, resource);
            for id in aliases {
                let _ = self.session.with_target(gridcraft_engine::Target::Doc(id), |session| {
                    if let Some(doc) = session.active_mut() {
                        doc.saved = std::sync::Arc::new((*doc.saved).clone());
                    }
                });
            }
            self.provider_mark_saved(capture.doc, capture.revision);
            self.toast = Some((format!("Saved {} to provider", capture.name), crate::now_ms()));
        } else if status == "failed" {
            self.alert("Provider save failed. Your workbook is still open.".into());
        }
    }
    fn provider_open(&mut self, host: &Host, command: Command) {
        let Command::Open { request, resource: _, name, bytes } = command else { return };
        let data = gridcraft_engine::io::base64_encode(&bytes);
        match self.run("file.open", json!({"name":name,"base64":data})) {
            Ok(result) => {
                self.after_open(None, &result);
                let capture = self.session.active().map(|doc| Capture { doc: doc.uid, revision: doc.revision, name, save_as: false });
                host.opened(&request, capture, "The imported workbook has no document");
            }
            Err(error) => {
                // Password-required and malformed bytes cannot bind the old active workbook.
                host.opened(&request, None, &error);
                self.alert(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use crate::file_providers::Host;
    use crate::{Services, SheetApp, json};

    fn app() -> (SheetApp, Host) {
        let host = Host::default();
        host.set_session("0123456789abcdef0123456789abcdef".into()).unwrap();
        host.connect("test".into(), "Test storage".into(), "account".into()).unwrap();
        let services = Services { file_providers: Some(host.clone()), ..Default::default() };
        let mut app = SheetApp::new(gridcraft_engine::Session::new(), services);
        app.session.run("file.new", json!({})).unwrap();
        app.session.run("cell.set", json!({"cell":"A1","input":"Guardado con ñ"})).unwrap();
        (app, host)
    }

    fn save(app: &mut SheetApp, host: &Host) -> (String, Vec<u8>) {
        assert!(app.choose_provider_save(false));
        let dialog = host.take_events().remove(0);
        let request = dialog.metadata["requestId"].as_str().unwrap().to_string();
        host.resolve_dialog(&request, "provider", "Workbook.xlsx").unwrap();
        app.provider_tick();
        let event = host.take_events().into_iter().find(|e| e.metadata["type"] == "save").unwrap();
        (request, event.bytes.unwrap())
    }

    #[test]
    fn xlsx_save_and_reopen_are_durable_and_keep_device_recents_separate() {
        let (mut app, host) = app();
        let (request, bytes) = save(&mut app, &host);
        assert!(app.session.active().unwrap().is_dirty());
        let (workbook, _) = gridcraft_engine::io::open_bytes("Workbook.xlsx", &bytes).unwrap();
        assert_eq!(workbook.sheets.len(), app.session.active().unwrap().wb.sheets.len());
        host.result(request, "saved".into(), "resource-one".into()).unwrap();
        app.provider_tick();
        assert!(!app.session.active().unwrap().is_dirty());
        let open = host.request_open("resource-one").unwrap();
        host.open(open, "Workbook.xlsx".into(), bytes).unwrap();
        app.provider_tick();
        assert_eq!(app.session.documents().len(), 2);
        assert!(app.session.active().unwrap().path.is_none());
        assert!(app.ui.recent.is_empty());
    }

    #[test]
    fn edits_after_capture_and_failures_stay_dirty() {
        let (mut app, host) = app();
        let (request, _) = save(&mut app, &host);
        app.session.run("cell.set", json!({"cell":"B2","input":"42"})).unwrap();
        host.result(request, "saved".into(), "resource-one".into()).unwrap();
        app.provider_tick();
        assert!(app.session.active().unwrap().is_dirty());
        let (request, _) = save(&mut app, &host);
        host.result(request, "failed".into(), String::new()).unwrap();
        app.provider_tick();
        assert!(app.session.active().unwrap().is_dirty());
    }
}
