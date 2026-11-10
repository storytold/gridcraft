//! The desktop host for the Action Pen's model: where the models live, the download (streamed
//! over HTTPS, checked, then moved into place) and the recognizer loader.
//!
//! Model weights never ship with GridCraft and are never committed (see `docs/action-pen.md`
//! and `MODEL-POLICY`). This file is the only code that fetches one, and it only runs when the
//! model panel's Download button is clicked.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gridcraft_ink::{Manifest, ModelFile, Recognizer, TROCR_SMALL_HANDWRITTEN};
use gridcraft_ui_egui::ModelHost;

/// The desktop's [`ModelHost`]: a download per model id, plus the recognizer loader the UI hooks
/// up to the engine session.
#[derive(Default)]
pub struct Models;

impl Models {
    pub fn new() -> Models {
        Models
    }

    /// The model root the app uses: the first root that holds the model, else where a download
    /// belongs (`$GRIDCRAFT_MODELS` when set, otherwise the settings folder's `models/`). The
    /// same order every Craft app uses.
    pub fn root() -> PathBuf {
        gridcraft_ink::root_for(TROCR_SMALL_HANDWRITTEN.id).unwrap_or_else(|| PathBuf::from("models"))
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
            // Already there with the right size and hash: a previous download finished. Size
            // alone is not enough — a damaged file of the right length would otherwise survive
            // the download the error hint advises.
            if up_to_date(&dest, file) {
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let part = dest.with_extension("part");
            if let Err(e) = download_to(file.url, &part, file.bytes) {
                let _ = std::fs::remove_file(&part);
                return Err(e);
            }
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

/// Whether `dest` already holds `file`: its exact size and its SHA-256. A file of the right size
/// with the wrong contents is not complete, so a Download repairs it instead of skipping it.
fn up_to_date(dest: &Path, file: &ModelFile) -> bool {
    std::fs::metadata(dest).is_ok_and(|m| m.is_file() && m.len() == file.bytes)
        && gridcraft_ink::manifest::sha256_file(dest).is_ok_and(|hash| hash.eq_ignore_ascii_case(file.sha256))
}

/// Streams one file to `dest`, stopping as soon as it holds more than `limit` bytes (the size the
/// manifest gives for it). Only HTTPS; the connection has timeouts so a stalled server fails
/// instead of hanging the worker for ever.
fn download_to(url: &str, dest: &Path, limit: u64) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("refusing to download over anything but HTTPS: {url}"));
    }
    let agent =
        ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(30)).timeout_read(std::time::Duration::from_secs(60)).build();
    let response = agent.get(url).call().map_err(|e| format!("{url}: {e}"))?;
    copy_capped(&mut response.into_reader(), dest, limit, url)
}

/// Copies a response body into `dest`, failing once more than `limit` bytes have arrived: a
/// server that streams an oversized body must not fill the disk before the size check runs.
fn copy_capped(reader: &mut dyn Read, dest: &Path, limit: u64, url: &str) -> Result<(), String> {
    let mut file = std::fs::File::create(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{url}: {e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err(format!("{url} is larger than the {limit} bytes the manifest gives"));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gridcraft-ink-host-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn a_streamed_body_is_capped_at_the_manifest_size() {
        let dir = scratch("cap");
        // A body of the size the manifest gives is copied whole.
        let ok = dir.join("ok.bin");
        let body = vec![7u8; 4000];
        assert!(copy_capped(&mut Cursor::new(&body), &ok, 4000, "https://example.invalid/ok").is_ok());
        assert!(std::fs::read(&ok).is_ok_and(|b| b.len() == 4000));

        // An oversized body stops at the limit instead of streaming to disk for ever.
        let big = dir.join("big.bin");
        let body = vec![7u8; 200_000];
        let err = copy_capped(&mut Cursor::new(&body), &big, 100_000, "https://example.invalid/big");
        assert!(err.is_err(), "an oversized body must fail");
        let written = std::fs::metadata(&big).map(|m| m.len()).unwrap_or(u64::MAX);
        assert!(written <= 100_000, "wrote {written} bytes past a 100000-byte limit");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_the_right_size_with_the_wrong_hash_is_not_up_to_date() {
        let dir = scratch("hash");
        // "abc" has this SHA-256.
        let file = ModelFile {
            path: "m.onnx",
            bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            url: "https://example.invalid/m.onnx",
        };
        let dest = dir.join("m.onnx");
        std::fs::write(&dest, b"abc").ok();
        assert!(up_to_date(&dest, &file), "the manifest's file counts as complete");
        // Same size, different bytes: the size check alone would have skipped it.
        std::fs::write(&dest, b"abd").ok();
        assert!(!up_to_date(&dest, &file), "a wrong-hash file must be downloaded again");
        assert!(!up_to_date(&dir.join("absent.onnx"), &file), "a missing file is not complete");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
