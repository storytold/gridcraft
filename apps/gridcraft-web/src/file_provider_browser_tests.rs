//! One original synthetic workbook drives the actual window publisher and SheetApp consumer.
//! Fault-injection helpers adapt sibling WordCraft work, copyright 2026 ArtCraft Team and
//! WordCraft contributors, under Apache-2.0 (repository LICENSE-APACHE).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use gridcraft_engine::{
    Session,
    core::CellRef,
    model::{DefinedName, Preserved, PreservedPart, PreservedRel, Sheet},
};
use gridcraft_ui_egui::{Services, SheetApp, file_providers::Host};
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use serde_json::json;
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

use super::{API, EVENT, error, install, publish};

wasm_bindgen_test_configure!(run_in_browser);

// Fault injection stays in Rust. The actual Function::call0 catches a recoverable exception;
// restore the exact own descriptor (or remove our shadow) before propagating that Result.
// Fatal WASM traps, true memory exhaustion and process termination are outside this fixture.
fn with_replacement(target: &JsValue, key: &str, replacement: &JsValue, callback: &Function) -> Result<Result<JsValue, JsValue>, JsValue> {
    let object = Reflect::get(&js_sys::global(), &"Object".into())?;
    let key = JsValue::from_str(key);
    let descriptor = call(&object, "getOwnPropertyDescriptor", &Array::of2(target, &key))?;
    let original = Reflect::get(target, &key)?;
    let outcome = match Reflect::set(target, &key, replacement) {
        Ok(true) => callback.call0(&JsValue::UNDEFINED),
        Ok(false) => Err(error("injected method is not writable")),
        Err(error) => Err(error),
    };
    if descriptor.is_undefined() {
        if !Reflect::delete_property(target.unchecked_ref::<Object>(), &key)? {
            return Err(error("could not remove injected method"));
        }
    } else {
        call(&object, "defineProperty", &Array::of3(target, &key, &descriptor))?;
    }
    let restored = call(&object, "getOwnPropertyDescriptor", &Array::of2(target, &key))?;
    if !Object::is(&Reflect::get(target, &key)?, &original) || restored.is_undefined() != descriptor.is_undefined() {
        return Err(error("injected property was not restored exactly"));
    }
    if !descriptor.is_undefined() {
        for attribute in ["value", "get", "set", "writable", "enumerable", "configurable"] {
            if !Object::is(&Reflect::get(&restored, &attribute.into())?, &Reflect::get(&descriptor, &attribute.into())?) {
                return Err(error("injected property descriptor changed"));
            }
        }
    }
    Ok(outcome)
}

fn fault_receipt(calls: u32, requests: &Array) -> Result<JsValue, JsValue> {
    let receipt = Object::new();
    for (key, value) in
        [("calls", JsValue::from_f64(f64::from(calls))), ("restored", JsValue::from_bool(true)), ("requests", JsValue::from(requests.clone()))]
    {
        if !Reflect::set(&receipt, &key.into(), &value)? {
            return Err(error("could not record restored fault injection"));
        }
    }
    Ok(receipt.into())
}

fn copy_failure(callback: &Function) -> Result<JsValue, JsValue> {
    let calls = Rc::new(Cell::new(0_u32));
    let observed = calls.clone();
    let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |_| {
        observed.set(observed.get().saturating_add(1));
        Err(js_sys::RangeError::new("injected GridCraft byte copy failure").into())
    });
    let array = Reflect::get(&js_sys::global(), &"Uint8Array".into())?;
    with_replacement(&array, "from", fault.as_ref(), callback)??;
    fault_receipt(calls.get(), &Array::new())
}

fn request_failure(callback: &Function) -> Result<JsValue, JsValue> {
    let window = web_sys::window().ok_or_else(|| error("no test window"))?;
    let original = Reflect::get(&window, &"dispatchEvent".into())?.dyn_into::<Function>()?;
    let receiver: JsValue = window.into();
    let target = receiver.clone();
    let calls = Rc::new(Cell::new(0_u32));
    let observed = calls.clone();
    let requests = Array::new();
    let rejected = requests.clone();
    let fault = Closure::<dyn FnMut(JsValue) -> Result<JsValue, JsValue>>::new(move |event: JsValue| {
        if Reflect::get(&event, &"type".into())?.as_string().as_deref() == Some(EVENT) {
            let detail = Reflect::get(&event, &"detail".into())?;
            let kind = Reflect::get(&detail, &"type".into())?.as_string();
            if matches!(kind.as_deref(), Some("saveDialog" | "openRequest")) {
                observed.set(observed.get().saturating_add(1));
                let request = Object::new();
                for name in ["type", "requestId"] {
                    if !Reflect::set(&request, &name.into(), &Reflect::get(&detail, &name.into())?)? {
                        return Err(error("could not record rejected request"));
                    }
                }
                rejected.push(&request);
                return Err(js_sys::Error::new("injected GridCraft request dispatch failure").into());
            }
        }
        original.call1(&receiver, &event)
    });
    with_replacement(&target, "dispatchEvent", fault.as_ref(), callback)??;
    fault_receipt(calls.get(), &requests)
}

struct Events {
    window: web_sys::Window,
    listener: Closure<dyn FnMut(web_sys::Event)>,
    details: Rc<RefCell<Vec<JsValue>>>,
    blocked_disconnects: Rc<RefCell<Vec<bool>>>,
    installed: Option<JsValue>,
}

impl Events {
    fn new(host: Host) -> Result<Self, JsValue> {
        let window = web_sys::window().ok_or_else(|| error("no test window"))?;
        assert!(!Reflect::has(&window, &API.into())?, "the fixture owns a fresh page without editor startup");
        let details = Rc::new(RefCell::new(Vec::new()));
        let blocked_disconnects = Rc::new(RefCell::new(Vec::new()));
        let received = details.clone();
        let blocked = blocked_disconnects.clone();
        let listener = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(event) = event.dyn_ref::<web_sys::CustomEvent>() {
                let detail = event.detail();
                if Reflect::get(&detail, &"type".into())
                    .ok()
                    .and_then(|value| value.as_string())
                    .is_some_and(|kind| matches!(kind.as_str(), "openFailed" | "saveFailed"))
                {
                    blocked.borrow_mut().push(host.disconnect().is_err());
                }
                received.borrow_mut().push(detail);
            }
        });
        window.add_event_listener_with_callback(EVENT, listener.as_ref().unchecked_ref())?;
        Ok(Self { window, listener, details, blocked_disconnects, installed: None })
    }

    fn matching(&self, kind: &str) -> Result<Vec<JsValue>, JsValue> {
        let mut found = Vec::new();
        for detail in self.details.borrow().iter() {
            if Reflect::get(detail, &"type".into())?.as_string().as_deref() == Some(kind) {
                found.push(detail.clone());
            }
        }
        Ok(found)
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        let _ = self.window.remove_event_listener_with_callback(EVENT, self.listener.as_ref().unchecked_ref());
        if let Some(owned) = &self.installed
            && let Ok(current) = Reflect::get(&self.window, &API.into())
            && Object::is(&current, owned)
        {
            let _ = Reflect::delete_property(self.window.unchecked_ref::<Object>(), &API.into());
        }
    }
}

fn call(api: &JsValue, method: &str, args: &Array) -> Result<JsValue, JsValue> {
    Reflect::get(api, &method.into())?.dyn_into::<Function>()?.apply(api, args)
}

fn field(value: &JsValue, name: &str) -> Result<String, JsValue> {
    Reflect::get(value, &name.into())?.as_string().ok_or_else(|| error(format!("missing {name}")))
}

fn frame(app: &mut SheetApp, ctx: &egui::Context) {
    let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
}

fn choose(app: &mut SheetApp) -> Result<(), JsValue> {
    assert_eq!(app.run("ui.chooseProviderSave", json!({"saveAs":false})).map_err(error)?["handled"], true);
    Ok(())
}

#[wasm_bindgen_test]
fn page_local_provider_delivery_failures_release_workbook_owners_and_recover() -> Result<(), JsValue> {
    let host = Host::default();
    let mut events = Events::new(host.clone())?;
    let ctx = egui::Context::default();
    ctx.set_fonts(gridcraft_ui_egui::theme::pinned_font_definitions());
    // Capture our API even if install publishes it and the ready dispatch then fails.
    // Events::new already established the page was blank; Drop removes only this identity.
    let installed = install(host.clone(), ctx.clone());
    let api = Reflect::get(&events.window, &API.into())?;
    events.installed = Some(api.clone());
    installed?;
    let connection = call(&api, "registerProvider", &Array::of3(&"test".into(), &"Test storage".into(), &"account".into()))?;
    let resources = Array::new();
    let resource = Object::new();
    assert!(Reflect::set(&resource, &"id".into(), &"later".into())?);
    assert!(Reflect::set(&resource, &"name".into(), &"Later.xlsm".into())?);
    resources.push(&resource);
    call(&api, "setResources", &Array::of2(&connection, &resources))?;
    let mut app = SheetApp::new(Session::new(), Services { file_providers: Some(host.clone()), ..Default::default() });
    app.use_prepared_fonts();
    app.session.run("file.new", json!({})).map_err(error)?;
    {
        let wb = Arc::make_mut(&mut app.session.active_mut().ok_or_else(|| error("missing fixture workbook"))?.wb);
        wb.sheets.push(Arc::new(Sheet::new("Second")));
        wb.names.push(DefinedName {
            name: "RecoveryValue".into(),
            scope: None,
            formula: "Sheet1!$A$1".into(),
            comment: String::new(),
            hidden: false,
        });
        wb.vba_project = Some(Arc::new(vec![7, 11, 23, 42]));
        wb.preserved = Some(Arc::new(Preserved {
            rels: vec![PreservedRel {
                id: "rIdRecovery".into(),
                rel_type: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml".into(),
                target: "customXml/recovery.xml".into(),
                external: false,
            }],
            parts: vec![PreservedPart {
                name: "customXml/recovery.xml".into(),
                content_type: "application/xml".into(),
                data: Arc::from(b"<recovery>original synthetic resource</recovery>".as_slice()),
                rels: Vec::new(),
            }],
            ..Default::default()
        }));
    }
    app.session.run("cell.set", json!({"cell":"A1","input":"Original con ñ"})).map_err(error)?;
    app.session.run("cell.set", json!({"cell":"B1","input":"=2+3"})).map_err(error)?;
    frame(&mut app, &ctx);
    let document = app.session.active().ok_or_else(|| error("missing workbook"))?.uid;
    let saved = app.session.active().ok_or_else(|| error("missing workbook"))?.saved.clone();
    choose(&mut app)?;
    publish(&host, &ctx);
    let dialogs = events.matching("saveDialog")?;
    assert_eq!(dialogs.len(), 1);
    let failed_request = field(&dialogs[0], "requestId")?;
    call(&api, "resolveSaveDialog", &Array::of3(&failed_request.clone().into(), &"provider".into(), &"Recovery.xlsm".into()))?;
    frame(&mut app, &ctx);
    let captured_revision = app.session.active().ok_or_else(|| error("missing captured workbook"))?.revision;
    app.session.run("cell.set", json!({"cell":"C1","input":"Newer edit survives"})).map_err(error)?;
    let newer = app.session.active().ok_or_else(|| error("missing newer workbook"))?.wb.clone();
    let newer_revision = app.session.active().ok_or_else(|| error("missing newer workbook"))?.revision;
    assert_ne!(captured_revision, newer_revision);
    let later = call(&api, "requestOpen", &Array::of1(&"later".into()))?;
    call(&api, "cancelOpen", &Array::of2(&later, &"later terminal survived".into()))?;
    let pending_host = host.clone();
    let pending_ctx = ctx.clone();
    let publisher = Closure::<dyn FnMut()>::new(move || publish(&pending_host, &pending_ctx));
    let fault = copy_failure(publisher.as_ref().unchecked_ref())?;
    assert_eq!(Reflect::get(&fault, &"calls".into())?.as_f64(), Some(1.0));
    assert_eq!(Reflect::get(&fault, &"restored".into())?.as_bool(), Some(true));
    assert!(events.matching("save")?.is_empty(), "failed copy must not emit partial workbook bytes");
    let later_events = events.matching("openFailed")?;
    assert_eq!(later_events.len(), 1, "a failed save copy must not discard the rest of the batch");
    assert_eq!(Reflect::get(&later_events[0], &"requestId".into())?, later);
    assert_eq!(field(&later_events[0], "error")?, "later terminal survived");
    assert!(host.pending_for(document));
    assert!(call(&api, "saveResult", &Array::of3(&failed_request.into(), &"saved".into(), &"late".into())).is_err());
    frame(&mut app, &ctx);
    let current = app.session.active().ok_or_else(|| error("missing failed-save workbook"))?;
    assert_eq!(current.uid, document);
    assert_eq!(current.revision, newer_revision);
    assert!(Arc::ptr_eq(&current.wb, &newer) && Arc::ptr_eq(&current.saved, &saved));
    assert!(current.is_dirty());
    assert!(!host.pending_for(document));
    assert_eq!(host.snapshot()["documents"][0]["provider"], false);
    assert!(app.message.as_ref().is_some_and(|(_, text)| text.contains("Provider save failed")));

    choose(&mut app)?;
    publish(&host, &ctx);
    let dialogs = events.matching("saveDialog")?;
    assert_eq!(dialogs.len(), 2);
    let retry = field(&dialogs[1], "requestId")?;
    call(&api, "resolveSaveDialog", &Array::of3(&retry.clone().into(), &"provider".into(), &"Recovery.xlsm".into()))?;
    frame(&mut app, &ctx);
    let retry_revision = app.session.active().ok_or_else(|| error("missing retry workbook"))?.revision;
    publish(&host, &ctx);
    let saves = events.matching("save")?;
    assert_eq!(saves.len(), 1);
    assert_eq!(field(&saves[0], "requestId")?, retry);
    assert_eq!(field(&saves[0], "name")?, "Recovery.xlsm");
    assert_eq!(Reflect::get(&saves[0], &"revision".into())?.as_f64(), Some(retry_revision as f64));
    let bytes = Reflect::get(&saves[0], &"bytes".into())?.dyn_into::<Uint8Array>()?.to_vec();
    assert_eq!(Reflect::get(&saves[0], &"bytesLength".into())?.as_f64(), Some(bytes.len() as f64));
    let (restored, _) = gridcraft_engine::io::open_bytes("Recovery.xlsm", &bytes).map_err(error)?;
    assert_eq!(restored.sheets.len(), 2);
    assert_eq!(restored.sheets[1].name, "Second");
    assert_eq!(restored.sheets[0].cell(CellRef::new(0, 1)).ok_or_else(|| error("formula cell missing"))?.input_text(), "=2+3");
    assert_eq!(restored.sheets[0].cell(CellRef::new(0, 2)).ok_or_else(|| error("newer cell missing"))?.input_text(), "Newer edit survives");
    assert!(restored.names.iter().any(|name| name.name == "RecoveryValue"));
    assert_eq!(restored.vba_project, newer.vba_project);
    let kept = restored
        .preserved
        .as_ref()
        .and_then(|parts| parts.parts.iter().find(|part| part.name == "customXml/recovery.xml"))
        .ok_or_else(|| error("opaque workbook resource missing"))?;
    assert_eq!(kept.data.as_ref(), b"<recovery>original synthetic resource</recovery>");
    assert!(app.session.active().ok_or_else(|| error("missing workbook"))?.is_dirty());
    call(&api, "saveResult", &Array::of3(&retry.into(), &"saved".into(), &"recovered".into()))?;
    frame(&mut app, &ctx);
    assert!(!app.session.active().ok_or_else(|| error("missing saved workbook"))?.is_dirty());

    choose(&mut app)?;
    let read = call(&api, "requestOpen", &Array::of1(&"recovered".into()))?;
    let fault = request_failure(publisher.as_ref().unchecked_ref())?;
    assert_eq!(Reflect::get(&fault, &"calls".into())?.as_f64(), Some(2.0));
    assert_eq!(Reflect::get(&fault, &"restored".into())?.as_bool(), Some(true));
    let failures = events.matching("openFailed")?;
    assert_eq!(failures.len(), 2);
    assert_eq!(Reflect::get(&failures[1], &"requestId".into())?, read);
    assert_eq!(field(&failures[1], "resourceId")?, "recovered");
    let dialog_failures = events.matching("saveFailed")?;
    assert_eq!(dialog_failures.len(), 1);
    let dialog = field(&dialog_failures[0], "requestId")?;
    let rejected = Reflect::get(&fault, &"requests".into())?.dyn_into::<Array>()?;
    assert_eq!(rejected.length(), 2);
    assert_eq!(field(&rejected.get(0), "requestId")?, dialog);
    assert_eq!(Reflect::get(&rejected.get(1), &"requestId".into())?, read);
    assert!(call(&api, "resolveSaveDialog", &Array::of3(&dialog.into(), &"provider".into(), &"Recovery.xlsm".into())).is_err());
    assert!(call(&api, "cancelOpen", &Array::of2(&read, &"late".into())).is_err());
    assert!(!host.busy());
    assert!(events.blocked_disconnects.borrow().len() >= 3);
    assert!(
        events.blocked_disconnects.borrow().iter().all(|blocked| *blocked),
        "the actual last terminal's in-flight batch must block account replacement"
    );
    choose(&mut app)?;
    publish(&host, &ctx);
    let dialogs = events.matching("saveDialog")?;
    assert_eq!(dialogs.len(), 3);
    let final_choice = field(&dialogs[2], "requestId")?;
    call(&api, "resolveSaveDialog", &Array::of3(&final_choice.into(), &"cancel".into(), &"".into()))?;
    let final_read = call(&api, "requestOpen", &Array::of1(&"recovered".into()))?;
    publish(&host, &ctx);
    call(&api, "cancelOpen", &Array::of2(&final_read, &"cancelled".into()))?;
    publish(&host, &ctx);
    assert!(!host.busy());
    assert_eq!(call(&api, "unregisterProvider", &Array::new())?.as_bool(), Some(true));
    Ok(())
}
