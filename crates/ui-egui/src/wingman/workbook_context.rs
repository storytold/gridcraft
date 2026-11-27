//! Read the workbook the user can see. Do not send the document's path, hidden sheets,
//! invisible names, protected formula sources, embedded pictures, or an entire workbook dump.

use crate::SheetApp;
use gridcraft_engine::DocState;
use gridcraft_engine::core::{CellRef, RangeRef, Value};
use gridcraft_engine::model::{Comment, Sheet, Visibility};
use serde_json::{Value as Json, json};

pub(in crate::wingman) const MENTIONS: &[&str] = &["@selection", "@read", "@cell", "@sheet", "@workbook", "@charts", "@comments", "@vba"];

pub(super) fn sheet<'a>(d: &'a DocState, args: &Json) -> Result<&'a Sheet, String> {
    let i = match args["sheet"].as_str() {
        Some(s) => d.wb.sheet_index(s).ok_or("No such worksheet")?,
        None => d.wb.active_sheet(),
    };
    d.wb.sheet(i).filter(|sh| sh.visibility == Visibility::Visible).ok_or_else(|| "Worksheet is not visible".into())
}

pub(super) fn clip(s: &str, chars: usize) -> String {
    let mut it = s.chars();
    let mut out = it.by_ref().take(chars).collect::<String>();
    if it.next().is_some() {
        out.push('…');
    }
    out
}

fn cell_data(d: &DocState, sh: &Sheet, c: CellRef, detail: bool) -> Json {
    let entry = sh.cell(c);
    let style = d.wb.styles.get(sh.style_id(c));
    let hidden = sh.is_protected() && style.protection.hidden;
    let limit = if detail { 4096 } else { 768 };
    let value = match sh.value(c) {
        Value::Empty => Json::Null,
        Value::Text(t) => json!(clip(&t, limit)),
        Value::Number(n) => json!(n),
        Value::Bool(b) => json!(b),
        Value::Error(e) => json!({"error":e.as_str()}),
        Value::Array(a) => json!({"arrayRows":a.rows,"arrayColumns":a.cols}),
    };
    let mut out = json!({"cell":c.a1(),"value":value});
    if let Some(f) = entry.and_then(|x| x.formula.as_ref()) {
        if hidden {
            out["formulaHidden"] = json!(true);
        } else {
            out["formula"] = json!(format!("={}", clip(&f.text, limit)));
        }
    }
    if detail {
        out["style"] = json!({"font":clip(&style.font.name,128),"size":style.font.size,"bold":style.font.bold,"italic":style.font.italic,
            "strike":style.font.strike,"color":style.font.color.hex(&d.wb.theme),"fill":style.fill,"align":style.align,
            "numFmt":clip(style.num_fmt.as_str(),255)});
        out["comment"] = sh.comments.get(&c).map_or(Json::Null, |v| comment(v, true));
        if let Some(py) = entry.and_then(|x| x.py.as_deref()) {
            out["python"] = if hidden { json!({"hidden":true}) } else { json!({"code":clip(&py.code,limit),"stale":py.stale}) };
        }
        if let Some(p) = sh.cell_pictures.get(&c) {
            out["picture"] = json!({"alt":clip(&p.alt,256),"mime":clip(&p.mime,80)});
        }
    }
    out
}

fn comment(c: &Comment, detail: bool) -> Json {
    let mut out = json!({"author":clip(&c.author,128),"text":clip(&c.text,if detail {2048} else {256}),
        "threaded":c.threaded,"resolved":c.resolved,"replyCount":c.replies.len()});
    if detail {
        out["replies"] = json!(c.replies.iter().take(8).map(|(by, msg)| json!({"author":clip(by,128),"text":clip(msg,256)})).collect::<Vec<_>>());
    }
    out
}

fn sheet_info(sh: &Sheet) -> Json {
    json!({"sheet":clip(&sh.name,128),"cells":sh.cells.len(),"charts":sh.charts.len(),
        "comments":sh.comments.len(),"tables":sh.tables.len(),"protected":sh.is_protected(),
        "autoFilter":sh.autofilter.as_ref().map(|af|af.range.a1()),"conditionalFormats":sh.cond_formats.len()})
}

/// Entries are bounded before they enter JSON. Pagination also enforces a byte budget for a
/// workbook full of long non-ASCII strings; the next offset counts only entries actually sent.
pub(super) fn page(items: impl Iterator<Item = Json>, offset: usize, limit: usize) -> Json {
    let mut collected = Vec::new();
    let mut bytes = 0;
    let mut more = false;
    for mut item in items.take(limit + 1) {
        let mut size = item.to_string().len();
        if size > 26 * 1024 {
            item = json!({"itemOmitted":"Entry exceeds the page size","bytes":size});
            size = item.to_string().len();
        }
        if collected.len() == limit || size + bytes > 26 * 1024 {
            more = true;
            break;
        }
        bytes += size;
        collected.push(item);
    }
    let next = more.then_some(offset.saturating_add(collected.len()));
    json!({"items":collected, "nextOffset":next})
}

fn info(d: &DocState, offset: usize) -> Json {
    let sheets = d.wb.sheets.iter().filter(|s| s.visibility == Visibility::Visible).skip(offset).map(|s| sheet_info(s));
    json!({"workbook":clip(&d.display_title(),128),"sheets":page(sheets,offset,32),"activeSheet":d.wb.active().map(|s|clip(&s.name,128)),
        "activeCell":d.selection.active.a1(),"readOnly":d.read_only_source.is_some(),"protectedStructure":d.wb.protected_structure})
}

fn objects(d: &DocState, sh: &Sheet, kind: &str, offset: usize) -> Result<Json, String> {
    let data = match kind {
        "charts" => page(sh.charts.iter().skip(offset).map(|c|json!({"id":c.id,"type":format!("{:?}",c.kind),
            "title":c.title.as_deref().map(|s|clip(s,128)),"at":c.anchor.cell.a1(),"source":c.source.as_deref().map(|s|clip(s,240)),
            "series":c.series.len(),"xTitle":c.x_title.as_deref().map(|s|clip(s,128)),"yTitle":c.y_title.as_deref().map(|s|clip(s,128)),
            "legend":format!("{:?}",c.legend),"dataLabels":c.data_labels,"gridlines":c.gridlines})),offset,32),
        "comments" => page(sh.comments.iter().skip(offset).map(|(cell,c)| json!({"cell":cell.a1(),"comment":comment(c,false)})),offset,32),
        "tables" => page(sh.tables.iter().skip(offset).map(|t|json!({"name":clip(&t.name,128),"range":t.range.a1(),"columns":t.columns.len()})),offset,32),
        "names" => page(d.wb.names.iter().filter(|n| !n.hidden && n.scope.is_none_or(|i|d.wb.sheet(i).is_some_and(|s|s.visibility==Visibility::Visible)))
            .skip(offset).map(|n|json!({"name":clip(&n.name,128),"refersTo":clip(&n.formula,256),"scope":n.scope.and_then(|i|d.wb.sheet(i)).map(|s|clip(&s.name,128))})),offset,32),
        _ => return Err("Unknown object category".into()),
    };
    Ok(json!({"sheet":clip(&sh.name,128),"kind":kind,"page":data}))
}

fn cells(d: &DocState, sh: &Sheet, range: RangeRef, offset: usize) -> Json {
    json!({"sheet":clip(&sh.name,128),"range":range.a1(),"page":page(range.iter().skip(offset).map(|c|cell_data(d,sh,c,false)),offset,256)})
}

pub(super) fn query(app: &SheetApp, name: &str, args: &Json) -> Result<Json, String> {
    let d = app.session.active().ok_or("No open workbook")?;
    let sh = sheet(d, args)?;
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    match name {
        "workbook_info" => Ok(info(d, offset)),
        "inspect_cell" => Ok(cell_data(d, sh, args["cell"].as_str().and_then(CellRef::parse).ok_or("Invalid cell address")?, true)),
        "read_range" => Ok(cells(d, sh, super::limited_range(args["range"].as_str().ok_or("Missing range")?)?, offset)),
        "list_objects" => objects(d, sh, args["kind"].as_str().ok_or("Missing object category")?, offset),
        _ => Err("Unknown inspection tool".into()),
    }
}

pub(in crate::wingman) fn mention(app: &SheetApp, token: &str) -> Result<Json, String> {
    let d = app.session.active().ok_or("No open workbook")?;
    let sh = sheet(d, &Json::Null)?;
    match token.trim_start_matches('@') {
        "selection" | "read" => {
            // Capture only the explicitly selected cells, never the bounding rectangle between
            // disjoint areas. A whole-column selection still costs at most one page.
            let areas = d.selection.ranges.iter().take(32).map(RangeRef::a1).collect::<Vec<_>>();
            let items = d.selection.ranges.iter().flat_map(RangeRef::iter).map(|c| cell_data(d, sh, c, false));
            let preview = page(items, 0, 128);
            let truncated = preview["nextOffset"].is_number();
            Ok(
                json!({"sheet":clip(&sh.name,128),"activeCell":d.selection.active.a1(),"areas":areas,"areaCount":d.selection.ranges.len(),"preview":preview["items"],"truncated":truncated}),
            )
        }
        "cell" => Ok(cell_data(d, sh, d.selection.active, true)),
        "sheet" => {
            // Computing the used rectangle scans occupied rows. Limit this to one explicitly
            // requested worksheet, rather than scanning every tab for a workbook summary.
            let mut info = sheet_info(sh);
            info["usedRange"] = json!(sh.used_range().map(|r| r.a1()));
            Ok(info)
        }
        "workbook" => Ok(info(d, 0)),
        "charts" => objects(d, sh, "charts", 0),
        "comments" => objects(d, sh, "comments", 0),
        "vba" => Ok(json!({"embeddedVbaBytes":d.wb.vba_project.as_ref().map(|b|b.len()),
            "capability":"GridCraft preserves an embedded VBA project as opaque bytes. Paste module text for review or formatting; binary modules cannot be read, rewritten or run by Wingman."})),
        _ => {
            let (on_sheet, address) = crate::wingman::composer::read_target(token)?;
            let target = match on_sheet {
                Some(name) => sheet(d, &json!({"sheet":name}))?,
                None => sh,
            };
            Ok(cells(d, target, super::limited_range(&address)?, 0))
        }
    }
}
