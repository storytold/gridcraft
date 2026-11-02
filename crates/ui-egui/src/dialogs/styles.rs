//! Home › Cell Styles: New Cell Style… and Merge Styles….

use super::show::ok_cancel;
use super::{Dialog, Field, Json, SheetApp, json};

/// New Cell Style…: a name for a style that takes the active cell's formats (`home.newCellStyle`).
/// It starts on the first free "Style N".
pub(super) fn open_new_cell_style(app: &SheetApp) -> Dialog {
    let styles = app.session.active().map(|d| d.wb.cell_styles.as_slice()).unwrap_or_default();
    let taken = |name: &str| styles.iter().any(|(n, _)| n.eq_ignore_ascii_case(name));
    let name = (1..=styles.len().saturating_add(1)).map(|n| format!("Style {n}")).find(|n| !taken(n)).unwrap_or_default();
    Dialog::form("newCellStyle", "Style", "home.newCellStyle", vec![Field::Text { key: "name", label: "Style name:" }], json!({"name": name}))
}

/// The open workbooks other than the active one (id, name), in the order they were opened.
fn other_workbooks(app: &SheetApp) -> Vec<(u64, String)> {
    let active = app.session.active().map(|d| d.uid);
    app.session.documents().iter().filter(|d| Some(d.uid) != active).map(|d| (d.uid, d.display_title())).collect()
}

/// Merge Styles…, with the first other open workbook chosen.
pub(super) fn open_merge_styles(app: &SheetApp) -> Dialog {
    let first = other_workbooks(app).first().map(|(uid, _)| *uid);
    let mut d = Dialog::custom("mergeStyles", "Merge Styles", json!({"from": first}));
    d.command = Some("home.mergeStyles");
    d
}

/// The styles of the active workbook that merging from workbook `from` would replace with a
/// different definition of the same name.
fn same_names(app: &SheetApp, from: u64) -> Vec<String> {
    let source = app.session.documents().iter().find(|d| d.uid == from);
    let (Some(target), Some(source)) = (app.session.active(), source) else { return vec![] };
    gridcraft_engine::cmd::format::merge_cell_styles(&target.wb.cell_styles, &source.wb.cell_styles, &target.wb.theme, false).replaced
}

/// Merge Styles' body: the list of the other open workbooks, then, when the chosen one has
/// styles with the same names as the active one's, whether to replace them (Yes), keep them and
/// merge the rest (No), or go back to the list (Cancel). OK runs `home.mergeStyles`.
pub(super) fn merge_styles(app: &SheetApp, ui: &mut egui::Ui, d: &mut Dialog, confirm: &mut bool, open: &mut bool) {
    let lang = crate::i18n::current(ui.ctx());
    let others = other_workbooks(app);
    // A workbook closed while the dialog is open leaves the list (and its question goes).
    let stored = d.values.get("from").and_then(Json::as_u64);
    let from = stored.filter(|f| others.iter().any(|(uid, _)| uid == f)).or(others.first().map(|o| o.0));
    if from != stored {
        d.values.remove("clash");
    }
    d.values.insert("from".into(), json!(from));
    let source = others.iter().find(|(uid, _)| Some(*uid) == from).map(|(_, name)| name.as_str());
    // The command names the workbook as its title bar does, so a recorded macro runs again in
    // another session; of two open workbooks with one name, the id tells which.
    let unique = |name: &str| app.session.documents().iter().filter(|d| d.display_title().to_lowercase() == name.to_lowercase()).count() == 1;
    d.values.insert("source".into(), source.filter(|n| unique(n)).map_or(json!(from), |n| json!(n)));
    if let (Some(names), Some(source)) = (d.values.get("clash").and_then(Json::as_array).cloned(), source) {
        ui.label(lang.tr("{name} has styles with the same names as this workbook:").replace("{name}", source));
        let names: Vec<&str> = names.iter().filter_map(Json::as_str).collect();
        let mut shown = names.iter().take(12).copied().collect::<Vec<_>>().join(", ");
        if names.len() > 12 {
            shown.push_str(", …");
        }
        ui.label(egui::RichText::new(shown).strong());
        ui.label(
            lang.tr("Replace them with the styles from {name}? No keeps this workbook's styles and merges the others.").replace("{name}", source),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            for (label, answer) in [("Yes", Some("replace")), ("No", Some("keep")), ("Cancel", None)] {
                if ui.button(format!(" {} ", lang.tr(label))).clicked() {
                    d.values.remove("clash");
                    if let Some(answer) = answer {
                        d.values.insert("sameNames".into(), json!(answer));
                        *confirm = true;
                    }
                }
            }
        });
        return;
    }
    ui.label(lang.tr("Merge styles from:"));
    if others.is_empty() {
        ui.label(lang.tr("No other workbook is open. Open the workbook whose styles you want, then choose Merge Styles again."));
        let mut ok = false;
        ok_cancel(ui, &mut ok, open);
        if ok {
            *open = false;
        }
        return;
    }
    let mut chosen = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
            for (uid, name) in &others {
                let r = ui.selectable_label(from == Some(*uid), name);
                if r.clicked() {
                    d.values.insert("from".into(), json!(uid));
                }
                chosen |= r.double_clicked();
            }
        });
    });
    let mut ok = false;
    ok_cancel(ui, &mut ok, open);
    if (ok || chosen)
        && let Some(from) = d.values.get("from").and_then(Json::as_u64)
    {
        let names = same_names(app, from);
        if names.is_empty() {
            *confirm = true;
        } else {
            // Ask first; the answer goes in `sameNames`.
            d.values.insert("clash".into(), json!(names));
        }
    }
}
