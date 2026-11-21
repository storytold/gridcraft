//! One background check at startup (and on Help ▸ Check for Updates…) and the modal prompt that
//! offers the new release: Update and Restart (direct self-update), Download or Later. The flow
//! itself is `craft_update::updater::Updater`.

use craft_update::CheckResultResponse;
use craft_update::updater::{Action, Status, Updater};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::SheetApp;

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Whether the update dialogs draw in Chinese: set from the app's interface language each frame,
/// read by [`zh_translate`] when a craft-update dialog draws a string.
static UPDATE_DIALOG_ZH: AtomicBool = AtomicBool::new(false);

/// The craft-update strings in Chinese, keyed by their English text (the lookup key upstream
/// keeps). Keys upstream added after the vendored fork got fresh translations here.
const ZH: &[(&str, &str)] = &[
    ("v{from} to {to}", "从 {from} 更新到 {to}"),
    ("Downloading and installing update…", "正在下载并安装更新…"),
    (
        "{app} {version} is available with the latest features and bug fixes.",
        "{app} {version} 已发布，包含最新功能与错误修复。",
    ),
    ("What's new", "更新内容"),
    ("Dismiss", "忽略"),
    ("Update and Restart", "更新并重启"),
    ("Restart to Update", "重启以更新"),
    ("Download", "下载"),
    ("Later", "稍后"),
    ("by {developer}", "来自 {developer}"),
    ("Update", "更新"),
    ("Close", "关闭"),
    ("Checking for updates…", "正在检查更新…"),
    ("Installing update…", "正在安装更新…"),
    ("Update ready. Close {app} to install it.", "更新已就绪。关闭 {app} 即可安装。"),
    (
        "The update is installed. Close {app} to finish; it restarts by itself.",
        "更新已安装。关闭 {app} 即可完成；应用会自动重启。",
    ),
    (
        "{app} {version} follows the version chosen in the launcher, so it does not update itself.",
        "{app} {version} 跟随启动器选择的版本，因此不会自行更新。",
    ),
    ("{app} updated to {version}.", "{app} 已更新到 {version}。"),
    ("The update could not be installed: {reason}", "无法安装更新：{reason}"),
    ("{app} {version} is the newest version available.", "{app} {version} 已是最新版本。"),
    ("Could not check for updates.", "无法检查更新。"),
    ("All releases are listed at {url}", "所有版本发布于 {url}"),
    ("OK", "确定"),
    ("Check Again", "重新检查"),
    ("The update failed unexpectedly.", "更新意外失败。"),
    ("The update stopped unexpectedly.", "更新意外中止。"),
    ("Save or close your open work first; the update restarts the app.", "请先保存或关闭打开的工作；更新会重启应用。"),
    (
        "Cannot auto-update this installation type. Please download manually.",
        "此安装类型无法自动更新，请手动下载。",
    ),
    ("Couldn't start the update ({error})", "无法启动更新（{error}）"),
];

/// The [`craft_update::translate::Translate`] handed to the updater: the English key in Chinese
/// when the interface language is Chinese, with its `{name}` placeholders filled either way.
fn zh_translate(text: &str, args: &[(&str, &str)]) -> String {
    let chosen = if UPDATE_DIALOG_ZH.load(Ordering::Relaxed) {
        ZH.iter().find(|(en, _)| *en == text).map_or(text, |(_, zh)| *zh)
    } else {
        text
    };
    craft_update::translate::fill(chosen, args)
}

pub(crate) struct Runtime {
    updater: Updater,
    /// The check at start was asked for; `tick` starts it with the context.
    startup: bool,
    /// Help ▸ Check for Updates… was chosen; the next `tick` starts it.
    manual: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            updater: Updater::new(&crate::UPDATE_APP, VERSION).with_translator(zh_translate),
            startup: false,
            manual: false,
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
    // The craft-update dialogs read the interface language through the translator's flag.
    UPDATE_DIALOG_ZH.store(app.ui.language == crate::i18n::Language::Zh, Ordering::Relaxed);
    if std::mem::take(&mut rt.manual) {
        rt.updater.check_now(ctx);
    } else if rt.startup {
        rt.updater.check_at_start(ctx);
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

pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    // The close for a ready update waits until no workbook holds unsaved work.
    let clean = !app.session.documents().iter().any(|d| d.is_dirty());
    match app.updates_rt.updater.show_if(ctx, clean) {
        Action::OpenUrl(url) => {
            if let Some(open) = &app.services.open_url {
                open(&url);
            }
        }
        // Quit closes every workbook window. The install helper starts once the app exits.
        Action::Close => app.request_quit(ctx),
        Action::None => {}
    }
    let rt = &mut app.updates_rt;
    if result_window_shows(rt.updater.status(), rt.updater.prompt_open()) {
        match craft_update::CheckResultDialog::new(&crate::UPDATE_APP, VERSION)
            .with_translator(zh_translate)
            .show(ctx, rt.updater.status())
        {
            CheckResultResponse::Close => rt.updater.dismiss_status(),
            CheckResultResponse::CheckAgain => rt.updater.check_now(ctx),
            // The next `show_if` asks the window to close once nothing is unsaved.
            CheckResultResponse::RestartToUpdate => rt.updater.restart_to_update(),
            CheckResultResponse::None => {}
        }
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
