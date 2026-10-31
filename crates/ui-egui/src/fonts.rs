//! The fonts installed on the machine, so the font list can offer them and cells can render with
//! them (#4).
//!
//! egui has no font discovery of its own and panics when asked for a family that was never bound
//! to font data. So we do both halves: scan the platform's font folders for family names (a read
//! of each font file's `name` table — damaged or unreadable files yield nothing, never a panic),
//! and bind the files a workbook actually uses into the families
//! [`theme::font_definitions`](crate::theme::font_definitions) assembles.
//!
//! Native only: on wasm there are no system fonts, every function here reports "nothing", and the
//! font list is the standard [`crate::ribbon::FONTS`] set.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A font file is skipped by the scan past this size: a catalog only needs each face's name, and a
/// damaged or enormous file must not turn startup into a long read.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FONT_FILE: u64 = 64 << 20;

/// A face keeps at most this many family names, so a hostile `name` table can't grow the catalog.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FAMILY_NAMES: usize = 16;

/// One face of one installed font file.
#[derive(Clone, Debug)]
pub struct Face {
    /// Typographic family name, e.g. `"Segoe UI"` (the English one when the font has it): what the
    /// font list shows.
    pub family: String,
    /// Every family name the face carries, `family` first, then each language's typographic and
    /// legacy family names (`"Malgun Gothic"`, `"맑은 고딕"`). A workbook may name the font by any of
    /// them: Korean Excel saves `맑은 고딕` (#1898).
    pub names: Vec<String>,
    /// The file the face lives in.
    pub path: PathBuf,
    /// Face index within the file (collections hold several, e.g. a family's Regular and Bold).
    pub index: u32,
    /// Weight implied by the face's style name (400 = regular).
    pub weight: f32,
    /// Whether the style name reads as italic or oblique.
    pub italic: bool,
}

/// The font folders of the platform (the system's and the user's). Empty on wasm.
pub fn system_font_dirs() -> Vec<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        Vec::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        if cfg!(target_os = "macos") {
            dirs.extend(["/System/Library/Fonts", "/System/Library/Fonts/Supplemental", "/Library/Fonts"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join("Library/Fonts"));
            }
        } else if cfg!(windows) {
            let root = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
            dirs.push(root.join("Fonts"));
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
            }
        } else {
            dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/TTF"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join(".fonts"));
                dirs.push(h.join(".local/share/fonts"));
            }
        }
        dirs
    }
}

/// Weight implied by a style name (`"Bold"` → 700, `"Light"` → 300).
#[cfg(not(target_arch = "wasm32"))]
fn style_weight(style: &str) -> f32 {
    let s: String = style.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    const TABLE: &[(&str, f32)] = &[
        ("hairline", 100.0),
        ("thin", 100.0),
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("light", 300.0),
        ("medium", 500.0),
        ("demibold", 600.0),
        ("semibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, w)| *w).unwrap_or(400.0)
}

impl Face {
    /// Whether the face goes by `name` in any language.
    pub fn answers_to(&self, name: &str) -> bool {
        self.names.iter().any(|n| n.eq_ignore_ascii_case(name))
    }
}

/// Whether a style name reads as italic or oblique.
#[cfg(not(target_arch = "wasm32"))]
fn style_italic(style: &str) -> bool {
    let s: String = style.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    s.contains("italic") || s.contains("oblique")
}

/// Every face in `data` as `(index, family, names, style)`, where `names` holds every family name
/// (see [`Face::names`]). Damaged data yields nothing, never a panic.
#[cfg(not(target_arch = "wasm32"))]
fn file_faces(data: &[u8]) -> Vec<(u32, String, Vec<String>, String)> {
    use skrifa::raw::FileRef;
    use skrifa::{MetadataProvider, string::StringId};
    let count = match FileRef::new(data) {
        Ok(FileRef::Font(_)) => 1,
        Ok(FileRef::Collection(c)) => c.len(),
        Err(_) => return Vec::new(),
    };
    let name = |f: &skrifa::FontRef<'_>, ids: &[StringId]| -> Option<String> {
        ids.iter().find_map(|id| f.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
    };
    let family_ids = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME];
    (0..count)
        .filter_map(|i| {
            let f = skrifa::FontRef::from_index(data, i).ok()?;
            let family = name(&f, &family_ids)?;
            let mut names = vec![family.clone()];
            for s in family_ids.iter().flat_map(|id| f.localized_strings(*id)) {
                if names.len() >= MAX_FAMILY_NAMES {
                    break;
                }
                let s = s.to_string();
                if !s.is_empty() && !names.iter().any(|n| n.eq_ignore_ascii_case(&s)) {
                    names.push(s);
                }
            }
            let style = name(&f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
            Some((i, family, names, style))
        })
        .collect()
}

/// Catalog every face under `dirs`, recursing into subfolders. A folder reached twice (through a
/// symlink) is read once, so a link back to a parent ends the walk.
#[cfg(not(target_arch = "wasm32"))]
pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<Face> {
    let mut out: Vec<Face> = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    while let Some(dir) = stack.pop() {
        let key = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !seen_dirs.insert(key) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_font = path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"));
            if !is_font || std::fs::metadata(&path).map(|m| m.len() > MAX_FONT_FILE).unwrap_or(true) {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else { continue };
            for (index, family, names, style) in file_faces(&data) {
                out.push(Face { family, names, path: path.clone(), index, weight: style_weight(&style), italic: style_italic(&style) });
            }
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
pub fn scan_dirs(_dirs: &[PathBuf]) -> Vec<Face> {
    Vec::new()
}

static CATALOG: OnceLock<Vec<Face>> = OnceLock::new();

/// The catalog of installed faces, scanned on first use. Blocks until the scan finishes.
pub fn catalog() -> &'static [Face] {
    CATALOG.get_or_init(|| scan_dirs(&system_font_dirs()))
}

/// The catalog if the scan has finished, else `None` — never blocks, for use while painting.
fn catalog_now() -> Option<&'static [Face]> {
    CATALOG.get().map(Vec::as_slice)
}

/// Start cataloging the installed fonts on a background thread, so the first frame doesn't wait for
/// it. A no-op on wasm and after the scan has started.
pub fn start_scan() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        if CATALOG.get().is_none() {
            // A failed spawn leaves the scan to the first blocking lookup.
            let _ = std::thread::Builder::new().name("font-scan".into()).spawn(|| {
                catalog();
            });
        }
    });
}

/// Whether the background scan has finished.
pub fn scan_done() -> bool {
    CATALOG.get().is_some()
}

/// The faces of one installed family, by any of its names, regular-weight first (the order roles
/// pick from).
pub fn faces_of(family: &str) -> Vec<Face> {
    catalog_now().map(|cat| faces_named(cat, family)).unwrap_or_default()
}

/// The faces in `cat` that go by `wanted`. When two families claim one name the winner is fixed,
/// not left to the scan's directory order: a family whose own (English) name it is wins, else the
/// family of the first claiming face by path and index. Only the faces that carry the name are
/// kept, so a legacy name (`"Arial Black"`, name ID 1) picks its own face out of its family.
fn faces_named(cat: &[Face], wanted: &str) -> Vec<Face> {
    let mut v: Vec<Face> = if cat.iter().any(|f| f.family.eq_ignore_ascii_case(wanted)) {
        cat.iter().filter(|f| f.family.eq_ignore_ascii_case(wanted)).cloned().collect()
    } else {
        let owner = cat.iter().filter(|f| f.answers_to(wanted)).min_by(|a, b| a.path.cmp(&b.path).then(a.index.cmp(&b.index)));
        match owner {
            Some(o) => cat.iter().filter(|f| f.family.eq_ignore_ascii_case(&o.family) && f.answers_to(wanted)).cloned().collect(),
            None => Vec::new(),
        }
    };
    v.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)).then(a.index.cmp(&b.index)));
    v
}

/// The subset of `names` that is installed, sorted and deduped. These are the families whose files
/// get bound for a workbook, under the name the workbook uses: that is the name
/// [`cell_family`](crate::theme::cell_family) looks up, so `맑은 고딕` binds as `맑은 고딕`, not as
/// the catalog's `Malgun Gothic`.
pub fn installed_among(names: &[String]) -> Vec<String> {
    catalog_now().map(|cat| installed_in(cat, names)).unwrap_or_default()
}

/// [`installed_among`] over a given catalog. Names dedupe exactly, not case-folded: a role key is
/// built from the cell's exact string, so `Calibri` and `calibri` each need their own binding (the
/// font data behind them is shared, see [`face_key`]).
fn installed_in(cat: &[Face], names: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for wanted in names {
        if seen.insert(wanted.as_str()) && cat.iter().any(|f| f.answers_to(wanted)) {
            out.push(wanted.clone());
        }
    }
    out.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
    out
}

/// The font list a combo box shows: the standard [`crate::ribbon::FONTS`] names first (what Excel
/// offers, and what we substitute), then every other installed family A–Z. Never blocks: until the
/// background scan finishes it returns just the standard names.
pub fn families() -> Vec<String> {
    let mut out: Vec<String> = crate::ribbon::FONTS.iter().map(|s| s.to_string()).collect();
    let Some(cat) = catalog_now() else { return out };
    let mut seen: HashSet<String> = out.iter().map(|s| s.to_lowercase()).collect();
    let mut installed: Vec<String> = cat.iter().map(|f| f.family.clone()).collect();
    installed.sort_by_key(|a| a.to_lowercase());
    installed.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    for f in installed {
        if seen.insert(f.to_lowercase()) {
            out.push(f);
        }
    }
    out
}

/// The egui family name for one role of one family: `fnt:{family}:{b|n}{i|n}`.
pub fn role_key(family: &str, bold: bool, italic: bool) -> String {
    format!("fnt:{family}:{}{}", if bold { 'b' } else { 'n' }, if italic { 'i' } else { 'n' })
}

/// The face to use for a role, or `None` when the family has no faces. Missing bold/italic faces
/// fall back to the family's regular face, so every bound role has data behind it.
fn pick_face(faces: &[Face], bold: bool, italic: bool) -> Option<&Face> {
    let regular = || faces.iter().find(|f| !f.italic && (f.weight - 400.0).abs() < 1.0);
    let exact = |wanted_bold: bool, wanted_italic: bool| {
        faces.iter().find(|f| f.italic == wanted_italic && if wanted_bold { f.weight >= 600.0 } else { f.weight < 600.0 })
    };
    (match (bold, italic) {
        (true, true) => exact(true, true).or_else(|| exact(true, false)).or_else(|| exact(false, true)),
        (true, false) => exact(true, false),
        (false, true) => exact(false, true),
        (false, false) => None,
    })
    .or_else(regular)
    .or_else(|| faces.first())
}

/// A stable key for a face's font data, one per file and index: every name a family is bound under
/// (`Malgun Gothic`, `맑은 고딕`) shares the data instead of loading the file again.
fn face_key(face: &Face) -> String {
    format!("fnt-src:{}:{}", face.path.display(), face.index)
}

/// Bind one installed family's faces into `fonts` under `family` (the name the workbook uses), one
/// egui family per cell role, and return the
/// role keys that are now bound. A role with no exact face chains to the family's regular face, so
/// no role is ever left pointing at missing data (egui panics on that). A family with no faces
/// binds nothing.
///
/// Each role's chain ends with egui's default proportional fonts, so glyphs the installed font
/// lacks (emoji, a stray script) still render.
pub fn bind_faces(fonts: &mut egui::FontDefinitions, family: &str, faces: &[Face]) -> Vec<String> {
    if faces.is_empty() {
        return Vec::new();
    }
    let base: Vec<String> = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    let mut bound: Vec<String> = Vec::new();
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let Some(primary) = pick_face(faces, bold, italic) else { continue };
        let mut chain: Vec<String> = Vec::new();
        for face in [Some(primary), pick_face(faces, false, false)] {
            let Some(face) = face else { continue };
            let key = face_key(face);
            if chain.contains(&key) {
                continue;
            }
            if !fonts.font_data.contains_key(&key) {
                let Ok(bytes) = std::fs::read(&face.path) else { continue };
                let mut fd = egui::FontData::from_owned(bytes);
                fd.index = face.index;
                fonts.font_data.insert(key.clone(), std::sync::Arc::new(fd));
            }
            chain.push(key);
        }
        if chain.is_empty() {
            continue;
        }
        chain.extend(base.iter().cloned());
        let role = role_key(family, bold, italic);
        fonts.families.insert(egui::FontFamily::Name(role.clone().into()), chain);
        bound.push(role);
    }
    bound
}

/// Bind every installed face of `family`, found by any of its names (see [`bind_faces`]).
pub fn bind_family(fonts: &mut egui::FontDefinitions, family: &str) -> Vec<String> {
    bind_faces(fonts, family, &faces_of(family))
}

/// The role families currently bound to font data. `cell_family` consults this, so it never names a
/// family egui hasn't been given.
static BOUND: OnceLock<std::sync::RwLock<HashSet<String>>> = OnceLock::new();

fn bound() -> &'static std::sync::RwLock<HashSet<String>> {
    BOUND.get_or_init(|| std::sync::RwLock::new(HashSet::new()))
}

/// Record the role families a freshly built [`egui::FontDefinitions`] bound.
pub fn set_bound(keys: impl IntoIterator<Item = String>) {
    *bound().write().unwrap_or_else(std::sync::PoisonError::into_inner) = keys.into_iter().collect();
}

/// Is this exact role family bound to font data?
pub fn is_bound(role: &str) -> bool {
    bound().read().unwrap_or_else(std::sync::PoisonError::into_inner).contains(role)
}

/// Serializes tests that mutate the process-wide bound-family registry.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_names_map_to_weight_and_slant() {
        assert_eq!(style_weight("Bold"), 700.0);
        assert_eq!(style_weight("SemiBold Italic"), 600.0);
        assert_eq!(style_weight("Regular"), 400.0);
        assert_eq!(style_weight("Light Oblique"), 300.0);
        assert!(style_italic("Bold Italic") && style_italic("Oblique"));
        assert!(!style_italic("Bold"));
        // The weight table is checked in order, so "extralight" wins over "light".
        assert_eq!(style_weight("ExtraLight"), 200.0);
    }

    #[test]
    fn role_keys_are_stable_and_distinct() {
        assert_eq!(role_key("Segoe UI", false, false), "fnt:Segoe UI:nn");
        assert_eq!(role_key("Segoe UI", true, false), "fnt:Segoe UI:bn");
        assert_eq!(role_key("Segoe UI", false, true), "fnt:Segoe UI:ni");
        assert_eq!(role_key("Segoe UI", true, true), "fnt:Segoe UI:bi");
    }

    #[test]
    fn nothing_is_bound_until_font_definitions_says_so() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A role key is only safe to name after font_definitions bound it.
        let k = role_key("Some Installed Font", true, false);
        assert!(!is_bound(&k));
        set_bound([k.clone()]);
        assert!(is_bound(&k));
        set_bound([]);
        assert!(!is_bound(&k));
    }

    #[test]
    fn damaged_or_non_font_files_contribute_nothing() {
        assert!(file_faces(b"ttcf\x00\x01\x00\x00\xff\xff\xff\xff").is_empty());
        assert!(file_faces(b"not a font at all").is_empty());
        assert!(file_faces(&[]).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_scan_catalogs_fonts_and_reads_each_folder_once() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).expect("temp dir");
        std::fs::write(dir.join("Ubuntu-Light.ttf"), epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        std::fs::write(dir.join("damaged.ttf"), b"ttcf\x00\x01\x00\x00\xff\xff\xff\xff").expect("write");
        std::fs::write(dir.join("readme.txt"), b"not a font").expect("write");
        // The same folder listed twice (once through `Sub/..`) is read once.
        let faces = scan_dirs(&[dir.clone(), dir.join("Sub/..")]);
        let families: Vec<&str> = faces.iter().map(|f| f.family.as_str()).collect();
        assert_eq!(families, ["Ubuntu"], "one face, family read from the name table");
        assert_eq!(faces[0].weight, 300.0, "“Light” reads as weight 300");
        assert!(!faces[0].italic);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_back_to_a_parent_folder_ends_the_scan() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-loop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).expect("temp dir");
        std::fs::write(dir.join("Sub/Ubuntu-Light.ttf"), epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        std::os::unix::fs::symlink(&dir, dir.join("Sub/loop")).expect("symlink");
        assert_eq!(scan_dirs(std::slice::from_ref(&dir)).len(), 1, "the loop doesn't multiply faces");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn binding_a_family_binds_all_four_roles_and_never_stores_missing_data() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("Ubuntu-Light.ttf");
        std::fs::write(&path, epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        let face = Face { family: "Ubuntu".into(), names: vec!["Ubuntu".into()], path, index: 0, weight: 300.0, italic: false };

        let mut fonts = egui::FontDefinitions::default();
        let bound = bind_faces(&mut fonts, "Ubuntu", std::slice::from_ref(&face));
        assert_eq!(bound.len(), 4, "regular, bold, italic and bold-italic roles");
        for key in &bound {
            let chain = fonts.families.get(&egui::FontFamily::Name(key.as_str().into())).expect("role family");
            assert!(chain.iter().all(|k| fonts.font_data.contains_key(k)), "every chained key has data");
        }
        // A family with no faces binds nothing and never touches the map.
        assert!(bind_faces(&mut fonts, "No Such Font", &[]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A font that is just a table directory and a `name` table holding `records` as
    /// `(Windows language id, name id, text)`: all [`file_faces`] reads.
    #[cfg(not(target_arch = "wasm32"))]
    fn font_with_names(records: &[(u16, u16, &str)]) -> Vec<u8> {
        fn be(v: &mut Vec<u8>, x: u16) {
            v.extend_from_slice(&x.to_be_bytes());
        }
        let mut name: Vec<u8> = Vec::new();
        let mut strings: Vec<u8> = Vec::new();
        for x in [0, records.len() as u16, (6 + 12 * records.len()) as u16] {
            be(&mut name, x);
        }
        for (language, id, text) in records {
            let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
            // Platform 3 (Windows), encoding 1 (Unicode BMP).
            for x in [3, 1, *language, *id, utf16.len() as u16, strings.len() as u16] {
                be(&mut name, x);
            }
            strings.extend(utf16);
        }
        name.extend(strings);
        let mut font: Vec<u8> = 0x0001_0000u32.to_be_bytes().to_vec();
        for x in [1, 16, 0, 0] {
            be(&mut font, x);
        }
        font.extend(b"name");
        font.extend(0u32.to_be_bytes());
        font.extend(28u32.to_be_bytes());
        font.extend((name.len() as u32).to_be_bytes());
        font.extend(name);
        font
    }

    const EN: u16 = 0x0409;
    const KO: u16 = 0x0412;

    fn face(family: &str, names: &[&str], path: &str, weight: f32) -> Face {
        Face { family: family.into(), names: names.iter().map(|s| s.to_string()).collect(), path: path.into(), index: 0, weight, italic: false }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_face_keeps_its_family_names_in_every_language() {
        // Korean Excel saves `맑은 고딕`, so the Korean name has to be kept next to the English one (#1898).
        let font = font_with_names(&[(EN, 1, "Malgun Gothic"), (EN, 2, "Regular"), (KO, 1, "맑은 고딕"), (KO, 2, "보통")]);
        let faces = file_faces(&font);
        assert_eq!(faces.len(), 1);
        let (_, family, names, style) = &faces[0];
        assert_eq!(family, "Malgun Gothic", "the font list keeps the English name");
        assert_eq!(names, &["Malgun Gothic", "맑은 고딕"]);
        assert_eq!(style, "Regular");

        // Both name IDs are kept: a legacy family name (ID 1) names a single face (#1606).
        let font = font_with_names(&[(EN, 1, "Noto Sans KR Medium"), (EN, 2, "Regular"), (EN, 16, "Noto Sans KR"), (EN, 17, "Medium")]);
        let (_, family, names, _) = &file_faces(&font)[0];
        assert_eq!(family, "Noto Sans KR");
        assert_eq!(names, &["Noto Sans KR", "Noto Sans KR Medium"]);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_name_table_full_of_names_is_capped() {
        let texts: Vec<String> = (0..100).map(|i| format!("Family {i}")).collect();
        let records: Vec<(u16, u16, &str)> = texts.iter().enumerate().map(|(i, t)| (0x0400 + i as u16, 1, t.as_str())).collect();
        let (_, _, names, _) = &file_faces(&font_with_names(&records))[0];
        assert_eq!(names.len(), MAX_FAMILY_NAMES);
    }

    #[test]
    fn a_family_is_found_by_any_of_its_names() {
        let names = ["Malgun Gothic", "맑은 고딕"];
        let cat = [face("Malgun Gothic", &names, "malgunbd.ttf", 700.0), face("Malgun Gothic", &names, "malgun.ttf", 400.0)];
        for wanted in ["Malgun Gothic", "malgun gothic", "맑은 고딕"] {
            let found: Vec<f32> = faces_named(&cat, wanted).iter().map(|f| f.weight).collect();
            assert_eq!(found, [400.0, 700.0], "{wanted:?}: both faces, regular first");
        }
        assert!(faces_named(&cat, "맑은").is_empty(), "a part of a name is not a name");
        assert!(faces_named(&cat, "").is_empty());
    }

    #[test]
    fn a_name_two_families_claim_has_a_fixed_owner() {
        let a = face("Alpha", &["Alpha", "공유"], "a/alpha.ttf", 400.0);
        let b = face("Beta", &["Beta", "공유"], "b/beta.ttf", 400.0);
        // Whatever order the scan saw the files in, the first by path wins.
        for cat in [[a.clone(), b.clone()], [b.clone(), a.clone()]] {
            let found = faces_named(&cat, "공유");
            assert_eq!(found.iter().map(|f| f.family.as_str()).collect::<Vec<_>>(), ["Alpha"]);
        }
        // A family's own name beats another family's alias, even one scanned earlier.
        let c = face("Gamma", &["Gamma", "Beta"], "0/gamma.ttf", 400.0);
        let found = faces_named(&[c, b], "Beta");
        assert_eq!(found.iter().map(|f| f.family.as_str()).collect::<Vec<_>>(), ["Beta"]);
    }

    #[test]
    fn a_legacy_name_picks_its_own_face_out_of_the_family() {
        let cat = [
            face("Noto Sans KR", &["Noto Sans KR"], "NotoSansKR-Regular.otf", 400.0),
            face("Noto Sans KR", &["Noto Sans KR", "Noto Sans KR Medium"], "NotoSansKR-Medium.otf", 500.0),
        ];
        assert_eq!(faces_named(&cat, "Noto Sans KR").len(), 2);
        let medium = faces_named(&cat, "Noto Sans KR Medium");
        assert_eq!(medium.iter().map(|f| f.weight).collect::<Vec<_>>(), [500.0]);
    }

    #[test]
    fn installed_names_are_kept_as_the_workbook_spells_them() {
        // The binding is keyed on the name `cell_family` looks up, so every spelling in use is
        // returned as written: the English and the Korean name both bind, over the same faces.
        let cat = [face("Malgun Gothic", &["Malgun Gothic", "맑은 고딕"], "malgun.ttf", 400.0)];
        let used: Vec<String> = ["맑은 고딕", "Malgun Gothic", "Not Installed", "맑은 고딕", "malgun gothic"].map(String::from).to_vec();
        assert_eq!(installed_in(&cat, &used), ["Malgun Gothic", "malgun gothic", "맑은 고딕"]);
        assert!(installed_in(&[], &used).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_family_bound_under_two_names_shares_its_font_data() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-alias-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("Ubuntu-Light.ttf");
        std::fs::write(&path, epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        let f = Face { family: "Ubuntu".into(), names: vec!["Ubuntu".into(), "우분투".into()], path, index: 0, weight: 300.0, italic: false };

        let mut fonts = egui::FontDefinitions::default();
        let before = fonts.font_data.len();
        let mut bound = bind_faces(&mut fonts, "Ubuntu", std::slice::from_ref(&f));
        bound.extend(bind_faces(&mut fonts, "우분투", std::slice::from_ref(&f)));
        assert!(bound.contains(&role_key("우분투", false, false)), "bound under the workbook's name");
        assert!(bound.contains(&role_key("Ubuntu", true, true)));
        assert_eq!(fonts.font_data.len(), before + 1, "one file, loaded once");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// On a Windows machine with Malgun Gothic, the real file carries its Korean name.
    #[cfg(windows)]
    #[test]
    fn malgun_gothic_answers_to_its_korean_name_when_installed() {
        let root = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        let Ok(data) = std::fs::read(root.join("Fonts").join("malgun.ttf")) else { return };
        let faces = file_faces(&data);
        assert!(faces.iter().any(|(_, family, names, _)| family == "Malgun Gothic" && names.iter().any(|n| n == "맑은 고딕")), "{faces:?}");
    }
}
