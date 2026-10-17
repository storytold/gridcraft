//! Static in-cell PNG/JPEG images and the newer error values (`#SPILL!`, `#CALC!`), from the
//! public MS-XLSX rich-data schemas. Excel caches a newer error as the cell value `#VALUE!` plus
//! value metadata pointing at an `_error` rich value that carries the real error type.
//! `vm` and metadata type IDs are one-based; every subsequent table index is zero-based.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_core::CellError;
use gridcraft_model::CellPicture;

use crate::IoError;
use crate::package::{Rel, normalize};
use crate::read::Ctx;
use crate::write::{NS_MAIN, NS_REL, Out, Rels, XML_DECL};
use crate::xml::{self, El, esc};

const NS_RICH: &str = "http://schemas.microsoft.com/office/spreadsheetml/2017/richdata";
const NS_IMAGE_REL: &str = "http://schemas.microsoft.com/office/spreadsheetml/2022/richvaluerel";
// Lower case, as Excel writes them: Excel refuses a file that spells these extension URIs in
// upper case.
const RICH_EXT: &str = "{3e2802c4-a4d2-4d8b-9148-e3be6c30e623}";
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

/// What a cell's value metadata (`vm`) resolves to.
#[derive(Clone, Debug)]
pub enum RichCell {
    Picture(Arc<CellPicture>),
    /// The real error behind a cached `#VALUE!`.
    Error(CellError),
}

/// Resolves every value-metadata block without assuming the indices at each layer coincide.
pub fn read(cx: &mut Ctx<'_>, workbook_rels: &[Rel]) -> Result<Vec<Option<RichCell>>, IoError> {
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
    let Some(structure_path) = part(cx, &rels, "rdRichValueStructure") else {
        cx.warn("missing rich-value structure relationship; cell pictures were not loaded");
        return Ok(vec![]);
    };
    let Some(data) = read_xml(cx, &data_path, "rvData")? else { return Ok(vec![]) };
    let Some(structures) = read_xml(cx, &structure_path, "rvStructures")? else { return Ok(vec![]) };
    // Only pictures need the image relationships; a package holding error values alone has none.
    let (image_rel_list, image_rels) = match part(cx, &rels, "richValueRel") {
        Some(path) => (read_xml(cx, &path, "richValueRels")?, cx.pkg.rels(&path)?),
        None => (None, vec![]),
    };
    let structures = entries(cx, &structures, "s")?;
    let image_rel_list = match &image_rel_list {
        Some(list) => entries(cx, list, "rel")?,
        None => vec![],
    };
    let values = entries(cx, &data, "rv")?;
    let mut pictures = Vec::with_capacity(values.len());
    let mut cache = HashMap::new();
    let mut total = 0usize;
    for value in values {
        let picture = read_value(cx, value, &structures, &image_rel_list, &image_rels, &mut cache, &mut total)?;
        if picture.is_none() {
            cx.warn("some rich values are unsupported or malformed; their scalar fallbacks were retained (cell pictures may be missing)");
        }
        pictures.push(picture);
    }
    let Some(future) = metadata.kids("futureMetadata").find(|el| el.attr("name") == Some("XLRICHVALUE")) else {
        cx.warn("missing XLRICHVALUE future metadata; cell pictures were not loaded");
        return Ok(vec![]);
    };
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
                .and_then(|n| pictures.get(n))
                .cloned()
                .flatten()
        })
        .collect())
}

fn read_value(
    cx: &mut Ctx<'_>,
    value: &El,
    structures: &[&El],
    image_rel_list: &[&El],
    image_rels: &[Rel],
    cache: &mut HashMap<(String, String), Arc<CellPicture>>,
    total: &mut usize,
) -> Result<Option<RichCell>, IoError> {
    let Some(structure) = value.attr_u32("s").and_then(|n| structures.get(n as usize)) else { return Ok(None) };
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
    match structure.attr("t") {
        Some("_localImage") => {}
        // Error kinds GridCraft does not model (#FIELD!, #BLOCKED!...) keep the #VALUE! fallback.
        Some("_error") => {
            let kind = lookup("errorType", "i").and_then(|v| v.trim().parse::<u32>().ok()).and_then(error_kind);
            return Ok(kind.map(RichCell::Error));
        }
        _ => return Ok(None),
    }
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
        return Ok(Some(RichCell::Picture(picture.clone())));
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
    Ok(Some(RichCell::Picture(picture)))
}

/// The `_error` `errorType` is zero-based where `ERROR.TYPE` is one-based (#SPILL! 8, #CALC! 13).
fn error_kind(error_type: u32) -> Option<CellError> {
    CellError::ALL.into_iter().find(|e| e.code() == error_type.saturating_add(1))
}

/// Whether the file format stores this error only as a rich value behind a cached `#VALUE!`.
pub fn is_rich_error(error: CellError) -> bool {
    matches!(error, CellError::Spill | CellError::Calc)
}

/// One rich value per shared model payload or error kind; cells reuse a metadata block.
#[derive(Default)]
pub struct RichValuesOut {
    values: Vec<RichCell>,
    pictures: HashMap<*const CellPicture, u32>,
    errors: HashMap<CellError, u32>,
}

impl RichValuesOut {
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    /// The one-based `vm` of a cell picture.
    pub fn picture(&mut self, picture: &Arc<CellPicture>) -> u32 {
        let key = Arc::as_ptr(picture);
        if let Some(index) = self.pictures.get(&key) {
            return *index;
        }
        let index = self.push(RichCell::Picture(picture.clone()));
        self.pictures.insert(key, index);
        index
    }
    /// The one-based `vm` of an error value (see [`is_rich_error`]).
    pub fn error(&mut self, error: CellError) -> u32 {
        if let Some(index) = self.errors.get(&error) {
            return *index;
        }
        let index = self.push(RichCell::Error(error));
        self.errors.insert(error, index);
        index
    }
    fn push(&mut self, value: RichCell) -> u32 {
        self.values.push(value);
        self.values.len().min(u32::MAX as usize) as u32
    }
}

pub fn write(out: &mut Out, workbook_rels: &mut Rels) -> Result<(), IoError> {
    let values = std::mem::take(&mut out.rich_values).values;
    if values.len() > MAX_RECORDS {
        return Err(IoError::TooLarge("too many cell pictures".into()));
    }
    let count = values.len();
    let has_pictures = values.iter().any(|v| matches!(v, RichCell::Picture(_)));
    let has_errors = values.iter().any(|v| matches!(v, RichCell::Error(_)));
    // Structures are listed pictures first, so the error structure follows the image one.
    let error_structure = usize::from(has_pictures);
    let mut data = format!("{XML_DECL}<rvData xmlns=\"{NS_RICH}\" count=\"{count}\">");
    let mut rel_list = format!("{XML_DECL}<richValueRels xmlns=\"{NS_IMAGE_REL}\" xmlns:r=\"{NS_REL}\">");
    let mut image_rels = Rels::default();
    let mut total = 0usize;
    let mut i = 0usize;
    for value in &values {
        let picture = match value {
            RichCell::Picture(picture) => picture,
            RichCell::Error(error) => {
                let _ = write!(data, "<rv s=\"{error_structure}\"><v>{}</v></rv>", error.code().saturating_sub(1));
                continue;
            }
        };
        total = total.saturating_add(picture.data.len());
        if picture.data.len() as u64 > MAX_IMAGE || total > MAX_IMAGES_TOTAL {
            return Err(IoError::TooLarge("cell pictures exceed the 16 MiB image or 128 MiB workbook limit".into()));
        }
        let (mime, ext) = image_type(&picture.data)
            .ok_or_else(|| IoError::Format("cell pictures must be PNG/JPEG with at most 8192 pixels per side and 16 million pixels".into()))?;
        let image_name = format!("cellImage{}.{}", i + 1, ext);
        out.default_type(ext, mime);
        out.part(&format!("xl/media/{image_name}"), None, picture.data.clone());
        let rid = image_rels.add("image", &format!("../media/{image_name}"));
        let _ = write!(rel_list, "<rel r:id=\"{rid}\"/>");
        let _ = write!(data, "<rv s=\"0\"><v>{i}</v><v>5</v><v>{}</v></rv>", esc(&picture.alt));
        i += 1;
    }
    data.push_str("</rvData>");
    rel_list.push_str("</richValueRels>");
    let mut structures = format!("{XML_DECL}<rvStructures xmlns=\"{NS_RICH}\" count=\"{}\">", error_structure + usize::from(has_errors));
    if has_pictures {
        structures
            .push_str("<s t=\"_localImage\"><k n=\"_rvRel:LocalImageIdentifier\" t=\"i\"/><k n=\"CalcOrigin\" t=\"i\"/><k n=\"Text\" t=\"s\"/></s>");
    }
    if has_errors {
        structures.push_str("<s t=\"_error\"><k n=\"errorType\" t=\"i\"/></s>");
    }
    structures.push_str("</rvStructures>");
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
        metadata(count, out.dynamic_arrays).into_bytes(),
    );
    workbook_rels.add("sheetMetadata", "metadata.xml");
    Ok(())
}

fn metadata(count: usize, dynamic: bool) -> String {
    let type_id = if dynamic { 2 } else { 1 };
    let mut xml = format!(
        "{XML_DECL}<metadata xmlns=\"{NS_MAIN}\" xmlns:rv=\"{NS_RICH}\" xmlns:xda=\"http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray\"><metadataTypes count=\"{type_id}\">"
    );
    if dynamic {
        xml.push_str("<metadataType name=\"XLDAPR\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" coerce=\"1\" cellMeta=\"1\"/>");
    }
    xml.push_str("<metadataType name=\"XLRICHVALUE\" minSupportedVersion=\"120000\" copy=\"1\" pasteAll=\"1\" pasteValues=\"1\" merge=\"1\" splitFirst=\"1\" rowColShift=\"1\" clearFormats=\"1\" clearComments=\"1\" assign=\"1\" coerce=\"1\"/></metadataTypes>");
    if dynamic {
        xml.push_str("<futureMetadata name=\"XLDAPR\" count=\"1\"><bk><extLst><ext uri=\"{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}\"><xda:dynamicArrayProperties fDynamic=\"1\" fCollapsed=\"0\"/></ext></extLst></bk></futureMetadata>");
    }
    let _ = write!(xml, "<futureMetadata name=\"XLRICHVALUE\" count=\"{count}\">");
    for i in 0..count {
        let _ = write!(xml, "<bk><extLst><ext uri=\"{RICH_EXT}\"><rv:rvb i=\"{i}\"/></ext></extLst></bk>");
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
