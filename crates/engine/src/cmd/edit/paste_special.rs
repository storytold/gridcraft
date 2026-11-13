use super::clipboard::What;
use super::{
    Cell, CellRef, EngineError, Formula, Json, RangeRef, Result, Session, StyleId, Value, bad, bool_param, cell_param_on, edit, json, str_param,
    target_sheet,
};

#[allow(clippy::cognitive_complexity)] // The command applies the selected paste mode in one atomic edit.
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
    // Whole columns or rows paste whole (the copy keeps only their used part as its text), so
    // what the destination columns hold further down is replaced too, as in Excel. The cells are
    // visited sparsely below, so this costs what the columns hold, not 1,048,576 rows.
    let whole = clip.areas.is_empty() && (clip.selected.is_full_cols() || clip.selected.is_full_rows()) && clip.selected.contains_range(&clip.range);
    let src = if whole { clip.selected } else { clip.range };
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
    // A single cell or row repeated down a filtered selection skips the rows the filter hides; a
    // taller block is pasted straight down, hidden rows included, as in Excel.
    let skip_rows = match s.doc()?.wb.sheet(dest_sheet) {
        Some(sh) if tile && h == 1 => super::filtered_rows(sh, sel),
        _ => Default::default(),
    };
    // Excel refuses a paste that would run off the sheet (whole columns pasted below row 1, whole
    // rows right of column A, a whole column transposed) instead of pasting part of it.
    if at.offset(i64::from(tiles_r * h) - 1, i64::from(tiles_c * w) - 1).is_none() {
        return Err(EngineError::Other("The copied cells don't fit here: paste them where the whole copied area fits on the sheet.".into()));
    }
    // Paste Link writes a formula into every cell, blank ones included, so it is the one paste
    // whose work grows with the area rather than with what the source holds.
    let link_src = clip.range;
    if link && link_src.count().saturating_mul(u64::from(tiles_r) * u64::from(tiles_c)) > MAX_PASTE_LINK_CELLS {
        return Err(EngineError::Other("Too many cells to paste as links at once.".into()));
    }
    let same_doc = s.doc()?.uid == clip.doc_uid;
    let cut_move = clip.cut && same_doc && what == What::All;
    let src_wb = clip.wb.clone();
    let Some(src_sheet) = src_wb.sheet(clip.sheet) else { return Err(EngineError::Other("The copied sheet no longer exists.".into())) };
    let src_sheet_name = src_sheet.name.clone();
    edit(s, |cx| {
        // Styles from another workbook must be re-interned.
        let dest_name = cx.wb.sheet(dest_sheet).map(|sh| sh.name.clone()).unwrap_or_default();
        let mut style_map = std::collections::HashMap::new();
        let mut map_style = |id: StyleId, wb: &mut gridcraft_model::Workbook| -> StyleId {
            if same_doc {
                return id;
            }
            *style_map.entry(id).or_insert_with(|| wb.styles.intern(src_wb.styles.get(id).clone()))
        };
        let mut moved_formulas = std::collections::BTreeMap::new();
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
            moved_formulas = taken.into_iter().collect();
        }
        // The cells of the copied block that hold something (or that a link reads), as offsets
        // inside it. Blank source cells matter only where the destination holds something.
        let source_offsets: std::collections::BTreeSet<(u32, u32)> = if link {
            (0..link_src.height()).flat_map(|r| (0..link_src.width()).map(move |c| (r, c))).collect()
        } else {
            let offset = |c: CellRef| (c.row - src.start.row, c.col - src.start.col);
            let mut held: std::collections::BTreeSet<(u32, u32)> = src_sheet.cells.iter_range(src).map(|(c, _)| offset(c)).collect();
            let side =
                src_sheet.spill.keys().chain(src_sheet.cell_pictures.keys()).chain(src_sheet.comments.keys()).chain(src_sheet.hyperlinks.keys());
            held.extend(side.chain(moved_formulas.keys()).chain(moved_pictures.keys()).filter(|c| src.contains(**c)).map(|c| offset(*c)));
            held
        };
        let skip_cells = matches!(what, What::Validation | What::ColumnWidths);
        // A blank source cell clears (or reformats) a destination cell that holds something, so
        // those cells join the visit. One scan of the whole pasted area sorts them by tile.
        let mut dest_held: std::collections::BTreeMap<(u32, u32), Vec<(u32, u32)>> = Default::default();
        if !link && !skip_cells {
            let pasted = RangeRef::new(at, at.offset_clamped(i64::from(tiles_r * h) - 1, i64::from(tiles_c * w) - 1));
            let held =
                cx.wb.sheet(dest_sheet).into_iter().flat_map(|sh| {
                    sh.cells.iter_range(pasted).map(|(c, _)| c).chain(sh.cell_pictures.keys().copied().filter(|c| pasted.contains(*c)))
                });
            for d in held.collect::<Vec<_>>() {
                let (dr, dc) = (d.row - at.row, d.col - at.col);
                let local = (dr % h, dc % w);
                dest_held.entry((dr / h, dc / w)).or_default().push(if transpose { (local.1, local.0) } else { local });
            }
        }
        let no_cells = std::collections::BTreeSet::new();
        for tr in 0..tiles_r {
            for tc in 0..tiles_c {
                let merged;
                let offsets = match dest_held.get(&(tr, tc)) {
                    Some(extra) => {
                        merged = source_offsets.iter().chain(extra).copied().collect::<std::collections::BTreeSet<_>>();
                        &merged
                    }
                    None if skip_cells => &no_cells,
                    None => &source_offsets,
                };
                for &(r, c) in offsets {
                    let sc = CellRef::new(src.start.row + r, src.start.col + c);
                    let (dr, dc) = if transpose { (c, r) } else { (r, c) };
                    let Some(dest) = at.offset((tr * h + dr) as i64, (tc * w + dc) as i64) else { continue };
                    if skip_rows.contains(&dest.row) {
                        continue;
                    }
                    let src_cell = if cut_move { moved_formulas.get(&sc).cloned() } else { src_sheet.cell(sc).cloned() };
                    let src_val = src_sheet.value(sc);
                    let src_picture = if cut_move { moved_pictures.get(&sc) } else { src_sheet.cell_pictures.get(&sc) }.cloned();
                    if skip_blanks
                        && src_picture.is_none()
                        && src_val.is_empty()
                        && src_cell.as_ref().is_none_or(|x| x.formula.is_none() && x.py.is_none())
                    {
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
                            let a = old.value.clone();
                            let b = new.value.clone();
                            // Numbers stored as text take part (multiplying by 1 turns them into
                            // numbers); other text and errors stay as they are.
                            let (sys, region) = (cx.wb.date_system, cx.wb.region);
                            if let (Ok(x), Ok(y)) = (a.to_number_region(sys, region), b.to_number_region(sys, region))
                                && (a.is_number() || a.is_empty() || a.is_text())
                                && (b.is_number() || b.is_empty())
                            {
                                new.value = match op.as_str() {
                                    "add" => Value::number(x + y),
                                    "subtract" => Value::number(x - y),
                                    "multiply" => Value::number(x * y),
                                    "divide" => {
                                        if y == 0.0 {
                                            Value::Error(gridcraft_core::CellError::Div0)
                                        } else {
                                            Value::number(x / y)
                                        }
                                    }
                                    _ => b.clone(),
                                };
                            } else if (a.is_text() || a.is_error()) && (b.is_number() || b.is_empty()) {
                                new.value = a;
                            }
                            // A formula already there is kept and combined with the pasted number.
                            if let Some(f) = &old.formula
                                && let Ok(y) = b.to_number_region(sys, region)
                                && (b.is_number() || b.is_empty())
                                && let Some(sym) = match op.as_str() {
                                    "add" => Some('+'),
                                    "subtract" => Some('-'),
                                    "multiply" => Some('*'),
                                    "divide" => Some('/'),
                                    _ => None,
                                }
                            {
                                let text = format!("=({}){sym}{}", f.text, gridcraft_core::number_to_text(y));
                                new.formula = Some(std::sync::Arc::new(Formula::new(&text)));
                                new.value = Value::Empty;
                            }
                        }
                    }
                    match what {
                        What::Comments => {
                            if let Some(cm) = src_sheet.comments.get(&sc).cloned() {
                                cx.sheet_mut(dest_sheet)?.comments.insert(dest, cm);
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
            }
        }
        let pasted = RangeRef::new(at, at.offset_clamped(i64::from(tiles_r * h) - 1, i64::from(tiles_c * w) - 1));
        if !link && matches!(what, What::All | What::AllExceptBorders | What::Validation) {
            paste_validations(cx, src_sheet, src, dest_sheet, pasted, PasteMode { transpose, skip_blanks, skip_rows: &skip_rows })?;
        }
        if what == What::ColumnWidths {
            // Each pasted column takes the width of the column copied to it (transposed, the
            // copied rows land in columns and all take the last copied column's width).
            let cols = cx.sheet_mut(dest_sheet)?;
            for dc in 0..pasted.width() {
                let info = if transpose {
                    (0..src.width()).rev().find_map(|c| src_sheet.cols.get(&(src.start.col + c)).copied())
                } else {
                    src_sheet.cols.get(&(src.start.col + dc % w)).copied()
                };
                if let Some(info) = info {
                    cols.cols.insert(pasted.start.col + dc, info);
                }
            }
        }
        // Merged areas inside the copied block travel with it.
        if matches!(what, What::All | What::Formats) && !transpose {
            let merges: Vec<RangeRef> = src_sheet.merges.iter().filter(|m| src.contains_range(m)).copied().collect();
            for m in merges {
                let dr = at.row as i64 - src.start.row as i64;
                let dc = at.col as i64 - src.start.col as i64;
                if let (Some(a), Some(b)) = (m.start.offset(dr, dc), m.end.offset(dr, dc)) {
                    let nm = RangeRef::new(a, b);
                    let sh = cx.sheet_mut(dest_sheet)?;
                    sh.merges.retain(|x| !x.intersects(&nm));
                    sh.merges.push(nm);
                }
            }
        }
        if cut_move {
            // References elsewhere to the moved block now point to its new place.
            let e = if dest_sheet == clip.sheet {
                gridcraft_formula::adjust::Edit::Move { from: src, to_row: at.row, to_col: at.col }
            } else {
                gridcraft_formula::adjust::Edit::MoveToSheet { from: src, to_sheet: dest_name.clone(), to_row: at.row, to_col: at.col }
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

/// Most cells one Paste Link writes (it writes a formula into every cell of the copied area,
/// blank ones included).
const MAX_PASTE_LINK_CELLS: u64 = 1_048_576;
/// Most validation ranges one paste adds (a rule covering part of a small block tiled over a huge
/// selection makes one range per tile).
const MAX_PASTE_VALIDATION_RANGES: usize = 100_000;

/// How a validation paste lays the rules out: transposed or not, keeping the destination's rule under
/// blank source cells, and leaving the rows a filter hides as they are.
#[derive(Clone, Copy)]
struct PasteMode<'a> {
    transpose: bool,
    skip_blanks: bool,
    skip_rows: &'a std::collections::BTreeSet<u32>,
}

/// `range` less the rows in `skip`: the runs of rows between them, as ranges of its columns.
fn without_rows(range: RangeRef, skip: &std::collections::BTreeSet<u32>) -> Vec<RangeRef> {
    let mut out = Vec::new();
    let mut next = range.start.row;
    for &row in skip.range(range.start.row..=range.end.row) {
        if row > next {
            out.push(RangeRef::new(CellRef::new(next, range.start.col), CellRef::new(row - 1, range.end.col)));
        }
        next = row + 1;
    }
    if next <= range.end.row {
        out.push(RangeRef::new(CellRef::new(next, range.start.col), CellRef::new(range.end.row, range.end.col)));
    }
    out
}

/// Pasting validation works on ranges, as Excel stores it: the pasted rectangle is cut out of the
/// destination's rules (cells whose source had no rule end up with none), then each source rule
/// that covers part of the copied block `src` comes along as one rule over the translated parts.
/// `size` is one tile's (height, width) at the destination; `pasted` is all the tiles together.
fn paste_validations(
    cx: &mut crate::cmd::Ctx<'_>,
    src_sheet: &gridcraft_model::Sheet,
    src: RangeRef,
    dest_sheet: usize,
    pasted: RangeRef,
    mode: PasteMode<'_>,
) -> Result<()> {
    let PasteMode { transpose, skip_blanks, skip_rows } = mode;
    let (h, w) = if transpose { (src.width(), src.height()) } else { (src.height(), src.width()) };
    let (tiles_r, tiles_c) = (pasted.height() / h.max(1), pasted.width() / w.max(1));
    let mut rules = Vec::new();
    // Under Skip Blanks the destination keeps its rule below a blank source cell that has none, so
    // only the destination of cells that hold something (and of the rules) is cleared. Nothing is
    // collected when no destination rule is under the paste.
    let mut cleared: Vec<RangeRef> = Vec::new();
    let dest_rules =
        cx.wb.sheet(dest_sheet).is_some_and(|sh| sh.validations.iter().any(|dv| dv.ranges.iter().any(|r| r.intersection(&pasted).is_some())));
    if skip_blanks && dest_rules {
        let mut cells = Vec::new();
        for (sc, cell) in src_sheet.cells.iter_range(src) {
            if cell.formula.is_none() && cell.py.is_none() && src_sheet.value(sc).is_empty() {
                continue;
            }
            let (r, c) = (sc.row - src.start.row, sc.col - src.start.col);
            let (r, c) = if transpose { (c, r) } else { (r, c) };
            for tr in 0..tiles_r {
                for tc in 0..tiles_c {
                    if let Some(d) = pasted.start.offset(i64::from(tr * h + r), i64::from(tc * w + c))
                        && !skip_rows.contains(&d.row)
                    {
                        cells.push((d.row, d.col));
                    }
                }
            }
        }
        // Coalesce populated cells into horizontal runs, then join identical runs in adjacent
        // rows. The range limit applies to the cuts, not the number of populated cells.
        cells.sort_unstable();
        let mut runs: Vec<RangeRef> = Vec::new();
        let mut i = 0;
        while i < cells.len() {
            let (row, start_col) = cells[i];
            let mut end_col = start_col;
            i += 1;
            while i < cells.len() && cells[i].0 == row && cells[i].1 <= end_col.saturating_add(1) {
                end_col = end_col.max(cells[i].1);
                i += 1;
            }
            runs.push(RangeRef::new(CellRef::new(row, start_col), CellRef::new(row, end_col)));
        }
        let mut last_run_by_columns = std::collections::BTreeMap::<(u32, u32), usize>::new();
        for run in runs {
            let columns = (run.start.col, run.end.col);
            if let Some(&index) = last_run_by_columns.get(&columns)
                && cleared[index].end.row.saturating_add(1) == run.start.row
            {
                cleared[index].end.row = run.end.row;
            } else {
                let index = cleared.len();
                cleared.push(run);
                last_run_by_columns.insert(columns, index);
            }
        }
    }
    let mut total = 0usize;
    for dv in &src_sheet.validations {
        let parts: Vec<RangeRef> = dv.ranges.iter().filter_map(|r| r.intersection(&src)).collect();
        let Some(first) = dv.ranges.first().map(|r| r.start) else { continue };
        if parts.is_empty() {
            continue;
        }
        let mut ranges = Vec::new();
        if parts.contains(&src) {
            ranges.push(pasted);
        } else {
            total = total.saturating_add(parts.len().saturating_mul(tiles_r as usize).saturating_mul(tiles_c as usize));
            if total > MAX_PASTE_VALIDATION_RANGES {
                return Err(EngineError::Other("Too many separate validation areas to paste at once.".into()));
            }
            for tr in 0..tiles_r {
                for tc in 0..tiles_c {
                    let Some(origin) = pasted.start.offset(i64::from(tr * h), i64::from(tc * w)) else { continue };
                    for p in &parts {
                        let (r0, c0) = (p.start.row - src.start.row, p.start.col - src.start.col);
                        let (r1, c1) = (p.end.row - src.start.row, p.end.col - src.start.col);
                        let ((r0, c0), (r1, c1)) = if transpose { ((c0, r0), (c1, r1)) } else { ((r0, c0), (r1, c1)) };
                        let Some(a) = origin.offset(i64::from(r0), i64::from(c0)) else { continue };
                        let b = origin.offset_clamped(i64::from(r1), i64::from(c1));
                        ranges.push(RangeRef::new(a, b));
                    }
                }
            }
        }
        // The rows a filter hides keep what they have.
        let ranges: Vec<RangeRef> = ranges.into_iter().flat_map(|r| without_rows(r, skip_rows)).collect();
        let Some(anchor) = ranges.first().map(|r| r.start) else { continue };
        let mut rule = dv.clone();
        // Relative references move with the rule, as pasted formulas do: they are written for the
        // rule's first cell.
        rule.f1 = super::super::data::move_rule_formula(&dv.f1, first, anchor);
        rule.f2 = dv.f2.as_ref().map(|f| super::super::data::move_rule_formula(f, first, anchor));
        rule.ranges = ranges;
        rules.push(rule);
    }
    if cleared.len() > MAX_PASTE_VALIDATION_RANGES {
        return Err(EngineError::Other("Too many separate validation areas to paste at once.".into()));
    }
    let sh = cx.sheet_mut(dest_sheet)?;
    if skip_blanks {
        cleared.extend(rules.iter().flat_map(|r| r.ranges.iter().copied()));
        super::super::data::remove_validation_cells(&mut sh.validations, &cleared);
    } else {
        super::super::data::remove_validation_cells(&mut sh.validations, &without_rows(pasted, skip_rows));
    }
    sh.validations.extend(rules);
    Ok(())
}

/// Applies a reference adjustment to every cell formula and defined name in the workbook.
pub(crate) fn rewrite_all_formulas(wb: &mut gridcraft_model::Workbook, target: &str, e: &gridcraft_formula::adjust::Edit) -> Result<()> {
    use gridcraft_formula::adjust::Edit;
    // Rows and columns inserted or deleted on the target sheet move its legacy array ranges too
    // (a cut moves its arrays itself).
    let moves_arrays = matches!(e, Edit::Insert { .. } | Edit::Delete { .. } | Edit::InsertCells { .. } | Edit::DeleteCells { .. });
    for si in 0..wb.sheets.len() {
        let host = wb.sheets.get(si).map(|s| s.name.clone()).unwrap_or_default();
        let keys: Vec<(CellRef, std::sync::Arc<Formula>)> =
            wb.sheets.get(si).map(|s| s.cells.iter().filter_map(|(c, cell)| cell.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
        if keys.is_empty() {
            continue;
        }
        let on_target = moves_arrays && host.eq_ignore_ascii_case(target);
        let Some(sheet) = wb.sheet_mut(si) else { continue };
        for (c, f) in keys {
            // Borrows the tree and copies and prints only the formulas the edit changes. A formula
            // the edit leaves alone keeps its stored text, even if that text is not canonical.
            let Some(new) = f.with_expr(|x| gridcraft_formula::adjust::adjust_if_changed(x, &host, target, e)) else { continue };
            let array = match f.array {
                Some(a) if on_target => crate::cmd::sheet::map_range(a, e),
                a => a,
            };
            let nf = match new {
                Some(new) => Formula { array, dynamic: f.dynamic, ..Formula::from_expr(new) },
                None if array != f.array => Formula { array, ..(*f).clone() },
                None => continue,
            };
            if let Some(cell) = sheet.cells.get_mut(c) {
                cell.formula = Some(std::sync::Arc::new(nf));
            }
        }
    }
    for n in wb.names.iter_mut() {
        if let Ok(expr) = gridcraft_formula::parse_name(&n.formula) {
            // Names are workbook-level: unqualified refs don't occur; qualified ones adjust.
            let new = gridcraft_formula::adjust::adjust(expr, "\u{0}", target, e);
            n.formula = gridcraft_formula::print(&new);
        }
    }
    rewrite_cell_links(wb, |host, expr| gridcraft_formula::adjust::adjust(expr, host, target, e));
    rewrite_python_refs(wb, &|host, expr| gridcraft_formula::adjust::adjust(expr, host, target, e))
}

/// Runs `f` over every reference a floating object holds (a chart's series values, categories,
/// name and bubble sizes, and the source block it was made from, and a form control's linked
/// cell). Every edit that moves
/// references calls it: inserting and deleting lines and cells and cut-moves (through
/// [`rewrite_all_formulas`]), and renaming and deleting sheets. `f` maps a reference held by an
/// object on the sheet named `host` to its new form; text that does not parse is kept. A source
/// block that no longer exists is dropped.
pub(crate) fn rewrite_cell_links(wb: &mut gridcraft_model::Workbook, f: impl Fn(&str, gridcraft_formula::Expr) -> gridcraft_formula::Expr) {
    for si in 0..wb.sheets.len() {
        let Some(host) = wb.sheets.get(si).filter(|s| !s.charts.is_empty() || !s.controls.is_empty()).map(|s| s.name.clone()) else { continue };
        let Some(sh) = wb.sheet_mut(si) else { continue };
        let fix = |text: &str| -> String {
            match gridcraft_formula::parse(text) {
                Ok(expr) => gridcraft_formula::print(&f(&host, expr)),
                Err(_) => text.to_string(),
            }
        };
        for ch in sh.charts.iter_mut() {
            ch.source = ch.source.as_deref().map(fix).filter(|s| !s.contains("#REF!"));
            for se in ch.series.iter_mut() {
                se.values = fix(&se.values);
                se.categories = se.categories.as_deref().map(fix);
                se.name = se.name.as_deref().map(fix);
                se.bubble_sizes = se.bubble_sizes.as_deref().map(fix);
            }
        }
        for c in sh.controls.iter_mut() {
            c.link = c.link.as_deref().map(fix);
        }
    }
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
