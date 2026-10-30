//! The data-model PivotTable Fields pane: Excel's field list for a PivotTable backed by the
//! workbook data model, driven by `dataModel.pivotFields` and applied with `dataModel.createPivot`.
//!
//! It shows on the right when the active cell is inside a model PivotTable (or the user reopened
//! it from its tab), groups the model's tables with their columns and measures, and keeps the four
//! areas — Filters, Columns, Rows, Σ Values — as *pending* placements until Apply writes the
//! report. Pending placements are pane state (nothing is written before Apply), and every
//! placement is a `Table[Column]` reference or a measure name, exactly what the command takes.
//!
//! `dataModel.pivotFields` reads the package's pivot parts back, so its answer is cached per
//! (document uid, revision, model shape) and per (document uid, revision, model shape, pivot name),
//! like `app.grid.pivot_fields` keeps the worksheet pane's field list.

use egui::{Color32, Sense, Ui, vec2};
use serde_json::{Value as Json, json};

use crate::SheetApp;
use crate::i18n;
use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// The pane's panel id.
const PANE: &str = "model_pivot_pane";
/// The strip that reopens a pane the user closed.
const TAB: &str = "model_pivot_fields_tab";
/// Values one filter may keep: `dataModel.createPivot` accepts 64 per filter.
const MAX_FILTER_VALUES: usize = 64;

/// (document uid, revision, model shape): a command answer goes stale when any of them changes.
type CacheKey = (u64, u64, (usize, usize));
/// [`CacheKey`] plus the PivotTable the answer describes.
type NamedKey = (u64, u64, (usize, usize), String);

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
    measure: bool,
}

/// One table and the fields it owns, as the checklist groups them.
struct TableFields {
    name: String,
    columns: Vec<Field>,
    measures: Vec<Field>,
}

/// The report the pane is building, before Apply writes it.
#[derive(Clone, PartialEq)]
struct Draft {
    rows: Vec<String>,
    columns: Vec<String>,
    /// `[{field, allowed}]`, as `dataModel.createPivot` takes them.
    filters: Vec<Json>,
    values: Vec<String>,
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
        Self {
            rows: list("row_fields"),
            columns: list("column_fields"),
            filters,
            values: list("values"),
            layout: selected["layout"].as_str().filter(|layout| !layout.is_empty()).unwrap_or("compact").to_string(),
            grand_totals: selected["grand_totals"].as_bool().unwrap_or(false),
        }
    }

    fn contains(&self, reference: &str) -> bool {
        self.rows.iter().chain(&self.columns).chain(&self.values).any(|placed| placed == reference)
            || self.filters.iter().any(|filter| filter["field"].as_str() == Some(reference))
    }

    /// The references one area holds, in the order Excel shows them.
    fn entries(&self, area: Area) -> Vec<String> {
        match area {
            Area::Rows => self.rows.clone(),
            Area::Columns => self.columns.clone(),
            Area::Values => self.values.clone(),
            Area::Filters => self.filters.iter().filter_map(|filter| filter["field"].as_str()).map(str::to_string).collect(),
        }
    }

    /// Ticking a field: text and date columns go to Rows, numeric columns and measures to Values,
    /// which is what Excel's PivotTable Fields list does.
    fn add(&mut self, field: &Field) {
        if self.contains(&field.reference) {
            return;
        }
        if field.measure || field.numeric {
            self.values.push(field.reference.clone());
        } else {
            self.rows.push(field.reference.clone());
        }
    }

    fn remove(&mut self, reference: &str) {
        for area in [&mut self.rows, &mut self.columns, &mut self.values] {
            area.retain(|placed| placed != reference);
        }
        self.filters.retain(|filter| filter["field"].as_str() != Some(reference));
    }

    /// Moves a placed field to another area (Filters is handled by the caller, which also needs
    /// the field's members for the filter's `allowed` list).
    fn move_to(&mut self, reference: &str, area: Area) {
        self.remove(reference);
        match area {
            Area::Rows => self.rows.push(reference.to_string()),
            Area::Columns => self.columns.push(reference.to_string()),
            Area::Values => self.values.push(reference.to_string()),
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
    draft: Option<Draft>,
    /// The last command error, shown in the pane (no toast).
    error: Option<String>,
    /// The user reopened the pane from its tab.
    opened: bool,
    /// The user closed the pane with its ×.
    closed: bool,
    /// A filter field's members, for the filter's `allowed` list.
    members: Option<(NamedKey, Vec<Json>)>,
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
    let Some((uid, revision)) = app.session.active().map(|doc| (doc.uid, doc.revision)) else { return };
    let fingerprint = app.session.dax_model.as_ref().map(|model| (model.tables.len(), model.measures.len()));
    let mut state = load(ui.ctx());
    if fingerprint.is_none() {
        // Without a model there is nothing to read, and no model PivotTable to describe.
        state.all = None;
        state.one = None;
    }
    let key = (uid, revision, fingerprint.unwrap_or_default());
    // Reading the field list serializes the workbook and reads its parts back, so it runs only
    // when it can tell the person something: the workbook carries model PivotTables (their parts
    // are in the package) or the pane was opened to build one.
    let kept = has_kept_pivots(app);
    let answer = if fingerprint.is_some() && (state.opened || (!state.closed && kept)) { read_all(app, &mut state, key) } else { None };
    if let Some(answer) = answer {
        let info = answer.as_ref().ok();
        let pivots: Vec<Json> = info.and_then(|info| info["pivots"].as_array()).cloned().unwrap_or_default();
        let model = info.and_then(|info| info.get("model")).cloned().unwrap_or(Json::Null);
        let fields = catalog(&model);
        let active = active_pivot(app, &pivots);
        let name = active
            .as_ref()
            .and_then(|pivot| pivot["name"].as_str())
            .map(str::to_string)
            .or_else(|| state.bound.clone().filter(|bound| pivots.iter().any(|pivot| pivot["name"].as_str() == Some(bound))))
            .or_else(|| pivots.first().and_then(|pivot| pivot["name"].as_str()).map(str::to_string));
        if state.bound != name {
            // Another report: its own fields, a fresh error, and the pane follows it again.
            state.bound = name.clone();
            state.draft = None;
            state.error = None;
            state.members = None;
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
    } else if fingerprint.is_some() || kept {
        // Nothing read: the closed pane's strip is all there is to paint.
        reopen_tab(ui, &mut state);
    }
    store(ui.ctx(), &state);
}

/// Whether the workbook's package carries PivotTable parts: the only place a model PivotTable lives.
fn has_kept_pivots(app: &SheetApp) -> bool {
    app.session.active().and_then(|doc| doc.wb.preserved.as_ref()).is_some_and(|preserved| {
        preserved.parts.iter().any(|part| {
            let name = part.name.to_ascii_lowercase();
            name.starts_with("xl/pivottables/") && name.ends_with(".xml")
        })
    })
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
    let named = (key.0, key.1, key.2, name.to_string());
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

/// The model PivotTable the active cell is in: same sheet, at or below its report's top-left
/// cell, the nearest one when several qualify.
fn active_pivot(app: &SheetApp, pivots: &[Json]) -> Option<Json> {
    let doc = app.session.active()?;
    let sheet = doc.wb.active()?;
    let cell = doc.selection.active;
    let mut found: Option<(u32, u32, Json)> = None;
    for pivot in pivots {
        if pivot["sheet"].as_str() != Some(sheet.name.as_str()) {
            continue;
        }
        let Some(at) = pivot["at"].as_str().and_then(gridcraft_engine::core::RangeRef::parse).map(|range| range.start) else {
            continue;
        };
        if cell.row < at.row || cell.col < at.col {
            continue;
        }
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
                Some(Field {
                    reference: format!("{name}[{caption}]"),
                    caption: caption.to_string(),
                    numeric: numeric_type(column["type"].as_str().or_else(|| column["data_type"].as_str()).unwrap_or("")),
                    measure: false,
                })
            })
            .collect();
        tables.push(TableFields { name: name.to_string(), columns, measures: Vec::new() });
    }
    for measure in model["measures"].as_array().into_iter().flatten() {
        let Some(caption) = measure["name"].as_str() else { continue };
        let table = measure["table"].as_str().unwrap_or("");
        let field = Field { reference: caption.to_string(), caption: caption.to_string(), numeric: true, measure: true };
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

/// The short caption of a placement: the column of `Table[Column]`, or a measure's name.
fn caption_of(reference: &str) -> &str {
    reference.split_once('[').map(|(_, column)| column.strip_suffix(']').unwrap_or(column)).unwrap_or(reference)
}

/// Whether the field list knows `reference` as a measure (a measure may only sit in Σ Values).
fn is_measure(reference: &str, fields: &[TableFields]) -> bool {
    fields.iter().flat_map(|table| &table.measures).any(|field| field.reference == reference)
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

    egui::Panel::right(PANE).default_size(300.0).resizable(true).frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(10)).show(ui, |ui| {
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
                changed |= checklist(ui, &lang, view.fields, &mut draft);
                ui.separator();
                ui.label(egui::RichText::new(lang.tr("Use a field's menu to move it between areas:")).small().color(t.text_dim));
                let outcome = areas(ui, &t, &lang, &mut draft, view.fields);
                changed |= outcome.changed;
                place_filter = outcome.place_filter;
                ui.separator();
                changed |= options(ui, &lang, &mut draft);
            }
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
        match members(app, state, key, &reference) {
            Ok(allowed) => {
                draft.remove(&reference);
                draft.filters.push(json!({"field": reference, "allowed": allowed}));
                changed = true;
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

/// One field's check box: ticking places the field the way Excel does.
fn tick(ui: &mut Ui, field: &Field, icon: Icon, draft: &mut Draft) -> bool {
    let t = Tokens::get(ui.ctx());
    let mut on = draft.contains(&field.reference);
    let mut changed = false;
    ui.horizontal(|ui| {
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
    changed
}

/// What the area lists asked for.
struct Outcome {
    changed: bool,
    /// A field moved to Filters: its members fill the filter's `allowed` list.
    place_filter: Option<String>,
}

/// The four area lists: each placed field has the existing pane's move/remove menu.
fn areas(ui: &mut Ui, t: &Tokens, lang: &i18n::Language, draft: &mut Draft, fields: &[TableFields]) -> Outcome {
    let mut outcome = Outcome { changed: false, place_filter: None };
    ui.columns(2, |columns| {
        for (index, area) in Area::ALL.into_iter().enumerate() {
            let ui = &mut columns[index % 2];
            let entries = draft.entries(area);
            let title = if matches!(area, Area::Values) && entries.len() > 1 { "Σ Values" } else { area.title() };
            ui.label(egui::RichText::new(lang.tr(title)).strong());
            egui::Frame::NONE.fill(t.window).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                ui.set_min_size(vec2(ui.available_width(), 54.0));
                for reference in entries {
                    let caption = caption_of(&reference);
                    let button = egui::Button::new(egui::RichText::new(format!("{caption} ▾")).font(theme::ui_font(12.0)))
                        .fill(t.ribbon)
                        .min_size(vec2(ui.available_width(), 20.0));
                    let placed = ui.add(button);
                    let placed = if matches!(area, Area::Values) && !is_measure(&reference, fields) {
                        // A column needs a measure to summarize it; the command says so, and the
                        // chip should not have to be applied to find out.
                        placed.on_hover_text(lang.tr("This column needs a measure to summarize it."))
                    } else {
                        placed
                    };
                    egui::Popup::menu(&placed).show(|ui| {
                        for target in Area::ALL {
                            if target == area || !can_move(&reference, area, target, fields) {
                                continue;
                            }
                            if ui.button(lang.tr(target.moving())).clicked() {
                                if matches!(target, Area::Filters) {
                                    outcome.place_filter = Some(reference.clone());
                                } else {
                                    draft.move_to(&reference, target);
                                    outcome.changed = true;
                                }
                            }
                        }
                        ui.separator();
                        if ui.button(lang.tr("Remove Field")).clicked() {
                            draft.remove(&reference);
                            outcome.changed = true;
                        }
                    });
                }
            });
        }
    });
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

/// The Apply call: the pane's placements in `dataModel.createPivot`'s vocabulary.
fn apply_params(view: &View<'_>, draft: &Draft) -> Json {
    let measure = draft
        .values
        .first()
        .cloned()
        .or_else(|| view.selected.and_then(|selected| selected["measure"].as_str().or(selected["source"].as_str())).map(str::to_string))
        .or_else(|| {
            view.model["measures"].as_array().and_then(|measures| measures.first()).and_then(|measure| measure["name"].as_str()).map(str::to_string)
        })
        .unwrap_or_default();
    let mut params = json!({
        "measure": measure,
        "values": draft.values,
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

/// A filter field's members, for its `allowed` list: `VALUES(Table[Column])` over the model.
fn members(app: &mut SheetApp, state: &mut State, key: CacheKey, reference: &str) -> Result<Vec<Json>, String> {
    let named = (key.0, key.1, key.2, reference.to_string());
    if let Some((cached, values)) = &state.members
        && *cached == named
    {
        return Ok(values.clone());
    }
    let answer = app.session.run("dataModel.evaluate", json!({"formula": format!("VALUES({reference})"), "table": true}))?;
    let mut values: Vec<Json> = answer["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row.as_array().and_then(|values| values.first()).cloned())
        .filter(|value| !value.is_null())
        .collect();
    if values.is_empty() {
        return Err(format!("`{reference}` has no values to filter on."));
    }
    if values.len() > MAX_FILTER_VALUES {
        values.truncate(MAX_FILTER_VALUES);
    }
    state.members = Some((named, values.clone()));
    Ok(values)
}
