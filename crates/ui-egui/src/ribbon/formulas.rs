use super::controls::{act, anchor_keytip, control_keytip, invoke, keytip, menu_items, popup_command, sep};
use super::{Cat, Icon, SheetApp, Tokens, Ui, big_button, big_split_button, json, keytip_table, small_button, small_split_button};
use crate::function_library;
use crate::icons::FnGlyph;
use crate::widgets::small_button_accent;

/// How many cells Formulas › Error Checking › Circular References lists.
const CIRCULAR_MENU_LIMIT: usize = 100;

/// The cell listed for each circular reference, for the Circular References submenu: each once, in the order
/// the calculation found them, as (sheet index, cell, "Sheet!A1").
fn circular_references(app: &SheetApp) -> Vec<(usize, String, String)> {
    let Some(d) = app.session.active() else { return Vec::new() };
    let mut seen = std::collections::HashSet::new();
    d.calc
        .circular
        .iter()
        .filter(|k| seen.insert(**k))
        .filter_map(|(si, c)| d.wb.sheet(*si).map(|sh| (*si, c.a1(), format!("{}!{}", gridcraft_engine::formula::quote_sheet(&sh.name), c.a1()))))
        .take(CIRCULAR_MENU_LIMIT)
        .collect()
}

/// What a Function Library button's menu lists.
#[derive(Clone, Copy, Debug)]
enum LibraryMenu {
    Recent,
    Category(&'static str),
    More,
}

/// One Function Library category button: its label, its menu, the glyph and colour of its book,
/// and the control id its keytip sequence targets in `keytips.json`.
struct LibraryItem {
    label: &'static str,
    menu: LibraryMenu,
    book: (FnGlyph, Cat),
    keytip: &'static str,
}

impl LibraryItem {
    const fn new(label: &'static str, menu: LibraryMenu, book: (FnGlyph, Cat), keytip: &'static str) -> LibraryItem {
        LibraryItem { label, menu, book, keytip }
    }
}

/// The Function Library categories after AutoSum, column by column (three rows each). Every
/// category has its own colour.
const LIBRARY: [LibraryItem; 8] = [
    LibraryItem::new("Recently Used", LibraryMenu::Recent, (FnGlyph::Star, Cat::Amber), "formulas.recentlyUsed"),
    LibraryItem::new("Financial", LibraryMenu::Category("Financial"), (FnGlyph::Coins, Cat::Green), "formulas.financial"),
    LibraryItem::new("Logical", LibraryMenu::Category("Logical"), (FnGlyph::Question, Cat::Purple), "formulas.logical"),
    LibraryItem::new("Text", LibraryMenu::Category("Text"), (FnGlyph::Letter, Cat::Blue), "formulas.text"),
    LibraryItem::new("Date & Time", LibraryMenu::Category("DateTime"), (FnGlyph::Clock, Cat::Orange), "formulas.dateTime"),
    LibraryItem::new("Lookup & Reference", LibraryMenu::Category("Lookup"), (FnGlyph::Magnifier, Cat::Teal), "formulas.lookup"),
    LibraryItem::new("Math & Trig", LibraryMenu::Category("MathTrig"), (FnGlyph::Theta, Cat::Red), "formulas.mathTrig"),
    LibraryItem::new("More Functions", LibraryMenu::More, (FnGlyph::Dots, Cat::Slate), "formulas.moreFunctions"),
];

/// One Function Library category button: a book in the category's colour that opens the
/// category's menu.
fn library_button(app: &mut SheetApp, ui: &mut Ui, item: &LibraryItem) {
    let (glyph, cat) = item.book;
    let accent = Some(Tokens::get(ui.ctx()).cat(cat));
    let anchor = small_button_accent(ui, Icon::FnBook(glyph), accent, item.label, item.label, true);
    control_keytip(app, ui, &anchor, item.keytip);
    let forced = invoke(app, item.keytip);
    egui::Popup::menu(&anchor).open_memory(popup_command(forced, anchor.clicked())).show(|ui| match item.menu {
        LibraryMenu::Recent => function_library::recent_menu(app, ui),
        LibraryMenu::Category(id) => function_library::category_menu(app, ui, id),
        LibraryMenu::More => function_library::more_menu(app, ui),
    });
}

/// The Formulas tab: Insert Function, AutoSum, the function-category menus (names in the formula
/// language, descriptions in the interface language), defined names, auditing and calculation.
pub(super) fn formulas(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::Function, "Insert\nFunction", "Insert Function (⇧F3)", false);
    control_keytip(app, ui, &__k, "dialog:insertFunction");
    if __k.clicked() {
        app.open_dialog("insertFunction", json!({}));
    }
    // Function Library: AutoSum as a big split button, then the categories in columns of three
    // labelled small buttons, as Excel lays the group out when the window is not wide enough for
    // a big button per category.
    let (sum, sum_arrow) = big_split_button(ui, Icon::Sum, "AutoSum", "AutoSum");
    control_keytip(app, ui, &sum_arrow, "formulas.autoSum");
    if sum.clicked() {
        act(app, "formulas.autoSum", json!({}));
    }
    let forced = invoke(app, "formulas.autoSum");
    egui::Popup::menu(&sum_arrow).open_memory(popup_command(forced, sum_arrow.clicked())).show(|ui| function_library::auto_sum_menu(app, ui));
    for column in LIBRARY.chunks(3) {
        ui.vertical(|ui| {
            for item in column {
                library_button(app, ui, item);
            }
        });
    }
    sep(ui);
    let __k = big_button(ui, Icon::Name, "Name\nManager", "Name Manager", false);
    control_keytip(app, ui, &__k, "dialog:nameManager");
    if __k.clicked() {
        app.open_dialog("nameManager", json!({}));
    }
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Name, "Define Name", "Define Name", false);
        control_keytip(app, ui, &__k, "dialog:defineName");
        if __k.clicked() {
            app.open_dialog("defineName", json!({}));
        }
        let use_in = small_button(ui, Icon::Function, "Use in Formula", "Use in Formula", true);
        anchor_keytip(app, ui, &use_in, &["dialog:pasteName"]);
        egui::Popup::menu(&use_in).show(|ui| {
            // The menu lists the names Paste Names lists; clicking one runs `formulas.useInFormula`, which puts it at the caret.
            // Listed when the menu opens, and again only after the workbook changes.
            let cache = egui::Id::unique("gridcraft.use_in_formula_names");
            let key = app.session.active().map(|d| (d.uid, d.revision, d.wb.active_sheet()));
            let cached = ui.ctx().data(|m| m.get_temp::<(Option<(u64, u64, usize)>, std::sync::Arc<Vec<String>>)>(cache)).filter(|(k, _)| *k == key);
            let names = match cached {
                Some((_, names)) => names,
                None => {
                    let names: std::sync::Arc<Vec<String>> = std::sync::Arc::new(
                        app.session
                            .active()
                            .map(|d| gridcraft_engine::cmd::formulas::pasteable_names(&d.wb).into_iter().map(|(n, _)| n).collect())
                            .unwrap_or_default(),
                    );
                    ui.ctx().data_mut(|m| m.insert_temp(cache, (key, names.clone())));
                    names
                }
            };
            for n in names.iter() {
                if ui.button(n).clicked() {
                    app.run_or_alert("formulas.useInFormula", json!({"name": n}));
                }
            }
            if !names.is_empty() {
                ui.separator();
            }
            if ui.button(app.ui.language.tr("Paste Names…")).clicked() {
                app.open_dialog("pasteName", json!({}));
            }
        });
        let __k = small_button(ui, Icon::Table, "Create from Selection", "Create from Selection", false);
        control_keytip(app, ui, &__k, "formulas.createFromSelection");
        if __k.clicked() {
            act(app, "formulas.createFromSelection", json!({}));
        }
    });
    sep(ui);
    ui.vertical(|ui| {
        if small_button(ui, Icon::Trace, "Trace Precedents", "Trace Precedents", false).clicked()
            && let Ok(r) = app.run("formulas.tracePrecedents", json!({}))
        {
            app.grid.trace = crate::grid::Trace::new(app, r);
        }
        if small_button(ui, Icon::Trace, "Trace Dependents", "Trace Dependents", false).clicked()
            && let Ok(r) = app.run("formulas.traceDependents", json!({}))
        {
            app.grid.trace = crate::grid::Trace::new(app, r);
        }
        if small_button(ui, Icon::Close, "Remove Arrows", "Remove Arrows", false).clicked() {
            app.grid.trace = None;
        }
    });
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Fx, "Show Formulas", "Show Formulas", false);
        control_keytip(app, ui, &__k, "formulas.showFormulas");
        if __k.clicked() {
            act(app, "formulas.showFormulas", json!({}));
        }
        let (ec, ec_arrow) = small_split_button(ui, Icon::Validation, "Error Checking", "Error Checking");
        control_keytip(app, ui, &ec, "dialog:errorChecking");
        if ec.clicked() {
            app.open_dialog("errorChecking", json!({}));
        }
        egui::Popup::menu(&ec_arrow).show(|ui| {
            let lang = app.ui.language;
            if ui.button(format!("{}…", lang.tr("Error Checking"))).clicked() {
                app.open_dialog("errorChecking", json!({}));
            }
            // A cell without an error, or one its error starts in without references, has no
            // arrows: the ones shown stay.
            if ui.button(lang.tr("Trace Error")).clicked()
                && let Ok(r) = app.run("formulas.traceError", json!({}))
                && r["arrows"].as_array().is_some_and(|a| !a.is_empty())
            {
                app.grid.trace = crate::grid::Trace::new(app, r);
            }
            let circular = circular_references(app);
            ui.add_enabled_ui(!circular.is_empty(), |ui| {
                ui.menu_button(lang.tr("Circular References"), |ui| {
                    egui::ScrollArea::vertical().id_salt("circular_references").max_height(400.0).show(ui, |ui| {
                        for (sheet, cell, label) in circular {
                            if ui.button(label).clicked() {
                                act(app, "selection.set", json!({"range": cell, "sheet": sheet}));
                                app.grid.ensure_visible = true;
                            }
                        }
                    });
                });
            });
        });
        let __k = small_button(ui, Icon::Calc, "Evaluate Formula", "Evaluate Formula", false);
        control_keytip(app, ui, &__k, "dialog:evaluateFormula");
        if __k.clicked() {
            app.open_dialog("evaluateFormula", json!({}));
        }
    });
    if big_button(ui, Icon::Search, "Watch\nWindow", "Watch Window", false).clicked() {
        app.grid.pane = Some("watch".into());
    }
    sep(ui);
    let co = big_button(ui, Icon::Calc, "Calculation\nOptions", "Calculation Options", true);
    for sequence in keytip_table::group_extension(app.keytips.prefix().unwrap_or_default(), &["formulas.calculationOptions"]) {
        keytip(app, ui, sequence, co.rect.center_bottom());
    }
    egui::Popup::menu(&co).show(|ui| {
        menu_items(
            app,
            ui,
            &[
                ("Automatic", "formulas.calculationOptions", json!({"mode": "automatic"})),
                ("Automatic Except for Data Tables", "formulas.calculationOptions", json!({"mode": "automaticExceptTables"})),
                ("Manual", "formulas.calculationOptions", json!({"mode": "manual"})),
            ],
        );
    });
    ui.vertical(|ui| {
        let __k = small_button(ui, Icon::Calc, "Calculate Now", "Calculate Now (F9)", false);
        control_keytip(app, ui, &__k, "formulas.calculateNow");
        if __k.clicked() {
            act(app, "formulas.calculateNow", json!({}));
        }
        let __k = small_button(ui, Icon::Sheet, "Calculate Sheet", "Calculate Sheet (⇧F9)", false);
        control_keytip(app, ui, &__k, "formulas.calculateSheet");
        if __k.clicked() {
            act(app, "formulas.calculateSheet", json!({}));
        }
    });
}

#[cfg(test)]
mod function_library_tests {
    use super::*;

    #[test]
    fn every_function_library_book_has_its_own_glyph_and_colour() {
        let books: Vec<(FnGlyph, Cat)> = LIBRARY.iter().map(|i| i.book).collect();
        assert_eq!(books.len(), 8, "every category is a coloured book");
        for (i, (glyph, cat)) in books.iter().enumerate() {
            assert!(books.iter().skip(i + 1).all(|(g, c)| g != glyph && c != cat), "{glyph:?} / {cat:?} is used twice");
        }
        assert!(matches!(LIBRARY.first().map(|i| i.menu), Some(LibraryMenu::Recent)));
        for item in &LIBRARY {
            assert!(!keytip_table::sequences_for(item.keytip).is_empty(), "{} has a keytip sequence", item.label);
        }
    }
}
