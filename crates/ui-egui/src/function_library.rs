//! The Formulas tab's Function Library: the function categories, their menus, the AutoSum menu
//! (shared with Home › Editing), and the Recently Used menu, which lists what the engine's
//! `formulas.recentlyUsed` returns.

use std::sync::OnceLock;

use egui::{Ui, vec2};
use serde_json::json;

use crate::SheetApp;

/// The Insert Function category that lists [`recent`].
pub const MOST_RECENTLY_USED: &str = "Most Recently Used";
/// The Insert Function category that lists every function.
pub const ALL: &str = "All";

/// The function categories in the order the Insert Function dialog lists them: (the engine's
/// category id, the label people see).
pub const CATEGORIES: [(&str, &str); 14] = [
    ("Financial", "Financial"),
    ("DateTime", "Date & Time"),
    ("MathTrig", "Math & Trig"),
    ("Statistical", "Statistical"),
    ("Lookup", "Lookup & Reference"),
    ("Database", "Database"),
    ("Text", "Text"),
    ("Logical", "Logical"),
    ("Information", "Information"),
    ("Engineering", "Engineering"),
    ("Cube", "Cube"),
    ("Compatibility", "Compatibility"),
    ("Web", "Web"),
    ("AddIn", "Add-in and Automation"),
];

/// The categories under More Functions, each a cascading submenu.
pub const MORE_FUNCTIONS: [&str; 6] = ["Statistical", "Engineering", "Cube", "Information", "Compatibility", "Web"];

/// The label of an engine category id (the id itself if it is unknown).
pub fn category_label(id: &str) -> &str {
    CATEGORIES.iter().find(|(c, _)| *c == id).map(|(_, l)| *l).unwrap_or(id)
}

/// The engine category id for a category given by id or label, in any case.
pub fn category_id(name: &str) -> Option<&'static str> {
    CATEGORIES.iter().find(|(c, l)| c.eq_ignore_ascii_case(name) || l.eq_ignore_ascii_case(name)).map(|(c, _)| *c)
}

/// One function of the library.
#[derive(Clone, Debug)]
pub struct FnEntry {
    pub name: String,
    pub category: String,
    pub description: String,
}

/// Every function, sorted by name (built once; the list is static).
pub fn all() -> &'static [FnEntry] {
    static LIST: OnceLock<Vec<FnEntry>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut list: Vec<FnEntry> = gridcraft_engine::cmd::formulas::function_list()
            .iter()
            .filter_map(|f| {
                Some(FnEntry {
                    name: f["name"].as_str()?.to_string(),
                    category: f["category"].as_str().unwrap_or("").to_string(),
                    description: f["description"].as_str().unwrap_or("").to_string(),
                })
            })
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list.dedup_by(|a, b| a.name == b.name);
        list
    })
}

/// The function named `name` (upper case), if the library knows it.
pub fn find(name: &str) -> Option<&'static FnEntry> {
    let list = all();
    list.binary_search_by(|f| f.name.as_str().cmp(name)).ok().and_then(|i| list.get(i))
}

/// The functions of one engine category, in name order.
pub fn in_category(id: &str) -> impl Iterator<Item = &'static FnEntry> + '_ {
    all().iter().filter(move |f| f.category == id)
}

/// The name a function shows under in the interface language.
fn display_name<'a>(app: &SheetApp, name: &'a str) -> &'a str {
    let locale = app.ui.language.formula_locale();
    app.session.active().map(|d| crate::formula_locale::completion_name(name, locale, &d.wb, d.wb.active_sheet())).unwrap_or(name)
}

/// One menu row per function, with its description on hover; a click starts the formula.
fn function_items<'a>(app: &mut SheetApp, ui: &mut Ui, names: impl Iterator<Item = &'a str>) {
    let language = app.ui.language;
    let locale = language.formula_locale();
    ui.set_max_height(420.0);
    egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
        for n in names {
            let Some(entry) = find(n) else { continue };
            let name = display_name(app, &entry.name);
            let r = ui.add(egui::Button::new(name).frame(false).min_size(vec2(180.0, 20.0)));
            // The description is looked up only while the row is hovered.
            let r = r.on_hover_ui(|ui| {
                ui.label(crate::formula_locale::description(&entry.name, locale, language).unwrap_or_default());
            });
            if r.clicked() {
                app.run_or_alert("formulas.insertFunction", json!({"name": entry.name}));
                ui.close();
            }
        }
    });
}

/// The "Insert Function…" row that ends every category menu; it opens the dialog on `category`.
fn insert_function_item(app: &mut SheetApp, ui: &mut Ui, category: &str) {
    ui.separator();
    let label = app.ui.language.tr("Insert Function…");
    if ui.add(egui::Button::new(label).frame(false).min_size(vec2(180.0, 22.0))).clicked() {
        app.open_dialog("insertFunction", json!({"category": category}));
        ui.close();
    }
}

/// The menu of one category: its functions, then Insert Function….
pub fn category_menu(app: &mut SheetApp, ui: &mut Ui, id: &str) {
    let names: Vec<&str> = in_category(id).map(|f| f.name.as_str()).collect();
    function_items(app, ui, names.into_iter());
    insert_function_item(app, ui, category_label(id));
}

/// The functions `formulas.recentlyUsed` lists, newest first (none without a workbook).
pub fn recent(app: &mut SheetApp) -> Vec<String> {
    let list = app.session.execute("formulas.recentlyUsed", json!({})).unwrap_or_default();
    list.as_array().map(|a| a.iter().filter_map(|n| n.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Recently Used: the most recent functions, newest first, then Insert Function….
pub fn recent_menu(app: &mut SheetApp, ui: &mut Ui) {
    let recent = recent(app);
    function_items(app, ui, recent.iter().map(String::as_str));
    insert_function_item(app, ui, MOST_RECENTLY_USED);
}

/// More Functions: a cascading submenu per remaining category.
pub fn more_menu(app: &mut SheetApp, ui: &mut Ui) {
    for id in MORE_FUNCTIONS {
        let label = app.ui.language.tr(category_label(id)).to_string();
        ui.menu_button(label, |ui| category_menu(app, ui, id));
    }
}

/// The AutoSum menu, shared by Home › Editing and Formulas › Function Library: a function, or
/// the Insert Function dialog.
pub fn auto_sum_menu(app: &mut SheetApp, ui: &mut Ui) {
    for (label, f) in [("Sum", "SUM"), ("Average", "AVERAGE"), ("Count Numbers", "COUNT"), ("Max", "MAX"), ("Min", "MIN")] {
        let label = app.ui.language.tr(label);
        if ui.add(egui::Button::new(label).frame(false).min_size(vec2(160.0, 22.0))).clicked() {
            app.run_or_alert("formulas.autoSum", json!({"function": f}));
            ui.close();
        }
    }
    ui.separator();
    let label = app.ui.language.tr("More Functions…");
    if ui.add(egui::Button::new(label).frame(false).min_size(vec2(160.0, 22.0))).clicked() {
        app.open_dialog("insertFunction", json!({}));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_cover_every_function() {
        for f in all() {
            assert!(category_id(&f.category).is_some(), "{} is in an unknown category {}", f.name, f.category);
        }
        assert_eq!(category_id("date & time"), Some("DateTime"));
        assert_eq!(category_id("Lookup"), Some("Lookup"));
        assert_eq!(category_id("nope"), None);
        for c in MORE_FUNCTIONS {
            assert!(category_id(c).is_some());
        }
        assert!(all().windows(2).all(|w| w[0].name < w[1].name), "sorted by name, without duplicates");
        assert_eq!(find("SUM").map(|f| f.category.as_str()), Some("MathTrig"));
        assert!(find("NOT_A_FUNCTION").is_none());
    }
}
