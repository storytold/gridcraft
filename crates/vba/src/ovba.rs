//! [MS-OVBA] building blocks: the compression container used by the `dir` stream and module
//! source (§2.4.1).
//!
//! [MS-OVBA]: https://learn.microsoft.com/en-us/openspecs/office_file_formats/ms-ovba/

/// The most bytes one stream may decompress to (16 MiB). Callers pass this as `max_out`.
pub const MAX_DECOMPRESSED: usize = 16 * 1024 * 1024;

/// CompressedContainer.SignatureByte (§2.4.1.1.1).
const SIGNATURE_BYTE: u8 = 0x01;
/// The most bytes a DecompressedChunk holds (§2.4.1.1.3), and the size of a raw chunk's data
/// (§2.4.1.3.3).
const DECOMPRESSED_CHUNK_MAX: usize = 4096;
/// CompressedChunkHeader length in bytes (§2.4.1.1.5).
const HEADER_LEN: usize = 2;
/// CompressedChunkHeader bits 0–11: CompressedChunkSize, the chunk length minus 3.
const SIZE_MASK: u16 = 0x0FFF;
/// CompressedChunkHeader bits 12–14: CompressedChunkSignature, which MUST be 0b011.
const SIGNATURE_MASK: u16 = 0x7000;
const SIGNATURE_BITS: u16 = 0x3000;
/// CompressedChunkHeader bit 15: CompressedChunkFlag (1 = token sequences, 0 = raw).
const COMPRESSED_FLAG: u16 = 0x8000;
/// A raw chunk's CompressedChunkSize MUST be 4095, i.e. 4098 bytes with the header (§2.4.1.1.5).
const RAW_SIZE_FIELD: u16 = 0x0FFF;

/// Why a CompressedContainer couldn't be decompressed. `at` is a byte offset into the input.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OvbaError {
    /// The input is empty or doesn't start with 0x01 (§2.4.1.1.1).
    #[error("the compressed data is empty or doesn't start with the 0x01 signature byte")]
    InvalidSignature,
    /// A chunk header is cut short or has a bad signature, a chunk reaches past the end of the
    /// input, a raw chunk isn't 4096 bytes, or a chunk decompresses to more than 4096 bytes.
    /// `at` is the chunk header's offset.
    #[error("the compressed chunk at byte {at} is damaged or cut short")]
    InvalidChunk { at: usize },
    /// A copy token reaches back before the start of its decompressed chunk, or would grow the
    /// chunk past 4096 bytes. `at` is the token's offset.
    #[error("the copy token at byte {at} points outside its chunk")]
    InvalidCopyToken { at: usize },
    /// The output would be larger than the caller's limit.
    #[error("the decompressed data is larger than the allowed limit")]
    OutputLimit,
}

/// Decompresses an [MS-OVBA] CompressedContainer (§2.4.1.1.1) per the Decompression algorithm
/// (§2.4.1.3.1), returning at most `max_out` bytes (callers pass [`MAX_DECOMPRESSED`]).
///
/// Each CompressedChunk (§2.4.1.1.4) starts with a little-endian CompressedChunkHeader
/// (§2.4.1.1.5). Compressed chunks hold TokenSequences (§2.4.1.1.7, §2.4.1.3.4): a FlagByte, then
/// up to eight literal bytes or CopyTokens (§2.4.1.1.8, §2.4.1.3.5), where the token's
/// offset/length split depends on the position in the decompressed chunk (CopyToken Help
/// §2.4.1.3.19.1, Unpack CopyToken §2.4.1.3.19.2). Copies may overlap and run byte by byte
/// (Byte Copy, §2.4.1.3.11). Raw chunks hold 4096 bytes copied as is (§2.4.1.3.3).
///
/// This is stricter than the pseudocode in two places, as the structure rules require: a chunk
/// whose declared size reaches past the input is an error rather than read up to the end, and a
/// raw chunk must declare the full 4098 bytes.
///
/// # Errors
///
/// Returns an [`OvbaError`] for a bad signature, a damaged chunk, a copy token outside its
/// chunk, or output above `max_out`. No partial output is returned on error.
pub fn decompress(input: &[u8], max_out: usize) -> Result<Vec<u8>, OvbaError> {
    if input.first() != Some(&SIGNATURE_BYTE) {
        return Err(OvbaError::InvalidSignature);
    }
    let mut out = Vec::new();
    let mut chunk_start = 1;
    while chunk_start < input.len() {
        chunk_start = decompress_chunk(input, chunk_start, &mut out, max_out)?;
    }
    Ok(out)
}

/// Decompressing a CompressedChunk (§2.4.1.3.2). Returns the offset of the next chunk.
fn decompress_chunk(input: &[u8], chunk_start: usize, out: &mut Vec<u8>, max_out: usize) -> Result<usize, OvbaError> {
    let bad_chunk = OvbaError::InvalidChunk { at: chunk_start };
    let header = read_u16(input, chunk_start).ok_or_else(|| bad_chunk.clone())?;
    if header & SIGNATURE_MASK != SIGNATURE_BITS {
        return Err(bad_chunk);
    }
    // Extract CompressedChunkSize (§2.4.1.3.12): the chunk length is the field plus 3.
    let size_field = header & SIZE_MASK;
    let chunk_len = usize::from(size_field).checked_add(3).ok_or_else(|| bad_chunk.clone())?;
    let chunk_end = chunk_start.checked_add(chunk_len).filter(|&end| end <= input.len()).ok_or_else(|| bad_chunk.clone())?;
    let data_start = chunk_start.checked_add(HEADER_LEN).ok_or_else(|| bad_chunk.clone())?;

    if header & COMPRESSED_FLAG == 0 {
        // Decompressing a RawChunk (§2.4.1.3.3): exactly 4096 bytes, copied as is.
        if size_field != RAW_SIZE_FIELD {
            return Err(bad_chunk);
        }
        let data = input.get(data_start..chunk_end).ok_or(bad_chunk)?;
        reserve_within(out, data.len(), max_out)?;
        out.extend_from_slice(data);
        return Ok(chunk_end);
    }

    let chunk_out_start = out.len();
    let mut current = data_start;
    // Decompressing a TokenSequence (§2.4.1.3.4), until the chunk's bytes run out.
    while current < chunk_end {
        let flags = *input.get(current).ok_or_else(|| bad_chunk.clone())?;
        current = current.checked_add(1).ok_or_else(|| bad_chunk.clone())?;
        for index in 0..8u32 {
            if current >= chunk_end {
                break;
            }
            // Extract FlagBit (§2.4.1.3.17): bit `index`, 0 = literal, 1 = CopyToken.
            if (flags >> index) & 1 == 0 {
                let byte = *input.get(current).ok_or_else(|| bad_chunk.clone())?;
                if out.len().saturating_sub(chunk_out_start) >= DECOMPRESSED_CHUNK_MAX {
                    return Err(bad_chunk);
                }
                reserve_within(out, 1, max_out)?;
                out.push(byte);
                current = current.checked_add(1).ok_or_else(|| bad_chunk.clone())?;
            } else {
                // A CopyToken is two bytes and must lie inside this chunk.
                let token_end = current.checked_add(2).filter(|&end| end <= chunk_end).ok_or_else(|| bad_chunk.clone())?;
                let token = read_u16(input, current).ok_or_else(|| bad_chunk.clone())?;
                copy_token(out, chunk_out_start, token, current, max_out)?;
                current = token_end;
            }
        }
    }
    Ok(chunk_end)
}

/// Decodes one CopyToken at input offset `at` (§2.4.1.3.5) and appends its bytes.
fn copy_token(out: &mut Vec<u8>, chunk_out_start: usize, token: u16, at: usize, max_out: usize) -> Result<(), OvbaError> {
    let bad_token = OvbaError::InvalidCopyToken { at };
    let current = out.len();
    let difference = current.checked_sub(chunk_out_start).ok_or_else(|| bad_token.clone())?;
    let (offset, length) = unpack_copy_token(token, difference);
    // The source must not reach before the start of the decompressed chunk (§2.4.1.1.8).
    if offset > difference {
        return Err(bad_token);
    }
    // A DecompressedChunk holds at most 4096 bytes (§2.4.1.1.3).
    let chunk_len_after = difference.checked_add(length).ok_or_else(|| bad_token.clone())?;
    if chunk_len_after > DECOMPRESSED_CHUNK_MAX {
        return Err(bad_token);
    }
    let source = current.checked_sub(offset).ok_or_else(|| bad_token.clone())?;
    reserve_within(out, length, max_out)?;
    // Byte Copy (§2.4.1.3.11): byte by byte, so an overlapping copy repeats its own output.
    for step in 0..length {
        let from = source.checked_add(step).ok_or_else(|| bad_token.clone())?;
        let byte = *out.get(from).ok_or_else(|| bad_token.clone())?;
        out.push(byte);
    }
    Ok(())
}

/// CopyToken Help (§2.4.1.3.19.1) and Unpack CopyToken (§2.4.1.3.19.2): splits `token` into
/// (Offset, Length) for a token found `difference` bytes into the decompressed chunk.
fn unpack_copy_token(token: u16, difference: usize) -> (usize, usize) {
    let bit_count = copy_token_bit_count(difference);
    let length_mask = 0xFFFFu16.checked_shr(bit_count).unwrap_or(0);
    let offset_bits = token.checked_shr(16u32.saturating_sub(bit_count)).unwrap_or(0);
    let offset = usize::from(offset_bits).saturating_add(1);
    let length = usize::from(token & length_mask).saturating_add(3);
    (offset, length)
}

/// CopyToken Help (§2.4.1.3.19.1): BitCount, the number of offset bits in a CopyToken found
/// `difference` bytes into the decompressed chunk.
fn copy_token_bit_count(difference: usize) -> u32 {
    // BitCount = max(ceil(log2(difference)), 4). ceil(log2(d)) is the bit width of d - 1 for
    // d >= 1; d = 0 (a token first in its chunk) gets the minimum and fails the offset check.
    // Clamped to 12 because a decompressed chunk never exceeds 4096 bytes.
    let width = usize::BITS.saturating_sub(difference.saturating_sub(1).leading_zeros());
    width.clamp(4, 12)
}

/// Compresses `data` into an [MS-OVBA] CompressedContainer per the Compression algorithm
/// (§2.4.1.3.6). Test-only: it exists to check [`decompress`] and to build fixtures.
///
/// Each 4096-byte DecompressedChunk becomes one CompressedChunk (§2.4.1.3.7) of TokenSequences
/// (§2.4.1.3.8, §2.4.1.3.9). Matches come from the spec's Matching search (§2.4.1.3.19.4): every
/// earlier position in the chunk is tried, nearest first, and the longest wins (ties go to the
/// nearest). A match of three or more bytes becomes a CopyToken, clipped to the longest length
/// the token can hold at that position; anything shorter is a literal.
///
/// If the tokens don't fit in 4096 bytes, the chunk is stored raw (§2.4.1.3.10). As the spec
/// says, a raw chunk is always 4096 bytes, so a short last chunk stored raw is padded with 0x00
/// and decompresses to more bytes than went in. That only happens for a last chunk of 3,641 to
/// 4,095 bytes that barely compresses: with fewer bytes the literals and flag bytes still fit.
#[cfg(any(test, feature = "test-support"))]
pub fn compress(data: &[u8]) -> Vec<u8> {
    let mut out = vec![SIGNATURE_BYTE];
    // `chunks` never yields an empty slice, so empty input is the signature byte alone.
    for chunk in data.chunks(DECOMPRESSED_CHUNK_MAX) {
        compress_chunk(chunk, &mut out);
    }
    out
}

/// Compressing a DecompressedChunk (§2.4.1.3.7): appends one CompressedChunk for `chunk`
/// (1 to 4096 bytes) to `out`.
#[cfg(any(test, feature = "test-support"))]
fn compress_chunk(chunk: &[u8], out: &mut Vec<u8>) {
    // CompressedEnd: a CompressedChunk is at most 4098 bytes, header included.
    const COMPRESSED_END: usize = HEADER_LEN + DECOMPRESSED_CHUNK_MAX;
    let mut buf = Vec::with_capacity(COMPRESSED_END);
    buf.extend_from_slice(&[0, 0]); // the header, filled in below
    let mut current = 0; // DecompressedCurrent, relative to the chunk start
    let mut full = false; // Compressing a Token's "SET CompressedCurrent TO CompressedEnd"
    while current < chunk.len() && buf.len() < COMPRESSED_END && !full {
        // Compressing a TokenSequence (§2.4.1.3.8): reserve the FlagByte, then up to 8 tokens.
        let flag_index = buf.len();
        buf.push(0);
        let mut flags = 0u8;
        for index in 0..8u32 {
            if current >= chunk.len() || buf.len() >= COMPRESSED_END {
                break;
            }
            // Compressing a Token (§2.4.1.3.9).
            if let Some((offset, length)) = find_match(chunk, current) {
                if buf.len().saturating_add(1) >= COMPRESSED_END {
                    full = true;
                    break;
                }
                buf.extend_from_slice(&pack_copy_token(offset, length, current).to_le_bytes());
                flags |= 1u8.checked_shl(index).unwrap_or(0);
                current = current.saturating_add(length);
            } else {
                let Some(&byte) = chunk.get(current) else { break };
                buf.push(byte);
                current = current.saturating_add(1);
            }
        }
        if let Some(slot) = buf.get_mut(flag_index) {
            *slot = flags;
        }
    }
    if current < chunk.len() {
        // Compressing a RawChunk (§2.4.1.3.10): the bytes as is, padded with 0x00 to 4096.
        out.extend_from_slice(&(SIGNATURE_BITS | RAW_SIZE_FIELD).to_le_bytes());
        out.extend_from_slice(chunk);
        out.resize(out.len().saturating_add(DECOMPRESSED_CHUNK_MAX.saturating_sub(chunk.len())), 0);
        return;
    }
    // Pack CompressedChunkSize/Flag/Signature (§2.4.1.3.13, §2.4.1.3.14, §2.4.1.3.16). The
    // chunk holds at least the header, a FlagByte and one token, so the size is at least 4.
    let size_field = u16::try_from(buf.len().saturating_sub(3)).unwrap_or(SIZE_MASK) & SIZE_MASK;
    let header = COMPRESSED_FLAG | SIGNATURE_BITS | size_field;
    if let Some(slot) = buf.get_mut(..HEADER_LEN) {
        slot.copy_from_slice(&header.to_le_bytes());
    }
    out.extend_from_slice(&buf);
}

/// Matching (§2.4.1.3.19.4): the (Offset, Length) of the longest earlier match for the bytes at
/// `current` in `chunk`, or `None` if it is shorter than three bytes. A match may overlap
/// `current`. Ties go to the nearest candidate, as in the spec's backward scan.
#[cfg(any(test, feature = "test-support"))]
fn find_match(chunk: &[u8], current: usize) -> Option<(usize, usize)> {
    let ahead = chunk.get(current..)?;
    let mut best_length = 0;
    let mut best_candidate = 0;
    for candidate in (0..current).rev() {
        let from = chunk.get(candidate..)?;
        let length = ahead.iter().zip(from).take_while(|(a, b)| a == b).count();
        if length > best_length {
            best_length = length;
            best_candidate = candidate;
            if length == ahead.len() {
                break; // nothing further back can be strictly longer
            }
        }
    }
    if best_length < 3 {
        return None;
    }
    // MaximumLength from CopyToken Help (§2.4.1.3.19.1).
    let max_length = 0xFFFFusize.checked_shr(copy_token_bit_count(current)).unwrap_or(0).saturating_add(3);
    Some((current.saturating_sub(best_candidate), best_length.min(max_length)))
}

/// Pack CopyToken (§2.4.1.3.19.3) for a token `difference` bytes into the decompressed chunk.
/// `offset` is 1..=difference and `length` 3..=MaximumLength, so both fields fit.
#[cfg(any(test, feature = "test-support"))]
fn pack_copy_token(offset: usize, length: usize, difference: usize) -> u16 {
    let bit_count = copy_token_bit_count(difference);
    let offset_part = offset.saturating_sub(1).checked_shl(16u32.saturating_sub(bit_count)).unwrap_or(0);
    let token = offset_part | length.saturating_sub(3);
    u16::try_from(token).unwrap_or(0)
}

/// Fails with [`OvbaError::OutputLimit`] if `extra` more bytes would take `out` past `max_out`.
/// Checked before every append, so the output never grows past the cap.
fn reserve_within(out: &[u8], extra: usize, max_out: usize) -> Result<(), OvbaError> {
    match out.len().checked_add(extra) {
        Some(total) if total <= max_out => Ok(()),
        _ => Err(OvbaError::OutputLimit),
    }
}

/// Reads a little-endian u16 at `at`, or `None` if fewer than two bytes remain.
fn read_u16(input: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    match input.get(at..end)? {
        [lo, hi] => Some(u16::from_le_bytes([*lo, *hi])),
        _ => None,
    }
}

/// Property tests P1 (round trip) and P2 (arbitrary input), in `ovba/props.rs`.
#[cfg(test)]
mod props;

#[cfg(test)]
mod tests {
    use super::*;

    /// [MS-OVBA] §3.2.1 No Compression Example.
    #[test]
    fn spec_example_no_compression() {
        let compressed = [
            0x01, 0x19, 0xB0, 0x00, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x00, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70, 0x00, 0x71,
            0x72, 0x73, 0x74, 0x75, 0x76, 0x2E,
        ];
        let out = decompress(&compressed, MAX_DECOMPRESSED).unwrap();
        assert_eq!(out, b"abcdefghijklmnopqrstuv.");
    }

    /// [MS-OVBA] §3.2.2 Normal Compression Example.
    #[test]
    fn spec_example_normal_compression() {
        let compressed = [
            0x01, 0x2F, 0xB0, 0x00, 0x23, 0x61, 0x61, 0x61, 0x62, 0x63, 0x64, 0x65, 0x82, 0x66, 0x00, 0x70, 0x61, 0x67, 0x68, 0x69, 0x6A, 0x01, 0x38,
            0x08, 0x61, 0x6B, 0x6C, 0x00, 0x20, 0x6D, 0x6E, 0x6F, 0x70, 0x06, 0x71, 0x02, 0x70, 0x04, 0x00, 0x72, 0x73, 0x74, 0x75, 0x76, 0x10, 0x77,
            0x78, 0x79, 0x7A, 0x00, 0x2C,
        ];
        let out = decompress(&compressed, MAX_DECOMPRESSED).unwrap();
        assert_eq!(out, b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa");
    }

    /// [MS-OVBA] §3.2.3 Maximum Compression Example: 73 × 'a'.
    #[test]
    fn spec_example_maximum_compression() {
        let compressed = [0x01, 0x03, 0xB0, 0x02, 0x61, 0x45, 0x00];
        let out = decompress(&compressed, MAX_DECOMPRESSED).unwrap();
        assert_eq!(out, vec![b'a'; 73]);
    }

    #[test]
    fn signature_only_is_empty() {
        assert_eq!(decompress(&[0x01], MAX_DECOMPRESSED), Ok(Vec::new()));
        assert_eq!(decompress(&[0x01], 0), Ok(Vec::new()));
    }

    #[test]
    fn empty_or_wrong_signature_is_rejected() {
        assert_eq!(decompress(&[], MAX_DECOMPRESSED), Err(OvbaError::InvalidSignature));
        assert_eq!(decompress(&[0x00, 0x19, 0xB0], MAX_DECOMPRESSED), Err(OvbaError::InvalidSignature));
        assert_eq!(decompress(&[0x02], MAX_DECOMPRESSED), Err(OvbaError::InvalidSignature));
    }

    #[test]
    fn bad_chunk_signature_is_rejected() {
        // 0x8000: flag set, signature bits 0b000 instead of 0b011.
        let input = [0x01, 0x00, 0x80, 0x00];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidChunk { at: 1 }));
        // 0xF000: signature bits 0b111.
        let input = [0x01, 0x00, 0xF0, 0x00];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidChunk { at: 1 }));
    }

    #[test]
    fn chunk_past_end_of_input_is_rejected() {
        // Declares 0x19 + 3 = 28 bytes, but only 4 follow the signature.
        let input = [0x01, 0x19, 0xB0, 0x00, 0x61];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidChunk { at: 1 }));
        // A header cut to one byte.
        assert_eq!(decompress(&[0x01, 0x19], MAX_DECOMPRESSED), Err(OvbaError::InvalidChunk { at: 1 }));
        // The §3.2.1 example with its last byte missing.
        let mut truncated = vec![0x01, 0x19, 0xB0, 0x00];
        truncated.extend_from_slice(b"abcdefgh");
        assert_eq!(decompress(&truncated, MAX_DECOMPRESSED), Err(OvbaError::InvalidChunk { at: 1 }));
    }

    #[test]
    fn copy_token_cut_by_chunk_end_is_rejected() {
        // Chunk = header + flag 0x01 + one byte: the CopyToken needs two.
        let input = [0x01, 0x01, 0xB0, 0x01, 0x00];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidChunk { at: 1 }));
    }

    fn raw_chunk(data: &[u8; 4096]) -> Vec<u8> {
        let mut chunk = vec![0xFF, 0x3F]; // size field 4095, signature 0b011, flag 0
        chunk.extend_from_slice(data);
        chunk
    }

    /// A raw chunk (CompressedChunkFlag 0) holding 4096 bytes decompresses to those bytes unchanged.
    #[test]
    fn raw_chunk_is_copied() {
        let mut data = [0u8; 4096];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = (i % 251) as u8;
        }
        let mut input = vec![0x01];
        input.extend(raw_chunk(&data));
        assert_eq!(decompress(&input, MAX_DECOMPRESSED).unwrap(), data.to_vec());
    }

    #[test]
    fn raw_chunk_must_be_4096_bytes() {
        // Flag 0 with a size field of 2: a raw chunk must declare 4095.
        let input = [0x01, 0x02, 0x30, 0x61, 0x62, 0x63];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidChunk { at: 1 }));
    }

    #[test]
    fn raw_chunk_then_compressed_chunk() {
        let data = [b'x'; 4096];
        let mut input = vec![0x01];
        input.extend(raw_chunk(&data));
        input.extend_from_slice(&[0x19, 0xB0, 0x00]);
        input.extend_from_slice(b"abcdefgh");
        input.push(0x00);
        input.extend_from_slice(b"ijklmnop");
        input.push(0x00);
        input.extend_from_slice(b"qrstuv.");
        let mut expected = data.to_vec();
        expected.extend_from_slice(b"abcdefghijklmnopqrstuv.");
        assert_eq!(decompress(&input, MAX_DECOMPRESSED).unwrap(), expected);
    }

    #[test]
    fn copy_token_before_chunk_start_is_rejected() {
        // First token in the chunk is a copy: nothing to copy from (offset 1 > difference 0).
        let input = [0x01, 0x02, 0xB0, 0x01, 0x00, 0x00];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidCopyToken { at: 4 }));
        // "ab", then a copy with offset 3 (token 0x2000 at difference 2): one byte too far back.
        let input = [0x01, 0x04, 0xB0, 0x04, 0x61, 0x62, 0x00, 0x20];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidCopyToken { at: 6 }));
    }

    #[test]
    fn copy_token_cannot_reach_into_previous_chunk() {
        // A full raw chunk, then a compressed chunk whose first token copies offset 1.
        let mut input = vec![0x01];
        input.extend(raw_chunk(&[b'x'; 4096]));
        input.extend_from_slice(&[0x02, 0xB0, 0x01, 0x00, 0x00]);
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidCopyToken { at: 4102 }));
    }

    #[test]
    fn overlapping_copy_repeats_output() {
        // "ab", then copy offset 2 length 5 (token (1 << 12) | 2 at difference 2): "abababa".
        let input = [0x01, 0x04, 0xB0, 0x04, 0x61, 0x62, 0x02, 0x10];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED).unwrap(), b"abababa");
    }

    #[test]
    fn offset_length_split_widens_with_position() {
        // 17 literals put the token at difference 17: BitCount 5, so token 0x0800 is
        // offset (0x0800 >> 11) + 1 = 2 and length (0x0800 & 0x07FF) + 3 = 3.
        let mut input = vec![0x01, 0x00, 0x00, 0x00];
        input.extend_from_slice(b"abcdefgh");
        input.push(0x00);
        input.extend_from_slice(b"ijklmnop");
        input.push(0x02); // literal 'q', then the token
        input.extend_from_slice(&[b'q', 0x00, 0x08]);
        let size_field = (input.len() - 1 - 3) as u16;
        let header = 0xB000 | size_field;
        input[1..3].copy_from_slice(&header.to_le_bytes());
        assert_eq!(decompress(&input, MAX_DECOMPRESSED).unwrap(), b"abcdefghijklmnopqpqp");
    }

    #[test]
    fn decompressed_chunk_cannot_exceed_4096_bytes() {
        // 'a', then copy offset 1 with the longest length at difference 1 (0x0FFF + 3 = 4098).
        let input = [0x01, 0x03, 0xB0, 0x02, 0x61, 0xFF, 0x0F];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED), Err(OvbaError::InvalidCopyToken { at: 5 }));
        // Length 4095 fills the chunk to exactly 4096.
        let input = [0x01, 0x03, 0xB0, 0x02, 0x61, 0xFC, 0x0F];
        assert_eq!(decompress(&input, MAX_DECOMPRESSED).unwrap(), vec![b'a'; 4096]);
    }

    #[test]
    fn output_cap_is_enforced() {
        let max = [0x01, 0x03, 0xB0, 0x02, 0x61, 0x45, 0x00];
        assert_eq!(decompress(&max, 72), Err(OvbaError::OutputLimit));
        assert_eq!(decompress(&max, 10), Err(OvbaError::OutputLimit));
        assert_eq!(decompress(&max, 0), Err(OvbaError::OutputLimit));
        assert_eq!(decompress(&max, 73).unwrap().len(), 73);

        let mut raw = vec![0x01];
        raw.extend(raw_chunk(&[0u8; 4096]));
        assert_eq!(decompress(&raw, 4095), Err(OvbaError::OutputLimit));
        assert_eq!(decompress(&raw, 4096).unwrap().len(), 4096);
    }

    #[test]
    fn error_messages_are_plain() {
        assert_eq!(OvbaError::InvalidChunk { at: 7 }.to_string(), "the compressed chunk at byte 7 is damaged or cut short");
        assert_eq!(OvbaError::InvalidCopyToken { at: 9 }.to_string(), "the copy token at byte 9 points outside its chunk");
    }

    /// Fixed-seed xorshift64 bytes: incompressible enough that a 4096-byte chunk goes raw.
    fn xorshift_bytes(seed: u64, len: usize) -> Vec<u8> {
        let mut state = seed;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    fn assert_round_trip(data: &[u8]) {
        let compressed = compress(data);
        assert_eq!(decompress(&compressed, MAX_DECOMPRESSED).unwrap(), data, "round trip of {} bytes", data.len());
    }

    /// Repeated English-like text with a counter, so it compresses but isn't one long run.
    fn text(len: usize) -> Vec<u8> {
        let mut out = Vec::new();
        let mut line = 0;
        while out.len() < len {
            out.extend_from_slice(format!("Sub Macro{line}()\r\n    Range(\"A{line}\").Value = {line}\r\nEnd Sub\r\n").as_bytes());
            line += 1;
        }
        out.truncate(len);
        out
    }

    /// The three [MS-OVBA] §3.2 examples, compressed: our encoder follows the spec's Matching
    /// search (nearest candidate wins ties), so it reproduces the spec's bytes exactly.
    #[test]
    fn compress_matches_spec_examples() {
        // §3.2.1 No Compression Example.
        let expected = [
            0x01, 0x19, 0xB0, 0x00, 0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x00, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70, 0x00, 0x71,
            0x72, 0x73, 0x74, 0x75, 0x76, 0x2E,
        ];
        assert_eq!(compress(b"abcdefghijklmnopqrstuv."), expected);
        // §3.2.2 Normal Compression Example.
        let expected = [
            0x01, 0x2F, 0xB0, 0x00, 0x23, 0x61, 0x61, 0x61, 0x62, 0x63, 0x64, 0x65, 0x82, 0x66, 0x00, 0x70, 0x61, 0x67, 0x68, 0x69, 0x6A, 0x01, 0x38,
            0x08, 0x61, 0x6B, 0x6C, 0x00, 0x20, 0x6D, 0x6E, 0x6F, 0x70, 0x06, 0x71, 0x02, 0x70, 0x04, 0x00, 0x72, 0x73, 0x74, 0x75, 0x76, 0x10, 0x77,
            0x78, 0x79, 0x7A, 0x00, 0x2C,
        ];
        assert_eq!(compress(b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa"), expected);
        // §3.2.3 Maximum Compression Example.
        assert_eq!(compress(&[b'a'; 73]), [0x01, 0x03, 0xB0, 0x02, 0x61, 0x45, 0x00]);
    }

    #[test]
    fn compress_empty_and_one_byte() {
        assert_eq!(compress(&[]), [0x01]);
        assert_round_trip(&[]);
        // Header 0xB001 (size 4 - 3), FlagByte 0, the literal.
        assert_eq!(compress(b"x"), [0x01, 0x01, 0xB0, 0x00, b'x']);
        assert_round_trip(b"x");
    }

    /// Inputs just under, at and just over one and two 4096-byte chunks, and a 20,000-byte input, compress and decompress back to themselves.
    #[test]
    fn compress_round_trips_around_the_chunk_size() {
        for len in [4095, 4096, 4097, 8192, 8193] {
            assert_round_trip(&text(len));
        }
        assert_round_trip(&text(20_000));
    }

    #[test]
    fn compress_round_trips_a_long_run() {
        let run = vec![0x41; 10_000];
        assert_round_trip(&run);
        // Three chunks, each well under 4096 bytes compressed.
        assert!(compress(&run).len() < 64);
    }

    #[test]
    fn compress_round_trips_random_bytes() {
        // Full chunks of random bytes go raw; the 1-byte tail is a compressed chunk.
        assert_round_trip(&xorshift_bytes(0x9E37_79B9_7F4A_7C15, 8193));
        // Short random input still fits as literals.
        assert_round_trip(&xorshift_bytes(42, 1000));
    }

    #[test]
    fn incompressible_chunk_is_stored_raw() {
        let data = xorshift_bytes(0x2545_F491_4F6C_DD1D, 4096);
        let compressed = compress(&data);
        // Signature, raw header 0x3FFF (size field 4095, signature 0b011, flag 0), the bytes.
        assert_eq!(compressed.len(), 1 + 2 + 4096);
        assert_eq!(compressed.get(..3), Some(&[0x01, 0xFF, 0x3F][..]));
        assert_eq!(compressed.get(3..), Some(&data[..]));
        assert_round_trip(&data);
    }

    /// §2.4.1.3.10: a short last chunk stored raw is padded to 4096 bytes with 0x00, so it
    /// decompresses to the input plus the padding.
    #[test]
    fn short_raw_chunk_is_padded() {
        let data = xorshift_bytes(7, 4000);
        let compressed = compress(&data);
        assert_eq!(compressed.len(), 1 + 2 + 4096);
        assert_eq!(compressed.get(..3), Some(&[0x01, 0xFF, 0x3F][..]));
        let mut expected = data.clone();
        expected.resize(4096, 0);
        assert_eq!(decompress(&compressed, MAX_DECOMPRESSED).unwrap(), expected);
        // 3,640 literals plus 455 FlagBytes still fit in 4096 bytes, so this one isn't padded.
        assert_round_trip(&xorshift_bytes(7, 3640));
    }
}
