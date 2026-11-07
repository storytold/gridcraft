//! GridCraft ink: what the Action Pen needs that is not a model runtime.
//!
//! A stroke is a list of points in sheet points. [`raster::rasterize`] turns it into a
//! [`raster::LineImage`] (white paper, black ink, the pen's width, padding around the ink) and
//! [`raster::model_input`] stretches that to the 384 × 384, mean/std 0.5 input a TrOCR encoder
//! expects. A [`Recognizer`] does the inference; this crate only defines the trait, the model
//! [`Manifest`] and the directory discovery, so the engine and the UI can depend on it without a
//! neural-network runtime.
//!
//! Model weights never ship with GridCraft (see `docs/action-pen.md`): the manifest lists the
//! files, their sizes and SHA-256 hashes, and the person downloads them or copies them into the
//! documented directory. [`Manifest::check_sizes`] is the cheap check the UI may run every
//! frame; [`Manifest::verify`] reads and hashes the files and is what a load or an import runs.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod manifest;
pub mod raster;

use std::path::{Path, PathBuf};

pub use manifest::{InstallState, Manifest, ModelFile, TROCR_SMALL_HANDWRITTEN};
pub use raster::{Bbox, IMAGE_SIZE, LineImage};

/// Everything that can go wrong before or during recognition. Errors are values, never panics:
/// model files are untrusted input (see `docs/decisions.md`).
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The stroke held fewer than one usable point, or it was too large to rasterise.
    #[error("{0}")]
    Ink(String),
    /// No recognizer is installed (the model is absent, incomplete or was refused).
    #[error("No handwriting model is installed. Download it from Draw › Action Pen, or copy its files into {0}.")]
    NotInstalled(String),
    /// A model file is missing or has the wrong size.
    #[error("The handwriting model is {0}.")]
    Incomplete(String),
    /// A model file's SHA-256 does not match the manifest.
    #[error("{0}")]
    Corrupt(String),
    /// The recognizer ran but could not produce text.
    #[error("Handwriting recognition failed: {0}")]
    Recognize(String),
    /// Reading or writing the model directory failed.
    #[error("{0}")]
    Io(String),
}

impl Error {
    /// A one-line, actionable message for the UI's model panel and for command errors.
    pub fn hint(&self) -> String {
        match self {
            Error::NotInstalled(dir) => format!("Download the handwriting model into {dir}."),
            Error::Incomplete(_) | Error::Corrupt(_) => "Open Draw › Action Pen and download the model again, or import its files.".to_string(),
            Error::Ink(msg) | Error::Recognize(msg) | Error::Io(msg) => msg.clone(),
        }
    }
}

/// Where inference runs. CPU is the only backend this workspace ships and the only one every
/// platform (Windows, macOS, Linux, BSD) has; the enum is the seam an optional accelerator crate
/// plugs into later, so the feature never depends on one. See `docs/action-pen.md`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Accelerator {
    /// Pure-Rust CPU inference (`rten`): every platform, no extra runtime, always available.
    #[default]
    Cpu,
    /// Intel OpenVINO (NPU/GPU/CPU). Not shipped: an optional backend crate would report it.
    OpenVino,
    /// Apple Neural Engine / Metal. Not shipped: an optional backend crate would report it.
    Apple,
}

impl Accelerator {
    /// The wire name commands and `ink.status` use.
    pub fn as_str(self) -> &'static str {
        match self {
            Accelerator::Cpu => "cpu",
            Accelerator::OpenVino => "openvino",
            Accelerator::Apple => "apple",
        }
    }

    /// A display name for the UI and messages.
    pub fn label(self) -> &'static str {
        match self {
            Accelerator::Cpu => "CPU",
            Accelerator::OpenVino => "OpenVINO NPU/GPU",
            Accelerator::Apple => "Apple Neural Engine",
        }
    }

    /// The accelerator a wire name asks for (`"cpu"`, `"openvino"`, `"apple"`), case-insensitive.
    pub fn parse(name: &str) -> Option<Accelerator> {
        match name.trim().to_ascii_lowercase().replace(['-', '_', ' '], "").as_str() {
            "cpu" => Some(Accelerator::Cpu),
            "openvino" | "npu" | "intelnpu" => Some(Accelerator::OpenVino),
            "apple" | "ane" | "coreml" | "metal" => Some(Accelerator::Apple),
            _ => None,
        }
    }
}

/// Handwriting recognition a host can install on an engine session (this crate stays
/// runtime-agnostic; `gridcraft-ink-rten` is the shipped implementation).
///
/// Implementations must be usable from any thread and must not panic: a model runtime's failure
/// is a [`Error::Recognize`].
pub trait Recognizer: Send + Sync {
    /// Recognises one line of handwriting. The image is paper-white with dark ink, as
    /// [`raster::rasterize`] produces it.
    fn recognize(&self, image: &LineImage) -> Result<String, Error>;
    /// The manifest id the recognizer was loaded from (`"trocr-small-handwritten"`).
    fn model_id(&self) -> &'static str;
    /// Where this recognizer runs. The shipped CPU backend answers [`Accelerator::Cpu`]; an
    /// optional accelerator crate overrides it. Behaviour is identical either way.
    fn accelerator(&self) -> Accelerator {
        Accelerator::Cpu
    }
    /// The accelerators this recognizer can use in this build, best first.
    fn accelerators(&self) -> &'static [Accelerator] {
        &[Accelerator::Cpu]
    }
}

/// A recognizer that answers with a fixed string, for tests (no model file, no network).
#[derive(Clone, Debug)]
pub struct FixedRecognizer {
    text: String,
}

impl FixedRecognizer {
    pub fn new(text: impl Into<String>) -> Self {
        FixedRecognizer { text: text.into() }
    }
}

impl Recognizer for FixedRecognizer {
    fn recognize(&self, _image: &LineImage) -> Result<String, Error> {
        Ok(self.text.clone())
    }
    fn model_id(&self) -> &'static str {
        TROCR_SMALL_HANDWRITTEN.id
    }
}

/// Whether `p` is a safe file name relative to a model directory: no root, no parent
/// component, no drive or UNC prefix, no backslash. Model manifests are data, so a path that
/// escapes the model directory must be refused before anything opens it.
pub fn is_safe_relative_path(p: &str) -> bool {
    if p.is_empty() || p.len() > 512 || p.contains('\\') || p.contains('\0') || p.contains(':') {
        return false;
    }
    if p.starts_with('/') || Path::new(p).is_absolute() {
        return false;
    }
    let mut parts = 0usize;
    for part in p.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return false;
        }
        parts += 1;
    }
    parts > 0 && Path::new(p).components().all(|c| matches!(c, std::path::Component::Normal(_)))
}

// ---------------------------------------------------------------- directory discovery

/// The environment variable that overrides every other model location.
pub const MODELS_ENV: &str = "GRIDCRAFT_MODELS";

/// The application's settings directory (the one that holds `prefs.json`), matching the desktop
/// app: `~/Library/Application Support/GridCraft` on macOS, `%APPDATA%\GridCraft` on Windows and
/// `$XDG_CONFIG_HOME/gridcraft` (or `~/.config/gridcraft`) elsewhere.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/GridCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("GridCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|c| c.join("gridcraft"))
    }
}

/// The directory the app writes a downloaded model into: `$GRIDCRAFT_MODELS`, else
/// `config_dir()/models`. May not exist yet.
pub fn install_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(MODELS_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    Some(config_dir()?.join("models"))
}

/// Every directory a model may live in, in the order they are tried (`$GRIDCRAFT_MODELS`, the
/// app's config directory, beside the executable, then the source tree for development).
/// Duplicates and empty entries are dropped; a candidate need not exist.
pub fn roots() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.iter().any(|q| q == &p) {
            out.push(p);
        }
    };
    if let Some(dir) = std::env::var_os(MODELS_ENV).filter(|d| !d.is_empty()) {
        push(PathBuf::from(dir));
    }
    if let Some(dir) = config_dir() {
        push(dir.join("models"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        push(dir.join("models"));
    }
    // Developer convenience: `models/` in the checkout. Baked in at build time (a build tree
    // only), and harmless in a packaged app, where the path does not exist.
    push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join("models"));
    out
}

/// `<root>/<id>`: one model's directory.
pub fn model_dir(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

/// The first root that holds `id` at all (even an incomplete copy), so a person who started an
/// install by hand is told what is still missing instead of being sent to a second directory.
pub fn resolve_root(id: &str) -> Option<PathBuf> {
    roots().into_iter().find(|r| model_dir(r, id).is_dir())
}

/// The root to use for `id`: where an existing copy is, else where a download would go.
pub fn root_for(id: &str) -> Option<PathBuf> {
    resolve_root(id).or_else(install_root)
}

/// The root for the manifest, whether or not the model is there (`None` only when no settings
/// directory can be found and `$GRIDCRAFT_MODELS` is unset).
pub fn default_root() -> Option<PathBuf> {
    root_for(TROCR_SMALL_HANDWRITTEN.id)
}
