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

/// `r` minus `cut`, as up to four non-overlapping rectangles.
fn subtract_range(r: RangeRef, cut: &RangeRef) -> Vec<RangeRef> {
    let Some(i) = r.intersection(cut) else { return vec![r] };
    let mut out = Vec::new();
    if i.start.row > r.start.row {
        out.push(RangeRef::new(r.start, CellRef::new(i.start.row - 1, r.end.col)));
    }
    if i.end.row < r.end.row {
        out.push(RangeRef::new(CellRef::new(i.end.row + 1, r.start.col), r.end));
    }
    if i.start.col > r.start.col {
        out.push(RangeRef::new(CellRef::new(i.start.row, r.start.col), CellRef::new(i.end.row, i.start.col - 1)));
    }
    if i.end.col < r.end.col {
        out.push(RangeRef::new(CellRef::new(i.start.row, i.end.col + 1), CellRef::new(i.end.row, r.end.col)));
    }
    out
}

/// Removes the cells of `cuts` from every validation, keeping the rest of each rule's ranges.
pub(in super::super) fn remove_validation_cells(validations: &mut Vec<Validation>, cuts: &[RangeRef]) {
    for dv in validations.iter_mut() {
        for cut in cuts {
            dv.ranges = dv.ranges.iter().flat_map(|r| subtract_range(*r, cut)).collect();
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

/// List entries of a list validation (literal or from a range).
pub fn list_items(wb: &Workbook, sheet: usize, dv: &Validation) -> Vec<String> {
    let f = dv.f1.trim();
    if is_literal_list(f) {
        // A quoted list is a string literal: `""` inside stands for one `"`.
        let items = match f.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
            Some(body) => body.replace("\"\"", "\""),
            None => f.trim_matches('"').to_string(),
        };
        return items.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    }
    let at = dv.ranges.first().map(|r| r.start).unwrap_or_default();
    match gridcraft_calc::evaluate(wb, sheet, at, f) {
        Value::Array(a) => a.data.iter().filter(|v| !v.is_empty()).map(Value::display).collect(),
        v if !v.is_empty() && !v.is_error() => vec![v.display()],
        _ => vec![],
    }
}

/// Most entries a searched validation list returns (lists can come from huge ranges).
pub const LIST_MATCH_CAP: usize = 10_000;

/// Text folded for list search: lower case, with the accents of Latin letters removed
/// (`É` → `e`, `ß` → `ss`, `Œ` → `oe`, `Ộ` → `o`) and combining marks dropped. Covers Latin-1,
/// Latin Extended-A and -B and Latin Extended Additional; other scripts keep their marks.
pub fn search_fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars().flat_map(char::to_lowercase) {
        match fold_latin(ch) {
            Some(f) => out.push_str(f),
            None => out.push(ch),
        }
    }
    out
}

/// The unaccented spelling of a lower-case Latin letter (`é` → `e`, `ß` → `ss`, `ộ` → `o`);
/// combining marks fold to nothing. `None` leaves the character as it is.
fn fold_latin(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{300}'..='\u{36f}' => "",
        'à'..='å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ð' | 'ď' | 'đ' => "d",
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'þ' => "th",
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        // Latin Extended-B and Latin Extended Additional (Vietnamese, pinyin tones, …): the base
        // letter of each canonical decomposition (Unicode 15).
        'ǎ' | 'ǟ' | 'ǡ' | 'ǻ' | 'ȁ' | 'ȃ' | 'ȧ' | 'ḁ' | 'ạ' | 'ả' | 'ấ' | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' | 'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' => {
            "a"
        }
        'ḃ' | 'ḅ' | 'ḇ' => "b",
        'ḉ' => "c",
        'ḋ' | 'ḍ' | 'ḏ' | 'ḑ' | 'ḓ' => "d",
        'ȅ' | 'ȇ' | 'ȩ' | 'ḕ' | 'ḗ' | 'ḙ' | 'ḛ' | 'ḝ' | 'ẹ' | 'ẻ' | 'ẽ' | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => "e",
        'ḟ' => "f",
        'ǧ' | 'ǵ' | 'ḡ' => "g",
        'ȟ' | 'ḣ' | 'ḥ' | 'ḧ' | 'ḩ' | 'ḫ' | 'ẖ' => "h",
        'ǐ' | 'ȉ' | 'ȋ' | 'ḭ' | 'ḯ' | 'ỉ' | 'ị' => "i",
        'ǰ' => "j",
        'ǩ' | 'ḱ' | 'ḳ' | 'ḵ' => "k",
        'ḷ' | 'ḹ' | 'ḻ' | 'ḽ' => "l",
        'ḿ' | 'ṁ' | 'ṃ' => "m",
        'ǹ' | 'ṅ' | 'ṇ' | 'ṉ' | 'ṋ' => "n",
        'ơ' | 'ǒ' | 'ǫ' | 'ǭ' | 'ȍ' | 'ȏ' | 'ȫ' | 'ȭ' | 'ȯ' | 'ȱ' | 'ṍ' | 'ṏ' | 'ṑ' | 'ṓ' | 'ọ' | 'ỏ' | 'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ớ' | 'ờ'
        | 'ở' | 'ỡ' | 'ợ' => "o",
        'ṕ' | 'ṗ' => "p",
        'ȑ' | 'ȓ' | 'ṙ' | 'ṛ' | 'ṝ' | 'ṟ' => "r",
        'ṡ' | 'ṣ' | 'ṥ' | 'ṧ' | 'ṩ' => "s",
        'ṫ' | 'ṭ' | 'ṯ' | 'ṱ' | 'ẗ' => "t",
        'ư' | 'ǔ' | 'ǖ' | 'ǘ' | 'ǚ' | 'ǜ' | 'ȕ' | 'ȗ' | 'ṳ' | 'ṵ' | 'ṷ' | 'ṹ' | 'ṻ' | 'ụ' | 'ủ' | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => {
            "u"
        }
        'ṽ' | 'ṿ' => "v",
        'ẁ' | 'ẃ' | 'ẅ' | 'ẇ' | 'ẉ' | 'ẘ' => "w",
        'ẋ' | 'ẍ' => "x",
        'ȳ' | 'ẏ' | 'ẙ' | 'ỳ' | 'ỵ' | 'ỷ' | 'ỹ' => "y",
        'ẑ' | 'ẓ' | 'ẕ' => "z",
        _ => return None,
    })
}

/// The search term as typed, folded for [`search_matches`]. Leading spaces are ignored.
pub fn search_term(typed: &str) -> String {
    search_fold(typed.trim_start())
}

/// Whether a folded list entry matches a folded search term, as Excel's in-cell list search:
/// the term appears as one piece that starts at the beginning of a word ("United St" finds
/// "Los Angeles - United States", "geles" and "United Amer" don't).
pub fn search_matches(folded_item: &str, folded_term: &str) -> bool {
    if folded_term.is_empty() {
        return true;
    }
    let mut prev: Option<char> = None;
    for (i, ch) in folded_item.char_indices() {
        if prev.is_none_or(|p| !p.is_alphanumeric()) && folded_item.get(i..).is_some_and(|rest| rest.starts_with(folded_term)) {
            return true;
        }
        prev = Some(ch);
    }
    false
}

/// Indices of the list entries matching `typed`, in list order, at most `cap` of them.
pub fn filter_list_items(items: &[String], typed: &str, cap: usize) -> Vec<usize> {
    let term = search_term(typed);
    items.iter().enumerate().filter(|(_, it)| search_matches(&search_fold(it), &term)).map(|(i, _)| i).take(cap).collect()
}

/// `data.validationList`: a cell's list validation entries, searched like the in-cell dropdown,
/// at most [`LIST_MATCH_CAP`] of them.
pub(super) fn validation_list_cmd(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let at = cell_param(s, p, "cell")?.unwrap_or(d.selection.active);
    let sheet = d.wb.active_sheet();
    let dv =
        d.wb.sheet(sheet)
            .and_then(|sh| sh.validations.iter().find(|v| v.kind == ValidationKind::List && v.ranges.iter().any(|r| r.contains(at))))
            .ok_or_else(|| EngineError::Other(format!("{} has no list validation", at.a1())))?;
    let items = list_items(&d.wb, sheet, dv);
    let picked: Vec<usize> = match str_param(p, "search") {
        Some(term) => filter_list_items(&items, term, LIST_MATCH_CAP.saturating_add(1)),
        None => (0..items.len().min(LIST_MATCH_CAP.saturating_add(1))).collect(),
    };
    let truncated = picked.len() > LIST_MATCH_CAP;
    let out: Vec<&String> = picked.iter().take(LIST_MATCH_CAP).filter_map(|&i| items.get(i)).collect();
    Ok(json!({"items": out, "truncated": truncated}))
}

/// Checks typed input against the validation of a cell. `Ok(None)` = valid.
pub fn check_validation(wb: &Workbook, sheet: usize, at: CellRef, input: &str) -> Option<(Validation, String)> {
    let sh = wb.sheet(sheet)?;
    let dv = sh.validations.iter().find(|d| d.ranges.iter().any(|r| r.contains(at)))?;
    if dv.kind == ValidationKind::Any || !dv.show_error {
        return None;
    }
    if input.is_empty() && dv.allow_blank {
        return None;
    }
    let v = gridcraft_core::parse::parse_input(input, wb.date_system).value;
    // The formulas are written for the first cell of the range and see the value being entered,
    // as in Excel (a custom `=A1>10` on A1:A9 checks A5's new value at A5).
    let first = dv.ranges.first().map(|r| r.start).unwrap_or(at);
    let entered;
    let wb = if sh.value(at) == v {
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
    let eval = |f: &str| gridcraft_calc::evaluate(wb, sheet, at, &formula_at(f, first, at));
    let num = |f: &str| eval(f).to_number().ok();
    let ok = match dv.kind {
        ValidationKind::List => list_items(wb, sheet, dv).iter().any(|x| x.eq_ignore_ascii_case(input)),
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
    };
    if ok {
        return None;
    }
    let msg = if dv.error_message.is_empty() {
        "This value doesn't match the data validation restrictions defined for this cell.".to_string()
    } else {
        dv.error_message.clone()
    };
    Some((dv.clone(), msg))
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
