//! Writing worksheet parts and the parts they own (tables, comments, drawings, charts).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_model::{
    AutoFilter, Cell, CfOperator, CfRule, CfValueKind, Color, CondFormat, ErrorStyle, FilterCriterion, LineInfo, Orientation, Sheet, SparklineKind,
    Style, Table, TotalsFn, Validation, ValidationKind, Workbook,
};

use crate::drawing::{Obj, image_content_type, image_ext};
use crate::fmla::text_to_file;
use crate::styles::{color_attrs, dxf_id};
use crate::tables::{paper_id, px_to_col_width, px_to_pt};
use crate::write::{CT_CHART, CT_COMMENTS, CT_DRAWING, CT_TABLE, NS_MAIN, NS_REL, Out, Rels, XML_DECL};
use crate::xml::{encode_xstring, esc, esc_attr, num};

fn sqref(ranges: &[RangeRef]) -> String {
    ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(" ")
}

fn cf_op_name(op: CfOperator) -> &'static str {
    match op {
        CfOperator::Between => "between",
        CfOperator::NotBetween => "notBetween",
        CfOperator::Equal => "equal",
        CfOperator::NotEqual => "notEqual",
        CfOperator::Greater => "greaterThan",
        CfOperator::Less => "lessThan",
        CfOperator::GreaterOrEqual => "greaterThanOrEqual",
        CfOperator::LessOrEqual => "lessThanOrEqual",
    }
}

/// Valid table name: letters, digits, `_` and `.`, starting with a letter or `_`.
fn table_name(name: &str, used: &mut HashSet<String>, n: u32) -> String {
    let mut s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '.' { c } else { '_' }).collect();
    if s.is_empty() || !s.starts_with(|c: char| c.is_alphabetic() || c == '_') || gridcraft_core::CellRef::parse(&s).is_some() {
        s = format!("Table{n}");
    }
    let mut out = s.clone();
    let mut k = 2;
    while !used.insert(out.to_ascii_lowercase()) {
        out = format!("{s}{k}");
        k += 1;
    }
    out
}

/// Column names for a table: one per column of its range, unique and non-empty.
fn table_columns(t: &Table) -> Vec<String> {
    let w = t.range.width() as usize;
    let mut used = HashSet::new();
    (0..w)
        .map(|i| {
            let base = t
                .columns
                .get(i)
                .map(|c| c.name.replace(['\r', '\n'], " "))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| format!("Column{}", i + 1));
            let mut name = base.clone();
            let mut k = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base}{k}");
                k += 1;
            }
            name
        })
        .collect()
}

pub fn write_sheet(wb: &Workbook, si: usize, selected: bool, out: &mut Out) -> (String, Rels) {
    let mut rels = Rels::default();
    let Some(sheet) = wb.sheet(si) else { return (String::new(), rels) };
    let nstyles = wb.styles.len() as u32;
    let sid = |s: gridcraft_model::StyleId| if s.0 < nstyles { s.0 } else { 0 };

    // Table header cells must hold the column names.
    let mut overrides: BTreeMap<CellRef, String> = BTreeMap::new();
    let tables: Vec<(&Table, Vec<String>)> = sheet
        .tables
        .iter()
        .filter(|t| t.range.height() > t.header_row as u32 + t.totals_row as u32 || (t.header_row && !t.totals_row && t.range.height() >= 1))
        .map(|t| (t, table_columns(t)))
        .collect();
    for (t, cols) in &tables {
        if t.header_row {
            for (i, name) in cols.iter().enumerate() {
                overrides.insert(CellRef::new(t.range.start.row, t.range.start.col + i as u32), name.clone());
            }
        }
    }
    // Dynamic-array spills: anchor → range, and the spilled values to cache.
    let dynamic: BTreeMap<CellRef, RangeRef> = sheet
        .spill_ranges
        .iter()
        .filter(|(a, r)| !r.is_single() && sheet.cells.get(**a).is_some_and(|c| c.formula.as_ref().is_some_and(|f| f.array.is_none())))
        .map(|(a, r)| (*a, *r))
        .collect();
    let mut extra: BTreeMap<CellRef, Value> = BTreeMap::new();
    for (a, r) in &dynamic {
        for c in r.iter().take(1_000_000) {
            if c != *a
                && let Some(v) = sheet.spill.get(&c)
                && sheet.cells.get(c).is_none_or(|x| x.formula.is_none() && x.value.is_empty())
            {
                extra.insert(c, v.clone());
            }
        }
    }

    let mut s = format!("{XML_DECL}<worksheet xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\">");
    // sheetPr
    let fit = sheet.print.fit_to.is_some();
    if sheet.tab_color.is_some() || fit {
        s.push_str("<sheetPr>");
        if let Some(c) = &sheet.tab_color {
            let _ = write!(s, "<tabColor {}/>", color_attrs(c));
        }
        if fit {
            s.push_str("<pageSetUpPr fitToPage=\"1\"/>");
        }
        s.push_str("</sheetPr>");
    }
    let mut used = sheet.cells.used_range();
    for c in extra.keys().chain(overrides.keys()) {
        let r = RangeRef::cell(*c);
        used = Some(used.map_or(r, |u| u.union(&r)));
    }
    let _ = write!(s, "<dimension ref=\"{}\"/>", used.map(|u| u.a1()).unwrap_or_else(|| "A1".into()));

    // sheetViews
    s.push_str("<sheetViews><sheetView");
    if selected {
        s.push_str(" tabSelected=\"1\"");
    }
    if !sheet.show_gridlines {
        s.push_str(" showGridLines=\"0\"");
    }
    if !sheet.show_headings {
        s.push_str(" showRowColHeaders=\"0\"");
    }
    if sheet.show_formulas {
        s.push_str(" showFormulas=\"1\"");
    }
    if !sheet.show_zeros {
        s.push_str(" showZeros=\"0\"");
    }
    if sheet.right_to_left {
        s.push_str(" rightToLeft=\"1\"");
    }
    if sheet.zoom != 100 && (10..=400).contains(&sheet.zoom) {
        let _ = write!(s, " zoomScale=\"{0}\" zoomScaleNormal=\"{0}\"", sheet.zoom);
    }
    if sheet.view_top_left != CellRef::default() && sheet.freeze.is_none() {
        let _ = write!(s, " topLeftCell=\"{}\"", sheet.view_top_left.a1());
    }
    s.push_str(" workbookViewId=\"0\">");
    let active = sheet.view_active.a1();
    match sheet.freeze {
        Some((rows, cols)) if rows > 0 || cols > 0 => {
            let pane = match (rows > 0, cols > 0) {
                (true, true) => "bottomRight",
                (true, false) => "bottomLeft",
                _ => "topRight",
            };
            s.push_str("<pane");
            if cols > 0 {
                let _ = write!(s, " xSplit=\"{cols}\"");
            }
            if rows > 0 {
                let _ = write!(s, " ySplit=\"{rows}\"");
            }
            let _ = write!(
                s,
                " topLeftCell=\"{}\" activePane=\"{pane}\" state=\"frozen\"/>",
                CellRef::new(rows.min(gridcraft_core::MAX_ROWS - 1), cols.min(gridcraft_core::MAX_COLS - 1)).a1()
            );
            let _ = write!(s, "<selection pane=\"{pane}\" activeCell=\"{active}\" sqref=\"{active}\"/>");
        }
        _ => {
            let _ = write!(s, "<selection activeCell=\"{active}\" sqref=\"{active}\"/>");
        }
    }
    s.push_str("</sheetView></sheetViews>");

    // sheetFormatPr
    let max_row_outline = sheet.rows.values().map(|r| r.outline).max().unwrap_or(0);
    let max_col_outline = sheet.cols.values().map(|r| r.outline).max().unwrap_or(0);
    s.push_str("<sheetFormatPr");
    if (sheet.default_col_width - gridcraft_model::DEFAULT_COL_WIDTH).abs() > 0.01 {
        let _ = write!(s, " defaultColWidth=\"{}\"", num(px_to_col_width(sheet.default_col_width)));
    }
    let _ = write!(s, " defaultRowHeight=\"{}\"", num(px_to_pt(sheet.default_row_height)));
    if (sheet.default_row_height - gridcraft_model::DEFAULT_ROW_HEIGHT).abs() > 0.01 {
        s.push_str(" customHeight=\"1\"");
    }
    if max_row_outline > 0 {
        let _ = write!(s, " outlineLevelRow=\"{max_row_outline}\"");
    }
    if max_col_outline > 0 {
        let _ = write!(s, " outlineLevelCol=\"{max_col_outline}\"");
    }
    s.push_str("/>");

    // cols: runs of identical settings.
    if !sheet.cols.is_empty() {
        s.push_str("<cols>");
        let mut run: Option<(u32, u32, LineInfo)> = None;
        let flush = |s: &mut String, r: (u32, u32, LineInfo)| {
            let (a, b, i) = r;
            let w = i.size.unwrap_or(sheet.default_col_width);
            let _ = write!(s, "<col min=\"{}\" max=\"{}\" width=\"{}\"", a + 1, b + 1, num(px_to_col_width(w)));
            if let Some(st) = i.style {
                let _ = write!(s, " style=\"{}\"", sid(st));
            }
            if i.hidden {
                s.push_str(" hidden=\"1\"");
            }
            if i.size.is_some() {
                s.push_str(" customWidth=\"1\"");
            }
            if i.outline > 0 {
                let _ = write!(s, " outlineLevel=\"{}\"", i.outline.min(7));
            }
            if i.collapsed {
                s.push_str(" collapsed=\"1\"");
            }
            s.push_str("/>");
        };
        for (&c, info) in &sheet.cols {
            if c >= gridcraft_core::MAX_COLS {
                continue;
            }
            run = match run {
                Some((a, b, i)) if b + 1 == c && i == *info => Some((a, c, i)),
                Some(r) => {
                    flush(&mut s, r);
                    Some((c, c, *info))
                }
                None => Some((c, c, *info)),
            };
        }
        if let Some(r) = run {
            flush(&mut s, r);
        }
        s.push_str("</cols>");
    }

    // sheetData
    let mut keys: BTreeSet<CellRef> = sheet.cells.iter().map(|(c, _)| c).collect();
    keys.extend(sheet.cell_pictures.keys().copied());
    keys.extend(extra.keys().copied());
    keys.extend(overrides.keys().copied());
    let mut rows: BTreeMap<u32, Vec<CellRef>> = BTreeMap::new();
    for k in keys {
        rows.entry(k.row).or_default().push(k);
    }
    for r in sheet.rows.keys() {
        rows.entry(*r).or_default();
    }
    s.push_str("<sheetData>");
    for (r, cells) in &rows {
        let info = sheet.rows.get(r).copied().unwrap_or_default();
        if cells.is_empty() && info == LineInfo::default() {
            continue;
        }
        let _ = write!(s, "<row r=\"{}\"", r + 1);
        if let Some(st) = info.style {
            let _ = write!(s, " s=\"{}\" customFormat=\"1\"", sid(st));
        }
        if let Some(h) = info.size {
            let _ = write!(s, " ht=\"{}\"{}", num(px_to_pt(h)), if info.custom { " customHeight=\"1\"" } else { "" });
        }
        if info.hidden {
            s.push_str(" hidden=\"1\"");
        }
        if info.outline > 0 {
            let _ = write!(s, " outlineLevel=\"{}\"", info.outline.min(7));
        }
        if info.collapsed {
            s.push_str(" collapsed=\"1\"");
        }
        if cells.is_empty() {
            s.push_str("/>");
            continue;
        }
        s.push('>');
        for c in cells {
            let cell = sheet.cells.get(*c);
            let ov = overrides.get(c);
            let ex = extra.get(c);
            write_cell(&mut s, *c, cell, sheet.cell_pictures.get(c), ov, ex, dynamic.get(c), &sid, out);
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");

    // sheetProtection
    if let Some(p) = &sheet.protection {
        s.push_str("<sheetProtection sheet=\"1\" objects=\"1\" scenarios=\"1\"");
        for (attr, allowed) in [
            ("formatCells", p.format_cells),
            ("formatColumns", p.format_columns),
            ("formatRows", p.format_rows),
            ("insertColumns", p.insert_columns),
            ("insertRows", p.insert_rows),
            ("deleteColumns", p.delete_columns),
            ("deleteRows", p.delete_rows),
            ("sort", p.sort),
            ("autoFilter", p.autofilter),
        ] {
            if allowed {
                let _ = write!(s, " {attr}=\"0\"");
            }
        }
        if !p.select_locked {
            s.push_str(" selectLockedCells=\"1\"");
        }
        if !p.select_unlocked {
            s.push_str(" selectUnlockedCells=\"1\"");
        }
        s.push_str("/>");
    }
    // autoFilter (a sheet filter overlapping a table would be invalid).
    if let Some(af) = &sheet.autofilter
        && !sheet.tables.iter().any(|t| t.range.intersects(&af.range))
    {
        s.push_str(&autofilter_xml(af, out));
    }
    if !sheet.merges.is_empty() {
        let merges: Vec<&RangeRef> = sheet.merges.iter().filter(|m| !m.is_single()).collect();
        let _ = write!(s, "<mergeCells count=\"{}\">", merges.len());
        for m in merges {
            let _ = write!(s, "<mergeCell ref=\"{}\"/>", m.a1());
        }
        s.push_str("</mergeCells>");
    }
    // Conditional formats, priorities renumbered 1..n in priority order.
    let mut order: Vec<usize> = (0..sheet.cond_formats.len()).collect();
    order.sort_by_key(|&i| sheet.cond_formats.get(i).map(|c| c.priority).unwrap_or(0));
    let mut prio = vec![0u32; sheet.cond_formats.len()];
    for (rank, &i) in order.iter().enumerate() {
        if let Some(p) = prio.get_mut(i) {
            *p = rank as u32 + 1;
        }
    }
    for (i, cf) in sheet.cond_formats.iter().enumerate() {
        if cf.ranges.is_empty() {
            continue;
        }
        let _ = write!(
            s,
            "<conditionalFormatting sqref=\"{}\">{}</conditionalFormatting>",
            sqref(&cf.ranges),
            cf_rule_xml(cf, prio.get(i).copied().unwrap_or(1), out)
        );
    }
    let vals: Vec<&Validation> = sheet.validations.iter().filter(|v| !v.ranges.is_empty()).collect();
    if !vals.is_empty() {
        let _ = write!(s, "<dataValidations count=\"{}\">", vals.len());
        for v in vals {
            s.push_str(&validation_xml(v));
        }
        s.push_str("</dataValidations>");
    }
    if !sheet.hyperlinks.is_empty() {
        s.push_str("<hyperlinks>");
        for (c, h) in &sheet.hyperlinks {
            let t = h.target.trim();
            if t.is_empty() {
                continue;
            }
            let external = t.contains("://")
                || t.to_ascii_lowercase().starts_with("mailto:")
                || t.starts_with("\\\\")
                || t.to_ascii_lowercase().starts_with("file:");
            let _ = write!(s, "<hyperlink ref=\"{}\"", c.a1());
            if external {
                let (url, loc) = match t.split_once('#') {
                    Some((u, l)) if !u.is_empty() => (u, Some(l)),
                    _ => (t, None),
                };
                let id = rels.add_external("hyperlink", url);
                let _ = write!(s, " r:id=\"{id}\"");
                if let Some(l) = loc {
                    let _ = write!(s, " location=\"{}\"", esc_attr(l));
                }
            } else {
                let _ = write!(s, " location=\"{}\" display=\"{}\"", esc_attr(t.trim_start_matches('#')), esc_attr(t.trim_start_matches('#')));
            }
            if let Some(tt) = &h.tooltip {
                let _ = write!(s, " tooltip=\"{}\"", esc_attr(tt));
            }
            s.push_str("/>");
        }
        s.push_str("</hyperlinks>");
    }
    // Printing.
    let p = &sheet.print;
    if p.gridlines || p.headings || p.center_h || p.center_v {
        s.push_str("<printOptions");
        for (a, v) in [("horizontalCentered", p.center_h), ("verticalCentered", p.center_v), ("headings", p.headings), ("gridLines", p.gridlines)] {
            if v {
                let _ = write!(s, " {a}=\"1\"");
            }
        }
        s.push_str("/>");
    }
    let m = |i: usize| num(p.margins.get(i).copied().unwrap_or(0.75) as f64);
    let _ =
        write!(s, "<pageMargins left=\"{}\" right=\"{}\" top=\"{}\" bottom=\"{}\" header=\"{}\" footer=\"{}\"/>", m(0), m(1), m(2), m(3), m(4), m(5));
    s.push_str("<pageSetup");
    if let Some(id) = paper_id(&p.paper)
        && id != 1
    {
        let _ = write!(s, " paperSize=\"{id}\"");
    }
    if p.scale != 100 && (10..=400).contains(&p.scale) {
        let _ = write!(s, " scale=\"{}\"", p.scale);
    }
    if let Some((w, h)) = p.fit_to {
        let _ = write!(s, " fitToWidth=\"{w}\" fitToHeight=\"{h}\"");
    }
    let _ = write!(s, " orientation=\"{}\"/>", if p.orientation == Orientation::Landscape { "landscape" } else { "portrait" });
    if !p.header.is_empty() || !p.footer.is_empty() {
        s.push_str("<headerFooter>");
        if !p.header.is_empty() {
            let _ = write!(s, "<oddHeader>{}</oddHeader>", esc(&p.header));
        }
        if !p.footer.is_empty() {
            let _ = write!(s, "<oddFooter>{}</oddFooter>", esc(&p.footer));
        }
        s.push_str("</headerFooter>");
    }
    for (tag, list, max) in [("rowBreaks", &p.row_breaks, 16383u32), ("colBreaks", &p.col_breaks, 1048575)] {
        let list: Vec<u32> = list.iter().copied().filter(|&b| b > 0).collect();
        if !list.is_empty() {
            let _ = write!(s, "<{tag} count=\"{0}\" manualBreakCount=\"{0}\">", list.len());
            for b in list {
                let _ = write!(s, "<brk id=\"{b}\" max=\"{max}\" man=\"1\"/>");
            }
            let _ = write!(s, "</{tag}>");
        }
    }

    // Drawing: pictures, charts, shapes.
    if !sheet.images.is_empty() || !sheet.charts.is_empty() || !sheet.shapes.is_empty() {
        out.drawings += 1;
        let dn = out.drawings;
        let mut drels = Rels::default();
        let mut objs: Vec<Obj<'_>> = vec![];
        for img in &sheet.images {
            if img.data.is_empty() {
                continue;
            }
            out.images += 1;
            let ext = image_ext(&img.mime, &img.data);
            let name = format!("xl/media/image{}.{ext}", out.images);
            out.default_type(ext, image_content_type(ext));
            out.part(&name, None, img.data.clone());
            let rid = drels.add("image", &format!("../media/image{}.{ext}", out.images));
            objs.push(Obj::Image(img, rid));
        }
        for ch in &sheet.charts {
            out.charts += 1;
            let name = format!("xl/charts/chart{}.xml", out.charts);
            out.part(&name, Some(CT_CHART), crate::chart::write_chart(wb, si, ch).into_bytes());
            let rid = drels.add("chart", &format!("../charts/chart{}.xml", out.charts));
            objs.push(Obj::Chart(ch, rid));
        }
        for sh in &sheet.shapes {
            objs.push(Obj::Shape(sh));
        }
        out.part(&format!("xl/drawings/drawing{dn}.xml"), Some(CT_DRAWING), crate::drawing::write_drawing(sheet, &wb.theme, &objs).into_bytes());
        if !drels.is_empty() {
            out.part(&format!("xl/drawings/_rels/drawing{dn}.xml.rels"), None, drels.xml().into_bytes());
        }
        let rid = rels.add("drawing", &format!("../drawings/drawing{dn}.xml"));
        let _ = write!(s, "<drawing r:id=\"{rid}\"/>");
    }
    // Notes: comments part + VML so Excel shows them.
    if !sheet.comments.is_empty() {
        out.comments += 1;
        let n = out.comments;
        out.part(&format!("xl/comments{n}.xml"), Some(CT_COMMENTS), comments_xml(sheet).into_bytes());
        out.default_type("vml", "application/vnd.openxmlformats-officedocument.vmlDrawing");
        out.part(&format!("xl/drawings/vmlDrawing{n}.vml"), None, vml_xml(sheet, n).into_bytes());
        rels.add("comments", &format!("../comments{n}.xml"));
        let rid = rels.add("vmlDrawing", &format!("../drawings/vmlDrawing{n}.vml"));
        let _ = write!(s, "<legacyDrawing r:id=\"{rid}\"/>");
    }
    // Tables.
    if !tables.is_empty() {
        let mut parts = String::new();
        let mut used_names: HashSet<String> = HashSet::new();
        for (t, cols) in &tables {
            out.tables += 1;
            out.table_id += 1;
            let n = out.tables;
            let name = table_name(&t.name, &mut used_names, out.table_id);
            out.part(&format!("xl/tables/table{n}.xml"), Some(CT_TABLE), table_xml(t, cols, &name, out.table_id).into_bytes());
            let rid = rels.add("table", &format!("../tables/table{n}.xml"));
            let _ = write!(parts, "<tablePart r:id=\"{rid}\"/>");
        }
        let _ = write!(s, "<tableParts count=\"{}\">{parts}</tableParts>", tables.len());
    }
    // Sparklines (Excel 2010 extension).
    if !sheet.sparklines.is_empty() {
        s.push_str("<extLst><ext uri=\"{05C60535-1F16-4fd2-B633-F4F36F0B64E0}\" xmlns:x14=\"http://schemas.microsoft.com/office/spreadsheetml/2009/9/main\"><x14:sparklineGroups xmlns:xm=\"http://schemas.microsoft.com/office/excel/2006/main\">");
        for sp in &sheet.sparklines {
            let ty = match sp.kind {
                SparklineKind::Line => "",
                SparklineKind::Column => " type=\"column\"",
                SparklineKind::WinLoss => " type=\"stacked\"",
            };
            let src =
                if sp.source.contains('!') { sp.source.clone() } else { format!("{}!{}", gridcraft_formula::quote_sheet(&sheet.name), sp.source) };
            let color = if sp.color == Color::Auto { "rgb=\"FF376092\"".to_string() } else { color_attrs(&sp.color) };
            let _ = write!(
                s,
                "<x14:sparklineGroup displayEmptyCellsAs=\"gap\"{ty}{}><x14:colorSeries {color}/><x14:sparklines><x14:sparkline><xm:f>{}</xm:f><xm:sqref>{}</xm:sqref></x14:sparkline></x14:sparklines></x14:sparklineGroup>",
                if sp.markers { " markers=\"1\"" } else { "" },
                esc(&src),
                sp.cell.a1()
            );
        }
        s.push_str("</x14:sparklineGroups></ext></extLst>");
    }
    s.push_str("</worksheet>");
    (s, rels)
}

fn write_cell(
    s: &mut String,
    c: CellRef,
    cell: Option<&Cell>,
    picture: Option<&std::sync::Arc<gridcraft_model::CellPicture>>,
    ov: Option<&String>,
    extra: Option<&Value>,
    dynamic: Option<&RangeRef>,
    sid: &dyn Fn(gridcraft_model::StyleId) -> u32,
    out: &mut Out,
) {
    let style = cell.map(|x| sid(x.style)).unwrap_or(0);
    let _ = write!(s, "<c r=\"{}\"", c.a1());
    if style != 0 {
        let _ = write!(s, " s=\"{style}\"");
    }
    if let Some(picture) = picture {
        let vm = out.cell_pictures.index(picture);
        let _ = write!(s, " t=\"e\" vm=\"{vm}\"><v>#VALUE!</v></c>");
        return;
    }
    let formula = if ov.is_some() { None } else { cell.and_then(|x| x.formula.as_ref()) };
    let value: Value = match (ov, extra) {
        (Some(t), _) => Value::text(t.as_str()),
        (None, Some(v)) => v.clone(),
        _ => cell.map(|x| x.value.scalar()).unwrap_or(Value::Empty),
    };
    let value = match value {
        Value::Array(a) => a.get(0, 0).cloned().unwrap_or(Value::Empty),
        v => v,
    };
    let value = match value {
        Value::Error(CellError::Circ) => Value::Number(0.0),
        // #SPILL! and #CALC! are not error codes of the file format (Excel refuses a file that
        // caches them); Excel itself caches them as #VALUE!.
        Value::Error(CellError::Spill | CellError::Calc) => Value::Error(CellError::Value),
        v => v,
    };
    if formula.is_some() && dynamic.is_some() {
        s.push_str(" cm=\"1\"");
        out.dynamic_arrays = true;
    }
    if let Some(f) = formula {
        let t = match &value {
            Value::Text(_) => " t=\"str\"",
            Value::Bool(_) => " t=\"b\"",
            Value::Error(_) => " t=\"e\"",
            _ => "",
        };
        s.push_str(t);
        s.push('>');
        let text = esc(&crate::fmla::to_file(f));
        match (f.array, dynamic) {
            (Some(r), _) => {
                let _ = write!(s, "<f t=\"array\" ref=\"{}\">{text}</f>", r.a1());
            }
            (None, Some(r)) => {
                let _ = write!(s, "<f t=\"array\" ref=\"{}\">{text}</f>", r.a1());
            }
            _ => {
                let _ = write!(s, "<f>{text}</f>");
            }
        }
        match &value {
            Value::Number(n) => {
                let _ = write!(s, "<v>{}</v>", num(*n));
            }
            Value::Text(t) => {
                let _ = write!(s, "<v>{}</v>", esc(&encode_xstring(t)));
            }
            Value::Bool(b) => {
                let _ = write!(s, "<v>{}</v>", *b as u8);
            }
            Value::Error(e) => {
                let _ = write!(s, "<v>{}</v>", e.as_str());
            }
            _ => out.needs_calc = true,
        }
        s.push_str("</c>");
        return;
    }
    match &value {
        Value::Empty | Value::Array(_) => s.push_str("/>"),
        Value::Number(n) => {
            let _ = write!(s, "><v>{}</v></c>", num(*n));
        }
        Value::Text(t) => {
            let i = out.sst.index(t);
            let _ = write!(s, " t=\"s\"><v>{i}</v></c>");
        }
        Value::Bool(b) => {
            let _ = write!(s, " t=\"b\"><v>{}</v></c>", *b as u8);
        }
        Value::Error(e) => {
            let _ = write!(s, " t=\"e\"><v>{}</v></c>", e.as_str());
        }
    }
}

fn cfvo_xml(v: &CfValueKind) -> String {
    match v {
        CfValueKind::Min => "<cfvo type=\"min\"/>".into(),
        CfValueKind::Max => "<cfvo type=\"max\"/>".into(),
        CfValueKind::Number(n) => format!("<cfvo type=\"num\" val=\"{}\"/>", num(*n)),
        CfValueKind::Percent(n) => format!("<cfvo type=\"percent\" val=\"{}\"/>", num(*n)),
        CfValueKind::Percentile(n) => format!("<cfvo type=\"percentile\" val=\"{}\"/>", num(*n)),
        CfValueKind::Formula(f) => format!("<cfvo type=\"formula\" val=\"{}\"/>", esc_attr(&text_to_file(f))),
    }
}

fn quote(t: &str) -> String {
    format!("\"{}\"", t.replace('"', "\"\""))
}

fn cf_rule_xml(cf: &CondFormat, priority: u32, out: &mut Out) -> String {
    let tl = cf.ranges.first().map(|r| r.start.a1()).unwrap_or_else(|| "A1".into());
    let stop = if cf.stop_if_true { " stopIfTrue=\"1\"" } else { "" };
    let mut dxf = |st: &Style| dxf_id(&mut out.dxfs, st);
    let f = |x: &str| format!("<formula>{}</formula>", esc(&text_to_file(x)));
    let simple = |ty: &str, st: &Style, extra: &str, body: String, dxf: &mut dyn FnMut(&Style) -> u32| {
        format!("<cfRule type=\"{ty}\" dxfId=\"{}\" priority=\"{priority}\"{stop}{extra}>{body}</cfRule>", dxf(st))
    };
    match &cf.rule {
        CfRule::CellIs { op, a, b, style } => {
            let mut body = f(a);
            if let Some(b) = b {
                body.push_str(&f(b));
            }
            simple("cellIs", style, &format!(" operator=\"{}\"", cf_op_name(*op)), body, &mut dxf)
        }
        CfRule::Expression { formula, style } => simple("expression", style, "", f(formula), &mut dxf),
        CfRule::ContainsText { text, style } => simple(
            "containsText",
            style,
            &format!(" operator=\"containsText\" text=\"{}\"", esc_attr(text)),
            f(&format!("NOT(ISERROR(SEARCH({},{tl})))", quote(text))),
            &mut dxf,
        ),
        CfRule::NotContainsText { text, style } => simple(
            "notContainsText",
            style,
            &format!(" operator=\"notContains\" text=\"{}\"", esc_attr(text)),
            f(&format!("ISERROR(SEARCH({},{tl}))", quote(text))),
            &mut dxf,
        ),
        CfRule::BeginsWith { text, style } => simple(
            "beginsWith",
            style,
            &format!(" operator=\"beginsWith\" text=\"{}\"", esc_attr(text)),
            f(&format!("LEFT({tl},LEN({0}))={0}", quote(text))),
            &mut dxf,
        ),
        CfRule::EndsWith { text, style } => simple(
            "endsWith",
            style,
            &format!(" operator=\"endsWith\" text=\"{}\"", esc_attr(text)),
            f(&format!("RIGHT({tl},LEN({0}))={0}", quote(text))),
            &mut dxf,
        ),
        CfRule::Blanks { style } => simple("containsBlanks", style, "", f(&format!("LEN(TRIM({tl}))=0")), &mut dxf),
        CfRule::NoBlanks { style } => simple("notContainsBlanks", style, "", f(&format!("LEN(TRIM({tl}))>0")), &mut dxf),
        CfRule::Errors { style } => simple("containsErrors", style, "", f(&format!("ISERROR({tl})")), &mut dxf),
        CfRule::NoErrors { style } => simple("notContainsErrors", style, "", f(&format!("NOT(ISERROR({tl}))")), &mut dxf),
        CfRule::Duplicate { style } => simple("duplicateValues", style, "", String::new(), &mut dxf),
        CfRule::Unique { style } => simple("uniqueValues", style, "", String::new(), &mut dxf),
        CfRule::Top10 { bottom, percent, rank, style } => {
            let extra = format!("{}{} rank=\"{}\"", if *percent { " percent=\"1\"" } else { "" }, if *bottom { " bottom=\"1\"" } else { "" }, rank);
            simple("top10", style, &extra, String::new(), &mut dxf)
        }
        CfRule::AboveAverage { below, equal, std_dev, style } => {
            let mut extra = String::new();
            if *below {
                extra.push_str(" aboveAverage=\"0\"");
            }
            if *equal {
                extra.push_str(" equalAverage=\"1\"");
            }
            if *std_dev > 0 {
                let _ = write!(extra, " stdDev=\"{std_dev}\"");
            }
            simple("aboveAverage", style, &extra, String::new(), &mut dxf)
        }
        CfRule::TimePeriod { period, style } => {
            let d = format!("FLOOR({tl},1)");
            let body = match period.as_str() {
                "yesterday" => format!("{d}=TODAY()-1"),
                "tomorrow" => format!("{d}=TODAY()+1"),
                "last7Days" => format!("AND(TODAY()-{d}<=6,{d}<=TODAY())"),
                "thisMonth" => format!("AND(MONTH({tl})=MONTH(TODAY()),YEAR({tl})=YEAR(TODAY()))"),
                "lastMonth" => format!("AND(MONTH({tl})=MONTH(EDATE(TODAY(),0-1)),YEAR({tl})=YEAR(EDATE(TODAY(),0-1)))"),
                "nextMonth" => format!("AND(MONTH({tl})=MONTH(EDATE(TODAY(),0+1)),YEAR({tl})=YEAR(EDATE(TODAY(),0+1)))"),
                "thisWeek" => format!("AND(TODAY()-ROUNDDOWN({tl},0)<=WEEKDAY(TODAY())-1,ROUNDDOWN({tl},0)-TODAY()<=7-WEEKDAY(TODAY()))"),
                "lastWeek" => format!("AND(TODAY()-ROUNDDOWN({tl},0)>=(WEEKDAY(TODAY())),TODAY()-ROUNDDOWN({tl},0)<(WEEKDAY(TODAY())+7))"),
                "nextWeek" => format!("AND(ROUNDDOWN({tl},0)-TODAY()>(7-WEEKDAY(TODAY())),ROUNDDOWN({tl},0)-TODAY()<(15-WEEKDAY(TODAY())))"),
                _ => format!("{d}=TODAY()"),
            };
            simple("timePeriod", style, &format!(" timePeriod=\"{}\"", esc_attr(period)), f(&body), &mut dxf)
        }
        CfRule::ColorScale { stops } => {
            let mut b = String::from("<colorScale>");
            for (v, _) in stops {
                b.push_str(&cfvo_xml(v));
            }
            for (_, c) in stops {
                let _ = write!(b, "<color {}/>", color_attrs(c));
            }
            b.push_str("</colorScale>");
            format!("<cfRule type=\"colorScale\" priority=\"{priority}\"{stop}>{b}</cfRule>")
        }
        CfRule::DataBar { min, max, color, show_value, .. } => format!(
            "<cfRule type=\"dataBar\" priority=\"{priority}\"{stop}><dataBar{}>{}{}<color {}/></dataBar></cfRule>",
            if *show_value { "" } else { " showValue=\"0\"" },
            cfvo_xml(min),
            cfvo_xml(max),
            if *color == Color::Auto { "rgb=\"FF638EC6\"".to_string() } else { color_attrs(color) }
        ),
        CfRule::IconSet { set, thresholds, reverse, show_value } => {
            let mut b = format!("<iconSet iconSet=\"{}\"", esc_attr(set));
            if *reverse {
                b.push_str(" reverse=\"1\"");
            }
            if !*show_value {
                b.push_str(" showValue=\"0\"");
            }
            b.push('>');
            for t in thresholds {
                b.push_str(&cfvo_xml(t));
            }
            b.push_str("</iconSet>");
            format!("<cfRule type=\"iconSet\" priority=\"{priority}\"{stop}>{b}</cfRule>")
        }
    }
}

fn validation_formula(f: &str) -> String {
    let f = f.trim();
    if f.starts_with('"') {
        return esc(f);
    }
    esc(&text_to_file(f.strip_prefix('=').unwrap_or(f)))
}

fn validation_xml(v: &Validation) -> String {
    let ty = match v.kind {
        ValidationKind::Any => "none",
        ValidationKind::Whole => "whole",
        ValidationKind::Decimal => "decimal",
        ValidationKind::List => "list",
        ValidationKind::Date => "date",
        ValidationKind::Time => "time",
        ValidationKind::TextLength => "textLength",
        ValidationKind::Custom => "custom",
    };
    let mut s = format!("<dataValidation type=\"{ty}\"");
    match v.error_style {
        ErrorStyle::Stop => {}
        ErrorStyle::Warning => s.push_str(" errorStyle=\"warning\""),
        ErrorStyle::Information => s.push_str(" errorStyle=\"information\""),
    }
    if v.op != CfOperator::Between && !matches!(v.kind, ValidationKind::List | ValidationKind::Custom | ValidationKind::Any) {
        let _ = write!(s, " operator=\"{}\"", cf_op_name(v.op));
    }
    if v.allow_blank {
        s.push_str(" allowBlank=\"1\"");
    }
    if !v.in_cell_dropdown {
        s.push_str(" showDropDown=\"1\"");
    }
    if v.show_input {
        s.push_str(" showInputMessage=\"1\"");
    }
    if v.show_error {
        s.push_str(" showErrorMessage=\"1\"");
    }
    for (a, t) in [("errorTitle", &v.error_title), ("error", &v.error_message), ("promptTitle", &v.input_title), ("prompt", &v.input_message)] {
        if !t.is_empty() {
            let _ = write!(s, " {a}=\"{}\"", esc_attr(t));
        }
    }
    let _ = write!(s, " sqref=\"{}\">", sqref(&v.ranges));
    if !v.f1.is_empty() {
        let _ = write!(s, "<formula1>{}</formula1>", validation_formula(&v.f1));
    }
    if let Some(f2) = v.f2.as_ref().filter(|f| !f.is_empty()) {
        let _ = write!(s, "<formula2>{}</formula2>", validation_formula(f2));
    }
    s.push_str("</dataValidation>");
    s
}

fn filter_operator(op: &str) -> &'static str {
    match op.trim() {
        "<" => " operator=\"lessThan\"",
        "<=" => " operator=\"lessThanOrEqual\"",
        "<>" => " operator=\"notEqual\"",
        ">=" => " operator=\"greaterThanOrEqual\"",
        ">" => " operator=\"greaterThan\"",
        _ => "",
    }
}

fn autofilter_xml(af: &AutoFilter, out: &mut Out) -> String {
    let mut s = format!("<autoFilter ref=\"{}\"", af.range.a1());
    if af.criteria.is_empty() {
        s.push_str("/>");
        return s;
    }
    s.push('>');
    for (col, c) in &af.criteria {
        let _ = write!(s, "<filterColumn colId=\"{col}\">");
        match c {
            FilterCriterion::Values { values, blanks } => {
                let _ = write!(s, "<filters{}>", if *blanks { " blank=\"1\"" } else { "" });
                for v in values {
                    let _ = write!(s, "<filter val=\"{}\"/>", esc_attr(v));
                }
                s.push_str("</filters>");
            }
            FilterCriterion::Custom { a, b, and } => {
                let _ = write!(s, "<customFilters{}>", if *and { " and=\"1\"" } else { "" });
                for (op, v) in std::iter::once(a).chain(b.iter()) {
                    let _ = write!(s, "<customFilter{} val=\"{}\"/>", filter_operator(op), esc_attr(v));
                }
                s.push_str("</customFilters>");
            }
            FilterCriterion::Top10 { bottom, percent, count } => {
                let _ =
                    write!(s, "<top10{}{} val=\"{count}\"/>", if *bottom { " top=\"0\"" } else { "" }, if *percent { " percent=\"1\"" } else { "" });
            }
            FilterCriterion::AboveAverage(above) => {
                let _ = write!(s, "<dynamicFilter type=\"{}\"/>", if *above { "aboveAverage" } else { "belowAverage" });
            }
            FilterCriterion::FillColor(c) => {
                let st = Style { fill: gridcraft_model::Fill::solid(*c), ..Default::default() };
                let _ = write!(s, "<colorFilter dxfId=\"{}\"/>", dxf_id(&mut out.dxfs, &st));
            }
            FilterCriterion::FontColor(c) => {
                let mut st = Style::default();
                st.font.color = *c;
                let _ = write!(s, "<colorFilter dxfId=\"{}\" cellColor=\"0\"/>", dxf_id(&mut out.dxfs, &st));
            }
        }
        s.push_str("</filterColumn>");
    }
    s.push_str("</autoFilter>");
    s
}

fn table_xml(t: &Table, cols: &[String], name: &str, id: u32) -> String {
    let mut s =
        format!("{XML_DECL}<table xmlns=\"{NS_MAIN}\" id=\"{id}\" name=\"{0}\" displayName=\"{0}\" ref=\"{1}\"", esc_attr(name), t.range.a1());
    if !t.header_row {
        s.push_str(" headerRowCount=\"0\"");
    }
    if t.totals_row {
        s.push_str(" totalsRowCount=\"1\"");
    } else {
        s.push_str(" totalsRowShown=\"0\"");
    }
    s.push('>');
    if t.header_row && t.filter_button {
        let end_row = if t.totals_row { t.range.end.row.saturating_sub(1).max(t.range.start.row) } else { t.range.end.row };
        let r = RangeRef::new(t.range.start, CellRef::new(end_row, t.range.end.col));
        let _ = write!(s, "<autoFilter ref=\"{}\"/>", r.a1());
    }
    let _ = write!(s, "<tableColumns count=\"{}\">", cols.len());
    for (i, name) in cols.iter().enumerate() {
        let col = t.columns.get(i);
        let _ = write!(s, "<tableColumn id=\"{}\" name=\"{}\"", i + 1, esc_attr(&encode_xstring(name)));
        let totals = col.map(|c| c.totals).unwrap_or_default();
        if t.totals_row {
            let f = match totals {
                TotalsFn::None => None,
                TotalsFn::Average => Some("average"),
                TotalsFn::Count => Some("count"),
                TotalsFn::CountNums => Some("countNums"),
                TotalsFn::Max => Some("max"),
                TotalsFn::Min => Some("min"),
                TotalsFn::Sum => Some("sum"),
                TotalsFn::StdDev => Some("stdDev"),
                TotalsFn::Var => Some("var"),
                TotalsFn::Custom => Some("custom"),
            };
            if let Some(f) = f {
                let _ = write!(s, " totalsRowFunction=\"{f}\"");
            } else if let Some(l) = col.and_then(|c| c.totals_label.as_ref()) {
                let _ = write!(s, " totalsRowLabel=\"{}\"", esc_attr(l));
            }
        }
        let calc = col.and_then(|c| c.formula.as_ref()).filter(|f| !f.is_empty());
        let custom = if t.totals_row && totals == TotalsFn::Custom { col.and_then(|c| c.totals_label.as_ref()) } else { None };
        if calc.is_none() && custom.is_none() {
            s.push_str("/>");
            continue;
        }
        s.push('>');
        if let Some(f) = calc {
            let _ = write!(s, "<calculatedColumnFormula>{}</calculatedColumnFormula>", esc(&text_to_file(f)));
        }
        if let Some(f) = custom {
            let _ = write!(s, "<totalsRowFormula>{}</totalsRowFormula>", esc(&text_to_file(f)));
        }
        s.push_str("</tableColumn>");
    }
    s.push_str("</tableColumns>");
    let style = if t.style.is_empty() { "TableStyleMedium2" } else { &t.style };
    let _ = write!(
        s,
        "<tableStyleInfo name=\"{}\" showFirstColumn=\"{}\" showLastColumn=\"{}\" showRowStripes=\"{}\" showColumnStripes=\"{}\"/></table>",
        esc_attr(style),
        t.first_col as u8,
        t.last_col as u8,
        t.banded_rows as u8,
        t.banded_cols as u8
    );
    s
}

fn comments_xml(sheet: &Sheet) -> String {
    let mut authors: Vec<String> = vec![];
    for c in sheet.comments.values() {
        if !authors.contains(&c.author) {
            authors.push(c.author.clone());
        }
    }
    let mut s = format!("{XML_DECL}<comments xmlns=\"{NS_MAIN}\"><authors>");
    for a in &authors {
        let _ = write!(s, "<author>{}</author>", esc(a));
    }
    s.push_str("</authors><commentList>");
    for (cell, c) in &sheet.comments {
        let aid = authors.iter().position(|a| *a == c.author).unwrap_or(0);
        let mut text = c.text.clone();
        // Threaded replies are kept as text lines.
        for (who, reply) in &c.replies {
            let _ = write!(text, "\n{who}: {reply}");
        }
        let _ = write!(s, "<comment ref=\"{}\" authorId=\"{aid}\"><text><r>{}</r></text></comment>", cell.a1(), crate::xml::t_el(&text));
    }
    s.push_str("</commentList></comments>");
    s
}

/// Legacy VML shapes Excel uses to display notes.
fn vml_xml(sheet: &Sheet, n: u32) -> String {
    let mut s = format!(
        "<xml xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:x=\"urn:schemas-microsoft-com:office:excel\"><o:shapelayout v:ext=\"edit\"><o:idmap v:ext=\"edit\" data=\"{n}\"/></o:shapelayout><v:shapetype id=\"_x0000_t202\" coordsize=\"21600,21600\" o:spt=\"202\" path=\"m,l,21600r21600,l21600,xe\"><v:stroke joinstyle=\"miter\"/><v:path gradientshapeok=\"t\" o:connecttype=\"rect\"/></v:shapetype>"
    );
    for (i, (cell, c)) in sheet.comments.iter().enumerate() {
        let id = n as u64 * 1024 + 1 + i as u64;
        let vis = if c.visible { "visible" } else { "hidden" };
        let _ = write!(
            s,
            "<v:shape id=\"_x0000_s{id}\" type=\"#_x0000_t202\" style=\"position:absolute;margin-left:59.25pt;margin-top:1.5pt;width:108pt;height:59.25pt;z-index:{};visibility:{vis}\" fillcolor=\"#ffffe1\" o:insetmode=\"auto\"><v:fill color2=\"#ffffe1\"/><v:shadow on=\"t\" color=\"black\" obscured=\"t\"/><v:path o:connecttype=\"none\"/><v:textbox style=\"mso-direction-alt:auto\"><div style=\"text-align:left\"></div></v:textbox><x:ClientData ObjectType=\"Note\"><x:MoveWithCells/><x:SizeWithCells/><x:Anchor>{}, 15, {}, 2, {}, 15, {}, 16</x:Anchor><x:AutoFill>False</x:AutoFill><x:Row>{}</x:Row><x:Column>{}</x:Column>{}</x:ClientData></v:shape>",
            i + 1,
            cell.col + 1,
            cell.row,
            cell.col + 3,
            cell.row + 4,
            cell.row,
            cell.col,
            if c.visible { "<x:Visible/>" } else { "" }
        );
    }
    s.push_str("</xml>");
    s
}
