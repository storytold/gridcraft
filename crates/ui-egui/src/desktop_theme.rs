//! The desktop's light/dark choice and text size, for interface settings that follow it.
//!
//! winit reports no system theme on Linux — `ActiveEventLoop::system_theme` is `None` on every
//! desktop there — so "Use system setting" has to ask the desktop itself. The XDG Desktop Portal's
//! Settings interface answers on GNOME, KDE and XFCE alike, over X11 and Wayland alike.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

/// How often [`DesktopTheme::start`] re-reads the desktop's answer. The portal also publishes a
/// change signal, but subscribing needs a connection kept alive for the life of every window; one
/// round trip every few seconds is cheap and reacts soon enough after a theme switch.
const POLL: Duration = Duration::from_secs(4);

/// How long one read waits for the portal's reply. Without it a portal that never answers blocks
/// the watcher for good, and the theme stops following the desktop. Shorter than [`POLL`]: a read
/// that times out drops the connection, and the next poll asks again on a fresh one.
#[cfg(target_os = "linux")]
const REPLY: Duration = Duration::from_secs(2);

/// Whether the desktop asks applications to draw dark. `None` when it says nothing, and when
/// there is no session bus or no portal to ask (a bare X server, a container), so the caller
/// keeps whatever it chose itself.
pub fn prefers_dark() -> Option<bool> {
    read(&mut Link::default())
}

/// Independently available desktop preferences. Missing or malformed answers leave the app's
/// last usable value in place; text size defaults to 1.0 before the first usable answer.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Preferences {
    pub dark: Option<bool>,
    pub text_scale: Option<f32>,
}

/// Watches the desktop's appearance and text size while the app runs.
#[derive(Default)]
pub struct DesktopTheme {
    /// `None` when no watcher thread could be started; the one-shot read still works.
    changes: Option<Receiver<Preferences>>,
    /// Dropping the watcher stops polling and releases the repaint context.
    _stop: Option<Sender<()>>,
}

impl DesktopTheme {
    /// Start watching. Nothing runs before this is called, and calling it once is enough. The
    /// first answer is sent straight away, so the caller does not have to ask for it separately.
    pub fn start(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        let (stop, cancelled) = channel::<()>();
        let started = std::thread::Builder::new().name("gridcraft-desktop-theme".into()).spawn(move || {
            let mut link = Link::default();
            let mut last = None;
            loop {
                let now = Preferences { dark: read(&mut link), text_scale: read_text_scale(&mut link) };
                if last != Some(now) {
                    last = Some(now);
                    if tx.send(now).is_err() {
                        return;
                    }
                    // A desktop change must also reach an idle window, without mouse input.
                    ctx.request_repaint();
                }
                if cancelled.recv_timeout(POLL) != Err(RecvTimeoutError::Timeout) {
                    return;
                }
            }
        });
        if started.is_ok() { Self { changes: Some(rx), _stop: Some(stop) } } else { Self::default() }
    }

    /// The desktop's new answer if it changed since the last call, to be polled once a frame.
    /// Several changes in one frame collapse to the newest usable value of each field, and a
    /// desktop that stops answering leaves the last answer in place.
    pub fn take_change(&mut self) -> Option<Preferences> {
        self.changes
            .as_ref()?
            .try_iter()
            .reduce(|old, new| Preferences { dark: new.dark.or(old.dark), text_scale: new.text_scale.or(old.text_scale) })
    }
}

/// The session-bus connection the watcher keeps between reads, opened on first use. Off Linux
/// there is no bus to keep, and a unit struct rather than `()` keeps `let mut link = …` from
/// reading as a unit binding, which `cargo xtask ci` refuses on those platforms.
#[cfg(target_os = "linux")]
type Link = Option<zbus::blocking::Connection>;
#[cfg(not(target_os = "linux"))]
#[derive(Default)]
pub(crate) struct NoLink;
#[cfg(not(target_os = "linux"))]
type Link = NoLink;

/// Ask the portal once, over `link`'s connection (opened if absent, dropped if the bus went
/// away). D-Bus calls block, so this belongs off the UI thread: the watcher's thread is its
/// only repeated caller.
#[cfg(target_os = "linux")]
fn read(link: &mut Link) -> Option<bool> {
    const SCHEMAS: [&str; 2] = ["org.gnome.desktop.interface", "org.freedesktop.interface.color-scheme"];
    for schema in SCHEMAS {
        let Some(answer) = read_setting(link, schema, "color-scheme") else { continue };
        if let Ok(scheme) = String::try_from(&*answer) {
            return parse(&scheme);
        }
    }
    None
}

/// A bounded portal read on the watcher's shared connection; never called by the frame loop.
#[cfg(target_os = "linux")]
fn read_setting(link: &mut Link, schema: &str, key: &str) -> Option<zbus::zvariant::OwnedValue> {
    if link.is_none() {
        *link = Some(zbus::blocking::connection::Builder::session().ok()?.method_timeout(REPLY).build().ok()?);
    }
    let reply = match link.as_ref()?.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.Settings"),
        "ReadOne",
        &(schema, key),
    ) {
        Ok(reply) => reply,
        Err(zbus::Error::InputOutput(_)) => {
            *link = None;
            return None;
        }
        Err(_) => return None,
    };
    reply.body().deserialize().ok()
}

/// GNOME-compatible portals expose the desktop's text scale as a double, separately from DPI.
#[cfg(target_os = "linux")]
fn read_text_scale(link: &mut Link) -> Option<f32> {
    let answer = read_setting(link, "org.gnome.desktop.interface", "text-scaling-factor")?;
    parse_text_scale(f64::try_from(&*answer).ok()?)
}

/// Keep Auto within the same supported range as a manual interface size, without rounding away
/// fractional desktop settings such as 1.3636. Nonpositive and nonfinite values are not settings.
#[cfg(target_os = "linux")]
fn parse_text_scale(scale: f64) -> Option<f32> {
    (scale.is_finite() && scale > 0.0)
        .then(|| scale.clamp(crate::INTERFACE_SCALE_MIN as f64 / 100.0, crate::INTERFACE_SCALE_MAX as f64 / 100.0) as f32)
}

/// The portal's `color-scheme` answer. `default` means the desktop states no preference, so
/// neither does the application.
#[cfg(target_os = "linux")]
fn parse(scheme: &str) -> Option<bool> {
    match scheme {
        "prefer-dark" => Some(true),
        "prefer-light" => Some(false),
        _ => None,
    }
}

/// macOS and Windows answer through winit, which the caller already has.
#[cfg(not(target_os = "linux"))]
fn read(_: &mut Link) -> Option<bool> {
    None
}

/// Other platforms keep their existing display-scale behavior.
#[cfg(not(target_os = "linux"))]
fn read_text_scale(_: &mut Link) -> Option<f32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The portal answers with one of three strings; only the two preferences are a choice.
    #[test]
    #[cfg(target_os = "linux")]
    fn the_portal_answer_reads_as_a_choice_or_as_none() {
        assert_eq!(parse("prefer-dark"), Some(true));
        assert_eq!(parse("prefer-light"), Some(false));
        assert_eq!(parse("default"), None);
        assert_eq!(parse(""), None);
    }

    /// Off Linux the caller already has winit's answer, so this adds nothing.
    #[test]
    #[cfg(not(target_os = "linux"))]
    fn other_platforms_answer_through_winit() {
        assert_eq!(prefers_dark(), None);
    }

    /// A watcher that was never started is inert rather than a panic.
    #[test]
    fn a_watcher_without_a_thread_is_inert() {
        assert_eq!(DesktopTheme::default().take_change(), None);
    }

    /// Fractional desktop sizes survive; invalid values are ignored and extremes stay usable.
    #[test]
    #[cfg(target_os = "linux")]
    fn desktop_text_scale_is_finite_positive_and_bounded() {
        assert_eq!(parse_text_scale(1.3636), Some(1.3636));
        assert_eq!(parse_text_scale(1.0), Some(1.0));
        assert_eq!(parse_text_scale(0.1), Some(0.75));
        assert_eq!(parse_text_scale(f64::MAX), Some(2.0));
        for invalid in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(parse_text_scale(invalid), None);
        }
    }

    /// A failed field in the newest snapshot must not discard another pending usable answer.
    #[test]
    fn pending_preferences_merge_independently() {
        let (tx, rx) = channel();
        let mut watcher = DesktopTheme { changes: Some(rx), ..Default::default() };
        tx.send(Preferences { dark: Some(true), text_scale: Some(1.25) }).unwrap();
        tx.send(Preferences { dark: Some(false), text_scale: None }).unwrap();
        assert_eq!(watcher.take_change(), Some(Preferences { dark: Some(false), text_scale: Some(1.25) }));
        assert_eq!(watcher.take_change(), None);
    }

    /// Auto follows live desktop sizes on top of display DPI; explicit sizes and sheet zoom do
    /// not change, and a missing portal answer keeps the last usable desktop size.
    #[test]
    fn auto_follows_desktop_text_size_without_overriding_manual_size() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session.execute("view.zoom", serde_json::json!({"percent": 125})).unwrap();
        let (tx, rx) = channel();
        let mut app = crate::SheetApp::new(session, crate::Services::default());
        app.use_prepared_fonts();
        app.desktop_theme = DesktopTheme { changes: Some(rx), ..Default::default() };
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1200.0, 800.0))
            .with_pixels_per_point(1.5)
            .with_context_setup(|ctx| {
                ctx.set_fonts(crate::theme::pinned_font_definitions());
                crate::theme::apply(ctx, false);
            })
            .build_ui_state(
                |ui, app: &mut crate::SheetApp| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                },
                app,
            );
        h.run_steps(2);
        assert_eq!(h.ctx.zoom_factor(), 1.0, "no portal answer keeps the old default");
        tx.send(Preferences { text_scale: Some(1.3636), ..Default::default() }).unwrap();
        h.run_steps(2);
        assert_eq!(h.ctx.zoom_factor(), 1.3636);
        assert!((h.ctx.pixels_per_point() - 1.5 * 1.3636).abs() < 0.001);
        let saved = serde_json::to_value(&h.state().ui).unwrap();
        assert_eq!(saved["interfaceScale"], 100, "Auto remains Auto when saved");

        h.state_mut().run("view.interfaceScale", serde_json::json!({"percent": 125})).unwrap();
        tx.send(Preferences { text_scale: Some(1.5), ..Default::default() }).unwrap();
        h.run_steps(2);
        assert_eq!(h.ctx.zoom_factor(), 1.25, "manual sizing overrides desktop text size");
        h.state_mut().run("view.interfaceScale", serde_json::json!({"percent": "auto"})).unwrap();
        h.run_steps(2);
        assert_eq!(h.ctx.zoom_factor(), 1.5, "returning to Auto uses the latest desktop size");
        tx.send(Preferences::default()).unwrap();
        h.run_steps(2);
        assert_eq!(h.ctx.zoom_factor(), 1.5, "an unavailable portal keeps the last usable size");
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().zoom, 125, "interface size does not edit sheet zoom");
    }

    #[cfg(target_os = "linux")]
    struct Settings(std::sync::Arc<std::sync::Mutex<Option<f64>>>);

    #[cfg(target_os = "linux")]
    #[zbus::interface(name = "org.freedesktop.portal.Settings")]
    impl Settings {
        /// Supply only the text-size key: an unavailable theme must not block text-size reads.
        fn read_one(&self, namespace: &str, key: &str) -> zbus::fdo::Result<zbus::zvariant::OwnedValue> {
            if namespace != "org.gnome.desktop.interface" || key != "text-scaling-factor" {
                return Err(zbus::fdo::Error::InvalidArgs("unknown setting".into()));
            }
            let scale = self.0.lock().map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
            match *scale {
                Some(scale) => Ok(scale.into()),
                None => Ok(true.into()), // Deliberately wrong D-Bus type.
            }
        }
    }

    /// Exercise real D-Bus serialization, connection reuse, live polling and idle-window wakeups
    /// without ever changing a desktop setting. Run only under `dbus-run-session`.
    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "requires a private D-Bus session; run with dbus-run-session"]
    fn desktop_text_scale_portal_round_trip_and_live_changes() {
        use std::sync::{Arc, Mutex};
        let scale = Arc::new(Mutex::new(Some(1.3636)));
        let _server = zbus::blocking::connection::Builder::session()
            .unwrap()
            .name("org.freedesktop.portal.Desktop")
            .unwrap()
            .serve_at("/org/freedesktop/portal/desktop", Settings(scale.clone()))
            .unwrap()
            .build()
            .unwrap();
        let mut link = Link::default();
        assert_eq!(read_text_scale(&mut link), Some(1.3636));
        let name = link.as_ref().unwrap().unique_name().cloned();
        assert_eq!(read_text_scale(&mut link), Some(1.3636));
        assert_eq!(link.as_ref().unwrap().unique_name().cloned(), name);

        let ctx = egui::Context::default();
        let (wake_tx, wake_rx) = channel();
        ctx.set_request_repaint_callback(move |_| {
            let _ = wake_tx.send(());
        });
        let mut watcher = DesktopTheme::start(ctx.clone());
        let rx = watcher.changes.as_ref().unwrap();
        assert_eq!(rx.recv_timeout(REPLY * 2).unwrap().text_scale, Some(1.3636));
        wake_rx.recv_timeout(REPLY).unwrap();
        // Consume egui's outstanding repaint requests before testing the next idle wakeup.
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |_| {});
            output.textures_delta.clear(); // no renderer consumes uploads in this test
        }
        while wake_rx.try_recv().is_ok() {}
        *scale.lock().unwrap() = Some(1.5);
        assert_eq!(rx.recv_timeout(POLL + REPLY * 2).unwrap().text_scale, Some(1.5));
        wake_rx.recv_timeout(REPLY).unwrap();
        *scale.lock().unwrap() = None;
        assert_eq!(rx.recv_timeout(POLL + REPLY * 2).unwrap().text_scale, None);
        *scale.lock().unwrap() = Some(1.25);
        assert_eq!(rx.recv_timeout(POLL + REPLY * 2).unwrap().text_scale, Some(1.25));
        // Once all handles are dropped, the worker must release its context without another
        // desktop change (the callback owns the last sender).
        drop(watcher._stop.take());
        drop(ctx);
        assert_eq!(wake_rx.recv_timeout(REPLY), Err(RecvTimeoutError::Disconnected));
    }
}
