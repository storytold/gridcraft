//! Shared declarative-catalog loader: the single parse + validate path for every
//! category table.
//!
//! Each `functions/<category>.toml` file holds that category's rows as pure data.
//! This module parses one file into `Vec<FunctionSpec>`; the per-category modules
//! only pair their table with its category, so Sonar CPD sees one implementation
//! instead of fourteen copies of the same `spec(...)` scaffolding.
//!
//! TOML schema per `[[function]]` row:
//! - `name`, `min`, `max` (`max_open = true` means `usize::MAX`)
//! - `[[function.params]]`: `name`, `kind`, optional `optional = true`
//! - `[[function.signature]]`: `kinds`, `variadic`
//! - `returns`: `decimal`, `table`, ... (lowercase [`ReturnType`] names)
//! - `flags`: `standard`, `excel`, `engine`, ... (see [`flags`])
//! - `sugar`: optional name of the function this one is syntax sugar for
//! - `blank`: optional `blank` / `zero` / `unspecified` ([`BlankPolicy`])
//! - `description`, `examples`
//!
//! A malformed table is a developer error, not user input: every failure mode
//! returns [`CatalogError`] with the file's row and field attached. The accessors
//! share a loaded table through [`shared`]: on corrupt checked-in data it records
//! the error for [`failures`] and serves an empty slice, so a typo fails loudly in
//! `cargo test` instead of panicking the first `lookup()` (the crate denies
//! `clippy::panic`, and binding must never crash the host).

use super::{BlankPolicy, FunctionCategory, FunctionFlags, FunctionSpec, Overload, ParamKind, Parameter, ReturnType};

/// What went wrong while loading one category table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogError {
    /// The `[[function]]` row being built, if the failure is row-specific.
    pub function: Option<String>,
    /// Machine-readable cause.
    pub reason: String,
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.function {
            Some(function) => write!(formatter, "catalog row `{function}`: {}", self.reason),
            None => write!(formatter, "catalog table: {}", self.reason),
        }
    }
}

impl std::error::Error for CatalogError {}

#[derive(Debug, serde::Deserialize)]
struct CatalogFile {
    #[serde(default)]
    function: Vec<Row>,
}

#[derive(Debug, serde::Deserialize)]
struct Row {
    name: String,
    min: usize,
    max: usize,
    #[serde(default)]
    max_open: bool,
    #[serde(default)]
    params: Vec<Param>,
    #[serde(default)]
    signature: Vec<Signature>,
    returns: String,
    #[serde(default)]
    flags: Flags,
    #[serde(default)]
    sugar: Option<String>,
    #[serde(default)]
    blank: Option<String>,
    description: String,
    examples: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
struct Param {
    name: String,
    kind: String,
    #[serde(default)]
    optional: bool,
}

#[derive(Debug, serde::Deserialize)]
struct Signature {
    kinds: Vec<String>,
    #[serde(default)]
    variadic: bool,
}

#[derive(Debug, Default, serde::Deserialize)]
#[serde(untagged)]
enum Flags {
    #[default]
    Standard,
    Named(String),
    Tuple((bool, bool, bool, bool, bool)),
}

fn fail(function: Option<&str>, reason: impl Into<String>) -> CatalogError {
    CatalogError { function: function.map(str::to_owned), reason: reason.into() }
}

fn kind(name: &str) -> Result<ParamKind, CatalogError> {
    match name {
        "Column" => Ok(ParamKind::Column),
        "Table" => Ok(ParamKind::Table),
        "Expression" => Ok(ParamKind::Expression),
        "Scalar" => Ok(ParamKind::Scalar),
        "Logical" => Ok(ParamKind::Logical),
        "Any" => Ok(ParamKind::Any),
        "Date" => Ok(ParamKind::Date),
        "Text" => Ok(ParamKind::Text),
        "Number" => Ok(ParamKind::Number),
        "Boolean" => Ok(ParamKind::Boolean),
        "Function" => Ok(ParamKind::Function),
        "Reference" => Ok(ParamKind::Reference),
        "Json" => Ok(ParamKind::Json),
        "Variant" => Ok(ParamKind::Variant),
        other => Err(fail(None, format!("unknown ParamKind `{other}`"))),
    }
}

fn returns(name: &str) -> Result<ReturnType, CatalogError> {
    match name {
        "integer" => Ok(ReturnType::Integer),
        "numeric" => Ok(ReturnType::Numeric),
        "currency" => Ok(ReturnType::Currency),
        "decimal" => Ok(ReturnType::Decimal),
        "any" => Ok(ReturnType::Any),
        "boolean" => Ok(ReturnType::Boolean),
        "datetime" => Ok(ReturnType::DateTime),
        "string" => Ok(ReturnType::String),
        "table" => Ok(ReturnType::Table),
        "blank" => Ok(ReturnType::Blank),
        "variant" => Ok(ReturnType::Variant),
        other => Err(fail(None, format!("unknown ReturnType `{other}`"))),
    }
}

fn flags(flags: &Flags, iterator: bool) -> Result<FunctionFlags, CatalogError> {
    match flags {
        Flags::Standard => Ok(super::standard(iterator)),
        Flags::Named(name) => match name.as_str() {
            "standard" => Ok(super::standard(false)),
            "standard-iterator" => Ok(super::standard(true)),
            "standard-excel" => Ok(FunctionFlags::new(false, false, false, true, false)),
            "excel" => Ok(FunctionFlags::new(false, false, false, true, true)),
            "iterator-excel" => Ok(FunctionFlags::new(true, false, false, true, true)),
            "engine" => Ok(FunctionFlags::new(false, false, false, false, true)),
            "iterator-engine" => Ok(FunctionFlags::new(true, false, false, false, true)),
            "discouraged" => Ok(FunctionFlags::new(false, false, true, true, true)),
            "eager" => Ok(FunctionFlags::new(false, false, true, false, true)),
            "dq" => Ok(FunctionFlags::new(false, false, false, true, true)),
            "volatile-excel" => Ok(FunctionFlags::new(false, true, false, true, true)),
            "volatile-engine" => Ok(FunctionFlags::new(false, true, false, false, true)),
            "volatile-visual" => Ok(FunctionFlags::new(false, true, false, false, false)),
            "volatile-standard" => Ok(FunctionFlags::new(false, true, false, true, false)),
            "visual" => Ok(FunctionFlags::new(false, false, false, false, false)),
            "internal" => Ok(FunctionFlags::new(false, false, false, false, true)),
            other => Err(fail(None, format!("unknown flags `{other}`"))),
        },
        Flags::Tuple((iterator_flag, volatile, not_recommended, excel, dq)) => {
            Ok(FunctionFlags::new(*iterator_flag || iterator, *volatile, *not_recommended, *excel, *dq))
        }
    }
}

fn blank(name: Option<&str>) -> Result<BlankPolicy, CatalogError> {
    match name {
        None => Ok(BlankPolicy::Unspecified),
        Some("blank") => Ok(BlankPolicy::BlankWhenEmpty),
        Some("zero") => Ok(BlankPolicy::ZeroWhenRowsButNoMatch),
        Some("unspecified") => Ok(BlankPolicy::Unspecified),
        Some(other) => Err(fail(None, format!("unknown blank policy `{other}`"))),
    }
}

fn build(category: FunctionCategory, file: CatalogFile) -> Result<Vec<FunctionSpec>, CatalogError> {
    file.function
        .into_iter()
        .map(|row| {
            let label = row.name.clone();
            let at = |reason: String| fail(Some(&label), reason);
            if row.name.is_empty() {
                return Err(fail(None, "catalog row with an empty name".to_owned()));
            }
            if row.description.is_empty() {
                return Err(at("no description".to_owned()));
            }
            if row.examples.is_empty() {
                return Err(at("no examples".to_owned()));
            }
            let max = if row.max_open { usize::MAX } else { row.max };
            if row.min > max {
                return Err(at(format!("min_args {} above max_args {}", row.min, max)));
            }
            let mut params = Vec::with_capacity(row.params.len());
            for param in &row.params {
                if param.name.is_empty() {
                    return Err(at("a parameter with an empty name".to_owned()));
                }
                params.push(Parameter {
                    name: param.name.clone().leak(),
                    kind: kind(&param.kind).map_err(|error| at(error.reason))?,
                    optional: param.optional,
                });
            }
            let mut overloads = Vec::with_capacity(row.signature.len());
            for overload in &row.signature {
                let mut kinds = Vec::with_capacity(overload.kinds.len());
                for name in &overload.kinds {
                    kinds.push(kind(name).map_err(|error| at(error.reason))?);
                }
                overloads.push(Overload { params: kinds.leak(), variadic: overload.variadic });
            }
            for arity in row.min..=max.min(row.min.saturating_add(64)) {
                if !overloads.iter().any(|overload| overload.matches(arity)) {
                    return Err(at(format!("no overload for arity {arity}")));
                }
            }
            let name: &'static str = row.name.leak();
            let params: &'static [Parameter] = params.leak();
            let overloads: &'static [Overload] = overloads.leak();
            let description: &'static str = row.description.leak();
            let mut owned_examples = Vec::with_capacity(row.examples.len());
            for example in row.examples {
                owned_examples.push(example.leak() as &str);
            }
            Ok(FunctionSpec {
                name,
                category,
                min_args: row.min,
                max_args: max,
                params,
                overloads,
                return_type: returns(&row.returns).map_err(|error| at(error.reason))?,
                flags: flags(&row.flags, false).map_err(|error| at(error.reason))?,
                sugar_of: row.sugar.map(|sugar| sugar.leak() as &str),
                blank_policy: blank(row.blank.as_deref()).map_err(|error| at(error.reason))?,
                description,
                examples: owned_examples.leak(),
            })
        })
        .collect()
}

/// Parses one category table, validating every field: a typo in the data fails
/// loudly instead of silently changing behaviour.
pub fn load(category: FunctionCategory, text: &str) -> Result<Vec<FunctionSpec>, CatalogError> {
    let file: CatalogFile = toml::from_str(text).map_err(|error| fail(None, format!("TOML does not parse: {error}")))?;
    build(category, file)
}

/// The table served when a category file fails to load: empty, so callers keep
/// working (and the fidelity test reports the recorded error instead).
static EMPTY: [FunctionSpec; 0] = [];

/// One entry per category table that failed to load, for the fidelity test.
static FAILURES: std::sync::Mutex<Vec<CatalogError>> = std::sync::Mutex::new(Vec::new());

/// Shares a lazily loaded category table with its accessor: the table on success,
/// an empty slice with the error recorded in [`failures`] on corrupt checked-in
/// data. Never panics, so binding a typo'd catalog degrades to "unknown function"
/// instead of crashing the host (CodeRabbit, #3412).
pub fn shared(table: &'static Result<Vec<FunctionSpec>, CatalogError>) -> &'static [FunctionSpec] {
    match table {
        Ok(functions) => functions,
        Err(error) => {
            if let Ok(mut failures) = FAILURES.lock()
                && !failures.contains(error)
            {
                failures.push(error.clone());
            }
            &EMPTY
        }
    }
}

/// Every category-table load error recorded by [`shared`] so far.
pub fn failures() -> Vec<CatalogError> {
    FAILURES.lock().map(|failures| failures.clone()).unwrap_or_default()
}
