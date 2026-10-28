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

    /// The face that should serve `script` when a cell asks for it and the hand-written chains in
    /// [`crate::theme`] found nothing: the preferred family for the script when it is installed,
    /// else any family that covers it that does not read as an auxiliary design (symbol, emoji,
    /// monospace…). `serif` asks for a serif face and falls back to a sans one, the same way the
    /// serif chains do: a serif cell in a script with no serif design still wants that script.
    pub fn font_for_script(&self, script: ScriptClass, serif: bool) -> Option<&FontEntry> {
        for name in preferred_families(script, serif) {
            let faces = self.family(name);
            if let Some(entry) = pick(&faces, script, serif) {
                return Some(entry);
            }
        }
        let list = self.by_script.get(&script)?;
        let mut best: Option<&FontEntry> = None;
        for entry in list.iter().filter_map(|at| self.entries.get(*at)) {
            if script != ScriptClass::Symbols && !reads_as_text(&entry.family) {
                continue;
            }
            if best.is_none_or(|current| rank(entry, serif) < rank(current, serif)) {
                best = Some(entry);
            }
        }
        best
    }
}

/// The families the distributions ship for a script, most preferred first. Fontconfig names a
/// family by its typographic name, so a package that installs only `Noto Sans Thai` under a
/// different weight still matches by name here.
fn preferred_families(script: ScriptClass, serif: bool) -> &'static [&'static str] {
    match (script, serif) {
        (ScriptClass::Han, false) => &["Noto Sans CJK SC", "Noto Sans SC", "Source Han Sans SC", "Noto Sans CJK TC", "WenQuanYi Micro Hei"],
        (ScriptClass::Han, true) => &["Noto Serif CJK SC", "Source Han Serif SC", "Noto Serif CJK TC", "Noto Serif CJK JP"],
        (ScriptClass::Japanese, false) => &["Noto Sans CJK JP", "Noto Sans JP", "Source Han Sans JP"],
        (ScriptClass::Japanese, true) => &["Noto Serif CJK JP", "Source Han Serif JP"],
        (ScriptClass::Korean, false) => &["Noto Sans KR", "Noto Sans CJK KR", "Malgun Gothic", "NanumGothic"],
        (ScriptClass::Korean, true) => &["Noto Serif KR", "Noto Serif CJK KR", "NanumMyeongjo"],
        (ScriptClass::Thai, false) => &["Noto Sans Thai", "Loma", "Garuda"],
        (ScriptClass::Thai, true) => &["Noto Serif Thai"],
        (ScriptClass::Lao, false) => &["Noto Sans Lao", "Phetsarath OT"],
        (ScriptClass::Lao, true) => &["Noto Serif Lao"],
        (ScriptClass::Hebrew, false) => &["Noto Sans Hebrew", "DejaVu Sans", "Liberation Sans"],
        (ScriptClass::Hebrew, true) => &["Noto Serif Hebrew", "DejaVu Serif", "Liberation Serif"],
        (ScriptClass::Arabic, false) => &["Noto Sans Arabic", "Noto Naskh Arabic", "DejaVu Sans"],
        (ScriptClass::Arabic, true) => &["Noto Naskh Arabic", "Amiri", "Noto Serif Arabic"],
        (ScriptClass::Devanagari, false) => &["Noto Sans Devanagari", "Lohit Devanagari"],
        (ScriptClass::Devanagari, true) => &["Noto Serif Devanagari"],
        (ScriptClass::Khmer, false) => &["Noto Sans Khmer", "Khmer OS"],
        (ScriptClass::Khmer, true) => &["Noto Serif Khmer"],
        (ScriptClass::Myanmar, false) => &["Noto Sans Myanmar", "Myanmar Text"],
        (ScriptClass::Myanmar, true) => &["Noto Sans Myanmar", "Myanmar Text"],
        (ScriptClass::Tibetan, false) => &["Noto Serif Tibetan", "Noto Sans Tibetan", "Jomolhari"],
        (ScriptClass::Tibetan, true) => &["Noto Serif Tibetan", "Jomolhari"],
        (ScriptClass::Yi, false) => &["Noto Sans Yi", "Nuosu SIL"],
        (ScriptClass::Yi, true) => &["Noto Sans Yi", "Nuosu SIL"],
        (ScriptClass::Bengali, false) => &["Noto Sans Bengali", "Lohit Bengali", "Mukti Narrow"],
        (ScriptClass::Bengali, true) => &["Noto Serif Bengali", "Noto Sans Bengali", "Lohit Bengali"],
        (ScriptClass::Ethiopic, false) => &["Noto Sans Ethiopic", "Abyssinica SIL"],
        (ScriptClass::Ethiopic, true) => &["Noto Serif Ethiopic", "Noto Sans Ethiopic", "Abyssinica SIL"],
        (ScriptClass::Symbols, false) => &["Noto Sans Symbols 2", "Noto Sans Symbols", "DejaVu Sans"],
        (ScriptClass::Symbols, true) => &["Noto Sans Symbols 2", "DejaVu Sans"],
        (ScriptClass::Latin, false) => &["DejaVu Sans", "Liberation Sans", "Noto Sans"],
        (ScriptClass::Latin, true) => &["DejaVu Serif", "Liberation Serif", "Noto Serif"],
        (ScriptClass::Common, _) => &[],
    }
}

/// The best of `faces` for `script`: it must cover the script, and among those the upright
/// regular face wins, the requested serif design before a sans one.
fn pick<'a>(faces: &[&'a FontEntry], script: ScriptClass, serif: bool) -> Option<&'a FontEntry> {
    faces.iter().copied().filter(|entry| entry.scripts.has(script)).min_by(|a, b| rank(a, serif).cmp(&rank(b, serif)))
}

/// How suitable `entry` is as the face for a script, smaller is better: a serif answer for a sans
/// request (or the other way round) first, then slant, then distance from regular, then the name
/// so the choice does not depend on the order fontconfig listed the files.
fn rank(entry: &FontEntry, serif: bool) -> (u8, u8, u32, &str) {
    let weight = (entry.weight - 400.0).abs();
    (u8::from(entry.serif != serif), u8::from(entry.italic), weight as u32, entry.family.as_str())
}

/// Whether a family name reads as a design made for text in a script, rather than an auxiliary one
/// that happens to cover a few of its characters.
fn reads_as_text(family: &str) -> bool {
    const AUXILIARY: [&str; 8] = ["mono", "symbol", "emoji", "dingbat", "icon", "webding", "wingding", "math"];
    let lower = family.to_lowercase();
    !AUXILIARY.iter().any(|word| lower.contains(word))
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
    fn the_preferred_family_wins_over_another_that_covers_the_script() {
        let faces = [
            entry("Zzz Thai Display", &["Zzz Thai Display"], 400.0, false, ScriptClass::Thai),
            entry("Noto Sans Thai", &["Noto Sans Thai"], 400.0, false, ScriptClass::Thai),
        ];
        let catalog = FontCatalog::new(&faces);
        assert_eq!(catalog.font_for_script(ScriptClass::Thai, false).map(|e| e.family.as_str()), Some("Noto Sans Thai"));
    }

    #[test]
    fn bengali_and_ethiopic_name_their_own_families() {
        let faces = [
            entry("Some Fallback", &["Some Fallback"], 400.0, false, ScriptClass::Bengali),
            entry("Noto Sans Bengali", &["Noto Sans Bengali"], 400.0, false, ScriptClass::Bengali),
            entry("Noto Sans Ethiopic", &["Noto Sans Ethiopic"], 400.0, false, ScriptClass::Ethiopic),
        ];
        let catalog = FontCatalog::new(&faces);
        assert_eq!(catalog.font_for_script(ScriptClass::Bengali, false).map(|e| e.family.as_str()), Some("Noto Sans Bengali"));
        assert_eq!(catalog.font_for_script(ScriptClass::Ethiopic, false).map(|e| e.family.as_str()), Some("Noto Sans Ethiopic"));
    }

    #[test]
    fn a_script_without_its_preferred_family_uses_any_covering_one() {
        let faces = [entry("Zzz Thai Display", &["Zzz Thai Display"], 400.0, false, ScriptClass::Thai)];
        let catalog = FontCatalog::new(&faces);
        assert_eq!(catalog.font_for_script(ScriptClass::Thai, false).map(|e| e.family.as_str()), Some("Zzz Thai Display"));
        assert!(catalog.font_for_script(ScriptClass::Lao, false).is_none(), "nothing draws Lao");
    }

    #[test]
    fn a_family_that_lacks_the_script_is_not_offered() {
        let faces = [entry("Noto Sans Thai", &["Noto Sans Thai"], 400.0, false, ScriptClass::Latin)];
        let catalog = FontCatalog::new(&faces);
        assert!(catalog.font_for_script(ScriptClass::Thai, false).is_none());
    }

    #[test]
    fn a_serif_request_prefers_the_serif_face() {
        let mut sans = entry("Zzz Thai", &["Zzz Thai"], 400.0, false, ScriptClass::Thai);
        let mut serif = entry("Zzz Thai", &["Zzz Thai"], 400.0, false, ScriptClass::Thai);
        serif.serif = true;
        sans.serif = false;
        let catalog = FontCatalog::new(&[sans, serif]);
        assert!(catalog.font_for_script(ScriptClass::Thai, true).is_some_and(|e| e.serif));
        assert!(catalog.font_for_script(ScriptClass::Thai, false).is_some_and(|e| !e.serif));
    }

    #[test]
    fn an_auxiliary_family_is_not_used_for_text() {
        let mut face = entry("Noto Sans Symbols 2", &["Noto Sans Symbols 2"], 400.0, false, ScriptClass::Thai);
        face.scripts = face.scripts.with(ScriptClass::Symbols);
        let catalog = FontCatalog::new(&[face]);
        assert!(catalog.font_for_script(ScriptClass::Thai, false).is_none());
        assert!(catalog.font_for_script(ScriptClass::Symbols, false).is_some(), "symbols may have a symbol face");
    }

    #[test]
    fn the_regular_upright_face_wins_within_a_family() {
        let faces = [
            entry("Noto Sans Thai", &["Noto Sans Thai"], 700.0, false, ScriptClass::Thai),
            entry("Noto Sans Thai", &["Noto Sans Thai"], 300.0, true, ScriptClass::Thai),
            entry("Noto Sans Thai", &["Noto Sans Thai"], 400.0, false, ScriptClass::Thai),
        ];
        let catalog = FontCatalog::new(&faces);
        let picked = catalog.font_for_script(ScriptClass::Thai, false).expect("Thai is covered");
        assert_eq!((picked.weight, picked.italic), (400.0, false));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_installed_catalog_answers_for_the_scripts_it_covers() {
        let _guard = crate::fonts::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(catalog) = FontCatalog::installed() else { return };
        for script in ScriptClass::ALL {
            let Some(entry) = catalog.font_for_script(script, false) else { continue };
            assert!(entry.scripts.has(script), "{} was offered for {:?}", entry.family, script);
        }
        if catalog.covers(ScriptClass::Thai) {
            assert!(catalog.font_for_script(ScriptClass::Thai, false).is_some());
        }
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
