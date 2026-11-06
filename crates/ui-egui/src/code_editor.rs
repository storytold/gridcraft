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

/// End (exclusive, in chars) of the Python string literal opening with quote `c` at char `i`.
fn python_string_end(chars: &[(usize, char)], i: usize, c: char) -> usize {
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
    j.min(chars.len())
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
            let j = python_string_end(&chars, i, c);
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

/// Underlines the byte range `mark` of `job` in `color` (an error's position), splitting the
/// sections it crosses. An empty range marks the character before it (the end of the text, for
/// an error about missing input). A range past the end of the text is clamped to it, and its
/// ends are moved out to char boundaries: the mark may come from an earlier version of the text
/// (the equation editor computes it before this frame's edit), and egui slices the text at every
/// section boundary.
pub fn underline(job: &mut egui::text::LayoutJob, mark: Range<usize>, color: egui::Color32) {
    let text = job.text.as_str();
    let floor = |i: usize| (0..=i.min(text.len())).rev().find(|&b| text.is_char_boundary(b)).unwrap_or(0);
    let ceil = |i: usize| (i.min(text.len())..=text.len()).find(|&b| text.is_char_boundary(b)).unwrap_or(text.len());
    let start = floor(mark.start);
    let mut mark = start..ceil(mark.end).max(start);
    if mark.is_empty() {
        let start = text.get(..mark.start).and_then(|t| t.char_indices().next_back()).map_or(0, |(i, _)| i);
        mark = start..mark.start.max(start + text.get(start..).and_then(|t| t.chars().next()).map_or(0, char::len_utf8));
    }
    let mut out = Vec::with_capacity(job.sections.len() + 2);
    for sec in job.sections.drain(..) {
        let r = sec.byte_range.start.0..sec.byte_range.end.0;
        let (a, b) = (mark.start.clamp(r.start, r.end), mark.end.clamp(r.start, r.end));
        for (piece, marked) in [(r.start..a, false), (a..b, true), (b..r.end, false)] {
            if piece.is_empty() {
                continue;
            }
            let mut s = sec.clone();
            s.byte_range = egui::text::ByteIndex(piece.start)..egui::text::ByteIndex(piece.end);
            if marked {
                s.format.underline = egui::Stroke::new(1.5, color);
                s.format.color = color;
            }
            if s.byte_range.start.0 != r.start {
                s.leading_space = 0.0;
            }
            out.push(s);
        }
    }
    job.sections = out;
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
        // Pasted text keeps its platform's line breaks (`\r\n` on Windows); code has `\n` only, as
        // a cell's code is stored, and a lone `\r` has no glyph.
        EditKey::Insert(s) if s.contains('\r') => insert(text, sel, &s.replace("\r\n", "\n").replace('\r', "\n")),
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
    /// The caller's key the pass stopped at (see [`apply_in_order_until`]).
    pub stop: Option<egui::Event>,
    /// An edit was refused: it would have made text the caller does not accept (see
    /// [`apply_in_order_within`]).
    pub refused: bool,
}

/// Whether the caller accepts an edit: the text after it, then the text before it.
pub type Accept<'a> = &'a dyn Fn(&str, &str) -> bool;

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
    apply_in_order_until(events, text, sel, lang, &|_| false)
}

/// [`apply_in_order`], stopping at the first event `stop` picks: a key the caller acts on itself
/// (commit, cancel, a mode switch). It counts as one of the editor's keys for the order: the
/// typing before it is applied first, and after an event the text field acts on it is held back
/// for the next pass with the rest. Once the pass stops (`Applied::stop`), the keyboard events
/// after the stop key are dropped: the caller acts on the text as it was when the key was
/// pressed, and what followed it was typed after it.
pub fn apply_in_order_until(
    events: &mut Vec<egui::Event>,
    text: &mut String,
    sel: &mut (usize, usize),
    lang: Lang,
    stop: &dyn Fn(&egui::Event) -> bool,
) -> Applied {
    apply_in_order_within(events, text, sel, lang, stop, None)
}

/// [`apply_in_order_until`], where each of the editor's edits is checked with `accept` (the text
/// after it, the text before it): an edit it refuses is undone at once, so the text and the
/// selection stay as they were, and the pass goes on with the next event (`Applied::refused`).
/// Typing and pasting past a limit are refused this way, one edit at a time.
pub fn apply_in_order_within(
    events: &mut Vec<egui::Event>,
    text: &mut String,
    sel: &mut (usize, usize),
    lang: Lang,
    stop: &dyn Fn(&egui::Event) -> bool,
    accept: Option<Accept<'_>>,
) -> Applied {
    let mut out = Applied::default();
    // An event the field acts on has gone by: the editor's keys after it wait for the next pass.
    let mut field_seen = false;
    // A key is waiting: every keyboard event after it waits too, so the order holds.
    let mut holding = false;
    let mut keep = Vec::with_capacity(events.len());
    for ev in std::mem::take(events) {
        if out.stop.is_some() {
            if !is_keyboard(&ev) {
                keep.push(ev);
            }
            continue;
        }
        if holding && is_keyboard(&ev) {
            out.held.push(ev);
            continue;
        }
        if is_keyboard(&ev) && stop(&ev) {
            if field_seen {
                holding = true;
                out.held.push(ev);
            } else {
                out.stop = Some(ev);
            }
            continue;
        }
        match classify(&ev) {
            Class::Edit(_) if field_seen => {
                holding = true;
                out.held.push(ev);
            }
            Class::Edit(key) => {
                let before = accept.map(|_| text.clone());
                let e = apply(key, text, *sel, lang);
                match (accept, before) {
                    (Some(ok), Some(before)) if !ok(text, &before) => {
                        *text = before;
                        out.refused = true;
                    }
                    _ => {
                        *sel = (e.anchor, e.caret);
                        out.edited = true;
                    }
                }
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
    code_editor_marked(ui, id, text, lang, max_lines, None)
}

/// [`code_editor`] with the byte range `mark` underlined in the error colour (see [`underline`]).
pub fn code_editor_marked(ui: &mut Ui, id: egui::Id, text: &mut String, lang: Lang, max_lines: usize, mark: Option<Range<usize>>) -> Response {
    let t = Tokens::get(ui.ctx());
    let row_h = ui.ctx().fonts_mut(|f| f.row_height(&font()));
    let max_lines = max_lines.clamp(1, 200);
    let mut response = None;
    egui::Frame::new().fill(t.input_bg).stroke(egui::Stroke::new(1.0, t.input_border)).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
        let options = FieldOptions { max_height: row_h * max_lines as f32, mark: mark.clone(), ..FieldOptions::new(lang) };
        response = Some(code_field(ui, id, text, &options).response);
    });
    response.unwrap_or_else(|| ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover()))
}

/// How serious a mark in the code is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkLevel {
    Error,
    Warning,
    Info,
}

/// A place in the code to point at: a wavy underline on the span, with a tooltip, and a mark beside
/// its line in the gutter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    /// Char indices into the text. A mark worked out before this frame's edit lands a little off
    /// for one frame, and can't split a character.
    pub chars: Range<usize>,
    /// The 1-based line of its start.
    pub line: u32,
    pub level: MarkLevel,
    pub tip: String,
}

fn mark_color(level: MarkLevel, t: &Tokens) -> egui::Color32 {
    match level {
        MarkLevel::Error => t.danger,
        MarkLevel::Warning => t.warning,
        MarkLevel::Info => t.text_dim,
    }
}

/// Paints the wavy underline of `mark`, on each row it crosses, when it is in view.
fn paint_mark(painter: &egui::Painter, galley: &egui::Galley, galley_pos: egui::Pos2, clip: egui::Rect, mark: &Mark, color: egui::Color32) {
    let end = mark.chars.end.max(mark.chars.start.saturating_add(1));
    let first = galley.pos_from_cursor(CCursor::new(mark.chars.start)).translate(galley_pos.to_vec2());
    let last = galley.pos_from_cursor(CCursor::new(end)).translate(galley_pos.to_vec2());
    if last.bottom() < clip.top() || first.top() > clip.bottom() {
        return;
    }
    if (first.top() - last.top()).abs() < 1.0 {
        paint_wave(painter, first.left(), last.right(), first.bottom(), color);
        return;
    }
    for row in &galley.rows {
        let rect = row.rect().translate(galley_pos.to_vec2());
        if rect.bottom() < first.top() || rect.top() > last.bottom() {
            continue;
        }
        let from = if rect.top() <= first.top() { first.left() } else { rect.left() };
        let to = if rect.bottom() >= last.bottom() { last.right() } else { rect.right() };
        paint_wave(painter, from, to, rect.bottom(), color);
    }
}

/// A wavy line from `from` to `to`, its peaks at `y`.
fn paint_wave(painter: &egui::Painter, from: f32, to: f32, y: f32, color: egui::Color32) {
    const STEP: f32 = 2.5;
    let to = to.max(from + STEP * 2.0);
    let steps = ((to - from) / STEP).ceil() as usize;
    let points = (0..=steps)
        .map(|i| {
            let x = (from + i as f32 * STEP).min(to);
            egui::pos2(x, if i % 2 == 0 { y } else { y - 2.0 })
        })
        .collect();
    painter.add(egui::epaint::PathShape::line(points, egui::epaint::PathStroke::new(1.0, color)));
}

/// The mark under `pos` in the text, if any.
fn mark_at<'a>(marks: &'a [Mark], galley: &egui::Galley, galley_pos: egui::Pos2, pos: egui::Pos2) -> Option<&'a Mark> {
    if marks.is_empty() {
        return None;
    }
    let at: usize = galley.cursor_from_pos(pos - galley_pos).index.into();
    marks.iter().find(|m| m.chars.contains(&at))
}

/// A mark beside each line that has one, the worst kind first, with its tips on hover. Only the rows
/// in view.
fn paint_gutter_marks(ui: &Ui, id: egui::Id, laid: (&egui::Galley, egui::Pos2), clip: egui::Rect, left: f32, marks: &[Mark], t: &Tokens) {
    let (galley, galley_pos) = laid;
    if marks.is_empty() {
        return;
    }
    let painter = ui.painter();
    for_each_visible_line(galley, galley_pos, clip, |line, rect| {
        let here: Vec<&Mark> = marks.iter().filter(|m| m.line == u32::try_from(line).unwrap_or(u32::MAX)).collect();
        if here.is_empty() {
            return;
        }
        let tips: Vec<&str> = here.iter().map(|m| m.tip.as_str()).collect();
        let worst = here.iter().map(|m| m.level).fold(MarkLevel::Info, |a, b| match (a, b) {
            (MarkLevel::Error, _) | (_, MarkLevel::Error) => MarkLevel::Error,
            (MarkLevel::Warning, _) | (_, MarkLevel::Warning) => MarkLevel::Warning,
            _ => MarkLevel::Info,
        });
        let center = egui::pos2(left + MARK_ROOM / 2.0, rect.center().y);
        let color = mark_color(worst, t);
        match worst {
            MarkLevel::Error => {
                painter.circle_filled(center, 3.5, color);
            }
            MarkLevel::Warning => {
                let points = vec![center + egui::vec2(0.0, -4.0), center + egui::vec2(4.0, 3.5), center + egui::vec2(-4.0, 3.5)];
                painter.add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
            }
            MarkLevel::Info => {
                painter.circle_stroke(center, 3.0, egui::Stroke::new(1.0, color));
            }
        }
        let area = egui::Rect::from_center_size(center, egui::vec2(MARK_ROOM, rect.height()));
        let hover = ui.interact(area, id.with(("gutter_mark", line)), egui::Sense::hover());
        hover.on_hover_text(tips.join("\n"));
    });
}

/// How a [`code_field`] is laid out, and the keys its caller takes.
pub struct FieldOptions<'a> {
    pub lang: Lang,
    /// The field's width (`f32::INFINITY`: all there is), its line-number gutter included.
    pub width: f32,
    /// The height shown before it scrolls; the scroll area follows the caret.
    pub max_height: f32,
    /// The field is `max_height` tall even when its text is shorter (a click below the text still
    /// lands in it).
    pub fill: bool,
    /// A line-number gutter at the left, scrolled with the text (see [`gutter_width`]).
    pub gutter: bool,
    /// Whether it takes input. A dialog or message box over the field has the keyboard.
    pub interactive: bool,
    /// Keys the caller acts on itself, in input order (see [`apply_in_order_until`]).
    pub stop: &'a dyn Fn(&egui::Event) -> bool,
    /// Edits the caller accepts (the text after, the text before); the others are refused and
    /// undone, typing and pasting alike (see [`apply_in_order_within`]). `None`: every edit.
    pub accept: Option<Accept<'a>>,
    /// Esc stays with the field (for a caller that stops at it) instead of releasing the focus.
    pub keep_escape: bool,
    /// Puts the caret here (a char index) before the pass's input: the caller changed the text.
    pub caret: Option<usize>,
    /// The field's name for screen readers.
    pub name: &'a str,
    /// A byte range underlined in the error colour (see [`underline`]).
    pub mark: Option<Range<usize>>,
    /// Places to point at in the code, each with a wavy underline and a tooltip, and in a gutter a
    /// mark beside its line.
    pub marks: &'a [Mark],
}

impl FieldOptions<'_> {
    pub fn new(lang: Lang) -> FieldOptions<'static> {
        FieldOptions {
            lang,
            width: f32::INFINITY,
            max_height: 100.0,
            fill: false,
            gutter: false,
            interactive: true,
            stop: &|_| false,
            accept: None,
            keep_escape: false,
            caret: None,
            // The editor is a generic code field, so the name is not part of the UI's translated
            // strings unless the caller gives one.
            name: "Code Editor",
            mark: None,
            marks: &[],
        }
    }
}

/// What a [`code_field`] laid out and did.
pub struct FieldOutput {
    pub response: Response,
    /// The caller's key the pass stopped at, if any.
    pub stop: Option<egui::Event>,
    /// Laid-out rows of the text.
    pub lines: usize,
    /// The caret on screen, while the field has the focus.
    pub caret: Option<egui::Rect>,
    /// The caret and the other end of the selection (char indices).
    pub selection: (usize, usize),
    /// The line-number gutter's width (0 without one).
    pub gutter: f32,
    /// An edit of this pass was refused by [`FieldOptions::accept`] and undone.
    pub refused: bool,
}

/// The code editor's text field without a frame: monospace text with syntax colours, Enter that
/// keeps the indentation, Tab and Shift+Tab, applied in input order (see [`apply_in_order`]).
pub fn code_field(ui: &mut Ui, id: egui::Id, text: &mut String, options: &FieldOptions<'_>) -> FieldOutput {
    let t = Tokens::get(ui.ctx());
    let font = font();
    let lang = options.lang;
    // The input the editor handles itself (Enter with indentation, Tab, Shift+Tab), applied in
    // input order with the typing around it, before the text field sees the rest; what had to
    // wait for the field in the previous pass comes first.
    let focused = options.interactive && ui.memory(|m| m.has_focus(id));
    let held = take_held(ui, id, focused);
    let mut edited = None;
    let mut stop = None;
    let mut refused = false;
    let count = text.chars().count();
    if let Some(caret) = options.caret {
        let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let caret = caret.min(count);
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(caret))));
        state.store(ui.ctx(), id);
        edited = Some(Edit { anchor: caret, caret });
    }
    // The text and selection the text field starts from, for undoing an edit of its own (an IME
    // commit, undo, redo) that the caller refuses.
    let mut field_start = None;
    if focused {
        let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        let mut sel = state.cursor.char_range().map(|r| (r.secondary.index.into(), r.primary.index.into())).unwrap_or((count, count));
        let applied = ui.input_mut(|i| {
            if !held.is_empty() {
                let mut events = held;
                events.append(&mut i.events);
                i.events = events;
            }
            apply_in_order_within(&mut i.events, text, &mut sel, lang, options.stop, options.accept)
        });
        if applied.edited {
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.0), CCursor::new(sel.1))));
            state.store(ui.ctx(), id);
            edited = Some(Edit { anchor: sel.0, caret: sel.1 });
        }
        if !applied.held.is_empty() {
            hold(ui, id, applied.held);
        }
        stop = applied.stop;
        refused = applied.refused;
        if options.accept.is_some() {
            field_start = Some((text.clone(), sel));
        }
    }
    let syntax = t.syntax;
    let plain = t.text;
    let danger = t.danger;
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = layout_job(buf.as_str(), lang, &self::font(), plain, &syntax, wrap_width);
        if let Some(m) = options.mark.clone() {
            underline(&mut job, m, danger);
        }
        ui.fonts_mut(|f| f.layout_job(job))
    };
    let filter = egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: options.keep_escape };
    let gutter = if options.gutter { gutter_width(ui, line_count(text)) } else { 0.0 };
    let max_height = options.max_height.max(1.0);
    let scrolled =
        egui::ScrollArea::vertical().id_salt(id.with("scroll")).max_height(max_height).auto_shrink([false, !options.fill]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let gutter_left = ui.cursor().left();
                if gutter > 0.0 {
                    ui.add_space(gutter);
                }
                let mut edit = TextEdit::multiline(text)
                    .id(id)
                    .font(font.clone())
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .desired_width((options.width - gutter).max(1.0))
                    .desired_rows(1)
                    .interactive(options.interactive)
                    .event_filter(filter)
                    .layouter(&mut layouter);
                if options.fill {
                    edit = edit.min_size(egui::vec2(0.0, max_height));
                }
                let out = edit.show(ui);
                if let Some(e) = edited {
                    let caret = out.galley.pos_from_cursor(CCursor::new(e.caret)).translate(out.galley_pos.to_vec2());
                    ui.scroll_to_rect(caret, None);
                }
                let clip = ui.clip_rect();
                for mark in options.marks {
                    paint_mark(ui.painter(), &out.galley, out.galley_pos, clip, mark, mark_color(mark.level, &t));
                }
                if gutter > 0.0 {
                    let right = gutter_left + gutter - GUTTER_GAP;
                    let rule = egui::Stroke::new(1.0, t.separator);
                    ui.painter().vline(right + GUTTER_GAP / 2.0, clip.y_range(), rule);
                    paint_line_numbers(ui.painter(), &out.galley, out.galley_pos, clip, right, &font, t.text_dim);
                    paint_gutter_marks(ui, id, (&out.galley, out.galley_pos), clip, gutter_left, options.marks, &t);
                }
                out
            })
            .inner
        });
    let out = scrolled.inner;
    // The text field's own edits go through the same check as the editor's.
    if let (Some(accept), Some((before, sel))) = (options.accept, field_start)
        && *text != before
        && !accept(text, &before)
    {
        *text = before;
        let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(sel.0), CCursor::new(sel.1))));
        state.store(ui.ctx(), id);
        refused = true;
        ui.ctx().request_repaint();
    }
    // The field has no visible label; name it for screen readers and the strict kittest
    // accessibility check.
    let mut response = out.response.response.accessible_name(options.name);
    if edited.is_some() {
        response.mark_changed();
    }
    if let Some(mark) = response.hover_pos().and_then(|pos| mark_at(options.marks, &out.galley, out.galley_pos, pos)) {
        response = response.on_hover_text_at_pointer(mark.tip.as_str());
    }
    let selection = out.cursor_range.map(|r| (r.primary.index.into(), r.secondary.index.into())).unwrap_or((count, count));
    let caret = out
        .cursor_range
        .map(|r| out.galley.pos_from_cursor(r.primary).translate(out.galley_pos.to_vec2()))
        .filter(|_| response.has_focus())
        .map(|r| r.intersect(scrolled.inner_rect.expand(1.0)));
    FieldOutput { response, stop, lines: out.galley.rows.len().max(1), caret, selection, gutter, refused }
}

/// Space between the line numbers and the code, with the gutter's rule in the middle.
const GUTTER_GAP: f32 = 10.0;
/// Space in the gutter for the marks beside the line numbers.
const MARK_ROOM: f32 = 11.0;

/// Lines of `text`: one more than its line breaks.
pub fn line_count(text: &str) -> usize {
    text.bytes().filter(|b| *b == b'\n').count().saturating_add(1)
}

/// Digits in the number of the last of `lines` lines (at least one).
pub fn gutter_digits(lines: usize) -> usize {
    lines.max(1).checked_ilog10().map_or(1, |d| d as usize + 1)
}

/// The width of a line-number gutter for `lines` lines: as many digits of the code font as the
/// last line's number has, room for a mark, and the gap before the code. It grows when the number
/// of lines gains a digit (10, 100, 1000).
pub fn gutter_width(ui: &Ui, lines: usize) -> f32 {
    let digit = ui.fonts_mut(|f| f.glyph_width(&font(), '0'));
    digit * gutter_digits(lines) as f32 + MARK_ROOM + GUTTER_GAP
}

/// Paints line numbers right-aligned at `right`, beside `galley` laid out at `galley_pos`: one per
/// line of the text, on the first row of a wrapped line. Only rows inside `clip` are painted, so
/// the work for long code is the rows in view (and a count of the line breaks above them).
pub fn paint_line_numbers(
    painter: &egui::Painter,
    galley: &egui::Galley,
    galley_pos: egui::Pos2,
    clip: egui::Rect,
    right: f32,
    font: &FontId,
    color: egui::Color32,
) {
    for_each_visible_line(galley, galley_pos, clip, |line, rect| {
        painter.text(egui::pos2(right, rect.top()), egui::Align2::RIGHT_TOP, line.to_string(), font.clone(), color);
    });
}

/// Calls `f` with the 1-based number and the first row's rect of each line of `galley` in view. The
/// work is the rows in view, and a count of the line breaks above them.
fn for_each_visible_line(galley: &egui::Galley, galley_pos: egui::Pos2, clip: egui::Rect, mut f: impl FnMut(usize, egui::Rect)) {
    // Rows are in order from the top: skip the ones above the view without laying anything out.
    let first = galley.rows.partition_point(|row| galley_pos.y + row.rect().max.y < clip.top());
    let mut line = galley.rows.iter().take(first).filter(|row| row.ends_with_newline).count().saturating_add(1);
    let mut starts_line = first.checked_sub(1).and_then(|i| galley.rows.get(i)).is_none_or(|row| row.ends_with_newline);
    for row in galley.rows.iter().skip(first) {
        let rect = row.rect().translate(galley_pos.to_vec2());
        if rect.top() > clip.bottom() {
            break;
        }
        if starts_line {
            f(line, rect);
            line = line.saturating_add(1);
        }
        starts_line = row.ends_with_newline;
    }
}

#[cfg(test)]
mod tests;
