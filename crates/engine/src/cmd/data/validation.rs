use super::{
    Cell, CellRef, CfOperator, EngineError, ErrorStyle, Json, RangeRef, Result, Session, Validation, ValidationKind, Value, Workbook, bad,
    bool_param, cell_param, edit, json, ok, str_param, target_ranges, target_sheet,
};

// ---------------------------------------------------------------- validation

fn op_param(s: Option<&str>) -> CfOperator {
    match s.unwrap_or("between") {
        "notBetween" => CfOperator::NotBetween,
        "equal" => CfOperator::Equal,
        "notEqual" => CfOperator::NotEqual,
        "greater" | "greaterThan" => CfOperator::Greater,
        "less" | "lessThan" => CfOperator::Less,
        "greaterOrEqual" | "greaterThanOrEqual" => CfOperator::GreaterOrEqual,
        "lessOrEqual" | "lessThanOrEqual" => CfOperator::LessOrEqual,
        _ => CfOperator::Between,
    }
}

/// Removes a set of cuts from one validation range in a row sweep, updating the result once
/// instead of fragmenting it again for every cut.
fn subtract_ranges(r: RangeRef, cuts: &[RangeRef]) -> Vec<RangeRef> {
    let mut events = std::collections::BTreeMap::<u64, Vec<((u32, u32), bool)>>::new();
    for cut in cuts {
        let Some(i) = r.intersection(cut) else { continue };
        let columns = (i.start.col, i.end.col);
        events.entry(u64::from(i.start.row)).or_default().push((columns, true));
        events.entry(u64::from(i.end.row) + 1).or_default().push((columns, false));
    }
    if events.is_empty() {
        return vec![r];
    }
    events.entry(u64::from(r.end.row) + 1).or_default();

    let mut active = std::collections::BTreeMap::<(u32, u32), usize>::new();
    let mut out: Vec<RangeRef> = Vec::new();
    let mut last_by_columns = std::collections::BTreeMap::<(u32, u32), usize>::new();
    let mut previous_row = u64::from(r.start.row);
    for (row, changes) in events {
        if previous_row < row {
            let start_row = previous_row as u32;
            let end_row = (row - 1).min(u64::from(r.end.row)) as u32;
            let mut covered = Vec::<(u32, u32)>::new();
            for &(start, end) in active.keys() {
                match covered.last_mut() {
                    Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
                    _ => covered.push((start, end)),
                }
            }
            let mut next_col = u64::from(r.start.col);
            let mut remaining = Vec::new();
            for (start, end) in covered {
                if next_col < u64::from(start) {
                    remaining.push((next_col as u32, start - 1));
                }
                next_col = u64::from(end) + 1;
            }
            if next_col <= u64::from(r.end.col) {
                remaining.push((next_col as u32, r.end.col));
            }
            for (start_col, end_col) in remaining {
                let columns = (start_col, end_col);
                if let Some(&index) = last_by_columns.get(&columns)
                    && out[index].end.row.saturating_add(1) == start_row
                {
                    out[index].end.row = end_row;
                } else {
                    let index = out.len();
                    out.push(RangeRef::new(CellRef::new(start_row, start_col), CellRef::new(end_row, end_col)));
                    last_by_columns.insert(columns, index);
                }
            }
        }
        for (columns, add) in changes {
            if add {
                *active.entry(columns).or_default() += 1;
            } else if let Some(count) = active.get_mut(&columns) {
                *count -= 1;
                if *count == 0 {
                    active.remove(&columns);
                }
            }
        }
        previous_row = row;
    }
    out
}

/// Removes the cells of `cuts` from every validation, keeping the rest of each rule's ranges.
/// When the rule's top-left cell goes, its relative formulas are rewritten for the new top-left
/// cell, so the cells left keep checking the same cells.
pub(in super::super) fn remove_validation_cells(validations: &mut Vec<Validation>, cuts: &[RangeRef]) {
    for dv in validations.iter_mut() {
        let before = RangeRef::top_left(&dv.ranges);
        dv.ranges = dv.ranges.iter().flat_map(|r| subtract_ranges(*r, cuts)).collect();
        if let (Some(from), Some(to)) = (before, RangeRef::top_left(&dv.ranges)) {
            dv.f1 = move_rule_formula(&dv.f1, from, to);
            dv.f2 = dv.f2.take().map(|f| move_rule_formula(&f, from, to));
        }
    }
    validations.retain(|d| !d.ranges.is_empty());
}

pub(super) fn validation(s: &mut Session, p: &Json) -> Result<Json> {
    let ranges = target_ranges(s, p)?;
    let sheet = target_sheet(s, p)?;
    if bool_param(p, "clear").unwrap_or(false) {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            remove_validation_cells(&mut sh.validations, &ranges);
            Ok(Json::Null)
        });
    }
    let Some(kind) = str_param(p, "type") else {
        s.ui_requests.push(crate::UiRequest::Dialog("dataValidation".into(), json!({})));
        return ok();
    };
    let kind = match kind {
        "whole" => ValidationKind::Whole,
        "decimal" => ValidationKind::Decimal,
        "list" => ValidationKind::List,
        "date" => ValidationKind::Date,
        "time" => ValidationKind::Time,
        "textLength" => ValidationKind::TextLength,
        "custom" => ValidationKind::Custom,
        "any" => ValidationKind::Any,
        // An unknown type used to give an "any value" rule, saved as `type="none"`: no validation.
        other => return Err(bad("data.validation", format!("unknown type `{other}` (any, whole, decimal, list, date, time, textLength, custom)"))),
    };
    if kind == ValidationKind::List && str_param(p, "formula1").is_none_or(|f| f.trim().trim_start_matches('=').is_empty()) {
        return Err(bad("data.validation", "a list validation needs `formula1`"));
    }
    const OPERATORS: [&str; 12] = [
        "between",
        "notBetween",
        "equal",
        "notEqual",
        "greater",
        "greaterThan",
        "less",
        "lessThan",
        "greaterOrEqual",
        "greaterThanOrEqual",
        "lessOrEqual",
        "lessThanOrEqual",
    ];
    if let Some(op) = str_param(p, "operator").filter(|op| !OPERATORS.contains(op)) {
        return Err(bad("data.validation", format!("unknown operator `{op}` ({})", OPERATORS.join(", "))));
    }
    let error_style = match str_param(p, "errorStyle") {
        None | Some("stop") => ErrorStyle::Stop,
        Some("warning") => ErrorStyle::Warning,
        Some("information") => ErrorStyle::Information,
        Some(other) => return Err(bad("data.validation", format!("unknown errorStyle `{other}` (stop, warning, information)"))),
    };
    let f1 = str_param(p, "formula1").unwrap_or("");
    let f1 = if kind == ValidationKind::List { list_source(f1) } else { f1.trim_start_matches('=').to_string() };
    let dv = Validation {
        ranges: ranges.clone(),
        kind,
        op: op_param(str_param(p, "operator")),
        f1,
        f2: str_param(p, "formula2").map(|f| f.trim_start_matches('=').to_string()),
        allow_blank: bool_param(p, "ignoreBlank").unwrap_or(true),
        in_cell_dropdown: bool_param(p, "dropdown").unwrap_or(true),
        input_title: str_param(p, "inputTitle").unwrap_or("").into(),
        input_message: str_param(p, "inputMessage").unwrap_or("").into(),
        show_input: true,
        error_title: str_param(p, "errorTitle").unwrap_or("").into(),
        error_message: str_param(p, "errorMessage").unwrap_or("").into(),
        error_style,
        show_error: true,
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        remove_validation_cells(&mut sh.validations, &ranges);
        sh.validations.push(dv.clone());
        Ok(Json::Null)
    })
}

/// Whether a list rule's source is a literal list (`"a,b"`, or `a,b` typed without quotes) rather
/// than a reference or a name to evaluate.
fn is_literal_list(f: &str) -> bool {
    f.starts_with('"')
        || (!f.contains('!')
            && gridcraft_formula::parse(f).map(|e| !matches!(e, gridcraft_formula::Expr::Ref(_) | gridcraft_formula::Expr::Name(_))).unwrap_or(true))
}

/// A list rule's source as a file stores it. A literal list typed without quotes, as in Excel's
/// Source box (`Open,In progress,Closed`), becomes the quoted string Excel saves
/// (`"Open,In progress,Closed"`, inner quotes doubled); unquoted, the file would read it as a
/// formula. A formula (`=…`), a reference, a name or an already quoted list is kept as before.
fn list_source(f: &str) -> String {
    let t = f.trim();
    if t.is_empty() || t.starts_with('=') || t.starts_with('"') || !is_literal_list(t) {
        return f.trim_start_matches('=').to_string();
    }
    format!("\"{}\"", t.replace('"', "\"\""))
}

/// List entries of a list validation (literal or from a range) for the cell `at`. A relative
/// source is written for the rule's top-left cell and read from `at`, as in Excel (`=$B1:$B3` on
/// A1:A9 lists B5:B7 at A5).
pub fn list_items(wb: &Workbook, sheet: usize, at: CellRef, dv: &Validation) -> Vec<String> {
    let f = dv.f1.trim();
    if is_literal_list(f) {
        // A quoted list is a string literal: `""` inside stands for one `"`.
        let items = match f.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            Some(body) => body.replace("\"\"", "\""),
            None => f.trim_matches('"').to_string(),
        };
        return items.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    let first = RangeRef::top_left(&dv.ranges).unwrap_or(at);
    match gridcraft_calc::evaluate(wb, sheet, at, &formula_at(f, first, at)) {
        Value::Array(a) => a.data.iter().filter(|v| !v.is_empty()).map(Value::display).collect(),
        v if !v.is_empty() && !v.is_error() => vec![v.display()],
        _ => vec![],
    }
}

/// Checks typed input against the validation of a cell. `Ok(None)` = valid.
pub fn check_validation(wb: &Workbook, sheet: usize, at: CellRef, input: &str) -> Option<(Validation, String)> {
    let sh = wb.sheet(sheet)?;
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(at)))?;
    if !alerts(dv) || (input.is_empty() && dv.allow_blank) {
        return None;
    }
    let v = gridcraft_core::parse::parse_input(input, wb.date_system).value;
    let entered;
    let after = if sh.value(at) == v {
        wb
    } else {
        let mut w = wb.clone();
        if let Some(sh) = w.sheet_mut(sheet) {
            let style = sh.style_id(at);
            sh.cells.set(at, Cell { value: v.clone(), style, ..Default::default() });
        }
        entered = w;
        &entered
    };
    if accepts(wb, after, sheet, at, dv, input, &v) {
        return None;
    }
    Some((dv.clone(), refusal_message(dv)))
}

/// Whether a rule shows an error alert at all: "Any value" never refuses, and a rule whose Error
/// Alert is switched off lets every entry through.
fn alerts(dv: &Validation) -> bool {
    dv.kind != ValidationKind::Any && dv.show_error
}

/// The message a rule's alert shows: its own, or the default one.
fn refusal_message(dv: &Validation) -> String {
    if dv.error_message.is_empty() {
        "This value doesn't match the data validation restrictions defined for this cell.".to_string()
    } else {
        dv.error_message.clone()
    }
}

/// Whether rule `dv` accepts `input` (parsed as `v`) entered at `at`. `original` is the workbook
/// before the entry, `after` the one with the entry made (the same workbook when the cell already
/// holds `v`).
fn accepts(original: &Workbook, after: &Workbook, sheet: usize, at: CellRef, dv: &Validation, input: &str, v: &Value) -> bool {
    // The formulas are written for the top-left cell of all the areas and see the value being entered,
    // as in Excel (a custom `=A1>10` on A1:A9 checks A5's new value at A5).
    let first = RangeRef::top_left(&dv.ranges).unwrap_or(at);
    let eval = |f: &str| gridcraft_calc::evaluate(after, sheet, at, &formula_at(f, first, at));
    let num = |f: &str| eval(f).to_number().ok();
    match dv.kind {
        // The list is the one the dropdown showed before the entry, as in Excel: a source that takes
        // in the cell itself (`=$A1:$A2` on A2:A3 is A2:A3 at A3) doesn't list the value being typed.
        ValidationKind::List => list_items(original, sheet, at, dv).iter().any(|x| x.eq_ignore_ascii_case(input)),
        ValidationKind::Custom => eval(&dv.f1).to_bool().unwrap_or(false),
        ValidationKind::TextLength => {
            let n = input.chars().count() as f64;
            compare_op(dv.op, n, num(&dv.f1), dv.f2.as_deref().and_then(num))
        }
        _ => match v.as_f64() {
            Some(n) => {
                if dv.kind == ValidationKind::Whole && n.fract() != 0.0 {
                    false
                } else {
                    compare_op(dv.op, n, num(&dv.f1), dv.f2.as_deref().and_then(num))
                }
            }
            None => false,
        },
    }
}

/// Most cells one area of a fill may hold. `range.fill` refuses a larger area, and the fill's
/// validation check doesn't look at one ([`FillValidation::NotChecked`]).
pub const MAX_FILL_AREA: u64 = 2_000_000;

/// A destination of a fill whose rule refuses the input.
#[derive(Clone, Debug)]
pub struct FillAlert {
    pub cell: CellRef,
    pub rule: Validation,
    pub message: String,
}

/// What the data validation of a fill's destinations says about the input (Ctrl+Enter). No write
/// command enforces validation; this is the question the interface asks before it fills.
#[derive(Clone, Debug)]
pub enum FillValidation {
    /// Every destination the fill writes accepts the input, or has no rule that alerts.
    Passed,
    /// The most severe refusal: the first Stop, else the first Warning, else the first
    /// Information, in the order the fill visits the cells.
    Alert(Box<FillAlert>),
    /// An area is larger than [`MAX_FILL_AREA`], so nothing was checked. This is not a pass:
    /// `range.fill` refuses such an area too.
    NotChecked,
}

/// Checks `input` entered in every cell of `ranges` (Ctrl+Enter's fill) against each cell's rule.
/// Only the cells the fill writes are asked: rows an applied filter hides are skipped by
/// `range.fill` (#1175), so their rules don't decide it. A formula is judged per destination as
/// [`check_validation`] judges a formula typed there; any other input with
/// [`check_literal_fill_validation`].
pub fn check_fill_validation(wb: &Workbook, sheet: usize, ranges: &[RangeRef], input: &str) -> FillValidation {
    if !crate::cmd::edit::is_formula_input(input) {
        return check_literal_fill_validation(wb, sheet, ranges, input);
    }
    if ranges.iter().any(|r| r.count() > MAX_FILL_AREA) {
        return FillValidation::NotChecked;
    }
    let Some(sh) = wb.sheet(sheet) else { return FillValidation::Passed };
    // One scratch workbook for every destination, restored to the original cell before the
    // next destination is entered, instead of a fresh workbook (and sheet) clone per cell:
    // each check still sees the original workbook plus only its own entry, exactly as
    // `check_validation` presents it, so the refusal results are unchanged.
    let v = gridcraft_core::parse::parse_input(input, wb.date_system).value;
    let mut scratch: Option<Workbook> = None;
    let mut entered: Option<CellRef> = None;
    let mut refusals: Vec<FillAlert> = Vec::new();
    for (at, _) in fill_destinations(sh, ranges, input) {
        let Some(dv) = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(at))) else { continue };
        if !alerts(dv) || (input.is_empty() && dv.allow_blank) {
            continue;
        }
        let after: &Workbook = if sh.value(at) == v {
            wb
        } else {
            if scratch.is_none() {
                scratch = Some(wb.clone());
            }
            if let Some(out) = scratch.as_mut().and_then(|w| w.sheet_mut(sheet)) {
                if let Some(prev) = entered.take() {
                    let original = sh.cells.get(prev).cloned().unwrap_or_default();
                    out.cells.set(prev, original);
                }
                let style = sh.style_id(at);
                out.cells.set(at, Cell { value: v.clone(), style, ..Default::default() });
            }
            entered = Some(at);
            scratch.as_ref().unwrap_or(wb)
        };
        if !accepts(wb, after, sheet, at, dv, input, &v) {
            refusals.push(FillAlert { cell: at, rule: dv.clone(), message: refusal_message(dv) });
        }
    }
    worst(refusals.into_iter())
}

/// Checks a literal (non-formula) `input` filled into `ranges`. Every destination then holds the
/// same value, so each rule is evaluated once against the workbook as the whole fill leaves it: a
/// rule reading the other filled cells (a uniqueness rule) sees them filled. Rows an applied
/// filter hides are neither written nor asked.
pub fn check_literal_fill_validation(wb: &Workbook, sheet: usize, ranges: &[RangeRef], input: &str) -> FillValidation {
    if ranges.iter().any(|r| r.count() > MAX_FILL_AREA) {
        return FillValidation::NotChecked;
    }
    let Some(sh) = wb.sheet(sheet) else { return FillValidation::Passed };
    let asked = fill_destinations(sh, ranges, input);
    if asked.is_empty() {
        return FillValidation::Passed;
    }
    let v = gridcraft_core::parse::parse_input(input, wb.date_system).value;
    let mut after = wb.clone();
    if let Some(out) = after.sheet_mut(sheet) {
        for r in ranges {
            let skip = crate::cmd::edit::filtered_rows(sh, *r);
            for at in r.iter().filter(|at| !skip.contains(&at.row)) {
                if sh.value(at) != v {
                    let style = sh.style_id(at);
                    out.cells.set(at, Cell { value: v.clone(), style, ..Default::default() });
                }
            }
        }
    }
    worst(asked.into_iter().filter_map(|(at, i)| {
        let dv = sh.validations.get(i)?;
        (!accepts(wb, &after, sheet, at, dv, input, &v)).then(|| FillAlert { cell: at, rule: dv.clone(), message: refusal_message(dv) })
    }))
}

/// The destinations a fill of `input` writes whose rule (the first that holds the cell, as
/// [`check_validation`] finds it) shows an alert for it, with that rule's index, in the order the
/// fill visits them: area by area, row by row. Each cell once.
fn fill_destinations(sh: &gridcraft_model::Sheet, ranges: &[RangeRef], input: &str) -> Vec<(CellRef, usize)> {
    let mut out: Vec<(usize, CellRef, usize)> = Vec::new();
    for (n, r) in ranges.iter().enumerate() {
        let skip = crate::cmd::edit::filtered_rows(sh, *r);
        for (i, dv) in sh.validations.iter().enumerate() {
            if !alerts(dv) || (input.is_empty() && dv.allow_blank) {
                continue;
            }
            for part in dv.ranges.iter().filter_map(|vr| vr.intersection(r)) {
                for at in part.iter().filter(|at| !skip.contains(&at.row)) {
                    if sh.validations.iter().position(|d| d.ranges.iter().any(|x| x.contains(at))) == Some(i) {
                        out.push((n, at, i));
                    }
                }
            }
        }
    }
    out.sort_by_key(|&(n, at, _)| (n, at.row, at.col));
    let mut seen = std::collections::HashSet::new();
    out.into_iter().filter(|&(_, at, _)| seen.insert(at)).map(|(_, at, i)| (at, i)).collect()
}

/// The most severe of the refusals, in order: the first Stop (looking no further), else the first
/// Warning, else the first Information.
fn worst(refusals: impl Iterator<Item = FillAlert>) -> FillValidation {
    let (mut warning, mut information) = (None, None);
    for a in refusals {
        match a.rule.error_style {
            ErrorStyle::Stop => return FillValidation::Alert(Box::new(a)),
            ErrorStyle::Warning => {
                warning.get_or_insert(a);
            }
            ErrorStyle::Information => {
                information.get_or_insert(a);
            }
        }
    }
    warning.or(information).map_or(FillValidation::Passed, |a| FillValidation::Alert(Box::new(a)))
}

/// A formula written for `first`, as seen from `at` (relative references shifted).
fn formula_at(f: &str, first: CellRef, at: CellRef) -> String {
    if first == at {
        return f.to_string();
    }
    match gridcraft_formula::parse(f.trim_start_matches('=')) {
        Ok(e) => gridcraft_formula::print(&gridcraft_formula::adjust::shift_relative(
            e,
            at.row as i64 - first.row as i64,
            at.col as i64 - first.col as i64,
        )),
        Err(_) => f.to_string(),
    }
}

/// Moves the relative references of a rule formula written for `first` so it reads the same cells from `to`
/// (pasting a rule); text without relative references (constants, literal lists) stays as typed.
pub(in super::super) fn move_rule_formula(f: &str, first: CellRef, to: CellRef) -> String {
    if first == to {
        return f.to_string();
    }
    let moved = formula_at(f, first, to);
    let has_relative = gridcraft_formula::parse(f.trim_start_matches('=')).is_ok_and(|e| gridcraft_formula::print(&e) != moved);
    let eq = if f.starts_with('=') { "=" } else { "" };
    if has_relative { format!("{eq}{moved}") } else { f.to_string() }
}

fn compare_op(op: CfOperator, n: f64, a: Option<f64>, b: Option<f64>) -> bool {
    let Some(a) = a else { return false };
    match op {
        CfOperator::Between => b.is_some_and(|b| n >= a.min(b) && n <= a.max(b)),
        CfOperator::NotBetween => b.is_some_and(|b| n < a.min(b) || n > a.max(b)),
        CfOperator::Equal => n == a,
        CfOperator::NotEqual => n != a,
        CfOperator::Greater => n > a,
        CfOperator::Less => n < a,
        CfOperator::GreaterOrEqual => n >= a,
        CfOperator::LessOrEqual => n <= a,
    }
}

pub(super) fn validate_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
    let input = str_param(p, "input").unwrap_or("");
    match check_validation(&d.wb, d.wb.active_sheet(), at, input) {
        None => Ok(json!({"ok": true})),
        Some((dv, msg)) => Ok(json!({"ok": false, "message": msg, "title": dv.error_title, "style": format!("{:?}", dv.error_style)})),
    }
}

pub(super) fn circle_invalid(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let sheet = d.wb.active_sheet();
    let Some(sh) = d.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let mut bad_cells = Vec::new();
    for dv in &sh.validations {
        for r in &dv.ranges {
            let r = sh.used_range().and_then(|u| r.intersection(&u));
            let Some(r) = r else { continue };
            for c in r.iter().take(100_000) {
                let text = sh.input_text(c);
                if !text.is_empty() && check_validation(&d.wb, sheet, c, &text).is_some() {
                    bad_cells.push(c.a1());
                }
            }
        }
    }
    Ok(json!({"invalid": bad_cells}))
}
