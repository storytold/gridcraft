//! One command of a script: `id={json}`, `id {json}`, `id` or `{"command": id, "params": {..}}`.

use serde_json::{Value, json};

/// A command line that cannot be read.
#[derive(Debug, PartialEq, thiserror::Error)]
pub(crate) enum ScriptError {
    #[error("bad JSON: {0}")]
    BadJson(String),
    /// A key this does not read is a mistake, not a comment: `{"command": …, "args": {…}}` and
    /// `{"command": …, "name": "Report"}` ran the command with no parameters at all and the script still exited 0, so a
    /// typo made a valid, wrong workbook (#2256).
    #[error("unknown {plural} `{list}`: a line is {{\"command\": \"id\", \"params\": {{…}}}}")]
    UnknownKeys { plural: &'static str, list: String },
    #[error("JSON command needs a `command` field")]
    NoCommand,
    #[error("{id}: bad JSON params: {why}")]
    BadParams { id: String, why: String },
}

/// A command and its parameters.
pub(crate) type Step = (String, Value);

/// The command in `text`, or `None` for a blank line, a `#` comment or a `//` comment.
pub(crate) fn parse_command(text: &str) -> Result<Option<Step>, ScriptError> {
    let t = text.trim();
    if t.is_empty() || t.starts_with('#') || t.starts_with("//") {
        return Ok(None);
    }
    if t.starts_with('{') {
        let v: Value = serde_json::from_str(t).map_err(|e| ScriptError::BadJson(e.to_string()))?;
        if let Some(o) = v.as_object() {
            let mut unknown: Vec<&str> = o.keys().map(String::as_str).filter(|k| !matches!(*k, "command" | "id" | "params")).collect();
            if !unknown.is_empty() {
                unknown.sort_unstable();
                let plural = if unknown.len() == 1 { "key" } else { "keys" };
                return Err(ScriptError::UnknownKeys { plural, list: unknown.join("`, `") });
            }
        }
        let id = v.get("command").or_else(|| v.get("id")).and_then(Value::as_str).ok_or(ScriptError::NoCommand)?;
        return Ok(Some((id.to_string(), v.get("params").cloned().unwrap_or_else(|| json!({})))));
    }
    let split = t.find(['=', ' ', '\t']);
    let (id, rest) = match split {
        Some(i) => (t.get(..i).unwrap_or(t), t.get(i + 1..).unwrap_or("").trim()),
        None => (t, ""),
    };
    let params = if rest.is_empty() {
        json!({})
    } else {
        serde_json::from_str(rest).map_err(|e| ScriptError::BadParams { id: id.to_string(), why: e.to_string() })?
    };
    Ok(Some((id.to_string(), params)))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("home.bold={\"range\":\"A1\"}").unwrap(), Some(("home.bold".into(), json!({"range": "A1"}))));
        assert_eq!(parse_command("edit.undo").unwrap(), Some(("edit.undo".into(), json!({}))));
        assert_eq!(
            parse_command("cell.set {\"cell\":\"A1\",\"input\":\"=1\"}").unwrap(),
            Some(("cell.set".into(), json!({"cell": "A1", "input": "=1"})))
        );
        assert_eq!(parse_command("{\"command\":\"edit.redo\"}").unwrap(), Some(("edit.redo".into(), json!({}))));
        assert_eq!(parse_command("  # comment").unwrap(), None);
        assert!(parse_command("x={bad").is_err());
        // A key this does not read is a mistake: the command used to run with no parameters and
        // the script still exited 0, so a typo made a valid, wrong workbook (#2256).
        for (line, named) in [
            (r#"{"command":"home.insertSheet","args":{"name":"R"}}"#, "`args`"),
            (r#"{"command":"home.insertSheet","name":"R"}"#, "`name`"),
            (r#"{"command":"home.insertSheet","params":{},"note":"why"}"#, "`note`"),
        ] {
            let e = parse_command(line).expect_err("refused").to_string();
            assert!(e.contains("unknown key") && e.contains(named), "{line}: {e}");
            assert!(e.contains(r#"{"command": "id", "params": {…}}"#), "{line}: {e}");
        }
        // Several at once are listed in one message, sorted.
        let e = parse_command(r#"{"command":"x","name":"R","args":{}}"#).expect_err("refused").to_string();
        assert!(e.contains("unknown keys `args`, `name`"), "{e}");
        // `id` is still the accepted alternative to `command`, and `params` still carries them.
        assert_eq!(parse_command(r#"{"id":"edit.undo"}"#).unwrap(), Some(("edit.undo".into(), json!({}))));
        assert_eq!(
            parse_command(r#"{"command":"home.insertSheet","params":{"name":"R"}}"#).unwrap(),
            Some(("home.insertSheet".into(), json!({"name": "R"})))
        );
    }
}
