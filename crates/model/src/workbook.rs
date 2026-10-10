//! Workbooks.

use std::sync::Arc;

use gridcraft_core::DateSystem;
use gridcraft_formula::Expr;
use serde::{Deserialize, Serialize};

use crate::cell::Formula;
use crate::sheet::Sheet;
use crate::style::{StyleTable, Theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalcMode {
    #[default]
    Automatic,
    AutomaticExceptTables,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalcSettings {
    pub mode: CalcMode,
    pub iterative: bool,
    pub max_iterations: u32,
    pub max_change: f64,
    /// Use precision as displayed.
    pub precision_as_displayed: bool,
    /// Calculate on several threads (Excel's "Enable multi-threaded calculation"; XLSX
    /// `concurrentCalc`).
    #[serde(default = "multi_threaded_default")]
    pub multi_threaded: bool,
    /// Calculation threads; 0 uses every processor (XLSX `concurrentManualCount`).
    #[serde(default)]
    pub threads: u32,
}

fn multi_threaded_default() -> bool {
    true
}

impl Default for CalcSettings {
    fn default() -> Self {
        CalcSettings {
            mode: CalcMode::Automatic,
            iterative: false,
            max_iterations: 100,
            max_change: 0.001,
            precision_as_displayed: false,
            multi_threaded: true,
            threads: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DefinedName {
    pub name: String,
    /// `None` = workbook scope; otherwise the sheet index.
    pub scope: Option<usize>,
    /// Formula text without `=` (`Sheet1!$A$1:$A$10`, `0.07`, `LAMBDA(x,x*2)`).
    pub formula: String,
    pub comment: String,
    pub hidden: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DocProps {
    pub title: String,
    pub subject: String,
    pub author: String,
    pub company: String,
    pub keywords: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workbook {
    pub sheets: Vec<Arc<Sheet>>,
    pub names: Vec<DefinedName>,
    pub styles: StyleTable,
    pub theme: Theme,
    pub date_system: DateSystem,
    pub calc: CalcSettings,
    pub active_sheet: usize,
    pub props: DocProps,
    /// Workbook structure protection.
    pub protected_structure: bool,
    /// Custom fill lists (Options › Custom Lists) beyond the built-in ones.
    pub custom_lists: Vec<Vec<String>>,
    /// Named cell styles (Cell Styles gallery): name → style.
    pub cell_styles: Vec<(String, crate::style::Style)>,
}

impl Default for Workbook {
    fn default() -> Self {
        Workbook::new()
    }
}

impl Workbook {
    /// A new workbook with one sheet, "Sheet1".
    pub fn new() -> Workbook {
        Workbook {
            sheets: vec![Arc::new(Sheet::new("Sheet1"))],
            names: vec![],
            styles: StyleTable::default(),
            theme: Theme::default(),
            date_system: DateSystem::D1900,
            calc: CalcSettings::default(),
            active_sheet: 0,
            props: DocProps::default(),
            protected_structure: false,
            custom_lists: vec![],
            cell_styles: vec![],
        }
    }
    pub fn sheet(&self, i: usize) -> Option<&Sheet> {
        self.sheets.get(i).map(|s| s.as_ref())
    }
    pub fn sheet_mut(&mut self, i: usize) -> Option<&mut Sheet> {
        self.sheets.get_mut(i).map(Arc::make_mut)
    }
    pub fn sheet_index(&self, name: &str) -> Option<usize> {
        self.sheets.iter().position(|s| s.name.eq_ignore_ascii_case(name))
    }
    pub fn active(&self) -> Option<&Sheet> {
        self.sheet(self.active_sheet)
    }
    /// A fresh sheet name `SheetN` not in use.
    pub fn next_sheet_name(&self) -> String {
        (1..).map(|n| format!("Sheet{n}")).find(|n| self.sheet_index(n).is_none()).unwrap_or_else(|| "Sheet".into())
    }
    /// Validates a sheet name like Excel: 1–31 chars, none of `: \ / ? * [ ]`, not starting or
    /// ending with `'`, unique (case-insensitive) unless it is `except`'s own name.
    pub fn check_sheet_name(&self, name: &str, except: Option<usize>) -> Result<(), String> {
        let n = name.chars().count();
        if n == 0 {
            return Err("A sheet name can't be blank.".into());
        }
        if n > 31 {
            return Err("A sheet name can't be longer than 31 characters.".into());
        }
        if name.chars().any(|c| matches!(c, ':' | '\\' | '/' | '?' | '*' | '[' | ']')) {
            return Err("A sheet name can't contain any of these characters: : \\ / ? * [ ]".into());
        }
        if name.starts_with('\'') || name.ends_with('\'') {
            return Err("A sheet name can't begin or end with an apostrophe.".into());
        }
        if name.eq_ignore_ascii_case("History") {
            return Err("\"History\" is a reserved name.".into());
        }
        if let Some(i) = self.sheet_index(name)
            && Some(i) != except
        {
            return Err("That name is already taken. Try a different one.".into());
        }
        Ok(())
    }
    /// Finds a defined name visible from `sheet`: sheet scope first, then workbook scope.
    pub fn name(&self, name: &str, sheet: usize) -> Option<&DefinedName> {
        self.names
            .iter()
            .find(|n| n.scope == Some(sheet) && n.name.eq_ignore_ascii_case(name))
            .or_else(|| self.names.iter().find(|n| n.scope.is_none() && n.name.eq_ignore_ascii_case(name)))
    }
    /// Spells calls of LAMBDA names the way the names are defined (`Double(4)`, where the
    /// parser gives `DOUBLE(4)`: it upper-cases function names, as built-in ones are shown).
    pub fn name_call_case(&self, e: Expr) -> Expr {
        spell_calls(e, &self.lambda_names())
    }
    /// [`Workbook::name_call_case`] for every cell formula and defined name (after reading a
    /// file).
    pub fn apply_name_call_case(&mut self) {
        let lambdas = self.lambda_names();
        if lambdas.is_empty() {
            return;
        }
        for sheet in self.sheets.iter_mut() {
            let fixed: Vec<_> = sheet
                .cells
                .iter()
                .filter_map(|(c, cell)| {
                    let f = cell.formula.as_ref()?;
                    let e = f.expr()?;
                    let ne = spell_calls(e.clone(), &lambdas);
                    (ne != e).then(|| (c, Formula { array: f.array, ..Formula::from_expr(ne) }))
                })
                .collect();
            if fixed.is_empty() {
                continue;
            }
            let sheet = Arc::make_mut(sheet);
            for (c, f) in fixed {
                if let Some(cell) = sheet.cells.get_mut(c) {
                    cell.formula = Some(Arc::new(f));
                }
            }
        }
        for n in self.names.iter_mut() {
            if let Ok(e) = gridcraft_formula::parse(&n.formula) {
                let ne = spell_calls(e.clone(), &lambdas);
                if ne != e {
                    n.formula = gridcraft_formula::print(&ne);
                }
            }
        }
    }
    /// Names of the defined names holding a LAMBDA (callable like functions).
    fn lambda_names(&self) -> Vec<String> {
        let is_lambda = |f: &str| f.trim_start_matches('=').trim_start().get(..7).is_some_and(|p| p.eq_ignore_ascii_case("LAMBDA("));
        self.names.iter().filter(|n| is_lambda(&n.formula)).map(|n| n.name.clone()).collect()
    }
    /// Table by name across sheets: (sheet index, table index).
    pub fn table(&self, name: &str) -> Option<(usize, usize)> {
        for (si, s) in self.sheets.iter().enumerate() {
            if let Some(ti) = s.tables.iter().position(|t| t.name.eq_ignore_ascii_case(name)) {
                return Some((si, ti));
            }
        }
        None
    }
    pub fn next_object_id(&self) -> u32 {
        let mut m = 0;
        for s in &self.sheets {
            for c in &s.charts {
                m = m.max(c.id);
            }
            for c in &s.images {
                m = m.max(c.id);
            }
            for c in &s.shapes {
                m = m.max(c.id);
            }
            for c in &s.tables {
                m = m.max(c.id);
            }
        }
        m + 1
    }
}

/// Calls of the names in `lambdas` spelled as the names are.
fn spell_calls(e: Expr, lambdas: &[String]) -> Expr {
    if lambdas.is_empty() {
        return e;
    }
    e.map(&mut |x| match x {
        Expr::Call(n, args) => match lambdas.iter().find(|l| l.eq_ignore_ascii_case(&n)) {
            Some(l) => Expr::Call(l.clone(), args),
            None => Expr::Call(n, args),
        },
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sheet_names() {
        let wb = Workbook::new();
        assert_eq!(wb.next_sheet_name(), "Sheet2");
        assert!(wb.check_sheet_name("sheet1", None).is_err());
        assert!(wb.check_sheet_name("sheet1", Some(0)).is_ok());
        assert!(wb.check_sheet_name("a/b", None).is_err());
        assert!(wb.check_sheet_name(&"x".repeat(32), None).is_err());
        assert!(wb.check_sheet_name("Budget 2026", None).is_ok());
    }
}
