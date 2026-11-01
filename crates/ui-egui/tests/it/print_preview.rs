//! File › Print previews the page as it prints: cell borders, Center on Page and merged-cell
//! alignment match the PDF (#3059).
use egui::{Color32, Context, Pos2, RawInput, Rect, Shape, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, theme};
use serde_json::{Value, json};

fn frame(ctx: &Context, app: &mut SheetApp) -> egui::FullOutput {
    let mut output =
        ctx.run_ui(RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 900.0))), ..Default::default() }, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        });
    output.textures_delta.clear();
    output
}

/// What the print preview paints for a sheet with boxed cells A1:C3, B5:D7 merged and centred
/// with a thick outline, and (`centred`) Center on Page both ways.
struct Painted {
    /// The paper on screen.
    paper: Rect,
    /// Screen pixels per point.
    per_pt: f32,
    /// Black line segments inside the paper.
    lines: Vec<[Pos2; 2]>,
    /// Text drawn inside the paper: (text, its screen rectangle).
    texts: Vec<(String, Rect)>,
    /// `file.printPreview`'s page 1 with its cells.
    page: Value,
}

fn preview(centred: bool) -> Painted {
    let ctx = Context::default();
    theme::apply(&ctx, false);
    let mut session = Session::new();
    session.new_workbook();
    let mut app = crate::common::app(session);
    for (id, params) in [
        ("cell.set", json!({"cell": "A1", "input": "Name"})),
        ("cell.set", json!({"cell": "C3", "input": "42"})),
        ("home.borders", json!({"range": "A1:C3", "preset": "all"})),
        ("cell.set", json!({"cell": "B5", "input": "Merged"})),
        ("home.mergeCenter", json!({"range": "B5:D7"})),
        ("home.alignMiddle", json!({"range": "B5:D7"})),
        ("home.borders", json!({"range": "B5:D7", "preset": "thickOutside"})),
        ("pageLayout.pageSetup", json!({"centerH": centred, "centerV": centred})),
    ] {
        app.run(id, params).unwrap();
    }
    let page = app.run("file.printPreview", json!({"page": 1, "cells": true})).unwrap()["page"].clone();
    app.run("ui.backstage", json!({"page": "print"})).unwrap();
    for _ in 0..4 {
        frame(&ctx, &mut app);
    }
    let output = frame(&ctx, &mut app);
    let (w, h) = (page["width"].as_f64().unwrap() as f32, page["height"].as_f64().unwrap() as f32);
    let paper = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            Shape::Rect(r) if r.fill == Color32::WHITE && r.rect.width() > 200.0 && (r.rect.width() / r.rect.height() - w / h).abs() < 0.01 => {
                Some(r.rect)
            }
            _ => None,
        })
        .expect("the paper is painted");
    let mut lines = Vec::new();
    let mut texts = Vec::new();
    for s in &output.shapes {
        match &s.shape {
            Shape::LineSegment { points, stroke } if stroke.color == Color32::BLACK && points.iter().all(|p| paper.expand(1.0).contains(*p)) => {
                lines.push(*points)
            }
            Shape::Text(t) if paper.contains(t.pos) => texts.push((t.galley.text().to_string(), t.visual_bounding_rect())),
            _ => {}
        }
    }
    Painted { paper, per_pt: paper.width() / w, lines, texts, page }
}

fn cell<'a>(page: &'a Value, a1: &str) -> &'a Value {
    page["cells"].as_array().unwrap().iter().find(|c| c["cell"] == a1).unwrap()
}

/// A cell's rectangle on screen.
fn on_screen(p: &Painted, c: &Value) -> Rect {
    let f = |k: &str| c[k].as_f64().unwrap() as f32 * p.per_pt;
    Rect::from_min_size(p.paper.min + vec2(f("x"), f("y")), vec2(f("w"), f("h")))
}

fn text<'a>(p: &'a Painted, s: &str) -> &'a Rect {
    &p.texts.iter().find(|(t, _)| t == s).unwrap_or_else(|| panic!("`{s}` is painted: {:?}", p.texts)).1
}

#[test]
fn the_preview_draws_the_cell_borders_that_print() {
    let p = preview(false);
    // A1:C3 has all borders, B5:D7 an outline: every edge of each is drawn.
    let a1 = on_screen(&p, cell(&p.page, "A1"));
    let has = |a: Pos2, b: Pos2| {
        p.lines.iter().any(|l| (l[0].distance(a) < 0.5 && l[1].distance(b) < 0.5) || (l[0].distance(b) < 0.5 && l[1].distance(a) < 0.5))
    };
    assert!(has(a1.left_top(), a1.right_top()) && has(a1.left_bottom(), a1.right_bottom()), "A1's top and bottom: {:?}", p.lines);
    assert!(has(a1.left_top(), a1.left_bottom()) && has(a1.right_top(), a1.right_bottom()), "A1's left and right");
    let m = on_screen(&p, cell(&p.page, "B5"));
    assert!(has(m.right_top(), m.right_bottom()) && has(m.left_bottom(), m.right_bottom()), "the merged range's right and bottom edges");
    assert!(p.lines.len() >= 9 * 4 + 4, "{} border lines", p.lines.len());
}

#[test]
fn the_preview_centres_the_page_content_as_it_prints() {
    let p = preview(true);
    // The content's left edge is where the PDF puts it, well inside the 0.7" margin.
    let content_x = p.page["content"]["x"].as_f64().unwrap() as f32;
    assert!(content_x > 0.7 * 72.0 + 100.0, "centred content: {}", p.page["content"]);
    let name = text(&p, "Name");
    let a1 = on_screen(&p, cell(&p.page, "A1"));
    assert!(a1.left() - p.paper.left() > (0.7 * 72.0 + 100.0) * p.per_pt, "A1 is moved right by Center on Page");
    assert!(name.left() >= a1.left() && name.left() < a1.left() + 4.0, "Name starts in A1: {name:?} vs {a1:?}");
    assert!(name.top() >= a1.top() && name.bottom() <= a1.bottom() + 1.0, "Name is in A1's row: {name:?} vs {a1:?}");
}

#[test]
fn the_preview_centres_merged_text_over_the_merged_range() {
    let p = preview(false);
    let m = on_screen(&p, cell(&p.page, "B5"));
    let merged = text(&p, "Merged");
    assert!((merged.center().x - m.center().x).abs() < 1.0, "centred across B5:D7: {merged:?} in {m:?}");
    assert!((merged.center().y - m.center().y).abs() < 2.0, "centred down B5:D7: {merged:?} in {m:?}");
}
