//! Cell comments (notes) in `.xls` files: the Note record names the cell, its author and the
//! drawing object that holds the text ([MS-XLS] 2.4.179 Note); the Obj record before it says
//! the object is a comment ([MS-XLS] 2.4.181 Obj, 2.5.143 FtCmo, `ot` 0x19); the TxO record
//! that follows holds the text in its Continue records ([MS-XLS] 2.4.329 TxO).

/// Characters in one comment at most.
pub(super) const MAX_NOTE_CHARS: usize = 32_767;

/// FtCmo's object type for a comment.
pub(super) const OT_NOTE: u16 = 0x19;

/// The text of a TxO record: `cch` characters in the Continue records after its fixed part,
/// each starting with its own compression flag (0: one byte a character, 1: UTF-16). The
/// formatting runs after the text are ignored.
pub(super) fn txo_text(parts: &[&[u8]], cch: usize) -> String {
    let cch = cch.min(MAX_NOTE_CHARS);
    let mut units: Vec<u16> = Vec::with_capacity(cch);
    for part in parts.iter().skip(1) {
        if units.len() >= cch {
            break;
        }
        let Some((&flag, rest)) = part.split_first() else { continue };
        if flag & 1 != 0 {
            for pair in rest.as_chunks::<2>().0 {
                if units.len() >= cch {
                    break;
                }
                units.push(u16::from_le_bytes(*pair));
            }
        } else {
            for b in rest {
                if units.len() >= cch {
                    break;
                }
                units.push(u16::from(*b));
            }
        }
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn txo_text_reads_compressed_and_wide_parts_and_stops_at_its_length() {
        let fixed: &[u8] = &[0; 18];
        assert_eq!(txo_text(&[fixed, &[0, b'H', b'i']], 2), "Hi");
        let wide: Vec<u8> = [1u8].into_iter().chain("Đỗ".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(txo_text(&[fixed, &wide], 2), "Đỗ");
        // A text over two Continue records, then the formatting runs (not text).
        assert_eq!(txo_text(&[fixed, &[0, b'a', b'b'], &[0, b'c'], &[0, 0, 0, 0]], 3), "abc");
        assert_eq!(txo_text(&[fixed], 5), "", "no text record");
        assert_eq!(txo_text(&[fixed, &[1, 0x41]], 1), "", "half a UTF-16 unit is dropped");
    }
}
