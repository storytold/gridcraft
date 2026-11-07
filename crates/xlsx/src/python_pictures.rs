//! The pictures of Python cells in a drawing part. How Excel stores a Python image isn't documented,
//! so they are ordinary pictures whose descriptions say what they are, as an equation's does
//! (`gridcraft:latex:`). Any other application sees a normal picture.
//!
//! A floating picture linked to a Python cell has the description
//! `gridcraft:python:<n>:<reference>:<alt>`. A sheet name can hold any separator, so `<n>`, the number
//! of characters of the reference, is what ends it; characters and not bytes, so that a script holding
//! the description as text can slice it. A picture whose description doesn't read this way is a plain
//! picture, with the description as its alternative text.
//!
//! The picture a Python cell gave, for either return type, is a companion picture: hidden, anchored to
//! one cell, on that cell, named `Python G5`, with the description
//! `gridcraft:pythonCell:<cell>:<density>:<alt>`. The density is what the file holds nowhere else, as
//! the pixels come from the image's header, and a density written `2s` says the cell's text stands in
//! for the picture of an Excel Value with no room for a rich value. A reopened workbook then shows its saved plots without
//! running code. Reading goes by the anchor, as that moves with its cell when another application
//! inserts a row and the description doesn't. With no Python cell there, as after a save through
//! openpyxl, the companion is an ordinary floating picture, so that no picture is lost.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::addr::unquote_sheet;
use gridcraft_core::{CellRef, Value};
use gridcraft_model::{CellPicture, PyCode, PyImage, PyOutput, Sheet, Workbook};

use crate::xml;

const LINK: &str = "gridcraft:python:";
const CELL: &str = "gridcraft:pythonCell:";
/// What a Python cell whose result is a picture holds as its value, and what the worker names the
/// result's type.
pub const IMAGE: &str = "Image";
/// Longest reference a marker holds: a quoted sheet name of 31 characters, each doubled, and the
/// largest cell come to 77. A crafted file can claim any length.
const MAX_REFERENCE_CHARS: usize = 128;
/// Digits of the longest length.
const MAX_LENGTH_DIGITS: usize = 3;

/// The description of a floating picture linked to `reference`. `None` for a reference a file can't
/// hold, such as `#REF!`: the picture is then saved as a plain picture.
pub fn linked_descr(reference: &str, alt: &str) -> Option<String> {
    // What XML can't carry never reaches the file, so it isn't counted either. A sheet name loses the
    // same characters, and the reference still names the sheet when it is read.
    let reference: String = reference.chars().filter(|c| xml::is_xml_char(*c)).collect();
    parse_reference(&reference)?;
    let n = reference.chars().count();
    (n <= MAX_REFERENCE_CHARS).then(|| format!("{LINK}{n}:{reference}:{alt}"))
}

/// The reference and alternative text of a linked picture's description. `None` for any other
/// description, and for a marker that is malformed.
pub fn read_link(descr: &str) -> Option<(&str, &str)> {
    let rest = descr.strip_prefix(LINK)?;
    let (digits, rest) = rest.split_once(':')?;
    if digits.is_empty() || digits.len() > MAX_LENGTH_DIGITS || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: usize = digits.parse().ok()?;
    if n > MAX_REFERENCE_CHARS {
        return None;
    }
    let (end, _) = rest.char_indices().nth(n)?;
    let reference = rest.get(..end)?;
    let alt = rest.get(end..)?.strip_prefix(':')?;
    parse_reference(reference)?;
    Some((reference, alt))
}

/// The sheet and cell of a reference such as `'My Sheet'!$G$5`. The last `!` ends the sheet's name,
/// as a cell holds none and a name can.
fn parse_reference(reference: &str) -> Option<(String, CellRef)> {
    let (sheet, cell) = reference.rsplit_once('!')?;
    let sheet = unquote_sheet(sheet);
    if sheet.is_empty() {
        return None;
    }
    Some((sheet, CellRef::parse(&cell.replace('$', ""))?))
}

/// Forgets the link of every picture whose source isn't a Python cell of this workbook, which takes
/// every sheet to be read: opening a file never deletes a picture, and the engine removes a linked
/// picture whose source is gone. Code too long to run counts as no Python cell, as the engine drops
/// it. Returns how many links went.
pub fn settle_links(wb: &mut Workbook) -> usize {
    if !wb.sheets.iter().any(|s| s.images.iter().any(|o| o.python_source.is_some())) {
        return 0;
    }
    let sheets: HashMap<String, usize> = wb.sheets.iter().enumerate().map(|(i, s)| (s.name.to_ascii_lowercase(), i)).collect();
    let names_python_cell = |wb: &Workbook, reference: &str| {
        let Some((sheet, at)) = parse_reference(reference) else { return false };
        let py = sheets.get(&sheet.to_ascii_lowercase()).and_then(|i| wb.sheets.get(*i)).and_then(|s| s.cell(at)).and_then(|c| c.py.as_deref());
        py.is_some_and(|py| py.fits())
    };
    let mut dropped = 0;
    for i in 0..wb.sheets.len() {
        let Some(sheet) = wb.sheets.get(i) else { continue };
        let kept: Vec<bool> = sheet.images.iter().map(|o| o.python_source.as_deref().is_none_or(|r| names_python_cell(wb, r))).collect();
        if kept.iter().all(|k| *k) {
            continue;
        }
        let Some(sheet) = wb.sheet_mut(i) else { continue };
        for (picture, kept) in sheet.images.iter_mut().zip(kept) {
            if !kept {
                picture.python_source = None;
                dropped += 1;
            }
        }
    }
    dropped
}

/// The description of the companion picture of the cell `at`.
pub fn cell_descr(at: CellRef, image: &PyImage, stands_in: bool) -> String {
    format!("{CELL}{}:{}{}:{}", at.a1(), image.density, if stands_in { "s" } else { "" }, image.picture.alt)
}

/// What a companion picture's description says, past its cell.
#[derive(Debug, PartialEq, Eq)]
pub struct CellMarker<'a> {
    pub density: u32,
    /// The cell's text stands in for the picture, though its return type is Excel Value.
    pub stands_in: bool,
    pub alt: &'a str,
}

/// A companion picture's description. `None` for any other description, and for a marker that is
/// malformed. A worker's picture has a density of 1 or 2.
pub fn read_cell_marker(descr: &str) -> Option<CellMarker<'_>> {
    let (cell, rest) = descr.strip_prefix(CELL)?.split_once(':')?;
    CellRef::parse(cell)?;
    let (density, alt) = rest.split_once(':')?;
    let (density, stands_in) = match density {
        "1" => (1, false),
        "2" => (2, false),
        "1s" => (1, true),
        "2s" => (2, true),
        _ => return None,
    };
    Some(CellMarker { density, stands_in, alt })
}

/// `picture` as a Python cell's image, its size worked out as a run works it out. `None` for bytes
/// past the limits of a cell picture, which a worker never sends.
fn saved_image(picture: Arc<CellPicture>, density: u32) -> Option<PyImage> {
    let (width, height, _) = crate::richdata::picture_size(&picture.data)?;
    let density = density.max(1);
    Some(PyImage { picture, width: (width / density).max(1), height: (height / density).max(1), density, floor: None, fitted: None })
}

/// The result of a cell whose picture a file holds: what a run left, without what only a run knows.
fn saved_output(image: PyImage) -> PyOutput {
    PyOutput {
        value: Value::text(IMAGE),
        type_name: IMAGE.to_string(),
        preview: IMAGE.to_string(),
        convertible: false,
        table: None,
        from_run: false,
        image: Some(image),
    }
}

/// `py` with the picture a cell's rich value holds as its result. `None` when it can't be one.
pub fn with_cell_picture(py: &PyCode, picture: Arc<CellPicture>) -> Option<PyCode> {
    Some(PyCode { output: Some(saved_output(saved_image(picture, 1)?)), ..py.clone() })
}

/// Takes a companion picture as the result of the Python cell at `at`, when that cell's result is a
/// picture and it can be this one: the file says so with its value, or with a rich value of the same
/// bytes, whose density only the companion holds. Any other value is a newer result, from an
/// application that left the companion alone. A spill, or code too long to stay, is no place for it
/// either. Returns whether it was taken.
pub fn take_for_cell(sheet: &mut Sheet, at: CellRef, marker: &CellMarker<'_>, data: &[u8]) -> bool {
    if sheet.spill_ranges.contains_key(&at) {
        return false;
    }
    let Some(cell) = sheet.cells.get_mut(at) else { return false };
    let Some(py) = cell.py.as_mut().filter(|py| py.fits()) else { return false };
    let image = match py.image() {
        Some(kept) if kept.picture.data != data => return false,
        Some(kept) => saved_image(kept.picture.clone(), marker.density),
        // The word from a Python Object, or from an Excel Value saved with no room for a rich value. For
        // any other Excel Value it is a text the code gave, newer than the companion.
        None if (py.object || marker.stands_in) && matches!(&cell.value, Value::Text(text) if &**text == IMAGE) => {
            crate::richdata::picture_size(data).and_then(|(_, _, mime)| {
                let picture = CellPicture { data: data.to_vec(), mime: mime.to_string(), alt: marker.alt.to_string() };
                saved_image(Arc::new(picture), marker.density)
            })
        }
        None => return false,
    };
    let Some(image) = image else { return false };
    Arc::make_mut(py).output = Some(saved_output(image));
    cell.value = Value::text(IMAGE);
    true
}
