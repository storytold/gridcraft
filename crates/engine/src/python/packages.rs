//! What a workbook's code imports that this Python doesn't have. Worked out from the code alone, and
//! only for code or a package list it hasn't seen: an edit reads the one cell that changed. Nothing
//! is reported while the list isn't known, since an empty list would make every import look missing.

use std::collections::HashMap;
use std::sync::Arc;

use gridcraft_core::CellRef;
use gridcraft_model::Workbook;
use gridcraft_python::lint::missing_modules_each;

use super::PythonStats;
use super::inputs::code_hash;
use super::plan::DocRun;

/// How many Python cells are read, in run order.
const MAX_CELLS: usize = 10_000;
/// How many missing packages are reported.
const MAX_MISSING: usize = 100;

/// One module the workbook needs, and the first cell that imports it: `None` when the initialization
/// code does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Missing {
    pub module: String,
    pub place: Option<(usize, CellRef)>,
}

/// The state of the check, for one workbook.
#[derive(Default)]
pub(super) struct PackageCheck {
    /// The workbook and the package list the answer is for.
    checked: Option<(Arc<Workbook>, u64)>,
    /// What each cell's code is missing, by its hash.
    by_code: HashMap<u64, Vec<String>>,
    pub missing: Vec<Missing>,
}

impl DocRun {
    /// Brings [`PackageCheck::missing`] up to date. `generation` tells package lists apart.
    pub fn check_packages(&mut self, wb: &Arc<Workbook>, generation: u64, packages: &[(String, String)], stats: &mut PythonStats) {
        if !self.tracked || packages.is_empty() {
            self.packages = PackageCheck::default();
            return;
        }
        let list_changed = self.packages.checked.as_ref().is_none_or(|(_, g)| *g != generation);
        if !list_changed && self.packages.checked.as_ref().is_some_and(|(seen, _)| Arc::ptr_eq(seen, wb)) {
            return;
        }
        if list_changed {
            self.packages.by_code.clear();
        }
        self.read_new_code(wb, packages, stats);
        self.collect_missing(wb);
        self.packages.checked = Some((wb.clone(), generation));
    }

    /// The initialization code a person wrote, with its hash. What the application that wrote the file
    /// put there says what it lacks when it runs, without asking for what the worker gives lazily.
    fn person_init(wb: &Workbook) -> Option<(u64, &str)> {
        wb.python_env.as_deref().filter(|e| e.user_modified && e.has_code()).map(|e| (code_hash(&e.code, false), e.code.as_str()))
    }

    /// Reads the code not seen under this package list, once per piece of code.
    fn read_new_code(&mut self, wb: &Workbook, packages: &[(String, String)], stats: &mut PythonStats) {
        let mut hashes = Vec::new();
        let mut codes = Vec::new();
        if let Some((hash, code)) = Self::person_init(wb).filter(|(hash, _)| !self.packages.by_code.contains_key(hash)) {
            hashes.push(hash);
            codes.push(code);
        }
        for e in self.entries.iter().take(MAX_CELLS) {
            if self.packages.by_code.contains_key(&e.code_hash) || hashes.contains(&e.code_hash) {
                continue;
            }
            if let Some(py) = wb.sheet(e.sheet).and_then(|sh| sh.cell(e.at)).and_then(|c| c.py.as_deref()) {
                hashes.push(e.code_hash);
                codes.push(py.code.as_str());
            }
        }
        if codes.is_empty() {
            return;
        }
        stats.code_reads = stats.code_reads.saturating_add(codes.len() as u64);
        let names: Vec<String> = packages.iter().map(|(name, _)| name.clone()).collect();
        // Code that has gone since leaves its answer behind, so the map is bounded here.
        if self.packages.by_code.len().saturating_add(codes.len()) > MAX_CELLS.saturating_mul(2) {
            self.packages.by_code.clear();
        }
        self.packages.by_code.extend(hashes.into_iter().zip(missing_modules_each(codes, &names)));
    }

    fn collect_missing(&mut self, wb: &Workbook) {
        let mut missing: Vec<Missing> = Vec::new();
        let mut add = |code: u64, place: Option<(usize, CellRef)>| {
            for module in self.packages.by_code.get(&code).into_iter().flatten() {
                if missing.len() < MAX_MISSING && !missing.iter().any(|m| m.module == *module) {
                    missing.push(Missing { module: module.clone(), place });
                }
            }
        };
        if let Some((hash, _)) = Self::person_init(wb) {
            add(hash, None);
        }
        for e in self.entries.iter().take(MAX_CELLS) {
            add(e.code_hash, Some((e.sheet, e.at)));
        }
        self.packages.missing = missing;
    }
}
