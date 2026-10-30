use super::{Dialog, Key, SheetApp, json, vec2};
use crate::widgets::tr;

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
    let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(width).hint_text(tr(ui, "Type a command, e.g. freeze, chart, bold…")));

    if !d.focus_initialized {
        r.request_focus();
        d.focus_initialized = true;
    }

    // Matches the label as shown in the interface language as well as the English label and id,
    // so "gras" finds Bold in French and "bold" still does; the translated menu path counts too
    // ("accueil" lists the Home commands). Case and accents are ignored.
    let lang = app.ui.language;
    let q = fold(&d.search);
    let cmds: Vec<(String, String, String)> = app
        .session
        .commands()
        .into_iter()
        .filter(|c| c.enabled)
        .map(|c| {
            let shown = shown_label(lang, c.label, &c.menu);
            let menu = c.menu.iter().map(|m| lang.tr(m)).collect::<Vec<_>>().join(" › ");
            (c, shown, menu)
        })
        .filter(|(c, shown, menu)| {
            q.is_empty() || fold(shown).contains(&q) || fold(menu).contains(&q) || fold(c.label).contains(&q) || fold(c.id).contains(&q)
        })
        .map(|(c, shown, menu)| (c.id.to_string(), shown, menu))
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

/// A command's label as the interface shows it: the ribbon keys some labels by their tab
/// ("Page Layout|Orientation"), so that translation wins over the bare label's.
fn shown_label(lang: crate::i18n::Language, label: &str, menu: &[&str]) -> String {
    menu.iter().map(|m| lang.tr(&format!("{m}|{label}")).to_string()).find(|t| t != label).unwrap_or_else(|| lang.tr(label).to_string())
}

/// Lowercases `s` and strips accents from Latin (Vietnamese included) and Greek letters
/// ("Insérer" → "inserer", "Đậm" → "dam"), so a query matches whether or not it is typed with
/// its accents; Greek final ς folds to σ. Combining marks are dropped too.
fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            '\u{300}'..='\u{36f}' => {}
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            'ß' => out.push_str("ss"),
            c => out.push(match c {
                'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
                'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
                'ď' | 'đ' => 'd',
                'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
                'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
                'ĥ' | 'ħ' => 'h',
                'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
                'ĵ' => 'j',
                'ķ' => 'k',
                'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
                'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
                'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
                'ŕ' | 'ŗ' | 'ř' => 'r',
                'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => 's',
                'ţ' | 'ť' | 'ŧ' | 'ț' => 't',
                'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
                'ŵ' => 'w',
                'ý' | 'ÿ' | 'ŷ' => 'y',
                'ź' | 'ż' | 'ž' => 'z',
                'ơ' => 'o',
                'ư' => 'u',
                // Vietnamese letters with tone marks (Latin Extended Additional).
                '\u{1ea0}'..='\u{1eb7}' => 'a',
                '\u{1eb8}'..='\u{1ec7}' => 'e',
                '\u{1ec8}'..='\u{1ecb}' => 'i',
                '\u{1ecc}'..='\u{1ee3}' => 'o',
                '\u{1ee4}'..='\u{1ef1}' => 'u',
                '\u{1ef2}'..='\u{1ef9}' => 'y',
                'ά' => 'α',
                'έ' => 'ε',
                'ή' => 'η',
                'ί' | 'ϊ' | 'ΐ' => 'ι',
                'ό' => 'ο',
                'ύ' | 'ϋ' | 'ΰ' => 'υ',
                'ώ' => 'ω',
                'ς' => 'σ',
                c => c,
            }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{fold, shown_label};
    use crate::i18n::Language;

    #[test]
    fn fold_ignores_case_and_accents() {
        assert_eq!(fold("Insérer un GRAPHIQUE"), "inserer un graphique");
        assert_eq!(fold("Œuvre Ærø Straße"), "oeuvre aero strasse");
        assert_eq!(fold("Ștergere Łódź"), "stergere lodz");
        assert_eq!(fold("e\u{301}cart"), "ecart");
        // Turkish İ lowercases to i plus a combining dot, which is dropped.
        assert_eq!(fold("İtalik"), "italik");
        assert_eq!(fold("Đậm Cỡ chữ"), "dam co chu");
        assert_eq!(fold("ΜΈΓΕΘΟΣ"), fold("Μέγεθος"));
        assert_eq!(fold("Μέγεθος"), "μεγεθοσ");
    }

    #[test]
    fn shown_label_prefers_the_ribbon_context_key() {
        let menu = ["Page Layout", "Page Setup"];
        assert_eq!(shown_label(Language::Ja, "Orientation", &menu), Language::Ja.tr("Page Layout|Orientation"));
        assert_ne!(Language::Ja.tr("Page Layout|Orientation"), Language::Ja.tr("Orientation"));
        assert_eq!(shown_label(Language::Fr, "Bold", &["Home", "Font"]), "Gras");
        assert_eq!(shown_label(Language::En, "Orientation", &menu), "Orientation");
    }
}
