//! Bordered PDF tables, interpreted without rasterizing pages or executing PDF actions.
//! The preview retains cell text; numeric conversion is an explicit loading choice.
use craft_pdf::interpret::{Context, InterpreterCache, InterpreterSettings, TransformExt};
use kurbo::Rect;
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;
mod capture;
mod lattice;

/// File input policy shared with the family PDF reader, checked before allocation.
pub const MAX_INPUT_BYTES: usize = craft_pdf::MAX_INPUT_BYTES;
pub const MAX_CELLS: usize = 100_000;
/// XLSX cell text is limited in UTF-16 code units. Refuse rather than truncate data.
pub const MAX_CELL_TEXT: usize = 32_767;
const MAX_GLYPHS: usize = 200_000;
const MAX_SEGMENTS: usize = 16_384;
const MAX_WORK: usize = 2_000_000;
const MAX_TEXT: usize = 8 * 1024 * 1024;
const TOLERANCE: f64 = 0.5;

#[derive(Clone, Debug, thiserror::Error)]
pub enum ImportError {
    #[error("PDF import was canceled.")]
    Cancelled,
    #[error("PDF import exceeded its {0} budget; no partial table was loaded.")]
    Limit(&'static str),
    #[error("This page cannot be imported safely yet: {0}")]
    Unsupported(&'static str),
    #[error("Cannot import PDF: {0}")]
    Invalid(String),
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PdfTable {
    /// Left, top, right and bottom, in displayed page points (crop/rotation applied).
    pub bounds: [f64; 4],
    pub rows: Vec<Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageTables {
    pub page: u32,
    pub pages: usize,
    pub tables: Vec<PdfTable>,
}
pub(super) struct Control {
    stop: Arc<AtomicBool>,
    started: Option<Instant>,
    work: usize,
}
impl Control {
    fn new(stop: Arc<AtomicBool>) -> Self {
        // The web engine has no native clock. It still enforces work/allocation budgets;
        // the file-picker workflow and wall-clock deadline are desktop-only.
        #[cfg(not(target_arch = "wasm32"))]
        #[allow(clippy::disallowed_methods)] // guarded native-only call, per clippy.toml
        let started = Some(Instant::now());
        #[cfg(target_arch = "wasm32")]
        let started = None;
        Self { stop, started, work: 0 }
    }
    fn check(&self) -> Result<(), ImportError> {
        if self.started.is_some_and(|start| start.elapsed().as_secs_f64() > 10.0) {
            return Err(ImportError::Limit("page processing time"));
        }
        if self.stop.load(Ordering::Relaxed) {
            return Err(ImportError::Cancelled);
        }
        Ok(())
    }
    fn spend(&mut self, n: usize) -> Result<(), ImportError> {
        self.check()?;
        self.work = self.work.checked_add(n).ok_or(ImportError::Limit("geometry work"))?;
        if self.work > MAX_WORK {
            return Err(ImportError::Limit("geometry work"));
        }
        Ok(())
    }
}
/// Interpret one explicit, one-based page. Work happens on the caller's worker.
/// A canceled, unsupported or incomplete page never returns a partial table.
pub fn inspect(bytes: Arc<Vec<u8>>, page: u32, stop: Arc<AtomicBool>) -> Result<PageTables, ImportError> {
    let control = Control::new(stop.clone());
    control.check()?;
    let started = control.started;
    let check_stop = stop.clone();
    let cancel = craft_pdf::syntax::Cancellation::new(move || {
        check_stop.load(Ordering::Relaxed) || started.is_some_and(|start| start.elapsed().as_secs_f64() > 10.0)
    });
    let pdf = craft_pdf::open_with_options_and_cancel(bytes, None, &Default::default(), &cancel).map_err(|e| ImportError::Invalid(e.to_string()))?;
    control.check()?;
    let index = page.checked_sub(1).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| ImportError::Invalid("page must be at least 1".into()))?;
    let p = pdf.pages().get(index).ok_or_else(|| ImportError::Invalid("the selected page does not exist".into()))?;
    let (w, h) = p.render_dimensions();
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(ImportError::Invalid("invalid page dimensions".into()));
    }
    let warnings = Arc::new(AtomicBool::new(false));
    let warned = warnings.clone();
    let settings = InterpreterSettings {
        render_annotations: false,
        cancelled: Some(stop),
        warning_sink: Arc::new(move |_| {
            warned.store(true, Ordering::Relaxed);
        }),
        ..Default::default()
    };
    let cache = InterpreterCache::new();
    let mut context = Context::new(p.initial_transform(true).to_kurbo(), Rect::new(0., 0., w as f64, h as f64), &cache, pdf.xref(), settings);
    let mut capture = capture::Capture::new(control, Rect::new(0., 0., w as f64, h as f64));
    let interpreted = craft_pdf::interpret_page(p, &mut context, &mut capture);
    if let Some(e) = capture.failure.take() {
        return Err(e);
    }
    capture.control.check()?;
    interpreted.map_err(|e| ImportError::Invalid(e.to_string()))?;
    if warnings.load(Ordering::Relaxed) {
        return Err(ImportError::Unsupported("the PDF interpreter reported incomplete or unsupported content."));
    }
    let tables = lattice::detect(&mut capture)?;
    Ok(PageTables { page, pages: pdf.pages().len(), tables })
}
/// Bound a native file before reading, including a file that grows after the metadata check.
pub fn read(path: &std::path::Path) -> Result<Arc<Vec<u8>>, ImportError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| ImportError::Invalid(e.to_string()))?;
    if file.metadata().map_err(|e| ImportError::Invalid(e.to_string()))?.len() > MAX_INPUT_BYTES as u64 {
        return Err(ImportError::Limit("file size"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_INPUT_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|e| ImportError::Invalid(e.to_string()))?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(ImportError::Limit("file size"));
    }
    Ok(Arc::new(bytes))
}
