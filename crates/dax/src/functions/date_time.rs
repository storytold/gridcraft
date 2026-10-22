//! Date and time functions.
//!
//! Every record follows the shared catalog shape in `super`: exact name, arity, parameter
//! kinds, overloads, return type, flags, description and parseable examples.

use super::{FunctionCategory, FunctionFlags, FunctionSpec, ParamKind, ReturnType, optional, overload, required, spec, standard};

use FunctionCategory::DateAndTime;

pub static FUNCTIONS: &[FunctionSpec] = &[
    spec(
        "CALENDAR",
        DateAndTime,
        2,
        2,
        &[required("StartDate", ParamKind::Date), required("EndDate", ParamKind::Date)],
        &[overload(&[ParamKind::Date, ParamKind::Date])],
        ReturnType::Table,
        standard(false),
        "CALENDAR ( <StartDate>, <EndDate> ) returns a single-column table named Date with every \
         day from StartDate to EndDate, both included. Dates are stored as numbers, so integer \
         bounds such as CALENDAR ( 1, 100 ) are accepted; a date table should cover whole years \
         for time-intelligence compatibility.",
        &["CALENDAR ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ) )", "CALENDAR ( DATE ( 2023, 7, 1 ), DATE ( 2024, 6, 30 ) )"],
    ),
    spec(
        "CALENDARAUTO",
        DateAndTime,
        0,
        1,
        &[optional("FiscalYearEndMonth", ParamKind::Number)],
        &[overload(&[]), overload(&[ParamKind::Number])],
        ReturnType::Table,
        standard(false),
        "CALENDARAUTO ( [<FiscalYearEndMonth>] ) scans the imported date columns and returns a \
         contiguous Date table covering whole fiscal years. FiscalYearEndMonth is 1-12 and \
         defaults to 12 (December); calculated tables and calculated columns are ignored, and \
         the model must contain at least one imported datetime column or the call fails.",
        &["CALENDARAUTO ()", "CALENDARAUTO ( 3 )"],
    ),
    spec(
        "DATE",
        DateAndTime,
        3,
        3,
        &[required("Year", ParamKind::Number), required("Month", ParamKind::Number), required("Day", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::DateTime,
        standard(false),
        "DATE ( <Year>, <Month>, <Day> ) builds a DateTime from numeric parts. Years 0-99 add \
         1900 (DATE ( 98, 1, 2 ) is 1998-01-02) while years below -1800 or above 9999 raise an \
         error; month and day overflow roll into neighbouring months and days (DATE ( 2024, 13, \
         1 ) is 2025-01-01).",
        &["DATE ( 2024, 1, 1 )", "DATE ( 2024, 13, 1 )", "DATE ( YEAR ( Sales[Order Date] ), 1, 1 )"],
    ),
    spec(
        "DATEDIFF",
        DateAndTime,
        3,
        3,
        &[required("Date1", ParamKind::Date), required("Date2", ParamKind::Date), required("Interval", ParamKind::Any)],
        &[overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Any])],
        ReturnType::Integer,
        standard(false),
        "DATEDIFF ( <Date1>, <Date2>, <Interval> ) counts the Interval boundaries crossed \
         between two dates, positively when Date2 is later and negatively otherwise. Interval is \
         an unquoted keyword: SECOND, MINUTE, HOUR, DAY, WEEK, MONTH, QUARTER or YEAR.",
        &["DATEDIFF ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ), DAY )", "DATEDIFF ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ), MONTH )"],
    ),
    spec(
        "DATEVALUE",
        DateAndTime,
        1,
        1,
        &[required("DateText", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::DateTime,
        standard(false),
        "DATEVALUE ( <DateText> ) converts a text date to a DateTime using the locale of the \
         model, so \"1/8/2009\" may mean January 8 or August 1. A missing year is taken from the \
         current year and any time portion is ignored.",
        &["DATEVALUE ( \"2024-01-01\" )", "DATEVALUE ( \"1/8/2009\" )"],
    ),
    spec(
        "DAY",
        DateAndTime,
        1,
        1,
        &[required("Date", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Integer,
        standard(false),
        "DAY ( <Date> ) returns the day of the month as an integer from 1 to 31. A text argument \
         is converted with the same rules as DATEVALUE, so the locale of the model decides how \
         the text is read.",
        &["DAY ( DATE ( 2024, 1, 15 ) )", "DAY ( Sales[Order Date] )"],
    ),
    spec(
        "EDATE",
        DateAndTime,
        2,
        2,
        &[required("StartDate", ParamKind::Date), required("Months", ParamKind::Number)],
        &[overload(&[ParamKind::Date, ParamKind::Number])],
        ReturnType::DateTime,
        standard(false),
        "EDATE ( <StartDate>, <Months> ) shifts a date by the given number of months, keeping the \
         day of the month and clamping to the end of shorter months (2024-01-31 plus one month \
         is 2024-02-29). Unlike EOMONTH it does not move to the end of the month.",
        &["EDATE ( DATE ( 2024, 1, 31 ), 1 )", "EDATE ( Sales[Order Date], -3 )"],
    ),
    spec(
        "EOMONTH",
        DateAndTime,
        2,
        2,
        &[required("StartDate", ParamKind::Date), required("Months", ParamKind::Number)],
        &[overload(&[ParamKind::Date, ParamKind::Number])],
        ReturnType::DateTime,
        standard(false),
        "EOMONTH ( <StartDate>, <Months> ) returns the last day of the month that is Months \
         before or after StartDate; 0 gives the end of the current month. There is no SOMONTH, so \
         the start of a month is EOMONTH ( <date>, -1 ) + 1.",
        &["EOMONTH ( DATE ( 2024, 1, 15 ), 0 )", "EOMONTH ( Sales[Order Date], 1 )"],
    ),
    spec(
        "HOUR",
        DateAndTime,
        1,
        1,
        &[required("Datetime", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Integer,
        standard(false),
        "HOUR ( <Datetime> ) returns the hour as an integer from 0 to 23. The argument may be a \
         DateTime or text in time format such as \"16:48:00\"; text is parsed with the locale and \
         time settings of the client computer.",
        &["HOUR ( TIME ( 16, 48, 23 ) )", "HOUR ( Sales[Order Time] )"],
    ),
    spec(
        "MINUTE",
        DateAndTime,
        1,
        1,
        &[required("Datetime", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Integer,
        standard(false),
        "MINUTE ( <Datetime> ) returns the minute as an integer from 0 to 59. The argument may be \
         a DateTime or text in time format such as \"16:48:00\"; text is parsed with the locale \
         and time settings of the client computer.",
        &["MINUTE ( TIME ( 16, 48, 23 ) )", "MINUTE ( Sales[Order Time] )"],
    ),
    spec(
        "MONTH",
        DateAndTime,
        1,
        1,
        &[required("Date", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Integer,
        standard(false),
        "MONTH ( <Date> ) returns the month as an integer from 1 (January) to 12 (December). A \
         text argument is converted with the same rules as DATEVALUE.",
        &["MONTH ( DATE ( 2024, 1, 15 ) )", "MONTH ( Sales[Order Date] )"],
    ),
    spec(
        "NETWORKDAYS",
        DateAndTime,
        2,
        4,
        &[
            required("start_date", ParamKind::Date),
            required("end_date", ParamKind::Date),
            optional("weekend", ParamKind::Number),
            optional("holidays", ParamKind::Table),
        ],
        &[
            overload(&[ParamKind::Date, ParamKind::Date]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number]),
            overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number, ParamKind::Table]),
        ],
        ReturnType::Integer,
        FunctionFlags::new(false, false, false, false, true),
        "NETWORKDAYS ( <start_date>, <end_date> [, <weekend>] [, <holidays>] ) counts whole \
         workdays between two dates, both included, skipping weekends and holidays. The weekend \
         code defaults to 1 (Saturday and Sunday), accepts 2-7 for other pairs and 11-17 for a \
         single day; holidays is a single-column table of dates. Not available in Excel's DAX \
         engine.",
        &[
            "NETWORKDAYS ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ) )",
            "NETWORKDAYS ( DATE ( 2024, 1, 1 ), DATE ( 2024, 1, 31 ), 1, { DATE ( 2024, 1, 15 ) } )",
        ],
    ),
    spec(
        "NOW",
        DateAndTime,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::DateTime,
        FunctionFlags::new(false, true, false, true, true),
        "NOW () returns the current date and time. Volatile: it is evaluated once per query and \
         only changes when the model or the containing calculated column is refreshed, not \
         continuously.",
        &["NOW ()", "YEAR ( NOW () )"],
    ),
    spec(
        "QUARTER",
        DateAndTime,
        1,
        1,
        &[required("Date", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Integer,
        standard(false),
        "QUARTER ( <Date> ) returns the calendar quarter as an integer from 1 (January-March) to \
         4 (October-December); it does not follow fiscal periods. A text argument is converted \
         with the same rules as DATEVALUE.",
        &["QUARTER ( DATE ( 2024, 5, 15 ) )", "QUARTER ( Sales[Order Date] )"],
    ),
    spec(
        "SECOND",
        DateAndTime,
        1,
        1,
        &[required("Datetime", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Integer,
        standard(false),
        "SECOND ( <Datetime> ) returns the second as an integer from 0 to 59. The argument may be \
         a DateTime or text in time format such as \"16:48:23\"; text is parsed with the locale \
         and time settings of the client computer.",
        &["SECOND ( TIME ( 16, 48, 23 ) )", "SECOND ( Sales[Order Time] )"],
    ),
    spec(
        "TIME",
        DateAndTime,
        3,
        3,
        &[required("Hour", ParamKind::Number), required("Minute", ParamKind::Number), required("Second", ParamKind::Number)],
        &[overload(&[ParamKind::Number, ParamKind::Number, ParamKind::Number])],
        ReturnType::DateTime,
        standard(false),
        "TIME ( <Hour>, <Minute>, <Second> ) builds a time on the 1899-12-30 epoch. Overflowing \
         arguments wrap inside the same day instead of raising an error (TIME ( 10, 90, 0 ) is \
         11:30 and TIME ( 50, 0, 0 ) is 02:00), and each argument is limited to 32,767.",
        &["TIME ( 16, 48, 23 )", "TIME ( 10, 90, 0 )"],
    ),
    spec(
        "TIMEVALUE",
        DateAndTime,
        1,
        1,
        &[required("TimeText", ParamKind::Text)],
        &[overload(&[ParamKind::Text])],
        ReturnType::DateTime,
        standard(false),
        "TIMEVALUE ( <TimeText> ) converts a text time to a DateTime, ignoring any date portion \
         of the string. Parsing depends on the locale and date/time settings of the client \
         computer, so the same text can fail or mean a different time on another machine.",
        &["TIMEVALUE ( \"16:48:23\" )", "TIMEVALUE ( \"4:48:47 PM\" )"],
    ),
    spec(
        "TODAY",
        DateAndTime,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::DateTime,
        FunctionFlags::new(false, true, false, true, true),
        "TODAY () returns the current date at 12:00:00 AM. Volatile: it is evaluated once per \
         query and only changes when the model or the containing calculated column is refreshed, \
         not continuously.",
        &["TODAY ()", "TODAY () - Sales[Order Date]"],
    ),
    spec(
        "UTCNOW",
        DateAndTime,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::DateTime,
        FunctionFlags::new(false, true, false, true, true),
        "UTCNOW () returns the current date and time expressed in Coordinated Universal Time. \
         Volatile: it is evaluated once per query. The Power BI Service always runs on UTC and \
         applies no daylight saving, so NOW and UTCNOW agree there.",
        &["UTCNOW ()", "NOW () - UTCNOW ()"],
    ),
    spec(
        "UTCTODAY",
        DateAndTime,
        0,
        0,
        &[],
        &[overload(&[])],
        ReturnType::DateTime,
        FunctionFlags::new(false, true, false, true, true),
        "UTCTODAY () returns the current date at 12:00:00 AM in Coordinated Universal Time. \
         Volatile: it is evaluated once per query and only changes when the model is refreshed.",
        &["UTCTODAY ()", "TODAY () - UTCTODAY ()"],
    ),
    spec(
        "WEEKDAY",
        DateAndTime,
        1,
        2,
        &[required("Date", ParamKind::Date), optional("ReturnType", ParamKind::Number)],
        &[overload(&[ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "WEEKDAY ( <Date> [, <ReturnType>] ) returns the day of the week. ReturnType 1 (default) \
         numbers Sunday 1 through Saturday 7, 2 numbers Monday 1 through Sunday 7, 3 numbers \
         Monday 0 through Sunday 6, and 11-17 start the week on Monday through Sunday. A text \
         argument is converted with the same rules as DATEVALUE.",
        &["WEEKDAY ( DATE ( 2024, 1, 1 ) )", "WEEKDAY ( Sales[Order Date], 2 )"],
    ),
    spec(
        "WEEKNUM",
        DateAndTime,
        1,
        2,
        &[required("Date", ParamKind::Date), optional("ReturnType", ParamKind::Number)],
        &[overload(&[ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Number])],
        ReturnType::Integer,
        standard(false),
        "WEEKNUM ( <Date> [, <ReturnType>] ) returns the week number in the year. ReturnType 1 \
         (default) and 2-17 use system 1, where the week containing January 1 is week 1; \
         ReturnType 21 uses ISO 8601 system 2, where the week containing the first Thursday is \
         week 1. A text argument is converted with the same rules as DATEVALUE.",
        &["WEEKNUM ( DATE ( 2024, 1, 15 ) )", "WEEKNUM ( Sales[Order Date], 21 )"],
    ),
    spec(
        "YEAR",
        DateAndTime,
        1,
        1,
        &[required("Date", ParamKind::Date)],
        &[overload(&[ParamKind::Date])],
        ReturnType::Integer,
        standard(false),
        "YEAR ( <Date> ) returns the year as a four-digit integer in the range 1900-9999. A text \
         argument is converted with the same rules as DATEVALUE.",
        &["YEAR ( DATE ( 2024, 1, 1 ) )", "YEAR ( Sales[Order Date] )"],
    ),
    spec(
        "YEARFRAC",
        DateAndTime,
        2,
        3,
        &[required("StartDate", ParamKind::Date), required("EndDate", ParamKind::Date), optional("Basis", ParamKind::Number)],
        &[overload(&[ParamKind::Date, ParamKind::Date]), overload(&[ParamKind::Date, ParamKind::Date, ParamKind::Number])],
        ReturnType::Decimal,
        standard(false),
        "YEARFRAC ( <StartDate>, <EndDate> [, <Basis>] ) returns the fraction of a year between \
         two dates. Basis selects the day-count convention: 0 (default) US 30/360, 1 \
         actual/actual, 2 actual/360, 3 actual/365 and 4 European 30/360. Because of a known bug \
         in age calculations, dax.guide suggests INT arithmetic instead.",
        &["YEARFRAC ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ) )", "YEARFRAC ( DATE ( 2024, 1, 1 ), DATE ( 2024, 12, 31 ), 1 )"],
    ),
];
