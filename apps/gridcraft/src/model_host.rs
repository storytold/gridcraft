//! The native local-model host: where models live, and the one HTTPS transport a user-consented
//! download uses.
//!
//! The policy is in `MODEL-POLICY.md` and `docs/translate.md`: runtimes ship, weights do not. A
//! download is always user-initiated, shows source/size/licence/hash first, and is verified before
//! it is installed. Nothing here runs at startup beyond resolving a directory; inference never
//! touches the network.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gridcraft_engine::Session;
use gridcraft_engine::translate::ModelFetcher;
use gridcraft_ui_egui::ModelHost;

/// How long one phase of a download may take. A whole-file deadline would fail a large model on a
/// slow link, so each phase and each read is bounded: a stalled transfer is an error, a slow but
/// progressing one finishes.
const DOWNLOAD_PHASE_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(60);

pub struct Models {
    root: PathBuf,
}

impl Models {
    pub fn new() -> Models {
        Models { root: gridcraft_translate_candle::models_root() }
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
    fn fetch(&self, url: &str, dest: &Path, max_bytes: u64) -> Result<(), String> {
        let agent = ureq::Agent::config_builder()
            .timeout_resolve(Some(DOWNLOAD_PHASE_TIMEOUT))
            .timeout_connect(Some(DOWNLOAD_PHASE_TIMEOUT))
            .timeout_send_request(Some(DOWNLOAD_READ_TIMEOUT))
            .timeout_recv_response(Some(DOWNLOAD_READ_TIMEOUT))
            .timeout_per_read(Some(DOWNLOAD_READ_TIMEOUT))
            .max_response_header_size(64 * 1024)
            .build()
            .new_agent();
        let mut response = agent.get(url).call().map_err(|e| e.to_string())?;
        if let Some(length) = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok())
            && length > max_bytes
        {
            return Err(format!("the file is larger than the {max_bytes} bytes the manifest declares"));
        }
        // The manifest's size is also the body limit, so a server that lies about the length
        // cannot stream more than the manifest allows into the staging file.
        let mut body = response.body_mut().with_config().limit(max_bytes).reader();
        let mut file = std::fs::File::create(dest).map_err(|e| format!("could not create {}: {e}", dest.display()))?;
        std::io::copy(&mut body, &mut file).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Resolves the model root, installs the download transport and injects a lazy backend when the
/// model directory exists. Called once at startup and again after an install/removal. Also
/// installs the opener the engine's install commands call to refresh the running session's
/// backend after their own verified install, so a model-less session translates right away.
pub fn refresh(session: &mut Session) {
    let root = gridcraft_translate_candle::models_root();
    session.translate_models_dir = root.display().to_string();
    session.translate_fetcher = Some(Arc::new(HttpFetcher));
    session.translate_backend = gridcraft_translate_candle::installed_backend(&root);
    session.translate_backend_opener = Some(Arc::new(|s: &Session, id: &str| {
        (!s.translate_models_dir.trim().is_empty() && gridcraft_translate::manifest::valid_id(id))
            .then(|| gridcraft_translate_candle::installed_backend_for(std::path::Path::new(&s.translate_models_dir), id))
    }));
}
