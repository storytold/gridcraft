//! Explicit, non-toggling format commands. All writes still go through their ribbon command;
//! the extra guards prevent a model from discarding covered cells or resizing adjacent merges.
use super::{context, object, string};
use gridcraft_engine::{
    DocState,
    core::{CellRef, RangeRef},
    model::{Sheet, StyleId},
};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

const MAX_FORMAT_CELLS: u64 = 100_000;

pub(super) fn field(key: &str) -> Json {
    let list = |v: &[&str]| json!({"type":"string","enum":v});
    let boolean = || json!({"type":"boolean"});
    match key {
        "preset" => list(&[
            "all",
            "none",
            "outside",
            "inside",
            "insideHorizontal",
            "insideVertical",
            "top",
            "bottom",
            "left",
            "right",
            "topBottom",
            "diagonalDown",
            "diagonalUp",
        ]),
        "lineStyle" => list(&[
            "thin",
            "medium",
            "thick",
            "hair",
            "double",
            "dashed",
            "dotted",
            "mediumDashed",
            "dashDot",
            "mediumDashDot",
            "dashDotDot",
            "mediumDashDotDot",
            "slantDashDot",
        ]),
        _ => object(
            json!({
                "font":object(json!({"name":string(128),"size":{"type":"number","minimum":1,"maximum":409},"bold":boolean(),"italic":boolean(),"strike":boolean(),
                    "underline":list(&["None","Single","Double","SingleAccounting","DoubleAccounting"]),
                    "vert":list(&["Baseline","Superscript","Subscript"])}),&[]),
                "align":object(json!({"h":list(&["General","Left","Center","Right","Fill","Justify","CenterAcross","Distributed"]),
                    "v":list(&["Top","Center","Bottom","Justify","Distributed"]),"wrap":boolean(),"shrink":boolean(),
                    "indent":{"type":"integer","minimum":0,"maximum":15},"rotation":{"type":"integer","minimum":-90,"maximum":255,"description":"Degrees -90 to 90, or 255 for stacked text"},
                    "readingOrder":list(&["Context","LeftToRight","RightToLeft"])}),&[]),
                "numFmt":string(255)
            }),
            &[],
        ),
    }
}

fn rectangle(p: &Json) -> Result<RangeRef, String> {
    let range = p["range"].as_str().and_then(RangeRef::parse).ok_or("Supply one unqualified A1 rectangle.")?;
    if range.count() > MAX_FORMAT_CELLS {
        return Err("Style operations are limited to 100,000 cells per rectangle.".into());
    }
    Ok(range)
}

/// Whole lines use native row/column style defaults plus the existing stored cells; don't
/// multiply their advertised width and height as if each empty coordinate would be written.
pub(super) fn style_range(sh: &Sheet, p: &Json) -> Result<RangeRef, String> {
    let r = p["range"].as_str().and_then(RangeRef::parse).ok_or("Supply an unqualified A1 rectangle, row or column range.")?;
    if r.is_full_cols() && r.is_full_rows() {
        return Err("Specify the columns or rows to format, not the entire worksheet.".into());
    }
    if r.is_full_cols() || r.is_full_rows() {
        if (r.is_full_cols() && r.width() > 128) || (r.is_full_rows() && r.height() > 1024) {
            return Err("Specify at most 128 whole columns or 1,024 whole rows per edit.".into());
        }
        if sh.cells.iter_range(r).nth(MAX_FORMAT_CELLS as usize).is_some() {
            return Err("More than 100,000 stored cells in those lines; split the edit into smaller ranges.".into());
        }
        return Ok(r);
    }
    rectangle(p)
}

pub(super) fn prepare(_d: &DocState, sh: &Sheet, name: &str, p: &mut Json) -> Result<(), String> {
    let r = if name == "format_range" { style_range(sh, p)? } else { rectangle(p)? };
    match name {
        "format_range" => {
            let style = p["style"]
                .as_object()
                .filter(|s| s.values().any(|v| !v.is_object() || v.as_object().is_some_and(|m| !m.is_empty())))
                .ok_or("Supply a style property to change.")?;
            if style.get("font").and_then(|v| v["name"].as_str()).is_some_and(|s| s.trim().is_empty()) {
                return Err("Font name must not be blank.".into());
            }
            if style.get("align").and_then(|v| v["rotation"].as_i64()).is_some_and(|n| !(-90..=90).contains(&n) && n != 255) {
                return Err("Rotation is -90 through 90 degrees, or 255 for stacked text.".into());
            }
        }
        "set_borders" => {
            if let Some(s) = p["color"].as_str()
                && s != "auto"
                && !(s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return Err("Supply a #RRGGBB border color or auto.".into());
            }
            if p["preset"] == "none" && (p.get("lineStyle").is_some() || p.get("color").is_some()) {
                return Err("Clearing borders takes neither lineStyle nor color.".into());
            }
            if let Some(style) = p.as_object_mut().and_then(|o| o.remove("lineStyle")) {
                p["style"] = style;
            }
        }
        "merge_cells" => {
            if r.is_single() {
                return Err("Merge needs at least two cells.".into());
            }
            if sh.merges.iter().any(|m| m.intersects(&r) && *m != r) {
                return Err("Unmerge the existing merged area first.".into());
            }
            if sh.spill_ranges.values().any(|s| s.intersects(&r)) {
                return Err("The range intersects a spilled array.".into());
            }
            if sh.pivots.iter().any(|pt| r.contains(pt.anchor) || pt.last_range.is_some_and(|area| area.intersects(&r))) {
                return Err("The range intersects a PivotTable.".into());
            }
            let covered = |c: &CellRef| *c != r.start && r.contains(*c);
            if let Some(at) = sh
                .cells
                .iter_range(r)
                .find_map(|(at, v)| (covered(&at) && v.has_content()).then_some(at))
                .or_else(|| {
                    let bounds = r.start..=r.end;
                    sh.comments
                        .range(bounds.clone())
                        .map(|(a, _)| a)
                        .chain(sh.cell_pictures.range(bounds.clone()).map(|(a, _)| a))
                        .chain(sh.hyperlinks.range(bounds.clone()).map(|(a, _)| a))
                        .chain(sh.spill.range(bounds).map(|(a, _)| a))
                        .copied()
                        .find(covered)
                })
                .or_else(|| sh.sparklines.iter().map(|sp| sp.cell).find(covered))
            {
                return Err(format!("{} has content that would be lost or covered. Move it first, or use CenterAcross alignment.", at.a1()));
            }
        }
        "unmerge_cells" => {
            let mut matches = sh.merges.iter().filter(|m| m.intersects(&r)).peekable();
            if matches.peek().is_none() {
                return Err("No merged cells in this range.".into());
            }
            if !r.is_single() && matches.any(|m| !r.contains_range(m)) {
                return Err("Specify the entire merged range, or one cell inside it.".into());
            }
        }
        _ => return Err("Unknown format operation.".into()),
    }
    Ok(())
}

/// Summarize all distinct stored styles, including inherited row/column styles on empty
/// cells. Counts and first addresses describe the entire rectangle, not a sampled corner.
/// Do not serialize the backing cell values, formulas, or conditional-format expressions.
pub(super) fn inspect(d: &DocState, p: &Json) -> Result<Json, String> {
    let sh = context::sheet(d, p)?;
    let r = rectangle(p)?;
    let mut styles = BTreeMap::<StyleId, (usize, CellRef)>::new();
    for at in r.iter() {
        styles.entry(sh.style_id(at)).or_insert((0, at)).0 += 1;
    }
    let style_count = styles.len();
    let display_color = |col: gridcraft_engine::model::Color| col.hex(&d.wb.theme).unwrap_or_else(|| "auto".to_owned());
    let rows = styles.into_iter().map(|(id, (count, at))| {
        let s = d.wb.styles.get(id);
        let line = |v: gridcraft_engine::model::BorderLine| json!({"style": v.style, "color": display_color(v.color)});
        json!({"kind":"style","count":count,"first":at.a1(),
            "font":{"name":context::clip(&s.font.name,128),"size":s.font.size,"bold":s.font.bold,"italic":s.font.italic,"strike":s.font.strike,
                "underline":s.font.underline,"vert":s.font.vert,"color":display_color(s.font.color)},
            "numFmt":context::clip(s.num_fmt.as_str(),255),
            "fill":{"pattern":s.fill.pattern,"foreground":display_color(s.fill.fg),"background":display_color(s.fill.bg)},
            "align":{"h":s.align.h,"v":s.align.v,"wrap":s.align.wrap,"shrink":s.align.shrink,"indent":s.align.indent,"rotation":s.align.rotation,"readingOrder":s.align.reading_order},
            "borders":{"top":line(s.borders.top),"bottom":line(s.borders.bottom),"left":line(s.borders.left),"right":line(s.borders.right),
                "diagonalUp":line(s.borders.diag_up),"diagonalDown":line(s.borders.diag_down)}})
    });
    let intersects = |m: &&RangeRef| m.intersects(&r);
    let merges = sh.merges.iter().filter(intersects);
    let merge_count = merges.clone().count();
    let offset = p["offset"].as_u64().unwrap_or(0) as usize;
    // Skip before building JSON; comments, pictures and cell text do not enter these receipts.
    let page = context::page(
        rows.skip(offset.min(style_count))
            .chain(merges.skip(offset.saturating_sub(style_count)).map(|m| json!({"kind":"merge","range":m.a1(),"anchor":m.start.a1()}))),
        offset,
        12,
    );
    Ok(json!({"sheet":sh.name,"range":r.a1(),"cellCount":r.count(),"mixedStyles":style_count>1,"distinctStyles":style_count,
        "mergedAreas":merge_count,"layer":"stored cell formatting before conditional formats and table styles","page":page}))
}

#[cfg(test)]
mod tests {
    use super::super::execute;
    use super::*;
    use crate::SheetApp;

    fn book() -> SheetApp {
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        SheetApp::new(session, Default::default())
    }

    #[test]
    fn column_format_is_inherited_without_materializing_blank_rows() {
        let mut app = book();
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        execute(&mut app, "format_range", &json!({"sheet":sheet,"range":"C:C","style":{"numFmt":"0.000","align":{"h":"Center"}}}), true).unwrap();
        assert_eq!(app.session.active().unwrap().wb.active().unwrap().cells.len(), 0);
        execute(&mut app, "set_cell", &json!({"sheet":sheet,"cell":"C1000000","input":"7"}), true).unwrap();
        let report = execute(&mut app, "inspect_format", &json!({"sheet":sheet,"range":"C999999:C1000000"}), false).unwrap();
        assert_eq!(report["mixedStyles"], false);
        assert_eq!(report["page"]["items"][0]["numFmt"], "0.000");
    }

    #[test]
    fn outside_borders_and_centered_paragraphs_are_read_back_from_the_target_sheet() {
        let mut app = book();
        let original = app.session.active().unwrap().wb.active().unwrap().name.clone();
        app.session.execute("home.insertSheet", json!({"name":"Other"})).unwrap();
        let params = json!({"sheet":original,"range":"B5:D7","preset":"outside","lineStyle":"mediumDashDotDot","color":"#329977"});
        execute(&mut app, "set_borders", &params, true).unwrap();
        execute(
            &mut app,
            "format_range",
            &json!({"sheet":original,"range":"B5:D7","style":{"align":{"h":"Center","wrap":true},
            "font":{"italic":true,"underline":"Single"}}}),
            true,
        )
        .unwrap();
        let report = execute(&mut app, "inspect_format", &json!({"sheet":original,"range":"B5:D7"}), false).unwrap();
        let rows = report["page"]["items"].as_array().unwrap();
        assert_eq!(rows.iter().filter_map(|v| v["count"].as_u64()).sum::<u64>(), 9);
        assert!(rows.iter().all(|v| v["align"]["h"] == "Center" && v["align"]["wrap"] == true && v["font"]["italic"] == true));
        let corner = rows.iter().find(|v| v["first"] == "B5").unwrap();
        assert_eq!(corner["borders"]["left"], json!({"style":"MediumDashDotDot","color":"#329977"}));
        assert_eq!(corner["borders"]["right"]["style"], "None");
        assert_eq!(report["mixedStyles"], true);
        let other = execute(&mut app, "inspect_format", &json!({"sheet":"Other","range":"B5:D7"}), false).unwrap();
        assert_eq!(other["mixedStyles"], false);
    }

    #[test]
    fn merge_rejects_nonempty_followers_and_partial_unmerge_keeps_neighboring_areas() {
        let mut app = book();
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let area = json!({"sheet":sheet,"range":"B2:D2"});
        app.session.execute("cell.set", json!({"cell":"B2","input":"heading\nsecond line"})).unwrap();
        app.session.execute("review.newNote", json!({"cell":"D2","text":"do not cover"})).unwrap();
        assert!(execute(&mut app, "merge_cells", &area, true).is_err());
        app.session.execute("review.deleteComment", json!({"cell":"D2"})).unwrap();
        app.session.execute("cell.set", json!({"cell":"D2","input":"=IF(FALSE,7,\"\")"})).unwrap();
        assert!(execute(&mut app, "merge_cells", &area, true).is_err());
        app.session.execute("edit.clearContents", json!({"range":"D2"})).unwrap();
        execute(&mut app, "merge_cells", &area, true).unwrap();
        let neighbor = json!({"sheet":sheet,"range":"E2:F2"});
        execute(&mut app, "merge_cells", &neighbor, true).unwrap();
        assert!(execute(&mut app, "unmerge_cells", &json!({"sheet":sheet,"range":"C2:E2"}), true).is_err());
        execute(&mut app, "unmerge_cells", &json!({"sheet":sheet,"range":"C2"}), true).unwrap();
        let check = |app: &mut SheetApp| execute(app, "inspect_format", &json!({"sheet":sheet,"range":"B2:F2"}), false).unwrap();
        assert_eq!(check(&mut app)["mergedAreas"], 1);
        assert_eq!(execute(&mut app, "inspect_cell", &json!({"sheet":sheet,"cell":"B2"}), false).unwrap()["value"], "heading\nsecond line");
        app.run("edit.undo", json!({})).unwrap();
        assert_eq!(check(&mut app)["mergedAreas"], 2);
    }
}
