use super::{LIMIT, NO_NODES, Result, SYNTAX, canvas, caption, plain, take_id, text};
use egui::{Rect, Stroke, Ui, pos2, vec2};

struct Person {
    id: String,
    label: String,
    actor: bool,
}

struct Message {
    from: usize,
    to: usize,
    label: String,
    dashed: bool,
    arrow: bool,
}

pub(super) struct Sequence {
    people: Vec<Person>,
    messages: Vec<Message>,
    summary: String,
}

impl Sequence {
    fn person(&mut self, id: &str) -> Result<usize> {
        if let Some(i) = self.people.iter().position(|p| p.id == id) {
            return Ok(i);
        }
        if self.people.len() >= 12 {
            return Err(LIMIT);
        }
        self.people.push(Person { id: id.into(), label: id.into(), actor: false });
        Ok(self.people.len() - 1)
    }

    fn statement(&mut self, s: &str) -> Result<()> {
        if let Some((kind, tail)) = s.split_once(char::is_whitespace)
            && matches!(kind, "participant" | "actor")
        {
            let (id, tail) = take_id(tail.trim_start())?;
            let alias = if tail.is_empty() {
                id
            } else {
                let alias = tail.trim_start().strip_prefix("as").and_then(|s| s.strip_prefix(char::is_whitespace)).ok_or(SYNTAX)?;
                plain(alias.trim())?
            };
            let i = self.person(id)?;
            self.people[i].label = alias.to_owned();
            self.people[i].actor = kind == "actor";
            return Ok(());
        }
        let (from, tail) = take_id(s)?;
        let tail = tail.trim_start();
        let (tail, dashed, arrow) = if let Some(t) = tail.strip_prefix("-->>") {
            (t, true, true)
        } else if let Some(t) = tail.strip_prefix("->>") {
            (t, false, true)
        } else if let Some(t) = tail.strip_prefix("-->") {
            (t, true, false)
        } else if let Some(t) = tail.strip_prefix("->") {
            (t, false, false)
        } else {
            return Err(SYNTAX);
        };
        let (to, tail) = take_id(tail.trim_start())?;
        let label = plain(tail.trim_start().strip_prefix(':').ok_or(SYNTAX)?.trim())?;
        if self.messages.len() >= 64 {
            return Err(LIMIT);
        }
        let from = self.person(from)?;
        let to = self.person(to)?;
        self.messages.push(Message { from, to, label: label.to_owned(), dashed, arrow });
        Ok(())
    }

    pub(super) fn show(&self, ui: &mut Ui) {
        let names: Vec<_> = self.people.iter().map(|p| text(ui, &p.label, 130.0)).collect();
        let mut x = Vec::with_capacity(names.len());
        let mut width = 0.0;
        let mut header: f32 = 0.0;
        for (p, g) in self.people.iter().zip(&names) {
            let w = (g.size().x + 24.0).max(100.0);
            x.push(width + w / 2.0);
            width += w + 14.0;
            header = header.max(g.size().y + if p.actor { 53.0 } else { 14.0 });
        }
        width -= 14.0;
        let mut y = header + 8.0;
        let mut rows = Vec::with_capacity(self.messages.len());
        for m in &self.messages {
            let gap = (x[m.from] - x[m.to]).abs();
            let wrap = if m.from == m.to { 90.0 } else { (gap - 10.0).min(190.0) };
            let g = text(ui, &m.label, wrap);
            let line = y + g.size().y + 5.0;
            rows.push((g, line));
            y = line + if m.from == m.to { 35.0 } else { 16.0 };
        }
        let height = y + 2.0;
        canvas(ui, vec2(width, height), &self.people[0].label, &self.summary, |ink, origin| {
            let at = |x, y| pos2(x + origin.x, y + origin.y);
            let stroke = Stroke::new(1.4, ink.text);
            for (i, p) in self.people.iter().enumerate() {
                let cx = x[i];
                let name = &names[i];
                // A long sequence can be thousands of points tall. Generate only visible dashes.
                let clip = ink.painter.clip_rect();
                let start = (header + origin.y).max(clip.top());
                let end = (height + origin.y).min(clip.bottom());
                if start < end && clip.x_range().contains(cx + origin.x) {
                    ink.line(&[pos2(cx + origin.x, start), pos2(cx + origin.x, end)], true, false, false);
                }
                if p.actor {
                    let top = header - name.size().y - 47.0;
                    ink.painter.circle(at(cx, top + 7.0), 6.0, ink.background, stroke);
                    ink.line(&[at(cx, top + 13.0), at(cx, top + 28.0)], false, false, false);
                    ink.line(&[at(cx - 10.0, top + 18.0), at(cx + 10.0, top + 18.0)], false, false, false);
                    ink.line(&[at(cx - 8.0, top + 38.0), at(cx, top + 28.0), at(cx + 8.0, top + 38.0)], false, false, false);
                    ink.label(at(cx, header - 4.0 - name.size().y / 2.0), name);
                } else {
                    let size = name.size() + vec2(18.0, 12.0);
                    let rect = Rect::from_center_size(at(cx, header - size.y / 2.0), size);
                    ink.painter.rect(rect, 3, ink.fill, stroke, egui::StrokeKind::Inside);
                    ink.label(rect.center(), name);
                }
            }
            for (m, (g, line)) in self.messages.iter().zip(rows) {
                let (a, b) = (x[m.from], x[m.to]);
                let center = at((a + b) / 2.0, line - g.size().y / 2.0 - 4.0);
                ink.painter.rect_filled(Rect::from_center_size(center, g.size() + vec2(4.0, 2.0)), 2, ink.background);
                ink.label(center, &g);
                let points = if a == b {
                    vec![at(a, line), at(a + 28.0, line), at(a + 28.0, line + 17.0), at(a, line + 17.0)]
                } else {
                    vec![at(a, line), at(b, line)]
                };
                ink.line(&points, m.dashed, false, m.arrow);
            }
        });
    }
}

pub(super) fn parse(lines: &[&str]) -> Result<Sequence> {
    let mut s = Sequence { people: Vec::new(), messages: Vec::new(), summary: String::new() };
    for line in lines {
        s.statement(line)?;
    }
    if s.people.is_empty() {
        return Err(NO_NODES);
    }
    for p in &s.people {
        if !s.summary.is_empty() {
            s.summary.push_str(", ");
        }
        s.summary.push_str(&p.label);
    }
    for m in &s.messages {
        caption(&mut s.summary, &s.people[m.from].label, &s.people[m.to].label, &m.label, true);
    }
    Ok(s)
}
