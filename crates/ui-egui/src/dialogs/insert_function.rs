use super::show::{ok_cancel, take_first_focus};
use super::{Dialog, Json, SheetApp, json, vec2};

/// Insert Function: search names (as the formula language spells them) and descriptions, or pick
/// a category (Most Recently Used, All or one category), then a function; each hit shows its
/// category and description. OK starts editing `=NAME(` (or appends to the formula).
pub(super) fn insert_function(app: &mut SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool) {
    use crate::function_library::{self as lib, ALL, MOST_RECENTLY_USED};
    let language = app.ui.language;
    ui.horizontal(|ui| {
        ui.label(language.tr("Search:"));
        let r = ui.add(egui::TextEdit::singleline(&mut d.search).desired_width(300.0).hint_text(language.tr("e.g. lookup, average, date")));
        // Once only, so Tab can move on to the category list.
        take_first_focus(d, &r);
    });
    // A new search (typed, or set through the control channel) looks through every function, as
    // Excel's does.
    if d.values.get("searched").and_then(Json::as_str) != Some(d.search.as_str()) {
        d.list_index = 0;
        if !d.search.trim().is_empty() {
            d.values.insert("category".into(), json!(ALL));
        }
        d.values.insert("searched".into(), json!(d.search));
    }
    // The category may be given by label or by engine id (a Function Library menu, `ui.dialog.set`).
    let saved = d.values.get("category").and_then(Json::as_str).unwrap_or(MOST_RECENTLY_USED).to_string();
    let mut category = if saved.eq_ignore_ascii_case(ALL) {
        ALL.to_string()
    } else {
        lib::category_id(&saved).map(lib::category_label).unwrap_or(MOST_RECENTLY_USED).to_string()
    };
    // The category list is the dialog's only menu, so a menu open as the frame starts is its own
    // (Enter on a row picks it and closes the menu in this frame).
    let menu_was_open = egui::Popup::is_any_open(ui.ctx());
    let picker = ui.horizontal(|ui| {
        let label = ui.label(language.tr("Or select a category:"));
        craft_ui::ComboBox::from_id_salt("insertFunctionCategory")
            .width(220.0)
            .selected_text(language.tr(&category))
            .show_ui(ui, |ui| {
                for c in [MOST_RECENTLY_USED, ALL].into_iter().chain(lib::CATEGORIES.iter().map(|(_, l)| *l)) {
                    ui.selectable_value(&mut category, c.to_string(), language.tr(c));
                }
            })
            .response
            .labelled_by(label.id)
    });
    // Enter on the category list or in its menu opens it or picks a category, as Space does
    // (egui clicks a focused control on either key); it does not also confirm the dialog.
    let picker = picker.inner;
    if menu_was_open || picker.has_focus() || craft_ui::ComboBox::is_open(ui.ctx(), picker.id) {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    }
    if category != saved {
        d.list_index = 0;
    }
    d.values.insert("category".into(), json!(category));
    let locale = language.formula_locale();
    // The names and descriptions searched are the ones shown, so in the interface language.
    let q = locale.lowercase(d.search.trim());
    let tr_fold = |s: &str| -> String {
        s.chars()
            .map(|c| match c {
                'i' | 'İ' | 'ı' | 'I' => 'i',
                'ç' | 'Ç' => 'ç',
                'ğ' | 'Ğ' => 'ğ',
                'ö' | 'Ö' => 'ö',
                'ş' | 'Ş' => 'ş',
                'ü' | 'Ü' => 'ü',
                other => other.to_ascii_lowercase(),
            })
            .collect()
    };
    let q_tr = tr_fold(d.search.trim());
    let matches = |f: &lib::FnEntry| {
        if q.is_empty() {
            return true;
        }
        if locale == gridcraft_engine::formula::FormulaLocale::Tr {
            tr_fold(locale.function_name(&f.name)).contains(&q_tr)
                || tr_fold(&f.name).contains(&q_tr)
                || tr_fold(language.function_description(&f.description).unwrap_or(&f.description)).contains(&q_tr)
                || tr_fold(&f.description).contains(&q_tr)
        } else {
            locale.lowercase(locale.function_name(&f.name)).contains(&q)
                || f.name.to_lowercase().contains(&q)
                || locale.lowercase(language.function_description(&f.description).unwrap_or(&f.description)).contains(&q)
                || f.description.to_lowercase().contains(&q)
        }
    };
    let hits: Vec<&lib::FnEntry> = if category == MOST_RECENTLY_USED {
        lib::recent(app).iter().filter_map(|n| lib::find(n)).filter(|f| matches(f)).collect()
    } else {
        let id = lib::category_id(&category);
        // The catalog is fixed (some 500 functions), so All lists every one.
        lib::all().iter().filter(|f| id.is_none_or(|id| f.category == id) && matches(f)).collect()
    };
    let t = crate::theme::Tokens::get(ui.ctx());
    let font = egui::TextStyle::Body.resolve(ui.style());
    let rows: Vec<(&str, &str)> = hits.iter().map(|f| (locale.function_name(&f.name), language.tr(lib::category_label(&f.category)))).collect();
    // The category column starts after the widest name shown, so a long name in any formula
    // language or font never runs into it.
    let name_w = rows.iter().map(|(n, _)| ui.painter().layout_no_wrap(n.to_string(), font.clone(), t.text).size().x).fold(0.0_f32, f32::max);
    egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, false]).show(ui, |ui| {
        if rows.is_empty() {
            ui.label(egui::RichText::new(language.tr("No functions match.")).italics());
        }
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        for (i, (name, category)) in rows.iter().enumerate() {
            let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width().max(name_w + 120.0), row_h), egui::Sense::click());
            if i == d.list_index {
                ui.painter().rect_filled(rect, 3.0, ui.visuals().selection.bg_fill);
            } else if r.hovered() {
                ui.painter().rect_filled(rect, 3.0, t.hover);
            }
            let color = if i == d.list_index { ui.visuals().selection.stroke.color } else { t.text };
            ui.painter().text(rect.left_center() + vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, *name, font.clone(), color);
            ui.painter().text(rect.left_center() + vec2(6.0 + name_w + 16.0, 0.0), egui::Align2::LEFT_CENTER, *category, font.clone(), t.text_dim);
            r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, i == d.list_index, format!("{name} ({category})")));
            if r.clicked() {
                d.list_index = i;
            }
            if r.double_clicked() {
                d.list_index = i;
                *confirm = true;
            }
        }
    });
    if let Some(f) = hits.get(d.list_index) {
        ui.separator();
        ui.label(egui::RichText::new(crate::formula_locale::signature(&f.name, locale, language).unwrap_or_default()).strong());
        ui.label(crate::formula_locale::description(&f.name, locale, language).unwrap_or_default());
    }
    let mut open = true;
    ok_cancel(ui, confirm, &mut open);
    // The function counts as recently used once the formula is entered (the engine notes it).
    if *confirm && let Some(f) = hits.get(d.list_index) {
        let n = app
            .session
            .active()
            .map(|doc| crate::formula_locale::completion_name(&f.name, locale, &doc.wb, doc.wb.active_sheet()))
            .unwrap_or(&f.name);
        let cur = app.editor.as_ref().map(|e| e.text.clone());
        let text = match cur {
            Some(t) if t.starts_with('=') => format!("{t}{n}("),
            _ => format!("={n}("),
        };
        app.begin_edit(Some(text), false);
    }
    if !open {
        *confirm = true;
    }
}
