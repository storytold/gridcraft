//! The control-flow header is editable independently of its nested lists. No API accepts
//! raw commands or replaces a branch as a side effect of changing a condition.
use crate::macro_editor;
use gridcraft_engine::{
    core::{MAX_COLS, MAX_ROWS},
    model::{CompareOp, RepeatOf, Step, StepKind},
};
use serde_json::{Map, Value, json};

fn fields(kind: &str) -> Value {
    let reference =
        json!({"type":"string","minLength":1,"maxLength":1024,"description":"An A1 reference or native macro token such as Input.col(2)."});
    match kind {
        "if" => json!({"cell":reference,"op":{"type":"string","enum":["eq","ne","gt","ge","lt","le","empty","notEmpty","contains"]},
            "value":{"type":["null","number","boolean","string"],"maxLength":4096}}),
        "repeat" => json!({"over":reference,"of":{"type":"string","enum":["rows","columns"]},"max":{"type":"integer","minimum":1,"maximum":100000}}),
        "stop" => json!({"message":{"type":"string","maxLength":4096},"rollback":{"type":"boolean"}}),
        _ => json!({"note":{"type":"string","maxLength":4096},"enabled":{"type":"boolean"},"breakpoint":{"type":"boolean"}}),
    }
}

fn object(props: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}

fn merge(a: &mut Value, b: Value) {
    if let (Some(a), Value::Object(b)) = (a.as_object_mut(), b) {
        a.extend(b);
    }
}

pub(super) fn step_schema() -> Value {
    let mut variants = Vec::new();
    for (kind, required) in
        [("set_cell", &[][..]), ("comments_to_column", &[]), ("command", &[]), ("if", &["cell", "op"]), ("repeat", &["over"]), ("stop", &[])]
    {
        let mut p = fields("flags");
        let (names, args) = if kind == "command" {
            (json!(["set_values", "format_range", "set_fill", "set_ink"]), json!({"type":"object"}))
        } else if kind == "comments_to_column" {
            (json!([kind]), super::workbook::comment_transfer_params())
        } else if kind == "set_cell" {
            let input = json!({"type":"string","maxLength":32767});
            let literal = object(
                json!({"sheet":{"type":"string"},"cell":{"type":"string","description":"Unqualified A1 cell address"},"input":input}),
                &["sheet", "cell", "input"],
            );
            let relative = object(
                json!({"cell":{"type":"string","pattern":r"^(Row\.cell\(1,[1-9][0-9]{0,4}\)|Column\.cell\([1-9][0-9]{0,6},1\))$",
                "description":"In the nearest Repeat only: Row.cell(1,n), n=1..16384 for rows; Column.cell(n,1), n=1..1048576 for columns. Omit sheet."},"input":input}),
                &["cell", "input"],
            );
            (json!(["set_cell"]), json!({"anyOf":[literal,relative]}))
        } else {
            (json!([kind]), object(fields(kind), required))
        };
        merge(&mut p, json!({"tool":{"type":"string","enum":names},"args":args}));
        variants.push(object(p, &["tool", "args"]));
    }
    json!({"anyOf":variants})
}

/// Mirror the native interpreter's innermost iteration, through either arm of an If.
/// list_at/split_path borrow existing steps without cloning a potentially large Repeat.
pub(super) fn enclosing_repeat(steps: &[Step], mut path: &str) -> Option<RepeatOf> {
    while !path.is_empty() {
        let owner = path.strip_suffix(".then").or_else(|| path.strip_suffix(".else")).unwrap_or(path);
        let (parent, index) = macro_editor::split_path(owner)?;
        if let StepKind::Repeat { of, .. } = &macro_editor::list_at(steps, parent)?.get(index)?.kind {
            return Some(*of);
        }
        path = parent;
    }
    None
}

/// Only canonical positive digits. No offset, formula, sheet prefix, or arbitrary token method.
pub(super) fn relative_axis(s: &str) -> Option<RepeatOf> {
    let (axis, n, max) = if let Some(n) = s.strip_prefix("Row.cell(1,").and_then(|n| n.strip_suffix(')')) {
        (RepeatOf::Rows, n, MAX_COLS)
    } else {
        (RepeatOf::Columns, s.strip_prefix("Column.cell(")?.strip_suffix(",1)")?, MAX_ROWS)
    };
    (!n.starts_with('0') && n.bytes().all(|c| c.is_ascii_digit()) && n.parse::<u32>().ok()? <= max).then_some(axis)
}

fn uses_iteration(steps: &[Step], inputs: &[&str]) -> bool {
    use gridcraft_engine::macros::resolve::{iteration_ref, params_use_iteration};
    steps.iter().any(|s| match &s.kind {
        StepKind::Command { params, .. } => params_use_iteration(params, inputs),
        StepKind::If { cell, then, otherwise, .. } => iteration_ref(cell) || uses_iteration(then, inputs) || uses_iteration(otherwise, inputs),
        // The inner Repeat resolves its header *before* binding a fresh item for its body.
        StepKind::Repeat { over, .. } => iteration_ref(over),
        // Native Stop messages and If comparison values are literal.
        StepKind::Stop { .. } => false,
    })
}

pub(super) fn update_schema() -> Value {
    let mut p = fields("flags");
    for k in ["if", "repeat", "stop"] {
        merge(&mut p, fields(k));
    }
    let mut out = object(p, &[]);
    out["minProperties"] = json!(1);
    out["description"] = json!(
        "Patch flags or fields of the existing step at index without replacing its body/branches. Fields must match the step type. Cannot be combined with steps/remove."
    );
    out
}

pub(super) fn header(kind: &StepKind) -> Value {
    match kind {
        StepKind::If { cell, op, value, then, otherwise } => {
            json!({"tool":"if","args":{"cell":cell,"op":op,"value":value},"then":then.len(),"else":otherwise.len()})
        }
        StepKind::Repeat { over, of, max, body } => json!({"tool":"repeat","args":{"over":over,"of":of,"max":max},"children":body.len()}),
        StepKind::Stop { message, rollback } => json!({"tool":"stop","args":{"message":message,"rollback":rollback}}),
        StepKind::Command { .. } => Value::Null,
    }
}

fn text(v: &Value, max: usize, nonempty: bool) -> Result<String, String> {
    v.as_str()
        .filter(|s| s.chars().count() <= max && (!nonempty || !s.trim().is_empty()))
        .map(str::to_owned)
        .ok_or_else(|| "Invalid macro text field.".into())
}
fn flag(v: &Value) -> Result<bool, String> {
    v.as_bool().ok_or_else(|| "Expected a boolean macro flag.".into())
}

fn header_field(kind: &mut StepKind, k: &str, v: &Value, inputs: &[&str]) -> Result<(), String> {
    match (kind, k) {
        (StepKind::If { cell, .. }, "cell") => *cell = text(v, 1024, true)?,
        (StepKind::If { op, .. }, "op") => *op = serde_json::from_value(v.clone()).map_err(|_| "Unknown macro comparison.")?,
        (StepKind::If { value, .. }, "value") if !v.is_array() && !v.is_object() && v.as_str().is_none_or(|s| s.chars().count() <= 4096) => {
            *value = v.clone()
        }
        (StepKind::Repeat { over, .. }, "over") => *over = text(v, 1024, true)?,
        (StepKind::Repeat { of, body, .. }, "of") => {
            let axis = serde_json::from_value(v.clone()).map_err(|_| "Repeat of rows or columns.")?;
            if *of != axis && uses_iteration(body, inputs) {
                return Err("Change or remove the current Repeat-relative parameters or references before switching its axis.".into());
            }
            *of = axis;
        }
        (StepKind::Repeat { max, .. }, "max") => *max = v.as_u64().filter(|n| (1..=100000).contains(n)).ok_or("Loop limit must be 1–100000.")? as u32,
        (StepKind::Stop { message, .. }, "message") => *message = text(v, 4096, false)?,
        (StepKind::Stop { rollback, .. }, "rollback") => *rollback = flag(v)?,
        _ => return Err(format!("{k} is not an editable field of this macro step.")),
    }
    Ok(())
}

pub(super) fn create(tool: &str, args: &Value, within: Option<RepeatOf>) -> Result<Option<Step>, String> {
    if tool == "set_cell" && args.get("sheet").is_none() {
        let axis = args["cell"]
            .as_str()
            .and_then(relative_axis)
            .ok_or("A literal cell requires a sheet; relative destinations must be Row.cell(1,n) or Column.cell(n,1).")?;
        if within != Some(axis) {
            return Err("The destination must match the nearest enclosing Repeat's rows or columns.".into());
        }
        if args.as_object().is_none_or(|o| o.len() != 2) || !super::workbook::valid_field("input", &args["input"]) {
            return Err("A Repeat-relative set_cell requires exactly cell and a text input.".into());
        }
        // The native macro interpreter resolves the sheet and cell on each iteration.
        // Its ordinary cell.set dispatch rechecks current sheet/cell protection at that time.
        return Ok(Some(Step::command("cell.set", args.clone())));
    }
    let (kind, required): (_, &[&str]) = match tool {
        "if" => {
            (StepKind::If { cell: String::new(), op: CompareOp::Eq, value: Value::Null, then: Vec::new(), otherwise: Vec::new() }, &["cell", "op"])
        }
        "repeat" => (StepKind::Repeat { over: String::new(), of: RepeatOf::Rows, max: 10000, body: Vec::new() }, &["over"]),
        "stop" => (StepKind::Stop { message: String::new(), rollback: false }, &[]),
        _ => return Ok(None),
    };
    let mut step = Step::new(kind);
    let obj = args.as_object().ok_or("Expected control-flow arguments.")?;
    if required.iter().any(|k| !obj.contains_key(*k)) {
        return Err("Missing required macro control-flow argument.".into());
    }
    for (k, v) in obj {
        header_field(&mut step.kind, k, v, &[])?;
    }
    Ok(Some(step))
}

// Re-enabling a disabled imported command, or an enclosing branch, must not turn an
// unsupported effect into something Wingman can enable through a generic flags edit.
fn safe_to_enable(step: &Step) -> bool {
    match &step.kind {
        StepKind::Command { id, .. } => {
            matches!(
                id.as_str(),
                "cell.set" | "range.setValues" | "home.formatCells" | "home.fillColor" | "home.fontColor" | "review.commentsToColumn"
            )
        }
        kind => kind.children().flat_map(|list| list.iter()).filter(|s| s.enabled).all(safe_to_enable),
    }
}

pub(super) fn update(step: &mut Step, patch: &Map<String, Value>, inputs: &[&str]) -> Result<(), String> {
    if patch.is_empty() {
        return Err("Specify at least one step field.".into());
    }
    for (k, v) in patch {
        match k.as_str() {
            "note" => step.note = text(v, 4096, false)?,
            "breakpoint" => step.breakpoint = flag(v)?,
            "enabled" => {
                let enable = flag(v)?;
                if enable && !step.enabled && !safe_to_enable(step) {
                    return Err("Wingman cannot enable this command or its enclosing control flow.".into());
                }
                step.enabled = enable;
            }
            key => header_field(&mut step.kind, key, v, inputs)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod axis_tests {
    use super::*;
    #[test]
    fn axis_change_checks_every_command_and_only_references_in_the_current_scope() {
        let repeat = |over: &str, body: Vec<Step>| Step::new(StepKind::Repeat { of: RepeatOf::Rows, over: over.into(), max: 8, body });
        let cond = |cell: &str, otherwise| {
            Step::new(StepKind::If { cell: cell.into(), op: CompareOp::Eq, value: json!("{{Column}}"), then: vec![], otherwise })
        };
        let set = |params| Step::command("range.setValues", params);
        let switch = |body| header_field(&mut repeat("A1:C3", body).kind, "of", &json!("columns"), &["Input"]);
        for params in [
            json!({"range":"Row.cell(1,2)", "values":[[7]]}),
            json!({"nested":{"ranges":["A1",{"$ref":"Column.offset(1,0)"}]}}),
            json!({"values":[["=SUM({{Row}})"]]}),
            json!({"target":"Row.right"}),
        ] {
            assert!(switch(vec![cond("A1", vec![set(params.clone())])]).is_err(), "{params}");
        }
        assert!(switch(vec![cond("Column.cell(1,1)", vec![])]).is_err());
        assert!(switch(vec![repeat("Row", vec![])]).is_err());
        assert!(switch(vec![repeat("A1:A3", vec![set(json!({"range":"Row.right"}))])]).is_ok());
        assert!(
            switch(vec![cond(
                "A1",
                vec![set(json!({"values":[["Row",{"$text":"{{Row}}"}]],"range":"'Row'!B2","area":"row","to":"Column","note":"{{{{Row}}"}))]
            )])
            .is_ok()
        );
    }
}
