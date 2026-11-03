use super::*;

pub(super) fn read_hyperlinks(cx: &mut Ctx<'_>, name: &str, e: &El, rels: &[Rel], sheet: &mut Sheet) {
    let mut cells = 0u64;
    let mut bytes = 0u64;
    for h in e.kids("hyperlink") {
        let Some(range) = h.attr("ref").and_then(RangeRef::parse) else { continue };
        let mut target = h.attr("id").and_then(|id| rels.iter().find(|r| r.id == id)).map(|r| r.target.clone()).unwrap_or_default();
        // A relationship is a file or web page; a bare `location` is a place in this workbook.
        let external = !target.is_empty();
        if let Some(loc) = h.attr("location").filter(|l| !l.is_empty()) {
            if target.is_empty() {
                target = loc.trim_start_matches('#').to_string();
            } else {
                target = format!("{target}#{loc}");
            }
        }
        if target.is_empty() {
            continue;
        }
        let link = Hyperlink { target: target.clone(), tooltip: h.attr("tooltip").map(str::to_string), external };
        let link_bytes = (target.len() + link.tooltip.as_ref().map_or(0, |t| t.len())) as u64;
        for c in range.iter() {
            if cells >= MAX_HYPERLINK_CELLS {
                cx.warn(format!("sheet \"{name}\": hyperlinks past the first {MAX_HYPERLINK_CELLS} cells were dropped"));
                return;
            }
            if bytes.saturating_add(link_bytes) > MAX_HYPERLINK_BYTES {
                cx.warn(format!("sheet \"{name}\": hyperlink targets past {MAX_HYPERLINK_BYTES} bytes were dropped"));
                return;
            }
            cells += 1;
            bytes += link_bytes;
            sheet.hyperlinks.insert(c, link.clone());
        }
    }
}

fn filter_op(s: Option<&str>) -> &'static str {
    match s.unwrap_or("equal") {
        "lessThan" => "<",
        "lessThanOrEqual" => "<=",
        "notEqual" => "<>",
        "greaterThanOrEqual" => ">=",
        "greaterThan" => ">",
        _ => "=",
    }
}

pub fn read_autofilter(cx: &mut Ctx<'_>, e: &El) -> Option<AutoFilter> {
    let range = e.attr("ref").and_then(RangeRef::parse)?;
    let mut criteria = vec![];
    for fc in e.kids("filterColumn") {
        let Some(col) = fc.attr_u32("colId") else { continue };
        let crit = if let Some(f) = fc.child("filters") {
            FilterCriterion::Values {
                values: f.kids("filter").filter_map(|x| x.attr("val")).map(str::to_string).collect(),
                blanks: f.flag("blank", false),
            }
        } else if let Some(cf) = fc.child("customFilters") {
            let mut it = cf.kids("customFilter").map(|c| (filter_op(c.attr("operator")).to_string(), c.attr("val").unwrap_or("").to_string()));
            let Some(a) = it.next() else { continue };
            FilterCriterion::Custom { a, b: it.next(), and: cf.flag("and", false) }
        } else if let Some(t) = fc.child("top10") {
            FilterCriterion::Top10 {
                bottom: !t.flag("top", true),
                percent: t.flag("percent", false),
                count: t.attr_f64("val").unwrap_or(10.0).clamp(0.0, 1e9) as u32,
            }
        } else if let Some(d) = fc.child("dynamicFilter") {
            match d.attr("type") {
                Some("aboveAverage") => FilterCriterion::AboveAverage(true),
                Some("belowAverage") => FilterCriterion::AboveAverage(false),
                _ => {
                    cx.warn("date filters in AutoFilters are not supported");
                    continue;
                }
            }
        } else if let Some(c) = fc.child("colorFilter") {
            let Some(dxf) = c.attr_u32("dxfId").and_then(|i| cx.dxfs.get(i as usize)) else { continue };
            if c.flag("cellColor", true) { FilterCriterion::FillColor(dxf.fill.fg) } else { FilterCriterion::FontColor(dxf.font.color) }
        } else if let Some(i) = fc.child("iconFilter") {
            let Some(set) = i.attr("iconSet") else { continue };
            FilterCriterion::Icon { set: set.to_string(), id: i.attr_u32("iconId") }
        } else {
            continue;
        };
        criteria.push((col, crit));
    }
    Some(AutoFilter { range, criteria })
}

pub fn read_table(cx: &mut Ctx<'_>, x: &El) -> Option<Table> {
    let range = x.attr("ref").and_then(RangeRef::parse)?;
    let name = x.attr("displayName").or_else(|| x.attr("name")).unwrap_or("Table").to_string();
    let header_row = x.attr_u32("headerRowCount").unwrap_or(1) > 0;
    let totals_row = x.attr_u32("totalsRowCount").unwrap_or(0) > 0;
    let columns = x
        .child("tableColumns")
        .map(|tc| {
            tc.kids("tableColumn")
                .map(|c| {
                    let totals = match c.attr("totalsRowFunction").unwrap_or("none") {
                        "average" => TotalsFn::Average,
                        "count" => TotalsFn::Count,
                        "countNums" => TotalsFn::CountNums,
                        "max" => TotalsFn::Max,
                        "min" => TotalsFn::Min,
                        "sum" => TotalsFn::Sum,
                        "stdDev" => TotalsFn::StdDev,
                        "var" => TotalsFn::Var,
                        "custom" => TotalsFn::Custom,
                        _ => TotalsFn::None,
                    };
                    let totals_label = if totals == TotalsFn::Custom {
                        c.child("totalsRowFormula").map(|f| from_file(f.text.trim()))
                    } else {
                        c.attr("totalsRowLabel").map(str::to_string)
                    };
                    TableColumn {
                        name: xml::decode_xstring(c.attr("name").unwrap_or("")),
                        totals,
                        totals_label,
                        formula: c.child("calculatedColumnFormula").map(|f| from_file(f.text.trim())),
                        unique_name: c.attr("uniqueName").map(str::to_string),
                        xml: c.child("xmlColumnPr").and_then(|x| {
                            Some(XmlColumnPr {
                                map_id: x.attr_u32("mapId")?,
                                xpath: x.attr("xpath")?.to_string(),
                                data_type: x.attr("xmlDataType").unwrap_or("string").to_string(),
                                denormalized: x.flag("denormalized", false),
                            })
                        }),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let si = x.child("tableStyleInfo");
    // Only XML tables are kept as such: a query table's definition hangs off the table part, which
    // is rewritten, so it would lose it.
    let xml_table = x.attr("tableType") == Some("xml");
    Some(Table {
        id: x.attr_u32("id").unwrap_or(0),
        name,
        range,
        header_row,
        totals_row,
        columns,
        style: si.and_then(|s| s.attr("name")).unwrap_or("").to_string(),
        banded_rows: si.is_some_and(|s| s.flag("showRowStripes", false)),
        banded_cols: si.is_some_and(|s| s.flag("showColumnStripes", false)),
        first_col: si.is_some_and(|s| s.flag("showFirstColumn", false)),
        last_col: si.is_some_and(|s| s.flag("showLastColumn", false)),
        filter_button: x.child("autoFilter").is_some(),
        filter: x.child("autoFilter").and_then(|e| read_autofilter(cx, e)).map(|af| af.criteria).unwrap_or_default(),
        table_type: xml_table.then(|| "xml".to_string()),
        connection_id: x.attr_u32("connectionId").filter(|_| xml_table),
    })
}

pub(super) fn read_comments(x: &El, sheet: &mut Sheet) {
    let authors: Vec<String> = x.child("authors").map(|a| a.kids("author").map(|a| a.text.clone()).collect()).unwrap_or_default();
    let Some(list) = x.child("commentList") else { return };
    for c in list.kids("comment") {
        let Some(cell) = c.attr("ref").and_then(CellRef::parse) else { continue };
        let author = c.attr_u32("authorId").and_then(|i| authors.get(i as usize)).cloned().unwrap_or_default();
        let mut text = c.child("text").map(xml::rich_text).unwrap_or_default();
        // Excel starts a note with a bold "Author:" line.
        if !author.is_empty()
            && let Some(rest) = text.strip_prefix(&format!("{author}:"))
        {
            text = rest.strip_prefix('\n').unwrap_or(rest).to_string();
        }
        sheet
            .comments
            .insert(cell, Comment { author, text, replies: vec![], threaded: false, resolved: false, visible: false, dates: vec![], note_box: None });
    }
}

/// Reads the notes' VML shapes: a shape carrying `<x:Visible/>` marks its note as shown, and its
/// `<x:Anchor>` and style size become the note's box. Shapes may sit in groups.
pub(super) fn read_note_shapes(x: &El, sheet: &mut Sheet, depth: usize) {
    if depth > xml::MAX_DEPTH {
        return;
    }
    for e in &x.children {
        if e.name != "shape" {
            read_note_shapes(e, sheet, depth + 1);
            continue;
        }
        let Some(cd) = e.child("ClientData").filter(|cd| cd.attr("ObjectType") == Some("Note")) else { continue };
        let num = |k: &str| cd.child(k).and_then(|v| v.text.trim().parse::<u32>().ok());
        let (Some(row), Some(col)) = (num("Row"), num("Column")) else { continue };
        let cell = CellRef::new(row, col);
        let Some(c) = sheet.comments.get_mut(&cell) else { continue };
        if cd.child("Visible").is_some() && !c.threaded {
            c.visible = true;
        }
        if let Some(anchor) = cd.child("Anchor") {
            c.note_box = note_box(&anchor.text, e.attr("style").unwrap_or(""), cell).filter(|b| *b != crate::sheet_write::DEFAULT_NOTE_BOX);
        }
    }
}

/// A note's box from its VML `<x:Anchor>` (`LeftColumn, LeftOffset, TopRow, TopOffset,
/// RightColumn, RightOffset, BottomRow, BottomOffset`; offsets in pixels) and its shape's style,
/// the corners made relative to the note's `cell`. `None` when the anchor isn't eight in-sheet
/// numbers.
fn note_box(anchor: &str, style: &str, cell: CellRef) -> Option<NoteBox> {
    let v: Vec<u32> = anchor.split(',').map(|p| p.trim().parse::<u32>().ok()).collect::<Option<_>>()?;
    let [c1, dx1, r1, dy1, c2, dx2, r2, dy2] = <[u32; 8]>::try_from(v).ok()?;
    if c1.max(c2) >= MAX_COLS || r1.max(r2) >= MAX_ROWS {
        return None;
    }
    let corner = |c: u32, dx: u32, r: u32, dy: u32| NoteCorner {
        col: i64::from(c) - i64::from(cell.col),
        col_px: dx,
        row: i64::from(r) - i64::from(cell.row),
        row_px: dy,
    };
    let pt = |key: &str| {
        style
            .split(';')
            .filter_map(|d| d.split_once(':'))
            .find(|(k, _)| k.trim().eq_ignore_ascii_case(key))
            .and_then(|(_, v)| v.trim().strip_suffix("pt")?.trim().parse::<f64>().ok().filter(|n| n.is_finite() && *n > 0.0 && *n < 1e6))
    };
    Some(NoteBox { from: corner(c1, dx1, r1, dy1), to: corner(c2, dx2, r2, dy2), size_pt: pt("width").zip(pt("height")) })
}

pub(super) fn read_threaded(cx: &Ctx<'_>, x: &El, sheet: &mut Sheet) {
    let mut by_id: HashMap<String, CellRef> = HashMap::new();
    let mut replies: Vec<(String, String, String, String)> = vec![];
    for t in x.kids("threadedComment") {
        let Some(cell) = t.attr("ref").and_then(CellRef::parse) else { continue };
        let author = t.attr("personId").and_then(|p| cx.persons.get(p)).cloned().unwrap_or_default();
        let text = t.child("text").map(|e| e.text.clone()).unwrap_or_default();
        let date = t.attr("dT").unwrap_or("").to_string();
        match t.attr("parentId") {
            Some(p) => replies.push((p.to_string(), author, text, date)),
            None => {
                if let Some(id) = t.attr("id") {
                    by_id.insert(id.to_string(), cell);
                }
                let dates = if date.is_empty() { vec![] } else { vec![date] };
                sheet.comments.insert(
                    cell,
                    Comment { author, text, replies: vec![], threaded: true, resolved: t.flag("done", false), visible: false, dates, note_box: None },
                );
            }
        }
    }
    // Replies in file order; ones whose thread is missing are dropped.
    for (parent, author, text, date) in replies {
        if let Some(c) = by_id.get(&parent)
            && let Some(cm) = sheet.comments.get_mut(c)
        {
            if !date.is_empty() {
                // `dates[0]` is the thread, `dates[i + 1]` reply i; unknown dates stay empty.
                let at = cm.replies.len() + 1;
                if cm.dates.len() < at {
                    cm.dates.resize(at, String::new());
                }
                cm.dates.push(date);
            }
            cm.replies.push((author, text));
        }
    }
}
