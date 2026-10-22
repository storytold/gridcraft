//! Data Model (Power Pivot prototype) commands: load a JSON model, evaluate DAX, and group a
//! measure into a pivot-shaped table.

use gridcraft_dax::{Binder, DataModel, Evaluator, parse_expression, pivot_cross, pivot_fields};
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
    ]
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
    let measures: Vec<Json> =
        model.measures.iter().map(|measure| json!({"name": measure.name, "table": measure.table, "formula": measure.formula})).collect();
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

    fn fixture(name: &str) -> (std::path::PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("gridcraft-dax-model-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("model.json");
        let json = r#"{
            "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                        "rows":[["East",10],["West",20],["East",30]]}],
            "relationships": [],
            "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
        }"#;
        std::fs::write(&path, json).expect("write model");
        (path, dir.to_string_lossy().into_owned())
    }

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
}
