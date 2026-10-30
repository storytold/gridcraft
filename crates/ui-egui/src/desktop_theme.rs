//! The desktop's own light/dark choice, for an interface setting that follows it.
//!
//! winit reports no system theme on Linux — `ActiveEventLoop::system_theme` is `None` on every
//! desktop there — so "Use system setting" has to ask the desktop itself. The XDG Desktop Portal's
//! Settings interface answers on GNOME, KDE and XFCE alike, over X11 and Wayland alike.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
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

/// Watches the desktop and reports its choice when it changes. [`prefers_dark`] covers the
/// answer at start-up; this covers the user switching theme while the app runs.
#[derive(Default)]
pub struct DesktopTheme {
    /// `None` when no watcher thread could be started; the one-shot read still works.
    changes: Option<Receiver<Option<bool>>>,
}

impl DesktopTheme {
    /// Start watching. Nothing runs before this is called, and calling it once is enough. The
    /// first answer is sent straight away, so the caller does not have to ask for it separately.
    pub fn start() -> Self {
        let (tx, rx) = channel();
        let started = std::thread::Builder::new().name("gridcraft-desktop-theme".into()).spawn(move || {
            #[allow(clippy::let_unit_value)] // `Link` is `()` off Linux
            let mut link = Link::default();
            let mut last = read(&mut link);
            if tx.send(last).is_err() {
                return;
            }
            loop {
                std::thread::sleep(POLL);
                let now = read(&mut link);
                if now != last {
                    last = now;
                    if tx.send(now).is_err() {
                        return;
                    }
                }
            }
        });
        Self { changes: started.ok().map(|_| rx) }
    }

    /// The desktop's new answer if it changed since the last call, to be polled once a frame.
    /// Several changes in one frame collapse to the newest, and a desktop that stops answering
    /// leaves the last answer in place.
    pub fn take_change(&mut self) -> Option<bool> {
        let Some(changes) = &self.changes else { return None };
        let mut newest = None;
        loop {
            match changes.try_recv() {
                Ok(changed) => newest = changed,
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return newest,
            }
        }
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
    const PORTAL: &str = "org.freedesktop.portal.Desktop";
    const SETTINGS: &str = "org.freedesktop.portal.Settings";
    const SCHEMAS: [(&str, &str); 2] = [("org.gnome.desktop.interface", "color-scheme"), ("org.freedesktop.interface.color-scheme", "color-scheme")];
    if link.is_none() {
        *link = Some(zbus::blocking::connection::Builder::session().ok()?.method_timeout(REPLY).build().ok()?);
    }
    let connection = link.as_ref()?;
    for (schema, key) in SCHEMAS {
        let reply = match connection.call_method(Some(PORTAL), "/org/freedesktop/portal/desktop", Some(SETTINGS), "ReadOne", &(schema, key)) {
            Ok(reply) => reply,
            Err(zbus::Error::InputOutput(_)) => {
                *link = None;
                return None;
            }
            Err(_) => continue,
        };
        let Ok((answer,)) = reply.body().deserialize::<(zbus::zvariant::OwnedValue,)>() else { continue };
        if let Ok(scheme) = String::try_from(&*answer) {
            return parse(&scheme);
        }
    }
    None
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
}
