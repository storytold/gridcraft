//! More ribbon commands: Chart Design and Format, object arrangement, workbook views, table
//! tools, Get Data, scripts (Automate), cell checkboxes, theme colours and fonts.

use gridcraft_core::{CellRef, RangeRef, Value};
use gridcraft_model::*;
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        // Chart Design / Format
        cmd!(
            "chart.addElement",
            "Add Chart Element",
            ["Chart Design", "Chart Layouts"],
            None,
            "{chart?, element: title|legend|dataLabels|gridlines|axisTitles|smooth|trendline, on?: bool, position?: bottom|top|left|right; trendline: series?: index (default all), type?: linear|exponential|linearForecast|movingAverage|logarithmic|power|polynomial, order?: 2..6, period?: 2..255, forward?, backward?, displayEquation?, displayRSquared?}",
            has_doc,
            add_element
        ),
        cmd!("chart.quickLayout", "Quick Layout", ["Chart Design", "Chart Layouts"], None, "{chart?, layout: 1..6}", has_doc, quick_layout),
        cmd!(
            "chart.changeColors",
            "Change Colors",
            ["Chart Design", "Chart Styles"],
            None,
            "{chart?, palette: colorful|monochrome|accent1..accent6 | colors: [hex…]}",
            has_doc,
            change_colors
        ),
        cmd!(
            "chart.selectData",
            "Select Data…",
            ["Chart Design", "Data"],
            None,
            "{chart?, range?: \"Sheet1!A1:D7\", byRows?: bool, categories?: \"Sheet1!A2:A7\" (Horizontal (Category) Axis Labels for every series; \"\" clears), removeSeries?: [index…]}",
            has_doc,
            select_data
        ),
        cmd!("chart.changeType", "Change Chart Type…", ["Chart Design", "Type"], None, "{chart?, type, subtype?}", has_doc, |s, p| s
            .execute("chart.set", p.clone())),
        cmd!(
            "chart.move",
            "Move Chart…",
            ["Chart Design", "Location"],
            None,
            "{chart?, sheet?: name (moves to that sheet), at?: \"B2\"}",
            has_doc,
            move_chart
        ),
        cmd!(
            "chart.formatSelection",
            "Format Selection",
            ["Format", "Current Selection"],
            None,
            "{chart?, series?: index, color?: hex}",
            has_doc,
            format_series
        ),
        cmd!(
            "chart.shapeFill",
            "Shape Fill",
            ["Format", "Shape Styles"],
            None,
            "{chart? | kind: shape, id?, series?, color: hex}",
            has_doc,
            shape_fill
        ),
        cmd!("chart.shapeOutline", "Shape Outline", ["Format", "Shape Styles"], None, "{kind: shape, id?, color: hex}", has_doc, shape_outline),
        // Arrange
        cmd!(
            "arrange.bringForward",
            "Bring Forward",
            ["Page Layout", "Arrange"],
            None,
            "{kind: chart|image|shape, id, toFront?: bool}",
            has_doc,
            |s, p| arrange(s, p, true)
        ),
        cmd!(
            "arrange.sendBackward",
            "Send Backward",
            ["Page Layout", "Arrange"],
            None,
            "{kind: chart|image|shape, id, toBack?: bool}",
            has_doc,
            |s, p| arrange(s, p, false)
        ),
        cmd!(query "arrange.selectionPane", "Selection Pane", ["Page Layout", "Arrange"], None, "{} → objects on the sheet in stacking order", has_doc, selection_pane),
        cmd!(
            "arrange.align",
            "Align",
            ["Page Layout", "Arrange"],
            None,
            "{kind: chart|image|shape, ids: [..], align: left|center|right|top|middle|bottom}",
            has_doc,
            align
        ),
        cmd!(
            "arrange.group",
            "Group",
            ["Page Layout", "Arrange"],
            None,
            "{kind, ids} (moves objects together: aligns their anchors to the first)",
            has_doc,
            |s, p| align(s, &super::format::merge_json(p.clone(), &json!({"align": "left"})))
        ),
        cmd!(
            "arrange.rotate",
            "Rotate",
            ["Page Layout", "Arrange"],
            None,
            "{kind: shape, id} (swaps width/height for a 90° turn; `flip` is rejected: shapes have no flip state)",
            has_doc,
            rotate
        ),
        // Views
        cmd!(noundo "view.normal", "Normal", ["View", "Workbook Views"], None, "{sheet?} → {view} (per sheet, saved with the file)", has_doc, |s, p| set_view(
            s,
            p,
            SheetViewMode::Normal
        )),
        cmd!(
            noundo "view.pageLayout",
            "Page Layout",
            ["View", "Workbook Views"],
            None,
            "{sheet?} → {view}: pages on paper with rulers; headers and footers are edited in place (per sheet, saved with the file)",
            has_doc,
            |s, p| set_view(s, p, SheetViewMode::PageLayout)
        ),
        cmd!(
            noundo "view.pageBreakPreview",
            "Page Break Preview",
            ["View", "Workbook Views"],
            None,
            "{sheet?} → {view} (per sheet, saved with the file)",
            has_doc,
            |s, p| set_view(s, p, SheetViewMode::PageBreakPreview)
        ),
        cmd!(noundo "view.customViews", "Custom Views…", ["View", "Workbook Views"], None, "{save?: name, show?: name, delete?: name} → list", has_doc, custom_views),
        cmd!(noundo "view.split", "Split", ["View", "Window"], None, "{} (toggles a split at the active cell; shown as frozen panes in this version)", has_doc, split),
        cmd!(noundo "view.newWindow", "New Window", ["View", "Window"], None, "{} opens a second window on the same workbook (a linked copy)", has_doc, new_window),
        cmd!(noundo "view.arrangeAll", "Arrange All", ["View", "Window"], None, "{}", has_doc, |_, _| ok()),
        cmd!(noundo "view.hideWindow", "Hide", ["View", "Window"], None, "{} hides the active window and activates the next visible one → {id, index}; the last visible window cannot be hidden", has_doc, hide_window),
        cmd!(noundo "view.unhideWindow", "Unhide…", ["View", "Window"], None, "{id?, index?} (default: the most recently hidden window) → {id, index}", has_doc, unhide_window),
        cmd!(query "view.switchWindows", "Switch Windows", ["View", "Window"], None, "{} → open windows, as window.list", always, window_list),
        cmd!(query "window.list", "Windows", ["Window"], None, "{} → [{id, index, title, path, dirty, active, hidden}] in document order; `id` is the window's stable identity, `index` its current position", always, window_list),
        cmd!(noundo "window.activate", "Switch Window", ["Window"], None, "{id} or {index} (id wins) → {id, index}; unhides the window if it was hidden", has_doc, activate_window),
        cmd!(noundo "window.next", "Next Window", ["Window"], None, "{} activates the next visible window in document order, wrapping → {id, index}", has_doc, next_window),
        cmd!(noundo "window.previous", "Previous Window", ["Window"], None, "{} activates the previous visible window in document order, wrapping → {id, index}", has_doc, previous_window),
        cmd!(noundo "view.ruler", "Ruler", ["View", "Show"], None, "{on?} → {on}: rulers in Page Layout view (an application option)", has_doc, ruler),
        cmd!(query "view.macros", "Macros", ["View", "Macros"], None, "{} → scripts", has_doc, all_scripts),
        // Table tools
        cmd!("table.resize", "Resize Table", ["Table Design", "Properties"], None, "{table?, range: \"A1:F20\"}", has_doc, table_resize),
        cmd!("table.removeDuplicates", "Remove Duplicates", ["Table Design", "Tools"], None, "{table?, columns?}", has_doc, |s, p| s
            .execute("data.removeDuplicates", p.clone())),
        // Get & Transform
        cmd!(
            "data.fromTextCsv",
            "From Text/CSV",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text, delimiter?: \",\", sheet?: new sheet name} → imports into a new sheet as a table",
            has_doc,
            from_csv
        ),
        cmd!(
            "data.getData",
            "Get Data",
            ["Data", "Get & Transform Data"],
            None,
            "{path | text} (CSV/TSV/JSON array of objects) → new sheet table",
            has_doc,
            from_csv
        ),
        cmd!(
            "data.fromTableRange",
            "From Table/Range",
            ["Data", "Get & Transform Data"],
            None,
            "{range?} → copies the table/range to a new sheet as a table",
            has_doc,
            from_table_range
        ),
        cmd!(query "data.queriesConnections", "Queries & Connections", ["Data", "Queries & Connections"], None, "{} → imported tables", has_doc, queries),
        // Automate
        cmd!(noundo "automate.recordActions", "Record Actions", ["Automate", "Scripting Tools"], None, "{stop?: bool, name?: script name when stopping}", has_doc, record),
        cmd!(
            "automate.newScript",
            "New Script",
            ["Automate", "Scripting Tools"],
            None,
            "{name, commands: [{command, params}] }",
            has_doc,
            new_script
        ),
        cmd!(query "automate.allScripts", "All Scripts", ["Automate", "Scripting Tools"], None, "{} → saved scripts", has_doc, all_scripts),
        cmd!(
            "automate.runScript",
            "Run Script",
            [],
            None,
            "{name, trusted?: bool (runs a script from an untrusted workbook this once)} — refuses scripts in a workbook opened from a file until it is trusted, and steps that reach outside the workbook (file.*, link.*, any `path` param)",
            has_doc,
            run_script
        ),
        cmd!(
            noundo "automate.trustScripts",
            "Trust Scripts",
            ["Automate", "Scripting Tools"],
            None,
            "{trusted?: bool (default true)} → {trusted}: lets this workbook's scripts run for the rest of the session",
            has_doc,
            trust_scripts
        ),
        cmd!("automate.deleteScript", "Delete Script", [], None, "{name}", has_doc, delete_script),
        cmd!(
            noundo "automate.stopAction",
            "Stop Action",
            [],
            None,
            "{} → {stopped: bool}: stops an action left running on the active workbook (an open undo group, such as a debugger's) and puts back the workbook and its undo history as they were when it began",
            has_doc,
            stop_action
        ),
        // Cell controls, themes, borders, styles
        cmd!(
            "insert.checkbox",
            "Checkbox",
            ["Insert", "Cell Controls"],
            None,
            "{range?} (TRUE/FALSE and empty cells draw as checkboxes, empty ones become FALSE, other values show as they are; click or Space toggles; Delete unchecks, then removes; Clear Formats removes and keeps the values)",
            has_doc,
            checkbox
        ),
        cmd!("cell.toggleCheckbox", "Toggle Checkbox", [], Some("Space"), "{cell?}", has_doc, toggle_checkbox),
        cmd!("pageLayout.themeColors", "Colors", ["Page Layout", "Themes"], None, "{name: theme name | colors: [12 hex]}", has_doc, theme_colors),
        cmd!("pageLayout.themeFonts", "Fonts", ["Page Layout", "Themes"], None, "{major, minor}", has_doc, theme_fonts),
        cmd!(
            "pageLayout.themeEffects",
            "Effects",
            ["Page Layout", "Themes"],
            None,
            "{name} (chart effects preset; stored with the theme name)",
            has_doc,
            |_, _| ok()
        ),
        cmd!(
            "pageLayout.background",
            "Background",
            ["Page Layout", "Page Setup"],
            None,
            "{path? | clear?: bool} (sheet background picture)",
            has_doc,
            background
        ),
        cmd!(
            "home.drawBorder",
            "Draw Border",
            ["Home", "Font"],
            None,
            "{range, edge: top|bottom|left|right|grid, style?, color?}",
            has_doc,
            draw_border
        ),
        cmd!(
            "home.newCellStyle",
            "New Cell Style…",
            ["Home", "Styles"],
            None,
            "{name, fromCell?: \"A1\" (default: active cell)}",
            has_doc,
            new_cell_style
        ),
        cmd!(query "home.addIns", "Add-ins", ["Home", "Add-ins"], None, "{} → GridCraft extends through MCP and scripts instead of add-ins", always, |_, _| Ok(json!({"addIns": [], "hint": "Use gridcraft-cli mcp or Automate › scripts"}))),
        cmd!(query "formulas.recentlyUsed", "Recently Used", ["Formulas", "Function Library"], None, "{} → recently used functions in this workbook", has_doc, recently_used),
        cmd!(query "formulas.financial", "Financial", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Financial"}))),
        cmd!(query "formulas.logical", "Logical", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Logical"}))),
        cmd!(query "formulas.text", "Text", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Text"}))),
        cmd!(query "formulas.dateTime", "Date & Time", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "DateTime"}))),
        cmd!(query "formulas.lookupReference", "Lookup & Reference", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "Lookup"}))),
        cmd!(query "formulas.mathTrig", "Math & Trig", ["Formulas", "Function Library"], None, "{}", always, |s, _| s.execute("formulas.functions", json!({"category": "MathTrig"}))),
        cmd!(query "formulas.moreFunctions", "More Functions", ["Formulas", "Function Library"], None, "{category?: Statistical|Engineering|Information|Database|Compatibility|Web|Cube}", always, |s, p| s.execute("formulas.functions", json!({"category": str_param(p, "category").unwrap_or("Statistical")}))),
        cmd!(
            "formulas.useInFormula",
            "Use in Formula",
            ["Formulas", "Defined Names"],
            None,
            "{name} (inserts the name into the cell being edited)",
            has_doc,
            use_in_formula
        ),
        cmd!(query "formulas.watchWindow", "Watch Window", ["Formulas", "Formula Auditing"], None, "{add?: \"Sheet1!B5\", remove?: \"…\"} → watched cells with current values", has_doc, watch_window),
    ]
}

fn find_chart(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    let si = d.wb.active_sheet;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let idx = match u32_param(p, "chart") {
        Some(id) => sh.charts.iter().position(|c| c.id == id),
        None => sh.charts.len().checked_sub(1),
    };
    idx.map(|i| (si, i)).ok_or_else(|| bad("chart", "no chart on this sheet"))
}

fn with_chart(s: &mut Session, p: &Json, f: impl FnOnce(&mut Chart) -> Result<()>) -> Result<Json> {
    let (si, ci) = find_chart(s, p)?;
    edit(s, |cx| {
        let c = cx.sheet_mut(si)?.charts.get_mut(ci).ok_or_else(|| bad("chart", "no such chart"))?;
        f(c)?;
        Ok(json!({"chart": c.id}))
    })
}

fn add_element(s: &mut Session, p: &Json) -> Result<Json> {
    let el = str_param(p, "element").unwrap_or("title").to_string();
    let on = bool_param(p, "on").unwrap_or(true);
    let pos = str_param(p, "position").map(str::to_string);
    with_chart(s, p, |c| {
        match el.as_str() {
            "title" => c.title = if on { c.title.clone().filter(|t| !t.is_empty()).or(Some("Chart Title".into())) } else { Some(String::new()) },
            "legend" => {
                c.legend = if !on {
                    LegendPos::None
                } else {
                    match pos.as_deref() {
                        Some("top") => LegendPos::Top,
                        Some("left") => LegendPos::Left,
                        Some("right") => LegendPos::Right,
                        _ => LegendPos::Bottom,
                    }
                }
            }
            "dataLabels" => c.data_labels = on,
            "gridlines" => c.gridlines = on,
            "axisTitles" => {
                c.x_title = on.then(|| c.x_title.clone().unwrap_or_else(|| "Axis Title".into()));
                c.y_title = on.then(|| c.y_title.clone().unwrap_or_else(|| "Axis Title".into()));
            }
            "smooth" => {
                for s in &mut c.series {
                    s.smooth = on;
                }
            }
            "trendline" => {
                let only = u32_param(p, "series").map(|i| i as usize);
                if only.is_some_and(|i| i >= c.series.len()) {
                    return Err(bad("chart.addElement", "no such series"));
                }
                let targets: Vec<usize> = (0..c.series.len()).filter(|i| only.is_none_or(|o| o == *i)).collect();
                if !on {
                    for i in targets {
                        if let Some(se) = c.series.get_mut(i) {
                            se.trendlines.clear();
                        }
                    }
                    return Ok(());
                }
                let t = trendline_param(p)?;
                let eligible: Vec<usize> =
                    targets.into_iter().filter(|i| gridcraft_chart::supports_trendline(gridcraft_chart::series_plot_kind(c, *i))).collect();
                if eligible.is_empty() {
                    return Err(bad("chart.addElement", "trendlines need a 2-D column, bar, line, area, scatter or bubble series"));
                }
                for i in eligible {
                    if let Some(se) = c.series.get_mut(i) {
                        se.trendlines.push(t.clone());
                    }
                }
            }
            other => return Err(bad("chart.addElement", format!("unknown element `{other}`"))),
        }
        Ok(())
    })
}

/// The trendline described by `chart.addElement`'s trendline parameters (Excel's Add Chart
/// Element ▸ Trendline choices, plus the Format Trendline options).
fn trendline_param(p: &Json) -> Result<Trendline> {
    let num = |k: &str| Trendline::clamp_forecast(f64_param(p, k).unwrap_or(0.0));
    let mut t = Trendline {
        forward: num("forward"),
        backward: num("backward"),
        display_equation: bool_param(p, "displayEquation").unwrap_or(false),
        display_r_squared: bool_param(p, "displayRSquared").unwrap_or(false),
        ..Trendline::default()
    };
    t.kind = match str_param(p, "type").unwrap_or("linear") {
        "linear" => TrendlineKind::Linear,
        // Excel's Linear Forecast: a linear trendline projected two periods ahead.
        "linearForecast" => {
            if t.forward == 0.0 {
                t.forward = 2.0;
            }
            TrendlineKind::Linear
        }
        "exponential" => TrendlineKind::Exponential,
        "logarithmic" => TrendlineKind::Logarithmic,
        "power" => TrendlineKind::Power,
        "polynomial" => TrendlineKind::Polynomial(u32_param(p, "order").unwrap_or(2).clamp(2, 6) as u8),
        "movingAverage" => TrendlineKind::MovingAverage(u32_param(p, "period").unwrap_or(2).clamp(2, 255)),
        other => return Err(bad("chart.addElement", format!("unknown trendline type `{other}`"))),
    };
    Ok(t)
}

fn quick_layout(s: &mut Session, p: &Json) -> Result<Json> {
    let n = u32_param(p, "layout").unwrap_or(1);
    with_chart(s, p, |c| {
        let (legend, labels, grid, axes) = match n {
            2 => (LegendPos::Top, true, false, false),
            3 => (LegendPos::Bottom, false, true, false),
            4 => (LegendPos::Bottom, true, false, false),
            5 => (LegendPos::None, false, true, true),
            6 => (LegendPos::Right, false, true, true),
            _ => (LegendPos::Right, false, true, false),
        };
        c.legend = legend;
        c.data_labels = labels;
        c.gridlines = grid;
        if axes {
            c.x_title.get_or_insert_with(|| "Axis Title".into());
            c.y_title.get_or_insert_with(|| "Axis Title".into());
        } else {
            c.x_title = None;
            c.y_title = None;
        }
        Ok(())
    })
}

fn change_colors(s: &mut Session, p: &Json) -> Result<Json> {
    let explicit: Option<Vec<Color>> =
        p.get("colors").and_then(Json::as_array).map(|a| a.iter().filter_map(|c| c.as_str().and_then(Color::from_hex)).collect());
    let palette = str_param(p, "palette").unwrap_or("colorful").to_string();
    with_chart(s, p, |c| {
        for (i, se) in c.series.iter_mut().enumerate() {
            se.color = match &explicit {
                Some(cols) if !cols.is_empty() => cols.get(i % cols.len()).copied(),
                _ => match palette.as_str() {
                    "monochrome" => Some(Color::Theme(4, (-400 + (i as i16 * 200)).clamp(-800, 800))),
                    p if p.starts_with("accent") => {
                        let k: u8 = p.trim_start_matches("accent").parse().unwrap_or(1);
                        Some(Color::Theme(3 + k.clamp(1, 6), (-500 + i as i16 * 250).clamp(-800, 800)))
                    }
                    _ => None,
                },
            };
        }
        Ok(())
    })
}

fn select_data(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    // A new data range rebuilds the series, as in Excel's Select Data › Chart data range.
    let rebuilt = match str_param(p, "range") {
        Some(range) => {
            let range = range.trim_start_matches('=');
            let (sheet_name, body) = split_sheet(range);
            let src_si = match sheet_name {
                Some(n) => d.wb.sheet_index(&n).ok_or_else(|| bad("chart.selectData", "no such sheet"))?,
                None => d.wb.active_sheet,
            };
            let r = RangeRef::parse(body).ok_or_else(|| bad("chart.selectData", "not a range"))?;
            if r.count() > 1_000_000 {
                return Err(bad("chart.selectData", "range too large for a chart"));
            }
            let src = d.wb.sheet(src_si).ok_or(EngineError::NoDocument)?;
            let by_rows = bool_param(p, "byRows").unwrap_or(r.width() > r.height() + 1);
            let series = super::insert::series_from_range(src, r, by_rows);
            Some((series, format!("{}!{}", gridcraft_formula::quote_sheet(&src.name), r.a1()), by_rows))
        }
        None => None,
    };
    // Horizontal (Category) Axis Labels › Edit: one labels range for every series ("" clears).
    let categories = match str_param(p, "categories") {
        None => None,
        Some(t) if t.trim().trim_start_matches('=').is_empty() => Some(None),
        Some(t) => {
            let t = t.trim().trim_start_matches('=');
            let (sheet_name, body) = split_sheet(t);
            let si = match sheet_name {
                Some(n) => d.wb.sheet_index(&n).ok_or_else(|| bad("chart.selectData", "no such sheet"))?,
                None => d.wb.active_sheet,
            };
            let r = RangeRef::parse(body).ok_or_else(|| bad("chart.selectData", "`categories` is not a range"))?;
            let name = d.wb.sheet(si).map(|sh| sh.name.clone()).ok_or(EngineError::NoDocument)?;
            let abs = |c: CellRef| format!("${}${}", gridcraft_core::col_to_letters(c.col), c.row + 1);
            Some(Some(format!("{}!{}:{}", gridcraft_formula::quote_sheet(&name), abs(r.start), abs(r.end))))
        }
    };
    let mut remove: Vec<usize> = p
        .get("removeSeries")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_u64).filter_map(|i| usize::try_from(i).ok()).collect())
        .unwrap_or_default();
    remove.sort_unstable();
    remove.dedup();
    if rebuilt.is_none() && categories.is_none() && remove.is_empty() {
        return Err(bad("chart.selectData", "missing `range`, `categories` or `removeSeries`"));
    }
    with_chart(s, p, move |c| {
        if let Some((series, source, by_rows)) = rebuilt {
            c.series = series;
            super::insert::scatter_x_from_first(c.kind, &mut c.series);
            c.source = Some(source);
            c.by_rows = by_rows;
        }
        for i in remove.iter().rev() {
            if *i < c.series.len() && c.series.len() > 1 {
                c.series.remove(*i);
            }
        }
        if let Some(cats) = categories {
            for se in &mut c.series {
                se.categories = cats.clone();
            }
        }
        Ok(())
    })
}

fn move_chart(s: &mut Session, p: &Json) -> Result<Json> {
    let (si, ci) = find_chart(s, p)?;
    let target = match p.get("sheet").and_then(Json::as_str) {
        Some(n) => Some(s.doc()?.wb.sheet_index(n).ok_or_else(|| bad("chart.move", "no such sheet"))?),
        None => None,
    };
    let at = cell_param_on(s, p, "at", target.unwrap_or(si))?;
    edit(s, |cx| {
        let mut chart = cx.sheet_mut(si)?.charts.remove(ci);
        if let Some(a) = at {
            chart.anchor.cell = a;
        }
        let dest = target.unwrap_or(si);
        cx.sheet_mut(dest)?.charts.push(chart);
        Ok(Json::Null)
    })
}

fn format_series(s: &mut Session, p: &Json) -> Result<Json> {
    let idx = u32_param(p, "series").unwrap_or(0) as usize;
    let color = str_param(p, "color").and_then(Color::from_hex);
    with_chart(s, p, |c| {
        if let Some(se) = c.series.get_mut(idx) {
            se.color = color;
        }
        Ok(())
    })
}

fn shape_fill(s: &mut Session, p: &Json) -> Result<Json> {
    if str_param(p, "kind") == Some("shape") {
        let color = str_param(p, "color").and_then(Color::from_hex).unwrap_or_default();
        return with_shape(s, p, |sh| sh.fill = color);
    }
    format_series(s, p)
}

fn shape_outline(s: &mut Session, p: &Json) -> Result<Json> {
    let color = str_param(p, "color").and_then(Color::from_hex).unwrap_or_default();
    with_shape(s, p, |sh| sh.line = color)
}

fn with_shape(s: &mut Session, p: &Json, f: impl FnOnce(&mut Shape)) -> Result<Json> {
    let si = s.doc()?.wb.active_sheet;
    let id = u32_param(p, "id");
    edit(s, |cx| {
        let shapes = &mut cx.sheet_mut(si)?.shapes;
        let sh = match id {
            Some(id) => shapes.iter_mut().find(|x| x.id == id),
            None => shapes.last_mut(),
        }
        .ok_or_else(|| bad("shape", "no such shape"))?;
        f(sh);
        Ok(Json::Null)
    })
}

fn arrange(s: &mut Session, p: &Json, forward: bool) -> Result<Json> {
    let id = u32_param(p, "id").ok_or_else(|| bad("arrange", "missing `id`"))?;
    let kind = str_param(p, "kind").unwrap_or("shape").to_string();
    let extreme = bool_param(p, "toFront").or(bool_param(p, "toBack")).unwrap_or(false);
    let si = s.doc()?.wb.active_sheet;
    fn reorder<T>(v: &mut Vec<T>, i: usize, forward: bool, extreme: bool) {
        if i >= v.len() {
            return;
        }
        let item = v.remove(i);
        let j = match (forward, extreme) {
            (true, true) => v.len(),
            (false, true) => 0,
            (true, false) => (i + 1).min(v.len()),
            (false, false) => i.saturating_sub(1),
        };
        v.insert(j, item);
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(si)?;
        match kind.as_str() {
            "chart" => {
                if let Some(i) = sh.charts.iter().position(|c| c.id == id) {
                    reorder(&mut sh.charts, i, forward, extreme);
                }
            }
            "image" => {
                if let Some(i) = sh.images.iter().position(|c| c.id == id) {
                    reorder(&mut sh.images, i, forward, extreme);
                }
            }
            _ => {
                if let Some(i) = sh.shapes.iter().position(|c| c.id == id) {
                    reorder(&mut sh.shapes, i, forward, extreme);
                }
            }
        }
        Ok(Json::Null)
    })
}

fn selection_pane(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let mut v = Vec::new();
    for c in &sh.shapes {
        v.push(json!({"kind": "shape", "id": c.id, "name": format!("{:?} {}", c.kind, c.id), "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    for c in &sh.images {
        v.push(json!({"kind": "image", "id": c.id, "name": format!("Picture {}", c.id), "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    for c in &sh.charts {
        v.push(json!({"kind": "chart", "id": c.id, "name": format!("Chart {}", c.id), "title": c.title, "at": c.anchor.cell.a1(), "anchorMode": anchor_mode_name(c.anchor.mode)}));
    }
    Ok(Json::Array(v))
}

fn anchors_of<'a>(sh: &'a mut Sheet, kind: &str) -> Vec<(u32, &'a mut Anchor)> {
    match kind {
        "chart" => sh.charts.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
        "image" => sh.images.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
        _ => sh.shapes.iter_mut().map(|c| (c.id, &mut c.anchor)).collect(),
    }
}

fn align(s: &mut Session, p: &Json) -> Result<Json> {
    let kind = str_param(p, "kind").unwrap_or("shape").to_string();
    let ids: Vec<u32> =
        p.get("ids").and_then(Json::as_array).map(|a| a.iter().filter_map(|v| v.as_u64().map(|x| x as u32)).collect()).unwrap_or_default();
    let how = str_param(p, "align").unwrap_or("left").to_string();
    let si = s.doc()?.wb.active_sheet;
    edit(s, |cx| {
        let sheet = cx.sheet_mut(si)?;
        let geo: Vec<(u32, f64, f64, f32, f32)> = {
            let shc = sheet.clone();
            anchors_of(sheet, &kind)
                .into_iter()
                .filter(|(id, _)| ids.contains(id))
                .map(|(id, a)| (id, shc.col_left(a.cell.col) + a.dx as f64, shc.row_top(a.cell.row) + a.dy as f64, a.width, a.height))
                .collect()
        };
        if geo.len() < 2 {
            return Ok(Json::Null);
        }
        let left = geo.iter().map(|g| g.1).fold(f64::INFINITY, f64::min);
        let right = geo.iter().map(|g| g.1 + g.3 as f64).fold(f64::NEG_INFINITY, f64::max);
        let top = geo.iter().map(|g| g.2).fold(f64::INFINITY, f64::min);
        let bottom = geo.iter().map(|g| g.2 + g.4 as f64).fold(f64::NEG_INFINITY, f64::max);
        let shc = sheet.clone();
        for (id, a) in anchors_of(sheet, &kind) {
            let Some(g) = geo.iter().find(|g| g.0 == id) else { continue };
            let (mut x, mut y) = (g.1, g.2);
            match how.as_str() {
                "left" => x = left,
                "right" => x = right - g.3 as f64,
                "center" => x = (left + right) / 2.0 - g.3 as f64 / 2.0,
                "top" => y = top,
                "bottom" => y = bottom - g.4 as f64,
                _ => y = (top + bottom) / 2.0 - g.4 as f64 / 2.0,
            }
            let c = CellRef::new(shc.row_at(y), shc.col_at(x));
            a.cell = c;
            a.dx = (x - shc.col_left(c.col)) as f32;
            a.dy = (y - shc.row_top(c.row)) as f32;
        }
        Ok(Json::Null)
    })
}

fn rotate(s: &mut Session, p: &Json) -> Result<Json> {
    if p.get("flip").is_some_and(|f| !f.is_null()) {
        return Err(bad("arrange.rotate", "`flip` is not supported: shapes have no flip state"));
    }
    with_shape(s, p, |sh| std::mem::swap(&mut sh.anchor.width, &mut sh.anchor.height))
}

/// Switches a sheet's view. Like zoom, the view is saved with the file but is not an edit: it
/// neither records an undo step nor marks a clean workbook as changed.
fn set_view(s: &mut Session, p: &Json, mode: SheetViewMode) -> Result<Json> {
    let si = target_sheet(s, p)?;
    let d = s.doc_mut()?;
    let dirty = d.is_dirty();
    let wb = std::sync::Arc::make_mut(&mut d.wb);
    if let Some(sh) = wb.sheet_mut(si) {
        sh.view = mode;
    }
    if !dirty {
        d.saved = d.wb.clone();
    }
    Ok(json!({"view": mode.as_str()}))
}

fn ruler(s: &mut Session, p: &Json) -> Result<Json> {
    let on = bool_param(p, "on").unwrap_or(s.hide_ruler);
    s.hide_ruler = !on;
    Ok(json!({"on": on}))
}

const VIEWS_NAME: &str = "_sc_custom_views";
const SCRIPTS_NAME: &str = "_sc_scripts";
const WATCH_NAME: &str = "_sc_watch";

/// Workbook-level JSON stored in a hidden defined name (round-trips through XLSX).
fn stash_get(wb: &Workbook, key: &str) -> Json {
    wb.names
        .iter()
        .find(|n| n.name == key)
        .and_then(|n| n.formula.strip_prefix('"').and_then(|t| t.strip_suffix('"')).map(|t| t.replace("\"\"", "\"")))
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(json!({}))
}

/// Like `stash_get`, for the name-keyed lists (views, scripts): a stored value that is not a JSON object is treated as empty.
fn stash_map(wb: &Workbook, key: &str) -> Json {
    let v = stash_get(wb, key);
    if v.is_object() { v } else { json!({}) }
}

fn stash_set(wb: &mut Workbook, key: &str, v: &Json) {
    let text = v.to_string();
    let formula = format!("\"{}\"", text.replace('"', "\"\""));
    wb.names.retain(|n| n.name != key);
    wb.names.push(DefinedName { name: key.into(), scope: None, formula, comment: String::new(), hidden: true });
}

/// The Watch Window list isn't an edit (Excel doesn't undo it): undo and redo keep the current
/// list when they swap in another snapshot of the workbook.
pub(crate) fn keep_watch_list(from: &Workbook, to: &mut std::sync::Arc<Workbook>) {
    let find = |wb: &Workbook| wb.names.iter().find(|n| n.name == WATCH_NAME).cloned();
    let keep = find(from);
    if keep == find(to) {
        return;
    }
    let wb = std::sync::Arc::make_mut(to);
    wb.names.retain(|n| n.name != WATCH_NAME);
    wb.names.extend(keep);
}

fn custom_views(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let mut views = stash_map(&d.wb, VIEWS_NAME);
    if let Some(name) = str_param(p, "save") {
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        views[name] = json!({"sheet": d.wb.active_sheet, "selection": d.selection.a1(), "zoom": sh.zoom, "freeze": sh.freeze});
        let v = views.clone();
        edit(s, |cx| {
            stash_set(&mut cx.wb, VIEWS_NAME, &v);
            Ok(())
        })?;
    } else if let Some(name) = str_param(p, "show") {
        let v = views.get(name).cloned().ok_or_else(|| bad("view.customViews", "no such view"))?;
        if let Some(si) = v["sheet"].as_u64() {
            s.execute("sheet.activate", json!({"sheet": si}))?;
        }
        if let Some(sel) = v["selection"].as_str() {
            s.execute("selection.set", json!({"range": sel}))?;
        }
        if let Some(z) = v["zoom"].as_u64() {
            s.execute("view.zoom", json!({"percent": z}))?;
        }
        if let Some(f) = v.get("freeze") {
            let freeze = match (f[0].as_u64(), f[1].as_u64()) {
                (Some(r), Some(c)) => u32::try_from(r).ok().zip(u32::try_from(c).ok()),
                _ => None,
            };
            edit(s, |cx| {
                let si = cx.sheet_index();
                cx.sheet_mut(si)?.freeze = freeze;
                Ok(())
            })?;
        }
    } else if let Some(name) = str_param(p, "delete") {
        if let Some(o) = views.as_object_mut() {
            o.remove(name);
        }
        let v = views.clone();
        edit(s, |cx| {
            stash_set(&mut cx.wb, VIEWS_NAME, &v);
            Ok(())
        })?;
    }
    Ok(stash_get(&s.doc()?.wb, VIEWS_NAME))
}

fn split(s: &mut Session, _: &Json) -> Result<Json> {
    let has = s.doc()?.wb.active().is_some_and(|sh| sh.freeze.is_some());
    if has { s.execute("view.unfreezePanes", json!({})) } else { s.execute("view.freezePanes", json!({})) }
}

fn new_window(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?.clone();
    let mut copy = crate::DocState::new((*d.wb).clone(), d.path.clone(), format!("{}:2", d.display_title()), d.trust);
    copy.selection = d.selection.clone();
    copy.scripts_trusted = d.scripts_trusted;
    let i = s.add_document(copy);
    Ok(json!({"index": i}))
}

// ---------------------------------------------------------------- windows
//
// A window is the document's `uid`: stable for the session, where an index shifts whenever a
// lower document closes. Every command here answers with the window that is active afterwards,
// `{id, index}`, so callers can hold the uid and look the index up when they need it. A later
// multi-window front end (one native window per document) maps its windows to these uids and
// keeps `window.*` as the one API.

/// `{id, index}` of the active window.
fn window_result(s: &Session) -> Result<Json> {
    let d = s.doc()?;
    Ok(json!({"id": d.uid, "index": s.active_index()}))
}

/// Activates document `i`, unhiding it if it was hidden.
fn show_window(s: &mut Session, i: usize) -> Result<Json> {
    if i >= s.documents().len() {
        return Err(bad("window.activate", "no such window"));
    }
    s.set_active(i);
    window_result(s)
}

/// The index of the visible window `step` places from the active one in document order (`1`
/// next, `-1` previous), wrapping around; the active window itself when it is the only visible
/// one.
fn visible_neighbour(s: &Session, step: i64) -> Option<usize> {
    let n = s.documents().len() as i64;
    if n == 0 {
        return None;
    }
    let cur = s.active_index();
    (1..=n)
        .map(|k| (cur as i64).saturating_add(step.saturating_mul(k)).rem_euclid(n) as usize)
        .find(|&j| j == cur || s.documents().get(j).is_some_and(|d| !s.is_hidden(d.uid)))
}

fn window_list(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(Json::Array(
        s.documents()
            .iter()
            .enumerate()
            .map(|(i, d)| {
                json!({
                    "id": d.uid,
                    "index": i,
                    "title": d.display_title(),
                    "path": d.path,
                    "dirty": d.is_dirty(),
                    "active": i == s.active_index(),
                    "hidden": s.is_hidden(d.uid),
                })
            })
            .collect(),
    ))
}

/// `{id?, index?}` → the index of the window it names (`id` wins when both are given); `None`
/// when neither is given, an error when the one given names no open window.
fn window_param(s: &Session, cmd: &str, p: &Json) -> Result<Option<usize>> {
    // A selector that is given but unusable is an error, not "none given".
    let given = |key: &str| p.get(key).is_some_and(|v| !v.is_null());
    if given("id") {
        let id = u64_param(p, "id").ok_or_else(|| bad(cmd, "`id` must be a window id"))?;
        return s.document_index(id).map(Some).ok_or_else(|| bad(cmd, format!("no window with id {id}")));
    }
    if given("index") {
        let i = u32_param(p, "index").ok_or_else(|| bad(cmd, "`index` must be a non-negative integer"))?;
        return if (i as usize) < s.documents().len() { Ok(Some(i as usize)) } else { Err(bad(cmd, format!("no window at index {i}"))) };
    }
    Ok(None)
}

fn activate_window(s: &mut Session, p: &Json) -> Result<Json> {
    let i = window_param(s, "window.activate", p)?.ok_or_else(|| bad("window.activate", "missing `id` or `index`"))?;
    show_window(s, i)
}

fn next_window(s: &mut Session, _: &Json) -> Result<Json> {
    let i = visible_neighbour(s, 1).ok_or(EngineError::NoDocument)?;
    show_window(s, i)
}

fn previous_window(s: &mut Session, _: &Json) -> Result<Json> {
    let i = visible_neighbour(s, -1).ok_or(EngineError::NoDocument)?;
    show_window(s, i)
}

fn hide_window(s: &mut Session, _: &Json) -> Result<Json> {
    let uid = s.doc()?.uid;
    let visible = s.documents().iter().filter(|d| !s.is_hidden(d.uid)).count();
    if visible <= 1 {
        return Err(bad("view.hideWindow", "the last visible window cannot be hidden"));
    }
    if !s.is_hidden(uid) {
        s.hidden_windows.push(uid);
    }
    let next = visible_neighbour(s, 1).ok_or(EngineError::NoDocument)?;
    show_window(s, next)
}

fn unhide_window(s: &mut Session, p: &Json) -> Result<Json> {
    let i = match window_param(s, "view.unhideWindow", p)? {
        Some(i) => i,
        None => {
            let last = s.hidden_windows.last().copied().ok_or_else(|| bad("view.unhideWindow", "no hidden windows"))?;
            s.document_index(last).ok_or_else(|| bad("view.unhideWindow", "no such window"))?
        }
    };
    show_window(s, i)
}

fn table_resize(s: &mut Session, p: &Json) -> Result<Json> {
    let r = str_param(p, "range").and_then(RangeRef::parse).ok_or_else(|| bad("table.resize", "missing `range`"))?;
    let d = s.doc()?;
    let si = d.wb.active_sheet;
    let sh = d.wb.sheet(si).ok_or(EngineError::NoDocument)?;
    let ti = match str_param(p, "table") {
        Some(n) => sh.tables.iter().position(|t| t.name.eq_ignore_ascii_case(n)),
        None => sh.tables.iter().position(|t| t.range.contains(d.selection.active)),
    }
    .ok_or_else(|| bad("table.resize", "select a cell in a table"))?;
    edit(s, |cx| {
        let sheet = cx.sheet_mut(si)?;
        let headers: Vec<String> = (r.start.col..=r.end.col).map(|c| sheet.value(CellRef::new(r.start.row, c)).display()).collect();
        if sheet.tables.iter().enumerate().any(|(i, o)| i != ti && o.range.intersects(&r)) {
            return Err(EngineError::Other("A table can't overlap another table.".into()));
        }
        let t = sheet.tables.get_mut(ti).ok_or_else(|| bad("table", "gone"))?;
        if r.start.row != t.range.start.row || !r.intersects(&t.range) {
            return Err(EngineError::Other(
                "The headers must remain in the same row, and the resulting table range must overlap the original table range.".into(),
            ));
        }
        let mut cols = Vec::new();
        for (i, h) in headers.iter().enumerate() {
            let name = if h.is_empty() { format!("Column{}", i + 1) } else { h.clone() };
            let existing = t.columns.iter().find(|c| c.name.eq_ignore_ascii_case(&name)).cloned();
            cols.push(existing.unwrap_or(TableColumn {
                name,
                totals: TotalsFn::None,
                totals_label: None,
                formula: None,
                unique_name: None,
                xml: None,
            }));
        }
        t.columns = cols;
        t.range = r;
        cx.structural = true;
        Ok(Json::Null)
    })
}

fn from_csv(s: &mut Session, p: &Json) -> Result<Json> {
    let bytes = if let Some(t) = str_param(p, "text") {
        t.as_bytes().to_vec()
    } else if let Some(path) = str_param(p, "path") {
        crate::io::read_file(path)?
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let text = String::from_utf8_lossy(&bytes);
    // JSON array of objects → rows.
    let rows: Vec<Vec<Json>> =
        if text.trim_start().starts_with('[') {
            let arr: Vec<Json> = serde_json::from_str(&text).map_err(|e| bad("data.getData", e.to_string()))?;
            let mut keys: Vec<String> = Vec::new();
            for o in &arr {
                if let Some(m) = o.as_object() {
                    for k in m.keys() {
                        if !keys.contains(k) {
                            keys.push(k.clone());
                        }
                    }
                }
            }
            let mut out = vec![keys.iter().map(|k| json!(k)).collect::<Vec<_>>()];
            for o in &arr {
                out.push(keys.iter().map(|k| o.get(k).cloned().unwrap_or(Json::Null)).collect());
            }
            out
        } else {
            let delim = str_param(p, "delimiter")
                .and_then(|d| d.chars().next())
                .unwrap_or(if text.lines().next().is_some_and(|l| l.contains('\t')) { '\t' } else { ',' });
            let opts = gridcraft_xlsx::CsvOptions { delimiter: delim as u8, ..Default::default() };
            let wb = gridcraft_xlsx::read_csv(&bytes, &opts).map_err(|e| EngineError::Other(e.to_string()))?;
            let sh = wb.sheet(0).ok_or(EngineError::NoDocument)?;
            let Some(u) = sh.used_range() else { return Err(EngineError::Other("The file is empty.".into())) };
            (u.start.row..=u.end.row)
                .map(|r| {
                    (u.start.col..=u.end.col)
                        .map(|c| match sh.value(CellRef::new(r, c)) {
                            Value::Number(n) => json!(n),
                            Value::Bool(b) => json!(b),
                            Value::Empty => Json::Null,
                            v => json!(v.display()),
                        })
                        .collect()
                })
                .collect()
        };
    if rows.is_empty() || rows.len() > 1_048_576 {
        return Err(EngineError::Other("Nothing to import.".into()));
    }
    let name = str_param(p, "sheet").map(str::to_string).or_else(|| {
        str_param(p, "path").and_then(|x| std::path::Path::new(x).file_stem().and_then(|f| f.to_str()).map(|f| f.chars().take(31).collect()))
    });
    let r = s.execute("home.insertSheet", json!({"name": name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    let w = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    s.execute("range.setValues", json!({"range": "A1", "values": rows}))?;
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(rows.len() as u32 - 1, w as u32 - 1));
    let _ = s.execute("insert.table", json!({"range": range.a1(), "header": true}));
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, w as u32 - 1).a1()}));
    Ok(json!({"sheet": r["name"], "rows": rows.len(), "cols": w}))
}

fn from_table_range(s: &mut Session, p: &Json) -> Result<Json> {
    let r = target_range(s, p)?;
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let r = if r.is_single() { crate::selection::current_region(sh, r.start) } else { r };
    let text = crate::display::range_text(&d.wb, d.wb.active_sheet, r);
    from_csv(s, &json!({"text": text, "delimiter": "\t", "sheet": format!("{} (2)", sh.name).chars().take(31).collect::<String>()}))
}

fn queries(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    Ok(Json::Array(
        d.wb.sheets
            .iter()
            .flat_map(|sh| {
                sh.tables
                    .iter()
                    .map(move |t| json!({"name": t.name, "sheet": sh.name, "range": t.range.a1(), "rows": t.range.height().saturating_sub(1)}))
            })
            .collect(),
    ))
}

fn record(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "stop").unwrap_or(false) || s.recording.is_some() {
        let Some(start) = s.recording.take() else { return Ok(json!({"recording": false})) };
        let cmds: Vec<Json> =
            s.journal.iter().skip(start).filter(|(id, _)| !id.starts_with("automate.")).map(|(id, p)| json!({"command": id, "params": p})).collect();
        let name = str_param(p, "name").map(str::to_string).unwrap_or_else(|| format!("Script {}", start));
        s.execute("automate.newScript", json!({"name": name, "commands": cmds}))?;
        return Ok(json!({"recording": false, "saved": name, "steps": cmds.len()}));
    }
    s.recording = Some(s.journal.len());
    Ok(json!({"recording": true}))
}

fn new_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.newScript", "missing `name`"))?.to_string();
    let cmds = p.get("commands").cloned().unwrap_or(json!([]));
    if !cmds.is_array() {
        return Err(bad("automate.newScript", "`commands` must be an array"));
    }
    edit(s, |cx| {
        let mut all = stash_map(&cx.wb, SCRIPTS_NAME);
        all[name.as_str()] = cmds.clone();
        stash_set(&mut cx.wb, SCRIPTS_NAME, &all);
        Ok(json!({"name": name}))
    })
}

fn all_scripts(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let (wb, trusted) = (&d.wb, d.scripts_trusted);
    let all = stash_get(wb, SCRIPTS_NAME);
    let mut list: Vec<Json> = all
        .as_object()
        .map(|o| o.iter().map(|(k, v)| json!({"name": k, "steps": v.as_array().map_or(0, Vec::len), "trusted": trusted})).collect())
        .unwrap_or_default();
    // A preserved VBA project is reported alongside GridCraft's own scripts; it is carried on
    // save but never executed here, so it has no step count.
    if let Some(bytes) = &wb.vba_project {
        list.push(json!({"name": "(VBA)", "steps": 0, "kind": "vba", "size": bytes.len(), "executed": false}));
    }
    Ok(Json::Array(list))
}

/// Whether `wb` carries GridCraft scripts (a workbook opened with them starts untrusted).
pub(crate) fn has_scripts(wb: &Workbook) -> bool {
    stash_map(wb, SCRIPTS_NAME).as_object().is_some_and(|o| !o.is_empty())
}

/// Why a script step is refused, whatever the workbook's trust: scripts stay inside the
/// workbook. Anything that reads or writes files (`file.*`, or any `path` parameter: Get Data,
/// From Text/CSV, Insert Picture) or leaves the app (`link.*` opens URLs) is the caller's to run
/// directly, not a stored script's. (Nested `automate.*` steps are skipped.) This check goes by
/// command id and parameter name, so it is only the up-front refusal: the steps also run inside
/// an [`crate::io::ScriptSandbox`] (file access refused whatever the parameter is called) and
/// [`run_script`] drops any `OpenUrl` request a step raises.
fn denied_script_step(id: &str, params: &Json) -> Option<String> {
    if id.starts_with("file.") || id.starts_with("link.") {
        return Some(format!("scripts can't run `{id}`: it reaches outside the workbook"));
    }
    // Scripts are scoped to their workbook: a step that switches the active window would
    // point every later step at another open document, so those are refused too.
    if id.starts_with("window.") {
        return Some(format!("scripts can't run `{id}`: it reaches outside the workbook"));
    }
    if params.get("path").is_some() {
        return Some(format!("scripts can't pass a file `path` to `{id}`"));
    }
    None
}

fn trust_scripts(s: &mut Session, p: &Json) -> Result<Json> {
    // Only a missing `trusted` means "trust"; `"false"`, `0` or `null` are mistakes, not consent.
    let trusted = match p.get("trusted") {
        None => true,
        Some(Json::Bool(b)) => *b,
        Some(_) => return Err(bad("automate.trustScripts", "`trusted` must be true or false")),
    };
    s.doc_mut()?.scripts_trusted = trusted;
    Ok(json!({"trusted": trusted}))
}

/// Runs a stored script. Scripts in a workbook opened from a file run only once the workbook is
/// trusted for this session (`automate.trustScripts`, or `trusted: true` on this call); steps that
/// reach outside the workbook ([`denied_script_step`]) are refused before any step runs.
fn run_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.runScript", "missing `name`"))?;
    let d = s.doc()?;
    let all = stash_get(&d.wb, SCRIPTS_NAME);
    let steps = all.get(name).and_then(Json::as_array).cloned().ok_or_else(|| bad("automate.runScript", "no such script"))?;
    if !d.scripts_trusted && !bool_param(p, "trusted").unwrap_or(false) {
        return Err(bad(
            "automate.runScript",
            "scripts in this workbook came from a file and are disabled; trust the workbook first (Trust Scripts in View › Macros, Alt+F8; `automate.trustScripts`; or `trusted: true`)",
        ));
    }
    let steps: Vec<Json> = steps.into_iter().take(100_000).collect();
    for st in &steps {
        let id = st["command"].as_str().unwrap_or("");
        if let Some(why) = denied_script_step(id, &st["params"]) {
            return Err(bad("automate.runScript", why));
        }
    }
    let _sandbox = crate::io::ScriptSandbox::enter();
    let mut n = 0;
    for st in &steps {
        let id = st["command"].as_str().unwrap_or("");
        if id.starts_with("automate.") {
            continue;
        }
        let before = s.ui_requests.len();
        let ran = s.execute(id, st["params"].clone());
        // A step that asks the host to open a URL leaves the app: drop the request and stop.
        if s.ui_requests.get(before..).is_some_and(|new| new.iter().any(|r| matches!(r, crate::UiRequest::OpenUrl(_)))) {
            let mut i = 0;
            s.ui_requests.retain(|r| {
                i += 1;
                i <= before || !matches!(r, crate::UiRequest::OpenUrl(_))
            });
            return Err(bad("automate.runScript", format!("scripts can't open links (`{id}`)")));
        }
        ran?;
        n += 1;
    }
    Ok(json!({"ran": n}))
}

/// Automate › Stop Action: rolls back the undo groups open on the active workbook, so Undo and
/// Redo work there again.
fn stop_action(s: &mut Session, _: &Json) -> Result<Json> {
    let uid = s.doc()?.uid;
    Ok(json!({"stopped": s.abort_groups(uid)}))
}

fn delete_script(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("automate.deleteScript", "missing `name`"))?.to_string();
    edit(s, |cx| {
        let mut all = stash_map(&cx.wb, SCRIPTS_NAME);
        if let Some(o) = all.as_object_mut() {
            o.remove(&name);
        }
        stash_set(&mut cx.wb, SCRIPTS_NAME, &all);
        Ok(Json::Null)
    })
}

/// Number format that marks checkbox cells (TRUE/FALSE values drawn as boxes).
pub const CHECKBOX_FORMAT: &str = "\"☐\";\"☐\";\"☐\";[=1]\"☑\"";

/// Whether `c` has checkbox formatting.
pub(crate) fn is_checkbox_cell(wb: &gridcraft_model::Workbook, sh: &gridcraft_model::Sheet, c: CellRef) -> bool {
    wb.styles.get(sh.style_id(c)).num_fmt.as_str() == "checkbox"
}

fn checkbox(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let si = target_sheet(s, p)?;
    edit(s, |cx| {
        for r in &ranges {
            if r.count() > 100_000 {
                return Err(bad("insert.checkbox", "range too large"));
            }
            for c in r.iter() {
                let st = cx.wb.sheet(si).map(|sh| sh.style_id(c)).unwrap_or_default();
                let new = cx.wb.styles.derive(st, |s| {
                    s.num_fmt = NumFmt::new("checkbox");
                    s.align.h = HAlign::Center;
                });
                let sh = cx.sheet_mut(si)?;
                // Values and formulas already there stay (only TRUE/FALSE draw as a box); an empty
                // cell starts unchecked.
                let mut cell = sh.cell(c).cloned().unwrap_or_default();
                if cell.value.is_empty() && cell.formula.is_none() {
                    cell.value = Value::Bool(false);
                }
                cell.style = new;
                sh.set_cell(c, cell);
                cx.touch(si, c);
            }
        }
        Ok(Json::Null)
    })
}

fn toggle_checkbox(s: &mut Session, p: &Json) -> Result<Json> {
    let si = target_sheet(s, p)?;
    let d = s.doc()?;
    let cells: Vec<CellRef> = match cell_param_on(s, p, "cell", si)? {
        Some(c) => vec![c],
        None => d.selection.ranges.iter().flat_map(|r| r.iter().take(100_000).collect::<Vec<_>>()).collect(),
    };
    edit(s, |cx| {
        for c in cells {
            let checkbox = cx.wb.sheet(si).is_some_and(|sh| is_checkbox_cell(&cx.wb, sh, c));
            let sh = cx.sheet_mut(si)?;
            if let Some(cell) = sh.cells.get_mut(c)
                && cell.formula.is_none()
            {
                // An empty checkbox cell counts as FALSE.
                let next = match cell.value {
                    Value::Bool(b) if checkbox => Value::Bool(!b),
                    Value::Empty if checkbox => Value::Bool(true),
                    _ => continue,
                };
                cell.value = next;
                cx.changed.push((si, c));
            }
        }
        Ok(Json::Null)
    })
}

/// Copy of the params that is always a JSON object, so keys can be assigned; any other JSON value counts as no params (like null).
fn object_params(p: &Json) -> Json {
    Json::Object(p.as_object().cloned().unwrap_or_default())
}

fn theme_colors(s: &mut Session, p: &Json) -> Result<Json> {
    let mut params = object_params(p);
    if params.get("name").is_none() {
        params["name"] = json!(s.doc()?.wb.theme.name.clone());
    }
    s.execute("pageLayout.theme", params)
}

fn theme_fonts(s: &mut Session, p: &Json) -> Result<Json> {
    let name = s.doc()?.wb.theme.name.clone();
    let colors: Vec<String> = s.doc()?.wb.theme.colors.iter().map(|c| format!("#{c:06X}")).collect();
    s.execute("pageLayout.theme", json!({"name": name, "colors": colors, "majorFont": p.get("major"), "minorFont": p.get("minor")}))
}

fn background(s: &mut Session, p: &Json) -> Result<Json> {
    if bool_param(p, "clear").unwrap_or(false) {
        let si = s.doc()?.wb.active_sheet;
        return edit(s, |cx| {
            cx.sheet_mut(si)?.images.retain(|i| i.alt != "Sheet background");
            Ok(Json::Null)
        });
    }
    let mut params = object_params(p);
    params["alt"] = json!("Sheet background");
    params["at"] = json!("A1");
    s.execute("insert.picture", params)
}

fn draw_border(s: &mut Session, p: &Json) -> Result<Json> {
    let edge = str_param(p, "edge").unwrap_or("grid");
    let preset = match edge {
        "top" => "top",
        "bottom" => "bottom",
        "left" => "left",
        "right" => "right",
        "outline" => "outside",
        _ => "all",
    };
    let mut params = object_params(p);
    params["preset"] = json!(preset);
    s.execute("home.borders", params)
}

fn new_cell_style(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("home.newCellStyle", "missing `name`"))?.to_string();
    let d = s.doc()?;
    let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
    let c = cell_param(s, p, "fromCell")?.unwrap_or(d.selection.active);
    let style = d.wb.styles.get(sh.style_id(c)).clone();
    edit(s, |cx| {
        cx.wb.cell_styles.retain(|(n, _)| !n.eq_ignore_ascii_case(&name));
        cx.wb.cell_styles.push((name.clone(), style.clone()));
        Ok(json!({"name": name}))
    })
}

fn recently_used(s: &mut Session, _: &Json) -> Result<Json> {
    let mut seen: Vec<String> = Vec::new();
    for (id, p) in s.journal.iter().rev() {
        if id != "cell.set" {
            continue;
        }
        let input = p.get("input").and_then(Json::as_str).unwrap_or("");
        if let Ok(e) = gridcraft_formula::parse(input.trim_start_matches('=')) {
            for f in e.functions() {
                if !seen.iter().any(|x| x == f) {
                    seen.push(f.to_string());
                }
            }
        }
        if seen.len() >= 10 {
            break;
        }
    }
    Ok(json!(seen))
}

fn use_in_formula(s: &mut Session, p: &Json) -> Result<Json> {
    let name = str_param(p, "name").ok_or_else(|| bad("formulas.useInFormula", "missing `name`"))?;
    s.ui_requests.push(crate::UiRequest::EditCell(Some(format!("={name}"))));
    ok()
}

fn watch_window(s: &mut Session, p: &Json) -> Result<Json> {
    let mut list: Vec<String> =
        stash_get(&s.doc()?.wb, WATCH_NAME).as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let mut changed = false;
    if let Some(a) = str_param(p, "add") {
        if !list.iter().any(|x| x == a) {
            list.push(a.to_string());
        }
        changed = true;
    }
    if let Some(r) = str_param(p, "remove") {
        list.retain(|x| x != r);
        changed = true;
    }
    if changed {
        let v = json!(list.clone());
        let d = s.doc_mut()?;
        // The watch list is view state: don't add an undo step.
        let wb = std::sync::Arc::make_mut(&mut d.wb);
        stash_set(wb, WATCH_NAME, &v);
    }
    let d = s.doc()?;
    let rows: Vec<Json> = list
        .iter()
        .map(|w| {
            let (sheet, body) = split_sheet(w);
            let si = sheet.and_then(|n| d.wb.sheet_index(&n)).unwrap_or(d.wb.active_sheet);
            let c = CellRef::parse(body).unwrap_or_default();
            let sh = d.wb.sheet(si);
            json!({"cell": w, "value": sh.map(|s| s.value(c).display()), "formula": sh.and_then(|s| s.cell(c)).and_then(|x| x.formula.as_ref()).map(|f| format!("={}", f.text))})
        })
        .collect();
    Ok(Json::Array(rows))
}
