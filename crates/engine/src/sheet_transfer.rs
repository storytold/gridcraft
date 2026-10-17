//! Checked, two-phase transfer of a self-contained worksheet between workbooks.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gridcraft_core::MAX_COLS;
use gridcraft_formula::{Expr, SheetSel};
use gridcraft_model::{CfRule, CfValueKind, DefinedName, Sheet, StyleId, TotalsFn, Visibility, Workbook};

use crate::{EngineError, Result};

pub(crate) fn run(s: &mut crate::Session, target: &serde_json::Value, sheet: usize, before: usize, copy: bool) -> Result<serde_json::Value> {
    let source_index = s.active_index();
    let source = s.doc()?.wb.clone();
    let new = target.as_str() == Some("new");
    let target_index = if new {
        None
    } else {
        let uid = target.as_u64().ok_or_else(|| refuse("choose an open workbook or 'new'."))?;
        Some(s.documents().iter().position(|d| d.uid == uid).ok_or_else(|| refuse("the destination workbook is no longer open."))?)
    };
    let mut blank = Workbook::new();
    blank.date_system = source.date_system;
    blank.theme = source.theme.clone();
    blank.styles = gridcraft_model::StyleTable::with_default(source.styles.get(StyleId::DEFAULT).clone());
    let mut destination = if let Some(index) = target_index {
        s.documents().get(index).ok_or(crate::EngineError::NoDocument)?.wb.as_ref().clone()
    } else {
        let mut empty = blank.clone();
        empty.sheets.clear();
        empty
    };
    if new && source.sheet(sheet).is_some_and(|sh| sh.visibility != Visibility::Visible) {
        return Err(refuse("a new workbook needs a visible worksheet."));
    }
    // New workbooks have no insertion list; stale positions are not reused.
    let before = if new { 0 } else { before };
    let (mut remaining, output, at) = prepare(&source, &destination, sheet, before, copy)?;
    destination = output;
    let mut destination_calc = gridcraft_calc::Calc::new();
    destination_calc.recalc_all(&mut destination);
    let remaining_calc = if copy {
        None
    } else {
        let mut calc = gridcraft_calc::Calc::new();
        calc.recalc_all(&mut remaining);
        Some(calc)
    };
    // All validation and recalculation has succeeded. Publish both results together.
    let index = if let Some(index) = target_index {
        index
    } else {
        let n = (1..).find(|n| !s.documents().iter().any(|d| d.title == format!("Book{n}") && d.path.is_none())).unwrap_or(1);
        s.add_document(crate::DocState::new(blank, None, format!("Book{n}")))
    };
    if let Some(calc) = remaining_calc
        && let Some(d) = s.docs.get_mut(source_index)
    {
        d.wb = Arc::new(remaining);
        d.calc = calc;
        d.selection = crate::Selection::at(d.wb.active().map(|sh| sh.view_active).unwrap_or_default());
        if sheet < d.sheet_selections.len() {
            d.sheet_selections.remove(sheet);
        }
    }
    let d = s.docs.get_mut(index).ok_or(crate::EngineError::NoDocument)?;
    d.wb = Arc::new(destination);
    d.calc = destination_calc;
    d.sheet_selections.clear();
    d.selection = crate::Selection::at(d.wb.active().map(|sh| sh.view_active).unwrap_or_default());
    let uid = d.uid;
    s.set_active(index);
    if !copy {
        s.clipboard = None;
    }
    Ok(serde_json::json!({"workbook": uid, "sheet": at}))
}

fn refuse(reason: impl std::fmt::Display) -> EngineError {
    EngineError::Other(format!("Cannot transfer this worksheet: {reason}"))
}

/// Neither input is mutated. Recalculation and publication happen only after validation.
pub(crate) fn prepare(source: &Workbook, destination: &Workbook, sheet: usize, before: usize, copy: bool) -> Result<(Workbook, Workbook, usize)> {
    let original = source.sheet(sheet).ok_or_else(|| refuse("the source sheet no longer exists."))?;
    if source.protected_structure || destination.protected_structure {
        return Err(refuse("a workbook's structure is protected."));
    }
    if !copy && original.visibility == Visibility::Visible && source.sheets.iter().filter(|s| s.visibility == Visibility::Visible).count() <= 1 {
        return Err(refuse("the source workbook must keep at least one visible worksheet. Use Create a copy instead."));
    }
    if source.date_system != destination.date_system {
        return Err(refuse("the workbooks use different date systems."));
    }
    if source.theme.colors != destination.theme.colors
        || source.theme.major_font != destination.theme.major_font
        || source.theme.minor_font != destination.theme.minor_font
    {
        return Err(refuse("the workbooks use different themes; theme conversion is not supported yet."));
    }
    if source.vba_project.is_some() || original.preserved.as_ref().is_some_and(|p| !p.is_empty()) {
        return Err(refuse("VBA or preserved package content cannot yet be transferred safely."));
    }
    if !original.pivots.is_empty() || !original.slicers.is_empty() || original.charts.iter().any(|c| c.pivot.is_some()) {
        return Err(refuse("PivotTables, PivotCharts and slicers cannot yet be transferred safely."));
    }
    if before > destination.sheets.len() {
        return Err(refuse("the destination sheet position no longer exists."));
    }
    let name = unique_name(destination, &original.name)?;
    let mut output = destination.clone();
    let mut moved = original.clone();
    moved.name = name.clone();
    let table_names: HashSet<_> = original.tables.iter().map(|t| t.name.to_ascii_lowercase()).collect();
    for table in &moved.tables {
        if table.connection_id.is_some() || table.table_type.is_some() || table.columns.iter().any(|c| c.xml.is_some()) {
            return Err(refuse("a table is bound to workbook-level data that cannot be transferred yet."));
        }
        if output.sheets.iter().flat_map(|s| &s.tables).any(|t| t.name.eq_ignore_ascii_case(&table.name))
            || output.names.iter().any(|n| n.name.eq_ignore_ascii_case(&table.name))
        {
            return Err(refuse(format!("the destination already has a table or name called '{}'.", table.name)));
        }
    }
    let mut needed: BTreeSet<usize> = source.names.iter().enumerate().filter(|(_, n)| n.scope == Some(sheet)).map(|(i, _)| i).collect();
    let mut rewrite = |text: &str| rewrite_formula(text, source, &output, sheet, &name, &table_names, &mut needed);
    map_formulas(&mut moved, &mut rewrite)?;
    // Names can refer to other names. A set bounds traversal even for cyclic definitions.
    let mut done = BTreeSet::new();
    let mut imported = Vec::new();
    while let Some(index) = needed.iter().find(|i| !done.contains(*i)).copied() {
        done.insert(index);
        let Some(old) = source.names.get(index) else { return Err(refuse("a defined name is unavailable.")) };
        let formula = rewrite_formula(&old.formula, source, &output, sheet, &name, &table_names, &mut needed)?;
        if (old.scope.is_none() && output.names.iter().any(|n| n.scope.is_none() && n.name.eq_ignore_ascii_case(&old.name)))
            || output.sheets.iter().flat_map(|s| &s.tables).any(|t| t.name.eq_ignore_ascii_case(&old.name))
        {
            return Err(refuse(format!("the destination already defines '{}'; resolve the name conflict first.", old.name)));
        }
        imported.push(DefinedName { formula, scope: old.scope.map(|_| before), ..old.clone() });
    }
    remap_styles(source, &mut output, &mut moved);
    // Tables have workbook-wide ids, whereas drawing ids are local to their sheet.
    let mut id = output.sheets.iter().flat_map(|s| &s.tables).map(|t| t.id).max().unwrap_or(0);
    for table in &mut moved.tables {
        id = id.checked_add(1).ok_or_else(|| refuse("there are too many table ids."))?;
        table.id = id;
    }
    for n in &mut output.names {
        if let Some(scope) = &mut n.scope
            && *scope >= before
        {
            *scope += 1;
        }
    }
    output.names.extend(imported);
    let visible = moved.visibility == Visibility::Visible;
    output.sheets.insert(before, Arc::new(moved));
    output.active_sheet = if visible { before } else { destination.active_sheet + usize::from(destination.active_sheet >= before) };
    let mut remaining = source.clone();
    if !copy {
        check_inbound(source, sheet, &table_names)?;
        remaining.sheets.remove(sheet);
        remaining.names.retain(|n| n.scope != Some(sheet));
        for n in &mut remaining.names {
            if let Some(scope) = &mut n.scope
                && *scope > sheet
            {
                *scope -= 1;
            }
        }
        remaining.active_sheet =
            remaining.active_sheet.saturating_sub(usize::from(remaining.active_sheet >= sheet)).min(remaining.sheets.len().saturating_sub(1));
    }
    Ok((remaining, output, before))
}

fn unique_name(destination: &Workbook, base: &str) -> Result<String> {
    if destination.sheet_index(base).is_none() {
        return Ok(base.to_string());
    }
    for n in 2..=destination.sheets.len().saturating_add(2) {
        let suffix = format!(" ({n})");
        let keep = 31usize.saturating_sub(suffix.chars().count());
        let name = format!("{}{suffix}", base.chars().take(keep).collect::<String>().trim_end());
        if destination.sheet_index(&name).is_none() {
            return Ok(name);
        }
    }
    Err(refuse("no available sheet name."))
}

fn rewrite_formula(
    text: &str,
    source: &Workbook,
    destination: &Workbook,
    sheet: usize,
    name: &str,
    tables: &HashSet<String>,
    needed: &mut BTreeSet<usize>,
) -> Result<String> {
    if text.is_empty() {
        return Ok(String::new());
    }
    let expr = gridcraft_formula::parse(text).map_err(|_| refuse("a formula could not be checked."))?;
    let old = source.sheet(sheet).map(|s| s.name.as_str()).unwrap_or("");
    let mut error = None;
    let mapped = expr.map(&mut |mut e| {
        match &mut e {
            Expr::Ref(r) => match &mut r.sheet {
                SheetSel::Current => {}
                SheetSel::Named(s) if s.eq_ignore_ascii_case(old) => *s = name.to_string(),
                SheetSel::Span(a, b) if a.eq_ignore_ascii_case(old) && b.eq_ignore_ascii_case(old) => {
                    *a = name.to_string();
                    *b = name.to_string();
                }
                _ => error = Some("a formula refers to another sheet or workbook.".to_string()),
            },
            Expr::Struct(r) if !r.table.is_empty() && !tables.contains(&r.table.to_ascii_lowercase()) => {
                error = Some("a formula refers to a table on another sheet.".into());
            }
            Expr::Name(n) => {
                if source.sheets.iter().flat_map(|s| &s.tables).any(|t| t.name.eq_ignore_ascii_case(n)) && !tables.contains(&n.to_ascii_lowercase()) {
                    error = Some("a formula refers to a table on another sheet.".into());
                }
                let found = source
                    .names
                    .iter()
                    .position(|d| d.scope == Some(sheet) && d.name.eq_ignore_ascii_case(n))
                    .or_else(|| source.names.iter().position(|d| d.scope.is_none() && d.name.eq_ignore_ascii_case(n)));
                if let Some(index) = found {
                    needed.insert(index);
                } else if n.contains(['!', '[', ']'])
                    || destination.names.iter().any(|d| d.name.eq_ignore_ascii_case(n))
                    || destination.sheets.iter().flat_map(|s| &s.tables).any(|t| t.name.eq_ignore_ascii_case(n))
                {
                    error = Some(format!("the name '{n}' cannot be resolved safely in the destination."));
                }
            }
            Expr::Call(n, _) if n.eq_ignore_ascii_case("INDIRECT") => error = Some("INDIRECT references cannot be checked safely.".into()),
            _ => {}
        }
        e
    });
    if let Some(error) = error {
        return Err(refuse(error));
    }
    Ok(gridcraft_formula::print(&mapped))
}

fn remap_styles(source: &Workbook, destination: &mut Workbook, sheet: &mut Sheet) {
    let mut ids = HashMap::new();
    let mut map = |id: StyleId| *ids.entry(id).or_insert_with(|| destination.styles.intern(source.styles.get(id).clone()));
    let default = map(StyleId::DEFAULT);
    let cells: Vec<_> = sheet.cells.iter().map(|(c, cell)| (c, cell.style)).collect();
    for (c, old) in cells {
        let new = map(old);
        if new != old
            && let Some(cell) = sheet.cells.get_mut(c)
        {
            cell.style = new;
        }
    }
    for line in sheet.rows.values_mut().chain(sheet.cols.values_mut()) {
        if let Some(old) = line.style {
            line.style = Some(map(old));
        }
    }
    // A worksheet has no separate Normal style. Preserve implicit formatting through
    // column defaults rather than materialising its millions of empty cells.
    if default != StyleId::DEFAULT {
        for col in 0..MAX_COLS {
            sheet.cols.entry(col).or_default().style.get_or_insert(default);
        }
    }
}

fn check_inbound(source: &Workbook, moving: usize, tables: &HashSet<String>) -> Result<()> {
    let name = source.sheet(moving).map(|s| s.name.as_str()).unwrap_or("");
    let mut check = |text: &str| -> Result<String> {
        if text.is_empty() {
            return Ok(String::new());
        }
        let e = gridcraft_formula::parse(text).map_err(|_| refuse("a remaining formula could not be checked."))?;
        let mut links = false;
        e.walk(&mut |e| match e {
            Expr::Ref(r) => {
                links |= match &r.sheet {
                    SheetSel::Named(s) => s.eq_ignore_ascii_case(name),
                    SheetSel::Span(a, b) => {
                        let a = source.sheet_index(a);
                        let b = source.sheet_index(b);
                        a.zip(b).is_some_and(|(a, b)| (a.min(b)..=a.max(b)).contains(&moving))
                    }
                    SheetSel::Current => false,
                }
            }
            Expr::Struct(r) => links |= tables.contains(&r.table.to_ascii_lowercase()),
            Expr::Name(n) => {
                links |= tables.contains(&n.to_ascii_lowercase())
                    || n.rsplit_once('!').is_some_and(|(sh, _)| sh.trim_matches('\'').replace("''", "'").eq_ignore_ascii_case(name));
            }
            Expr::Call(n, _) if n.eq_ignore_ascii_case("INDIRECT") => links = true,
            _ => {}
        });
        if links {
            Err(refuse("another sheet or defined name depends on the sheet being moved. Use Create a copy instead."))
        } else {
            Ok(text.to_string())
        }
    };
    for (index, sh) in source.sheets.iter().enumerate() {
        if index != moving {
            map_formulas(&mut (**sh).clone(), &mut check)?;
            for pivot in &sh.pivots {
                check(&pivot.source)?;
            }
            if sh.slicers.iter().any(|s| tables.contains(&s.table.to_ascii_lowercase())) {
                return Err(refuse("a slicer depends on a table on the sheet being moved."));
            }
        }
    }
    for n in &source.names {
        if n.scope != Some(moving) {
            check(&n.formula)?;
        }
    }
    Ok(())
}

fn map_formulas(sheet: &mut Sheet, f: &mut impl FnMut(&str) -> Result<String>) -> Result<()> {
    let formulas: Vec<_> = sheet.cells.iter().filter_map(|(c, cell)| cell.formula.as_ref().map(|v| (c, v.clone()))).collect();
    for (c, old) in formulas {
        let text = f(&old.text)?;
        if text != old.text {
            let mut formula = gridcraft_model::Formula::new(&text);
            formula.array = old.array;
            if let Some(cell) = sheet.cells.get_mut(c) {
                cell.formula = Some(Arc::new(formula));
            }
        }
    }
    let optional = |v: &mut Option<String>, f: &mut dyn FnMut(&str) -> Result<String>| -> Result<()> {
        if let Some(text) = v {
            *text = f(text)?;
        }
        Ok(())
    };
    for t in &mut sheet.tables {
        for c in &mut t.columns {
            optional(&mut c.formula, f)?;
            if c.totals == TotalsFn::Custom {
                optional(&mut c.totals_label, f)?;
            }
        }
    }
    for c in &mut sheet.charts {
        optional(&mut c.source, f)?;
        for s in &mut c.series {
            s.values = f(&s.values)?;
            optional(&mut s.categories, f)?;
            optional(&mut s.name, f)?;
        }
    }
    for s in &mut sheet.sparklines {
        s.source = f(&s.source)?;
    }
    for h in sheet.hyperlinks.values_mut() {
        if !h.external {
            let prefix = if h.target.starts_with('#') { "#" } else { "" };
            h.target = format!("{prefix}{}", f(h.target.trim_start_matches('#'))?);
        } else if !h.target.contains(":") && !h.target.starts_with('/') && !h.target.starts_with('\\') {
            return Err(refuse("relative file hyperlinks cannot yet be transferred safely."));
        }
    }
    for v in &mut sheet.validations {
        // Match list_items: references and names may be written without a leading =.
        let literal_list = v.kind == gridcraft_model::ValidationKind::List
            && (v.f1.trim().starts_with('"')
                || (!v.f1.contains('!') && gridcraft_formula::parse(&v.f1).map(|e| !matches!(e, Expr::Ref(_) | Expr::Name(_))).unwrap_or(true)));
        if !literal_list {
            v.f1 = f(&v.f1)?;
        }
        optional(&mut v.f2, f)?;
    }
    let threshold = |v: &mut CfValueKind, f: &mut dyn FnMut(&str) -> Result<String>| -> Result<()> {
        if let CfValueKind::Formula(text) = v {
            *text = f(text)?;
        }
        Ok(())
    };
    for cf in &mut sheet.cond_formats {
        match &mut cf.rule {
            CfRule::CellIs { a, b, .. } => {
                *a = f(a)?;
                optional(b, f)?;
            }
            CfRule::Expression { formula, .. } => *formula = f(formula)?,
            CfRule::ColorScale { stops } => {
                for (v, _) in stops {
                    threshold(v, f)?;
                }
            }
            CfRule::DataBar { min, max, .. } => {
                threshold(min, f)?;
                threshold(max, f)?;
            }
            CfRule::IconSet { thresholds, .. } => {
                for v in thresholds {
                    threshold(v, f)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}
