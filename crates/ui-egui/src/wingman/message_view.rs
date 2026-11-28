//! Presentation only: every clipboard action borrows original bytes; nothing here executes code.
use super::{code_preview, composer, math_preview, mermaid, message_math};
use crate::i18n::Language;
use egui::{Label, Ui, WidgetText};
use std::ops::Range;

fn without_line_ending(text: &str) -> &str {
    text.strip_suffix("\r\n").or_else(|| text.strip_suffix('\n')).unwrap_or(text)
}

pub(super) fn code_block(ui: &mut Ui, l: Language, language: &str, body: &str, closed: bool, finished: bool, left: &mut usize) {
    let diagram = language.eq_ignore_ascii_case("mermaid");
    let equation = ["math", "latex", "tex"].iter().any(|tag| language.eq_ignore_ascii_case(tag));
    let size_limit = if diagram { mermaid::MAX_SOURCE_BYTES } else { gridcraft_engine::math::MAX_SOURCE_BYTES };
    let preview = closed && (diagram || equation) && *left != 0 && body.len() <= size_limit;
    if preview {
        *left -= 1;
    }
    // Include the content in the toggle ID: when old chat entries scroll out of history their
    // recycled row indices must not change another diagram's view.
    let source_id = ui.scope_id().with(if preview { body } else { "" });
    let mut source = preview && ui.data(|d| d.get_temp::<bool>(source_id).unwrap_or(false));
    ui.horizontal_wrapped(|ui| {
        if language.len() > 32 {
            ui.weak(l.tr("Code"));
        } else if !language.is_empty() {
            ui.weak(language);
        }
        if (closed || finished) && ui.small_button(l.tr("Copy code")).clicked() {
            ui.ctx().copy_text(body.to_owned());
        }
        if preview && ui.toggle_value(&mut source, l.tr("Source")).changed() {
            ui.data_mut(|d| d.insert_temp(source_id, source));
        }
    });
    if preview && !source {
        let result = if diagram { mermaid::show(ui, body) } else { math_preview::show(ui, body.trim()) };
        match result {
            Ok(()) => return,
            Err(reason) => {
                ui.weak(l.tr(reason));
            }
        }
    } else if closed && (diagram || equation) && !preview {
        ui.weak(l.tr(if body.len() > size_limit {
            "This block is too large to preview; showing source."
        } else {
            "Preview limit reached; showing source."
        }));
    }
    code_preview::show(ui, if closed { language } else { "" }, body);
}

fn label(ui: &mut Ui, text: &str, start: usize, refs: &[Range<usize>]) {
    if text.is_empty() {
        return;
    }
    let first = refs.partition_point(|s| s.start < start);
    let mut local = refs[first..].iter().take_while(|s| s.end <= start + text.len()).map(|s| s.start - start..s.end - start).peekable();
    let label: WidgetText = if local.peek().is_some() { composer::reference_layout(ui, text, local).into() } else { text.into() };
    ui.add(Label::new(label).wrap().selectable(true));
}

pub(super) fn prose(ui: &mut Ui, l: Language, raw: &str, start: usize, refs: &[Range<usize>], left: &mut usize) {
    let text = without_line_ending(raw);
    let spans = if text.contains(['$', '\\']) { message_math::spans(text, *left, start, refs) } else { Vec::new() };
    *left -= spans.len();
    let mut at = 0;
    let mut remaining = spans.as_slice();
    loop {
        let count = remaining.iter().take_while(|s| !s.display).count();
        let (inline, rest) = remaining.split_at(count);
        let until = rest.first().map_or(text.len(), |s| s.outer.start);
        let tail_end = if rest.is_empty() { until } else { at + without_line_ending(&text[at..until]).len() };
        if inline.is_empty() {
            label(ui, &text[at..tail_end], start + at, refs);
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for s in inline {
                    label(ui, &text[at..s.outer.start], start + at, refs);
                    if math_preview::inline(ui, &text[s.body.clone()]).is_err() {
                        label(ui, &text[s.outer.clone()], start + s.outer.start, refs);
                    }
                    at = s.outer.end;
                }
                label(ui, &text[at..tail_end], start + at, refs);
            });
        }
        let Some((s, next)) = rest.split_first() else { break };
        if let Err(reason) = math_preview::show(ui, text[s.body.clone()].trim()) {
            ui.weak(l.tr(reason));
            label(ui, &text[s.outer.clone()], start + s.outer.start, refs);
        }
        at = s.outer.end;
        let after = &text[at..];
        at += if after.starts_with("\r\n") { 2 } else { usize::from(after.starts_with('\n')) };
        remaining = next;
    }
}
