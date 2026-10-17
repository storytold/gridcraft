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

/// Directory entry type of a stream object ([MS-CFB] 2.6.1).
const STREAM_OBJECT: u8 = 2;
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

/// A parsed compound file: the FAT, the mini FAT and the directory, all bounds-checked.
/// Implemented from Microsoft's public [MS-CFB] specification.
pub(crate) struct Compound<'a> {
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
    pub(crate) fn open(b: &'a [u8]) -> Result<Compound<'a>, String> {
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

    /// The contents of the stream called `name` (case-insensitive) directly under the root
    /// storage, or `None` when there is no such stream.
    /// The directory entry of the stream called `name` (case-insensitive) directly under the
    /// root storage.
    fn root_entry(&self, name: &str) -> Result<Option<&DirEntry>, String> {
        let Some(root) = self.entries.first() else { return Ok(None) };
        // Walk the root's children (a red-black tree through left/right siblings), bounded.
        let mut stack = vec![root.child];
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
            if e.kind == STREAM_OBJECT && e.name.eq_ignore_ascii_case(name) {
                return Ok(Some(e));
            }
            stack.push(e.left);
            stack.push(e.right);
        }
        Ok(None)
    }

    /// Whether the root storage holds a stream called `name`, without reading it.
    pub(crate) fn has_root_stream(&self, name: &str) -> bool {
        matches!(self.root_entry(name), Ok(Some(_)))
    }

    /// The contents of the stream called `name` (case-insensitive) directly under the root
    /// storage, or `None` when there is no such stream.
    pub(crate) fn root_stream(&self, name: &str) -> Result<Option<Vec<u8>>, String> {
        let Some(root) = self.entries.first() else { return Ok(None) };
        let found = self.root_entry(name)?;
        let Some(e) = found else { return Ok(None) };
        if e.size > MAX_STREAM {
            return Err("the workbook stream is larger than 1 GiB".into());
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
        Ok(Some(data))
    }
}

// ---- writing -------------------------------------------------------------------------------

/// Sector size of the files written (version 3).
const SECTOR: usize = 512;
const MINI_SECTOR: usize = 64;
/// Streams shorter than this live in the mini stream.
const MINI_CUTOFF: usize = 4096;
const FREE_SECT: u32 = 0xFFFF_FFFF;
const FAT_SECT: u32 = 0xFFFF_FFFD;
const DIFAT_SECT: u32 = 0xFFFF_FFFC;
/// FAT sector ids the header holds; the rest go in DIFAT sectors, 127 to a sector.
const HEADER_DIFAT: usize = 109;
const STORAGE_OBJECT: u8 = 1;
const ROOT_STORAGE_OBJECT: u8 = 5;

/// A storage or stream being written.
struct Node<'a> {
    name: Vec<u16>,
    kind: u8,
    data: &'a [u8],
    children: Vec<usize>,
}

/// Where an entry sits among its siblings: its subtrees and its colour.
#[derive(Clone, Copy)]
struct Link {
    left: u32,
    right: u32,
    red: bool,
}

/// [MS-CFB] 2.6.4: siblings are ordered by name length, then by the names uppercased. The names
/// written here are ASCII, so ASCII uppercasing is the whole rule.
fn name_order(a: &[u16], b: &[u16]) -> std::cmp::Ordering {
    let upper = |u: &u16| if (0x61..=0x7A).contains(u) { u - 0x20 } else { *u };
    a.len().cmp(&b.len()).then_with(|| a.iter().map(upper).cmp(b.iter().map(upper)))
}

/// Links the `sorted` siblings into a balanced binary tree and returns its root. The nodes on
/// the deepest level are red and the rest black, so every path holds as many black nodes and
/// no red node has a red parent: a valid red-black tree, as [MS-CFB] asks for.
fn balance(sorted: &[usize], depth: u32, red_depth: u32, links: &mut [Link]) -> Result<u32, String> {
    let mid = sorted.len() / 2;
    let Some(&node) = sorted.get(mid) else { return Ok(NO_STREAM) };
    let left = balance(sorted.get(..mid).unwrap_or_default(), depth + 1, red_depth, links)?;
    let right = balance(sorted.get(mid + 1..).unwrap_or_default(), depth + 1, red_depth, links)?;
    if let Some(l) = links.get_mut(node) {
        *l = Link { left, right, red: depth > 0 && depth == red_depth };
    }
    u32::try_from(node).map_err(|_| "too many directory entries".to_string())
}

/// A version 3 compound file holding `streams`, each named by its path from the root with `/`
/// between storages (`"a/b"` is stream `b` in storage `a`). Streams under 4096 bytes go in the
/// mini stream, as Office writes them. Implemented from Microsoft's public [MS-CFB].
pub(crate) fn write(streams: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let id = |x: usize| u32::try_from(x).map_err(|_| "the file is too large for a compound file".to_string());
    let mut nodes = vec![Node { name: "Root Entry".encode_utf16().collect(), kind: ROOT_STORAGE_OBJECT, data: &[], children: vec![] }];
    for (path, data) in streams {
        let parts: Vec<&str> = path.split('/').collect();
        let mut parent = 0;
        for (k, part) in parts.iter().enumerate() {
            let stream = k + 1 == parts.len();
            let name: Vec<u16> = part.encode_utf16().collect();
            if name.is_empty() || name.len() > 31 {
                return Err(format!("'{path}' isn't a valid compound file name"));
            }
            let siblings = nodes.get(parent).map(|p| p.children.clone()).unwrap_or_default();
            let existing = siblings.into_iter().find(|&c| nodes.get(c).is_some_and(|n| name_order(&n.name, &name).is_eq()));
            parent = match existing {
                Some(c) if !stream && nodes.get(c).is_some_and(|n| n.kind == STORAGE_OBJECT) => c,
                Some(_) => return Err(format!("'{path}' is written twice")),
                None => {
                    let kind = if stream { STREAM_OBJECT } else { STORAGE_OBJECT };
                    nodes.push(Node { name, kind, data: if stream { data } else { &[] }, children: vec![] });
                    let new = nodes.len() - 1;
                    if let Some(p) = nodes.get_mut(parent) {
                        p.children.push(new);
                    }
                    new
                }
            };
        }
    }

    // The mini stream: small streams in 64-byte mini sectors, chained by the mini FAT.
    let is_mini = |n: &Node| n.kind == STREAM_OBJECT && n.data.len() < MINI_CUTOFF;
    let mut starts = vec![END_OF_CHAIN; nodes.len()];
    let mut container = vec![];
    let mut mini_fat: Vec<u32> = vec![];
    for (n, start) in nodes.iter().zip(starts.iter_mut()) {
        if !is_mini(n) || n.data.is_empty() {
            continue;
        }
        let first = mini_fat.len();
        let count = n.data.len().div_ceil(MINI_SECTOR);
        for k in 1..count {
            mini_fat.push(id(first + k)?);
        }
        mini_fat.push(END_OF_CHAIN);
        *start = id(first)?;
        container.extend_from_slice(n.data);
        container.resize((first + count) * MINI_SECTOR, 0);
    }
    let mini_fat: Vec<u8> = mini_fat.iter().flat_map(|v| v.to_le_bytes()).collect();

    // Regular sectors, in order: the FAT, the DIFAT, then the directory, the mini FAT, the mini
    // stream and each larger stream.
    let large: Vec<usize> = (0..nodes.len()).filter(|&i| nodes.get(i).is_some_and(|n| n.kind == STREAM_OBJECT && !is_mini(n))).collect();
    let dir_sectors = nodes.len().div_ceil(SECTOR / 128);
    let mut counts = vec![dir_sectors, mini_fat.len().div_ceil(SECTOR), container.len().div_ceil(SECTOR)];
    counts.extend(large.iter().map(|&i| nodes.get(i).map_or(0, |n| n.data.len().div_ceil(SECTOR))));
    let body: usize = counts.iter().sum();
    // The FAT maps every sector, its own and the DIFAT's included, 128 to a FAT sector.
    let mut fat_sectors = body.div_ceil(127).max(1);
    let difat_sectors = loop {
        let difat = fat_sectors.saturating_sub(HEADER_DIFAT).div_ceil(127);
        if fat_sectors * 128 >= fat_sectors + difat + body {
            break difat;
        }
        fat_sectors += 1;
    };
    let mut fat = vec![FAT_SECT; fat_sectors];
    fat.resize(fat_sectors + difat_sectors, DIFAT_SECT);
    let mut chain_starts = vec![];
    for &n in &counts {
        if n == 0 {
            chain_starts.push(END_OF_CHAIN);
            continue;
        }
        let first = fat.len();
        chain_starts.push(id(first)?);
        for k in 1..n {
            fat.push(id(first + k)?);
        }
        fat.push(END_OF_CHAIN);
    }
    fat.resize(fat_sectors * 128, FREE_SECT);
    let chain = |k: usize| chain_starts.get(k).copied().unwrap_or(END_OF_CHAIN);
    for (j, &i) in large.iter().enumerate() {
        if let Some(s) = starts.get_mut(i) {
            *s = chain(3 + j);
        }
    }

    // The directory: every storage's children as a red-black tree.
    let mut links = vec![Link { left: NO_STREAM, right: NO_STREAM, red: false }; nodes.len()];
    let mut child = vec![NO_STREAM; nodes.len()];
    for (n, c) in nodes.iter().zip(child.iter_mut()) {
        let mut kids = n.children.clone();
        kids.sort_by(|&a, &b| match (nodes.get(a), nodes.get(b)) {
            (Some(a), Some(b)) => name_order(&a.name, &b.name),
            _ => std::cmp::Ordering::Equal,
        });
        *c = balance(&kids, 0, kids.len().checked_ilog2().unwrap_or(0), &mut links)?;
    }
    let mut dir = Vec::with_capacity(dir_sectors * SECTOR);
    for (((n, link), c), start) in nodes.iter().zip(&links).zip(&child).zip(&starts) {
        let mut name = [0u8; 64];
        name.as_chunks_mut::<2>().0.iter_mut().zip(&n.name).for_each(|(slot, u)| slot.copy_from_slice(&u.to_le_bytes()));
        dir.extend_from_slice(&name);
        dir.extend_from_slice(&u16::try_from((n.name.len() + 1) * 2).unwrap_or(64).to_le_bytes());
        dir.extend_from_slice(&[n.kind, if link.red { 0 } else { 1 }]);
        for v in [link.left, link.right, *c] {
            dir.extend_from_slice(&v.to_le_bytes());
        }
        // CLSID, state bits, creation and modification times.
        dir.extend_from_slice(&[0; 36]);
        let (start, size) = match n.kind {
            ROOT_STORAGE_OBJECT => (chain(2), container.len()),
            STREAM_OBJECT => (*start, n.data.len()),
            _ => (0, 0),
        };
        dir.extend_from_slice(&start.to_le_bytes());
        dir.extend_from_slice(&u64::from(id(size)?).to_le_bytes());
    }
    while dir.len() < dir_sectors * SECTOR {
        dir.extend_from_slice(&[0; 68]);
        for _ in 0..3 {
            dir.extend_from_slice(&NO_STREAM.to_le_bytes());
        }
        dir.extend_from_slice(&[0; 48]);
    }

    let mut out = Vec::with_capacity((1 + fat_sectors + difat_sectors + body) * SECTOR);
    out.extend_from_slice(&SIGNATURE);
    out.extend_from_slice(&[0; 16]); // CLSID
    for v in [0x3E, 3, 0xFFFE, 9, 6] {
        // minor and major version, byte order, sector and mini sector shifts
        out.extend_from_slice(&u16::to_le_bytes(v));
    }
    out.extend_from_slice(&[0; 6]);
    let first_difat = if difat_sectors > 0 { id(fat_sectors)? } else { END_OF_CHAIN };
    // Directory sectors (none in version 3), FAT sectors, first directory sector, transaction
    // signature, mini stream cutoff, mini FAT, DIFAT.
    for v in [0, id(fat_sectors)?, chain(0), 0, id(MINI_CUTOFF)?, chain(1), id(counts.get(1).copied().unwrap_or(0))?, first_difat, id(difat_sectors)?]
    {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let fat_ids = (0..fat_sectors).map(id).collect::<Result<Vec<u32>, String>>()?;
    let (head, rest) = fat_ids.split_at(fat_ids.len().min(HEADER_DIFAT));
    for k in 0..HEADER_DIFAT {
        out.extend_from_slice(&head.get(k).copied().unwrap_or(FREE_SECT).to_le_bytes());
    }
    for v in &fat {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for (d, ids) in rest.chunks(127).enumerate() {
        for k in 0..127 {
            out.extend_from_slice(&ids.get(k).copied().unwrap_or(FREE_SECT).to_le_bytes());
        }
        let next = if d + 1 < difat_sectors { id(fat_sectors + d + 1)? } else { END_OF_CHAIN };
        out.extend_from_slice(&next.to_le_bytes());
    }
    let blobs = [dir.as_slice(), &mini_fat, &container].into_iter().chain(large.iter().map(|&i| nodes.get(i).map_or(&[][..], |n| n.data)));
    for blob in blobs {
        out.extend_from_slice(blob);
        out.resize(out.len().div_ceil(SECTOR) * SECTOR, 0);
    }
    Ok(out)
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

    /// The names under storage entry `i`, in tree order (left subtree, node, right subtree).
    fn children(cf: &Compound, i: u32) -> Vec<(String, u8)> {
        fn walk(cf: &Compound, i: u32, out: &mut Vec<(String, u8)>) {
            let Some(e) = cf.entries.get(i as usize) else { return };
            walk(cf, e.left, out);
            out.push((e.name.clone(), e.kind));
            walk(cf, e.right, out);
        }
        let mut out = vec![];
        walk(cf, cf.entries[i as usize].child, &mut out);
        out
    }

    fn index(cf: &Compound, name: &str) -> u32 {
        cf.entries.iter().position(|e| e.name == name).unwrap() as u32
    }

    #[test]
    fn the_writer_nests_storages_and_orders_siblings() {
        let big: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        let streams: [(&str, &[u8]); 7] =
            [("Zeta", b"z"), ("dir/x", b"22"), ("Big", &big), ("dir/sub/y", b"333"), ("Empty", b""), ("ab", b"a"), ("dir/AA", b"4")];
        let file = write(&streams).unwrap();
        let cf = Compound::open(&file).unwrap();
        assert_eq!(cf.root_stream("Big").unwrap().unwrap(), big);
        assert_eq!(cf.root_stream("Zeta").unwrap().unwrap(), b"z");
        assert_eq!(cf.root_stream("Empty").unwrap().unwrap(), b"");
        // Siblings sorted as [MS-CFB] 2.6.4 says: shorter names first, then case-insensitively.
        let names = |i| children(&cf, i).into_iter().map(|(n, k)| format!("{n}:{k}")).collect::<Vec<_>>();
        assert_eq!(names(0), ["ab:2", "Big:2", "dir:1", "Zeta:2", "Empty:2"]);
        assert_eq!(names(index(&cf, "dir")), ["x:2", "AA:2", "sub:1"]);
        assert_eq!(names(index(&cf, "sub")), ["y:2"]);
        assert!(write(&[("a", b"1"), ("A", b"2")]).is_err(), "a name used twice");
        assert!(write(&[("a/", b"1")]).is_err(), "an empty name");
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
}
