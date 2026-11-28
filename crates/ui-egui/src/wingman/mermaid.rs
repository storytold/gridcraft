//! Read-only previews of small diagrams in chat. Syntax references: Mermaid 12.4
//! [flowcharts](https://mermaid.js.org/syntax/flowchart.html) and
//! [sequence diagrams](https://mermaid.js.org/syntax/sequenceDiagram.html).
//! Parse the whole fence before painting: an unrecognized statement is never silently omitted.
mod flow;
mod sequence;

use egui::{Color32, Galley, Painter, Pos2, Shape, Stroke, Ui, Vec2, vec2};
use std::sync::Arc;

type Result<T> = std::result::Result<T, &'static str>;
pub(super) const MAX_SOURCE_BYTES: usize = 12 * 1024;
const LIMIT: &str = "This diagram exceeds the inline preview limits.";
const SYNTAX: &str = "This diagram uses syntax the inline preview does not support.";
const NO_NODES: &str = "The diagram has no nodes to display.";

enum Diagram {
    Flow(flow::Flow),
    Sequence(sequence::Sequence),
}

#[derive(Default)]
struct Parser;
impl egui::cache::ComputerMut<&str, Result<Arc<Diagram>>> for Parser {
    fn compute(&mut self, source: &str) -> Result<Arc<Diagram>> {
        parse(source).map(Arc::new)
    }
}

fn parse(source: &str) -> Result<Diagram> {
    // Split statements conservatively. Semicolons inside a quoted label aren't separators.
    let mut statements = Vec::new();
    for line in source.lines().map(str::trim).filter(|s| !s.is_empty()) {
        if line.starts_with("%%{") {
            return Err("Diagram configuration directives are not supported in the inline preview.");
        }
        if line.starts_with("%%") {
            continue;
        }
        let mut quoted = false;
        let mut start = 0;
        for (offset, ch) in line.char_indices() {
            if ch == '"' {
                quoted = !quoted;
            } else if ch == ';' && !quoted {
                let statement = line[start..offset].trim();
                if !statement.is_empty() {
                    statements.push(statement);
                    if statements.len() > 100 {
                        return Err(LIMIT);
                    }
                }
                start = offset + 1;
            }
        }
        if quoted {
            return Err(SYNTAX);
        }
        if !line[start..].trim().is_empty() {
            statements.push(line[start..].trim());
        }
        if statements.len() > 100 {
            return Err(LIMIT);
        }
    }
    let (header, lines) = statements.split_first().ok_or(NO_NODES)?;
    if *header == "sequenceDiagram" {
        return sequence::parse(lines).map(Diagram::Sequence);
    }
    let mut words = header.split_whitespace();
    if !matches!(words.next(), Some("flowchart" | "graph")) {
        return Err("The inline preview supports flowcharts and sequence diagrams.");
    }
    let direction = words.next().ok_or(SYNTAX)?;
    if words.next().is_some() {
        return Err(SYNTAX);
    }
    flow::parse(direction, lines).map(Diagram::Flow)
}

pub(super) fn show(ui: &mut Ui, src: &str) -> Result<()> {
    // Bound work before even hashing an untrusted fence.
    if src.len() > MAX_SOURCE_BYTES {
        return Err(LIMIT);
    }
    type Cache = egui::cache::FrameCache<Result<Arc<Diagram>>, Parser>;
    let diagram = ui.memory_mut(|m| m.caches.cache::<Cache>().get(src).clone())?;
    match &*diagram {
        Diagram::Flow(f) => f.show(ui),
        Diagram::Sequence(s) => s.show(ui),
    }
    Ok(())
}

fn take_id(text: &str) -> Result<(&str, &str)> {
    let n = text.bytes().take_while(|b| b.is_ascii_alphanumeric() || *b == b'_').count();
    if n == 0 {
        return Err(SYNTAX);
    }
    if n > 80 {
        return Err(LIMIT);
    }
    Ok(text.split_at(n))
}

fn plain(label: &str) -> Result<&str> {
    if label.len() > 384 {
        return Err(LIMIT);
    }
    if label.is_empty()
        || label.contains(['<', '>', '`', '#'])
        || label.contains("%%")
        || label.contains("$$")
        || (label.contains('&') && label.contains(';'))
        || label.contains("fa:")
        || label.chars().any(char::is_control)
    {
        return Err(SYNTAX);
    }
    Ok(label)
}

fn text(ui: &Ui, s: &str, width: f32) -> Arc<Galley> {
    ui.painter().layout(s.to_owned(), egui::TextStyle::Body.resolve(ui.style()), ui.visuals().text_color(), width)
}

struct Ink<'a> {
    painter: &'a Painter,
    text: Color32,
    fill: Color32,
    background: Color32,
}

impl Ink<'_> {
    fn label(&self, center: Pos2, galley: &Arc<Galley>) {
        self.painter.galley(center - galley.size() * 0.5, galley.clone(), self.text);
    }

    fn line(&self, points: &[Pos2], dashed: bool, thick: bool, arrow: bool) {
        let stroke = Stroke::new(if thick { 3.0 } else { 1.4 }, self.text);
        if dashed {
            self.painter.extend(Shape::dashed_line(points, stroke, 4.0, 3.0));
        } else {
            self.painter.add(Shape::line(points.to_vec(), stroke));
        }
        if arrow && let [.., before, end] = points {
            let back = (*before - *end).normalized() * 7.0;
            let side = vec2(-back.y, back.x) * 0.5;
            self.painter.add(Shape::convex_polygon(vec![*end, *end + back + side, *end + back - side], self.text, Stroke::NONE));
        }
    }
}

fn canvas(ui: &mut Ui, size: Vec2, title: &str, description: &str, draw: impl FnOnce(&Ink<'_>, Vec2)) {
    let pad = Vec2::splat(8.0);
    let size = size + pad * 2.0;
    let overflow = size.x > ui.available_width();
    let draw_area = |ui: &mut Ui| {
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::focusable_noninteractive());
        ui.ctx().accesskit_node_builder(response.id, |node| {
            node.set_role(egui::Role::Image);
            node.set_label(title);
            node.set_description(description);
        });
        if response.has_focus() {
            let visible = rect.intersect(ui.clip_rect()).shrink(1.0);
            ui.painter().rect_stroke(visible, 2.0, ui.visuals().selection.stroke, egui::StrokeKind::Inside);
            if overflow {
                for (key, delta) in [(egui::Key::ArrowLeft, 64.0), (egui::Key::ArrowRight, -64.0)] {
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                        ui.scroll_with_delta(vec2(delta, 0.0));
                    }
                }
            }
        }
        if ui.is_rect_visible(rect) {
            draw(
                &Ink {
                    painter: ui.painter(),
                    text: ui.visuals().text_color(),
                    fill: ui.visuals().faint_bg_color,
                    background: ui.visuals().panel_fill,
                },
                rect.min.to_vec2() + pad,
            );
        }
    };
    if overflow {
        egui::ScrollArea::horizontal().id_salt(ui.next_auto_id()).min_scrolled_width(0.0).show(ui, draw_area);
    } else {
        draw_area(ui);
    }
}

fn caption(summary: &mut String, from: &str, to: &str, label: &str, arrow: bool) {
    summary.push('\n');
    summary.push_str(from);
    summary.push_str(if arrow { " → " } else { " — " });
    summary.push_str(to);
    if !label.is_empty() {
        summary.push_str(": ");
        summary.push_str(label);
    }
}

#[cfg(test)]
mod tests {
    use super::{Diagram, parse};

    #[test]
    fn rejects_the_entire_preview_if_an_edge_or_directive_would_be_lost() {
        for suffix in ["P---oQ", "P---xQ", "P-->|open|", "Q[(Store)]", "click P callback", "%%{init: {}}%%"] {
            let source = format!("graph LR\nP((Port)) --> Q\n{suffix}");
            assert!(parse(&source).is_err(), "{suffix}");
        }
        assert!(matches!(parse("graph RL; P((Port)) -->|read| Q[Ledger]; Q -.-> P"), Ok(Diagram::Flow(_))));
    }

    #[test]
    fn treats_sequence_activations_as_unsupported_not_as_participant_names() {
        assert!(parse("sequenceDiagram\nReader->>+Ledger: begin").is_err());
        assert!(matches!(parse("sequenceDiagram\nactor R as Reader\nR->>Page: Fetch\nPage-->>R: Done"), Ok(Diagram::Sequence(_))));
    }
}
