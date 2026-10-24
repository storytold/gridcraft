//! Standalone chart SVG and lossless PNG, including text. No worksheet chrome.
use crate::{Chart, ChartData, HAlign, Measure, Prim, Rgba, VAlign, render};
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};

#[cfg(any(target_arch = "wasm32", test))]
fn load_bundled_fonts(db: &mut resvg::usvg::fontdb::Database) {
    for bytes in [epaint_default_fonts::UBUNTU_LIGHT, epaint_default_fonts::HACK_REGULAR, epaint_default_fonts::NOTO_EMOJI_REGULAR] {
        db.load_font_data(bytes.to_vec());
    }
    db.set_sans_serif_family("Ubuntu");
    db.set_monospace_family("Hack");
}

fn font_database() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            #[cfg(not(target_arch = "wasm32"))]
            db.load_system_fonts();
            #[cfg(target_arch = "wasm32")]
            load_bundled_fonts(&mut db);
            // Chart unit tests can run on minimal CI hosts too. The normal native library
            // still reports missing system fonts; the engine integration test covers that case.
            #[cfg(all(not(target_arch = "wasm32"), test))]
            if db.faces().next().is_none() {
                load_bundled_fonts(&mut db);
            }
            if let Some(family) = ["DejaVu Sans", "Arial", "Liberation Sans", "Helvetica", "Noto Sans"]
                .into_iter()
                .find(|name| db.faces().any(|face| face.families.iter().any(|(f, _)| f == name)))
                .map(str::to_owned)
                .or_else(|| db.faces().find_map(|face| face.families.first().map(|(name, _)| name.clone())))
            {
                db.set_sans_serif_family(family);
            }
            Arc::new(db)
        })
        .clone()
}

fn esc(text: &str) -> String {
    text.chars()
        .filter(|c| *c == '\n' || *c == '\r' || *c == '\t' || (*c >= ' ' && *c != '\u{fffe}' && *c != '\u{ffff}'))
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '"' => "&quot;".into(),
            '\'' => "&apos;".into(),
            _ => c.to_string(),
        })
        .collect()
}
fn color(c: Rgba) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}
fn fill(c: Option<Rgba>) -> String {
    c.map(|c| format!("fill=\"{}\" fill-opacity=\"{}\"", color(c), c[3] as f32 / 255.0)).unwrap_or_else(|| "fill=\"none\"".into())
}
fn stroke(s: Option<(Rgba, f32)>) -> String {
    s.filter(|(_, w)| *w > 0.0)
        .map(|(c, w)| format!("stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{w}\"", color(c), c[3] as f32 / 255.0))
        .unwrap_or_else(|| "stroke=\"none\"".into())
}

pub fn export_svg(chart: &Chart, data: &ChartData, measure: &dyn Measure, scale: f32, transparent: bool) -> Result<String, String> {
    let (w, h) = (chart.anchor.width, chart.anchor.height);
    if !w.is_finite() || !h.is_finite() || w < 1.0 || h < 1.0 || w > 1e5 || h > 1e5 || !scale.is_finite() || !(0.25..=8.0).contains(&scale) {
        return Err("Invalid chart export size".into());
    }
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {w} {h}\"><defs><clipPath id=\"chartClip\"><rect width=\"{w}\" height=\"{h}\"/></clipPath></defs><g clip-path=\"url(#chartClip)\">",
        w * scale,
        h * scale
    );
    let mut export_chart = chart.clone();
    if transparent {
        let transparent_fill = Some([0, 0, 0, 0]);
        export_chart.format.chart_area.fill = transparent_fill;
        export_chart.format.plot_area.fill = transparent_fill;
    }
    for primitive in render(&export_chart, data, w, h, measure) {
        match primitive {
            Prim::Rect { x, y, w, h, fill: f, stroke: s, radius } => {
                let _ = write!(svg, "<rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"{h}\" rx=\"{radius}\" {} {}/>", fill(f), stroke(s));
            }
            Prim::Circle { cx, cy, r, fill: f, stroke: s } => {
                let _ = write!(svg, "<circle cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\" {} {}/>", fill(f), stroke(s));
            }
            Prim::Line { pts, color: c, width, dash } => {
                let points = pts.iter().map(|p| format!("{},{}", p[0], p[1])).collect::<Vec<_>>().join(" ");
                let _ = write!(
                    svg,
                    "<polyline points=\"{points}\" fill=\"none\" {} {}/>",
                    stroke(Some((c, width))),
                    if dash { "stroke-dasharray=\"6 4\"" } else { "" }
                );
            }
            Prim::Polygon { pts, fill: f, stroke: s } => {
                let points = pts.iter().map(|p| format!("{},{}", p[0], p[1])).collect::<Vec<_>>().join(" ");
                let _ = write!(svg, "<polygon points=\"{points}\" {} {}/>", fill(Some(f)), stroke(s));
            }
            Prim::Wedge { cx, cy, r_outer, r_inner, a0, a1, fill: f, stroke: s } => {
                let at = |a: f32, r: f32| (cx + r * a.sin(), cy - r * a.cos());
                let n = ((a1 - a0).abs() / std::f32::consts::PI).ceil().clamp(1.0, 4.0) as usize;
                let (x, y) = at(a0, r_outer);
                let mut path = format!("M{x} {y}");
                let clockwise = usize::from(a1 >= a0);
                for i in 1..=n {
                    let (x, y) = at(a0 + (a1 - a0) * i as f32 / n as f32, r_outer);
                    let _ = write!(path, " A{r_outer} {r_outer} 0 0 {clockwise} {x} {y}");
                }
                if r_inner > 0.0 {
                    let (x, y) = at(a1, r_inner);
                    let _ = write!(path, " L{x} {y}");
                    for i in 1..=n {
                        let (x, y) = at(a1 - (a1 - a0) * i as f32 / n as f32, r_inner);
                        let _ = write!(path, " A{r_inner} {r_inner} 0 0 {} {x} {y}", 1 - clockwise);
                    }
                } else {
                    let _ = write!(path, " L{cx} {cy}");
                }
                let _ = write!(svg, "<path d=\"{path} Z\" {} {}/>", fill(Some(f)), stroke(s));
            }
            Prim::Text { x, y, text, size, color: c, bold, format, align, valign, rotation, .. } => {
                let _ = write!(
                    svg,
                    "<text x=\"{x}\" y=\"{y}\" font-family=\"{}\" font-size=\"{size}\" font-weight=\"{}\" font-style=\"{}\" text-anchor=\"{}\" dominant-baseline=\"{}\" transform=\"rotate({} {x} {y})\" {}>{}</text>",
                    esc(format.name.as_deref().unwrap_or("sans-serif")),
                    if bold { "bold" } else { "normal" },
                    if format.italic.unwrap_or(false) { "italic" } else { "normal" },
                    match align {
                        HAlign::Left => "start",
                        HAlign::Center => "middle",
                        HAlign::Right => "end",
                    },
                    match valign {
                        VAlign::Top => "text-before-edge",
                        VAlign::Middle => "central",
                        VAlign::Bottom => "text-after-edge",
                    },
                    rotation.to_degrees(),
                    fill(Some(c)),
                    esc(&text)
                );
            }
        }
    }
    svg.push_str("</g></svg>");
    Ok(svg)
}

pub fn export_png(chart: &Chart, data: &ChartData, measure: &dyn Measure, scale: f32, transparent: bool) -> Result<Vec<u8>, String> {
    let svg = export_svg(chart, data, measure, scale, transparent)?;
    let width = (chart.anchor.width * scale).ceil() as u32;
    let height = (chart.anchor.height * scale).ceil() as u32;
    if width > 8192 || height > 8192 || u64::from(width) * u64::from(height) > 16_777_216 {
        return Err("PNG export is limited to 16 megapixels and 8192 pixels per side".into());
    }
    let options = resvg::usvg::Options { fontdb: font_database(), ..Default::default() };
    if options.fontdb.faces().next().is_none() {
        return Err("PNG export needs installed fonts; use SVG on this platform".into());
    }
    let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
    let mut pixels = resvg::tiny_skia::Pixmap::new(width, height).ok_or("Could not allocate PNG image")?;
    resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixels.as_mut());
    pixels.encode_png().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{font_database, load_bundled_fonts};
    use std::sync::Arc;

    #[test]
    fn font_database_is_shared_between_exports() {
        assert!(Arc::ptr_eq(&font_database(), &font_database()));
    }

    #[test]
    fn bundled_fonts_rasterize_text_without_system_fonts() {
        let mut db = resvg::usvg::fontdb::Database::new();
        load_bundled_fonts(&mut db);
        let options = resvg::usvg::Options { fontdb: Arc::new(db), ..Default::default() };
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="40"><text x="2" y="25" font-family="sans-serif" font-size="20">Chart 123</text></svg>"#;
        let tree = resvg::usvg::Tree::from_str(svg, &options).unwrap();
        let mut pixels = resvg::tiny_skia::Pixmap::new(120, 40).unwrap();
        resvg::render(&tree, resvg::tiny_skia::Transform::identity(), &mut pixels.as_mut());
        assert!(pixels.pixels().iter().any(|pixel| pixel.alpha() > 0));
    }
}
