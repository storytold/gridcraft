//! GridCraft actuarial extensions. Pure, deterministic worksheet functions (L1).
//!
//! `ACT.*` is a GridCraft extension, not an Excel-compatible function namespace.
//! See `docs/actuarial.md` for probability, timing and truncation conventions.

// Keep the operational wrapper inside L1: neither the registry nor the kernels import engine.
macro_rules! act {
    ($name:literal, $min:expr, $max:expr, $cat:ident, $scalar:expr, $sig:literal, $desc:literal, $imp:expr) => {
        crate::FnSpec {
            name: $name,
            min_args: $min,
            max_args: $max,
            category: crate::Category::$cat,
            volatile: false,
            scalar_args: $scalar,
            value_args: &[],
            signature: $sig,
            description: $desc,
            imp: {
                /// Checks the host's operational bank before allocating or entering an ACT kernel.
                fn bounded(a: &[crate::Arg], c: &mut dyn crate::Ctx) -> gridcraft_core::Value {
                    if let Err(error) = crate::actuarial::work::charge(c, crate::actuarial::work::estimate($name, a)) {
                        return gridcraft_core::Value::Error(error);
                    }
                    crate::util::finish(($imp)(a, c))
                }
                bounded
            },
        }
    };
}

mod bootstrap;
mod decision;
mod extreme;
mod inference;
mod life;
mod loss;
mod markov;
mod reinsurance;
mod reserving;
mod work;

use crate::FnSpec;

/// Apply ACT defaults to omitted slots and empty literals, retaining ordinary blank-reference coercion.
pub(super) fn optional_num(args: &[crate::Arg], index: usize, default: f64) -> crate::util::R<f64> {
    if crate::util::has(args, index) { crate::util::num(args, index) } else { Ok(default) }
}

/// Collect all deterministic actuarial worksheet specifications without introducing an engine dependency.
pub(crate) fn specs() -> Vec<FnSpec> {
    let mut result = life::specs();
    result.extend(loss::specs());
    result.extend(markov::specs());
    result.extend(inference::specs());
    result.extend(reinsurance::specs());
    result.extend(extreme::specs());
    result.extend(decision::specs());
    result.extend(bootstrap::specs());
    result.extend(reserving::specs());
    result
}
