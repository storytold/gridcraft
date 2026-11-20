//! XLSB import, delegated to `craft_xlsx::Package`.
//! Cell values, formulas, defined names, and merged cells are imported; formatting, charts,
//! images, macros and other workbook features are not imported.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, DateSystem, RangeRef, Value};
use gridcraft_model::{Cell, DefinedName, Formula, Sheet, StyleId, Visibility, Workbook};

use crate::{IoError, ReadReport};

fn map_error(err: craft_xlsx::Error) -> IoError {
    match err {
        craft_xlsx::Error::Zip(m) => IoError::Zip(m),
        craft_xlsx::Error::Xml(m) => IoError::Xml(m),
        craft_xlsx::Error::Invalid(m) => IoError::Format(m),
        craft_xlsx::Error::TooLarge(m) => IoError::TooLarge(m),
    }
}

fn map_cell_error(err: craft_xlsx::CellError) -> CellError {
    match err {
        craft_xlsx::CellError::Null => CellError::Null,
        craft_xlsx::CellError::Div0 => CellError::Div0,
        craft_xlsx::CellError::Value => CellError::Value,
        craft_xlsx::CellError::Ref => CellError::Ref,
        craft_xlsx::CellError::Name => CellError::Name,
        craft_xlsx::CellError::Num => CellError::Num,
        craft_xlsx::CellError::NA => CellError::NA,
        craft_xlsx::CellError::GettingData => CellError::GettingData,
        craft_xlsx::CellError::Spill => CellError::Spill,
        craft_xlsx::CellError::Calc => CellError::Calc,
        craft_xlsx::CellError::Blocked => CellError::Blocked,
        craft_xlsx::CellError::Python => CellError::Python,
        craft_xlsx::CellError::Busy => CellError::Busy,
        craft_xlsx::CellError::Timeout => CellError::Timeout,
        _ => CellError::Value,
    }
}

fn map_value(value: craft_xlsx::Value) -> Value {
    match value {
        craft_xlsx::Value::Empty => Value::Empty,
        craft_xlsx::Value::Number(n) => Value::number(n),
        craft_xlsx::Value::Text(t) => Value::Text(t),
        craft_xlsx::Value::Bool(b) => Value::Bool(b),
        craft_xlsx::Value::Error(e) => Value::Error(map_cell_error(e)),
    }
}

fn map_visibility(visibility: craft_xlsx::Visibility) -> Visibility {
    match visibility {
        craft_xlsx::Visibility::Visible => Visibility::Visible,
        craft_xlsx::Visibility::Hidden => Visibility::Hidden,
        craft_xlsx::Visibility::VeryHidden => Visibility::VeryHidden,
    }
}

pub(super) fn import_limits() -> craft_xlsx::Limits {
    let limits = craft_xlsx::Limits::default();
    // Keep the lower decoded-text budget in the browser's WebAssembly heap.
    if cfg!(target_arch = "wasm32") { craft_xlsx::Limits { binary_text: 64 << 20, ..limits } } else { limits }
}

fn map_formula(f: &craft_xlsx::CellFormula, at: craft_xlsx::CellRef, report: &mut ReadReport) -> Option<Formula> {
    let text = crate::fmla::from_file(f.text.trim());
    if text.is_empty() {
        return None;
    }
    match f.kind {
        craft_xlsx::FormulaKind::Normal | craft_xlsx::FormulaKind::Shared => Some(Formula::new(text)),
        craft_xlsx::FormulaKind::Array => {
            let pos = CellRef::new(at.row, at.col);
            let mut fm = Formula::new(text);
            fm.array = Some(
                f.reference
                    .map(|r| RangeRef::new(CellRef::new(r.first.row, r.first.col), CellRef::new(r.last.row, r.last.col)))
                    .unwrap_or_else(|| RangeRef::cell(pos)),
            );
            Some(fm)
        }
        craft_xlsx::FormulaKind::DataTable => {
            report.warn("what-if data tables are not supported; kept their values");
            None
        }
    }
}

fn import_defined_names(book: &craft_xlsx::Workbook, file_to_model: &[Option<usize>], wb: &mut Workbook) {
    for d in &book.names {
        let scope = match d.local_sheet_id {
            Some(i) => match file_to_model.get(i as usize).copied().flatten() {
                Some(m) => Some(m),
                None => continue,
            },
            None => None,
        };
        let lname = d.name.to_ascii_lowercase();
        if lname == "_xlnm._filterdatabase" || lname.starts_with("_xlchart.") || lname == "_xlnm.print_area" || lname == "_xlnm.print_titles" {
            continue;
        }
        wb.names.push(DefinedName {
            name: d.name.clone(),
            scope,
            formula: crate::fmla::from_file(d.formula.trim()),
            comment: d.comment.clone(),
            hidden: d.hidden,
        });
    }
}

/// Imports worksheet cell values, formulas, defined names, and merged cells. Number formats are
/// omitted; dates remain serial numbers with the workbook's original date system. Inspect warnings.
pub fn read_xlsb(bytes: &[u8]) -> Result<(Workbook, ReadReport), IoError> {
    let limits = import_limits();
    let mut package = craft_xlsx::Package::open(bytes, &limits).map_err(map_error)?;
    if !package.is_binary() {
        return Err(IoError::Format("XLSB: not a binary workbook".into()));
    }
    let ct_bytes = package
        .opc()
        .read("[Content_Types].xml")
        .map_err(|e| IoError::Format(format!("XLSB: {e}")))?
        .ok_or_else(|| IoError::Format("XLSB: missing [Content_Types].xml".into()))?;
    let canonical = craft_xml::Canonical::new(&[("ct", craft_opc::NS_CONTENT_TYPES)]);
    let ct_el = craft_xml::Options::canonical(&canonical).strict().parse(&ct_bytes).map_err(|e| IoError::Xml(e.to_string()))?;
    if ct_el.name != "ct:Types" {
        return Err(IoError::Format("XLSB: wrong [Content_Types].xml namespace".into()));
    }
    if let Some(root_rels) = package.opc().read("_rels/.rels").map_err(|e| IoError::Format(format!("XLSB: {e}")))? {
        craft_xml::Options::local().strict().parse(&root_rels).map_err(|e| IoError::Xml(e.to_string()))?;
    }
    let book_part = package
        .opc()
        .rels("")
        .map_err(|e| IoError::Format(format!("XLSB: {e}")))?
        .first("officeDocument")
        .map(|r| r.target.clone())
        .unwrap_or_else(|| "xl/workbook.bin".into());
    let book_rels_path =
        if let Some((dir, file)) = book_part.rsplit_once('/') { format!("{dir}/_rels/{file}.rels") } else { format!("_rels/{book_part}.rels") };
    if let Some(book_rels) = package.opc().read(&book_rels_path).map_err(|e| IoError::Format(format!("XLSB: {e}")))? {
        craft_xml::Options::local().strict().parse(&book_rels).map_err(|e| IoError::Xml(e.to_string()))?;
    }
    let ct = craft_opc::ContentTypes::parse(&ct_bytes);
    let book = package.workbook().map_err(map_error)?;
    let strings = package.shared_strings().map_err(map_error)?;
    let mut wb = Workbook::new();
    wb.sheets.clear();
    wb.date_system = if book.date_1904 { DateSystem::D1904 } else { DateSystem::D1900 };
    let mut report = ReadReport::default();
    report.warn("XLSB import: formatting, charts, images, macros and other features are not imported; dates, currency and percentages may display as numbers. Save a new .xlsx file; XLSB saving is not supported.");
    let mut file_to_model = Vec::with_capacity(book.sheets.len());
    for entry in &book.sheets {
        if entry.kind != craft_xlsx::SheetKind::Worksheet {
            if !matches!(entry.kind, craft_xlsx::SheetKind::Chartsheet | craft_xlsx::SheetKind::Dialogsheet | craft_xlsx::SheetKind::Macrosheet) {
                return Err(IoError::Format("XLSB: unknown sheet relationship type".into()));
            }
            report.warn("XLSB non-worksheet sheets were skipped.");
            file_to_model.push(None);
            continue;
        }
        wb.check_sheet_name(&entry.name, None).map_err(|e| IoError::Format(format!("XLSB: {e}")))?;
        if let Some(ref part_name) = entry.part
            && ct.of(part_name) != Some("application/vnd.ms-excel.worksheet")
        {
            return Err(IoError::Format("XLSB: wrong worksheet content type".into()));
        }
        let mut sheet = Sheet::new(&entry.name);
        sheet.visibility = map_visibility(entry.visibility);
        let part = package
            .sheet(entry, |row| {
                for cell in row.cells {
                    let value = map_value(cell.value.resolve(&strings, book.date_1904));
                    let formula = cell.formula.as_ref().and_then(|f| map_formula(f, cell.at, &mut report));
                    if !value.is_empty() || formula.is_some() {
                        sheet.cells.set(
                            CellRef::new(cell.at.row, cell.at.col),
                            Cell { value, formula: formula.map(Arc::new), style: StyleId::DEFAULT, py: None },
                        );
                    }
                }
                Ok(())
            })
            .map_err(map_error)?;
        sheet.merges =
            part.merges.into_iter().map(|r| RangeRef::new(CellRef::new(r.first.row, r.first.col), CellRef::new(r.last.row, r.last.col))).collect();
        file_to_model.push(Some(wb.sheets.len()));
        wb.sheets.push(Arc::new(sheet));
    }
    import_defined_names(&book, &file_to_model, &mut wb);
    for warning in package.warnings() {
        report.warn(warning);
    }
    if wb.sheets.is_empty() {
        return Err(IoError::Format("XLSB: no supported worksheets".into()));
    }
    wb.set_active_sheet(wb.sheets.iter().position(|s| s.visibility == Visibility::Visible).unwrap_or(0));
    Ok((wb, report))
}

pub(crate) fn is_xlsb(bytes: &[u8]) -> bool {
    craft_xlsx::is_xlsb(bytes)
}
