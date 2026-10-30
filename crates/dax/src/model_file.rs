//! Reader/writer for the Spreadsheet Data Model container (`xl/model/item.data`).
//!
//! Excel stores a Power Pivot model as an Analysis Services backup stream in
//! `xl/model/item.data`. The container is page aligned (4096 bytes) and has three sections:
//!
//! 1. a fixed one-page header: a UTF-16LE byte order mark, the
//!    `STREAM_STORAGE_SIGNATURE_)!@#$%^&*(` signature, then a `<BackupLog>` XML document padded
//!    with zeros;
//! 2. the files section, beginning with the UTF-16LE BOM and the `<Partitions>` document, then
//!    every file. Each file is followed by a four-byte CRC-32 trailer. Files other than the
//!    partitions document and the backup log are stored as a sequence of *xpress8* chunks:
//!    `u16 uncompressed_len`, `u16 compressed_len`, then the body. When the two lengths are equal
//!    the body is stored verbatim; otherwise it is a raw MS-XCA plain LZ77 (XPRESS) stream. A
//!    chunk is at most 4096 uncompressed bytes in the files Excel writes;
//! 3. the virtual directory: a UTF-16LE `<VirtualDirectory>` document (no BOM) listing each
//!    file's opaque storage path, stored size and offset, padded to the next page.
//!
//! Measure definitions are not separate files: the cube's `MdxScript` file stores them in the
//! `-- PowerPivot measures command --` command text as
//!
//! ```text
//! CREATE MEASURE 'Sales'[Total Sales]=SUM(Sales[Amount]);
//! ```
//!
//! and the sibling `<CalculationProperties>` element stores the display format. This module
//! reads those definitions and rewrites the script while preserving every other file's stored
//! bytes, the backup log's manifests and the virtual directory's bookkeeping.
//!
//! The codec is the plain LZ77 variant of `[MS-XCA]` section 2.3/2.4, not the LZ77+Huffman
//! variant. `write_measures` stores the rewritten script as verbatim (uncompressed) xpress8
//! chunks, which the format permits; every untouched file keeps its original bytes and CRC.

use std::borrow::Cow;

use quick_xml::escape::unescape;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer, XmlVersion};
use serde::{Deserialize, Serialize};

mod model_data;
mod model_io;
mod vertipaq;
pub mod xpress8;

pub use model_data::{ModelColumnData, ModelData, ModelRelationship, ModelTableData, RelationshipMultiplicity};
pub use model_io::{ModelColumnMetrics, column_metrics, file_contents, read_model, write_model};

/// Page size of the Spreadsheet Data Model container.
pub const PAGE_SIZE: usize = 4096;

/// Signature stored after the header BOM.
const SIGNATURE: &str = "STREAM_STORAGE_SIGNATURE_)!@#$%^&*(";

/// Maximum container size accepted by the parser (512 MiB).
const MAX_CONTAINER_BYTES: usize = 512 * 1024 * 1024;
/// Maximum virtual-directory XML size accepted (16 MiB).
const MAX_VDIR_BYTES: usize = 16 * 1024 * 1024;
/// Maximum number of files listed by the virtual directory.
const MAX_FILES: usize = 100_000;
/// Maximum size of one decompressed embedded file (256 MiB).
const MAX_FILE_CONTENT: usize = 256 * 1024 * 1024;
/// Maximum XML element nesting accepted in the container's metadata.
const MAX_XML_DEPTH: usize = 256;
/// Number of uncompressed bytes per chunk when writing.
const WRITE_CHUNK: usize = PAGE_SIZE;

/// A measure definition read from a Power Pivot model.
///
/// `table` is the measure's home table, `name` the measure name, `formula` the DAX expression
/// and `format` the Excel number-format string (`""` for General).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelMeasure {
    pub table: String,
    pub name: String,
    pub formula: String,
    #[serde(default)]
    pub format: String,
}

/// Errors returned by the model-file reader/writer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ModelFileError {
    /// The input is not a Spreadsheet Data Model stream.
    #[error("not a Spreadsheet Data Model stream: {0}")]
    Format(String),
    /// The container is structurally corrupt.
    #[error("corrupt model file: {0}")]
    Corrupt(String),
    /// A compressed file cannot be decoded.
    #[error("invalid Xpress data: {0}")]
    Compression(String),
    /// The model has no MdxScript file to hold measures.
    #[error("model contains no MdxScript file")]
    NoMeasureScript,
    /// A measure has an empty name, table or formula.
    #[error("invalid measure: {0}")]
    InvalidMeasure(String),
    /// The requested output would exceed the configured limits.
    #[error("model file exceeds the supported size limit")]
    TooLarge,
    /// The model contains a construct this reader or writer does not implement.
    #[error("unsupported model construct: {0}")]
    Unsupported(String),
    /// The caller's model cannot be written into this container.
    #[error("invalid model: {0}")]
    InvalidModel(String),
}

type Result<T, E = ModelFileError> = std::result::Result<T, E>;

// ---------------------------------------------------------------------------------------------
// CRC-32
// ---------------------------------------------------------------------------------------------

/// Builds the CRC table for the non-reflected CRC-32 described by [MS-XLDM] section 2.1.2.2.1.1.
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut value = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 0x8000_0000 != 0 { (value << 1) ^ 0x04C1_1DB7 } else { value << 1 };
            bit += 1;
        }
        table[i] = value;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = crc_table();

/// CRC-32 used by the container's file-end markers: poly `0x04C11DB7`, init `0xFFFFFFFF`,
/// final XOR `0xFFFFFFFF`, computed over the stored bytes of the file.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        let index = ((crc >> 24) as u8 ^ byte) as usize;
        crc = (crc << 8) ^ CRC_TABLE.get(index).copied().unwrap_or(0);
    }
    crc ^ 0xFFFF_FFFF
}

// ---------------------------------------------------------------------------------------------
// Plain XPRESS (MS-XCA LZ77) decoder
// ---------------------------------------------------------------------------------------------

fn read_u16(input: &[u8], pos: &mut usize) -> Result<u16> {
    let end = pos.checked_add(2).ok_or_else(|| ModelFileError::Compression("offset overflow".into()))?;
    let bytes = input.get(*pos..end).ok_or_else(|| ModelFileError::Compression("truncated stream".into()))?;
    *pos = end;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Decodes one raw MS-XCA plain LZ77 buffer into exactly `expected` bytes.
///
/// This is the `COMPRESSION_FORMAT_XPRESS` (3) variant: 32-bit flag words, most significant bit
/// first, a literal byte for a zero flag and a two-byte match token for a one flag. Every read
/// and copy is bounds checked and the output length is bounded by the caller.
fn xpress_decompress(input: &[u8], expected: usize) -> Result<Vec<u8>> {
    if expected > MAX_FILE_CONTENT {
        return Err(ModelFileError::TooLarge);
    }
    xpress8::decompress(input, expected).map_err(ModelFileError::Compression)
}

// ---------------------------------------------------------------------------------------------
// UTF-16 and XML helpers
// ---------------------------------------------------------------------------------------------

fn decode_utf16(bytes: &[u8], strip_bom: bool) -> Result<String> {
    if !bytes.len().is_multiple_of(2) {
        return Err(ModelFileError::Corrupt("odd UTF-16 byte length".into()));
    }
    let mut units = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        units.push(u16::from_le_bytes([bytes[index], bytes[index + 1]]));
        index += 2;
    }
    let mut start = 0usize;
    if strip_bom && units.first() == Some(&0xFEFF) {
        start = 1;
    }
    String::from_utf16(&units[start..]).map_err(|_| ModelFileError::Corrupt("invalid UTF-16 text".into()))
}

fn encode_utf16(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(text.len().saturating_mul(2));
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// Returns the text of the first `<tag>` element in `xml` (no attributes are expected).
fn tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?.checked_add(open.len())?;
    let end = xml.get(start..)?.find(&close)?.checked_add(start)?;
    xml.get(start..end)
}

fn tag_text_required(xml: &str, tag: &str) -> Result<String> {
    tag_text(xml, tag).map(|value| value.trim().to_string()).ok_or_else(|| ModelFileError::Corrupt(format!("missing <{tag}> in model metadata")))
}

fn parse_usize_tag(xml: &str, tag: &str) -> Result<usize> {
    let value = tag_text_required(xml, tag)?;
    value.parse::<usize>().map_err(|_| ModelFileError::Corrupt(format!("invalid <{tag}> value")))
}

fn parse_bool_tag(xml: &str, tag: &str) -> Result<bool> {
    let value = tag_text_required(xml, tag)?;
    match value.as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(ModelFileError::Corrupt(format!("invalid <{tag}> boolean"))),
    }
}

/// Replaces the text of one element, preserving the tag itself.
fn replace_tag_text(xml: &str, tag: &str, value: &str) -> Result<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open).ok_or_else(|| ModelFileError::Corrupt(format!("missing <{tag}>")))?;
    let content_start = start.checked_add(open.len()).ok_or_else(|| ModelFileError::Corrupt("offset overflow".into()))?;
    let content_end = xml
        .get(content_start..)
        .and_then(|rest| rest.find(&close))
        .and_then(|relative| content_start.checked_add(relative))
        .ok_or_else(|| ModelFileError::Corrupt(format!("missing </{tag}>")))?;
    let mut out = String::with_capacity(xml.len().saturating_add(value.len()));
    out.push_str(xml.get(..content_start).ok_or_else(|| ModelFileError::Corrupt("slice".into()))?);
    out.push_str(value);
    out.push_str(xml.get(content_end..).ok_or_else(|| ModelFileError::Corrupt("slice".into()))?);
    Ok(out)
}

fn esc(text: &str) -> String {
    quick_xml::escape::escape(text).into_owned()
}

// ---------------------------------------------------------------------------------------------
// Minimal XML tree used for the virtual directory, the backup log and the MdxScript document
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    text: String,
    children: Vec<XmlNode>,
}

impl XmlNode {
    fn local_name(&self) -> &str {
        local_name(&self.name)
    }

    fn child(&self, name: &str) -> Option<&XmlNode> {
        self.children.iter().find(|child| child.local_name() == name)
    }

    fn find_descendant(&self, name: &str) -> Option<&XmlNode> {
        if self.local_name() == name {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find_descendant(name))
    }

    fn find_descendant_mut(&mut self, name: &str) -> Option<&mut XmlNode> {
        if self.local_name() == name {
            return Some(self);
        }
        self.children.iter_mut().find_map(|child| child.find_descendant_mut(name))
    }
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn utf8(bytes: &[u8]) -> Result<String> {
    std::str::from_utf8(bytes).map(|value| value.to_string()).map_err(|_| ModelFileError::Corrupt("non-UTF-8 XML name".into()))
}

fn xml_error(error: impl std::fmt::Display) -> ModelFileError {
    ModelFileError::Corrupt(format!("XML parse error: {error}"))
}

fn parse_xml(xml: &str) -> Result<XmlNode> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<XmlNode> = Vec::new();
    let mut root: Option<XmlNode> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(event)) => {
                if stack.len() >= MAX_XML_DEPTH {
                    return Err(ModelFileError::Corrupt("XML nesting is too deep".into()));
                }
                stack.push(start_node(&event, &reader)?);
            }
            Ok(Event::Empty(event)) => {
                let node = start_node(&event, &reader)?;
                attach_node(&mut stack, &mut root, node)?;
            }
            Ok(Event::End(_)) => {
                let node = stack.pop().ok_or_else(|| ModelFileError::Corrupt("unbalanced XML end tag".into()))?;
                attach_node(&mut stack, &mut root, node)?;
            }
            Ok(Event::Text(event)) => {
                let decoded = event.decode().map_err(xml_error)?;
                let text = unescape(&decoded).map_err(xml_error)?;
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(text.as_ref());
                }
            }
            Ok(Event::CData(event)) => {
                let text = event.decode().map_err(xml_error)?;
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(text.as_ref());
                }
            }
            Ok(Event::GeneralRef(event)) => {
                let Some(node) = stack.last_mut() else { continue };
                match event.resolve_char_ref() {
                    Ok(Some(character)) => node.text.push(character),
                    Ok(None) => {
                        let name = event.decode().map_err(xml_error)?;
                        if let Some(value) =
                            quick_xml::escape::resolve_predefined_entity(&name).or_else(|| quick_xml::escape::resolve_xml_entity(&name))
                        {
                            node.text.push_str(value);
                        } else {
                            node.text.push('&');
                            node.text.push_str(name.as_ref());
                            node.text.push(';');
                        }
                    }
                    Err(error) => return Err(xml_error(error)),
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(error) => return Err(xml_error(error)),
        }
    }

    if !stack.is_empty() {
        return Err(ModelFileError::Corrupt("unclosed XML element".into()));
    }
    root.ok_or_else(|| ModelFileError::Corrupt("empty XML document".into()))
}

fn start_node(event: &BytesStart<'_>, reader: &Reader<&[u8]>) -> Result<XmlNode> {
    let name = utf8(event.name().as_ref())?;
    let mut attrs = Vec::new();
    for attribute in event.attributes().with_checks(false) {
        let attribute = attribute.map_err(xml_error)?;
        let key = utf8(attribute.key.as_ref())?;
        let value = attribute.decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder()).map_err(xml_error)?.into_owned();
        attrs.push((key, value));
    }
    Ok(XmlNode { name, attrs, text: String::new(), children: Vec::new() })
}

fn attach_node(stack: &mut [XmlNode], root: &mut Option<XmlNode>, node: XmlNode) -> Result<()> {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else if root.is_none() {
        *root = Some(node);
    } else {
        return Err(ModelFileError::Corrupt("multiple XML roots".into()));
    }
    Ok(())
}

fn write_xml_node(node: &XmlNode, writer: &mut Writer<Vec<u8>>) -> Result<()> {
    let mut start = BytesStart::new(node.name.as_str());
    for (key, value) in &node.attrs {
        start.push_attribute((key.as_str(), value.as_str()));
    }
    if node.children.is_empty() && node.text.is_empty() {
        writer.write_event(Event::Empty(start)).map_err(xml_error)?;
        return Ok(());
    }
    writer.write_event(Event::Start(start)).map_err(xml_error)?;
    if !node.text.is_empty() {
        writer.write_event(Event::Text(BytesText::new(&node.text))).map_err(xml_error)?;
    }
    for child in &node.children {
        write_xml_node(child, writer)?;
    }
    writer.write_event(Event::End(BytesEnd::new(node.name.as_str()))).map_err(xml_error)?;
    Ok(())
}

fn serialize_xml(root: &XmlNode) -> Result<String> {
    let mut writer = Writer::new(Vec::new());
    write_xml_node(root, &mut writer)?;
    String::from_utf8(writer.into_inner()).map_err(|_| ModelFileError::Corrupt("XML writer produced invalid UTF-8".into()))
}

fn push_text_element(name: &str, text: &str) -> XmlNode {
    XmlNode { name: name.to_string(), attrs: Vec::new(), text: text.to_string(), children: Vec::new() }
}

// ---------------------------------------------------------------------------------------------
// Container structures
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Header {
    xml: String,
    error_code: bool,
    apply_compression: bool,
    data_offset: usize,
    vdir_offset: usize,
    vdir_size: usize,
    files: usize,
}

impl Header {
    fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < PAGE_SIZE {
            return Err(ModelFileError::Format("model stream is shorter than one page".into()));
        }
        if data.first().copied() != Some(0xFF) || data.get(1).copied() != Some(0xFE) {
            return Err(ModelFileError::Format("missing UTF-16 byte order mark".into()));
        }
        let signature = decode_utf16(data.get(2..72).ok_or_else(|| ModelFileError::Format("short signature".into()))?, true)?;
        if signature != SIGNATURE {
            return Err(ModelFileError::Format("stream storage signature mismatch".into()));
        }
        let raw = decode_utf16(data.get(72..PAGE_SIZE).ok_or_else(|| ModelFileError::Format("short header".into()))?, false)?;
        let xml = match raw.find("</BackupLog>") {
            Some(index) => raw.get(..index + "</BackupLog>".len()).unwrap_or(&raw).to_string(),
            None => return Err(ModelFileError::Corrupt("unterminated <BackupLog> header".into())),
        };
        let error_code = parse_bool_tag(&xml, "ErrorCode")?;
        let apply_compression = parse_bool_tag(&xml, "ApplyCompression")?;
        let vdir_offset = parse_usize_tag(&xml, "m_cbOffsetHeader")?;
        let vdir_size = parse_usize_tag(&xml, "DataSize")?;
        let files = parse_usize_tag(&xml, "Files")?;
        let data_offset = parse_usize_tag(&xml, "m_cbOffsetData")?;
        Ok(Self { xml, error_code, apply_compression, data_offset, vdir_offset, vdir_size, files })
    }
}

#[derive(Clone, Debug)]
struct VdirEntry {
    path: String,
    size: usize,
    offset: usize,
    delete: String,
    created: String,
    access: String,
    last_write: String,
}

fn parse_vdir(bytes: &[u8]) -> Result<Vec<VdirEntry>> {
    if bytes.len() > MAX_VDIR_BYTES {
        return Err(ModelFileError::TooLarge);
    }
    let text = decode_utf16(bytes, true)?;
    let root = parse_xml(&text)?;
    let mut entries = Vec::new();
    for child in root.children.iter().filter(|child| child.local_name() == "BackupFile") {
        if entries.len() >= MAX_FILES {
            return Err(ModelFileError::TooLarge);
        }
        let text_of = |name: &str| child.child(name).map(|node| node.text.trim().to_string()).unwrap_or_default();
        let size = text_of("Size").parse::<usize>().map_err(|_| ModelFileError::Corrupt("invalid virtual-directory size".into()))?;
        let offset = text_of("m_cbOffsetHeader").parse::<usize>().map_err(|_| ModelFileError::Corrupt("invalid virtual-directory offset".into()))?;
        let path = text_of("Path");
        if path.is_empty() {
            return Err(ModelFileError::Corrupt("virtual-directory entry has no path".into()));
        }
        entries.push(VdirEntry {
            path,
            size,
            offset,
            delete: text_of("Delete"),
            created: text_of("CreatedTimestamp"),
            access: text_of("Access"),
            last_write: text_of("LastWriteTime"),
        });
    }
    Ok(entries)
}

struct Container<'a> {
    header: Header,
    entries: Vec<VdirEntry>,
    data: &'a [u8],
}

impl<'a> Container<'a> {
    fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() > MAX_CONTAINER_BYTES {
            return Err(ModelFileError::TooLarge);
        }
        let header = Header::parse(data)?;
        if !header.vdir_offset.is_multiple_of(PAGE_SIZE) {
            return Err(ModelFileError::Corrupt("virtual directory is not page aligned".into()));
        }
        let vdir_end =
            header.vdir_offset.checked_add(header.vdir_size).ok_or_else(|| ModelFileError::Corrupt("virtual-directory offset overflow".into()))?;
        if vdir_end > data.len() || header.vdir_size > MAX_VDIR_BYTES {
            return Err(ModelFileError::Corrupt("virtual directory lies outside the file".into()));
        }
        let vdir_bytes = data.get(header.vdir_offset..vdir_end).ok_or_else(|| ModelFileError::Corrupt("virtual directory bounds".into()))?;
        let entries = parse_vdir(vdir_bytes)?;
        if entries.len() != header.files {
            return Err(ModelFileError::Corrupt(format!("virtual directory lists {} files but the header declares {}", entries.len(), header.files)));
        }
        for entry in &entries {
            let end = entry.offset.checked_add(entry.size).ok_or_else(|| ModelFileError::Corrupt("file offset overflow".into()))?;
            if end > data.len() || entry.offset < header.data_offset {
                return Err(ModelFileError::Corrupt(format!("file {} lies outside the data section", entry.path)));
            }
        }
        let container = Self { header, entries, data };
        if container.header.error_code {
            // Validate every CRC once, including files this reader never decodes: a corrupt
            // container must be rejected as a whole rather than only when a file is touched.
            for entry in &container.entries {
                let stored = container.entry_bytes(entry)?;
                if stored.len() < 4 {
                    return Err(ModelFileError::Corrupt(format!("file {} has no CRC trailer", entry.path)));
                }
                let split = stored.len() - 4;
                let payload = stored.get(..split).ok_or_else(|| ModelFileError::Corrupt("file body".into()))?;
                let trailer = stored.get(split..).ok_or_else(|| ModelFileError::Corrupt("CRC trailer".into()))?;
                let expected = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
                if crc32(payload) != expected {
                    return Err(ModelFileError::Corrupt(format!("CRC mismatch in file {}", entry.path)));
                }
            }
        }
        Ok(container)
    }

    fn entry_bytes(&self, entry: &VdirEntry) -> Result<&'a [u8]> {
        let end = entry.offset.checked_add(entry.size).ok_or_else(|| ModelFileError::Corrupt("file offset overflow".into()))?;
        self.data.get(entry.offset..end).ok_or_else(|| ModelFileError::Corrupt("file bounds".into()))
    }

    /// Decodes one embedded file, validating its CRC trailer and chunk framing.
    fn decode_entry(&self, entry: &VdirEntry) -> Result<Vec<u8>> {
        let stored = self.entry_bytes(entry)?;
        let (body, expected_crc) = if self.header.error_code {
            if stored.len() < 4 {
                return Err(ModelFileError::Corrupt(format!("file {} has no CRC trailer", entry.path)));
            }
            let split = stored.len() - 4;
            let trailer = stored.get(split..).ok_or_else(|| ModelFileError::Corrupt("CRC trailer".into()))?;
            let value = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
            (stored.get(..split).ok_or_else(|| ModelFileError::Corrupt("file body".into()))?, Some(value))
        } else {
            (stored, None)
        };
        if let Some(expected) = expected_crc {
            let actual = crc32(body);
            if actual != expected {
                return Err(ModelFileError::Corrupt(format!("CRC mismatch in file {}", entry.path)));
            }
        }
        if !self.header.apply_compression {
            return Ok(body.to_vec());
        }
        // The partitions document and the backup log are stored as UTF-16 XML with a BOM rather
        // than in xpress8 chunks.
        if body.starts_with(&[0xFF, 0xFE]) {
            return Ok(body.to_vec());
        }
        if body.is_empty() {
            return Ok(Vec::new());
        }
        decode_chunk_stream(body, &entry.path)
    }

    /// Returns the entry index of the cube's MDX script.
    ///
    /// The virtual directory only has opaque storage paths; the backup log maps them back to
    /// names such as `MdxScript.1.scr.xml`. If the log cannot be read, the files are scanned for
    /// an XML document containing `<MdxScript>`.
    fn measure_script_index(&self) -> Result<Option<usize>> {
        if let Some(log_index) = self.entries.iter().position(|entry| entry.path.eq_ignore_ascii_case("LOG"))
            && let Ok(log_content) = self.decode_entry(&self.entries[log_index])
            && let Ok(text) = log_text(&log_content)
            && let Ok(root) = parse_xml(&text)
            && let Some(index) = self.script_from_log(&root)
        {
            return Ok(Some(index));
        }
        for (index, entry) in self.entries.iter().enumerate() {
            if let Ok(content) = self.decode_entry(entry) {
                let text = content_text(&content);
                if text.contains("<MdxScript") {
                    return Ok(Some(index));
                }
            }
        }
        Ok(None)
    }

    fn script_from_log(&self, root: &XmlNode) -> Option<usize> {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if node.local_name() == "BackupFile" {
                let path = node.child("Path").map(|child| child.text.trim()).unwrap_or("");
                let storage = node.child("StoragePath").map(|child| child.text.trim()).unwrap_or("");
                let lower = path.to_ascii_lowercase();
                if (lower.contains("mdxscript") || lower.ends_with(".scr.xml"))
                    && let Some(index) = self.entries.iter().position(|entry| entry.path == storage)
                {
                    return Some(index);
                }
            }
            stack.extend(node.children.iter());
        }
        None
    }
}

/// Decodes one file body made of `u16 uncompressed_len, u16 compressed_len, body` chunks.
///
/// Equal lengths mean the body is stored verbatim. A compressed chunk must declare more than
/// eight bytes, at least eight compressed bytes and strictly fewer compressed bytes than its
/// declared output, as required by the Xpress8 acceptance rules.
fn decode_chunk_stream(body: &[u8], path: &str) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut pos = 0usize;
    while pos < body.len() {
        let uncompressed = read_u16(body, &mut pos)? as usize;
        let compressed = read_u16(body, &mut pos)? as usize;
        if compressed == 0 {
            return Err(ModelFileError::Compression(format!("zero-length chunk in file {path}")));
        }
        let end = pos.checked_add(compressed).ok_or_else(|| ModelFileError::Compression("chunk offset overflow".into()))?;
        let chunk = body.get(pos..end).ok_or_else(|| ModelFileError::Compression("truncated chunk".into()))?;
        pos = end;
        let decoded = if uncompressed == compressed {
            Cow::Borrowed(chunk)
        } else {
            if uncompressed > 8 * 1024 * 8 {
                return Err(ModelFileError::Compression("chunk declares more than 64 KiB".into()));
            }
            if uncompressed <= 8 || compressed < 8 || compressed >= uncompressed {
                return Err(ModelFileError::Compression("invalid compressed chunk lengths".into()));
            }
            Cow::Owned(xpress_decompress(chunk, uncompressed).map_err(|error| ModelFileError::Compression(format!("{path}: {error}")))?)
        };
        if output.len().checked_add(decoded.len()).is_none_or(|len| len > MAX_FILE_CONTENT) {
            return Err(ModelFileError::TooLarge);
        }
        output.extend_from_slice(&decoded);
    }
    Ok(output)
}

fn content_text(content: &[u8]) -> Cow<'_, str> {
    if content.starts_with(&[0xFF, 0xFE]) {
        match decode_utf16(content, true) {
            Ok(text) => Cow::Owned(text),
            Err(_) => Cow::Owned(String::new()),
        }
    } else {
        String::from_utf8_lossy(content)
    }
}

fn log_text(content: &[u8]) -> Result<String> {
    decode_utf16(content, true)
}

// ---------------------------------------------------------------------------------------------
// Measure parsing
// ---------------------------------------------------------------------------------------------

fn normalize_reference(reference: &str) -> String {
    let mut value = reference.trim();
    if let Some(rest) = value.strip_prefix("Measures.") {
        value = rest.trim();
    }
    if value.starts_with('[') && value.ends_with(']') && value.len() >= 2 {
        value = &value[1..value.len() - 1];
    }
    value.replace("]]", "]").trim().to_string()
}

fn find_ci(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    let rest = haystack.get(from..)?;
    if needle.len() > rest.len() {
        return None;
    }
    rest.windows(needle.len()).position(|window| window.eq_ignore_ascii_case(needle)).map(|index| index + from)
}

fn skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(|byte| byte.is_ascii_whitespace()) {
        index += 1;
    }
    index
}

fn parse_quoted_table(bytes: &[u8], start: usize) -> Option<(String, usize)> {
    let close = match *bytes.get(start)? {
        b'\'' => b'\'',
        b'[' => b']',
        _ => return None,
    };
    let mut index = start + 1;
    let mut value: Vec<u8> = Vec::new();
    loop {
        let byte = *bytes.get(index)?;
        if byte == close {
            // A doubled delimiter is an escaped delimiter inside the name.
            if bytes.get(index + 1) == Some(&close) {
                value.push(close);
                index += 2;
            } else {
                return Some((String::from_utf8(value).ok()?, index + 1));
            }
        } else {
            value.push(byte);
            index += 1;
        }
    }
}

fn parse_measure_statements(text: &str) -> Result<Vec<ModelMeasure>> {
    let bytes = text.as_bytes();
    let mut measures = Vec::new();
    let mut cursor = 0usize;
    const KEYWORD: &[u8] = b"CREATE MEASURE";
    while let Some(position) = find_ci(bytes, KEYWORD, cursor) {
        let mut index = position + KEYWORD.len();
        index = skip_ws(bytes, index);
        let (table, after_table) = parse_quoted_table(bytes, index).ok_or_else(|| {
            ModelFileError::Corrupt(format!(
                "malformed CREATE MEASURE table name near {:?}",
                text.get(position..).map(|rest| rest.get(..80).unwrap_or(rest)).unwrap_or("")
            ))
        })?;
        index = skip_ws(bytes, after_table);
        let (name, after_name) = parse_quoted_table(bytes, index).ok_or_else(|| {
            ModelFileError::Corrupt(format!(
                "malformed CREATE MEASURE measure name near {:?}",
                text.get(position..).map(|rest| rest.get(..80).unwrap_or(rest)).unwrap_or("")
            ))
        })?;
        index = skip_ws(bytes, after_name);
        if bytes.get(index).copied() != Some(b'=') {
            return Err(ModelFileError::Corrupt("CREATE MEASURE without '='".into()));
        }
        index += 1;
        let formula_start = index;
        let end = find_statement_end(bytes, formula_start);
        let formula = text.get(formula_start..end).ok_or_else(|| ModelFileError::Corrupt("measure formula bounds".into()))?.trim().to_string();
        measures.push(ModelMeasure { table, name, formula, format: String::new() });
        cursor = if end < bytes.len() { end + 1 } else { bytes.len() };
    }
    Ok(measures)
}

/// Finds the `;` terminating a measure formula, skipping quoted strings, bracketed identifiers
/// and comments.
fn find_statement_end(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    let mut in_double = false;
    let mut in_single = false;
    let mut in_bracket = false;
    let mut line_comment = false;
    let mut block_comment = false;
    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        if line_comment {
            if byte == b'\n' || byte == b'\r' {
                line_comment = false;
            }
            index += 1;
            continue;
        }
        if block_comment {
            if byte == b'*' && next == Some(b'/') {
                block_comment = false;
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if in_double {
            if byte == b'"' {
                if next == Some(b'"') {
                    index += 2;
                } else {
                    in_double = false;
                    index += 1;
                }
            } else {
                index += 1;
            }
            continue;
        }
        if in_single {
            if byte == b'\'' {
                if next == Some(b'\'') {
                    index += 2;
                } else {
                    in_single = false;
                    index += 1;
                }
            } else {
                index += 1;
            }
            continue;
        }
        if in_bracket {
            if byte == b']' {
                if next == Some(b']') {
                    index += 2;
                } else {
                    in_bracket = false;
                    index += 1;
                }
            } else {
                index += 1;
            }
            continue;
        }
        match byte {
            b'"' => {
                in_double = true;
                index += 1;
            }
            b'\'' => {
                in_single = true;
                index += 1;
            }
            b'[' => {
                in_bracket = true;
                index += 1;
            }
            b'/' if next == Some(b'/') => {
                line_comment = true;
                index += 2;
            }
            b'/' if next == Some(b'*') => {
                block_comment = true;
                index += 2;
            }
            b';' => return index,
            _ => index += 1,
        }
    }
    index
}

fn format_from_property(node: &XmlNode) -> String {
    let raw = node.child("FormatString").map(|child| child.text.trim()).unwrap_or("");
    if raw.is_empty() {
        return String::new();
    }
    if raw.len() >= 2 && raw.starts_with('\'') && raw.ends_with('\'') {
        return raw.get(1..raw.len() - 1).unwrap_or("").replace("''", "'");
    }
    raw.to_string()
}

fn measures_from_script(xml: &str) -> Result<Vec<ModelMeasure>> {
    let root = parse_xml(xml)?;
    let mut properties: Vec<(String, String)> = Vec::new();
    if let Some(calculation_properties) = root.find_descendant("CalculationProperties") {
        for property in calculation_properties.children.iter().filter(|child| child.local_name() == "CalculationProperty") {
            let reference = property.child("CalculationReference").map(|node| normalize_reference(&node.text)).unwrap_or_default();
            properties.push((reference, format_from_property(property)));
        }
    }
    let mut measures = Vec::new();
    let Some(script) = root.find_descendant("MdxScript") else {
        return Ok(measures);
    };
    if let Some(commands) = script.child("Commands") {
        for command in commands.children.iter().filter(|child| child.local_name() == "Command") {
            let Some(text) = command.child("Text").map(|node| node.text.as_str()) else {
                continue;
            };
            let mut parsed = parse_measure_statements(text)?;
            for measure in &mut parsed {
                if let Some((_, format)) = properties.iter().find(|(reference, _)| reference == &measure.name) {
                    measure.format = format.clone();
                }
            }
            measures.extend(parsed);
        }
    }
    Ok(measures)
}

// ---------------------------------------------------------------------------------------------
// Writing helpers
// ---------------------------------------------------------------------------------------------

fn mdx_table(table: &str) -> String {
    format!("'{}'", table.replace('\'', "''"))
}

fn mdx_name(name: &str) -> String {
    format!("[{}]", name.replace(']', "]]"))
}

fn measure_command(measure: &ModelMeasure) -> XmlNode {
    let text = format!(
        "----------------------------------------------------------\n-- PowerPivot measures command (do not modify manually) --\n----------------------------------------------------------\n\n\nCREATE MEASURE {}{}={};\n",
        mdx_table(&measure.table),
        mdx_name(&measure.name),
        measure.formula
    );
    let mut annotations = XmlNode { name: "Annotations".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
    for (name, value) in [("FullName", measure.name.as_str()), ("Table", measure.table.as_str())] {
        let mut annotation = XmlNode { name: "Annotation".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
        annotation.children.push(push_text_element("Name", name));
        annotation.children.push(push_text_element("Value", value));
        annotations.children.push(annotation);
    }
    let mut command = XmlNode { name: "Command".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
    command.children.push(push_text_element("Text", &text));
    command.children.push(annotations);
    command
}

fn decimal_places(format: &str) -> usize {
    let section = format.split(';').next().unwrap_or(format);
    let Some(dot) = section.find('.') else {
        return 0;
    };
    let mut count = 0usize;
    for byte in section.as_bytes().iter().skip(dot + 1) {
        if *byte == b'0' || *byte == b'#' {
            count += 1;
        } else {
            break;
        }
    }
    count
}

fn format_attributes(format: &str) -> Vec<(&'static str, String)> {
    let trimmed = format.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("general") {
        return vec![("Format", "General".to_string())];
    }
    let lower = trimmed.to_ascii_lowercase();
    let thousand = trimmed.contains(',');
    let decimals = decimal_places(trimmed);
    // Excel rejects a `Currency` (and `Date`) format annotation written by a third party even
    // when the attribute set matches its own output, so keep the number kind numeric and let the
    // FormatString carry the currency/date layout. Percentage, Scientific and the two number
    // kinds are accepted by Excel and round-trip through CalculationProperties.
    let kind = if lower.contains('%') {
        "Percentage"
    } else if lower.contains("e+") || lower.contains("e-") {
        "Scientific"
    } else if decimals == 0 && !trimmed.contains('.') {
        "NumberWhole"
    } else {
        "NumberDecimal"
    };
    let mut attrs = vec![("Format", kind.to_string())];
    if decimals > 0 {
        attrs.push(("Accuracy", decimals.to_string()));
    }
    if thousand {
        attrs.push(("ThousandSeparator", "True".to_string()));
    }
    attrs
}

fn format_string_expression(format: &str) -> String {
    if format.trim().is_empty() {
        return "''".to_string();
    }
    format!("'{}'", format.replace('\'', "''"))
}

fn calculation_property(measure: &ModelMeasure) -> XmlNode {
    let mut property = XmlNode { name: "CalculationProperty".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
    property.children.push(push_text_element("CalculationReference", &mdx_name(&measure.name)));
    property.children.push(push_text_element("CalculationType", "Member"));
    property.children.push(push_text_element("Description", ""));
    property.children.push(push_text_element("FormatString", &format_string_expression(&measure.format)));
    for empty in
        ["ForeColor", "BackColor", "FontName", "FontSize", "FontFlags", "NonEmptyBehavior", "AssociatedMeasureGroupID", "DisplayFolder", "Language"]
    {
        property.children.push(push_text_element(empty, ""));
    }

    let mut annotations = XmlNode { name: "Annotations".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
    for (name, value) in [("Type", "User"), ("IsPrivate", "False")] {
        let mut annotation = XmlNode { name: "Annotation".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
        annotation.children.push(push_text_element("Name", name));
        annotation.children.push(push_text_element("Value", value));
        annotations.children.push(annotation);
    }
    let mut format_annotation = XmlNode { name: "Annotation".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
    format_annotation.children.push(push_text_element("Name", "Format"));
    let mut value = XmlNode { name: "Value".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() };
    let mut format_element = XmlNode {
        name: "Format".to_string(),
        attrs: format_attributes(&measure.format).into_iter().map(|(key, value)| (key.to_string(), value)).collect(),
        text: String::new(),
        children: Vec::new(),
    };
    // The annotation's payload is in no namespace even though the outer document has a default
    // namespace; Excel writes `xmlns=""` here.
    format_element.attrs.push(("xmlns".to_string(), String::new()));
    value.children.push(format_element);
    format_annotation.children.push(value);
    annotations.children.push(format_annotation);
    property.children.push(annotations);
    property.children.push(push_text_element("Translations", ""));
    property
}

fn command_text(node: &XmlNode) -> &str {
    node.child("Text").map(|text| text.text.as_str()).unwrap_or("")
}

fn is_measure_command(node: &XmlNode) -> bool {
    let text = command_text(node).to_ascii_lowercase();
    text.contains("create measure") || text.contains("powerpivot measures command")
}

fn ensure_child<'a>(parent: &'a mut XmlNode, name: &str) -> &'a mut XmlNode {
    if let Some(index) = parent.children.iter().position(|child| child.local_name() == name) {
        &mut parent.children[index]
    } else {
        parent.children.push(XmlNode { name: name.to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() });
        let index = parent.children.len().saturating_sub(1);
        &mut parent.children[index]
    }
}

fn rewrite_script(xml: &str, measures: &[ModelMeasure]) -> Result<String> {
    let mut root = parse_xml(xml)?;
    let mut old_names: Vec<String> = Vec::new();
    {
        let script = root.find_descendant_mut("MdxScript").ok_or(ModelFileError::NoMeasureScript)?;
        if let Some(commands) = script.child("Commands") {
            for command in commands.children.iter().filter(|child| child.local_name() == "Command" && is_measure_command(child)) {
                for measure in parse_measure_statements(command_text(command)).unwrap_or_default() {
                    old_names.push(measure.name);
                }
            }
        }
    }
    apply_script_edits(&mut root, &old_names, measures)?;
    serialize_xml(&root)
}

/// Applies measure edits to a parsed `<Load>` document. Kept separate so the root wrapper is
/// never discarded.
fn apply_script_edits(root: &mut XmlNode, old_names: &[String], measures: &[ModelMeasure]) -> Result<()> {
    let script = root.find_descendant_mut("MdxScript").ok_or(ModelFileError::NoMeasureScript)?;
    {
        let commands = ensure_child(&mut *script, "Commands");
        let mut kept = Vec::new();
        for child in commands.children.drain(..) {
            if child.local_name() == "Command" && is_measure_command(&child) {
                continue;
            }
            kept.push(child);
        }
        commands.children = kept;
        for measure in measures {
            commands.children.push(measure_command(measure));
        }
    }
    {
        if script.child("CalculationProperties").is_none() {
            script.children.push(XmlNode { name: "CalculationProperties".to_string(), attrs: Vec::new(), text: String::new(), children: Vec::new() });
        }
        let properties = ensure_child(&mut *script, "CalculationProperties");
        properties.children.retain(|child| {
            if child.local_name() != "CalculationProperty" {
                return true;
            }
            let reference = child.child("CalculationReference").map(|node| normalize_reference(&node.text)).unwrap_or_default();
            !old_names.iter().any(|name| name == &reference)
        });
        for measure in measures {
            properties.children.push(calculation_property(measure));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Encoding and rebuilding
// ---------------------------------------------------------------------------------------------

fn encode_file(content: &[u8], compressed: bool, error_code: bool) -> Result<Vec<u8>> {
    let mut blob = Vec::new();
    // Files whose logical payload is UTF-16 with a BOM (PARTITIONS and the backup log) stay raw.
    if compressed && !content.starts_with(&[0xFF, 0xFE]) {
        for chunk in content.chunks(WRITE_CHUNK) {
            let length = u16::try_from(chunk.len()).map_err(|_| ModelFileError::TooLarge)?;
            match xpress8::compress(chunk) {
                Some(encoded) => {
                    let compressed_length = u16::try_from(encoded.len()).map_err(|_| ModelFileError::TooLarge)?;
                    blob.extend_from_slice(&length.to_le_bytes());
                    blob.extend_from_slice(&compressed_length.to_le_bytes());
                    blob.extend_from_slice(&encoded);
                }
                None => {
                    blob.extend_from_slice(&length.to_le_bytes());
                    blob.extend_from_slice(&length.to_le_bytes());
                    blob.extend_from_slice(chunk);
                }
            }
        }
    } else {
        blob.extend_from_slice(content);
    }
    if error_code {
        blob.extend_from_slice(&crc32(&blob).to_le_bytes());
    }
    Ok(blob)
}

fn align_up(value: usize, alignment: usize) -> Result<usize> {
    let remainder = value % alignment;
    if remainder == 0 { Ok(value) } else { value.checked_add(alignment - remainder).ok_or(ModelFileError::TooLarge) }
}

fn build_vdir(entries: &[VdirEntry], blobs: &[Vec<u8>], offsets: &[usize]) -> Result<String> {
    let mut xml = String::from("<VirtualDirectory>");
    for (index, entry) in entries.iter().enumerate() {
        let blob = blobs.get(index).ok_or_else(|| ModelFileError::Corrupt("vdir blob mismatch".into()))?;
        let offset = offsets.get(index).ok_or_else(|| ModelFileError::Corrupt("vdir offset mismatch".into()))?;
        xml.push_str("<BackupFile><Path>");
        xml.push_str(&esc(&entry.path));
        xml.push_str("</Path><Size>");
        xml.push_str(&blob.len().to_string());
        xml.push_str("</Size><m_cbOffsetHeader>");
        xml.push_str(&offset.to_string());
        xml.push_str("</m_cbOffsetHeader><Delete>");
        xml.push_str(&esc(&entry.delete));
        xml.push_str("</Delete><CreatedTimestamp>");
        xml.push_str(&esc(&entry.created));
        xml.push_str("</CreatedTimestamp><Access>");
        xml.push_str(&esc(&entry.access));
        xml.push_str("</Access><LastWriteTime>");
        xml.push_str(&esc(&entry.last_write));
        xml.push_str("</LastWriteTime></BackupFile>");
    }
    xml.push_str("</VirtualDirectory>");
    Ok(xml)
}

fn rebuild(header: &Header, entries: &[VdirEntry], blobs: &[Vec<u8>]) -> Result<Vec<u8>> {
    let mut offsets = Vec::with_capacity(entries.len());
    let mut position = header.data_offset.max(PAGE_SIZE);
    if !position.is_multiple_of(PAGE_SIZE) {
        position = align_up(position, PAGE_SIZE)?;
    }
    for blob in blobs {
        offsets.push(position);
        position = position.checked_add(blob.len()).ok_or(ModelFileError::TooLarge)?;
    }
    let vdir_offset = align_up(position, PAGE_SIZE)?;
    let vdir_xml = build_vdir(entries, blobs, &offsets)?;
    let vdir_bytes = encode_utf16(&vdir_xml);
    if vdir_bytes.len() > MAX_VDIR_BYTES {
        return Err(ModelFileError::TooLarge);
    }

    let mut header_xml = replace_tag_text(&header.xml, "m_cbOffsetHeader", &vdir_offset.to_string())?;
    header_xml = replace_tag_text(&header_xml, "DataSize", &vdir_bytes.len().to_string())?;
    header_xml = replace_tag_text(&header_xml, "Files", &entries.len().to_string())?;
    let header_bytes = encode_utf16(&header_xml);
    if header_bytes.len() + 72 > PAGE_SIZE {
        return Err(ModelFileError::Corrupt("header no longer fits in one page".into()));
    }

    let total = align_up(vdir_offset.checked_add(vdir_bytes.len()).ok_or(ModelFileError::TooLarge)?, PAGE_SIZE)?;
    if total > MAX_CONTAINER_BYTES {
        return Err(ModelFileError::TooLarge);
    }
    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(&[0xFF, 0xFE]);
    output.extend_from_slice(&encode_utf16(SIGNATURE));
    output.extend_from_slice(&header_bytes);
    output.resize(PAGE_SIZE, 0);
    for (index, blob) in blobs.iter().enumerate() {
        let expected = offsets.get(index).copied().ok_or_else(|| ModelFileError::Corrupt("file offset mismatch".into()))?;
        if output.len() != expected {
            return Err(ModelFileError::Corrupt("file layout is not contiguous".into()));
        }
        output.extend_from_slice(blob);
    }
    if output.len() > vdir_offset {
        return Err(ModelFileError::Corrupt("files section overlaps the virtual directory".into()));
    }
    output.resize(vdir_offset, 0);
    output.extend_from_slice(&vdir_bytes);
    output.resize(total, 0);
    Ok(output)
}

fn update_log_content(content: &[u8], storage_path: &str, new_size: usize) -> Option<Vec<u8>> {
    let text = decode_utf16(content.strip_prefix(&[0xFF, 0xFE]).unwrap_or(content), true).ok()?;
    let mut changed = false;
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0usize;
    while let Some(start_relative) = text.get(cursor..).and_then(|rest| rest.find("<BackupFile>")) {
        let start = cursor + start_relative;
        let Some(end_relative) = text.get(start..).and_then(|rest| rest.find("</BackupFile>")) else {
            break;
        };
        let end = start + end_relative + "</BackupFile>".len();
        let block = text.get(start..end)?;
        let is_target = block
            .find("<StoragePath>")
            .and_then(|index| block.get(index + "<StoragePath>".len()..))
            .and_then(|rest| rest.find("</StoragePath>").map(|close| (rest, close)))
            .is_some_and(|(rest, close)| rest.get(..close) == Some(storage_path));
        if is_target && let Ok(updated) = replace_tag_text(block, "Size", &new_size.to_string()) {
            output.push_str(text.get(cursor..start)?);
            output.push_str(&updated);
            cursor = end;
            changed = true;
            continue;
        }
        output.push_str(text.get(cursor..end)?);
        cursor = end;
    }
    output.push_str(text.get(cursor..)?);
    if !changed {
        return None;
    }
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend_from_slice(&encode_utf16(&output));
    Some(bytes)
}

// ---------------------------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------------------------

/// Returns `true` when `data` starts with the Spreadsheet Data Model signature.
pub fn is_model_file(data: &[u8]) -> bool {
    data.len() >= 72
        && data.first().copied() == Some(0xFF)
        && data.get(1).copied() == Some(0xFE)
        && decode_utf16(data.get(2..72).unwrap_or_default(), true).is_ok_and(|signature| signature == SIGNATURE)
}

/// Reads the measure definitions from a `xl/model/item.data` stream.
///
/// The returned order is the order of the `CREATE MEASURE` statements in the cube's MDX script.
/// A model without user measures yields an empty vector.
pub fn read_measures(data: &[u8]) -> Result<Vec<ModelMeasure>> {
    let container = Container::parse(data)?;
    let Some(index) = container.measure_script_index()? else {
        return Ok(Vec::new());
    };
    let entry = container.entries.get(index).ok_or_else(|| ModelFileError::Corrupt("script index".into()))?;
    let content = container.decode_entry(entry)?;
    let text = content_text(&content);
    measures_from_script(&text)
}

/// Rewrites the model's measure definitions and returns a complete replacement stream.
///
/// All files other than the cube's MDX script keep their original stored bytes (including their
/// CRC trailers). The script is re-encoded as verbatim xpress8 chunks, the backup log's size for
/// the script is updated, and the virtual directory and header offsets are recomputed. The
/// result remains page aligned.
pub fn write_measures(data: &[u8], measures: &[ModelMeasure]) -> Result<Vec<u8>> {
    for measure in measures {
        if measure.name.trim().is_empty() || measure.table.trim().is_empty() {
            return Err(ModelFileError::InvalidMeasure("table and name must not be empty".into()));
        }
        if measure.formula.trim().is_empty() {
            return Err(ModelFileError::InvalidMeasure(format!("measure {} has an empty formula", measure.name)));
        }
    }
    let container = Container::parse(data)?;
    let script_index = container.measure_script_index()?.ok_or(ModelFileError::NoMeasureScript)?;
    let script_entry = container.entries.get(script_index).ok_or_else(|| ModelFileError::Corrupt("script index".into()))?;
    let script_content = container.decode_entry(script_entry)?;
    let script_text = content_text(&script_content);
    let new_xml = rewrite_script(&script_text, measures)?;
    let new_script_blob = encode_file(new_xml.as_bytes(), container.header.apply_compression, container.header.error_code)?;

    let mut blobs: Vec<Vec<u8>> = Vec::with_capacity(container.entries.len());
    for (index, entry) in container.entries.iter().enumerate() {
        if index == script_index {
            blobs.push(new_script_blob.clone());
        } else {
            blobs.push(container.entry_bytes(entry)?.to_vec());
        }
    }

    // Keep the backup log's per-file Size in sync with the rewritten script.
    if let Some(log_index) = container.entries.iter().position(|entry| entry.path.eq_ignore_ascii_case("LOG"))
        && let Ok(log_content) = container.decode_entry(&container.entries[log_index])
        && let Some(updated) = update_log_content(&log_content, &script_entry.path, new_xml.len())
    {
        let blob = encode_file(&updated, false, container.header.error_code)?;
        if let Some(slot) = blobs.get_mut(log_index) {
            *slot = blob;
        }
    }

    rebuild(&container.header, &container.entries, &blobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abc_stream() -> Vec<u8> {
        // [MS-XCA] section "LZ77": 300 bytes of "abc" compressed with Plain LZ77.
        vec![0xff, 0xff, 0xff, 0x1f, 0x61, 0x62, 0x63, 0x17, 0x00, 0x0f, 0xff, 0x26, 0x01]
    }

    #[test]
    fn decodes_the_ms_xca_plain_lz77_example() -> Result<()> {
        let decoded = xpress_decompress(&abc_stream(), 300)?;
        assert_eq!(decoded.len(), 300);
        assert!(decoded.chunks(3).all(|chunk| chunk == b"abc"));
        Ok(())
    }

    #[test]
    fn decodes_literal_only_stream() {
        // Four zero flag bytes followed by 32 literals.
        let mut stream = vec![0u8; 4];
        stream.extend_from_slice(b"0123456789abcdef0123456789abcdef");
        let decoded = xpress_decompress(&stream, 32);
        assert_eq!(decoded.as_deref(), Ok(b"0123456789abcdef0123456789abcdef".as_slice()));
    }

    #[test]
    fn rejects_truncated_stream() {
        let error = xpress_decompress(&[0x00], 4);
        assert!(error.is_err());
    }

    #[test]
    fn crc_matches_known_vector() {
        // CRC-32/MPEG-2 of "123456789" is 0x0376E6E7.
        assert_eq!(crc32(b"123456789"), 0xFC89_1918);
    }

    fn synthetic_container(script: &str) -> Result<Vec<u8>> {
        let header_xml = "<BackupLog><BackupRestoreSyncVersion>150</BackupRestoreSyncVersion><Fault>false</Fault><faultcode>43981</faultcode><ErrorCode>true</ErrorCode><EncryptionFlag>false</EncryptionFlag><EncryptionKey>6</EncryptionKey><ApplyCompression>true</ApplyCompression><m_cbOffsetHeader>4096</m_cbOffsetHeader><DataSize>0</DataSize><Files>1</Files><ObjectID>00000000-0000-0000-0000-000000000000</ObjectID><m_cbOffsetData>4096</m_cbOffsetData></BackupLog>".to_string();
        let header = Header {
            xml: header_xml,
            error_code: true,
            apply_compression: true,
            data_offset: PAGE_SIZE,
            vdir_offset: PAGE_SIZE,
            vdir_size: 0,
            files: 1,
        };
        let entry = VdirEntry {
            path: "SCRIPT".to_string(),
            size: 0,
            offset: PAGE_SIZE,
            delete: "false".to_string(),
            created: "0".to_string(),
            access: "0".to_string(),
            last_write: "0".to_string(),
        };
        let blob = encode_file(script.as_bytes(), true, true)?;
        rebuild(&header, &[entry], &[blob])
    }

    #[test]
    fn reads_and_writes_measures_round_trip() -> Result<()> {
        let script = "<Load xmlns=\"http://schemas.microsoft.com/analysisservices/2003/engine\"><ObjectDefinition><MdxScript><Name>MdxScript</Name><Commands><Command><Text>CALCULATE;</Text></Command></Commands><CalculationProperties/></MdxScript></ObjectDefinition></Load>";
        let container = synthetic_container(script)?;
        assert!(read_measures(&container)?.is_empty());
        assert!(read_measures(&container)?.is_empty());
        assert!(matches!(read_measures(b"not a model"), Err(ModelFileError::Format(_)) | Err(ModelFileError::Corrupt(_))));
        // The script path is not the cube's MdxScript file name; the fallback scan still finds
        // the document and the writer operates on it.
        let measure = ModelMeasure {
            table: "Sales".to_string(),
            name: "Total Sales".to_string(),
            formula: "SUM(Sales[Amount])".to_string(),
            format: "0.00".to_string(),
        };
        let updated = write_measures(&container, std::slice::from_ref(&measure))?;
        let measures = read_measures(&updated)?;
        assert_eq!(measures, vec![measure]);
        Ok(())
    }

    #[test]
    fn quoted_names_decode_as_utf8() {
        // Each UTF-8 byte used to become a separate Latin-1 character, which made every
        // non-ASCII table or measure name mojibake and broke the rewrite's old-name matching.
        let text = "CREATE MEASURE 'Ventes Été'[Marge brute €] = SUM('Ventes Été'[Montant])";
        let measures = parse_measure_statements(text).expect("parse");
        assert_eq!(measures.len(), 1);
        assert_eq!(measures[0].table, "Ventes Été");
        assert_eq!(measures[0].name, "Marge brute €");
        assert_eq!(measures[0].formula, "SUM('Ventes Été'[Montant])");

        // A doubled delimiter is still an escaped delimiter, not a terminator.
        assert_eq!(parse_quoted_table(b"'O''Brien'", 0), Some(("O'Brien".to_string(), 10)));
        assert_eq!(parse_quoted_table(b"[Amount]]]", 0), Some(("Amount]".to_string(), 10)));
        // Invalid UTF-8 is rejected instead of being lossily decoded.
        assert_eq!(parse_quoted_table(&[b'\'', 0xFF, b'\''], 0), None);
    }

    #[test]
    fn parser_survives_fuzzed_lengths() {
        for length in 0..128usize {
            let data = vec![0xAB; length];
            let _ = read_measures(&data);
        }
    }

    #[test]
    fn xml_text_preserves_quotes() -> Result<()> {
        let xml = "<a><b>CREATE MEASURE 'Sales'[X]=1;</b></a>";
        let root = parse_xml(xml)?;
        let serialized = serialize_xml(&root)?;
        let reparsed = parse_xml(&serialized)?;
        let text = reparsed.find_descendant("b").map(|node| node.text.clone()).ok_or_else(|| ModelFileError::Corrupt("missing b".into()))?;
        assert_eq!(text, "CREATE MEASURE 'Sales'[X]=1;");
        Ok(())
    }

    /// Opt-in local verification against a real workbook stream. Set
    /// `GRIDCRAFT_MODEL_FIXTURE` to an extracted `xl/model/item.data`; optionally set
    /// `GRIDCRAFT_MODEL_OUTPUT` to write a copy with an appended probe measure.
    #[test]
    fn fixture_round_trip_when_configured() -> Result<()> {
        let Ok(input_path) = std::env::var("GRIDCRAFT_MODEL_FIXTURE") else {
            return Ok(());
        };
        let data = std::fs::read(&input_path).map_err(|error| ModelFileError::Corrupt(format!("read fixture: {error}")))?;
        let measures = read_measures(&data)?;
        println!("fixture measures: {measures:?}");
        let Ok(output_path) = std::env::var("GRIDCRAFT_MODEL_OUTPUT") else {
            return Ok(());
        };
        let mut updated = measures.clone();
        updated.push(ModelMeasure {
            table: "Sales".to_string(),
            name: std::env::var("GRIDCRAFT_MODEL_PROBE_NAME").unwrap_or_else(|_| "Model File Probe".to_string()),
            formula: std::env::var("GRIDCRAFT_MODEL_PROBE_FORMULA").unwrap_or_else(|_| "SUM(Sales[Amount])".to_string()),
            format: std::env::var("GRIDCRAFT_MODEL_PROBE_FORMAT").unwrap_or_else(|_| "0.00".to_string()),
        });
        let bytes = write_measures(&data, &updated)?;
        std::fs::write(&output_path, &bytes).map_err(|error| ModelFileError::Corrupt(format!("write output: {error}")))?;
        let read_back = read_measures(&bytes)?;
        assert_eq!(read_back, updated);
        println!("wrote {} bytes to {output_path}", bytes.len());
        Ok(())
    }

    #[test]
    fn measure_parser_ignores_semicolons_in_strings() -> Result<()> {
        let text = "CREATE MEASURE 'Sales'[Semi]=\"a;b\" & SUM(Sales[Amount]);\n";
        let measures = parse_measure_statements(text)?;
        assert_eq!(measures.len(), 1);
        let measure = measures.first().ok_or_else(|| ModelFileError::Corrupt("measure".into()))?;
        assert_eq!(measure.name, "Semi");
        assert_eq!(measure.formula, "\"a;b\" & SUM(Sales[Amount])");
        Ok(())
    }
}
