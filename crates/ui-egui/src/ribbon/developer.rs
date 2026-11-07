//! Developer tab: the workbook's macros, and Excel's other Developer entries as placeholders.
//!
//! The layout follows the Developer tab observed in Excel 16.0 (build 20527, English UI):
//! Code (Visual Basic, Macros, Record Macro, Use Relative References, Macro Security), Add-ins
//! (Add-ins, Excel Add-ins, COM Add-ins), Controls (Insert, Design Mode, Properties, View Code,
//! Run Dialog) and XML (Source, Map Properties, Expansion Packs, Refresh Data, Import, Export).
//! Macros is the one real control — it runs `view.macros`, which opens the Macros dialog. Every
//! other entry is a disabled stub whose tooltip names #2960: the features are tracked there, and
//! Office Scripts stay in the Automate tab.

use super::controls::{act, control_keytip, sep};
use super::{Icon, SheetApp, Ui, big_button, json};
use crate::widgets::small_button;

/// The tooltip every stub carries: what it is waiting for and the issue that tracks it.
const IN_DEVELOPMENT: &str = "In development — tracked in #2960";

/// Draws the Developer tab.
pub(super) fn developer(app: &mut SheetApp, ui: &mut Ui) {
    // Code.
    stub_big(ui, Icon::Script, "Visual Basic", IN_DEVELOPMENT, false);
    let macros = big_button(ui, Icon::Macro, "Macros", "Macros and scripts in this workbook (Alt+F8)", false);
    control_keytip(app, ui, &macros, "view.macros");
    if macros.clicked() {
        act(app, "view.macros", json!({}));
    }
    ui.vertical(|ui| {
        stub(ui, Icon::Record, "Record Macro", IN_DEVELOPMENT);
        stub(ui, Icon::Relative, "Use Relative References", IN_DEVELOPMENT);
        stub(ui, Icon::Lock, "Macro Security", IN_DEVELOPMENT);
    });
    sep(ui);
    // Add-ins.
    stub_big(ui, Icon::Settings, "Add-ins", IN_DEVELOPMENT, false);
    stub_big(ui, Icon::Plus, "Excel Add-ins", IN_DEVELOPMENT, false);
    stub_big(ui, Icon::Link, "COM Add-ins", IN_DEVELOPMENT, false);
    sep(ui);
    // Controls: Insert is a menu in Excel, hence the arrow.
    stub_big(ui, Icon::Insert, "Insert", IN_DEVELOPMENT, true);
    stub_big(ui, Icon::Settings, "Design Mode", IN_DEVELOPMENT, false);
    ui.vertical(|ui| {
        stub(ui, Icon::Format, "Properties", IN_DEVELOPMENT);
        stub(ui, Icon::Script, "View Code", IN_DEVELOPMENT);
        stub(ui, Icon::Play, "Run Dialog", IN_DEVELOPMENT);
    });
    sep(ui);
    // XML: Source, then Map Properties / Expansion Packs / Refresh Data beside Import / Export.
    stub_big(ui, Icon::Script, "Source", IN_DEVELOPMENT, false);
    ui.vertical(|ui| {
        stub(ui, Icon::Table, "Map Properties", IN_DEVELOPMENT);
        stub(ui, Icon::Plus, "Expansion Packs", IN_DEVELOPMENT);
        stub(ui, Icon::Redo, "Refresh Data", IN_DEVELOPMENT);
    });
    ui.vertical(|ui| {
        stub(ui, Icon::OpenFolder, "Import", IN_DEVELOPMENT);
        stub(ui, Icon::Share, "Export", IN_DEVELOPMENT);
    });
}

/// A disabled ribbon entry that never runs a command: painted dimmed, and its reason shows on
/// hover. The accessible node is disabled, so assistive technology and tests see a stub.
fn stub(ui: &mut Ui, icon: Icon, label: &str, tip: &str) {
    let lang = crate::i18n::current(ui.ctx());
    let drawn = ui
        .add_enabled_ui(false, |ui| {
            ui.set_opacity(0.45);
            small_button(ui, icon, label, tip, false)
        })
        .inner;
    // The enabled tooltip `small_button` installs never shows while disabled; this one does.
    drawn.on_disabled_hover_text(lang.tr(tip));
}

/// A dimmed large button for a stub whose Excel counterpart is a big button.
fn stub_big(ui: &mut Ui, icon: Icon, label: &str, tip: &str, dropdown: bool) {
    let lang = crate::i18n::current(ui.ctx());
    let drawn = ui
        .add_enabled_ui(false, |ui| {
            ui.set_opacity(0.45);
            big_button(ui, icon, label, tip, dropdown)
        })
        .inner;
    drawn.on_disabled_hover_text(lang.tr(tip));
}
