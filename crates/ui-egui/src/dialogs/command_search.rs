use super::{Dialog, Key, SheetApp, json, vec2};

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
    let width = ui.available_width();
    let r = ui.add(
        egui::TextEdit::singleline(&mut d.search)
            .desired_width(width)
            .hint_text(crate::i18n::current(ui.ctx()).tr("Type a command, e.g. freeze, chart, bold…")),
    );

    if !d.focus_initialized {
        r.request_focus();
        d.focus_initialized = true;
    }

    // A command is found by its id, its English label or its label in the UI language (the hint
    // suggests words in that language).
    let lang = crate::i18n::current(ui.ctx());
    let q = fold(&d.search);
    let cmds: Vec<(String, String, String)> = app
        .session
        .commands()
        .into_iter()
        .filter(|c| c.enabled)
        .filter_map(|c| {
            let label = command_label(lang, c.menu.first().copied(), c.label);
            let found = q.is_empty() || fold(c.id).contains(&q) || fold(c.label).contains(&q) || fold(&label).contains(&q);
            found.then(|| (c.id.to_string(), label, c.menu.iter().map(|m| lang.tr(m)).collect::<Vec<_>>().join(" › ")))
        })
        .take(40)
        .collect();

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
        for (i, (id, label, menu)) in cmds.iter().enumerate() {
            let r = ui.add(egui::Button::selectable(i == d.list_index, format!("{label}    {menu}")).min_size(vec2(scroll_area_width, 20.0)));

            if (down || up || home || end || pgup || pgdn) && i == d.list_index && !ui.clip_rect().contains_rect(r.rect) {
                r.scroll_to_me(Some(if down || end || pgdn { egui::Align::Max } else { egui::Align::Min }));
            }

            if r.clicked() {
                d.list_index = i;
                *confirm = true;
            }

            r.on_hover_text(id);
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

/// A command's label as the ribbon shows it: the tab-qualified key (`Page Layout|Orientation`)
/// when the language has one, otherwise the plain label.
fn command_label(lang: crate::i18n::Language, tab: Option<&str>, label: &str) -> String {
    let contextual = tab.map(|tab| lang.tr(&format!("{tab}|{label}")).to_string()).filter(|t| t != label);
    contextual.unwrap_or_else(|| lang.tr(label).to_string())
}

/// Lower case for matching, without the combining dot that lower-casing Turkish `İ` leaves
/// (`İtalik` → `i̇talik`), so `italik` finds it.
fn fold(s: &str) -> String {
    s.to_lowercase().replace('\u{307}', "")
}

#[cfg(test)]
mod tests {
    use super::{command_label, fold};
    use crate::i18n::Language;

    #[test]
    fn a_command_shows_the_label_its_ribbon_tab_uses() {
        assert_eq!(command_label(Language::De, Some("Page Layout"), "Orientation"), "Seitenausrichtung");
        assert_eq!(command_label(Language::En, Some("Page Layout"), "Orientation"), "Orientation");
        assert_eq!(command_label(Language::Pl, Some("Home"), "Bold"), "Pogrubienie");
    }

    #[test]
    fn matching_ignores_case_turkish_dotted_capital_i_included() {
        assert!(fold("İtalik").contains(&fold("italik")));
        assert!(fold("Pogrubienie").contains(&fold("POGRUB")));
    }
}
