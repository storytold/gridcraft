//! "Kestrel Coffee Roasters": a fiscal-year dashboard over a month-by-coffee-by-channel sales
//! ledger. Original content; every figure past the ledger is a formula.

use std::sync::Arc;

use gridcraft_model::{CfRule, CfValueKind, ChartKind, Color, Series};
use serde_json::{Value as Json, json};

use super::run;
use crate::Session;

/// Coffee, origin, roast, monthly bags at the start of the year (retail, wholesale, subscription),
/// and the cost of one bag (green coffee, roasting, packing).
const COFFEES: [(&str, &str, &str, [f64; 3], f64); 7] = [
    ("Huila Sunrise", "Colombia", "Medium", [320.0, 900.0, 260.0], 6.2),
    ("Yirgacheffe Bloom", "Ethiopia", "Light", [260.0, 420.0, 300.0], 7.4),
    ("Cerrado Velvet", "Brazil", "Medium-dark", [240.0, 1100.0, 140.0], 5.1),
    ("Antigua Ember", "Guatemala", "Medium", [200.0, 520.0, 180.0], 6.6),
    ("Nyeri Peaberry", "Kenya", "Light", [150.0, 260.0, 170.0], 8.3),
    ("Midnight Espresso", "House blend", "Dark", [380.0, 1400.0, 220.0], 5.4),
    ("Sierra Decaf", "Mexico", "Medium", [110.0, 240.0, 90.0], 6.8),
];
/// Price of a bag per channel, per coffee (retail, wholesale, subscription).
const PRICES: [[f64; 3]; 7] =
    [[19.0, 12.5, 17.0], [22.0, 14.5, 19.5], [17.0, 11.0, 15.5], [20.0, 13.0, 18.0], [24.0, 16.0, 21.5], [18.0, 11.5, 16.0], [18.0, 12.0, 16.0]];
const CHANNELS: [&str; 3] = ["Retail", "Wholesale", "Subscription"];
/// Seasonality from October to September, per channel: the café and web shop peak at the holidays,
/// wholesale builds through the year as accounts open.
const SEASON: [[f64; 12]; 3] = [
    [1.00, 1.09, 1.45, 0.90, 0.87, 0.95, 0.98, 1.01, 0.96, 0.99, 1.02, 1.08],
    [1.00, 1.01, 1.05, 0.96, 0.97, 1.00, 1.02, 1.03, 1.04, 1.05, 1.07, 1.10],
    [1.00, 1.03, 1.10, 1.02, 1.00, 1.01, 1.00, 1.01, 0.99, 1.00, 1.01, 1.02],
];
/// Growth across the year per channel (subscriptions are the story), and per coffee (the light
/// roasts are catching on; the decaf is fading).
const GROWTH: [f64; 3] = [0.12, 0.30, 0.72];
const DRIFT: [f64; 7] = [0.02, 0.20, -0.06, 0.0, 0.32, 0.04, -0.16];
/// Excel serial dates of the first of each month, October 2025 to September 2026.
const MONTHS: [u32; 12] = [45931, 45962, 45992, 46023, 46054, 46082, 46113, 46143, 46174, 46204, 46235, 46266];

const INK: &str = "#1F2321";
const MUTED: &str = "#6B736E";
const ACCENT: &str = "#107C41";
const HEAD_FILL: &str = "#E6F2EB";
const HEAD_INK: &str = "#0E4F2B";
const BAND: &str = "#F6FAF7";
const RULE: &str = "#E3E8E4";

/// A small, repeatable jitter in 0.94..1.06 so the ledger doesn't look generated.
fn jitter(a: usize, b: usize, c: usize) -> f64 {
    let h = (a as u64 * 7919 + b as u64 * 104_729 + c as u64 * 1_299_709).wrapping_mul(2_654_435_761) % 1000;
    0.94 + h as f64 / 1000.0 * 0.12
}

/// Bags of coffee `c` sold through channel `ch` in month `m`.
fn bags(c: usize, ch: usize, m: usize) -> f64 {
    let base = COFFEES.get(c).map_or(0.0, |x| x.3.get(ch).copied().unwrap_or(0.0));
    let season = SEASON.get(ch).and_then(|s| s.get(m)).copied().unwrap_or(1.0);
    let growth = 1.0 + (GROWTH.get(ch).copied().unwrap_or(0.0) + DRIFT.get(c).copied().unwrap_or(0.0)) * m as f64 / 11.0;
    (base * season * growth * jitter(c, ch, m)).round()
}

fn price(c: usize, ch: usize) -> f64 {
    PRICES.get(c).and_then(|p| p.get(ch)).copied().unwrap_or(0.0)
}

fn cost(c: usize) -> f64 {
    COFFEES.get(c).map_or(0.0, |x| x.4)
}

fn series(name: &str, cats: &str, values: &str, kind: Option<ChartKind>, color: Option<u32>) -> Series {
    Series {
        name: Some(name.into()),
        categories: Some(cats.into()),
        values: values.into(),
        bubble_sizes: None,
        color: color.map(Color::Rgb),
        secondary: false,
        kind,
        quartiles: Default::default(),
        smooth: false,
        trendlines: Vec::new(),
    }
}

/// Runs a list of commands, stopping at the first that fails.
fn all(s: &mut Session, steps: Vec<(&str, Json)>) -> bool {
    steps.into_iter().all(|(id, p)| run(s, id, p))
}

pub(super) fn showcase(s: &mut Session) -> bool {
    // Coffees in order of the year's revenue, so the dashboard's table reads as a ranking.
    let mut order: Vec<usize> = (0..COFFEES.len()).collect();
    let revenue = |c: usize| (0..12).map(|m| (0..3).map(|ch| bags(c, ch, m) * price(c, ch)).sum::<f64>()).sum::<f64>();
    order.sort_by(|a, b| revenue(*b).total_cmp(&revenue(*a)));
    // A green theme: charts without their own colours (the doughnut's slices) take its accents.
    let theme = ["#FFFFFF", "#000000", "#EEF3EF", "#1F2321", "#107C41", "#6FBF92", "#B7DCC6", "#0E4F2B", "#2E9A62", "#5E8A70", "#0E6B38", "#5F6661"];
    run(s, "pageLayout.theme", json!({"name": "Kestrel", "colors": theme, "majorFont": "Inter", "minorFont": "Inter"}))
        && data_sheet(s)
        && monthly_sheet(s)
        && dashboard(s, &order)
        && monthly_analysis(s)
        && pivot_sheet(s)
        && run(s, "sheet.activate", json!({"sheet": 0}))
}

/// The ledger: one row per month, coffee and channel.
fn data_sheet(s: &mut Session) -> bool {
    let mut rows = vec![json!(["Month", "Coffee", "Channel", "Bags", "Revenue", "Cost"])];
    for (m, month) in MONTHS.iter().enumerate() {
        for (c, coffee) in COFFEES.iter().enumerate() {
            for (ch, channel) in CHANNELS.iter().enumerate() {
                let n = bags(c, ch, m);
                rows.push(json!([month, coffee.0, channel, n, n * price(c, ch), (n * cost(c) * 100.0).round() / 100.0]));
            }
        }
    }
    let last = rows.len() + 3;
    all(
        s,
        vec![
            ("home.insertSheet", json!({"name": "Data", "before": 1})),
            ("range.setValues", json!({"range": "B4", "values": rows})),
            ("insert.table", json!({"range": format!("B4:G{last}"), "style": "TableStyleLight1", "name": "Sales"})),
            ("range.setValues", json!({"range": "B2", "values": [["Sales ledger"]]})),
            ("range.setValues", json!({"range": "B3", "values": [["One row per month, coffee and channel · bags of 340 g"]]})),
            ("home.numberFormat", json!({"range": format!("B5:B{last}"), "code": "mmm yyyy"})),
            ("home.numberFormat", json!({"range": format!("E5:E{last}"), "code": "#,##0"})),
            ("home.numberFormat", json!({"range": format!("F5:G{last}"), "code": "\"$\"#,##0.00"})),
            ("home.fontName", json!({"range": format!("A1:H{last}"), "name": "Inter"})),
            ("home.fontSize", json!({"range": format!("A1:H{last}"), "size": 11})),
            ("home.rowHeight", json!({"rows": format!("1:{last}"), "height": 30})),
            ("home.alignMiddle", json!({"range": format!("A1:H{last}")})),
            ("home.fontSize", json!({"range": "B2", "size": 18})),
            ("home.bold", json!({"range": "B2", "on": true})),
            ("home.fontColor", json!({"range": "B2", "color": INK})),
            ("home.fontColor", json!({"range": "B3", "color": MUTED})),
            ("home.fillColor", json!({"range": "B4:G4", "color": HEAD_FILL})),
            ("home.fontColor", json!({"range": "B4:G4", "color": HEAD_INK})),
            ("home.borders", json!({"range": "B4:G4", "preset": "bottom", "style": "medium", "color": ACCENT})),
            ("home.columnWidth", json!({"cols": "A:A", "width": 30})),
            ("home.columnWidth", json!({"cols": "B:B", "width": 110})),
            ("home.columnWidth", json!({"cols": "C:C", "width": 170})),
            ("home.columnWidth", json!({"cols": "D:D", "width": 130})),
            ("home.columnWidth", json!({"cols": "E:G", "width": 110})),
            ("home.alignLeft", json!({"range": format!("B5:B{last}")})),
            ("sheet.tabColor", json!({"color": "#9AA39D"})),
            ("view.gridlines", json!({"on": false})),
            ("view.freezePanes", json!({"cell": "A5"})),
            ("selection.set", json!({"cell": "B5"})),
        ],
    )
}

/// The month-by-month view: channels, margin and the prior year, plus the blocks the dashboard
/// charts read.
fn monthly_sheet(s: &mut Session) -> bool {
    let mut rows = vec![json!(["Month", "Retail", "Wholesale", "Subscription", "Revenue", "Gross profit", "Margin", "Bags", "FY25", "vs FY25"])];
    for (m, month) in MONTHS.iter().enumerate() {
        // Last year's month: the same business about a fifth smaller, a little less so late in the year.
        let now: f64 = (0..COFFEES.len()).map(|c| (0..3).map(|ch| bags(c, ch, m) * price(c, ch)).sum::<f64>()).sum();
        let before = (now / (1.30 - 0.008 * m as f64) / 10.0).round() * 10.0;
        rows.push(json!([month, null, null, null, null, null, null, null, before]));
    }
    let mut steps = vec![
        ("home.insertSheet", json!({"name": "Monthly", "before": 1})),
        // A workbook-level LAMBDA, used across the book.
        (
            "formulas.defineName",
            json!({"name": "PctChange", "refersTo": "=LAMBDA(current,prior,IF(prior=0,0,current/prior-1))", "comment": "Change from prior to current, as a fraction."}),
        ),
        ("range.setValues", json!({"range": "B5", "values": rows})),
    ];
    for r in 6..=17 {
        for (col, ch) in [("C", "Retail"), ("D", "Wholesale"), ("E", "Subscription")] {
            steps.push((
                "cell.set",
                json!({"cell": format!("{col}{r}"), "input": format!("=SUMIFS(Sales[Revenue],Sales[Month],$B{r},Sales[Channel],\"{ch}\")")}),
            ));
        }
        steps.push(("cell.set", json!({"cell": format!("F{r}"), "input": format!("=SUM(C{r}:E{r})")})));
        steps.push(("cell.set", json!({"cell": format!("G{r}"), "input": format!("=F{r}-SUMIFS(Sales[Cost],Sales[Month],$B{r})")})));
        steps.push(("cell.set", json!({"cell": format!("H{r}"), "input": format!("=G{r}/F{r}")})));
        steps.push(("cell.set", json!({"cell": format!("I{r}"), "input": format!("=SUMIFS(Sales[Bags],Sales[Month],$B{r})")})));
        steps.push(("cell.set", json!({"cell": format!("K{r}"), "input": format!("=PctChange(F{r},J{r})")})));
    }
    steps.extend([
        ("insert.table", json!({"range": "B5:K17", "style": "TableStyleLight1", "name": "Monthly"})),
        ("table.totalRow", json!({"table": "Monthly", "on": true})),
    ]);
    for col in ["Retail", "Wholesale", "Subscription", "Revenue", "Gross profit", "Bags", "FY25"] {
        steps.push(("table.totalFunction", json!({"table": "Monthly", "column": col, "function": "sum"})));
    }
    steps.extend([
        ("table.totalFunction", json!({"table": "Monthly", "column": "Margin", "function": "none"})),
        ("table.totalFunction", json!({"table": "Monthly", "column": "vs FY25", "function": "none"})),
        ("cell.set", json!({"cell": "H18", "input": "=G18/F18"})),
        ("cell.set", json!({"cell": "K18", "input": "=PctChange(F18,J18)"})),
        ("range.setValues", json!({"range": "B2", "values": [["Month by month"]]})),
        ("range.setValues", json!({"range": "B3", "values": [["Revenue by channel from the Data ledger, against FY25"]]})),
        ("home.numberFormat", json!({"range": "B6:B17", "code": "mmm yyyy"})),
        ("home.numberFormat", json!({"range": "C6:G18", "code": "\"$\"#,##0"})),
        ("home.numberFormat", json!({"range": "J6:J18", "code": "\"$\"#,##0"})),
        ("home.numberFormat", json!({"range": "H6:H18", "code": "0.0%"})),
        ("home.numberFormat", json!({"range": "I6:I18", "code": "#,##0"})),
        ("home.numberFormat", json!({"range": "K6:K18", "code": "+0.0%;-0.0%;0.0%"})),
        ("home.conditionalFormat", json!({"range": "F6:F17", "rule": {"type": "dataBar", "color": "#CDE8D7", "gradient": false}})),
    ]);
    all(s, steps)
}

/// The blocks below the monthly table that need the dashboard's coffee list: a coffee-by-month
/// heat map, revenue by roast and by channel, and the fastest growers.
fn monthly_analysis(s: &mut Session) -> bool {
    let mut steps = vec![
        ("sheet.activate", json!({"sheet": "Monthly"})),
        ("range.setValues", json!({"range": "B21", "values": [["Revenue by coffee and month"]]})),
        ("range.setValues", json!({"range": "B22", "values": [["Coffee"]]})),
        // Dynamic arrays: the months across, the coffees down, and one SUMIFS that spills the grid.
        ("cell.set", json!({"cell": "C22", "input": "=TRANSPOSE(Monthly[Month])"})),
        ("cell.set", json!({"cell": "B23", "input": "=Coffees[Coffee]"})),
        ("cell.set", json!({"cell": "C23", "input": "=SUMIFS(Sales[Revenue],Sales[Coffee],B23#,Sales[Month],C22#)"})),
        ("range.setValues", json!({"range": "B32", "values": [["By roast", "Revenue", "Share"]]})),
        ("cell.set", json!({"cell": "B33", "input": "=LET(r,UNIQUE(Coffees[Roast]),SORTBY(r,SUMIFS(Coffees[Revenue],Coffees[Roast],r),1))"})),
        ("cell.set", json!({"cell": "C33", "input": "=SUMIFS(Coffees[Revenue],Coffees[Roast],B33#)"})),
        ("cell.set", json!({"cell": "D33", "input": "=C33#/SUM(Coffees[Revenue])"})),
        ("range.setValues", json!({"range": "F32", "values": [["By channel", "Revenue", "Margin"], ["Wholesale"], ["Retail"], ["Subscription"]]})),
    ];
    for r in 33..=35 {
        steps.push(("cell.set", json!({"cell": format!("G{r}"), "input": format!("=SUMIFS(Sales[Revenue],Sales[Channel],F{r})")})));
        steps.push((
            "cell.set",
            json!({"cell": format!("H{r}"), "input": format!("=LET(rev,G{r},cost,SUMIFS(Sales[Cost],Sales[Channel],F{r}),(rev-cost)/rev)")}),
        ));
    }
    steps.extend([
        ("home.numberFormat", json!({"range": "C22:N22", "code": "mmm"})),
        ("home.numberFormat", json!({"range": "C23:N29", "code": "\"$\"#,##0.0,\"K\""})),
        ("home.conditionalFormat", json!({"range": "C23:N29", "rule": {"type": "colorScale", "colors": ["#FFFFFF", "#9ED0B3"]}})),
        ("home.numberFormat", json!({"range": "C33:C36", "code": "\"$\"#,##0"})),
        ("home.numberFormat", json!({"range": "D33:D36", "code": "0.0%"})),
        (
            "home.conditionalFormat",
            json!({"range": "D33:D36", "rule": {"type": "dataBar", "color": "#CDE8D7", "gradient": false, "min": {"type": "num", "value": 0}}}),
        ),
        ("home.numberFormat", json!({"range": "G33:G35", "code": "\"$\"#,##0"})),
        ("home.numberFormat", json!({"range": "H33:H35", "code": "0.0%"})),
        // The look, shared with the dashboard.
        ("home.fontName", json!({"range": "A1:O40", "name": "Inter"})),
        ("home.fontSize", json!({"range": "A1:O40", "size": 11})),
        ("home.rowHeight", json!({"rows": "1:40", "height": 30})),
        ("home.alignMiddle", json!({"range": "A1:O40"})),
        ("home.fontSize", json!({"range": "B2", "size": 18})),
        ("home.bold", json!({"range": "B2", "on": true})),
        ("home.fontColor", json!({"range": "B2", "color": INK})),
        ("home.fontColor", json!({"range": "B3", "color": MUTED})),
        ("home.rowHeight", json!({"rows": "4:4", "height": 14})),
        ("home.rowHeight", json!({"rows": "19:20", "height": 18})),
        ("home.rowHeight", json!({"rows": "30:31", "height": 18})),
        ("home.fillColor", json!({"range": "B5:K5", "color": HEAD_FILL})),
        ("home.fontColor", json!({"range": "B5:K5", "color": HEAD_INK})),
        ("home.borders", json!({"range": "B5:K5", "preset": "bottom", "style": "medium", "color": ACCENT})),
        ("home.fontSize", json!({"range": "B21", "size": 13})),
        ("home.bold", json!({"range": "B21", "on": true})),
        ("home.fontColor", json!({"range": "B21", "color": INK})),
    ]);
    for head in ["B22:N22", "B32:D32", "F32:H32"] {
        steps.push(("home.bold", json!({"range": head, "on": true})));
        steps.push(("home.fontColor", json!({"range": head, "color": HEAD_INK})));
        steps.push(("home.fillColor", json!({"range": head, "color": HEAD_FILL})));
        steps.push(("home.borders", json!({"range": head, "preset": "bottom", "style": "medium", "color": ACCENT})));
    }
    for block in ["B23:N29", "B33:D36", "F33:H35"] {
        steps.push(("home.borders", json!({"range": block, "preset": "insideHorizontal", "color": RULE})));
        steps.push(("home.borders", json!({"range": block, "preset": "bottom", "color": RULE})));
    }
    steps.extend([
        ("home.alignRight", json!({"range": "C22:N22"})),
        ("home.alignRight", json!({"range": "C32:D32"})),
        ("home.alignRight", json!({"range": "G32:H32"})),
        ("home.alignLeft", json!({"range": "B6:B17"})),
        ("home.alignRight", json!({"range": "C5:J5"})),
        ("home.alignRight", json!({"range": "K5"})),
        ("home.fontColor", json!({"range": "K6:K18", "color": ACCENT})),
        ("selection.set", json!({"cell": "B6"})),
        ("data.filter", json!({})),
        ("home.columnWidth", json!({"cols": "A:A", "width": 30})),
        ("home.columnWidth", json!({"cols": "B:B", "width": 150})),
        ("home.columnWidth", json!({"cols": "C:K", "width": 104})),
        ("home.columnWidth", json!({"cols": "L:N", "width": 80})),
        ("home.columnWidth", json!({"cols": "E:E", "width": 112})),
        ("sheet.tabColor", json!({"color": "#6FBF92"})),
        ("view.gridlines", json!({"on": false})),
        ("selection.set", json!({"cell": "B6"})),
    ]);
    all(s, steps)
}

/// Icon sets in the sheet read against fixed growth bands (falling, steady, strong) rather than
/// the default thirds of the range.
fn icon_thresholds(s: &mut Session, sheet: &str, low: f64, high: f64) -> bool {
    let Ok(d) = s.doc_mut() else { return false };
    let wb = Arc::make_mut(&mut d.wb);
    let Some(i) = wb.sheet_index(sheet) else { return false };
    if let Some(sh) = wb.sheet_mut(i) {
        for cf in &mut sh.cond_formats {
            if let CfRule::IconSet { thresholds, .. } = &mut cf.rule {
                *thresholds = vec![CfValueKind::Number(low), CfValueKind::Number(high)];
            }
        }
    }
    true
}

fn pivot_sheet(s: &mut Session) -> bool {
    all(
        s,
        vec![
            ("home.insertSheet", json!({"name": "Pivot", "before": 2})),
            (
                "insert.pivotTable",
                json!({"source": "Sales", "destination": "Pivot!B5", "name": "ByChannel", "rows": ["Coffee"], "columns": ["Channel"], "values": [{"field": "Revenue", "func": "sum", "name": "Total revenue"}]}),
            ),
            ("pivot.valueSettings", json!({"pivot": "ByChannel", "index": 0, "numberFormat": "\"$\"#,##0"})),
            ("pivot.style", json!({"pivot": "ByChannel", "style": "PivotStyleLight14"})),
            ("pivot.sort", json!({"pivot": "ByChannel", "field": "Coffee", "order": "asc"})),
            ("range.setValues", json!({"range": "B2", "values": [["Revenue by coffee and channel"]]})),
            ("range.setValues", json!({"range": "B3", "values": [["A PivotTable over the Sales ledger · refresh it after the data changes"]]})),
            ("home.fontName", json!({"range": "A1:H20", "name": "Inter"})),
            ("home.fontSize", json!({"range": "A1:H20", "size": 11})),
            ("home.rowHeight", json!({"rows": "1:20", "height": 30})),
            ("home.alignMiddle", json!({"range": "A1:H20"})),
            ("home.fontSize", json!({"range": "B2", "size": 18})),
            ("home.bold", json!({"range": "B2", "on": true})),
            ("home.fontColor", json!({"range": "B2", "color": INK})),
            ("home.fontColor", json!({"range": "B3", "color": MUTED})),
            ("home.columnWidth", json!({"cols": "A:A", "width": 30})),
            ("home.columnWidth", json!({"cols": "B:B", "width": 170})),
            ("home.columnWidth", json!({"cols": "C:F", "width": 116})),
            ("selection.set", json!({"cell": "B7"})),
            (
                "insert.pivotChart",
                json!({"pivot": "ByChannel", "type": "bar", "subtype": "stacked", "title": "Revenue by coffee and channel", "at": "H5", "width": 620, "height": 380}),
            ),
            ("chart.changeColors", json!({"colors": ["#B7DCC6", "#6FBF92", "#107C41"]})),
            ("chart.addElement", json!({"element": "legend", "position": "top"})),
            ("sheet.tabColor", json!({"color": "#2E9A62"})),
            ("view.gridlines", json!({"on": false})),
            ("selection.set", json!({"cell": "B2"})),
        ],
    )
}

/// The hero sheet: title, a strip of four figures, the coffee ranking and three charts.
fn dashboard(s: &mut Session, order: &[usize]) -> bool {
    let mut rows = vec![json!(["Coffee", "Origin", "Roast", "Revenue", "Margin", "H2 growth", "Trend"])];
    for &c in order {
        if let Some(x) = COFFEES.get(c) {
            rows.push(json!([x.0, x.1, x.2]));
        }
    }
    let mut steps = vec![("sheet.activate", json!({"sheet": 0})), ("range.setValues", json!({"range": "B10", "values": rows}))];
    for r in 11..=17 {
        steps.push(("cell.set", json!({"cell": format!("E{r}"), "input": format!("=SUMIFS(Sales[Revenue],Sales[Coffee],B{r})")})));
        steps.push(("cell.set", json!({"cell": format!("F{r}"), "input": format!("=1-SUMIFS(Sales[Cost],Sales[Coffee],B{r})/E{r}")})));
        steps.push((
            "cell.set",
            json!({"cell": format!("G{r}"), "input": format!("=PctChange(SUMIFS(Sales[Revenue],Sales[Coffee],B{r},Sales[Month],\">=\"&DATE(2026,4,1)),SUMIFS(Sales[Revenue],Sales[Coffee],B{r},Sales[Month],\"<\"&DATE(2026,4,1)))")}),
        ));
        steps.push(("insert.sparkline", json!({"range": format!("Monthly!C{}:N{}", r + 12, r + 12), "location": format!("H{r}"), "type": "line"})));
    }
    steps.extend([
        ("insert.table", json!({"range": "B10:H17", "style": "TableStyleLight1", "name": "Coffees"})),
        ("table.totalRow", json!({"table": "Coffees", "on": true})),
        ("table.totalFunction", json!({"table": "Coffees", "column": "Revenue", "function": "sum"})),
        ("table.totalFunction", json!({"table": "Coffees", "column": "Trend", "function": "none"})),
        ("cell.set", json!({"cell": "F18", "input": "=1-SUM(Sales[Cost])/E18"})),
        ("cell.set", json!({"cell": "G18", "input": "=PctChange(SUM(Monthly!F12:F17),SUM(Monthly!F6:F11))"})),
        ("selection.set", json!({"cell": "B11"})),
        ("data.filter", json!({})),
        // The figures strip.
        ("range.setValues", json!({"range": "B5", "values": [["Revenue", "Gross margin", null, "Bags shipped", null, "Subscription run-rate"]]})),
        ("cell.set", json!({"cell": "B6", "input": "=SUM(Sales[Revenue])"})),
        ("cell.set", json!({"cell": "B7", "input": "=PctChange(B6,Monthly!J18)"})),
        ("cell.set", json!({"cell": "C6", "input": "=LET(rev,SUM(Sales[Revenue]),cost,SUM(Sales[Cost]),(rev-cost)/rev)"})),
        ("cell.set", json!({"cell": "C7", "input": "=(C6-0.562)*100"})),
        ("cell.set", json!({"cell": "E6", "input": "=SUM(Sales[Bags])"})),
        ("cell.set", json!({"cell": "E7", "input": "=PctChange(E6,93400)"})),
        ("cell.set", json!({"cell": "G6", "input": "=XLOOKUP(MAX(Monthly[Month]),Monthly[Month],Monthly[Subscription])*12"})),
        ("cell.set", json!({"cell": "G7", "input": "=PctChange(G6,INDEX(Monthly[Subscription],1)*12)"})),
        ("home.numberFormat", json!({"range": "B6", "code": "\"$\"0.00,,\"M\""})),
        ("home.numberFormat", json!({"range": "B7", "code": "\"+\"0.0%\"  vs FY25\";\"-\"0.0%\"  vs FY25\""})),
        ("home.numberFormat", json!({"range": "C6", "code": "0.0%"})),
        ("home.numberFormat", json!({"range": "C7", "code": "\"+\"0.0\" pts  vs FY25\";\"-\"0.0\" pts  vs FY25\""})),
        ("home.numberFormat", json!({"range": "E6", "code": "#,##0"})),
        ("home.numberFormat", json!({"range": "E7", "code": "\"+\"0.0%\"  vs FY25\";\"-\"0.0%\"  vs FY25\""})),
        ("home.numberFormat", json!({"range": "G6", "code": "\"$\"#,##0,\"K\""})),
        ("home.numberFormat", json!({"range": "G7", "code": "\"+\"0%\"  since October\";\"-\"0%\"  since October\""})),
        ("home.numberFormat", json!({"range": "E11:E18", "code": "\"$\"#,##0"})),
        ("home.numberFormat", json!({"range": "F11:F18", "code": "0.0%"})),
        ("home.numberFormat", json!({"range": "G11:G18", "code": "+0.0%;-0.0%;0.0%"})),
        ("home.conditionalFormat", json!({"range": "E11:E17", "rule": {"type": "dataBar", "color": "#CDE8D7", "gradient": false}})),
        ("home.conditionalFormat", json!({"range": "G11:G17", "rule": {"type": "iconSet", "set": "3Arrows"}})),
        ("home.alignLeft", json!({"range": "G10:G18"})),
        ("home.increaseIndent", json!({"range": "G10:G18"})),
        ("home.alignRight", json!({"range": "E10"})),
        ("home.alignCenter", json!({"range": "F10:F18"})),
        (
            "home.conditionalFormat",
            json!({"range": "B7:H7", "rule": {"type": "cellIs", "operator": "less", "value": 0, "style": {"font": {"color": "#C0392B"}}}}),
        ),
        // Type: Inter at 12 pt on 26-screen-px rows, with a title, a quiet subtitle and section labels.
        ("home.fontName", json!({"range": "A1:Q40", "name": "Inter"})),
        ("home.fontSize", json!({"range": "A1:Q40", "size": 12})),
        ("home.rowHeight", json!({"rows": "1:40", "height": 32.5})),
        ("home.alignMiddle", json!({"range": "A1:Q40"})),
        ("range.setValues", json!({"range": "B2", "values": [["Kestrel Coffee Roasters"]]})),
        ("range.setValues", json!({"range": "B3", "values": [["Fiscal year 2026 performance · October 2025 to September 2026 · USD"]]})),
        ("home.fontSize", json!({"range": "B2", "size": 21})),
        ("home.bold", json!({"range": "B2", "on": true})),
        ("home.fontColor", json!({"range": "B2", "color": INK})),
        ("home.fontColor", json!({"range": "B3", "color": MUTED})),
        ("home.rowHeight", json!({"rows": "1:1", "height": 14})),
        ("home.rowHeight", json!({"rows": "2:2", "height": 42})),
        ("home.rowHeight", json!({"rows": "3:3", "height": 24})),
        ("home.rowHeight", json!({"rows": "4:4", "height": 22})),
        ("home.rowHeight", json!({"rows": "5:5", "height": 34})),
        ("home.rowHeight", json!({"rows": "6:6", "height": 44})),
        ("home.rowHeight", json!({"rows": "7:7", "height": 32})),
        ("home.rowHeight", json!({"rows": "8:8", "height": 22})),
        ("home.rowHeight", json!({"rows": "9:9", "height": 36})),
        ("range.setValues", json!({"range": "B9", "values": [["Coffees, ranked by revenue"]]})),
        ("home.fontSize", json!({"range": "B9", "size": 13})),
        ("home.bold", json!({"range": "B9", "on": true})),
        ("home.fontColor", json!({"range": "B9", "color": INK})),
        ("home.alignBottom", json!({"range": "B9"})),
        // The fastest growers, as one dynamic array: sort the table by growth and keep the top three.
        ("range.setValues", json!({"range": "B20", "values": [["Fastest growing, second half against first"]]})),
        (
            "cell.set",
            json!({"cell": "B21", "input": "=TAKE(SORTBY(HSTACK(Coffees[Coffee],Coffees[Origin],Coffees[H2 growth]),Coffees[H2 growth],-1),3)"}),
        ),
        ("home.rowHeight", json!({"rows": "19:19", "height": 14})),
        ("home.rowHeight", json!({"rows": "20:20", "height": 34})),
        ("home.rowHeight", json!({"rows": "21:23", "height": 30})),
        ("home.fontSize", json!({"range": "B20", "size": 13})),
        ("home.bold", json!({"range": "B20", "on": true})),
        ("home.fontColor", json!({"range": "B20", "color": INK})),
        ("home.alignBottom", json!({"range": "B20"})),
        ("home.fontColor", json!({"range": "C21:C23", "color": MUTED})),
        ("home.numberFormat", json!({"range": "D21:D23", "code": "+0.0%;-0.0%"})),
        ("home.fontColor", json!({"range": "D21:D23", "color": ACCENT})),
        ("home.bold", json!({"range": "D21:D23", "on": true})),
        ("home.borders", json!({"range": "B21:D23", "preset": "insideHorizontal", "color": RULE})),
        ("home.borders", json!({"range": "B21:D23", "preset": "bottom", "color": RULE})),
        // The strip: four figures on a soft card, split by hairlines.
        ("home.mergeCells", json!({"range": "C5:D5"})),
        ("home.mergeCells", json!({"range": "C6:D6"})),
        ("home.mergeCells", json!({"range": "C7:D7"})),
        ("home.mergeCells", json!({"range": "E5:F5"})),
        ("home.mergeCells", json!({"range": "E6:F6"})),
        ("home.mergeCells", json!({"range": "E7:F7"})),
        ("home.mergeCells", json!({"range": "G5:H5"})),
        ("home.mergeCells", json!({"range": "G6:H6"})),
        ("home.mergeCells", json!({"range": "G7:H7"})),
        ("home.fillColor", json!({"range": "B5:H7", "color": BAND})),
        ("home.borders", json!({"range": "B5:H7", "preset": "outside", "color": "#D5E3DA"})),
        ("home.borders", json!({"range": "C5:C7", "preset": "left", "color": "#DCE8E0"})),
        ("home.borders", json!({"range": "E5:E7", "preset": "left", "color": "#DCE8E0"})),
        ("home.borders", json!({"range": "G5:G7", "preset": "left", "color": "#DCE8E0"})),
        ("home.alignLeft", json!({"range": "B5:H7"})),
        ("home.increaseIndent", json!({"range": "B5:H7"})),
        ("home.fontSize", json!({"range": "B5:H5", "size": 10.5})),
        ("home.fontColor", json!({"range": "B5:H5", "color": MUTED})),
        ("home.alignBottom", json!({"range": "B5:H5"})),
        ("home.fontSize", json!({"range": "B6:H6", "size": 22})),
        ("home.bold", json!({"range": "B6:H6", "on": true})),
        ("home.fontColor", json!({"range": "B6:H6", "color": INK})),
        ("home.fontSize", json!({"range": "B7:H7", "size": 10.5})),
        ("home.fontColor", json!({"range": "B7:H7", "color": ACCENT})),
        ("home.alignTop", json!({"range": "B7:H7"})),
        // The table: the soft green header and banding over its style.
        ("home.fillColor", json!({"range": "B10:H10", "color": HEAD_FILL})),
        ("home.fontColor", json!({"range": "B10:H10", "color": HEAD_INK})),
        ("home.borders", json!({"range": "B10:H10", "preset": "bottom", "style": "medium", "color": ACCENT})),
        ("home.fontColor", json!({"range": "C11:D17", "color": MUTED})),
    ]);
    for r in 11..=17 {
        let fill = if r % 2 == 0 { BAND } else { "#FFFFFF" };
        steps.push(("home.fillColor", json!({"range": format!("B{r}:H{r}"), "color": fill})));
    }
    steps.extend([
        ("home.columnWidth", json!({"cols": "A:A", "width": 30})),
        ("home.columnWidth", json!({"cols": "B:B", "width": 190})),
        ("home.columnWidth", json!({"cols": "C:C", "width": 108})),
        ("home.columnWidth", json!({"cols": "D:D", "width": 108})),
        ("home.columnWidth", json!({"cols": "E:E", "width": 120})),
        ("home.columnWidth", json!({"cols": "F:F", "width": 96})),
        ("home.columnWidth", json!({"cols": "G:G", "width": 112})),
        ("home.columnWidth", json!({"cols": "H:H", "width": 104})),
        ("home.columnWidth", json!({"cols": "I:I", "width": 34})),
        // Charts: the year against last year, the channel mix and revenue by roast.
        ("selection.set", json!({"range": "B10:B17"})),
        ("insert.chart", json!({"type": "combo", "title": "Monthly revenue, FY26 against FY25", "at": "J5", "width": 780, "height": 300})),
        ("chart.addElement", json!({"element": "legend", "position": "top"})),
        ("insert.chart", json!({"type": "doughnut", "title": "Revenue by channel", "at": "J5", "width": 382, "height": 284})),
        ("chart.set", json!({"dy": 316})),
        ("chart.addElement", json!({"element": "legend", "position": "right"})),
        ("insert.chart", json!({"type": "bar", "title": "Revenue by roast", "at": "J5", "width": 382, "height": 284})),
        ("chart.set", json!({"legend": "none", "dx": 398, "dy": 316})),
        ("view.gridlines", json!({"on": false})),
        ("sheet.tabColor", json!({"color": ACCENT})),
        ("selection.set", json!({"cell": "B2"})),
    ]);
    all(s, steps) && dashboard_charts(s) && icon_thresholds(s, "Dashboard", 0.0, 0.2)
}

/// Points the dashboard's charts at the Monthly sheet and colours them; sparklines in the accent.
fn dashboard_charts(s: &mut Session) -> bool {
    let Ok(d) = s.doc_mut() else { return false };
    let wb = Arc::make_mut(&mut d.wb);
    let Some(sh) = wb.sheet_mut(0) else { return false };
    for sp in &mut sh.sparklines {
        sp.color = Color::Rgb(0x107C41);
    }
    let months = "Monthly!$B$6:$B$17";
    let mut charts = sh.charts.iter_mut();
    if let Some(c) = charts.next() {
        c.series = vec![
            series("FY26", months, "Monthly!$F$6:$F$17", Some(ChartKind::ColumnClustered), Some(0x2E9A62)),
            series("FY25", months, "Monthly!$J$6:$J$17", Some(ChartKind::Line), Some(0xA3ACA6)),
        ];
    }
    if let Some(c) = charts.next() {
        c.series = vec![series("Monthly!$G$32", "Monthly!$F$33:$F$35", "Monthly!$G$33:$G$35", None, None)];
    }
    if let Some(c) = charts.next() {
        c.series = vec![series("Monthly!$C$32", "Monthly!$B$33:$B$36", "Monthly!$C$33:$C$36", None, Some(0x6FBF92))];
    }
    true
}
