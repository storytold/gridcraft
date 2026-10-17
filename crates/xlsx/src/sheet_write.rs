//! Writing worksheet parts and the parts they own (tables, comments, drawings, charts).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_model::{
    Cell, CfOperator, CfRule, CfValueKind, Color, CondFormat, ErrorStyle, FilterCriterion, LineInfo, Orientation, Sheet, SparklineKind, Style, Table,
    TotalsFn, Validation, ValidationKind, Workbook, default_icon_thresholds, icon_count,
};

use crate::drawing::{Obj, image_content_type, image_ext};
use crate::fmla::text_to_file;
use crate::styles::{color_attrs, dxf_id};
use crate::tables::{paper_id, px_to_col_width, px_to_pt};
use crate::write::{
    CT_CHART, CT_COMMENTS, CT_DRAWING, CT_TABLE, CT_THREADED_COMMENTS, NS_MAIN, NS_REL, NS_THREADED, Out, REL_THREADED_COMMENT, Rels, XML_DECL,
};
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

/// Valid table name: letters, digits, `_` and `.`, starting with a letter, `_` or `\`
/// (Microsoft "Names in formulas" rules).
fn table_name(name: &str, used: &mut HashSet<String>, n: u32) -> String {
    let mut s: String =
        name.chars().enumerate().map(|(i, c)| if c.is_alphanumeric() || c == '_' || c == '.' || (i == 0 && c == '\\') { c } else { '_' }).collect();
    if s.is_empty() || !s.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '\\') || gridcraft_core::CellRef::parse(&s).is_some() {
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
            let base = t.columns.get(i).map(|c| c.name.clone()).filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("Column{}", i + 1));
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
        .filter(|t| t.range.height() > t.header_row as u32 + t.totals_row as u32 || (t.header_row && t.range.height() > t.totals_row as u32))
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
    // Values inside spills and legacy array ranges.
    for (a, r) in sheet.spill_ranges.iter().filter(|(_, r)| !r.is_single()) {
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
    if sheet.view != gridcraft_model::SheetViewMode::Normal {
        let _ = write!(s, " view=\"{}\"", sheet.view.as_str());
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
                CellRef::new(
                    rows.max(sheet.view_top_left.row).min(gridcraft_core::MAX_ROWS - 1),
                    cols.max(sheet.view_top_left.col).min(gridcraft_core::MAX_COLS - 1)
                )
                .a1()
            );
            let _ = write!(s, "<selection pane=\"{pane}\" activeCell=\"{active}\" sqref=\"{active}\"/>");
        }
        _ => {
            let _ = write!(s, "<selection activeCell=\"{active}\" sqref=\"{active}\"/>");
        }
    }
    write_focus_cell(&mut s, sheet);
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
            let array = dynamic.get(c).copied().or_else(|| {
                let f = cell?.formula.as_ref()?;
                (f.array.is_none() && crate::array_formula::needs_array(wb, si, *c, f)).then_some(RangeRef::cell(*c))
            });
            write_cell(&mut s, *c, cell, sheet.cell_pictures.get(c), ov, ex, array.as_ref(), &sid, out);
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
        if let Some(h) = &p.password {
            s.push_str(&password_attrs(h, ""));
        }
        s.push_str("/>");
    }
    s.push_str(&protected_ranges_xml(sheet));
    // autoFilter (a sheet filter overlapping a table would be invalid).
    if let Some(af) = &sheet.autofilter
        && !sheet.tables.iter().any(|t| t.range.intersects(&af.range))
    {
        s.push_str(&autofilter_xml(af.range, &af.criteria, out));
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
            // `external` is what the file said, or what Insert › Link chose; the scheme test covers
            // links made before the flag existed.
            let external = h.external
                || t.contains("://")
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
    // Unmodelled content kept from the source file (printer settings, controls, OLE objects…).
    let kept = crate::passthrough::SheetKept::write(sheet, &mut rels, out);
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
    let _ = write!(s, " orientation=\"{}\"", if p.orientation == Orientation::Landscape { "landscape" } else { "portrait" });
    if let Some(id) = kept.page_setup_id() {
        let _ = write!(s, " r:id=\"{id}\"");
    }
    s.push_str("/>");
    if p.has_header_footer() || p.different_first || p.different_odd_even || !p.hf_scale_with_doc || !p.hf_align_with_margins {
        s.push_str("<headerFooter");
        for (attr, on, default) in [
            ("differentOddEven", p.different_odd_even, false),
            ("differentFirst", p.different_first, false),
            ("scaleWithDoc", p.hf_scale_with_doc, true),
            ("alignWithMargins", p.hf_align_with_margins, true),
        ] {
            if on != default {
                let _ = write!(s, " {attr}=\"{}\"", u8::from(on));
            }
        }
        s.push('>');
        // CT_HeaderFooter's element order.
        for (tag, text) in [
            ("oddHeader", &p.header),
            ("oddFooter", &p.footer),
            ("evenHeader", &p.even_header),
            ("evenFooter", &p.even_footer),
            ("firstHeader", &p.first_header),
            ("firstFooter", &p.first_footer),
        ] {
            if !text.is_empty() {
                let _ = write!(s, "<{tag}>{}</{tag}>", esc(text));
            }
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
            let rid = if crate::chartex::is_chartex(ch.kind) {
                let n = out.charts;
                let xml = crate::chartex::write_chartex(wb, si, ch, &mut out.chart_names);
                out.part(&format!("xl/charts/chartEx{n}.xml"), Some(crate::chartex::CT_CHARTEX), xml.into_bytes());
                // Excel expects a chartex part's style and colour parts.
                out.part(&format!("xl/charts/style{n}.xml"), Some(crate::chartex::CT_STYLE), crate::chartex::style_xml().into_bytes());
                out.part(&format!("xl/charts/colors{n}.xml"), Some(crate::chartex::CT_COLORS), crate::chartex::colors_xml().into_bytes());
                let mut crels = Rels::default();
                crels.add_uri(crate::chartex::REL_STYLE, &format!("style{n}.xml"));
                crels.add_uri(crate::chartex::REL_COLORS, &format!("colors{n}.xml"));
                out.part(&format!("xl/charts/_rels/chartEx{n}.xml.rels"), None, crels.xml().into_bytes());
                drels.add_uri(crate::chartex::REL_CHARTEX, &format!("../charts/chartEx{n}.xml"))
            } else {
                let name = format!("xl/charts/chart{}.xml", out.charts);
                out.part(&name, Some(CT_CHART), crate::chart::write_chart(wb, si, ch).into_bytes());
                drels.add("chart", &format!("../charts/chart{}.xml", out.charts))
            };
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
    // Notes: comments part + VML so Excel shows them. Threaded comments also get a legacy note
    // (its text with the replies as lines, for readers without threaded comments) linked by id.
    if !sheet.comments.is_empty() {
        out.comments += 1;
        let n = out.comments;
        let ids = thread_ids(sheet, si);
        out.part(&format!("xl/comments{n}.xml"), Some(CT_COMMENTS), comments_xml(sheet, &ids).into_bytes());
        if !ids.is_empty() {
            let x = threaded_xml(sheet, &ids, &mut out.persons);
            out.part(&format!("xl/threadedComments/threadedComment{n}.xml"), Some(CT_THREADED_COMMENTS), x.into_bytes());
            rels.add_uri(REL_THREADED_COMMENT, &format!("../threadedComments/threadedComment{n}.xml"));
        }
        out.default_type("vml", "application/vnd.openxmlformats-officedocument.vmlDrawing");
        let vml_name = format!("xl/drawings/vmlDrawing{n}.vml");
        let vml = kept.notes_vml(vml_xml(sheet, n), &vml_name, out);
        out.part(&vml_name, None, vml.into_bytes());
        rels.add("comments", &format!("../comments{n}.xml"));
        let rid = rels.add("vmlDrawing", &format!("../drawings/vmlDrawing{n}.vml"));
        let _ = write!(s, "<legacyDrawing r:id=\"{rid}\"/>");
    } else if let Some(rid) = kept.lone_vml(&mut rels, out) {
        let _ = write!(s, "<legacyDrawing r:id=\"{rid}\"/>");
    }
    s.push_str(&kept.elements());
    // Tables.
    if !tables.is_empty() {
        let mut parts = String::new();
        let mut used_names: HashSet<String> = HashSet::new();
        for (t, cols) in &tables {
            out.tables += 1;
            out.table_id += 1;
            let n = out.tables;
            let name = table_name(&t.name, &mut used_names, out.table_id);
            let xml = table_xml(t, cols, &name, out.table_id, out);
            out.part(&format!("xl/tables/table{n}.xml"), Some(CT_TABLE), xml.into_bytes());
            let rid = rels.add("table", &format!("../tables/table{n}.xml"));
            let _ = write!(parts, "<tablePart r:id=\"{rid}\"/>");
        }
        let _ = write!(s, "<tableParts count=\"{}\">{parts}</tableParts>", tables.len());
    }
    // Excel 2010 extensions: solid data bars, then sparklines.
    let solid_bars: Vec<(&CondFormat, u32)> = sheet
        .cond_formats
        .iter()
        .enumerate()
        .filter(|(_, cf)| !cf.ranges.is_empty() && matches!(cf.rule, CfRule::DataBar { gradient: false, .. }))
        .map(|(i, cf)| (cf, prio.get(i).copied().unwrap_or(1)))
        .collect();
    if !solid_bars.is_empty() || !sheet.sparklines.is_empty() {
        s.push_str("<extLst>");
    }
    if !solid_bars.is_empty() {
        let _ = write!(
            s,
            "<ext uri=\"{{78C0D931-6437-407d-A8A1-0F90B4350D7D}}\" xmlns:x14=\"{X14_NS}\" xmlns:xm=\"{XM_NS}\"><x14:conditionalFormattings>"
        );
        for (cf, p) in &solid_bars {
            if let CfRule::DataBar { min, max, .. } = &cf.rule {
                let (id, lo, hi) = (solid_bar_id(*p), x14_cfvo(min), x14_cfvo(max));
                let _ = write!(s, "<x14:conditionalFormatting><x14:cfRule type=\"dataBar\" id=\"{id}\"><x14:dataBar gradient=\"0\">{lo}{hi}");
                let _ = write!(s, "</x14:dataBar></x14:cfRule><xm:sqref>{}</xm:sqref></x14:conditionalFormatting>", sqref(&cf.ranges));
            }
        }
        s.push_str("</x14:conditionalFormattings></ext>");
    }
    if !sheet.sparklines.is_empty() {
        let _ = write!(s, "<ext uri=\"{{05C60535-1F16-4fd2-B633-F4F36F0B64E0}}\" xmlns:x14=\"{X14_NS}\"><x14:sparklineGroups xmlns:xm=\"{XM_NS}\">");
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
        s.push_str("</x14:sparklineGroups></ext>");
    }
    if !solid_bars.is_empty() || !sheet.sparklines.is_empty() {
        s.push_str("</extLst>");
    }
    s.push_str("</worksheet>");
    (s, rels)
}

/// The Focus Cell extension inside `<sheetView>`. Written whenever the sheet carries either
/// half of the setting, so a color chosen while the tint is off stays remembered.
fn write_focus_cell(s: &mut String, sheet: &Sheet) {
    if !sheet.focus_cell && sheet.focus_cell_color.is_none() {
        return;
    }
    let on = u8::from(sheet.focus_cell);
    let color = sheet.focus_cell_color.as_ref().map(|c| format!(" {}", color_attrs(c))).unwrap_or_default();
    let _ = write!(s, "<extLst><ext uri=\"{FOCUS_CELL_NS}\" xmlns:gcf=\"{FOCUS_CELL_NS}\"><gcf:focusCell on=\"{on}\"{color}/></ext></extLst>");
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
    // A non-spilling formula may still need array evaluation to avoid implicit intersection.
    if dynamic.is_some() && formula.is_some_and(|f| f.array.is_none()) {
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

/// Focus Cell is not an ISO/IEC 29500 sheet-view attribute, so it rides in the view's extension
/// list under GridCraft's own namespace. Excel does not read it, and keeps its own extensions
/// beside it.
const FOCUS_CELL_NS: &str = "https://github.com/storytold/gridcraft/xlsx/2026/focuscell";

const X14_NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const XM_NS: &str = "http://schemas.microsoft.com/office/excel/2006/main";

/// The id linking a solid data bar's `cfRule` to its x14 extension; unique per sheet because priorities are.
fn solid_bar_id(priority: u32) -> String {
    format!("{{00000000-0000-0000-0000-{priority:012}}}")
}

/// The `cfRule` extension that points a solid data bar at its x14 rule.
fn solid_bar_ext(priority: u32) -> String {
    let id = solid_bar_id(priority);
    format!("<extLst><ext uri=\"{{B025F937-C7B1-47D3-B67F-A62EFF666E3E}}\" xmlns:x14=\"{X14_NS}\"><x14:id>{id}</x14:id></ext></extLst>")
}

/// An x14 `cfvo`: unlike the 2006 one it has no `val` attribute, the value is an `xm:f` child.
fn x14_cfvo(v: &CfValueKind) -> String {
    let (ty, val) = match v {
        CfValueKind::Min => return "<x14:cfvo type=\"min\"/>".into(),
        CfValueKind::Max => return "<x14:cfvo type=\"max\"/>".into(),
        CfValueKind::Number(n) => ("num", num(*n)),
        CfValueKind::Percent(n) => ("percent", num(*n)),
        CfValueKind::Percentile(n) => ("percentile", num(*n)),
        CfValueKind::Formula(f) => ("formula", text_to_file(f)),
    };
    format!("<x14:cfvo type=\"{ty}\"><xm:f>{}</xm:f></x14:cfvo>", esc(&val))
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
        CfRule::DataBar { min, max, color, show_value, gradient } => format!(
            "<cfRule type=\"dataBar\" priority=\"{priority}\"{stop}><dataBar{}>{}{}<color {}/></dataBar>{}</cfRule>",
            if *show_value { "" } else { " showValue=\"0\"" },
            cfvo_xml(min),
            cfvo_xml(max),
            if *color == Color::Auto { "rgb=\"FF638EC6\"".to_string() } else { color_attrs(color) },
            if *gradient { String::new() } else { solid_bar_ext(priority) }
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
            // CT_IconSet needs one cfvo per icon (minOccurs 2); the first is the lower bound.
            let k = icon_count(set);
            let defaults = default_icon_thresholds(set);
            let cfvos: Vec<&CfValueKind> = match thresholds.len() {
                n if n == k => thresholds.iter().collect(),
                n if n + 1 == k => defaults.iter().take(1).chain(thresholds).collect(),
                _ => defaults.iter().collect(),
            };
            for t in cfvos {
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

fn autofilter_xml(range: RangeRef, criteria: &[(u32, FilterCriterion)], out: &mut Out) -> String {
    let mut s = format!("<autoFilter ref=\"{}\"", range.a1());
    if criteria.is_empty() {
        s.push_str("/>");
        return s;
    }
    s.push('>');
    for (col, c) in criteria {
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
            FilterCriterion::Icon { set, id } => {
                let _ = write!(s, "<iconFilter iconSet=\"{}\"", esc_attr(set));
                if let Some(id) = id {
                    let _ = write!(s, " iconId=\"{id}\"");
                }
                s.push_str("/>");
            }
        }
        s.push_str("</filterColumn>");
    }
    s.push_str("</autoFilter>");
    s
}

fn table_xml(t: &Table, cols: &[String], name: &str, id: u32, out: &mut Out) -> String {
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
    if let Some(ty) = &t.table_type {
        let _ = write!(s, " tableType=\"{}\"", esc_attr(ty));
    }
    if let Some(c) = t.connection_id {
        let _ = write!(s, " connectionId=\"{c}\"");
    }
    s.push('>');
    if t.header_row && t.filter_button {
        let end_row = if t.totals_row { t.range.end.row.saturating_sub(1).max(t.range.start.row) } else { t.range.end.row };
        let r = RangeRef::new(t.range.start, CellRef::new(end_row, t.range.end.col));
        s.push_str(&autofilter_xml(r, &t.filter, out));
    }
    let _ = write!(s, "<tableColumns count=\"{}\">", cols.len());
    for (i, name) in cols.iter().enumerate() {
        let col = t.columns.get(i);
        let _ = write!(s, "<tableColumn id=\"{}\"", i + 1);
        if let Some(u) = col.and_then(|c| c.unique_name.as_ref()) {
            let _ = write!(s, " uniqueName=\"{}\"", esc_attr(u));
        }
        let _ = write!(s, " name=\"{}\"", esc_attr(&encode_xstring(name)));
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
        let xml = col.and_then(|c| c.xml.as_ref());
        if calc.is_none() && custom.is_none() && xml.is_none() {
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
        if let Some(x) = xml {
            let _ = write!(s, "<xmlColumnPr mapId=\"{}\" xpath=\"{}\"", x.map_id, esc_attr(&x.xpath));
            if x.denormalized {
                s.push_str(" denormalized=\"1\"");
            }
            let _ = write!(s, " xmlDataType=\"{}\"/>", esc_attr(&x.data_type));
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

/// Ids of the threaded comments of sheet `si`: the thread's id, then one per reply. GUID-shaped
/// and deterministic; they only need to be unique within the file.
fn thread_ids(sheet: &Sheet, si: usize) -> BTreeMap<CellRef, Vec<String>> {
    let mut seq: u64 = 0;
    let mut ids = BTreeMap::new();
    for (cell, c) in sheet.comments.iter().filter(|(_, c)| c.threaded) {
        let v: Vec<String> = (0..=c.replies.len())
            .map(|_| {
                seq += 1;
                format!("{{{:08X}-0000-4000-8001-{:012X}}}", si as u64 & 0xFFFF_FFFF, seq & 0xFFFF_FFFF_FFFF)
            })
            .collect();
        ids.insert(*cell, v);
    }
    ids
}

fn comments_xml(sheet: &Sheet, ids: &BTreeMap<CellRef, Vec<String>>) -> String {
    // A threaded comment's legacy note is authored by `tc={thread id}`.
    let author_of = |cell: &CellRef, c: &gridcraft_model::Comment| match ids.get(cell).and_then(|v| v.first()) {
        Some(id) => format!("tc={id}"),
        None => c.author.clone(),
    };
    let mut authors: Vec<String> = vec![];
    for (cell, c) in &sheet.comments {
        let a = author_of(cell, c);
        if !authors.contains(&a) {
            authors.push(a);
        }
    }
    let mut s = if ids.is_empty() {
        format!("{XML_DECL}<comments xmlns=\"{NS_MAIN}\"><authors>")
    } else {
        format!(
            "{XML_DECL}<comments xmlns=\"{NS_MAIN}\" xmlns:mc=\"http://schemas.openxmlformats.org/markup-compatibility/2006\" mc:Ignorable=\"xr\" xmlns:xr=\"http://schemas.microsoft.com/office/spreadsheetml/2014/revision\"><authors>"
        )
    };
    for a in &authors {
        let _ = write!(s, "<author>{}</author>", esc(a));
    }
    s.push_str("</authors><commentList>");
    for (cell, c) in &sheet.comments {
        let a = author_of(cell, c);
        let aid = authors.iter().position(|x| *x == a).unwrap_or(0);
        let mut text = c.text.clone();
        // Threaded replies are kept as text lines.
        for (who, reply) in &c.replies {
            let _ = write!(text, "\n{who}: {reply}");
        }
        let link = match ids.get(cell).and_then(|v| v.first()) {
            Some(id) => format!(" shapeId=\"0\" xr:uid=\"{id}\""),
            None => String::new(),
        };
        let _ = write!(s, "<comment ref=\"{}\" authorId=\"{aid}\"{link}><text><r>{}</r></text></comment>", cell.a1(), crate::xml::t_el(&text));
    }
    s.push_str("</commentList></comments>");
    s
}

/// `2024-05-01T09:30:00` with optional fraction and zone: what `dT` (xsd:dateTime) accepts.
/// Anything else is left out rather than written into the file.
fn iso_datetime(d: &str) -> bool {
    let b = d.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|s| s.iter().all(u8::is_ascii_digit));
    d.len() <= 40
        && digits(0..4)
        && b.get(4) == Some(&b'-')
        && digits(5..7)
        && b.get(7) == Some(&b'-')
        && digits(8..10)
        && b.get(10) == Some(&b'T')
        && digits(11..13)
        && b.get(13) == Some(&b':')
        && digits(14..16)
        && b.get(16) == Some(&b':')
        && digits(17..19)
        && b.get(19..).is_some_and(|rest| rest.iter().all(|c| c.is_ascii_digit() || b".:+-Z".contains(c)))
}

/// `xl/threadedComments/threadedCommentN.xml`: each thread, then its replies (`parentId`).
fn threaded_xml(sheet: &Sheet, ids: &BTreeMap<CellRef, Vec<String>>, persons: &mut Vec<String>) -> String {
    let mut person = |name: &str| {
        let i = persons.iter().position(|p| p == name).unwrap_or_else(|| {
            persons.push(name.to_string());
            persons.len() - 1
        });
        crate::write::person_guid(i)
    };
    let mut s = format!("{XML_DECL}<ThreadedComments xmlns=\"{NS_THREADED}\" xmlns:x=\"{NS_MAIN}\">");
    for (cell, id) in ids {
        let Some(c) = sheet.comments.get(cell) else { continue };
        let entries = std::iter::once((&c.author, &c.text)).chain(c.replies.iter().map(|(a, t)| (a, t)));
        for (k, ((author, text), own)) in entries.zip(id).enumerate() {
            let date = match c.dates.get(k).filter(|d| iso_datetime(d)) {
                Some(d) => format!(" dT=\"{}\"", esc_attr(d)),
                None => String::new(),
            };
            let tail = match (k, id.first()) {
                (0, _) if c.resolved => " done=\"1\"".to_string(),
                (0, _) | (_, None) => String::new(),
                (_, Some(parent)) => format!(" parentId=\"{parent}\""),
            };
            let _ = write!(
                s,
                "<threadedComment ref=\"{}\"{date} personId=\"{}\" id=\"{own}\"{tail}><text>{}</text></threadedComment>",
                cell.a1(),
                person(author),
                esc(text)
            );
        }
    }
    s.push_str("</ThreadedComments>");
    s
}

/// A protection password hash as attributes, each name preceded by `prefix` (see
/// [`crate::sheet_read::read_password`]).
pub(crate) fn password_attrs(h: &gridcraft_model::PasswordHash, prefix: &str) -> String {
    let name = |base: &str| crate::sheet_read::password_attr(prefix, base);
    match h {
        gridcraft_model::PasswordHash::Legacy(h) => format!(" {}=\"{h:04X}\"", name("password")),
        gridcraft_model::PasswordHash::Iterated { algorithm, hash, salt, spin_count } => format!(
            " {}=\"{}\" {}=\"{}\" {}=\"{}\" {}=\"{spin_count}\"",
            name("algorithmName"),
            esc_attr(algorithm),
            name("hashValue"),
            esc_attr(hash),
            name("saltValue"),
            esc_attr(salt),
            name("spinCount")
        ),
    }
}

/// Allow Edit Ranges: `<protectedRanges>` (after `sheetProtection`).
fn protected_ranges_xml(sheet: &Sheet) -> String {
    let mut s = String::new();
    for r in sheet.edit_ranges.iter().filter(|r| !r.ranges.is_empty()) {
        let sqref: Vec<String> = r.ranges.iter().map(|x| x.a1()).collect();
        let _ = write!(s, "<protectedRange name=\"{}\" sqref=\"{}\"", esc_attr(&r.title), sqref.join(" "));
        if let Some(h) = &r.password {
            s.push_str(&password_attrs(h, ""));
        }
        if let Some(sd) = &r.security_descriptor {
            let _ = write!(s, " securityDescriptor=\"{}\"", esc_attr(sd));
        }
        s.push_str("/>");
    }
    if s.is_empty() { s } else { format!("<protectedRanges>{s}</protectedRanges>") }
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
