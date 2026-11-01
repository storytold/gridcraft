use super::read::{map_rel_ids, vml_block_max};
use super::*;

// ---------------------------------------------------------------- writing

/// Names given to kept parts while a package is written.
#[derive(Default)]
pub struct KeptOut {
    used: HashSet<String>,
    n: u32,
    /// The highest VML shape block a kept header and footer drawing uses: notes go after it.
    vml_blocks: u32,
    /// Package-root relationships (type, target, external), added when `_rels/.rels` is written.
    root: Vec<(String, String, bool)>,
}

impl KeptOut {
    /// True when a kept (pass-through) part already holds this exact name: the writer must
    /// not put its own part there (a zip entry cannot repeat).
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
        // A comment between tags is skipped whole: its text may hold anything.
        if s[i..].starts_with("<!--") {
            let Some(end) = s[i..].find("-->").map(|e| i + e + 3) else { break };
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
fn merge_connections(data: &[u8], wb: &Workbook) -> Option<String> {
    let raw = std::str::from_utf8(data).ok()?;
    // Nothing of ours in the part and nothing to write: leave the file's part alone (a
    // foreign-only connections.xml is never rewritten, byte for byte).
    if wb.web_queries.is_empty() && !raw.contains(crate::write::GC_URI) {
        return None;
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
        // The element name may be followed by any XML whitespace, not only a space: a tab or a
        // newline between `<connection` and its first attribute is valid. `<connection` also
        // prefixes `<connections` (the root), so every occurrence is checked until one is an
        // element of its own.
        let Some(start) = rest.match_indices("<connection").map(|(s, _)| s).find(|&s| {
            rest.as_bytes().get(s + "<connection".len()).is_some_and(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n'))
        }) else { break };
        let after = &rest[start..];
        let end = block_end(after)?;
        let block = &after[..end];
        if !block.contains(crate::write::GC_URI) {
            foreign.push_str(block);
        }
        rest = &after[end..];
    }
    // The rebuilt root keeps the original's namespace and attribute declarations: a
    // foreign connection block may rely on one (Power Query's `xel:`, say) declared on the
    // root, and copying the block under a root without it would not be XML it parses from.
    // GridCraft's own `xmlns:gc` is dropped from the kept set (it is written anew), and the
    // parser gives every attribute exactly as the file held it.
    let x = crate::xml::parse(data).ok()?;
    let mut root = String::new();
    for (k, v) in &x.attrs {
        if k == "xmlns" || k == "xmlns:gc" {
            continue;
        }
        let _ = write!(root, " {k}=\"{}\"", crate::xml::esc_attr(v));
    }
    let mut w = String::from(crate::write::XML_DECL);
    let _ = write!(w, "<connections{root} xmlns=\"{}\" xmlns:gc=\"{}\">",
        crate::write::NS_MAIN,
        crate::write::GC_NS);
    w.push_str(&foreign);
    // Sparse foreign ids (Excel's own files reuse numbers freely): the new connections
    // start past the highest id the part holds, never at a count that could collide.
    let mut id = x.kids("connection").filter_map(|c| c.attr_u32("id")).max().unwrap_or(0) + 1;
    for q in &wb.web_queries {
        let refresh = if q.refresh_on_open { " refreshOnLoad=\"1\"" } else { "" };
        let tid = q.table_id.unwrap_or(0);
        let synced = q.last_synced.unwrap_or(0);
        let _ = write!(w,
            "<connection id=\"{}\" name=\"{}\" type=\"5\"{refresh} refreshedVersion=\"6\"><webPr url=\"{}\" xmlTables=\"1\"/><extLst><ext uri=\"{}\"><gc:webQuery table=\"{}\" occurrence=\"{}\" sheet=\"{}\" tableId=\"{}\" lastSynced=\"{}\"/></ext></extLst></connection>",
            id,
            crate::xml::esc_attr(&q.name),
            crate::xml::esc_attr(&q.url),
            crate::write::GC_URI,
            crate::xml::esc_attr(&q.table),
            q.occurrence,
            crate::xml::esc_attr(&q.sheet),
            tid,
            synced);
        id += 1;
    }
    w.push_str("</connections>");
    Some(w)
}

/// Writes the workbook's kept parts and relationships. Chart sheets that can't be written back
/// (see [`chart_sheet_usable`]) are left out with the parts only they reach.
pub fn write_workbook(wb: &Workbook, wb_rels: &mut Rels, out: &mut Out) -> WorkbookKept {
    let Some(p) = wb.preserved.as_deref() else { return WorkbookKept::default() };
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
    if let Some(q) = parts.iter_mut().find(|q| normalize(&q.name) == "xl/connections.xml") {
        // Runs even when the model holds no connections now: the part's stale GridCraft
        // connections (removed since the open) must not survive the save, while foreign ones
        // stay whatever the model does.
        if let Some(merged) = merge_connections(&q.data, wb) {
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
    WorkbookKept {
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
    }
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
