//! Static in-cell PNG/JPEG images, from the public MS-XLSX rich-data schemas.
//! `vm` and metadata type IDs are one-based; every subsequent table index is zero-based.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_model::{CellPicture, LinkedField, LinkedRecord};

use crate::IoError;
use crate::package::{Rel, normalize};
use crate::read::Ctx;
use crate::write::{NS_MAIN, NS_REL, Out, Rels, XML_DECL};
use crate::xml::{self, El, esc, esc_attr};

const NS_RICH: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/richdata";
const NS_IMAGE_REL: &str = "http://schemas.microsoft.com/office/spreadsheetml/2022/richvaluerel";
const RICH_EXT: &str = "{3E2802C4-A4D2-4D8B-9148-E3BE6C30E623}";
const MAX_IMAGE: u64 = 16 * 1024 * 1024;
const MAX_IMAGES_TOTAL: usize = 128 * 1024 * 1024;
const MAX_RECORDS: usize = 100_000;
const MAX_XML: u64 = 16 * 1024 * 1024;
const MAX_PIXELS: u64 = 16_000_000;

/// Header dimensions bound decode allocations in consumers. Full decoding is left to the UI.
fn image_type(data: &[u8]) -> Option<(&'static str, &'static str)> {
    let dimensions = if data.len() >= 33 && data.starts_with(b"\x89PNG\r\n\x1a\n") && data.get(8..16)? == b"\0\0\0\rIHDR" {
        let w = u32::from_be_bytes(data.get(16..20)?.try_into().ok()?);
        let h = u32::from_be_bytes(data.get(20..24)?.try_into().ok()?);
        Some((w, h, "image/png", "png"))
    } else if data.starts_with(&[0xff, 0xd8]) {
        jpeg_dimensions(data).map(|(w, h)| (w, h, "image/jpeg", "jpeg"))
    } else {
        None
    };
    let (w, h, mime, ext) = dimensions?;
    (w > 0 && h > 0 && w <= 8192 && h <= 8192 && u64::from(w) * u64::from(h) <= MAX_PIXELS).then_some((mime, ext))
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    let mut offset = 2usize;
    while offset < data.len() {
        if *data.get(offset)? != 0xff {
            return None;
        }
        while data.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *data.get(offset)?;
        offset += 1;
        if matches!(marker, 0xd9 | 0xda) {
            return None;
        }
        if marker == 0x01 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let len = usize::from(u16::from_be_bytes(data.get(offset..offset.checked_add(2)?)?.try_into().ok()?));
        let end = offset.checked_add(len)?;
        if len < 2 || end > data.len() {
            return None;
        }
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) && len >= 8 {
            let h = u16::from_be_bytes(data.get(offset + 3..offset + 5)?.try_into().ok()?);
            let w = u16::from_be_bytes(data.get(offset + 5..offset + 7)?.try_into().ok()?);
            return Some((u32::from(w), u32::from(h)));
        }
        offset = end;
    }
    None
}

fn part(cx: &mut Ctx<'_>, rels: &[Rel], kind: &str) -> Option<String> {
    let mut target: Option<String> = None;
    for rel in rels.iter().filter(|rel| rel.kind == kind) {
        if rel.external {
            cx.warn(format!("external {kind} relationship is unsupported; cell pictures may be missing"));
            return None;
        }
        if target.as_ref().is_some_and(|old| normalize(old) != normalize(&rel.target)) {
            cx.warn(format!("conflicting {kind} relationships; cell pictures may be missing"));
            return None;
        }
        target = Some(rel.target.clone());
    }
    target
}

fn read_xml(cx: &mut Ctx<'_>, path: &str, root: &str) -> Result<Option<El>, IoError> {
    let bytes = match cx.pkg.read_limited(path, MAX_XML) {
        Ok(Some(bytes)) => bytes,
        Err(error @ IoError::TooLarge(_)) => return Err(error),
        other => {
            cx.warn(format!("missing or damaged cell-picture part {path}: {}", other.err().map(|e| e.to_string()).unwrap_or_default()));
            return Ok(None);
        }
    };
    match xml::parse(&bytes) {
        Ok(el) if el.name == root => Ok(Some(el)),
        _ => {
            cx.warn(format!("invalid cell-picture part {path}"));
            Ok(None)
        }
    }
}

fn entries<'a>(cx: &mut Ctx<'_>, el: &'a El, child: &'a str) -> Result<Vec<&'a El>, IoError> {
    let entries: Vec<_> = el.kids(child).take(MAX_RECORDS + 1).collect();
    if entries.len() > MAX_RECORDS {
        return Err(IoError::TooLarge("too many cell-picture metadata records".into()));
    }
    if el.attr("count").is_some() && el.attr_u32("count").map(|n| n as usize) != Some(entries.len()) {
        cx.warn(format!("cell-picture {} count does not match its records", el.name));
    }
    Ok(entries)
}

/// Resolves every value-metadata block without assuming the indices at each layer coincide.
pub fn read(cx: &mut Ctx<'_>, workbook_rels: &[Rel]) -> Result<Vec<Option<Arc<CellPicture>>>, IoError> {
    let Some(metadata_path) = part(cx, workbook_rels, "sheetMetadata") else { return Ok(vec![]) };
    let Some(metadata) = read_xml(cx, &metadata_path, "metadata")? else { return Ok(vec![]) };
    let Some(types) = metadata.child("metadataTypes") else { return Ok(vec![]) };
    let types = entries(cx, types, "metadataType")?;
    let Some(type_id) = types.iter().position(|el| el.attr("name") == Some("XLRICHVALUE")).map(|n| n + 1) else { return Ok(vec![]) };
    if types.iter().filter(|el| el.attr("name") == Some("XLRICHVALUE")).count() != 1 {
        cx.warn("duplicate XLRICHVALUE metadata types; cell pictures were not loaded");
        return Ok(vec![]);
    }
    // Support workbook-owned parts and the implicit owner chain described by MS-XLSX.
    let mut rels = workbook_rels.to_vec();
    rels.extend(cx.pkg.rels(&metadata_path)?);
    let Some(data_path) = part(cx, &rels, "rdRichValue") else {
        cx.warn("missing rich-value data relationship; cell pictures were not loaded");
        return Ok(vec![]);
    };
    rels.extend(cx.pkg.rels(&data_path)?);
    let image_rels_path = part(cx, &rels, "richValueRel");
    let Some(structure_path) = part(cx, &rels, "rdRichValueStructure") else {
        cx.warn("missing rich-value structure or image relationships; cell pictures were not loaded");
        return Ok(vec![]);
    };
    let Some(structures) = read_xml(cx, &structure_path, "rvStructures")? else { return Ok(vec![]) };
    let structures = entries(cx, &structures, "s")?;
    // A workbook with only linked data type records has no image relationships.
    if image_rels_path.is_none() && structures.iter().any(|s| is_linked(s)) && !structures.iter().any(|s| s.attr("t") == Some("_localImage")) {
        return Ok(vec![]);
    }
    let Some(image_rels_path) = image_rels_path else {
        cx.warn("missing rich-value structure or image relationships; cell pictures were not loaded");
        return Ok(vec![]);
    };
    let Some(data) = read_xml(cx, &data_path, "rvData")? else { return Ok(vec![]) };
    let Some(image_rel_list) = read_xml(cx, &image_rels_path, "richValueRels")? else { return Ok(vec![]) };
    let image_rels = cx.pkg.rels(&image_rels_path)?;
    let image_rel_list = entries(cx, &image_rel_list, "rel")?;
    let values = entries(cx, &data, "rv")?;
    let mut pictures = Vec::with_capacity(values.len());
    let mut cache = HashMap::new();
    let mut total = 0usize;
    for value in values {
        let picture = read_value(cx, value, &structures, &image_rel_list, &image_rels, &mut cache, &mut total)?;
        let linked = value.attr_u32("s").and_then(|n| structures.get(n as usize)).is_some_and(|s| is_linked(s));
        if picture.is_none() && !linked {
            cx.warn("some rich values are unsupported or malformed; their scalar fallbacks were retained (cell pictures may be missing)");
        }
        pictures.push(picture);
    }
    let Some(future) = metadata.kids("futureMetadata").find(|el| el.attr("name") == Some("XLRICHVALUE")) else {
        cx.warn("missing XLRICHVALUE future metadata; cell pictures were not loaded");
        return Ok(vec![]);
    };
    let indices = block_indices(cx, &metadata, type_id, future)?;
    Ok(indices.into_iter().map(|index| index.and_then(|n| pictures.get(n)).cloned().flatten()).collect())
}

/// The rich-value index of each value-metadata block (the worksheet's one-based `vm`).
fn block_indices(cx: &mut Ctx<'_>, metadata: &El, type_id: usize, future: &El) -> Result<Vec<Option<usize>>, IoError> {
    let future: Vec<_> = entries(cx, future, "bk")?
        .into_iter()
        .map(|block| {
            block
                .child("extLst")
                .and_then(|el| el.kids("ext").find(|ext| ext.attr("uri").is_some_and(|uri| uri.eq_ignore_ascii_case(RICH_EXT))))
                .and_then(|ext| ext.child("rvb"))
                .and_then(|rvb| rvb.attr_u32("i"))
                .map(|n| n as usize)
        })
        .collect();
    let Some(blocks) = metadata.child("valueMetadata") else { return Ok(vec![]) };
    let blocks = entries(cx, blocks, "bk")?;
    Ok(blocks
        .into_iter()
        .map(|block| {
            block
                .kids("rc")
                .find(|rc| rc.attr_u32("t").map(|n| n as usize) == Some(type_id))
                .and_then(|rc| rc.attr_u32("v"))
                .and_then(|n| future.get(n as usize))
                .copied()
                .flatten()
        })
        .collect())
}

fn is_linked(structure: &El) -> bool {
    matches!(structure.attr("t"), Some("_linkedentity" | "_linkedentitycore"))
}

/// Linked data type records (Excel's Stocks, Currencies…) by the worksheet's one-based `vm`
/// block. Fields of a type other than number, text or boolean (icons, hyperlinks, format bags)
/// are not kept.
pub fn read_linked(cx: &mut Ctx<'_>, workbook_rels: &[Rel]) -> Result<Vec<Option<Arc<LinkedRecord>>>, IoError> {
    let Some(metadata_path) = part(cx, workbook_rels, "sheetMetadata") else { return Ok(vec![]) };
    let mut rels = workbook_rels.to_vec();
    rels.extend(cx.pkg.rels(&metadata_path)?);
    let Some(data_path) = part(cx, &rels, "rdRichValue") else { return Ok(vec![]) };
    rels.extend(cx.pkg.rels(&data_path)?);
    let Some(structure_path) = part(cx, &rels, "rdRichValueStructure") else { return Ok(vec![]) };
    let Some(structures) = read_xml(cx, &structure_path, "rvStructures")? else { return Ok(vec![]) };
    let structures = entries(cx, &structures, "s")?;
    if !structures.iter().any(|s| is_linked(s)) {
        return Ok(vec![]);
    }
    let Some(metadata) = read_xml(cx, &metadata_path, "metadata")? else { return Ok(vec![]) };
    let Some(types) = metadata.child("metadataTypes") else { return Ok(vec![]) };
    let types = entries(cx, types, "metadataType")?;
    let Some(type_id) = types.iter().position(|el| el.attr("name") == Some("XLRICHVALUE")).map(|n| n + 1) else { return Ok(vec![]) };
    let Some(future) = metadata.kids("futureMetadata").find(|el| el.attr("name") == Some("XLRICHVALUE")) else { return Ok(vec![]) };
    let Some(data) = read_xml(cx, &data_path, "rvData")? else { return Ok(vec![]) };
    let values = entries(cx, &data, "rv")?;
    let cores: Vec<Option<Arc<LinkedRecord>>> = values
        .iter()
        .map(|value| {
            let structure = value.attr_u32("s").and_then(|n| structures.get(n as usize))?;
            if structure.attr("t") == Some("_linkedentitycore") { linked_core(structure, value).map(Arc::new) } else { None }
        })
        .collect();
    let resolved: Vec<Option<Arc<LinkedRecord>>> = values
        .iter()
        .zip(&cores)
        .map(|(value, core)| {
            let structure = value.attr_u32("s").and_then(|n| structures.get(n as usize))?;
            match structure.attr("t") {
                Some("_linkedentitycore") => core.clone(),
                // The wrapper's `%cvi` points at the rich value holding the record.
                Some("_linkedentity") => {
                    let at = structure.kids("k").position(|k| k.attr("n") == Some("%cvi"))?;
                    let n = value.kids("v").nth(at)?.text.trim().parse::<usize>().ok()?;
                    cores.get(n).cloned().flatten()
                }
                _ => None,
            }
        })
        .collect();
    let indices = block_indices(cx, &metadata, type_id, future)?;
    let blocks = indices.into_iter().map(|index| index.and_then(|n| resolved.get(n)).cloned().flatten()).collect();
    Ok(blocks)
}

fn linked_core(structure: &El, value: &El) -> Option<LinkedRecord> {
    let keys: Vec<_> = structure.kids("k").collect();
    let values: Vec<_> = value.kids("v").collect();
    if keys.len() != values.len() {
        return None;
    }
    let mut fields = Vec::new();
    for (key, value) in keys.iter().zip(&values) {
        let (Some(name), Some(kind)) = (key.attr("n"), key.attr("t")) else { continue };
        if matches!(kind, "d" | "i" | "s" | "b") {
            fields.push(LinkedField { name: name.to_string(), kind: kind.to_string(), value: value.text.clone() });
        }
    }
    let display = fields.iter().find(|f| f.name == "_DisplayString" && f.kind == "s").map(|f| f.value.clone()).unwrap_or_default();
    Some(LinkedRecord { display, fields })
}

fn read_value(
    cx: &mut Ctx<'_>,
    value: &El,
    structures: &[&El],
    image_rel_list: &[&El],
    image_rels: &[Rel],
    cache: &mut HashMap<(String, String), Arc<CellPicture>>,
    total: &mut usize,
) -> Result<Option<Arc<CellPicture>>, IoError> {
    let Some(structure) = value.attr_u32("s").and_then(|n| structures.get(n as usize)) else { return Ok(None) };
    if structure.attr("t") != Some("_localImage") {
        return Ok(None);
    }
    let keys: Vec<_> = structure.kids("k").collect();
    let values: Vec<_> = value.kids("v").collect();
    if keys.len() != values.len() {
        return Ok(None);
    }
    let lookup = |name: &str, ty: &str| {
        let mut matches = keys.iter().zip(&values).filter(|(k, _)| k.attr("n") == Some(name));
        let (key, value) = matches.next()?;
        (matches.next().is_none() && key.attr("t") == Some(ty)).then_some(value.text.as_str())
    };
    let Some(rel_id) = lookup("_rvRel:LocalImageIdentifier", "i")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .and_then(|n| image_rel_list.get(n))
        .and_then(|rel| rel.attr("id"))
    else {
        return Ok(None);
    };
    let mut matches = image_rels.iter().filter(|rel| rel.id == rel_id);
    let Some(rel) = matches.next() else { return Ok(None) };
    if matches.next().is_some() || rel.kind != "image" || rel.external {
        cx.warn("a cell picture has an external or invalid image relationship; kept its scalar fallback");
        return Ok(None);
    }
    let alt = lookup("Text", "s").unwrap_or("").to_string();
    let key = (normalize(&rel.target), alt.clone());
    if let Some(picture) = cache.get(&key) {
        return Ok(Some(picture.clone()));
    }
    let data = match cx.pkg.read_limited(&rel.target, MAX_IMAGE) {
        Ok(Some(data)) => data,
        other => {
            cx.warn(format!(
                "missing, damaged or oversized cell picture {}: {}; kept its scalar fallback",
                rel.target,
                other.err().map(|e| e.to_string()).unwrap_or_default()
            ));
            return Ok(None);
        }
    };
    let Some((mime, _)) = image_type(&data) else {
        cx.warn("a cell picture is not a supported PNG/JPEG or exceeds 8192 pixels per side / 16 million pixels; kept its scalar fallback");
        return Ok(None);
    };
    if total.saturating_add(data.len()) > MAX_IMAGES_TOTAL {
        cx.warn("cell pictures exceed the 128 MiB workbook limit; remaining pictures kept their scalar fallbacks");
        return Ok(None);
    }
    *total += data.len();
    let picture = Arc::new(CellPicture { data, mime: mime.to_string(), alt });
    cache.insert(key, picture.clone());
    Ok(Some(picture))
}

enum Item {
    Picture(Arc<CellPicture>),
    Linked(Arc<LinkedRecord>),
}

/// One rich value per shared model payload; copied cells can reuse a metadata block.
#[derive(Default)]
pub struct PicturesOut {
    items: Vec<Item>,
    indices: HashMap<*const (), u32>,
}

impl PicturesOut {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn index(&mut self, picture: &Arc<CellPicture>) -> u32 {
        self.push(Arc::as_ptr(picture).cast(), Item::Picture(picture.clone()))
    }
    pub fn index_linked(&mut self, record: &Arc<LinkedRecord>) -> u32 {
        self.push(Arc::as_ptr(record).cast(), Item::Linked(record.clone()))
    }
    fn push(&mut self, key: *const (), item: Item) -> u32 {
        if let Some(index) = self.indices.get(&key) {
            return *index;
        }
        let index = self.items.len().saturating_add(1) as u32;
        self.items.push(item);
        self.indices.insert(key, index);
        index
    }
}

/// The structure element for a linked record's fields.
fn core_structure(record: &LinkedRecord) -> String {
    let mut xml = String::from("<s t=\"_linkedentitycore\">");
    for field in &record.fields {
        let _ = write!(xml, "<k n=\"{}\" t=\"{}\"/>", esc_attr(&field.name), esc_attr(&field.kind));
    }
    xml.push_str("</s>");
    xml
}

pub fn write(out: &mut Out, workbook_rels: &mut Rels) -> Result<(), IoError> {
    let items = std::mem::take(&mut out.cell_pictures).items;
    // A linked record emits two `rv` elements (its core and its wrapper), so count what is written.
    let emitted = items.iter().fold(0usize, |n, item| n.saturating_add(if matches!(item, Item::Linked(_)) { 2 } else { 1 }));
    if emitted > MAX_RECORDS {
        return Err(IoError::TooLarge("too many cell pictures".into()));
    }
    let has_pictures = items.iter().any(|item| matches!(item, Item::Picture(_)));
    let mut rel_list = format!("{XML_DECL}<richValueRels xmlns=\"{NS_IMAGE_REL}\" xmlns:r=\"{NS_REL}\">");
    let mut image_rels = Rels::default();
    let mut total = 0usize;
    let mut pictures = 0usize;
    let mut structures: Vec<String> = vec![];
    if has_pictures {
        structures.push(
            "<s t=\"_localImage\"><k n=\"_rvRel:LocalImageIdentifier\" t=\"i\"/><k n=\"CalcOrigin\" t=\"i\"/><k n=\"Text\" t=\"s\"/></s>".into(),
        );
    }
    // `_linkedentity` wraps the `_linkedentitycore` record, which has one structure per key set.
    let mut wrapper: Option<usize> = None;
    let mut cores: HashMap<String, usize> = HashMap::new();
    let mut values = String::new();
    let mut rv_count = 0usize;
    // The rich value each item's cell points at (a linked record's wrapper, after its core).
    let mut cell_values = Vec::with_capacity(items.len());
    for item in &items {
        match item {
            Item::Picture(picture) => {
                total = total.saturating_add(picture.data.len());
                if picture.data.len() as u64 > MAX_IMAGE || total > MAX_IMAGES_TOTAL {
                    return Err(IoError::TooLarge("cell pictures exceed the 16 MiB image or 128 MiB workbook limit".into()));
                }
                let (mime, ext) = image_type(&picture.data).ok_or_else(|| {
                    IoError::Format("cell pictures must be PNG/JPEG with at most 8192 pixels per side and 16 million pixels".into())
                })?;
                pictures += 1;
                let image_name = format!("cellImage{pictures}.{ext}");
                out.default_type(ext, mime);
                out.part(&format!("xl/media/{image_name}"), None, picture.data.clone());
                let rid = image_rels.add("image", &format!("../media/{image_name}"));
                let _ = write!(rel_list, "<rel r:id=\"{rid}\"/>");
                let _ = write!(values, "<rv s=\"0\"><v>{}</v><v>5</v><v>{}</v></rv>", pictures - 1, esc(&picture.alt));
                cell_values.push(rv_count);
                rv_count += 1;
            }
            Item::Linked(record) => {
                let core = core_structure(record);
                let core_index = *cores.entry(core.clone()).or_insert_with(|| {
                    structures.push(core);
                    structures.len() - 1
                });
                let wrapper_index = *wrapper.get_or_insert_with(|| {
                    structures.push("<s t=\"_linkedentity\"><k n=\"%cvi\" t=\"r\"/></s>".into());
                    structures.len() - 1
                });
                let _ = write!(values, "<rv s=\"{core_index}\">");
                for field in &record.fields {
                    let _ = write!(values, "<v>{}</v>", esc(&field.value));
                }
                let _ = write!(values, "</rv><rv s=\"{wrapper_index}\"><v>{rv_count}</v></rv>");
                cell_values.push(rv_count + 1);
                rv_count += 2;
            }
        }
    }
    let data = format!("{XML_DECL}<rvData xmlns=\"{NS_RICH}\" count=\"{rv_count}\">{values}</rvData>");
    rel_list.push_str("</richValueRels>");
    let structures = format!("{XML_DECL}<rvStructures xmlns=\"{NS_RICH}\" count=\"{}\">{}</rvStructures>", structures.len(), structures.concat());
    let mut parts = vec![
        (
            "rdrichvalue.xml",
            "application/vnd.ms-excel.rdRichValue+xml",
            "http://schemas.microsoft.com/office/2017/06/relationships/rdRichValue",
            data.into_bytes(),
        ),
        (
            "rdrichvaluestructure.xml",
            "application/vnd.ms-excel.rdRichValueStructure+xml",
            "http://schemas.microsoft.com/office/2017/06/relationships/rdRichValueStructure",
            structures.into_bytes(),
        ),
    ];
    if has_pictures {
        parts.push((
            "richValueRel.xml",
            "application/vnd.ms-excel.richvaluerel+xml",
            "http://schemas.microsoft.com/office/2022/10/relationships/richValueRel",
            rel_list.into_bytes(),
        ));
    }
    for (name, content_type, relation, bytes) in parts {
        out.part(&format!("xl/richData/{name}"), Some(content_type), bytes);
        workbook_rels.add_uri(relation, &format!("richData/{name}"));
    }
    if has_pictures {
        out.part("xl/richData/_rels/richValueRel.xml.rels", None, image_rels.xml().into_bytes());
    }
    out.part(
        "xl/metadata.xml",
        Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml"),
        metadata(&cell_values, out.dynamic_arrays).into_bytes(),
    );
    workbook_rels.add("sheetMetadata", "metadata.xml");
    Ok(())
}

/// `rich_values[i]` is the rich value that value-metadata block `i` (the cell's `vm`, minus one) holds.
fn metadata(rich_values: &[usize], dynamic: bool) -> String {
    let count = rich_values.len();
    let type_id = if dynamic { 2 } else { 1 };
    let mut xml = format!(
        "{XML_DECL}<metadata xmlns=\"{NS_MAIN}\" xmlns:rv=\"{NS_RICH}\" xmlns:xda=\"http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray\"><metadataTypes count=\"{type_id}\">"
    );
    if dynamic {
        xml.push_str("<metadataType name=\"XLDAPR\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" coerce=\"1\" cellMeta=\"1\"/>");
    }
    xml.push_str("<metadataType name=\"XLRICHVALUE\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" cellMeta=\"0\"/></metadataTypes>");
    if dynamic {
        xml.push_str("<futureMetadata name=\"XLDAPR\" count=\"1\"><bk><extLst><ext uri=\"{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}\"><xda:dynamicArrayProperties fDynamic=\"1\" fCollapsed=\"0\"/></ext></extLst></bk></futureMetadata>");
    }
    let _ = write!(xml, "<futureMetadata name=\"XLRICHVALUE\" count=\"{count}\">");
    for rich_value in rich_values {
        let _ = write!(xml, "<bk><extLst><ext uri=\"{RICH_EXT}\"><rv:rvb i=\"{rich_value}\"/></ext></extLst></bk>");
    }
    xml.push_str("</futureMetadata>");
    if dynamic {
        xml.push_str("<cellMetadata count=\"1\"><bk><rc t=\"1\" v=\"0\"/></bk></cellMetadata>");
    }
    let _ = write!(xml, "<valueMetadata count=\"{count}\">");
    for i in 0..count {
        let _ = write!(xml, "<bk><rc t=\"{type_id}\" v=\"{i}\"/></bk>");
    }
    xml.push_str("</valueMetadata></metadata>");
    xml
}
