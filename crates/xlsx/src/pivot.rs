//! PivotTables: pivot cache definitions and records (ECMA-376 Part 1 §18.10.1) and PivotTable
//! definitions (§18.10.1.73), written from the model's [`PivotTable`]s and read back into them.
//!
//! Writing: one cache per PivotTable, with a records snapshot of the source list (omitted, with
//! `saveData="0"`, for very large sources) and `refreshOnLoad="1"` so a reader recomputes the
//! report from the source on open. Fields used on an axis get shared items (or, for date
//! grouping, `fieldGroup` group items) so the table's `<items>` can refer to them. The report
//! cells the engine wrote stay on the sheet as plain cells.
//!
//! Reading: the cache's source and field names plus the table's areas, functions, show-values-as,
//! layout flags, grand totals and style become a model [`PivotTable`]. Anything we don't model
//! (OLAP sources, calculated fields, numeric grouping…) becomes a warning.

use std::collections::HashMap;
use std::fmt::Write as _;

use gridcraft_core::date::datetime_from_serial;
use gridcraft_core::{CellRef, DateSystem, RangeRef, Value, number_to_text};
use gridcraft_formula::quote_sheet;
use gridcraft_model::{
    PivotBaseItem, PivotCalcField, PivotDateGroup, PivotField, PivotFilter, PivotFunc, PivotLayout, PivotShowAs, PivotSort, PivotTable, PivotTopOf,
    PivotValue, PivotValueFilter, PivotValueRule, Workbook,
};

use crate::IoError;
use crate::read::Ctx;
use crate::write::{NS_MAIN, NS_REL, Out, Rels, XML_DECL, abs_range};
use crate::xml::{El, esc_attr};

mod read;
mod write;

pub use read::{CachesIn, read_pivots};
pub use write::PivotWriter;

pub const CT_PIVOT_TABLE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotTable+xml";
pub const CT_PIVOT_CACHE_DEF: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheDefinition+xml";
pub const CT_PIVOT_CACHE_REC: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.pivotCacheRecords+xml";

/// Largest source block scanned for a cache (cells); bigger sources are not written.
const MAX_SOURCE_CELLS: u64 = 10_000_000;
/// Largest records snapshot written (cells); bigger caches are written without records.
const MAX_RECORD_CELLS: u64 = 2_000_000;
/// Most shared items kept per field (Excel's limit is 1,048,576 unique items).
const MAX_ITEMS: usize = 1_048_576;
/// Most cache fields read.
const MAX_FIELDS: usize = 16_384;
/// The `x14:dataField` extension (Excel 2010 show-values-as additions such as rank).
const EXT_X14_DATA_FIELD: &str = "{E15A36E0-9728-4e99-A89B-3F7291B0FE68}";
const EXT_X14_PIVOT_TABLE: &str = "{962EF5D1-5CA2-4c93-8EF4-DBF5C05439D2}";
const NS_X14: &str = "http://schemas.microsoft.com/office/spreadsheetml/2009/9/main";

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

// ================================================================ shared helpers

/// The pivot item label of a value (blank shows as "(blank)").
fn label(v: &Value) -> String {
    match v {
        Value::Empty => "(blank)".into(),
        Value::Number(n) => number_to_text(*n),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(_) => label(&v.scalar()),
    }
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn func_name(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "sum",
        PivotFunc::Count => "count",
        PivotFunc::Average => "average",
        PivotFunc::Max => "max",
        PivotFunc::Min => "min",
        PivotFunc::Product => "product",
        PivotFunc::CountNumbers => "countNums",
        PivotFunc::StdDev => "stdDev",
        PivotFunc::StdDevP => "stdDevp",
        PivotFunc::Var => "var",
        PivotFunc::VarP => "varp",
    }
}

fn func_from(s: &str) -> Option<PivotFunc> {
    Some(match s {
        "sum" => PivotFunc::Sum,
        "count" => PivotFunc::Count,
        "average" => PivotFunc::Average,
        "max" => PivotFunc::Max,
        "min" => PivotFunc::Min,
        "product" => PivotFunc::Product,
        "countNums" => PivotFunc::CountNumbers,
        "stdDev" => PivotFunc::StdDev,
        "stdDevp" => PivotFunc::StdDevP,
        "var" => PivotFunc::Var,
        "varp" => PivotFunc::VarP,
        _ => return None,
    })
}

fn func_caption(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "Sum",
        PivotFunc::Count => "Count",
        PivotFunc::Average => "Average",
        PivotFunc::Max => "Max",
        PivotFunc::Min => "Min",
        PivotFunc::Product => "Product",
        PivotFunc::CountNumbers => "Count",
        PivotFunc::StdDev => "StdDev",
        PivotFunc::StdDevP => "StdDevp",
        PivotFunc::Var => "Var",
        PivotFunc::VarP => "Varp",
    }
}

/// How a show-values-as mode is saved: the `showDataAs` attribute (ECMA-376 `ST_ShowDataAs`)
/// and, for the modes Excel 2010 added, the `x14:dataField pivotShowAs` extension.
fn show_as_xml(mode: PivotShowAs) -> (Option<&'static str>, Option<&'static str>) {
    match mode {
        PivotShowAs::Normal => (None, None),
        PivotShowAs::PercentOfGrandTotal => (Some("percentOfTotal"), None),
        PivotShowAs::PercentOfColumnTotal => (Some("percentOfCol"), None),
        PivotShowAs::PercentOfRowTotal => (Some("percentOfRow"), None),
        PivotShowAs::PercentOf => (Some("percent"), None),
        PivotShowAs::DifferenceFrom => (Some("difference"), None),
        PivotShowAs::PercentDifferenceFrom => (Some("percentDiff"), None),
        PivotShowAs::RunningTotal => (Some("runTotal"), None),
        PivotShowAs::Index => (Some("index"), None),
        PivotShowAs::PercentOfParentRowTotal => (None, Some("percentOfParentRow")),
        PivotShowAs::PercentRunningTotal => (None, Some("percentOfRunningTotal")),
        PivotShowAs::Rank => (None, Some("rankDescending")),
        PivotShowAs::RankAscending => (None, Some("rankAscending")),
    }
}

/// The mode saved as `showDataAs` and the `pivotShowAs` extension (which wins); `Err` names a
/// mode we don't model.
fn show_as_from_xml<'a>(show_data_as: Option<&'a str>, extension: Option<&'a str>) -> Result<PivotShowAs, &'a str> {
    if let Some(ext) = extension {
        return match ext {
            "percentOfParentRow" => Ok(PivotShowAs::PercentOfParentRowTotal),
            "percentOfRunningTotal" => Ok(PivotShowAs::PercentRunningTotal),
            "rankDescending" => Ok(PivotShowAs::Rank),
            "rankAscending" => Ok(PivotShowAs::RankAscending),
            other => Err(other),
        };
    }
    match show_data_as {
        None | Some("normal") => Ok(PivotShowAs::Normal),
        Some("percentOfTotal") => Ok(PivotShowAs::PercentOfGrandTotal),
        Some("percentOfCol") => Ok(PivotShowAs::PercentOfColumnTotal),
        Some("percentOfRow") => Ok(PivotShowAs::PercentOfRowTotal),
        Some("percent") => Ok(PivotShowAs::PercentOf),
        Some("difference") => Ok(PivotShowAs::DifferenceFrom),
        Some("percentDiff") => Ok(PivotShowAs::PercentDifferenceFrom),
        Some("runTotal") => Ok(PivotShowAs::RunningTotal),
        Some("index") => Ok(PivotShowAs::Index),
        Some(other) => Err(other),
    }
}

/// How a value filter is saved: the `<filter type>` and its `<filterColumn>` content (`top10`
/// for Top/Bottom, `customFilters` for comparisons).
fn value_filter_xml(rule: PivotValueRule) -> (&'static str, String) {
    let n = crate::xml::num;
    let top10 = |count: f64, of: PivotTopOf, top: bool| {
        let percent = if of == PivotTopOf::Percent { " percent=\"1\"" } else { "" };
        let bottom = if top { "" } else { " top=\"0\"" };
        format!("<top10{bottom}{percent} val=\"{0}\" filterVal=\"{0}\"/>", n(count))
    };
    let custom = |op: &str, v: f64| format!("<customFilters><customFilter operator=\"{op}\" val=\"{}\"/></customFilters>", n(v));
    let kind = |of: PivotTopOf| match of {
        PivotTopOf::Items => "count",
        PivotTopOf::Percent => "percent",
        PivotTopOf::Sum => "sum",
    };
    match rule {
        PivotValueRule::Top { count, of } => (kind(of), top10(count, of, true)),
        PivotValueRule::Bottom { count, of } => (kind(of), top10(count, of, false)),
        PivotValueRule::Equal(v) => ("valueEqual", custom("equal", v)),
        PivotValueRule::NotEqual(v) => ("valueNotEqual", custom("notEqual", v)),
        PivotValueRule::Greater(v) => ("valueGreaterThan", custom("greaterThan", v)),
        PivotValueRule::GreaterOrEqual(v) => ("valueGreaterThanOrEqual", custom("greaterThanOrEqual", v)),
        PivotValueRule::Less(v) => ("valueLessThan", custom("lessThan", v)),
        PivotValueRule::LessOrEqual(v) => ("valueLessThanOrEqual", custom("lessThanOrEqual", v)),
        PivotValueRule::Between(a, b) => (
            "valueBetween",
            format!(
                "<customFilters and=\"1\"><customFilter operator=\"greaterThanOrEqual\" val=\"{}\"/><customFilter operator=\"lessThanOrEqual\" val=\"{}\"/></customFilters>",
                n(a.min(b)),
                n(a.max(b))
            ),
        ),
        PivotValueRule::NotBetween(a, b) => (
            "valueNotBetween",
            format!(
                "<customFilters><customFilter operator=\"lessThan\" val=\"{}\"/><customFilter operator=\"greaterThan\" val=\"{}\"/></customFilters>",
                n(a.min(b)),
                n(a.max(b))
            ),
        ),
    }
}

/// The value filter a `<filter>` saves; `None` for other kinds (label and date filters) or
/// unreadable thresholds.
fn value_filter_from_xml(filter: &El) -> Option<PivotValueRule> {
    let column = filter.path(&["autoFilter", "filterColumn"]);
    let num = |v: Option<&str>| v.and_then(|t| t.trim().parse::<f64>().ok()).filter(|n| n.is_finite());
    let top10 = column.and_then(|c| c.child("top10"));
    let top = || {
        let t = top10?;
        let count = num(t.attr("val")).or_else(|| num(filter.attr("stringValue1")))?;
        Some((count, t.flag("top", true)))
    };
    let customs: Vec<f64> = column
        .and_then(|c| c.child("customFilters"))
        .map(|cf| cf.kids("customFilter").filter_map(|f| num(f.attr("val"))).collect())
        .unwrap_or_default();
    let first = customs.first().copied().or_else(|| num(filter.attr("stringValue1")));
    let second = customs.get(1).copied().or_else(|| num(filter.attr("stringValue2")));
    let ranked =
        |of: PivotTopOf| top().map(|(count, is_top)| if is_top { PivotValueRule::Top { count, of } } else { PivotValueRule::Bottom { count, of } });
    match filter.attr("type")? {
        "count" => ranked(PivotTopOf::Items),
        "percent" => ranked(PivotTopOf::Percent),
        "sum" => ranked(PivotTopOf::Sum),
        "valueEqual" => first.map(PivotValueRule::Equal),
        "valueNotEqual" => first.map(PivotValueRule::NotEqual),
        "valueGreaterThan" => first.map(PivotValueRule::Greater),
        "valueGreaterThanOrEqual" => first.map(PivotValueRule::GreaterOrEqual),
        "valueLessThan" => first.map(PivotValueRule::Less),
        "valueLessThanOrEqual" => first.map(PivotValueRule::LessOrEqual),
        "valueBetween" => Some(PivotValueRule::Between(first?, second?)),
        "valueNotBetween" => Some(PivotValueRule::NotBetween(first?, second?)),
        _ => None,
    }
}

/// `baseItem` values that stand for the item before and after the cell's own (MS-OI29500 for
/// §18.10.1.22); any other value is an index into the base field's items.
const BASE_ITEM_PREVIOUS: u32 = 1_048_828;
const BASE_ITEM_NEXT: u32 = 1_048_829;

fn group_by_name(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::Years => "years",
        PivotDateGroup::Quarters => "quarters",
        PivotDateGroup::Months => "months",
        PivotDateGroup::Days => "days",
        PivotDateGroup::None => "range",
    }
}

fn group_caption(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::Years => "Years",
        PivotDateGroup::Quarters => "Quarters",
        PivotDateGroup::Months => "Months",
        PivotDateGroup::Days => "Days",
        PivotDateGroup::None => "Group",
    }
}

/// `2024-03-05T00:00:00` for a date serial.
fn iso(sys: DateSystem, serial: f64) -> Option<String> {
    let d = datetime_from_serial(sys, serial)?;
    Some(format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", d.year, d.month, d.day, d.hour, d.minute, d.second))
}

/// `3/5/2024` for a date serial (group boundary captions).
fn mdy(sys: DateSystem, serial: f64) -> String {
    match datetime_from_serial(sys, serial) {
        Some(d) => format!("{}/{}/{}", d.month, d.day, d.year),
        None => number_to_text(serial),
    }
}

// ================================================================ writing

/// Identity of a shared item (text items match case-insensitively, as in Excel).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Blank,
    Num(u64),
    Text(String),
    Bool(bool),
    Err(&'static str),
}

fn key(v: &Value) -> Key {
    match v {
        Value::Empty => Key::Blank,
        Value::Number(n) => Key::Num(if *n == 0.0 { 0 } else { n.to_bits() }),
        Value::Text(t) => Key::Text(t.to_lowercase()),
        Value::Bool(b) => Key::Bool(*b),
        Value::Error(e) => Key::Err(e.as_str()),
        Value::Array(_) => key(&v.scalar()),
    }
}
