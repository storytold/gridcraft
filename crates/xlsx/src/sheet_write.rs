//! Writing worksheet parts and the parts they own (tables, comments, drawings, charts).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write as _;

use gridcraft_core::{CellError, CellRef, MAX_COLS, MAX_ROWS, RangeRef, Value};
use gridcraft_model::{
    Cell, CfOperator, CfRule, CfValueKind, Color, CondFormat, ErrorStyle, FilterCriterion, LineInfo, NoteBox, NoteCorner, Orientation, Sheet,
    SparklineKind, Style, Table, TotalsFn, Validation, ValidationKind, Workbook, default_icon_thresholds, icon_count,
};

use crate::drawing::{Obj, image_content_type, image_ext};
use crate::fmla::text_to_file;
use crate::styles::{color_attrs, dxf_id};
use crate::tables::{paper_id, px_to_col_width, px_to_pt};
use crate::write::{
    CT_CHART, CT_COMMENTS, CT_DRAWING, CT_TABLE, CT_THREADED_COMMENTS, NS_MAIN, NS_REL, NS_THREADED, Out, REL_THREADED_COMMENT, Rels, XML_DECL,
};
use crate::xml::{encode_xstring, esc, esc_attr, num, push_a1, push_u64};

mod parts;
mod rules;

pub(crate) use parts::password_attrs;
use parts::{autofilter_xml, comments_xml, protected_ranges_xml, table_columns, table_name, table_xml, thread_ids, threaded_xml, vml_xml};
use rules::{cf_rule_xml, solid_bar_id, validation_xml, x14_cfvo};

fn sqref(ranges: &[RangeRef]) -> String {
    ranges.iter().map(|r| r.a1()).collect::<Vec<_>>().join(" ")
}

pub fn write_sheet(wb: &Workbook, si: usize, selected: bool, out: &mut Out) -> (String, Rels) {
    let mut rels = Rels::default();
    let Some(sheet) = wb.sheet(si) else { return (String::new(), rels) };
    let nstyles = wb.styles.len() as u32;
    let sid = |s: gridcraft_model::StyleId| if s.0 < nstyles { s.0 } else { 0 };

    // A table's dropdown filter is kept in `sheet.autofilter`, not in `Table::filter` (the
    // engine's only live filter state, which the reader also fills). Where the two coincide,
    // fold the sheet filter's criteria into the table so they reach the file: the stored
    // `colId` offsets are relative to the range's first column, and both ranges start at the
    // same column, so they carry over verbatim. A filter that only partially covers a table
    // has nowhere valid to go, so it keeps being suppressed below.
    let folded: Vec<Table> = match &sheet.autofilter {
        Some(af) if !af.criteria.is_empty() => sheet
            .tables
            .iter()
            .cloned()
            .map(|mut t| {
                if t.header_row
                    && t.filter_button
                    && t.filter.is_empty()
                    && t.range.contains_range(&af.range)
                    && t.range.start.col == af.range.start.col
                {
                    t.filter = af.criteria.clone();
                }
                t
            })
            .collect(),
        _ => sheet.tables.clone(),
    };
    // Table header cells must hold the column names.
    let mut overrides: BTreeMap<CellRef, String> = BTreeMap::new();
    let tables: Vec<(&Table, Vec<String>)> = folded
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
    let summary = !sheet.summary_below || !sheet.summary_right;
    if sheet.tab_color.is_some() || fit || summary {
        s.push_str("<sheetPr>");
        if let Some(c) = &sheet.tab_color {
            let _ = write!(s, "<tabColor {}/>", color_attrs(c));
        }
        if summary {
            s.push_str("<outlinePr");
            for (key, on) in [("summaryBelow", sheet.summary_below), ("summaryRight", sheet.summary_right)] {
                if !on {
                    let _ = write!(s, " {key}=\"0\"");
                }
            }
            s.push_str("/>");
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

    write_sheet_views(&mut s, sheet, selected);

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

    write_sheet_data(&mut s, SheetCellData { workbook: wb, index: si, sheet, overrides: &overrides, extra: &extra, dynamic: &dynamic }, &sid, out);

    let prio = write_sheet_rules(&mut s, sheet, &mut rels, out);
    let kept = write_sheet_printing(&mut s, sheet, &mut rels, out);

    write_sheet_drawing(&mut s, wb, si, sheet, &mut rels, out);
    write_sheet_notes(&mut s, sheet, si, &kept, &mut rels, out);
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
    write_sheet_extensions(&mut s, sheet, &prio);
    s.push_str("</worksheet>");
    (s, rels)
}

struct CellWriteInput<'a> {
    reference: CellRef,
    cell: Option<&'a Cell>,
    picture: Option<&'a std::sync::Arc<gridcraft_model::CellPicture>>,
    header_override: Option<&'a String>,
    spill_value: Option<&'a Value>,
    dynamic_range: Option<&'a RangeRef>,
}

fn write_sheet_extensions(s: &mut String, sheet: &Sheet, prio: &[u32]) {
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
}

fn write_sheet_notes(s: &mut String, sheet: &Sheet, si: usize, kept: &crate::passthrough::SheetKept<'_>, rels: &mut Rels, out: &mut Out) {
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
        let vml = kept.notes_vml(vml_xml(sheet, out.kept.notes_block(n)), &vml_name, out);
        out.part(&vml_name, None, vml.into_bytes());
        rels.add("comments", &format!("../comments{n}.xml"));
        let rid = rels.add("vmlDrawing", &format!("../drawings/vmlDrawing{n}.vml"));
        let _ = write!(s, "<legacyDrawing r:id=\"{rid}\"/>");
    } else if let Some(rid) = kept.lone_vml(rels, out) {
        let _ = write!(s, "<legacyDrawing r:id=\"{rid}\"/>");
    }
    s.push_str(&kept.elements());
}

fn write_sheet_drawing(s: &mut String, wb: &Workbook, si: usize, sheet: &Sheet, rels: &mut Rels, out: &mut Out) {
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
            let ext = image_ext(&img.mime, &img.data);
            out.default_type(ext, image_content_type(ext));
            let file = out.image_part(ext, &img.data);
            let rid = drels.add("image", &format!("../media/{file}"));
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
                out.part(&name, Some(CT_CHART), crate::chart::write_chart(wb, si, ch, &out.book_name).into_bytes());
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
}

fn write_sheet_printing<'a>(s: &mut String, sheet: &'a Sheet, rels: &mut Rels, out: &mut Out) -> crate::passthrough::SheetKept<'a> {
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
    let kept = crate::passthrough::SheetKept::write(sheet, rels, out);
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

    kept
}

fn write_sheet_rules(s: &mut String, sheet: &Sheet, rels: &mut Rels, out: &mut Out) -> Vec<u32> {
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
    prio
}

struct SheetCellData<'a> {
    workbook: &'a Workbook,
    index: usize,
    sheet: &'a Sheet,
    overrides: &'a BTreeMap<CellRef, String>,
    extra: &'a BTreeMap<CellRef, Value>,
    dynamic: &'a BTreeMap<CellRef, RangeRef>,
}

fn write_sheet_data(s: &mut String, data: SheetCellData<'_>, sid: &dyn Fn(gridcraft_model::StyleId) -> u32, out: &mut Out) {
    let SheetCellData { workbook, index, sheet, overrides, extra, dynamic } = data;
    // Cell storage is already row-major; sort only when extra cells need merging.
    let mut keys: Vec<(CellRef, Option<&Cell>)> = Vec::with_capacity(sheet.cells.len());
    keys.extend(sheet.cells.iter().map(|(c, cell)| (c, Some(cell))));
    if !sheet.cell_pictures.is_empty() || !extra.is_empty() || !overrides.is_empty() || !keys.is_sorted_by_key(|(c, _)| *c) {
        let mut all: BTreeSet<CellRef> = keys.iter().map(|(c, _)| *c).collect();
        all.extend(sheet.cell_pictures.keys().copied());
        all.extend(extra.keys().copied());
        all.extend(overrides.keys().copied());
        keys = all.into_iter().map(|c| (c, sheet.cells.get(c))).collect();
    }
    s.reserve(keys.len().saturating_mul(32));
    s.push_str("<sheetData>");
    let mut row_infos = sheet.rows.iter().peekable();
    let mut next = 0;
    loop {
        let cell_row = keys.get(next).map(|(cell, _)| cell.row);
        let info_row = row_infos.peek().map(|(row, _)| **row);
        let row = match (cell_row, info_row) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        let info = match row_infos.next_if(|(entry_row, _)| **entry_row == row) {
            Some((_, info)) => *info,
            None => LineInfo::default(),
        };
        let first = next;
        while keys.get(next).is_some_and(|(cell, _)| cell.row == row) {
            next += 1;
        }
        let cells = keys.get(first..next).unwrap_or_default();
        if cells.is_empty() && info == LineInfo::default() {
            continue;
        }
        s.push_str("<row r=\"");
        push_u64(s, u64::from(row) + 1);
        s.push('"');
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
        for &(cell_ref, cell) in cells {
            let ov = overrides.get(&cell_ref);
            let ex = extra.get(&cell_ref);
            let array = dynamic.get(&cell_ref).copied().or_else(|| {
                let f = cell?.formula.as_ref()?;
                (f.array.is_none() && crate::array_formula::needs_array(workbook, index, cell_ref, f)).then_some(RangeRef::cell(cell_ref))
            });
            write_cell(
                s,
                CellWriteInput {
                    reference: cell_ref,
                    cell,
                    picture: sheet.cell_pictures.get(&cell_ref),
                    header_override: ov,
                    spill_value: ex,
                    dynamic_range: array.as_ref(),
                },
                sid,
                out,
            );
        }
        s.push_str("</row>");
    }
    s.push_str("</sheetData>");
}

fn write_sheet_views(s: &mut String, sheet: &Sheet, selected: bool) {
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
    if !sheet.show_outline_symbols {
        s.push_str(" showOutlineSymbols=\"0\"");
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
    s.push_str("</sheetView></sheetViews>");
}

fn write_cell(s: &mut String, input: CellWriteInput<'_>, sid: &dyn Fn(gridcraft_model::StyleId) -> u32, out: &mut Out) {
    let CellWriteInput { reference: c, cell, picture, header_override: ov, spill_value: extra, dynamic_range: dynamic } = input;
    let style = cell.map(|x| sid(x.style)).unwrap_or(0);
    s.push_str("<c r=\"");
    push_a1(s, c);
    s.push('"');
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
    // A formula read with dynamic-array metadata keeps it, even where the shape analysis would
    // not need it: Excel decided the formula is evaluated as an array.
    let own = formula.filter(|f| f.dynamic && f.array.is_none()).map(|_| RangeRef::cell(c));
    let dynamic = dynamic.or(own.as_ref());
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

const X14_NS: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";
const XM_NS: &str = "http://schemas.microsoft.com/office/excel/2006/main";

/// The box a note made here gets: two columns right of its cell, four rows tall, 108 x 59.25 pt.
/// A box read from a file that equals it is kept as `None`, so a note's model survives a save.
pub(crate) const DEFAULT_NOTE_BOX: NoteBox = NoteBox {
    from: NoteCorner { col: 1, col_px: 15, row: 0, row_px: 2 },
    to: NoteCorner { col: 3, col_px: 15, row: 4, row_px: 16 },
    size_pt: Some(DEFAULT_NOTE_SIZE_PT),
};
const DEFAULT_NOTE_SIZE_PT: (f64, f64) = (108.0, 59.25);
