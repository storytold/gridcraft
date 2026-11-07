//! Script records and their folder: one JSON file per script under the application's settings
//! folder. An id is always `[a-z0-9-]`, so a name can never point the store outside its folder.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::syntax;
use crate::{MAX_ID_LEN, MAX_SCRIPTS};

/// One saved Office Script.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRecord {
    pub id: String,
    pub name: String,
    pub code: String,
    /// Milliseconds since the Unix epoch.
    pub created: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: u64,
    /// The `.osts` fields (everything but the body), when the script came from or has been
    /// exported to an Office Scripts file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub osts: Option<crate::osts::OstsMeta>,
}

impl ScriptRecord {
    /// A record for a new script.
    pub fn new(id: impl Into<String>, name: impl Into<String>, code: impl Into<String>, now: u64) -> Self {
        ScriptRecord { id: id.into(), name: name.into(), code: code.into(), created: now, modified: now, osts: None }
    }
}

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("the scripts folder is not available")]
    NoFolder,
    #[error("{0}")]
    InvalidId(String),
    #[error("there is no script with id `{0}`")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Syntax(#[from] crate::SyntaxError),
    #[error("the scripts folder cannot be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("a script file is damaged: {0}")]
    Damaged(String),
}

impl fmt::Debug for ScriptRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptRecord")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("code", &format_args!("{} chars", self.code.chars().count()))
            .field("created", &self.created)
            .field("modified", &self.modified)
            .finish()
    }
}

/// The folder scripts live in. Every path the store touches is `<root>/<id>.json` with an id
/// this module produced or validated, never a name or a caller's string as a path.
#[derive(Clone, Debug)]
pub struct ScriptStore {
    root: PathBuf,
}

impl ScriptStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        ScriptStore { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path_for(&self, id: &str) -> Result<PathBuf, StoreError> {
        if !valid_id(id) {
            return Err(StoreError::InvalidId(format!("`{id}` is not a script id")));
        }
        Ok(self.root.join(format!("{id}.json")))
    }

    /// Every readable script, by name. A damaged file is left out (and logged) rather than
    /// making the whole list fail: one bad file must not hide the person's other scripts.
    pub fn list(&self) -> Result<Vec<ScriptRecord>, StoreError> {
        let dir = match std::fs::read_dir(&self.root) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StoreError::Io(e)),
        };
        let mut out = Vec::new();
        for entry in dir {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Some(id) = path.file_stem().and_then(|s| s.to_str()) else { continue };
            if !valid_id(id) {
                log::warn!("ignoring script file with an invalid id: {}", path.display());
                continue;
            }
            match read_record(&path) {
                Ok(record) => out.push(record),
                Err(e) => log::warn!("ignoring damaged script file {}: {e}", path.display()),
            }
        }
        // `.osts` files put in the folder by Excel (or a synced OneDrive folder) are scripts
        // too: they appear in the list and run from there, without an import step. A record with
        // the same name or id wins, so saving a script never shadows itself twice.
        let mut seen_names: Vec<String> = out.iter().map(|r| r.name.to_lowercase()).collect();
        let mut seen_ids: Vec<String> = out.iter().map(|r| r.id.clone()).collect();
        if let Ok(dir) = std::fs::read_dir(&self.root) {
            for entry in dir.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("osts") {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
                let id = slug(stem);
                if !valid_id(&id) || seen_ids.iter().any(|x| x == &id) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else { continue };
                let Ok(file) = crate::osts::OstsFile::parse(&text) else { continue };
                let name = if file.description.trim().is_empty() { stem.to_string() } else { file.description.clone() };
                if seen_names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                    continue;
                }
                seen_names.push(name.to_lowercase());
                seen_ids.push(id.clone());
                out.push(ScriptRecord { id, name, code: file.code(), created: 0, modified: 0, osts: Some(crate::osts::OstsMeta::from_file(&file)) });
            }
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
        Ok(out)
    }

    pub fn load(&self, id: &str) -> Result<ScriptRecord, StoreError> {
        let path = self.path_for(id)?;
        if !path.exists() {
            // A `.osts` file in the folder (from Excel) is a script too; its id is its slug.
            return self.list()?.into_iter().find(|r| r.id == id).ok_or_else(|| StoreError::NotFound(id.to_string()));
        }
        read_record(&path)
    }

    /// Saves `code` under `name`: an existing script with that name (ignoring case) is replaced
    /// and keeps its id and creation time; otherwise a new id is made from the name.
    pub fn save(&self, name: &str, code: &str) -> Result<ScriptRecord, StoreError> {
        self.save_with(name, code, None)
    }

    /// Saves a script imported from an `.osts` file, keeping its fields. Saving over the same
    /// name keeps the file's metadata unless this import has its own.
    pub fn save_imported(&self, name: &str, code: &str, meta: crate::osts::OstsMeta) -> Result<ScriptRecord, StoreError> {
        self.save_with(name, code, Some(meta))
    }

    fn save_with(&self, name: &str, code: &str, meta: Option<crate::osts::OstsMeta>) -> Result<ScriptRecord, StoreError> {
        let name = syntax::check_name(name).map_err(StoreError::Invalid)?;
        syntax::check(code)?;
        let mut records = self.list()?;
        let now = now_millis();
        let existing = records.iter().position(|r| r.name.eq_ignore_ascii_case(name));
        let record = match existing {
            Some(i) => {
                let mut record = records.remove(i);
                record.name = name.to_string();
                record.code = code.to_string();
                record.modified = now;
                if meta.is_some() {
                    record.osts = meta.clone();
                }
                record
            }
            None => {
                if records.len() >= MAX_SCRIPTS {
                    return Err(StoreError::Invalid(format!("there can be at most {MAX_SCRIPTS} saved scripts")));
                }
                let mut record = ScriptRecord::new(self.unique_id(name, &records), name, code, now);
                record.osts = meta;
                record
            }
        };
        self.write(&record)?;
        Ok(record)
    }

    pub fn delete(&self, id: &str) -> Result<(), StoreError> {
        let path = self.path_for(id)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(StoreError::NotFound(id.to_string())),
            Err(e) => Err(StoreError::Io(e)),
        }
    }

    /// A name not in use: `base`, then `base 2`, `base 3`…
    pub fn free_name(&self, base: &str) -> Result<String, StoreError> {
        let records = self.list()?;
        let base = syntax::check_name(base).map_err(StoreError::Invalid)?.to_string();
        if !records.iter().any(|r| r.name.eq_ignore_ascii_case(&base)) {
            return Ok(base);
        }
        for n in 2..=MAX_SCRIPTS.saturating_add(1) {
            let candidate = format!("{base} {n}");
            if !records.iter().any(|r| r.name.eq_ignore_ascii_case(&candidate)) {
                return Ok(candidate);
            }
        }
        Err(StoreError::Invalid("there are too many scripts with that name".into()))
    }

    fn unique_id(&self, name: &str, records: &[ScriptRecord]) -> String {
        let base = slug(name);
        if !records.iter().any(|r| r.id == base) {
            return base;
        }
        for n in 2..=MAX_SCRIPTS.saturating_add(1) {
            let candidate = format!("{base}-{n}");
            if candidate.len() <= MAX_ID_LEN && !records.iter().any(|r| r.id == candidate) {
                return candidate;
            }
        }
        let fallback = format!("script-{}", now_millis());
        if fallback.len() <= MAX_ID_LEN && !records.iter().any(|r| r.id == fallback) {
            return fallback;
        }
        // Ids are short and the store is capped; two of this shape can only collide after a
        // pathological number of saves in one millisecond.
        format!("script-{}", now_millis().wrapping_add(records.len() as u64))
    }

    fn write(&self, record: &ScriptRecord) -> Result<(), StoreError> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.path_for(&record.id)?;
        let bytes = serde_json::to_vec_pretty(record).map_err(|e| StoreError::Damaged(e.to_string()))?;
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, &bytes)?;
        // Windows refuses to rename over an existing file; the script is small and this window
        // is short, and a crash between the two leaves the old file or none, never a partial one.
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        std::fs::rename(&temp, &path)?;
        Ok(())
    }
}

fn read_record(path: &Path) -> Result<ScriptRecord, StoreError> {
    let bytes = std::fs::read(path)?;
    let record: ScriptRecord = serde_json::from_slice(&bytes).map_err(|e| StoreError::Damaged(e.to_string()))?;
    if !valid_id(&record.id) {
        return Err(StoreError::Damaged(format!("`{}` is not a script id", record.id)));
    }
    Ok(record)
}

/// An id is `[a-z0-9-]`, starts and ends with a letter or digit, and is at most
/// [`MAX_ID_LEN`] bytes. This is what keeps `../` and absolute paths out of the store.
pub fn valid_id(id: &str) -> bool {
    if id.is_empty() || id.len() > MAX_ID_LEN {
        return false;
    }
    let bytes = id.as_bytes();
    let edge = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    edge(bytes[0]) && edge(bytes[bytes.len() - 1]) && bytes.iter().all(|b| edge(*b) || *b == b'-')
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(c);
        } else {
            dash = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() { "script".into() } else { trimmed.to_string() }
}

// web_time is std's clock on native and the JavaScript clock on wasm, so this call is safe
// everywhere; the lint that guards std's trapping wasm implementation does not apply.
#[allow(clippy::disallowed_methods)]
fn now_millis() -> u64 {
    web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis().min(u128::from(u64::MAX)) as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(tag: &str) -> ScriptStore {
        let root = std::env::temp_dir().join(format!("gridcraft-script-{}-{tag}-{}", std::process::id(), now_millis()));
        let _ = std::fs::remove_dir_all(&root);
        ScriptStore::new(root)
    }

    fn code(value: &str) -> String {
        format!("function main(workbook: ExcelScript.Workbook) {{\n  workbook.getActiveWorksheet().getRange(\"A1\").setValue({value});\n}}")
    }

    #[test]
    fn saves_lists_loads_and_deletes() {
        let s = store("basic");
        let a = s.save("Band", &code("1")).unwrap();
        assert_eq!(a.id, "band");
        assert_eq!(s.list().unwrap().len(), 1);
        assert_eq!(s.load("band").unwrap().code, code("1"));
        let b = s.save("Band", &code("2")).unwrap();
        assert_eq!(b.id, a.id);
        assert_eq!(b.created, a.created);
        assert!(b.modified >= a.modified);
        assert_eq!(s.load("band").unwrap().code, code("2"));
        assert_eq!(s.list().unwrap().len(), 1);
        s.delete(&b.id).unwrap();
        assert!(s.list().unwrap().is_empty());
        assert!(matches!(s.load("band"), Err(StoreError::NotFound(_))));
    }

    #[test]
    fn names_that_share_a_slug_get_distinct_ids() {
        let s = store("slug");
        let a = s.save("Band 1", &code("1")).unwrap();
        let b = s.save("Band-1", &code("2")).unwrap();
        let c = s.save("band 1", &code("3")).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(c.id, a.id, "a name that differs only in case replaces the script");
    }

    #[test]
    fn rejects_path_traversal_ids() {
        let s = store("traversal");
        for id in ["../x", "..", "a/b", "a\\b", "A", "", "-a", "a-", "a.b", &"a".repeat(MAX_ID_LEN + 1)] {
            assert!(!valid_id(id), "{id:?}");
            assert!(matches!(s.load(id), Err(StoreError::InvalidId(_))), "{id:?}");
            assert!(matches!(s.delete(id), Err(StoreError::InvalidId(_))), "{id:?}");
        }
        assert!(valid_id("a"));
        assert!(valid_id("a-1"));
        assert!(valid_id("2"));
    }

    #[test]
    fn a_damaged_file_does_not_hide_the_others() {
        let s = store("damaged");
        s.save("Good", &code("1")).unwrap();
        std::fs::write(s.root().join("bad.json"), b"{ not json").unwrap();
        std::fs::write(s.root().join("..json"), b"{}").unwrap();
        let list = s.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Good");
    }

    #[test]
    fn refuses_invalid_names_and_syntax() {
        let s = store("invalid");
        assert!(matches!(s.save("", &code("1")), Err(StoreError::Invalid(_))));
        assert!(matches!(s.save("X", "{"), Err(StoreError::Syntax(_))));
        assert!(matches!(s.save("X", ""), Err(StoreError::Syntax(_))));
    }

    #[test]
    fn free_name_counts_up() {
        let s = store("free");
        assert_eq!(s.free_name("Recorded Script").unwrap(), "Recorded Script");
        s.save("Recorded Script", &code("1")).unwrap();
        assert_eq!(s.free_name("Recorded Script").unwrap(), "Recorded Script 2");
        s.save("recorded script 2", &code("2")).unwrap();
        assert_eq!(s.free_name("Recorded Script").unwrap(), "Recorded Script 3");
    }

    #[test]
    fn an_osts_file_in_the_folder_is_a_script() {
        let s = store("osts");
        std::fs::create_dir_all(s.root()).unwrap();
        let text = serde_json::json!({
            "version": "0.3.0",
            "body": "function main(workbook: ExcelScript.Workbook) {\r\n}\r\n",
            "description": "From Excel",
            "noCodeMetadata": "",
            "parameterInfo": "{\"version\":1}",
            "apiInfo": "{\"variant\":\"synchronous\"}"
        })
        .to_string();
        std::fs::write(s.root().join("Excel Script.osts"), text).unwrap();
        let list = s.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "From Excel");
        assert_eq!(list[0].id, "excel-script");
        assert!(list[0].code.contains("function main"));
        assert!(s.load("excel-script").is_ok());
        // A GridCraft record with the same id wins (it is ours to manage).
        s.save("From Excel", &code("1")).unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
    }

    #[test]
    fn the_store_is_bounded() {
        let s = store("bounds");
        for n in 0..MAX_SCRIPTS {
            let record = ScriptRecord::new(format!("s{n}"), format!("S{n}"), code("1"), now_millis());
            s.write(&record).unwrap();
        }
        assert!(matches!(s.save("One more", &code("1")), Err(StoreError::Invalid(_))));
    }
}
