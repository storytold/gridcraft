//! `dataModel.createPivot`: a model-backed PivotTable written as a worksheet PivotTable — the
//! report cells, the conversion to the pivot-cache spec, and the pivot parts in the saved package.

use gridcraft_core::{CellRef, RangeRef};
use gridcraft_engine::Session;
use gridcraft_xlsx::read_model_pivots;
use serde_json::{Value as Json, json};

const MODEL: &str = r##"{
    "tables": [{"name":"Sales",
                "columns":[{"name":"Region","data_type":"String"},{"name":"Category","data_type":"String"},
                           {"name":"Channel","data_type":"String"},{"name":"Amount","data_type":"Double"}],
                "rows":[["East","A","Web",10],["East","B","Web",20],["West","A","Store",30],["West","B","Store",40]]}],
    "relationships": [],
    "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales","format":"#,##0.00"}]
}"##;

fn load() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("dataModel.load", json!({"json": MODEL})).expect("load model");
    s
}

fn create(s: &mut Session, params: Json) -> Json {
    s.execute("dataModel.createPivot", params).expect("create pivot")
}

fn cell(s: &Session, row: u32, col: u32) -> String {
    s.doc().expect("doc").wb.sheet(0).expect("sheet").value(CellRef::new(row, col)).display()
}

/// The sheet as a grid of display strings, `rows` by `cols`, from `(row, col)`.
fn grid(s: &Session, row: u32, col: u32, rows: u32, cols: u32) -> Vec<Vec<String>> {
    (0..rows).map(|r| (0..cols).map(|c| cell(s, row + r, col + c)).collect()).collect()
}

/// A report rectangle: its top-left cell, rows and columns.
type Rect = (CellRef, u32, u32);

/// Every non-empty cell of the `old` rectangle that the `new` one does not cover: after a re-apply a
/// report of another shape must leave nothing of the previous one behind.
fn stale_cells(s: &Session, old: Rect, new: Rect) -> Vec<String> {
    let end = |(at, rows, cols): Rect| CellRef::new(at.row + rows - 1, at.col + cols - 1);
    let (old_at, new_at) = (old.0, new.0);
    let covered = RangeRef::new(new_at, end(new));
    let mut stale = Vec::new();
    for row in old_at.row..=end(old).row {
        for col in old_at.col..=end(old).col {
            let at = CellRef::new(row, col);
            let text = cell(s, row, col);
            if !covered.contains(at) && !text.is_empty() {
                stale.push(format!("{}={text}", at.a1()));
            }
        }
    }
    stale
}

#[test]
fn a_row_field_a_column_field_a_filter_and_totals_write_the_report_grid() {
    let mut s = load();
    let reply = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Region]"],
            "column_fields": ["Sales[Category]"],
            "filters": [{"field": "Sales[Channel]", "allowed": ["Web"]}],
            "grand_totals": true,
            "layout": "tabular",
            "at": "H3",
        }),
    );
    assert_eq!(reply["name"], json!("ModelPivot1"));
    assert_eq!(reply["sheet"], json!("Sheet1"));
    assert_eq!(reply["at"], json!("H3"), "the pivot starts at the requested cell");
    assert_eq!(reply["rows"], json!(3), "one header row and two report rows");
    assert_eq!(reply["columns"], json!(4));
    assert_eq!(reply["values"], json!(["Total Sales"]));
    assert_eq!(reply["cached"], json!(true), "the workbook now carries the pivot parts");

    // The page field takes two rows above the body: its caption, then the selected item.
    assert_eq!(grid(&s, 2, 7, 2, 1), vec![vec!["Channel"], vec!["Web"]]);
    // The body: the label column, the two column groups and the grand total. The filter leaves
    // the East rows only.
    let body = grid(&s, 4, 7, 3, 4);
    assert_eq!(body, vec![vec!["Region", "A", "B", "Grand Total"], vec!["East", "10", "20", "30"], vec!["Grand Total", "10", "20", "30"],]);
    // The value columns carry the measure's number format; the header and labels don't.
    let wb = &s.doc().expect("doc").wb;
    let sheet = wb.sheet(0).expect("sheet");
    let format = |row: u32, col: u32| wb.styles.get(sheet.style_id(CellRef::new(row, col))).num_fmt.as_str().to_string();
    assert_eq!(format(5, 8), "#,##0.00");
    assert_eq!(format(5, 9), "#,##0.00");
    assert_eq!(format(4, 7), "General", "the header keeps the sheet style");
}

#[test]
fn two_row_fields_write_subtotal_and_grand_total_rows() {
    let mut s = load();
    let reply = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Region]", "Sales[Category]"],
            "row_subtotals": true,
            "grand_totals": true,
            "layout": "outline",
        }),
    );
    assert_eq!(reply["at"], json!("A3"), "A3 is the default location");
    assert_eq!(reply["rows"], json!(8));
    assert_eq!(reply["columns"], json!(3));
    let body = grid(&s, 2, 0, 8, 3);
    assert_eq!(
        body,
        vec![
            vec!["Region", "Category", "Total Sales"],
            vec!["East", "A", "10"],
            vec!["", "B", "20"],
            vec!["East Total", "", "30"],
            vec!["West", "A", "30"],
            vec!["", "B", "40"],
            vec!["West Total", "", "70"],
            vec!["Grand Total", "", "100"],
        ]
    );
}

#[allow(clippy::cognitive_complexity)] // Asserts every part and slot the written package must carry.
#[test]
fn the_package_carries_the_pivot_table_and_cache_definitions() {
    let mut s = load();
    let params = json!({
        "measure": "Total Sales",
        "row_fields": ["Sales[Region]"],
        "column_fields": ["Sales[Category]"],
        "filters": [{"field": "Sales[Channel]", "allowed": ["Web", "Store"], "include_blank": true}],
        "column_subtotals": false,
        "grand_totals": true,
        "name": "By Region",
        "at": "H3",
    });
    let (reply, package) = gridcraft_engine::cmd::dax_pivot::write_pivot(&mut s, &params).expect("write pivot");
    assert_eq!(reply["name"], json!("By Region"));
    let package = package.expect("the pivot parts are written");

    // The workbook itself keeps the parts, so an ordinary save writes them too.
    let kept: Vec<String> = s
        .doc()
        .expect("doc")
        .wb
        .preserved
        .as_ref()
        .map(|preserved| preserved.parts.iter().map(|part| part.name.clone()).collect())
        .unwrap_or_default();
    assert!(kept.iter().any(|name| name.starts_with("xl/pivotTables/pivotTable")), "{kept:?}");
    assert!(kept.iter().any(|name| name.starts_with("xl/pivotCache/pivotCacheDefinition")), "{kept:?}");

    // The package holds the two definitions Excel needs, their relationships and the model
    // connection, and the workbook declares the cache.
    let names: Vec<String> = {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(package.clone())).expect("zip");
        (0..archive.len()).map(|i| archive.by_index(i).expect("entry").name().to_string()).collect()
    };
    assert!(names.iter().any(|name| name.starts_with("xl/pivotTables/pivotTable")), "{names:?}");
    assert!(names.iter().any(|name| name.starts_with("xl/pivotCache/pivotCacheDefinition")), "{names:?}");
    assert!(names.iter().any(|name| name == "xl/connections.xml"), "{names:?}");
    assert!(names.iter().any(|name| name.starts_with("xl/pivotTables/_rels/")), "{names:?}");
    let workbook = {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(package.clone())).expect("zip");
        let mut file = archive.by_name("xl/workbook.xml").expect("workbook part");
        let mut text = String::new();
        std::io::Read::read_to_string(&mut file, &mut text).expect("workbook xml");
        text
    };
    assert!(workbook.contains("<pivotCaches>"), "the workbook declares the cache: {workbook}");
    // The model extension is only written when a table resolves to a worksheet connection; this
    // workbook has none, and Excel refuses an extension whose connections are empty, so it is
    // omitted rather than emitted blank.
    assert!(!workbook.contains("connection=\"\""), "no empty connection attribute: {workbook}");
    assert!(!workbook.contains("x15:modelTables"), "no model tables without a connection: {workbook}");

    // Reading the package back gives the same pivot: the axes, the filter with blank, the value
    // and the totals the command was asked for.
    let (parts, report) = read_model_pivots(&package).expect("read model pivots");
    assert_eq!(parts.len(), 1);
    assert!(
        report.warnings.iter().any(|warning| warning.contains("measure formulas were not read")),
        "the reader says the formulas live in the model database: {:?}",
        report.warnings
    );
    let part = &parts[0];
    assert_eq!(part.sheet, "Sheet1");
    assert_eq!(part.location, CellRef::new(4, 7), "the body sits below the page field");
    assert_eq!(part.spec.name, "By Region");
    assert_eq!(part.spec.cache_id, 1);
    assert_eq!(part.spec.row_fields.len(), 1);
    assert_eq!(part.spec.row_fields[0].column, "Region");
    assert_eq!(part.spec.column_fields.len(), 1);
    assert_eq!(part.spec.column_fields[0].column, "Category");
    assert_eq!(part.spec.values.len(), 1);
    assert_eq!(part.spec.values[0].measure, "Total Sales");
    // The cache writer stores `numFmtId="0"` for every field, so the format the engine passed in
    // the spec does not survive a read; the report cells carry it (the grid test checks that).
    assert_eq!(part.spec.filters.len(), 1);
    assert!(part.spec.filters[0].include_blank, "the blank shared item becomes include_blank");
    assert_eq!(part.spec.filters[0].allowed.len(), 2, "the two values; blank is reported by include_blank");
    assert!(part.spec.grand_totals);
    assert_eq!(part.spec.measures.len(), 1);
    assert_eq!(part.spec.measures[0].name, "Total Sales", "the cache names the measure; its formula lives in the model database");
    assert_eq!(part.spec.tables.len(), 1);
    assert_eq!(part.spec.tables[0].columns.len(), 4, "every model column is declared, used or not");
}

#[test]
fn a_second_pivot_gets_its_own_name_and_cache() {
    let mut s = load();
    let first = create(&mut s, json!({"measure": "Total Sales", "row_fields": ["Sales[Region]"], "at": "A1"}));
    let second = create(&mut s, json!({"measure": "Total Sales", "column_fields": ["Sales[Category]"], "at": "F1"}));
    assert_eq!(first["name"], json!("ModelPivot1"));
    assert_eq!(second["name"], json!("ModelPivot2"), "the first pivot's name is taken");
    let bytes = gridcraft_xlsx::write_xlsx(&s.doc().expect("doc").wb).expect("save");
    let (parts, _) = read_model_pivots(&bytes).expect("read model pivots");
    assert_eq!(parts.len(), 2);
    assert_ne!(parts[0].spec.cache_id, parts[1].spec.cache_id, "each PivotTable has its own cache");
    assert_ne!(parts[0].spec.name, parts[1].spec.name);
}

#[test]
fn the_edit_is_one_undo_step() {
    let mut s = load();
    create(&mut s, json!({"measure": "Total Sales", "row_fields": ["Sales[Region]"]}));
    assert_eq!(s.doc().expect("doc").undo.len(), 1);
    assert_eq!(cell(&s, 2, 0), "Row Labels");
    s.execute("edit.undo", json!({})).expect("undo");
    assert_eq!(cell(&s, 2, 0), "", "undo takes the report away");
    assert!(s.doc().expect("doc").wb.preserved.is_none(), "undo takes the pivot parts away with the report");
}

#[test]
fn unknown_names_are_engine_errors() {
    let mut s = load();
    let cases = [
        (json!({"measure": "Nope"}), "no measure named `Nope`"),
        (json!({"measure": "Total Sales", "row_fields": ["Sales[Nope]"]}), "unknown column `Nope` in table `Sales`"),
        (json!({"measure": "Total Sales", "row_fields": ["Nope[Region]"]}), "unknown table `Nope`"),
        (json!({"measure": "Total Sales", "filters": [{"field": "Sales[Nope]", "allowed": ["A"]}]}), "unknown column `Nope` in table `Sales`"),
        (json!({"measure": "Total Sales", "row_fields": ["Sales[Region]"], "column_fields": ["Sales[Region]"]}), "both the row and column axes"),
        (json!({"measure": "Total Sales", "sheet": "Nope"}), "no sheet named `Nope`"),
        (json!({"measure": "Total Sales", "at": "Sheet2!B2"}), "names a sheet other than"),
        (json!({"measure": "Total Sales", "layout": "wide"}), "unknown layout `wide`"),
    ];
    for (params, expected) in cases {
        let error = s.execute("dataModel.createPivot", params.clone()).expect_err(&format!("{params} must fail"));
        assert!(error.to_string().contains(expected), "{params}: {error}");
    }
    // A field that does not parse as `Table[Column]` is refused too.
    let malformed = json!({"measure": "Total Sales", "row_fields": ["Sales[Amount"]});
    assert!(s.execute("dataModel.createPivot", malformed).is_err(), "an unparsable field is refused");
    // A failed call leaves the workbook alone: no cells, no undo step.
    assert_eq!(cell(&s, 0, 0), "");
    assert!(s.doc().expect("doc").undo.is_empty());
}

#[test]
fn an_empty_model_reports_the_missing_measure() {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("dataModel.load", json!({"json": r#"{"tables":[],"relationships":[],"measures":[]}"#})).expect("load");
    let error = s.execute("dataModel.createPivot", json!({"measure": "Total Sales"})).expect_err("empty model");
    assert!(error.to_string().contains("no measure named `Total Sales`"), "{error}");
}

#[test]
fn hostile_parameters_never_panic() {
    let mut s = load();
    let huge: Vec<String> = (0..1_000).map(|i| format!("Sales[Region{i}]")).collect();
    let cases = [
        json!({"measure": 42}),
        json!({"measure": null}),
        json!({"measure": "Total Sales", "row_fields": {"a": 1}}),
        json!({"measure": "Total Sales", "row_fields": huge}),
        json!({"measure": "Total Sales", "column_fields": [1, 2]}),
        json!({"measure": "Total Sales", "layout": true}),
        json!({"measure": "Total Sales", "grand_totals": "yes"}),
        json!({"measure": "Total Sales", "filters": [1, 2]}),
        json!({"measure": "Total Sales", "filters": [{"field": "Sales[Region]", "allowed": {"a": 1}}]}),
        json!({"measure": "Total Sales", "filters": [{"allowed": ["A"]}]}),
        json!({"measure": "Total Sales", "at": "not a cell"}),
        json!({"measure": "Total Sales", "at": "A1048576"}),
        json!({"measure": "Total Sales", "name": 7}),
        json!({"measure": "Total Sales", "sheet": []}),
    ];
    for params in cases {
        assert!(s.execute("dataModel.createPivot", params.clone()).is_err(), "{params} must be refused");
        assert_eq!(cell(&s, 0, 0), "", "{params} wrote nothing");
    }
    assert!(s.doc().expect("doc").undo.is_empty(), "no failed call records an undo step");
}

#[test]
fn the_commands_are_disabled_without_a_model() {
    let s = Session::new();
    for id in ["dataModel.createPivot", "insert.modelPivotTable"] {
        assert!(s.commands().iter().any(|command| command.id == id && !command.enabled), "{id} is greyed out until a model loads");
    }
    let mut s = load();
    for id in ["dataModel.createPivot", "insert.modelPivotTable"] {
        assert!(s.commands().iter().any(|command| command.id == id && command.enabled), "{id} is live with a model");
    }
    // The Insert command is the same command under another id.
    let reply = s.execute("insert.modelPivotTable", json!({"measure": "Total Sales", "at": "F1"})).expect("insert");
    assert_eq!(reply["at"], json!("F1"));
}

/// `dataModel.pivotFields` on a session holding `s`.
fn fields(s: &mut Session, params: Json) -> Json {
    s.execute("dataModel.pivotFields", params).expect("pivot fields")
}

#[test]
fn the_field_list_is_disabled_without_a_model() {
    let mut s = Session::new();
    assert!(
        s.commands().iter().any(|command| command.id == "dataModel.pivotFields" && !command.enabled),
        "the field list is greyed out until a model loads"
    );
    // The command itself refuses without a model, with the same message as its siblings.
    let error = s.execute("dataModel.pivotFields", json!({})).expect_err("no model");
    assert!(error.to_string().contains("no data model loaded"), "{error}");
    let s = load();
    assert!(s.commands().iter().any(|command| command.id == "dataModel.pivotFields" && command.enabled), "the field list is live with a model");
}

#[test]
fn the_field_list_reports_the_model_with_no_pivot() {
    let mut s = load();
    let reply = fields(&mut s, json!({}));
    assert_eq!(reply["pivots"], json!([]), "a workbook with no model PivotTable lists none");
    assert!(reply["selected"].is_null(), "no `name`, no selection");
    let model = &reply["model"];
    assert_eq!(model["tables"].as_array().map(Vec::len), Some(1));
    assert_eq!(model["tables"][0]["name"], json!("Sales"));
    assert_eq!(
        model["tables"][0]["columns"],
        json!([
            {"name": "Region", "type": "String"},
            {"name": "Category", "type": "String"},
            {"name": "Channel", "type": "String"},
            {"name": "Amount", "type": "Double"},
        ])
    );
    assert_eq!(model["measures"], json!([{"name": "Total Sales", "table": "Sales", "format": "#,##0.00"}]));
    assert_eq!(model["relationships"], json!([]));
}

#[test]
fn the_field_list_reports_the_created_pivot() {
    let mut s = load();
    let created = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Region]"],
            "column_fields": ["Sales[Category]"],
            "filters": [{"field": "Sales[Channel]", "allowed": ["Web", null]}],
            "layout": "tabular",
            "grand_totals": true,
        }),
    );
    assert_eq!(created["name"], json!("ModelPivot1"));
    let undo = s.doc().expect("doc").undo.len();
    let reply = fields(&mut s, json!({"name": "ModelPivot1"}));
    assert_eq!(s.doc().expect("doc").undo.len(), undo, "the field list records no undo step");
    // One page field takes two rows above the body, so the body starts at A5 while the command's
    // `at` (the pivot's top-left, page fields included) was A3.
    assert_eq!(reply["pivots"], json!([{"name": "ModelPivot1", "sheet": "Sheet1", "at": "A5"}]));
    assert_eq!(
        reply["selected"],
        json!({
            "name": "ModelPivot1",
            "sheet": "Sheet1",
            "at": "A5",
            "row_fields": ["Sales[Region]"],
            "column_fields": ["Sales[Category]"],
            "filters": [{"field": "Sales[Channel]", "allowed": ["Web", null]}],
            "values": ["Total Sales"],
            "layout": "tabular",
            "row_subtotals": false,
            "column_subtotals": false,
            "grand_totals": true,
        })
    );
    // The pane round-trips: the axes and the filter of `selected` are what `createPivot` takes back.
    let again = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": reply["selected"]["row_fields"].clone(),
            "column_fields": reply["selected"]["column_fields"].clone(),
            "filters": reply["selected"]["filters"].clone(),
            "layout": reply["selected"]["layout"].clone(),
            "grand_totals": reply["selected"]["grand_totals"].clone(),
            "at": "J1",
        }),
    );
    assert_eq!(again["name"], json!("ModelPivot2"));
    let edits = s.doc().expect("doc").undo.len();
    assert_eq!(edits, 2, "each create is one edit");
    let both = fields(&mut s, json!({}));
    assert_eq!(both["pivots"].as_array().map(Vec::len), Some(2), "every model PivotTable is listed");
    assert!(both["selected"].is_null(), "without `name` nothing is selected");
    // The command is a query: it writes nothing and records no undo step.
    assert_eq!(s.doc().expect("doc").undo.len(), edits, "the field list records no undo step");
    assert_eq!(cell(&s, 0, 0), "", "the field list wrote no cell");
}

#[test]
fn a_values_area_lists_every_measure() {
    let mut s = load();
    s.execute("dataModel.addMeasure", json!({"name": "Total Qty", "formula": "SUM(Sales[Amount])", "table": "Sales", "format": "#,##0"}))
        .expect("add measure");
    let reply = create(&mut s, json!({"measure": "Total Sales", "values": ["Total Sales", "Total Qty"], "row_fields": ["Sales[Region]"]}));
    assert_eq!(reply["values"], json!(["Total Sales", "Total Qty"]));
    assert_eq!(reply["columns"], json!(3), "a label column and one column per value measure");
    let reply = fields(&mut s, json!({"name": "ModelPivot1"}));
    assert_eq!(reply["selected"]["values"], json!(["Total Sales", "Total Qty"]));
    // Both measures are in the model the pane gets, so it can draw them as available fields.
    let measures = reply["model"]["measures"].as_array().expect("measures");
    assert_eq!(measures.len(), 2);
    assert_eq!(measures[1]["name"], json!("Total Qty"));
    assert_eq!(measures[1]["format"], json!("#,##0"));
    // An absent `values` keeps the single-measure pivot.
    let single = create(&mut s, json!({"measure": "Total Sales", "at": "J1"}));
    assert_eq!(single["values"], json!(["Total Sales"]));
    // An unknown measure in the values area is refused like an unknown `measure`.
    for params in [json!({"measure": "Total Sales", "values": ["Nope"]}), json!({"measure": "Total Sales", "values": ["Total Sales", "Nope"]})] {
        let error = s.execute("dataModel.createPivot", params.clone()).expect_err("unknown value measure");
        assert!(error.to_string().contains("no measure named `Nope`"), "{params}: {error}");
    }
}

#[test]
fn an_unknown_pivot_name_selects_nothing() {
    let mut s = load();
    create(&mut s, json!({"measure": "Total Sales", "row_fields": ["Sales[Region]"]}));
    let reply = fields(&mut s, json!({"name": "NoSuchPivot"}));
    assert!(reply["selected"].is_null(), "a name the workbook does not carry selects nothing");
    assert_eq!(reply["pivots"].as_array().map(Vec::len), Some(1), "the carried pivots are still listed");
    // The name is matched case-insensitively, as Excel names are.
    let reply = fields(&mut s, json!({"name": " modelpivot1 "}));
    assert_eq!(reply["selected"]["name"], json!("ModelPivot1"));
}

#[test]
fn the_field_list_survives_hostile_parameters() {
    let mut s = load();
    create(&mut s, json!({"measure": "Total Sales", "at": "H1"}));
    let huge: Vec<String> = (0..1_000).map(|i| format!("Measure {i}")).collect();
    for params in [json!({"name": 42}), json!({"name": {"a": 1}}), json!({"name": [1, 2]})] {
        let error = s.execute("dataModel.pivotFields", params.clone()).expect_err("a non-string name");
        assert!(error.to_string().contains("`name` must be a string"), "{params}: {error}");
    }
    // An empty name is no name, not an error.
    assert!(fields(&mut s, json!({"name": ""}))["selected"].is_null());
    for params in [
        json!({"measure": "Total Sales", "values": {"a": 1}}),
        json!({"measure": "Total Sales", "values": [1, 2]}),
        json!({"measure": "Total Sales", "values": huge}),
    ] {
        assert!(s.execute("dataModel.createPivot", params.clone()).is_err(), "{params} must be refused");
    }
    // A single measure string is accepted like a one-item list.
    let reply = create(&mut s, json!({"measure": "Total Sales", "values": "Total Sales", "at": "H20"}));
    assert_eq!(reply["values"], json!(["Total Sales"]));
    // No refused call wrote a cell or recorded an edit; the two successful creates are the only ones.
    assert_eq!(s.doc().expect("doc").undo.len(), 2);
    assert_eq!(cell(&s, 0, 0), "");
}

#[test]
fn a_taken_name_and_a_busy_area_are_refused_without_replace() {
    let mut s = load();
    create(&mut s, json!({"measure": "Total Sales", "name": "Mine", "at": "H1"}));
    let error = s.execute("dataModel.createPivot", json!({"measure": "Total Sales", "name": "Mine"})).expect_err("duplicate name");
    assert!(error.to_string().contains("already exists"), "{error}");
    let error = s.execute("dataModel.createPivot", json!({"measure": "Total Sales", "at": "H1"})).expect_err("busy area");
    assert!(error.to_string().contains("There's already data in Sheet1!H1"), "{error}");
    let reply = create(&mut s, json!({"measure": "Total Sales", "at": "H1", "replace": true}));
    assert_eq!(reply["at"], json!("H1"));
}

#[test]
fn replace_re_applies_a_kept_pivot_under_its_own_name() {
    let mut s = load();
    create(&mut s, json!({"measure": "Total Sales", "row_fields": ["Sales[Region]"], "name": "Mine", "at": "H1"}));
    // The field-list pane's Apply: the same name with `replace` rewrites that pivot, so the workbook
    // still carries one PivotTable under the name and it has the axes that were just applied.
    let reply = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Region]"],
            "column_fields": ["Sales[Category]"],
            "grand_totals": true,
            "name": "Mine",
            "at": "H1",
            "replace": true,
        }),
    );
    assert_eq!(reply["name"], json!("Mine"));
    let listing = fields(&mut s, json!({"name": "Mine"}));
    assert_eq!(listing["pivots"].as_array().map(Vec::len), Some(1), "one pivot under the name, not two");
    assert_eq!(listing["selected"]["column_fields"], json!(["Sales[Category]"]), "the applied axes are the pivot's");
    assert_eq!(listing["selected"]["grand_totals"], json!(true), "the applied totals are the pivot's");
}

#[test]
fn replace_clears_the_previous_report_when_the_shape_changes() {
    let mut s = load();
    // Two row fields and one column field: 5 rows by 3 columns at A3 (A3:C7).
    let first = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Region]", "Sales[Category]"],
            "column_fields": ["Sales[Channel]"],
            "name": "Mine",
        }),
    );
    assert_eq!((first["at"].as_str(), first["rows"].as_u64(), first["columns"].as_u64()), (Some("A3"), Some(5), Some(3)));
    // One row field and two column fields: 3 rows by 5 columns at A3 (A3:E5). The previous report
    // ran two rows further down, and Excel fails to refresh the pivot while those cells stay.
    let second = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Region]"],
            "column_fields": ["Sales[Category]", "Sales[Channel]"],
            "name": "Mine",
            "replace": true,
        }),
    );
    assert_eq!((second["at"].as_str(), second["rows"].as_u64(), second["columns"].as_u64()), (Some("A3"), Some(3), Some(5)));
    assert_eq!(
        stale_cells(&s, (CellRef::new(2, 0), 5, 3), (CellRef::new(2, 0), 3, 5)),
        Vec::<String>::new(),
        "no cell of the old report is left behind"
    );
    // Still one pivot under the name, with the applied axes.
    let listing = fields(&mut s, json!({"name": "Mine"}));
    assert_eq!(listing["pivots"].as_array().map(Vec::len), Some(1));
    assert_eq!(listing["selected"]["row_fields"], json!(["Sales[Region]"]));
}

#[test]
fn replace_clears_a_previous_report_that_was_bigger_in_both_directions() {
    let mut s = load();
    // One row field and two column fields: 3 rows by 5 columns at A3 (A3:E5).
    let first = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "row_fields": ["Sales[Region]"],
            "column_fields": ["Sales[Category]", "Sales[Channel]"],
            "name": "Mine",
        }),
    );
    assert_eq!((first["rows"].as_u64(), first["columns"].as_u64()), (Some(3), Some(5)));
    // A page filter and no axis at all: the report keeps its `Row Labels` column, so 2 rows by 2
    // columns, its page fields on A3:A4 and its body on A5:A6. The old report's columns C:E must go,
    // and so must its body rows inside the new area.
    let second = create(
        &mut s,
        json!({
            "measure": "Total Sales",
            "filters": [{"field": "Sales[Channel]", "allowed": ["Web"]}],
            "name": "Mine",
            "replace": true,
        }),
    );
    assert_eq!((second["at"].as_str(), second["rows"].as_u64(), second["columns"].as_u64()), (Some("A3"), Some(2), Some(2)));
    // The new pivot occupies A3:A6 (two page rows, then the report); the old A3:E5 must hold nothing
    // else.
    assert_eq!(
        stale_cells(&s, (CellRef::new(2, 0), 3, 5), (CellRef::new(2, 0), 4, 2)),
        Vec::<String>::new(),
        "the smaller report leaves nothing behind"
    );
    let listing = fields(&mut s, json!({"name": "Mine"}));
    assert_eq!(listing["selected"]["row_fields"], json!([]), "the applied report has no row fields");
    assert_eq!(listing["selected"]["column_fields"], json!([]), "the applied report has no column fields");
    assert_eq!(listing["selected"]["filters"], json!([{"field": "Sales[Channel]", "allowed": ["Web"]}]));
}
