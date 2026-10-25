//! The browser shell: web `Services`, drag-and-drop, and the eframe web runner.

use gridcraft_engine::Session;
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

const CANVAS_ID: &str = "gridcraft_canvas";
const LOADING_ID: &str = "gridcraft_loading";

pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Info).ok();
    wasm_bindgen_futures::spawn_local(async {
        let Some(document) = web_sys::window().and_then(|w| w.document()) else {
            log::error!("no document");
            return;
        };
        let Some(canvas) = document.get_element_by_id(CANVAS_ID).and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok()) else {
            log::error!("missing <canvas id=\"{CANVAS_ID}\">");
            return;
        };
        let mut options = eframe::WebOptions::default();
        if let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup {
            let default_descriptor = std::sync::Arc::clone(&setup.device_descriptor);

            setup.device_descriptor = std::sync::Arc::new(move |adapter| {
                let mut descriptor = default_descriptor(adapter);

                descriptor.required_limits = descriptor.required_limits.or_worse_values_from(&adapter.limits());

                descriptor
            });
        }
        if query().contains("webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        // `?host=parent`: a same-origin page framing GridCraft (such as a Nextcloud app) opens
        // workbooks in it and stores what it saves (`host`).
        let bridge = if query_param("host").as_deref() == Some("parent") { crate::host::Bridge::new() } else { None };
        let app_bridge = bridge.clone();
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| {
                    let inbox: Inbox = Inbox::default();
                    let mut session = Session::new();
                    // In host mode the parent sends the workbook to edit; no sample meanwhile.
                    let q = if app_bridge.is_some() { String::new() } else { query() };
                    if let Some(name) = q.split(['?', '&']).find_map(|kv| kv.strip_prefix("sample=")) {
                        let _ = session.execute("file.new", serde_json::json!({"sample": name}));
                    } else if q.contains("sample") {
                        let _ = session.execute("file.new", serde_json::json!({"sample": "sales"}));
                    }
                    let mut services = services(inbox.clone(), cc.egui_ctx.clone());
                    if let Some(bridge) = &app_bridge {
                        bridge.connect(&mut services, inbox.clone(), cc.egui_ctx.clone());
                    }
                    let mut app = SheetApp::new(session, services);
                    app.ui.system_theme = true;
                    // Follow the browser's language, except a CJK one: the web build has no system
                    // CJK fonts (none are bundled), so that interface would render as tofu.
                    let browser = gridcraft_ui_egui::i18n::Language::system();
                    if !matches!(
                        browser,
                        gridcraft_ui_egui::i18n::Language::Zh | gridcraft_ui_egui::i18n::Language::Ja | gridcraft_ui_egui::i18n::Language::Ko
                    ) {
                        app.ui.language = browser;
                    }
                    // Preferences from an earlier visit (#1562), as the desktop app reads `ui.json`
                    // and `prefs.json`; a saved language replaces the browser's.
                    let (stored_ui, stored_prefs) = (stored(crate::prefs::UI_KEY), stored(crate::prefs::PREFS_KEY));
                    crate::prefs::restore(&mut app, stored_ui.as_deref(), stored_prefs.as_deref());
                    let mut prefs = crate::prefs::PrefsSync::new(stored_ui, stored_prefs);
                    SheetApp::setup_context_for_language(&cc.egui_ctx, false, app.ui.language);
                    if let Some(bridge) = &app_bridge {
                        prefs.keep_user_name(app.session.prefs.user_name.clone());
                        if let Some(author) = query_param("author") {
                            crate::host::Bridge::set_author(&mut app, &author);
                        }
                        bridge.post_ready();
                    }
                    Ok(Box::new(WebShell { app, inbox, bridge: app_bridge, prefs }))
                }),
            )
            .await;
        if let (Err(e), Some(bridge)) = (&result, &bridge) {
            bridge.post_failed(&format!("{e:?}"));
        }
        if let Some(el) = document.get_element_by_id(LOADING_ID) {
            match result {
                Ok(()) => el.remove(),
                Err(e) => el.set_inner_html(&format!("<p>GridCraft failed to start: {e:?}</p><p>A browser with WebGPU or WebGL2 is required.</p>")),
            }
        }
    });
}

fn query() -> String {
    web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default()
}

fn query_param(key: &str) -> Option<String> {
    web_sys::UrlSearchParams::new_with_str(&query()).ok()?.get(key)
}

struct WebShell {
    app: SheetApp,
    inbox: Inbox,
    bridge: Option<crate::host::Bridge>,
    prefs: crate::prefs::PrefsSync,
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// A value stored on an earlier visit, if the browser allows storage.
fn stored(key: &str) -> Option<String> {
    local_storage()?.get_item(key).ok()?
}

impl eframe::App for WebShell {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        for f in dropped {
            let inbox = self.inbox.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let name = f.path().file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "dropped.xlsx".into());
                match f.bytes_async().await {
                    Ok(bytes) => {
                        inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
                        ctx.request_repaint();
                    }
                    Err(e) => log::error!("couldn't read dropped file {name}: {e}"),
                }
            });
        }
        self.app.logic(ctx);
        // Store changed preferences straight away: a reload or closed tab runs no exit code (#1562).
        let changed = self.prefs.changed(&self.app);
        if !changed.is_empty() {
            match local_storage() {
                Some(store) => {
                    for (key, json) in changed {
                        if let Err(e) = store.set_item(key, &json) {
                            log::warn!("preferences: cannot save {key}: {e:?}");
                        }
                    }
                }
                None => log::warn!("preferences: browser storage is unavailable"),
            }
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        if let Some(bridge) = &self.bridge {
            bridge.sync(&mut self.app);
        }
    }
}

fn services(inbox: Inbox, ctx: egui::Context) -> Services {
    let open_inbox = inbox.clone();
    let picture_ctx = ctx.clone();
    Services {
        copy_html: Some(Box::new(copy_html)),
        open_async: Some(Box::new(move || {
            let inbox = open_inbox.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .add_filter("Spreadsheets", &["xlsx", "xlsm", "xls", "xlsb", "ods", "csv", "tsv", "txt", "json"])
                    .pick_file()
                    .await
                else {
                    return;
                };
                let bytes = file.read().await;
                inbox.lock().unwrap_or_else(|e| e.into_inner()).push((file.file_name(), bytes));
                ctx.request_repaint();
            });
        })),
        pick_picture_async: Some(Box::new(move |inbox| {
            let ctx = picture_ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(file) = rfd::AsyncFileDialog::new().add_filter("Pictures (PNG, JPEG)", &["png", "jpg", "jpeg"]).pick_file().await else {
                    return;
                };
                let bytes = file.read().await;
                inbox.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((file.file_name(), bytes));
                ctx.request_repaint();
            });
        })),
        download: Some(Box::new(|name: &str, bytes: &[u8]| {
            if let Err(e) = download(name, bytes) {
                log::error!("download of {name} failed: {e}");
            }
        })),
        print: Some(Box::new(|bytes: &[u8]| {
            if let Err(e) = print_pdf(bytes) {
                log::error!("print failed: {e}");
            }
        })),
        open_url: Some(Box::new(|url: &str| {
            if let Some(w) = web_sys::window()
                && let gridcraft_engine::links::LinkKind::External(url) = gridcraft_engine::links::classify(url)
            {
                // With `noopener` the new browsing context can't hold a reference back to the
                // editor window and navigate it while the user reads the linked page.
                let _ = w.open_with_url_and_target(&url, "_blank,noopener");
            }
        })),
        inbox: Some(inbox),
        ..Default::default()
    }
}

/// Start inside the copy gesture; deferring `write` can lose browser user activation.
fn copy_html(html: &str, text: &str) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let clipboard = window.navigator().clipboard();
    if clipboard.is_undefined() || clipboard.is_null() {
        return Err("browser clipboard is unavailable (a secure context is required)".into());
    }
    if !js_sys::Reflect::get(&clipboard, &"write".into()).map_err(js)?.is_function() {
        return Err("browser does not support rich clipboard writes".into());
    }
    let data = js_sys::Object::new();
    for (mime, value) in [("text/html", html), ("text/plain", text)] {
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type(mime);
        let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(value));
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts).map_err(js)?;
        js_sys::Reflect::set(&data, &mime.into(), &blob).map_err(js)?;
    }
    let item = web_sys::ClipboardItem::new_with_record_from_str_to_blob_promise(&data).map_err(js)?;
    let promise = clipboard.write(&js_sys::Array::of1(&item));
    let text = text.to_owned();
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = wasm_bindgen_futures::JsFuture::from(promise).await {
            log::warn!("Rich clipboard write failed; trying plain text: {e:?}");
            if let Err(e) = wasm_bindgen_futures::JsFuture::from(clipboard.write_text(&text)).await {
                log::error!("Plain-text clipboard fallback also failed: {e:?}");
            }
        }
    });
    Ok(())
}

/// Opens `bytes` (a PDF) as an object URL in a hidden iframe and calls the iframe's own
/// `print()` once it has finished loading — the standard way to drive the browser's native print
/// dialog on a PDF without a download or a popup window the browser might block. The blob URL and
/// the iframe are both cleaned up once the print dialog closes, so repeated prints don't leak
/// them.
fn print_pdf(bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;

    let iframe: web_sys::HtmlIFrameElement = document.create_element("iframe").map_err(js)?.dyn_into().map_err(|_| "not an iframe")?;
    iframe.style().set_property("display", "none").map_err(js)?;
    let body = document.body().ok_or("no body")?;

    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/pdf");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;

    if let Err(e) = body.append_child(&iframe) {
        cleanup_print(&iframe, &url);
        return Err(js(e));
    }

    // `onload` fires once the PDF has actually rendered inside the iframe;
    // printing before that would show a blank page. Set the handler BEFORE
    // `src` so a fast/cached load can't fire before we're listening.
    let iframe_for_load = iframe.clone();
    let url_for_cleanup = url.clone();
    let on_load = wasm_bindgen::closure::Closure::once_into_js(move || {
        iframe_for_load.set_onload(None);
        if let Err(e) = print_loaded_pdf(&iframe_for_load, &url_for_cleanup) {
            cleanup_print(&iframe_for_load, &url_for_cleanup);
            log::error!("print failed: {e:?}");
        }
    });
    iframe.set_onload(Some(on_load.unchecked_ref()));
    iframe.set_src(&url);
    Ok(())
}

fn cleanup_print(iframe: &web_sys::HtmlIFrameElement, url: &str) {
    // Both operations are idempotent: print can fail after dispatching `afterprint`.
    web_sys::Url::revoke_object_url(url).ok();
    iframe.remove();
}

fn print_loaded_pdf(iframe: &web_sys::HtmlIFrameElement, url: &str) -> Result<(), JsValue> {
    let content_window = iframe.content_window().ok_or_else(|| JsValue::from_str("no print content window"))?;
    let iframe_for_cleanup = iframe.clone();
    let url_for_cleanup = url.to_owned();
    let on_after_print = wasm_bindgen::closure::Closure::once(move || {
        cleanup_print(&iframe_for_cleanup, &url_for_cleanup);
    });
    // Listen before `print()`, which can dispatch `afterprint` before it returns.
    // A one-shot listener also releases the callback when the dialog closes or is cancelled.
    let options = web_sys::AddEventListenerOptions::new();
    options.set_once(true);
    let callback = on_after_print.as_ref().unchecked_ref();
    content_window.add_event_listener_with_callback_and_add_event_listener_options("afterprint", callback, &options)?;
    if let Err(e) = content_window.print() {
        let _ = content_window.remove_event_listener_with_callback("afterprint", callback);
        return Err(e);
    }
    let _ = on_after_print.into_js_value();
    Ok(())
}

fn mime_for(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("xlsm") => "application/vnd.ms-excel.sheet.macroEnabled.12",
        Some("xlsb") => "application/vnd.ms-excel.sheet.binary.macroEnabled.12",
        Some("ods") => "application/vnd.oasis.opendocument.spreadsheet",
        Some("csv") => "text/csv",
        Some("tsv" | "txt") => "text/plain",
        Some("pdf") => "application/pdf",
        Some("html" | "htm") => "text/html",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        _ => "application/octet-stream",
    }
}

/// Trigger a browser save dialog or download of `bytes` named after `name`.
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;

    let show_picker_fn: Option<js_sys::Function> =
        js_sys::Reflect::get(&window, &JsValue::from_str("showSaveFilePicker")).ok().and_then(|v| v.dyn_into().ok());

    if let Some(picker_fn) = show_picker_fn {
        let ext = std::path::Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("xlsx").to_ascii_lowercase();
        let dot_ext = format!(".{ext}");
        let mime = mime_for(name);

        let accept_obj = js_sys::Object::new();
        let exts_arr = js_sys::Array::new();
        exts_arr.push(&JsValue::from_str(&dot_ext));
        let _ = js_sys::Reflect::set(&accept_obj, &JsValue::from_str(mime), &exts_arr);

        let type_desc = match ext.as_str() {
            "xlsx" => "Excel Workbook (*.xlsx)",
            "xlsm" => "Excel Macro-Enabled Workbook (*.xlsm)",
            "xlsb" => "Excel Binary Workbook (*.xlsb)",
            "ods" => "OpenDocument Spreadsheet (*.ods)",
            "csv" => "CSV (Comma delimited) (*.csv)",
            "tsv" => "Tab Delimited Text (*.tsv)",
            "txt" => "Plain Text (*.txt)",
            "pdf" => "PDF Document (*.pdf)",
            "html" | "htm" => "HTML Document (*.html)",
            "json" => "JSON Document (*.json)",
            "png" => "PNG Image (*.png)",
            "jpg" | "jpeg" => "JPEG Image (*.jpeg)",
            _ => "Spreadsheet",
        };

        let type_obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&type_obj, &JsValue::from_str("description"), &JsValue::from_str(type_desc));
        let _ = js_sys::Reflect::set(&type_obj, &JsValue::from_str("accept"), &accept_obj);

        let types_arr = js_sys::Array::new();
        types_arr.push(&type_obj);

        let picker_opts = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&picker_opts, &JsValue::from_str("suggestedName"), &JsValue::from_str(name));
        let _ = js_sys::Reflect::set(&picker_opts, &JsValue::from_str("types"), &types_arr);

        let prom_val = picker_fn.call1(&window, &picker_opts).map_err(js)?;
        let prom: js_sys::Promise = prom_val.dyn_into().map_err(|_| "not a promise".to_string())?;

        let bytes_vec = bytes.to_vec();
        let name_owned = name.to_string();
        // Anything but the user cancelling (no user gesture, a denied permission, a failed write)
        // falls back to an ordinary download, so a save never fails silently.
        let fallback = move |bytes: &[u8]| {
            if let Some(w) = web_sys::window()
                && let Err(e) = trigger_anchor_download(&w, &name_owned, bytes)
            {
                log::error!("download fallback failed: {e}");
            }
        };
        wasm_bindgen_futures::spawn_local(async move {
            match wasm_bindgen_futures::JsFuture::from(prom).await {
                Ok(handle_val) => {
                    if let Err(e) = write_file_handle(&handle_val, &bytes_vec).await {
                        log::error!("failed writing to file handle: {e}");
                        fallback(&bytes_vec);
                    } else {
                        log::info!("Successfully saved file through save file picker");
                    }
                }
                Err(e) => {
                    let err_name = js_sys::Reflect::get(&e, &JsValue::from_str("name")).ok().and_then(|v| v.as_string()).unwrap_or_default();
                    if err_name == "AbortError" {
                        log::info!("Save file dialog cancelled by user");
                    } else {
                        log::warn!("showSaveFilePicker refused ({err_name}); downloading instead");
                        fallback(&bytes_vec);
                    }
                }
            }
        });
        return Ok(());
    }

    // Fallback for browsers without showSaveFilePicker (e.g. Firefox, Safari):
    trigger_anchor_download(&window, name, bytes)
}

async fn write_file_handle(handle: &JsValue, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let create_fn: js_sys::Function = js_sys::Reflect::get(handle, &JsValue::from_str("createWritable"))
        .map_err(js)?
        .dyn_into()
        .map_err(|_| "createWritable is not a function".to_string())?;
    let stream_prom: js_sys::Promise = create_fn.call0(handle).map_err(js)?.dyn_into().map_err(|_| "not a promise".to_string())?;
    let stream = wasm_bindgen_futures::JsFuture::from(stream_prom).await.map_err(js)?;

    let write_fn: js_sys::Function =
        js_sys::Reflect::get(&stream, &JsValue::from_str("write")).map_err(js)?.dyn_into().map_err(|_| "write is not a function".to_string())?;
    let uint8 = js_sys::Uint8Array::from(bytes);
    let write_prom: js_sys::Promise = write_fn.call1(&stream, &uint8).map_err(js)?.dyn_into().map_err(|_| "not a promise".to_string())?;
    wasm_bindgen_futures::JsFuture::from(write_prom).await.map_err(js)?;

    let close_fn: js_sys::Function =
        js_sys::Reflect::get(&stream, &JsValue::from_str("close")).map_err(js)?.dyn_into().map_err(|_| "close is not a function".to_string())?;
    let close_prom: js_sys::Promise = close_fn.call0(&stream).map_err(js)?.dyn_into().map_err(|_| "not a promise".to_string())?;
    wasm_bindgen_futures::JsFuture::from(close_prom).await.map_err(js)?;
    Ok(())
}

fn trigger_anchor_download(window: &web_sys::Window, name: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let document = window.document().ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime_for(name));
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not an anchor".to_string())?;
    a.set_href(&url);
    a.set_download(name);
    a.style().set_property("display", "none").map_err(js)?;
    let body = document.body().ok_or("no body")?;
    body.append_child(&a).map_err(js)?;
    a.click();
    a.remove();
    let revoke = wasm_bindgen::closure::Closure::once_into_js(move || {
        web_sys::Url::revoke_object_url(&url).ok();
    });
    window.set_timeout_with_callback_and_timeout_and_arguments_0(revoke.unchecked_ref(), 10_000).map_err(js)?;
    Ok(())
}
