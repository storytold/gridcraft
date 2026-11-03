//! Chart tools facade: shared selection snapshot and stable crate-level entry points.
//! Menus use clipboard/export actions and templates; those implementations live in child modules.
use crate::SheetApp;
use gridcraft_engine::model::Chart;

mod clipboard;
mod export;
mod menus;
mod templates;

pub(crate) use clipboard::ChartClipboard;
pub(crate) use menus::{context_menu, element_menu};
pub(crate) use templates::{apply_template, save_template, set_default_template};

#[cfg(test)]
use clipboard::fingerprint;

#[cfg(test)]
#[path = "chart_clipboard_tests.rs"]
mod tests;

impl SheetApp {
    pub(crate) fn selected_chart_snapshot(&self) -> Option<Chart> {
        let id = self.selected_chart()?;
        self.session.active()?.wb.active()?.charts.iter().find(|c| c.id == id).cloned()
    }
}
