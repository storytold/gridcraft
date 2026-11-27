//! The control-flow header is editable independently of its nested lists. No API accepts
//! raw commands or replaces a branch as a side effect of changing a condition.
use gridcraft_engine::model::{CompareOp, RepeatOf, Step, StepKind};
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
    for (kind, required) in [("command", &[][..]), ("if", &["cell", "op"]), ("repeat", &["over"]), ("stop", &[])] {
        let mut p = fields("flags");
        let (names, args) = if kind == "command" {
            (json!(["set_cell", "set_values", "format_range", "set_fill", "set_ink"]), json!({"type":"object"}))
        } else {
            (json!([kind]), object(fields(kind), required))
        };
        merge(&mut p, json!({"tool":{"type":"string","enum":names},"args":args}));
        variants.push(object(p, &["tool", "args"]));
    }
    json!({"anyOf":variants})
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

fn header_field(kind: &mut StepKind, k: &str, v: &Value) -> Result<(), String> {
    match (kind, k) {
        (StepKind::If { cell, .. }, "cell") => *cell = text(v, 1024, true)?,
        (StepKind::If { op, .. }, "op") => *op = serde_json::from_value(v.clone()).map_err(|_| "Unknown macro comparison.")?,
        (StepKind::If { value, .. }, "value") if !v.is_array() && !v.is_object() && v.as_str().is_none_or(|s| s.chars().count() <= 4096) => {
            *value = v.clone()
        }
        (StepKind::Repeat { over, .. }, "over") => *over = text(v, 1024, true)?,
        (StepKind::Repeat { of, .. }, "of") => *of = serde_json::from_value(v.clone()).map_err(|_| "Repeat of rows or columns.")?,
        (StepKind::Repeat { max, .. }, "max") => *max = v.as_u64().filter(|n| (1..=100000).contains(n)).ok_or("Loop limit must be 1–100000.")? as u32,
        (StepKind::Stop { message, .. }, "message") => *message = text(v, 4096, false)?,
        (StepKind::Stop { rollback, .. }, "rollback") => *rollback = flag(v)?,
        _ => return Err(format!("{k} is not an editable field of this macro step.")),
    }
    Ok(())
}

pub(super) fn create(tool: &str, args: &Value) -> Result<Option<Step>, String> {
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
        header_field(&mut step.kind, k, v)?;
    }
    Ok(Some(step))
}

// Re-enabling a disabled imported command, or an enclosing branch, must not turn an
// unsupported effect into something Wingman can enable through a generic flags edit.
fn safe_to_enable(step: &Step) -> bool {
    match &step.kind {
        StepKind::Command { id, .. } => {
            matches!(id.as_str(), "cell.set" | "range.setValues" | "home.formatCells" | "home.fillColor" | "home.fontColor")
        }
        kind => kind.children().flat_map(|list| list.iter()).filter(|s| s.enabled).all(safe_to_enable),
    }
}

pub(super) fn update(step: &mut Step, patch: &Map<String, Value>) -> Result<(), String> {
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
            key => header_field(&mut step.kind, key, v)?,
        }
    }
    Ok(())
}
