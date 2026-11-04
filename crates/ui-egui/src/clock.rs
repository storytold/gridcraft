/// Wall-clock milliseconds since the Unix epoch, for user-visible timestamps (Backstage's "last
/// changed", toasts). It can jump when the system clock is adjusted, so it must not drive
/// deadlines or pacing; use [`monotonic_ms`] for those.
pub fn now_ms() -> f64 {
    gridcraft_engine::core::date::unix_now_ms().unwrap_or(0.0)
}

/// Monotonic milliseconds since the first call, for screenshot deadlines and AutoSave pacing.
///
/// Deliberately separate from [`now_ms`]: a wall-clock adjustment must not extend (or expire) a
/// screenshot deadline or save a workbook twice. Native uses [`std::time::Instant`]; wasm uses the
/// browser's `performance.now()`, which is monotonic too.
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::disallowed_methods)] // the std clock is read only off wasm
pub fn monotonic_ms() -> f64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
}

#[cfg(target_arch = "wasm32")]
/// Web monotonic clock: the browser's `performance.now()`, or 0 when it is unavailable.
pub fn monotonic_ms() -> f64 {
    web_sys::window().and_then(|window| window.performance()).map_or(0.0, |performance| performance.now())
}
