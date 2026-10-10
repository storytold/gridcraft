//! Sample workbooks built in code (original content), for the start screen, demos, README
//! screenshots and tests.

use std::sync::Arc;

use gridcraft_model::{Sheet, Workbook};
use serde_json::json;

use crate::lang::Lang;
use crate::{DocState, Session};

pub const SAMPLES: &[(&str, &str)] = &[("budget", "Household Budget"), ("sales", "Quarterly Sales Dashboard"), ("grades", "Gradebook")];

pub fn title(name: &str) -> String {
    title_in(name, Lang::English)
}

/// A sample's title in `lang` (the German samples are German workbooks).
pub fn title_in(name: &str, lang: Lang) -> String {
    let de = match name {
        "budget" => "Haushaltsbudget",
        "sales" => "Quartalsumsätze",
        "grades" => "Notenliste",
        _ => "Beispiel",
    };
    match lang {
        Lang::English => SAMPLES.iter().find(|(n, _)| *n == name).map(|(_, t)| t.to_string()).unwrap_or_else(|| "Sample".into()),
        Lang::German => de.into(),
    }
}

/// Builds a sample by replaying commands (so samples also exercise the command layer).
pub fn build(name: &str) -> Option<Workbook> {
    build_in(name, Lang::English)
}

/// [`build`] in `lang`: German samples are our own German text in the same layout, with euro
/// amounts. Number format codes are stored as in Excel files, whatever the language.
pub fn build_in(name: &str, lang: Lang) -> Option<Workbook> {
    let l = |en, de| lang.pick(en, de);
    let mut s = Session::new();
    // Generated cell text (a table's totals label) follows the sample's language.
    s.lang = lang;
    let mut wb = Workbook::new();
    if let Some(sh) = wb.sheet_mut(0) {
        sh.name = match name {
            "budget" => "Budget".into(),
            "sales" => l("Sales", "Umsatz").into(),
            "grades" => l("Grades", "Noten").into(),
            _ => return None,
        };
    }
    s.add_document(DocState::new(wb, None, title(name)));
    let ok = match name {
        "budget" => budget(&mut s, lang),
        "sales" => sales(&mut s, lang),
        "grades" => grades(&mut s, lang),
        _ => false,
    };
    if !ok {
        return None;
    }
    let mut wb = (*s.doc().ok()?.wb).clone();
    wb.active_sheet = 0;
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

fn sales(s: &mut Session, lang: Lang) -> bool {
    let l = |en, de| lang.pick(en, de);
    let (table, sheet) = (l("Sales", "Umsatz"), l("Sales", "Umsatz"));
    let (total, growth, trend) = (l("Total", "Gesamt"), l("Growth", "Wachstum"), "Trend");
    let rows = json!([
        [l("Region", "Region"), "Q1", "Q2", "Q3", "Q4", total, growth, trend],
        [l("North", "Nord"), 48200, 51340, 56010, 61870],
        [l("South", "Süd"), 39150, 37980, 42400, 45120],
        [l("East", "Ost"), 55300, 58910, 57230, 63480],
        [l("West", "West"), 61020, 64400, 70150, 76390],
        [l("Central", "Mitte"), 28740, 31260, 33980, 36510],
        ["Online", 72410, 81630, 95220, 112840],
    ]);
    let money = l("\"$\"#,##0", "#,##0 \"€\"");
    let mut ok = run(s, "range.setValues", json!({"range": "B4", "values": rows}));
    ok &= run(s, "range.setValues", json!({"range": "B2", "values": [[l("Quarterly Sales Dashboard", "Quartalsumsätze")]]}));
    ok &= run(s, "home.cellStyle", json!({"range": "B2", "name": "Title"}));
    ok &= run(
        s,
        "range.setValues",
        json!({"range": "B3", "values": [[l("Fiscal year 2026 · all figures in USD", "Geschäftsjahr 2026 · alle Angaben in EUR")]]}),
    );
    ok &= run(s, "home.cellStyle", json!({"range": "B3", "name": "Explanatory Text"}));
    for r in 5..=10 {
        ok &= run(s, "cell.set", json!({"cell": format!("G{r}"), "input": format!("=SUM(C{r}:F{r})")}));
        ok &= run(s, "cell.set", json!({"cell": format!("H{r}"), "input": format!("=F{r}/C{r}-1")}));
        ok &= run(s, "insert.sparkline", json!({"range": format!("C{r}:F{r}"), "location": format!("I{r}"), "type": "line", "markers": true}));
    }
    ok &= run(s, "insert.table", json!({"range": "B4:I10", "style": "TableStyleMedium2", "name": table}));
    ok &= run(s, "table.totalRow", json!({"table": table, "on": true}));
    for col in ["Q1", "Q2", "Q3", "Q4", total] {
        ok &= run(s, "table.totalFunction", json!({"table": table, "column": col, "function": "sum"}));
    }
    ok &= run(s, "table.totalFunction", json!({"table": table, "column": growth, "function": "average"}));
    ok &= run(s, "table.totalFunction", json!({"table": table, "column": trend, "function": "none"}));
    ok &= run(s, "home.numberFormat", json!({"range": "C5:G11", "code": money}));
    ok &= run(s, "home.numberFormat", json!({"range": "H5:H11", "code": "0.0%"}));
    ok &= run(s, "home.conditionalFormat", json!({"range": "G5:G10", "rule": {"type": "dataBar", "color": "#5B9BD5"}}));
    ok &= run(s, "home.conditionalFormat", json!({"range": "H5:H10", "rule": {"type": "iconSet", "set": "3Arrows"}}));
    ok &= run(s, "home.columnWidth", json!({"cols": "A:A", "width": 20}));
    // German labels are longer.
    let (label_width, growth_width) = match lang {
        Lang::English => (132, 76),
        Lang::German => (176, 92),
    };
    ok &= run(s, "home.columnWidth", json!({"cols": "B:B", "width": label_width}));
    ok &= run(s, "home.columnWidth", json!({"cols": "C:G", "width": 84}));
    ok &= run(s, "home.columnWidth", json!({"cols": "H:I", "width": growth_width}));
    ok &= run(s, "selection.set", json!({"range": "B4:F10"}));
    ok &= run(
        s,
        "insert.chart",
        json!({"type": "column", "title": l("Revenue by Region", "Umsatz nach Region"), "at": "K3", "width": 460, "height": 270}),
    );
    ok &= run(s, "selection.set", json!({"range": "B4:B10"}));
    // Pie of totals.
    let pie_title = l("Share of Annual Total", "Anteil am Jahresumsatz");
    ok &= run(s, "insert.chart", json!({"range": "B4:B10", "type": "pie", "title": pie_title, "at": "K19", "width": 460, "height": 260}));
    if let Ok(d) = s.doc_mut() {
        let wb = Arc::make_mut(&mut d.wb);
        if let Some(sh) = wb.sheet_mut(0)
            && let Some(c) = sh.charts.last_mut()
        {
            c.series = vec![gridcraft_model::Series {
                name: Some(format!("{sheet}!$G$4")),
                categories: Some(format!("{sheet}!$B$5:$B$10")),
                values: format!("{sheet}!$G$5:$G$10"),
                bubble_sizes: None,
                color: None,
                secondary: false,
                kind: None,
            }];
        }
    }
    let labels = match lang {
        Lang::English => json!([["Best region"], ["Average quarter"], ["Online share"], ["Year-over-year"]]),
        Lang::German => json!([["Stärkste Region"], ["Durchschnitt je Quartal"], ["Online-Anteil"], ["Veränderung zum Vorjahr"]]),
    };
    ok &= run(s, "range.setValues", json!({"range": "B14", "values": labels}));
    ok &= run(s, "cell.set", json!({"cell": "C14", "input": format!("=INDEX({table}[Region],MATCH(MAX({table}[{total}]),{table}[{total}],0))")}));
    ok &= run(s, "cell.set", json!({"cell": "C15", "input": "=AVERAGE(C5:F10)"}));
    ok &= run(s, "cell.set", json!({"cell": "C16", "input": "=G10/SUM(G5:G10)"}));
    ok &= run(s, "cell.set", json!({"cell": "C17", "input": "=SUM(F5:F10)/SUM(C5:C10)-1"}));
    ok &= run(s, "home.numberFormat", json!({"range": "C15", "code": money}));
    ok &= run(s, "home.numberFormat", json!({"range": "C16:C17", "code": "0.0%"}));
    ok &= run(s, "home.bold", json!({"range": "B14:B17", "on": true}));
    ok &= run(s, "home.borders", json!({"range": "B14:C17", "preset": "outside"}));
    ok &= run(s, "home.fillColor", json!({"range": "B14:C17", "color": "#F2F7FC"}));
    ok &= run(s, "review.newNote", json!({"cell": "H4", "text": l("Q4 vs Q1 growth for each region.", "Wachstum von Q1 bis Q4 je Region.")}));
    ok &= run(s, "selection.set", json!({"cell": "B2"}));
    ok
}

fn budget(s: &mut Session, lang: Lang) -> bool {
    let l = |en, de| lang.pick(en, de);
    let rows = json!([
        [l("Category", "Kategorie"), "Budget", l("Actual", "Ist"), l("Difference", "Differenz"), l("% Used", "% verbraucht")],
        [l("Rent", "Miete"), 1850, 1850],
        [l("Groceries", "Lebensmittel"), 620, 684.35],
        [l("Utilities", "Nebenkosten"), 240, 212.8],
        [l("Transport", "Mobilität"), 180, 203.5],
        [l("Internet & phone", "Internet & Telefon"), 95, 95],
        [l("Insurance", "Versicherungen"), 210, 210],
        [l("Dining out", "Essen gehen"), 200, 265.9],
        [l("Entertainment", "Freizeit"), 120, 88.25],
        [l("Savings", "Sparen"), 600, 600],
        [l("Art supplies", "Künstlerbedarf"), 150, 172.4],
    ]);
    let mut ok = run(s, "range.setValues", json!({"range": "B2", "values": [[l("Monthly Budget — October 2026", "Monatsbudget – Oktober 2026")]]}));
    ok &= run(s, "home.cellStyle", json!({"range": "B2", "name": "Title"}));
    ok &= run(s, "range.setValues", json!({"range": "B4", "values": rows}));
    for r in 5..=14 {
        ok &= run(s, "cell.set", json!({"cell": format!("E{r}"), "input": format!("=C{r}-D{r}")}));
        ok &= run(s, "cell.set", json!({"cell": format!("F{r}"), "input": format!("=D{r}/C{r}")}));
    }
    ok &= run(s, "range.setValues", json!({"range": "B15", "values": [[l("Total", "Summe")]]}));
    for c in ["C", "D", "E"] {
        ok &= run(s, "cell.set", json!({"cell": format!("{c}15"), "input": format!("=SUM({c}5:{c}14)")}));
    }
    ok &= run(s, "cell.set", json!({"cell": "F15", "input": "=D15/C15"}));
    ok &= run(s, "home.cellStyle", json!({"range": "B4:F4", "name": "Heading 3"}));
    ok &= run(s, "home.cellStyle", json!({"range": "B15:F15", "name": "Total"}));
    ok &= match lang {
        Lang::English => run(s, "home.numberFormat", json!({"range": "C5:E15", "format": "Accounting"})),
        Lang::German => run(s, "home.numberFormat", json!({"range": "C5:E15", "code": "#,##0.00 \"€\""})),
    };
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
    ok &= run(s, "insert.chart", json!({"type": "bar", "title": l("Budget vs Actual", "Budget und Ist"), "at": "H3", "width": 440, "height": 300}));
    ok &= run(s, "view.freezePanes", json!({"cell": "A5"}));
    ok &= run(s, "selection.set", json!({"cell": "B2"}));
    ok
}

fn grades(s: &mut Session, lang: Lang) -> bool {
    let l = |en, de| lang.pick(en, de);
    let header = match lang {
        Lang::English => json!(["Student", "Essay", "Midterm", "Project", "Final", "Average", "Grade"]),
        Lang::German => json!(["Name", "Aufsatz", "Zwischentest", "Projekt", "Abschlusstest", "Durchschnitt", "Note"]),
    };
    let rows = json!([
        header,
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
        // Letter grades in English; German school grades 1 (best) to 5 on the same bands.
        let grade = match lang {
            Lang::English => format!("=IFS(F{r}>=90,\"A\",F{r}>=80,\"B\",F{r}>=70,\"C\",F{r}>=60,\"D\",TRUE,\"F\")"),
            Lang::German => format!("=IFS(F{r}>=90,1,F{r}>=80,2,F{r}>=70,3,F{r}>=60,4,TRUE,5)"),
        };
        ok &= run(s, "cell.set", json!({"cell": format!("G{r}"), "input": grade}));
    }
    ok &= run(s, "home.numberFormat", json!({"range": "F2:F9", "code": "0.0"}));
    ok &= run(s, "insert.table", json!({"range": "A1:G9", "style": "TableStyleLight9", "name": l("Grades", "Noten")}));
    ok &= run(s, "home.columnWidth", json!({"cols": "A:A", "width": 120}));
    ok &= run(
        s,
        "home.conditionalFormat",
        json!({"range": "G2:G9", "rule": {"type": "cellIs", "operator": "equal", "value": match lang { Lang::English => json!("A"), Lang::German => json!(1) }, "preset": "greenFillDarkGreenText"}}),
    );
    ok &= run(s, "home.conditionalFormat", json!({"range": "F2:F9", "rule": {"type": "dataBar", "color": "#63BE7B"}}));
    ok
}

/// A blank sheet (for callers that need one without a session).
pub fn blank_sheet(name: &str) -> Sheet {
    Sheet::new(name)
}
