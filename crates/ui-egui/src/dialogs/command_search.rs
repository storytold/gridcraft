use super::{Dialog, Key, SheetApp, json, vec2};

/// Shows enabled commands and their shortcuts from engine metadata.
pub(super) fn command_search(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    let (down, up, home, end, pgup, pgdn) = ui.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
            i.consume_key(egui::Modifiers::CTRL, Key::Home),
            i.consume_key(egui::Modifiers::CTRL, Key::End),
            i.consume_key(egui::Modifiers::NONE, Key::PageUp),
            i.consume_key(egui::Modifiers::NONE, Key::PageDown),
        )
    });
    let language = app.ui.language;
    let width = ui.available_width();
    // The hint goes through the catalog like every other label, so it reads Hebrew under a Hebrew
    // interface (`he.rs` translates it) instead of falling back to the English literal.
    let r =
        ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(width).hint_text(language.tr("Type a command, e.g. freeze, chart, bold…")));

    if !d.focus_initialized {
        r.request_focus();
        d.focus_initialized = true;
    }

    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    let cmds = matching_commands(app, mac, &d.search);

    let last = cmds.len().saturating_sub(1);

    let row_h = ui.spacing().interact_size.y.max(20.0);
    let row_step = row_h + ui.spacing().item_spacing.y;

    const LIST_HEIGHT: f32 = 320.0;

    let page_size = ((LIST_HEIGHT / row_step).floor() as usize).saturating_sub(1).max(1);

    if down {
        d.list_index = (d.list_index + 1).min(last);
    }
    if up {
        d.list_index = d.list_index.saturating_sub(1);
    }
    if pgdn {
        d.list_index = (d.list_index + page_size).min(last);
    }
    if pgup {
        d.list_index = d.list_index.saturating_sub(page_size);
    }
    if home {
        d.list_index = 0;
    }
    if end {
        d.list_index = last;
    }

    d.list_index = d.list_index.min(last);

    egui::ScrollArea::vertical().max_height(LIST_HEIGHT).auto_shrink([true, false]).show(ui, |ui| {
        let scroll_area_width = ui.available_width();
        for (i, (id, label, menu, shortcut)) in cmds.iter().enumerate() {
            let text = match shortcut {
                Some(keys) => format!("{label}  ({keys})    {menu}"),
                None => format!("{label}    {menu}"),
            };
            let r = ui.add(egui::Button::selectable(i == d.list_index, text.clone()).truncate().min_size(vec2(scroll_area_width, 20.0)));

            if (down || up || home || end || pgup || pgdn) && i == d.list_index && !ui.clip_rect().contains_rect(r.rect) {
                r.scroll_to_me(Some(if down || end || pgdn { egui::Align::Max } else { egui::Align::Min }));
            }

            if r.clicked() {
                d.list_index = i;
                *confirm = true;
            }

            r.on_hover_text(format!("{text}\n{id}"));
        }
    });
    if ui.input(|i| i.key_pressed(Key::Enter)) {
        *confirm = true;
    }
    if *confirm && let Some((id, ..)) = cmds.get(d.list_index) {
        let id = id.clone();
        app.dialog = None;
        app.run_or_alert(&id, json!({}));
    }
}

fn matching_commands(app: &SheetApp, mac: bool, query: &str) -> Vec<(String, String, String, Option<String>)> {
    let language = app.ui.language;
    let q = query.to_lowercase();
    app.session
        .commands()
        .into_iter()
        .filter(|c| c.enabled)
        .filter(|c| {
            let tr_label = language.tr(c.label);
            q.is_empty() || c.label.to_lowercase().contains(&q) || tr_label.to_lowercase().contains(&q) || c.id.to_lowercase().contains(&q)
        })
        .map(|c| {
            let label = language.tr(c.label).to_string();
            let menu = c.menu.iter().map(|m| language.tr(m)).collect::<Vec<_>>().join(" › ");
            (c.id.to_string(), label, menu, c.shortcut_for_platform(mac))
        })
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::matching_commands;

    #[test]
    fn command_search_matches_hebrew_translations_and_english_names() {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = crate::SheetApp::new(session, crate::Services::default());
        app.ui.language = crate::i18n::Language::He;

        for (query, expected_id) in [("הקפא", "view.freezePanes"), ("תרשים", "insert.chart"), ("מודגש", "home.bold"), ("bold", "home.bold")]
        {
            let hits = matching_commands(&app, true, query);
            assert!(hits.iter().any(|(id, ..)| id == expected_id), "query {query:?} missed {expected_id}: {hits:?}");
        }
    }
}
