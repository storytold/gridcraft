//! Insert ▸ Library: pictures from the user's own folders, browsed in a side pane while the
//! workbook stays open for editing.
//!
//! The folders are a preference ([`crate::Prefs::library_folders`]); `library.addFolder` and
//! `library.removeFolder` change it, and the front end saves it with the other preferences.
//! Listing, searching and thumbnails only read: no undo step, no journal entry. `library.insert`
//! puts a picture on the sheet as `insert.picture` does, in one undo step. The answers are the
//! Library core's the Craft apps share ([`craft_library`]), with GridCraft's nouns (`picture`,
//! `pictures`, `pictureCount`). Every folder and file goes through [`crate::io`], so a script or
//! macro step can't reach one, and every bound is in [`crate::library`]. The browser build has no
//! folders to read: there every `library.*` command says so.

use serde_json::{Value as Json, json};

use super::{CommandSpec, always, bad, cmd, has_doc};
use crate::library::{self, Confined};
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "library.folders",
            "Library Folders",
            [],
            None,
            "{} → [{path, files, entries, truncated?, skippedDeep?, skippedLinks?, unreadable?, error?}]: the folders the Library browses and how many pictures each holds (an unreadable folder carries its `error`)",
            always,
            folders
        ),
        cmd!(
            query "library.addFolder",
            "Add Library Folder",
            ["Insert", "Library"],
            None,
            "{path} → {path, added, folders}: browse a folder of pictures (PNG, JPEG) in the Library, kept with the preferences; at most 32 folders",
            always,
            add_folder
        ),
        cmd!(
            query "library.removeFolder",
            "Remove Library Folder",
            ["Insert", "Library"],
            None,
            "{path} → {path, folders}: stop browsing a Library folder (its files are not touched)",
            always,
            remove_folder
        ),
        cmd!(
            query "library.list",
            "List Library",
            [],
            None,
            "{folder?, file?, pictures?: bool (ignored: no file is read to list it)} → [{file, folder, title, pictures: [{index, title, hidden}], pictureCount, truncated?, error?}]: the picture files in the Library folders (or one folder, or one file), one picture each, known from the file name; a file over 256 MB is listed with its `error`, and one that isn't a picture fails when it is drawn or inserted",
            always,
            list
        ),
        cmd!(
            query "library.search",
            "Search Library",
            [],
            None,
            "{query, limit?: 1..=500 (default 100), folder?} → [{file, picture, title, match: title|text|file}]: pictures whose file name or format (PNG, JPEG) holds `query`, any case; no file is read",
            always,
            search
        ),
        cmd!(
            query "library.thumbnail",
            "Library Thumbnail",
            [],
            None,
            "{file, picture: 0, width?: px 32..=512 (default 160)} → {png: base64, width, height}: a Library picture drawn small",
            always,
            thumbnail
        ),
        cmd!(
            "library.insert",
            "Insert from Library",
            ["Insert", "Library"],
            None,
            "{file, picture?: 0, placement?: cell|overCells (default overCells), at?: \"B2\", width?, height?, alt?, mode?: moveOnly|moveAndSize|absolute} → {file, image} or {file, cell, placement}: put a Library picture on the sheet as Insert ▸ Pictures does (one undo step)",
            has_doc,
            insert
        ),
    ]
}

/// Every `library.*` command reads folders on this computer, which the browser build can't.
fn native(cmd: &str) -> Result<()> {
    if cfg!(target_arch = "wasm32") { Err(bad(cmd, library::NOT_IN_BROWSER)) } else { Ok(()) }
}

fn folders(s: &mut Session, _p: &Json) -> Result<Json> {
    native("library.folders")?;
    Ok(s.library.folders(&Confined, &s.prefs.library_folders))
}

fn add_folder(s: &mut Session, p: &Json) -> Result<Json> {
    native("library.addFolder")?;
    library::Cache::add_folder(&Confined, &mut s.prefs.library_folders, p).map_err(|e| bad("library.addFolder", e))
}

fn remove_folder(s: &mut Session, p: &Json) -> Result<Json> {
    native("library.removeFolder")?;
    library::Cache::remove_folder(&Confined, &mut s.prefs.library_folders, p).map_err(|e| bad("library.removeFolder", e))
}

fn list(s: &mut Session, p: &Json) -> Result<Json> {
    native("library.list")?;
    s.library.list(&Confined, &s.prefs.library_folders, p).map_err(|e| bad("library.list", e))
}

fn search(s: &mut Session, p: &Json) -> Result<Json> {
    native("library.search")?;
    s.library.search(&Confined, &s.prefs.library_folders, p).map_err(|e| bad("library.search", e))
}

fn thumbnail(s: &mut Session, p: &Json) -> Result<Json> {
    native("library.thumbnail")?;
    s.library.thumbnail(&Confined, &s.prefs.library_folders, p).map_err(|e| bad("library.thumbnail", e))
}

/// `library.insert`: the picture file `file` names, which must be in a Library folder, put on the
/// active sheet through Insert ▸ Pictures' own code, from the bytes the Library read.
fn insert(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "library.insert";
    native(CMD)?;
    let one = library::NOUNS.one;
    // A picture file holds one picture, so the index may be left out.
    let index = match p.get(one) {
        None | Some(Json::Null) => 0,
        Some(_) => s.library.item_param(p).map_err(|e| bad(CMD, e))?,
    };
    if index != 0 {
        return Err(bad(CMD, format!("a picture file holds one picture: `{one}` is 0, not {index}")));
    }
    // Only what Insert ▸ Pictures takes from its own parameters: never a `path` or `base64` of
    // another file.
    if p.get("path").is_some() || p.get("base64").is_some() {
        return Err(bad(CMD, "the picture comes from `file`, a Library file; Insert ▸ Pictures (insert.picture) takes a `path`"));
    }
    let (path, stamp, _) = s.library.file(&Confined, &s.prefs.library_folders, p).map_err(|e| bad(CMD, e))?;
    let picture = s.library.doc(&Confined, &path, stamp).map_err(|e| bad(CMD, e))?;
    // The type is the one the Library read from the picture itself.
    let mut p = p.clone();
    if let Some(o) = p.as_object_mut() {
        o.insert("mime".into(), json!(picture.mime));
    }
    let mut placed = super::insert::place_picture(s, &p, picture.bytes.clone(), CMD)?;
    if let Some(o) = placed.as_object_mut() {
        o.insert("file".into(), json!(path.to_string_lossy()));
    }
    Ok(placed)
}

#[cfg(test)]
mod tests;
