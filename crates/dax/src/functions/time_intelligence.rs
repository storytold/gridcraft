//! Time intelligence functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.
//!
//! Every function in this category assumes a well-formed date table: a contiguous range of
//! dates, one unique date/date-time column marked as the date table, and a relationship to the
//! fact table based on that column (or the table explicitly marked as a date table). The
//! optional `YearEndDate` arguments control fiscal years; the week-based functions and the
//! calendar-only arguments introduced with model calendars require a calendar reference rather
//! than a date column and are not available to Excel's embedded engine.

use super::{FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec};
use crate::functions::FunctionCategory;

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "CLOSINGBALANCEMONTH",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Dates", ParamKind::Date), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Date]), overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression at the last date of the current month, after applying the \
         optional filter. It opens a month-to-date window only at its closing date, so a filter \
         on the date table is replaced by ENDOFMONTH; the date table must be contiguous and \
         marked as a date table for the relationship to be treated as a calendar.",
        &["CLOSINGBALANCEMONTH ( [Sales Amount], 'Date'[Date] )"],
    ),
    spec(
        "CLOSINGBALANCEQUARTER",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Dates", ParamKind::Date), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Date]), overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression at the last date of the current quarter, after applying the \
         optional filter. The quarter is derived from the marked date table, so missing dates or \
         a relationship not based on the date column invalidate the closing balance.",
        &["CLOSINGBALANCEQUARTER ( [Sales Amount], 'Date'[Date] )"],
    ),
    spec(
        "CLOSINGBALANCEWEEK",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Calendar", ParamKind::Table), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Table]), overload(&[ParamKind::Expression, ParamKind::Table, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, false, true),
        "Evaluates the expression at the last date of the current week of a model calendar, \
         after applying the optional filter. Week boundaries come from the calendar definition, \
         so a plain date column is not accepted and the calendar must define week categories.",
        &["CLOSINGBALANCEWEEK ( [Sales Amount], Gregorian )"],
    ),
    spec(
        "CLOSINGBALANCEYEAR",
        FunctionCategory::TimeIntelligence,
        2,
        4,
        &[
            required("Expression", ParamKind::Expression),
            required("Dates", ParamKind::Date),
            optional("Filter", ParamKind::Any),
            optional("YearEndDate", ParamKind::Text),
        ],
        &[
            overload(&[ParamKind::Expression, ParamKind::Date]),
            overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any]),
            overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any, ParamKind::Text]),
        ],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression at the last date of the current year, after applying the \
         optional filter. Pass YearEndDate to make the closing date follow a fiscal year; the \
         date table must contain every day of that fiscal year and stay marked as a date table.",
        &["CLOSINGBALANCEYEAR ( [Sales Amount], 'Date'[Date] )", "CLOSINGBALANCEYEAR ( [Sales Amount], 'Date'[Date], ALL ( Sales ), \"6/30/2008\" )"],
    ),
    spec(
        "DATEADD",
        FunctionCategory::TimeIntelligence,
        3,
        5,
        &[
            required("Dates", ParamKind::Date),
            required("NumberOfIntervals", ParamKind::Number),
            required("Interval", ParamKind::Text),
            optional("Extension", ParamKind::Text),
            optional("Truncation", ParamKind::Text),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Number, ParamKind::Text]),
            overload(&[ParamKind::Date, ParamKind::Number, ParamKind::Text, ParamKind::Text]),
            overload(&[ParamKind::Date, ParamKind::Number, ParamKind::Text, ParamKind::Text, ParamKind::Text]),
        ],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Shifts the dates visible in the filter context by NumberOfIntervals units of Interval \
         (DAY, MONTH, QUARTER or YEAR) and returns the dates that exist in the date column. \
         Extension and Truncation are available only for a model calendar; for a date column \
         DATEADD accepts exactly three arguments. With a calendar, YEAR follows the calendar's \
         year definition, which may be a fiscal year.",
        &["DATEADD ( 'Date'[Date], -1, YEAR )", "DATEADD ( Gregorian, 1, MONTH )", "DATEADD ( Gregorian, 1, MONTH, EXTENDING, ANCHORED )"],
    ),
    spec(
        "DATESBETWEEN",
        FunctionCategory::TimeIntelligence,
        3,
        3,
        &[required("Dates", ParamKind::Date), required("StartDate", ParamKind::Date), required("EndDate", ParamKind::Date)],
        &[overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the dates between two inclusive boundaries that exist in the date column. A \
         BLANK StartDate begins at MIN ( Dates ), a BLANK EndDate ends at MAX ( Dates ), and a \
         StartDate after EndDate returns an empty table. The result keeps the lineage of the \
         marked date table, so the date table must be contiguous for windows to be meaningful.",
        &["DATESBETWEEN ( 'Date'[Date], DATE ( 2008, 8, 1 ), DATE ( 2008, 8, 31 ) )"],
    ),
    spec(
        "DATESINPERIOD",
        FunctionCategory::TimeIntelligence,
        4,
        5,
        &[
            required("Dates", ParamKind::Date),
            required("StartDate", ParamKind::Date),
            required("NumberOfIntervals", ParamKind::Number),
            required("Interval", ParamKind::Text),
            optional("EndBehavior", ParamKind::Text),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Text]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Text, ParamKind::Text]),
        ],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the dates from StartDate over NumberOfIntervals units of Interval (DAY, WEEK, \
         MONTH, QUARTER or YEAR); a negative count looks backwards. BLANK StartDate uses the \
         minimum date in the column. EndBehavior (PRECISE or ENDALIGNED) is available only with \
         a model calendar; consider WINDOW when the interval arithmetic is not needed.",
        &[
            "DATESINPERIOD ( 'Date'[Date], MAX ( 'Date'[Date] ), -12, MONTH )",
            "DATESINPERIOD ( Gregorian, DATE ( 2008, 2, 29 ), -1, MONTH, ENDALIGNED )",
        ],
    ),
    spec(
        "DATESMTD",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the month-to-date window: dates from the first day of the month through the last \
         date visible in the filter context, restricted to dates present in the column. This is \
         syntax sugar for DATESBETWEEN over the current month and requires a marked, contiguous \
         date table.",
        &["DATESMTD ( 'Date'[Date] )"],
    ),
    spec(
        "DATESQTD",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the quarter-to-date window: dates from the first day of the current quarter \
         through the last date visible in the filter context. Quarter boundaries come from the \
         marked date table, so an incomplete or unmarked date table breaks the window.",
        &["DATESQTD ( 'Date'[Date] )"],
    ),
    spec(
        "DATESWTD",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Calendar", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, true),
        "Returns the week-to-date window of a model calendar: dates from the first day of the \
         current week through the last visible date. Week numbering and the first day of the week \
         come from the calendar definition, not from the date column, and the function is not \
         available to Excel's DAX engine.",
        &["DATESWTD ( Gregorian )"],
    ),
    spec(
        "DATESYTD",
        FunctionCategory::TimeIntelligence,
        1,
        2,
        &[required("Dates", ParamKind::Date), optional("YearEndDate", ParamKind::Text)],
        &[overload(&[ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Text])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the year-to-date window: dates from the start of the year through the last date \
         visible in the filter context, keeping only dates present in the column. YearEndDate \
         selects a fiscal year end (for example \"6/30\"); the date table must cover whole fiscal \
         years and be marked as a date table for the window to be correct.",
        &["DATESYTD ( 'Date'[Date] )", "DATESYTD ( 'Date'[Date], \"6/30/2008\" )"],
    ),
    spec(
        "ENDOFMONTH",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the last date of the month visible in the filter context as a single-row table, \
         restricted to dates present in the date column. Use it as a CALCULATE modifier for \
         semi-additive measures; the marked date table must contain the month-end date.",
        &["ENDOFMONTH ( 'Date'[Date] )"],
    ),
    spec(
        "ENDOFQUARTER",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the last date of the quarter visible in the filter context as a single-row table, \
         restricted to dates present in the date column. Quarter boundaries follow the marked \
         date table; a non-contiguous table can omit the quarter-end date.",
        &["ENDOFQUARTER ( 'Date'[Date] )"],
    ),
    spec(
        "ENDOFWEEK",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Calendar", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, true),
        "Returns the last date of the week visible in the filter context for a model calendar. \
         The week definition, including the first day of the week, comes from the calendar, so a \
         date column is not accepted and Excel's DAX engine does not support the function.",
        &["ENDOFWEEK ( Gregorian )"],
    ),
    spec(
        "ENDOFYEAR",
        FunctionCategory::TimeIntelligence,
        1,
        2,
        &[required("Dates", ParamKind::Date), optional("YearEndDate", ParamKind::Text)],
        &[overload(&[ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Text])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the last date of the year visible in the filter context as a single-row table. \
         YearEndDate selects a fiscal year end; without it the window is December 31. The date \
         table must be marked and must contain the resulting date.",
        &["ENDOFYEAR ( 'Date'[Date] )", "ENDOFYEAR ( 'Date'[Date], \"6/30/2008\" )"],
    ),
    spec(
        "FIRSTDATE",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the first date visible in the filter context, as a single-row table, restricted \
         to dates that exist in the date column. It considers blanks, unlike FIRSTNONBLANK; the \
         table must be marked as a date table for time intelligence windows to behave correctly.",
        &["FIRSTDATE ( 'Date'[Date] )"],
    ),
    spec(
        "FIRSTNONBLANK",
        FunctionCategory::TimeIntelligence,
        2,
        2,
        &[required("ColumnName", ParamKind::Column), required("Expression", ParamKind::Expression)],
        &[overload(&[ParamKind::Column, ParamKind::Expression])],
        ReturnType::Table,
        FunctionFlags::new(true, false, false, true, true),
        "Iterates the column in its sort order and returns the first value for which the \
         expression is not blank, as a single-column, single-row table. The column need not be a \
         date, but over a marked date table it defines the start of the time window; the iterator \
         can cause performance and memory issues on large models, so prefer an optimized pattern.",
        &["FIRSTNONBLANK ( 'Date'[Date], [Sales Amount] )"],
    ),
    spec(
        "FIRSTNONBLANKVALUE",
        FunctionCategory::TimeIntelligence,
        2,
        2,
        &[required("ColumnName", ParamKind::Column), required("Expression", ParamKind::Expression)],
        &[overload(&[ParamKind::Column, ParamKind::Expression])],
        ReturnType::Any,
        FunctionFlags::new(true, false, false, true, true),
        "Returns the expression evaluated at the start of the visible time window: the first \
         value of the column whose expression is not blank, without returning a table. It is the \
         scalar counterpart of FIRSTNONBLANK and is commonly used for semi-additive measures over \
         a marked date table; the evaluation is an iterator, so large models pay per-row cost.",
        &["FIRSTNONBLANKVALUE ( 'Date'[Date], [Sales Amount] )"],
    ),
    spec(
        "LASTDATE",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the last date visible in the filter context, as a single-row table, restricted \
         to dates that exist in the date column. It is the anchor of many YTD windows; a missing \
         or unmarked date table makes the result follow partial selections instead of the calendar.",
        &["LASTDATE ( 'Date'[Date] )"],
    ),
    spec(
        "LASTNONBLANK",
        FunctionCategory::TimeIntelligence,
        2,
        2,
        &[required("ColumnName", ParamKind::Column), required("Expression", ParamKind::Expression)],
        &[overload(&[ParamKind::Column, ParamKind::Expression])],
        ReturnType::Table,
        FunctionFlags::new(true, false, false, true, true),
        "Iterates the column in its sort order and returns the last value for which the \
         expression is not blank, as a single-column, single-row table. The column need not be a \
         date, but over a marked date table it defines the end of the time window; the iterator \
         can be expensive, so dax.guide recommends optimized semi-additive patterns.",
        &["LASTNONBLANK ( 'Date'[Date], [Sales Amount] )"],
    ),
    spec(
        "LASTNONBLANKVALUE",
        FunctionCategory::TimeIntelligence,
        2,
        2,
        &[required("ColumnName", ParamKind::Column), required("Expression", ParamKind::Expression)],
        &[overload(&[ParamKind::Column, ParamKind::Expression])],
        ReturnType::Any,
        FunctionFlags::new(true, false, false, true, true),
        "Returns the expression evaluated at the end of the visible time window: the last value \
         of the column whose expression is not blank, without returning a table. It is the scalar \
         counterpart of LASTNONBLANK and a typical semi-additive pattern over a marked date \
         table; the per-row iteration is costly on large models.",
        &["LASTNONBLANKVALUE ( 'Date'[Date], [Sales Amount] )"],
    ),
    spec(
        "NEXTDAY",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the day after the last date visible in the filter context, as a single-row table \
         restricted to dates present in the date column. Because the date table must be \
         contiguous, a missing next day yields an empty table.",
        &["NEXTDAY ( 'Date'[Date] )"],
    ),
    spec(
        "NEXTMONTH",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns all dates of the month after the last date visible in the filter context. The \
         month boundaries come from the marked date table, so gaps in the calendar produce \
         incomplete results and a bad relationship makes the window follow the wrong selection.",
        &["NEXTMONTH ( 'Date'[Date] )"],
    ),
    spec(
        "NEXTQUARTER",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns all dates of the quarter after the last date visible in the filter context. The \
         quarter is derived from the marked date table; the result keeps the date column lineage \
         and excludes dates absent from the calendar.",
        &["NEXTQUARTER ( 'Date'[Date] )"],
    ),
    spec(
        "NEXTWEEK",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Calendar", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, true),
        "Returns all dates of the week after the last date visible in the filter context, using \
         the week definition of a model calendar. A plain date column cannot supply week \
         boundaries, and Excel's embedded DAX engine does not expose the calendar-based function.",
        &["NEXTWEEK ( Gregorian )"],
    ),
    spec(
        "NEXTYEAR",
        FunctionCategory::TimeIntelligence,
        1,
        2,
        &[required("Dates", ParamKind::Date), optional("YearEndDate", ParamKind::Text)],
        &[overload(&[ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Text])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns all dates of the year after the last date visible in the filter context. \
         YearEndDate shifts the year to a fiscal calendar; the date table must contain the whole \
         following year, be contiguous and be marked as a date table.",
        &["NEXTYEAR ( 'Date'[Date] )", "NEXTYEAR ( 'Date'[Date], \"6/30/2008\" )"],
    ),
    spec(
        "OPENINGBALANCEMONTH",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Dates", ParamKind::Date), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Date]), overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression at the last date of the previous month, after applying the \
         optional filter. The opening balance uses the marked date table's month boundaries, so \
         the relationship and date coverage must be valid or the window slides on partial data.",
        &["OPENINGBALANCEMONTH ( [Sales Amount], 'Date'[Date] )"],
    ),
    spec(
        "OPENINGBALANCEQUARTER",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Dates", ParamKind::Date), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Date]), overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression at the last date of the previous quarter, after applying the \
         optional filter. Quarter boundaries come from the marked date table; an unmarked table \
         or a relationship not based on the date column changes the opening period.",
        &["OPENINGBALANCEQUARTER ( [Sales Amount], 'Date'[Date] )"],
    ),
    spec(
        "OPENINGBALANCEWEEK",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Calendar", ParamKind::Table), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Table]), overload(&[ParamKind::Expression, ParamKind::Table, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, false, true),
        "Evaluates the expression at the last date of the previous week of a model calendar, \
         after applying the optional filter. The calendar defines the week, so a date column is \
         not accepted and the function is unavailable to Excel's DAX engine.",
        &["OPENINGBALANCEWEEK ( [Sales Amount], Gregorian )"],
    ),
    spec(
        "OPENINGBALANCEYEAR",
        FunctionCategory::TimeIntelligence,
        2,
        4,
        &[
            required("Expression", ParamKind::Expression),
            required("Dates", ParamKind::Date),
            optional("Filter", ParamKind::Any),
            optional("YearEndDate", ParamKind::Text),
        ],
        &[
            overload(&[ParamKind::Expression, ParamKind::Date]),
            overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any]),
            overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any, ParamKind::Text]),
        ],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression at the last date of the previous year, after applying the \
         optional filter. YearEndDate aligns the opening balance with a fiscal year; the date \
         table must cover entire fiscal years and be marked as a date table.",
        &["OPENINGBALANCEYEAR ( [Sales Amount], 'Date'[Date] )", "OPENINGBALANCEYEAR ( [Sales Amount], 'Date'[Date], ALL ( Sales ), \"6/30/2008\" )"],
    ),
    spec(
        "PARALLELPERIOD",
        FunctionCategory::TimeIntelligence,
        3,
        3,
        &[required("Dates", ParamKind::Date), required("NumberOfIntervals", ParamKind::Number), required("Interval", ParamKind::Text)],
        &[overload(&[ParamKind::Date, ParamKind::Number, ParamKind::Text])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Shifts the first and last dates visible in the filter context by NumberOfIntervals units \
         of MONTH, QUARTER or YEAR, then returns all contiguous dates between the shifted \
         boundaries, expanding partial periods to complete ones. The result contains only dates \
         in the marked date column, so a discontinuous calendar leaves gaps.",
        &["PARALLELPERIOD ( 'Date'[Date], -1, YEAR )", "PARALLELPERIOD ( 'Date'[Date], 0, MONTH )"],
    ),
    spec(
        "PREVIOUSDAY",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the day before the first date visible in the filter context, as a single-row \
         table restricted to dates present in the date column. The marked date table must be \
         contiguous; otherwise the previous day may not exist in the model.",
        &["PREVIOUSDAY ( 'Date'[Date] )"],
    ),
    spec(
        "PREVIOUSMONTH",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns all dates of the month before the first date visible in the filter context. Month \
         boundaries come from the marked date table; gaps in the calendar or an incorrect \
         relationship make the previous month incomplete.",
        &["PREVIOUSMONTH ( 'Date'[Date] )"],
    ),
    spec(
        "PREVIOUSQUARTER",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns all dates of the quarter before the first date visible in the filter context. The \
         quarter is derived from the marked date table, so the table must contain every day of \
         the previous quarter for the window to be complete.",
        &["PREVIOUSQUARTER ( 'Date'[Date] )"],
    ),
    spec(
        "PREVIOUSWEEK",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Calendar", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, true),
        "Returns all dates of the week before the first date visible in the filter context, using \
         a model calendar's week definition. Week boundaries are calendar metadata, so a date \
         column is not accepted and Excel's DAX engine cannot call the function.",
        &["PREVIOUSWEEK ( Gregorian )"],
    ),
    spec(
        "PREVIOUSYEAR",
        FunctionCategory::TimeIntelligence,
        1,
        2,
        &[required("Dates", ParamKind::Date), optional("YearEndDate", ParamKind::Text)],
        &[overload(&[ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Text])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns all dates of the year before the first date visible in the filter context, \
         optionally aligned to a fiscal year end. The marked date table must include the whole \
         previous year, including leap days, for the window to be complete.",
        &["PREVIOUSYEAR ( 'Date'[Date] )", "PREVIOUSYEAR ( 'Date'[Date], \"6/30/2008\" )"],
    ),
    spec(
        "SAMEPERIODLASTYEAR",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the dates of the same period one year earlier, preserving the number of days \
         rather than the month boundaries. It is syntax sugar for DATEADD ( Dates, -1, YEAR ) and \
         relies on a marked, contiguous date table; a fiscal year requires adapting the offset \
         or the calendar definition.",
        &["SAMEPERIODLASTYEAR ( 'Date'[Date] )"],
    ),
    spec(
        "STARTOFMONTH",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the first date of the month visible in the filter context, as a single-row table \
         restricted to dates in the date column. The marked date table must contain the month's \
         first day for the result to be non-empty.",
        &["STARTOFMONTH ( 'Date'[Date] )"],
    ),
    spec(
        "STARTOFQUARTER",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Dates", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the first date of the quarter visible in the filter context, as a single-row \
         table restricted to dates in the date column. Quarter boundaries follow the marked date \
         table, so an incomplete calendar can omit the first day.",
        &["STARTOFQUARTER ( 'Date'[Date] )"],
    ),
    spec(
        "STARTOFWEEK",
        FunctionCategory::TimeIntelligence,
        1,
        1,
        &[required("Calendar", ParamKind::Table)],
        &[overload(&[ParamKind::Table])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, false, true),
        "Returns the first date of the week visible in the filter context for a model calendar. \
         The calendar defines the first day of the week, so a date column is not accepted and the \
         function is not available to Excel's embedded DAX engine.",
        &["STARTOFWEEK ( Gregorian )"],
    ),
    spec(
        "STARTOFYEAR",
        FunctionCategory::TimeIntelligence,
        1,
        2,
        &[required("Dates", ParamKind::Date), optional("YearEndDate", ParamKind::Text)],
        &[overload(&[ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Text])],
        ReturnType::Table,
        FunctionFlags::new(false, false, false, true, true),
        "Returns the first date of the year visible in the filter context as a single-row table. \
         YearEndDate shifts the start to a fiscal year; the date table must be marked and include \
         every day from the fiscal-year start through year end.",
        &["STARTOFYEAR ( 'Date'[Date] )", "STARTOFYEAR ( 'Date'[Date], \"6/30/2008\" )"],
    ),
    spec(
        "TOTALMTD",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Dates", ParamKind::Date), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Date]), overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression over the month-to-date window, from the first day of the month \
         through the last visible date, after applying the optional filter. It is syntax sugar \
         for CALCULATE with DATESMTD and depends on a marked, contiguous date table.",
        &["TOTALMTD ( SUM ( Sales[Amount] ), 'Date'[Date] )"],
    ),
    spec(
        "TOTALQTD",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Dates", ParamKind::Date), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Date]), overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression over the quarter-to-date window, from the first day of the \
         quarter through the last visible date, after applying the optional filter. It is syntax \
         sugar for CALCULATE with DATESQTD and requires the date table to be marked.",
        &["TOTALQTD ( SUM ( Sales[Amount] ), 'Date'[Date] )"],
    ),
    spec(
        "TOTALWTD",
        FunctionCategory::TimeIntelligence,
        2,
        3,
        &[required("Expression", ParamKind::Expression), required("Calendar", ParamKind::Table), optional("Filter", ParamKind::Any)],
        &[overload(&[ParamKind::Expression, ParamKind::Table]), overload(&[ParamKind::Expression, ParamKind::Table, ParamKind::Any])],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, false, true),
        "Evaluates the expression over the week-to-date window defined by a model calendar, from \
         the first day of the week through the last visible date, after applying the optional \
         filter. Week boundaries come from the calendar, and Excel's DAX engine does not support \
         the function.",
        &["TOTALWTD ( SUM ( Sales[Amount] ), Gregorian )"],
    ),
    spec(
        "TOTALYTD",
        FunctionCategory::TimeIntelligence,
        2,
        4,
        &[
            required("Expression", ParamKind::Expression),
            required("Dates", ParamKind::Date),
            optional("Filter", ParamKind::Any),
            optional("YearEndDate", ParamKind::Text),
        ],
        &[
            overload(&[ParamKind::Expression, ParamKind::Date]),
            overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any]),
            overload(&[ParamKind::Expression, ParamKind::Date, ParamKind::Any, ParamKind::Text]),
        ],
        ReturnType::Any,
        FunctionFlags::new(false, false, false, true, true),
        "Evaluates the expression over the year-to-date window, from the first day of the year \
         through the last visible date, after applying the optional filter. It is syntax sugar \
         for CALCULATE with DATESYTD; YearEndDate selects a fiscal year and the date table must \
         be marked and cover whole fiscal years.",
        &[
            "TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date] )",
            "TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date], Sales[Region] = \"US\" )",
            "TOTALYTD ( SUM ( Sales[Amount] ), 'Date'[Date], ALL ( Sales ), \"6/30/2008\" )",
        ],
    ),
];
