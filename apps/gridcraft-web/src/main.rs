//! GridCraft in the browser.
//!
//! Runs the same [`gridcraft_ui_egui::SheetApp`] as the desktop app through eframe's web runner
//! (wgpu: WebGPU where available, WebGL2 otherwise). Build with `trunk build --release` from
//! this directory (output in `dist/web`).
//!
//! Differences from the desktop app: no TCP control channel (an embedding page drives it over
//! `postMessage` instead, see below); Open uses the browser file picker (bytes arrive
//! asynchronously through `Services::inbox`); Save downloads the file; dropped files are read
//! asynchronously.
//!
//! URL query flags: `?webgl` forces WebGL2; `?sample=sales|budget|grades` opens a sample;
//! `?host=parent` (inside an `<iframe>`) talks to a same-origin parent page instead: it sends the
//! workbook to open, stores what GridCraft saves and may drive the control channel (`host`,
//! `protocol`), and `&author=` names the user for notes and comments.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(target_arch = "wasm32")]
mod host;
// Plain JSON, so its tests run natively.
#[cfg(any(target_arch = "wasm32", test))]
mod protocol;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() {
    web::start();
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("gridcraft-web only runs in the browser: build it with `trunk build --release` in apps/gridcraft-web");
}
