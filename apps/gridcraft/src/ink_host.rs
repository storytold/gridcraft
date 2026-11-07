//! The desktop host for the Action Pen's model: where the models live, the download (streamed
//! over HTTPS, checked, then moved into place) and the recognizer loader.
//!
//! Model weights never ship with GridCraft and are never committed (see `docs/action-pen.md`
//! and `MODEL-POLICY`). This file is the only code that fetches one, and it only runs when the
//! model panel's Download button is clicked.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gridcraft_ink::{Manifest, Recognizer, TROCR_SMALL_HANDWRITTEN};
use gridcraft_ui_egui::ModelHost;

/// The desktop's [`ModelHost`]: a download per model id, plus the recognizer loader the UI hooks
/// up to the engine session.
#[derive(Default)]
pub struct Models;

impl Models {
    pub fn new() -> Models {
        Models
    }

    /// The model root the app uses: `$GRIDCRAFT_MODELS` first, then the first existing root that
    /// holds a model, then the settings folder's `models/`. The same order every Craft app uses.
    pub fn root() -> PathBuf {
        gridcraft_ink::roots().into_iter().find(|root| root.is_dir()).or_else(gridcraft_ink::install_root).unwrap_or_else(|| PathBuf::from("models"))
    }

    /// The manifest for `id`, or why this app doesn't know it.
    fn manifest(id: &str) -> Result<&'static Manifest, String> {
        match id {
            id if id == TROCR_SMALL_HANDWRITTEN.id => Ok(&TROCR_SMALL_HANDWRITTEN),
            other => Err(format!("Unknown model `{other}`.")),
        }
    }
}

impl ModelHost for Models {
    fn models_dir(&self) -> PathBuf {
        Self::root()
    }

    fn download(&self, id: &str) -> Result<(), String> {
        let manifest = Self::manifest(id)?;
        let dir = manifest.dir(&self.models_dir());
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for file in manifest.files {
            let dest = dir.join(file.path);
            // Already there and the right size: a previous download finished (every file is
            // hash-checked before it is moved into place, so size is enough here).
            if std::fs::metadata(&dest).is_ok_and(|m| m.is_file() && m.len() == file.bytes) {
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let part = dest.with_extension("part");
            download_to(file.url, &part)?;
            let size = std::fs::metadata(&part).map_err(|e| format!("{}: {e}", part.display()))?.len();
            if size != file.bytes {
                let _ = std::fs::remove_file(&part);
                return Err(format!("{} is {size} bytes, expected {}", file.path, file.bytes));
            }
            let hash = gridcraft_ink::manifest::sha256_file(&part).map_err(|e| {
                let _ = std::fs::remove_file(&part);
                e.to_string()
            })?;
            if !hash.eq_ignore_ascii_case(file.sha256) {
                let _ = std::fs::remove_file(&part);
                return Err(format!("{} did not match its SHA-256; the download may be damaged. Try again.", file.path));
            }
            std::fs::rename(&part, &dest).map_err(|e| {
                let _ = std::fs::remove_file(&part);
                format!("{}: {e}", dest.display())
            })?;
        }
        Ok(())
    }
}

/// Streams one file to `dest`. Only HTTPS; the connection has timeouts so a stalled server
/// fails instead of hanging the worker for ever.
fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing to download over anything but HTTPS: {url}"));
    }
    let agent =
        ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(30)).timeout_read(std::time::Duration::from_secs(60)).build();
    let response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut reader = response.into_reader();
    let mut file = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{url}: {e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    Ok(())
}

/// Builds the TrOCR recognizer from a model root (the UI's `Services::ink_loader`). Runs on a
/// worker thread: it hashes about 250 MB and parses both ONNX graphs.
pub fn load_recognizer(root: &Path) -> Result<Arc<dyn Recognizer>, String> {
    Ok(Arc::new(gridcraft_ink_rten::RtenRecognizer::load(root).map_err(|e| e.to_string())?))
}
