//! GridCraft macros: typed, declarative command lists that run as one undo step. They are not VBA:
//! a VBA project in a file is carried through unchanged (`Workbook::vba_project`) and never run.

use serde::{Deserialize, Serialize};

/// The macro format version written by this build.
pub const MACRO_VERSION: u32 = 1;
/// The most macros a workbook keeps.
pub const MAX_MACROS: usize = 1_000;
/// The most steps in one macro, counting the steps inside Repeat and If.
pub const MAX_MACRO_STEPS: usize = 10_000;
/// How deeply Repeat and If may nest (a top-level step is at depth 1).
pub const MAX_MACRO_DEPTH: usize = 4;
/// The longest macro or input name, in characters.
pub const MAX_MACRO_NAME: usize = 255;
/// The longest macro description or step note, in characters.
pub const MAX_MACRO_TEXT: usize = 4_096;

/// A recorded or hand-made macro: a list of typed steps that runs as one undo step. Most steps run
/// a GridCraft command; Repeat, If and Stop add bounded control flow. References in step
/// parameters can be tokens (`"Input.col(2)"`) that are resolved against the declared inputs when
/// the macro runs, so a macro recorded on one block of data works on another.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Macro {
    /// Format version ([`MACRO_VERSION`]).
    pub v: u32,
    pub name: String,
    pub description: String,
    /// Keyboard shortcut (`Ctrl+Shift+M`) for a user interface to offer; the engine only stores it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    /// The ranges the macro works on, checked before it runs.
    pub inputs: Vec<NamedInput>,
    pub steps: Vec<Step>,
    /// Where results go and what to do when they would land on existing data.
    pub output: OutputRule,
    /// Commands that need permission (reading or writing files) which the macro may run.
    pub permissions: Vec<String>,
}

impl Default for Macro {
    fn default() -> Self {
        Macro {
            v: MACRO_VERSION,
            name: String::new(),
            description: String::new(),
            shortcut: None,
            inputs: vec![],
            steps: vec![],
            output: OutputRule::default(),
            permissions: vec![],
        }
    }
}

impl Macro {
    /// Steps in the macro, counting those inside Repeat and If.
    pub fn step_count(&self) -> usize {
        fn count(steps: &[Step]) -> usize {
            steps.iter().map(|s| 1usize.saturating_add(s.kind.children().map(|b| count(b)).sum::<usize>())).sum()
        }
        count(&self.steps)
    }

    /// Checks the format version ([`check_version`]), the size limits ([`MAX_MACRO_STEPS`],
    /// [`MAX_MACRO_DEPTH`], name and text lengths) and that the name has no control characters.
    pub fn check_limits(&self) -> Result<(), String> {
        check_version(u64::from(self.v))?;
        let name = self.name.trim();
        if name.is_empty() {
            return Err("A macro needs a name.".into());
        }
        if name.chars().count() > MAX_MACRO_NAME {
            return Err(format!("A macro name can be at most {MAX_MACRO_NAME} characters long."));
        }
        if name.chars().any(char::is_control) {
            return Err("A macro name can't hold line breaks or other control characters.".into());
        }
        if self.description.chars().count() > MAX_MACRO_TEXT {
            return Err(format!("A macro description can be at most {MAX_MACRO_TEXT} characters long."));
        }
        // Count without recursing past the depth limit (a hostile file can nest deeper).
        fn walk(steps: &[Step], level: usize, count: &mut usize) -> Result<(), String> {
            if level > MAX_MACRO_DEPTH {
                return Err(format!("Repeat and If steps can nest at most {MAX_MACRO_DEPTH} deep."));
            }
            for s in steps {
                *count = count.saturating_add(1);
                if *count > MAX_MACRO_STEPS {
                    return Err(format!("A macro can have at most {MAX_MACRO_STEPS} steps."));
                }
                if s.note.chars().count() > MAX_MACRO_TEXT {
                    return Err(format!("A step note can be at most {MAX_MACRO_TEXT} characters long."));
                }
                for body in s.kind.children() {
                    walk(body, level + 1, count)?;
                }
            }
            Ok(())
        }
        walk(&self.steps, 1, &mut 0)?;
        if self.inputs.len() > 64 {
            return Err("A macro can declare at most 64 inputs.".into());
        }
        for (k, i) in self.inputs.iter().enumerate() {
            let n = i.name.as_str();
            let ident = n.chars().next().is_some_and(char::is_alphabetic) && n.chars().all(|c| c.is_alphanumeric() || c == '_');
            if !ident || n.chars().count() > MAX_MACRO_NAME {
                return Err(format!("'{n}' can't name an input: use letters, digits and _, starting with a letter."));
            }
            if ["Output", "Row", "Column", "Name"].iter().any(|r| r.eq_ignore_ascii_case(n)) {
                return Err(format!("'{n}' is a token of its own and can't name an input."));
            }
            if self.inputs.iter().take(k).any(|o| o.name.eq_ignore_ascii_case(n)) {
                return Err(format!("Two inputs are called '{n}'."));
            }
        }
        Ok(())
    }
}

/// Refuses a format version newer than [`MACRO_VERSION`]: this build would drop the fields it
/// doesn't know, and a save would then write the rest back as if it were the whole macro.
pub fn check_version(v: u64) -> Result<(), String> {
    if v > u64::from(MACRO_VERSION) {
        return Err(format!("This macro was made by a newer version of GridCraft (macro format {v}); this version reads format {MACRO_VERSION}."));
    }
    Ok(())
}

/// Reads the `macros` field of a JSON workbook as raw entries, whatever they hold, so they can be
/// checked one at a time.
pub(crate) fn entries<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<serde_json::Value>, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::Array(list) => list,
        serde_json::Value::Null => vec![],
        other => vec![other],
    })
}

/// A range the macro works on, with the shape and content it expects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NamedInput {
    /// The token that stands for it in step parameters (`Input`, `Rates`).
    pub name: String,
    pub kind: InputKind,
    /// Allowed data rows (header excluded), `[min, max]`; a max of 0 means no limit.
    pub rows: [u32; 2],
    /// Allowed columns, `[min, max]`; a max of 0 means no limit.
    pub cols: [u32; 2],
    /// The first row holds column names.
    pub header: bool,
    /// What each column must hold, first column first; columns past the list may hold anything.
    pub col_types: Vec<ColType>,
    /// Every data cell must have something in it.
    pub non_empty: bool,
    /// The range used when a run doesn't name one (else the current selection, for the first input).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_ref: Option<String>,
}

impl Default for NamedInput {
    fn default() -> Self {
        NamedInput {
            name: "Input".into(),
            kind: InputKind::Range,
            rows: [1, 0],
            cols: [1, 0],
            header: false,
            col_types: vec![],
            non_empty: false,
            default_ref: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InputKind {
    /// A single cell.
    Cell,
    #[default]
    Range,
}

/// What a column of an input must hold. Empty cells pass unless the input is `non_empty`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColType {
    Number,
    Text,
    /// A number shown with a date or time format.
    Date,
    Bool,
    #[default]
    Any,
}

impl ColType {
    /// The plural noun used in messages ("must hold numbers").
    pub fn noun(self) -> &'static str {
        match self {
            ColType::Number => "numbers",
            ColType::Text => "text",
            ColType::Date => "dates",
            ColType::Bool => "TRUE or FALSE",
            ColType::Any => "anything",
        }
    }
}

/// One macro step and its editor flags.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    #[serde(flatten)]
    pub kind: StepKind,
    /// A disabled step is skipped.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
    /// The debugger stops before this step.
    #[serde(default, skip_serializing_if = "is_false")]
    pub breakpoint: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

fn yes() -> bool {
    true
}
fn is_true(b: &bool) -> bool {
    *b
}
fn is_false(b: &bool) -> bool {
    !*b
}

impl Step {
    pub fn new(kind: StepKind) -> Step {
        Step { kind, enabled: true, breakpoint: false, note: String::new() }
    }

    /// A step that runs command `id` with `params`.
    pub fn command(id: impl Into<String>, params: serde_json::Value) -> Step {
        Step::new(StepKind::Command { id: id.into(), params })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StepKind {
    /// Runs a GridCraft command. String parameters may hold tokens, resolved when the step runs.
    Command {
        id: String,
        #[serde(default)]
        params: serde_json::Value,
    },
    /// Runs `body` once for each row (or column) of the range `over`, a token or reference; inside
    /// it, `Row` (or `Column`) stands for the current one. At most `max` times.
    Repeat {
        over: String,
        #[serde(default)]
        of: RepeatOf,
        #[serde(default = "repeat_max")]
        max: u32,
        #[serde(default)]
        body: Vec<Step>,
    },
    /// Compares the value of `cell` (a token or reference to one cell) with `value` and runs
    /// `then` or `else`.
    If {
        cell: String,
        op: CompareOp,
        #[serde(default)]
        value: serde_json::Value,
        #[serde(default)]
        then: Vec<Step>,
        #[serde(default, rename = "else")]
        otherwise: Vec<Step>,
    },
    /// Ends the run with a message. The changes made so far are kept, or undone with `rollback`.
    Stop {
        #[serde(default)]
        message: String,
        #[serde(default, skip_serializing_if = "is_false")]
        rollback: bool,
    },
}

fn repeat_max() -> u32 {
    10_000
}

impl StepKind {
    /// The step lists inside this step (Repeat's body, If's branches).
    pub fn children(&self) -> impl Iterator<Item = &Vec<Step>> {
        let (a, b) = match self {
            StepKind::Repeat { body, .. } => (Some(body), None),
            StepKind::If { then, otherwise, .. } => (Some(then), Some(otherwise)),
            _ => (None, None),
        };
        a.into_iter().chain(b)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RepeatOf {
    #[default]
    Rows,
    Columns,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    /// The cell is empty (`value` is ignored).
    Empty,
    NotEmpty,
    /// The cell's text contains `value`'s text, ignoring case.
    Contains,
}

/// Where a macro's results go.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OutputRule {
    /// What the `Output` token stands for: a token (`Input.offset(0,4)`), a reference or `Name:X`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// What to do when the cells the macro writes, outside its inputs, already hold data.
    pub on_conflict: OnConflict,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OnConflict {
    /// Ask first: overwrite, shift the data down or right, or cancel. Programmatic runs fail.
    #[default]
    Ask,
    Fail,
    Overwrite,
    ShiftDown,
    ShiftRight,
}

impl OnConflict {
    pub fn parse(s: &str) -> Option<OnConflict> {
        Some(match s.to_ascii_lowercase().replace(['_', '-', ' '], "").as_str() {
            "ask" => OnConflict::Ask,
            "fail" | "cancel" => OnConflict::Fail,
            "overwrite" => OnConflict::Overwrite,
            "shiftdown" => OnConflict::ShiftDown,
            "shiftright" => OnConflict::ShiftRight,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            OnConflict::Ask => "ask",
            OnConflict::Fail => "fail",
            OnConflict::Overwrite => "overwrite",
            OnConflict::ShiftDown => "shiftDown",
            OnConflict::ShiftRight => "shiftRight",
        }
    }
}
