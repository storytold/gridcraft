//! The real worker: the user's interpreter running `shim.py`.
//!
//! Each worker gets a private directory (owner-only on Unix) holding a copy of the shim, written
//! read-only from the copy compiled into the app, and an empty working directory. The interpreter
//! starts as `<python> -I -S -E <shim> <package folders...>` (isolated mode, no `site`, no
//! `PYTHON*` variables) with an empty environment apart from `OPENBLAS_NUM_THREADS=1`. [`Launch`]
//! is the seam where a wrapper set with [`ProcessTransport::with_wrap`] can change the command;
//! [`sandboxed`] is the wrapper that cells run under.

use std::any::Any;
use std::ffi::OsString;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gridcraft_sandbox::{Canary, Policy};

use crate::describe::Interpreter;
use crate::run::{MemoryProbe, Pipes, Transport};

/// The shim's source, written to each worker's private directory.
pub const SHIM: &str = include_str!("shim.py");
/// The interpreter flags: isolated mode (no user site, no script directory on the path, no
/// `PYTHON*` variables), and no `site` module (no `.pth` file runs).
pub const PYTHON_FLAGS: [&str; 3] = ["-I", "-S", "-E"];
/// The only environment variable the worker gets: numpy's BLAS runs on one thread.
pub const WORKER_ENV: [(&str, &str); 1] = [("OPENBLAS_NUM_THREADS", "1")];
/// Bytes of the worker's standard error kept (its end).
const STDERR_TAIL: usize = 8 * 1024;

/// How long the last words of a child wait for the thread reading them. A child that has exited has
/// closed the pipe, so this runs out only when something else still holds it open.
const LAST_WORDS_WAIT: Duration = Duration::from_millis(200);

/// How to start one worker: everything the command uses. A wrapper set with
/// [`ProcessTransport::with_wrap`] can replace it with another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The whole environment (nothing is inherited).
    pub env: Vec<(OsString, OsString)>,
    pub cwd: PathBuf,
    /// The worker's private folder: the shim and `cwd` are inside it.
    pub folder: PathBuf,
}

impl Launch {
    /// The interpreter running `shim` in `cwd`, inside `folder`, given its package folders.
    pub fn python(python: &Path, shim: &Path, cwd: &Path, folder: &Path, site_dirs: &[PathBuf]) -> Launch {
        let mut args: Vec<OsString> = PYTHON_FLAGS.iter().map(OsString::from).collect();
        args.push(shim.as_os_str().to_owned());
        args.extend(site_dirs.iter().map(|d| d.as_os_str().to_owned()));
        Launch {
            program: python.to_path_buf(),
            args,
            env: WORKER_ENV.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect(),
            cwd: cwd.to_path_buf(),
            folder: folder.to_path_buf(),
        }
    }

    /// The command, with piped standard streams and only [`Launch::env`] in its environment.
    pub fn command(&self) -> Command {
        let mut c = Command::new(&self.program);
        c.args(&self.args).env_clear().envs(self.env.iter().map(|(k, v)| (k, v))).current_dir(&self.cwd);
        c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }
}

/// What a wrapper makes of a launch: the launch to run, and what must stay alive while it runs.
pub struct Wrapped {
    pub launch: Launch,
    pub guard: Option<Box<dyn Any + Send>>,
}

impl From<Launch> for Wrapped {
    fn from(launch: Launch) -> Wrapped {
        Wrapped { launch, guard: None }
    }
}

/// Turns the plain launch into the one to run.
pub type Wrap = Arc<dyn Fn(Launch) -> Result<Wrapped, String> + Send + Sync>;

/// The locale and time zone, which the launcher hands on to the program; the rest of the app's
/// environment doesn't reach the launcher at all.
const KEPT_ENV: [&str; 4] = ["LANG", "LC_ALL", "LC_CTYPE", "TZ"];

/// The wrapper cells run under: the launch goes through the confined launcher, which may read the
/// interpreter's files and the worker's own folder, and execute only the interpreter. `launcher` is
/// the executable that starts it, by default this one; it must hand its arguments to
/// `gridcraft_sandbox::handle_args` first.
pub fn sandboxed(interpreter: &Interpreter, launcher: Option<PathBuf>) -> Wrap {
    let read = interpreter.read_paths.clone();
    Arc::new(move |launch: Launch| {
        let exe = match &launcher {
            Some(exe) => exe.clone(),
            None => std::env::current_exe().map_err(|e| format!("couldn't find GridCraft's own program: {e}"))?,
        };
        let mut policy = Policy::new().with_system_libraries().read(&launch.folder).exec(&launch.program);
        for path in &read {
            policy = policy.read(path);
        }
        let canary = Canary::create().map_err(|e| e.to_string())?;
        let cmd = gridcraft_sandbox::launch_command_with(&exe, &policy, &canary, &launch.program, &launch.args).map_err(|e| e.to_string())?;
        let env = KEPT_ENV.iter().filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v))).chain(launch.env.iter().cloned()).collect();
        let wrapped = Launch {
            program: cmd.get_program().into(),
            args: cmd.get_args().map(OsString::from).collect(),
            env,
            cwd: launch.cwd,
            folder: launch.folder,
        };
        Ok(Wrapped { launch: wrapped, guard: Some(Box::new(canary)) })
    })
}

/// Starts the interpreter at a path as the worker.
pub struct ProcessTransport {
    python: PathBuf,
    site_dirs: Vec<PathBuf>,
    wrap: Option<Wrap>,
    running: Option<Running>,
}

struct Running {
    child: Child,
    dir: PrivateDir,
    stderr: ErrorTail,
    _guard: Option<Box<dyn Any + Send>>,
}

impl ProcessTransport {
    /// `python` is an absolute path to an interpreter, as `interp::detect` found it.
    pub fn new(python: PathBuf) -> ProcessTransport {
        ProcessTransport { python, site_dirs: Vec::new(), wrap: None, running: None }
    }

    /// A described interpreter, confined: what cells run in.
    pub fn confined(interpreter: &Interpreter, launcher: Option<PathBuf>) -> ProcessTransport {
        ProcessTransport {
            python: interpreter.run_as.clone(),
            site_dirs: interpreter.site_dirs.clone(),
            wrap: Some(sandboxed(interpreter, launcher)),
            running: None,
        }
    }

    /// The folders of installed packages the worker is given.
    pub fn with_site_dirs(mut self, site_dirs: Vec<PathBuf>) -> ProcessTransport {
        self.site_dirs = site_dirs;
        self
    }

    /// Every launch goes through `wrap` before it runs.
    pub fn with_wrap(mut self, wrap: Wrap) -> ProcessTransport {
        self.wrap = Some(wrap);
        self
    }

    pub fn python(&self) -> &Path {
        &self.python
    }

    /// The running worker's private folder.
    pub fn folder(&self) -> Option<&Path> {
        self.running.as_ref().map(|r| r.dir.root.as_path())
    }
}

impl Transport for ProcessTransport {
    fn start(&mut self) -> Result<Pipes, String> {
        self.kill();
        if !self.python.is_absolute() {
            return Err("the Python interpreter's path isn't absolute".into());
        }
        let dir = PrivateDir::create().map_err(|e| format!("couldn't make a folder for Python: {e}"))?;
        let plain = Launch::python(&self.python, &dir.shim, &dir.work, &dir.root, &self.site_dirs);
        let Wrapped { launch, guard } = match &self.wrap {
            Some(wrap) => wrap(plain)?,
            None => plain.into(),
        };
        let mut child = launch.command().spawn().map_err(|e| format!("couldn't start {}: {e}", self.python.display()))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            reap(child, dir);
            return Err("Python started without its pipes".into());
        };
        // Without the drain the worker could fill its error pipe and stall; it would then hit its
        // deadline, which is still safe.
        let stderr = ErrorTail::start(child.stderr.take());
        let memory = memory_probe(child.id());
        self.running = Some(Running { child, dir, stderr, _guard: guard });
        Ok(Pipes { stdin: Box::new(stdin), stdout: Box::new(stdout), memory })
    }

    fn kill(&mut self) {
        if let Some(mut r) = self.running.take() {
            let _ = r.child.kill();
            reap(r.child, r.dir);
        }
    }

    fn last_words(&mut self) -> String {
        self.running.as_ref().map_or_else(String::new, |r| r.stderr.words())
    }
}

impl Drop for ProcessTransport {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Removes a killed child's folder now, and waits for the child on a thread so the caller
/// doesn't. (A folder still in use, as on Windows while the child exits, is removed again after
/// the wait.)
fn reap(mut child: Child, dir: PrivateDir) {
    dir.remove();
    let spawned = std::thread::Builder::new().name("python-reap".into()).spawn(move || {
        let _ = child.wait();
        drop(dir);
    });
    // Without a thread the child is left to the system to reap; the folder was removed above.
    drop(spawned);
}

/// The end of a child's standard error, kept by a thread of its own.
struct ErrorTail {
    bytes: Arc<Mutex<Vec<u8>>>,
    /// Closes when the thread has read to the end of the pipe.
    read_all: Option<Receiver<()>>,
}

impl ErrorTail {
    fn start(input: Option<impl Read + Send + 'static>) -> ErrorTail {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let read_all = input.and_then(|input| {
            let tail = bytes.clone();
            let (done, read_all) = std::sync::mpsc::channel();
            let drain = move || {
                drain_tail(input, &tail);
                drop(done);
            };
            std::thread::Builder::new().name("python-stderr".into()).spawn(drain).ok().map(|_| read_all)
        });
        ErrorTail { bytes, read_all }
    }

    /// What it has read. The pipe closes when the child exits, at the same moment as its standard
    /// output, so whoever saw that output end may get here before the thread has the last line, and
    /// the launcher's refusal is that line. So this waits for the thread, a little.
    fn words(&self) -> String {
        if let Some(read_all) = &self.read_all {
            let _ = read_all.recv_timeout(LAST_WORDS_WAIT);
        }
        let bytes = self.bytes.lock().map(|b| b.clone()).unwrap_or_default();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// Keeps the last [`STDERR_TAIL`] bytes of `input`.
fn drain_tail(mut input: impl Read, tail: &Mutex<Vec<u8>>) {
    let mut chunk = [0u8; 4096];
    while let Ok(n) = input.read(&mut chunk) {
        if n == 0 {
            return;
        }
        let Ok(mut t) = tail.lock() else { return };
        t.extend_from_slice(chunk.get(..n).unwrap_or_default());
        let over = t.len().saturating_sub(STDERR_TAIL);
        t.drain(..over);
    }
}

/// A fresh folder only this user can read: the shim (read-only) and an empty working directory.
/// Removed when dropped.
pub struct PrivateDir {
    pub root: PathBuf,
    pub shim: PathBuf,
    pub work: PathBuf,
}

static DIRS: AtomicU64 = AtomicU64::new(0);

impl PrivateDir {
    pub fn create() -> std::io::Result<PrivateDir> {
        let base = std::env::temp_dir();
        let salt = {
            use std::hash::{BuildHasher, Hasher};
            std::collections::hash_map::RandomState::new().build_hasher().finish()
        };
        let mut last = None;
        for _ in 0..16 {
            let n = DIRS.fetch_add(1, Ordering::Relaxed);
            let root = base.join(format!("gridcraft-python-{}-{n}-{salt:016x}", std::process::id()));
            // Creating, not opening: a name that exists (or a planted link) is refused.
            match make_private_dir(&root) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    last = Some(e);
                    continue;
                }
                Err(e) => return Err(e),
            }
            let dir = PrivateDir { shim: root.join("shim.py"), work: root.join("work"), root };
            make_private_dir(&dir.work)?;
            write_private_file(&dir.shim, SHIM.as_bytes())?;
            return Ok(dir);
        }
        Err(last.unwrap_or_else(|| std::io::Error::other("no free folder name")))
    }
}

impl PrivateDir {
    fn remove(&self) {
        // The shim is read-only; on Windows that stops its removal.
        #[cfg(not(unix))]
        if let Ok(meta) = std::fs::metadata(&self.shim) {
            let mut p = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            p.set_readonly(false);
            let _ = std::fs::set_permissions(&self.shim, p);
        }
        if self.root.exists() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

impl Drop for PrivateDir {
    fn drop(&mut self) {
        self.remove();
    }
}

#[cfg(unix)]
fn make_private_dir(path: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(path)
}

#[cfg(not(unix))]
fn make_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir(path)
}

fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    let mut p = f.metadata()?.permissions();
    #[cfg(unix)]
    std::os::unix::fs::PermissionsExt::set_mode(&mut p, 0o400);
    #[cfg(not(unix))]
    p.set_readonly(true);
    f.set_permissions(p)
}

/// How this system reads a process's memory.
fn memory_probe(pid: u32) -> Option<Box<dyn MemoryProbe>> {
    #[cfg(target_os = "linux")]
    {
        Some(Box::new(ProcStatus { pid }))
    }
    #[cfg(target_os = "macos")]
    {
        Some(Box::new(PsRss { pid }))
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        None
    }
}

/// Linux: `VmRSS` from `/proc/<pid>/status`, in kB. (`statm` counts pages, and the page size
/// can't be read without libc; `status` gives the same number in kB.)
#[cfg(target_os = "linux")]
struct ProcStatus {
    pid: u32,
}

#[cfg(target_os = "linux")]
impl MemoryProbe for ProcStatus {
    fn rss_bytes(&mut self) -> Option<u64> {
        let mut text = String::new();
        std::fs::File::open(format!("/proc/{}/status", self.pid)).ok()?.take(16 * 1024).read_to_string(&mut text).ok()?;
        parse_vm_rss(&text)
    }
    fn interval(&self) -> Duration {
        Duration::from_millis(100)
    }
}

/// The `VmRSS:` line of `/proc/<pid>/status`, in bytes.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.trim_start_matches("VmRSS:").trim().trim_end_matches("kB").trim().parse().ok()?;
    kb.checked_mul(1024)
}

/// macOS: `ps -o rss= -p <pid>` (kB), at most every 500 ms, each with its own short deadline.
#[cfg(target_os = "macos")]
struct PsRss {
    pid: u32,
}

#[cfg(target_os = "macos")]
impl MemoryProbe for PsRss {
    fn rss_bytes(&mut self) -> Option<u64> {
        let out = run_bounded(Command::new("/bin/ps").args(["-o", "rss=", "-p", &self.pid.to_string()]).env_clear(), Duration::from_millis(300), 64)?;
        let kb: u64 = String::from_utf8_lossy(&out).trim().parse().ok()?;
        kb.checked_mul(1024)
    }
    fn interval(&self) -> Duration {
        Duration::from_millis(500)
    }
}

/// What a command printed, when it finishes within `deadline` and prints at most `max` bytes.
/// Otherwise it is killed and the answer is `None`.
pub(crate) fn run_bounded(cmd: &mut Command, deadline: Duration, max: usize) -> Option<Vec<u8>> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::Builder::new().name("python-probe".into()).spawn(move || {
        let mut buf = Vec::new();
        // One byte over the limit tells a flood from a full answer.
        let r = stdout.take(max as u64 + 1).read_to_end(&mut buf);
        let _ = tx.send(r.ok().map(|_| buf));
    });
    if reader.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let out = match rx.recv_timeout(deadline) {
        Ok(Some(buf)) if buf.len() <= max => Some(buf),
        _ => None,
    };
    if out.is_none() {
        let _ = child.kill();
    }
    // The output ended, so the process is exiting; it is reaped on a thread in case it lingers.
    let _ = std::thread::Builder::new().name("python-probe-reap".into()).spawn(move || {
        let _ = child.kill();
        let _ = child.wait();
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_is_isolated_with_an_empty_environment() {
        let l = Launch::python(
            Path::new("/usr/bin/python3"),
            Path::new("/tmp/x/shim.py"),
            Path::new("/tmp/x/work"),
            Path::new("/tmp/x"),
            &[PathBuf::from("/site")],
        );
        assert_eq!(l.args, ["-I", "-S", "-E", "/tmp/x/shim.py", "/site"].map(OsString::from));
        assert_eq!(l.env, vec![(OsString::from("OPENBLAS_NUM_THREADS"), OsString::from("1"))]);
        let c = l.command();
        let envs: Vec<_> = c.get_envs().collect();
        assert_eq!(envs.len(), 1, "only the one variable is set: {envs:?}");
        assert_eq!(c.get_current_dir(), Some(Path::new("/tmp/x/work")));
    }

    #[test]
    fn private_dir_holds_a_read_only_shim_and_an_empty_work_folder() {
        let dir = PrivateDir::create().unwrap();
        assert_eq!(std::fs::read_to_string(&dir.shim).unwrap(), SHIM);
        assert_eq!(std::fs::read_dir(&dir.work).unwrap().count(), 0);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&dir.root).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(std::fs::metadata(&dir.work).unwrap().permissions().mode() & 0o777, 0o700);
            assert_eq!(std::fs::metadata(&dir.shim).unwrap().permissions().mode() & 0o777, 0o400);
        }
        let root = dir.root.clone();
        let other = PrivateDir::create().unwrap();
        assert_ne!(other.root, root);
        drop(dir);
        assert!(!root.exists(), "the folder goes with the worker");
    }

    #[test]
    fn vm_rss_parses_in_bytes() {
        assert_eq!(parse_vm_rss("Name:\tpython3\nVmRSS:\t   12345 kB\nVmData: 1 kB\n"), Some(12345 * 1024));
        assert_eq!(parse_vm_rss("VmRSS: lots kB"), None);
        assert_eq!(parse_vm_rss(""), None);
        assert_eq!(parse_vm_rss(&format!("VmRSS: {} kB", u64::MAX)), None);
    }

    /// The folder goes when the worker is killed, without waiting for the child to be reaped.
    #[cfg(unix)]
    #[test]
    fn a_killed_worker_leaves_no_folder() {
        let mut t = ProcessTransport::new(PathBuf::from("/bin/sleep"));
        // `sleep -I -S -E shim` fails at once; the folder is made all the same.
        let pipes = t.start().unwrap();
        let folder = t.folder().unwrap().to_path_buf();
        assert!(folder.join("shim.py").is_file());
        t.kill();
        assert!(!folder.exists());
        drop(pipes);
        let wrapped = ProcessTransport::new(PathBuf::from("/bin/sleep")).with_wrap(Arc::new(|l: Launch| {
            assert_eq!(l.args.len(), 4);
            Ok(Launch { args: vec!["30".into()], ..l }.into())
        }));
        let mut t = wrapped;
        let _pipes = t.start().unwrap();
        let folder = t.folder().unwrap().to_path_buf();
        drop(t);
        assert!(!folder.exists(), "dropping the transport kills the worker and removes its folder");
    }

    #[test]
    fn a_relative_interpreter_path_is_refused() {
        let mut t = ProcessTransport::new(PathBuf::from("python3"));
        assert!(t.start().is_err());
    }

    /// Bytes a little later, as from a pipe whose reading thread hasn't run yet.
    struct Late(Option<&'static [u8]>);

    impl Read for Late {
        fn read(&mut self, into: &mut [u8]) -> std::io::Result<usize> {
            std::thread::sleep(Duration::from_millis(20));
            let Some(bytes) = self.0.take() else { return Ok(0) };
            into[..bytes.len()].copy_from_slice(bytes);
            Ok(bytes.len())
        }
    }

    #[test]
    fn the_last_words_wait_for_the_line_still_on_its_way() {
        let tail = ErrorTail::start(Some(Late(Some(b"gridcraft-sandbox: refused: no Landlock\n"))));
        assert_eq!(tail.words(), "gridcraft-sandbox: refused: no Landlock\n");
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // the clock: this module exists only off wasm
    fn the_last_words_dont_wait_long_for_a_pipe_something_else_holds_open() {
        struct Open;
        impl Read for Open {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                std::thread::sleep(Duration::from_secs(5));
                Ok(0)
            }
        }
        let started = std::time::Instant::now();
        assert_eq!(ErrorTail::start(Some(Open)).words(), "");
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[test]
    fn no_pipe_has_no_last_words() {
        assert_eq!(ErrorTail::start(None::<std::io::Empty>).words(), "");
    }
}
