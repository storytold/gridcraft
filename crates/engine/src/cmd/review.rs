//! Review tab: protection, spelling (basic), workbook statistics.

use gridcraft_model::{SheetProtection, password_hash};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "review.protectSheet",
            "Protect Sheet…",
            ["Review", "Protect"],
            None,
            "{password?, formatCells?, formatColumns?, formatRows?, insertRows?, insertColumns?, deleteRows?, deleteColumns?, sort?, autofilter?}",
            has_doc,
            protect_sheet
        ),
        cmd!("review.unprotectSheet", "Unprotect Sheet", ["Review", "Protect"], None, "{password?}", has_doc, unprotect_sheet),
        cmd!("review.protectWorkbook", "Protect Workbook", ["Review", "Protect"], None, "{on?: bool}", has_doc, protect_workbook),
        cmd!(query "review.workbookStatistics", "Workbook Statistics", ["Review", "Proofing"], None, "{}", has_doc, stats),
        cmd!(query "review.checkAccessibility", "Check Accessibility", ["Review", "Accessibility"], None, "{} → issues", has_doc, accessibility),
    ]
}

fn protect_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let b = |k: &str| bool_param(p, k).unwrap_or(false);
    let prot = SheetProtection {
        password_hash: str_param(p, "password").filter(|x| !x.is_empty()).map(password_hash),
        select_locked: true,
        select_unlocked: true,
        format_cells: b("formatCells"),
        format_columns: b("formatColumns"),
        format_rows: b("formatRows"),
        insert_columns: b("insertColumns"),
        insert_rows: b("insertRows"),
        delete_columns: b("deleteColumns"),
        delete_rows: b("deleteRows"),
        sort: b("sort"),
        autofilter: b("autofilter"),
    };
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.protection = Some(prot.clone());
        Ok(Json::Null)
    })
}

fn unprotect_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ph = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.protection.as_ref()).and_then(|pr| pr.password_hash);
    if let Some(h) = ph
        && str_param(p, "password").map(password_hash) != Some(h)
    {
        return Err(EngineError::Other("The password you supplied is not correct.".into()));
    }
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.protection = None;
        Ok(Json::Null)
    })
}

fn protect_workbook(s: &mut Session, p: &Json) -> Result<Json> {
    let cur = s.doc()?.wb.protected_structure;
    let on = bool_param(p, "on").unwrap_or(!cur);
    edit(s, |cx| {
        cx.wb.protected_structure = on;
        Ok(json!({"on": on}))
    })
}

fn stats(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let formulas = sh.cells.iter().filter(|(_, c)| c.formula.is_some()).count();
    Ok(json!({
        "sheet": {"name": sh.name, "endOfSheet": sh.used_range().map(|r| r.end.a1()), "cellsWithData": sh.cells.iter().filter(|(_, c)| !c.value.is_empty()).count(), "tables": sh.tables.len(), "formulas": formulas, "charts": sh.charts.len(), "comments": sh.comments.len()},
        "workbook": {"sheets": d.wb.sheets.len(), "cellsWithData": d.wb.sheets.iter().map(|s| s.cells.len()).sum::<usize>(), "tables": d.wb.sheets.iter().map(|s| s.tables.len()).sum::<usize>(), "charts": d.wb.sheets.iter().map(|s| s.charts.len()).sum::<usize>(), "names": d.wb.names.len()}
    }))
}

fn accessibility(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let mut issues = Vec::new();
    let loc = &d.wb.locale;
    let default_prefix = loc.ui.content("sheet");
    let picture = loc.ui.content("picture");
    for sh in &d.wb.sheets {
        for im in &sh.images {
            if im.alt.is_empty() {
                issues.push(json!({"sheet": sh.name, "issue": "Missing alternative text", "object": format!("{picture} {}", im.id)}));
            }
        }
        if !sh.merges.is_empty() {
            issues.push(json!({"sheet": sh.name, "issue": "Merged cells", "count": sh.merges.len()}));
        }
        if sh.name.starts_with(default_prefix) || sh.name.starts_with("Sheet") {
            issues.push(json!({"sheet": sh.name, "issue": "Default sheet name"}));
        }
    }
    Ok(Json::Array(issues))
}
