//! Test support: builds password-encrypted workbooks in code, so tests here and in the crates
//! above (engine, UI) never need an encrypted file on disk. Compiled for this crate's tests and,
//! elsewhere, only with the `test-support` feature (a dev-dependency feature, never shipped).
//! The encryption is `craft-offcrypto`'s and the compound file `craft-cfb`'s.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// The spin count `encrypt_workbook` uses: Excel's default.
pub const SPIN_COUNT: u32 = 100_000;

/// `package` (normally an `.xlsx` zip) as Excel stores it with a password: Agile encryption
/// (AES-256, SHA-512) inside a compound file.
pub fn encrypt_workbook(package: &[u8], password: &str) -> Vec<u8> {
    craft_offcrypto::encrypt_with_spin_count(package, password, SPIN_COUNT).unwrap()
}

/// The same with Standard encryption (AES-128, SHA-1), as Excel 2007 wrote it.
pub fn encrypt_workbook_standard(package: &[u8], password: &str) -> Vec<u8> {
    craft_offcrypto::encrypt_standard(package, password, 128).unwrap()
}

/// A version 3 compound file (512-byte sectors) holding `streams` under the root: streams under
/// 4096 bytes in the mini stream, larger ones in regular sectors.
pub fn compound_file(streams: &[(&str, &[u8])]) -> Vec<u8> {
    craft_cfb::write_streams(craft_cfb::Version::V3, streams).unwrap()
}

/// One cell of a [`numbers_document`].
#[derive(Clone, Copy, Debug)]
pub enum NumbersCell<'a> {
    Empty,
    Text(&'a str),
    /// Rich text: the cell stores a reference to a text storage object.
    Rich(&'a str),
    Number(f64),
    /// Seconds after 2001-01-01 00:00:00 UTC.
    Date(f64),
    Bool(bool),
    /// Seconds.
    Duration(f64),
    Error,
}

/// A table of a [`numbers_document`]: its name and its rows.
pub type NumbersTable<'a> = (&'a str, Vec<Vec<NumbersCell<'a>>>);

fn pb_varint(mut n: u64, out: &mut Vec<u8>) {
    loop {
        let byte = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn pb_field_varint(number: u32, n: u64, out: &mut Vec<u8>) {
    pb_varint(u64::from(number) << 3, out);
    pb_varint(n, out);
}

fn pb_field_bytes(number: u32, data: &[u8], out: &mut Vec<u8>) {
    pb_varint(u64::from(number) << 3 | 2, out);
    pb_varint(data.len() as u64, out);
    out.extend_from_slice(data);
}

fn pb_reference(id: u64) -> Vec<u8> {
    let mut out = Vec::new();
    pb_field_varint(1, id, &mut out);
    out
}

/// A raw Snappy block holding only literals, which is valid and needs no compressor.
pub fn snappy_literals(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    pb_varint(data.len() as u64, &mut out);
    for piece in data.chunks(60_000) {
        let n = piece.len() - 1;
        if n < 60 {
            out.push((n as u8) << 2);
        } else {
            out.push(61 << 2);
            out.extend_from_slice(&(n as u16).to_le_bytes());
        }
        out.extend_from_slice(piece);
    }
    out
}

/// An IWA part: the archives' bytes in 24-bit-length chunks of Snappy blocks.
fn iwa_part(archives: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in archives.chunks(60_000) {
        let block = snappy_literals(chunk);
        out.push(0);
        out.extend_from_slice(&(block.len() as u32).to_le_bytes()[..3]);
        out.extend_from_slice(&block);
    }
    out
}

fn archive(id: u64, kind: u32, payload: &[u8], out: &mut Vec<u8>) {
    let mut info = Vec::new();
    pb_field_varint(1, kind.into(), &mut info);
    pb_field_varint(3, payload.len() as u64, &mut info);
    let mut head = Vec::new();
    pb_field_varint(1, id, &mut head);
    pb_field_bytes(2, &info, &mut head);
    pb_varint(head.len() as u64, out);
    out.extend_from_slice(&head);
    out.extend_from_slice(payload);
}

/// An IEEE 754 decimal128 (BID) number from its shortest decimal spelling.
fn decimal128(n: f64) -> [u8; 16] {
    let text = format!("{n:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap();
    let negative = mantissa.starts_with('-');
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len() as i32);
    let exponent = exponent.parse::<i32>().unwrap() - fraction;
    let mut bits = digits.parse::<u128>().unwrap() | (((exponent + 6176) as u128) << 113);
    if negative {
        bits |= 1 << 127;
    }
    bits.to_le_bytes()
}

struct Lists {
    strings: Vec<(u32, String)>,
    rich: Vec<(u32, u64)>,
}

fn packed_cell(cell: &NumbersCell<'_>, lists: &mut Lists, next_id: &mut u64, extra: &mut Vec<u8>) -> Vec<u8> {
    let (kind, flags, body): (u8, u32, Vec<u8>) = match cell {
        NumbersCell::Empty => (0, 0, vec![]),
        NumbersCell::Text(text) => {
            let key = lists.strings.len() as u32 + 1;
            lists.strings.push((key, (*text).to_string()));
            // A style id (0x20) and a text format id (0x20000) follow the string id, as real files have.
            (3, 0x2_0028, [key.to_le_bytes(), 7u32.to_le_bytes(), 1u32.to_le_bytes()].concat())
        }
        NumbersCell::Rich(text) => {
            let storage = *next_id;
            let payload = *next_id + 1;
            *next_id += 2;
            let mut storage_msg = Vec::new();
            pb_field_bytes(3, text.as_bytes(), &mut storage_msg);
            archive(storage, 2001, &storage_msg, extra);
            let mut payload_msg = Vec::new();
            pb_field_bytes(1, &pb_reference(storage), &mut payload_msg);
            archive(payload, 6218, &payload_msg, extra);
            let key = lists.rich.len() as u32 + 1;
            lists.rich.push((key, payload));
            (9, 0x10, key.to_le_bytes().to_vec())
        }
        NumbersCell::Number(n) => (2, 0x2001, [decimal128(*n).to_vec(), 2u32.to_le_bytes().to_vec()].concat()),
        NumbersCell::Date(s) => (5, 0x4, s.to_le_bytes().to_vec()),
        NumbersCell::Bool(b) => (6, 0x2, f64::from(u8::from(*b)).to_le_bytes().to_vec()),
        NumbersCell::Duration(s) => (7, 0x2, s.to_le_bytes().to_vec()),
        NumbersCell::Error => (8, 0, vec![]),
    };
    let mut out = vec![5, kind, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&flags.to_le_bytes());
    out.extend_from_slice(&body);
    out
}

/// A Numbers package built in code: `sheets` are `(sheet name, tables)`. Rows past 256 start a
/// new tile, as Numbers does, and tile objects go in a second index part.
pub fn numbers_document(sheets: &[(&str, Vec<NumbersTable<'_>>)]) -> Vec<u8> {
    numbers_document_with(sheets, false, false)
}

/// [`numbers_document`] with options: `wide_offsets` stores each cell offset in four-byte units and
/// sets the row's wide-offsets flag; `corrupt_tiles` ends every tile with a truncated field.
pub fn numbers_document_with(sheets: &[(&str, Vec<NumbersTable<'_>>)], wide_offsets: bool, corrupt_tiles: bool) -> Vec<u8> {
    let mut main = Vec::new();
    let mut tiles_part = Vec::new();
    let mut next_id = 100u64;
    let mut sheet_refs = Vec::new();
    for (sheet_name, tables) in sheets {
        let mut drawables = Vec::new();
        for (table_name, rows) in tables {
            let mut lists = Lists { strings: vec![], rich: vec![] };
            let mut tile_entries = Vec::new();
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
            for (tile_index, block) in rows.chunks(256).enumerate() {
                let mut tile = Vec::new();
                for (r, row) in block.iter().enumerate() {
                    let (mut buffer, mut offsets) = (Vec::new(), Vec::new());
                    for cell in row {
                        if matches!(cell, NumbersCell::Empty) {
                            offsets.extend_from_slice(&0xFFFFu16.to_le_bytes());
                            continue;
                        }
                        offsets.extend_from_slice(&((buffer.len() / if wide_offsets { 4 } else { 1 }) as u16).to_le_bytes());
                        let packed = packed_cell(cell, &mut lists, &mut next_id, &mut main);
                        buffer.extend_from_slice(&packed);
                    }
                    let mut info = Vec::new();
                    pb_field_varint(1, r as u64, &mut info);
                    pb_field_varint(2, row.len() as u64, &mut info);
                    pb_field_varint(5, 5, &mut info);
                    pb_field_bytes(6, &buffer, &mut info);
                    pb_field_bytes(7, &offsets, &mut info);
                    if wide_offsets {
                        pb_field_varint(8, 1, &mut info);
                    }
                    pb_field_bytes(5, &info, &mut tile);
                }
                if corrupt_tiles {
                    tile.extend_from_slice(&[0x0A, 0xFF]);
                }
                let tile_id = next_id;
                next_id += 1;
                archive(tile_id, 6002, &tile, &mut tiles_part);
                let mut entry = Vec::new();
                pb_field_varint(1, tile_index as u64, &mut entry);
                pb_field_bytes(2, &pb_reference(tile_id), &mut entry);
                tile_entries.push(entry);
            }
            let list_message = |entries: Vec<Vec<u8>>| {
                let mut msg = Vec::new();
                pb_field_varint(1, 1, &mut msg);
                for entry in entries {
                    pb_field_bytes(3, &entry, &mut msg);
                }
                msg
            };
            let strings = list_message(
                lists
                    .strings
                    .iter()
                    .map(|(key, text)| {
                        let mut e = Vec::new();
                        pb_field_varint(1, u64::from(*key), &mut e);
                        pb_field_varint(2, 1, &mut e);
                        pb_field_bytes(3, text.as_bytes(), &mut e);
                        e
                    })
                    .collect(),
            );
            let rich = list_message(
                lists
                    .rich
                    .iter()
                    .map(|(key, payload)| {
                        let mut e = Vec::new();
                        pb_field_varint(1, u64::from(*key), &mut e);
                        pb_field_varint(2, 1, &mut e);
                        pb_field_bytes(9, &pb_reference(*payload), &mut e);
                        e
                    })
                    .collect(),
            );
            let (string_id, rich_id, model_id, info_id) = (next_id, next_id + 1, next_id + 2, next_id + 3);
            next_id += 4;
            archive(string_id, 6005, &strings, &mut main);
            archive(rich_id, 6005, &rich, &mut main);
            let mut tiles = Vec::new();
            for entry in &tile_entries {
                pb_field_bytes(1, entry, &mut tiles);
            }
            pb_field_varint(2, 256, &mut tiles);
            let mut store = Vec::new();
            pb_field_bytes(3, &tiles, &mut store);
            pb_field_bytes(4, &pb_reference(string_id), &mut store);
            pb_field_bytes(17, &pb_reference(rich_id), &mut store);
            let mut model = Vec::new();
            pb_field_bytes(4, &store, &mut model);
            pb_field_varint(6, rows.len() as u64, &mut model);
            pb_field_varint(7, cols as u64, &mut model);
            pb_field_bytes(8, table_name.as_bytes(), &mut model);
            archive(model_id, 6001, &model, &mut main);
            let mut info = Vec::new();
            pb_field_bytes(2, &pb_reference(model_id), &mut info);
            archive(info_id, 6000, &info, &mut main);
            drawables.push(info_id);
        }
        let sheet_id = next_id;
        next_id += 1;
        let mut sheet = Vec::new();
        pb_field_bytes(1, sheet_name.as_bytes(), &mut sheet);
        for id in drawables {
            pb_field_bytes(2, &pb_reference(id), &mut sheet);
        }
        archive(sheet_id, 2, &sheet, &mut main);
        sheet_refs.push(sheet_id);
    }
    let mut document = Vec::new();
    for id in sheet_refs {
        pb_field_bytes(1, &pb_reference(id), &mut document);
    }
    archive(1, 1, &document, &mut main);
    numbers_package(&[("Index/Document.iwa", &iwa_part(&main)), ("Index/Tables/Tile-100.iwa", &iwa_part(&tiles_part))])
}

/// A Numbers-shaped ZIP from raw parts, for malformed-input tests.
pub fn numbers_package(parts: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in parts {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
