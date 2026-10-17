use super::sort::sort_key;
use super::{
    AutoFilter, CellRef, EngineError, FilterCriterion, Json, Ordering, RangeRef, Result, Session, Sheet, Value, Workbook, bad, bool_param, col_param,
    data_target, edit, json,
};
use std::collections::{HashMap, HashSet};

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
        // An explicit range over part of the table is refused, as when installing a filter on it.
        if p.get("range").is_some()
            && let Some(sh) = s.doc()?.wb.sheet(sheet)
            && let Some(t) = sh.tables.iter().find(|t| t.name == name)
            && t.range.intersects(&r)
            && !(t.header_row
                && t.range.start.row == r.start.row
                && t.range.contains(r.start)
                && t.range.contains(r.end)
                && r.start.col == t.range.start.col
                && r.end.col == t.range.end.col
                && t.data_range().is_none_or(|data| r.end.row >= data.end.row))
        {
            return Err(bad("data.filter", "a filter range can't overlap a table; filter inside the table instead"));
        }
        return edit(s, |cx| {
            let sh = cx.sheet_mut(sheet)?;
            let filtered: Vec<u32> = sh.rows.keys().copied().filter(|r| sh.is_row_filtered(*r)).collect();
            let mut cleared = false;
            if let Some(t) = sh.tables.iter_mut().find(|t| t.name == name) {
                t.filter_button = !t.filter_button;
                if !t.filter_button {
                    t.filter.clear();
                    // The sheet AutoFilter hosted on this table's header row is the table's filter too.
                    cleared = sh.autofilter.as_ref().is_some_and(|af| af.range.start.row == t.range.start.row && af.range.intersects(&t.range));
                }
            }
            if cleared {
                sh.autofilter = None;
                for r in filtered {
                    if let Some(info) = sh.rows.get_mut(&r) {
                        info.hidden = false;
                    }
                }
                // Rows another table's criteria hide stay hidden: evaluate each such table's saved
                // criteria through the live filter, then hide the union once (a later table would
                // unhide what an earlier one excluded) and take the filter off again.
                let others: Vec<(RangeRef, Vec<(u32, FilterCriterion)>)> =
                    sh.tables.iter().filter(|t| !t.filter.is_empty()).map(|t| (t.range, t.filter.clone())).collect();
                let mut excluded: std::collections::HashSet<u32> = std::collections::HashSet::new();
                for (range, criteria) in others {
                    if let Some(sh) = cx.wb.sheet_mut(sheet) {
                        sh.autofilter = Some(AutoFilter { range, criteria });
                    }
                    apply_filter(&mut cx.wb, sheet);
                    if let Some(sh) = cx.wb.sheet(sheet) {
                        excluded.extend((range.start.row.saturating_add(1)..=range.end.row).filter(|r| sh.is_row_hidden(*r)));
                    }
                }
                if let Some(sh) = cx.wb.sheet_mut(sheet) {
                    sh.autofilter = None;
                    for r in excluded {
                        sh.rows.entry(r).or_default().hidden = true;
                    }
                }
                cx.structural = true;
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
///
/// A sheet AutoFilter never overlaps a table, because a file can't hold one (Excel refuses
/// `Range("A1:C8").AutoFilter` over part of a table, and from a cell outside a table it leaves the
/// table's cells out of the region). Inside a table the filter covers the table from its header
/// row, which is saved as the table's own filter, and takes over the criteria the table already
/// had so there is one filter to save.
fn install_filter(s: &mut Session, p: &Json) -> Result<Json> {
    let (sheet, mut r) = data_target(s, p, "data.filter")?;
    {
        let d = s.doc()?;
        let sh = d.wb.active().ok_or(EngineError::NoDocument)?;
        if p.get("range").is_none() && sh.table_at(d.selection.active).is_none() && d.selection.current().is_single() {
            r = crate::selection::current_region_outside_tables(sh, d.selection.active);
        }
        // A selected header row filters the data below it (#201).
        if p.get("range").is_none() && r.height() == 1 {
            let region = match sh.table_at(r.start) {
                Some(t) => t.data_range().map(|dr| RangeRef::new(t.range.start, dr.end)).unwrap_or(t.range),
                None => crate::selection::current_region_outside_tables(sh, r.start),
            };
            if region.end.row > r.end.row {
                r = RangeRef::new(r.start, CellRef::new(region.end.row, r.end.col));
            }
        }
    }
    let Some(sh) = s.doc()?.wb.sheet(sheet) else { return Err(EngineError::NoDocument) };
    // The table whose header row the filter starts on, when it lies inside that table.
    let host =
        sh.tables.iter().position(|t| t.header_row && t.range.start.row == r.start.row && t.range.contains(r.start) && t.range.contains(r.end));
    if host.is_none() && sh.tables.iter().any(|t| t.range.intersects(&r)) {
        return Err(bad("data.filter", "a filter range can't overlap a table; filter inside the table instead"));
    }
    // A narrower range would leave out the table's criteria on the columns it doesn't cover, and saving would drop them.
    if let Some(t) = host.and_then(|i| sh.tables.get(i)) {
        if t.data_range().is_some_and(|data| r.end.row < data.end.row) {
            return Err(bad("data.filter", "filter the table's full data range"));
        }
        let shift = r.start.col.saturating_sub(t.range.start.col);
        if t.filter.iter().any(|(o, _)| o.checked_sub(shift).is_none_or(|c| c >= r.width())) {
            return Err(bad("data.filter", "the table is filtered on a column this range leaves out; filter the table's full width"));
        }
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        let mut criteria = vec![];
        if let Some(t) = host.and_then(|i| sh.tables.get_mut(i)) {
            // The criteria are saved with the table only while its filter buttons are on.
            t.filter_button = true;
            let shift = r.start.col.saturating_sub(t.range.start.col);
            criteria = std::mem::take(&mut t.filter).into_iter().filter_map(|(o, c)| Some((o.checked_sub(shift)?, c))).collect();
        }
        sh.autofilter = Some(AutoFilter { range: r, criteria });
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

/// Character-wise Unicode lowercase key matching `gridcraft_core::compare_text`'s final tie-breaker
/// (avoiding `str::to_lowercase`'s context-sensitive Greek final sigma).
fn filter_case_key(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
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

/// Whether a `Values` criterion includes blank and whitespace-only cells, including legacy saved criteria.
fn values_include_blanks(values: &[String], blanks: bool) -> bool {
    blanks || values.iter().any(|x| x.trim().is_empty())
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

/// Decodes `pct_encode` output; `None` when it is not valid UTF-8.
fn pct_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Splits a stored identity remainder `<kind>:<val>:<eff>[:<tail>]` into its base key and tail, re-folding the
/// value parts with the Unicode case key. Identities saved with ASCII-only folding (#2912) then compare equal to
/// the current base of every row the dropdown groups under that entry; already-folded identities are unchanged.
fn normalized_identity_base(rest: &str) -> Option<(String, Option<&str>)> {
    let fold = |enc: &str| pct_decode(enc).map_or_else(|| enc.to_string(), |t| pct_encode(&filter_case_key(&t)));
    let mut parts = rest.splitn(4, ':');
    let (kind, val, eff) = (parts.next()?, parts.next()?, parts.next()?);
    Some((format!("{kind}:{}:{}", fold(val), fold(eff)), parts.next()))
}

/// Format-independent, mode-independent `<kind>:<encoded_value>` key of a non-blank cell.
fn typed_value_token(v: &Value) -> String {
    let fold = filter_case_key;
    let (kind, norm) = match v {
        Value::Number(n) => {
            let s = if *n == 0.0 { "0".to_string() } else { fold(v.display().trim()) };
            ("n", s)
        }
        Value::Text(t) => ("t", fold(t.trim())),
        Value::Bool(b) => ("b", if *b { "true".to_string() } else { "false".to_string() }),
        _ => ("e", fold(v.display().trim())),
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
    let eff = filter_case_key(if vtext.trim().is_empty() { v.display() } else { vtext }.trim());
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
    let stored = |rest| normalized_identity_base(rest).filter(|(b, _)| b == base);
    id.strip_prefix("c:").and_then(stored).is_some_and(|(_, tail)| tail.is_none())
        || id.strip_prefix("f:").and_then(stored).is_some_and(|(_, tail)| tail.is_some())
}

/// Checks whether a non-blank cell matches any primary stored identity (`d:`, `c:`, `f:`, `w:`, `s:`).
fn row_matches_primary_identities(wb: &Workbook, sh: &Sheet, c: CellRef, v: &Value, identities: &[String]) -> bool {
    let base = cell_base_token(wb, sh, c, v);
    // Stored bases are re-folded, so identities saved with ASCII-only folding match every row of their merged entry.
    let stored = |rest| normalized_identity_base(rest).filter(|(b, _)| *b == base).map(|(_, tail)| tail);
    let mut formula_enc: Option<Option<String>> = None;
    let mut slicer_enc: Option<Option<String>> = None;
    identities.iter().any(|id| {
        if let Some(rest) = id.strip_prefix("d:") {
            stored(rest) == Some(None)
        } else if let Some(rest) = id.strip_prefix("c:") {
            if stored(rest) != Some(None) {
                return false;
            }
            formula_enc.get_or_insert_with(|| visible_formula_text(wb, sh, c).map(|ft| pct_encode(&ft.trim().to_ascii_lowercase()))).is_none()
        } else if let Some(rest) = id.strip_prefix("f:") {
            let Some(Some(tail)) = stored(rest) else {
                return false;
            };
            formula_enc.get_or_insert_with(|| visible_formula_text(wb, sh, c).map(|ft| pct_encode(&ft.trim().to_ascii_lowercase()))).as_deref()
                == Some(tail)
        } else if let Some(rest) = id.strip_prefix("w:") {
            let Some(Some(tail)) = stored(rest) else {
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

/// Header-and-data range covered by a table's own AutoFilter (excluding any totals row).
fn table_filter_range(t: &gridcraft_model::Table) -> Option<RangeRef> {
    if !t.header_row {
        return None;
    }
    let end_row = t.range.end.row.checked_sub(u32::from(t.totals_row))?;
    (end_row >= t.range.start.row).then(|| RangeRef::new(t.range.start, CellRef::new(end_row, t.range.end.col)))
}

/// Precomputed per-criterion stats, blank flags, and lowercased value sets for evaluating a filter over rows.
struct PreparedFilter<'a> {
    range: RangeRef,
    criteria: &'a [(u32, FilterCriterion)],
    criterion_blanks: Vec<bool>,
    criterion_values: HashMap<usize, HashSet<String>>,
    col_stats: HashMap<usize, (Option<f64>, f64)>,
}

impl<'a> PreparedFilter<'a> {
    /// Precomputes blank flags, value lookup sets, Top10 thresholds, and column averages once before evaluating rows.
    fn new(sh: &Sheet, af_range: RangeRef, criteria: &'a [(u32, FilterCriterion)]) -> Self {
        let range = RangeRef::new(af_range.start, CellRef::new(af_range.end.row.min(gridcraft_core::MAX_ROWS - 1), af_range.end.col));
        let criterion_blanks = criteria
            .iter()
            .map(|(_, c)| match c {
                FilterCriterion::Values { values, blanks, .. } => values_include_blanks(values, *blanks),
                _ => false,
            })
            .collect();
        let criterion_values = criteria
            .iter()
            .enumerate()
            .filter_map(|(i, (_, c))| match c {
                FilterCriterion::Values { values, .. } => Some((i, values.iter().map(|v| filter_case_key(v.trim())).collect())),
                _ => None,
            })
            .collect();
        let mut col_stats = HashMap::new();
        for (i, (off, c)) in criteria.iter().enumerate() {
            if matches!(c, FilterCriterion::Top10 { .. } | FilterCriterion::AboveAverage(_)) {
                let Some(col) = range.start.col.checked_add(*off) else {
                    continue;
                };
                let mut nums: Vec<f64> =
                    (range.start.row.saturating_add(1)..=range.end.row).filter_map(|row| sh.value(CellRef::new(row, col)).as_f64()).collect();
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
        Self { range, criteria, criterion_blanks, criterion_values, col_stats }
    }

    /// Evaluates a single criterion `(i, off, crit)` on `row`.
    #[allow(clippy::too_many_arguments)]
    fn criterion_matches(
        &self,
        wb: &Workbook,
        sh: &Sheet,
        row: u32,
        i: usize,
        off: u32,
        crit: &FilterCriterion,
        pre_migration: bool,
        icon_uses_hidden: bool,
    ) -> bool {
        let Some(col) = self.range.start.col.checked_add(off) else {
            return true;
        };
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let text = crate::display::cell_text(wb, sh, c);
        match crit {
            FilterCriterion::Values { values, blanks, identities } => {
                let matches_non_blank = || {
                    let shown = crate::display::value_text_as(wb, &v, wb.styles.get(sh.style_id(c)).num_fmt.as_str());
                    if identities
                        .iter()
                        .filter_map(|id| id.strip_prefix("fmtws:"))
                        .any(|ws_enc| shown.trim().is_empty() && pct_encode(&shown.to_ascii_lowercase()) == ws_enc)
                    {
                        return true;
                    }
                    let has_primary = identities.iter().any(|id| !id.starts_with("fmtws:"));
                    if has_primary {
                        row_matches_primary_identities(wb, sh, c, &v, identities)
                    } else {
                        self.criterion_values
                            .get(&i)
                            .is_some_and(|set| set.contains(&filter_case_key(text.trim())) || set.contains(&filter_case_key(v.display().trim())))
                    }
                };
                if pre_migration {
                    if filter_blank(&v) {
                        *blanks
                    } else if filter_dropdown_blank(&v) {
                        *blanks || values.iter().any(|x| x.eq_ignore_ascii_case(&text))
                    } else {
                        matches_non_blank()
                    }
                } else if filter_dropdown_blank(&v) {
                    self.criterion_blanks[i]
                } else {
                    matches_non_blank()
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
            // Icon criteria are retained from files but cannot be evaluated yet.
            FilterCriterion::Icon { .. } => !icon_uses_hidden || !sh.is_row_hidden(row),
        }
    }

    /// Whether `row` passes all criteria of this filter (rows outside `range` are unaffected).
    fn row_passes(&self, wb: &Workbook, sh: &Sheet, row: u32, pre_migration: bool) -> bool {
        if self.criteria.is_empty() || row <= self.range.start.row || row > self.range.end.row {
            return true;
        }
        self.criteria.iter().enumerate().all(|(i, (off, crit))| self.criterion_matches(wb, sh, row, i, *off, crit, pre_migration, true))
    }

    /// Whether `row` passes every active criterion except `excluded_offset`.
    fn row_matches_other_columns(&self, wb: &Workbook, sh: &Sheet, row: u32, excluded_offset: u32) -> bool {
        self.criteria
            .iter()
            .enumerate()
            .all(|(i, (off, crit))| *off == excluded_offset || self.criterion_matches(wb, sh, row, i, *off, crit, false, false))
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
        (prepared.range.start.row.saturating_add(1)..=prepared.range.end.row).map(|row| (row, !prepared.row_passes(wb, sh, row, false))).collect()
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
/// criterion `value` (`None` for the `(Blanks)` toggle), its canonical checklist `identity`,
/// whether it is `checked`, and the `count` of rows it stands for among the rows the list was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterChoice {
    pub label: String,
    pub value: Option<String>,
    pub identity: Option<String>,
    pub checked: bool,
    pub count: u32,
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

/// Builds the `data.filterBy` parameters that leave `col` filtered to `choice` alone (the dropdown's Only button).
pub fn filter_only_params(col: &str, choice: &FilterChoice) -> Json {
    let Some(value) = &choice.value else {
        return json!({"column": col, "values": [], "blanks": true});
    };
    let ids: Vec<&String> = choice.identity.iter().collect();
    json!({"column": col, "values": [value], "identities": ids, "blanks": false})
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

/// Distinct checklist entries from rows matching the other columns' criteria, each with the count of those rows.
/// The current criterion only determines checkboxes, so its unchecked alternatives stay available and keep their counts.
pub fn filter_choices(wb: &Workbook, sheet: usize, col: u32) -> Vec<FilterChoice> {
    column_filter_choices(wb, sheet, col, true)
}

/// Applies edited eligible checklist choices to the whole column's checkbox state, retaining choices hidden by other filters.
pub fn filter_choices_selection(wb: &Workbook, sheet: usize, col: u32, choices: &[FilterChoice]) -> Vec<FilterChoice> {
    let mut edited: std::collections::BTreeMap<Option<String>, bool> = std::collections::BTreeMap::new();
    for c in choices {
        *edited.entry(c.identity.clone()).or_insert(false) |= c.checked;
    }
    let mut selection = column_filter_choices(wb, sheet, col, false);
    for c in &mut selection {
        if let Some(&checked) = edited.get(&c.identity) {
            c.checked = checked;
        }
    }
    selection
}

/// Applies edited eligible label choices to the whole column's checkbox state, retaining choices hidden by other filters.
pub fn filter_selection(wb: &Workbook, sheet: usize, col: u32, choices: &[(String, bool)]) -> Vec<(String, bool)> {
    let mut edited = std::collections::BTreeMap::new();
    for (value, on) in choices {
        *edited.entry(filter_case_key(value)).or_insert(false) |= *on;
    }
    let mut selection: Vec<(String, bool)> = column_filter_choices(wb, sheet, col, false).into_iter().map(|c| (c.label, c.checked)).collect();
    for (value, on) in &mut selection {
        if let Some(checked) = edited.get(&filter_case_key(value)) {
            *on = *checked;
        }
    }
    selection
}

fn column_filter_choices(wb: &Workbook, sheet: usize, col: u32, eligible_only: bool) -> Vec<FilterChoice> {
    let Some(sh) = wb.sheet(sheet) else { return vec![] };
    let Some(af) = &sh.autofilter else { return vec![] };
    if col < af.range.start.col || col > af.range.end.col {
        return vec![];
    }
    let off = col - af.range.start.col;
    let selected = af.criteria.iter().enumerate().find(|(_, (o, _))| *o == off);
    let selected_identities = match selected {
        Some((_, (_, FilterCriterion::Values { identities, .. }))) if !identities.is_empty() => Some(identities.as_slice()),
        _ => None,
    };
    let prepared = PreparedFilter::new(sh, af.range, &af.criteria);
    type ChoiceBucket = ((u8, i64, String), FilterChoice, (String, &'static str));
    let mut seen: std::collections::BTreeMap<Option<String>, ChoiceBucket> = std::collections::BTreeMap::new();
    // Imported end rows cannot add choices beyond the grid either.
    for row in
        af.range.start.row.saturating_add(1)..=af.range.end.row.min(af.range.start.row.saturating_add(1_000_000)).min(gridcraft_core::MAX_ROWS - 1)
    {
        if eligible_only && !prepared.row_matches_other_columns(wb, sh, row, off) {
            continue;
        }
        let c = CellRef::new(row, col);
        let v = sh.value(c);
        let blank = filter_dropdown_blank(&v);
        let t = if blank { "(Blanks)".to_string() } else { crate::display::cell_text(wb, sh, c) };
        let on = selected.is_none_or(|(i, (crit_off, crit))| prepared.criterion_matches(wb, sh, row, i, *crit_off, crit, false, false));
        if blank {
            let (_, entry, _) = seen.entry(None).or_insert((
                sort_key(&Value::Empty),
                FilterChoice { label: t, value: None, identity: None, checked: false, count: 0 },
                (String::new(), ""),
            ));
            entry.checked |= on;
            entry.count = entry.count.saturating_add(1);
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
        let (_, entry, _) = seen.entry(Some(id.clone())).or_insert((
            sort,
            FilterChoice { label: base_label, value: raw_value, identity: Some(id), checked: false, count: 0 },
            (qualifier, kind_label),
        ));
        entry.checked |= on;
        entry.count = entry.count.saturating_add(1);
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

#[cfg(test)]
mod unicode_filter_tests {
    use super::*;

    fn is_hidden(s: &Session, row: u32) -> bool {
        s.doc().unwrap().wb.active().unwrap().rows.get(&row).is_some_and(|info| info.hidden)
    }

    /// Unicode-equivalent labels stay visible and checked in value-list filters.
    #[test]
    fn unicode_value_filter_keeps_rows_and_dropdown_in_sync() {
        let mut s = Session::new();
        s.new_workbook();

        s.execute(
            "range.setValues",
            json!({
                "range": "A1",
                "values": [
                    ["Name"],
                    ["ÁRBOL"],
                    ["árbol"],
                    ["ÉCOLE"],
                    ["école"],
                    ["Pino"]
                ]
            }),
        )
        .unwrap();

        s.execute("data.filter", json!({"range": "A1:A6"})).unwrap();

        s.execute("data.filterBy", json!({"column": "A", "values": ["árbol"]})).unwrap();

        // Both uppercase and lowercase forms must match.
        for row in [1, 2] {
            assert!(!is_hidden(&s, row), "row {row} must remain visible");
        }
        for row in [3, 4, 5] {
            assert!(is_hidden(&s, row), "row {row} must be hidden");
        }

        // Case variants share one dropdown entry (as in Excel), and it shows as selected.
        let options = filter_values(&s.doc().unwrap().wb, 0, 0);
        assert_eq!(options.len(), 3, "{options:?}");
        for (label, expected) in [("ÁRBOL", true), ("ÉCOLE", false), ("Pino", false)] {
            let selected = options.iter().find(|(text, _)| text == label).map(|(_, on)| *on);

            assert_eq!(selected, Some(expected), "{label}: {options:?}");
        }

        // Another accented word uses the same comparison.
        s.execute("data.filterBy", json!({"column": "A", "values": ["école"]})).unwrap();

        for row in [1, 2, 5] {
            assert!(is_hidden(&s, row), "row {row} must be hidden");
        }
        for row in [3, 4] {
            assert!(!is_hidden(&s, row), "row {row} must remain visible");
        }

        // Custom filters and value-list filters must agree.
        s.execute(
            "data.filterBy",
            json!({
                "column": "A",
                "custom": {"op": "=", "value": "árbol"}
            }),
        )
        .unwrap();

        for row in [1, 2] {
            assert!(!is_hidden(&s, row));
        }
        for row in [3, 4, 5] {
            assert!(is_hidden(&s, row));
        }
    }

    /// Numeric values and the special Blanks group keep their existing behavior.
    #[test]
    fn unicode_value_filter_preserves_numbers_and_blanks() {
        let mut s = Session::new();
        s.new_workbook();

        s.execute(
            "range.setValues",
            json!({
                "range": "A1",
                "values": [
                    ["Value"],
                    [42],
                    [""],
                    ["Other"]
                ]
            }),
        )
        .unwrap();

        s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();

        s.execute(
            "data.filterBy",
            json!({
                "column": "A",
                "values": ["42"],
                "blanks": true
            }),
        )
        .unwrap();

        assert!(!is_hidden(&s, 1), "numeric value must match");
        assert!(!is_hidden(&s, 2), "blank must remain visible");
        assert!(is_hidden(&s, 3), "unselected text must be hidden");

        let options = filter_values(&s.doc().unwrap().wb, 0, 0);

        for (label, expected) in [("42", true), ("(Blanks)", true), ("Other", false)] {
            let selected = options.iter().find(|(text, _)| text == label).map(|(_, on)| *on);

            assert_eq!(selected, Some(expected), "{label}");
        }
    }

    /// Value keys use character-wise lowercase just like `compare_text`: `"ΟΣ"` matches `"οσ"`
    /// while remaining distinct from final-sigma `"ος"`.
    #[test]
    fn unicode_value_filter_matches_greek_sigma_consistently_with_compare_text() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Name"], ["ΟΣ"], ["οσ"], ["ος"]]})).unwrap();
        s.execute("data.filter", json!({"range": "A1:A4"})).unwrap();

        s.execute("data.filterBy", json!({"column": "A", "values": ["ΟΣ"]})).unwrap();
        assert!(!is_hidden(&s, 1), "ΟΣ must remain visible");
        assert!(!is_hidden(&s, 2), "οσ must match ΟΣ under compare_text");
        assert!(is_hidden(&s, 3), "final-sigma ος is distinct under compare_text");

        // `ΟΣ`/`οσ` share one entry; final-sigma `ος` stays separate.
        let options = filter_values(&s.doc().unwrap().wb, 0, 0);
        assert_eq!(options.len(), 2, "{options:?}");
        for (label, expected) in [("ΟΣ", true), ("ος", false)] {
            let selected = options.iter().find(|(text, _)| text == label).map(|(_, on)| *on);
            assert_eq!(selected, Some(expected), "{label}: {options:?}");
        }
    }

    /// `filter_selection` keeps comparator-distinct labels (`"ΟΣ"` vs `"ος"`) separate when
    /// round-tripping an unchanged selection.
    #[test]
    fn filter_selection_keeps_greek_final_sigma_distinct() {
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Name"], ["ΟΣ"], ["ος"]]})).unwrap();
        s.execute("data.filter", json!({"range": "A1:A3"})).unwrap();
        s.execute("data.filterBy", json!({"column": "A", "values": ["ΟΣ"]})).unwrap();

        let choices = filter_values(&s.doc().unwrap().wb, 0, 0);
        let merged = filter_selection(&s.doc().unwrap().wb, 0, 0, &choices);
        assert_eq!(merged.iter().find(|(t, _)| t == "ΟΣ").map(|(_, on)| *on), Some(true), "{merged:?}");
        assert_eq!(merged.iter().find(|(t, _)| t == "ος").map(|(_, on)| *on), Some(false), "{merged:?}");

        let values: Vec<String> = merged.into_iter().filter_map(|(v, on)| on.then_some(v)).collect();
        s.execute("data.filterBy", json!({"column": "A", "values": values})).unwrap();
        assert!(!is_hidden(&s, 1), "ΟΣ stays visible after unchanged apply");
        assert!(is_hidden(&s, 2), "ος stays excluded after unchanged apply");
    }

    /// A checklist identity saved with ASCII-only folding (`Ä` as `d:t:%C3%84:%C3%84`) still matches its row,
    /// and the dropdown still shows that entry as checked.
    #[test]
    fn legacy_ascii_folded_identity_keeps_matching_its_row() {
        use gridcraft_core::{CellRef, RangeRef};
        use gridcraft_model::AutoFilter;
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Name"], ["Ä"], ["Other"]]})).unwrap();
        let sh = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.autofilter = Some(AutoFilter {
            range: RangeRef::new(CellRef::new(0, 0), CellRef::new(2, 0)),
            criteria: vec![(0, FilterCriterion::Values { values: vec!["Ä".into()], blanks: false, identities: vec!["d:t:%C3%84:%C3%84".into()] })],
        });
        s.execute("data.reapply", json!({})).unwrap();
        assert!(!is_hidden(&s, 1), "legacy identity must keep its row visible");
        assert!(is_hidden(&s, 2));
        let options = filter_values(&s.doc().unwrap().wb, 0, 0);
        assert_eq!(options.iter().find(|(t, _)| t == "Ä").map(|(_, on)| *on), Some(true), "{options:?}");
        assert_eq!(options.iter().find(|(t, _)| t == "Other").map(|(_, on)| *on), Some(false), "{options:?}");
    }

    /// A legacy identity for one case variant selects every row of the merged entry, as reapplying the unchanged checklist does.
    #[test]
    fn legacy_ascii_folded_identity_matches_all_case_variants_of_its_entry() {
        use gridcraft_core::{CellRef, RangeRef};
        use gridcraft_model::AutoFilter;
        let mut s = Session::new();
        s.new_workbook();
        s.execute("range.setValues", json!({"range": "A1", "values": [["Name"], ["Ä"], ["ä"], ["Other"]]})).unwrap();
        let sh = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.autofilter = Some(AutoFilter {
            range: RangeRef::new(CellRef::new(0, 0), CellRef::new(3, 0)),
            criteria: vec![(0, FilterCriterion::Values { values: vec!["Ä".into()], blanks: false, identities: vec!["d:t:%C3%84:%C3%84".into()] })],
        });
        s.execute("data.reapply", json!({})).unwrap();
        assert!(!is_hidden(&s, 1) && !is_hidden(&s, 2), "both case variants share the checked entry");
        assert!(is_hidden(&s, 3));
        let choices = filter_choices(&s.doc().unwrap().wb, 0, 0);
        assert_eq!(choices.len(), 2, "{choices:?}");
        let again: Vec<String> = choices.iter().filter(|c| c.checked).filter_map(|c| c.identity.clone()).collect();
        let sh = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap();
        sh.autofilter.as_mut().unwrap().criteria = vec![(0, FilterCriterion::Values { values: vec!["Ä".into()], blanks: false, identities: again })];
        s.execute("data.reapply", json!({})).unwrap();
        assert!(!is_hidden(&s, 1) && !is_hidden(&s, 2) && is_hidden(&s, 3), "reapplying the unchanged checklist keeps the same rows");
    }
}
