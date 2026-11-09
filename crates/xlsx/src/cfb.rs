//! Just enough of the Compound File Binary (CFB/OLE2) container to tell what a non-zip
//! spreadsheet file is: Excel wraps password-protected workbooks in a CFB holding an
//! `EncryptedPackage` stream, while Excel 97-2003 `.xls` (and other legacy Office files) are
//! CFB files without one. Every read is bounds-checked and every loop bounded.

/// The CFB signature at the start of the file.
pub(crate) const SIGNATURE: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
/// Sector ids at or above this are markers (free, end of chain, FAT/DIFAT sector), not sectors.
const MAX_REG_SECT: u32 = 0xFFFF_FFFA;
/// Directory sectors followed at most (4096 entries with 512-byte sectors).
const MAX_DIR_SECTORS: usize = 1024;
/// DIFAT sectors followed at most.
const MAX_DIFAT_SECTORS: usize = 1024;

/// Stream names that mark an encrypted (password-protected) OOXML package.
const ENCRYPTED_STREAMS: [&str; 2] = ["EncryptedPackage", "EncryptionInfo"];

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// Whether the compound file holds an encryption stream. Walks the directory through the FAT;
/// when the structure can't be followed (truncated or malformed), falls back to scanning the
/// bytes for the UTF-16LE stream names.
pub(crate) fn is_encrypted(bytes: &[u8]) -> bool {
    match directory_names(bytes) {
        Some(names) => names.iter().any(|n| ENCRYPTED_STREAMS.iter().any(|e| n.eq_ignore_ascii_case(e))),
        None => ENCRYPTED_STREAMS.iter().any(|e| contains_utf16(bytes, e)),
    }
}

fn contains_utf16(hay: &[u8], needle: &str) -> bool {
    let n: Vec<u8> = needle.encode_utf16().flat_map(u16::to_le_bytes).collect();
    !n.is_empty() && hay.windows(n.len()).any(|w| w == n.as_slice())
}

/// Names of every directory entry, or `None` when the directory can't be read.
fn directory_names(b: &[u8]) -> Option<Vec<String>> {
    if !b.starts_with(&SIGNATURE) {
        return None;
    }
    let shift = u16_at(b, 30)?;
    if shift != 9 && shift != 12 {
        return None;
    }
    let size = 1usize << shift;
    let per = size / 4;
    let sector = |id: u32| -> Option<&[u8]> {
        let start = (id as usize).checked_add(1)?.checked_mul(size)?;
        b.get(start..start.checked_add(size)?)
    };
    // The DIFAT: the FAT's sector ids, 109 in the header then chained DIFAT sectors.
    let mut difat: Vec<u32> = (0..109).filter_map(|i| u32_at(b, 76 + i * 4)).filter(|&s| s < MAX_REG_SECT).collect();
    let mut next = u32_at(b, 68)?;
    for _ in 0..MAX_DIFAT_SECTORS {
        if next >= MAX_REG_SECT {
            break;
        }
        let s = sector(next)?;
        difat.extend((0..per - 1).filter_map(|i| u32_at(s, i * 4)).filter(|&x| x < MAX_REG_SECT));
        next = u32_at(s, (per - 1) * 4)?;
    }
    let fat_next = |id: u32| -> Option<u32> {
        let idx = id as usize;
        let fat_sector = *difat.get(idx / per)?;
        u32_at(sector(fat_sector)?, (idx % per) * 4)
    };
    let mut names = Vec::new();
    let mut id = u32_at(b, 48)?;
    for _ in 0..MAX_DIR_SECTORS {
        if id == END_OF_CHAIN {
            return Some(names);
        }
        if id >= MAX_REG_SECT {
            return None;
        }
        let s = sector(id)?;
        for e in s.as_chunks::<128>().0 {
            let len = usize::from(u16_at(e, 64)?);
            if !(2..=64).contains(&len) {
                continue;
            }
            let units: Vec<u16> = (0..(len - 2) / 2).filter_map(|i| u16_at(e, i * 2)).collect();
            names.push(String::from_utf16_lossy(&units));
        }
        id = fat_next(id)?;
    }
    // A directory longer than the bound (or a cyclic chain): report what was read.
    Some(names)
}

/// Directory entry type of a storage object ([MS-CFB] 2.6.1).
const STORAGE_OBJECT: u8 = 1;
/// Directory entry type of a stream object ([MS-CFB] 2.6.1).
const STREAM_OBJECT: u8 = 2;
/// Path elements followed at most by [`Compound::stream_at_path`] and
/// [`Compound::children_at_path`]. A VBA project needs three (`_VBA_PROJECT_CUR/VBA/dir`).
const MAX_PATH_DEPTH: usize = 8;
/// "No entry" in the directory tree (NOSTREAM).
const NO_STREAM: u32 = 0xFFFF_FFFF;
/// Streams larger than this are refused (the format allows more, a workbook never needs it).
const MAX_STREAM: u64 = 1 << 30;

/// One directory entry ([MS-CFB] 2.6).
struct DirEntry {
    name: String,
    kind: u8,
    left: u32,
    right: u32,
    child: u32,
    start: u32,
    size: u64,
}

/// Whether two directory entry names are the same name. [MS-CFB] 2.6.4 compares names as UTF-16
/// code units after mapping each one to upper case with the simple (one-to-one) Unicode mapping,
/// so `ä` and `Ä` are the same name and `ß` (whose upper case is two letters) only matches itself.
/// Code units with no single-unit upper case, and surrogates, are compared as they are.
fn names_match(a: &str, b: &str) -> bool {
    /// The simple (one-to-one) upper case of one UTF-16 code unit; a unit with no single-unit upper case, or a surrogate, is returned unchanged.
    fn upper(u: u16) -> u16 {
        let Some(c) = char::from_u32(u32::from(u)) else { return u };
        let mut up = c.to_uppercase();
        match (up.next(), up.next()) {
            (Some(x), None) => u16::try_from(u32::from(x)).unwrap_or(u),
            _ => u,
        }
    }
    a.encode_utf16().map(upper).eq(b.encode_utf16().map(upper))
}
/// A read-only reader for a Compound File Binary (CFB/OLE2) container: the FAT, the mini FAT
/// and the directory, all bounds-checked. Streams are found by path through nested storages
/// ([`Compound::stream_at_path`]) and storages can be listed ([`Compound::children_at_path`]).
/// Names match case-insensitively with [MS-CFB] 2.6.4 simple upper-casing, so accented names match across case.
/// Every chain and tree walk is bounded, so hostile files end in an error, never a hang.
/// Implemented from Microsoft's public [MS-CFB] specification.
pub struct Compound<'a> {
    bytes: &'a [u8],
    sector_size: usize,
    mini_sector_size: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    mini_fat: Vec<u32>,
    entries: Vec<DirEntry>,
}

impl<'a> Compound<'a> {
    /// Parses the header, FAT, mini FAT and directory. Errors on anything that can't be followed.
    ///
    /// # Errors
    ///
    /// A message describing the damage when the header, FAT, DIFAT or directory is malformed,
    /// loops or points outside the file.
    pub fn open(b: &'a [u8]) -> Result<Compound<'a>, String> {
        let bad = |m: &str| format!("damaged compound file: {m}");
        if !b.starts_with(&SIGNATURE) {
            return Err(bad("missing signature"));
        }
        let major = u16_at(b, 26).ok_or_else(|| bad("truncated header"))?;
        let shift = u16_at(b, 30).ok_or_else(|| bad("truncated header"))?;
        if shift != 9 && shift != 12 {
            return Err(bad("unsupported sector size"));
        }
        let mini_shift = u16_at(b, 32).ok_or_else(|| bad("truncated header"))?;
        if mini_shift != 6 {
            return Err(bad("unsupported mini sector size"));
        }
        let sector_size = 1usize << shift;
        let per = sector_size / 4;
        // Sectors that physically exist after the header sector: every chain is bounded by it.
        let sectors = (b.len() / sector_size).saturating_sub(1);
        let sector = |id: u32| -> Option<&[u8]> {
            let start = (id as usize).checked_add(1)?.checked_mul(sector_size)?;
            b.get(start..start.checked_add(sector_size)?)
        };
        // DIFAT: 109 FAT sector ids in the header, then chained DIFAT sectors.
        let mut fat_sectors: Vec<u32> = (0..109).filter_map(|i| u32_at(b, 76 + i * 4)).filter(|&s| s < MAX_REG_SECT).collect();
        let mut next = u32_at(b, 68).ok_or_else(|| bad("truncated header"))?;
        let mut hops = 0usize;
        while next < MAX_REG_SECT {
            hops += 1;
            if hops > sectors.min(MAX_DIFAT_SECTORS * 64) {
                return Err(bad("DIFAT chain loops"));
            }
            let s = sector(next).ok_or_else(|| bad("DIFAT sector out of range"))?;
            fat_sectors.extend((0..per - 1).filter_map(|i| u32_at(s, i * 4)).filter(|&x| x < MAX_REG_SECT));
            next = u32_at(s, (per - 1) * 4).ok_or_else(|| bad("truncated DIFAT sector"))?;
        }
        if fat_sectors.len() > sectors {
            return Err(bad("more FAT sectors than the file holds"));
        }
        let mut fat = Vec::with_capacity(fat_sectors.len().saturating_mul(per));
        for id in fat_sectors {
            let s = sector(id).ok_or_else(|| bad("FAT sector out of range"))?;
            fat.extend((0..per).filter_map(|i| u32_at(s, i * 4)));
        }
        let mut cf = Compound { bytes: b, sector_size, mini_sector_size: 64, mini_cutoff: 4096, fat, mini_fat: vec![], entries: vec![] };
        if let Some(c) = u32_at(b, 56) {
            cf.mini_cutoff = u64::from(c);
        }
        // Directory.
        let dir_start = u32_at(b, 48).ok_or_else(|| bad("truncated header"))?;
        let dir = cf.read_chain(dir_start, u64::MAX)?;
        for e in dir.as_chunks::<128>().0 {
            let len = usize::from(u16_at(e, 64).unwrap_or(0)).min(64);
            let units: Vec<u16> = (0..len.saturating_sub(2) / 2).filter_map(|i| u16_at(e, i * 2)).collect();
            let mut size = u32_at(e, 120).map(u64::from).unwrap_or(0);
            if major >= 4 {
                size |= u64::from(u32_at(e, 124).unwrap_or(0)) << 32;
            }
            cf.entries.push(DirEntry {
                name: String::from_utf16_lossy(&units),
                kind: e.get(66).copied().unwrap_or(0),
                left: u32_at(e, 68).unwrap_or(NO_STREAM),
                right: u32_at(e, 72).unwrap_or(NO_STREAM),
                child: u32_at(e, 76).unwrap_or(NO_STREAM),
                start: u32_at(e, 116).unwrap_or(END_OF_CHAIN),
                size,
            });
        }
        if cf.entries.first().is_none_or(|r| r.kind != 5) {
            return Err(bad("missing root entry"));
        }
        // Mini FAT.
        let mini_start = u32_at(b, 60).ok_or_else(|| bad("truncated header"))?;
        if mini_start < MAX_REG_SECT {
            let raw = cf.read_chain(mini_start, u64::MAX)?;
            cf.mini_fat = (0..raw.len() / 4).filter_map(|i| u32_at(&raw, i * 4)).collect();
        }
        Ok(cf)
    }

    /// Follows a FAT chain from `start`, reading at most `limit` bytes. A chain longer than the
    /// FAT (so a cycle) or pointing outside the file is an error. The file may end inside the
    /// chain's last sector (some writers don't pad it, and Excel opens those files): that sector
    /// reads short, and the caller checks the stream's declared size against what was read.
    fn read_chain(&self, start: u32, limit: u64) -> Result<Vec<u8>, String> {
        let limit = usize::try_from(limit.min(MAX_STREAM)).unwrap_or(usize::MAX);
        let mut out = Vec::new();
        let mut id = start;
        let mut seen = vec![false; self.fat.len()];
        let mut cut = false;
        while id != END_OF_CHAIN && out.len() < limit {
            if cut {
                return Err("damaged compound file: sector out of range (the file may be truncated)".into());
            }
            if id >= MAX_REG_SECT {
                return Err("damaged compound file: broken sector chain".into());
            }
            match seen.get_mut(id as usize) {
                Some(v) if !*v => *v = true,
                Some(_) => return Err("damaged compound file: sector chain loops".into()),
                None => return Err("damaged compound file: sector missing from the FAT".into()),
            }
            let start = (id as usize).checked_add(1).and_then(|i| i.checked_mul(self.sector_size));
            let s = start
                .and_then(|s| self.bytes.get(s..s.checked_add(self.sector_size)?.min(self.bytes.len())))
                .ok_or("damaged compound file: sector out of range (the file may be truncated)")?;
            cut = s.len() < self.sector_size;
            out.extend_from_slice(s);
            id = *self.fat.get(id as usize).ok_or("damaged compound file: sector missing from the FAT")?;
        }
        Ok(out)
    }

    /// Follows a mini FAT chain inside the mini stream container. Like `read_chain`, the
    /// container may end inside the chain's last mini sector.
    fn read_mini_chain(&self, container: &[u8], start: u32, limit: usize) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        let mut id = start;
        let mut seen = vec![false; self.mini_fat.len()];
        let mut cut = false;
        while id != END_OF_CHAIN && out.len() < limit {
            if cut {
                return Err("damaged compound file: mini sector out of range".into());
            }
            if id >= MAX_REG_SECT {
                return Err("damaged compound file: broken mini sector chain".into());
            }
            match seen.get_mut(id as usize) {
                Some(v) if !*v => *v = true,
                Some(_) => return Err("damaged compound file: mini sector chain loops".into()),
                None => return Err("damaged compound file: mini sector missing from the mini FAT".into()),
            }
            let at = (id as usize).checked_mul(self.mini_sector_size);
            let s = at
                .and_then(|a| container.get(a..a.checked_add(self.mini_sector_size)?.min(container.len())))
                .ok_or("damaged compound file: mini sector out of range")?;
            cut = s.len() < self.mini_sector_size;
            out.extend_from_slice(s);
            id = *self.mini_fat.get(id as usize).ok_or("damaged compound file: mini sector missing from the mini FAT")?;
        }
        Ok(out)
    }

    /// Walks the children of `parent` (a red-black tree through left/right siblings), bounded by
    /// the directory size, calling `visit` on each entry until it returns `true`. Returns the
    /// entry `visit` stopped at, or `None` when the walk finished.
    fn walk_children<'s>(&'s self, parent: &DirEntry, mut visit: impl FnMut(&'s DirEntry) -> bool) -> Result<Option<&'s DirEntry>, String> {
        let mut stack = vec![parent.child];
        let mut seen = 0usize;
        while let Some(i) = stack.pop() {
            if i == NO_STREAM {
                continue;
            }
            seen += 1;
            if seen > self.entries.len() {
                return Err("damaged compound file: directory tree loops".into());
            }
            let Some(e) = self.entries.get(i as usize) else { continue };
            if visit(e) {
                return Ok(Some(e));
            }
            stack.push(e.left);
            stack.push(e.right);
        }
        Ok(None)
    }
    /// The entry of kind `want` called `name` (case-insensitive, as [`names_match`] compares) directly
    /// under `parent`.
    fn child_entry(&self, parent: &DirEntry, name: &str, want: u8) -> Result<Option<&DirEntry>, String> {
        self.walk_children(parent, |e| e.kind == want && names_match(&e.name, name))
    }
    /// Checks a path's depth against [`MAX_PATH_DEPTH`].
    fn check_depth(path: &[&str]) -> Result<(), String> {
        if path.len() > MAX_PATH_DEPTH {
            return Err(format!("compound file path is deeper than {MAX_PATH_DEPTH} elements"));
        }
        Ok(())
    }
    /// The storage reached by following `path` from the root through storage entries; the root
    /// itself for an empty path. `None` when a hop is missing.
    fn storage_at_path(&self, path: &[&str]) -> Result<Option<&DirEntry>, String> {
        Self::check_depth(path)?;
        let Some(mut at) = self.entries.first() else { return Ok(None) };
        for name in path {
            match self.child_entry(at, name, STORAGE_OBJECT)? {
                Some(e) => at = e,
                None => return Ok(None),
            }
        }
        Ok(Some(at))
    }
    /// The directory entry of the stream at `path`: every element but the last names a storage,
    /// the last names a stream. An empty path or one deeper than [`MAX_PATH_DEPTH`] is an error.
    fn entry_at_path(&self, path: &[&str]) -> Result<Option<&DirEntry>, String> {
        let Some((last, storages)) = path.split_last() else { return Err("empty compound file path".into()) };
        Self::check_depth(path)?;
        let Some(parent) = self.storage_at_path(storages)? else { return Ok(None) };
        self.child_entry(parent, last, STREAM_OBJECT)
    }
    /// Whether the root storage holds a stream called `name`, without reading it.
    pub(crate) fn has_root_stream(&self, name: &str) -> bool {
        matches!(self.entry_at_path(&[name]), Ok(Some(_)))
    }
    /// The contents of the stream called `name` (case-insensitive) directly under the root
    /// storage, or `None` when there is no such stream.
    pub(crate) fn root_stream(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        self.stream_at_path(&[name])
    }
    /// The contents of the stream at `path`, where every element but the last names a storage
    /// and the last names a stream (each matched case-insensitively), starting at the root
    /// storage. For example `&["_VBA_PROJECT_CUR", "VBA", "dir"]`. Returns `None` when any
    /// element is missing or has the wrong kind.
    ///
    /// # Errors
    ///
    /// An empty path, a path deeper than 8 elements, a stream over 1 GiB, a looping directory
    /// tree or sector chain, or a stream shorter than its declared size.
    pub fn stream_at_path(&self, path: &[&str]) -> Result<Option<Vec<u8>>, String> {
        let Some(e) = self.entry_at_path(path)? else { return Ok(None) };
        self.read_stream(e).map(Some)
    }
    /// The names of every entry (streams and storages) directly under the storage at `path`,
    /// sorted by ordinal (`str` order) so the result doesn't depend on the tree's shape. An empty
    /// path lists the root storage.
    ///
    /// # Errors
    ///
    /// A path deeper than 8 elements, a missing storage on the path, or a looping directory tree.
    pub fn children_at_path(&self, path: &[&str]) -> Result<Vec<String>, String> {
        let Some(storage) = self.storage_at_path(path)? else {
            return Err(format!("compound file has no storage at {}", path.join("/")));
        };
        let mut names = Vec::new();
        self.walk_children(storage, |e| {
            names.push(e.name.clone());
            false
        })?;
        names.sort();
        Ok(names)
    }
    /// Reads a stream entry's bytes from the mini stream (below the cutoff) or regular sectors,
    /// refusing streams over [`MAX_STREAM`] and truncating to the declared size.
    fn read_stream(&self, e: &DirEntry) -> Result<Vec<u8>, String> {
        let Some(root) = self.entries.first() else { return Err("damaged compound file: missing root entry".into()) };
        if e.size > MAX_STREAM {
            return Err("a compound file stream is larger than 1 GiB".into());
        }
        let size = usize::try_from(e.size).map_err(|_| "stream too large".to_string())?;
        let mut data = if e.size < self.mini_cutoff {
            let container = self.read_chain(root.start, root.size)?;
            self.read_mini_chain(&container, e.start, size)?
        } else {
            self.read_chain(e.start, e.size)?
        };
        if data.len() < size {
            return Err("damaged compound file: a stream is shorter than its declared size (the file may be truncated)".into());
        }
        data.truncate(size);
        Ok(data)
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal valid compound file (512-byte sectors: FAT at sector 0, directory at sector 1)
    /// whose directory holds a root entry plus the given stream names.
    pub(crate) fn build(streams: &[&str]) -> Vec<u8> {
        let mut b = vec![0u8; 512 * 3];
        b[..8].copy_from_slice(&SIGNATURE);
        b[24..26].copy_from_slice(&0x3Eu16.to_le_bytes()); // minor version
        b[26..28].copy_from_slice(&3u16.to_le_bytes()); // major version
        b[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes()); // byte order
        b[30..32].copy_from_slice(&9u16.to_le_bytes()); // sector shift
        b[32..34].copy_from_slice(&6u16.to_le_bytes()); // mini sector shift
        b[44..48].copy_from_slice(&1u32.to_le_bytes()); // FAT sectors
        b[48..52].copy_from_slice(&1u32.to_le_bytes()); // first directory sector
        b[60..64].copy_from_slice(&END_OF_CHAIN.to_le_bytes()); // no mini FAT
        b[68..72].copy_from_slice(&END_OF_CHAIN.to_le_bytes()); // no DIFAT sectors
        for i in 0..109 {
            b[76 + i * 4..80 + i * 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        }
        b[76..80].copy_from_slice(&0u32.to_le_bytes()); // FAT lives in sector 0
        // FAT (sector 0, at byte 512): sector 0 is a FAT sector, sector 1 ends its chain.
        for i in 0..128 {
            b[512 + i * 4..516 + i * 4].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        }
        b[512..516].copy_from_slice(&0xFFFF_FFFDu32.to_le_bytes());
        b[516..520].copy_from_slice(&END_OF_CHAIN.to_le_bytes());
        // Directory (sector 1, at byte 1024).
        for (k, name) in std::iter::once("Root Entry").chain(streams.iter().copied()).take(4).enumerate() {
            let e = 1024 + k * 128;
            let units: Vec<u16> = name.encode_utf16().take(31).collect();
            for (i, u) in units.iter().enumerate() {
                b[e + i * 2..e + i * 2 + 2].copy_from_slice(&u.to_le_bytes());
            }
            b[e + 64..e + 66].copy_from_slice(&((units.len() as u16 + 1) * 2).to_le_bytes());
            b[e + 66] = if k == 0 { 5 } else { 2 };
        }
        b
    }

    /// A one-element path finds the same stream as `root_stream`, case-insensitively; a stream can't be an intermediate hop, and an empty path is an error.
    #[test]
    fn root_level_paths_match_root_streams() {
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let file = crate::testing::compound_file(&[("Small", b"hello"), ("Workbook", &big)]);
        let cf = Compound::open(&file).unwrap();
        for name in ["Small", "Workbook", "workbook", "Missing"] {
            assert_eq!(cf.stream_at_path(&[name]).unwrap(), cf.root_stream(name).unwrap(), "{name}");
        }
        assert_eq!(cf.stream_at_path(&["SMALL"]).unwrap().unwrap(), b"hello");
        assert_eq!(cf.stream_at_path(&["Missing"]).unwrap(), None);
        // A stream is not a storage: it can't be an intermediate hop.
        assert_eq!(cf.stream_at_path(&["Small", "X"]).unwrap(), None);
        assert_eq!(cf.stream_at_path(&["Nope", "X"]).unwrap(), None);
        assert!(cf.stream_at_path(&[]).is_err());
        assert!(cf.stream_at_path(&["a"; MAX_PATH_DEPTH]).is_ok());
        assert!(cf.stream_at_path(&["a"; MAX_PATH_DEPTH + 1]).is_err());
        assert!(cf.has_root_stream("Workbook") && !cf.has_root_stream("Missing"));
    }
    #[test]
    fn children_at_path_lists_the_root_sorted() {
        let file = crate::testing::compound_file(&[("Workbook", &[1, 2, 3]), ("Small", b"hello"), ("EncryptionInfo", &[])]);
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.children_at_path(&[]).unwrap(), vec!["EncryptionInfo", "Small", "Workbook"]);
        let empty = crate::testing::compound_file(&[]);
        assert_eq!(Compound::open(&empty).unwrap().children_at_path(&[]).unwrap(), Vec::<String>::new());
        // Streams have no children; a missing storage or an over-deep path is an error.
        assert!(cf.children_at_path(&["Workbook"]).is_err());
        assert!(cf.children_at_path(&["Missing"]).is_err());
        assert!(cf.children_at_path(&["a"; MAX_PATH_DEPTH + 1]).is_err());
    }
    use crate::testing::{Node, build_tree};
    /// Deterministic filler bytes, different for each `seed`.
    fn filler(len: usize, seed: u32) -> Vec<u8> {
        (0..len as u32).map(|i| (i.wrapping_mul(31).wrapping_add(seed) % 251) as u8).collect()
    }
    /// A VBA-shaped tree: a root `PROJECT` and `\u{5}DigitalSignature`, and a `VBA` storage
    /// holding a mini-stream `dir`, a regular-sector `Module1` and an empty `_VBA_PROJECT`.
    fn vba_tree() -> Vec<u8> {
        build_tree(&[
            Node::Stream("PROJECT", filler(300, 1)),
            Node::Storage(
                "VBA",
                vec![Node::Stream("dir", filler(1000, 2)), Node::Stream("Module1", filler(5000, 3)), Node::Stream("_VBA_PROJECT", vec![])],
            ),
            Node::Stream("\u{5}DigitalSignature", filler(40, 4)),
        ])
    }
    /// The directory index of the entry called `name`.
    fn index_of(file: &[u8], name: &str) -> usize {
        Compound::open(file).unwrap().entries.iter().position(|e| e.name == name).unwrap()
    }
    /// Overwrites the u32 at `field` of directory entry `k` (the writer lays the directory out
    /// in consecutive sectors).
    fn patch_entry(file: &mut [u8], k: usize, field: usize, v: u32) {
        let at = 512 * (1 + u32_at(file, 48).unwrap() as usize) + k * 128 + field;
        file[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    /// Overwrites FAT (`mini` false, sector 0 holds the FAT) or mini FAT entry `id` with `v`.
    fn patch_fat(file: &mut [u8], mini: bool, id: u32, v: u32) {
        let sector = if mini { u32_at(file, 60).unwrap() } else { u32_at(file, 76).unwrap() };
        let at = 512 * (1 + sector as usize) + id as usize * 4;
        file[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    const RIGHT: usize = 72;
    const LEFT: usize = 68;
    const CHILD: usize = 76;
    const SIZE: usize = 120;
    #[test]
    fn nested_streams_round_trip() {
        let file = vba_tree();
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.stream_at_path(&["PROJECT"]).unwrap().unwrap(), filler(300, 1));
        assert_eq!(cf.stream_at_path(&["VBA", "dir"]).unwrap().unwrap(), filler(1000, 2));
        assert_eq!(cf.stream_at_path(&["VBA", "Module1"]).unwrap().unwrap(), filler(5000, 3));
        assert_eq!(cf.stream_at_path(&["VBA", "_VBA_PROJECT"]).unwrap().unwrap(), Vec::<u8>::new());
        assert_eq!(cf.stream_at_path(&["\u{5}DigitalSignature"]).unwrap().unwrap(), filler(40, 4));
        // Matching is case-insensitive at every hop.
        assert_eq!(cf.stream_at_path(&["vba", "MODULE1"]).unwrap().unwrap(), filler(5000, 3));
        assert_eq!(cf.stream_at_path(&["Vba", "DIR"]).unwrap().unwrap(), filler(1000, 2));
        // A storage is not a stream, and a stream doesn't leak out of its storage.
        assert_eq!(cf.stream_at_path(&["VBA"]).unwrap(), None);
        assert_eq!(cf.stream_at_path(&["dir"]).unwrap(), None);
        assert!(!cf.has_root_stream("VBA"));
    }
    #[test]
    fn children_are_listed_sorted_with_control_chars_kept() {
        let file = vba_tree();
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.children_at_path(&["VBA"]).unwrap(), vec!["Module1", "_VBA_PROJECT", "dir"]);
        assert_eq!(cf.children_at_path(&["vba"]).unwrap(), vec!["Module1", "_VBA_PROJECT", "dir"]);
        assert_eq!(cf.children_at_path(&[]).unwrap(), vec!["\u{5}DigitalSignature", "PROJECT", "VBA"]);
        let empty = build_tree(&[Node::Storage("VBA", vec![])]);
        assert_eq!(Compound::open(&empty).unwrap().children_at_path(&["VBA"]).unwrap(), Vec::<String>::new());
    }
    /// [`vba_tree`] relinked so every sibling is reached through a left link: the root's child
    /// is `\u{5}DigitalSignature`, whose left is the `VBA` storage, whose left is `PROJECT`; inside
    /// `VBA` the child is `_VBA_PROJECT`, whose left is `Module1`, whose left is `dir`. No right
    /// link is set, so a reader that skipped left links would find nothing past the first entry.
    fn left_linked_vba_tree() -> Vec<u8> {
        let mut file = vba_tree();
        let at = |name: &str| index_of(&vba_tree(), name);
        let (project, vba, sig) = (at("PROJECT"), at("VBA"), at("\u{5}DigitalSignature"));
        let (dir, module, vba_project) = (at("dir"), at("Module1"), at("_VBA_PROJECT"));
        for k in [project, vba, sig, dir, module, vba_project] {
            patch_entry(&mut file, k, RIGHT, NO_STREAM);
            patch_entry(&mut file, k, LEFT, NO_STREAM);
        }
        patch_entry(&mut file, 0, CHILD, sig as u32);
        patch_entry(&mut file, sig, LEFT, vba as u32);
        patch_entry(&mut file, vba, LEFT, project as u32);
        patch_entry(&mut file, vba, CHILD, vba_project as u32);
        patch_entry(&mut file, vba_project, LEFT, module as u32);
        patch_entry(&mut file, module, LEFT, dir as u32);
        file
    }
    /// Valid sibling trees linked through left links (a storage and streams, at the root and
    /// inside a storage) are found and listed the same as right-linked ones.
    #[test]
    fn siblings_reached_through_left_links_are_found_and_listed() {
        let file = left_linked_vba_tree();
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.stream_at_path(&["PROJECT"]).unwrap().unwrap(), filler(300, 1));
        assert_eq!(cf.stream_at_path(&["\u{5}DigitalSignature"]).unwrap().unwrap(), filler(40, 4));
        assert_eq!(cf.stream_at_path(&["VBA", "dir"]).unwrap().unwrap(), filler(1000, 2));
        assert_eq!(cf.stream_at_path(&["vba", "MODULE1"]).unwrap().unwrap(), filler(5000, 3));
        assert_eq!(cf.stream_at_path(&["VBA", "_VBA_PROJECT"]).unwrap().unwrap(), Vec::<u8>::new());
        assert!(cf.has_root_stream("PROJECT"));
        assert!(!cf.has_root_stream("VBA"));
        assert_eq!(cf.children_at_path(&["VBA"]).unwrap(), vec!["Module1", "_VBA_PROJECT", "dir"]);
        assert_eq!(cf.children_at_path(&[]).unwrap(), vec!["\u{5}DigitalSignature", "PROJECT", "VBA"]);
    }
    #[test]
    fn missing_or_wrong_kind_intermediates() {
        let file = vba_tree();
        let cf = Compound::open(&file).unwrap();
        // A missing intermediate storage: no stream, and nothing to list.
        assert_eq!(cf.stream_at_path(&["Nope", "dir"]).unwrap(), None);
        assert_eq!(cf.stream_at_path(&["VBA", "Nope", "dir"]).unwrap(), None);
        assert!(cf.children_at_path(&["Nope"]).is_err());
        assert!(cf.children_at_path(&["VBA", "Nope"]).is_err());
        // A stream used as a storage behaves the same way.
        assert_eq!(cf.stream_at_path(&["PROJECT", "dir"]).unwrap(), None);
        assert_eq!(cf.stream_at_path(&["VBA", "dir", "x"]).unwrap(), None);
        assert!(cf.children_at_path(&["PROJECT"]).is_err());
        assert!(cf.children_at_path(&["VBA", "dir"]).is_err());
    }
    /// `depth` nested storages `S0/S1/…`, the innermost holding a stream `x`.
    fn nested(depth: usize) -> (Vec<u8>, Vec<String>) {
        let names: Vec<String> = (0..depth).map(|i| format!("S{i}")).collect();
        let mut node = Node::Stream("x", filler(100, 9));
        for name in names.iter().rev() {
            node = Node::Storage(name.as_str(), vec![node]);
        }
        (build_tree(&[node]), names)
    }
    #[test]
    fn deep_paths_are_capped_at_eight() {
        let file = build_tree(&[Node::Storage(
            "_VBA_PROJECT_CUR",
            vec![Node::Stream("PROJECT", filler(50, 5)), Node::Storage("VBA", vec![Node::Stream("dir", filler(5000, 6))])],
        )]);
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.stream_at_path(&["_VBA_PROJECT_CUR", "VBA", "dir"]).unwrap().unwrap(), filler(5000, 6));
        assert_eq!(cf.stream_at_path(&["_VBA_PROJECT_CUR", "PROJECT"]).unwrap().unwrap(), filler(50, 5));
        assert_eq!(cf.children_at_path(&["_VBA_PROJECT_CUR"]).unwrap(), vec!["PROJECT", "VBA"]);
        // Seven storages and a stream: exactly the cap.
        let (file, names) = nested(MAX_PATH_DEPTH - 1);
        let cf = Compound::open(&file).unwrap();
        let mut path: Vec<&str> = names.iter().map(String::as_str).collect();
        path.push("x");
        assert_eq!(cf.stream_at_path(&path).unwrap().unwrap(), filler(100, 9));
        assert_eq!(cf.children_at_path(&path[..MAX_PATH_DEPTH - 1]).unwrap(), vec!["x"]);
        // Eight storages and a stream: the stream exists but the path is too deep.
        let (file, names) = nested(MAX_PATH_DEPTH);
        let cf = Compound::open(&file).unwrap();
        let mut path: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_eq!(cf.children_at_path(&path).unwrap(), vec!["x"]);
        path.push("x");
        assert!(cf.stream_at_path(&path).unwrap_err().contains("deeper than 8"));
        assert!(cf.children_at_path(&path).is_err());
    }
    /// A sibling link inside the `VBA` storage that loops (to itself, to an earlier sibling, or back through a left link) is an error for lookups and listings there, while root-level streams still read.
    #[test]
    fn a_sibling_loop_inside_a_storage_is_an_error() {
        let clean = vba_tree();
        let (dir, module) = (index_of(&clean, "dir"), index_of(&clean, "Module1"));
        // Module1's right sibling is itself, then an earlier sibling; then a left link back.
        for (k, field, to) in [(module, RIGHT, module), (module, RIGHT, dir), (module, LEFT, dir)] {
            let mut file = clean.clone();
            patch_entry(&mut file, k, field, to as u32);
            let cf = Compound::open(&file).unwrap();
            assert!(cf.stream_at_path(&["VBA", "Missing"]).unwrap_err().contains("directory tree loops"), "{k} {field} {to}");
            assert!(cf.children_at_path(&["VBA"]).unwrap_err().contains("directory tree loops"));
            // The root level is untouched.
            assert_eq!(cf.stream_at_path(&["PROJECT"]).unwrap().unwrap(), filler(300, 1));
        }
    }
    /// Child links that cycle between two storages, or a storage that is its own child, end in an error within the path depth cap, never a hang.
    #[test]
    fn a_child_cycle_between_storages_is_bounded() {
        let clean = build_tree(&[Node::Storage("A", vec![Node::Storage("B", vec![Node::Stream("x", filler(10, 7))])])]);
        let (a, b) = (index_of(&clean, "A"), index_of(&clean, "B"));
        // B's child is A (a cycle through two storages), then B itself.
        for to in [a, b] {
            let mut file = clean.clone();
            patch_entry(&mut file, b, CHILD, to as u32);
            let cf = Compound::open(&file).unwrap();
            let cycle: Vec<&str> = ["A", "B"].iter().copied().cycle().take(MAX_PATH_DEPTH + 1).collect();
            for len in 1..=MAX_PATH_DEPTH {
                let _ = cf.stream_at_path(&cycle[..len]);
                let _ = cf.children_at_path(&cycle[..len]);
            }
            assert!(cf.stream_at_path(&cycle).is_err());
            assert!(cf.children_at_path(&cycle).is_err());
            assert_eq!(cf.stream_at_path(&["A", "B", "x"]).unwrap(), None);
        }
        // A storage whose child is itself and whose child list loops back.
        let mut file = clean.clone();
        patch_entry(&mut file, a, CHILD, a as u32);
        patch_entry(&mut file, a, RIGHT, a as u32);
        let cf = Compound::open(&file).unwrap();
        assert!(cf.children_at_path(&["A"]).unwrap_err().contains("directory tree loops"));
        assert!(cf.stream_at_path(&["A", "x"]).is_err());
    }
    #[test]
    fn a_cyclic_sector_chain_is_an_error() {
        let clean = vba_tree();
        let cf = Compound::open(&clean).unwrap();
        let start = |name: &str| cf.entries[index_of(&clean, name)].start;
        // A regular-sector stream whose first FAT entry points back at itself.
        let mut file = clean.clone();
        patch_fat(&mut file, false, start("Module1"), start("Module1"));
        let err = Compound::open(&file).unwrap().stream_at_path(&["VBA", "Module1"]).unwrap_err();
        assert!(err.contains("sector chain loops"), "{err}");
        // The same in the mini FAT.
        let mut file = clean.clone();
        patch_fat(&mut file, true, start("dir"), start("dir"));
        let err = Compound::open(&file).unwrap().stream_at_path(&["VBA", "dir"]).unwrap_err();
        assert!(err.contains("mini sector chain loops"), "{err}");
    }
    /// A stream whose declared size is larger than its sector chain holds, in regular and in mini sectors, is an error instead of a short read.
    #[test]
    fn a_declared_size_past_the_data_is_an_error() {
        let clean = vba_tree();
        // Module1 holds 10 sectors (5120 bytes); dir holds 16 mini sectors (1024 bytes).
        for (name, size) in [("Module1", 6000u32), ("dir", 2000)] {
            let mut file = clean.clone();
            patch_entry(&mut file, index_of(&clean, name), SIZE, size);
            let err = Compound::open(&file).unwrap().stream_at_path(&["VBA", name]).unwrap_err();
            assert!(err.contains("shorter than its declared size"), "{name}: {err}");
        }
    }
    #[test]
    fn reads_directory_names() {
        let b = build(&["EncryptionInfo", "EncryptedPackage"]);
        assert_eq!(directory_names(&b), Some(vec!["Root Entry".into(), "EncryptionInfo".into(), "EncryptedPackage".into()]));
        assert!(is_encrypted(&b));
        assert!(!is_encrypted(&build(&["Workbook", "\u{5}SummaryInformation"])));
    }

    #[test]
    fn hostile_headers_never_panic() {
        let mut b = build(&["Workbook"]);
        for at in [30usize, 48, 68, 76, 512, 516, 1024 + 64] {
            for v in [0u32, 1, 7, 0xFFFF_FFFA, 0xFFFF_FFFF, 0x7FFF_FFFF] {
                let mut c = b.clone();
                c[at..at + 4].copy_from_slice(&v.to_le_bytes());
                let _ = is_encrypted(&c);
                for cut in [0usize, 8, 100, 600, 1100] {
                    let _ = is_encrypted(&c[..cut.min(c.len())]);
                }
            }
        }
        // A directory chain that loops back on itself stops at the bound.
        b[516..520].copy_from_slice(&1u32.to_le_bytes());
        assert!(!is_encrypted(&b));
    }

    /// `compound_file` with one regular-sector stream of `len` bytes (FAT in sector 0, directory
    /// in sector 1, the stream from sector 2), cut right after the stream's last byte, the way
    /// some writers leave the final sector unpadded.
    fn unpadded(len: usize) -> (Vec<u8>, Vec<u8>) {
        let data: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();
        let mut file = crate::testing::compound_file(&[("Workbook", &data)]);
        file.truncate(512 * 3 + len);
        (file, data)
    }

    #[test]
    fn an_unpadded_last_sector_still_reads() {
        let (file, data) = unpadded(5000);
        assert_ne!(file.len() % 512, 0);
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.root_stream("Workbook").unwrap().unwrap(), data);
        // Bytes the stream needs that the file doesn't hold are still damage.
        let cf = Compound::open(&file[..file.len() - 1]).unwrap();
        assert!(cf.root_stream("Workbook").is_err());
    }

    #[test]
    fn an_unpadded_last_mini_sector_still_reads() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut file = crate::testing::compound_file(&[("Workbook", &data)]);
        // Header, FAT, directory, mini FAT, then the mini stream holding the stream's bytes.
        file.truncate(512 * 4 + data.len());
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.root_stream("Workbook").unwrap().unwrap(), data);
        let cf = Compound::open(&file[..file.len() - 1]).unwrap();
        assert!(cf.root_stream("Workbook").is_err());
    }

    #[test]
    fn a_cut_sector_inside_a_chain_is_damage() {
        let (mut file, _) = unpadded(5000);
        // The cut sector (11) points on to the directory sector and the stream claims those
        // bytes too: the partial sector would splice 392 bytes into the middle of the stream.
        file[512 + 11 * 4..512 + 12 * 4].copy_from_slice(&1u32.to_le_bytes());
        file[1024 + 128 + 120..1024 + 128 + 124].copy_from_slice(&5512u32.to_le_bytes());
        let cf = Compound::open(&file).unwrap();
        assert!(cf.root_stream("Workbook").is_err());
    }

    #[test]
    fn a_cut_mini_sector_inside_a_chain_is_damage() {
        let small = [7u8; 100];
        let data: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        let mut file = crate::testing::compound_file(&[("Small", &small), ("Workbook", &data)]);
        // "Small" holds mini sectors 0-1, "Workbook" 2-17; the mini stream starts at sector 3.
        file.truncate(512 * 4 + 128 + data.len());
        assert_eq!(Compound::open(&file).unwrap().root_stream("Workbook").unwrap().unwrap(), data);
        // The cut mini sector (17) points on to sector 0 and the stream claims those bytes too.
        file[1536 + 17 * 4..1536 + 18 * 4].copy_from_slice(&0u32.to_le_bytes());
        file[1024 + 2 * 128 + 120..1024 + 2 * 128 + 124].copy_from_slice(&1064u32.to_le_bytes());
        let cf = Compound::open(&file).unwrap();
        assert!(cf.root_stream("Workbook").is_err());
    }

    /// Storage and stream names with accented letters match in any case, and `ß` matches only itself, per [MS-CFB] 2.6.4.
    #[test]
    fn names_match_with_unicode_upper_case() {
        use crate::testing::{Node, build_tree};
        let bin = build_tree(&[Node::Storage("Ärger", vec![Node::Stream("Café", b"x".to_vec())])]);
        let cf = Compound::open(&bin).expect("valid file");
        for path in [["Ärger", "Café"], ["ärger", "CAFÉ"], ["ÄRGER", "café"]] {
            assert_eq!(cf.stream_at_path(&path).expect("reads"), Some(b"x".to_vec()), "{path:?}");
        }
        assert_eq!(cf.children_at_path(&["ärger"]).expect("lists"), vec!["Café".to_string()]);
        assert_eq!(cf.stream_at_path(&["Arger", "Cafe"]).expect("reads"), None);
        assert!(names_match("Straße", "STRAßE"));
        assert!(!names_match("Straße", "STRASSE"));
    }
}
