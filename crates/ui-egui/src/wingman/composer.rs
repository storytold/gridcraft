//! Spreadsheet context completion for the message editor. The interaction pattern follows
//! <https://zed.dev/docs/ai/agent-panel#adding-context>; all references are workbook-local.

use std::ops::Range;

use egui::{
    Key, Modifiers, TextEdit,
    text::{CCursor, CCursorRange},
};

use super::workbook::MENTIONS;
use crate::SheetApp;
use crate::i18n::Language;
use gridcraft_engine::model::Visibility;

mod references;
pub(super) use references::{mentions, read_target};

const COMMANDS: &[&str] = &["/help", "/new", "/clear", "/mode", "/settings"];
const MODE_COMMANDS: &[&str] = &["/mode chat", "/mode read", "/mode write"];

fn hint(name: &str) -> &'static str {
    match name {
        "@selection" => "Selected cells",
        "@read" => "Read a range: @read B2:D8 or @read 'Q1 Sales'!B2:D8",
        "@cell" => "Active cell",
        "@sheet" => "Current sheet",
        "@workbook" => "Workbook overview",
        "@charts" => "Charts on this sheet",
        "@comments" => "Cell comments",
        "@vba" => "VBA project information",
        "/new" => "Start a new conversation",
        "/clear" => "Clear this conversation",
        "/help" => "List commands and references",
        "/mode" => "Choose a permission mode",
        "/mode chat" => "Chat with no tools",
        "/mode read" => "Inspect the workbook",
        "/mode write" => "Propose workbook edits",
        "/settings" => "Open the configuration file",
        _ => "",
    }
}

#[derive(Default)]
pub(super) struct Composer {
    pub(super) text: String,
    focus_requested: bool,
    picker: Option<Picker>,
    no_matches: Option<Query>,
    dismissed: Option<Query>,
    ime_active: bool,
    menu_rect: Option<egui::Rect>,
}

#[derive(PartialEq, Eq, Clone)]
struct Query {
    /// Byte range of the entire word, not just the typed prefix before the insertion point.
    span: Range<usize>,
    prefix: String,
    /// Identity and revision invalidate suggestions after a rename, hide, or workbook switch.
    sheets: Option<(u64, u64)>,
}

struct Picker {
    query: Query,
    matches: Vec<String>,
    selected: usize,
}

impl Query {
    fn at(text: &str, cursor: CCursorRange, app: &SheetApp) -> Option<Self> {
        if cursor.primary.index != cursor.secondary.index {
            return None;
        }
        let caret: usize = cursor.primary.index.into();
        let pos = text.char_indices().nth(caret).map_or(text.len(), |(pos, _)| pos);
        let (before, after) = text.split_at(pos);
        let slash = before.trim_start();
        if let Some(mode) = slash.strip_prefix("/mode ")
            && mode.bytes().all(|b| b.is_ascii_alphabetic())
        {
            let end = text.len() - after.trim_start_matches(char::is_alphabetic).len();
            return Some(Self { span: (pos - slash.len())..end, prefix: slash.to_ascii_lowercase(), sheets: None });
        }
        if let Some(query) = Self::sheet_at(text, pos, app) {
            return Some(query);
        }
        let word_start = before.trim_end_matches(char::is_alphabetic).len();
        let sigil = before[..word_start].chars().next_back()?;
        if !matches!(sigil, '@' | '/') {
            return None;
        }
        let start = word_start.checked_sub(1)?;
        let heading = &text[..start];
        if (sigil == '/' && !heading.trim().is_empty()) || (sigil == '@' && !references::boundary(heading)) || references::in_code(heading) {
            return None;
        }
        // A caret inside an explicit range must not replace just the '@read' part.
        let suffix_end = after.trim_start_matches(char::is_alphabetic);
        if matches!(suffix_end.as_bytes().first(), Some(b':' | b'0'..=b'9' | b'_')) {
            return None;
        }
        let end = pos + after.len() - suffix_end.len();
        Some(Self { span: start..end, prefix: text[start..pos].to_ascii_lowercase(), sheets: None })
    }

    fn sheet_at(text: &str, pos: usize, app: &SheetApp) -> Option<Self> {
        let before = &text[..pos];
        let start = before.rfind('@')?;
        let typed = &before[start + 1..];
        let target = typed.strip_prefix("read ").or_else(|| typed.strip_prefix("read:"))?;
        let quoted = target.starts_with('\'');
        if target.is_empty()
            || target.len() > 260
            || (!quoted && references::looks_like_a1(target))
            || !references::boundary(&text[..start])
            || references::in_code(&text[..start])
        {
            return None;
        }
        let tail = &text[pos..];
        // Only complete an unfinished sheet name, not a name already followed by !address.
        let word = |c: char| c.is_alphanumeric() || matches!(c, '.' | '-' | '_');
        let prefix = if quoted {
            let raw = &target[1..];
            if raw.contains(['\r', '\n', '!']) {
                return None;
            }
            raw.trim_end_matches('\'').replace("''", "'")
        } else {
            if !target.chars().all(word) {
                return None;
            }
            target.to_owned()
        };
        let rest = tail.trim_start_matches(|c| word(c) || (quoted && c == '\''));
        if rest.starts_with('!') {
            return None;
        }
        let d = app.session.active()?;
        Some(Self { span: start..text.len() - rest.len(), prefix: prefix.to_lowercase(), sheets: Some((d.uid, d.revision)) })
    }

    fn options(&self, app: &SheetApp) -> Vec<String> {
        if self.sheets.is_some() {
            return app
                .session
                .active()
                .into_iter()
                .flat_map(|d| d.wb.sheets.iter())
                .filter(|s| s.visibility == Visibility::Visible && s.name.to_lowercase().starts_with(&self.prefix))
                .take(8)
                .map(|s| format!("@read {}!", references::quote_sheet(&s.name)))
                .collect();
        }
        let terms = if self.prefix.starts_with("/mode ") {
            MODE_COMMANDS
        } else if self.prefix.starts_with('/') {
            COMMANDS
        } else {
            MENTIONS
        };
        terms.iter().filter(|s| s.starts_with(&self.prefix)).map(|&s| s.to_owned()).collect()
    }
}

impl Composer {
    pub(super) fn id() -> egui::Id {
        egui::Id::unique("gridcraft.wingman.composer")
    }

    pub(super) fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    pub(super) fn take_draft(&mut self) -> Option<String> {
        if self.text.trim().is_empty() {
            return None;
        }
        self.dismissed = None;
        self.no_matches = None;
        self.picker = None;
        self.menu_rect = None;
        self.request_focus();
        Some(std::mem::take(&mut self.text))
    }

    fn refresh(&mut self, cursor: Option<CCursorRange>, app: &SheetApp) {
        let query = cursor.and_then(|cursor| Query::at(&self.text, cursor, app));
        if query != self.dismissed {
            self.dismissed = None;
        }
        let Some(query) = query.filter(|q| self.dismissed.as_ref() != Some(q)) else {
            self.picker = None;
            return;
        };
        if self.no_matches.as_ref() == Some(&query) {
            self.picker = None;
            return;
        }
        if self.picker.as_ref().is_some_and(|p| p.query == query) {
            return;
        }
        let matches = query.options(app);
        if matches.is_empty() {
            self.no_matches = Some(query);
            self.picker = None;
        } else {
            self.no_matches = None;
            self.picker = Some(Picker { query, matches, selected: 0 });
        }
    }

    fn accept(&mut self, ui: &egui::Ui, index: usize) {
        let Some(p) = self.picker.take() else { return };
        let Some(name) = p.matches.get(index) else { return };
        let end = p.query.span.end;
        let tail = &self.text[end..];
        let sheet = p.query.sheets.is_some();
        let insert_space = !sheet && !tail.starts_with(char::is_whitespace);
        let before_chars = self.text[..p.query.span.start].chars().count();
        let advance_over_space = usize::from(!sheet && !insert_space);
        let replacement = if insert_space { format!("{name} ") } else { name.to_owned() };
        self.text.replace_range(p.query.span, &replacement);
        let caret = before_chars + replacement.chars().count() + advance_over_space;
        let mut state = TextEdit::load_state(ui.ctx(), Self::id()).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(caret))));
        state.store(ui.ctx(), Self::id());
        ui.memory_mut(|m| m.request_focus(Self::id()));
        self.dismissed = None;
        self.menu_rect = None;
        ui.ctx().request_repaint();
    }

    fn keys(&mut self, ui: &egui::Ui, focused: bool) -> ComposerKey {
        if !focused {
            self.ime_active = false;
            return ComposerKey::None;
        }
        // Enter belongs to the IME while a preedit is live, including its final commit frame.
        let had_preedit = self.ime_active;
        let ime_frame = ui.input(|input| {
            let mut touched = false;
            for e in &input.events {
                if let egui::Event::Ime(ev) = e {
                    touched = true;
                    match ev {
                        egui::ImeEvent::Preedit { text, .. } => self.ime_active = !text.is_empty(),
                        egui::ImeEvent::Commit(_) => self.ime_active = false,
                        _ => {}
                    }
                }
            }
            touched
        });
        if ime_frame || had_preedit || self.ime_active {
            return ComposerKey::None;
        }
        ui.input_mut(|input| {
            // Require an unmodified key. In particular, Shift+Enter is a newline and Shift+Tab
            // can always leave the composer.
            let consume = |i: &mut egui::InputState, key| {
                let mut first = false;
                i.events.retain(|event| {
                    if let egui::Event::Key { key: k, modifiers, repeat, pressed: true, .. } = event
                        && *k == key
                        && modifiers.is_none()
                    {
                        first |= !*repeat || matches!(key, Key::ArrowDown | Key::ArrowUp);
                        return false;
                    }
                    true
                });
                first
            };
            if let Some(p) = self.picker.as_mut() {
                if input.consume_key(Modifiers::SHIFT, Key::Tab) {
                    self.dismissed = Some(p.query.clone());
                    self.picker = None;
                    return ComposerKey::PreviousFocus;
                }
                if consume(input, Key::Escape) {
                    self.dismissed = Some(p.query.clone());
                    self.picker = None;
                } else if consume(input, Key::ArrowDown) {
                    p.selected = (p.selected + 1) % p.matches.len();
                } else if consume(input, Key::ArrowUp) {
                    p.selected = (p.selected + p.matches.len() - 1) % p.matches.len();
                } else if consume(input, Key::Tab) {
                    return ComposerKey::Accept;
                }
            }
            if consume(input, Key::Enter) { ComposerKey::Enter } else { ComposerKey::None }
        })
    }

    /// The caller handles slash actions locally before starting a provider turn.
    pub(super) fn show(&mut self, ui: &mut egui::Ui, enabled: bool, lang: Language, app: &SheetApp) -> Option<String> {
        let id = Self::id();
        if std::mem::take(&mut self.focus_requested) && enabled {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let focused = ui.memory(|m| m.has_focus(id));
        let key = self.keys(ui, focused && enabled);
        if key == ComposerKey::PreviousFocus {
            ui.memory_mut(|m| m.move_focus(egui::FocusDirection::Previous));
        }
        let label = ui.label(lang.tr("Message"));
        let width = ui.available_width();
        let field = egui::ScrollArea::vertical()
            .id_salt(id.with("lines"))
            .max_height(ui.spacing().interact_size.y * 5.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                TextEdit::multiline(&mut self.text)
                    .id(id)
                    .interactive(enabled)
                    .char_limit(65_536)
                    .hint_text(lang.tr("Type @ for workbook context or / for commands"))
                    .desired_width(width)
                    .desired_rows(3)
                    .event_filter(egui::EventFilter {
                        tab: self.picker.is_some(),
                        escape: self.picker.is_some(),
                        horizontal_arrows: true,
                        vertical_arrows: true,
                    })
                    .return_key(Some(egui::KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter)))
                    .show(ui)
            })
            .inner;
        let response = field.response.response.labelled_by(label.id);
        let over_menu = self.menu_rect.is_some_and(|rect| ui.rect_contains_pointer(rect));
        let cursor = field.cursor_range.or_else(|| field.state.cursor.char_range());
        if enabled && (response.has_focus() || over_menu) {
            self.refresh(cursor, app);
        } else {
            self.picker = None;
        }
        let mut submit = false;
        if let Some(p) = &self.picker {
            let exact = p.matches.get(p.selected).is_some_and(|s| self.text.get(p.query.span.clone()) == Some(s.as_str()));
            if key == ComposerKey::Accept || key == ComposerKey::Enter && (!exact || p.query.prefix == "/mode") {
                self.accept(ui, p.selected);
            } else if key == ComposerKey::Enter {
                submit = true;
            }
        } else if key == ComposerKey::Enter {
            submit = true;
        }
        self.menu(ui, lang);
        if response.has_focus() {
            // egui chooses Tab's focus target at the beginning of the *next* frame, so publish
            // the filter after processing this frame's typed '@', rather than a frame later.
            let picking = self.picker.is_some();
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(id, egui::EventFilter { tab: picking, escape: picking, horizontal_arrows: true, vertical_arrows: true })
            });
        }
        if submit { self.take_draft() } else { None }
    }

    fn menu(&mut self, ui: &mut egui::Ui, lang: Language) {
        let Some(p) = &self.picker else {
            self.menu_rect = None;
            return;
        };
        let mut clicked = None;
        let area =
            egui::ScrollArea::vertical().id_salt(Self::id().with("suggestions")).max_height(ui.spacing().interact_size.y * 4.0).show(ui, |ui| {
                for (i, name) in p.matches.iter().enumerate() {
                    let label = if p.query.sheets.is_some() { "Add an address after !" } else { hint(name) };
                    let row = ui.selectable_label(p.selected == i, format!("{name}   {}", lang.tr(label)));
                    if row.clicked() {
                        clicked = Some(i);
                    }
                    if p.selected == i {
                        ui.ctx().accesskit_node_builder(Self::id(), |n| {
                            n.set_active_descendant(row.id.accesskit_id());
                            n.set_expanded(true);
                        });
                        if ui.input(|s| s.key_down(Key::ArrowDown) || s.key_down(Key::ArrowUp)) {
                            row.scroll_to_me(None);
                        }
                    }
                }
            });
        self.menu_rect = Some(area.inner_rect);
        if let Some(i) = clicked {
            self.accept(ui, i);
        }
    }
}

#[derive(PartialEq, Eq)]
enum ComposerKey {
    None,
    Enter,
    Accept,
    PreviousFocus,
}
