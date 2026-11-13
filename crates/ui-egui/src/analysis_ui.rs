//! Cooperative analysis driving and progress UI over the engine's shared job commands.
use crate::SheetApp;
use serde_json::json;

/// Retains only a UI-started cooperative job, never synchronous results or external jobs.
pub(crate) fn retain_dialog(app: &mut SheetApp, home: Option<u64>, dialog: &crate::dialogs::Dialog, result: &serde_json::Value) {
    if dialog.name == "monteCarlo"
        && let Some(id) = result.get("job").and_then(serde_json::Value::as_u64)
    {
        app.pending_analysis_dialog = Some((id, home, dialog.clone()));
    }
}

/// Advances at most one bounded batch per frame and publishes a completed UI-owned job.
/// Explicit MCP/CLI jobs remain caller-driven unless autoAdvance was requested.
pub(crate) fn tick(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(progress) = app.session.analysis_progress() else { return };
    if progress["autoAdvance"].as_bool() != Some(true) {
        return;
    }
    let id = progress["job"].clone();
    let cancelled = ctx.input(|input| input.key_pressed(egui::Key::Escape));
    let command = if cancelled {
        "analysis.cancel"
    } else if progress["state"] == "ready" {
        "analysis.finish"
    } else {
        "analysis.step"
    };
    let result = app.run(command, json!({"job":id}));
    if let Err(error) = result {
        if !app.session.analysis_active()
            && let Some((pending_id, home, dialog)) = app.pending_analysis_dialog.take()
            && Some(pending_id) == id.as_u64()
        {
            app.return_dialog(home, dialog);
        }
        app.alert(error);
    } else if matches!(command, "analysis.finish" | "analysis.cancel") {
        app.pending_analysis_dialog = None;
    }
    if app.session.analysis_active() {
        ctx.request_repaint();
    }
}

/// Shows progress and a real cancellation action while normal workbook controls are disabled.
pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(progress) = app.session.analysis_progress() else { return };
    let mut cancel = false;
    egui::Window::new("Monte Carlo progress")
        .id(egui::Id::unique("analysis-progress"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(if progress["stage"] == "sampling" { "Preparing simulation samples…" } else { "Evaluating isolated workbook trials…" });
            let fraction = progress["progress"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
            ui.add(egui::ProgressBar::new(fraction).show_percentage());
            ui.label(format!("{} / {} trials", progress["completed"], progress["total"]));
            ui.label("Your source workbook is preserved. A report appears after completion.");
            cancel = ui.button("Cancel simulation").clicked();
        });
    if cancel && let Err(error) = app.run("analysis.cancel", json!({"job":progress["job"]})) {
        app.alert(error);
    } else if cancel {
        app.pending_analysis_dialog = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A late failure restores the exact submitted dialog instead of losing its settings.
    #[test]
    fn failed_background_run_restores_dialog() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session.execute("cell.set", json!({"cell":"A1","input":"1"})).unwrap();
        session.execute("cell.set", json!({"cell":"B1","input":"=SQRT(A1)"})).unwrap();
        let mut app = SheetApp::new(session, Default::default());
        let mut dialog = crate::dialogs::Dialog::open(&mut app, "monteCarlo", json!({})).unwrap();
        dialog.values.insert("inputs".into(), json!([{"cell":"A1","distribution":"uniform","min":-1,"max":-1}]));
        dialog.values.insert("outputs".into(), json!("B1"));
        dialog.values.insert("iterations".into(), json!("2"));
        dialog.values.insert("method".into(), json!("random"));
        let params = crate::dialogs::build_params(&dialog);
        let result = app.run_dialog_command("data.monteCarlo", params).unwrap();
        let home = app.ui_window();
        retain_dialog(&mut app, home, &dialog, &result);
        let ctx = egui::Context::default();
        for _ in 0..8 {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| tick(&mut app, ui.ctx()));
            output.textures_delta.clear();
            if !app.session.analysis_active() {
                break;
            }
        }
        assert!(!app.session.analysis_active());
        assert_eq!(app.dialog.as_ref().unwrap().values["outputs"], "B1");
        assert_eq!(app.dialog.as_ref().unwrap().values["iterations"], "2");
        assert!(app.message.is_some());
    }
}
