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

/// File › Options › Formulas › Calculation options, stored per workbook (`<calcPr>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CalcSettings {
    pub mode: CalcMode,
    /// Enable iterative calculation: circular references are calculated repeatedly instead of
    /// reporting an error.
    pub iterative: bool,
    /// Maximum Iterations, 1 to [`CalcSettings::MAX_ITERATIONS`].
    pub max_iterations: u32,
    /// Maximum Change: iteration stops once no value in a circular reference changes by more.
    pub max_change: f64,
    /// Use precision as displayed.
    pub precision_as_displayed: bool,
}

impl Default for CalcSettings {
    fn default() -> Self {
        CalcSettings {
            mode: CalcMode::Automatic,
            iterative: false,
            max_iterations: Self::DEFAULT_ITERATIONS,
            max_change: Self::DEFAULT_CHANGE,
            precision_as_displayed: false,
        }
    }
}

impl CalcSettings {
    pub const DEFAULT_ITERATIONS: u32 = 100;
    pub const DEFAULT_CHANGE: f64 = 0.001;
    /// The largest Maximum Iterations Excel accepts.
    pub const MAX_ITERATIONS: u32 = 32_767;

    /// Sets Maximum Iterations, kept within 1 to [`CalcSettings::MAX_ITERATIONS`].
    pub fn set_max_iterations(&mut self, n: u32) {
        self.max_iterations = n.clamp(1, Self::MAX_ITERATIONS);
    }

    /// Sets Maximum Change (its magnitude); a value that isn't finite is ignored.
    pub fn set_max_change(&mut self, d: f64) {
        if d.is_finite() {
            self.max_change = d.abs();
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
    /// Password of the structure protection (XLSX `workbookProtection`), never the password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure_password: Option<crate::PasswordHash>,
    /// Custom fill lists (Options › Custom Lists) beyond the built-in ones.
    pub custom_lists: Vec<Vec<String>>,
    /// Named cell styles (Cell Styles gallery): name → style.
    pub cell_styles: Vec<(String, crate::style::Style)>,
    /// VBA project (`xl/vbaProject.bin`) carried through unchanged. GridCraft does not run
    /// macros; the blob is preserved so that saving a macro-enabled workbook never destroys the
    /// macros it arrived with. base64 in JSON. `None` = the workbook has no macros. Shared, so a
    /// copy of the workbook (every edit's undo step) doesn't copy it.
    #[serde(default, with = "crate::b64::opt")]
    pub vba_project: Option<Arc<Vec<u8>>>,
    /// Field separator of the CSV this workbook was read from, so saving it as CSV writes the same
    /// separator back (a `;` file stays a `;` file). `None` = not read from CSV: CSV saves use `,`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub csv_delimiter: Option<u8>,
    /// Unmodelled file content kept for saving (package root and workbook level). Not in JSON.
    #[serde(skip)]
    pub preserved: Option<Arc<crate::preserved::Preserved>>,
    /// The folder the workbook was read from, which relative paths to linked workbooks in
    /// `preserved` are relative to. Saving into another folder rewrites them from there. Not in JSON.
    #[serde(skip)]
    pub source_dir: Option<std::path::PathBuf>,
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
            structure_password: None,
            custom_lists: vec![],
            cell_styles: vec![],
            vba_project: None,
            csv_delimiter: None,
            preserved: None,
            source_dir: None,
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
            for c in &s.slicers {
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
