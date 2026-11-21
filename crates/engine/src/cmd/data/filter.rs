use super::sort::sort_key;
use super::{
    AutoFilter, CellRef, EngineError, FilterCriterion, Json, Ordering, RangeRef, Result, Session, Sheet, Value, Workbook, bad, bool_param, col_param,
    current_region, data_target, edit, json,
};

// ---------------------------------------------------------------- filter

/// Toggles an AutoFilter on the active table or worksheet selection.
pub(super) fn toggle_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, r) = data_target(s, p, "data.filter")?;
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    // The active cell belongs to the active sheet: on another sheet the table is the one at `range`.
    let at = if sheet == s.doc()?.wb.active_sheet() { s.doc()?.selection.active } else { r.start };
    let has = s.doc()?.wb.sheet(sheet).is_some_and(|sh| sh.autofilter.is_some());
    let in_table = s.doc()?.wb.sheet(sheet).and_then(|sh| sh.table_at(at).map(|t| t.name.clone()));
    if let Some(name) = in_table {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            if let Some(t) = sh.tables.iter_mut().find(|t| t.name == name) {
                t.filter_button = !t.filter_button;
                if !t.filter_button {
                    t.filter.clear();
                }
            }
            Ok(Json::Null)
        });
    }
    if has {
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            // Reveal only rows the filter hid; rows hidden by hand or by a collapsed group stay hidden.
            let filtered: Vec<u32> = sh.rows.keys().copied().filter(|r| sh.is_row_filtered(*r)).collect();
            sh.autofilter = None;
            for r in filtered {
                if let Some(info) = sh.rows.get_mut(&r) {
                    info.hidden = false;
                }
            }
            cx.structural = true;
            Ok(json!({"filter": false}))
        });
    }
    install_filter(s, p)
}

/// Turn a sheet AutoFilter on over `p.range`, or over the data range of the selection.
fn install_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, mut r) = data_target(s, p, "data.filter")?;
    // A selected header row filters the data below it (#201).
    if p.get("range").is_none() && r.height() == 1 {
        let d = s.doc()?;
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        let below = current_region(sh, r.start).end.row;
        if below > r.end.row {
            r = RangeRef::new(r.start, CellRef::new(below, r.end.col));
        }
    }
    edit(s, |cx| {
        cx.sheet_mut(sheet)?.autofilter = Some(AutoFilter { range: r, criteria: vec![] });
        Ok(json!({"filter": true, "range": r.a1()}))
    })
}

/// Applies, updates, or clears an AutoFilter criterion on a column.
pub(super) fn filter_by(s: &mut Session, p: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    let Some(sh) = s.doc()?.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    let af = match &sh.autofilter {
        Some(a) => a.clone(),
        None => {
            install_filter(s, &json!({}))?;
            s.doc()?.wb.sheet(sheet).and_then(|sh| sh.autofilter.clone()).ok_or_else(|| EngineError::Other("no filter".into()))?
        }
    };
    let col = match col_param(p.get("column"), af.range.start.col) {
        Some(c) => c,
        // A number too large to be a column is outside the filter range, not a missing parameter.
        None if p.get("column").is_some_and(|v| v.as_u64().is_some()) => return Err(bad("data.filterBy", "column outside the filter range")),
        None => return Err(bad("data.filterBy", "missing `column`")),
    };
    if col < af.range.start.col || col > af.range.end.col {
        return Err(bad("data.filterBy", "column outside the filter range"));
    }
    let off = col - af.range.start.col;
    let crit = if bool_param(p, "clear").unwrap_or(false) {
        None
    } else if let Some(vals) = p.get("values").and_then(Json::as_array) {
        let mut values: Vec<String> = vals.iter().map(crate::cmd::edit::json_to_input).collect();
        let identities: Vec<String> = p
            .get("identities")
            .and_then(Json::as_array)
            .map(|arr| arr.iter().filter_map(Json::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        // Callers such as programmatic `data.filterBy` pass `values` without `blanks`;
        // since whitespace-only text belongs to the dropdown's `(Blanks)` group, include blanks
        // automatically unless the caller explicitly passed `blanks`.
        let blanks = bool_param(p, "blanks").unwrap_or_else(|| values.iter().any(|v| v.trim().is_empty()));
        values.retain(|v| !v.trim().is_empty());
        Some(FilterCriterion::Values { values, blanks, identities })
    } else if let Some(c) = p.get("custom") {
        let a =
            (c.get("op").and_then(Json::as_str).unwrap_or("=").to_string(), c.get("value").map(crate::cmd::edit::json_to_input).unwrap_or_default());
        let b =
            c.get("op2").and_then(Json::as_str).map(|o| (o.to_string(), c.get("value2").map(crate::cmd::edit::json_to_input).unwrap_or_default()));
        Some(FilterCriterion::Custom { a, b, and: c.get("and").and_then(Json::as_bool).unwrap_or(true) })
    } else if let Some(t) = p.get("top") {
        Some(FilterCriterion::Top10 {
            bottom: t.get("bottom").and_then(Json::as_bool).unwrap_or(false),
            percent: t.get("percent").and_then(Json::as_bool).unwrap_or(false),
            count: t.get("count").and_then(Json::as_u64).unwrap_or(10) as u32,
        })
    } else {
        bool_param(p, "aboveAverage").map(FilterCriterion::AboveAverage)
    };
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(a) = sh.autofilter.as_mut() {
            a.criteria.retain(|(o, _)| *o != off);
            if let Some(c) = crit.clone() {
                a.criteria.push((off, c));
            }
        }
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })?;
    let hidden = s.doc()?.wb.sheet(sheet).map(|sh| sh.rows.values().filter(|r| r.hidden).count()).unwrap_or(0);
    Ok(json!({"hiddenRows": hidden}))
}

/// Does a value pass a custom comparison (`>`, `=*abc*`, `<>x`…)?
fn custom_ok(v: &Value, text: &str, op: &str, crit: &str) -> bool {
    let num = gridcraft_core::parse::parse_number_text(crit);
    let cmp = match (v.as_f64(), num) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        // A numeric ordering comparison only matches numbers; blanks and text never do.
        (None, Some(_)) if matches!(op, "<" | "<=" | ">" | ">=") => None,
        _ => {
            let (lhs, rhs) = if matches!(op, "=" | "<>") { (text.trim(), crit.trim()) } else { (text, crit) };
            Some(gridcraft_core::compare_text(lhs, rhs))
        }
    };
    let wild = || crate::cmd::edit::wildcard(&text.trim().to_lowercase(), &crit.trim().to_lowercase());
    match op {
        "=" => {
            if crit.contains(['*', '?']) {
                wild()
            } else {
                cmp == Some(Ordering::Equal)
            }
        }
        "<>" => {
            if crit.contains(['*', '?']) {
                !wild()
            } else {
                cmp != Some(Ordering::Equal)
            }
        }
        ">" => cmp == Some(Ordering::Greater),
        "<" => cmp == Some(Ordering::Less),
        ">=" => matches!(cmp, Some(Ordering::Greater | Ordering::Equal)),
        "<=" => matches!(cmp, Some(Ordering::Less | Ordering::Equal)),
        "beginsWith" => text.to_lowercase().starts_with(&crit.to_lowercase()),
        "endsWith" => text.to_lowercase().ends_with(&crit.to_lowercase()),
        "contains" => text.to_lowercase().contains(&crit.to_lowercase()),
        "notContains" => !text.to_lowercase().contains(&crit.to_lowercase()),
        _ => true,
    }
}

/// Compare AutoFilter text values, ignoring surrounding spaces.
fn filter_value_eq(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Whether a `Values` criterion includes blank and whitespace-only cells, including legacy saved criteria.
fn values_include_blanks(values: &[String], blanks: bool) -> bool {
    blanks || values.iter().any(|x| x.trim().is_empty())
}

/// Checks whether a non-blank cell matches any selected value criterion via displayed or underlying text.
fn row_matches_values(values: &[String], text: &str, v: &Value) -> bool {
    values.iter().any(|x| filter_value_eq(x, text) || filter_value_eq(x, &v.display()))
}

/// Percent-encodes arbitrary text into ASCII `[A-Za-z0-9_.-%]` so composite checklist identity
/// tokens never collide on `:` delimiters and serialize cleanly inside XML attributes.
fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            out.push(b as char);
        } else {
            let _ = std::fmt::Write::write_fmt(&mut out, format_args!("%{b:02X}"));
        }
    }
    out
}

/// Format-independent, mode-independent `<kind>:<encoded_value>` key of a non-blank cell.
fn typed_value_token(v: &Value) -> String {
    let (kind, norm) = match v {
        Value::Number(n) => {
            let s = if *n == 0.0 { "0".to_string() } else { v.display().trim().to_ascii_lowercase() };
            ("n", s)
        }
        Value::Text(t) => ("t", t.trim().to_ascii_lowercase()),
        Value::Bool(b) => ("b", if *b { "true".to_string() } else { "false".to_string() }),
        _ => ("e", v.display().trim().to_ascii_lowercase()),
    };
    format!("{kind}:{}", pct_encode(&norm))
}

/// Non-hidden formula text of `c` (independent of whether `sh.show_formulas` is currently enabled).
fn visible_formula_text<'a>(wb: &Workbook, sh: &'a Sheet, c: CellRef) -> Option<&'a str> {
    let f = sh.cell(c).and_then(|x| x.formula.as_ref())?;
    (!crate::display::formula_hidden(wb, sh, c)).then_some(f.text.as_str())
}

/// Mode-independent `<kind>:<val>:<eff_disp>` base key of a non-blank cell.
///
/// Cells with distinct non-empty formatted displays (`42` vs `42.00`) stay distinct, whereas a cell
/// whose custom format displays only whitespace falls back to its underlying value (`42`), keeping
/// General `42` stable if later formatted as spaces.
fn cell_base_token(wb: &Workbook, sh: &Sheet, c: CellRef, v: &Value) -> String {
    let typed = typed_value_token(v);
    let vtext = crate::display::value_text_as(wb, v, wb.styles.get(sh.style_id(c)).num_fmt.as_str());
    let eff = if vtext.trim().is_empty() { v.display().trim().to_ascii_lowercase() } else { vtext.trim().to_ascii_lowercase() };
    format!("{typed}:{}", pct_encode(&eff))
}

/// Slicer tile text for `c`: formatted value display (mode-independent of Show Formulas),
/// falling back to the underlying value display only when a non-blank cell's formatted display is empty/whitespace.
pub(crate) fn slicer_tile_text(wb: &Workbook, sh: &Sheet, c: CellRef) -> Option<String> {
    let v = sh.value(c);
    if filter_dropdown_blank(&v) {
        return None;
    }
    let vtext = crate::display::value_text_as(wb, &v, wb.styles.get(sh.style_id(c)).num_fmt.as_str());
    let tile = if vtext.trim().is_empty() { v.display().trim().to_string() } else { vtext.trim().to_string() };
    (!tile.is_empty()).then_some(tile)
}

/// Encodes a slicer tile's display key as an identity token (`s:<encoded_tile>`).
pub(crate) fn slicer_identity_token(tile: &str) -> String {
    format!("s:{}", pct_encode(&tile.trim().to_lowercase()))
}

/// Canonical checklist identity for `c`: `"d:<base>"` when formulas and constants share a value
/// group, `"f:<base>:<formula>"` / `"c:<base>"` when formulas are distinguished, or `"w:<base>:<ws>"`
/// when disambiguating whitespace-formatted cells.
fn cell_identity_token(wb: &Workbook, sh: &Sheet, c: CellRef, v: &Value, shown: &str, split_formulas: bool, split_fmtws: bool) -> String {
    let base = cell_base_token(wb, sh, c, v);
    if split_fmtws && shown.trim().is_empty() {
        return format!("w:{base}:{}", pct_encode(&shown.to_ascii_lowercase()));
    }
    if !split_formulas {
        return format!("d:{base}");
    }
    match visible_formula_text(wb, sh, c) {
        Some(ft) => format!("f:{base}:{}", pct_encode(&ft.trim().to_ascii_lowercase())),
        None => format!("c:{base}"),
    }
}

/// Whether stored checklist identity `id` distinguishes formula vs constant rows for `base`.
fn id_splits_base(id: &str, base: &str) -> bool {
    id.strip_prefix("c:") == Some(base) || id.strip_prefix("f:").and_then(|rest| rest.strip_prefix(base)).is_some_and(|rest| rest.starts_with(':'))
}

/// Checks whether a non-blank cell matches any primary stored identity (`d:`, `c:`, `f:`, `w:`, `s:`).
fn row_matches_primary_identities(wb: &Workbook, sh: &Sheet, c: CellRef, v: &Value, identities: &[String]) -> bool {
    let base = cell_base_token(wb, sh, c, v);
    let mut formula_enc: Option<Option<String>> = None;
    let mut slicer_enc: Option<Option<String>> = None;
    identities.iter().any(|id| {
        if let Some(rest) = id.strip_prefix("d:") {
            rest == base
        } else if let Some(rest) = id.strip_prefix("c:") {
            if rest != base {
                return false;
            }
            formula_enc.get_or_insert_with(|| visible_formula_text(wb, sh, c).map(|ft| pct_encode(&ft.trim().to_ascii_lowercase()))).is_none()
        } else if let Some(rest) = id.strip_prefix("f:") {
            let Some(tail) = rest.strip_prefix(&base).and_then(|s| s.strip_prefix(':')) else {
                return false;
            };
            formula_enc.get_or_insert_with(|| visible_formula_text(wb, sh, c).map(|ft| pct_encode(&ft.trim().to_ascii_lowercase()))).as_deref()
                == Some(tail)
        } else if let Some(rest) = id.strip_prefix("w:") {
            let Some(tail) = rest.strip_prefix(&base).and_then(|s| s.strip_prefix(':')) else {
                return false;
            };
            let shown = crate::display::value_text_as(wb, v, wb.styles.get(sh.style_id(c)).num_fmt.as_str());
            shown.trim().is_empty() && pct_encode(&shown.to_ascii_lowercase()) == tail
        } else if let Some(rest) = id.strip_prefix("s:") {
            slicer_enc.get_or_insert_with(|| slicer_tile_text(wb, sh, c).map(|t| pct_encode(&t.to_lowercase()))).as_deref() == Some(rest)
        } else {
            false
        }
    })
}

/// Checks whether a non-blank cell matches a `Values` criterion: legacy `fmtws:` formatted-space
/// tokens, exact checklist/slicer identities when present, or string matching for `data.filterBy`.
fn row_matches_criterion(wb: &Workbook, sh: &Sheet, c: CellRef, v: &Value, text: &str, values: &[String], identities: &[String]) -> bool {
    let shown = crate::display::value_text_as(wb, v, wb.styles.get(sh.style_id(c)).num_fmt.as_str());
    if identities
        .iter()
        .filter_map(|id| id.strip_prefix("fmtws:"))
        .any(|ws_enc| shown.trim().is_empty() && pct_encode(&shown.to_ascii_lowercase()) == ws_enc)
    {
        return true;
    }
    let has_primary = identities.iter().any(|id| !id.starts_with("fmtws:"));
    if has_primary { row_matches_primary_identities(wb, sh, c, v, identities) } else { row_matches_values(values, text, v) }
}

/// Header-and-data range covered by a table's own AutoFilter (excluding any totals row).
fn table_filter_range(t: &gridcraft_model::Table) -> Option<RangeRef> {
    if !t.header_row {
        return None;
    }
    let end_row = t.range.end.row.checked_sub(u32::from(t.totals_row))?;
    (end_row >= t.range.start.row).then(|| RangeRef::new(t.range.start, CellRef::new(end_row, t.range.end.col)))
}

/// Precomputed per-criterion stats and blank flags for evaluating a sheet or table filter over rows.
struct PreparedFilter<'a> {
    range: RangeRef,
    criteria: &'a [(u32, FilterCriterion)],
    criterion_blanks: Vec<bool>,
    col_stats: std::collections::HashMap<usize, (Option<f64>, f64)>,
}

impl<'a> PreparedFilter<'a> {
    /// Precomputes blank flags, Top10 thresholds, and column averages once before evaluating rows.
    fn new(sh: &Sheet, af_range: RangeRef, criteria: &'a [(u32, FilterCriterion)]) -> Self {
        let range = RangeRef::new(af_range.start, CellRef::new(af_range.end.row.min(gridcraft_core::MAX_ROWS - 1), af_range.end.col));
        let criterion_blanks = criteria
            .iter()
            .map(|(_, c)| match c {
                FilterCriterion::Values { values, blanks, .. } => values_include_blanks(values, *blanks),
                _ => false,
            })
            .collect();
        let mut col_stats = std::collections::HashMap::new();
        for (i, (off, c)) in criteria.iter().enumerate() {
            if matches!(c, FilterCriterion::Top10 { .. } | FilterCriterion::AboveAverage(_)) {
                let Some(col) = range.start.col.checked_add(*off).filter(|&col| col <= range.end.col) else {
                    continue;
                };
                let mut nums: Vec<f64> = (range.start.row + 1..=range.end.row).filter_map(|row| sh.value(CellRef::new(row, col)).as_f64()).collect();
                let avg = if nums.is_empty() { 0.0 } else { nums.iter().sum::<f64>() / nums.len() as f64 };
                let thr = match c {
                    FilterCriterion::Top10 { bottom, percent, count } => {
                        nums.sort_by(|a, b| if *bottom { a.partial_cmp(b) } else { b.partial_cmp(a) }.unwrap_or(Ordering::Equal));
                        let k = if *percent { ((nums.len() as f64) * (*count as f64) / 100.0).ceil() as usize } else { *count as usize };
                        let k = k.clamp(1, nums.len().max(1));
                        nums.get(k - 1).copied()
                    }
                    _ => None,
                };
                col_stats.insert(i, (thr, avg));
            }
        }
        Self { range, criteria, criterion_blanks, col_stats }
    }

    /// Whether `row` passes all criteria of this filter (rows outside `range` are unaffected).
    fn row_passes(&self, wb: &Workbook, sh: &Sheet, row: u32, pre_migration: bool) -> bool {
        if self.criteria.is_empty() || row <= self.range.start.row || row > self.range.end.row {
            return true;
        }
        for (i, (off, crit)) in self.criteria.iter().enumerate() {
            let Some(col) = self.range.start.col.checked_add(*off).filter(|&col| col <= self.range.end.col) else {
                continue;
            };
            let c = CellRef::new(row, col);
            let v = sh.value(c);
            let text = crate::display::cell_text(wb, sh, c);
            let ok = match crit {
                FilterCriterion::Values { values, blanks, identities } => {
                    if pre_migration {
                        if filter_blank(&v) {
                            *blanks
                        } else if filter_dropdown_blank(&v) {
                            *blanks || values.iter().any(|x| x.eq_ignore_ascii_case(&text))
                        } else {
                            row_matches_criterion(wb, sh, c, &v, &text, values, identities)
                        }
                    } else if filter_dropdown_blank(&v) {
                        self.criterion_blanks[i]
                    } else {
                        row_matches_criterion(wb, sh, c, &v, &text, values, identities)
                    }
                }
                FilterCriterion::Custom { a, b, and } => {
                    let x = custom_ok(&v, &text, &a.0, &a.1);
                    match b {
                        Some(b) => {
                            let y = custom_ok(&v, &text, &b.0, &b.1);
                            if *and { x && y } else { x || y }
                        }
                        None => x,
                    }
                }
                FilterCriterion::Top10 { bottom, .. } => match (v.as_f64(), self.col_stats.get(&i)) {
                    (Some(n), Some((thr, _))) => {
                        let thr = thr.unwrap_or(n);
                        if *bottom { n <= thr } else { n >= thr }
                    }
                    _ => false,
                },
                FilterCriterion::AboveAverage(above) => match (v.as_f64(), self.col_stats.get(&i)) {
                    (Some(n), Some((_, avg))) => {
                        if *above {
                            n > *avg
                        } else {
                            n < *avg
                        }
                    }
                    _ => false,
                },
                FilterCriterion::FillColor(col) => wb.styles.get(sh.style_id(c)).fill.fg == *col,
                FilterCriterion::FontColor(col) => wb.styles.get(sh.style_id(c)).font.color == *col,
                // Icons are not evaluated here: the row keeps its current visibility.
                FilterCriterion::Icon { .. } => !sh.is_row_hidden(row),
            };
            if !ok {
                return false;
            }
        }
        true
    }
}

/// Collects all active filter ranges and criteria on `sh` (sheet AutoFilter plus table AutoFilters).
fn collect_sheet_filters(sh: &Sheet) -> Vec<(RangeRef, Vec<(u32, FilterCriterion)>)> {
    let mut out = Vec::new();
    if let Some(af) = &sh.autofilter
        && !af.criteria.is_empty()
    {
        out.push((af.range, af.criteria.clone()));
    }
    for t in &sh.tables {
        if !t.filter.is_empty()
            && let Some(range) = table_filter_range(t)
        {
            out.push((range, t.filter.clone()));
        }
    }
    out
}

/// Migrates legacy saved value criteria that listed whitespace-only strings into `blanks: true`
/// on load, revealing only rows that were excluded by the legacy blank rule and are not excluded
/// by any other filter on the sheet (preserving manually hidden rows).
pub(crate) fn migrate_loaded_autofilters(wb: &mut Workbook) {
    let migrate_list = |criteria: &mut [(u32, FilterCriterion)]| -> bool {
        let mut changed = false;
        for (_, crit) in criteria {
            if let FilterCriterion::Values { values, blanks, identities } = crit
                && values.iter().any(|v| v.trim().is_empty())
            {
                for v in values.iter().filter(|v| v.trim().is_empty()) {
                    let tok = format!("fmtws:{}", pct_encode(&v.to_ascii_lowercase()));
                    if !identities.contains(&tok) {
                        identities.push(tok);
                    }
                }
                *blanks = true;
                values.retain(|v| !v.trim().is_empty());
                changed = true;
            }
        }
        changed
    };
    for si in 0..wb.sheets.len() {
        let Some(before_filters) = wb.sheet(si).map(collect_sheet_filters) else { continue };
        let mut any_changed = false;
        if let Some(sh) = wb.sheet_mut(si) {
            if let Some(af) = sh.autofilter.as_mut() {
                any_changed |= migrate_list(&mut af.criteria);
            }
            for t in &mut sh.tables {
                any_changed |= migrate_list(&mut t.filter);
            }
        }
        if !any_changed {
            continue;
        }
        let unhide: Vec<u32> = if let Some(sh) = wb.sheet(si) {
            let after_specs = collect_sheet_filters(sh);
            let old_prepared: Vec<_> = before_filters.iter().map(|(r, c)| PreparedFilter::new(sh, *r, c)).collect();
            let new_prepared: Vec<_> = after_specs.iter().map(|(r, c)| PreparedFilter::new(sh, *r, c)).collect();
            sh.rows
                .iter()
                .filter(|(_, info)| info.hidden)
                .map(|(&row, _)| row)
                .filter(|&row| {
                    let old_pass = old_prepared.iter().all(|f| f.row_passes(wb, sh, row, true));
                    let new_pass = new_prepared.iter().all(|f| f.row_passes(wb, sh, row, false));
                    !old_pass && new_pass
                })
                .collect()
        } else {
            Vec::new()
        };
        if !unhide.is_empty()
            && let Some(shm) = wb.sheet_mut(si)
        {
            for row in unhide {
                if let Some(info) = shm.rows.get_mut(&row) {
                    info.hidden = false;
                }
            }
        }
    }
}

/// The filter's "(Blanks)" group: truly empty cells and zero-length text such as a `=""` result.
fn filter_blank(v: &Value) -> bool {
    v.is_empty() || matches!(v, Value::Text(t) if t.is_empty())
}

/// Excel AutoFilter's (Blanks) selection also matches whitespace-only text.
/// Do not use this for detecting genuinely empty cells elsewhere.
pub(crate) fn filter_dropdown_blank(v: &Value) -> bool {
    filter_blank(v) || matches!(v, Value::Text(t) if t.trim().is_empty())
}

/// Excel grows an AutoFilter range when a value is entered in the row right below it, in one of its columns (#2342).
/// A table's own filter is left to the table, and a row typed after a blank row does not count.
/// A formula counts before it is calculated: this runs inside the edit, ahead of the recalc.
pub(crate) fn grow_autofilter(sh: &mut Sheet, at: CellRef) {
    // The AutoFilter checks come first: they are cheap, and most sheets have none.
    let Some(af) = sh.autofilter.as_ref() else { return };
    let r = af.range;
    if r.end.row.checked_add(1) != Some(at.row) || at.col < r.start.col || at.col > r.end.col || sh.tables.iter().any(|t| t.range == r) {
        return;
    }
    if !sh.cells.get(at).is_some_and(|c| !filter_blank(&c.value) || c.formula.is_some()) {
        return;
    }
    let grown = RangeRef::new(r.start, CellRef::new(at.row, r.end.col));
    if let Some(af) = sh.autofilter.as_mut() {
        af.range = grown;
    }
}

/// Hides rows of the autofilter range that fail any criterion.
pub(crate) fn apply_filter(wb: &mut Workbook, sheet: usize) {
    let Some(af) = wb.sheet(sheet).and_then(|sh| sh.autofilter.clone()) else { return };
    apply_criteria_range(wb, sheet, af.range, &af.criteria);
}

/// Evaluates `criteria` over `af_range` on `sheet` and updates row visibility.
fn apply_criteria_range(wb: &mut Workbook, sheet: usize, af_range: RangeRef, criteria: &[(u32, FilterCriterion)]) {
    let hide: Vec<(u32, bool)> = if let Some(sh) = wb.sheet(sheet) {
        let prepared = PreparedFilter::new(sh, af_range, criteria);
        (prepared.range.start.row + 1..=prepared.range.end.row).map(|row| (row, !prepared.row_passes(wb, sh, row, false))).collect()
    } else {
        return;
    };
    let Some(shm) = wb.sheet_mut(sheet) else { return };
    for (row, h) in hide {
        if h {
            shm.rows.entry(row).or_default().hidden = true;
        } else if let Some(e) = shm.rows.get_mut(&row) {
            e.hidden = false;
        }
    }
}

/// Clears all active AutoFilter criteria on the active worksheet.
pub(super) fn clear_filter(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        if let Some(a) = sh.autofilter.as_mut() {
            a.criteria.clear();
        }
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// Re-evaluates existing AutoFilter criteria against current cell values.
pub(super) fn reapply(s: &mut Session, _: &Json) -> Result<Json> {
    let sheet = s.doc()?.wb.active_sheet();
    crate::cmd::refuse_unless_allowed(s, sheet, "Filtering", |p| p.autofilter)?;
    edit(s, |cx| {
        apply_filter(&mut cx.wb, sheet);
        cx.structural = true;
        Ok(Json::Null)
    })
}

/// One entry in a column's AutoFilter value checklist: its distinct display `label`, the raw
/// criterion `value` (`None` for the `(Blanks)` toggle), its canonical checklist `identity`, and
/// whether it is `checked`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterChoice {
    pub label: String,
    pub value: Option<String>,
    pub identity: Option<String>,
    pub checked: bool,
}

/// Builds the `data.filterBy` parameters from the AutoFilter dropdown's `(choice, checked)` state.
pub fn filter_choices_params(col: &str, active_checks: &[(&FilterChoice, bool)]) -> Json {
    if active_checks.iter().all(|(_, on)| *on) {
        return json!({"column": col, "clear": true});
    }
    let blanks = active_checks.iter().any(|(c, on)| *on && c.value.is_none());
    let vals: Vec<String> = active_checks.iter().filter(|(_, on)| *on).filter_map(|(c, _)| c.value.clone()).collect();
    let ids: Vec<String> = active_checks.iter().filter(|(_, on)| *on).filter_map(|(c, _)| c.identity.clone()).collect();
    json!({"column": col, "values": vals, "identities": ids, "blanks": blanks})
}

/// Builds the `data.filterBy` parameters for filtering `col` by the selected cell `c` ("Filter by Selected Cell's Value").
pub fn filter_by_cell_params(wb: &Workbook, sh: &Sheet, c: CellRef, col: &str) -> Json {
    let v = sh.value(c);
    if filter_dropdown_blank(&v) {
        return json!({"column": col, "values": [], "blanks": true});
    }
    let t = crate::display::cell_text(wb, sh, c);
    let has_visible_formula = sh.show_formulas && visible_formula_text(wb, sh, c).is_some();
    let raw = if has_visible_formula || t.trim().is_empty() { v.display().trim().to_string() } else { t.trim().to_string() };
    let id = cell_identity_token(wb, sh, c, &v, &t, sh.show_formulas, false);
    json!({"column": col, "values": [raw], "identities": [id], "blanks": false})
}

/// Escapes literal text whose trimmed form is `(Blanks)` or `'...(Blanks)...'` by wrapping one
/// outer pair of single quotes, keeping `(Blanks)`, `'(Blanks)'`, and `''(Blanks)''` distinct.
fn escape_reserved_blanks_label(trimmed: &str) -> String {
    let mut inner = trimmed;
    while inner.len() >= 2 && inner.starts_with('\'') && inner.ends_with('\'') {
        inner = &inner[1..inner.len() - 1];
    }
    if inner.eq_ignore_ascii_case("(Blanks)") { format!("'{trimmed}'") } else { trimmed.to_string() }
}

/// Distinct checklist entries of a filter column, keeping display labels separate from filter identities.
pub fn filter_choices(wb: &Workbook, sheet: usize, col: u32) -> Vec<FilterChoice> {
    let Some(sh) = wb.sheet(sheet) else { return vec![] };
    let Some(af) = &sh.autofilter else { return vec![] };
    let off = col.saturating_sub(af.range.start.col);
    let selected: Option<&FilterCriterion> = af.criteria.iter().find(|(o, _)| *o == off).map(|(_, c)| c);
    let (selected_blanks, selected_identities) = match selected {
        Some(FilterCriterion::Values { values, blanks, identities }) => {
            (values_include_blanks(values, *blanks), (!identities.is_empty()).then_some(identities.as_slice()))
        }
        _ => (false, None),
    };
    type ChoiceBucket = ((u8, i64, String), FilterChoice, (String, &'static str));
    let mut seen: std::collections::BTreeMap<Option<String>, ChoiceBucket> = std::collections::BTreeMap::new();
    for row in af.range.start.row + 1..=af.range.end.row.min(af.range.start.row + 1_000_000) {
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let blank = filter_dropdown_blank(&v);
        let t = if blank { "(Blanks)".to_string() } else { crate::display::cell_text(wb, sh, c) };
        let on = match selected {
            Some(FilterCriterion::Values { values, identities, .. }) => {
                if blank {
                    selected_blanks
                } else {
                    row_matches_criterion(wb, sh, c, &v, &t, values, identities)
                }
            }
            _ => true,
        };
        if blank {
            seen.entry(None).and_modify(|(_, entry, _)| entry.checked |= on).or_insert((
                sort_key(&Value::Empty),
                FilterChoice { label: t, value: None, identity: None, checked: on },
                (String::new(), ""),
            ));
            continue;
        }
        let base = cell_base_token(wb, sh, c, &v);
        let split_formulas = sh.show_formulas || selected_identities.is_some_and(|ids| ids.iter().any(|id| id_splits_base(id, &base)));
        let split_fmtws = true;
        let shown = crate::display::value_text_as(wb, &v, wb.styles.get(sh.style_id(c)).num_fmt.as_str());
        let id = cell_identity_token(wb, sh, c, &v, &shown, split_formulas, split_fmtws);
        let val_trimmed = v.display().trim().to_string();
        let formula_src = visible_formula_text(wb, sh, c);
        let has_visible_formula = sh.show_formulas && formula_src.is_some();
        let trimmed = t.trim();
        let disp_empty = trimmed.is_empty();
        let base_label = if disp_empty { escape_reserved_blanks_label(&val_trimmed) } else { escape_reserved_blanks_label(trimmed) };
        let raw_value = Some(if has_visible_formula || disp_empty { val_trimmed.clone() } else { trimmed.to_string() });
        let sort = if matches!(v, Value::Text(_)) { (1, 0, base_label.to_ascii_lowercase()) } else { sort_key(&v) };
        let kind_label = match v {
            Value::Number(_) => "number",
            Value::Text(_) => "text",
            Value::Bool(_) => "boolean",
            _ => "error",
        };
        let qualifier = if has_visible_formula {
            format!("result: {val_trimmed}")
        } else if split_formulas && let Some(ft) = formula_src {
            format!("formula: ={ft}")
        } else if disp_empty {
            "formatted blank".to_string()
        } else if !val_trimmed.eq_ignore_ascii_case(base_label.trim()) {
            format!("value: {val_trimmed}")
        } else {
            kind_label.to_string()
        };
        seen.entry(Some(id.clone())).and_modify(|(_, entry, _)| entry.checked |= on).or_insert((
            sort,
            FilterChoice { label: base_label, value: raw_value, identity: Some(id), checked: on },
            (qualifier, kind_label),
        ));
    }
    let mut values: Vec<_> = seen.into_iter().collect();
    values.sort_by(|(id_a, (sort_a, choice_a, _)), (id_b, (sort_b, choice_b, _))| {
        sort_a
            .0
            .cmp(&sort_b.0)
            .then(sort_a.1.cmp(&sort_b.1))
            .then_with(|| gridcraft_core::compare_text(&sort_a.2, &sort_b.2))
            .then_with(|| gridcraft_core::compare_text(&choice_a.label, &choice_b.label))
            .then_with(|| id_a.cmp(id_b))
    });
    let mut label_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, (_, choice, _)) in &values {
        *label_counts.entry(choice.label.clone()).or_insert(0) += 1;
    }
    let mut qualified: Vec<(FilterChoice, &'static str)> = values
        .into_iter()
        .map(|(_, (_, mut choice, (qualifier, kind_label)))| {
            if label_counts.get(&choice.label).copied().unwrap_or(0) > 1 && !qualifier.is_empty() {
                choice.label = format!("{} ({qualifier})", choice.label);
            }
            (choice, kind_label)
        })
        .collect();
    let mut qualified_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (choice, _) in &qualified {
        *qualified_counts.entry(choice.label.clone()).or_insert(0) += 1;
    }
    for (choice, kind_label) in &mut qualified {
        if qualified_counts.get(&choice.label).copied().unwrap_or(0) > 1 && !kind_label.is_empty() {
            if let Some(stem) = choice.label.strip_suffix(')') {
                choice.label = format!("{stem}, {kind_label})");
            } else {
                choice.label = format!("{} ({kind_label})", choice.label);
            }
        }
    }
    qualified.into_iter().map(|(choice, _)| choice).collect()
}

/// Distinct display values of a filter column (for the dropdown list).
pub fn filter_values(wb: &Workbook, sheet: usize, col: u32) -> Vec<(String, bool)> {
    filter_choices(wb, sheet, col).into_iter().map(|c| (c.label, c.checked)).collect()
}
