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

/// An imported texture paints a textured mesh along the stroke, and the stroke is snapshotted with it.
#[test]
fn an_imported_texture_paints_a_stroke_and_is_snapshotted() {
    let mut h = harness();
    let bytes = png(32, 32);
    let imported =
        h.state_mut().run("draw.textureImport", json!({"name": "checker.png", "base64": gridcraft_engine::io::base64_encode(&bytes)})).unwrap();
    let id = imported["id"].as_str().unwrap().to_string();
    h.state_mut().run("draw.pen", json!({"texture": id, "textureScale": 2})).unwrap();
    h.state_mut().run("draw.stroke", json!({"points": [[80, 80], [240, 110], [320, 200], [180, 260]], "color": "#1F5FC9", "width": 12})).unwrap();
    h.run_steps(4);

    let meshes = textured_meshes(&h);
    assert_eq!(meshes.len(), 1, "the stroke is painted as one textured mesh");
    assert!(meshes[0].1 >= 12, "a four-point stroke tiles at least three segments");

    // The off-screen snapshot: not a committed file, written where the build keeps its output.
    let image = h.render().expect("the wgpu test renderer draws the frame");
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
    std::fs::create_dir_all(&dir).expect("the target directory exists");
    let path = dir.join("ink-texture-stroke.png");
    image.save(&path).expect("the snapshot is written");
    assert!(path.is_file());
    eprintln!("ink texture snapshot: {}", path.display());
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
