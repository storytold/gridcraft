//! What Craft AI does inside GridCraft. Craft AI is one assistant across the ArtCraft family
//! — the model, the runtime and the shared folder belong to the family — but its *work* is the
//! app's own: the system prompt tells the model which app it is inside and what that app does,
//! and every ready-made task is framed for that app's world, so the answer that comes back is
//! about cells, ranges and ribbon commands, not generic advice.
//!
//! Each app of the family carries its own `tasks.rs` — WordCraft's asks about documents and
//! styles, VectorCraft's about paths and Pathfinder steps, and so on — same model, same shared
//! folder, different craft. The labels and hints are catalog keys: the panel shows them
//! translated.

/// The app Craft AI is embedded in, as the shared folder's records name it.
pub const APP: &str = "GridCraft";

/// One ready-made task of the Assistant tab.
pub struct Task {
    /// The label the picker shows (a catalog key).
    pub label: &'static str,
    /// The input field's hint (a catalog key).
    pub hint: &'static str,
}

/// The tasks, in the picker's order.
pub const TASKS: &[Task] = &[
    Task { label: "Ask anything", hint: "Ask in any language" },
    Task { label: "Explain a formula", hint: "Paste the formula, or a cell like =B2*1.1" },
    Task { label: "Build a formula", hint: "What to compute, and where the numbers are" },
    Task { label: "Clean or transform data", hint: "Describe the data and the shape you want" },
    Task { label: "Chart and pivot advice", hint: "The data and the story it should tell" },
    Task { label: "Function finder", hint: "What you need, in plain words" },
];

/// Tell the model where it is and how to answer: briefly, practically, in the language the user
/// wrote in, about this app's own world — the words of its interface, the files it opens and
/// writes. The last line is the honesty clause: a local model can be wrong, and it is told to
/// say so rather than invent.
pub fn system_prompt() -> String {
    "You are Craft AI, the assistant built into GridCraft, a desktop spreadsheet. GridCraft's \
     world, in its own words: cells, ranges and A1 references; formulas and the worksheet \
     functions it carries (SUM, IF, XLOOKUP, TEXTJOIN and the rest); tables and PivotTables; \
     charts; conditional formatting and data validation; sorting and filtering; freeze panes; \
     .xlsx, .csv and .tsv files opened and written; and the ribbon's Home, Insert, Data and \
     Formulas tabs. Answer in the language the user wrote in, Arabic included. Be brief and \
     practical: name the ribbon tab and the command, give the steps as a short list, say what to \
     watch out for, and when a question is not about GridCraft or spreadsheets, answer it \
     anyway, briefly. Never invent function or menu names: if you are not sure GridCraft has a \
     function or a command, say so plainly."
        .to_string()
}

/// Frame the user's input for the task they picked.
pub fn user_prompt(task: usize, input: &str) -> String {
    let input = input.trim();
    match TASKS[task.min(TASKS.len() - 1)].label {
        "Explain a formula" => {
            format!("In GridCraft, explain in practical steps how this formula works — what it computes, and in what order: {input}")
        }
        "Build a formula" => format!("In GridCraft, build a working formula for this, naming the cells or ranges the numbers are in: {input}"),
        "Clean or transform data" => {
            format!("In GridCraft, explain in practical steps how to clean or transform this data into the shape wanted: {input}")
        }
        "Chart and pivot advice" => format!(
            "Advise on charting or pivoting this data for the story it should tell — chart type, fields, layout, pitfalls — in a short list: {input}"
        ),
        "Function finder" => format!("In GridCraft, name the worksheet functions for this need, with a one-line job description each: {input}"),
        _ => input.to_string(),
    }
}
