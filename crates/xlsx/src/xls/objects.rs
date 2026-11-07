//! Cell notes, hyperlinks and window panes of a worksheet substream, from the public [MS-XLS]
//! and [MS-OSHARED] specifications:
//!
//! - notes: Obj ([MS-XLS] 2.4.181, its FtCmo names the note's object id), TxO (2.4.329, the text,
//!   in the `Continue` records after it) and Note (2.4.179, the cell, author and shown flag);
//! - hyperlinks: HLink (2.4.140) holding a Hyperlink Object ([MS-OSHARED] 2.3.7.1) with a URL
//!   or file moniker and/or a location in the workbook, and HLinkTooltip (2.4.141);
//! - panes: Window2 (2.4.346, the display flags and whether panes are frozen) and Pane (2.4.189).
//!
//! The records are gathered while the substream is read and applied when it ends, so their
//! order in the file doesn't matter. A malformed note or hyperlink is skipped with a warning;
//! it never fails the whole file.

use std::collections::HashMap;

use gridcraft_core::{CellRef, MAX_ROWS, RangeRef};
use gridcraft_model::{Comment, Hyperlink, Sheet, SheetViewMode};

use super::Budget;
use super::records::{Cursor, Record};
use crate::{IoError, ReadReport};

const NOTE: u16 = 0x001C;
const PANE: u16 = 0x0041;
const OBJ: u16 = 0x005D;
const MSO_DRAWING: u16 = 0x00EC;
const TXO: u16 = 0x01B6;
const HLINK: u16 = 0x01B8;
const WINDOW2: u16 = 0x023E;
const HLINK_TOOLTIP: u16 = 0x0800;

/// Whether [`Objects::record`] reads records of this type.
pub(super) fn handles(id: u16) -> bool {
    matches!(id, NOTE | PANE | OBJ | MSO_DRAWING | TXO | HLINK | WINDOW2 | HLINK_TOOLTIP)
}

/// Notes and hyperlink records kept per sheet at most (Excel allows 65,530 hyperlinks).
const MAX_NOTES: usize = 65_536;
const MAX_LINKS: usize = 65_536;
/// Cells given a hyperlink per sheet at most (as for .xlsx).
const MAX_LINK_CELLS: usize = 10_000;
/// Bytes of hyperlink targets and tooltips copied onto a sheet's cells at most (copied per cell).
const MAX_LINK_BYTES: usize = 16 * 1024 * 1024;
/// `..\` steps of a relative file link at most.
const MAX_ANTI: usize = 64;

/// FtCmo.ot of a note ([MS-XLS] 2.5.143).
const OT_NOTE: u16 = 0x0019;
/// CLSID_URLMoniker {79EAC9E0-BAF9-11CE-8C82-00AA004BA90B} as stored.
const URL_MONIKER: [u8; 16] = [0xE0, 0xC9, 0xEA, 0x79, 0xF9, 0xBA, 0xCE, 0x11, 0x8C, 0x82, 0x00, 0xAA, 0x00, 0x4B, 0xA9, 0x0B];
/// CLSID_FileMoniker {00000303-0000-0000-C000-000000000046} as stored.
const FILE_MONIKER: [u8; 16] = [0x03, 0x03, 0, 0, 0, 0, 0, 0, 0xC0, 0, 0, 0, 0, 0, 0, 0x46];

struct Note {
    pos: CellRef,
    shown: bool,
    obj: u16,
    author: String,
}

struct Pane {
    x: u16,
    y: u16,
    top: u16,
    left: u16,
}

/// What a worksheet's object and window records said, applied by [`Objects::finish`].
#[derive(Default)]
pub(super) struct Objects {
    /// The object id of the last Obj that is a note, waiting for its TxO.
    note_obj: Option<u16>,
    texts: HashMap<u16, String>,
    notes: Vec<Note>,
    links: Vec<(RangeRef, Hyperlink)>,
    /// Index in `links` of the last link for each range, for the tooltips that follow.
    link_at: HashMap<RangeRef, usize>,
    drawing: bool,
    any_object: bool,
    other_objects: bool,
    bad_notes: bool,
    bad_links: bool,
    window: Option<(u16, CellRef)>,
    pane: Option<Pane>,
}

/// An error that must stop the import (a resource budget), as opposed to a malformed record.
fn fatal(e: &IoError) -> bool {
    matches!(e, IoError::TooLarge(_))
}

/// `n` UTF-16 code units (fewer if the record ends first), up to the first NUL.
fn utf16(c: &mut Cursor<'_, '_>, n: usize) -> Result<String, IoError> {
    let n = n.min(c.remaining() / 2);
    let mut units = Vec::with_capacity(n);
    for _ in 0..n {
        units.push(c.u16()?);
    }
    if let Some(end) = units.iter().position(|&u| u == 0) {
        units.truncate(end);
    }
    Ok(String::from_utf16_lossy(&units))
}

/// HyperlinkString ([MS-OSHARED] 2.3.7.9): a character count that includes the final NUL,
/// then UTF-16.
fn hyperlink_string(c: &mut Cursor<'_, '_>) -> Result<String, IoError> {
    let n = c.u32()? as usize;
    utf16(c, n)
}

/// HyperlinkMoniker ([MS-OSHARED] 2.3.7.2): a URL or file moniker. `None` for other kinds.
fn moniker(c: &mut Cursor<'_, '_>) -> Result<Option<String>, IoError> {
    let mut clsid = [0u8; 16];
    for b in clsid.iter_mut() {
        *b = c.u8()?;
    }
    if clsid == URL_MONIKER {
        // URLMoniker (2.3.7.6): byte length, then the NUL-terminated URL (maybe more after it).
        let len = c.u32()? as usize;
        return Ok(Some(utf16(c, len / 2)?));
    }
    if clsid != FILE_MONIKER {
        return Ok(None);
    }
    // FileMoniker (2.3.7.8): `..\` count, ANSI path, then maybe the Unicode path.
    let anti = usize::from(c.u16()?).min(MAX_ANTI);
    let ansi_len = (c.u32()? as usize).min(c.remaining());
    let mut ansi = Vec::with_capacity(ansi_len);
    for _ in 0..ansi_len {
        ansi.push(c.u8()?);
    }
    if let Some(end) = ansi.iter().position(|&b| b == 0) {
        ansi.truncate(end);
    }
    let mut path: String = ansi.iter().map(|&b| char::from(b)).collect();
    // endServer, versionNumber, reserved1 (16) and reserved2 (4).
    if c.remaining() >= 24 {
        c.skip(24)?;
        let size = c.u32()? as usize;
        if size > 0 {
            let bytes = c.u32()? as usize;
            c.u16()?; // usKeyValue
            let wide = utf16(c, bytes / 2)?;
            if !wide.is_empty() {
                path = wide;
            }
        }
    }
    Ok(Some(format!("{}{path}", "..\\".repeat(anti))))
}

fn ref8(c: &mut Cursor<'_, '_>) -> Result<Option<RangeRef>, IoError> {
    let (r0, r1, c0, c1) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
    if r1 < r0 || c1 < c0 || c1 >= 256 || u32::from(r1) >= MAX_ROWS {
        return Ok(None);
    }
    Ok(Some(RangeRef::new(CellRef::new(u32::from(r0), u32::from(c0)), CellRef::new(u32::from(r1), u32::from(c1)))))
}

/// HLink: the cells and the link. `Ok(None)` for a link GridCraft can't represent.
fn hlink(c: &mut Cursor<'_, '_>) -> Result<Option<(RangeRef, Hyperlink)>, IoError> {
    let Some(range) = ref8(c)? else { return Ok(None) };
    c.skip(16)?; // hlinkClsid
    let _version = c.u32()?;
    let flags = c.u32()?;
    if flags & 0x10 != 0 {
        hyperlink_string(c)?; // displayName: the cell's text is what shows
    }
    if flags & 0x80 != 0 {
        hyperlink_string(c)?; // targetFrameName
    }
    let mut target = String::new();
    let external = flags & 0x01 != 0;
    if external {
        target = if flags & 0x100 != 0 {
            hyperlink_string(c)?
        } else {
            match moniker(c)? {
                Some(t) => t,
                None => return Ok(None),
            }
        };
    }
    let location = if flags & 0x08 != 0 { hyperlink_string(c)? } else { String::new() };
    let location = location.trim_start_matches('#');
    if !location.is_empty() {
        target = if target.is_empty() { location.to_string() } else { format!("{target}#{location}") };
    }
    if target.is_empty() {
        return Ok(None);
    }
    Ok(Some((range, Hyperlink { target, tooltip: None, external })))
}

/// TxO: the text in the `Continue` records after the header, each starting with a compression flag.
fn txo_text(rec: &Record<'_>) -> Result<String, IoError> {
    let mut c = Cursor::new(&rec.parts);
    c.skip(10)?; // flags, rot, controlInfo
    let cch = usize::from(c.u16()?);
    if cch == 0 {
        return Ok(String::new());
    }
    let parts = rec.parts.get(1..).unwrap_or(&[]);
    let mut t = Cursor::new(parts);
    let high = t.u8()? & 1 != 0;
    t.chars(cch.min(t.remaining()), high)
}

/// Note text as Excel shows it: line breaks as `\n`, without the "Author:" line Excel starts
/// a note with (as the .xlsx reader does).
fn note_text(raw: &str, author: &str) -> String {
    let text = raw.replace("\r\n", "\n").replace('\r', "\n");
    if !author.is_empty()
        && let Some(rest) = text.strip_prefix(&format!("{author}:"))
    {
        return rest.strip_prefix('\n').unwrap_or(rest).to_string();
    }
    text
}

impl Objects {
    /// Reads one of the records this module handles (see the `const`s).
    pub(super) fn record(&mut self, rec: &Record<'_>, budget: &mut Budget) -> Result<(), IoError> {
        let mut c = Cursor::new(&rec.parts);
        match rec.id {
            MSO_DRAWING => self.drawing = true,
            OBJ => {
                self.any_object = true;
                self.note_obj = None;
                // FtCmo: ft, cb, ot, id.
                let cmo = (|| Ok::<_, IoError>((c.u16()?, c.u16()?, c.u16()?, c.u16()?)))();
                match cmo {
                    Ok((0x0015, _, OT_NOTE, id)) => self.note_obj = Some(id),
                    _ => self.other_objects = true,
                }
            }
            TXO => {
                let Some(id) = self.note_obj.take() else { return Ok(()) };
                match txo_text(rec) {
                    Ok(text) => {
                        budget.text(text.len())?;
                        self.texts.insert(id, text);
                    }
                    Err(e) if fatal(&e) => return Err(e),
                    Err(_) => self.bad_notes = true,
                }
            }
            NOTE => {
                let note = (|| {
                    let (row, col, grbit, obj) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
                    let author = if c.remaining() >= 3 { c.string()? } else { String::new() };
                    Ok::<_, IoError>((row, col, grbit, obj, author))
                })();
                match note {
                    Ok((row, col, grbit, obj, author)) if col < 256 && self.notes.len() < MAX_NOTES => {
                        budget.text(author.len())?;
                        self.notes.push(Note { pos: CellRef::new(u32::from(row), u32::from(col)), shown: grbit & 0x0002 != 0, obj, author });
                    }
                    Err(e) if fatal(&e) => return Err(e),
                    _ => self.bad_notes = true,
                }
            }
            HLINK => match hlink(&mut c) {
                Ok(Some(link)) if self.links.len() < MAX_LINKS => {
                    budget.text(link.1.target.len())?;
                    self.link_at.insert(link.0, self.links.len());
                    self.links.push(link);
                }
                Err(e) if fatal(&e) => return Err(e),
                _ => self.bad_links = true,
            },
            HLINK_TOOLTIP => {
                let tip = (|| {
                    c.u16()?; // rt
                    let range = ref8(&mut c)?;
                    let n = c.remaining() / 2;
                    Ok::<_, IoError>((range, utf16(&mut c, n)?))
                })();
                if let Ok((Some(range), tip)) = tip
                    && !tip.is_empty()
                    && let Some(&i) = self.link_at.get(&range)
                    && let Some((_, link)) = self.links.get_mut(i)
                {
                    budget.text(tip.len())?;
                    link.tooltip = Some(tip);
                }
            }
            WINDOW2 => {
                // The first Window2 is the sheet's window; a truncated one is ignored.
                if self.window.is_none()
                    && c.remaining() >= super::WINDOW2_MIN_LEN
                    && let Ok((grbit, top, left)) = (|| Ok::<_, IoError>((c.u16()?, c.u16()?, c.u16()?)))()
                {
                    self.window = Some((grbit, CellRef::new(u32::from(top), u32::from(left.min(255)))));
                }
            }
            PANE => {
                if self.pane.is_none()
                    && let Ok((x, y, top, left)) = (|| Ok::<_, IoError>((c.u16()?, c.u16()?, c.u16()?, c.u16()?)))()
                {
                    self.pane = Some(Pane { x, y, top, left });
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Applies what was read to the sheet.
    pub(super) fn finish(self, sheet: &mut Sheet, report: &mut ReadReport) {
        for n in self.notes {
            let raw = self.texts.get(&n.obj).map_or("", String::as_str);
            let text = note_text(raw, &n.author);
            sheet.comments.insert(
                n.pos,
                Comment {
                    author: n.author,
                    text,
                    replies: vec![],
                    threaded: false,
                    resolved: false,
                    visible: n.shown,
                    dates: vec![],
                    note_box: None,
                },
            );
        }
        let mut cells = 0usize;
        let mut bytes = 0usize;
        'links: for (range, link) in self.links {
            for at in range.iter() {
                if cells >= MAX_LINK_CELLS {
                    report.warn(format!("Hyperlinks past the first {MAX_LINK_CELLS} cells of a sheet were dropped."));
                    break 'links;
                }
                // Each copy holds the target and the tooltip, which can span Continue records.
                let copied = link.target.len().saturating_add(link.tooltip.as_ref().map_or(0, String::len));
                if bytes.saturating_add(copied) > MAX_LINK_BYTES {
                    report.warn(format!("Hyperlink data past {MAX_LINK_BYTES} bytes of a sheet were dropped."));
                    break 'links;
                }
                cells += 1;
                bytes += copied;
                sheet.hyperlinks.insert(at, link.clone());
            }
        }
        if let Some((grbit, top_left)) = self.window {
            sheet.show_formulas = grbit & 0x0001 != 0;
            sheet.show_gridlines = grbit & 0x0002 != 0;
            sheet.show_headings = grbit & 0x0004 != 0;
            sheet.show_zeros = grbit & 0x0010 != 0;
            sheet.right_to_left = grbit & 0x0040 != 0;
            if grbit & 0x0800 != 0 {
                sheet.view = SheetViewMode::PageBreakPreview;
            }
            sheet.view_top_left = top_left;
            let frozen = grbit & 0x0008 != 0;
            match &self.pane {
                Some(p) if frozen && (p.x > 0 || p.y > 0) => {
                    sheet.freeze = Some((u32::from(p.y), u32::from(p.x.min(255))));
                    // With frozen panes the scroll position is the scrolling pane's first cell.
                    sheet.view_top_left = CellRef::new(u32::from(p.top), u32::from(p.left.min(255)));
                }
                Some(p) if !frozen && (p.x > 0 || p.y > 0) => {
                    report.warn("Split (not frozen) window panes in .xls files are not supported and were removed.")
                }
                _ => {}
            }
        }
        if self.bad_notes {
            report.warn("Some cell notes in this .xls file could not be read and were skipped.");
        }
        if self.bad_links {
            report.warn("Some hyperlinks in this .xls file could not be read and were skipped.");
        }
        // Notes are drawing objects too: warn only for the other kinds.
        if self.other_objects || (self.drawing && !self.any_object) {
            report.warn("Pictures, charts and shapes in .xls files are not imported yet.");
        }
    }
}
