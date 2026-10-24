//! A small code editor: monospace text with syntax colours, Tab to indent, Enter that keeps the
//! indentation, and a scroll area that follows the caret. It colours Python (for code in cells,
//! where `xl()` reads the sheet) and LaTeX (for equation source); reached as
//! `widgets::code_editor`. The tokenizer and the key edits are pure functions, tested below.

use std::ops::Range;

use egui::text::{CCursor, CCursorRange};
use egui::{FontFamily, FontId, Key, Response, TextEdit, Ui};

use crate::theme::{self, Syntax, Tokens};

/// The language a code editor colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Python,
    Latex,
    Plain,
}

/// A coloured span's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Keyword,
    String,
    Number,
    Comment,
    Builtin,
}

/// Spaces one Tab inserts.
pub const INDENT: &str = "    ";

const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except",
    "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "match", "case", "nonlocal", "not", "or", "pass", "raise", "return",
    "try", "while", "with", "yield",
];

const PY_BUILTINS: &[&str] = &[
    "abs",
    "all",
    "any",
    "bool",
    "dict",
    "enumerate",
    "filter",
    "float",
    "format",
    "int",
    "isinstance",
    "len",
    "list",
    "map",
    "max",
    "min",
    "print",
    "range",
    "reversed",
    "round",
    "set",
    "sorted",
    "str",
    "sum",
    "tuple",
    "type",
    "zip",
    "xl",
];

/// The coloured spans of `text` (byte ranges on char boundaries, in order, not overlapping).
/// Anything not covered is plain text. Never panics, whatever the input.
pub fn highlight(text: &str, lang: Lang) -> Vec<(Range<usize>, Token)> {
    match lang {
        Lang::Python => python(text),
        Lang::Latex => latex(text),
        Lang::Plain => Vec::new(),
    }
}

fn python(text: &str) -> Vec<(Range<usize>, Token)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let end_of = |i: usize| chars.get(i).map(|(b, _)| *b).unwrap_or(text.len());
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&(start, c)) = chars.get(i) {
        if c == '#' {
            let mut j = i;
            while chars.get(j).is_some_and(|(_, c)| *c != '\n') {
                j += 1;
            }
            out.push((start..end_of(j), Token::Comment));
            i = j;
        } else if c == '"' || c == '\'' {
            let triple = chars.get(i + 1).is_some_and(|(_, d)| *d == c) && chars.get(i + 2).is_some_and(|(_, d)| *d == c);
            let mut j = if triple { i + 3 } else { i + 1 };
            loop {
                match chars.get(j) {
                    None => break,
                    Some((_, '\\')) => j += 2,
                    Some((_, '\n')) if !triple => break,
                    Some((_, d)) if *d == c => {
                        if !triple {
                            j += 1;
                            break;
                        }
                        if chars.get(j + 1).is_some_and(|(_, d)| *d == c) && chars.get(j + 2).is_some_and(|(_, d)| *d == c) {
                            j += 3;
                            break;
                        }
                        j += 1;
                    }
                    Some(_) => j += 1,
                }
            }
            let j = j.min(chars.len());
            out.push((start..end_of(j), Token::String));
            i = j;
        } else if c.is_ascii_digit() || (c == '.' && chars.get(i + 1).is_some_and(|(_, d)| d.is_ascii_digit())) {
            let mut j = i + 1;
            while chars.get(j).is_some_and(|(_, d)| d.is_ascii_alphanumeric() || *d == '_' || *d == '.') {
                j += 1;
            }
            out.push((start..end_of(j), Token::Number));
            i = j;
        } else if c.is_alphabetic() || c == '_' {
            let mut j = i + 1;
            while chars.get(j).is_some_and(|(_, d)| d.is_alphanumeric() || *d == '_') {
                j += 1;
            }
            let word = text.get(start..end_of(j)).unwrap_or("");
            if PY_KEYWORDS.contains(&word) {
                out.push((start..end_of(j), Token::Keyword));
            } else if PY_BUILTINS.contains(&word) {
                out.push((start..end_of(j), Token::Builtin));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn latex(text: &str) -> Vec<(Range<usize>, Token)> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let end_of = |i: usize| chars.get(i).map(|(b, _)| *b).unwrap_or(text.len());
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(&(start, c)) = chars.get(i) {
        match c {
            '%' => {
                let mut j = i;
                while chars.get(j).is_some_and(|(_, c)| *c != '\n') {
                    j += 1;
                }
                out.push((start..end_of(j), Token::Comment));
                i = j;
            }
            '\\' => {
                let mut j = i + 1;
                if chars.get(j).is_some_and(|(_, d)| d.is_ascii_alphabetic()) {
                    while chars.get(j).is_some_and(|(_, d)| d.is_ascii_alphabetic()) {
                        j += 1;
                    }
                } else if chars.get(j).is_some() {
                    j += 1; // a control symbol such as \{ or \%
                }
                out.push((start..end_of(j), Token::Keyword));
                i = j;
            }
            '^' | '_' | '&' => {
                out.push((start..end_of(i + 1), Token::Builtin));
                i += 1;
            }
            d if d.is_ascii_digit() => {
                let mut j = i + 1;
                while chars.get(j).is_some_and(|(_, d)| d.is_ascii_digit() || *d == '.') {
                    j += 1;
                }
                out.push((start..end_of(j), Token::Number));
                i = j;
            }
            _ => i += 1,
        }
    }
    out
}

fn color(s: &Syntax, token: Token) -> egui::Color32 {
    match token {
        Token::Keyword => s.keyword,
        Token::String => s.string,
        Token::Number => s.number,
        Token::Comment => s.comment,
        Token::Builtin => s.builtin,
    }
}

/// The code editor's font: the monospace face at 12.5 px.
pub fn font() -> FontId {
    FontId::new(12.5, FontFamily::Name(theme::MONO.into()))
}

/// A layout job for `text` with syntax colours.
pub fn layout_job(text: &str, lang: Lang, font: &FontId, plain: egui::Color32, syntax: &Syntax, wrap_width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut pos = 0usize;
    for (range, token) in highlight(text, lang) {
        if range.start < pos {
            continue;
        }
        if let (Some(before), Some(span)) = (text.get(pos..range.start), text.get(range.clone())) {
            job.append(before, 0.0, egui::TextFormat::simple(font.clone(), plain));
            job.append(span, 0.0, egui::TextFormat::simple(font.clone(), color(syntax, token)));
            pos = range.end;
        }
    }
    job.append(text.get(pos..).unwrap_or(""), 0.0, egui::TextFormat::simple(font.clone(), plain));
    job.wrap.max_width = wrap_width;
    job
}

fn byte_of(text: &str, char_index: usize) -> usize {
    text.char_indices().nth(char_index).map(|(b, _)| b).unwrap_or(text.len())
}

/// Start (char index) of the line holding char `at`.
fn line_start(text: &str, at: usize) -> usize {
    let b = byte_of(text, at);
    text.get(..b).and_then(|s| s.rfind('\n')).map(|nl| text.get(..nl + 1).map(|s| s.chars().count()).unwrap_or(0)).unwrap_or(0)
}

/// What a key did to the text: the new caret (and selection anchor), as char indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edit {
    pub anchor: usize,
    pub caret: usize,
}

/// Enter: a newline that keeps the current line's indentation (one level more after a Python `:`).
/// `sel` is the selection as char indices (anchor, caret); it is replaced.
pub fn newline(text: &mut String, sel: (usize, usize), lang: Lang) -> Edit {
    let count = text.chars().count();
    let (a, b) = (sel.0.min(sel.1).min(count), sel.0.max(sel.1).min(count));
    let ls = line_start(text, a);
    let line_before: String = text.chars().skip(ls).take(a - ls.min(a)).collect();
    let mut indent: String = line_before.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    if lang == Lang::Python && line_before.trim_end().ends_with(':') {
        indent.push_str(INDENT);
    }
    let insert = format!("\n{indent}");
    let (ba, bb) = (byte_of(text, a), byte_of(text, b));
    text.replace_range(ba..bb, &insert);
    let caret = a + insert.chars().count();
    Edit { anchor: caret, caret }
}

/// Typed text (or a paste) replacing the selection.
pub fn insert(text: &mut String, sel: (usize, usize), s: &str) -> Edit {
    let count = text.chars().count();
    let (a, b) = (sel.0.min(sel.1).min(count), sel.0.max(sel.1).min(count));
    let (ba, bb) = (byte_of(text, a), byte_of(text, b));
    text.replace_range(ba..bb, s);
    let caret = a + s.chars().count();
    Edit { anchor: caret, caret }
}

/// Backspace (`back`) or Delete: the selection goes, or the character before or after the caret.
pub fn delete(text: &mut String, sel: (usize, usize), back: bool) -> Edit {
    let count = text.chars().count();
    let (mut a, mut b) = (sel.0.min(sel.1).min(count), sel.0.max(sel.1).min(count));
    if a == b {
        if back {
            a = a.saturating_sub(1);
        } else {
            b = (b + 1).min(count);
        }
    }
    let (ba, bb) = (byte_of(text, a), byte_of(text, b));
    text.replace_range(ba..bb, "");
    Edit { anchor: a, caret: a }
}

/// An edit the editor makes itself, read off an input event by `classify`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKey<'a> {
    Insert(&'a str),
    Enter,
    Tab { dedent: bool },
    Backspace,
    Delete,
}

/// What one input event means to the editor. See `apply_in_order`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class<'a> {
    /// The editor applies it: typed text, a paste, Enter, Tab, Shift+Tab, Backspace, Delete.
    Edit(EditKey<'a>),
    /// The editor takes it off the queue and does nothing: Tab under a command or option
    /// modifier, which the field would turn into a tab character in place of the selection.
    Dropped,
    /// The text field acts on it: a caret or selection key, a command shortcut (select all,
    /// undo, word deletion), IME, Copy, Cut, or a click, which the field reads from the pointer
    /// state.
    Field,
    /// Neither reads it from the queue: a key release, a printable key press (its character
    /// arrives as the `Text` event after it, also under Shift or Option), a pointer move, Escape
    /// (the focus system acts on it when the pass begins).
    Other,
}

fn classify(ev: &egui::Event) -> Class<'_> {
    use egui::Event;
    match ev {
        // The field treats a lone newline as the Enter key's business, and ignores it.
        Event::Text(t) if t != "\n" && t != "\r" => Class::Edit(EditKey::Insert(t)),
        Event::Text(_) => Class::Other,
        Event::Paste(t) => Class::Edit(EditKey::Insert(t)),
        Event::Key { key, pressed: true, modifiers, .. } => {
            let command = modifiers.command || modifiers.ctrl || modifiers.mac_cmd;
            match key {
                // The field makes a newline of Enter under any modifier but a command one, and
                // a tab character of Tab under every modifier; the editor takes all of those.
                Key::Enter if !command => Class::Edit(EditKey::Enter),
                Key::Tab if !command && !modifiers.alt => Class::Edit(EditKey::Tab { dedent: modifiers.shift }),
                Key::Tab => Class::Dropped,
                Key::Backspace if modifiers.is_none() => Class::Edit(EditKey::Backspace),
                Key::Delete if modifiers.is_none() => Class::Edit(EditKey::Delete),
                // The keys egui's text field reads: caret and selection moves, word and line
                // deletion, and its command shortcuts (select all, undo, redo, the Emacs keys).
                Key::Enter | Key::Backspace | Key::Delete => Class::Field,
                Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => Class::Field,
                _ if command => Class::Field,
                _ => Class::Other,
            }
        }
        Event::Copy | Event::Cut | Event::Ime(_) | Event::PointerButton { pressed: true, .. } => Class::Field,
        _ => Class::Other,
    }
}

/// Whether an event is part of the keyboard stream, whose order the editor keeps. Pointer,
/// touch, scroll and window events are not: egui folds them into its input state when the
/// pass begins, so they act in the pass they arrive in whatever their place among the keys.
fn is_keyboard(ev: &egui::Event) -> bool {
    use egui::Event;
    matches!(ev, Event::Key { .. } | Event::Text(_) | Event::Paste(_) | Event::Copy | Event::Cut | Event::Ime(_))
}

fn apply(key: EditKey<'_>, text: &mut String, sel: (usize, usize), lang: Lang) -> Edit {
    match key {
        EditKey::Insert(s) => insert(text, sel, s),
        EditKey::Enter => newline(text, sel, lang),
        EditKey::Tab { dedent } => indent(text, sel, dedent),
        EditKey::Backspace => delete(text, sel, true),
        EditKey::Delete => delete(text, sel, false),
    }
}

/// What `apply_in_order` did with a pass's events.
#[derive(Debug, Default, PartialEq)]
pub struct Applied {
    /// Whether the text or the selection changed.
    pub edited: bool,
    /// The keyboard events held back for the next pass, in order: the editor's first key after
    /// an event the text field acts on, and every keyboard event after that key.
    pub held: Vec<egui::Event>,
}

/// Applies the pass's input to `text` in the order it came. A pass can carry several events
/// (typing between repaints, key repeat), and two readers take them: this editor, for typed
/// text, pastes, Enter, Tab, Shift+Tab, Backspace and Delete, and then egui's text field, for
/// the caret and selection keys, command shortcuts, IME and clicks. The editor runs first, so
/// it can only apply its keys up to the first event the field acts on: its edits before that
/// event are applied and taken from the queue, the field's event and the run after it stay in
/// the queue for the field, and from the editor's next key on the keyboard events are held
/// back (`Applied::held`) for the next pass, where `code_editor` puts them ahead of that
/// pass's input and they go through this again. So select all, Tab in one pass indents every
/// line in two passes, and the field never gets an Enter or a Tab of a focused editor.
/// Printable key presses do not split the stream: the field reads the `Text` event that
/// follows each one, not the key. `sel` is the selection as char indices (anchor, caret),
/// updated.
pub fn apply_in_order(events: &mut Vec<egui::Event>, text: &mut String, sel: &mut (usize, usize), lang: Lang) -> Applied {
    let mut out = Applied::default();
    // An event the field acts on has gone by: the editor's keys after it wait for the next pass.
    let mut field_seen = false;
    // A key is waiting: every keyboard event after it waits too, so the order holds.
    let mut holding = false;
    let mut keep = Vec::with_capacity(events.len());
    for ev in std::mem::take(events) {
        if holding && is_keyboard(&ev) {
            out.held.push(ev);
            continue;
        }
        match classify(&ev) {
            Class::Edit(_) if field_seen => {
                holding = true;
                out.held.push(ev);
            }
            Class::Edit(key) => {
                let e = apply(key, text, *sel, lang);
                *sel = (e.anchor, e.caret);
                out.edited = true;
            }
            Class::Dropped => {}
            Class::Field => {
                field_seen = true;
                keep.push(ev);
            }
            Class::Other => keep.push(ev),
        }
    }
    *events = keep;
    out
}

/// The keyboard events a code editor held back, with the pass that held them.
#[derive(Clone, Debug, Default)]
struct Held {
    pass: u64,
    events: Vec<egui::Event>,
}

fn held_id(id: egui::Id) -> egui::Id {
    id.with("held")
}

/// The events the editor `id` held back in the previous pass, to go ahead of this pass's input
/// (see `apply_in_order`). Only the editor they were typed into gets them, only while it still
/// has the focus, and only in the pass right after: once the focus has moved on, or the editor
/// was not shown in between, they are dropped rather than typed into another widget.
fn take_held(ui: &Ui, id: egui::Id, focused: bool) -> Vec<egui::Event> {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx()
        .data_mut(|d| d.remove_temp::<Held>(held_id(id)))
        .filter(|h| focused && h.pass.checked_add(1) == Some(pass))
        .map(|h| h.events)
        .unwrap_or_default()
}

/// Holds `events` back for the editor's next pass, and asks for that pass.
fn hold(ui: &Ui, id: egui::Id, events: Vec<egui::Event>) {
    let pass = ui.ctx().cumulative_pass_nr();
    ui.ctx().data_mut(|d| d.insert_temp(held_id(id), Held { pass, events }));
    ui.ctx().request_repaint();
}

/// Tab (indent) or Shift+Tab (dedent). A selection over several lines indents or dedents each
/// line; otherwise Tab inserts four spaces at the caret.
pub fn indent(text: &mut String, sel: (usize, usize), dedent: bool) -> Edit {
    let count = text.chars().count();
    let (a, b) = (sel.0.min(sel.1).min(count), sel.0.max(sel.1).min(count));
    let multi = text.chars().skip(a).take(b - a).any(|c| c == '\n');
    if !dedent && !multi {
        let (ba, bb) = (byte_of(text, a), byte_of(text, b));
        text.replace_range(ba..bb, INDENT);
        let caret = a + INDENT.len();
        return Edit { anchor: caret, caret };
    }
    // Line starts (char indices) of every line the selection touches. A selection that ends at
    // the start of the next line (`"a\n"` in `"a\nb"`) does not touch that line.
    let mut starts = vec![line_start(text, a)];
    for (i, c) in text.chars().enumerate().skip(a).take(b - a) {
        if c == '\n' && i + 1 < b {
            starts.push(i + 1);
        }
    }
    let (mut na, mut nb) = (a, b);
    // Work from the last line back so earlier indices stay valid.
    for &s in starts.iter().rev() {
        let bs = byte_of(text, s);
        let n = if dedent {
            let n = text.get(bs..).map(|rest| rest.chars().take(INDENT.len()).take_while(|c| *c == ' ').count()).unwrap_or(0);
            text.replace_range(bs..bs + n, "");
            n
        } else {
            text.insert_str(bs, INDENT);
            0
        };
        let shift = |p: usize| {
            if dedent {
                if p > s { p - n.min(p - s) } else { p }
            } else if p >= s {
                p + INDENT.len()
            } else {
                p
            }
        };
        na = shift(na);
        nb = shift(nb);
    }
    if sel.1 >= sel.0 { Edit { anchor: na, caret: nb } } else { Edit { anchor: nb, caret: na } }
}

/// The code editor. `max_lines` rows are shown before it scrolls; the scroll area follows the
/// caret.
pub fn code_editor(ui: &mut Ui, id: egui::Id, text: &mut String, lang: Lang, max_lines: usize) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = font();
    let row_h = ui.ctx().fonts_mut(|f| f.row_height(&font));
    let max_lines = max_lines.clamp(1, 200);
    // The input the editor handles itself (Enter with indentation, Tab, Shift+Tab), applied in
    // input order with the typing around it, before the text field sees the rest; what had to
    // wait for the field in the previous pass comes first.
    let focused = ui.memory(|m| m.has_focus(id));
    let held = take_held(ui, id, focused);
    let mut edited = None;
    if focused {
        let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let count = text.chars().count();
        let mut sel = state.cursor.char_range().map(|r| (r.secondary.index.into(), r.primary.index.into())).unwrap_or((count, count));
        let applied = ui.input_mut(|i| {
            if !held.is_empty() {
                let mut events = held;
                events.append(&mut i.events);
                i.events = events;
            }
            apply_in_order(&mut i.events, text, &mut sel, lang)
        });
        if applied.edited {
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.0), CCursor::new(sel.1))));
            state.store(ui.ctx(), id);
            edited = Some(Edit { anchor: sel.0, caret: sel.1 });
        }
        if !applied.held.is_empty() {
            hold(ui, id, applied.held);
        }
    }
    let syntax = t.syntax;
    let plain = t.text;
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let job = layout_job(buf.as_str(), lang, &self::font(), plain, &syntax, wrap_width);
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let frame_h = row_h * max_lines as f32 + 8.0;
    let mut response = None;
    egui::Frame::new().fill(t.input_bg).stroke(egui::Stroke::new(1.0, t.input_border)).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(id.with("scroll")).max_height(frame_h - 8.0).auto_shrink([false, true]).show(ui, |ui| {
            let out = TextEdit::multiline(text)
                .id(id)
                .font(font.clone())
                .code_editor()
                .frame(egui::Frame::NONE)
                .desired_width(f32::INFINITY)
                .desired_rows(1)
                .lock_focus(true)
                .layouter(&mut layouter)
                .show(ui);
            if let Some(e) = edited {
                let caret = out.galley.pos_from_cursor(CCursor::new(e.caret)).translate(out.galley_pos.to_vec2());
                ui.scroll_to_rect(caret, None);
            }
            let mut r = out.response.response;
            if edited.is_some() {
                r.mark_changed();
            }
            response = Some(r);
        });
    });
    response.unwrap_or_else(|| ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Modifiers;

    fn spans(text: &str, lang: Lang) -> Vec<(&str, Token)> {
        highlight(text, lang).into_iter().map(|(r, k)| (text.get(r).unwrap_or("?"), k)).collect()
    }

    #[test]
    fn python_tokens() {
        let s = spans("def f(x):\n    return len(x) + 1.5  # done\nxl(\"A1:B2\")", Lang::Python);
        assert_eq!(
            s,
            vec![
                ("def", Token::Keyword),
                ("return", Token::Keyword),
                ("len", Token::Builtin),
                ("1.5", Token::Number),
                ("# done", Token::Comment),
                ("xl", Token::Builtin),
                ("\"A1:B2\"", Token::String),
            ]
        );
        assert_eq!(spans("s = '''a\n'b'\nc'''", Lang::Python), vec![("'''a\n'b'\nc'''", Token::String)]);
        assert_eq!(spans("x = 'it\\'s'", Lang::Python), vec![("'it\\'s'", Token::String)]);
        assert_eq!(spans("hello", Lang::Plain), vec![]);
    }

    #[test]
    fn latex_tokens() {
        let s = spans("\\frac{a^2}{3} % half\n\\{", Lang::Latex);
        assert_eq!(
            s,
            vec![
                ("\\frac", Token::Keyword),
                ("^", Token::Builtin),
                ("2", Token::Number),
                ("3", Token::Number),
                ("% half", Token::Comment),
                ("\\{", Token::Keyword),
            ]
        );
    }

    #[test]
    fn hostile_input_never_panics_and_spans_stay_on_char_boundaries() {
        let inputs = [
            "",
            "'",
            "\"",
            "'''",
            "\"\"\"x",
            "#",
            "\\",
            "%",
            "1",
            ".",
            "é'ü\\",
            "θ = 'π' # ∑",
            "\u{1F600}'\u{1F600}",
            "'\\",
            "\\\u{1F600}",
            "\u{200B}\n\t'''\n",
            "0x_é",
            "a.b.c",
            "____",
        ];
        for text in inputs {
            for lang in [Lang::Python, Lang::Latex, Lang::Plain] {
                let mut last = 0;
                for (r, _) in highlight(text, lang) {
                    assert!(r.start >= last && r.start <= r.end && r.end <= text.len(), "{text:?} {lang:?} {r:?}");
                    assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end), "{text:?} {lang:?} {r:?}");
                    last = r.end;
                }
                let job = layout_job(text, lang, &FontId::default(), egui::Color32::BLACK, &Tokens::light().syntax, f32::INFINITY);
                assert_eq!(job.text, text, "every character is laid out once");
            }
        }
    }

    #[test]
    fn enter_keeps_indentation_and_indents_after_a_colon() {
        let mut s = "    x = 1".to_string();
        let e = newline(&mut s, (9, 9), Lang::Python);
        assert_eq!(s, "    x = 1\n    ");
        assert_eq!(e.caret, 14);
        let mut s = "if x:".to_string();
        let e = newline(&mut s, (5, 5), Lang::Python);
        assert_eq!(s, "if x:\n    ");
        assert_eq!(e.caret, 10);
        // Plain text keeps the indentation but adds none after a colon.
        let mut s = "  a:".to_string();
        newline(&mut s, (4, 4), Lang::Plain);
        assert_eq!(s, "  a:\n  ");
        // A selection is replaced; out-of-range carets are clamped.
        let mut s = "abcdef".to_string();
        let e = newline(&mut s, (4, 2), Lang::Plain);
        assert_eq!((s.as_str(), e.caret), ("ab\nef", 3));
        let mut s = "é".to_string();
        let e = newline(&mut s, (99, 99), Lang::Plain);
        assert_eq!((s.as_str(), e.caret), ("é\n", 2));
    }

    #[test]
    fn shift_tab_in_the_editor_dedents_rather_than_indents() {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
            |ui, text: &mut String| {
                if ui.ctx().cumulative_pass_nr() == 0 {
                    let mut fonts = egui::FontDefinitions::default();
                    let mono = fonts.families.get(&egui::FontFamily::Monospace).cloned().unwrap_or_default();
                    fonts.families.insert(egui::FontFamily::Name(theme::MONO.into()), mono);
                    ui.ctx().set_fonts(fonts);
                    return;
                }
                let id = egui::Id::new("code");
                ui.memory_mut(|m| m.request_focus(id));
                code_editor(ui, id, text, Lang::Python, 5);
            },
            "        x".to_string(),
        );
        h.run_steps(2);
        let tab = |modifiers| egui::Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers };
        h.input_mut().events.push(tab(Modifiers::SHIFT));
        h.run_steps(1);
        assert_eq!(h.state(), "    x");
        // A plain Tab still inserts spaces at the caret (the end).
        h.input_mut().events.push(tab(Modifiers::NONE));
        h.run_steps(1);
        assert_eq!(h.state(), "    x    ");
    }

    /// Gives the context the editor's monospace family (the test has no craft fonts).
    fn install_mono(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let mono = fonts.families.get(&egui::FontFamily::Monospace).cloned().unwrap_or_default();
        fonts.families.insert(egui::FontFamily::Name(theme::MONO.into()), mono);
        ctx.set_fonts(fonts);
    }

    fn editor_harness(text: &str) -> egui_kittest::Harness<'static, String> {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
            |ui, text: &mut String| {
                if ui.ctx().cumulative_pass_nr() == 0 {
                    install_mono(ui.ctx());
                    return;
                }
                let id = egui::Id::new("code");
                ui.memory_mut(|m| m.request_focus(id));
                code_editor(ui, id, text, Lang::Python, 5);
            },
            text.to_string(),
        );
        h.run_steps(2);
        h
    }

    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    #[test]
    fn batched_input_applies_in_order_and_keeps_every_key() {
        // Events gather between repaints (typing, key repeat). The text typed before an Enter
        // comes first, and two Enters make two lines.
        let mut h = editor_harness("");
        h.input_mut().events.extend([egui::Event::Text("if x:".into()), key(Key::Enter, Modifiers::NONE), key(Key::Enter, Modifiers::NONE)]);
        h.run_steps(1);
        assert_eq!(h.state(), "if x:\n    \n    ");
        // Backspace and Tab in the same frame as the typing around them.
        h.input_mut().events.extend([
            egui::Event::Text("y = 1".into()),
            key(Key::Backspace, Modifiers::NONE),
            egui::Event::Text("2".into()),
            key(Key::Tab, Modifiers::NONE),
        ]);
        h.run_steps(1);
        assert_eq!(h.state(), "if x:\n    \n    y = 2    ");
        // An event the field acts on (an arrow) keeps the order too: the text typed before it
        // is applied first, the field moves the caret, and the Enter after it waits for the
        // next pass, where the editor applies it at the moved caret, with the indentation.
        h.input_mut().events.extend([egui::Event::Text("a".into()), key(Key::ArrowLeft, Modifiers::NONE), key(Key::Enter, Modifiers::NONE)]);
        h.run_steps(1);
        assert_eq!(h.state(), "if x:\n    \n    y = 2    a", "the Enter waits for the arrow");
        h.run_steps(1);
        assert_eq!(h.state(), "if x:\n    \n    y = 2    \n    a");
    }

    #[test]
    fn a_selection_ending_at_a_line_start_leaves_that_line_alone() {
        // Selecting "a\n" in "a\nb" ends at the start of the second line, which is not selected.
        let mut s = "a\nb".to_string();
        indent(&mut s, (0, 2), false);
        assert_eq!(s, "    a\nb");
        let mut s = "    a\n    b".to_string();
        indent(&mut s, (0, 6), true);
        assert_eq!(s, "a\n    b");
        // One character into the second line, both lines are touched.
        let mut s = "a\nb".to_string();
        indent(&mut s, (0, 3), false);
        assert_eq!(s, "    a\n    b");
    }

    #[test]
    fn typed_text_and_deletions_replace_the_selection() {
        let mut s = "abcd".to_string();
        assert_eq!(insert(&mut s, (3, 1), "X"), Edit { anchor: 2, caret: 2 });
        assert_eq!(s, "aXd");
        assert_eq!(delete(&mut s, (2, 2), true), Edit { anchor: 1, caret: 1 });
        assert_eq!(s, "ad");
        assert_eq!(delete(&mut s, (1, 1), false), Edit { anchor: 1, caret: 1 });
        assert_eq!(s, "a");
        assert_eq!(delete(&mut s, (0, 0), true), Edit { anchor: 0, caret: 0 });
        assert_eq!(delete(&mut s, (9, 9), false), Edit { anchor: 1, caret: 1 });
        assert_eq!(s, "a");
        let mut s = "éa".to_string();
        delete(&mut s, (2, 0), true);
        assert_eq!(s, "");
        // The queue keeps what the editor does not handle, and everything after it.
        let mut events = vec![
            egui::Event::Text("x".into()),
            egui::Event::Text("\n".into()),
            key(Key::ArrowLeft, Modifiers::NONE),
            key(Key::Enter, Modifiers::NONE),
        ];
        let (mut s, mut sel) = (String::new(), (0, 0));
        let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
        assert!(out.edited);
        assert_eq!((s.as_str(), sel), ("x", (1, 1)));
        assert_eq!(events.len(), 2, "the lone newline and the arrow stay for the field: {events:?}");
        assert_eq!(out.held, vec![key(Key::Enter, Modifiers::NONE)], "the Enter after the arrow waits for the next pass");
        let mut events = vec![egui::Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }];
        let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
        assert!(!out.edited && out.held.is_empty(), "a key release is nothing to apply");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn select_all_then_tab_in_one_pass_indents_every_line() {
        // Cmd+A is the field's, Tab the editor's: the Tab waits for the pass after the
        // selection and indents the selected lines, rather than reaching the field, which would
        // replace the selection with a tab character.
        let mut h = editor_harness("a\nb");
        h.input_mut().events.extend([key(Key::A, Modifiers::COMMAND), key(Key::Tab, Modifiers::NONE)]);
        h.run_steps(1);
        assert_eq!(h.state(), "a\nb", "the Tab waits for the selection");
        h.run_steps(1);
        assert_eq!(h.state(), "    a\n    b");
        h.input_mut().events.extend([key(Key::A, Modifiers::COMMAND), key(Key::Tab, Modifiers::SHIFT)]);
        h.run_steps(2);
        assert_eq!(h.state(), "a\nb");
    }

    #[test]
    fn a_keystroke_sequence_as_the_platform_sends_it_keeps_the_editors_enter() {
        // egui-winit sends a Key event before each Text event. Those presses are nobody's to
        // read, so they do not hand the Enter to the field, which knows no indentation.
        let mut h = editor_harness("");
        let typed = |k, c: &str, m| [key(k, m), egui::Event::Text(c.into())];
        let mut events = Vec::new();
        events.extend(typed(Key::I, "i", Modifiers::NONE));
        events.extend(typed(Key::F, "f", Modifiers::NONE));
        events.extend(typed(Key::Space, " ", Modifiers::NONE));
        events.extend(typed(Key::X, "x", Modifiers::NONE));
        events.extend(typed(Key::Colon, ":", Modifiers::SHIFT));
        events.push(key(Key::Enter, Modifiers::NONE));
        h.input_mut().events.extend(events);
        h.run_steps(1);
        assert_eq!(h.state(), "if x:\n    ");
        // A character typed with Option (macOS) comes as a key under that modifier and its text;
        // the field reads neither the key nor Option, so the Enter after it is still the editor's.
        h.input_mut().events.extend([key(Key::E, Modifiers::ALT), egui::Event::Text("\u{e9}".into()), key(Key::Enter, Modifiers::NONE)]);
        h.run_steps(1);
        assert_eq!(h.state(), "if x:\n    \u{e9}\n    ");
    }

    #[test]
    fn tab_and_enter_under_modifiers_never_reach_the_field() {
        // The field would make a tab character of Ctrl+Tab (in place of the selection) and a
        // newline of Shift+Enter; Cmd+Enter is nobody's here and stays for a shortcut.
        let mut events = vec![key(Key::Tab, Modifiers::CTRL), key(Key::Enter, Modifiers::SHIFT), key(Key::Enter, Modifiers::COMMAND)];
        let (mut s, mut sel) = ("if x:".to_string(), (5, 5));
        let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
        assert_eq!(s, "if x:\n    ");
        assert_eq!(events, vec![key(Key::Enter, Modifiers::COMMAND)]);
        assert!(out.held.is_empty());
    }

    #[test]
    fn events_after_a_held_key_wait_with_it_and_pointer_events_do_not() {
        let mut events = vec![
            key(Key::X, Modifiers::NONE),
            egui::Event::Text("x".into()),
            key(Key::A, Modifiers::COMMAND),
            key(Key::Tab, Modifiers::NONE),
            egui::Event::PointerMoved(egui::Pos2::ZERO),
            key(Key::ArrowLeft, Modifiers::NONE),
            egui::Event::Text("y".into()),
        ];
        let (mut s, mut sel) = (String::new(), (0, 0));
        let out = apply_in_order(&mut events, &mut s, &mut sel, Lang::Python);
        assert_eq!((s.as_str(), sel), ("x", (1, 1)), "the typing before the select-all is applied");
        assert_eq!(events, vec![key(Key::X, Modifiers::NONE), key(Key::A, Modifiers::COMMAND), egui::Event::PointerMoved(egui::Pos2::ZERO)]);
        assert_eq!(out.held, vec![key(Key::Tab, Modifiers::NONE), key(Key::ArrowLeft, Modifiers::NONE), egui::Event::Text("y".into())]);
    }

    #[test]
    fn held_back_events_land_in_the_next_pass_only() {
        let mut h = editor_harness("");
        let id = egui::Id::new("code");
        let held = |pass, c: &str| Held { pass, events: vec![egui::Event::Text(c.into())] };
        // What the pass just completed held back comes in with the next one.
        let last = h.ctx.cumulative_pass_nr() - 1;
        h.ctx.data_mut(|d| d.insert_temp(held_id(id), held(last, "a")));
        h.run_steps(1);
        assert_eq!(h.state(), "a");
        // Anything older is dropped rather than typed out of context, and cleared.
        h.ctx.data_mut(|d| d.insert_temp(held_id(id), held(last, "b")));
        h.run_steps(1);
        assert_eq!(h.state(), "a");
        assert!(h.ctx.data(|d| d.get_temp::<Held>(held_id(id))).is_none());
    }

    #[test]
    fn held_back_events_go_nowhere_once_the_editor_lost_the_focus() {
        // Typed into the editor, they must not land in whatever has the focus next.
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
            |ui, s: &mut (String, String, bool)| {
                if ui.ctx().cumulative_pass_nr() == 0 {
                    install_mono(ui.ctx());
                    return;
                }
                let (code, other) = (egui::Id::new("code"), egui::Id::new("other"));
                ui.memory_mut(|m| m.request_focus(if s.2 { code } else { other }));
                code_editor(ui, code, &mut s.0, Lang::Python, 5);
                ui.add(TextEdit::singleline(&mut s.1).id(other));
            },
            (String::new(), String::new(), true),
        );
        h.run_steps(2);
        h.input_mut().events.extend([key(Key::ArrowLeft, Modifiers::NONE), egui::Event::Text("x".into())]);
        h.run_steps(1);
        assert_eq!(h.state().0, "", "the x waits for the arrow");
        h.state_mut().2 = false;
        h.run_steps(2);
        assert_eq!((h.state().0.as_str(), h.state().1.as_str()), ("", ""));
    }

    #[test]
    fn a_repeat_pass_of_the_frame_counts_as_the_next_pass() {
        // When a widget asks egui to discard and repeat the pass, the repeat pass gets the
        // held-back keys, with the selection the field made in the first pass.
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(400.0, 300.0)).build_ui_state(
            |ui, s: &mut (String, bool)| {
                if ui.ctx().cumulative_pass_nr() == 0 {
                    install_mono(ui.ctx());
                    return;
                }
                let id = egui::Id::new("code");
                ui.memory_mut(|m| m.request_focus(id));
                code_editor(ui, id, &mut s.0, Lang::Python, 5);
                if std::mem::take(&mut s.1) {
                    ui.ctx().request_discard("the test repeats this pass");
                }
            },
            ("a\nb".to_string(), false),
        );
        h.run_steps(2);
        h.state_mut().1 = true;
        h.input_mut().events.extend([key(Key::A, Modifiers::COMMAND), key(Key::Tab, Modifiers::NONE)]);
        h.run_steps(1);
        assert_eq!(h.state().0, "    a\n    b");
    }
}
