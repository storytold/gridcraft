use super::{Json, PivotBaseItem, PivotDateGroup, PivotFunc, PivotLayout, PivotShowAs, PivotSort, Result, bad, u32_param};

pub(super) fn area_param(cmd: &str, v: Option<&str>) -> Result<&'static str> {
    match v.map(str::to_ascii_lowercase) {
        Some(a) if a == "rows" || a == "row" => Ok("rows"),
        Some(a) if a == "columns" || a == "column" || a == "cols" => Ok("columns"),
        Some(a) if a == "values" || a == "value" || a == "data" => Ok("values"),
        Some(a) if a == "filters" || a == "filter" => Ok("filters"),
        Some(a) => Err(bad(cmd, format!("unknown area `{a}` (rows|columns|values|filters)"))),
        None => Err(bad(cmd, "missing `area` (rows|columns|values|filters)")),
    }
}

pub(super) fn func_param(v: Option<&str>) -> Option<PivotFunc> {
    Some(match v?.to_ascii_lowercase().as_str() {
        "sum" => PivotFunc::Sum,
        "count" | "counta" => PivotFunc::Count,
        "average" | "avg" | "mean" => PivotFunc::Average,
        "max" => PivotFunc::Max,
        "min" => PivotFunc::Min,
        "product" => PivotFunc::Product,
        "countnumbers" | "countnums" | "count numbers" => PivotFunc::CountNumbers,
        "stddev" => PivotFunc::StdDev,
        "stddevp" => PivotFunc::StdDevP,
        "var" => PivotFunc::Var,
        "varp" => PivotFunc::VarP,
        _ => return None,
    })
}

pub(super) fn func_name(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "sum",
        PivotFunc::Count => "count",
        PivotFunc::Average => "average",
        PivotFunc::Max => "max",
        PivotFunc::Min => "min",
        PivotFunc::Product => "product",
        PivotFunc::CountNumbers => "countNumbers",
        PivotFunc::StdDev => "stdDev",
        PivotFunc::StdDevP => "stdDevP",
        PivotFunc::Var => "var",
        PivotFunc::VarP => "varP",
    }
}

/// The Show Values As mode a `showAs` parameter names (ignoring case; "none" is Normal).
pub(super) fn show_as_param(v: &str) -> Option<PivotShowAs> {
    if v.eq_ignore_ascii_case("none") {
        return Some(PivotShowAs::Normal);
    }
    PivotShowAs::ALL.into_iter().find(|mode| show_as_name(*mode).eq_ignore_ascii_case(v))
}

/// The `showAs` parameter name of a mode, as `pivot.fields` reports it.
pub(super) fn show_as_name(v: PivotShowAs) -> &'static str {
    match v {
        PivotShowAs::Normal => "normal",
        PivotShowAs::PercentOfGrandTotal => "percentOfGrandTotal",
        PivotShowAs::PercentOfColumnTotal => "percentOfColumnTotal",
        PivotShowAs::PercentOfRowTotal => "percentOfRowTotal",
        PivotShowAs::PercentOfParentRowTotal => "percentOfParentRowTotal",
        PivotShowAs::PercentOf => "percentOf",
        PivotShowAs::DifferenceFrom => "differenceFrom",
        PivotShowAs::PercentDifferenceFrom => "percentDifferenceFrom",
        PivotShowAs::RunningTotal => "runningTotal",
        PivotShowAs::PercentRunningTotal => "percentRunningTotal",
        PivotShowAs::Rank => "rank",
        PivotShowAs::RankAscending => "rankAscending",
        PivotShowAs::Index => "index",
    }
}

/// `baseItem` parameter text for the item before and after the cell's own, as Excel lists them.
pub(super) const BASE_ITEM_PREVIOUS: &str = "(previous)";
pub(super) const BASE_ITEM_NEXT: &str = "(next)";

/// The `baseItem` parameter text of a base item: "(previous)", "(next)" or its label.
pub(super) fn base_item_name(item: &PivotBaseItem) -> &str {
    match item {
        PivotBaseItem::Previous => BASE_ITEM_PREVIOUS,
        PivotBaseItem::Next => BASE_ITEM_NEXT,
        PivotBaseItem::Named(label) => label,
    }
}

pub(super) fn layout_name(l: PivotLayout) -> &'static str {
    match l {
        PivotLayout::Compact => "compact",
        PivotLayout::Outline => "outline",
        PivotLayout::Tabular => "tabular",
    }
}

pub(super) fn group_name(g: PivotDateGroup) -> &'static str {
    match g {
        PivotDateGroup::None => "none",
        PivotDateGroup::Years => "years",
        PivotDateGroup::Quarters => "quarters",
        PivotDateGroup::Months => "months",
        PivotDateGroup::Days => "days",
    }
}

pub(super) fn sort_name(o: PivotSort) -> &'static str {
    match o {
        PivotSort::Asc => "asc",
        PivotSort::Desc => "desc",
        PivotSort::None => "none",
    }
}

// ---------------------------------------------------------------- field commands

pub(super) fn position_param(p: &Json) -> Option<usize> {
    u32_param(p, "position").map(|v| v as usize)
}

pub(super) fn group_param(v: Option<&str>) -> Option<PivotDateGroup> {
    Some(match v?.to_ascii_lowercase().as_str() {
        "years" | "year" => PivotDateGroup::Years,
        "quarters" | "quarter" => PivotDateGroup::Quarters,
        "months" | "month" => PivotDateGroup::Months,
        "days" | "day" => PivotDateGroup::Days,
        "none" | "ungroup" => PivotDateGroup::None,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_show_as_mode_has_a_name_that_reads_back() {
        let mut seen = std::collections::HashSet::new();
        for mode in PivotShowAs::ALL {
            let name = show_as_name(mode);
            assert!(seen.insert(name.to_ascii_lowercase()), "{name} is used twice");
            assert_eq!(show_as_param(name), Some(mode));
            assert_eq!(show_as_param(&name.to_ascii_uppercase()), Some(mode), "names ignore case");
        }
        assert_eq!(show_as_param("none"), Some(PivotShowAs::Normal));
        assert_eq!(show_as_param("percent"), None);
        assert_eq!(show_as_param(""), None);
    }
}
