//! VertiPaq column payloads: bit-packed `.idf` segments, dictionaries and value encoding.
//!
//! Implemented in this module so the container reader/writer and the tests share one codec.

use super::model_data::ModelColumnData;
use super::{ModelFileError, Result};

/// Largest id a 32-bit packed segment can carry.
const MAX_ID: i64 = i32::MAX as i64;

/// Bit widths that Excel uses for `XMRENoSplitCompressionInfo<N>` metadata classes.
pub const BIT_WIDTHS: [u32; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 16, 21, 32];

/// Returns the smallest metadata bit width whose value range covers `range` distinct offsets.
pub fn width_for(range: u64) -> Option<u32> {
    BIT_WIDTHS.into_iter().find(|width| if *width >= 64 { true } else { (1u64 << *width) > range })
}

/// One decoded column segment as described by the table metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentInfo {
    pub records: usize,
    pub min: i32,
    pub width: u32,
}

/// The declared storage/encoding shape of one column.
#[derive(Clone, Debug, PartialEq)]
pub enum ColumnEncoding {
    /// Value-encoded long/real: `value = (id + base_id) * magnitude`.
    Value { base_id: i64, magnitude: f64 },
    /// A numeric dictionary file and its element kind.
    HashNumeric { dictionary: String, real: bool, element_bytes: usize },
    /// A string dictionary file.
    HashString { dictionary: String, flags: i64 },
    /// No data file: every row is null.
    None,
}

/// What the engine should surface for a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnKind {
    Empty,
    Integer,
    Real,
    Currency,
    Date,
    Boolean,
    String,
}

impl ColumnKind {
    /// Canonical model type name used by `ModelColumnData::data_type`.
    pub fn type_name(self) -> &'static str {
        match self {
            ColumnKind::Empty => "Empty",
            ColumnKind::Integer => "Int64",
            ColumnKind::Real => "Double",
            ColumnKind::Currency => "Currency",
            ColumnKind::Date => "DateTime",
            ColumnKind::Boolean => "Boolean",
            ColumnKind::String => "String",
        }
    }

    /// Maps the OLE DB `DBType` and `XMType` pair recorded in `ColumnStats`.
    pub fn from_db_type(db_type: i32, xm_type: i32) -> ColumnKind {
        match db_type {
            2..=3 | 18..=21 => ColumnKind::Integer,
            4 | 5 => ColumnKind::Real,
            6 => ColumnKind::Currency,
            7 => ColumnKind::Date,
            11 => ColumnKind::Boolean,
            130 => ColumnKind::String,
            0 | 1 => ColumnKind::Empty,
            _ if xm_type == 2 => ColumnKind::String,
            _ if xm_type == 1 => ColumnKind::Real,
            _ => ColumnKind::Integer,
        }
    }
}

/// Reads one `.idf` record header (`u64` units) and slices its payload.
pub fn read_record<'a>(data: &'a [u8], position: &mut usize) -> Result<&'a [u8]> {
    let end = position.checked_add(8).ok_or_else(|| ModelFileError::Corrupt("idf record offset overflow".into()))?;
    let header = data.get(*position..end).ok_or_else(|| ModelFileError::Corrupt("truncated idf record size".into()))?;
    *position = end;
    let units = usize::try_from(u64::from_le_bytes([header[0], header[1], header[2], header[3], header[4], header[5], header[6], header[7]]))
        .map_err(|_| ModelFileError::TooLarge)?;
    let length = units.checked_mul(8).ok_or(ModelFileError::TooLarge)?;
    let payload_end = position.checked_add(length).ok_or_else(|| ModelFileError::Corrupt("idf payload offset overflow".into()))?;
    let payload = data.get(*position..payload_end).ok_or_else(|| ModelFileError::Corrupt("truncated idf record payload".into()))?;
    *position = payload_end;
    Ok(payload)
}

fn u32_at(payload: &[u8], index: usize) -> Result<u32> {
    let end = index.checked_add(4).ok_or_else(|| ModelFileError::Corrupt("idf entry offset overflow".into()))?;
    let bytes = payload.get(index..end).ok_or_else(|| ModelFileError::Corrupt("truncated idf entry".into()))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn i32_at(payload: &[u8], index: usize) -> Result<i32> {
    Ok(u32_at(payload, index)? as i32)
}

/// Decodes one column-data `.idf` into exactly `segments[i].records` ids per segment.
pub fn decode_column_ids(data: &[u8], segments: &[SegmentInfo]) -> Result<Vec<u32>> {
    let mut position = 0usize;
    let mut output = Vec::new();
    for segment in segments {
        let primary = read_record(data, &mut position)?;
        let subsegment = read_record(data, &mut position)?;
        if primary.len() % 8 != 0 || subsegment.len() % 8 != 0 {
            return Err(ModelFileError::Corrupt("idf record is not a multiple of eight bytes".into()));
        }
        let mut produced = 0usize;
        let mut taken = 0usize;
        let entries = primary.len() / 8;
        let mut entry = 0usize;
        while produced < segment.records && entry < entries {
            let marker = i32_at(primary, entry * 8)?;
            let second = u32_at(primary, entry * 8 + 4)?;
            entry += 1;
            if marker >= 0 {
                let repeat = usize::try_from(second).map_err(|_| ModelFileError::TooLarge)?;
                let remaining = segment.records - produced;
                let count = repeat.min(remaining);
                output.extend(std::iter::repeat_n(marker as u32, count));
                produced += count;
            } else {
                let expected_marker = -(1i64 + i64::try_from(taken).map_err(|_| ModelFileError::TooLarge)?);
                if i64::from(marker) != expected_marker {
                    return Err(ModelFileError::Corrupt("bit-packed entry marker is inconsistent".into()));
                }
                let count = usize::try_from(second).map_err(|_| ModelFileError::TooLarge)?;
                let remaining = segment.records - produced;
                let count = count.min(remaining);
                let values = decode_bitpacked(subsegment, taken, count, segment.width, segment.min)?;
                taken = taken.checked_add(count).ok_or(ModelFileError::TooLarge)?;
                output.extend(values);
                produced += count;
            }
        }
        if produced != segment.records {
            return Err(ModelFileError::Corrupt(format!("idf segment decoded {produced} of {} rows", segment.records)));
        }
    }
    Ok(output)
}

/// Decodes `count` bit-packed ids starting at value index `first`.
pub fn decode_bitpacked(payload: &[u8], first: usize, count: usize, width: u32, min: i32) -> Result<Vec<u32>> {
    if width == 0 || width > 32 {
        return Err(ModelFileError::Corrupt(format!("invalid bit width {width}")));
    }
    let per_word = 64 / usize::try_from(width).map_err(|_| ModelFileError::Corrupt("bit width".into()))?;
    if per_word == 0 {
        return Err(ModelFileError::Corrupt("bit width exceeds a word".into()));
    }
    let mask = (1u64 << width) - 1;
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let value_index = first.checked_add(index).ok_or(ModelFileError::TooLarge)?;
        let word_index = value_index / per_word;
        let start = word_index.checked_mul(8).ok_or(ModelFileError::TooLarge)?;
        let end = start.checked_add(8).ok_or(ModelFileError::TooLarge)?;
        let word = payload.get(start..end).ok_or_else(|| ModelFileError::Corrupt("truncated bit-packed subsegment".into()))?;
        let word = u64::from_le_bytes([word[0], word[1], word[2], word[3], word[4], word[5], word[6], word[7]]);
        let shift = (value_index % per_word) * usize::try_from(width).map_err(|_| ModelFileError::Corrupt("bit width".into()))?;
        let packed = (word >> shift) & mask;
        let value = i64::try_from(packed).map_err(|_| ModelFileError::TooLarge)? + i64::from(min);
        if !(-MAX_ID..=MAX_ID).contains(&value) {
            return Err(ModelFileError::Corrupt("packed id overflows".into()));
        }
        values.push(value as u32);
    }
    Ok(values)
}

/// Encodes `count` ids as bit-packed words (exactly `ceil(count / per_word)` words).
pub fn encode_bitpacked(values: &[u32], width: u32, min: i32) -> Result<Vec<u8>> {
    if width == 0 || width > 32 {
        return Err(ModelFileError::Corrupt(format!("invalid bit width {width}")));
    }
    let per_word = 64usize / width as usize;
    if per_word == 0 {
        return Err(ModelFileError::Corrupt("bit width exceeds a word".into()));
    }
    let words = values.len().div_ceil(per_word);
    let mut output = vec![0u8; words.checked_mul(8).ok_or(ModelFileError::TooLarge)?];
    for (index, value) in values.iter().enumerate() {
        let shifted = i64::from(*value) - i64::from(min);
        if shifted < 0 || shifted > i64::from(u32::MAX) {
            return Err(ModelFileError::Corrupt("id is outside the packed range".into()));
        }
        let word_index = index / per_word;
        let start = word_index * 8;
        let mut word = u64::from_le_bytes([
            output[start],
            output[start + 1],
            output[start + 2],
            output[start + 3],
            output[start + 4],
            output[start + 5],
            output[start + 6],
            output[start + 7],
        ]);
        word |= (shifted as u64) << ((index % per_word) * width as usize);
        output[start..start + 8].copy_from_slice(&word.to_le_bytes());
    }
    Ok(output)
}

/// Result of splitting ids into RLE entries and one or more bit-packed runs.
pub struct EncodedSegments {
    pub bytes: Vec<u8>,
    pub packed_values: usize,
    pub rle_runs: usize,
    pub packed_entries: usize,
}

/// Encodes ids into one column segment (RLE runs of at least 64, bit-packed otherwise).
pub fn encode_column_ids(ids: &[u32], min: i32, width: u32) -> Result<EncodedSegments> {
    let mut primary = Vec::new();
    let mut packed = Vec::new();
    let mut taken = 0usize;
    let mut index = 0usize;
    let mut rle_runs = 0usize;
    let mut packed_entries = 0usize;
    while index < ids.len() {
        let value = ids[index];
        let mut run = 1usize;
        while index + run < ids.len() && ids[index + run] == value {
            run += 1;
        }
        if run >= 64 {
            primary.extend_from_slice(&value.to_le_bytes());
            primary.extend_from_slice(&u32::try_from(run).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
            rle_runs += 1;
            index += run;
            continue;
        }
        // Collect the contiguous stretch of short runs into one bit-packed entry.
        let start = index;
        while index < ids.len() {
            let mut short = 1usize;
            while index + short < ids.len() && ids[index + short] == ids[index] {
                short += 1;
            }
            if short >= 64 {
                break;
            }
            packed.extend_from_slice(&ids[index..index + short]);
            index += short;
        }
        let count = index - start;
        let marker = -(1i64 + i64::try_from(taken).map_err(|_| ModelFileError::TooLarge)?);
        let marker = i32::try_from(marker).map_err(|_| ModelFileError::TooLarge)?;
        primary.extend_from_slice(&marker.to_le_bytes());
        primary.extend_from_slice(&u32::try_from(count).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
        taken = taken.checked_add(count).ok_or(ModelFileError::TooLarge)?;
        packed_entries += 1;
    }
    // Excel writes at least sixteen units (128 bytes) for a column-data primary record, so
    // reproduce that footprint; the trailing units are legal zero padding.
    let primary_units = (primary.len() / 8).max(16);
    primary.resize(primary_units * 8, 0);
    let packed_bytes = encode_bitpacked(&packed, width, min)?;
    let mut bytes = Vec::with_capacity(8 + primary.len() + 8 + packed_bytes.len());
    bytes.extend_from_slice(&u64::try_from(primary_units).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    bytes.extend_from_slice(&primary);
    let packed_units = packed_bytes.len() / 8;
    bytes.extend_from_slice(&u64::try_from(packed_units).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    bytes.extend_from_slice(&packed_bytes);
    Ok(EncodedSegments { bytes, packed_values: packed.len(), rle_runs, packed_entries })
}

/// Encodes `n` consecutive implicit ids for a row-number segment: a 128-byte primary record
/// with `(-1, n)` and an empty bit-packed subsegment.
pub fn encode_row_number(n: usize) -> Result<Vec<u8>> {
    let mut primary = vec![0u8; 128];
    primary[0..4].copy_from_slice(&(-1i32).to_le_bytes());
    primary[4..8].copy_from_slice(&u32::try_from(n).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    let mut bytes = Vec::with_capacity(144);
    bytes.extend_from_slice(&16u64.to_le_bytes());
    bytes.extend_from_slice(&primary);
    bytes.extend_from_slice(&0u64.to_le_bytes());
    Ok(bytes)
}

/// Encodes a `XMRENoSplit` array (hierarchy mapping or relationship index) as one record.
///
/// `min` is the `CompressionInfo` minimum the metadata declares, so the stored value is
/// `value - min`. The bit-packing itself therefore runs from a zero base; subtracting `min`
/// twice (once here and again inside [`encode_bitpacked`]) would shift every value by `min`.
pub fn encode_no_split(values: &[i64], width: u32, min: i32) -> Result<Vec<u8>> {
    let converted: Vec<u32> = values
        .iter()
        .map(|value| {
            let relative = value.checked_sub(i64::from(min)).ok_or_else(|| ModelFileError::Corrupt("no-split value underflows".into()))?;
            u32::try_from(relative).map_err(|_| ModelFileError::Corrupt("no-split value overflows".into()))
        })
        .collect::<Result<_>>()?;
    let mut packed = encode_bitpacked(&converted, width, 0)?;
    let per_word = 64 / width as usize;
    if per_word > 0 && !converted.is_empty() && converted.len().is_multiple_of(per_word) {
        // Excel adds one zero padding word when the values exactly fill their words.
        packed.extend_from_slice(&0u64.to_le_bytes());
    }
    let mut bytes = Vec::with_capacity(8 + packed.len());
    bytes.extend_from_slice(&u64::try_from(packed.len() / 8).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    bytes.extend_from_slice(&packed);
    Ok(bytes)
}

/// Builds a minimal uncompressed string dictionary page.
///
/// Strings are written in id order; ids start at 3, so a string's index in `strings` is its
/// data id minus 3.
pub fn encode_string_dictionary(strings: &[String], nulls: bool) -> Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut handles = Vec::with_capacity(strings.len());
    let mut longest = 0usize;
    for value in strings {
        handles.push(u32::try_from(buffer.len() / 2).map_err(|_| ModelFileError::TooLarge)?);
        let units: Vec<u16> = value.encode_utf16().collect();
        longest = longest.max(units.len());
        for unit in units {
            buffer.extend_from_slice(&unit.to_le_bytes());
        }
        buffer.extend_from_slice(&0u16.to_le_bytes());
    }
    let count = strings.len();
    let mut out = Vec::new();
    out.extend_from_slice(&2i32.to_le_bytes()); // XM_TYPE_STRING
    out.extend_from_slice(&1i32.to_le_bytes()); // hash algorithm (observed)
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&64u32.to_le_bytes());
    out.extend_from_slice(&6u32.to_le_bytes());
    out.extend_from_slice(&u64::MAX.to_le_bytes()); // c_bins = -1
    out.extend_from_slice(&u64::try_from(count).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    out.push(0); // store_compressed
    out.extend_from_slice(&u64::try_from(longest).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    out.extend_from_slice(&1u64.to_le_bytes()); // page count
    out.extend_from_slice(&0u64.to_le_bytes()); // page mask
    out.push(u8::from(nulls));
    out.extend_from_slice(&0u64.to_le_bytes()); // page start index
    out.extend_from_slice(&u64::try_from(count).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    out.push(0); // page not compressed
    out.extend_from_slice(&0xAABB_CCDDu32.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // remaining store
    out.extend_from_slice(&u64::try_from(buffer.len() / 2).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    out.extend_from_slice(&u64::try_from(buffer.len()).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    out.extend_from_slice(&buffer);
    out.extend_from_slice(&0xABCD_ABCDu32.to_le_bytes());
    out.extend_from_slice(&u64::try_from(count).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    for offset in handles {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    Ok(out)
}

/// Builds a numeric dictionary from values in id order.
pub fn encode_numeric_dictionary(values: &[i64], real: bool, element_bytes: usize) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    out.extend_from_slice(&i32::from(real).to_le_bytes());
    out.extend_from_slice(&(-1i32).to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&64u32.to_le_bytes());
    out.extend_from_slice(&6u32.to_le_bytes());
    out.extend_from_slice(&u64::MAX.to_le_bytes());
    out.extend_from_slice(&u64::try_from(values.len()).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    out.extend_from_slice(&u32::try_from(element_bytes).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    for value in values {
        if real {
            out.extend_from_slice(&(f64::from_bits(*value as u64)).to_le_bytes());
        } else if element_bytes == 4 {
            out.extend_from_slice(
                &i32::try_from(*value).map_err(|_| ModelFileError::InvalidModel("integer does not fit the dictionary".into()))?.to_le_bytes(),
            );
        } else {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(out)
}

/// Decodes a numeric dictionary into the values that ids 3, 4, ... map to.
pub fn decode_numeric_dictionary(data: &[u8]) -> Result<(bool, usize, Vec<i64>)> {
    let read = |offset: usize, length: usize| -> Result<&[u8]> {
        let end = offset.checked_add(length).ok_or_else(|| ModelFileError::Corrupt("dictionary offset overflow".into()))?;
        data.get(offset..end).ok_or_else(|| ModelFileError::Corrupt("truncated numeric dictionary".into()))
    };
    let kind = i32::from_le_bytes(read(0, 4)?.try_into().map_err(|_| ModelFileError::Corrupt("dictionary kind".into()))?);
    let real = kind == 1;
    let count = u64::from_le_bytes(read(28, 8)?.try_into().map_err(|_| ModelFileError::Corrupt("dictionary count".into()))?);
    let element_bytes = u32::from_le_bytes(read(36, 4)?.try_into().map_err(|_| ModelFileError::Corrupt("dictionary element size".into()))?) as usize;
    if !matches!(element_bytes, 4 | 8) {
        return Err(ModelFileError::Unsupported(format!("numeric dictionary element size {element_bytes}")));
    }
    let count = usize::try_from(count).map_err(|_| ModelFileError::TooLarge)?;
    let available = data.len().saturating_sub(40) / element_bytes;
    if count > available {
        return Err(ModelFileError::Corrupt("numeric dictionary is truncated".into()));
    }
    let mut values = Vec::with_capacity(count);
    for index in 0..count {
        let offset = 40 + index * element_bytes;
        let bytes = read(offset, element_bytes)?;
        if real {
            let bits = if element_bytes == 8 {
                u64::from_le_bytes(bytes.try_into().map_err(|_| ModelFileError::Corrupt("dictionary value".into()))?)
            } else {
                u64::from(u32::from_le_bytes(bytes.try_into().map_err(|_| ModelFileError::Corrupt("dictionary value".into()))?))
            };
            values.push(bits as i64);
        } else if element_bytes == 4 {
            values.push(i64::from(i32::from_le_bytes(bytes.try_into().map_err(|_| ModelFileError::Corrupt("dictionary value".into()))?)));
        } else {
            values.push(i64::from_le_bytes(bytes.try_into().map_err(|_| ModelFileError::Corrupt("dictionary value".into()))?));
        }
    }
    Ok((real, element_bytes, values))
}

/// Decodes a string dictionary (uncompressed pages only) into id order (id 3 first).
pub fn decode_string_dictionary(data: &[u8], hash_information: bool) -> Result<Vec<String>> {
    let mut position = 0usize;
    let take = |position: &mut usize, length: usize| -> Result<&[u8]> {
        let end = position.checked_add(length).ok_or_else(|| ModelFileError::Corrupt("dictionary offset overflow".into()))?;
        let slice = data.get(*position..end).ok_or_else(|| ModelFileError::Corrupt("truncated string dictionary".into()))?;
        *position = end;
        Ok(slice)
    };
    let kind = i32::from_le_bytes(take(&mut position, 4)?.try_into().map_err(|_| ModelFileError::Corrupt("dictionary kind".into()))?);
    if kind != 2 {
        return Err(ModelFileError::Corrupt("string dictionary has the wrong kind".into()));
    }
    if hash_information {
        take(&mut position, 24)?;
    }
    let count = u64::from_le_bytes(take(&mut position, 8)?.try_into().map_err(|_| ModelFileError::Corrupt("dictionary count".into()))?);
    let _compressed = take(&mut position, 1)?[0];
    let _longest = take(&mut position, 8)?;
    let pages = u64::from_le_bytes(take(&mut position, 8)?.try_into().map_err(|_| ModelFileError::Corrupt("page count".into()))?);
    if count > 16 * 1024 * 1024 || pages > 16 * 1024 * 1024 {
        return Err(ModelFileError::TooLarge);
    }
    if pages > usize::try_from(count).unwrap_or(usize::MAX) as u64 {
        return Err(ModelFileError::Corrupt("string dictionary has more pages than strings".into()));
    }
    let mut page_buffers: Vec<Vec<u8>> = Vec::new();
    for _ in 0..pages {
        let _mask = take(&mut position, 8)?;
        let _nulls = take(&mut position, 1)?[0];
        let _start = take(&mut position, 8)?;
        let _page_strings = take(&mut position, 8)?;
        let page_compressed = take(&mut position, 1)?[0];
        let begin = u32::from_le_bytes(take(&mut position, 4)?.try_into().map_err(|_| ModelFileError::Corrupt("page mark".into()))?);
        if begin != 0xAABB_CCDD {
            return Err(ModelFileError::Corrupt(format!("string dictionary page mark mismatch at {position}")));
        }
        if page_compressed != 0 {
            return Err(ModelFileError::Unsupported("Huffman-compressed string dictionary page".into()));
        }
        let _remaining = take(&mut position, 8)?;
        let _used = take(&mut position, 8)?;
        let allocation = u64::from_le_bytes(take(&mut position, 8)?.try_into().map_err(|_| ModelFileError::Corrupt("page allocation".into()))?);
        let allocation = usize::try_from(allocation).map_err(|_| ModelFileError::TooLarge)?;
        let buffer = take(&mut position, allocation)?;
        let end = u32::from_le_bytes(take(&mut position, 4)?.try_into().map_err(|_| ModelFileError::Corrupt("page mark".into()))?);
        // The specification prints 0xABCDCDAB; every Excel 16 page observed here (and the
        // reference decode) writes 0xABCDABCD. Accept both readings and emit the observed value.
        if end != 0xABCD_CDAB && end != 0xABCD_ABCD {
            return Err(ModelFileError::Corrupt(format!("string dictionary page end mark {end:#010x} at {position} (allocation {allocation})")));
        }
        page_buffers.push(buffer.to_vec());
    }
    let handle_count = u64::from_le_bytes(take(&mut position, 8)?.try_into().map_err(|_| ModelFileError::Corrupt("handle count".into()))?);
    let element_size = u32::from_le_bytes(take(&mut position, 4)?.try_into().map_err(|_| ModelFileError::Corrupt("handle size".into()))?);
    if element_size != 8 {
        return Err(ModelFileError::Unsupported(format!("string dictionary handle size {element_size}")));
    }
    let handle_count = usize::try_from(handle_count).map_err(|_| ModelFileError::TooLarge)?;
    if handle_count != usize::try_from(count).unwrap_or(usize::MAX) {
        return Err(ModelFileError::Corrupt("string dictionary handle count does not match its string count".into()));
    }
    let handle_bytes = handle_count.checked_mul(8).ok_or(ModelFileError::TooLarge)?;
    let handles = take(&mut position, handle_bytes)?;
    // Record handles are in id order; the page's character buffer is not necessarily laid out
    // in id order (Excel sorts pages for RLE), so the offset must be honoured.
    let mut strings = Vec::with_capacity(handle_count);
    for index in 0..handle_count {
        let slot = index.checked_mul(8).ok_or(ModelFileError::TooLarge)?;
        let offset = u32::from_le_bytes(handles[slot..slot + 4].try_into().map_err(|_| ModelFileError::Corrupt("handle".into()))?) as usize;
        let page = u32::from_le_bytes(handles[slot + 4..slot + 8].try_into().map_err(|_| ModelFileError::Corrupt("handle".into()))?) as usize;
        let buffer = page_buffers.get(page).ok_or_else(|| ModelFileError::Corrupt("string handle points outside the pages".into()))?;
        // Excel stores the handle as a UTF-16 code-unit offset for uncompressed pages (the
        // specification calls it a byte offset; the observed files are character based).
        let byte_offset = offset.checked_mul(2).ok_or(ModelFileError::TooLarge)?;
        let rest = buffer.get(byte_offset..).ok_or_else(|| ModelFileError::Corrupt("string handle is outside its page".into()))?;
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
        let length = units.iter().position(|unit| *unit == 0).ok_or_else(|| ModelFileError::Corrupt("unterminated dictionary string".into()))?;
        strings.push(String::from_utf16(&units[..length]).map_err(|_| ModelFileError::Corrupt("invalid UTF-16 dictionary string".into()))?);
    }
    Ok(strings)
}

/// Converts `ModelColumnData` values into ids plus a mapping for hash-encoded columns.
///
/// The caller supplies the base column's encoding so a rewrite keeps its encoding kind.
pub fn value_encoded_ids(column: &ModelColumnData, magnitude: f64) -> Result<(Vec<u32>, i64, i64)> {
    if !magnitude.is_finite() || magnitude == 0.0 {
        return Err(ModelFileError::InvalidModel("column magnitude must be a finite non-zero number".into()));
    }
    let mut scaled: Vec<Option<i64>> = Vec::with_capacity(column.values.len());
    for value in &column.values {
        let raw = match value {
            crate::value::Scalar::Blank => None,
            crate::value::Scalar::Boolean(flag) => Some(i64::from(*flag)),
            crate::value::Scalar::Integer(number) => Some(*number),
            crate::value::Scalar::Currency(scaled) => Some(round_scaled(*scaled as f64 / 10_000.0 / magnitude)?),
            crate::value::Scalar::Decimal(number) => Some(round_scaled(*number / magnitude)?),
            crate::value::Scalar::DateTime(serial) => Some(round_scaled(*serial / magnitude)?),
            crate::value::Scalar::String(_) => {
                return Err(ModelFileError::InvalidModel(format!("text value in value-encoded column {}", column.name)));
            }
        };
        scaled.push(raw);
    }
    let minimum = scaled.iter().flatten().copied().min();
    let Some(minimum) = minimum else {
        return Ok((vec![2; scaled.len()], 0, 2));
    };
    // `value = (id + BaseId) * Magnitude`, so the smallest value must map to id 3:
    // `BaseId = minimum - 3` and `id = value - BaseId`.
    let base_id = minimum.checked_sub(3).ok_or_else(|| ModelFileError::InvalidModel("value range overflows the id space".into()))?;
    let has_nulls = scaled.iter().any(Option::is_none);
    let min = if has_nulls { 2 } else { 3 };
    let mut ids = Vec::with_capacity(scaled.len());
    let mut maximum_id = min;
    for value in scaled {
        let id = match value {
            None => 2,
            Some(number) => {
                let id = number.checked_sub(base_id).ok_or_else(|| ModelFileError::InvalidModel("id overflows".into()))?;
                if !(3..=MAX_ID).contains(&id) {
                    return Err(ModelFileError::InvalidModel("value maps outside the data-id range".into()));
                }
                maximum_id = maximum_id.max(id);
                id
            }
        };
        ids.push(u32::try_from(id).map_err(|_| ModelFileError::InvalidModel("id overflows".into()))?);
    }
    Ok((ids, base_id, maximum_id))
}

fn round_scaled(number: f64) -> Result<i64> {
    if !number.is_finite() || number.abs() > 9.0e15 {
        return Err(ModelFileError::InvalidModel("numeric value is out of range".into()));
    }
    let rounded = number.round();
    if (number - rounded).abs() > 1.0e-6 {
        return Err(ModelFileError::InvalidModel("value is not a multiple of the column magnitude".into()));
    }
    Ok(rounded as i64)
}

/// Entries a `.hidx` header declares per bucket.
const HIDX_ENTRIES_PER_BIN: u32 = 6;
/// Largest bucket chain this encoder emits; beyond it Excel's layout has no free slot.
const HIDX_MAX_CHAIN: usize = 5;
/// Largest bucket count accepted, bounding the allocation at 64 MiB.
const HIDX_MAX_BINS: usize = 1 << 20;

/// Builds the `XMHierarchyDataID2PositionHashIndex` (`.hidx`) used by value-encoded columns.
///
/// `positions` lists the column's data ids in hierarchy position order: position 0 is the null id
/// `2` when the column has nulls, followed by the data ids in value order (which is id order for
/// value encoding). The layout reproduced here is the one Excel 16 writes:
///
/// * a 40-byte header: `-1`, element size 8, page size 64, entries per bucket 6, bucket count,
///   record count and the bucket mask;
/// * a statistics byte (`1`) and two 64-byte prefix blocks: per-bucket occupancy statistics and
///   the histogram of bucket chains longer than one;
/// * one 64-byte block per bucket: a 24-byte header (with the used-slot count at offset 20) and
///   up to five `{u32 data id, u32 position}` slots;
/// * 20 zero bytes of trailer.
///
/// The bucket count reproduces Excel's rule (a quarter of the records, rounded up to a power of
/// two, never below sixteen) and the bucket is `(id * 0x12B9B6A5) >> shift` masked to the bucket
/// count. A chain longer than five entries has no room in this layout, so it is rejected instead
/// of emitting a hash index Excel would refuse.
pub fn encode_hierarchy_hidx(positions: &[u32]) -> Result<Vec<u8>> {
    let records = positions.len();
    let mut bins = 16usize;
    while bins < records.div_ceil(4) {
        bins = bins.checked_mul(2).ok_or(ModelFileError::TooLarge)?;
    }
    if bins > HIDX_MAX_BINS {
        return Err(ModelFileError::Unsupported(format!("hierarchy hash index needs {bins} buckets")));
    }
    let shift = 32 - bins.trailing_zeros();
    let mut buckets: Vec<Vec<(u32, u32)>> = vec![Vec::new(); bins];
    for (position, id) in positions.iter().enumerate() {
        if *id == 0 {
            return Err(ModelFileError::Corrupt("hierarchy hash index holds the null id".into()));
        }
        let bucket = ((*id).wrapping_mul(0x12B9_B6A5) >> shift) as usize & (bins - 1);
        let slot = buckets.get_mut(bucket).ok_or_else(|| ModelFileError::Corrupt("hierarchy hash bucket".into()))?;
        slot.push((*id, u32::try_from(position).map_err(|_| ModelFileError::TooLarge)?));
    }
    let mut used = 0u64;
    let mut singles = 0u64;
    let mut max_chain = 0usize;
    let mut histogram = [0u64; HIDX_MAX_CHAIN - 1];
    for bucket in &buckets {
        if bucket.is_empty() {
            continue;
        }
        used = used.saturating_add(1);
        max_chain = max_chain.max(bucket.len());
        if bucket.len() == 1 {
            singles = singles.saturating_add(1);
        } else if let Some(count) = histogram.get_mut(bucket.len() - 2) {
            *count = count.saturating_add(1);
        }
    }
    if max_chain > HIDX_MAX_CHAIN {
        return Err(ModelFileError::Unsupported(format!(
            "{max_chain} data ids hash to one hierarchy bucket; Excel's index has only {HIDX_MAX_CHAIN} slots"
        )));
    }
    let records_u64 = u64::try_from(records).map_err(|_| ModelFileError::TooLarge)?;
    let bins_u64 = u64::try_from(bins).map_err(|_| ModelFileError::TooLarge)?;
    let mut out = Vec::new();
    out.extend_from_slice(&(-1i32).to_le_bytes());
    out.extend_from_slice(&8u32.to_le_bytes()); // element size
    out.extend_from_slice(&64u32.to_le_bytes()); // page size
    out.extend_from_slice(&HIDX_ENTRIES_PER_BIN.to_le_bytes());
    out.extend_from_slice(&bins_u64.to_le_bytes());
    out.extend_from_slice(&records_u64.to_le_bytes());
    out.extend_from_slice(&(bins_u64 - 1).to_le_bytes()); // bucket mask
    out.push(1); // statistics present
    // Statistics record: record and bucket counts, occupancy summary, then the constant 6/10/8
    // fields Excel writes in every observed file, and finally the singleton count.
    let max_chain_u64 = u64::try_from(max_chain).map_err(|_| ModelFileError::TooLarge)?;
    for value in [records_u64, bins_u64, used, records_u64, u64::from(HIDX_ENTRIES_PER_BIN), max_chain_u64, 10] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&u32::try_from(singles).map_err(|_| ModelFileError::TooLarge)?.to_le_bytes());
    // Histogram of bucket chains 2..=5: one u32 every eight bytes starting at offset 4.
    let mut block = [0u8; 64];
    for (index, count) in histogram.iter().enumerate() {
        let offset = 4 + index * 8;
        let count = u32::try_from(*count).map_err(|_| ModelFileError::TooLarge)?;
        block.get_mut(offset..offset + 4).ok_or_else(|| ModelFileError::Corrupt("hierarchy histogram".into()))?.copy_from_slice(&count.to_le_bytes());
    }
    out.extend_from_slice(&block);
    for bucket in &buckets {
        let mut block = [0u8; 64];
        let count = u32::try_from(bucket.len()).map_err(|_| ModelFileError::TooLarge)?;
        block.get_mut(20..24).ok_or_else(|| ModelFileError::Corrupt("hierarchy bucket header".into()))?.copy_from_slice(&count.to_le_bytes());
        for (index, (id, position)) in bucket.iter().enumerate() {
            let start = 24 + index * 8;
            let slot = block.get_mut(start..start + 8).ok_or_else(|| ModelFileError::Corrupt("hierarchy bucket slot".into()))?;
            slot[0..4].copy_from_slice(&id.to_le_bytes());
            slot[4..8].copy_from_slice(&position.to_le_bytes());
        }
        out.extend_from_slice(&block);
    }
    out.extend_from_slice(&[0u8; 20]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Scalar;

    #[test]
    fn value_encoding_maps_the_minimum_to_id_three() -> Result<()> {
        let column = ModelColumnData {
            name: "C".into(),
            data_type: "Currency".into(),
            values: vec![Scalar::Currency(100), Scalar::Blank, Scalar::Currency(300), Scalar::Currency(600)],
        };
        let (ids, base_id, maximum) = value_encoded_ids(&column, 0.01)?;
        assert_eq!(ids, vec![3, 2, 5, 8]);
        assert_eq!(base_id, 1 - 3);
        assert_eq!(maximum, 8);
        Ok(())
    }

    #[test]
    fn column_ids_round_trip_rle_and_bitpacked_runs() -> Result<()> {
        let mut ids = vec![3u32; 130];
        ids.extend([4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);
        ids.extend(vec![7u32; 80]);
        ids.extend([2, 5, 6]);
        let encoded = encode_column_ids(&ids, 2, 4)?;
        assert_eq!(encoded.rle_runs, 2);
        assert!(encoded.packed_entries >= 1);
        let segments = [SegmentInfo { records: ids.len(), min: 2, width: 4 }];
        let decoded = decode_column_ids(&encoded.bytes, &segments)?;
        assert_eq!(decoded, ids);
        Ok(())
    }

    #[test]
    fn bit_packing_uses_two_values_per_word_at_width_32() -> Result<()> {
        let values = [3u32, 4, 5, 9];
        let encoded = encode_bitpacked(&values, 32, 2)?;
        assert_eq!(encoded.len(), 16);
        assert_eq!(decode_bitpacked(&encoded, 0, 4, 32, 2)?, values);
        Ok(())
    }

    #[test]
    fn no_split_subtracts_its_minimum_once() -> Result<()> {
        // Excel's relationship INDEX declares Min = -1; stored slots are `value + 1`.
        let bytes = encode_no_split(&[-1, 0, 1, 2], 2, -1)?;
        let units = u64::from_le_bytes(bytes[0..8].try_into().map_err(|_| ModelFileError::Corrupt("record".into()))?);
        assert_eq!(units, 1);
        let payload = &bytes[8..];
        assert_eq!(decode_bitpacked(payload, 0, 4, 2, 0)?, vec![0, 1, 2, 3]);
        // The six-value relationship index Excel writes for the reference fixture: values
        // [-1, -1, -1, 0, 1, 2] at width two, Min -1.
        let excel = encode_no_split(&[-1, -1, -1, 0, 1, 2], 2, -1)?;
        assert_eq!(&excel[..], &[1, 0, 0, 0, 0, 0, 0, 0, 0x40, 0x0e, 0, 0, 0, 0, 0, 0]);
        Ok(())
    }

    #[test]
    fn hierarchy_hash_index_matches_the_observed_excel_layout() -> Result<()> {
        // The `Sales[Amount]` hierarchy of the reference fixture: null first, then the fifteen
        // data ids in value order.
        let positions = [2u32, 3, 13, 28, 33, 43, 53, 93, 103, 133, 153, 173, 203, 253, 263, 353];
        let bytes = encode_hierarchy_hidx(&positions)?;
        assert_eq!(bytes.len(), 1213, "header (41) + two prefix blocks (128) + 16 buckets (1024) + trailer (20)");
        assert_eq!(i32::from_le_bytes(bytes[0..4].try_into().map_err(|_| ModelFileError::Corrupt("header".into()))?), -1);
        let word = |offset: usize| -> Result<u64> {
            Ok(u64::from_le_bytes(
                bytes
                    .get(offset..offset + 8)
                    .ok_or_else(|| ModelFileError::Corrupt("word".into()))?
                    .try_into()
                    .map_err(|_| ModelFileError::Corrupt("word".into()))?,
            ))
        };
        assert_eq!(word(16)?, 16, "bucket count");
        assert_eq!(word(24)?, 16, "record count");
        assert_eq!(word(32)?, 15, "bucket mask");
        assert_eq!(bytes[40], 1, "statistics byte");
        // Statistics record, then the histogram block.
        assert_eq!(word(41)?, 16);
        assert_eq!(word(49)?, 16);
        assert_eq!(word(57)?, 11, "used buckets");
        assert_eq!(word(65)?, 16);
        assert_eq!(word(73)?, 6);
        assert_eq!(word(81)?, 3, "longest chain");
        assert_eq!(word(89)?, 10);
        assert_eq!(&bytes[145..169], &[0u8; 24]);
        // Every entry decodes back to its position; every bucket's slot count matches its header.
        let mut seen = 0usize;
        for bucket in 0..16usize {
            let start = 169 + bucket * 64;
            let count = u32::from_le_bytes(
                bytes
                    .get(start + 20..start + 24)
                    .ok_or_else(|| ModelFileError::Corrupt("bucket".into()))?
                    .try_into()
                    .map_err(|_| ModelFileError::Corrupt("bucket".into()))?,
            ) as usize;
            assert!(count <= 5);
            for slot in 0..count {
                let offset = start + 24 + slot * 8;
                let id = u32::from_le_bytes(
                    bytes
                        .get(offset..offset + 4)
                        .ok_or_else(|| ModelFileError::Corrupt("slot".into()))?
                        .try_into()
                        .map_err(|_| ModelFileError::Corrupt("slot".into()))?,
                );
                let position = u32::from_le_bytes(
                    bytes
                        .get(offset + 4..offset + 8)
                        .ok_or_else(|| ModelFileError::Corrupt("slot".into()))?
                        .try_into()
                        .map_err(|_| ModelFileError::Corrupt("slot".into()))?,
                );
                assert_eq!(positions.get(position as usize), Some(&id));
                seen += 1;
            }
        }
        assert_eq!(seen, positions.len());
        Ok(())
    }

    #[test]
    fn hierarchy_hash_index_rejects_a_chain_with_no_free_slot() -> Result<()> {
        // With sixteen buckets the shift is 28, so collect six ids landing in one bucket.
        let mut colliding = Vec::new();
        for id in 3u32..10_000 {
            if (id.wrapping_mul(0x12B9_B6A5) >> 28) & 15 == 0 {
                colliding.push(id);
                if colliding.len() == 6 {
                    break;
                }
            }
        }
        assert_eq!(colliding.len(), 6);
        match encode_hierarchy_hidx(&colliding) {
            Err(ModelFileError::Unsupported(message)) => assert!(message.contains("hash"), "{message}"),
            other => panic!("expected a rejected chain, got {other:?}"),
        }
        Ok(())
    }

    #[test]
    fn string_dictionary_round_trips() -> Result<()> {
        let strings = vec!["Alpha".to_string(), "Beta".to_string(), "Ventes Été".to_string()];
        let bytes = encode_string_dictionary(&strings, true)?;
        assert_eq!(decode_string_dictionary(&bytes, true)?, strings);
        Ok(())
    }

    #[test]
    fn numeric_dictionary_round_trips() -> Result<()> {
        let values = [-5i64, 0, 1_000_000];
        let bytes = encode_numeric_dictionary(&values, false, 8)?;
        let (real, element_bytes, decoded) = decode_numeric_dictionary(&bytes)?;
        assert!(!real);
        assert_eq!(element_bytes, 8);
        assert_eq!(decoded, values);
        Ok(())
    }

    #[test]
    fn truncated_dictionary_and_idf_are_errors() {
        let strings = vec!["x".to_string(); 4];
        let bytes = encode_string_dictionary(&strings, false).expect("encode");
        for end in 0..bytes.len() {
            assert!(decode_string_dictionary(&bytes[..end], true).is_err(), "prefix {end}");
        }
        let ids = vec![3u32; 10];
        let encoded = encode_column_ids(&ids, 3, 1).expect("encode");
        for end in 0..encoded.bytes.len() {
            let segments = [SegmentInfo { records: ids.len(), min: 3, width: 1 }];
            assert!(decode_column_ids(&encoded.bytes[..end], &segments).is_err(), "prefix {end}");
        }
    }
}
