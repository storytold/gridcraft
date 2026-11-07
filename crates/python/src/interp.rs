//! Finding the user's Python: one explicit path given by the caller, or the search in
//! [`candidates`], where uv's environments and Pythons come first. Only absolute paths to existing
//! files count: a relative `PATH` entry (`.` included) is skipped, so the folder the app runs in
//! can't plant an interpreter. Each candidate is asked its version with a hard deadline and a
//! bounded read; one that hangs or floods is killed and counted as not found. Python 3.11 or newer
//! is needed.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A Python version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Version {
    /// `3.12.1` (with an optional newline).
    pub fn parse(s: &str) -> Option<Version> {
        let mut parts = s.trim().split('.');
        let v = Version { major: parts.next()?.parse().ok()?, minor: parts.next()?.parse().ok()?, patch: parts.next()?.parse().ok()? };
        parts.next().is_none().then_some(v)
    }
}

/// The oldest Python that runs cells.
pub const MIN_VERSION: Version = Version { major: 3, minor: 11, patch: 0 };
/// How long a version probe may take.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Bytes a version probe may print.
#[cfg(not(target_arch = "wasm32"))]
const PROBE_BYTES: usize = 64;
/// A cell's code that lists what is installed in the interpreter that runs it, as rows of name and
/// version: run through the confined worker, so it sees what cells see.
pub const LIST_PACKAGES: &str = "import importlib.metadata as m\nsorted(([d.metadata['Name'] or '', d.version or ''] for d in m.distributions()), key=lambda r: r[0].lower()) or None";
/// The probe's script: the version, on a Python that may be too old for anything newer.
pub const VERSION_SCRIPT: &str = "import sys; sys.stdout.write('%d.%d.%d\\n' % tuple(sys.version_info[:3]))";

/// What the search found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detection {
    Found {
        path: PathBuf,
        version: Version,
    },
    /// No usable interpreter answered.
    NoneFound,
    /// The first interpreter that answered is older than [`MIN_VERSION`], and none newer was found.
    TooOld {
        path: PathBuf,
        version: Version,
    },
    /// The path chosen is Apple's `/usr/bin/python3`, a stub that may open an installer window. It
    /// isn't asked anything.
    Stub {
        path: PathBuf,
    },
}

/// Why a probe gave no version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// The path isn't absolute, or isn't an existing file.
    NotAFile,
    /// It didn't run, or didn't answer within the deadline, or printed too much or something
    /// that isn't a version.
    NoAnswer,
}

/// Where to look: the environment variables the search reads, so tests can give their own.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchEnv {
    pub virtual_env: Option<OsString>,
    pub conda_prefix: Option<OsString>,
    pub path: Option<OsString>,
    pub home: Option<OsString>,
    /// `UV_PYTHON_INSTALL_DIR`: where uv keeps its Pythons, when it isn't the default.
    pub uv_python_dir: Option<OsString>,
    /// `XDG_DATA_HOME`: uv keeps its Pythons under it, by default `~/.local/share`.
    pub xdg_data_home: Option<OsString>,
    /// Also look where Pythons are usually installed, outside `PATH`.
    pub known_dirs: bool,
}

impl SearchEnv {
    /// This process's environment.
    pub fn from_process() -> SearchEnv {
        SearchEnv {
            virtual_env: std::env::var_os("VIRTUAL_ENV"),
            conda_prefix: std::env::var_os("CONDA_PREFIX"),
            path: std::env::var_os("PATH"),
            home: std::env::var_os("HOME"),
            uv_python_dir: std::env::var_os("UV_PYTHON_INSTALL_DIR"),
            xdg_data_home: std::env::var_os("XDG_DATA_HOME"),
            known_dirs: true,
        }
    }

    /// The environment GridCraft suggests making with uv, under the home directory.
    pub fn recommended_env(&self) -> Option<PathBuf> {
        let home = PathBuf::from(self.home.as_ref()?);
        home.is_absolute().then(|| home.join(RECOMMENDED_ENV))
    }

    fn uv_pythons_dir(&self) -> Option<PathBuf> {
        let dir = match (&self.uv_python_dir, &self.xdg_data_home, &self.home) {
            (Some(d), _, _) => PathBuf::from(d),
            (None, Some(d), _) => PathBuf::from(d).join("uv/python"),
            (None, None, Some(h)) => PathBuf::from(h).join(".local/share/uv/python"),
            (None, None, None) => return None,
        };
        dir.is_absolute().then_some(dir)
    }
}

/// The environment GridCraft suggests making with uv, relative to the home directory.
pub const RECOMMENDED_ENV: &str = ".gridcraft/python";
/// Apple's stub: it starts the Command Line Tools' Python (too old), or opens an installer window.
const APPLE_STUB: &str = "/usr/bin/python3";
/// Pythons of uv's own store looked at.
const MAX_UV_PYTHONS: usize = 64;

#[cfg(windows)]
const NAMES: [&str; 2] = ["python3.exe", "python.exe"];
#[cfg(not(windows))]
const NAMES: [&str; 2] = ["python3", "python"];
/// Where an environment keeps its interpreter.
#[cfg(windows)]
const ENV_BIN: &[&str] = &["Scripts", ""];
#[cfg(not(windows))]
const ENV_BIN: &[&str] = &["bin"];

/// The paths to try, in order, without duplicates: absolute and existing files only.
///
/// uv comes first: the environment GridCraft suggests making with it, the active virtual
/// environment (which `uv venv` makes), then uv's own Pythons, newest first. After those, for people
/// who use other tools: the active conda environment, `PATH`, and the places Pythons are usually
/// installed, which matter because an app started from the Dock doesn't get the shell's `PATH`.
pub fn candidates(env: &SearchEnv) -> Vec<PathBuf> {
    let env_dirs = |prefix: PathBuf| -> Vec<PathBuf> { ENV_BIN.iter().map(|b| if b.is_empty() { prefix.clone() } else { prefix.join(b) }).collect() };
    let mut dirs: Vec<PathBuf> = Vec::new();
    for prefix in [env.recommended_env(), env.virtual_env.as_ref().map(PathBuf::from)].into_iter().flatten() {
        dirs.extend(env_dirs(prefix));
    }
    dirs.extend(uv_python_bins(env));
    if let Some(prefix) = &env.conda_prefix {
        dirs.extend(env_dirs(PathBuf::from(prefix)));
    }
    let on_path: Vec<PathBuf> = env.path.as_ref().map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if usable_file(&p) && !out.contains(&p) && !is_apple_stub(&p) {
            out.push(p);
        }
    };
    for d in dirs.iter().filter(|d| d.is_absolute()) {
        for n in NAMES {
            push(d.join(n));
        }
    }
    // `python3` anywhere on PATH comes before `python`.
    for n in NAMES {
        for d in on_path.iter().filter(|d| d.is_absolute()) {
            push(d.join(n));
        }
    }
    if env.known_dirs {
        for d in known_dirs(env) {
            for n in NAMES {
                push(d.join(n));
            }
        }
    }
    out
}

/// The `bin` folders of uv's own Pythons, newest first. A folder is named like
/// `cpython-3.12.13-macos-aarch64-none`.
fn uv_python_bins(env: &SearchEnv) -> Vec<PathBuf> {
    let Some(dir) = env.uv_pythons_dir() else { return Vec::new() };
    let Ok(listing) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<((u32, u32, u32), PathBuf)> = listing
        .take(MAX_UV_PYTHONS)
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let mut parts = name.to_str()?.strip_prefix("cpython-")?.split('-').next()?.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
            Some(((major, minor, patch), e.path().join("bin")))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, bin)| bin).collect()
}

/// Where Pythons are usually installed, outside `PATH`.
fn known_dirs(env: &SearchEnv) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.push("/opt/homebrew/bin".into());
        dirs.push("/usr/local/bin".into());
        // python.org's installer, newest first.
        let versions = Path::new("/Library/Frameworks/Python.framework/Versions");
        if let Ok(listing) = std::fs::read_dir(versions) {
            let mut named: Vec<PathBuf> = listing.take(MAX_UV_PYTHONS).flatten().map(|e| e.path().join("bin")).collect();
            named.sort();
            named.reverse();
            dirs.extend(named);
        }
    } else if cfg!(unix) {
        dirs.push("/usr/bin".into());
        dirs.push("/usr/local/bin".into());
    }
    if let Some(home) = &env.home {
        let pyenv = PathBuf::from(home).join(".pyenv/versions");
        if let Ok(listing) = std::fs::read_dir(pyenv) {
            let mut named: Vec<PathBuf> = listing.take(MAX_UV_PYTHONS).flatten().map(|e| e.path().join("bin")).collect();
            named.sort();
            named.reverse();
            dirs.extend(named);
        }
    }
    dirs
}

fn is_apple_stub(p: &Path) -> bool {
    cfg!(target_os = "macos") && p == Path::new(APPLE_STUB)
}

fn usable_file(p: &Path) -> bool {
    p.is_absolute() && p.is_file()
}

/// Where uv is: on `PATH`, or where its installer and Homebrew put it.
pub fn find_uv(env: &SearchEnv) -> Option<PathBuf> {
    let name = if cfg!(windows) { "uv.exe" } else { "uv" };
    let mut dirs: Vec<PathBuf> = env.path.as_ref().map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    if let Some(home) = &env.home {
        dirs.push(PathBuf::from(home).join(".local/bin"));
        dirs.push(PathBuf::from(home).join(".cargo/bin"));
    }
    dirs.push("/opt/homebrew/bin".into());
    dirs.push("/usr/local/bin".into());
    dirs.into_iter().filter(|d| d.is_absolute()).map(|d| d.join(name)).find(|p| p.is_file())
}

/// Finds an interpreter: `explicit` alone when given, otherwise the
/// [`candidates`] in order, the first new enough winning.
pub fn detect(env: &SearchEnv, explicit: Option<&Path>, timeout: Duration) -> Detection {
    let list = match explicit {
        Some(p) if is_apple_stub(p) => return Detection::Stub { path: p.to_path_buf() },
        Some(p) => vec![p.to_path_buf()],
        None => candidates(env),
    };
    let mut too_old = None;
    for path in list {
        match probe(&path, timeout) {
            Ok(version) if version >= MIN_VERSION => return Detection::Found { path, version },
            Ok(version) => {
                too_old.get_or_insert(Detection::TooOld { path, version });
            }
            Err(_) => {}
        }
    }
    too_old.unwrap_or(Detection::NoneFound)
}

/// Asks the interpreter at `path` its version: `<python> -I -S -E -c <script>`, with an empty
/// environment, within `timeout`, reading at most a few bytes.
#[cfg(not(target_arch = "wasm32"))]
pub fn probe(path: &Path, timeout: Duration) -> Result<Version, ProbeError> {
    if !usable_file(path) {
        return Err(ProbeError::NotAFile);
    }
    let mut cmd = std::process::Command::new(path);
    cmd.args(crate::process::PYTHON_FLAGS)
        .args(["-c", VERSION_SCRIPT])
        .env_clear()
        .envs(crate::process::WORKER_ENV)
        .current_dir(std::env::temp_dir());
    let out = crate::process::run_bounded(&mut cmd, timeout, PROBE_BYTES).ok_or(ProbeError::NoAnswer)?;
    Version::parse(&String::from_utf8_lossy(&out)).ok_or(ProbeError::NoAnswer)
}

/// The web build runs no Python.
#[cfg(target_arch = "wasm32")]
pub fn probe(_path: &Path, _timeout: Duration) -> Result<Version, ProbeError> {
    Err(ProbeError::NoAnswer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_order() {
        assert_eq!(Version::parse("3.12.1\n"), Some(Version { major: 3, minor: 12, patch: 1 }));
        for bad in ["", "3.12", "3.12.1.4", "three", "3.x.1", "Python 3.12.1"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version::parse("3.10.14").unwrap() < MIN_VERSION);
        assert!(Version::parse("3.11.0").unwrap() >= MIN_VERSION);
        assert!(Version::parse("4.0.0").unwrap() >= MIN_VERSION);
    }

    #[cfg(unix)]
    #[allow(clippy::disallowed_methods)] // the clock, off wasm
    mod unix {
        use super::super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::time::Instant;

        /// A folder with fake interpreters: shell scripts that answer the probe.
        struct Bin(PathBuf);

        impl Bin {
            fn new(tag: &str) -> Bin {
                let dir = std::env::temp_dir().join(format!("gridcraft-interp-{tag}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&dir);
                std::fs::create_dir_all(&dir).unwrap();
                Bin(dir)
            }
            fn script(&self, name: &str, body: &str) -> PathBuf {
                let p = self.0.join(name);
                std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
                p
            }
        }

        impl Drop for Bin {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn uv_comes_first_and_the_newest_of_its_pythons_first() {
            let root = Bin::new("uv");
            let fake = |dir: &str| {
                let bin = root.0.join(dir).join("bin");
                std::fs::create_dir_all(&bin).unwrap();
                let p = bin.join("python3");
                std::fs::write(&p, "#!/bin/sh\n").unwrap();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
                p
            };
            let recommended = fake("home/.gridcraft/python");
            let venv = fake("venv");
            let old_uv = fake("uv/cpython-3.11.9-macos-aarch64-none");
            let new_uv = fake("uv/cpython-3.13.2-macos-aarch64-none");
            let conda = fake("conda");
            let on_path = fake("path");
            let env = SearchEnv {
                virtual_env: Some(root.0.join("venv").into_os_string()),
                conda_prefix: Some(root.0.join("conda").into_os_string()),
                path: std::env::join_paths([root.0.join("path/bin")]).ok(),
                home: Some(root.0.join("home").into_os_string()),
                uv_python_dir: Some(root.0.join("uv").into_os_string()),
                ..SearchEnv::default()
            };
            assert_eq!(candidates(&env), vec![recommended, venv, new_uv, old_uv, conda, on_path]);
        }

        #[test]
        fn uv_is_found_outside_path() {
            let root = Bin::new("find-uv");
            let bin = root.0.join(".local/bin");
            std::fs::create_dir_all(&bin).unwrap();
            let uv = bin.join("uv");
            std::fs::write(&uv, "").unwrap();
            let env = SearchEnv { home: Some(root.0.clone().into_os_string()), path: Some("relative/bin".into()), ..SearchEnv::default() };
            assert_eq!(find_uv(&env), Some(uv));
        }

        #[cfg(target_os = "macos")]
        #[test]
        fn apples_stub_is_never_asked() {
            let env = SearchEnv { path: Some("/usr/bin".into()), ..SearchEnv::default() };
            assert!(!candidates(&env).iter().any(|p| p == Path::new("/usr/bin/python3")));
            let stub = Path::new("/usr/bin/python3");
            assert_eq!(detect(&SearchEnv::default(), Some(stub), Duration::from_secs(1)), Detection::Stub { path: stub.to_path_buf() });
        }

        fn path_of(dirs: &[&Path]) -> Option<OsString> {
            Some(std::env::join_paths(dirs).unwrap())
        }

        #[test]
        fn relative_path_entries_are_skipped() {
            let good = Bin::new("good");
            let py = good.script("python3", "echo 3.12.4");
            // A relative entry that does lead to an interpreter, from the folder the tests run in.
            let planted = Bin::new("planted");
            planted.script("python3", "echo 3.13.0");
            let depth = std::env::current_dir().unwrap().components().count().saturating_sub(1);
            let relative = PathBuf::from("../".repeat(depth)).join(planted.0.strip_prefix("/").unwrap());
            assert!(relative.join("python3").is_file(), "{}", relative.display());
            // `.`, an empty entry and relative folders come first and are never used.
            let mut path = OsString::from(".:bin::");
            path.push(relative.as_os_str());
            path.push(":");
            path.push(good.0.as_os_str());
            let env = SearchEnv { path: Some(path), ..SearchEnv::default() };
            assert_eq!(candidates(&env), vec![py.clone()]);
            assert_eq!(detect(&env, None, PROBE_TIMEOUT), Detection::Found { path: py, version: Version { major: 3, minor: 12, patch: 4 } });
            // A relative explicit path is refused too.
            assert_eq!(probe(Path::new("python3"), PROBE_TIMEOUT), Err(ProbeError::NotAFile));
            assert_eq!(detect(&SearchEnv::default(), Some(Path::new("bin/python3")), PROBE_TIMEOUT), Detection::NoneFound);
        }

        #[test]
        fn an_old_interpreter_is_reported_and_a_newer_one_wins() {
            let old = Bin::new("old");
            let old_py = old.script("python3", "echo 3.9.18");
            let env = SearchEnv { path: path_of(&[&old.0]), ..SearchEnv::default() };
            assert_eq!(detect(&env, None, PROBE_TIMEOUT), Detection::TooOld { path: old_py, version: Version { major: 3, minor: 9, patch: 18 } });
            let venv = Bin::new("venv");
            std::fs::create_dir_all(venv.0.join("bin")).unwrap();
            let venv_py = venv.script("bin/python", "echo 3.13.0");
            let env = SearchEnv { virtual_env: Some(venv.0.clone().into_os_string()), path: path_of(&[&old.0]), ..SearchEnv::default() };
            assert_eq!(candidates(&env).first(), Some(&venv_py), "the virtual environment comes first");
            assert_eq!(detect(&env, None, PROBE_TIMEOUT), Detection::Found { path: venv_py, version: Version { major: 3, minor: 13, patch: 0 } });
            // An explicit interpreter is the only one tried.
            assert!(matches!(detect(&env, Some(&old.0.join("python3")), PROBE_TIMEOUT), Detection::TooOld { .. }));
        }

        #[test]
        fn a_probe_that_hangs_or_floods_is_killed_and_not_found() {
            let bin = Bin::new("hang");
            let hang = bin.script("python3", "exec sleep 30");
            let t = Instant::now();
            assert_eq!(probe(&hang, Duration::from_millis(300)), Err(ProbeError::NoAnswer));
            assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
            let flood = bin.script("python", "exec yes 3.12.0");
            let t = Instant::now();
            assert_eq!(probe(&flood, Duration::from_secs(5)), Err(ProbeError::NoAnswer));
            assert!(t.elapsed() < Duration::from_secs(4), "a flood is cut at the byte cap, not the deadline: {:?}", t.elapsed());
            let env = SearchEnv { path: path_of(&[&bin.0]), ..SearchEnv::default() };
            assert_eq!(detect(&env, None, Duration::from_millis(300)), Detection::NoneFound);
            let junk = bin.script("python3", "echo not a version");
            assert_eq!(probe(&junk, PROBE_TIMEOUT), Err(ProbeError::NoAnswer));
        }
    }
}
