//! The folders automation may read and write (#445, #2172).
//!
//! Adapted from Isaac Levine's PR #480 (20bc9432a773689698190e0b32067a7863e0cf22).
//!
//! `gridcraft-cli mcp --automation-read-root <dir> --automation-write-root <dir>` confines the
//! files an agent's commands reach. [`AutomationRoots`] holds the two folders. While they are in
//! force, every document open and save applies their read or write policy. The checks sit behind
//! [`crate::io::read_file`] and [`crate::io::write_file`], the
//! only functions the engine opens user-named files with (`file.open`, `file.saveAs`,
//! `file.export`, `file.exportPdf`, `insert.picture`, CSV and text import), so a new command that
//! reads or writes through them is confined without further work.
//!
//! - The path is made absolute against the working directory, as the file system reads it. Its
//!   links are followed: for a file that doesn't exist yet, the links of its nearest existing
//!   folder, the rest of the path being plain names (`..` below a missing folder is refused, and so
//!   is a link that leads nowhere).
//! - The result must lie inside the root, compared by whole components (`/work2` isn't inside
//!   `/work`), using their canonical spellings rather than Unicode case folding.
//! - Read and write access are separate: a root given alone grants only its own kind of access.
//!
//! Roots are in force on a thread inside [`confine`], and in the whole process after
//! [`confine_process`] (`gridcraft-cli mcp`), except inside [`unconfined`] (what the person at the
//! keyboard does). Without roots nothing is checked.
//!
//! Native I/O resolves each checked path relative to an open root directory. Opens and atomic
//! saves use capability-relative operations, so replacing a checked component with an escaping
//! symlink cannot redirect them. This is a document-I/O policy, not a process sandbox.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// Read access or write access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// The folders automation may read and write; a root left out grants no access of its kind.
#[derive(Debug)]
pub struct AutomationRoots {
    read: Option<Root>,
    write: Option<Root>,
}

#[derive(Debug)]
struct Root {
    /// The folder made absolute, links followed: what paths are compared with.
    resolved: PathBuf,
    /// The same without Windows' `\\?\` prefix, for people.
    shown: PathBuf,
    /// Retain the authorized directory object instead of reopening its ambient path for I/O.
    #[cfg(not(target_arch = "wasm32"))]
    dir: cap_std::fs::Dir,
}

impl Root {
    /// The folder at `path`, which must exist.
    fn open(path: &Path, access: Access) -> Result<Self, String> {
        Self::open_resolving(path, access, |p| std::fs::canonicalize(p))
    }

    fn open_resolving(path: &Path, access: Access, resolve: impl FnOnce(&Path) -> std::io::Result<PathBuf>) -> Result<Self, String> {
        let kind = match access {
            Access::Read => "read",
            Access::Write => "write",
        };
        // Capture the operator-selected directory before resolving its display/check name.
        // A rename or symlink swap during resolution must not change the retained authority:
        // later I/O always uses this handle, never reopens the resolved ambient pathname.
        #[cfg(not(target_arch = "wasm32"))]
        let dir = cap_std::fs::Dir::open_ambient_dir(path, cap_std::ambient_authority())
            .map_err(|e| format!("cannot open the automation {kind} root `{}`: {e}", path.display()))?;
        let resolved = resolve(path)
            .and_then(|p| if p.is_dir() { Ok(p) } else { Err(std::io::ErrorKind::NotADirectory.into()) })
            .map_err(|e| format!("cannot open the automation {kind} root `{}`: {e}", path.display()))?;
        Ok(Self {
            shown: plain(resolved.clone()),
            resolved,
            #[cfg(not(target_arch = "wasm32"))]
            dir,
        })
    }
}

/// `path` without the `\\?\` prefix `canonicalize` gives on Windows (`\\?\C:\x` becomes `C:\x`,
/// `\\?\UNC\server\share` becomes `\\server\share`); other paths are returned as they are.
fn plain(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let Some(text) = path.to_str() else { return path };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

impl AutomationRoots {
    /// The roots `read` and `write`, which must be existing folders; `None` when neither is given
    /// (nothing is confined then).
    pub fn new(read: Option<&Path>, write: Option<&Path>) -> Result<Option<Arc<Self>>, String> {
        if read.is_none() && write.is_none() {
            return Ok(None);
        }
        let read = read.map(|p| Root::open(p, Access::Read)).transpose()?;
        let write = write.map(|p| Root::open(p, Access::Write)).transpose()?;
        Ok(Some(Arc::new(Self { read, write })))
    }

    /// The read root, absolute with its links followed.
    pub fn read_root(&self) -> Option<&Path> {
        self.read.as_ref().map(|r| r.shown.as_path())
    }

    /// The write root, absolute with its links followed.
    pub fn write_root(&self) -> Option<&Path> {
        self.write.as_ref().map(|r| r.shown.as_path())
    }

    /// May the file or folder at `path` be read (or written)? `Err` says why not.
    pub fn check(&self, path: &str, access: Access) -> Result<(), String> {
        self.checked(path, access).map(|_| ())
    }

    /// Resolve a path to the selected root plus a relative name; native I/O uses that root's
    /// retained handle, so the ambient pathname is never reopened after this check.
    fn checked(&self, path: &str, access: Access) -> Result<(&Root, PathBuf), String> {
        let root = match access {
            Access::Read => self.read.as_ref(),
            Access::Write => self.write.as_ref(),
        };
        let Some(root) = root else {
            return match access {
                Access::Read => Err(format!("automation filesystem access is not granted: read authority is absent: {path}")),
                Access::Write => Err(format!("automation filesystem access is not granted: write authority is absent: {path}")),
            };
        };
        let resolved = resolve(path)?;
        if inside(&resolved, &root.resolved) {
            return resolved
                .strip_prefix(&root.resolved)
                .map(|p| (root, p.to_path_buf()))
                .map_err(|e| format!("automation path rejected: {path}: {e}"));
        }
        let shown = root.shown.display();
        match access {
            Access::Read => Err(format!("automation path rejected: outside the read root {shown}: {path}")),
            Access::Write => Err(format!("automation path rejected: outside the write root {shown}: {path}")),
        }
    }
}

/// Where the roots are in force on a thread ([`confine`], [`unconfined`]).
#[derive(Clone, Debug)]
enum Scope {
    Confined(Arc<AutomationRoots>),
    Unconfined,
}

thread_local! {
    /// This thread's scope; `None`: the process's roots ([`confine_process`]).
    static SCOPE: RefCell<Option<Scope>> = const { RefCell::new(None) };
}

/// The roots in force in every thread outside [`confine`] and [`unconfined`].
static PROCESS: OnceLock<Arc<AutomationRoots>> = OnceLock::new();

/// Put `roots` in force in the whole process, for good. `false` when roots were in force already
/// (they stay as they were).
pub fn confine_process(roots: Arc<AutomationRoots>) -> bool {
    PROCESS.set(roots).is_ok()
}

/// Puts the scope it replaced back when dropped (also when `f` panics).
struct Restore(Option<Scope>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPE.with(|s| *s.borrow_mut() = previous);
    }
}

fn with_scope<T>(scope: Option<Scope>, f: impl FnOnce() -> T) -> T {
    let _restore = Restore(SCOPE.with(|s| s.replace(scope)));
    f()
}

/// Run `f` with `roots` in force on this thread (`None`: as things stand).
pub fn confine<T>(roots: Option<&Arc<AutomationRoots>>, f: impl FnOnce() -> T) -> T {
    match roots {
        Some(r) => with_scope(Some(Scope::Confined(r.clone())), f),
        None => f(),
    }
}

/// Run `f` with no roots in force on this thread: what the person at the keyboard does.
pub fn unconfined<T>(f: impl FnOnce() -> T) -> T {
    with_scope(Some(Scope::Unconfined), f)
}

/// The roots in force on this thread, if any.
pub fn current() -> Option<Arc<AutomationRoots>> {
    match SCOPE.with(|s| s.borrow().clone()) {
        Some(Scope::Confined(r)) => Some(r),
        Some(Scope::Unconfined) => None,
        None => PROCESS.get().cloned(),
    }
}

/// May `path` be read here? Always, without roots in force.
pub fn check_read(path: &str) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path, Access::Read))
}

/// May `path` be written here? Always, without roots in force.
pub fn check_write(path: &str) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path, Access::Write))
}

/// Open a document relative to the retained read root, or use ordinary I/O when unconfined.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_read(path: &str) -> Result<std::fs::File, String> {
    let result = match current() {
        Some(roots) => {
            let (root, relative) = roots.checked(path, Access::Read)?;
            root.dir.open(relative).map(cap_std::fs::File::into_std)
        }
        None => std::fs::File::open(path),
    };
    result.map_err(|e| format!("{path}: {e}"))
}

/// Pin the save's parent directory before creating its temporary file. Both creation and
/// rename are relative to this same handle, even if its ambient path is replaced meanwhile.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_parent(path: &str) -> Result<(cap_std::fs::Dir, OsString), String> {
    let result = match current() {
        Some(roots) => {
            let (root, _) = roots.checked(path, Access::Write)?;
            // Atomic rename replaces the requested leaf, not its symlink target. Resolve only
            // the parent here, after also checking the leaf above, to preserve that behavior.
            let absolute = std::path::absolute(path).map_err(|e| format!("{path}: {e}"))?;
            let name = absolute.file_name().ok_or_else(|| format!("{path}: not a file path"))?;
            let parent = absolute.parent().ok_or_else(|| format!("{path}: no parent directory"))?;
            let parent = std::fs::canonicalize(parent).map_err(|e| format!("{path}: {e}"))?;
            let relative = parent
                .strip_prefix(&root.resolved)
                .map_err(|_| format!("automation path rejected: outside the write root {}: {path}", root.shown.display()))?
                .join(name);
            parent_in(&root.dir, &relative)
        }
        None => {
            let absolute = std::path::absolute(path).map_err(|e| format!("{path}: {e}"))?;
            let name = absolute.file_name().ok_or_else(|| format!("{path}: not a file path"))?.to_os_string();
            let parent = absolute.parent().ok_or_else(|| format!("{path}: no parent directory"))?;
            cap_std::fs::Dir::open_ambient_dir(parent, cap_std::ambient_authority()).map(|dir| (dir, name))
        }
    };
    result.map_err(|e| format!("{path}: {e}"))
}

#[cfg(not(target_arch = "wasm32"))]
fn parent_in(dir: &cap_std::fs::Dir, relative: &Path) -> std::io::Result<(cap_std::fs::Dir, OsString)> {
    let name = relative.file_name().ok_or(std::io::ErrorKind::InvalidInput)?.to_os_string();
    let parent = relative.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    dir.open_dir(parent).map(|dir| (dir, name))
}

/// `path` made absolute with its links followed; for a path that doesn't exist (yet), its nearest
/// existing folder's links followed and the rest appended.
fn resolve(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("automation path rejected: path is empty".into());
    }
    // Checked as given: making `…\NUL` absolute gives the device path `\\.\NUL` on Windows.
    if cfg!(windows) && Path::new(path).components().any(|c| matches!(c, Component::Normal(n) if is_device_name(&n.to_string_lossy()))) {
        return Err(format!("automation path rejected: reserved device names are not allowed: {path}"));
    }
    let absolute = std::path::absolute(path).map_err(|e| format!("automation path rejected: cannot resolve {path}: {e}"))?;
    // The names below the nearest existing folder, last first.
    let mut missing: Vec<OsString> = vec![];
    let mut at = absolute.as_path();
    loop {
        match std::fs::canonicalize(at) {
            Ok(mut found) => {
                found.extend(missing.iter().rev());
                return Ok(found);
            }
            Err(e) => match std::fs::symlink_metadata(at) {
                Ok(m) if m.file_type().is_symlink() => return Err(format!("automation path rejected: a link that leads nowhere: {path}")),
                // There, yet its real path can't be had (no permission): nothing to compare.
                Ok(_) => return Err(format!("automation path rejected: cannot resolve {path}: {e}")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("automation path rejected: cannot resolve {path}: {e}")),
            },
        }
        let parent = at.parent();
        match (at.components().next_back(), parent) {
            (Some(Component::Normal(name)), Some(parent)) => {
                missing.push(name.to_os_string());
                at = parent;
            }
            (Some(Component::ParentDir), _) => {
                return Err(format!("automation path rejected: parent traversal below a folder that doesn't exist: {path}"));
            }
            // Down to the root (or drive) and it doesn't exist either.
            _ => {
                let e = std::io::Error::from(std::io::ErrorKind::NotFound);
                return Err(format!("automation path rejected: cannot resolve {path}: {e}"));
            }
        }
    }
}

/// Is `path` (resolved) `root` or inside it, component by component?
///
/// Compare canonical spellings exactly. Manual Unicode case folding can conflate distinct
/// directories on case-sensitive Windows volumes; a mismatch must fail closed.
fn inside(path: &Path, root: &Path) -> bool {
    path.starts_with(root)
}

/// A Windows device name (`CON`, `NUL`, `COM1`… also with an extension), which names no file.
fn is_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component).trim_end_matches(' ').to_ascii_uppercase();
    let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// A fresh folder holding `in` (read root), `out` (write root) and `outside`.
    fn folders(name: &str) -> (PathBuf, Arc<AutomationRoots>) {
        #[allow(clippy::disallowed_methods)] // Native-only filesystem fixture; retain every run.
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("gc-file-access-{name}-{}-{stamp}", std::process::id()));
        for d in ["in/sub", "out", "outside", "in2"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("in/a.csv"), "a,b").unwrap();
        std::fs::write(base.join("outside/secret.csv"), "x").unwrap();
        let roots = AutomationRoots::new(Some(&base.join("in")), Some(&base.join("out"))).unwrap().unwrap();
        (base, roots)
    }

    fn s(p: PathBuf) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn paths_inside_the_roots_pass_and_others_dont() {
        let (base, roots) = folders("inside");
        assert_eq!(roots.check(&s(base.join("in/a.csv")), Access::Read), Ok(()));
        assert_eq!(roots.check(&s(base.join("in")), Access::Read), Ok(()), "the root itself");
        assert_eq!(roots.check(&s(base.join("in/sub/new.csv")), Access::Read), Ok(()), "missing files are checked by their folder");
        assert_eq!(roots.check(&s(base.join("out/new/deeper/x.xlsx")), Access::Write), Ok(()), "and by their nearest folder");
        let e = roots.check(&s(base.join("outside/secret.csv")), Access::Read).unwrap_err();
        assert!(e.starts_with("automation path rejected: outside the read root"), "{e}");
        // Read and write access are separate.
        let e = roots.check(&s(base.join("in/a.csv")), Access::Write).unwrap_err();
        assert!(e.contains("outside the write root"), "{e}");
        assert!(roots.check(&s(base.join("out/x.xlsx")), Access::Read).unwrap_err().contains("outside the read root"));
    }

    #[test]
    fn both_roots_may_be_the_same_folder() {
        let (base, _) = folders("same");
        let both = AutomationRoots::new(Some(&base.join("in")), Some(&base.join("in"))).unwrap().unwrap();
        assert_eq!(both.check(&s(base.join("in/a.csv")), Access::Read), Ok(()));
        assert_eq!(both.check(&s(base.join("in/new.xlsx")), Access::Write), Ok(()));
        assert!(both.check(&s(base.join("out/new.xlsx")), Access::Write).is_err());
    }

    /// A nested write root permits output without granting writes to the surrounding inputs.
    #[test]
    fn write_root_may_be_a_subfolder_of_the_read_root() {
        let (base, _) = folders("nested");
        let read = base.join("in");
        let write = read.join("_artcraft");
        std::fs::create_dir_all(&write).unwrap();
        let roots = AutomationRoots::new(Some(&read), Some(&write)).unwrap().unwrap();
        for path in [read.join("a.csv"), write.join("new.xlsx"), write.join("deeper/new.xlsx")] {
            assert_eq!(roots.check(&s(path), Access::Read), Ok(()));
        }
        for path in [write.clone(), write.join("new.xlsx"), write.join("deeper/new.xlsx")] {
            assert_eq!(roots.check(&s(path), Access::Write), Ok(()));
        }
        for path in [read.join("a.csv"), read.join("new.xlsx"), read.join("_artcraft2/new.xlsx"), write.join("../new.xlsx")] {
            assert!(roots.check(&s(path), Access::Write).unwrap_err().contains("outside the write root"));
        }
        assert!(roots.check(&s(base.join("outside/secret.csv")), Access::Read).is_err());
        assert_eq!(std::fs::read_to_string(read.join("a.csv")).unwrap(), "a,b");
    }

    /// Existing case aliases use the filesystem's canonical spelling on macOS.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_existing_case_aliases_follow_the_filesystem() {
        let (base, roots) = folders("macos-case");
        let original = base.join("in/MixedCase");
        std::fs::create_dir_all(&original).unwrap();
        std::fs::write(original.join("Input.csv"), "synthetic,7").unwrap();
        let alias = base.join("IN/mixedcase/input.CSV");
        if alias.exists() {
            assert_eq!(roots.check(&s(alias), Access::Read), Ok(()));
        } else {
            assert!(roots.check(&s(alias), Access::Read).is_err());
        }
        assert_eq!(std::fs::read_to_string(original.join("Input.csv")).unwrap(), "synthetic,7");
    }

    #[test]
    fn a_sibling_with_the_same_prefix_is_outside() {
        let (base, roots) = folders("prefix");
        std::fs::write(base.join("in2/b.csv"), "b").unwrap();
        assert!(roots.check(&s(base.join("in2/b.csv")), Access::Read).is_err(), "in2 is not inside in");
        assert!(roots.check(&format!("{}2/c.csv", s(base.join("in"))), Access::Read).is_err());
    }

    #[test]
    fn parent_traversal_is_resolved_or_refused() {
        let (base, roots) = folders("dotdot");
        let sep = std::path::MAIN_SEPARATOR;
        let up = format!("{}{sep}..{sep}outside{sep}secret.csv", s(base.join("in")));
        assert!(roots.check(&up, Access::Read).unwrap_err().contains("outside the read root"), "`..` out of the root");
        let back = format!("{}{sep}..{sep}a.csv", s(base.join("in/sub")));
        assert_eq!(roots.check(&back, Access::Read), Ok(()), "`..` that stays inside");
        let missing = format!("{}{sep}nope{sep}..{sep}..{sep}outside{sep}x.csv", s(base.join("out")));
        let e = roots.check(&missing, Access::Write).unwrap_err();
        // Windows resolves `..` by name first (so does opening the file there): then it's outside.
        assert!(e.contains("parent traversal below a folder that doesn't exist") || e.contains("outside the write root"), "{e}");
    }

    #[test]
    fn relative_and_empty_paths() {
        let (_base, roots) = folders("relative");
        // The tests' working directory is the crate's folder: not inside the roots.
        assert!(roots.check("a.csv", Access::Read).unwrap_err().contains("outside the read root"));
        assert!(roots.check("", Access::Read).unwrap_err().contains("path is empty"));
    }

    #[test]
    fn a_missing_root_is_an_error_and_none_is_no_confinement() {
        let (base, _) = folders("missing");
        let e = AutomationRoots::new(Some(&base.join("nope")), None).unwrap_err();
        assert!(e.starts_with("cannot open the automation read root"), "{e}");
        let e = AutomationRoots::new(None, Some(&base.join("in/a.csv"))).unwrap_err();
        assert!(e.starts_with("cannot open the automation write root"), "{e}");
        assert!(AutomationRoots::new(Some(Path::new("")), None).unwrap_err().starts_with("cannot open the automation read root ``"));
        assert!(AutomationRoots::new(None, None).unwrap().is_none());
    }

    #[test]
    fn a_root_left_out_grants_none_of_its_access() {
        let (base, _) = folders("one-root");
        let read_only = AutomationRoots::new(Some(&base.join("in")), None).unwrap().unwrap();
        assert_eq!(read_only.check(&s(base.join("in/a.csv")), Access::Read), Ok(()));
        let e = read_only.check(&s(base.join("in/x.xlsx")), Access::Write).unwrap_err();
        assert!(e.contains("write authority is absent"), "{e}");
        let write_only = AutomationRoots::new(None, Some(&base.join("out"))).unwrap().unwrap();
        assert_eq!(write_only.check(&s(base.join("out/x.xlsx")), Access::Write), Ok(()));
        let e = write_only.check(&s(base.join("out/x.xlsx")), Access::Read).unwrap_err();
        assert!(e.contains("read authority is absent"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn links_are_followed_before_comparing() {
        use std::os::unix::fs::symlink;
        let (base, roots) = folders("links");
        symlink(base.join("outside/secret.csv"), base.join("in/leak.csv")).unwrap();
        assert!(roots.check(&s(base.join("in/leak.csv")), Access::Read).unwrap_err().contains("outside the read root"), "a file link out");
        symlink(base.join("outside"), base.join("out/away")).unwrap();
        let e = roots.check(&s(base.join("out/away/new.xlsx")), Access::Write).unwrap_err();
        assert!(e.contains("outside the write root"), "a folder link out: {e}");
        symlink(base.join("outside/gone.csv"), base.join("out/dangling.xlsx")).unwrap();
        let e = roots.check(&s(base.join("out/dangling.xlsx")), Access::Write).unwrap_err();
        assert!(e.contains("a link that leads nowhere"), "writing would create the file outside: {e}");
        // A link to a folder inside the root stays inside.
        symlink(base.join("in/sub"), base.join("in/alias")).unwrap();
        assert_eq!(roots.check(&s(base.join("in/alias/x.csv")), Access::Read), Ok(()));
        // A root given through a link is the folder it leads to.
        symlink(base.join("in"), base.join("in-link")).unwrap();
        let linked = AutomationRoots::new(Some(&base.join("in-link")), None).unwrap().unwrap();
        assert_eq!(linked.check(&s(base.join("in/a.csv")), Access::Read), Ok(()));
    }

    #[cfg(windows)]
    #[test]
    fn windows_resolves_case_and_refuses_devices() {
        let (base, roots) = folders("case");
        assert_eq!(roots.check(&s(base.join("IN/A.CSV")), Access::Read), Ok(()));
        let e = roots.check(&s(base.join("out/NUL")), Access::Write).unwrap_err();
        assert!(e.contains("reserved device names"), "{e}");
        assert!(roots.check(&s(base.join("out/com1.txt")), Access::Write).is_err());
        assert_eq!(roots.check(&s(base.join("out/console.txt")), Access::Write), Ok(()));
    }

    #[test]
    fn scopes_nest_and_restore() {
        let (base, roots) = folders("scopes");
        let outside = s(base.join("outside/secret.csv"));
        assert_eq!(check_read(&outside), Ok(()), "nothing is confined by default");
        confine(Some(&roots), || {
            assert!(check_read(&outside).is_err());
            assert_eq!(unconfined(|| check_read(&outside)), Ok(()));
            assert!(check_read(&outside).is_err(), "restored after unconfined");
            let o = outside.clone();
            assert!(std::thread::spawn(move || check_read(&o).is_ok()).join().unwrap(), "another thread is not in this thread's scope");
        });
        assert_eq!(check_read(&outside), Ok(()), "restored after confine");
        let r = std::panic::catch_unwind(|| confine(Some(&roots), || panic!("boom")));
        assert!(r.is_err());
        assert_eq!(check_read(&outside), Ok(()), "restored after a panic");
        assert_eq!(confine(None, || check_read(&outside)), Ok(()));
    }

    #[test]
    fn canonical_components_are_not_case_folded() {
        assert!(inside(Path::new("scope/Case/child"), Path::new("scope/Case")));
        assert!(!inside(Path::new("scope/case/child"), Path::new("scope/Case")));
        assert!(!inside(Path::new("scope/Case2/child"), Path::new("scope/Case")));
    }

    #[test]
    fn device_names() {
        for d in ["CON", "nul", "Com1", "LPT9.txt", "aux.csv", "NUL "] {
            assert!(is_device_name(d), "{d}");
        }
        for f in ["console", "com10", "COM0", "lpt", "a.csv", "nul-x"] {
            assert!(!is_device_name(f), "{f}");
        }
    }

    /// A root replaced during name resolution must never replace the captured authority.
    #[cfg(unix)]
    #[test]
    fn root_handle_is_captured_before_resolving_its_name() {
        for access in [Access::Read, Access::Write] {
            for swap_before_resolution in [false, true] {
                let (base, _) = folders("root-setup-swap");
                let requested = base.join("in");
                let retained = base.join("retained-root");
                std::fs::write(requested.join("secret.csv"), "authorized sentinel").unwrap();
                let root = Root::open_resolving(&requested, access, |path| {
                    let swap = || {
                        std::fs::rename(&requested, &retained).unwrap();
                        std::os::unix::fs::symlink(base.join("outside"), &requested).unwrap();
                    };
                    if swap_before_resolution {
                        swap();
                    }
                    let resolved = std::fs::canonicalize(path);
                    if !swap_before_resolution {
                        swap();
                    }
                    resolved
                })
                .unwrap();
                assert_eq!(
                    root.dir.read("secret.csv").unwrap(),
                    b"authorized sentinel",
                    "{access:?}, swap before resolution: {swap_before_resolution}"
                );
                root.dir.write("new.csv", "authorized output").unwrap();
                assert_eq!(std::fs::read(retained.join("new.csv")).unwrap(), b"authorized output");
                assert!(!base.join("outside/new.csv").exists());
                assert_eq!(std::fs::read(base.join("outside/secret.csv")).unwrap(), b"x");
            }
        }
    }

    /// cap-std's Windows handle prevents replacing the root while its name is resolved.
    #[cfg(windows)]
    #[test]
    fn root_handle_prevents_replacement_during_name_resolution() {
        let (base, _) = folders("root-setup-lock");
        let requested = base.join("in");
        let root = Root::open_resolving(&requested, Access::Read, |path| {
            assert!(std::fs::rename(&requested, base.join("retained-root")).is_err());
            std::fs::canonicalize(path)
        })
        .unwrap();
        assert_eq!(root.dir.read("a.csv").unwrap(), b"a,b");
        assert!(requested.is_dir());
    }

    /// Replacing a checked path component must not redirect the later capability-relative open.
    #[cfg(unix)]
    #[test]
    fn checked_read_and_save_parent_reject_a_subsequent_escaping_symlink() {
        let (base, roots) = folders("checked-swap");
        std::fs::write(base.join("in/sub/secret.csv"), "inside").unwrap();
        std::fs::create_dir_all(base.join("out/sub")).unwrap();
        let input = s(base.join("in/sub/secret.csv"));
        let output = s(base.join("out/sub/book.xlsx"));
        let (read, relative_read) = roots.checked(&input, Access::Read).unwrap();
        let (write, relative_write) = roots.checked(&output, Access::Write).unwrap();
        for kind in ["in", "out"] {
            std::fs::rename(base.join(kind).join("sub"), base.join(kind).join("retained-sub")).unwrap();
            std::os::unix::fs::symlink(base.join("outside"), base.join(kind).join("sub")).unwrap();
        }
        assert!(read.dir.open(relative_read).is_err());
        assert!(parent_in(&write.dir, &relative_write).is_err());
        assert_eq!(std::fs::read_to_string(base.join("outside/secret.csv")).unwrap(), "x");
        assert!(!base.join("outside/book.xlsx").exists());
        assert_eq!(std::fs::read_to_string(base.join("in/retained-sub/secret.csv")).unwrap(), "inside");
    }

    /// An outside symlink into the write root cannot authorize replacing that outside entry.
    #[cfg(unix)]
    #[test]
    fn write_parent_rejects_an_outside_leaf_alias_into_the_root() {
        let (base, roots) = folders("outside-alias");
        let target = base.join("out/book.xlsx");
        std::fs::write(&target, "inside sentinel").unwrap();
        let alias = base.join("outside/alias.xlsx");
        std::os::unix::fs::symlink(&target, &alias).unwrap();
        let result = confine(Some(&roots), || crate::io::write_file(alias.to_str().unwrap(), b"must not be saved"));
        assert!(result.is_err());
        assert!(std::fs::symlink_metadata(&alias).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "inside sentinel");
    }

    /// Ordinary inside-pointing links remain usable after resolving them to root-relative names.
    #[cfg(unix)]
    #[test]
    fn native_read_accepts_an_inside_pointing_symlink() {
        let (base, roots) = folders("inside-alias");
        let alias = base.join("in/sub/link.csv");
        std::os::unix::fs::symlink(base.join("in/a.csv"), &alias).unwrap();
        let bytes = confine(Some(&roots), || crate::io::read_file(alias.to_str().unwrap())).unwrap();
        assert_eq!(bytes, b"a,b");
    }
}
