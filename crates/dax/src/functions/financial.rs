//! Financial functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! All financial functions are Excel-compatible scalar calculations. Functions that take a
//! `Basis` argument use it as the day-count convention: 0 or omitted is US (NASD) 30/360,
//! 1 is actual/actual, 2 is actual/360, 3 is actual/365 and 4 is European 30/360. Functions
//! that take `Type` use 0 or omitted for payments at the end of the period and 1 for payments
//! at the beginning. Settlement must precede maturity, and frequency must be 1, 2 or 4 where a
//! coupon frequency is accepted. These restrictions are documented rather than modelled as
//! parameter kinds, because they depend on values, not on expression shape.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec};

/// Flags for the non-iterator financial functions: Excel-compatible and subject to the usual
/// DirectQuery calculated-column restrictions.
const FINANCIAL: FunctionFlags = FunctionFlags::new(false, false, false, true, true);

/// `XIRR` and `XNPV` iterate a table and evaluate Values/Dates once per row.
const FINANCIAL_ITERATOR: FunctionFlags = FunctionFlags::new(true, false, false, true, true);

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "ACCRINT",
        FunctionCategory::Financial,
        6,
        8,
        &[
            required("Issue", ParamKind::Date),
            required("First_interest", ParamKind::Date),
            required("Settlement", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Par", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
            optional("Calc_method", ParamKind::Boolean),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Boolean,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Interest accrued on a periodically paying security, from issue or from the first coupon \
         date to settlement. Basis selects the day-count method; Calc_method chooses interest \
         from issue (TRUE, the default) or from first_interest (FALSE) when settlement is after \
         the first coupon date. Frequency must be 1, 2 or 4 and issue must precede \
         settlement.",
        &[
            "ACCRINT ( DATE ( 2021, 4, 1 ), DATE ( 2021, 10, 1 ), DATE ( 2022, 3, 15 ), 0.045, 5000, 2, 1 )",
            "ACCRINT ( DATE ( 2021, 4, 1 ), DATE ( 2021, 10, 1 ), DATE ( 2022, 3, 15 ), 0.045, 5000, 2, 1, FALSE )",
        ],
    ),
    spec(
        "ACCRINTM",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Issue", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Par", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Accrued interest of a security that pays interest only at maturity. Basis selects the \
         day-count method (0 or omitted is US 30/360). Issue must be earlier than maturity.",
        &[
            "ACCRINTM ( DATE ( 2022, 1, 10 ), DATE ( 2022, 7, 10 ), 0.04, 2500, 3 )",
            "ACCRINTM ( DATE ( 2022, 1, 10 ), DATE ( 2022, 7, 10 ), 0.04, 2500 )",
        ],
    ),
    spec(
        "AMORDEGRC",
        FunctionCategory::Financial,
        6,
        7,
        &[
            required("Cost", ParamKind::Number),
            required("Date_purchased", ParamKind::Date),
            required("First_period", ParamKind::Date),
            required("Salvage", ParamKind::Number),
            required("Period", ParamKind::Number),
            required("Rate", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[
                ParamKind::Number,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Depreciation of an asset for one accounting period under the French accounting system, \
         with a coefficient derived from the asset life (1/Rate). A purchase in the middle of a \
         period is prorated. Basis accepts only 0, 1, 3 or 4; period 0 is the first, prorated \
         period.",
        &[
            "AMORDEGRC ( 12500, DATE ( 2022, 9, 14 ), DATE ( 2022, 12, 31 ), 1500, 0, 0.12, 1 )",
            "AMORDEGRC ( 12500, DATE ( 2022, 9, 14 ), DATE ( 2022, 12, 31 ), 1500, 1, 0.12 )",
        ],
    ),
    spec(
        "AMORLINC",
        FunctionCategory::Financial,
        6,
        7,
        &[
            required("Cost", ParamKind::Number),
            required("Date_purchased", ParamKind::Date),
            required("First_period", ParamKind::Date),
            required("Salvage", ParamKind::Number),
            required("Period", ParamKind::Number),
            required("Rate", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[
                ParamKind::Number,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Straight-line depreciation for one accounting period under the French accounting system, \
         prorating the period of purchase. Unlike AMORDEGRC it applies no life-dependent \
         coefficient. Basis selects the day-count method.",
        &[
            "AMORLINC ( 12500, DATE ( 2022, 9, 14 ), DATE ( 2022, 12, 31 ), 1500, 0, 0.12, 1 )",
            "AMORLINC ( 12500, DATE ( 2022, 9, 14 ), DATE ( 2022, 12, 31 ), 1500, 1, 0.12 )",
        ],
    ),
    spec(
        "COUPDAYBS",
        FunctionCategory::Financial,
        3,
        4,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Integer,
        FINANCIAL,
        "Days elapsed since the coupon period opened, up to settlement. Frequency must be 1, 2 \
         or 4; Basis selects the day-count method, so the result depends on the convention rather \
         than on the calendar alone.",
        &["COUPDAYBS ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2, 1 )", "COUPDAYBS ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2 )"],
    ),
    spec(
        "COUPDAYS",
        FunctionCategory::Financial,
        3,
        4,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Length of the coupon period around settlement, counted in days under the Basis \
         convention; the result may be fractional rather than a calendar count.",
        &["COUPDAYS ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2, 1 )", "COUPDAYS ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2 )"],
    ),
    spec(
        "COUPDAYSNC",
        FunctionCategory::Financial,
        3,
        4,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Integer,
        FINANCIAL,
        "Days left from settlement to the following coupon date. Frequency must be 1, 2 or 4 and \
         Basis picks the day-count method.",
        &["COUPDAYSNC ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2, 1 )", "COUPDAYSNC ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2 )"],
    ),
    spec(
        "COUPNCD",
        FunctionCategory::Financial,
        3,
        4,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::DateTime,
        FINANCIAL,
        "Date of the coupon that follows settlement, derived from maturity and frequency; Basis \
         affects only the day-count arithmetic that locates it.",
        &["COUPNCD ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2, 1 )", "COUPNCD ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2 )"],
    ),
    spec(
        "COUPNUM",
        FunctionCategory::Financial,
        3,
        4,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Integer,
        FINANCIAL,
        "Coupon payments remaining between settlement and maturity, rounded up to a whole \
         coupon. Frequency must be 1, 2 or 4 and Basis selects the day-count method.",
        &["COUPNUM ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2, 1 )", "COUPNUM ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2 )"],
    ),
    spec(
        "COUPPCD",
        FunctionCategory::Financial,
        3,
        4,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::DateTime,
        FINANCIAL,
        "Date of the coupon immediately before settlement, derived from maturity and frequency; \
         Basis selects the day-count method used in the arithmetic.",
        &["COUPPCD ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2, 1 )", "COUPPCD ( DATE ( 2023, 2, 20 ), DATE ( 2028, 9, 30 ), 2 )"],
    ),
    spec(
        "CUMIPMT",
        FunctionCategory::Financial,
        6,
        6,
        &[
            required("Rate", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pv", ParamKind::Number),
            required("Start_period", ParamKind::Number),
            required("End_period", ParamKind::Number),
            required("Type", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Cumulative interest paid between Start_period and End_period of a loan with constant \
         payments and a constant interest rate. Periods are 1-based, Type is 0 (end) or 1 \
         (beginning) and every argument is required.",
        &["CUMIPMT ( 0.045 / 12, 15 * 12, 250000, 13, 24, 0 )"],
    ),
    spec(
        "CUMPRINC",
        FunctionCategory::Financial,
        6,
        6,
        &[
            required("Rate", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pv", ParamKind::Number),
            required("Start_period", ParamKind::Number),
            required("End_period", ParamKind::Number),
            required("Type", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Cumulative principal paid between Start_period and End_period of a loan with constant \
         payments and a constant interest rate. Periods are 1-based, Type is 0 (end) or 1 \
         (beginning) and every argument is required.",
        &["CUMPRINC ( 0.045 / 12, 15 * 12, 250000, 13, 24, 0 )"],
    ),
    spec(
        "DB",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Cost", ParamKind::Number),
            required("Salvage", ParamKind::Number),
            required("Life", ParamKind::Number),
            required("Period", ParamKind::Number),
            optional("Month", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Depreciation for one period under fixed-declining balance. Month sets how many months \
         the first year holds (12 when omitted), so a partial first year changes every later \
         period's depreciation.",
        &["DB ( 45000, 5000, 8, 1, 9 )", "DB ( 45000, 5000, 8, 2 )"],
    ),
    spec(
        "DDB",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Cost", ParamKind::Number),
            required("Salvage", ParamKind::Number),
            required("Life", ParamKind::Number),
            required("Period", ParamKind::Number),
            optional("Factor", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Depreciation for one period under double-declining balance, or under another \
         accelerating factor when Factor is supplied (2 when omitted). Depreciation stops at the \
         salvage value.",
        &["DDB ( 45000, 5000, 8, 1, 2 )", "DDB ( 45000, 5000, 8, 2 )"],
    ),
    spec(
        "DISC",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Pr", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Discount rate of a security bought at Pr per 100 of face value and redeemed at \
         Redemption. Basis selects the day-count method used to annualise the discount.",
        &["DISC ( DATE ( 2022, 2, 10 ), DATE ( 2022, 8, 20 ), 97.5, 100, 1 )", "DISC ( DATE ( 2022, 2, 10 ), DATE ( 2022, 8, 20 ), 97.5, 100 )"],
    ),
    spec(
        "DOLLARDE",
        FunctionCategory::Financial,
        2,
        2,
        &[required("Fractional_dollar", ParamKind::Number), required("Fraction", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Converts a price quoted with a fractional part, such as 1.02 for 1 and 2/16, into a \
         decimal price. Fraction is the denominator of the fractional part and is truncated to \
         an integer; a fraction below 1 returns an error.",
        &["DOLLARDE ( 1.05, 8 )", "DOLLARDE ( 1.25, 16 )"],
    ),
    spec(
        "DOLLARFR",
        FunctionCategory::Financial,
        2,
        2,
        &[required("Decimal_dollar", ParamKind::Number), required("Fraction", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Converts a decimal price into a price quoted with a fractional part, the inverse of \
         DOLLARDE. Fraction is the denominator of the fractional part and is truncated to an \
         integer; a fraction below 1 returns an error.",
        &["DOLLARFR ( 1.375, 8 )", "DOLLARFR ( 1.1875, 16 )"],
    ),
    spec(
        "DURATION",
        FunctionCategory::Financial,
        5,
        6,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Coupon", ParamKind::Number),
            required("Yld", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Macaulay duration of a bond with an assumed par value of 100: the present-value \
         weighted average time of its cash flows, in years. Frequency must be 1, 2 or 4 and \
         Basis selects the day-count method.",
        &[
            "DURATION ( DATE ( 2023, 3, 1 ), DATE ( 2033, 3, 1 ), 0.06, 0.065, 2, 1 )",
            "DURATION ( DATE ( 2023, 3, 1 ), DATE ( 2033, 3, 1 ), 0.06, 0.065, 2 )",
        ],
    ),
    spec(
        "EFFECT",
        FunctionCategory::Financial,
        2,
        2,
        &[required("Nominal_rate", ParamKind::Number), required("Npery", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Effective annual interest rate for a nominal annual rate compounded Npery times per \
         year. Npery is truncated to an integer; both arguments must be positive.",
        &["EFFECT ( 0.045, 12 )", "EFFECT ( 0.08, 4 )"],
    ),
    spec(
        "FV",
        FunctionCategory::Financial,
        3,
        5,
        &[
            required("Rate", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pmt", ParamKind::Number),
            optional("Pv", ParamKind::Number),
            optional("Type", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Future value of an investment with constant periodic payments at a constant rate. Pv is \
         the present value (0 when omitted) and Type is 0 for payments at the end of the period \
         or 1 for payments at the beginning.",
        &["FV ( 0.05 / 12, 36, -150, -1200, 1 )", "FV ( 0.05 / 12, 36, -150 )"],
    ),
    spec(
        "INTRATE",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Investment", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Interest rate earned by a fully invested security bought at Investment and redeemed at \
         Redemption. The rate is annualised with the Basis day-count convention.",
        &[
            "INTRATE ( DATE ( 2022, 3, 1 ), DATE ( 2022, 6, 1 ), 250000, 253500, 2 )",
            "INTRATE ( DATE ( 2022, 3, 1 ), DATE ( 2022, 6, 1 ), 250000, 253500 )",
        ],
    ),
    spec(
        "IPMT",
        FunctionCategory::Financial,
        4,
        6,
        &[
            required("Rate", ParamKind::Number),
            required("Per", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pv", ParamKind::Number),
            optional("Fv", ParamKind::Number),
            optional("Type", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Interest portion of the payment in a single period Per of a loan or investment with \
         level payments at a fixed rate. Per is 1-based and must not exceed Nper; Type is 0 \
         (end) or 1 (beginning).",
        &["IPMT ( 0.06 / 12, 1, 60, 25000, 0, 1 )", "IPMT ( 0.06 / 12, 1, 60, 25000 )"],
    ),
    spec(
        "ISPMT",
        FunctionCategory::Financial,
        4,
        4,
        &[
            required("Rate", ParamKind::Number),
            required("Per", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pv", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Interest paid or received in period Per for a schedule whose principal repayments are \
         equal, rather than one with constant total payments. Per and Nper use the same period \
         unit, and Per is limited by Nper.",
        &["ISPMT ( 0.06 / 12, 1, 60, 25000 )"],
    ),
    spec(
        "MDURATION",
        FunctionCategory::Financial,
        5,
        6,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Coupon", ParamKind::Number),
            required("Yld", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Modified Macaulay duration of a bond priced at par 100: the Macaulay duration divided \
         by 1 + Yld/Frequency. Frequency must be 1, 2 or 4 and Basis selects the day-count \
         method.",
        &[
            "MDURATION ( DATE ( 2023, 3, 1 ), DATE ( 2033, 3, 1 ), 0.06, 0.065, 2, 1 )",
            "MDURATION ( DATE ( 2023, 3, 1 ), DATE ( 2033, 3, 1 ), 0.06, 0.065, 2 )",
        ],
    ),
    spec(
        "NOMINAL",
        FunctionCategory::Financial,
        2,
        2,
        &[required("Effect_rate", ParamKind::Number), required("Npery", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Nominal annual interest rate that produces the given effective rate when compounded \
         Npery times per year. Npery is truncated to an integer; both arguments must be positive.",
        &["NOMINAL ( 0.046, 12 )", "NOMINAL ( 0.08, 4 )"],
    ),
    spec(
        "NPER",
        FunctionCategory::Financial,
        3,
        5,
        &[
            required("Rate", ParamKind::Number),
            required("Pmt", ParamKind::Number),
            required("Pv", ParamKind::Number),
            optional("Fv", ParamKind::Number),
            optional("Type", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Number of periods required for an investment with constant payments at a constant rate \
         to reach Fv. Pv and Fv use opposite signs by convention; Type is 0 (end) or 1 \
         (beginning) and defaults to 0.",
        &["NPER ( 0.06 / 12, -250, -8000, 20000, 1 )", "NPER ( 0.06 / 12, -250, -8000 )"],
    ),
    spec(
        "ODDFPRICE",
        FunctionCategory::Financial,
        8,
        9,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Issue", ParamKind::Date),
            required("First_coupon", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Yld", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Price per 100 of face value for a security whose first coupon period is irregular, \
         whether short, long or odd. Issue and First_coupon define that period; Frequency must \
         be 1, 2 or 4 and Basis selects the day-count method.",
        &[
            "ODDFPRICE ( DATE ( 2022, 7, 15 ), DATE ( 2030, 5, 1 ), DATE ( 2022, 6, 1 ), DATE ( 2022, 11, 1 ), 0.045, 0.05, 100, 2, 1 )",
            "ODDFPRICE ( DATE ( 2022, 7, 15 ), DATE ( 2030, 5, 1 ), DATE ( 2022, 6, 1 ), DATE ( 2022, 11, 1 ), 0.045, 0.05, 100, 2 )",
        ],
    ),
    spec(
        "ODDFYIELD",
        FunctionCategory::Financial,
        8,
        9,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Issue", ParamKind::Date),
            required("First_coupon", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Pr", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Yield of a security whose first coupon period is irregular, whether short, long or odd. \
         Pr is the price per 100 of face value; the calculation is iterative, and Basis selects \
         the day-count method.",
        &[
            "ODDFYIELD ( DATE ( 2022, 7, 15 ), DATE ( 2030, 5, 1 ), DATE ( 2022, 6, 1 ), DATE ( 2022, 11, 1 ), 0.04, 96.5, 100, 2, 0 )",
            "ODDFYIELD ( DATE ( 2022, 7, 15 ), DATE ( 2030, 5, 1 ), DATE ( 2022, 6, 1 ), DATE ( 2022, 11, 1 ), 0.04, 96.5, 100, 2 )",
        ],
    ),
    spec(
        "ODDLPRICE",
        FunctionCategory::Financial,
        7,
        8,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Last_interest", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Yld", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Price per 100 of face value for a security whose last coupon period is irregular, \
         whether short, long or odd. Last_interest starts that period; Frequency must be 1, 2 or \
         4 and Basis selects the day-count method.",
        &[
            "ODDLPRICE ( DATE ( 2022, 3, 10 ), DATE ( 2022, 7, 31 ), DATE ( 2021, 11, 30 ), 0.035, 0.038, 100, 2, 0 )",
            "ODDLPRICE ( DATE ( 2022, 3, 10 ), DATE ( 2022, 7, 31 ), DATE ( 2021, 11, 30 ), 0.035, 0.038, 100, 2 )",
        ],
    ),
    spec(
        "ODDLYIELD",
        FunctionCategory::Financial,
        7,
        8,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Last_interest", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Pr", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Yield of a security whose last coupon period is irregular, whether short, long or odd. \
         Pr is the price per 100 of face value; Frequency must be 1, 2 or 4 and Basis selects the \
         day-count method.",
        &[
            "ODDLYIELD ( DATE ( 2022, 4, 12 ), DATE ( 2022, 7, 31 ), DATE ( 2021, 12, 31 ), 0.035, 98.75, 100, 2, 0 )",
            "ODDLYIELD ( DATE ( 2022, 4, 12 ), DATE ( 2022, 7, 31 ), DATE ( 2021, 12, 31 ), 0.035, 98.75, 100, 2 )",
        ],
    ),
    spec(
        "PDURATION",
        FunctionCategory::Financial,
        3,
        3,
        &[required("Rate", ParamKind::Number), required("Pv", ParamKind::Number), required("Fv", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Number of periods an investment at a constant Rate needs to grow from Pv to Fv. Rate, \
         Pv and Fv must be positive; the result is not rounded to whole periods.",
        &["PDURATION ( 0.03, 1500, 1800 )", "PDURATION ( 0.08, 500, 1500 )"],
    ),
    spec(
        "PMT",
        FunctionCategory::Financial,
        3,
        5,
        &[
            required("Rate", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pv", ParamKind::Number),
            optional("Fv", ParamKind::Number),
            optional("Type", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Constant payment for a loan or investment at a constant Rate over Nper periods. Fv is \
         the balance after the last payment (0 when omitted) and Type is 0 (end) or 1 \
         (beginning).",
        &["PMT ( 0.06 / 12, 12, 12000, 0, 1 )", "PMT ( 0.06 / 12, 12, 12000 )"],
    ),
    spec(
        "PPMT",
        FunctionCategory::Financial,
        4,
        6,
        &[
            required("Rate", ParamKind::Number),
            required("Per", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pv", ParamKind::Number),
            optional("Fv", ParamKind::Number),
            optional("Type", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Principal portion of the payment in a single period Per of a loan or investment with \
         level payments at a fixed rate. Per is 1-based and must not exceed Nper; Type is 0 \
         (end) or 1 (beginning).",
        &["PPMT ( 0.06 / 12, 1, 48, 18000, 0, 1 )", "PPMT ( 0.06 / 12, 1, 48, 18000 )"],
    ),
    spec(
        "PRICE",
        FunctionCategory::Financial,
        6,
        7,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Yld", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Clean price per 100 of face value for a periodically paying security quoted from yield \
         Yld. Frequency must be 1, 2 or 4 and Basis selects the day-count method used to accrue \
         the first coupon.",
        &[
            "PRICE ( DATE ( 2022, 3, 10 ), DATE ( 2031, 10, 25 ), 0.0425, 0.05, 100, 2, 0 )",
            "PRICE ( DATE ( 2022, 3, 10 ), DATE ( 2031, 10, 25 ), 0.0425, 0.05, 100, 2 )",
        ],
    ),
    spec(
        "PRICEDISC",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Discount", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Price per 100 of face value for a discount instrument such as a Treasury bill, derived \
         from its annual Discount rate. Basis selects the day-count method used to prorate the \
         discount.",
        &[
            "PRICEDISC ( DATE ( 2022, 3, 1 ), DATE ( 2022, 4, 15 ), 0.04, 100, 2 )",
            "PRICEDISC ( DATE ( 2022, 3, 1 ), DATE ( 2022, 4, 15 ), 0.04, 100 )",
        ],
    ),
    spec(
        "PRICEMAT",
        FunctionCategory::Financial,
        5,
        6,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Issue", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Yld", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Price per 100 of face value for a security whose only interest payment falls at \
         maturity. Issue, Rate and the yield Yld determine the price; Basis selects the \
         day-count method.",
        &[
            "PRICEMAT ( DATE ( 2022, 2, 15 ), DATE ( 2022, 5, 12 ), DATE ( 2021, 10, 9 ), 0.05, 0.052, 0 )",
            "PRICEMAT ( DATE ( 2022, 2, 15 ), DATE ( 2022, 5, 12 ), DATE ( 2021, 10, 9 ), 0.05, 0.052 )",
        ],
    ),
    spec(
        "PV",
        FunctionCategory::Financial,
        3,
        5,
        &[
            required("Rate", ParamKind::Number),
            required("Nper", ParamKind::Number),
            required("Pmt", ParamKind::Number),
            optional("Fv", ParamKind::Number),
            optional("Type", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Present value of a loan or investment with constant payments at a constant Rate. Fv is \
         the cash balance after the last payment (0 when omitted) and Type is 0 (end) or 1 \
         (beginning).",
        &["PV ( 0.06 / 12, 15 * 12, 350, 0, 1 )", "PV ( 0.06 / 12, 15 * 12, 350 )"],
    ),
    spec(
        "RATE",
        FunctionCategory::Financial,
        3,
        6,
        &[
            required("Nper", ParamKind::Number),
            required("Pmt", ParamKind::Number),
            required("Pv", ParamKind::Number),
            optional("Fv", ParamKind::Number),
            optional("Type", ParamKind::Number),
            optional("Guess", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Per-period interest rate of an annuity. RATE solves iteratively from Guess (0.1 when \
         omitted) and returns an error when it does not converge within 20 iterations to \
         0.0000001; several rates can satisfy the same cash flows.",
        &["RATE ( 6 * 12, -300, 20000, 0, 1, 0.02 )", "RATE ( 6 * 12, -300, 20000 )"],
    ),
    spec(
        "RECEIVED",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Investment", ParamKind::Number),
            required("Discount", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Cash paid at maturity by a fully invested security bought at Investment with an annual \
         Discount rate. Basis selects the day-count method used to prorate the discount.",
        &[
            "RECEIVED ( DATE ( 2022, 4, 1 ), DATE ( 2022, 7, 1 ), 500000, 0.045, 2 )",
            "RECEIVED ( DATE ( 2022, 4, 1 ), DATE ( 2022, 7, 1 ), 500000, 0.045 )",
        ],
    ),
    spec(
        "RRI",
        FunctionCategory::Financial,
        3,
        3,
        &[required("Nper", ParamKind::Number), required("Pv", ParamKind::Number), required("Fv", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Equivalent compound interest rate per period for an investment growing from Pv to Fv over \
         Nper periods. Pv and Fv must be positive and Nper must be at least 1.",
        &["RRI ( 60, 5000, 7500 )", "RRI ( 8, 800, 1600 )"],
    ),
    spec(
        "SLN",
        FunctionCategory::Financial,
        3,
        3,
        &[required("Cost", ParamKind::Number), required("Salvage", ParamKind::Number), required("Life", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Straight-line depreciation for a single period: (Cost - Salvage) / Life. Life is the \
         number of periods, not a date; the function performs no day-count adjustment.",
        &["SLN ( 45000, 9000, 8 )", "SLN ( 2000, 200, 4 )"],
    ),
    spec(
        "SYD",
        FunctionCategory::Financial,
        4,
        4,
        &[
            required("Cost", ParamKind::Number),
            required("Salvage", ParamKind::Number),
            required("Life", ParamKind::Number),
            required("Per", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Sum-of-years' digits depreciation for period Per. Life and Per must be expressed in the \
         same periods and Per must not exceed Life; there is no day-count argument.",
        &["SYD ( 45000, 9000, 8, 1 )", "SYD ( 45000, 9000, 8, 8 )"],
    ),
    spec(
        "TBILLEQ",
        FunctionCategory::Financial,
        3,
        3,
        &[required("Settlement", ParamKind::Date), required("Maturity", ParamKind::Date), required("Discount", ParamKind::Number)],
        &[overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Bond-equivalent yield of a Treasury bill from its Discount rate. The function uses the \
         actual/360 day count and requires maturity within one year of settlement.",
        &["TBILLEQ ( DATE ( 2022, 4, 1 ), DATE ( 2022, 6, 15 ), 0.085 )", "TBILLEQ ( DATE ( 2022, 4, 1 ), DATE ( 2022, 6, 15 ), 0.08 )"],
    ),
    spec(
        "TBILLPRICE",
        FunctionCategory::Financial,
        3,
        3,
        &[required("Settlement", ParamKind::Date), required("Maturity", ParamKind::Date), required("Discount", ParamKind::Number)],
        &[overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Price per 100 of face value of a Treasury bill from its Discount rate. The function uses \
         the actual/360 day count and requires maturity within one year of settlement.",
        &["TBILLPRICE ( DATE ( 2022, 4, 1 ), DATE ( 2022, 6, 15 ), 0.08 )", "TBILLPRICE ( DATE ( 2022, 4, 1 ), DATE ( 2022, 6, 15 ), 0.085 )"],
    ),
    spec(
        "TBILLYIELD",
        FunctionCategory::Financial,
        3,
        3,
        &[required("Settlement", ParamKind::Date), required("Maturity", ParamKind::Date), required("Pr", ParamKind::Number)],
        &[overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL,
        "Yield of a Treasury bill from its price Pr per 100 of face value. The function uses the \
         actual/360 day count and requires maturity within one year of settlement.",
        &["TBILLYIELD ( DATE ( 2022, 4, 1 ), DATE ( 2022, 6, 15 ), 97.85 )", "TBILLYIELD ( DATE ( 2022, 4, 1 ), DATE ( 2022, 6, 15 ), 97.9 )"],
    ),
    spec(
        "VDB",
        FunctionCategory::Financial,
        5,
        7,
        &[
            required("Cost", ParamKind::Number),
            required("Salvage", ParamKind::Number),
            required("Life", ParamKind::Number),
            required("Start_period", ParamKind::Number),
            required("End_period", ParamKind::Number),
            optional("Factor", ParamKind::Number),
            optional("No_switch", ParamKind::Boolean),
        ],
        &[
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Boolean,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Variable declining-balance depreciation between Start_period and End_period, including \
         partial periods. Factor defaults to 2; No_switch TRUE prevents the switch to \
         straight-line depreciation when that becomes larger.",
        &["VDB ( 4800, 600, 8, 0, 1.75, 1.5 )", "VDB ( 4800, 600, 8, 0, 2 )"],
    ),
    spec(
        "XIRR",
        FunctionCategory::Financial,
        3,
        5,
        &[
            required("Table", ParamKind::Table),
            required("Values", ParamKind::Expression),
            required("Dates", ParamKind::Expression),
            optional("Guess", ParamKind::Number),
            optional("AlternateResult", ParamKind::Any),
        ],
        &[
            overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Expression]),
            overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Expression, ParamKind::Number]),
            overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Expression, ParamKind::Number, ParamKind::Any]),
        ],
        ReturnType::Decimal,
        FINANCIAL_ITERATOR,
        "Discount rate that drives an irregular schedule of cash flows to zero present value. \
         Values and Dates are paired once per Table row, the first date anchors the schedule and \
         the result is annualised. The rate is found iteratively from Guess, so it can fail to \
         converge; AlternateResult replaces the error when supplied.",
        &["XIRR ( CashFlows, CashFlows[Amount], CashFlows[Date], 0.1 )", "XIRR ( CashFlows, CashFlows[Amount], CashFlows[Date] )"],
    ),
    spec(
        "XNPV",
        FunctionCategory::Financial,
        4,
        4,
        &[
            required("Table", ParamKind::Table),
            required("Values", ParamKind::Expression),
            required("Dates", ParamKind::Expression),
            required("Rate", ParamKind::Number),
        ],
        &[overload(&[ParamKind::Table, ParamKind::Expression, ParamKind::Expression, ParamKind::Number])],
        ReturnType::Decimal,
        FINANCIAL_ITERATOR,
        "Present value of an irregular schedule of cash flows discounted at Rate. Values and \
         Dates are paired once per Table row and discounted with actual/365 day counts from the \
         earliest date, so Rate is an annual effective rate.",
        &["XNPV ( CashFlows, CashFlows[Amount], CashFlows[Date], 0.09 )", "XNPV ( CashFlows, CashFlows[Amount], CashFlows[Date], 0.1 )"],
    ),
    spec(
        "YIELD",
        FunctionCategory::Financial,
        6,
        7,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Pr", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            required("Frequency", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
            overload(&[
                ParamKind::Date,
                ParamKind::Date,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
                ParamKind::Number,
            ]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Yield of a bond that pays periodic interest, given its price Pr per 100 of face value. \
         The calculation is iterative; Frequency must be 1, 2 or 4 and Basis selects the \
         day-count method.",
        &[
            "YIELD ( DATE ( 2022, 3, 1 ), DATE ( 2030, 12, 15 ), 0.045, 92.15, 100, 2, 0 )",
            "YIELD ( DATE ( 2022, 3, 1 ), DATE ( 2030, 12, 15 ), 0.045, 92.15, 100, 2 )",
        ],
    ),
    spec(
        "YIELDDISC",
        FunctionCategory::Financial,
        4,
        5,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Pr", ParamKind::Number),
            required("Redemption", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Annual yield of a discounted security bought at Pr and redeemed at Redemption. Basis \
         selects the day-count method used to annualise the return.",
        &[
            "YIELDDISC ( DATE ( 2022, 3, 1 ), DATE ( 2022, 4, 20 ), 98.75, 100, 2 )",
            "YIELDDISC ( DATE ( 2022, 3, 1 ), DATE ( 2022, 4, 20 ), 98.75, 100 )",
        ],
    ),
    spec(
        "YIELDMAT",
        FunctionCategory::Financial,
        5,
        6,
        &[
            required("Settlement", ParamKind::Date),
            required("Maturity", ParamKind::Date),
            required("Issue", ParamKind::Date),
            required("Rate", ParamKind::Number),
            required("Pr", ParamKind::Number),
            optional("Basis", ParamKind::Number),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Number, ParamKind::Number]),
        ],
        ReturnType::Decimal,
        FINANCIAL,
        "Annual yield of a security whose only interest payment falls at maturity, bought at Pr \
         per 100 of face value. Issue and Rate define the interest; Basis selects the day-count \
         method.",
        &[
            "YIELDMAT ( DATE ( 2022, 3, 1 ), DATE ( 2022, 10, 20 ), DATE ( 2021, 9, 15 ), 0.055, 101.25, 0 )",
            "YIELDMAT ( DATE ( 2022, 3, 1 ), DATE ( 2022, 10, 20 ), DATE ( 2021, 9, 15 ), 0.055, 101.25 )",
        ],
    ),
];
