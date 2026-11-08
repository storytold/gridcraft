//! BIFF8 record framing ([MS-XLS] 2.1.4) and the string structures ([MS-XLS] 2.5.240,
//! 2.5.293, 2.5.294) that may continue across `Continue` records.

use super::invalid;
use crate::IoError;

/// `Continue` record type.
pub(super) const CONTINUE: u16 = 0x003C;
/// Characters in one string at most (Excel's cell limit is 32,767; the SST may hold longer).
const MAX_CHARS: usize = 1 << 20;
/// `Continue` records gathered for one record at most.
const MAX_PARTS: usize = 1 << 16;

/// One record and the payloads of the `Continue` records that follow it.
pub(super) struct Record<'a> {
    pub id: u16,
    pub parts: Vec<&'a [u8]>,
}

/// Iterates over the records of a stream starting at `pos`.
pub(super) struct Records<'a> {
    stream: &'a [u8],
    pos: usize,
}

impl<'a> Records<'a> {
    pub fn new(stream: &'a [u8], pos: usize) -> Self {
        Records { stream, pos }
    }

    fn raw(&mut self) -> Result<Option<(u16, &'a [u8])>, IoError> {
        if self.pos >= self.stream.len() {
            return Ok(None);
        }
        let head = self.stream.get(self.pos..self.pos.saturating_add(4)).ok_or_else(|| invalid("truncated record header"))?;
        let id = u16::from_le_bytes([head.first().copied().unwrap_or(0), head.get(1).copied().unwrap_or(0)]);
        let len = usize::from(u16::from_le_bytes([head.get(2).copied().unwrap_or(0), head.get(3).copied().unwrap_or(0)]));
        let start = self.pos + 4;
        let data = self.stream.get(start..start.saturating_add(len)).ok_or_else(|| invalid("truncated record"))?;
        self.pos = start + len;
        Ok(Some((id, data)))
    }

    fn peek_id(&self) -> Option<u16> {
        let h = self.stream.get(self.pos..self.pos.checked_add(2)?)?;
        Some(u16::from_le_bytes([*h.first()?, *h.get(1)?]))
    }

    /// The next record with its `Continue` records, or `None` at the end of the stream.
    pub fn next_record(&mut self) -> Result<Option<Record<'a>>, IoError> {
        let Some((id, data)) = self.raw()? else { return Ok(None) };
        let mut parts = vec![data];
        while self.peek_id() == Some(CONTINUE) && parts.len() < MAX_PARTS {
            if let Some((_, more)) = self.raw()? {
                parts.push(more);
            }
        }
        Ok(Some(Record { id, parts }))
    }
}

/// Reads a record's payload, crossing into its `Continue` records when needed.
pub(super) struct Cursor<'a, 'b> {
    parts: &'b [&'a [u8]],
    part: usize,
    pos: usize,
    /// Bytes in every part after the current one, kept up to date as parts advance, so
    /// `remaining()` is constant-time however many `Continue` records a string session crosses.
    after: usize,
}

impl<'a, 'b> Cursor<'a, 'b> {
    pub fn new(parts: &'b [&'a [u8]]) -> Self {
        let after = parts.iter().skip(1).map(|p| p.len()).sum::<usize>();
        Cursor { parts, part: 0, pos: 0, after }
    }

    /// Skips to the next part when the current one is used up. Returns whether a new part began.
    fn settle(&mut self) -> bool {
        let mut moved = false;
        while self.parts.get(self.part).is_some_and(|p| self.pos >= p.len()) && self.part + 1 < self.parts.len() {
            self.after = self.after.saturating_sub(self.parts[self.part + 1].len());
            self.part += 1;
            self.pos = 0;
            moved = true;
        }
        moved
    }

    pub fn u8(&mut self) -> Result<u8, IoError> {
        self.settle();
        let b = self.parts.get(self.part).and_then(|p| p.get(self.pos)).copied().ok_or_else(|| invalid("record is shorter than its contents"))?;
        self.pos += 1;
        Ok(b)
    }

    pub fn u16(&mut self) -> Result<u16, IoError> {
        Ok(u16::from_le_bytes([self.u8()?, self.u8()?]))
    }

    pub fn u32(&mut self) -> Result<u32, IoError> {
        Ok(u32::from_le_bytes([self.u8()?, self.u8()?, self.u8()?, self.u8()?]))
    }

    pub fn f64(&mut self) -> Result<f64, IoError> {
        let mut b = [0u8; 8];
        for x in b.iter_mut() {
            *x = self.u8()?;
        }
        Ok(f64::from_le_bytes(b))
    }

    pub fn skip(&mut self, n: usize) -> Result<(), IoError> {
        for _ in 0..n {
            self.u8()?;
        }
        Ok(())
    }

    /// Bytes left in the record (all parts), constant-time in the part count.
    pub fn remaining(&self) -> usize {
        let here = self.parts.get(self.part).map_or(0, |p| p.len().saturating_sub(self.pos));
        here.saturating_add(self.after)
    }

    /// The rest of the record as one buffer.
    pub fn rest(&mut self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.remaining());
        if let Some(p) = self.parts.get(self.part) {
            out.extend_from_slice(p.get(self.pos..).unwrap_or(&[]));
        }
        for p in self.parts.iter().skip(self.part + 1) {
            out.extend_from_slice(p);
        }
        self.part = self.parts.len();
        self.pos = 0;
        self.after = 0;
        out
    }

    /// `cch` characters, compressed (one byte each) or UTF-16. When the characters run into
    /// a `Continue` record, that record starts with a fresh compression flag ([MS-XLS] 2.5.293).
    pub fn chars(&mut self, cch: usize, mut high: bool) -> Result<String, IoError> {
        if cch > MAX_CHARS {
            return Err(invalid("string too long"));
        }
        let mut units: Vec<u16> = Vec::with_capacity(cch.min(self.remaining()));
        while units.len() < cch {
            let at_end = self.parts.get(self.part).is_none_or(|p| self.pos >= p.len());
            if at_end && self.settle() {
                high = self.u8()? & 1 != 0;
            }
            units.push(if high { self.u16()? } else { u16::from(self.u8()?) });
        }
        Ok(String::from_utf16_lossy(&units))
    }

    /// ShortXLUnicodeString: 1-byte count, flags, characters.
    pub fn short_string(&mut self) -> Result<String, IoError> {
        let cch = usize::from(self.u8()?);
        let high = self.u8()? & 1 != 0;
        self.chars(cch, high)
    }

    /// XLUnicodeString: 2-byte count, flags, characters.
    pub fn string(&mut self) -> Result<String, IoError> {
        let cch = usize::from(self.u16()?);
        let high = self.u8()? & 1 != 0;
        self.chars(cch, high)
    }

    /// XLUnicodeRichExtendedString: the text; formatting runs and phonetic data are skipped.
    pub fn rich_string(&mut self) -> Result<String, IoError> {
        let cch = usize::from(self.u16()?);
        let flags = self.u8()?;
        let runs = if flags & 0x08 != 0 { usize::from(self.u16()?) } else { 0 };
        let ext = if flags & 0x04 != 0 { self.u32()? as usize } else { 0 };
        let text = self.chars(cch, flags & 1 != 0)?;
        let extra = runs.saturating_mul(4).saturating_add(ext);
        if extra > self.remaining() {
            return Err(invalid("string formatting runs past the end of the record"));
        }
        self.skip(extra)?;
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_continue_with_a_new_compression_flag() {
        // "ab" compressed, then "ç" and "d" as UTF-16 in a Continue part.
        let first: &[u8] = &[4, 0, 0, b'a', b'b'];
        let second: &[u8] = &[1, 0xE7, 0, b'd', 0];
        let parts = [first, second];
        let mut c = Cursor::new(&parts);
        assert_eq!(c.rich_string().unwrap(), "abçd");
        assert_eq!(c.remaining(), 0);
        let mut c = Cursor::new(&parts);
        assert!(c.chars(MAX_CHARS + 1, false).is_err());
    }

    #[test]
    fn remaining_counts_every_part_wherever_the_cursor_is() {
        let parts: [&[u8]; 4] = [&[1, 2, 3], &[2, 3, 4, 5], &[3, 4, 5, 6, 7], &[4]];
        let mut c = Cursor::new(&parts);
        assert_eq!(c.remaining(), 13);
        for b in 1..=3u64 {
            assert_eq!(u64::from(c.u8().unwrap()), b, "part 0 in order");
        }
        assert_eq!(c.remaining(), 10, "after the first part is used up");
        for _ in 0..4 {
            c.u8().unwrap();
        }
        assert_eq!(c.remaining(), 6, "into the Continue parts");
        let mut c = Cursor::new(&parts);
        assert_eq!(u64::from(c.u8().unwrap()), 1);
        assert_eq!(c.rest().len(), 12);
        assert_eq!(c.remaining(), 0);
    }
}
