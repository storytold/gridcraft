//! `cargo xtask parity [--check] [--out PATH]`: write `docs/parity-checklist.md` from the engine's command
//! catalog (`gridcraft_engine::catalog::CATALOG`, Excel's ribbon and menu tree). A catalog id is
//! implemented when a command with that id is registered (`gridcraft_engine::find_command`).
//!
//! `--check` fails if `docs/parity-checklist.md` is out of date instead of writing it; `--out` writes
//! somewhere else. The rendering is pure (`render`) so it is unit-tested without the engine.

use std::path::Path;
#[cfg(feature = "engine")]
use std::path::PathBuf;

/// One catalog row as the renderer sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub tab: String,
    pub group: String,
    pub label: String,
    pub done: bool,
}

/// RFC-0052 hand marks for catalog ids with no engine command: `id|headless|reach|why`.
///
/// `headless` is `no` or `partial`; `reach` is `window` (needs a desktop window, dialog or pointer),
/// `os` (needs an OS or online service the engine doesn't wrap) or `engine-gap` (could be an engine
/// command, isn't one yet). `yes` is never written here: an id is `yes` only when the engine registry
/// that `gridcraft-cli` and the MCP server load has it, and a mark on such an id is stale
/// (`mark_errors`). A missing id with no mark is `unknown`.
pub const HEADLESS: &str = "\
insert.smartArt|no|engine-gap|diagrams could be built by a command
insert.screenshot|no|os|screen capture of other windows
insert.map|no|os|online map service
insert.timeline|no|engine-gap|a date filter for pivot tables could be a command
insert.signatureLine|no|engine-gap|a signature line object could be inserted by a command
insert.object|no|os|embedding another program's object
data.fromWeb|no|os|fetching a web page
data.stocks|no|os|online stock data service
data.geography|no|os|online geography data service
data.relationships|no|engine-gap|data-model relationships could be commands
data.analyzeData|no|os|online analysis service
review.smartLookup|no|os|online search service
review.translate|no|os|online translation service
review.showChanges|no|os|co-authoring change history service
";

/// A row's RFC-0052 `headless:` value, and for `no`/`partial` its `reach:` and reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Headless<'a> {
    pub value: &'a str,
    pub reach: Option<&'a str>,
    pub why: &'a str,
}

const YES: Headless<'static> = Headless { value: "yes", reach: None, why: "" };
const UNKNOWN: Headless<'static> = Headless { value: "unknown", reach: None, why: "" };

/// Parse `HEADLESS`-format marks. A malformed line, a hand-written `yes`, an unknown reach or a
/// repeated id is an error.
pub fn parse_marks(src: &str) -> Result<Vec<(&str, Headless<'_>)>, String> {
    let mut out: Vec<(&str, Headless<'_>)> = Vec::new();
    for line in src.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.splitn(4, '|').collect();
        let [id, value, reach, why] = f[..] else {
            return Err(format!("headless mark `{line}`: want `id|headless|reach|why`"));
        };
        if !matches!(value, "no" | "partial") {
            return Err(format!("headless mark `{id}`: `{value}` is not `no` or `partial` (`yes` comes from the registry)"));
        }
        if !matches!(reach, "window" | "os" | "engine-gap") {
            return Err(format!("headless mark `{id}`: reach `{reach}` is not window, os or engine-gap"));
        }
        if out.iter().any(|(m, _)| *m == id) {
            return Err(format!("headless mark `{id}` is written twice"));
        }
        out.push((id, Headless { value, reach: Some(reach), why }));
    }
    Ok(out)
}

/// `yes` for an id the engine registry has, else its mark, else `unknown`.
pub fn headless_of<'a>(e: &Entry, marks: &[(&str, Headless<'a>)]) -> Headless<'a> {
    if e.done {
        return YES;
    }
    marks.iter().find(|(m, _)| *m == e.id).map_or(UNKNOWN, |(_, h)| *h)
}

/// Marks that name no catalog id, or an id the engine registry has (it would hide a `yes`).
pub fn mark_errors(src: &str, entries: &[Entry]) -> Vec<String> {
    let marks = match parse_marks(src) {
        Ok(m) => m,
        Err(e) => return vec![e],
    };
    let mut errs = Vec::new();
    for (id, _) in &marks {
        match entries.iter().find(|e| e.id == *id) {
            None => errs.push(format!("headless mark `{id}` is not a catalog id")),
            Some(e) if e.done => errs.push(format!("headless mark `{id}` is stale: the engine registry has it, so it is `yes`")),
            Some(_) => {}
        }
    }
    errs
}

fn pct(done: usize, total: usize) -> f64 {
    if total == 0 { 0.0 } else { 100.0 * done as f64 / total as f64 }
}

/// The markdown for docs/parity-checklist.md. Ids are counted once (the first catalog row wins), tabs keep
/// catalog order.
pub fn render(entries: &[Entry]) -> String {
    render_with(entries, HEADLESS)
}

/// `render` with the given `HEADLESS`-format marks (bad marks are left out; `mark_errors` reports them).
pub fn render_with(entries: &[Entry], marks: &str) -> String {
    let marks = parse_marks(marks).unwrap_or_default();
    let mut seen: Vec<&str> = Vec::new();
    let mut unique: Vec<&Entry> = Vec::new();
    for e in entries {
        if !seen.contains(&e.id.as_str()) {
            seen.push(&e.id);
            unique.push(e);
        }
    }
    let mut tabs: Vec<&str> = Vec::new();
    for e in &unique {
        if !tabs.contains(&e.tab.as_str()) {
            tabs.push(&e.tab);
        }
    }
    let done = unique.iter().filter(|e| e.done).count();
    let total = unique.len();

    let mut s = String::new();
    s += "# Excel parity checklist\n\n";
    s += "<!-- Generated by `cargo xtask parity` from crates/engine/src/catalog.rs. Don't edit by hand. -->\n\n";
    s += "Each entry is an Excel ribbon or menu command (feature names only). It counts as implemented when GridCraft \
          registers a command with the same id. This measures breadth only; the overall assessment (depth, \
          fidelity, effort) is in [target-app-parity.md](target-app-parity.md).\n\n";
    s += &format!("**Overall: {done} / {total} commands implemented ({:.1}%).**\n\n", pct(done, total));
    let hs: Vec<Headless<'_>> = unique.iter().map(|e| headless_of(e, &marks)).collect();
    let count = |v: &str| hs.iter().filter(|h| h.value == v).count();
    let no = |r: &str| hs.iter().filter(|h| h.value == "no" && h.reach == Some(r)).count();
    s += &format!(
        "**Headless ([RFC-0052](https://github.com/storytold/craft-contrib/blob/main/rfcs/0052-headless-reach.md)): {} yes, {} no \
         (window {}, os {}, engine-gap {}), {} partial, {} unknown.** An implemented command is `headless: yes`: it is in the \
         engine registry that `gridcraft-cli` and the MCP server load. A missing one carries its hand mark from `HEADLESS` in \
         `xtask/src/parity.rs`, or `unknown`.\n\n",
        count("yes"),
        count("no"),
        no("window"),
        no("os"),
        no("engine-gap"),
        count("partial"),
        count("unknown")
    );
    s += "| Tab | Implemented | Total | Parity |\n|---|---|---|---|\n";
    for t in &tabs {
        let rows: Vec<&&Entry> = unique.iter().filter(|e| e.tab == *t).collect();
        let d = rows.iter().filter(|e| e.done).count();
        s += &format!("| {t} | {d} | {} | {:.0}% |\n", rows.len(), pct(d, rows.len()));
    }
    for t in &tabs {
        let rows: Vec<&&Entry> = unique.iter().filter(|e| e.tab == *t).collect();
        let d = rows.iter().filter(|e| e.done).count();
        s += &format!("\n## {t} ({d} / {}, {:.0}%)\n", rows.len(), pct(d, rows.len()));
        for (title, want) in [("Implemented", true), ("Missing", false)] {
            let list: Vec<String> = rows
                .iter()
                .filter(|e| e.done == want)
                .map(|e| {
                    let line = format!("- `{}` {} ({})", e.id, e.label, e.group);
                    if want {
                        return line;
                    }
                    let h = headless_of(e, &marks);
                    match h.reach {
                        Some(r) => format!("{line}: headless: {}, reach: {r} ({})", h.value, h.why),
                        None => format!("{line}: headless: {}", h.value),
                    }
                })
                .collect();
            if !list.is_empty() {
                s += &format!("\n{title}:\n\n{}\n", list.join("\n"));
            }
        }
    }
    s
}

/// Whether the checked-in checklist matches the generated text. A Windows checkout with
/// `core.autocrlf=true` (Git for Windows' default) has CRLF line ends, which aren't a change.
pub fn up_to_date(current: &str, generated: &str) -> bool {
    current == generated || current.replace("\r\n", "\n") == generated
}

#[cfg(feature = "engine")]
fn entries() -> Vec<Entry> {
    use gridcraft_engine::catalog::CATALOG;
    CATALOG
        .iter()
        .map(|(id, tab, group, label)| Entry {
            id: (*id).to_string(),
            tab: (*tab).to_string(),
            group: (*group).to_string(),
            label: (*label).to_string(),
            done: gridcraft_engine::find_command(id).is_some(),
        })
        .collect()
}

#[cfg(feature = "engine")]
fn summary_line() -> String {
    let (done, total) = gridcraft_engine::catalog::parity();
    format!("parity: {done} / {total} catalog commands implemented ({:.1}%)", pct(done, total))
}

#[cfg(feature = "engine")]
pub fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    let mut check = false;
    let mut out: PathBuf = root.join("docs/parity-checklist.md");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--check" => check = true,
            "--out" => out = PathBuf::from(it.next().ok_or("--out needs a path")?),
            other => return Err(format!("parity: unknown argument `{other}` (usage: cargo xtask parity [--check] [--out PATH])")),
        }
    }
    let entries = entries();
    let errs = mark_errors(HEADLESS, &entries);
    if !errs.is_empty() {
        return Err(format!("parity: RFC-0052 headless marks in xtask/src/parity.rs:\n  {}", errs.join("\n  ")));
    }
    let text = render(&entries);
    if check {
        let current = std::fs::read_to_string(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        if !up_to_date(&current, &text) {
            return Err(format!("{} is out of date; run `cargo xtask parity`", out.display()));
        }
        println!("{} ({} up to date)", summary_line(), out.display());
        return Ok(());
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    }
    std::fs::write(&out, text).map_err(|e| format!("write {}: {e}", out.display()))?;
    println!("{} -> {}", summary_line(), out.display());
    Ok(())
}

#[cfg(not(feature = "engine"))]
pub fn run(_root: &Path, _args: &[&str]) -> Result<(), String> {
    Err("xtask was built without the `engine` feature; run `cargo xtask parity` with default features".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, tab: &str, done: bool) -> Entry {
        Entry { id: id.into(), tab: tab.into(), group: "G".into(), label: id.to_uppercase(), done }
    }

    #[test]
    fn groups_by_tab_and_counts_unique_ids() {
        let s = render(&[e("file.new", "File", true), e("file.open", "File", false), e("home.bold", "Home", true), e("file.new", "Home", false)]);
        assert!(s.contains("**Overall: 2 / 3 commands implemented (66.7%).**"), "{s}");
        assert!(s.contains("| File | 1 | 2 | 50% |"), "{s}");
        assert!(s.contains("| Home | 1 | 1 | 100% |"), "{s}");
        assert!(s.contains("## File (1 / 2, 50%)"));
        assert!(s.contains("Missing:\n\n- `file.open` FILE.OPEN (G)"));
        // File comes before Home (catalog order).
        assert!(s.find("## File").unwrap_or(usize::MAX) < s.find("## Home").unwrap_or(0));
    }

    #[test]
    fn headless_yes_comes_only_from_the_registry() {
        let marks = "file.open|no|window|a dialog\n";
        let s = render_with(&[e("file.new", "File", true), e("file.open", "File", false), e("home.bold", "Home", false)], marks);
        assert!(s.contains("1 yes, 1 no (window 1, os 0, engine-gap 0), 0 partial, 1 unknown"), "{s}");
        assert!(s.contains("- `file.open` FILE.OPEN (G): headless: no, reach: window (a dialog)"), "{s}");
        assert!(s.contains("- `home.bold` HOME.BOLD (G): headless: unknown"), "{s}");
        // A live id is `yes` whatever a mark says.
        let parsed = parse_marks("file.new|no|os|x\n").unwrap_or_default();
        assert_eq!(headless_of(&e("file.new", "File", true), &parsed).value, "yes");
    }

    #[test]
    fn bad_marks_are_refused() {
        for bad in ["a.b|yes|window|x", "a.b|no|desk|x", "a.b|no", "a.b|no|os|x\na.b|partial|os|y"] {
            assert!(parse_marks(bad).is_err(), "{bad}");
        }
        let entries = [e("a.b", "T", true), e("c.d", "T", false)];
        let errs = mark_errors("a.b|no|os|x\nz.z|no|os|y\nc.d|partial|engine-gap|z\n", &entries);
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(errs.iter().any(|m| m.contains("`a.b` is stale")), "{errs:?}");
        assert!(errs.iter().any(|m| m.contains("`z.z` is not a catalog id")), "{errs:?}");
        assert_eq!(mark_errors("junk", &entries).len(), 1);
    }

    #[cfg(feature = "engine")]
    #[test]
    fn checked_in_marks_are_current() {
        let errs = mark_errors(HEADLESS, &entries());
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn empty_catalog_renders() {
        assert!(render(&[]).contains("0 / 0 commands implemented (0.0%)"));
    }

    #[test]
    fn a_crlf_checkout_of_the_current_checklist_is_up_to_date() {
        let text = render(&[e("file.new", "File", true), e("home.bold", "Home", false)]);
        assert!(up_to_date(&text, &text));
        assert!(up_to_date(&text.replace('\n', "\r\n"), &text));
        let stale = render(&[e("file.new", "File", false), e("home.bold", "Home", false)]);
        assert!(!up_to_date(&stale, &text));
        assert!(!up_to_date(&stale.replace('\n', "\r\n"), &text));
    }
}
