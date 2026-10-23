//! Painting a cell gradient with egui's mesh painter.

use egui::{Color32, Mesh, Painter, Rect, pos2};
use gridcraft_engine::model::{GradientFill, Theme};

/// Paints `gradient` across `rect`.
///
/// The mesh is sampled at a fixed grid. A flat triangle pair cannot represent a multi-stop
/// gradient, and egui has no gradient shader, so a 2-D grid is the smallest form that works for
/// both linear and path gradients.
pub fn paint_rect(painter: &Painter, rect: Rect, gradient: &GradientFill, theme: &Theme) {
    if !(rect.is_finite() && rect.is_positive()) {
        return;
    }
    // Resolve once per cell: sampling every vertex must not allocate.
    let stops = gradient.resolved_stops(theme);
    if stops.is_empty() {
        return;
    }
    // A fixed 16x16 grid is 512 triangles even for a 20 px cell. Roughly one segment per 8 px
    // (clamped to 2..=16) keeps small cells cheap without visible banding.
    let segments = |len: f32| ((len / 8.0).ceil() as usize).clamp(2, 16);
    let (cols, rows) = (segments(rect.width()), segments(rect.height()));
    let mut mesh = Mesh::default();
    mesh.vertices.reserve((cols + 1) * (rows + 1));
    mesh.indices.reserve(cols * rows * 6);
    for y in 0..=rows {
        for x in 0..=cols {
            let u = x as f32 / cols as f32;
            let v = y as f32 / rows as f32;
            let Some(rgb) = gradient.sample_resolved(&stops, u as f64, v as f64) else { return };
            let pos = pos2(rect.left() + rect.width() * u, rect.top() + rect.height() * v);
            mesh.colored_vertex(pos, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
    }
    for y in 0..rows {
        for x in 0..cols {
            let row = cols + 1;
            let i = y * row + x;
            mesh.add_triangle(i as u32, (i + 1) as u32, (i + row) as u32);
            mesh.add_triangle((i + 1) as u32, (i + row + 1) as u32, (i + row) as u32);
        }
    }
    painter.add(mesh);
}
