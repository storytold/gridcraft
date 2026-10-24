//! CSV import and export (RFC 4180 with the usual real-world leniency).

use std::sync::Arc;

use gridcraft_core::parse::parse_input;
use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, Value};
use gridcraft_model::{Cell, CsvEncoding, NumFmt, Sheet, StyleId, Workbook};

use crate::{CsvOptions, IoError};

/// The largest CSV or tab-separated file written (bytes).
pub const MAX_CSV_BYTES: usize = 512 << 20;

/// Bytes one encode step writes into: encoding a 512 MiB CSV must not need a second copy of it
/// (larger than any single character the code pages here need, so every step makes progress).
const ENCODE_CHUNK: usize = 8 * 1024;

/// Windows-1252 code points for bytes 0x80–0x9F (others map to Latin-1).
const CP1252: [u32; 32] = [
    0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019,
    0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
];

/// Windows-1256 code points for bytes 0x80–0xFF (unicode.org `CP1256.TXT`).
pub(crate) const CP1256: [u32; 128] = [
    0x20AC, 0x067E, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0679, 0x2039, 0x0152, 0x0686, 0x0698, 0x0688, 0x06AF, 0x2018,
    0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x06A9, 0x2122, 0x0691, 0x203A, 0x0153, 0x200C, 0x200D, 0x06BA, 0x00A0, 0x060C, 0x00A2, 0x00A3,
    0x00A4, 0x00A5, 0x00A6, 0x00A7, 0x00A8, 0x00A9, 0x06BE, 0x00AB, 0x00AC, 0x00AD, 0x00AE, 0x00AF, 0x00B0, 0x00B1, 0x00B2, 0x00B3, 0x00B4, 0x00B5,
    0x00B6, 0x00B7, 0x00B8, 0x00B9, 0x061B, 0x00BB, 0x00BC, 0x00BD, 0x00BE, 0x061F, 0x06C1, 0x0621, 0x0622, 0x0623, 0x0624, 0x0625, 0x0626, 0x0627,
    0x0628, 0x0629, 0x062A, 0x062B, 0x062C, 0x062D, 0x062E, 0x062F, 0x0630, 0x0631, 0x0632, 0x0633, 0x0634, 0x0635, 0x0636, 0x00D7, 0x0637, 0x0638,
    0x0639, 0x063A, 0x0640, 0x0641, 0x0642, 0x0643, 0x00E0, 0x0644, 0x00E2, 0x0645, 0x0646, 0x0647, 0x0648, 0x00E7, 0x00E8, 0x00E9, 0x00EA, 0x00EB,
    0x0649, 0x064A, 0x00EE, 0x00EF, 0x064B, 0x064C, 0x064D, 0x064E, 0x00F4, 0x064F, 0x0650, 0x00F7, 0x0651, 0x00F9, 0x0652, 0x00FB, 0x00FC, 0x200E,
    0x200F, 0x06D2,
];

/// Evidence that bytes which aren't UTF-8 are Windows-1256 Arabic rather than Windows-1252.
#[derive(Default)]
struct Cp1256Evidence {
    /// High bytes that decode to different characters in the two code pages, apart from invisible
    /// marks and ordinals after a digit.
    differing: usize,
    /// Of those, Arabic letters and Arabic punctuation (، ؛ ؟) in Windows-1256.
    arabic: usize,
    /// Arabic bytes next to an ASCII letter: Western accents sit inside Latin words, Arabic doesn't.
    arabic_touching_latin: usize,
    /// Accented Latin bytes the two code pages share, next to another high byte: Hebrew
    /// Windows-1255 letters land there, Arabic and Western text rarely do.
    shared_letters_in_runs: usize,
}

impl Cp1256Evidence {
    fn of(bytes: &[u8]) -> Self {
        let mut e = Self::default();
        for (i, &b) in bytes.iter().enumerate() {
            if b < 0x80 {
                continue;
            }
            let before = i.checked_sub(1).and_then(|j| bytes.get(j)).copied();
            let after = bytes.get(i + 1).copied();
            let next_to = |pred: fn(u8) -> bool| before.is_some_and(pred) || after.is_some_and(pred);
            if same_in_cp1252_and_cp1256(b) {
                if is_shared_latin_letter(b) && next_to(|x| x >= 0x80) {
                    e.shared_letters_in_runs += 1;
                }
                continue;
            }
            if is_invisible_mark(b) || is_ordinal_after_digit(b, before) {
                continue;
            }
            e.differing += 1;
            if is_cp1256_arabic(b) {
                e.arabic += 1;
                if next_to(|x| x.is_ascii_alphabetic()) {
                    e.arabic_touching_latin += 1;
                }
            }
        }
        e
    }

    /// Thresholds chosen against month, day and language names from 95 glibc locales (Western in
    /// Windows-1252, Arabic, Persian and Urdu in 1256, Hebrew and Yiddish in 1255) plus CSV samples.
    fn is_arabic(&self) -> bool {
        self.arabic >= 2
            && self.arabic * 10 >= self.differing * 9
            && self.arabic_touching_latin * 10 <= self.arabic * 3
            && self.shared_letters_in_runs * 20 <= self.arabic
    }
}

fn same_in_cp1252_and_cp1256(b: u8) -> bool {
    matches!(
        b,
        0x80 | 0x82..=0x89 | 0x8B..=0x8C | 0x91..=0x97 | 0x99 | 0x9B..=0x9C | 0xA0 | 0xA2..=0xA9 | 0xAB..=0xB9 | 0xBB..=0xBE | 0xD7 | 0xE0 | 0xE2 | 0xE7..=0xEB
            | 0xEE..=0xEF | 0xF4 | 0xF7 | 0xF9 | 0xFB..=0xFC
    )
}

/// ZWNJ, ZWJ, LRM and RLM: common inside Persian words and around Arabic cells, but no evidence
/// either way.
fn is_invisible_mark(b: u8) -> bool {
    matches!(b, 0x9D | 0x9E | 0xFD | 0xFE)
}

/// `º` and `ª` after a digit (`1º`, `2ª`) are Western ordinals, though Windows-1256 reads the bytes
/// as `؛` and `ھ`; either way they are no evidence.
fn is_ordinal_after_digit(b: u8, before: Option<u8>) -> bool {
    matches!(b, 0xAA | 0xBA) && before.is_some_and(|x| x.is_ascii_digit())
}

fn is_shared_latin_letter(b: u8) -> bool {
    matches!(b, 0xE0 | 0xE2 | 0xE7..=0xEB | 0xEE..=0xEF | 0xF4 | 0xF9 | 0xFB..=0xFC)
}

fn is_cp1256_arabic(b: u8) -> bool {
    matches!(
        b,
        0x81 | 0x8A | 0x8D..=0x90 | 0x98 | 0x9A | 0x9F | 0xA1 | 0xAA | 0xBA | 0xBF..=0xD6 | 0xD8..=0xDB | 0xDD..=0xDF | 0xE1 | 0xE3..=0xE6 | 0xEC..=0xED | 0xFF
    )
}

/// Whether text holds hiragana or full-width katakana: kana that only CP932 two-byte sequences
/// produce, so [`CsvEncoding::Auto`] can take it as proof the bytes really are Shift-JIS.
///
/// Half-width katakana deliberately does not count: every CP932 single byte from `0xA1` to `0xDF`
/// is a half-width katakana, and that range is `À`–`ß` and `¡`–`¿` in Windows-1252, so counting it
/// would read `Straße` or `Ärger` as Japanese.
fn has_kana(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{3040}'..='\u{30FF}'))
}

/// The CP932 evidence [`CsvEncoding::Auto`] needs when a clean decode holds no kana. A clean decode
/// alone is not enough: Windows-1252 smart quotes and accented letters are valid CP932 pairs too
/// (`‘a’b` is `0x91 0x61 0x92 0x62` and decodes as `疎鍛`), so counting pairs alone reads quoted
/// Western text as Japanese. Two measurements separate the two:
///
/// - at least 80% of the non-ASCII bytes are consumed by valid double-byte pairs — a Japanese text
///   is nearly all pairs, while a Windows-1252 word leaves its one-byte letters lone; and
/// - valid CP932 pairs outnumber the "high byte + ASCII letter" readings a Windows-1252 word has.
///   An accented letter followed by a letter is one of each, so `Prénom`, `Größe` and their kind
///   stay balanced and keep the Windows-1252 reading, while a kanji-only CSV's dense pairs win:
///   `東京都千代田区` is seven pairs against two such readings.
///
/// Two pairs are required, so a lone pair stays Windows-1252. A kanji-only file is therefore read
/// as Shift-JIS when its pairs are dense; a two-character value whose trails both happen to be
/// ASCII letters can still lose the coin toss and can be read by naming `shift_jis`.
fn cp932_evidence(bytes: &[u8]) -> bool {
    // Half-width katakana are single-byte CP932 characters in the same byte range as Windows-1252's
    // Latin-1 punctuation and uppercase accents (`0xA1`–`0xDF`), so they are evidence neither way:
    // they are left out of the ratio (half-width katakana alone never selects Shift-JIS — it has no
    // pairs at all).
    let is_ambiguous = |b: u8| matches!(b, 0xA1..=0xDF);
    let high = bytes.iter().filter(|&&b| b >= 0x80 && !is_ambiguous(b)).count();
    if high < 2 {
        return false;
    }
    let mut pairs = 0usize;
    let mut consumed = 0usize;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        let lead = matches!(b, 0x81..=0x9F | 0xE0..=0xFC);
        match bytes.get(i + 1) {
            Some(&trail) if lead && matches!(trail, 0x40..=0x7E | 0x80..=0xFC) => {
                pairs += 1;
                consumed += 1; // the lead is always a high, non-ambiguous byte
                if trail >= 0x80 && !is_ambiguous(trail) {
                    consumed += 1;
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    // The same high bytes read as a Windows-1252 character followed by a letter (`é` + `n`).
    let accent_pairs = bytes.windows(2).filter(|w| w[0] >= 0x80 && w[1].is_ascii_alphabetic()).count();
    pairs >= 2 && pairs > accent_pairs && consumed.saturating_mul(5) >= high.saturating_mul(4)
}

/// Whether text holds a CP932 private-use character (`U+E000`–`U+F8FF`). CP932 only produces
/// those from its user-defined area, so their presence means the bytes probably aren't CP932 at
/// all rather than a Japanese file: [`CsvEncoding::Auto`] then keeps the Windows-1252 reading.
fn has_private_use(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{E000}'..='\u{F8FF}'))
}

/// Windows-1252, the historical fallback: bytes 0x80–0x9F use their code points, others are Latin-1.
fn decode_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => char::from_u32(CP1252.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
            _ => b as char,
        })
        .collect()
}

/// Windows-1256, Microsoft's Arabic ANSI code page: bytes 0x80–0xFF use the code points of
/// [`CP1256`], others are ASCII.
fn decode_cp1256(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0xFF => char::from_u32(CP1256.get((b - 0x80) as usize).copied().unwrap_or(0xFFFD)).unwrap_or('\u{FFFD}'),
            _ => b as char,
        })
        .collect()
}

/// Shift-JIS (CP932), with bytes it can't map replaced by `U+FFFD`. `decode_without_bom_handling`
/// keeps an explicit code page from being overridden by a BOM.
fn decode_shift_jis(bytes: &[u8]) -> String {
    encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes).0.into_owned()
}

/// UTF-8, with invalid sequences replaced by `U+FFFD` and a leading BOM dropped (it isn't text).
fn decode_utf8(bytes: &[u8]) -> String {
    let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(rest).into_owned()
}

/// UTF-16 with the given endianness, malformed units replaced by `U+FFFD`.
fn decode_utf16(bytes: &[u8], le: bool) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// [`CsvEncoding::Auto`]: UTF-8/UTF-16 BOMs, else UTF-8, else Shift-JIS when the decoded text holds
/// kana or its pairs are dense enough to be a kanji-only Japanese file ([`cp932_evidence`]), with
/// no private-use characters, else Windows-1256 when the bytes read as Arabic
/// ([`Cp1256Evidence`]), else Windows-1252 — today's fallback, so a file that isn't convincingly
/// Japanese reads as before.
fn decode_auto(bytes: &[u8]) -> (String, Option<CsvEncoding>) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (String::from_utf8_lossy(rest).into_owned(), Some(CsvEncoding::Utf8Bom));
    }
    // UTF-16 has no CSV encoding to write back: saving such a workbook writes UTF-8.
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return (decode_utf16(rest, true), None);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return (decode_utf16(rest, false), None);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), Some(CsvEncoding::Utf8)),
        Err(_) => {
            // Shift-JIS only for a *clean* CP932 decode that looks Japanese: kana, or the dense
            // double-byte pairs of a kanji-only file (see `cp932_evidence`), with no private-use
            // characters (the CP932 user-defined area, which real Japanese text doesn't use).
            // Everything else keeps the historical Windows-1252 reading, so a Western file is
            // never respelled as Japanese.
            let (shift_jis, had_errors) = encoding_rs::SHIFT_JIS.decode_without_bom_handling(bytes);
            let clean_japanese = !had_errors && !has_private_use(&shift_jis);
            // Kana is proof the bytes are CP932, so it is settled first. The weaker kanji-only pair
            // density is tried only after Windows-1256: an Arabic word made of letters in CP932's
            // lead-byte range (`محمد` is `E3 CD E3 CF`) is a clean, dense pair reading too, so
            // measuring Arabic evidence first keeps those files Arabic.
            if clean_japanese && has_kana(&shift_jis) {
                (shift_jis.into_owned(), Some(CsvEncoding::ShiftJis))
            } else if Cp1256Evidence::of(bytes).is_arabic() {
                // Windows-1256 Arabic is read; there is no Windows-1256 export, so saving writes
                // UTF-8 (`None`), as for UTF-16.
                (decode_cp1256(bytes), None)
            } else if clean_japanese && cp932_evidence(bytes) {
                (shift_jis.into_owned(), Some(CsvEncoding::ShiftJis))
            } else {
                (decode_cp1252(bytes), Some(CsvEncoding::Windows1252))
            }
        }
    }
}

/// Decodes CSV bytes: UTF-8/UTF-16 BOMs, else UTF-8, else Shift-JIS when they read as Japanese,
/// else Windows-1256 when they read as Arabic, else Windows-1252.
pub fn decode_text(bytes: &[u8]) -> String {
    decode_text_as(bytes, CsvEncoding::Auto).0
}

/// [`decode_text`] with the code page chosen (or detected for [`CsvEncoding::Auto`]), and the
/// encoding the bytes turned out to be in — what the workbook remembers for saving. `None` means
/// the bytes were UTF-16, or Windows-1256 Arabic that [`CsvEncoding::Auto`] detected: there is no
/// CSV encoding that writes them back, so saving uses UTF-8.
pub fn decode_text_as(bytes: &[u8], encoding: CsvEncoding) -> (String, Option<CsvEncoding>) {
    match encoding {
        CsvEncoding::Auto => decode_auto(bytes),
        CsvEncoding::Utf8 | CsvEncoding::Utf8Bom => (decode_utf8(bytes), Some(encoding)),
        CsvEncoding::ShiftJis => (decode_shift_jis(bytes), Some(CsvEncoding::ShiftJis)),
        CsvEncoding::Windows1252 => (decode_cp1252(bytes), Some(CsvEncoding::Windows1252)),
    }
}

/// Encodes CSV text in a legacy code page: a character the code page cannot map becomes `?`, as
/// Word and Excel write it when saving to a legacy code page — never encoding_rs' default HTML
/// numeric character reference (`&#128512;`), which would corrupt the CSV fields.
fn encode_legacy(text: &str, encoding: &'static encoding_rs::Encoding) -> Vec<u8> {
    let mut encoder = encoding.new_encoder();
    let mut out = Vec::with_capacity(text.len());
    let mut buf = vec![0u8; ENCODE_CHUNK];
    let mut rest = text;
    loop {
        let (result, read, written) = encoder.encode_from_utf8_without_replacement(rest, &mut buf, true);
        if let Some(bytes) = buf.get(..written) {
            out.extend_from_slice(bytes);
        }
        rest = rest.get(read..).unwrap_or_default();
        match result {
            encoding_rs::EncoderResult::InputEmpty => break,
            // The chunk was filled: the next step encodes the rest.
            encoding_rs::EncoderResult::OutputFull => {}
            encoding_rs::EncoderResult::Unmappable(_) => out.push(b'?'),
        }
    }
    out
}

/// Microsoft CP932 has a byte for the wave dash and the double vertical line that WHATWG
/// `shift_jis` only maps when *decoding*; encoding them directly would write `?` (so `1〜5` would
/// export as `1?5`). Excel writes `0x8160`/`0x8161`, which is what their CP932 equivalents encode
/// to, so translate those two before encoding.
fn cp932_text(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains(['\u{301C}', '\u{2016}']) {
        return std::borrow::Cow::Borrowed(text);
    }
    std::borrow::Cow::Owned(
        text.chars()
            .map(|c| match c {
                '\u{301C}' => '\u{FF5E}',
                '\u{2016}' => '\u{2225}',
                c => c,
            })
            .collect(),
    )
}

/// Encodes CSV text in `encoding`. `Auto` writes UTF-8: it applies to reading only. The text is
/// taken by value so the UTF-8 paths reuse its buffer instead of copying it (`into_bytes`): a
/// near-limit export would otherwise hold the CSV twice.
pub(crate) fn encode_text(text: String, encoding: CsvEncoding) -> Vec<u8> {
    match encoding {
        CsvEncoding::Auto | CsvEncoding::Utf8 => text.into_bytes(),
        CsvEncoding::Utf8Bom => {
            // The BOM goes into the buffer's own spare capacity (reserved first, so it does not
            // grow): one allocation and one shift, not a second copy of the whole text.
            let mut text = text;
            text.reserve(3);
            text.insert(0, '\u{FEFF}');
            text.into_bytes()
        }
        CsvEncoding::ShiftJis => encode_legacy(&cp932_text(&text), encoding_rs::SHIFT_JIS),
        CsvEncoding::Windows1252 => encode_legacy(&text, encoding_rs::WINDOWS_1252),
    }
}

/// Picks comma, semicolon or tab: the candidate with the most consistent count per line
/// (outside quotes) over the first non-blank lines. A candidate must appear on every such line, otherwise comma wins.
pub fn sniff_delimiter(text: &str, quote: char) -> u8 {
    let mut best = (b',', 0usize, 0usize);
    for d in [',', ';', '\t'] {
        let mut counts = vec![];
        let mut n = 0usize;
        let mut in_q = false;
        let mut blank = true;
        for ch in text.chars().take(64 * 1024) {
            if !in_q && ch == '\n' {
                // Blank lines carry no delimiter evidence.
                if !blank {
                    counts.push(n);
                }
                n = 0;
                blank = true;
                if counts.len() >= 20 {
                    break;
                }
                continue;
            }
            if ch != '\r' {
                blank = false;
            }
            if ch == quote {
                in_q = !in_q;
            } else if !in_q && ch == d {
                n += 1;
            }
        }
        if !blank {
            counts.push(n);
        }
        let min = counts.iter().copied().min().unwrap_or(0);
        let total: usize = counts.iter().sum();
        if min > 0 && (min, total) > (best.1, best.2) {
            best = (d as u8, min, total);
        }
    }
    best.0
}

/// Splits CSV text into records of fields. A record keeps at most [`MAX_COLS`] fields; the rest are
/// dropped as they are read, so a row of endless delimiters cannot allocate a string per field.
pub(crate) fn parse_records(text: &str, delim: char, quote: char, mut on_record: impl FnMut(Vec<String>) -> bool) {
    let mut field = String::new();
    let mut record: Vec<String> = vec![];
    let mut in_q = false;
    let mut chars = text.chars().peekable();
    let mut field_started_quoted = false;
    while let Some(c) = chars.next() {
        if in_q {
            if c == quote {
                if chars.peek() == Some(&quote) {
                    field.push(quote);
                    chars.next();
                } else {
                    in_q = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        if c == quote && field.is_empty() && !field_started_quoted {
            in_q = true;
            field_started_quoted = true;
        } else if c == delim {
            let f = std::mem::take(&mut field);
            if record.len() < MAX_COLS as usize {
                record.push(f);
            }
            field_started_quoted = false;
        } else if c == '\r' || c == '\n' {
            if c == '\r' && chars.peek() == Some(&'\n') {
                chars.next();
            }
            let f = std::mem::take(&mut field);
            if record.len() < MAX_COLS as usize {
                record.push(f);
            }
            field_started_quoted = false;
            if !on_record(std::mem::take(&mut record)) {
                return;
            }
        } else {
            field.push(c);
        }
    }
    if !field.is_empty() || !record.is_empty() || field_started_quoted {
        if record.len() < MAX_COLS as usize {
            record.push(field);
        }
        on_record(record);
    }
}

pub fn read_csv(bytes: &[u8], opts: &CsvOptions) -> Result<Workbook, IoError> {
    let (text, encoding) = decode_text_as(bytes, opts.encoding);
    let quote = if opts.quote == 0 { '"' } else { opts.quote as char };
    let delim = if opts.delimiter == 0 { sniff_delimiter(&text, quote) } else { opts.delimiter } as char;
    if delim == quote {
        return Err(IoError::Format("the delimiter and quote character must differ".into()));
    }
    let mut wb = Workbook::new();
    wb.date_system = opts.date_system;
    wb.csv_delimiter = u8::try_from(delim).ok();
    // Like the separator: saving this workbook as CSV writes the encoding it was read in.
    wb.csv_encoding = encoding;
    let mut sheet = Sheet::new("Sheet1");
    let mut styles = std::mem::take(&mut wb.styles);
    let mut fmt_cache: Vec<(&'static str, StyleId)> = vec![];
    let mut row: u32 = 0;
    parse_records(&text, delim, quote, |rec| {
        if row >= MAX_ROWS {
            return false;
        }
        // A blank line is an empty row.
        for (col, f) in rec.into_iter().enumerate() {
            if col as u32 >= MAX_COLS {
                break;
            }
            if f.is_empty() {
                continue;
            }
            let pos = CellRef::new(row, col as u32);
            let cell = if opts.parse_values {
                let p = parse_input(&f, opts.date_system);
                let style = match p.format {
                    Some(code) => match fmt_cache.iter().find(|(c, _)| *c == code) {
                        Some((_, id)) => *id,
                        None => {
                            let id = styles.derive(StyleId::DEFAULT, |s| s.num_fmt = NumFmt::new(code));
                            fmt_cache.push((code, id));
                            id
                        }
                    },
                    None => StyleId::DEFAULT,
                };
                Cell { value: p.value, formula: None, style }
            } else {
                Cell::value(Value::Text(Arc::from(f)))
            };
            sheet.cells.set(pos, cell);
        }
        row += 1;
        true
    });
    wb.styles = styles;
    wb.sheets = vec![Arc::new(sheet)];
    Ok(wb)
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
    write_csv_in(sheet, delimiter, CsvEncoding::Utf8, MAX_CSV_BYTES, cell_text)
}

/// [`write_csv`] with the output cap `limit` (bytes) given.
pub fn write_csv_limited(sheet: &Sheet, delimiter: u8, limit: usize, cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    write_csv_in(sheet, delimiter, CsvEncoding::Utf8, limit, cell_text)
}

/// [`write_csv`] in `encoding`: [`CsvEncoding::Utf8Bom`] writes `EF BB BF` before the UTF-8 bytes,
/// [`CsvEncoding::ShiftJis`] and [`CsvEncoding::Windows1252`] encode in that code page (a character
/// it cannot map becomes `?`), and [`CsvEncoding::Auto`] writes UTF-8 without a BOM.
pub fn write_csv_encoded(sheet: &Sheet, delimiter: u8, encoding: CsvEncoding, cell_text: impl FnMut(CellRef) -> String) -> Result<Vec<u8>, IoError> {
    write_csv_in(sheet, delimiter, encoding, MAX_CSV_BYTES, cell_text)
}

fn write_csv_in(
    sheet: &Sheet,
    delimiter: u8,
    encoding: CsvEncoding,
    limit: usize,
    mut cell_text: impl FnMut(CellRef) -> String,
) -> Result<Vec<u8>, IoError> {
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
            let quoted = t.contains(delim) || t.contains('"') || t.contains('\n') || t.contains('\r');
            // The exact size of the field as written (the delimiter was pushed above), so a row
            // never grows past the cap unseen and no doubled-quote copy of the text is built.
            let field_len = if quoted { t.len() + t.matches('"').count() + 2 } else { t.len() };
            if out.len().saturating_add(field_len) > limit {
                return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
            }
            if quoted {
                out.push('"');
                for ch in t.chars() {
                    if ch == '"' {
                        out.push('"');
                    }
                    out.push(ch);
                }
                out.push('"');
            } else {
                out.push_str(&t);
            }
        }
        out.push_str("\r\n");
        if out.len() > limit {
            return Err(too_large(format!("more than {} MiB (stopped at row {})", limit >> 20, row + 1)));
        }
    }
    // UTF-8 is the widest of the encodings here, so the check above is the conservative one; this
    // exact check catches the BOM pushing a just-under-the-limit export over it. `out` is moved in:
    // the UTF-8 path reuses its buffer, so an accepted export is not held twice.
    let bytes = encode_text(out, encoding);
    if bytes.len() > limit {
        return Err(too_large(format!("more than {} MiB when encoded as {}", limit >> 20, encoding.as_str())));
    }
    Ok(bytes)
}
