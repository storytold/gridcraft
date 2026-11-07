//! The native local-model host: where models live, and the one HTTPS transport a user-consented
//! download uses.
//!
//! The policy is in `MODEL-POLICY.md` and `docs/translate.md`: runtimes ship, weights do not. A
//! download is always user-initiated, shows source/size/licence/hash first, and is verified before
//! it is installed. Nothing here runs at startup beyond resolving a directory; inference never
//! touches the network.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gridcraft_engine::Session;
use gridcraft_engine::translate::ModelFetcher;
use gridcraft_ui_egui::ModelHost;

/// How long one model file may take; a stalled download is an error, not a hang.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
/// A sanity bound before the manifest's exact size check; no model here is near it.
const MAX_FILE_BYTES: usize = 512 * 1024 * 1024;

pub struct Models {
    root: PathBuf,
}

impl Models {
    pub fn new() -> Models {
        Models { root: gridcraft_translate_rten::models_root() }
    }
}

impl Default for Models {
    fn default() -> Self {
        Models::new()
    }
}

impl ModelHost for Models {
    fn models_dir(&self) -> PathBuf {
        self.root.clone()
    }

    fn download(&self, id: &str) -> Result<(), String> {
        // Run the engine command so consent, manifest paths, sizes and SHA-256 are checked in one
        // place. A throwaway session is enough: the command touches no workbook.
        let mut session = Session::new();
        session.translate_models_dir = self.root.display().to_string();
        session.translate_fetcher = Some(Arc::new(HttpFetcher));
        session.execute("translate.modelDownload", serde_json::json!({"id": id, "consent": true})).map(|_| ()).map_err(|e| e.to_string())
    }
}

/// The HTTPS transport for model files. Only a user-initiated `translate.modelDownload` call
/// reaches it; no inference path holds one.
pub struct HttpFetcher;

impl ModelFetcher for HttpFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        let agent = ureq::Agent::config_builder().timeout_global(Some(DOWNLOAD_TIMEOUT)).max_response_header_size(64 * 1024).build().new_agent();
        let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
        if let Some(length) = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<usize>().ok())
            && length > MAX_FILE_BYTES
        {
            return Err(format!("the file is larger than {MAX_FILE_BYTES} bytes"));
        }
        let bytes = response.body_mut().read_to_vec().map_err(|e| e.to_string())?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(format!("the file is larger than {MAX_FILE_BYTES} bytes"));
        }
        Ok(bytes)
    }
}

/// Resolves the model root, installs the download transport and injects a lazy backend when the
/// model directory exists. Called once at startup and again after an install/removal.
pub fn refresh(session: &mut Session) {
    let root = gridcraft_translate_rten::models_root();
    session.translate_models_dir = root.display().to_string();
    session.translate_fetcher = Some(Arc::new(HttpFetcher));
    session.translate_backend = gridcraft_translate_rten::installed_backend(&root);
}
