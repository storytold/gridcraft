//! `cargo xtask stats`: tests and lines of Rust per crate.

use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt as _;

#[derive(Default, Clone, Copy)]
struct Counts {
    files: usize,
    lines: usize,
    tests: usize,
    proptests: usize,
}

/// Larger files (or devices behind a symlink) are not source files and are not read.
const MAX_SOURCE_BYTES: u64 = 16 << 20;

fn walk(dir: &Path, c: &mut Counts, seen: &mut std::collections::HashSet<(u64, u64)>, budget: &mut usize) {
    if *budget == 0 {
        // Hard stop for pathological trees: two directory symlinks pointing at each other
        // would otherwise be walked exponentially.
        return;
    }
    // Directory identities (dev+inode on unix) make a symlink to a visited directory a
    // no-op, not another exponential branch of the scan.
    #[cfg(unix)]
    let id = std::fs::metadata(dir).ok().map(|m| (m.dev(), m.ino()));
    #[cfg(not(unix))]
    let id: Option<(u64, u64)> = None;
    if let Some(id) = id
        && !seen.insert(id)
    {
        return;
    }
    *budget -= 1;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        // A symlinked directory is descended only if its identity is new (checked above).
        if p.is_dir() {
            // Nested fuzz workspaces and build output are not part of the crate.
            if name != "target" && name != "fuzz" && !name.starts_with('.') {
                walk(&p, c, seen, budget);
            }
        } else if name.ends_with(".rs")
            // The entry's own metadata does not follow a symlink: only regular files are sources.
            && e.metadata().is_ok_and(|m| m.is_file() && m.len() <= MAX_SOURCE_BYTES)
            && let Some(s) = read_source(&p)
        {
            c.files += 1;
            c.lines += s.lines().count();
            let (t, pt) = count_tests(&s);
            c.tests += t;
            c.proptests += pt;
        }
    }
}

/// A source file's text, read up to [`MAX_SOURCE_BYTES`]: one that grew or was replaced since its
/// size was checked is skipped, not read whole.
fn read_source(p: &Path) -> Option<String> {
    use std::io::Read as _;
    let mut s = String::new();
    std::fs::File::open(p).ok()?.take(MAX_SOURCE_BYTES + 1).read_to_string(&mut s).ok()?;
    (s.len() as u64 <= MAX_SOURCE_BYTES).then_some(s)
}

/// `#[test]` attributes and `proptest!` blocks (each block may hold many
/// property tests, and macro-generated tests are only counted once — use
/// `--exact` for harness-accurate numbers).
fn count_tests(src: &str) -> (usize, usize) {
    let mut tests = 0;
    let mut props = 0;
    for line in src.lines() {
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        tests += t.matches("#[test]").count();
        if t.starts_with("proptest!") {
            props += 1;
        }
    }
    (tests, props)
}

fn exact_tests(pkg: &str) -> Option<usize> {
    let out = crate::cargo().args(["test", "-q", "-p", pkg, "--", "--list"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).lines().filter(|l| l.ends_with(": test")).count())
}

pub fn run(_root: &Path, exact: bool) -> Result<(), String> {
    let meta = crate::metadata()?;
    let mut rows = Vec::new();
    for p in meta["packages"].as_array().into_iter().flatten() {
        let name = p["name"].as_str().unwrap_or("?").to_owned();
        let Some(dir) = p["manifest_path"].as_str().and_then(|m| Path::new(m).parent().map(Path::to_path_buf)) else {
            continue;
        };
        let mut c = Counts::default();
        walk(&dir, &mut c, &mut std::collections::HashSet::new(), &mut 10_000);
        let ex = if exact { exact_tests(&name) } else { None };
        rows.push((name, c, ex));
    }
    rows.sort_by(|a, b| a.0.cmp(&b.0));

    let exact_col = if exact { format!(" {:>8}", "harness") } else { String::new() };
    println!("{:<26} {:>6} {:>8} {:>7} {:>10}{exact_col}", "crate", "files", "lines", "#[test]", "proptest!");
    println!("{}", "-".repeat(62 + if exact { 9 } else { 0 }));
    let mut total = Counts::default();
    let mut total_exact = 0;
    for (name, c, ex) in &rows {
        let ex_s = match (exact, ex) {
            (false, _) => String::new(),
            (true, Some(n)) => {
                total_exact += n;
                format!(" {n:>8}")
            }
            (true, None) => format!(" {:>8}", "error"),
        };
        println!("{name:<26} {:>6} {:>8} {:>7} {:>10}{ex_s}", c.files, c.lines, c.tests, c.proptests);
        total.files += c.files;
        total.lines += c.lines;
        total.tests += c.tests;
        total.proptests += c.proptests;
    }
    println!("{}", "-".repeat(62 + if exact { 9 } else { 0 }));
    let ex_s = if exact { format!(" {total_exact:>8}") } else { String::new() };
    println!("{:<26} {:>6} {:>8} {:>7} {:>10}{ex_s}", "TOTAL", total.files, total.lines, total.tests, total.proptests);
    if !exact {
        println!("\n(#[test] counts source attributes; macro-generated tests count once. Use --exact for harness counts.)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{MAX_SOURCE_BYTES, count_tests, read_source};

    #[test]
    fn counts_attributes_and_proptest_blocks() {
        let src = "#[test]\nfn a() {}\n// #[test] commented\n    #[test]\nfn b() {}\nproptest! {\n}\n";
        assert_eq!(count_tests(src), (2, 1));
    }

    #[test]
    fn a_source_past_the_size_limit_is_not_read() {
        let dir = std::env::temp_dir().join(format!("xtask-stats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (small, big) = (dir.join("a.rs"), dir.join("b.rs"));
        std::fs::write(&small, "fn a() {}\n").unwrap();
        let file = std::fs::File::create(&big).unwrap();
        file.set_len(MAX_SOURCE_BYTES + 1).unwrap(); // sparse: nothing is written
        drop(file);
        assert_eq!(read_source(&small).as_deref(), Some("fn a() {}\n"));
        assert_eq!(read_source(&big), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
