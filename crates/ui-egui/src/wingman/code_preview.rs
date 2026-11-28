//! Read-only source in the transcript. A fence is a language hint, never a request to run code.

use std::{ops::Range, sync::LazyLock};

use egui::{FontId, TextStyle, Ui, text::LayoutJob};
use egui_extras::syntax_highlighting::{self, CodeTheme, TokenType};
use gridcraft_engine::formula::lexer::{self, Tok};
use regex::Regex;

use crate::{
    code_editor::{self, Lang, Token},
    theme::Tokens,
};

// Bound tokenization *before* constructing a layout, including for a single line with no newline.
const MAX_BYTES: usize = 8 * 1024;
const MAX_LINES: usize = 64;

fn visible_source(source: &str) -> (&str, bool) {
    let source = source.strip_suffix("\r\n").or_else(|| source.strip_suffix('\n')).unwrap_or(source);
    let prefix = &source[..source.floor_char_boundary(MAX_BYTES.min(source.len()))];
    let preview = prefix.match_indices('\n').nth(MAX_LINES - 1).map_or(prefix, |(end, _)| {
        let through_line = &prefix[..end];
        through_line.strip_suffix('\r').unwrap_or(through_line)
    });
    (preview, preview.len() < source.len())
}

pub(super) fn show(ui: &mut Ui, language: &str, source: &str) {
    let (mut text, truncated) = visible_source(source);
    let t = Tokens::get(ui.ctx());
    let l = crate::i18n::current(ui.ctx());
    // Info strings are untrusted, too. Never allocate or display an unbounded name here.
    let language = if language.len() <= 32 { language.to_ascii_lowercase() } else { String::new() };
    let font = TextStyle::Monospace.resolve(ui.style());
    let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, width: f32| {
        let mut job = highlighted(ui, &language, buf.as_str(), &font, &t, width);
        job.wrap.max_width = width;
        job.keep_trailing_whitespace = true;
        ui.fonts_mut(|f| f.layout_job(job))
    };
    // &str is egui's immutable buffer: exposed as read-only to AccessKit while keeping the
    // caret, keyboard selection and ordinary Tab navigation. No editable draft or run action.
    let response = ui
        .add(
            egui::TextEdit::multiline(&mut text)
                .font(TextStyle::Monospace)
                .background_color(t.input_bg)
                .desired_width(f32::INFINITY)
                .desired_rows(1)
                .cursor_at_end(false)
                .layouter(&mut layouter),
        )
        .accessible_name(l.tr("Code preview"));
    if response.has_focus() {
        ui.painter().rect_stroke(response.rect, 2.0, egui::Stroke::new(2.0, t.popup_focus_ring()), egui::StrokeKind::Inside);
    }
    if truncated {
        ui.label(l.tr("Showing only the beginning of this code block."));
    }
}

fn highlighted(ui: &Ui, language: &str, source: &str, font: &FontId, t: &Tokens, width: f32) -> LayoutJob {
    match language {
        "python" | "py" | "python3" => code_editor::layout_job(source, Lang::Python, font, t.text, &t.syntax, width),
        "latex" | "tex" => code_editor::layout_job(source, Lang::Latex, font, t.text, &t.syntax, width),
        "formula" | "excel" | "excel-formula" | "xlsx" | "fx" => {
            code_editor::layout_spans(source, formula_spans(source), font, t.text, &t.syntax, width)
        }
        "vb" | "vba" | "vbscript" | "visualbasic" => code_editor::layout_spans(source, vba_spans(source), font, t.text, &t.syntax, width),
        "rs" | "rust" | "json" | "jsonc" | "json5" | "c" | "h" | "hpp" | "cpp" | "c++" | "js" | "javascript" | "jsx" | "mjs" | "ts"
        | "typescript" | "tsx" | "bash" | "sh" | "shell" | "zsh" | "sql" | "toml" | "yaml" | "yml" => {
            // Use the pinned egui highlighter, but with the same contrast-checked colours and
            // surface as the workbook's code editor (including punctuation and whitespace).
            use TokenType::{Comment, Function, Keyword, Literal, Punctuation, StringLiteral, Whitespace};
            let mut theme = CodeTheme::from_style(ui.style());
            for (token, color) in [
                (Comment, t.syntax.comment),
                (Function, t.syntax.builtin),
                (Keyword, t.syntax.keyword),
                (StringLiteral, t.syntax.string),
                (Literal, t.text),
                (Punctuation, t.text),
                (Whitespace, t.text),
            ] {
                *theme.format_mut(token) = egui::TextFormat::simple(font.clone(), color);
            }
            syntax_highlighting::highlight(ui.ctx(), ui.style(), &theme, source, language)
        }
        _ => LayoutJob::simple(source.to_owned(), font.clone(), t.text, width),
    }
}

fn formula_spans(source: &str) -> impl Iterator<Item = (Range<usize>, Token)> {
    // A malformed or partial formula stays readable. The lexer never evaluates a cell.
    let body = source.strip_prefix('=').unwrap_or(source);
    let offset = source.len() - body.len();
    lexer::tokenize(body).unwrap_or_default().into_iter().filter_map(move |span| {
        let kind = match span.tok {
            Tok::Number(_) => Token::Number,
            Tok::Text(_) => Token::String,
            Tok::Func(_) => Token::Builtin,
            Tok::Bool(_) | Tok::Error(_) | Tok::Word(_) | Tok::Sheet(_, _) | Tok::Struct(_) => Token::Keyword,
            _ => return None,
        };
        Some((offset + span.start..offset + span.end, kind))
    })
}

fn vba_spans(source: &str) -> impl Iterator<Item = (Range<usize>, Token)> {
    // Basic lexical colour only. Quotes inside strings are doubled; an apostrophe inside a
    // string is not a comment. A partially streamed line need not parse as a complete macro.
    static VBA: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(concat!(
            r#"(?ix) (?P<string> "(?: "" | [^"\r\n] )* "? )"#,
            r"| (?P<comment> '[^\r\n]* | \brem\b[^\r\n]* )",
            r"| (?P<keyword> \b(?:and|as|boolean|byref|byval|call|case|const|date|dim|do|double|each|else|elseif|empty|end|error|exit|",
            r"false|for|function|goto|if|in|integer|is|long|loop|me|mod|new|next|not|nothing|null|on|option|or|private|property|public|",
            r"redim|resume|select|set|step|string|sub|then|to|true|until|variant|wend|while|with|xor)\b )",
            r"| (?P<number> \b[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?\b )",
        ))
        .ok()
    });
    VBA.as_ref().into_iter().flat_map(move |pattern| pattern.captures_iter(source)).filter_map(|cap| {
        [("string", Token::String), ("comment", Token::Comment), ("keyword", Token::Keyword), ("number", Token::Number)]
            .into_iter()
            .find_map(|(name, kind)| cap.name(name).map(|m| (m.range(), kind)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_limits_preserve_utf8_and_crlf_source() {
        let long = format!("{}🌱", "x".repeat(MAX_BYTES - 1));
        let (shown, truncated) = visible_source(&long);
        assert!(truncated && long.starts_with(shown) && shown.ends_with('x'));
        let lines = "  行\r\n".repeat(MAX_LINES + 1);
        let (shown, truncated) = visible_source(&lines);
        assert!(truncated && lines.starts_with(shown) && shown.ends_with('行'));
        assert_eq!(shown.lines().count(), MAX_LINES);
        assert_eq!(visible_source("  x\r\n\r\n"), ("  x\r\n", false));
    }

    #[test]
    fn literals_keep_comment_markers_and_unicode_in_their_own_spans() {
        let vb = "a = \"he said \"\"l'été\"\"\" ' 🌱";
        let spans = vba_spans(vb).map(|(r, kind)| (&vb[r], kind)).collect::<Vec<_>>();
        assert_eq!(spans, [("\"he said \"\"l'été\"\"\"", Token::String), ("' 🌱", Token::Comment)]);
        let formula = "=IF(A1,\"東京\",42)";
        let spans = formula_spans(formula).map(|(r, kind)| (&formula[r], kind)).collect::<Vec<_>>();
        assert!(spans.contains(&("\"東京\"", Token::String)) && spans.contains(&("42", Token::Number)));
    }
}
