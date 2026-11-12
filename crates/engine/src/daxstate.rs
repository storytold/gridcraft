//! Taking back the data-model edits an undo group made.
//!
//! `Session::dax_model`, `Session::dax_metadata` and `Session::dax_view` are session state, not
//! the document's, and the `dataModel.*` editing commands change them without an undo step of
//! their own. An undo group therefore records the inverse of every data-model edit made while it
//! is the innermost group on the active document: one [`Change`] per entity of that state the
//! edit changed — a measure, a relationship, a table, a column's declared type, a metadata entry,
//! a Data View setting.
//!
//! A rollback replays its group's changes backwards. Each one puts its `before` value back only
//! while the entity still holds the `after` value the edit left, so an entity another open group
//! edited in the meantime keeps that group's edit. Restoring one snapshot of the whole state, as
//! an undo group used to, would throw the other group's work away: the state is shared by every
//! document, so a snapshot cannot tell whose edits are in it.

use gridcraft_dax::model::{ColumnMetadata, ModelMetadata};
use gridcraft_dax::{DataModel, Measure, ModelTable, Relationship};

use crate::cmd::dax::{ModelView, TableView};

/// What identifies one entity of the data-model state. Names are lowercased: the model matches
/// table, column, measure and relationship names case-insensitively.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    /// The whole model: it was absent, or the edit replaced more than an entity can hold (a load).
    Model,
    /// The whole metadata, for the same reason (a load adds or drops tables and columns).
    Metadata,
    Table(String),
    ColumnType(String, String),
    Measure(String),
    Relationship(RelKey),
    TableDate(String),
    ColumnMeta(String, String),
    TableView(String),
    ShowHidden,
    ShowImplicitMeasures,
}

/// A relationship, by its four names, lowercased.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RelKey {
    from_table: String,
    from_column: String,
    to_table: String,
    to_column: String,
}

impl RelKey {
    /// The identity of `relationship` with all four names lowercased, the form
    /// [`RelKey::matches`] compares against.
    fn of(relationship: &Relationship) -> RelKey {
        RelKey {
            from_table: relationship.from_table.to_ascii_lowercase(),
            from_column: relationship.from_column.to_ascii_lowercase(),
            to_table: relationship.to_table.to_ascii_lowercase(),
            to_column: relationship.to_column.to_ascii_lowercase(),
        }
    }

    /// Whether `relationship` joins these four endpoints, matching names case-insensitively (as
    /// the model does).
    fn matches(&self, relationship: &Relationship) -> bool {
        self.from_table.eq_ignore_ascii_case(&relationship.from_table)
            && self.from_column.eq_ignore_ascii_case(&relationship.from_column)
            && self.to_table.eq_ignore_ascii_case(&relationship.to_table)
            && self.to_column.eq_ignore_ascii_case(&relationship.to_column)
    }
}

/// The value of one entity, absent (`None`) when the collection it lives in has no entry for it.
#[derive(Clone, Debug, PartialEq)]
enum Entity {
    Model(Option<DataModel>),
    Metadata(ModelMetadata),
    Table(Option<ModelTable>),
    ColumnType(Option<String>),
    Measure(Option<Measure>),
    Relationship(Option<Relationship>),
    TableDate(Option<String>),
    ColumnMeta(Option<ColumnMetadata>),
    TableView(Option<TableView>),
    ShowHidden(bool),
    ShowImplicitMeasures(bool),
}

/// One data-model edit, as the group that made it holds it: the entity, what it held before and
/// after, and where it sat in its collection.
#[derive(Clone, Debug)]
pub(crate) struct Change {
    /// Position in the session's order of data-model edits: a rollback reverts the newest first.
    pub(crate) seq: u64,
    key: Key,
    before: Entity,
    after: Entity,
    /// Where `before` sat in its collection, for a rollback that has to put a dropped value back.
    at: usize,
}

impl Change {
    /// A change of `key` from `before` to `after`, at position `at` in its collection. `seq`
    /// starts at 0: the session stamps it when it records the change.
    fn new(key: Key, before: Entity, after: Entity, at: usize) -> Change {
        Change { seq: 0, key, before, after, at }
    }
}

/// Puts `change`'s `before` value back, unless the entity no longer holds the value the edit left
/// (another open group changed it since, and its edit stays).
pub(crate) fn revert(change: &Change, model: &mut Option<DataModel>, metadata: &mut ModelMetadata, view: &mut ModelView) {
    if read(model, metadata, view, &change.key) != change.after {
        return;
    }
    write(model, metadata, view, &change.key, change.before.clone(), change.at);
}

/// The changes that take `after` back to `before` for the model.
pub(crate) fn model_changes(before: &Option<DataModel>, after: Option<&DataModel>) -> Vec<Change> {
    let mut out = Vec::new();
    match (before.as_ref(), after) {
        (None, None) => {}
        (Some(before), Some(after)) => {
            if before == after {
                return out;
            }
            model_entities(before, after, &mut out);
            // A change no entity explains (tables or measures re-ordered by a load) is the whole
            // model's.
            if out.is_empty() {
                out.push(Change::new(Key::Model, Entity::Model(Some(before.clone())), Entity::Model(Some(after.clone())), 0));
            }
        }
        _ => out.push(Change::new(Key::Model, Entity::Model(before.clone()), Entity::Model(after.cloned()), 0)),
    }
    out
}

/// Appends one change per table, column type, measure and relationship the two models differ in.
/// A table whose columns and rows match is compared column type by column type; any other
/// difference is a change of the whole table.
///
/// A collection's removed entities are emitted before its other changes and by descending index:
/// a rollback reverts the newest change first, so the removals come back by ascending index and
/// the entities end up in their original order (CodeRabbit, #3412). Editing additions before the
/// removals would instead insert each entity at an index the still-present addition shifted.
fn model_entities(before: &DataModel, after: &DataModel, out: &mut Vec<Change>) {
    let mut removed: Vec<Change> = Vec::new();
    let mut changed: Vec<Change> = Vec::new();
    for name in names_union(before.tables.iter().map(|table| &table.name), after.tables.iter().map(|table| &table.name)) {
        let found = (before.table_index(&name), after.table_index(&name));
        let at = found.0.or(found.1).unwrap_or(0);
        let (was, is) = (found.0.map(|i| &before.tables[i]), found.1.map(|i| &after.tables[i]));
        match (was, is) {
            (Some(was), Some(is)) => {
                if was == is {
                    if found.0 != found.1 {
                        // The entity survived unchanged but moved: a load that reorders tables
                        // while adding a measure would otherwise leave the order wrong after the
                        // rollback (CodeRabbit, #3412). The reorder rides on the entity's own
                        // key, so an entity another group edited since keeps that group's edit.
                        changed.push(Change::new(
                            Key::Table(name),
                            Entity::Table(Some(was.clone())),
                            Entity::Table(Some(is.clone())),
                            found.0.unwrap_or(0),
                        ));
                    }
                    continue;
                }
                if same_table_shape(was, is) {
                    column_types(&name, was, is, &mut changed);
                } else {
                    changed.push(Change::new(Key::Table(name), Entity::Table(Some(was.clone())), Entity::Table(Some(is.clone())), at));
                }
            }
            (Some(was), None) => removed.push(Change::new(Key::Table(name), Entity::Table(Some(was.clone())), Entity::Table(None), at)),
            (None, Some(is)) => changed.push(Change::new(Key::Table(name), Entity::Table(None), Entity::Table(Some(is.clone())), at)),
            (None, None) => {}
        }
    }
    out.extend(removed.drain(..).rev());
    out.extend(changed);

    let mut removed: Vec<Change> = Vec::new();
    let mut changed: Vec<Change> = Vec::new();
    for name in names_union(before.measures.iter().map(|measure| &measure.name), after.measures.iter().map(|measure| &measure.name)) {
        let found = (before.measure_index(&name), after.measure_index(&name));
        let (was, is) = (found.0.map(|i| &before.measures[i]), found.1.map(|i| &after.measures[i]));
        if was == is {
            if found.0.is_some_and(|was| Some(was) != found.1) {
                // Same rule as tables: an unchanged measure that only moved is recorded so
                // the rollback puts the order back (CodeRabbit, #3412).
                changed.push(Change::new(Key::Measure(name), Entity::Measure(was.cloned()), Entity::Measure(is.cloned()), found.0.unwrap_or(0)));
            }
            continue;
        }
        let change = Change::new(Key::Measure(name), Entity::Measure(was.cloned()), Entity::Measure(is.cloned()), found.0.unwrap_or(0));
        if is.is_none() { removed.push(change) } else { changed.push(change) }
    }
    out.extend(removed.drain(..).rev());
    out.extend(changed);

    let mut removed: Vec<Change> = Vec::new();
    let mut changed: Vec<Change> = Vec::new();
    for key in relationship_keys(before, after) {
        let found = (
            before.relationships.iter().position(|relationship| key.matches(relationship)),
            after.relationships.iter().position(|relationship| key.matches(relationship)),
        );
        let (was, is) = (found.0.map(|i| &before.relationships[i]), found.1.map(|i| &after.relationships[i]));
        if was == is {
            if found.0.is_some_and(|was| Some(was) != found.1) {
                // Same rule: an unchanged relationship that only moved is recorded.
                changed.push(Change::new(
                    Key::Relationship(key),
                    Entity::Relationship(was.cloned()),
                    Entity::Relationship(is.cloned()),
                    found.0.unwrap_or(0),
                ));
            }
            continue;
        }
        let change = Change::new(Key::Relationship(key), Entity::Relationship(was.cloned()), Entity::Relationship(is.cloned()), found.0.unwrap_or(0));
        if is.is_none() { removed.push(change) } else { changed.push(change) }
    }
    out.extend(removed.drain(..).rev());
    out.extend(changed);
}

/// A table whose columns and rows are the same on both sides: only their declared types can have
/// changed, and each is a change of its own.
fn same_table_shape(was: &ModelTable, is: &ModelTable) -> bool {
    was.name == is.name
        && was.rows == is.rows
        && was.columns.len() == is.columns.len()
        && was.columns.iter().zip(&is.columns).all(|(was, is)| was.name == is.name)
}

/// Appends a change for every column of a same-shape table whose declared type differs.
fn column_types(table: &str, was: &ModelTable, is: &ModelTable, out: &mut Vec<Change>) {
    for (index, (was, is)) in was.columns.iter().zip(&is.columns).enumerate() {
        if was.data_type == is.data_type {
            continue;
        }
        out.push(Change::new(
            Key::ColumnType(table.to_string(), was.name.to_ascii_lowercase()),
            Entity::ColumnType(was.data_type.clone()),
            Entity::ColumnType(is.data_type.clone()),
            index,
        ));
    }
}

/// The changes that take `after` back to `before` for the model metadata.
pub(crate) fn metadata_changes(before: &ModelMetadata, after: &ModelMetadata) -> Vec<Change> {
    let mut out = Vec::new();
    if before == after {
        return out;
    }
    // A load can add or drop tables and columns; a date column and a column's settings can only
    // change where both sides have the entry.
    if !same_metadata_layout(before, after) {
        out.push(Change::new(Key::Metadata, Entity::Metadata(before.clone()), Entity::Metadata(after.clone()), 0));
        return out;
    }
    for name in names_union(before.tables.iter().map(|table| &table.name), after.tables.iter().map(|table| &table.name)) {
        let (Some(was), Some(is)) = (before.table(&name), after.table(&name)) else { continue };
        if was.date_column != is.date_column {
            out.push(Change::new(
                Key::TableDate(name.clone()),
                Entity::TableDate(was.date_column.clone()),
                Entity::TableDate(is.date_column.clone()),
                0,
            ));
        }
        for column in names_union(was.columns.iter().map(|column| &column.name), is.columns.iter().map(|column| &column.name)) {
            let found = (
                was.columns.iter().find(|entry| entry.name.eq_ignore_ascii_case(&column)),
                is.columns.iter().find(|entry| entry.name.eq_ignore_ascii_case(&column)),
            );
            if found.0 == found.1 || found.0.is_none() || found.1.is_none() {
                continue;
            }
            out.push(Change::new(
                Key::ColumnMeta(name.clone(), column.to_ascii_lowercase()),
                Entity::ColumnMeta(found.0.cloned()),
                Entity::ColumnMeta(found.1.cloned()),
                0,
            ));
        }
    }
    // The layout matched, so a change no entity explains is the metadata as a whole's.
    if out.is_empty() {
        out.push(Change::new(Key::Metadata, Entity::Metadata(before.clone()), Entity::Metadata(after.clone()), 0));
    }
    out
}

/// Both sides hold the same tables and the same columns in them, so only the settings of an entry
/// can have changed.
fn same_metadata_layout(before: &ModelMetadata, after: &ModelMetadata) -> bool {
    before.tables.len() == after.tables.len()
        && before.tables.iter().all(|table| {
            after.table(&table.name).is_some_and(|other| {
                table.columns.len() == other.columns.len()
                    && table.columns.iter().all(|column| other.columns.iter().any(|entry| entry.name.eq_ignore_ascii_case(&column.name)))
            })
        })
}

/// The changes that take `after` back to `before` for the Data View.
pub(crate) fn view_changes(before: &ModelView, after: &ModelView) -> Vec<Change> {
    let mut out = Vec::new();
    if before == after {
        return out;
    }
    if before.show_hidden != after.show_hidden {
        out.push(Change::new(Key::ShowHidden, Entity::ShowHidden(before.show_hidden), Entity::ShowHidden(after.show_hidden), 0));
    }
    if before.show_implicit_measures != after.show_implicit_measures {
        out.push(Change::new(
            Key::ShowImplicitMeasures,
            Entity::ShowImplicitMeasures(before.show_implicit_measures),
            Entity::ShowImplicitMeasures(after.show_implicit_measures),
            0,
        ));
    }
    for name in names_union(before.tables.keys(), after.tables.keys()) {
        let found = (before.tables.get(&name), after.tables.get(&name));
        if found.0 == found.1 {
            continue;
        }
        out.push(Change::new(Key::TableView(name), Entity::TableView(found.0.cloned()), Entity::TableView(found.1.cloned()), 0));
    }
    out
}

/// Every name spelled on either side, once, in the order first seen (matching ignores case).
fn names_union<'a>(before: impl Iterator<Item = &'a String>, after: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in before.chain(after) {
        let lower = name.to_ascii_lowercase();
        if !out.contains(&lower) {
            out.push(lower);
        }
    }
    out
}

/// Every relationship on either side, once, in the order first seen.
fn relationship_keys(before: &DataModel, after: &DataModel) -> Vec<RelKey> {
    let mut out: Vec<RelKey> = Vec::new();
    for relationship in before.relationships.iter().chain(&after.relationships) {
        let key = RelKey::of(relationship);
        if !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// The state's current value for `key`: an absent entity reads as its `None`, the way a rollback
/// compares it with the value its edit left.
fn read(model: &Option<DataModel>, metadata: &ModelMetadata, view: &ModelView, key: &Key) -> Entity {
    match key {
        Key::Model => Entity::Model(model.clone()),
        Key::Metadata => Entity::Metadata(metadata.clone()),
        Key::Table(name) => Entity::Table(model.as_ref().and_then(|model| model.table_index(name).map(|index| model.tables[index].clone()))),
        Key::ColumnType(table, column) => Entity::ColumnType(model.as_ref().and_then(|model| {
            let table = model.table_index(table)?;
            let column = model.column_index(table, column)?;
            model.tables[table].columns[column].data_type.clone()
        })),
        Key::Measure(name) => Entity::Measure(model.as_ref().and_then(|model| model.measure_index(name).map(|index| model.measures[index].clone()))),
        Key::Relationship(key) => {
            Entity::Relationship(model.as_ref().and_then(|model| model.relationships.iter().find(|entry| key.matches(entry)).cloned()))
        }
        Key::TableDate(name) => Entity::TableDate(metadata.table(name).and_then(|table| table.date_column.clone())),
        Key::ColumnMeta(table, column) => Entity::ColumnMeta(metadata.column(table, column).cloned()),
        Key::TableView(name) => Entity::TableView(view.tables.get(name).cloned()),
        Key::ShowHidden => Entity::ShowHidden(view.show_hidden),
        Key::ShowImplicitMeasures => Entity::ShowImplicitMeasures(view.show_implicit_measures),
    }
}

/// Puts `value` back at `key`, inserting a value whose collection lost it at `at`. A key whose
/// model or metadata side has since gone (no model loaded, table dropped) is left alone.
fn write(model: &mut Option<DataModel>, metadata: &mut ModelMetadata, view: &mut ModelView, key: &Key, value: Entity, at: usize) {
    match (key, value) {
        (Key::Model, Entity::Model(state)) => *model = state,
        (Key::Metadata, Entity::Metadata(state)) => *metadata = state,
        (Key::Table(name), Entity::Table(state)) => {
            let Some(model) = model.as_mut() else { return };
            let found = model.table_index(name);
            match (state, found) {
                // A surviving entity that only moved comes back to its recorded position:
                // take it out where it sits, then put it back where it was, so a load that
                // reordered tables while adding a measure rolls back to the original order
                // (CodeRabbit, #3412). Another group's later edit of that entity keeps its
                // edit: `revert` skips a change whose current value is no longer `after`.
                (Some(table), Some(index)) if index != at => {
                    model.tables.remove(index);
                    // The recorded position counts the entity itself, so removing an entity
                    // at or before it shifts the target down by one.
                    let target = if index < at { at.saturating_sub(1) } else { at };
                    model.tables.insert(target.min(model.tables.len()), table);
                }
                (Some(table), Some(index)) => model.tables[index] = table,
                (Some(table), None) => model.tables.insert(at.min(model.tables.len()), table),
                (None, Some(index)) => {
                    model.tables.remove(index);
                }
                (None, None) => {}
            }
        }
        (Key::ColumnType(table, column), Entity::ColumnType(data_type)) => {
            let Some(model) = model.as_mut() else { return };
            let Some(table) = model.table_index(table) else { return };
            let Some(column) = model.column_index(table, column) else { return };
            model.tables[table].columns[column].data_type = data_type;
        }
        (Key::Measure(name), Entity::Measure(state)) => {
            let Some(model) = model.as_mut() else { return };
            let found = model.measure_index(name);
            match (state, found) {
                (Some(measure), Some(index)) if index != at => {
                    model.measures.remove(index);
                    let target = if index < at { at.saturating_sub(1) } else { at };
                    model.measures.insert(target.min(model.measures.len()), measure);
                }
                (Some(measure), Some(index)) => model.measures[index] = measure,
                (Some(measure), None) => model.measures.insert(at.min(model.measures.len()), measure),
                (None, Some(index)) => {
                    model.measures.remove(index);
                }
                (None, None) => {}
            }
        }
        (Key::Relationship(key), Entity::Relationship(state)) => {
            let Some(model) = model.as_mut() else { return };
            let found = model.relationships.iter().position(|entry| key.matches(entry));
            match (state, found) {
                (Some(relationship), Some(index)) if index != at => {
                    model.relationships.remove(index);
                    let target = if index < at { at.saturating_sub(1) } else { at };
                    model.relationships.insert(target.min(model.relationships.len()), relationship);
                }
                (Some(relationship), Some(index)) => model.relationships[index] = relationship,
                (Some(relationship), None) => model.relationships.insert(at.min(model.relationships.len()), relationship),
                (None, Some(index)) => {
                    model.relationships.remove(index);
                }
                (None, None) => {}
            }
        }
        (Key::TableDate(table), Entity::TableDate(date)) => {
            if let Some(table) = metadata.table_mut(table) {
                table.date_column = date;
            }
        }
        (Key::ColumnMeta(table, column), Entity::ColumnMeta(state)) => {
            let Some(table) = metadata.table_mut(table) else { return };
            let found = table.columns.iter().position(|entry| entry.name.eq_ignore_ascii_case(column));
            match (state, found) {
                (Some(entry), Some(index)) => table.columns[index] = entry,
                (Some(entry), None) => table.columns.push(entry),
                (None, Some(index)) => {
                    table.columns.remove(index);
                }
                (None, None) => {}
            }
        }
        (Key::TableView(name), Entity::TableView(state)) => match state {
            Some(state) => {
                view.tables.insert(name.clone(), state);
            }
            None => {
                view.tables.remove(name);
            }
        },
        (Key::ShowHidden, Entity::ShowHidden(show)) => {
            view.show_hidden = show;
        }
        (Key::ShowImplicitMeasures, Entity::ShowImplicitMeasures(show)) => {
            view.show_implicit_measures = show;
        }
        // A key and a value of different entities never pair up: the diff builds both.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use gridcraft_dax::model::{ModelMetadata, ModelTable};

    use super::*;

    const MODEL: &str = r#"{
        "tables": [{"name":"Sales","columns":[{"name":"Region"},{"name":"Amount"}],
                    "rows":[["East",10],["West",20]]}],
        "relationships": [],
        "measures": [{"name":"Total Sales","formula":"SUM(Sales[Amount])","table":"Sales"}]
    }"#;

    /// The [`MODEL`] fixture parsed as a model.
    fn loaded_model() -> DataModel {
        serde_json::from_str(MODEL).expect("model json")
    }

    /// The metadata [`MODEL`] carries, read the way `dataModel.load` reads it.
    fn loaded_metadata() -> ModelMetadata {
        ModelMetadata::from_model_json(MODEL).expect("metadata json")
    }

    /// A measure with no table, format or parsed expression, for diffs that compare values only.
    fn measure(name: &str, formula: &str) -> Measure {
        Measure { name: name.into(), formula: formula.into(), table: None, format: None, expr: None }
    }

    /// Reverts the changes newest first, as
    /// [`Session::group_rollback`](crate::Session::group_rollback) does.
    fn revert_all(changes: &[Change], model: &mut Option<DataModel>, metadata: &mut ModelMetadata, view: &mut ModelView) {
        for change in changes.iter().rev() {
            revert(change, model, metadata, view);
        }
    }

    /// A measure edit reverts that measure only: one another edit added since stays.
    #[test]
    fn a_measure_edit_reverts_its_own_measure_only() {
        // The state is session-wide, so a rollback must put back what its edit changed and
        // nothing else.
        let before = Some(loaded_model());
        let mut after = before.clone();
        after.as_mut().expect("model").measures.push(measure("Double", "1"));
        let changes = model_changes(&before, after.as_ref());
        assert_eq!(changes.len(), 1, "one measure changed");

        // Another group added a measure of its own since: the rollback leaves it.
        let mut current = after.clone();
        current.as_mut().expect("model").measures.push(measure("Other", "2"));
        let mut metadata = ModelMetadata::default();
        let mut view = ModelView::default();
        revert_all(&changes, &mut current, &mut metadata, &mut view);
        let names: Vec<&str> = current.as_ref().expect("model").measures.iter().map(|entry| entry.name.as_str()).collect();
        assert_eq!(names, ["Total Sales", "Other"], "the group's measure went, the other group's stayed");

        // With nothing else in the way the state is exactly as it was.
        let mut current = after;
        revert_all(&changes, &mut current, &mut metadata, &mut view);
        assert_eq!(current, before);
    }

    /// Loading a model over none reverts to no model, no metadata and the empty view.
    #[test]
    fn a_load_reverts_to_no_model_with_its_metadata_and_view() {
        let model = None;
        let metadata = ModelMetadata::default();
        let view = ModelView::default();
        let model_changes = model_changes(&model, Some(&loaded_model()));
        let metadata_changes = metadata_changes(&metadata, &loaded_metadata());
        let mut loaded = Some(loaded_model());
        let mut loaded_metadata = loaded_metadata();
        let mut loaded_view = ModelView { show_hidden: true, ..ModelView::default() };
        let view_changes = view_changes(&view, &loaded_view);

        revert_all(&model_changes, &mut loaded, &mut loaded_metadata, &mut loaded_view);
        revert_all(&metadata_changes, &mut loaded, &mut loaded_metadata, &mut loaded_view);
        revert_all(&view_changes, &mut loaded, &mut loaded_metadata, &mut loaded_view);
        assert_eq!(loaded, model, "no model again");
        assert_eq!(loaded_metadata, metadata, "no metadata again");
        assert_eq!(loaded_view, view, "the view is back");
    }

    /// A column type edit reverts that column only: a type another edit set since stays.
    #[test]
    fn a_column_type_edit_reverts_that_column_only() {
        let mut before = Some(loaded_model());
        before.as_mut().expect("model").tables[0].columns[1].data_type = Some("Double".into());
        let mut after = before.clone();
        after.as_mut().expect("model").tables[0].columns[0].data_type = Some("String".into());
        let changes = model_changes(&before, after.as_ref());
        assert_eq!(changes.len(), 1, "one column type changed");

        // A second group sets the other column's type afterwards: it survives.
        let mut current = after;
        current.as_mut().expect("model").tables[0].columns[1].data_type = Some("Currency".into());
        let mut metadata = ModelMetadata::default();
        let mut view = ModelView::default();
        revert_all(&changes, &mut current, &mut metadata, &mut view);
        let columns = &current.as_ref().expect("model").tables[0].columns;
        assert_eq!(columns[0].data_type, None, "the group's column type went");
        assert_eq!(columns[1].data_type.as_deref(), Some("Currency"), "the other group's type stayed");
    }

    /// A collection replaced wholesale reverts to its original entities, in their original order.
    #[test]
    fn a_replaced_collection_reverts_to_its_original_order() {
        let table = |name: &str| ModelTable { name: name.into(), columns: Vec::new(), rows: Vec::new() };

        let before = Some(DataModel { tables: vec![table("A"), table("B"), table("C")], ..DataModel::default() });
        let after = Some(DataModel { tables: vec![table("D")], ..DataModel::default() });
        let changes = model_changes(&before, after.as_ref());
        let mut current = after;
        revert_all(&changes, &mut current, &mut ModelMetadata::default(), &mut ModelView::default());
        assert_eq!(current, before, "the tables come back in their original order");

        // An entity added ahead of a surviving one must not shift the restored one.
        let before = Some(DataModel { tables: vec![table("A"), table("B")], ..DataModel::default() });
        let after = Some(DataModel { tables: vec![table("D"), table("A")], ..DataModel::default() });
        let changes = model_changes(&before, after.as_ref());
        let mut current = after;
        revert_all(&changes, &mut current, &mut ModelMetadata::default(), &mut ModelView::default());
        assert_eq!(current, before, "the removal is restored behind the survivor");
    }

    /// A view edit reverts that table's sort and filters only: a toggle another edit set stays.
    #[test]
    fn a_view_edit_reverts_its_own_table_only() {
        let mut before = ModelView::default();
        before.tables.insert("sales".into(), TableView::default());
        let mut after = before.clone();
        after.tables.get_mut("sales").expect("table").sort =
            Some(crate::cmd::dax::TableSort { column: "Amount".into(), by_table: None, by_column: None, descending: true });
        let changes = view_changes(&before, &after);
        assert_eq!(changes.len(), 1, "one table's view changed");

        // Another edit turned on a toggle since: the table's state comes back, the toggle stays.
        let mut current = after;
        current.show_hidden = true;
        let mut model = None;
        let mut metadata = ModelMetadata::default();
        revert_all(&changes, &mut model, &mut metadata, &mut current);
        assert_eq!(current.tables, before.tables, "the sort went");
        assert!(current.show_hidden, "the other edit's toggle stayed");
    }
    /// A reorder of surviving entities is recorded even when value changes exist: a load that
    /// reorders the tables while adding a measure rolls back to the original order.
    #[test]
    fn a_reorder_of_surviving_tables_rolls_back_with_value_changes() {
        let table = |name: &str| ModelTable { name: name.into(), columns: Vec::new(), rows: Vec::new() };
        let before = Some(DataModel { tables: vec![table("A"), table("B")], measures: vec![measure("M", "1")], ..DataModel::default() });
        let after =
            Some(DataModel { tables: vec![table("B"), table("A")], measures: vec![measure("M", "1"), measure("X", "2")], ..DataModel::default() });
        let changes = model_changes(&before, after.as_ref());
        let mut current = after;
        revert_all(&changes, &mut current, &mut ModelMetadata::default(), &mut ModelView::default());
        assert_eq!(current, before, "the reorder rolls back with the added measure");
    }

    /// Each display toggle reverts on its own: rolling back one group's toggle keeps the other
    /// group's toggle.
    #[test]
    fn each_display_toggle_reverts_on_its_own() {
        let before = ModelView::default();
        let mut after = before.clone();
        after.show_hidden = true;
        let changes = view_changes(&before, &after);
        assert_eq!(changes.len(), 1, "only the toggled toggle changed");

        // Another group enabled the other toggle since: it survives the rollback.
        let mut current = after;
        current.show_implicit_measures = true;
        let mut model = None;
        let mut metadata = ModelMetadata::default();
        revert_all(&changes, &mut model, &mut metadata, &mut current);
        assert!(!current.show_hidden, "the group's toggle went");
        assert!(current.show_implicit_measures, "the other group's toggle stayed");
    }
}
