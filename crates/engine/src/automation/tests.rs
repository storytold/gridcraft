use super::*;

/// A fresh folder holding `in` (with `a.csv`), `out`, `in2` (a sibling sharing the prefix) and
/// `outside` (with `secret.csv`).
fn folders(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("gc-automation-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    for d in ["in", "out", "in2", "outside"] {
        std::fs::create_dir_all(base.join(d)).unwrap();
    }
    std::fs::write(base.join("in/a.csv"), "a,b\n").unwrap();
    std::fs::write(base.join("outside/secret.csv"), "top,secret\n").unwrap();
    base
}

/// `p` as text, the way commands receive paths.
fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Reads stay in the read root and writes in the write root, however the path is spelled; the
/// write root itself and folders are not files to write.
#[test]
fn reads_and_writes_stay_inside_their_roots() {
    let base = folders("roots");
    let a = Automation::new(Some(&base.join("in")), Some(&base.join("out")), None).unwrap().unwrap();
    assert!(a.check_path(&s(&base.join("in/a.csv")), Access::Read).is_ok());
    assert!(a.check_path(&s(&base.join("out/new.xlsx")), Access::Write).is_ok());
    assert!(a.check_path(&s(&base.join("out/sub/new.xlsx")), Access::Write).is_ok(), "a folder still to be made");
    for (path, access) in [
        (base.join("outside/secret.csv"), Access::Read),
        (base.join("in2/a.csv"), Access::Read),
        (base.join("in/../outside/secret.csv"), Access::Read),
        (base.join("in/a.csv"), Access::Write),
        (base.join("out/../in/x.xlsx"), Access::Write),
        (base.join("out/missing/../../outside/x.xlsx"), Access::Write),
        (base.join("outside/x.xlsx"), Access::Write),
        // The root itself, and a folder inside it, are not files to write.
        (base.join("out"), Access::Write),
        (base.join("out/."), Access::Write),
        (base.join("out/sub/.."), Access::Write),
    ] {
        let e = a.check_path(&s(&path), access).unwrap_err();
        assert!(e.contains("automation path rejected"), "{path:?}: {e}");
    }
    assert!(a.check_path("", Access::Read).is_err());
    std::fs::create_dir_all(base.join("out/folder")).unwrap();
    let e = a.check_path(&s(&base.join("out/folder")), Access::Write).unwrap_err();
    assert!(e.contains("a folder is not a file to write"), "{e}");
    let _ = std::fs::remove_dir_all(&base);
}

/// With one root only, the other kind of access is refused outright.
#[test]
fn a_root_left_out_grants_none_of_its_access() {
    let base = folders("one-root");
    let read_only = Automation::new(Some(&base.join("in")), None, None).unwrap().unwrap();
    let e = read_only.check_path(&s(&base.join("in/x.xlsx")), Access::Write).unwrap_err();
    assert!(e.contains("write root is absent"), "{e}");
    let write_only = Automation::new(None, Some(&base.join("out")), None).unwrap().unwrap();
    assert!(write_only.check_path(&s(&base.join("in/a.csv")), Access::Read).is_err());
    let _ = std::fs::remove_dir_all(&base);
}

/// A root must be an existing folder; with nothing asked, nothing is in force.
#[test]
fn roots_must_be_existing_folders() {
    let base = folders("bad-roots");
    assert!(Automation::new(Some(&base.join("nope")), None, None).is_err());
    assert!(Automation::new(Some(&base.join("in/a.csv")), None, None).is_err());
    assert!(Automation::new(Some(Path::new("")), None, None).is_err());
    assert!(Automation::new(None, None, None).unwrap().is_none(), "nothing asked: nothing in force");
    let _ = std::fs::remove_dir_all(&base);
}

/// Symbolic links that lead out of a root, to a file or a folder, are outside it.
#[cfg(unix)]
#[test]
fn links_that_lead_out_are_outside() {
    let base = folders("links");
    std::os::unix::fs::symlink(base.join("outside/secret.csv"), base.join("in/link.csv")).unwrap();
    std::os::unix::fs::symlink(base.join("outside"), base.join("out/escape")).unwrap();
    std::os::unix::fs::symlink(base.join("outside/x.xlsx"), base.join("out/dangling.xlsx")).unwrap();
    std::os::unix::fs::symlink(base.join("in/a.csv"), base.join("outside/in-link.csv")).unwrap();
    let a = Automation::new(Some(&base.join("in")), Some(&base.join("out")), None).unwrap().unwrap();
    assert!(a.check_path(&s(&base.join("in/link.csv")), Access::Read).is_err(), "a file link out");
    assert!(a.check_path(&s(&base.join("out/escape/x.xlsx")), Access::Write).is_err(), "a folder link out");
    assert!(a.check_path(&s(&base.join("out/dangling.xlsx")), Access::Write).is_err(), "a link that leads out (or nowhere)");
    assert!(a.check_path(&s(&base.join("outside/in-link.csv")), Access::Write).is_err(), "a link outside pointing in is not a way to write outside");
    let _ = std::fs::remove_dir_all(&base);
}

/// Windows: device names are refused and case does not make another folder.
#[cfg(windows)]
#[test]
fn device_names_and_case() {
    let base = folders("win");
    let a = Automation::new(Some(&base.join("in")), Some(&base.join("out")), None).unwrap().unwrap();
    for name in ["NUL", "con.xlsx", "COM1.csv", "lpt2"] {
        assert!(a.check_path(&s(&base.join("out").join(name)), Access::Write).is_err(), "{name}");
    }
    let upper = s(&base.join("OUT").join("x.xlsx"));
    assert!(a.check_path(&upper, Access::Write).is_ok(), "case doesn't matter on Windows");
    let _ = std::fs::remove_dir_all(&base);
}

/// Lists take ids and `group.*` groups; unknown ids, bad patterns and empty or oversized lists are
/// errors.
#[test]
fn command_lists_take_ids_and_groups_and_refuse_typos() {
    let l = CommandAllowlist::parse("cell.set, home.*\n# saving\nfile.save  # in place\n").unwrap();
    assert!(l.allows("cell.set") && l.allows("home.bold") && l.allows("file.save"));
    assert!(!l.allows("file.open") && !l.allows("homex.bold") && !l.allows("automate.runScript"));
    assert_eq!(l.describe(), vec!["cell.set", "file.save", "home.*"]);
    for bad in ["", "  # nothing\n", "cell.sett", "nope.*", "*", ".*", "home*", "ho*me.bold", "home.**"] {
        assert!(CommandAllowlist::parse(bad).is_err(), "{bad:?}");
    }
    assert!(CommandAllowlist::parse(&"x".repeat(MAX_LIST_BYTES + 1)).is_err());
}

/// A thread scope holds only inside `confine` and ends with it.
#[test]
fn a_thread_scope_is_in_force_inside_confine_only() {
    let base = folders("scope");
    let list = CommandAllowlist::parse("cell.set").unwrap();
    let a = Arc::new(Automation::new(Some(&base.join("in")), None, Some(list)).unwrap().unwrap());
    assert!(check_command("file.open").is_ok());
    confine(Some(&a), || {
        assert!(check_command("cell.set").is_ok());
        let e = check_command("file.open").unwrap_err().to_string();
        assert!(e.contains("file.open") && e.contains("--allow-commands"), "{e}");
        assert!(check_read(&s(&base.join("in/a.csv"))).is_ok());
        assert!(check_read(&s(&base.join("outside/secret.csv"))).is_err());
        assert!(check_write(&s(&base.join("in/x.xlsx"))).is_err());
    });
    assert!(check_command("file.open").is_ok(), "the scope ends with confine");
    assert!(check_read(&s(&base.join("outside/secret.csv"))).is_ok());
    let _ = std::fs::remove_dir_all(&base);
}

/// A command list alone leaves files as they were.
#[test]
fn commands_only_leave_files_unrestricted() {
    let a = Automation::new(None, None, Some(CommandAllowlist::parse("cell.set").unwrap())).unwrap().unwrap();
    assert!(a.check_path("/anything/at/all.xlsx", Access::Write).is_ok());
    assert!(a.check_command("file.open").is_err());
}
