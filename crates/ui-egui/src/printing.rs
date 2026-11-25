//! Localized print-page wording around the shared, backend-independent controls.
use craft_ui::printing::Labels;
pub use craft_ui::printing::{Discover, Duplex, Feedback, Printer, Settings, State, Submit};

/// Draws the shared print controls.
///
/// `craft_ui::printing::State::show` also calls `poll`, so this path polls a second time on top of
/// [`crate::SheetApp::poll_printing`] in `logic`. That poll is idempotent (it only drains worker
/// messages into the same state), and the logic-side poll is what lets a hidden window and a
/// logic-only frame see completion, so it stays. Splitting poll out of the shared `show` API is a
/// craft-ui follow-up, not a GridCraft-local change.
pub fn show(state: &mut State, ui: &mut egui::Ui, discover: &Discover, translate: impl Fn(&str) -> String) {
    let labels = Labels {
        printer: &translate("Printer"),
        select_printer: &translate("Select a printer"),
        refresh: &translate("Refresh printers"),
        copies: &translate("Copies"),
        duplex: &translate("Two-sided printing"),
        printer_default: &translate("Printer default"),
        one_sided: &translate("One-sided"),
        long_edge: &translate("Two-sided: long edge"),
        short_edge: &translate("Two-sided: short edge"),
        grayscale: &translate("Grayscale"),
    };
    state.show(ui, egui::Id::unique("gridcraft_print_settings"), discover, &labels);
    if let Some(message) = &state.message {
        ui.label(feedback(message, crate::i18n::current(ui.ctx()), translate));
    }
}

/// Read print settings/status, or validate an explicit update before changing anything.
/// `refresh: true` starts discovery only; this command never submits a job or opens a dialog.
pub fn settings(state: &mut State, params: &serde_json::Value, discover: Option<&Discover>) -> Result<serde_json::Value, String> {
    use serde_json::{Value, json};
    let object = params.as_object().ok_or("print settings must be an object")?;
    if object.keys().any(|key| !["printer", "copies", "duplex", "grayscale", "refresh"].contains(&key.as_str())) {
        return Err("unknown print setting; use printer, copies, duplex, grayscale or refresh".into());
    }
    if state.busy() && !object.is_empty() {
        return Err("wait for the active print submission before changing settings".into());
    }
    let mut next = state.settings.clone();
    if let Some(value) = object.get("printer") {
        let printer = value.as_str().ok_or("printer must be a name from the discovered list")?;
        if !state.printers.iter().any(|available| available.name == printer) {
            return Err("printer must be a name from the discovered list".into());
        }
        next.printer = printer.to_owned();
    }
    if let Some(value) = object.get("copies") {
        let copies = value.as_u64().filter(|copies| (1..=999).contains(copies)).ok_or("copies must be an integer from 1 to 999")?;
        next.copies = copies as u32;
    }
    if let Some(value) = object.get("duplex") {
        next.duplex = match value.as_str() {
            Some("printerDefault") => None,
            Some("off") => Some(Duplex::Off),
            Some("longEdge") => Some(Duplex::LongEdge),
            Some("shortEdge") => Some(Duplex::ShortEdge),
            None if value.is_null() => None,
            _ => return Err("duplex must be printerDefault, off, longEdge or shortEdge".into()),
        };
    }
    if let Some(value) = object.get("grayscale") {
        next.grayscale = value.as_bool().ok_or("grayscale must be a boolean")?;
    }
    let refresh = object.get("refresh").map(|value| value.as_bool().ok_or("refresh must be a boolean")).transpose()?.unwrap_or(false);
    if refresh {
        let discover = discover.ok_or("native printer discovery is unavailable here")?;
        if !state.refresh(discover) {
            return Err("printer discovery or submission is already active".into());
        }
    }
    state.settings = next;
    let (status, detail) = match state.message.as_ref() {
        None => ("idle", ""),
        Some(Feedback::FindingPrinters) => ("findingPrinters", ""),
        Some(Feedback::NoPrinters) => ("noPrinters", ""),
        Some(Feedback::DiscoveryFailed) => ("discoveryFailed", ""),
        Some(Feedback::Submitting) => ("submitting", ""),
        Some(Feedback::Submitted(receipt)) => ("submitted", receipt.as_str()),
        Some(Feedback::Failed(error)) => ("failed", error.as_str()),
        Some(Feedback::WorkerStopped) => ("workerStopped", ""),
    };
    Ok(json!({
        "printer": state.settings.printer, "copies": state.settings.copies,
        "duplex": match state.settings.duplex { None => "printerDefault", Some(Duplex::Off) => "off", Some(Duplex::LongEdge) => "longEdge", Some(Duplex::ShortEdge) => "shortEdge" },
        "grayscale": state.settings.grayscale,
        "printers": state.printers.iter().map(|printer| json!({"name": printer.name, "default": printer.default})).collect::<Vec<Value>>(),
        "busy": state.busy(), "discovering": state.discovering(), "ready": state.ready(), "status": status, "detail": detail,
    }))
}

pub fn feedback(message: &Feedback, language: crate::i18n::Language, translate: impl Fn(&str) -> String) -> String {
    match message {
        Feedback::FindingPrinters => translate("Finding printers…"),
        Feedback::NoPrinters => translate("No printers found. Configure a system printer or export a PDF."),
        Feedback::DiscoveryFailed => translate("Printer discovery failed."),
        Feedback::Submitting => translate("Submitting print job…"),
        Feedback::Submitted(receipt) if receipt.trim().is_empty() => translate("Print job submitted."),
        Feedback::Submitted(receipt) => translate("Print job submitted: {receipt}").replace("{receipt}", receipt),
        Feedback::Failed(error) => translate("Print failed: {error}").replace("{error}", &language.tr_error(error)),
        Feedback::WorkerStopped => translate("Print failed: the printing worker stopped."),
    }
}
