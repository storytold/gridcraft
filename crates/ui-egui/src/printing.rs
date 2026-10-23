//! Localized print-page wording around the shared, backend-independent controls.
pub use craft_ui::capability::Capability;
pub use craft_ui::printing::network as net;
pub use craft_ui::printing::{Discover, Duplex, Feedback, Printer, Settings, State, Submit};

use crate::i18n::Language;

/// What the desktop host supplies for printing to a network printer (WS-Print or IPP): the address
/// policy, the worker that asks a printer what it lists, and the worker that sends a job and follows
/// it. The model, its rules and the widgets are craft-ui's (`printing::network`); the protocols are
/// craft-print's, in the app crate. Absent in the browser (no sockets) and in builds without the
/// `network-print` feature.
pub struct Network {
    /// Checks the normalised address against the network policy; `Err` is shown under the field.
    pub validate: Box<net::Validate>,
    /// Asks the printer at an address what it is and what it lists. Sends no job.
    pub query: net::QueryPrinter,
    /// Makes the worker that sends one job (see [`NetworkJob`]).
    pub send: Box<dyn Fn(NetworkJob) -> net::SendJob>,
    /// The media types the app can produce (a PDF).
    pub formats: &'static [&'static str],
}

/// One document for [`Network::send`].
pub struct NetworkJob {
    /// The job's name on the printer.
    pub name: String,
    /// The print-ready PDF.
    pub pdf: std::sync::Arc<Vec<u8>>,
    /// The person's ticked confirmation for this one job (craft-ui clears it when the job starts).
    pub confirmed: bool,
}

/// The Print page's network destination: craft-ui's state and whether it is the chosen destination.
#[derive(Default)]
pub struct NetworkDest {
    pub state: net::State,
    pub selected: bool,
}

/// The `MediaSizeName` of the standard sheet whose size in points is `width` x `height` (either
/// orientation), for the sizes printers list.
pub fn media_for_points(width: f64, height: f64) -> Option<&'static str> {
    let (short, long) = if width <= height { (width, height) } else { (height, width) };
    let near = |a: f64, b: f64| (a - b).abs() <= 2.0;
    [
        (612.0, 792.0, "na_letter_8.5x11in"),
        (612.0, 1008.0, "na_legal_8.5x14in"),
        (792.0, 1224.0, "na_ledger_11x17in"),
        (842.0, 1191.0, "iso_a3_297x420mm"),
        (595.0, 842.0, "iso_a4_210x297mm"),
        (420.0, 595.0, "iso_a5_148x210mm"),
        (522.0, 756.0, "na_executive_7.25x10.5in"),
        (297.6, 419.5, "iso_a6_105x148mm"),
        (729.0, 1032.0, "jis_b4_257x364mm"),
        (516.0, 729.0, "jis_b5_182x257mm"),
        (363.0, 516.0, "jis_b6_128x182mm"),
        (709.0, 1001.0, "iso_b4_250x353mm"),
        (499.0, 709.0, "iso_b5_176x250mm"),
        (354.0, 499.0, "iso_b6_125x176mm"),
        (283.5, 419.5, "jpn_hagaki_100x148mm"),
        (419.5, 567.0, "jpn_oufuku_148x200mm"),
        (340.0, 666.0, "jpn_chou3_120x235mm"),
        (255.0, 581.0, "jpn_chou4_90x205mm"),
        (680.0, 941.0, "jpn_kaku2_240x332mm"),
        (612.0, 785.0, "jpn_kaku3_216x277mm"),
    ]
    .into_iter()
    .find(|&(w, h, _)| near(short, w) && near(long, h))
    .map(|(_, _, token)| token)
}

/// The PWG 5101.1 `MediaSizeName` of a Page Setup paper name (spelled as the engine accepts it).
pub fn media_for_paper(paper: &str) -> Option<&'static str> {
    let name = paper.trim().to_ascii_lowercase().replace([' ', '-', '_', '(', ')'], "");
    Some(match name.as_str() {
        "letter" | "usletter" => "na_letter_8.5x11in",
        "legal" | "uslegal" => "na_legal_8.5x14in",
        "executive" => "na_executive_7.25x10.5in",
        "tabloid" | "11x17" | "ledger" => "na_ledger_11x17in",
        "a3" => "iso_a3_297x420mm",
        "a4" => "iso_a4_210x297mm",
        "a5" => "iso_a5_148x210mm",
        "a6" => "iso_a6_105x148mm",
        // B4/B5/B6 are the JIS sizes (as in the engine); the ISO series is spelled out.
        "b4" | "b4jis" => "jis_b4_257x364mm",
        "b5" | "b5jis" => "jis_b5_182x257mm",
        "b6" | "b6jis" => "jis_b6_128x182mm",
        "b4iso" => "iso_b4_250x353mm",
        "b5iso" => "iso_b5_176x250mm",
        "b6iso" => "iso_b6_125x176mm",
        "japanesepostcard" | "postcard" => "jpn_hagaki_100x148mm",
        "japanesedoublepostcard" | "doublepostcard" => "jpn_oufuku_148x200mm",
        "japaneseenvelope#3" | "chou#3" => "jpn_chou3_120x235mm",
        "japaneseenvelope#4" | "chou#4" => "jpn_chou4_90x205mm",
        "japaneseenvelopekaku#2" | "kaku#2" => "jpn_kaku2_240x332mm",
        "japaneseenvelopekaku#3" | "kaku#3" => "jpn_kaku3_216x277mm",
        _ => return None,
    })
}

/// The wording of the network destination in the interface language. Unlisted strings fall back to
/// English, like the rest of the Print page.
pub fn network_labels(l: Language) -> net::Labels<'static> {
    net::Labels {
        address: l.tr("Printer address:"),
        address_hint: "http://192.168.1.50:5357/service",
        address_note: l.tr(
            "Only IP addresses on this network are accepted (private, link-local or loopback), not host names. An ipp:// address or a path with /ipp/ uses IPP; anything else uses WS-Print.",
        ),
        query_printer: l.tr("Query printer"),
        querying: l.tr("Asking the printer…"),
        query_first: l.tr("Query the printer first to see what it supports."),
        printer_status: l.tr("Printer status: {state}"),
        reasons: l.tr("Reasons: {reasons}"),
        no_common_format: l.tr("The printer accepts none of the formats this document can be sent in"),
        copies: l.tr("Copies:"),
        sides: l.tr("Two-sided:"),
        sides_off: l.tr("Off"),
        sides_long_edge: l.tr("Flip on long edge"),
        sides_short_edge: l.tr("Flip on short edge"),
        orientation: l.tr("Printer orientation:"),
        portrait: l.tr("Portrait"),
        landscape: l.tr("Landscape"),
        reverse_portrait: l.tr("Reverse portrait"),
        reverse_landscape: l.tr("Reverse landscape"),
        quality: l.tr("Quality:"),
        draft: l.tr("Draft"),
        normal: l.tr("Normal"),
        high: l.tr("High"),
        photo: l.tr("Photo"),
        color: l.tr("Color:"),
        color_color: l.tr("Colour"),
        color_monochrome: l.tr("Grayscale"),
        resolution: l.tr("Resolution:"),
        resolution_dpi: l.tr("{dpi} dpi"),
        resolution_dpi_xy: l.tr("{x} x {y} dpi"),
        auto: l.tr("Auto"),
        unsupported_copies: l.tr("This printer does not take a copies setting."),
        unsupported_sides: l.tr("This printer only prints one-sided."),
        unsupported_orientation: l.tr("This printer does not take an orientation setting."),
        unsupported_quality: l.tr("This printer does not take a quality setting."),
        unsupported_color: l.tr("This printer only prints in grayscale."),
        unsupported_resolution: l.tr("This printer does not take a resolution setting."),
        unconfirmed_note: l.tr("Not confirmed by the printer"),
        confirm: l.tr("I confirm sending this document to this printer"),
        sending: l.tr("Sending to {printer}…"),
        canceling: l.tr("Canceling…"),
        cancel: l.tr("Cancel"),
        sent: l.tr("Sent to {printer}"),
        accepted: l.tr("Accepted by {printer}"),
        stage_waiting: l.tr("Waiting in the printer's queue"),
        stage_held: l.tr("Held by the printer"),
        stage_printing: l.tr("The printer is printing it"),
        stage_stopped: l.tr("The printer paused the job; it needs attention"),
        sheets: l.tr("Sheets printed: {n}"),
        sheets_progress: l.tr("Sheets printed: {done} of {total}"),
        outcome_warnings: l.tr("Printed with warnings"),
        outcome_errors: l.tr("Printed with errors"),
        outcome_canceled: l.tr("The job was canceled"),
        outcome_aborted: l.tr("The printer stopped the job"),
        outcome_forgotten: l.tr("The printer no longer lists the job; it most likely finished"),
        outcome_still_running: l.tr("Still printing; the job continues on the printer"),
        outcome_accepted_unknown: l.tr("Sent. The printer can't say whether it printed."),
        outcome_unknown_warning: l.tr("The printer may have received the document. Check the printer before sending again."),
        failure_policy: l.tr("The printer address was refused"),
        failure_transport: l.tr("Couldn't reach the printer"),
        failure_protocol: l.tr("The printer's reply couldn't be understood"),
        failure_fault: l.tr("The printer refused the request"),
        failure_no_common_format: l.tr("The printer accepts none of the formats this document can be sent in"),
        failure_unsupported: l.tr("The printer doesn't support this setting"),
        failure_encode: l.tr("The document couldn't be prepared"),
        failure_invalid: l.tr("A setting isn't valid"),
        failure_stopped: l.tr("The print job stopped unexpectedly"),
        failure_other: l.tr("The print job stopped unexpectedly"),
    }
}

/// The sentence a finished or failed network job ends with, for the notice when the Print page is
/// not showing it. `None` while the job is still going.
pub fn network_notice(view: &net::JobView, labels: &net::Labels<'_>) -> Option<String> {
    match view {
        net::JobView::Finished(report) => Some(
            labels
                .outcome_text(report.outcome)
                .map_or_else(|| labels.sent.replace("{printer}", &report.printer), |text| format!("{}: {text}", report.printer)),
        ),
        net::JobView::Failed(failure) => {
            let mut text = format!("{}: {}", labels.failure_heading(failure.kind), failure.detail);
            if failure.outcome_unknown {
                text = format!("{text} {}", labels.outcome_unknown_warning);
            }
            Some(text)
        }
        _ => None,
    }
}

/// What a printer is called on screen: its label (the system's description) when it has one, else its
/// name. The name stays the selection key.
fn row_text(printer: &Printer) -> &str {
    if printer.label.trim().is_empty() { &printer.name } else { &printer.label }
}

/// The selected printer's on-screen text (its label, else its name; the name itself if it is not listed).
fn shown_name(printers: &[Printer], selected: &str) -> String {
    printers.iter().find(|printer| printer.name == selected).map_or_else(|| selected.to_owned(), |printer| row_text(printer).to_owned())
}

/// The Print page's printer controls: ONE Printer list holding the system's queues (the default
/// first and marked) and, where the host can send to the network, a final "Network printer…" entry
/// (`network` is that entry's selected flag). Choosing a queue clears the flag; choosing the entry sets
/// it, and the caller then draws the network section instead of the local options. The local options
/// (copies, two-sided, grayscale) are drawn only while a queue is chosen.
pub fn show(
    state: &mut State,
    ui: &mut egui::Ui,
    discover: &Discover,
    mut network: Option<&mut bool>,
    locked: bool,
    translate: impl Fn(&str) -> String,
) {
    if state.printers.is_empty() && state.message.is_none() && !state.discovering() {
        state.refresh(discover);
    }
    state.poll(ui.ctx());
    ui.push_id(egui::Id::new("gridcraft_print_settings"), |ui| {
        ui.add_enabled_ui(!state.busy() && !locked, |ui| {
            let width = ui.available_width().clamp(32.0, 230.0);
            let network_chosen = network.as_deref().is_some_and(|chosen| *chosen);
            let network_label = translate(NETWORK_ENTRY);
            let printer_label = ui.label(translate("Printer"));
            let shown = if network_chosen {
                network_label.clone()
            } else if state.settings.printer.is_empty() {
                translate("Select a printer")
            } else {
                shown_name(&state.printers, &state.settings.printer)
            };
            let mut order: Vec<&Printer> = state.printers.iter().collect();
            order.sort_by_key(|printer| !printer.default);
            let mut chosen = None;
            egui::ComboBox::from_id_salt("printer")
                .width(width)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for printer in order {
                        let text =
                            if printer.default { format!("{} ({})", row_text(printer), translate("default")) } else { row_text(printer).to_owned() };
                        if ui.selectable_label(!network_chosen && state.settings.printer == printer.name, text).clicked() {
                            chosen = Some(printer.name.clone());
                        }
                    }
                    if network.is_some() && ui.selectable_label(network_chosen, &network_label).clicked() {
                        chosen = Some(String::new());
                    }
                })
                .response
                .labelled_by(printer_label.id);
            match (chosen, network.as_deref_mut()) {
                (Some(name), Some(flag)) if name.is_empty() => *flag = true,
                (Some(name), flag) => {
                    state.settings.printer = name;
                    if let Some(flag) = flag {
                        *flag = false;
                    }
                }
                (None, _) => {}
            }
            if ui.add_enabled(!state.discovering(), egui::Button::new(translate("Refresh printers"))).clicked() {
                state.refresh(discover);
            }
            if network.as_deref().is_some_and(|chosen| *chosen) {
                return;
            }
            ui.horizontal(|ui| {
                let label = ui.label(translate("Copies"));
                ui.add(egui::DragValue::new(&mut state.settings.copies).range(1..=999)).labelled_by(label.id);
            });
            let default = translate("Printer default");
            let one_sided = translate("One-sided");
            let long_edge = translate("Two-sided: long edge");
            let short_edge = translate("Two-sided: short edge");
            let choices = [
                (None, default.as_str()),
                (Some(Duplex::Off), one_sided.as_str()),
                (Some(Duplex::LongEdge), long_edge.as_str()),
                (Some(Duplex::ShortEdge), short_edge.as_str()),
            ];
            let duplex_label = ui.label(translate("Two-sided printing"));
            egui::ComboBox::from_id_salt("duplex")
                .width(width)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .selected_text(choices.iter().find(|(value, _)| *value == state.settings.duplex).map_or(default.as_str(), |(_, text)| *text))
                .show_ui(ui, |ui| {
                    for (value, text) in choices {
                        ui.selectable_value(&mut state.settings.duplex, value, text);
                    }
                })
                .response
                .labelled_by(duplex_label.id);
            ui.checkbox(&mut state.settings.grayscale, translate("Grayscale"));
        });
    });
    if let Some(message) = &state.message {
        ui.label(feedback(message, crate::i18n::current(ui.ctx()), translate));
    }
}

/// The Printer list's last entry (the English text is also its catalog key).
pub const NETWORK_ENTRY: &str = "Network printer…";

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
        Feedback::Submitted(receipt) if receipt.trim().is_empty() => {
            translate("Print job sent to the print queue. Check the queue if it does not print.")
        }
        Feedback::Submitted(receipt) => {
            translate("Print job sent to the print queue: {receipt}. Check the queue if it does not print.").replace("{receipt}", receipt)
        }
        Feedback::Failed(error) => translate("Print failed: {error}").replace("{error}", &language.tr_error(error)),
        Feedback::WorkerStopped => translate("Print failed: the printing worker stopped."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_sheets_name_their_media_in_either_orientation() {
        assert_eq!(media_for_points(612.0, 792.0), Some("na_letter_8.5x11in"));
        assert_eq!(media_for_points(842.0, 595.0), Some("iso_a4_210x297mm"));
        assert_eq!(media_for_points(1224.0, 792.0), Some("na_ledger_11x17in"));
        assert_eq!(media_for_points(500.0, 500.0), None);
        assert_eq!(media_for_points(f64::NAN, 792.0), None);
    }

    /// Every paper Page Setup and Page Layout › Size offer has a media name, the same one its size gives.
    #[test]
    fn every_paper_size_has_a_media_name() {
        let doc = gridcraft_engine::find_command("pageLayout.pageSetup").map(|c| c.params).unwrap_or_default();
        let listed = doc.split("paper?: ").nth(1).and_then(|rest| rest.split(',').next()).expect("pageSetup lists its papers");
        let mut papers: Vec<&str> = listed.split('|').map(|p| p.split(" (B4/").next().unwrap_or(p).trim()).collect();
        papers.extend(["Letter", "Legal", "Tabloid", "Executive", "A3", "A4", "A5"]);
        for paper in papers {
            let media = media_for_paper(paper).unwrap_or_else(|| panic!("{paper} has no media name"));
            let (w, h) = gridcraft_engine::cmd::print::paper_size(paper);
            assert_eq!(media_for_points(f64::from(w), f64::from(h)), Some(media), "{paper}");
        }
        assert_eq!(media_for_paper(" us letter "), Some("na_letter_8.5x11in"));
        assert_eq!(media_for_paper("Napkin"), None);
    }

    #[test]
    fn a_finished_or_failed_job_gets_a_sentence_that_names_the_printer() {
        let labels = network_labels(Language::En);
        let done = net::JobView::Finished(net::Report { outcome: net::Outcome::Completed, printer: "Office".into(), sheets: 1, notes: Vec::new() });
        assert_eq!(network_notice(&done, &labels).as_deref(), Some("Sent to Office"));
        let warned = net::JobView::Finished(net::Report { outcome: net::Outcome::Aborted, printer: "Office".into(), sheets: 0, notes: Vec::new() });
        assert_eq!(network_notice(&warned, &labels).as_deref(), Some("Office: The printer stopped the job"));
        let failed = net::JobView::Failed(net::Failure::new(net::FailureKind::Transport, "timed out").with_outcome_unknown(true));
        let text = network_notice(&failed, &labels).expect("a sentence");
        assert!(text.starts_with("Couldn't reach the printer: timed out") && text.contains("Check the printer before sending again"), "{text}");
        assert_eq!(network_notice(&net::JobView::Idle, &labels), None);
    }

    #[test]
    fn a_printer_shows_its_label_or_else_its_name() {
        let printers = vec![
            Printer { name: "HP_LJ_4".into(), label: "Front Desk Laser".into(), default: false },
            Printer { name: "Plain".into(), label: "  ".into(), default: false },
        ];
        assert_eq!(row_text(&printers[0]), "Front Desk Laser");
        assert_eq!(row_text(&printers[1]), "Plain");
        assert_eq!(shown_name(&printers, "HP_LJ_4"), "Front Desk Laser");
        assert_eq!(shown_name(&printers, "Plain"), "Plain");
        assert_eq!(shown_name(&printers, "Gone"), "Gone");
    }

    #[test]
    fn the_network_destination_is_only_offered_by_the_host() {
        let app = crate::SheetApp::new(gridcraft_engine::Session::default(), Default::default());
        assert!(!app.network_selected(), "no host service: no network destination, even if it was selected");
    }
}
