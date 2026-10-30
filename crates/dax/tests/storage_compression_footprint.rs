//! Memory-accounting tests: per-column encoded versus logical bytes and the model total.
//!
//! The store exposes [`gridcraft_dax::storage::StorageFootprint`] (via `ModelStorage::footprint`)
//! with one [`gridcraft_dax::storage::ColumnFootprint`] per column. These tests pin the numbers that
//! the encoding decisions are supposed to produce:
//!
//! * a 100k-row run-length column is at least an order of magnitude smaller than its full-width
//!   payload;
//! * a 100k-row low-cardinality text column is several times smaller;
//! * a constant text column stores no code bytes at all;
//! * a high-entropy column never grows beyond its payload plus its null bitmap;
//! * the totals are the sums of the parts, and the ratio is the model-level claim.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::storage::{ColumnKind, ModelStorage, StorageFootprint};
use gridcraft_dax::value::Scalar;

const ROWS: usize = 100_000;
const REGIONS: [&str; 8] = ["East", "West", "North", "South", "Centre", "Coastal", "Inland", "Metro"];

fn model() -> DataModel {
    let mut rows = Vec::with_capacity(ROWS);
    for row in 0..ROWS {
        rows.push(vec![
            Scalar::Integer((row / 1_000) as i64),
            Scalar::String(REGIONS[row % REGIONS.len()].into()),
            Scalar::Integer(row as i64),
            Scalar::Currency((row / 1_000) as i64 * 10_000),
            Scalar::String("Constant".into()),
        ]);
    }
    DataModel {
        tables: vec![ModelTable {
            name: "Sales".into(),
            columns: vec![
                ModelColumn { name: "Block".into(), data_type: Some("Int64".into()) },
                ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                ModelColumn { name: "Price".into(), data_type: Some("Currency".into()) },
                ModelColumn { name: "Tag".into(), data_type: Some("String".into()) },
            ],
            rows,
        }],
        ..DataModel::default()
    }
}

fn find<'a>(footprint: &'a StorageFootprint, name: &str) -> &'a gridcraft_dax::ColumnFootprint {
    let table = footprint.tables.first().expect("one table");
    assert_eq!(table.table, "Sales");
    let index = match name {
        "Block" => 0,
        "Region" => 1,
        "Amount" => 2,
        "Price" => 3,
        "Tag" => 4,
        other => panic!("unknown column {other}"),
    };
    table.columns.get(index).expect("column footprint")
}

#[test]
fn run_length_and_packed_columns_account_for_far_less_than_their_logical_size() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let footprint = storage.footprint();

    let block = find(&footprint, "Block");
    assert_eq!(block.kind, ColumnKind::Integer);
    assert_eq!(block.rows, ROWS);
    assert_eq!(block.logical_bytes, ROWS * 8, "one i64 slot per row");
    // 100 runs: 100 values + 101 offsets = 1608 bytes, plus the 12.5 kB null bitmap.
    assert_eq!(block.encoded_bytes, 100 * 8 + 101 * 8 + ROWS.div_ceil(64) * 8);
    assert!(block.encoded_bytes * 10 < block.logical_bytes, "run-length encoding must be >10x smaller");
    assert_eq!(block.saved_bytes(), block.logical_bytes - block.encoded_bytes);

    let price = find(&footprint, "Price");
    assert_eq!(price.kind, ColumnKind::Currency);
    assert_eq!(price.logical_bytes, ROWS * 8);
    assert!(price.encoded_bytes * 10 < price.logical_bytes);

    let region = find(&footprint, "Region");
    assert_eq!(region.kind, ColumnKind::String);
    let dictionary_bytes: usize = REGIONS.iter().map(|name| name.len()).sum();
    assert_eq!(region.logical_bytes, ROWS * 4 + dictionary_bytes, "four bytes per plain code plus the labels");
    assert_eq!(region.encoded_bytes, (ROWS * 3).div_ceil(64) * 8 + dictionary_bytes + ROWS.div_ceil(64) * 8);
    assert!(region.encoded_bytes * 4 < region.logical_bytes, "3-bit codes must be several times smaller");
    assert!(region.ratio() < 0.2);
}

#[test]
fn a_constant_column_costs_no_code_bytes() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let footprint = storage.footprint();
    let tag = find(&footprint, "Tag");
    let dictionary_bytes = "Constant".len();
    assert_eq!(tag.logical_bytes, ROWS * 4 + dictionary_bytes);
    // Zero bits per code: only the labels and the null bitmap remain.
    assert_eq!(tag.encoded_bytes, dictionary_bytes + ROWS.div_ceil(64) * 8);
    assert!(tag.ratio() < 0.05, "a constant column stores the label once: {}", tag.ratio());
    assert!(tag.saved_bytes() > ROWS * 3);
}

#[test]
fn a_high_entropy_column_does_not_grow_beyond_its_payload_and_bitmap() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let footprint = storage.footprint();
    let amount = find(&footprint, "Amount");
    assert_eq!(amount.kind, ColumnKind::Integer);
    assert_eq!(amount.logical_bytes, ROWS * 8);
    assert_eq!(amount.encoded_bytes, ROWS * 8 + ROWS.div_ceil(64) * 8, "plain values plus the null bitmap");
    // The stated bound: a high-entropy column never exceeds its logical payload plus its bitmap.
    assert!(amount.encoded_bytes <= amount.logical_bytes + ROWS.div_ceil(8) + 64);
    assert_eq!(amount.saved_bytes(), 0);
    assert!(amount.ratio() > 1.0, "the null bitmap is the only overhead");
}

#[test]
fn totals_are_the_sum_of_the_parts() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let footprint = storage.footprint();
    let table = footprint.tables.first().expect("one table");
    let summed_logical: usize = table.columns.iter().map(|column| column.logical_bytes).sum();
    let summed_encoded: usize = table.columns.iter().map(|column| column.encoded_bytes).sum();
    assert_eq!(table.logical_bytes(), summed_logical);
    assert_eq!(table.encoded_bytes(), summed_encoded);
    assert_eq!(footprint.logical_bytes(), summed_logical);
    assert_eq!(footprint.encoded_bytes(), summed_encoded);
    assert_eq!(footprint.saved_bytes(), summed_logical - summed_encoded);

    // Five columns, each reported once with its table name.
    let columns: Vec<(&str, usize)> = footprint.columns().map(|(table, index, _)| (table, index)).collect();
    assert_eq!(columns, vec![("Sales", 0), ("Sales", 1), ("Sales", 2), ("Sales", 3), ("Sales", 4)]);

    // The model total saves the run-length and packed columns even with the plain ones counted.
    assert!(footprint.ratio() < 0.6, "measured ratio {}", footprint.ratio());
    assert!(footprint.saved_bytes() > ROWS * 8, "at least one full-width column's worth of savings");
}

#[test]
fn accounting_is_reproducible() {
    let first = ModelStorage::from_model(&model()).expect("storage").footprint();
    let second = ModelStorage::from_model(&model()).expect("storage").footprint();
    assert_eq!(first, second);
}
