//! What automation may reach: the folders it may read and write, and the commands it may run.
//!
//! `gridcraft-cli mcp --automation-read-root DIR --automation-write-root DIR --allow-commands LIST`
//! keeps an agent to the files and commands it was given (the same flags as the other crafting
//! apps). While [`Automation`] is in force:
//!
//! - every file [`crate::io::read_file`] opens must lie inside the **read root**, and every file
//!   [`crate::io::write_file`] writes inside the **write root**. Those two functions are the only
//!   way the engine reaches files (open, import, `data.getData`, templates, save, export…), so no
//!   command can go around them. With one root given, the access the other would grant is
//!   denied: with only a read root, nothing can be written. With neither root, files are not
//!   confined (a command list alone confines commands only).
//! - every command a client asks for, and every step of a stored script, must be on the command
//!   list. Commands a command runs itself are part of the allowed command and aren't checked
//!   again. Without a list, every command may run.
//!
//! Paths are made absolute against the working folder, then their links are followed: a file or
//! folder link that leads out of a root is outside it. For a path that doesn't exist yet, its
//! nearest existing folder's links are followed and the rest must be plain names (no `..`). A file
//! is written by name, so for writes the entry itself (its folder's links followed, its own name
//! as given) must be inside the write root as well as what an existing link there leads to. The
//! result is compared with the root by whole folder names (`/work2` is not inside `/work`),
//! without regard to case on Windows, where device names (`CON`, `NUL`, `COM1`…) are refused.
//!
//! A path is checked, then opened: another program that swaps a link in between can still win
//! that race. The roots keep an agent to the files it was given; they don't guard against other
//! programs on the computer.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

use crate::{EngineError, Result};

/// Longest command list accepted (bytes).
pub const MAX_LIST_BYTES: usize = 256 * 1024;

/// Read or write access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
}

/// The commands automation may run: ids and `group.*` prefixes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandAllowlist {
    ids: BTreeSet<String>,
    prefixes: BTreeSet<String>,
}

impl CommandAllowlist {
    /// Parse `list` (ids and `group.*`, separated by commas, spaces or new lines; `#` starts a
    /// comment). Unknown ids, groups that match no command and an empty list are errors, so a
    /// typo never leaves the agent with less, or more, than meant.
    pub fn parse(list: &str) -> std::result::Result<Self, String> {
        if list.len() > MAX_LIST_BYTES {
            return Err(format!("command list too long: {} bytes (at most {MAX_LIST_BYTES})", list.len()));
        }
        let all: Vec<&str> = crate::command_specs().iter().map(|c| c.id).collect();
        let mut out = Self::default();
        for line in list.lines() {
            let line = line.split_once('#').map_or(line, |(before, _)| before);
            for item in line.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()) {
                if let Some(prefix) = item.strip_suffix(".*") {
                    if prefix.is_empty() || prefix.contains('*') {
                        return Err(format!("bad command group `{item}`: use `group.*`"));
                    }
                    let dotted = format!("{prefix}.");
                    if !all.iter().any(|id| id.starts_with(&dotted)) {
                        return Err(format!("`{item}` matches no command"));
                    }
                    out.prefixes.insert(dotted);
                } else if item.contains('*') {
                    return Err(format!("bad command pattern `{item}`: only a trailing `.*` is allowed"));
                } else if all.contains(&item) {
                    out.ids.insert(item.to_string());
                } else {
                    return Err(format!("unknown command `{item}` (gridcraft-cli commands lists them)"));
                }
            }
        }
        if out.ids.is_empty() && out.prefixes.is_empty() {
            return Err("the command list is empty".into());
        }
        Ok(out)
    }

    /// Whether `id` is on the list.
    pub fn allows(&self, id: &str) -> bool {
        self.ids.contains(id) || self.prefixes.iter().any(|p| id.starts_with(p.as_str()))
    }

    /// The listed ids and groups, sorted, for logs.
    pub fn describe(&self) -> Vec<String> {
        self.ids.iter().cloned().chain(self.prefixes.iter().map(|p| format!("{p}*"))).collect()
    }
}

/// The folders and commands automation may reach. Each part left out leaves that kind of
/// access as it was (no roots: files unrestricted; no list: every command).
#[derive(Clone, Debug, Default)]
pub struct Automation {
    read_root: Option<PathBuf>,
    write_root: Option<PathBuf>,
    confined: bool,
    commands: Option<CommandAllowlist>,
}

impl Automation {
    /// Confine files to `read` and `write` (existing folders; with one root given, the access the
    /// other would grant is denied, and with neither, files are not confined) and, with
    /// `commands`, commands to that list. Neither root nor list: `None`.
    pub fn new(read: Option<&Path>, write: Option<&Path>, commands: Option<CommandAllowlist>) -> std::result::Result<Option<Self>, String> {
        let root = |p: Option<&Path>, kind: &str| -> std::result::Result<Option<PathBuf>, String> {
            match p {
                None => Ok(None),
                Some(p) if p.as_os_str().is_empty() => Err(format!("the {kind} root is empty")),
                Some(p) => {
                    let c = std::fs::canonicalize(p).map_err(|e| format!("the {kind} root {}: {e}", p.display()))?;
                    if !c.is_dir() {
                        return Err(format!("the {kind} root {} is not a folder", p.display()));
                    }
                    Ok(Some(c))
                }
            }
        };
        let (read_root, write_root) = (root(read, "read")?, root(write, "write")?);
        let confined = read.is_some() || write.is_some();
        if !confined && commands.is_none() {
            return Ok(None);
        }
        Ok(Some(Self { read_root, write_root, confined, commands }))
    }

    /// The folder files may be read from, resolved. `None` refuses every read only while file
    /// confinement is in force (a write root was given); with neither root, files are not confined.
    pub fn read_root(&self) -> Option<&Path> {
        self.read_root.as_deref()
    }

    /// The folder files may be written to, resolved. `None` refuses every write only while file
    /// confinement is in force (a read root was given); with neither root, files are not confined.
    pub fn write_root(&self) -> Option<&Path> {
        self.write_root.as_deref()
    }

    /// The commands that may run; `None`: every command.
    pub fn commands(&self) -> Option<&CommandAllowlist> {
        self.commands.as_ref()
    }

    /// May `path` be read (or written) under these roots?
    pub fn check_path(&self, path: &str, access: Access) -> std::result::Result<(), String> {
        if !self.confined {
            return Ok(());
        }
        let (root, kind) = match access {
            Access::Read => (self.read_root.as_deref(), "read"),
            Access::Write => (self.write_root.as_deref(), "write"),
        };
        let Some(root) = root else {
            return Err(format!("automation filesystem access is not granted: {kind} root is absent: {path}"));
        };
        let reject = |why: &str| format!("automation path rejected: {why}: {path}");
        if path.is_empty() {
            return Err(reject("path is empty"));
        }
        if cfg!(windows) && Path::new(path).components().any(|c| matches!(c, Component::Normal(n) if is_device_name(&n.to_string_lossy()))) {
            return Err(reject("reserved device names are not allowed"));
        }
        let absolute = std::path::absolute(path).map_err(|e| reject(&format!("cannot resolve ({e})")))?;
        let outside = || reject(&format!("outside the {kind} root {}", plain(root).display()));
        match access {
            Access::Read => {
                let resolved = resolve(&absolute).ok_or_else(|| reject("cannot resolve"))?;
                if !inside(root, &resolved) {
                    return Err(outside());
                }
            }
            Access::Write => {
                // The entry is written by name: its folder (links followed) and name must be
                // inside, and so must whatever an existing entry there leads to. The root itself
                // is not a file to write: a save writes a temporary sibling (`NAME.sctmp`) next
                // to its destination first, and next to the root that sibling is outside it.
                let name = absolute.file_name().ok_or_else(|| reject("not a file name"))?;
                let parent = absolute.parent().ok_or_else(|| reject("no folder"))?;
                let folder = resolve(parent).ok_or_else(|| reject("cannot resolve"))?;
                if !strictly_inside(root, &folder.join(name)) {
                    return Err(outside());
                }
                if std::fs::metadata(&absolute).is_ok_and(|m| m.is_dir()) {
                    return Err(reject("a folder is not a file to write"));
                }
                if std::fs::symlink_metadata(&absolute).is_ok() {
                    let target = std::fs::canonicalize(&absolute).map_err(|_| reject("a link that leads nowhere"))?;
                    if !inside(root, &target) {
                        return Err(outside());
                    }
                }
            }
        }
        Ok(())
    }

    /// May automation run command `id`?
    pub fn check_command(&self, id: &str) -> std::result::Result<(), String> {
        match &self.commands {
            Some(list) if !list.allows(id) => Err(format!("command `{id}` is not allowed: it is not on this session's --allow-commands list")),
            _ => Ok(()),
        }
    }
}

/// `path` made absolute with its links followed. For a path that doesn't exist (yet), its nearest
/// existing ancestor's links are followed and the rest must be plain names.
fn resolve(absolute: &Path) -> Option<PathBuf> {
    if let Ok(c) = std::fs::canonicalize(absolute) {
        return Some(c);
    }
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = absolute;
    loop {
        if std::fs::symlink_metadata(cur).is_ok() {
            // It exists but can't be resolved: a link that leads nowhere.
            let base = std::fs::canonicalize(cur).ok()?;
            return Some(rest.iter().rev().fold(base, |p, n| p.join(n)));
        }
        let name = match cur.components().next_back()? {
            Component::Normal(n) => n.to_os_string(),
            // `..` or `.` below a folder that doesn't exist can't be resolved safely.
            _ => return None,
        };
        rest.push(name);
        cur = cur.parent()?;
    }
}

/// Whether `path` lies in `root` (both resolved), compared by whole components; without regard
/// to case on Windows.
fn inside(root: &Path, path: &Path) -> bool {
    if cfg!(windows) {
        let lower = |p: &Path| p.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect::<Vec<_>>();
        let (r, p) = (lower(root), lower(path));
        p.len() >= r.len() && p.iter().zip(&r).all(|(a, b)| a == b)
    } else {
        path.starts_with(root)
    }
}

/// `path` is inside `root` and is not `root` itself.
fn strictly_inside(root: &Path, path: &Path) -> bool {
    inside(root, path) && path.components().count() > root.components().count()
}

/// `path` without Windows' `\\?\` prefix, for people to read.
fn plain(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if cfg!(windows) => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// Whether `name` is a Windows reserved device name (`NUL`, `con.xlsx`, `COM1`…), with or without
/// an extension: writing "to" one reaches a device, not a file in the folder.
fn is_device_name(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.chars().nth(3).is_some_and(|c| c.is_ascii_digit() || matches!(c, '¹' | '²' | '³')))
}

thread_local! {
    /// This thread's automation; `None`: the process's ([`confine_process`]).
    static SCOPE: RefCell<Option<Arc<Automation>>> = const { RefCell::new(None) };
}

/// Automation in force in every thread outside [`confine`].
static PROCESS: OnceLock<Arc<Automation>> = OnceLock::new();

/// Put `automation` in force in the whole process, for good. `false` when one was already.
pub fn confine_process(automation: Arc<Automation>) -> bool {
    PROCESS.set(automation).is_ok()
}

struct Restore(Option<Arc<Automation>>);

impl Drop for Restore {
    /// Put the thread's previous automation (or none) back in force.
    fn drop(&mut self) {
        let previous = self.0.take();
        let _ = SCOPE.try_with(|s| *s.borrow_mut() = previous);
    }
}

/// Run `f` with `automation` in force on this thread (`None`: as things stand).
pub fn confine<T>(automation: Option<&Arc<Automation>>, f: impl FnOnce() -> T) -> T {
    match automation {
        Some(a) => {
            let _restore = Restore(SCOPE.try_with(|s| s.replace(Some(a.clone()))).ok().flatten());
            f()
        }
        None => f(),
    }
}

/// The automation in force on this thread, if any.
pub fn current() -> Option<Arc<Automation>> {
    SCOPE.try_with(|s| s.borrow().clone()).ok().flatten().or_else(|| PROCESS.get().cloned())
}

/// May `path` be read here? Always, with no automation in force.
pub fn check_read(path: &str) -> Result<()> {
    current().map_or(Ok(()), |a| a.check_path(path, Access::Read).map_err(EngineError::Other))
}

/// May `path` be written here? Always, with no automation in force.
pub fn check_write(path: &str) -> Result<()> {
    current().map_or(Ok(()), |a| a.check_path(path, Access::Write).map_err(EngineError::Other))
}

/// May command `id` run here for a client (or a script step)? Always, with no list in force.
pub fn check_command(id: &str) -> Result<()> {
    current().map_or(Ok(()), |a| a.check_command(id).map_err(EngineError::Other))
}

#[cfg(test)]
mod tests;
