//! Allocation preflight for the compact-Thrift envelopes. Apache owns all actual Parquet decoding.
//! Checks lengths before its metadata vectors/page buffers are allocated; no payload values are read here.
use crate::{Result, invalid};
use parquet::file::metadata::ParquetMetaData;

pub(crate) const MAX_DECODED: usize = 512 << 20;
const MAX_PAGE: usize = 64 << 20;
const MAX_FOOTER: usize = 16 << 20;
#[derive(Clone, Copy)]
enum Context {
    Footer,
    SchemaRoot,
    Schema,
    Page,
    Counts,
    Other,
}
struct Compact<'a> {
    bytes: &'a [u8],
    pos: usize,
    uncompressed: Option<usize>,
    compressed: Option<usize>,
}
impl<'a> Compact<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0, uncompressed: None, compressed: None }
    }
    fn byte(&mut self) -> Result<u8> {
        let b = self.bytes.get(self.pos).copied().ok_or_else(|| invalid("Truncated Parquet metadata"))?;
        self.pos += 1;
        Ok(b)
    }
    fn take(&mut self, n: usize) -> Result<()> {
        self.pos = self.pos.checked_add(n).filter(|end| *end <= self.bytes.len()).ok_or_else(|| invalid("Invalid Parquet metadata length"))?;
        Ok(())
    }
    fn varint(&mut self) -> Result<u64> {
        let mut n = 0u64;
        for shift in (0..70).step_by(7) {
            let b = self.byte()?;
            if shift == 63 && b > 1 {
                break;
            }
            n |= u64::from(b & 127) << shift;
            if b & 128 == 0 {
                return Ok(n);
            }
        }
        Err(invalid("Invalid Parquet metadata integer"))
    }
    fn integer(&mut self) -> Result<i64> {
        let n = self.varint()?;
        Ok((n >> 1) as i64 ^ -((n & 1) as i64))
    }
    fn length(&mut self) -> Result<usize> {
        usize::try_from(self.varint()?).map_err(|_| invalid("Parquet length overflow"))
    }
    fn structure(&mut self, depth: u8, context: Context) -> Result<()> {
        if depth > 32 {
            return Err(invalid("Parquet metadata nesting exceeds 32 levels"));
        }
        let mut id = 0i64;
        loop {
            let header = self.byte()?;
            if header == 0 {
                return Ok(());
            }
            let delta = header >> 4;
            id = if delta == 0 { self.integer()? } else { id.checked_add(i64::from(delta)).ok_or_else(|| invalid("Parquet field id overflow"))? };
            let kind = header & 15;
            if matches!(context, Context::Footer) && id == 2 && kind == 9 {
                let (n, element) = self.collection()?;
                if element != 12 || n == 0 || n > gridcraft_core::MAX_COLS as usize + 1 {
                    return Err(invalid("Invalid or oversized Parquet schema"));
                }
                for i in 0..n {
                    self.structure(depth + 1, if i == 0 { Context::SchemaRoot } else { Context::Schema })?;
                }
            } else if matches!(kind, 4..=6) {
                let value = self.integer()?;
                if matches!(context, Context::SchemaRoot) && id == 5 && !(1..=i64::from(gridcraft_core::MAX_COLS)).contains(&value) {
                    return Err(invalid("Invalid Parquet schema field count"));
                }
                if matches!(context, Context::Schema) && id == 5 && value != 0 {
                    return Err(invalid("Nested Parquet fields are not supported; flatten the table first"));
                }
                if matches!(context, Context::Page) && matches!(id, 2 | 3) {
                    let size = usize::try_from(value).map_err(|_| invalid("Negative Parquet page size"))?;
                    if size > MAX_PAGE {
                        return Err(invalid("Parquet page exceeds the 64 MiB decoded-page budget"));
                    }
                    if id == 2 {
                        self.uncompressed = Some(size);
                    } else {
                        self.compressed = Some(size);
                    }
                }
                if matches!(context, Context::Counts) {
                    if value < 0 || value > MAX_PAGE as i64 {
                        return Err(invalid("Invalid Parquet page count or level length"));
                    }
                    if id == 1 && value > i64::from(gridcraft_core::MAX_ROWS) {
                        return Err(invalid("Invalid Parquet page value count"));
                    }
                }
            } else if matches!(context, Context::Page) && kind == 12 && matches!(id, 5 | 7 | 8) {
                self.structure(depth + 1, Context::Counts)?;
            } else {
                self.skip(kind, depth + 1)?;
            }
        }
    }
    fn collection(&mut self) -> Result<(usize, u8)> {
        let header = self.byte()?;
        let n = if header >> 4 == 15 { self.length()? } else { usize::from(header >> 4) };
        if n > self.bytes.len().saturating_sub(self.pos) {
            return Err(invalid("Parquet collection length exceeds its metadata"));
        }
        Ok((n, header & 15))
    }
    fn skip(&mut self, kind: u8, depth: u8) -> Result<()> {
        if depth > 32 {
            return Err(invalid("Parquet metadata nesting exceeds 32 levels"));
        }
        match kind {
            1 | 2 => {}
            3 => self.take(1)?,
            4..=6 => {
                self.varint()?;
            }
            7 => self.take(8)?,
            8 => {
                let n = self.length()?;
                self.take(n)?;
            }
            9 | 10 => {
                let (n, element) = self.collection()?;
                for _ in 0..n {
                    if matches!(element, 1 | 2) {
                        self.take(1)?;
                    } else {
                        self.skip(element, depth + 1)?;
                    }
                }
            }
            11 => {
                let n = self.length()?;
                if n > self.bytes.len().saturating_sub(self.pos) / 2 {
                    return Err(invalid("Oversized Parquet metadata map"));
                }
                if n != 0 {
                    let types = self.byte()?;
                    for _ in 0..n {
                        for t in [types >> 4, types & 15] {
                            if matches!(t, 1 | 2) {
                                self.take(1)?;
                            } else {
                                self.skip(t, depth + 1)?;
                            }
                        }
                    }
                }
            }
            12 => self.structure(depth + 1, Context::Other)?,
            _ => return Err(invalid("Invalid compact-Thrift type in Parquet metadata")),
        }
        Ok(())
    }
}
pub(crate) fn footer(bytes: &[u8]) -> Result<usize> {
    if bytes.starts_with(b"PARE") {
        return Err(invalid("Encrypted Parquet files are not supported"));
    }
    if bytes.len() < 12 || !bytes.starts_with(b"PAR1") || !bytes.ends_with(b"PAR1") {
        return Err(invalid("Not a complete Parquet file (PAR1 header/footer required)"));
    }
    let tail = bytes.len() - 8;
    let length = bytes
        .get(tail..tail + 4)
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| invalid("Missing Parquet footer length"))? as usize;
    if length > MAX_FOOTER {
        return Err(invalid("Parquet footer exceeds the 16 MiB metadata budget"));
    }
    let start = tail.checked_sub(length).filter(|start| *start >= 4).ok_or_else(|| invalid("Invalid Parquet footer length"))?;
    let mut compact = Compact::new(bytes.get(start..tail).ok_or_else(|| invalid("Invalid Parquet footer"))?);
    compact.structure(0, Context::Footer)?;
    if compact.pos != length {
        return Err(invalid("Unexpected bytes after Parquet metadata"));
    }
    Ok(start)
}
pub(crate) fn pages(bytes: &[u8], metadata: &ParquetMetaData, footer_start: usize) -> Result<()> {
    let mut decoded = 0usize;
    let mut pages = 0usize;
    for group in metadata.row_groups() {
        if group.num_rows() < 0 || group.num_rows() > metadata.file_metadata().num_rows() {
            return Err(invalid("Invalid Parquet row-group row count"));
        }
        for column in group.columns() {
            if column.file_path().is_some() {
                return Err(invalid("External Parquet column files are not supported"));
            }
            if column.num_values() < 0 || column.num_values() > group.num_rows() {
                return Err(invalid("Invalid flat Parquet column value count"));
            }
            let start = usize::try_from(column.dictionary_page_offset().unwrap_or(column.data_page_offset()))
                .map_err(|_| invalid("Invalid Parquet column offset"))?;
            let length = usize::try_from(column.compressed_size()).map_err(|_| invalid("Invalid Parquet column length"))?;
            let end = start
                .checked_add(length)
                .filter(|end| *end <= footer_start && start >= 4)
                .ok_or_else(|| invalid("Parquet column lies outside the file data"))?;
            let mut cursor = start;
            while cursor < end {
                pages += 1;
                if pages > 1_000_000 {
                    return Err(invalid("Parquet file exceeds the 1,000,000-page budget"));
                }
                let mut header = Compact::new(bytes.get(cursor..end).ok_or_else(|| invalid("Truncated Parquet column"))?);
                header.structure(0, Context::Page)?;
                let size = header.compressed.ok_or_else(|| invalid("Missing Parquet compressed-page size"))?;
                let uncompressed = header.uncompressed.ok_or_else(|| invalid("Missing Parquet decoded-page size"))?;
                decoded = decoded
                    .checked_add(uncompressed)
                    .filter(|n| *n <= MAX_DECODED)
                    .ok_or_else(|| invalid("Parquet data exceeds the 512 MiB decoded-data budget; select a smaller table first"))?;
                cursor = cursor
                    .checked_add(header.pos)
                    .and_then(|n| n.checked_add(size))
                    .filter(|n| *n <= end)
                    .ok_or_else(|| invalid("Parquet page lies outside its column"))?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn oversized_lengths_and_deep_envelopes_fail_before_allocations() {
        // A list claiming more elements than its entire enclosing buffer.
        assert!(Compact::new(&[0x19, 0xfc, 0xff, 0xff, 0xff, 0x7f]).structure(0, Context::Other).is_err());
        let mut nested = vec![0x1c; 40];
        nested.extend([0; 41]);
        assert!(Compact::new(&nested).structure(0, Context::Other).is_err());
        // PageHeader field 2 (uncompressed_page_size), zigzag 128 MiB.
        let header = [0x25, 0x80, 0x80, 0x80, 0x80, 0x01, 0];
        assert!(Compact::new(&header).structure(0, Context::Page).unwrap_err().to_string().contains("64 MiB"));
        // Negative decoded-page size and a data-page value count of -1.
        assert!(Compact::new(&[0x25, 1, 0]).structure(0, Context::Page).is_err());
        assert!(Compact::new(&[0x15, 1, 0]).structure(0, Context::Counts).is_err());
    }
    #[test]
    fn impossible_footer_and_encryption_are_rejected() {
        assert!(footer(b"PAR1\xff\xff\xff\x7fPAR1").unwrap_err().to_string().contains("metadata budget"));
        assert!(footer(b"PARE12345678").unwrap_err().to_string().contains("Encrypted"));
        assert!(footer(b"PAR1\x05\0\0\0PAR1").is_err());
    }
}
