//! `cargo xtask update`: keep a source install current.
//!
//! For people who run GridCraft from a checkout (`cargo install --path apps/gridcraft`) instead of
//! a release package. It fetches the upstream branch, fast-forwards the checkout when it is strictly
//! behind, and reinstalls `gridcraft` and `gridcraft-cli` with `cargo install`. `--launch` then
//! opens the freshly installed app, so a desktop shortcut can mean "update, then open".
//!
//! It never discards work: a dirty tree, another branch checked out, or local commits that are not
//! upstream all stop it with a message instead of a merge. Only `git merge --ff-only` touches the
//! checkout, and only when the plan says so.

use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask update [--check] [--force] [--remote NAME] [--branch NAME] [--launch [ARG...]]";

/// The packages `cargo install` puts on the PATH, relative to the workspace root.
const INSTALL_PATHS: [&str; 2] = ["apps/gridcraft", "apps/gridcraft-cli"];

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    /// Report only; never merge or install.
    pub check: bool,
    /// Reinstall even when there is nothing to pull (after `rustup update`, or a first install).
    pub force: bool,
    pub remote: String,
    pub branch: String,
    /// Run the installed `gridcraft` afterwards, with these arguments.
    pub launch: Option<Vec<String>>,
}

pub fn parse(args: &[&str]) -> Result<Options, String> {
    let mut opts = Options { check: false, force: false, remote: "origin".into(), branch: "main".into(), launch: None };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match *arg {
            "--check" => opts.check = true,
            "--force" => opts.force = true,
            "--remote" | "--branch" => {
                let value = it.next().ok_or_else(|| format!("{arg} needs a value\n\n{USAGE}"))?;
                if value.is_empty() || value.starts_with('-') {
                    return Err(format!("{arg}: `{value}` is not a git name\n\n{USAGE}"));
                }
                if *arg == "--remote" { opts.remote = value.to_string() } else { opts.branch = value.to_string() }
            }
            "--launch" => {
                opts.launch = Some(it.by_ref().map(|s| s.to_string()).collect());
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }
    Ok(opts)
}

/// Where the checkout stands relative to the upstream branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    /// Upstream commits not in HEAD.
    pub behind: usize,
    /// Local commits not upstream.
    pub ahead: usize,
    /// Tracked files with uncommitted changes.
    pub dirty: bool,
    /// `git rev-parse --abbrev-ref HEAD` (`HEAD` when detached).
    pub head_branch: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    UpToDate,
    FastForward,
    /// Something would be lost or merged; the string says what to do instead.
    Refuse(String),
}

/// What to do for `state`, given that the user wants to track `branch`.
pub fn plan(state: &State, branch: &str) -> Plan {
    if state.behind == 0 {
        return Plan::UpToDate;
    }
    if state.dirty {
        return Plan::Refuse("the working tree has uncommitted changes; commit or stash them first".into());
    }
    if state.head_branch != branch {
        return Plan::Refuse(format!(
            "`{}` is checked out, not `{branch}`; switch branches first (or pass --branch {})",
            state.head_branch, state.head_branch
        ));
    }
    if state.ahead > 0 {
        return Plan::Refuse(format!(
            "`{branch}` has {} local commit(s) that are not upstream; rebase or merge them yourself (nothing was changed)",
            state.ahead
        ));
    }
    Plan::FastForward
}

pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let opts = parse(args)?;
    let upstream = format!("{}/{}", opts.remote, opts.branch);

    let mut fetch = git_cmd(root);
    fetch.args(["fetch", "--quiet", &opts.remote, &opts.branch]);
    crate::run(fetch, &format!("git fetch {} {}", opts.remote, opts.branch))?;

    let state = state(root, &upstream)?;
    let head = git(root, &["rev-parse", "--short", "HEAD"])?;
    let target = git(root, &["rev-parse", "--short", &upstream])?;
    println!("{}", summary(&state, &head, &upstream, &target));

    let mut installed = false;
    match plan(&state, &opts.branch) {
        Plan::Refuse(why) => return Err(why),
        Plan::UpToDate => {
            if opts.force && !opts.check {
                install(root)?;
                installed = true;
            } else {
                println!("up to date");
            }
        }
        Plan::FastForward => {
            let log = git(root, &["log", "--oneline", "--no-decorate", &format!("HEAD..{upstream}")])?;
            println!("new commits:\n{log}");
            if opts.check {
                println!("run `cargo xtask update` to pull them and reinstall");
            } else {
                let mut merge = git_cmd(root);
                merge.args(["merge", "--ff-only", &upstream]);
                crate::run(merge, &format!("git merge --ff-only {upstream}"))?;
                install(root)?;
                installed = true;
            }
        }
    }
    if installed {
        println!("installed gridcraft and gridcraft-cli from {target}");
    }
    if let Some(args) = opts.launch {
        launch(&args)?;
    }
    Ok(())
}

/// One line, e.g. `HEAD bb685e3 vs origin/main 1a2b3c4: 3 behind, 0 ahead, clean`.
pub fn summary(state: &State, head: &str, upstream: &str, target: &str) -> String {
    format!(
        "HEAD {head} ({}) vs {upstream} {target}: {} behind, {} ahead, {}",
        state.head_branch,
        state.behind,
        state.ahead,
        if state.dirty { "uncommitted changes" } else { "clean" }
    )
}

fn state(root: &Path, upstream: &str) -> Result<State, String> {
    let behind = count(&git(root, &["rev-list", "--count", &format!("HEAD..{upstream}")])?)?;
    let ahead = count(&git(root, &["rev-list", "--count", &format!("{upstream}..HEAD")])?)?;
    let dirty = !git(root, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    let head_branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    Ok(State { behind, ahead, dirty, head_branch })
}

/// The number `git rev-list --count` printed.
pub fn count(output: &str) -> Result<usize, String> {
    let s = output.trim();
    s.parse().map_err(|e| format!("git rev-list --count printed `{s}`: {e}"))
}

fn install(root: &Path) -> Result<(), String> {
    for path in INSTALL_PATHS {
        let mut c = crate::cargo();
        c.current_dir(root);
        // --force: cargo refuses to reinstall the same version otherwise, and the workspace
        // version rarely changes between commits.
        c.args(["install", "--locked", "--force", "--path", path]);
        crate::run(c, &format!("cargo install --locked --force --path {path}"))?;
    }
    Ok(())
}

/// Where `cargo install` put `name`: `$CARGO_INSTALL_ROOT/bin`, else `$CARGO_HOME/bin`, else
/// `~/.cargo/bin`. Falls back to the bare name (PATH lookup) when that file does not exist.
pub fn installed_bin(name: &str) -> PathBuf {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let dir = std::env::var_os("CARGO_INSTALL_ROOT")
        .or_else(|| std::env::var_os("CARGO_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join(".cargo")));
    match dir.map(|d| d.join("bin").join(&exe)) {
        Some(p) if p.is_file() => p,
        _ => PathBuf::from(exe),
    }
}

fn launch(args: &[String]) -> Result<(), String> {
    let bin = installed_bin("gridcraft");
    eprintln!("$ {} {}", bin.display(), args.join(" "));
    // Don't wait: the app outlives this tool, and a shortcut should return to the shell.
    let child = Command::new(&bin).args(args).spawn().map_err(|e| format!("launch {}: {e}", bin.display()))?;
    println!("launched gridcraft (pid {})", child.id());
    Ok(())
}

fn git_cmd(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(root);
    c
}

/// Trimmed stdout of a git command that must succeed.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    let out = git_cmd(root).args(args).output().map_err(|e| format!("git {}: failed to spawn: {e}", args.join(" ")))?;
    if !out.status.success() {
        return Err(format!("git {}: exited with {}\n{}", args.join(" "), out.status, String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(behind: usize, ahead: usize, dirty: bool, branch: &str) -> State {
        State { behind, ahead, dirty, head_branch: branch.into() }
    }

    #[test]
    fn parses_defaults() {
        let o = parse(&[]).unwrap();
        assert_eq!(o, Options { check: false, force: false, remote: "origin".into(), branch: "main".into(), launch: None });
    }

    #[test]
    fn parses_flags_and_names() {
        let o = parse(&["--check", "--force", "--remote", "upstream", "--branch", "develop"]).unwrap();
        assert!(o.check && o.force);
        assert_eq!(o.remote, "upstream");
        assert_eq!(o.branch, "develop");
        assert_eq!(o.launch, None);
    }

    #[test]
    fn launch_takes_the_rest_of_the_line() {
        let o = parse(&["--launch", "--sample", "sales", "--check"]).unwrap();
        assert!(!o.check, "--check after --launch belongs to the app");
        assert_eq!(o.launch, Some(vec!["--sample".to_string(), "sales".into(), "--check".into()]));
        assert_eq!(parse(&["--launch"]).unwrap().launch, Some(vec![]));
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(parse(&["--remote"]).is_err());
        assert!(parse(&["--remote", "--check"]).is_err(), "a value that looks like an option is not a git name");
        assert!(parse(&["--branch", ""]).is_err());
        assert!(parse(&["--nope"]).is_err());
        assert!(parse(&["main"]).is_err());
    }

    #[test]
    fn up_to_date_wins_over_everything_else() {
        assert_eq!(plan(&st(0, 0, false, "main"), "main"), Plan::UpToDate);
        assert_eq!(plan(&st(0, 3, true, "feature"), "main"), Plan::UpToDate, "nothing to pull means nothing to refuse");
    }

    #[test]
    fn fast_forwards_only_a_clean_checkout_of_the_branch() {
        assert_eq!(plan(&st(2, 0, false, "main"), "main"), Plan::FastForward);
        assert_eq!(plan(&st(2, 0, false, "develop"), "develop"), Plan::FastForward);
    }

    #[test]
    fn refuses_when_work_could_be_lost_or_merged() {
        for (state, needle) in [
            (st(1, 0, true, "main"), "uncommitted"),
            (st(1, 0, false, "feature"), "`feature` is checked out"),
            (st(1, 0, false, "HEAD"), "`HEAD` is checked out"),
            (st(1, 2, false, "main"), "2 local commit(s)"),
        ] {
            match plan(&state, "main") {
                Plan::Refuse(why) => assert!(why.contains(needle), "{state:?}: {why}"),
                other => panic!("{state:?}: expected Refuse, got {other:?}"),
            }
        }
        // Dirty is checked before the branch so the message names the first thing to fix.
        assert!(matches!(plan(&st(1, 0, true, "feature"), "main"), Plan::Refuse(w) if w.contains("uncommitted")));
    }

    #[test]
    fn parses_rev_list_counts() {
        assert_eq!(count("0\n").unwrap(), 0);
        assert_eq!(count("  17 ").unwrap(), 17);
        assert!(count("").is_err());
        assert!(count("abc").is_err());
    }

    #[test]
    fn summarises_the_state() {
        let s = summary(&st(3, 0, false, "main"), "bb685e3", "origin/main", "1a2b3c4");
        assert_eq!(s, "HEAD bb685e3 (main) vs origin/main 1a2b3c4: 3 behind, 0 ahead, clean");
        assert!(summary(&st(0, 1, true, "x"), "a", "o/m", "b").ends_with("0 behind, 1 ahead, uncommitted changes"));
    }

    #[test]
    fn installed_bin_has_the_platform_suffix() {
        let p = installed_bin("gridcraft");
        let name = p.file_name().and_then(|n| n.to_str()).unwrap();
        assert_eq!(name, format!("gridcraft{}", std::env::consts::EXE_SUFFIX));
    }

    #[test]
    fn install_paths_are_workspace_members() {
        for p in INSTALL_PATHS {
            assert!(crate::root().join(p).join("Cargo.toml").is_file(), "{p}");
        }
    }
}
