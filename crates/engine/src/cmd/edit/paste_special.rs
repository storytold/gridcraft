use super::clipboard::What;
use super::*;

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
        for tr in 0..tiles_r {
            for tc in 0..tiles_c {
                for r in 0..src.height() {
                    for c in 0..src.width() {
                        let sc = CellRef::new(src.start.row + r, src.start.col + c);
                        let (dr, dc) = if transpose { (c, r) } else { (r, c) };
                        let Some(dest) = at.offset((tr * h + dr) as i64, (tc * w + dc) as i64) else { continue };
                        let src_cell = if cut_move {
                            moved_formulas.iter().find(|(k, _)| *k == sc).map(|(_, v)| v.clone())
                        } else {
                            src_sheet.cell(sc).cloned()
                        };
                        let src_val = src_sheet.value(sc);
                        let src_picture = if cut_move { moved_pictures.get(&sc) } else { src_sheet.cell_pictures.get(&sc) }.cloned();
                        if skip_blanks && src_picture.is_none() && src_val.is_empty() && src_cell.as_ref().is_none_or(|x| x.formula.is_none()) {
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
                            let moved =
                                gridcraft_formula::adjust::shift_relative(e, dest.row as i64 - sc.row as i64, dest.col as i64 - sc.col as i64);
                            Some(std::sync::Arc::new(Formula { dynamic: f.dynamic, ..Formula::from_expr(moved) }))
                        };
                        if link {
                            let sheet_prefix = if dest_sheet == clip.sheet && same_doc {
                                String::new()
                            } else {
                                format!("{}!", gridcraft_formula::quote_sheet(&src_sheet_name))
                            };
                            new.formula = Some(std::sync::Arc::new(Formula::new(&format!("={sheet_prefix}{}", sc.a1()))));
                            new.value = Value::Empty;
                            new_picture = None;
                        } else {
                            let sc_cell = src_cell.clone().unwrap_or_default();
                            match what {
                                What::All | What::AllExceptBorders => {
                                    new_picture = src_picture.clone();
                                    new.formula = shift(&sc_cell);
                                    new.value = if new.formula.is_some() { Value::Empty } else { sc_cell.value.clone() };
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
                                    if what == What::FormulasAndNumberFormats {
                                        let nf = src_wb.styles.get(sc_cell.style).num_fmt.clone();
                                        new.style = cx.wb.styles.derive(old.style, |s| s.num_fmt = nf);
                                    }
                                }
                                What::Values | What::ValuesAndNumberFormats => {
                                    new_picture = src_picture.clone();
                                    new.formula = None;
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
                            {
                                if old_picture.is_some() || new_picture.is_some() {
                                    new_picture = None;
                                    new.value = Value::Error(gridcraft_core::CellError::Value);
                                }
                                let a = old.value.clone();
                                let b = new.value.clone();
                                // Numbers stored as text take part (multiplying by 1 turns them into
                                // numbers); other text and errors stay as they are.
                                if let (Ok(x), Ok(y)) = (a.to_number(), b.to_number())
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
                                    && let Ok(y) = b.to_number()
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
                        // The copied cell's validation rule replaces whatever rule the destination had.
                        if !link && matches!(what, What::All | What::AllExceptBorders | What::Validation) {
                            let rule = src_sheet.validations.iter().find(|dv| dv.ranges.iter().any(|r| r.contains(sc))).cloned();
                            let sh = cx.sheet_mut(dest_sheet)?;
                            crate::cmd::data::remove_validation_cells(&mut sh.validations, &[RangeRef::new(dest, dest)]);
                            if let Some(mut dv) = rule {
                                // Relative references move with the rule, as pasted formulas do.
                                let first = dv.ranges.first().map_or(sc, |r| r.start);
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
            let dest_name = cx.wb.sheet(dest_sheet).map(|s| s.name.clone()).unwrap_or_default();
            let e = if dest_sheet == clip.sheet {
                gridcraft_formula::adjust::Edit::Move { from: src, to_row: at.row, to_col: at.col }
            } else {
                gridcraft_formula::adjust::Edit::MoveToSheet { from: src, to_sheet: dest_name, to_row: at.row, to_col: at.col }
            };
            rewrite_all_formulas(&mut cx.wb, &src_sheet_name, &e);
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

/// Applies a reference adjustment to every cell formula and defined name in the workbook.
pub(crate) fn rewrite_all_formulas(wb: &mut gridcraft_model::Workbook, target: &str, e: &gridcraft_formula::adjust::Edit) {
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
            let Some(expr) = f.expr() else { continue };
            let new = gridcraft_formula::adjust::adjust(expr, &host, target, e);
            let text = gridcraft_formula::print(&new);
            let array = match f.array {
                Some(a) if on_target => crate::cmd::sheet::map_range(a, e),
                a => a,
            };
            if (text != f.text || array != f.array)
                && let Some(cell) = sheet.cells.get_mut(c)
            {
                let mut nf = Formula::from_expr(new);
                nf.array = array;
                nf.dynamic = f.dynamic;
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
}
