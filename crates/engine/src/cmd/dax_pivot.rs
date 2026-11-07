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
//!
//! # Columns in the values area
//!
//! A `values` entry is a measure name, or `{column, aggregate}` — a model column summarized the
//! way Excel's implicit measures are. The command builds that implicit measure (`Sum of Amount`
//! over `SUM(Sales[Amount])`), appends it to the model for the duration of the call only, renders
//! and writes the pivot, then takes it out again: the Power Pivot window keeps exactly the
//! measures the user made. The measure is not lost from the workbook, because the pivot cache
//! needs it declared: it goes into the cache's measure list and into `xl/model/item.data`'s MDX
//! script, as any other measure does.
//!
//! # The field list
//!
//! `dataModel.pivotFields` is the read side of the same data: the model's tables, columns, measures
//! and relationships, every model PivotTable the workbook carries, and the settings of the one the
//! pane is showing. The PivotTables are not kept in the session — they live in the workbook's kept
//! pivot parts — and the command reads those parts in place
//! ([`gridcraft_xlsx::read_kept_model_pivots`]), so a field-list refresh never serializes the
//! workbook.

use std::collections::BTreeMap;
use std::sync::Arc;

use gridcraft_core::{CellRef, RangeRef, Value as CoreValue};
use gridcraft_dax::model::ModelColumn;
use gridcraft_dax::pivot_table::{PivotFilter, PivotLayout, PivotTable};
use gridcraft_dax::{DataModel, Measure, Scalar, parse_expression, pivot_fields};
use gridcraft_model::{Cell, NumFmt, StyleId, Workbook};
use gridcraft_xlsx::{
    ModelPivotColumn, ModelPivotDataType, ModelPivotFieldRef, ModelPivotFilter, ModelPivotLayout, ModelPivotMeasure, ModelPivotPartInfo,
    ModelPivotRelationship, ModelPivotScalar, ModelPivotSpec, ModelPivotTable, ModelPivotValue,
};
use serde_json::{Value as Json, json};

use super::{CommandSpec, EngineError, Result, Session, bad, cmd, edit};

/// The command that creates the PivotTable; also the id error messages name.
const CMD: &str = "dataModel.createPivot";
/// The field list query, and the id its parameter errors name.
const CMD_FIELDS: &str = "dataModel.pivotFields";

/// Most fields accepted on one axis, and most page filters.
const MAX_AXIS_FIELDS: usize = 64;
/// Most measures accepted in one values area.
const MAX_VALUE_FIELDS: usize = 64;
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
            query
            "dataModel.pivotFields",
            "PivotTable Fields",
            ["Data", "Data Model"],
            None,
            "{name?} → {pivots: [{name, sheet, at}], model: {tables: [{name, columns: [{name, type}]}], measures: [{name, table, format}], relationships: [{from_table, from_column, to_table, to_column}]}, selected: {name, sheet, at, row_fields, column_fields, filters: [{field, allowed}], values, layout, row_subtotals, column_subtotals, grand_totals} | null} lists the data model's fields and the model PivotTables the workbook carries; `selected` is the named PivotTable's settings (null when `name` is absent or names no model PivotTable); `at` is the report body's top-left cell, below any page fields",
            has_model,
            field_list_cmd
        ),
        cmd!(
            "dataModel.createPivot",
            "Create PivotTable",
            ["Data", "Data Model"],
            None,
            "{measure?, values?: [measure | {column: \"Sales[Amount]\", aggregate?: Sum|Average|Count|Min|Max}, ..], row_fields?: [\"Sales[Region]\"], column_fields?: [..], filters?: [{field, allowed?: [value, ..], include_blank?} (an absent `allowed` is the page field's (All))], name?, sheet? (default: the active sheet), at? (default: A3), layout?: compact|outline|tabular, row_subtotals?, column_subtotals?, grand_totals?, replace?} writes a PivotTable backed by the data model: `measure` groups the rows (optional when `values` names a field; the first value then supplies its table), `values` (default `[measure]`) fills the values area with one measure column each — a measure name, or a column with the aggregation that summarizes it, which the command evaluates as Excel's implicit measure (`Sum of Amount`) without adding a measure to the model — and the reply's report cells plus the pivot cache and PivotTable definitions carry it; `replace` overwrites the report area — clearing the cells the previous report of `name` occupied — and re-applies that kept PivotTable → {name, sheet, at, rows, columns, values, cached}; `cached` says the workbook now carries the pivot parts, so a save keeps them (Excel still needs the model database part to refresh)",
            has_model,
            create_pivot
        ),
        cmd!(
            "insert.modelPivotTable",
            "PivotTable from Data Model",
            ["Insert", "Tables"],
            None,
            "{measure, values?, row_fields?, column_fields?, filters?, name?, sheet?, at?, layout?, row_subtotals?, column_subtotals?, grand_totals?, replace?} (same as dataModel.createPivot)",
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

/// [`write_pivot`]'s and `create_pivot`'s body, as [the command's own
/// doc](self) describes it.
///
/// The values area may summarize columns the model never turned into measures. Those implicit
/// measures are appended to the model for the duration of the call — the renderer, the cache spec
/// and `xl/model/item.data` all read them there — and taken out before the reply, so a pivot over
/// a column leaves the Power Pivot window exactly as the user left it.
fn create(s: &mut Session, p: &Json, package: bool) -> Result<(Json, Option<Vec<u8>>)> {
    let area = {
        let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
        // The measures earlier pivots declared live only in their kept cache definitions, so their
        // names are read from there: a second pivot over a different table must not pick the same
        // implicit name, or it would rewrite the first pivot's measure in `xl/model/item.data`.
        // Only `{column, aggregate}` values make implicit measures, so a measure-only values area
        // skips the parse.
        let kept = if has_column_value(p) { kept_pivot_measures(s) } else { Vec::new() };
        values_area(model, p, &kept)?
    };
    let base = s.dax_model.as_ref().map_or(0, |model| model.measures.len());
    if let Some(model) = s.dax_model.as_mut() {
        model.measures.extend(area.implicit);
    }
    let out = create_with(s, p, &area.names, package);
    if let Some(model) = s.dax_model.as_mut() {
        model.measures.truncate(base);
    }
    out
}

/// [`create`] with the implicit value measures already in the model.
fn create_with(s: &mut Session, p: &Json, value_fields: &[String], package: bool) -> Result<(Json, Option<Vec<u8>>)> {
    let model = s.dax_model.as_ref().ok_or_else(|| bad(CMD, "no data model loaded"))?;
    let pivot = pivot_param(model, p, value_fields)?;
    let table = pivot.render(model).map_err(|error| other(error.to_string()))?;
    let report = report_grid(&table)?;
    let values = spec_values(model, &pivot)?;
    let mut spec = spec_from_model(model, &pivot, &values)?;
    check_axes(&spec)?;
    let formats = value_formats(&values);
    let model_data = model_data(s)?;
    let replace = optional_bool(p, "replace")?.unwrap_or(false);
    let requested_name = optional_str(p, "name")?;
    let (sheet, at) = destination(s, p)?;
    let name = requested_name.map(str::trim).filter(|name| !name.is_empty());
    // Only a named PivotTable can be replaced: `attach_model_pivot` updates the part with that name
    // and otherwise adds one, so an unnamed `replace` would leave the old PivotTable attached and
    // stack a second one on the same cells.
    if replace && name.is_none() {
        return Err(other("`name` is required when `replace` is true."));
    }
    // A named `replace` re-applies the pivot the workbook keeps under that name: find what its
    // previous report occupied so the edit can clear it before writing a report of another shape.
    let previous = match (replace, name) {
        (true, Some(name)) => previous_report(s, name)?,
        _ => None,
    };

    let (reply, package) = edit(s, |cx| {
        let sheet_index = cx.wb.sheet_index(&sheet).ok_or_else(|| bad(CMD, format!("no sheet named `{sheet}`")))?;
        let name = pivot_name(&cx.wb, requested_name, replace)?;
        let body = CellRef::new(at.row.saturating_add((spec.filters.len() * PAGE_ROWS) as u32), at.col);
        let end = CellRef::new(
            body.row.saturating_add((report.len().saturating_sub(1)) as u32),
            body.col.saturating_add((report.first().map_or(0, Vec::len).saturating_sub(1)) as u32),
        );
        let area = RangeRef::new(at, end);
        if area.end.row >= gridcraft_core::MAX_ROWS || area.end.col >= gridcraft_core::MAX_COLS {
            return Err(other("The PivotTable report will not fit on the sheet. Choose a different location."));
        }
        {
            let existing = cx.wb.sheet(sheet_index).ok_or_else(|| bad(CMD, format!("no sheet named `{sheet}`")))?;
            // A replacement may overwrite the cells of its own previous report and no others; with
            // nothing to replace, the whole area must be free.
            let own =
                |at: CellRef| replace && previous.as_ref().is_some_and(|(old_sheet, old)| old_sheet.eq_ignore_ascii_case(&sheet) && old.contains(at));
            if let Some((cell, _)) =
                existing.cells.iter_range(area).find(|(at, cell)| !own(*at) && (!cell.value.is_empty() || cell.formula.is_some()))
            {
                return Err(other(format!(
                    "There's already data in {}!{}. Do you want to replace it? (Run again with replace: true to overwrite.)",
                    crate::pivot::quote_sheet(&sheet),
                    cell.a1()
                )));
            }
        }
        let pages = page_rows(&spec);
        let ids = intern_formats(&mut cx.wb, &formats);
        // `replace` re-applies the pivot: its previous report is cleared first, cell and style
        // alike (the way `edit.clearAll` leaves a cell), so a smaller second report leaves nothing
        // behind for Excel to trip over on refresh. Its page rows go too, then are rewritten below.
        if let Some((old_sheet, old_area)) = previous
            && let Some(index) = cx.wb.sheet_index(&old_sheet)
        {
            let cleared: Vec<CellRef> =
                cx.wb.sheet(index).map(|sh| sh.cells.iter_range(old_area).map(|(cell, _)| cell).collect()).unwrap_or_default();
            for cell in &cleared {
                cx.touch(index, *cell);
            }
            let sh = cx.sheet_mut(index)?;
            for cell in cleared {
                sh.remove_cell(cell);
            }
        }
        {
            let sh = cx.sheet_mut(sheet_index)?;
            for (i, row) in pages.iter().enumerate() {
                for (j, text) in row.iter().enumerate() {
                    let cell = CellRef::new(at.row.saturating_add(i as u32), at.col.saturating_add(j as u32));
                    sh.set_cell(cell, Cell::value(CoreValue::text(text.as_str())));
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

// ---------------------------------------------------------------- field list

/// The field-list pane's data: the model's tables, columns, measures and relationships, every model
/// PivotTable the workbook carries, and — for the one `name` selects — what it is built from (axes,
/// filters, values, layout, totals), so the pane can show those checked.
fn field_list_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(model) = s.dax_model.as_ref() else {
        return Err(bad(CMD_FIELDS, "no data model loaded"));
    };
    // An empty `name` is no name: the pane asks for a pivot by the name the command reported.
    let name = match p.get("name") {
        None | Some(Json::Null) => None,
        Some(Json::String(name)) => Some(name.trim()).filter(|name| !name.is_empty()),
        Some(_) => return Err(bad(CMD_FIELDS, "`name` must be a string")),
    };
    let data_model = model;
    let model = model_json(data_model);
    let parts = read_pivots(s)?;
    let pivots: Vec<Json> = parts
        .iter()
        .map(|part| {
            // `at` is the body's top-left; the pane also needs the whole report (page-field rows
            // included) to know whether the active cell is inside the pivot.
            let body = kept_location_ref(s, &part.spec.name).unwrap_or_else(|| RangeRef::cell(part.location));
            let page_rows = u32::try_from(part.spec.filters.len().saturating_mul(PAGE_ROWS)).unwrap_or(u32::MAX);
            let area = RangeRef::new(CellRef::new(body.start.row.saturating_sub(page_rows), body.start.col), body.end);
            json!({"name": part.spec.name, "sheet": part.sheet, "at": part.location.a1(), "range": area.a1()})
        })
        .collect();
    // Names are unique case-insensitively (the command refuses a duplicate), so the first match is
    // the one asked for; a name that is not there is `null`, not an error: the pane polls.
    let selected =
        name.and_then(|name| parts.iter().find(|part| part.spec.name.eq_ignore_ascii_case(name))).map(|part| selected_json(data_model, part));
    Ok(json!({"pivots": pivots, "model": model, "selected": selected}))
}

/// Every model PivotTable the workbook carries, read from its kept (pass-through) pivot parts.
///
/// The parts a save would write are already in the workbook ([`gridcraft_xlsx::attach_model_pivot`]
/// puts them there), so parsing them directly answers the field list without zipping every sheet
/// on each workbook edit. No open document carries none; a part that cannot be parsed is an engine
/// error, so a broken workbook is reported instead of the pane silently showing an empty list.
fn read_pivots(s: &Session) -> Result<Vec<ModelPivotPartInfo>> {
    let Ok(d) = s.doc() else { return Ok(Vec::new()) };
    let (parts, _report) = gridcraft_xlsx::read_kept_model_pivots(&d.wb)
        .map_err(|error| other(format!("the workbook's PivotTable definitions could not be read: {error}")))?;
    Ok(parts)
}

/// The `(name, table)` of every measure the workbook's kept model PivotTables declare.
///
/// A pivot's cache definition carries the whole measure set it was written with, implicit measures
/// included, so this is how a later call learns the names earlier calls took. A workbook whose
/// parts cannot be read contributes none: the check is a guard against a collision, not a
/// prerequisite for writing the pivot.
fn kept_pivot_measures(s: &Session) -> Vec<(String, String)> {
    let Ok(d) = s.doc() else { return Vec::new() };
    gridcraft_xlsx::read_kept_model_pivots(&d.wb)
        .map(|(parts, _report)| {
            parts.into_iter().flat_map(|part| part.spec.measures.into_iter().map(|measure| (measure.name, measure.table))).collect()
        })
        .unwrap_or_default()
}

/// The rectangle the named model PivotTable's previous report occupies — its page-field rows
/// included — and the sheet it is on. A named `replace` clears it before writing the new report: a
/// second report of a different shape would otherwise leave cells behind, and Excel's refresh of the
/// PivotTable then fails with `#SPILL!` at its anchor.
///
/// The body comes from the kept definition's `<location ref>` (the reader only reports the body's
/// top-left); the page fields above it take two rows each, as the writer lays them out.
fn previous_report(s: &Session, name: &str) -> Result<Option<(String, RangeRef)>> {
    let Some(part) = read_pivots(s)?.into_iter().find(|part| part.spec.name.eq_ignore_ascii_case(name)) else {
        return Ok(None);
    };
    let body = kept_location_ref(s, &part.spec.name).unwrap_or_else(|| RangeRef::cell(part.location));
    let page_rows = u32::try_from(part.spec.filters.len().saturating_mul(PAGE_ROWS)).unwrap_or(u32::MAX);
    let area = RangeRef::new(CellRef::new(body.start.row.saturating_sub(page_rows), body.start.col), body.end);
    let rows = u64::from(area.end.row.saturating_sub(area.start.row)).saturating_add(1);
    let cols = u64::from(area.end.col.saturating_sub(area.start.col)).saturating_add(1);
    // A rectangle outside the sheet, or bigger than the report budget, is not one this command
    // wrote: leave it alone rather than wiping an arbitrary part of the sheet.
    if area.end.row >= gridcraft_core::MAX_ROWS || area.end.col >= gridcraft_core::MAX_COLS || rows.saturating_mul(cols) > MAX_REPORT_CELLS as u64 {
        return Ok(None);
    }
    Ok(Some((part.sheet, area)))
}

/// The `<location ref>` of the kept PivotTable definition called `name` (case-insensitively): the
/// report's body, which the writer records as a range.
fn kept_location_ref(s: &Session, name: &str) -> Option<RangeRef> {
    let d = s.doc().ok()?;
    for part in &d.wb.preserved.as_deref()?.parts {
        if !is_pivot_table_part(&part.name) {
            continue;
        }
        let Ok(xml) = std::str::from_utf8(&part.data) else { continue };
        // The root element's `name` comes before any child's, so the first match is the definition's.
        let Some(defined) = attribute_value(xml, "name=\"") else { continue };
        if !defined.eq_ignore_ascii_case(name) {
            continue;
        }
        return attribute_value(xml, "location ref=\"").and_then(|reference| RangeRef::parse(&reference));
    }
    None
}

/// Whether `name` is a kept PivotTable definition part (`xl/pivotTables/*.xml`, not its rels).
///
/// A kept part whose name collided when it was written is renamed `pivotTable1_2.xml`, so the
/// stem's numeric suffix cannot identify a definition part.
fn is_pivot_table_part(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("xl/pivottables/") && !lower.contains("/_rels/") && lower.ends_with(".xml")
}

/// The value of the first `key="value"` pair in `xml`, where `key` ends at its opening quote.
///
/// The standard XML entities are decoded, so a PivotTable named `R&D` (written `R&amp;D`) matches
/// the name the caller asked for. `&amp;` is decoded last so `&amp;lt;` stays `&lt;`.
fn attribute_value(xml: &str, key: &str) -> Option<String> {
    let rest = xml.get(xml.find(key)?.saturating_add(key.len())..)?;
    let raw = rest.get(..rest.find('"')?)?;
    Some(raw.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&"))
}

/// The model as the field list shows it: tables with their columns and data types, the measures with
/// their home table and number format, and the relationships.
fn model_json(model: &DataModel) -> Json {
    json!({
        "tables": model
            .tables
            .iter()
            .map(|table| {
                json!({
                    "name": table.name,
                    "columns": table
                        .columns
                        .iter()
                        .map(|column| json!({"name": column.name, "type": column.data_type}))
                        .collect::<Vec<_>>(),
                })
            })
            .collect::<Vec<_>>(),
        "measures": model
            .measures
            .iter()
            .map(|measure| json!({"name": measure.name, "table": measure.table, "format": measure.format}))
            .collect::<Vec<_>>(),
        "relationships": model
            .relationships
            .iter()
            .map(|relationship| {
                json!({
                    "from_table": relationship.from_table,
                    "from_column": relationship.from_column,
                    "to_table": relationship.to_table,
                    "to_column": relationship.to_column,
                })
            })
            .collect::<Vec<_>>(),
    })
}

/// One model PivotTable's settings as the pane shows them: the axes as `Table[Column]` references,
/// the filters with their allowed values (a blank as JSON `null`), the value measures, the layout
/// and the three totals flags.
fn selected_json(model: &DataModel, part: &ModelPivotPartInfo) -> Json {
    let spec = &part.spec;
    json!({
        "name": spec.name,
        "sheet": part.sheet,
        "at": part.location.a1(),
        "row_fields": spec.row_fields.iter().map(field_text).collect::<Vec<_>>(),
        "column_fields": spec.column_fields.iter().map(field_text).collect::<Vec<_>>(),
        "filters": spec
            .filters
            .iter()
            .map(|filter| {
                let field = field_text(&filter.field);
                // `(All)` is the field with no `allowed` key, the shape `createPivot` reads back as
                // a filter that keeps every value.
                if filter.all {
                    return json!({"field": field});
                }
                let mut allowed: Vec<Json> = filter.allowed.iter().map(pivot_scalar_json).collect();
                // The reader reports blank as a flag on the filter; the pane lists it as a member
                // like any other, which is the shape `createPivot` accepts back.
                if filter.include_blank {
                    allowed.push(Json::Null);
                }
                json!({"field": field, "allowed": allowed})
            })
            .collect::<Vec<_>>(),
        "values": spec.values.iter().map(|value| value_json(model, spec, value)).collect::<Vec<_>>(),
        "layout": layout_name(spec.layout),
        "row_subtotals": spec.row_subtotals,
        "column_subtotals": spec.column_subtotals,
        "grand_totals": spec.grand_totals,
    })
}

/// One Σ Values entry as the pane (and `createPivot.values`) takes it back.
///
/// A measure the session model declares is its name. A column put in Σ Values becomes an implicit
/// measure that lives only in the pivot cache; reporting it as `{column, aggregate}` is what lets
/// the field list re-apply the pivot, because the session model has no measure to name.
fn value_json(model: &DataModel, spec: &ModelPivotSpec, value: &ModelPivotValue) -> Json {
    if model.measure_index(&value.measure).is_some() {
        return Json::String(value.measure.clone());
    }
    let Some((aggregate, rest)) = value.measure.split_once(" of ") else {
        return Json::String(value.measure.clone());
    };
    if !matches!(aggregate, "Sum" | "Average" | "Count" | "Min" | "Max") {
        return Json::String(value.measure.clone());
    }
    let table = spec
        .measures
        .iter()
        .find(|measure| measure.name.eq_ignore_ascii_case(&value.measure))
        .and_then(|measure| model.tables.iter().find(|table| table.name.eq_ignore_ascii_case(&measure.table)));
    let Some(table) = table else { return Json::String(value.measure.clone()) };
    // `implicit_measure` appends one numeric suffix (2, 3, …) when the name is taken, so try the
    // exact column first and then every split that leaves such a suffix — shortest suffix first.
    let mut candidates = vec![rest.to_string()];
    let digits = rest.len() - rest.trim_end_matches(|character: char| character.is_ascii_digit()).len();
    for cut in 1..=digits {
        let (column, suffix) = rest.split_at(rest.len() - cut);
        if !suffix.starts_with('0') && suffix.parse::<u32>().is_ok_and(|number| number >= 2) {
            candidates.push(column.to_string());
        }
    }
    let Some(column) = candidates.iter().find_map(|candidate| table.columns.iter().find(|column| column.name.eq_ignore_ascii_case(candidate))) else {
        return Json::String(value.measure.clone());
    };
    json!({"column": format!("{}[{}]", table.name, column.name), "aggregate": aggregate})
}

/// A field reference as the model's `Table[Column]` text.
fn field_text(field: &ModelPivotFieldRef) -> String {
    format!("{}[{}]", field.table, field.column)
}

/// A layout as the command's own `compact|outline|tabular` spelling.
fn layout_name(layout: ModelPivotLayout) -> &'static str {
    match layout {
        ModelPivotLayout::Compact => "compact",
        ModelPivotLayout::Outline => "outline",
        ModelPivotLayout::Tabular => "tabular",
    }
}

/// A cache scalar as JSON: numbers as numbers, text as text, blank as `null`. A non-finite number
/// has no JSON form and reads as `null`.
fn pivot_scalar_json(value: &ModelPivotScalar) -> Json {
    match value {
        ModelPivotScalar::Blank => Json::Null,
        ModelPivotScalar::Number(number) => serde_json::Number::from_f64(*number).map(Json::Number).unwrap_or(Json::Null),
        ModelPivotScalar::Text(text) => Json::String(text.clone()),
        ModelPivotScalar::Boolean(flag) => Json::Bool(*flag),
    }
}

// ---------------------------------------------------------------- parameters

fn wrong_type(key: &str, what: &str) -> EngineError {
    bad(CMD, format!("`{key}` must be {what}"))
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

/// One aggregation Excel offers for a column in the values area, and the DAX an implicit measure
/// over that column evaluates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aggregate {
    Sum,
    Average,
    Count,
    Min,
    Max,
}

impl Aggregate {
    /// The five aggregates, in the order Excel's Summarize Values By menu lists them.
    const ALL: [Aggregate; 5] = [Aggregate::Sum, Aggregate::Average, Aggregate::Count, Aggregate::Min, Aggregate::Max];

    /// The aggregate's canonical name (`Sum`): the prefix of an implicit measure's name and the
    /// spelling a `values` entry accepts, case-insensitively.
    fn label(self) -> &'static str {
        match self {
            Aggregate::Sum => "Sum",
            Aggregate::Average => "Average",
            Aggregate::Count => "Count",
            Aggregate::Min => "Min",
            Aggregate::Max => "Max",
        }
    }

    /// The aggregate `name` spells; `None` for anything else.
    fn parse(name: &str) -> Option<Aggregate> {
        Aggregate::ALL.into_iter().find(|aggregate| name.trim().eq_ignore_ascii_case(aggregate.label()))
    }

    /// Whether the aggregate needs a numeric column. `Count` counts a column's non-blank values,
    /// so it takes any type.
    fn needs_number(self) -> bool {
        !matches!(self, Aggregate::Count)
    }

    /// The formula Excel's implicit measure for `table[column]` evaluates, spelled by the same
    /// builder the model window uses. `Count` is `COUNT` and not `COUNTROWS`: Excel counts the
    /// column's non-blank values.
    fn formula(self, table: &str, column: &str) -> Option<String> {
        gridcraft_dax::implicit_measure_formula(self.label(), table, column)
    }
}

/// The aggregate a column gets when its `values` entry names none: numbers sum, everything else
/// counts — Excel's default summarization.
fn default_aggregate(column: &ModelColumn) -> Aggregate {
    if matches!(data_type(column.data_type.as_deref()), ModelPivotDataType::Number) { Aggregate::Sum } else { Aggregate::Count }
}

/// One entry of the values area as the caller wrote it: a measure name, or a model column and the
/// aggregation that summarizes it.
enum ValueEntry {
    Measure(String),
    Column { field: String, aggregate: Option<String> },
}

/// The values area resolved against the model: the value fields in column order, and the implicit
/// measures the `{column, aggregate}` entries declare.
struct ValuesArea {
    /// The measure names [`PivotTable::values`] holds.
    names: Vec<String>,
    /// Measures over model columns, made for this call only (see [`create`]).
    implicit: Vec<Measure>,
}

/// The optional `values` area, in column order: measure names and `{column, aggregate}` objects (a
/// single string is accepted too). Absent, `null` or empty keeps the single measure the pivot is
/// built around.
fn values_param(p: &Json) -> Result<Vec<ValueEntry>> {
    match p.get("values") {
        None | Some(Json::Null) => Ok(Vec::new()),
        Some(Json::String(measure)) => Ok(vec![ValueEntry::Measure(measure.clone())]),
        Some(Json::Array(items)) => {
            if items.len() > MAX_VALUE_FIELDS {
                return Err(bad(CMD, format!("`values` accepts at most {MAX_VALUE_FIELDS} fields")));
            }
            items.iter().map(value_entry).collect()
        }
        Some(_) => Err(wrong_type("values", "a list of measure names or {column, aggregate} objects")),
    }
}

/// Whether `values` holds a `{column, aggregate}` entry — the only kind that makes an implicit
/// measure, and so the only kind that needs the measure names the kept PivotTables declare. A
/// malformed `values` answers `false` and is left for [`values_param`] to refuse.
fn has_column_value(p: &Json) -> bool {
    p.get("values").and_then(Json::as_array).is_some_and(|items| items.iter().any(Json::is_object))
}

/// One `values` entry: a measure name, or `{column, aggregate?}`.
fn value_entry(item: &Json) -> Result<ValueEntry> {
    if let Some(measure) = item.as_str() {
        return Ok(ValueEntry::Measure(measure.to_string()));
    }
    let object = item.as_object().ok_or_else(|| wrong_type("values", "a list of measure names or {column, aggregate} objects"))?;
    let field = object
        .get("column")
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .ok_or_else(|| bad(CMD, "a `values` entry needs `column` (`Table[Column]`) or a measure name"))?;
    let aggregate = match object.get("aggregate") {
        None | Some(Json::Null) => None,
        Some(Json::String(name)) => Some(name.clone()),
        Some(_) => return Err(wrong_type("aggregate", "a summarization name (Sum, Average, Count, Min or Max)")),
    };
    Ok(ValueEntry::Column { field: field.to_string(), aggregate })
}

/// The values area resolved against `model`: every measure name in the model's spelling, and one
/// implicit measure per `{column, aggregate}` entry, named the way Excel names it (`Sum of
/// Amount`) and made unique in the model.
///
/// The column is checked before anything renders: it must exist, and `Sum`, `Average`, `Min` and
/// `Max` need a numeric column (`Count` takes any type).
fn values_area(model: &DataModel, p: &Json, kept: &[(String, String)]) -> Result<ValuesArea> {
    let entries = values_param(p)?;
    let mut names: Vec<String> = Vec::with_capacity(entries.len());
    let mut implicit: Vec<Measure> = Vec::new();
    for entry in entries {
        match entry {
            ValueEntry::Measure(name) => names.push(resolve_measure(model, &name)?),
            ValueEntry::Column { field, aggregate } => {
                let measure = implicit_measure(model, &field, aggregate.as_deref(), &implicit, kept)?;
                names.push(measure.name.clone());
                implicit.push(measure);
            }
        }
    }
    Ok(ValuesArea { names, implicit })
}

/// The implicit measure a `{column, aggregate}` value is.
///
/// The name is Excel's `Sum of Amount`, with a number appended when the model, an earlier value of
/// the same call, or a measure a kept PivotTable declares over another table already carries it;
/// the formula is the one Excel's implicit measure evaluates, parsed here because
/// [`gridcraft_dax::Evaluator`] takes only prepared measures.
fn implicit_measure(model: &DataModel, field: &str, aggregate: Option<&str>, taken: &[Measure], kept: &[(String, String)]) -> Result<Measure> {
    let reference = field_ref(model, field)?;
    let column = model
        .tables
        .iter()
        .find(|table| table.name.eq_ignore_ascii_case(&reference.table))
        .and_then(|table| table.columns.iter().find(|column| column.name.eq_ignore_ascii_case(&reference.column)))
        .ok_or_else(|| bad(CMD, format!("`{field}` is not a model column")))?;
    let aggregate = match aggregate {
        Some(name) => Aggregate::parse(name)
            .ok_or_else(|| bad(CMD, format!("unknown summarization `{name}` for `{field}` (Sum, Average, Count, Min or Max)")))?,
        None => default_aggregate(column),
    };
    if aggregate.needs_number() && !matches!(data_type(column.data_type.as_deref()), ModelPivotDataType::Number) {
        return Err(bad(CMD, format!("`{field}` is not a numeric column, so {} can't summarize it; use Count instead", aggregate.label())));
    }
    let base = format!("{} of {}", aggregate.label(), reference.column);
    let mut name = base.clone();
    let mut suffix = 1u32;
    while model.measure_index(&name).is_some()
        || taken.iter().any(|measure| measure.name.eq_ignore_ascii_case(&name))
        || kept.iter().any(|(kept_name, kept_table)| kept_name.eq_ignore_ascii_case(&name) && !kept_table.eq_ignore_ascii_case(&reference.table))
    {
        // A bound, not a search: the model and the values area both hold a bounded number of
        // measures, so a name collides only that many times.
        suffix = suffix.saturating_add(1);
        if suffix > MAX_VALUE_FIELDS as u32 + 1 {
            return Err(other(format!("too many values over `{field}`")));
        }
        name = format!("{base}{suffix}");
    }
    let formula = aggregate
        .formula(&reference.table, &reference.column)
        .ok_or_else(|| other(format!("the implicit measure for `{field}` could not be built")))?;
    let expr = parse_expression(&formula).map_err(|error| other(format!("the implicit measure for `{field}` could not be built: {error}")))?;
    Ok(Measure { name, formula, table: Some(reference.table), format: None, expr: Some(expr) })
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

/// The `filters` parameter: `[{field, allowed: [..], include_blank?}]`.
///
/// An absent `allowed` key is Excel's `(All)`: the field becomes a page field that keeps every
/// value (`PivotFilter::all`). An `allowed` list that is present, even empty, is an exact member
/// set — an empty one keeps nothing, as in `gridcraft_dax::PivotFilter` — so the two cases stay
/// distinguishable.
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
        let requested = item.get("allowed").or_else(|| item.get("values"));
        // JSON `null` means absent, as it does for every other parameter: the field is a page
        // field that keeps every value.
        let all = matches!(requested, None | Some(Json::Null));
        let mut allowed = match requested {
            None | Some(Json::Null) => Vec::new(),
            Some(Json::Array(values)) => {
                // This bounds the members of one page filter, not the number of fields on an axis:
                // a pivot read back from a file can select more than an axis may hold.
                if values.len() > MAX_AXIS_MEMBERS {
                    return Err(bad(CMD, format!("a filter accepts at most {MAX_AXIS_MEMBERS} values")));
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
        out.push(PivotFilter { field: field.to_string(), allowed, all });
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

/// A measure name resolved to the model's own spelling; an unknown name is a parameter error.
fn resolve_measure(model: &DataModel, name: &str) -> Result<String> {
    model
        .measure_index(name)
        .and_then(|index| model.measures.get(index))
        .map(|measure| measure.name.clone())
        .ok_or_else(|| bad(CMD, format!("no measure named `{name}` in the data model")))
}

/// The pivot the parameters describe, with every value field resolved by [`values_area`] and every
/// other name resolved against the model, so an unknown measure, table or field is an error before
/// anything is rendered.
fn pivot_param(model: &DataModel, p: &Json, values: &[String]) -> Result<PivotTable> {
    // The source measure supplies the grouping table. `measure` is optional when the values area
    // names one: a pivot built from a column alone groups by that column's table, exactly as the
    // implicit measure it declared says.
    let measure = match optional_str(p, "measure")?.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => resolve_measure(model, name)?,
        None => values.first().cloned().ok_or_else(|| bad(CMD, "a PivotTable needs `measure` or a column in `values`"))?,
    };
    let row_fields = field_list(p, "row_fields")?;
    let column_fields = field_list(p, "column_fields")?;
    // Resolving now reports an unknown field as a parameter error, and proves every field belongs
    // to a model table before the renderer and the cache writer see it.
    for field in row_fields.iter().chain(&column_fields) {
        pivot_fields(model, std::slice::from_ref(field)).map_err(|error| bad(CMD, error.to_string()))?;
    }
    let filters = filters_param(p)?;
    let mut pivot = PivotTable::new(optional_str(p, "name")?.unwrap_or_default(), measure);
    pivot.values = values.to_vec();
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

/// The workbook's `xl/model/item.data` with the session model's measures written into it, when a
/// read kept the part: the cache can then be refreshed in Excel and its measures evaluate.
///
/// Measures live only in the session model, so a cache that references them needs them in the
/// workbook's model too; `write_measures` rewrites the `MdxScript` of the kept container without
/// touching its tables.
fn model_data(s: &Session) -> Result<Option<Arc<[u8]>>> {
    let Ok(d) = s.doc() else { return Ok(None) };
    let Some(preserved) = d.wb.preserved.as_ref() else { return Ok(None) };
    let Some(part) = preserved.parts.iter().find(|part| part.name.eq_ignore_ascii_case(MODEL_DATA_PART)) else {
        return Ok(None);
    };
    let bytes = Arc::clone(&part.data);
    let Some(model) = s.dax_model.as_ref() else { return Ok(Some(bytes)) };
    let mut measures: Vec<gridcraft_dax::model_file::ModelMeasure> = model
        .measures
        .iter()
        .filter_map(|measure| {
            let table = measure.table.clone()?;
            let formula = measure.formula.trim().trim_start_matches('=').trim().to_string();
            (!table.trim().is_empty() && !formula.is_empty()).then(|| gridcraft_dax::model_file::ModelMeasure {
                table,
                name: measure.name.clone(),
                formula,
                format: measure.format.clone().unwrap_or_default(),
            })
        })
        .collect();
    // A rewritten script replaces every measure, so keep the ones the workbook's kept model
    // PivotTables still reference (an implicit measure written by an earlier call lives only in
    // the cache). A session measure of the same name wins.
    let (parts, _) = gridcraft_xlsx::read_kept_model_pivots(&d.wb).unwrap_or_default();
    let referenced: std::collections::BTreeSet<String> =
        parts.iter().flat_map(|part| part.spec.values.iter().map(|value| value.measure.clone())).collect();
    let current = gridcraft_dax::model_file::read_measures(&bytes).unwrap_or_default();
    for name in referenced {
        if measures.iter().any(|measure| measure.name.eq_ignore_ascii_case(&name)) {
            continue;
        }
        if let Some(existing) = current.iter().find(|measure| measure.name.eq_ignore_ascii_case(&name)) {
            measures.push(existing.clone());
        }
    }
    if measures.is_empty() {
        return Ok(Some(bytes));
    }
    let written = gridcraft_dax::model_file::write_measures(&bytes, &measures)
        .map_err(|error| other(format!("the data model could not be written to the workbook: {error}")))?;
    Ok(Some(Arc::from(written)))
}

/// The PivotTable's name: the `name` parameter (checked against every PivotTable the workbook
/// already has) or the first free `ModelPivotN`.
///
/// `replace` re-applies a PivotTable the workbook keeps under that name — the field-list pane's
/// Apply — and [`gridcraft_xlsx::attach_model_pivot`] updates that pivot in place. A PivotTable the
/// writer cannot rewrite (a worksheet pivot) keeps its name reserved either way.
fn pivot_name(wb: &Workbook, requested: Option<&str>, replace: bool) -> Result<String> {
    let kept = kept_pivot_names(wb);
    let kept_name = |name: &str| kept.iter().any(|kept| kept.eq_ignore_ascii_case(name));
    let taken = |name: &str| wb.sheets.iter().any(|sheet| sheet.pivots.iter().any(|pivot| pivot.name.eq_ignore_ascii_case(name))) || kept_name(name);
    match requested.map(str::trim).filter(|name| !name.is_empty()) {
        Some(name) => {
            if taken(name) && !(replace && kept_name(name)) {
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
        .filter(|part| is_pivot_table_part(&part.name))
        .filter_map(|part| std::str::from_utf8(&part.data).ok())
        .filter_map(|xml| attribute_value(xml, "name=\""))
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

/// The item a page field shows: its only allowed value, `(All)`, `(Multiple Items)`, or
/// `(No Items)` for an explicitly empty list — an exact empty member set, which keeps no rows.
fn page_item(filter: &ModelPivotFilter) -> String {
    if filter.all {
        return "(All)".to_string();
    }
    match filter.allowed.as_slice() {
        [value] => value.label(),
        [] => "(No Items)".to_string(),
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
            Ok(ModelPivotFilter { field, allowed, include_blank, all: filter.all })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(all: bool, allowed: Vec<ModelPivotScalar>) -> ModelPivotFilter {
        ModelPivotFilter { field: ModelPivotFieldRef { table: "Sales".into(), column: "Channel".into() }, allowed, include_blank: false, all }
    }

    #[test]
    fn a_page_field_captions_all_and_an_empty_member_set_differently() {
        assert_eq!(page_item(&filter(true, Vec::new())), "(All)", "an absent allowed list keeps every value");
        assert_eq!(page_item(&filter(false, Vec::new())), "(No Items)", "an explicit empty list keeps nothing");
        assert_eq!(page_item(&filter(false, vec![ModelPivotScalar::Text("Web".into())])), "Web");
        assert_eq!(page_item(&filter(false, vec![ModelPivotScalar::Text("Web".into()), ModelPivotScalar::Text("Store".into())])), "(Multiple Items)");
    }
}
