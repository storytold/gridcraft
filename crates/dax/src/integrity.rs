//! Referential-integrity and relationship-cardinality validation for a [`DataModel`].
//!
//! Phase 4 of the data-model roadmap asks the store to prove the model it holds is well-formed,
//! not just queryable: every fact key should find its dimension row, the one side of a
//! relationship should be unique, relationship chains should not loop, blank keys should be
//! visible, and a relationship declared one-to-many should actually be one-to-many rather than
//! silently many-to-many.
//!
//! Two entry points share one checker:
//!
//! * [`validate_model`] reads the model scalars — the values the author typed — and resolves
//!   relationships by name, so it also reports endpoints that do not exist and tables declared
//!   twice;
//! * [`ModelStorage::validate`] checks the materialized columns, i.e. exactly the keys the
//!   evaluator's `RELATED`/filter propagation uses. A faithful model (see
//!   [`ModelStorage::is_faithful`]) produces the same findings as [`validate_model`]; a model whose
//!   columns were promoted to text or decimal is validated as the evaluator sees it.
//!
//! # What is checked, and how severe it is
//!
//! | Check | Severity | Meaning |
//! |---|---|---|
//! | [`IntegrityCheck::OrphanRow`] | error | a non-blank `from` key has no row on the `to` side |
//! | [`IntegrityCheck::DuplicateKey`] | error | a non-blank `to` key appears in more than one row |
//! | [`IntegrityCheck::CircularRelationship`] | error | a relationship chain loops back on itself |
//! | [`IntegrityCheck::InvalidCardinality`] | error / warning | declared one-to-many is actually many-to-many (error), one-to-one or many-to-one (warning) |
//! | [`IntegrityCheck::BlankKey`] | warning | a key cell is BLANK on either side |
//! | [`IntegrityCheck::UnknownEndpoint`] | error | a relationship names a missing table or column |
//! | [`IntegrityCheck::DuplicateTable`] | error | a table name is defined more than once |
//!
//! Blank keys are reported as warnings and left out of the cardinality and duplicate-key
//! analysis, because DAX does not propagate a BLANK through a relationship.
//!
//! # Order
//!
//! Findings are deterministic. Duplicate table names come first, then one block per relationship in
//! model order: orphan findings (keys ascending), duplicate-key findings (keys ascending), the blank
//! `from` key, the blank `to` key and the cardinality finding. An unresolved endpoint replaces its
//! relationship's block with the [`IntegrityCheck::UnknownEndpoint`] finding, and cycles come last.
//!
//! # Bounds
//!
//! Nothing here panics or recurses: tables, relationships and keys are visited iteratively and in
//! model / [`ScalarKey`] order, row ids stay ascending, at most [`MAX_REPORTED_ROWS`] row ids are
//! kept per finding, at most [`MAX_REPORTED_KEYS`] key findings are emitted per relationship and at
//! most [`MAX_REPORTED_CYCLES`] cycles are listed. Totals (`total`, `orphan_rows`, ...) are the
//! uncapped counts, so a truncated report still says how much it truncated.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::model::{DataModel, ModelTable, Relationship};
use crate::storage::{ModelStorage, RelationshipIndex, TableStorage};
use crate::value::{Scalar, ScalarKey};

/// The most row ids one finding lists. The uncapped count is kept in [`IntegrityFinding::total`].
pub const MAX_REPORTED_ROWS: usize = 20;

/// The most key-level findings (orphans, duplicates) reported for one relationship.
pub const MAX_REPORTED_KEYS: usize = 16;

/// The most circular relationship chains reported for one model.
pub const MAX_REPORTED_CYCLES: usize = 8;

/// How badly a finding affects the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Warning,
    Error,
}

impl Severity {
    /// The report name of this severity.
    pub fn name(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
        }
    }

    /// Whether this is an error (as opposed to a warning).
    pub fn is_error(self) -> bool {
        self == Severity::Error
    }
}

/// The kind of integrity check that produced a finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IntegrityCheck {
    /// A non-blank `from` key has no matching row on the `to` side.
    OrphanRow,
    /// A non-blank `to` key appears in more than one row, so the one side is not unique.
    DuplicateKey,
    /// A relationship chain loops back to a table it already visited.
    CircularRelationship,
    /// The key distributions do not match the declared one-to-many cardinality.
    InvalidCardinality,
    /// A key cell is BLANK on one of the relationship's sides.
    BlankKey,
    /// A relationship names a table or column that does not exist.
    UnknownEndpoint,
    /// A table name is defined more than once, so name-based resolution is ambiguous.
    DuplicateTable,
}

impl IntegrityCheck {
    /// The report name of this check.
    pub fn name(self) -> &'static str {
        match self {
            IntegrityCheck::OrphanRow => "orphanRow",
            IntegrityCheck::DuplicateKey => "duplicateKey",
            IntegrityCheck::CircularRelationship => "circularRelationship",
            IntegrityCheck::InvalidCardinality => "invalidCardinality",
            IntegrityCheck::BlankKey => "blankKey",
            IntegrityCheck::UnknownEndpoint => "unknownEndpoint",
            IntegrityCheck::DuplicateTable => "duplicateTable",
        }
    }
}

/// A relationship's cardinality, as declared or as measured from the key distributions.
///
/// A model relationship is declared one-to-many by orientation: `from` is the many side (a fact
/// foreign key) and `to` is the one side (a dimension primary key). `bidirectional` makes the filter
/// cross both ways and `inactive` defers it to `USERELATIONSHIP`; neither changes which side must be
/// unique.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cardinality {
    /// Every key is unique on both sides.
    OneToOne,
    /// Many `from` rows share a `to` row: the declared shape.
    OneToMany,
    /// The `from` side is unique and the `to` side repeats, i.e. the relationship is stored
    /// backwards.
    ManyToOne,
    /// Keys repeat on both sides.
    ManyToMany,
}

impl Cardinality {
    /// The report name of this cardinality.
    pub fn name(self) -> &'static str {
        match self {
            Cardinality::OneToOne => "oneToOne",
            Cardinality::OneToMany => "oneToMany",
            Cardinality::ManyToOne => "manyToOne",
            Cardinality::ManyToMany => "manyToMany",
        }
    }

    /// The cardinality a model relationship declares by its orientation.
    pub fn declared() -> Self {
        Cardinality::OneToMany
    }
}

/// One integrity problem, with the endpoints it involves and the offending rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IntegrityFinding {
    /// Which check produced the finding.
    pub check: IntegrityCheck,
    /// Whether the model is broken (`error`) or merely suspect (`warning`).
    pub severity: Severity,
    /// The index in [`DataModel::relationships`] this finding belongs to, when it belongs to one.
    pub relationship: Option<usize>,
    /// The table the finding is about.
    pub table: String,
    /// The column the finding is about; empty for table-level findings such as a duplicate table.
    pub column: String,
    /// The other endpoint of the relationship, when the finding names two.
    pub related_table: Option<String>,
    /// The other endpoint's column, when the finding names two.
    pub related_column: Option<String>,
    /// The offending key, for per-key findings (orphans, duplicates, blank keys).
    pub key: Option<Scalar>,
    /// The first [`MAX_REPORTED_ROWS`] offending row ids, ascending.
    pub rows: Vec<usize>,
    /// The uncapped number of offending rows.
    pub total: usize,
    /// The tables of a circular chain, first table repeated at the end; empty otherwise.
    pub cycle: Vec<String>,
    /// A human-readable explanation, safe to show in a report.
    pub message: String,
}

impl IntegrityFinding {
    /// Whether this finding is an error.
    pub fn is_error(&self) -> bool {
        self.severity.is_error()
    }
}

/// The measured shape of one relationship.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelationshipCheck {
    /// The index in [`DataModel::relationships`].
    pub index: usize,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    /// Whether the relationship filters in both directions.
    pub bidirectional: bool,
    /// Whether the relationship is inactive until activated by `USERELATIONSHIP`.
    pub inactive: bool,
    /// The cardinality the orientation declares: always [`Cardinality::OneToMany`].
    pub declared: Cardinality,
    /// The cardinality the key distributions actually show.
    pub actual: Cardinality,
    /// Rows in the `from` table.
    pub from_rows: usize,
    /// Distinct non-blank keys in the `from` table.
    pub from_keys: usize,
    /// Rows in the `to` table.
    pub to_rows: usize,
    /// Distinct non-blank keys in the `to` table.
    pub to_keys: usize,
    /// Non-blank `from` keys with no `to` row.
    pub orphan_keys: usize,
    /// Rows carried by those orphan keys.
    pub orphan_rows: usize,
    /// Non-blank `to` keys that appear in more than one row.
    pub duplicate_to_keys: usize,
    /// Rows on the `from` side with a blank key.
    pub blank_from_keys: usize,
    /// Rows on the `to` side with a blank key.
    pub blank_to_keys: usize,
}

impl RelationshipCheck {
    /// Whether the measured cardinality matches what the relationship declares.
    pub fn is_valid(&self) -> bool {
        self.actual == Cardinality::OneToMany
    }
}

/// The full result of a validation pass.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IntegrityReport {
    /// The number of tables inspected.
    pub tables: usize,
    /// The total number of rows inspected across all tables.
    pub rows: usize,
    /// One entry per relationship whose endpoints resolved, in model order.
    pub relationships: Vec<RelationshipCheck>,
    /// Every finding, in the deterministic order documented on the module.
    pub findings: Vec<IntegrityFinding>,
}

impl IntegrityReport {
    /// `true` when nothing at all was reported.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }

    /// `true` when at least one finding is an error.
    pub fn has_errors(&self) -> bool {
        self.findings.iter().any(IntegrityFinding::is_error)
    }

    /// The error findings, in report order.
    pub fn errors(&self) -> impl Iterator<Item = &IntegrityFinding> {
        self.findings.iter().filter(|finding| finding.is_error())
    }

    /// The warning findings, in report order.
    pub fn warnings(&self) -> impl Iterator<Item = &IntegrityFinding> {
        self.findings.iter().filter(|finding| !finding.is_error())
    }

    /// The findings produced by one check, in report order.
    pub fn findings_for(&self, check: IntegrityCheck) -> impl Iterator<Item = &IntegrityFinding> {
        self.findings.iter().filter(move |finding| finding.check == check)
    }

    /// The first error message, for callers that only need one line.
    pub fn first_error(&self) -> Option<&str> {
        self.errors().next().map(|finding| finding.message.as_str())
    }
}

/// Validates a model by reading its rows.
///
/// Use this before materializing a store: it reports unknown endpoints and duplicate table names
/// instead of failing construction, so a hostile or half-written model still produces a report.
pub fn validate_model(model: &DataModel) -> IntegrityReport {
    let mut findings: Vec<IntegrityFinding> = Vec::new();
    let table_index = table_index_map(model, &mut findings);
    let rows = model.tables.iter().map(|table| table.rows.len()).sum();
    let mut relationships: Vec<RelationshipCheck> = Vec::new();
    let mut adjacency: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();

    for (index, relationship) in model.relationships.iter().enumerate() {
        let facts = match model_facts(model, &table_index, index, relationship) {
            Ok(facts) => facts,
            Err(missing) => {
                findings.push(unknown_endpoint(index, missing));
                continue;
            }
        };
        let (check, mut relationship_findings) = check_relationship(&facts);
        add_edges(&mut adjacency, &facts);
        findings.append(&mut relationship_findings);
        relationships.push(check);
    }

    sort_edges(&mut adjacency);
    findings.extend(cycle_findings(model.tables.iter().map(|table| table.name.as_str()), &adjacency, model.tables.len()));
    IntegrityReport { tables: model.tables.len(), rows, relationships, findings }
}

impl ModelStorage {
    /// Validates the materialized columns: the same checks as [`validate_model`], applied to the
    /// keys the evaluator's relationships use.
    ///
    /// A faithful store yields the same findings as [`validate_model`]. A store whose columns were
    /// promoted compares the promoted values, which is exactly what `RELATED` and filter
    /// propagation compare, so the report describes the storage rather than the JSON.
    pub fn validate(&self) -> IntegrityReport {
        let mut findings: Vec<IntegrityFinding> = Vec::new();
        let mut relationships: Vec<RelationshipCheck> = Vec::new();
        let mut adjacency: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();

        for (index, relationship) in self.relationships().iter().enumerate() {
            let facts = storage_facts(self, relationship, index);
            let (check, mut relationship_findings) = check_relationship(&facts);
            add_edges(&mut adjacency, &facts);
            findings.append(&mut relationship_findings);
            relationships.push(check);
        }

        sort_edges(&mut adjacency);
        let names: Vec<String> = self.tables().iter().map(|table| table.name().to_string()).collect();
        findings.extend(cycle_findings(names.iter().map(String::as_str), &adjacency, names.len()));
        IntegrityReport { tables: self.tables().len(), rows: self.total_rows(), relationships, findings }
    }
}

/// The key distributions of one relationship, as the checker consumes them.
struct RelationshipFacts {
    index: usize,
    from_table_index: usize,
    to_table_index: usize,
    from_table: String,
    from_column: String,
    to_table: String,
    to_column: String,
    bidirectional: bool,
    inactive: bool,
    from_rows: usize,
    to_rows: usize,
    from_keys: BTreeMap<ScalarKey, Occurrence>,
    to_keys: BTreeMap<ScalarKey, Occurrence>,
}

/// How often one key appears, with only the first [`MAX_REPORTED_ROWS`] row ids retained.
#[derive(Clone, Debug, Default)]
struct Occurrence {
    total: usize,
    rows: Vec<usize>,
}

impl Occurrence {
    fn push(&mut self, row: usize) {
        self.total = self.total.saturating_add(1);
        if self.rows.len() < MAX_REPORTED_ROWS {
            self.rows.push(row);
        }
    }

    fn from_rows(rows: &[usize]) -> Self {
        Occurrence { total: rows.len(), rows: rows.iter().copied().take(MAX_REPORTED_ROWS).collect() }
    }
}

fn add_edges(adjacency: &mut BTreeMap<usize, Vec<(usize, usize)>>, facts: &RelationshipFacts) {
    // A relationship is one undirected edge, identified by its index. Direction does not matter for
    // a chain: `A -> B` and `B -> A` used together loop exactly like `A -> B -> C -> A`, while a
    // single bidirectional relationship must not be its own cycle, which the edge id handles.
    if facts.from_table_index == facts.to_table_index {
        adjacency.entry(facts.from_table_index).or_default().push((facts.to_table_index, facts.index));
    } else {
        adjacency.entry(facts.from_table_index).or_default().push((facts.to_table_index, facts.index));
        adjacency.entry(facts.to_table_index).or_default().push((facts.from_table_index, facts.index));
    }
}

/// Sorts each adjacency list by `(neighbour, edge)` so the cycle walk is deterministic.
fn sort_edges(adjacency: &mut BTreeMap<usize, Vec<(usize, usize)>>) {
    for neighbours in adjacency.values_mut() {
        neighbours.sort_unstable();
    }
}

/// Resolves names to positions and reports duplicate table names.
fn table_index_map(model: &DataModel, findings: &mut Vec<IntegrityFinding>) -> BTreeMap<String, usize> {
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for (position, table) in model.tables.iter().enumerate() {
        let key = table.name.to_ascii_lowercase();
        if let Some(first) = index.get(&key).copied() {
            findings.push(IntegrityFinding {
                check: IntegrityCheck::DuplicateTable,
                severity: Severity::Error,
                relationship: None,
                table: table.name.clone(),
                column: String::new(),
                related_table: None,
                related_column: None,
                key: None,
                rows: Vec::new(),
                total: 0,
                cycle: Vec::new(),
                message: format!(
                    "table `{}` is defined more than once (first at position {first}, duplicate at position {position}); name-based lookups resolve to the first",
                    table.name
                ),
            });
        } else {
            index.insert(key, position);
        }
    }
    index
}

/// Builds the facts for one model relationship, or the endpoint that does not exist.
fn model_facts(
    model: &DataModel,
    table_index: &BTreeMap<String, usize>,
    index: usize,
    relationship: &Relationship,
) -> Result<RelationshipFacts, MissingEndpoint> {
    let from_position = table_index.get(&relationship.from_table.to_ascii_lowercase()).copied();
    let to_position = table_index.get(&relationship.to_table.to_ascii_lowercase()).copied();
    let Some(from_table) = from_position.and_then(|position| model.tables.get(position)) else {
        return Err(MissingEndpoint::new(relationship, &relationship.from_table, &relationship.from_column, true));
    };
    let Some(to_table) = to_position.and_then(|position| model.tables.get(position)) else {
        return Err(MissingEndpoint::new(relationship, &relationship.to_table, &relationship.to_column, false));
    };
    let (Some(from_column), Some(to_column)) = (column_index(from_table, &relationship.from_column), column_index(to_table, &relationship.to_column))
    else {
        let (missing_table, missing_column, missing_from) = if column_index(from_table, &relationship.from_column).is_none() {
            (from_table, &relationship.from_column, true)
        } else {
            (to_table, &relationship.to_column, false)
        };
        return Err(MissingEndpoint::new(relationship, &missing_table.name, missing_column, missing_from));
    };
    let (from_position, to_position) = (from_position.unwrap_or(0), to_position.unwrap_or(0));
    Ok(RelationshipFacts {
        index,
        from_table_index: from_position,
        to_table_index: to_position,
        from_table: from_table.name.clone(),
        from_column: column_name(from_table, from_column),
        to_table: to_table.name.clone(),
        to_column: column_name(to_table, to_column),
        bidirectional: relationship.bidirectional,
        inactive: relationship.inactive,
        from_rows: from_table.rows.len(),
        to_rows: to_table.rows.len(),
        from_keys: key_occurrences(from_table, from_column),
        to_keys: key_occurrences(to_table, to_column),
    })
}

/// A relationship endpoint that does not exist, small enough to return by value.
struct MissingEndpoint {
    table: String,
    column: String,
    related_table: String,
    related_column: String,
}

impl MissingEndpoint {
    fn new(relationship: &Relationship, table: &str, column: &str, missing_from: bool) -> Self {
        let (related_table, related_column) = if missing_from {
            (relationship.to_table.clone(), relationship.to_column.clone())
        } else {
            (relationship.from_table.clone(), relationship.from_column.clone())
        };
        MissingEndpoint { table: table.to_string(), column: column.to_string(), related_table, related_column }
    }
}

fn unknown_endpoint(index: usize, missing: MissingEndpoint) -> IntegrityFinding {
    IntegrityFinding {
        check: IntegrityCheck::UnknownEndpoint,
        severity: Severity::Error,
        relationship: Some(index),
        table: missing.table.clone(),
        column: missing.column.clone(),
        related_table: Some(missing.related_table.clone()),
        related_column: Some(missing.related_column.clone()),
        key: None,
        rows: Vec::new(),
        total: 0,
        cycle: Vec::new(),
        message: format!(
            "relationship {index} names `{}[{}]`, which does not exist (other endpoint `{}[{}]`)",
            missing.table, missing.column, missing.related_table, missing.related_column
        ),
    }
}

fn column_index(table: &ModelTable, name: &str) -> Option<usize> {
    table.columns.iter().position(|column| column.name.eq_ignore_ascii_case(name))
}

fn column_name(table: &ModelTable, column: usize) -> String {
    table.columns.get(column).map_or_else(String::new, |column| column.name.clone())
}

/// One entry per key value, with row ids capped at [`MAX_REPORTED_ROWS`].
///
/// A cell missing from a ragged row is BLANK, exactly as [`TableStorage::from_rows`] blank-extends
/// it, so both entry points agree on ragged models.
fn key_occurrences(table: &ModelTable, column: usize) -> BTreeMap<ScalarKey, Occurrence> {
    let blank = Scalar::Blank;
    let mut keys: BTreeMap<ScalarKey, Occurrence> = BTreeMap::new();
    for (row, cells) in table.rows.iter().enumerate() {
        let value = cells.get(column).unwrap_or(&blank);
        keys.entry(ScalarKey::from(value)).or_default().push(row);
    }
    keys
}

/// Builds the facts for one materialized relationship.
fn storage_facts(storage: &ModelStorage, relationship: &RelationshipIndex, index: usize) -> RelationshipFacts {
    RelationshipFacts {
        index,
        from_table_index: relationship.from_table,
        to_table_index: relationship.to_table,
        from_table: table_name(storage, relationship.from_table),
        from_column: column_name_of(storage, relationship.from_table, relationship.from_column),
        to_table: table_name(storage, relationship.to_table),
        to_column: column_name_of(storage, relationship.to_table, relationship.to_column),
        bidirectional: relationship.bidirectional,
        inactive: relationship.inactive,
        from_rows: storage.table(relationship.from_table).map_or(0, TableStorage::row_count),
        to_rows: storage.table(relationship.to_table).map_or(0, TableStorage::row_count),
        from_keys: relationship.from_rows.iter().map(|(key, rows)| (key.clone(), Occurrence::from_rows(rows))).collect(),
        to_keys: relationship.to_rows.iter().map(|(key, rows)| (key.clone(), Occurrence::from_rows(rows))).collect(),
    }
}

fn table_name(storage: &ModelStorage, table: usize) -> String {
    storage.table(table).map_or_else(String::new, |table| table.name().to_string())
}

fn column_name_of(storage: &ModelStorage, table: usize, column: usize) -> String {
    storage.column(table, column).map_or_else(String::new, |column| column.name().to_string())
}

/// Runs every per-relationship check and returns its summary plus its findings.
fn check_relationship(facts: &RelationshipFacts) -> (RelationshipCheck, Vec<IntegrityFinding>) {
    let mut findings: Vec<IntegrityFinding> = Vec::new();
    let from_unique = facts.from_keys.iter().filter(|(key, _)| !is_blank(key)).all(|(_, occurrence)| occurrence.total <= 1);
    let to_unique = facts.to_keys.iter().filter(|(key, _)| !is_blank(key)).all(|(_, occurrence)| occurrence.total <= 1);
    let actual = match (from_unique, to_unique) {
        (true, true) => Cardinality::OneToOne,
        (false, true) => Cardinality::OneToMany,
        (true, false) => Cardinality::ManyToOne,
        (false, false) => Cardinality::ManyToMany,
    };

    let from_keys = facts.from_keys.iter().filter(|(key, _)| !is_blank(key)).count();
    let to_keys = facts.to_keys.iter().filter(|(key, _)| !is_blank(key)).count();

    let mut orphan_keys = 0usize;
    let mut orphan_rows = 0usize;
    let mut reported = 0usize;
    for (key, occurrence) in &facts.from_keys {
        if is_blank(key) || facts.to_keys.contains_key(key) {
            continue;
        }
        orphan_keys += 1;
        orphan_rows = orphan_rows.saturating_add(occurrence.total);
        if reported < MAX_REPORTED_KEYS {
            reported += 1;
            findings.push(IntegrityFinding {
                check: IntegrityCheck::OrphanRow,
                severity: Severity::Error,
                relationship: Some(facts.index),
                table: facts.from_table.clone(),
                column: facts.from_column.clone(),
                related_table: Some(facts.to_table.clone()),
                related_column: Some(facts.to_column.clone()),
                key: Some(key.to_scalar()),
                rows: occurrence.rows.clone(),
                total: occurrence.total,
                cycle: Vec::new(),
                message: format!(
                    "{} row(s) in `{}[{}]` have key {} with no match in `{}[{}]`",
                    occurrence.total,
                    facts.from_table,
                    facts.from_column,
                    display_key(key),
                    facts.to_table,
                    facts.to_column
                ),
            });
        }
    }

    let mut duplicate_to_keys = 0usize;
    let mut reported = 0usize;
    for (key, occurrence) in &facts.to_keys {
        if is_blank(key) || occurrence.total <= 1 {
            continue;
        }
        duplicate_to_keys += 1;
        if reported < MAX_REPORTED_KEYS {
            reported += 1;
            findings.push(IntegrityFinding {
                check: IntegrityCheck::DuplicateKey,
                severity: Severity::Error,
                relationship: Some(facts.index),
                table: facts.to_table.clone(),
                column: facts.to_column.clone(),
                related_table: Some(facts.from_table.clone()),
                related_column: Some(facts.from_column.clone()),
                key: Some(key.to_scalar()),
                rows: occurrence.rows.clone(),
                total: occurrence.total,
                cycle: Vec::new(),
                message: format!(
                    "key {} appears in {} row(s) of `{}[{}]`, so the one side of the relationship is not unique",
                    display_key(key),
                    occurrence.total,
                    facts.to_table,
                    facts.to_column
                ),
            });
        }
    }

    let mut blank_from_keys = 0usize;
    if let Some(occurrence) = facts.from_keys.get(&ScalarKey::Blank) {
        blank_from_keys = occurrence.total;
        findings.push(blank_key(facts, true, occurrence));
    }
    let mut blank_to_keys = 0usize;
    if let Some(occurrence) = facts.to_keys.get(&ScalarKey::Blank) {
        blank_to_keys = occurrence.total;
        findings.push(blank_key(facts, false, occurrence));
    }

    match actual {
        Cardinality::ManyToMany => findings.push(cardinality_finding(facts, actual, Severity::Error)),
        Cardinality::OneToOne | Cardinality::ManyToOne => findings.push(cardinality_finding(facts, actual, Severity::Warning)),
        Cardinality::OneToMany => {}
    }

    let check = RelationshipCheck {
        index: facts.index,
        from_table: facts.from_table.clone(),
        from_column: facts.from_column.clone(),
        to_table: facts.to_table.clone(),
        to_column: facts.to_column.clone(),
        bidirectional: facts.bidirectional,
        inactive: facts.inactive,
        declared: Cardinality::declared(),
        actual,
        from_rows: facts.from_rows,
        from_keys,
        to_rows: facts.to_rows,
        to_keys,
        orphan_keys,
        orphan_rows,
        duplicate_to_keys,
        blank_from_keys,
        blank_to_keys,
    };
    (check, findings)
}

fn blank_key(facts: &RelationshipFacts, from_side: bool, occurrence: &Occurrence) -> IntegrityFinding {
    let (table, column, related_table, related_column) = if from_side {
        (&facts.from_table, &facts.from_column, &facts.to_table, &facts.to_column)
    } else {
        (&facts.to_table, &facts.to_column, &facts.from_table, &facts.from_column)
    };
    IntegrityFinding {
        check: IntegrityCheck::BlankKey,
        severity: Severity::Warning,
        relationship: Some(facts.index),
        table: table.clone(),
        column: column.clone(),
        related_table: Some(related_table.clone()),
        related_column: Some(related_column.clone()),
        key: Some(Scalar::Blank),
        rows: occurrence.rows.clone(),
        total: occurrence.total,
        cycle: Vec::new(),
        message: format!("{} row(s) in `{table}[{column}]` have a blank relationship key", occurrence.total),
    }
}

fn cardinality_finding(facts: &RelationshipFacts, actual: Cardinality, severity: Severity) -> IntegrityFinding {
    let endpoint = format!("`{}[{}]` -> `{}[{}]`", facts.from_table, facts.from_column, facts.to_table, facts.to_column);
    let message = match actual {
        Cardinality::ManyToMany => {
            format!("relationship {endpoint} declares one-to-many but both sides have duplicate keys (many-to-many); the one side must be unique")
        }
        Cardinality::OneToOne => format!("relationship {endpoint} declares one-to-many but every key is unique on both sides (one-to-one)"),
        Cardinality::ManyToOne => format!(
            "relationship {endpoint} declares one-to-many but the from side is unique and the to side repeats (many-to-one); the relationship is stored in the wrong direction"
        ),
        Cardinality::OneToMany => format!("relationship {endpoint} is one-to-many"),
    };
    IntegrityFinding {
        check: IntegrityCheck::InvalidCardinality,
        severity,
        relationship: Some(facts.index),
        table: facts.from_table.clone(),
        column: facts.from_column.clone(),
        related_table: Some(facts.to_table.clone()),
        related_column: Some(facts.to_column.clone()),
        key: None,
        rows: Vec::new(),
        total: 0,
        cycle: Vec::new(),
        message,
    }
}

fn is_blank(key: &ScalarKey) -> bool {
    matches!(key, ScalarKey::Blank)
}

fn display_key(key: &ScalarKey) -> String {
    match key.to_scalar() {
        Scalar::Blank => "BLANK".to_string(),
        Scalar::String(text) => format!("`{text}`"),
        other => other.to_text(),
    }
}

/// Finds circular relationship chains with an explicit-stack depth-first search.
///
/// Each relationship is one undirected edge carrying its model index, so a chain is a set of
/// relationships that leads back to an already-visited table without reusing the edge it arrived
/// on. That models the two real cases exactly: `A -> B -> C -> A` (three links), two relationships
/// between the same pair of tables, and a self relationship `T -> T` are chains; a single
/// relationship — even a bidirectional one — is not, because its reverse traversal would be the
/// same edge. A chain is reported once per distinct cycle (canonicalised to start at its smallest
/// table id) and at most [`MAX_REPORTED_CYCLES`] times per model, so a hostile graph cannot flood
/// the report. A dense graph can contain more cycles than the cap; the reported ones are the first
/// the deterministic walk finds.
fn cycle_findings<'a>(
    names: impl Iterator<Item = &'a str>,
    adjacency: &BTreeMap<usize, Vec<(usize, usize)>>,
    table_count: usize,
) -> Vec<IntegrityFinding> {
    let names: Vec<&str> = names.collect();
    let mut findings = Vec::new();
    for cycle in detect_cycles(adjacency, table_count) {
        let chain: Vec<String> = cycle.iter().map(|table| names.get(*table).copied().unwrap_or_default().to_string()).collect();
        findings.push(IntegrityFinding {
            check: IntegrityCheck::CircularRelationship,
            severity: Severity::Error,
            relationship: None,
            table: chain.first().cloned().unwrap_or_default(),
            column: String::new(),
            related_table: None,
            related_column: None,
            key: None,
            rows: Vec::new(),
            total: 0,
            cycle: chain.clone(),
            message: format!("circular relationship chain: {}", chain.join(" -> ")),
        });
    }
    findings
}

/// Depth-first cycle detection over undirected relationship edges, iterative and bounded.
///
/// The stack entry keeps the edge the walk arrived on so the walk never counts that edge as the
/// second half of a cycle. Grey marks the current path (a grey neighbour closes a chain), black
/// marks a finished table; in an undirected depth-first walk every non-tree edge reaches a grey
/// ancestor, so black pruning loses nothing.
fn detect_cycles(adjacency: &BTreeMap<usize, Vec<(usize, usize)>>, table_count: usize) -> Vec<Vec<usize>> {
    const WHITE: u8 = 0;
    const GREY: u8 = 1;
    const BLACK: u8 = 2;
    let mut colour = vec![WHITE; table_count];
    let mut cycles: BTreeSet<Vec<usize>> = BTreeSet::new();
    for start in 0..table_count {
        if colour.get(start).copied().unwrap_or(BLACK) != WHITE {
            continue;
        }
        if let Some(slot) = colour.get_mut(start) {
            *slot = GREY;
        }
        let mut stack: Vec<(usize, usize, Option<usize>)> = vec![(start, 0, None)];
        let mut path: Vec<usize> = vec![start];
        while let Some(&(node, cursor, arrived_on)) = stack.last() {
            let neighbours = adjacency.get(&node).map_or(&[][..], Vec::as_slice);
            if cursor < neighbours.len() {
                if let Some(entry) = stack.last_mut() {
                    entry.1 = cursor + 1;
                }
                let (next, edge) = neighbours[cursor];
                if arrived_on == Some(edge) {
                    // The edge we came in on is not a second path back.
                    continue;
                }
                match colour.get(next).copied().unwrap_or(BLACK) {
                    WHITE => {
                        if let Some(slot) = colour.get_mut(next) {
                            *slot = GREY;
                        }
                        stack.push((next, 0, Some(edge)));
                        path.push(next);
                    }
                    GREY => {
                        if let Some(position) = path.iter().position(|table| *table == next) {
                            let cycle = path.get(position..).map_or_else(Vec::new, <[usize]>::to_vec);
                            cycles.insert(canonical_cycle(&cycle));
                            if cycles.len() >= MAX_REPORTED_CYCLES {
                                return cycles.into_iter().collect();
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                stack.pop();
                path.pop();
                if let Some(slot) = colour.get_mut(node) {
                    *slot = BLACK;
                }
            }
        }
    }
    cycles.into_iter().collect()
}

/// Rotates a cycle so it starts at its smallest table id and closes the loop explicitly.
fn canonical_cycle(nodes: &[usize]) -> Vec<usize> {
    if nodes.is_empty() {
        return Vec::new();
    }
    let Some(position) = nodes.iter().enumerate().min_by_key(|(_, table)| **table).map(|(position, _)| position) else {
        return Vec::new();
    };
    let mut cycle: Vec<usize> = nodes.get(position..).unwrap_or_default().to_vec();
    cycle.extend_from_slice(nodes.get(..position).unwrap_or_default());
    if let Some(first) = cycle.first().copied() {
        cycle.push(first);
    }
    cycle
}
