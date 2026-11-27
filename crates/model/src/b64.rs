//! Standard base64 (RFC 4648 §4): for serde (image bytes in JSON), and for every crate that
//! reads or writes it in a file (encryption descriptors, password hashes, `data:` URLs).

use serde::{Deserialize, Deserializer, Serializer};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET.get(((n >> shift) & 63) as usize).copied().unwrap_or(b'A')));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The 6-bit value of a base64 character (inverse of [`ALPHABET`]).
fn value(c: u8) -> Option<u32> {
    let v = match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    Some(u32::from(v))
}

/// Decodes base64, whitespace and padding ignored; `None` for any other character.
pub fn decode(s: &str) -> Option<Vec<u8>> {
    decode_limited(s, usize::MAX)
}

/// [`decode`], also `None` when the output would pass `limit` bytes (for input from a file).
pub fn decode_limited(s: &str, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity((s.len() / 4 * 3).min(limit).min(1 << 20));
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = value(c)?;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
            if out.len() > limit {
                return None;
            }
        }
    }
    Some(out)
}

pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&encode(v))
}

pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let s = String::deserialize(d)?;
    decode(&s).ok_or_else(|| serde::de::Error::custom("invalid base64"))
}

/// Base64 for byte blobs shared through `Arc` (floating pictures), so copies of a workbook, such as
/// its undo steps, share one payload.
pub mod shared {
    use std::sync::Arc;

    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Arc<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
        super::serialize(v, s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<Vec<u8>>, D::Error> {
        super::deserialize(d).map(Arc::new)
    }
}

/// Base64 for optional shared byte blobs (VBA projects): `null`/missing stays `None`.
pub mod opt {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(v: &Option<Arc<Vec<u8>>>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(data) => s.serialize_str(&super::encode(data)),
            None => s.serialize_none(),
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Arc<Vec<u8>>>, D::Error> {
        let s = Option::<String>::deserialize(d)?;
        match s {
            None => Ok(None),
            Some(s) => super::decode(&s).map(|v| Some(Arc::new(v))).ok_or_else(|| serde::de::Error::custom("invalid base64")),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decoding_is_bounded_and_strict() {
        assert_eq!(super::decode_limited("AAAA", 3).as_deref(), Some(&[0u8, 0, 0][..]));
        assert_eq!(super::decode_limited("AAAAAAAA", 3), None);
        assert_eq!(super::decode("AA!A"), None);
        assert_eq!(super::decode(" QU Jj\n").as_deref(), Some(&b"ABc"[..]));
        // The lookup is the alphabet's inverse, and nothing else decodes.
        for c in 0..=u8::MAX {
            assert_eq!(super::value(c), super::ALPHABET.iter().position(|a| *a == c).map(|i| i as u32), "{c}");
        }
    }

    #[test]
    fn roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        for data in [&b""[..], b"a", b"ab", b"abc", b"hello world", &[0, 255, 128, 7]] {
            assert_eq!(super::decode(&super::encode(data)).ok_or("none")?, data);
        }
        assert_eq!(super::encode(b"Man"), "TWFu");
        Ok(())
    }
}
