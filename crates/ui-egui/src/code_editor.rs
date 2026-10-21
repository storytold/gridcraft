//! A small code editor: monospace text with syntax colours, Tab to indent, Enter that keeps the
//! indentation, and a scroll area that follows the caret. It colours Python (for code in cells,
//! where `xl()` reads the sheet) and LaTeX (for equation source); reached as
//! `widgets::code_editor`. The tokenizer and the key edits are pure functions, tested below.

use std::ops::Range;

use egui::text::{CCursor, CCursorRange};
use egui::{FontFamily, FontId, Key, Modifiers, Response, TextEdit, Ui};

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
    // Line starts (char indices) of every line the selection touches.
    let mut starts = vec![line_start(text, a)];
    for (i, c) in text.chars().enumerate().skip(a).take(b - a) {
        if c == '\n' && i < b {
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
    // Keys the editor handles itself, before the TextEdit sees them.
    let focused = ui.memory(|m| m.has_focus(id));
    let mut edited = None;
    if focused {
        // Shift+Tab first: a plain Tab pattern matches a shifted Tab too.
        let (tab, shift_tab, enter) = ui.input_mut(|i| {
            let shift_tab = i.consume_key(Modifiers::SHIFT, Key::Tab);
            (i.consume_key(Modifiers::NONE, Key::Tab), shift_tab, i.consume_key(Modifiers::NONE, Key::Enter))
        });
        if tab || shift_tab || enter {
            let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            let count = text.chars().count();
            let sel = state.cursor.char_range().map(|r| (r.secondary.index.into(), r.primary.index.into())).unwrap_or((count, count));
            let e = if enter { newline(text, sel, lang) } else { indent(text, sel, shift_tab) };
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(e.anchor), CCursor::new(e.caret))));
            state.store(ui.ctx(), id);
            edited = Some(e);
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

    #[test]
    fn tab_inserts_spaces_and_shift_tab_dedents() {
        let mut s = "ab".to_string();
        let e = indent(&mut s, (1, 1), false);
        assert_eq!((s.as_str(), e.caret), ("a    b", 5));
        let mut s = "a\nb\nc".to_string();
        indent(&mut s, (0, 3), false);
        assert_eq!(s, "    a\n    b\nc");
        indent(&mut s, (2, 9), true);
        assert_eq!(s, "a\nb\nc");
        let mut s = "  x".to_string();
        let e = indent(&mut s, (3, 3), true);
        assert_eq!((s.as_str(), e.caret), ("x", 1));
        let mut s = String::new();
        indent(&mut s, (7, 3), true);
        assert_eq!(s, "");
    }
}
