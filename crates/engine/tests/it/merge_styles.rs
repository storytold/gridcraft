//! Home › Cell Styles › Merge Styles (`home.mergeStyles`): another open workbook's cell styles
//! merged into the active one, built as in the measurement of Excel's `Styles.Merge`.

use gridcraft_engine::cmd::format::{merge_cell_styles, named_cell_style};
use gridcraft_engine::model::{Color, Style, Theme, Underline};
use gridcraft_engine::{DocState, Session, Trust, io};
use serde_json::{Value as Json, json};

fn run(s: &mut Session, id: &str, p: Json) -> Json {
    s.execute(id, p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

/// Formats `cell` with `steps` and saves its formats as the cell style `name`.
fn make_style(s: &mut Session, name: &str, cell: &str, steps: &[(&str, Json)]) {
    run(s, "cell.set", json!({"cell": cell, "input": "1"}));
    for (id, p) in steps {
        let mut p = p.clone();
        p["range"] = json!(cell);
        run(s, id, p);
    }
    run(s, "home.newCellStyle", json!({"name": name, "fromCell": cell}));
}

/// The active workbook's own definition of style `name`, if it has one.
fn own(s: &Session, name: &str) -> Option<Style> {
    s.doc().unwrap().wb.cell_styles.iter().find(|(n, _)| n == name).map(|(_, st)| st.clone())
}

fn names(s: &Session) -> Vec<String> {
    s.doc().unwrap().wb.cell_styles.iter().map(|(n, _)| n.clone()).collect()
}

fn strings(v: &Json) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

/// Book1, the target: custom styles Shared (double underline, `0`) and Target Only (9 pt), Bad
/// changed to 16 pt; A1 = 0.5 in Shared, A2 = 1, A3 = 2 in Good, A4 = 3 in Bad, B1 bold.
/// Book2, the source: Src Only (bold, red, yellow fill, `0%`), Shared (italic, `0%`), NumOnly
/// (only the number format `@`), Good changed to 20 pt. Book1 is active.
fn two_books() -> (Session, usize, usize) {
    let mut s = Session::new();
    let target = s.new_workbook();
    make_style(&mut s, "Shared", "D1", &[("home.doubleUnderline", json!({})), ("home.numberFormat", json!({"code": "0"}))]);
    make_style(&mut s, "Target Only", "D2", &[("home.fontSize", json!({"size": 9}))]);
    make_style(&mut s, "Bad", "D3", &[("home.cellStyle", json!({"name": "Bad"})), ("home.fontSize", json!({"size": 16}))]);
    run(&mut s, "edit.clearAll", json!({"range": "D1:D3"}));
    run(&mut s, "range.setValues", json!({"range": "A1", "values": [[0.5], [1], [2], [3]]}));
    run(&mut s, "home.cellStyle", json!({"range": "A1", "name": "Shared"}));
    run(&mut s, "home.cellStyle", json!({"range": "A3", "name": "Good"}));
    run(&mut s, "home.cellStyle", json!({"range": "A4", "name": "Bad"}));
    run(&mut s, "home.bold", json!({"range": "B1"}));
    let source = s.new_workbook();
    make_style(
        &mut s,
        "Src Only",
        "A1",
        &[
            ("home.bold", json!({})),
            ("home.fontColor", json!({"color": "#FF0000"})),
            ("home.fillColor", json!({"color": "#FFFF00"})),
            ("home.numberFormat", json!({"code": "0%"})),
        ],
    );
    make_style(&mut s, "Shared", "A2", &[("home.italic", json!({})), ("home.numberFormat", json!({"code": "0%"}))]);
    make_style(&mut s, "NumOnly", "A3", &[("home.numberFormat", json!({"code": "@"}))]);
    make_style(&mut s, "Good", "A4", &[("home.cellStyle", json!({"name": "Good"})), ("home.fontSize", json!({"size": 20}))]);
    s.set_active(target);
    assert_eq!(s.doc().unwrap().display_title(), "Book1");
    (s, target, source)
}

fn cell_style(s: &Session, doc: usize, a1: &str) -> Style {
    let wb = &s.documents()[doc].wb;
    let sh = wb.active().unwrap();
    wb.styles.get(sh.style_id(gridcraft_engine::core::CellRef::parse(a1).unwrap())).clone()
}

#[test]
fn the_sources_styles_come_over_and_replace_those_with_the_same_names() {
    let (mut s, target, source) = two_books();
    let source_before = s.documents()[source].wb.clone();
    let shared_there = s.documents()[source].wb.cell_styles.iter().find(|(n, _)| n == "Shared").unwrap().1.clone();
    let num_only = s.documents()[source].wb.cell_styles.iter().find(|(n, _)| n == "NumOnly").unwrap().1.clone();
    let cells_before: Vec<Style> = ["A1", "A2", "A3", "A4", "B1"].iter().map(|a| cell_style(&s, target, a)).collect();
    let undo_before = s.doc().unwrap().undo.len();

    let r = run(&mut s, "home.mergeStyles", json!({"from": "Book2"}));
    assert_eq!(r["from"], "Book2");
    assert_eq!(strings(&r["added"]), ["Src Only", "NumOnly"]);
    assert_eq!(strings(&r["replaced"]), ["Shared", "Bad", "Good"]);
    assert_eq!(strings(&r["kept"]), Vec::<String>::new());
    assert_eq!(s.doc().unwrap().undo.len(), undo_before + 1, "one undo step");

    // Custom styles 2 → 4: the target's own one stays, the source's come over.
    assert_eq!(names(&s), ["Shared", "Target Only", "Src Only", "NumOnly", "Good"]);
    let shared = own(&s, "Shared").unwrap();
    assert_eq!(shared, shared_there, "the source's Shared replaces the target's completely");
    assert!(shared.font.italic && shared.font.underline == Underline::None && shared.num_fmt.as_str() == "0%");
    assert_eq!(own(&s, "Target Only").unwrap().font.size, 9.0);
    // A style with only a number format comes over as it is.
    assert_eq!(own(&s, "NumOnly").unwrap(), num_only);
    assert_eq!(num_only.num_fmt.as_str(), "@");
    // Built-ins: the source's changed Good comes over; the target's changed Bad, unchanged in the
    // source, goes back to the built-in Bad.
    assert_eq!(own(&s, "Good").unwrap().font.size, 20.0);
    assert_eq!(own(&s, "Bad"), None);
    let wb = &s.doc().unwrap().wb;
    assert_eq!(named_cell_style(wb, "Bad"), gridcraft_engine::cmd::format::builtin_cell_style("Bad", &wb.theme));

    // The source is unchanged.
    assert!(std::sync::Arc::ptr_eq(&s.documents()[source].wb, &source_before));
    // Cells keep the formats they had (they don't remember their style), B1 its bold.
    let cells_after: Vec<Style> = ["A1", "A2", "A3", "A4", "B1"].iter().map(|a| cell_style(&s, target, a)).collect();
    assert_eq!(cells_after, cells_before);

    // Applying a merged style gives its formats.
    run(&mut s, "home.cellStyle", json!({"range": "C1", "name": "Src Only"}));
    let c1 = cell_style(&s, target, "C1");
    assert!(c1.font.bold);
    assert_eq!(c1.font.color, Color::rgb(0xFF, 0, 0));
    assert_eq!(c1.fill.fg, Color::rgb(0xFF, 0xFF, 0));
    assert_eq!(c1.num_fmt.as_str(), "0%");

    // Undo takes the merge back as one step.
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(names(&s), ["Shared", "Target Only", "Bad"]);
    assert_eq!(own(&s, "Shared").unwrap().font.underline, Underline::Double);
    assert_eq!(own(&s, "Bad").unwrap().font.size, 16.0);
    run(&mut s, "edit.redo", json!({}));
    assert_eq!(names(&s), ["Shared", "Target Only", "Src Only", "NumOnly", "Good"]);
}

#[test]
fn keeping_same_names_adds_only_the_styles_the_target_lacks() {
    let (mut s, _, _) = two_books();
    let r = run(&mut s, "home.mergeStyles", json!({"from": "Book2", "sameNames": "keep"}));
    assert_eq!(strings(&r["added"]), ["Src Only", "NumOnly"]);
    assert_eq!(strings(&r["replaced"]), Vec::<String>::new());
    assert_eq!(strings(&r["kept"]), ["Shared", "Bad", "Good"]);
    assert_eq!(names(&s), ["Shared", "Target Only", "Bad", "Src Only", "NumOnly"]);
    assert_eq!(own(&s, "Shared").unwrap().font.underline, Underline::Double);
    assert_eq!(own(&s, "Bad").unwrap().font.size, 16.0);
    assert_eq!(own(&s, "Good"), None, "every workbook has Good: the target keeps its own");
    // `replace` is the default, spelled out.
    s.execute("edit.undo", json!({})).unwrap();
    let r = run(&mut s, "home.mergeStyles", json!({"from": "Book2", "sameNames": "replace"}));
    assert_eq!(strings(&r["replaced"]), ["Shared", "Bad", "Good"]);
}

#[test]
fn merging_from_an_untouched_workbook_resets_the_built_ins_and_keeps_the_custom_styles() {
    let (mut s, target, _) = two_books();
    run(&mut s, "home.mergeStyles", json!({"from": "Book2"}));
    s.new_workbook();
    s.set_active(target);
    let r = run(&mut s, "home.mergeStyles", json!({"from": "Book3"}));
    assert_eq!(strings(&r["added"]), Vec::<String>::new());
    assert_eq!(strings(&r["replaced"]), ["Good"]);
    assert_eq!(names(&s), ["Shared", "Target Only", "Src Only", "NumOnly"]);
}

#[test]
fn a_merge_that_changes_nothing_is_no_undo_step() {
    let mut s = Session::new();
    let target = s.new_workbook();
    s.new_workbook();
    s.set_active(target);
    // Neither workbook has styles of its own.
    let r = run(&mut s, "home.mergeStyles", json!({"from": "Book2"}));
    assert_eq!(r, json!({"from": "Book2", "added": [], "replaced": [], "kept": []}));
    assert!(s.doc().unwrap().undo.is_empty());
    assert!(!s.doc().unwrap().is_dirty());
    // A style both define the same way is neither replaced nor kept.
    make_style(&mut s, "Same", "A1", &[("home.bold", json!({}))]);
    s.set_active(1);
    make_style(&mut s, "Same", "A1", &[("home.bold", json!({}))]);
    s.set_active(target);
    let undo = s.doc().unwrap().undo.len();
    for same_names in ["replace", "keep"] {
        let r = run(&mut s, "home.mergeStyles", json!({"from": "Book2", "sameNames": same_names}));
        assert_eq!(r, json!({"from": "Book2", "added": [], "replaced": [], "kept": []}));
    }
    assert_eq!(s.doc().unwrap().undo.len(), undo);
}

#[test]
fn the_source_is_named_by_its_title_or_id_and_bad_sources_are_refused() {
    let (mut s, target, source) = two_books();
    let before = s.doc().unwrap().wb.clone();
    let uid = s.documents()[target].uid;
    let source_uid = s.documents()[source].uid;
    for (p, why) in [
        (json!({}), "missing `from`"),
        (json!({"from": null}), "missing `from`"),
        (json!({"from": "Book1"}), "itself"),
        (json!({"from": " book1 "}), "itself"),
        (json!({"from": uid}), "itself"),
        (json!({"from": "Book9"}), "no open workbook is called `Book9`"),
        (json!({"from": 999_999}), "no open workbook has id 999999"),
        (json!({"from": -1}), "no open workbook has id -1"),
        (json!({"from": true}), "name or id"),
        (json!({"from": ["Book2"]}), "name or id"),
        (json!({"from": "Book2", "sameNames": "ask"}), "replace or keep"),
        (json!({"from": "Book2", "sameNames": 1}), "replace or keep"),
    ] {
        let e = s.execute("home.mergeStyles", p.clone()).unwrap_err().to_string();
        assert!(e.contains(why), "{p}: {e}");
        assert!(std::sync::Arc::ptr_eq(&s.doc().unwrap().wb, &before), "{p} changed the workbook");
    }
    // The name ignores case; the id works too.
    run(&mut s, "home.mergeStyles", json!({"from": "BOOK2"}));
    s.execute("edit.undo", json!({})).unwrap();
    run(&mut s, "home.mergeStyles", json!({"from": source_uid}));
    assert!(own(&s, "Src Only").is_some());

    // A saved workbook goes by its file name, with or without the extension.
    let mut saved = gridcraft_engine::model::Workbook::new();
    saved.cell_styles.push(("From File".into(), Style::default()));
    s.add_document(DocState::new(saved, Some("/tmp/styles/Corporate Look.xlsx".into()), "Corporate Look".into(), Trust::Local));
    s.set_active(target);
    run(&mut s, "home.mergeStyles", json!({"from": "corporate look.xlsx"}));
    assert!(own(&s, "From File").is_some());
    s.execute("edit.undo", json!({})).unwrap();
    run(&mut s, "home.mergeStyles", json!({"from": "Corporate Look"}));
    assert!(own(&s, "From File").is_some());

    // Two open workbooks with the same name need the id.
    s.add_document(DocState::new(gridcraft_engine::model::Workbook::new(), Some("/tmp/other/Corporate Look.xlsx".into()), "x".into(), Trust::Local));
    s.set_active(target);
    let e = s.execute("home.mergeStyles", json!({"from": "Corporate Look.xlsx"})).unwrap_err().to_string();
    assert!(e.contains("several open workbooks"), "{e}");
}

#[test]
fn with_one_workbook_open_there_is_nothing_to_merge_from() {
    let mut s = Session::new();
    s.new_workbook();
    let e = s.execute("home.mergeStyles", json!({"from": "Book2"})).unwrap_err().to_string();
    assert!(e.contains("no open workbook is called"), "{e}");
    let e = s.execute("home.mergeStyles", json!({"from": "Book1"})).unwrap_err().to_string();
    assert!(e.contains("itself"), "{e}");
    // New Cell Style needs a name.
    assert!(s.execute("home.newCellStyle", json!({"name": "  "})).is_err());
    assert!(s.doc().unwrap().wb.cell_styles.is_empty());
}

#[test]
fn merged_styles_are_saved_and_read_back() {
    let (mut s, _, _) = two_books();
    run(&mut s, "home.mergeStyles", json!({"from": "Book2"}));
    let bytes = io::save_bytes(&s.doc().unwrap().wb, "merged.xlsx").unwrap();
    let (wb, _) = io::open_bytes("merged.xlsx", &bytes).unwrap();
    let found: Vec<&str> = wb.cell_styles.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(found, ["Shared", "Target Only", "Src Only", "NumOnly", "Good"]);
    let mut reopened = Session::new();
    reopened.add_document(DocState::new(wb, None, "merged".into(), Trust::FromFile));
    for (cell, name) in [("A1", "Src Only"), ("A2", "Shared"), ("A3", "NumOnly"), ("A4", "Good")] {
        run(&mut reopened, "home.cellStyle", json!({"range": cell, "name": name}));
    }
    let a1 = cell_style(&reopened, 0, "A1");
    assert!(a1.font.bold);
    assert_eq!(a1.font.color, Color::rgb(0xFF, 0, 0));
    assert_eq!(a1.fill.fg, Color::rgb(0xFF, 0xFF, 0));
    assert_eq!(a1.num_fmt.as_str(), "0%");
    let a2 = cell_style(&reopened, 0, "A2");
    assert!(a2.font.italic && a2.font.underline == Underline::None && a2.num_fmt.as_str() == "0%");
    assert_eq!(cell_style(&reopened, 0, "A3").num_fmt.as_str(), "@");
    assert_eq!(cell_style(&reopened, 0, "A4").font.size, 20.0);
}

#[test]
fn the_first_of_several_styles_with_one_name_counts_and_names_ignore_case() {
    let theme = Theme::default();
    let bold = Style { font: gridcraft_engine::model::Font { bold: true, ..Default::default() }, ..Default::default() };
    let italic = Style { font: gridcraft_engine::model::Font { italic: true, ..Default::default() }, ..Default::default() };
    let target = vec![("Mine".to_string(), bold.clone()), ("MINE".to_string(), italic.clone())];
    let source = vec![
        ("mine".to_string(), italic.clone()),
        ("Mine".to_string(), bold.clone()),
        ("new".to_string(), bold.clone()),
        ("NEW".to_string(), italic),
    ];
    let m = merge_cell_styles(&target, &source, &theme, false);
    assert_eq!(m.replaced, ["Mine"]);
    assert_eq!(m.added, ["new"]);
    assert_eq!(m.styles.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["mine", "new"]);
    assert!(m.changes());
    // Thousands of styles merge at once.
    let many: Vec<(String, Style)> = (0..20_000).map(|i| (format!("S{i}"), bold.clone())).collect();
    let m = merge_cell_styles(&many, &many, &theme, false);
    assert!(!m.changes());
    assert_eq!(m.styles.len(), 20_000);
}
