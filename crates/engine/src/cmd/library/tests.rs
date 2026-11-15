//! Insert ▸ Library: the `library.*` commands over real folders of pictures.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value as Json, json};

use crate::library::{MAX_DEPTH, MAX_ENTRIES, MAX_FOLDERS};
use crate::{EngineError, Session};

/// A folder of its own under the system's temporary folder, removed when dropped.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Dir {
    fn new(tag: &str) -> Dir {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("gridcraft-library-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // The canonical path, as the Library keeps it (macOS's temporary folder is behind a link).
        Dir(std::fs::canonicalize(&dir).unwrap())
    }
    fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let p = self.0.join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, bytes).unwrap();
        p.to_str().unwrap().to_string()
    }
}

fn png(w: u32, h: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(w, h, image::Rgba([200, 40, 40, 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    image::RgbImage::from_pixel(w, h, image::Rgb([40, 160, 40])).write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
    out.into_inner()
}

fn s() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn add(s: &mut Session, dir: &Dir) -> Json {
    s.execute("library.addFolder", json!({"path": dir.path()})).unwrap()
}

fn list(s: &mut Session) -> Vec<Json> {
    s.execute("library.list", json!({})).unwrap().as_array().unwrap().clone()
}

fn err(r: crate::Result<Json>) -> String {
    let e = r.unwrap_err();
    assert!(!matches!(e, EngineError::Internal(..)), "{e}");
    e.to_string()
}

fn images(s: &Session) -> usize {
    s.doc().unwrap().wb.active().unwrap().images.len()
}

/// A folder of the kinds of files a picture folder holds: two pictures, a picture in a
/// subfolder, and files the Library leaves out.
fn pictures() -> Dir {
    let d = Dir::new("pictures");
    d.file("Logo.png", &png(64, 32));
    d.file("photo.JPG", &jpeg(40, 30));
    d.file("icons/arrow.png", &png(16, 16));
    d.file("notes.txt", b"not a picture");
    d.file(".hidden.png", &png(8, 8));
    d.file("~$lock.png", &png(8, 8));
    d.file("drawing.svg", b"<svg/>");
    d
}

#[test]
fn folders_are_added_listed_and_removed() {
    let d = pictures();
    let mut s = s();
    assert_eq!(s.execute("library.folders", json!({})).unwrap(), json!([]));
    let added = add(&mut s, &d);
    assert_eq!(added["path"], d.path());
    assert_eq!(added["added"], true);
    assert_eq!(s.prefs.library_folders, [d.path()]);
    // The same folder written another way is the same folder.
    let again = s.execute("library.addFolder", json!({"path": format!("{}/icons/..", d.path())})).unwrap();
    assert_eq!(again["added"], false);
    assert_eq!(s.prefs.library_folders.len(), 1);

    let folders = s.execute("library.folders", json!({})).unwrap();
    assert_eq!(folders[0]["path"], d.path());
    assert_eq!(folders[0]["files"], 3, "{folders}");

    let files = list(&mut s);
    let names: Vec<&str> = files.iter().map(|f| f["pictures"][0]["title"].as_str().unwrap()).collect();
    assert_eq!(names, ["Logo.png", "photo.JPG", "arrow.png"], "a folder's files in name order, then its subfolders");
    for f in &files {
        assert_eq!(f["pictureCount"], 1, "{f}");
        assert_eq!(f["folder"], d.path());
        assert!(f.get("error").is_none(), "{f}");
    }
    assert_eq!(files[0]["title"], "Logo", "the file name stands for the title");
    // A picture is known from its file name, so `pictures: false` gives the same answer.
    let quick = s.execute("library.list", json!({"pictures": false})).unwrap();
    assert_eq!(quick, json!(files));
    // One file.
    let one = s.execute("library.list", json!({"file": d.0.join("Logo.png")})).unwrap();
    assert_eq!(one.as_array().unwrap().len(), 1);

    // Not a folder, and not there.
    assert!(err(s.execute("library.addFolder", json!({"path": d.0.join("Logo.png")}))).contains("is not a folder"));
    assert!(err(s.execute("library.addFolder", json!({"path": d.0.join("missing")}))).contains("missing"));
    assert!(err(s.execute("library.removeFolder", json!({"path": "/no/such/folder"}))).contains("not a Library folder"));

    let removed = s.execute("library.removeFolder", json!({"path": d.path()})).unwrap();
    assert_eq!(removed["folders"], json!([]));
    assert!(s.prefs.library_folders.is_empty());
    assert!(list(&mut s).is_empty());
    // The files are where they were.
    assert!(d.0.join("Logo.png").is_file());
}

#[test]
fn the_folder_list_is_bounded() {
    let dirs: Vec<Dir> = (0..=MAX_FOLDERS).map(|_| Dir::new("many")).collect();
    let mut s = s();
    for d in dirs.iter().take(MAX_FOLDERS) {
        add(&mut s, d);
    }
    let last = dirs.last().unwrap();
    assert!(err(s.execute("library.addFolder", json!({"path": last.path()}))).contains("at most 32 folders"));
    assert_eq!(s.prefs.library_folders.len(), MAX_FOLDERS);
}

#[test]
fn deep_and_crowded_folders_are_bounded() {
    let d = Dir::new("deep");
    let mut deep = String::new();
    for i in 0..=MAX_DEPTH + 1 {
        deep.push_str(&format!("level{i}/"));
    }
    d.file(&format!("{deep}far.png"), &png(4, 4));
    d.file("near.png", &png(4, 4));
    let mut s = s();
    add(&mut s, &d);
    let folders = s.execute("library.folders", json!({})).unwrap();
    assert_eq!(folders[0]["files"], 1, "{folders}");
    assert_eq!(folders[0]["skippedDeep"], 1, "{folders}");

    let crowded = Dir::new("crowded");
    for i in 0..MAX_ENTRIES + 5 {
        crowded.file(&format!("f{i:05}.txt"), b"");
    }
    add(&mut s, &crowded);
    let folders = s.execute("library.folders", json!({})).unwrap();
    assert_eq!(folders[1]["entries"], MAX_ENTRIES, "{}", folders[1]);
    assert_eq!(folders[1]["truncated"], true);
}

#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    let outside = Dir::new("outside");
    let secret = outside.file("secret.png", &png(8, 8));
    let d = Dir::new("links");
    d.file("mine.png", &png(8, 8));
    std::os::unix::fs::symlink(&outside.0, d.0.join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(&secret, d.0.join("link.png")).unwrap();
    let mut s = s();
    add(&mut s, &d);
    let folders = s.execute("library.folders", json!({})).unwrap();
    assert_eq!(folders[0]["files"], 1, "{folders}");
    assert_eq!(folders[0]["skippedLinks"], 2, "{folders}");
    let files = list(&mut s);
    assert_eq!(files.len(), 1);
    assert_eq!(files[0]["pictures"][0]["title"], "mine.png");
    // Named through the link, the file is outside the Library: neither drawn nor inserted.
    let through = d.0.join("link.png");
    assert!(err(s.execute("library.thumbnail", json!({"file": through, "picture": 0}))).contains("not in a Library folder"));
    assert!(err(s.execute("library.insert", json!({"file": through}))).contains("not in a Library folder"));
    assert_eq!(images(&s), 0);
}

#[cfg(unix)]
#[test]
fn a_pipe_named_like_a_picture_is_left_alone() {
    let d = Dir::new("fifo");
    d.file("real.png", &png(8, 8));
    let fifo = d.0.join("pipe.png");
    if !std::process::Command::new("mkfifo").arg(&fifo).status().is_ok_and(|s| s.success()) {
        return; // no mkfifo here
    }
    let mut s = s();
    add(&mut s, &d);
    // Listing doesn't open it (it would block), and naming it is refused before opening.
    assert_eq!(list(&mut s).len(), 1);
    assert!(err(s.execute("library.insert", json!({"file": fifo}))).contains("not a file"));
}

#[test]
fn search_finds_pictures_by_name_and_format() {
    // Searching reads no file: these are known from their names.
    let d = pictures();
    let mut s = s();
    add(&mut s, &d);
    let hits = s.execute("library.search", json!({"query": "LOGO"})).unwrap();
    assert_eq!(hits.as_array().unwrap().len(), 1, "{hits}");
    assert_eq!(hits[0]["title"], "Logo.png");
    assert_eq!(hits[0]["picture"], 0);
    assert_eq!(hits[0]["match"], "title");
    let jpegs = s.execute("library.search", json!({"query": "jpeg"})).unwrap();
    assert_eq!(jpegs.as_array().unwrap().len(), 1, "{jpegs}");
    assert_eq!(jpegs[0]["match"], "text");
    let limited = s.execute("library.search", json!({"query": "png", "limit": 1})).unwrap();
    assert_eq!(limited.as_array().unwrap().len(), 1);
    // One folder only.
    let other = Dir::new("other");
    other.file("logo-dark.png", &png(4, 4));
    add(&mut s, &other);
    assert_eq!(s.execute("library.search", json!({"query": "logo"})).unwrap().as_array().unwrap().len(), 2);
    assert_eq!(s.execute("library.search", json!({"query": "logo", "folder": other.path()})).unwrap().as_array().unwrap().len(), 1);
    // Bad searches.
    assert!(err(s.execute("library.search", json!({}))).contains("missing `query`"));
    assert!(err(s.execute("library.search", json!({"query": "x".repeat(300)}))).contains("longer than 256"));
    assert!(err(s.execute("library.search", json!({"query": "x", "limit": 0}))).contains("limit"));
    assert!(err(s.execute("library.search", json!({"query": "x", "folder": "/not/a/library/folder"}))).contains("not a Library folder"));
}

#[test]
fn thumbnails_are_small_pngs() {
    let d = pictures();
    let mut s = s();
    add(&mut s, &d);
    let logo = d.0.join("Logo.png");
    let t = s.execute("library.thumbnail", json!({"file": logo, "picture": 0, "width": 32})).unwrap();
    assert_eq!((t["width"].as_u64(), t["height"].as_u64()), (Some(32), Some(16)), "{t}");
    let png_bytes = crate::io::base64_decode(t["png"].as_str().unwrap()).unwrap();
    assert!(png_bytes.starts_with(&[0x89, b'P', b'N', b'G']));
    // The default width is wider than this picture: it isn't enlarged.
    let t = s.execute("library.thumbnail", json!({"file": logo, "picture": 0})).unwrap();
    assert_eq!(t["width"], 64);
    assert!(err(s.execute("library.thumbnail", json!({"file": logo, "picture": 1}))).contains("has 1 pictures"));
    assert!(err(s.execute("library.thumbnail", json!({"file": logo}))).contains("missing `picture`"));
    assert!(err(s.execute("library.thumbnail", json!({"file": logo, "picture": 0, "width": 4096}))).contains("32 to 512"));
    assert!(err(s.execute("library.thumbnail", json!({"file": d.0.join("notes.txt"), "picture": 0}))).contains("is not a"));
}

#[test]
fn inserting_a_picture_is_one_undo_step() {
    let d = pictures();
    let mut s = s();
    add(&mut s, &d);
    let undo = s.doc().unwrap().undo.len();
    let logo = d.0.join("Logo.png");
    let r = s.execute("library.insert", json!({"file": logo, "at": "C3"})).unwrap();
    assert_eq!(r["file"], logo.to_str().unwrap());
    assert!(r["image"].as_u64().is_some(), "{r}");
    let sh = s.doc().unwrap().wb.active().unwrap();
    let img = sh.images.first().unwrap();
    assert_eq!(img.data.as_slice(), std::fs::read(&logo).unwrap().as_slice(), "the file's own bytes");
    assert_eq!(img.mime, "image/png");
    assert_eq!(img.anchor.cell.a1(), "C3");
    assert_eq!((img.anchor.width, img.anchor.height), (64.0, 32.0));
    assert_eq!(s.doc().unwrap().undo.len(), undo + 1);
    assert_eq!(s.doc().unwrap().undo.last().unwrap().label, "Insert from Library");
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(images(&s), 0);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(images(&s), 1);

    // A JPEG keeps its type whatever `mime` says; a picture can go in a cell too.
    s.execute("library.insert", json!({"file": d.0.join("photo.JPG"), "picture": 0, "mime": "text/html"})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().images.last().unwrap().mime, "image/jpeg");
    let r = s.execute("library.insert", json!({"file": logo, "placement": "cell", "at": "E5"})).unwrap();
    assert_eq!(r["cell"], "E5", "{r}");
    assert_eq!(r["placement"], "cell");
}

#[test]
fn only_library_pictures_are_inserted() {
    let d = pictures();
    let elsewhere = Dir::new("elsewhere");
    let stray = elsewhere.file("stray.png", &png(8, 8));
    let mut s = s();
    // Before any folder is added, nothing is in the Library.
    assert!(err(s.execute("library.insert", json!({"file": d.0.join("Logo.png")}))).contains("not in a Library folder"));
    add(&mut s, &d);
    assert!(err(s.execute("library.insert", json!({"file": stray}))).contains("not in a Library folder"));
    assert!(err(s.execute("library.insert", json!({"file": d.0.join("notes.txt")}))).contains("is not a .png, .jpg, .jpeg file"));
    assert!(err(s.execute("library.insert", json!({"file": d.0.join("missing.png")}))).contains("missing.png"));
    // Never another file through Insert ▸ Pictures' own parameters.
    let logo = d.0.join("Logo.png");
    assert!(err(s.execute("library.insert", json!({"file": logo, "path": stray}))).contains("comes from `file`"));
    assert!(err(s.execute("library.insert", json!({"file": logo, "base64": "AAAA"}))).contains("comes from `file`"));
    assert!(err(s.execute("library.insert", json!({"file": logo, "picture": 1}))).contains("holds one picture"));
    assert!(err(s.execute("library.insert", json!({"file": logo, "placement": "floating"}))).contains("placement"));
    assert_eq!(images(&s), 0);
    assert!(s.doc().unwrap().undo.is_empty(), "nothing failed half-way");
}

#[test]
fn a_corrupt_or_oversized_file_fails_when_drawn_or_inserted() {
    let d = Dir::new("corrupt");
    d.file("good.png", &png(8, 8));
    d.file("text.png", b"this is text, not a picture");
    let mut cut = png(64, 64);
    cut.truncate(20);
    d.file("cut.png", &cut);
    d.file("renamed.jpg", &png(8, 8));
    // A real PNG header in a file past what Insert ▸ Pictures takes, and one past what the Library
    // reads at all (sparse: no disk is used).
    let big = d.file("big.png", &png(1, 1));
    std::fs::OpenOptions::new().write(true).open(&big).unwrap().set_len(65 << 20).unwrap();
    let huge = d.file("huge.png", &png(1, 1));
    std::fs::OpenOptions::new().write(true).open(&huge).unwrap().set_len(257 << 20).unwrap();
    let mut s = s();
    add(&mut s, &d);
    // Listing reads no file: each is listed from its name, one picture, except the one too large
    // for the Library to read at all.
    let files = list(&mut s);
    let by = |name: &str| files.iter().find(|f| f["file"].as_str().unwrap().ends_with(name)).unwrap().clone();
    for name in ["good.png", "text.png", "cut.png", "renamed.jpg", "big.png"] {
        assert_eq!(by(name)["pictureCount"], 1, "{}", by(name));
        assert!(by(name).get("error").is_none(), "{}", by(name));
    }
    assert!(by("huge.png")["error"].as_str().unwrap().contains("256 MB"), "{}", by("huge.png"));
    // Drawing or inserting reads the file, and says what is wrong with it.
    let thumb = |s: &mut Session, name: &str| err(s.execute("library.thumbnail", json!({"file": d.0.join(name), "picture": 0})));
    assert!(thumb(&mut s, "text.png").contains("not a PNG or JPEG"));
    assert!(!thumb(&mut s, "cut.png").is_empty());
    assert!(thumb(&mut s, "big.png").contains("larger than 64 MB"), "{}", thumb(&mut s, "big.png"));
    assert!(thumb(&mut s, "huge.png").contains("256 MB"));
    // A picture is known by its content: a PNG named .jpg is a PNG.
    s.execute("library.insert", json!({"file": d.0.join("renamed.jpg")})).unwrap();
    assert_eq!(s.doc().unwrap().wb.active().unwrap().images[0].mime, "image/png");
    // The broken ones don't insert, and leave the workbook as it was.
    for name in ["text.png", "cut.png", "big.png", "huge.png"] {
        assert!(!err(s.execute("library.insert", json!({"file": d.0.join(name)}))).is_empty());
    }
    assert_eq!(images(&s), 1);
}

/// Listing and search never open a file: one nobody may read is listed and found, and fails only
/// when it is drawn or inserted.
#[cfg(unix)]
#[test]
fn listing_and_search_read_no_file() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new("unreadable");
    let locked = d.file("locked-logo.png", &png(8, 8));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&locked).is_ok() {
        return; // running as a user who reads everything
    }
    let mut s = s();
    add(&mut s, &d);
    let files = list(&mut s);
    assert_eq!(files[0]["pictures"][0]["title"], "locked-logo.png", "{files:?}");
    assert!(files[0].get("error").is_none());
    let hits = s.execute("library.search", json!({"query": "logo"})).unwrap();
    assert_eq!(hits.as_array().map(Vec::len), Some(1), "{hits}");
    assert!(!err(s.execute("library.thumbnail", json!({"file": locked, "picture": 0}))).is_empty());
    assert!(!err(s.execute("library.insert", json!({"file": locked}))).is_empty());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
}

#[test]
fn a_changed_file_is_read_again() {
    let d = Dir::new("changed");
    let path = d.file("p.png", &png(10, 10));
    let mut s = s();
    add(&mut s, &d);
    assert_eq!(list(&mut s)[0]["pictures"][0]["title"], "p.png");
    let t = s.execute("library.thumbnail", json!({"file": path, "picture": 0, "width": 32})).unwrap();
    assert_eq!(t["width"], 10);
    // A different size (and so a different length) is a different file.
    std::fs::write(&path, png(20, 5)).unwrap();
    let t = s.execute("library.thumbnail", json!({"file": path, "picture": 0, "width": 32})).unwrap();
    assert_eq!((t["width"].as_u64(), t["height"].as_u64()), (Some(20), Some(5)));
}

#[test]
fn hostile_parameters_are_errors_not_crashes() {
    let d = pictures();
    let mut s = s();
    add(&mut s, &d);
    let long = "x".repeat(5_000);
    let logo = d.0.join("Logo.png");
    let odd = [
        json!(null),
        json!([]),
        json!([1, 2]),
        json!("text"),
        json!(5),
        json!({"path": long}),
        json!({"path": 7}),
        json!({"path": ""}),
        json!({"path": "\0"}),
        json!({"folder": long}),
        json!({"folder": 3}),
        json!({"file": long}),
        json!({"file": "\0.png"}),
        json!({"file": logo, "picture": -1}),
        json!({"file": logo, "picture": u64::MAX}),
        json!({"file": logo, "picture": 1e300}),
        json!({"file": logo, "picture": "0"}),
        json!({"file": logo, "picture": 0, "width": -5}),
        json!({"file": logo, "picture": 0, "width": u64::MAX}),
        json!({"query": long}),
        json!({"query": "a", "limit": u64::MAX}),
        json!({"query": ["a"]}),
        json!({"file": logo, "at": "ZZZZZ99999999"}),
        json!({"pictures": "yes"}),
    ];
    for id in
        ["library.folders", "library.addFolder", "library.removeFolder", "library.list", "library.search", "library.thumbnail", "library.insert"]
    {
        for p in &odd {
            let r = s.execute(id, p.clone());
            assert!(!matches!(r, Err(EngineError::Internal(..))), "{id} {p}: {r:?}");
        }
    }
    // Still the one folder: nothing hostile was added.
    assert_eq!(s.prefs.library_folders, [d.path()]);
}

#[test]
fn scripts_and_macros_cannot_reach_the_library() {
    let d = pictures();
    let mut s = s();
    add(&mut s, &d);
    {
        // A script or macro step runs inside the file sandbox: no folder or file is read.
        let _sandbox = crate::io::ScriptSandbox::enter();
        assert!(err(s.execute("library.list", json!({"folder": d.path()}))).contains("can't read or write files"));
        assert!(err(s.execute("library.insert", json!({"file": d.0.join("Logo.png")}))).contains("can't read or write files"));
        assert!(err(s.execute("library.addFolder", json!({"path": d.path()}))).contains("can't read or write files"));
    }
    // A stored script is refused before it runs.
    let other = Dir::new("script");
    s.execute("automate.newScript", json!({"name": "add", "commands": [{"command": "library.addFolder", "params": {"path": other.path()}}]}))
        .unwrap();
    assert!(err(s.execute("automate.runScript", json!({"name": "add"}))).contains("library.addFolder"));
    s.execute("automate.newScript", json!({"name": "rm", "commands": [{"command": "library.removeFolder", "params": {"path": d.path()}}]})).unwrap();
    assert!(s.execute("automate.runScript", json!({"name": "rm"})).is_err());
    assert_eq!(s.prefs.library_folders, [d.path()]);
    // And a macro step can't run one at all.
    let policy = crate::macros::policy::macro_policy("library.insert", &json!({}), crate::Trust::Local);
    assert!(matches!(policy, crate::macros::policy::Policy::Deny(_)), "{policy:?}");
}

#[test]
fn browsing_is_not_journaled_and_inserting_is() {
    let d = pictures();
    let mut s = s();
    let before = s.journal.len();
    add(&mut s, &d);
    let logo = d.0.join("Logo.png");
    s.execute("library.folders", json!({})).unwrap();
    s.execute("library.list", json!({})).unwrap();
    s.execute("library.search", json!({"query": "logo"})).unwrap();
    s.execute("library.thumbnail", json!({"file": logo, "picture": 0})).unwrap();
    assert_eq!(s.journal.len(), before, "{:?}", s.journal);
    assert!(s.doc().unwrap().undo.is_empty());
    s.execute("library.insert", json!({"file": logo})).unwrap();
    assert_eq!(s.journal.last().map(|(id, _)| id.as_str()), Some("library.insert"));
}

#[test]
fn folders_persist_with_the_preferences() {
    let d = pictures();
    let mut s = s();
    add(&mut s, &d);
    let saved = serde_json::to_value(&s.prefs).unwrap();
    assert_eq!(saved["libraryFolders"], json!([d.path()]));
    let back: crate::Prefs = serde_json::from_value(saved).unwrap();
    assert_eq!(back.library_folders, [d.path()]);
    // A prefs.json from before the Library has none; a hostile one loses only this list.
    let old: crate::Prefs = serde_json::from_value(json!({"r1c1": true})).unwrap();
    assert!(old.library_folders.is_empty());
    let hostile: crate::Prefs = serde_json::from_value(json!({"r1c1": true, "libraryFolders": {"a": 1}})).unwrap();
    assert!(hostile.r1c1 && hostile.library_folders.is_empty());
}

#[test]
fn an_unreadable_folder_says_why() {
    let gone = Dir::new("gone");
    let mut s = s();
    add(&mut s, &gone);
    let path = gone.0.clone();
    drop(gone);
    assert!(!Path::new(&path).exists());
    let folders = s.execute("library.folders", json!({})).unwrap();
    assert!(folders[0]["error"].as_str().is_some(), "{folders}");
    // Listing every folder skips it; asking for it by name says why.
    assert!(list(&mut s).is_empty());
    assert!(!err(s.execute("library.list", json!({"folder": path}))).is_empty());
}
