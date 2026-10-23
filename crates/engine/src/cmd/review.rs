//! Review tab: protection, spelling (basic), workbook statistics, comment navigation and the
//! Comments pane, Show All Notes and Convert to Comments.

use gridcraft_core::CellRef;
use gridcraft_formula::{Expr, StructItem};
use gridcraft_model::{EditRange, PasswordHash, SheetProtection, StyleId, Visibility, password_hash};
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
        cmd!(
            "review.protectWorkbook",
            "Protect Workbook",
            ["Review", "Protect"],
            None,
            "{on?: bool (default: toggle), password?} (structure protection; unprotecting needs the password when one was set)",
            has_doc,
            protect_workbook
        ),
        cmd!(query "review.workbookStatistics", "Workbook Statistics", ["Review", "Proofing"], None, "{}", has_doc, stats),
        // Not journaled: the params can hold a password.
        CommandSpec {
            id: "review.allowEditRanges",
            label: "Allow Edit Ranges…",
            menu: &["Review", "Protect"],
            shortcut: None,
            params: "{action?: list (default) | add | modify | delete | unlock, title?, range?, password?, newTitle?, sheet?} → ranges",
            enabled: has_doc,
            run: allow_edit_ranges,
            journal: false,
            undoable: true,
        },
        cmd!(query "review.checkAccessibility", "Check Accessibility", ["Review", "Accessibility"], None, "{} → issues", has_doc, accessibility),
        cmd!(
            query
            "review.checkPerformance",
            "Check Performance",
            ["Review", "Accessibility"],
            None,
            "{limit?: number (default 500, max 5000)} → {version, usedCells, optimizableCells, truncated, sheets, cells}",
            has_doc,
            check_performance
        ),
        cmd!(
            "review.optimizePerformance",
            "Optimize Cells",
            ["Review", "Accessibility"],
            None,
            "{sheet?: number | name} → {optimized, truncated, skippedProtected, sheets} (clears formatting from blank cells)",
            has_doc,
            optimize_performance
        ),
        cmd!(
            query
            "review.auditCalculation",
            "Audit Calculation",
            [],
            None,
            "{limit?: number (default 500, max 5000)} → {version, total, truncated, issues, categories, sheets} (extension: volatile and oversized references)",
            has_doc,
            audit_calculation
        ),
        cmd!(noundo "review.previousComment", "Previous Comment", ["Review", "Comments"], None, "{} → {sheet, cell}", has_comments, |s, _| {
            step_comment(s, false)
        }),
        cmd!(noundo "review.nextComment", "Next Comment", ["Review", "Comments"], None, "{} → {sheet, cell}", has_comments, |s, _| step_comment(
            s, true
        )),
        cmd!(query "review.showComments", "Show Comments", ["Review", "Comments"], None, "{on?: bool} (toggles the Comments pane)", has_doc, |s, p| {
            s.comments_pane = bool_param(p, "on").unwrap_or(!s.comments_pane);
            Ok(json!({"on": s.comments_pane}))
        }),
        cmd!(query "review.showAllNotes", "Show All Notes", ["Review", "Notes"], None, "{on?: bool}", has_doc, |s, p| {
            s.show_all_notes = bool_param(p, "on").unwrap_or(!s.show_all_notes);
            Ok(json!({"on": s.show_all_notes}))
        }),
        cmd!(
            "review.convertToComments",
            "Convert to Comments",
            ["Review", "Notes"],
            None,
            "{} (every note in the workbook) → {converted}",
            has_doc,
            convert_notes
        ),
    ]
}

/// Threaded comments in navigation order: visible sheets in tab order, then row by row.
fn comment_cells(s: &Session) -> Vec<(usize, CellRef)> {
    let Some(d) = s.active() else { return vec![] };
    d.wb.sheets
        .iter()
        .enumerate()
        .filter(|(_, sh)| sh.visibility == Visibility::Visible)
        // `CellRef` orders by row, then column: reading order.
        .flat_map(|(i, sh)| sh.comments.iter().filter(|(_, c)| c.threaded).map(move |(at, _)| (i, *at)))
        .collect()
}

fn has_comments(s: &Session) -> std::result::Result<(), String> {
    has_doc(s)?;
    if comment_cells(s).is_empty() { Err("there are no comments in this workbook".into()) } else { Ok(()) }
}

/// Selects the next (or previous) threaded comment after the active cell, moving on to the next
/// sheet and wrapping around at the end of the workbook.
fn step_comment(s: &mut Session, forward: bool) -> Result<Json> {
    let list = comment_cells(s);
    let d = s.doc()?;
    let here = (d.wb.active_sheet, d.selection.active);
    let target = if forward {
        list.iter().find(|c| **c > here).or_else(|| list.first())
    } else {
        list.iter().rev().find(|c| **c < here).or_else(|| list.last())
    };
    let Some(&(sheet, at)) = target else {
        return Err(EngineError::Other("There are no comments in this workbook.".into()));
    };
    let d = s.doc_mut()?;
    d.wb_switch_sheet(sheet);
    d.selection = crate::Selection::at(at);
    if let Some(sh) = d.wb.active() {
        d.selection.expand_merges(sh);
    }
    let name = d.wb.active().map(|sh| sh.name.clone()).unwrap_or_default();
    Ok(json!({"sheet": name, "cell": at.a1()}))
}

/// Review › Notes › Convert to Comments: every note in the workbook becomes a threaded comment
/// with the same author and text.
fn convert_notes(s: &mut Session, _: &Json) -> Result<Json> {
    let sheets: Vec<usize> =
        s.doc()?.wb.sheets.iter().enumerate().filter(|(_, sh)| sh.comments.values().any(|c| !c.threaded)).map(|(i, _)| i).collect();
    if sheets.is_empty() {
        return Ok(json!({"converted": 0}));
    }
    edit(s, |cx| {
        let mut n = 0usize;
        for i in &sheets {
            for c in cx.sheet_mut(*i)?.comments.values_mut().filter(|c| !c.threaded) {
                c.threaded = true;
                c.visible = false;
                n += 1;
            }
        }
        Ok(json!({"converted": n}))
    })
}

fn protect_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    // Re-protecting a password-protected sheet would replace the password without knowing it:
    // unprotect it first, as in Excel. One without a password can take new options directly.
    let has_password = |pr: &SheetProtection| pr.password.is_some() || pr.legacy_password_hash.is_some();
    if s.doc()?.wb.sheet(sheet).and_then(|sh| sh.protection.as_ref()).is_some_and(has_password) {
        return Err(EngineError::Other("The sheet is protected with a password. Unprotect it first (Review › Unprotect Sheet).".into()));
    }
    let b = |k: &str| bool_param(p, k).unwrap_or(false);
    // Selection rights default to what the sheet has now (a file may forbid selecting locked cells).
    let current = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.protection.clone());
    let select_locked = bool_param(p, "selectLockedCells").unwrap_or_else(|| current.as_ref().is_none_or(|c| c.select_locked));
    let select_unlocked = bool_param(p, "selectUnlockedCells").unwrap_or_else(|| current.as_ref().is_none_or(|c| c.select_unlocked));
    let prot = SheetProtection {
        password: None,
        legacy_password_hash: None,
        select_locked,
        select_unlocked,
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
    let d = s.doc()?;
    let what = d.wb.sheet(sheet).map(|sh| sh.name.clone()).unwrap_or_default();
    let prot = SheetProtection { password: str_param(p, "password").filter(|x| !x.is_empty()).map(|pw| new_password_hash(d, &what, pw)), ..prot };
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.protection = Some(prot.clone());
        Ok(Json::Null)
    })
}

fn unprotect_sheet(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    if let Some(pr) = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.protection.as_ref()) {
        let pw = str_param(p, "password").unwrap_or("");
        let ok = match (&pr.password, pr.legacy_password_hash) {
            (Some(h), _) => h.verify(pw).map_err(EngineError::Other)?,
            (None, Some(h)) => password_hash(pw) == h,
            (None, None) => true,
        };
        if !ok {
            return Err(EngineError::Other("The password you supplied is not correct.".into()));
        }
    }
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.protection = None;
        Ok(Json::Null)
    })
}

/// A new Excel-compatible (SHA-512, salted) hash of `pw`. The salt mixes `what` is protected
/// with the document, a counter and the clock, so equal passwords get different hashes.
fn new_password_hash(d: &DocState, what: &str, pw: &str) -> PasswordHash {
    static SALT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SALT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let seed = format!("{what}\u{0}{n}\u{0}{}\u{0}{}", d.uid, gridcraft_calc::now_serial().to_bits());
    PasswordHash::new_sha512(pw, &gridcraft_model::password::salt_from(seed.as_bytes()))
}

fn range_text(r: &EditRange) -> String {
    r.ranges.iter().map(|x| x.a1()).collect::<Vec<_>>().join(",")
}

/// Review › Allow Edit Ranges: list, add, modify or delete the ranges that stay editable on a
/// protected sheet, or unlock a password range for this session.
fn allow_edit_ranges(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let action = str_param(p, "action").unwrap_or("list");
    let title = str_param(p, "title").map(str::trim).filter(|t| !t.is_empty());
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    let sheet_name = sh.name.clone();
    let find = |t: Option<&str>| -> Result<usize> {
        let t = t.ok_or_else(|| bad("review.allowEditRanges", "missing `title`"))?;
        sh.edit_ranges
            .iter()
            .position(|r| r.title.eq_ignore_ascii_case(t))
            .ok_or_else(|| bad("review.allowEditRanges", format!("no range titled `{t}`")))
    };
    let hash = |pw: &str, title: &str| new_password_hash(d, &format!("{sheet_name}\u{0}{title}"), pw);
    if matches!(action, "add" | "modify" | "delete") && sh.is_protected() {
        return Err(EngineError::Other("Unprotect the sheet to change its edit ranges.".into()));
    }
    match action {
        "list" => {}
        "unlock" => {
            let i = find(title)?;
            let Some(r) = sh.edit_ranges.get(i) else { return Err(bad("review.allowEditRanges", "no such range")) };
            if let Some(h) = &r.password {
                let pw = str_param(p, "password").unwrap_or("");
                if !h.verify(pw).map_err(EngineError::Other)? {
                    return Err(EngineError::Other("The password you supplied is not correct.".into()));
                }
                let key = (sheet_name.clone(), r.title.clone());
                let d = s.doc_mut()?;
                if !d.unlocked_ranges.contains(&key) {
                    d.unlocked_ranges.push(key);
                }
            }
        }
        "add" => {
            let t = match title {
                Some(t) => t.to_string(),
                None => {
                    (1..).map(|n| format!("Range{n}")).find(|t| !sh.edit_ranges.iter().any(|r| r.title.eq_ignore_ascii_case(t))).unwrap_or_default()
                }
            };
            if sh.edit_ranges.iter().any(|r| r.title.eq_ignore_ascii_case(&t)) {
                return Err(bad("review.allowEditRanges", format!("a range titled `{t}` already exists")));
            }
            let ranges = target_ranges(s, p)?;
            let password = str_param(p, "password").filter(|pw| !pw.is_empty()).map(|pw| hash(pw, &t));
            let r = EditRange { title: t, ranges, password, security_descriptor: None };
            edit(s, |cx| {
                cx.sheet_mut(sheet)?.edit_ranges.push(r.clone());
                Ok(())
            })?;
        }
        "modify" => {
            let i = find(title)?;
            let mut r = sh.edit_ranges.get(i).cloned().ok_or_else(|| bad("review.allowEditRanges", "no such range"))?;
            if let Some(n) = str_param(p, "newTitle").map(str::trim).filter(|n| !n.is_empty()) {
                if sh.edit_ranges.iter().enumerate().any(|(j, x)| j != i && x.title.eq_ignore_ascii_case(n)) {
                    return Err(bad("review.allowEditRanges", format!("a range titled `{n}` already exists")));
                }
                r.title = n.to_string();
            }
            if let Some(pw) = str_param(p, "password") {
                r.password = (!pw.is_empty()).then(|| hash(pw, &r.title));
            }
            if p.get("range").is_some() {
                r.ranges = target_ranges(s, p)?;
            }
            edit(s, |cx| {
                if let Some(x) = cx.sheet_mut(sheet)?.edit_ranges.get_mut(i) {
                    *x = r.clone();
                }
                Ok(())
            })?;
        }
        "delete" => {
            let i = find(title)?;
            edit(s, |cx| {
                let list = &mut cx.sheet_mut(sheet)?.edit_ranges;
                if i < list.len() {
                    list.remove(i);
                }
                Ok(())
            })?;
        }
        other => return Err(bad("review.allowEditRanges", format!("unknown action `{other}`"))),
    }
    let d = s.doc()?;
    let sh = d.wb.sheet(sheet).ok_or(EngineError::NoDocument)?;
    Ok(Json::Array(
        sh.edit_ranges
            .iter()
            .map(|r| {
                let unlocked = r.password.is_none() || d.unlocked_ranges.iter().any(|(s, t)| *s == sh.name && *t == r.title);
                json!({"title": r.title, "range": range_text(r), "password": r.password.is_some(), "unlocked": unlocked})
            })
            .collect(),
    ))
}

/// Review › Protect Workbook: lock the workbook structure (sheets can't be added, moved,
/// renamed, hidden or deleted), optionally with a password that unprotecting must match.
fn protect_workbook(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let cur = d.wb.protected_structure;
    let on = bool_param(p, "on").unwrap_or(!cur);
    if on == cur {
        return Ok(json!({"on": on}));
    }
    let password = if on {
        str_param(p, "password").filter(|pw| !pw.is_empty()).map(|pw| new_password_hash(d, "workbook", pw))
    } else {
        if let Some(h) = &d.wb.structure_password
            && !h.verify(str_param(p, "password").unwrap_or("")).map_err(EngineError::Other)?
        {
            return Err(EngineError::Other("The password you supplied is not correct.".into()));
        }
        None
    };
    edit(s, |cx| {
        cx.wb.protected_structure = on;
        cx.wb.structure_password = password.clone();
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
    for sh in &d.wb.sheets {
        for im in &sh.images {
            if im.alt.is_empty() {
                issues.push(json!({"sheet": sh.name, "issue": "Missing alternative text", "object": format!("Picture {}", im.id)}));
            }
        }
        if !sh.merges.is_empty() {
            issues.push(json!({"sheet": sh.name, "issue": "Merged cells", "count": sh.merges.len()}));
        }
        if sh.name.starts_with("Sheet") {
            issues.push(json!({"sheet": sh.name, "issue": "Default sheet name"}));
        }
    }
    Ok(Json::Array(issues))
}

// ---------------------------------------------------------------- Check Performance

const AUDIT_CATEGORIES: &[(&str, &str)] = &[
    ("volatileFormula", "Volatile formulas"),
    ("fullColumnReference", "Full-column references"),
    ("fullRowReference", "Full-row references"),
    ("wholeTableReference", "Whole-table references"),
    ("oversizedRange", "Oversized ranges"),
];
const AUDIT_CATEGORY_COUNT: usize = AUDIT_CATEGORIES.len();
const PERFORMANCE_LIMIT: u64 = 500;
const PERFORMANCE_MAX_LIMIT: u64 = 5_000;
/// Ranges bigger than this are reported even when they are not a whole column or row.
const OVERSIZED_RANGE_CELLS: u64 = 1_000_000;
/// One Optimize run clears at most this many blank formatted cells, so a pathological file cannot
/// turn a single click into an unbounded edit.
const OPTIMIZE_LIMIT: usize = 200_000;

const CAT_VOLATILE: usize = 0;
const CAT_FULL_COLUMN: usize = 1;
const CAT_FULL_ROW: usize = 2;
const CAT_WHOLE_TABLE: usize = 3;
const CAT_OVERSIZED: usize = 4;

struct SheetPerformance {
    name: String,
    protected: bool,
    cells: usize,
    formulas: usize,
    issues: u64,
}

/// The cells Excel's Check Performance can remove: no value, no formula, but a stored non-default
/// style. Notes and comments live outside the cell and never make a cell optimizable, and neither
/// does a dynamic-array spill: formatting a range over a spilled `=SEQUENCE(3)` stores empty cells
/// under the spill, whose displayed values are the spilled results, so removing that formatting
/// would change what the sheet shows.
fn optimizable(sh: &gridcraft_model::Sheet, at: CellRef, cell: &gridcraft_model::Cell) -> bool {
    sh.merge_at(at).is_none()
        && !sh.spill.contains_key(&at)
        && !sh.spill_ranges.values().any(|range| range.contains(at))
        && cell.value.is_empty()
        && cell.formula.is_none()
        && cell.style != StyleId::DEFAULT
}

/// Excel's volatile functions. `OFFSET`, `INDIRECT`, `CELL` and `INFO` are volatile in Excel but
/// are evaluator built-ins here, so they are not in the function registry's `volatile` flag.
fn volatile_function(name: &str) -> bool {
    matches!(name, "NOW" | "TODAY" | "OFFSET" | "INDIRECT" | "RAND" | "RANDBETWEEN" | "RANDARRAY" | "CELL" | "INFO")
        || gridcraft_functions::lookup(name).is_some_and(|spec| spec.volatile)
}

fn push_unique(out: &mut Vec<String>, value: String) {
    if !out.contains(&value) {
        out.push(value);
    }
}

fn goto_reference(sheet: &str, cell: CellRef) -> String {
    format!("'{}'!{}", sheet.replace('\'', "''"), cell.a1())
}

/// Review › Check Performance: Excel's Workbook Performance pane data. It reports the cells used
/// per sheet and how many are blank but still carry formatting, plus a bounded list of those
/// cells. Walking stored cells keeps the cost proportional to the used cells, never a whole
/// column. This is exactly Excel's documented feature; the calculation audit is
/// [`audit_calculation`], opt-in and separate.
fn check_performance(s: &mut Session, p: &Json) -> Result<Json> {
    let limit = p.get("limit").and_then(Json::as_u64).unwrap_or(PERFORMANCE_LIMIT).clamp(1, PERFORMANCE_MAX_LIMIT) as usize;
    let d = s.doc()?;
    let (mut used_cells, mut optimizable_cells) = (0u64, 0u64);
    let mut cells: Vec<Json> = Vec::new();
    let mut sheets: Vec<Json> = Vec::with_capacity(d.wb.sheets.len());
    for (si, sh) in d.wb.sheets.iter().enumerate() {
        let used = sh.cells.len() as u64;
        let mut sheet_optimizable = 0u64;
        for (at, cell) in sh.cells.iter() {
            if !optimizable(sh, at, cell) {
                continue;
            }
            sheet_optimizable += 1;
            if cells.len() < limit {
                cells.push(json!({
                    "sheet": sh.name,
                    "sheetIndex": si,
                    "cell": at.a1(),
                    "style": cell.style.0,
                    "reason": "Blank cell with stored formatting.",
                    "optimize": "clearFormatting",
                    "goto": goto_reference(&sh.name, at)
                }));
            }
        }
        used_cells += used;
        optimizable_cells += sheet_optimizable;
        sheets.push(json!({
            "name": sh.name,
            "index": si,
            "usedCells": used,
            "optimizableCells": sheet_optimizable,
            "protected": sh.is_protected()
        }));
    }
    let shown = cells.len() as u64;
    Ok(json!({
        "version": 1,
        "usedCells": used_cells,
        "optimizableCells": optimizable_cells,
        "shown": shown,
        "truncated": optimizable_cells > shown,
        "limit": limit,
        "sheets": sheets,
        "cells": cells
    }))
}

/// Extension, not Excel's Check Performance: a deterministic, read-only calculation audit. It
/// walks stored cells only (never a whole column), so the cost is proportional to the used cells,
/// and caps the issues returned while still counting every finding.
fn audit_calculation(s: &mut Session, p: &Json) -> Result<Json> {
    let limit = p.get("limit").and_then(Json::as_u64).unwrap_or(PERFORMANCE_LIMIT).clamp(1, PERFORMANCE_MAX_LIMIT) as usize;
    let d = s.doc()?;
    let mut counts = [0u64; AUDIT_CATEGORY_COUNT];
    let mut found: Vec<(usize, usize, CellRef, Json)> = Vec::new();
    let mut sheets: Vec<SheetPerformance> = Vec::with_capacity(d.wb.sheets.len());
    let (mut scanned_cells, mut scanned_formulas) = (0usize, 0usize);

    for (si, sh) in d.wb.sheets.iter().enumerate() {
        let (mut sheet_cells, mut sheet_formulas, mut sheet_issues) = (0usize, 0usize, 0u64);
        for (at, cell) in sh.cells.iter() {
            sheet_cells += 1;
            let Some(formula) = &cell.formula else { continue };
            sheet_formulas += 1;
            let Some(expr) = formula.expr_arc() else { continue };

            let (mut volatile, mut full_cols, mut full_rows, mut tables, mut oversized) =
                (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
            expr.walk(&mut |node| match node {
                Expr::Call(name, _) if volatile_function(name) => push_unique(&mut volatile, name.clone()),
                Expr::Ref(reference) => {
                    let range = reference.range();
                    let text = range.a1();
                    let (cols, rows) = (range.is_full_cols(), range.is_full_rows());
                    if cols && rows {
                        push_unique(&mut oversized, text);
                    } else if cols {
                        push_unique(&mut full_cols, text);
                    } else if rows {
                        push_unique(&mut full_rows, text);
                    } else if range.count() > OVERSIZED_RANGE_CELLS {
                        push_unique(&mut oversized, text);
                    }
                }
                Expr::Struct(sr) => {
                    let all_columns =
                        sr.col_start.is_none() && sr.col_end.is_none() && sr.specifiers.iter().all(|item| matches!(item, StructItem::Data));
                    if all_columns || sr.specifiers.iter().any(|item| matches!(item, StructItem::All)) {
                        let mut label = if sr.table.is_empty() { "table".to_string() } else { sr.table.clone() };
                        label.push_str(if sr.specifiers.iter().any(|item| matches!(item, StructItem::All)) { "[#All]" } else { "[all columns]" });
                        push_unique(&mut tables, label);
                    }
                }
                Expr::Name(name) if d.wb.table(name).is_some() => push_unique(&mut tables, format!("{name}[#All]")),
                _ => {}
            });

            volatile.sort();
            let formula_text = format!("={}", formula.text);
            let goto = goto_reference(&sh.name, at);
            let mut record = |category: usize, references: Vec<String>, functions: Vec<String>, reason: String, optimize: Option<&str>| {
                counts[category] += 1;
                sheet_issues += 1;
                if found.len() < limit {
                    let range = if references.is_empty() { at.a1() } else { references.join(", ") };
                    found.push((
                        category,
                        si,
                        at,
                        json!({
                            "category": AUDIT_CATEGORIES[category].0,
                            "categoryLabel": AUDIT_CATEGORIES[category].1,
                            "sheet": sh.name,
                            "sheetIndex": si,
                            "cell": at.a1(),
                            "range": range,
                            "formula": formula_text,
                            "references": references,
                            "functions": functions,
                            "reason": reason,
                            "optimize": optimize,
                            "goto": goto
                        }),
                    ));
                }
            };
            if !volatile.is_empty() {
                record(
                    CAT_VOLATILE,
                    Vec::new(),
                    volatile,
                    "Uses a volatile function; it recalculates whenever the workbook recalculates.".into(),
                    None,
                );
            }
            if !full_cols.is_empty() {
                record(
                    CAT_FULL_COLUMN,
                    full_cols,
                    Vec::new(),
                    "Full-column reference spans all 1,048,576 rows; bound it to the used range.".into(),
                    None,
                );
            }
            if !full_rows.is_empty() {
                record(CAT_FULL_ROW, full_rows, Vec::new(), "Full-row reference spans all 16,384 columns; bound it to the used range.".into(), None);
            }
            if !tables.is_empty() {
                record(
                    CAT_WHOLE_TABLE,
                    tables,
                    Vec::new(),
                    "Structured reference reads a whole table; reference only the columns you need.".into(),
                    None,
                );
            }
            if !oversized.is_empty() {
                record(CAT_OVERSIZED, oversized, Vec::new(), "Range spans more than 1,000,000 cells; narrow it to the data you need.".into(), None);
            }
        }
        scanned_cells += sheet_cells;
        scanned_formulas += sheet_formulas;
        sheets.push(SheetPerformance {
            name: sh.name.clone(),
            protected: sh.is_protected(),
            cells: sheet_cells,
            formulas: sheet_formulas,
            issues: sheet_issues,
        });
    }

    // Group by category, then sheet and cell, so the pane and every caller see the same order.
    found.sort_by_key(|(category, sheet, at, _)| (*category, *sheet, *at));
    let shown = found.len() as u64;
    let total: u64 = counts.iter().sum();
    let issues: Vec<Json> = found.into_iter().map(|(_, _, _, issue)| issue).collect();
    let categories: Vec<Json> =
        AUDIT_CATEGORIES.iter().enumerate().map(|(i, (id, label))| json!({"id": id, "label": label, "count": counts[i]})).collect();
    let sheet_rows: Vec<Json> = sheets
        .iter()
        .map(|sheet| {
            json!({
                "name": sheet.name,
                "protected": sheet.protected,
                "cells": sheet.cells,
                "formulas": sheet.formulas,
                "issues": sheet.issues
            })
        })
        .collect();
    Ok(json!({
        "version": 1,
        "total": total,
        "shown": shown,
        "truncated": total > shown,
        "limit": limit,
        "scanned": {"cells": scanned_cells, "formulas": scanned_formulas},
        "categories": categories,
        "sheets": sheet_rows,
        "issues": issues
    }))
}

/// Review › Check Performance › Optimize all / Optimize sheet. Only a provably safe change is
/// made: blank cells whose stored style is non-default lose that style. Values, formulas, notes
/// and comments are untouched; protected sheets are skipped.
fn optimize_performance(s: &mut Session, p: &Json) -> Result<Json> {
    let target = match p.get("sheet") {
        Some(value) if !value.is_null() => Some(target_sheet(s, p)?),
        _ => None,
    };
    let d = s.doc()?;
    let mut plan: Vec<(usize, Vec<CellRef>)> = Vec::new();
    let mut skipped: Vec<Json> = Vec::new();
    let mut per_sheet: Vec<Json> = Vec::new();
    let mut selected = 0usize;
    let mut truncated = false;
    for (si, sh) in d.wb.sheets.iter().enumerate() {
        if target.is_some_and(|target| target != si) {
            continue;
        }
        if sh.is_protected() {
            skipped.push(json!(sh.name));
            continue;
        }
        let mut cells = Vec::new();
        for (at, cell) in sh.cells.iter() {
            if optimizable(sh, at, cell) {
                if selected >= OPTIMIZE_LIMIT {
                    truncated = true;
                    break;
                }
                cells.push(at);
                selected += 1;
            }
        }
        per_sheet.push(json!({"name": sh.name, "index": si, "optimized": cells.len()}));
        if !cells.is_empty() {
            plan.push((si, cells));
        }
        if truncated {
            break;
        }
    }
    if selected == 0 {
        return Ok(json!({"optimized": 0, "truncated": truncated, "skippedProtected": skipped, "sheets": per_sheet}));
    }
    edit(s, |cx| {
        for (si, cells) in plan {
            for at in cells {
                cx.sheet_mut(si)?.remove_cell(at);
                cx.touch(si, at);
            }
        }
        Ok(Json::Null)
    })?;
    Ok(json!({"optimized": selected, "truncated": truncated, "skippedProtected": skipped, "sheets": per_sheet}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
        s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
        for (sheet, cell) in [(0, "C1"), (0, "A3"), (1, "B2")] {
            s.execute("review.newComment", json!({"sheet": sheet, "cell": cell, "text": "c", "author": "Ann"})).unwrap();
        }
        s.execute("review.newNote", json!({"sheet": 0, "cell": "A2", "text": "note", "author": "Bob"})).unwrap();
        s
    }

    fn at(s: &Session) -> (usize, String) {
        let d = s.doc().unwrap();
        (d.wb.active_sheet, d.selection.active.a1())
    }

    #[test]
    fn next_and_previous_comment_go_in_reading_order_across_sheets_and_wrap() {
        let mut s = book();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
        // Notes are skipped: A2 holds a note, not a comment.
        let order = [(0, "C1"), (0, "A3"), (1, "B2"), (0, "C1")];
        for (sheet, cell) in order {
            s.execute("review.nextComment", json!({})).unwrap();
            assert_eq!(at(&s), (sheet, cell.to_string()));
        }
        for (sheet, cell) in [(1, "B2"), (0, "A3"), (0, "C1")] {
            s.execute("review.previousComment", json!({})).unwrap();
            assert_eq!(at(&s), (sheet, cell.to_string()));
        }
    }

    #[test]
    fn comment_navigation_is_disabled_without_comments() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("review.newNote", json!({"cell": "A1", "text": "note"})).unwrap();
        assert!(s.execute("review.nextComment", json!({})).is_err());
        assert!(s.execute("review.previousComment", json!({})).is_err());
        assert_eq!(at(&s), (0, "A1".to_string()));
    }

    #[test]
    fn protection_that_forbids_selecting_locked_cells_is_enforced_and_kept() {
        let mut s = book();
        s.execute("home.lockCell", json!({"range": "B2", "on": false})).unwrap();
        s.execute("review.protectSheet", json!({"selectLockedCells": false})).unwrap();
        // Locked cells can't be selected; unlocked ones can.
        assert!(s.execute("selection.set", json!({"cell": "A1"})).is_err());
        s.execute("selection.set", json!({"cell": "B2"})).unwrap();
        // Arrow keys skip locked cells, and stay put when nothing selectable is left.
        s.execute("selection.move", json!({"dr": 0, "dc": 1})).unwrap();
        assert_eq!(s.doc().unwrap().selection.active.a1(), "B2");
        // Re-protecting without the flag keeps the sheet's selection rights.
        s.execute("review.unprotectSheet", json!({})).unwrap();
        s.execute("review.protectSheet", json!({"selectLockedCells": false})).unwrap();
        s.execute("review.protectSheet", json!({"sort": true})).unwrap();
        assert!(!s.doc().unwrap().wb.active().unwrap().protection.clone().unwrap().select_locked);
        s.execute("review.unprotectSheet", json!({})).unwrap();
        s.execute("selection.set", json!({"cell": "A1"})).unwrap();
    }

    #[test]
    fn sheet_passwords_use_excel_hashes_and_old_json_hashes_still_verify() {
        let mut s = book();
        s.execute("review.protectSheet", json!({"password": "pa55"})).unwrap();
        let prot = s.doc().unwrap().wb.active().unwrap().protection.clone().unwrap();
        assert!(matches!(&prot.password, Some(PasswordHash::Iterated { algorithm, .. }) if algorithm == "SHA-512"));
        assert!(s.execute("review.unprotectSheet", json!({})).is_err());
        assert!(s.execute("review.unprotectSheet", json!({"password": "pass"})).is_err());
        // Protecting again can't swap the password out from under it.
        assert!(s.execute("review.protectSheet", json!({})).is_err());
        assert_eq!(s.doc().unwrap().wb.active().unwrap().protection.as_ref(), Some(&prot));
        s.execute("review.unprotectSheet", json!({"password": "pa55"})).unwrap();
        assert!(!s.doc().unwrap().wb.active().unwrap().is_protected());
        // A GridCraft JSON workbook saved before carried the internal hash under `password_hash`.
        let old = format!(
            r#"{{"password_hash": {}, "select_locked": true, "select_unlocked": true, "format_cells": false, "format_columns": false, "format_rows": false, "insert_columns": false, "insert_rows": false, "delete_columns": false, "delete_rows": false, "sort": false, "autofilter": false}}"#,
            password_hash("old")
        );
        let prot: SheetProtection = serde_json::from_str(&old).unwrap();
        assert_eq!((prot.legacy_password_hash, prot.password.is_none()), (Some(password_hash("old")), true));
        let i = s.doc().unwrap().wb.active_sheet;
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        std::sync::Arc::make_mut(&mut wb.sheets[i]).protection = Some(prot);
        assert!(s.execute("review.unprotectSheet", json!({"password": "new"})).is_err());
        s.execute("review.unprotectSheet", json!({"password": "old"})).unwrap();
    }

    #[test]
    fn protected_structure_blocks_rename_move_copy_hide_and_unhide_sheet() {
        let mut s = book();
        let names = |s: &Session| s.doc().unwrap().wb.sheets.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
        let before = names(&s);
        s.execute("review.protectWorkbook", json!({})).unwrap();
        assert!(s.execute("sheet.rename", json!({"sheet": 0, "name": "Renamed"})).is_err());
        assert!(s.execute("sheet.move", json!({"sheet": 0, "to": 1})).is_err());
        assert!(s.execute("sheet.move", json!({"sheet": 0, "to": 1, "copy": true})).is_err());
        assert!(s.execute("sheet.hide", json!({"sheet": 1})).is_err());
        assert_eq!(names(&s), before);
        assert!(s.doc().unwrap().wb.sheets.iter().all(|x| x.visibility == Visibility::Visible));
        s.execute("review.protectWorkbook", json!({})).unwrap();
        s.execute("sheet.rename", json!({"sheet": 0, "name": "Renamed"})).unwrap();
        s.execute("sheet.move", json!({"sheet": 0, "to": 1})).unwrap();
        s.execute("sheet.move", json!({"sheet": 0, "to": 1, "copy": true})).unwrap();
        assert_eq!(s.doc().unwrap().wb.sheets.len(), 3);
        s.execute("sheet.hide", json!({"sheet": 1})).unwrap();
        s.execute("review.protectWorkbook", json!({})).unwrap();
        assert!(s.execute("sheet.unhide", json!({"sheet": 1})).is_err());
        assert!(s.execute("sheet.unhide", json!({})).is_err());
        assert_eq!(s.doc().unwrap().wb.sheets[1].visibility, Visibility::Hidden);
        s.execute("review.protectWorkbook", json!({})).unwrap();
        s.execute("sheet.unhide", json!({"sheet": 1})).unwrap();
    }

    #[test]
    fn protect_workbook_with_a_password_really_verifies() {
        let mut s = book();
        s.execute("review.protectWorkbook", json!({"password": "pa55"})).unwrap();
        assert!(s.doc().unwrap().wb.protected_structure);
        assert!(s.execute("home.insertSheet", json!({"name": "Three"})).is_err(), "the structure is locked");
        // A second Protect keeps it protected; unprotecting needs the right password.
        assert_eq!(s.execute("review.protectWorkbook", json!({"on": true})).unwrap()["on"], true);
        assert!(s.execute("review.protectWorkbook", json!({})).is_err());
        assert!(s.execute("review.protectWorkbook", json!({"on": false, "password": "pass"})).is_err());
        assert!(s.doc().unwrap().wb.protected_structure);
        s.execute("review.protectWorkbook", json!({"on": false, "password": "pa55"})).unwrap();
        let wb = &s.doc().unwrap().wb;
        assert!(!wb.protected_structure && wb.structure_password.is_none());
        // Without a password it toggles freely.
        s.execute("review.protectWorkbook", json!({})).unwrap();
        s.execute("review.protectWorkbook", json!({})).unwrap();
        assert!(!s.doc().unwrap().wb.protected_structure);
    }

    #[test]
    fn convert_to_comments_turns_every_note_into_a_comment_and_undoes() {
        let mut s = book();
        s.execute("review.newNote", json!({"sheet": 1, "cell": "D4", "text": "second", "author": "Cy"})).unwrap();
        s.execute("review.showNote", json!({"sheet": 1, "cell": "D4"})).unwrap();
        let r = s.execute("review.convertToComments", json!({})).unwrap();
        assert_eq!(r["converted"], 2);
        let wb = &s.doc().unwrap().wb;
        let a2 = &wb.sheets[0].comments[&CellRef::parse("A2").unwrap()];
        let d4 = &wb.sheets[1].comments[&CellRef::parse("D4").unwrap()];
        assert!(a2.threaded && d4.threaded && !d4.visible);
        assert_eq!((a2.author.as_str(), a2.text.as_str(), d4.author.as_str(), d4.text.as_str()), ("Bob", "note", "Cy", "second"));
        // Nothing left to convert; one undo restores the notes.
        assert_eq!(s.execute("review.convertToComments", json!({})).unwrap()["converted"], 0);
        s.execute("edit.undo", json!({})).unwrap();
        assert!(!s.doc().unwrap().wb.sheets[0].comments[&CellRef::parse("A2").unwrap()].threaded);
    }

    #[test]
    fn show_comments_and_show_all_notes_toggle() {
        let mut s = book();
        assert_eq!(s.execute("review.showComments", json!({})).unwrap()["on"], true);
        assert_eq!(s.execute("review.showComments", json!({})).unwrap()["on"], false);
        assert_eq!(s.execute("review.showComments", json!({"on": true})).unwrap()["on"], true);
        assert!(s.comments_pane);
        assert_eq!(s.execute("review.showAllNotes", json!({})).unwrap()["on"], true);
        assert_eq!(s.execute("review.showAllNotes", json!({"on": false})).unwrap()["on"], false);
        assert!(!s.show_all_notes);
    }

    // ------------------------------------------------------------ Check Performance

    fn category_count(report: &Json, id: &str) -> u64 {
        report["categories"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap()["count"].as_u64().unwrap()
    }

    fn cells_with_category(report: &Json, id: &str) -> Vec<String> {
        report["issues"].as_array().unwrap().iter().filter(|i| i["category"] == id).map(|i| i["cell"].as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn check_performance_is_registered_under_review_accessibility() {
        let spec = crate::find_command("review.checkPerformance").expect("registered");
        assert_eq!((spec.label, spec.menu), ("Check Performance", &["Review", "Accessibility"][..]));
        assert!(crate::catalog::CATALOG.iter().any(|(id, tab, group, label)| {
            *id == "review.checkPerformance" && *tab == "Review" && *group == "Accessibility" && *label == "Check Performance"
        }));
        let audit = crate::find_command("review.auditCalculation").expect("extension command registered");
        assert_eq!((audit.label, audit.menu), ("Audit Calculation", &[][..]));
    }

    #[test]
    fn check_performance_reports_nothing_on_an_empty_workbook() {
        let mut s = Session::new();
        s.new_workbook();
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!(
            (r["version"].as_u64(), r["usedCells"].as_u64(), r["optimizableCells"].as_u64(), r["truncated"].as_bool()),
            (Some(1), Some(0), Some(0), Some(false))
        );
        assert_eq!(r["cells"], json!([]));
        assert_eq!(r["shown"], 0);
        assert_eq!(r["sheets"], json!([{"name": "Sheet1", "index": 0, "usedCells": 0, "optimizableCells": 0, "protected": false}]));
        // The Excel-parity response has no calculation-audit categories.
        assert!(r.get("categories").is_none() && r.get("issues").is_none() && r.get("total").is_none());
    }

    #[test]
    fn check_performance_counts_used_and_blank_formatted_cells() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "keep"})).unwrap();
        s.execute("home.bold", json!({"range": "B2"})).unwrap();
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!((r["usedCells"].as_u64(), r["optimizableCells"].as_u64()), (Some(2), Some(1)));
        assert_eq!(r["sheets"][0]["usedCells"], 2);
        assert_eq!(r["sheets"][0]["optimizableCells"], 1);
        let cell = &r["cells"][0];
        assert_eq!((cell["sheet"].as_str(), cell["cell"].as_str(), cell["optimize"].as_str()), (Some("Sheet1"), Some("B2"), Some("clearFormatting")));
        assert!(cell["style"].as_u64().unwrap() > 0);
        assert_eq!(cell["goto"], "'Sheet1'!B2");
    }

    #[test]
    fn check_performance_ignores_notes_comments_and_normal_formulas() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("review.newNote", json!({"cell": "A1", "text": "note"})).unwrap();
        s.execute("review.newComment", json!({"cell": "B2", "text": "comment"})).unwrap();
        s.execute("cell.set", json!({"cell": "C1", "input": "=NOW()"})).unwrap();
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!((r["usedCells"].as_u64(), r["optimizableCells"].as_u64()), (Some(1), Some(0)));
        assert_eq!(r["cells"], json!([]));
        assert_eq!(r["truncated"], false);
    }

    #[test]
    fn check_performance_caps_details_but_counts_every_optimizable_cell() {
        let mut s = Session::new();
        s.new_workbook();
        for cell in ["A1", "A2", "A3"] {
            s.execute("home.bold", json!({"range": cell, "on": true})).unwrap();
        }
        let r = s.execute("review.checkPerformance", json!({"limit": 2})).unwrap();
        assert_eq!((r["usedCells"].as_u64(), r["optimizableCells"].as_u64()), (Some(3), Some(3)));
        assert_eq!((r["shown"].as_u64(), r["truncated"].as_bool(), r["limit"].as_u64()), (Some(2), Some(true), Some(2)));
        assert_eq!(r["cells"].as_array().unwrap().len(), 2);
        // A hostile or huge limit is clamped rather than trusted.
        assert_eq!(s.execute("review.checkPerformance", json!({"limit": 9_999_999})).unwrap()["limit"], PERFORMANCE_MAX_LIMIT);
        assert_eq!(s.execute("review.checkPerformance", json!({"limit": 0})).unwrap()["limit"], 1);
    }

    #[test]
    fn check_performance_optimize_sheet_clears_only_that_sheet_and_reports_counts() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
        for (sheet, cell) in [(0, "A1"), (1, "B2")] {
            s.execute("home.bold", json!({"sheet": sheet, "range": cell, "on": true})).unwrap();
        }
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!((r["usedCells"].as_u64(), r["optimizableCells"].as_u64()), (Some(2), Some(2)));
        assert_eq!((r["sheets"][0]["optimizableCells"].as_u64(), r["sheets"][1]["optimizableCells"].as_u64()), (Some(1), Some(1)));
        let optimized = s.execute("review.optimizePerformance", json!({"sheet": 1})).unwrap();
        assert_eq!((optimized["optimized"].as_u64(), optimized["truncated"].as_bool()), (Some(1), Some(false)));
        assert_eq!(optimized["sheets"], json!([{"name": "Two", "index": 1, "optimized": 1}]));
        assert_eq!(optimized["skippedProtected"], json!([]));
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!((r["sheets"][0]["optimizableCells"].as_u64(), r["sheets"][1]["optimizableCells"].as_u64()), (Some(1), Some(0)));
        let all = s.execute("review.optimizePerformance", json!({})).unwrap();
        assert_eq!(all["optimized"], 1);
        assert_eq!(all["sheets"], json!([{"name": "Sheet1", "index": 0, "optimized": 1}, {"name": "Two", "index": 1, "optimized": 0}]));
        assert_eq!(s.execute("review.checkPerformance", json!({})).unwrap()["optimizableCells"], 0);
    }

    #[test]
    fn check_performance_optimize_is_undoable_and_keeps_values() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("home.bold", json!({"range": "A1"})).unwrap();
        s.execute("review.newComment", json!({"cell": "A1", "text": "keep me"})).unwrap();
        s.execute("range.setValues", json!({"range": "B1", "values": [["keep"]]})).unwrap();
        assert_eq!(s.execute("review.checkPerformance", json!({})).unwrap()["optimizableCells"], 1);
        s.execute("review.optimizePerformance", json!({})).unwrap();
        assert_eq!(s.execute("review.checkPerformance", json!({})).unwrap()["optimizableCells"], 0);
        {
            let sh = s.doc().unwrap().wb.active().unwrap();
            assert!(sh.cell(CellRef::parse("A1").unwrap()).is_none(), "the blank formatted cell is gone");
            assert!(sh.comments.contains_key(&CellRef::parse("A1").unwrap()), "the comment stays");
        }
        assert_eq!(s.doc().unwrap().wb.active().unwrap().value(CellRef::parse("B1").unwrap()).display(), "keep");
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.execute("review.checkPerformance", json!({})).unwrap()["optimizableCells"], 1);
    }

    #[test]
    fn check_performance_skips_cells_inside_merged_ranges() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("home.bold", json!({"range": "A1:B1"})).unwrap();
        s.execute("home.mergeCells", json!({"range": "A1:B1"})).unwrap();
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!(r["optimizableCells"], 0, "merged edge cells carry border/style state");
    }

    #[test]
    fn check_performance_keeps_the_formatting_of_cells_that_show_a_spill() {
        // Formatting A10:A12 over `=SEQUENCE(3)` stores empty cells under the spill: A11 and A12
        // hold no value of their own, but they display 2 and 3. Optimizing them away turned
        // 200.00% and 300.00% into 2 and 3.
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A10", "input": "=SEQUENCE(3)"})).unwrap();
        s.execute("home.numberFormat", json!({"range": "A10:A12", "code": "0.00%"})).unwrap();
        {
            let d = s.doc().unwrap();
            let sh = d.wb.active().unwrap();
            let a11 = CellRef::parse("A11").unwrap();
            assert!(sh.cell(a11).is_some_and(|cell| cell.value.is_empty() && cell.formula.is_none()), "the format stores an empty cell");
            assert_eq!(sh.value(a11), gridcraft_core::Value::Number(2.0));
            assert_eq!(crate::display::cell_text(&d.wb, sh, a11), "200.00%");
        }
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!(r["optimizableCells"], 0, "a cell that displays a spilled value is not blank");
        assert_eq!(r["cells"], json!([]));
        assert_eq!(s.execute("review.optimizePerformance", json!({})).unwrap()["optimized"], 0);
        let d = s.doc().unwrap();
        let sh = d.wb.active().unwrap();
        for (at, text) in [("A11", "200.00%"), ("A12", "300.00%")] {
            let c = CellRef::parse(at).unwrap();
            assert!(sh.cell(c).is_some_and(|cell| cell.style != StyleId::DEFAULT), "{at} keeps its number format");
            assert_eq!(crate::display::cell_text(&d.wb, sh, c), text);
        }
    }

    #[test]
    fn check_performance_counts_but_optimize_skips_a_protected_sheet() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("home.bold", json!({"range": "A1"})).unwrap();
        s.execute("review.protectSheet", json!({})).unwrap();
        let r = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!(r["optimizableCells"], 1);
        assert_eq!(r["sheets"][0]["protected"], true);
        let optimized = s.execute("review.optimizePerformance", json!({"sheet": "Sheet1"})).unwrap();
        assert_eq!((optimized["optimized"].as_u64(), optimized["skippedProtected"].clone()), (Some(0), json!(["Sheet1"])));
        assert_eq!(optimized["sheets"], json!([]));
        assert_eq!(s.execute("review.checkPerformance", json!({})).unwrap()["optimizableCells"], 1);
        s.execute("review.unprotectSheet", json!({})).unwrap();
        assert_eq!(s.execute("review.optimizePerformance", json!({})).unwrap()["optimized"], 1);
    }

    #[test]
    fn audit_calculation_is_opt_in_and_does_not_change_check_performance() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("cell.set", json!({"cell": "A1", "input": "=NOW()+SUM(C:C)"})).unwrap();
        let default = s.execute("review.checkPerformance", json!({})).unwrap();
        assert_eq!((default["optimizableCells"].as_u64(), default["truncated"].as_bool()), (Some(0), Some(false)));
        assert!(default.get("issues").is_none());
        let audit = s.execute("review.auditCalculation", json!({})).unwrap();
        assert_eq!((audit["total"].as_u64(), audit["categories"].as_array().unwrap().len()), (Some(2), AUDIT_CATEGORY_COUNT));
        assert_eq!(category_count(&audit, "volatileFormula"), 1);
        assert_eq!(category_count(&audit, "fullColumnReference"), 1);
    }

    #[test]
    fn audit_calculation_flags_volatile_functions_and_leaves_ordinary_formulas_alone() {
        let mut s = Session::new();
        s.new_workbook();
        for (cell, formula) in [
            ("A1", "=NOW()"),
            ("A2", "=RANDBETWEEN(1,10)"),
            ("A3", "=SUM(B1:B3)"),
            ("A4", "=1+1"),
            ("A5", "=1+1"),
            ("A6", "=TODAY()+OFFSET(B1,0,0)"),
            ("A7", "=INDIRECT(\"B1\")"),
        ] {
            s.execute("cell.set", json!({"cell": cell, "input": formula})).unwrap();
        }
        let r = s.execute("review.auditCalculation", json!({})).unwrap();
        assert_eq!(r["total"], 4);
        assert_eq!(category_count(&r, "volatileFormula"), 4);
        assert_eq!(cells_with_category(&r, "volatileFormula"), ["A1", "A2", "A6", "A7"].map(String::from).to_vec());
        // Repeated identical formulas are legitimate, and a bounded SUM is not a defect.
        assert!(cells_with_category(&r, "volatileFormula").iter().all(|c| c.as_str() != "A3" && c.as_str() != "A4"));
        let today = r["issues"].as_array().unwrap().iter().find(|i| i["cell"] == "A6").unwrap();
        assert_eq!(today["functions"], json!(["OFFSET", "TODAY"]));
        assert_eq!(today["optimize"], Json::Null);
        assert_eq!(today["sheet"], "Sheet1");
        assert_eq!(today["goto"], "'Sheet1'!A6");
    }

    #[test]
    fn audit_calculation_distinguishes_full_column_and_row_references_from_bounded_ranges() {
        let mut s = Session::new();
        s.new_workbook();
        for (cell, formula) in [("A1", "=SUM(C:C)"), ("A2", "=SUM(1:1)"), ("A3", "=SUM(C1:C10)"), ("A4", "=SUM(A1:Z10)")] {
            s.execute("cell.set", json!({"cell": cell, "input": formula})).unwrap();
        }
        let r = s.execute("review.auditCalculation", json!({})).unwrap();
        assert_eq!(r["total"], 2);
        assert_eq!(category_count(&r, "fullColumnReference"), 1);
        assert_eq!(category_count(&r, "fullRowReference"), 1);
        let column = r["issues"].as_array().unwrap().iter().find(|i| i["category"] == "fullColumnReference").unwrap();
        let row = r["issues"].as_array().unwrap().iter().find(|i| i["category"] == "fullRowReference").unwrap();
        assert_eq!((column["cell"].as_str(), column["range"].as_str()), (Some("A1"), Some("C:C")));
        assert_eq!((row["cell"].as_str(), row["range"].as_str()), (Some("A2"), Some("1:1")));
        assert!(column["reason"].as_str().unwrap().contains("1,048,576"));
        assert!(row["reason"].as_str().unwrap().contains("16,384"));
    }

    #[test]
    fn audit_calculation_flags_whole_table_and_oversized_ranges() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("insert.table", json!({"range": "A1:B3", "name": "Data"})).unwrap();
        s.execute("cell.set", json!({"cell": "E1", "input": "=SUM(Data[#All])"})).unwrap();
        s.execute("cell.set", json!({"cell": "A200", "input": "=SUM(A1:XFC62)"})).unwrap();
        let r = s.execute("review.auditCalculation", json!({})).unwrap();
        assert_eq!(category_count(&r, "wholeTableReference"), 1);
        assert_eq!(category_count(&r, "oversizedRange"), 1);
        let table = r["issues"].as_array().unwrap().iter().find(|i| i["category"] == "wholeTableReference").unwrap();
        assert_eq!(table["cell"], "E1");
        assert!(table["formula"].as_str().unwrap().contains("Data[#All]"));
        let oversized = r["issues"].as_array().unwrap().iter().find(|i| i["category"] == "oversizedRange").unwrap();
        assert_eq!(oversized["cell"], "A200");
        assert_eq!(oversized["references"], json!(["A1:XFC62"]));
    }

    #[test]
    fn audit_calculation_caps_returned_issues_but_still_counts_them() {
        let mut s = Session::new();
        s.new_workbook();
        for cell in ["A1", "A2", "A3"] {
            s.execute("cell.set", json!({"cell": cell, "input": "=NOW()"})).unwrap();
        }
        let r = s.execute("review.auditCalculation", json!({"limit": 2})).unwrap();
        assert_eq!((r["total"].as_u64(), r["shown"].as_u64(), r["truncated"].as_bool()), (Some(3), Some(2), Some(true)));
        assert_eq!(r["issues"].as_array().unwrap().len(), 2);
        assert_eq!(category_count(&r, "volatileFormula"), 3);
        assert_eq!(r["limit"], 2);
        // A hostile or huge limit is clamped rather than trusted.
        assert_eq!(s.execute("review.auditCalculation", json!({"limit": 9_999_999})).unwrap()["limit"], PERFORMANCE_MAX_LIMIT);
        assert_eq!(s.execute("review.auditCalculation", json!({"limit": 0})).unwrap()["limit"], 1);
    }
}

#[cfg(test)]
mod edit_range_tests {
    use super::*;

    fn protected_book() -> Session {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("review.allowEditRanges", json!({"action": "add", "title": "Open", "range": "A1:A3"})).unwrap();
        s.execute("review.allowEditRanges", json!({"action": "add", "title": "Budget", "range": "B1:B3,D1", "password": "pa55"})).unwrap();
        s.execute("review.protectSheet", json!({})).unwrap();
        s
    }

    #[test]
    fn edit_ranges_open_locked_cells_and_passwords_really_verify() {
        let mut s = protected_book();
        s.execute("cell.set", json!({"cell": "A2", "input": "1"})).unwrap();
        assert!(s.execute("cell.set", json!({"cell": "C1", "input": "1"})).is_err());
        let err = s.execute("cell.set", json!({"cell": "D1", "input": "1"})).unwrap_err().to_string();
        assert!(err.contains("Budget"), "{err}");
        // A paste or fill is checked the same way.
        assert!(s.execute("range.setValues", json!({"range": "B2", "values": [[5]]})).is_err());
        assert!(s.execute("review.allowEditRanges", json!({"action": "unlock", "title": "Budget", "password": "pass"})).is_err());
        assert!(s.execute("review.allowEditRanges", json!({"action": "unlock", "title": "Budget"})).is_err());
        let list = s.execute("review.allowEditRanges", json!({"action": "unlock", "title": "budget", "password": "pa55"})).unwrap();
        assert_eq!(list[1]["unlocked"], true);
        s.execute("range.setValues", json!({"range": "B2", "values": [[5]]})).unwrap();
        s.execute("cell.set", json!({"cell": "D1", "input": "2"})).unwrap();
        // Ranges can't change while the sheet is protected.
        assert!(s.execute("review.allowEditRanges", json!({"action": "delete", "title": "Open"})).is_err());
    }

    #[test]
    fn edit_ranges_add_modify_delete_list_and_undo() {
        let mut s = Session::new();
        s.new_workbook();
        // Empty params list; nothing to list yet.
        assert_eq!(s.execute("review.allowEditRanges", json!({})).unwrap(), json!([]));
        s.execute("selection.set", json!({"range": "C3:D4"})).unwrap();
        let list = s.execute("review.allowEditRanges", json!({"action": "add"})).unwrap();
        assert_eq!(list, json!([{"title": "Range1", "range": "C3:D4", "password": false, "unlocked": true}]));
        assert!(s.execute("review.allowEditRanges", json!({"action": "add", "title": "range1", "range": "A1"})).is_err());
        let list = s
            .execute("review.allowEditRanges", json!({"action": "modify", "title": "Range1", "newTitle": "Mine", "range": "E5", "password": "x"}))
            .unwrap();
        assert_eq!((list[0]["title"].as_str(), list[0]["range"].as_str(), list[0]["password"].as_bool()), (Some("Mine"), Some("E5"), Some(true)));
        // Inserting a row above moves the range with its cells.
        s.execute("home.insertRows", json!({"rows": "1:1"})).unwrap();
        assert_eq!(s.execute("review.allowEditRanges", json!({})).unwrap()[0]["range"], "E6");
        s.execute("review.allowEditRanges", json!({"action": "delete", "title": "Mine"})).unwrap();
        assert_eq!(s.execute("review.allowEditRanges", json!({})).unwrap(), json!([]));
        s.execute("edit.undo", json!({})).unwrap();
        assert_eq!(s.execute("review.allowEditRanges", json!({})).unwrap()[0]["title"], "Mine");
        assert!(s.execute("review.allowEditRanges", json!({"action": "nope"})).is_err());
        assert!(s.execute("review.allowEditRanges", json!({"action": "delete"})).is_err());
    }
}
