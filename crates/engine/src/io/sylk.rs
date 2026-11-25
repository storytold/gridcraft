//! SYLK (`.slk`) import. `craft_csv::sylk` reads the file (values, formulas rewritten from R1C1 to
//! A1, number formats, column widths); this module makes it a one-sheet workbook: formulas parsed
//! (a formula GridCraft can't parse is skipped and counted), error values recognised, formats
//! interned as styles and widths converted from characters, and the options record's calendar
//! flag kept so the file's dates survive.

use std::collections::HashMap;
use std::sync::Arc;

use craft_csv::sylk;
use gridcraft_core::{CellError, CellRef, DateSystem, Value};
use gridcraft_model::{Cell, Formula, NumFmt, Sheet, Style, Workbook};

use crate::{EngineError, Result};

/// Whether the file's options record selects the 1904 date system. Excel writes the flag in the
/// `O` record's `V` field: `V4` for a 1904 workbook and `V0` for a 1900 one (the field is a bit
/// mask, and bit 2 is the calendar). Keeping it makes the file's serials show the dates it shows;
/// read as 1900 dates, every one would be 1,462 days early.
fn uses_1904_calendar(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    for line in bytes.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let mut fields = line.split(|&b| b == b';');
        let Some(kind) = fields.next() else { continue };
        // The reader stops at the end record: don't honour an option past it.
        if kind == b"E" {
            return false;
        }
        if kind != b"O" {
            continue;
        }
        for field in fields {
            if let Some(arg) = field.strip_prefix(b"V")
                && let Ok(arg) = std::str::from_utf8(arg)
                && let Ok(flags) = arg.trim().parse::<u32>()
            {
                return flags & 4 != 0;
            }
        }
    }
    false
}

/// Reads a SYLK file into a one-sheet workbook. Returns the workbook and its warnings.
pub fn read(name: &str, bytes: &[u8]) -> Result<(Workbook, Vec<String>)> {
    let file = sylk::read(bytes).map_err(|e| match e {
        sylk::Error::TooLarge => EngineError::Other(format!("We can't open '{name}': the SYLK file is larger than {} MB.", sylk::MAX_BYTES >> 20)),
        _ => EngineError::Other(format!("We can't open '{name}': it isn't a SYLK file (it doesn't start with an ID record).")),
    })?;
    let mut wb = Workbook::new();
    if uses_1904_calendar(bytes) {
        wb.date_system = DateSystem::D1904;
    }
    let mut sheet = Sheet::new("Sheet1");
    let mut skipped = file.skipped;
    let mut styles = HashMap::new();
    for (&(row, col), c) in &file.cells {
        let value = match &c.value {
            sylk::Value::Number(n) => Value::Number(*n),
            sylk::Value::Text(s) => Value::Text(Arc::from(s.as_str())),
            sylk::Value::Bool(b) => Value::Bool(*b),
            sylk::Value::Error(s) => CellError::parse(s).map_or_else(|| Value::Text(Arc::from(s.as_str())), Value::Error),
            _ => Value::Empty,
        };
        let mut cell = Cell::value(value);
        if let Some(a1) = &c.formula {
            let f = Formula::new(a1);
            if f.expr.is_some() {
                cell.formula = Some(Arc::new(f));
            } else {
                skipped += 1;
            }
        }
        if let Some(code) = &c.format {
            cell.style = *styles
                .entry(code.as_str())
                .or_insert_with(|| wb.styles.intern(Style { num_fmt: NumFmt(Arc::from(code.as_str())), ..Style::default() }));
        }
        sheet.set_cell(CellRef::new(row, col), cell);
    }
    for w in &file.column_widths {
        let size = crate::cmd::format::chars_to_points(w.width) as f32;
        for c in w.first..=w.last {
            sheet.cols.entry(c).or_default().size = Some(size);
        }
    }
    if let Some(first) = wb.sheet_mut(0) {
        sheet.name = std::mem::take(&mut first.name);
        *first = sheet;
    }
    let mut warnings = Vec::new();
    if skipped > 0 {
        warnings.push(format!("{name}: {skipped} SYLK records GridCraft couldn't read were skipped"));
    }
    Ok((wb, warnings))
}
