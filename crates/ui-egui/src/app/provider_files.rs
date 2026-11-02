//! Optional provider persistence is UI state; ordinary engine file commands stay local.

use crate::file_providers::{Capture, Command, Host};
use crate::{SheetApp, json};

impl SheetApp {
    pub(super) fn open_provider_resource(&self, params: &crate::Json) -> Result<crate::Json, String> {
        let resource = params.get("resource").and_then(crate::Json::as_str).ok_or("Missing provider resource ID")?;
        let host = self.services.file_providers.as_ref().ok_or("No file provider is configured")?;
        let request = host.request_open(resource)?;
        Ok(json!({"pending":"fileProvider","requestId":request}))
    }

    pub(crate) fn choose_provider_save(&mut self, save_as: bool) -> bool {
        let Some(host) = self.services.file_providers.clone().filter(|host| host.enabled()) else { return false };
        if let Err(error) = self.settle_drafts() {
            if !self.message.as_ref().is_some_and(|(_, message)| *message == error) {
                self.alert(error);
            }
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
            "dirty":doc.is_dirty()||self.provider_drafts_for(doc.uid), "provider":host.bound(doc.uid), "pending":host.pending_for(doc.uid)})
            })
            .collect::<Vec<_>>();
        host.publish_documents(json!(documents), &ids);
    }

    fn provider_mark_saved(&mut self, id: u64, revision: u64) {
        if self.provider_drafts_for(id) {
            return;
        }
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
        if let Err(error) = self.settle_provider_drafts(doc) {
            host.failed(&request, &error);
            self.alert(error);
            return;
        }
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
            Ok((_, bytes)) => {
                let result = self
                    .services
                    .provider_download
                    .as_ref()
                    .ok_or_else(|| "No device download service is available".to_string())
                    .and_then(|download| download(&name, &bytes));
                match result {
                    Ok(()) => {
                        // The browser accepted the download; its picker/write can still fail.
                        host.finish_device(&request);
                        self.toast = Some((format!("Download started for {name}"), crate::now_ms()));
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
                        doc.revision = doc.revision.wrapping_add(1);
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
        let Command::Open { request, name, bytes } = command else { return };
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
        let dialog = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
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

    #[test]
    fn newer_alias_overwrite_invalidates_the_revision_before_an_old_ack() {
        let (mut app, host) = app();
        let (request, bytes) = save(&mut app, &host);
        host.result(request, "saved".into(), "shared".into()).unwrap();
        app.provider_tick();
        let read = host.request_open("shared").unwrap();
        host.open(read, "Workbook.xlsx".into(), bytes).unwrap();
        app.provider_tick();
        assert_eq!(app.session.documents().len(), 2);
        app.session.set_active(0);
        app.follow_active_window();
        app.session.run("cell.set", json!({"cell":"A2","input":"First"})).unwrap();
        let (old, _) = save(&mut app, &host);
        app.session.set_active(1);
        app.follow_active_window();
        app.session.run("cell.set", json!({"cell":"A2","input":"Second"})).unwrap();
        let (new, _) = save(&mut app, &host);
        host.result(new, "saved".into(), "shared".into()).unwrap();
        app.provider_tick();
        host.result(old, "saved".into(), "shared".into()).unwrap();
        app.provider_tick();
        assert!(app.session.documents()[0].is_dirty());
    }
    #[test]
    fn resolving_a_save_commits_only_its_parked_workbook_draft() {
        let (mut app, host) = app();
        app.follow_active_window();
        let first = app.session.active().unwrap().uid;
        assert!(app.choose_provider_save(false));
        let events = host.take_events();
        let request = events[0].metadata["requestId"].as_str().unwrap();
        app.begin_edit(Some("Late first workbook draft".into()), false);
        // An independent window can be active while the chooser waits.
        app.session.run("file.new", json!({})).unwrap();
        app.follow_active_window();
        let second = app.session.active().unwrap().uid;
        app.begin_edit(Some("Unrelated second draft".into()), false);
        host.resolve_dialog(request, "provider", "First.xlsx").unwrap();
        app.provider_tick();
        assert_eq!(app.session.active().unwrap().uid, second);
        assert_eq!(app.editor.as_ref().unwrap().text, "Unrelated second draft");
        let event = host.take_events().into_iter().find(|event| event.metadata["type"] == "save").unwrap();
        let (workbook, _) = gridcraft_engine::io::open_bytes("First.xlsx", event.bytes.as_ref().unwrap()).unwrap();
        let cell = workbook.sheets[0].cell(gridcraft_engine::core::CellRef::new(0, 0)).unwrap();
        assert_eq!(cell.input_text(), "Late first workbook draft");
        let first_index = app.session.documents().iter().position(|doc| doc.uid == first).unwrap();
        assert!(app.session.documents()[first_index].is_dirty());
    }
    #[test]
    fn an_uncommitted_cell_after_capture_is_not_acknowledged_as_saved() {
        let (mut app, host) = app();
        let (request, _) = save(&mut app, &host);
        app.begin_edit(Some("After capture".into()), false);
        host.result(request, "saved".into(), "resource-one".into()).unwrap();
        app.provider_tick();
        assert!(app.session.active().unwrap().is_dirty());
        assert_eq!(app.editor.as_ref().unwrap().text, "After capture");
        assert_eq!(host.snapshot()["documents"][0]["dirty"], true);
    }

    #[test]
    fn refused_provider_save_keeps_the_validation_rules_title_and_draft() {
        let (mut app, host) = app();
        app.follow_active_window();
        app.session
            .run(
                "data.validation",
                json!({"range":"A1","type":"whole","operator":"between","formula1":"1","formula2":"2",
                    "errorStyle":"stop","errorTitle":"Quantity rule","errorMessage":"Use one or two"}),
            )
            .unwrap();
        app.begin_edit(Some("9".into()), false);
        assert!(app.choose_provider_save(false));
        assert_eq!(app.message, Some(("Quantity rule".into(), "Use one or two".into())));
        assert_eq!(app.editor.as_ref().unwrap().text, "9");
        assert!(host.take_events().is_empty(), "a refused draft must not start a save chooser");
    }

    #[test]
    fn a_started_or_failed_device_download_never_acknowledges_workbook_persistence() {
        for bound in [false, true] {
            for accepted in [false, true] {
                let (mut app, host) = app();
                if bound {
                    let (request, _) = save(&mut app, &host);
                    host.result(request, "saved".into(), "stored-workbook".into()).unwrap();
                    app.provider_tick();
                    app.session.run("cell.set", json!({"cell":"B2","input":"Later edit"})).unwrap();
                }
                let delivered = std::rc::Rc::new(std::cell::Cell::new(false));
                let observed = delivered.clone();
                app.services.provider_download = Some(Box::new(move |_, bytes| {
                    assert!(!bytes.is_empty());
                    observed.set(true);
                    if accepted { Ok(()) } else { Err("The download was refused".into()) }
                }));
                assert!(app.choose_provider_save(false));
                let dialog = host.take_events().into_iter().find(|event| event.metadata["type"] == "saveDialog").unwrap();
                let request = dialog.metadata["requestId"].as_str().unwrap();
                host.resolve_dialog(request, "device", "Device.xlsx").unwrap();
                app.provider_tick();
                let doc = app.session.active().unwrap();
                assert!(delivered.get());
                assert!(doc.is_dirty(), "starting a picker or fallback is not a durable write");
                assert_eq!(host.bound(doc.uid), bound);
                assert!(!host.pending_for(doc.uid));
                assert_eq!(host.snapshot()["documents"][0]["dirty"], true);
            }
        }
    }

    fn resource_list(host: &Host) {
        let connection = host.snapshot()["connectionId"].as_str().unwrap().to_string();
        host.set_resources(
            &connection,
            vec![crate::file_providers::Resource { id: "recent-book".into(), name: "Recent.xlsx".into(), updated_at: String::new() }],
        )
        .unwrap();
    }

    #[test]
    fn provider_recent_open_is_discoverable_and_reachable_through_the_control_command() {
        let (mut app, host) = app();
        resource_list(&host);
        let ctx = egui::Context::default();
        let discovery = crate::control::handle(&mut app, &ctx, "engine.commands", &json!({"search":"openProviderResource"}));
        let crate::control::Outcome::Done(commands) = discovery else {
            panic!("command discovery must be synchronous");
        };
        assert!(commands["result"].as_array().unwrap().iter().any(|entry| entry["id"] == "file.openProviderResource" && entry["enabled"] == true));
        let crate::control::Outcome::Done(open) = crate::control::handle(
            &mut app,
            &ctx,
            "engine.execute",
            &json!({"command":"file.openProviderResource","params":{"resource":"recent-book"}}),
        ) else {
            panic!("opening a provider resource queues a host read, not a screenshot");
        };
        assert_eq!(open["ok"], true);
        let events = host.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metadata["type"], "openRequest");
        assert_eq!(events[0].metadata["resourceId"], "recent-book");
        assert_eq!(events[0].metadata["requestId"], open["result"]["requestId"]);
        assert_eq!(events[0].metadata["namespace"], "account");
        assert!(app.run("file.openProviderResource", json!({"resource":"forged"})).is_err());
        assert!(host.take_events().is_empty());
    }

    #[test]
    fn refused_provider_recent_open_uses_persistent_message_feedback() {
        let (mut app, host) = app();
        resource_list(&host);
        for _ in 0..16 {
            host.request_open("recent-book").unwrap();
        }
        host.take_events();
        app.run_or_alert("file.openProviderResource", json!({"resource":"recent-book"}));
        let message = app.message.clone().expect("the refusal must be visible after the click frame");
        assert!(message.1.contains("too many pending provider reads"));
        app.provider_tick();
        assert_eq!(app.message, Some(message));
        assert!(host.take_events().is_empty());
    }

    #[test]
    fn disabled_provider_rows_and_command_stay_inactive() {
        let (mut app, host) = app();
        resource_list(&host);
        assert_eq!(host.recent_entries().unwrap().1[0].id, "recent-book");
        host.set_enabled(false).unwrap();
        assert!(host.recent_entries().is_none());
        let discovery = crate::control::handle(&mut app, &egui::Context::default(), "engine.commands", &json!({"search":"openProviderResource"}));
        let crate::control::Outcome::Done(commands) = discovery else {
            panic!("command discovery must be synchronous");
        };
        assert!(commands["result"].as_array().unwrap().iter().any(|entry| entry["id"] == "file.openProviderResource" && entry["enabled"] == false));
        assert!(app.run("file.openProviderResource", json!({"resource":"recent-book"})).is_err());
        assert!(host.take_events().is_empty());
    }
}
