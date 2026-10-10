//! Formatting: Home › Font, Alignment, Number, Styles, Cells › Format.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // Font
        cmd!("home.bold", "Bold", ["Home", "Font"], Some("Cmd+B"), "{range?, on?: bool}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.font.bold,
            |st, v| st.font.bold = v
        )),
        cmd!("home.italic", "Italic", ["Home", "Font"], Some("Cmd+I"), "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.font.italic,
            |st, v| st.font.italic = v
        )),
        cmd!(
            "home.underline",
            "Underline",
            ["Home", "Font"],
            Some("Cmd+U"),
            "{range?, on?, style?: single|double|singleAccounting|doubleAccounting}",
            has_doc,
            underline
        ),
        cmd!("home.doubleUnderline", "Double Underline", ["Home", "Font"], None, "{range?}", has_doc, |s, p| underline(
            s,
            &with(p, "style", json!("double"))
        )),
        cmd!("home.strikethrough", "Strikethrough", ["Home", "Font"], Some("Cmd+Shift+X"), "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.font.strike,
            |st, v| st.font.strike = v
        )),
        cmd!("home.superscript", "Superscript", ["Home", "Font"], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.font.vert == VertAlign::Superscript,
            |st, v| st.font.vert = if v { VertAlign::Superscript } else { VertAlign::Baseline }
        )),
        cmd!("home.subscript", "Subscript", ["Home", "Font"], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.font.vert == VertAlign::Subscript,
            |st, v| st.font.vert = if v { VertAlign::Subscript } else { VertAlign::Baseline }
        )),
        cmd!("home.fontName", "Font", ["Home", "Font"], None, "{range?, name: \"Calibri\"}", has_doc, font_name),
        cmd!("home.fontSize", "Font Size", ["Home", "Font"], None, "{range?, size: 11}", has_doc, font_size),
        cmd!("home.increaseFontSize", "Increase Font Size", ["Home", "Font"], Some("Cmd+Shift+>"), "{range?}", has_doc, |s, p| step_font(s, p, true)),
        cmd!("home.decreaseFontSize", "Decrease Font Size", ["Home", "Font"], Some("Cmd+Shift+<"), "{range?}", has_doc, |s, p| step_font(
            s, p, false
        )),
        cmd!(
            "home.fontColor",
            "Font Color",
            ["Home", "Font"],
            None,
            "{range?, color: \"#C00000\" | \"auto\" | {theme: 4, tint: -0.25}}",
            has_doc,
            font_color
        ),
        cmd!("home.fillColor", "Fill Color", ["Home", "Font"], None, "{range?, color: \"#FFFF00\" | \"none\" | {theme, tint}}", has_doc, fill_color),
        cmd!(
            "home.borders",
            "Borders",
            ["Home", "Font"],
            None,
            "{range?, preset: bottom|top|left|right|none|all|outside|thickOutside|thickBottom|doubleBottom|topBottom|topThickBottom|topDoubleBottom|insideHorizontal|insideVertical|inside|diagonalDown|diagonalUp, style?: thin|medium|thick|dashed|dotted|double|hair…, color?}",
            has_doc,
            borders
        ),
        cmd!(
            "home.formatCells",
            "Format Cells…",
            ["Home", "Cells", "Format"],
            Some("Cmd+1"),
            "{range?, style: {font?, fill?, borders?, align?, numFmt?, protection?}} (partial Style JSON merged in)",
            has_doc,
            format_cells
        ),
        // Alignment
        cmd!("home.alignLeft", "Align Left", ["Home", "Alignment"], Some("Cmd+L"), "{range?}", has_doc, |s, p| halign(s, p, HAlign::Left)),
        cmd!("home.alignCenter", "Center", ["Home", "Alignment"], Some("Cmd+E"), "{range?}", has_doc, |s, p| halign(s, p, HAlign::Center)),
        cmd!("home.alignRight", "Align Right", ["Home", "Alignment"], Some("Cmd+R"), "{range?}", has_doc, |s, p| halign(s, p, HAlign::Right)),
        cmd!("home.justify", "Justify", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| halign(s, p, HAlign::Justify)),
        cmd!("home.alignTop", "Top Align", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| valign(s, p, VAlign::Top)),
        cmd!("home.alignMiddle", "Middle Align", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| valign(s, p, VAlign::Center)),
        cmd!("home.alignBottom", "Bottom Align", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| valign(s, p, VAlign::Bottom)),
        cmd!("home.wrapText", "Wrap Text", ["Home", "Alignment"], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.align.wrap,
            |st, v| st.align.wrap = v
        )),
        cmd!("home.shrinkToFit", "Shrink to Fit", [], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.align.shrink,
            |st, v| st.align.shrink = v
        )),
        cmd!("home.increaseIndent", "Increase Indent", ["Home", "Alignment"], Some("Ctrl+Alt+Tab"), "{range?}", has_doc, |s, p| indent(s, p, 1)),
        cmd!("home.decreaseIndent", "Decrease Indent", ["Home", "Alignment"], Some("Ctrl+Alt+Shift+Tab"), "{range?}", has_doc, |s, p| indent(
            s, p, -1
        )),
        cmd!(
            "home.orientation",
            "Orientation",
            ["Home", "Alignment"],
            None,
            "{range?, angle: -90..90 | \"vertical\" | \"counterclockwise\" | \"clockwise\" | \"up\" | \"down\" | 0}",
            has_doc,
            orientation
        ),
        cmd!("home.mergeCenter", "Merge & Center", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "center")),
        cmd!("home.mergeAcross", "Merge Across", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "across")),
        cmd!("home.mergeCells", "Merge Cells", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "cells")),
        cmd!("home.unmergeCells", "Unmerge Cells", ["Home", "Alignment"], None, "{range?}", has_doc, |s, p| merge(s, p, "unmerge")),
        // Number
        cmd!(
            "home.numberFormat",
            "Number Format",
            ["Home", "Number"],
            None,
            "{range?, format: \"General\"|\"Number\"|\"Currency\"|\"Accounting\"|\"Short Date\"|\"Long Date\"|\"Time\"|\"Percentage\"|\"Fraction\"|\"Scientific\"|\"Text\" | code: \"0.00\"}",
            has_doc,
            number_format
        ),
        cmd!("home.accounting", "Accounting Number Format", ["Home", "Number"], None, "{range?, symbol?: \"$\"}", has_doc, |s, p| set_fmt(
            s, p, ACCOUNTING
        )),
        cmd!("home.percent", "Percent Style", ["Home", "Number"], Some("Ctrl+Shift+%"), "{range?}", has_doc, |s, p| set_fmt(s, p, "0%")),
        cmd!("home.comma", "Comma Style", ["Home", "Number"], None, "{range?}", has_doc, |s, p| set_fmt(s, p, COMMA)),
        cmd!("home.increaseDecimal", "Increase Decimal", ["Home", "Number"], None, "{range?}", has_doc, |s, p| decimals(s, p, 1)),
        cmd!("home.decreaseDecimal", "Decrease Decimal", ["Home", "Number"], None, "{range?}", has_doc, |s, p| decimals(s, p, -1)),
        // Styles
        cmd!(
            "home.cellStyle",
            "Cell Styles",
            ["Home", "Styles"],
            None,
            "{range?, name: \"Good\"|\"Bad\"|\"Neutral\"|\"Heading 1\"|\"Title\"|\"Total\"|\"Accent1\"…|\"Normal\"}",
            has_doc,
            cell_style
        ),
        // Cells › Format
        cmd!(
            "home.rowHeight",
            "Row Height…",
            ["Home", "Cells", "Format"],
            None,
            "{rows?: \"2:5\", height: 20 (points on screen)}",
            has_doc,
            row_height
        ),
        cmd!(
            "home.columnWidth",
            "Column Width…",
            ["Home", "Cells", "Format"],
            None,
            "{cols?: \"B:D\", width: 64 (points) | chars: 8.43}",
            has_doc,
            col_width
        ),
        cmd!("home.autofitRowHeight", "AutoFit Row Height", ["Home", "Cells", "Format"], None, "{rows?}", has_doc, autofit_rows),
        cmd!("home.autofitColumnWidth", "AutoFit Column Width", ["Home", "Cells", "Format"], None, "{cols?}", has_doc, autofit_cols),
        cmd!("home.defaultWidth", "Default Width…", ["Home", "Cells", "Format"], None, "{width: 64}", has_doc, default_width),
        cmd!("home.hideRows", "Hide Rows", ["Home", "Cells", "Format", "Hide & Unhide"], Some("Cmd+9"), "{rows?}", has_doc, |s, p| hide(
            s, p, true, true
        )),
        cmd!("home.hideColumns", "Hide Columns", ["Home", "Cells", "Format", "Hide & Unhide"], Some("Cmd+0"), "{cols?}", has_doc, |s, p| hide(
            s, p, false, true
        )),
        cmd!("home.unhideRows", "Unhide Rows", ["Home", "Cells", "Format", "Hide & Unhide"], Some("Cmd+Shift+9"), "{rows?}", has_doc, |s, p| hide(
            s, p, true, false
        )),
        cmd!(
            "home.unhideColumns",
            "Unhide Columns",
            ["Home", "Cells", "Format", "Hide & Unhide"],
            Some("Cmd+Shift+0"),
            "{cols?}",
            has_doc,
            |s, p| hide(s, p, false, false)
        ),
        cmd!("home.lockCell", "Lock Cell", ["Home", "Cells", "Format"], None, "{range?, on?}", has_doc, |s, p| toggle(
            s,
            p,
            |st| st.protection.locked,
            |st, v| st.protection.locked = v
        )),
    ]
}

const ACCOUNTING: &str = "_(\"$\"* #,##0.00_);_(\"$\"* \\(#,##0.00\\);_(\"$\"* \"-\"??_);_(@_)";
const COMMA: &str = "_(* #,##0.00_);_(* \\(#,##0.00\\);_(* \"-\"??_);_(@_)";

fn with(p: &Json, key: &str, v: Json) -> Json {
    let mut m = p.as_object().cloned().unwrap_or_default();
    m.insert(key.into(), v);
    Json::Object(m)
}

/// Applies `f` to the style of every targeted cell (whole rows/columns set the line format too).
pub(crate) fn apply_style(s: &mut Session, p: &Json, f: impl Fn(&mut Style)) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    if s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.protection.as_ref().is_some_and(|pr| !pr.format_cells)) {
        return Err(EngineError::Other("The cell or chart you're trying to change is on a protected sheet.".into()));
    }
    edit(s, |cx| {
        let mut cache: std::collections::HashMap<StyleId, StyleId> = std::collections::HashMap::new();
        for r in &ranges {
            let full_rows = r.is_full_rows();
            let full_cols = r.is_full_cols();
            if full_rows || full_cols {
                // Line formats.
                let lines: Vec<u32> = if full_rows && !full_cols { (r.start.row..=r.end.row).collect() } else { (r.start.col..=r.end.col).collect() };
                for line in lines {
                    let cur = {
                        let sh = cx.wb.sheet(sheet);
                        let info = if full_rows && !full_cols { sh.and_then(|s| s.rows.get(&line)) } else { sh.and_then(|s| s.cols.get(&line)) };
                        info.and_then(|i| i.style).unwrap_or_default()
                    };
                    let new = *cache.entry(cur).or_insert_with(|| cx.wb.styles.derive(cur, &f));
                    let sh = cx.sheet_mut(sheet)?;
                    let map = if full_rows && !full_cols { &mut sh.rows } else { &mut sh.cols };
                    map.entry(line).or_default().style = Some(new);
                }
                // Existing cells in those lines.
                let cells: Vec<(CellRef, StyleId)> =
                    cx.wb.sheet(sheet).map(|sh| sh.cells.iter_range(*r).map(|(c, x)| (c, x.style)).collect()).unwrap_or_default();
                for (c, st) in cells {
                    let new = *cache.entry(st).or_insert_with(|| cx.wb.styles.derive(st, &f));
                    cx.sheet_mut(sheet)?.set_style(c, new);
                }
                continue;
            }
            if r.count() > 4_000_000 {
                return Err(bad("format", "selection too large to format cell by cell"));
            }
            if r.height() <= 100_000 {
                cx.fit_rows.extend((r.start.row..=r.end.row).map(|row| (sheet, row)));
            }
            for c in r.iter() {
                let st = cx.wb.sheet(sheet).map(|sh| sh.style_id(c)).unwrap_or_default();
                let new = *cache.entry(st).or_insert_with(|| cx.wb.styles.derive(st, &f));
                if new != st {
                    cx.sheet_mut(sheet)?.set_style(c, new);
                }
            }
        }
        Ok(Json::Null)
    })
}

/// The style of the active cell (what toggles read).
pub(crate) fn active_style(s: &Session) -> Result<Style> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    Ok(d.wb.styles.get(sh.style_id(d.selection.active)).clone())
}

fn toggle(s: &mut Session, p: &Json, get: fn(&Style) -> bool, set: fn(&mut Style, bool)) -> Result<Json> {
    let on = match bool_param(p, "on") {
        Some(v) => v,
        None => !get(&active_style(s)?),
    };
    apply_style(s, p, |st| set(st, on))?;
    Ok(json!({"on": on}))
}

fn underline(s: &mut Session, p: &Json) -> Result<Json> {
    let style = match str_param(p, "style").unwrap_or("single") {
        "double" => Underline::Double,
        "singleAccounting" => Underline::SingleAccounting,
        "doubleAccounting" => Underline::DoubleAccounting,
        "none" => Underline::None,
        _ => Underline::Single,
    };
    let cur = active_style(s)?.font.underline;
    let on = bool_param(p, "on").unwrap_or(cur != style);
    let u = if on { style } else { Underline::None };
    apply_style(s, p, |st| st.font.underline = u)?;
    Ok(json!({"on": on}))
}

fn font_name(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(name) = str_param(p, "name").map(str::to_string) else { return Err(bad("home.fontName", "missing `name`")) };
    apply_style(s, p, |st| st.font.name = name.clone())
}

fn font_size(s: &mut Session, p: &Json) -> Result<Json> {
    let size = f64_param(p, "size").ok_or_else(|| bad("home.fontSize", "missing `size`"))?;
    if !(1.0..=409.0).contains(&size) {
        return Err(EngineError::Other("The number must be between 1 and 409.".into()));
    }
    let size = ((size * 2.0).round() / 2.0) as f32;
    apply_style(s, p, |st| st.font.size = size)
}

const SIZES: [f32; 16] = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0, 48.0, 72.0, 96.0, 409.0];

fn step_font(s: &mut Session, p: &Json, up: bool) -> Result<Json> {
    let cur = active_style(s)?.font.size;
    let next =
        if up { SIZES.iter().copied().find(|x| *x > cur).unwrap_or(409.0) } else { SIZES.iter().rev().copied().find(|x| *x < cur).unwrap_or(1.0) };
    apply_style(s, p, |st| st.font.size = next)?;
    Ok(json!({"size": next}))
}

pub(crate) fn color_param(v: Option<&Json>) -> Option<Color> {
    match v? {
        Json::String(s) if s.eq_ignore_ascii_case("auto") || s.eq_ignore_ascii_case("automatic") || s.eq_ignore_ascii_case("none") => {
            Some(Color::Auto)
        }
        Json::String(s) => Color::from_hex(s),
        Json::Object(o) => {
            let theme = o.get("theme")?.as_u64()? as u8;
            let tint = o.get("tint").and_then(Json::as_f64).unwrap_or(0.0);
            Some(Color::Theme(theme.min(11), (tint.clamp(-1.0, 1.0) * 1000.0) as i16))
        }
        _ => None,
    }
}

fn font_color(s: &mut Session, p: &Json) -> Result<Json> {
    let c = color_param(p.get("color")).ok_or_else(|| bad("home.fontColor", "missing or invalid `color`"))?;
    apply_style(s, p, |st| st.font.color = c)
}

fn fill_color(s: &mut Session, p: &Json) -> Result<Json> {
    let c = color_param(p.get("color")).ok_or_else(|| bad("home.fillColor", "missing or invalid `color`"))?;
    apply_style(s, p, |st| st.fill = if c == Color::Auto { Fill::default() } else { Fill::solid(c) })
}

fn border_style(name: &str) -> BorderStyle {
    match name {
        "medium" => BorderStyle::Medium,
        "thick" => BorderStyle::Thick,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "double" => BorderStyle::Double,
        "hair" => BorderStyle::Hair,
        "mediumDashed" => BorderStyle::MediumDashed,
        "dashDot" => BorderStyle::DashDot,
        "mediumDashDot" => BorderStyle::MediumDashDot,
        "dashDotDot" => BorderStyle::DashDotDot,
        "slantDashDot" => BorderStyle::SlantDashDot,
        "none" => BorderStyle::None,
        _ => BorderStyle::Thin,
    }
}

fn borders(s: &mut Session, p: &Json) -> Result<Json> {
    let preset = str_param(p, "preset").unwrap_or("bottom").to_string();
    let color = color_param(p.get("color")).unwrap_or(Color::Auto);
    let base = border_style(str_param(p, "style").unwrap_or("thin"));
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    if s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.protection.as_ref().is_some_and(|pr| !pr.format_cells)) {
        return Err(EngineError::Other("The cell or chart you're trying to change is on a protected sheet.".into()));
    }
    edit(s, |cx| {
        for r in &ranges {
            if r.count() > 2_000_000 {
                return Err(bad("home.borders", "selection too large"));
            }
            for c in r.iter() {
                let (top, bottom, left, right) = (c.row == r.start.row, c.row == r.end.row, c.col == r.start.col, c.col == r.end.col);
                let st = cx.wb.sheet(sheet).map(|sh| sh.style_id(c)).unwrap_or_default();
                let line = |style: BorderStyle| BorderLine { style, color };
                let new = cx.wb.styles.derive(st, |s| {
                    let b = &mut s.borders;
                    match preset.as_str() {
                        "none" => *b = Borders::default(),
                        "all" => {
                            b.top = line(base);
                            b.bottom = line(base);
                            b.left = line(base);
                            b.right = line(base);
                        }
                        "outside" | "thickOutside" => {
                            let st = if preset == "thickOutside" { BorderStyle::Thick } else { base };
                            if top {
                                b.top = line(st);
                            }
                            if bottom {
                                b.bottom = line(st);
                            }
                            if left {
                                b.left = line(st);
                            }
                            if right {
                                b.right = line(st);
                            }
                        }
                        "inside" | "insideHorizontal" | "insideVertical" => {
                            if preset != "insideVertical" {
                                if !top {
                                    b.top = line(base);
                                }
                                if !bottom {
                                    b.bottom = line(base);
                                }
                            }
                            if preset != "insideHorizontal" {
                                if !left {
                                    b.left = line(base);
                                }
                                if !right {
                                    b.right = line(base);
                                }
                            }
                        }
                        "top" if top => b.top = line(base),
                        "bottom" if bottom => b.bottom = line(base),
                        "left" if left => b.left = line(base),
                        "right" if right => b.right = line(base),
                        "thickBottom" if bottom => b.bottom = line(BorderStyle::Thick),
                        "doubleBottom" if bottom => b.bottom = line(BorderStyle::Double),
                        "topBottom" => {
                            if top {
                                b.top = line(base);
                            }
                            if bottom {
                                b.bottom = line(base);
                            }
                        }
                        "topThickBottom" => {
                            if top {
                                b.top = line(base);
                            }
                            if bottom {
                                b.bottom = line(BorderStyle::Thick);
                            }
                        }
                        "topDoubleBottom" => {
                            if top {
                                b.top = line(base);
                            }
                            if bottom {
                                b.bottom = line(BorderStyle::Double);
                            }
                        }
                        "diagonalDown" => b.diag_down = line(base),
                        "diagonalUp" => b.diag_up = line(base),
                        _ => {}
                    }
                });
                cx.sheet_mut(sheet)?.set_style(c, new);
                if preset == "none" {
                    // A shared line can be stored on either cell. Selected cells are handled
                    // by this loop; remove only facing edges outside this range.
                    for (boundary, dr, dc) in [(top, -1, 0), (bottom, 1, 0), (left, 0, -1), (right, 0, 1)] {
                        if !boundary {
                            continue;
                        }
                        let Some(neighbor) = c.offset(dr, dc) else { continue };
                        let st = cx.wb.sheet(sheet).map(|sh| sh.style_id(neighbor)).unwrap_or_default();
                        let new = cx.wb.styles.derive(st, |s| {
                            let edge = match (dr, dc) {
                                (-1, _) => &mut s.borders.bottom,
                                (1, _) => &mut s.borders.top,
                                (_, -1) => &mut s.borders.right,
                                _ => &mut s.borders.left,
                            };
                            *edge = BorderLine::default();
                        });
                        if new != st {
                            cx.sheet_mut(sheet)?.set_style(neighbor, new);
                        }
                    }
                }
            }
        }
        Ok(Json::Null)
    })
}

fn format_cells(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(patch) = p.get("style").cloned() else {
        // Menu invocation without params: the UI opens the dialog.
        s.ui_requests.push(crate::UiRequest::Dialog("formatCells".into(), json!({})));
        return ok();
    };
    // Validate by merging into the active style once.
    let base = serde_json::to_value(active_style(s)?).map_err(|e| EngineError::Other(e.to_string()))?;
    let merged = merge_json(base, &patch);
    let _: Style = serde_json::from_value(merged).map_err(|e| bad("home.formatCells", e.to_string()))?;
    apply_style(s, p, |st| {
        if let Ok(cur) = serde_json::to_value(&*st)
            && let Ok(new) = serde_json::from_value::<Style>(merge_json(cur, &patch))
        {
            *st = new;
        }
    })
}

/// Deep-merges `patch` into `base` (objects merge, everything else replaces).
pub(crate) fn merge_json(mut base: Json, patch: &Json) -> Json {
    match (&mut base, patch) {
        (Json::Object(b), Json::Object(p)) => {
            for (k, v) in p {
                let cur = b.remove(k).unwrap_or(Json::Null);
                b.insert(k.clone(), merge_json(cur, v));
            }
            base
        }
        (_, p) => p.clone(),
    }
}

fn halign(s: &mut Session, p: &Json, a: HAlign) -> Result<Json> {
    // Clicking the active alignment again returns to General.
    let cur = active_style(s)?.align.h;
    let a = if cur == a && p.get("on").is_none() { HAlign::General } else { a };
    apply_style(s, p, |st| st.align.h = a)
}

fn valign(s: &mut Session, p: &Json, a: VAlign) -> Result<Json> {
    apply_style(s, p, |st| st.align.v = a)
}

fn indent(s: &mut Session, p: &Json, d: i32) -> Result<Json> {
    apply_style(s, p, |st| {
        st.align.indent = (st.align.indent as i32 + d).clamp(0, 250) as u8;
        if st.align.indent > 0 && matches!(st.align.h, HAlign::General | HAlign::Center | HAlign::Fill | HAlign::Justify | HAlign::CenterAcross) {
            st.align.h = HAlign::Left;
        }
    })
}

fn orientation(s: &mut Session, p: &Json) -> Result<Json> {
    let angle: i16 = match p.get("angle") {
        Some(Json::Number(n)) => n.as_f64().unwrap_or(0.0).clamp(-90.0, 90.0) as i16,
        Some(Json::String(t)) => match t.as_str() {
            "vertical" => 255,
            "counterclockwise" => 45,
            "clockwise" => -45,
            "up" => 90,
            "down" => -90,
            _ => 0,
        },
        _ => 0,
    };
    apply_style(s, p, |st| st.align.rotation = angle)
}

fn merge(s: &mut Session, p: &Json, how: &str) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let how =
        if how == "center" && s.doc()?.wb.sheet(sheet).is_some_and(|sh| ranges.iter().all(|r| sh.merges.contains(r))) { "unmerge" } else { how };
    edit(s, |cx| {
        let mut touched = Vec::new();
        for r in &ranges {
            let sh = cx.sheet_mut(sheet)?;
            sh.merges.retain(|m| !m.intersects(r));
            match how {
                "unmerge" => {}
                "across" => {
                    for row in r.start.row..=r.end.row {
                        let m = RangeRef::new(CellRef::new(row, r.start.col), CellRef::new(row, r.end.col));
                        if !m.is_single() {
                            sh.merges.push(m);
                        }
                    }
                }
                _ => {
                    if !r.is_single() {
                        sh.merges.push(*r);
                    }
                    // Only the upper-left value survives (Excel warns about this).
                    if r.count() <= 1_000_000 {
                        let others: Vec<CellRef> = sh.cells.iter_range(*r).map(|(c, _)| c).filter(|c| *c != r.start).collect();
                        for c in others {
                            if let Some(mut cell) = sh.cells.get(c).cloned() {
                                cell.value = gridcraft_core::Value::Empty;
                                cell.formula = None;
                                sh.set_cell(c, cell);
                            }
                            touched.push((sheet, c));
                        }
                    }
                }
            }
        }
        cx.changed.extend(touched);
        Ok(())
    })?;
    if how == "center" {
        apply_style(s, p, |st| st.align.h = HAlign::Center)?;
    }
    ok()
}

/// Ribbon format names → codes.
pub fn format_code_for(name: &str) -> &str {
    match name {
        "General" => "General",
        "Number" => "0.00",
        "Currency" => "\"$\"#,##0.00",
        "Accounting" => ACCOUNTING,
        "Short Date" => "m/d/yyyy",
        "Long Date" => "[$-F800]dddd, mmmm dd, yyyy",
        "Time" => "[$-F400]h:mm:ss AM/PM",
        "Percentage" => "0.00%",
        "Fraction" => "# ?/?",
        "Scientific" => "0.00E+00",
        "Text" => "@",
        "Comma" => COMMA,
        other => other,
    }
}

fn number_format(s: &mut Session, p: &Json) -> Result<Json> {
    let code = str_param(p, "code")
        .map(str::to_string)
        .or_else(|| str_param(p, "format").map(|f| format_code_for(f).to_string()))
        .ok_or_else(|| bad("home.numberFormat", "missing `format` or `code`"))?;
    set_fmt(s, p, &code)
}

fn set_fmt(s: &mut Session, p: &Json, code: &str) -> Result<Json> {
    let nf = NumFmt::new(code);
    apply_style(s, p, |st| st.num_fmt = nf.clone())
}

/// Adds or removes one decimal place in the number format (Increase/Decrease Decimal).
pub fn adjust_decimals(code: &str, value: &gridcraft_core::Value, delta: i32) -> String {
    if code == "General" {
        // Start from the displayed number of decimals.
        let text = value.as_f64().map(gridcraft_core::number_to_text).unwrap_or_default();
        let dec = text.split_once('.').map_or(0, |(_, d)| d.len()) as i32;
        let n = (dec + delta).max(0) as usize;
        return if n == 0 { "0".into() } else { format!("0.{}", "0".repeat(n)) };
    }
    let sections: Vec<String> = code
        .split(';')
        .map(|sec| {
            // Find the last digit placeholder run before any exponent / percent etc.
            let chars: Vec<char> = sec.chars().collect();
            let mut in_quote = false;
            let mut in_bracket = false;
            let mut last_digit = None;
            let mut point = None;
            for (i, c) in chars.iter().enumerate() {
                if *c == '"' {
                    in_quote = !in_quote;
                }
                if *c == '[' && !in_quote {
                    in_bracket = true;
                }
                if *c == ']' && !in_quote {
                    in_bracket = false;
                    continue;
                }
                if in_quote || in_bracket {
                    continue;
                }
                if matches!(c, '0' | '#' | '?') {
                    last_digit = Some(i);
                }
                if *c == '.' && point.is_none() {
                    point = Some(i);
                }
                if matches!(c, 'E' | 'e') {
                    break;
                }
            }
            let Some(ld) = last_digit else { return sec.to_string() };
            let mut out: Vec<char> = chars.clone();
            if delta > 0 {
                match point {
                    Some(_) => out.insert(ld + 1, '0'),
                    None => {
                        out.insert(ld + 1, '0');
                        out.insert(ld + 1, '.');
                    }
                }
            } else if let Some(pt) = point
                && ld > pt
            {
                out.remove(ld);
                if ld == pt + 1 {
                    out.remove(pt);
                }
            }
            out.into_iter().collect()
        })
        .collect();
    sections.join(";")
}

fn decimals(s: &mut Session, p: &Json, delta: i32) -> Result<Json> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let a = d.selection.active;
    let code = d.wb.styles.get(sh.style_id(a)).num_fmt.as_str().to_string();
    let new = adjust_decimals(&code, &sh.value(a), delta);
    set_fmt(s, p, &new)
}

/// Built-in cell styles (our own palette).
pub fn builtin_cell_style(name: &str, theme: &Theme) -> Option<Style> {
    let _ = theme;
    let mut s = Style::default();
    let accent = |i: u8, tint: i16| Color::Theme(i, tint);
    match name {
        "Normal" => {}
        "Good" => {
            s.fill = Fill::solid(Color::rgb(0xC6, 0xEF, 0xCE));
            s.font.color = Color::rgb(0x00, 0x61, 0x00);
        }
        "Bad" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xC7, 0xCE));
            s.font.color = Color::rgb(0x9C, 0x00, 0x06);
        }
        "Neutral" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xEB, 0x9C));
            s.font.color = Color::rgb(0x9C, 0x57, 0x00);
        }
        "Calculation" => {
            s.fill = Fill::solid(Color::rgb(0xF2, 0xF2, 0xF2));
            s.font.color = Color::rgb(0xFA, 0x7D, 0x00);
            s.font.bold = true;
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0x7F, 0x7F, 0x7F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Check Cell" => {
            s.fill = Fill::solid(Color::rgb(0xA5, 0xA5, 0xA5));
            s.font.color = Color::rgb(0xFF, 0xFF, 0xFF);
            s.font.bold = true;
            let l = BorderLine { style: BorderStyle::Double, color: Color::rgb(0x3F, 0x3F, 0x3F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Explanatory Text" => {
            s.font.italic = true;
            s.font.color = Color::rgb(0x7F, 0x7F, 0x7F);
        }
        "Input" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xCC, 0x99));
            s.font.color = Color::rgb(0x3F, 0x3F, 0x76);
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0x7F, 0x7F, 0x7F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Linked Cell" => {
            s.font.color = Color::rgb(0xFA, 0x7D, 0x00);
            s.borders.bottom = BorderLine { style: BorderStyle::Double, color: Color::rgb(0xFF, 0x80, 0x01) };
        }
        "Note" => {
            s.fill = Fill::solid(Color::rgb(0xFF, 0xFF, 0xCC));
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0xB2, 0xB2, 0xB2) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Output" => {
            s.fill = Fill::solid(Color::rgb(0xF2, 0xF2, 0xF2));
            s.font.color = Color::rgb(0x3F, 0x3F, 0x3F);
            s.font.bold = true;
            let l = BorderLine { style: BorderStyle::Thin, color: Color::rgb(0x3F, 0x3F, 0x3F) };
            s.borders = Borders { left: l, right: l, top: l, bottom: l, ..Default::default() };
        }
        "Warning Text" => s.font.color = Color::rgb(0xFF, 0x00, 0x00),
        "Title" => {
            s.font.size = 18.0;
            s.font.color = Color::Theme(3, 0);
            s.font.name = theme.major_font.clone();
        }
        "Heading 1" => {
            s.font.size = 15.0;
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
            s.borders.bottom = BorderLine { style: BorderStyle::Thick, color: accent(4, 0) };
        }
        "Heading 2" => {
            s.font.size = 13.0;
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
            s.borders.bottom = BorderLine { style: BorderStyle::Thick, color: accent(4, 500) };
        }
        "Heading 3" => {
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
            s.borders.bottom = BorderLine { style: BorderStyle::Medium, color: accent(4, 400) };
        }
        "Heading 4" => {
            s.font.bold = true;
            s.font.color = Color::Theme(3, 0);
        }
        "Total" => {
            s.font.bold = true;
            s.borders.top = BorderLine { style: BorderStyle::Thin, color: accent(4, 0) };
            s.borders.bottom = BorderLine { style: BorderStyle::Double, color: accent(4, 0) };
        }
        "Currency" => s.num_fmt = NumFmt::new(ACCOUNTING),
        "Comma" => s.num_fmt = NumFmt::new(COMMA),
        "Percent" => s.num_fmt = NumFmt::new("0%"),
        "Currency [0]" => s.num_fmt = NumFmt::new("_(\"$\"* #,##0_);_(\"$\"* \\(#,##0\\);_(\"$\"* \"-\"_);_(@_)"),
        "Comma [0]" => s.num_fmt = NumFmt::new("_(* #,##0_);_(* \\(#,##0\\);_(* \"-\"_);_(@_)"),
        n => {
            // AccentN, 20%/40%/60% - AccentN
            let (pct, rest) = match n.split_once("% - ") {
                Some((p, r)) => (p.parse::<i16>().ok(), r),
                None => (None, n),
            };
            let i: u8 = rest.strip_prefix("Accent")?.parse().ok()?;
            if !(1..=6).contains(&i) {
                return None;
            }
            match pct {
                None => {
                    s.fill = Fill::solid(accent(3 + i, 0));
                    s.font.color = Color::Theme(0, 0);
                }
                Some(p) => {
                    s.fill = Fill::solid(accent(3 + i, (1000 - p * 10).clamp(0, 1000)));
                    s.font.color = if p >= 60 { Color::Theme(0, 0) } else { Color::Theme(1, 0) };
                }
            }
        }
    }
    Some(s)
}

pub const CELL_STYLE_NAMES: &[&str] = &[
    "Normal",
    "Bad",
    "Good",
    "Neutral",
    "Calculation",
    "Check Cell",
    "Explanatory Text",
    "Input",
    "Linked Cell",
    "Note",
    "Output",
    "Warning Text",
    "Heading 1",
    "Heading 2",
    "Heading 3",
    "Heading 4",
    "Title",
    "Total",
    "20% - Accent1",
    "20% - Accent2",
    "20% - Accent3",
    "20% - Accent4",
    "20% - Accent5",
    "20% - Accent6",
    "40% - Accent1",
    "40% - Accent2",
    "40% - Accent3",
    "40% - Accent4",
    "40% - Accent5",
    "40% - Accent6",
    "60% - Accent1",
    "60% - Accent2",
    "60% - Accent3",
    "60% - Accent4",
    "60% - Accent5",
    "60% - Accent6",
    "Accent1",
    "Accent2",
    "Accent3",
    "Accent4",
    "Accent5",
    "Accent6",
    "Comma",
    "Comma [0]",
    "Currency",
    "Currency [0]",
    "Percent",
];

fn cell_style(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").unwrap_or("Normal").to_string();
    let d = s.doc()?;
    let style =
        d.wb.cell_styles
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&name))
            .map(|(_, s)| s.clone())
            .or_else(|| builtin_cell_style(&name, &d.wb.theme));
    let Some(style) = style else { return Err(bad("home.cellStyle", format!("unknown cell style `{name}`"))) };
    // Number-format styles only change the number format.
    let only_fmt = matches!(name.as_str(), "Comma" | "Comma [0]" | "Currency" | "Currency [0]" | "Percent");
    apply_style(s, p, move |st| {
        if only_fmt {
            st.num_fmt = style.num_fmt.clone();
        } else {
            let keep_fmt = st.num_fmt.clone();
            *st = style.clone();
            if style.num_fmt.as_str() == "General" {
                st.num_fmt = keep_fmt;
            }
        }
    })
}

// ---------------------------------------------------------------- sizes

fn line_targets(s: &Session, p: &Json, rows: bool) -> Result<Vec<(u32, u32)>> {
    let key = if rows { "rows" } else { "cols" };
    if let Some(t) = str_param(p, key) {
        let r = RangeRef::parse(t).ok_or_else(|| bad(key, format!("not a range: {t}")))?;
        return Ok(vec![if rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) }]);
    }
    Ok(s.doc()?.selection.ranges.iter().map(|r| if rows { (r.start.row, r.end.row) } else { (r.start.col, r.end.col) }).collect())
}

fn row_height(s: &mut Session, p: &Json) -> Result<Json> {
    let h = f64_param(p, "height").ok_or_else(|| bad("home.rowHeight", "missing `height`"))?;
    if !(0.0..=546.0).contains(&h) {
        return Err(EngineError::Other("Row height must be between 0 and 409 points.".into()));
    }
    let lines = line_targets(s, p, true)?;
    edit(s, |cx| {
        let si = cx.sheet_index();
        let sh = cx.sheet_mut(si)?;
        for (a, b) in lines {
            for r in a..=b.min(a.saturating_add(1_048_575)) {
                let e = sh.rows.entry(r).or_default();
                e.size = Some(h as f32);
                e.hidden = h == 0.0;
                e.custom = true;
            }
        }
        Ok(Json::Null)
    })
}

fn col_width(s: &mut Session, p: &Json) -> Result<Json> {
    let w = match (f64_param(p, "width"), f64_param(p, "chars")) {
        (Some(w), _) => w,
        (None, Some(c)) => chars_to_points(c),
        _ => return Err(bad("home.columnWidth", "missing `width`")),
    };
    if !(0.0..=1800.0).contains(&w) {
        return Err(EngineError::Other("Column width must be between 0 and 255 characters.".into()));
    }
    let lines = line_targets(s, p, false)?;
    edit(s, |cx| {
        let si = cx.sheet_index();
        let sh = cx.sheet_mut(si)?;
        for (a, b) in lines {
            for c in a..=b {
                let e = sh.cols.entry(c).or_default();
                e.size = Some(w as f32);
                e.hidden = w == 0.0;
            }
        }
        Ok(Json::Null)
    })
}

/// Excel column width in characters (of the default font's digit width, 7 px) → points.
pub fn chars_to_points(c: f64) -> f64 {
    if c <= 0.0 {
        0.0
    } else if c < 1.0 {
        (c * 12.0).round()
    } else {
        (c * 7.0 + 5.0).round()
    }
}

pub fn points_to_chars(px: f64) -> f64 {
    if px <= 12.0 { (px / 12.0 * 100.0).round() / 100.0 } else { ((px - 5.0) / 7.0 * 100.0 + 0.5).trunc() / 100.0 }
}

/// Approximate text width in points for autofit (no font metrics in the engine: average glyph
/// widths for the default font; the UI refines with real measurement when it autofits).
pub fn approx_text_width(text: &str, size: f32, bold: bool) -> f32 {
    let mut w = 0.0;
    for c in text.chars() {
        w += match c {
            'i' | 'l' | 'j' | '\'' | '.' | ',' | ':' | ';' | '!' | '|' => 0.28,
            'f' | 't' | 'r' | 'I' | ' ' | '(' | ')' | '-' => 0.38,
            'm' | 'w' | 'M' | 'W' => 0.85,
            c if c.is_ascii_uppercase() => 0.65,
            c if c.is_ascii_digit() => 0.56,
            c if c.is_ascii() => 0.52,
            _ => 1.0,
        };
    }
    w * size * if bold { 1.07 } else { 1.0 } * 96.0 / 72.0
}

fn autofit_cols(s: &mut Session, p: &Json) -> Result<Json> {
    let lines = line_targets(s, p, false)?;
    edit(s, |cx| {
        let si = cx.sheet_index();
        let Some(sh) = cx.wb.sheet(si) else { return Ok(Json::Null) };
        let mut widths = Vec::new();
        let used = sh.used_range();
        for (a, b) in lines {
            for c in a..=b.min(a + 16383) {
                let Some(u) = used else { continue };
                let col = RangeRef::new(CellRef::new(u.start.row, c), CellRef::new(u.end.row, c));
                let mut max: f32 = 0.0;
                for (cell, _) in sh.cells.iter_range(col) {
                    if sh.merge_at(cell).is_some_and(|m| !m.is_single()) {
                        continue;
                    }
                    let st = cx.wb.styles.get(sh.style_id(cell));
                    let text = crate::display::cell_text(&cx.wb, sh, cell);
                    for line in text.lines() {
                        max = max.max(approx_text_width(line, st.font.size, st.font.bold) + 6.0 + st.align.indent as f32 * 9.0);
                    }
                }
                if max > 0.0 {
                    widths.push((c, max.clamp(8.0, 1785.0)));
                }
            }
        }
        let sh = cx.sheet_mut(si)?;
        for (c, w) in widths {
            let e = sh.cols.entry(c).or_default();
            e.size = Some(w.ceil());
            e.hidden = false;
        }
        Ok(Json::Null)
    })
}

fn autofit_rows(s: &mut Session, p: &Json) -> Result<Json> {
    let lines = line_targets(s, p, true)?;
    edit(s, |cx| {
        let si = cx.sheet_index();
        for (a, b) in lines {
            let b = b.min(a + 100_000);
            if let Some(sh) = cx.wb.sheet_mut(si) {
                for r in a..=b {
                    if let Some(e) = sh.rows.get_mut(&r) {
                        e.custom = false;
                        e.hidden = false;
                    }
                }
            }
            cx.fit_rows.extend((a..=b).map(|r| (si, r)));
        }
        Ok(Json::Null)
    })
}

fn default_width(s: &mut Session, p: &Json) -> Result<Json> {
    let w = f64_param(p, "width").ok_or_else(|| bad("home.defaultWidth", "missing `width`"))?;
    edit(s, |cx| {
        let si = cx.sheet_index();
        cx.sheet_mut(si)?.default_col_width = w.clamp(0.0, 1800.0) as f32;
        Ok(Json::Null)
    })
}

fn hide(s: &mut Session, p: &Json, rows: bool, on: bool) -> Result<Json> {
    let lines = line_targets(s, p, rows)?;
    edit(s, |cx| {
        let si = cx.sheet_index();
        let sh = cx.sheet_mut(si)?;
        for (a, b) in lines {
            // Unhiding a single selected line also reveals hidden neighbours (Excel needs the
            // lines on both sides selected; we're forgiving).
            let (a, b) = if !on && a == b { (a.saturating_sub(1), b + 1) } else { (a, b) };
            for i in a..=b.min(a.saturating_add(1_048_575)) {
                let map = if rows { &mut sh.rows } else { &mut sh.cols };
                if on {
                    map.entry(i).or_default().hidden = true;
                } else if let Some(e) = map.get_mut(&i) {
                    e.hidden = false;
                    if e.size == Some(0.0) {
                        e.size = None;
                    }
                }
            }
        }
        cx.structural = true; // SUBTOTAL(10x) depends on hidden rows
        Ok(Json::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_steps() {
        let v = gridcraft_core::Value::Number(1.5);
        assert_eq!(adjust_decimals("General", &v, 1), "0.00");
        assert_eq!(adjust_decimals("General", &v, -1), "0");
        assert_eq!(adjust_decimals("0", &v, 1), "0.0");
        assert_eq!(adjust_decimals("0.00", &v, -1), "0.0");
        assert_eq!(adjust_decimals("0.0", &v, -1), "0");
        assert_eq!(adjust_decimals("#,##0.00;[Red]-#,##0.00", &v, 1), "#,##0.000;[Red]-#,##0.000");
        assert_eq!(adjust_decimals("0%", &v, 1), "0.0%");
    }

    #[test]
    fn width_units() {
        assert_eq!(chars_to_points(8.43), 64.0);
        assert!((points_to_chars(64.0) - 8.43).abs() < 0.01);
    }
}
