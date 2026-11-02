use super::{
    Cell, CellRef, EngineError, Formula, Json, MAX_ROWS, RangeRef, Result, Session, StyleId, Value, bad, bool_param, cell_param_on, edit, json,
    locked_cell_error, str_param, target_range, target_ranges, target_sheet,
};

// ---------------------------------------------------------------- entry

/// Whether typing `input` into a cell enters a formula: `=…`, or `+…`/`-…` followed by a name or
/// a parenthesis (not a number), as Excel takes `+SUM(1)`.
pub(crate) fn is_formula_input(input: &str) -> bool {
    input.starts_with('=')
        || (input.len() > 1
            && (input.starts_with('+') || input.starts_with('-'))
            && gridcraft_core::parse::parse_number_text(input).is_none()
            && input.chars().nth(1).is_some_and(|c| c.is_ascii_alphabetic() || c == '('))
}

/// What typing `input` into a cell produces (constant or formula, plus an automatic number
/// format). Inherits the destination's cell, row or column style, including its number
/// format, before parsing. Errors when the formula can't be parsed.
pub(crate) fn input_to_cell(input: &str, sheet: usize, at: CellRef, wb: &mut gridcraft_model::Workbook) -> Result<Option<Cell>> {
    let style = wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
    if input.is_empty() {
        let c = Cell { value: Value::Empty, formula: None, style };
        return Ok(if c.is_blank() { None } else { Some(c) });
    }
    let fmt_is_text = wb.styles.get(style).num_fmt.as_str() == "@";
    if fmt_is_text {
        return Ok(Some(Cell { value: Value::text(input), formula: None, style }));
    }
    if is_formula_input(input) {
        let body = input.strip_prefix('=').unwrap_or(input);
        let expr = wb.name_call_case(parse_typed_formula(body)?);
        check_linked_books(&expr, wb.external_books.len())?;
        check_arg_counts(&expr)?;
        let mut cell = Cell::formula(Formula::from_expr(expr));
        cell.style = style;
        // Formulas whose result is a date/time get a format from functions like TODAY().
        if wb.styles.get(style).num_fmt.as_str() == "General" {
            let f = cell.formula.as_ref().map(|f| f.text.to_ascii_uppercase()).unwrap_or_default();
            let auto = if f.starts_with("TODAY(")
                || f.starts_with("DATE(")
                || f.starts_with("EDATE(")
                || f.starts_with("EOMONTH(")
                || f.starts_with("WORKDAY(")
            {
                Some("m/d/yyyy")
            } else if f.starts_with("NOW(") {
                Some("m/d/yyyy h:mm")
            } else if f.starts_with("TIME(") {
                Some("h:mm AM/PM")
            } else {
                None
            };
            if let Some(code) = auto {
                cell.style = wb.styles.derive(style, |s| s.num_fmt = gridcraft_model::NumFmt::new(code));
            }
        }
        return Ok(Some(cell));
    }
    let mut parsed = gridcraft_core::parse::parse_input(input, wb.date_system);
    // Like Excel's automatic percent entry: a bare number of 1 or more typed into a cell already
    // formatted as a percentage is a percent (3 -> 3%); smaller numbers are taken as fractions.
    if parsed.format.is_none()
        && !input.contains('%')
        && wb.styles.get(style).num_fmt.as_str().ends_with('%')
        && let Value::Number(n) = parsed.value
        && n.abs() >= 1.0
    {
        parsed.value = Value::Number(n / 100.0);
    }
    let mut style = style;
    if let Some(code) = parsed.format {
        let cur = wb.styles.get(style).num_fmt.as_str().to_string();
        if cur == "General" {
            style = wb.styles.derive(style, |s| s.num_fmt = gridcraft_model::NumFmt::new(code));
        }
    }
    // Wrap text automatically when the input has line breaks (Alt+Enter).
    if input.contains('\n') && !wb.styles.get(style).align.wrap {
        style = wb.styles.derive(style, |s| s.align.wrap = true);
    }
    Ok(Some(Cell { value: parsed.value, formula: None, style }))
}

/// The workbook a sheet name (or the sheet part of a qualified name) is in: `Ok(None)` = this one,
/// `Ok(Some(n))` = `[n]Prices`, linked workbook `n` (`[0]` = this one). `Err` = a bracketed prefix
/// that names no workbook (`[-1]`, `[Book2.xlsx]`, `[ 1]`): a sheet name can't contain `[` or `]`,
/// and a formula names a linked workbook only by its index.
fn sheet_book(s: &str) -> std::result::Result<Option<usize>, String> {
    if !s.contains(['[', ']']) {
        return Ok(None);
    }
    let bad = || s.find(']').and_then(|i| s.get(..=i)).unwrap_or(s).to_string();
    let Some((book, rest)) = s.strip_prefix('[').and_then(|x| x.split_once(']')) else { return Err(bad()) };
    if book.is_empty() || !book.bytes().all(|b| b.is_ascii_digit()) || rest.contains(['[', ']']) {
        return Err(bad());
    }
    book.parse().map(Some).map_err(|_| bad())
}

/// The sheets an expression names, as written: both ends of a sheet range (`'[1]Jan:Dec'`) and the
/// sheet part of qualified names (`[1]!Rates`, `[1]Prices!Rates`). `true` = the second sheet of a
/// range, which can't carry a workbook index (MS-XLSX `sheet-range`).
fn sheets_named(e: &gridcraft_formula::Expr) -> Vec<(&str, bool)> {
    use gridcraft_formula::{Expr, SheetSel};
    match e {
        Expr::Ref(gridcraft_formula::Reference { sheet, .. }) | Expr::LinkedRefError(sheet) => match sheet {
            SheetSel::Named(a) => vec![(a.as_str(), false)],
            SheetSel::Span(a, b) => vec![(a.as_str(), false), (b.as_str(), true)],
            SheetSel::Current => vec![],
        },
        Expr::Name(n) => n.rsplit_once('!').map(|(s, _)| vec![(s, false)]).unwrap_or_default(),
        _ => vec![],
    }
}

/// The first workbook prefix in `expr` that isn't `[0]` or one of `links` linked workbooks
/// (`[1]` is the first), as written (`[5]`, `[Book2.xlsx]`).
fn first_bad_book(expr: &gridcraft_formula::Expr, links: usize) -> Option<String> {
    let mut bad = None;
    expr.walk(&mut |e| {
        for (s, range_end) in sheets_named(e) {
            if bad.is_some() {
                return;
            }
            bad = match sheet_book(s) {
                Ok(None) => None,
                Ok(Some(n)) if n <= links && !range_end => None,
                Ok(Some(_)) => s.find(']').and_then(|i| s.get(..=i)).map(str::to_string),
                Err(prefix) => Some(prefix),
            };
        }
    });
    bad
}

/// A cell reference after a bare workbook index (`[1]!A1`, `SUM([1]!A1:B2)`, `[0]!A1`), as written.
/// Only a defined name can follow `[n]!`: the reference would be to a sheet called `[1]`, which no
/// sheet can be.
fn first_book_only_ref(expr: &gridcraft_formula::Expr) -> Option<String> {
    use gridcraft_formula::{Expr, SheetSel};
    let book_only = |s: &str| matches!(sheet_book(s), Ok(Some(_))) && s.ends_with(']');
    let mut bad = None;
    expr.walk(&mut |e| {
        let sheet = match e {
            Expr::Ref(gridcraft_formula::Reference { sheet: SheetSel::Named(a), .. }) | Expr::LinkedRefError(SheetSel::Named(a)) => Some(a.as_str()),
            _ => None,
        };
        if bad.is_none()
            && let Some(s) = sheet.filter(|s| book_only(s))
        {
            bad = Some(s.to_string());
        }
    });
    bad
}

/// Typed references into other workbooks. `[n]` must name one of the workbook's `links` external
/// links (`[1]` is the first): Excel refuses any other index on entry, and a file saved with one
/// can't be opened. `[0]` is this workbook, as in Excel (the parser enters `[0]Sheet1!A1` as
/// `Sheet1!A1`). Only a defined name can follow a bare `[n]!`.
pub(crate) fn check_linked_books(expr: &gridcraft_formula::Expr, links: usize) -> Result<()> {
    if let Some(book) = first_bad_book(expr, links) {
        return Err(EngineError::Other(format!("There's a problem with this formula: {book} isn't a linked workbook.")));
    }
    if let Some(book) = first_book_only_ref(expr) {
        return Err(EngineError::Other(format!("There's a problem with this formula: only a defined name can follow {book}!.")));
    }
    Ok(())
}

/// Conditional formats and data validation can't refer to other workbooks: Excel refuses it in
/// those dialogs. Text that isn't a formula is left to the caller.
pub(crate) fn refuse_linked_books(text: &str) -> Result<()> {
    match gridcraft_formula::parse(text.trim_start_matches('=')) {
        Ok(e) if first_bad_book(&e, 0).is_some() || first_book_only_ref(&e).is_some() => {
            Err(EngineError::Other("There's a problem with this formula: references to other workbooks can't be used here.".into()))
        }
        _ => Ok(()),
    }
}

/// Which of the source workbook's links (`[1]` first) the target holds at the same index: the same
/// linked workbook with the same saved sheets and names. A New Window of the workbook shares them.
pub(crate) fn shared_links(src: &gridcraft_model::Workbook, dst: &gridcraft_model::Workbook) -> Vec<bool> {
    if std::sync::Arc::ptr_eq(&src.external_books, &dst.external_books) {
        return vec![true; src.external_books.len()];
    }
    src.external_books.iter().enumerate().map(|(i, b)| dst.external_books.get(i) == Some(b)).collect()
}

/// A formula pasted from another workbook: its references into links the target doesn't share
/// (`shared`, from [`shared_links`]) are `#REF!`. `[1]` there would be a link it doesn't have, or a
/// different workbook.
pub(crate) fn drop_linked_books(expr: gridcraft_formula::Expr, shared: &[bool]) -> gridcraft_formula::Expr {
    use gridcraft_formula::Expr;
    if shared.iter().all(|&s| s) && first_bad_book(&expr, shared.len()).is_none() {
        return expr;
    }
    expr.map(&mut |e| {
        let unshared = sheets_named(&e).iter().any(|(s, _)| match sheet_book(s) {
            Ok(None | Some(0)) => false,
            Ok(Some(n)) => !shared.get(n - 1).copied().unwrap_or(false),
            Err(_) => true,
        });
        if unshared { Expr::Error(gridcraft_core::CellError::Ref) } else { e }
    })
}

/// Parses a formula as it is typed (without the `=`). Excel closes missing parentheses for you,
/// and refuses a formula that still doesn't parse.
pub(crate) fn parse_typed_formula(body: &str) -> Result<gridcraft_formula::Expr> {
    let parsed = gridcraft_formula::parse_closing_parens(body, gridcraft_formula::parse);
    parsed.map_err(|e| EngineError::Other(format!("There's a problem with this formula: {e}")))
}

/// Like Excel, a formula that calls a built-in function with too few or too many arguments is
/// refused when it's entered, rather than stored to show `#VALUE!`. Functions the registry doesn't
/// hold (special forms like LET, LAMBDA, OFFSET, INDIRECT, SUBTOTAL), defined LAMBDA names and
/// unknown functions are skipped.
pub(crate) fn check_arg_counts(expr: &gridcraft_formula::Expr) -> Result<()> {
    let mut bad = None;
    expr.walk(&mut |e| {
        if bad.is_none()
            && let gridcraft_formula::Expr::Call(name, args) = e
            && let Some(spec) = gridcraft_functions::lookup(name)
            && !spec.accepts(args.len())
        {
            let few = args.len() < spec.min_args;
            bad = Some(format!("{} has too {} arguments. Use {}", spec.name, if few { "few" } else { "many" }, spec.signature));
        }
    });
    match bad {
        Some(msg) => Err(EngineError::Other(msg)),
        None => Ok(()),
    }
}

pub(super) fn cell_set(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let at = match cell_param_on(s, p, "cell", sheet)? {
        Some(c) => c,
        None => s.doc()?.selection.active,
    };
    // Both spellings take a string, number or boolean, as typed (#697).
    let input = p.get("input").or_else(|| p.get("value")).map(json_to_input).ok_or_else(|| bad("cell.set", "missing `input` (or `value`)"))?;
    let array = bool_param(p, "array").unwrap_or(false);
    let d = s.doc()?;
    if let Some(sh) = d.wb.sheet(sheet)
        && sh.is_protected()
        && d.wb.styles.get(sh.style_id(at)).protection.locked
        && let Some(m) = locked_cell_error(sh, at, &d.unlocked_ranges)
    {
        return Err(EngineError::Other(m));
    }
    let active_sheet = s.doc()?.wb.active_sheet();
    edit(s, |cx| {
        if array {
            // Legacy Ctrl+Shift+Enter array formula over the selection, when the selection holds
            // the cell; a cell elsewhere (or on another sheet) gets a one-cell array.
            let selected = cx.sel.current();
            let range = if sheet == active_sheet && selected.contains(at) { selected } else { RangeRef::cell(at) };
            // A table header can't hold an array's value, and a table resizes under it.
            if !range.is_single() && cx.wb.sheet(sheet).is_some_and(|sh| sh.tables.iter().any(|t| t.range.intersects(&range))) {
                return Err(EngineError::Other("Multi-cell array formulas aren't allowed in tables.".into()));
            }
            let mut cell = input_to_cell(&input, sheet, range.start, &mut cx.wb)?.unwrap_or_default();
            if let Some(f) = cell.formula.as_mut() {
                std::sync::Arc::make_mut(f).array = Some(range);
            }
            let sh = cx.sheet_mut(sheet)?;
            // The array takes the whole range: other cells in it lose their contents.
            for c in range.iter().skip(1).take(1_000_000) {
                if let Some(old) = sh.cells.get(c).filter(|x| !x.value.is_empty() || x.formula.is_some()) {
                    let style = old.style;
                    sh.set_cell(c, Cell { style, ..Cell::default() });
                }
            }
            sh.set_cell(range.start, cell);
            cx.touch(sheet, range.start);
            return Ok(Json::Null);
        }
        let cell = input_to_cell(&input, sheet, at, &mut cx.wb)?;
        let sh = cx.sheet_mut(sheet)?;
        match cell {
            Some(c) => sh.set_cell(at, c),
            None => {
                sh.remove_cell(at);
            }
        }
        crate::cmd::data::grow_autofilter(sh, at);
        cx.touch(sheet, at);
        Ok(Json::Null)
    })?;
    let v = s.doc()?.wb.sheet(sheet).map(|sh| sh.value(at)).unwrap_or_default();
    Ok(json!({"cell": at.a1(), "value": v.display()}))
}

pub(crate) fn json_to_input(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::Bool(b) => {
            if *b {
                "TRUE".into()
            } else {
                "FALSE".into()
            }
        }
        Json::Number(n) => n.to_string(),
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// How many converted strings `range.setValues` reports back, and how much of each input and
/// stored text: the report is a diagnostic, so a big write must not put a second copy of its data
/// in the result.
const MAX_REPORTED_CONVERSIONS: usize = 20;
const MAX_REPORTED_CHARS: usize = 32;

/// `text` cut to [`MAX_REPORTED_CHARS`] characters for the conversion report, with `…` when it was
/// cut. Counting characters keeps a multi-byte input whole.
fn clipped(text: &str) -> String {
    let mut out: String = text.chars().take(MAX_REPORTED_CHARS).collect();
    if out.len() < text.len() {
        out.push('…');
    }
    out
}

pub(super) fn range_set_values(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let start = target_range(s, p)?.start;
    let Some(rows) = p.get("values").and_then(Json::as_array) else { return Err(bad("range.setValues", "missing `values` (array of rows)")) };
    // A string is run through typed input by default, which reads it with en-US rules: a caller
    // sending "05.04.2026" from a dd.mm.yyyy export got 4 May, and "2.500" became 2.5, in silence
    // (#2255). `parse: false` stores a string as the text it is; numbers, booleans and formulas
    // are unaffected either way.
    let parse = match p.get("parse") {
        None | Some(Json::Null) => true,
        Some(Json::Bool(b)) => *b,
        Some(other) => return Err(bad("range.setValues", format!("parse {other}: true or false"))),
    };
    if rows.len() > MAX_ROWS as usize {
        return Err(bad("range.setValues", "too many rows"));
    }
    let mut count = 0;
    // The strings typed input read as something other than the text given, for the result: a
    // caller can notice a "05.04.2026" that became a date without reading the file back (#2255).
    let mut converted: Vec<(CellRef, String)> = Vec::new();
    let mut converted_count = 0;
    edit(s, |cx| {
        for (ri, row) in rows.iter().enumerate() {
            let cols: Vec<&Json> = match row.as_array() {
                Some(a) => a.iter().collect(),
                None => vec![row],
            };
            for (ci, v) in cols.iter().enumerate() {
                let Some(at) = start.offset(ri as i64, ci as i64) else { continue };
                let input = json_to_input(v);
                let cell = match v {
                    Json::Number(n) => Some(Cell {
                        value: Value::number(n.as_f64().unwrap_or(0.0)),
                        formula: None,
                        style: cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default(),
                    }),
                    Json::Bool(b) => {
                        Some(Cell { value: Value::Bool(*b), formula: None, style: cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default() })
                    }
                    // Text as given: no date or number reading, and a leading `'` is part of the
                    // text rather than the marker that makes it text. A formula is still a
                    // formula, which is what the ticket asks for and what `=` means in a cell.
                    Json::String(text) if !parse && !text.starts_with('=') => {
                        if text.is_empty() {
                            // An empty string clears the cell, as `null` and the default path do:
                            // an empty text cell is not blank, and a script clearing cells with
                            // `""` would leave cells `COUNTA` counts. The cell keeps its formatting.
                            input_to_cell("", sheet, at, &mut cx.wb)?
                        } else {
                            let mut style = cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
                            // Line breaks still turn on Wrap Text, as typed input does.
                            if text.contains('\n') && !cx.wb.styles.get(style).align.wrap {
                                style = cx.wb.styles.derive(style, |s| s.align.wrap = true);
                            }
                            Some(Cell { value: Value::Text(text.as_str().into()), formula: None, style })
                        }
                    }
                    _ => input_to_cell(&input, sheet, at, &mut cx.wb)?,
                };
                // A string that became a formula was written as one (`=`), which is deliberate;
                // only a number (dates and times included), boolean or error is a reading of the
                // string and worth reporting.
                if let Some(c) = &cell
                    && matches!(v, Json::String(_))
                    && c.formula.is_none()
                    && matches!(c.value, Value::Number(_) | Value::Bool(_) | Value::Error(_))
                {
                    converted_count += 1;
                    if converted.len() < MAX_REPORTED_CONVERSIONS {
                        converted.push((at, input.clone()));
                    }
                }
                let sh = cx.sheet_mut(sheet)?;
                match cell {
                    Some(c) => sh.set_cell(at, c),
                    None => {
                        sh.remove_cell(at);
                    }
                }
                crate::cmd::data::grow_autofilter(sh, at);
                cx.touch(sheet, at);
                count += 1;
            }
        }
        Ok(())
    })?;
    let d = s.doc()?;
    let reported: Vec<Json> =
        d.wb.sheet(sheet)
            .map(|sh| {
                converted
                .iter()
                .map(|(at, input)| json!({"cell": at.a1(), "input": clipped(input), "stored": clipped(&crate::display::cell_text(&d.wb, sh, *at))}))
                .collect()
            })
            .unwrap_or_default();
    Ok(json!({"cells": count, "convertedCount": converted_count, "converted": reported}))
}

pub(super) fn range_fill(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = target_sheet(s, p)?;
    let ranges = target_ranges(s, p)?;
    let input = str_param(p, "input").unwrap_or("").to_string();
    let origin = s.doc()?.selection.active;
    edit(s, |cx| {
        // Each cell gets what typing the input there gives: the automatic number format where
        // it's General, its own format otherwise, text where it's Text. That depends only on
        // the cell's style, so the input is parsed once per style.
        let mut by_style: std::collections::HashMap<StyleId, Option<Cell>> = std::collections::HashMap::new();
        for r in &ranges {
            if r.count() > 2_000_000 {
                return Err(bad("range.fill", "range too large"));
            }
            for at in r.iter() {
                let style = cx.wb.sheet(sheet).map(|sh| sh.style_id(at)).unwrap_or_default();
                let base = match by_style.get(&style) {
                    Some(c) => c.clone(),
                    None => {
                        let c = input_to_cell(&input, sheet, at, &mut cx.wb)?;
                        by_style.insert(style, c.clone());
                        c
                    }
                };
                let mut cell = base.unwrap_or_default();
                if let Some(f) = &cell.formula
                    && let Some(e) = f.expr()
                {
                    let shifted = gridcraft_formula::adjust::shift_relative(e, at.row as i64 - origin.row as i64, at.col as i64 - origin.col as i64);
                    cell.formula = Some(std::sync::Arc::new(f.rewritten(shifted)));
                }
                let sh = cx.sheet_mut(sheet)?;
                sh.set_cell(at, cell);
                crate::cmd::data::grow_autofilter(sh, at);
                cx.touch(sheet, at);
            }
        }
        Ok(Json::Null)
    })
}
