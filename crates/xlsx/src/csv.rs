//! CSV import and export (RFC 4180 with the usual real-world leniency). Decoding, the record parser
//! and field quoting are `craft-csv`'s; how the lines split and which decimal separator the numbers
//! use are detected in [`crate::text_layout`]; this module turns records into typed cells (with
//! each column's decimal separator and date order) and writes a sheet.

use std::borrow::Cow;
use std::sync::Arc;

use gridcraft_core::date::{days_in_month_in, serial_from_ymd};
use gridcraft_core::parse::{Parsed, parse_input};
use gridcraft_core::{CellRef, DateSystem, MAX_COLS, MAX_ROWS, Value, col_to_letters};
use gridcraft_model::{Cell, NumFmt, Sheet, StyleId, Workbook};

use crate::text_layout::{
    self, Completeness, Decimal, Evidence, Numbers, Split, TextReport, detect, detect_numbers, evidence, most_common_fields, records_before_table,
    sample_rows, split_records,
};
use crate::{CsvOptions, IoError, ReadReport};

/// The largest CSV or tab-separated file written (bytes).
pub const MAX_CSV_BYTES: usize = 512 << 20;

pub use craft_csv::decode_text;

/// [`craft_csv::parse_records`] with a record kept to the grid's [`MAX_COLS`] fields.
pub(crate) fn parse_records(text: &str, delim: char, quote: char, on_record: impl FnMut(Vec<String>) -> bool) {
    craft_csv::parse_records(text, delim, quote, MAX_COLS as usize, on_record);
}

/// What a column's numeric dates say about the order of day and month.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateRule {
    /// No `d/m/y` or `d-m-y` value in the sample: read as typed input does.
    NoDates,
    /// A value reads only month first (`12/31/2026`), none reads only day first.
    MonthFirst,
    /// A value reads only day first (`31/05/2026`), none reads only month first.
    DayFirst,
    /// Every date is ambiguous (`03/04/2026`): keep the input parser's month-first reading.
    Ambiguous,
    /// Values read only day first and values read only month first: the dates stay text.
    Conflict,
}

impl DateRule {
    fn of(day_first: usize, month_first: usize, ambiguous: usize) -> Self {
        match (day_first > 0, month_first > 0, ambiguous > 0) {
            (true, true, _) => Self::Conflict,
            (true, false, _) => Self::DayFirst,
            (false, true, _) => Self::MonthFirst,
            (false, false, true) => Self::Ambiguous,
            (false, false, false) => Self::NoDates,
        }
    }
}

/// The evidence a column's sample gives for its date order.
#[derive(Clone, Default)]
struct DateEvidence {
    day_first: usize,
    month_first: usize,
    ambiguous: usize,
    day_first_example: Option<String>,
    month_first_example: Option<String>,
    ambiguous_example: Option<String>,
}

/// A numeric `d/m/y` or `d-m-y` field, with the serial it has read each way and its time of day.
struct NumericDate {
    day_first: Option<f64>,
    month_first: Option<f64>,
    time: Option<f64>,
}

impl NumericDate {
    /// The field under the column's rule: a date when that reading is valid, else text.
    fn read(&self, field: &str, rule: DateRule) -> Parsed {
        let serial = match rule {
            DateRule::DayFirst => self.day_first,
            DateRule::MonthFirst | DateRule::Ambiguous => self.month_first,
            DateRule::Conflict | DateRule::NoDates => None,
        };
        match serial {
            Some(serial) => Parsed {
                value: Value::number(serial + self.time.unwrap_or(0.0)),
                format: Some(if self.time.is_some() { "m/d/yyyy h:mm" } else { "m/d/yyyy" }),
            },
            None => Parsed { value: Value::text(field), format: None },
        }
    }
}

/// A numeric date written with `/` or `-`, with the serial it has read day first and month first.
/// `None` when the field isn't such a date (including ISO `yyyy-mm-dd`, month names and dots,
/// which read one way and keep the input parser).
fn numeric_date(field: &str, sys: DateSystem) -> Option<NumericDate> {
    let text = field.trim();
    let (date, time) = split_time(text, sys);
    // Typed input reads a trailing comma after a date (`3/4/2026,`); do the same here.
    let date = date.trim_end_matches(',');
    let mut parts = date.split(['/', '-']);
    let (a, b, c) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let (a, b, c) = (a.trim(), b.trim(), c.trim());
    // ISO dates are unambiguous; the input parser reads them. A three-digit year is not a
    // date to typed input either.
    if a.len() == 4 || c.len() == 3 {
        return None;
    }
    let a = parse_digits(a)?;
    let b = parse_digits(b)?;
    let year = expand_year(c, parse_digits(c)?);
    let day_first = valid_date(sys, year, b, a);
    let month_first = valid_date(sys, year, a, b);
    if day_first.is_none() && month_first.is_none() {
        return None;
    }
    Some(NumericDate { day_first, month_first, time })
}

/// Splits a field into its date and a time of day, when the tail reads as a time. Both the first
/// and last space are tried, so `03/04/2026 10:30 AM` and `03 / 04 / 2026 10:30` both split.
fn split_time(text: &str, sys: DateSystem) -> (&str, Option<f64>) {
    for at in [text.find(' '), text.rfind(' ')].into_iter().flatten() {
        let (date, time) = text.split_at(at);
        let time = time.trim();
        // Times are short; a long tail is not one, and parsing it would be wasted work.
        if time.len() > 40 {
            continue;
        }
        if let Some(fraction) = time_of_day(time, sys) {
            return (date.trim(), Some(fraction));
        }
    }
    (text, None)
}

fn time_of_day(text: &str, sys: DateSystem) -> Option<f64> {
    match parse_input(text, sys) {
        Parsed { value: Value::Number(fraction), format: Some(code) } if code.contains('h') => Some(fraction),
        _ => None,
    }
}

fn parse_digits(s: &str) -> Option<i64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Expands a one- or two-digit year the way typed input does (`26` is 2026, `30` is 1930).
fn expand_year(text: &str, year: i64) -> i64 {
    if text.len() <= 2 { if year < 30 { 2000 + year } else { 1900 + year } } else { year }
}

/// The serial for a date the input parser accepts, or `None`.
fn valid_date(sys: DateSystem, year: i64, month: i64, day: i64) -> Option<f64> {
    if !(1..=12).contains(&month) || day < 1 || year < 1900 {
        return None;
    }
    let year = i32::try_from(year).ok()?;
    let month = u32::try_from(month).ok()?;
    if day > i64::from(days_in_month_in(sys, year, month)) {
        return None;
    }
    serial_from_ymd(sys, i64::from(year), i64::from(month), day)
}

/// The date rule for each column, from the sample.
fn infer_date_rules(rows: &[Vec<String>], sys: DateSystem) -> Vec<(DateRule, DateEvidence)> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0).min(MAX_COLS as usize);
    let mut columns = vec![DateEvidence::default(); width];
    for row in rows {
        for (col, field) in row.iter().enumerate() {
            let Some(column) = columns.get_mut(col) else { continue };
            let Some(date) = numeric_date(field, sys) else { continue };
            let example = || field.trim().chars().take(40).collect::<String>();
            match (date.day_first.is_some(), date.month_first.is_some()) {
                (true, true) => {
                    column.ambiguous += 1;
                    column.ambiguous_example.get_or_insert_with(example);
                }
                (true, false) => {
                    column.day_first += 1;
                    column.day_first_example.get_or_insert_with(example);
                }
                (false, true) => {
                    column.month_first += 1;
                    column.month_first_example.get_or_insert_with(example);
                }
                (false, false) => {}
            }
        }
    }
    columns.into_iter().map(|c| (DateRule::of(c.day_first, c.month_first, c.ambiguous), c)).collect()
}

/// Warns when the sample could not choose a date order: an all-ambiguous column keeps the
/// month-first reading, and a column with both kinds keeps its dates as text.
fn date_warnings(columns: &[(DateRule, DateEvidence)]) -> Vec<String> {
    let mut out = vec![];
    for (col, (rule, evidence)) in columns.iter().enumerate() {
        match rule {
            DateRule::Ambiguous => out.push(format!(
                "column {}: every date is ambiguous, so it was read month first ({})",
                col_to_letters(col as u32),
                evidence.ambiguous_example.as_deref().unwrap_or("a date")
            )),
            DateRule::Conflict => out.push(format!(
                "column {}: it holds both day-first and month-first dates ({} and {}), so its dates stay text",
                col_to_letters(col as u32),
                evidence.day_first_example.as_deref().unwrap_or("a day-first date"),
                evidence.month_first_example.as_deref().unwrap_or("a month-first date")
            )),
            DateRule::NoDates | DateRule::MonthFirst | DateRule::DayFirst => {}
        }
    }
    out
}

pub fn read_csv(bytes: &[u8], opts: &CsvOptions) -> Result<Workbook, IoError> {
    read_text(bytes, opts).map(|(wb, _)| wb)
}

/// [`read_csv`], with the warnings an open shows: a column whose date order had to be guessed,
/// or whose dates stayed text because it holds both orders.
pub fn read_csv_with_report(bytes: &[u8], opts: &CsvOptions) -> Result<(Workbook, ReadReport), IoError> {
    let (wb, text) = read_text(bytes, opts)?;
    let mut report = ReadReport::default();
    for w in text.dates {
        report.warn(w);
    }
    Ok((wb, report))
}

/// The settings a text file is read with, given or detected.
struct Resolved {
    text: String,
    quote: Option<char>,
    split: Split,
    layout: Option<text_layout::Layout>,
    start: usize,
    fields: usize,
    numbers: Numbers,
    /// Each column's date order, from the sample (#2765); empty when every field is kept as text.
    dates: Vec<(DateRule, DateEvidence)>,
}

impl Resolved {
    fn new(bytes: &[u8], opts: &CsvOptions) -> Result<Resolved, IoError> {
        let text = decode_text(bytes);
        let quote = (!opts.no_qualifier).then_some(if opts.quote == 0 { '"' } else { opts.quote as char });
        let (split, layout) = match (&opts.split, opts.delimiter) {
            (Some(split), _) => (split.clone(), None),
            (None, 0) => {
                let layout = detect(&text, opts.kind, quote, opts.detect_any);
                (layout.split.clone(), Some(layout))
            }
            (None, d) => (Split::delimiter(d as char), None),
        };
        if let (Some(q), Split::Delimited { delimiters, .. }) = (quote, &split)
            && delimiters.iter().any(|d| d.contains(q))
        {
            return Err(IoError::Format("the delimiter and quote character must differ".into()));
        }
        let start = match opts.start_row {
            Some(n) => n,
            None => layout.as_ref().map_or(0, |l| records_before_table(&text, &split, quote, l.start_row)),
        };
        let sample = sample_rows(&text, &split, quote, start);
        let fields = layout.as_ref().map_or_else(|| most_common_fields(&sample), |l| l.fields);
        let numbers = match opts.decimal {
            Some(d) => Numbers::given(d),
            None => detect_numbers(&sample, &split),
        };
        let dates = if opts.parse_values { infer_date_rules(&sample, opts.date_system) } else { vec![] };
        Ok(Resolved { text, quote, split, layout, start, fields, numbers, dates })
    }

    fn report(self, done: Completeness) -> TextReport {
        TextReport {
            split: self.split,
            detected: self.layout.is_some(),
            start_row: self.start,
            numbers: self.numbers,
            fields: self.fields,
            rows: done.rows,
            complete: done.complete,
            other_rows: done.other_rows,
            split_numbers: done.split_numbers,
            uncertain: self.layout.map(|l| l.uncertain).unwrap_or_default(),
            dates: date_warnings(&self.dates),
        }
    }
}

/// The Text Import Wizard's preview: the first `rows` rows of the file as `opts` splits them,
/// fields as the text they hold, and what reading found (counted over the whole of `bytes`).
pub fn preview_text(bytes: &[u8], opts: &CsvOptions, rows: usize) -> Result<(Vec<Vec<String>>, TextReport), IoError> {
    let r = Resolved::new(bytes, opts)?;
    let mut out = vec![];
    let mut record = 0usize;
    let mut done = Completeness { fields: r.fields, ..Default::default() };
    split_records(&r.text, &r.split, r.quote, |rec| {
        record += 1;
        if record > r.start {
            done.add(record, &rec, &r.split, r.numbers.decimal);
            if out.len() < rows {
                out.push(rec);
            }
        }
        record < MAX_ROWS as usize
    });
    Ok((out, r.report(done)))
}

/// [`read_csv`], with what reading found: the split used (detected when neither `split` nor
/// `delimiter` is given), the decimal separator, and how complete the rows are over the file.
pub fn read_text(bytes: &[u8], opts: &CsvOptions) -> Result<(Workbook, TextReport), IoError> {
    let resolved = Resolved::new(bytes, opts)?;
    let Resolved { text, quote, split, start, numbers, dates, .. } = &resolved;
    let (quote, start) = (*quote, *start);
    let mut wb = Workbook::new();
    wb.date_system = opts.date_system;
    wb.csv_delimiter = split.single_byte();
    wb.csv_decimal_comma = opts.parse_values && numbers.decimal == Decimal::Comma;
    let mut sheet = Sheet::new("Sheet1");
    let mut styles = std::mem::take(&mut wb.styles);
    let mut fmt_cache: Vec<(Cow<'static, str>, StyleId)> = vec![];
    let mut row: u32 = 0;
    // The bounding box of the non-empty cells, which is the sheet's used range.
    let mut used: Option<(u32, u32, u32, u32)> = None;
    let mut too_many = false;
    let mut record = 0usize;
    let mut done = Completeness { fields: resolved.fields, ..Default::default() };
    split_records(text, split, quote, |rec| {
        record += 1;
        if record <= start {
            return true;
        }
        if row >= MAX_ROWS {
            return false;
        }
        done.add(record, &rec, split, numbers.decimal);
        // A blank line is an empty row.
        for (col, f) in rec.into_iter().enumerate() {
            if col as u32 >= MAX_COLS {
                break;
            }
            if f.is_empty() {
                continue;
            }
            let pos = CellRef::new(row, col as u32);
            let (r0, c0, c1) = used.map_or((row, col as u32, col as u32), |(r0, c0, _, c1)| (r0, c0.min(col as u32), c1.max(col as u32)));
            used = Some((r0, c0, row, c1));
            let cell = if opts.parse_values {
                let rule = dates.get(col).map_or(DateRule::NoDates, |(rule, _)| *rule);
                let p = read_field(&f, numbers.column(col), rule, opts.date_system);
                let style = match p.format {
                    Some(code) => match fmt_cache.iter().find(|(c, _)| *c == code) {
                        Some((_, id)) => *id,
                        None => {
                            let id = styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(&code));
                            fmt_cache.push((code, id));
                            id
                        }
                    },
                    None => StyleId::DEFAULT,
                };
                Cell { value: p.value, formula: None, style, py: None }
            } else {
                Cell::value(Value::Text(Arc::from(f)))
            };
            sheet.cells.set(pos, cell);
        }
        // Stop at the first record that takes the used range past the limit, before more is stored.
        if let (Some(max), Some((r0, c0, r1, c1))) = (opts.max_used_cells, used)
            && u64::from(r1 - r0 + 1) * u64::from(c1 - c0 + 1) > max
        {
            too_many = true;
            return false;
        }
        row += 1;
        true
    });
    if too_many {
        return Err(IoError::TooLarge(format!("the CSV's used range is over the limit of {} cells", opts.max_used_cells.unwrap_or(0))));
    }
    wb.styles = styles;
    wb.sheets = vec![Arc::new(sheet)];
    Ok((wb, resolved.report(done)))
}

/// A field's value and the number format it gets.
struct Read {
    value: Value,
    format: Option<Cow<'static, str>>,
}

impl From<Parsed> for Read {
    fn from(p: Parsed) -> Read {
        Read { value: p.value, format: p.format.map(Cow::Borrowed) }
    }
}

/// One field read like typed input, with the column's decimal separator: a decimal comma
/// number is rewritten with a point first (`1.234,5` → `1,234.5`); a value that is a number
/// only with the other separator stays text, as it does in Excel (`0.125` under a decimal
/// comma), and `1,5E3` is 1500. In a column holding both kinds (`None`), any value that depends
/// on the separator stays text. Dates with dots are day first (`01.11.2026`), dates with `/` or
/// `-` follow the column's date order (`rule`), amounts carry any currency sign (`7,50 €`), Swiss
/// `'` groups and a trailing minus read (`1'234.50`, `7,50-`).
fn read_field(f: &str, decimal: Option<Decimal>, rule: DateRule, sys: DateSystem) -> Read {
    let text = || Read { value: Value::text(f), format: None };
    if let Some(date) = dotted_date(f, sys) {
        return date;
    }
    if rule != DateRule::NoDates
        && let Some(date) = numeric_date(f, sys)
    {
        return date.read(f, rule).into();
    }
    let number = |p: Parsed| matches!(p.value, Value::Number(_)).then(|| Read::from(p));
    let amount = |d: Decimal| text_layout::currency_amount(f, d).map(|(v, code)| Read { value: Value::Number(v), format: Some(Cow::Owned(code)) });
    // Typed input, else the same number with a point and `,` groups, else an amount.
    let with_point = |p: Parsed| {
        number(p.clone())
            .or_else(|| text_layout::to_point(f, Decimal::Point).and_then(|t| number(parse_input(&t, sys))))
            .or_else(|| amount(Decimal::Point))
            .unwrap_or_else(|| p.into())
    };
    match decimal {
        Some(Decimal::Point) => with_point(parse_input(f, sys)),
        Some(Decimal::Comma) => match text_layout::comma_to_point(f) {
            // Rewritten for the reader only: what doesn't read as a number keeps the file's text.
            Some(point) => number(parse_input(&point, sys)).or_else(|| amount(Decimal::Comma)).unwrap_or_else(text),
            None if evidence(f) == Some(Evidence::Point) => text(),
            None => {
                let p = parse_input(f, sys);
                let date = p.format.is_some_and(|code| code.chars().any(|c| matches!(c.to_ascii_lowercase(), 'y' | 'd' | 'h' | 'm' | 's')));
                if matches!(p.value, Value::Number(_)) && f.contains(['.', ',']) && !date { text() } else { p.into() }
            }
        },
        None => match evidence(f) {
            Some(Evidence::Point | Evidence::Comma | Evidence::Ambiguous) => text(),
            _ => with_point(parse_input(f, sys)),
        },
    }
}

/// `25.10.2026`, `1.11.26`, with a time or without (`25.10.2026 12:30`): day first, as
/// everywhere dates are written with dots; the input parser reads them month first.
fn dotted_date(f: &str, sys: gridcraft_core::DateSystem) -> Option<Read> {
    let t = f.trim();
    let (date, time) = t.split_once(' ').map_or((t, None), |(d, tm)| (d, Some(tm.trim())));
    let parts: Vec<&str> = date.split('.').collect();
    let [d, m, y] = parts.as_slice() else { return None };
    let digits = |s: &str, lens: &[usize]| lens.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(d, &[1, 2]) || !digits(m, &[1, 2]) || !digits(y, &[2, 4]) {
        return None;
    }
    let (d, m, y): (i64, i64, i64) = (d.parse().ok()?, m.parse().ok()?, y.parse().ok()?);
    let y = match y {
        0..30 => 2000 + y,
        30..100 => 1900 + y,
        _ => y,
    };
    let month = u32::try_from(m).ok().filter(|m| (1..=12).contains(m))?;
    if y < 1900 || d < 1 || d > i64::from(gridcraft_core::date::days_in_month_in(sys, i32::try_from(y).ok()?, month)) {
        return None;
    }
    let day = gridcraft_core::date::serial_from_ymd(sys, y, m, d)?;
    match time {
        None => Some(Read { value: Value::Number(day), format: Some(Cow::Borrowed("dd.mm.yyyy")) }),
        Some(tm) => match parse_input(tm, sys) {
            Parsed { value: Value::Number(fr), format: Some(code) } if (0.0..1.0).contains(&fr) && code.contains('h') => {
                Some(Read { value: Value::Number(day + fr), format: Some(Cow::Borrowed("dd.mm.yyyy hh:mm")) })
            }
            _ => None,
        },
    }
}

/// Writes the sheet as CSV from A1 to the end of its used range, so leading empty columns and rows
/// stay as empty fields and every cell keeps its position (CRLF line ends, UTF-8 without BOM). `cell_text` gives
/// each cell's field text: the caller decides how values look (Save As CSV writes them as
/// displayed, number formats applied); quoting is done here.
///
/// The output is capped at [`MAX_CSV_BYTES`]: a cell far from A1 makes the file as big as the whole
/// rectangle between them (`A1` and `XFD1048576` are 16 GiB of commas).
/// An export that would pass the cap is refused before anything is allocated, or at the first
/// field or line end that takes it past the cap (a field is measured before it is written), with
/// [`IoError::TooLarge`]; accepted exports keep every field in its place.
pub fn write_csv(sheet: &Sheet, delimiter: u8, cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    write_csv_limited(sheet, delimiter, MAX_CSV_BYTES, cell_text)
}

/// [`write_csv`] with the output cap `limit` (bytes) given.
pub fn write_csv_limited(sheet: &Sheet, delimiter: u8, limit: usize, mut cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    let delim = if delimiter == 0 { ',' } else { delimiter as char };
    let mut out = String::new();
    let Some(r) = sheet.used_range() else { return Ok(Vec::new()) };
    let too_large =
        |what: String| IoError::TooLarge(format!("the CSV would be {what}, over the limit of {} MiB; export a smaller range", limit >> 20));
    // Every row has a delimiter between its fields and a CRLF, whatever the cells hold: a lower
    // bound on the output that needs no cell text.
    let (rows, cols) = (u64::from(r.end.row) + 1, u64::from(r.end.col) + 1);
    let per_row = (cols - 1).saturating_mul(delim.len_utf8() as u64).saturating_add(2);
    let least = rows.saturating_mul(per_row);
    if least > limit as u64 {
        return Err(too_large(format!("at least {} MiB ({rows} rows by {cols} columns)", least >> 20)));
    }
    for row in 0..=r.end.row {
        for col in 0..=r.end.col {
            if col > 0 {
                out.push(delim);
            }
            let t = cell_text(CellRef::new(row, col));
            // The exact size of the field as written (the delimiter was pushed above), so a row
            // never grows past the cap unseen.
            if out.len().saturating_add(craft_csv::field_len(&t, delim)) > limit {
                return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
            }
            craft_csv::push_field(&mut out, &t, delim);
        }
        out.push_str("\r\n");
        if out.len() > limit {
            return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
        }
    }
    Ok(out.into_bytes())
}
