use super::*;

pub(super) fn insert_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    let name = match str_param(p, "name") {
        Some(n) => n.to_string(),
        None => s.doc()?.wb.next_sheet_name(),
    };
    s.doc()?.wb.check_sheet_name(&name, None).map_err(EngineError::Other)?;
    let d = s.doc_mut()?;
    let before = p.get("before").and_then(Json::as_u64).map(|v| v as usize);
    let idx = edit_doc(d, |cx| {
        // Excel inserts before the active sheet; the "+" button appends after it.
        let at = before.unwrap_or(cx.wb.active_sheet() + 1).min(cx.wb.sheets.len());
        cx.wb.sheets.insert(at, Arc::new(Sheet::new(name.clone())));
        // Names scoped to later sheets shift.
        for n in cx.wb.names.iter_mut() {
            if let Some(sc) = n.scope.as_mut()
                && *sc >= at
            {
                *sc += 1;
            }
        }
        cx.wb.set_active_sheet(at);
        *cx.sel = crate::Selection::default();
        cx.structural = true;
        Ok(at)
    })?;
    d.sheet_selections.insert(idx.min(d.sheet_selections.len()), crate::Selection::default());
    Ok(json!({"index": idx, "name": name}))
}

fn edit_doc<R>(d: &mut crate::DocState, f: impl FnOnce(&mut Ctx) -> Result<R>) -> Result<R> {
    crate::cmd::commit(d, f)
}

pub(super) fn delete_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let d = s.doc()?;
    if d.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    if d.wb.sheets.iter().filter(|s| s.visibility == Visibility::Visible).count() <= 1 {
        return Err(EngineError::Other("A workbook must contain at least one visible worksheet.".into()));
    }
    let name = d.wb.sheet(i).map(|s| s.name.clone()).unwrap_or_default();
    let order: Vec<String> = d.wb.sheets.iter().map(|s| s.name.clone()).collect();
    let d = s.doc_mut()?;
    edit_doc(d, |cx| {
        cx.wb.sheets.remove(i);
        for si in 0..cx.wb.sheets.len() {
            let keys: Vec<(CellRef, Arc<gridcraft_model::Formula>)> =
                cx.wb.sheets.get(si).map(|s| s.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
            let Some(sh) = cx.wb.sheet_mut(si) else { continue };
            for (c, f) in keys {
                if let Some(e) = f.expr() {
                    let ne = gridcraft_formula::adjust::delete_sheet(e, &name, &order);
                    if let Some(cell) = sh.cells.get_mut(c) {
                        cell.formula = Some(Arc::new(gridcraft_model::Formula::from_expr(ne)));
                    }
                }
            }
        }
        cx.wb.names.retain(|n| n.scope != Some(i));
        for n in cx.wb.names.iter_mut() {
            if let Some(sc) = n.scope.as_mut()
                && *sc > i
            {
                *sc -= 1;
            }
            // Names referring to the sheet become #REF!.
            if let Ok(e) = gridcraft_formula::parse_name(&n.formula) {
                let ne = gridcraft_formula::adjust::delete_sheet(e.clone(), &name, &order);
                if ne != e {
                    n.formula = gridcraft_formula::print(&ne);
                }
            }
        }
        if cx.wb.active_sheet() >= cx.wb.sheets.len() || cx.wb.active_sheet() > i {
            cx.wb.set_active_sheet(cx.wb.active_sheet().saturating_sub(1).min(cx.wb.sheets.len().saturating_sub(1)));
        }
        *cx.sel = crate::Selection::default();
        cx.structural = true;
        Ok(())
    })?;
    if i < d.sheet_selections.len() {
        d.sheet_selections.remove(i);
    }
    ok()
}

pub(super) fn rename_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    let i = target_sheet(s, p)?;
    let Some(name) = str_param(p, "name").map(|n| n.trim().to_string()) else { return Err(bad("sheet.rename", "missing `name`")) };
    s.doc()?.wb.check_sheet_name(&name, Some(i)).map_err(EngineError::Other)?;
    edit(s, |cx| {
        let old = cx.wb.sheet(i).map(|s| s.name.clone()).unwrap_or_default();
        cx.sheet_mut(i)?.name = name.clone();
        for si in 0..cx.wb.sheets.len() {
            let keys: Vec<(CellRef, Arc<gridcraft_model::Formula>)> =
                cx.wb.sheets.get(si).map(|s| s.cells.iter().filter_map(|(c, x)| x.formula.clone().map(|f| (c, f))).collect()).unwrap_or_default();
            let Some(sh) = cx.wb.sheet_mut(si) else { continue };
            for (c, f) in keys {
                if let Some(e) = f.expr() {
                    let ne = gridcraft_formula::adjust::rename_sheet(e, &old, &name);
                    let text = gridcraft_formula::print(&ne);
                    if text != f.text
                        && let Some(cell) = sh.cells.get_mut(c)
                    {
                        cell.formula = Some(Arc::new(gridcraft_model::Formula::from_expr(ne)));
                    }
                }
            }
            let fix = |t: &str| {
                gridcraft_formula::parse(t)
                    .map(|e| gridcraft_formula::print(&gridcraft_formula::adjust::rename_sheet(e, &old, &name)))
                    .unwrap_or_else(|_| t.to_string())
            };
            for ch in sh.charts.iter_mut() {
                for se in ch.series.iter_mut() {
                    se.values = fix(&se.values);
                    se.categories = se.categories.as_deref().map(fix);
                    se.name = se.name.as_deref().map(fix);
                }
                // A PivotChart follows its PivotTable's sheet.
                if let Some((sheet, pivot)) = ch.pivot.as_deref().and_then(crate::pivot::parse_chart_link)
                    && sheet.eq_ignore_ascii_case(&old)
                {
                    ch.pivot = Some(crate::pivot::chart_link(&name, &pivot));
                }
            }
        }
        for n in cx.wb.names.iter_mut() {
            if let Ok(e) = gridcraft_formula::parse_name(&n.formula) {
                n.formula = gridcraft_formula::print(&gridcraft_formula::adjust::rename_sheet(e, &old, &name));
            }
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

pub(super) fn move_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    let i = target_sheet(s, p)?;
    let copy = bool_param(p, "copy").unwrap_or(false);
    let to = p.get("to").and_then(Json::as_u64).map(|v| v as usize).ok_or_else(|| bad("sheet.move", "missing `to`"))?;
    edit(s, |cx| {
        let n = cx.wb.sheets.len();
        let Some(src) = cx.wb.sheets.get(i).cloned() else { return Err(bad("sheet.move", "no such sheet")) };
        if copy {
            let mut sh = (*src).clone();
            let base = sh.name.clone();
            // `Name (2)`: the name is shortened so the suffix fits in Excel's 31 characters.
            let name = (2..)
                .map(|k| {
                    let suffix = format!(" ({k})");
                    let keep = 31usize.saturating_sub(suffix.chars().count());
                    format!("{}{suffix}", base.chars().take(keep).collect::<String>().trim_end())
                })
                .find(|nm| cx.wb.sheet_index(nm).is_none())
                .unwrap_or(base.clone());
            sh.name = name;
            sh.tables.clear(); // table names must be unique
            sh.preserved = None; // kept file content (form controls, OLE objects…) stays with the original
            let at = to.min(n);
            // The copy gets its own copies of the sheet's names, referring to the copy.
            let copies: Vec<gridcraft_model::DefinedName> = cx
                .wb
                .names
                .iter()
                .filter(|nm| nm.scope == Some(i))
                .map(|nm| {
                    let formula = gridcraft_formula::parse_name(&nm.formula)
                        .map(|e| gridcraft_formula::print(&gridcraft_formula::adjust::rename_sheet(e, &base, &sh.name)))
                        .unwrap_or_else(|_| nm.formula.clone());
                    gridcraft_model::DefinedName { scope: Some(at), formula, ..nm.clone() }
                })
                .collect();
            for nm in cx.wb.names.iter_mut() {
                if let Some(sc) = nm.scope.as_mut()
                    && *sc >= at
                {
                    *sc += 1;
                }
            }
            cx.wb.names.extend(copies);
            cx.wb.sheets.insert(at, Arc::new(sh));
            cx.wb.set_active_sheet(at);
        } else {
            let sh = cx.wb.sheets.remove(i);
            let at = to.min(n - 1);
            cx.wb.sheets.insert(at, sh);
            cx.wb.set_active_sheet(at);
            // Sheet-level names follow their sheet.
            for nm in cx.wb.names.iter_mut() {
                if let Some(sc) = nm.scope.as_mut() {
                    *sc = match *sc {
                        x if x == i => at,
                        x if i < x && x <= at => x - 1,
                        x if at <= x && x < i => x + 1,
                        x => x,
                    };
                }
            }
        }
        cx.structural = true;
        Ok(Json::Null)
    })
}

pub(super) fn tab_color(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let c = crate::cmd::format::color_param(p.get("color"));
    edit(s, |cx| {
        cx.sheet_mut(i)?.tab_color = c.filter(|c| *c != gridcraft_model::Color::Auto);
        Ok(Json::Null)
    })
}

pub(super) fn hide_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    let i = target_sheet(s, p)?;
    if s.doc()?.wb.sheets.iter().filter(|s| s.visibility == Visibility::Visible).count() <= 1 {
        return Err(EngineError::Other("A workbook must contain at least one visible worksheet.".into()));
    }
    edit(s, |cx| {
        cx.sheet_mut(i)?.visibility = Visibility::Hidden;
        if cx.wb.active_sheet() == i {
            cx.wb.set_active_sheet(cx.wb.sheets.iter().position(|s| s.visibility == Visibility::Visible).unwrap_or(0));
        }
        Ok(Json::Null)
    })
}

pub(super) fn unhide_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    if s.doc()?.wb.protected_structure {
        return Err(EngineError::Other("The workbook is protected and cannot be changed.".into()));
    }
    if p.get("sheet").is_none() {
        let hidden: Vec<String> = s.doc()?.wb.sheets.iter().filter(|s| s.visibility != Visibility::Visible).map(|s| s.name.clone()).collect();
        if hidden.is_empty() {
            return Err(EngineError::Other("No sheets are hidden.".into()));
        }
        s.ui_requests.push(crate::UiRequest::Dialog("unhideSheet".into(), json!({"sheets": hidden})));
        return ok();
    }
    let i = target_sheet(s, p)?;
    edit(s, |cx| {
        cx.sheet_mut(i)?.visibility = Visibility::Visible;
        cx.wb.set_active_sheet(i);
        Ok(Json::Null)
    })
}

pub(super) fn activate(s: &mut Session, p: &Json) -> Result<Json> {
    let i = target_sheet(s, p)?;
    let d = s.doc_mut()?;
    d.wb_switch_sheet(i);
    Ok(json!({"sheet": i, "name": d.wb.active().map(|s| s.name.clone())}))
}

pub(super) fn step_sheet(s: &mut Session, dir: isize) -> Result<Json> {
    let d = s.doc()?;
    let mut i = d.wb.active_sheet();
    while let Some(next) = i.checked_add_signed(dir) {
        let Some(sheet) = d.wb.sheet(next) else { break };
        if sheet.visibility == Visibility::Visible {
            s.doc_mut()?.wb_switch_sheet(next);
            return ok();
        }
        i = next;
    }
    // No visible sheet in this direction: keep the current sheet and its selection.
    ok()
}
