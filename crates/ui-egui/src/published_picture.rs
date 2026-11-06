//! How a floating picture on the system clipboard is known again. Copying one publishes its pixels,
//! and all a later paste finds there is pixels: a hash of them tells the picture this process
//! published from any other. Another running process has its own object clipboard, so the hash also
//! goes in a small file, for that process to refuse the picture ([`crate::SheetApp::paste_color_image`]).

use std::io::{Read as _, Write as _};
use std::path::Path;

use egui::Color32;

/// What a copied picture leaves on the system clipboard where the host can't take a picture (the web,
/// or a picture too large to decode), so that Paste can tell whether anything came after it.
pub const PICTURE_COPIED_TEXT: &str = "GridCraft picture";

/// The note's name, in the temporary folder.
pub const NOTE_FILE: &str = "gridcraft-published-picture";

/// More than the note's 16 digits and a line break, and little enough to read on any paste.
const NOTE_MAX_BYTES: u64 = 32;

/// FNV-1a over the size and the pixels: the same in every process, and needing no key. A pixel counts
/// as egui holds it, premultiplied, because egui's `Event::PasteImage` gives nothing else, and going
/// back from there isn't exact for a pixel that is partly transparent.
fn hash(width: u32, height: u32, pixels: impl Iterator<Item = [u8; 4]>) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let eat = |hash: u64, bytes: &[u8]| bytes.iter().fold(hash, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(PRIME));
    let start = eat(eat(0xcbf2_9ce4_8422_2325, &width.to_le_bytes()), &height.to_le_bytes());
    pixels.fold(start, |hash, pixel| eat(hash, &pixel))
}

/// The hash of a picture as the host's clipboard gives it.
pub fn rgba_hash(image: &image::RgbaImage) -> u64 {
    hash(
        image.width(),
        image.height(),
        image.pixels().map(|p| {
            let [r, g, b, a] = p.0;
            Color32::from_rgba_unmultiplied(r, g, b, a).to_array()
        }),
    )
}

/// The hash of a picture as egui's `Event::PasteImage` gives it. The same as [`rgba_hash`] of the
/// picture it was made from.
pub fn color_image_hash(image: &egui::ColorImage) -> u64 {
    let [width, height] = image.size.map(|side| u32::try_from(side).unwrap_or(u32::MAX));
    hash(width, height, image.pixels.iter().map(|c| c.to_array()))
}

/// Writes the hash to the note, or with `None` removes the note. The temporary folder can be everyone's,
/// so the note arrives by renaming a file made new here, which follows no link somebody left there.
pub fn write(path: &Path, hash: Option<u64>) -> std::io::Result<()> {
    let Some(hash) = hash else {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    };
    let temp = path.with_extension(format!("{hash:016x}"));
    let _ = std::fs::remove_file(&temp);
    let written = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp).and_then(|mut file| writeln!(file, "{hash:016x}"));
    let done = written.and_then(|()| std::fs::rename(&temp, path));
    if done.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    done
}

/// The hash in the note. `None` for anything that isn't one: no note, an error, or a file that isn't
/// what [`write`] writes. Not a pipe or a device either, whose read could wait for ever.
pub fn read(path: &Path) -> Option<u64> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > NOTE_MAX_BYTES {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(path).ok()?.take(NOTE_MAX_BYTES).read_to_string(&mut text).ok()?;
    u64::from_str_radix(text.trim(), 16).ok()
}
