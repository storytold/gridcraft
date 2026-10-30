//! Circular-relationship detection tests.
//!
//! A cycle is reported by table name with the offending chain closed (`A -> B -> C -> A`), once per
//! distinct cycle, capped at [`MAX_REPORTED_CYCLES`], and the search is iterative so a long acyclic
//! chain cannot overflow the stack. These tests cover the plain cycle, the self relationship, the
//! edge a bidirectional relationship adds, the acyclic control, the cap and determinism.

use gridcraft_dax::model::{DataModel, ModelColumn, ModelTable, Relationship};
use gridcraft_dax::storage::ModelStorage;
use gridcraft_dax::value::Scalar;
use gridcraft_dax::{IntegrityCheck, IntegrityFinding, MAX_REPORTED_CYCLES, Severity, validate_model};

fn link_table(name: &str, key: i64) -> ModelTable {
    ModelTable {
        name: name.into(),
        columns: vec![
            ModelColumn { name: "Key".into(), data_type: Some("Int64".into()) },
            ModelColumn { name: "Ref".into(), data_type: Some("Int64".into()) },
        ],
        rows: vec![vec![Scalar::Integer(key), Scalar::Integer(key)]],
    }
}

fn link(from_table: &str, to_table: &str) -> Relationship {
    Relationship {
        from_table: from_table.into(),
        from_column: "Ref".into(),
        to_table: to_table.into(),
        to_column: "Key".into(),
        bidirectional: false,
        inactive: false,
    }
}

fn model(tables: Vec<ModelTable>, relationships: Vec<Relationship>) -> DataModel {
    DataModel { tables, relationships, measures: Vec::new() }
}

fn cycles(report: &gridcraft_dax::IntegrityReport) -> Vec<&IntegrityFinding> {
    report.findings_for(IntegrityCheck::CircularRelationship).collect()
}

#[test]
fn a_three_table_cycle_is_reported_by_name() {
    let report = validate_model(&model(
        vec![link_table("A", 1), link_table("B", 1), link_table("C", 1)],
        vec![link("A", "B"), link("B", "C"), link("C", "A")],
    ));
    let found = cycles(&report);
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    let cycle = found.first().expect("one cycle");
    assert_eq!(cycle.severity, Severity::Error);
    assert_eq!(cycle.check, IntegrityCheck::CircularRelationship);
    assert_eq!(cycle.table, "A");
    assert_eq!(cycle.cycle, vec!["A".to_string(), "B".into(), "C".into(), "A".into()]);
    assert_eq!(cycle.message, "circular relationship chain: A -> B -> C -> A");
    assert_eq!(cycle.relationship, None);
    assert!(cycle.rows.is_empty());
    assert_eq!(cycle.total, 0);
    let repeat = validate_model(&model(
        vec![link_table("A", 1), link_table("B", 1), link_table("C", 1)],
        vec![link("A", "B"), link("B", "C"), link("C", "A")],
    ));
    assert_eq!(report.findings, repeat.findings, "the cycle report is deterministic");
}

#[test]
fn a_self_relationship_is_a_cycle() {
    let report = validate_model(&model(vec![link_table("Node", 1)], vec![link("Node", "Node")]));
    let found = cycles(&report);
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    let cycle = found.first().expect("one cycle");
    assert_eq!(cycle.table, "Node");
    assert_eq!(cycle.cycle, vec!["Node".to_string(), "Node".into()]);
    assert_eq!(cycle.message, "circular relationship chain: Node -> Node");
    // The materialized storage reports the same chain.
    let storage = ModelStorage::from_model(&model(vec![link_table("Node", 1)], vec![link("Node", "Node")])).expect("storage");
    assert_eq!(cycles(&storage.validate()), found);
}

#[test]
fn a_bidirectional_relationship_adds_the_reverse_edge() {
    let mut forward = link("A", "B");
    forward.bidirectional = true;
    let report = validate_model(&model(vec![link_table("A", 1), link_table("B", 1)], vec![forward, link("B", "A")]));
    let found = cycles(&report);
    assert_eq!(found.len(), 1, "findings: {:?}", report.findings);
    let cycle = found.first().expect("one cycle");
    assert_eq!(cycle.cycle, vec!["A".to_string(), "B".into(), "A".into()]);
    assert_eq!(cycle.message, "circular relationship chain: A -> B -> A");
}

#[test]
fn an_acyclic_star_has_no_cycle_finding() {
    let report = validate_model(&model(
        vec![link_table("Fact", 1), link_table("Dim1", 1), link_table("Dim2", 1)],
        vec![link("Fact", "Dim1"), link("Fact", "Dim2")],
    ));
    assert!(cycles(&report).is_empty(), "findings: {:?}", report.findings);
}

#[test]
fn a_long_acyclic_chain_is_walked_iteratively() {
    // 500 tables in a straight line: guards against recursion on the path depth.
    let tables: Vec<ModelTable> = (0..500).map(|index| link_table(&format!("T{index}"), 1)).collect();
    let relationships: Vec<Relationship> = (0..499).map(|index| link(&format!("T{index}"), &format!("T{}", index + 1))).collect();
    let report = validate_model(&model(tables, relationships));
    assert!(cycles(&report).is_empty(), "findings: {:?}", report.findings);
}

#[test]
fn cycles_are_capped() {
    // Nine independent two-table cycles; only the first eight are reported.
    let mut tables = Vec::new();
    let mut relationships = Vec::new();
    for pair in 0..9 {
        let left = format!("T{}", pair * 2);
        let right = format!("T{}", pair * 2 + 1);
        tables.push(link_table(&left, 1));
        tables.push(link_table(&right, 1));
        relationships.push(link(&left, &right));
        relationships.push(link(&right, &left));
    }
    let report = validate_model(&model(tables, relationships));
    let found = cycles(&report);
    assert_eq!(found.len(), MAX_REPORTED_CYCLES);
    let starts: Vec<&str> = found.iter().map(|finding| finding.table.as_str()).collect();
    assert_eq!(starts, vec!["T0", "T2", "T4", "T6", "T8", "T10", "T12", "T14"]);
    for finding in &found {
        assert_eq!(finding.cycle.len(), 3, "chain names include the closing table");
        assert_eq!(finding.cycle.first(), finding.cycle.last());
    }
}
