use super::controls::{act, anchor_keytip, control_keytip, invoke, keytip, menu_items, popup_command, sep};
use super::home::autosum_menu;
use super::{Icon, SheetApp, Ui, big_button, big_split_button, json, keytip_table, small_button, small_split_button};

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

/// The Formulas tab: Insert Function, AutoSum, the function-category menus (names in the formula
/// language, descriptions in the interface language), defined names, auditing and calculation.
pub(super) fn formulas(app: &mut SheetApp, ui: &mut Ui) {
    let __k = big_button(ui, Icon::Function, "Insert\nFunction", "Insert Function (⇧F3)", false);
    control_keytip(app, ui, &__k, "dialog:insertFunction");
    if __k.clicked() {
        app.open_dialog("insertFunction", json!({}));
    }
    let (s, sa) = big_split_button(ui, Icon::Sum, "AutoSum", "AutoSum");
    control_keytip(app, ui, &sa, "formulas.autoSum");
    if s.clicked() {
        act(app, "formulas.autoSum", json!({}));
    }
    egui::Popup::menu(&sa).show(|ui| autosum_menu(app, ui));
    for (label, cat, cid) in [
        ("Financial", "Financial", "formulas.financial"),
        ("Logical", "Logical", "formulas.logical"),
        ("Text", "Text", "formulas.text"),
        ("Date &\nTime", "DateTime", "formulas.dateTime"),
        ("Lookup &\nReference", "Lookup", "formulas.lookup"),
        ("Math &\nTrig", "MathTrig", "formulas.mathTrig"),
        ("More\nFunctions", "Statistical", "formulas.moreFunctions"),
    ] {
        let r = big_button(ui, Icon::Book, label, label, true);
        control_keytip(app, ui, &r, cid);
        let forced_c = invoke(app, cid);
        egui::Popup::menu(&r).open_memory(popup_command(forced_c, r.clicked())).show(|ui| {
            ui.set_max_height(420.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for f in gridcraft_engine::cmd::formulas::function_list() {
                    if f["category"].as_str() == Some(cat)
                        && let Some(n) = f["name"].as_str()
                    {
                        let locale = app.ui.language.formula_locale();
                        let name =
                            app.session.active().map(|d| crate::formula_locale::completion_name(n, locale, &d.wb, d.wb.active_sheet())).unwrap_or(n);
                        let desc = crate::formula_locale::description(n, locale, app.ui.language).unwrap_or_default();
                        if ui.button(name).on_hover_text(desc).clicked() {
                            app.run_or_alert("formulas.insertFunction", json!({"name": n}));
                            ui.close();
                        }
                    }
                }
            });
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
