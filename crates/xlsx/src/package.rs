//! OPC package access through `craft-opc`: zip parts read against GridCraft's limits,
//! relationships with GridCraft's `Rel`, part-name resolution, and a deterministic zip writer.
//! Parts are parsed with this crate's XML reader.
//! Worksheet and shared-string XML are inflated from a second zip index and not retained:
//! `craft-opc` only hands back a buffered part, and that buffer stops at [`MAX_PART`].

use std::collections::HashMap;
use std::io::{BufRead, Cursor, Read};

use crate::IoError;
use crate::xml::{self, El};

pub use craft_opc::{normalize, resolve, split_dir};

/// Largest decompressed part accepted when the bytes are kept.
pub const MAX_PART: u64 = 512 * 1024 * 1024;
/// Largest total decompressed size of parts that are kept.
pub const MAX_TOTAL: u64 = 1024 * 1024 * 1024;
/// Largest decompressed worksheet or shared-string part. Those parts are parsed from the inflate
/// stream and not retained, so the cap is higher than [`MAX_PART`] without doubling peak memory.
pub const MAX_STREAM_PART: u64 = 2 * 1024 * 1024 * 1024;
/// Largest total decompressed size of streamed parts in one package.
pub const MAX_STREAM_TOTAL: u64 = 4 * 1024 * 1024 * 1024;
/// Most entries accepted in the zip directory.
pub const MAX_ENTRIES: usize = 200_000;

const LIMITS: craft_opc::Limits = craft_opc::Limits::new(MAX_ENTRIES, MAX_PART, MAX_TOTAL);

pub struct Package<'a> {
    opc: craft_opc::LazyPackage<'a>,
    /// Worksheet and shared-string XML. Inflated here and not retained.
    stream: zip::ZipArchive<Cursor<&'a [u8]>>,
    /// Lower-case normalized part name → index in `stream`.
    stream_names: HashMap<String, usize>,
    /// Decompressed bytes of parts that were streamed and discarded.
    streamed: u64,
}

/// A relationship.
#[derive(Clone, Debug)]
pub struct Rel {
    pub id: String,
    /// Last path segment of the relationship type (`worksheet`, `styles`, …): the same for the
    /// transitional and strict namespaces.
    pub kind: String,
    /// Resolved part name (no leading `/`), or the raw target for external relationships.
    pub target: String,
    pub external: bool,
}

/// GridCraft's wording of a package error.
fn error(e: craft_opc::Error) -> IoError {
    use craft_opc::Error as E;
    match e {
        E::Zip(m) | E::Write(m) => IoError::Zip(m),
        E::TooManyEntries { declared, .. } => IoError::TooLarge(format!("{declared} zip entries")),
        E::PartTooLarge { part, limit } => IoError::TooLarge(format!("{part} expands to more than {limit} bytes")),
        E::PackageTooLarge { .. } => IoError::TooLarge("package expands to more than 1 GB".into()),
        E::Xml { part, error } => IoError::Xml(format!("{part}: {error}")),
        e if e.is_limit() => IoError::TooLarge(e.to_string()),
        e => IoError::Zip(e.to_string()),
    }
}

impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Package<'a>, IoError> {
        let opc = craft_opc::LazyPackage::open(bytes, &LIMITS).map_err(error)?;
        let stream = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| IoError::Zip(e.to_string()))?;
        if stream.len() > MAX_ENTRIES {
            return Err(IoError::TooLarge(format!("{} zip entries", stream.len())));
        }
        let mut stream_names = HashMap::new();
        for i in 0..stream.len() {
            if let Some(n) = stream.name_for_index(i) {
                stream_names.entry(normalize(n)).or_insert(i);
            }
        }
        Ok(Package { opc, stream, stream_names, streamed: 0 })
    }

    /// Every part's (normalized) name, sorted.
    #[cfg(test)]
    pub fn names(&self) -> Vec<String> {
        self.opc.names().into_iter().map(str::to_string).collect()
    }

    pub fn has(&self, part: &str) -> bool {
        self.opc.has(part)
    }

    /// Reads a part's bytes; `Ok(None)` when it doesn't exist.
    pub fn read(&mut self, part: &str) -> Result<Option<Vec<u8>>, IoError> {
        self.opc.read(part).map_err(error)
    }

    /// Reads a part the reader can do without (the caller warns and goes on when it fails). A read
    /// that fails partway is charged this part's whole allowance instead of ending the package's.
    pub fn read_optional_limited(&mut self, part: &str, maximum: u64) -> Result<Option<Vec<u8>>, IoError> {
        self.opc.read_optional_limited(part, maximum).map_err(error)
    }

    /// Reads a small metadata part without allowing it to expand to the general part limit.
    pub fn read_limited(&mut self, part: &str, maximum: u64) -> Result<Option<Vec<u8>>, IoError> {
        self.opc.read_limited(part, maximum).map_err(error)
    }

    pub fn read_xml(&mut self, part: &str) -> Result<Option<El>, IoError> {
        match self.read(part)? {
            Some(b) => xml::parse(&b).map(Some).map_err(|e| prefix(part, e)),
            None => Ok(None),
        }
    }

    /// Parses a part from the inflate stream. Repeated elements named in `split` are handed to
    /// `cb` and not kept. The decompressed bytes are not retained. `Ok(None)` when the part is
    /// missing. UTF-16 parts are buffered, and still refused past [`MAX_PART`].
    pub fn parse_xml_streaming(&mut self, part: &str, split: &[&str], cb: &mut dyn FnMut(El) -> Result<(), IoError>) -> Result<Option<El>, IoError> {
        let Some(&idx) = self.stream_names.get(&normalize(part)) else {
            return Ok(None);
        };
        let budget = MAX_STREAM_TOTAL.saturating_sub(self.streamed);
        let limit = MAX_STREAM_PART.min(budget);
        let (n, result) = {
            let mut file = self.stream.by_index(idx).map_err(|e| IoError::Zip(format!("{part}: {e}")))?;
            let declared = file.size();
            if declared > limit {
                return Err(IoError::TooLarge(if budget < MAX_STREAM_PART {
                    "package expands past the streamed decompression limit".into()
                } else {
                    format!("{part} expands to {declared} bytes")
                }));
            }
            let mut buffered = std::io::BufReader::with_capacity(64 * 1024, &mut file);
            let head = buffered.fill_buf().map_err(|e| IoError::Zip(format!("{part}: {e}")))?;
            let utf16 = is_utf16_xml(head);
            if utf16 && (declared > MAX_PART || limit.min(MAX_PART) == 0) {
                return Err(IoError::TooLarge(format!("{part} expands past {MAX_PART} bytes")));
            }
            let cap = if utf16 { limit.min(MAX_PART) } else { limit };
            let mut limited = LimitRead { inner: buffered, left: cap, read: 0 };
            let result = if utf16 {
                let mut buf = Vec::new();
                // Count the bytes even when inflate fails: `?` would return before `streamed`
                // is updated, and a later sheet would not see this part in the stream budget.
                match limited.read_to_end(&mut buf) {
                    Ok(_) => {
                        let utf8 = xml::to_utf8(&buf).into_owned();
                        drop(buf);
                        xml::parse_bufread(Cursor::new(utf8), split, cb)
                    }
                    Err(e) => Err(map_stream_io(part, e)),
                }
            } else {
                xml::parse_bufread(&mut limited, split, cb)
            };
            (limited.read, result)
        };
        self.streamed = self.streamed.saturating_add(n);
        result.map(Some).map_err(|e| prefix(part, e))
    }

    /// Relationships of a part (`xl/workbook.xml` → `xl/_rels/workbook.xml.rels`). Missing or
    /// broken rels parts give an empty list.
    pub fn rels(&mut self, part: &str) -> Result<Vec<Rel>, IoError> {
        let rels = self.opc.rels(part).map_err(error)?;
        Ok(rels
            .list
            .into_iter()
            .map(|r| {
                let kind = r.kind().to_string();
                Rel { id: r.id, kind, target: r.target, external: r.external }
            })
            .collect())
    }
}

/// The first part whose declared unpacked size is over [`MAX_PART`], with that size.
pub fn oversized_part(bytes: &[u8]) -> Option<(String, u64)> {
    craft_opc::declared_oversized(bytes, &LIMITS)
}

fn is_utf16_xml(head: &[u8]) -> bool {
    head.starts_with(&[0xFF, 0xFE])
        || head.starts_with(&[0xFE, 0xFF])
        || (head.len() >= 4 && head[1] == 0 && head[3] == 0 && head[0] != 0)
        || (head.len() >= 4 && head[0] == 0 && head[2] == 0 && head[1] != 0)
}

fn map_stream_io(part: &str, e: std::io::Error) -> IoError {
    let s = e.to_string();
    if s.contains(xml::PART_LIMIT_MARK) {
        IoError::TooLarge(format!("{part} expands past the decompression limit"))
    } else {
        IoError::Zip(format!("{part}: {s}"))
    }
}

/// Stops with an error once `left` bytes have been consumed and the inner reader still has data,
/// so a part that merely ends at the cap is accepted and one that runs past it is refused.
struct LimitRead<R> {
    inner: R,
    left: u64,
    read: u64,
}

impl<R: BufRead> Read for LimitRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = {
            let avail = self.fill_buf()?;
            if avail.is_empty() {
                return Ok(0);
            }
            let n = avail.len().min(buf.len());
            buf[..n].copy_from_slice(&avail[..n]);
            n
        };
        self.consume(n);
        Ok(n)
    }
}

impl<R: BufRead> BufRead for LimitRead<R> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        let buf = self.inner.fill_buf()?;
        if buf.is_empty() {
            return Ok(buf);
        }
        if self.left == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, xml::PART_LIMIT_MARK));
        }
        let n = buf.len().min(self.left as usize);
        Ok(&buf[..n])
    }
    fn consume(&mut self, amt: usize) {
        let amt = amt.min(self.left as usize);
        self.inner.consume(amt);
        self.left -= amt as u64;
        self.read += amt as u64;
    }
}

fn prefix(part: &str, e: IoError) -> IoError {
    match e {
        IoError::Xml(m) => IoError::Xml(format!("{part}: {m}")),
        o => o,
    }
}

// ---------------------------------------------------------------- writing

/// A zip package being written: every entry deflated, with one fixed timestamp.
pub struct ZipOut {
    zip: craft_opc::ZipOut,
}

impl ZipOut {
    pub fn new() -> ZipOut {
        ZipOut { zip: craft_opc::ZipOut::new() }
    }
    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<(), IoError> {
        self.zip.add(name, data, craft_opc::Compression::Deflate).map_err(error)
    }
    pub fn finish(self) -> Result<Vec<u8>, IoError> {
        self.zip.finish().map_err(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolving() {
        assert_eq!(resolve("xl", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve("xl/worksheets", "../drawings/d1.xml"), "xl/drawings/d1.xml");
        assert_eq!(resolve("xl/worksheets", "/xl/tables/t.xml"), "xl/tables/t.xml");
        assert_eq!(resolve("", "xl/workbook.xml"), "xl/workbook.xml");
        assert_eq!(resolve("a", "../../../x"), "x");
        assert_eq!(split_dir("xl/workbook.xml"), ("xl", "workbook.xml"));
        assert_eq!(split_dir("x.xml"), ("", "x.xml"));
    }
    /// A part that ends on the cap is kept; one that runs past it is refused.
    #[test]
    fn limited_read_stops_past_the_cap() {
        let mut lim = LimitRead { inner: std::io::Cursor::new([1, 2, 3, 4, 5]), left: 3, read: 0 };
        let mut buf = Vec::new();
        let err = lim.read_to_end(&mut buf).unwrap_err();
        assert!(err.to_string().contains(xml::PART_LIMIT_MARK));
        assert_eq!(buf, vec![1, 2, 3]);
        let mut exact = LimitRead { inner: std::io::Cursor::new([1, 2, 3]), left: 3, read: 0 };
        let mut buf = Vec::new();
        exact.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, vec![1, 2, 3]);
    }
}
