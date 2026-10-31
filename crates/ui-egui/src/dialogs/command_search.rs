use super::{Dialog, Key, SheetApp, vec2};

/// Search (Alt+Q, the title bar's search icon, Automate › Command Palette): type to find a ribbon
/// command; with nothing typed, the commands recently run from here. The first match is
/// highlighted, Up/Down move the highlight, Enter or a click runs it and closes the box. A command
/// that asks for its dialog opens it, as from the ribbon.
pub(super) fn command_search(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    // Arrow keys, Home/End and the page keys move the highlight here, so the text cursor and the
    // list don't both react.
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
            .hint_text(app.ui.language.tr("Search for a command, e.g. freeze, chart, bold…")),
    );
    if !d.focus_initialized {
        r.request_focus();
        d.focus_initialized = true;
    }
    if r.changed() {
        d.list_index = 0;
    }
    let recent = d.search.trim().is_empty();
    let cmds = if recent {
        crate::command_search::recent(&app.session, app.recent_commands())
    } else {
        crate::command_search::search(&app.session, &d.search)
    };
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

    let t = crate::theme::Tokens::get(ui.ctx());
    if recent {
        ui.label(egui::RichText::new(if cmds.is_empty() { "Type to search for a command." } else { "Recently used" }).color(t.text_dim));
    } else if cmds.is_empty() {
        ui.label(egui::RichText::new("No matching commands.").color(t.text_dim));
    }
    egui::ScrollArea::vertical().max_height(LIST_HEIGHT).auto_shrink([true, false]).show(ui, |ui| {
        let scroll_area_width = ui.available_width();
        for (i, c) in cmds.iter().enumerate() {
            let shortcut = c.shortcut.map(|s| format!("    {s}")).unwrap_or_default();
            let text = format!("{}    {}{shortcut}", c.label, c.menu.join(" › "));
            let r = ui.add(egui::Button::selectable(i == d.list_index, text).min_size(vec2(scroll_area_width, 20.0)));
            if (down || up || home || end || pgup || pgdn) && i == d.list_index && !ui.clip_rect().contains_rect(r.rect) {
                r.scroll_to_me(Some(if down || end || pgdn { egui::Align::Max } else { egui::Align::Min }));
            }
            if r.clicked() {
                d.list_index = i;
                *confirm = true;
            }
            r.on_hover_text(c.id);
        }
    });
    // Enter with nothing listed keeps the box open.
    if !cmds.is_empty() && ui.input(|i| i.key_pressed(Key::Enter)) {
        *confirm = true;
    }
    if *confirm && let Some(c) = cmds.get(d.list_index) {
        let id = c.id;
        app.dialog = None;
        app.run_searched_command(ui.ctx(), id);
    }
}
