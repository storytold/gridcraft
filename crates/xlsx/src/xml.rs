//! A small XML tree built on `quick-xml`, with streaming of repeated elements (shared strings,
//! Python scripts) so large parts never need a full tree, plus escaping helpers for writing.

use std::fmt::Write as _;

use gridcraft_core::MAX_CELL_CHARS;
use gridcraft_model::{PhoneticProps, PhoneticRun, PhoneticString};
use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::IoError;

/// Maximum element nesting accepted in any part.
pub const MAX_DEPTH: usize = 256;

/// Most elements kept in one part's parsed XML tree: an emptied `El` costs well over a hundred
/// bytes, so a highly-compressible part of endless empty elements could fit the package byte
/// limit and still take gigabytes of tree before any semantic check ran. Parts whose big runs
/// matter (shared strings, Python scripts) are streamed instead and never hold this tree.
pub const MAX_XML_NODES: usize = 1_000_000;

/// An element: local name (namespace prefix dropped), attributes by local name, children and
/// its direct text content.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<El>,
    pub text: String,
    /// Non-zero on a streamed element when the parser dropped something inside it at the element
    /// cap, other than a direct child of a name the caller doesn't list (nothing the caller reads):
    /// what it hands over is then not all the part held. An element inside it that lost part of
    /// what it held is dropped whole, so each child handed over is complete, and the streamed
    /// element has the bit of that child's name set ([`El::lost`]): bit `i` for the caller's
    /// listed name at place `i`, counted from 0 (the names at place 31 and on share the last bit).
    pub cut: u32,
}

/// The bit in [`El::cut`] of the caller's listed child name at place `i`.
fn listed_bit(i: usize) -> u32 {
    1 << i.min(31)
}

impl El {
    /// Whether this streamed element lost, at the element cap, a child named `name`, one of the
    /// `children` it was streamed with ([`Streaming::children`]).
    pub fn lost(&self, children: &[(&str, usize)], name: &str) -> bool {
        self.cut != 0 && children.iter().position(|(n, _)| *n == name).is_some_and(|i| self.cut & listed_bit(i) != 0)
    }
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn attr_u32(&self, k: &str) -> Option<u32> {
        self.attr(k).and_then(|v| v.trim().parse::<u32>().ok())
    }
    pub fn attr_i64(&self, k: &str) -> Option<i64> {
        self.attr(k).and_then(|v| v.trim().parse::<i64>().ok())
    }
    pub fn attr_f64(&self, k: &str) -> Option<f64> {
        self.attr(k).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite())
    }
    /// xsd:boolean (`1`/`true`/`0`/`false`).
    pub fn attr_bool(&self, k: &str) -> Option<bool> {
        match self.attr(k)?.trim() {
            "1" | "true" | "on" | "t" => Some(true),
            "0" | "false" | "off" | "f" => Some(false),
            _ => None,
        }
    }
    pub fn flag(&self, k: &str, default: bool) -> bool {
        self.attr_bool(k).unwrap_or(default)
    }
    pub fn child(&self, name: &str) -> Option<&El> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Follows a path of child names.
    pub fn path(&self, names: &[&str]) -> Option<&El> {
        let mut e = self;
        for n in names {
            e = e.child(n)?;
        }
        Some(e)
    }
    /// `val` attribute of a child (the common `<c:x val=".."/>` pattern).
    pub fn child_val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.attr("val"))
    }
}

/// Text of a SpreadsheetML string item (`<si>`, `<is>`, comment `<text>`): `<t>` and rich text
/// runs `<r><t>`, phonetic runs ignored (read them with [`phonetic_data`] instead — they are hints,
/// not part of the text). Decodes `_xHHHH_` escapes.
pub fn rich_text(e: &El) -> String {
    let mut s = String::new();
    for c in &e.children {
        match c.name.as_str() {
            "t" => s.push_str(&c.text),
            "r" => {
                for t in c.kids("t") {
                    s.push_str(&t.text);
                }
            }
            _ => {}
        }
    }
    decode_xstring(&s)
}

/// The phonetic (furigana) data of a string item: its `<rPh>` runs in document order, and its
/// `<phoneticPr>` when present.
///
/// `base` is the item's base text ([`rich_text`]); the offsets are validated against it and never
/// used to index anything. Behaviour reference: ISO/IEC 29500-1 §18.4.6 `rPh` (`sb` and `eb` are
/// zero-based offsets into the base text with `sb < eb`, both within its length; child `<t>` holds
/// the reading) and §18.4.7 `phoneticPr` — the `PhoneticRun` and `PhoneticProperties` classes on
/// Microsoft Learn:
/// <https://learn.microsoft.com/en-us/previous-versions/office/developer/office-2010/cc820083(v=office.14)>,
/// <https://learn.microsoft.com/en-us/previous-versions/office/developer/office-2010/cc797016(v=office.14)>.
/// Damaged runs are dropped rather than repaired: one without both offsets, with `sb >= eb`, wholly
/// past the end of the base text, or without a reading; the caller reports how many.
pub fn phonetic_data(e: &El, base: &str) -> (Vec<PhoneticRun>, Option<PhoneticProps>) {
    // The base text's length in UTF-16 code units: the widest reading of "the total length of the
    // base text" (§18.4.6), so a run is only dropped when it cannot refer to the text at all.
    let len = u32::try_from(base.encode_utf16().count()).unwrap_or(u32::MAX);
    let mut runs = Vec::new();
    for r in e.kids("rPh") {
        let (Some(sb), Some(eb)) = (r.attr_u32("sb"), r.attr_u32("eb")) else { continue };
        let start = sb.min(len);
        let end = eb.min(len);
        if start >= end {
            continue;
        }
        let reading = r.child("t").map(|t| decode_xstring(&t.text)).unwrap_or_default();
        if reading.is_empty() {
            continue;
        }
        runs.push(PhoneticRun { start, end, text: reading });
    }
    (runs, e.child("phoneticPr").map(phonetic_props))
}

/// `<phoneticPr>` attributes (ISO/IEC 29500-1 §18.4.7 `CT_PhoneticPr`: `fontId` is required,
/// `type`, `alignment`). `type` and `alignment` are kept only when they are values §18.18.57
/// `ST_PhoneticType` / §18.18.56 `ST_PhoneticAlignment` define, so a damaged file cannot make the
/// saved file invalid; a missing or unusable `fontId` becomes 0, the Normal style's font, which an
/// out-of-range index falls back to anyway.
/// `ST_PhoneticType` (ISO/IEC 29500-1 §18.18.57), as `phoneticPr/@type` may write them.
const PHONETIC_KINDS: &[&str] = &["halfwidthKatakana", "fullwidthKatakana", "Hiragana", "noConversion"];
/// `ST_PhoneticAlignment` (ISO/IEC 29500-1 §18.18.56), as `phoneticPr/@alignment` may write them.
const PHONETIC_ALIGNMENTS: &[&str] = &["noControl", "left", "center", "distributed"];

fn phonetic_props(e: &El) -> PhoneticProps {
    const KINDS: &[&str] = PHONETIC_KINDS;
    const ALIGNMENTS: &[&str] = PHONETIC_ALIGNMENTS;
    PhoneticProps {
        font_id: e.attr_u32("fontId").unwrap_or(0),
        kind: e.attr("type").map(str::trim).filter(|v| KINDS.contains(v)).map(str::to_string),
        alignment: e.attr("alignment").map(str::trim).filter(|v| ALIGNMENTS.contains(v)).map(str::to_string),
    }
}

/// Converts input bytes to UTF-8 XML text (handles BOMs and UTF-16).
pub(crate) fn to_utf8(xml: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if let Some(rest) = xml.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return std::borrow::Cow::Borrowed(rest);
    }
    let utf16 = |le: bool, b: &[u8]| -> Vec<u8> {
        let units = b.as_chunks::<2>().0.iter().map(|c| if le { u16::from_le_bytes(*c) } else { u16::from_be_bytes(*c) });
        let s: String = char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect();
        // The declaration may claim UTF-16; quick-xml without the encoding feature ignores it.
        s.into_bytes()
    };
    if let Some(rest) = xml.strip_prefix(&[0xFF, 0xFE]) {
        return std::borrow::Cow::Owned(utf16(true, rest));
    }
    if let Some(rest) = xml.strip_prefix(&[0xFE, 0xFF]) {
        return std::borrow::Cow::Owned(utf16(false, rest));
    }
    // UTF-16 without BOM: `<\0?\0`.
    if xml.len() >= 4 && xml[1] == 0 && xml[3] == 0 && xml[0] != 0 {
        return std::borrow::Cow::Owned(utf16(true, xml));
    }
    if xml.len() >= 4 && xml[0] == 0 && xml[2] == 0 && xml[1] != 0 {
        return std::borrow::Cow::Owned(utf16(false, xml));
    }
    std::borrow::Cow::Borrowed(xml)
}

fn local(name: &[u8]) -> String {
    let n = match name.iter().rposition(|&b| b == b':') {
        Some(i) => name.get(i + 1..).unwrap_or(name),
        None => name,
    };
    String::from_utf8_lossy(n).into_owned()
}

fn start_el(e: &BytesStart<'_>) -> El {
    let mut el = El { name: local(e.name().as_ref()), ..Default::default() };
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key.as_ref();
        // Namespace declarations carry no data for us.
        if key == b"xmlns" || key.starts_with(b"xmlns:") {
            continue;
        }
        let v = match a.normalized_value(XmlVersion::Explicit1_0) {
            Ok(v) => v.into_owned(),
            Err(_) => String::from_utf8_lossy(&a.value).into_owned(),
        };
        el.attrs.push((local(key), v));
    }
    el
}

fn named_entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        _ => return None,
    })
}

/// Parses a whole part into a tree.
pub fn parse(xml: &[u8]) -> Result<El, IoError> {
    parse_streaming(xml, Streaming { split: &[], children: &[] }, &mut |_| Ok(())).map(|s| s.root).map_err(|f| f.error)
}

/// The first element named `local_name` (namespace prefix dropped) in `xml`, read from its start tag
/// alone: no children, and nothing after the tag is read, so unbalanced markup around it (a VML
/// caption's unclosed `<br>`) neither hides nor moves it. `None` when there is none, or when the
/// markup before it is malformed.
pub fn first_start(xml: &[u8], local_name: &str) -> Option<El> {
    let mut reader = Reader::from_reader(xml);
    {
        let cfg = reader.config_mut();
        cfg.check_end_names = false;
        cfg.allow_unmatched_ends = true;
        cfg.allow_dangling_amp = true;
    }
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf).ok()? {
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == local_name => return Some(start_el(&e)),
            Event::Eof => return None,
            _ => {}
        }
        buf.clear();
    }
}

/// How a part is streamed: complete elements named in `split` that are children or grandchildren of
/// the root (`sst/si`, `python/pythonScripts/pythonScript`; see [`is_streamed`]) are handed to the
/// callback instead of being kept in the tree, and each of them holds no more direct children of a
/// name than `children` allows for it. The caller lists every child the schema allows the element, each with
/// its own allowance (`("phoneticPr", 1)`: the one phonetic properties element a shared string may
/// carry; `("code", 1)`: the one code a Python script holds), so a name that is not listed is one
/// the schema does not know: such a name is admitted once, and no more than
/// [`MAX_UNLISTED_CHILD_NAMES`] of them at all. So a crafted part cannot grow one streamed element
/// without bound through any name, nor make the counting itself slow, nor use up a listed name's
/// allowance with another name (an unknown child whose own descendants fill the element cap can
/// still leave no room for what follows it): a child past its allowance is kept out of the tree
/// once parsed, with its subtree
/// (which holds nothing streamed: streamed elements sit at depth 1 or 2), and counted in
/// [`Streamed::dropped`]. What a streamed element holds at any depth is bounded by
/// [`MAX_XML_NODES`]: an element that would go past it is dropped and counted the same way, and
/// so is each element holding it up to the streamed element's own child, which is lost whole.
/// Dropping an element gives what it held back to the count, so the streamed element's later
/// children are kept while they fit under the cap, and the rest of the part is still read.
#[derive(Clone, Copy)]
pub struct Streaming<'a> {
    pub split: &'a [&'a str],
    pub children: &'a [(&'a str, usize)],
}

/// Most distinct child names not listed in [`Streaming::children`] one streamed element may hold.
/// Every child the schema allows is listed by its caller, so an unlisted name is one the schema does
/// not know (an extension a future schema might add, or an invention); a few are tolerated once
/// each, and refusing the rest keeps the per-name count a scan of a handful of entries instead of
/// one that grows with every invented name. A listed name is never refused for the names an
/// element already holds, only for its own allowance or the element cap.
pub const MAX_UNLISTED_CHILD_NAMES: usize = 4;

/// Whether cell text is longer than Excel's limit of characters per cell ([`MAX_CELL_CHARS`]),
/// counted in UTF-16 units (a character outside the Basic Multilingual Plane counts as two). A
/// file Excel wrote never is; one that is gets kept whole, since cutting it would lose the user's
/// data at the next save, and the caller warns about it.
pub fn exceeds_cell_text(text: &str) -> bool {
    let limit = MAX_CELL_CHARS as usize;
    // A character takes at least as many UTF-8 bytes as UTF-16 units and at most three bytes per
    // unit (three bytes are one unit, four bytes two), so only a text between those lengths in
    // bytes needs counting.
    if text.len() <= limit {
        return false;
    }
    if text.len() > limit.saturating_mul(3) {
        return true;
    }
    utf16_units(text) > limit
}

/// The number of UTF-16 units in `text`, counted from its bytes: each byte is one unit, except a
/// continuation byte (`10xxxxxx`), which adds none, and the first byte of a four-byte character
/// (`11110xxx`), the only one taking two units, which adds two. Counting in blocks into
/// byte-sized counters (a block is shorter than 256 bytes, so they cannot overflow) lets the
/// compiler count many bytes per instruction.
#[inline(never)]
fn utf16_units(text: &str) -> usize {
    let (mut continuations, mut four_byte) = (0usize, 0usize);
    for block in text.as_bytes().chunks(192) {
        let (mut block_continuations, mut block_four_byte) = (0u8, 0u8);
        for &byte in block {
            block_continuations += u8::from(byte & 0xC0 == 0x80);
            block_four_byte += u8::from(byte >= 0xF0);
        }
        continuations = continuations.saturating_add(usize::from(block_continuations));
        four_byte = four_byte.saturating_add(usize::from(block_four_byte));
    }
    text.len().saturating_sub(continuations).saturating_add(four_byte)
}

/// What a streamed parse leaves behind: the tree without the streamed elements, how many
/// elements inside streamed elements were dropped, for exceeding [`Streaming::children`] or
/// [`MAX_XML_NODES`], and how many children of a name not listed there the streamed elements
/// handed out held. The caller turns non-zero counts into import warnings.
pub struct Streamed {
    pub root: El,
    pub dropped: u64,
    pub unlisted: u64,
}

/// A streamed parse that failed part-way, with what it had dropped until then, so the caller can
/// still say so.
#[derive(Debug)]
pub struct StreamFailure {
    pub error: IoError,
    pub dropped: u64,
    pub unlisted: u64,
}

/// The parser's counts while it streams a part.
#[derive(Default)]
struct Tally {
    // Elements kept in the tree so far, so a pathological part that passes the byte limits can
    // still be refused before the tree itself outgrows memory. Elements inside a `split` element
    // are handed out with it and do not stay in the tree, so a shared-string table or Python part
    // of any size passes: `kept_at_start` remembers, for each open element, the count when it
    // began and, for a streamed one ([`is_streamed`], tested once, at its start tag), whether its
    // name counts are open (`Some(true)`) in `child_counts`.
    kept: usize,
    kept_at_start: Vec<(usize, Option<bool>)>,
    // Elements inside streamed elements dropped (for exceeding their name's allowance, or the
    // element cap), and the counts by name of the open streamed elements that have met a name that
    // needs one (innermost last): a string of text and runs alone never opens a count list.
    dropped: u64,
    child_counts: Vec<Vec<(Slot, usize)>>,
    // Children of unlisted names the closed streamed elements held, from their counts.
    unlisted: u64,
    /// Emptied count lists, reused for the next streamed element instead of allocated per element.
    spare: Vec<Vec<(Slot, usize)>>,
}

impl Tally {
    /// Puts away the name counts of the streamed element that has just closed, for the next one.
    /// Its unlisted children are added up first: an unlisted name is admitted once, and its entry
    /// is removed when the child is then dropped at the element cap, so the counts add up to the
    /// unlisted children the element holds.
    fn close_streamed(&mut self) {
        if let Some(mut counts) = self.child_counts.pop() {
            let held = counts.iter().filter(|(slot, _)| matches!(slot, Slot::Unlisted(_))).map(|(_, n)| *n as u64).sum::<u64>();
            self.unlisted = self.unlisted.saturating_add(held);
            counts.clear();
            self.spare.push(counts);
        }
    }
}

/// A child name a streamed element has met: one listed in [`Streaming::children`], by its place
/// in the list, or one not listed, by name.
enum Slot {
    Listed(usize),
    Unlisted(String),
}

/// Parses a part, streaming the elements `how` names to `cb`.
pub fn parse_streaming(xml: &[u8], how: Streaming<'_>, cb: &mut dyn FnMut(El) -> Result<(), IoError>) -> Result<Streamed, StreamFailure> {
    let mut tally = Tally::default();
    match stream(xml, how, cb, &mut tally) {
        Ok(root) => Ok(Streamed { root, dropped: tally.dropped, unlisted: tally.unlisted }),
        Err(error) => Err(StreamFailure { error, dropped: tally.dropped, unlisted: tally.unlisted }),
    }
}

/// Whether an element named `name` with `ancestors` elements above it is one `how` streams: a child or
/// grandchild of the root (`sst/si`, `python/pythonScripts/pythonScript`) named in `split`. A deeper
/// element of the same name is an ordinary element of the tree.
fn is_streamed(how: Streaming<'_>, ancestors: usize, name: &str) -> bool {
    // Names are compared byte by byte: they are a few bytes long, and this runs for every element.
    (1..=2).contains(&ancestors) && how.split.iter().any(|s| s.bytes().eq(name.bytes()))
}

fn stream(xml: &[u8], how: Streaming<'_>, cb: &mut dyn FnMut(El) -> Result<(), IoError>, tally: &mut Tally) -> Result<El, IoError> {
    let data = to_utf8(xml);
    let mut reader = Reader::from_reader(data.as_ref());
    {
        let cfg = reader.config_mut();
        cfg.check_end_names = false;
        cfg.allow_unmatched_ends = true;
        cfg.allow_dangling_amp = true;
        cfg.trim_text_start = false;
        cfg.trim_text_end = false;
    }
    let mut stack: Vec<El> = Vec::new();
    let mut root: Option<El> = None;
    let mut buf = Vec::new();
    // Attach a finished element to its parent (or emit it / make it the root).
    fn finish(
        stack: &mut [El],
        root: &mut Option<El>,
        el: El,
        how: Streaming<'_>,
        cb: &mut dyn FnMut(El) -> Result<(), IoError>,
        tally: &mut Tally,
        opened: Option<(usize, Option<bool>)>,
    ) -> Result<(), IoError> {
        let children = how.children;
        // An element that had a start tag carries what was noted then; an empty one is tested here.
        let (kept_at_start, streamed) = match opened {
            Some((k, streamed)) => (Some(k), streamed.is_some()),
            None => (None, is_streamed(how, stack.len(), &el.name)),
        };
        // Dropping an element releases what it held: the count goes back to where it began.
        let release = |tally: &mut Tally| {
            if let Some(k) = kept_at_start {
                tally.kept = k;
            }
        };
        // A dropped element is counted with everything it held, and releases it.
        let drop_whole = |tally: &mut Tally| {
            let held = kept_at_start.map_or(0, |k| tally.kept.saturating_sub(k));
            tally.dropped = tally.dropped.saturating_add(1).saturating_add(held as u64);
            release(tally);
        };
        match stack.last_mut() {
            Some(parent) => {
                // A direct child of the streamed element, counted against its name's allowance, and
                // its name's place in the caller's list, looked up once (byte by byte, as in
                // `is_streamed`).
                let direct = tally.kept_at_start.last().is_some_and(|&(_, counted)| counted.is_some());
                let listed = if direct && !streamed { children.iter().position(|(n, _)| n.bytes().eq(el.name.bytes())) } else { None };
                if streamed {
                    cb(el)?;
                    // The element and everything parsed inside it have left the tree.
                    release(tally);
                } else if direct && !admit(tally, children, listed, &el.name) {
                    // The streamed element already holds all a legitimate one can of this name:
                    // the child is dropped with its subtree (which holds nothing streamed), all of
                    // it counted.
                    drop_whole(tally);
                } else if (tally.kept >= MAX_XML_NODES || el.cut != 0)
                    && let Some(&(began, _)) = tally.kept_at_start.iter().take(3).rev().find(|(_, streamed)| streamed.is_some())
                {
                    // Inside a streamed element (they sit at depth 1 or 2). If the cap was already
                    // reached before the innermost open one (the one this element sits in) began,
                    // nothing in it can be kept, so the part is refused, as one that streams nothing is.
                    if began >= MAX_XML_NODES {
                        return Err(IoError::Xml(format!("the part holds more than {MAX_XML_NODES} elements")));
                    }
                    // Inside a streamed element that has filled the element cap, or an element that
                    // lost part of what it held to the cap: this element is dropped with what it
                    // holds, all of it counted, and the rest of the part is still read. Its parent
                    // is marked, so whatever held it goes whole too, up to the streamed element,
                    // which is handed out marked as cut, with the bit of its own child's name set.
                    drop_whole(tally);
                    // Read again: admitting this child may have just opened the parent's counts.
                    if tally.kept_at_start.last().is_some_and(|&(_, counted)| counted == Some(true))
                        && let Some(counts) = tally.child_counts.last_mut()
                        && let Some(count) = slot_count(counts, listed, &el.name)
                    {
                        // Dropped at the cap, not for its allowance: the slot it took is free again,
                        // and an unlisted name left holding no child gives back its place among the
                        // names the element may hold.
                        *count = count.saturating_sub(1);
                        if listed.is_none() {
                            counts.retain(|(slot, n)| *n > 0 || matches!(slot, Slot::Listed(_)));
                        }
                    }
                    // A direct child of a name the caller doesn't list is nothing it reads: losing
                    // it doesn't cut the streamed element. A listed one marks its name's bit there.
                    parent.cut |= match (direct, listed) {
                        (false, _) => 1,
                        (true, Some(i)) => listed_bit(i),
                        (true, None) => 0,
                    };
                } else {
                    tally.kept = tally.kept.saturating_add(1);
                    if tally.kept > MAX_XML_NODES {
                        return Err(IoError::Xml(format!("the part holds more than {MAX_XML_NODES} elements")));
                    }
                    parent.children.push(el);
                }
            }
            None => {
                if root.is_none() {
                    *root = Some(el);
                }
            }
        }
        Ok(())
    }
    // Counts one more child of `name` (`listed`: its place in the caller's list) for its parent,
    // the innermost open streamed element; `false` when the element already holds all the
    // allowance for that name (one, for a name not listed), or already holds as many unlisted
    // names as a legitimate element can.
    fn admit(tally: &mut Tally, children: &[(&str, usize)], listed: Option<usize>, name: &str) -> bool {
        let allowance = listed.and_then(|i| children.get(i)).map_or(1, |(_, a)| *a);
        // A name without a limit is never refused, so it needs no count.
        if allowance == usize::MAX {
            return true;
        }
        // The first child that needs a count opens the element's count list.
        let Some((_, Some(counted))) = tally.kept_at_start.last_mut() else { return true };
        if !*counted {
            *counted = true;
            let counts = tally.spare.pop().unwrap_or_default();
            tally.child_counts.push(counts);
        }
        let Some(counts) = tally.child_counts.last_mut() else { return true };
        if let Some(count) = slot_count(counts, listed, name) {
            if *count >= allowance {
                return false;
            }
            *count += 1;
            return true;
        }
        // A name this element has not seen yet: a listed one gets its entry (unless its allowance
        // is zero); an unlisted one only while the element has room for another unknown name.
        let unlisted_names = counts.iter().filter(|(slot, _)| matches!(slot, Slot::Unlisted(_))).count();
        if allowance == 0 || (listed.is_none() && unlisted_names >= MAX_UNLISTED_CHILD_NAMES) {
            return false;
        }
        let slot = match listed {
            Some(i) => Slot::Listed(i),
            None => Slot::Unlisted(name.to_string()),
        };
        counts.push((slot, 1));
        true
    }
    // The count of the slot `name` takes (`listed`: its place in the caller's list), if it has one.
    fn slot_count<'a>(counts: &'a mut [(Slot, usize)], listed: Option<usize>, name: &str) -> Option<&'a mut usize> {
        let (_, count) = counts.iter_mut().find(|(slot, _)| match (slot, listed) {
            (Slot::Listed(i), Some(j)) => *i == j,
            (Slot::Unlisted(n), None) => n == name,
            _ => false,
        })?;
        Some(count)
    }
    loop {
        let ev = reader.read_event_into(&mut buf).map_err(|e| IoError::Xml(format!("{e} at byte {}", reader.buffer_position())))?;
        match ev {
            Event::Start(e) => {
                if stack.len() >= MAX_DEPTH {
                    return Err(IoError::Xml("XML nested too deeply".into()));
                }
                let el = start_el(&e);
                let streamed = is_streamed(how, stack.len(), &el.name).then_some(false);
                stack.push(el);
                tally.kept_at_start.push((tally.kept, streamed));
            }
            Event::Empty(e) => {
                let el = start_el(&e);
                finish(&mut stack, &mut root, el, how, cb, tally, None)?;
            }
            Event::End(_) => {
                if let Some(el) = stack.pop() {
                    let opened = tally.kept_at_start.pop();
                    if let Some((_, Some(true))) = opened {
                        tally.close_streamed();
                    }
                    finish(&mut stack, &mut root, el, how, cb, tally, opened)?;
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    let s = t.xml_content(XmlVersion::Explicit1_0).map_err(|e| IoError::Xml(e.to_string()))?;
                    top.text.push_str(&s);
                }
            }
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::GeneralRef(r) => {
                if let Some(top) = stack.last_mut() {
                    match r.resolve_char_ref() {
                        Ok(Some(c)) => top.text.push(c),
                        _ => {
                            let name = String::from_utf8_lossy(&r).into_owned();
                            match named_entity(&name) {
                                Some(c) => top.text.push(c),
                                None => {
                                    top.text.push('&');
                                    top.text.push_str(&name);
                                    top.text.push(';');
                                }
                            }
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    // Unclosed elements (truncated part): close them so we keep what we have.
    while let Some(el) = stack.pop() {
        let opened = tally.kept_at_start.pop();
        if let Some((_, Some(true))) = opened {
            tally.close_streamed();
        }
        finish(&mut stack, &mut root, el, how, cb, tally, opened)?;
    }
    root.ok_or_else(|| IoError::Xml("no root element".into()))
}

// ---------------------------------------------------------------- writing helpers

// XML escaping and SpreadsheetML strings are shared; phonetic/model mapping stays here.
pub use craft_xml::{esc, esc_attr};
#[cfg(test)]
use craft_xml::{is_xml_char, push_esc};

/// Appends `n` in decimal.
pub fn push_u64(o: &mut String, mut n: u64) {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    loop {
        i -= 1;
        if let Some(d) = buf.get_mut(i) {
            *d = b'0' + (n % 10) as u8;
        }
        n /= 10;
        if n == 0 || i == 0 {
            break;
        }
    }
    for &d in buf.get(i..).unwrap_or_default() {
        o.push(char::from(d));
    }
}

/// Appends the `A1`-style text of `c` ([`gridcraft_core::CellRef::a1`]).
#[cfg(test)]
fn push_a1(o: &mut String, c: gridcraft_core::CellRef) {
    let mut letters = [0u8; 8];
    let mut i = letters.len();
    let mut n = u64::from(c.col) + 1;
    while n > 0 && i > 0 {
        i -= 1;
        if let Some(l) = letters.get_mut(i) {
            *l = b'A' + ((n - 1) % 26) as u8;
        }
        n = (n - 1) / 26;
    }
    for &l in letters.get(i..).unwrap_or_default() {
        o.push(char::from(l));
    }
    push_u64(o, u64::from(c.row) + 1);
}

// Existing call sites and regression coverage use the production shared codec.
pub use craft_xlsx::{decode_xstring, encode_xstring};

/// `<t>` element with `xml:space="preserve"` when needed.
pub fn t_el(s: &str) -> String {
    let enc = esc(&encode_xstring(s));
    if s.starts_with([' ', '\t', '\n', '\r']) || s.ends_with([' ', '\t', '\n', '\r']) || s.contains('\n') {
        format!("<t xml:space=\"preserve\">{enc}</t>")
    } else {
        format!("<t>{enc}</t>")
    }
}

/// The contents of a shared string item (`<si>`): the base text, then — when the string has
/// phonetic (furigana) data — its `<rPh sb="…" eb="…"><t>…</t></rPh>` runs in document order and
/// its `<phoneticPr …/>`. Behaviour reference: ISO/IEC 29500-1 §18.4.8 `si` with §18.4.6 `rPh` and
/// §18.4.7 `phoneticPr`. A string without phonetic data gives exactly [`t_el`]'s output.
///
/// The runs and the properties are independent: `<phoneticPr>` is a valid child of `<si>` on its
/// own (§18.4.8's `CT_SharedStringItem` allows it without any `rPh`), so a string whose runs were
/// all dropped still keeps its settings.
pub fn si_body(text: &str, phonetic: Option<&PhoneticString>) -> String {
    let mut s = t_el(text);
    let Some(p) = phonetic else { return s };
    // The model is public and restored from GridCraft JSON too, so the reader's checks are not the
    // only way data gets here: validate again and write only what the schema allows (§18.4.6: an
    // `sb` before `eb`, both inside the base text, a non-empty reading; §18.18.56/57: a known enum
    // value). Damaged data is dropped, never written back invalid.
    let len = u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
    for r in &p.runs {
        let start = r.start.min(len);
        let end = r.end.min(len);
        if start >= end || r.text.is_empty() {
            continue;
        }
        let _ = write!(s, "<rPh sb=\"{start}\" eb=\"{end}\">{}</rPh>", t_el(&r.text));
    }
    if let Some(pr) = &p.props {
        let _ = write!(s, "<phoneticPr fontId=\"{}\"", pr.font_id);
        if let Some(kind) = pr.kind.as_deref().filter(|kind| PHONETIC_KINDS.contains(kind)) {
            let _ = write!(s, " type=\"{}\"", esc_attr(kind));
        }
        if let Some(alignment) = pr.alignment.as_deref().filter(|a| PHONETIC_ALIGNMENTS.contains(a)) {
            let _ = write!(s, " alignment=\"{}\"", esc_attr(alignment));
        }
        s.push_str("/>");
    }
    s
}

/// Formats a number for an XML attribute/value (shortest round-trip form).
pub fn num(n: f64) -> String {
    let mut s = String::new();
    push_num(&mut s, n);
    s
}

/// Appends [`num`]`(n)` to `o`.
pub fn push_num(o: &mut String, n: f64) {
    if !n.is_finite() {
        o.push('0');
        return;
    }
    if n == n.trunc() && n.abs() < 1e15 {
        let i = n as i64;
        if i < 0 {
            o.push('-');
        }
        push_u64(o, i.unsigned_abs());
        return;
    }
    let start = o.len();
    let _ = write!(o, "{n:?}");
    if let Some(e) = o.get(start..).and_then(|t| t.find('e')) {
        o.replace_range(start + e..start + e + 1, "E");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_text_limit_counts_utf16_units_not_bytes() {
        let limit = MAX_CELL_CHARS as usize;
        // Three bytes and one UTF-16 unit each: past the limit in bytes, well within it in units.
        let euros = "€".repeat(20_000);
        assert!(!exceeds_cell_text(&euros), "20,000 euro signs are 20,000 units");
        let accents = "é".repeat(limit + 1);
        assert!(exceeds_cell_text(&accents), "32,768 two-byte characters exceed");
        let more_euros = "€".repeat(limit + 1);
        assert!(exceeds_cell_text(&more_euros), "32,768 three-byte characters exceed");
        // Four bytes and two UTF-16 units each.
        let emoji = "😀".repeat(16_384);
        assert!(exceeds_cell_text(&emoji), "16,384 emoji are 32,768 units");
        let emoji_and_letter = format!("{}x", "😀".repeat(16_383));
        assert!(!exceeds_cell_text(&emoji_and_letter), "16,383 emoji and a letter are 32,767 units");
    }

    #[test]
    fn cell_text_limit_matches_utf16_count_at_the_boundaries() {
        let limit = MAX_CELL_CHARS as usize;
        let chars = ["a", "é", "€", "😀"];
        let mut checked = 0;
        for body in chars {
            for tail in chars {
                let (body_units, tail_units) = (body.encode_utf16().count(), tail.encode_utf16().count());
                // Texts of `body` characters then up to three `tail` ones, whose units land on
                // the limit, one either side of it and near the byte bounds.
                for tails in 0..4 {
                    let tail_total = tails * tail_units;
                    for target in [limit - 1, limit, limit + 1, limit + 2] {
                        let Some(rest) = target.checked_sub(tail_total) else { continue };
                        let count = rest / body_units;
                        for n in [count.saturating_sub(1), count, count + 1] {
                            let text = format!("{}{}", body.repeat(n), tail.repeat(tails));
                            let expected = text.encode_utf16().count() > limit;
                            assert_eq!(exceeds_cell_text(&text), expected, "{n} of {body:?} then {tails} of {tail:?}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        // The byte bounds themselves: exactly at the limit in bytes, and three bytes per unit.
        for text in [
            "a".repeat(limit),
            "a".repeat(limit + 1),
            "€".repeat(limit),
            format!("{}a", "€".repeat(limit)),
            format!("{}é", "€".repeat(limit - 1)),
            format!("{}😀", "€".repeat(limit - 1)),
            String::new(),
        ] {
            assert_eq!(exceeds_cell_text(&text), text.encode_utf16().count() > limit);
            checked += 1;
        }
        assert!(checked > 500);
    }

    #[test]
    fn si_without_phonetics_is_written_exactly_as_before() {
        for s in ["plain", " leading ", "a & <b>", "line\nbreak", "日本語", "x_x0041_"] {
            assert_eq!(si_body(s, None), t_el(s));
        }
    }

    #[test]
    fn a_streamed_part_may_hold_more_elements_than_the_cap() {
        // 5001 streamed elements of 100 children each is 1,000,200 `c` and `v` elements, more than
        // the cap, but the streamed elements are handed out one by one and never pile up in the
        // tree, so the part is read whole. Kept in a tree (no split), the same part is refused.
        let row = format!("<row>{}</row>", "<c><v>1</v></c>".repeat(100));
        let xml = format!("<sheetData>{}</sheetData>", row.repeat(5001));
        let mut rows = 0usize;
        let streamed = parse_streaming(xml.as_bytes(), Streaming { split: &["row"], children: &[("c", usize::MAX)] }, &mut |r| {
            assert_eq!(r.children.len(), 100);
            rows += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(rows, 5001);
        assert!(streamed.root.children.is_empty());
        assert_eq!(streamed.dropped, 0);
        let err = parse(xml.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("elements"), "{err}");
    }

    #[test]
    fn a_streamed_element_keeps_exactly_its_limit_of_children_and_drops_the_rest() {
        // Exactly the limit: nothing dropped, every child kept. One more: the extra child is
        // dropped and counted, the element keeps exactly the limit, and the parse goes on with the
        // next streamed element untouched.
        let how = Streaming { split: &["row"], children: &[("c", 3)] };
        let full = b"<sheetData><row><c>1</c><c>2</c><c>3</c></row><row><c>4</c></row></sheetData>";
        let mut seen = Vec::new();
        let s = parse_streaming(full, how, &mut |r| {
            seen.push(r.children.iter().map(|c| c.text.clone()).collect::<Vec<_>>());
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, [vec!["1", "2", "3"], vec!["4"]]);
        assert_eq!(s.dropped, 0);
        let over = b"<sheetData><row><c>1</c><c>2</c><c>3</c><c>4</c></row><row><c>5</c><c>6</c><c>7</c><c>8</c><c>9</c></row></sheetData>";
        let mut seen = Vec::new();
        let s = parse_streaming(over, how, &mut |r| {
            seen.push(r.children.iter().map(|c| c.text.clone()).collect::<Vec<_>>());
            Ok(())
        })
        .unwrap();
        assert_eq!(seen, [vec!["1", "2", "3"], vec!["5", "6", "7"]]);
        assert_eq!(s.dropped, 3);
        // The limit is on the streamed element's own children, by name: deeper nesting is not
        // counted, elements outside the streamed ones are not limited at all, and a name not
        // listed is allowed once (the second `extLst` is dropped).
        let deep = b"<sheetData><cols><col/><col/><col/><col/><col/></cols><row><c><f/><v/><is/><x/></c><extLst/><extLst/></row></sheetData>";
        let mut names = Vec::new();
        let s = parse_streaming(deep, how, &mut |r| {
            names = r.children.iter().map(|c| c.name.clone()).collect();
            assert_eq!(r.children[0].children.len(), 4);
            Ok(())
        })
        .unwrap();
        assert_eq!(names, ["c", "extLst"]);
        assert_eq!(s.dropped, 1);
        assert_eq!(s.root.child("cols").map(|c| c.children.len()), Some(5));
    }

    #[test]
    fn a_streamed_element_holds_only_a_handful_of_unlisted_child_names() {
        // Four invented names are admitted once each; the fifth and every later one is dropped
        // and counted. The listed name is still admitted up to its own allowance after a
        // thousand invented ones.
        let mut row = String::from("<row>");
        for i in 0..1000 {
            row.push_str(&format!("<unknown{i}/>"));
        }
        row.push_str("<c/><c/><c/><c/></row>");
        let xml = format!("<sheetData>{row}{row}</sheetData>");
        let mut kept = Vec::new();
        let s = parse_streaming(xml.as_bytes(), Streaming { split: &["row"], children: &[("c", 3)] }, &mut |r| {
            kept.push(r.children.iter().map(|c| c.name.clone()).collect::<Vec<String>>());
            Ok(())
        })
        .unwrap();
        let mut expected: Vec<String> = (0..MAX_UNLISTED_CHILD_NAMES).map(|i| format!("unknown{i}")).collect();
        expected.extend(["c", "c", "c"].map(String::from));
        assert_eq!(kept, [expected.clone(), expected]);
        assert_eq!(s.dropped, 2 * ((1000 - MAX_UNLISTED_CHILD_NAMES as u64) + 1));
    }

    #[test]
    fn an_unlisted_name_dropped_at_the_element_cap_frees_its_place_among_the_names() {
        // Four invented names each hold the element cap's worth of elements and are dropped at it,
        // whole: the first row holds none of them, so a fifth invented name after them is admitted
        // and counted as an unlisted child. A row that keeps four invented names still refuses a
        // fifth.
        let many = "<x/>".repeat(MAX_XML_NODES);
        let crossing: String = (0..MAX_UNLISTED_CHILD_NAMES).map(|i| format!("<crossing{i}>{many}</crossing{i}>")).collect();
        let four: String = (0..MAX_UNLISTED_CHILD_NAMES).map(|i| format!("<kept{i}/>")).collect();
        let xml = format!("<sheetData><row><c/>{crossing}<fifth/></row><row>{four}<fifth/></row></sheetData>");
        let mut kept = Vec::new();
        let s = parse_streaming(xml.as_bytes(), Streaming { split: &["row"], children: &[("c", 1)] }, &mut |r| {
            kept.push(r.children.iter().map(|c| c.name.clone()).collect::<Vec<String>>());
            Ok(())
        })
        .unwrap();
        let second: Vec<String> = (0..MAX_UNLISTED_CHILD_NAMES).map(|i| format!("kept{i}")).collect();
        assert_eq!(kept, [vec!["c".to_string(), "fifth".to_string()], second]);
        assert_eq!(s.unlisted, 1 + MAX_UNLISTED_CHILD_NAMES as u64);
    }

    #[test]
    fn dropped_children_do_not_count_against_the_element_cap() {
        // One streamed element stuffed with more `c` children than the element cap, each with a `v`
        // inside: the excess is dropped as it is parsed, counted with what it holds, and the elements
        // counted while each dropped child was open are counted back out, so the element keeps the
        // ten its allowance lets through, and the `d` after them, instead of reaching the cap and
        // dropping it.
        let xml = format!("<sheetData><row>{}<d/></row></sheetData>", "<c><v>1</v></c>".repeat(MAX_XML_NODES + 1));
        let mut rows = 0;
        let s = parse_streaming(xml.as_bytes(), Streaming { split: &["row"], children: &[("c", 10), ("d", 1)] }, &mut |r| {
            assert_eq!(r.children.len(), 11);
            assert_eq!(r.cut, 0);
            rows += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(rows, 1);
        assert_eq!(s.dropped, 2 * (MAX_XML_NODES as u64 + 1 - 10));
    }

    #[test]
    fn a_limited_child_lost_at_the_cap_frees_its_place_for_a_later_one() {
        // The first `code` is the first child that needs a count, and it crosses the cap: it is
        // dropped and gives its place back, so the second `code` is kept.
        let xml = format!("<p><s><code>{}</code><code>kept</code></s></p>", "<x/>".repeat(MAX_XML_NODES));
        let mut kept = Vec::new();
        parse_streaming(xml.as_bytes(), Streaming { split: &["s"], children: &[("code", 1)] }, &mut |s| {
            kept = s.children.iter().map(|c| c.text.clone()).collect();
            Ok(())
        })
        .unwrap();
        assert_eq!(kept, ["kept"]);
    }

    #[test]
    fn a_streamed_element_says_which_listed_children_it_lost_at_the_element_cap() {
        // Each string loses one part at the cap: a run whose `t` holds the crossing element (lost
        // through what it held), a phonetic run that crosses it directly, an unlisted child (which
        // cuts nothing), and nothing at all. The names it lost are the ones `lost` reports.
        let many = "<x/>".repeat(MAX_XML_NODES);
        let how = Streaming { split: &["si"], children: &[("t", usize::MAX), ("phoneticPr", 1), ("r", usize::MAX), ("rPh", usize::MAX)] };
        let xml = format!(
            "<sst><si><t>a</t><r><t>{many}</t></r></si><si><t>a</t><rPh>{many}</rPh></si><si><t>a</t><extra>{many}</extra></si><si><t>a</t></si></sst>"
        );
        let mut seen = Vec::new();
        parse_streaming(xml.as_bytes(), how, &mut |si| {
            let lost: Vec<&str> = how.children.iter().map(|(name, _)| *name).filter(|name| si.lost(how.children, name)).collect();
            seen.push((si.cut != 0, lost.join(",")));
            Ok(())
        })
        .unwrap();
        let expected = [(true, "r"), (true, "rPh"), (false, ""), (false, "")];
        assert_eq!(seen, expected.map(|(cut, lost)| (cut, lost.to_string())));
    }

    #[test]
    fn a_part_of_endless_empty_elements_is_refused() {
        assert!(parse(b"<r><x/><y t=\"1\"/><x/></r>").is_ok());
        let xml = format!("<r>{}<y/></r>", "<x/>".repeat(MAX_XML_NODES + 1));
        let err = parse(xml.as_bytes()).unwrap_err();
        assert!(err.to_string().contains("elements"), "{err}");
    }

    #[test]
    fn elements_past_the_cap_after_a_streamed_element_are_refused() {
        // A streamed element's count list closes at its end tag, so elements past the cap outside
        // any streamed element are refused, as in a part that streams nothing, not dropped.
        let xml = format!("<sst><si><t>a</t></si>{}</sst>", "<x/>".repeat(MAX_XML_NODES + 1));
        let mut strings = 0;
        let parsed = parse_streaming(xml.as_bytes(), Streaming { split: &["si"], children: &[("t", 1)] }, &mut |_| {
            strings += 1;
            Ok(())
        });
        let Err(failure) = parsed else { panic!("a part past the element cap is refused") };
        assert!(failure.error.to_string().contains("elements"), "{}", failure.error);
        assert_eq!(strings, 1);
    }

    #[test]
    fn a_part_whose_cap_is_filled_before_its_streamed_elements_is_refused() {
        // The tree outside the strings already holds the cap when the first string begins: its
        // text can't be kept, so the part is refused, as one that streams nothing is, instead of
        // handing out strings that each lost their text. An empty string loses nothing and passes,
        // as on main: the cap bounds the elements the parser holds, and an empty streamed element
        // holds none.
        let filler = "<x/>".repeat(MAX_XML_NODES);
        let how = Streaming { split: &["si"], children: &[("t", usize::MAX)] };
        let mut strings = 0;
        let xml = format!("<sst>{filler}<si/><si><t>alpha</t></si><si><t>beta</t></si></sst>");
        let parsed = parse_streaming(xml.as_bytes(), how, &mut |_| {
            strings += 1;
            Ok(())
        });
        let Err(failure) = parsed else { panic!("a part whose cap is filled outside its strings is refused") };
        assert!(failure.error.to_string().contains("elements"), "{}", failure.error);
        assert_eq!(strings, 1, "only the empty string is handed out");
    }

    #[test]
    fn strings_that_hold_no_element_are_handed_out_with_the_cap_full_as_on_main() {
        // With the tree outside the strings at the cap, or one under it, a string holding no
        // element (self-closing, open and closed, or whitespace only) is handed out with nothing
        // in it, and so is each of three in a row: the cap bounds the elements the parser holds,
        // and such a string holds none. One under the cap, a string's text still fits.
        let how = Streaming { split: &["si"], children: &[("t", usize::MAX)] };
        for (fill, strings, expected) in [
            (MAX_XML_NODES, "<si></si>", vec![""]),
            (MAX_XML_NODES, "<si> </si>", vec![""]),
            (MAX_XML_NODES - 1, "<si/><si><t>alpha</t></si>", vec!["", "alpha"]),
            (MAX_XML_NODES, "<si/><si></si><si/>", vec!["", "", ""]),
        ] {
            let xml = format!("<sst>{}{strings}</sst>", "<x/>".repeat(fill));
            let mut seen = Vec::new();
            parse_streaming(xml.as_bytes(), how, &mut |si| {
                assert_eq!(si.cut, 0, "{strings}");
                seen.push(rich_text(&si));
                Ok(())
            })
            .unwrap_or_else(|f| panic!("{strings} at {fill}: {}", f.error));
            assert_eq!(seen, expected, "{strings} at {fill}");
        }
    }

    #[test]
    fn a_streamed_element_that_begins_inside_another_with_the_cap_full_is_refused() {
        // The outer string's runs fill the cap, and the string inside it begins with the cap
        // already full: its text can't be kept, so the part is refused, as on main, instead of
        // handing out an inner string that lost its text.
        let xml = format!("<sst><si>{}<si><t>inner</t></si></si></sst>", "<r/>".repeat(MAX_XML_NODES));
        let how = Streaming { split: &["si"], children: &[("t", usize::MAX), ("r", usize::MAX)] };
        let mut strings = 0;
        let parsed = parse_streaming(xml.as_bytes(), how, &mut |_| {
            strings += 1;
            Ok(())
        });
        let Err(failure) = parsed else { panic!("a part whose cap is filled before an inner string begins is refused") };
        assert!(failure.error.to_string().contains("elements"), "{}", failure.error);
        assert_eq!(strings, 0);
    }

    #[test]
    fn phonetic_data_validates_offsets() {
        let x = r#"<si><t>課きく</t><rPh sb="0" eb="1"><t>カ</t></rPh><rPh sb="2" eb="9"><t>ク</t></rPh><rPh sb="2" eb="2"><t>x</t></rPh><rPh sb="1"/></si>"#;
        let si = parse(x.as_bytes()).unwrap();
        let (runs, props) = phonetic_data(&si, &rich_text(&si));
        assert_eq!(runs, vec![PhoneticRun { start: 0, end: 1, text: "カ".into() }, PhoneticRun { start: 2, end: 3, text: "ク".into() }]);
        assert!(props.is_none());
    }

    /// The write path validates too: a `PhoneticString` that came from JSON or an API caller (not
    /// from this reader) is written only where the schema allows it — an `sb` before `eb`, both
    /// inside the base text, a reading that isn't empty, and known `type`/`alignment` values.
    #[test]
    fn si_body_drops_damaged_phonetic_data() {
        // 課きく is three UTF-16 units.
        let text = "課きく";
        let damaged = PhoneticString {
            text: text.into(),
            runs: vec![
                PhoneticRun { start: 8, end: 9, text: "x".into() },    // past the text
                PhoneticRun { start: 2, end: 2, text: "x".into() },    // empty range
                PhoneticRun { start: 0, end: 1, text: String::new() }, // no reading
                PhoneticRun { start: 1, end: 3, text: "く".into() },   // valid, clamped end
            ],
            props: Some(PhoneticProps { font_id: 7, kind: Some("bogus".into()), alignment: Some("center".into()) }),
        };
        let s = si_body(text, Some(&damaged));
        assert_eq!(s, "<t>課きく</t><rPh sb=\"1\" eb=\"3\"><t>く</t></rPh><phoneticPr fontId=\"7\" alignment=\"center\"/>");
        // With no run left, valid properties are still written: `<phoneticPr>` is a legal child of
        // `<si>` on its own, and Excel keeps the settings when the last reading is removed.
        let empty = PhoneticString { text: text.into(), runs: vec![], props: damaged.props.clone() };
        assert_eq!(si_body(text, Some(&empty)), "<t>課きく</t><phoneticPr fontId=\"7\" alignment=\"center\"/>");
        // With neither runs nor properties there is nothing to write.
        let none = PhoneticString { text: text.into(), runs: vec![], props: None };
        assert_eq!(si_body(text, Some(&none)), t_el(text));
        // A known kind is written; the schema default ("none") is not invented for a missing one.
        let known = PhoneticString {
            text: text.into(),
            runs: vec![PhoneticRun { start: 0, end: 1, text: "カ".into() }],
            props: Some(PhoneticProps { font_id: 1, kind: Some("Hiragana".into()), alignment: None }),
        };
        assert_eq!(si_body(text, Some(&known)), "<t>課きく</t><rPh sb=\"0\" eb=\"1\"><t>カ</t></rPh><phoneticPr fontId=\"1\" type=\"Hiragana\"/>");
    }

    #[test]
    fn parse_basic() {
        let x = br#"<?xml version="1.0"?><a:root xmlns:a="x" k="v &amp; w"><b>t &lt; u&#65;</b><c/><![CDATA[<raw>]]></a:root>"#;
        let e = parse(x).unwrap();
        assert_eq!(e.name, "root");
        assert_eq!(e.attr("k"), Some("v & w"));
        assert_eq!(e.child("b").unwrap().text, "t < uA");
        assert!(e.child("c").is_some());
        assert_eq!(e.text, "<raw>");
    }

    #[test]
    fn streaming_split() {
        let x = b"<r><row n='1'/><row n='2'><c/></row><other/></r>";
        let mut n = 0;
        let root = parse_streaming(x, Streaming { split: &["row"], children: &[("c", usize::MAX)] }, &mut |e| {
            assert_eq!(e.name, "row");
            n += 1;
            Ok(())
        })
        .unwrap()
        .root;
        assert_eq!(n, 2);
        assert_eq!(root.children.len(), 1);
    }

    #[test]
    fn an_element_deeper_than_the_streamed_level_sharing_a_split_name_is_neither_streamed_nor_held_to_the_allowances() {
        // An element named in `split` (with a namespace prefix, dropped from its name) six levels
        // below the root is not a child or grandchild of it, so it is an ordinary element of the
        // tree: not streamed, kept with everything it holds, and nothing in it counted as dropped.
        // Given children past the allowance on purpose, so counting them would show.
        let xml = "<root><list><item><c/><c/></item><item><c/></item></list>\
                   <outer><middle><inner><deeper><deepest><ns:item><c/><c/><c/></ns:item></deepest></deeper></inner></middle></outer>\
                   </root>";
        let mut streamed = Vec::new();
        let s = parse_streaming(xml.as_bytes(), Streaming { split: &["item"], children: &[("c", 1)] }, &mut |r| {
            streamed.push(r.children.len());
            Ok(())
        })
        .unwrap();
        // The two streamed elements, the first cut to its one allowed child.
        assert_eq!(streamed, [1, 1]);
        assert_eq!(s.dropped, 1);
        let deep_item = ["outer", "middle", "inner", "deeper", "deepest", "item"]
            .iter()
            .try_fold(&s.root, |el, name| el.child(name))
            .expect("the deeper element stays in the tree");
        assert_eq!(deep_item.children.len(), 3);
    }

    #[test]
    fn deep_nesting_is_an_error() {
        let mut s = String::new();
        for _ in 0..10_000 {
            s.push_str("<a>");
        }
        assert!(parse(s.as_bytes()).is_err());
    }

    #[test]
    fn garbage() {
        assert!(parse(b"").is_err());
        assert!(parse(b"\x00\x01\x02 not xml").is_err() || parse(b"\x00\x01\x02 not xml").is_ok());
        let _ = parse(b"<a><b></a>");
        let _ = parse(b"<a attr='&bogus;'>&unknown;</a>");
    }

    #[test]
    fn xstring() {
        assert_eq!(decode_xstring("a_x000D_b"), "a\rb");
        assert_eq!(encode_xstring("a\r\nb\rc"), "a_x000D_\nb_x000D_c");
        assert_eq!(decode_xstring(&encode_xstring("a\r\nb\rc")), "a\r\nb\rc");
        assert_eq!(encode_xstring("a\u{1}b"), "a_x0001_b");
        assert_eq!(decode_xstring(&encode_xstring("_x0041_")), "_x0041_");
        assert_eq!(decode_xstring("_x00"), "_x00");
    }

    #[test]
    fn xstring_surrogate_pairs() {
        assert_eq!(decode_xstring("a_xD83D__xDE00_b"), "a\u{1F600}b");
        assert_eq!(decode_xstring("_xd83d__xde00_"), "\u{1F600}");
        // Lone surrogates stay as literal escape text.
        assert_eq!(decode_xstring("_xD83D_"), "_xD83D_");
        assert_eq!(decode_xstring("_xDE00_"), "_xDE00_");
        assert_eq!(decode_xstring("_xD83D_x"), "_xD83D_x");
        assert_eq!(decode_xstring("_xD83D__x0041_"), "_xD83D_A");
        // A protected literal escape is not a low surrogate.
        assert_eq!(decode_xstring("_xD83D__x005F_xDE00_"), "_xD83D__xDE00_");
    }

    #[test]
    fn text_escaping_preserves_carriage_returns() {
        assert_eq!(esc("a\rb"), "a&#13;b");
        assert_eq!(esc_attr("a\rb"), "a&#13;b");
    }

    /// The appending helpers the sheet writer uses write exactly what the allocating spellings
    /// (`CellRef::a1`, `to_string`, the `num` formula and the `esc` character loop) write.
    #[test]
    fn appending_helpers_match_the_allocating_spellings() {
        use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS};
        for c in [
            CellRef::new(0, 0),
            CellRef::new(9, 25),
            CellRef::new(99, 26),
            CellRef::new(0, 701),
            CellRef::new(0, 702),
            CellRef::new(MAX_ROWS - 1, MAX_COLS - 1),
            CellRef::new(u32::MAX, u32::MAX),
        ] {
            let mut s = String::from("x");
            push_a1(&mut s, c);
            assert_eq!(s, format!("x{}", c.a1()));
        }
        for n in [0, 9, 10, 99, 1_048_576, u64::MAX] {
            let mut s = String::from("x");
            push_u64(&mut s, n);
            assert_eq!(s, format!("x{n}"));
        }
        for n in
            [0.0, -0.0, 1.0, -3.0, 0.1, -2.5, 1e-7, 1.5e20, 1e15, -1e15, 999_999_999_999_999.0, f64::NAN, f64::INFINITY, f64::MIN_POSITIVE, f64::MAX]
        {
            let reference = if !n.is_finite() {
                "0".to_string()
            } else if n == n.trunc() && n.abs() < 1e15 {
                format!("{}", n as i64)
            } else {
                format!("{n:?}").replace('e', "E")
            };
            let mut s = String::from("x");
            push_num(&mut s, n);
            assert_eq!(s, format!("x{reference}"), "{n}");
            assert_eq!(num(n), reference);
        }
        for t in ["plain", "", "a & <b> > c \"q\"", "tab\tnew\nline\rcr", "\u{1}\u{8}ctl", "日本語 ü", "\u{FFFE}\u{FFFF}x", "~\u{7F}"] {
            let mut reference = String::new();
            for c in t.chars() {
                match c {
                    '&' => reference.push_str("&amp;"),
                    '<' => reference.push_str("&lt;"),
                    '>' => reference.push_str("&gt;"),
                    '\r' => reference.push_str("&#13;"),
                    c if is_xml_char(c) => reference.push(c),
                    _ => {}
                }
            }
            let mut s = String::from("x");
            push_esc(&mut s, t);
            assert_eq!(s, format!("x{reference}"), "{t:?}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(num(1.0), "1");
        assert_eq!(num(0.1), "0.1");
        assert_eq!(num(1e-7), "1E-7");
        assert_eq!(num(-3.0), "-3");
        assert_eq!(num(1.5e20), "1.5E20");
        assert_eq!("1.5E20".parse::<f64>().unwrap(), 1.5e20);
    }
}

#[cfg(test)]
mod shared_escape_parity {
    use super::*;
    #[test]
    fn xml_content_and_attributes_preserve_grid_bytes() {
        let value = "&<>\"'\t\n\r\0\u{1f}\u{fffe}\u{ffff}é😀";
        assert_eq!(esc(value), "&amp;&lt;&gt;\"'\t\n&#13;é😀");
        assert_eq!(esc_attr(value), "&amp;&lt;&gt;&quot;'&#9;&#10;&#13;é😀");
        let mut appended = "prefix:".to_owned();
        push_esc(&mut appended, value);
        assert_eq!(appended, format!("prefix:{}", esc(value)));
        assert_eq!(esc("plain ASCII\t\n"), "plain ASCII\t\n");
        assert_eq!(decode_xstring(&encode_xstring("_x000D_\r\0😀")), "_x000D_\r\0😀");
    }
}

#[cfg(test)]
mod shared_xstring_parity {
    use super::*;

    #[test]
    fn xstring_preserves_controls_unicode_and_literal_escapes() {
        // XML-invalid scalars, C1 controls and literal escapes survive save/reopen.
        for c in (0..=0xffff).filter_map(char::from_u32) {
            let text = format!("left{c}right_x0041_");
            assert_eq!(decode_xstring(&encode_xstring(&text)), text);
        }
        for (encoded, decoded) in [
            ("_x005F_x000D_", "_x000D_"),
            ("_xD83D__xDE00_", "😀"),
            ("_xD83D__x0041_", "_xD83D_A"),
            ("_xD83D__x005F_xDE00_", "_xD83D__xDE00_"),
            ("_X0041_", "_X0041_"),
            ("_xZZZZ_", "_xZZZZ_"),
        ] {
            assert_eq!(decode_xstring(encoded), decoded);
        }
        assert_eq!(encode_xstring("\r\n\t\0"), "_x000D_\n\t_x0000_");
    }
}
