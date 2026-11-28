use super::{LIMIT, NO_NODES, Result, SYNTAX, canvas, caption, plain, take_id, text};
use egui::{Rect, Shape, Stroke, Ui, Vec2, vec2};

#[derive(Clone, Copy)]
enum BoxKind {
    Plain,
    Round,
    Diamond,
    Circle,
    Stadium,
}
struct Node {
    id: String,
    label: String,
    kind: BoxKind,
}
struct Edge {
    from: usize,
    to: usize,
    label: String,
    dashed: bool,
    thick: bool,
    arrow: bool,
}

pub(super) struct Flow {
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    horizontal: bool,
    reverse: bool,
    summary: String,
    layers: Vec<Vec<usize>>,
    slots: Vec<Slot>,
    routes: Vec<Vec<usize>>,
}

enum Slot {
    Node(usize),
    Label(usize),
    Way,
}

impl Flow {
    fn read_node<'a>(&mut self, s: &'a str) -> Result<(usize, &'a str)> {
        let (id, mut rest) = take_id(s.trim_start())?;
        if matches!(id, "subgraph" | "end" | "direction" | "classDef" | "linkStyle" | "style" | "click" | "class") {
            return Err(SYNTAX);
        }
        let mut declaration = None;
        for (open, close, kind) in [
            ("((", "))", BoxKind::Circle),
            ("([", "])", BoxKind::Stadium),
            ("[", "]", BoxKind::Plain),
            ("(", ")", BoxKind::Round),
            ("{", "}", BoxKind::Diamond),
        ] {
            let Some(body) = rest.strip_prefix(open) else { continue };
            let (label, tail) = if let Some(quoted) = body.strip_prefix('"') {
                let (label, tail) = quoted.split_once('"').ok_or(SYNTAX)?;
                (plain(label)?, tail.strip_prefix(close).ok_or(SYNTAX)?)
            } else {
                let (label, tail) = body.split_once(close).ok_or(SYNTAX)?;
                // Reject other shape delimiters (e.g. subroutines, cylinders and parallelograms).
                if label.contains(['[', ']', '{', '}', '(', ')', '"']) || label.starts_with(['/', '\\']) || label.ends_with(['/', '\\']) {
                    return Err(SYNTAX);
                }
                (plain(label.trim())?, tail)
            };
            declaration = Some((kind, label));
            rest = tail;
            break;
        }
        let idx = if let Some(i) = self.nodes.iter().position(|n| n.id == id) {
            if let Some((kind, label)) = declaration {
                self.nodes[i].kind = kind;
                self.nodes[i].label = label.into();
            }
            i
        } else {
            if self.nodes.len() >= 24 {
                return Err(LIMIT);
            }
            let (kind, label) = declaration.unwrap_or((BoxKind::Plain, id));
            self.nodes.push(Node { id: id.into(), label: label.into(), kind });
            self.nodes.len() - 1
        };
        Ok((idx, rest.trim_start()))
    }

    fn statement(&mut self, s: &str) -> Result<()> {
        let (mut from, mut rest) = self.read_node(s)?;
        while !rest.is_empty() {
            if self.edges.len() >= 40 {
                return Err(LIMIT);
            }
            let (tail, dashed, thick, arrow) = if let Some(t) = rest.strip_prefix("-->") {
                (t, false, false, true)
            } else if let Some(t) = rest.strip_prefix("---") {
                // The two suffixes decorate the edge; they are not part of the next node's ID.
                if t.starts_with(['o', 'x']) {
                    return Err(SYNTAX);
                }
                (t, false, false, false)
            } else if let Some(t) = rest.strip_prefix("-.->") {
                (t, true, false, true)
            } else if let Some(t) = rest.strip_prefix("==>") {
                (t, false, true, true)
            } else {
                return Err(SYNTAX);
            };
            let (label, tail) = if let Some(s) = tail.trim_start().strip_prefix('|') {
                let (s, tail) = s.split_once('|').ok_or(SYNTAX)?;
                let label = s.trim();
                let label = if let Some(q) = label.strip_prefix('"') { q.strip_suffix('"').ok_or(SYNTAX)? } else { label };
                if label.contains('"') {
                    return Err(SYNTAX);
                }
                (plain(label)?, tail)
            } else {
                ("", tail)
            };
            let (to, tail) = self.read_node(tail)?;
            if from == to {
                return Err("Flowchart self-links are not supported in the inline preview.");
            }
            self.edges.push(Edge { from, to, label: label.into(), dashed, thick, arrow });
            from = to;
            rest = tail;
        }
        Ok(())
    }

    // Establish a finite order even for cycles. Assign each node its longest forward rank.
    // Insert a route waypoint in every intervening layer so long arrows never pass through
    // a node, and a distinct label slot per edge so parallel links remain distinguishable.
    fn route(&mut self) -> Result<()> {
        let n = self.nodes.len();
        let mut order = Vec::with_capacity(n);
        let mut placed = vec![false; n];
        for _ in 0..n {
            let remaining = (0..n).filter(|&i| !placed[i]);
            let Some(next) = remaining.min_by_key(|&i| self.edges.iter().filter(|e| e.to == i && !placed[e.from]).count()) else { break };
            order.push(next);
            placed[next] = true;
        }
        let mut rank = vec![0; n];
        for &i in &order {
            placed[i] = false;
            for e in &self.edges {
                let other = if e.from == i {
                    e.to
                } else if e.to == i {
                    e.from
                } else {
                    continue;
                };
                if placed[other] {
                    rank[other] = rank[other].max(rank[i] + 2);
                }
            }
        }
        self.layers.resize_with(1 + rank.iter().max().copied().unwrap_or(0), Vec::new);
        for (i, &r) in rank.iter().enumerate() {
            self.slots.push(Slot::Node(i));
            self.layers[r].push(i);
        }
        for (index, e) in self.edges.iter().enumerate() {
            let (low, high) = if rank[e.from] < rank[e.to] { (e.from, e.to) } else { (e.to, e.from) };
            let mut path = vec![low];
            let middle = (rank[low] + rank[high]) / 2;
            for (r, layer) in self.layers.iter_mut().enumerate().take(rank[high]).skip(rank[low] + 1) {
                let slot = self.slots.len();
                if slot >= 384 {
                    return Err(LIMIT);
                }
                self.slots.push(if r == middle && !e.label.is_empty() { Slot::Label(index) } else { Slot::Way });
                layer.push(slot);
                path.push(slot);
            }
            path.push(high);
            if e.from == high {
                path.reverse();
            }
            self.routes.push(path);
        }
        Ok(())
    }
}

pub(super) fn parse(direction: &str, statements: &[&str]) -> Result<Flow> {
    let (horizontal, reverse) = match direction {
        "TD" | "TB" => (false, false),
        "BT" => (false, true),
        "LR" => (true, false),
        "RL" => (true, true),
        _ => return Err(SYNTAX),
    };
    let mut f = Flow {
        nodes: Vec::new(),
        edges: Vec::new(),
        horizontal,
        reverse,
        summary: String::new(),
        layers: Vec::new(),
        slots: Vec::new(),
        routes: Vec::new(),
    };
    for s in statements {
        f.statement(s)?;
    }
    if f.nodes.is_empty() {
        return Err(NO_NODES);
    }
    for n in &f.nodes {
        if !f.summary.is_empty() {
            f.summary.push_str(", ");
        }
        f.summary.push_str(&n.label);
    }
    for e in &f.edges {
        caption(&mut f.summary, &f.nodes[e.from].label, &f.nodes[e.to].label, &e.label, e.arrow);
    }
    f.route()?;
    Ok(f)
}

impl Flow {
    fn axes(&self, value: Vec2) -> Vec2 {
        if self.horizontal { value } else { vec2(value.y, value.x) }
    }

    pub(super) fn show(&self, ui: &mut Ui) {
        let mut sizes = Vec::with_capacity(self.slots.len());
        let mut labels = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            let (galley, size) = match *slot {
                Slot::Node(i) => {
                    let n = &self.nodes[i];
                    let g = text(ui, &n.label, 132.0);
                    let s = g.size() + vec2(20.0, 12.0);
                    let size = match n.kind {
                        BoxKind::Circle => Vec2::splat(s.length()),
                        BoxKind::Diamond => s * 2.0,
                        BoxKind::Stadium => s + vec2(s.y * 0.5, 0.0),
                        _ => s,
                    };
                    (Some(g), size)
                }
                Slot::Label(i) => {
                    let g = text(ui, &self.edges[i].label, 110.0);
                    let s = g.size() + vec2(8.0, 4.0);
                    (Some(g), s)
                }
                Slot::Way => (None, Vec2::splat(1.0)),
            };
            sizes.push(size);
            labels.push(galley);
        }
        // Work in (rank axis, cross axis), then map to screen x/y at the end.
        let mut centers = vec![Vec2::ZERO; self.slots.len()];
        let mut major: f32 = 0.0;
        let mut minor: f32 = 0.0;
        for layer in &self.layers {
            let depth = layer.iter().map(|&i| self.axes(sizes[i]).x).fold(0.0, f32::max);
            let mut across = 0.0;
            for &i in layer {
                let breadth = self.axes(sizes[i]).y;
                centers[i] = vec2(major + depth / 2.0, across + breadth / 2.0);
                across += breadth + 16.0;
            }
            across -= 16.0;
            for &i in layer {
                centers[i].y -= across / 2.0;
            }
            major += depth + 14.0;
            minor = minor.max(across);
        }
        major -= 14.0;
        for p in &mut centers {
            if self.reverse {
                p.x = major - p.x;
            }
            p.y += minor / 2.0;
            *p = self.axes(*p);
        }
        let size = self.axes(vec2(major, minor));
        canvas(ui, size, &self.nodes[0].label, &self.summary, |ink, origin| {
            let center = |i: usize| (centers[i] + origin).to_pos2();
            for (e, route) in self.edges.iter().zip(&self.routes) {
                let mut path: Vec<_> = route.iter().map(|&i| center(i)).collect();
                let sign = self.axes(centers[e.to] - centers[e.from]).x.signum();
                let side = self.axes(vec2(sign, 0.0));
                if let Some(p) = path.first_mut() {
                    *p += side * self.axes(sizes[e.from]).x * 0.5;
                }
                if let Some(p) = path.last_mut() {
                    *p -= side * self.axes(sizes[e.to]).x * 0.5;
                }
                ink.line(&path, e.dashed, e.thick, e.arrow);
            }
            for (i, slot) in self.slots.iter().enumerate() {
                let Some(galley) = &labels[i] else { continue };
                let r = Rect::from_center_size(center(i), sizes[i]);
                if let Slot::Node(idx) = slot {
                    let stroke = Stroke::new(1.3, ink.text);
                    match self.nodes[*idx].kind {
                        BoxKind::Circle => {
                            ink.painter.circle(r.center(), r.width() * 0.5, ink.fill, stroke);
                        }
                        BoxKind::Diamond => {
                            ink.painter.add(Shape::convex_polygon(
                                vec![r.center_top(), r.right_center(), r.center_bottom(), r.left_center()],
                                ink.fill,
                                stroke,
                            ));
                        }
                        kind => {
                            let round = match kind {
                                BoxKind::Stadium => (r.height() / 2.0) as u8,
                                BoxKind::Round => 7,
                                _ => 0,
                            };
                            ink.painter.rect(r, round, ink.fill, stroke, egui::StrokeKind::Inside);
                        }
                    }
                } else {
                    ink.painter.rect_filled(r, 2, ink.background);
                }
                ink.label(center(i), galley);
            }
        });
    }
}
