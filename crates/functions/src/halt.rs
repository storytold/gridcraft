//! The hand-back rule and the one loop shape that keeps it (see [`Halt`]).

use gridcraft_core::{CellError, Value};

/// An evaluation that can be handed back: on a calculation thread, a read of a formula that isn't
/// done yet, or a random draw, hands the formula back to the main thread, which calculates it
/// again, so the result of the evaluation is thrown away whatever it comes to.
///
/// **The rule.** Once the evaluation is handed back it stops: no further reads of cells, no
/// further random draws and no further LAMBDA calls, and a loop that has not reserved its result
/// yet does not reserve it. Where nothing can be handed back (one thread, the main thread) nothing
/// is asked, and a loop asks nothing per item.
///
/// **What a new site must do.**
/// - A loop whose items can read, draw or call a LAMBDA goes through [`Gather`]: `grid` or
///   `collect` for a loop that builds a result (which it reserves for every item once the first
///   item has come and not handed the evaluation back, so a hand-back at a later item leaves that
///   reservation in place until the result is dropped), `each` for one that folds, sums or skips,
///   `each_kept` for one that keeps what an item gave only once it was asked about (a union's
///   row). The loop's body is a closure given the evaluation; `Gather` asks before the first item
///   (the evaluation may have been handed back before the loop) and after each item, and ends the
///   loop.
/// - In the evaluator (`gridcraft-calc`), a read of the host goes through the evaluator's read
///   methods (its `fence` module), which make no read once the evaluation was handed back and ask
///   after each read they make; the host's reads of cells are reachable only there (its random
///   draw, for a function's context, is asked about after the function's call). A function's call
///   of its LAMBDA through the context (GROUPBY, PIVOTBY) is refused there once handed back.
///
/// **Loops outside `Gather`.** Three loops whose items can read or call a LAMBDA don't go through
/// it, and each is stopped by the evaluation's own state instead. GROUPBY's and PIVOTBY's loops
/// over their groups call the LAMBDA through the context, which refuses every call once the
/// evaluation is handed back (the call gives `#CALC!` without being made), so the rest of the
/// loop reads and calls nothing. A database function's loop over its records with computed
/// criteria asks `Ctx::exhausted` before each record, criteria row and condition, and a
/// hand-back leaves the evaluation exhausted, so no further criterion is evaluated.
///
/// **What forces it.** Only this module can make an [`Asked`], so [`Halt::halted`] and
/// `Ctx::halted` can be called only by `Gather`; in the evaluator the host is behind a type whose
/// reads of cells only the `fence` module reaches. A test in that module evaluates every registered
/// function and the evaluator's own functions on a host that hands back at the first read or draw,
/// and fails on any read, draw or LAMBDA call made after it.
pub trait Halt {
    /// Whether [`Halt::halted`] can become true here at all (a calculation thread's evaluation),
    /// asked once before a loop.
    fn can_halt(&self) -> bool;
    /// Before a loop's first item and after each item's reads, draws or calls: whether the
    /// evaluation was handed back (it then stops, as when it runs out of work).
    fn halted(&mut self, asked: Asked) -> bool;
}

/// Made only by [`Gather`]: the halt question is asked through it (see [`Halt`]).
pub struct Asked(());

/// The loop stopped because the evaluation was handed back; its result is thrown away, and stands
/// as `#CALC!`.
pub struct Halted;

impl From<Halted> for CellError {
    fn from(_: Halted) -> Self {
        CellError::Calc
    }
}

impl From<Halted> for Value {
    fn from(_: Halted) -> Self {
        Value::Error(CellError::Calc)
    }
}

/// The one shape of a loop whose items can hand the evaluation back (see [`Halt`]). Where nothing
/// can be handed back each method runs its loop asking nothing and reserves the result ahead;
/// where it can, it asks before the first item and after each item, ends the loop with
/// [`Halted`] when it was handed back, and reserves the whole result once the first item has come
/// and was not.
#[derive(Clone, Copy, Debug)]
pub struct Gather {
    watch: bool,
}

// `grid` and `collect` run the plain loop where they are called, so that its body is compiled
// into the caller's loop as a loop written there would be, and the watched loop, which reserves
// its result only after the first item, as a function of its own, so that the item is called from
// one place in each. `each` and `each_kept` reserve nothing, so the watched and plain loops differ
// only in the questions, and each is one loop in which a question is a test of `watch` where it
// can't be asked.
impl Gather {
    /// For a loop of `h`'s evaluation (asks [`Halt::can_halt`] once).
    pub fn new<H: Halt + ?Sized>(h: &H) -> Gather {
        Gather { watch: h.can_halt() }
    }

    /// Whether `h` was handed back, asked before the first item and after each.
    #[inline]
    fn stops<H: Halt + ?Sized>(h: &mut H) -> bool {
        h.halted(Asked(()))
    }

    /// [`Gather::stops`] where it can be asked.
    #[inline(always)]
    fn watched_stops<H: Halt + ?Sized>(self, h: &mut H) -> bool {
        self.watch && Gather::stops(h)
    }

    /// One value for each of `rows` x `cols` positions, row-major: `item(h, row, col)`.
    #[inline(always)]
    pub fn grid<H, T, E>(
        self,
        h: &mut H,
        (rows, cols): (usize, usize),
        mut item: impl FnMut(&mut H, usize, usize) -> Result<T, E>,
    ) -> Result<Vec<T>, E>
    where
        H: Halt + ?Sized,
        E: From<Halted>,
    {
        if self.watch {
            return grid_watched(h, (rows, cols), item);
        }
        let mut out = Vec::with_capacity(rows.saturating_mul(cols));
        for r in 0..rows {
            for c in 0..cols {
                out.push(item(h, r, c)?);
            }
        }
        Ok(out)
    }

    /// One value for each of `items`, in order: `item(h, each)`.
    #[inline(always)]
    pub fn collect<H, I, T, E>(self, h: &mut H, items: I, mut item: impl FnMut(&mut H, I::Item) -> Result<T, E>) -> Result<Vec<T>, E>
    where
        H: Halt + ?Sized,
        I: ExactSizeIterator,
        E: From<Halted>,
    {
        if self.watch {
            return collect_watched(h, items, item);
        }
        let mut out = Vec::with_capacity(items.len());
        for each in items {
            out.push(item(h, each)?);
        }
        Ok(out)
    }

    /// A step for each of `items`, in order, whose outcome `keep` takes only once it was asked
    /// about, as `grid` and `collect` keep their values: `step(h, each)`, then `keep(h, outcome)`.
    #[inline(always)]
    pub fn each_kept<H, I, V, E>(
        self,
        h: &mut H,
        items: I,
        mut step: impl FnMut(&mut H, I::Item) -> V,
        mut keep: impl FnMut(&mut H, V),
    ) -> Result<(), E>
    where
        H: Halt + ?Sized,
        I: IntoIterator,
        E: From<Halted>,
    {
        if self.watched_stops(h) {
            return Err(Halted.into());
        }
        for each in items {
            let v = step(h, each);
            if self.watched_stops(h) {
                return Err(Halted.into());
            }
            keep(h, v);
        }
        Ok(())
    }

    /// A step for each of `items`, in order, keeping no result of its own: `step(h, each)`.
    #[inline(always)]
    pub fn each<H, I, E>(self, h: &mut H, items: I, mut step: impl FnMut(&mut H, I::Item) -> Result<(), E>) -> Result<(), E>
    where
        H: Halt + ?Sized,
        I: IntoIterator,
        E: From<Halted>,
    {
        if self.watched_stops(h) {
            return Err(Halted.into());
        }
        for each in items {
            step(h, each)?;
            if self.watched_stops(h) {
                return Err(Halted.into());
            }
        }
        Ok(())
    }
}

/// [`Gather::grid`] where the evaluation can be handed back.
#[inline(never)]
fn grid_watched<H, T, E>(h: &mut H, (rows, cols): (usize, usize), mut item: impl FnMut(&mut H, usize, usize) -> Result<T, E>) -> Result<Vec<T>, E>
where
    H: Halt + ?Sized,
    E: From<Halted>,
{
    if Gather::stops(h) {
        return Err(Halted.into());
    }
    let len = rows.saturating_mul(cols);
    let mut out = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let v = item(h, r, c)?;
            if Gather::stops(h) {
                return Err(Halted.into());
            }
            if out.is_empty() {
                out.reserve_exact(len);
            }
            out.push(v);
        }
    }
    Ok(out)
}

/// [`Gather::collect`] where the evaluation can be handed back.
#[inline(never)]
fn collect_watched<H, I, T, E>(h: &mut H, items: I, mut item: impl FnMut(&mut H, I::Item) -> Result<T, E>) -> Result<Vec<T>, E>
where
    H: Halt + ?Sized,
    I: ExactSizeIterator,
    E: From<Halted>,
{
    if Gather::stops(h) {
        return Err(Halted.into());
    }
    let len = items.len();
    let mut out = Vec::new();
    for each in items {
        let v = item(h, each)?;
        if Gather::stops(h) {
            return Err(Halted.into());
        }
        if out.is_empty() {
            out.reserve_exact(len);
        }
        out.push(v);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts the items it is asked about; handed back after `stop_at` of them (before the loop
    /// when 0, never when `None`).
    struct Counting {
        can: bool,
        items: usize,
        stop_at: Option<usize>,
    }
    impl Halt for Counting {
        fn can_halt(&self) -> bool {
            self.can
        }
        fn halted(&mut self, _: Asked) -> bool {
            self.stop_at.is_some_and(|at| self.items >= at)
        }
    }

    #[test]
    fn a_loop_stops_after_the_item_that_hands_back_and_reserves_only_after_one_that_did_not() {
        // Handed back before the loop (0): no item at all.
        for stop_at in [0, 1, 2, 5].map(Some) {
            let mut h = Counting { can: true, items: 0, stop_at };
            let out: Result<Vec<usize>, CellError> = Gather::new(&h).grid(&mut h, (3, 3), |h, r, c| {
                h.items += 1;
                Ok(r * 3 + c)
            });
            assert!(matches!(out, Err(CellError::Calc)), "{stop_at:?}");
            assert_eq!(Some(h.items), stop_at, "no item after the one that handed back");
            let mut h = Counting { can: true, items: 0, stop_at };
            let out: Result<Vec<usize>, CellError> = Gather::new(&h).collect(&mut h, 0..9, |h, i| {
                h.items += 1;
                Ok(i)
            });
            assert!(out.is_err());
            assert_eq!(Some(h.items), stop_at);
            let mut h = Counting { can: true, items: 0, stop_at };
            let out: Result<(), CellError> = Gather::new(&h).each(&mut h, 0..9, |h, _| {
                h.items += 1;
                Ok(())
            });
            assert!(out.is_err());
            assert_eq!(Some(h.items), stop_at);
            let mut h = Counting { can: true, items: 0, stop_at };
            let out: Result<(), CellError> = Gather::new(&h).each_kept(&mut h, 0..9, |h, _| h.items += 1, |_, ()| ());
            assert!(out.is_err());
            assert_eq!(Some(h.items), stop_at);
        }
        // Not handed back: every item, and the full result reserved once.
        let mut h = Counting { can: true, items: 0, stop_at: None };
        let out: Result<Vec<usize>, CellError> = Gather::new(&h).collect(&mut h, 0..9, |_, i| Ok(i));
        let out = out.unwrap_or_default();
        assert_eq!(out, (0..9).collect::<Vec<_>>());
        assert_eq!(out.capacity(), 9);
        // Where nothing can be handed back nothing is asked, even by a host that would say so.
        let mut h = Counting { can: false, items: 0, stop_at: Some(0) };
        let out: Result<Vec<usize>, CellError> = Gather::new(&h).grid(&mut h, (2, 2), |h, r, c| {
            h.items += 1;
            Ok(r + c)
        });
        assert_eq!(out.unwrap_or_default().len(), 4);
        assert_eq!(h.items, 4);
    }

    #[test]
    fn an_item_error_ends_the_loop_with_that_error() {
        let mut h = Counting { can: true, items: 0, stop_at: None };
        let out: Result<Vec<usize>, CellError> = Gather::new(&h).collect(&mut h, 0..9, |h, i| {
            h.items += 1;
            if i == 3 { Err(CellError::Num) } else { Ok(i) }
        });
        assert!(matches!(out, Err(CellError::Num)));
        assert_eq!(h.items, 4);
    }
}
