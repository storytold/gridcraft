use std::cmp::Ordering;

use gridcraft_core::{CellError, CellRef, RangeRef, Value, compare};
use gridcraft_formula::Expr;
use gridcraft_model::{PivotField, PivotFunc, PivotLayout, PivotTable, PivotValue};

use super::{Area, Ev, Evaluator, err};

/// An evaluated `(field, item_value, item_text)` pair from `GETPIVOTDATA`.
struct FieldItem {
    field: String,
    value: Value,
    text: String,
}

fn same_label(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn func_prefix(f: PivotFunc) -> &'static str {
    match f {
        PivotFunc::Sum => "Sum",
        PivotFunc::Count => "Count",
        PivotFunc::Average => "Average",
        PivotFunc::Max => "Max",
        PivotFunc::Min => "Min",
        PivotFunc::Product => "Product",
        PivotFunc::CountNumbers => "Count Numbers",
        PivotFunc::StdDev => "StdDev",
        PivotFunc::StdDevP => "StdDevp",
        PivotFunc::Var => "Var",
        PivotFunc::VarP => "Varp",
    }
}

fn matches_data_field(v: &PivotValue, query: &str) -> bool {
    same_label(&v.name, query) || same_label(&v.source_col, query) || same_label(&format!("{} of {}", func_prefix(v.func), v.source_col), query)
}

fn matches_axis_field(f: &PivotField, query: &str) -> bool {
    same_label(&f.source_col, query)
}

fn value_matches_item(cell_val: &Value, item_val: &Value, item_text: &str) -> bool {
    if cell_val.is_empty() {
        return false;
    }
    if compare(cell_val, item_val) == Ordering::Equal {
        return true;
    }
    if let (Ok(a), Ok(b)) = (cell_val.to_number(), item_val.to_number())
        && (a - b).abs() <= 1e-12
    {
        return true;
    }
    cell_val.to_text().is_ok_and(|t| same_label(&t, item_text))
}

fn is_grand_label(v: &Value, custom: Option<&str>) -> bool {
    let Ok(t) = v.to_text() else { return false };
    same_label(&t, "Grand Total") || custom.is_some_and(|c| same_label(&t, c))
}

fn is_subtotal_of(v: &Value, item_text: &str, val_name: &str) -> bool {
    let Ok(t) = v.to_text() else { return false };
    same_label(&t, &format!("{} Total", item_text.trim())) || same_label(&t, &format!("{} {}", item_text.trim(), val_name.trim()))
}

fn is_any_subtotal_or_grand(v: &Value, custom_grand: Option<&str>) -> bool {
    if is_grand_label(v, custom_grand) {
        return true;
    }
    v.to_text().is_ok_and(|t| t.trim_end().ends_with(" Total"))
}

impl<'h> Evaluator<'h> {
    pub(super) fn getpivotdata(&mut self, args: &[Expr]) -> Ev {
        if args.len() < 2 || !args.len().is_multiple_of(2) {
            return err(CellError::Value);
        }
        let data_val = self.value(&args[0]).scalar();
        if let Value::Error(e) = data_val {
            return err(e);
        }
        if matches!(data_val, Value::Empty) {
            return err(CellError::Ref);
        }
        let data_field = match data_val.to_text() {
            Ok(s) if !s.trim().is_empty() => s,
            Ok(_) => return err(CellError::Ref),
            Err(e) => return err(e),
        };
        let area = match self.eval(&args[1]) {
            Ev::R(areas) => match areas.as_slice() {
                [a] => *a,
                _ => return err(CellError::Ref),
            },
            Ev::V(Value::Error(e)) => return err(e),
            _ => return err(CellError::Ref),
        };
        let mut pairs = Vec::with_capacity((args.len() - 2) / 2);
        for [fe, ie] in args[2..].as_chunks::<2>().0 {
            let fv = self.value(fe).scalar();
            if let Value::Error(e) = fv {
                return err(e);
            }
            let iv = self.value(ie).scalar();
            if let Value::Error(e) = iv {
                return err(e);
            }
            if matches!(fv, Value::Empty) || matches!(iv, Value::Empty) {
                return err(CellError::Ref);
            }
            let field = match fv.to_text() {
                Ok(s) if !s.trim().is_empty() => s,
                Ok(_) => return err(CellError::Ref),
                Err(e) => return err(e),
            };
            let text = match iv.to_text() {
                Ok(s) => s,
                Err(e) => return err(e),
            };
            pairs.push(FieldItem { field, value: iv, text });
        }

        let found_pt = self.host.workbook().sheet(area.sheet).and_then(|sh| {
            sh.pivots
                .iter()
                .find(|pt| pt.last_range.is_some_and(|r| r.intersection(&area.range).is_some()) || area.range.contains(pt.anchor))
                .cloned()
        });
        let res = match found_pt {
            Some(pt) => self.eval_pivot_model(area.sheet, &pt, &data_field, &pairs),
            None => self.eval_pivot_cells(area, &data_field, &pairs),
        };
        match res {
            Ok(v) => Ev::V(v),
            Err(e) => err(e),
        }
    }

    fn eval_pivot_model(&mut self, sheet: usize, pt: &PivotTable, data_field: &str, pairs: &[FieldItem]) -> Result<Value, CellError> {
        let vi = pt
            .values
            .iter()
            .position(|v| same_label(&v.name, data_field))
            .or_else(|| pt.values.iter().position(|v| matches_data_field(v, data_field)))
            .ok_or(CellError::Ref)?;
        let val_name = pt.values.get(vi).map(|v| v.name.as_str()).unwrap_or("");
        let mut row_req: Vec<Option<&FieldItem>> = vec![None; pt.rows.len()];
        let mut col_req: Vec<Option<&FieldItem>> = vec![None; pt.columns.len()];
        for p in pairs {
            if let Some(i) = pt.rows.iter().position(|f| matches_axis_field(f, &p.field)) {
                row_req[i] = Some(p);
            } else if let Some(i) = pt.columns.iter().position(|f| matches_axis_field(f, &p.field)) {
                col_req[i] = Some(p);
            } else if self.matches_pivot_filter(sheet, pt, p) {
                continue;
            } else {
                return Err(CellError::Ref);
            }
        }
        let body_end = pt.last_range.map_or(pt.anchor, |r| r.end);
        let r0 = pt.anchor.row;
        let c0 = pt.anchor.col;
        let r1 = body_end.row.max(r0);
        let c1 = body_end.col.max(c0);
        let k = pt.values.len();
        let on_rows = pt.values_on_rows && k > 1;
        let row_levels = pt.rows.len() + usize::from(on_rows);
        let compact = pt.layout == PivotLayout::Compact;
        let l = if row_levels == 0 && pt.columns.is_empty() {
            0
        } else if compact {
            1
        } else {
            row_levels.max(1)
        };
        let header_rows = if !pt.columns.is_empty() {
            1 + pt.columns.len() + usize::from(k > 1 && !on_rows)
        } else {
            usize::from(!(row_levels == 0 && k <= 1) && (pt.show_headers || (!on_rows && k > 1)))
        };
        let body_start_row = (r0 + header_rows as u32).min(r1);
        let first_data_col = (c0 + l as u32).min(c1);
        let target_col = self.find_pivot_col(sheet, pt, (r0, r1), (first_data_col, c1), vi, val_name, &col_req)?;
        let target_row = self.find_pivot_row(sheet, pt, (body_start_row, r1), c0, target_col, vi, val_name, &row_req)?;
        let val = self.cell(sheet, CellRef::new(target_row, target_col));
        if val.is_empty() { Err(CellError::Ref) } else { Ok(val) }
    }

    fn matches_pivot_filter(&mut self, sheet: usize, pt: &PivotTable, p: &FieldItem) -> bool {
        let Some(f) = pt.filters.iter().find(|f| same_label(&f.source_col, &p.field)) else { return false };
        if let Some(sel) = &f.selected
            && sel.len() == 1
            && sel.first().is_some_and(|s| same_label(s, &p.text))
        {
            return true;
        }
        let top = pt.last_range.map_or(pt.anchor.row, |r| r.start.row);
        for r in top..pt.anchor.row {
            let fn_cell = self.cell(sheet, CellRef::new(r, pt.anchor.col));
            let fv_cell = self.cell(sheet, CellRef::new(r, pt.anchor.col + 1));
            if fn_cell.to_text().is_ok_and(|t| same_label(&t, &p.field)) && value_matches_item(&fv_cell, &p.value, &p.text) {
                return true;
            }
        }
        false
    }

    #[allow(clippy::too_many_arguments)]
    fn find_pivot_col(
        &mut self,
        sheet: usize,
        pt: &PivotTable,
        rows: (u32, u32),
        cols: (u32, u32),
        vi: usize,
        val_name: &str,
        col_req: &[Option<&FieldItem>],
    ) -> Result<u32, CellError> {
        let (r0, r1) = rows;
        let (c_start, c1) = cols;
        let k = pt.values.len();
        let on_rows = pt.values_on_rows && k > 1;
        if pt.columns.is_empty() {
            let col = if on_rows || k <= 1 { c_start } else { c_start + vi as u32 };
            return (col <= c1).then_some(col).ok_or(CellError::Ref);
        }
        if col_req.iter().all(Option::is_none) {
            if !pt.grand_totals_rows {
                return Err(CellError::Ref);
            }
            let col = if k <= 1 || on_rows { c1 } else { (c1 + 1).saturating_sub(k as u32) + vi as u32 };
            return (col >= c_start && col <= c1).then_some(col).ok_or(CellError::Ref);
        }
        let first_level_row = (r0 + 1).min(r1);
        let last_req = col_req.iter().rposition(Option::is_some).unwrap_or(0);
        let is_subtotal = last_req + 1 < pt.columns.len();
        let mut cur: Vec<Value> = vec![Value::Empty; pt.columns.len()];
        for c in c_start..=c1 {
            let top_v = self.cell(sheet, CellRef::new(first_level_row, c));
            if is_grand_label(&top_v, None) {
                break;
            }
            let mut subtotal_at = None;
            for lv in 0..pt.columns.len() {
                let v = self.cell(sheet, CellRef::new((first_level_row + lv as u32).min(r1), c));
                if !v.is_empty() {
                    if let Some(req) = col_req.get(lv).copied().flatten()
                        && is_subtotal_of(&v, &req.text, val_name)
                    {
                        subtotal_at = Some(lv);
                    } else {
                        if let Some(slot) = cur.get_mut(lv) {
                            *slot = v;
                        }
                        for deeper in cur.iter_mut().skip(lv + 1) {
                            *deeper = Value::Empty;
                        }
                    }
                }
            }
            if is_subtotal {
                if subtotal_at == Some(last_req) && self.col_prefix_matches(&cur, col_req, last_req) {
                    return Ok(c);
                }
                continue;
            }
            if subtotal_at.is_some() || !self.col_prefix_matches(&cur, col_req, last_req + 1) {
                continue;
            }
            if k > 1 && !on_rows {
                let val_row = (first_level_row + pt.columns.len() as u32).min(r1);
                let hv = self.cell(sheet, CellRef::new(val_row, c));
                if hv.to_text().is_ok_and(|t| same_label(&t, val_name)) {
                    return Ok(c);
                }
            } else {
                return Ok(c);
            }
        }
        Err(CellError::Ref)
    }

    fn col_prefix_matches(&self, cur: &[Value], reqs: &[Option<&FieldItem>], upto: usize) -> bool {
        for (cv, req) in cur.iter().zip(reqs).take(upto) {
            if let Some(r) = req
                && !value_matches_item(cv, &r.value, &r.text)
            {
                return false;
            }
        }
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn find_pivot_row(
        &mut self,
        sheet: usize,
        pt: &PivotTable,
        rows: (u32, u32),
        c0: u32,
        target_col: u32,
        vi: usize,
        val_name: &str,
        row_req: &[Option<&FieldItem>],
    ) -> Result<u32, CellError> {
        let (body_start, r1) = rows;
        let k = pt.values.len();
        let on_rows = pt.values_on_rows && k > 1;
        let grand_cap = None;
        if pt.rows.is_empty() {
            if !on_rows {
                return Ok(body_start);
            }
            for r in body_start..=r1 {
                if self.cell(sheet, CellRef::new(r, c0)).to_text().is_ok_and(|t| same_label(&t, val_name)) {
                    return Ok(r);
                }
            }
            return Err(CellError::Ref);
        }
        if row_req.iter().all(Option::is_none) {
            if !pt.grand_totals_cols {
                return Err(CellError::Ref);
            }
            let want_total = format!("Total {val_name}");
            for r in (body_start..=r1).rev() {
                let v = self.cell(sheet, CellRef::new(r, c0));
                if on_rows {
                    if v.to_text().is_ok_and(|t| same_label(&t, &want_total)) {
                        return Ok(r);
                    }
                } else if is_grand_label(&v, grand_cap) {
                    return Ok(r);
                }
            }
            return Ok(r1);
        }
        if pt.layout == PivotLayout::Compact {
            return self.find_compact_row(sheet, pt, (body_start, r1), c0, target_col, vi, val_name, row_req);
        }
        self.find_columnar_row(sheet, pt, (body_start, r1), c0, target_col, val_name, row_req)
    }

    #[allow(clippy::too_many_arguments)]
    fn find_compact_row(
        &mut self,
        sheet: usize,
        pt: &PivotTable,
        rows: (u32, u32),
        c0: u32,
        target_col: u32,
        vi: usize,
        val_name: &str,
        row_req: &[Option<&FieldItem>],
    ) -> Result<u32, CellError> {
        let (body_start, r1) = rows;
        let on_rows = pt.values_on_rows && pt.values.len() > 1;
        let last_req = row_req.iter().rposition(Option::is_some).unwrap_or(0);
        let is_subtotal = last_req + 1 < pt.rows.len();
        let mut cur: Vec<Value> = vec![Value::Empty; pt.rows.len()];
        for r in body_start..=r1 {
            let v = self.cell(sheet, CellRef::new(r, c0));
            if v.is_empty() || is_grand_label(&v, None) {
                continue;
            }
            let indent = self
                .host
                .workbook()
                .sheet(sheet)
                .map_or(0, |sh| self.host.workbook().styles.get(sh.style_id(CellRef::new(r, c0))).align.indent as usize);
            if is_subtotal
                && let Some(req) = row_req.get(last_req).copied().flatten()
                && is_subtotal_of(&v, &req.text, val_name)
                && self.col_prefix_matches(&cur, row_req, last_req)
            {
                return Ok(r);
            }
            let lv = if pt.rows.len() == 1 {
                0
            } else if indent < pt.rows.len()
                && (indent > 0 || row_req.first().and_then(|x| *x).is_some_and(|req| value_matches_item(&v, &req.value, &req.text)))
            {
                indent
            } else {
                row_req.iter().enumerate().find_map(|(d, req)| req.filter(|rq| value_matches_item(&v, &rq.value, &rq.text)).map(|_| d)).unwrap_or(0)
            };
            if let Some(slot) = cur.get_mut(lv) {
                *slot = v;
            }
            for deeper in cur.iter_mut().skip(lv + 1) {
                *deeper = Value::Empty;
            }
            if lv == last_req && self.col_prefix_matches(&cur, row_req, last_req + 1) {
                if is_subtotal && !(pt.subtotals && pt.subtotals_top) {
                    continue;
                }
                if on_rows {
                    let candidate = (r + 1 + vi as u32).min(r1);
                    if self.cell(sheet, CellRef::new(candidate, c0)).to_text().is_ok_and(|t| same_label(&t, val_name)) {
                        return Ok(candidate);
                    }
                }
                if !self.cell(sheet, CellRef::new(r, target_col)).is_empty() {
                    return Ok(r);
                }
            }
        }
        Err(CellError::Ref)
    }

    #[allow(clippy::too_many_arguments)]
    fn find_columnar_row(
        &mut self,
        sheet: usize,
        pt: &PivotTable,
        rows: (u32, u32),
        c0: u32,
        target_col: u32,
        val_name: &str,
        row_req: &[Option<&FieldItem>],
    ) -> Result<u32, CellError> {
        let (body_start, r1) = rows;
        let on_rows = pt.values_on_rows && pt.values.len() > 1;
        let vpos = on_rows.then(|| pt.values_position.unwrap_or(pt.rows.len()).min(pt.rows.len()));
        let col_of = |d: usize| c0 + (d + usize::from(vpos.is_some_and(|vp| vp <= d))) as u32;
        let last_req = row_req.iter().rposition(Option::is_some).unwrap_or(0);
        let is_subtotal = last_req + 1 < pt.rows.len();
        let mut cur: Vec<Value> = vec![Value::Empty; pt.rows.len()];
        for r in body_start..=r1 {
            let v0 = self.cell(sheet, CellRef::new(r, c0));
            if is_grand_label(&v0, None) {
                break;
            }
            let mut subtotal_at = None;
            for d in 0..pt.rows.len() {
                let v = self.cell(sheet, CellRef::new(r, col_of(d)));
                if !v.is_empty() {
                    if let Some(req) = row_req.get(d).copied().flatten()
                        && is_subtotal_of(&v, &req.text, val_name)
                    {
                        subtotal_at = Some(d);
                    } else if is_any_subtotal_or_grand(&v, None) {
                        subtotal_at = Some(d);
                    } else {
                        if let Some(slot) = cur.get_mut(d) {
                            *slot = v;
                        }
                        for deeper in cur.iter_mut().skip(d + 1) {
                            *deeper = Value::Empty;
                        }
                    }
                }
            }
            if is_subtotal {
                if subtotal_at == Some(last_req) && self.col_prefix_matches(&cur, row_req, last_req) {
                    return Ok(r);
                }
                if subtotal_at.is_none()
                    && pt.layout == PivotLayout::Outline
                    && pt.subtotals
                    && pt.subtotals_top
                    && cur.get(last_req + 1).is_some_and(Value::is_empty)
                    && self.col_prefix_matches(&cur, row_req, last_req + 1)
                    && !self.cell(sheet, CellRef::new(r, target_col)).is_empty()
                {
                    return Ok(r);
                }
                continue;
            }
            if subtotal_at.is_some() || !self.col_prefix_matches(&cur, row_req, last_req + 1) || cur.get(last_req).is_none_or(Value::is_empty) {
                continue;
            }
            if let Some(vp) = vpos {
                let vv = self.cell(sheet, CellRef::new(r, c0 + vp as u32));
                if !vv.to_text().is_ok_and(|t| same_label(&t, val_name)) {
                    continue;
                }
            }
            if !self.cell(sheet, CellRef::new(r, target_col)).is_empty() {
                return Ok(r);
            }
        }
        Err(CellError::Ref)
    }

    /// Fallback when `sheet.pivots` does not hold a `PivotTable` covering `area` (for example, a
    /// PivotTable backed by an external/DataModel cache that was kept as plain worksheet cells).
    fn eval_pivot_cells(&mut self, area: Area, data_field: &str, pairs: &[FieldItem]) -> Result<Value, CellError> {
        let bounds = self.detect_cell_table(area)?;
        let (r0, c0) = (bounds.start.row, bounds.start.col);
        let (r1, c1) = (bounds.end.row, bounds.end.col);
        let header_end = r0.saturating_add(3).min(r1);
        let mut found_hdr = None;
        'hdr: for r in r0..=header_end {
            for c in c0..=c1 {
                let Ok(t) = self.pivot_label_cell(area.sheet, CellRef::new(r, c)).to_text() else { continue };
                if same_label(&t, data_field)
                    || same_label(&t, &format!("Sum of {data_field}"))
                    || same_label(&t, &format!("Count of {data_field}"))
                    || same_label(&t, &format!("Average of {data_field}"))
                {
                    found_hdr = Some((r, c));
                    break 'hdr;
                }
            }
        }
        let (hdr_r, hdr_c) = found_hdr.ok_or(CellError::Ref)?;
        let has_col_axis = hdr_c == c0 && c1 > c0;
        let label_row = if has_col_axis { (hdr_r + 1).min(r1) } else { hdr_r };
        let col_hdr =
            if has_col_axis { self.pivot_label_cell(area.sheet, CellRef::new(hdr_r, c0 + 1)).to_text().unwrap_or_default() } else { String::new() };
        let row_hdr = self.pivot_label_cell(area.sheet, CellRef::new(label_row, c0)).to_text().unwrap_or_default();
        let named_row = !row_hdr.is_empty() && !same_label(&row_hdr, "Row Labels");
        let named_col = !col_hdr.is_empty() && !same_label(&col_hdr, "Column Labels");
        let scan_r1 = self.host.used_range(area.sheet).map_or(label_row, |u| u.end.row.min(r1));
        let mut target_col = if has_col_axis { c1 } else { hdr_c };
        let mut target_row = None;
        let mut col_matched = false;
        for p in pairs {
            let can_col = has_col_axis && !(named_row && same_label(&row_hdr, &p.field)) && (!named_col || same_label(&col_hdr, &p.field));
            let can_row = !(named_col && same_label(&col_hdr, &p.field)) && (!named_row || same_label(&row_hdr, &p.field));
            let col_hit = if can_col {
                ((c0 + 1)..=c1).find(|&c| value_matches_item(&self.pivot_label_cell(area.sheet, CellRef::new(label_row, c)), &p.value, &p.text))
            } else {
                None
            };
            let row_hit = if can_row {
                ((label_row + 1)..=scan_r1).find(|&r| value_matches_item(&self.pivot_label_cell(area.sheet, CellRef::new(r, c0)), &p.value, &p.text))
            } else {
                None
            };
            match (col_hit, row_hit) {
                (Some(c), None) => {
                    target_col = c;
                    col_matched = true;
                }
                (None, Some(r)) => {
                    target_row = Some(r);
                }
                (Some(c), Some(r)) => {
                    if col_matched && target_row.is_none() {
                        target_row = Some(r);
                    } else {
                        target_col = c;
                        col_matched = true;
                    }
                }
                (None, None) => return Err(CellError::Ref),
            }
        }
        if target_row.is_none() {
            for r in ((label_row + 1)..=scan_r1).rev() {
                let v = self.pivot_label_cell(area.sheet, CellRef::new(r, c0));
                if is_grand_label(&v, Some("Total")) {
                    target_row = Some(r);
                    break;
                }
            }
            if target_row.is_none() {
                target_row = Some(r1);
            }
        }
        let row = target_row.ok_or(CellError::Ref)?;
        let val = self.cell(area.sheet, CellRef::new(row, target_col));
        if val.is_empty() { Err(CellError::Ref) } else { Ok(val) }
    }

    fn detect_cell_table(&mut self, area: Area) -> Result<RangeRef, CellError> {
        let r0 = area.range.start.row;
        let c0 = area.range.start.col;
        let mut r1 = area.range.end.row.min(gridcraft_core::MAX_ROWS - 1);
        let mut c1 = area.range.end.col.min(gridcraft_core::MAX_COLS - 1);
        let next_start_r = r0.saturating_add(1).min(gridcraft_core::MAX_ROWS - 1);
        if !self.is_table_cell(area.sheet, CellRef::new(r0, c0)) && !self.is_table_cell(area.sheet, CellRef::new(next_start_r, c0)) {
            return Err(CellError::Ref);
        }
        let header_probe_r = r0.saturating_add(1).min(gridcraft_core::MAX_ROWS - 1);
        while c1 < gridcraft_core::MAX_COLS - 1 && c1.saturating_sub(c0) < 128 {
            let next_c = c1 + 1;
            let any = (r0..=header_probe_r).any(|r| self.is_table_cell(area.sheet, CellRef::new(r, next_c)));
            if any {
                c1 = next_c;
            } else {
                break;
            }
        }
        let max_probe_c = c0.saturating_add(2).min(c1);
        while r1 < gridcraft_core::MAX_ROWS - 1 && r1.saturating_sub(r0) < 5_000 {
            let next_r = r1 + 1;
            let any = (c0..=max_probe_c).any(|c| self.is_table_cell(area.sheet, CellRef::new(next_r, c)));
            if any {
                r1 = next_r;
            } else {
                break;
            }
        }
        let max_probe_r = r0.saturating_add(2).min(r1);
        while c1 < gridcraft_core::MAX_COLS - 1 && c1.saturating_sub(c0) < 128 {
            let next_c = c1 + 1;
            let any = (r0..=max_probe_r).any(|r| self.is_table_cell(area.sheet, CellRef::new(r, next_c)));
            if any {
                c1 = next_c;
            } else {
                break;
            }
        }
        Ok(RangeRef::new(CellRef::new(r0, c0), CellRef::new(r1, c1)))
    }

    fn is_table_cell(&self, sheet: usize, cell: CellRef) -> bool {
        (sheet, cell) != (self.sheet, self.at) && self.host.is_cell_occupied(sheet, cell)
    }

    fn pivot_label_cell(&mut self, sheet: usize, cell: CellRef) -> Value {
        if (sheet, cell) == (self.sheet, self.at) || self.host.is_cell_active(sheet, cell) {
            return Value::Empty;
        }
        self.cell(sheet, cell)
    }
}
