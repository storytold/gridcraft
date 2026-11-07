//! One background check at startup (and on Help ▸ Check for Updates…) and the modal prompt that
//! offers the new release: Update and Restart (direct self-update), Download or Later. The flow
//! itself is `craft_update::updater::Updater`.

use craft_update::CheckResultResponse;
use craft_update::updater::{Action, Status, Updater};

use crate::{SheetApp, i18n::Language};

thread_local! {
    static UPDATE_LANGUAGE: std::cell::Cell<Language> = const { std::cell::Cell::new(Language::En) };
}

/// Translates the shared updater UI using the language of the window drawing it.
fn translate_update(text: &str, args: &[(&str, &str)]) -> String {
    UPDATE_LANGUAGE.with(|language| craft_update::translate::fill(language.get().tr(text), args))
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

pub(crate) struct Runtime {
    updater: Updater,
    /// The check at start was asked for; `tick` starts it with the context.
    startup: bool,
    /// Help ▸ Check for Updates… was chosen; the next `tick` starts it.
    manual: bool,
    /// The result dialog of a requested check is showing.
    result_open: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            updater: Updater::new(&crate::UPDATE_APP, VERSION).with_translator(translate_update),
            startup: false,
            manual: false,
            result_open: false,
        }
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
        rt.updater.check_now(ctx);
        rt.result_open = true;
    } else if rt.startup {
        rt.updater.check_at_start(ctx);
    }
    rt.updater.tick(ctx);
    // A requested check that found a release leaves the status idle and the prompt opens.
    if matches!(rt.updater.status(), Status::Idle) {
        rt.result_open = false;
    }
}

/// Draws update status and handles updater actions only when unsaved document state permits them.
pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    // The restart helper is scheduled before the app closes, so no update starts over unsaved work.
    let clean = !app.session.documents().iter().any(|d| d.is_dirty());
    let previous_language = UPDATE_LANGUAGE.replace(app.ui.language);
    let action = app.updates_rt.updater.show_if(ctx, clean);
    UPDATE_LANGUAGE.set(previous_language);
    match action {
        Action::OpenUrl(url) => {
            if let Some(open) = &app.services.open_url {
                open(&url);
            }
        }
        // The guarded close path asks about unsaved work; the restart helper relaunches once the
        // process has exited.
        Action::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Action::None => {}
    }
    if app.updates_rt.result_open {
        let rt = &mut app.updates_rt;
        match check_result(ctx, rt.updater.status(), app.ui.language) {
            CheckResultResponse::Close => {
                rt.updater.dismiss_status();
                rt.result_open = false;
            }
            CheckResultResponse::CheckAgain => rt.updater.check_now(ctx),
            CheckResultResponse::None => {}
        }
    }
}

/// Presents the shared updater's result through the application's translation catalog.
fn check_result(ctx: &egui::Context, status: &Status, language: crate::i18n::Language) -> CheckResultResponse {
    if matches!(status, Status::Idle) {
        return CheckResultResponse::None;
    }
    let mut response = CheckResultResponse::None;
    egui::Modal::new(egui::Id::unique("gridcraft.update_result")).show(ctx, |ui| {
        ui.set_width(400.0);
        ui.heading(language.tr("Check for Updates…"));
        ui.separator();
        match status {
            Status::Checking => {
                ui.spinner();
                ui.label(language.tr("Checking for updates…"));
            }
            Status::UpToDate => {
                ui.label(craft_i18n::fmt(language.tr("GridCraft {version} is the newest version available."), &[("version", VERSION)]));
            }
            Status::Failed(error) => {
                ui.label(language.tr("Could not check for updates."));
                ui.label(error);
            }
            Status::Idle => {}
        }
        ui.horizontal(|ui| {
            if matches!(status, Status::Failed(_)) && ui.button(language.tr("Check Again")).clicked() {
                response = CheckResultResponse::CheckAgain;
            }
            if ui.button(language.tr(if matches!(status, Status::Checking) { "Cancel" } else { "OK" })).clicked() {
                response = CheckResultResponse::Close;
            }
        });
    });
    if ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        response = CheckResultResponse::Close;
    }
    response
}
