use super::{Dialog, Key, SheetApp, json, vec2};
use unicode_casefold::{Locale, UnicodeCaseFold, Variant};
use unicode_normalization::UnicodeNormalization;

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
    let width = ui.available_width();
    let r = ui.add(
        egui::TextEdit::singleline(&mut d.search).desired_width(width).hint_text(app.ui.language.tr("Type a command, e.g. freeze, chart, bold…")),
    );

    if !d.focus_initialized {
        r.request_focus();
        d.focus_initialized = true;
    }

    let mac = ui.ctx().os() == egui::os::OperatingSystem::Mac;
    // Use translated labels for searching, falling back to English
    let q = search_fold(d.search.trim());
    let cmds: Vec<(String, String, String, Option<String>)> = app
        .session
        .commands()
        .into_iter()
        .filter(|c| c.enabled)
        .filter(|c| {
            let label = app.ui.language.tr(c.label);
            q.is_empty() || search_fold(label).contains(&q) || search_fold(c.label).contains(&q) || c.id.to_ascii_lowercase().contains(&q)
        })
        .map(|c| {
            let display_label = app.ui.language.tr(c.label).to_string();
            let menu = c.menu.iter().map(|segment| app.ui.language.tr(segment)).collect::<Vec<_>>().join(" › ");
            (c.id.to_string(), display_label, menu, c.shortcut_for_platform(mac))
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

/// Folds text for searching with full Unicode case folding, so expansions match (`ß` and `ẞ` both
/// fold to `ss`, so `strasse` finds `Straße`). Text is normalized to NFC before and after folding,
/// so a composed accent and a base letter plus its combining mark compare equal.
///
/// The fold uses the Turkic locale, which maps `İ` to `i` and `I` to `ı` (mapped back to `i`
/// below), so `I`, `İ`, `i` and `ı` all match one another without dropping a combining dot. Dots
/// that belong to other letters — a decomposed Polish `ż`, for one — are left untouched.
fn search_fold(text: &str) -> String {
    text.nfc().case_fold_with(Variant::Full, Locale::Turkic).map(|c| if c == 'ı' { 'i' } else { c }).collect::<String>().nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::search_fold;

    #[test]
    fn search_fold_treats_turkish_i_forms_alike() {
        for text in ["I", "İ", "i", "ı"] {
            assert_eq!(search_fold(text), "i", "{text:?}");
        }
        // A query typed with plain `i` still finds labels written with dotted `İ` or dotless `ı`.
        assert!(search_fold("İtalik").contains(&search_fold("italik")));
        assert!(search_fold("Yapıştır").contains(&search_fold("yapıştır")));
    }

    #[test]
    fn search_fold_lowercases_ordinary_text() {
        assert_eq!(search_fold("Bold"), "bold");
        assert_eq!(search_fold("ÇĞÖŞÜ"), "çğöşü");
    }

    #[test]
    fn search_fold_expands_full_case_folding() {
        assert_eq!(search_fold("Straße"), "strasse");
        assert_eq!(search_fold("STRAẞE"), "strasse");
        assert_eq!(search_fold("strasse"), search_fold("Straße"));
        // Greek final sigma folds to the ordinary sigma.
        assert_eq!(search_fold("ΟΔΟΣ"), search_fold("οδος"));
    }

    #[test]
    fn search_fold_matches_composed_and_decomposed_accents() {
        // "Gráfico" with a precomposed U+00E1 and with "a" + U+0301 must fold the same.
        assert_eq!(search_fold("Gráfico"), "gráfico");
        assert_eq!(search_fold("Gra\u{301}fico"), "gráfico");
        assert_eq!(search_fold("GRA\u{301}FICO"), "gráfico");
    }

    #[test]
    fn search_fold_keeps_dots_that_do_not_come_from_turkish_i() {
        // Polish `ż` is `z` + U+0307 when decomposed; its dot must survive.
        assert_eq!(search_fold("Pokaż formuły"), "pokaż formuły");
        assert_eq!(search_fold("Pokaz\u{307} formuły"), "pokaż formuły");
        assert_eq!(search_fold("POKAZ\u{307} FORMUŁY"), "pokaż formuły");
        // A decomposed `İ` still folds to the plain `i`.
        assert_eq!(search_fold("I\u{307}"), "i");
    }
}
