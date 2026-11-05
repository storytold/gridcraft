//! The data-model PivotTable Fields pane: Excel's field list for a PivotTable backed by the
//! workbook data model, driven by `dataModel.pivotFields` and applied with `dataModel.createPivot`.
//!
//! It shows on the right when the active cell is inside a model PivotTable (or the user reopened
//! it from its tab), groups the model's tables with their columns and measures, and keeps the four
//! areas — Filters, Columns, Rows, Σ Values — as *pending* placements until Apply writes the
//! report. Pending placements are pane state (nothing is written before Apply): an axis and a
//! filter take a `Table[Column]` reference, and Σ Values takes a measure name or a column plus the
//! aggregation that summarizes it — `Sum of Amount`, Excel's implicit measure, which the command
//! evaluates without adding a measure to the model.
//!
//! `dataModel.pivotFields` reads the model's metadata and the package's kept pivot parts, so its
//! answer is cached per (document uid, model contents, kept-pivot parts) and per that key plus the
//! pivot name — not per document revision: an ordinary cell edit changes neither input, so the
//! cached answer survives it (and the parts are not parsed again). The field list's `VALUES()`
//! members are model data, so their cache does take the revision.

use egui::{Color32, Sense, Ui, vec2};
use gridcraft_dax::DataModel;
use gridcraft_engine::model::Workbook;
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::dnd;
use crate::i18n;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// The pane's panel id.
const PANE: &str = "model_pivot_pane";
/// The strip that reopens a pane the user closed.
const TAB: &str = "model_pivot_fields_tab";
/// The pane's bottom strip: Apply and Refresh stay visible while the field list scrolls.
const ACTIONS: &str = "model_pivot_actions";
/// Characters the field search box keeps: a pasted novel must not be scanned on every frame.
const MAX_SEARCH_CHARS: usize = 80;
/// The aggregations `dataModel.createPivot` accepts for a column in Σ Values, in Excel's order.
const AGGREGATES: [&str; 5] = ["Sum", "Average", "Count", "Min", "Max"];

/// What `dataModel.pivotFields` reads: the open workbook, the model's contents and the kept
/// PivotTable parts the command parses. Deliberately not the document revision: a cell edit
/// changes none of them, so the cached answer stays valid and the parts are not read again.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CacheKey {
    /// The open workbook (a different one is a different answer).
    uid: u64,
    /// [`model_hash`]: the model's contents as the answer reports them.
    model: u64,
    /// [`kept_pivot_fingerprint`]: the kept PivotTable parts it is read from.
    parts: u64,
}

/// [`CacheKey`] plus the PivotTable the `selected` answer describes.
type NamedKey = (CacheKey, String);

/// A filter field's `VALUES(Table[Column])` members read model data, which a cell edit can change,
/// so they also take the document revision: (uid, revision, model hash, field reference).
type MemberKey = (u64, u64, u64, String);

/// Where a field sits in the report.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Area {
    Filters,
    Columns,
    Rows,
    Values,
}

impl Area {
    /// In the order Excel lists them: report filters first, Σ Values last.
    const ALL: [Self; 4] = [Self::Filters, Self::Columns, Self::Rows, Self::Values];

    fn title(self) -> &'static str {
        match self {
            Self::Filters => "Filters",
            Self::Columns => "Columns",
            Self::Rows => "Rows",
            Self::Values => "Values",
        }
    }

    /// The menu action that puts a field in this area.
    fn moving(self) -> &'static str {
        match self {
            Self::Filters => "Move to Report Filter",
            Self::Columns => "Move to Column Labels",
            Self::Rows => "Move to Row Labels",
            Self::Values => "Move to Values",
        }
    }
}

/// One entry of the field list.
#[derive(Clone)]
struct Field {
    /// `Table[Column]` for a column; the measure's name for a measure.
    reference: String,
    /// What the checklist shows: the column or measure name.
    caption: String,
    numeric: bool,
    /// The column's type holds dates: a date field has no timeline here (see [`slicer_card`]).
    date: bool,
    measure: bool,
}

/// One table and the fields it owns, as the checklist groups them.
#[derive(Clone)]
struct TableFields {
    name: String,
    columns: Vec<Field>,
    measures: Vec<Field>,
}

/// One entry of Σ Values: a model measure by name, or a column the command summarizes as Excel's
/// implicit measure (`Sum of Amount`).
#[derive(Clone, PartialEq)]
struct ValueField {
    /// `Table[Column]` for a summarized column; the measure's name for a measure.
    reference: String,
    /// The aggregation a column is summarized by; `None` for a measure.
    aggregate: Option<&'static str>,
}

impl ValueField {
    /// A measure's entry.
    fn measure(reference: impl Into<String>) -> Self {
        Self { reference: reference.into(), aggregate: None }
    }

    /// A field's entry in Σ Values: a measure by name, a numeric column summed and a text or date
    /// column counted — Excel's default summarization.
    fn for_field(field: &Field) -> Self {
        let aggregate = if field.measure {
            None
        } else if field.numeric {
            Some("Sum")
        } else {
            Some("Count")
        };
        Self { reference: field.reference.clone(), aggregate }
    }

    /// A `values` entry as `dataModel.createPivot` takes it.
    fn json(&self) -> Json {
        match self.aggregate {
            Some(aggregate) => json!({"column": self.reference, "aggregate": aggregate}),
            None => Json::String(self.reference.clone()),
        }
    }

    /// The chip's caption: `Sum of Amount` for a summarized column, the measure's name otherwise.
    fn caption(&self) -> String {
        match self.aggregate {
            Some(aggregate) => format!("{aggregate} of {}", caption_of(&self.reference)),
            None => self.reference.clone(),
        }
    }

    /// The measure name this entry contributes, for the pivot's `measure` parameter: none for a
    /// column, whose implicit measure the command names itself.
    fn measure_name(&self) -> Option<&str> {
        self.aggregate.is_none().then_some(self.reference.as_str())
    }
}

/// A `values` entry read back from `dataModel.pivotFields`' `selected` (or kept in a draft): a
/// measure name, or `{column, aggregate}` when the engine ever reports a summarized column.
fn value_from_json(item: &Json) -> Option<ValueField> {
    if let Some(name) = item.as_str() {
        return Some(ValueField::measure(name));
    }
    let reference = item["column"].as_str()?;
    let aggregate = item["aggregate"].as_str().and_then(|name| AGGREGATES.iter().copied().find(|known| known.eq_ignore_ascii_case(name)));
    Some(ValueField { reference: reference.to_string(), aggregate })
}

/// A field being dragged: out of the tree (`from` is `None`) or out of one area's list, where it
/// also remembers its place so a drop inside the same area reorders instead of appending.
#[derive(Clone)]
struct FieldDrag {
    reference: String,
    caption: String,
    from: Option<(Area, usize)>,
}

/// The report the pane is building, before Apply writes it.
#[derive(Clone, PartialEq)]
struct Draft {
    rows: Vec<String>,
    columns: Vec<String>,
    /// `[{field, allowed}]`, as `dataModel.createPivot` takes them.
    filters: Vec<Json>,
    /// A measure name or a summarized column, as `dataModel.createPivot` takes them.
    values: Vec<ValueField>,
    layout: String,
    grand_totals: bool,
}

impl Default for Draft {
    fn default() -> Self {
        Self { rows: Vec::new(), columns: Vec::new(), filters: Vec::new(), values: Vec::new(), layout: "compact".into(), grand_totals: false }
    }
}

impl Draft {
    /// The placements a saved PivotTable already has.
    fn from_selected(selected: &Json) -> Self {
        let list = |key: &str| -> Vec<String> {
            selected[key].as_array().map(|items| items.iter().filter_map(|item| item.as_str()).map(str::to_string).collect()).unwrap_or_default()
        };
        let filters = selected["filters"]
            .as_array()
            .map(|items| items.iter().filter(|item| item["field"].as_str().is_some()).cloned().collect())
            .unwrap_or_default();
        let values = selected["values"].as_array().map(|items| items.iter().filter_map(value_from_json).collect()).unwrap_or_default();
        Self {
            rows: list("row_fields"),
            columns: list("column_fields"),
            filters,
            values,
            layout: selected["layout"].as_str().filter(|layout| !layout.is_empty()).unwrap_or("compact").to_string(),
            grand_totals: selected["grand_totals"].as_bool().unwrap_or(false),
        }
    }

    fn contains(&self, reference: &str) -> bool {
        self.rows.iter().chain(&self.columns).chain(self.values.iter().map(|value| &value.reference)).any(|placed| placed == reference)
            || self.filters.iter().any(|filter| filter["field"].as_str() == Some(reference))
    }

    /// The references one area holds, in the order Excel shows them.
    fn entries(&self, area: Area) -> Vec<String> {
        match area {
            Area::Rows => self.rows.clone(),
            Area::Columns => self.columns.clone(),
            Area::Values => self.values.iter().map(|value| value.reference.clone()).collect(),
            Area::Filters => self.filters.iter().filter_map(|filter| filter["field"].as_str()).map(str::to_string).collect(),
        }
    }

    /// The Σ Values entry for `reference`, when it is one.
    fn value(&self, reference: &str) -> Option<&ValueField> {
        self.values.iter().find(|value| value.reference == reference)
    }

    /// The Σ Values entry for `reference`, for the chip menu's aggregation change.
    fn value_mut(&mut self, reference: &str) -> Option<&mut ValueField> {
        self.values.iter_mut().find(|value| value.reference == reference)
    }

    /// Ticking a field: text and date columns go to Rows, numeric columns and measures to Values,
    /// which is what Excel's PivotTable Fields list does.
    fn add(&mut self, field: &Field) {
        if self.contains(&field.reference) {
            return;
        }
        if field.measure || field.numeric {
            self.values.push(ValueField::for_field(field));
        } else {
            self.rows.push(field.reference.clone());
        }
    }

    fn remove(&mut self, reference: &str) {
        self.rows.retain(|placed| placed != reference);
        self.columns.retain(|placed| placed != reference);
        self.values.retain(|placed| placed.reference != reference);
        self.filters.retain(|filter| filter["field"].as_str() != Some(reference));
    }

    /// Moves a placed field to another area (Filters is handled by the caller, which adds the
    /// page field's `(All)` filter). A field entering Σ Values keeps its
    /// kind: a measure stays a measure, a model column is summarized (`Sum`, or `Count` for text).
    fn move_to(&mut self, reference: &str, area: Area, fields: &[TableFields]) {
        self.remove(reference);
        match area {
            Area::Rows => self.rows.push(reference.to_string()),
            Area::Columns => self.columns.push(reference.to_string()),
            Area::Values => {
                let entry = fields
                    .iter()
                    .flat_map(|table| table.columns.iter().chain(&table.measures))
                    .find(|field| field.reference == reference)
                    .map_or_else(|| ValueField::measure(reference), ValueField::for_field);
                self.values.push(entry);
            }
            Area::Filters => {}
        }
    }
}

/// The pane's own state: the cached command answers and the report being built.
#[derive(Clone, Default)]
struct State {
    /// `dataModel.pivotFields` with no name: the model and every model PivotTable.
    all: Option<(CacheKey, Result<Json, String>)>,
    /// `dataModel.pivotFields {name}`: the pivot's `selected` placements.
    one: Option<(NamedKey, Result<Json, String>)>,
    /// The PivotTable the draft belongs to; a different report starts from its own fields.
    bound: Option<String>,
    /// The PivotTable this pane wrote last. It keeps the pane on that report for the frame after
    /// Apply, when the selection has not moved into it yet; opening the pane outside every report
    /// does not use it, so it starts a new report instead of rewriting the last one.
    written: Option<String>,
    draft: Option<Draft>,
    /// The last command error, shown in the pane (no toast).
    error: Option<String>,
    /// The user reopened the pane from its tab.
    opened: bool,
    /// The user closed the pane with its ×.
    closed: bool,
    /// A filter field's members, for the filter's `allowed` list.
    members: Option<(MemberKey, Vec<Json>)>,
    /// The kept PivotTable parts' fingerprint, memoized by (uid, revision): the field list's key
    /// needs it only when it reads, and hashing the package is not something a repaint may repeat.
    parts: Option<(u64, u64, u64)>,
    /// The slicers the person added from a filter field: the sheet's slicer object binds to a
    /// worksheet table's column, so a data-model field's slicer is drawn here and drives the
    /// PivotTable's page filter through Apply.
    slicers: Vec<SlicerCard>,
    /// The field search box's query; empty shows the whole tree.
    search: String,
}

/// One slicer the pane draws: a page filter's members as value tiles.
#[derive(Clone)]
struct SlicerCard {
    /// The filter field, `Table[Column]`.
    field: String,
    /// What the card's header shows: the column name.
    caption: String,
    /// Every member of the field, from `VALUES(Table[Column])`: the tiles.
    values: Vec<Json>,
}

impl State {
    fn cached_all(&self, key: CacheKey) -> Option<Result<Json, String>> {
        self.all.as_ref().filter(|(cached, _)| *cached == key).map(|(_, answer)| answer.clone())
    }

    fn cached_one(&self, key: &NamedKey) -> Option<Result<Json, String>> {
        self.one.as_ref().filter(|(cached, _)| cached == key).map(|(_, answer)| answer.clone())
    }
}

/// The cached answers and the model's field list, as one value for the drawing code.
struct View<'a> {
    name: Option<&'a str>,
    selected: Option<&'a Json>,
    model: &'a Json,
    fields: &'a [TableFields],
    /// What `dataModel.pivotFields` said when it could not read the model.
    read_error: Option<&'a str>,
    /// Where the report goes:
    sheet: &'a str,
    at: &'a str,
}

/// Draws the pane when it should show, and its reopen tab when the user closed it.
pub fn show(app: &mut SheetApp, ui: &mut Ui) {
    // One task pane at a time: a worksheet PivotTable's own field list, a task pane or the
    // comments pane already owns the right-hand edge.
    if crate::pivot_pane::active_pivot(app).is_some() || app.grid.pane.is_some() || app.session.comments_pane {
        return;
    }
    let Some(uid) = app.session.active().map(|doc| doc.uid) else { return };
    // Model edits are session state and do not bump the document revision, so the model is part
    // of the key by its contents, not by a revision number.
    let Some(model) = app.session.dax_model.as_ref().map(model_hash) else {
        // Without a model there is nothing to read, and no model PivotTable to describe.
        let mut state = load(ui.ctx());
        state.all = None;
        state.one = None;
        state.members = None;
        store(ui.ctx(), &state);
        return;
    };
    let mut state = load(ui.ctx());
    // The field list reads the model's metadata and the package's kept pivot parts; it runs only
    // when it can tell the person something: the workbook carries model PivotTables (their parts
    // are in the package) or the pane was opened to build one. The part fingerprint is therefore
    // only computed for a read — hashing every kept definition on each repaint of a closed pane
    // would parse the package for nothing.
    let kept = has_kept_pivots(app);
    if !state.opened && (state.closed || !kept) {
        // Nothing read: the closed pane's strip is all there is to paint.
        reopen_tab(ui, &mut state);
        store(ui.ctx(), &state);
        return;
    }
    let key = field_list_key(app, &mut state, uid, model);
    let answer = read_all(app, &mut state, key);
    if let Some(answer) = answer {
        let info = answer.as_ref().ok();
        let pivots: Vec<Json> = info.and_then(|info| info["pivots"].as_array()).cloned().unwrap_or_default();
        let model = info.and_then(|info| info.get("model")).cloned().unwrap_or(Json::Null);
        let fields = catalog(&model);
        let active = active_pivot(app, &pivots);
        if active.is_some() {
            // Inside a report the active cell alone says which one: a leftover `written` must not
            // outlive the selection and later rewrite that report from outside it.
            state.written = None;
        }
        // Bound to the active report, or to the one this pane just wrote; outside every report the
        // pane starts a new one (a `pivots.first()` fallback would silently rewrite that report).
        let name = active
            .as_ref()
            .and_then(|pivot| pivot["name"].as_str())
            .map(str::to_string)
            .or_else(|| state.written.clone().filter(|written| pivots.iter().any(|pivot| pivot["name"].as_str() == Some(written))));
        if state.bound != name {
            // Another report: its own fields, a fresh error, and the pane follows it again.
            state.bound = name.clone();
            state.draft = None;
            state.error = None;
            state.members = None;
            state.slicers.clear();
            state.closed = false;
            state.opened = false;
        }
        let selected = named_selected(app, &mut state, key, name.as_deref());
        let draft = state.draft.clone().unwrap_or_else(|| selected.as_ref().map_or_else(Draft::default, Draft::from_selected));
        let (sheet, at) = destination(app, selected.as_ref());
        let wanted = state.opened || (!state.closed && active.is_some());
        if wanted {
            let view = View {
                name: name.as_deref(),
                selected: selected.as_ref(),
                model: &model,
                fields: &fields,
                read_error: answer.as_ref().err().map(String::as_str),
                sheet: &sheet,
                at: &at,
            };
            draw(app, ui, &mut state, &draft, &view, key);
        } else {
            reopen_tab(ui, &mut state);
        }
    } else {
        // Nothing read: the closed pane's strip is all there is to paint.
        reopen_tab(ui, &mut state);
    }
    store(ui.ctx(), &state);
}

/// Whether the workbook's package carries PivotTable parts: the only place a model PivotTable lives.
fn has_kept_pivots(app: &SheetApp) -> bool {
    app.session
        .active()
        .and_then(|doc| doc.wb.preserved.as_ref())
        .is_some_and(|preserved| preserved.parts.iter().any(|part| is_pivot_part(&part.name)))
}

/// Whether `name` is a kept PivotTable definition part (`xl/pivotTables/*.xml`).
fn is_pivot_part(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("xl/pivottables/") && name.ends_with(".xml")
}

/// The field list's [`CacheKey`], hashing the kept PivotTable parts at most once per document
/// revision: [`show`] only calls this for a read, and the fingerprint is reused for every repaint
/// until an edit moves the revision (the parts and the model cannot change without one).
fn field_list_key(app: &mut SheetApp, state: &mut State, uid: u64, model: u64) -> CacheKey {
    let revision = app.session.active().map_or(0, |doc| doc.revision);
    let parts = match state.parts {
        Some((cached_uid, cached_revision, fingerprint)) if cached_uid == uid && cached_revision == revision => fingerprint,
        _ => {
            let fingerprint = app.session.active().map_or(0, |doc| kept_pivot_fingerprint(&doc.wb));
            state.parts = Some((uid, revision, fingerprint));
            fingerprint
        }
    };
    CacheKey { uid, model, parts }
}

/// The model's contents as `dataModel.pivotFields` reports them in `model`: table and column names
/// and types, measure names, home tables and formats, and relationships. Model edits are session
/// state and do not bump the document revision, so the field list's key hashes them instead.
fn model_hash(model: &DataModel) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for table in &model.tables {
        table.name.hash(&mut hasher);
        for column in &table.columns {
            column.name.hash(&mut hasher);
            column.data_type.hash(&mut hasher);
        }
    }
    for measure in &model.measures {
        measure.name.hash(&mut hasher);
        measure.table.hash(&mut hasher);
        measure.format.hash(&mut hasher);
    }
    for relationship in &model.relationships {
        relationship.from_table.hash(&mut hasher);
        relationship.from_column.hash(&mut hasher);
        relationship.to_table.hash(&mut hasher);
        relationship.to_column.hash(&mut hasher);
    }
    hasher.finish()
}

/// The kept PivotTable parts `dataModel.pivotFields` parses, by name and contents: every
/// definition under `xl/pivotTables/`, the cache definition each one names, and the sheet names
/// and relationships that say which sheet owns it. The cache's record part is not read by the
/// command, so it is not hashed (it can be the largest part in the package).
fn kept_pivot_fingerprint(wb: &Workbook) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // `part.sheet` in the answer comes from the sheet's relationship to the definition part.
    for sheet in &wb.sheets {
        sheet.name.hash(&mut hasher);
        for rel in sheet.preserved.as_deref().map_or(&[][..], |preserved| preserved.rels.as_slice()) {
            rel.rel_type.hash(&mut hasher);
            rel.target.trim_start_matches('/').to_ascii_lowercase().hash(&mut hasher);
            rel.external.hash(&mut hasher);
        }
    }
    let Some(preserved) = wb.preserved.as_deref() else { return hasher.finish() };
    for part in preserved.parts.iter().filter(|part| is_pivot_part(&part.name)) {
        part.name.to_ascii_lowercase().hash(&mut hasher);
        part.data.hash(&mut hasher);
        for rel in &part.rels {
            rel.rel_type.hash(&mut hasher);
            rel.target.hash(&mut hasher);
            rel.external.hash(&mut hasher);
        }
        // The fields, hierarchies and measures the answer reports come from this part.
        let cache = part
            .rels
            .iter()
            .find(|rel| !rel.external && rel.rel_type.ends_with("/pivotCacheDefinition"))
            .and_then(|rel| preserved.parts.iter().find(|kept| kept.name.eq_ignore_ascii_case(rel.target.trim_start_matches('/'))));
        if let Some(cache) = cache {
            cache.data.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// The `dataModel.pivotFields` answer for the workbook, read once per [`CacheKey`].
fn read_all(app: &mut SheetApp, state: &mut State, key: CacheKey) -> Option<Result<Json, String>> {
    if let Some(answer) = state.cached_all(key) {
        return Some(answer);
    }
    // Nothing to read without a data model.
    app.session.dax_model.as_ref()?;
    let answer = app.session.run("dataModel.pivotFields", json!({})).map_err(|error| crate::clean_error(&error));
    state.all = Some((key, answer.clone()));
    Some(answer)
}

/// The named answer's `selected`: the pivot's fields, read once per [`NamedKey`].
fn named_selected(app: &mut SheetApp, state: &mut State, key: CacheKey, name: Option<&str>) -> Option<Json> {
    let Some(name) = name else {
        state.one = None;
        return None;
    };
    let named = (key, name.to_string());
    let answer = match state.cached_one(&named) {
        Some(answer) => answer,
        None => {
            let answer = app.session.run("dataModel.pivotFields", json!({"name": name})).map_err(|error| crate::clean_error(&error));
            state.one = Some((named, answer.clone()));
            answer
        }
    };
    answer.ok().and_then(|answer| answer.get("selected").cloned()).filter(|selected| !selected.is_null())
}

/// The model PivotTable the active cell is in: same sheet, inside the report's full range (its
/// page-field rows included), the nearest one when several qualify.
fn active_pivot(app: &SheetApp, pivots: &[Json]) -> Option<Json> {
    let doc = app.session.active()?;
    let sheet = doc.wb.active()?;
    let cell = doc.selection.active;
    let mut found: Option<(u32, u32, Json)> = None;
    for pivot in pivots {
        if pivot["sheet"].as_str() != Some(sheet.name.as_str()) {
            continue;
        }
        let Some(area) = pivot["range"].as_str().or(pivot["at"].as_str()).and_then(gridcraft_engine::core::RangeRef::parse) else {
            continue;
        };
        if !area.contains(cell) {
            continue;
        }
        let at = area.start;
        if found.as_ref().is_none_or(|(row, col, _)| (at.row, at.col) > (*row, *col)) {
            found = Some((at.row, at.col, pivot.clone()));
        }
    }
    found.map(|(_, _, pivot)| pivot)
}

/// Where Apply writes: the selected pivot's own place, or the active cell for a new report.
fn destination(app: &SheetApp, selected: Option<&Json>) -> (String, String) {
    if let Some(selected) = selected
        && let (Some(sheet), Some(at)) = (selected["sheet"].as_str(), selected["at"].as_str())
    {
        // The read-back `at` is the report body, below the page fields the filters write (two rows
        // each); the command takes the PivotTable's top-left, so Apply must not drift down.
        let pages = selected["filters"].as_array().map_or(0, Vec::len).saturating_mul(2);
        let top = gridcraft_engine::core::RangeRef::parse(at)
            .map(|range| range.start)
            .map(|cell| gridcraft_engine::core::CellRef::new(cell.row.saturating_sub(pages as u32), cell.col));
        return (sheet.to_string(), top.map_or_else(|| at.to_string(), |cell| cell.a1()));
    }
    match app.session.active().and_then(|doc| doc.wb.active().map(|sheet| (sheet.name.clone(), doc.selection.active))) {
        Some((sheet, cell)) => (sheet, cell.a1()),
        None => (String::new(), "A3".to_string()),
    }
}

/// The model's tables with their columns and measures, as the checklist groups them.
fn catalog(model: &Json) -> Vec<TableFields> {
    let mut tables: Vec<TableFields> = Vec::new();
    for table in model["tables"].as_array().into_iter().flatten() {
        let Some(name) = table["name"].as_str() else { continue };
        let columns = table["columns"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|column| {
                let caption = column["name"].as_str()?;
                let kind = column["type"].as_str().or_else(|| column["data_type"].as_str()).unwrap_or("");
                Some(Field {
                    reference: format!("{name}[{caption}]"),
                    caption: caption.to_string(),
                    numeric: numeric_type(kind),
                    date: date_type(kind),
                    measure: false,
                })
            })
            .collect();
        tables.push(TableFields { name: name.to_string(), columns, measures: Vec::new() });
    }
    for measure in model["measures"].as_array().into_iter().flatten() {
        let Some(caption) = measure["name"].as_str() else { continue };
        let table = measure["table"].as_str().unwrap_or("");
        let field = Field { reference: caption.to_string(), caption: caption.to_string(), numeric: true, date: false, measure: true };
        match tables.iter_mut().find(|owner| owner.name == table) {
            Some(owner) => owner.measures.push(field),
            None => tables.push(TableFields { name: table.to_string(), columns: Vec::new(), measures: vec![field] }),
        }
    }
    tables
}

/// Whether a model column type is summed rather than grouped: Excel's numeric types.
fn numeric_type(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    ["int", "double", "decimal", "currency", "single", "float", "number", "numeric", "money", "real"].iter().any(|name| kind.contains(name))
}

/// Whether a model column type holds dates: the model's `DateTime` type, and the date/time type
/// names a Power Pivot model may declare. A date field has no timeline in this pane: a timeline
/// needs the field's date grouping, which a model PivotTable does not carry (`PivotFilter` holds
/// plain member values, not a date axis), so [`slicer_card`] reports that instead of a slicer.
fn date_type(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("date") || kind.contains("time")
}

/// The short caption of a placement: the column of `Table[Column]`, or a measure's name.
fn caption_of(reference: &str) -> &str {
    reference.split_once('[').map(|(_, column)| column.strip_suffix(']').unwrap_or(column)).unwrap_or(reference)
}

/// Whether `reference` is a measure (a measure may only sit in Σ Values): a bare name, never a
/// `Table[Column]` column. A value the workbook carries but the session model does not — the
/// implicit measure of an earlier Apply, `Sum of Amount` — is a measure too.
fn is_measure(reference: &str, fields: &[TableFields]) -> bool {
    !is_column(reference, fields)
}

/// Whether `reference` is a column (only columns can be report filters).
fn is_column(reference: &str, fields: &[TableFields]) -> bool {
    fields.iter().flat_map(|table| &table.columns).any(|field| field.reference == reference)
}

fn load(ctx: &egui::Context) -> State {
    ctx.data_mut(|data| data.get_temp::<State>(egui::Id::unique(PANE)).unwrap_or_default())
}

fn store(ctx: &egui::Context, state: &State) {
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(PANE), state.clone()));
}

/// The field search box's stable id, so a control request or a test can focus it.
pub fn search_id() -> egui::Id {
    egui::Id::unique(PANE).with("search")
}

/// Sets the field tree's search query from outside the frame — a control request or a test — so
/// the next frame filters the tree without anyone typing into the box.
pub fn set_search(ctx: &egui::Context, query: &str) {
    let mut state = load(ctx);
    state.search = query.chars().take(MAX_SEARCH_CHARS).collect();
    store(ctx, &state);
}

/// The query the field tree is filtered by; empty when the box is clear.
pub fn search(ctx: &egui::Context) -> String {
    load(ctx).search
}

/// One area's drop zone, for the control channel and tests (matching is case-insensitive).
pub fn area_rect(ctx: &egui::Context, title: &str) -> Option<egui::Rect> {
    let area = Area::ALL.into_iter().find(|area| area.title().eq_ignore_ascii_case(title))?;
    last_rect(ctx, area_id(area))
}

/// A tree field's row id: its drag handle covers exactly this id's rect.
fn field_id(reference: &str) -> egui::Id {
    egui::Id::unique(PANE).with(("field", reference))
}

/// One area's drop-zone id, and the key of its rect in the context's data.
fn area_id(area: Area) -> egui::Id {
    egui::Id::unique(PANE).with(("area", area.title()))
}

/// One placed chip's id: its drag handle sits under the chip's button.
fn chip_id(area: Area, reference: &str) -> egui::Id {
    egui::Id::unique(PANE).with(("chip", area.title(), reference))
}

/// Remembers where a widget was drawn this pass, so next pass's drag handle can cover it.
fn remember_rect(ctx: &egui::Context, id: egui::Id, rect: egui::Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| data.insert_temp(id, (pass, rect)));
}

/// The rect `id` was drawn at in this or the previous pass. A row or chip the search hides (or a
/// field the menu removed) stops being drawn, so its old rect stops answering: no invisible
/// handle can be grabbed where it used to be.
fn last_rect(ctx: &egui::Context, id: egui::Id) -> Option<egui::Rect> {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|data| data.get_temp::<(u64, egui::Rect)>(id)).filter(|(drawn, _)| drawn.saturating_add(1) >= pass).map(|(_, rect)| rect)
}

/// The tables the search keeps, case-insensitively: a table whose name matches keeps all its
/// children, otherwise only the children whose own name matches. An empty query keeps everything.
fn filtered(fields: &[TableFields], query: &str) -> Vec<TableFields> {
    let query = query.trim().to_lowercase();
    let matches = |caption: &str| caption.to_lowercase().contains(&query);
    let mut kept = Vec::new();
    for table in fields {
        let table_matches = matches(&table.name);
        let columns: Vec<Field> = table.columns.iter().filter(|field| table_matches || matches(&field.caption)).cloned().collect();
        let measures: Vec<Field> = table.measures.iter().filter(|field| table_matches || matches(&field.caption)).cloned().collect();
        if table_matches || !columns.is_empty() || !measures.is_empty() {
            kept.push(TableFields { name: table.name.clone(), columns, measures });
        }
    }
    kept
}

/// The strip that brings a closed pane back, as Excel's collapsed field list does.
fn reopen_tab(ui: &mut Ui, state: &mut State) {
    let t = Tokens::get(ui.ctx());
    let lang = i18n::current(ui.ctx());
    egui::Panel::right(TAB).exact_size(28.0).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(2)).show(ui, |ui| {
        let (rect, response) = ui.allocate_exact_size(vec2(24.0, 70.0), Sense::click());
        if response.is_pointer_button_down_on() {
            ui.painter().rect_filled(rect, 3.0, t.pressed);
        } else if response.hovered() {
            ui.painter().rect_filled(rect, 3.0, t.hover);
        }
        let glyph = egui::Rect::from_center_size(rect.center(), vec2(18.0, 18.0));
        icons::paint(ui.painter(), glyph, Icon::DataModel, t.text);
        let label = lang.tr("Show PivotTable Fields");
        let response = response.on_hover_text(label);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if response.clicked() {
            state.opened = true;
            state.closed = false;
            // The strip is the way back to the field list, not to the last report it wrote: a
            // reopened pane outside every report starts a new one.
            state.written = None;
            state.bound = None;
        }
    });
}

/// Draws the pane and runs what it was asked to do.
fn draw(app: &mut SheetApp, ui: &mut Ui, state: &mut State, draft: &Draft, view: &View<'_>, key: CacheKey) {
    let t = Tokens::get(ui.ctx());
    let lang = i18n::current(ui.ctx());
    let mut draft = draft.clone();
    let mut error = state.error.clone();
    let mut changed = false;
    let mut apply = false;
    let mut refresh = false;
    let mut close = false;
    let mut place_filter: Option<String> = None;
    let mut add_slicer: Option<String> = None;

    egui::Panel::right(PANE).default_size(300.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
        // The actions live in their own bottom strip, so a long field list scrolls above them
        // instead of pushing the report's one write out of the pane.
        egui::Panel::bottom(ACTIONS).frame(egui::Frame::NONE.fill(t.ribbon)).show(ui, |ui| {
            ui.separator();
            ui.horizontal(|ui| {
                let can_apply = !draft.rows.is_empty() || !draft.columns.is_empty() || !draft.values.is_empty();
                if ui.add_enabled(can_apply, egui::Button::new(lang.tr("Apply"))).clicked() {
                    apply = true;
                }
                if ui.button(lang.tr("Refresh")).clicked() {
                    refresh = true;
                }
            });
            if let Some(message) = &error {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(message).small().color(t.danger));
            }
        });
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(lang.tr("PivotTable Fields")).font(theme::ui_bold(15.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::widgets::icon_button(ui, Icon::Close, t.text_dim, lang.tr("Close"), vec2(22.0, 22.0)).clicked() {
                        close = true;
                    }
                });
            });
            ui.label(egui::RichText::new(lang.tr("Choose fields to add to report:")).small().color(t.text_dim));
            if let Some(message) = view.read_error {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(lang.tr("Load a data model to build a PivotTable from it.")).color(t.danger));
                ui.label(egui::RichText::new(message).small().color(t.text_dim));
            } else if view.fields.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(lang.tr("This data model has no tables to report.")).small().color(t.text_dim));
            } else {
                search_box(ui, &t, &lang, &mut state.search);
                // The empty query keeps the model's own list: no copy on the common frame.
                let narrowed;
                let fields: &[TableFields] = if state.search.trim().is_empty() {
                    view.fields
                } else {
                    narrowed = filtered(view.fields, &state.search);
                    &narrowed
                };
                if fields.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(lang.tr("No fields match")).small().color(t.text_dim));
                } else {
                    changed |= checklist(ui, &lang, fields, &mut draft);
                }
                ui.separator();
                ui.label(egui::RichText::new(lang.tr("Drag a field into an area, or use its menu, to place it:")).small().color(t.text_dim));
                let outcome = areas(ui, &t, &lang, &mut draft, view.fields);
                changed |= outcome.changed;
                place_filter = outcome.place_filter;
                add_slicer = outcome.add_slicer;
                ui.separator();
                changed |= options(ui, &lang, &mut draft);
                if !state.slicers.is_empty() {
                    ui.separator();
                    changed |= slicer_cards(ui, &t, &lang, &mut draft, &mut state.slicers);
                }
            }
        });
    });

    if close {
        state.closed = true;
        state.opened = false;
    }
    if refresh {
        state.all = None;
        state.one = None;
        state.members = None;
        error = None;
    }
    if let Some(reference) = place_filter {
        // A field moved to Filters starts as Excel's `(All)`: the page field names the column and
        // keeps every value. Sending the member list instead would drop the rows whose value is
        // blank or past the command's cap before the user has selected anything.
        draft.remove(&reference);
        draft.filters.push(json!({"field": reference}));
        changed = true;
    }
    if let Some(reference) = add_slicer {
        match slicer_card(app, state, key, &reference, view) {
            Ok(card) => {
                // Re-adding a field's slicer refreshes its tiles instead of stacking a second card.
                state.slicers.retain(|existing| existing.field != card.field);
                state.slicers.push(card);
            }
            Err(message) => error = Some(message),
        }
    }
    let mut written = false;
    if apply {
        match app.run("dataModel.createPivot", apply_params(view, &draft)) {
            Ok(reply) => {
                // The workbook carries the report now: the next frame reads it back.
                written = true;
                error = None;
                state.all = None;
                state.one = None;
                state.members = None;
                // The engine puts the selection on the report's top-left, which a filtered report
                // hides above its body: the pane stays up on the report it just wrote.
                state.opened = true;
                state.closed = false;
                if let Some(name) = reply["name"].as_str() {
                    state.bound = Some(name.to_string());
                    state.written = Some(name.to_string());
                }
            }
            Err(message) => error = Some(crate::clean_error(&message)),
        }
    }
    if written {
        state.draft = None;
    } else if changed {
        state.draft = Some(draft);
    }
    state.error = error;
}

/// The field search box above the checklist: typing narrows the tree to the matching tables and
/// fields, case-insensitively. It is a normal single-line text edit, so Tab reaches it.
fn search_box(ui: &mut Ui, t: &Tokens, lang: &i18n::Language, query: &mut String) {
    ui.horizontal(|ui| {
        glyph(ui, Icon::Search, t.text_dim);
        let width = (ui.available_width() - 4.0).max(80.0);
        let response = ui.add(
            egui::TextEdit::singleline(query).id(search_id()).hint_text(lang.tr("Search fields")).char_limit(MAX_SEARCH_CHARS).desired_width(width),
        );
        if response.changed() {
            // The tree below is filtered next frame; keyboard focus stays in the box.
            ui.ctx().request_repaint();
        }
    });
}

/// The field checklist, grouped by table: its columns, then the measures it owns.
fn checklist(ui: &mut Ui, lang: &i18n::Language, fields: &[TableFields], draft: &mut Draft) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut changed = false;
    egui::ScrollArea::vertical().max_height(220.0).auto_shrink([false, true]).id_salt("model_pv_fields").show(ui, |ui| {
        for table in fields {
            if !table.columns.is_empty() || !table.measures.is_empty() {
                ui.horizontal(|ui| {
                    glyph(ui, Icon::ModelTable, t.text_dim);
                    let name = if table.name.is_empty() { lang.tr("Measures") } else { table.name.as_str() };
                    ui.label(egui::RichText::new(name).strong().font(theme::ui_font(12.0)));
                });
            }
            for field in &table.columns {
                changed |= tick(ui, field, Icon::ModelColumn, draft);
            }
            for field in &table.measures {
                changed |= tick(ui, field, Icon::Measure, draft);
            }
        }
    });
    changed
}

/// One field's check box: ticking places the field the way Excel does, and dragging the row
/// carries the field to an area.
fn tick(ui: &mut Ui, field: &Field, icon: Icon, draft: &mut Draft) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut on = draft.contains(&field.reference);
    let mut changed = false;
    let handle = field_id(&field.reference);
    let row = ui.horizontal(|ui| {
        // The drag handle covers the row drawn last pass, under the check box. It senses clicks
        // too, so egui waits for real movement before it starts a drag and a click still ticks.
        if let Some(rect) = last_rect(ui.ctx(), handle) {
            let drag = ui.interact(rect, handle, Sense::click_and_drag());
            dnd::drag_source(ui, &drag, FieldDrag { reference: field.reference.clone(), caption: field.caption.clone(), from: None });
        }
        ui.add_space(12.0);
        glyph(ui, icon, t.text_dim);
        let response = ui.checkbox(&mut on, egui::RichText::new(&field.caption).font(theme::ui_font(13.0)));
        if response.changed() {
            if on {
                draft.add(field);
            } else {
                draft.remove(&field.reference);
            }
            changed = true;
        }
    });
    remember_rect(ui.ctx(), handle, row.response.rect);
    changed
}

/// What the area lists asked for.
struct Outcome {
    changed: bool,
    /// A field moved to Filters: it becomes the page field's `(All)` filter.
    place_filter: Option<String>,
    /// A filter field asked for a slicer: the pane adds a card that drives its page filter.
    add_slicer: Option<String>,
}

/// What a completed drop asked the caller to do.
enum DropAction {
    /// The area refuses the field, or it was already there.
    Nothing,
    /// The draft changed.
    Moved,
    /// The field moved to Filters: the caller adds the `(All)` filter outside the frame.
    Filter(String),
}

/// The four area lists: each placed field has the existing pane's move/remove menu, and fields
/// can also be dragged from the tree into an area or between areas.
fn areas(ui: &mut Ui, t: &Tokens, lang: &i18n::Language, draft: &mut Draft, fields: &[TableFields]) -> Outcome {
    let drag = egui::DragAndDrop::payload::<FieldDrag>(ui.ctx());
    let pointer = ui.ctx().pointer_latest_pos();
    let released = ui.input(|input| input.pointer.any_released());
    let mut outcome = Outcome { changed: false, place_filter: None, add_slicer: None };
    // The area under the pointer, and the gap the field would land in. Remembered even when the
    // area refuses it: releasing over an area must never also count as "dropped outside".
    let mut hovered: Option<(Area, usize)> = None;
    ui.columns(2, |columns| {
        for (index, area) in Area::ALL.into_iter().enumerate() {
            let ui = &mut columns[index % 2];
            let entries = draft.entries(area);
            let title = if matches!(area, Area::Values) && entries.len() > 1 { "Σ Values" } else { area.title() };
            ui.label(egui::RichText::new(lang.tr(title)).strong());
            let mut chips: Vec<egui::Rect> = Vec::new();
            let frame = egui::Frame::NONE.fill(t.window).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_size(vec2(ui.available_width(), 54.0));
                for (at, reference) in entries.iter().enumerate() {
                    // A summarized column shows the aggregation the command applies
                    // (`Sum of Amount`); a measure shows its own name.
                    let caption = draft.value(reference).map_or_else(|| caption_of(reference).to_string(), ValueField::caption);
                    let id = chip_id(area, reference);
                    if let Some(rect) = last_rect(ui.ctx(), id) {
                        // The handle covers the chip drawn last pass, under its button. It senses
                        // clicks too, so a click still opens the menu and only movement drags it.
                        let handle = ui.interact(rect, id, Sense::click_and_drag());
                        dnd::drag_source(ui, &handle, FieldDrag { reference: reference.clone(), caption: caption.clone(), from: Some((area, at)) });
                    }
                    let button = egui::Button::new(egui::RichText::new(format!("{caption} ▾")).font(theme::ui_font(12.0)))
                        .fill(t.ribbon)
                        .min_size(vec2(ui.available_width(), 20.0));
                    let placed = ui.add(button);
                    chips.push(placed.rect);
                    remember_rect(ui.ctx(), id, placed.rect);
                    // A summarized column offers Excel's Summarize Values By on its own menu.
                    let aggregate = draft.value(reference).and_then(|value| value.aggregate);
                    egui::Popup::menu(&placed).show(|ui| {
                        if let Some(current) = aggregate {
                            for candidate in AGGREGATES {
                                let title = format!("{} {candidate}", lang.tr("Summarize by"));
                                if ui.selectable_label(current == candidate, title).clicked() && current != candidate {
                                    if let Some(value) = draft.value_mut(reference) {
                                        value.aggregate = Some(candidate);
                                    }
                                    outcome.changed = true;
                                }
                            }
                            ui.separator();
                        }
                        for target in Area::ALL {
                            if target == area || !can_move(reference, area, target, fields) {
                                continue;
                            }
                            if ui.button(lang.tr(target.moving())).clicked() {
                                if matches!(target, Area::Filters) {
                                    outcome.place_filter = Some(reference.clone());
                                } else {
                                    draft.move_to(reference, target, fields);
                                    outcome.changed = true;
                                }
                            }
                        }
                        if matches!(area, Area::Filters) && ui.button(lang.tr("Add as Slicer")).clicked() {
                            outcome.add_slicer = Some(reference.clone());
                        }
                        ui.separator();
                        if ui.button(lang.tr("Remove Field")).clicked() {
                            draft.remove(reference);
                            outcome.changed = true;
                        }
                    });
                }
            });
            let zone = frame.response.rect;
            remember_rect(ui.ctx(), area_id(area), zone);
            if let (Some(carry), Some(at)) = (&drag, pointer)
                && zone.contains(at)
            {
                if accepts(carry, area, fields) {
                    dnd::drop_marker(ui, zone, &chips, at.y);
                }
                hovered = Some((area, dnd::insert_index(&chips, at.y)));
            }
        }
    });
    if let (Some(carry), Some(at)) = (&drag, pointer) {
        dnd::ghost(ui, &carry.caption, at);
        if released {
            match hovered {
                Some((area, index)) if accepts(carry, area, fields) => match place(draft, carry, area, index, fields) {
                    DropAction::Nothing => {}
                    DropAction::Moved => outcome.changed = true,
                    DropAction::Filter(reference) => outcome.place_filter = Some(reference),
                },
                Some(_) => {}
                // Released away from every area: a placed field leaves the report, as dragging a
                // field back onto Excel's field list does. A tree field just returns.
                None if carry.from.is_some() => {
                    draft.remove(&carry.reference);
                    outcome.changed = true;
                }
                None => {}
            }
            if hovered.is_some() || carry.from.is_some() {
                let _ = egui::DragAndDrop::take_payload::<FieldDrag>(ui.ctx());
            }
        }
    }
    outcome
}

/// Whether a placed field may move to `target`: only columns sit on an axis or in a filter.
fn can_move(reference: &str, from: Area, target: Area, fields: &[TableFields]) -> bool {
    if matches!(target, Area::Filters) && !is_column(reference, fields) {
        return false;
    }
    if matches!(from, Area::Values) && is_measure(reference, fields) && !matches!(target, Area::Values) {
        return false;
    }
    true
}

/// Whether a dragged field may land in `area`: a measure only summarizes, a filter only takes a
/// column, and a placed measure moves only between Σ Values slots.
fn accepts(drag: &FieldDrag, area: Area, fields: &[TableFields]) -> bool {
    if is_measure(&drag.reference, fields) {
        return matches!(area, Area::Values);
    }
    if matches!(area, Area::Filters) {
        return is_column(&drag.reference, fields);
    }
    true
}

/// Applies a drop: a chip inside its own area takes the new place, and a field from another area
/// or from the tree moves over (appending when the gap is the end).
fn place(draft: &mut Draft, drag: &FieldDrag, area: Area, index: usize, fields: &[TableFields]) -> DropAction {
    match area {
        Area::Filters => {
            if draft.entries(Area::Filters).iter().any(|placed| placed == &drag.reference) {
                return DropAction::Nothing;
            }
            DropAction::Filter(drag.reference.clone())
        }
        Area::Rows | Area::Columns | Area::Values => {
            if matches!(drag.from, Some((from, _)) if from == area) {
                return if reorder(draft, area, &drag.reference, index) { DropAction::Moved } else { DropAction::Nothing };
            }
            draft.move_to(&drag.reference, area, fields);
            // `index` counts the target's chips as they were, without the newcomer.
            let _ = reorder(draft, area, &drag.reference, index);
            DropAction::Moved
        }
    }
}

/// Moves a placed field inside its own area to the gap before `index`, counted with the field
/// still in the list (so dropping it back where it was changes nothing).
fn reorder(draft: &mut Draft, area: Area, reference: &str, index: usize) -> bool {
    match area {
        Area::Rows => reorder_entry(&mut draft.rows, |placed| placed.as_str(), reference, index),
        Area::Columns => reorder_entry(&mut draft.columns, |placed| placed.as_str(), reference, index),
        Area::Values => reorder_entry(&mut draft.values, |value| value.reference.as_str(), reference, index),
        Area::Filters => false,
    }
}

/// [`reorder`] over one area's list: `key` names each entry's reference.
fn reorder_entry<T>(list: &mut Vec<T>, key: impl Fn(&T) -> &str, reference: &str, index: usize) -> bool {
    let Some(from) = list.iter().position(|entry| key(entry) == reference) else { return false };
    if index == from || index == from + 1 {
        return false;
    }
    let entry = list.remove(from);
    let to = if index > from { index - 1 } else { index }.min(list.len());
    list.insert(to, entry);
    true
}

/// Report layout and grand totals, which Apply carries over.
fn options(ui: &mut Ui, lang: &i18n::Language, draft: &mut Draft) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let selected = match draft.layout.as_str() {
            "outline" => lang.tr("Outline"),
            "tabular" => lang.tr("Tabular"),
            _ => lang.tr("Compact"),
        };
        let combo = craft_ui::ComboBox::from_id_salt("model_pivot_layout").selected_text(selected).show_ui(ui, |ui| {
            for (label, value) in [("Compact", "compact"), ("Outline", "outline"), ("Tabular", "tabular")] {
                if ui.selectable_label(draft.layout == value, lang.tr(label)).clicked() && draft.layout != value {
                    draft.layout = value.to_string();
                    changed = true;
                }
            }
        });
        combo.response.accessible_name(lang.tr("Report Layout"));
        if ui.checkbox(&mut draft.grand_totals, lang.tr("Grand Totals")).changed() {
            changed = true;
        }
    });
    changed
}

/// A small model glyph next to a label (the icon set's data-model vocabulary).
fn glyph(ui: &mut Ui, icon: Icon, color: Color32) {
    let (rect, _response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    icons::paint(ui.painter(), rect.shrink(1.0), icon, color);
}

/// The card for a filter field, or why a data-model field cannot have one.
///
/// A date field has no slicer: a timeline needs the field's date grouping — a date axis over the
/// members with a from/to range — and a model PivotTable's page filter carries plain member values
/// only (`PivotFilter { field, allowed, all }`), so the pane reports the limitation instead of offering
/// tiles that could not filter. Everything else (text, numbers) lists its `VALUES()` members.
fn slicer_card(app: &mut SheetApp, state: &mut State, key: CacheKey, reference: &str, view: &View<'_>) -> Result<SlicerCard, String> {
    let date = view.fields.iter().flat_map(|table| &table.columns).any(|field| field.reference == reference && field.date);
    if date {
        return Err(format!(
            "`{reference}` is a date field: a timeline needs the field's date grouping, which a model PivotTable does not carry, so it has no slicer here."
        ));
    }
    let values = members(app, state, key, reference)?;
    Ok(SlicerCard { field: reference.to_string(), caption: caption_of(reference).to_string(), values })
}

/// The slicer cards: each filter field's members as tiles, the selected ones marked.
///
/// A tile click edits the filter's `allowed` list in the pending draft, so Apply writes the
/// PivotTable's page filter with exactly the tiles that are on — the grid shrinks to those
/// members, and "Clear Slicer Filter" removes the list again, which is the page field's `(All)`:
/// every tile on, every row back.
fn slicer_cards(ui: &mut Ui, t: &Tokens, lang: &i18n::Language, draft: &mut Draft, cards: &mut Vec<SlicerCard>) -> bool {
    // A card whose filter left the draft has nothing to drive: drop it.
    cards.retain(|card| draft.filters.iter().any(|filter| filter["field"].as_str() == Some(card.field.as_str())));
    if cards.is_empty() {
        return false;
    }
    let mut changed = false;
    ui.label(egui::RichText::new(lang.tr("Slicers")).strong());
    ui.label(
        egui::RichText::new(lang.tr("A data-model field's slicer is drawn here: a sheet slicer object filters a table column."))
            .small()
            .color(t.text_dim),
    );
    let mut removed: Vec<String> = Vec::new();
    for card in cards.iter() {
        let field = card.field.clone();
        let caption = card.caption.clone();
        let values = card.values.clone();
        ui.add_space(4.0);
        egui::Frame::NONE.fill(t.window).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&caption).strong().font(theme::ui_font(12.0)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::widgets::icon_button(ui, Icon::Close, t.text_dim, lang.tr("Remove Slicer"), vec2(18.0, 18.0)).clicked() {
                        removed.push(field.clone());
                    }
                });
            });
            let Some(filter) = draft.filters.iter_mut().find(|filter| filter["field"].as_str() == Some(field.as_str())) else { return };
            // A filter with no `allowed` key is `(All)`: every member is on.
            let mut all_on = filter.get("allowed").is_none();
            let mut card_changed = false;
            let mut clear = false;
            let mut allowed: Vec<Json> = filter["allowed"].as_array().cloned().unwrap_or_else(|| values.clone());
            // Excel's slicer click: a plain click selects exactly that tile (the rest turn off) and
            // clicking the only selected tile clears the filter, while Ctrl/Cmd-click toggles one
            // tile at a time.
            let multi = ui.input(|input| input.modifiers.command || input.modifiers.ctrl);
            ui.horizontal_wrapped(|ui| {
                for value in &values {
                    let on = all_on || allowed.iter().any(|kept| kept == value);
                    if ui.selectable_label(on, egui::RichText::new(tile_text(value)).font(theme::ui_font(12.0))).clicked() {
                        all_on = false;
                        card_changed = true;
                        let only = allowed.len() == 1 && allowed.first() == Some(value);
                        if multi {
                            if on {
                                allowed.retain(|kept| kept != value);
                                // Deselecting the last member is `(All)`, like clicking it alone:
                                // an empty list keeps no rows and the writer rejects it.
                                if allowed.is_empty() {
                                    clear = true;
                                }
                            } else {
                                allowed.push(value.clone());
                            }
                        } else if only {
                            clear = true;
                        } else {
                            allowed = vec![value.clone()];
                        }
                        changed = true;
                    }
                }
            });
            // Only an explicit selection narrows the page field: clicking the last selected tile
            // clears the key again, which is `(All)` — the same state the Clear Slicer Filter
            // button writes, and one that picks up members added to the model later.
            if clear {
                if let Some(map) = filter.as_object_mut() {
                    map.remove("allowed");
                }
                changed = true;
            } else if card_changed {
                if let Some(map) = filter.as_object_mut() {
                    map.insert("allowed".to_string(), Json::Array(allowed));
                }
                changed = true;
            }
            if ui.button(lang.tr("Clear Slicer Filter")).clicked() {
                // Excel's Clear Slicer Filter restores `(All)` — every member, blanks included.
                if let Some(map) = filter.as_object_mut() {
                    map.remove("allowed");
                }
                changed = true;
            }
        });
    }
    cards.retain(|card| !removed.contains(&card.field));
    changed
}

/// A member as its tile shows it: text as it is, a blank as `(Blank)`, a number as JSON spells it.
fn tile_text(value: &Json) -> String {
    match value {
        Json::String(text) => text.clone(),
        Json::Null => "(Blank)".to_string(),
        other => other.to_string(),
    }
}

/// The Apply call: the pane's placements in `dataModel.createPivot`'s vocabulary.
fn apply_params(view: &View<'_>, draft: &Draft) -> Json {
    // The source measure only supplies the grouping table: a values area of summarized columns
    // lets the command take the first column's table (an empty `measure` is that request).
    // Falling back to the model's first measure would group from another table whenever the
    // values area holds only summarized columns.
    let measure = draft
        .values
        .iter()
        .find_map(ValueField::measure_name)
        .map(str::to_string)
        .or_else(|| view.selected.and_then(|selected| selected["measure"].as_str().or(selected["source"].as_str())).map(str::to_string))
        .or_else(|| {
            // A summarized column supplies the grouping table, so no measure is needed there; with
            // no value field at all the report still needs one to group by.
            if draft.values.is_empty() {
                view.model["measures"]
                    .as_array()
                    .and_then(|measures| measures.first())
                    .and_then(|measure| measure["name"].as_str())
                    .map(str::to_string)
            } else {
                None
            }
        })
        .unwrap_or_default();
    let mut params = json!({
        "measure": measure,
        "values": draft.values.iter().map(ValueField::json).collect::<Vec<Json>>(),
        "row_fields": draft.rows,
        "column_fields": draft.columns,
        "filters": draft.filters,
        "layout": draft.layout,
        "row_subtotals": view.selected.and_then(|selected| selected["row_subtotals"].as_bool()).unwrap_or(false),
        "column_subtotals": view.selected.and_then(|selected| selected["column_subtotals"].as_bool()).unwrap_or(false),
        "grand_totals": draft.grand_totals,
        // A new report must not overwrite cells that are not its own; replacing one reuses its area.
        "replace": view.name.is_some(),
        "sheet": view.sheet,
        "at": view.at,
    });
    if let Some(name) = view.name {
        params["name"] = json!(name);
    }
    params
}

/// `VALUES(...)` for a filter field. `reference` is the plain `Table[Column]` spelling
/// `createPivot` takes, so the parts are quoted (and `]` escaped) for DAX here.
fn values_formula(reference: &str) -> String {
    match reference.split_once('[').and_then(|(table, rest)| rest.strip_suffix(']').map(|column| (table, column))) {
        Some((table, column)) => format!("VALUES({}{})", gridcraft_dax::table_ref(table), gridcraft_dax::column_ref(column)),
        // A measure is a bare name: it is already the reference DAX wants.
        None => format!("VALUES({reference})"),
    }
}

/// A filter field's members, for its `allowed` list: `VALUES(Table[Column])` over the model.
fn members(app: &mut SheetApp, state: &mut State, key: CacheKey, reference: &str) -> Result<Vec<Json>, String> {
    // The members are model data (`DataModel::tables[..].rows`), which a cell edit can change: the
    // cache takes the revision as well as the model's contents.
    let revision = app.session.active().map_or(0, |doc| doc.revision);
    let named: MemberKey = (key.uid, revision, key.model, reference.to_string());
    if let Some((cached, values)) = &state.members
        && *cached == named
    {
        return Ok(values.clone());
    }
    let answer = app.session.run("dataModel.evaluate", json!({"formula": values_formula(reference), "table": true}))?;
    // Every member, blanks included: a blank is a member the user can tick, and dropping any of
    // them here would silently exclude those rows once a selection is written.
    let values: Vec<Json> =
        answer["rows"].as_array().into_iter().flatten().filter_map(|row| row.as_array().and_then(|values| values.first()).cloned()).collect();
    if values.is_empty() {
        return Err(format!("`{reference}` has no values to filter on."));
    }
    state.members = Some((named, values.clone()));
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_dax::value::Scalar;
    use gridcraft_dax::{Measure, ModelColumn, ModelTable, Relationship};
    use gridcraft_engine::core::{CellRef, Value};
    use gridcraft_engine::model::{Cell, Preserved, PreservedPart, PreservedRel};
    use std::sync::Arc;

    const PIVOT_TABLE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable";
    const PIVOT_CACHE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition";

    fn rel(rel_type: &str, target: &str) -> PreservedRel {
        PreservedRel { id: "rId1".to_string(), rel_type: rel_type.to_string(), target: target.to_string(), external: false }
    }

    fn part(name: &str, data: &str, rels: Vec<PreservedRel>) -> PreservedPart {
        PreservedPart { name: name.to_string(), content_type: String::new(), data: Arc::from(data.as_bytes()), rels }
    }

    /// A workbook with one model PivotTable: its sheet relates to the definition, which relates
    /// to its cache definition, and a model database part the command never reads.
    fn workbook() -> Workbook {
        let mut wb = Workbook::new();
        wb.sheet_mut(0).expect("Sheet1").preserved =
            Some(Arc::new(Preserved { rels: vec![rel(PIVOT_TABLE_REL, "xl/pivotTables/pivotTable1.xml")], ..Default::default() }));
        wb.preserved = Some(Arc::new(Preserved {
            parts: vec![
                part(
                    "xl/pivotTables/pivotTable1.xml",
                    r#"<pivotTableDefinition name="PivotTable1"><location ref="A3:B4"/></pivotTableDefinition>"#,
                    vec![rel(PIVOT_CACHE_REL, "xl/pivotCache/pivotCacheDefinition1.xml")],
                ),
                part("xl/pivotCache/pivotCacheDefinition1.xml", "<pivotCacheDefinition saveData=\"0\"/>", Vec::new()),
                part("xl/model/item.data", "<data/>", Vec::new()),
            ],
            ..Default::default()
        }));
        wb
    }

    fn part_mut<'a>(wb: &'a mut Workbook, name: &str) -> &'a mut PreservedPart {
        let preserved = Arc::make_mut(wb.preserved.as_mut().expect("the test workbook carries kept parts"));
        preserved.parts.iter_mut().find(|part| part.name == name).expect("the test workbook carries this part")
    }

    /// The cache key's inputs: a cell edit must reuse the field list, while the model PivotTable's
    /// definition, its cache definition, the sheet that owns it and the model metadata must not.
    #[test]
    fn field_list_key_changes_only_with_its_inputs() {
        let mut wb = workbook();
        let before = kept_pivot_fingerprint(&wb);

        // An ordinary cell edit changes neither the kept parts nor the model.
        wb.sheet_mut(0).expect("Sheet1").set_cell(CellRef::new(0, 0), Cell::value(Value::text("edited")));
        assert_eq!(before, kept_pivot_fingerprint(&wb), "a cell edit must reuse the cached field list");

        // Nor does a kept part the command never reads: the model database can be large.
        part_mut(&mut wb, "xl/model/item.data").data = Arc::from(&b"<data>edited</data>"[..]);
        assert_eq!(before, kept_pivot_fingerprint(&wb));

        // The PivotTable definition is what the answer reports: its part changed is a new answer.
        part_mut(&mut wb, "xl/pivotTables/pivotTable1.xml").data = Arc::from(&b"<pivotTableDefinition name=\"PivotTable1\"/>"[..]);
        let definition = kept_pivot_fingerprint(&wb);
        assert_ne!(before, definition);

        // Its cache definition carries the fields, hierarchies and measures.
        part_mut(&mut wb, "xl/pivotCache/pivotCacheDefinition1.xml").data = Arc::from(&b"<pivotCacheDefinition/>"[..]);
        let cache = kept_pivot_fingerprint(&wb);
        assert_ne!(definition, cache);

        // Which sheet owns the PivotTable is in the answer (`part.sheet`).
        wb.sheet_mut(0).expect("Sheet1").name = "Renamed".to_string();
        assert_ne!(cache, kept_pivot_fingerprint(&wb));
    }

    fn model() -> DataModel {
        DataModel {
            tables: vec![
                ModelTable {
                    name: "Sales".to_string(),
                    columns: vec![ModelColumn { name: "Amount".to_string(), data_type: Some("Double".to_string()) }],
                    rows: vec![vec![Scalar::Decimal(1.0)]],
                },
                ModelTable {
                    name: "Products".to_string(),
                    columns: vec![ModelColumn { name: "Name".to_string(), data_type: Some("String".to_string()) }],
                    rows: vec![vec![Scalar::String("Widget".to_string())]],
                },
            ],
            measures: vec![Measure {
                name: "Total".to_string(),
                formula: "SUM(Sales[Amount])".to_string(),
                table: Some("Sales".to_string()),
                format: None,
                expr: None,
            }],
            relationships: Vec::new(),
        }
    }

    /// The model side of the key: everything `dataModel.pivotFields` puts in `model`, and nothing
    /// the field list does not show (the rows behind `VALUES()` are the members' own key).
    #[test]
    fn model_hash_follows_the_field_list_metadata_not_the_rows() {
        let before = model_hash(&model());
        assert_eq!(before, model_hash(&model()), "an unchanged model reuses the cached field list");

        let mut data = model();
        data.tables[0].rows = vec![vec![Scalar::Decimal(2.0)], vec![Scalar::Decimal(3.0)]];
        assert_eq!(before, model_hash(&data), "model data does not change the field list");

        let mut formatted = model();
        formatted.measures[0].format = Some("#,##0.00".to_string());
        assert_ne!(before, model_hash(&formatted), "the measure's format is in the field list");

        let mut related = model();
        related.relationships.push(Relationship {
            from_table: "Sales".to_string(),
            from_column: "Amount".to_string(),
            to_table: "Products".to_string(),
            to_column: "Name".to_string(),
            bidirectional: false,
            inactive: false,
        });
        assert_ne!(before, model_hash(&related), "the relationships are in the field list");
    }
}
