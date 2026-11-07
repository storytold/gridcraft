//! What an interpreter is, as the rest of the app reads it: on every build, the web one included,
//! where no interpreter is ever found. `describe` works one out.

use std::path::{Path, PathBuf};

use crate::interp::Version;

/// Where a Python came from, as the person would name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An environment made with `uv venv`.
    UvEnvironment,
    /// One of uv's own Pythons.
    UvPython,
    /// A virtual environment made another way.
    Venv,
    Conda,
    Homebrew,
    /// python.org's installer, or another framework build.
    Framework,
    System,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::UvEnvironment => "uv environment",
            Kind::UvPython => "uv Python",
            Kind::Venv => "virtual environment",
            Kind::Conda => "conda environment",
            Kind::Homebrew => "Homebrew",
            Kind::Framework => "python.org",
            Kind::System => "system",
        }
    }

    /// Whether packages can be installed into it: a Python outside an environment is usually
    /// managed by something else, which refuses them.
    pub fn is_environment(self) -> bool {
        matches!(self, Kind::UvEnvironment | Kind::Venv | Kind::Conda)
    }
}

/// An interpreter, described: what to start, its package folders, what it must read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interpreter {
    /// The path found or chosen.
    pub path: PathBuf,
    /// The binary to start.
    pub run_as: PathBuf,
    pub version: Version,
    pub kind: Kind,
    /// Folders of installed packages, to go on `sys.path`.
    pub site_dirs: Vec<PathBuf>,
    /// What the sandbox must let it read.
    pub read_paths: Vec<PathBuf>,
}

/// Packages to suggest for a new environment: what Excel's Python makes available first.
pub const STARTER_PACKAGES: [&str; 5] = ["numpy", "pandas", "matplotlib", "seaborn", "statsmodels"];
/// Installs uv, for a person who doesn't have it.
pub const UV_INSTALL: &str = "curl -LsSf https://astral.sh/uv/install.sh | sh";

/// A path as text a shell reads back as one word.
fn quoted(path: &Path) -> String {
    let text = path.to_string_lossy();
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "/._-~".contains(c)) {
        return text.into_owned();
    }
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The commands that make the suggested environment with uv and install `packages` in it. The
/// search finds that environment first, so nothing else needs choosing. Bytecode is compiled at
/// install time because the sandbox can't write it later: without it, the first plot in every new
/// worker takes about 10 seconds where it otherwise takes about 0.3.
pub fn uv_setup_commands(packages: &[&str]) -> Vec<String> {
    let env = format!("~/{}", crate::interp::RECOMMENDED_ENV);
    let mut out = vec![format!("uv venv {env} --python 3.12")];
    if !packages.is_empty() {
        out.push(format!("uv pip install --compile-bytecode --python {env} {}", packages.join(" ")));
    }
    out
}

/// The commands that add `packages` for this interpreter, with uv: into it when it is an
/// environment, otherwise into a new environment, since a Python outside one refuses them.
pub fn uv_install_commands(interpreter: &Interpreter, packages: &[&str]) -> Vec<String> {
    if interpreter.kind.is_environment() {
        vec![format!("uv pip install --compile-bytecode --python {} {}", quoted(&interpreter.path), packages.join(" "))]
    } else {
        uv_setup_commands(packages)
    }
}

/// The same with pip, for people who manage their Pythons another way.
pub fn pip_install_command(interpreter: &Interpreter, packages: &[&str]) -> String {
    format!("{} -m pip install {}", quoted(&interpreter.path), packages.join(" "))
}
