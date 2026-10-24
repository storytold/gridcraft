//! Macros: typed command lists that run as one undo step. See [`crate::macros`] for the model,
//! tokens, runner and policy. Automate scripts (`automate.*`) are a separate, older feature.

use serde_json::Value as Json;

use super::*;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "macro.run",
            "Run Macro",
            [],
            None,
            "{name, inputs?: {Input: \"B2:D9\"} | \"B2:D9\", onConflict?: ask|fail|overwrite|shiftDown|shiftRight, overwrite?: bool, approve?: bool, timeLimit?: seconds} runs as one undo step; any error rolls it all back → {macro, ran, inputs, stopped?, shifted?}",
            has_doc,
            run
        ),
        cmd!(
            query "macro.check",
            "Check Macro",
            [],
            None,
            "{name, inputs?, timeLimit?: seconds} validates the inputs and dry-runs the macro on the workbook (then puts everything back) → {ok, errors?, needsApproval?, inputs, output?, conflicts, footprint}",
            has_doc,
            check
        ),
    ]
}

fn name_param<'a>(cmd: &str, p: &'a Json) -> Result<&'a str> {
    str_param(p, "name").filter(|n| !n.trim().is_empty()).ok_or_else(|| bad(cmd, "missing `name`"))
}

fn run(s: &mut Session, p: &Json) -> Result<Json> {
    let name = name_param("macro.run", p)?;
    crate::macros::run::run(s, name, p)
}

fn check(s: &mut Session, p: &Json) -> Result<Json> {
    let name = name_param("macro.check", p)?;
    crate::macros::run::check(s, name, p)
}
