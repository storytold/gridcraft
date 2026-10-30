//! Data-model PivotTables: the pivot cache definition and PivotTable definition Excel needs to
//! open a workbook whose PivotTable is backed by the workbook data model (an OLAP cache), rather
//! than by a worksheet range.
//!
//! [`ModelPivotSpec`] is GridCraft's neutral description of such a pivot (tables, columns,
//! relationships, measures, row/column fields, filters, values, layout and totals). It deliberately
//! names columns and measures (`Sales[Region]`) instead of holding a data-model evaluator: the
//! engine converts its own pivot type to this spec, renders the report, and calls
//! [`write_model_pivot`].
//!
//! Writing takes the report cells (the evaluated grid the engine already produced), writes them
//! onto the sheet, and attaches the package parts the PivotTable needs as the workbook's kept
//! (pass-through) content, so that *every* later save of the workbook keeps them
//! ([`attach_model_pivot`]):
//!
//! * `xl/pivotCache/pivotCacheDefinitionN.xml`, whose `<cacheSource type="external">` names the
//!   model connection (`connectionId`) and whose `cacheFields`/`cacheHierarchies` describe the
//!   columns and measures of the model;
//! * `xl/pivotTables/pivotTableN.xml` with the fields on their axes and the measures in the data
//!   area;
//! * `xl/connections.xml` with the workbook's model connection (`type="5"`, reused or extended when
//!   the workbook already has one);
//! * the workbook's `<pivotCaches>` element and the `x15:dataModel` extension, the cache and
//!   connection relationships, the owning sheet's PivotTable relationship, and (when supplied)
//!   `xl/model/item.data`.
//!
//! Applying the same pivot again (the field-list pane does, once per Apply) updates the pivot the
//! workbook already has under that name: its definition part and cache part are overwritten with
//! the new bytes and its cache id and relationships are kept, so a workbook never collects two
//! pivots with the same name, a second cache with the same id or a second `<pivotCaches>` entry.
//! When the existing part's cache is gone or its relationships no longer point at one, the old
//! part is removed (with every relationship that named it) and a fresh pivot is created instead.
//!
//! Seams with other work on the same files: the `<pivotCaches>` element is kept under the
//! `pivotCaches` slot ([`SLOT_PIVOT_CACHES`]) — PR #2499's per-cache `pivotCache` slot supersedes
//! that representation and only [`add_pivot_cache`] has to change; the `x15:dataModel` extension
//! uses the generic `ext` slot that PR #2492 also writes, so both are concatenated into the same
//! `<extLst>` rather than either owning it.
//!
//! [`write_model_pivot`] attaches and then returns the saved package, for a caller that just wants
//! the bytes.
//!
//! The model database itself (`xl/model/item.data`) is produced by the model storage writer. This
//! module accepts those bytes and places them in the package; without them Excel opens the file
//! (showing the report cells) but cannot refresh the PivotTable — observed on Excel 16.
//!
//! Reading ([`read_model_pivots`]) maps a workbook made by Excel back to [`ModelPivotSpec`]:
//! model tables and relationships come from the workbook's `x15:dataModel` extension, columns and
//! measures from the cache hierarchies, and the axes, filters, values, layout and totals from the
//! PivotTable definition. Calculated members, value filters, grouping and member properties are
//! reported as warnings, following the range-pivot reader's style.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::sync::Arc;

use gridcraft_core::{CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value, number_to_text};
use gridcraft_model::{Preserved, PreservedPart, PreservedRel, PreservedXml, Workbook};

use crate::IoError;
use crate::ReadReport;
use crate::package::{MAX_PART, Package};
use crate::write::{NS_MAIN, NS_REL, XML_DECL};
use crate::xml::{El, esc_attr};

/// Content type of a PivotTable definition part.
pub const CT_PIVOT_TABLE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
/// Content type of a PivotTable cache definition part.
pub const CT_PIVOT_CACHE_DEF: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";
/// Content type of `xl/connections.xml`.
pub const CT_CONNECTIONS: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.connections+xml";
/// Content type of the data model database part (a `Default` on the `data` extension).
pub const CT_MODEL_DATA: &str = "application/vnd.openxmlformats-officedocument.model+data";

/// The workbook part that carries the Spreadsheet Data Model (`xl/model/item.data`).
pub const MODEL_DATA_PART: &str = "xl/model/item.data";
const REL_PIVOT_TABLE: &str = "pivotTable";
const REL_PIVOT_CACHE: &str = "pivotCacheDefinition";
const REL_CONNECTIONS: &str = "connections";
const REL_MODEL: &str = "powerPivotData";
/// The workbook `PreservedXml` slot holding `<pivotCaches>`.
///
/// One whole element per workbook, holding every cache of this workbook's data-model PivotTables.
/// PR #2499 replaces this with one `PreservedXml { slot: "pivotCache" }` entry per cache and a
/// `kept_entries` merge in `PivotWriter::workbook_xml`; when that lands, `add_pivot_cache` is the
/// only function to change — it is the single writer and reader of this slot.
const SLOT_PIVOT_CACHES: &str = "pivotCaches";
const EXT_DATA_MODEL: &str = "{FCE2AD5D-F65C-4FA6-A056-5C36A1767C68}";
const NS_X15: &str = "http://schemas.microsoft.com/office/spreadsheetml/2010/11/main";
/// The model connection every data-model PivotTable's cache points at.
const MODEL_CONNECTION_NAME: &str = "ThisWorkbookDataModel";
/// Most tables, columns, fields and measures accepted in one spec.
const MAX_ITEMS: usize = 65_536;

// ================================================================ public description

/// The data type a model column has, as far as the cache metadata needs to say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelPivotDataType {
    /// Text (`[Sales].[Region]`).
    #[default]
    Text,
    /// A floating-point number.
    Number,
    /// A date/time.
    Date,
    /// A boolean.
    Boolean,
    /// Unknown (written as text metadata).
    Unknown,
}

impl ModelPivotDataType {
    /// The MSOLAP `memberValueDatatype` code Excel writes for the type.
    fn member_value_datatype(self) -> u32 {
        match self {
            ModelPivotDataType::Text | ModelPivotDataType::Unknown => 130,
            ModelPivotDataType::Number => 5,
            ModelPivotDataType::Date => 7,
            ModelPivotDataType::Boolean => 11,
        }
    }
    /// The shared-item element for one value.
    fn shared_item(self, v: &ModelPivotScalar) -> String {
        match v {
            ModelPivotScalar::Blank => "<m/>".to_string(),
            ModelPivotScalar::Number(n) => {
                let _ = self;
                format!("<n v=\"{}\"/>", esc_attr(&number_to_text(*n)))
            }
            ModelPivotScalar::Text(s) => format!("<s v=\"{}\"/>", esc_attr(s)),
            ModelPivotScalar::Boolean(b) => format!("<b v=\"{}\"/>", u8::from(*b)),
        }
    }
}

/// A scalar value in a filter or a shared item.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelPivotScalar {
    /// A blank.
    Blank,
    /// A number.
    Number(f64),
    /// Text.
    Text(String),
    /// A boolean.
    Boolean(bool),
}

impl ModelPivotScalar {
    /// The label Excel's shared item gets.
    pub fn label(&self) -> String {
        match self {
            ModelPivotScalar::Blank => "(blank)".to_string(),
            ModelPivotScalar::Number(n) => number_to_text(*n),
            ModelPivotScalar::Text(s) => s.clone(),
            ModelPivotScalar::Boolean(true) => "TRUE".to_string(),
            ModelPivotScalar::Boolean(false) => "FALSE".to_string(),
        }
    }
}

/// The row-label layout of the pivot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ModelPivotLayout {
    /// One label column, ancestors indented.
    #[default]
    Compact,
    /// One label column per row field, every ancestor repeated.
    Tabular,
    /// One label column per row field, repeated ancestors left blank.
    Outline,
}

/// One column of a model table.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotColumn {
    /// The column name in the model (`Region`).
    pub name: String,
    /// The caption shown in the field list; the name when blank.
    pub caption: String,
    /// The column's data type.
    pub data_type: ModelPivotDataType,
}

/// One table of the workbook data model.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotTable {
    /// The table name as DAX refers to it (`Sales`).
    pub name: String,
    /// The workbook connection the table is loaded from, as `x15:modelTable` records it. The model
    /// storage writer creates that connection; this module only writes the declaration.
    pub connection: String,
    /// The table's columns.
    pub columns: Vec<ModelPivotColumn>,
}

/// A relationship between two model tables.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotRelationship {
    /// The table holding the foreign key.
    pub from_table: String,
    /// The foreign-key column.
    pub from_column: String,
    /// The table holding the primary key.
    pub to_table: String,
    /// The primary-key column.
    pub to_column: String,
}

/// A DAX measure of the model.
///
/// The formula itself is stored in the model database (`xl/model/item.data`); the pivot cache only
/// names the measure. Keep it here so the engine can hand the model writer the same definition and
/// the pivot's value can be matched to it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotMeasure {
    /// The table the measure belongs to.
    pub table: String,
    /// The measure name (`Total Amount`).
    pub name: String,
    /// The DAX expression, without a leading `=` (`SUM(Sales[Amount])`).
    pub formula: String,
    /// Number format code, when the engine has one.
    pub number_format: Option<String>,
}

/// A column reference: a table and a column of the model.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ModelPivotFieldRef {
    /// The table name (`Sales`).
    pub table: String,
    /// The column name (`Region`).
    pub column: String,
}

/// A filter on one column: only the listed values (and optionally blank) pass.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotFilter {
    /// The column filtered.
    pub field: ModelPivotFieldRef,
    /// The allowed values; empty keeps nothing.
    pub allowed: Vec<ModelPivotScalar>,
    /// Whether blank passes as well.
    pub include_blank: bool,
}

/// One measure of the values area.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotValue {
    /// The measure name (`Total Amount`), matching a [`ModelPivotMeasure`].
    pub measure: String,
    /// The caption shown in the report; the measure name when blank.
    pub caption: String,
    /// Number format code, when known.
    pub number_format: Option<String>,
}

/// A data-model PivotTable, independent of any evaluator.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotSpec {
    /// The PivotTable's name (`ModelPivot`).
    pub name: String,
    /// The workbook `cacheId` of the cache backing the table.
    pub cache_id: u32,
    /// The `id` of the model connection in `xl/connections.xml`.
    pub connection_id: u32,
    /// The model's tables, in field-list order.
    pub tables: Vec<ModelPivotTable>,
    /// The model's relationships.
    pub relationships: Vec<ModelPivotRelationship>,
    /// The model's measures.
    pub measures: Vec<ModelPivotMeasure>,
    /// Row fields, outermost first.
    pub row_fields: Vec<ModelPivotFieldRef>,
    /// The members of each row field, in display order (one list per [`Self::row_fields`] entry,
    /// empty when unknown). Excel caches them as the field's shared items and item list; they are
    /// what the pivot's `<rowItems>` index into, so a pivot without them is not reloadable.
    pub row_members: Vec<Vec<ModelPivotScalar>>,
    /// Column fields, outermost first.
    pub column_fields: Vec<ModelPivotFieldRef>,
    /// The members of each column field, in display order (see [`Self::row_members`]).
    pub column_members: Vec<Vec<ModelPivotScalar>>,
    /// Page (slicer-style) filters.
    pub filters: Vec<ModelPivotFilter>,
    /// The measures in the values area, in column order.
    pub values: Vec<ModelPivotValue>,
    /// Row-label layout.
    pub layout: ModelPivotLayout,
    /// Show a subtotal row per row-field prefix group.
    pub row_subtotals: bool,
    /// Show a subtotal column per column-field prefix group.
    pub column_subtotals: bool,
    /// Put subtotals above/before their groups (Excel's default).
    pub subtotal_top: bool,
    /// Show the grand-total row and column.
    pub grand_totals: bool,
    /// Pivot style name; `PivotStyleLight16` when blank.
    pub style: Option<String>,
}

impl Default for ModelPivotSpec {
    fn default() -> Self {
        ModelPivotSpec {
            name: String::new(),
            cache_id: 1,
            connection_id: 1,
            tables: Vec::new(),
            relationships: Vec::new(),
            measures: Vec::new(),
            row_fields: Vec::new(),
            row_members: Vec::new(),
            column_fields: Vec::new(),
            column_members: Vec::new(),
            filters: Vec::new(),
            values: Vec::new(),
            layout: ModelPivotLayout::default(),
            row_subtotals: false,
            column_subtotals: false,
            subtotal_top: false,
            grand_totals: false,
            style: None,
        }
    }
}

/// A data-model PivotTable found in a package, with its sheet and body location.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPivotPartInfo {
    /// The worksheet's name.
    pub sheet: String,
    /// The PivotTable's body location (page fields sit above it).
    pub location: CellRef,
    /// The description read from the package.
    pub spec: ModelPivotSpec,
}

// ================================================================ writing

/// Writes a data-model PivotTable into `wb` and returns the saved `.xlsx` package.
///
/// `report` is the evaluated grid (header/page rows included), written starting at `location` on
/// `sheet`. `model_data`, when given, is the `xl/model/item.data` the model storage writer built for
/// `spec`; without it the file opens but Excel cannot refresh the model-backed PivotTable.
///
/// # Errors
/// A missing sheet, an invalid location or report (outside the sheet), a duplicate cache id, or a
/// reference to a table/column/measure that [`ModelPivotSpec`] does not declare.
///
/// The parts stay attached to `wb` (see [`attach_model_pivot`]): saving it again keeps the pivot.
pub fn write_model_pivot(
    wb: &mut Workbook,
    sheet: &str,
    location: CellRef,
    spec: &ModelPivotSpec,
    report: &[Vec<Value>],
    model_data: Option<&[u8]>,
) -> Result<Vec<u8>, IoError> {
    attach_model_pivot(wb, sheet, location, spec, report, model_data)?;
    crate::write_xlsx(wb)
}

fn write_report(wb: &mut Workbook, sheet_index: usize, location: CellRef, report: &[Vec<Value>]) -> Result<(), IoError> {
    let sheet = wb.sheet_mut(sheet_index).ok_or_else(|| IoError::Format("missing worksheet".into()))?;
    for (r, cells) in report.iter().enumerate() {
        let row = u32::try_from(r)
            .ok()
            .and_then(|r| location.row.checked_add(r))
            .filter(|row| *row < MAX_ROWS)
            .ok_or_else(|| IoError::Format("the PivotTable report does not fit on the sheet".into()))?;
        for (c, value) in cells.iter().enumerate() {
            let col = u32::try_from(c)
                .ok()
                .and_then(|c| location.col.checked_add(c))
                .filter(|col| *col < MAX_COLS)
                .ok_or_else(|| IoError::Format("the PivotTable report does not fit on the sheet".into()))?;
            sheet.set_value(CellRef::new(row, col), value.clone());
        }
    }
    Ok(())
}

/// Checks the spec's self-consistency before any XML is written.
fn validate_spec(spec: &ModelPivotSpec) -> Result<(), IoError> {
    if spec.cache_id == 0 {
        return Err(IoError::Format("a model PivotTable needs a cache id of at least 1".into()));
    }
    if spec.tables.len() > MAX_ITEMS || spec.row_fields.len() > MAX_ITEMS || spec.column_fields.len() > MAX_ITEMS {
        return Err(IoError::Format("the model PivotTable has too many tables or fields".into()));
    }
    if spec.name.trim().is_empty() {
        return Err(IoError::Format("a model PivotTable needs a name".into()));
    }
    if spec.values.len() > MAX_ITEMS || spec.measures.len() > MAX_ITEMS {
        return Err(IoError::Format("the model PivotTable has too many values or measures".into()));
    }
    if !spec.row_members.is_empty() && spec.row_members.len() != spec.row_fields.len() {
        return Err(IoError::Format("the model PivotTable has one row-member list per row field".into()));
    }
    if !spec.column_members.is_empty() && spec.column_members.len() != spec.column_fields.len() {
        return Err(IoError::Format("the model PivotTable has one column-member list per column field".into()));
    }
    let mut seen = std::collections::HashSet::new();
    for f in spec.row_fields.iter().chain(&spec.column_fields) {
        if !spec.tables.iter().any(|t| t.name == f.table && t.columns.iter().any(|c| c.name == f.column)) {
            return Err(IoError::Format(format!("the model PivotTable refers to unknown column {}[{}]", f.table, f.column)));
        }
        if !seen.insert((&f.table, &f.column)) {
            return Err(IoError::Format(format!("the model PivotTable uses {}[{}] on more than one axis", f.table, f.column)));
        }
    }
    for v in &spec.values {
        if !spec.measures.iter().any(|m| m.name == v.measure) {
            return Err(IoError::Format(format!("the model PivotTable value \"{}\" is not a declared measure", v.measure)));
        }
    }
    for f in &spec.filters {
        if !spec.tables.iter().any(|t| t.name == f.field.table && t.columns.iter().any(|c| c.name == f.field.column)) {
            return Err(IoError::Format(format!("the model PivotTable filters unknown column {}[{}]", f.field.table, f.field.column)));
        }
        if spec.row_fields.contains(&f.field) || spec.column_fields.contains(&f.field) {
            return Err(IoError::Format(format!(
                "the model PivotTable uses {}[{}] both as a page filter and on an axis",
                f.field.table, f.field.column
            )));
        }
        if f.allowed.is_empty() && !f.include_blank {
            return Err(IoError::Format(format!(
                "the page filter on {}[{}] keeps no values; a filter needs at least one member",
                f.field.table, f.field.column
            )));
        }
    }
    Ok(())
}

/// One hierarchy in the cache: a table column or a measure.
struct Hierarchy {
    unique_name: String,
    caption: String,
    table: String,
    /// `None` for a measure.
    column: Option<String>,
    data_type: ModelPivotDataType,
}

/// Refuses a cache whose indices would point outside the lists it writes: Excel refuses such a
/// package, so a broken plan must fail here instead of producing a file nothing can open.
fn check_indices(hierarchies: &[Hierarchy], fields: &[CacheField]) -> Result<(), IoError> {
    if hierarchies.is_empty() {
        return Err(IoError::Format("the model PivotTable has no model hierarchy".into()));
    }
    if fields.len() > MAX_ITEMS || hierarchies.len() > MAX_ITEMS {
        return Err(IoError::Format("the model PivotTable has too many cache fields or hierarchies".into()));
    }
    for (i, field) in fields.iter().enumerate() {
        if field.hierarchy >= hierarchies.len() {
            return Err(IoError::Format(format!("cache field {i} points at hierarchy {} of {}", field.hierarchy, hierarchies.len())));
        }
        if field.measure != hierarchies.get(field.hierarchy).is_some_and(|h| h.column.is_none()) {
            return Err(IoError::Format(format!("cache field {i} and hierarchy {} disagree about being a measure", field.hierarchy)));
        }
    }
    Ok(())
}

/// A cache field (the used columns and measures, in field order).
struct CacheField {
    name: String,
    caption: String,
    hierarchy: usize,
    /// `true` for a measure field (`[Measures].[...]`, `level="32767"`).
    measure: bool,
    items: Vec<ModelPivotScalar>,
    /// The filter this field comes from, when it is a page field.
    allowed: Option<Vec<ModelPivotScalar>>,
}

fn esc_scalar(v: &ModelPivotScalar) -> String {
    match v {
        ModelPivotScalar::Blank => "(blank)".to_string(),
        ModelPivotScalar::Number(n) => number_to_text(*n),
        ModelPivotScalar::Text(s) => s.clone(),
        ModelPivotScalar::Boolean(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
    }
}

/// Builds the hierarchy list (every table column, then every measure) and the used cache fields.
#[allow(clippy::type_complexity)] // Hierarchies, cache fields and the column → hierarchy index come out together.
fn plan_fields(spec: &ModelPivotSpec) -> (Vec<Hierarchy>, Vec<CacheField>, HashMap<(String, String), usize>) {
    let mut hierarchies = Vec::new();
    let mut by_column: HashMap<(String, String), usize> = HashMap::new();
    for table in &spec.tables {
        for column in &table.columns {
            by_column.insert((table.name.clone(), column.name.clone()), hierarchies.len());
            hierarchies.push(Hierarchy {
                unique_name: format!("[{}].[{}]", table.name, column.name),
                caption: if column.caption.trim().is_empty() { column.name.clone() } else { column.caption.clone() },
                table: table.name.clone(),
                column: Some(column.name.clone()),
                data_type: column.data_type,
            });
        }
    }
    let mut by_measure = HashMap::new();
    for measure in &spec.measures {
        by_measure.insert(measure.name.clone(), hierarchies.len());
        hierarchies.push(Hierarchy {
            unique_name: format!("[Measures].[{}]", measure.name),
            caption: measure.name.clone(),
            table: measure.table.clone(),
            column: None,
            data_type: ModelPivotDataType::Number,
        });
    }
    let mut fields: Vec<CacheField> = Vec::new();
    let mut field_index: HashMap<(String, String), usize> = HashMap::new();
    let mut add = |table: &str, column: &str, caption: &str, measure: bool, allowed: Option<Vec<ModelPivotScalar>>, fields: &mut Vec<CacheField>| {
        let key = (table.to_string(), column.to_string());
        if let Some(&i) = field_index.get(&key) {
            if allowed.is_some()
                && let Some(f) = fields.get_mut(i)
                && f.allowed.is_none()
            {
                f.allowed = allowed;
            }
            return;
        }
        let hierarchy = if measure { by_measure.get(column).copied().unwrap_or(0) } else { by_column.get(&key).copied().unwrap_or(0) };
        let name = if measure { format!("[Measures].[{column}]") } else { format!("[{table}].[{column}].[{column}]") };
        field_index.insert(key, fields.len());
        fields.push(CacheField {
            name,
            caption: if caption.trim().is_empty() { column.to_string() } else { caption.to_string() },
            hierarchy,
            measure,
            items: allowed.clone().unwrap_or_default(),
            allowed,
        });
    };
    for f in &spec.row_fields {
        let caption =
            hierarchies.get(by_column.get(&(f.table.clone(), f.column.clone())).copied().unwrap_or(0)).map(|h| h.caption.clone()).unwrap_or_default();
        add(&f.table, &f.column, &caption, false, None, &mut fields);
    }
    for f in &spec.column_fields {
        let caption =
            hierarchies.get(by_column.get(&(f.table.clone(), f.column.clone())).copied().unwrap_or(0)).map(|h| h.caption.clone()).unwrap_or_default();
        add(&f.table, &f.column, &caption, false, None, &mut fields);
    }
    for filter in &spec.filters {
        let caption = hierarchies
            .get(by_column.get(&(filter.field.table.clone(), filter.field.column.clone())).copied().unwrap_or(0))
            .map(|h| h.caption.clone())
            .unwrap_or_else(|| filter.field.column.clone());
        let mut allowed = filter.allowed.clone();
        if filter.include_blank && !allowed.iter().any(|v| matches!(v, ModelPivotScalar::Blank)) {
            allowed.push(ModelPivotScalar::Blank);
        }
        add(&filter.field.table, &filter.field.column, &caption, false, Some(allowed), &mut fields);
    }
    for value in &spec.values {
        let caption = if value.caption.trim().is_empty() { value.measure.clone() } else { value.caption.clone() };
        add("", &value.measure, &caption, true, None, &mut fields);
    }
    // The members of the row/column fields are the cache's shared items for them: Excel indexes the
    // pivot's `<items>`/`<rowItems>` into exactly this list.
    let seed = |axes: &[ModelPivotFieldRef], members: &[Vec<ModelPivotScalar>], fields: &mut Vec<CacheField>| {
        for (f, list) in axes.iter().zip(members.iter()) {
            if list.is_empty() {
                continue;
            }
            if let Some(&i) = field_index.get(&(f.table.clone(), f.column.clone()))
                && let Some(field) = fields.get_mut(i)
            {
                field.items = list.clone();
            }
        }
    };
    seed(&spec.row_fields, &spec.row_members, &mut fields);
    seed(&spec.column_fields, &spec.column_members, &mut fields);
    (hierarchies, fields, by_column)
}

fn build_cache_xml(spec: &ModelPivotSpec, connection_id: u32) -> Result<String, IoError> {
    let (hierarchies, fields, _) = plan_fields(spec);
    check_indices(&hierarchies, &fields)?;
    let mut s = format!(
        "{XML_DECL}<pivotCacheDefinition xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\" saveData=\"0\" refreshOnLoad=\"1\" createdVersion=\"6\" refreshedVersion=\"8\" minRefreshableVersion=\"3\" recordCount=\"0\" supportSubquery=\"1\" supportAdvancedDrill=\"1\">"
    );
    let _ = write!(s, "<cacheSource type=\"external\" connectionId=\"{connection_id}\"/>");
    let _ = write!(s, "<cacheFields count=\"{}\">", fields.len());
    for field in &fields {
        let _ = write!(
            s,
            "<cacheField name=\"{}\" caption=\"{}\" numFmtId=\"0\" hierarchy=\"{}\" level=\"{}\"",
            esc_attr(&field.name),
            esc_attr(&field.caption),
            field.hierarchy,
            if field.measure { 32767 } else { 1 }
        );
        if field.measure {
            // A measure's cache field carries no shared items: what it shows is in the model.
            s.push_str("/>");
            continue;
        }
        s.push('>');
        if field.items.is_empty() {
            s.push_str("<sharedItems containsSemiMixedTypes=\"0\" containsNonDate=\"0\" containsString=\"0\"/>");
        } else {
            let data_type = hierarchies.get(field.hierarchy).map(|h| h.data_type).unwrap_or_default();
            let _ = write!(s, "<sharedItems count=\"{}\">", field.items.len());
            for item in &field.items {
                s.push_str(&data_type.shared_item(item));
            }
            s.push_str("</sharedItems>");
        }
        s.push_str("</cacheField>");
    }
    s.push_str("</cacheFields>");

    let _ = write!(s, "<cacheHierarchies count=\"{}\">", hierarchies.len());
    for (i, h) in hierarchies.iter().enumerate() {
        let used = fields.iter().find(|f| f.hierarchy == i);
        let count = used.map(|f| f.items.len()).unwrap_or(0);
        let fields_usage = used.map(|f| {
            let idx = fields.iter().position(|x| x.hierarchy == i && x.name == f.name).unwrap_or(0);
            if f.measure {
                format!("<fieldsUsage count=\"1\"><fieldUsage x=\"{idx}\"/></fieldsUsage>")
            } else {
                format!("<fieldsUsage count=\"2\"><fieldUsage x=\"-1\"/><fieldUsage x=\"{idx}\"/></fieldsUsage>")
            }
        });
        if let Some(column) = &h.column {
            let _ = write!(
                s,
                "<cacheHierarchy uniqueName=\"{}\" caption=\"{}\" attribute=\"1\" defaultMemberUniqueName=\"{}.[All]\" allUniqueName=\"{}.[All]\" dimensionUniqueName=\"[{}]\" displayFolder=\"\" count=\"{count}\" memberValueDatatype=\"{}\" unbalanced=\"0\"",
                esc_attr(&h.unique_name),
                esc_attr(&h.caption),
                esc_attr(&h.unique_name),
                esc_attr(&h.unique_name),
                esc_attr(&h.table),
                h.data_type.member_value_datatype()
            );
            let _ = column;
            match fields_usage {
                Some(u) => {
                    s.push('>');
                    s.push_str(&u);
                    s.push_str("</cacheHierarchy>");
                }
                None => s.push_str("/>"),
            }
        } else {
            let _ = write!(
                s,
                "<cacheHierarchy uniqueName=\"{}\" caption=\"{}\" measure=\"1\" displayFolder=\"\" measureGroup=\"{}\" count=\"0\"",
                esc_attr(&h.unique_name),
                esc_attr(&h.caption),
                esc_attr(&h.table)
            );
            match fields_usage {
                Some(u) => {
                    s.push_str(" oneField=\"1\">");
                    s.push_str(&u);
                    s.push_str("</cacheHierarchy>");
                }
                None => s.push_str("/>"),
            }
        }
    }
    s.push_str("</cacheHierarchies><kpis count=\"0\"/>");

    // Dimensions and measure groups in Excel's order: alphabetical by table, with the Measures
    // dimension where "Measures" sorts (Customers, Date, Measures, Products, Sales) — Excel numbers
    // them that way and the `<maps>` must agree.
    let mut table_names: Vec<String> = spec.tables.iter().map(|t| t.name.clone()).collect();
    table_names.sort();
    let measures_dimension =
        if spec.measures.is_empty() { None } else { Some(table_names.iter().position(|n| n.as_str() > "Measures").unwrap_or(table_names.len())) };
    let mut dimension_of: HashMap<&str, usize> = HashMap::new();
    {
        let mut index = 0usize;
        for (i, name) in table_names.iter().enumerate() {
            if measures_dimension == Some(i) {
                index += 1;
            }
            dimension_of.insert(name.as_str(), index);
            index += 1;
        }
    }
    let _ = write!(s, "<dimensions count=\"{}\">", table_names.len() + usize::from(measures_dimension.is_some()));
    let mut dims = String::new();
    let mut measures_written = false;
    for (i, name) in table_names.iter().enumerate() {
        if measures_dimension == Some(i) {
            dims.push_str("<dimension measure=\"1\" name=\"Measures\" uniqueName=\"[Measures]\" caption=\"Measures\"/>");
            measures_written = true;
        }
        let _ = write!(dims, "<dimension name=\"{}\" uniqueName=\"[{}]\" caption=\"{}\"/>", esc_attr(name), esc_attr(name), esc_attr(name));
    }
    if measures_dimension.is_some() && !measures_written {
        dims.push_str("<dimension measure=\"1\" name=\"Measures\" uniqueName=\"[Measures]\" caption=\"Measures\"/>");
    }
    s.push_str(&dims);
    s.push_str("</dimensions>");

    let _ = write!(s, "<measureGroups count=\"{}\">", table_names.len());
    for name in &table_names {
        let _ = write!(s, "<measureGroup name=\"{}\" caption=\"{}\"/>", esc_attr(name), esc_attr(name));
    }
    s.push_str("</measureGroups>");

    // Maps: each measure group maps to its own dimension and to dimensions across a relationship.
    let mut maps: Vec<(String, String)> = Vec::new();
    for (i, name) in table_names.iter().enumerate() {
        maps.push((name.clone(), dimension_of.get(name.as_str()).copied().unwrap_or(i).to_string()));
    }
    for rel in &spec.relationships {
        if let (Some(&to_dim), Some(_)) = (dimension_of.get(rel.to_table.as_str()), dimension_of.get(rel.from_table.as_str())) {
            maps.push((rel.from_table.clone(), to_dim.to_string()));
        }
        if let (Some(&from_dim), Some(_)) = (dimension_of.get(rel.from_table.as_str()), dimension_of.get(rel.to_table.as_str())) {
            maps.push((rel.to_table.clone(), from_dim.to_string()));
        }
    }
    let mut seen_maps = std::collections::HashSet::new();
    let mut map_xml = String::new();
    let mut count = 0usize;
    for (table, dim) in maps {
        if !seen_maps.insert((table.clone(), dim.clone())) {
            continue;
        }
        if let Some(group) = table_names.iter().position(|n| *n == table) {
            let _ = write!(map_xml, "<map measureGroup=\"{group}\" dimension=\"{dim}\"/>");
            count += 1;
        }
    }
    let _ = write!(s, "<maps count=\"{count}\">{map_xml}</maps>");
    s.push_str("<extLst><ext uri=\"{725AE2AE-9491-48be-B2B4-4EB974FC3084}\" xmlns:x14=\"http://schemas.microsoft.com/office/spreadsheetml/2009/9/main\"><x14:pivotCacheDefinition supportSubqueryNonVisual=\"1\" supportSubqueryCalcMem=\"1\" supportAddCalcMems=\"1\"/></ext></extLst>");
    s.push_str("</pivotCacheDefinition>");
    Ok(s)
}

fn build_table_xml(spec: &ModelPivotSpec, anchor: CellRef, rows: usize, cols: usize) -> String {
    let (hierarchies, fields, _) = plan_fields(spec);
    let last_row = u32::try_from(rows.saturating_sub(1)).unwrap_or(u32::MAX);
    let last_col = u32::try_from(cols.saturating_sub(1)).unwrap_or(u32::MAX);
    let end = CellRef::new(anchor.row.saturating_add(last_row).min(MAX_ROWS - 1), anchor.col.saturating_add(last_col).min(MAX_COLS - 1));
    let location = RangeRef::new(anchor, end);
    let name = if spec.name.trim().is_empty() { format!("PivotTable{}", spec.cache_id) } else { spec.name.clone() };
    let mut s = format!(
        "{XML_DECL}<pivotTableDefinition xmlns=\"{NS_MAIN}\" name=\"{}\" cacheId=\"{}\" applyNumberFormats=\"0\" applyBorderFormats=\"0\" applyFontFormats=\"0\" applyPatternFormats=\"0\" applyAlignmentFormats=\"0\" applyWidthHeightFormats=\"1\" dataCaption=\"Values\" updatedVersion=\"8\" minRefreshableVersion=\"3\" useAutoFormatting=\"1\" itemPrintTitles=\"1\" createdVersion=\"6\" indent=\"0\"",
        esc_attr(&name),
        spec.cache_id
    );
    match spec.layout {
        ModelPivotLayout::Compact => s.push_str(" outline=\"1\" outlineData=\"1\""),
        ModelPivotLayout::Outline => s.push_str(" compact=\"0\" compactData=\"0\" outline=\"1\" outlineData=\"1\""),
        ModelPivotLayout::Tabular => s.push_str(" compact=\"0\" compactData=\"0\""),
    }
    if !spec.grand_totals {
        s.push_str(" rowGrandTotals=\"0\" colGrandTotals=\"0\"");
    }
    s.push_str(" multipleFieldFilters=\"0\">");

    let first_header_row = usize::from(!spec.column_fields.is_empty());
    let first_data_row = first_header_row + 1;
    let first_data_col = match spec.layout {
        ModelPivotLayout::Compact => usize::from(!spec.row_fields.is_empty()),
        _ => spec.row_fields.len(),
    };
    let _ = write!(
        s,
        "<location ref=\"{}\" firstHeaderRow=\"{first_header_row}\" firstDataRow=\"{first_data_row}\" firstDataCol=\"{first_data_col}\"",
        location.a1()
    );
    if !spec.filters.is_empty() {
        let _ = write!(s, " rowPageCount=\"{}\" colPageCount=\"1\"", spec.filters.len());
    }
    s.push_str("/>");

    // Field index → its position among the written fields (== cacheFields order).
    let field_index = |table: &str, column: &str| -> Option<usize> {
        fields
            .iter()
            .position(|f| if f.measure { f.name == format!("[Measures].[{column}]") } else { f.name == format!("[{table}].[{column}].[{column}]") })
    };
    let _ = write!(s, "<pivotFields count=\"{}\">", fields.len());
    for (i, f) in fields.iter().enumerate() {
        let row = spec.row_fields.iter().any(|r| field_index(&r.table, &r.column) == Some(i));
        let col = spec.column_fields.iter().any(|r| field_index(&r.table, &r.column) == Some(i));
        let page = spec.filters.iter().any(|r| field_index(&r.field.table, &r.field.column) == Some(i));
        s.push_str("<pivotField");
        if row {
            s.push_str(" axis=\"axisRow\"");
        } else if col {
            s.push_str(" axis=\"axisCol\"");
        } else if page {
            s.push_str(" axis=\"axisPage\"");
        }
        if f.measure {
            s.push_str(" dataField=\"1\"");
        }
        // Excel refuses a package whose row/column fields are not marked as fully drilled:
        // `allDrilled="1"` on every axis field, row and column alike (verified by Excel bisect).
        if row || col {
            s.push_str(" allDrilled=\"1\"");
        }
        match spec.layout {
            ModelPivotLayout::Compact => {}
            ModelPivotLayout::Outline => s.push_str(" compact=\"0\""),
            ModelPivotLayout::Tabular => s.push_str(" compact=\"0\" outline=\"0\""),
        }
        s.push_str(" showAll=\"0\"");
        let subtotal = if row {
            spec.row_subtotals
        } else if col {
            spec.column_subtotals
        } else {
            false
        };
        if !subtotal {
            s.push_str(" defaultSubtotal=\"0\"");
        }
        if !spec.subtotal_top {
            s.push_str(" subtotalTop=\"0\"");
        }
        if page && f.allowed.as_ref().is_some_and(|a| a.len() != 1) {
            s.push_str(" multipleItemSelectionAllowed=\"1\"");
        }
        // An axis field lists its members here (Excel indexes `<rowItems>`/`<colItems>` into this
        // list); a filtered field lists the members it keeps (a single value is named by its
        // `<pageField>`).
        let shown: &[ModelPivotScalar] = if page { f.allowed.as_deref().unwrap_or(&[]) } else { f.items.as_slice() };
        if (page && shown.len() < 2) || (!page && shown.is_empty()) {
            s.push_str("/>");
        } else {
            s.push('>');
            let _ = write!(s, "<items count=\"{}\">", shown.len());
            for i in 0..shown.len() {
                let _ = write!(s, "<item x=\"{i}\"/>");
            }
            s.push_str("</items></pivotField>");
        }
    }
    s.push_str("</pivotFields>");

    // The item cache of one axis: one `<i>` per member, the first as `<x/>` (index 0), then the
    // grand total row Excel always ends an axis with.
    let item_cache = |tag: &str, list: &[usize]| -> String {
        let counts: Vec<usize> = list.iter().map(|i| fields.get(*i).map(|f| f.items.len()).unwrap_or(0)).collect();
        if counts.iter().all(|c| *c == 0) {
            return String::new();
        }
        let total: usize = counts.iter().sum::<usize>().saturating_add(usize::from(spec.grand_totals));
        let mut out = format!("<{tag} count=\"{total}\">");
        for (k, count) in counts.iter().enumerate() {
            for i in 0..*count {
                if k == 0 && i == 0 {
                    out.push_str("<i><x/></i>");
                } else {
                    let _ = write!(out, "<i><x v=\"{i}\"/></i>");
                }
            }
        }
        if spec.grand_totals {
            out.push_str("<i t=\"grand\"><x/></i>");
        }
        let _ = write!(out, "</{tag}>");
        out
    };
    let row_x: Vec<usize> = spec.row_fields.iter().filter_map(|f| field_index(&f.table, &f.column)).collect();
    let col_x: Vec<usize> = spec.column_fields.iter().filter_map(|f| field_index(&f.table, &f.column)).collect();
    let values_on_columns = spec.values.len() > 1;
    let axis_xml = |tag: &str, list: &[usize], values: bool| -> String {
        let n = list.len() + usize::from(values);
        if n == 0 {
            return String::new();
        }
        let mut out = format!("<{tag} count=\"{n}\">");
        for i in list {
            let _ = write!(out, "<field x=\"{i}\"/>");
        }
        if values {
            out.push_str("<field x=\"-2\"/>");
        }
        let _ = write!(out, "</{tag}>");
        out
    };
    s.push_str(&axis_xml("rowFields", &row_x, false));
    s.push_str(&item_cache("rowItems", &row_x));
    s.push_str(&axis_xml("colFields", &col_x, values_on_columns));
    s.push_str(&item_cache("colItems", &col_x));

    if !spec.filters.is_empty() {
        let _ = write!(s, "<pageFields count=\"{}\">", spec.filters.len());
        for filter in &spec.filters {
            let Some(idx) = field_index(&filter.field.table, &filter.field.column) else { continue };
            let hier = fields.get(idx).map(|f| f.hierarchy).unwrap_or(0);
            let allowed = fields.get(idx).and_then(|f| f.allowed.clone()).unwrap_or_default();
            let single = if allowed.len() == 1 { allowed.first().cloned() } else { None };
            let _ = write!(s, "<pageField fld=\"{idx}\" hier=\"{hier}\"");
            match &single {
                Some(v) => {
                    // No `item` attribute: the member is named by `name`, as Excel writes it. An
                    // `item` index would point into `<items>`, which a single-value filter has none of.
                    let member = format!("[{}].[{}].&[{}]", filter.field.table, filter.field.column, esc_attr(&esc_scalar(v)));
                    let _ = write!(s, " name=\"{}\" cap=\"{}\"", esc_attr(&member), esc_attr(&v.label()));
                }
                None => {
                    let member = format!("[{}].[{}].[All]", filter.field.table, filter.field.column);
                    let _ = write!(s, " name=\"{}\" cap=\"All\"", esc_attr(&member));
                }
            }
            s.push_str("/>");
        }
        s.push_str("</pageFields>");
    }

    if !spec.values.is_empty() {
        let _ = write!(s, "<dataFields count=\"{}\">", spec.values.len());
        for value in &spec.values {
            if let Some(idx) = field_index("", &value.measure) {
                let _ = write!(s, "<dataField fld=\"{idx}\" subtotal=\"count\" baseField=\"0\" baseItem=\"0\"/>");
            }
        }
        s.push_str("</dataFields>");
    }

    let _ = write!(s, "<pivotHierarchies count=\"{}\">", hierarchies.len());
    for h in &hierarchies {
        match &h.column {
            Some(_) => s.push_str("<pivotHierarchy dragToData=\"1\"/>"),
            None => s.push_str("<pivotHierarchy dragToRow=\"0\" dragToCol=\"0\" dragToPage=\"0\" dragToData=\"1\"/>"),
        }
    }
    s.push_str("</pivotHierarchies>");
    let style = spec.style.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "PivotStyleLight16".to_string());
    let _ = write!(
        s,
        "<pivotTableStyleInfo name=\"{}\" showRowHeaders=\"1\" showColHeaders=\"1\" showRowStripes=\"0\" showColStripes=\"0\" showLastColumn=\"1\"/>",
        esc_attr(&style)
    );
    let usages = |tag: &str, usage: &str, list: &[usize], values: bool| -> String {
        let n = list.len() + usize::from(values);
        if n == 0 {
            return String::new();
        }
        let mut out = format!("<{tag} count=\"{n}\">");
        for i in list {
            let hier = fields.get(*i).map(|f| f.hierarchy).unwrap_or(0);
            let _ = write!(out, "<{usage} hierarchyUsage=\"{hier}\"/>");
        }
        if values {
            let _ = write!(out, "<{usage} hierarchyUsage=\"-2\"/>");
        }
        let _ = write!(out, "</{tag}>");
        out
    };
    s.push_str(&usages("rowHierarchiesUsage", "rowHierarchyUsage", &row_x, false));
    s.push_str(&usages("colHierarchiesUsage", "colHierarchyUsage", &col_x, values_on_columns));
    s.push_str("</pivotTableDefinition>");
    s
}

// ---------------------------------------------------------------- attaching to a workbook

fn rel_uri(kind: &str) -> String {
    format!("http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}")
}

/// The workbook's pass-through content, created (empty) when the workbook has none yet.
fn preserved(wb: &mut Workbook) -> &mut Preserved {
    Arc::make_mut(wb.preserved.get_or_insert_with(|| Arc::new(Preserved::default())))
}

/// A relationship id that no kept relationship of `p` uses. The writer reassigns every id, so it
/// only has to be unique here.
fn fresh_rel_id(p: &Preserved) -> String {
    let mut n = 1usize;
    loop {
        let id = format!("gcRel{n}");
        if !p.rels.iter().any(|r| r.id == id) {
            return id;
        }
        n = n.saturating_add(1);
    }
}

/// The relationship of `kind` to `target`, adding it when the workbook has none.
fn ensure_rel(p: &mut Preserved, kind: &str, target: &str) -> String {
    let ty = rel_uri(kind);
    if let Some(r) = p.rels.iter().find(|r| r.rel_type == ty && r.target.eq_ignore_ascii_case(target)) {
        return r.id.clone();
    }
    let id = fresh_rel_id(p);
    p.rels.push(PreservedRel { id: id.clone(), rel_type: ty, target: target.to_string(), external: false });
    id
}

/// A part name in `dir` that no kept part uses.
fn fresh_part_name(p: &Preserved, dir: &str, stem: &str, ext: &str, start: u32) -> String {
    let mut n = start.max(1);
    loop {
        let name = format!("{dir}/{stem}{n}{ext}");
        if !p.parts.iter().any(|q| q.name.eq_ignore_ascii_case(&name)) {
            return name;
        }
        n = n.saturating_add(1);
    }
}

fn append_xml_before(xml: &mut String, close: &str, insert: &str) -> bool {
    match xml.rfind(close) {
        Some(pos) => {
            xml.insert_str(pos, insert);
            true
        }
        None => false,
    }
}

fn model_connection_xml(id: u32) -> String {
    format!(
        "<connection id=\"{id}\" keepAlive=\"1\" name=\"{MODEL_CONNECTION_NAME}\" description=\"Data Model\" type=\"5\" refreshedVersion=\"8\" minRefreshableVersion=\"5\" background=\"1\"><dbPr connection=\"Data Model Connection\" command=\"Model\" commandType=\"1\"/><olapPr sendLocale=\"1\" rowDrillCount=\"1000\"/><extLst><ext uri=\"{{DE250136-89BD-433C-8126-D09CA5730AF9}}\" xmlns:x15=\"{NS_X15}\"><x15:connection id=\"\" model=\"1\"/></ext></extLst></connection>"
    )
}

/// The `id` of the workbook's model connection in a `xl/connections.xml` part.
fn model_connection_id(xml: &str) -> Option<u32> {
    let at = xml.find(&format!("name=\"{MODEL_CONNECTION_NAME}\""))?;
    let start = xml.get(..at)?.rfind('<')?;
    let end = at.checked_add(xml.get(at..)?.find('>')?)?;
    let tag = xml.get(start..end)?;
    let value = tag.split(" id=\"").nth(1)?.split('"').next()?;
    value.trim().parse::<u32>().ok()
}

/// Makes sure `xl/connections.xml` declares the model connection; returns the `id` it has there
/// (the cache must point at it).
fn ensure_connections(p: &mut Preserved, connection_id: u32) -> u32 {
    let wanted = connection_id.max(1);
    if let Some(part) = p.parts.iter_mut().find(|q| q.name.eq_ignore_ascii_case("xl/connections.xml")) {
        let text = String::from_utf8_lossy(&part.data).into_owned();
        if let Some(id) = model_connection_id(&text) {
            return id;
        }
        let mut patched = text;
        if append_xml_before(&mut patched, "</connections>", &model_connection_xml(wanted)) {
            part.data = Arc::from(patched.into_bytes());
            return wanted;
        }
        return wanted;
    }
    let xml = format!("{XML_DECL}<connections xmlns=\"{NS_MAIN}\">{}</connections>", model_connection_xml(wanted));
    p.parts.push(PreservedPart {
        name: "xl/connections.xml".to_string(),
        content_type: CT_CONNECTIONS.to_string(),
        data: Arc::from(xml.into_bytes()),
        ..Default::default()
    });
    wanted
}

/// Fills the tables without a connection name from the workbook's kept `xl/connections.xml`: Excel
/// names a worksheet connection `WorksheetConnection_<file>!<Table>`, and `<x15:modelTable>` must
/// carry that name or Excel loads the package but never shows the PivotTable.
fn resolve_connections(p: &Preserved, spec: &ModelPivotSpec) -> ModelPivotSpec {
    if spec.tables.iter().all(|t| !t.connection.trim().is_empty()) {
        return spec.clone();
    }
    let Some(part) = p.parts.iter().find(|q| q.name.eq_ignore_ascii_case("xl/connections.xml")) else { return spec.clone() };
    let text = String::from_utf8_lossy(&part.data);
    let names: Vec<&str> = text.split("name=\"").skip(1).filter_map(|rest| rest.split('"').next()).collect();
    let mut out = spec.clone();
    for table in &mut out.tables {
        if !table.connection.trim().is_empty() {
            continue;
        }
        let suffix = format!("!{}", table.name);
        if let Some(name) = names.iter().find(|n| n.ends_with(&suffix)) {
            table.connection = (*name).to_string();
        }
    }
    out
}

/// The workbook's `x15:dataModel` extension, or `None` when it cannot be written faithfully.
///
/// Excel refuses a package whose `<x15:modelTable>` has an empty `connection` attribute (observed
/// by bisecting opens of the same package with and without the attribute), so a table without a
/// usable connection name gets no `connection` attribute at all, and a spec with no usable name
/// anywhere gets no extension at all — which Excel accepts.
fn data_model_ext(spec: &ModelPivotSpec) -> Option<String> {
    if !spec.tables.iter().any(|t| !t.connection.trim().is_empty()) {
        return None;
    }
    let mut s = format!("<ext uri=\"{EXT_DATA_MODEL}\" xmlns:x15=\"{NS_X15}\"><x15:dataModel><x15:modelTables>");
    for table in &spec.tables {
        let hash = fnv1a(&table.name);
        let mut attrs = format!("id=\"{}_{hash:016x}\" name=\"{}\"", esc_attr(&table.name), esc_attr(&table.name));
        if !table.connection.trim().is_empty() {
            let _ = write!(attrs, " connection=\"{}\"", esc_attr(&table.connection));
        }
        let _ = write!(s, "<x15:modelTable {attrs}/>");
    }
    s.push_str("</x15:modelTables>");
    if !spec.relationships.is_empty() {
        s.push_str("<x15:modelRelationships>");
        for rel in &spec.relationships {
            let _ = write!(
                s,
                "<x15:modelRelationship fromTable=\"{}\" fromColumn=\"{}\" toTable=\"{}\" toColumn=\"{}\"/>",
                esc_attr(&rel.from_table),
                esc_attr(&rel.from_column),
                esc_attr(&rel.to_table),
                esc_attr(&rel.to_column)
            );
        }
        s.push_str("</x15:modelRelationships>");
    }
    s.push_str("</x15:dataModel></ext>");
    Some(s)
}

fn fnv1a(s: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for b in s.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

/// Adds `cacheId`/`r:id` to the workbook's `<pivotCaches>`, creating the element when absent.
fn add_pivot_cache(p: &mut Preserved, cache_id: u32, rid: &str) -> Result<(), IoError> {
    // `cacheId="0"` is not a valid pivot cache id: the first cache is 1.
    let cache_id = cache_id.max(1);
    let child = format!("<pivotCache cacheId=\"{cache_id}\" r:id=\"{}\"/>", esc_attr(rid));
    if let Some(element) = p.elements.iter_mut().find(|e| e.slot == SLOT_PIVOT_CACHES) {
        if element.xml.contains(&format!("cacheId=\"{cache_id}\"")) {
            return Ok(());
        }
        if !append_xml_before(&mut element.xml, "</pivotCaches>", &child) {
            return Err(IoError::Xml("the workbook's kept <pivotCaches> element is not closed".into()));
        }
        return Ok(());
    }
    p.elements.push(PreservedXml { slot: SLOT_PIVOT_CACHES.to_string(), xml: format!("<pivotCaches xmlns:r=\"{NS_REL}\">{child}</pivotCaches>") });
    Ok(())
}

/// Keeps the workbook's `x15:dataModel` declaration in step with `spec` (tables and relationships).
fn set_data_model_ext(p: &mut Preserved, spec: &ModelPivotSpec) {
    // No table names a connection: leave whatever the file already declares alone and write none
    // of our own (an extension with empty connection attributes is refused by Excel).
    let Some(xml) = data_model_ext(spec) else { return };
    match p.elements.iter_mut().find(|e| e.slot == "ext" && e.xml.contains("x15:dataModel")) {
        Some(element) => element.xml = xml,
        None => p.elements.push(PreservedXml { slot: "ext".to_string(), xml }),
    }
}

/// The sheet's relationship to its PivotTable definition part.
fn attach_sheet_rel(wb: &mut Workbook, sheet_index: usize, table_part: &str) -> Result<(), IoError> {
    let sheet = wb.sheet_mut(sheet_index).ok_or_else(|| IoError::Format("missing worksheet".into()))?;
    let p = Arc::make_mut(sheet.preserved.get_or_insert_with(|| Arc::new(Preserved::default())));
    let ty = rel_uri(REL_PIVOT_TABLE);
    if p.rels.iter().any(|r| r.rel_type == ty && r.target.eq_ignore_ascii_case(table_part)) {
        return Ok(());
    }
    let id = fresh_rel_id(p);
    p.rels.push(PreservedRel { id, rel_type: ty, target: table_part.to_string(), external: false });
    Ok(())
}

/// A PivotTable the workbook already keeps under the name a new spec wants to use.
enum ExistingPivot {
    /// Its definition part, cache part and cache id can be written over in place.
    Usable { table_part: String, cache_part: String, cache_id: u32 },
    /// A part with the name exists, but its cache part is missing or its relationships no longer
    /// name one: it cannot be updated faithfully, so it is removed and the pivot is recreated.
    Stale { table_part: String },
}

/// The definition part under `xl/pivotTables/` whose `name` is `name`, if the workbook keeps one.
fn pivot_part_named<'a>(p: &'a Preserved, name: &str) -> Option<&'a PreservedPart> {
    p.parts.iter().find(|part| {
        let lower = part.name.to_ascii_lowercase();
        if !lower.starts_with("xl/pivottables/") || !lower.ends_with(".xml") {
            return false;
        }
        crate::xml::parse(&part.data)
            .is_ok_and(|root| root.name == "pivotTableDefinition" && root.attr("name").is_some_and(|n| n.eq_ignore_ascii_case(name)))
    })
}

/// The PivotTable definition part named `name` and the cache it uses, recovered from the part's
/// own `pivotCacheDefinition` relationship.
fn existing_pivot(p: &Preserved, name: &str) -> Option<ExistingPivot> {
    let part = pivot_part_named(p, name)?;
    let usable = (|| {
        let root = crate::xml::parse(&part.data).ok()?;
        let cache_id = root.attr_u32("cacheId").filter(|id| *id > 0)?;
        let rel = part.rels.iter().find(|r| !r.external && r.rel_type == rel_uri(REL_PIVOT_CACHE))?;
        let cache = p.parts.iter().find(|q| q.name.eq_ignore_ascii_case(&rel.target))?;
        Some((cache.name.clone(), cache_id))
    })();
    match usable {
        Some((cache_part, cache_id)) => Some(ExistingPivot::Usable { table_part: part.name.clone(), cache_part, cache_id }),
        None => Some(ExistingPivot::Stale { table_part: part.name.clone() }),
    }
}

/// Drops a PivotTable definition part that cannot be updated in place, together with its cache and
/// every relationship or `<pivotCaches>` entry that named either of them, so the recreated pivot
/// leaves neither a duplicate name nor a dangling relationship behind.
fn drop_stale_pivot(wb: &mut Workbook, table_part: &str) {
    let cache_part = wb
        .preserved
        .as_deref()
        .and_then(|p| p.parts.iter().find(|q| q.name.eq_ignore_ascii_case(table_part)))
        .and_then(|part| part.rels.iter().find(|r| !r.external && r.rel_type == rel_uri(REL_PIVOT_CACHE)))
        .map(|r| r.target.clone());
    if let (Some(p), Some(cache)) = (wb.preserved.as_mut().map(Arc::make_mut), cache_part.as_deref()) {
        // The cache part goes only when nothing else uses it; its workbook relationships and
        // `<pivotCaches>` entries always go, whether or not the part still exists.
        let shared = p
            .parts
            .iter()
            .any(|q| !q.name.eq_ignore_ascii_case(table_part) && q.rels.iter().any(|r| !r.external && r.target.eq_ignore_ascii_case(cache)));
        let ty = rel_uri(REL_PIVOT_CACHE);
        let mut ids: Vec<String> = vec![];
        p.rels.retain(|r| {
            let drop = r.rel_type == ty && r.target.eq_ignore_ascii_case(cache);
            if drop {
                ids.push(r.id.clone());
            }
            !drop
        });
        drop_pivot_cache_entries(p, &ids);
        if !shared {
            p.parts.retain(|q| !q.name.eq_ignore_ascii_case(cache));
        }
    }
    let ty = rel_uri(REL_PIVOT_TABLE);
    let targets_part = |r: &PreservedRel| r.rel_type == ty && r.target.eq_ignore_ascii_case(table_part);
    if let Some(p) = wb.preserved.as_mut().map(Arc::make_mut) {
        p.parts.retain(|q| !q.name.eq_ignore_ascii_case(table_part));
        p.rels.retain(|r| !targets_part(r));
    }
    for i in 0..wb.sheets.len() {
        let Some(sheet) = wb.sheet_mut(i) else { continue };
        if let Some(preserved) = sheet.preserved.as_mut().map(Arc::make_mut) {
            preserved.rels.retain(|r| !targets_part(r));
        }
    }
}

/// Removes the `<pivotCache>` entries of the kept `<pivotCaches>` element whose `r:id` is in `ids`;
/// the element itself goes when no entry is left (adding a cache recreates it).
fn drop_pivot_cache_entries(p: &mut Preserved, ids: &[String]) {
    if ids.is_empty() {
        return;
    }
    let Some(element) = p.elements.iter_mut().find(|e| e.slot == SLOT_PIVOT_CACHES) else { return };
    let wanted: Vec<String> = ids.iter().map(|id| format!("r:id=\"{}\"", esc_attr(id))).collect();
    let mut out = String::with_capacity(element.xml.len());
    let mut rest = element.xml.as_str();
    while let Some(at) = rest.find("<pivotCache ") {
        out.push_str(rest.get(..at).unwrap_or_default());
        let tail = rest.get(at..).unwrap_or_default();
        let end = tail.find("/>").map(|i| i.saturating_add(2)).unwrap_or(tail.len());
        let tag = tail.get(..end).unwrap_or_default();
        if !wanted.iter().any(|id| tag.contains(id.as_str())) {
            out.push_str(tag);
        }
        rest = tail.get(end..).unwrap_or_default();
    }
    out.push_str(rest);
    element.xml = out;
    if !element.xml.contains("<pivotCache ") {
        p.elements.retain(|e| e.slot != SLOT_PIVOT_CACHES);
    }
}

/// Drops the `pivotTable` relationship of every sheet other than `keep` that points at
/// `table_part`: an updated PivotTable lives on the sheet it was applied to, once.
fn remove_other_sheet_rels(wb: &mut Workbook, table_part: &str, keep: usize) {
    let ty = rel_uri(REL_PIVOT_TABLE);
    for i in 0..wb.sheets.len() {
        if i == keep {
            continue;
        }
        let Some(sheet) = wb.sheet_mut(i) else { continue };
        if let Some(p) = sheet.preserved.as_mut().map(Arc::make_mut) {
            p.rels.retain(|r| !(r.rel_type == ty && r.target.eq_ignore_ascii_case(table_part)));
        }
    }
}

/// Writes a data-model PivotTable into `wb` so that every later save keeps it.
///
/// The report cells go on `sheet` starting at `location`; the package parts the PivotTable needs
/// (its cache definition, its definition part, `xl/connections.xml` with the model connection and,
/// when given, the model database) are added as the workbook's kept (pass-through) content: the
/// workbook's `<pivotCaches>` element, the cache/connection relationships, the sheet's
/// relationship to the PivotTable part and the `x15:dataModel` extension. Nothing is serialized
/// here — the next [`crate::write_xlsx`] of `wb` produces the package.
///
/// When the workbook already keeps a PivotTable named `spec.name`, that pivot is updated: its
/// definition part, its cache part and their relationship ids stay, so the workbook never holds
/// two pivots of the same name. A kept part whose name matches but whose cache part is missing or
/// unrelated is dropped (with the relationships that named it) and the pivot is created fresh.
///
/// # Errors
/// A missing sheet, an invalid location or report (outside the sheet), a duplicate cache id, a
/// model database larger than [`MAX_PART`], or a reference to a table/column/measure that
/// [`ModelPivotSpec`] does not declare.
pub fn attach_model_pivot(
    wb: &mut Workbook,
    sheet: &str,
    location: CellRef,
    spec: &ModelPivotSpec,
    report: &[Vec<Value>],
    model_data: Option<&[u8]>,
) -> Result<(), IoError> {
    // Excel ignores a PivotTable whose `cacheId` is 0 (it loads the model but never shows the
    // pivot), so a caller's placeholder id is clamped instead of failing; a real id is kept as is.
    let mut spec = spec.clone();
    spec.cache_id = spec.cache_id.max(1);
    let spec = &spec;
    validate_spec(spec)?;
    let sheet_index = wb.sheet_index(sheet).ok_or_else(|| IoError::Format(format!("there is no sheet named \"{sheet}\"")))?;
    if !location.is_valid() {
        return Err(IoError::Format(format!("PivotTable location {} is outside the sheet", location.a1())));
    }
    let rows = report.len();
    let cols = report.iter().map(Vec::len).max().unwrap_or(0);
    if rows as u64 > MAX_ROWS as u64 - location.row as u64 || cols as u64 > MAX_COLS as u64 - location.col as u64 {
        return Err(IoError::Format("the PivotTable report does not fit on the sheet".into()));
    }
    if let Some(data) = model_data
        && data.len() as u64 > MAX_PART
    {
        return Err(IoError::TooLarge("the data model part is larger than 512 MB".into()));
    }
    write_report(wb, sheet_index, location, report)?;

    // A field-list pane re-applies one pivot as its fields change, so update the pivot the workbook
    // already keeps under this name instead of accumulating a second definition and cache.
    let existing = wb.preserved.as_deref().and_then(|p| existing_pivot(p, &spec.name));
    if let Some(ExistingPivot::Stale { table_part }) = &existing {
        drop_stale_pivot(wb, table_part);
    }
    let reused = match existing {
        Some(ExistingPivot::Usable { table_part, cache_part, cache_id }) => Some((table_part, cache_part, cache_id)),
        _ => None,
    };
    let updating = reused.is_some();
    // The definition carries the id the file already uses, so it keeps matching its
    // `<pivotCaches>` entry.
    let mut built = spec.clone();
    if let Some((_, _, cache_id)) = &reused {
        built.cache_id = *cache_id;
    }
    let built = &built;
    let table_xml = build_table_xml(built, location, rows.max(1), cols.max(1));

    let p = preserved(wb);
    let connection_id = ensure_connections(p, built.connection_id);
    let cache_xml = build_cache_xml(built, connection_id)?;
    let (cache_part, table_part) = match reused {
        Some((table_part, cache_part, _)) => (cache_part, table_part),
        None => (
            fresh_part_name(p, "xl/pivotCache", "pivotCacheDefinition", ".xml", built.cache_id),
            fresh_part_name(p, "xl/pivotTables", "pivotTable", ".xml", built.cache_id),
        ),
    };
    match p.parts.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&cache_part)) {
        Some(part) => {
            part.content_type = CT_PIVOT_CACHE_DEF.to_string();
            part.data = Arc::from(cache_xml.into_bytes());
        }
        None => p.parts.push(PreservedPart {
            name: cache_part.clone(),
            content_type: CT_PIVOT_CACHE_DEF.to_string(),
            data: Arc::from(cache_xml.into_bytes()),
            ..Default::default()
        }),
    }
    let cache_rel = PreservedRel { id: "rId1".to_string(), rel_type: rel_uri(REL_PIVOT_CACHE), target: cache_part.clone(), external: false };
    match p.parts.iter_mut().find(|q| q.name.eq_ignore_ascii_case(&table_part)) {
        Some(part) => {
            part.content_type = CT_PIVOT_TABLE.to_string();
            part.data = Arc::from(table_xml.into_bytes());
            // The part's own relationship ids are kept: its bytes never name them.
            match part.rels.iter_mut().find(|r| !r.external && r.rel_type == cache_rel.rel_type) {
                Some(rel) => rel.target = cache_part.clone(),
                None => part.rels.push(cache_rel),
            }
        }
        None => p.parts.push(PreservedPart {
            name: table_part.clone(),
            content_type: CT_PIVOT_TABLE.to_string(),
            data: Arc::from(table_xml.into_bytes()),
            rels: vec![cache_rel],
        }),
    }
    if let Some(data) = model_data {
        match p.parts.iter_mut().find(|q| q.name.eq_ignore_ascii_case(MODEL_DATA_PART)) {
            // A kept part holds the model as read; the caller's bytes carry the session's measures,
            // so they replace it rather than being dropped.
            Some(part) => part.data = Arc::from(data),
            None => p.parts.push(PreservedPart {
                name: MODEL_DATA_PART.to_string(),
                content_type: CT_MODEL_DATA.to_string(),
                data: Arc::from(data),
                ..Default::default()
            }),
        }
    }
    let cache_rid = ensure_rel(p, REL_PIVOT_CACHE, &cache_part);
    if p.parts.iter().any(|q| q.name.eq_ignore_ascii_case("xl/connections.xml")) {
        let _ = ensure_rel(p, REL_CONNECTIONS, "xl/connections.xml");
    }
    if p.parts.iter().any(|q| q.name.eq_ignore_ascii_case("xl/model/item.data")) {
        let _ = ensure_rel(p, REL_MODEL, "xl/model/item.data");
    }
    add_pivot_cache(p, built.cache_id, &cache_rid)?;
    let resolved = resolve_connections(p, built);
    set_data_model_ext(p, &resolved);
    if updating {
        // One PivotTable part is related from one sheet only.
        remove_other_sheet_rels(wb, &table_part, sheet_index);
    }
    attach_sheet_rel(wb, sheet_index, &table_part)
}

// ================================================================ reading

/// Reads the data-model PivotTables of a package into [`ModelPivotSpec`]s.
///
/// Caches whose source is not the workbook's model (`type="external"` with a connection) are
/// skipped with a warning: those are range/OLAP-server pivots, not the model pivots this module
/// writes. Calculated members, value filters and member properties are reported as warnings.
pub fn read_model_pivots(bytes: &[u8]) -> Result<(Vec<ModelPivotPartInfo>, ReadReport), IoError> {
    let mut report = ReadReport::default();
    let mut pkg = Package::open(bytes)?;
    let root_rels = pkg.rels("")?;
    let wb_part = root_rels.iter().find(|r| r.kind == "officeDocument").map(|r| r.target.clone()).unwrap_or_else(|| "xl/workbook.xml".into());
    let Some(wb_xml) = pkg.read_xml(&wb_part)? else {
        return Ok((Vec::new(), report));
    };
    let wb_rels = pkg.rels(&wb_part)?;
    let mut cache_by_id: HashMap<u32, String> = HashMap::new();
    if let Some(caches) = wb_xml.child("pivotCaches") {
        for c in caches.kids("pivotCache") {
            if let (Some(id), Some(rid)) = (c.attr_u32("cacheId"), c.attr("id"))
                && let Some(rel) = wb_rels.iter().find(|r| r.id == rid && !r.external)
            {
                cache_by_id.insert(id, rel.target.clone());
            }
        }
    }
    let (model_tables, relationships, model_declared) = read_model_extension(&mut pkg, &wb_xml, &wb_rels);
    let mut out = Vec::new();
    for sheet in wb_xml.child("sheets").map(|s| s.kids("sheet").collect::<Vec<_>>()).unwrap_or_default() {
        let name = sheet.attr("name").unwrap_or("").to_string();
        let Some(rid) = sheet.attr("id") else { continue };
        let Some(sheet_rel) = wb_rels.iter().find(|r| r.id == rid && !r.external) else { continue };
        let Ok(rels) = pkg.rels(&sheet_rel.target) else { continue };
        for rel in rels.iter().filter(|r| r.kind == REL_PIVOT_TABLE && !r.external) {
            let Some(table_xml) = pkg.read_xml(&rel.target)? else { continue };
            if table_xml.name != "pivotTableDefinition" {
                report.warn(format!("{} is not a PivotTable definition; skipped", rel.target));
                continue;
            }
            let table_rels = pkg.rels(&rel.target).unwrap_or_default();
            let cache_part = table_rels
                .iter()
                .find(|r| r.kind == REL_PIVOT_CACHE)
                .map(|r| r.target.clone())
                .or_else(|| table_xml.attr_u32("cacheId").and_then(|id| cache_by_id.get(&id).cloned()));
            let Some(cache_part) = cache_part else {
                report.warn(format!("PivotTable {} has no cache; skipped", rel.target));
                continue;
            };
            let Some(cache) = read_cache(&mut pkg, &cache_part, &mut report)? else { continue };
            if let Some(info) = read_table(&table_xml, &cache, &model_tables, &relationships, &mut report) {
                out.push(ModelPivotPartInfo { sheet: name.clone(), location: info.0, spec: info.1 });
            }
        }
    }
    if !out.is_empty() && !model_declared {
        report.warn("the workbook has no x15:dataModel declaration; the model PivotTable's tables could not be read");
    }
    Ok((out, report))
}

struct CacheRead {
    connection_id: u32,
    fields: Vec<CacheFieldRead>,
    /// One entry per `cacheHierarchy` element, `None` for the ones a spec cannot name: the
    /// `cacheField/@hierarchy` indices point into this list as written, so holes stay.
    hierarchies: Vec<Option<HierarchyRead>>,
}

struct CacheFieldRead {
    caption: String,
    hierarchy: usize,
    items: Vec<ModelPivotScalar>,
}

struct HierarchyRead {
    caption: String,
    table: String,
    column: Option<String>,
    measure: bool,
    data_type: ModelPivotDataType,
}

fn read_cache(pkg: &mut Package<'_>, part: &str, report: &mut ReadReport) -> Result<Option<CacheRead>, IoError> {
    let Some(x) = pkg.read_xml(part)? else {
        report.warn(format!("missing part {part}"));
        return Ok(None);
    };
    if x.name != "pivotCacheDefinition" {
        report.warn(format!("{part} is not a pivot cache definition; its PivotTable was skipped"));
        return Ok(None);
    }
    let Some(source) = x.child("cacheSource") else {
        report.warn(format!("PivotTable cache {part} has no source; skipped"));
        return Ok(None);
    };
    if source.attr("type") != Some("external") {
        report.warn(format!(
            "{} uses a {} PivotTable cache; read_model_pivots only reads data-model PivotTables",
            part,
            source.attr("type").unwrap_or("unknown")
        ));
        return Ok(None);
    }
    let connection_id = source.attr_u32("connectionId").unwrap_or(0);
    let mut hierarchies = Vec::new();
    if let Some(list) = x.child("cacheHierarchies") {
        for h in list.kids("cacheHierarchy") {
            let unique_name = h.attr("uniqueName").unwrap_or("").to_string();
            let Some((table, column)) = parse_unique_name(&unique_name) else {
                hierarchies.push(None);
                continue;
            };
            let measure = h.flag("measure", false);
            // Excel's per-table implicit count measure and its placeholder are not measures of the
            // model: a spec can only name measures the model database defines.
            if measure && (column.starts_with("__XL_Count ") || column == "__No measures defined") {
                hierarchies.push(None);
                continue;
            }
            let data_type = match h.attr_u32("memberValueDatatype").unwrap_or(130) {
                5 => ModelPivotDataType::Number,
                7 => ModelPivotDataType::Date,
                11 => ModelPivotDataType::Boolean,
                _ => ModelPivotDataType::Text,
            };
            // A measure's hierarchy lives in `[Measures]`; the table it belongs to is `measureGroup`.
            let table = if measure { h.attr("measureGroup").filter(|g| !g.trim().is_empty()).unwrap_or(&table).to_string() } else { table };
            hierarchies.push(Some(HierarchyRead {
                caption: h.attr("caption").unwrap_or(&column).to_string(),
                table,
                column: if measure { None } else { Some(column) },
                measure,
                data_type,
            }));
        }
    }
    let mut fields = Vec::new();
    if let Some(list) = x.child("cacheFields") {
        for f in list.kids("cacheField") {
            let caption = f.attr("caption").unwrap_or("").to_string();
            let hierarchy = f.attr_u32("hierarchy").map(|n| n as usize).unwrap_or(0);
            let mut items = Vec::new();
            if let Some(shared) = f.child("sharedItems") {
                for item in &shared.children {
                    match item.name.as_str() {
                        "m" => items.push(ModelPivotScalar::Blank),
                        "n" => {
                            if let Some(v) = item.attr("v").and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()) {
                                items.push(ModelPivotScalar::Number(v));
                            }
                        }
                        "b" => items.push(ModelPivotScalar::Boolean(matches!(item.attr("v").unwrap_or("").trim(), "1" | "true"))),
                        "s" | "d" => items.push(ModelPivotScalar::Text(item.attr("v").unwrap_or("").to_string())),
                        _ => {}
                    }
                }
            }
            fields.push(CacheFieldRead { caption, hierarchy, items });
        }
    }
    Ok(Some(CacheRead { connection_id, fields, hierarchies }))
}

fn parse_unique_name(name: &str) -> Option<(String, String)> {
    let inner = name.strip_prefix('[')?.strip_suffix(']')?;
    let (table, column) = inner.split_once("].[")?;
    Some((table.to_string(), column.to_string()))
}

/// The member text of a unique name such as `[Sales].[Region].&[East]`.
fn member_from_name(name: &str) -> Option<String> {
    let rest = name.split(".&[").nth(1)?;
    let end = rest.rfind(']')?;
    rest.get(..end).map(str::to_string)
}

fn read_model_extension(
    pkg: &mut Package<'_>,
    wb_xml: &El,
    wb_rels: &[crate::package::Rel],
) -> (Vec<ModelPivotTable>, Vec<ModelPivotRelationship>, bool) {
    let _ = pkg;
    let mut tables: BTreeMap<String, ModelPivotTable> = BTreeMap::new();
    let mut relationships = Vec::new();
    let mut saw_model = false;
    if let Some(exts) = wb_xml.child("extLst") {
        for ext in exts.kids("ext") {
            let Some(model) = ext.child("dataModel") else { continue };
            saw_model = true;
            if let Some(list) = model.child("modelTables") {
                for t in list.kids("modelTable") {
                    let name = t.attr("name").unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    tables.insert(
                        name.clone(),
                        ModelPivotTable { name, connection: t.attr("connection").unwrap_or("").to_string(), columns: Vec::new() },
                    );
                }
            }
            if let Some(list) = model.child("modelRelationships") {
                for r in list.kids("modelRelationship") {
                    relationships.push(ModelPivotRelationship {
                        from_table: r.attr("fromTable").unwrap_or("").to_string(),
                        from_column: r.attr("fromColumn").unwrap_or("").to_string(),
                        to_table: r.attr("toTable").unwrap_or("").to_string(),
                        to_column: r.attr("toColumn").unwrap_or("").to_string(),
                    });
                }
            }
        }
    }
    let _ = wb_rels;
    (tables.into_values().collect(), relationships, saw_model)
}

fn read_table(
    x: &El,
    cache: &CacheRead,
    model_tables: &[ModelPivotTable],
    relationships: &[ModelPivotRelationship],
    report: &mut ReadReport,
) -> Option<(CellRef, ModelPivotSpec)> {
    let Some(loc) = x.child("location").and_then(|l| l.attr("ref")).and_then(|r| RangeRef::parse(&r.replace('$', ""))) else {
        report.warn("a model PivotTable has no valid location; skipped");
        return None;
    };
    let mut spec = ModelPivotSpec {
        name: x.attr("name").unwrap_or("").to_string(),
        cache_id: x.attr_u32("cacheId").unwrap_or(0),
        connection_id: cache.connection_id,
        tables: Vec::new(),
        relationships: relationships.to_vec(),
        measures: Vec::new(),
        row_fields: Vec::new(),
        row_members: Vec::new(),
        column_fields: Vec::new(),
        column_members: Vec::new(),
        filters: Vec::new(),
        values: Vec::new(),
        layout: ModelPivotLayout::Compact,
        row_subtotals: false,
        column_subtotals: false,
        subtotal_top: true,
        grand_totals: true,
        style: x.child("pivotTableStyleInfo").and_then(|s| s.attr("name")).map(str::to_string),
    };
    if spec.name.trim().is_empty() {
        spec.name = format!("PivotTable{}", spec.cache_id);
    }
    if x.child("calculatedItems").is_some() || x.child("calculatedMembers").is_some() {
        report.warn(format!("model PivotTable \"{}\": calculated members are not supported", spec.name));
    }
    if x.path(&["filters", "filter"]).is_some() {
        report.warn(format!("model PivotTable \"{}\": label/value filters are not supported", spec.name));
    }

    // Layout and subtotals from the first field on each axis.
    let pivot_fields: Vec<&El> = x.child("pivotFields").map(|p| p.kids("pivotField").collect()).unwrap_or_default();
    let first_on = |list: &str| -> Option<&El> {
        let index = x.child(list)?.kids("field").find_map(|f| f.attr_i64("x")).filter(|i| *i >= 0)?;
        pivot_fields.get(usize::try_from(index).ok()?).copied()
    };
    let row_first = first_on("rowFields");
    let col_first = first_on("colFields");
    let layout_field = row_first.or(col_first).or(pivot_fields.first().copied());
    let compact = layout_field.map(|f| f.flag("compact", true)).unwrap_or(x.flag("compact", true));
    let outline = layout_field.map(|f| f.flag("outline", true)).unwrap_or(x.flag("outline", false));
    spec.layout = if compact {
        ModelPivotLayout::Compact
    } else if outline {
        ModelPivotLayout::Outline
    } else {
        ModelPivotLayout::Tabular
    };
    spec.row_subtotals = row_first.map(|f| f.flag("defaultSubtotal", true)).unwrap_or(false);
    spec.column_subtotals = col_first.map(|f| f.flag("defaultSubtotal", true)).unwrap_or(false);
    spec.subtotal_top = layout_field.map(|f| f.flag("subtotalTop", true)).unwrap_or(true);
    spec.grand_totals = x.flag("rowGrandTotals", true) || x.flag("colGrandTotals", true);

    // Columns from the cache hierarchies, grouped into tables.
    let mut tables: BTreeMap<String, ModelPivotTable> = BTreeMap::new();
    for table in model_tables {
        tables.insert(table.name.clone(), table.clone());
    }
    for h in cache.hierarchies.iter().flatten() {
        if h.measure {
            spec.measures.push(ModelPivotMeasure { table: h.table.clone(), name: h.caption.clone(), formula: String::new(), number_format: None });
            continue;
        }
        let entry = tables.entry(h.table.clone()).or_insert_with(|| ModelPivotTable {
            name: h.table.clone(),
            connection: String::new(),
            columns: Vec::new(),
        });
        if let Some(column) = &h.column {
            entry.columns.push(ModelPivotColumn { name: column.clone(), caption: h.caption.clone(), data_type: h.data_type });
        }
    }
    spec.tables = tables.into_values().collect();
    if !spec.measures.is_empty() {
        report.warn(format!("model PivotTable \"{}\": DAX formulas live in xl/model/item.data; measure formulas were not read", spec.name));
    }

    // Axes: cache field index → table/column or measure.
    let field_ref = |idx: usize| -> Option<ModelPivotFieldRef> {
        let f = cache.fields.get(idx)?;
        let h = cache.hierarchies.get(f.hierarchy)?.as_ref()?;
        if h.measure {
            return None;
        }
        Some(ModelPivotFieldRef { table: h.table.clone(), column: h.column.clone()? })
    };
    if let Some(rows) = x.child("rowFields") {
        for f in rows.kids("field") {
            if f.attr_i64("x") == Some(-2) {
                report.warn(format!("model PivotTable \"{}\": Σ Values on rows is not supported", spec.name));
                continue;
            }
            if let Some(idx) = f.attr_i64("x").filter(|i| *i >= 0).map(|i| i as usize)
                && let Some(r) = field_ref(idx)
            {
                spec.row_fields.push(r);
                spec.row_members.push(cache.fields.get(idx).map(|f| f.items.clone()).unwrap_or_default());
            }
        }
    }
    if let Some(cols) = x.child("colFields") {
        for f in cols.kids("field") {
            if f.attr_i64("x") == Some(-2) {
                continue;
            }
            if let Some(idx) = f.attr_i64("x").filter(|i| *i >= 0).map(|i| i as usize)
                && let Some(r) = field_ref(idx)
            {
                spec.column_fields.push(r);
                spec.column_members.push(cache.fields.get(idx).map(|f| f.items.clone()).unwrap_or_default());
            }
        }
    }
    if let Some(pages) = x.child("pageFields") {
        for page in pages.kids("pageField") {
            let Some(idx) = page.attr_u32("fld").map(|i| i as usize) else { continue };
            let Some(field) = field_ref(idx) else { continue };
            let field_def = cache.fields.get(idx);
            let field_def_items = field_def.map(|f| f.items.as_slice()).unwrap_or(&[]);
            let multi = pivot_fields.get(idx).is_some_and(|p| p.flag("multipleItemSelectionAllowed", false));
            let mut named = page.attr("cap").unwrap_or("");
            if named.eq_ignore_ascii_case("All") || named == "(All)" {
                named = "";
            }
            // A multi-value filter lists its members in the field's `<items>`; a single value is
            // named by `<pageField item=..>` or, when the cache holds no shared items (Excel leaves
            // them empty for an OLAP filter), by the member inside its `name` attribute.
            let allowed: Vec<ModelPivotScalar> = if multi {
                pivot_fields
                    .get(idx)
                    .and_then(|p| p.child("items"))
                    .map(|items| {
                        items
                            .kids("item")
                            .filter_map(|it| it.attr_u32("x").map(|x| x as usize))
                            .filter_map(|x| field_def_items.get(x).cloned())
                            .collect()
                    })
                    .unwrap_or_default()
            } else if let Some(item) = page.attr_u32("item").map(|i| i as usize) {
                field_def_items.get(item).cloned().into_iter().collect()
            } else if !named.is_empty() {
                let member = member_from_name(page.attr("name").unwrap_or(""));
                match member {
                    Some(text) => vec![ModelPivotScalar::Text(text)],
                    None => field_def_items.to_vec(),
                }
            } else {
                Vec::new()
            };
            if allowed.is_empty() && !named.is_empty() {
                report.warn(format!(
                    "model PivotTable \"{}\": the filter on {}[{}] could not be read from the cache's shared items",
                    spec.name, field.table, field.column
                ));
            }
            spec.filters.push(ModelPivotFilter {
                field,
                include_blank: allowed.iter().any(|v| matches!(v, ModelPivotScalar::Blank)),
                allowed: allowed.into_iter().filter(|v| !matches!(v, ModelPivotScalar::Blank)).collect(),
            });
        }
    }
    if let Some(data) = x.child("dataFields") {
        for f in data.kids("dataField") {
            let Some(idx) = f.attr_u32("fld").map(|i| i as usize) else { continue };
            let Some(cf) = cache.fields.get(idx) else { continue };
            let name = cache.hierarchies.get(cf.hierarchy).and_then(|h| h.as_ref()).map(|h| h.caption.clone()).unwrap_or_else(|| cf.caption.clone());
            if !cache.hierarchies.get(cf.hierarchy).and_then(|h| h.as_ref()).is_some_and(|h| h.measure) {
                report.warn(format!("model PivotTable \"{}\": implicit measures on columns are not supported", spec.name));
                continue;
            }
            spec.values.push(ModelPivotValue { measure: name, caption: cf.caption.clone(), number_format: None });
        }
    }
    Some((loc.start, spec))
}
