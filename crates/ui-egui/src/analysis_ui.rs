//! Cooperative analysis driving and progress UI over the engine's shared job commands.
use crate::SheetApp;
use serde_json::json;

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
    if let Err(error) = app.run(command, json!({"job":id})) {
        app.alert(error);
    }
    if app.session.analysis_active() {
        ctx.request_repaint();
    }
}

/// Shows progress and a real cancellation action while normal workbook controls are disabled.
pub(crate) fn show(app: &mut SheetApp, ctx: &egui::Context) {
    let Some(progress) = app.session.analysis_progress() else { return };
    let mut cancel = false;
    egui::Window::new("Monte Carlo progress").id(egui::Id::unique("analysis-progress")).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label(if progress["stage"] == "sampling" { "Preparing simulation samples…" } else { "Evaluating isolated workbook trials…" });
        let fraction = progress["progress"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        ui.add(egui::ProgressBar::new(fraction).show_percentage());
        ui.label(format!("{} / {} trials", progress["completed"], progress["total"]));
        ui.label("Your source workbook is preserved. A report appears after completion.");
        cancel = ui.button("Cancel simulation").clicked();
    });
    if cancel && let Err(error) = app.run("analysis.cancel", json!({"job":progress["job"]})) {
        app.alert(error);
    }
}
