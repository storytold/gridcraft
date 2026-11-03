//! Chart tools facade: shared selection snapshot and stable crate-level entry points.
use crate::SheetApp;
use gridcraft_engine::model::Chart;

mod menus;

pub(crate) use menus::{context_menu, element_menu};

impl SheetApp {
    pub(crate) fn selected_chart_snapshot(&self) -> Option<Chart> {
        let id = self.selected_chart()?;
        self.session.active()?.wb.active()?.charts.iter().find(|c| c.id == id).cloned()
    }
}
