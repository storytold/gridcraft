//! Inspect the existing calculation graph without recalculating, expanding ranges, selecting
//! another sheet, or following links recursively.
use super::context;
use crate::SheetApp;
use gridcraft_engine::{
    DocState,
    core::{CellRef, RangeRef, split_sheet},
    model::{Sheet, Visibility},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const SCAN: usize = 2048;
const PAGE: usize = 24;
const PAGE_BYTES: usize = 14 * 1024;

fn hidden(d: &DocState, sh: &Sheet, at: CellRef) -> bool {
    sh.is_protected() && d.wb.styles.get(sh.style_id(at)).protection.hidden
}

pub(super) fn query(app: &SheetApp, args: &Value) -> Result<Value, String> {
    let d = app.session.active().ok_or("No open workbook")?;
    let (qualifier, local) = split_sheet(args["cell"].as_str().ok_or("Specify one A1 cell.")?);
    let at = CellRef::parse(local).ok_or("Expected one A1 cell, optionally qualified by worksheet.")?;
    let idx = |name: &str| d.wb.sheet_index(name).ok_or("No such worksheet");
    let explicit = args["sheet"].as_str().map(idx).transpose()?;
    let qualified = qualifier.as_deref().map(idx).transpose()?;
    if matches!((explicit,qualified), (Some(a),Some(b)) if a != b) {
        return Err("Worksheet and qualified cell do not agree.".into());
    }
    let si = explicit.or(qualified).unwrap_or_else(|| d.wb.active_sheet());
    let sh = d.wb.sheet(si).filter(|s| s.visibility == Visibility::Visible).ok_or("Worksheet is not visible.")?;
    let direction = args["direction"].as_str().unwrap_or("precedents");
    if !matches!(direction, "precedents" | "dependents") {
        return Err("Choose precedents or dependents.".into());
    }
    let offset = args["offset"].as_u64().unwrap_or(0);
    if offset > SCAN as u64 {
        return Err(format!("Offset exceeds the {SCAN}-entry inspection window."));
    }
    let mut out = json!({"sheet":sh.name,"cell":context::cell_data(d,sh,at,false),"direction":direction,
        "calculationMode":d.wb.calc.mode,"scope":"Direct static references from the existing graph only. Values are cached. Dynamic or external references may not be represented.",
        "scanTruncated":false,"page":{"items":[],"nextOffset":null}});
    if hidden(d, sh, at) {
        out["indexState"] = json!("hidden");
        return Ok(out);
    }
    let graph = &d.calc.graph;
    // Only visible edges participate in pagination. Sort and deduplicate just the bounded slice.
    let mut found = BTreeSet::new();
    if direction == "precedents" {
        let mut origin = at;
        if sh.cell(at).and_then(|c| c.formula.as_ref()).is_none() && sh.spill.contains_key(&at) {
            if let Some((&anchor, _)) = sh.spill_ranges.iter().take(SCAN).find(|(a, r)| **a != at && r.contains(at)) {
                if hidden(d, sh, anchor) {
                    out["indexState"] = json!("hidden");
                    return Ok(out);
                }
                origin = anchor;
                out["formulaAnchor"] = context::cell_data(d, sh, anchor, false);
            } else {
                out["indexState"] = json!("spillOwnerNotResolved");
                out["scanTruncated"] = json!(sh.spill_ranges.len() > SCAN);
                return Ok(out);
            }
        }
        let Some((areas, dynamic)) = graph.precedent_info((si, origin)) else {
            out["indexState"] = json!(if sh.cell(origin).is_some_and(|c| c.formula.is_some()) { "unindexed" } else { "noFormula" });
            return Ok(out);
        };
        out["indexState"] = json!("indexed");
        out["dynamic"] = json!(dynamic);
        out["scanTruncated"] = json!(areas.len() > SCAN);
        for a in areas.iter().take(SCAN) {
            if d.wb.sheet(a.sheet).is_some_and(|s| s.visibility == Visibility::Visible) {
                found.insert((a.sheet, a.range.start, a.range.end));
            }
        }
    } else {
        out["indexState"] = json!(if graph.is_empty() { "emptyOrNotBuilt" } else { "available" });
        let complete = graph.visit_dependents_bounded(si, at, SCAN, |(i, c)| {
            // The relationship itself could reveal a protected formula.
            if d.wb.sheet(i).is_some_and(|s| s.visibility == Visibility::Visible && !hidden(d, s, c)) {
                found.insert((i, c, c));
            }
        });
        out["scanTruncated"] = json!(!complete);
    }
    let mut items = Vec::new();
    let mut bytes = 0;
    for &(i, start, end) in found.iter().skip(offset as usize).take(PAGE) {
        let Some(s) = d.wb.sheet(i) else { continue };
        let mut entry = json!({"sheet":s.name,"range":RangeRef::new(start,end).a1()});
        if start == end {
            entry["cell"] = context::cell_data(d, s, start, false);
        }
        let n = entry.to_string().len();
        if bytes + n > PAGE_BYTES && !items.is_empty() {
            break;
        }
        bytes += n;
        items.push(entry);
    }
    let end = offset as usize + items.len();
    out["page"] = json!({"items":items,"nextOffset":(end<found.len()).then_some(end)});
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;

    #[test]
    fn cross_sheet_trace_uses_indexed_ranges_and_masks_hidden_edges_without_selecting() {
        let mut session = Session::new();
        session.new_workbook();
        for (cmd, args) in [
            ("sheet.rename", json!({"name":"Source Data"})),
            ("cell.set", json!({"cell":"A2","input":"3"})),
            ("home.insertSheet", json!({})),
            ("sheet.rename", json!({"name":"Totals"})),
            ("cell.set", json!({"cell":"B3","input":"=SUM('Source Data'!A1:A4)"})),
            ("cell.set", json!({"cell":"D2","input":"=INDIRECT(\"B3\")"})),
        ] {
            session.execute(cmd, args).unwrap();
        }
        let mut app = SheetApp::new(session, Default::default());
        let before = app.session.active().unwrap().revision;
        let p = query(&app, &json!({"cell":"Totals!B3"})).unwrap();
        assert_eq!(p["page"]["items"], json!([{"sheet":"Source Data","range":"A1:A4"}]));
        assert_eq!(p["cell"]["value"].as_f64(), Some(3.0));
        let d = query(&app, &json!({"cell":"'Source Data'!A2","direction":"dependents"})).unwrap();
        assert_eq!(d["page"]["items"][0]["cell"]["cell"], "B3");
        assert_eq!(query(&app, &json!({"cell":"D2"})).unwrap()["dynamic"], true);
        assert!(query(&app, &json!({"cell":"'Source Data'!A2","sheet":"Totals"})).is_err());
        assert!(query(&app, &json!({"cell":"B3","offset":999999999})).is_err());
        let doc = app.session.active().unwrap();
        assert_eq!(doc.revision, before);
        assert_eq!(doc.wb.active().unwrap().name, "Totals");
        app.session.execute("home.formatCells", json!({"range":"B3","style":{"protection":{"hidden":true}}})).unwrap();
        app.session.execute("review.protectSheet", json!({})).unwrap();
        let masked = query(&app, &json!({"cell":"B3"})).unwrap();
        assert_eq!(masked["indexState"], "hidden");
        assert!(masked["cell"].get("formula").is_none());
        assert_eq!(query(&app, &json!({"cell":"'Source Data'!A2","direction":"dependents"})).unwrap()["page"]["items"], json!([]));
        app.session.execute("review.unprotectSheet", json!({})).unwrap();
        app.session.execute("sheet.hide", json!({"sheet":"Source Data"})).unwrap();
        assert_eq!(query(&app, &json!({"cell":"B3"})).unwrap()["page"]["items"], json!([]));
        assert!(query(&app, &json!({"cell":"'Source Data'!A2"})).is_err());
    }

    #[test]
    fn bounded_reverse_lookup_reports_incomplete_even_when_output_is_small() {
        let mut s = Session::new();
        s.new_workbook();
        for (cell, input) in [("H1", "=SUM(A1:B5)"), ("H2", "=SUM(A1:EZ5)"), ("H3", "=B4")] {
            s.execute("cell.set", json!({"cell":cell,"input":input})).unwrap();
        }
        let g = &s.active().unwrap().calc.graph;
        let at = CellRef::parse("B4").unwrap();
        assert!(!g.visit_dependents_bounded(0, at, 0, |_| {}));
        let mut limited = Vec::new();
        assert!(g.visit_dependents_bounded(0, at, SCAN, |key| limited.push(key)));
        let mut full = Vec::new();
        g.dependents(0, at, &mut full);
        limited.sort();
        full.sort();
        assert_eq!(limited, full);
        assert_eq!(limited.len(), 3);
    }
}
