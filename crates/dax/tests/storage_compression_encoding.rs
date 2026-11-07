//! Compression round-trip and vectorized-scan tests for the columnar store.
//!
//! The fixture is the shape compression exists for: a 100k-row fact-like table whose keys repeat
//! in long blocks, whose labels have few distinct values, and whose measures are high entropy. The
//! tests prove three things:
//!
//! * the encoding selection is the one documented (`RleInteger`/`RleCurrency` for long runs,
//!   `PackedString` for few labels, plain vectors for high-entropy columns);
//! * decoding is exact — every value reads back, [`ModelStorage::is_faithful`] stays true, and every
//!   aggregate equals the value computed from the generator, including `sum_batch`/`sum_batches`
//!   and the allocation-free `for_each_value` scan;
//! * the same aggregate answers whether or not a selection is sorted, has duplicates, or is empty.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable};
use gridcraft_dax::storage::{ColumnData, ColumnKind, ColumnStorage, ModelStorage, SCAN_BATCH_ROWS, ValueRef};
use gridcraft_dax::value::Scalar;

const ROWS: usize = 100_000;
const REGIONS: [&str; 8] = ["East", "West", "North", "South", "Centre", "Coastal", "Inland", "Metro"];

fn block(row: usize) -> i64 {
    (row / 1_000) as i64
}

fn region(row: usize) -> &'static str {
    REGIONS[row % REGIONS.len()]
}

fn price(row: usize) -> i64 {
    (row / 1_000) as i64 * 10_000
}

fn model() -> DataModel {
    let mut rows = Vec::with_capacity(ROWS);
    for row in 0..ROWS {
        rows.push(vec![
            Scalar::Integer(block(row)),
            Scalar::String(region(row).into()),
            Scalar::Integer(row as i64),
            Scalar::Currency(price(row)),
            Scalar::String("Constant".into()),
            Scalar::Boolean(row % 2 == 0),
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
                ModelColumn { name: "Flag".into(), data_type: Some("Boolean".into()) },
            ],
            rows,
        }],
        ..DataModel::default()
    }
}

fn column_data(storage: &ModelStorage, name: &str) -> ColumnData {
    let table = storage.table(0).expect("table");
    table.column_by_name(name).expect("column").data().clone()
}

#[test]
fn low_cardinality_and_run_columns_compress_and_high_entropy_columns_do_not() {
    let model = model();
    let storage = ModelStorage::from_model(&model).expect("storage");
    assert!(storage.is_faithful(), "compression must stay lossless");

    assert!(matches!(column_data(&storage, "Block"), ColumnData::RleInteger { .. }), "1k-row runs should run-length encode");
    assert!(matches!(column_data(&storage, "Price"), ColumnData::RleCurrency { .. }), "currency runs should run-length encode");
    assert!(matches!(column_data(&storage, "Region"), ColumnData::PackedString { .. }), "8 labels should bit-pack");
    assert!(matches!(column_data(&storage, "Tag"), ColumnData::PackedString { .. }), "a constant label should bit-pack");
    assert!(matches!(column_data(&storage, "Amount"), ColumnData::Integer { .. }), "high-entropy integers stay plain");
    assert!(matches!(column_data(&storage, "Flag"), ColumnData::Boolean { .. }), "alternating booleans have no runs to save");

    let table = storage.table(0).expect("table");
    // A constant column needs zero bits per code.
    let tag = table.column_by_name("Tag").expect("Tag");
    if let ColumnData::PackedString { packed, .. } = tag.data() {
        assert_eq!(packed.bits_per_code(), 0);
        assert_eq!(packed.bytes(), 0);
    } else {
        panic!("Tag should be packed, got {:?}", tag.data());
    }
    let region = table.column_by_name("Region").expect("Region");
    if let ColumnData::PackedString { packed, .. } = region.data() {
        assert_eq!(packed.bits_per_code(), 3, "8 labels need 3 bits");
    } else {
        panic!("Region should be packed, got {:?}", region.data());
    }
}

#[test]
fn every_value_decodes_exactly_including_blanks() {
    let mut model = model();
    // Add blanks so the bitmap path is exercised under compression.
    for row in (0..ROWS).step_by(7) {
        if let Some(cells) = model.tables[0].rows.get_mut(row) {
            cells[0] = Scalar::Blank;
            cells[2] = Scalar::Blank;
            cells[3] = Scalar::Blank;
        }
    }
    let storage = ModelStorage::from_model(&model).expect("storage");
    assert!(storage.is_faithful());
    let table = storage.table(0).expect("table");
    for row in 0..ROWS {
        let expected = model.tables[0].rows.get(row).expect("row");
        for column in 0..expected.len() {
            assert_eq!(table.value(column, row).as_ref(), expected.get(column), "cell ({column}, {row})");
        }
    }
    // Blanks really are masked, not zeroed.
    assert_eq!(table.value_by_name("Block", 0), Some(Scalar::Blank));
    assert_eq!(table.value_by_name("Block", 1), Some(Scalar::Integer(0)));
}

#[test]
fn aggregates_match_the_generator_for_run_and_packed_columns() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let table = storage.table(0).expect("table");
    let all: Vec<usize> = (0..ROWS).collect();

    let block_sum: i64 = (0..ROWS).map(block).sum();
    assert_eq!(table.sum(0, &all), Ok(Scalar::Integer(block_sum)));
    let amount_sum: i64 = (0..ROWS).map(|row| row as i64).sum();
    assert_eq!(table.sum(2, &all), Ok(Scalar::Integer(amount_sum)));
    let price_sum: i64 = (0..ROWS).map(price).sum();
    assert_eq!(table.sum(3, &all), Ok(Scalar::Currency(price_sum)));

    assert_eq!(table.count(0, &all), Ok(Scalar::Integer(ROWS as i64)));
    assert_eq!(table.distinct_count(1, &all), Ok(Scalar::Integer(8)));
    assert_eq!(table.distinct_count(4, &all), Ok(Scalar::Integer(1)));
    assert_eq!(table.count_blank(0, &all), Ok(Scalar::Integer(0)));
    assert_eq!(table.min(0, &all), Ok(Scalar::Integer(0)));
    assert_eq!(table.max(0, &all), Ok(Scalar::Integer(block(ROWS - 1))));

    let mut expected_regions: Vec<Scalar> = REGIONS.iter().map(|name| Scalar::String((*name).into())).collect();
    let distinct = table.distinct_values(1, &all).expect("distinct");
    let mut distinct_sorted: Vec<Scalar> = distinct.clone();
    expected_regions.sort_by_key(Scalar::to_text);
    distinct_sorted.sort_by_key(Scalar::to_text);
    assert_eq!(distinct_sorted, expected_regions);

    // numeric_values length and a few observations.
    let values = table.numeric_values(0, &all).expect("numeric values");
    assert_eq!(values.len(), ROWS);
    assert_eq!(values.first().copied(), Some(0.0));
    assert_eq!(values.last().copied(), Some(block(ROWS - 1) as f64));
    assert!(table.numeric_values(1, &all).expect("text values").is_empty());
}

#[test]
fn subset_sums_are_order_independent_and_match_scalar_arithmetic() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let table = storage.table(0).expect("table");

    let unsorted = vec![99_999, 0, 500, 500, 999, 99_998, 3, 3, 3];
    let expected: i64 = unsorted.iter().map(|row| block(*row)).sum();
    assert!(unsorted.windows(2).any(|pair| pair[0] > pair[1]), "selection is deliberately unsorted");
    assert_eq!(table.sum(0, &unsorted), Ok(Scalar::Integer(expected)));
    assert_eq!(table.sum_batch(0, &unsorted), Ok(Scalar::Integer(expected)));

    // An empty selection and a selection of blanks are BLANK.
    assert_eq!(table.sum(0, &[]), Ok(Scalar::Blank));
    assert_eq!(table.sum_batch(0, &[]), Ok(Scalar::Blank));

    let batches: Vec<&[usize]> = vec![&[0, 1, 2], &[99_999], &[], &[500, 500]];
    let totals = table.sum_batches(0, &batches).expect("batches");
    assert_eq!(totals, vec![Scalar::Integer(0), Scalar::Integer(block(99_999)), Scalar::Blank, Scalar::Integer(0),]);

    // A scan block boundary must not change the answer.
    let crossing: Vec<usize> = (SCAN_BATCH_ROWS - 3..=SCAN_BATCH_ROWS + 3).collect();
    let expected: i64 = crossing.iter().map(|row| block(*row)).sum();
    assert_eq!(table.sum(0, &crossing), Ok(Scalar::Integer(expected)));
}

#[test]
fn for_each_value_scans_without_materializing_scalars() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let table = storage.table(0).expect("table");
    let rows = vec![0, 1, 7, 8, 99_999];

    let mut seen: Vec<(usize, String)> = Vec::new();
    table
        .for_each_value(1, &rows, |row, value| {
            let text = match value {
                ValueRef::Text(text) => text.to_string(),
                other => panic!("expected text, got {other:?}"),
            };
            seen.push((row, text));
        })
        .expect("scan");
    assert_eq!(seen, rows.iter().map(|row| (*row, region(*row).to_string())).collect::<Vec<_>>());

    // Integer and boolean views keep their exact representation.
    let mut integers = Vec::new();
    table
        .for_each_value(0, &rows, |_, value| match value {
            ValueRef::Integer(cell) => integers.push(cell),
            other => panic!("expected integer, got {other:?}"),
        })
        .expect("scan");
    assert_eq!(integers, rows.iter().map(|row| block(*row)).collect::<Vec<_>>());

    let mut booleans = Vec::new();
    table
        .for_each_value(5, &rows, |_, value| match value {
            ValueRef::Boolean(cell) => booleans.push(cell),
            other => panic!("expected boolean, got {other:?}"),
        })
        .expect("scan");
    assert_eq!(booleans, rows.iter().map(|row| row % 2 == 0).collect::<Vec<_>>());

    // Out-of-range row ids are rejected before any value is visited.
    assert!(table.for_each_value(0, &[ROWS], |_, _| panic!("nothing should be visited")).is_err());
}

#[test]
fn filters_and_distinct_paths_read_packed_codes() {
    let storage = ModelStorage::from_model(&model()).expect("storage");
    let table = storage.table(0).expect("table");
    let wanted = vec![gridcraft_dax::value::ScalarKey::String("West".into())];
    let rows = table.filter_rows(1, &wanted).expect("filter");
    assert_eq!(rows.len(), ROWS / REGIONS.len());
    assert!(rows.iter().all(|row| region(*row) == "West"));

    for column in 0..table.columns().len() {
        let storage = table.column(column).expect("column");
        if storage.kind() != ColumnKind::String {
            continue;
        }
        assert!(matches!(storage.data(), ColumnData::PackedString { .. }), "column {column} should be packed: {:?}", storage.data());
    }
}

#[test]
fn short_columns_keep_the_plain_representation() {
    // Below the minimum saving a plain vector wins: no fixture with a handful of rows changes shape.
    let model = DataModel {
        tables: vec![ModelTable {
            name: "Tiny".into(),
            columns: vec![
                ModelColumn { name: "Key".into(), data_type: Some("Int64".into()) },
                ModelColumn { name: "Label".into(), data_type: Some("String".into()) },
            ],
            rows: vec![
                vec![Scalar::Integer(1), Scalar::String("East".into())],
                vec![Scalar::Integer(1), Scalar::String("West".into())],
                vec![Scalar::Integer(1), Scalar::String("East".into())],
                vec![Scalar::Integer(1), Scalar::String("North".into())],
                vec![Scalar::Integer(1), Scalar::String("East".into())],
                vec![Scalar::Integer(1), Scalar::String("West".into())],
            ],
        }],
        ..DataModel::default()
    };
    let storage = ModelStorage::from_model(&model).expect("storage");
    let table = storage.table(0).expect("table");
    assert!(matches!(table.column(0).map(ColumnStorage::data), Some(ColumnData::Integer { .. })));
    assert!(matches!(table.column(1).map(ColumnStorage::data), Some(ColumnData::String { .. })));
    // The stored codes remain the plain vector the model produced.
    assert_eq!(table.column(1).map(ColumnStorage::data).map(ColumnData::codes), Some(&[0_u32, 1, 0, 2, 0, 1][..]));
}
