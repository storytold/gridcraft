//! Equations: painting a laid-out [`MathList`] with egui.
//!
//! Painting only ever reads the layout cached on the shape (`Shape::math`); it never lays an
//! equation out, so nothing here can reach the layout engine. Glyphs are drawn with the system's
//! maths fonts (`theme::MATH*`); a glyph no font has becomes a `[?]` box the size of its TeX box.

use std::sync::Arc;

use egui::epaint::{Mesh, PathShape, PathStroke};
use egui::text::{CCursor, CCursorRange};
use egui::{Color32, FontFamily, FontId, Key, Painter, Pos2, Rect, Stroke, StrokeKind, TextEdit, pos2, vec2};
use gridcraft_engine::math::{Face, Item, MathError, MathList, PathCmd};
use gridcraft_engine::model::{Shape, ShapeKind, Workbook};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::code_editor::{Lang, code_editor_marked};
use crate::dialogs::Dialog;
use crate::theme::{self, Tokens};

/// Most points one outline may have; longer ones are stroked instead of filled.
const MAX_FILL_POINTS: usize = 4000;

fn family(face: Face) -> FontFamily {
    FontFamily::Name(
        match face {
            Face::Roman | Face::Large(_) => theme::MATH,
            Face::Italic => theme::MATH_ITALIC,
            Face::Bold | Face::BoldItalic => theme::MATH_BOLD,
            Face::Sans => theme::CELL,
            Face::SansBold => theme::CELL_BOLD,
            Face::SansItalic => theme::CELL_ITALIC,
            Face::Mono => theme::MONO,
        }
        .into(),
    )
}

// ---------------------------------------------------------------- editor

/// The editor's quick-insert buttons: (label, LaTeX, tooltip). The caret lands in the first `{}`.
pub const QUICK: &[(&str, &str, &str)] = &[
    ("a/b", "\\frac{}{}", "Fraction"),
    ("\u{221A}x", "\\sqrt{}", "Square root"),
    ("x^n", "^{}", "Superscript"),
    ("x_i", "_{}", "Subscript"),
    ("\u{2211}", "\\sum_{i=1}^{n} ", "Summation"),
    ("\u{222B}", "\\int_{a}^{b} ", "Integral"),
    ("lim", "\\lim_{x \\to {}} ", "Limit"),
    ("( )", "\\left( {} \\right)", "Brackets that grow"),
    ("[ ]", "\\begin{bmatrix} {} & b \\\\ c & d \\end{bmatrix}", "Matrix"),
    ("\u{03B1}", "\\alpha ", "alpha"),
    ("\u{03B2}", "\\beta ", "beta"),
    ("\u{03B8}", "\\theta ", "theta"),
    ("\u{03C0}", "\\pi ", "pi"),
    ("\u{0394}", "\\Delta ", "Delta"),
    ("\u{00B1}", "\\pm ", "Plus or minus"),
    ("\u{00D7}", "\\times ", "Times"),
    ("\u{00F7}", "\\div ", "Divided by"),
    ("\u{2264}", "\\le ", "Less than or equal to"),
    ("\u{2265}", "\\ge ", "Greater than or equal to"),
    ("\u{2260}", "\\ne ", "Not equal to"),
    ("\u{221E}", "\\infty ", "Infinity"),
    ("\u{2192}", "\\to ", "Tends to"),
];

/// The editor dialog: a new equation at the active cell, or `{id}` to edit one.
pub fn open_editor(app: &SheetApp, params: &Json) -> Option<Dialog> {
    let id = params.get("id").and_then(Json::as_u64);
    let mut d = match id {
        Some(id) => {
            let source = app
                .session
                .active()
                .and_then(|doc| doc.wb.active())
                .and_then(|sh| sh.shapes.iter().find(|s| s.id as u64 == id && s.kind == ShapeKind::Equation))
                .map(|s| s.text.clone())?;
            let mut d = Dialog::custom("equation", "Edit Equation", json!({"id": id, "latex": source}));
            d.command = Some("equation.edit");
            d
        }
        None => {
            let at = app.session.active().map(|doc| doc.selection.active.a1()).unwrap_or_default();
            let latex = params.get("latex").and_then(Json::as_str).unwrap_or("");
            let mut d = Dialog::custom("equation", "Insert Equation", json!({"at": at, "latex": latex}));
            d.command = Some("insert.equation");
            d
        }
    };
    d.result = None;
    Some(d)
}

/// The preview's layout, kept in egui memory and rebuilt only when the source changes.
#[derive(Clone)]
struct Preview {
    source: String,
    result: Result<Arc<MathList>, MathError>,
}

/// The source laid out (guarded, capped) for the preview; the editor is one of the two places,
/// with commands, that may lay equations out.
fn preview(ctx: &egui::Context, id: egui::Id, source: &str) -> Preview {
    if let Some(p) = ctx.data(|d| d.get_temp::<Preview>(id)).filter(|p| p.source == source) {
        return p;
    }
    let p = Preview { source: source.to_string(), result: gridcraft_engine::math::layout(source).map(Arc::new) };
    ctx.data_mut(|d| d.insert_temp(id, p.clone()));
    p
}

/// Inserts `snippet` at the editor's caret (replacing a selection) and puts the caret in its
/// first `{}`.
fn insert_snippet(ctx: &egui::Context, id: egui::Id, text: &mut String, snippet: &str) {
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    let count = text.chars().count();
    let (a, b) = state
        .cursor
        .char_range()
        .map(|r| -> (usize, usize) { (r.primary.index.into(), r.secondary.index.into()) })
        .map_or((count, count), |(p, q)| (p.min(q), p.max(q)));
    let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(i, _)| i);
    let (ba, bb) = (byte(a.min(count)), byte(b.min(count)));
    text.replace_range(ba..bb, snippet);
    let caret = a + snippet.find("{}").map_or(snippet.chars().count(), |i| snippet.get(..i).map_or(0, |s| s.chars().count()) + 1);
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(caret))));
    state.store(ctx, id);
    // Give the source the focus back once it has been drawn this frame.
    ctx.data_mut(|m| m.insert_temp(id.with("refocus"), true));
}

/// The editor's body: quick-insert buttons, the LaTeX source with its error underlined, a live
/// preview on a sheet-coloured card, and OK/Cancel.
pub fn editor(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let _ = app;
    let t = Tokens::get(ui.ctx());
    let edit_id = ui.scope_id().with("equation-source");
    let mut text = d.values.get("latex").and_then(Json::as_str).unwrap_or("").to_string();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(3.0, 3.0);
        for (label, snippet, tip) in QUICK {
            let b = ui.add(egui::Button::new(*label).min_size(vec2(30.0, 22.0))).on_hover_text(*tip);
            if b.clicked() {
                insert_snippet(ui.ctx(), edit_id, &mut text, snippet);
            }
        }
    });
    ui.add_space(6.0);
    ui.label("LaTeX:");
    let pv = preview(ui.ctx(), ui.scope_id().with("equation-preview"), &text);
    let mark = pv.result.as_ref().err().and_then(|e| e.span.clone()).or_else(|| pv.result.as_ref().err().map(|_| text.len()..text.len()));
    let mark = mark.filter(|_| !text.trim().is_empty());
    let r = code_editor_marked(ui, edit_id, &mut text, Lang::Latex, 6, mark);
    // Focus the source when the dialog opens and after a quick-insert button. The "opened" flag
    // lives on the dialog, so every way of closing it (OK, Cancel, Escape, the close button)
    // leaves the next one to focus its source again.
    let refocus = ui.ctx().data_mut(|m| m.remove_temp::<bool>(edit_id.with("refocus")).unwrap_or(false));
    if refocus || !d.focus_initialized {
        r.request_focus();
        d.focus_initialized = true;
    }
    let pv = preview(ui.ctx(), ui.scope_id().with("equation-preview"), &text);
    d.values.insert("latex".into(), json!(text));
    ui.add_space(4.0);
    match &pv.result {
        Err(e) if !text.trim().is_empty() => {
            let msg = match e.char_pos(&text) {
                Some(p) => format!("Error at character {p}: {}", e.message),
                None => e.message.clone(),
            };
            ui.colored_label(t.danger, msg);
        }
        _ => {
            ui.label(egui::RichText::new("Preview").color(t.text_dim));
        }
    }
    // The preview card is the sheet's colour, with the sheet's ink, in both themes.
    let (card, _) = ui.allocate_exact_size(vec2(ui.available_width(), 110.0), egui::Sense::hover());
    ui.painter().rect_filled(card, 4.0, t.grid_bg);
    ui.painter().rect_stroke(card, 4.0, Stroke::new(1.0, t.input_border), StrokeKind::Inside);
    match &pv.result {
        Ok(list) if !list.items.is_empty() => {
            // Fit, but never larger than 24 pt.
            let s = fit(list, card, 10.0).min(24.0 * 96.0 / 72.0);
            let o = card.center() - vec2(list.width, list.total_height()) * s / 2.0;
            paint_at(&ui.painter().with_clip_rect(card), o, list, t.cell_text, s);
        }
        _ => {
            let hint = if text.trim().is_empty() { "Type an equation in LaTeX, such as x^2 + y^2 = r^2" } else { "" };
            ui.painter().text(card.center(), egui::Align2::CENTER_CENTER, hint, theme::ui_font(12.0), t.sheet_control_text);
        }
    }
    ui.add_space(8.0);
    let valid = pv.result.as_ref().is_ok_and(|l| !l.items.is_empty());
    let cmd_enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Enter));
    ui.allocate_ui_with_layout(vec2(ui.available_width(), 28.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let ok = ui.add_enabled(valid, egui::Button::new(egui::RichText::new("OK").color(t.on_accent)).fill(t.accent).min_size(vec2(64.0, 0.0)));
        if ok.clicked() || (valid && cmd_enter) {
            *confirm = true;
        }
        if ui.add(egui::Button::new("Cancel").min_size(vec2(64.0, 0.0))).clicked() {
            *open = false;
        }
        let verb = if d.command == Some("equation.edit") { "apply" } else { "insert" };
        ui.label(egui::RichText::new(format!("\u{2318}/Ctrl+Enter to {verb}")).small().color(t.text_dim));
    });
}

/// The gallery's layouts, made once (guarded, like every layout) the first time the menu opens.
fn gallery_layouts() -> &'static [(&'static str, &'static str, Option<MathList>)] {
    static G: std::sync::OnceLock<Vec<(&'static str, &'static str, Option<MathList>)>> = std::sync::OnceLock::new();
    G.get_or_init(|| gridcraft_engine::math::GALLERY.iter().map(|(name, src)| (*name, *src, gridcraft_engine::math::layout(src).ok())).collect())
}

/// The gallery as menu rows: a name over a preview on a sheet-coloured card. Returns the source
/// of the row clicked.
pub fn gallery(ui: &mut egui::Ui) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    let mut picked = None;
    for (name, src, list) in gallery_layouts() {
        let (r, resp) = ui.allocate_exact_size(vec2(290.0, 62.0), egui::Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.hover);
        }
        ui.painter().text(r.left_top() + vec2(6.0, 4.0), egui::Align2::LEFT_TOP, *name, theme::ui_font(12.0), t.text);
        let card = Rect::from_min_max(r.left_top() + vec2(6.0, 21.0), r.right_bottom() - vec2(6.0, 3.0));
        ui.painter().rect_filled(card, 3.0, t.grid_bg);
        if let Some(list) = list {
            let s = fit(list, card, 3.0).min(16.0);
            paint_at(&ui.painter().with_clip_rect(card), card.center() - vec2(list.width, list.total_height()) * s / 2.0, list, t.cell_text, s);
        }
        if resp.on_hover_text(*src).clicked() {
            picked = Some(*src);
        }
    }
    picked
}

/// Draws an equation shape in its frame `r` (screen pixels, zoom `z`).
pub fn paint_shape(p: &Painter, r: Rect, sp: &Shape, wb: &Workbook, z: f32) {
    let t = Tokens::get(p.ctx());
    let ink = sp.line.resolve(&wb.theme).map(theme::color32).unwrap_or(t.cell_text);
    let p = p.with_clip_rect(r.intersect(p.clip_rect()));
    match sp.math.get(&sp.text) {
        Some(Ok(list)) => {
            paint_list(&p, r, list, ink, gridcraft_engine::cmd::equation::PADDING * z);
        }
        // No layout (a source that doesn't parse, from a file): show the source in a dashed
        // frame so it can be found and fixed.
        other => {
            let font = FontId::new(13.0 * z, FontFamily::Name(theme::MONO.into()));
            let source = gridcraft_engine::cmd::equation::source_excerpt(&sp.text).to_owned();
            let g = p.layout(source, font, t.cell_text, (r.width() - 8.0 * z).max(1.0));
            p.galley(r.min + vec2(4.0, 4.0) * z, g, t.cell_text);
            if matches!(other, Some(Err(_))) {
                let dash = egui::Shape::dashed_line(
                    &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()],
                    Stroke::new(1.0, t.sheet_control_border),
                    4.0,
                    3.0,
                );
                p.extend(dash);
            }
        }
    }
}

/// The scale (screen pixels per em) at which `list` fits `r` inside `pad`.
pub fn fit(list: &MathList, r: Rect, pad: f32) -> f32 {
    let (w, h) = (list.width.max(0.01), list.total_height().max(0.01));
    let s = ((r.width() - 2.0 * pad) / w).min((r.height() - 2.0 * pad) / h);
    if s.is_finite() { s.clamp(0.0, 4000.0) } else { 0.0 }
}

/// Draws `list` centred in `r` at the largest scale that fits inside `pad`. Returns the scale.
pub fn paint_list(p: &Painter, r: Rect, list: &MathList, ink: Color32, pad: f32) -> f32 {
    let s = fit(list, r, pad);
    paint_at(p, r.center() - vec2(list.width, list.total_height()) * s / 2.0, list, ink, s);
    s
}

/// Draws `list` with its box's top-left at `o`, at `s` screen pixels per em.
pub fn paint_at(p: &Painter, o: Pos2, list: &MathList, ink: Color32, s: f32) {
    if s < 0.05 {
        return;
    }
    let col = |c: Option<[u8; 3]>| c.map_or(ink, |[r, g, b]| Color32::from_rgb(r, g, b));
    for item in &list.items {
        match item {
            Item::Glyph(g) => {
                let color = col(g.color);
                let (x, baseline) = (o.x + g.x * s, o.y + g.y * s);
                let large = matches!(g.face, Face::Large(_));
                // A large operator or delimiter is drawn at the height of its TeX box.
                let px = if large { (g.height + g.depth) * s } else { g.size * s }.clamp(0.0, 4000.0);
                if px < 1.0 {
                    continue;
                }
                let font = FontId::new(px, family(g.face));
                let ch = if renders(p, &font, g.ch) { Some(g.ch) } else { g.alt.filter(|a| renders(p, &font, *a)) };
                let Some(ch) = ch else {
                    unknown_box(
                        p,
                        Rect::from_min_max(pos2(x, baseline - g.height * s), pos2(x + g.width.max(0.3) * s, baseline + g.depth * s)),
                        color,
                    );
                    continue;
                };
                let galley = p.layout_no_wrap(ch.to_string(), font, color);
                let ascent = galley.rows.first().and_then(|row| row.glyphs.first().map(|gl| row.pos.y + gl.pos.y)).unwrap_or(px * 0.8);
                let (gx, gy) = if large {
                    // Centre the glyph on its box: maths fonts centre these on the maths axis,
                    // about a quarter em above the baseline.
                    let mid = o.y + (g.y - (g.height - g.depth) / 2.0) * s;
                    (x + (g.width * s - galley.size().x) / 2.0, mid + 0.25 * px - ascent)
                } else {
                    (x, baseline - ascent)
                };
                p.galley(pos2(gx, gy), galley, color);
            }
            Item::Rule(rule) => {
                let h = if rule.height > 0.0 { (rule.height * s).max(1.0) } else { 0.0 };
                let w = if rule.width > 0.0 { (rule.width * s).max(1.0) } else { 0.0 };
                if w > 0.0 && h > 0.0 {
                    p.rect_filled(Rect::from_min_size(pos2(o.x + rule.x * s, o.y + rule.y * s), vec2(w, h)), 0.0, col(rule.color));
                }
            }
            Item::Path(path) => {
                let at = pos2(o.x + path.x * s, o.y + path.y * s);
                let color = col(path.color);
                for sub in flatten(&path.commands, at, s) {
                    if path.fill && sub.len() >= 3 && sub.len() <= MAX_FILL_POINTS {
                        p.add(fill_polygon(&sub, color));
                    } else if sub.len() >= 2 {
                        p.add(PathShape::line(sub, PathStroke::new(1.0, color)));
                    }
                }
            }
        }
    }
}

/// Whether some face in `font`'s family draws `c` (rather than the replacement glyph).
///
/// `Fonts::has_glyph` alone reports a false negative for every character of the face that also
/// supplies the replacement glyph (a maths font usually has `◻`), so a "no" is double-checked by
/// comparing the glyph's atlas slot with the replacement glyph's.
pub fn renders(p: &Painter, font: &FontId, c: char) -> bool {
    if p.ctx().fonts_mut(|f| f.has_glyph(font, c)) {
        return true;
    }
    let uv = |c: char| {
        let g = p.layout_no_wrap(c.to_string(), font.clone(), Color32::WHITE);
        g.rows.first().and_then(|r| r.glyphs.first().map(|gl| gl.uv_rect))
    };
    let Some(mine) = uv(c) else { return false };
    if mine.is_nothing() {
        return false;
    }
    ['\u{25FB}', '?'].iter().all(|r| *r == c || uv(*r) != Some(mine))
}

/// A glyph no font has: a box the size of its TeX box with a question mark.
fn unknown_box(p: &Painter, r: Rect, color: Color32) {
    if r.width() < 4.0 || r.height() < 4.0 {
        p.rect_filled(r, 0.0, color.gamma_multiply(0.4));
        return;
    }
    p.rect_stroke(r.shrink(0.5), 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
    let size = (r.height() * 0.6).min(r.width() * 0.9).max(4.0);
    p.text(r.center(), egui::Align2::CENTER_CENTER, "?", FontId::new(size, FontFamily::Name(theme::CELL.into())), color);
}

/// The outline as screen-space polygons, one per subpath, with curves as short lines.
fn flatten(cmds: &[PathCmd], at: Pos2, s: f32) -> Vec<Vec<Pos2>> {
    let pt = |x: f32, y: f32| pos2(at.x + x * s, at.y + y * s);
    let mut out: Vec<Vec<Pos2>> = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    for c in cmds {
        match *c {
            PathCmd::MoveTo(x, y) => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                cur.push(pt(x, y));
            }
            PathCmd::LineTo(x, y) => cur.push(pt(x, y)),
            PathCmd::QuadTo(x1, y1, x, y) => {
                let p0 = cur.last().copied().unwrap_or(pt(x1, y1));
                let (c1, p1) = (pt(x1, y1), pt(x, y));
                for k in 1..=8 {
                    let t = k as f32 / 8.0;
                    let u = 1.0 - t;
                    cur.push(pos2(u * u * p0.x + 2.0 * u * t * c1.x + t * t * p1.x, u * u * p0.y + 2.0 * u * t * c1.y + t * t * p1.y));
                }
            }
            PathCmd::CubicTo(x1, y1, x2, y2, x, y) => {
                let p0 = cur.last().copied().unwrap_or(pt(x1, y1));
                let (c1, c2, p1) = (pt(x1, y1), pt(x2, y2), pt(x, y));
                for k in 1..=10 {
                    let t = k as f32 / 10.0;
                    let u = 1.0 - t;
                    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                    cur.push(pos2(a * p0.x + b * c1.x + c * c2.x + d * p1.x, a * p0.y + b * c1.y + c * c2.y + d * p1.y));
                }
            }
            PathCmd::Close => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if cur.len() > 1 {
        out.push(cur);
    }
    out
}

/// A filled simple polygon (convex or not), triangulated by ear clipping.
fn fill_polygon(pts: &[Pos2], color: Color32) -> Mesh {
    let mut mesh = Mesh::default();
    let mut pts: Vec<Pos2> = pts.to_vec();
    pts.dedup_by(|a, b| (*a - *b).length_sq() < 1e-6);
    if pts.len() > 2 && pts.first().zip(pts.last()).is_some_and(|(a, b)| (*a - *b).length_sq() < 1e-6) {
        pts.pop();
    }
    for q in &pts {
        mesh.colored_vertex(*q, color);
    }
    for [a, b, c] in triangulate(&pts) {
        mesh.add_triangle(a as u32, b as u32, c as u32);
    }
    mesh
}

fn cross(o: Pos2, a: Pos2, b: Pos2) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

/// Ear clipping of a simple polygon. A degenerate polygon that has no ear left is finished as a
/// fan, so the result is always a valid (if imperfect) set of triangles.
fn triangulate(pts: &[Pos2]) -> Vec<[usize; 3]> {
    let n = pts.len();
    if n < 3 {
        return vec![];
    }
    let area: f32 =
        (0..n).map(|i| cross(Pos2::ZERO, pts.get(i).copied().unwrap_or_default(), pts.get((i + 1) % n).copied().unwrap_or_default())).sum();
    let ccw = area > 0.0;
    let at = |i: usize| pts.get(i).copied().unwrap_or_default();
    let mut idx: Vec<usize> = (0..n).collect();
    let mut tris = Vec::with_capacity(n);
    let mut guard = 0usize;
    while idx.len() > 3 && guard < n * n {
        guard += 1;
        let m = idx.len();
        let mut clipped = false;
        for k in 0..m {
            let (ia, ib, ic) =
                (idx.get((k + m - 1) % m).copied().unwrap_or(0), idx.get(k).copied().unwrap_or(0), idx.get((k + 1) % m).copied().unwrap_or(0));
            let (a, b, c) = (at(ia), at(ib), at(ic));
            let turn = cross(a, b, c);
            if (turn > 0.0) != ccw || turn.abs() < 1e-9 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                if j == ia || j == ib || j == ic {
                    return false;
                }
                let q = at(j);
                let (d1, d2, d3) = (cross(a, b, q), cross(b, c, q), cross(c, a, q));
                if ccw { d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0 } else { d1 <= 0.0 && d2 <= 0.0 && d3 <= 0.0 }
            });
            if inside {
                continue;
            }
            tris.push([ia, ib, ic]);
            idx.remove(k);
            clipped = true;
            break;
        }
        if !clipped {
            break;
        }
    }
    // Whatever is left (a triangle, or a degenerate remainder) as a fan.
    if let Some((&first, rest)) = idx.split_first() {
        for w in rest.windows(2) {
            if let [b, c] = *w {
                tris.push([first, b, c]);
            }
        }
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ear_clipping_covers_a_concave_polygon() {
        // An L shape: area 3 unit squares.
        let l = [pos2(0.0, 0.0), pos2(2.0, 0.0), pos2(2.0, 1.0), pos2(1.0, 1.0), pos2(1.0, 2.0), pos2(0.0, 2.0)];
        for pts in [l.to_vec(), l.iter().rev().copied().collect::<Vec<_>>()] {
            let t = triangulate(&pts);
            assert_eq!(t.len(), 4);
            let area: f32 = t.iter().map(|[a, b, c]| cross(pts[*a], pts[*b], pts[*c]).abs() / 2.0).sum();
            assert!((area - 3.0).abs() < 1e-4, "{area}");
        }
        assert!(triangulate(&[pos2(0.0, 0.0), pos2(1.0, 1.0)]).is_empty());
        // Collinear points still come back as (degenerate) triangles rather than looping.
        assert!(triangulate(&[pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(2.0, 0.0), pos2(3.0, 0.0)]).len() <= 2);
    }

    #[test]
    fn curves_are_flattened_per_subpath() {
        let cmds = [
            PathCmd::MoveTo(0.0, 0.0),
            PathCmd::QuadTo(1.0, 1.0, 2.0, 0.0),
            PathCmd::Close,
            PathCmd::MoveTo(5.0, 5.0),
            PathCmd::CubicTo(6.0, 6.0, 7.0, 6.0, 8.0, 5.0),
        ];
        let subs = flatten(&cmds, Pos2::ZERO, 10.0);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].len(), 9);
        assert_eq!(subs[1].len(), 11);
        assert_eq!(subs[1].last().copied(), Some(pos2(80.0, 50.0)));
    }
}
