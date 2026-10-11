//! The browser shell: web `Services`, drag-and-drop, and the eframe web runner.

use gridcraft_engine::{Prefs, Session};
use gridcraft_ui_egui::{Inbox, Services, SheetApp};
use wasm_bindgen::JsCast as _;

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
        if query().contains("webgl")
            && let eframe::egui_wgpu::WgpuSetup::CreateNew(create) = &mut options.wgpu_options.wgpu_setup
        {
            create.instance_descriptor.backends = eframe::wgpu::Backends::GL;
        }
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(move |cc| {
                    let inbox: Inbox = Inbox::default();
                    let mut session = Session::new();
                    // Saved preferences first, then the browser language ("system" prefs follow it).
                    if let Some(prefs) = load_json::<Prefs>(PREFS_KEY) {
                        session.prefs = prefs;
                    }
                    session.set_system_locale(&browser_locale());
                    let q = query();
                    if let Some(name) = q.split(['?', '&']).find_map(|kv| kv.strip_prefix("sample=")) {
                        let _ = session.execute("file.new", serde_json::json!({"sample": name}));
                    } else if q.contains("sample") {
                        let _ = session.execute("file.new", serde_json::json!({"sample": "sales"}));
                    }
                    let mut app = SheetApp::new(session, services(inbox.clone(), cc.egui_ctx.clone()));
                    if let Some(ui) = load_json::<gridcraft_ui_egui::UiState>(UI_KEY) {
                        app.ui = ui;
                    }
                    SheetApp::setup_context(&cc.egui_ctx, app.ui.dark);
                    Ok(Box::new(WebShell::new(app, inbox)))
                }),
            )
            .await;
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

/// `localStorage` keys of the saved preferences (engine preferences and interface state), JSON.
const PREFS_KEY: &str = "gridcraft.prefs";
const UI_KEY: &str = "gridcraft.ui";

/// Seconds between two checks for changed preferences.
const SAVE_INTERVAL: f64 = 0.25;

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// The browser's preferred language (`navigator.language`, e.g. `pt-BR`); en-US when unavailable.
fn browser_locale() -> String {
    web_sys::window().and_then(|w| w.navigator().language()).unwrap_or_else(|| "en-US".to_string())
}

fn load_json<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    let text = storage()?.get_item(key).ok().flatten()?;
    serde_json::from_str(&text).ok()
}

struct WebShell {
    app: SheetApp,
    inbox: Inbox,
    /// Last saved JSON of the engine preferences and the interface state.
    saved_prefs: String,
    saved_ui: String,
    /// Egui time of the next check for changed preferences.
    next_save_check: f64,
}

impl WebShell {
    fn new(app: SheetApp, inbox: Inbox) -> WebShell {
        let saved_prefs = serde_json::to_string(&app.session.prefs).unwrap_or_default();
        let saved_ui = serde_json::to_string(&app.ui).unwrap_or_default();
        WebShell { app, inbox, saved_prefs, saved_ui, next_save_check: 0.0 }
    }

    /// Writes the preferences to `localStorage` when they changed, at most every [`SAVE_INTERVAL`].
    fn save_prefs(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now < self.next_save_check {
            return;
        }
        self.next_save_check = now + SAVE_INTERVAL;
        let Some(storage) = storage() else { return };
        for (key, current, saved) in [
            (PREFS_KEY, serde_json::to_string(&self.app.session.prefs), &mut self.saved_prefs),
            (UI_KEY, serde_json::to_string(&self.app.ui), &mut self.saved_ui),
        ] {
            if let Ok(text) = current
                && text != *saved
                && storage.set_item(key, &text).is_ok()
            {
                *saved = text;
            }
        }
    }
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
        self.save_prefs(ctx);
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        // Preferences only change in response to input. After a frame with input, one more frame
        // once the throttle has elapsed compares and saves what this frame changed, even when
        // nothing else repaints; idle frames schedule nothing, so the canvas can stay idle.
        if ui.ctx().input(|i| !i.events.is_empty() || i.pointer.any_down()) {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SAVE_INTERVAL));
        }
    }
}

fn services(inbox: Inbox, ctx: egui::Context) -> Services {
    let open_inbox = inbox.clone();
    let font_inbox = Inbox::default();
    let fetch_inbox = font_inbox.clone();
    let fetch_ctx = ctx.clone();
    let picture_ctx = ctx.clone();
    Services {
        copy_html: Some(Box::new(copy_html)),
        open_async: Some(Box::new(move || {
            let inbox = open_inbox.clone();
            let ctx = ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .add_filter("Spreadsheets", &["xlsx", "xlsm", "xlsb", "ods", "csv", "tsv", "txt", "json"])
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
        open_url: Some(Box::new(|url: &str| {
            if let Some(w) = web_sys::window() {
                let _ = w.open_with_url_and_target(url, "_blank");
            }
        })),
        inbox: Some(inbox),
        // CJK text and CJK interface languages need a font that is not part of the bundle: the page
        // serves it from `./fonts/<name>` (see packaging/web/README.md).
        fetch_font: Some(Box::new(move |name: &str| {
            let name = name.to_string();
            let inbox = fetch_inbox.clone();
            let ctx = fetch_ctx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                match fetch_bytes(&format!("./fonts/{name}")).await {
                    Ok(bytes) => {
                        inbox.lock().unwrap_or_else(|e| e.into_inner()).push((name, bytes));
                        ctx.request_repaint();
                    }
                    Err(e) => log::warn!("font ./fonts/{name} is not available ({e}); CJK text shows as boxes without it"),
                }
            });
        })),
        font_inbox: Some(font_inbox),
        ..Default::default()
    }
}

/// Downloads `url` (relative to the page) and returns the response body.
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url)).await.map_err(js)?;
    let response: web_sys::Response = response.dyn_into().map_err(|_| "not a response")?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    let buffer = wasm_bindgen_futures::JsFuture::from(response.array_buffer().map_err(js)?).await.map_err(js)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
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

/// Triggers a browser download of `bytes`.
fn download(name: &str, bytes: &[u8]) -> Result<(), String> {
    let js = |e: wasm_bindgen::JsValue| format!("{e:?}");
    let window = web_sys::window().ok_or("no window")?;
    let document = window.document().ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(if name.ends_with(".csv") { "text/csv" } else { "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" });
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).map_err(js)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js)?;
    let a: web_sys::HtmlAnchorElement = document.create_element("a").map_err(js)?.dyn_into().map_err(|_| "not an anchor")?;
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
