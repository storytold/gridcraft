//! Saved worksheet layout and local PivotTables. No printing, file access, external data
//! refresh, or implicit pivot destination over existing cells.
use super::{Tool, context, string};
use gridcraft_engine::{
    DocState,
    core::RangeRef,
    model::{Orientation, PivotTable, Sheet, Visibility},
    pivot,
};
use serde_json::{Value as Json, json};

#[path = "workbook_data_tools.rs"]
mod data_tools;
#[path = "workbook_drawing.rs"]
mod drawing;

pub(super) fn tools() -> &'static [Tool] {
    &[
        Tool {
            name: "remove_duplicates",
            help: "Keep the first row for each duplicate key in a standalone values-only rectangle. Compacts cells inside the rectangle; references elsewhere still point to their original coordinates. Tables, filters, formulas and Python cells are refused. Without keyColumns compare every column. header=true preserves the first row.",
            command: Some("data.removeDuplicates"),
            required: &["sheet", "range", "header"],
            optional: &["keyColumns"],
        },
        Tool {
            name: "group_outline",
            help: "Add one outline level to explicit whole rows or columns. Example: outlineAxis=rows, outlineSpan=2:8, or columns and C:E.",
            command: Some("data.group"),
            required: &["sheet", "outlineAxis", "outlineSpan"],
            optional: &[],
        },
        Tool {
            name: "ungroup_outline",
            help: "Remove one outline level from explicit whole rows or columns; native ungrouping can reveal hidden detail.",
            command: Some("data.ungroup"),
            required: &["sheet", "outlineAxis", "outlineSpan"],
            optional: &[],
        },
        Tool {
            name: "preview_goal_seek",
            help: "Find a numeric candidate for changingCell to make formulaCell equal targetValue. Runs at most 32 native trial iterations on temporary workbook data. No cell is edited; only propose applying a converged value through set_cell.",
            command: None,
            required: &["sheet", "formulaCell", "targetValue", "changingCell"],
            optional: &[],
        },
        Tool {
            name: "inspect_shapes",
            help: "List shapes and drawn ink on this sheet with IDs, anchors, dimensions, colors and bounded text. Image bytes and ink point lists are not returned.",
            command: None,
            required: &["sheet"],
            optional: &["offset"],
        },
        Tool {
            name: "create_shape",
            help: "Insert native geometry or a text box anchored to a cell on the active sheet. Dimensions are sheet points. Fill and outline accept #RRGGBB.",
            command: Some("insert.shape"),
            required: &["sheet", "at", "shapeKind"],
            optional: &["text", "width", "height", "shapeFill", "shapeLine"],
        },
        Tool {
            name: "add_ink_stroke",
            help: "Draw a freehand stroke on the active sheet. inkPath points are [x,y] offsets in sheet points from the top-left of the at cell, increasing right and down.",
            command: Some("draw.stroke"),
            required: &["sheet", "at", "inkPath"],
            optional: &["color", "penWidth"],
        },
        Tool {
            name: "ink_to_shape",
            help: "Recognize one native ink stroke as geometry using its shape ID on the active sheet.",
            command: Some("draw.inkToShape"),
            required: &["sheet", "shape"],
            optional: &[],
        },
        Tool {
            name: "shape_fill",
            help: "Set the visible fill on a closed geometric shape or text box on the active sheet, by ID and #RRGGBB.",
            command: Some("chart.shapeFill"),
            required: &["sheet", "shape", "color"],
            optional: &[],
        },
        Tool {
            name: "shape_outline",
            help: "Set a shape's outline or stroke color on the active sheet from its ID and #RRGGBB.",
            command: Some("chart.shapeOutline"),
            required: &["sheet", "shape", "color"],
            optional: &[],
        },
        Tool {
            name: "edit_shape_text",
            help: "Replace literal text in an existing text box or WordArt on the active sheet, by ID.",
            command: Some("shape.setText"),
            required: &["sheet", "shape", "text"],
            optional: &[],
        },
        Tool {
            name: "inspect_layout",
            help: "Inspect saved print settings and frozen row/column counts without printing or generating pages.",
            command: None,
            required: &["sheet"],
            optional: &[],
        },
        Tool {
            name: "set_page_layout",
            help: "Set only the supplied saved print settings. printArea takes one local A1 rectangle; an empty string clears it. scalePercent and fit-to dimensions are alternatives; a zero fit dimension means automatic.",
            command: Some("pageLayout.pageSetup"),
            required: &["sheet"],
            optional: &["orientation", "paper", "printArea", "scalePercent", "fitWidth", "fitHeight", "centerH", "centerV", "gridlines", "headings"],
        },
        Tool {
            name: "freeze_panes",
            help: "Freeze the rows above and columns left of cell. B2 freezes the first row and first column; A2 freezes only the first row; A1 clears the dividers.",
            command: Some("view.freezePanes"),
            required: &["sheet", "cell"],
            optional: &[],
        },
        Tool {
            name: "unfreeze_panes",
            help: "Remove both frozen pane dividers from the given sheet.",
            command: Some("view.unfreezePanes"),
            required: &["sheet"],
            optional: &[],
        },
        Tool {
            name: "create_pivot",
            help: "Summarize a labeled local source rectangle in a new worksheet (no overwrite). rowField, optional columnField, and valueField are source column headers; aggregate defaults to sum.",
            command: Some("insert.pivotTable"),
            required: &["sheet", "source", "rowField", "valueField"],
            optional: &["name", "columnField", "aggregate"],
        },
        Tool {
            name: "inspect_pivots",
            help: "List PivotTables on this sheet with IDs, report ranges, axes and aggregation. Inspect the report range for its actual values.",
            command: None,
            required: &["sheet"],
            optional: &["offset"],
        },
        Tool {
            name: "set_pivot_layout",
            help: "Change an existing PivotTable report to compact, outline, or tabular, optionally adjusting subtotals. Requires the report sheet and ID from inspect_pivots. Existing cells outside the report will not be overwritten.",
            command: Some("pivot.layout"),
            required: &["sheet", "pivot", "reportLayout"],
            optional: &["subtotals"],
        },
    ]
}

pub(super) fn field(key: &str) -> Option<Json> {
    Some(match key {
        "orientation" => json!({"type":"string","enum":["portrait","landscape"]}),
        "paper" | "rowField" | "columnField" | "valueField" => string(255),
        "printArea" => string(42),
        "scalePercent" => json!({"type":"integer","minimum":10,"maximum":400}),
        "fitWidth" | "fitHeight" => json!({"type":"integer","minimum":0,"maximum":32767}),
        "centerH" | "centerV" | "headings" => json!({"type":"boolean"}),
        "aggregate" => json!({"type":"string","enum":["sum","count","average","min","max","product","countNumbers","stddev","stddevp","var","varp"]}),
        "pivot" => json!({"type":"integer","minimum":0,"maximum":u32::MAX}),
        "reportLayout" => json!({"type":"string","enum":["compact","outline","tabular"]}),
        "subtotals" => json!({"type":"string","enum":["top","bottom","none"]}),
        _ => return drawing::field(key).or_else(|| data_tools::field(key)),
    })
}

fn visible_source<'a>(d: &'a DocState, sh: &Sheet, pt: &PivotTable) -> Option<(&'a Sheet, RangeRef)> {
    if pt.source.len() > 2048 {
        return None;
    }
    let (i, r) = pivot::resolve_source(&d.wb, &pt.source, d.wb.sheet_index(&sh.name)?).ok()?;
    Some((d.wb.sheet(i).filter(|s| s.visibility == Visibility::Visible)?, r))
}

pub(super) fn query(d: &DocState, sh: &Sheet, name: &str, p: &Json) -> Result<Json, String> {
    match name {
        "inspect_layout" => {
            let pr = &sh.print;
            let (rows, cols) = sh.freeze.unwrap_or_default();
            Ok(json!({"sheet":context::clip(&sh.name,128),"frozenRows":rows,"frozenColumns":cols,
                "orientation":if pr.orientation==Orientation::Landscape {"landscape"}else{"portrait"},"paper":context::clip(&pr.paper,128),
                "printArea":pr.print_area.map(|r|r.a1()),"scalePercent":pr.scale,
                "fitWidth":pr.fit_to.map(|f|f.0),"fitHeight":pr.fit_to.map(|f|f.1),
                "centerH":pr.center_h,"centerV":pr.center_v,"gridlines":pr.gridlines,"headings":pr.headings}))
        }
        "inspect_pivots" => {
            let offset = p["offset"].as_u64().unwrap_or(0) as usize;
            Ok(context::page(sh.pivots.iter().skip(offset).map(|pt|{
                let names=|axis:&[gridcraft_engine::model::PivotField]| axis.iter().take(16).map(|f|context::clip(&f.source_col,128)).collect::<Vec<_>>();
                json!({"id":pt.id,"name":context::clip(&pt.name,128),"anchor":pt.anchor.a1(),"reportRange":pt.last_range.map(|r|r.a1()),
                    "source":visible_source(d,sh,pt).map(|(s,r)|json!({"sheet":context::clip(&s.name,128),"range":r.a1()})),
                    "reportLayout":format!("{:?}",pt.layout).to_ascii_lowercase(),"rows":names(&pt.rows),"columns":names(&pt.columns),
                    "values":pt.values.iter().take(16).map(|v|json!({"field":context::clip(&v.source_col,128),"aggregate":format!("{:?}",v.func)})).collect::<Vec<_>>(),
                    "fieldsTruncated":pt.rows.len()>16 || pt.columns.len()>16 || pt.values.len()>16})
            }), offset, 12))
        }
        "inspect_shapes" => Ok(drawing::inspect(d, sh, p)),
        "preview_goal_seek" => data_tools::preview(d, sh, p),
        _ => Err("Unknown layout inspection.".into()),
    }
}

pub(super) fn prepare(d: &DocState, sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    match name {
        "set_page_layout" => {
            if p.as_object().is_none_or(|o| o.len() < 2) {
                return Err("Supply at least one print setting.".into());
            }
            if let Some(area) = p["printArea"].as_str()
                && !area.is_empty()
                && RangeRef::parse(area).is_none()
            {
                return Err("printArea must be a local A1 rectangle or an empty string.".into());
            }
            if let Some(scale) = p["scalePercent"].as_u64() {
                if p.get("fitWidth").is_some() || p.get("fitHeight").is_some() {
                    return Err("Choose percent scale or fit-to-page dimensions, not both.".into());
                }
                p["scale"] = json!(scale);
                p.as_object_mut().ok_or("Invalid settings object.")?.remove("scalePercent");
            }
        }
        "freeze_panes" | "unfreeze_panes" => {}
        "create_pivot" => {
            if d.wb.protected_structure {
                return Err("The workbook structure is protected.".into());
            }
            let r = p["source"].as_str().and_then(RangeRef::parse).ok_or("Supply a local A1 source rectangle with a header row.")?;
            if r.height() < 2 || r.count() > 65_536 {
                return Err("Pivot source needs headers and data, at most 65,536 cells.".into());
            }
            let row = p["rowField"].as_str().ok_or("Missing rowField")?;
            let value = p["valueField"].as_str().ok_or("Missing valueField")?;
            let col = p["columnField"].as_str();
            if row.trim().is_empty() || value.trim().is_empty() || col.is_some_and(|s| s.trim().is_empty() || s.eq_ignore_ascii_case(row)) {
                return Err("Specify nonblank header names; rowField and columnField must differ.".into());
            }
            let mut native = json!({"source":pivot::range_text(&sh.name,r),"destination":"new","rows":[row],
                "values":[{"field":value,"func":p["aggregate"].as_str().unwrap_or("sum")}]});
            if let Some(col) = col {
                native["columns"] = json!([col]);
            }
            if let Some(name) = p["name"].as_str() {
                native["name"] = json!(name);
            }
            *p = native;
        }
        "set_pivot_layout" => {
            let id = p["pivot"].as_u64().and_then(|i| u32::try_from(i).ok()).ok_or("Invalid pivot ID.")?;
            let pt = sh.pivots.iter().find(|pt| pt.id == id).ok_or("No such PivotTable on this sheet.")?;
            if d.wb.sheets.iter().flat_map(|s| &s.pivots).filter(|x| x.id == id).take(2).count() != 1 {
                return Err("Ambiguous PivotTable ID.".into());
            }
            let (_, r) = visible_source(d, sh, pt).ok_or("A visible local PivotTable source is required.")?;
            if r.count() > 65_536 || pt.last_range.is_some_and(|r| r.count() > 65_536) {
                return Err("Report exceeds the 65,536-cell Wingman pivot limit.".into());
            }
            let layout = p.as_object_mut().and_then(|o| o.remove("reportLayout")).ok_or("Missing reportLayout")?;
            p["layout"] = layout;
        }
        "remove_duplicates" | "group_outline" | "ungroup_outline" => return data_tools::prepare(sh, name, p),
        _ => return drawing::prepare(d, sh, name, p),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_engine::Session;

    #[test]
    fn explicit_pivot_source_is_qualified_and_destination_cannot_overwrite() {
        let mut s = Session::new();
        s.new_workbook();
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let mut p = json!({"sheet":sh.name,"source":"B3:D9","rowField":"Area","valueField":"Items","aggregate":"count"});
        prepare(d, sh, "create_pivot", &mut p).unwrap();
        assert_eq!(p["source"], pivot::range_text(&sh.name, RangeRef::parse("B3:D9").unwrap()));
        assert_eq!(p["values"], json!([{"field":"Items","func":"count"}]));
        assert_eq!(p["destination"], "new");
        assert!(p.get("replace").is_none());
        assert!(prepare(d, sh, "set_page_layout", &mut json!({"sheet":sh.name,"printArea":"Other!A1:B9"})).is_err());
    }

    #[test]
    fn ink_offsets_use_sheet_dimensions_and_reject_malformed_points() {
        let mut s = Session::new();
        s.new_workbook();
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        let mut p = json!({"sheet":sh.name,"at":"B3","inkPath":[[0,0],[11,17]]});
        prepare(d, sh, "add_ink_stroke", &mut p).unwrap();
        assert_eq!(p["points"], json!([[sh.col_left(1), sh.row_top(2)], [sh.col_left(1) + 11.0, sh.row_top(2) + 17.0]]));
        assert!(prepare(d, sh, "add_ink_stroke", &mut json!({"at":"A1","inkPath":[[3],[4,5]]})).is_err());
    }
}
