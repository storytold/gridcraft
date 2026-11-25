//! Native grid benchmark. Use `cargo xtask renderer-bench --help` for the guarded workflow.
use std::error::Error;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use egui::{FontData, FontDefinitions, FontFamily, Shape, vec2};
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_ui_egui::{SheetApp, grid, theme};
use serde_json::{Value, json};
use skrifa::MetadataProvider;

#[path = "renderer_tools/provenance.rs"]
mod provenance;

const NOTO_SHA256: &str = "5a1c559bb539583c8a1fd99d1c5b9491e5e14478c9cd2bd0970d5c3096cc9ef8";
const CELLS: usize = 500;
type Harness = egui_kittest::Harness<'static, SheetApp>;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Options {
    out: PathBuf,
    script: String,
    font: Option<PathBuf>,
    font_hash: String,
    configs: Vec<(u16, u16)>,
    cold: usize,
    warm: usize,
    snapshots: bool,
}

fn count(value: &str, minimum: usize) -> Result<usize> {
    let n: usize = value.parse()?;
    if !(minimum..=1000).contains(&n) {
        return Err(format!("sample count must be {minimum}..1000").into());
    }
    Ok(n)
}

fn configuration(value: &str) -> Result<Vec<(u16, u16)>> {
    let all = vec![(50, 1), (50, 2), (100, 1), (100, 2), (200, 1), (200, 2)];
    if value == "all" {
        return Ok(all);
    }
    let (zoom, dpi) = value.split_once('x').ok_or("configuration must look like 100x1")?;
    let pair = (zoom.parse()?, dpi.parse()?);
    if !all.contains(&pair) {
        return Err("unsupported zoom/DPI configuration".into());
    }
    Ok(vec![pair])
}

fn options(args: impl Iterator<Item = String>) -> Result<Options> {
    let mut opts = Options {
        out: PathBuf::new(),
        script: "latin".into(),
        font: None,
        font_hash: NOTO_SHA256.into(),
        configs: configuration("100x1")?,
        cold: 30,
        warm: 100,
        snapshots: false,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        if arg == "--snapshots" {
            opts.snapshots = true;
            continue;
        }
        let value = args.next().ok_or_else(|| format!("missing value after {arg}"))?;
        match arg.as_str() {
            "--out" => opts.out = value.into(),
            "--script" if matches!(value.as_str(), "latin" | "thai" | "mixed") => opts.script = value,
            "--font" => opts.font = Some(value.into()),
            "--font-sha256" => opts.font_hash = value.to_ascii_lowercase(),
            "--config" => opts.configs = configuration(&value)?,
            "--cold" => opts.cold = count(&value, 1)?,
            "--warm" => opts.warm = count(&value, 2)?,
            _ => return Err(format!("unknown option or invalid value: {arg} {value}").into()),
        }
    }
    if opts.out.as_os_str().is_empty() {
        return Err("use cargo xtask renderer-bench (missing --out)".into());
    }
    if opts.font_hash.len() != 64 || !opts.font_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("font SHA-256 must contain 64 hexadecimal digits".into());
    }
    if opts.script != "latin" && opts.font.is_none() {
        opts.font = std::env::var_os("CRAFT_FONTS_DIR").map(|p| PathBuf::from(p).join("fonts/noto-sans-thai/NotoSansThai.ttf"));
        if opts.font.is_none() {
            return Err("Thai/mixed requires --font or CRAFT_FONTS_DIR; see docs/renderer-benchmarks.md".into());
        }
    }
    Ok(opts)
}

fn workload(script: &str) -> Vec<String> {
    let words = match script {
        "thai" => "วันนี้อากาศดีมาก ข้อมูลยอดขายรายเดือน ทดสอบการตัดบรรทัด ",
        "mixed" => "วันนี้ Total 123.45 บาท รายการ ABC ทดสอบการตัดบรรทัด ",
        _ => "Quarterly revenue forecast inventory customer invoice total ",
    };
    let words: Vec<_> = words.split_whitespace().collect();
    (0..CELLS)
        .map(|id| {
            let mut text = format!("T{id:03} ");
            let mut state = id as u64 ^ 0x4752_4944_4352_4146;
            while text.chars().count() < 80 {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let index = (state % words.len().max(1) as u64) as usize;
                text.push_str(words.get(index).copied().unwrap_or("text"));
                text.push(' ');
            }
            text.chars().take(80).collect()
        })
        .collect()
}

fn fonts(opts: &Options, texts: &[String]) -> Result<(FontDefinitions, Value)> {
    let mut defs = FontDefinitions::default();
    let mut chain = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut evidence = json!({"fixture": "egui bundled defaults; identified by Cargo.lock"});
    if let Some(path) = &opts.font {
        let mut bytes = Vec::new();
        fs::File::open(path)?.take(32 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 32 * 1024 * 1024 {
            return Err("font exceeds 32 MiB".into());
        }
        let digest = provenance::bytes_hash(&bytes);
        if digest != opts.font_hash {
            return Err(format!("font SHA-256 mismatch: expected {}, got {digest}", opts.font_hash).into());
        }
        let face = skrifa::FontRef::from_index(&bytes, 0)?;
        let map = face.charmap();
        for ch in texts.iter().flat_map(|s| s.chars()) {
            if map.map(ch).is_none() {
                return Err(format!("fixture font has no glyph for {ch:?}").into());
            }
        }
        evidence = json!({"sha256": digest, "bytes": bytes.len(), "face_index": 0, "workload_glyph_coverage": true});
        defs.font_data.insert("renderer-fixture".into(), Arc::new(FontData::from_owned(bytes)));
        chain.insert(0, "renderer-fixture".into());
    }
    for role in [theme::CELL, theme::CELL_BOLD, theme::CELL_ITALIC, theme::CELL_BOLD_ITALIC, theme::UI, theme::UI_BOLD, theme::SERIF, theme::MONO] {
        defs.families.insert(FontFamily::Name(role.into()), chain.clone());
    }
    let mut faces: Vec<_> = defs
        .font_data
        .iter()
        .map(|(name, data)| (name.clone(), provenance::bytes_hash(data.font.as_ref().as_ref()), data.index, format!("{:?}", data.tweak)))
        .collect();
    faces.sort();
    let mut families: Vec<_> = defs.families.iter().map(|(family, chain)| (format!("{family:?}"), chain.clone())).collect();
    families.sort();
    evidence["font_set_sha256"] = json!(provenance::bytes_hash(&serde_json::to_vec(&(faces, families))?));
    Ok((defs, evidence))
}

fn app(config: (u16, u16), texts: &[String]) -> Result<SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("home.columnWidth", json!({"cols": "A:T", "width": 84}))?;
    session.execute("home.rowHeight", json!({"rows": "1:25", "height": 52}))?;
    for (id, text) in texts.iter().enumerate() {
        session.execute("cell.set", json!({"cell": CellRef::new((id / 20) as u32, (id % 20) as u32).a1(), "input": text}))?;
    }
    session.execute("home.formatCells", json!({"range": "A1:T25", "style": {"align": {"wrap": true, "v": "Top"}}}))?;
    session.execute("view.zoom", json!({"percent": config.0}))?;
    let mut app = SheetApp::new(session, Default::default());
    app.use_prepared_fonts();
    Ok(app)
}

fn viewport(config: (u16, u16)) -> egui::Vec2 {
    let z = f32::from(config.0) * 0.008;
    vec2(20.0 * 84.0 * z + 200.0, 25.0 * 52.0 * z + 200.0)
}

fn harness(config: (u16, u16), texts: &[String], fonts: &FontDefinitions) -> Result<(Harness, Arc<AtomicBool>)> {
    let visible = Arc::new(AtomicBool::new(false));
    let draw = Arc::clone(&visible);
    let fonts = fonts.clone();
    let harness = Harness::builder()
        .with_size(viewport(config))
        .with_pixels_per_point(f32::from(config.1))
        .with_accessibility_check(false)
        .with_context_setup(move |ctx| {
            ctx.set_fonts(fonts.clone());
            theme::apply(ctx, false);
        })
        .build_ui_state(
            move |ui, app: &mut SheetApp| {
                app.logic(ui.ctx());
                if draw.load(Ordering::Acquire) {
                    grid::show(app, ui);
                }
            },
            app(config, texts)?,
        );
    Ok((harness, visible))
}

/// Serialize viewport and cell geometry independently of painted text.
fn geometry_bytes(h: &Harness) -> Result<Vec<u8>> {
    let app = h.state();
    let sheet = app.session.active().and_then(|d| d.wb.active()).ok_or("missing benchmark sheet")?;
    let geo = app.grid.geo.as_ref().ok_or("missing Grid Geo")?;
    let mut bytes = Vec::with_capacity((CELLS + 2) * 16);
    for rect in
        [geo.rect, geo.cells].into_iter().chain((0..CELLS).map(|id| geo.screen_cell_rect(sheet, CellRef::new((id / 20) as u32, (id % 20) as u32))))
    {
        for n in [rect.left(), rect.top(), rect.right(), rect.bottom()] {
            bytes.extend_from_slice(&n.to_bits().to_le_bytes());
        }
    }
    Ok(bytes)
}

/// Hash painted output and prove every generated cell is visible, outside the timed interval.
fn evidence(h: &Harness, texts: &[String]) -> Result<String> {
    let app = h.state();
    let sheet = app.session.active().and_then(|d| d.wb.active()).ok_or("missing benchmark sheet")?;
    let geo = app.grid.geo.as_ref().ok_or("renderer did not produce Grid Geo")?;
    let mut seen = [false; CELLS];
    let mut output = geometry_bytes(h)?;
    for clipped in &h.output().shapes {
        if let Shape::Text(text) = &clipped.shape {
            let string = &text.galley.job.text;
            output.extend_from_slice(&(string.len() as u64).to_le_bytes());
            output.extend_from_slice(string.as_bytes());
            for n in
                [text.pos.x, text.pos.y, clipped.clip_rect.left(), clipped.clip_rect.top(), clipped.clip_rect.right(), clipped.clip_rect.bottom()]
            {
                output.extend_from_slice(&n.to_bits().to_le_bytes());
            }
            if !string.starts_with('T') {
                continue;
            }
            let Some(id) = string.get(1..4).and_then(|s| s.parse::<usize>().ok()) else {
                continue;
            };
            let Some(expected) = texts.get(id) else {
                continue;
            };
            if string.get(..4) != expected.get(..4) {
                continue;
            }
            let rect = geo.screen_cell_rect(sheet, CellRef::new((id / 20) as u32, (id % 20) as u32));
            if !geo.cells.contains_rect(rect) || !rect.intersects(clipped.clip_rect) || !rect.expand(0.5).contains(text.pos) {
                return Err(format!("cell {id} is not fully visible or has invalid text geometry").into());
            }
            if let Some(slot) = seen.get_mut(id) {
                *slot = true;
            }
        }
    }
    if seen.iter().filter(|v| **v).count() != CELLS {
        return Err("renderer did not paint all 500 expected cell prefixes".into());
    }
    Ok(provenance::bytes_hash(&output))
}

// Profiling clock only; report/calendar timestamps are supplied by xtask through chrono.
fn step(h: &mut Harness, texts: &[String]) -> Result<(u64, String)> {
    if cfg!(target_arch = "wasm32") {
        return Err("renderer measurements require a native host".into());
    }
    #[allow(clippy::disallowed_methods)] // Guarded native profiling clock; never executed on wasm.
    let started = Instant::now();
    h.step();
    let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
    Ok((elapsed, evidence(h, texts)?))
}

fn percentile(samples: &[u64], percent: usize) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let index = sorted.len().saturating_mul(percent).div_ceil(100).saturating_sub(1);
    sorted.get(index).copied().unwrap_or_default()
}

fn measure(opts: &Options, config: (u16, u16), texts: &[String], fonts: &FontDefinitions) -> Result<Value> {
    let mut cold = Vec::with_capacity(opts.cold);
    for _ in 0..opts.cold {
        let (mut h, visible) = harness(config, texts, fonts)?;
        visible.store(true, Ordering::Release);
        cold.push(step(&mut h, texts)?.0);
    }
    let (mut h, visible) = harness(config, texts, fonts)?;
    visible.store(true, Ordering::Release);
    let mut last = String::new();
    let mut stable = 0usize;
    let mut passes = 0usize;
    while stable < 3 && passes < 256 {
        let hash = step(&mut h, texts)?.1;
        stable = if hash == last { stable + 1 } else { 0 };
        last = hash;
        passes += 1;
    }
    if stable < 3 {
        return Err("painted output did not stabilize within 256 warmup passes".into());
    }
    let mut warm = Vec::with_capacity(opts.warm);
    for _ in 0..opts.warm {
        let (ns, hash) = step(&mut h, texts)?;
        if hash != last {
            return Err("painted output changed during warm samples; discard this run".into());
        }
        warm.push(ns);
    }
    if opts.snapshots {
        // A separate GPU harness; setup/render/PNG encoding never enter a timing interval.
        let mut visual = egui_kittest::Harness::builder()
            .with_size(viewport(config))
            .with_pixels_per_point(f32::from(config.1))
            .with_accessibility_check(false)
            .with_context_setup({
                let fonts = fonts.clone();
                move |ctx| {
                    ctx.set_fonts(fonts.clone());
                    theme::apply(ctx, false);
                }
            })
            .wgpu()
            .build_ui_state(
                |ui, app: &mut SheetApp| {
                    app.logic(ui.ctx());
                    grid::show(app, ui);
                },
                app(config, texts)?,
            );
        visual.run_steps(passes.saturating_add(1));
        evidence(&visual, texts)?;
        let dir = opts.out.parent().ok_or("output has no parent")?;
        visual.render()?.save(dir.join(format!("grid-{}x{}.png", config.0, config.1)))?;
    }
    Ok(json!({"zoom_percent": config.0, "dpi": config.1, "visible_cells": CELLS, "warmup_passes": passes,
        "cell_geometry_sha256": provenance::bytes_hash(&geometry_bytes(&h)?),
        "output_sha256": last, "cold_first_pass_ns": cold, "warm_pass_ns": warm,
        "cold_p50_ns": percentile(&cold, 50), "cold_p95_ns": percentile(&cold, 95),
        "warm_p50_ns": percentile(&warm, 50), "warm_p95_ns": percentile(&warm, 95)}))
}

fn main() -> Result<()> {
    if std::env::args().any(|arg| matches!(arg.as_str(), "--help" | "-h")) {
        println!("Use cargo xtask renderer-bench --help. The runner verifies source before/after building and measuring.");
        return Ok(());
    }
    let opts = options(std::env::args().skip(1))?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let source = provenance::capture(&root)?;
    if option_env!("GRIDCRAFT_BENCH_SOURCE_SHA256") != Some(source.digest.as_str())
        || option_env!("GRIDCRAFT_BENCH_SOURCE_HEAD") != Some(source.head.as_str())
    {
        return Err("source differs from the build identity (or direct build); rerun cargo xtask renderer-bench".into());
    }
    let texts = workload(&opts.script);
    let (fonts, font) = fonts(&opts, &texts)?;
    let mut configurations = Vec::new();
    for config in &opts.configs {
        configurations.push(measure(&opts, *config, &texts, &fonts)?);
    }
    provenance::unchanged(&source, &provenance::capture(&root)?)?;
    if let Some(path) = &opts.font
        && provenance::file_hash(path, 32 * 1024 * 1024)? != opts.font_hash
    {
        return Err("font changed during measurement".into());
    }
    fs::write(
        &opts.out,
        serde_json::to_vec_pretty(&json!({"script": opts.script, "font": font,
        "source_files": source.manifest["files"].as_array().map(Vec::len), "workload_sha256": provenance::bytes_hash(&serde_json::to_vec(&texts)?),
        "cell_scalars": 80, "timing": "app.logic + production grid::show; excludes validation, GPU rendering and PNG encoding", "configurations": configurations}))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workloads_have_unique_ids_and_eighty_scalars() {
        for script in ["latin", "thai", "mixed"] {
            let texts = workload(script);
            assert_eq!(texts.len(), CELLS);
            for (id, text) in texts.iter().enumerate() {
                assert_eq!(text.chars().count(), 80);
                assert!(text.starts_with(&format!("T{id:03} ")));
            }
        }
    }
    #[test]
    fn cli_limits_and_nearest_rank_percentiles() {
        assert!(count("0", 1).is_err());
        assert!(count("1001", 1).is_err());
        assert!(configuration("100x3").is_err());
        assert_eq!(percentile(&[40, 10, 30, 20], 50), 20);
        assert_eq!(percentile(&[40, 10, 30, 20], 95), 40);
    }
}
