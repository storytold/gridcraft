//! An imported texture paints a stroke: the committed ink is a mesh that tiles the image, and an
//! off-screen snapshot shows it.

use egui::{Shape, TextureId};
use gridcraft_ui_egui::SheetApp;
use serde_json::json;

/// A two-colour pattern, so a painted stroke shows real repeats rather than a flat fill.
fn png(w: u32, h: u32) -> Vec<u8> {
    // A two-colour pattern, so the render shows actual repeats rather than a flat fill.
    let image =
        image::RgbaImage::from_fn(
            w,
            h,
            |x, y| {
                if (x / 4 + y / 4) % 2 == 0 { image::Rgba([240, 60, 40, 255]) } else { image::Rgba([20, 100, 240, 255]) }
            },
        );
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

/// A blank four-column sheet harness to draw ink on.
fn harness() -> egui_kittest::Harness<'static, SheetApp> {
    crate::support::blank_harness(4)
}

/// Every textured mesh in the last frame, with its vertex count.
fn textured_meshes(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<(TextureId, usize)> {
    h.output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Mesh(mesh) if mesh.texture_id != TextureId::default() && mesh.vertices.len() >= 4 => Some((mesh.texture_id, mesh.vertices.len())),
            _ => None,
        })
        .collect()
}

/// Counts the pixels of the two pattern colours inside `rect` (in pixels): the texture is a
/// red-and-blue checker, so a stroke painted with it shows both, while a plain stroke shows
/// neither. Sampling only the stroke's own rectangle keeps the ribbon's pen tiles out of the count.
fn pattern_pixels(image: &image::RgbaImage, rect: egui::Rect) -> (usize, usize) {
    let (mut red, mut blue) = (0, 0);
    let x0 = rect.left().floor().max(0.0) as u32;
    let y0 = rect.top().floor().max(0.0) as u32;
    let x1 = (rect.right().ceil() as u32).min(image.width());
    let y1 = (rect.bottom().ceil() as u32).min(image.height());
    for y in y0..y1 {
        for x in x0..x1 {
            let [r, g, b, a] = image.get_pixel(x, y).0;
            if a < 200 {
                continue;
            }
            if r > 180 && g < 100 && b < 100 {
                red += 1;
            } else if b > 180 && r < 100 && g < 140 {
                blue += 1;
            }
        }
    }
    (red, blue)
}

/// The screen rectangle a textured mesh covers, in points.
fn textured_mesh_bounds(h: &egui_kittest::Harness<'static, SheetApp>) -> Option<egui::Rect> {
    h.output().shapes.iter().find_map(|s| match &s.shape {
        Shape::Mesh(mesh) if mesh.texture_id != TextureId::default() && mesh.vertices.len() >= 4 => {
            Some(mesh.vertices.iter().fold(egui::Rect::NOTHING, |rect, v| rect.union(egui::Rect::from_min_size(v.pos, egui::Vec2::ZERO))))
        }
        _ => None,
    })
}

/// An imported texture paints a textured mesh along the stroke, and the rendered frame really
/// shows both of the pattern's colours; the snapshot goes to a file of this test's own.
#[test]
fn an_imported_texture_paints_a_stroke_and_is_snapshotted() {
    let mut h = harness();
    let bytes = png(32, 32);
    let imported =
        h.state_mut().run("draw.textureImport", json!({"name": "checker.png", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    let id = imported["id"].as_str().unwrap().to_string();
    h.state_mut().run("draw.pen", json!({"texture": id, "textureScale": 2})).unwrap();
    // A white pen leaves the pattern's own colours in the render, so the assertion below is about
    // the texture, not the tint.
    h.state_mut().run("draw.stroke", json!({"points": [[80, 80], [240, 110], [320, 200], [180, 260]], "color": "#FFFFFF", "width": 12})).unwrap();
    h.run_steps(4);

    let meshes = textured_meshes(&h);
    assert_eq!(meshes.len(), 1, "the stroke is painted as one textured mesh");
    assert!(meshes[0].1 >= 12, "a four-point stroke tiles at least three segments");

    let image = h.render().expect("the wgpu test renderer draws the frame");
    let bounds = textured_mesh_bounds(&h).expect("the textured mesh has a screen rectangle");
    // The render is in physical pixels; the mesh is in points.
    let ppp = h.ctx.pixels_per_point();
    let stroke_rect =
        egui::Rect::from_min_max(egui::pos2(bounds.left() * ppp, bounds.top() * ppp), egui::pos2(bounds.right() * ppp, bounds.bottom() * ppp));
    let (red, blue) = pattern_pixels(&image, stroke_rect);
    assert!(red > 20 && blue > 20, "the rendered stroke shows the pattern's two colours: {red} red, {blue} blue");
    // Without the texture the same rectangle paints plainly: no pattern colours at all.
    h.state_mut().run("draw.textureRemove", json!({"id": id})).unwrap();
    h.run_steps(3);
    let plain = h.render().expect("the second frame renders");
    let (red, blue) = pattern_pixels(&plain, stroke_rect);
    assert_eq!((red, blue), (0, 0), "a removed texture leaves a plain stroke");

    // The snapshot: a file of this test's own, not a name shared with every run of the suite.
    let path = std::env::temp_dir().join(format!("gridcraft-ink-texture-stroke-{}.png", std::process::id()));
    image.save(&path).expect("the snapshot is written");
    assert!(path.is_file());
    let _ = std::fs::remove_file(&path);
}

/// A stroke whose texture was removed paints as a plain line rather than disappearing.
#[test]
fn a_removed_texture_falls_back_to_a_plain_stroke() {
    let mut h = harness();
    let bytes = png(16, 16);
    let id = h.state_mut().run("draw.textureImport", json!({"name": "a.png", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.state_mut().run("draw.stroke", json!({"points": [[80, 80], [240, 110], [320, 200]], "texture": id, "width": 8})).unwrap();
    h.run_steps(2);
    assert_eq!(textured_meshes(&h).len(), 1);
    h.state_mut().run("draw.textureRemove", json!({"id": id})).unwrap();
    h.run_steps(2);
    assert!(textured_meshes(&h).is_empty(), "the stroke paints plainly once its texture is gone");
    let has_plain_ink = h.output().shapes.iter().any(|s| matches!(&s.shape, Shape::Path(p) if p.points.len() == 3));
    assert!(has_plain_ink);
}

/// The vertical extent of each textured mesh in the last frame, smallest first.
fn textured_mesh_extents(h: &egui_kittest::Harness<'static, SheetApp>) -> Vec<f32> {
    let mut extents: Vec<f32> = h
        .output()
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            Shape::Mesh(mesh) if mesh.texture_id != TextureId::default() && mesh.vertices.len() >= 4 => {
                let (lo, hi) = mesh.vertices.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| (lo.min(v.pos.y), hi.max(v.pos.y)));
                Some(hi - lo)
            }
            _ => None,
        })
        .collect();
    extents.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    extents
}

/// A textured highlighter commits as wide as its preview, not at the narrower pen width.
#[test]
fn a_textured_highlighter_commits_as_wide_as_its_preview() {
    // The preview measures a highlighter at its height (twice the pen width); the committed
    // textured branch measured every tool at its pen width, so the image narrowed on release.
    let mut h = harness();
    let id = h
        .state_mut()
        .run("draw.textureImport", json!({"name": "tiles.png", "base64": gridcraft_engine::io::base64_encode(&png(16, 16))}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.state_mut().run("draw.stroke", json!({"points": [[80, 80], [280, 80]], "texture": id, "width": 6, "brush": "pen"})).unwrap();
    h.state_mut().run("draw.stroke", json!({"points": [[80, 160], [280, 160]], "texture": id, "width": 6, "brush": "highlighter"})).unwrap();
    h.run_steps(4);
    let extents = textured_mesh_extents(&h);
    assert_eq!(extents.len(), 2, "both strokes are painted as textured meshes");
    assert!((extents[1] / extents[0] - 2.0).abs() < 0.01, "a highlighter paints at its height, twice the pen width: {extents:?}");
}

/// A textured pencil commits as narrow as its preview, not at the full pen width.
#[test]
fn a_textured_pencil_commits_as_narrow_as_its_preview() {
    // The preview measures a pencil at 0.8 of its width; the committed textured branch must use
    // the same across width, or the image widens on release.
    let mut h = harness();
    let id = h
        .state_mut()
        .run("draw.textureImport", json!({"name": "tiles.png", "base64": gridcraft_engine::io::base64_encode(&png(16, 16))}))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    h.state_mut().run("draw.stroke", json!({"points": [[80, 80], [280, 80]], "texture": id, "width": 10, "brush": "pen"})).unwrap();
    h.state_mut().run("draw.stroke", json!({"points": [[80, 160], [280, 160]], "texture": id, "width": 10, "brush": "pencil"})).unwrap();
    h.run_steps(4);
    let extents = textured_mesh_extents(&h);
    assert_eq!(extents.len(), 2, "both strokes are painted as textured meshes");
    assert!((extents[1] / extents[0] - 0.8).abs() < 0.01, "a pencil paints at 0.8 of the pen width: {extents:?}");
}
