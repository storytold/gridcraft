use super::clipboard::What;
use super::{
    Cell, CellRef, EngineError, Formula, Json, RangeRef, Result, Session, StyleId, Value, bad, bool_param, cell_param_on, edit, json, str_param,
    target_sheet,
};

fn tiled_cells(rows: u32, cols: u32, tiles_r: u32, tiles_c: u32) -> impl Iterator<Item = (u32, u32, u32, u32)> + Clone {
    (0..tiles_r).flat_map(move |tr| (0..tiles_c).flat_map(move |tc| (0..rows).flat_map(move |r| (0..cols).map(move |c| (tr, tc, r, c)))))
}

fn apply_operation(op: &str, old: &Cell, new: &mut Cell) {
    let operand = new.value.clone();
    if let (Ok(x), Ok(y)) = (old.value.to_number(), operand.to_number())
        && (old.value.is_number() || old.value.is_empty() || old.value.is_text())
        && (operand.is_number() || operand.is_empty())
    {
        new.value = match op {
            "add" => Value::number(x + y),
            "subtract" => Value::number(x - y),
            "multiply" => Value::number(x * y),
            "divide" if y == 0.0 => Value::Error(gridcraft_core::CellError::Div0),
            "divide" => Value::number(x / y),
            _ => operand.clone(),
        };
    } else if (old.value.is_text() || old.value.is_error()) && (operand.is_number() || operand.is_empty()) {
        new.value = old.value.clone();
    }
    if let Some(formula) = &old.formula
        && let Ok(y) = operand.to_number()
        && (operand.is_number() || operand.is_empty())
        && let Some(symbol) = operation_symbol(op)
    {
        let text = format!("=({}){symbol}{}", formula.text, gridcraft_core::number_to_text(y));
        new.formula = Some(std::sync::Arc::new(Formula::new(&text)));
        new.value = Value::Empty;
    }
}

fn operation_symbol(op: &str) -> Option<char> {
    match op {
        "add" => Some('+'),
        "subtract" => Some('-'),
        "multiply" => Some('*'),
        "divide" => Some('/'),
        _ => None,
    }
}

fn copy_merges(
    cx: &mut crate::cmd::Ctx,
    dest_sheet: usize,
    src_sheet: &gridcraft_model::Sheet,
    src: RangeRef,
    at: CellRef,
    what: What,
    transpose: bool,
) -> Result<()> {
    if !matches!(what, What::All | What::Formats) || transpose {
        return Ok(());
    }
    let merges: Vec<RangeRef> = src_sheet.merges.iter().filter(|merge| src.contains_range(merge)).copied().collect();
    for merge in merges {
        let row_offset = at.row as i64 - src.start.row as i64;
        let col_offset = at.col as i64 - src.start.col as i64;
        if let (Some(start), Some(end)) = (merge.start.offset(row_offset, col_offset), merge.end.offset(row_offset, col_offset)) {
            let moved = RangeRef::new(start, end);
            let sheet = cx.sheet_mut(dest_sheet)?;
            sheet.merges.retain(|existing| !existing.intersects(&moved));
            sheet.merges.push(moved);
        }
    }
    Ok(())
}

pub(super) fn paste_special(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(clip) = s.clipboard.clone() else { return Err(EngineError::Other("Nothing to paste.".into())) };
    let what = match str_param(p, "what").unwrap_or("all") {
        "all" => What::All,
        "formulas" => What::Formulas,
        "values" => What::Values,
        "formats" => What::Formats,
        "comments" | "notes" => What::Comments,
        "validation" => What::Validation,
        "allExceptBorders" => What::AllExceptBorders,
        "columnWidths" => What::ColumnWidths,
        "formulasAndNumberFormats" => What::FormulasAndNumberFormats,
        "valuesAndNumberFormats" => What::ValuesAndNumberFormats,
        other => return Err(bad("edit.pasteSpecial", format!("unknown `what`: {other}"))),
    };
    let op = str_param(p, "operation").unwrap_or("none").to_string();
    let skip_blanks = bool_param(p, "skipBlanks").unwrap_or(false);
    let transpose = bool_param(p, "transpose").unwrap_or(false);
    let link = bool_param(p, "link").unwrap_or(false);
    if link && !clip.areas.is_empty() {
        return Err(EngineError::Other("This action won't work on multiple selections.".into()));
    }
    let dest_sheet = target_sheet(s, p)?;
    let sel = s.doc()?.selection.current();
    let at = cell_param_on(s, p, "at", dest_sheet)?.unwrap_or(sel.start);
    let src = clip.range;
    let (h, w) = if transpose { (src.width(), src.height()) } else { (src.height(), src.width()) };
    // Tile the copy over a larger destination that is a multiple of its size.
    let tile = sel.height() % h == 0
        && sel.width() % w == 0
        && !sel.is_single()
        && cell_param_on(s, p, "at", dest_sheet)?.is_none()
        && !sel.is_full_cols()
        && !sel.is_full_rows()
        && sel.count() <= 4_000_000;
    let (tiles_r, tiles_c) = if tile { (sel.height() / h, sel.width() / w) } else { (1, 1) };
    let same_doc = s.doc()?.uid == clip.doc_uid;
    let cut_move = clip.cut && same_doc && what == What::All;
    let src_wb = clip.wb.clone();
    let Some(src_sheet) = src_wb.sheet(clip.sheet) else { return Err(EngineError::Other("The copied sheet no longer exists.".into())) };
    let src_sheet_name = src_sheet.name.clone();
    // A cut moves its block as it is, as Excel's does: it can't be transposed or tiled over a
    // larger selection (the move's references, rules and arrays follow the block unturned).
    if cut_move && (transpose || tiles_r > 1 || tiles_c > 1) {
        return Err(EngineError::Other("The cut area can only be pasted as it is: paste it without Transpose, into one cell.".into()));
    }
    if cut_move && at.offset((tiles_r * h).saturating_sub(1) as i64, (tiles_c * w).saturating_sub(1) as i64).is_none() {
        // Check the whole footprint before the source is emptied, so a move that overflows the grid loses nothing.
        return Err(EngineError::Other("The cut area doesn't fit at this position; nothing was moved.".into()));
    }
    edit(s, |cx| {
        // Styles from another workbook must be re-interned.
        let mut style_map = std::collections::HashMap::new();
        let mut map_style = |id: StyleId, wb: &mut gridcraft_model::Workbook| -> StyleId {
            if same_doc {
                return id;
            }
            *style_map.entry(id).or_insert_with(|| wb.styles.intern(src_wb.styles.get(id).clone()))
        };
        let mut moved_formulas = Vec::new();
        let mut moved_pictures = std::collections::BTreeMap::new();
        if cut_move {
            // Remove the source first (contents and formats), and fix references to it.
            let source = cx.sheet_mut(clip.sheet)?;
            moved_pictures = source.cell_pictures.iter().filter(|(c, _)| src.contains(**c)).map(|(c, p)| (*c, p.clone())).collect();
            let taken = source.take_cells(src);
            // Notes, hyperlinks and merged areas sit beside the cells, and a cut takes them
            // with the block: the destination is written from the clipboard snapshot taken
            // before the cut, so removing them here loses nothing and leaves the old place
            // clean instead of leaving one merged block and one note behind.
            source.comments.retain(|c, _| !src.contains(*c));
            source.hyperlinks.retain(|c, _| !src.contains(*c));
            source.merges.retain(|m| !src.contains_range(m));
            for (c, _) in &taken {
                cx.touch(clip.sheet, *c);
            }
            moved_formulas = taken;
        }
        let dest_name = cx.wb.sheet(dest_sheet).map(|sh| sh.name.clone()).unwrap_or_default();
        for (tr, tc, r, c) in tiled_cells(src.height(), src.width(), tiles_r, tiles_c) {
            let sc = CellRef::new(src.start.row + r, src.start.col + c);
            let (dr, dc) = if transpose { (c, r) } else { (r, c) };
            let Some(dest) = at.offset((tr * h + dr) as i64, (tc * w + dc) as i64) else { continue };
            let src_cell = if cut_move { moved_formulas.iter().find(|(k, _)| *k == sc).map(|(_, v)| v.clone()) } else { src_sheet.cell(sc).cloned() };
            let src_val = src_sheet.value(sc);
            let src_picture = if cut_move { moved_pictures.get(&sc) } else { src_sheet.cell_pictures.get(&sc) }.cloned();
            if skip_blanks && src_picture.is_none() && src_val.is_empty() && src_cell.as_ref().is_none_or(|x| x.formula.is_none() && x.py.is_none()) {
                continue;
            }
            let old = cx.wb.sheet(dest_sheet).and_then(|sh| sh.cell(dest)).cloned().unwrap_or_default();
            let old_picture = cx.wb.sheet(dest_sheet).and_then(|sh| sh.cell_pictures.get(&dest)).cloned();
            let mut new_picture = old_picture.clone();
            let mut new = old.clone();
            let shift = |cell: &Cell| -> Option<std::sync::Arc<Formula>> {
                let f = cell.formula.as_ref()?;
                let e = f.expr()?;
                if cut_move {
                    // A moved formula still reads cells on its original sheet. Qualify
                    // those references before the workbook-wide move adjustment below.
                    let e = if dest_sheet != clip.sheet {
                        e.map(&mut |x| match x {
                            gridcraft_formula::Expr::Ref(mut r) if r.sheet == gridcraft_formula::SheetSel::Current => {
                                r.sheet = gridcraft_formula::SheetSel::Named(src_sheet_name.clone());
                                gridcraft_formula::Expr::Ref(r)
                            }
                            other => other,
                        })
                    } else {
                        e
                    };
                    let mut moved = Formula::from_expr(e);
                    // The array range moves with its formula.
                    let (dr, dc) = (dest.row as i64 - sc.row as i64, dest.col as i64 - sc.col as i64);
                    moved.array = f.array.and_then(|a| Some(RangeRef::new(a.start.offset(dr, dc)?, a.end.offset(dr, dc)?)));
                    moved.dynamic = f.dynamic;
                    return Some(std::sync::Arc::new(moved));
                }
                let moved = gridcraft_formula::adjust::shift_relative(e, dest.row as i64 - sc.row as i64, dest.col as i64 - sc.col as i64);
                Some(std::sync::Arc::new(Formula { dynamic: f.dynamic, ..Formula::from_expr(moved) }))
            };
            // A Python cell's code travels with it. Cut to another sheet, it keeps reading the
            // cells its `xl()` references named on its original sheet, as a moved formula does:
            // they are qualified with that sheet before the workbook-wide move adjustment below.
            let moved_py = |cell: &Cell| -> Result<Option<std::sync::Arc<gridcraft_model::PyCode>>> {
                let Some(py) = cell.py.clone() else { return Ok(None) };
                if !cut_move || dest_sheet == clip.sheet {
                    return Ok(Some(py));
                }
                let qualified = gridcraft_model::python::rewrite_xl(&py.code, |reference| {
                    let e = gridcraft_formula::parse(reference).ok()?.map(&mut |x| match x {
                        gridcraft_formula::Expr::Ref(mut r) if r.sheet == gridcraft_formula::SheetSel::Current => {
                            r.sheet = gridcraft_formula::SheetSel::Named(src_sheet_name.clone());
                            gridcraft_formula::Expr::Ref(r)
                        }
                        other => other,
                    });
                    Some(gridcraft_formula::print(&e))
                });
                Ok(Some(match qualified {
                    Some(code) => {
                        let moved = gridcraft_model::PyCode { code, ..(*py).clone() };
                        check_python_fits(&moved, &dest_name, dest)?;
                        std::sync::Arc::new(moved)
                    }
                    None => py,
                }))
            };
            if link {
                let sheet_prefix = if dest_sheet == clip.sheet && same_doc {
                    String::new()
                } else {
                    format!("{}!", gridcraft_formula::quote_sheet(&src_sheet_name))
                };
                new.formula = Some(std::sync::Arc::new(Formula::new(&format!("={sheet_prefix}{}", sc.a1()))));
                new.value = Value::Empty;
                new.py = None;
                new_picture = None;
            } else {
                let sc_cell = src_cell.clone().unwrap_or_default();
                match what {
                    What::All | What::AllExceptBorders => {
                        new_picture = src_picture.clone();
                        new.formula = shift(&sc_cell);
                        new.value = if new.formula.is_some() { Value::Empty } else { sc_cell.value.clone() };
                        new.py = moved_py(&sc_cell)?;
                        let st = map_style(sc_cell.style, &mut cx.wb);
                        new.style = if what == What::AllExceptBorders {
                            let borders = cx.wb.styles.get(old.style).borders;
                            cx.wb.styles.derive(st, |s| s.borders = borders)
                        } else {
                            st
                        };
                    }
                    What::Formulas | What::FormulasAndNumberFormats => {
                        new_picture = src_picture.clone();
                        new.formula = shift(&sc_cell);
                        new.value = if new.formula.is_some() { Value::Empty } else { sc_cell.value.clone() };
                        new.py = moved_py(&sc_cell)?;
                        if what == What::FormulasAndNumberFormats {
                            let nf = src_wb.styles.get(sc_cell.style).num_fmt.clone();
                            new.style = cx.wb.styles.derive(old.style, |s| s.num_fmt = nf);
                        }
                    }
                    What::Values | What::ValuesAndNumberFormats => {
                        new_picture = src_picture.clone();
                        new.formula = None;
                        new.py = None;
                        new.value = src_val.clone();
                        if what == What::ValuesAndNumberFormats {
                            let nf = src_wb.styles.get(sc_cell.style).num_fmt.clone();
                            new.style = cx.wb.styles.derive(old.style, |s| s.num_fmt = nf);
                        }
                    }
                    What::Formats => new.style = map_style(sc_cell.style, &mut cx.wb),
                    What::Comments | What::Validation | What::ColumnWidths => {}
                }
                // Operations combine numbers with what is already there.
                if op != "none"
                    && matches!(what, What::All | What::Values | What::ValuesAndNumberFormats | What::Formulas)
                    && new.formula.is_none()
                    && new.py.is_none()
                {
                    if old_picture.is_some() || new_picture.is_some() {
                        new_picture = None;
                        new.value = Value::Error(gridcraft_core::CellError::Value);
                    }
                    // Numbers stored as text participate; other text and errors stay as they are.
                    apply_operation(&op, &old, &mut new);
                }
            }
            // The copied cell's validation rule replaces whatever rule the destination had.
            if !link && !cut_move && matches!(what, What::All | What::AllExceptBorders | What::Validation) {
                let rule = src_sheet.validations.iter().find(|dv| dv.ranges.iter().any(|r| r.contains(sc))).cloned();
                let sh = cx.sheet_mut(dest_sheet)?;
                crate::cmd::data::remove_validation_cells(&mut sh.validations, &[RangeRef::new(dest, dest)]);
                if let Some(mut dv) = rule {
                    // Relative references move with the rule, as pasted formulas do.
                    let first = RangeRef::top_left(&dv.ranges).unwrap_or(sc);
                    dv.f1 = crate::cmd::data::move_rule_formula(&dv.f1, first, dest);
                    dv.f2 = dv.f2.map(|f| crate::cmd::data::move_rule_formula(&f, first, dest));
                    dv.ranges = vec![RangeRef::new(dest, dest)];
                    sh.validations.push(dv);
                }
            }
            match what {
                What::Comments => {
                    if let Some(cm) = src_sheet.comments.get(&sc).cloned() {
                        cx.sheet_mut(dest_sheet)?.comments.insert(dest, cm);
                    }
                }
                What::ColumnWidths => {
                    if let Some(info) = src_sheet.cols.get(&sc.col).copied() {
                        cx.sheet_mut(dest_sheet)?.cols.insert(dest.col, info);
                    }
                }
                What::All => {
                    if let Some(cm) = src_sheet.comments.get(&sc).cloned() {
                        cx.sheet_mut(dest_sheet)?.comments.insert(dest, cm);
                    }
                    if let Some(h) = src_sheet.hyperlinks.get(&sc).cloned() {
                        cx.sheet_mut(dest_sheet)?.hyperlinks.insert(dest, h);
                    }
                    cx.sheet_mut(dest_sheet)?.set_cell(dest, new);
                }
                _ => cx.sheet_mut(dest_sheet)?.set_cell(dest, new),
            }
            if let Some(picture) = new_picture {
                cx.sheet_mut(dest_sheet)?.cell_pictures.insert(dest, picture);
            }
            cx.touch(dest_sheet, dest);
        }
        // Merged areas inside the copied block travel with it.
        copy_merges(cx, dest_sheet, src_sheet, src, at, what, transpose)?;
        if cut_move {
            move_cut_rules(cx, src_sheet, (clip.sheet, dest_sheet), src, at)?;
            // References elsewhere to the moved block now point to its new place.
            let dest_name = cx.wb.sheet(dest_sheet).map(|s| s.name.clone()).unwrap_or_default();
            let e = if dest_sheet == clip.sheet {
                gridcraft_formula::adjust::Edit::Move { from: src, to_row: at.row, to_col: at.col }
            } else {
                gridcraft_formula::adjust::Edit::MoveToSheet { from: src, to_sheet: dest_name, to_row: at.row, to_col: at.col }
            };
            rewrite_all_formulas(&mut cx.wb, &src_sheet_name, &e)?;
            cx.structural = true;
        }
        let end = at.offset_clamped((tiles_r * h) as i64 - 1, (tiles_c * w) as i64 - 1);
        *cx.sel = crate::Selection { active: at, anchor: at, ranges: vec![RangeRef::new(at, end)] };
        Ok(())
    })?;
    // Formulas and validation from an untrusted file leave the destination untrusted; plain values,
    // formats (cell styles and merged areas, no conditional formats), column widths and notes
    // carry nothing active. A linked paste writes a formula into every cell whatever `what` says,
    // except for column widths and notes, which write no cells at all.
    let passive =
        matches!(what, What::ColumnWidths | What::Comments) || (!link && matches!(what, What::Formats | What::Values | What::ValuesAndNumberFormats));
    if !same_doc && !passive {
        let d = s.doc_mut()?;
        d.trust = d.trust.receive(clip.trust);
    }
    if cut_move {
        s.clipboard = None;
    }
    Ok(json!({"pasted": s.doc()?.selection.a1()}))
}

/// A cut takes its cells' conditional formats and validation with it, from sheet `from` to the
/// block at `at` on sheet `to`, replacing the rules there. The moved parts' formulas are handled
/// as the cut cells' are: kept written for where their cells were (on another sheet, with
/// their references to it named), so the reference rewrite for the move shifts those into the
/// block once. `snapshot` is the source sheet as it was cut.
fn move_cut_rules(cx: &mut crate::cmd::Ctx, snapshot: &gridcraft_model::Sheet, (from, to): (usize, usize), src: RangeRef, at: CellRef) -> Result<()> {
    use crate::cmd::data::Rule;
    let mut cfs = crate::cmd::data::moved_rule_parts(&snapshot.cond_formats, src, at);
    let mut dvs = crate::cmd::data::moved_rule_parts(&snapshot.validations, src, at);
    if from != to {
        let name = |t: &mut String| {
            if let Ok(e) = gridcraft_formula::parse(t.trim_start_matches('=')) {
                let e = e.map(&mut |x| match x {
                    gridcraft_formula::Expr::Ref(mut r) if r.sheet == gridcraft_formula::SheetSel::Current => {
                        r.sheet = gridcraft_formula::SheetSel::Named(snapshot.name.clone());
                        gridcraft_formula::Expr::Ref(r)
                    }
                    other => other,
                });
                *t = format!("{}{}", if t.starts_with('=') { "=" } else { "" }, gridcraft_formula::print(&e));
            }
        };
        cfs.iter_mut().flat_map(|r| r.parts().1).for_each(name);
        dvs.iter_mut().flat_map(|r| r.parts().1).for_each(name);
    }
    let source = cx.sheet_mut(from)?;
    crate::cmd::data::remove_rule_cells(&mut source.cond_formats, &[src]);
    crate::cmd::data::remove_rule_cells(&mut source.validations, &[src]);
    let block = RangeRef::new(at, at.offset_clamped(i64::from(src.height()) - 1, i64::from(src.width()) - 1));
    let dest = cx.sheet_mut(to)?;
    crate::cmd::data::remove_rule_cells(&mut dest.cond_formats, &[block]);
    crate::cmd::data::remove_rule_cells(&mut dest.validations, &[block]);
    dest.cond_formats.extend(cfs);
    dest.validations.extend(dvs);
    Ok(())
}

/// Applies a reference adjustment to every reference the workbook holds
/// ([`Workbook::rewrite_references`]: formulas, names, conditional formats, validation,
/// charts, sparklines, PivotTable sources) and its Python cells, and moves the legacy array
/// ranges on `target`.
///
/// [`Workbook::rewrite_references`]: gridcraft_model::Workbook::rewrite_references
pub(crate) fn rewrite_all_formulas(wb: &mut gridcraft_model::Workbook, target: &str, e: &gridcraft_formula::adjust::Edit) -> Result<()> {
    use gridcraft_formula::adjust::Edit;
    wb.rewrite_references(&|host, expr| gridcraft_formula::adjust::adjust(expr, host.name, target, e));
    // Rows and columns inserted or deleted on the target sheet move its legacy array ranges too
    // (a cut moves its arrays itself).
    if matches!(e, Edit::Insert { .. } | Edit::Delete { .. } | Edit::InsertCells { .. } | Edit::DeleteCells { .. })
        && let Some(si) = wb.sheet_index(target)
        && let Some(sheet) = wb.sheet_mut(si)
    {
        let arrays: Vec<(CellRef, std::sync::Arc<Formula>)> =
            sheet.cells.iter().filter_map(|(c, cell)| cell.formula.clone().filter(|f| f.array.is_some()).map(|f| (c, f))).collect();
        for (c, f) in arrays {
            let array = f.array.and_then(|a| crate::cmd::sheet::map_range(a, e));
            if array != f.array
                && let Some(cell) = sheet.cells.get_mut(c)
            {
                cell.formula = Some(std::sync::Arc::new(Formula { array, ..(*f).clone() }));
            }
        }
    }
    rewrite_python_refs(wb, &|host, expr| gridcraft_formula::adjust::adjust(expr, host, target, e))
}

/// Rewrites the `xl("…")` literals in every Python cell with `f`, which maps a reference held on
/// sheet `host` to its new form. Every reference-moving edit (insert and delete of lines or cells,
/// cut-move, sheet rename and delete) runs it, as it adjusts formulas. A literal is only replaced
/// when the reference it names changed, and text that doesn't parse is kept. A cell's cached
/// result is kept while its references still name the same data (a rename, a move); a reference
/// the edit deleted (`#REF!`), or one that grew or shrank (rows or columns inserted or deleted
/// inside it), leaves the result stale, so the cell is new code (`#BLOCKED!`) and the
/// recalculation removes its spill. A rewrite that would
/// make a cell's code longer than one cell holds (a longer sheet name, `Z1` becoming `AA1`) refuses
/// the whole edit, which then changes nothing: see [`check_python_fits`].
pub(crate) fn rewrite_python_refs(
    wb: &mut gridcraft_model::Workbook,
    f: &impl Fn(&str, gridcraft_formula::Expr) -> gridcraft_formula::Expr,
) -> Result<()> {
    for si in 0..wb.sheets.len() {
        let Some(sh) = wb.sheets.get(si) else { continue };
        let host = sh.name.clone();
        let mut new_code: Vec<(CellRef, String, bool)> = Vec::new();
        for (c, cell) in sh.cells.iter() {
            let Some(py) = &cell.py else { continue };
            let mut lost = false;
            let changed = gridcraft_model::python::rewrite_xl(&py.code, |reference| {
                let expr = gridcraft_formula::parse(reference).ok()?;
                let before = gridcraft_formula::print(&expr);
                let after = gridcraft_formula::print(&f(&host, expr));
                lost |= (after.contains("#REF!") && !before.contains("#REF!")) || area_shape(&before) != area_shape(&after);
                (after != before).then_some(after)
            });
            if let Some(code) = changed {
                check_python_fits(&gridcraft_model::PyCode { code: code.clone(), object: py.object, ..Default::default() }, &host, c)?;
                new_code.push((c, code, lost));
            }
        }
        if new_code.is_empty() {
            continue;
        }
        let Some(sh) = wb.sheet_mut(si) else { continue };
        for (c, code, lost) in new_code {
            if let Some(cell) = sh.cells.get_mut(c)
                && let Some(py) = cell.py.as_mut()
            {
                let py = std::sync::Arc::make_mut(py);
                py.code = code;
                if lost {
                    py.output = None;
                    cell.value = Value::Error(gridcraft_core::CellError::Blocked);
                }
            }
        }
    }
    Ok(())
}

/// The rows and columns a printed reference covers (`A1:B3`, `Sheet2!$A$1`, `C:E`), or `None`
/// for any other form (a name, an error).
fn area_shape(reference: &str) -> Option<(u32, u32)> {
    let r = RangeRef::parse(reference.rsplit('!').next()?)?;
    Some((r.height(), r.width()))
}

/// Refuses Python code an edit has made longer than one cell holds: the limit `python.set`
/// enforces and opening a file applies (code over it is dropped to its value on open). The edit
/// is refused rather than made, so no code is lost and none keeps a reference the edit moved.
pub(crate) fn check_python_fits(py: &gridcraft_model::PyCode, sheet: &str, at: CellRef) -> Result<()> {
    if py.fits() {
        return Ok(());
    }
    Err(EngineError::Other(format!(
        "The edit would make the Python code in {}!{} longer than Excel's {}-character formula limit, so nothing was changed. Shorten the code first.",
        gridcraft_formula::quote_sheet(sheet),
        at.a1(),
        gridcraft_model::python::MAX_FORMULA_CHARS
    )))
}
