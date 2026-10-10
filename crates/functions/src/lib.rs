//! GridCraft worksheet function library.
//!
//! Every function is described by a [`FnSpec`] (name, arity, category, which parameters take
//! scalars, a signature and a one-line description) and implemented as a plain Rust function over
//! already-evaluated arguments. The calculation engine looks functions up by name with
//! [`lookup`] and invokes them through [`call`], which applies Excel's automatic array lifting:
//! passing an array where a scalar is expected evaluates the function once per element.
//!
//! Special forms that need references, laziness or hidden-row information (ROW, OFFSET,
//! INDIRECT, LET, LAMBDA, SUBTOTAL, AGGREGATE, TEXT…) are evaluated by the host; this crate
//! exports [`aggregate_values`] for SUBTOTAL/AGGREGATE.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::sync::OnceLock;

use gridcraft_core::{CellError, DateSystem, Value};
use gridcraft_locale::{INVARIANT, Locale};

/// Builds a [`FnSpec`] whose implementation returns `Result<Value, CellError>`.
///
/// `f!(NAME, min, max, Category, scalar_args, "SIG", "description", path::to::impl)`; prefix the
/// arguments with `volatile;` for volatile functions.
macro_rules! f {
    (volatile; $name:literal, $min:expr, $max:expr, $cat:ident, $scal:expr, $sig:literal, $desc:literal, $imp:expr) => {
        $crate::FnSpec {
            name: $name,
            min_args: $min,
            max_args: $max,
            category: $crate::Category::$cat,
            volatile: true,
            scalar_args: $scal,
            signature: $sig,
            description: $desc,
            imp: {
                fn w(a: &[$crate::Arg], c: &mut dyn $crate::Ctx) -> gridcraft_core::Value {
                    $crate::util::finish(($imp)(a, c))
                }
                w
            },
        }
    };
    ($name:literal, $min:expr, $max:expr, $cat:ident, $scal:expr, $sig:literal, $desc:literal, $imp:expr) => {
        $crate::FnSpec {
            name: $name,
            min_args: $min,
            max_args: $max,
            category: $crate::Category::$cat,
            volatile: false,
            scalar_args: $scal,
            signature: $sig,
            description: $desc,
            imp: {
                fn w(a: &[$crate::Arg], c: &mut dyn $crate::Ctx) -> gridcraft_core::Value {
                    $crate::util::finish(($imp)(a, c))
                }
                w
            },
        }
    };
}

pub(crate) mod criteria;
pub(crate) mod database;
pub(crate) mod date;
mod docs;
pub(crate) mod dynamic;
pub(crate) mod engineering;
pub(crate) mod financial;
pub(crate) mod info;
pub(crate) mod lift;
pub(crate) mod lookup;
pub(crate) mod math;
pub(crate) mod special;
pub(crate) mod stat;
pub(crate) mod text;
pub(crate) mod util;

pub use docs::{FnDoc, docs, is_special};
pub use stat::{aggregate_values, aggregate_values_k};

/// An evaluated argument. Ranges arrive as `Value::Array` (blank cells = `Value::Empty`) with
/// `from_ref = true`.
#[derive(Clone, Debug)]
pub struct Arg {
    pub value: Value,
    pub from_ref: bool,
}

impl Arg {
    /// A literal (non-reference) argument.
    pub fn val(value: impl Into<Value>) -> Arg {
        Arg { value: value.into(), from_ref: false }
    }
    /// An argument that came from a cell or range reference.
    pub fn reference(value: impl Into<Value>) -> Arg {
        Arg { value: value.into(), from_ref: true }
    }
}

/// Services a function may need from the host.
pub trait Ctx {
    fn date_system(&self) -> DateSystem;
    /// Current local date-time serial.
    fn now_serial(&self) -> f64;
    /// Uniform in [0, 1).
    fn random(&mut self) -> f64;
    /// Language and region of the workbook: how text is read as numbers and numbers are written
    /// as text, which `TRUE`/`FALSE` and `CELL` keywords are accepted. Invariant en-US unless
    /// the host overrides it.
    fn locale(&self) -> &Locale {
        &INVARIANT
    }
}

pub type FnImpl = fn(&[Arg], &mut dyn Ctx) -> Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Financial,
    Logical,
    Text,
    DateTime,
    Lookup,
    MathTrig,
    Statistical,
    Engineering,
    Information,
    Database,
    Compatibility,
    Web,
    Cube,
}

pub struct FnSpec {
    /// Upper-case, without any `_xlfn.` prefix.
    pub name: &'static str,
    pub min_args: usize,
    /// `usize::MAX` for variadic functions (Excel allows 255 arguments).
    pub max_args: usize,
    pub category: Category,
    pub volatile: bool,
    /// For automatic array lifting: true for each parameter position that expects a scalar. When
    /// an argument at a scalar position is an array, the function is applied element-wise
    /// (broadcast like Excel's dynamic arrays). The last entry repeats for variadic tails; an
    /// empty slice means no lifting.
    pub scalar_args: &'static [bool],
    pub signature: &'static str,
    pub description: &'static str,
    pub imp: FnImpl,
}

impl std::fmt::Debug for FnSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FnSpec").field("name", &self.name).field("min_args", &self.min_args).field("max_args", &self.max_args).finish()
    }
}

impl FnSpec {
    /// Whether parameter `i` expects a scalar (and is therefore lifted over arrays).
    pub fn is_scalar_arg(&self, i: usize) -> bool {
        match self.scalar_args.get(i) {
            Some(b) => *b,
            None => self.scalar_args.last().copied().unwrap_or(false),
        }
    }
}

/// Variadic marker for `max_args`.
pub(crate) const VAR: usize = usize::MAX;

fn registry() -> &'static (Vec<FnSpec>, HashMap<&'static str, usize>) {
    static REG: OnceLock<(Vec<FnSpec>, HashMap<&'static str, usize>)> = OnceLock::new();
    REG.get_or_init(|| {
        let mut v: Vec<FnSpec> = Vec::new();
        for list in [
            math::specs(),
            stat::specs(),
            text::specs(),
            date::specs(),
            lookup::specs(),
            dynamic::specs(),
            financial::specs(),
            info::specs(),
            engineering::specs(),
            database::specs(),
        ] {
            v.extend(list);
        }
        let mut map = HashMap::with_capacity(v.len());
        for (i, s) in v.iter().enumerate() {
            map.entry(s.name).or_insert(i);
        }
        (v, map)
    })
}

/// Looks a function up by name (case-insensitive; `_xlfn.`, `_xlws.` and `_xludf.`-free names).
pub fn lookup(name: &str) -> Option<&'static FnSpec> {
    let mut n = name.trim().to_ascii_uppercase();
    loop {
        if let Some(r) = n.strip_prefix("_XLFN.") {
            n = r.to_string();
        } else if let Some(r) = n.strip_prefix("_XLWS.") {
            n = r.to_string();
        } else {
            break;
        }
    }
    let (v, map) = registry();
    map.get(n.as_str()).and_then(|&i| v.get(i))
}

/// Every registered function.
pub fn all() -> &'static [FnSpec] {
    &registry().0
}

/// Calls a function with array lifting applied. Wrong arity gives `#VALUE!`.
pub fn call(spec: &FnSpec, args: &[Arg], ctx: &mut dyn Ctx) -> Value {
    if args.len() < spec.min_args || args.len() > spec.max_args || args.len() > 255 {
        return Value::Error(CellError::Value);
    }
    lift::call_lifted(spec, args, ctx)
}

#[cfg(test)]
mod locale_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_consistent() {
        let mut seen = std::collections::HashSet::new();
        for s in all() {
            assert!(seen.insert(s.name), "duplicate function {}", s.name);
            assert_eq!(s.name, s.name.to_ascii_uppercase(), "{} not upper-case", s.name);
            assert!(s.min_args <= s.max_args, "{} arity", s.name);
            assert!(!s.description.is_empty() && !s.signature.is_empty(), "{} docs", s.name);
        }
        println!("registered functions: {}", all().len());
        assert!(all().len() >= 400);
        assert!(lookup("_xlfn._xlws.sort").is_some_and(|s| s.name == "SORT"));
        assert!(lookup("nosuchfn").is_none());
        for s in all() {
            assert!(lookup(&format!("_xlfn.{}", s.name.to_ascii_lowercase())).is_some_and(|x| x.name == s.name));
        }
    }

    #[test]
    fn hostile_inputs_never_panic() {
        use crate::util::testutil::*;
        use gridcraft_core::{Array, CellError};
        let mixed = Value::from(
            Array::new(2, 3, vec![Value::Number(1.0), Value::from("x"), Value::Empty, Value::Bool(true), Value::Number(-1e300), Value::Number(0.5)])
                .unwrap(),
        );
        let patterns: Vec<Arg> = vec![
            n(0.0),
            n(-1.0),
            n(1e308),
            n(-1e308),
            n(1e10),
            n(2.5),
            t(""),
            t("abc"),
            t("*?~"),
            b(true),
            e(CellError::NA),
            empty(),
            av(mixed.clone()),
            rf(mixed),
            rf(col(&[3.0, 1.0, 2.0])),
        ];
        for spec in all() {
            let max = spec.max_args.min(spec.min_args + 3);
            for count in spec.min_args..=max {
                for p in &patterns {
                    let args = vec![p.clone(); count];
                    let _ = call(spec, &args, &mut TestCtx::default());
                }
                // Mixed: cycle through patterns.
                let args: Vec<Arg> = (0..count).map(|i| patterns[(i * 7 + count) % patterns.len()].clone()).collect();
                let _ = call(spec, &args, &mut TestCtx::default());
            }
        }
    }
}
