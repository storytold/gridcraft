//! Sample workbooks built in code (original content), for the start screen, demos, README
//! screenshots and tests.

use std::sync::Arc;

use gridcraft_model::{Sheet, Workbook};
use serde_json::json;

use crate::{DocState, Session};

pub const SAMPLES: &[(&str, &str)] = &[("budget", "Household Budget"), ("sales", "Quarterly Sales Dashboard"), ("grades", "Gradebook")];

pub fn title(name: &str) -> String {
    title_in(name, "en-US")
}

/// The sample's title in an interface language (Georgian for `ka`, English otherwise).
pub fn title_in(name: &str, lang: &str) -> String {
    SAMPLES.iter().find(|(n, _)| *n == name).map(|(_, t)| Text::of(lang).tr(t).to_string()).unwrap_or_else(|| "Sample".into())
}

/// Sample text in one language: English is the source, other languages look it up.
#[derive(Clone, Copy)]
struct Text(&'static [(&'static str, &'static str)]);

impl Text {
    fn of(lang: &str) -> Text {
        Text(if lang.get(..2).is_some_and(|l| l.eq_ignore_ascii_case("ka")) { KA } else { &[] })
    }
    fn tr(self, en: &'static str) -> &'static str {
        self.0.iter().find(|(e, _)| *e == en).map_or(en, |(_, t)| t)
    }
    fn is_english(self) -> bool {
        self.0.is_empty()
    }
}

/// Georgian sample text (original, reviewed with the interface catalog's glossary).
const KA: &[(&str, &str)] = &[
    ("Household Budget", "ოჯახის ბიუჯეტი"),
    ("Quarterly Sales Dashboard", "კვარტალური გაყიდვების პანელი"),
    ("Gradebook", "შეფასებების ჟურნალი"),
    ("Budget", "ბიუჯეტი"),
    ("Sales", "გაყიდვები"),
    ("Grades", "შეფასებები"),
    ("Region", "რეგიონი"),
    ("Total", "სულ"),
    ("Growth", "ზრდა"),
    ("Trend", "ტენდენცია"),
    ("North", "ჩრდილოეთი"),
    ("South", "სამხრეთი"),
    ("East", "აღმოსავლეთი"),
    ("West", "დასავლეთი"),
    ("Central", "ცენტრი"),
    ("Online", "ონლაინ"),
    ("Fiscal year 2026 · all figures in USD", "2026 საფინანსო წელი · ყველა თანხა ლარშია"),
    ("Revenue by Region", "შემოსავალი რეგიონების მიხედვით"),
    ("Share of Annual Total", "წლიური ჯამის წილი"),
    ("Best region", "საუკეთესო რეგიონი"),
    ("Average quarter", "საშუალო კვარტალი"),
    ("Online share", "ონლაინის წილი"),
    ("Year-over-year", "წლიური ზრდა"),
    ("Q4 vs Q1 growth for each region.", "მე-4 კვარტლის ზრდა 1-ელთან შედარებით, თითოეული რეგიონისთვის."),
    ("Category", "კატეგორია"),
    ("Actual", "ფაქტობრივი"),
    ("Difference", "სხვაობა"),
    ("% Used", "% გამოყენებული"),
    ("Rent", "ქირა"),
    ("Groceries", "სურსათი"),
    ("Utilities", "კომუნალური"),
    ("Transport", "ტრანსპორტი"),
    ("Internet & phone", "ინტერნეტი და ტელეფონი"),
    ("Insurance", "დაზღვევა"),
    ("Dining out", "კაფე-რესტორანი"),
    ("Entertainment", "გართობა"),
    ("Savings", "დანაზოგი"),
    ("Art supplies", "სახატავი მასალები"),
    ("Monthly Budget — October 2026", "თვის ბიუჯეტი — ოქტომბერი 2026"),
    ("Budget vs Actual", "ბიუჯეტი და ფაქტობრივი"),
    ("Student", "სტუდენტი"),
    ("Essay", "ესე"),
    ("Midterm", "შუალედური"),
    ("Project", "პროექტი"),
    ("Final", "დასკვნითი"),
    ("Average", "საშუალო"),
    ("Grade", "შეფასება"),
    ("Amara Okafor", "ანა ბერიძე"),
    ("Bruno Silva", "ბექა ლომიძე"),
    ("Chen Wei", "გიორგი ჩხეიძე"),
    ("Dana Kowalski", "დარეჯან კაპანაძე"),
    ("Elif Yılmaz", "ელენე გოგიჩაიშვილი"),
    ("Farid Haddad", "ზურაბ ხარაიშვილი"),
    ("Grace Mensah", "თამარ მელაძე"),
    ("Hiro Tanaka", "ირაკლი ნოზაძე"),
];

/// Builds a sample by replaying commands (so samples also exercise the command layer).
pub fn build(name: &str) -> Option<Workbook> {
    build_in(name, "en-US")
}

/// [`build`] in an interface language: sheet, headers, labels, chart titles and the note are in
/// that language (Georgian for `ka`); formulas stay canonical and follow the translated headers.
pub fn build_in(name: &str, lang: &str) -> Option<Workbook> {
    let t = Text::of(lang);
    let mut s = Session::new();
    let mut wb = Workbook::new();
    if let Some(sh) = wb.sheet_mut(0) {
        sh.name = match name {
            "budget" => t.tr("Budget").into(),
            "sales" => t.tr("Sales").into(),
            "grades" => t.tr("Grades").into(),
            _ => return None,
        };
    }
    s.add_document(DocState::new(wb, None, title_in(name, lang)));
    let ok = match name {
        "budget" => budget(&mut s, t),
        "sales" => sales(&mut s, t),
        "grades" => grades(&mut s, t),
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

/// A column width for the sample's language: Georgian labels and the `₾` suffix run about 30% wider.
fn wide(t: Text, px: u32) -> u32 {
    if t.is_english() { px } else { px * 13 / 10 }
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

fn sales(s: &mut Session, t: Text) -> bool {
    let rows = json!([
        [t.tr("Region"), t.tr("Q1"), t.tr("Q2"), t.tr("Q3"), t.tr("Q4"), t.tr("Total"), t.tr("Growth"), t.tr("Trend")],
        [t.tr("North"), 48200, 51340, 56010, 61870],
        [t.tr("South"), 39150, 37980, 42400, 45120],
        [t.tr("East"), 55300, 58910, 57230, 63480],
        [t.tr("West"), 61020, 64400, 70150, 76390],
        [t.tr("Central"), 28740, 31260, 33980, 36510],
        [t.tr("Online"), 72410, 81630, 95220, 112840],
    ]);
    let mut ok = run(s, "range.setValues", json!({"range": "B4", "values": rows}));
    ok &= run(s, "range.setValues", json!({"range": "B2", "values": [[t.tr("Quarterly Sales Dashboard")]]}));
    ok &= run(s, "home.cellStyle", json!({"range": "B2", "name": "Title"}));
    ok &= run(s, "range.setValues", json!({"range": "B3", "values": [[t.tr("Fiscal year 2026 · all figures in USD")]]}));
    ok &= run(s, "home.cellStyle", json!({"range": "B3", "name": "Explanatory Text"}));
    for r in 5..=10 {
        ok &= run(s, "cell.set", json!({"cell": format!("G{r}"), "input": format!("=SUM(C{r}:F{r})")}));
        ok &= run(s, "cell.set", json!({"cell": format!("H{r}"), "input": format!("=F{r}/C{r}-1")}));
        ok &= run(s, "insert.sparkline", json!({"range": format!("C{r}:F{r}"), "location": format!("I{r}"), "type": "line", "markers": true}));
    }
    ok &= run(s, "insert.table", json!({"range": "B4:I10", "style": "TableStyleMedium2", "name": "Sales"}));
    ok &= run(s, "table.totalRow", json!({"table": "Sales", "on": true}));
    for col in ["Q1", "Q2", "Q3", "Q4", "Total"] {
        ok &= run(s, "table.totalFunction", json!({"table": "Sales", "column": t.tr(col), "function": "sum"}));
    }
    ok &= run(s, "table.totalFunction", json!({"table": "Sales", "column": t.tr("Growth"), "function": "average"}));
    ok &= run(s, "table.totalFunction", json!({"table": "Sales", "column": t.tr("Trend"), "function": "none"}));
    if !t.is_english() {
        // The engine labels a total row in English; the sample's own label follows its language.
        ok &= run(s, "range.setValues", json!({"range": "B11", "values": [[t.tr("Total")]]}));
    }
    // Figures in the region's currency: dollars in English, lari in Georgian.
    let money = if t.is_english() { "\"$\"#,##0" } else { "#,##0 \"₾\"" };
    ok &= run(s, "home.numberFormat", json!({"range": "C5:G11", "code": money}));
    ok &= run(s, "home.numberFormat", json!({"range": "H5:H11", "code": "0.0%"}));
    ok &= run(s, "home.conditionalFormat", json!({"range": "G5:G10", "rule": {"type": "dataBar", "color": "#5B9BD5"}}));
    ok &= run(s, "home.conditionalFormat", json!({"range": "H5:H10", "rule": {"type": "iconSet", "set": "3Arrows"}}));
    ok &= run(s, "home.columnWidth", json!({"cols": "A:A", "width": 20}));
    ok &= run(s, "home.columnWidth", json!({"cols": "B:B", "width": wide(t, 132)}));
    ok &= run(s, "home.columnWidth", json!({"cols": "C:G", "width": wide(t, 84)}));
    ok &= run(s, "home.columnWidth", json!({"cols": "H:I", "width": wide(t, 76)}));
    ok &= run(s, "selection.set", json!({"range": "B4:F10"}));
    ok &= run(s, "insert.chart", json!({"type": "column", "title": t.tr("Revenue by Region"), "at": "K3", "width": 460, "height": 270}));
    ok &= run(s, "selection.set", json!({"range": "B4:B10"}));
    // Pie of totals.
    ok &= run(
        s,
        "insert.chart",
        json!({"range": "B4:B10", "type": "pie", "title": t.tr("Share of Annual Total"), "at": "K19", "width": 460, "height": 260}),
    );
    if let Ok(d) = s.doc_mut() {
        let wb = Arc::make_mut(&mut d.wb);
        if let Some(sh) = wb.sheet_mut(0)
            && let Some(c) = sh.charts.last_mut()
        {
            let sheet = if t.is_english() { "Sales".to_string() } else { format!("'{}'", t.tr("Sales")) };
            c.series = vec![gridcraft_model::Series {
                name: Some(format!("{sheet}!$G$4")),
                categories: Some(format!("{sheet}!$B$5:$B$10")),
                values: format!("{sheet}!$G$5:$G$10"),
                bubble_sizes: None,
                color: None,
                secondary: false,
                kind: None,
                smooth: false,
                trendlines: Vec::new(),
            }];
        }
    }
    ok &= run(
        s,
        "range.setValues",
        json!({"range": "B14", "values": [[t.tr("Best region")], [t.tr("Average quarter")], [t.tr("Online share")], [t.tr("Year-over-year")]]}),
    );
    ok &= run(
        s,
        "cell.set",
        json!({"cell": "C14", "input": format!("=INDEX(Sales[{r}],MATCH(MAX(Sales[{tot}]),Sales[{tot}],0))", r = t.tr("Region"), tot = t.tr("Total"))}),
    );
    ok &= run(s, "cell.set", json!({"cell": "C15", "input": "=AVERAGE(C5:F10)"}));
    ok &= run(s, "cell.set", json!({"cell": "C16", "input": "=G10/SUM(G5:G10)"}));
    ok &= run(s, "cell.set", json!({"cell": "C17", "input": "=SUM(F5:F10)/SUM(C5:C10)-1"}));
    ok &= run(s, "home.numberFormat", json!({"range": "C15", "code": money}));
    ok &= run(s, "home.numberFormat", json!({"range": "C16:C17", "code": "0.0%"}));
    ok &= run(s, "home.bold", json!({"range": "B14:B17", "on": true}));
    ok &= run(s, "home.borders", json!({"range": "B14:C17", "preset": "outside"}));
    ok &= run(s, "home.fillColor", json!({"range": "B14:C17", "color": "#F2F7FC"}));
    ok &= run(s, "review.newNote", json!({"cell": "H4", "text": t.tr("Q4 vs Q1 growth for each region.")}));
    ok &= run(s, "selection.set", json!({"cell": "B2"}));
    ok
}

fn budget(s: &mut Session, t: Text) -> bool {
    let rows = json!([
        [t.tr("Category"), t.tr("Budget"), t.tr("Actual"), t.tr("Difference"), t.tr("% Used")],
        [t.tr("Rent"), 1850, 1850],
        [t.tr("Groceries"), 620, 684.35],
        [t.tr("Utilities"), 240, 212.8],
        [t.tr("Transport"), 180, 203.5],
        [t.tr("Internet & phone"), 95, 95],
        [t.tr("Insurance"), 210, 210],
        [t.tr("Dining out"), 200, 265.9],
        [t.tr("Entertainment"), 120, 88.25],
        [t.tr("Savings"), 600, 600],
        [t.tr("Art supplies"), 150, 172.4],
    ]);
    let mut ok = run(s, "range.setValues", json!({"range": "B2", "values": [[t.tr("Monthly Budget — October 2026")]]}));
    ok &= run(s, "home.cellStyle", json!({"range": "B2", "name": "Title"}));
    ok &= run(s, "range.setValues", json!({"range": "B4", "values": rows}));
    for r in 5..=14 {
        ok &= run(s, "cell.set", json!({"cell": format!("E{r}"), "input": format!("=C{r}-D{r}")}));
        ok &= run(s, "cell.set", json!({"cell": format!("F{r}"), "input": format!("=D{r}/C{r}")}));
    }
    ok &= run(s, "range.setValues", json!({"range": "B15", "values": [[t.tr("Total")]]}));
    for c in ["C", "D", "E"] {
        ok &= run(s, "cell.set", json!({"cell": format!("{c}15"), "input": format!("=SUM({c}5:{c}14)")}));
    }
    ok &= run(s, "cell.set", json!({"cell": "F15", "input": "=D15/C15"}));
    ok &= run(s, "home.cellStyle", json!({"range": "B4:F4", "name": "Heading 3"}));
    ok &= run(s, "home.cellStyle", json!({"range": "B15:F15", "name": "Total"}));
    // Accounting is a dollar format; the Georgian budget is in lari.
    ok &= if t.is_english() {
        run(s, "home.numberFormat", json!({"range": "C5:E15", "format": "Accounting"}))
    } else {
        run(s, "home.numberFormat", json!({"range": "C5:E15", "code": "#,##0.00 \"₾\";[Red]-#,##0.00 \"₾\""}))
    };
    ok &= run(s, "home.numberFormat", json!({"range": "F5:F15", "code": "0%"}));
    ok &= run(
        s,
        "home.conditionalFormat",
        json!({"range": "E5:E14", "rule": {"type": "cellIs", "operator": "less", "value": 0, "preset": "lightRedFillDarkRedText"}}),
    );
    ok &= run(s, "home.conditionalFormat", json!({"range": "F5:F14", "rule": {"type": "colorScale", "colors": ["#63BE7B", "#FFEB84", "#F8696B"]}}));
    ok &= run(s, "home.columnWidth", json!({"cols": "B:B", "width": wide(t, 130)}));
    ok &= run(s, "home.columnWidth", json!({"cols": "C:E", "width": wide(t, 90)}));
    ok &= run(s, "selection.set", json!({"range": "B4:D14"}));
    ok &= run(s, "insert.chart", json!({"type": "bar", "title": t.tr("Budget vs Actual"), "at": "H3", "width": 440, "height": 300}));
    ok &= run(s, "view.freezePanes", json!({"cell": "A5"}));
    ok &= run(s, "selection.set", json!({"cell": "B2"}));
    ok
}

fn grades(s: &mut Session, t: Text) -> bool {
    let rows = json!([
        [t.tr("Student"), t.tr("Essay"), t.tr("Midterm"), t.tr("Project"), t.tr("Final"), t.tr("Average"), t.tr("Grade")],
        [t.tr("Amara Okafor"), 92, 88, 95, 91],
        [t.tr("Bruno Silva"), 78, 82, 74, 80],
        [t.tr("Chen Wei"), 85, 91, 89, 94],
        [t.tr("Dana Kowalski"), 67, 72, 80, 70],
        [t.tr("Elif Yılmaz"), 98, 95, 97, 99],
        [t.tr("Farid Haddad"), 74, 69, 71, 77],
        [t.tr("Grace Mensah"), 88, 84, 90, 86],
        [t.tr("Hiro Tanaka"), 59, 64, 70, 62],
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
    ok &= run(s, "home.columnWidth", json!({"cols": "A:A", "width": wide(t, 120)}));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_follow_the_interface_language() {
        for (name, _) in SAMPLES {
            let en = build_in(name, "en-US").unwrap_or_else(|| panic!("{name} en"));
            let ka = build_in(name, "ka-GE").unwrap_or_else(|| panic!("{name} ka"));
            let (e, k) = (en.sheet(0).map(|s| s.name.clone()), ka.sheet(0).map(|s| s.name.clone()));
            assert_ne!(e, k, "{name}: the Georgian sample keeps the English sheet name");
            assert!(k.is_some_and(|n| n.chars().any(|c| ('\u{10D0}'..='\u{10FF}').contains(&c))), "{name}");
        }
        // Formulas follow the translated headers: the Georgian structured reference still finds the
        // best region, and the summary still calculates.
        let ka = build_in("sales", "ka-GE").unwrap_or_else(|| panic!("sales ka"));
        let sheet = ka.sheet(0).unwrap_or_else(|| panic!("sheet"));
        let at = |a: &str| sheet.value(gridcraft_core::CellRef::parse(a).unwrap_or_else(|| panic!("{a}")));
        assert_eq!(at("C14"), gridcraft_core::Value::text("ონლაინ"));
        assert!(matches!(at("C17"), gridcraft_core::Value::Number(_)), "{:?}", at("C17"));
        assert_eq!(at("B11"), gridcraft_core::Value::text("სულ"), "the total row's label follows the language");
        assert_eq!(title_in("sales", "ka"), "კვარტალური გაყიდვების პანელი");
        assert_eq!(title_in("sales", "uk-UA"), "Quarterly Sales Dashboard");
    }
}
