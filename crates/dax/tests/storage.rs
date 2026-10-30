//! Columnar storage tests over the evaluator's in-memory model shape.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::storage::{Aggregate, Bitmap, ColumnData, ColumnKind, ColumnStorage, ModelStorage, StorageError, TableStorage};
use gridcraft_dax::value::{Scalar, ScalarKey};

fn sample_model() -> DataModel {
    DataModel {
        tables: vec![
            ModelTable {
                name: "Sales".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Region".into(), data_type: Some("String".into()) },
                    ModelColumn { name: "Amount".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Price".into(), data_type: Some("Currency".into()) },
                    ModelColumn { name: "Closed".into(), data_type: Some("DateTime".into()) },
                    ModelColumn { name: "Returned".into(), data_type: Some("Boolean".into()) },
                    ModelColumn { name: "Note".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![
                        Scalar::Integer(2),
                        Scalar::String("East".into()),
                        Scalar::Integer(10),
                        Scalar::Currency(12500),
                        Scalar::DateTime(45000.0),
                        Scalar::Boolean(true),
                        Scalar::Blank,
                    ],
                    vec![
                        Scalar::Integer(1),
                        Scalar::String("West".into()),
                        Scalar::Integer(20),
                        Scalar::Currency(5000),
                        Scalar::DateTime(45001.5),
                        Scalar::Boolean(false),
                        Scalar::Blank,
                    ],
                    vec![
                        Scalar::Integer(2),
                        Scalar::String("East".into()),
                        Scalar::Integer(30),
                        Scalar::Currency(7500),
                        Scalar::DateTime(45002.0),
                        Scalar::Boolean(true),
                        Scalar::Blank,
                    ],
                    vec![Scalar::Integer(3), Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank],
                    vec![
                        Scalar::Integer(99),
                        Scalar::String("North".into()),
                        Scalar::Integer(40),
                        Scalar::Currency(2500),
                        Scalar::DateTime(45003.25),
                        Scalar::Boolean(false),
                        Scalar::Blank,
                    ],
                    vec![
                        Scalar::Blank,
                        Scalar::String("East".into()),
                        Scalar::Integer(5),
                        Scalar::Currency(1000),
                        Scalar::DateTime(45004.0),
                        Scalar::Boolean(true),
                        Scalar::Blank,
                    ],
                ],
            },
            ModelTable {
                name: "Product".into(),
                columns: vec![
                    ModelColumn { name: "ProductKey".into(), data_type: Some("Int64".into()) },
                    ModelColumn { name: "Name".into(), data_type: Some("String".into()) },
                ],
                rows: vec![
                    vec![Scalar::Integer(1), Scalar::String("Widget".into())],
                    vec![Scalar::Integer(2), Scalar::String("Gadget".into())],
                    vec![Scalar::Integer(2), Scalar::String("Gadget".into())],
                    vec![Scalar::Integer(3), Scalar::String("Gizmo".into())],
                ],
            },
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "ProductKey".into(),
            to_table: "Product".into(),
            to_column: "ProductKey".into(),
            bidirectional: false,
            inactive: false,
        }],
        measures: vec![],
    }
}

fn storage() -> ModelStorage {
    match ModelStorage::from_model(&sample_model()) {
        Ok(storage) => storage,
        Err(error) => panic!("sample model builds: {error}"),
    }
}

fn sales(storage: &ModelStorage) -> &TableStorage {
    match storage.table_by_name("Sales") {
        Some(table) => table,
        None => panic!("Sales table exists"),
    }
}

fn column_index(table: &TableStorage, name: &str) -> usize {
    match table.column_index(name) {
        Some(index) => index,
        None => panic!("column `{name}` exists"),
    }
}

#[test]
fn builds_tables_with_stable_ids_and_inferred_types() {
    let storage = storage();
    assert_eq!(storage.tables().len(), 2);
    assert_eq!(storage.table_index("sales"), Some(0));
    assert_eq!(storage.table_index("PRODUCT"), Some(1));
    assert_eq!(storage.table(2), None);
    assert_eq!(storage.table_by_name("missing"), None);
    assert_eq!(storage.total_rows(), 10);

    let sales = sales(&storage);
    assert_eq!(sales.name(), "Sales");
    assert_eq!(sales.row_count(), 6);
    assert_eq!(sales.columns().len(), 7);
    assert_eq!(sales.column_index("region"), Some(1));
    assert_eq!(sales.column_index("Missing"), None);

    let kinds: Vec<ColumnKind> = sales.columns().iter().map(|column| column.kind()).collect();
    assert_eq!(
        kinds,
        vec![
            ColumnKind::Integer,
            ColumnKind::String,
            ColumnKind::Integer,
            ColumnKind::Currency,
            ColumnKind::DateTime,
            ColumnKind::Boolean,
            ColumnKind::Blank,
        ]
    );
    assert_eq!(sales.columns()[0].len(), 6);
    assert!(sales.columns()[0].kind().is_numeric());
    assert!(!sales.columns()[1].kind().is_numeric());

    let product = match storage.table(1) {
        Some(table) => table,
        None => panic!("Product table exists"),
    };
    assert_eq!(product.column_by_name("name").map(ColumnStorage::kind), Some(ColumnKind::String));

    // Table ids and column ids follow the model, so lookups agree with the metadata layer.
    let model = sample_model();
    assert_eq!(model.table_index("Sales"), Some(0));
    assert_eq!(model.column_index(0, "Amount"), Some(2));
    assert_eq!(storage.column_index(0, "Amount"), Some(2));
    assert_eq!(storage.column(0, 2).map(ColumnStorage::name), Some("Amount"));
    assert_eq!(storage.column(9, 0), None);
    assert_eq!(storage.value(0, 2, 0), Some(Scalar::Integer(10)));
    assert_eq!(storage.value(0, 2, 6), None);
    assert_eq!(storage.value(0, 2, 3), Some(Scalar::Blank));
}

#[test]
fn string_columns_are_dictionary_encoded() {
    let storage = storage();
    let sales = sales(&storage);
    let region = match sales.column(1).map(ColumnStorage::data) {
        Some(ColumnData::String { dictionary, codes, nulls }) => (dictionary.clone(), codes.clone(), nulls.clone()),
        other => panic!("Region should be dictionary encoded, got {other:?}"),
    };
    let (dictionary, codes, nulls) = region;
    assert_eq!(dictionary, vec!["East".to_string(), "West".to_string(), "North".to_string()]);
    assert_eq!(codes, vec![0, 1, 0, 0, 2, 0]);
    assert_eq!(nulls.count_ones(), 1);
    assert!(nulls.get(3));
    assert!(!nulls.get(0));

    // Every row reads back exactly, including the blank placeholder code.
    let expected = [
        Scalar::String("East".into()),
        Scalar::String("West".into()),
        Scalar::String("East".into()),
        Scalar::Blank,
        Scalar::String("North".into()),
        Scalar::String("East".into()),
    ];
    for (row, value) in expected.iter().enumerate() {
        assert_eq!(sales.value_by_name("Region", row).as_ref(), Some(value));
    }
    assert_eq!(sales.column(1).map(|column| column.data().value(6)), Some(None));
}

#[test]
fn filters_by_value_set_including_blanks_and_duplicates() {
    let storage = storage();
    let sales = sales(&storage);
    let region = column_index(sales, "Region");
    assert_eq!(
        sales.filter_rows(region, &[ScalarKey::String("East".into()), ScalarKey::Blank, ScalarKey::String("East".into())]),
        Ok(vec![0, 2, 3, 5])
    );
    assert_eq!(sales.filter_rows(region, &[ScalarKey::String("West".into())]), Ok(vec![1]));
    assert_eq!(sales.filter_rows(region, &[ScalarKey::String("east".into())]), Ok(vec![]));
    assert_eq!(sales.filter_rows(region, &[]), Ok(vec![]));

    let product_key = column_index(sales, "ProductKey");
    assert_eq!(sales.filter_rows(product_key, &[ScalarKey::Integer(2)]), Ok(vec![0, 2]));
    assert_eq!(sales.filter_rows(product_key, &[ScalarKey::Blank]), Ok(vec![5]));
    assert_eq!(sales.filter_rows(product_key, &[ScalarKey::Integer(2), ScalarKey::Blank]), Ok(vec![0, 2, 5]));

    // All-blank columns match only the blank key; the empty string is a different value.
    let note = column_index(sales, "Note");
    assert_eq!(sales.filter_rows(note, &[ScalarKey::Blank]), Ok(vec![0, 1, 2, 3, 4, 5]));
    assert_eq!(sales.filter_rows(note, &[ScalarKey::String(String::new())]), Ok(vec![]));

    assert_eq!(sales.filter_rows(99, &[ScalarKey::Blank]), Err(StorageError::ColumnOutOfRange { table: "Sales".into(), index: 99 }));
}

#[test]
fn aggregates_skip_blanks_but_count_distinct_blanks() {
    let storage = storage();
    let sales = sales(&storage);
    let amount = column_index(sales, "Amount");
    let all_rows: Vec<usize> = (0..sales.row_count()).collect();

    assert_eq!(sales.sum(amount, &all_rows), Ok(Scalar::Integer(105)));
    assert_eq!(sales.count(amount, &all_rows), Ok(Scalar::Integer(5)));
    assert_eq!(sales.distinct_count(amount, &all_rows), Ok(Scalar::Integer(6)));
    assert_eq!(sales.min(amount, &all_rows), Ok(Scalar::Integer(5)));
    assert_eq!(sales.max(amount, &all_rows), Ok(Scalar::Integer(40)));
    assert_eq!(sales.aggregate(amount, &all_rows, Aggregate::Sum), Ok(Scalar::Integer(105)));
    assert_eq!(sales.aggregate(amount, &all_rows, Aggregate::Count), Ok(Scalar::Integer(5)));
    assert_eq!(sales.aggregate(amount, &all_rows, Aggregate::DistinctCount), Ok(Scalar::Integer(6)));
    assert_eq!(sales.aggregate(amount, &all_rows, Aggregate::Min), Ok(Scalar::Integer(5)));
    assert_eq!(sales.aggregate(amount, &all_rows, Aggregate::Max), Ok(Scalar::Integer(40)));

    // A filtered subset, plus empty-selection blank semantics.
    assert_eq!(sales.sum(amount, &[0, 1]), Ok(Scalar::Integer(30)));
    assert_eq!(sales.sum(amount, &[]), Ok(Scalar::Blank));
    assert_eq!(sales.count(amount, &[]), Ok(Scalar::Integer(0)));
    assert_eq!(sales.distinct_count(amount, &[]), Ok(Scalar::Integer(0)));
    assert_eq!(sales.min(amount, &[]), Ok(Scalar::Blank));
    assert_eq!(sales.max(amount, &[]), Ok(Scalar::Blank));

    // Currency keeps its scaled representation.
    let price = column_index(sales, "Price");
    assert_eq!(sales.sum(price, &all_rows), Ok(Scalar::Currency(28_500)));

    // DateTime sums as a decimal serial; min/max keep the DateTime type.
    let closed = column_index(sales, "Closed");
    assert_eq!(sales.sum(closed, &all_rows), Ok(Scalar::Decimal(45000.0 + 45001.5 + 45002.0 + 45003.25 + 45004.0)));
    assert_eq!(sales.min(closed, &all_rows), Ok(Scalar::DateTime(45000.0)));
    assert_eq!(sales.max(closed, &all_rows), Ok(Scalar::DateTime(45004.0)));

    assert_eq!(sales.sum(column_index(sales, "Region"), &all_rows), Err(StorageError::NotNumeric { table: "Sales".into(), column: "Region".into() }));
    assert_eq!(sales.sum(amount, &[6]), Err(StorageError::RowOutOfRange { table: "Sales".into(), column: "Amount".into(), row: 6, row_count: 6 }));
}

#[test]
fn blank_columns_handle_every_aggregate() {
    let storage = storage();
    let sales = sales(&storage);
    let note = column_index(sales, "Note");
    let all_rows: Vec<usize> = (0..sales.row_count()).collect();
    let data = match sales.column(note) {
        Some(column) => column.data(),
        None => panic!("Note column exists"),
    };
    assert_eq!(data.kind(), ColumnKind::Blank);
    assert_eq!(data.len(), 6);
    assert_eq!(data.null_bitmap(), None);
    assert_eq!(data.blank_count(), 6);
    assert_eq!(data.is_blank_at(0), Some(true));
    assert_eq!(data.is_blank_at(6), None);
    assert_eq!(data.value(0), Some(Scalar::Blank));

    assert_eq!(sales.sum(note, &all_rows), Ok(Scalar::Blank));
    assert_eq!(sales.count(note, &all_rows), Ok(Scalar::Integer(0)));
    assert_eq!(sales.distinct_count(note, &all_rows), Ok(Scalar::Integer(1)));
    assert_eq!(sales.min(note, &all_rows), Ok(Scalar::Blank));
    assert_eq!(sales.max(note, &all_rows), Ok(Scalar::Blank));
}

#[test]
fn relationship_indexes_map_keys_to_row_ids() {
    let storage = storage();
    assert_eq!(storage.relationships().len(), 1);
    let relationship = match storage.relationships().first() {
        Some(relationship) => relationship,
        None => panic!("relationship exists"),
    };
    assert_eq!((relationship.from_table, relationship.from_column), (0, 0));
    assert_eq!((relationship.to_table, relationship.to_column), (1, 0));
    assert!(!relationship.bidirectional);
    assert!(!relationship.inactive);

    // Duplicate keys on both sides keep every row id.
    assert_eq!(relationship.rows_from(&ScalarKey::Integer(2)), &[0, 2]);
    assert_eq!(relationship.rows_from(&ScalarKey::Integer(1)), &[1]);
    assert_eq!(relationship.rows_from(&ScalarKey::Integer(3)), &[3]);
    assert_eq!(relationship.rows_from(&ScalarKey::Integer(99)), &[4]);
    assert_eq!(relationship.rows_from(&ScalarKey::Blank), &[5]);
    assert!(relationship.rows_from(&ScalarKey::Integer(1000)).is_empty());

    assert_eq!(relationship.rows_to(&ScalarKey::Integer(2)), &[1, 2]);
    assert_eq!(relationship.rows_to(&ScalarKey::Integer(1)), &[0]);
    assert_eq!(relationship.rows_to(&ScalarKey::Integer(3)), &[3]);
    assert!(relationship.rows_to(&ScalarKey::Integer(99)).is_empty());
    assert!(relationship.rows_to(&ScalarKey::Blank).is_empty());

    assert_eq!(relationship.from_key_count(), 5);
    assert_eq!(relationship.to_key_count(), 3);
    assert_eq!(storage.relationships_from(0).count(), 1);
    assert_eq!(storage.relationships_to(1).count(), 1);
    assert_eq!(storage.relationships_from(1).count(), 0);
    assert!(storage.relationship("sales", "productkey", "product", "productkey").is_some());
    assert!(storage.relationship("Sales", "Region", "Product", "ProductKey").is_none());

    // The index joins to a filtered key: product 2 selects Sales rows 0 and 2.
    let sales = sales(&storage);
    let amount = column_index(sales, "Amount");
    assert_eq!(sales.sum(amount, relationship.rows_from(&ScalarKey::Integer(2))), Ok(Scalar::Integer(40)));
}

#[test]
fn values_round_trip_for_every_representation() {
    let columns = vec![
        "Integer".to_string(),
        "Currency".to_string(),
        "Decimal".to_string(),
        "DateTime".to_string(),
        "Boolean".to_string(),
        "String".to_string(),
        "AllBlank".to_string(),
    ];
    let rows = vec![
        vec![
            Scalar::Integer(7),
            Scalar::Currency(12345),
            Scalar::Decimal(1.5),
            Scalar::DateTime(45000.25),
            Scalar::Boolean(true),
            Scalar::String("x".into()),
            Scalar::Blank,
        ],
        vec![Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank, Scalar::Blank],
        vec![
            Scalar::Integer(-3),
            Scalar::Currency(-500),
            Scalar::Decimal(-2.25),
            Scalar::DateTime(45001.0),
            Scalar::Boolean(false),
            Scalar::String("y".into()),
            Scalar::Blank,
        ],
    ];
    let table = match TableStorage::from_rows("Mixed", columns, rows.clone()) {
        Ok(table) => table,
        Err(error) => panic!("mixed table builds: {error}"),
    };
    let expected_kinds = vec![
        ColumnKind::Integer,
        ColumnKind::Currency,
        ColumnKind::Decimal,
        ColumnKind::DateTime,
        ColumnKind::Boolean,
        ColumnKind::String,
        ColumnKind::Blank,
    ];
    for (column, kind) in table.columns().iter().zip(expected_kinds) {
        assert_eq!(column.kind(), kind, "column {}", column.name());
    }
    for (row, values) in rows.iter().enumerate() {
        for (column, expected) in values.iter().enumerate() {
            assert_eq!(table.value(column, row).as_ref(), Some(expected), "row {row} column {column}");
        }
    }

    // Ragged rows are blank-extended, and extra cells are ignored.
    let ragged = match TableStorage::from_rows(
        "Ragged",
        vec!["A".to_string(), "B".to_string()],
        vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2), Scalar::String("two".into()), Scalar::Integer(99)]],
    ) {
        Ok(table) => table,
        Err(error) => panic!("ragged table builds: {error}"),
    };
    assert_eq!(ragged.row_count(), 2);
    assert_eq!(ragged.value_by_name("B", 0), Some(Scalar::Blank));
    assert_eq!(ragged.value_by_name("B", 1), Some(Scalar::String("two".into())));
    let a = match ragged.column_by_name("A").map(ColumnStorage::data) {
        Some(ColumnData::Integer { values, .. }) => values.clone(),
        other => panic!("A should be integer, got {other:?}"),
    };
    assert_eq!(a, vec![1, 2]);
}

#[test]
fn construction_and_lookup_failures_are_bounded() {
    assert_eq!(
        TableStorage::from_rows("Dup", vec!["A".to_string(), "a".to_string()], vec![vec![Scalar::Integer(1), Scalar::Integer(2)]]),
        Err(StorageError::DuplicateColumn { table: "Dup".into(), column: "a".into() })
    );

    let column = ColumnStorage::new("A", ColumnData::Integer { values: vec![1], nulls: Bitmap::new(1) });
    assert_eq!(
        TableStorage::new("Short", 2, vec![column]),
        Err(StorageError::ColumnLengthMismatch { table: "Short".into(), column: "A".into(), column_len: 1, row_count: 2 })
    );

    let mut invalid = sample_model();
    invalid.relationships.push(Relationship {
        from_table: "Sales".into(),
        from_column: "ProductKey".into(),
        to_table: "Missing".into(),
        to_column: "ProductKey".into(),
        bidirectional: false,
        inactive: false,
    });
    assert_eq!(ModelStorage::from_model(&invalid), Err(StorageError::UnknownTable { table: "Missing".into() }));

    let mut duplicate = sample_model();
    let first = duplicate.tables[0].clone();
    duplicate.tables.push(first);
    assert_eq!(ModelStorage::from_model(&duplicate), Err(StorageError::DuplicateTable { table: "Sales".into() }));
}
