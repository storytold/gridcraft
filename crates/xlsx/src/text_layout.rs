//! Delimited and fixed-width text (#2553): splitting lines into fields, and detecting from the
//! file itself how its lines split and which decimal separator its numbers use.
//!
//! Detection scores each candidate split by complete rows × clean cells × (k − 1)/k, where k is
//! the most common field count: the data consistency measure (row patterns × cell types) of van
//! den Burg, Nazábal and Sutton, "Wrangling messy CSV files by detecting row and type patterns"
//! (Data Mining and Knowledge Discovery, 2019). Complete rows alone don't decide: in `A;1,5` /
//! `B;2,25` both `,` and `;` give two fields per row, and only the cells (`1,5` is a number,
//! `A;1` a leftover) tell them apart.

use gridcraft_core::MAX_COLS;

/// Rows detection looks at (Power Query's type detection uses the first 200 too).
const SAMPLE_ROWS: usize = 200;
/// The most text detection reads for those rows.
const SAMPLE_BYTES: usize = 1 << 20;
/// Leading rows with fewer fields (a title) that may be skipped before the table.
const MAX_SKIP: usize = 10;
/// Rows a report lists by number.
const MAX_LISTED: usize = 10;
/// Values a report quotes as ambiguous.
const MAX_AMBIGUOUS: usize = 5;
/// Distinct punctuation characters, and strings of them, considered as delimiters (each).
const MAX_CANDIDATES: usize = 16;
/// Characters per line fixed-width detection looks at.
const MAX_FIXED_WIDTH: usize = 4096;

/// How a line splits into fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Split {
    /// Fields end at any of these delimiters, each one or more characters (`;`, `||`).
    /// `consecutive` treats a run of delimiters as one, like Text to Columns.
    Delimited { delimiters: Vec<String>, consecutive: bool },
    /// Fields start at these character positions after the first one at 0, like Text to
    /// Columns' `fixedWidths`; each field is trimmed.
    FixedWidth(Vec<usize>),
    /// Each line is one field.
    Lines,
}

impl Split {
    /// One delimiter, consecutive delimiters kept apart.
    pub fn delimiter(d: impl Into<String>) -> Split {
        Split::Delimited { delimiters: vec![d.into()], consecutive: false }
    }

    /// The single ASCII delimiter a CSV save can write back, when that is what this is.
    pub fn single_byte(&self) -> Option<u8> {
        match self {
            Split::Delimited { delimiters, consecutive: false } => match delimiters.as_slice() {
                [d] => match d.as_bytes() {
                    [b] if b.is_ascii() => Some(*b),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    /// The one delimiter character, when the split is exactly that.
    fn single_char(&self) -> Option<char> {
        match self {
            Split::Delimited { delimiters, .. } => match delimiters.as_slice() {
                [d] => {
                    let mut cs = d.chars();
                    cs.next().filter(|_| cs.next().is_none())
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// A short name for messages: `;`, `TAB`, `||`, `fixed width`, `no delimiter`.
    pub fn describe(&self) -> String {
        match self {
            Split::Delimited { delimiters, consecutive } => {
                let names: Vec<String> = delimiters
                    .iter()
                    .map(|d| match d.as_str() {
                        "\t" => "TAB".into(),
                        " " => "SPACE".into(),
                        _ => d.clone(),
                    })
                    .collect();
                if *consecutive { format!("{} (consecutive as one)", names.join(" ")) } else { names.join(" ") }
            }
            Split::FixedWidth(_) => "fixed width".into(),
            Split::Lines => "no delimiter".into(),
        }
    }
}

/// The kind of text file, which breaks ties between equally good splits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextKind {
    /// `.csv`: comma or semicolon first.
    #[default]
    Csv,
    /// `.txt`, `.tsv`, `.tab`: tab first.
    Tab,
    /// `.prn` (Formatted Text): fixed width first.
    Prn,
    Other,
}

/// The decimal separator numbers are read with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Decimal {
    /// `1.5`, `1,234.5` (digit groups `,` or a space).
    #[default]
    Point,
    /// `1,5`, `1.234,5` (digit groups `.` or a space).
    Comma,
}

impl Decimal {
    pub fn as_char(self) -> char {
        match self {
            Decimal::Point => '.',
            Decimal::Comma => ',',
        }
    }
}

/// Why the decimal separator is the one it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecimalBasis {
    /// The caller gave it.
    Given,
    /// Values that read only one way (`1,5`, `0.125`) decided.
    Values,
    /// Only ambiguous values (`1.234`): a guess from the delimiter (`;` comma, `,` point).
    Delimiter,
    /// No value depends on it: the decimal point.
    Default,
}

/// How numbers are read, file-wide and per column.
#[derive(Clone, Debug, PartialEq)]
pub struct Numbers {
    pub decimal: Decimal,
    pub basis: DecimalBasis,
    /// Columns (0-based, ascending) whose own values decided against `decimal` (a file holding
    /// both kinds).
    pub columns: Vec<(usize, Decimal)>,
    /// Columns (ascending) holding values of both kinds: there, values that depend on the
    /// separator stay text.
    pub mixed: Vec<usize>,
    /// Values that read differently with either separator (`1.234`), at most a few.
    pub ambiguous: Vec<String>,
}

impl Numbers {
    pub fn given(decimal: Decimal) -> Numbers {
        Numbers { decimal, basis: DecimalBasis::Given, columns: vec![], mixed: vec![], ambiguous: vec![] }
    }

    /// The separator column `col` is read with; `None` for a mixed column. Asked for every
    /// cell, so it searches the sorted lists.
    pub fn column(&self, col: usize) -> Option<Decimal> {
        if self.mixed.binary_search(&col).is_ok() {
            return None;
        }
        Some(self.columns.binary_search_by_key(&col, |(c, _)| *c).ok().and_then(|i| self.columns.get(i)).map_or(self.decimal, |(_, d)| *d))
    }
}

/// What detection found in the first rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub split: Split,
    /// Leading rows with fewer fields (a title) before the table.
    pub start_row: usize,
    /// The most common field count.
    pub fields: usize,
    /// Rows looked at after `start_row`, blank lines not counted.
    pub sampled: usize,
    /// Of those, rows with exactly `fields` fields.
    pub complete: usize,
    /// Why the split may be wrong, empty when it is clear.
    pub uncertain: Vec<String>,
}

/// What reading a text file found: the layout used, the number reading and how complete the
/// rows are over the whole file.
#[derive(Clone, Debug, PartialEq)]
pub struct TextReport {
    pub split: Split,
    /// The split was detected rather than given.
    pub detected: bool,
    pub start_row: usize,
    pub numbers: Numbers,
    /// The most common field count (from the first rows).
    pub fields: usize,
    /// Rows read after `start_row`, blank lines not counted.
    pub rows: usize,
    /// Of those, rows with exactly `fields` fields.
    pub complete: usize,
    /// The first rows with another field count: (row number in the file, 1-based; fields).
    pub other_rows: Vec<(usize, usize)>,
    /// Rows that are complete again when two neighbouring fields join into one number, such as
    /// an unquoted `1,234.50` in a comma file: (row number, the joined number).
    pub split_numbers: Vec<(usize, String)>,
    /// Why the split may be wrong (detected splits only).
    pub uncertain: Vec<String>,
}

// ---------------------------------------------------------------- splitting

/// The lines of `text` (CRLF, LF or CR), without a last empty one after a final line end.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let r = rest?;
        if r.is_empty() {
            rest = None;
            return None;
        }
        match r.find(['\r', '\n']) {
            Some(i) => {
                let line = r.get(..i).unwrap_or_default();
                let after = r.get(i..).unwrap_or_default();
                let skip = if after.starts_with("\r\n") { 2 } else { 1 };
                rest = after.get(skip..);
                Some(line)
            }
            None => {
                rest = None;
                Some(r)
            }
        }
    })
}

/// Splits text into records of fields as `split` says. In delimited text `quote` opens a quoted
/// field at the start of a field (elsewhere it is an ordinary character); inside one, a doubled
/// quote is a quote and delimiters and line ends are text. A record keeps at most [`MAX_COLS`]
/// fields. A blank line is a record of one empty field. `on_record` returns false to stop.
pub fn split_records(text: &str, split: &Split, quote: Option<char>, mut on_record: impl FnMut(Vec<String>) -> bool) {
    match split {
        Split::Delimited { delimiters, consecutive } => match (split.single_char(), quote) {
            // One character, quoted: the CSV reader's own loop.
            (Some(d), Some(q)) if !consecutive => crate::csv::parse_records(text, d, q, on_record),
            _ => delimited(text, delimiters, *consecutive, quote, on_record),
        },
        Split::FixedWidth(breaks) => {
            // Ordered once, not for every line.
            let mut breaks: Vec<usize> = breaks.iter().copied().filter(|b| *b > 0).collect();
            breaks.sort_unstable();
            breaks.dedup();
            breaks.truncate(MAX_COLS as usize - 1);
            for line in lines(text) {
                if !on_record(fixed_fields(line, &breaks)) {
                    return;
                }
            }
        }
        Split::Lines => {
            for line in lines(text) {
                if !on_record(vec![line.to_string()]) {
                    return;
                }
            }
        }
    }
}

fn push_field(record: &mut Vec<String>, field: String) {
    if record.len() < MAX_COLS as usize {
        record.push(field);
    }
}

fn delimited(text: &str, delimiters: &[String], consecutive: bool, quote: Option<char>, mut on_record: impl FnMut(Vec<String>) -> bool) {
    // Longest first, so `||` wins over `|` when both are given.
    let mut delims: Vec<&str> = delimiters.iter().map(String::as_str).filter(|d| !d.is_empty()).collect();
    delims.sort_by_key(|d| std::cmp::Reverse(d.len()));
    let mut field = String::new();
    let mut record: Vec<String> = vec![];
    let mut in_quote = false;
    let mut started_quoted = false;
    let mut after_delimiter = false;
    let mut i = 0;
    while let Some(c) = text.get(i..).and_then(|r| r.chars().next()) {
        let n = c.len_utf8();
        if in_quote {
            if Some(c) == quote {
                if text.get(i + n..).is_some_and(|r| r.starts_with(c)) {
                    field.push(c);
                    i += 2 * n;
                    continue;
                }
                in_quote = false;
            } else {
                field.push(c);
            }
            i += n;
            continue;
        }
        if Some(c) == quote && field.is_empty() && !started_quoted {
            in_quote = true;
            started_quoted = true;
            after_delimiter = false;
            i += n;
            continue;
        }
        if let Some(d) = delims.iter().find(|d| text.get(i..).is_some_and(|r| r.starts_with(**d))) {
            i += d.len();
            if consecutive && after_delimiter {
                continue;
            }
            push_field(&mut record, std::mem::take(&mut field));
            started_quoted = false;
            after_delimiter = true;
            continue;
        }
        if c == '\r' || c == '\n' {
            i += n;
            if c == '\r' && text.get(i..).is_some_and(|r| r.starts_with('\n')) {
                i += 1;
            }
            push_field(&mut record, std::mem::take(&mut field));
            started_quoted = false;
            after_delimiter = false;
            if !on_record(std::mem::take(&mut record)) {
                return;
            }
            continue;
        }
        field.push(c);
        after_delimiter = false;
        i += n;
    }
    if !field.is_empty() || !record.is_empty() || started_quoted {
        push_field(&mut record, field);
        on_record(record);
    }
}

/// A line cut at the break positions inside it (ascending, above 0), each piece trimmed (Text
/// to Columns' rule).
fn fixed_fields(line: &str, breaks: &[usize]) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut cuts = vec![0];
    cuts.extend(breaks.iter().copied().take_while(|b| *b < chars.len()));
    cuts.push(chars.len());
    cuts.windows(2)
        .map(|w| match w {
            [a, b] => chars.get(*a..*b).map(|s| s.iter().collect::<String>().trim().to_string()).unwrap_or_default(),
            _ => String::new(),
        })
        .collect()
}

// ---------------------------------------------------------------- numbers

/// What a value says about the decimal separator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// A number only with a decimal point (`1.5`, `0.125`, `1,234.5`).
    Point,
    /// A number only with a decimal comma (`1,5`, `1.234,5`).
    Comma,
    /// A number either way, with different values (`1.234`, `1,234`).
    Ambiguous,
    /// A number either way, with the same value (`1234`, `1 234`).
    Same,
}

const CURRENCY: [char; 6] = ['$', '€', '£', '¥', '₹', '₩'];

fn is_group_space(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}')
}

/// A digit group separator with either decimal separator: a space, or the Swiss `'`.
fn is_group_mark(c: char) -> bool {
    is_group_space(c) || matches!(c, '\'' | '\u{2019}')
}

/// `suffix` without a trailing minus (`1.234,50-`, as accounting exports write negatives).
fn without_trailing_minus(suffix: &str) -> Option<&str> {
    suffix.trim_end().strip_suffix('-')
}

/// Splits `s` into the text before a number, the number (digits with `.`, `,`, spaces or `'`
/// between them) and the text after: a sign or currency before, a percent sign, currency or a
/// trailing minus after.
fn number_parts(s: &str) -> Option<(&str, &str, &str)> {
    let t = s.trim();
    let first = t.find(|c: char| c.is_ascii_digit())?;
    // A separator right before the first digit starts the number (`,5`, `.5`).
    let start = if t.get(..first).is_some_and(|p| p.ends_with(['.', ','])) { first.saturating_sub(1) } else { first };
    let prefix = t.get(..start)?;
    if !prefix.chars().all(|c| matches!(c, '+' | '-' | '(' | ' ') || CURRENCY.contains(&c)) {
        return None;
    }
    let rest = t.get(start..)?;
    // The number runs to its last digit; separators inside it must have digits on both sides.
    let mut end = 0;
    let mut prev_digit = start < first;
    for (i, c) in rest.char_indices() {
        if c.is_ascii_digit() {
            end = i + 1;
            prev_digit = true;
        } else if (c == '.' || c == ',' || is_group_mark(c)) && prev_digit {
            prev_digit = false;
        } else {
            break;
        }
    }
    let number = rest.get(..end)?;
    let suffix = rest.get(end..)?;
    // An exponent goes with the number (`1,5E3`); the separators are the mantissa's.
    let after = suffix.get(exponent_len(suffix)..).unwrap_or(suffix);
    let after = match without_trailing_minus(after) {
        Some(a) if !prefix.contains(['-', '+', '(']) => a,
        _ => after,
    };
    if !after.chars().all(|c| matches!(c, '%' | ')' | ' ') || CURRENCY.contains(&c)) {
        return None;
    }
    Some((prefix, number, suffix))
}

/// The length of an exponent at the start of `s` (`E3`, `e-2`), or 0.
fn exponent_len(s: &str) -> usize {
    let b = s.as_bytes();
    if !matches!(b.first(), Some(b'e' | b'E')) {
        return 0;
    }
    let sign = usize::from(matches!(b.get(1), Some(b'+' | b'-')));
    let digits = b.iter().skip(1 + sign).take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 { 0 } else { 1 + sign + digits }
}

/// The value of `number` (digits and separators only) read with `decimal`, digit groups of
/// three after a first group of one to three that doesn't start with 0.
fn value_with(number: &str, decimal: Decimal) -> Option<f64> {
    let d = decimal.as_char();
    let mut parts = number.split(d);
    let int = parts.next()?;
    let frac = parts.next();
    if parts.next().is_some() || frac.is_some_and(|f| f.is_empty() || !f.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }
    let group = int.chars().find(|c| !c.is_ascii_digit());
    let digits: String = match group {
        None => int.to_string(),
        Some(g) => {
            let allowed = match decimal {
                Decimal::Point => g == ',' || is_group_mark(g),
                Decimal::Comma => g == '.' || is_group_mark(g),
            };
            let groups: Vec<&str> = int.split(g).collect();
            let first = groups.first()?;
            if !allowed
                || first.is_empty()
                || first.len() > 3
                || first.starts_with('0')
                || groups.iter().any(|x| !x.chars().all(|c| c.is_ascii_digit()))
                || groups.iter().skip(1).any(|x| x.len() != 3)
            {
                return None;
            }
            groups.concat()
        }
    };
    // `,5` is 0.5; a separator alone is nothing.
    if digits.is_empty() && frac.is_none() {
        return None;
    }
    format!("0{digits}.{}", frac.unwrap_or("0")).parse().ok()
}

/// An amount with a currency sign the input parser doesn't read (`1.234,50 €`, `€ 7,50`,
/// `-3 €`): its value read with `decimal`, and a number format that shows the sign where the
/// file writes it.
pub fn currency_amount(field: &str, decimal: Decimal) -> Option<(f64, String)> {
    let (prefix, number, suffix) = number_parts(field)?;
    let sign = prefix.chars().chain(suffix.chars()).find(|c| CURRENCY.contains(c))?;
    let signs = prefix.chars().filter(|c| matches!(c, '+' | '-')).count();
    if exponent_len(suffix) > 0
        || suffix.contains('%')
        || prefix.contains('(') != suffix.contains(')')
        || signs > 1
        || prefix.chars().chain(suffix.chars()).filter(|c| CURRENCY.contains(c)).count() > 1
    {
        return None;
    }
    let value = value_with(number, decimal)?;
    let minus = without_trailing_minus(suffix);
    let suffix = minus.unwrap_or(suffix);
    let negative = prefix.contains('-') || prefix.contains('(') || minus.is_some();
    let decimals = number.rsplit_once(decimal.as_char()).map_or(0, |(_, f)| f.len());
    let digits = if decimals == 0 { "#,##0".to_string() } else { format!("#,##0.{}", "0".repeat(decimals)) };
    let code = match prefix.find(sign) {
        Some(i) => {
            let gap = prefix.get(i + sign.len_utf8()..).is_some_and(|after| after.contains(' '));
            format!("\"{sign}\"{}{digits}", if gap { " " } else { "" })
        }
        None => format!("{digits}{}\"{sign}\"", if suffix.trim_start_matches(')').starts_with(' ') { " " } else { "" }),
    };
    Some((if negative { -value } else { value }, code))
}

/// What `field` says about the decimal separator; `None` when it isn't a plain number.
pub fn evidence(field: &str) -> Option<Evidence> {
    let (_, number, _) = number_parts(field)?;
    match (value_with(number, Decimal::Point), value_with(number, Decimal::Comma)) {
        (Some(p), Some(c)) => Some(if p == c { Evidence::Same } else { Evidence::Ambiguous }),
        (Some(_), None) => Some(Evidence::Point),
        (None, Some(_)) => Some(Evidence::Comma),
        (None, None) => None,
    }
}

/// `field` rewritten with a decimal point and `,` digit groups when it is a number with a
/// decimal comma (`1.234,5` → `1,234.5`, `12,5 %` → `12.5 %`), so typed-input parsing reads
/// it and keeps its thousands format.
pub fn comma_to_point(field: &str) -> Option<String> {
    to_point(field, Decimal::Comma)
}

/// `field`, a number with `decimal`, the way typed-input parsing reads it: a decimal point,
/// `,` digit groups (`1'234.5` → `1,234.5`) and a trailing minus in front (`7,5-` → `-7.5`).
pub fn to_point(field: &str, decimal: Decimal) -> Option<String> {
    let (prefix, number, suffix) = number_parts(field)?;
    value_with(number, decimal)?;
    let (int, frac) = number.split_once(decimal.as_char()).map_or((number, None), |(i, f)| (i, Some(f)));
    let int: String = int.chars().map(|c| if c.is_ascii_digit() { c } else { ',' }).collect();
    let (sign, suffix) = without_trailing_minus(suffix).map_or(("", suffix), |s| ("-", s));
    Some(match frac {
        Some(f) => format!("{sign}{prefix}{int}.{f}{suffix}"),
        None => format!("{sign}{prefix}{int}{suffix}"),
    })
}

/// The decimal separator of the sample `rows` (after any start rows), split with `split`:
/// values that read only one way decide, per column when the file holds both kinds; with only
/// ambiguous values, `;` files get the comma and `,` files the point.
pub fn detect_numbers(rows: &[Vec<String>], split: &Split) -> Numbers {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut per_col = vec![(0usize, 0usize); width];
    let (mut point, mut comma) = (0usize, 0usize);
    let mut ambiguous: Vec<String> = vec![];
    for row in rows {
        for (col, field) in row.iter().enumerate() {
            let Some(e) = evidence(field) else { continue };
            let Some(slot) = per_col.get_mut(col) else { continue };
            match e {
                Evidence::Point => {
                    slot.0 += 1;
                    point += 1;
                }
                Evidence::Comma => {
                    slot.1 += 1;
                    comma += 1;
                }
                Evidence::Ambiguous => {
                    let v = field.trim().to_string();
                    if ambiguous.len() < MAX_AMBIGUOUS && !ambiguous.contains(&v) {
                        ambiguous.push(v);
                    }
                }
                Evidence::Same => {}
            }
        }
    }
    let (decimal, basis) = if point + comma > 0 {
        (if comma > point { Decimal::Comma } else { Decimal::Point }, DecimalBasis::Values)
    } else if !ambiguous.is_empty() {
        match split.single_char() {
            Some(';') => (Decimal::Comma, DecimalBasis::Delimiter),
            Some(',') => (Decimal::Point, DecimalBasis::Delimiter),
            _ => (Decimal::Point, DecimalBasis::Default),
        }
    } else {
        (Decimal::Point, DecimalBasis::Default)
    };
    let mut columns = vec![];
    let mut mixed = vec![];
    for (col, (p, c)) in per_col.into_iter().enumerate() {
        match (p > 0, c > 0) {
            (true, true) => mixed.push(col),
            (true, false) if decimal != Decimal::Point => columns.push((col, Decimal::Point)),
            (false, true) if decimal != Decimal::Comma => columns.push((col, Decimal::Comma)),
            _ => {}
        }
    }
    Numbers { decimal, basis, columns, mixed, ambiguous }
}

// ---------------------------------------------------------------- detection

/// Never guessed as delimiters: they are part of numbers, dates, amounts and ordinary text.
/// They can still be given.
const NO_AUTO: [char; 22] = ['.', '-', '+', '\'', '"', '(', ')', '[', ']', '{', '}', '%', '$', '€', '£', '¥', '@', '&', '*', '\\', '_', '!'];
/// Always tried, and ties go in this order (Excel's Tab, Semicolon, Comma, then the pipe).
const STANDARD: [&str; 4] = ["\t", ";", ",", "|"];
/// A cell starting or ending with one of these is a split leftover (`#|`, `Name#`).
const LEFTOVER_EDGE: [char; 13] = ['#', '|', '^', '~', ':', '=', '/', '§', '¦', '•', ';', ',', '\t'];

fn is_punct(c: char) -> bool {
    !(c.is_alphanumeric() || c.is_whitespace() || c == '"' || c == '\'')
}

/// The line with quoted parts emptied, for counting delimiters outside quotes.
fn unquoted(line: &str, quote: Option<char>) -> String {
    let Some(q) = quote else { return line.to_string() };
    let mut out = String::with_capacity(line.len());
    let mut inside = false;
    for c in line.chars() {
        if c == q {
            inside = !inside;
        } else if !inside {
            out.push(c);
        }
    }
    out
}

/// On almost every line, and almost always the same number of times.
fn steady(lines: &[String], token: &str) -> bool {
    if lines.is_empty() {
        return false;
    }
    let counts: Vec<usize> = lines.iter().map(|l| l.matches(token).count()).collect();
    let on = counts.iter().filter(|n| **n > 0).count();
    if on * 10 < lines.len() * 9 {
        return false;
    }
    let mut freq: Vec<(usize, usize)> = vec![];
    for n in counts.into_iter().filter(|n| *n > 0) {
        match freq.iter_mut().find(|(k, _)| *k == n) {
            Some(f) => f.1 += 1,
            None => freq.push((n, 1)),
        }
    }
    freq.iter().map(|(_, f)| *f).max().unwrap_or(0) * 10 >= lines.len() * 8
}

/// Mostly part of dates and times (`12:30`, `04/03/2024`): the words holding it read as one.
/// Between plain numbers (`1§2`) it is still a delimiter.
fn inside_dates(lines: &[String], ch: char) -> bool {
    let (mut all, mut dates) = (0usize, 0usize);
    for l in lines {
        for word in l.split(|c: char| c.is_whitespace() || STANDARD.iter().any(|s| s.starts_with(c))).filter(|w| w.contains(ch)) {
            all += 1;
            if is_date_or_time(word) {
                dates += 1;
            }
        }
    }
    dates * 2 > all
}

/// A value of a known kind: empty, a number either way, a date or time, a boolean.
fn is_typed(t: &str) -> bool {
    t.is_empty() || evidence(t).is_some() || is_date_or_time(t) || ["true", "false", "wahr", "falsch"].iter().any(|b| t.eq_ignore_ascii_case(b))
}

/// `25.10.2026`, `04/03/2024`, `2026-10-25`, `12:30`, `12:30:00`, `2026-10-25 12:30`.
fn is_date_or_time(t: &str) -> bool {
    let groups = |s: &str, seps: &[char]| -> Option<Vec<usize>> {
        let parts: Vec<&str> = s.split(|c| seps.contains(&c)).collect();
        parts.iter().all(|p| !p.is_empty() && p.len() <= 4 && p.chars().all(|c| c.is_ascii_digit())).then(|| parts.iter().map(|p| p.len()).collect())
    };
    let date = |s: &str| groups(s, &['.', '/', '-']).is_some_and(|g| g.len() == 3);
    let time = |s: &str| groups(s, &[':']).is_some_and(|g| (2..=3).contains(&g.len()) && g.iter().skip(1).all(|n| *n == 2));
    match t.split_once([' ', 'T']) {
        Some((d, tm)) => date(d) && time(tm),
        None => date(t) || time(t),
    }
}

/// How clean a cell is: 1 for a typed value, ½ for ordinary text, 0 for a split leftover.
fn quality(cell: &str, others: &[&str]) -> f64 {
    let t = cell.trim();
    if is_typed(t) {
        return 1.0;
    }
    let leftover =
        others.iter().any(|o| t.contains(o)) || t.chars().all(|c| !c.is_alphanumeric()) || t.starts_with(LEFTOVER_EDGE) || t.ends_with(LEFTOVER_EDGE);
    if leftover { 0.0 } else { 0.5 }
}

/// The leading title rows (one field each), the most common field count after them (ties: the
/// larger), and how complete the rest is.
struct Shape {
    k: usize,
    skip: usize,
    rows: usize,
    complete: usize,
    /// Rows after the skipped ones with at least two fields.
    split_rows: usize,
    /// (complete + ½ × shorter) / rows: a longer row means the delimiter occurs in the data.
    score: f64,
}

fn mode(counts: &[usize]) -> usize {
    let mut freq: Vec<(usize, usize)> = vec![];
    for n in counts {
        match freq.iter_mut().find(|(k, _)| k == n) {
            Some(f) => f.1 += 1,
            None => freq.push((*n, 1)),
        }
    }
    freq.iter().max_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0))).map_or(1, |(k, _)| *k)
}

fn shape(counts: &[usize]) -> Shape {
    // A title is a leading row of one field before a table of more.
    let mut skip = 0;
    while skip < MAX_SKIP && skip + 1 < counts.len() && counts.get(skip) == Some(&1) {
        skip += 1;
    }
    let mut k = mode(counts.get(skip..).unwrap_or_default());
    if k <= 1 {
        skip = 0;
        k = mode(counts);
    }
    let rest = counts.get(skip..).unwrap_or_default();
    let complete = rest.iter().filter(|n| **n == k).count();
    let shorter = rest.iter().filter(|n| **n < k).count();
    let split_rows = rest.iter().filter(|n| **n >= 2).count();
    let score = if rest.is_empty() { 0.0 } else { (complete as f64 + 0.5 * shorter as f64) / rest.len() as f64 };
    Shape { k, skip, rows: rest.len(), complete, split_rows, score }
}

/// The sample split with `split`: its first non-blank records.
fn sample_records(sample: &str, split: &Split, quote: Option<char>) -> Vec<Vec<String>> {
    let mut rows = vec![];
    split_records(sample, split, quote, |r| {
        if !(r.len() == 1 && r.first().is_some_and(|f| f.trim().is_empty())) {
            rows.push(r);
        }
        rows.len() < SAMPLE_ROWS
    });
    rows
}

/// Fixed-width breaks: positions where every line has a space (or has ended) and the next
/// position holds text on some line.
fn fixed_breaks(lines: &[String]) -> Vec<usize> {
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).min(MAX_FIXED_WIDTH);
    let mut blank = vec![true; width];
    for l in lines {
        for (p, c) in l.chars().take(width).enumerate() {
            if c != ' '
                && let Some(b) = blank.get_mut(p)
            {
                *b = false;
            }
        }
    }
    (1..width).filter(|p| blank.get(p - 1) == Some(&true) && blank.get(*p) == Some(&false)).collect()
}

/// Almost every line holding the delimiter is, whole, a number (`1.234,5`, `7,5`, `1 234`): a
/// column of numbers, not fields. `1,2` beside a line `a,b` is two fields. `Some(true)` when
/// another line lacks it, such as a header (`Betrag` above `99,00`): then even a `.csv` is one
/// column, where `1,2` lines alone could be two.
fn splits_numbers(lines: &[String], ch: char) -> Option<bool> {
    let with: Vec<&String> = lines.iter().filter(|l| l.contains(ch)).collect();
    let hits = with.iter().filter(|l| evidence(l).is_some()).count();
    if with.is_empty() || hits * 10 < with.len() * 9 {
        return None;
    }
    Some(lines.iter().any(|l| !l.contains(ch) && evidence(l).is_none()))
}

struct Scored {
    split: Split,
    shape: Shape,
    q: f64,
    rows: Vec<Vec<String>>,
    splits_numbers: bool,
}

fn order(split: &Split) -> usize {
    match split {
        Split::Delimited { delimiters, consecutive } => match (delimiters.first().map(String::as_str), consecutive) {
            (Some("\t"), _) => 0,
            (Some(";"), _) => 1,
            (Some(","), _) => 2,
            (Some("|"), _) => 3,
            (Some(" "), false) => 4,
            (Some(" "), true) => 5,
            _ => 7,
        },
        Split::FixedWidth(_) => 6,
        Split::Lines => 8,
    }
}

/// Detects how the lines of `text` split: delimiter (one or more characters), runs of spaces,
/// fixed width or none, and a title before the table. Reads at most the first
/// [`SAMPLE_ROWS`] rows and [`SAMPLE_BYTES`] bytes. Without `any`, only `,`, `;` and tab are
/// tried, as Excel's File › Open of a CSV splits: nothing shows the result before it opens.
pub fn detect(text: &str, kind: TextKind, quote: Option<char>, any: bool) -> Layout {
    let mut cut = text.len().min(SAMPLE_BYTES);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let sample = text.get(..cut).unwrap_or_default();
    let raw: Vec<String> = lines(sample).filter(|l| !l.trim().is_empty()).take(SAMPLE_ROWS).map(str::to_string).collect();
    let plain: Vec<String> = raw.iter().map(|l| unquoted(l, quote)).collect();

    let mut candidates: Vec<Split> = STANDARD.iter().filter(|d| any || **d != "|").map(|d| Split::delimiter(*d)).collect();
    // Other punctuation and strings only with lines to compare: on a single line, every
    // character is "steady".
    let several = any && plain.len() >= 2;
    // Each punctuation character, and each string of two or three, with the number of lines
    // it is on; the ones on (almost) every line are tried, most widespread first.
    let mut tally: Vec<(String, usize)> = vec![];
    for l in plain.iter().filter(|_| several) {
        let cs: Vec<char> = l.chars().collect();
        let mut here: Vec<String> = vec![];
        let mut i = 0;
        while i < cs.len() {
            let start = i;
            while cs.get(i).is_some_and(|c| is_punct(*c)) {
                i += 1;
            }
            let run = cs.get(start..i).unwrap_or_default();
            for n in 1..=3 {
                for w in run.windows(n) {
                    let one_standard = n == 1 && w.first().is_some_and(|c| STANDARD.iter().any(|s| s.starts_with(*c)));
                    // `,,` and `;;` are empty fields, not a delimiter of their own (`||` can be).
                    let empty_fields = w.first().is_some_and(|f| matches!(f, ',' | ';') && w.iter().all(|c| c == f));
                    if !one_standard && !empty_fields && !w.iter().any(|c| NO_AUTO.contains(c)) {
                        here.push(w.iter().collect());
                    }
                }
            }
            i += 1;
        }
        here.sort_unstable();
        here.dedup();
        for s in here {
            match tally.iter().position(|(t, _)| *t == s) {
                Some(i) => {
                    if let Some(entry) = tally.get_mut(i) {
                        entry.1 += 1;
                    }
                }
                None if tally.len() < 16 * MAX_CANDIDATES => tally.push((s, 1)),
                None => {}
            }
        }
    }
    tally.retain(|(_, n)| n * 10 >= plain.len() * 9);
    tally.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    tally.truncate(MAX_CANDIDATES);
    for (s, _) in tally {
        let single = {
            let mut cs = s.chars();
            cs.next().filter(|_| cs.next().is_none())
        };
        if steady(&plain, &s) && single.is_none_or(|c| !inside_dates(&raw, c)) {
            candidates.push(Split::delimiter(s));
        }
    }
    let runs = !plain.is_empty() && plain.iter().filter(|l| has_space_run(l)).count() * 10 >= plain.len() * 9;
    if any {
        candidates.push(Split::Delimited { delimiters: vec![" ".into()], consecutive: runs });
    }
    if any && (kind == TextKind::Prn || runs) {
        candidates.push(Split::FixedWidth(fixed_breaks(&raw)));
    }
    candidates.push(Split::Lines);

    let shaped: Vec<(Split, Vec<Vec<String>>, Shape)> = candidates
        .into_iter()
        .map(|split| {
            let rows = sample_records(sample, &split, quote);
            let counts: Vec<usize> = rows.iter().map(Vec::len).collect();
            let shape = shape(&counts);
            (split, rows, shape)
        })
        .collect();
    // Delimiters that split (nearly) every row the same way: a cell still holding one of them
    // after another split is a leftover.
    let steady_ones: Vec<String> = shaped
        .iter()
        .filter(|(s, _, sh)| sh.k > 1 && sh.score >= 0.9 && matches!(s, Split::Delimited { consecutive: false, .. }))
        .filter_map(|(s, _, _)| match s {
            Split::Delimited { delimiters, .. } => delimiters.first().filter(|d| d.as_str() != " ").cloned(),
            _ => None,
        })
        .collect();
    // Of those, the ones that split every row from the first: a row holding one of them is that
    // table's header, never a title another split may skip (`Menge;Preis` above `1,5;2,30`).
    let from_first: Vec<String> = shaped
        .iter()
        .filter(|(_, _, sh)| sh.skip == 0)
        .filter_map(|(s, _, _)| match s {
            Split::Delimited { delimiters, .. } => delimiters.first().filter(|d| steady_ones.contains(d)).cloned(),
            _ => None,
        })
        .collect();
    let mut scored: Vec<Scored> = shaped
        .into_iter()
        .map(|(split, rows, shape)| {
            let own = match &split {
                Split::Delimited { delimiters, .. } => delimiters.first().cloned().unwrap_or_default(),
                _ => String::new(),
            };
            let others: Vec<&str> = steady_ones.iter().map(String::as_str).filter(|o| *o != own && !own.contains(o)).collect();
            let skips_a_header = rows
                .get(..shape.skip)
                .unwrap_or_default()
                .iter()
                .flatten()
                .any(|f| from_first.iter().any(|o| *o != own && !own.contains(o.as_str()) && f.contains(o.as_str())));
            let body = rows.get(shape.skip..).unwrap_or_default();
            // Whether each column is empty in every row, looked up by index for every cell.
            let empty_cols: Vec<bool> =
                if shape.k > 1 { (0..shape.k).map(|i| body.iter().all(|r| r.get(i).is_none_or(|f| f.trim().is_empty()))).collect() } else { vec![] };
            let (mut sum, mut n) = (0.0, 0usize);
            for r in body {
                for (i, f) in r.iter().enumerate() {
                    n += 1;
                    if !empty_cols.get(i).copied().unwrap_or(false) {
                        sum += quality(f, &others);
                    }
                }
            }
            let standard =
                matches!(&split, Split::Delimited { delimiters, .. } if delimiters.first().is_some_and(|d| STANDARD.contains(&d.as_str())));
            // Only empty fields (`;;;` lines): a table of empty cells.
            let t = if n == 0 {
                0.0
            } else if standard && shape.k > 1 && empty_cols.iter().all(|e| *e) {
                1.0
            } else {
                sum / n as f64
            };
            let lines_split = matches!(split, Split::Lines);
            let w = if shape.k <= 1 { 0.3 } else { (shape.k - 1) as f64 / shape.k as f64 };
            // A delimiter missing from more than a quarter of the rows is text, not a separator
            // (a one-column export whose cells sometimes hold `;`).
            let covered = shape.split_rows * 4 >= shape.rows * 3;
            let single_space = matches!(&split, Split::Delimited { delimiters, consecutive: false } if delimiters.first().is_some_and(|d| d == " "));
            let prior = match (&split, kind) {
                (Split::Delimited { delimiters, .. }, TextKind::Tab) if delimiters.first().is_some_and(|d| d == "\t") => 1.15,
                (Split::Delimited { delimiters, .. }, TextKind::Csv) if delimiters.first().is_some_and(|d| d == "," || d == ";") => 1.1,
                (Split::FixedWidth(_), TextKind::Prn) => 1.15,
                _ => 1.0,
            };
            let number_char = match &split {
                Split::Delimited { delimiters, .. } => delimiters.first().and_then(|d| d.chars().next()).filter(|c| *c == ',' || *c == ' '),
                _ => None,
            };
            let inside_numbers = number_char.and_then(|c| splits_numbers(&raw, c));
            // Anything but a usual delimiter needs cleaner cells: punctuation inside text
            // (`https://`, `Re:`, `Order #1`) cuts each line once into text and leftovers.
            let unusual = matches!(&split, Split::Delimited { delimiters, .. }
                if delimiters.first().is_some_and(|d| d != " " && !STANDARD.contains(&d.as_str())));
            let unclean = unusual && if shape.k == 2 { t <= 0.75 } else { t < 0.5 };
            let q = if (!lines_split && (shape.k <= 1 || !covered)) || (single_space && t < 0.75) || unclean || skips_a_header {
                0.0
            } else {
                let numbers = inside_numbers.is_some_and(|header| header || kind != TextKind::Csv);
                shape.score * w * t * 0.97f64.powi(shape.skip as i32) * prior * if numbers { 0.3 } else { 1.0 }
            };
            Scored { split, shape, q, rows, splits_numbers: inside_numbers.is_some() }
        })
        .collect();
    scored.sort_by(|a, b| b.q.total_cmp(&a.q).then(order(&a.split).cmp(&order(&b.split))));
    // Nothing splits cleanly: no delimiter.
    if scored.first().is_some_and(|s| s.q <= 0.0)
        && let Some(i) = scored.iter().position(|s| s.split == Split::Lines)
    {
        let lines = scored.remove(i);
        scored.insert(0, lines);
    }
    let mut it = scored.into_iter();
    let Some(best) = it.next() else {
        return Layout { split: Split::Lines, start_row: 0, fields: 1, sampled: 0, complete: 0, uncertain: vec![] };
    };
    let mut uncertain = vec![];
    if best.shape.rows > 0 && best.shape.complete * 20 < best.shape.rows * 19 {
        uncertain.push(format!("only {} of {} rows have {} fields", best.shape.complete, best.shape.rows, best.shape.k));
    }
    if let Some(second) = it.find(|s| s.q > 0.0 && s.split != best.split)
        && second.q > 0.85 * best.q
        && second.rows != best.rows
    {
        uncertain.push(format!("{} splits almost as well", second.split.describe()));
    }
    if best.splits_numbers {
        uncertain.push(format!("{} also sits inside numbers", best.split.describe()));
    }
    Layout { split: best.split, start_row: best.shape.skip, fields: best.shape.k, sampled: best.shape.rows, complete: best.shape.complete, uncertain }
}

fn has_space_run(line: &str) -> bool {
    let t = line.trim();
    t.contains("  ")
}

/// The first non-blank rows of `text` after its first `skip` records, split with `split`, for
/// [`detect_numbers`] and the field count.
pub fn sample_rows(text: &str, split: &Split, quote: Option<char>, skip: usize) -> Vec<Vec<String>> {
    let mut cut = text.len().min(SAMPLE_BYTES);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut rows = vec![];
    let mut seen = 0usize;
    split_records(text.get(..cut).unwrap_or_default(), split, quote, |r| {
        seen += 1;
        if seen > skip && !(r.len() == 1 && r.first().is_some_and(|f| f.trim().is_empty())) {
            rows.push(r);
        }
        rows.len() < SAMPLE_ROWS
    });
    rows
}

/// The records before the table when `title_rows` non-blank rows precede it: those rows and
/// the blank lines up to the table.
pub fn records_before_table(text: &str, split: &Split, quote: Option<char>, title_rows: usize) -> usize {
    if title_rows == 0 {
        return 0;
    }
    let (mut records, mut titles) = (0usize, 0usize);
    split_records(text, split, quote, |r| {
        let blank = r.len() == 1 && r.first().is_some_and(|f| f.trim().is_empty());
        if !blank && titles == title_rows {
            return false;
        }
        records += 1;
        if !blank {
            titles += 1;
        }
        true
    });
    records
}

/// The most common field count of `rows` (ties: the larger), 1 for none.
pub fn most_common_fields(rows: &[Vec<String>]) -> usize {
    shape(&rows.iter().map(Vec::len).collect::<Vec<_>>()).k
}

/// Counts complete rows while a file is read, for [`TextReport`].
#[derive(Debug, Default)]
pub(crate) struct Completeness {
    pub fields: usize,
    pub rows: usize,
    pub complete: usize,
    pub other_rows: Vec<(usize, usize)>,
    pub split_numbers: Vec<(usize, String)>,
}

impl Completeness {
    /// One record, at `row_number` (1-based) in the file. Blank lines don't count.
    pub fn add(&mut self, row_number: usize, record: &[String], split: &Split, decimal: Decimal) {
        if record.len() == 1 && record.first().is_some_and(|f| f.trim().is_empty()) {
            return;
        }
        self.rows += 1;
        if record.len() == self.fields {
            self.complete += 1;
            return;
        }
        if self.other_rows.len() < MAX_LISTED {
            self.other_rows.push((row_number, record.len()));
        }
        // One field too many where two neighbours join into a grouped number: an unquoted
        // thousands separator (`1,234.50` in a comma file).
        if record.len() == self.fields + 1
            && self.split_numbers.len() < MAX_LISTED
            && let Some(d) = split.single_char().filter(|c| matches!(c, ',' | '.' | ' '))
        {
            for pair in record.windows(2) {
                let [a, b] = pair else { continue };
                let (a, b) = (a.trim(), b.trim());
                if a.ends_with(|c: char| c.is_ascii_digit()) && b.starts_with(|c: char| c.is_ascii_digit()) {
                    let joined = format!("{a}{d}{b}");
                    if let Some((_, number, _)) = number_parts(&joined)
                        && number.contains(d)
                        && value_with(number, decimal).is_some()
                    {
                        self.split_numbers.push((row_number, joined));
                        break;
                    }
                }
            }
        }
    }
}
