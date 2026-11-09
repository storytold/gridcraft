//! The shipped model manifest: identity, pinned revision, file list with byte sizes and SHA-256,
//! and the model card's licence.
//!
//! No weights are in the repository. The user downloads a model (after consent) or places the
//! files in the model directory by hand; both paths end in the same verification. The manifest is
//! a table so another language pair is a new entry, not a new code path.

use serde::Serialize;

/// One model's metadata and files.
#[derive(Clone, Copy, Debug)]
pub struct ModelManifest {
    /// Stable id and directory name (`qwen3-1.7b`).
    pub id: &'static str,
    pub name: &'static str,
    /// Hugging Face repository or other source.
    pub source: &'static str,
    /// Immutable commit revision, never a moving branch or `refs/pr/N`.
    pub revision: &'static str,
    /// The model card's licence, recorded per pair (OPUS-MT licences vary).
    pub license: &'static str,
    pub license_url: &'static str,
    pub license_note: &'static str,
    pub files: &'static [ModelFile],
    /// The GGUF weights inside `files`.
    pub weights: &'static str,
    pub pairs: &'static [LanguagePair],
    /// Approximate resident memory the loaded backend needs, for pre-flight admission.
    pub min_ram_mb: u32,
    /// A short runtime/accelerator hint shown before download.
    pub runtime: &'static str,
}

/// A file the model needs, and the hash that makes it reproducible.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ModelFile {
    /// Relative path inside the model directory, with `/` separators.
    pub path: &'static str,
    /// The exact pinned download URL (a model may span two repositories: weights and tokenizer).
    pub url: &'static str,
    pub bytes: u64,
    /// Lower-case hex SHA-256 of the file.
    pub sha256: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct LanguagePair {
    pub from: &'static str,
    pub to: &'static str,
}

/// The working v1 model: Qwen3 1.7B, a dense Apache-2.0 multilingual model quantized to Q8_0.
///
/// Qwen3.5 is a hybrid linear-attention architecture with no implementation in candle, so the
/// dense Qwen3 is used. The GGUF weights and the tokenizer are pinned to their own immutable
/// commits (the GGUF repository does not ship the tokenizer).
pub const QWEN3_1_7B: ModelManifest = ModelManifest {
    id: "qwen3-1.7b",
    name: "Qwen3 1.7B (multilingual translation)",
    source: "Qwen/Qwen3-1.7B-GGUF + Qwen/Qwen3-1.7B tokenizer",
    revision: "90862c4b9d2787eaed51d12237eafdfe7c5f6077",
    license: "Apache-2.0",
    license_url: "https://huggingface.co/Qwen/Qwen3-1.7B",
    license_note: "Qwen3 1.7B weights and tokenizer, Apache-2.0. Qwen3.5 is a different (hybrid linear-attention) architecture and is not used.",
    files: &[
        ModelFile {
            path: "Qwen3-1.7B-Q8_0.gguf",
            url: "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/90862c4b9d2787eaed51d12237eafdfe7c5f6077/Qwen3-1.7B-Q8_0.gguf",
            bytes: 1_834_426_016,
            sha256: "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a",
        },
        ModelFile {
            path: "tokenizer.json",
            url: "https://huggingface.co/Qwen/Qwen3-1.7B/resolve/70d244cc86ccca08cf5af4e1e306ecf908b1ad5e/tokenizer.json",
            bytes: 11_422_654,
            sha256: "aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4",
        },
    ],
    weights: "Qwen3-1.7B-Q8_0.gguf",
    pairs: &[
        LanguagePair { from: "en", to: "es" },
        LanguagePair { from: "en", to: "de" },
        LanguagePair { from: "en", to: "ja" },
        LanguagePair { from: "zh", to: "en" },
    ],
    min_ram_mb: 3072,
    runtime: "candle 0.11 quantized Qwen3 (Q8_0), pure-Rust CPU; ~1.8 GB on disk; not enabled in the web build",
};

/// The small alternative: Qwen3 0.6B Q8_0. Same tokenizer as the 1.7B, its own revision.
pub const QWEN3_0_6B: ModelManifest = ModelManifest {
    id: "qwen3-0.6b",
    name: "Qwen3 0.6B (multilingual translation, small)",
    source: "Qwen/Qwen3-0.6B-GGUF + Qwen/Qwen3-0.6B tokenizer",
    revision: "23749fefcc72300e3a2ad315e1317431b06b590a",
    license: "Apache-2.0",
    license_url: "https://huggingface.co/Qwen/Qwen3-0.6B",
    license_note: "Qwen3 0.6B weights and tokenizer, Apache-2.0. Smaller and faster; prefer the 1.7B for quality.",
    files: &[
        ModelFile {
            path: "Qwen3-0.6B-Q8_0.gguf",
            url: "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/23749fefcc72300e3a2ad315e1317431b06b590a/Qwen3-0.6B-Q8_0.gguf",
            bytes: 639_446_688,
            sha256: "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
        },
        ModelFile {
            path: "tokenizer.json",
            url: "https://huggingface.co/Qwen/Qwen3-0.6B/resolve/c1899de289a04d12100db370d81485cdf75e47ca/tokenizer.json",
            bytes: 11_422_654,
            sha256: "aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4",
        },
    ],
    weights: "Qwen3-0.6B-Q8_0.gguf",
    pairs: &[
        LanguagePair { from: "en", to: "es" },
        LanguagePair { from: "en", to: "de" },
        LanguagePair { from: "en", to: "ja" },
        LanguagePair { from: "zh", to: "en" },
    ],
    min_ram_mb: 1536,
    runtime: "candle 0.11 quantized Qwen3 (Q8_0), pure-Rust CPU; ~639 MB on disk; faster and less precise than 1.7B",
};

/// Every model this build knows about. More pairs are new rows.
pub const MANIFESTS: &[ModelManifest] = &[QWEN3_1_7B, QWEN3_0_6B];

pub fn find(id: &str) -> Option<&'static ModelManifest> {
    MANIFESTS.iter().find(|m| m.id == id)
}

/// Total download size in bytes.
pub fn total_bytes(m: &ModelManifest) -> u64 {
    m.files.iter().fold(0u64, |sum, f| sum.saturating_add(f.bytes))
}

/// Model ids are directory names: lower-case ASCII, digits and single hyphens.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        && !id.starts_with('-')
        && !id.starts_with('.')
        && !id.ends_with('-')
        && !id.ends_with('.')
        && !id.contains("--")
        && !id.contains("..")
}

/// Whether a manifest path is a plain relative path with no traversal, drives or surprising
/// characters. Model files are untrusted input, so paths are checked before any join.
pub fn valid_relative_path(path: &str) -> bool {
    if path.is_empty() || path.len() > 240 || path.starts_with('/') || path.starts_with('\\') {
        return false;
    }
    if path.contains('\\') || path.contains(':') || path.contains('\0') {
        return false;
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 8 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty()
            && *part != "."
            && *part != ".."
            && part.len() <= 100
            && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    })
}

/// The manifest's file entry for a relative path.
pub fn file(m: &ModelManifest, path: &str) -> Option<&'static ModelFile> {
    m.files.iter().find(|f| f.path == path)
}

/// Checks that the manifest itself is well-formed. A test enforces this for every entry; a bad
/// entry is a build bug, but callers still get an error instead of an index panic.
pub fn validate(m: &ModelManifest) -> Result<(), String> {
    if !valid_id(m.id) {
        return Err(format!("model id `{}` is not a directory name", m.id));
    }
    if m.files.is_empty() {
        return Err(format!("model `{}` has no files", m.id));
    }
    // The loader opens `weights`; a name that is not one of the listed files means the model
    // cannot load even though every file verified.
    if file(m, m.weights).is_none() {
        return Err(format!("model `{}` names `{}` as its weights, which is not one of its files", m.id, m.weights));
    }
    for f in m.files {
        if !valid_relative_path(f.path) {
            return Err(format!("model `{}` has an unsafe path `{}`", m.id, f.path));
        }
        if f.sha256.len() != 64 || !f.sha256.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
            return Err(format!("model `{}` file `{}` has an invalid SHA-256", m.id, f.path));
        }
        if f.bytes == 0 {
            return Err(format!("model `{}` file `{}` is empty", m.id, f.path));
        }
    }
    if m.pairs.is_empty() {
        return Err(format!("model `{}` lists no language pairs", m.id));
    }
    if m.license.trim().is_empty() || m.license_url.trim().is_empty() {
        return Err(format!("model `{}` records no licence", m.id));
    }
    if m.revision.len() < 7 || m.revision.eq_ignore_ascii_case("main") || m.revision.starts_with("refs/") {
        return Err(format!("model `{}` is not pinned to an immutable revision", m.id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_manifest_is_valid() {
        for m in MANIFESTS {
            validate(m).unwrap_or_else(|e| panic!("{e}"));
        }
    }

    #[test]
    fn the_working_pair_is_en_es_and_hashed() {
        let m = find("qwen3-1.7b").expect("manifest");
        assert!(m.pairs.iter().any(|p| (p.from, p.to) == ("en", "es")));
        assert!(m.pairs.iter().any(|p| (p.from, p.to) == ("zh", "en")));
        assert_eq!(m.license, "Apache-2.0");
        assert_eq!(total_bytes(m), 1_845_848_670);
    }

    #[test]
    fn traversal_paths_are_refused() {
        assert!(valid_relative_path("onnx/model.onnx"));
        for bad in ["", "../model.onnx", "onnx/../../x", "/etc/passwd", "C:/x", "onnx\\x", "a//b", "onnx/./x", "a b"] {
            assert!(!valid_relative_path(bad), "{bad}");
        }
    }

    #[test]
    fn ids_are_directory_names() {
        assert!(valid_id("qwen3-1.7b"));
        for bad in ["", "../x", "Opus", "a/b", "-x", "x-", "a--b"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }
}
