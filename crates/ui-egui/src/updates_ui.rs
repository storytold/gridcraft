//! One background check at startup (and on Help ▸ Check for Updates…) and the modal prompt that
//! offers the new release: Update and Restart (direct self-update), Download or Later. The flow
//! itself is `craft_update::updater::Updater`; its dialogs run on the app's
//! [`DialogHost`](craft_ui::dialog::DialogHost).

use craft_ui::dialog::DialogResult;
use craft_update::stack_dialogs::{CheckChoice, UpdateCheckDialog};
use craft_update::updater::{Action, Status, Updater};

use crate::SheetApp;

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Runtime {
    updater: Updater,
    /// The check at start was asked for; `tick` starts it with the context.
    startup: bool,
    /// Help ▸ Check for Updates… was chosen; the next `tick` starts it.
    manual: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self { updater: Updater::new(&crate::UPDATE_APP, VERSION), startup: false, manual: false }
    }
}

/// Ask for a check; the next frame's `tick` starts it. A manual one reports every outcome.
pub(crate) fn start(app: &mut SheetApp, manual: bool) {
    if manual {
        app.updates_rt.manual = true;
    } else {
        app.updates_rt.startup = true;
    }
}

pub(crate) fn tick(app: &mut SheetApp, ctx: &egui::Context) {
    let rt = &mut app.updates_rt;
    if std::mem::take(&mut rt.manual) {
        rt.updater.check_now();
    } else if rt.startup {
        rt.updater.check_at_start();
    }
    rt.updater.tick(ctx);
}

/// Starts the helper of a ready update; it installs once the process has exited.
pub(crate) fn start_pending_update(app: &mut SheetApp) {
    app.updates_rt.updater.start_pending_update();
}

/// Whether the check result window shows: for every status but idle (a requested check, and the
/// first start after an update, which reports that it installed or why it failed), and never
/// over the startup prompt.
fn result_window_shows(status: &Status, prompt_open: bool) -> bool {
    !prompt_open && !matches!(status, Status::Idle)
}

/// One frame of the update flow, before the dialog host runs: answers the result dialog closed in
/// the last pass, drives the startup prompt, and keeps the result dialog open and current while the
/// updater has something to report.
pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    // The close for a ready update waits until no workbook holds unsaved work.
    let clean = !app.session.documents().iter().any(|d| d.is_dirty());
    // Answered before the dialog is kept open below, so a dismissed outcome does not open again at once.
    match app.framework_dialogs.take::<UpdateCheckDialog>() {
        Some(DialogResult::Ok(CheckChoice::CheckNow | CheckChoice::CheckAgain)) => app.updates_rt.manual = true,
        // The next `run` asks the window to close once nothing is unsaved.
        Some(DialogResult::Ok(CheckChoice::RestartToUpdate)) => app.updates_rt.updater.restart_to_update(),
        Some(DialogResult::Ok(CheckChoice::Update)) => {
            app.updates_rt.updater.request_install(clean);
        }
        Some(DialogResult::Ok(CheckChoice::Download)) => open_url(app, &crate::UPDATE_APP.releases_url()),
        // Closing the dialog dismisses the outcome it showed.
        Some(_) => app.updates_rt.updater.dismiss_status(),
        None => {}
    }
    match app.updates_rt.updater.run(&mut app.framework_dialogs, clean) {
        Action::OpenUrl(url) => open_url(app, &url),
        // Quit closes every workbook window. The install helper starts once the app exits.
        Action::Close => app.request_quit(ctx),
        _ => {}
    }
    let updater = &app.updates_rt.updater;
    let shows = result_window_shows(updater.status(), updater.prompt_open());
    let (status, installing, message) = (updater.status().clone(), updater.is_installing(), updater.status_message());
    let dialogs = &mut app.framework_dialogs;
    if shows && !dialogs.is_open::<UpdateCheckDialog>() {
        dialogs.open(UpdateCheckDialog::new(VERSION));
    } else if !shows && dialogs.is_open::<UpdateCheckDialog>() {
        dialogs.cancel::<UpdateCheckDialog>(ctx);
    }
    if let Some(dialog) = dialogs.get_mut::<UpdateCheckDialog>() {
        dialog.set_status(status);
        dialog.set_installing(installing);
        dialog.set_message(message);
    }
}

fn open_url(app: &SheetApp, url: &str) {
    if let Some(open) = &app.services.open_url {
        open(url);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_window_shows_every_status_but_idle() {
        let shown = [
            Status::Checking,
            Status::UpToDate,
            Status::Failed("no network".into()),
            Status::Installing,
            Status::Ready,
            Status::Managed,
            Status::Updated("0.2.0".into()),
            Status::InstallFailed("disk full".into()),
        ];
        for status in &shown {
            assert!(result_window_shows(status, false), "{status:?}");
            assert!(!result_window_shows(status, true), "{status:?} over the prompt");
        }
        assert!(!result_window_shows(&Status::Idle, false));
    }
}
