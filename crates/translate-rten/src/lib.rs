//! GridCraft's native translation backend: OPUS-MT models exported to ONNX, executed by `rten`
//! on the CPU.
//!
//! The model is user-supplied (downloaded through the app after consent, or placed in the model
//! directory by hand). It is never committed, never bundled, and never fetched during inference.
//! Files are verified against [`gridcraft_translate::manifest`] (size and SHA-256) before the
//! graph is loaded, so a truncated or swapped file is an error, not undefined behaviour.
//!
//! `rten` is a pure-Rust CPU runtime: it builds and runs on Windows, macOS, Linux and BSD, needs
//! no C/C++ runtime, and is not part of the web build. [`gridcraft_translate::Accelerator`] is the
//! seam a future optional OpenVINO backend plugs into; this crate reports `cpu`.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod spm;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gridcraft_translate::manifest::{self, ModelFile, ModelManifest};
use gridcraft_translate::{Accelerator, Backend, BackendError, BatchRequest, Cancel, LangTag};
use rten::{Model, ValueView};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

/// Why a model could not be verified or loaded. Messages name files and sizes, never cell text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ModelError {
    #[error("the model directory is not readable: {0}")]
    Io(String),
    #[error("the model file `{0}` is missing")]
    Missing(String),
    #[error("the model file `{path}` has {actual} bytes; the manifest says {expected}")]
    Size { path: String, expected: u64, actual: u64 },
    #[error("the model file `{0}` does not match its SHA-256")]
    Hash(String),
    #[error("the model file path `{0}` is unsafe")]
    Path(String),
    #[error("the model file `{path}` is not a usable ONNX model: {reason}")]
    Format { path: String, reason: String },
    #[error("the model is not installed in `{0}`")]
    NotInstalled(String),
    #[error("{0}")]
    Other(String),
}

/// One loaded pair. Everything is immutable after construction; inference takes `&self`.
struct Loaded {
    encoder: Model,
    decoder: Model,
    source: Tokenizer,
    target: Tokenizer,
    config: ModelConfig,
}

#[derive(Clone, Debug)]
struct ModelConfig {
    eos_token_id: u32,
    decoder_start_token_id: u32,
    max_positions: usize,
    max_decode_steps: usize,
}

/// The backend. Constructing it is cheap; the weights load (and verify) on the first translate,
/// on the worker that runs the job, so opening the pane never blocks and never reads 116 MB.
pub struct RtenBackend {
    dir: PathBuf,
    manifest: &'static ModelManifest,
    loaded: OnceLock<Result<Loaded, String>>,
}

impl RtenBackend {
    /// Opens the model `id` under `root` (`<root>/<id>`). Missing files are reported on first
    /// use with an actionable error, so a host may construct this whenever a directory exists.
    pub fn open(root: impl AsRef<Path>, id: &str) -> Result<RtenBackend, ModelError> {
        let manifest = manifest::find(id).ok_or_else(|| ModelError::Other(format!("`{id}` is not a known translation model")))?;
        Ok(RtenBackend::with_manifest(root.as_ref().join(id), manifest))
    }

    /// Opens a model directory whose name is the manifest id.
    pub fn open_dir(dir: impl AsRef<Path>) -> Result<RtenBackend, ModelError> {
        let dir = dir.as_ref();
        let name = dir.file_name().and_then(|n| n.to_str()).ok_or_else(|| ModelError::NotInstalled(dir.display().to_string()))?;
        let manifest = manifest::find(name).ok_or_else(|| ModelError::Other(format!("`{name}` is not a known translation model")))?;
        Ok(RtenBackend::with_manifest(dir.to_path_buf(), manifest))
    }

    pub fn with_manifest(dir: PathBuf, manifest: &'static ModelManifest) -> RtenBackend {
        RtenBackend { dir, manifest, loaded: OnceLock::new() }
    }

    pub fn model_dir(&self) -> &Path {
        &self.dir
    }

    pub fn manifest(&self) -> &'static ModelManifest {
        self.manifest
    }

    /// Verifies every manifest file and loads the graphs. Cached after the first success.
    pub fn ensure_loaded(&self) -> Result<(), ModelError> {
        match self.loaded.get_or_init(|| load(&self.dir, self.manifest).map_err(|e| e.to_string())) {
            Ok(_) => Ok(()),
            Err(message) => Err(ModelError::Other(message.clone())),
        }
    }

    fn loaded(&self) -> Result<&Loaded, BackendError> {
        match self.loaded.get_or_init(|| load(&self.dir, self.manifest).map_err(|e| e.to_string())) {
            Ok(loaded) => Ok(loaded),
            Err(message) => Err(BackendError::Model(message.clone())),
        }
    }

    fn translate_one(&self, loaded: &Loaded, text: &str, cancel: &Cancel) -> Result<String, BackendError> {
        cancel.check()?;
        let ids: Vec<i32> =
            loaded.source.encode(text, true).map_err(|e| BackendError::Input(e.to_string()))?.get_ids().iter().map(|&id| id as i32).collect();
        if ids.len() + 1 > loaded.config.max_positions {
            return Err(BackendError::Input("a sentence is longer than the model can read".into()));
        }
        if ids.is_empty() {
            return Ok(String::new());
        }
        let mut source_ids = ids;
        source_ids.push(loaded.config.eos_token_id as i32);
        let length = source_ids.len();
        let mask = vec![1i32; length];
        let encoder = &loaded.encoder;
        let decoder = &loaded.decoder;
        let input_ids = encoder.node_id("input_ids").map_err(rten_error)?;
        let attention_mask = encoder.node_id("attention_mask").map_err(rten_error)?;
        let hidden_out = encoder.node_id("last_hidden_state").map_err(rten_error)?;
        let hidden = encoder
            .run(
                vec![
                    (input_ids, ValueView::from_shape([1, length], &source_ids).map_err(rten_error)?.into()),
                    (attention_mask, ValueView::from_shape([1, length], &mask).map_err(rten_error)?.into()),
                ],
                &[hidden_out],
                None,
            )
            .map_err(|e| BackendError::Internal(e.to_string()))?;
        let hidden = hidden
            .into_iter()
            .next()
            .ok_or_else(|| BackendError::Internal("the encoder returned no hidden state".into()))?
            .into_shape_vec::<f32, 3>()
            .map_err(|e| BackendError::Internal(e.to_string()))?;
        let (hidden_shape, hidden_data) = hidden;
        if hidden_shape[0] != 1 || hidden_shape[1] != length {
            return Err(BackendError::Internal("the encoder returned an unexpected shape".into()));
        }
        let decoder_ids = decoder.node_id("input_ids").map_err(rten_error)?;
        let decoder_hidden = decoder.node_id("encoder_hidden_states").map_err(rten_error)?;
        let decoder_mask = decoder.node_id("encoder_attention_mask").map_err(rten_error)?;
        let logits_out = decoder.node_id("logits").map_err(rten_error)?;
        let mut output = vec![loaded.config.decoder_start_token_id as i32];
        for _ in 0..loaded.config.max_decode_steps {
            cancel.check()?;
            let sequence = output.len();
            let logits = decoder
                .run(
                    vec![
                        (decoder_ids, ValueView::from_shape([1, sequence], &output).map_err(rten_error)?.into()),
                        (decoder_hidden, ValueView::from_shape([1, hidden_shape[1], hidden_shape[2]], &hidden_data).map_err(rten_error)?.into()),
                        (decoder_mask, ValueView::from_shape([1, length], &mask).map_err(rten_error)?.into()),
                    ],
                    &[logits_out],
                    None,
                )
                .map_err(|e| BackendError::Internal(e.to_string()))?;
            let (shape, data) = logits
                .into_iter()
                .next()
                .ok_or_else(|| BackendError::Internal("the decoder returned no logits".into()))?
                .into_shape_vec::<f32, 3>()
                .map_err(|e| BackendError::Internal(e.to_string()))?;
            let vocab = *shape.get(2).ok_or_else(|| BackendError::Internal("the decoder returned an unexpected shape".into()))?;
            if vocab == 0 {
                return Err(BackendError::Internal("the decoder vocabulary is empty".into()));
            }
            let start = (sequence.saturating_sub(1)).saturating_mul(vocab);
            let row =
                data.get(start..start.saturating_add(vocab)).ok_or_else(|| BackendError::Internal("the decoder logits are truncated".into()))?;
            let token = argmax(row);
            output.push(token);
            if token == loaded.config.eos_token_id as i32 {
                break;
            }
        }
        let ids: Vec<u32> = output
            .iter()
            .filter(|&&id| id >= 0 && id as u32 != loaded.config.decoder_start_token_id && id as u32 != loaded.config.eos_token_id)
            .map(|&id| id as u32)
            .collect();
        loaded.target.decode(&ids, true).map_err(|e| BackendError::Internal(e.to_string()))
    }
}

impl Backend for RtenBackend {
    fn model_id(&self) -> &str {
        self.manifest.id
    }

    fn accelerator(&self) -> Accelerator {
        Accelerator::Cpu
    }

    fn supports(&self, from: &LangTag, to: &LangTag) -> bool {
        self.manifest.pairs.iter().any(|pair| {
            let (Ok(f), Ok(t)) = (LangTag::parse(pair.from), LangTag::parse(pair.to)) else { return false };
            from.matches(&f) && to.matches(&t)
        })
    }

    fn translate(&self, req: &BatchRequest<'_>, cancel: &Cancel) -> Result<Vec<String>, BackendError> {
        if !self.supports(req.from, req.to) {
            return Err(BackendError::UnsupportedPair { from: req.from.canonical(), to: req.to.canonical() });
        }
        let loaded = self.loaded()?;
        let mut out = Vec::with_capacity(req.texts.len());
        for text in req.texts {
            cancel.check()?;
            out.push(self.translate_one(loaded, text, cancel)?);
        }
        Ok(out)
    }
}

fn argmax(values: &[f32]) -> i32 {
    let mut best = 0usize;
    let mut best_value = f32::NEG_INFINITY;
    for (index, value) in values.iter().enumerate() {
        if value.is_finite() && *value > best_value {
            best_value = *value;
            best = index;
        }
    }
    i32::try_from(best).unwrap_or(0)
}

fn rten_error<E: std::fmt::Display>(e: E) -> BackendError {
    BackendError::Internal(e.to_string())
}

fn load(dir: &Path, manifest: &'static ModelManifest) -> Result<Loaded, ModelError> {
    verify_files(dir, manifest)?;
    let config = read_config(dir.join("config.json"))?;
    let encoder_path = dir.join("onnx/encoder_model_quantized.onnx");
    let decoder_path = dir.join("onnx/decoder_model_quantized.onnx");
    let encoder = Model::load_file(&encoder_path)
        .map_err(|e| ModelError::Format { path: "onnx/encoder_model_quantized.onnx".into(), reason: e.to_string() })?;
    let decoder = Model::load_file(&decoder_path)
        .map_err(|e| ModelError::Format { path: "onnx/decoder_model_quantized.onnx".into(), reason: e.to_string() })?;
    let source =
        spm::tokenizer(dir.join("source.spm"), dir.join("vocab.json")).map_err(|reason| ModelError::Format { path: "source.spm".into(), reason })?;
    let target =
        spm::tokenizer(dir.join("target.spm"), dir.join("vocab.json")).map_err(|reason| ModelError::Format { path: "target.spm".into(), reason })?;
    Ok(Loaded { encoder, decoder, source, target, config })
}

fn read_config(path: PathBuf) -> Result<ModelConfig, ModelError> {
    let bytes = std::fs::read(&path).map_err(|e| ModelError::Io(format!("config.json: {e}")))?;
    let json: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| ModelError::Format { path: "config.json".into(), reason: e.to_string() })?;
    let eos = json.get("eos_token_id").and_then(serde_json::Value::as_u64).unwrap_or(0);
    let start = json.get("decoder_start_token_id").and_then(serde_json::Value::as_u64).unwrap_or(65_000);
    let positions = json.get("max_position_embeddings").and_then(serde_json::Value::as_u64).unwrap_or(512);
    Ok(ModelConfig {
        eos_token_id: u32::try_from(eos).unwrap_or(0),
        decoder_start_token_id: u32::try_from(start).unwrap_or(65_000),
        max_positions: usize::try_from(positions).unwrap_or(512).clamp(8, 4096),
        // Greedy decoding, bounded: a runaway decoder is an error in the input, not a hang.
        max_decode_steps: 128,
    })
}

/// Verifies every file in a manifest: path safety, exact byte size and SHA-256.
pub fn verify_files(dir: &Path, manifest: &ModelManifest) -> Result<(), ModelError> {
    for file in manifest.files {
        verify_file(dir, file.path, file.bytes, file.sha256)?;
    }
    Ok(())
}

/// Verifies one relative file against an expected size and SHA-256.
pub fn verify_file(dir: &Path, relative: &str, bytes: u64, sha256: &str) -> Result<(), ModelError> {
    if !manifest::valid_relative_path(relative) {
        return Err(ModelError::Path(relative.to_string()));
    }
    let path = dir.join(relative);
    let metadata = std::fs::metadata(&path).map_err(|_| ModelError::Missing(relative.to_string()))?;
    let actual = metadata.len();
    if actual != bytes {
        return Err(ModelError::Size { path: relative.to_string(), expected: bytes, actual });
    }
    let mut file = std::fs::File::open(&path).map_err(|e| ModelError::Io(format!("{relative}: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|e| ModelError::Io(format!("{relative}: {e}")))?;
        if read == 0 {
            break;
        }
        hasher.update(buffer.get(..read).unwrap_or_default());
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if !hex.eq_ignore_ascii_case(sha256) {
        return Err(ModelError::Hash(relative.to_string()));
    }
    Ok(())
}

/// The model files a manifest lists, for a host that installs or removes a model.
pub fn files(manifest: &ModelManifest) -> &'static [ModelFile] {
    manifest.files
}

/// The model root, resolved the same way in every Craft app (see `docs/translate.md`):
///
/// 1. `$GRIDCRAFT_MODELS`;
/// 2. the platform data directory (`%APPDATA%\GridCraft\models`,
///    `~/Library/Application Support/GridCraft/models`, `$XDG_DATA_HOME/gridcraft/models`);
/// 3. `models/` beside the executable;
/// 4. `models/` in the working directory (a source checkout).
///
/// The first three are returned even before they exist, so a download can create them; a source
/// tree's `models/` is only used when it is already there.
pub fn models_root() -> std::path::PathBuf {
    if let Some(root) = std::env::var_os("GRIDCRAFT_MODELS").filter(|v| !v.is_empty()) {
        return std::path::PathBuf::from(root);
    }
    let data = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(|p| std::path::PathBuf::from(p).join("GridCraft"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join("Library/Application Support/GridCraft"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".local/share")))
            .map(|p| p.join("gridcraft"))
    };
    if let Some(dir) = data {
        return dir.join("models");
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        return parent.join("models");
    }
    std::path::PathBuf::from("models")
}

/// Opens the installed `opus-mt-en-es` model when its directory exists. A host calls this at
/// startup and after a download/import; construction is cheap and verification happens on the
/// worker that first translates.
pub fn installed_backend(root: &std::path::Path) -> Option<std::sync::Arc<dyn gridcraft_translate::Backend>> {
    if !root.join("opus-mt-en-es").is_dir() {
        return None;
    }
    RtenBackend::open(root, "opus-mt-en-es").ok().map(|b| std::sync::Arc::new(b) as std::sync::Arc<dyn gridcraft_translate::Backend>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("gridcraft-translate-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or(());
        dir
    }

    #[test]
    fn verifies_size_hash_and_path_safety() {
        let dir = temp_dir("verify");
        let mut file = std::fs::File::create(dir.join("a.bin")).unwrap_or_else(|e| panic!("{e}"));
        let _ = file.write_all(b"hello");
        let hash = {
            let mut h = Sha256::new();
            h.update(b"hello");
            h.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>()
        };
        assert_eq!(verify_file(&dir, "a.bin", 5, &hash), Ok(()));
        assert!(matches!(verify_file(&dir, "a.bin", 6, &hash), Err(ModelError::Size { .. })));
        assert!(matches!(verify_file(&dir, "a.bin", 5, &"0".repeat(64)), Err(ModelError::Hash(_))));
        assert!(matches!(verify_file(&dir, "../a.bin", 5, &hash), Err(ModelError::Path(_))));
        assert!(matches!(verify_file(&dir, "missing.bin", 5, &hash), Err(ModelError::Missing(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zero_byte_and_truncated_files_are_errors_not_panics() {
        let dir = temp_dir("truncated");
        let _ = std::fs::File::create(dir.join("empty.onnx"));
        assert!(matches!(verify_file(&dir, "empty.onnx", 0, &"0".repeat(64)), Err(ModelError::Hash(_))));
        let _ = std::fs::write(dir.join("short.onnx"), b"\x08\x07");
        assert!(matches!(verify_file(&dir, "short.onnx", 100, &"0".repeat(64)), Err(ModelError::Size { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_format_is_a_structured_error() {
        let dir = temp_dir("format");
        let _ = std::fs::write(dir.join("bad.onnx"), b"not an onnx model");
        let error = Model::load_file(dir.join("bad.onnx"));
        assert!(error.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_model_id_is_refused() {
        assert!(RtenBackend::open(std::env::temp_dir(), "../../etc").is_err());
        assert!(RtenBackend::open(std::env::temp_dir(), "no-such-model").is_err());
    }
}
