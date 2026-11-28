//! The evaluator's side of the hand-back rule (see [`Halt`]): the host is held as a [`Reader`],
//! whose reads of cells only this module reaches. The evaluator reads through the methods here,
//! which ask after each read and stop the evaluation when it was handed back; its loops over such
//! reads go through [`gridcraft_functions::Gather`], which asks through the [`Halt`] the evaluator
//! implements here. The host's random draw is not fenced the same way: [`Reader::random`] is open
//! to every module of the evaluator, and only `FnCtx::random` calls it, inside a function's call,
//! which `call_function` asks about after it.

use std::sync::Arc;

use gridcraft_core::{CellError, CellRef, RangeRef, Value};
use gridcraft_formula::Expr;
use gridcraft_functions::{Arg, Asked, FnSpec, Halt, Halted};
use gridcraft_model::Workbook;

use super::{Area, Ev, Evaluator, FnCtx, Host, Lambda, err};

/// The host as the evaluator holds it: everything but the reads of cells, which go through the
/// evaluator's methods below (see [`Halt`]), and a random draw for `FnCtx::random`.
pub struct Reader<'h>(&'h mut dyn Host);

impl<'h> Reader<'h> {
    #[inline]
    pub(super) fn new(host: &'h mut dyn Host) -> Self {
        Reader(host)
    }
    /// The same host for a nested evaluation (a computed criterion).
    #[inline]
    pub(super) fn reborrow(&mut self) -> Reader<'_> {
        Reader(&mut *self.0)
    }
    #[inline]
    pub(crate) fn workbook(&self) -> &Workbook {
        self.0.workbook()
    }
    #[inline]
    pub(super) fn now_serial(&self) -> f64 {
        self.0.now_serial()
    }
    #[inline]
    pub(super) fn can_halt(&self) -> bool {
        self.0.can_halt()
    }
    #[inline]
    pub(super) fn stack_top(&self) -> Option<usize> {
        self.0.stack_top()
    }
    #[inline]
    pub(super) fn stack_exhausted(&mut self) {
        self.0.stack_exhausted();
    }
    #[inline]
    pub(super) fn note_volatile(&mut self) {
        self.0.note_volatile();
    }
    #[inline]
    pub(super) fn name_position(&mut self, name: &str, scope_sheet: usize) -> Option<usize> {
        self.0.name_position(name, scope_sheet)
    }
    #[inline]
    pub(super) fn name_expr(&mut self, position: usize) -> Option<Arc<Expr>> {
        self.0.name_expr(position)
    }
    #[inline]
    pub(super) fn used_range(&mut self, sheet: usize) -> Option<RangeRef> {
        self.0.used_range(sheet)
    }
    /// A random draw, for `FnCtx::random` alone: a function that draws is called through
    /// [`Evaluator::call_function`], which asks after it, and its own loop of draws goes through
    /// `Gather`.
    #[inline]
    pub(super) fn random(&mut self) -> f64 {
        self.0.random()
    }
}

// Reads not made because the formula was handed back before them, on this thread, for tests: a
// loop that goes on after the hand-back makes them, one per item. And the LAMBDA calls the
// evaluation had left when the hand-back was first seen, so that a test can count those after it.
#[cfg(test)]
thread_local! {
    static SKIPPED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FUEL_AT_HAND_BACK: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

impl Evaluator<'_> {
    /// Asked of the host right after each step that can hand the formula back: a read, a
    /// volatile function's call, a function's draw (see [`Halt`]). Where it was, the evaluation
    /// then stops, as when it runs out of work. Nothing is asked of the host where it can't be
    /// (`can_halt`, asked once when the evaluator is made).
    fn stop_if_halted(&mut self) -> bool {
        let halted = self.can_halt && self.host.0.halted();
        if halted {
            #[cfg(test)]
            FUEL_AT_HAND_BACK.with(|f| f.set(Some(f.get().unwrap_or(self.fuel))));
            self.work = 0;
            self.exhausted = true;
        }
        halted
    }

    /// Whether the formula was handed back, before a read or after an item of a loop. Each step
    /// that can hand it back was asked about right after it (`stop_if_halted`), which then left
    /// the evaluation `exhausted` (a computed criterion's passes up to the caller), so until it
    /// is the host is not asked again.
    #[inline]
    fn handed_back(&mut self) -> bool {
        self.exhausted && self.stop_if_halted()
    }

    /// The one way the evaluator reads its host (see [`Halt`]): `read` is made, and asked about
    /// after, unless the formula was handed back before it, which then gives `skipped` (a value
    /// thrown away) without reading. A read that handed it back gives `skipped` too, whatever the
    /// host returned, so no value read for a formula handed back is passed on (a visit has passed
    /// its values to its closure by then). Where nothing can be handed back, `read` alone.
    #[inline(always)]
    fn read<T>(&mut self, skipped: T, read: impl FnOnce(&mut dyn Host) -> T) -> T {
        if !self.can_halt {
            return read(&mut *self.host.0);
        }
        if self.handed_back() {
            #[cfg(test)]
            SKIPPED.with(|n| n.set(n.get() + 1));
            return skipped;
        }
        let v = read(&mut *self.host.0);
        if self.stop_if_halted() {
            return skipped;
        }
        v
    }

    /// The current value of a cell (see [`Host::cell_value`]).
    #[inline]
    pub(crate) fn read_cell(&mut self, sheet: usize, c: CellRef) -> Value {
        self.read(Value::Error(CellError::Calc), |h| h.cell_value(sheet, c))
    }

    /// The values of a block, row-major (see [`Host::range_values`]).
    #[inline]
    pub(crate) fn read_values(&mut self, sheet: usize, range: RangeRef) -> Vec<Value> {
        self.read(Vec::new(), |h| h.range_values(sheet, range))
    }

    /// See [`Host::visit_range`]: once handed back, walked with nothing in it.
    #[inline]
    pub(super) fn read_visit(&mut self, sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
        self.read(true, |h| h.visit_range(sheet, range, f))
    }

    /// See [`Host::range_sum`].
    #[inline]
    pub(super) fn read_sum(&mut self, sheet: usize, range: RangeRef) -> Option<(f64, u64)> {
        self.read(None, |h| h.range_sum(sheet, range))
    }

    /// See [`Host::spill_range`].
    #[inline]
    pub(super) fn read_spill(&mut self, sheet: usize, anchor: CellRef) -> Option<RangeRef> {
        self.read(None, |h| h.spill_range(sheet, anchor))
    }

    /// Calls a built-in function with its evaluated arguments. A volatile function's draws can
    /// hand the formula back: asked after the call (a function drawing many asks in its own loop).
    #[inline]
    pub(super) fn call_function(&mut self, spec: &FnSpec, args: &[Arg], criteria: Option<Area>, lambda: Option<Arc<Lambda>>) -> Value {
        let mut ctx = FnCtx { ev: self, criteria, lambda };
        let v = gridcraft_functions::call(spec, args, &mut ctx);
        if spec.volatile {
            self.stop_if_halted();
        }
        v
    }

    /// For a function's loop through `Gather` (`Ctx::halted`): its draws are not asked about one
    /// by one, so the host is asked.
    pub(super) fn draws_halted(&mut self, _asked: Asked) -> bool {
        self.stop_if_halted()
    }

    /// Before a LAMBDA call that a function makes through its context (GROUPBY and PIVOTBY, one
    /// per group, from loops that don't go through `Gather`), and before a LAMBDA applied to its
    /// arguments (`apply_lambda`, and `tail_call` for a call in a body's tail position, which the
    /// loop in `call_lambda_values` makes in place of a nested one): whether the formula was
    /// handed back, so the call is not made.
    pub(super) fn refuses_calls(&mut self) -> bool {
        self.stop_if_halted()
    }
}

/// The evaluator's own loops: an item's reads, volatile calls and functions' loops of draws were
/// each asked about right after (see `handed_back`).
impl Halt for Evaluator<'_> {
    fn can_halt(&self) -> bool {
        self.can_halt
    }
    #[inline]
    fn halted(&mut self, _asked: Asked) -> bool {
        self.handed_back()
    }
}

/// A loop of the evaluator that `Gather` ended stands as `#CALC!` (its result is thrown away).
impl From<Halted> for Ev {
    fn from(_: Halted) -> Self {
        err(CellError::Calc)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{LAMBDA_FUEL, SPECIAL_FUNCTIONS};
    use super::*;

    /// A worker's host over a sheet of ones: the first read or draw hands the formula back. It
    /// notes every read, draw and volatile call asked of it after that.
    #[derive(Default)]
    struct HandingBack {
        wb: Workbook,
        halted: bool,
        after: Vec<&'static str>,
    }
    impl HandingBack {
        fn ask(&mut self, what: &'static str) {
            if self.halted {
                self.after.push(what);
            }
            self.halted = true;
        }
    }
    impl Host for HandingBack {
        fn workbook(&self) -> &Workbook {
            &self.wb
        }
        fn cell_value(&mut self, _sheet: usize, _c: CellRef) -> Value {
            self.ask("a cell");
            Value::number(1.0)
        }
        fn now_serial(&self) -> f64 {
            0.0
        }
        fn random(&mut self) -> f64 {
            self.ask("a draw");
            0.5
        }
        fn note_volatile(&mut self) {
            if self.halted {
                self.after.push("a volatile call");
            }
        }
        fn spill_range(&mut self, _sheet: usize, _anchor: CellRef) -> Option<RangeRef> {
            self.ask("a spill");
            None
        }
        fn range_values(&mut self, _sheet: usize, range: RangeRef) -> Vec<Value> {
            self.ask("a block");
            vec![Value::number(1.0); range.count() as usize]
        }
        fn visit_range(&mut self, _sheet: usize, range: RangeRef, f: &mut dyn FnMut(&Value) -> bool) -> bool {
            self.ask("a visit");
            for _ in 0..range.count() {
                if !f(&Value::number(1.0)) {
                    break;
                }
            }
            true
        }
        fn range_sum(&mut self, _sheet: usize, range: RangeRef) -> Option<(f64, u64)> {
            self.ask("a sum");
            Some((range.count() as f64, range.count()))
        }
        fn can_halt(&self) -> bool {
            true
        }
        fn halted(&self) -> bool {
            self.halted
        }
    }

    /// Arguments that read (a block, a cell, a union), give items to loop over (an array of
    /// positions), or call a LAMBDA whose body reads first (so any call after the first is one
    /// made after the hand-back), or a function by name that draws.
    const ARGUMENTS: &[&str] =
        &["A1:B3", "A1", "(A1,B2,C3)", "{1;2;3}", "{1,2;3,4}", "3", "LAMBDA(x,A1+x)", "LAMBDA(x,y,A1+x)", "RANDBETWEEN", "\"x\""];

    /// Reference constructs the evaluator reads by itself, outside any function, and loops of
    /// LAMBDA calls whose bodies read nothing, after an argument that reads (so every call would
    /// be one made after the hand-back), and LAMBDAs applied directly, or by a LET name, to an
    /// argument that reads before the call (`A1+0`: a bare `A1` is passed as a reference, read
    /// in the body), the LET name's lambda once already called before that argument, and the
    /// same calls in a LAMBDA body's tail position (`tail_call`: by a LET name, applied in place,
    /// in an IF branch and parentheses).
    const CONSTRUCTS: &[&str] = &[
        "A1:B3",
        "@A1:B3",
        "A1#",
        "(A1,B2,C3)",
        "-(A1,B2)",
        "A1:B3+A1",
        "A1:INDEX(A1:B3,{1;2},1)",
        "MAP(A1:B3,LAMBDA(item,item+1))",
        "MAP({1;2},A1:B2,LAMBDA(first,second,first+second))",
        "LET(step,LAMBDA(item,item+1),MAP(A1:B3,step))",
        "BYROW(A1:B3,LAMBDA(line,SUM(line)))",
        "BYCOL(A1:B3,LAMBDA(line,SUM(line)))",
        "SCAN(A1,{1;2;3},LAMBDA(total,item,total+item))",
        "REDUCE(A1,{1;2;3},LAMBDA(total,item,total+item))",
        "MAKEARRAY(A1+1,2,LAMBDA(down,across,down+across))",
        "GROUPBY(A1:A3,{1;2;3},LAMBDA(group,SUM(group)))",
        "PIVOTBY(A1:A3,{1;2;3},{1;2;3},LAMBDA(group,SUM(group)))",
        "LAMBDA(x,x+1)(A1+0)",
        "LET(step,LAMBDA(item,item+1),step(A1+0))",
        "LET(step,LAMBDA(item,item+1),step(1)+step(A1+0))",
        "LET(step,LAMBDA(item,item+1),LAMBDA(x,step(A1+x))(1))",
        "LAMBDA(x,LAMBDA(item,item+1)(A1+x))(1)",
        "LET(step,LAMBDA(item,item+1),LAMBDA(x,IF(x,(step(A1+x))))(1))",
    ];

    /// A read that hands the formula back passes on its skip value, not what the host returned
    /// (a spill range and a visit give the same either way on this host).
    #[test]
    fn a_read_that_hands_the_formula_back_passes_nothing_on() {
        let corner = CellRef::new(0, 0);
        let block = RangeRef::new(corner, CellRef::new(2, 1));
        let mut host = HandingBack::default();
        let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
        assert_eq!(ev.read_cell(0, corner), Value::Error(CellError::Calc), "a cell");
        let mut host = HandingBack::default();
        let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
        assert_eq!(ev.read_values(0, block), Vec::new(), "a block");
        let mut host = HandingBack::default();
        let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
        assert_eq!(ev.read_sum(0, block), None, "a sum");
    }

    /// The formulas the guard evaluates for `name`, called with `count` arguments: each argument of
    /// [`ARGUMENTS`] in every position, the others all `{1;2;3}` or all `3`.
    fn calls(name: &str, count: usize, out: &mut Vec<String>) {
        let call = |args: Vec<&str>| format!("{name}({})", args.join(","));
        for &arg in ARGUMENTS {
            out.push(call(vec![arg; count]));
            for at in 0..count {
                for rest in ["{1;2;3}", "3"] {
                    let mut args = vec![rest; count];
                    args[at] = arg;
                    out.push(call(args));
                }
            }
        }
    }

    /// The guard of the hand-back rule (see [`Halt`]): every function in the registry and every
    /// function the evaluator runs itself (`SPECIAL_FUNCTIONS`), called with the fewest arguments
    /// it takes, and one or two more where that stays within five, from [`ARGUMENTS`], and the
    /// [`CONSTRUCTS`], are
    /// evaluated on a host that hands the formula back at the first read or draw. None may read,
    /// draw or call a volatile function after it, nor make a LAMBDA call after it, nor try more
    /// than one read that the fence then doesn't make (a loop that went on would try one per item;
    /// a value already evaluated as a reference is read once more at the end).
    /// A loop or read added outside the fence fails here when these arguments reach it; one that
    /// only other arguments reach (a sheet name, a count past what is given here) is not seen.
    #[test]
    fn nothing_is_read_drawn_or_called_after_the_formula_is_handed_back() {
        let mut formulas: Vec<String> = CONSTRUCTS.iter().map(|c| (*c).to_string()).collect();
        for spec in gridcraft_functions::all() {
            let last = spec.max_args.min(spec.min_args + 2).min(spec.min_args.max(5));
            for count in spec.min_args..=last {
                calls(spec.name, count, &mut formulas);
            }
        }
        for name in SPECIAL_FUNCTIONS {
            for count in 0..=4 {
                calls(name, count, &mut formulas);
            }
        }
        let mut wrong = Vec::new();
        let mut handed_back = 0usize;
        let mut unparsed = 0usize;
        for text in &formulas {
            let Ok(expr) = gridcraft_formula::parse(text) else {
                unparsed += 1;
                continue;
            };
            let mut host = HandingBack::default();
            SKIPPED.with(|n| n.set(0));
            FUEL_AT_HAND_BACK.with(|f| f.set(None));
            let mut ev = Evaluator::new(&mut host, 0, CellRef::new(9, 9));
            let _ = ev.value(&expr);
            let lambda_calls = LAMBDA_FUEL - ev.fuel;
            let calls_after = FUEL_AT_HAND_BACK.with(|f| f.get()).map_or(0, |fuel| fuel - ev.fuel);
            let skipped = SKIPPED.with(|n| n.get());
            if host.halted {
                handed_back += 1;
                if !host.after.is_empty() || calls_after > 0 || skipped > 1 {
                    wrong.push(format!(
                        "{text}: after the hand-back {:?}, {skipped} reads not made, {calls_after} of {lambda_calls} LAMBDA calls",
                        host.after
                    ));
                }
            }
        }
        let evaluated = formulas.len() - unparsed;
        println!("{evaluated} formulas evaluated, {unparsed} built did not parse, {handed_back} handed back");
        assert!(wrong.is_empty(), "{} of {evaluated} formulas went on after the hand-back:\n{}", wrong.len(), wrong.join("\n"));
        // Every formula built parses (at least 53,000 of them), and about a quarter
        // of them read or draw (the rest are refused before, or read nothing).
        assert_eq!(unparsed, 0, "of {} formulas built", formulas.len());
        assert!(handed_back > evaluated / 5, "{handed_back} of {evaluated} formulas read or drew");
    }
}
