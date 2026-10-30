//! System TrueType fonts for PDF export, so text the standard PDF fonts cannot encode
//! (Cyrillic, Greek, …) is embedded instead of left to the reader's fonts.
//!
//! The first export looks for a metric-compatible family per standard face (Arial or
//! Liberation Sans for Helvetica, Times New Roman or Liberation Serif for Times, Courier New or
//! Liberation Mono for Courier, then DejaVu) in the usual font folders and installs what it finds.
//! Nothing found leaves the previous behaviour: a non-embedded CID font.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use gridcraft_pdf::{EmbeddedFont, Font};

/// File names to try per face, best first. Matched without regard to case.
fn candidates(face: Font) -> &'static [&'static str] {
    match face {
        Font::Helvetica => {
            &["arial.ttf", "LiberationSans-Regular.ttf", "Arimo-Regular.ttf", "DejaVuSans.ttf", "NotoSans-Regular.ttf", "FreeSans.ttf"]
        }
        Font::HelveticaBold => &[
            "arialbd.ttf",
            "Arial Bold.ttf",
            "LiberationSans-Bold.ttf",
            "Arimo-Bold.ttf",
            "DejaVuSans-Bold.ttf",
            "NotoSans-Bold.ttf",
            "FreeSansBold.ttf",
        ],
        Font::HelveticaOblique => &[
            "ariali.ttf",
            "Arial Italic.ttf",
            "LiberationSans-Italic.ttf",
            "Arimo-Italic.ttf",
            "DejaVuSans-Oblique.ttf",
            "NotoSans-Italic.ttf",
            "FreeSansOblique.ttf",
        ],
        Font::HelveticaBoldOblique => &[
            "arialbi.ttf",
            "Arial Bold Italic.ttf",
            "LiberationSans-BoldItalic.ttf",
            "Arimo-BoldItalic.ttf",
            "DejaVuSans-BoldOblique.ttf",
            "NotoSans-BoldItalic.ttf",
            "FreeSansBoldOblique.ttf",
        ],
        Font::TimesRoman => &[
            "times.ttf",
            "Times New Roman.ttf",
            "LiberationSerif-Regular.ttf",
            "Tinos-Regular.ttf",
            "DejaVuSerif.ttf",
            "NotoSerif-Regular.ttf",
            "FreeSerif.ttf",
        ],
        Font::TimesBold => &[
            "timesbd.ttf",
            "Times New Roman Bold.ttf",
            "LiberationSerif-Bold.ttf",
            "Tinos-Bold.ttf",
            "DejaVuSerif-Bold.ttf",
            "NotoSerif-Bold.ttf",
            "FreeSerifBold.ttf",
        ],
        Font::TimesItalic => &[
            "timesi.ttf",
            "Times New Roman Italic.ttf",
            "LiberationSerif-Italic.ttf",
            "Tinos-Italic.ttf",
            "DejaVuSerif-Italic.ttf",
            "NotoSerif-Italic.ttf",
            "FreeSerifItalic.ttf",
        ],
        Font::TimesBoldItalic => &[
            "timesbi.ttf",
            "Times New Roman Bold Italic.ttf",
            "LiberationSerif-BoldItalic.ttf",
            "Tinos-BoldItalic.ttf",
            "DejaVuSerif-BoldItalic.ttf",
            "NotoSerif-BoldItalic.ttf",
            "FreeSerifBoldItalic.ttf",
        ],
        Font::Courier => &["cour.ttf", "Courier New.ttf", "LiberationMono-Regular.ttf", "Cousine-Regular.ttf", "DejaVuSansMono.ttf", "FreeMono.ttf"],
        Font::CourierBold => {
            &["courbd.ttf", "Courier New Bold.ttf", "LiberationMono-Bold.ttf", "Cousine-Bold.ttf", "DejaVuSansMono-Bold.ttf", "FreeMonoBold.ttf"]
        }
        Font::CourierOblique => &[
            "couri.ttf",
            "Courier New Italic.ttf",
            "LiberationMono-Italic.ttf",
            "Cousine-Italic.ttf",
            "DejaVuSansMono-Oblique.ttf",
            "FreeMonoOblique.ttf",
        ],
        Font::CourierBoldOblique => &[
            "courbi.ttf",
            "Courier New Bold Italic.ttf",
            "LiberationMono-BoldItalic.ttf",
            "Cousine-BoldItalic.ttf",
            "DejaVuSansMono-BoldOblique.ttf",
            "FreeMonoBoldOblique.ttf",
        ],
    }
}

/// The folders fonts are installed in on this platform.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    if cfg!(windows) {
        dirs.push(env("WINDIR").unwrap_or_else(|| PathBuf::from("C:\\Windows")).join("Fonts"));
        if let Some(local) = env("LOCALAPPDATA") {
            dirs.push(local.join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = env("HOME") {
            dirs.push(home.join("Library").join("Fonts"));
        }
    } else {
        if let Some(data) = env("XDG_DATA_HOME") {
            dirs.push(data.join("fonts"));
        }
        if let Some(home) = env("HOME") {
            dirs.push(home.join(".local").join("share").join("fonts"));
            dirs.push(home.join(".fonts"));
        }
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        dirs.push(PathBuf::from("/usr/share/fonts"));
    }
    dirs
}

/// Most directories visited and files listed while looking for fonts.
const MAX_DIRS: usize = 2_000;
const MAX_FILES: usize = 50_000;
const MAX_DEPTH: usize = 5;

/// Every `.ttf` file under `dirs`, as (lower-case file name, path), within the limits above.
fn list_fonts(dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut pending: Vec<(PathBuf, usize)> = dirs.iter().map(|d| (d.clone(), 0)).collect();
    let mut visited = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        visited += 1;
        if visited > MAX_DIRS || found.len() >= MAX_FILES {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    pending.push((path, depth + 1));
                }
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                let lower = name.to_lowercase();
                if lower.ends_with(".ttf") && found.len() < MAX_FILES {
                    found.push((lower, path));
                }
            }
        }
    }
    found
}

/// Loads the best candidate for `face` among `files`.
fn load(face: Font, files: &[(String, PathBuf)]) -> Option<EmbeddedFont> {
    candidates(face).iter().find_map(|want| {
        let want = want.to_lowercase();
        let (_, path) = files.iter().find(|(name, _)| *name == want)?;
        read_font(path)
    })
}

fn read_font(path: &Path) -> Option<EmbeddedFont> {
    // Check before reading: a huge file, a device or a FIFO named `*.ttf` is never read.
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > gridcraft_pdf::MAX_FONT_BYTES as u64 {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    EmbeddedFont::from_bytes(data, 0).ok()
}

/// Installs the system fonts for PDF export, once per process. Returns how many faces got one.
pub fn ensure_installed() -> usize {
    static DONE: OnceLock<usize> = OnceLock::new();
    *DONE.get_or_init(|| install_from(&font_dirs()))
}

/// Installs a font per standard face from `dirs`. Returns how many faces got one.
pub fn install_from(dirs: &[PathBuf]) -> usize {
    let files = list_fonts(dirs);
    let mut installed = 0;
    for face in Font::ALL {
        if let Some(font) = load(face, &files) {
            gridcraft_pdf::install_font(face, font);
            installed += 1;
        }
    }
    installed
}
