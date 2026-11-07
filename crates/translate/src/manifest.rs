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
    /// Stable id and directory name (`opus-mt-en-es`).
    pub id: &'static str,
    pub name: &'static str,
    /// Hugging Face repository or other source.
    pub source: &'static str,
    /// Immutable commit revision, never a moving branch or `refs/pr/N`.
    pub revision: &'static str,
    /// Prefix the file paths are appended to.
    pub url_base: &'static str,
    /// The model card's licence, recorded per pair (OPUS-MT licences vary).
    pub license: &'static str,
    pub license_url: &'static str,
    pub license_note: &'static str,
    pub files: &'static [ModelFile],
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
    pub bytes: u64,
    /// Lower-case hex SHA-256 of the file.
    pub sha256: &'static str,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct LanguagePair {
    pub from: &'static str,
    pub to: &'static str,
}

/// The working v1 pair: OPUS-MT English→Spanish, the ONNX conversion, quantized weights.
///
/// The ONNX conversion is pinned to the conversion commit (not to the mutable `refs/pr` ref the
/// candle example used), and the quantized files are the ones measured token-for-token against
/// onnxruntime. `vocab.json` and the two sentencepiece models come from the same revision as the
/// conversion, because the Xenova vocabulary differs from the Helsinki main-branch one.
pub const OPUS_MT_EN_ES: ModelManifest = ModelManifest {
    id: "opus-mt-en-es",
    name: "OPUS-MT English → Spanish",
    source: "Xenova/opus-mt-en-es",
    revision: "4b002a4c7edd54a7ced58877258b87f7efd3f892",
    url_base: "https://huggingface.co/Xenova/opus-mt-en-es/resolve/4b002a4c7edd54a7ced58877258b87f7efd3f892/",
    license: "Apache-2.0",
    license_url: "https://huggingface.co/Xenova/opus-mt-en-es",
    license_note: "ONNX conversion of Helsinki-NLP/opus-mt-en-es; the model card is Apache-2.0.",
    files: &[
        ModelFile {
            path: "onnx/encoder_model_quantized.onnx",
            bytes: 52_899_742,
            sha256: "03434edd1147e6c49e22066e9dc5888235e99cc35a6711d1c4ea5b06455d5da9",
        },
        ModelFile {
            path: "onnx/decoder_model_quantized.onnx",
            bytes: 59_842_102,
            sha256: "2b45074cc819f1d901e40510d7bc0781d99327d16c7eca04518c4ee0c3d6ba10",
        },
        ModelFile { path: "source.spm", bytes: 801_636, sha256: "4dd547c24816a335e7b0b2e63376a8f1b3cbfc671eda5ab808dd44fdadaa8791" },
        ModelFile { path: "target.spm", bytes: 825_924, sha256: "e236ee6d866b635c0142114f8647f39831f9d92534aa2aad75c942f6a78ad0e3" },
        ModelFile { path: "vocab.json", bytes: 1_720_044, sha256: "b074b4cca0036ade5a39ea97faabd534e1015482c480fc2cb02c6481983eb163" },
        ModelFile { path: "config.json", bytes: 1_468, sha256: "499eae1f97eeba63172fc884f92fd727ae803b7ceb79c0f653aa4b011152af2f" },
    ],
    pairs: &[LanguagePair { from: "en", to: "es" }],
    min_ram_mb: 512,
    runtime: "rten 0.27, pure-Rust CPU; runs on Windows, macOS, Linux and BSD; not enabled in the web build",
};

/// Every model this build knows about. More pairs are new rows.
pub const MANIFESTS: &[ModelManifest] = &[OPUS_MT_EN_ES];

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
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
        && !id.contains("--")
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
        let m = find("opus-mt-en-es").expect("manifest");
        assert_eq!(m.pairs.len(), 1);
        assert_eq!((m.pairs[0].from, m.pairs[0].to), ("en", "es"));
        assert_eq!(m.license, "Apache-2.0");
        assert_eq!(total_bytes(m), 116_090_916);
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
        assert!(valid_id("opus-mt-en-es"));
        for bad in ["", "../x", "Opus", "a/b", "-x", "x-", "a--b"] {
            assert!(!valid_id(bad), "{bad}");
        }
    }
}
