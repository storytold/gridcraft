use super::Workbook;
use super::graph::{Graph, Key};
use gridcraft_formula::Expr;

/// Whether `e` draws random numbers itself: such formulas share one generator, so they are
/// calculated in order on the main thread (a draw inside a name's body is caught as it happens).
/// It reads nothing but the tree, so the graph keeps its answer per formula (`Node::random`).
pub(super) fn uses_random(e: &Expr) -> bool {
    random_use(e).calls
}

/// What a formula's tree shows of random draws (see [`random_use`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Draws {
    /// It calls a function that draws random numbers ([`uses_random`]).
    pub(crate) calls: bool,
    /// Evaluating it may draw one (`Node::may_draw`).
    pub(crate) may_draw: bool,
}

/// From one walk of `e`: whether it calls a function that draws random numbers
/// ([`uses_random`]), and whether evaluating it may draw any (`Node::may_draw`): also through a
/// function passed by name (`MAKEARRAY(2,2,RANDBETWEEN)`, a name in the tree that `uses_random`
/// doesn't look at). Any call or name with `RAND` in it, in any case, counts for the second: a
/// wider net costs one thread a plan, a narrower one could let one thread and several draw in
/// different orders.
pub(crate) fn random_use(e: &Expr) -> Draws {
    let (mut calls, mut may) = (false, false);
    e.walk(&mut |x| match x {
        // Only a name with `RAND` in it can be one of the three, so no other call is looked up.
        Expr::Call(n, _) if mentions_rand(n) => {
            calls |= call_draws(n);
            may = true;
        }
        Expr::Name(n) => may |= mentions_rand(n),
        _ => {}
    });
    Draws { calls, may_draw: calls || may }
}

/// Whether a call named `name`, as the parser leaves it, runs a built-in that draws random numbers
/// (RAND, RANDBETWEEN, RANDARRAY), by the registry's name for it: the registry strips the `_xlfn.`
/// and `_xlws.` prefixes the parser leaves (`_xlws._xlfn.RAND()` parses to `_XLFN.RAND` and runs
/// RAND). The planner's walk (`precedents_drawing`) and [`random_use`] both ask it.
pub(crate) fn call_draws(name: &str) -> bool {
    let function = gridcraft_functions::lookup(name).map_or(name, |f| f.name);
    matches!(function, "RAND" | "RANDBETWEEN" | "RANDARRAY")
}

/// Whether `text` holds `RAND` in any ASCII case: the three functions that draw random numbers
/// (RAND, RANDBETWEEN, RANDARRAY) are named so, with or without a prefix such as `_xlfn.`.
///
/// A pass can look through all the defined names' texts with it, and a definition can be long, so
/// only the bytes that are `R` or `r` are compared with `RAND`. They are found 32 at a time: a
/// fixed-size block is turned into a mask of its `R`s by a loop with no early exit, which the
/// compiler can turn into vector instructions, and the four bytes from each `R` in the mask are
/// then compared ignoring ASCII case: about 2 instructions per byte of formula text, against 12
/// for comparing at every byte; a text made mostly of `R`s costs more than comparing at every
/// byte, as each `R` is compared. A byte of a multi-byte UTF-8 character is never an ASCII
/// letter, so the answer is that of comparing every four bytes ignoring ASCII case.
pub(crate) fn mentions_rand(text: &str) -> bool {
    const BLOCK: usize = 32;
    let bytes = text.as_bytes();
    let is_rand = |w: &[u8]| w.eq_ignore_ascii_case(b"RAND");
    // Most texts it is given are a function's or a name's name in a tree: a text shorter than a
    // block is compared at each byte, which costs less than setting up the blocks.
    if bytes.len() < BLOCK {
        return bytes.windows(4).any(is_rand);
    }
    let rand_at = |at: usize| bytes.get(at..).and_then(|rest| rest.get(..4)).is_some_and(is_rand);
    let (blocks, tail) = bytes.as_chunks::<BLOCK>();
    let mut start = 0;
    for block in blocks {
        let mut rs = block.iter().enumerate().fold(0u32, |rs, (i, b)| rs | (u32::from(*b | 0x20 == b'r') << i));
        while rs != 0 {
            if rand_at(start + rs.trailing_zeros() as usize) {
                return true;
            }
            rs &= rs - 1;
        }
        start += BLOCK;
    }
    tail.iter().enumerate().any(|(i, b)| *b | 0x20 == b'r' && rand_at(start + i))
}

/// Whether evaluating a defined name of `wb` may draw a random number: a definition whose text
/// mentions one of the functions that draw ([`mentions_rand`]), parsed or not, whatever its
/// scope. A formula reaches a name's definition by its text alone (a name in its tree, or text
/// INDIRECT reads), so every name counts.
pub(super) fn names_may_draw(wb: &Workbook) -> bool {
    #[cfg(test)]
    NAME_TEXT_SCANS.with(|n| n.set(n.get().saturating_add(1)));
    wb.names.iter().any(|n| mentions_rand(&n.formula))
}

#[cfg(test)]
thread_local! {
    /// Calls of [`names_may_draw`] on this thread, which a test reads: `Calc::run` makes none
    /// while a formula of the graph may draw.
    pub(crate) static NAME_TEXT_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// [`uses_random`] for the formula at `k` evaluated as `e`: the flag its graph node keeps when the
/// node was built from this same tree, else a walk of `e`.
pub(super) fn draws_random(graph: Option<&Graph>, k: Key, e: &std::sync::Arc<Expr>) -> bool {
    match graph.and_then(|g| g.nodes.get(&k)) {
        Some(node) if node.formula.as_ref().and_then(|f| f.expr.as_ref()).is_some_and(|tree| std::sync::Arc::ptr_eq(tree, e)) => node.random,
        _ => uses_random(e),
    }
}
