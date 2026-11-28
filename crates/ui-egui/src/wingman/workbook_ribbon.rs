//! The agent's ribbon is independent of the visible UI. Only the active tab has callable
//! schemas. A reviewed plan can compose controls from tabs visited in the current turn.
use super::{Tool, context, field, lookup, object, validate, write_args};
use crate::SheetApp;
use gridcraft_engine::{DocState, EngineError};
use serde_json::{Value as Json, json};
use std::{borrow::Cow, collections::BTreeSet};

pub(in crate::wingman) const SECTIONS: &[&str] = &[
    "Workbook",
    "File",
    "Home",
    "Insert",
    "Draw",
    "Page Layout",
    "Formulas",
    "Data",
    "Review",
    "View",
    "Automate",
    "Developer",
    "Table Design",
    "Chart Design",
    "Format",
    "PivotTable Analyze",
    "PivotTable Design",
    "External",
];

pub(in crate::wingman) fn tab(name: &str) -> Option<&'static str> {
    Some(match name {
        "inspect_shapes" | "add_ink_stroke" | "ink_to_shape" => "Draw",
        "create_shape" => "Insert",
        "shape_fill" | "shape_outline" | "edit_shape_text" => "Format",
        "inspect_table"
        | "set_table_style"
        | "table_banded_rows"
        | "table_banded_columns"
        | "table_first_column"
        | "table_last_column"
        | "table_filter_buttons"
        | "table_totals"
        | "table_total_function" => "Table Design",
        "inspect_rules" | "set_validation" | "clear_validation" | "remove_duplicates" | "group_outline" | "ungroup_outline" | "preview_goal_seek" => {
            "Data"
        }
        "insert_rows" | "delete_rows" | "insert_columns" | "delete_columns" | "conditional_format" | "clear_format_rules" => "Home",
        "clear_filter" | "reapply_filter" | "inspect_pivots" => "Data",
        "inspect_layout" | "set_page_layout" => "Page Layout",
        "freeze_panes" | "unfreeze_panes" => "View",
        "create_pivot" => "Insert",
        "set_pivot_layout" => "PivotTable Design",
        "change_tab" | "apply_edits" | "workbook_info" | "find_commands" | "grep" | "inspect_cell" | "read_range" | "select_range"
        | "inspect_visual" => "Workbook",
        "set_cell" | "set_values" | "format_range" | "set_borders" | "inspect_format" | "number_format" | "merge_cells" | "unmerge_cells"
        | "set_fill" | "set_ink" | "fill_down" | "fill_right" | "drag_fill" | "clear_contents" | "transfer_range" | "size_rows" | "size_columns"
        | "data_bars" | "add_sheet" | "rename_sheet" => "Home",
        "create_chart" | "recommend_charts" | "create_table" | "list_objects" => "Insert",
        "sort_rows" | "enable_filter" | "filter_column" | "reshape_delimited" | "profile_range" => "Data",
        "find_functions" | "inspect_formula" | "check_formula" | "formula_diagnostics" | "define_range" => "Formulas",
        "set_comment" | "set_note" | "reply_comment" | "resolve_comment" | "comments_to_column" => "Review",
        "list_scripts" | "read_script" | "save_script" | "read_macro_draft" | "edit_macro_draft" => "Automate",
        "inspect_chart" | "update_chart" => "Chart Design",
        "rename_table" => "Table Design",
        _ => return None,
    })
}
fn also_on(name: &str, section: &str) -> bool {
    matches!(
        (section, name),
        ("Insert", "inspect_chart" | "update_chart")
            | ("Chart Design", "recommend_charts" | "create_chart" | "list_objects")
            | ("Home", "inspect_formula" | "inspect_rules" | "create_table" | "set_table_style")
            | ("View", "inspect_layout")
            | ("Insert" | "PivotTable Design" | "PivotTable Analyze", "inspect_pivots")
            | ("Insert" | "Format", "inspect_shapes")
            | ("Draw", "create_shape")
            | ("Insert", "inspect_table")
            | ("Developer", "read_macro_draft" | "edit_macro_draft" | "list_scripts" | "read_script" | "save_script")
            | ("PivotTable Analyze", "set_pivot_layout" | "create_pivot")
            | ("Formulas", "set_cell" | "drag_fill" | "profile_range")
            | ("Review" | "Table Design", "list_objects")
            | ("Table Design", "create_table" | "filter_column")
            | ("Format", "inspect_format" | "format_range" | "set_fill" | "set_ink" | "set_borders")
    )
}
pub(in crate::wingman) struct Ribbon {
    active: &'static str,
    group: Option<&'static str>,
    group_tools: Option<BTreeSet<&'static str>>,
    issued: BTreeSet<String>,
}
impl Default for Ribbon {
    fn default() -> Self {
        Self { active: "Workbook", group: None, group_tools: None, issued: BTreeSet::new() }
    }
}
pub(super) fn switch_schema() -> Json {
    object(json!({"tab":field("tab"),"group":field("group")}), &["tab"])
}
impl Ribbon {
    pub(in crate::wingman) fn location(&self) -> (&'static str, Option<&'static str>) {
        (self.active, self.group)
    }
    pub(in crate::wingman) fn includes(&self, name: &str) -> bool {
        if tab(name) == Some("Workbook") {
            return true;
        }
        if let Some(tools) = &self.group_tools {
            return tools.contains(name);
        }
        tab(name) == Some(self.active) || also_on(name, self.active)
    }
    pub(in crate::wingman) fn external(&self) -> bool {
        self.active == "External"
    }
    pub(in crate::wingman) fn switch(&mut self, name: &str) -> Result<(), String> {
        self.active = SECTIONS.iter().copied().find(|v| v.eq_ignore_ascii_case(name)).ok_or("Unknown ribbon tab.")?;
        self.group_tools = None;
        self.group = None;
        Ok(())
    }
    pub(in crate::wingman) fn issued(&mut self, names: &BTreeSet<String>) {
        self.issued.extend(names.iter().cloned());
    }
    fn seen(&self, name: &str) -> bool {
        self.issued.contains(name)
    }
    pub(in crate::wingman) fn open(&mut self, args: &Json, mode: crate::wingman::Mode, editor: bool) -> Result<Json, String> {
        validate(&switch_schema(), args, "change_tab")?;
        let name = args["tab"].as_str().ok_or("Specify tab.")?;
        if name == "External" && mode != crate::wingman::Mode::Mcp {
            return Err("External tools require Read / write / MCP access.".into());
        }
        let groups = super::menu::groups(name)?;
        let group = args["group"]
            .as_str()
            .map(|key| {
                groups.keys().copied().find(|g| g.eq_ignore_ascii_case(key)).ok_or("Unknown group; open the tab without a group to list its groups.")
            })
            .transpose()?;
        self.switch(name)?;
        self.group_tools = group.map(|g| super::menu::group_tools(name, g)).transpose()?;
        self.group = group;
        let controls = super::schemas(mode.writes(), self, editor)
            .into_iter()
            .filter_map(|v| v["function"]["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        Ok(
            json!({"activeTab":self.active,"activeGroup":group,"groups":groups,"workbookControls":controls,"menu":"Use find_commands with section, optional group and search to page individual buttons. Next request carries these schemas."}),
        )
    }
}

pub(super) fn resolve_selection<'a>(d: &DocState, args: &'a Json) -> Result<Cow<'a, Json>, String> {
    let mut out = Cow::Borrowed(args);
    for k in ["range", "source", "target", "cell", "at"] {
        if args[k] != "@selection" {
            continue;
        }
        let dest;
        let target = if k == "at" && args["destinationSheet"].is_string() {
            dest = json!({"sheet":args["destinationSheet"]});
            &dest
        } else {
            args
        };
        let sheet = context::sheet(d, target)?;
        if !d.wb.active().is_some_and(|s| s.uid == sheet.uid) || d.selection.ranges.len() != 1 {
            return Err("@selection needs one rectangle on the active target sheet.".into());
        }
        let r = d.selection.current();
        if matches!(k, "cell" | "at") && !r.is_single() {
            return Err("Use an explicit address for a single-cell target.".into());
        }
        out.to_mut()[k] = json!(r.a1());
    }
    Ok(out)
}

// Exclude scripts, external calls, structural sheet changes and nested plans.
fn batchable(t: &Tool) -> bool {
    matches!(
        t.name,
        "select_range"
            | "number_format"
            | "format_range"
            | "set_fill"
            | "set_ink"
            | "set_borders"
            | "merge_cells"
            | "unmerge_cells"
            | "set_cell"
            | "set_values"
            | "clear_contents"
            | "fill_right"
            | "fill_down"
            | "drag_fill"
            | "transfer_range"
            | "reshape_delimited"
            | "size_rows"
            | "size_columns"
            | "sort_rows"
            | "enable_filter"
            | "filter_column"
            | "data_bars"
            | "define_range"
            | "rename_table"
            | "create_table"
            | "create_chart"
            | "update_chart"
            | "set_comment"
            | "set_note"
            | "reply_comment"
            | "resolve_comment"
            | "comments_to_column"
            | "clear_filter"
            | "reapply_filter"
            | "conditional_format"
            | "clear_format_rules"
            | "freeze_panes"
            | "unfreeze_panes"
            | "set_page_layout"
            | "set_pivot_layout"
            | "shape_fill"
            | "shape_outline"
            | "edit_shape_text"
            | "create_shape"
            | "add_ink_stroke"
            | "set_validation"
            | "clear_validation"
            | "set_table_style"
            | "table_first_column"
            | "table_last_column"
            | "table_banded_rows"
            | "table_banded_columns"
            | "table_filter_buttons"
            | "table_totals"
            | "table_total_function"
    )
}
pub(in crate::wingman) fn apply(app: &mut SheetApp, ribbon: &Ribbon, args: &Json) -> Result<Json, String> {
    if args.to_string().len() > super::MAX_ARGS {
        return Err("Split the plan; at most 64 KiB per approval.".into());
    }
    validate(&object(json!({"actions":field("actions")}), &["actions"]), args, "apply_edits")?;
    let steps = args["actions"].as_array().ok_or("Specify actions.")?;
    let mut plan = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        let t = lookup(step["tool"].as_str().ok_or("Invalid tool name")?)?;
        if !batchable(t) || !ribbon.seen(t.name) {
            return Err(format!("Step {}: {} is not an opened, batchable workbook control.", i + 1, t.name));
        }
        validate(&t.params(), &step["args"], t.name)?;
        let id = t.command.ok_or("Expected a workbook edit.")?;
        plan.push((t, id, &step["args"]));
    }
    let (results, undo_label) = super::super::history::transaction(app, "Wingman: Edit plan", |s| {
        let mut results = Vec::new();
        for (i, (t, id, args)) in plan.iter().enumerate() {
            let mut run = || -> Result<Json, String> {
                if *id != "selection.set" && !gridcraft_engine::find_command(id).is_some_and(|c| c.undoable) {
                    return Err("Not an undoable command.".into());
                }
                // Bind relative targets against the preceding step's actual state.
                let p = write_args(s.active().ok_or("Workbook closed.")?, t, args)?;
                s.run(id, p)
            };
            let v = run().map_err(|e| EngineError::Other(format!("Step {} ({}): {e}. Plan rolled back.", i + 1, t.name)))?;
            let result = if v.to_string().len() <= 1024 { v } else { json!({"detailsOmitted":true}) };
            results.push(json!({"step":i+1,"tool":t.name,"result":result}));
        }
        Ok(json!(results))
    })?;
    Ok(json!({"applied":true,"steps":results,"undoLabel":undo_label}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Services, SheetApp};
    use gridcraft_engine::Session;

    fn issue(r: &mut Ribbon, write: bool) {
        let names = super::super::schemas(write, r, false).into_iter().map(|s| s["function"]["name"].as_str().unwrap().to_owned()).collect();
        r.issued(&names);
    }
    #[test]
    fn plan_requires_issued_schemas_and_rebases_selection_between_native_steps() {
        let mut session = Session::new();
        session.new_workbook();
        let mut app = SheetApp::new(session, Services::default());
        let sheet = app.session.active().unwrap().wb.active().unwrap().name.clone();
        let mut ribbon = Ribbon::default();
        let plan = json!({"actions":[
            {"tool":"number_format","args":{"sheet":sheet,"range":"B2:C4","format":"Currency"}},
            {"tool":"select_range","args":{"sheet":sheet,"range":"D2"}},
            {"tool":"set_cell","args":{"sheet":sheet,"cell":"@selection","input":"=B2+$C2"}},
            {"tool":"drag_fill","args":{"sheet":sheet,"source":"@selection","target":"D2:D4"}}]});
        ribbon.switch("Home").unwrap();
        issue(&mut ribbon, false);
        assert!(apply(&mut app, &ribbon, &plan).is_err());
        issue(&mut ribbon, true);
        assert!(
            apply(&mut app, &ribbon, &json!({"actions":[{"tool":"drag_fill","args":{"sheet":sheet,"source":"D2","target":"D2:E10"}}]}))
                .unwrap_err()
                .contains("one axis")
        );
        ribbon.switch("Data").unwrap();
        assert!(!ribbon.includes("drag_fill"));
        assert_eq!(apply(&mut app, &ribbon, &plan).unwrap()["applied"], true);
        let cell = |app: &mut SheetApp| super::super::execute(app, "inspect_cell", &json!({"sheet":sheet,"cell":"D4"}), false).unwrap();
        assert_eq!(cell(&mut app)["formula"], "=B4+$C4");
        app.run("edit.undo", json!({})).unwrap();
        assert!(cell(&mut app)["formula"].is_null());
        let selection = app.session.active().unwrap().selection.a1();
        let bad = json!({"actions":[{"tool":"select_range","args":{"sheet":sheet,"range":"A3"}},
            {"tool":"set_cell","args":{"sheet":sheet,"cell":"@selection","input":"temporary"}},
            {"tool":"drag_fill","args":{"sheet":sheet,"source":"Z10","target":"B2:B10"}}]});
        assert!(apply(&mut app, &ribbon, &bad).unwrap_err().contains("rolled back"));
        assert_eq!(app.session.active().unwrap().selection.a1(), selection);
        assert!(super::super::execute(&mut app, "inspect_cell", &json!({"sheet":sheet,"cell":"A3"}), false).unwrap()["value"].is_null());
        app.run("edit.redo", json!({})).unwrap();
        assert_eq!(cell(&mut app)["formula"], "=B4+$C4");
    }
    #[test]
    fn every_registered_tool_is_discoverable_and_has_a_typed_schema() {
        for t in super::super::tools() {
            let name = tab(t.name).unwrap_or_else(|| panic!("missing ribbon tab: {}", t.name));
            let mut ribbon = Ribbon::default();
            ribbon.switch(name).unwrap();
            assert!(ribbon.includes(t.name));
            assert_eq!(t.params()["additionalProperties"], false);
        }
    }
}
