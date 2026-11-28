//! Explicit clipboard-neutral block edits. Neither the mouse selection nor the OS clipboard is input.
use super::{Clipboard, EngineError, Json, RangeRef, Result, Session, bad, edit, json, paste_special::paste_snapshot};
use gridcraft_core::{CellRef, Value};
use gridcraft_model::{Sheet, Visibility, Workbook};

#[path = "transfer_bindings.rs"]
mod bindings;

const TRANSFER: &str = "range.transfer";
const RESHAPE: &str = "range.reshapeDelimited";
const MAX_BLOCK: u64 = 262_144;
const MAX_FIELDS: usize = 65_536;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

struct Request {
    from: usize,
    to: usize,
    src: RangeRef,
    at: CellRef,
    overwrite: bool,
}

fn target(wb: &Workbook, name: &str, id: &str) -> Result<usize> {
    let i = wb.sheet_index(name).filter(|&i| wb.sheets[i].visibility == Visibility::Visible).ok_or_else(|| bad(id, "No such visible worksheet."))?;
    if wb.sheets[i].is_protected() {
        return Err(bad(id, "Unprotect the worksheet before transferring cells."));
    }
    Ok(i)
}

// Trim only the unbounded dimension. Keep interior gaps, style-only cells and standalone
// notes. A distant entry in column Z must not turn A:A into a million-row block.
fn source(sh: &Sheet, r: RangeRef, id: &str) -> Result<RangeRef> {
    if r.is_full_cols() && r.is_full_rows() {
        return Err(bad(id, "Specify a block, not the entire worksheet."));
    }
    if !r.is_full_cols() && !r.is_full_rows() {
        return Ok(r);
    }
    let mut extent = None::<RangeRef>;
    let mut include = |area: RangeRef| {
        if let Some(part) = r.intersection(&area) {
            let used = extent.map_or(part, |prev| prev.union(&part));
            let cells =
                if r.is_full_cols() { u64::from(used.height()) * u64::from(r.width()) } else { u64::from(r.height()) * u64::from(used.width()) };
            if cells > MAX_BLOCK {
                return Err(bad(id, format!("Limit the occupied source to {MAX_BLOCK} positions.")));
            }
            extent = Some(used);
        }
        Ok(())
    };
    if r.is_full_cols() {
        // Row-major sparse storage directly yields the first/last occupied rows.
        let mut cells = sh.cells.iter_range(r);
        if let Some((at, _)) = cells.next() {
            include(RangeRef::cell(at))?;
        }
        if let Some((at, _)) = cells.next_back() {
            include(RangeRef::cell(at))?;
        }
    } else {
        for (at, _) in sh.cells.iter_range(r) {
            include(RangeRef::cell(at))?;
        }
    }
    for at in sh.comments.keys().chain(sh.hyperlinks.keys()).chain(sh.cell_pictures.keys()).chain(sh.spill.keys()) {
        if r.contains(*at) {
            include(RangeRef::cell(*at))?;
        }
    }
    for &m in &sh.merges {
        include(m)?;
    }
    let used = extent.ok_or_else(|| bad(id, "No occupied source cells in this row or column."))?;
    Ok(if r.is_full_cols() {
        RangeRef::new(CellRef::new(used.start.row, r.start.col), CellRef::new(used.end.row, r.end.col))
    } else {
        RangeRef::new(CellRef::new(r.start.row, used.start.col), CellRef::new(r.end.row, used.end.col))
    })
}

impl Request {
    fn read(s: &Session, p: &Json, id: &str) -> Result<Self> {
        let d = s.doc()?;
        if d.read_only_source.is_some() {
            return Err(bad(id, "This workbook was opened read-only."));
        }
        let from = target(&d.wb, p["sheet"].as_str().ok_or_else(|| bad(id, "Specify the source worksheet by name."))?, id)?;
        let to = match p.get("destinationSheet") {
            None => from,
            Some(n) => target(&d.wb, n.as_str().ok_or_else(|| bad(id, "Expected a destination worksheet name."))?, id)?,
        };
        let range = p["source"].as_str().and_then(RangeRef::parse).ok_or_else(|| bad(id, "Expected an unqualified source range."))?;
        let src = source(&d.wb.sheets[from], range, id)?;
        if src.count() > MAX_BLOCK {
            return Err(bad(id, format!("Limit the source to {MAX_BLOCK} positions.")));
        }
        let at = p["at"].as_str().and_then(CellRef::parse).ok_or_else(|| bad(id, "Specify the top-left destination as one A1 cell."))?;
        let overwrite = match p.get("overwrite") {
            None => false,
            Some(v) => v.as_bool().ok_or_else(|| bad(id, "overwrite must be true or false."))?,
        };
        Ok(Self { from, to, src, at, overwrite })
    }
    fn dest(&self, rows: u32, cols: u32, id: &str) -> Result<RangeRef> {
        let end = self
            .at
            .offset(i64::from(rows) - 1, i64::from(cols) - 1)
            .ok_or_else(|| bad(id, "The complete result would extend beyond the worksheet."))?;
        Ok(RangeRef::new(self.at, end))
    }
}

fn shared_guard(sh: &Sheet, r: RangeRef, id: &str) -> Result<()> {
    let reason = if sh.rows.range(r.start.row..=r.end.row).any(|(_, l)| l.hidden) || sh.cols.range(r.start.col..=r.end.col).any(|(_, l)| l.hidden) {
        "contains hidden rows or columns"
    } else if sh.tables.iter().any(|t| t.range.intersects(&r))
        || sh.pivots.iter().any(|x| x.last_range.unwrap_or(RangeRef::cell(x.anchor)).intersects(&r))
        || sh.autofilter.as_ref().is_some_and(|af| af.range.intersects(&r))
    {
        "intersects a table, filter or PivotTable"
    } else if sh.spill_ranges.values().any(|a| a.intersects(&r)) {
        "intersects an array or spill"
    } else if sh.sparklines.iter().any(|x| r.contains(x.cell)) {
        "contains a sparkline"
    } else {
        return Ok(());
    };
    Err(bad(id, format!("{}!{} {reason}. Choose a regular cell block.", sh.name, r.a1())))
}

fn check_dest(sh: &Sheet, dest: RangeRef, v: &Request, id: &str, same_source_move: bool, formats: bool) -> Result<()> {
    shared_guard(sh, dest, id)?;
    let moving = |c| same_source_move && v.src.contains(c);
    if sh.merges.iter().any(|m| m.intersects(&dest) && !(same_source_move && v.src.contains_range(m))) {
        return Err(bad(id, "Unmerge the destination first."));
    }
    if v.overwrite {
        return Ok(());
    }
    let occupied = sh.cells.iter_range(dest).any(|(at, c)| !moving(at) && (formats || c.has_content()))
        || sh
            .comments
            .keys()
            .chain(sh.hyperlinks.keys())
            .chain(sh.cell_pictures.keys())
            .chain(sh.spill.keys())
            .any(|at| dest.contains(*at) && !moving(*at))
        || sh
            .validations
            .iter()
            .flat_map(|v| &v.ranges)
            .chain(sh.cond_formats.iter().flat_map(|c| &c.ranges))
            .any(|r| r.intersection(&dest).is_some_and(|part| !(same_source_move && v.src.contains_range(&part))));
    if occupied {
        return Err(EngineError::Confirm(format!(
            "{}!{} is occupied. Choose an empty destination or explicitly approve overwrite.",
            sh.name,
            dest.a1()
        )));
    }
    Ok(())
}

pub(super) fn transfer(s: &mut Session, p: &Json) -> Result<Json> {
    let v = Request::read(s, p, TRANSFER)?;
    let (cut, transpose) = match p.get("operation").map(Json::as_str) {
        None | Some(Some("copy")) => (false, false),
        Some(Some("move")) => (true, false),
        Some(Some("transpose")) => (false, true),
        _ => return Err(bad(TRANSFER, "Use copy, move, or transpose (a transposed copy).")),
    };
    let dest = if transpose { v.dest(v.src.width(), v.src.height(), TRANSFER)? } else { v.dest(v.src.height(), v.src.width(), TRANSFER)? };
    let d = s.doc()?;
    let from = &d.wb.sheets[v.from];
    let to = &d.wb.sheets[v.to];
    let hits_array = if v.from == v.to {
        from.cells.legacy_array_intersects_any(&[v.src, dest])
    } else {
        from.cells.legacy_array_intersects(v.src) || to.cells.legacy_array_intersects(dest)
    };
    if hits_array {
        return Err(bad(TRANSFER, "The transfer intersects a legacy array."));
    }
    shared_guard(from, v.src, TRANSFER)?;
    if from.merges.iter().any(|m| m.intersects(&v.src) && (transpose || !v.src.contains_range(m))) {
        return Err(bad(TRANSFER, "Unmerge first, or copy/move the entire merged block without transposing it."));
    }
    if !cut && from.cond_formats.iter().any(|c| c.ranges.iter().any(|r| v.src.intersects(r))) {
        return Err(bad(
            TRANSFER,
            "A native cell copy does not carry conditional-format rules. Move the block or choose a source without those rules.",
        ));
    }
    // Literal copies carry the stored formula text; only rewriting a reference needs a parsed tree.
    let verbatim = !cut && p.get("keepReferences").and_then(Json::as_bool) == Some(true);
    if !verbatim && from.cells.iter_range(v.src).any(|(_, c)| c.formula.as_ref().is_some_and(|f| f.with_expr(|_| ()).is_none())) {
        return Err(bad(TRANSFER, "An unsupported source formula cannot be relocated without loss."));
    }
    if cut {
        bindings::guard_callers(&d.wb, &v)?;
        bindings::guard(&d.wb, &v)?;
    }
    if v.from == v.to && dest == v.src && !transpose {
        return Err(bad(TRANSFER, "Source and destination are the same block."));
    }
    check_dest(to, dest, &v, TRANSFER, cut && v.from == v.to, true)?;
    // Reuse native relative formula copying and workbook-wide cut-reference retargeting.
    // Do not render the source as TSV, consume the user's pending cut, or move the selection.
    let receipt = json!({"sheet":from.name,"source":v.src.a1(),"destinationSheet":to.name,"destination":dest.a1()});
    let selection = d.selection.clone();
    let clip = Clipboard {
        wb: d.wb.clone(),
        sheet: v.from,
        range: v.src,
        selected: v.src,
        cut,
        doc_uid: d.uid,
        text: String::new(),
        areas: vec![],
        trust: d.trust,
    };
    let mut paste = json!({"what":"all","at":v.at.a1(),"sheet":v.to,"transpose":transpose});
    if let Some(keep) = p.get("keepReferences") {
        paste["keepReferences"] = keep.clone();
    }
    let result = paste_snapshot(s, &paste, &clip, v.overwrite.then_some(dest));
    s.doc_mut()?.selection = selection;
    result?;
    Ok(receipt)
}

pub(super) fn reshape(s: &mut Session, p: &Json) -> Result<Json> {
    use std::borrow::Cow;
    let v = Request::read(s, p, RESHAPE)?;
    let layout = match p.get("layout").map(Json::as_str) {
        None => "rows",
        Some(Some(s @ ("rows" | "columns" | "row"))) => s,
        _ => return Err(bad(RESHAPE, "Use rows, columns or row.")),
    };
    let delimiter = p["delimiter"].as_str().ok_or_else(|| bad(RESHAPE, "Specify one literal delimiter character."))?;
    let mut chars = delimiter.chars();
    let delimiter = chars
        .next()
        .filter(|c| *c != '"' && *c != '\0' && chars.next().is_none())
        .ok_or_else(|| bad(RESHAPE, "Specify a single delimiter other than double quote or NUL."))?;
    if v.src.width() != 1 && v.src.height() != 1 {
        return Err(bad(RESHAPE, "Specify one source column or row."));
    }
    if v.src.count() > MAX_FIELDS as u64 {
        return Err(bad(RESHAPE, "Too many source cells; use a smaller range."));
    }
    let d = s.doc()?;
    let from = &d.wb.sheets[v.from];
    let to = &d.wb.sheets[v.to];
    if from.rows.range(v.src.start.row..=v.src.end.row).any(|(_, l)| l.hidden)
        || from.cols.range(v.src.start.col..=v.src.end.col).any(|(_, l)| l.hidden)
        || from.merges.iter().any(|m| m.intersects(&v.src))
    {
        return Err(bad(RESHAPE, "Choose unmerged, visible source cells."));
    }
    let mut parsed = Vec::new();
    let mut bytes = 0usize;
    let mut fields = 0usize;
    let mut longest = 0usize;
    for at in v.src.iter() {
        if from.cell_pictures.contains_key(&at) {
            return Err(bad(RESHAPE, format!("{} contains an image, not delimited text.", at.a1())));
        }
        let text = match from.value_ref(at) {
            None | Some(Value::Empty) => Cow::Borrowed(""),
            Some(Value::Text(t)) => Cow::Borrowed(t.as_ref()),
            Some(v @ (Value::Number(_) | Value::Bool(_))) => Cow::Owned(v.display()),
            _ => return Err(bad(RESHAPE, format!("{} does not contain text or a scalar value.", at.a1()))),
        };
        bytes = bytes.saturating_add(text.len());
        if bytes > MAX_TEXT_BYTES || text.encode_utf16().take(32768).count() > 32767 {
            return Err(bad(RESHAPE, "Source text exceeds the per-cell or transfer size limit."));
        }
        let parts = crate::cmd::data::split_delimited(&text, &[delimiter], false, '"');
        fields += parts.len();
        if fields > MAX_FIELDS {
            return Err(bad(RESHAPE, "Too many fields. Split the source into smaller blocks."));
        }
        longest = longest.max(parts.len());
        parsed.push(parts);
    }
    // Each field is literal. Never feed strings beginning with '=' (or ambiguous dates,
    // leading zeroes, etc.) through typed cell entry. All reads precede all writes.
    let (rows, cols) = match layout {
        "row" => (1, fields),
        "columns" => (longest, parsed.len()),
        _ => (parsed.len(), longest),
    };
    let dest = v.dest(rows as u32, cols as u32, RESHAPE)?;
    if to.cells.legacy_array_intersects(dest) {
        return Err(bad(RESHAPE, "The destination intersects a legacy array."));
    }
    if dest.count() > MAX_BLOCK {
        return Err(bad(RESHAPE, "The result is too wide or tall; split the source into smaller blocks."));
    }
    if to.validations.iter().any(|dv| dv.ranges.iter().any(|r| r.intersects(&dest)))
        || to.cond_formats.iter().any(|cf| cf.ranges.iter().any(|r| r.intersects(&dest)))
    {
        return Err(bad(RESHAPE, "Choose an output area without validation or conditional-format rules."));
    }
    check_dest(to, dest, &v, RESHAPE, false, false)?;
    let receipt = json!({"sheet":from.name,"source":v.src.a1(),"destinationSheet":to.name,"destination":dest.a1(),"fields":fields});
    edit(s, |cx| {
        let mut index = 0;
        for (i, row) in parsed.into_iter().enumerate() {
            // A ragged record occupies its share of the advertised rectangle, including
            // empty trailing fields. Flattened output has no padding.
            let width = if layout == "row" { row.len() } else { longest };
            for (j, text) in row.into_iter().map(Some).chain(std::iter::repeat(None)).take(width).enumerate() {
                let (dr, dc) = match layout {
                    "row" => (0, index),
                    "columns" => (j, i),
                    _ => (i, j),
                };
                let at = CellRef::new(v.at.row + dr as u32, v.at.col + dc as u32);
                let sh = cx.sheet_mut(v.to)?;
                if text.is_none()
                    && !(v.overwrite
                        && (sh.cell(at).is_some_and(|c| c.has_content())
                            || sh.comments.contains_key(&at)
                            || sh.hyperlinks.contains_key(&at)
                            || sh.cell_pictures.contains_key(&at)))
                {
                    continue;
                }
                if v.overwrite {
                    sh.comments.remove(&at);
                    sh.hyperlinks.remove(&at);
                }
                sh.set_value(at, text.as_deref().filter(|t| !t.is_empty()).map_or(Value::Empty, Value::text));
                cx.touch(v.to, at);
                index += 1;
            }
        }
        Ok(())
    })?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("sheet.rename", json!({"name":"Input"})).unwrap();
        s
    }
    fn get(s: &mut Session, sheet: &str, at: &str) -> Json {
        s.execute("cell.get", json!({"sheet":sheet,"cell":at})).unwrap()
    }
    #[test]
    fn same_sheet_cut_refuses_a_row_dependent_structured_reference_without_defined_names() {
        let mut s = setup();
        s.execute("cell.set", json!({"cell":"B2","input":"=Sales[@Amount]"})).unwrap();
        let before = s.doc().unwrap().wb.clone();
        let err = s.execute(TRANSFER, json!({"sheet":"Input","source":"B2","at":"B3","operation":"move"})).unwrap_err();
        assert!(err.to_string().contains("structured reference"));
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    }

    #[test]
    fn overwrite_clears_only_conditional_formats_inside_the_destination() {
        for operation in ["copy", "transpose"] {
            let mut s = setup();
            s.execute("home.conditionalFormat", json!({"range":"E2:E5","rule":{"type":"expression","formula":"E2>5","preset":"lightRedFill"}}))
                .unwrap();
            let before = s.doc().unwrap().wb.sheets[0].cond_formats.clone();
            let request = json!({"sheet":"Input","source":"A1:B1","at":"E2","operation":operation,"overwrite":true});
            let receipt = s.execute(TRANSFER, request).unwrap();
            let dest = RangeRef::parse(receipt["destination"].as_str().unwrap()).unwrap();
            let formats = &s.doc().unwrap().wb.sheets[0].cond_formats;
            let outside = CellRef::parse("E5").unwrap();
            assert!(formats.iter().flat_map(|f| &f.ranges).any(|r| r.contains(outside)));
            assert!(!formats.iter().flat_map(|f| &f.ranges).any(|r| r.intersects(&dest)));
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(s.doc().unwrap().wb.sheets[0].cond_formats, before);
        }
    }

    #[test]
    fn copied_validation_thresholds_transpose_from_the_actual_copied_cells() {
        let mut s = setup();
        s.execute("data.validation", json!({"range":"A1:C2","type":"decimal","formula1":"=B1","formula2":"=$G2"})).unwrap();
        s.execute(TRANSFER, json!({"sheet":"Input","source":"B2:C2","at":"E2","operation":"transpose"})).unwrap();
        for (cell, expected) in [("E2", "E3"), ("E3", "E4")] {
            let dest = RangeRef::parse(cell).unwrap();
            let rule = s.doc().unwrap().wb.sheets[0].validations.iter().find(|r| r.ranges.contains(&dest)).unwrap();
            assert_eq!((rule.f1.as_str(), rule.f2.as_deref()), (expected, Some("$G3")));
        }
    }

    #[test]
    fn only_mixed_references_split_a_transposed_validation() {
        let mut s = setup();
        s.execute("data.validation", json!({"range":"A1:A2","type":"custom","formula1":"=$G2"})).unwrap();
        s.execute(TRANSFER, json!({"sheet":"Input","source":"A1:A2","at":"D4","operation":"transpose"})).unwrap();
        for (cell, expected) in [("D4", "$G2"), ("E4", "$G3")] {
            let dest = RangeRef::parse(cell).unwrap();
            let rule = s.doc().unwrap().wb.sheets[0].validations.iter().find(|r| r.ranges.contains(&dest)).unwrap();
            assert_eq!(rule.f1, expected);
        }
        s.execute("edit.undo", json!({})).unwrap();
        s.execute("data.validation", json!({"range":"A1:A2","type":"custom","formula1":"=B1+$G$2"})).unwrap();
        s.execute(TRANSFER, json!({"sheet":"Input","source":"A1:A2","at":"D4","operation":"transpose"})).unwrap();
        let dest = RangeRef::parse("D4:E4").unwrap();
        let rule = s.doc().unwrap().wb.sheets[0].validations.iter().find(|r| r.ranges.contains(&dest)).unwrap();
        assert_eq!(rule.f1, "D5+$G$2");
    }

    #[test]
    fn verbatim_copy_changes_placement_but_not_cell_or_validation_references() {
        for (operation, pasted) in [("copy", "D4:E4"), ("transpose", "D4:D5")] {
            let mut s = setup();
            s.execute("cell.set", json!({"cell":"B2","input":"=A1*$G$2"})).unwrap();
            s.execute("data.validation", json!({"range":"B2:C2","type":"decimal","formula1":"=A1","formula2":"=$G2"})).unwrap();
            let formula = get(&mut s, "Input", "B2")["formula"].clone();
            s.execute(TRANSFER, json!({"sheet":"Input","source":"B2:C2","at":"D4","operation":operation,"keepReferences":true})).unwrap();
            assert_eq!(get(&mut s, "Input", "D4")["formula"], formula);
            let dest = RangeRef::parse(pasted).unwrap();
            let rules = &s.doc().unwrap().wb.sheets[0].validations;
            let copied = rules.iter().find(|r| r.ranges.contains(&dest)).unwrap();
            assert_eq!((copied.f1.as_str(), copied.f2.as_deref()), ("A1", Some("$G2")));
            s.execute("edit.undo", json!({})).unwrap();
            assert!(get(&mut s, "Input", "D4")["formula"].is_null());
            assert_eq!(s.doc().unwrap().wb.sheets[0].validations.len(), 1);
            let before = s.doc().unwrap().wb.clone();
            assert!(s.execute(TRANSFER, json!({"sheet":"Input","source":"B2:C2","at":"D4","operation":"move","keepReferences":true})).is_err());
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
        }
    }

    #[test]
    fn unrelated_hidden_workbook_names_are_never_disclosed_by_move_guard() {
        let mut s = setup();
        s.execute("cell.set", json!({"cell":"A1","input":"2"})).unwrap();
        s.execute("home.insertSheet", json!({"name":"Output"})).unwrap();
        std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb).names.push(gridcraft_model::DefinedName {
            name: "HiddenSentinel".into(),
            scope: None,
            formula: "$A$1".into(),
            comment: String::new(),
            hidden: true,
        });
        let before = s.doc().unwrap().wb.clone();
        let error = s
            .execute(TRANSFER, json!({"sheet":"Input","source":"A1","destinationSheet":"Output","at":"B1","operation":"move"}))
            .unwrap_err()
            .to_string();
        assert!(error.contains("caller-relative") && !error.contains("HiddenSentinel"));
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
    }

    #[test]
    fn moves_preserve_unmoved_callers_of_local_relative_names() {
        let mut s = setup();
        for (cell, input) in [("B1", "9"), ("D3", "11"), ("E3", "22")] {
            s.execute("cell.set", json!({"cell":cell,"input":input})).unwrap();
        }
        s.execute("formulas.defineName", json!({"name":"Relative","scope":"Input","refersTo":"B1"})).unwrap();
        s.execute("cell.set", json!({"cell":"C3","input":"=Relative"})).unwrap();
        s.execute("home.insertSheet", json!({"name":"Output"})).unwrap();
        for target in ["Input", "Output"] {
            let before = s.doc().unwrap().wb.clone();
            let request = json!({"sheet":"Input","source":"B1","at":"C1","destinationSheet":target,"operation":"move"});
            assert!(s.execute(TRANSFER, request).unwrap_err().to_string().contains("caller-relative"));
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
            assert_eq!(get(&mut s, "Input", "C3")["value"], 11.0);
        }
    }
    #[test]
    fn moved_name_bindings_and_untouched_callers_are_preserved_or_refused_before_editing() {
        for separate_destination_binding in [false, true] {
            let mut s = setup();
            s.execute("formulas.defineName", json!({"name":"Rate","scope":"Input","refersTo":"7"})).unwrap();
            s.execute("cell.set", json!({"cell":"A1","input":"=Rate"})).unwrap();
            s.execute("home.insertSheet", json!({"name":"Output"})).unwrap();
            if separate_destination_binding {
                s.execute("formulas.defineName", json!({"name":"Rate","scope":"Output","refersTo":"9"})).unwrap();
            }
            let before = s.doc().unwrap().wb.clone();
            let undo = s.doc().unwrap().undo.len();
            assert!(s.execute(TRANSFER, json!({"sheet":"Input","source":"A1","destinationSheet":"Output","at":"D1","operation":"move"})).is_err());
            assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
            assert_eq!(undo, s.doc().unwrap().undo.len());
            s.execute("formulas.defineName", json!({"name":"SafeRate","refersTo":"7"})).unwrap();
            s.execute("cell.set", json!({"sheet":"Input","cell":"A1","input":"=SafeRate"})).unwrap();
            s.execute("cell.set", json!({"cell":"B1","input":"=SafeRate"})).unwrap();
            s.execute(TRANSFER, json!({"sheet":"Input","source":"A1","destinationSheet":"Output","at":"D1","operation":"move"})).unwrap();
            assert_eq!(get(&mut s, "Output", "D1")["value"], 7.0);
            assert_eq!(get(&mut s, "Output", "B1")["value"], 7.0);
        }
    }

    #[test]
    fn approved_overwrite_removes_destination_only_sidecars_even_under_blank_source_cells() {
        for operation in ["copy", "move", "transpose"] {
            let mut s = setup();
            let c = |s: &str| CellRef::parse(s).unwrap();
            let (second, third) = if operation == "transpose" { ("C2", "C3") } else { ("D1", "E1") };
            s.execute("review.newNote", json!({"cell":"A1","text":"source"})).unwrap();
            s.execute("review.newNote", json!({"cell":second,"text":"old"})).unwrap();
            let sh = std::sync::Arc::make_mut(&mut s.active_mut().unwrap().wb).sheet_mut(0).unwrap();
            sh.hyperlinks.insert(c(third), gridcraft_model::features::Hyperlink { target: "Input!A1".into(), tooltip: None, external: false });
            // The origin and target overlap at C1; everything read from the source must
            // use the original, even when the destination also has independent sidecars.
            let original = sh.hyperlinks.clone();
            let notes = sh.comments.clone();
            s.execute(TRANSFER, json!({"sheet":"Input","source":"A1:C1","at":"C1","operation":operation,"overwrite":true})).unwrap();
            let sh = &s.doc().unwrap().wb.sheets[0];
            assert_eq!(sh.comments[&c("C1")].text, "source");
            assert!(!sh.comments.contains_key(&c(second)));
            assert!(!sh.hyperlinks.contains_key(&c(third)));
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(s.doc().unwrap().wb.sheets[0].comments, notes);
            assert_eq!(s.doc().unwrap().wb.sheets[0].hyperlinks, original);
        }
    }

    #[test]
    fn cross_sheet_cut_retargets_formulas_and_preserves_user_clipboard_and_selection() {
        let mut s = setup();
        s.execute("range.setValues", json!({"range":"A1","values":[[7,"=A1*2","=B1"]]})).unwrap();
        s.execute("review.newComment", json!({"cell":"A1","text":"kept with the block"})).unwrap();
        s.execute("home.insertSheet", json!({"name":"Result"})).unwrap();
        s.execute("edit.cut", json!({"sheet":"Input","range":"C1"})).unwrap();
        let before = s.doc().unwrap().selection.clone();
        let out = s.execute(TRANSFER, json!({"sheet":"Input","source":"A1:B1","at":"F3","destinationSheet":"Result","operation":"move"})).unwrap();
        assert_eq!(out["destination"], "F3:G3");
        assert_eq!(s.doc().unwrap().selection, before);
        assert!(s.clipboard.as_ref().unwrap().cut);
        assert_eq!(s.clipboard.as_ref().unwrap().range.a1(), "C1");
        assert_eq!(get(&mut s, "Result", "G3")["value"], json!(14.0));
        assert!(get(&mut s, "Input", "C1")["formula"].as_str().unwrap().contains("G3"));
        let c = CellRef::parse("F3").unwrap();
        assert!(s.doc().unwrap().wb.sheets[1].comments.contains_key(&c));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(get(&mut s, "Input", "A1")["value"], json!(7.0));
        assert_eq!(get(&mut s, "Input", "C1")["formula"], "=B1");
        assert!(!s.doc().unwrap().wb.sheets[1].comments.contains_key(&c));
    }

    #[test]
    fn column_transposition_preserves_gaps_and_refuses_occupied_tail_without_side_effects() {
        let mut s = setup();
        for (at, input) in [("A6", "north"), ("A8", "south"), ("D1", "existing"), ("Z999999", "unrelated")] {
            s.execute("cell.set", json!({"cell":at,"input":input})).unwrap();
        }
        let mut p = json!({"sheet":"Input","source":"A:A","at":"B1","operation":"transpose"});
        let before = s.doc().unwrap().wb.clone();
        assert!(s.execute(TRANSFER, p.clone()).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, &s.doc().unwrap().wb));
        p["overwrite"] = true.into();
        let out = s.execute(TRANSFER, p).unwrap();
        assert_eq!((out["source"].as_str(), out["destination"].as_str()), (Some("A6:A8"), Some("B1:D1")));
        assert_eq!(get(&mut s, "Input", "D1")["value"], "south");
        assert!(get(&mut s, "Input", "C1")["value"].is_null());
        assert_eq!(get(&mut s, "Input", "A8")["value"], "south");
    }

    #[test]
    fn reshaping_handles_quoted_delimiters_empty_fields_and_formula_looking_literals_in_one_step() {
        let mut s = setup();
        s.execute("cell.set", json!({"cell":"A2","input":"001,\"two,words\",,=B2"})).unwrap();
        s.execute("cell.set", json!({"cell":"A3","input":"\"multi\nline\",end"})).unwrap();
        let p = json!({"sheet":"Input","source":"A:A","at":"B2","delimiter":",","layout":"row"});
        let out = s.execute(RESHAPE, p).unwrap();
        assert_eq!(out["destination"], "B2:G2");
        for (a, v) in [("B2", "001"), ("C2", "two,words"), ("E2", "=B2"), ("F2", "multi\nline"), ("G2", "end")] {
            let cell = get(&mut s, "Input", a);
            assert_eq!(cell["value"], v);
            assert!(cell["formula"].is_null());
        }
        assert!(get(&mut s, "Input", "D2")["value"].is_null());
        s.execute("edit.undo", json!({})).unwrap();
        assert!(get(&mut s, "Input", "B2")["value"].is_null());
        assert!(get(&mut s, "Input", "G2")["value"].is_null());
    }

    #[test]
    fn ragged_reshape_clears_padding_and_destination_metadata_with_one_undo() {
        for layout in ["rows", "columns"] {
            let mut s = setup();
            s.execute("range.setValues", json!({"range":"A1","values":[["p,q"],["r"]]})).unwrap();
            s.execute("cell.set", json!({"cell":"C2","input":"stale"})).unwrap();
            s.execute("review.newNote", json!({"cell":"C2","text":"old note"})).unwrap();
            s.execute("review.newNote", json!({"cell":"B1","text":"also old"})).unwrap();
            let params = json!({"sheet":"Input","source":"A1:A2","at":"B1","layout":layout,"delimiter":",","overwrite":true});
            let response = s.execute(RESHAPE, params).unwrap();
            assert_eq!(response["destination"], "B1:C2");
            let sh = &s.doc().unwrap().wb.sheets[0];
            let c = CellRef::parse("C2").unwrap();
            assert!(sh.value(c).is_empty());
            assert!(!sh.comments.contains_key(&c));
            assert!(!sh.comments.contains_key(&CellRef::parse("B1").unwrap()));
            s.execute("edit.undo", json!({})).unwrap();
            assert_eq!(get(&mut s, "Input", "C2")["value"], "stale");
            assert_eq!(s.doc().unwrap().wb.sheets[0].comments[&c].text, "old note");
        }
    }
}
