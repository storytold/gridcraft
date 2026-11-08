//! Same-origin file-provider bridge. Persistence belongs to the registered provider, not WASM.
//!
//! Trusted page scripts opt in; the host authenticates and authorizes storage. Callbacks enqueue
//! commands for a later egui frame. Exporting bytes never acknowledges them. Startup retains its
//! callbacks only while the shell owns the installation; a failed ready dispatch is retryable.

use std::any::Any;

use gridcraft_ui_egui::{
    Services,
    file_providers::{Host, Resource},
};
use js_sys::{Array, Function, Object, Reflect, Uint8Array};
use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};

const API: &str = "gridcraftFileProviders";
const EVENT: &str = "gridcraft-file-provider";
const MAX_BYTES: u32 = 80 * 1024 * 1024;

#[wasm_bindgen]
extern "C" {
    // Keep recoverable JS allocation/copy errors inside the actual publisher's Result path.
    #[wasm_bindgen(catch, js_namespace = Uint8Array, js_name = from)]
    fn copy_event_bytes(bytes: &[u8]) -> Result<Uint8Array, JsValue>;
}

fn error(message: impl ToString) -> JsValue {
    JsValue::from_str(&message.to_string())
}

fn object(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(&value.to_string())
}

fn set(target: &JsValue, key: &str, value: &JsValue) -> Result<(), JsValue> {
    if Reflect::set(target, &key.into(), value)? { Ok(()) } else { Err(error("file-provider API is not writable")) }
}

// Inspect UTF-16 length before copying a host string into WASM memory.
fn bounded_string(value: JsValue, max_units: u32) -> Result<String, JsValue> {
    let text = value.dyn_into::<js_sys::JsString>().map_err(|_| error("resource fields must be strings"))?;
    if text.length() > max_units {
        return Err(error("resource field is too large"));
    }
    text.as_string().ok_or_else(|| error("invalid resource field"))
}

fn resource(value: JsValue) -> Result<Resource, JsValue> {
    if value.is_null() || !value.is_object() || Array::is_array(&value) {
        return Err(error("resource entries must be objects"));
    }
    let entry = value.dyn_into::<Object>()?;
    let id = bounded_string(Reflect::get(&entry, &"id".into())?, 160)?;
    let name = bounded_string(Reflect::get(&entry, &"name".into())?, 255)?;
    let updated = Reflect::get(&entry, &"updatedAt".into())?;
    let updated_at = if updated.is_undefined() { String::new() } else { bounded_string(updated, 100)? };
    Ok(Resource { id, name, updated_at })
}

fn wake_later(ctx: &egui::Context) {
    let ctx = ctx.clone();
    wasm_bindgen_futures::spawn_local(async move { ctx.request_repaint() });
}

/// Owns the page-local API and its Rust callbacks. Failed startup drops the same owner, so no
/// forgotten closure or initialized Host is reused by a later installation attempt.
pub struct Installation {
    window: web_sys::Window,
    api: Object,
    host: Host,
    _callbacks: Vec<Box<dyn Any>>,
}

impl Drop for Installation {
    fn drop(&mut self) {
        let key = JsValue::from_str(API);
        if let Ok(current) = Reflect::get(&self.window, &key)
            && Object::is(&current, self.api.as_ref())
            && !matches!(Reflect::delete_property(self.window.unchecked_ref::<Object>(), &key), Ok(true))
        {
            log::warn!("could not remove the retired file-provider API");
        }
    }
}

/// Attach only a complete installation. Existing host modes own Save and do not install this
/// independent capability. Failure leaves the normal browser services unchanged.
pub fn install_for_services(services: &mut Services, ctx: egui::Context, host_connected: bool) -> Result<Option<Installation>, JsValue> {
    if host_connected {
        return Ok(None);
    }
    if services.file_providers.is_some() {
        return Err(error("file provider services already exist"));
    }
    let installation = install(ctx)?;
    services.file_providers = Some(installation.host.clone());
    Ok(Some(installation))
}

/// Makes the capability discoverable without enabling it or changing the browser's writer.
fn install(ctx: egui::Context) -> Result<Installation, JsValue> {
    let window = web_sys::window().ok_or_else(|| error("no window"))?;
    if Reflect::has(&window, &API.into())? {
        return Err(error("file-provider API already exists"));
    }
    // A failed attempt never exposes this fresh Host to services. A delayed result from an
    // earlier attempt must never address a later installation's request.
    let host = Host::default();
    let crypto = Reflect::get(&window, &"crypto".into())?;
    let random = Reflect::get(&crypto, &"getRandomValues".into())?.dyn_into::<Function>()?;
    let nonce = Uint8Array::new_with_length(16);
    random.call1(&crypto, &nonce)?;
    host.set_session(nonce.to_vec().iter().map(|byte| format!("{byte:02x}")).collect()).map_err(error)?;

    let api = Object::new();
    set(&api, "protocolVersion", &1.into())?;
    set(&api, "app", &"gridcraft".into())?;
    set(&api, "capabilities", &object(&serde_json::json!(["revisionAck", "externalRecents", "saveDestination", "openBytes"]))?)?;
    let mut callbacks: Vec<Box<dyn Any>> = Vec::new();

    let h = host.clone();
    let wake = ctx.clone();
    let register = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<String, JsValue>>::new(move |id, label, namespace| {
        let id = bounded_string(id, 160)?;
        let label = bounded_string(label, 100)?;
        let namespace = bounded_string(namespace, 160)?;
        let connection = h.connect(id, label, namespace).map_err(error)?;
        wake_later(&wake);
        Ok(connection)
    });
    set(&api, "registerProvider", register.as_ref())?;
    callbacks.push(Box::new(register));

    let h = host.clone();
    let wake = ctx.clone();
    let unregister = Closure::<dyn FnMut() -> Result<bool, JsValue>>::new(move || {
        h.disconnect().map_err(error)?;
        wake_later(&wake);
        Ok(true)
    });
    set(&api, "unregisterProvider", unregister.as_ref())?;
    callbacks.push(Box::new(unregister));

    let h = host.clone();
    let wake = ctx.clone();
    let enabled = Closure::<dyn FnMut(JsValue) -> Result<bool, JsValue>>::new(move |value: JsValue| {
        let value = value.as_bool().ok_or_else(|| error("enabled must be a boolean"))?;
        h.set_enabled(value).map_err(error)?;
        wake_later(&wake);
        Ok(true)
    });
    set(&api, "setEnabled", enabled.as_ref())?;
    callbacks.push(Box::new(enabled));

    let h = host.clone();
    let wake = ctx.clone();
    let resources = Closure::<dyn FnMut(JsValue, JsValue) -> Result<(), JsValue>>::new(move |connection, value: JsValue| {
        let connection = bounded_string(connection, 160)?;
        if !Array::is_array(&value) {
            return Err(error("resources must be an array of at most 100 entries"));
        }
        let entries = value.dyn_into::<Array>()?;
        if entries.length() > 100 {
            return Err(error("resources must be an array of at most 100 entries"));
        }
        // Read only bounded metadata; do not copy or serialize arbitrary host properties.
        let resources = entries.iter().map(resource).collect::<Result<Vec<_>, _>>()?;
        h.set_resources(&connection, resources).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "setResources", resources.as_ref())?;
    callbacks.push(Box::new(resources));

    let h = host.clone();
    let wake = ctx.clone();
    let request_open = Closure::<dyn FnMut(JsValue) -> Result<String, JsValue>>::new(move |resource| {
        let resource = bounded_string(resource, 160)?;
        let request = h.request_open(&resource).map_err(error)?;
        wake_later(&wake);
        Ok(request)
    });
    set(&api, "requestOpen", request_open.as_ref())?;
    callbacks.push(Box::new(request_open));

    let h = host.clone();
    let wake = ctx.clone();
    let open = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, name, value: JsValue| {
        let request = bounded_string(request, 160)?;
        let name = bounded_string(name, 255)?;
        let bytes = value.dyn_into::<Uint8Array>().map_err(|_| error("bytes must be a Uint8Array"))?;
        if bytes.length() > MAX_BYTES {
            return Err(error("file exceeds the provider limit"));
        }
        h.open(request, name, bytes.to_vec()).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "openResource", open.as_ref())?;
    callbacks.push(Box::new(open));

    let h = host.clone();
    let wake = ctx.clone();
    let cancel_open = Closure::<dyn FnMut(JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, message| {
        let request = bounded_string(request, 160)?;
        let message = bounded_string(message, 1024)?;
        h.cancel_open(request, message).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "cancelOpen", cancel_open.as_ref())?;
    callbacks.push(Box::new(cancel_open));

    let h = host.clone();
    let wake = ctx.clone();
    let result = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, status, resource| {
        let request = bounded_string(request, 160)?;
        let status = bounded_string(status, 8)?;
        let resource = bounded_string(resource, 160)?;
        h.result(request, status, resource).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "saveResult", result.as_ref())?;
    callbacks.push(Box::new(result));

    let h = host.clone();
    let wake = ctx.clone();
    let dialog = Closure::<dyn FnMut(JsValue, JsValue, JsValue) -> Result<(), JsValue>>::new(move |request, destination, name| {
        let request = bounded_string(request, 160)?;
        let destination = bounded_string(destination, 8)?;
        let name = bounded_string(name, 255)?;
        h.resolve_dialog(&request, &destination, &name).map_err(error)?;
        wake_later(&wake);
        Ok(())
    });
    set(&api, "resolveSaveDialog", dialog.as_ref())?;
    callbacks.push(Box::new(dialog));

    let h = host.clone();
    let snapshot = Closure::<dyn FnMut() -> Result<JsValue, JsValue>>::new(move || object(&h.snapshot()));
    set(&api, "snapshot", snapshot.as_ref())?;
    callbacks.push(Box::new(snapshot));
    let installation = Installation { window, api: Object::freeze(&api), host, _callbacks: callbacks };
    // Publish before ready so synchronous listeners can register and enqueue work. The owner
    // rolls back only this exact API if dispatch fails, never a listener's replacement object.
    set(&installation.window, API, installation.api.as_ref())?;
    dispatch(&serde_json::json!({"type": "ready", "protocolVersion": 1}), None)?;
    if !Object::is(&Reflect::get(&installation.window, &API.into())?, installation.api.as_ref()) {
        return Err(error("file-provider API changed during startup"));
    }
    Ok(installation)
}

fn dispatch(metadata: &serde_json::Value, bytes: Option<Vec<u8>>) -> Result<(), JsValue> {
    let detail = object(metadata)?;
    if let Some(bytes) = bytes {
        set(&detail, "bytes", &copy_event_bytes(&bytes)?.into())?;
    }
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    let event = web_sys::CustomEvent::new_with_event_init_dict(EVENT, &init)?;
    web_sys::window().ok_or_else(|| error("no window"))?.dispatch_event(&event)?;
    Ok(())
}

pub fn publish(host: &Host, ctx: &egui::Context) {
    if host.publish_events_with(dispatch, |error| log::error!("file-provider event delivery failed: {error:?}")) {
        ctx.request_repaint();
    }
}

#[cfg(test)]
#[path = "file_provider_browser_tests.rs"]
mod browser_tests;
