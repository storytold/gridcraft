# Regular bond arithmetic

PRICE, YIELD, DURATION and MDURATION share cash-flow evaluation in
`crates/functions/src/financial/securities.rs`. Intermediate coupons and discounted payments use
`financial::scaled::Scaled`, which has binary64 precision and a separate binary exponent.
Only the final price, yield or duration is converted to a worksheet number.

This addresses a common numerical failure behind #4146–#4153. Evaluating an algebraic formula
entirely with ordinary `f64` intermediates can create infinity or zero before a later division
or ratio produces a representable result. The worksheet's finite-number guard correctly
rejects infinity and NaN; changing that guard cannot recover the lost value.

The shared calculation preserves:

- Coupons before frequency and discount scaling, and accrued interest before day-count scaling.
- Discounted redemption when the discount power would overflow.
- The common scale of present values and their time weights when calculating duration.
- Zero coupons without dividing zero by an underflowed power.
- Single-coupon yield arithmetic until its final day-count division.

Multi-coupon YIELD compares positive gross present values against price plus accrued interest.
It bisects `log(1 + yield/frequency)` up to the largest finite positive yield, avoiding the
previous 60-doubling ceiling. Iterations and coupon loops remain bounded. The established
negative-yield lower bound is retained; its compatibility policy is independent of extending
the positive search range.

This extends numerical range, not precision. Severe cancellation between nearby cash amounts
can still lose significant digits. Final results outside binary64 still return `#NUM!`;
results below its representable range round to zero. Dates, argument admission, array lifting,
and workbook recalculation do not change.

Extreme-value expectations use cash-flow identities and independently reordered arithmetic.
Native Excel is also inaccurate on several of these inputs, so agreement with native output
alone is insufficient. Regression tests exercise the public function registry in both date
systems and compare tiny nonzero results without an absolute tolerance floor.

Related interest-at-maturity functions have independent implementations. This patch does not
change ACCRINTM, PRICEMAT or YIELDMAT (#4095, #4097, #4099).
