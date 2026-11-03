use super::read::{map_rel_ids, vml_block_max};
use super::*;

// ---------------------------------------------------------------- writing

/// Names given to kept parts while a package is written.
#[derive(Default)]
pub struct KeptOut {
    used: HashSet<String>,
    n: u32,
    /// Whether a connections relationship survived from the input file (see
    /// [`Self::has_connections`]).
    has_connections: bool,
    /// The highest VML shape block a kept header and footer drawing uses: notes go after it.
    vml_blocks: u32,
    /// Package-root relationships (type, target, external), added when `_rels/.rels` is written.
    root: Vec<(String, String, bool)>,
}

impl KeptOut {
    /// True when a kept (pass-through) part already holds this exact name: the writer must
    /// not put its own part there (a zip entry cannot repeat).
    /// True when the input file already has a connections relationship, whatever its part
    /// is named (`xl/connections2.xml` included): the writer must not add a second one, or
    /// the reopen would read the stale original ahead of the model's own part. Set from the
    /// relationship's type when the kept relationships are written, never from a name shape.
    pub fn has_connections(&self) -> bool {
        self.has_connections
    }

    pub fn used_contains(&self, name: &str) -> bool {
        self.used.contains(&name.to_ascii_lowercase())
    }

    /// Reserves the VML shape blocks the kept header and footer drawings of `wb` use, so that the
    /// notes drawings GridCraft writes don't reuse their shape ids.
    pub fn reserve_vml_blocks(&mut self, wb: &Workbook) {
        let vmls =
            wb.sheets.iter().filter_map(|s| s.preserved.as_deref()).flat_map(|p| &p.parts).filter(|q| q.content_type.eq_ignore_ascii_case(CT_VML));
        let max = vmls.map(|q| vml_block_max(&String::from_utf8_lossy(&q.data))).max().unwrap_or(0);
        self.vml_blocks = u32::try_from(max).unwrap_or(u32::MAX);
    }

    /// The shape block of the `n`th notes drawing.
    pub fn notes_block(&self, n: u32) -> u32 {
        n.saturating_add(self.vml_blocks)
    }

    /// A part name for a kept part: its original name when that can't collide with anything
    /// written, otherwise a fresh one in the same folder.
    pub(super) fn claim(&mut self, original: &str) -> String {
        let lower = original.to_ascii_lowercase();
        let own = OWN_DIRS.iter().any(|d| lower.starts_with(d));
        if own && self.used.insert(lower) {
            return original.to_string();
        }
        let (dir, file) = package::split_dir(original);
        let (stem, ext) = match file.rsplit_once('.') {
            Some((s, e)) => (s, format!(".{e}")),
            None => (file, String::new()),
        };
        let stem = if own { format!("{stem}_") } else { "kept".to_string() };
        loop {
            self.n = self.n.saturating_add(1);
            let file = format!("{stem}{}{ext}", self.n);
            let cand = if dir.is_empty() { file } else { format!("{dir}/{file}") };
            if self.used.insert(cand.to_ascii_lowercase()) || self.n == u32::MAX {
                return cand;
            }
        }
    }
}

/// Original part name (normalized) → written part name.
pub(super) type NameMap = HashMap<String, String>;

fn rel_target(r: &PreservedRel, names: &NameMap) -> Option<String> {
    if r.external { Some(r.target.clone()) } else { names.get(&normalize(&r.target)).map(|n| format!("/{n}")) }
}

/// A kept part's relationships, ids unchanged, targets renamed.
pub(super) fn part_rels_xml(rels: &[PreservedRel], names: &NameMap) -> String {
    let mut s = format!("{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">");
    for r in rels {
        let Some(target) = rel_target(r, names) else { continue };
        let _ = write!(
            s,
            "<Relationship Id=\"{}\" Type=\"{}\" Target=\"{}\"{}/>",
            esc_attr(&r.id),
            esc_attr(&r.rel_type),
            esc_attr(&target),
            if r.external { " TargetMode=\"External\"" } else { "" }
        );
    }
    s.push_str("</Relationships>");
    s
}

pub(super) fn write_parts(out: &mut Out, parts: &[PreservedPart]) -> NameMap {
    let mut names = NameMap::new();
    for p in parts {
        names.entry(normalize(&p.name)).or_insert_with(|| out.kept.claim(&p.name));
    }
    let mut written = HashSet::new();
    for p in parts {
        let Some(new) = names.get(&normalize(&p.name)).cloned() else { continue };
        if !written.insert(new.clone()) {
            continue;
        }
        out.part(&new, Some(&p.content_type), p.data.to_vec());
        if !p.rels.is_empty() {
            out.part(&rels_part_name(&new), None, part_rels_xml(&p.rels, &names).into_bytes());
        }
    }
    names
}

/// Adds the owner's relationships; returns old id → new id.
pub(super) fn attach(rels: &mut Rels, list: &[PreservedRel], names: &NameMap) -> HashMap<String, String> {
    let mut ids = HashMap::new();
    for r in list {
        if let Some(target) = rel_target(r, names) {
            let id = rels.add_raw(&r.rel_type, &target, r.external);
            ids.insert(r.id.clone(), id);
        }
    }
    ids
}

pub(super) fn remap(x: &str, ids: &HashMap<String, String>) -> Option<String> {
    map_rel_ids(x, &mut |id| ids.get(id).cloned())
}

/// A kept chart sheet being written: its `<sheet>` entry's fields and its new relationship id.
pub struct KeptSheet {
    before: usize,
    name: String,
    visibility: Visibility,
    rid: String,
    sheet_id: u32,
}

/// The `sheetId`s of the written sheets: each keeps the one the file gave it, unless another
/// sheet took it first (a copied sheet) or it has none (a new sheet), and those get the next id
/// that no sheet asks for.
pub struct SheetIds {
    wanted: HashSet<u32>,
    given: HashSet<u32>,
    next: u32,
}

impl SheetIds {
    pub fn new(wb: &Workbook, kept: &WorkbookKept) -> SheetIds {
        let wanted: HashSet<u32> = wb.sheets.iter().map(|s| s.file_id).chain(kept.sheets.iter().map(|s| s.sheet_id)).filter(|&id| id != 0).collect();
        SheetIds { wanted, given: HashSet::new(), next: 1 }
    }

    pub fn take(&mut self, wanted: u32) -> u32 {
        if wanted != 0 && self.given.insert(wanted) {
            return wanted;
        }
        while self.wanted.contains(&self.next) || self.given.contains(&self.next) {
            self.next += 1;
        }
        self.given.insert(self.next);
        self.next
    }
}

/// The workbook's kept content as written.
#[derive(Default)]
pub struct WorkbookKept {
    /// Workbook XML elements that go right after `<sheets>` (`<externalReferences>`).
    pub elements: String,
    sheets: Vec<KeptSheet>,
}

impl WorkbookKept {
    /// Where the model's sheet `i` is in the file's sheet list: chart sheets come between worksheets.
    pub fn file_index(&self, i: usize) -> usize {
        i + self.sheets.iter().filter(|s| s.before <= i).count()
    }

    /// The `<sheet>` entries of the chart sheets that go right before the model's sheet `i`
    /// (`i` = the number of sheets for those that come last), with their ids from `ids`.
    pub fn sheet_entries(&self, i: usize, last: bool, ids: &mut SheetIds) -> String {
        let mut x = String::new();
        for s in self.sheets.iter().filter(|s| if last { s.before >= i } else { s.before == i }) {
            let state = match s.visibility {
                Visibility::Visible => "",
                Visibility::Hidden => " state=\"hidden\"",
                Visibility::VeryHidden => " state=\"veryHidden\"",
            };
            let name: String = s.name.chars().take(31).collect();
            let id = ids.take(s.sheet_id);
            let _ = write!(x, "<sheet name=\"{}\" sheetId=\"{id}\"{state} r:id=\"{}\"/>", esc_attr(&name), esc_attr(&s.rid));
        }
        x
    }
}

/// The part names reachable from `root` through kept parts' relationships (normalized).
fn reachable(p: &Preserved, root: &PreservedRel) -> HashSet<String> {
    let by_name: HashMap<String, &PreservedPart> = p.parts.iter().map(|q| (normalize(&q.name), q)).collect();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([normalize(&root.target)]);
    while let Some(n) = queue.pop_front() {
        if !seen.insert(n.clone()) {
            continue;
        }
        if let Some(q) = by_name.get(&n) {
            queue.extend(q.rels.iter().filter(|r| !r.external).map(|r| normalize(&r.target)));
        }
    }
    seen
}

/// Every formula (`<c:f>`) of a chart part, depth first.
fn chart_formulas(el: &xml::El, out: &mut Vec<String>) {
    if el.name == "f" && !el.text.trim().is_empty() {
        out.push(el.text.trim().to_string());
    }
    for c in &el.children {
        chart_formulas(c, out);
    }
}

/// The sheet names a formula such as `'My Sheet'!$A$1:$A$9,Data!$B$1` refers to. References to
/// other workbooks (`[1]Sheet!A1`) are not sheets of this one and are left out.
pub(crate) fn sheet_names_in(formula: &str) -> Vec<String> {
    let mut names = vec![];
    let chars: Vec<char> = formula.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut i = 0;
    while let Some(c) = at(i) {
        let mut name = String::new();
        let start = i;
        if c == '\'' {
            i += 1;
            while let Some(c) = at(i) {
                if c == '\'' {
                    if at(i + 1) == Some('\'') {
                        name.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                name.push(c);
                i += 1;
            }
            i += 1;
        } else {
            while let Some(c) = at(i).filter(|c| !matches!(c, '!' | ',' | '(' | ')' | '\'' | ' ')) {
                name.push(c);
                i += 1;
            }
        }
        if at(i) == Some('!') {
            if !name.is_empty() && !name.starts_with('[') {
                names.push(name);
            }
            i += 1;
        } else if i == start {
            i += 1;
        }
    }
    names
}

/// Whether a kept chart sheet can be written back: its chart's series must still refer to sheets
/// that exist under the same names (the chart is kept as it was, so a series on a deleted or
/// renamed sheet would be left pointing at nothing), and no worksheet may have taken its name.
fn chart_sheet_usable(wb: &Workbook, p: &Preserved, s: &PreservedSheet) -> bool {
    let same = |a: &str, b: &str| a.chars().take(31).collect::<String>().eq_ignore_ascii_case(&b.chars().take(31).collect::<String>());
    if wb.sheets.iter().any(|w| same(&w.name, &s.name)) {
        return false;
    }
    let Some(root) = p.rels.iter().find(|r| r.id == s.rel_id) else { return false };
    let parts = reachable(p, root);
    let mut formulas = vec![];
    for q in p.parts.iter().filter(|q| parts.contains(&normalize(&q.name)) && q.content_type.contains("chart")) {
        if let Ok(x) = xml::parse(&q.data) {
            chart_formulas(&x, &mut formulas);
        }
    }
    formulas.iter().flat_map(|f| sheet_names_in(f)).all(|n| wb.sheets.iter().any(|w| same(&w.name, &n)))
}

/// The attributes of a raw start tag (`<connections xmlns="..." k="v">`), each as it was
/// written: name, `=` and quoted value, quote-aware so a value holding spaces stays whole.
/// The element name itself is not an attribute and is skipped.
fn raw_attributes(tag: &str) -> impl Iterator<Item = String> + '_ {
    let mut out = Vec::new();
    let bytes = tag.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // The name: up to `=`.
        let Some(eq) = tag[i..].find('=').map(|e| i + e) else { break };
        // The value: a quote-delimited string (the closing quote is the boundary, so spaces
        // inside survive). The attribute is the raw slice, name, `=` and value as written.
        // XML allows whitespace around the `=` (`xmlns:xel = "..."`), so the quote is the
        // first quote after it, not necessarily the byte right next to it. Only a real
        // quote starts a value (an unquoted or a missing quote ends the scan, so malformed
        // input cannot slice inside a multibyte character and panic the save), and the name
        // is bounded by XML whitespace only (a Unicode space would land mid-byte).
        let q_at = (eq + 1..).find(|&j| bytes.get(j).is_some_and(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'))).filter(|&j| bytes.get(j).is_some_and(|b| matches!(*b, b'"' | b'\'')));
        let Some(q_at) = q_at else { break };
        let q = bytes[q_at];
        let vstart = q_at + 1;
        let Some(vend) = tag[vstart..].find(q as char).map(|v| vstart + v) else { break };
        let name_end = tag[..eq].trim_end_matches([' ', '\t', '\r', '\n']).len();
        let name_start = tag[..name_end].rfind([' ', '\t', '\r', '\n']).map(|w| w + 1).unwrap_or(0);
        let attribute = &tag[name_start..=vend];
        if !attribute.starts_with(char::is_whitespace) {
            out.push(attribute.to_string());
        }
        i = vend + 1;
    }
    out.into_iter()
}

/// The end of the element starting at `s` (`<connection ...`): the whole block, whether it
/// is self-closing (`<connection id="1"/>`) or holds children (`<connection ...><webPr .../>/// </connection>`). A depth count walks the tags: an inner `/>` closes a child, not the
/// connection, and a later block's `</connection>` never ends an earlier self-closing one.
fn block_end(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    // Depth 0 = outside the element; the first tag is the connection's own opener, and the
    // walk returns when it closes (self-closing at once, or its `</connection>`).
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        // A comment or a CDATA section between tags is skipped whole: their text may hold
        // anything, including a `</connection>` that would end the block early.
        if s[i..].starts_with("<!--") {
            let Some(end) = s[i..].find("-->").map(|e| i + e + 3) else { break };
            i = end;
            continue;
        }
        if s[i..].starts_with("<![CDATA[") {
            let Some(end) = s[i..].find("]]>").map(|e| i + e + 3) else { break };
            i = end;
            continue;
        }
        // The tag's end: a `>` inside a quoted attribute value is not it.
        let mut gt = None;
        let mut quote = 0u8;
        for (j, &b) in bytes[i..].iter().enumerate() {
            match (quote, b) {
                (0, b'"') | (0, b'\'') => quote = b,
                (q, b) if q == b => quote = 0,
                (0, b'>') => {
                    gt = Some(i + j);
                    break;
                }
                _ => {}
            }
        }
        let Some(gt) = gt else { break };
        let tag = &s[i..=gt];
        if tag.ends_with("/>") {
            // Self-closing: opens nothing. At depth 0 it is the connection's own opener.
            if depth == 0 {
                return Some(gt + 1);
            }
        } else if tag.starts_with("</") {
            // A closing tag: at depth 1 it closes the connection itself.
            depth -= 1;
            if depth == 0 {
                return Some(gt + 1);
            }
        } else {
            depth += 1;
        }
        i = gt + 1;
    }
    None
}

/// The kept `xl/connections.xml` with the model's current From Web connections merged in:
/// every foreign `<connection>` (no GridCraft extension) is copied verbatim, and the
/// GridCraft ones (with the extension) are replaced by `wb.web_queries` in order. `None`
/// when the part does not parse: it is kept exactly as the file held it.
/// Whether a parsed `<connection>` holds the GridCraft extension. The parser decodes XML
/// character references, so an entity-encoded `ext/@uri` matches here even where a raw
/// text search for the literal marker would miss it.
fn is_gridcraft_connection(connection: &crate::xml::El) -> bool {
    connection.kids("extLst").any(|ext_lst| ext_lst.kids("ext").any(|ext| ext.attr("uri") == Some(crate::write::GC_URI)))
}

/// Whether a connections fragment (a single `<connection>` block, or a whole part) holds a
/// GridCraft connection, by parsed value: an entity-encoded marker does not survive a
/// raw-text check. The wrapper keeps a bare `<connection>` block parseable, and `descendants`
/// reaches the connections wherever the fragment leaves them (a whole part nests them under
/// `<connections>`). A fragment that does not parse holds none (the whole part must parse
/// for the merge, so this is a block-level fallback).
fn parses_as_gridcraft(xml: &[u8]) -> bool {
    crate::xml::parse(format!("<r>{}</r>", String::from_utf8_lossy(xml)).as_bytes()).is_ok_and(|root| descendants(&root, "connection").iter().any(|c| is_gridcraft_connection(c)))
}

/// Every descendant named `name`, at any depth (`kids` is only one level).
fn descendants<'a>(el: &'a crate::xml::El, name: &str) -> Vec<&'a crate::xml::El> {
    let mut out = Vec::new();
    fn walk<'a>(el: &'a crate::xml::El, name: &str, out: &mut Vec<&'a crate::xml::El>) {
        for c in &el.children {
            if c.name == name {
                out.push(c);
            }
            walk(c, name, out);
        }
    }
    walk(el, name, &mut out);
    out
}

fn merge_connections(data: &[u8], wb: &Workbook, remap: &[crate::write::TableIdRemap]) -> Result<Option<String>, IoError> {
    // The encodings the XML reader supports (a BOM, UTF-16): a file Excel saved as UTF-16
    // still merges, rather than failing every open-and-save with the part in it.
    let decoded = crate::xml::to_utf8(data);
    // A part in an unsupported encoding is not necessarily a failure: `to_utf8` only
    // converts BOM-marked or detectable UTF-16, and other bytes pass through. When the
    // model has nothing to write and the ORIGINAL BYTES hold no GridCraft marker (checked
    // before the UTF-8 conversion), the part is foreign-only in an unknown encoding and is
    // kept exactly as it is, rather than failing every open-and-save on it. With something
    // of ours to write, an unreadable part is an honest save error.
    let raw = match std::str::from_utf8(&decoded) {
        Ok(raw) => raw,
        Err(_) if wb.web_queries.is_empty() && !data.windows(crate::write::GC_URI.len()).any(|w| w == crate::write::GC_URI.as_bytes()) => return Ok(None),
        Err(_) => return Err(IoError::Format("the connections part is not text in a supported encoding".into())),
    };
    // Nothing of ours in the part and nothing to write: leave the file's part alone (a
    // foreign-only connections.xml is never rewritten, byte for byte). The marker check is
    // by parsed value as well: an entity-encoded `ext/@uri` is ours to the reader, and a
    // raw-text-only check would leave the stale connection in the part.
    if wb.web_queries.is_empty() && !raw.contains(crate::write::GC_URI) && !parses_as_gridcraft(data) {
        return Ok(None);
    }
    // Foreign connections are kept as the exact bytes the file held: each <connection> block
    // (self-closing or a whole tree) is scanned out of the raw XML with a depth count \—
    // a child's `/>` is not the connection's end, and a later block's `</connection>` is not
    // an earlier self-closing one's \— and the ones without GridCraft's extension uri
    // are copied verbatim.
    let mut foreign = String::new();
    let mut rest = raw;
    // The element name may be followed by any XML whitespace, not only a space: a tab or a
    // newline between `<connection` and its first attribute is valid, and a flat search for
    // "<connection " would skip such a block (and drop it from the merge).
    loop {
        // The element may carry a namespace prefix (`<xel:connection`, Power Query's style)
        // and its name may be followed by any XML whitespace, not only a space. `<connection`
        // also prefixes `<connections` (the root), so every occurrence is checked until one
        // is an element of its own.
        // The name ends at whitespace, a `/` (an attribute-less `<connection/>`), or a
        // closing quote of an attribute-less element; `<connections` (the root) is still
        // rejected because `s` is not any of those.
        let unprefixed = rest.match_indices("<connection").map(|(s, _)| s).find(|&s| {
            rest.as_bytes().get(s + "<connection".len()).is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b'/'))
        });
        // A prefixed element (`<xel:connection`, Power Query's style) is found by its
        // `:connection` tail: each occurrence walks back to its `<`, and the text between
        // must be a prefix name. The block scan itself starts at the `<`, prefix included.
        let prefixed = rest.match_indices(":connection").map(|(s, _)| s).find(|&s| {
            let Some(lt) = rest[..s].rfind('<') else { return false };
            let prefix = &rest[lt + 1..s];
            !prefix.is_empty() && prefix.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')
                // `s` is the colon's index, so the element name ends one past it plus its
                // length: whitespace, or a `/` for an attribute-less element.
                && matches!(rest.as_bytes().get(s + 1 + "connection".len()), Some(b' ' | b'\t' | b'\r' | b'\n' | b'/'))
        });
        // The block starts at the `<` before the prefix (`<xel:connection`), not at the name.
        // Whichever element comes first in the part, prefixed or not: scanning by one form
        // and jumping past the other would drop it (an unprefixed match further right must
        // not shadow an earlier prefixed one).
        let prefixed_start = prefixed.map(|c| rest[..c].rfind('<').unwrap_or(c));
        let start = match (unprefixed, prefixed_start) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        let after = &rest[start..];
        let end = block_end(after).ok_or_else(|| IoError::Format("the connections part could not be scanned".into()))?;
        let block = &after[..end];
        if !block.contains(crate::write::GC_URI) && !parses_as_gridcraft(block.as_bytes()) {
            foreign.push_str(block);
        }
        rest = &after[end..];
    }
    // The rebuilt root keeps the original's namespace declarations exactly as the file held
    // them: a foreign connection block may rely on one (Power Query's `xel:`, say) declared
    // on the root, and copying the block under a root without it would not be XML it parses
    // from. The parser drops xmlns:* from its attribute list, so the raw start tag is scanned
    // instead; GridCraft's own `xmlns:gc` is dropped from the kept set (it is written anew),
    // and every other declaration is copied verbatim between the element name and its `>`.
    // The root by its XML local name, prefixed or not: a valid part may spell it
    // `<x:connections`, and a fixed `<connections` search would find nothing and fail the
    // save. Any name char can sit between the prefix and `connections`, so every
    // occurrence of the local name with a `<` before it is checked.
    let tag_start = raw
        .match_indices("connections")
        .map(|(s, _)| s)
        .find(|&s| raw[..s].ends_with('<') || raw[..s].rfind('<').is_some_and(|lt| raw[lt + 1..s].chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':')))
        .ok_or_else(|| IoError::Format("the connections part has no connections element".into()))?;
    let tag_end = raw[tag_start..].find('>').map(|e| tag_start + e).ok_or_else(|| IoError::Format("the connections part's root tag is unterminated".into()))?;
    // The raw start tag's attributes, one by one (quote-aware, so a value with spaces in it
    // stays whole); the xmlns declarations are copied verbatim except GridCraft's own.
    let attrs: Vec<String> = raw_attributes(&raw[tag_start..=tag_end]).collect();
    // Nothing is filtered by name shape: a foreign `xmlns:gc` binding (another add-in's
    // own gc prefix) is KEPT - the preserved `gc:` elements it binds must keep their
    // identity. GridCraft never reuses the root's gc prefix; it declares its own
    // nonconflicting one locally on the elements it writes.
    let kept: Vec<String> = attrs
        .iter()
        .filter(|a| a.starts_with("xmlns=") || a.starts_with("xmlns:"))
        .filter(|a| a.trim_end_matches(['"', '>']) != "xmlns=\"https://gridcraft.app/ns/webquery\"")
        .cloned()
        // Markup-compatibility attributes (`mc:Ignorable`, whatever prefix binds the
        // Markup Compatibility namespace) are kept too: the foreign blocks below may rely
        // on them to tell an importer which namespaces it may safely ignore. The prefix is
        // resolved through the root's own declarations, so any spelling survives.
        .chain(attrs.iter().filter(|a| {
            let Some((name, _)) = a.split_once('=') else { return false };
            let Some((prefix, _)) = name.split_once(':') else { return false };
            let binding = format!("xmlns:{prefix}=");
            attrs.iter().any(|d| d.starts_with(&binding) && d.contains(crate::write::MC_NS))
        }).cloned())
        .collect();
    let kept_decls: String = kept.iter().map(|a| format!(" {a}")).collect();
    // The rebuilt root keeps the SpreadsheetML namespace one way or another: an unprefixed
    // root carried it as its default (and does again), and a prefixed one
    // (`<x:connections>`) whose declarations are kept verbatim still has its prefix bound -
    // but a prefixed root with NO default namespace would leave the model's unprefixed
    // `<connection>` elements outside any namespace, so the default is emitted for it.
    // Parsed attributes, not a quote-style search: a default namespace may be single-quoted
    // (`xmlns='...'`) and a `contains("xmlns=\"")` check would miss it and add a second
    // xmlns to the rebuilt root.
    // The check reads the declarations that are KEPT: a root whose default is GridCraft's
    // own namespace has it dropped from `kept`, and the rebuilt root must then gain the
    // SpreadsheetML default (checking the raw tag would see the dropped one and skip it).
    let has_default_ns = kept.iter().any(|a| a.starts_with("xmlns=") || a.starts_with("xmlns='"));
    let default_ns = if has_default_ns { String::new() } else { format!(" xmlns=\"{}\"", crate::write::NS_MAIN) };
    // A kept DEFAULT namespace makes an unprefixed root foreign, so a prefixed root keeps
    // its prefix: `<x:connections xmlns="urn:foreign">` stays prefixed, and the generated
    // (unprefixed) connections must then bind the SpreadsheetML namespace inline or they
    // would inherit the foreign default.
    let root_prefix = if has_default_ns {
        // `tag_start` points at the local name, so the prefix is the text between the `<`
        // and it (`x:` for `<x:connections`); the trailing colon is stripped for the check.
        raw[..tag_start].rsplit('<').next().map(|s: &str| s.trim_end_matches(':')).filter(|s| !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.')).map(|s| format!("{s}:")).unwrap_or_default()
    } else {
        String::new()
    };
    let sml_default = if root_prefix.is_empty() { String::new() } else { format!(" xmlns=\"{}\"", crate::write::NS_MAIN) };
    // GridCraft's namespace rides on its OWN generated elements (the prefix declared
    // inline), never on the root: a foreign root-level `xmlns:gc` binding belongs to the
    // foreign elements and stays theirs.
    let gc = format!(" xmlns:gc=\"{}\"", crate::write::GC_NS);
    let mut w = String::from(crate::write::XML_DECL);
    let _ = write!(w, "<{root_prefix}connections{kept_decls}{default_ns}>");
    w.push_str(&foreign);
    // Sparse foreign ids (Excel's own files reuse numbers freely): the new connections
    // start past the highest id the part holds, never at a count that could collide.
    let x = crate::xml::parse(data).map_err(|e| IoError::Format(format!("the connections part does not parse: {e}")))?;
    // Every query gets an id, however crowded the part is: the ids below the maximum that no
    // preserved connection holds (sparse parts reuse numbers freely, so the gaps are legal),
    // then the tail past the maximum. A zip over a bounded range could not do this: a range
    // that ends early silently drops the queries it runs out of, and the save would lose them.
    let used: std::collections::HashSet<u32> = x.kids("connection").filter_map(|c| c.attr_u32("id")).collect();
    let mut ids = (1u32..=u32::MAX).filter(|i| !used.contains(i));
    for q in &wb.web_queries {
        // None only when every id the format allows is taken by a preserved connection.
        let Some(id) = ids.next() else { return Err(IoError::Format("no connection id is left for a web query".into())) };
        let refresh = if q.refresh_on_open { " refreshOnLoad=\"1\"" } else { "" };
        let tid = crate::write::connection_table_id(q, remap);
        let synced = q.last_synced.unwrap_or(0);
        let _ = write!(w,
            "<connection{sml_default} id=\"{}\" name=\"{}\" type=\"5\"{refresh} refreshedVersion=\"6\"><webPr url=\"{}\" xmlTables=\"1\"/><extLst><ext uri=\"{}\"{gc}><gc:webQuery table=\"{}\" occurrence=\"{}\" sheet=\"{}\" tableId=\"{}\" lastSynced=\"{}\"/></ext></extLst></connection>",
            id,
            crate::xml::esc_attr(&q.name),
            crate::xml::esc_attr(&q.url),
            crate::write::GC_URI,
            crate::xml::esc_attr(&q.table),
            q.occurrence,
            crate::xml::esc_attr(&q.sheet),
            tid,
            synced);
    }
    let _ = write!(w, "</{root_prefix}connections>");
    Ok(Some(w))
}

/// Writes the workbook's kept parts and relationships. Chart sheets that can't be written back
/// (see [`chart_sheet_usable`]) are left out with the parts only they reach.
pub fn write_workbook(wb: &Workbook, wb_rels: &mut Rels, out: &mut Out) -> Result<WorkbookKept, IoError> {
    let Some(p) = wb.preserved.as_deref() else { return Ok(WorkbookKept::default()) };
    let (live, dropped): (Vec<&PreservedSheet>, Vec<&PreservedSheet>) = p.sheets.iter().partition(|s| chart_sheet_usable(wb, p, s));
    let root_of = |s: &PreservedSheet| p.rels.iter().find(|r| r.id == s.rel_id);
    let dropped_ids: HashSet<&str> = dropped.iter().map(|s| s.rel_id.as_str()).collect();
    // Parts reached only by dropped chart sheets go with them.
    let mut gone: HashSet<String> = dropped.iter().filter_map(|s| root_of(s)).flat_map(|r| reachable(p, r)).collect();
    if !gone.is_empty() {
        let others = p.rels.iter().filter(|r| !dropped_ids.contains(r.id.as_str())).flat_map(|r| reachable(p, r));
        for n in others {
            gone.remove(&n);
        }
    }
    let mut parts: Vec<PreservedPart> = p.parts.iter().filter(|q| !gone.contains(&normalize(&q.name))).cloned().collect();
    let rels: Vec<PreservedRel> = p.rels.iter().filter(|r| !dropped_ids.contains(r.id.as_str())).cloned().collect();
    // The kept connections part gets the model's current From Web connections merged in: the
    // user may have changed a refresh setting, imported another table or removed a connection
    // since the open, and a save must write the current state, not the file's. Foreign
    // connections (a database query) are kept verbatim; GridCraft's (the ones with the
    // extension element) are replaced wholesale by the model's.
    // The connections part THROUGH ITS RELATIONSHIP, not by a fixed name: the file may
    // name it xl/connections2.xml, and merging into a phantom `xl/connections.xml` while
    // the real part went unchanged would leave the save stale and add a second relationship.
    let conn_rel = rels.iter().find(|r| r.rel_type.ends_with("/connections"));
    out.kept.has_connections = conn_rel.is_some();
    let conn_target = conn_rel.map(|r| normalize(&r.target));
    if let Some(target) = conn_target
        && let Some(q) = parts.iter_mut().find(|q| normalize(&q.name) == target)
    {
        // Runs even when the model holds no connections now: the part's stale GridCraft
        // connections (removed since the open) must not survive the save, while foreign ones
        // stay whatever the model does. A merge that cannot run (malformed part, no ids
        // left) fails the save instead of writing the stale bytes: a save that silently
        // dropped the user's new connection would lose it on the next open.
        if let Some(merged) = merge_connections(&q.data, wb, &out.table_id_remap)? {
            q.data = merged.into_bytes().into();
        }
    }
    let names = write_parts(out, &parts);
    let ids = attach(wb_rels, &rels, &names);
    for r in &p.package_rels {
        if let Some(target) = rel_target(r, &names) {
            out.kept.root.push((r.rel_type.clone(), target, r.external));
        }
    }
    Ok(WorkbookKept {
        elements: p.elements.iter().filter(|e| e.slot == "externalReferences").filter_map(|e| remap(&e.xml, &ids)).collect(),
        sheets: live
            .iter()
            .filter_map(|s| {
                Some(KeptSheet {
                    before: s.before,
                    name: s.name.clone(),
                    visibility: s.visibility,
                    rid: ids.get(&s.rel_id)?.clone(),
                    sheet_id: s.sheet_id,
                })
            })
            .collect(),
    })
}

/// The workbook's kept elements of `slot` that refer to no part (`fileVersion`, `ext`).
pub fn workbook_elements(wb: &Workbook, slot: &str) -> String {
    let Some(p) = wb.preserved.as_deref() else { return String::new() };
    p.elements.iter().filter(|e| e.slot == slot).map(|e| e.xml.as_str()).collect()
}

/// Adds the kept package-root relationships (custom properties…) to `_rels/.rels`.
pub fn write_package_rels(root: &mut Rels, out: &mut Out) {
    for (ty, target, external) in std::mem::take(&mut out.kept.root) {
        root.add_raw(&ty, &target, external);
    }
}
