use super::*;

/// Largest package part read while validating a written package.
const MAX_XML_PART: u64 = 64 * 1024 * 1024;

/// Most zip entries looked at while validating a written package.
const MAX_ENTRIES: usize = 200_000;

/// Relationship kinds (lower case) whose absence after a save isn't a loss: the calculation chain is
/// a cache Excel rebuilds, and GridCraft never writes it.
pub(super) const REBUILT_KINDS: [&str; 1] = ["calcchain"];

// ---------------------------------------------------------------- package validation

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => name.get(i + 1..).unwrap_or(name),
        None => name,
    }
}

/// `(element local name, attributes)` for every start/empty element of an XML part.
type XmlElement = (String, Vec<(String, String)>);

fn xml_elements(xml: &[u8]) -> Result<Vec<XmlElement>, String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut out = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let attrs = e
                    .attributes()
                    .with_checks(false)
                    .flatten()
                    .map(|a| {
                        let v = match a.normalized_value(quick_xml::XmlVersion::Explicit1_0) {
                            Ok(v) => v.into_owned(),
                            Err(_) => String::from_utf8_lossy(&a.value).into_owned(),
                        };
                        (String::from_utf8_lossy(local_name(a.key.as_ref())).into_owned(), v)
                    })
                    .collect();
                out.push((String::from_utf8_lossy(local_name(e.name().as_ref())).into_owned(), attrs));
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(format!("invalid XML: {e}")),
        }
        buf.clear();
    }
    Ok(out)
}

fn attr<'a>(attrs: &'a [(String, String)], k: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

/// Lower-cased part name without a leading slash: OPC part names compare case-insensitively.
fn part_key(name: &str) -> String {
    name.trim_start_matches('/').to_ascii_lowercase()
}

/// `%XX` escapes in a relationship target.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        let hex = |j: usize| b.get(j).and_then(|h| (*h as char).to_digit(16));
        match (c, hex(i + 1), hex(i + 2)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Resolves a relationship target against the source part's directory.
fn resolve_target(source_dir: &str, target: &str) -> String {
    let target = percent_decode(target.split('#').next().unwrap_or(target));
    let joined = match target.strip_prefix('/') {
        Some(abs) => abs.to_string(),
        None if source_dir.is_empty() => target,
        None => format!("{source_dir}/{target}"),
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

type Zip<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

fn zip_names(zip: &mut Zip<'_>) -> Vec<String> {
    (0..zip.len().min(MAX_ENTRIES)).filter_map(|i| zip.by_index_raw(i).ok().filter(|f| !f.is_dir()).map(|f| f.name().to_string())).collect()
}

fn read_zip_part(zip: &mut Zip<'_>, name: &str) -> Result<Vec<u8>, String> {
    let f = zip.by_name(name).map_err(|e| format!("{name}: {e}"))?;
    let mut buf = Vec::new();
    f.take(MAX_XML_PART).read_to_end(&mut buf).map_err(|e| format!("{name}: {e}"))?;
    Ok(buf)
}

/// Whether the producer flagged its cached results as stale with `<calcPr fullCalcOnLoad="1">`:
/// libraries that don't calculate write it, and store 0 or nothing as every formula's value.
pub(super) fn full_calc_on_load(bytes: &[u8]) -> bool {
    let Ok(mut zip) = zip::ZipArchive::new(Cursor::new(bytes)) else { return false };
    let names = zip_names(&mut zip);
    let find = |key: &str| names.iter().find(|n| part_key(n) == key).cloned();
    let mut workbook = "xl/workbook.xml".to_string();
    if let Some(rels) = find("_rels/.rels")
        && let Ok(xml) = read_zip_part(&mut zip, &rels)
        && let Ok(els) = xml_elements(&xml)
        && let Some(target) = els
            .iter()
            .find(|(e, a)| e == "Relationship" && attr(a, "Type").is_some_and(|t| t.ends_with("/officeDocument")))
            .and_then(|(_, a)| attr(a, "Target"))
    {
        workbook = resolve_target("", target);
    }
    let Some(name) = find(&part_key(&workbook)) else { return false };
    let Ok(xml) = read_zip_part(&mut zip, &name) else { return false };
    xml_elements(&xml)
        .ok()
        .into_iter()
        .flatten()
        .any(|(e, a)| e == "calcPr" && attr(&a, "fullCalcOnLoad").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")))
}

/// Problems Excel would repair in a written package: parts without a content type, content-type
/// overrides for missing parts, relationship files for missing parts, duplicate relationship ids
/// and internal relationships pointing at missing parts. `Err` when the package can't be read.
pub fn validate_package(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let names = zip_names(&mut zip);
    let parts: HashSet<String> = names.iter().map(|n| part_key(n)).collect();
    let mut read_part = |name: &str| read_zip_part(&mut zip, name);
    let mut issues = Vec::new();
    let ct_name = names.iter().find(|n| part_key(n) == "[content_types].xml").cloned();
    let (mut defaults, mut overrides) = (HashSet::new(), HashSet::new());
    match ct_name {
        None => issues.push("no [Content_Types].xml".to_string()),
        Some(n) => {
            for (el, attrs) in xml_elements(&read_part(&n)?).map_err(|e| format!("[Content_Types].xml: {e}"))? {
                match el.as_str() {
                    "Default" => {
                        if let Some(x) = attr(&attrs, "Extension") {
                            defaults.insert(x.to_ascii_lowercase());
                        }
                    }
                    "Override" => {
                        if let Some(p) = attr(&attrs, "PartName") {
                            overrides.insert(part_key(p));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort();
    for n in &sorted {
        let key = part_key(n);
        if key == "[content_types].xml" {
            continue;
        }
        let ext = key.rsplit('/').next().and_then(|f| f.rsplit_once('.')).map(|(_, e)| e.to_string());
        if !overrides.contains(&key) && !ext.is_some_and(|e| defaults.contains(&e)) {
            issues.push(format!("part {n} has no content type"));
        }
    }
    let mut missing_overrides: Vec<&String> = overrides.iter().filter(|p| !parts.contains(*p)).collect();
    missing_overrides.sort();
    for p in missing_overrides {
        issues.push(format!("content type override for missing part /{p}"));
    }
    for n in sorted {
        let key = part_key(n);
        let Some(rels_file) = key.strip_suffix(".rels") else { continue };
        // `dir/_rels/name.rels` describes `dir/name`; `_rels/.rels` the package itself.
        let (dir, file) = match rels_file.rsplit_once('/') {
            Some((d, f)) => (d, f),
            None => continue,
        };
        let Some(src_dir) = dir.strip_suffix("_rels").map(|d| d.trim_end_matches('/')) else { continue };
        let source = if src_dir.is_empty() { file.to_string() } else { format!("{src_dir}/{file}") };
        if !source.is_empty() && !parts.contains(&source) {
            issues.push(format!("{n} describes missing part {source}"));
        }
        let mut ids = HashSet::new();
        for (el, attrs) in xml_elements(&read_part(n)?).map_err(|e| format!("{n}: {e}"))? {
            if el != "Relationship" {
                continue;
            }
            let id = attr(&attrs, "Id").unwrap_or("");
            if !ids.insert(id.to_string()) {
                issues.push(format!("{n}: duplicate relationship id {id}"));
            }
            if attr(&attrs, "TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External")) {
                continue;
            }
            let target = attr(&attrs, "Target").unwrap_or("");
            let resolved = resolve_target(src_dir, target);
            if !parts.contains(&resolved.to_ascii_lowercase()) {
                let kind = attr(&attrs, "Type").and_then(|t| t.rsplit('/').next()).unwrap_or("?");
                issues.push(format!("{n}: {id} ({kind}) targets missing part {resolved}"));
            }
        }
    }
    Ok(issues)
}

/// Every relationship kind (last segment of the type, the same in the transitional and strict
/// namespaces) in the package's relationship parts, keyed in lower case. Relationships that carry
/// nothing are left out, as a save may rightly drop them: a link to a place in the workbook
/// (`#Sheet1!A1`, written as a cell's `location`) and a target part with nothing in it.
pub(super) fn relationship_kinds(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let names = zip_names(&mut zip);
    let by_key: HashMap<String, &String> = names.iter().map(|n| (part_key(n), n)).collect();
    // Several relationships name the same part; each fresh check decompressed it again, so
    // repeated references to one highly compressible part multiplied the work. The verdict is
    // remembered per (part, named content) and the decompression the package can cause is
    // budgeted as a whole (finding: repeated decompression).
    let mut verdicts: HashMap<(String, &'static str), bool> = HashMap::new();
    let mut budget: u64 = 512 * 1024 * 1024;
    let mut kinds = BTreeMap::new();
    for n in names.iter().filter(|n| part_key(n).ends_with(".rels")) {
        let Some(src_dir) = rels_source_dir(&part_key(n)) else { continue };
        let Ok(els) = read_zip_part(&mut zip, n).and_then(|xml| xml_elements(&xml)) else { continue };
        for (_, attrs) in els.iter().filter(|(el, _)| el == "Relationship") {
            let Some(kind) = attr(attrs, "Type").and_then(|t| t.rsplit('/').next()) else { continue };
            let target = attr(attrs, "Target").unwrap_or("");
            if kind.eq_ignore_ascii_case("hyperlink") && target.starts_with('#') {
                continue;
            }
            let external = attr(attrs, "TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
            if !external
                && let Some(part) = by_key.get(&resolve_target(&src_dir, target).to_ascii_lowercase())
                && !holds_something(&mut zip, part, kind, &mut verdicts, &mut budget)
            {
                continue;
            }
            kinds.entry(kind.to_ascii_lowercase()).or_insert_with(|| kind.to_string());
        }
    }
    Ok(kinds)
}

/// The directory of the part a rels part describes: `xl` for `xl/_rels/workbook.xml.rels`, the
/// empty string for the package's `_rels/.rels`.
fn rels_source_dir(key: &str) -> Option<String> {
    let (dir, _) = key.strip_suffix(".rels")?.rsplit_once('/')?;
    Some(dir.strip_suffix("_rels")?.trim_end_matches('/').to_string())
}

/// Whether a part has content: an element under its root, or for comments (whose authors list
/// comes first) a comment, for VML a shape. Read only as far as needed; an unreadable part counts.
/// `verdicts` remembers the answer per (part, named content); `budget` is the decompression this
/// package's remaining checks may cause, and once it is spent the part counts as having content.
fn holds_something(zip: &mut Zip<'_>, name: &str, kind: &str, verdicts: &mut HashMap<(String, &'static str), bool>, budget: &mut u64) -> bool {
    let wanted: Option<&'static [u8]> = match kind.to_ascii_lowercase().as_str() {
        "comments" => Some(b"comment"),
        "vmldrawing" => Some(b"shape"),
        _ => None,
    };
    let marker = match wanted {
        Some(b"comment") => "comment",
        Some(_) => "shape",
        None => "any",
    };
    if let Some(v) = verdicts.get(&(name.to_ascii_lowercase(), marker)) {
        return *v;
    }
    let v = holds_something_uncached(zip, name, marker, budget);
    verdicts.insert((name.to_ascii_lowercase(), marker), v);
    v
}

fn holds_something_uncached(zip: &mut Zip<'_>, name: &str, marker: &'static str, budget: &mut u64) -> bool {
    use quick_xml::events::Event;
    let Ok(f) = zip.by_name(name) else { return true };
    if *budget == 0 {
        return true;
    }
    let take = f.size().min(MAX_XML_PART).min(*budget);
    *budget -= take;
    let mut reader = quick_xml::Reader::from_reader(std::io::BufReader::new(f.take(take)));
    let wanted: Option<&'static [u8]> = match marker {
        "comment" => Some(b"comment"),
        "shape" => Some(b"shape"),
        _ => None,
    };
    let mut buf = Vec::new();
    let mut in_root = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let found = match wanted {
                    Some(w) => local_name(e.name().as_ref()) == w,
                    None => in_root,
                };
                if found {
                    return true;
                }
                in_root = true;
            }
            Ok(Event::Eof) => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
        buf.clear();
    }
}

/// The `docProps/core.xml` properties GridCraft keeps (`DocProps`), as Excel writes them.
pub(super) const CORE_PROPERTIES: [&str; 15] = [
    "dc:title",
    "dc:subject",
    "dc:creator",
    "cp:keywords",
    "dc:description",
    "cp:lastModifiedBy",
    "cp:lastPrinted",
    "dcterms:created",
    "dcterms:modified",
    "cp:category",
    "cp:contentStatus",
    "cp:revision",
    "cp:version",
    "dc:identifier",
    "dc:language",
];

/// The `properties` category: every property of [`CORE_PROPERTIES`] the source sets must come back
/// with the same text. `property dcterms:created lost`, `property cp:lastModifiedBy changed: "A" → "B"`.
pub(super) fn diff_core_properties(source: &[u8], written: &[u8], max: usize) -> Category {
    let mut d = Diffs::new("properties", max);
    match (core_properties(source), core_properties(written)) {
        (Ok(before), Ok(after)) => {
            for name in CORE_PROPERTIES {
                let local = name.rsplit(':').next().unwrap_or(name);
                let Some(was) = before.get(local).filter(|v| !v.is_empty()) else { continue };
                match after.get(local).filter(|v| !v.is_empty()) {
                    None => d.push(|| format!("property {name} lost")),
                    Some(now) if now != was => {
                        let quoted = |v: &String| serde_json::to_string(v).unwrap_or_default();
                        d.push(|| format!("property {name} changed: {} → {}", quoted(was), quoted(now)))
                    }
                    Some(_) => {}
                }
            }
        }
        // An unreadable source part has nothing to compare; an unreadable written one is a loss.
        (Ok(before), Err(e)) if !before.is_empty() => d.push(|| format!("core properties unreadable after the save: {e}")),
        _ => {}
    }
    d.done()
}

/// The text of each element under the root of the package's core-properties part (found through
/// `_rels/.rels`), by local name. Empty when the package has none.
pub(super) fn core_properties(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    use quick_xml::events::Event;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| format!("not a zip package: {e}"))?;
    let names = zip_names(&mut zip);
    let find = |key: &str| names.iter().find(|n| part_key(n) == key).cloned();
    let Some(rels) = find("_rels/.rels") else { return Ok(BTreeMap::new()) };
    let rels = xml_elements(&read_zip_part(&mut zip, &rels)?)?;
    let target = rels
        .iter()
        .find(|(e, a)| e == "Relationship" && attr(a, "Type").is_some_and(|t| t.ends_with("/core-properties")))
        .and_then(|(_, a)| attr(a, "Target"));
    let Some(name) = target.and_then(|t| find(&part_key(&resolve_target("", t)))) else { return Ok(BTreeMap::new()) };
    let xml = read_zip_part(&mut zip, &name)?;
    let mut reader = quick_xml::Reader::from_reader(xml.as_slice());
    let (mut buf, mut out, mut depth, mut current) = (Vec::new(), BTreeMap::new(), 0usize, None::<String>);
    loop {
        match reader.read_event_into(&mut buf).map_err(|e| format!("{name}: invalid XML: {e}"))? {
            Event::Start(e) => {
                depth += 1;
                if depth == 2 {
                    let local = String::from_utf8_lossy(local_name(e.name().as_ref())).into_owned();
                    out.entry(local.clone()).or_insert_with(String::new);
                    current = Some(local);
                }
            }
            Event::End(_) => {
                if depth == 2 {
                    current = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Text(t) => {
                if let Some(v) = current.as_ref().and_then(|c| out.get_mut(c)) {
                    v.push_str(&t.xml_content(quick_xml::XmlVersion::Explicit1_0).map_err(|e| format!("{name}: {e}"))?);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(v) = current.as_ref().and_then(|c| out.get_mut(c)) {
                    let named = match r.as_ref() {
                        b"amp" => Some('&'),
                        b"lt" => Some('<'),
                        b"gt" => Some('>'),
                        b"quot" => Some('"'),
                        b"apos" => Some('\''),
                        _ => None,
                    };
                    if let Some(c) = r.resolve_char_ref().ok().flatten().or(named) {
                        v.push(c);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}
