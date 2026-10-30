//! Data-model PivotTables: `dataModel.createPivot` turns a model-backed
//! [`gridcraft_dax::pivot_table::PivotTable`] into a worksheet PivotTable whose cache is the
//! workbook data model, instead of a worksheet range.
//!
//! One call does the whole job:
//!
//! * renders the pivot against `Session::dax_model` (`PivotTable::render`), so the cells on the
//!   sheet are the same grid `dataModel.pivot` returns;
//! * converts the model and the pivot to [`gridcraft_xlsx::ModelPivotSpec`] — every table and
//!   column with its data type, the relationships, the measures with their DAX formula and number
//!   format, then the row/column fields, page filters, values, layout and totals;
//! * writes the report cells and builds `xl/pivotTables/pivotTableN.xml` with its
//!   `pivotCacheDefinition` through [`gridcraft_xlsx::attach_model_pivot`].
//!
//! # Where the pivot parts live
//!
//! [`gridcraft_xlsx::attach_model_pivot`] adds the pivot definitions to the workbook's kept
//! (pass-through) content — the cache definition, the PivotTable definition, `xl/connections.xml`,
//! the workbook's `<pivotCaches>` and the sheet's relationship — so the next save writes them and
//! the reply's `cached` is `true`. Excel can only refresh the cache when the package also carries
//! the model database (`xl/model/item.data`): the command passes it when the workbook already has
//! it, and a workbook without one opens with the report cells but a cache Excel cannot refresh.
//!
//! Filters are written as page fields: two rows per filter (the field caption and the selected
//! item) above the report body, which is what `pivotTableDefinition`'s `rowPageCount` describes.
//! The report grid itself starts below them.

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value as CoreValue};
use gridcraft_dax::model::ModelColumn;
use gridcraft_dax::pivot_table::{PivotFilter, PivotLayout, PivotTable};
use gridcraft_dax::{DataModel, Scalar, pivot_fields};
use gridcraft_model::{Cell, NumFmt, StyleId, Workbook};
use gridcraft_xlsx::{
    ModelPivotColumn, ModelPivotDataType, ModelPivotFieldRef, ModelPivotFilter, ModelPivotLayout, ModelPivotMeasure, ModelPivotRelationship,
    ModelPivotScalar, ModelPivotSpec, ModelPivotTable, ModelPivotValue,
};
use serde_json::{Value as Json, json};

use super::*;

/// The command that creates the PivotTable; also the id error messages name.
const CMD: &str = "dataModel.createPivot";

/// Most fields accepted on one axis, and most page filters.
const MAX_AXIS_FIELDS: usize = 64;
/// Most page filters in one PivotTable.
const MAX_FILTERS: usize = 32;
/// Most cells the written report may occupy (the render budget is separate).
const MAX_REPORT_CELLS: usize = 1_000_000;
/// The sheet part that carries a workbook data model, which a refreshable cache needs.
const MODEL_DATA_PART: &str = "xl/model/item.data";
/// Rows a page field takes above the report body: its caption and its selected item.
const PAGE_ROWS: usize = 2;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "dataModel.createPivot",
            "Create PivotTable",
            ["Data", "Data Model"],
            None,
            "{measure, row_fields?: [\"Sales[Region]\"], column_fields?: [..], filters?: [{field, allowed: [value, ..], include_blank?}], name?, sheet? (default: the active sheet), at? (default: A3), layout?: compact|outline|tabular, row_subtotals?, column_subtotals?, grand_totals?, replace?} writes a PivotTable backed by the data model: its report cells plus the pivot cache and PivotTable definitions → {name, sheet, at, rows, columns, values, cached}; `cached` says the workbook now carries the pivot parts, so a save keeps them (Excel still needs the model database part to refresh)",
            has_model,
            create_pivot
        ),
        cmd!(
            "insert.modelPivotTable",
            "PivotTable from Data Model",
            ["Insert", "Tables"],
            None,
            "{measure, row_fields?, column_fields?, filters?, name?, sheet?, at?, layout?, row_subtotals?, column_subtotals?, grand_totals?, replace?} (same as dataModel.createPivot)",
            has_model,
            create_pivot
        ),
    ]
}

/// Both commands need a loaded model, exactly as the Power Pivot window does; the message is
/// `cmd/dax.rs`'s.
fn has_model(s: &Session) -> std::result::Result<(), String> {
    if s.dax_model.is_some() { Ok(()) } else { Err("no data model loaded".into()) }
}

fn other(msg: impl Into<String>) -> EngineError {
    EngineError::Other(msg.into())
}

// ---------------------------------------------------------------- command

fn create_pivot(s: &mut Session, p: &Json) -> Result<Json> {
    Ok(create(s, p, false)?.0)
}

/// [`create_pivot`] with the saved package the pivot parts live in.
///
/// The reply is the command's `{name, sheet, at, rows, columns, values, cached}`. The parts
/// themselves are attached to the workbook (`gridcraft_xlsx::attach_model_pivot`), so the next
/// save writes them; the package this returns is that save's bytes, for a caller that wants them
/// without saving to a file. The command drops them, which is why it asks for no package: building
/// one is a full zip of the workbook.
pub fn write_pivot(s: &mut Session, p: &Json) -> Result<(Json, Option<Vec<u8>>)> {
    create(s, p, true)
}

fn create(s: &mut Session, p: &Json, package: bool) -> Result<(Json, Option<Vec<u8>>)> {
    let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
    let pivot = pivot_param(model, p)?;
    let table = pivot.render(model).map_err(|error| other(error.to_string()))?;
    let report = report_grid(&table)?;
    let values = spec_values(model, &pivot)?;
    let mut spec = spec_from_model(model, &pivot, &values)?;
    check_axes(&spec)?;
    let formats = value_formats(&values);
    let model_data = model_data(s);
    let replace = optional_bool(p, "replace")?.unwrap_or(false);
    let requested_name = optional_str(p, "name")?;
    let (sheet, at) = destination(s, p)?;

    let (reply, package) = edit(s, |cx| {
        let sheet_index = cx.wb.sheet_index(&sheet).ok_or_else(|| bad(CMD, format!("no sheet named `{sheet}`")))?;
        let name = pivot_name(&cx.wb, requested_name)?;
        let body = CellRef::new(at.row.saturating_add((spec.filters.len() * PAGE_ROWS) as u32), at.col);
        let end = CellRef::new(
            body.row.saturating_add((report.len().saturating_sub(1)) as u32),
            body.col.saturating_add((report.first().map_or(0, Vec::len).saturating_sub(1)) as u32),
        );
        let area = RangeRef::new(at, end);
        if area.end.row >= gridcraft_core::MAX_ROWS || area.end.col >= gridcraft_core::MAX_COLS {
            return Err(other("The PivotTable report will not fit on the sheet. Choose a different location."));
        }
        if !replace {
            let existing = cx.wb.sheet(sheet_index).ok_or_else(|| bad(CMD, format!("no sheet named `{sheet}`")))?;
            if let Some((cell, _)) = existing.cells.iter_range(area).find(|(_, cell)| !cell.value.is_empty() || cell.formula.is_some()) {
                return Err(other(format!(
                    "There's already data in {}!{}. Do you want to replace it? (Run again with replace: true to overwrite.)",
                    crate::pivot::quote_sheet(&sheet),
                    cell.a1()
                )));
            }
        }
        let pages = page_rows(&spec);
        let ids = intern_formats(&mut cx.wb, &formats);
        {
            let sh = cx.sheet_mut(sheet_index)?;
            for (i, row) in pages.iter().enumerate() {
                for (j, text) in row.iter().enumerate() {
                    let cell = CellRef::new(at.row.saturating_add(i as u32), at.col.saturating_add(j as u32));
                    sh.set_cell(cell, Cell { value: CoreValue::text(text.as_str()), formula: None, style: StyleId::default() });
                }
            }
            let labels = label_column_count(spec.layout, spec.row_fields.len());
            apply_formats(sh, body, report.len(), report.first().map_or(0, Vec::len), labels, &ids);
        }
        spec.name = name.clone();
        spec.cache_id = next_cache_id(&cx.wb);
        gridcraft_xlsx::attach_model_pivot(&mut cx.wb, &sheet, body, &spec, &report, model_data.as_deref())
            .map_err(|error| other(format!("the PivotTable could not be written: {error}")))?;
        let bytes = if package {
            Some(gridcraft_xlsx::write_xlsx(&cx.wb).map_err(|error| other(format!("the workbook could not be written: {error}")))?)
        } else {
            None
        };
        cx.wb.active_sheet = sheet_index.into();
        *cx.sel = crate::Selection::at(at);
        cx.structural = true;
        let reply = json!({
            "name": name,
            "sheet": sheet,
            "at": at.a1(),
            "rows": report.len(),
            "columns": report.first().map_or(0, Vec::len),
            "values": values.iter().map(|value| value.caption.clone()).collect::<Vec<_>>(),
            // The workbook carries the pivot parts now, so a save keeps them. Excel still cannot
            // refresh without the model database (`xl/model/item.data`), which the report says
            // nothing about.
            "cached": true,
        });
        Ok((reply, bytes))
    })?;
    Ok((reply, package))
}

// ---------------------------------------------------------------- parameters

fn wrong_type(key: &str, what: &str) -> EngineError {
    bad(CMD, format!("`{key}` must be {what}"))
}

/// A required, non-blank string parameter, trimmed.
fn required_str<'a>(p: &'a Json, key: &str) -> Result<&'a str> {
    match p.get(key) {
        None | Some(Json::Null) => Err(bad(CMD, format!("`{key}` is required"))),
        Some(Json::String(s)) => match s.trim() {
            "" => Err(bad(CMD, format!("`{key}` is required"))),
            s => Ok(s),
        },
        Some(_) => Err(wrong_type(key, "a string")),
    }
}

/// An optional string parameter: absent and JSON `null` both mean "none"; anything else is an
/// error, so a mistyped parameter never silently takes the default.
fn optional_str<'a>(p: &'a Json, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(s)) => Ok(Some(s.trim())),
        Some(_) => Err(wrong_type(key, "a string")),
    }
}

/// An optional boolean parameter, strict about its type.
fn optional_bool(p: &Json, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(wrong_type(key, "true or false")),
    }
}

/// A field list: an array of `Table[Column]` references (a single string is accepted too).
fn field_list(p: &Json, key: &str) -> Result<Vec<String>> {
    match p.get(key) {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::String(s)) => Ok(vec![s.clone()]),
        Some(Json::Array(items)) => {
            if items.len() > MAX_AXIS_FIELDS {
                return Err(bad(CMD, format!("`{key}` accepts at most {MAX_AXIS_FIELDS} fields")));
            }
            items.iter().map(|item| item.as_str().map(str::to_string).ok_or_else(|| wrong_type(key, "a list of field names"))).collect()
        }
        Some(_) => Err(wrong_type(key, "a list of field names")),
    }
}

/// A filter value as a DAX scalar; JSON `null` is BLANK.
fn filter_scalar(value: &Json) -> Result<Scalar> {
    Ok(match value {
        Json::Null => Scalar::Blank,
        Json::Bool(b) => Scalar::Boolean(*b),
        Json::String(s) => Scalar::String(s.clone()),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Scalar::Integer(i),
            None => Scalar::Decimal(n.as_f64().ok_or_else(|| wrong_type("filters", "a list of values"))?),
        },
        _ => return Err(wrong_type("filters", "a list of values")),
    })
}

/// The `filters` parameter: `[{field, allowed: [..], include_blank?}]`. An empty `allowed` list
/// keeps nothing, as in `gridcraft_dax::PivotFilter`.
fn filters_param(p: &Json) -> Result<Vec<PivotFilter>> {
    let items = match p.get("filters") {
        None | Some(Json::Null) => return Ok(Vec::new()),
        Some(Json::Array(items)) => items,
        Some(_) => return Err(wrong_type("filters", "a list of {field, allowed} objects")),
    };
    if items.len() > MAX_FILTERS {
        return Err(bad(CMD, format!("a PivotTable can have at most {MAX_FILTERS} filters")));
    }
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let field = item
            .get("field")
            .and_then(Json::as_str)
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .ok_or_else(|| bad(CMD, "a filter needs `field`"))?;
        let mut allowed = match item.get("allowed").or_else(|| item.get("values")) {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(values)) => {
                if values.len() > MAX_AXIS_FIELDS {
                    return Err(bad(CMD, format!("a filter accepts at most {MAX_AXIS_FIELDS} values")));
                }
                values.iter().map(filter_scalar).collect::<Result<Vec<_>>>()?
            }
            Some(_) => return Err(wrong_type("filters", "a list of {field, allowed} objects")),
        };
        let include_blank = match item.get("include_blank") {
            None | Some(Json::Null) => false,
            Some(Json::Bool(b)) => *b,
            Some(_) => return Err(wrong_type("include_blank", "true or false")),
        };
        if include_blank && !allowed.iter().any(|value| matches!(value, Scalar::Blank)) {
            allowed.push(Scalar::Blank);
        }
        out.push(PivotFilter { field: field.to_string(), allowed });
    }
    Ok(out)
}

fn layout_param(p: &Json) -> Result<PivotLayout> {
    Ok(match optional_str(p, "layout")? {
        None | Some("") => PivotLayout::Compact,
        Some(name) => match name.to_ascii_lowercase().as_str() {
            "compact" => PivotLayout::Compact,
            "outline" => PivotLayout::Outline,
            "tabular" => PivotLayout::Tabular,
            _ => return Err(bad(CMD, format!("unknown layout `{name}` (compact, outline or tabular)"))),
        },
    })
}

/// The pivot the parameters describe, with every name resolved against the model so an unknown
/// measure, table or field is an error before anything is rendered.
fn pivot_param(model: &DataModel, p: &Json) -> Result<PivotTable> {
    let measure = required_str(p, "measure")?;
    let measure = model
        .measure_index(measure)
        .and_then(|index| model.measures.get(index))
        .map(|measure| measure.name.clone())
        .ok_or_else(|| bad(CMD, format!("no measure named `{measure}` in the data model")))?;
    let row_fields = field_list(p, "row_fields")?;
    let column_fields = field_list(p, "column_fields")?;
    // Resolving now reports an unknown field as a parameter error, and proves every field belongs
    // to a model table before the renderer and the cache writer see it.
    for field in row_fields.iter().chain(&column_fields) {
        pivot_fields(model, std::slice::from_ref(field)).map_err(|error| bad(CMD, error.to_string()))?;
    }
    let filters = filters_param(p)?;
    let mut pivot = PivotTable::new(optional_str(p, "name")?.unwrap_or_default(), measure);
    pivot.row_fields = row_fields;
    pivot.column_fields = column_fields;
    pivot.filters = filters;
    pivot.layout = layout_param(p)?;
    pivot.row_subtotals = optional_bool(p, "row_subtotals")?.unwrap_or(false);
    pivot.column_subtotals = optional_bool(p, "column_subtotals")?.unwrap_or(false);
    pivot.grand_totals = optional_bool(p, "grand_totals")?.unwrap_or(false);
    Ok(pivot)
}

/// The destination sheet (default: the active one) and the pivot's top-left cell (default: A3).
fn destination(s: &Session, p: &Json) -> Result<(String, CellRef)> {
    let d = s.doc()?;
    let sheet = match optional_str(p, "sheet")? {
        Some(name) if !name.is_empty() => {
            let index = d.wb.sheet_index(name).ok_or_else(|| bad(CMD, format!("no sheet named `{name}`")))?;
            d.wb.sheet(index).map(|sheet| sheet.name.clone()).unwrap_or_default()
        }
        _ => d.wb.active().map(|sheet| sheet.name.clone()).ok_or(EngineError::NoDocument)?,
    };
    let at = match optional_str(p, "at")? {
        None | Some("") => CellRef::new(2, 0),
        Some(text) => {
            let (named, body) = super::split_sheet(text);
            if let Some(named) = named
                && !named.eq_ignore_ascii_case(&sheet)
            {
                return Err(bad(CMD, format!("`{text}` names a sheet other than `{sheet}`")));
            }
            RangeRef::parse(body).map(|range| range.start).ok_or_else(|| bad(CMD, format!("not a cell: `{text}`")))?
        }
    };
    Ok((sheet, at))
}

/// The workbook's `xl/model/item.data`, when a read kept it: the cache can then be refreshed in
/// Excel.
fn model_data(s: &Session) -> Option<Arc<[u8]>> {
    let d = s.doc().ok()?;
    let preserved = d.wb.preserved.as_ref()?;
    preserved.parts.iter().find(|part| part.name.eq_ignore_ascii_case(MODEL_DATA_PART)).map(|part| Arc::clone(&part.data))
}

/// The PivotTable's name: the `name` parameter (checked against every PivotTable the workbook
/// already has) or the first free `ModelPivotN`.
fn pivot_name(wb: &Workbook, requested: Option<&str>) -> Result<String> {
    let kept = kept_pivot_names(wb);
    let taken = |name: &str| {
        wb.sheets.iter().any(|sheet| sheet.pivots.iter().any(|pivot| pivot.name.eq_ignore_ascii_case(name)))
            || kept.iter().any(|kept| kept.eq_ignore_ascii_case(name))
    };
    match requested.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => {
            if taken(name) {
                return Err(other("A PivotTable with that name already exists."));
            }
            Ok(name.to_string())
        }
        None => Ok((1..).map(|n| format!("ModelPivot{n}")).find(|name| !taken(name)).unwrap_or_default()),
    }
}

/// The names of the PivotTables the workbook keeps in its preserved parts — the model PivotTables
/// this command made before, whose names the model does not hold.
fn kept_pivot_names(wb: &Workbook) -> Vec<String> {
    let Some(preserved) = &wb.preserved else { return Vec::new() };
    preserved
        .parts
        .iter()
        .filter(|part| part_number(&part.name, "xl/pivottables/pivottable", ".xml").is_some())
        .filter_map(|part| std::str::from_utf8(&part.data).ok())
        .filter_map(|xml| {
            let rest = xml.get(xml.find("name=\"")?.saturating_add("name=\"".len())..)?;
            rest.get(..rest.find('"')?).map(str::to_string)
        })
        .collect()
}

/// The `cacheId` of the new cache: past every PivotTable id and every cache the workbook already
/// keeps, so a second model PivotTable gets a cache of its own.
fn next_cache_id(wb: &Workbook) -> u32 {
    let mut max = wb.sheets.iter().flat_map(|sheet| sheet.pivots.iter().map(|pivot| pivot.id)).max().unwrap_or(0);
    if let Some(preserved) = &wb.preserved {
        for part in &preserved.parts {
            if let Some(id) = part_number(&part.name, "xl/pivotcache/pivotcachedefinition", ".xml") {
                max = max.max(id);
            }
        }
        for element in preserved.elements.iter().filter(|element| element.slot == "pivotCaches") {
            for id in numbers_after(&element.xml, "cacheId=\"") {
                max = max.max(id);
            }
        }
    }
    wb.next_object_id().max(max.saturating_add(1))
}

/// The `N` of `<dir>/<stem>N<ext>`, matched case-insensitively.
fn part_number(name: &str, prefix: &str, suffix: &str) -> Option<u32> {
    name.to_ascii_lowercase().strip_prefix(prefix)?.strip_suffix(suffix)?.parse().ok()
}

/// Every number that follows `marker` in `xml` (`cacheId="` gives the declared cache ids).
fn numbers_after(xml: &str, marker: &str) -> Vec<u32> {
    xml.match_indices(marker)
        .filter_map(|(at, _)| {
            let rest = xml.get(at.saturating_add(marker.len())..)?;
            rest.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok()
        })
        .collect()
}

// ---------------------------------------------------------------- report

/// The rendered pivot as worksheet values: the header row, then one row per rendered row.
fn report_grid(table: &gridcraft_dax::TableValue) -> Result<Vec<Vec<CoreValue>>> {
    let width = table.columns.len();
    let cells = table.rows.len().saturating_mul(width).saturating_add(width);
    if cells > MAX_REPORT_CELLS {
        return Err(other(format!("The PivotTable report is too large for the sheet: {cells} cells (the limit is {MAX_REPORT_CELLS}).")));
    }
    let mut grid = Vec::with_capacity(table.rows.len() + 1);
    grid.push(table.columns.iter().map(|column| CoreValue::text(column.name.as_str())).collect());
    for row in &table.rows {
        let mut cells = Vec::with_capacity(width);
        for column in 0..width {
            cells.push(row.get(column).map(scalar_value).unwrap_or(CoreValue::Empty));
        }
        grid.push(cells);
    }
    Ok(grid)
}

/// A DAX scalar as a worksheet value: numbers and dates as numbers, text as text, BLANK as an
/// empty cell.
fn scalar_value(value: &Scalar) -> CoreValue {
    match value {
        Scalar::Blank => CoreValue::Empty,
        Scalar::Boolean(value) => CoreValue::Bool(*value),
        Scalar::Integer(value) => CoreValue::Number(*value as f64),
        Scalar::Currency(value) => CoreValue::Number(*value as f64 / 10_000.0),
        Scalar::Decimal(value) => CoreValue::Number(*value),
        Scalar::DateTime(value) => CoreValue::Number(*value),
        Scalar::String(value) => CoreValue::text(value.as_str()),
    }
}

/// The page-field rows above the report body: one caption row and one selected-item row per
/// filter, as Excel lays them out.
fn page_rows(spec: &ModelPivotSpec) -> Vec<Vec<String>> {
    let mut rows = Vec::with_capacity(spec.filters.len() * PAGE_ROWS);
    for filter in &spec.filters {
        rows.push(vec![filter.field.column.clone()]);
        rows.push(vec![page_item(filter)]);
    }
    rows
}

/// The item a page field shows: its only allowed value, or `(All)` / `(Multiple Items)`.
fn page_item(filter: &ModelPivotFilter) -> String {
    match filter.allowed.as_slice() {
        [value] => value.label(),
        [] => "(All)".to_string(),
        _ => "(Multiple Items)".to_string(),
    }
}

/// One number format per value field, in values order; `None` where the measure has no format.
fn value_formats(values: &[ModelPivotValue]) -> Vec<Option<String>> {
    values.iter().map(|value| value.number_format.clone().filter(|format| !format.trim().is_empty())).collect()
}

/// Interns one style per value field that has a format; the workbook's style table is shared, so
/// this runs before the sheet is touched.
fn intern_formats(wb: &mut Workbook, formats: &[Option<String>]) -> Vec<Option<StyleId>> {
    let base = wb.styles.get(StyleId::default()).clone();
    formats
        .iter()
        .map(|format| {
            format.as_ref().map(|format| {
                let mut style = base.clone();
                style.num_fmt = NumFmt::new(format);
                wb.styles.intern(style)
            })
        })
        .collect()
}

/// The number of label columns the layout writes before the value columns.
fn label_column_count(layout: ModelPivotLayout, row_fields: usize) -> usize {
    match layout {
        ModelPivotLayout::Compact => 1,
        ModelPivotLayout::Outline | ModelPivotLayout::Tabular => row_fields,
    }
}

/// Gives the value columns of the report their measure's number format, so the cells read the way
/// Excel's would. The report repeats the value fields once per column group, so the formats cycle;
/// the header row and the label columns keep the sheet's style.
fn apply_formats(sheet: &mut gridcraft_model::Sheet, body: CellRef, rows: usize, width: usize, label_columns: usize, formats: &[Option<StyleId>]) {
    if formats.is_empty() || width <= label_columns {
        return;
    }
    for row in 1..rows {
        for col in label_columns..width {
            let Some(id) = formats.get((col - label_columns) % formats.len()).copied().flatten() else { continue };
            sheet.set_style(CellRef::new(body.row.saturating_add(row as u32), body.col.saturating_add(col as u32)), id);
        }
    }
}

// ---------------------------------------------------------------- conversion

/// The data type a model column declares, as the cache metadata describes it.
fn data_type(name: Option<&str>) -> ModelPivotDataType {
    match name.map(str::trim) {
        None | Some("") | Some("String") | Some("Text") => ModelPivotDataType::Text,
        Some("Int64" | "Double" | "Decimal" | "Currency") => ModelPivotDataType::Number,
        Some("DateTime" | "Date") => ModelPivotDataType::Date,
        Some("Boolean") => ModelPivotDataType::Boolean,
        Some(_) => ModelPivotDataType::Unknown,
    }
}

/// A model column as the cache declares it; the caption is the name.
fn spec_column(column: &ModelColumn) -> ModelPivotColumn {
    ModelPivotColumn { name: column.name.clone(), caption: column.name.clone(), data_type: data_type(column.data_type.as_deref()) }
}

/// A `Table[Column]` reference resolved to the model's own spelling.
fn field_ref(model: &DataModel, field: &str) -> Result<ModelPivotFieldRef> {
    let resolved = pivot_fields(model, std::slice::from_ref(&field.to_string())).map_err(|error| bad(CMD, error.to_string()))?;
    let first = resolved.first().ok_or_else(|| bad(CMD, format!("`{field}` is not a model column")))?;
    let table = model.tables.get(first.table).ok_or_else(|| bad(CMD, format!("no table named `{field}`")))?;
    let column = table.columns.get(first.column).ok_or_else(|| bad(CMD, format!("no column named `{field}`")))?;
    Ok(ModelPivotFieldRef { table: table.name.clone(), column: column.name.clone() })
}

/// The values area: the pivot's measures with their captions and formats.
fn spec_values(model: &DataModel, pivot: &PivotTable) -> Result<Vec<ModelPivotValue>> {
    let mut out = Vec::new();
    for measure in pivot.value_measures() {
        let index = model.measure_index(&measure).ok_or_else(|| bad(CMD, format!("no measure named `{measure}` in the data model")))?;
        let measure = model.measures.get(index).ok_or_else(|| bad(CMD, format!("no measure named `{measure}` in the data model")))?;
        out.push(ModelPivotValue {
            measure: measure.name.clone(),
            caption: measure.name.clone(),
            number_format: measure.format.clone().filter(|format| !format.trim().is_empty()),
        });
    }
    Ok(out)
}

/// A field cannot be on both axes: the cache writer rejects it, and Excel has no such PivotTable.
fn check_axes(spec: &ModelPivotSpec) -> Result<()> {
    for field in &spec.row_fields {
        if spec.column_fields.contains(field) {
            return Err(bad(CMD, format!("`{}[{}]` can't be on both the row and column axes", field.table, field.column)));
        }
    }
    Ok(())
}

/// Most members one axis field may contribute to the cache's shared items.
const MAX_AXIS_MEMBERS: usize = 10_000;

/// The distinct values of one axis field, in the renderer's ascending group order (BLANK first).
///
/// A field with more than [`MAX_AXIS_MEMBERS`] values contributes no member list: the writer then
/// omits its `<items>` rather than writing an unbounded cache, and Excel rebuilds them on refresh.
fn axis_members(model: &DataModel, fields: &[ModelPivotFieldRef]) -> Vec<Vec<ModelPivotScalar>> {
    let mut out = Vec::with_capacity(fields.len());
    for field in fields {
        let values = model
            .table_index(&field.table)
            .and_then(|table| model.column_index(table, &field.column).map(|column| (table, column)))
            .and_then(|(table, column)| model.tables.get(table).map(|table| (table, column)))
            .map(|(table, column)| {
                let mut seen: BTreeMap<gridcraft_dax::value::ScalarKey, Scalar> = BTreeMap::new();
                for row in &table.rows {
                    let value = row.get(column).cloned().unwrap_or(Scalar::Blank);
                    seen.entry(gridcraft_dax::value::ScalarKey::from(&value)).or_insert(value);
                }
                seen.into_values().collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if values.len() > MAX_AXIS_MEMBERS {
            out.push(Vec::new());
        } else {
            out.push(values.iter().map(model_scalar).collect());
        }
    }
    out
}

/// The whole model plus the pivot's fields, filters, values, layout and totals, as the cache and
/// PivotTable writers read them.
fn spec_from_model(model: &DataModel, pivot: &PivotTable, values: &[ModelPivotValue]) -> Result<ModelPivotSpec> {
    let tables = model
        .tables
        .iter()
        .map(|table| ModelPivotTable {
            name: table.name.clone(),
            // The engine has no worksheet connection per model table: the model comes from JSON,
            // not from a sheet table, so the declaration carries no connection name.
            connection: String::new(),
            columns: table.columns.iter().map(spec_column).collect(),
        })
        .collect();
    let relationships = model
        .relationships
        .iter()
        .map(|relationship| ModelPivotRelationship {
            from_table: relationship.from_table.clone(),
            from_column: relationship.from_column.clone(),
            to_table: relationship.to_table.clone(),
            to_column: relationship.to_column.clone(),
        })
        .collect();
    let measures = model
        .measures
        .iter()
        .map(|measure| ModelPivotMeasure {
            table: measure.table.clone().unwrap_or_default(),
            name: measure.name.clone(),
            formula: measure.formula.trim().trim_start_matches('=').to_string(),
            number_format: measure.format.clone().filter(|format| !format.trim().is_empty()),
        })
        .collect();
    let row_fields = pivot.row_fields.iter().map(|field| field_ref(model, field)).collect::<Result<Vec<_>>>()?;
    let column_fields = pivot.column_fields.iter().map(|field| field_ref(model, field)).collect::<Result<Vec<_>>>()?;
    let filters = pivot
        .filters
        .iter()
        .map(|filter| {
            let field = field_ref(model, &filter.field)?;
            let allowed = filter.allowed.iter().map(model_scalar).collect();
            let include_blank = filter.allowed.iter().any(|value| matches!(value, Scalar::Blank));
            Ok(ModelPivotFilter { field, allowed, include_blank })
        })
        .collect::<Result<Vec<_>>>()?;
    let row_members = axis_members(model, &row_fields);
    let column_members = axis_members(model, &column_fields);
    Ok(ModelPivotSpec {
        name: pivot.name.clone(),
        // Filled in when the workbook is at hand.
        cache_id: 1,
        connection_id: 1,
        tables,
        relationships,
        measures,
        row_fields,
        row_members,
        column_fields,
        column_members,
        filters,
        values: values.to_vec(),
        layout: match pivot.layout {
            PivotLayout::Compact => ModelPivotLayout::Compact,
            PivotLayout::Outline => ModelPivotLayout::Outline,
            PivotLayout::Tabular => ModelPivotLayout::Tabular,
        },
        row_subtotals: pivot.row_subtotals,
        column_subtotals: pivot.column_subtotals,
        // The renderer writes a group's subtotal after its leaves.
        subtotal_top: false,
        grand_totals: pivot.grand_totals,
        style: None,
    })
}

/// A DAX scalar as a cache shared item: currency un-scales to its value, a date stays its serial.
fn model_scalar(value: &Scalar) -> ModelPivotScalar {
    match value {
        Scalar::Blank => ModelPivotScalar::Blank,
        Scalar::Boolean(value) => ModelPivotScalar::Boolean(*value),
        Scalar::Integer(value) => ModelPivotScalar::Number(*value as f64),
        Scalar::Currency(value) => ModelPivotScalar::Number(*value as f64 / 10_000.0),
        Scalar::Decimal(value) => ModelPivotScalar::Number(*value),
        Scalar::DateTime(value) => ModelPivotScalar::Number(*value),
        Scalar::String(value) => ModelPivotScalar::Text(value.clone()),
    }
}
