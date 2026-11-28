use std::path::{Path, PathBuf};

/// Why a command failed: printed as `error: {e}`, exit 1. The engine, the MCP backends and the sandbox report their
/// failures as text; [`CliError::Engine`] carries it.
#[derive(Debug, thiserror::Error)]
pub(crate) enum CliError {
    /// What the engine, a backend or a library reported.
    #[error("{0}")]
    Engine(String),
    /// The engine's commands take paths as JSON, which holds only text.
    #[error("{}: the path is not valid Unicode", .0.display())]
    NotUnicode(PathBuf),
    #[error("{}: {source}", path.display())]
    File { path: PathBuf, source: std::io::Error },
    #[error("{out}: unknown output format (use .xlsx, .csv, .tsv, .json, .html or .pdf)")]
    OutputFormat { out: String },
    #[error("cannot connect to the GridCraft app at {addr}: {message} (start it with `gridcraft --control {port}`)")]
    Connect { addr: String, port: String, message: String },
    #[error("step {step} ({origin}, {id}): {message}")]
    Step { step: usize, origin: String, id: String, message: String },
    #[error("{origin}: {source}")]
    Script { origin: String, source: crate::script::ScriptError },
    #[error("sandbox-check: {0}")]
    Sandbox(String),
    /// Standard output went away.
    #[error("can't write to stdout: {0}")]
    Stdout(std::io::Error),
}

impl CliError {
    pub(crate) fn engine(e: impl ToString) -> Self {
        Self::Engine(e.to_string())
    }
}

/// A path as the text the engine's commands take.
pub(crate) fn unicode(path: &Path) -> Result<&str, CliError> {
    path.to_str().ok_or_else(|| CliError::NotUnicode(path.to_owned()))
}
