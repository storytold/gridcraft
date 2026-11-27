//! Dry-run formula entry and chart inspection. Use the native cell editor and Recommended
//! Charts rules instead of a second parser or model-generated chart heuristic.
use super::context;
use crate::SheetApp;
use gridcraft_engine::{
    DocState, calc, cmd,
    core::{CellRef, MAX_COLS, RangeRef},
    formula::{self, Expr, SheetSel},
    model::{Sheet, Visibility, Workbook},
};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

fn chart_range(p: &Json) -> Result<RangeRef, String> {
    let r = p["range"].as_str().and_then(RangeRef::parse).ok_or("Supply an unqualified chart source, e.g. B2:F40.")?;
    if r.count() > 100_000 {
        return Err("Chart source exceeds 100,000 cells. Specify the populated rectangle.".into());
    }
    Ok(r)
}

pub(super) fn prepare_chart(d: &DocState, sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    if name == "update_chart" && p.get("type").is_none() && p.get("subtype").is_some() {
        use gridcraft_engine::model::ChartKind::*;
        let chart = sh.charts.iter().find(|c| Some(u64::from(c.id)) == p["chart"].as_u64()).ok_or("No such chart on this worksheet.")?;
        // chart.set consumes subtype only together with type.
        p["type"] = json!(match chart.kind {
            ColumnClustered | ColumnStacked | ColumnStacked100 => "column",
            BarClustered | BarStacked | BarStacked100 => "bar",
            Line | LineMarkers | LineStacked | LineStacked100 | LineStacked100Markers => "line",
            Area | AreaStacked | AreaStacked100 => "area",
            Scatter | ScatterLines => "scatter",
            _ => return Err("This chart type has no configurable subtype.".into()),
        });
    }
    let typ = p["type"].as_str();
    if let Some(sub) = p["subtype"].as_str() {
        let valid = match typ {
            Some("column" | "bar") => matches!(sub, "clustered" | "stacked" | "stacked100"),
            Some("line") => matches!(sub, "markers" | "stacked" | "stacked100" | "stacked100Markers"),
            Some("area") => matches!(sub, "stacked" | "stacked100"),
            Some("scatter") => sub == "lines",
            _ => false,
        };
        if !valid {
            return Err("The subtype must match the chart type; omit subtype for auto/default.".into());
        }
    }
    match name {
        "create_chart" => {
            let source = chart_range(p)?;
            if source.end.col + 2 >= MAX_COLS && p.get("at").is_none() {
                p["at"] = json!(source.start.a1());
            }
        }
        "update_chart" => {
            if !d.wb.active().is_some_and(|s| s.uid == sh.uid) {
                return Err("Chart updates apply only to the active worksheet.".into());
            }
            if typ == Some("auto") {
                return Err("Use recommend_charts first; updating an existing chart requires an explicit type.".into());
            }
            if !sh.charts.iter().any(|c| Some(u64::from(c.id)) == p["chart"].as_u64()) {
                return Err("No such chart on this worksheet.".into());
            }
        }
        _ => return Err("Unknown chart edit.".into()),
    }
    Ok(())
}

// Imported series can carry paths to other files or inline data caches. Show only bounded
// local references into visible worksheets, never the external/cache strings.
pub(super) fn visible_ref<'a>(wb: &Workbook, source: Option<&'a str>) -> Option<&'a str> {
    let s = source.filter(|s| s.len() <= 256)?;
    let Expr::Ref(r) = formula::parse(s).ok()? else { return None };
    match r.sheet {
        SheetSel::Current => Some(s),
        SheetSel::Named(n) if wb.sheet_index(&n).and_then(|i| wb.sheet(i)).is_some_and(|s| s.visibility == Visibility::Visible) => Some(s),
        _ => None,
    }
}

pub(super) fn query(app: &SheetApp, name: &str, p: &Json) -> Result<Json, String> {
    let d = app.session.active().ok_or("No open workbook")?;
    let sh = context::sheet(d, p)?;
    match name {
        "check_formula" => lint(d, sh, p),
        "recommend_charts" => {
            let range = chart_range(p)?;
            let proposals = cmd::insert::recommend_charts(&app.session, p).map_err(|e| e.to_string())?;
            let recommendations: Vec<_> = proposals
                .into_iter()
                .map(|(kind, sub, label)| {
                    let mut v = json!({"type":kind,"label":label});
                    if !sub.is_empty() {
                        v["subtype"] = json!(sub);
                    }
                    v
                })
                .collect();
            Ok(json!({"sheet":sh.name,"range":range.a1(),"recommendations":recommendations}))
        }
        "inspect_chart" => {
            let c = sh.charts.iter().find(|c| Some(u64::from(c.id)) == p["chart"].as_u64()).ok_or("No such chart on this worksheet.")?;
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            let title = |t: Option<&str>| t.map(|s| context::clip(s, 256));
            let page = context::page(
                c.series.iter().enumerate().skip(offset).map(|(i, s)| {
                    json!({
                        "index":i,"kind":s.kind,"secondaryAxis":s.secondary,"smooth":s.smooth,
                        "nameRef":visible_ref(&d.wb,s.name.as_deref()),"valuesRef":visible_ref(&d.wb,Some(&s.values)),
                        "categoryRef":visible_ref(&d.wb,s.categories.as_deref()),"bubbleSizesRef":visible_ref(&d.wb,s.bubble_sizes.as_deref()),
                    })
                }),
                offset,
                12,
            );
            Ok(json!({"sheet":sh.name,"chart":c.id,"kind":c.kind,"source":visible_ref(&d.wb,c.source.as_deref()),
                "byRows":c.by_rows,"title":title(c.title.as_deref()),"xTitle":title(c.x_title.as_deref()),"yTitle":title(c.y_title.as_deref()),
                "legend":c.legend,"dataLabels":c.data_labels,"gridlines":c.gridlines,"at":c.anchor.cell.a1(),
                "width":c.anchor.width,"height":c.anchor.height,"seriesCount":c.series.len(),"series":page}))
        }
        _ => Err("Unknown analysis tool.".into()),
    }
}

fn lint(d: &DocState, sh: &Sheet, p: &Json) -> Result<Json, String> {
    let f = p["formula"].as_str().filter(|s| s.starts_with('=')).ok_or("Start the formula with =, as for set_cell.")?;
    let at = p["cell"].as_str().map(|s| CellRef::parse(s).ok_or("Use a single unqualified A1 cell.")).transpose()?;
    let as_text = at.is_some_and(|c| d.wb.styles.get(sh.style_id(c)).num_fmt.as_str() == "@");
    let strict = formula::parse(f).err();
    let mut out = json!({"validSyntax":strict.is_none(),"evaluated":false,"wouldStoreAsText":as_text,"sheet":sh.name});
    if let Some(e) = &strict {
        out["positionInExpression"] = json!(e.pos);
    }
    let expr = match cmd::edit::validate_formula_entry(f) {
        Ok(e) => e,
        Err(e) => {
            out["acceptedAsFormula"] = json!(false);
            out["message"] = json!(e.to_string());
            return Ok(out);
        }
    };
    out["acceptedAsFormula"] = json!(!as_text);
    out["operandCheck"] = operand_check(&expr, d, sh);
    if strict.is_some() {
        out["autoClosedParentheses"] = json!(true);
    }
    if as_text {
        out["message"] = json!("The target uses the Text number format: entry would store the input literally.");
    }
    let mut identifiers = BTreeSet::new();
    let mut local_bindings = false;
    let mut has_ref_error = false;
    let mut self_reference = false;
    let mut more = false;
    expr.walk(&mut |node| {
        let (kind, name) = match node {
            Expr::Error(gridcraft_engine::core::CellError::Ref) => {
                has_ref_error = true;
                return;
            }
            Expr::Call(n, _) if matches!(n.as_str(), "LET" | "LAMBDA") => {
                local_bindings = true;
                return;
            }
            Expr::Call(n, _) if !calc::is_known_function(n) => ("function", n),
            Expr::Name(n) => ("name", n),
            Expr::Struct(r) if !r.table.is_empty() => ("table", &r.table),
            Expr::Ref(r) => {
                let here = matches!(r.sheet, SheetSel::Current) || matches!(&r.sheet,SheetSel::Named(n) if n.eq_ignore_ascii_case(&sh.name));
                self_reference |= here && at.is_some_and(|c| r.range().contains(c));
                if let SheetSel::Named(n) = &r.sheet {
                    ("sheet", n)
                } else {
                    return;
                }
            }
            _ => return,
        };
        if identifiers.len() < 24 {
            identifiers.insert((kind, name));
        } else {
            more = true;
        }
    });
    let si = d.wb.sheet_index(&sh.name).ok_or("Unknown worksheet")?;
    let mut unresolved = Vec::new();
    // Scan visible catalogs once. Never inspect name definitions. Partial scans and locally
    // bound LAMBDA/LET names cannot establish unknown identifiers.
    let mut budget = 4096_usize;
    if !identifiers.is_empty() {
        for n in &d.wb.names {
            if budget == 0 {
                break;
            }
            budget -= 1;
            if !n.hidden && n.scope.is_none_or(|i| i == si) {
                identifiers.retain(|(kind, name)| !matches!(*kind, "function" | "name") || !name.eq_ignore_ascii_case(&n.name));
            }
            if identifiers.is_empty() {
                break;
            }
        }
        'sheets: for s in &d.wb.sheets {
            if identifiers.is_empty() || budget == 0 {
                break;
            }
            budget -= 1;
            if s.visibility != Visibility::Visible {
                continue;
            }
            identifiers.retain(|(kind, name)| *kind != "sheet" || !name.eq_ignore_ascii_case(&s.name));
            for t in &s.tables {
                if budget == 0 {
                    break 'sheets;
                }
                budget -= 1;
                identifiers.retain(|(kind, name)| !matches!(*kind, "table" | "name") || !name.eq_ignore_ascii_case(&t.name));
                if identifiers.is_empty() {
                    break 'sheets;
                }
            }
        }
    }
    if budget > 0 {
        for (kind, name) in identifiers {
            if !local_bindings || matches!(kind, "table" | "sheet") {
                unresolved.push(json!({"kind":kind,"name":context::clip(name,128)}));
            }
        }
    }
    out["unresolvedInVisibleCatalog"] = json!(unresolved);
    out["catalogCheckIncomplete"] = json!(budget == 0 || more);
    if local_bindings {
        out["localNamesNotResolved"] = json!(true);
    }
    if has_ref_error {
        out["brokenReferenceLiteral"] = json!(true);
    }
    if self_reference {
        out["referencesTargetDirectly"] = json!(true);
    }
    Ok(out)
}

// Advisory, not a speculative evaluation: use the evaluator's actual numeric coercion for
// direct scalar operands. A reference in an IF branch may never be evaluated at runtime.
fn operand_check(expr: &Expr, d: &DocState, sh: &Sheet) -> Json {
    use gridcraft_engine::{core::Value, formula::BinOp};
    let mut warnings = Vec::new();
    let mut examined = 0;
    let mut limited = false;
    expr.walk(&mut |node| {
        let Expr::Binary(op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Pow), left, right) = node else {
            return;
        };
        for (side, arg) in [("left", left), ("right", right)] {
            if examined >= 128 || warnings.len() >= 16 {
                limited = true;
                return;
            }
            let Expr::Ref(r) = arg.as_ref() else {
                continue;
            };
            if !r.range().is_single() {
                continue;
            }
            examined += 1;
            let source = match &r.sheet {
                SheetSel::Current => sh,
                SheetSel::Named(n) => {
                    let Some(s) = d.wb.sheet_index(n).and_then(|i| d.wb.sheet(i)).filter(|s| s.visibility == Visibility::Visible) else {
                        continue;
                    };
                    s
                }
                _ => continue,
            };
            let at = r.range().start;
            let value = source.value_ref(at).unwrap_or(&Value::Empty);
            if matches!(value, Value::Array(_)) {
                continue;
            }
            if matches!(value,Value::Text(s) if s.len()>4096) {
                limited = true;
                continue;
            }
            let problem = match value.to_number_in(d.wb.date_system) {
                Err(e) => e.to_string(),
                Ok(0.0) if *op == BinOp::Div && side == "right" => "#DIV/0!".into(),
                _ => continue,
            };
            warnings.push(json!({"sheet":source.name,"cell":at.a1(),"operator":op.symbol(),"operand":side,"ifEvaluated":problem}));
        }
    });
    json!({"warnings":warnings,"limited":limited,"basis":"Direct arithmetic operands, cached visible scalar values only; not execution or full type inference."})
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;

    fn app() -> SheetApp {
        let mut s = Session::new();
        s.new_workbook();
        SheetApp::new(s, Default::default())
    }

    #[test]
    fn lint_separates_grammar_entry_and_resolution_without_calculating() {
        let app = app();
        let revision = app.session.active().unwrap().revision;
        let run = |f: &str| query(&app, "check_formula", &json!({"formula":f})).unwrap();
        let wrong = run("=ABS(1,2)");
        assert_eq!(wrong["validSyntax"], true);
        assert_eq!(wrong["acceptedAsFormula"], false);
        assert!(wrong["message"].as_str().unwrap().contains("ABS"));
        let auto = run("=SUM(1,2");
        assert_eq!(auto["validSyntax"], false);
        assert_eq!(auto["autoClosedParentheses"], true);
        assert_eq!(auto["acceptedAsFormula"], true);
        let unknown = run("=NO_SUCH_FUNCTION(2)");
        assert_eq!(unknown["unresolvedInVisibleCatalog"][0]["kind"], "function");
        assert_eq!(run("=LostSheet!B3")["unresolvedInVisibleCatalog"][0]["kind"], "sheet");
        assert_eq!(query(&app, "check_formula", &json!({"formula":"=SUM(A1:A3)","cell":"A2"})).unwrap()["referencesTargetDirectly"], true);
        assert_eq!(run("=LET(localAmount,2,localAmount+1)")["unresolvedInVisibleCatalog"], json!([]));
        assert_eq!(run("=PY(\"x\")")["acceptedAsFormula"], false);
        assert_eq!(app.session.active().unwrap().revision, revision);
    }

    #[test]
    fn recommended_type_is_the_inserted_type_and_readback_resolves_series() {
        let mut app = app();
        app.run("range.setValues", json!({"range":"A1","values":[[1,2],[2,4],[3,6]]})).unwrap();
        let reco = query(&app, "recommend_charts", &json!({"range":"A1:B3"})).unwrap();
        assert_eq!(reco["recommendations"][0]["type"], "scatter");
        let id = app.run("insert.chart", json!({"range":"A1:B3","type":"auto"})).unwrap()["chart"].clone();
        let sh = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let view = query(&app, "inspect_chart", &json!({"sheet":sh,"chart":id})).unwrap();
        assert_eq!(view["seriesCount"], 1);
        let first = &view["series"]["items"][0];
        assert!(first["categoryRef"].as_str().unwrap().contains("$A$1:$A$3"));
        assert!(first["valuesRef"].as_str().unwrap().contains("$B$1:$B$3"));
        assert!(visible_ref(&app.session.active().unwrap().wb, Some("[other.xlsx]Sheet1!A1")).is_none());
        let subtype = json!({"sheet":sh,"chart":id,"subtype":"lines"});
        super::super::execute(&mut app, "update_chart", &subtype, true).unwrap();
        assert_eq!(query(&app, "inspect_chart", &subtype).unwrap()["kind"], "ScatterLines");
        app.run("home.insertSheet", json!({"name":"PrivateData"})).unwrap();
        app.run("sheet.hide", json!({"sheet":"PrivateData"})).unwrap();
        let wb = std::sync::Arc::make_mut(&mut app.session.active_mut().unwrap().wb);
        wb.sheet_mut(0).unwrap().charts[0].source = Some("PrivateData!A1:B3".into());
        let listing = context::mention(&app, "@charts").unwrap();
        assert!(listing["page"]["items"][0]["id"] == id);
        assert!(listing["page"]["items"][0]["source"].is_null());
    }
}
