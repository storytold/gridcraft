//! The `.osts` file: Office Scripts' own JSON format (version 0.3.0), as Excel's Automate
//! editor stores it in OneDrive/SharePoint. GridCraft reads and writes it for transfer, and its
//! own store keeps each script in the same shape (plus a `gridcraft` field with its record
//! metadata), so a file taken from the store can be handed to Excel and one taken from Excel
//! can be kept.
//!
//! Nothing here runs code. The body is text; it is checked by [`crate::syntax`] and must define
//! `main`. Unknown top-level fields are kept and written back, so a file from a newer Excel
//! survives a round trip through GridCraft.

use serde_json::{Map, Value as Json, json};

use crate::syntax;
use crate::{MAX_CODE_CHARS, MAX_NAME_CHARS};

/// The format version GridCraft writes.
pub const FORMAT_VERSION: &str = "0.3.0";
/// Largest `.osts` accepted, in bytes. The code is capped at 100,000 characters; metadata and
/// unknown fields get room, and nothing larger is a script Excel would write.
pub const MAX_OSTS_BYTES: usize = 2 * 1024 * 1024;
/// The default `parameterInfo` for a script with no parameters, as a JSON string (the field is a
/// string holding JSON, not a nested object).
pub const DEFAULT_PARAMETER_INFO: &str =
    "{\"version\":1,\"originalParameterOrder\":[],\"parameterSchema\":{},\"returnSchema\":{},\"signature\":{\"comment\":\"\",\"parameters\":[]}}";
/// The `apiInfo` GridCraft honours: the synchronous ExcelScript API.
pub const SYNCHRONOUS_API_INFO: &str = "{\"variant\":\"synchronous\",\"variantVersion\":2}";

/// Why an `.osts` file could not be read.
#[derive(Debug, thiserror::Error)]
pub enum OstsError {
    #[error("the script file is too large: {0} bytes (at most {MAX_OSTS_BYTES})")]
    TooLarge(usize),
    #[error("the script file is not Office Scripts JSON: {0}")]
    Json(String),
    #[error("the script file has no `{0}` field")]
    Missing(&'static str),
    #[error("the script file is version {0}; GridCraft reads 0.x Office Scripts files")]
    Version(String),
    #[error("the script has no `main` function")]
    NoMain,
    #[error("{0}")]
    Syntax(#[from] crate::SyntaxError),
}

/// The Office Scripts JSON fields, with every unknown top-level field kept in [`OstsFile::extra`].
#[derive(Clone, Debug, PartialEq)]
pub struct OstsFile {
    pub version: String,
    /// The body exactly as the file holds it (CRLF line endings, as Excel writes them).
    pub body: String,
    pub description: String,
    pub no_code_metadata: String,
    /// A JSON string, not a nested object.
    pub parameter_info: String,
    /// A JSON string, not a nested object.
    pub api_info: String,
    /// Fields this crate does not know, written back unchanged.
    pub extra: Map<String, Json>,
}

impl Default for OstsFile {
    fn default() -> Self {
        OstsFile {
            version: FORMAT_VERSION.into(),
            body: String::new(),
            description: String::new(),
            no_code_metadata: String::new(),
            parameter_info: DEFAULT_PARAMETER_INFO.into(),
            api_info: SYNCHRONOUS_API_INFO.into(),
            extra: Map::new(),
        }
    }
}

impl OstsFile {
    /// A file for a script whose body is `code` (line endings become CRLF, as Office writes).
    pub fn for_script(name: &str, code: &str) -> OstsFile {
        OstsFile { body: to_crlf(code), description: name.to_string(), ..OstsFile::default() }
    }

    /// Parses a whole `.osts` file. `gridcraft` (this crate's own record metadata in its store)
    /// is left in [`OstsFile::extra`], so a store file can carry it and an exported file does not.
    pub fn parse(text: &str) -> Result<OstsFile, OstsError> {
        if text.len() > MAX_OSTS_BYTES {
            return Err(OstsError::TooLarge(text.len()));
        }
        let value: Json = serde_json::from_str(text).map_err(|e| OstsError::Json(e.to_string()))?;
        let Json::Object(mut object) = value else { return Err(OstsError::Json("the top level is not an object".into())) };
        let version = string_field(&mut object, "version")?.ok_or(OstsError::Missing("version"))?;
        if !version.starts_with("0.") {
            return Err(OstsError::Version(version));
        }
        let body = string_field(&mut object, "body")?.ok_or(OstsError::Missing("body"))?;
        if body.chars().count() > MAX_CODE_CHARS {
            return Err(OstsError::TooLarge(body.len()));
        }
        // Excel's editor always writes a main function; a file without one is not a script.
        if !has_main(&body) {
            return Err(OstsError::NoMain);
        }
        syntax::check(&body)?;
        Ok(OstsFile {
            version,
            body,
            description: string_field(&mut object, "description")?.unwrap_or_default(),
            no_code_metadata: string_field(&mut object, "noCodeMetadata")?.unwrap_or_default(),
            parameter_info: string_field(&mut object, "parameterInfo")?.unwrap_or_else(|| DEFAULT_PARAMETER_INFO.into()),
            api_info: string_field(&mut object, "apiInfo")?.unwrap_or_else(|| SYNCHRONOUS_API_INFO.into()),
            extra: object,
        })
    }

    /// The file as JSON (pretty, one object). Known fields are written in Office's order; the
    /// unknown ones follow unchanged.
    pub fn to_json(&self) -> String {
        let mut object = Map::new();
        object.insert("version".into(), Json::String(self.version.clone()));
        object.insert("body".into(), Json::String(self.body.clone()));
        object.insert("description".into(), Json::String(self.description.clone()));
        object.insert("noCodeMetadata".into(), Json::String(self.no_code_metadata.clone()));
        object.insert("parameterInfo".into(), Json::String(self.parameter_info.clone()));
        object.insert("apiInfo".into(), Json::String(self.api_info.clone()));
        for (key, value) in &self.extra {
            object.insert(key.clone(), value.clone());
        }
        serde_json::to_string_pretty(&Json::Object(object)).unwrap_or_else(|_| "{}".into())
    }

    /// The body as Rust/editor text: LF line endings.
    pub fn code(&self) -> String {
        self.body.replace("\r\n", "\n").replace('\r', "\n")
    }

    /// Replaces the body from editor text, keeping CRLF as the file format requires.
    pub fn set_code(&mut self, code: &str) {
        self.body = to_crlf(code);
    }

    /// The parsed `apiInfo` and whether GridCraft can honour it. Only the synchronous variant
    /// exists in the public API; a future asynchronous one is reported, not guessed at.
    pub fn api(&self) -> (Option<Json>, bool) {
        let parsed = serde_json::from_str::<Json>(&self.api_info).ok();
        let honoured =
            parsed.as_ref().and_then(|v| v.get("variant")).and_then(Json::as_str).is_none_or(|variant| variant.eq_ignore_ascii_case("synchronous"));
        (parsed, honoured)
    }

    /// Takes `gridcraft` (the record metadata) out, for a file exported to Excel.
    pub fn without_record(&self) -> OstsFile {
        let mut out = self.clone();
        out.extra.remove("gridcraft");
        out
    }

    /// The GridCraft record metadata stored in a file, when it has it.
    pub fn record_meta(&self) -> Option<RecordMeta> {
        let meta = self.extra.get("gridcraft")?.as_object()?;
        let id = meta.get("id").and_then(Json::as_str)?.to_string();
        let name = meta.get("name").and_then(Json::as_str)?.to_string();
        let created = meta.get("created").and_then(Json::as_u64).unwrap_or(0);
        let modified = meta.get("modified").and_then(Json::as_u64).unwrap_or(created);
        Some(RecordMeta { id, name, created, modified })
    }

    /// Sets the `gridcraft` metadata field (a store file carries it; an exported file drops it).
    pub fn set_record_meta(&mut self, meta: &RecordMeta) {
        self.extra.insert("gridcraft".into(), json!({"id": meta.id, "name": meta.name, "created": meta.created, "modified": meta.modified}));
    }
}

/// Everything in an `.osts` file except the body: kept with a stored script so an export writes
/// the fields it arrived with (including unknown ones), while the body follows the editor.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OstsMeta {
    pub version: String,
    pub description: String,
    pub no_code_metadata: String,
    pub parameter_info: String,
    pub api_info: String,
    pub extra: Map<String, Json>,
}

impl Default for OstsMeta {
    fn default() -> Self {
        let file = OstsFile::default();
        OstsMeta {
            version: file.version,
            description: file.description,
            no_code_metadata: file.no_code_metadata,
            parameter_info: file.parameter_info,
            api_info: file.api_info,
            extra: Map::new(),
        }
    }
}

impl OstsMeta {
    /// The metadata of a parsed file.
    pub fn from_file(file: &OstsFile) -> OstsMeta {
        OstsMeta {
            version: file.version.clone(),
            description: file.description.clone(),
            no_code_metadata: file.no_code_metadata.clone(),
            parameter_info: file.parameter_info.clone(),
            api_info: file.api_info.clone(),
            extra: file.extra.iter().filter(|(k, _)| k.as_str() != "gridcraft").map(|(k, v)| (k.clone(), v.clone())).collect(),
        }
    }

    /// The file for `code` with these fields (a fresh default file when there are none).
    pub fn file(&self, code: &str) -> OstsFile {
        OstsFile {
            version: if self.version.is_empty() { FORMAT_VERSION.into() } else { self.version.clone() },
            body: to_crlf(code),
            description: self.description.clone(),
            no_code_metadata: self.no_code_metadata.clone(),
            parameter_info: if self.parameter_info.is_empty() { DEFAULT_PARAMETER_INFO.into() } else { self.parameter_info.clone() },
            api_info: if self.api_info.is_empty() { SYNCHRONOUS_API_INFO.into() } else { self.api_info.clone() },
            extra: self.extra.clone(),
        }
    }
}

/// The record fields GridCraft keeps beside an `.osts` body in its own store.
#[derive(Clone, Debug, PartialEq)]
pub struct RecordMeta {
    pub id: String,
    pub name: String,
    pub created: u64,
    pub modified: u64,
}

/// CRLF line endings, as Office writes them.
pub fn to_crlf(code: &str) -> String {
    let mut out = String::with_capacity(code.len().saturating_add(code.len() / 20));
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\r\n");
            }
            '\n' => out.push_str("\r\n"),
            other => out.push(other),
        }
    }
    out
}

fn string_field(object: &mut Map<String, Json>, key: &str) -> Result<Option<String>, OstsError> {
    match object.remove(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s)),
        Some(_) => Err(OstsError::Json(format!("`{key}` is not text"))),
    }
}

/// Whether the body defines `main`, allowing whitespace and comments between the words.
fn has_main(body: &str) -> bool {
    let mut compact = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
            }
            c if c.is_whitespace() => {}
            c => compact.push(c),
        }
    }
    compact.contains("functionmain(") || compact.contains("main=(") || compact.contains("main=function")
}

/// Validates a user-visible name (the description) for a file; empty falls back to the file stem.
pub fn check_description(name: &str) -> Result<String, OstsError> {
    let trimmed = name.trim();
    if trimmed.chars().count() > MAX_NAME_CHARS {
        return Err(OstsError::Json(format!("a script name can be at most {MAX_NAME_CHARS} characters")));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(OstsError::Json("a script name can't hold control characters".into()));
    }
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "function main(workbook: ExcelScript.Workbook) {\r\n  workbook.getActiveWorksheet().getRange(\"A1\").setValue(1);\r\n}\r\n";

    fn file(extra: Json) -> String {
        let mut object = serde_json::Map::new();
        object.insert("version".into(), json!("0.3.0"));
        object.insert("body".into(), json!(BODY));
        object.insert("description".into(), json!("Test"));
        object.insert("noCodeMetadata".into(), json!(""));
        object.insert("parameterInfo".into(), json!(DEFAULT_PARAMETER_INFO));
        object.insert("apiInfo".into(), json!(SYNCHRONOUS_API_INFO));
        if let Json::Object(more) = extra {
            for (k, v) in more {
                object.insert(k, v);
            }
        }
        serde_json::to_string(&Json::Object(object)).unwrap()
    }

    #[test]
    fn a_file_round_trips_with_crlf_and_unknown_fields() {
        let text = file(json!({"futureField": {"nested": [1, 2]}}));
        let parsed = OstsFile::parse(&text).unwrap();
        assert_eq!(parsed.code(), BODY.replace("\r\n", "\n"));
        let out = parsed.to_json();
        let again = OstsFile::parse(&out).unwrap();
        assert_eq!(again, parsed);
        assert!(out.contains("futureField"));
        assert!(out.contains("\\r\\n"));
    }

    #[test]
    fn the_record_metadata_is_kept_in_the_store_and_stripped_on_export() {
        let mut parsed = OstsFile::parse(&file(json!({}))).unwrap();
        parsed.set_record_meta(&RecordMeta { id: "a".into(), name: "A".into(), created: 1, modified: 2 });
        let stored = parsed.to_json();
        let again = OstsFile::parse(&stored).unwrap();
        assert_eq!(again.record_meta().unwrap().id, "a");
        let exported = again.without_record();
        assert!(exported.record_meta().is_none());
        assert!(!exported.to_json().contains("gridcraft"));
    }

    #[test]
    fn edits_replace_the_body_with_crlf() {
        let mut parsed = OstsFile::parse(&file(json!({}))).unwrap();
        parsed.set_code("function main() {\n  console.log(1);\n}\n");
        assert!(parsed.body.contains("\r\n"));
        assert!(!parsed.code().contains('\r'));
    }

    #[test]
    fn malformed_inputs_are_structured_errors() {
        assert!(matches!(OstsFile::parse("not json"), Err(OstsError::Json(_))));
        assert!(matches!(OstsFile::parse("[]"), Err(OstsError::Json(_))));
        assert!(matches!(OstsFile::parse("{\"version\":\"0.3.0\"}"), Err(OstsError::Missing("body"))));
        assert!(matches!(OstsFile::parse(&file(json!({"version": "1.0"}))), Err(OstsError::Version(_))));
        let no_main = file(json!({"body": "const x = 1;"}));
        assert!(matches!(OstsFile::parse(&no_main), Err(OstsError::NoMain)));
        let broken = file(json!({"body": "function main() {"}));
        assert!(matches!(OstsFile::parse(&broken), Err(OstsError::Syntax(_))));
    }

    #[test]
    fn the_api_variant_is_reported() {
        let parsed = OstsFile::parse(&file(json!({}))).unwrap();
        assert!(parsed.api().1);
        let async_file = file(json!({"apiInfo": "{\"variant\":\"asynchronous\"}"}));
        assert!(!OstsFile::parse(&async_file).unwrap().api().1);
    }

    #[test]
    fn oversized_files_are_refused_before_parsing() {
        let huge = "x".repeat(MAX_OSTS_BYTES + 1);
        assert!(matches!(OstsFile::parse(&huge), Err(OstsError::TooLarge(_))));
    }

    #[test]
    fn crlf_conversion_never_doubles() {
        assert_eq!(to_crlf("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    }
}
