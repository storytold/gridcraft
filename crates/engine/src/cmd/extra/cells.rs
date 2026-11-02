use super::{
    CellRef, EngineError, HAlign, Json, NumFmt, Result, Session, Value, bad, bool_param, cell_param, cell_param_on, edit, json, object_params, ok,
    str_param, target_ranges, target_sheet,
};

/// Whether `c` has checkbox formatting.
pub(crate) fn is_checkbox_cell(wb: &gridcraft_model::Workbook, sh: &gridcraft_model::Sheet, c: CellRef) -> bool {
    wb.styles.get(sh.style_id(c)).num_fmt.as_str() == "checkbox"
}

pub(super) fn checkbox(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let si = target_sheet(s, p)?;
    edit(s, |cx| {
        for r in &ranges {
            if r.count() > 100_000 {
                return Err(bad("insert.checkbox", "range too large"));
            }
            for c in r.iter() {
                let st = cx.wb.sheet(si).map(|sh| sh.style_id(c)).unwrap_or_default();
                let new = cx.wb.styles.derive(st, |s| {
                    s.num_fmt = NumFmt::new("checkbox");
                    s.align.h = HAlign::Center;
                });
                let sh = cx.sheet_mut(si)?;
                // Values and formulas already there stay (only TRUE/FALSE draw as a box); an empty
                // cell starts unchecked.
                let mut cell = sh.cell(c).cloned().unwrap_or_default();
                if cell.value.is_empty() && cell.formula.is_none() {
                    cell.value = Value::Bool(false);
                }
                cell.style = new;
                sh.set_cell(c, cell);
                cx.touch(si, c);
            }
        }
        Ok(Json::Null)
    })
}

pub(super) fn toggle_checkbox(s: &mut Session, p: &Json) -> Result<Json> {
    let si = target_sheet(s, p)?;
    let d = s.doc()?;
    let cells: Vec<CellRef> = match cell_param_on(s, p, "cell", si)? {
        Some(c) => vec![c],
        None => d.selection.ranges.iter().flat_map(|r| r.iter().take(100_000).collect::<Vec<_>>()).collect(),
    };
    edit(s, |cx| {
        for c in cells {
            let checkbox = cx.wb.sheet(si).is_some_and(|sh| is_checkbox_cell(&cx.wb, sh, c));
            let sh = cx.sheet_mut(si)?;
            if let Some(cell) = sh.cells.get_mut(c)
                && cell.formula.is_none()
            {
                // An empty checkbox cell counts as FALSE.
                let next = match cell.value {
                    Value::Bool(b) if checkbox => Value::Bool(!b),
                    Value::Empty if checkbox => Value::Bool(true),
                    _ => continue,
                };
                cell.value = next;
                cx.changed.push((si, c));
            }
        }
        Ok(Json::Null)
    })
}

pub(super) fn theme_colors(s: &mut Session, p: &Json) -> Result<Json> {
    let mut params = object_params(p);
    if params.get("name").is_none() {
        params["name"] = json!(s.doc()?.wb.theme.name.clone());
    }
    s.execute("pageLayout.theme", params)
}

pub(super) fn theme_fonts(s: &mut Session, p: &Json) -> Result<Json> {
    let name = s.doc()?.wb.theme.name.clone();
    let colors: Vec<String> = s.doc()?.wb.theme.colors.iter().map(|c| format!("#{c:06X}")).collect();
    s.execute("pageLayout.theme", json!({"name": name, "colors": colors, "majorFont": p.get("major"), "minorFont": p.get("minor")}))
}

pub(super) fn background(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "clear").unwrap_or(false) {
        let si = s.doc()?.wb.active_sheet();
        return edit(s, |cx| {
            cx.sheet_mut(si)?.images.retain(|i| i.alt != "Sheet background");
            Ok(Json::Null)
        });
    }
    let mut params = object_params(p);
    params["alt"] = json!("Sheet background");
    params["at"] = json!("A1");
    s.execute("insert.picture", params)
}

pub(super) fn draw_border(s: &mut Session, p: &Json) -> Result<Json> {
    let edge = str_param(p, "edge").unwrap_or("grid");
    let preset = match edge {
        "top" => "top",
        "bottom" => "bottom",
        "left" => "left",
        "right" => "right",
        "outline" => "outside",
        _ => "all",
    };
    let mut params = object_params(p);
    params["preset"] = json!(preset);
    s.execute("home.borders", params)
}

pub(super) fn new_cell_style(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("home.newCellStyle", "missing `name`"))?.to_string();
    // A file has no place for a style without a name.
    if name.trim().is_empty() {
        return Err(bad("home.newCellStyle", "type a name for the style"));
    }
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let c = cell_param(s, p, "fromCell")?.unwrap_or(d.selection.active);
    let style = d.wb.styles.get(sh.style_id(c)).clone();
    edit(s, |cx| {
        cx.wb.cell_styles.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
        cx.wb.cell_styles.push((name.clone(), style.clone()));
        Ok(json!({"name": name}))
    })
}

/// The open workbook `from` names: its name as the title bar shows it (`Book2`, `Budget.xlsx`, or
/// without the extension), ignoring case, or its id ([`crate::DocState::uid`]).
fn source_workbook(s: &Session, from: Option<&Json>) -> Result<usize> {
    const ID: &str = "home.mergeStyles";
    let docs = s.documents();
    match from {
        None | Some(Json::Null) => Err(bad(ID, "missing `from` (the open workbook to merge styles from)")),
        Some(Json::Number(n)) => n.as_u64().and_then(|uid| s.document_index(uid)).ok_or_else(|| bad(ID, format!("no open workbook has id {n}"))),
        Some(Json::String(name)) => {
            let wanted = name.trim();
            let name = wanted.to_lowercase();
            let find = |same: &dyn Fn(&str) -> bool| -> Vec<usize> {
                docs.iter().enumerate().filter(|(_, d)| same(&d.display_title())).map(|(i, _)| i).collect()
            };
            let mut hits = find(&|title| title.to_lowercase() == name);
            if hits.is_empty() {
                hits = find(&|title| std::path::Path::new(title).file_stem().and_then(|x| x.to_str()).is_some_and(|x| x.to_lowercase() == name));
            }
            match hits.as_slice() {
                [i] => Ok(*i),
                [] => Err(bad(ID, format!("no open workbook is called `{wanted}`"))),
                _ => Err(bad(ID, format!("several open workbooks are called `{wanted}`: give its id"))),
            }
        }
        Some(_) => Err(bad(ID, "`from` must be the name or id of an open workbook")),
    }
}

/// Home › Cell Styles › Merge Styles: copies another open workbook's cell styles into the active
/// one ([`crate::cmd::format::merge_cell_styles`]), as one undo step. Cells keep the formats they
/// have: a cell doesn't remember the style it was given.
pub(super) fn merge_styles(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "home.mergeStyles";
    let keep = match p.get("sameNames") {
        None | Some(Json::Null) => false,
        Some(v) => match v.as_str() {
            Some("replace") => false,
            Some("keep") => true,
            _ => return Err(bad(ID, "`sameNames` must be replace or keep")),
        },
    };
    let i = source_workbook(s, p.get("from"))?;
    let d = s.doc()?;
    let source = s.documents().get(i).ok_or_else(|| bad(ID, "no such workbook"))?;
    if source.uid == d.uid {
        return Err(bad(ID, "a workbook can't merge styles from itself; choose another open workbook"));
    }
    let merge = crate::cmd::format::merge_cell_styles(&d.wb.cell_styles, &source.wb.cell_styles, &d.wb.theme, keep);
    let out = json!({"from": source.display_title(), "added": &merge.added, "replaced": &merge.replaced, "kept": &merge.kept});
    // Nothing to change: no undo step, and the workbook doesn't become unsaved.
    if !merge.changes() {
        return Ok(out);
    }
    edit(s, |cx| {
        cx.wb.cell_styles = merge.styles;
        Ok(out)
    })
}

/// Formulas › Recently Used: the functions most recently entered in a formula, newest first
/// ([`crate::Prefs::recent_functions`]).
pub(super) fn recently_used(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(json!(s.prefs.recent_functions))
}

pub(super) fn use_in_formula(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("formulas.useInFormula", "missing `name`"))?;
    s.ui_requests.push(crate::UiRequest::InsertName(name.to_string()));
    ok()
}
