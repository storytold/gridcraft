//! What an interpreter is: the binary to start, its package folders, and what the sandbox must let
//! it read. A fixed script asks the interpreter once, outside the sandbox and with an empty
//! environment; no workbook code runs and no `.pth` file is read (`-S`).
//!
//! The worker runs without `site`, and for two kinds of Python not even from the path that was
//! found. A framework build (Homebrew, python.org) must run `Python.app`'s binary, because the
//! sandbox refuses the process `bin/python3` starts. A virtual environment's `bin/python` is a link
//! the sandbox resolves. So the package folders are worked out here and handed to the worker.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::interp::{ProbeError, Version, probe};

/// Bytes the script may print.
const INFO_BYTES: usize = 64 * 1024;
/// Links in a package folder followed to what they point at.
const MAX_LINKED: usize = 16;
/// Entries of a package folder looked at for links.
const MAX_SCANNED: usize = 10_000;

const INFO_SCRIPT: &str = r#"import json, site, sys, sysconfig
p = sysconfig.get_paths()
s = [p.get("purelib"), p.get("platlib")]
try:
    s += site.getsitepackages()
except Exception:
    pass
sys.stdout.write(json.dumps({
    "prefix": sys.prefix, "base_prefix": sys.base_prefix,
    "exec_prefix": sys.exec_prefix, "base_exec_prefix": sys.base_exec_prefix,
    "executable": sys.executable, "base_executable": getattr(sys, "_base_executable", "") or "",
    "framework": sysconfig.get_config_var("PYTHONFRAMEWORK") or "",
    "sites": [x for x in s if x],
}))
"#;

#[derive(Deserialize)]
struct Info {
    prefix: String,
    base_prefix: String,
    exec_prefix: String,
    base_exec_prefix: String,
    executable: String,
    base_executable: String,
    framework: String,
    sites: Vec<String>,
}

pub use crate::interpreter::{Interpreter, Kind, STARTER_PACKAGES, UV_INSTALL, pip_install_command, uv_install_commands, uv_setup_commands};

/// Asks the interpreter at `path` what it is.
pub fn describe(path: &Path, timeout: Duration) -> Result<Interpreter, ProbeError> {
    let version = probe(path, timeout)?;
    let mut cmd = std::process::Command::new(path);
    cmd.args(crate::process::PYTHON_FLAGS).args(["-c", INFO_SCRIPT]).env_clear().current_dir(std::env::temp_dir());
    let out = crate::process::run_bounded(&mut cmd, timeout, INFO_BYTES).ok_or(ProbeError::NoAnswer)?;
    let info: Info = serde_json::from_slice(&out).map_err(|_| ProbeError::NoAnswer)?;
    Ok(from_info(path, version, &info))
}

fn from_info(path: &Path, version: Version, info: &Info) -> Interpreter {
    let base = PathBuf::from(&info.base_prefix);
    let run_as = run_as(path, info, &base);
    let venv = venv_root(path);
    let site_dirs = site_dirs(venv.as_deref(), version, &info.sites);
    let kind = kind(venv.as_deref(), &run_as, info, &base);
    let mut read: Vec<PathBuf> =
        [&info.base_prefix, &info.base_exec_prefix, &info.prefix, &info.exec_prefix].into_iter().map(PathBuf::from).collect();
    read.extend(venv.iter().cloned());
    read.extend(site_dirs.iter().cloned());
    read.extend(store_paths(&base));
    for dir in &site_dirs {
        read.extend(linked_targets(dir));
    }
    Interpreter { path: path.to_path_buf(), run_as, version, kind, site_dirs, read_paths: outermost(read) }
}

/// The binary to start: `Python.app`'s for a framework build, otherwise the real file behind the
/// path (a link in a virtual environment, or a shell script from pyenv).
fn run_as(path: &Path, info: &Info, base: &Path) -> PathBuf {
    let app = base.join("Resources/Python.app/Contents/MacOS/Python");
    if !info.framework.is_empty() && app.is_file() {
        return app;
    }
    [&info.base_executable, &info.executable]
        .into_iter()
        .map(PathBuf::from)
        .chain([path.to_path_buf()])
        .filter(|p| p.is_absolute())
        .find_map(|p| std::fs::canonicalize(p).ok().filter(|c| c.is_file()))
        .unwrap_or_else(|| path.to_path_buf())
}

/// The virtual environment `path` belongs to: `pyvenv.cfg` sits next to `bin`.
fn venv_root(path: &Path) -> Option<PathBuf> {
    let root = path.parent()?.parent()?;
    root.join("pyvenv.cfg").is_file().then(|| root.to_path_buf())
}

fn venv_setting(root: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(root.join("pyvenv.cfg")).ok()?;
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// The package folders that exist: a virtual environment's own, and the base Python's only when
/// the environment asks for them (as `site` would do); otherwise the base Python's.
fn site_dirs(venv: Option<&Path>, version: Version, sites: &[String]) -> Vec<PathBuf> {
    let base = sites.iter().map(PathBuf::from);
    let mut dirs: Vec<PathBuf> = Vec::new();
    match venv {
        Some(root) => {
            let py = format!("python{}.{}", version.major, version.minor);
            for lib in ["lib", "lib64"] {
                dirs.push(root.join(lib).join(&py).join("site-packages"));
            }
            dirs.push(root.join("Lib/site-packages"));
            if venv_setting(root, "include-system-site-packages").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
                dirs.extend(base);
            }
        }
        None => dirs.extend(base),
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if let Ok(real) = std::fs::canonicalize(&dir)
            && real.is_dir()
            && !out.contains(&real)
        {
            out.push(real);
        }
    }
    out
}

fn kind(venv: Option<&Path>, run_as: &Path, info: &Info, base: &Path) -> Kind {
    if let Some(root) = venv {
        return if venv_setting(root, "uv").is_some() { Kind::UvEnvironment } else { Kind::Venv };
    }
    let text = run_as.to_string_lossy();
    if text.contains("/uv/python/") {
        Kind::UvPython
    } else if base.join("conda-meta").is_dir() {
        Kind::Conda
    } else if text.contains("/Cellar/") {
        Kind::Homebrew
    } else if !info.framework.is_empty() {
        Kind::Framework
    } else {
        Kind::System
    }
}

/// Stores a Python links into for its own libraries: Homebrew's `_ssl` and `_sqlite3` load from
/// `opt` and `Cellar`, and Nix keeps everything in its store. These hold installed software, not
/// the person's data (Homebrew's `etc` and `var` stay out).
fn store_paths(base: &Path) -> Vec<PathBuf> {
    let Ok(real) = std::fs::canonicalize(base) else { return Vec::new() };
    let text = real.to_string_lossy().into_owned();
    if let Some((brew, _)) = text.split_once("/Cellar/") {
        return vec![Path::new(brew).join("Cellar"), Path::new(brew).join("opt")];
    }
    if text.starts_with("/nix/store/") {
        return vec![PathBuf::from("/nix/store")];
    }
    Vec::new()
}

/// What links directly inside a package folder point at (a conda environment made with soft links,
/// a shared package cache), so those packages can be read.
fn linked_targets(dir: &Path) -> Vec<PathBuf> {
    let Ok(listing) = std::fs::read_dir(dir) else { return Vec::new() };
    listing
        .take(MAX_SCANNED)
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_symlink()))
        .filter_map(|e| std::fs::canonicalize(e.path()).ok())
        .filter(|target| !target.starts_with(dir))
        .take(MAX_LINKED)
        .collect()
}

/// The paths that exist, canonical, without one that another contains.
fn outermost(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut real: Vec<PathBuf> = paths.into_iter().filter(|p| p.is_absolute()).filter_map(|p| std::fs::canonicalize(p).ok()).collect();
    real.sort();
    real.dedup();
    let mut out: Vec<PathBuf> = Vec::new();
    for p in real {
        if !out.iter().any(|kept| p.starts_with(kept)) {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().canonicalize().unwrap().join(format!("gridcraft-describe-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const V: Version = Version { major: 3, minor: 12, patch: 1 };

    #[test]
    fn a_virtual_environment_uses_its_own_packages_unless_it_asks_for_the_bases() {
        let root = scratch("venv");
        let own = root.join("lib/python3.12/site-packages");
        std::fs::create_dir_all(&own).unwrap();
        let base = scratch("venv-base");
        let sites = vec![base.to_string_lossy().into_owned()];
        std::fs::write(root.join("pyvenv.cfg"), "home = /x\nuv = 0.5.0\n").unwrap();
        assert_eq!(site_dirs(Some(&root), V, &sites), vec![own.clone()]);
        std::fs::write(root.join("pyvenv.cfg"), "include-system-site-packages = true\n").unwrap();
        assert_eq!(site_dirs(Some(&root), V, &sites), vec![own, base.clone()]);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn the_kind_names_where_a_python_came_from() {
        let root = scratch("kind");
        let info = |framework: &str| Info {
            prefix: String::new(),
            base_prefix: String::new(),
            exec_prefix: String::new(),
            base_exec_prefix: String::new(),
            executable: String::new(),
            base_executable: String::new(),
            framework: framework.into(),
            sites: Vec::new(),
        };
        let none = Path::new("/none");
        std::fs::write(root.join("pyvenv.cfg"), "uv = 0.5.0\n").unwrap();
        assert_eq!(kind(Some(&root), none, &info(""), none), Kind::UvEnvironment);
        std::fs::write(root.join("pyvenv.cfg"), "home = /x\n").unwrap();
        assert_eq!(kind(Some(&root), none, &info(""), none), Kind::Venv);
        assert_eq!(kind(None, Path::new("/h/.local/share/uv/python/c/bin/python3"), &info(""), none), Kind::UvPython);
        assert_eq!(kind(None, Path::new("/opt/homebrew/Cellar/p/3/bin/python3"), &info("Python"), none), Kind::Homebrew);
        assert_eq!(kind(None, Path::new("/Library/Frameworks/x"), &info("Python"), none), Kind::Framework);
        assert_eq!(kind(None, Path::new("/usr/bin/python3"), &info(""), none), Kind::System);
        std::fs::create_dir(root.join("conda-meta")).unwrap();
        assert_eq!(kind(None, Path::new("/e/bin/python3"), &info(""), &root), Kind::Conda);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_folder_inside_another_is_left_out() {
        let root = scratch("outermost");
        let inner = root.join("a/b");
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(outermost(vec![inner, root.clone(), root.join("missing"), "relative".into()]), vec![root.clone()]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn links_in_a_package_folder_are_followed_once() {
        let root = scratch("links");
        let site = root.join("site");
        let outside = root.join("outside/pkg");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, site.join("pkg")).unwrap();
        std::fs::create_dir(site.join("inner")).unwrap();
        std::os::unix::fs::symlink(site.join("inner"), site.join("alias")).unwrap();
        assert_eq!(linked_targets(&site), vec![outside]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn homebrew_and_nix_add_their_stores() {
        assert!(store_paths(Path::new("/definitely/not/here")).is_empty());
    }

    #[test]
    fn install_commands_speak_uv_and_quote_paths() {
        let mut i = Interpreter {
            path: PathBuf::from("/a b/it's/python"),
            run_as: PathBuf::new(),
            version: V,
            kind: Kind::UvEnvironment,
            site_dirs: Vec::new(),
            read_paths: Vec::new(),
        };
        assert_eq!(
            uv_install_commands(&i, &["pandas"]),
            vec!["uv pip install --compile-bytecode --python '/a b/it'\\''s/python' pandas".to_string()]
        );
        assert_eq!(pip_install_command(&i, &["pandas"]), "'/a b/it'\\''s/python' -m pip install pandas");
        i.kind = Kind::Homebrew;
        assert_eq!(
            uv_install_commands(&i, &["pandas"]),
            vec![
                "uv venv ~/.gridcraft/python --python 3.12".to_string(),
                "uv pip install --compile-bytecode --python ~/.gridcraft/python pandas".to_string()
            ]
        );
    }
}
