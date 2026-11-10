//! VertiPaq data-model read/write orchestration over the ABF container.
//!
//! The reader walks the backup log to recover logical file names, parses the cube, dimension and
//! table metadata, decodes each column's `.idf` and dictionary, and returns a [`ModelData`]. The
//! writer accepts a base container plus a model with the same table/column/relationship shape and
//! rewrites the payloads and every numeric invariant that depends on them.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::value::{Scalar, ScalarKey};

use super::model_data::{ModelColumnData, ModelData, ModelRelationship, ModelTableData, RelationshipMultiplicity};
use super::vertipaq::{
    ColumnEncoding, ColumnKind, SegmentInfo, decode_column_ids, decode_numeric_dictionary, decode_string_dictionary, encode_column_ids,
    encode_no_split, encode_numeric_dictionary, encode_row_number, encode_string_dictionary, value_encoded_ids, width_for,
};
use super::{Container, ModelFileError, Result, XmlNode, content_text, parse_xml, serialize_xml};

/// Metrics for one column, mirroring the numbers Excel's own files expose.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelColumnMetrics {
    pub table: String,
    pub column: String,
    pub rows: usize,
    /// Distinct data ids including the null id.
    pub cardinality: usize,
    pub nulls: bool,
    pub segments: usize,
    pub bit_widths: Vec<u32>,
    pub mins: Vec<i32>,
    /// Decompressed `.idf` bytes.
    pub encoded_bytes: usize,
    /// Stored size from the virtual directory (chunk headers plus payload plus CRC).
    pub stored_bytes: usize,
    /// Decompressed `.dictionary` bytes, or zero for value-encoded columns.
    pub dictionary_bytes: usize,
    pub encoding: String,
}

// ---------------------------------------------------------------------------------------------
// Container inventory
// ---------------------------------------------------------------------------------------------

struct EntryInfo {
    storage: String,
    logical: String,
    base: String,
}

struct Base<'a> {
    container: Container<'a>,
    entries: Vec<EntryInfo>,
    by_base: BTreeMap<String, usize>,
    log_index: Option<usize>,
}

impl<'a> Base<'a> {
    fn load(data: &'a [u8]) -> Result<Self> {
        let container = Container::parse(data)?;
        let mut storage_to_logical: BTreeMap<String, String> = BTreeMap::new();
        let mut log_index = None;
        if let Some(index) = container.entries.iter().position(|entry| entry.path.eq_ignore_ascii_case("LOG")) {
            log_index = Some(index);
            // A CRC or compression failure in the backup log is corruption, not a file we can
            // silently skip; only an unparsable/logically empty document falls back to paths.
            let content = container.decode_entry(&container.entries[index])?;
            if let Ok(text) = super::log_text(&content)
                && let Ok(root) = parse_xml(&text)
            {
                let mut stack = vec![&root];
                while let Some(node) = stack.pop() {
                    if node.local_name() == "BackupFile" {
                        let storage = node.child("StoragePath").map(|child| child.text.trim().to_string()).unwrap_or_default();
                        let path = node.child("Path").map(|child| child.text.trim().to_string()).unwrap_or_default();
                        if !storage.is_empty() && !path.is_empty() {
                            storage_to_logical.insert(storage, path);
                        }
                    }
                    stack.extend(node.children.iter());
                }
            }
        }
        let mut entries = Vec::with_capacity(container.entries.len());
        let mut by_base = BTreeMap::new();
        for (index, entry) in container.entries.iter().enumerate() {
            let logical = storage_to_logical.get(&entry.path).cloned().unwrap_or_else(|| entry.path.clone());
            let base = basename(&logical);
            by_base.entry(base.clone()).or_insert(index);
            entries.push(EntryInfo { storage: entry.path.clone(), logical, base });
        }
        Ok(Self { container, entries, by_base, log_index })
    }

    fn find(&self, name: &str) -> Option<usize> {
        self.by_base.get(name).copied()
    }

    fn storage(&self, index: usize) -> &str {
        self.entries.get(index).map(|entry| entry.storage.as_str()).unwrap_or("")
    }

    fn decode(&self, index: usize) -> Result<Vec<u8>> {
        let entry = self.container.entries.get(index).ok_or_else(|| ModelFileError::Corrupt("file index".into()))?;
        self.container.decode_entry(entry)
    }

    fn stored_size(&self, index: usize) -> usize {
        self.container.entries.get(index).map(|entry| entry.size).unwrap_or(0)
    }

    fn xml(&self, index: usize) -> Result<XmlNode> {
        let content = self.decode(index)?;
        let text = content_text(&content);
        parse_xml(&text)
    }

    fn xml_text(&self, index: usize) -> Result<String> {
        let content = self.decode(index)?;
        let text = content_text(&content);
        Ok(text.into_owned())
    }
}

fn basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_string()
}

fn ends_with_ci(value: &str, suffix: &str) -> bool {
    value.len() >= suffix.len() && value[value.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

// ---------------------------------------------------------------------------------------------
// XML helpers
// ---------------------------------------------------------------------------------------------

fn text_of<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    node.child(name).map(|child| child.text.trim())
}

fn prop<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    node.child("Properties").and_then(|properties| properties.child(name)).map(|child| child.text.trim())
}

fn prop_i64(node: &XmlNode, name: &str) -> Option<i64> {
    prop(node, name).and_then(|value| value.parse().ok())
}

fn prop_i32(node: &XmlNode, name: &str) -> Option<i32> {
    prop_i64(node, name).and_then(|value| i32::try_from(value).ok())
}

fn prop_bool(node: &XmlNode, name: &str) -> Option<bool> {
    prop(node, name).and_then(|value| match value {
        "true" | "True" | "1" => Some(true),
        "false" | "False" | "0" => Some(false),
        _ => None,
    })
}

/// Parses the decimal string used by `XMValueDataDictionary`, which Excel writes with a trailing
/// dot before an exponent (`1.`, `1.E-2`); Rust's float parser does not accept `1.E-2`.
fn parse_magnitude(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let upper = trimmed.to_ascii_uppercase();
    let (mantissa, exponent) = match upper.split_once('E') {
        Some((mantissa, exponent)) => (mantissa, Some(exponent)),
        None => (upper.as_str(), None),
    };
    let mantissa = mantissa.strip_suffix('.').unwrap_or(mantissa);
    let mantissa: f64 = mantissa.parse().ok()?;
    match exponent {
        Some(exponent) => {
            let exponent: i32 = exponent.parse().ok()?;
            Some(mantissa * 10f64.powi(exponent))
        }
        None => Some(mantissa),
    }
}

fn attr<'a>(node: &'a XmlNode, name: &str) -> Option<&'a str> {
    node.attrs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
}

fn collection<'a>(node: &'a XmlNode, name: &str) -> Option<&'a XmlNode> {
    node.child("Collections")?
        .children
        .iter()
        .find(|child| child.local_name() == "Collection" && child.child("Name").map(|child| child.text.trim()) == Some(name))
}

fn member<'a>(node: &'a XmlNode, name: &str) -> Option<&'a XmlNode> {
    node.child("Members")?
        .children
        .iter()
        .filter(|child| child.local_name() == "Member")
        .find(|child| child.child("Name").map(|child| child.text.trim()) == Some(name))
        .and_then(|child| child.child("XMObject"))
}

fn bandwidth(class: &str) -> Option<u32> {
    let start = class.find("CompressionInfo<")? + "CompressionInfo<".len();
    let rest = class.get(start..)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn child_index(node: &XmlNode, name: &str) -> Option<usize> {
    node.children.iter().position(|child| child.local_name() == name)
}

fn set_prop(node: &mut XmlNode, tag: &str, value: impl ToString) -> Result<()> {
    let properties = node.children.iter_mut().find(|child| child.local_name() == "Properties");
    let properties = match properties {
        Some(properties) => properties,
        None => {
            node.children.push(XmlNode { name: "Properties".into(), attrs: Vec::new(), text: String::new(), children: Vec::new() });
            node.children.last_mut().ok_or_else(|| ModelFileError::Corrupt("properties".into()))?
        }
    };
    let existing = properties.children.iter_mut().find(|child| child.local_name() == tag);
    match existing {
        Some(child) => child.text = value.to_string(),
        None => properties.children.push(XmlNode { name: tag.to_string(), attrs: Vec::new(), text: value.to_string(), children: Vec::new() }),
    }
    Ok(())
}

fn set_attr(node: &mut XmlNode, name: &str, value: &str) {
    if let Some(slot) = node.attrs.iter_mut().find(|(key, _)| key == name) {
        slot.1 = value.to_string();
    } else {
        node.attrs.push((name.to_string(), value.to_string()));
    }
}

fn member_mut<'a>(node: &'a mut XmlNode, name: &str) -> Option<&'a mut XmlNode> {
    let members_index = child_index(node, "Members")?;
    let members = node.children.get_mut(members_index)?;
    let member_position = members
        .children
        .iter()
        .position(|child| child.local_name() == "Member" && child.child("Name").map(|child| child.text.trim()) == Some(name))?;
    let member = members.children.get_mut(member_position)?;
    let object_index = child_index(member, "XMObject")?;
    member.children.get_mut(object_index)
}

fn collection_mut<'a>(node: &'a mut XmlNode, name: &str) -> Option<&'a mut XmlNode> {
    let collections_index = child_index(node, "Collections")?;
    let collections = node.children.get_mut(collections_index)?;
    let position = collections
        .children
        .iter()
        .position(|child| child.local_name() == "Collection" && child.child("Name").map(|child| child.text.trim()) == Some(name))?;
    collections.children.get_mut(position)
}

// ---------------------------------------------------------------------------------------------
// Metadata parsing
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct ColumnStats {
    row_count: usize,
    distinct_states: i64,
    max_data_id: i32,
}

#[derive(Clone, Debug)]
struct ColumnMeta {
    storage: String,
    db_type: i32,
    xm_type: i32,
    stats: ColumnStats,
    encoding: ColumnEncoding,
    idf: Option<String>,
    hidx: Option<String>,
    segments: Vec<SegmentInfo>,
    hierarchy: Option<String>,
    row_number: bool,
}

impl ColumnMeta {
    fn kind(&self) -> ColumnKind {
        ColumnKind::from_db_type(self.db_type, self.xm_type)
    }

    fn display(&self, names: &BTreeMap<String, String>) -> String {
        names.get(&self.storage).cloned().unwrap_or_else(|| self.storage.clone())
    }
}

fn parse_stats(object: &XmlNode) -> ColumnStats {
    ColumnStats {
        row_count: prop_i64(object, "RowCount").and_then(|value| usize::try_from(value).ok()).unwrap_or(0),
        distinct_states: prop_i64(object, "DistinctStates").unwrap_or(0),
        max_data_id: prop_i32(object, "MaxDataID").unwrap_or(2),
    }
}

fn parse_segments(column: &XmlNode) -> Result<Vec<SegmentInfo>> {
    let Some(segments) = collection(column, "Segments") else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for segment in segments.children.iter().filter(|child| child.local_name() == "XMObject") {
        let records = prop_i64(segment, "Records").and_then(|value| usize::try_from(value).ok()).unwrap_or(0);
        let sub = member(segment, "SubSegment");
        let compression = sub.and_then(|sub| member(sub, "CompressionInfo"));
        let min = compression.and_then(|info| prop_i32(info, "Min")).unwrap_or(3);
        let width = compression.and_then(|info| attr(info, "class")).and_then(bandwidth).unwrap_or(32);
        result.push(SegmentInfo { records, min, width });
    }
    if result.len() > 1_000_000 {
        return Err(ModelFileError::TooLarge);
    }
    Ok(result)
}

fn parse_column(object: &XmlNode) -> Result<ColumnMeta> {
    let storage = attr(object, "name").unwrap_or("").to_string();
    if storage.is_empty() {
        return Err(ModelFileError::Corrupt("column metadata has no storage name".into()));
    }
    let materialization = member(object, "IntrinsicHierarchy").and_then(|info| prop_i64(info, "TypeMaterialization")).unwrap_or(0);
    let hierarchy =
        member(object, "IntrinsicHierarchy").and_then(|info| prop(info, "TableStore")).filter(|name| !name.is_empty()).map(str::to_string);
    let stats_object = member(object, "ColumnStats");
    let stats = stats_object.map(parse_stats).unwrap_or_default();
    let db_type = stats_object.and_then(|stats| prop_i32(stats, "DBType")).unwrap_or(0);
    let xm_type = stats_object.and_then(|stats| prop_i32(stats, "XMType")).unwrap_or(0);
    let mut encoding = ColumnEncoding::None;
    let mut idf = None;
    let mut hidx = None;
    let mut partition_payloads = 0usize;
    if let Some(data_objects) = object.child("DataObjects") {
        for data_object in data_objects.children.iter().filter(|child| child.local_name() == "DataObject") {
            let Some(object) = data_object.child("XMObject") else { continue };
            let class = attr(object, "class").unwrap_or("");
            if class.contains("XMValueDataDictionary") {
                let base_id = prop_i64(object, "BaseId").unwrap_or(0);
                let magnitude = prop(object, "Magnitude").and_then(parse_magnitude).unwrap_or(1.0);
                encoding = ColumnEncoding::Value { base_id, magnitude };
            } else if class.contains("XMHashDataDictionary") {
                let name = attr(object, "name").unwrap_or("").to_string();
                if class.contains("XM_String") {
                    let flags = prop_i64(object, "DictionaryFlags").unwrap_or(0);
                    encoding = ColumnEncoding::HashString { dictionary: name, flags };
                } else {
                    let real = class.contains("XM_Real");
                    let operating_on_32 = prop_bool(object, "OperatingOn32").unwrap_or(false);
                    encoding = ColumnEncoding::HashNumeric { dictionary: name, real, element_bytes: if operating_on_32 { 4 } else { 8 } };
                }
            } else if class.contains("XMHierarchyDataID2PositionHashIndex") {
                hidx = Some(attr(object, "name").unwrap_or("").to_string());
            } else if class.contains("XMRawColumnPartitionDataObject") {
                partition_payloads = partition_payloads.saturating_add(1);
                idf = Some(attr(object, "name").unwrap_or("").to_string());
            }
        }
    }
    if partition_payloads > 1 {
        return Err(ModelFileError::Unsupported(format!("multi-partition column {}", storage)));
    }
    // The data-id to position hash index lives on the column's intrinsic hierarchy, not on the
    // column itself; Excel only writes it for value-encoded columns.
    if hidx.is_none()
        && let Some(data_objects) = member(object, "IntrinsicHierarchy").and_then(|hierarchy| hierarchy.child("DataObjects"))
        && let Some(object) = data_objects
            .children
            .iter()
            .filter_map(|child| child.child("XMObject"))
            .find(|object| attr(object, "class").is_some_and(|class| class.contains("XMHierarchyDataID2PositionHashIndex")))
    {
        hidx = Some(attr(object, "name").unwrap_or("").to_string());
    }
    let segments = parse_segments(object)?;
    let row_number = materialization == 3 || matches!(storage.as_str(), "RowNumber" | "__XL_RowNumber");
    Ok(ColumnMeta { storage, db_type, xm_type, stats, encoding, idf, hidx, segments, hierarchy, row_number })
}

fn parse_columns(object: &XmlNode) -> Result<Vec<ColumnMeta>> {
    let Some(columns) = collection(object, "Columns") else {
        return Ok(Vec::new());
    };
    let mut result = Vec::new();
    for column in
        columns.children.iter().filter(|child| child.local_name() == "XMObject" && attr(child, "class").is_some_and(|class| class == "XMRawColumn"))
    {
        result.push(parse_column(column)?);
    }
    Ok(result)
}

fn root_name(node: &XmlNode) -> &str {
    attr(node, "name").unwrap_or("")
}

#[derive(Clone, Debug, Default)]
struct DimInfo {
    id: String,
    name: String,
    attributes: BTreeMap<String, (String, String)>,
    relationships: Vec<DimRelationship>,
}

#[derive(Clone, Debug)]
struct DimRelationship {
    id: String,
    active: bool,
    from_dimension: String,
    from_attribute: String,
    from_multiplicity: String,
    to_dimension: String,
    to_attribute: String,
    to_multiplicity: String,
}

fn parse_dimension(xml: &XmlNode) -> Result<DimInfo> {
    let dimension = xml.find_descendant("Dimension").ok_or_else(|| ModelFileError::Corrupt("dimension document has no <Dimension>".into()))?;
    let id = text_of(dimension, "ID").unwrap_or("").to_string();
    let name = text_of(dimension, "Name").unwrap_or(&id).to_string();
    let mut attributes = BTreeMap::new();
    if let Some(attributes_node) = dimension.child("Attributes") {
        for attribute in attributes_node.children.iter().filter(|child| child.local_name() == "Attribute") {
            let attr_id = text_of(attribute, "ID").unwrap_or("").to_string();
            let attr_name = text_of(attribute, "Name").unwrap_or(&attr_id).to_string();
            let key_type = attribute
                .child("KeyColumns")
                .and_then(|keys| keys.children.iter().find(|child| child.local_name() == "KeyColumn"))
                .and_then(|key| text_of(key, "DataType"))
                .unwrap_or("")
                .to_string();
            if !attr_id.is_empty() {
                attributes.insert(attr_id, (attr_name, key_type));
            }
        }
    }
    let mut relationships = Vec::new();
    if let Some(node) = dimension.child("Relationships") {
        for relationship in node.children.iter().filter(|child| child.local_name() == "Relationship") {
            let id = text_of(relationship, "ID").unwrap_or("").to_string();
            let active = text_of(relationship, "Visible").map(|value| value.eq_ignore_ascii_case("true")).unwrap_or(true);
            let from = relationship.child("FromRelationshipEnd");
            let to = relationship.child("ToRelationshipEnd");
            let end = |end: Option<&XmlNode>| -> (String, String, String) {
                let Some(end) = end else { return (String::new(), String::new(), String::new()) };
                let dimension = text_of(end, "DimensionID").unwrap_or("").to_string();
                let attribute = end
                    .child("Attributes")
                    .and_then(|attributes| attributes.children.iter().find(|child| child.local_name() == "Attribute"))
                    .and_then(|attribute| text_of(attribute, "AttributeID"))
                    .unwrap_or("")
                    .to_string();
                let multiplicity = text_of(end, "Multiplicity").unwrap_or("").to_string();
                (dimension, attribute, multiplicity)
            };
            let (from_dimension, from_attribute, from_multiplicity) = end(from);
            let (to_dimension, to_attribute, to_multiplicity) = end(to);
            relationships.push(DimRelationship {
                id,
                active,
                from_dimension,
                from_attribute,
                from_multiplicity,
                to_dimension,
                to_attribute,
                to_multiplicity,
            });
        }
    }
    Ok(DimInfo { id, name, attributes, relationships })
}

#[derive(Clone, Debug)]
struct CubeDimension {
    id: String,
    name: String,
    visible: bool,
}

fn parse_cube(xml: &XmlNode) -> Result<Vec<CubeDimension>> {
    let dimensions = xml.find_descendant("Dimensions").ok_or_else(|| ModelFileError::Corrupt("cube document has no <Dimensions>".into()))?;
    let mut result = Vec::new();
    for dimension in dimensions.children.iter().filter(|child| child.local_name() == "Dimension") {
        let id = text_of(dimension, "ID").or_else(|| text_of(dimension, "DimensionID")).unwrap_or("").to_string();
        let name = text_of(dimension, "Name").unwrap_or(&id).to_string();
        let visible = text_of(dimension, "Visible").map(|value| value.eq_ignore_ascii_case("true")).unwrap_or(true);
        result.push(CubeDimension { id, name, visible });
    }
    Ok(result)
}

// ---------------------------------------------------------------------------------------------
// Loaded model description
// ---------------------------------------------------------------------------------------------

struct TableFile {
    index: usize,
    name: String,
    columns: Vec<ColumnMeta>,
}

struct ModelLayout {
    tables: Vec<TableFile>,
    dimensions: Vec<DimInfo>,
    hierarchies: Vec<(usize, String, XmlNode)>,
    relationship_tables: Vec<(usize, String, XmlNode)>,
    cube: Option<XmlNode>,
}

impl ModelLayout {
    fn load(base: &Base<'_>) -> Result<Self> {
        let mut table_indices = Vec::new();
        let mut hierarchy_indices = Vec::new();
        let mut relationship_indices = Vec::new();
        let mut dimension_indices = Vec::new();
        let mut cube_indices = Vec::new();
        for (index, entry) in base.entries.iter().enumerate() {
            if entry.base.eq_ignore_ascii_case("LOG") || entry.base.eq_ignore_ascii_case("PARTITIONS") {
                continue;
            }
            if ends_with_ci(&entry.base, ".tbl.xml") {
                if entry.logical.contains("H$") || entry.base.starts_with("H$") {
                    hierarchy_indices.push(index);
                } else if entry.logical.contains("R$") || entry.base.starts_with("R$") {
                    relationship_indices.push(index);
                } else {
                    table_indices.push(index);
                }
            } else if ends_with_ci(&entry.base, ".dim.xml") {
                dimension_indices.push(index);
            } else if ends_with_ci(&entry.base, ".cub.xml") {
                cube_indices.push(index);
            }
        }

        let mut tables = Vec::new();
        for index in table_indices {
            let xml = base.xml(index)?;
            let name = root_name(&xml).to_string();
            let columns = parse_columns(&xml)?;
            tables.push(TableFile { index, name, columns });
        }
        let mut hierarchies = Vec::new();
        for index in hierarchy_indices {
            let xml = base.xml(index)?;
            hierarchies.push((index, root_name(&xml).to_string(), xml));
        }
        let mut relationship_tables = Vec::new();
        for index in relationship_indices {
            let xml = base.xml(index)?;
            relationship_tables.push((index, root_name(&xml).to_string(), xml));
        }
        let mut dimensions = Vec::new();
        for index in dimension_indices {
            let xml = base.xml(index)?;
            dimensions.push(parse_dimension(&xml)?);
        }
        dimensions.sort_by(|left, right| left.id.cmp(&right.id));
        let cube = match cube_indices.first() {
            Some(index) => Some(base.xml(*index)?),
            None => None,
        };
        Ok(Self { tables, dimensions, hierarchies, relationship_tables, cube })
    }

    fn table(&self, name: &str) -> Option<&TableFile> {
        self.tables.iter().find(|table| table.name == name)
    }

    fn dimension(&self, id: &str) -> Option<&DimInfo> {
        self.dimensions.iter().find(|dimension| dimension.id == id)
    }

    fn dimensions_in_order(&self) -> Result<Vec<CubeDimension>> {
        match &self.cube {
            Some(cube) => parse_cube(cube),
            None => Ok(self.tables.iter().map(|table| CubeDimension { id: table.name.clone(), name: table.name.clone(), visible: true }).collect()),
        }
    }
}

fn row_count_of(columns: &[ColumnMeta]) -> usize {
    columns.iter().map(|column| column.stats.row_count).max().unwrap_or(0)
}

// ---------------------------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------------------------

fn load_dictionary_bytes(base: &Base<'_>, name: &str) -> Result<Vec<u8>> {
    let index = base.find(name).ok_or_else(|| ModelFileError::Corrupt(format!("missing dictionary file {name}")))?;
    base.decode(index)
}

fn read_column_values(base: &Base<'_>, column: &ColumnMeta, row_count: usize) -> Result<(Vec<Scalar>, Vec<u32>, usize, usize)> {
    let kind = column.kind();
    let Some(idf_name) = &column.idf else {
        return Ok((vec![Scalar::Blank; row_count], Vec::new(), 0, 0));
    };
    let idf_index = base.find(idf_name).ok_or_else(|| ModelFileError::Corrupt(format!("missing column data file {idf_name}")))?;
    let idf = base.decode(idf_index)?;
    let ids = decode_column_ids(&idf, &column.segments)?;
    if ids.len() != row_count {
        return Err(ModelFileError::Corrupt(format!("column {} decodes {} rows but the metadata declares {row_count}", column.storage, ids.len())));
    }
    for id in &ids {
        if *id < 2 {
            return Err(ModelFileError::Corrupt(format!("column {} contains an invalid data id", column.storage)));
        }
        if column.stats.max_data_id >= 3 && i64::from(*id) > i64::from(column.stats.max_data_id) && *id != 2 {
            return Err(ModelFileError::Corrupt(format!("column {} data id exceeds MaxDataID", column.storage)));
        }
    }
    let mut strings: Option<Vec<String>> = None;
    let mut numbers: Option<(bool, Vec<i64>)> = None;
    let mut dictionary_size = 0usize;
    match &column.encoding {
        ColumnEncoding::HashString { dictionary, flags } => {
            let bytes = load_dictionary_bytes(base, dictionary)?;
            dictionary_size = bytes.len();
            strings = Some(decode_string_dictionary(&bytes, flags & 1 != 0)?);
            if strings.as_ref().is_some_and(|values| values.len() + 2 < i64::from(column.stats.max_data_id).max(0) as usize) {
                return Err(ModelFileError::Corrupt(format!("string dictionary for {} is too short", column.storage)));
            }
        }
        ColumnEncoding::HashNumeric { dictionary, .. } => {
            let bytes = load_dictionary_bytes(base, dictionary)?;
            dictionary_size = bytes.len();
            let (real, _, values) = decode_numeric_dictionary(&bytes)?;
            numbers = Some((real, values));
        }
        ColumnEncoding::Value { .. } | ColumnEncoding::None => {}
    }
    let mut values = Vec::with_capacity(ids.len());
    for id in &ids {
        if *id == 2 || kind == ColumnKind::Empty {
            values.push(Scalar::Blank);
            continue;
        }
        let value = match &column.encoding {
            ColumnEncoding::Value { base_id, magnitude } => {
                let raw = (i64::from(*id) + *base_id) as f64 * *magnitude;
                match kind {
                    ColumnKind::Currency => Scalar::Currency(round_currency(raw)),
                    ColumnKind::Date => Scalar::DateTime(raw),
                    ColumnKind::Boolean => Scalar::Boolean(raw != 0.0),
                    ColumnKind::Real => Scalar::Decimal(raw),
                    _ => {
                        let raw_id = i64::from(*id) + *base_id;
                        Scalar::Integer(if (*magnitude - 1.0).abs() < 1.0e-9 { raw_id } else { (raw_id as f64 * *magnitude).round() as i64 })
                    }
                }
            }
            ColumnEncoding::HashString { .. } => {
                let index = usize::try_from(*id - 3).map_err(|_| ModelFileError::Corrupt("string id".into()))?;
                let text = strings
                    .as_ref()
                    .and_then(|values| values.get(index))
                    .ok_or_else(|| ModelFileError::Corrupt(format!("string id {id} is outside the dictionary")))?;
                Scalar::String(text.clone())
            }
            ColumnEncoding::HashNumeric { real, .. } => {
                let index = usize::try_from(*id - 3).map_err(|_| ModelFileError::Corrupt("numeric id".into()))?;
                let raw = numbers
                    .as_ref()
                    .and_then(|(_, values)| values.get(index).copied())
                    .ok_or_else(|| ModelFileError::Corrupt(format!("numeric id {id} is outside the dictionary")))?;
                let raw = if *real { f64::from_bits(raw as u64) } else { raw as f64 };
                match kind {
                    ColumnKind::Currency => Scalar::Currency(round_currency(raw / 100.0)),
                    ColumnKind::Date => Scalar::DateTime(raw),
                    ColumnKind::Boolean => Scalar::Boolean(raw != 0.0),
                    ColumnKind::Real => Scalar::Decimal(raw),
                    _ => Scalar::Integer(raw as i64),
                }
            }
            ColumnEncoding::None => Scalar::Blank,
        };
        values.push(value);
    }
    let encoded_bytes = idf.len();
    Ok((values, ids, encoded_bytes, dictionary_size))
}

fn round_currency(value: f64) -> i64 {
    (value * 10_000.0).round() as i64
}

fn build_measures(base: &Base<'_>) -> Result<Vec<super::ModelMeasure>> {
    let Some(index) = base.container.measure_script_index()? else {
        return Ok(Vec::new());
    };
    let content = base.decode(index)?;
    let text = content_text(&content);
    super::measures_from_script(&text)
}

fn build_relationships(layout: &ModelLayout) -> Vec<ModelRelationship> {
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for dimension in &layout.dimensions {
        for relationship in &dimension.relationships {
            if !seen.insert(relationship.id.clone()) {
                continue;
            }
            let Some(from) = layout.dimension(&relationship.from_dimension) else { continue };
            let Some(to) = layout.dimension(&relationship.to_dimension) else { continue };
            let from_column = from
                .attributes
                .get(&relationship.from_attribute)
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| relationship.from_attribute.clone());
            let to_column =
                to.attributes.get(&relationship.to_attribute).map(|(name, _)| name.clone()).unwrap_or_else(|| relationship.to_attribute.clone());
            let multiplicity = match (relationship.from_multiplicity.as_str(), relationship.to_multiplicity.as_str()) {
                ("Many", "One") => RelationshipMultiplicity::ManyToOne,
                ("One", "Many") => RelationshipMultiplicity::OneToMany,
                ("One", "One") => RelationshipMultiplicity::OneToOne,
                _ => RelationshipMultiplicity::Unknown,
            };
            result.push(ModelRelationship {
                foreign_table: from.name.clone(),
                foreign_column: from_column,
                primary_table: to.name.clone(),
                primary_column: to_column,
                active: relationship.active,
                multiplicity,
            });
        }
    }
    result
}

/// Reads the full VertiPaq data model out of a `xl/model/item.data` stream.
pub fn read_model(data: &[u8]) -> Result<ModelData> {
    let base = Base::load(data)?;
    let layout = ModelLayout::load(&base)?;
    let order = layout.dimensions_in_order()?;
    let mut tables = Vec::new();
    for dimension in &order {
        let Some(table) = layout.table(&dimension.id) else {
            return Err(ModelFileError::Corrupt(format!("cube dimension {} has no table metadata", dimension.id)));
        };
        let names: BTreeMap<String, String> = layout
            .dimension(&dimension.id)
            .map(|info| info.attributes.iter().map(|(id, (name, _))| (id.clone(), name.clone())).collect())
            .unwrap_or_default();
        let row_count = row_count_of(&table.columns);
        let mut columns = Vec::new();
        for column in &table.columns {
            if column.row_number {
                continue;
            }
            let (values, ids, _, _) = read_column_values(&base, column, row_count)?;
            if column.stats.distinct_states > 0 {
                let distinct: BTreeSet<u32> = ids.iter().copied().collect();
                if distinct.len() > usize::try_from(column.stats.distinct_states).unwrap_or(usize::MAX) {
                    return Err(ModelFileError::Corrupt(format!("column {} has more distinct ids than its statistics declare", column.storage)));
                }
            }
            columns.push(ModelColumnData { name: column.display(&names), data_type: column.kind().type_name().to_string(), values });
        }
        tables.push(ModelTableData { name: dimension.name.clone(), visible: dimension.visible, columns });
    }
    let relationships = build_relationships(&layout);
    let measures = build_measures(&base)?;
    Ok(ModelData { tables, relationships, measures })
}

/// Reports per-column VertiPaq metrics for a stream.
pub fn column_metrics(data: &[u8]) -> Result<Vec<ModelColumnMetrics>> {
    let base = Base::load(data)?;
    let layout = ModelLayout::load(&base)?;
    let order = layout.dimensions_in_order()?;
    let mut result = Vec::new();
    for dimension in &order {
        let Some(table) = layout.table(&dimension.id) else { continue };
        let row_count = row_count_of(&table.columns);
        for column in &table.columns {
            if column.row_number {
                continue;
            }
            let Some(idf_name) = &column.idf else { continue };
            let idf_index = base.find(idf_name).ok_or_else(|| ModelFileError::Corrupt(format!("missing column data file {idf_name}")))?;
            let idf = base.decode(idf_index)?;
            let ids = decode_column_ids(&idf, &column.segments)?;
            let nulls = ids.contains(&2);
            let dictionary_bytes = match &column.encoding {
                ColumnEncoding::HashString { dictionary, .. } | ColumnEncoding::HashNumeric { dictionary, .. } => {
                    load_dictionary_bytes(&base, dictionary)?.len()
                }
                _ => 0,
            };
            // Excel's own metric is `LastId - 2` for hash columns (the declared dictionary size,
            // which can exceed the number of ids actually referenced) and the number of distinct
            // ids including null for value-encoded columns.
            let cardinality = match &column.encoding {
                ColumnEncoding::HashString { dictionary, flags } => {
                    let bytes = load_dictionary_bytes(&base, dictionary)?;
                    decode_string_dictionary(&bytes, flags & 1 != 0)?.len() + usize::from(nulls)
                }
                ColumnEncoding::HashNumeric { dictionary, .. } => {
                    let bytes = load_dictionary_bytes(&base, dictionary)?;
                    decode_numeric_dictionary(&bytes)?.2.len() + usize::from(nulls)
                }
                _ => ids.iter().copied().collect::<BTreeSet<_>>().len(),
            };
            let encoding = match &column.encoding {
                ColumnEncoding::Value { .. } => "value",
                ColumnEncoding::HashString { .. } => "hash string",
                ColumnEncoding::HashNumeric { .. } => "hash numeric",
                ColumnEncoding::None => "none",
            };
            result.push(ModelColumnMetrics {
                table: dimension.name.clone(),
                column: column.storage.clone(),
                rows: row_count,
                cardinality,
                nulls,
                segments: column.segments.len(),
                bit_widths: column.segments.iter().map(|segment| segment.width).collect(),
                mins: column.segments.iter().map(|segment| segment.min).collect(),
                encoded_bytes: idf.len(),
                stored_bytes: base.stored_size(idf_index),
                dictionary_bytes,
                encoding: encoding.to_string(),
            });
        }
    }
    Ok(result)
}

// ---------------------------------------------------------------------------------------------
// Writing (implemented below the reader so the file stays one coherent unit)
// ---------------------------------------------------------------------------------------------

/// Returns every embedded file's logical name and decompressed content, in container order.
///
/// This is a read-only inspection helper for tests and for comparing a part against Excel's own
/// copy; it never modifies the stream. A file whose logical name the backup log does not map is
/// returned under its virtual-directory storage path.
pub fn file_contents(data: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let base = Base::load(data)?;
    let mut result = Vec::with_capacity(base.entries.len());
    for (index, entry) in base.entries.iter().enumerate() {
        result.push((entry.logical.clone(), base.decode(index)?));
    }
    Ok(result)
}

/// Rewrites the data model described by `model` on top of the `base` container.
pub fn write_model(base: &[u8], model: &ModelData) -> Result<Vec<u8>> {
    let loaded = Base::load(base)?;
    let layout = ModelLayout::load(&loaded)?;
    // Preserve every stored byte of a column whose values did not change: Excel's own ids,
    // dictionaries and hierarchy arrays are known-good and need no rebuild.
    let previous = read_model(base).unwrap_or_default();
    let plan = build_write_plan(&layout, model, &previous)?;
    apply_write_plan(&loaded, &layout, model, plan)
}

// ---------------------------------------------------------------------------------------------
// Write plan
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct HierarchyWrite {
    name: String,
    total: usize,
    records_per_segment: usize,
    segments: usize,
    files: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug)]
struct ColumnWrite {
    storage: String,
    row_number: bool,
    unchanged: bool,
    has_payload: bool,
    rows: usize,
    values: Vec<Scalar>,
    ids: Vec<u32>,
    packed_values: usize,
    rle_runs: usize,
    others_rle_runs: usize,
    distinct_incl_null: usize,
    non_null_distinct: usize,
    min: i32,
    min_id: i32,
    max_id: i32,
    has_nulls: bool,
    width: u32,
    idf_name: Option<String>,
    idf: Vec<u8>,
    hidx: Option<(String, Vec<u8>)>,
    dictionary: Option<(String, Vec<u8>)>,
    value_base_id: Option<i64>,
    hierarchy: Option<HierarchyWrite>,
}

#[derive(Clone, Debug)]
struct TableWrite {
    id: String,
    name: String,
    unchanged: bool,
    rows: usize,
    columns: Vec<ColumnWrite>,
}

#[derive(Clone, Debug)]
struct RelationshipWrite {
    records: usize,
    width: u32,
    index: Vec<i64>,
    idf_name: String,
}

#[derive(Clone, Debug)]
struct WritePlan {
    tables: Vec<TableWrite>,
    relationships: Vec<RelationshipWrite>,
    measures: Vec<super::ModelMeasure>,
}

fn model_table<'a>(model: &'a ModelData, name: &str) -> Option<&'a ModelTableData> {
    model.tables.iter().find(|table| table.name == name).or_else(|| model.tables.iter().find(|table| table.name.eq_ignore_ascii_case(name)))
}

fn model_column<'a>(table: &'a ModelTableData, names: &BTreeMap<String, String>, storage: &str) -> Option<&'a ModelColumnData> {
    let display = names.get(storage).map(String::as_str).unwrap_or(storage);
    table.columns.iter().find(|column| column.name == display).or_else(|| table.columns.iter().find(|column| column.name == storage))
}

fn check_type(column: &ColumnMeta, model_column: &ModelColumnData) -> Result<()> {
    let expected = column.kind().type_name();
    if !model_column.data_type.eq_ignore_ascii_case(expected) && !(expected == "Double" && model_column.data_type.eq_ignore_ascii_case("Decimal")) {
        return Err(ModelFileError::InvalidModel(format!(
            "column {} is {} in the model but the container declares {expected}",
            column.storage, model_column.data_type
        )));
    }
    Ok(())
}

fn build_write_plan(layout: &ModelLayout, model: &ModelData, previous: &ModelData) -> Result<WritePlan> {
    let order = layout.dimensions_in_order()?;
    let mut tables: Vec<TableWrite> = Vec::new();
    for dimension in &order {
        let Some(table) = layout.table(&dimension.id) else {
            return Err(ModelFileError::Corrupt(format!("cube dimension {} has no table metadata", dimension.id)));
        };
        let Some(table_model) = model_table(model, &dimension.name) else {
            return Err(ModelFileError::InvalidModel(format!("table {} is missing from the model", dimension.name)));
        };
        let names: BTreeMap<String, String> = layout
            .dimension(&dimension.id)
            .map(|info| info.attributes.iter().map(|(id, (name, _))| (id.clone(), name.clone())).collect())
            .unwrap_or_default();
        let previous_table = previous.tables.iter().find(|candidate| candidate.name == dimension.name);
        let data_columns: Vec<&ColumnMeta> = table.columns.iter().filter(|column| !column.row_number).collect();
        if data_columns.len() != table_model.columns.len() {
            return Err(ModelFileError::InvalidModel(format!(
                "table {} has {} stored columns but the model lists {}",
                dimension.name,
                data_columns.len(),
                table_model.columns.len()
            )));
        }
        let mut rows: Option<usize> = None;
        for column in &data_columns {
            let Some(model_column) = model_column(table_model, &names, &column.storage) else {
                return Err(ModelFileError::InvalidModel(format!(
                    "column {} of table {} is missing from the model",
                    column.display(&names),
                    dimension.name
                )));
            };
            check_type(column, model_column)?;
            match rows {
                Some(expected) if expected != model_column.values.len() => {
                    return Err(ModelFileError::InvalidModel(format!(
                        "column {} of table {} has {} rows but another column has {expected}",
                        model_column.name,
                        dimension.name,
                        model_column.values.len()
                    )));
                }
                None => rows = Some(model_column.values.len()),
                _ => {}
            }
        }
        let rows = rows.unwrap_or_else(|| row_count_of(&table.columns));
        let mut columns = Vec::new();
        for column in &data_columns {
            let model_column = model_column(table_model, &names, &column.storage)
                .ok_or_else(|| ModelFileError::InvalidModel(format!("column {} is missing from the model", column.display(&names))))?;
            let unchanged = previous_table.is_some_and(|previous_table| {
                previous_table.columns.iter().any(|previous_column| {
                    previous_column.name == model_column.name
                        && previous_column.data_type == model_column.data_type
                        && previous_column.values == model_column.values
                })
            });
            let mut write = build_column_write(layout, column, model_column, rows, unchanged)?;
            write.unchanged = unchanged;
            columns.push(write);
        }
        if let Some(row_number) = table.columns.iter().find(|column| column.row_number) {
            let idf = encode_row_number(rows)?;
            let rows_unchanged = previous_table.is_some_and(|previous_table| {
                previous_table
                    .columns
                    .iter()
                    .all(|previous_column| previous_column.name == row_number.storage || previous_column.values.len() == rows)
                    && previous_table.columns.first().map(|column| column.values.len()) == Some(rows)
            });
            columns.push(ColumnWrite {
                storage: row_number.storage.clone(),
                row_number: true,
                unchanged: rows_unchanged,
                has_payload: row_number.idf.is_some(),
                rows,
                values: Vec::new(),
                ids: Vec::new(),
                packed_values: rows,
                rle_runs: 0,
                others_rle_runs: 0,
                distinct_incl_null: rows,
                non_null_distinct: rows,
                min: 3,
                min_id: 3,
                max_id: 2 + i32::try_from(rows).unwrap_or(i32::MAX),
                has_nulls: false,
                width: 32,
                idf_name: row_number.idf.clone(),
                idf,
                hidx: None,
                dictionary: None,
                value_base_id: None,
                hierarchy: None,
            });
        }
        let unchanged = columns.iter().all(|column| column.unchanged);
        tables.push(TableWrite { id: dimension.id.clone(), name: dimension.name.clone(), unchanged, rows, columns });
    }

    let expected: BTreeSet<(String, String, String, String)> = model
        .relationships
        .iter()
        .map(|relationship| {
            (
                relationship.foreign_table.to_ascii_lowercase(),
                relationship.foreign_column.to_ascii_lowercase(),
                relationship.primary_table.to_ascii_lowercase(),
                relationship.primary_column.to_ascii_lowercase(),
            )
        })
        .collect();
    let mut actual = BTreeSet::new();
    let mut relationships = Vec::new();
    let mut seen_relationships = BTreeSet::new();
    for dimension in &layout.dimensions {
        for relationship in &dimension.relationships {
            if !seen_relationships.insert(relationship.id.clone()) {
                continue;
            }
            let Some(from_table) = layout.table(&relationship.from_dimension) else { continue };
            let Some(to_table) = layout.table(&relationship.to_dimension) else { continue };
            let from_name =
                layout.dimension(&relationship.from_dimension).map(|info| info.name.clone()).unwrap_or_else(|| relationship.from_dimension.clone());
            let to_name =
                layout.dimension(&relationship.to_dimension).map(|info| info.name.clone()).unwrap_or_else(|| relationship.to_dimension.clone());
            let from_display = layout
                .dimension(&relationship.from_dimension)
                .and_then(|info| info.attributes.get(&relationship.from_attribute))
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| relationship.from_attribute.clone());
            let to_display = layout
                .dimension(&relationship.to_dimension)
                .and_then(|info| info.attributes.get(&relationship.to_attribute))
                .map(|(name, _)| name.clone())
                .unwrap_or_else(|| relationship.to_attribute.clone());
            actual.insert((
                from_name.to_ascii_lowercase(),
                from_display.to_ascii_lowercase(),
                to_name.to_ascii_lowercase(),
                to_display.to_ascii_lowercase(),
            ));
            let from_write = tables
                .iter()
                .find(|table| table.name.eq_ignore_ascii_case(&from_name))
                .and_then(|table| table.columns.iter().find(|column| column.storage == relationship.from_attribute));
            let to_write = tables
                .iter()
                .find(|table| table.name.eq_ignore_ascii_case(&to_name))
                .and_then(|table| table.columns.iter().find(|column| column.storage == relationship.to_attribute));
            let (Some(from_write), Some(to_write)) = (from_write, to_write) else { continue };
            if from_write.unchanged && to_write.unchanged {
                // Both key columns keep their original ids and payloads, so the stored index is
                // still valid; leaving it untouched also avoids re-deriving Excel's id order.
                continue;
            }
            let _ = from_table;
            let relationship_table = layout
                .relationship_tables
                .iter()
                .find(|(_, name, _)| *name == format!("R${}${}", relationship.from_dimension, relationship.id))
                .or_else(|| {
                    layout
                        .relationship_tables
                        .iter()
                        .find(|(_, name, _)| name.starts_with(&format!("R${}$", relationship.from_dimension)) && name.contains(&relationship.id))
                });
            let Some((_, _, xml)) = relationship_table else {
                return Err(ModelFileError::Unsupported(format!("relationship index table for {} is absent", relationship.id)));
            };
            let index_column = parse_columns(xml)?.into_iter().find(|column| column.storage == "INDEX");
            let Some(idf_name) = index_column.and_then(|column| column.idf) else {
                return Err(ModelFileError::Corrupt(format!("relationship index table for {} has no payload", relationship.id)));
            };
            let _ = to_table;
            relationships.push(build_relationship_index(from_write, to_write, idf_name)?);
        }
    }
    if expected != actual {
        return Err(ModelFileError::InvalidModel("the model's relationships do not match the base container".into()));
    }
    Ok(WritePlan { tables, relationships, measures: model.measures.clone() })
}

fn build_column_write(
    layout: &ModelLayout,
    column: &ColumnMeta,
    model_column: &ModelColumnData,
    rows: usize,
    unchanged: bool,
) -> Result<ColumnWrite> {
    let kind = column.kind();
    validate_values(kind, &model_column.values, &model_column.name)?;
    let mut dictionary: Option<(String, Vec<u8>)> = None;
    let mut value_base_id = None;
    let ids: Vec<u32>;
    match (&column.encoding, kind) {
        (ColumnEncoding::Value { magnitude, .. }, _) => {
            let magnitude = if magnitude.is_finite() && *magnitude != 0.0 { *magnitude } else { 1.0 };
            let (encoded, base_id, _) = value_encoded_ids(model_column, magnitude)?;
            value_base_id = Some(base_id);
            ids = encoded;
        }
        (ColumnEncoding::HashString { dictionary: name, .. }, ColumnKind::String) => {
            let mut distinct: BTreeMap<ScalarKey, String> = BTreeMap::new();
            for value in &model_column.values {
                match value {
                    Scalar::Blank => {}
                    Scalar::String(text) => {
                        distinct.entry(ScalarKey::String(text.clone())).or_insert_with(|| text.clone());
                    }
                    _ => return Err(ModelFileError::InvalidModel(format!("text column {} holds a non-text value", model_column.name))),
                }
            }
            let values: Vec<String> = distinct.into_values().collect();
            let mut lookup: BTreeMap<ScalarKey, u32> = BTreeMap::new();
            for (index, value) in values.iter().enumerate() {
                lookup.insert(ScalarKey::String(value.clone()), u32::try_from(index).map_err(|_| ModelFileError::TooLarge)? + 3);
            }
            ids = model_column
                .values
                .iter()
                .map(|value| match value {
                    Scalar::Blank => Ok(2),
                    Scalar::String(text) => {
                        lookup.get(&ScalarKey::String(text.clone())).copied().ok_or_else(|| ModelFileError::Corrupt("missing string id".into()))
                    }
                    _ => Err(ModelFileError::InvalidModel("text column holds a non-text value".into())),
                })
                .collect::<Result<Vec<_>>>()?;
            let has_nulls = model_column.values.iter().any(|value| matches!(value, Scalar::Blank));
            dictionary = Some((name.clone(), encode_string_dictionary(&values, has_nulls)?));
        }
        (ColumnEncoding::HashNumeric { dictionary: name, real, element_bytes }, _) => {
            let mut distinct: BTreeMap<ScalarKey, Scalar> = BTreeMap::new();
            for value in &model_column.values {
                if matches!(value, Scalar::Blank) {
                    continue;
                }
                distinct.entry(ScalarKey::from(value)).or_insert_with(|| value.clone());
            }
            let values: Vec<Scalar> = distinct.into_values().collect();
            let mut raw = Vec::with_capacity(values.len());
            for value in &values {
                raw.push(numeric_raw(value, *real)?);
            }
            let mut lookup: BTreeMap<ScalarKey, u32> = BTreeMap::new();
            for (index, value) in values.iter().enumerate() {
                lookup.insert(ScalarKey::from(value), u32::try_from(index).map_err(|_| ModelFileError::TooLarge)? + 3);
            }
            ids = model_column
                .values
                .iter()
                .map(|value| match value {
                    Scalar::Blank => Ok(2),
                    other => lookup.get(&ScalarKey::from(other)).copied().ok_or_else(|| ModelFileError::Corrupt("missing numeric id".into())),
                })
                .collect::<Result<Vec<_>>>()?;
            dictionary = Some((name.clone(), encode_numeric_dictionary(&raw, *real, *element_bytes)?));
        }
        _ => {
            if model_column.values.iter().any(|value| !matches!(value, Scalar::Blank)) {
                return Err(ModelFileError::Unsupported(format!("writing {} values for a {} column", model_column.data_type, kind.type_name())));
            }
            ids = vec![2u32; model_column.values.len()];
        }
    }
    if ids.len() != rows {
        return Err(ModelFileError::InvalidModel(format!("column {} has an inconsistent row count", model_column.name)));
    }
    let has_nulls = ids.contains(&2);
    let min_id = i32::try_from(ids.iter().copied().filter(|id| *id != 2).min().unwrap_or(2))
        .map_err(|_| ModelFileError::InvalidModel("data id exceeds the signed range".into()))?;
    let max_id =
        i32::try_from(ids.iter().copied().max().unwrap_or(2)).map_err(|_| ModelFileError::InvalidModel("data id exceeds the signed range".into()))?;
    let min = if has_nulls { 2 } else { min_id };
    let width = width_for(u64::from((max_id - min).max(0) as u32)).unwrap_or(32);
    let encoded = encode_column_ids(&ids, min, width)?;
    let distinct: BTreeSet<u32> = ids.iter().copied().collect();
    let non_null_distinct = distinct.iter().filter(|id| **id != 2).count();
    // The hierarchy's positions are its `POS_TO_ID` prefix: the null id first when the column has
    // nulls, then the data ids in value order (ascending ids for both encodings).
    let mut positions: Vec<u32> = Vec::with_capacity(non_null_distinct + 1);
    if has_nulls {
        positions.push(2);
    }
    positions.extend(distinct.iter().copied().filter(|id| *id != 2));
    let hierarchy = match &column.hierarchy {
        Some(name) => build_hierarchy_write(layout, name, &positions)?,
        None => None,
    };
    // An unchanged column keeps Excel's hash index byte for byte, so never re-derive it (that
    // would also reject a valid Excel chain longer than this encoder's bucket layout).
    let hidx = if unchanged || !matches!(column.encoding, ColumnEncoding::Value { .. }) { None } else { build_column_hidx(column, &positions)? };
    Ok(ColumnWrite {
        storage: column.storage.clone(),
        row_number: false,
        unchanged: false,
        has_payload: column.idf.is_some(),
        rows,
        values: model_column.values.clone(),
        ids,
        packed_values: encoded.packed_values,
        rle_runs: encoded.rle_runs,
        others_rle_runs: encoded.packed_entries,
        distinct_incl_null: distinct.len(),
        non_null_distinct,
        min,
        min_id: min_id.max(2),
        max_id: max_id.max(2),
        has_nulls,
        width,
        idf_name: column.idf.clone(),
        idf: encoded.bytes,
        hidx,
        dictionary,
        value_base_id,
        hierarchy,
    })
}

fn validate_values(kind: ColumnKind, values: &[Scalar], name: &str) -> Result<()> {
    for value in values {
        let ok = match (kind, value) {
            (_, Scalar::Blank) => true,
            (ColumnKind::Integer, Scalar::Integer(_)) => true,
            (ColumnKind::Real, Scalar::Integer(_) | Scalar::Decimal(_)) => true,
            (ColumnKind::Currency, Scalar::Currency(_)) => true,
            (ColumnKind::Date, Scalar::DateTime(_)) => true,
            (ColumnKind::Boolean, Scalar::Boolean(_)) => true,
            (ColumnKind::String, Scalar::String(_)) => true,
            (ColumnKind::Empty, _) => false,
            _ => false,
        };
        if !ok {
            return Err(ModelFileError::InvalidModel(format!(
                "column {name} holds a {} value that does not match its {} storage",
                value.type_name(),
                kind.type_name()
            )));
        }
    }
    Ok(())
}

fn numeric_raw(value: &Scalar, real: bool) -> Result<i64> {
    match (value, real) {
        (Scalar::Integer(number), false) => Ok(*number),
        (Scalar::Integer(number), true) => Ok((*number as f64).to_bits() as i64),
        (Scalar::Decimal(number), true) => Ok(number.to_bits() as i64),
        (Scalar::Decimal(number), false) => {
            let rounded = number.round();
            if (number - rounded).abs() > 1.0e-6 {
                return Err(ModelFileError::InvalidModel("a fractional value cannot be stored in a long dictionary".into()));
            }
            Ok(rounded as i64)
        }
        (Scalar::Currency(scaled), false) => {
            if scaled % 100 != 0 {
                return Err(ModelFileError::InvalidModel("currency value is not a whole cent".into()));
            }
            Ok(scaled / 100)
        }
        (Scalar::Currency(scaled), true) => Ok((*scaled as f64 / 10_000.0).to_bits() as i64),
        (Scalar::DateTime(serial), true) => Ok(serial.to_bits() as i64),
        (Scalar::DateTime(serial), false) => Ok(serial.round() as i64),
        (Scalar::Boolean(flag), true) => Ok((if *flag { 1.0f64 } else { 0.0 }).to_bits() as i64),
        (Scalar::Boolean(flag), false) => Ok(i64::from(*flag)),
        (Scalar::String(_), _) | (Scalar::Blank, _) => Err(ModelFileError::InvalidModel("non-numeric value in a numeric dictionary".into())),
    }
}

fn build_column_hidx(column: &ColumnMeta, positions: &[u32]) -> Result<Option<(String, Vec<u8>)>> {
    let Some(name) = &column.hidx else { return Ok(None) };
    Ok(Some((name.clone(), super::vertipaq::encode_hierarchy_hidx(positions)?)))
}

fn build_hierarchy_write(layout: &ModelLayout, name: &str, positions: &[u32]) -> Result<Option<HierarchyWrite>> {
    let Some((_, _, xml)) = layout.hierarchies.iter().find(|(_, hierarchy, _)| hierarchy == name) else {
        return Ok(None);
    };
    // Excel reserves three trailing positions after the distinct data ids (plus the null id when
    // present) and sizes each segment by the number of distinct ids including null.
    let non_null_distinct = positions.len();
    let has_nulls = positions.first() == Some(&2);
    let non_null_distinct = non_null_distinct.saturating_sub(usize::from(has_nulls));
    let total = non_null_distinct.saturating_add(3);
    let records_per_segment = positions.len().max(1);
    let segments = total.div_ceil(records_per_segment);
    let mut ids: Vec<i64> = positions.iter().map(|id| i64::from(*id)).collect();
    // The mapping lists the actual data ids in value order (ids are assigned in value order by
    // both the hash and the value encodings), not a synthetic 3..n range: value-encoded columns
    // legitimately have gaps between ids. The reserved tail is zero-filled.
    ids.resize(total, 0);
    let mut inverse = vec![0i64; total];
    for (position, id) in ids.iter().enumerate() {
        if let Ok(index) = usize::try_from(*id)
            && let Some(slot) = inverse.get_mut(index)
        {
            *slot = i64::try_from(position).map_err(|_| ModelFileError::TooLarge)?;
        }
    }
    // Excel splits the arrays into `segments` records of `records_per_segment` values each (the
    // last one shorter). Reproduce that shape; each record is an independent bit-packed array.
    let encode_segments = |values: &[i64]| -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut start = 0usize;
        for segment in 0..segments {
            let remaining = total.saturating_sub(segment.saturating_mul(records_per_segment));
            let records = records_per_segment.min(remaining);
            let end = start.saturating_add(records).min(values.len());
            bytes.extend_from_slice(&encode_no_split(&values[start..end], 32, 0)?);
            start = end;
        }
        Ok(bytes)
    };
    let mut files = BTreeMap::new();
    for column in parse_columns(xml)? {
        let Some(idf_name) = column.idf else { continue };
        match column.storage.as_str() {
            "POS_TO_ID" => {
                files.insert(idf_name, encode_segments(&ids)?);
            }
            "ID_TO_POS" => {
                files.insert(idf_name, encode_segments(&inverse)?);
            }
            _ => {}
        }
    }
    Ok(Some(HierarchyWrite { name: name.to_string(), total, records_per_segment, segments, files }))
}

fn build_relationship_index(from: &ColumnWrite, to: &ColumnWrite, idf_name: String) -> Result<RelationshipWrite> {
    let mut lookup: BTreeMap<ScalarKey, usize> = BTreeMap::new();
    for (row, value) in to.values.iter().enumerate() {
        if !matches!(value, Scalar::Blank) {
            lookup.entry(ScalarKey::from(value)).or_insert(row);
        }
    }
    let maximum_id = from.ids.iter().copied().max().unwrap_or(2);
    let records = usize::try_from(maximum_id).unwrap_or(0).saturating_add(1).max(3);
    let mut index = vec![-1i64; records];
    let mut seen = BTreeSet::new();
    for (row, id) in from.ids.iter().enumerate() {
        if *id < 3 || !seen.insert(*id) {
            continue;
        }
        let Some(value) = from.values.get(row) else { continue };
        if matches!(value, Scalar::Blank) {
            continue;
        }
        if let Some(primary_row) = lookup.get(&ScalarKey::from(value))
            && let Some(slot) = index.get_mut(*id as usize)
        {
            *slot = i64::try_from(*primary_row).map_err(|_| ModelFileError::TooLarge)?;
        }
    }
    let maximum = index.iter().copied().max().unwrap_or(-1);
    let width = width_for(u64::try_from(maximum + 1).unwrap_or(1).max(1)).unwrap_or(1);
    Ok(RelationshipWrite { records, width, index, idf_name })
}

// ---------------------------------------------------------------------------------------------
// Metadata rewriting
// ---------------------------------------------------------------------------------------------

fn rewrite_table_metadata(xml: &str, table: &TableWrite) -> Result<String> {
    let mut root = parse_xml(xml)?;
    set_table_segment_map(&mut root, table.rows)?;
    if let Some(columns) = collection_mut(&mut root, "Columns") {
        for index in 0..columns.children.len() {
            let is_column = columns
                .children
                .get(index)
                .is_some_and(|child| child.local_name() == "XMObject" && attr(child, "class").is_some_and(|class| class == "XMRawColumn"));
            if !is_column {
                continue;
            }
            let storage = columns.children.get(index).and_then(|child| attr(child, "name")).unwrap_or("").to_string();
            let Some(write) = table.columns.iter().find(|column| column.storage == storage) else { continue };
            if write.unchanged || !write.has_payload {
                // A column without a stored data file (calculated or empty) keeps its original
                // metadata; only its row count may have changed, and writing values would
                // invalidate the expression that produces it.
                continue;
            }
            let object = columns.children.get_mut(index).ok_or_else(|| ModelFileError::Corrupt("column slot".into()))?;
            apply_column_metadata(object, write)?;
        }
    }
    serialize_xml(&root)
}

fn set_table_segment_map(root: &mut XmlNode, rows: usize) -> Result<()> {
    let Some(map) = member_mut(root, "SegmentMap") else { return Ok(()) };
    let class = attr(map, "class").unwrap_or("").to_string();
    if class.contains("XMSegment1Map") {
        set_prop(map, "Records", rows)?;
    } else if class.contains("XMMultiPartSegmentMap") {
        if let Some(partitions) = collection_mut(map, "Partitions")
            && let Some(partition) = partitions.children.iter_mut().find(|child| child.local_name() == "XMObject")
        {
            set_prop(partition, "Records", rows)?;
        }
    } else {
        set_prop(map, "Segments", 1)?;
        set_prop(map, "Records", rows)?;
        set_prop(map, "RecordsPerSegment", rows)?;
    }
    Ok(())
}

fn apply_column_metadata(object: &mut XmlNode, write: &ColumnWrite) -> Result<()> {
    if let Some(hierarchy) = member_mut(object, "IntrinsicHierarchy") {
        set_prop(hierarchy, "DistinctDataIDs", write.distinct_incl_null)?;
    }
    if let Some(stats) = member_mut(object, "ColumnStats") {
        set_prop(stats, "RowCount", write.rows)?;
        // Excel leaves DistinctStates at zero for hash-encoded columns and keeps the column-level
        // OriginalMinSegmentDataID at the reserved null id.
        set_prop(stats, "DistinctStates", if write.dictionary.is_some() { 0 } else { write.distinct_incl_null })?;
        set_prop(stats, "MinDataID", write.min_id)?;
        set_prop(stats, "MaxDataID", write.max_id)?;
        set_prop(stats, "OriginalMinSegmentDataID", 2)?;
        set_prop(stats, "HasNulls", write.has_nulls)?;
        set_prop(stats, "RLERuns", write.rle_runs)?;
        set_prop(stats, "OthersRLERuns", write.others_rle_runs)?;
        if !write.row_number {
            set_prop(stats, "CompressionParam", 0)?;
        }
    }
    if let Some(segments) = collection_mut(object, "Segments") {
        let template =
            segments.children.iter().find(|child| child.local_name() == "XMObject" && attr(child, "class") == Some("XMColumnSegment")).cloned();
        if let Some(mut template) = template {
            segments.children.retain(|child| child.local_name() != "XMObject");
            configure_segment(&mut template, write)?;
            segments.children.push(template);
        }
    }
    if let Some(data_objects) = object.child("DataObjects").cloned() {
        let mut updated = data_objects;
        for data_object in updated.children.iter_mut().filter(|child| child.local_name() == "DataObject") {
            let Some(object) = data_object.children.iter_mut().find(|child| child.local_name() == "XMObject") else { continue };
            let class = attr(object, "class").unwrap_or("").to_string();
            if class.contains("XMValueDataDictionary") {
                if let Some(base_id) = write.value_base_id {
                    set_prop(object, "BaseId", base_id)?;
                }
            } else if class.contains("XMHashDataDictionary") {
                set_prop(object, "LastId", 2 + i64::try_from(write.non_null_distinct).unwrap_or(i64::MAX))?;
                set_prop(object, "Nullable", write.has_nulls)?;
                set_prop(object, "Unique", write.non_null_distinct == write.rows && !write.has_nulls)?;
            } else if class.contains("XMRawColumnPartitionDataObject") {
                set_prop(object, "SegmentCount", 1)?;
            }
        }
        if let Some(slot) = object.children.iter_mut().find(|child| child.local_name() == "DataObjects") {
            *slot = updated;
        }
    }
    Ok(())
}

fn configure_segment(segment: &mut XmlNode, write: &ColumnWrite) -> Result<()> {
    set_prop(segment, "Records", write.rows)?;
    let class = if write.row_number { "XM123CompressionInfo".to_string() } else { format!("XMRENoSplitCompressionInfo<{}>", write.width) };
    if let Some(sub) = member_mut(segment, "SubSegment") {
        set_prop(sub, "Records", write.packed_values)?;
        if let Some(info) = member_mut(sub, "CompressionInfo") {
            set_attr(info, "class", &class);
            set_prop(info, "Min", write.min)?;
        }
        if let Some(stats) = member_mut(sub, "ColumnSegmentStats") {
            // Excel keeps the bit-packed subsegment's statistics as zero placeholders; only the
            // parent segment carries the real counts.
            apply_placeholder_stats(stats)?;
        }
    }
    if let Some(info) = member_mut(segment, "CompressionInfo") {
        set_attr(info, "class", &format!("XMHybridRLECompressionInfo<class {class}>"));
        if let Some(sub) = member_mut(info, "SubCompression") {
            set_attr(sub, "class", &class);
            set_prop(sub, "Min", write.min)?;
        }
    }
    if let Some(stats) = member_mut(segment, "ColumnSegmentStats") {
        apply_segment_stats(stats, write)?;
    }
    Ok(())
}

fn apply_placeholder_stats(stats: &mut XmlNode) -> Result<()> {
    for (name, value) in [
        ("DistinctStates", "0"),
        ("MinDataID", "2"),
        ("MaxDataID", "2"),
        ("OriginalMinSegmentDataID", "2"),
        ("RowCount", "0"),
        ("HasNulls", "false"),
        ("RLERuns", "0"),
        ("OthersRLERuns", "0"),
    ] {
        set_prop(stats, name, value)?;
    }
    Ok(())
}

fn apply_segment_stats(stats: &mut XmlNode, write: &ColumnWrite) -> Result<()> {
    set_prop(stats, "DistinctStates", 0)?;
    set_prop(stats, "MinDataID", write.min_id)?;
    set_prop(stats, "MaxDataID", write.max_id)?;
    // Excel records the declared minimum data id here even when the packed subsegment starts at
    // the reserved null id (`write.min`); the null minimum stays on the subsegment compression.
    set_prop(stats, "OriginalMinSegmentDataID", write.min_id)?;
    set_prop(stats, "RowCount", write.rows)?;
    set_prop(stats, "HasNulls", write.has_nulls)?;
    set_prop(stats, "RLERuns", write.rle_runs)?;
    set_prop(stats, "OthersRLERuns", write.others_rle_runs)?;
    Ok(())
}

fn rewrite_hierarchy_metadata(xml: &str, hierarchy: &HierarchyWrite) -> Result<String> {
    let mut root = parse_xml(xml)?;
    if let Some(map) = member_mut(&mut root, "SegmentMap") {
        set_prop(map, "Segments", hierarchy.segments)?;
        set_prop(map, "Records", hierarchy.total)?;
        set_prop(map, "RecordsPerSegment", hierarchy.records_per_segment)?;
    }
    if let Some(columns) = collection_mut(&mut root, "Columns") {
        for index in 0..columns.children.len() {
            let is_column = columns
                .children
                .get(index)
                .is_some_and(|child| child.local_name() == "XMObject" && attr(child, "class").is_some_and(|class| class == "XMRawColumn"));
            if !is_column {
                continue;
            }
            let storage = columns.children.get(index).and_then(|child| attr(child, "name")).unwrap_or("").to_string();
            if !hierarchy.files.keys().any(|name| name.contains(&format!(".{storage}."))) {
                continue;
            }
            let object = columns.children.get_mut(index).ok_or_else(|| ModelFileError::Corrupt("hierarchy column".into()))?;
            if let Some(segments) = collection_mut(object, "Segments") {
                let template = segments.children.iter().find(|child| child.local_name() == "XMObject").cloned();
                if let Some(template) = template {
                    segments.children.retain(|child| child.local_name() != "XMObject");
                    for segment in 0..hierarchy.segments {
                        let remaining = hierarchy.total.saturating_sub(segment.saturating_mul(hierarchy.records_per_segment));
                        let records = hierarchy.records_per_segment.min(remaining);
                        let mut node = template.clone();
                        set_prop(&mut node, "Records", records)?;
                        if let Some(info) = member_mut(&mut node, "CompressionInfo") {
                            set_attr(info, "class", "XMRENoSplitCompressionInfo<32>");
                            set_prop(info, "Min", 0)?;
                        }
                        if let Some(stats) = member_mut(&mut node, "ColumnSegmentStats") {
                            apply_placeholder_stats(stats)?;
                        }
                        segments.children.push(node);
                    }
                }
            }
            if let Some(data_objects) = object.child("DataObjects").cloned() {
                let mut updated = data_objects;
                for data_object in updated.children.iter_mut() {
                    for object in data_object.children.iter_mut().filter(|child| child.local_name() == "XMObject") {
                        set_prop(object, "SegmentCount", hierarchy.segments)?;
                    }
                }
                if let Some(slot) = object.children.iter_mut().find(|child| child.local_name() == "DataObjects") {
                    *slot = updated;
                }
            }
        }
    }
    serialize_xml(&root)
}

fn rewrite_relationship_metadata(xml: &str, relationship: &RelationshipWrite) -> Result<String> {
    let mut root = parse_xml(xml)?;
    if let Some(map) = member_mut(&mut root, "SegmentMap") {
        set_prop(map, "Records", relationship.records)?;
    }
    if let Some(columns) = collection_mut(&mut root, "Columns")
        && let Some(index) = columns.children.iter().position(|child| child.local_name() == "XMObject" && attr(child, "name") == Some("INDEX"))
    {
        let object = columns.children.get_mut(index).ok_or_else(|| ModelFileError::Corrupt("INDEX column".into()))?;
        if let Some(segments) = collection_mut(object, "Segments") {
            let template = segments.children.iter().find(|child| child.local_name() == "XMObject").cloned();
            if let Some(mut template) = template {
                segments.children.retain(|child| child.local_name() != "XMObject");
                set_prop(&mut template, "Records", relationship.records)?;
                if let Some(info) = member_mut(&mut template, "CompressionInfo") {
                    set_attr(info, "class", &format!("XMRENoSplitCompressionInfo<{}>", relationship.width));
                    set_prop(info, "Min", -1)?;
                }
                segments.children.push(template);
            }
        }
        if let Some(data_objects) = object.child("DataObjects").cloned() {
            let mut updated = data_objects;
            for data_object in updated.children.iter_mut() {
                for object in data_object.children.iter_mut().filter(|child| child.local_name() == "XMObject") {
                    set_prop(object, "SegmentCount", 1)?;
                }
            }
            if let Some(slot) = object.children.iter_mut().find(|child| child.local_name() == "DataObjects") {
                *slot = updated;
            }
        }
    }
    serialize_xml(&root)
}

// ---------------------------------------------------------------------------------------------
// Applying the plan
// ---------------------------------------------------------------------------------------------

fn apply_write_plan(base: &Base<'_>, layout: &ModelLayout, model: &ModelData, plan: WritePlan) -> Result<Vec<u8>> {
    let mut changes: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for table in &plan.tables {
        if table.unchanged {
            continue;
        }
        let Some(table_file) = layout.table(&table.id) else {
            return Err(ModelFileError::Corrupt(format!("table {} vanished", table.name)));
        };
        let xml = rewrite_table_metadata(&base.xml_text(table_file.index)?, table)?;
        changes.insert(table_file.index, xml.into_bytes());
        for column in &table.columns {
            if column.unchanged {
                continue;
            }
            if let Some(name) = &column.idf_name {
                let index = base.find(name).ok_or_else(|| ModelFileError::Corrupt(format!("missing payload {name}")))?;
                changes.insert(index, column.idf.clone());
            }
            if let Some((name, content)) = &column.hidx {
                let index = base.find(name).ok_or_else(|| ModelFileError::Corrupt(format!("missing hierarchy index {name}")))?;
                changes.insert(index, content.clone());
            }
            if let Some((name, content)) = &column.dictionary {
                let index = base.find(name).ok_or_else(|| ModelFileError::Corrupt(format!("missing dictionary {name}")))?;
                changes.insert(index, content.clone());
            }
            if let Some(hierarchy) = &column.hierarchy {
                let Some((index, _, xml)) = layout.hierarchies.iter().find(|(_, name, _)| name == &hierarchy.name) else {
                    return Err(ModelFileError::Corrupt(format!("missing hierarchy metadata {}", hierarchy.name)));
                };
                let rewritten = rewrite_hierarchy_metadata(&serialize_xml(xml)?, hierarchy)?;
                changes.insert(*index, rewritten.into_bytes());
                for (name, content) in &hierarchy.files {
                    let index = base.find(name).ok_or_else(|| ModelFileError::Corrupt(format!("missing hierarchy payload {name}")))?;
                    changes.insert(index, content.clone());
                }
            }
        }
    }
    for relationship in &plan.relationships {
        // The relationship table metadata is located by the payload name it owns.
        let Some((index, _, xml)) = layout.relationship_tables.iter().find(|(_, _, xml)| {
            parse_columns(xml)
                .map(|columns| columns.iter().any(|column| column.idf.as_deref() == Some(relationship.idf_name.as_str())))
                .unwrap_or(false)
        }) else {
            return Err(ModelFileError::Corrupt(format!("missing relationship metadata for {}", relationship.idf_name)));
        };
        let rewritten = rewrite_relationship_metadata(&serialize_xml(xml)?, relationship)?;
        changes.insert(*index, rewritten.into_bytes());
        changes.insert(
            base.find(&relationship.idf_name).ok_or_else(|| ModelFileError::Corrupt("missing relationship payload".into()))?,
            encode_no_split(&relationship.index, relationship.width, -1)?,
        );
    }

    if let Some(script_index) = base.container.measure_script_index()? {
        let content = base.decode(script_index)?;
        let text = content_text(&content);
        let rewritten = super::rewrite_script(&text, &plan.measures)?;
        changes.insert(script_index, rewritten.into_bytes());
    } else if !plan.measures.is_empty() {
        return Err(ModelFileError::NoMeasureScript);
    }
    let _ = model;

    if let Some(log_index) = base.log_index {
        let content = base.decode(log_index)?;
        let sizes: BTreeMap<String, usize> = changes
            .iter()
            .filter(|(index, _)| **index != log_index)
            .map(|(index, content)| (base.storage(*index).to_string(), content.len()))
            .collect();
        let updated = update_log_sizes(&content, &sizes)?;
        changes.insert(log_index, updated);
    }

    let header = &base.container.header;
    let mut blobs = Vec::with_capacity(base.container.entries.len());
    for (index, entry) in base.container.entries.iter().enumerate() {
        match changes.get(&index) {
            Some(content) => {
                let compressed = header.apply_compression && Some(index) != base.log_index;
                blobs.push(super::encode_file(content, compressed, header.error_code)?);
            }
            None => blobs.push(base.container.entry_bytes(entry)?.to_vec()),
        }
    }
    super::rebuild(header, &base.container.entries, &blobs)
}

fn update_log_sizes(content: &[u8], sizes: &BTreeMap<String, usize>) -> Result<Vec<u8>> {
    let text = super::log_text(content)?;
    let mut root = parse_xml(&text)?;
    let mut stack = vec![&mut root];
    while let Some(node) = stack.pop() {
        if node.local_name() == "BackupFile" {
            let storage = node.child("StoragePath").map(|child| child.text.trim().to_string()).unwrap_or_default();
            if let Some(size) = sizes.get(&storage)
                && let Some(slot) = node.children.iter_mut().find(|child| child.local_name() == "Size")
            {
                slot.text = size.to_string();
            }
        }
        for child in &mut node.children {
            stack.push(child);
        }
    }
    let serialized = serialize_xml(&root)?;
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend_from_slice(&super::encode_utf16(&serialized));
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_bom(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend_from_slice(&super::super::encode_utf16(text));
        bytes
    }

    const DEFAULT_HEADER: &str = "<BackupLog><BackupRestoreSyncVersion>150</BackupRestoreSyncVersion><Fault>false</Fault><faultcode>43981</faultcode><ErrorCode>true</ErrorCode><EncryptionFlag>false</EncryptionFlag><EncryptionKey>6</EncryptionKey><ApplyCompression>true</ApplyCompression><m_cbOffsetHeader>4096</m_cbOffsetHeader><DataSize>0</DataSize><Files>2</Files><ObjectID>00000000-0000-0000-0000-000000000000</ObjectID><m_cbOffsetData>4096</m_cbOffsetData></BackupLog>";

    fn empty_container() -> Vec<u8> {
        let header = super::super::Header {
            xml: DEFAULT_HEADER.to_string(),
            error_code: true,
            apply_compression: true,
            data_offset: 4096,
            vdir_offset: 4096,
            vdir_size: 0,
            files: 2,
        };
        let entries = vec![
            super::super::VdirEntry {
                path: "PARTITIONS".into(),
                size: 0,
                offset: 4096,
                delete: "false".into(),
                created: "0".into(),
                access: "0".into(),
                last_write: "0".into(),
            },
            super::super::VdirEntry {
                path: "LOG".into(),
                size: 0,
                offset: 4096,
                delete: "false".into(),
                created: "0".into(),
                access: "0".into(),
                last_write: "0".into(),
            },
        ];
        let partitions = utf16_bom("<Partitions></Partitions>");
        let log = utf16_bom("<BackupLog><CompressionFlag>true</CompressionFlag><FileGroups></FileGroups></BackupLog>");
        let blobs = vec![
            super::super::encode_file(&partitions, false, true).expect("partitions"),
            super::super::encode_file(&log, false, true).expect("log"),
        ];
        super::super::rebuild(&header, &entries, &blobs).expect("rebuild")
    }

    fn set_header_tag(data: &mut [u8], tag: &str, value: &str) {
        let raw = super::super::decode_utf16(&data[72..4096], false).expect("header text");
        let raw = raw.trim_end_matches('\0').to_string();
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = raw.find(&open).expect("open tag") + open.len();
        let end = raw[start..].find(&close).expect("close tag") + start;
        let mut text = raw[..start].to_string();
        text.push_str(value);
        text.push_str(&raw[end..]);
        let mut page = vec![0xFF, 0xFE];
        page.extend_from_slice(&super::super::encode_utf16(super::super::SIGNATURE));
        page.extend_from_slice(&super::super::encode_utf16(&text));
        assert!(page.len() < 4096);
        data[..page.len()].copy_from_slice(&page);
        data[page.len()..4096].fill(0);
    }

    #[test]
    fn empty_container_parses_and_every_truncation_fails() -> Result<()> {
        let data = empty_container();
        let model = read_model(&data)?;
        assert!(model.tables.is_empty());
        for cut in 0..data.len() {
            if data[cut..].iter().all(|byte| *byte == 0) {
                continue;
            }
            assert!(read_model(&data[..cut]).is_err(), "prefix {cut}");
        }
        Ok(())
    }

    #[test]
    fn a_flipped_crc_is_corrupt_not_compression() {
        let mut data = empty_container();
        let container = super::super::Container::parse(&data).expect("parse");
        let log = container.entries.iter().find(|entry| entry.path == "LOG").expect("LOG");
        let last = log.offset + log.size - 1;
        data[last] ^= 0x01;
        match read_model(&data) {
            Err(ModelFileError::Corrupt(_)) => {}
            other => panic!("expected Corrupt, got {other:?}"),
        }
    }

    #[test]
    fn hostile_header_counts_and_sizes_are_rejected() {
        for (tag, value) in [("Files", "100001"), ("DataSize", "999999999"), ("m_cbOffsetHeader", "4097"), ("m_cbOffsetData", "999999")] {
            let mut data = empty_container();
            set_header_tag(&mut data, tag, value);
            assert!(read_model(&data).is_err(), "header variant {tag} was accepted");
        }
    }

    #[test]
    fn writing_a_container_without_a_cube_is_all_or_nothing() {
        let data = empty_container();
        let model = ModelData::default();
        let written = write_model(&data, &model).expect("empty write");
        assert_eq!(read_model(&written).expect("read").tables.len(), 0);
    }
}
