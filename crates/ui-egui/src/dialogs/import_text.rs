use egui::{Color32, vec2};

use super::{Dialog, Json, Map, SheetApp, json};
use crate::theme;

const IMPORT_DELIMITERS: [(&str, &str); 4] = [("auto", "Detect automatically"), (",", "Comma"), (";", "Semicolon"), ("\\t", "Tab")];
const IMPORT_DECIMALS: [(&str, &str); 3] = [("auto", "Detect automatically"), ("dot", "Dot (1.5)"), ("comma", "Comma (1,5)")];
const IMPORT_DETECT: [(&str, &str); 3] =
    [("rows200", "Based on first 200 rows"), ("all", "Based on entire dataset"), ("none", "Do not detect data types")];
const IMPORT_TYPES: [(&str, &str); 5] =
    [("auto", "General"), ("text", "Text"), ("number", "Number"), ("date", "Date"), ("skip", "Do not import column")];
/// Rows of the file shown under the column formats.
const IMPORT_PREVIEW_ROWS: u32 = 15;

fn choice_label(options: &[(&'static str, &'static str)], value: &str) -> &'static str {
    options.iter().find(|(v, _)| *v == value).map_or("", |(_, t)| *t)
}

/// Opens Import Text/CSV on the `path`, `text` or `base64` in `params`. A workbook picked here
/// opens as File › Open would; any other read error is shown instead of the dialog.
pub(super) fn open(app: &mut SheetApp, params: &Json) -> Option<Dialog> {
    let mut d = Dialog::custom("importText", "Import Text/CSV", json!({"delimiter": "auto", "header": true, "decimal": "auto", "detect": "rows200"}));
    for k in ["path", "text", "base64"] {
        if let Some(v) = params.get(k).filter(|v| v.is_string()) {
            d.values.insert(k.into(), v.clone());
        }
    }
    d.command = Some("data.fromTextCsv");
    if let Err(e) = import_preview(app, &mut d) {
        match d.values.get("path").and_then(Json::as_str) {
            Some(path) if e.contains(gridcraft_engine::cmd::text_import::WORKBOOK_NOT_TEXT) => app.open_path(path),
            _ => app.message = Some((d.title.clone(), crate::clean_error(&e))),
        }
        return None;
    }
    Some(d)
}

/// Import Text/CSV: reads the file with the dialog's delimiter, header, decimal and detection
/// settings and (re)builds the per-column formats. A rebuild with the same number of columns keeps
/// the format chosen for each column.
fn import_preview(app: &mut SheetApp, d: &mut Dialog) -> Result<(), String> {
    let mut q = Map::new();
    for k in ["path", "text", "base64", "delimiter", "header", "decimal", "detect"] {
        if let Some(v) = d.values.get(k) {
            q.insert(k.into(), v.clone());
        }
    }
    q.insert("limit".into(), json!(IMPORT_PREVIEW_ROWS));
    let r = app.run("data.previewText", Json::Object(q))?;
    let cols = r["columns"].as_array().cloned().unwrap_or_default();
    let old = d.values.get("map").and_then(Json::as_array).cloned().filter(|m| m.len() == cols.len());
    let map = cols
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let kind = old.as_ref().and_then(|o| o.get(i)).map_or(json!("auto"), |o| o["type"].clone());
            json!({"source": c["index"], "name": c["name"], "type": kind, "detected": c["type"]})
        })
        .collect();
    d.values.insert("map".into(), Json::Array(map));
    d.result = Some(r);
    Ok(())
}

/// `data.fromTextCsv` params from Import Text/CSV: the file, how to read it and each column's format.
pub(super) fn import_params(d: &Dialog) -> Json {
    let mut p = Map::new();
    for k in ["path", "text", "base64", "delimiter", "header", "decimal", "detect"] {
        if let Some(v) = d.values.get(k) {
            p.insert(k.into(), v.clone());
        }
    }
    let map = d.values.get("map").and_then(Json::as_array).cloned().unwrap_or_default();
    p.insert("columns".into(), Json::Array(map.iter().map(|m| json!({"source": m["source"], "type": m["type"]})).collect()));
    Json::Object(p)
}

/// What stops the import as set: every column is "Do not import column".
fn import_problem(map: &[Json]) -> Option<&'static str> {
    map.iter().all(|m| m["type"] == "skip").then_some("Choose at least one column to import.")
}

fn combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    options: &[(&'static str, &'static str)],
    value: &mut String,
    shown: String,
) -> bool {
    let lang = crate::i18n::current(ui.ctx());
    let mut changed = false;
    craft_ui::ComboBox::from_id_salt(id).width(width).selected_text(shown).show_ui(ui, |ui| {
        for (v, t) in options {
            if ui.selectable_label(value == v, lang.tr(t)).clicked() && value != v {
                *value = (*v).to_string();
                changed = true;
            }
        }
    });
    changed
}

/// A settings combo whose `auto` choice shows what was detected: "Detect automatically (Semicolon)".
fn import_setting(ui: &mut egui::Ui, d: &mut Dialog, key: &str, options: &[(&'static str, &'static str)], detected: &str) -> bool {
    let lang = crate::i18n::current(ui.ctx());
    let mut value = d.values.get(key).and_then(Json::as_str).unwrap_or("auto").to_string();
    let shown = match value.as_str() {
        "auto" => format!("{} ({})", lang.tr("Detect automatically"), lang.tr(detected)),
        v => lang.tr(choice_label(options, v)).to_string(),
    };
    let changed = combo(ui, ("import", key), 200.0, options, &mut value, shown);
    if changed {
        d.values.insert(key.into(), json!(value));
    }
    changed
}

/// Data › Get Data (Text/CSV), as Excel's import: how the file is read (delimiter, decimal
/// separator, data type detection, a header row) and a preview with each column's format; Load
/// puts the data on a new sheet as a table.
pub(super) fn import_text(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    if let Some(path) = d.values.get("path").and_then(Json::as_str) {
        let name = std::path::Path::new(path).file_name().and_then(|f| f.to_str()).unwrap_or(path);
        ui.label(egui::RichText::new(name).strong());
    }
    let result = d.result.clone().unwrap_or(Json::Null);
    let mut reread = false;
    egui::Grid::new("importSettings").num_columns(3).spacing(vec2(16.0, 2.0)).show(ui, |ui| {
        for h in ["Delimiter", "Decimal separator", "Data Type Detection"] {
            ui.label(lang.tr(h));
        }
        ui.end_row();
        let delimiter = choice_label(&IMPORT_DELIMITERS, result["delimiter"].as_str().unwrap_or(","));
        reread |= import_setting(ui, d, "delimiter", &IMPORT_DELIMITERS, delimiter);
        let decimal = if result["decimal"] == "comma" { "Comma (1,5)" } else { "Dot (1.5)" };
        reread |= import_setting(ui, d, "decimal", &IMPORT_DECIMALS, decimal);
        let mut detect = d.values.get("detect").and_then(Json::as_str).unwrap_or("rows200").to_string();
        let shown = lang.tr(choice_label(&IMPORT_DETECT, &detect)).to_string();
        if combo(ui, "importDetect", 200.0, &IMPORT_DETECT, &mut detect, shown) {
            d.values.insert("detect".into(), json!(detect));
            reread = true;
        }
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let mut header = d.values.get("header").and_then(Json::as_bool).unwrap_or(true);
        if ui.checkbox(&mut header, lang.tr("My data has headers")).changed() {
            d.values.insert("header".into(), json!(header));
            reread = true;
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("{} {}", lang.tr("Rows of data:"), result["rows"].as_u64().unwrap_or(0))).weak());
    });
    if reread && let Err(e) = import_preview(app, d) {
        app.message = Some((d.title.clone(), crate::clean_error(&e)));
    }

    // The preview: names, then each column's format (General shows what was detected), then rows.
    let result = d.result.clone().unwrap_or(Json::Null);
    let mut map = d.values.get("map").and_then(Json::as_array).cloned().unwrap_or_default();
    let weak = ui.visuals().weak_text_color();
    ui.add_space(6.0);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        egui::ScrollArea::both().id_salt("importPreview").max_height(330.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("importPreviewGrid").striped(true).spacing(vec2(12.0, 3.0)).show(ui, |ui| {
                for m in &map {
                    let skip = m["type"] == "skip";
                    let name = egui::RichText::new(m["name"].as_str().unwrap_or_default()).strong();
                    ui.label(if skip { name.strikethrough().color(weak) } else { name });
                }
                ui.end_row();
                for (i, m) in map.iter_mut().enumerate() {
                    let mut kind = m["type"].as_str().unwrap_or("auto").to_string();
                    let shown = match kind.as_str() {
                        "auto" => {
                            let detected = choice_label(&IMPORT_TYPES, m["detected"].as_str().unwrap_or("text"));
                            format!("{} ({})", lang.tr("General"), lang.tr(detected))
                        }
                        v => lang.tr(choice_label(&IMPORT_TYPES, v)).to_string(),
                    };
                    if combo(ui, ("importType", i), 140.0, &IMPORT_TYPES, &mut kind, shown) {
                        m["type"] = json!(kind);
                    }
                }
                ui.end_row();
                for row in result["preview"].as_array().into_iter().flatten() {
                    for m in &map {
                        let at = m["source"].as_u64().unwrap_or(0) as usize;
                        let text = egui::RichText::new(row.get(at).and_then(Json::as_str).unwrap_or_default());
                        ui.label(if m["type"] == "skip" { text.color(weak) } else { text });
                    }
                    ui.end_row();
                }
            });
        });
    });

    let problem = import_problem(&map);
    d.values.insert("map".into(), Json::Array(map));
    if let Some(p) = problem {
        ui.colored_label(theme::Tokens::get(ui.ctx()).danger, lang.tr(p));
    }
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.button(format!(" {} ", lang.tr("Cancel"))).clicked() {
            *open = false;
        }
        let load = ui.add_enabled(
            problem.is_none(),
            egui::Button::new(egui::RichText::new(format!("   {}   ", lang.tr("Load"))).color(Color32::WHITE))
                .fill(theme::Tokens::get(ui.ctx()).accent),
        );
        if load.clicked() {
            *confirm = true;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::build_params;
    use super::*;

    #[test]
    fn import_text_opens_a_picked_workbook_instead() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        // A template (or any workbook) named whatever: told apart by content.
        let bytes = gridcraft_engine::io::save_bytes(&app.session.active().unwrap().wb, "book.xlsx").unwrap();
        let path = std::env::temp_dir().join(format!("gridcraft-import-{}.xltx", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let path = path.to_string_lossy().into_owned();
        assert!(Dialog::open(&mut app, "importText", json!({"path": path})).is_none());
        let _ = std::fs::remove_file(&path);
        assert_eq!(app.session.active().unwrap().path.as_deref(), Some(path.as_str()));
        assert!(app.message.is_none(), "{:?}", app.message);
    }

    #[test]
    fn import_text_reads_headers_and_formats_then_loads_a_table() {
        let mut session = gridcraft_engine::Session::default();
        session.new_workbook();
        let mut app = SheetApp::new(session, Default::default());
        let text = "Region;Produkt;Kwota\nPółnoc;A;1,5\nPołudnie;B;2\nPółnoc;B;3\n";
        let mut d = Dialog::open(&mut app, "importText", json!({"text": text})).unwrap();
        let map = d.values["map"].as_array().unwrap().clone();
        assert_eq!(map[0]["name"], "Region");
        assert_eq!(map[2]["detected"], "number");
        assert_eq!(d.result.as_ref().unwrap()["rows"], 3);
        // Without a header row the first row is data and the columns are Column1…
        d.values.insert("header".into(), json!(false));
        import_preview(&mut app, &mut d).unwrap();
        assert_eq!(d.values["map"][0]["name"], "Column1");
        assert_eq!(d.result.as_ref().unwrap()["rows"], 4);
        d.values.insert("header".into(), json!(true));
        import_preview(&mut app, &mut d).unwrap();
        // Do not import Produkt. The data loads as a table on one new sheet, nothing else.
        let mut map = d.values["map"].as_array().unwrap().clone();
        map[1]["type"] = json!("skip");
        d.values.insert("map".into(), Json::Array(map.clone()));
        assert_eq!(import_problem(&map), None);
        let sheets = app.session.active().unwrap().wb.sheets.len();
        let r = app.run(d.command.unwrap(), build_params(&d)).unwrap();
        assert_eq!(r["cols"], 2);
        assert!(r["pivot"].is_null(), "{r}");
        let doc = app.session.active().unwrap();
        assert_eq!(doc.wb.sheets.len(), sheets + 1);
        assert!(doc.wb.sheets.iter().all(|sh| sh.pivots.is_empty()));
        assert_eq!(doc.wb.active().unwrap().tables.len(), 1);
        // Every column skipped stops the import.
        for m in &mut map {
            m["type"] = json!("skip");
        }
        assert!(import_problem(&map).is_some());
    }
}
