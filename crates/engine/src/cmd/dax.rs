//! Data Model (Power Pivot prototype) commands: load a JSON model, edit its measures and
//! relationships, evaluate DAX, and group a measure into a pivot-shaped table.
//!
//! The editing commands mutate `Session::dax_model` — application state, not the workbook — so
//! they record no undo step; each one revalidates the whole model through
//! [`DataModel::parse_formulas`] before it is stored, so a failed edit leaves the model untouched.

use std::collections::BTreeSet;

use gridcraft_dax::{Binder, DataModel, Evaluator, Measure, Relationship, parse_expression, pivot_cross, pivot_fields};
use serde_json::{Value as Json, json};

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "dataModel.status", "Data Model", ["Data", "Data Model"], None, "{} → {tables, measures}", always, status),
        cmd!("dataModel.load", "Load Data Model", ["Data", "Data Model"], None, "{path} → {tables, measures}", always, load),
        cmd!(
            query
            "dataModel.evaluate",
            "Evaluate DAX",
            ["Data", "Data Model"],
            None,
            "{formula?, measure?, table?: bool} → scalar or table",
            always,
            evaluate
        ),
        cmd!(
            query
            "dataModel.pivot",
            "Data Model Pivot",
            ["Data", "Data Model"],
            None,
            "{measure, rows: [\"Sales[Region]\"]} → {columns, rows}",
            always,
            pivot_cmd
        ),
        cmd!(
            "dataModel.addMeasure",
            "Add Measure",
            ["Data", "Data Model"],
            None,
            "{name, formula, table?, format?} → status",
            has_model,
            add_measure
        ),
        cmd!(
            "dataModel.updateMeasure",
            "Update Measure",
            ["Data", "Data Model"],
            None,
            "{name, formula?, format?, new_name?} → status",
            has_model,
            update_measure
        ),
        cmd!("dataModel.deleteMeasure", "Delete Measure", ["Data", "Data Model"], None, "{name} → status", has_model, delete_measure),
        cmd!(
            "dataModel.addRelationship",
            "Create Relationship",
            ["Data", "Data Model"],
            None,
            "{from_table, from_column, to_table, to_column, bidirectional?, inactive?} → status",
            has_model,
            add_relationship
        ),
        cmd!(
            "dataModel.deleteRelationship",
            "Delete Relationship",
            ["Data", "Data Model"],
            None,
            "{from_table, from_column, to_table, to_column} → status",
            has_model,
            delete_relationship
        ),
    ]
}

/// Measure and relationship commands need a model, exactly as the Power Pivot window does: they
/// are greyed out until `dataModel.load` (or a workbook adapter) fills `Session::dax_model`.
fn has_model(s: &Session) -> std::result::Result<(), String> {
    if s.dax_model.is_some() { Ok(()) } else { Err("no data model loaded".into()) }
}

fn status(s: &mut Session, _: &Json) -> Result<Json> {
    let Some(model) = &s.dax_model else {
        return Ok(json!({"loaded": false, "tables": [], "measures": []}));
    };
    let tables: Vec<Json> = model
        .tables
        .iter()
        .map(|table| {
            json!({
                "name": table.name,
                "columns": table.columns.iter().map(|column| json!({"name": column.name, "type": column.data_type})).collect::<Vec<_>>(),
                "rows": table.rows.len(),
            })
        })
        .collect();
    let measures: Vec<Json> = model
        .measures
        .iter()
        .map(|measure| json!({"name": measure.name, "table": measure.table, "formula": measure.formula, "format": measure.format}))
        .collect();
    let relationships: Vec<Json> = model
        .relationships
        .iter()
        .map(|relationship| {
            json!({
                "from_table": relationship.from_table,
                "from_column": relationship.from_column,
                "to_table": relationship.to_table,
                "to_column": relationship.to_column,
                "bidirectional": relationship.bidirectional,
                "inactive": relationship.inactive,
            })
        })
        .collect();
    Ok(json!({"loaded": true, "tables": tables, "measures": measures, "relationships": relationships}))
}

fn load(s: &mut Session, p: &Json) -> Result<Json> {
    let path = p.get("path").and_then(Json::as_str).ok_or_else(|| EngineError::Other("dataModel.load needs `path`".into()))?;
    let text = std::fs::read_to_string(path).map_err(|error| EngineError::Other(format!("can't read {path}: {error}")))?;
    let mut model: DataModel = serde_json::from_str(&text).map_err(|error| EngineError::Other(format!("invalid model JSON: {error}")))?;
    model.parse_formulas().map_err(|error| EngineError::Other(error.to_string()))?;
    s.dax_model = Some(model);
    status(s, &Json::Null)
}

fn evaluate(s: &mut Session, p: &Json) -> Result<Json> {
    let model = s.dax_model.as_ref().ok_or_else(|| EngineError::Other("no data model loaded".into()))?;
    let evaluator = Evaluator::new(model).map_err(|error| EngineError::Other(error.to_string()))?;
    if let Some(measure) = p.get("measure").and_then(Json::as_str) {
        let value = evaluator.evaluate_measure(measure, Default::default()).map_err(|error| EngineError::Other(error.to_string()))?;
        return Ok(json!({"value": value_json(&value)}));
    }
    let formula =
        p.get("formula").and_then(Json::as_str).ok_or_else(|| EngineError::Other("dataModel.evaluate needs `formula` or `measure`".into()))?;
    let expr = parse_expression(formula).map_err(|error| EngineError::Other(error.to_string()))?;
    let bound = Binder::new(model).bind_expression(&expr).map_err(|error| EngineError::Other(error.to_string()))?;
    if p.get("table").and_then(Json::as_bool).unwrap_or(false) {
        let table = evaluator.evaluate_table(&bound).map_err(|error| EngineError::Other(error.to_string()))?;
        return Ok(table_json(&table));
    }
    let value = evaluator.evaluate(&bound).map_err(|error| EngineError::Other(error.to_string()))?;
    Ok(json!({"value": value_json(&value)}))
}

fn pivot_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let model = s.dax_model.as_ref().ok_or_else(|| EngineError::Other("no data model loaded".into()))?;
    let measure = p.get("measure").and_then(Json::as_str).ok_or_else(|| EngineError::Other("dataModel.pivot needs `measure`".into()))?;
    let rows: Vec<String> = p
        .get("rows")
        .and_then(Json::as_array)
        .map(|rows| rows.iter().filter_map(Json::as_str).map(str::to_string).collect())
        .ok_or_else(|| EngineError::Other("dataModel.pivot needs `rows`".into()))?;
    let fields = pivot_fields(model, &rows).map_err(|error| EngineError::Other(error.to_string()))?;
    let columns: Vec<String> = p
        .get("columns")
        .and_then(Json::as_array)
        .map(|columns| columns.iter().filter_map(Json::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    let column_fields = pivot_fields(model, &columns).map_err(|error| EngineError::Other(error.to_string()))?;
    let table = pivot_cross(model, measure, &fields, &column_fields).map_err(|error| EngineError::Other(error.to_string()))?;
    Ok(table_json(&table))
}

// ---------------------------------------------------------------- model editing

/// Every editing command takes the current model, applies the edit to a copy, revalidates it
/// (which re-parses every measure formula) and only then stores it. A rejected edit — a duplicate
/// name, an unknown table, an unparsable formula — therefore leaves the model exactly as it was.
fn commit(s: &mut Session, cmd: &str, mut model: DataModel) -> Result<Json> {
    model.parse_formulas().map_err(|error| bad(cmd, error.to_string()))?;
    s.dax_model = Some(model);
    status(s, &Json::Null)
}

/// A copy of the loaded model, or the "no model" error for a caller that skipped the enabled check.
fn edit_model(s: &Session, cmd: &str) -> Result<DataModel> {
    s.dax_model.clone().ok_or_else(|| bad(cmd, "no data model loaded"))
}

/// A required, non-blank string parameter, trimmed.
fn required_str<'a>(p: &'a Json, cmd: &str, key: &str) -> Result<&'a str> {
    str_param(p, key).map(str::trim).filter(|value| !value.is_empty()).ok_or_else(|| bad(cmd, format!("`{key}` is required")))
}

/// An optional string parameter; absent and JSON `null` both mean "none".
fn optional_string(p: &Json, key: &str) -> Option<String> {
    p.get(key).and_then(Json::as_str).map(str::to_string)
}

/// The model's own spelling of a table name (names match case-insensitively).
fn known_table(model: &DataModel, cmd: &str, table: &str) -> Result<String> {
    let index = model.table_index(table).ok_or_else(|| bad(cmd, format!("unknown table `{table}`")))?;
    Ok(model.tables[index].name.clone())
}

/// The model's own spelling of a column, rejecting an unknown table or column.
fn known_column(model: &DataModel, cmd: &str, table: &str, column: &str) -> Result<String> {
    let table_index = model.table_index(table).ok_or_else(|| bad(cmd, format!("unknown table `{table}`")))?;
    let name = &model.tables[table_index].name;
    let column_index = model.column_index(table_index, column).ok_or_else(|| bad(cmd, format!("unknown column `{column}` in table `{name}`")))?;
    Ok(model.tables[table_index].columns[column_index].name.clone())
}

fn measure_or_error(model: &DataModel, cmd: &str, name: &str) -> Result<usize> {
    model.measure_index(name).ok_or_else(|| bad(cmd, format!("unknown measure `{name}`")))
}

/// Measures that reference `target` from their formulas. Renaming or deleting the target would
/// leave those references pointing at an unknown name (which the binder would otherwise read as
/// an unqualified column), so edits must be rejected while dependents exist.
fn dependents(model: &DataModel, cmd: &str, target: &str) -> Result<BTreeSet<String>> {
    let graph = Binder::new(model).dependency_graph().map_err(|error| bad(cmd, error.to_string()))?;
    Ok(graph
        .into_iter()
        .filter(|(name, dependencies)| {
            !name.eq_ignore_ascii_case(target) && dependencies.iter().any(|dependency| dependency.eq_ignore_ascii_case(target))
        })
        .map(|(name, _)| name)
        .collect())
}

fn dangling_reference_error(cmd: &str, action: &str, target: &str, dependent: &BTreeSet<String>) -> EngineError {
    let names: Vec<&str> = dependent.iter().map(String::as_str).collect();
    bad(cmd, format!("cannot {action} measure `{target}`: referenced by {}", names.join(", ")))
}

/// Relationship identity: the four endpoint names, compared the way the model compares names.
fn same_ends(r: &Relationship, from_table: &str, from_column: &str, to_table: &str, to_column: &str) -> bool {
    r.from_table.eq_ignore_ascii_case(from_table)
        && r.from_column.eq_ignore_ascii_case(from_column)
        && r.to_table.eq_ignore_ascii_case(to_table)
        && r.to_column.eq_ignore_ascii_case(to_column)
}

fn add_measure(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.addMeasure";
    let name = required_str(p, CMD, "name")?;
    let formula = required_str(p, CMD, "formula")?;
    let mut model = edit_model(s, CMD)?;
    if model.measure_index(name).is_some() {
        return Err(bad(CMD, format!("a measure named `{name}` already exists")));
    }
    let table = match str_param(p, "table") {
        Some(table) => Some(known_table(&model, CMD, table.trim())?),
        None => None,
    };
    let expr = parse_expression(formula).map_err(|error| bad(CMD, format!("measure `{name}`: {error}")))?;
    model.measures.push(Measure {
        name: name.to_string(),
        formula: formula.to_string(),
        table,
        format: optional_string(p, "format"),
        expr: Some(expr),
    });
    commit(s, CMD, model)
}

fn update_measure(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.updateMeasure";
    let name = required_str(p, CMD, "name")?;
    let mut model = edit_model(s, CMD)?;
    let index = measure_or_error(&model, CMD, name)?;
    let old_name = model.measures[index].name.clone();
    if p.get("new_name").is_some() {
        let new_name = required_str(p, CMD, "new_name")?;
        if model.measure_index(new_name).is_some_and(|other| other != index) {
            return Err(bad(CMD, format!("a measure named `{new_name}` already exists")));
        }
        if !new_name.eq_ignore_ascii_case(&old_name) {
            let dependent = dependents(&model, CMD, &old_name)?;
            if !dependent.is_empty() {
                return Err(dangling_reference_error(CMD, "rename", &old_name, &dependent));
            }
        }
        model.measures[index].name = new_name.to_string();
    }
    if p.get("formula").is_some() {
        let formula = required_str(p, CMD, "formula")?;
        let expr = parse_expression(formula).map_err(|error| bad(CMD, format!("measure `{}`: {error}", model.measures[index].name)))?;
        model.measures[index].formula = formula.to_string();
        model.measures[index].expr = Some(expr);
    }
    if p.get("format").is_some() {
        model.measures[index].format = optional_string(p, "format");
    }
    commit(s, CMD, model)
}

fn delete_measure(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.deleteMeasure";
    let name = required_str(p, CMD, "name")?;
    let mut model = edit_model(s, CMD)?;
    let index = measure_or_error(&model, CMD, name)?;
    let dependent = dependents(&model, CMD, name)?;
    if !dependent.is_empty() {
        return Err(dangling_reference_error(CMD, "delete", name, &dependent));
    }
    model.measures.remove(index);
    commit(s, CMD, model)
}

fn add_relationship(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.addRelationship";
    let from_table = required_str(p, CMD, "from_table")?;
    let from_column = required_str(p, CMD, "from_column")?;
    let to_table = required_str(p, CMD, "to_table")?;
    let to_column = required_str(p, CMD, "to_column")?;
    let mut model = edit_model(s, CMD)?;
    let from_table = known_table(&model, CMD, from_table)?;
    let from_column = known_column(&model, CMD, &from_table, from_column)?;
    let to_table = known_table(&model, CMD, to_table)?;
    let to_column = known_column(&model, CMD, &to_table, to_column)?;
    if model.relationships.iter().any(|existing| same_ends(existing, &from_table, &from_column, &to_table, &to_column)) {
        return Err(bad(CMD, format!("a relationship from {from_table}[{from_column}] to {to_table}[{to_column}] already exists")));
    }
    model.relationships.push(Relationship {
        from_table,
        from_column,
        to_table,
        to_column,
        bidirectional: bool_param(p, "bidirectional").unwrap_or(false),
        inactive: bool_param(p, "inactive").unwrap_or(false),
    });
    commit(s, CMD, model)
}

fn delete_relationship(s: &mut Session, p: &Json) -> Result<Json> {
    const CMD: &str = "dataModel.deleteRelationship";
    let from_table = required_str(p, CMD, "from_table")?;
    let from_column = required_str(p, CMD, "from_column")?;
    let to_table = required_str(p, CMD, "to_table")?;
    let to_column = required_str(p, CMD, "to_column")?;
    let mut model = edit_model(s, CMD)?;
    let index = model
        .relationships
        .iter()
        .position(|existing| same_ends(existing, from_table, from_column, to_table, to_column))
        .ok_or_else(|| bad(CMD, format!("no relationship from {from_table}[{from_column}] to {to_table}[{to_column}]")))?;
    model.relationships.remove(index);
    commit(s, CMD, model)
}

fn value_json(value: &gridcraft_dax::Scalar) -> Json {
    serde_json::to_value(value).unwrap_or(Json::Null)
}

fn table_json(table: &gridcraft_dax::TableValue) -> Json {
    json!({
        "columns": table.columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>(),
        "rows": table.rows.iter().map(|row| row.iter().map(value_json).collect::<Vec<_>>()).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A model in its own temp directory, so tests that run in parallel can't delete each other's
    /// model file.
    fn write_model(name: &str, json: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("gridcraft-dax-model-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("model.json");
        std::fs::write(&path, json).expect("write model");
        (path, dir.to_string_lossy().into_owned())
    }

    /// [`write_model`] with the standard one-table model.
    fn fixture(name: &str) -> (std::path::PathBuf, String) {
        write_model(name, SALES_MODEL)
    }

    /// The one-table model the load/evaluate/pivot tests run against.
    const SALES_MODEL: &str = r#"{
        "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                    "rows":[["East",10],["West",20],["East",30]]}],
        "relationships": [],
        "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
    }"#;

    #[test]
    fn load_status_evaluate_and_pivot() {
        let (path, dir) = fixture("load-status-evaluate-pivot");
        let mut s = Session::new();
        let status = s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");
        assert_eq!(status["tables"][0]["name"], "Sales");
        assert_eq!(s.execute("dataModel.status", json!({})).expect("status")["measures"][0]["name"], "Total Sales");

        let value = s.execute("dataModel.evaluate", json!({"formula": "SUM(Sales[Amount])"})).expect("eval");
        assert_eq!(value["value"], 60.0);
        let value = s.execute("dataModel.evaluate", json!({"measure": "Total Sales"})).expect("measure");
        assert_eq!(value["value"], 60.0);

        let table = s.execute("dataModel.evaluate", json!({"formula": "FILTER(Sales, Sales[Amount] > 10)", "table": true})).expect("table");
        assert_eq!(table["rows"].as_array().expect("rows").len(), 2);

        let pivot = s.execute("dataModel.pivot", json!({"measure": "Total Sales", "rows": ["Sales[Region]"]})).expect("pivot");
        assert_eq!(pivot["rows"][0][0], "East");
        assert_eq!(pivot["rows"][0][1], 40.0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_model_and_bad_formula_report_errors() {
        let mut s = Session::new();
        assert!(s.execute("dataModel.evaluate", json!({"formula": "1"})).is_err());
        let (path, dir) = fixture("missing-model-and-bad-formula");
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");
        assert!(s.execute("dataModel.evaluate", json!({"formula": "SUM(Missing[Amount])"})).is_err());
        assert!(s.execute("dataModel.pivot", json!({"measure": "Missing", "rows": ["Sales[Region]"]})).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn measure<'a>(status: &'a Json, name: &str) -> &'a Json {
        status["measures"].as_array().expect("measures").iter().find(|m| m["name"] == name).expect("measure")
    }

    #[test]
    fn measure_add_update_and_delete() {
        let (path, dir) = write_model(
            "measure-edit",
            r#"{
                "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                            "rows":[["East",10],["West",20],["East",30]]}],
                "relationships": [],
                "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
            }"#,
        );
        let mut s = Session::new();
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Double Sales", "formula": "1"})).is_err(), "no model loaded yet");
        assert!(s.commands().iter().any(|c| c.id == "dataModel.addMeasure" && !c.enabled), "the edit commands are greyed out with no model");
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");
        assert!(s.commands().iter().any(|c| c.id == "dataModel.addMeasure" && c.enabled), "the edit commands are live with a model");

        let status = s
            .execute(
                "dataModel.addMeasure",
                json!({"name": "Double Sales", "formula": "SUM(Sales[Amount]) * 2", "table": "sales", "format": "#,##0"}),
            )
            .expect("add");
        assert_eq!(status["measures"].as_array().expect("measures").len(), 2);
        assert_eq!(measure(&status, "Double Sales")["table"], "Sales", "the measure keeps the model's spelling of the table");
        assert_eq!(measure(&status, "Double Sales")["format"], "#,##0");
        assert!(measure(&status, "Total Sales")["format"].is_null());
        assert_eq!(s.execute("dataModel.evaluate", json!({"measure": "Double Sales"})).expect("eval")["value"], 120.0);
        assert_eq!(s.execute("dataModel.pivot", json!({"measure": "Double Sales", "rows": ["Sales[Region]"]})).expect("pivot")["rows"][0][1], 80.0);

        assert!(s.execute("dataModel.addMeasure", json!({"name": "double sales", "formula": "1"})).is_err(), "duplicate name");
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Bad", "formula": "1", "table": "Missing"})).is_err(), "unknown table");
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Bad", "formula": "SUM("})).is_err(), "unparsable formula");
        assert!(s.execute("dataModel.addMeasure", json!({"formula": "1"})).is_err(), "`name` is required");
        assert!(s.execute("dataModel.addMeasure", json!({"name": "Bad", "formula": "  "})).is_err(), "a blank formula is required too");
        assert_eq!(
            s.execute("dataModel.status", json!({})).expect("status")["measures"].as_array().expect("measures").len(),
            2,
            "a rejected add changes nothing"
        );

        let status = s
            .execute(
                "dataModel.updateMeasure",
                json!({"name": "double sales", "formula": "SUM(Sales[Amount]) * 3", "new_name": "Triple Sales", "format": "0.00"}),
            )
            .expect("update");
        assert_eq!(measure(&status, "Triple Sales")["format"], "0.00");
        assert_eq!(measure(&status, "Triple Sales")["formula"], "SUM(Sales[Amount]) * 3");
        assert_eq!(s.execute("dataModel.evaluate", json!({"measure": "Triple Sales"})).expect("eval")["value"], 180.0);
        assert!(s.execute("dataModel.evaluate", json!({"measure": "Double Sales"})).is_err(), "the old name is gone");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Missing", "formula": "1"})).is_err(), "unknown measure");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "new_name": "Total Sales"})).is_err(), "duplicate new name");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "formula": "SUM("})).is_err(), "unparsable formula");
        assert!(s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "new_name": "  "})).is_err(), "blank new name");
        assert_eq!(
            s.execute("dataModel.evaluate", json!({"measure": "Triple Sales"})).expect("eval")["value"],
            180.0,
            "a rejected update keeps the old formula"
        );
        let status = s.execute("dataModel.updateMeasure", json!({"name": "Triple Sales", "format": null})).expect("clear format");
        assert!(measure(&status, "Triple Sales")["format"].is_null(), "`format: null` clears the format");

        let status = s.execute("dataModel.deleteMeasure", json!({"name": "triple sales"})).expect("delete");
        assert_eq!(status["measures"].as_array().expect("measures").len(), 1);
        assert!(s.execute("dataModel.evaluate", json!({"measure": "Triple Sales"})).is_err(), "the deleted measure is gone");
        assert!(s.execute("dataModel.deleteMeasure", json!({"name": "Triple Sales"})).is_err(), "already deleted");
        assert!(s.execute("dataModel.deleteMeasure", json!({"name": "Total Sales"})).is_ok(), "the last measure can go");
        assert!(s.execute("dataModel.status", json!({})).expect("status")["measures"].as_array().expect("measures").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relationship_add_and_delete_with_status_types() {
        let (path, dir) = write_model(
            "relationship-edit",
            r#"{
                "tables": [
                    {"name":"Sales","columns":[{"name":"Region","data_type":"String"},{"name":"Product","data_type":"String"},{"name":"Amount","data_type":"Double"}],
                     "rows":[["East","Widget",10],["West","Gadget",20]]},
                    {"name":"Products","columns":[{"name":"Name","data_type":"String"},{"name":"Price","data_type":"Double"}],
                     "rows":[["Widget",3],["Gadget",5]]}
                ],
                "relationships": [],
                "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
            }"#,
        );
        let mut s = Session::new();
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");

        let status = s.execute("dataModel.status", json!({})).expect("status");
        assert_eq!(status["tables"][0]["columns"][2]["type"], "Double", "a column reports its declared model type");
        assert_eq!(status["tables"][1]["columns"][0]["name"], "Name");
        assert!(status["relationships"].as_array().expect("relationships").is_empty());

        let status = s
            .execute(
                "dataModel.addRelationship",
                json!({"from_table": "sales", "from_column": "product", "to_table": "Products", "to_column": "name"}),
            )
            .expect("add relationship");
        let relationship = &status["relationships"][0];
        assert_eq!(relationship["from_table"], "Sales", "the relationship keeps the model's spelling");
        assert_eq!(relationship["from_column"], "Product");
        assert_eq!(relationship["to_table"], "Products");
        assert_eq!(relationship["to_column"], "Name");
        assert!(!relationship["bidirectional"].as_bool().expect("bool"));
        assert!(!relationship["inactive"].as_bool().expect("bool"));

        assert!(
            s.execute(
                "dataModel.addRelationship",
                json!({"from_table": "Sales", "from_column": "Product", "to_table": "Products", "to_column": "Name"})
            )
            .is_err(),
            "duplicate relationship"
        );
        assert!(
            s.execute(
                "dataModel.addRelationship",
                json!({"from_table": "Sales", "from_column": "Nope", "to_table": "Products", "to_column": "Name"})
            )
            .is_err(),
            "unknown column"
        );
        assert!(
            s.execute("dataModel.addRelationship", json!({"from_table": "Missing", "from_column": "X", "to_table": "Products", "to_column": "Name"}))
                .is_err(),
            "unknown table"
        );
        assert!(
            s.execute("dataModel.addRelationship", json!({"from_table": "Sales", "from_column": "Product", "to_table": "Products"})).is_err(),
            "`to_column` is required"
        );
        assert_eq!(s.execute("dataModel.status", json!({})).expect("status")["relationships"].as_array().expect("relationships").len(), 1);
        // Evaluate while the only relationship is one-directional: the evaluator's filter
        // propagation recurses forever on a bidirectional relationship (see `dax::eval::table`),
        // which is a pre-existing dax-crate bug, not something this command introduces.
        assert_eq!(s.execute("dataModel.evaluate", json!({"measure": "Total Sales"})).expect("eval")["value"], 30.0);

        let status = s
            .execute(
                "dataModel.addRelationship",
                json!({"from_table": "Sales", "from_column": "Region", "to_table": "Products", "to_column": "Name", "bidirectional": true, "inactive": true}),
            )
            .expect("second relationship");
        assert!(status["relationships"][1]["bidirectional"].as_bool().expect("bool"));
        assert!(status["relationships"][1]["inactive"].as_bool().expect("bool"));

        let status = s
            .execute(
                "dataModel.deleteRelationship",
                json!({"from_table": "sales", "from_column": "product", "to_table": "products", "to_column": "name"}),
            )
            .expect("delete relationship");
        assert_eq!(status["relationships"].as_array().expect("relationships").len(), 1);
        assert_eq!(status["relationships"][0]["from_column"], "Region", "the other relationship survives");
        assert!(
            s.execute(
                "dataModel.deleteRelationship",
                json!({"from_table": "Sales", "from_column": "Product", "to_table": "Products", "to_column": "Name"})
            )
            .is_err(),
            "already deleted"
        );
        assert!(
            s.execute(
                "dataModel.deleteRelationship",
                json!({"from_table": "Sales", "from_column": "Region", "to_table": "Products", "to_column": "Name"})
            )
            .is_ok()
        );
        assert!(s.execute("dataModel.status", json!({})).expect("status")["relationships"].as_array().expect("relationships").is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn measure_edits_reject_dangling_references() {
        let json = r#"{
            "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                        "rows":[["East",10],["West",20]]}],
            "relationships": [],
            "measures": [
                {"name":"Base","formula":"SUM(Sales[Amount])","table":"Sales"},
                {"name":"Derived","formula":"[Base] * 2","table":"Sales"}
            ]
        }"#;
        let (path, dir) = write_model("dangling", json);
        let mut s = Session::new();
        s.execute("dataModel.load", json!({"path": path.to_string_lossy()})).expect("load");

        let error = s.execute("dataModel.updateMeasure", json!({"name":"Base","new_name":"Renamed"})).expect_err("rename must fail");
        assert!(error.to_string().contains("Derived"), "{error}");
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Base"})).is_err());

        // Removing the dependent first makes the rename and deletion valid.
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Derived"})).is_ok());
        assert!(s.execute("dataModel.updateMeasure", json!({"name":"Base","new_name":"Renamed"})).is_ok());
        assert!(s.execute("dataModel.deleteMeasure", json!({"name":"Renamed"})).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
