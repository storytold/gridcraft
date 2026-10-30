//! VertiPaq reading, metrics and container-robustness tests.
//!
//! The Excel-authored fixtures are never committed. Set `GRIDCRAFT_VERTIPAQ_FIXTURE` to an
//! extracted `xl/model/item.data` to run the oracle-backed tests; the fuzz/truncation tests run
//! unconditionally.

use gridcraft_dax::model_file::{ModelData, column_metrics, is_model_file, read_model};
use gridcraft_dax::value::Scalar;

fn fixture() -> Option<Vec<u8>> {
    let path = std::env::var("GRIDCRAFT_VERTIPAQ_FIXTURE").ok()?;
    std::fs::read(path).ok()
}

fn table<'a>(model: &'a ModelData, name: &str) -> &'a gridcraft_dax::model_file::ModelTableData {
    model.tables.iter().find(|table| table.name == name).unwrap_or_else(|| panic!("missing table {name}"))
}

fn column<'a>(model: &'a ModelData, table_name: &str, name: &str) -> &'a gridcraft_dax::model_file::ModelColumnData {
    table(model, table_name).columns.iter().find(|column| column.name == name).unwrap_or_else(|| panic!("missing {table_name}[{name}]"))
}

#[test]
fn rejects_non_model_input_without_panicking() {
    for length in 0..4096usize {
        let data = vec![0xAB; length];
        assert!(read_model(&data).is_err());
        assert!(!is_model_file(&data));
    }
    let mut data = vec![0u8; 8192];
    data[0] = 0xFF;
    data[1] = 0xFE;
    assert!(read_model(&data).is_err());
}

#[test]
#[allow(clippy::cognitive_complexity)] // The lint budget is for the engine; tests keep one readable assertion table.
fn reads_an_excel_authored_model() {
    let Some(data) = fixture() else { return };
    assert!(is_model_file(&data));
    let model = read_model(&data).expect("read model");
    assert!(!model.tables.is_empty());
    for table in &model.tables {
        assert!(!table.name.is_empty());
        assert!(!table.columns.iter().any(|column| column.name.contains("RowNumber")));
        let rows = table.columns.first().map(|column| column.values.len()).unwrap_or(0);
        assert!(table.columns.iter().all(|column| column.values.len() == rows), "{} columns disagree on the row count", table.name);
        assert!(table.columns.iter().all(|column| !column.data_type.is_empty()));
    }
    assert!(!model.measures.iter().any(|measure| measure.name.starts_with("_Count ")));

    if model.tables.iter().any(|table| table.name == "Sales") {
        let sales = table(&model, "Sales");
        assert_eq!(sales.columns.len(), 8);
        let region = column(&model, "Sales", "Region");
        assert_eq!(region.data_type, "String");
        assert_eq!(region.values.len(), 16);
        let expected_region =
            ["East", "West", "North", "South", "East", "West", "North", "South", "East", "West", "North", "South", "East", "West", "North", "South"];
        assert!(region.values.iter().zip(expected_region).all(|(value, expected)| *value == Scalar::String(expected.to_string())));
        let amount = column(&model, "Sales", "Amount");
        assert_eq!(amount.data_type, "Int64");
        assert_eq!(amount.values[0], Scalar::Integer(100));
        assert_eq!(amount.values[4], Scalar::Blank);
        assert_eq!(amount.values[15], Scalar::Integer(80));
        assert_eq!(column(&model, "Sales", "Text").values[2], Scalar::Blank);
        assert_eq!(column(&model, "Sales", "Flag").data_type, "Boolean");
        assert_eq!(column(&model, "Sales", "Flag").values[0], Scalar::Boolean(true));
        assert_eq!(column(&model, "Sales", "Flag").values[1], Scalar::Boolean(false));
        assert_eq!(column(&model, "Sales", "Date").data_type, "DateTime");
        assert_eq!(column(&model, "Sales", "Date").values[0], Scalar::DateTime(45296.0));
        assert_eq!(column(&model, "Products", "Product").values[0], Scalar::String("Widget".into()));
        assert_eq!(column(&model, "Customers", "Segment").values[1], Scalar::String("SMB".into()));
        assert_eq!(table(&model, "Date").columns.iter().map(|column| column.values.len()).max(), Some(731));
        assert_eq!(model.relationships.len(), 3);
    } else if model.tables.iter().any(|table| table.name == "TheTable") {
        let sheet = table(&model, "TheTable");
        assert_eq!(sheet.columns.len(), 5);
        assert_eq!(column(&model, "TheTable", "A").values[0], Scalar::Integer(5));
        assert_eq!(column(&model, "TheTable", "N").values[0], Scalar::Integer(15));
        assert_eq!(column(&model, "TheTable", "C").data_type, "Currency");
        assert_eq!(column(&model, "TheTable", "C").values[0], Scalar::Blank);
        assert_eq!(column(&model, "TheTable", "C").values[100], Scalar::Currency(100));
        assert_eq!(column(&model, "TheTable", "S").values[0], Scalar::String("s5".into()));
        assert_eq!(column(&model, "TheTable", "K").values[0], Scalar::Integer(10));
    } else if model.tables.iter().any(|table| table.name == "Metrics") {
        assert_eq!(model.tables.len(), 8);
        assert!(!table(&model, "Metrics").visible);
        assert_eq!(table(&model, "Metrics").columns[0].values.len(), 6145);
        assert_eq!(model.relationships.len(), 7);
        assert!(model.measures.len() >= 2);
        assert!(column(&model, "Metrics", "Defect Qty").data_type == "Double");
    }
}

#[test]
fn reads_the_reference_metrics_ranges() {
    let Some(data) = fixture() else { return };
    let metrics = column_metrics(&data).expect("metrics");
    assert!(!metrics.is_empty());
    for metric in &metrics {
        assert!(!metric.column.contains("RowNumber"), "row-number column must not be surfaced");
        assert_eq!(metric.cardinality > 0, metric.rows > 0);
        assert!(metric.cardinality >= usize::from(metric.nulls), "{metric:?}");
        assert!(metric.encoded_bytes >= metric.segments * 16 || metric.segments == 0, "{metric:?}");
        assert!(!metric.bit_widths.is_empty());
        assert!(metric.bit_widths.iter().all(|width| *width >= 1 && *width <= 32));
    }
    if metrics.iter().any(|metric| metric.table == "Sales") {
        let amount = metrics.iter().find(|metric| metric.table == "Sales" && metric.column == "Amount").expect("Amount");
        assert_eq!(amount.rows, 16);
        assert_eq!(amount.cardinality, 16);
        assert!(amount.nulls);
        assert!(amount.dictionary_bytes == 0);
        let region = metrics.iter().find(|metric| metric.table == "Sales" && metric.column == "Region").expect("Region");
        assert!(region.dictionary_bytes > 0);
        assert!(region.stored_bytes > 0 && region.encoded_bytes > 0);
    } else if let Some(currency) = metrics.iter().find(|metric| metric.table == "TheTable" && metric.column == "C") {
        assert_eq!(currency.rows, 500);
        assert_eq!(currency.cardinality, 401);
        assert!(currency.nulls);
        assert_eq!(currency.dictionary_bytes, 0);
        assert_eq!(currency.segments, 1);
        assert_eq!(currency.bit_widths, vec![9]);
        assert_eq!(currency.mins, vec![2]);
        let text = metrics.iter().find(|metric| metric.table == "TheTable" && metric.column == "S").expect("S");
        assert!(text.dictionary_bytes > 0);
    }
}

#[test]
fn every_sampled_truncation_is_an_error() {
    let Some(data) = fixture() else { return };
    let mut cuts: Vec<usize> = (0..data.len().min(200)).collect();
    cuts.extend((0..=32).map(|offset| data.len().saturating_sub(offset)));
    let mut position = 4096usize;
    while position < data.len() {
        cuts.extend([position.saturating_sub(2), position, position + 1, position + 2]);
        position = position.saturating_add(4096);
    }
    cuts.sort_unstable();
    cuts.dedup();
    for cut in cuts.into_iter().filter(|cut| *cut < data.len()) {
        if data[cut..].iter().all(|byte| *byte == 0) {
            // Trailing page padding is not part of the stream's declared size; a reader may
            // accept a stream whose padding was cut away.
            continue;
        }
        assert!(read_model(&data[..cut]).is_err(), "prefix {cut} of {} bytes decoded", data.len());
    }
}

/// Opt-in metrics export used to compare against the specification's reference table.
#[test]
fn opt_in_export_metrics() {
    let Some(data) = fixture() else { return };
    let Ok(output) = std::env::var("GRIDCRAFT_VERTIPAQ_METRICS") else { return };
    let metrics = column_metrics(&data).expect("metrics");
    std::fs::write(output, serde_json::to_vec_pretty(&metrics).expect("serialize")).expect("write metrics");
}
