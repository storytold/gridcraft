use super::*;

/// Paste Special as Excel lays it out: Paste and Operation each in two columns of options, Skip
/// blanks and Transpose side by side, and Paste Link as a button that pastes at once. The options
/// come from the dialog's `fields`, so the letter shortcuts and the control channel see the same.
pub(super) fn paste_special(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = app.ui.language;
    let t = theme::Tokens::get(ui.ctx());
    let choices: Vec<(&'static str, &'static str, Vec<(&'static str, &'static str)>)> = d
        .fields
        .iter()
        .filter_map(|f| match f {
            Field::Choice { key, label, options } => Some((*key, *label, options.clone())),
            _ => None,
        })
        .collect();
    for (key, label, options) in choices {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(lang.tr(label)).strong());
        egui::Frame::new().stroke(Stroke::new(1.0, t.separator)).corner_radius(4.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            let cur = d.values.get(key).and_then(Json::as_str).unwrap_or("").to_string();
            // Column by column, as Excel fills them.
            let rows = options.len().div_ceil(2);
            egui::Grid::new(("paste_special", key)).num_columns(2).min_col_width(200.0).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                for r in 0..rows {
                    for c in 0..2 {
                        if let Some((value, text)) = options.get(c * rows + r) {
                            if ui.radio(cur == *value, lang.tr(text)).clicked() {
                                d.set_value(key, json!(value));
                            }
                        } else {
                            ui.label("");
                        }
                    }
                    ui.end_row();
                }
            });
        });
    }
    ui.add_space(8.0);
    // In the same columns as the options above.
    egui::Frame::new().inner_margin(egui::Margin::symmetric(11, 0)).show(ui, |ui| {
        egui::Grid::new("paste_special_checks").num_columns(2).min_col_width(200.0).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
            for (key, label) in [("skipBlanks", "Skip blanks"), ("transpose", "Transpose (E)")] {
                let mut b = d.values.get(key).and_then(Json::as_bool).unwrap_or(false);
                if ui.checkbox(&mut b, lang.tr(label)).changed() {
                    d.values.insert(key.into(), json!(b));
                }
            }
            ui.end_row();
        });
    });
    ui.add_space(8.0);
    ui.separator();
    ui.horizontal(|ui| {
        // Excel's Paste Link pastes references to the copied cells straight away.
        if ui.button(lang.tr("Paste Link")).on_hover_text(lang.tr("Paste formulas that refer to the copied cells")).clicked() {
            d.values.insert("what".into(), json!("all"));
            d.values.insert("operation".into(), json!("none"));
            d.values.insert("link".into(), json!(true));
            *confirm = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = ui.add(egui::Button::new(egui::RichText::new(format!("   {}   ", lang.tr("OK"))).color(t.on_accent)).fill(t.accent));
            if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                *confirm = true;
            }
            if ui.button(format!(" {} ", lang.tr("Cancel"))).clicked() {
                *open = false;
            }
        });
    });
}

/// Paste Special's legacy mnemonics select options; Enter still performs the paste.
pub(super) fn paste_special_shortcuts(d: &mut Dialog, ctx: &egui::Context) {
    if ctx.text_edit_focused() {
        return;
    }
    let right_alt = ctx.input(|i| {
        i.key_down(Key::AltRight)
            || i.events.iter().any(|event| {
                matches!(event, egui::Event::Key { key: Key::AltRight, .. })
                    || matches!(event, egui::Event::Key { physical_key: Some(Key::AltRight), .. })
            })
    });
    let mac = ctx.os() == egui::os::OperatingSystem::Mac;
    let allowed = |m: egui::Modifiers| !m.ctrl && !m.command && !m.mac_cmd && !(m.alt && (mac || right_alt));
    let mnemonic = |key| match key {
        Key::T => Some('t'),
        Key::V => Some('v'),
        Key::F => Some('f'),
        Key::C => Some('c'),
        Key::E => Some('e'),
        _ => None,
    };
    let mut text_to_consume = Vec::new();
    for event in ctx.input(|i| i.events.clone()) {
        let egui::Event::Key { key, pressed: true, repeat, modifiers, .. } = event else { continue };
        if !allowed(modifiers) {
            continue;
        }
        let Some(letter) = mnemonic(key) else { continue };
        text_to_consume.push(letter);
        if repeat {
            continue;
        }
        if key == Key::E {
            let transpose = d.values.get("transpose").and_then(Json::as_bool).unwrap_or(false);
            d.values.insert("transpose".into(), json!(!transpose));
        } else {
            let what = match key {
                Key::T => "formats",
                Key::V => "values",
                Key::F => "formulas",
                _ => "comments",
            };
            d.values.insert("what".into(), json!(what));
        }
    }
    // Browsers can deliver Text before or after Key. Act only on Key, then remove
    // its matching text so one physical press cannot toggle Transpose twice.
    ctx.input_mut(|i| {
        i.events.retain(|event| match event {
            egui::Event::Key { key, modifiers, .. } => !(allowed(*modifiers) && mnemonic(*key).is_some()),
            egui::Event::Text(text) if text.len() == 1 => {
                let letter = text.chars().next().unwrap_or_default().to_ascii_lowercase();
                if let Some(index) = text_to_consume.iter().position(|&c| c == letter) {
                    text_to_consume.remove(index);
                    false
                } else {
                    true
                }
            }
            _ => true,
        });
    });
}
