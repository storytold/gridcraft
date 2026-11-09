//! GridCraft's native translation backend: dense Qwen3 GGUF models, executed by `candle` on the
//! CPU.
//!
//! Qwen3 1.7B is Apache-2.0, dense and supported by candle's `quantized_qwen3`. Qwen3.5 is a
//! hybrid linear-attention architecture that candle does not implement, so it is not used. The
//! prompt is the Qwen chat format with `/no_think` (thinking mode off) and stops at
//! `<|im_end|>`/end-of-text; decoding is greedy, one segment at a time.
//!
//! The model is user-supplied (downloaded after consent or placed by hand), never committed and
//! never fetched during inference. Files are verified against
//! [`gridcraft_translate::manifest`] (size and SHA-256) before the GGUF is loaded. `candle` is a
//! pure-Rust CPU runtime; no accelerator is required and the web build does not include it.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::quantized_qwen3::ModelWeights;
use gridcraft_translate::manifest::{self, ModelFile, ModelManifest};
use gridcraft_translate::{Accelerator, Backend, BackendError, BatchRequest, Cancel};
use sha2::{Digest, Sha256};
use tokenizers::Tokenizer;

/// Prompt budget for one segment, including the chat wrapper. Qwen3's context is much larger, but
/// a cell segment has no business being a long document; hold inference bounded.
pub const MAX_PROMPT_TOKENS: usize = 2048;
/// New tokens per segment. A translation is shorter than its prompt; this is a runaway guard.
pub const MAX_NEW_TOKENS: usize = 256;

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
    #[error("the model file `{path}` is not a usable model: {reason}")]
    Format { path: String, reason: String },
    #[error("the model is not installed in `{0}`")]
    NotInstalled(String),
    #[error("{0}")]
    Other(String),
}

struct Loaded {
    model: Mutex<ModelWeights>,
    tokenizer: Tokenizer,
    im_end: u32,
    eos: u32,
}

/// The backend. Construction is cheap; the 1.8 GB GGUF loads (and verifies) on the first
/// translate, on the worker that runs the job.
pub struct CandleBackend {
    dir: PathBuf,
    manifest: &'static ModelManifest,
    loaded: OnceLock<Result<Loaded, String>>,
}

impl CandleBackend {
    pub fn open(root: impl AsRef<Path>, id: &str) -> Result<CandleBackend, ModelError> {
        let manifest = manifest::find(id).ok_or_else(|| ModelError::Other(format!("`{id}` is not a known translation model")))?;
        Ok(CandleBackend::with_manifest(root.as_ref().join(id), manifest))
    }

    pub fn open_dir(dir: impl AsRef<Path>) -> Result<CandleBackend, ModelError> {
        let dir = dir.as_ref();
        let name = dir.file_name().and_then(|n| n.to_str()).ok_or_else(|| ModelError::NotInstalled(dir.display().to_string()))?;
        let manifest = manifest::find(name).ok_or_else(|| ModelError::Other(format!("`{name}` is not a known translation model")))?;
        Ok(CandleBackend::with_manifest(dir.to_path_buf(), manifest))
    }

    pub fn with_manifest(dir: PathBuf, manifest: &'static ModelManifest) -> CandleBackend {
        CandleBackend { dir, manifest, loaded: OnceLock::new() }
    }

    pub fn model_dir(&self) -> &Path {
        &self.dir
    }

    pub fn manifest(&self) -> &'static ModelManifest {
        self.manifest
    }

    /// Verifies every manifest file and loads the GGUF. Cached after the first success.
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

    fn translate_one(
        &self,
        loaded: &Loaded,
        text: &str,
        from: Option<&gridcraft_translate::LangTag>,
        to: &gridcraft_translate::LangTag,
        cancel: &Cancel,
    ) -> Result<String, BackendError> {
        cancel.check()?;
        let target = to.english_name();
        // Name the real source language: the prompt used to say "English" for every pair, so
        // `zh -> en` asked for English into English and input already in the target came back
        // unchanged. With `from: auto` the model detects the source, so the prompt names none.
        let source = match from {
            Some(from) => format!("the following {} text", from.english_name()),
            None => "the following text".to_string(),
        };
        // Qwen's chat format with thinking forced off: the empty think block is what stops Qwen3
        // emitting a reasoning trace; `/no_think` alone is only a soft switch.
        let prompt = format!(
            "<|im_start|>user\nTranslate {source} to {target}. Reply with only the translation, no explanation.\n\n{text} /no_think<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"
        );
        let ids = loaded.tokenizer.encode(prompt.as_str(), true).map_err(|e| BackendError::Input(e.to_string()))?.get_ids().to_vec();
        if ids.is_empty() {
            return Ok(String::new());
        }
        if ids.len() >= MAX_PROMPT_TOKENS {
            return Err(BackendError::Input("a segment is longer than the model's prompt budget".into()));
        }
        let device = Device::Cpu;
        let mut model = match loaded.model.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        model.clear_kv_cache();
        let mut input = Tensor::new(ids.as_slice(), &device).and_then(|t| t.unsqueeze(0)).map_err(|e| BackendError::Internal(e.to_string()))?;
        let mut logits = model.forward(&input, 0).map_err(|e| BackendError::Internal(e.to_string()))?;
        let mut sampler = LogitsProcessor::new(1337, None, None);
        let mut generated: Vec<u32> = Vec::new();
        let mut offset = ids.len();
        let mut finished = false;
        loop {
            cancel.check()?;
            let row = logits.squeeze(0).map_err(|e| BackendError::Internal(e.to_string()))?;
            let token = sampler.sample(&row).map_err(|e| BackendError::Internal(e.to_string()))?;
            if token == loaded.im_end || token == loaded.eos {
                finished = true;
                break;
            }
            generated.push(token);
            if generated.len() >= MAX_NEW_TOKENS {
                break;
            }
            input = Tensor::new(&[token], &device).and_then(|t| t.unsqueeze(0)).map_err(|e| BackendError::Internal(e.to_string()))?;
            logits = model.forward(&input, offset).map_err(|e| BackendError::Internal(e.to_string()))?;
            offset += 1;
        }
        // A decode that hits the cap without its end token is a truncated sentence. Returning it
        // as `Ok` would mark the cell translated and write text that lost its tail.
        if !finished {
            return Err(BackendError::Input(format!("the translation did not finish within {MAX_NEW_TOKENS} tokens; split the cell and try again")));
        }
        let text = loaded.tokenizer.decode(&generated, true).map_err(|e| BackendError::Internal(e.to_string()))?;
        Ok(text.trim_end().to_string())
    }
}

impl Backend for CandleBackend {
    fn model_id(&self) -> &str {
        self.manifest.id
    }

    fn accelerator(&self) -> Accelerator {
        Accelerator::Cpu
    }

    fn supports(&self, from: &gridcraft_translate::LangTag, to: &gridcraft_translate::LangTag) -> bool {
        self.manifest.pairs.iter().any(|pair| {
            let (Ok(f), Ok(t)) = (gridcraft_translate::LangTag::parse(pair.from), gridcraft_translate::LangTag::parse(pair.to)) else {
                return false;
            };
            from.matches(&f) && to.matches(&t)
        })
    }

    fn supports_target(&self, to: &gridcraft_translate::LangTag) -> bool {
        self.manifest.pairs.iter().any(|pair| gridcraft_translate::LangTag::parse(pair.to).is_ok_and(|t| to.matches(&t)))
    }

    fn auto_source(&self) -> bool {
        // Qwen3 is multilingual: `auto` reaches the model, which translates into `to` directly.
        true
    }

    fn translate(&self, req: &BatchRequest<'_>, cancel: &Cancel) -> Result<Vec<String>, BackendError> {
        if !self.supports_target(req.to) {
            let from = req.from.map_or_else(|| "auto".to_string(), |f| f.canonical());
            return Err(BackendError::UnsupportedPair { from, to: req.to.canonical() });
        }
        let loaded = self.loaded()?;
        let mut out = Vec::with_capacity(req.texts.len());
        for text in req.texts {
            cancel.check()?;
            out.push(self.translate_one(loaded, text, req.from, req.to, cancel)?);
        }
        Ok(out)
    }
}

fn load(dir: &Path, manifest: &'static ModelManifest) -> Result<Loaded, ModelError> {
    verify_files(dir, manifest)?;
    // The manifest names its own weights (`Qwen3-1.7B-Q8_0.gguf` or `Qwen3-0.6B-Q8_0.gguf`);
    // a hard-coded name loads the 1.7B file for every model and fails for the 0.6B one.
    let weights = manifest.weights;
    let gguf = dir.join(weights);
    let mut file = std::fs::File::open(&gguf).map_err(|e| ModelError::Io(format!("{weights}: {e}")))?;
    let content = gguf_file::Content::read(&mut file).map_err(|e| ModelError::Format { path: weights.into(), reason: e.to_string() })?;
    let model =
        ModelWeights::from_gguf(content, &mut file, &Device::Cpu).map_err(|e| ModelError::Format { path: weights.into(), reason: e.to_string() })?;
    let tokenizer =
        Tokenizer::from_file(dir.join("tokenizer.json")).map_err(|e| ModelError::Format { path: "tokenizer.json".into(), reason: e.to_string() })?;
    let im_end = tokenizer.token_to_id("<|im_end|>").unwrap_or(151_645);
    let eos = tokenizer.token_to_id("<|endoftext|>").unwrap_or(151_643);
    Ok(Loaded { model: Mutex::new(model), tokenizer, im_end, eos })
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
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if !hex.eq_ignore_ascii_case(sha256) {
        return Err(ModelError::Hash(relative.to_string()));
    }
    Ok(())
}

/// The model root: `$GRIDCRAFT_MODELS`, the platform data directory, `models/` beside the
/// executable, or `models/` in a source checkout.
pub fn models_root() -> PathBuf {
    if let Some(root) = std::env::var_os("GRIDCRAFT_MODELS").filter(|v| !v.is_empty()) {
        return PathBuf::from(root);
    }
    let data = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("GridCraft"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support/GridCraft"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
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
    PathBuf::from("models")
}

/// Opens the installed model `id` when its directory holds every manifest file at its exact
/// byte size, falling back to any other complete manifest when `id` is unknown or incomplete,
/// so an install always becomes usable; `None` only when no complete model directory exists.
/// Size, not hash, is checked here: an interrupted 1.7B download never shadows a usable 0.6B
/// install, and the full SHA-256 stays on the inference worker so startup never hashes GBs.
pub fn installed_backend(root: &Path) -> Option<std::sync::Arc<dyn gridcraft_translate::Backend>> {
    installed_backend_for(root, "qwen3-1.7b")
        .or_else(|| manifest::MANIFESTS.iter().find_map(|m| (m.id != "qwen3-1.7b").then(|| installed_backend_for(root, m.id)).flatten()))
}

/// Opens the installed model `id` when its directory holds every manifest file at its exact
/// byte size; `None` otherwise (missing directory, unknown id, or an incomplete download).
/// Size, not hash: the worker verifies SHA-256 when it loads the model.
pub fn installed_backend_for(root: &Path, id: &str) -> Option<std::sync::Arc<dyn gridcraft_translate::Backend>> {
    if !manifest::valid_id(id) {
        return None;
    }
    let dir = root.join(id);
    let manifest = manifest::find(id)?;
    let complete = manifest.files.iter().all(|f| {
        manifest::valid_relative_path(f.path) && std::fs::metadata(dir.join(f.path)).is_ok_and(|meta| meta.is_file() && meta.len() == f.bytes)
    });
    if !complete {
        return None;
    }
    CandleBackend::open(root, id).ok().map(|b| std::sync::Arc::new(b) as std::sync::Arc<dyn gridcraft_translate::Backend>)
}

/// The model files a manifest lists.
pub fn files(manifest: &ModelManifest) -> &'static [ModelFile] {
    manifest.files
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
        let _ = std::fs::File::create(dir.join("empty.gguf"));
        assert!(matches!(verify_file(&dir, "empty.gguf", 0, &"0".repeat(64)), Err(ModelError::Hash(_))));
        let _ = std::fs::write(dir.join("short.gguf"), b"\x08\x07");
        assert!(matches!(verify_file(&dir, "short.gguf", 100, &"0".repeat(64)), Err(ModelError::Size { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn wrong_format_is_a_structured_error() {
        let dir = temp_dir("format");
        let _ = std::fs::write(dir.join("bad.gguf"), b"not a gguf model");
        let mut file = std::fs::File::open(dir.join("bad.gguf")).unwrap_or_else(|e| panic!("{e}"));
        assert!(gguf_file::Content::read(&mut file).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unknown_model_id_is_refused() {
        assert!(CandleBackend::open(std::env::temp_dir(), "../../etc").is_err());
        assert!(CandleBackend::open(std::env::temp_dir(), "no-such-model").is_err());
    }
}
