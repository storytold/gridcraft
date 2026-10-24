//! Legacy Excel 97-2003 `.xls` (BIFF8) import, implemented from Microsoft's public
//! specifications [MS-CFB] (the compound file container) and [MS-XLS] (the Workbook stream):
//! <https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-xls/cd03cb5f-ca02-4934-a391-bb674cb8aa06>.
//!
//! Read: worksheets, shared strings, cell values, formulas (text and cached results, including
//! shared and array formulas), number formats, the 1904 date system, merged cells, column
//! widths, row heights, defined names, print areas, print titles and the calculation settings
//! (mode, iteration, precision as displayed). Everything else (fonts, fills, borders, charts,
//! pictures, comments, conditional formats, validation, macros) is skipped with a warning.
//! Encrypted workbooks and pre-BIFF8 versions are refused.

mod formula;
mod ftab;
mod records;

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{CalcMode, CalcSettings, Cell, DefinedName, Formula, LineInfo, NumFmt, Sheet, Style, StyleId, Visibility, Workbook};

use crate::cfb::Compound;
use crate::tables::{builtin_format, col_width_to_px, pt_to_px};
use crate::{IoError, ReadReport};
use formula::{Links, Mode, SupBook, Unsupported, Xti, cell_error, decode};
use records::{Cursor, Record, Records};

const MAX_CELLS: usize = 4_000_000;
const MAX_STRINGS: usize = 4_000_000;
const MAX_TEXT: usize = 256 * 1024 * 1024;
const MAX_SHEETS: usize = 4096;
const MAX_NAMES: usize = 65_536;
const MAX_XFS: usize = 65_536;
const MAX_MERGES: usize = 100_000;
/// Records read from the whole Workbook stream at most (a 65,536 × 256 sheet has fewer). One
/// allowance shared by every substream: many BoundSheet records pointing into one huge
/// substream must not each get their own.
const MAX_RECORDS: usize = 40_000_000;

const BOF: u16 = 0x0809;
const EOF: u16 = 0x000A;
const BIFF8: u16 = 0x0600;
/// WsBool (MS-XLS 2.4.351): its first byte's `fDialog` bit marks a dialog sheet, whose BOF says worksheet.
const WSBOOL: u16 = 0x0081;
const WSBOOL_DIALOG: u8 = 0x10;

pub(crate) fn invalid(msg: impl Into<String>) -> IoError {
    IoError::Format(format!("XLS: {}", msg.into()))
}

/// The message for an encrypted workbook (a FilePass record).
const ENCRYPTED: &str =
    "this .xls workbook is password-protected (encrypted). GridCraft can't open encrypted .xls files; remove the password in Excel and try again.";

/// Whether the bytes are a compound file holding an Excel workbook stream (BIFF8 `Workbook`
/// or BIFF5 `Book`).
pub(crate) fn is_xls(bytes: &[u8]) -> bool {
    let Ok(cf) = Compound::open(bytes) else { return false };
    cf.has_root_stream("Workbook") || cf.has_root_stream("Book")
}

pub(crate) struct Budget {
    cells: usize,
    text: usize,
    records: usize,
    max_records: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Budget { cells: 0, text: 0, records: 0, max_records: MAX_RECORDS }
    }
}

impl Budget {
    /// A budget with a smaller record allowance (tests).
    #[cfg(test)]
    pub(crate) fn with_max_records(max_records: usize) -> Self {
        Budget { max_records, ..Budget::default() }
    }
    fn record(&mut self) -> Result<(), IoError> {
        self.records = self.records.saturating_add(1);
        if self.records > self.max_records {
            return Err(IoError::TooLarge("XLS: too many records".into()));
        }
        Ok(())
    }
    fn cell(&mut self) -> Result<(), IoError> {
        self.cells += 1;
        if self.cells > MAX_CELLS {
            return Err(IoError::TooLarge("XLS: more than 4 million cells".into()));
        }
        Ok(())
    }
    fn text(&mut self, len: usize) -> Result<(), IoError> {
        self.text = self.text.saturating_add(len);
        if self.text > MAX_TEXT {
            return Err(IoError::TooLarge("XLS: more than 256 MB of text".into()));
        }
        Ok(())
    }
}

struct BoundSheet {
    pos: usize,
    name: String,
    visibility: Visibility,
    kind: u8,
}

struct Lbl {
    name: String,
    builtin: Option<u8>,
    hidden: bool,
    macro_name: bool,
    itab: u16,
    rgce: Vec<u8>,
    rgcb: Vec<u8>,
}

#[derive(Default)]
struct Globals {
    sheets: Vec<BoundSheet>,
    date1904: bool,
    formats: HashMap<u16, String>,
    xf_formats: Vec<u16>,
    sst: Vec<Arc<str>>,
    links: Links,
    names: Vec<Lbl>,
    precision_as_displayed: bool,
}

/// Reads an Excel 97-2003 workbook. Inspect the report's warnings for what was skipped.
pub fn read_xls(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    read_xls_with(bytes, Budget::default())
}

/// [`read_xls`] with an explicit resource budget.
pub(crate) fn read_xls_with(bytes: &[u8], mut budget: Budget) -> Result<(Workbook, ReadReport), IoError> {
    let cf = Compound::open(bytes).map_err(invalid)?;
    let stream = match cf.root_stream("Workbook").map_err(invalid)? {
        Some(s) => s,
        None => {
            if cf.has_root_stream("Book") {
                return Err(invalid("Excel 5.0/95 workbooks (BIFF5) are not supported; save the file as .xlsx in Excel and try again."));
            }
            return Err(invalid("the file has no Workbook stream, so it is not an Excel workbook"));
        }
    };
    let mut report = ReadReport::default();
    let g = globals(&stream, &mut budget, &mut report)?;

    let mut wb = Workbook::new();
    wb.sheets.clear();
    wb.date_system = if g.date1904 { DateSystem::D1904 } else { DateSystem::D1900 };
    // One model style per XF: only the number format is imported.
    let styles: Vec<StyleId> = g
        .xf_formats
        .iter()
        .map(|ifmt| {
            let code = g.formats.get(ifmt).map(String::as_str).or_else(|| builtin_format(u32::from(*ifmt))).unwrap_or("General");
            if code.eq_ignore_ascii_case("General") {
                StyleId::DEFAULT
            } else {
                wb.styles.intern(Style { num_fmt: NumFmt::new(code), ..Style::default() })
            }
        })
        .collect();

    // BoundSheet8 index → model sheet index.
    let mut model_index: Vec<Option<usize>> = Vec::with_capacity(g.sheets.len());
    for bs in &g.sheets {
        if bs.kind != 0 {
            report.warn(match bs.kind {
                2 => "Chart sheets in .xls files are not imported yet and were skipped.",
                1 => "Excel 4.0 macro sheets were skipped.",
                _ => "VBA modules and other non-worksheet sheets were skipped.",
            });
            model_index.push(None);
            continue;
        }
        wb.check_sheet_name(&bs.name, None).map_err(invalid)?;
        let Some((mut sheet, calc)) = worksheet(&stream, bs, &g, &styles, &mut budget, &mut report)? else {
            report.warn("Dialog sheets and unreadable sheets were skipped.");
            model_index.push(None);
            continue;
        };
        sheet.visibility = bs.visibility;
        // Every worksheet repeats the workbook's calculation settings; Excel applies the first.
        if wb.sheets.is_empty() {
            wb.calc = calc;
        }
        model_index.push(Some(wb.sheets.len()));
        wb.sheets.push(Arc::new(sheet));
    }
    if wb.sheets.is_empty() {
        return Err(invalid("the workbook has no worksheets GridCraft can read"));
    }
    wb.calc.precision_as_displayed = g.precision_as_displayed;
    defined_names(&g, &model_index, &mut wb, &mut report);
    wb.active_sheet = wb.sheets.iter().position(|s| s.visibility == Visibility::Visible).unwrap_or(0);
    // The overview comes first.
    report.warnings.insert(
        0,
        "Imported from an Excel 97-2003 (.xls) workbook: values, formulas, number formats, merged cells, column widths, row heights and defined names were read. Fonts, fills, borders, charts, pictures, comments and other features are not imported yet. Save as .xlsx; saving .xls is not supported.".into(),
    );
    Ok((wb, report))
}

fn bof(rec: &Record<'_>) -> Result<(u16, u16), IoError> {
    if rec.id != BOF {
        return Err(invalid("a substream does not start with a BOF record"));
    }
    let mut c = Cursor::new(&rec.parts);
    Ok((c.u16()?, c.u16()?))
}

fn globals(stream: &[u8], budget: &mut Budget, report: &mut ReadReport) -> Result<Globals, IoError> {
    let mut recs = Records::new(stream, 0);
    let first = recs.next_record()?.ok_or_else(|| invalid("empty Workbook stream"))?;
    let (vers, dt) = bof(&first)?;
    if vers != BIFF8 {
        return Err(invalid(if vers == 0x0500 {
            "Excel 5.0/95 workbooks (BIFF5) are not supported; save the file as .xlsx in Excel and try again."
        } else {
            "unsupported BIFF version (only Excel 97-2003 BIFF8 files are read)"
        }));
    }
    if dt != 0x0005 {
        return Err(invalid("the Workbook stream does not start with the workbook globals"));
    }
    let mut g = Globals::default();
    let mut offsets = std::collections::HashSet::new();
    while let Some(rec) = recs.next_record()? {
        budget.record()?;
        let mut c = Cursor::new(&rec.parts);
        match rec.id {
            EOF => break,
            0x002F => return Err(invalid(ENCRYPTED)),
            0x0085 => {
                if g.sheets.len() >= MAX_SHEETS {
                    return Err(IoError::TooLarge("XLS: too many sheets".into()));
                }
                let pos = c.u32()? as usize;
                if !offsets.insert(pos) {
                    return Err(invalid("two sheets point at the same substream"));
                }
                let state = c.u8()?;
                let kind = c.u8()?;
                let name = c.short_string()?;
                let visibility = match state & 3 {
                    0 => Visibility::Visible,
                    1 => Visibility::Hidden,
                    _ => Visibility::VeryHidden,
                };
                g.links.sheets.push(name.clone());
                g.sheets.push(BoundSheet { pos, name, visibility, kind });
            }
            0x0022 => g.date1904 = c.u16()? == 1,
            0x000E => g.precision_as_displayed = c.u16()? == 0,
            0x041E => {
                let ifmt = c.u16()?;
                let cch = usize::from(c.u16()?);
                let high = c.u8()? & 1 != 0;
                // Some writers count one character more than the record holds; Excel opens
                // those files, so keep the characters that are there.
                let fit = c.remaining() / if high { 2 } else { 1 };
                if cch > fit {
                    report.warn("Some number formats were shorter than their declared length; the characters present were kept.");
                }
                let code = c.chars(cch.min(fit), high)?;
                budget.text(code.len())?;
                g.formats.insert(ifmt, code);
            }
            0x00E0 if g.xf_formats.len() < MAX_XFS => {
                c.u16()?;
                g.xf_formats.push(c.u16()?);
            }
            0x00FC => g.sst = shared_strings(&mut c, budget)?,
            0x01AE => {
                let _ctab = c.u16()?;
                let cch = c.u16()?;
                g.links.books.push(SupBook { internal: cch == 0x0401, addin: cch == 0x3A01, names: vec![] });
            }
            0x0023 => {
                // ExternName: flags, then an AddinUdf or ExternDocName body; both start with
                // four bytes before the name.
                c.u16()?;
                c.skip(4)?;
                let name = c.short_string()?;
                if let Some(book) = g.links.books.last_mut()
                    && book.names.len() < MAX_NAMES
                {
                    book.names.push(name);
                }
            }
            0x0017 => {
                let n = usize::from(c.u16()?);
                for _ in 0..n.min(c.remaining() / 6) {
                    let book = c.u16()?;
                    let first = c.u16()? as i16;
                    let last = c.u16()? as i16;
                    g.links.xti.push(Xti { book, first, last });
                }
            }
            0x0018 => {
                if g.names.len() >= MAX_NAMES {
                    continue;
                }
                let lbl = label(&mut c)?;
                g.links.names.push(lbl.name.clone());
                g.names.push(lbl);
            }
            _ => {}
        }
    }
    g.links.macros = g.names.iter().map(|l| l.macro_name).collect();
    // The same test `defined_names` applies, before any cell formula is decoded.
    let skipped = g.names.iter().map(|l| !l.rgce.is_empty() && decode(&l.rgce, &l.rgcb, CellRef::new(0, 0), Mode::Name, &g.links).is_err()).collect();
    g.links.skipped = skipped;
    Ok(g)
}

/// Built-in defined names ([MS-XLS] 2.4.150, Lbl.Name).
const BUILTIN_NAMES: [&str; 14] = [
    "Consolidate_Area",
    "Auto_Open",
    "Auto_Close",
    "Extract",
    "Database",
    "Criteria",
    "Print_Area",
    "Print_Titles",
    "Recorder",
    "Data_Form",
    "Auto_Activate",
    "Auto_Deactivate",
    "Sheet_Title",
    "_FilterDatabase",
];

fn label(c: &mut Cursor<'_, '_>) -> Result<Lbl, IoError> {
    let flags = c.u16()?;
    let _key = c.u8()?;
    let cch = usize::from(c.u8()?);
    let cce = usize::from(c.u16()?);
    c.skip(2)?;
    let itab = c.u16()?;
    c.skip(4)?;
    let high = c.u8()? & 1 != 0;
    let raw = c.chars(cch, high)?;
    let builtin = flags & 0x0020 != 0;
    let (name, builtin) = if builtin {
        let code = raw.chars().next().map_or(0, |ch| ch as u32);
        let code = u8::try_from(code).unwrap_or(u8::MAX);
        (BUILTIN_NAMES.get(usize::from(code)).map_or_else(|| raw.clone(), |n| (*n).to_string()), Some(code))
    } else {
        (raw, None)
    };
    let rest = c.rest();
    let rgce = rest.get(..cce).unwrap_or(&rest).to_vec();
    let rgcb = rest.get(cce..).unwrap_or(&[]).to_vec();
    Ok(Lbl { name, builtin, hidden: flags & 1 != 0, macro_name: flags & 0x000A != 0, itab, rgce, rgcb })
}

fn shared_strings(c: &mut Cursor<'_, '_>, budget: &mut Budget) -> Result<Vec<Arc<str>>, IoError> {
    let _total = c.u32()?;
    let unique = c.u32()? as usize;
    if unique > MAX_STRINGS {
        return Err(IoError::TooLarge("XLS: too many shared strings".into()));
    }
    // Each string takes at least three bytes: the declared count can't exceed what's there.
    let mut out = Vec::with_capacity(unique.min(c.remaining() / 3));
    for _ in 0..unique {
        if c.remaining() == 0 {
            break;
        }
        let s = c.rich_string()?;
        budget.text(s.len())?;
        out.push(Arc::from(s));
    }
    Ok(out)
}

/// A formula cell waiting for its shared or array formula (which follows it in the file).
struct Pending {
    pos: CellRef,
    anchor: CellRef,
}

/// A parsed formula: tokens and extra data.
struct Parsed {
    rgce: Vec<u8>,
    rgcb: Vec<u8>,
}

fn parsed(c: &mut Cursor<'_, '_>) -> Result<Parsed, IoError> {
    let cce = usize::from(c.u16()?);
    let rest = c.rest();
    if cce > rest.len() {
        return Err(invalid("formula runs past the end of its record"));
    }
    Ok(Parsed { rgce: rest.get(..cce).unwrap_or(&[]).to_vec(), rgcb: rest.get(cce..).unwrap_or(&[]).to_vec() })
}

struct SheetState<'g> {
    g: &'g Globals,
    styles: &'g [StyleId],
    sheet: Sheet,
    pending: Vec<Pending>,
    shared: HashMap<CellRef, Parsed>,
    arrays: HashMap<CellRef, (RangeRef, Parsed)>,
    last_formula: Option<CellRef>,
    /// The cell waiting for its String record, with its style: a blank cell isn't stored, so a
    /// formula kept as its value may have no cell to receive the text.
    pending_string: Option<(CellRef, StyleId)>,
    dropped: usize,
    unsupported: Vec<&'static str>,
    calc: CalcSettings,
}

impl SheetState<'_> {
    /// Records why a formula was kept as its value (each reason once).
    fn unsupported(&mut self, why: &'static str) {
        if !self.unsupported.contains(&why) {
            self.unsupported.push(why);
        }
    }

    fn style(&self, ixfe: u16) -> StyleId {
        self.styles.get(usize::from(ixfe)).copied().unwrap_or(StyleId::DEFAULT)
    }

    fn pos(&mut self, row: u16, col: u16) -> Option<CellRef> {
        let (r, c) = (u32::from(row), u32::from(col));
        if r >= MAX_ROWS || c >= MAX_COLS.min(256) {
            self.dropped += 1;
            return None;
        }
        Some(CellRef::new(r, c))
    }

    fn put(&mut self, budget: &mut Budget, row: u16, col: u16, ixfe: u16, value: Value) -> Result<Option<CellRef>, IoError> {
        let Some(pos) = self.pos(row, col) else { return Ok(None) };
        let style = self.style(ixfe);
        if value.is_empty() && style == StyleId::DEFAULT {
            return Ok(Some(pos));
        }
        budget.cell()?;
        self.sheet.cells.set(pos, Cell { value, formula: None, style });
        Ok(Some(pos))
    }

    /// Decodes a formula and attaches it to the cell at `pos`, or records why it can't be.
    fn attach(&mut self, pos: CellRef, p: &Parsed, mode: Mode, array: Option<RangeRef>) {
        match decode(&p.rgce, &p.rgcb, pos, mode, &self.g.links) {
            Ok(e) => {
                let text = gridcraft_formula::print(&e);
                let mut f = Formula::new(&text);
                if f.expr.is_none() {
                    self.unsupported("syntax GridCraft can't parse yet");
                    return;
                }
                f.array = array.filter(|r| !r.is_single());
                if let Some(cell) = self.sheet.cells.get_mut(pos) {
                    cell.formula = Some(Arc::new(f));
                } else {
                    self.sheet.cells.set(pos, Cell { value: Value::Empty, formula: Some(Arc::new(f)), style: StyleId::DEFAULT });
                }
            }
            Err(Unsupported(why)) => self.unsupported(why),
        }
    }
}

/// RK number ([MS-XLS] 2.5.217): a 30-bit integer or the top of an IEEE double, maybe ×100.
fn rk(v: u32) -> Value {
    let n = if v & 2 != 0 { f64::from((v as i32) >> 2) } else { f64::from_bits(u64::from(v & !3) << 32) };
    Value::number(if v & 1 != 0 { n / 100.0 } else { n })
}

fn worksheet(
    stream: &[u8],
    bs: &BoundSheet,
    g: &Globals,
    styles: &[StyleId],
    budget: &mut Budget,
    report: &mut ReadReport,
) -> Result<Option<(Sheet, CalcSettings)>, IoError> {
    if bs.pos >= stream.len() {
        return Err(invalid(format!("sheet '{}' points outside the Workbook stream", bs.name)));
    }
    let mut recs = Records::new(stream, bs.pos);
    let first = recs.next_record()?.ok_or_else(|| invalid("missing sheet substream"))?;
    let (vers, dt) = bof(&first)?;
    if vers != BIFF8 || dt != 0x0010 {
        return Ok(None);
    }
    let mut st = SheetState {
        g,
        styles,
        sheet: Sheet::new(&bs.name),
        pending: vec![],
        shared: HashMap::new(),
        arrays: HashMap::new(),
        last_formula: None,
        pending_string: None,
        dropped: 0,
        unsupported: vec![],
        calc: CalcSettings::default(),
    };
    let mut depth = 1usize;
    while let Some(rec) = recs.next_record()? {
        budget.record()?;
        match rec.id {
            BOF => {
                // An embedded chart's substream.
                depth += 1;
                continue;
            }
            EOF => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
                continue;
            }
            _ if depth > 1 => continue,
            WSBOOL if rec.parts.first().and_then(|p| p.first()).is_some_and(|b| b & WSBOOL_DIALOG != 0) => return Ok(None),
            _ => {}
        }
        sheet_record(&rec, &mut st, budget, report)?;
    }
    // Shared and array formulas: their definitions follow the first cell that uses them.
    let (arrays, shared) = (std::mem::take(&mut st.arrays), std::mem::take(&mut st.shared));
    let pending = std::mem::take(&mut st.pending);
    for p in &pending {
        if let Some((range, f)) = arrays.get(&p.anchor) {
            if p.pos == p.anchor {
                st.attach(p.pos, f, Mode::Cell, Some(*range));
            }
        } else if let Some(f) = shared.get(&p.anchor) {
            st.attach(p.pos, f, Mode::Shared, None);
        } else {
            st.unsupported("a shared formula that is missing from the file");
        }
    }
    // The other cells of a legacy array formula hold only cached results: dropped, as when an
    // .xlsx is read, so that the formula fills its range on recalculation. If the formula itself
    // couldn't be imported, they keep their values.
    for p in &pending {
        let filled = st.sheet.cell(p.anchor).and_then(|c| c.formula.as_ref()).and_then(|f| f.array).is_some_and(|r| r.contains(p.pos));
        if p.pos != p.anchor && filled {
            let style = st.sheet.style_id(p.pos);
            st.sheet.cells.set(p.pos, Cell { value: Value::Empty, formula: None, style });
        }
    }
    if st.dropped > 0 {
        report.warn(format!("{} cells outside the grid were skipped.", st.dropped));
    }
    for why in &st.unsupported {
        report.warn(format!("Some formulas use {why}; their saved results were kept as values."));
    }
    Ok(Some((st.sheet, st.calc)))
}

/// The shortest WINDOW2 a file holds (the chart-sheet form); a shorter one is truncated.
const WINDOW2_MIN_LEN: usize = 10;

fn sheet_record(rec: &Record<'_>, st: &mut SheetState<'_>, budget: &mut Budget, report: &mut ReadReport) -> Result<(), IoError> {
    let mut c = Cursor::new(&rec.parts);
    // A formula's string result is the String record right after it (shared/array records may sit between).
    if !matches!(rec.id, 0x0207 | 0x04BC | 0x0221 | 0x0236) {
        st.pending_string = None;
    }
    match rec.id {
        0x0203 => {
            let (r, col, x) = (c.u16()?, c.u16()?, c.u16()?);
            let v = Value::number(c.f64()?);
            st.put(budget, r, col, x, v)?;
        }
        0x027E => {
            let (r, col, x) = (c.u16()?, c.u16()?, c.u16()?);
            let v = rk(c.u32()?);
            st.put(budget, r, col, x, v)?;
        }
        0x00BD => {
            let r = c.u16()?;
            let first = c.u16()?;
            let n = c.remaining().saturating_sub(2) / 6;
            for i in 0..n {
                let x = c.u16()?;
                let v = rk(c.u32()?);
                let col = u16::try_from(usize::from(first) + i).unwrap_or(u16::MAX);
                st.put(budget, r, col, x, v)?;
            }
        }
        0x00FD => {
            let (r, col, x) = (c.u16()?, c.u16()?, c.u16()?);
            let i = c.u32()? as usize;
            match st.g.sst.get(i) {
                Some(s) => {
                    st.put(budget, r, col, x, Value::Text(s.clone()))?;
                }
                None => report.warn("Some cells refer to missing shared strings and were left empty."),
            }
        }
        0x0204 => {
            let (r, col, x) = (c.u16()?, c.u16()?, c.u16()?);
            let s = c.string()?;
            budget.text(s.len())?;
            st.put(budget, r, col, x, Value::from(s))?;
        }
        0x0205 => {
            let (r, col, x) = (c.u16()?, c.u16()?, c.u16()?);
            let (v, is_err) = (c.u8()?, c.u8()?);
            let v = if is_err != 0 { Value::Error(cell_error(v)) } else { Value::Bool(v != 0) };
            st.put(budget, r, col, x, v)?;
        }
        0x0201 => {
            let (r, col, x) = (c.u16()?, c.u16()?, c.u16()?);
            st.put(budget, r, col, x, Value::Empty)?;
        }
        0x00BE => {
            let r = c.u16()?;
            let first = c.u16()?;
            let n = c.remaining().saturating_sub(2) / 2;
            for i in 0..n {
                let x = c.u16()?;
                let col = u16::try_from(usize::from(first) + i).unwrap_or(u16::MAX);
                st.put(budget, r, col, x, Value::Empty)?;
            }
        }
        0x0006 => formula_cell(&mut c, st, budget)?,
        0x0207 => {
            if let Some((pos, style)) = st.pending_string.take() {
                let s = c.string()?;
                budget.text(s.len())?;
                if let Some(cell) = st.sheet.cells.get_mut(pos) {
                    cell.value = Value::from(s);
                } else {
                    st.sheet.cells.set(pos, Cell { value: Value::from(s), formula: None, style });
                }
            }
        }
        0x04BC => {
            // RefU (the cells that may use it), reserved, cUse.
            c.skip(8)?;
            let f = parsed(&mut c)?;
            if let Some(at) = st.last_formula {
                st.shared.insert(at, f);
            }
        }
        0x0221 => {
            let (r0, r1) = (c.u16()?, c.u16()?);
            let (c0, c1) = (c.u8()?, c.u8()?);
            c.skip(6)?;
            let f = parsed(&mut c)?;
            let range = RangeRef::new(CellRef::new(u32::from(r0), u32::from(c0)), CellRef::new(u32::from(r1.max(r0)), u32::from(c1.max(c0))));
            if let Some(at) = st.last_formula {
                st.arrays.insert(at, (range, f));
            }
        }
        0x000D => {
            st.calc.mode = match c.u16()? as i16 {
                0 => CalcMode::Manual,
                -1 => CalcMode::AutomaticExceptTables,
                _ => CalcMode::Automatic,
            }
        }
        0x000C => st.calc.set_max_iterations(u32::from(c.u16()?)),
        0x0011 => st.calc.iterative = c.u16()? == 1,
        0x0010 => st.calc.set_max_change(c.f64()?),
        0x0236 => report.warn("What-if data tables are not supported; their values were kept."),
        0x00E5 => {
            let n = usize::from(c.u16()?);
            for _ in 0..n.min(c.remaining() / 8) {
                let (r0, r1, c0, c1) = (c.u16()?, c.u16()?, c.u16()?, c.u16()?);
                if r1 < r0 || c1 < c0 || u32::from(c1) >= 256 {
                    continue;
                }
                if st.sheet.merges.len() >= MAX_MERGES {
                    return Err(IoError::TooLarge("XLS: too many merged ranges".into()));
                }
                let range = RangeRef::new(CellRef::new(u32::from(r0), u32::from(c0)), CellRef::new(u32::from(r1), u32::from(c1)));
                if !range.is_single() {
                    st.sheet.merges.push(range);
                }
            }
        }
        0x007D => {
            let (first, last) = (c.u16()?, c.u16()?);
            let width = c.u16()?;
            let x = c.u16()?;
            let flags = c.u16()?;
            let last = last.min(255);
            if first > last {
                return Ok(());
            }
            let mut info = LineInfo::default();
            let px = col_width_to_px(f64::from(width) / 256.0);
            if flags & 0x0002 != 0 || px != st.sheet.default_col_width {
                info.size = Some(px);
            }
            info.hidden = flags & 0x0001 != 0;
            info.outline = ((flags >> 8) & 7) as u8;
            info.collapsed = flags & 0x1000 != 0;
            let style = st.style(x);
            if style != StyleId::DEFAULT {
                info.style = Some(style);
            }
            if info != LineInfo::default() {
                for col in first..=last {
                    st.sheet.cols.insert(u32::from(col), info);
                }
            }
        }
        0x0208 => {
            let r = c.u16()?;
            c.skip(4)?;
            let height = c.u16()?;
            c.skip(4)?;
            let flags = c.u32()?;
            let mut info = LineInfo { custom: flags & 0x40 != 0, hidden: flags & 0x20 != 0, ..LineInfo::default() };
            let px = pt_to_px(f64::from(height & 0x7FFF) / 20.0);
            if info.custom || (px - st.sheet.default_row_height).abs() > 0.5 {
                info.size = Some(px);
            }
            info.outline = (flags & 7) as u8;
            info.collapsed = flags & 0x10 != 0;
            if flags & 0x80 != 0 {
                let style = st.style(((flags >> 16) & 0x0FFF) as u16);
                if style != StyleId::DEFAULT {
                    info.style = Some(style);
                }
            }
            if info != LineInfo::default() && u32::from(r) < MAX_ROWS {
                st.sheet.rows.insert(u32::from(r), info);
            }
        }
        0x0225 => {
            let flags = c.u16()?;
            let height = c.u16()?;
            if flags & 0x0002 == 0 && height > 0 {
                st.sheet.default_row_height = pt_to_px(f64::from(height) / 20.0);
            }
        }
        0x023E => {
            let flags = if c.remaining() >= WINDOW2_MIN_LEN { c.u16().ok() } else { None };
            let Some(flags) = flags else { return Ok(()) };
            let on = |bit: u16| flags & bit != 0;
            st.sheet.show_formulas = on(0x0001);
            st.sheet.show_gridlines = on(0x0002);
            st.sheet.show_headings = on(0x0004);
            st.sheet.show_zeros = on(0x0010);
            st.sheet.right_to_left = on(0x0040);
        }
        0x001C => report.warn("Cell comments in .xls files are not imported yet."),
        0x00EC => report.warn("Pictures, charts and shapes in .xls files are not imported yet."),
        0x01B0 | 0x0879 => report.warn("Conditional formatting in .xls files is not imported yet."),
        0x01B2 => report.warn("Data validation in .xls files is not imported yet."),
        0x01B8 => report.warn("Hyperlinks in .xls files are not imported yet."),
        0x00B0 => report.warn("PivotTables in .xls files are not imported yet; their values were kept."),
        _ => {}
    }
    Ok(())
}

fn formula_cell(c: &mut Cursor<'_, '_>, st: &mut SheetState<'_>, budget: &mut Budget) -> Result<(), IoError> {
    let (r, col, x) = (c.u16()?, c.u16()?, c.u16()?);
    let mut val = [0u8; 8];
    for b in val.iter_mut() {
        *b = c.u8()?;
    }
    let _flags = c.u16()?;
    c.skip(4)?;
    let f = parsed(c)?;
    let [b0, _, b2, _, _, _, e0, e1] = val;
    let mut string_result = false;
    let value = if [e0, e1] == [0xFF, 0xFF] {
        match b0 {
            0x00 => {
                string_result = true;
                Value::Empty
            }
            0x01 => Value::Bool(b2 != 0),
            0x02 => Value::Error(cell_error(b2)),
            0x03 => Value::from(""),
            _ => Value::Empty,
        }
    } else {
        Value::number(f64::from_le_bytes(val))
    };
    let Some(pos) = st.pos(r, col) else { return Ok(()) };
    budget.cell()?;
    let style = st.style(x);
    st.sheet.cells.set(pos, Cell { value, formula: None, style });
    st.last_formula = Some(pos);
    if string_result {
        st.pending_string = Some((pos, style));
    }
    // PtgExp: part of a shared or array formula anchored at another cell.
    if f.rgce.len() == 5 && f.rgce.first() == Some(&0x01) {
        let get = |i: usize| f.rgce.get(i).copied().unwrap_or(0);
        let anchor = CellRef::new(u32::from(u16::from_le_bytes([get(1), get(2)])), u32::from(u16::from_le_bytes([get(3), get(4)])));
        st.pending.push(Pending { pos, anchor });
        return Ok(());
    }
    st.attach(pos, &f, Mode::Cell, None);
    Ok(())
}

fn defined_names(g: &Globals, model_index: &[Option<usize>], wb: &mut Workbook, report: &mut ReadReport) {
    let scope_of = |itab: u16| -> Option<Option<usize>> {
        if itab == 0 {
            return Some(None);
        }
        model_index.get(usize::from(itab) - 1).copied().flatten().map(Some)
    };
    for lbl in &g.names {
        let Some(scope) = scope_of(lbl.itab) else { continue };
        let text = match decode(&lbl.rgce, &lbl.rgcb, CellRef::new(0, 0), Mode::Name, &g.links) {
            Ok(e) => gridcraft_formula::print(&e),
            Err(_) if lbl.rgce.is_empty() => continue,
            Err(Unsupported(why)) => {
                report.warn(format!("Some defined names use {why} and were skipped."));
                continue;
            }
        };
        match lbl.builtin {
            Some(6 | 7) => {
                let Some(si) = scope else { continue };
                let Some(sheet) = wb.sheet_mut(si) else { continue };
                if lbl.builtin == Some(6) {
                    sheet.print.print_area = crate::read::parse_print_area(&text);
                } else {
                    let (rows, cols) = crate::read::parse_print_titles(&text);
                    sheet.print.title_rows = rows;
                    sheet.print.title_cols = cols;
                }
            }
            Some(_) => {}
            None if lbl.macro_name => report.warn("Macro names were skipped."),
            None => wb.names.push(DefinedName { name: lbl.name.clone(), scope, formula: text, comment: String::new(), hidden: lbl.hidden }),
        }
    }
}
