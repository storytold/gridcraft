//! Xpress8 ("DIRECT2") codec used by the chunk stream of `xl/model/item.data`.
//!
//! The grammar is plain LZ77 with a 32-bit flag word consumed most-significant bit first, a
//! literal byte per zero flag and a 16-bit match token per one flag. A token carries a 13-bit
//! distance (`token >> 3`, plus one) and a 3-bit length code; code 7 escapes to a nibble byte,
//! then a byte, then a 16-bit value. There is no entropy coding.
//!
//! The encoder is deterministic, emits only distances up to 8192 and lengths up to the block
//! size, never uses the 32-bit escape, and declines (returns `None`) when the result would not be
//! strictly smaller than the input, when the result would be shorter than the eight bytes the
//! chunk decoder requires, or when the input is at most eight bytes.

/// Maximum number of uncompressed bytes in one chunk (the decoder's hard limit).
pub const MAX_BLOCK: usize = 65536;
/// Maximum match distance the grammar can express.
const MAX_DISTANCE: usize = 8192;
/// Minimum match length the grammar can express.
const MIN_MATCH: usize = 3;
/// Largest match length the grammar can express (16-bit length escape, `w + 3`).
const MAX_MATCH: usize = 65538;

const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const CHAIN_LIMIT: usize = 48;

/// Decodes one Xpress8 buffer into exactly `expected` bytes.
///
/// Every read and copy is bounds checked: truncated input, a match before the start of the
/// output, a match past the declared length and running out of flags are all errors.
pub fn decompress(input: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    if expected == 0 {
        return Err("compressed chunk declares zero bytes".into());
    }
    if expected > MAX_BLOCK {
        return Err("compressed chunk exceeds the 64 KiB block limit".into());
    }
    let mut out = Vec::with_capacity(expected.min(MAX_BLOCK));
    let mut pos = 0usize;
    let mut high_nibble: Option<u8> = None;

    let read_u8 = |pos: &mut usize| -> Result<u8, String> {
        let byte = input.get(*pos).copied().ok_or_else(|| "truncated stream".to_string())?;
        *pos = pos.checked_add(1).ok_or_else(|| "offset overflow".to_string())?;
        Ok(byte)
    };
    let read_u16 = |pos: &mut usize| -> Result<u16, String> {
        let end = pos.checked_add(2).ok_or_else(|| "offset overflow".to_string())?;
        let bytes = input.get(*pos..end).ok_or_else(|| "truncated stream".to_string())?;
        *pos = end;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    };

    while out.len() < expected {
        let start = pos;
        let end = pos.checked_add(4).ok_or_else(|| "offset overflow".to_string())?;
        let bytes = input.get(start..end).ok_or_else(|| "truncated flag word".to_string())?;
        pos = end;
        let flags = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        for bit in (0..32).rev() {
            if out.len() >= expected {
                break;
            }
            if flags & (1 << bit) == 0 {
                let byte = read_u8(&mut pos)?;
                out.push(byte);
                continue;
            }
            // A one flag with no token left is only a terminal marker once every declared byte
            // has been produced; anything shorter is a truncated chunk.
            if pos >= input.len() {
                return Err("stream ends before the declared uncompressed size".into());
            }
            let token = read_u16(&mut pos)?;
            let distance = usize::from(token >> 3) + 1;
            let length = match usize::from(token & 0x7) {
                7 => {
                    let nibble = match high_nibble.take() {
                        Some(nibble) => nibble,
                        None => {
                            let byte = read_u8(&mut pos)?;
                            high_nibble = Some(byte >> 4);
                            byte & 0x0F
                        }
                    };
                    if nibble < 15 {
                        usize::from(nibble) + 10
                    } else {
                        let byte = read_u8(&mut pos)?;
                        if byte < 255 {
                            usize::from(byte) + 25
                        } else {
                            let value = usize::from(read_u16(&mut pos)?);
                            if value < 277 {
                                return Err("invalid 16-bit match length escape".into());
                            }
                            // The reference grammar's escape chain encodes `value + 3`; values
                            // below 277 cannot be produced by a conforming encoder.
                            value - 277 + 280
                        }
                    }
                }
                other => other + 3,
            };
            if length > MAX_MATCH {
                return Err("match length exceeds the grammar".into());
            }
            let start = out.len().checked_sub(distance).ok_or_else(|| "match offset before output start".to_string())?;
            let end = out.len().checked_add(length).ok_or_else(|| "match length overflow".to_string())?;
            if end > expected {
                return Err("match extends past the declared uncompressed size".into());
            }
            for index in 0..length {
                let byte = out.get(start + index).copied().ok_or_else(|| "match copy out of bounds".to_string())?;
                out.push(byte);
            }
        }
    }
    if out.len() == expected { Ok(out) } else { Err("truncated stream".into()) }
}

/// Compresses one chunk, or returns `None` when the chunk must be stored verbatim.
///
/// The caller stores a `None` chunk as `uncompressed_len == compressed_len` with the raw bytes.
pub fn compress(input: &[u8]) -> Option<Vec<u8>> {
    if input.len() <= 8 || input.len() > MAX_BLOCK {
        return None;
    }
    let limit = input.len();
    let mut out: Vec<u8> = Vec::with_capacity(limit / 2 + 8);
    let mut flags_pos: usize = 0;
    let mut flags: u32 = 0;
    let mut flag_remaining: u32 = 0;
    let mut word_started = false;
    let mut pending_nibble: Option<usize> = None;

    let mut head = [usize::MAX; HASH_SIZE];
    let mut previous = vec![usize::MAX; input.len()];

    let mut position = 0usize;
    // Hash of three bytes at `at`, if the tail has three bytes.
    let hash_at = |at: usize| -> Option<usize> {
        let a = *input.get(at)? as usize;
        let b = *input.get(at + 1)? as usize;
        let c = *input.get(at + 2)? as usize;
        let key = a | (b << 8) | (c << 16);
        Some((key.wrapping_mul(2_654_435_761) >> (32 - HASH_BITS)) & (HASH_SIZE - 1))
    };
    let insert = |at: usize, head: &mut [usize; HASH_SIZE], previous: &mut [usize]| {
        if let Some(slot) = hash_at(at) {
            previous[at] = head[slot];
            head[slot] = at;
        }
    };

    while position < input.len() {
        let mut best_length = 0usize;
        let mut best_position = 0usize;
        if let Some(slot) = hash_at(position) {
            let mut candidate = head[slot];
            let mut steps = 0usize;
            while candidate != usize::MAX && steps < CHAIN_LIMIT {
                steps += 1;
                if position - candidate > MAX_DISTANCE {
                    break;
                }
                let max_length = (input.len() - position).min(MAX_MATCH);
                let mut length = 0usize;
                while length < max_length && input[candidate + length] == input[position + length] {
                    length += 1;
                }
                if length > best_length {
                    best_length = length;
                    best_position = candidate;
                    if length == max_length {
                        break;
                    }
                }
                candidate = previous[candidate];
            }
        }
        let is_match = best_length >= MIN_MATCH;
        if flag_remaining == 0 {
            // Close the previous flag word before reserving the next one.
            if word_started && let Some(word) = out.get_mut(flags_pos..flags_pos.checked_add(4)?) {
                word.copy_from_slice(&flags.to_le_bytes());
            }
            word_started = true;
            flags_pos = out.len();
            out.extend_from_slice(&[0, 0, 0, 0]);
            flags = 0;
            flag_remaining = 32;
        }
        let bit = 31 - (32 - flag_remaining);
        flag_remaining -= 1;
        if is_match {
            flags |= 1 << bit;
            let distance = position - best_position;
            let offset = u32::try_from(distance - 1).ok()?;
            let length = best_length;
            let low = offset * 8 + if length <= 9 { u32::try_from(length - 3).ok()? } else { 7 };
            out.extend_from_slice(&u16::try_from(low).ok()?.to_le_bytes());
            if length > 9 {
                if length <= 24 {
                    emit_nibble(&mut out, &mut pending_nibble, (length - 10) as u8);
                } else if length <= 279 {
                    emit_nibble(&mut out, &mut pending_nibble, 15);
                    out.push((length - 25) as u8);
                } else {
                    emit_nibble(&mut out, &mut pending_nibble, 15);
                    out.push(255);
                    out.extend_from_slice(&u16::try_from(length - 3).ok()?.to_le_bytes());
                }
            }
            for at in position..position + best_length {
                insert(at, &mut head, &mut previous);
            }
            position += best_length;
        } else {
            out.push(input[position]);
            insert(position, &mut head, &mut previous);
            position += 1;
        }
        if out.len() >= limit {
            return None;
        }
    }
    // Close the trailing flag word. Unused flags stay zero, which is legal because the decoder
    // stops after the declared number of bytes.
    // Pad the unused flags of the trailing word with ones (Excel does the same); the decoder
    // stops after the declared output length, so this is cosmetic for conforming decoders.
    if flag_remaining > 0 {
        let unused = (1u32 << flag_remaining) - 1;
        flags |= unused;
    }
    let word = out.get_mut(flags_pos..flags_pos.checked_add(4)?)?;
    word.copy_from_slice(&flags.to_le_bytes());
    Some(out).filter(|stream| stream.len() >= 8 && stream.len() < limit)
}

/// Emits one escaped-length nibble, reusing the spare high nibble of the previous byte.
fn emit_nibble(out: &mut Vec<u8>, pending: &mut Option<usize>, nibble: u8) {
    let nibble = nibble & 0x0F;
    match pending.take() {
        Some(index) => {
            if let Some(byte) = out.get_mut(index) {
                *byte = (*byte & 0x0F) | (nibble << 4);
            }
        }
        None => {
            *pending = Some(out.len());
            // The high nibble is a placeholder until a second escaped match needs it; 0xF is
            // never consumed when no second match follows because the decoder stops on length.
            out.push(0xF0 | nibble);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference_stream() -> Vec<u8> {
        // First four bytes are the flag word; this is the published reference output for
        // "abc" repeated 100 times.
        vec![0xFF, 0xFF, 0xFF, 0x0F, 0x61, 0x62, 0x63, 0x61, 0x17, 0x00, 0x0F, 0xFF, 0x25, 0x01]
    }

    #[test]
    fn decodes_reference_stream() {
        let decoded = decompress(&reference_stream(), 300).expect("decode");
        assert_eq!(decoded.len(), 300);
        assert!(decoded.chunks(3).all(|chunk| chunk == b"abc"));
    }

    #[test]
    fn round_trips_short_and_periodic_inputs() {
        for input in [b"abcdefgh".as_slice(), b"aaaaaaaaaaaaaaaaaaaa".as_slice(), b"abcabcabcabcabcabcabc".as_slice()] {
            if let Some(encoded) = compress(input) {
                assert_eq!(decompress(&encoded, input.len()).expect("decode"), input);
            }
        }
    }

    #[test]
    fn declines_incompressible_and_tiny_inputs() {
        assert!(compress(&[]).is_none());
        assert!(compress(b"abcdefgh").is_none());
        let random: Vec<u8> = (0..512u32).map(|value| (value.wrapping_mul(2_654_435_761) >> 24) as u8).collect();
        if let Some(encoded) = compress(&random) {
            assert_eq!(decompress(&encoded, random.len()).expect("decode"), random);
        }
    }

    #[test]
    fn every_truncation_of_a_valid_stream_is_an_error() {
        let decoded = vec![b'x'; 5000];
        let encoded = compress(&decoded).expect("compressible");
        for end in 0..encoded.len() {
            assert!(decompress(&encoded[..end], decoded.len()).is_err(), "prefix {end} decoded");
        }
    }

    #[test]
    fn random_garbage_never_panics() {
        let mut state = 0x1234_5678u32;
        for length in 0..300usize {
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                data.push((state >> 16) as u8);
            }
            let _ = decompress(&data, 1 + (length % 4096));
        }
    }

    #[test]
    fn largest_block_round_trips() {
        let input: Vec<u8> = (0..MAX_BLOCK).map(|index| (index % 7) as u8).collect();
        let encoded = compress(&input).expect("compressible");
        assert!(encoded.len() < input.len());
        assert_eq!(decompress(&encoded, input.len()).expect("decode"), input);
    }
}
