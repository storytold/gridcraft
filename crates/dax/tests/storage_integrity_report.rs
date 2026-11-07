//! Referential-integrity report tests over deliberately hostile models.
//!
//! Every model here is built in code — duplicate one-side keys, orphan fact keys, blank keys,
//! unknown endpoints, duplicate table names, a target model larger than the report cap — and the
//! tests assert the report contents field by field, including the deterministic order and the
//! truncation counts. Both entry points are covered, and they are required to agree on a model the
//! store can materialize faithfully.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::storage::ModelStorage;
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{Cardinality, IntegrityCheck, IntegrityFinding, MAX_REPORTED_KEYS, MAX_REPORTED_ROWS, Severity, validate_model};

fn key_column(name: &str) -> ModelColumn {
    ModelColumn { name: name.into(), data_type: Some("Int64".into()) }
}

fn text_column(name: &str) -> ModelColumn {
    ModelColumn { name: name.into(), data_type: Some("String".into()) }
}

fn table(name: &str, columns: Vec<ModelColumn>, rows: Vec<Vec<Scalar>>) -> ModelTable {
    ModelTable { name: name.into(), columns, rows }
}

fn relation(from_table: &str, from_column: &str, to_table: &str, to_column: &str, bidirectional: bool) -> Relationship {
    Relationship {
        from_table: from_table.into(),
        from_column: from_column.into(),
        to_table: to_table.into(),
        to_column: to_column.into(),
        bidirectional,
        inactive: false,
    }
}

/// `Product` has the one side's classic defects: key `1` twice and a blank key.
/// `Sales` has the many side's: an orphan key `7`, a blank key, and key `1` twice.
fn hostile_model() -> DataModel {
    DataModel {
        tables: vec![
            table(
                "Product",
                vec![key_column("ProductKey"), text_column("Name")],
                vec![
                    vec![Scalar::Integer(1), Scalar::String("One".into())],
                    vec![Scalar::Integer(2), Scalar::String("Two".into())],
                    vec![Scalar::Integer(3), Scalar::String("Three".into())],
                    vec![Scalar::Integer(1), Scalar::String("One again".into())],
                    vec![Scalar::Blank, Scalar::String("No key".into())],
                ],
            ),
            table(
                "Sales",
                vec![key_column("ProductKey"), key_column("Amount")],
                vec![
                    vec![Scalar::Integer(1), Scalar::Integer(10)],
                    vec![Scalar::Integer(2), Scalar::Integer(20)],
                    vec![Scalar::Integer(7), Scalar::Integer(30)],
                    vec![Scalar::Blank, Scalar::Integer(40)],
                    vec![Scalar::Integer(1), Scalar::Integer(50)],
                ],
            ),
        ],
        relationships: vec![relation("Sales", "ProductKey", "Product", "ProductKey", false)],
        measures: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)] // One named argument per column of the expected finding row.
fn finding(
    check: IntegrityCheck,
    severity: Severity,
    table: &str,
    column: &str,
    related_table: Option<&str>,
    related_column: Option<&str>,
    key: Option<Scalar>,
    rows: Vec<usize>,
    total: usize,
    message: &str,
) -> IntegrityFinding {
    IntegrityFinding {
        check,
        severity,
        relationship: Some(0),
        table: table.into(),
        column: column.into(),
        related_table: related_table.map(Into::into),
        related_column: related_column.map(Into::into),
        key,
        rows,
        total,
        cycle: Vec::new(),
        message: message.into(),
    }
}

#[test]
fn hostile_model_reports_orphans_duplicates_blanks_and_invalid_cardinality() {
    let model = hostile_model();
    let report = validate_model(&model);

    assert_eq!(report.tables, 2);
    assert_eq!(report.rows, 10);
    assert!(report.has_errors());
    assert_eq!(report.errors().count(), 3, "orphan, duplicate key and many-to-many are errors");
    assert_eq!(report.warnings().count(), 2, "both blank-key findings are warnings");

    let check = report.relationships.first().expect("one relationship");
    assert_eq!(check.index, 0);
    assert_eq!(check.from_table, "Sales");
    assert_eq!(check.from_column, "ProductKey");
    assert_eq!(check.to_table, "Product");
    assert_eq!(check.to_column, "ProductKey");
    assert_eq!(check.declared, Cardinality::OneToMany);
    assert_eq!(check.actual, Cardinality::ManyToMany);
    assert!(!check.is_valid());
    assert_eq!(check.from_rows, 5);
    assert_eq!(check.from_keys, 3);
    assert_eq!(check.to_rows, 5);
    assert_eq!(check.to_keys, 3);
    assert_eq!(check.orphan_keys, 1);
    assert_eq!(check.orphan_rows, 1);
    assert_eq!(check.duplicate_to_keys, 1);
    assert_eq!(check.blank_from_keys, 1);
    assert_eq!(check.blank_to_keys, 1);
    assert!(!check.bidirectional);
    assert!(!check.inactive);

    let expected = vec![
        finding(
            IntegrityCheck::OrphanRow,
            Severity::Error,
            "Sales",
            "ProductKey",
            Some("Product"),
            Some("ProductKey"),
            Some(Scalar::Integer(7)),
            vec![2],
            1,
            "1 row(s) in `Sales[ProductKey]` have key 7 with no match in `Product[ProductKey]`",
        ),
        finding(
            IntegrityCheck::DuplicateKey,
            Severity::Error,
            "Product",
            "ProductKey",
            Some("Sales"),
            Some("ProductKey"),
            Some(Scalar::Integer(1)),
            vec![0, 3],
            2,
            "key 1 appears in 2 row(s) of `Product[ProductKey]`, so the one side of the relationship is not unique",
        ),
        finding(
            IntegrityCheck::BlankKey,
            Severity::Warning,
            "Sales",
            "ProductKey",
            Some("Product"),
            Some("ProductKey"),
            Some(Scalar::Blank),
            vec![3],
            1,
            "1 row(s) in `Sales[ProductKey]` have a blank relationship key",
        ),
        finding(
            IntegrityCheck::BlankKey,
            Severity::Warning,
            "Product",
            "ProductKey",
            Some("Sales"),
            Some("ProductKey"),
            Some(Scalar::Blank),
            vec![4],
            1,
            "1 row(s) in `Product[ProductKey]` have a blank relationship key",
        ),
        finding(
            IntegrityCheck::InvalidCardinality,
            Severity::Error,
            "Sales",
            "ProductKey",
            Some("Product"),
            Some("ProductKey"),
            None,
            Vec::new(),
            0,
            "relationship `Sales[ProductKey]` -> `Product[ProductKey]` declares one-to-many but both sides have duplicate keys (many-to-many); the one side must be unique",
        ),
    ];
    assert_eq!(report.findings, expected);
    assert_eq!(report.first_error(), Some("1 row(s) in `Sales[ProductKey]` have key 7 with no match in `Product[ProductKey]`"));
}

#[test]
fn the_storage_entry_point_agrees_with_the_model_entry_point() {
    let model = hostile_model();
    let storage = ModelStorage::from_model(&model).expect("hostile keys still materialize");
    assert!(storage.is_faithful());
    assert_eq!(storage.validate(), validate_model(&model));
}

#[test]
fn a_clean_star_schema_reports_nothing() {
    let model = DataModel {
        tables: vec![
            table("Product", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2)], vec![Scalar::Integer(3)]]),
            table(
                "Sales",
                vec![key_column("ProductKey")],
                vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2)], vec![Scalar::Integer(2)], vec![Scalar::Integer(3)]],
            ),
        ],
        relationships: vec![relation("Sales", "ProductKey", "Product", "ProductKey", false)],
        measures: Vec::new(),
    };
    let report = validate_model(&model);
    assert!(report.is_clean(), "findings: {:?}", report.findings);
    assert!(!report.has_errors());
    assert!(report.first_error().is_none());
    let check = report.relationships.first().expect("one relationship");
    assert_eq!(check.actual, Cardinality::OneToMany);
    assert!(check.is_valid());
    assert_eq!(check.orphan_keys, 0);
    assert_eq!(check.blank_from_keys, 0);
    assert!(storage_agrees(&model, &report));
}

fn storage_agrees(model: &DataModel, report: &gridcraft_dax::IntegrityReport) -> bool {
    ModelStorage::from_model(model).is_ok_and(|storage| storage.validate() == *report)
}

#[test]
fn one_to_one_and_reversed_relationships_are_warnings_not_errors() {
    let one_to_one = DataModel {
        tables: vec![
            table("Product", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2)]]),
            table("Sales", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2)]]),
        ],
        relationships: vec![relation("Sales", "ProductKey", "Product", "ProductKey", false)],
        measures: Vec::new(),
    };
    let report = validate_model(&one_to_one);
    assert_eq!(report.findings.len(), 1);
    let only = report.findings.first().expect("one finding");
    assert_eq!(only.check, IntegrityCheck::InvalidCardinality);
    assert_eq!(only.severity, Severity::Warning);
    assert_eq!(report.relationships.first().map(|check| check.actual), Some(Cardinality::OneToOne));
    assert!(!report.has_errors());

    // The from side is unique and the to side repeats: the relationship points the wrong way.
    let reversed = DataModel {
        tables: vec![
            table("Product", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2)], vec![Scalar::Integer(2)]]),
            table("Sales", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(2)]]),
        ],
        relationships: vec![relation("Sales", "ProductKey", "Product", "ProductKey", false)],
        measures: Vec::new(),
    };
    let report = validate_model(&reversed);
    let check = report.relationships.first().expect("one relationship");
    assert_eq!(check.actual, Cardinality::ManyToOne);
    assert_eq!(check.duplicate_to_keys, 1);
    assert_eq!(report.findings_for(IntegrityCheck::DuplicateKey).count(), 1, "the repeated one-side key is still an error");
    let cardinality: Vec<&IntegrityFinding> = report.findings_for(IntegrityCheck::InvalidCardinality).collect();
    assert_eq!(cardinality.first().map(|finding| finding.severity), Some(Severity::Warning));
    assert!(
        cardinality.first().is_some_and(|finding| finding.message.contains("stored in the wrong direction")),
        "{:?}",
        cardinality.first().map(|finding| finding.message.as_str())
    );
}

#[test]
fn unknown_endpoints_are_reported_instead_of_failing() {
    let model = DataModel {
        tables: vec![table("Sales", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)]])],
        relationships: vec![relation("Sales", "ProductKey", "Missing", "ProductKey", false)],
        measures: Vec::new(),
    };
    let report = validate_model(&model);
    assert!(report.relationships.is_empty(), "an unresolved relationship has no measured cardinality");
    assert_eq!(report.findings.len(), 1);
    let only = report.findings.first().expect("one finding");
    assert_eq!(only.check, IntegrityCheck::UnknownEndpoint);
    assert_eq!(only.severity, Severity::Error);
    assert_eq!(only.table, "Missing");
    assert_eq!(only.column, "ProductKey");
    assert_eq!(only.related_table.as_deref(), Some("Sales"));
    assert_eq!(only.related_column.as_deref(), Some("ProductKey"));
    assert_eq!(only.relationship, Some(0));
    assert_eq!(only.total, 0);
    assert!(only.message.contains("which does not exist"), "{}", only.message);

    // A relationship naming a column that does not exist is the same kind of finding.
    let mut missing_column = model;
    missing_column.relationships[0].to_table = "Sales".into();
    missing_column.relationships[0].to_column = "Nope".into();
    let report = validate_model(&missing_column);
    assert_eq!(report.findings_for(IntegrityCheck::UnknownEndpoint).count(), 1);
    let only = report.findings.first().expect("one finding");
    assert_eq!(only.column, "Nope");
}

#[test]
fn duplicate_table_names_are_reported_first() {
    let sales = table("Sales", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)]]);
    let model = DataModel {
        tables: vec![sales.clone(), sales],
        relationships: vec![relation("Sales", "ProductKey", "Sales", "ProductKey", false)],
        measures: Vec::new(),
    };
    let report = validate_model(&model);
    assert_eq!(report.tables, 2);
    let first = report.findings.first().expect("one finding");
    assert_eq!(first.check, IntegrityCheck::DuplicateTable);
    assert_eq!(first.severity, Severity::Error);
    assert_eq!(first.table, "Sales");
    assert_eq!(first.column, "");
    assert_eq!(first.relationship, None);
    assert_eq!(
        first.message,
        "table `Sales` is defined more than once (first at position 0, duplicate at position 1); name-based lookups resolve to the first"
    );
    // The self relationship the duplicate resolution produces still gets measured.
    let check = report.relationships.first().expect("one relationship");
    assert_eq!(check.from_table, "Sales");
    assert_eq!(check.from_rows, 1);
}

#[test]
fn big_orphan_sets_are_capped_but_counted_in_full() {
    // 31 orphan keys: key 10 appears 25 times, keys 11..=40 once each. The one side holds key 1.
    let mut sales_rows = Vec::new();
    for _ in 0..25 {
        sales_rows.push(vec![Scalar::Integer(10)]);
    }
    for key in 11..=40 {
        sales_rows.push(vec![Scalar::Integer(key)]);
    }
    let model = DataModel {
        tables: vec![
            table("Product", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)]]),
            table("Sales", vec![key_column("ProductKey")], sales_rows),
        ],
        relationships: vec![relation("Sales", "ProductKey", "Product", "ProductKey", false)],
        measures: Vec::new(),
    };
    let report = validate_model(&model);
    let check = report.relationships.first().expect("one relationship");
    assert_eq!(check.orphan_keys, 31);
    assert_eq!(check.orphan_rows, 55);
    let orphans: Vec<&IntegrityFinding> = report.findings_for(IntegrityCheck::OrphanRow).collect();
    assert_eq!(orphans.len(), MAX_REPORTED_KEYS, "key findings are capped");
    let first = orphans.first().expect("first orphan");
    assert_eq!(first.key, Some(Scalar::Integer(10)));
    assert_eq!(first.total, 25, "the uncapped row count survives the cap");
    assert_eq!(first.rows.len(), MAX_REPORTED_ROWS, "row ids are capped");
    assert_eq!(first.rows.first().copied(), Some(0));
    assert_eq!(first.rows.last().copied(), Some(MAX_REPORTED_ROWS - 1));
    // The last captured key is still in ascending order and inside the cap.
    let last = orphans.last().expect("last orphan");
    assert_eq!(last.key, Some(Scalar::Integer(25)));
    assert_eq!(last.total, 1);
    // The many-to-one orientation of the capped report is unchanged.
    assert_eq!(check.actual, Cardinality::OneToMany);
    assert!(ModelStorage::from_model(&model).is_ok_and(|storage| storage.validate() == report));
}

#[test]
fn ragged_rows_treat_missing_cells_as_blank_keys_on_both_entry_points() {
    let model = DataModel {
        tables: vec![
            table("Product", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)]]),
            // Row 0 has no cells at all, so its key cell is missing (BLANK after blank extension).
            table(
                "Sales",
                vec![key_column("ProductKey"), key_column("Amount")],
                vec![vec![], vec![Scalar::Integer(1)], vec![Scalar::Integer(1), Scalar::Integer(5)]],
            ),
        ],
        relationships: vec![relation("Sales", "ProductKey", "Product", "ProductKey", false)],
        measures: Vec::new(),
    };
    let report = validate_model(&model);
    let check = report.relationships.first().expect("one relationship");
    assert_eq!(check.from_rows, 3);
    assert_eq!(check.from_keys, 1);
    assert_eq!(check.blank_from_keys, 1);
    assert_eq!(check.orphan_keys, 0);
    let blank: Vec<&IntegrityFinding> = report.findings_for(IntegrityCheck::BlankKey).collect();
    assert_eq!(blank.len(), 1);
    assert_eq!(blank.first().map(|finding| finding.rows.clone()), Some(vec![0]));

    let storage = ModelStorage::from_model(&model).expect("ragged rows are blank-extended");
    assert_eq!(storage.validate(), report);
}

#[test]
fn bidirectional_and_inactive_flags_are_reported_verbatim() {
    let model = DataModel {
        tables: vec![
            table("Product", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)]]),
            table("Sales", vec![key_column("ProductKey")], vec![vec![Scalar::Integer(1)], vec![Scalar::Integer(1)]]),
        ],
        relationships: vec![Relationship {
            from_table: "Sales".into(),
            from_column: "ProductKey".into(),
            to_table: "Product".into(),
            to_column: "ProductKey".into(),
            bidirectional: true,
            inactive: true,
        }],
        measures: Vec::new(),
    };
    let report = validate_model(&model);
    let check = report.relationships.first().expect("one relationship");
    assert!(check.bidirectional);
    assert!(check.inactive);
    assert_eq!(check.declared, Cardinality::OneToMany);
    assert_eq!(check.actual, Cardinality::OneToMany);
    assert!(report.is_clean(), "{:?}", report.findings);
}

#[test]
fn report_accessors_group_the_findings() {
    let report = validate_model(&hostile_model());
    assert_eq!(report.findings_for(IntegrityCheck::OrphanRow).count(), 1);
    assert_eq!(report.findings_for(IntegrityCheck::DuplicateKey).count(), 1);
    assert_eq!(report.findings_for(IntegrityCheck::BlankKey).count(), 2);
    assert_eq!(report.findings_for(IntegrityCheck::InvalidCardinality).count(), 1);
    assert_eq!(report.findings_for(IntegrityCheck::CircularRelationship).count(), 0);
    assert_eq!(report.findings_for(IntegrityCheck::UnknownEndpoint).count(), 0);
    assert_eq!(report.findings_for(IntegrityCheck::DuplicateTable).count(), 0);
    assert_eq!(IntegrityCheck::OrphanRow.name(), "orphanRow");
    assert_eq!(Severity::Warning.name(), "warning");
    assert_eq!(Cardinality::ManyToMany.name(), "manyToMany");
    assert!(Severity::Warning < Severity::Error);
    // The report serializes for the engine command's JSON payload.
    let json = serde_json::to_value(&report).expect("report serializes");
    assert_eq!(json["tables"], 2);
    assert_eq!(json["relationships"][0]["actual"], "manyToMany");
    assert_eq!(json["findings"][0]["check"], "orphanRow");
    assert_eq!(json["findings"][0]["severity"], "error");
}
