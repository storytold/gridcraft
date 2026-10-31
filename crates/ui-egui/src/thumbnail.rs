//! Workbook thumbnails, drawn by GridCraft itself (no screen capture): each visible sheet of a
//! workbook (up to `MAX_SHEETS`) is laid out by the real grid painter in an offscreen egui pass, and the meshes that pass
//! produces are rasterised here on the CPU. Any workbook can be drawn (the built-in samples, recent
//! files, the open document), and the images are kept on disk in the settings folder's
//! `thumbnails` directory, keyed by the file's path, size and modification time, so a changed file
//! gets a new one the next time it is shown.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::epaint::{ClippedPrimitive, Primitive};
use egui::{Color32, ColorImage, TextureHandle, TextureId, TextureOptions};
use gridcraft_engine::{DocState, Session, Trust};

use crate::theme;

/// Thumbnail size in pixels (twice the Backstage tile's 208 × 140 points).
pub const SIZE: [usize; 2] = [416, 280];
/// The part of the sheet a thumbnail shows, in points; drawn at `SIZE / VIEW` pixels per point.
const VIEW: [f32; 2] = [1040.0, 700.0];
/// Bumped when the drawing changes, so cached images from an older build are redrawn.
const VERSION: u32 = 2;
/// Sheets drawn per workbook (the tile flips through them on hover).
const MAX_SHEETS: usize = 8;

/// What a thumbnail is of.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Source {
    /// A built-in sample, by its name in `sample::SAMPLES`.
    Sample(String),
    /// A workbook file.
    File(String),
}

/// Draws sheet `sheet` of `wb` into an image, in the light or dark sheet colours.
pub fn render(mut wb: gridcraft_engine::model::Workbook, sheet: usize, dark: bool) -> Option<ColorImage> {
    // Only the cells: no row and column headings.
    if let Some(sh) = wb.sheet_mut(sheet) {
        sh.show_headings = false;
    }
    wb.set_active_sheet(sheet);
    // The installed fonts, so the workbook's own faces are drawn (a one-time scan per process).
    let _ = crate::fonts::catalog();
    let mut session = Session::new();
    session.add_document(DocState::new(wb, None, String::new(), Trust::FromFile));
    let mut app = crate::SheetApp::new(session, crate::Services::default());
    app.ui.dark_sheet = dark;
    app.ui.dark = dark;
    app.ui.system_theme = false;
    app.ui.palette = String::new();
    app.thumbnail_mode = true;
    let ctx = egui::Context::default();
    theme::apply(&ctx, dark);
    ctx.set_theme(if dark { egui::Theme::Dark } else { egui::Theme::Light });
    let ppp = SIZE[0] as f32 / VIEW[0];
    let mut textures: HashMap<TextureId, ColorImage> = HashMap::new();
    let mut last = None;
    // The app sets up its fonts (and binds the workbook's own faces) over the first passes, and
    // images decode over a few; the last pass is the one drawn.
    for _ in 0..5 {
        let input =
            egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(VIEW[0], VIEW[1]))), ..Default::default() };
        ctx.set_pixels_per_point(ppp);
        let mut out = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            let t = theme::Tokens::get(&ctx);
            egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.sheet_bg(dark))).show(ui, |ui| crate::grid::show(&mut app, ui));
        });
        for (id, deltas) in &out.textures_delta.set {
            for delta in deltas {
                apply_delta(&mut textures, *id, delta);
            }
        }
        for id in &out.textures_delta.free {
            textures.remove(id);
        }
        // Applied above; egui asserts (in debug builds) that a delta is never dropped unhandled.
        out.textures_delta.clear();
        last = Some(out);
    }
    let out = last?;
    let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
    Some(rasterize(&prims, &textures, out.pixels_per_point))
}

fn apply_delta(textures: &mut HashMap<TextureId, ColorImage>, id: TextureId, delta: &egui::epaint::ImageDelta) {
    let egui::ImageData::Color(img) = &delta.image;
    match delta.pos {
        None => {
            textures.insert(id, (**img).clone());
        }
        Some([x0, y0]) => {
            if let Some(tex) = textures.get_mut(&id) {
                let [w, h] = img.size;
                for y in 0..h {
                    for x in 0..w {
                        let (dx, dy) = (x0 + x, y0 + y);
                        if dx < tex.size[0]
                            && dy < tex.size[1]
                            && let (Some(src), Some(dst)) = (img.pixels.get(y * w + x).copied(), tex.pixels.get_mut(dy * tex.size[0] + dx))
                        {
                            *dst = src;
                        }
                    }
                }
            }
        }
    }
}

/// Scan-converts egui's meshes: per pixel centre, barycentric weights pick the vertex colour and
/// texture coordinate; colours are premultiplied and blended over what is there, as the GPU
/// renderer does.
fn rasterize(prims: &[ClippedPrimitive], textures: &HashMap<TextureId, ColorImage>, ppp: f32) -> ColorImage {
    let [w, h] = SIZE;
    let mut buf = vec![[0.0f32; 4]; w * h];
    for prim in prims {
        let Primitive::Mesh(mesh) = &prim.primitive else { continue };
        let tex = textures.get(&mesh.texture_id);
        let clip = prim.clip_rect;
        let (cx0, cy0) = (((clip.min.x * ppp).floor().max(0.0)) as usize, ((clip.min.y * ppp).floor().max(0.0)) as usize);
        let (cx1, cy1) = (((clip.max.x * ppp).ceil().max(0.0)) as usize, ((clip.max.y * ppp).ceil().max(0.0)) as usize);
        let (cx1, cy1) = (cx1.min(w), cy1.min(h));
        for tri in mesh.indices.as_chunks::<3>().0 {
            let (Some(a), Some(b), Some(c)) =
                (mesh.vertices.get(tri[0] as usize), mesh.vertices.get(tri[1] as usize), mesh.vertices.get(tri[2] as usize))
            else {
                continue;
            };
            let (pa, pb, pc) = (a.pos * ppp, b.pos * ppp, c.pos * ppp);
            let area = (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = (pa.x.min(pb.x).min(pc.x).floor().max(0.0) as usize).max(cx0);
            let y0 = (pa.y.min(pb.y).min(pc.y).floor().max(0.0) as usize).max(cy0);
            let x1 = (pa.x.max(pb.x).max(pc.x).ceil().max(0.0) as usize).min(cx1);
            let y1 = (pa.y.max(pb.y).max(pc.y).ceil().max(0.0) as usize).min(cy1);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((pb.x - px) * (pc.y - py) - (pb.y - py) * (pc.x - px)) / area;
                    let w1 = ((pc.x - px) * (pa.y - py) - (pc.y - py) * (pa.x - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    let mut col = [0.0f32; 4];
                    for (k, slot) in col.iter_mut().enumerate() {
                        *slot = (a.color.to_array()[k] as f32 * w0 + b.color.to_array()[k] as f32 * w1 + c.color.to_array()[k] as f32 * w2) / 255.0;
                    }
                    if let Some(tex) = tex {
                        let u = a.uv.x * w0 + b.uv.x * w1 + c.uv.x * w2;
                        let v = a.uv.y * w0 + b.uv.y * w1 + c.uv.y * w2;
                        let s = sample(tex, u, v);
                        for k in 0..4 {
                            col[k] *= s[k];
                        }
                    }
                    if let Some(dst) = buf.get_mut(y * w + x) {
                        let inv = 1.0 - col[3];
                        for k in 0..4 {
                            dst[k] = col[k] + dst[k] * inv;
                        }
                    }
                }
            }
        }
    }
    let pixels = buf
        .iter()
        .map(|p| {
            let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            Color32::from_rgba_premultiplied(to(p[0]), to(p[1]), to(p[2]), to(p[3]))
        })
        .collect();
    ColorImage::new([w, h], pixels)
}

/// Bilinear sample of a premultiplied texture at normalised `u`, `v`, as 0-1 floats.
fn sample(tex: &ColorImage, u: f32, v: f32) -> [f32; 4] {
    let [tw, th] = tex.size;
    if tw == 0 || th == 0 {
        return [1.0; 4];
    }
    let fx = (u * tw as f32 - 0.5).clamp(0.0, (tw - 1) as f32);
    let fy = (v * th as f32 - 0.5).clamp(0.0, (th - 1) as f32);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(tw - 1), (y0 + 1).min(th - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let px = |x: usize, y: usize| tex.pixels.get(y * tw + x).map_or([0.0; 4], |c| c.to_array().map(|v| v as f32 / 255.0));
    let (p00, p10, p01, p11) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let mut out = [0.0; 4];
    for k in 0..4 {
        let top = p00[k] + (p10[k] - p00[k]) * tx;
        let bottom = p01[k] + (p11[k] - p01[k]) * tx;
        out[k] = top + (bottom - top) * ty;
    }
    out
}

/// Loads (or builds) the workbook a source names.
/// The largest workbook file the File view previews: a recent file is read without the user
/// opening it, so a huge (or replaced) file must not cost a huge allocation. Bigger files keep
/// the drawn placeholder tile.
#[cfg(not(target_arch = "wasm32"))]
const MAX_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;
/// The most a previewed workbook package may unpack to, over all its parts: a small file can
/// declare far more than its size (normal opening allows up to 1 GiB).
#[cfg(not(target_arch = "wasm32"))]
const MAX_PREVIEW_UNPACKED: u64 = 64 * 1024 * 1024;

/// The file at `path` when it is at most `cap` bytes; never reads more than `cap + 1`.
#[cfg(not(target_arch = "wasm32"))]
fn read_capped(path: &std::path::Path, cap: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(cap.saturating_add(1)).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= cap).then_some(bytes)
}

fn load(source: &Source) -> Option<gridcraft_engine::model::Workbook> {
    match source {
        Source::Sample(name) => gridcraft_engine::sample::build(name),
        Source::File(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let bytes = read_capped(std::path::Path::new(path), MAX_PREVIEW_BYTES)?;
                if gridcraft_xlsx::unpacked_size(&bytes).is_some_and(|n| n > MAX_PREVIEW_UNPACKED) {
                    return None;
                }
                let name = std::path::Path::new(path).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("Book.xlsx");
                gridcraft_engine::io::open_bytes(name, &bytes).ok().map(|(wb, _)| wb)
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = path;
                None
            }
        }
    }
}

/// The cache name for a source (its sheets are `<key>-s<i>.png`, listed in `<key>.sheets`) as it is now (a file's size and modification time are part
/// of the key, so an edited file gets a new image), or `None` when the file cannot be read.
fn key(source: &Source, dark: bool) -> Option<String> {
    // The installed palette (Dark Green, an Omarchy theme) colours the drawing too, so a palette
    // change must not show tiles drawn in the old one: it is part of the name.
    let palette = theme::active_palette().map_or(0, |p| path_hash(&format!("{p:?}")));
    let theme = format!("{}{palette:x}", if dark { "d" } else { "l" });
    let version = env!("CARGO_PKG_VERSION");
    match source {
        Source::Sample(name) => {
            let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
            Some(format!("sample-{safe}-{theme}-{VERSION}-{version}"))
        }
        Source::File(path) => {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let meta = std::fs::metadata(path).ok()?;
                let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
                Some(format!("file-{:016x}-{modified}-{}-{theme}-{VERSION}", path_hash(path), meta.len()))
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = path;
                None
            }
        }
    }
}

/// A stable hash of a path (FNV-1a), so the cache name does not depend on the hasher's seed.
fn path_hash(path: &str) -> u64 {
    path.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// One sheet's thumbnail, with the sheet's name.
#[derive(Clone)]
pub struct Page {
    pub texture: TextureId,
    pub name: String,
}

/// Where a workbook's thumbnails stand: the sheets drawn so far, in order.
#[derive(Default)]
struct Entry {
    pages: Vec<(TextureHandle, String)>,
    failed: bool,
}

/// What the worker sends back: a sheet's image (cache key, sheet index, name, image), or that the
/// workbook could not be drawn (image `None`).
type Arrivals = Arc<Mutex<Vec<(String, usize, String, Option<ColorImage>)>>>;

/// What the key worker sends back: a file's cache key (`None` when it can't be read).
type KeyArrivals = Arc<Mutex<Vec<((Source, bool), Option<String>)>>>;

/// How long a file's cache key is trusted before its metadata is read again (seconds).
const KEY_TTL: f64 = 5.0;

/// Thumbnails known this run, by cache key; images arrive from a worker thread.
#[derive(Default)]
pub struct Thumbnails {
    entries: HashMap<String, Entry>,
    arrived: Arrivals,
    /// Files' cache keys (from their size and modification time) and when each was read. They
    /// are read on a worker: a sleeping drive or a lost network share must not stall drawing.
    keys: HashMap<(Source, bool), (Option<String>, f64)>,
    keys_pending: std::collections::HashSet<(Source, bool)>,
    keys_arrived: KeyArrivals,
    /// The web has no worker thread: drawing jobs wait here and run one per frame.
    #[cfg(target_arch = "wasm32")]
    deferred: std::collections::VecDeque<Box<dyn FnOnce()>>,
    /// The cache folder, if there is one.
    dir: Option<std::path::PathBuf>,
}

impl Thumbnails {
    pub fn new(dir: Option<std::path::PathBuf>) -> Thumbnails {
        Thumbnails { dir, ..Default::default() }
    }

    /// The sheet thumbnails of `source` in the light or dark colours that are ready (the first
    /// sheet first); the rest are read from the cache folder or drawn on a worker, and later
    /// frames show them.
    pub fn pages(&mut self, ctx: &egui::Context, source: &Source, dark: bool) -> Vec<Page> {
        self.take_arrivals(ctx);
        let Some(key) = self.key_for(ctx, source, dark) else { return Vec::new() };
        if let Some(e) = self.entries.get(&key) {
            return e.pages.iter().map(|(t, n)| Page { texture: t.id(), name: n.clone() }).collect();
        }
        self.entries.insert(key.clone(), Entry::default());
        let dir = self.dir.clone();
        let arrived = self.arrived.clone();
        let source = source.clone();
        let ctx2 = ctx.clone();
        let job = move || {
            let send = |i: usize, name: String, image: Option<ColorImage>| {
                if let Ok(mut a) = arrived.lock() {
                    a.push((key.clone(), i, name, image));
                }
                ctx2.request_repaint();
            };
            if let Some(pages) = cached(dir.as_deref(), &key) {
                for (i, (name, image)) in pages.into_iter().enumerate() {
                    send(i, name, Some(image));
                }
                return;
            }
            let Some(wb) = load(&source) else {
                send(0, String::new(), None);
                return;
            };
            let sheets: Vec<(usize, String)> = wb
                .sheets
                .iter()
                .enumerate()
                .filter(|(_, sh)| sh.visibility == gridcraft_engine::model::Visibility::Visible)
                .map(|(i, sh)| (i, sh.name.clone()))
                .take(MAX_SHEETS)
                .collect();
            let mut done = Vec::new();
            for (n, (i, name)) in sheets.into_iter().enumerate() {
                match render(wb.clone(), i, dark) {
                    Some(image) => {
                        done.push((name.clone(), image.clone()));
                        send(n, name, Some(image));
                    }
                    None if n == 0 => {
                        send(0, String::new(), None);
                        return;
                    }
                    None => {}
                }
            }
            store(dir.as_deref(), &key, &done);
        };
        #[cfg(not(target_arch = "wasm32"))]
        spawn(job);
        #[cfg(target_arch = "wasm32")]
        self.deferred.push_back(Box::new(job));
        Vec::new()
    }

    /// `source`'s cache key: at once for a sample; for a file, the last one the key worker
    /// read (asking it again once that is older than `KEY_TTL`), or `None` until it answers.
    fn key_for(&mut self, ctx: &egui::Context, source: &Source, dark: bool) -> Option<String> {
        if matches!(source, Source::Sample(_)) {
            return key(source, dark);
        }
        let id = (source.clone(), dark);
        let now = ctx.input(|i| i.time);
        let known = self.keys.get(&id).cloned();
        let stale = known.as_ref().is_none_or(|(_, at)| now - at > KEY_TTL);
        if stale && self.keys_pending.insert(id.clone()) {
            let arrived = self.keys_arrived.clone();
            let ctx2 = ctx.clone();
            spawn_key(move || {
                let k = key(&id.0, id.1);
                if let Ok(mut a) = arrived.lock() {
                    a.push((id, k));
                }
                ctx2.request_repaint();
            });
        }
        known.and_then(|(k, _)| k)
    }

    fn take_arrivals(&mut self, ctx: &egui::Context) {
        #[cfg(target_arch = "wasm32")]
        if let Some(job) = self.deferred.pop_front() {
            job();
            ctx.request_repaint();
        }
        let keys = self.keys_arrived.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
        if !keys.is_empty() {
            let now = ctx.input(|i| i.time);
            for (id, k) in keys {
                self.keys_pending.remove(&id);
                self.keys.insert(id, (k, now));
            }
        }
        let arrived = self.arrived.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
        for (key, i, name, image) in arrived {
            let entry = self.entries.entry(key.clone()).or_default();
            match image {
                Some(img) if i == entry.pages.len() => {
                    entry.pages.push((ctx.load_texture(format!("thumb-{key}-{i}"), img, TextureOptions::LINEAR), name));
                }
                Some(_) => {}
                None => entry.failed = true,
            }
        }
    }
}

/// Runs thumbnail work off the interface thread (one at a time, so a long list of recent files
/// does not start dozens of workers). The web has no threads: `Thumbnails` runs its jobs one
/// per frame instead.
#[cfg(not(target_arch = "wasm32"))]
fn spawn(job: impl FnOnce() + Send + 'static) {
    static QUEUE: WorkQueue = std::sync::OnceLock::new();
    queue(&QUEUE, "gridcraft-thumbnails", job);
}

/// Reads files' cache keys on a worker of their own, so a key is never stuck behind drawing.
fn spawn_key(job: impl FnOnce() + Send + 'static) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static QUEUE: WorkQueue = std::sync::OnceLock::new();
        queue(&QUEUE, "gridcraft-thumbnail-keys", job);
    }
    #[cfg(target_arch = "wasm32")]
    job();
}

#[cfg(not(target_arch = "wasm32"))]
type Job = Box<dyn FnOnce() + Send>;
#[cfg(not(target_arch = "wasm32"))]
type WorkQueue = std::sync::OnceLock<Mutex<std::sync::mpsc::Sender<Job>>>;

/// Sends `job` to the worker thread behind `cell`, starting it on first use.
#[cfg(not(target_arch = "wasm32"))]
fn queue(cell: &'static WorkQueue, name: &str, job: impl FnOnce() + Send + 'static) {
    let tx = cell.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let _ = std::thread::Builder::new().name(name.into()).spawn(move || {
            while let Ok(job) = rx.recv() {
                // A workbook that trips the renderer must not take the worker down with it.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            }
        });
        Mutex::new(tx)
    });
    if let Ok(tx) = tx.lock() {
        let _ = tx.send(Box::new(job));
    }
}

/// Reads a workbook's cached sheet thumbnails (all of them, or none).
fn cached(dir: Option<&std::path::Path>, key: &str) -> Option<Vec<(String, ColorImage)>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let dir = dir?;
        let names = std::fs::read_to_string(dir.join(format!("{key}.sheets"))).ok()?;
        let mut pages = Vec::new();
        for (i, name) in names.lines().enumerate().take(MAX_SHEETS) {
            let bytes = std::fs::read(dir.join(format!("{key}-s{i}.png"))).ok()?;
            let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            pages.push((name.to_string(), ColorImage::from_rgba_unmultiplied(size, img.as_raw())));
        }
        (!pages.is_empty()).then_some(pages)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (dir, key);
        None
    }
}

/// Writes a workbook's sheet thumbnails to the cache folder, then the list of their names (which
/// marks the set complete), dropping older images of the same file or sample in the same theme.
fn store(dir: Option<&std::path::Path>, key: &str, pages: &[(String, ColorImage)]) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(dir) = dir else { return };
        if pages.is_empty() || std::fs::create_dir_all(dir).is_err() {
            return;
        }
        // Older versions of this file's (or sample's) images: same source, same theme, other key.
        let parts: Vec<&str> = key.split('-').collect();
        let prefix = parts.get(..2).map(|p| p.join("-")).unwrap_or_default();
        let theme_at = if key.starts_with("sample-") { parts.len().saturating_sub(3) } else { parts.len().saturating_sub(2) };
        // The theme token is "l" or "d" and the palette's hash: compare the letter only, so
        // images drawn in an older palette are dropped too.
        let theme = parts.get(theme_at).and_then(|t| t.chars().next());
        if let Ok(read) = std::fs::read_dir(dir) {
            for e in read.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let same_theme = theme.is_some() && name.split('-').nth(theme_at).and_then(|t| t.chars().next()) == theme;
                if !prefix.is_empty() && name.starts_with(&format!("{prefix}-")) && !name.starts_with(key) && same_theme {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        for (i, (_, image)) in pages.iter().enumerate() {
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            let Some(buf) = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba) else { return };
            let tmp = dir.join(format!("{key}-s{i}.tmp"));
            if buf.save_with_format(&tmp, image::ImageFormat::Png).is_err() || std::fs::rename(&tmp, dir.join(format!("{key}-s{i}.png"))).is_err() {
                return;
            }
        }
        // Sheet names never hold a line break, so one per line.
        let names: Vec<&str> = pages.iter().map(|(n, _)| n.as_str()).collect();
        let _ = std::fs::write(dir.join(format!("{key}.sheets")), names.join("\n"));
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (dir, key, pages);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_draws_into_a_non_blank_image() {
        let wb = gridcraft_engine::sample::build("sales").expect("sales sample");
        let img = render(wb, 0, false).expect("rendered");
        assert_eq!(img.size, SIZE);
        let first = img.pixels[0];
        assert!(img.pixels.iter().any(|p| *p != first), "the sheet's content shows");
    }

    #[test]
    fn a_changed_file_gets_a_new_key() {
        let dir = std::env::temp_dir().join(format!("gridcraft-thumb-key-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("a.csv");
        std::fs::write(&path, "1,2\n").expect("write");
        let src = Source::File(path.to_string_lossy().to_string());
        let k1 = key(&src, false).expect("key");
        std::fs::write(&path, "1,2,3,4\n").expect("write");
        let k2 = key(&src, false).expect("key");
        assert_ne!(k1, k2, "a different size is a different image");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod preview_cap_tests {
    use super::read_capped;

    #[test]
    fn files_over_the_cap_are_not_previewed() {
        let path = std::env::temp_dir().join(format!("gridcraft-preview-cap-{}.bin", std::process::id()));
        std::fs::write(&path, [7u8; 10]).unwrap();
        assert_eq!(read_capped(&path, 10).map(|b| b.len()), Some(10));
        assert!(read_capped(&path, 9).is_none(), "one byte over the cap");
        let _ = std::fs::remove_file(&path);
    }
}
