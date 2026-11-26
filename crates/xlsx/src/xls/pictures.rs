//! Places the pictures `craft-xls` read from an `.xls` sheet. GridCraft shows PNG and JPEG, so
//! EMF metafiles are drawn to a PNG ([`super::emf`]) and bitmaps (BMP, a DIB with a file
//! header) are re-encoded as PNG ([`super::png`]); other formats (WMF) are reported as not
//! imported.
//!
//! Shapes that show one picture share its bytes in `craft-xls`, so one conversion serves them
//! all, and the work and the output are bounded for a whole workbook.

use std::sync::Arc;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS};
use gridcraft_model::{Anchor, AnchorMode, Image, Sheet};

use super::{emf, png};
use crate::ReadReport;

/// Largest picture kept after conversion.
const MAX_PICTURE: usize = 64 * 1024 * 1024;
/// Bytes of converted pictures a whole workbook may hold.
const MAX_STORE_BYTES: usize = 256 * 1024 * 1024;
/// Pixels of bitmaps a whole workbook may decode: each is expanded to four bytes a pixel.
const MAX_STORE_PIXELS: usize = 64 * 1024 * 1024;

/// A picture as GridCraft shows it, and its type.
type Shown = (Arc<Vec<u8>>, &'static str);

/// What the pictures of one workbook may still spend.
pub(super) struct Allowance {
    raster: usize,
    bytes: usize,
    pixels: usize,
    /// Pictures converted so far, by source bytes, so shapes of one picture share the result.
    done: Vec<(Arc<Vec<u8>>, Option<Shown>)>,
}

impl Default for Allowance {
    fn default() -> Self {
        Allowance { raster: emf::RASTER_BUDGET, bytes: MAX_STORE_BYTES, pixels: MAX_STORE_PIXELS, done: Vec::new() }
    }
}

impl Allowance {
    fn charge(&mut self, n: usize) -> Option<()> {
        self.bytes = self.bytes.checked_sub(n)?;
        Some(())
    }

    /// The picture as bytes GridCraft can show, converting once per stored picture.
    fn shown(&mut self, data: &Arc<Vec<u8>>, mime: &'static str) -> Option<Shown> {
        if let Some((_, r)) = self.done.iter().find(|(d, _)| Arc::ptr_eq(d, data)) {
            return r.clone();
        }
        let r = self.convert(data, mime);
        self.done.push((Arc::clone(data), r.clone()));
        r
    }

    fn convert(&mut self, data: &Arc<Vec<u8>>, mime: &'static str) -> Option<Shown> {
        match mime {
            "image/png" | "image/jpeg" => {
                self.charge(data.len())?;
                Some((Arc::clone(data), mime))
            }
            "image/x-emf" => {
                let png = emf::render_png(data, &mut self.raster)?;
                if png.len() > MAX_PICTURE {
                    return None;
                }
                self.charge(png.len())?;
                Some((Arc::new(png), "image/png"))
            }
            "image/bmp" => {
                // A file header (14 bytes) in front of the DIB.
                let png = dib_to_png(data.get(14..)?, &mut self.pixels)?;
                self.charge(png.len())?;
                Some((Arc::new(png), "image/png"))
            }
            _ => None,
        }
    }
}

/// Adds the sheet's pictures to `sheet`, numbering them from `next_id`. Pictures GridCraft can't
/// show are reported once.
pub(super) fn place(sheet: &mut Sheet, from: &craft_xls::Sheet, allowance: &mut Allowance, next_id: &mut u32, report: &mut ReadReport) {
    let mut skipped = false;
    for pic in &from.pictures {
        let (Some((data, mime)), Some(anchor)) = (allowance.shown(&pic.data, pic.mime), anchor_of(sheet, &pic.anchor)) else {
            skipped = true;
            continue;
        };
        sheet.images.push(Image { id: *next_id, anchor, data, mime: mime.to_string(), alt: String::new() });
        *next_id += 1;
    }
    if skipped {
        report.warn("Some pictures in .xls files are in a format GridCraft can't show (such as WMF) and were left out.");
    }
}

/// A picture's anchor: the cells it spans, with offsets in 1/1024 of a column and 1/256 of a row.
fn anchor_of(sheet: &Sheet, a: &craft_xls::ClientAnchor) -> Option<Anchor> {
    let point = |col: u16, dx: u16, row: u16, dy: u16| -> Option<(CellRef, f64, f64)> {
        let (col, row) = (u32::from(col), u32::from(row));
        if col >= MAX_COLS.min(256) || row >= MAX_ROWS {
            return None;
        }
        let x = sheet.col_left(col) + f64::from(sheet.col_width(col)) * f64::from(dx.min(1024)) / 1024.0;
        let y = sheet.row_top(row) + f64::from(sheet.row_height(row)) * f64::from(dy.min(256)) / 256.0;
        Some((CellRef::new(row, col), x, y))
    };
    let (cell, x1, y1) = point(a.col1, a.dx1, a.row1, a.dy1)?;
    let (_, x2, y2) = point(a.col2, a.dx2, a.row2, a.dy2)?;
    let (width, height) = ((x2 - x1) as f32, (y2 - y1) as f32);
    if !(width >= 1.0 && height >= 1.0) {
        return None;
    }
    Some(Anchor {
        cell,
        dx: (x1 - sheet.col_left(cell.col)) as f32,
        dy: (y1 - sheet.row_top(cell.row)) as f32,
        width,
        height,
        mode: AnchorMode::MoveAndSize,
    })
}

/// A device-independent bitmap (a BITMAPINFOHEADER and pixels, no file header) as PNG. Only
/// uncompressed 1, 4, 8, 24 and 32-bit bitmaps are read.
fn dib_to_png(d: &[u8], pixels_left: &mut usize) -> Option<Vec<u8>> {
    let u32_at = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let header = u32_at(0)? as usize;
    if header < 40 {
        return None;
    }
    let width = u32_at(4)? as i32;
    let height = u32_at(8)? as i32;
    let bits = u32::from(d.get(14..16).map(|b| u16::from_le_bytes([b[0], b[1]]))?);
    let compression = u32_at(16)?;
    let mut colors = u32_at(32)? as usize;
    if compression != 0 || width <= 0 || height == 0 || !matches!(bits, 1 | 4 | 8 | 24 | 32) {
        return None;
    }
    let (w, h) = (width as usize, height.unsigned_abs() as usize);
    let count = w.checked_mul(h)?;
    if count > 16 * 1024 * 1024 {
        return None;
    }
    // Taken before the pixels are expanded, from the allowance of the whole store.
    *pixels_left = pixels_left.checked_sub(count)?;
    if bits <= 8 && colors == 0 {
        colors = 1 << bits;
    }
    let pixels_at = header.checked_add(colors.min(256).checked_mul(4)?)?;
    let palette = d.get(header..pixels_at)?;
    let pixels = d.get(pixels_at..)?;
    let stride = w.checked_mul(bits as usize)?.div_ceil(32).checked_mul(4)?;
    let top_down = height < 0;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let src = if top_down { y } else { h - 1 - y };
        let from = src.checked_mul(stride)?;
        let row = pixels.get(from..from.checked_add(stride)?)?;
        for x in 0..w {
            let px = match bits {
                24 => [row[x * 3 + 2], row[x * 3 + 1], row[x * 3], 255],
                32 => [row[x * 4 + 2], row[x * 4 + 1], row[x * 4], 255],
                _ => {
                    let per = 8 / bits as usize;
                    let byte = row[x / per];
                    let shift = 8 - bits as usize * (x % per + 1);
                    let i = usize::from((byte >> shift) & (u8::MAX >> (8 - bits)));
                    let c = palette.get(i * 4..i * 4 + 3)?;
                    [c[2], c[1], c[0], 255]
                }
            };
            rgba.extend_from_slice(&px);
        }
    }
    png::encode(w as u32, h as u32, &rgba)
}
