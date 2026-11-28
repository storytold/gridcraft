//! Direct streaming `quick-xml` reader for worksheet XML parts.
//!
//! Parses `<sheetData>` rows and cells straight from XML byte events into `Sheet` without
//! constructing intermediate `El` trees or buffering `Vec<Row>`, while collecting the small
//! non-`<sheetData>` worksheet children (`cols`, `sheetViews`, `mergeCells`, `conditionalFormatting`,
//! `autoFilter`, `printOptions`, `extLst`, …) into a compact `xml::El` root.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{LineInfo, Sheet};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use super::{RowState, StreamedFormula, StreamedFormulaKind, apply_streamed_cell, charge_cells};
use crate::IoError;
use crate::read::Ctx;
use crate::tables::pt_to_px;
use crate::xml::{self, El, MAX_DEPTH, MAX_XML_NODES};

#[inline]
fn local_bytes(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => name.get(i + 1..).unwrap_or(name),
        None => name,
    }
}

/// Fast ASCII parser for cell references like `A1`, `XFD1048576` without allocating a `String`.
pub(super) fn parse_cell_ref_bytes(raw: &[u8]) -> Option<CellRef> {
    let b = trim_ascii(raw);
    if b.is_empty() {
        return None;
    }
    let mut i = 0usize;
    let mut col = 0u32;
    let mut has_col = false;
    while i < b.len() {
        let ch = b[i];
        let digit = if ch.is_ascii_uppercase() {
            (ch - b'A') as u32 + 1
        } else if ch.is_ascii_lowercase() {
            (ch - b'a') as u32 + 1
        } else {
            break;
        };
        has_col = true;
        col = col.checked_mul(26)?.checked_add(digit)?;
        if col > MAX_COLS {
            return None;
        }
        i += 1;
    }
    if !has_col || i >= b.len() {
        return None;
    }
    let mut row = 0u32;
    let mut has_row = false;
    while i < b.len() {
        let ch = b[i];
        if !ch.is_ascii_digit() {
            return None;
        }
        has_row = true;
        row = row.checked_mul(10)?.checked_add((ch - b'0') as u32)?;
        if row > MAX_ROWS {
            return None;
        }
        i += 1;
    }
    if !has_row || row == 0 || col == 0 {
        return None;
    }
    Some(CellRef::new(row - 1, col - 1))
}

#[inline]
fn trim_ascii(mut b: &[u8]) -> &[u8] {
    while let Some(&first) = b.first() {
        if first.is_ascii_whitespace() {
            b = &b[1..];
        } else {
            break;
        }
    }
    while let Some(&last) = b.last() {
        if last.is_ascii_whitespace() {
            b = &b[..b.len() - 1];
        } else {
            break;
        }
    }
    b
}

#[inline]
fn parse_u32_bytes(raw: &[u8]) -> Option<u32> {
    let b = trim_ascii(raw);
    if b.is_empty() {
        return None;
    }
    let mut n = 0u32;
    for &ch in b {
        if !ch.is_ascii_digit() {
            return None;
        }
        n = n.checked_mul(10)?.checked_add((ch - b'0') as u32)?;
    }
    Some(n)
}

#[inline]
fn parse_f64_bytes(raw: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(trim_ascii(raw)).ok()?;
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

#[inline]
fn parse_bool_bytes(raw: &[u8]) -> Option<bool> {
    match trim_ascii(raw) {
        b"1" | b"true" | b"TRUE" | b"on" | b"t" => Some(true),
        b"0" | b"false" | b"FALSE" | b"off" | b"f" => Some(false),
        _ => None,
    }
}

#[inline]
fn append_normalized_str(dst: &mut String, s: &str) {
    if !s.as_bytes().contains(&b'\r') {
        dst.push_str(s);
        return;
    }
    let bytes = s.as_bytes();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'\r' {
            dst.push_str(&s[start..i]);
            dst.push('\n');
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
            } else {
                i += 1;
            }
            start = i;
        } else {
            i += 1;
        }
    }
    dst.push_str(&s[start..]);
}

#[inline]
fn append_xml_bytes(dst: &mut String, raw: &[u8]) {
    match std::str::from_utf8(raw) {
        Ok(s) => append_normalized_str(dst, s),
        Err(_) => {
            let lossy = String::from_utf8_lossy(raw);
            append_normalized_str(dst, &lossy);
        }
    }
}

#[inline]
fn append_text_event(dst: &mut String, t: &quick_xml::events::BytesText<'_>) {
    append_xml_bytes(dst, t.as_ref());
}

#[inline]
fn append_general_ref(dst: &mut String, r: &quick_xml::events::BytesRef<'_>) {
    match r.resolve_char_ref() {
        Ok(Some(c)) => dst.push(c),
        _ => {
            let name = String::from_utf8_lossy(r);
            match xml::named_entity(&name) {
                Some(c) => dst.push(c),
                None => {
                    dst.push('&');
                    dst.push_str(&name);
                    dst.push(';');
                }
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellType {
    Number,
    Shared,
    Str,
    InlineStr,
    Bool,
    Error,
    Date,
}

#[derive(Clone, PartialEq, Eq)]
enum SdTag {
    Row,
    C,
    V,
    F,
    Is,
    R,
    T,
    RPh,
    Other(Vec<u8>),
}

impl SdTag {
    #[inline]
    fn from_local(name: &[u8]) -> Self {
        match name {
            b"row" => Self::Row,
            b"c" => Self::C,
            b"v" => Self::V,
            b"f" => Self::F,
            b"is" => Self::Is,
            b"r" => Self::R,
            b"t" => Self::T,
            b"rPh" => Self::RPh,
            _ => Self::Other(name.to_vec()),
        }
    }

    #[inline]
    fn matches(&self, local: &[u8]) -> bool {
        match self {
            Self::Row => local == b"row",
            Self::C => local == b"c",
            Self::V => local == b"v",
            Self::F => local == b"f",
            Self::Is => local == b"is",
            Self::R => local == b"r",
            Self::T => local == b"t",
            Self::RPh => local == b"rPh",
            Self::Other(b) => b.as_slice() == local,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CellSub {
    None,
    V,
    F,
    Is { in_t: bool },
    Other,
}

struct CellAcc {
    pos: Option<CellRef>,
    style: u32,
    ty: CellType,
    cm: Option<u32>,
    vm: Option<u32>,
    has_v: bool,
    has_is: bool,
    has_f: bool,
    f_kind: StreamedFormulaKind,
    f_si: Option<u32>,
    f_ref: Option<RangeRef>,
    sub: CellSub,
}

impl Default for CellAcc {
    fn default() -> Self {
        Self {
            pos: None,
            style: 0,
            ty: CellType::Number,
            cm: None,
            vm: None,
            has_v: false,
            has_is: false,
            has_f: false,
            f_kind: StreamedFormulaKind::Normal,
            f_si: None,
            f_ref: None,
            sub: CellSub::None,
        }
    }
}

impl CellAcc {
    fn reset(&mut self) {
        *self = Self::default();
    }
}

pub(super) struct StreamResult {
    pub root: El,
    pub rows_read: u64,
    pub total_cells: u64,
    pub dropped: u64,
    pub dropped_cells: u64,
    pub damaged: Option<String>,
}

fn parse_row_start(e: &BytesStart<'_>, next_row: &mut u32, cx: &Ctx<'_>, sheet: &mut Sheet) -> Option<u32> {
    let mut r_attr: Option<Option<u32>> = None;
    let mut ht: Option<f64> = None;
    let mut custom_height = false;
    let mut hidden = false;
    let mut outline = 0u8;
    let mut collapsed = false;
    let mut style_id: Option<u32> = None;
    let mut custom_format = false;
    for a in e.attributes().with_checks(false).flatten() {
        match local_bytes(a.key.as_ref()) {
            b"r" => r_attr = Some(parse_u32_bytes(a.value.as_ref())),
            b"ht" => ht = parse_f64_bytes(a.value.as_ref()),
            b"customHeight" => custom_height = parse_bool_bytes(a.value.as_ref()).unwrap_or(false),
            b"hidden" => hidden = parse_bool_bytes(a.value.as_ref()).unwrap_or(false),
            b"outlineLevel" => outline = parse_u32_bytes(a.value.as_ref()).unwrap_or(0).min(7) as u8,
            b"collapsed" => collapsed = parse_bool_bytes(a.value.as_ref()).unwrap_or(false),
            b"s" => style_id = parse_u32_bytes(a.value.as_ref()),
            b"customFormat" => custom_format = parse_bool_bytes(a.value.as_ref()).unwrap_or(false),
            _ => {}
        }
    }
    let index = match r_attr {
        Some(Some(r)) if (1..=MAX_ROWS).contains(&r) => r - 1,
        Some(_) => return None,
        None if *next_row < MAX_ROWS => *next_row,
        None => return None,
    };
    *next_row = index + 1;
    let mut info = LineInfo::default();
    if let Some(ht) = ht {
        info.custom = custom_height;
        if info.custom || (pt_to_px(ht) - sheet.default_row_height).abs() > 0.5 {
            info.size = Some(pt_to_px(ht));
        }
    }
    info.hidden = hidden;
    info.outline = outline;
    info.collapsed = collapsed;
    if custom_format && let Some(s) = style_id {
        info.style = Some(cx.style(Some(s)));
    }
    if info != LineInfo::default() {
        sheet.rows.insert(index, info);
    }
    Some(index)
}

fn parse_cell_start(e: &BytesStart<'_>, row_idx: u32, next_col: &mut u32, acc: &mut CellAcc) -> bool {
    acc.reset();
    let mut r_present = false;
    let mut r_parsed: Option<CellRef> = None;
    for a in e.attributes().with_checks(false).flatten() {
        match local_bytes(a.key.as_ref()) {
            b"r" => {
                r_present = true;
                r_parsed = parse_cell_ref_bytes(a.value.as_ref());
            }
            b"s" => acc.style = parse_u32_bytes(a.value.as_ref()).unwrap_or(0),
            b"t" => {
                acc.ty = match a.value.as_ref() {
                    b"s" => CellType::Shared,
                    b"str" => CellType::Str,
                    b"inlineStr" => CellType::InlineStr,
                    b"b" => CellType::Bool,
                    b"e" => CellType::Error,
                    b"d" => CellType::Date,
                    _ => CellType::Number,
                };
            }
            b"cm" => acc.cm = parse_u32_bytes(a.value.as_ref()),
            b"vm" => acc.vm = parse_u32_bytes(a.value.as_ref()),
            _ => {}
        }
    }
    let pos = if r_present {
        let Some(p) = r_parsed else { return false };
        p
    } else if *next_col < MAX_COLS {
        CellRef::new(row_idx, *next_col)
    } else {
        return false;
    };
    *next_col = pos.col + 1;
    acc.pos = Some(pos);
    true
}

fn parse_f_attrs(e: &BytesStart<'_>, acc: &mut CellAcc) {
    acc.has_f = true;
    acc.f_kind = StreamedFormulaKind::Normal;
    acc.f_si = None;
    acc.f_ref = None;
    for a in e.attributes().with_checks(false).flatten() {
        match local_bytes(a.key.as_ref()) {
            b"t" => {
                acc.f_kind = match a.value.as_ref() {
                    b"shared" => StreamedFormulaKind::Shared,
                    b"array" => StreamedFormulaKind::Array,
                    b"dataTable" => StreamedFormulaKind::DataTable,
                    _ => StreamedFormulaKind::Normal,
                };
            }
            b"si" => acc.f_si = parse_u32_bytes(a.value.as_ref()),
            b"ref" => {
                if let Ok(s) = std::str::from_utf8(a.value.as_ref()) {
                    acc.f_ref = RangeRef::parse(s.trim());
                }
            }
            _ => {}
        }
    }
}

fn decode_cell_value(cx: &mut Ctx<'_>, acc: &CellAcc, val_buf: &str, is_buf: &str) -> Value {
    match acc.ty {
        CellType::Shared => {
            if !acc.has_v {
                return Value::Empty;
            }
            match val_buf.trim().parse::<u32>() {
                Ok(idx) => match cx.sst.get(idx as usize) {
                    Some(s) => Value::Text(Arc::clone(s)),
                    None => {
                        cx.warn("some cells refer to missing shared strings");
                        Value::Empty
                    }
                },
                Err(_) => {
                    cx.warn("some numeric cells could not be read");
                    Value::Empty
                }
            }
        }
        CellType::Number => {
            let t = if acc.has_v { val_buf.trim() } else { "" };
            if t.is_empty() {
                return Value::Empty;
            }
            match t.parse::<f64>() {
                Ok(n) if n.is_finite() => Value::Number(if n == 0.0 { 0.0 } else { n }),
                _ => {
                    cx.warn("some numeric cells could not be read");
                    Value::Empty
                }
            }
        }
        CellType::Str => {
            if acc.has_v {
                Value::Text(Arc::from(xml::decode_xstring(val_buf)))
            } else {
                Value::Empty
            }
        }
        CellType::InlineStr => {
            if acc.has_is {
                Value::Text(Arc::from(xml::decode_xstring(is_buf)))
            } else if acc.has_v {
                Value::Text(Arc::from(xml::decode_xstring(val_buf)))
            } else {
                Value::Empty
            }
        }
        CellType::Bool => {
            if acc.has_v {
                Value::Bool(matches!(val_buf.trim(), "1" | "true" | "TRUE"))
            } else {
                Value::Empty
            }
        }
        CellType::Error => {
            if acc.has_v {
                Value::Error(CellError::parse(val_buf.trim()).unwrap_or(CellError::Value))
            } else {
                Value::Empty
            }
        }
        CellType::Date => {
            let d = if acc.has_v { val_buf.trim() } else { "" };
            if d.is_empty() {
                return Value::Empty;
            }
            match super::parse_iso_datetime(d, cx.date_system) {
                Some(n) => Value::number(n),
                None => {
                    cx.warn("some date cells could not be read");
                    Value::Empty
                }
            }
        }
    }
}

struct SheetStreamer {
    stack: Vec<El>,
    root: Option<El>,
    kept: usize,
    in_sheet_data: bool,
    sd_stack: Vec<SdTag>,
    next_row: u32,
    cur_row: Option<u32>,
    next_col: u32,
    row_node_count: usize,
    rows_read: u64,
    total_cells: u64,
    dropped: u64,
    dropped_cells: u64,
    val_buf: String,
    fmla_buf: String,
    is_buf: String,
    cell_acc: CellAcc,
    damaged: Option<String>,
}

impl SheetStreamer {
    fn new() -> Self {
        Self {
            stack: Vec::new(),
            root: None,
            kept: 0,
            in_sheet_data: false,
            sd_stack: Vec::with_capacity(8),
            next_row: 0,
            cur_row: None,
            next_col: 0,
            row_node_count: 0,
            rows_read: 0,
            total_cells: 0,
            dropped: 0,
            dropped_cells: 0,
            val_buf: String::with_capacity(64),
            fmla_buf: String::with_capacity(128),
            is_buf: String::new(),
            cell_acc: CellAcc::default(),
            damaged: None,
        }
    }

    fn finish_cell(&mut self, cx: &mut Ctx<'_>, sheet: &mut Sheet, st: &mut RowState, empty: bool) -> Result<(), IoError> {
        let Some(pos) = self.cell_acc.pos else { return Ok(()) };
        charge_cells(cx, 1)?;
        self.total_cells += 1;
        let (vb, ib, fb) = if empty { ("", "", "") } else { (self.val_buf.as_str(), self.is_buf.as_str(), self.fmla_buf.as_str()) };
        let value = decode_cell_value(cx, &self.cell_acc, vb, ib);
        let formula = self.cell_acc.has_f.then_some(StreamedFormula {
            kind: self.cell_acc.f_kind,
            text: fb.trim(),
            shared_index: self.cell_acc.f_si,
            range: self.cell_acc.f_ref,
        });
        apply_streamed_cell(cx, sheet, st, pos, self.cell_acc.style, value, formula, self.cell_acc.vm, self.cell_acc.cm);
        Ok(())
    }

    fn handle_sd_start(&mut self, e: &BytesStart<'_>, cx: &Ctx<'_>, sheet: &mut Sheet) -> Result<(), IoError> {
        let qname = e.name();
        let name = local_bytes(qname.as_ref());
        if self.stack.len() + self.sd_stack.len() >= MAX_DEPTH {
            return Err(IoError::Format(format!("sheet \"{}\" is damaged (XML nested too deeply)", sheet.name)));
        }
        let depth = self.sd_stack.len();
        self.sd_stack.push(SdTag::from_local(name));
        match depth {
            0 => {
                if name == b"row" {
                    self.row_node_count = 0;
                    self.cur_row = parse_row_start(e, &mut self.next_row, cx, sheet);
                    if self.cur_row.is_some() {
                        self.rows_read += 1;
                    } else {
                        self.dropped += 1;
                    }
                    self.next_col = 0;
                } else {
                    self.cur_row = None;
                }
            }
            1 => {
                self.row_node_count += 1;
                if self.row_node_count > MAX_XML_NODES {
                    return Err(IoError::Format(format!("sheet \"{}\" is damaged (more than {MAX_XML_NODES} elements in one row)", sheet.name)));
                }
                self.val_buf.clear();
                self.fmla_buf.clear();
                self.is_buf.clear();
                if name == b"c" && self.sd_stack.first() == Some(&SdTag::Row) {
                    if let Some(r) = self.cur_row {
                        if !parse_cell_start(e, r, &mut self.next_col, &mut self.cell_acc) {
                            self.dropped += 1;
                            self.dropped_cells += 1;
                        }
                    } else {
                        self.cell_acc.pos = None;
                        self.dropped_cells += 1;
                    }
                } else {
                    self.cell_acc.pos = None;
                }
            }
            2 => {
                self.row_node_count += 1;
                if self.cell_acc.pos.is_some() {
                    match name {
                        b"v" => {
                            self.cell_acc.has_v = true;
                            self.val_buf.clear();
                            self.cell_acc.sub = CellSub::V;
                        }
                        b"f" => {
                            self.fmla_buf.clear();
                            parse_f_attrs(e, &mut self.cell_acc);
                            self.cell_acc.sub = CellSub::F;
                        }
                        b"is" => {
                            self.cell_acc.has_is = true;
                            self.is_buf.clear();
                            self.cell_acc.sub = CellSub::Is { in_t: false };
                        }
                        _ => self.cell_acc.sub = CellSub::Other,
                    }
                }
            }
            _ => {
                self.row_node_count += 1;
                if let CellSub::Is { in_t } = &mut self.cell_acc.sub {
                    *in_t = matches!(self.sd_stack.get(3..), Some([SdTag::T] | [SdTag::R, SdTag::T]));
                }
            }
        }
        Ok(())
    }

    fn handle_sd_empty(&mut self, e: &BytesStart<'_>, cx: &mut Ctx<'_>, sheet: &mut Sheet, st: &mut RowState) -> Result<(), IoError> {
        let qname = e.name();
        let name = local_bytes(qname.as_ref());
        match self.sd_stack.len() {
            0 => {
                if name == b"row" {
                    if parse_row_start(e, &mut self.next_row, cx, sheet).is_some() {
                        self.rows_read += 1;
                    } else {
                        self.dropped += 1;
                    }
                }
            }
            1 => {
                self.row_node_count += 1;
                if self.row_node_count > MAX_XML_NODES {
                    return Err(IoError::Format(format!("sheet \"{}\" is damaged (more than {MAX_XML_NODES} elements in one row)", sheet.name)));
                }
                if name == b"c" && self.sd_stack.first() == Some(&SdTag::Row) {
                    if let Some(r) = self.cur_row {
                        if parse_cell_start(e, r, &mut self.next_col, &mut self.cell_acc) {
                            self.finish_cell(cx, sheet, st, true)?;
                        } else {
                            self.dropped += 1;
                            self.dropped_cells += 1;
                        }
                    } else {
                        self.dropped_cells += 1;
                    }
                }
            }
            2 => {
                self.row_node_count += 1;
                if self.cell_acc.pos.is_some() {
                    match name {
                        b"v" => {
                            self.cell_acc.has_v = true;
                            self.val_buf.clear();
                        }
                        b"f" => {
                            self.fmla_buf.clear();
                            parse_f_attrs(e, &mut self.cell_acc);
                        }
                        b"is" => {
                            self.cell_acc.has_is = true;
                            self.is_buf.clear();
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                self.row_node_count += 1;
            }
        }
        Ok(())
    }

    fn pop_sd_level(&mut self, cx: &mut Ctx<'_>, sheet: &mut Sheet, st: &mut RowState) -> Result<(), IoError> {
        if self.sd_stack.pop().is_none() {
            return Ok(());
        }
        match self.sd_stack.len() {
            0 => {
                self.cur_row = None;
            }
            1 => {
                if self.cell_acc.pos.is_some() {
                    self.finish_cell(cx, sheet, st, false)?;
                    self.cell_acc.pos = None;
                }
                self.cell_acc.sub = CellSub::None;
            }
            2 => {
                self.cell_acc.sub = CellSub::None;
            }
            _ => {
                if let CellSub::Is { in_t } = &mut self.cell_acc.sub {
                    *in_t = matches!(self.sd_stack.get(3..), Some([SdTag::T] | [SdTag::R, SdTag::T]));
                }
            }
        }
        Ok(())
    }

    fn handle_sd_end(&mut self, e: &quick_xml::events::BytesEnd<'_>, cx: &mut Ctx<'_>, sheet: &mut Sheet, st: &mut RowState) -> Result<(), IoError> {
        let qname = e.name();
        let name = local_bytes(qname.as_ref());
        if let Some(pos) = self.sd_stack.iter().rposition(|t| t.matches(name)) {
            while self.sd_stack.len() > pos {
                self.pop_sd_level(cx, sheet, st)?;
            }
        } else if name == b"sheetData" {
            while !self.sd_stack.is_empty() {
                self.pop_sd_level(cx, sheet, st)?;
            }
            self.in_sheet_data = false;
            if let Some(el) = self.stack.pop() {
                if let Some(parent) = self.stack.last_mut() {
                    parent.children.push(el);
                } else if self.root.is_none() {
                    self.root = Some(el);
                }
            }
        }
        Ok(())
    }

    /// Attaches a completed metadata element outside `<sheetData>`. Returns `false` if damaged (stop parsing).
    fn push_meta_el(&mut self, el: El) -> bool {
        match self.stack.last_mut() {
            Some(parent) => {
                self.kept = self.kept.saturating_add(1);
                if self.kept > MAX_XML_NODES {
                    self.damaged = Some(format!("more than {MAX_XML_NODES} elements"));
                    if self.stack.len() > 1 {
                        self.stack.pop();
                    }
                    return false;
                }
                parent.children.push(el);
            }
            None if self.root.is_none() => self.root = Some(el),
            None => {}
        }
        true
    }

    /// Handles one event outside `<sheetData>`. Returns `false` to stop parsing (EOF or damage).
    fn handle_meta_event(&mut self, ev: Event<'_>) -> bool {
        match ev {
            Event::Start(e) => {
                if self.stack.len() >= MAX_DEPTH {
                    self.damaged = Some("XML nested too deeply".into());
                    return false;
                }
                let is_sd = self.stack.len() == 1 && local_bytes(e.name().as_ref()) == b"sheetData";
                self.stack.push(xml::start_el(&e));
                if is_sd {
                    self.in_sheet_data = true;
                    self.sd_stack.clear();
                }
            }
            Event::Empty(e) => return self.push_meta_el(xml::start_el(&e)),
            Event::End(_) => {
                if let Some(el) = self.stack.pop() {
                    return self.push_meta_el(el);
                }
            }
            Event::Text(t) => {
                if let Some(top) = self.stack.last_mut() {
                    append_text_event(&mut top.text, &t);
                }
            }
            Event::CData(t) => {
                if let Some(top) = self.stack.last_mut() {
                    append_xml_bytes(&mut top.text, t.as_ref());
                }
            }
            Event::GeneralRef(r) => {
                if let Some(top) = self.stack.last_mut() {
                    append_general_ref(&mut top.text, &r);
                }
            }
            Event::Eof => return false,
            _ => {}
        }
        true
    }
}

/// Streams `xml` in a single pass: cells inside `<worksheet>/<sheetData>` go straight into
/// `sheet`, while all other elements of `<worksheet>` are collected into `StreamResult::root`.
pub(super) fn stream_worksheet(xml: &[u8], cx: &mut Ctx<'_>, sheet: &mut Sheet, st: &mut RowState) -> Result<StreamResult, IoError> {
    let data = xml::to_utf8(xml);
    let mut reader = Reader::from_reader(data.as_ref());
    {
        let cfg = reader.config_mut();
        cfg.check_end_names = false;
        cfg.allow_unmatched_ends = true;
        cfg.allow_dangling_amp = true;
        cfg.trim_text_start = false;
        cfg.trim_text_end = false;
    }

    let mut s = SheetStreamer::new();
    let mut buf = Vec::with_capacity(4096);

    loop {
        buf.clear();
        let ev = match reader.read_event_into(&mut buf) {
            Ok(ev) => ev,
            Err(e) => {
                if s.in_sheet_data {
                    while !s.sd_stack.is_empty() {
                        s.pop_sd_level(cx, sheet, st)?;
                    }
                    s.in_sheet_data = false;
                }
                s.damaged = Some(format!("{e} at byte {}", reader.buffer_position()));
                break;
            }
        };
        if s.in_sheet_data {
            match ev {
                Event::Start(e) => s.handle_sd_start(&e, cx, sheet)?,
                Event::Empty(e) => s.handle_sd_empty(&e, cx, sheet, st)?,
                Event::End(e) => s.handle_sd_end(&e, cx, sheet, st)?,
                Event::Text(t) => match s.cell_acc.sub {
                    CellSub::V => append_text_event(&mut s.val_buf, &t),
                    CellSub::F => append_text_event(&mut s.fmla_buf, &t),
                    CellSub::Is { in_t: true } => append_text_event(&mut s.is_buf, &t),
                    _ => {}
                },
                Event::CData(t) => match s.cell_acc.sub {
                    CellSub::V => append_xml_bytes(&mut s.val_buf, t.as_ref()),
                    CellSub::F => append_xml_bytes(&mut s.fmla_buf, t.as_ref()),
                    CellSub::Is { in_t: true } => append_xml_bytes(&mut s.is_buf, t.as_ref()),
                    _ => {}
                },
                Event::GeneralRef(r) => match s.cell_acc.sub {
                    CellSub::V => append_general_ref(&mut s.val_buf, &r),
                    CellSub::F => append_general_ref(&mut s.fmla_buf, &r),
                    CellSub::Is { in_t: true } => append_general_ref(&mut s.is_buf, &r),
                    _ => {}
                },
                Event::Eof => {
                    while !s.sd_stack.is_empty() {
                        s.pop_sd_level(cx, sheet, st)?;
                    }
                    s.in_sheet_data = false;
                    s.damaged.get_or_insert_with(|| "unexpected end of file".into());
                    break;
                }
                _ => {}
            }
        } else if !s.handle_meta_event(ev) {
            break;
        }
    }

    while let Some(el) = s.stack.pop() {
        match s.stack.last_mut() {
            Some(parent) => parent.children.push(el),
            None if s.root.is_none() => s.root = Some(el),
            None => {}
        }
    }

    Ok(StreamResult {
        root: s.root.unwrap_or_default(),
        rows_read: s.rows_read,
        total_cells: s.total_cells,
        dropped: s.dropped,
        dropped_cells: s.dropped_cells,
        damaged: s.damaged,
    })
}
