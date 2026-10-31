use super::show::ok_cancel;
use super::{Dialog, Json, Map, SheetApp, Stroke, json, vec2};

/// The check boxes of Text Import's delimiters, with the delimiter each stands for.
const TEXT_IMPORT_DELIMITERS: [(&str, &str, &str); 4] =
    [("tab", "\t", "Tab"), ("semicolon", ";", "Semicolon"), ("comma", ",", "Comma"), ("space", " ", "Space")];

/// The `file.open` params for Text Import's values: the file and how it splits.
pub(super) fn text_import_params(values: &Map<String, Json>) -> Json {
    let get = |k: &str| values.get(k);
    let on = |k: &str| get(k).and_then(Json::as_bool).unwrap_or(false);
    let mut p = json!({});
    for k in ["path", "name", "base64"] {
        if let Some(v) = get(k) {
            p[k] = v.clone();
        }
    }
    match get("mode").and_then(Json::as_str).unwrap_or("detect") {
        "delimited" => {
            let mut delimiters: Vec<&str> = TEXT_IMPORT_DELIMITERS.iter().filter(|(k, ..)| on(k)).map(|(_, d, _)| *d).collect();
            if on("otherOn")
                && let Some(o) = get("other").and_then(Json::as_str).filter(|o| !o.is_empty())
            {
                delimiters.push(o);
            }
            p["delimiters"] = json!(delimiters);
            p["treatConsecutive"] = json!(on("consecutive"));
        }
        "fixed" => {
            let text = get("fixedWidths").and_then(Json::as_str).unwrap_or_default();
            let starts: Vec<u64> = text.split([',', ';', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
            p["fixedWidths"] = json!(starts);
        }
        _ => p["delimiter"] = json!("auto"),
    }
    if let Some(row) = get("startRow").and_then(Json::as_u64) {
        p["startRow"] = json!(row.max(1));
    }
    p["decimal"] = get("decimal").cloned().unwrap_or_else(|| json!("auto"));
    p["textQualifier"] = get("qualifier").cloned().unwrap_or_else(|| json!("\""));
    if on("asText") {
        p["csvAsText"] = json!(true);
    }
    p
}

/// Leaving Detect for Delimited or Fixed width starts from what detection found.
fn text_import_prefill(d: &mut Dialog, found: &Json) {
    match found["split"].as_str() {
        Some("delimited") | Some("lines") => {
            let found_delims: Vec<&str> = found["delimiters"].as_array().map(|a| a.iter().filter_map(Json::as_str).collect()).unwrap_or_default();
            for (k, delim, _) in TEXT_IMPORT_DELIMITERS {
                d.values.insert(k.into(), json!(found_delims.contains(&delim)));
            }
            let other = found_delims.iter().copied().find(|x| !TEXT_IMPORT_DELIMITERS.iter().any(|(_, delim, _)| delim == x));
            d.values.insert("otherOn".into(), json!(other.is_some()));
            d.values.insert("other".into(), json!(other.unwrap_or_default()));
            d.values.insert("consecutive".into(), json!(found["treatConsecutive"].as_bool().unwrap_or(false)));
        }
        Some("fixedWidth") => {
            let starts: Vec<String> =
                found["fixedWidths"].as_array().map(|a| a.iter().filter_map(Json::as_u64).map(|n| n.to_string()).collect()).unwrap_or_default();
            d.values.insert("fixedWidths".into(), json!(starts.join(", ")));
        }
        _ => {}
    }
    if d.values.get("startRow").is_none_or(Json::is_null)
        && let Some(row) = found["startRow"].as_u64()
    {
        d.values.insert("startRow".into(), json!(row));
    }
}

/// Text Import (#2553), shown when a `.txt` or `.prn` file is opened: how its lines split into
/// cells, with a preview of the first rows and what reading found. Detect is the default: the
/// delimiter, a title before the table and the decimal separator come from the file.
pub(super) fn text_import(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let t = crate::theme::Tokens::get(ui.ctx());
    // The preview is asked for again only when a setting changes. A picked file's bytes stay out
    // of the key, which is built every frame, and the preview gets only the part it reads.
    let picked = d.values.remove("base64");
    let key = Json::Object(d.values.clone()).to_string();
    if d.result.as_ref().and_then(|r| r.get("key")).and_then(Json::as_str) != Some(key.as_str()) {
        let mut p = text_import_params(&d.values);
        if let Some(b) = picked.as_ref().and_then(Json::as_str) {
            // Four base64 characters hold three bytes.
            let head = (gridcraft_engine::io::PREVIEW_BYTES + 1).div_ceil(3) * 4;
            p["base64"] = json!(b.get(..head).unwrap_or(b));
        }
        p["rows"] = json!(20);
        let r = app.session.run("file.previewText", p).unwrap_or_else(|e| json!({"error": crate::clean_error(&e)}));
        d.result = Some(json!({"key": key, "r": r}));
    }
    if let Some(b) = picked {
        d.values.insert("base64".into(), b);
    }
    let r = d.result.as_ref().and_then(|x| x.get("r")).cloned().unwrap_or(Json::Null);
    let found = &r["text"];
    let text = |d: &Dialog, k: &str| d.values.get(k).and_then(Json::as_str).unwrap_or_default().to_string();
    let flag = |d: &Dialog, k: &str| d.values.get(k).and_then(Json::as_bool).unwrap_or(false);

    let source = text(d, "path");
    let source = if source.is_empty() { text(d, "name") } else { source };
    let file = std::path::Path::new(&source).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or(&source).to_string();
    ui.label(egui::RichText::new(file).strong());
    ui.add_space(4.0);

    let mode = text(d, "mode");
    let mut chosen = mode.clone();
    ui.horizontal(|ui| {
        ui.label(lang.tr("Original data type:"));
        for (value, label) in [("detect", "Detect"), ("delimited", "Delimited"), ("fixed", "Fixed width")] {
            if ui.radio(chosen == value, lang.tr(label)).clicked() {
                chosen = value.to_string();
            }
        }
    });
    if chosen != mode {
        if mode == "detect" {
            text_import_prefill(d, found);
        }
        d.values.insert("mode".into(), json!(chosen));
    }
    match chosen.as_str() {
        "delimited" => {
            ui.horizontal_wrapped(|ui| {
                ui.label(lang.tr("Delimiters:"));
                for (k, _, label) in TEXT_IMPORT_DELIMITERS {
                    let mut on = flag(d, k);
                    if ui.checkbox(&mut on, lang.tr(label)).changed() {
                        d.values.insert(k.into(), json!(on));
                    }
                }
                let mut on = flag(d, "otherOn");
                let other_box = ui.checkbox(&mut on, lang.tr("Other:"));
                if other_box.changed() {
                    d.values.insert("otherOn".into(), json!(on));
                }
                let mut other = text(d, "other");
                if ui.add(egui::TextEdit::singleline(&mut other).desired_width(48.0)).labelled_by(other_box.id).changed() {
                    d.values.insert("otherOn".into(), json!(!other.is_empty()));
                    d.values.insert("other".into(), json!(other));
                }
            });
            let mut on = flag(d, "consecutive");
            if ui.checkbox(&mut on, lang.tr("Treat consecutive delimiters as one")).changed() {
                d.values.insert("consecutive".into(), json!(on));
            }
        }
        "fixed" => {
            ui.horizontal(|ui| {
                let label = ui.label(lang.tr("Fields start at character:"));
                let mut starts = text(d, "fixedWidths");
                if ui.add(egui::TextEdit::singleline(&mut starts).hint_text("10, 20").desired_width(180.0)).labelled_by(label.id).changed() {
                    d.values.insert("fixedWidths".into(), json!(starts));
                }
            });
        }
        _ => {}
    }
    ui.horizontal(|ui| {
        if chosen != "fixed" {
            let label = ui.label(lang.tr("Text qualifier:"));
            let qualifier = text(d, "qualifier");
            let shown = |q: &str| if q.is_empty() { lang.tr("{none}").to_string() } else { q.to_string() };
            let mut picked = qualifier.clone();
            craft_ui::ComboBox::from_id_salt("textImportQualifier")
                .selected_text(shown(&qualifier))
                .show_ui(ui, |ui| {
                    for q in ["\"", "'", ""] {
                        ui.selectable_value(&mut picked, q.to_string(), shown(q));
                    }
                })
                .response
                .labelled_by(label.id);
            if picked != qualifier {
                d.values.insert("qualifier".into(), json!(picked));
            }
            ui.add_space(12.0);
        }
        let label = ui.label(lang.tr("Decimal separator:"));
        let decimal = text(d, "decimal");
        let shown = |x: &str| if x == "auto" { lang.tr("Detect").to_string() } else { x.to_string() };
        let mut picked = decimal.clone();
        craft_ui::ComboBox::from_id_salt("textImportDecimal")
            .selected_text(shown(&decimal))
            .show_ui(ui, |ui| {
                for x in ["auto", ".", ","] {
                    ui.selectable_value(&mut picked, x.to_string(), shown(x));
                }
            })
            .response
            .labelled_by(label.id);
        if picked != decimal {
            d.values.insert("decimal".into(), json!(picked));
        }
    });
    ui.horizontal(|ui| {
        let label = ui.label(lang.tr("Start import at row:"));
        let mut row = d.values.get("startRow").and_then(Json::as_u64).or(found["startRow"].as_u64()).unwrap_or(1);
        if ui.add(egui::DragValue::new(&mut row).range(1..=1_048_576).speed(0.2)).labelled_by(label.id).changed() {
            d.values.insert("startRow".into(), json!(row));
        }
        ui.add_space(12.0);
        let mut on = flag(d, "asText");
        if ui.checkbox(&mut on, lang.tr("Keep every field as text")).changed() {
            d.values.insert("asText".into(), json!(on));
        }
    });

    // What reading found.
    ui.add_space(6.0);
    if let Some(e) = r["error"].as_str() {
        ui.colored_label(t.warning, e);
    } else {
        let split = match found["split"].as_str() {
            Some("delimited") => {
                let names: Vec<String> = found["delimiters"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Json::as_str)
                    .map(|x| match TEXT_IMPORT_DELIMITERS.iter().find(|(_, delim, _)| *delim == x) {
                        Some((_, _, label)) => lang.tr(label).to_string(),
                        None => x.to_string(),
                    })
                    .collect();
                format!("{} {}", lang.tr("Delimiter:"), names.join(" "))
            }
            Some("fixedWidth") => {
                let starts: Vec<String> =
                    found["fixedWidths"].as_array().into_iter().flatten().filter_map(Json::as_u64).map(|n| n.to_string()).collect();
                format!("{} {}", lang.tr("Fixed width at:"), starts.join(", "))
            }
            _ => lang.tr("No delimiter: each line in one cell").to_string(),
        };
        let decimal = format!(
            "{}{}",
            lang.tr(if found["decimal"].as_str() == Some(",") { "Decimal comma" } else { "Decimal point" }),
            match found["decimalFrom"].as_str() {
                Some("values") => format!(" {}", lang.tr("(from the values)")),
                Some("delimiter") => format!(" {}", lang.tr("(guessed from the delimiter)")),
                _ => String::new(),
            }
        );
        let rows = format!(
            "{} {} / {}{}",
            lang.tr("Complete rows:"),
            found["completeRows"].as_u64().unwrap_or(0),
            found["rows"].as_u64().unwrap_or(0),
            if r["partial"].as_bool().unwrap_or(false) { format!(" {}", lang.tr("(in the first MiB)")) } else { String::new() }
        );
        ui.label(egui::RichText::new(format!("{split} · {decimal} · {rows}")).color(t.text_dim));
        for u in found["uncertain"].as_array().into_iter().flatten().filter_map(Json::as_str) {
            ui.colored_label(t.warning, u);
        }
        let ambiguous: Vec<&str> = found["ambiguous"].as_array().into_iter().flatten().filter_map(Json::as_str).collect();
        if !ambiguous.is_empty() && found["decimalFrom"].as_str() != Some("given") {
            ui.colored_label(t.warning, format!("{} {}", lang.tr("Could be read either way:"), ambiguous.join("  ")));
        }
        for n in found["splitNumbers"].as_array().into_iter().flatten() {
            let row = n["row"].as_u64().unwrap_or(0);
            ui.colored_label(t.warning, format!("{} {row}: {}", lang.tr("Row with a split number:"), n["number"].as_str().unwrap_or_default()));
        }
    }

    ui.add_space(4.0);
    egui::Frame::new().fill(t.input_bg).stroke(Stroke::new(1.0, t.input_border)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        egui::ScrollArea::both().max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("textImportPreview").striped(true).spacing(vec2(14.0, 2.0)).show(ui, |ui| {
                for row in r["preview"].as_array().into_iter().flatten() {
                    for field in row.as_array().into_iter().flatten().take(40) {
                        let shown: String = field.as_str().unwrap_or_default().chars().take(40).collect();
                        ui.label(egui::RichText::new(shown).monospace());
                    }
                    ui.end_row();
                }
            });
        });
    });
    ok_cancel(ui, confirm, open);
}
