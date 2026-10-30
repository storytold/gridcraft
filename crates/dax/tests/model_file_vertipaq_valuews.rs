//! Excel-survivable writes of modified values in value-encoded VertiPaq columns.
//!
//! Value encoding ties a value to a data id (`value = (id + BaseId) * Magnitude`), and Excel
//! couples that id space with the hierarchy's `POS_TO_ID`/`ID_TO_POS` arrays and the
//! `XMHierarchyDataID2PositionHashIndex` (`.hidx`) lookup. A write that changes a value must
//! rebuild all of them together; a write that changes nothing must leave every stored byte alone.
//!
//! Set `GRIDCRAFT_VERTIPAQ_FIXTURE` to an extracted Excel `xl/model/item.data` to run these
//! tests; without it they are skipped so the suite stays machine independent.

use gridcraft_dax::model_file::{
    ModelColumnData, ModelData, ModelMeasure, column_metrics, file_contents, read_measures, read_model, write_measures, write_model,
};
use gridcraft_dax::value::Scalar;

fn fixture() -> Option<Vec<u8>> {
    let path = std::env::var("GRIDCRAFT_VERTIPAQ_FIXTURE").ok()?;
    std::fs::read(path).ok()
}

fn table<'a>(model: &'a ModelData, name: &str) -> &'a gridcraft_dax::model_file::ModelTableData {
    model.tables.iter().find(|table| table.name == name).unwrap_or_else(|| panic!("missing table {name}"))
}

/// Finds a value-encoded column that exists both in the model and in the metrics report.
fn value_column(model: &ModelData, base: &[u8]) -> (String, String) {
    let metrics = column_metrics(base).expect("metrics");
    for metric in metrics.iter().filter(|metric| metric.encoding == "value") {
        let Some(candidate) = model.tables.iter().find(|table| table.name == metric.table) else { continue };
        if candidate.columns.iter().any(|column| column.name == metric.column) {
            return (metric.table.clone(), metric.column.clone());
        }
    }
    panic!("fixture has no value-encoded column present in the model")
}

fn column_mut<'a>(model: &'a mut ModelData, table_name: &str, column_name: &str) -> &'a mut ModelColumnData {
    model
        .tables
        .iter_mut()
        .find(|table| table.name == table_name)
        .and_then(|table| table.columns.iter_mut().find(|column| column.name == column_name))
        .unwrap_or_else(|| panic!("missing {table_name}[{column_name}]"))
}

fn column<'a>(model: &'a ModelData, table_name: &str, column_name: &str) -> &'a ModelColumnData {
    table(model, table_name).columns.iter().find(|column| column.name == column_name).unwrap_or_else(|| panic!("missing column"))
}

/// True when a logical file name identifies a single column (its dictionary, `.idf`, `.hidx` or
/// hierarchy metadata) rather than the table-wide metadata that carries every column.
fn is_column_file(logical: &str, table_name: &str, column_name: &str) -> bool {
    let file = logical.rsplit(['\\', '/']).next().unwrap_or(logical);
    file.contains(&format!("${table_name}${column_name}")) || file.contains(&format!(".{column_name}."))
}

fn stored<'a>(files: &'a [(String, Vec<u8>)], logical: &str) -> Option<&'a [u8]> {
    files.iter().find(|(name, _)| name == logical).map(|(_, bytes)| bytes.as_slice())
}

fn numeric(value: &Scalar) -> Option<f64> {
    match value {
        Scalar::Integer(number) => Some(*number as f64),
        Scalar::Decimal(number) => Some(*number),
        Scalar::Currency(scaled) => Some(*scaled as f64 / 10_000.0),
        _ => None,
    }
}

fn with_numeric(value: &Scalar, number: f64) -> Scalar {
    match value {
        Scalar::Integer(_) => Scalar::Integer(number as i64),
        Scalar::Decimal(_) => Scalar::Decimal(number),
        Scalar::Currency(_) => Scalar::Currency((number * 10_000.0).round() as i64),
        other => other.clone(),
    }
}

/// Compares two models column by column and asserts that only the edited column's values moved;
/// table shape, every other column, the relationships and the measures must be untouched.
fn assert_only_column_changed(before: &ModelData, after: &ModelData, table_name: &str, column_name: &str) {
    assert_eq!(before.tables.len(), after.tables.len());
    for (old_table, new_table) in before.tables.iter().zip(&after.tables) {
        assert_eq!(old_table.name, new_table.name);
        assert_eq!(old_table.visible, new_table.visible);
        for (old_column, new_column) in old_table.columns.iter().zip(&new_table.columns) {
            assert_eq!(old_column.name, new_column.name);
            assert_eq!(old_column.data_type, new_column.data_type);
            if old_table.name != table_name || old_column.name != column_name {
                assert_eq!(old_column.values, new_column.values, "untouched column {}[{}] changed", old_table.name, old_column.name);
            }
        }
    }
    assert_eq!(before.relationships, after.relationships);
    assert_eq!(before.measures, after.measures);
}

#[test]
fn changing_a_value_to_another_existing_value_round_trips() {
    let Some(base) = fixture() else { return };
    let original = read_model(&base).expect("read model");
    let (table_name, column_name) = value_column(&original, &base);
    let mut model = original.clone();
    let target = column_mut(&mut model, &table_name, &column_name);
    // Point one row at another row's value: the id set is unchanged, so only the column's `.idf`
    // has to move, while the hierarchy and its hash index stay exactly as Excel wrote them.
    let rows: Vec<usize> = target.values.iter().enumerate().filter(|(_, value)| numeric(value).is_some()).map(|(row, _)| row).collect();
    assert!(rows.len() >= 2, "value-encoded column needs two numeric rows");
    let source = target.values[rows[0]].clone();
    let destination = target.values[rows[1]].clone();
    assert_ne!(numeric(&source), numeric(&destination));
    target.values[rows[1]] = source.clone();

    let written = write_model(&base, &model).expect("write");
    let back = read_model(&written).expect("read back");
    assert_eq!(column(&back, &table_name, &column_name).values[rows[1]], source);
    assert_only_column_changed(&model, &back, &table_name, &column_name);

    // The hierarchy itself did not change ids, so Excel's hash index must still be valid byte for
    // byte: this is the strictest form of the "no unnecessary churn" requirement.
    let before_files = file_contents(&base).expect("base files");
    let after_files = file_contents(&written).expect("written files");
    for (logical, content) in &before_files {
        let file = logical.rsplit(['\\', '/']).next().unwrap_or(logical);
        if file.starts_with(&format!("H${table_name}${column_name}")) && file.ends_with(".hidx") {
            assert_eq!(stored(&after_files, logical), Some(content.as_slice()), "an unchanged id set must keep the hash index");
        }
    }
}

#[test]
fn growing_a_value_encoded_column_rebuilds_the_coupled_structures() {
    let Some(base) = fixture() else { return };
    let mut model = read_model(&base).expect("read model");
    let (table_name, column_name) = value_column(&model, &base);
    let target = column_mut(&mut model, &table_name, &column_name);
    let row = target.values.iter().position(|value| numeric(value).is_some()).unwrap_or_else(|| panic!("value-encoded column needs a numeric row"));
    let old = numeric(&target.values[row]).expect("numeric");
    // A value beyond the current maximum grows the value-id space (a new BaseId or a new high id).
    let candidate = target.values.iter().filter_map(numeric).fold(old, f64::max) + 1_000_000.0;
    target.values[row] = with_numeric(&target.values[row], candidate);

    let written = write_model(&base, &model).expect("write");
    let back = read_model(&written).expect("read back");
    assert_eq!(numeric(&column(&back, &table_name, &column_name).values[row]), Some(candidate));
    assert_only_column_changed(&model, &back, &table_name, &column_name);
    let again = write_model(&written, &back).expect("second write");
    assert_eq!(again, written, "re-writing the same model must be byte-identical");

    // Every column file other than the edited column keeps its exact stored bytes.
    let before_files = file_contents(&base).expect("base files");
    let after_files = file_contents(&written).expect("written files");
    for (logical, content) in &before_files {
        let file = logical.rsplit(['\\', '/']).next().unwrap_or(logical);
        let shared = file == "PARTITIONS"
            || file.starts_with("LOG")
            || file.ends_with(".dim.xml")
            || file.ends_with(".cub.xml")
            || file.ends_with(".scr.xml")
            || (file.ends_with(".tbl.xml") && !file.starts_with("H$") && !file.starts_with("R$"));
        if shared {
            continue;
        }
        let belongs_to_edited = is_column_file(logical, &table_name, &column_name);
        let belongs_to_column =
            model.tables.iter().any(|table| table.columns.iter().any(|column| is_column_file(logical, &table.name, &column.name)));
        if !belongs_to_edited && belongs_to_column {
            assert_eq!(stored(&after_files, logical), Some(content.as_slice()), "column file {logical} changed");
        }
    }
    // The edited column's hash index is rebuilt and still carries the observed Excel layout.
    let hidx = after_files
        .iter()
        .find(|(logical, _)| is_column_file(logical, &table_name, &column_name) && logical.ends_with(".hidx"))
        .map(|(_, content)| content.as_slice())
        .expect("value column hash index");
    let word = |offset: usize| u64::from_le_bytes(hidx[offset..offset + 8].try_into().expect("word"));
    let bins = word(16);
    assert!(bins.is_power_of_two());
    assert_eq!(word(32), bins - 1, "bucket mask");
    assert_eq!(hidx.len() as u64, 41 + 128 + bins * 64 + 20, "Excel's fixed block layout");
    assert_eq!(hidx[40], 1, "statistics present");
}

#[test]
fn lowering_the_minimum_rebases_every_data_id() {
    let Some(base) = fixture() else { return };
    let mut model = read_model(&base).expect("read model");
    let (table_name, column_name) = value_column(&model, &base);
    let target = column_mut(&mut model, &table_name, &column_name);
    let row = target.values.iter().position(|value| numeric(value).is_some()).unwrap_or_else(|| panic!("value-encoded column needs a numeric row"));
    let minimum = target.values.iter().filter_map(numeric).fold(f64::INFINITY, f64::min);
    assert!(minimum.is_finite());
    // A value below the current minimum lowers the value dictionary's BaseId, which shifts every
    // existing data id: the payload, POS_TO_ID and the hash index must all move together.
    let candidate = minimum - 10_000.0;
    target.values[row] = with_numeric(&target.values[row], candidate);
    let written = write_model(&base, &model).expect("write");
    let back = read_model(&written).expect("read back");
    assert_eq!(numeric(&column(&back, &table_name, &column_name).values[row]), Some(candidate));
    assert_only_column_changed(&model, &back, &table_name, &column_name);
    assert_eq!(write_model(&written, &back).expect("second write"), written);
}

#[test]
fn blank_and_non_blank_transitions_round_trip() {
    let Some(base) = fixture() else { return };
    let mut model = read_model(&base).expect("read model");
    let (table_name, column_name) = value_column(&model, &base);
    let target = column_mut(&mut model, &table_name, &column_name);
    let row = target.values.iter().position(|value| numeric(value).is_some()).unwrap_or_else(|| panic!("value-encoded column needs a numeric row"));
    let removed = target.values[row].clone();
    target.values[row] = Scalar::Blank;

    let written = write_model(&base, &model).expect("write blank");
    let back = read_model(&written).expect("read blank");
    assert_eq!(column(&back, &table_name, &column_name).values[row], Scalar::Blank);

    // Restore a value from the blank state; the writer must add the id back to the hierarchy.
    let mut restored = back.clone();
    column_mut(&mut restored, &table_name, &column_name).values[row] = removed.clone();
    let rewritten = write_model(&written, &restored).expect("write restored");
    let after = read_model(&rewritten).expect("read restored");
    assert_eq!(column(&after, &table_name, &column_name).values[row], removed);
    assert_only_column_changed(&restored, &after, &table_name, &column_name);
}

#[test]
fn a_no_op_rewrite_is_stable_and_touches_only_the_script() {
    let Some(base) = fixture() else { return };
    let model = read_model(&base).expect("read model");
    let written = write_model(&base, &model).expect("write");
    let back = read_model(&written).expect("read back");
    assert_eq!(back, model);
    assert_eq!(write_model(&written, &back).expect("second write"), written, "same inputs must produce byte-identical output");

    // No-op means no column payload, dictionary, hierarchy or hash index changes.
    let before_files = file_contents(&base).expect("base files");
    let after_files = file_contents(&written).expect("written files");
    assert_eq!(before_files.len(), after_files.len());
    for (logical, content) in &before_files {
        let file = logical.rsplit(['\\', '/']).next().unwrap_or(logical);
        if file.starts_with("LOG") || file == "PARTITIONS" || file.ends_with(".scr.xml") {
            continue;
        }
        assert_eq!(stored(&after_files, logical), Some(content.as_slice()), "no-op write changed {logical}");
    }
}

/// Opt-in harness: writes a part with exactly one changed value plus the expectation JSON the
/// Excel oracle run compares against.
///
/// `GRIDCRAFT_VERTIPAQ_EXCEL_OUTPUT` is the `.data` path and `GRIDCRAFT_VERTIPAQ_EXCEL_EXPECT` the
/// JSON path. The edited column is the first value-encoded column the model exposes.
#[test]
fn opt_in_export_single_value_for_the_excel_oracle() {
    let Some(base) = fixture() else { return };
    let Ok(output) = std::env::var("GRIDCRAFT_VERTIPAQ_EXCEL_OUTPUT") else { return };
    let mut model = read_model(&base).expect("read model");
    let (table_name, column_name) = value_column(&model, &base);
    let target = column_mut(&mut model, &table_name, &column_name);
    let row = target
        .values
        .iter()
        .position(|value| numeric(value).is_some_and(|number| (number - 50.0).abs() < 1.0e-9))
        .or_else(|| target.values.iter().position(|value| numeric(value).is_some()))
        .unwrap_or_else(|| panic!("value-encoded column needs a numeric row"));
    let old = target.values[row].clone();
    let new = with_numeric(&old, 777.0);
    target.values[row] = new.clone();
    let written = write_model(&base, &model).expect("write");
    // The Excel oracle reads the value through a pivot, so the part optionally carries one
    // measure; `write_measures` only rewrites the MDX script and the backup log.
    let written = if std::env::var("GRIDCRAFT_VERTIPAQ_EXCEL_MEASURE").is_ok() {
        let mut measures = read_measures(&written).expect("read measures");
        measures.push(ModelMeasure {
            table: table_name.clone(),
            name: "ValueWS Total Amount".into(),
            formula: format!("SUM({table_name}[{column_name}])"),
            format: "#,##0.00".into(),
        });
        write_measures(&written, &measures).expect("write measure")
    } else {
        written
    };
    std::fs::write(&output, &written).expect("write output");
    let sum: f64 = column(&model, &table_name, &column_name).values.iter().filter_map(numeric).sum();
    if let Ok(path) = std::env::var("GRIDCRAFT_VERTIPAQ_EXCEL_EXPECT") {
        let expectation = serde_json::json!({
            "table": table_name,
            "column": column_name,
            "row": row,
            "old": numeric(&old),
            "new": numeric(&new),
            "sum": sum,
        });
        std::fs::write(path, serde_json::to_vec_pretty(&expectation).expect("serialize")).expect("write expectation");
    }
}

/// Opt-in harness: reads the part Excel saved after opening the part this writer produced (and,
/// when the workbook has no linked source, refreshing and re-saving it).
///
/// `GRIDCRAFT_VERTIPAQ_RESAVED` is the extracted `xl/model/item.data`, `GRIDCRAFT_VERTIPAQ_EXCEL_EXPECT`
/// the expectation JSON written by [`opt_in_export_single_value_for_the_excel_oracle`].
#[test]
fn opt_in_excel_resaved_part_reads_back() {
    let Ok(path) = std::env::var("GRIDCRAFT_VERTIPAQ_RESAVED") else { return };
    let data = std::fs::read(path).expect("read resaved part");
    let model = read_model(&data).expect("read resaved model");
    let Ok(expectation) = std::env::var("GRIDCRAFT_VERTIPAQ_EXCEL_EXPECT") else { return };
    let expectation: serde_json::Value = serde_json::from_slice(&std::fs::read(expectation).expect("read expectation")).expect("parse expectation");
    let table_name = expectation["table"].as_str().expect("table");
    let column_name = expectation["column"].as_str().expect("column");
    let expected = expectation["new"].as_f64().expect("new");
    let discarded = expectation["old"].as_f64().expect("old");
    let values = &column(&model, table_name, column_name).values;
    let present: Vec<f64> = values.iter().filter_map(numeric).collect();
    assert!(present.iter().any(|number| (number - expected).abs() < 1.0e-9), "Excel's re-saved part lost the changed value {expected}");
    if (expected - discarded).abs() > 1.0e-9 {
        assert!(
            !present.iter().any(|number| (number - discarded).abs() < 1.0e-9),
            "Excel's re-saved part still holds the replaced value {discarded}"
        );
    }
    let sum: f64 = present.iter().sum();
    let expected_sum = expectation["sum"].as_f64().expect("sum");
    assert!((sum - expected_sum).abs() < 1.0e-6, "sum {sum} differs from the expected {expected_sum}");
}
