//! The installed faces indexed for lookup: by the names a workbook may use, and by the scripts
//! they cover (#1231).
//!
//! [`fonts`](crate::fonts) answers "which files are installed" and binds the ones a workbook needs;
//! this catalog is the query side — "which families can draw Thai", "which faces does `맑은 고딕`
//! name" — so font selection and fallback can work from coverage instead of trying families in
//! turn. It is a view over the same [`Face`]s, built once from the scan.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::fonts::{self, Face};
use crate::script::{ScriptClass, ScriptSet};

/// One installed face as the catalog keeps it: enough to name the file and to pick by weight,
/// slant and script without re-reading it.
#[derive(Clone, Debug, PartialEq)]
pub struct FontEntry {
    /// Typographic family name (the English one when the font has it).
    pub family: String,
    /// Every family name the face answers to, `family` first.
    pub names: Vec<String>,
    /// The file the face lives in.
    pub path: PathBuf,
    /// Face index within the file.
    pub index: u32,
    /// Weight from the face's style name (400 = regular).
    pub weight: f32,
    /// Whether the style reads as italic or oblique.
    pub italic: bool,
    /// Whether the face declares a serif style.
    pub serif: bool,
    /// The scripts the face has glyphs for.
    pub scripts: ScriptSet,
}

impl FontEntry {
    /// The entry for `face`.
    pub fn of(face: &Face) -> Self {
        Self {
            family: face.family.clone(),
            names: face.names.clone(),
            path: face.path.clone(),
            index: face.index,
            weight: face.weight,
            italic: face.italic,
            serif: face.serif,
            scripts: face.scripts,
        }
    }

    /// Whether `name` (any case) is one of the face's family names.
    pub fn answers_to(&self, name: &str) -> bool {
        self.names.iter().any(|n| n.eq_ignore_ascii_case(name))
    }
}

/// The installed faces, indexed by family name and by script. Build it from a scan with
/// [`FontCatalog::new`], or use [`FontCatalog::installed`] for the machine's catalog.
#[derive(Debug, Default)]
pub struct FontCatalog {
    entries: Vec<FontEntry>,
    /// Lowercased family name → the entries that answer to it, in file order.
    by_name: HashMap<String, Vec<usize>>,
    /// Script → the entries that cover it.
    by_script: HashMap<ScriptClass, Vec<usize>>,
}

impl FontCatalog {
    /// Index `faces`. Every family name of every face is a key, so a localized name finds the
    /// face its English name does.
    pub fn new(faces: &[Face]) -> Self {
        let mut catalog = Self::default();
        for face in faces {
            let mut entry = FontEntry::of(face);
            entry.names.dedup();
            let at = catalog.entries.len();
            for name in &entry.names {
                let key = name.to_lowercase();
                let list = catalog.by_name.entry(key).or_default();
                if list.last() != Some(&at) {
                    list.push(at);
                }
            }
            for script in entry.scripts.iter() {
                catalog.by_script.entry(script).or_default().push(at);
            }
            catalog.entries.push(entry);
        }
        catalog
    }

    /// The machine's catalog, built on first use from the same scan the font list uses. `None`
    /// while the scan is still running (or found nothing), so callers keep their old behaviour.
    pub fn installed() -> Option<&'static FontCatalog> {
        static INSTALLED: OnceLock<Option<FontCatalog>> = OnceLock::new();
        INSTALLED
            .get_or_init(|| {
                let faces = fonts::catalog();
                (!faces.is_empty()).then(|| FontCatalog::new(faces))
            })
            .as_ref()
    }

    /// Every face, in scan order.
    pub fn entries(&self) -> &[FontEntry] {
        &self.entries
    }

    /// Every family name in the catalog, sorted (each face's primary name).
    pub fn families(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.entries.iter().map(|e| e.family.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// The faces `name` (any case, any language) refers to, in file order.
    pub fn family(&self, name: &str) -> Vec<&FontEntry> {
        self.by_name.get(&name.to_lowercase()).map(|list| list.iter().filter_map(|at| self.entries.get(*at)).collect()).unwrap_or_default()
    }

    /// The families with a face that has glyphs for `script`, sorted and deduplicated. A family
    /// appears once however many of its faces cover the script.
    pub fn families_covering(&self, script: ScriptClass) -> Vec<&str> {
        let mut names: Vec<&str> = self
            .by_script
            .get(&script)
            .map(|list| list.iter().filter_map(|at| self.entries.get(*at)).map(|e| e.family.as_str()).collect())
            .unwrap_or_default();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Whether any installed face covers `script`.
    pub fn covers(&self, script: ScriptClass) -> bool {
        self.by_script.get(&script).is_some_and(|list| !list.is_empty())
    }

    /// The scripts the catalog covers in total.
    pub fn scripts(&self) -> ScriptSet {
        self.by_script.keys().copied().fold(ScriptSet::default(), ScriptSet::with)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(family: &str, names: &[&str], weight: f32, italic: bool, script: ScriptClass) -> Face {
        Face {
            family: family.into(),
            names: names.iter().map(|n| n.to_string()).collect(),
            path: PathBuf::from(format!("/fonts/{family}.ttf")),
            index: 0,
            weight,
            italic,
            serif: false,
            scripts: ScriptSet::default().with(script),
        }
    }

    #[test]
    fn a_localized_name_finds_the_same_faces() {
        let faces = [
            entry("Malgun Gothic", &["Malgun Gothic", "맑은 고딕"], 400.0, false, ScriptClass::Korean),
            entry("Malgun Gothic", &["Malgun Gothic", "맑은 고딕"], 700.0, false, ScriptClass::Korean),
        ];
        let catalog = FontCatalog::new(&faces);
        let korean = catalog.family("맑은 고딕");
        assert_eq!(korean.len(), 2);
        assert_eq!(korean, catalog.family("malgun gothic"));
        assert!(catalog.family("맑은고딕").is_empty(), "a name matches whole, not loosely");
    }

    #[test]
    fn families_are_listed_once_and_sorted() {
        let faces = [
            entry("Ubuntu", &["Ubuntu"], 400.0, false, ScriptClass::Latin),
            entry("Ubuntu", &["Ubuntu"], 400.0, true, ScriptClass::Latin),
            entry("Noto Sans Thai", &["Noto Sans Thai"], 400.0, false, ScriptClass::Thai),
        ];
        let catalog = FontCatalog::new(&faces);
        assert_eq!(catalog.families(), ["Noto Sans Thai", "Ubuntu"]);
        assert_eq!(catalog.entries().len(), 3);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_installed_catalog_lists_families_and_scripts() {
        let _guard = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(catalog) = FontCatalog::installed() else { return };
        assert!(catalog.entries().len() >= catalog.families().len());
        assert!(catalog.covers(ScriptClass::Latin), "every font stack has a Latin face");
        assert!(
            catalog.families_covering(ScriptClass::Latin).iter().all(|name| catalog.family(name).iter().any(|e| e.scripts.has(ScriptClass::Latin)))
        );
    }

    #[test]
    fn a_script_names_the_families_that_cover_it() {
        let faces = [
            entry("DejaVu Sans", &["DejaVu Sans"], 400.0, false, ScriptClass::Latin),
            entry("Noto Sans Thai", &["Noto Sans Thai"], 400.0, false, ScriptClass::Thai),
            entry("Noto Sans Thai", &["Noto Sans Thai"], 400.0, true, ScriptClass::Thai),
        ];
        let catalog = FontCatalog::new(&faces);
        assert_eq!(catalog.families_covering(ScriptClass::Thai), ["Noto Sans Thai"]);
        assert!(catalog.covers(ScriptClass::Latin));
        assert!(!catalog.covers(ScriptClass::Han), "no face draws Han");
        assert_eq!(catalog.scripts(), ScriptSet::default().with(ScriptClass::Latin).with(ScriptClass::Thai));
    }
}
