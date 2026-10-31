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
        "Accrued interest of a security that pays periodic interest. Basis selects the day-count \
         method; Calc_method chooses interest from issue (TRUE, the default) or from \
         first_interest (FALSE) when settlement is after the first coupon date. Frequency must \
         be 1, 2 or 4 and settlement must precede maturity.",
        &[
            "ACCRINT ( DATE ( 2007, 3, 1 ), DATE ( 2008, 8, 31 ), DATE ( 2008, 5, 1 ), 0.1, 1000, 2, 0 )",
            "ACCRINT ( DATE ( 2007, 3, 1 ), DATE ( 2008, 8, 31 ), DATE ( 2008, 5, 1 ), 0.1, 1000, 2, 0, FALSE )",
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
            "ACCRINTM ( DATE ( 2008, 4, 1 ), DATE ( 2008, 6, 15 ), 0.1, 1000, 3 )",
            "ACCRINTM ( DATE ( 2008, 4, 1 ), DATE ( 2008, 6, 15 ), 0.1, 1000 )",
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
            "AMORDEGRC ( 2400, DATE ( 2008, 8, 19 ), DATE ( 2008, 12, 31 ), 300, 0, 0.15, 1 )",
            "AMORDEGRC ( 2400, DATE ( 2008, 8, 19 ), DATE ( 2008, 12, 31 ), 300, 1, 0.15 )",
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
            "AMORLINC ( 2400, DATE ( 2008, 8, 19 ), DATE ( 2008, 12, 31 ), 300, 0, 0.15, 1 )",
            "AMORLINC ( 2400, DATE ( 2008, 8, 19 ), DATE ( 2008, 12, 31 ), 300, 1, 0.15 )",
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
        "Number of days from the beginning of the coupon period to the settlement date. \
         Frequency must be 1, 2 or 4; Basis selects the day-count method, so the result depends \
         on the convention rather than on the calendar alone.",
        &["COUPDAYBS ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2, 1 )", "COUPDAYBS ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2 )"],
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
        "Number of days in the coupon period that contains the settlement date. The result can be \
         fractional because it follows the Basis day-count convention, not a fixed calendar.",
        &["COUPDAYS ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2, 1 )", "COUPDAYS ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2 )"],
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
        "Number of days from the settlement date to the next coupon date. Frequency must be 1, 2 \
         or 4; Basis selects the day-count method.",
        &["COUPDAYSNC ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2, 1 )", "COUPDAYSNC ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2 )"],
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
        "Next coupon date after the settlement date. The date is derived from the maturity date \
         and the coupon frequency; Basis affects only the day-count arithmetic used to locate it.",
        &["COUPNCD ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2, 1 )", "COUPNCD ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2 )"],
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
        "Number of coupons payable between settlement and maturity, rounded up to the nearest \
         whole coupon. Frequency must be 1, 2 or 4; Basis selects the day-count method.",
        &["COUPNUM ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2, 1 )", "COUPNUM ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2 )"],
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
        "Previous coupon date before the settlement date. The date follows from the maturity date \
         and coupon frequency; Basis selects the day-count method used in the arithmetic.",
        &["COUPPCD ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2, 1 )", "COUPPCD ( DATE ( 2011, 1, 25 ), DATE ( 2011, 11, 15 ), 2 )"],
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
        &["CUMIPMT ( 0.09 / 12, 30 * 12, 125000, 13, 24, 0 )"],
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
        &["CUMPRINC ( 0.09 / 12, 30 * 12, 125000, 13, 24, 0 )"],
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
        "Depreciation for one period using the fixed-declining balance method. Month is the \
         number of months in the first year (12 when omitted), so a partial first year changes \
         every subsequent period's depreciation.",
        &["DB ( 1000000, 100000, 6, 1, 7 )", "DB ( 1000000, 100000, 6, 2 )"],
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
        "Depreciation for one period using the double-declining balance method, or another \
         accelerating factor when Factor is supplied (2 when omitted). Depreciation stops at the \
         salvage value.",
        &["DDB ( 2400, 300, 10, 1, 2 )", "DDB ( 2400, 300, 10, 2 )"],
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
        &["DISC ( DATE ( 2007, 1, 25 ), DATE ( 2007, 6, 15 ), 97.975, 100, 1 )", "DISC ( DATE ( 2007, 1, 25 ), DATE ( 2007, 6, 15 ), 97.975, 100 )"],
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
        &["DOLLARDE ( 1.02, 16 )", "DOLLARDE ( 1.1, 32 )"],
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
        &["DOLLARFR ( 1.125, 16 )", "DOLLARFR ( 1.0625, 32 )"],
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
            "DURATION ( DATE ( 2018, 1, 1 ), DATE ( 2028, 1, 1 ), 0.08, 0.09, 2, 1 )",
            "DURATION ( DATE ( 2018, 1, 1 ), DATE ( 2028, 1, 1 ), 0.08, 0.09, 2 )",
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
        &["EFFECT ( 0.0525, 4 )", "EFFECT ( 0.1, 12 )"],
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
        &["FV ( 0.06 / 12, 10, -200, -500, 1 )", "FV ( 0.06 / 12, 10, -200 )"],
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
            "INTRATE ( DATE ( 2008, 2, 15 ), DATE ( 2008, 5, 15 ), 1000000, 1014420, 2 )",
            "INTRATE ( DATE ( 2008, 2, 15 ), DATE ( 2008, 5, 15 ), 1000000, 1014420 )",
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
        "Interest payment in a single period Per of an investment or loan with constant payments \
         and a constant interest rate. Per is 1-based and must not exceed Nper; Type is 0 (end) \
         or 1 (beginning).",
        &["IPMT ( 0.1 / 12, 1, 36, 8000, 0, 1 )", "IPMT ( 0.1 / 12, 1, 36, 8000 )"],
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
        "Interest paid or received in period Per of a loan or investment with even principal \
         payments rather than constant total payments. Per and Nper use the same period unit, \
         and Per is limited by Nper.",
        &["ISPMT ( 0.1 / 12, 1, 36, 8000000 )"],
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
        "Modified Macaulay duration of a bond with an assumed par value of 100, the Macaulay \
         duration divided by 1 + Yld/Frequency. Frequency must be 1, 2 or 4 and Basis selects \
         the day-count method.",
        &[
            "MDURATION ( DATE ( 2018, 1, 1 ), DATE ( 2028, 1, 1 ), 0.08, 0.09, 2, 1 )",
            "MDURATION ( DATE ( 2018, 1, 1 ), DATE ( 2028, 1, 1 ), 0.08, 0.09, 2 )",
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
        &["NOMINAL ( 0.053543, 4 )", "NOMINAL ( 0.1, 12 )"],
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
        &["NPER ( 0.12 / 12, -100, -1000, 10000, 1 )", "NPER ( 0.12 / 12, -100, -1000 )"],
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
        "Price per 100 of face value of a security with an odd, short or long, first coupon \
         period. Issue and First_coupon define that period; Frequency must be 1, 2 or 4 and \
         Basis selects the day-count method.",
        &[
            "ODDFPRICE ( DATE ( 2008, 11, 11 ), DATE ( 2021, 3, 1 ), DATE ( 2008, 10, 15 ), DATE ( 2009, 3, 1 ), 0.0785, 0.0625, 100, 2, 1 )",
            "ODDFPRICE ( DATE ( 2008, 11, 11 ), DATE ( 2021, 3, 1 ), DATE ( 2008, 10, 15 ), DATE ( 2009, 3, 1 ), 0.0785, 0.0625, 100, 2 )",
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
        "Yield of a security with an odd, short or long, first coupon period. Pr is the price per \
         100 of face value; the calculation is iterative, and Basis selects the day-count method.",
        &[
            "ODDFYIELD ( DATE ( 2008, 11, 11 ), DATE ( 2021, 3, 1 ), DATE ( 2008, 10, 15 ), DATE ( 2009, 3, 1 ), 0.0575, 84.5, 100, 2, 0 )",
            "ODDFYIELD ( DATE ( 2008, 11, 11 ), DATE ( 2021, 3, 1 ), DATE ( 2008, 10, 15 ), DATE ( 2009, 3, 1 ), 0.0575, 84.5, 100, 2 )",
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
        "Price per 100 of face value of a security with an odd, short or long, last coupon \
         period. Last_interest starts that period; Frequency must be 1, 2 or 4 and Basis selects \
         the day-count method.",
        &[
            "ODDLPRICE ( DATE ( 2008, 2, 7 ), DATE ( 2008, 6, 15 ), DATE ( 2007, 10, 15 ), 0.0375, 0.0405, 100, 2, 0 )",
            "ODDLPRICE ( DATE ( 2008, 2, 7 ), DATE ( 2008, 6, 15 ), DATE ( 2007, 10, 15 ), 0.0375, 0.0405, 100, 2 )",
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
        "Yield of a security with an odd, short or long, last coupon period. Pr is the price per \
         100 of face value; Frequency must be 1, 2 or 4 and Basis selects the day-count method.",
        &[
            "ODDLYIELD ( DATE ( 2008, 4, 20 ), DATE ( 2008, 6, 15 ), DATE ( 2007, 12, 24 ), 0.0375, 99.875, 100, 2, 0 )",
            "ODDLYIELD ( DATE ( 2008, 4, 20 ), DATE ( 2008, 6, 15 ), DATE ( 2007, 12, 24 ), 0.0375, 99.875, 100, 2 )",
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
        &["PDURATION ( 0.025, 2000, 2200 )", "PDURATION ( 0.1, 1000, 2000 )"],
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
        &["PMT ( 0.08 / 12, 10, 10000, 0, 1 )", "PMT ( 0.08 / 12, 10, 10000 )"],
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
        "Principal payment in a single period Per of an investment or loan with constant payments \
         and a constant interest rate. Per is 1-based and must not exceed Nper; Type is 0 (end) \
         or 1 (beginning).",
        &["PPMT ( 0.1 / 12, 1, 24, 2000, 0, 1 )", "PPMT ( 0.1 / 12, 1, 24, 2000 )"],
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
        "Price per 100 of face value of a security that pays periodic interest, given the yield \
         Yld. Frequency must be 1, 2 or 4 and Basis selects the day-count method used to accrue \
         the first coupon.",
        &[
            "PRICE ( DATE ( 2008, 2, 15 ), DATE ( 2017, 11, 15 ), 0.0575, 0.065, 100, 2, 0 )",
            "PRICE ( DATE ( 2008, 2, 15 ), DATE ( 2017, 11, 15 ), 0.0575, 0.065, 100, 2 )",
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
        "Price per 100 of face value of a discounted security, such as a Treasury bill, from its \
         annual Discount rate. Basis selects the day-count method used to prorate the discount.",
        &[
            "PRICEDISC ( DATE ( 2008, 2, 16 ), DATE ( 2008, 3, 1 ), 0.0525, 100, 2 )",
            "PRICEDISC ( DATE ( 2008, 2, 16 ), DATE ( 2008, 3, 1 ), 0.0525, 100 )",
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
        "Price per 100 of face value of a security that pays interest only at maturity. Issue, \
         Rate and the yield Yld determine the price; Basis selects the day-count method.",
        &[
            "PRICEMAT ( DATE ( 2008, 2, 15 ), DATE ( 2008, 4, 13 ), DATE ( 2007, 11, 11 ), 0.061, 0.061, 0 )",
            "PRICEMAT ( DATE ( 2008, 2, 15 ), DATE ( 2008, 4, 13 ), DATE ( 2007, 11, 11 ), 0.061, 0.061 )",
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
        &["PV ( 0.08 / 12, 20 * 12, 500, 0, 1 )", "PV ( 0.08 / 12, 20 * 12, 500 )"],
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
        "Interest rate per period of an annuity. RATE solves iteratively from Guess (0.1 when \
         omitted) and returns an error when it does not converge within 20 iterations to \
         0.0000001; several rates can satisfy the same cash flows.",
        &["RATE ( 4 * 12, -200, 8000, 0, 1, 0.01 )", "RATE ( 4 * 12, -200, 8000 )"],
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
        "Amount received at maturity for a fully invested security bought at Investment with an \
         annual Discount rate. Basis selects the day-count method used to prorate the discount.",
        &[
            "RECEIVED ( DATE ( 2008, 2, 15 ), DATE ( 2008, 5, 15 ), 1000000, 0.0575, 2 )",
            "RECEIVED ( DATE ( 2008, 2, 15 ), DATE ( 2008, 5, 15 ), 1000000, 0.0575 )",
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
        &["RRI ( 96, 10000, 11000 )", "RRI ( 10, 1000, 2000 )"],
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
        &["SLN ( 30000, 7500, 10 )", "SLN ( 1000, 100, 5 )"],
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
        &["SYD ( 30000, 7500, 10, 1 )", "SYD ( 30000, 7500, 10, 10 )"],
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
        &["TBILLEQ ( DATE ( 2008, 3, 31 ), DATE ( 2008, 6, 1 ), 0.0914 )", "TBILLEQ ( DATE ( 2008, 3, 31 ), DATE ( 2008, 6, 1 ), 0.09 )"],
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
        &["TBILLPRICE ( DATE ( 2008, 3, 31 ), DATE ( 2008, 6, 1 ), 0.09 )", "TBILLPRICE ( DATE ( 2008, 3, 31 ), DATE ( 2008, 6, 1 ), 0.0914 )"],
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
        &["TBILLYIELD ( DATE ( 2008, 3, 31 ), DATE ( 2008, 6, 1 ), 98.45 )", "TBILLYIELD ( DATE ( 2008, 3, 31 ), DATE ( 2008, 6, 1 ), 98.75 )"],
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
        &["VDB ( 2400, 300, 10, 0, 0.875, 1.5 )", "VDB ( 2400, 300, 10, 0, 1 )"],
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
        "Internal rate of return for a schedule of cash flows that need not be periodic. Values \
         and Dates are evaluated once per Table row; the first date anchors the calculation and \
         the result is annualised. Solved iteratively from Guess, so it can fail to converge. \
         AlternateResult is returned instead of an error when supplied.",
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
        "Net present value for a schedule of cash flows that need not be periodic. Values and \
         Dates are evaluated once per Table row and discounted with actual/365 day counts from \
         the earliest date, so the Rate is an annual effective rate.",
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
            "YIELD ( DATE ( 2008, 2, 15 ), DATE ( 2016, 11, 15 ), 0.0575, 95.04287, 100, 2, 0 )",
            "YIELD ( DATE ( 2008, 2, 15 ), DATE ( 2016, 11, 15 ), 0.0575, 95.04287, 100, 2 )",
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
            "YIELDDISC ( DATE ( 2008, 2, 16 ), DATE ( 2008, 3, 1 ), 99.795, 100, 2 )",
            "YIELDDISC ( DATE ( 2008, 2, 16 ), DATE ( 2008, 3, 1 ), 99.795, 100 )",
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
        "Annual yield of a security that pays interest only at maturity, bought at Pr per 100 of \
         face value. Issue and Rate define the interest; Basis selects the day-count method.",
        &[
            "YIELDMAT ( DATE ( 2008, 3, 15 ), DATE ( 2008, 11, 3 ), DATE ( 2007, 11, 8 ), 0.0625, 100.0123, 0 )",
            "YIELDMAT ( DATE ( 2008, 3, 15 ), DATE ( 2008, 11, 3 ), DATE ( 2007, 11, 8 ), 0.0625, 100.0123 )",
        ],
    ),
];
