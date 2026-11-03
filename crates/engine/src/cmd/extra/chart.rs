use super::{
    CellRef, Chart, Color, EngineError, Json, LegendPos, RangeRef, Result, Session, Trendline, TrendlineKind, bad, bool_param, cell_param_on, edit,
    f64_param, json, split_sheet, str_param, u32_param,
};
use gridcraft_model::{ChartKind, ErrorAmount, ErrorBarDirection, ErrorBarSides, ErrorBars};

fn find_chart(s: &Session, p: &Json) -> Result<(usize, usize)> {
    let d = s.doc()?;
    let si = d.wb.active_sheet();
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

pub(super) fn add_element(s: &mut Session, p: &Json) -> Result<Json> {
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
            "dataTable" => crate::cmd::insert::update_data_table(c, p, Some(on))?,
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
            "errorBars" => add_error_bars(c, p, on)?,
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

/// `chart.addElement` `errorBars`: adds (or replaces, per direction) or removes error bars on
/// one series or all that can show them (Excel's Add Chart Element ▸ Error Bars and the Format
/// Error Bars options).
fn add_error_bars(c: &mut Chart, p: &Json, on: bool) -> Result<()> {
    let only = match p.get("series").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => {
            Some(v.as_u64().and_then(|i| usize::try_from(i).ok()).ok_or_else(|| bad("chart.addElement", "`series` must be a non-negative integer"))?)
        }
    };
    if only.is_some_and(|i| i >= c.series.len()) {
        return Err(bad("chart.addElement", "no such series"));
    }
    let targets: Vec<usize> = (0..c.series.len()).filter(|i| only.is_none_or(|o| o == *i)).collect();
    if !on {
        for i in targets {
            if let Some(se) = c.series.get_mut(i) {
                se.error_bars.clear();
            }
        }
        return Ok(());
    }
    let e = error_bars_param(p)?;
    let eligible: Vec<usize> = targets
        .into_iter()
        .filter(|i| {
            let k = gridcraft_chart::series_plot_kind(c, *i);
            gridcraft_chart::supports_error_bars(k)
                && (e.direction == ErrorBarDirection::Y || matches!(k, ChartKind::Scatter | ChartKind::ScatterLines | ChartKind::Bubble))
        })
        .collect();
    if eligible.is_empty() {
        return Err(bad(
            "chart.addElement",
            if e.direction == ErrorBarDirection::X {
                "X error bars need a scatter or bubble series"
            } else {
                "error bars need a 2-D column, bar, line, area, scatter or bubble series"
            },
        ));
    }
    for i in eligible {
        if let Some(se) = c.series.get_mut(i) {
            se.error_bars.retain(|o| o.direction != e.direction);
            se.error_bars.push(e.clone());
        }
    }
    Ok(())
}

/// Longest reference or array text accepted for custom error amounts.
const MAX_ERROR_REF: usize = 8192;

/// A text option of `chart.addElement` errorBars: `default` when absent or null, an error when present and not text.
fn word<'a>(p: &'a Json, k: &str, default: &'static str) -> Result<&'a str> {
    match p.get(k).filter(|v| !v.is_null()) {
        None => Ok(default),
        Some(v) => v.as_str().ok_or_else(|| bad("chart.addElement", format!("`{k}` must be text"))),
    }
}

/// The error bars described by `chart.addElement`'s errorBars parameters. Defaults follow
/// Excel's menu: Standard Error; Percentage is 5%, Standard Deviation 1, Fixed Value 1.
fn error_bars_param(p: &Json) -> Result<ErrorBars> {
    // A key that is absent or null takes its default; one that is present but unusable is an error.
    let given = |k: &str| p.get(k).filter(|v| !v.is_null());
    let value = |default: f64| -> Result<f64> {
        match given("value") {
            None => Ok(default),
            Some(v) => v
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0)
                .ok_or_else(|| bad("chart.addElement", "`value` must be a finite number that is not negative")),
        }
    };
    let amount = match word(p, "type", "standardError")? {
        "standardError" => ErrorAmount::StdErr,
        "percentage" => ErrorAmount::Percentage(value(5.0)?),
        "standardDeviation" => ErrorAmount::StdDev(value(1.0)?),
        "fixed" => ErrorAmount::Fixed(value(1.0)?),
        "custom" => {
            let r = |k: &str| -> Result<Option<String>> {
                match word(p, k, "")?.trim() {
                    "" => Ok(None),
                    t if t.len() > MAX_ERROR_REF => Err(bad("chart.addElement", format!("`{k}` is too long"))),
                    t => {
                        let t = t.strip_prefix('=').unwrap_or(t).trim();
                        match gridcraft_formula::parse(t) {
                            Ok(gridcraft_formula::Expr::Ref(_) | gridcraft_formula::Expr::Name(_)) => Ok(Some(t.to_string())),
                            // One row: the file keeps a flat list.
                            Ok(gridcraft_formula::Expr::Array(rows)) if rows.len() == 1 => Ok(Some(t.to_string())),
                            _ => Err(bad("chart.addElement", format!("`{k}` must be a reference, a name or a one-row array constant"))),
                        }
                    }
                }
            };
            let (plus, minus) = (r("plus")?, r("minus")?);
            if plus.is_none() && minus.is_none() {
                return Err(bad("chart.addElement", "custom error bars need `plus` or `minus`"));
            }
            ErrorAmount::Custom { plus, minus }
        }
        other => return Err(bad("chart.addElement", format!("unknown error bar type `{other}`"))),
    };
    let direction = match word(p, "direction", "y")? {
        "y" => ErrorBarDirection::Y,
        "x" => ErrorBarDirection::X,
        other => return Err(bad("chart.addElement", format!("unknown error bar direction `{other}`"))),
    };
    let sides = match word(p, "display", "both")? {
        "both" => ErrorBarSides::Both,
        "plus" => ErrorBarSides::Plus,
        "minus" => ErrorBarSides::Minus,
        other => return Err(bad("chart.addElement", format!("unknown error bar display `{other}`"))),
    };
    let no_end_cap = match word(p, "endStyle", "cap")? {
        "cap" => false,
        "noCap" => true,
        other => return Err(bad("chart.addElement", format!("unknown error bar end style `{other}`"))),
    };
    let color = match given("color") {
        None => None,
        Some(v) => Some(crate::cmd::format::color_param(Some(v)).ok_or_else(|| bad("chart.addElement", "`color` is not a colour"))?),
    };
    let width = match given("width") {
        None => None,
        Some(v) => Some(
            v.as_f64().filter(|w| w.is_finite() && *w > 0.0).ok_or_else(|| bad("chart.addElement", "`width` must be a positive number"))?.min(100.0)
                as f32,
        ),
    };
    Ok(ErrorBars { direction, sides, amount, no_end_cap, color: color.filter(|c| *c != Color::Auto), width, dash: None })
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

pub(super) fn quick_layout(s: &mut Session, p: &Json) -> Result<Json> {
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

pub(super) fn change_colors(s: &mut Session, p: &Json) -> Result<Json> {
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

pub(super) fn select_data(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    // A new data range rebuilds the series, as in Excel's Select Data › Chart data range.
    let rebuilt = match str_param(p, "range") {
        Some(range) => {
            let range = range.trim_start_matches('=');
            let (sheet_name, body) = split_sheet(range);
            let src_si = match sheet_name {
                Some(n) => d.wb.sheet_index(&n).ok_or_else(|| bad("chart.selectData", "no such sheet"))?,
                None => d.wb.active_sheet(),
            };
            let r = RangeRef::parse(body).ok_or_else(|| bad("chart.selectData", "not a range"))?;
            if r.count() > 1_000_000 {
                return Err(bad("chart.selectData", "range too large for a chart"));
            }
            let src = d.wb.sheet(src_si).ok_or(EngineError::NoDocument)?;
            let by_rows = bool_param(p, "byRows").unwrap_or(r.width() > r.height() + 1);
            let series = crate::cmd::insert::series_from_range(src, r, by_rows);
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
                None => d.wb.active_sheet(),
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
            crate::cmd::insert::scatter_x_from_first(c.kind, &mut c.series);
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

pub(super) fn move_chart(s: &mut Session, p: &Json) -> Result<Json> {
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

pub(super) fn format_series(s: &mut Session, p: &Json) -> Result<Json> {
    let idx = u32_param(p, "series").unwrap_or(0) as usize;
    let color = str_param(p, "color").and_then(Color::from_hex);
    with_chart(s, p, |c| {
        if let Some(se) = c.series.get_mut(idx) {
            se.color = color;
        }
        Ok(())
    })
}
