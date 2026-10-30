//! Sample workbooks built in code (original content), for the start screen, demos, README
//! screenshots and tests.

use std::sync::Arc;

use gridcraft_model::{Sheet, Workbook};
use serde_json::json;

use crate::{DocState, Session};

mod showcase;

pub const SAMPLES: &[(&str, &str)] =
    &[("showcase", "Coffee Roastery Dashboard"), ("budget", "Household Budget"), ("sales", "Quarterly Sales Dashboard"), ("grades", "Gradebook")];

pub fn title(name: &str) -> String {
    SAMPLES.iter().find(|(n, _)| *n == name).map(|(_, t)| t.to_string()).unwrap_or_else(|| "Sample".into())
}

/// Builds a sample by replaying commands (so samples also exercise the command layer).
pub fn build(name: &str) -> Option<Workbook> {
    let mut s = Session::new();
    let mut wb = Workbook::new();
    if let Some(sh) = wb.sheet_mut(0) {
        sh.name = match name {
            "budget" => "Budget".into(),
            "sales" => "Sales".into(),
            "showcase" => "Dashboard".into(),
            "grades" => "Grades".into(),
            _ => return None,
        };
    }
    s.add_document(DocState::new(wb, None, title(name), crate::Trust::Local));
    let ok = match name {
        "budget" => budget(&mut s),
        "sales" => sales(&mut s),
        "showcase" => showcase::showcase(&mut s),
        "grades" => grades(&mut s),
        _ => false,
    };
    if !ok {
        return None;
    }
    let mut wb = (*s.doc().ok()?.wb).clone();
    wb.set_active_sheet(0);
    if let Some(sh) = wb.sheet_mut(0) {
        sh.view_active = gridcraft_core::CellRef::default();
    }
    Some(wb)
}

fn run(s: &mut Session, id: &str, p: serde_json::Value) -> bool {
    match s.execute(id, p) {
        Ok(_) => true,
        Err(e) => {
            log::warn!("sample: {id} failed: {e}");
            false
        }
    }
}

fn sales(s: &mut Session) -> bool {
    let rows = json!([
        ["Region", "Q1", "Q2", "Q3", "Q4", "Total", "Growth", "Trend"],
        ["North", 48200, 51340, 56010, 61870],
        ["South", 39150, 37980, 42400, 45120],
        ["East", 55300, 58910, 57230, 63480],
        ["West", 61020, 64400, 70150, 76390],
        ["Central", 28740, 31260, 33980, 36510],
        ["Online", 72410, 81630, 95220, 112840],
    ]);
    let mut ok = run(s, "range.setValues", json!({"range": "B4", "values": rows}));
    // A green theme: its accents colour the doughnut's slices (North … Online), darkest for the
    // largest share.
    ok &= run(
        s,
        "pageLayout.themeColors",
        json!({"colors": ["#FFFFFF", "#000000", "#E8E8E8", "#1F2321", "#A7D6BA", "#CFE8D9", "#6FBF92", "#2E9A62", "#DDE3DF", "#0E6B38", "#107C41", "#5F6661"]}),
    );
    ok &= run(s, "range.setValues", json!({"range": "B2", "values": [["Quarterly Sales Dashboard"]]}));
    ok &= run(s, "range.setValues", json!({"range": "B3", "values": [["Fiscal year 2026 · all figures in USD"]]}));
    for r in 5..=10 {
        ok &= run(s, "cell.set", json!({"cell": format!("G{r}"), "input": format!("=SUM(C{r}:F{r})")}));
        ok &= run(s, "cell.set", json!({"cell": format!("H{r}"), "input": format!("=F{r}/C{r}-1")}));
        ok &= run(s, "insert.sparkline", json!({"range": format!("C{r}:F{r}"), "location": format!("I{r}"), "type": "line"}));
    }
    ok &= run(s, "insert.table", json!({"range": "B4:I10", "style": "TableStyleLight1", "name": "Sales"}));
    ok &= run(s, "table.totalRow", json!({"table": "Sales", "on": true}));
    for col in ["Q1", "Q2", "Q3", "Q4", "Total"] {
        ok &= run(s, "table.totalFunction", json!({"table": "Sales", "column": col, "function": "sum"}));
    }
    ok &= run(s, "table.totalFunction", json!({"table": "Sales", "column": "Growth", "function": "average"}));
    ok &= run(s, "table.totalFunction", json!({"table": "Sales", "column": "Trend", "function": "none"}));
    ok &= run(s, "home.numberFormat", json!({"range": "C5:G11", "code": "\"$\"#,##0"}));
    ok &= run(s, "home.numberFormat", json!({"range": "H5:H11", "code": "0.0%"}));
    ok &= run(s, "home.conditionalFormat", json!({"range": "G5:G10", "rule": {"type": "dataBar", "color": "#CDE8D7", "gradient": false}}));
    ok &= run(s, "home.conditionalFormat", json!({"range": "H5:H10", "rule": {"type": "iconSet", "set": "3Arrows"}}));
    // The sheet's look: Inter at 12 pt on 32.5 px rows (26 on screen), a quiet title, and the table's own soft
    // green header and banding laid over its style (explicit formatting wins over a table style).
    ok &= run(s, "home.fontName", json!({"range": "A1:Q40", "name": "Inter"}));
    ok &= run(s, "home.fontSize", json!({"range": "A1:Q40", "size": 12}));
    ok &= run(s, "home.rowHeight", json!({"rows": "1:40", "height": 32.5}));
    ok &= run(s, "home.alignMiddle", json!({"range": "A1:Q40"}));
    ok &= run(s, "home.fontSize", json!({"range": "B2", "size": 19}));
    ok &= run(s, "home.bold", json!({"range": "B2", "on": true}));
    ok &= run(s, "home.fontColor", json!({"range": "B2", "color": "#1F2321"}));
    ok &= run(s, "home.fontColor", json!({"range": "B3", "color": "#7A817C"}));
    ok &= run(s, "home.fillColor", json!({"range": "B4:I4", "color": "#E6F2EB"}));
    ok &= run(s, "home.fontColor", json!({"range": "B4:I4", "color": "#0E4F2B"}));
    ok &= run(s, "home.borders", json!({"range": "B4:I4", "preset": "bottom", "style": "medium", "color": "#107C41"}));
    for r in [5, 7, 9] {
        ok &= run(s, "home.fillColor", json!({"range": format!("B{r}:I{r}"), "color": "#FFFFFF"}));
    }
    for r in [6, 8, 10] {
        ok &= run(s, "home.fillColor", json!({"range": format!("B{r}:I{r}"), "color": "#F6FAF7"}));
    }
    ok &= run(s, "home.columnWidth", json!({"cols": "A:A", "width": 30}));
    ok &= run(s, "home.columnWidth", json!({"cols": "B:B", "width": 150}));
    ok &= run(s, "home.columnWidth", json!({"cols": "C:F", "width": 120}));
    ok &= run(s, "home.columnWidth", json!({"cols": "G:G", "width": 130}));
    ok &= run(s, "home.columnWidth", json!({"cols": "H:H", "width": 115}));
    ok &= run(s, "home.columnWidth", json!({"cols": "I:I", "width": 138}));
    ok &= run(s, "selection.set", json!({"range": "B4:F10"}));
    ok &= run(s, "insert.chart", json!({"type": "column", "title": "Revenue by Region", "at": "J2", "width": 640, "height": 362}));
    ok &= run(s, "chart.addElement", json!({"element": "legend", "position": "top"}));
    // One green ramp, light to dark from Q1 to Q4.
    ok &= run(s, "chart.changeColors", json!({"colors": ["#B7DCC6", "#6FBF92", "#2E9A62", "#0E6B38"]}));
    ok &= run(s, "selection.set", json!({"range": "B4:B10"}));
    // Pie of totals.
    ok &= run(
        s,
        "insert.chart",
        json!({"range": "B4:B10", "type": "doughnut", "title": "Share of Annual Total", "at": "J14", "width": 640, "height": 285}),
    );
    ok &= run(s, "chart.addElement", json!({"element": "legend", "position": "right"}));
    if let Ok(d) = s.doc_mut() {
        let wb = Arc::make_mut(&mut d.wb);
        // Trend lines in the accent green.
        if let Some(sh) = wb.sheet_mut(0) {
            for sp in &mut sh.sparklines {
                sp.color = gridcraft_model::Color::Rgb(0x107C41);
            }
        }
        if let Some(sh) = wb.sheet_mut(0)
            && let Some(c) = sh.charts.last_mut()
        {
            c.series = vec![gridcraft_model::Series {
                name: Some("Sales!$G$4".into()),
                categories: Some("Sales!$B$5:$B$10".into()),
                values: "Sales!$G$5:$G$10".into(),
                bubble_sizes: None,
                color: None,
                secondary: false,
                kind: None,
                quartiles: Default::default(),
                smooth: false,
                trendlines: Vec::new(),
            }];
        }
    }
    ok &= run(s, "range.setValues", json!({"range": "B14", "values": [["Best region"], ["Average quarter"], ["Online share"], ["Year-over-year"]]}));
    ok &= run(s, "cell.set", json!({"cell": "C14", "input": "=INDEX(Sales[Region],MATCH(MAX(Sales[Total]),Sales[Total],0))"}));
    ok &= run(s, "cell.set", json!({"cell": "C15", "input": "=AVERAGE(C5:F10)"}));
    ok &= run(s, "cell.set", json!({"cell": "C16", "input": "=G10/SUM(G5:G10)"}));
    ok &= run(s, "cell.set", json!({"cell": "C17", "input": "=SUM(F5:F10)/SUM(C5:C10)-1"}));
    ok &= run(s, "home.numberFormat", json!({"range": "C15", "code": "\"$\"#,##0"}));
    ok &= run(s, "home.numberFormat", json!({"range": "C16:C17", "code": "0.0%"}));
    ok &= run(s, "home.fontColor", json!({"range": "B14:B17", "color": "#5F6661"}));
    ok &= run(s, "home.bold", json!({"range": "C14:C17", "on": true}));
    ok &= run(s, "home.fontColor", json!({"range": "C17", "color": "#107C41"}));
    ok &= run(s, "home.borders", json!({"range": "B14:C17", "preset": "outside", "color": "#CFD6D1"}));
    ok &= run(s, "home.borders", json!({"range": "B14:C16", "preset": "insideHorizontal", "color": "#ECEEEC"}));
    ok &= run(s, "review.newNote", json!({"cell": "H4", "text": "Q4 vs Q1 growth for each region."}));
    ok &= run(s, "selection.set", json!({"cell": "B2"}));
    ok
}

fn budget(s: &mut Session) -> bool {
    let rows = json!([
        ["Category", "Budget", "Actual", "Difference", "% Used"],
        ["Rent", 1850, 1850],
        ["Groceries", 620, 684.35],
        ["Utilities", 240, 212.8],
        ["Transport", 180, 203.5],
        ["Internet & phone", 95, 95],
        ["Insurance", 210, 210],
        ["Dining out", 200, 265.9],
        ["Entertainment", 120, 88.25],
        ["Savings", 600, 600],
        ["Art supplies", 150, 172.4],
    ]);
    let mut ok = run(s, "range.setValues", json!({"range": "B2", "values": [["Monthly Budget — October 2026"]]}));
    ok &= run(s, "home.cellStyle", json!({"range": "B2", "name": "Title"}));
    ok &= run(s, "range.setValues", json!({"range": "B4", "values": rows}));
    for r in 5..=14 {
        ok &= run(s, "cell.set", json!({"cell": format!("E{r}"), "input": format!("=C{r}-D{r}")}));
        ok &= run(s, "cell.set", json!({"cell": format!("F{r}"), "input": format!("=D{r}/C{r}")}));
    }
    ok &= run(s, "range.setValues", json!({"range": "B15", "values": [["Total"]]}));
    for c in ["C", "D", "E"] {
        ok &= run(s, "cell.set", json!({"cell": format!("{c}15"), "input": format!("=SUM({c}5:{c}14)")}));
    }
    ok &= run(s, "cell.set", json!({"cell": "F15", "input": "=D15/C15"}));
    ok &= run(s, "home.cellStyle", json!({"range": "B4:F4", "name": "Heading 3"}));
    ok &= run(s, "home.cellStyle", json!({"range": "B15:F15", "name": "Total"}));
    ok &= run(s, "home.numberFormat", json!({"range": "C5:E15", "format": "Accounting"}));
    ok &= run(s, "home.numberFormat", json!({"range": "F5:F15", "code": "0%"}));
    ok &= run(
        s,
        "home.conditionalFormat",
        json!({"range": "E5:E14", "rule": {"type": "cellIs", "operator": "less", "value": 0, "preset": "lightRedFillDarkRedText"}}),
    );
    ok &= run(s, "home.conditionalFormat", json!({"range": "F5:F14", "rule": {"type": "colorScale", "colors": ["#63BE7B", "#FFEB84", "#F8696B"]}}));
    ok &= run(s, "home.columnWidth", json!({"cols": "B:B", "width": 130}));
    ok &= run(s, "home.columnWidth", json!({"cols": "C:E", "width": 90}));
    ok &= run(s, "selection.set", json!({"range": "B4:D14"}));
    ok &= run(s, "insert.chart", json!({"type": "bar", "title": "Budget vs Actual", "at": "H3", "width": 440, "height": 300}));
    ok &= run(s, "view.freezePanes", json!({"cell": "A5"}));
    ok &= run(s, "selection.set", json!({"cell": "B2"}));
    ok
}

fn grades(s: &mut Session) -> bool {
    let rows = json!([
        ["Student", "Essay", "Midterm", "Project", "Final", "Average", "Grade"],
        ["Amara Okafor", 92, 88, 95, 91],
        ["Bruno Silva", 78, 82, 74, 80],
        ["Chen Wei", 85, 91, 89, 94],
        ["Dana Kowalski", 67, 72, 80, 70],
        ["Elif Yılmaz", 98, 95, 97, 99],
        ["Farid Haddad", 74, 69, 71, 77],
        ["Grace Mensah", 88, 84, 90, 86],
        ["Hiro Tanaka", 59, 64, 70, 62],
    ]);
    let mut ok = run(s, "range.setValues", json!({"range": "A1", "values": rows}));
    for r in 2..=9 {
        ok &= run(s, "cell.set", json!({"cell": format!("F{r}"), "input": format!("=AVERAGE(B{r}:E{r})")}));
        ok &= run(
            s,
            "cell.set",
            json!({"cell": format!("G{r}"), "input": format!("=IFS(F{r}>=90,\"A\",F{r}>=80,\"B\",F{r}>=70,\"C\",F{r}>=60,\"D\",TRUE,\"F\")")}),
        );
    }
    ok &= run(s, "home.numberFormat", json!({"range": "F2:F9", "code": "0.0"}));
    ok &= run(s, "insert.table", json!({"range": "A1:G9", "style": "TableStyleLight9", "name": "Grades"}));
    ok &= run(s, "home.columnWidth", json!({"cols": "A:A", "width": 120}));
    ok &= run(
        s,
        "home.conditionalFormat",
        json!({"range": "G2:G9", "rule": {"type": "cellIs", "operator": "equal", "value": "A", "preset": "greenFillDarkGreenText"}}),
    );
    ok &= run(s, "home.conditionalFormat", json!({"range": "F2:F9", "rule": {"type": "dataBar", "color": "#63BE7B"}}));
    ok
}

/// A blank sheet (for callers that need one without a session).
pub fn blank_sheet(name: &str) -> Sheet {
    Sheet::new(name)
}
