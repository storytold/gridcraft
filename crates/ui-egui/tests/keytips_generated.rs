//! GENERATED from keytips.json by `cargo xtask keytips`. Edit the JSON, not this file.
//!
//! One end-to-end test per keytip sequence, through the real egui input path.
//! Assertions are generated from a probe of the live engine catalog (blank
//! workbook, empty params), so each test proves exactly what its command does
//! today: journaled, ran clean, surfaced a missing-params error, opened a
//! dialog, switched tabs, waited for more input, or stayed silent (pending).

use egui::os::OperatingSystem;
use egui::{Event, Key, Modifiers};
use gridcraft_engine::Session;
use gridcraft_ui_egui::SheetApp;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(session: Session) -> Harness {
    let mut h = egui_kittest::Harness::builder().with_os(OperatingSystem::Nix).with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h
}

fn blank() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s
}

fn key_event(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

fn events(h: &mut Harness, events: impl IntoIterator<Item = Event>) {
    h.input_mut().events.extend(events);
    h.run_steps(2);
}

fn key(h: &mut Harness, key: Key) {
    events(h, [key_event(key, true, Modifiers::NONE), key_event(key, false, Modifiers::NONE)]);
}

fn letter(h: &mut Harness, key: Key, text_first: bool) {
    let text = Event::Text(key.name().to_ascii_lowercase());
    let pressed = key_event(key, true, Modifiers::NONE);
    let pair = if text_first { [text, pressed] } else { [pressed, text] };
    events(h, pair.into_iter().chain([key_event(key, false, Modifiers::NONE)]));
}

fn sequence(h: &mut Harness, seq: &str, text_first: bool) {
    key(h, Key::F10);
    for ch in seq.chars() {
        let k = Key::from_name(&ch.to_string()).expect("sequence letters map to keys");
        letter(h, k, text_first);
    }
}

#[test]
fn keytip_h_() {
    let mut h = harness(blank());
    sequence(&mut h, "H", false);
    assert_eq!(h.state().ui.ribbon_tab, "Home", "H must switch to the Home tab");
}

#[test]
fn keytip_n_() {
    let mut h = harness(blank());
    sequence(&mut h, "N", false);
    assert_eq!(h.state().ui.ribbon_tab, "Insert", "N must switch to the Insert tab");
}

#[test]
fn keytip_ji_() {
    let mut h = harness(blank());
    sequence(&mut h, "JI", false);
    assert_eq!(h.state().ui.ribbon_tab, "Draw", "JI must switch to the Draw tab");
}

#[test]
fn keytip_p_() {
    let mut h = harness(blank());
    sequence(&mut h, "P", false);
    assert_eq!(h.state().ui.ribbon_tab, "Page Layout", "P must switch to the Page Layout tab");
}

#[test]
fn keytip_m_() {
    let mut h = harness(blank());
    sequence(&mut h, "M", false);
    assert_eq!(h.state().ui.ribbon_tab, "Formulas", "M must switch to the Formulas tab");
}

#[test]
fn keytip_a_() {
    let mut h = harness(blank());
    sequence(&mut h, "A", false);
    assert_eq!(h.state().ui.ribbon_tab, "Data", "A must switch to the Data tab");
}

#[test]
fn keytip_r_() {
    let mut h = harness(blank());
    sequence(&mut h, "R", false);
    assert_eq!(h.state().ui.ribbon_tab, "Review", "R must switch to the Review tab");
}

#[test]
fn keytip_w_() {
    let mut h = harness(blank());
    sequence(&mut h, "W", false);
    assert_eq!(h.state().ui.ribbon_tab, "View", "W must switch to the View tab");
}

#[test]
fn keytip_e_() {
    let mut h = harness(blank());
    sequence(&mut h, "E", false);
    assert_eq!(h.state().ui.ribbon_tab, "Home", "E must switch to the Home tab");
}

#[test]
fn keytip_o_() {
    let mut h = harness(blank());
    sequence(&mut h, "O", false);
    assert_eq!(h.state().ui.ribbon_tab, "Home", "O must switch to the Home tab");
}

#[test]
fn keytip_har_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.alignRight"), "HAR must journal home.alignRight");
}

#[test]
fn keytip_hat_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.alignTop"), "HAT must journal home.alignTop");
}

#[test]
fn keytip_ham_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAM", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.alignMiddle"), "HAM must journal home.alignMiddle");
}

#[test]
fn keytip_hab_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAB", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.alignBottom"), "HAB must journal home.alignBottom");
}

#[test]
fn keytip_hal_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAL", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.alignLeft"), "HAL must journal home.alignLeft");
}

#[test]
fn keytip_hac_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.alignCenter"), "HAC must journal home.alignCenter");
}

#[test]
fn keytip_hvs_() {
    let mut h = harness(blank());
    sequence(&mut h, "HVS", false);
    assert!(h.state().dialog.is_some(), "HVS must open the pasteSpecial dialog");
}

#[test]
fn keytip_es_() {
    let mut h = harness(blank());
    sequence(&mut h, "ES", false);
    assert!(h.state().dialog.is_some(), "ES must open the pasteSpecial dialog");
}

#[test]
fn keytip_hoi_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.autofitColumnWidth"), "HOI must journal home.autofitColumnWidth");
}

#[test]
fn keytip_oca_() {
    let mut h = harness(blank());
    sequence(&mut h, "OCA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.autofitColumnWidth"), "OCA must journal home.autofitColumnWidth");
}

#[test]
fn keytip_wvg_() {
    let mut h = harness(blank());
    sequence(&mut h, "WVG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.gridlines"), "WVG must journal view.gridlines");
}

#[test]
fn keytip_h0_() {
    let mut h = harness(blank());
    sequence(&mut h, "H0", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.increaseDecimal"), "H0 must journal home.increaseDecimal");
}

#[test]
fn keytip_h1_() {
    let mut h = harness(blank());
    sequence(&mut h, "H1", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.bold"), "H1 must journal home.bold");
}

#[test]
fn keytip_h2_() {
    let mut h = harness(blank());
    sequence(&mut h, "H2", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.italic"), "H2 must journal home.italic");
}

#[test]
fn keytip_h3_() {
    let mut h = harness(blank());
    sequence(&mut h, "H3", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.underline"), "H3 must journal home.underline");
}

#[test]
fn keytip_h5_() {
    let mut h = harness(blank());
    sequence(&mut h, "H5", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.decreaseIndent"), "H5 must journal home.decreaseIndent");
}

#[test]
fn keytip_h6_() {
    let mut h = harness(blank());
    sequence(&mut h, "H6", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.increaseIndent"), "H6 must journal home.increaseIndent");
}

#[test]
fn keytip_h9_() {
    let mut h = harness(blank());
    sequence(&mut h, "H9", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.decreaseDecimal"), "H9 must journal home.decreaseDecimal");
}

#[test]
fn keytip_han_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAN", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.accounting"), "HAN must journal home.accounting");
}

#[test]
fn keytip_has_() {
    let mut h = harness(blank());
    sequence(&mut h, "HAS", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HAS must run home.addIns cleanly");
}

#[test]
fn keytip_hb_() {
    let mut h = harness(blank());
    sequence(&mut h, "HB", false);
    assert_eq!(h.state().keytips.prefix(), Some("HB"), "HB must stay armed waiting for more input");
}

#[test]
fn keytip_hba_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBA must journal home.borders");
}

#[test]
fn keytip_hbb_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBB", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBB must journal home.borders");
}

#[test]
fn keytip_hbc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBC must journal home.borders");
}

#[test]
fn keytip_hbd_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBD", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBD must journal home.borders");
}

#[test]
fn keytip_hbe_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBE", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBE must journal home.borders");
}

#[test]
fn keytip_hbg_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.drawBorder"), "HBG must journal home.drawBorder");
}

#[test]
fn keytip_hbh_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBH must journal home.borders");
}

#[test]
fn keytip_hbi_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBI must journal home.borders");
}

#[test]
fn keytip_hbl_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBL", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBL must journal home.borders");
}

#[test]
fn keytip_hbm_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBM", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBM must journal home.borders");
}

#[test]
fn keytip_hbn_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBN", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBN must journal home.borders");
}

#[test]
fn keytip_hbo_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBO", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBO must journal home.borders");
}

#[test]
fn keytip_hbp_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBP", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBP must journal home.borders");
}

#[test]
fn keytip_hbr_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBR must journal home.borders");
}

#[test]
fn keytip_hbs_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBS must journal home.borders");
}

#[test]
fn keytip_hbt_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBT must journal home.borders");
}

#[test]
fn keytip_hbu_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBU", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBU must journal home.borders");
}

#[test]
fn keytip_hbw_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBW", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.drawBorder"), "HBW must journal home.drawBorder");
}

#[test]
fn keytip_hby_() {
    let mut h = harness(blank());
    sequence(&mut h, "HBY", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.borders"), "HBY must journal home.borders");
}

#[test]
fn keytip_hc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.copy"), "HC must journal edit.copy");
}

#[test]
fn keytip_hdc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HDC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.deleteColumns"), "HDC must journal home.deleteColumns");
}

#[test]
fn keytip_hdd_() {
    let mut h = harness(blank());
    sequence(&mut h, "HDD", false);
    assert!(h.state().dialog.is_some(), "HDD must open the deleteCells dialog");
}

#[test]
fn keytip_hdr_() {
    let mut h = harness(blank());
    sequence(&mut h, "HDR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.deleteRows"), "HDR must journal home.deleteRows");
}

#[test]
fn keytip_hds_() {
    let mut h = harness(blank());
    sequence(&mut h, "HDS", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HDS must invoke home.deleteSheet and surface its missing-params error");
}

#[test]
fn keytip_hea_() {
    let mut h = harness(blank());
    sequence(&mut h, "HEA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.clearAll"), "HEA must journal edit.clearAll");
}

#[test]
fn keytip_hec_() {
    let mut h = harness(blank());
    sequence(&mut h, "HEC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.clearContents"), "HEC must journal edit.clearContents");
}

#[test]
fn keytip_hef_() {
    let mut h = harness(blank());
    sequence(&mut h, "HEF", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.clearFormats"), "HEF must journal edit.clearFormats");
}

#[test]
fn keytip_hel_() {
    let mut h = harness(blank());
    sequence(&mut h, "HEL", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.clearHyperlinks"), "HEL must journal edit.clearHyperlinks");
}

#[test]
fn keytip_hem_() {
    let mut h = harness(blank());
    sequence(&mut h, "HEM", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.clearComments"), "HEM must journal edit.clearComments");
}

#[test]
fn keytip_hfc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFC", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HFC must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_hfdc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.conditionalFormat"), "HFDC must journal home.conditionalFormat");
}

#[test]
fn keytip_hfdf_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDF", false);
    assert!(h.state().dialog.is_some(), "HFDF must open the find dialog");
}

#[test]
fn keytip_hfdg_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDG", false);
    assert!(h.state().dialog.is_some(), "HFDG must open the goTo dialog");
}

#[test]
fn keytip_hfdm_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDM", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HFDM must invoke edit.find and surface its missing-params error");
}

#[test]
fn keytip_hfdo_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDO", false);
    assert!(h.state().dialog.is_none(), "HFDO must not open a dialog");
    assert!(h.state().message.is_none(), "HFDO must not open a message box");
    assert!(h.state().toast.is_none(), "HFDO must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "HFDO must not journal");
}

#[test]
fn keytip_hfdp_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDP", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HFDP must run arrange.selectionPane cleanly");
}

#[test]
fn keytip_hfdr_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDR", false);
    assert!(h.state().dialog.is_some(), "HFDR must open the find dialog");
}

#[test]
fn keytip_hfds_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFDS", false);
    assert!(h.state().dialog.is_some(), "HFDS must open the goToSpecial dialog");
}

#[test]
fn keytip_hff_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFF", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HFF must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_hfg_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.increaseFontSize"), "HFG must journal home.increaseFontSize");
}

#[test]
fn keytip_hfi_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFI", false);
    assert_eq!(h.state().keytips.prefix(), Some("HFI"), "HFI must stay armed waiting for more input");
}

#[test]
fn keytip_hfia_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFIA", false);
    assert!(h.state().dialog.is_none(), "HFIA must not open a dialog");
    assert!(h.state().message.is_none(), "HFIA must not open a message box");
    assert!(h.state().toast.is_none(), "HFIA must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "HFIA must not journal");
}

#[test]
fn keytip_hfid_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFID", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.fillDown"), "HFID must journal edit.fillDown");
}

#[test]
fn keytip_hfif_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFIF", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HFIF must invoke edit.flashFill and surface its missing-params error");
}

#[test]
fn keytip_hfij_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFIJ", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.justify"), "HFIJ must journal home.justify");
}

#[test]
fn keytip_hfil_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFIL", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.fillLeft"), "HFIL must journal edit.fillLeft");
}

#[test]
fn keytip_hfir_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFIR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.fillRight"), "HFIR must journal edit.fillRight");
}

#[test]
fn keytip_hfis_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFIS", false);
    assert!(h.state().dialog.is_some(), "HFIS must open the series dialog");
}

#[test]
fn keytip_hfiu_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFIU", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.fillUp"), "HFIU must journal edit.fillUp");
}

#[test]
fn keytip_hfk_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFK", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.decreaseFontSize"), "HFK must journal home.decreaseFontSize");
}

#[test]
fn keytip_hfm_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFM", false);
    assert!(h.state().dialog.is_some(), "HFM must open the formatCells dialog");
}

#[test]
fn keytip_hfp_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFP", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.formatPainter"), "HFP must journal edit.formatPainter");
}

#[test]
fn keytip_hfq_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFQ", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HFQ must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_hfs_() {
    let mut h = harness(blank());
    sequence(&mut h, "HFS", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HFS must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_hh_() {
    let mut h = harness(blank());
    sequence(&mut h, "HH", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HH must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_hic_() {
    let mut h = harness(blank());
    sequence(&mut h, "HIC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.insertColumns"), "HIC must journal home.insertColumns");
}

#[test]
fn keytip_hie_() {
    let mut h = harness(blank());
    sequence(&mut h, "HIE", false);
    assert!(h.state().dialog.is_some(), "HIE must open the insertCells dialog");
}

#[test]
fn keytip_hii_() {
    let mut h = harness(blank());
    sequence(&mut h, "HII", false);
    assert!(h.state().dialog.is_some(), "HII must open the insertCells dialog");
}

#[test]
fn keytip_hir_() {
    let mut h = harness(blank());
    sequence(&mut h, "HIR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.insertRows"), "HIR must journal home.insertRows");
}

#[test]
fn keytip_his_() {
    let mut h = harness(blank());
    sequence(&mut h, "HIS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.insertSheet"), "HIS must journal home.insertSheet");
}

#[test]
fn keytip_hj_() {
    let mut h = harness(blank());
    sequence(&mut h, "HJ", false);
    assert_eq!(h.state().keytips.prefix(), Some("HJ"), "HJ must stay armed waiting for more input");
}

#[test]
fn keytip_hjn_() {
    let mut h = harness(blank());
    sequence(&mut h, "HJN", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HJN must invoke home.newCellStyle and surface its missing-params error");
}

#[test]
fn keytip_hk_() {
    let mut h = harness(blank());
    sequence(&mut h, "HK", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.comma"), "HK must journal home.comma");
}

#[test]
fn keytip_hl_() {
    let mut h = harness(blank());
    sequence(&mut h, "HL", false);
    assert_eq!(h.state().keytips.prefix(), Some("HL"), "HL must stay armed waiting for more input");
}

#[test]
fn keytip_hlc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLC", false);
    assert_eq!(h.state().keytips.prefix(), Some("HLC"), "HLC must stay armed waiting for more input");
}

#[test]
fn keytip_hlce_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLCE", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.clearRules"), "HLCE must journal home.clearRules");
}

#[test]
fn keytip_hlcs_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLCS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.clearRules"), "HLCS must journal home.clearRules");
}

#[test]
fn keytip_hlct_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLCT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.clearRules"), "HLCT must journal home.clearRules");
}

#[test]
fn keytip_hld_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLD", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.conditionalFormat"), "HLD must journal home.conditionalFormat");
}

#[test]
fn keytip_hlh_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.conditionalFormat"), "HLH must journal home.conditionalFormat");
}

#[test]
fn keytip_hln_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLN", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.conditionalFormat"), "HLN must journal home.conditionalFormat");
}

#[test]
fn keytip_hlr_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.manageRules"), "HLR must journal home.manageRules");
}

#[test]
fn keytip_hlt_() {
    let mut h = harness(blank());
    sequence(&mut h, "HLT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.conditionalFormat"), "HLT must journal home.conditionalFormat");
}

#[test]
fn keytip_hm_() {
    let mut h = harness(blank());
    sequence(&mut h, "HM", false);
    assert_eq!(h.state().keytips.prefix(), Some("HM"), "HM must stay armed waiting for more input");
}

#[test]
fn keytip_hma_() {
    let mut h = harness(blank());
    sequence(&mut h, "HMA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.mergeAcross"), "HMA must journal home.mergeAcross");
}

#[test]
fn keytip_hmc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HMC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.mergeCenter"), "HMC must journal home.mergeCenter");
}

#[test]
fn keytip_hmm_() {
    let mut h = harness(blank());
    sequence(&mut h, "HMM", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.mergeCells"), "HMM must journal home.mergeCells");
}

#[test]
fn keytip_hmu_() {
    let mut h = harness(blank());
    sequence(&mut h, "HMU", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.unmergeCells"), "HMU must journal home.unmergeCells");
}

#[test]
fn keytip_hn_() {
    let mut h = harness(blank());
    sequence(&mut h, "HN", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "HN must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_hoa_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.autofitRowHeight"), "HOA must journal home.autofitRowHeight");
}

#[test]
fn keytip_hod_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOD", false);
    assert!(h.state().dialog.is_some(), "HOD must open the defaultWidth dialog");
}

#[test]
fn keytip_hoe_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOE", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.formatCells"), "HOE must journal home.formatCells");
}

#[test]
fn keytip_hoh_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOH", false);
    assert!(h.state().dialog.is_some(), "HOH must open the rowHeight dialog");
}

#[test]
fn keytip_hol_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOL", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.lockCell"), "HOL must journal home.lockCell");
}

#[test]
fn keytip_hom_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOM", false);
    assert!(h.state().dialog.is_some(), "HOM must open the moveSheet dialog");
}

#[test]
fn keytip_hop_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOP", false);
    assert!(h.state().dialog.is_some(), "HOP must open the protectSheet dialog");
}

#[test]
fn keytip_hor_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOR", false);
    assert!(h.state().dialog.is_some(), "HOR must open the renameSheet dialog");
}

#[test]
fn keytip_hot_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "sheet.tabColor"), "HOT must journal sheet.tabColor");
}

#[test]
fn keytip_houc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOUC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.hideColumns"), "HOUC must journal home.hideColumns");
}

#[test]
fn keytip_houh_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOUH", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HOUH must invoke sheet.unhide and surface its missing-params error");
}

#[test]
fn keytip_houl_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOUL", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.unhideColumns"), "HOUL must journal home.unhideColumns");
}

#[test]
fn keytip_houo_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOUO", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.unhideRows"), "HOUO must journal home.unhideRows");
}

#[test]
fn keytip_hour_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOUR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.hideRows"), "HOUR must journal home.hideRows");
}

#[test]
fn keytip_hous_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOUS", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HOUS must invoke sheet.hide and surface its missing-params error");
}

#[test]
fn keytip_how_() {
    let mut h = harness(blank());
    sequence(&mut h, "HOW", false);
    assert!(h.state().dialog.is_some(), "HOW must open the columnWidth dialog");
}

#[test]
fn keytip_hp_() {
    let mut h = harness(blank());
    sequence(&mut h, "HP", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.percent"), "HP must journal home.percent");
}

#[test]
fn keytip_hsc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HSC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.clearFilter"), "HSC must journal data.clearFilter");
}

#[test]
fn keytip_hsf_() {
    let mut h = harness(blank());
    sequence(&mut h, "HSF", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.filter"), "HSF must journal data.filter");
}

#[test]
fn keytip_hso_() {
    let mut h = harness(blank());
    sequence(&mut h, "HSO", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.sortDescending"), "HSO must journal data.sortDescending");
}

#[test]
fn keytip_hss_() {
    let mut h = harness(blank());
    sequence(&mut h, "HSS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.sortAscending"), "HSS must journal data.sortAscending");
}

#[test]
fn keytip_hsu_() {
    let mut h = harness(blank());
    sequence(&mut h, "HSU", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.sort"), "HSU must journal data.sort");
}

#[test]
fn keytip_ht_() {
    let mut h = harness(blank());
    sequence(&mut h, "HT", false);
    assert_eq!(h.state().keytips.prefix(), Some("HT"), "HT must stay armed waiting for more input");
}

#[test]
fn keytip_htn_() {
    let mut h = harness(blank());
    sequence(&mut h, "HTN", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HTN must invoke table.style and surface its missing-params error");
}

#[test]
fn keytip_htp_() {
    let mut h = harness(blank());
    sequence(&mut h, "HTP", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HTP must invoke table.style and surface its missing-params error");
}

#[test]
fn keytip_hu_() {
    let mut h = harness(blank());
    sequence(&mut h, "HU", false);
    assert_eq!(h.state().keytips.prefix(), Some("HU"), "HU must stay armed waiting for more input");
}

#[test]
fn keytip_hua_() {
    let mut h = harness(blank());
    sequence(&mut h, "HUA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "HUA must journal formulas.autoSum");
}

#[test]
fn keytip_huc_() {
    let mut h = harness(blank());
    sequence(&mut h, "HUC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "HUC must journal formulas.autoSum");
}

#[test]
fn keytip_hui_() {
    let mut h = harness(blank());
    sequence(&mut h, "HUI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "HUI must journal formulas.autoSum");
}

#[test]
fn keytip_hum_() {
    let mut h = harness(blank());
    sequence(&mut h, "HUM", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "HUM must journal formulas.autoSum");
}

#[test]
fn keytip_hv_() {
    let mut h = harness(blank());
    sequence(&mut h, "HV", false);
    assert_eq!(h.state().keytips.prefix(), Some("HV"), "HV must stay armed waiting for more input");
}

#[test]
fn keytip_hvp_() {
    let mut h = harness(blank());
    sequence(&mut h, "HVP", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "HVP must invoke edit.paste and surface its missing-params error");
}

#[test]
fn keytip_hw_() {
    let mut h = harness(blank());
    sequence(&mut h, "HW", false);
    assert_eq!(h.state().keytips.prefix(), Some("HW"), "HW must stay armed waiting for more input");
}

#[test]
fn keytip_hww_() {
    let mut h = harness(blank());
    sequence(&mut h, "HWW", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "home.wrapText"), "HWW must journal home.wrapText");
}

#[test]
fn keytip_hx_() {
    let mut h = harness(blank());
    sequence(&mut h, "HX", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "edit.cut"), "HX must journal edit.cut");
}

#[test]
fn keytip_nc1_() {
    let mut h = harness(blank());
    sequence(&mut h, "NC1", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "NC1 must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_nc2_() {
    let mut h = harness(blank());
    sequence(&mut h, "NC2", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "review.newComment"), "NC2 must journal review.newComment");
}

#[test]
fn keytip_ncm_() {
    let mut h = harness(blank());
    sequence(&mut h, "NCM", false);
    assert!(h.state().dialog.is_some(), "NCM must open the comment dialog");
}

#[test]
fn keytip_ncb_() {
    let mut h = harness(blank());
    sequence(&mut h, "NCB", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.checkbox"), "NCB must journal insert.checkbox");
}

#[test]
fn keytip_nd_() {
    let mut h = harness(blank());
    sequence(&mut h, "ND", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "ND must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_ne_() {
    let mut h = harness(blank());
    sequence(&mut h, "NE", false);
    assert!(h.state().dialog.is_none(), "NE must not open a dialog");
    assert!(h.state().message.is_none(), "NE must not open a message box");
    assert!(h.state().toast.is_none(), "NE must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "NE must not journal");
}

#[test]
fn keytip_nf_() {
    let mut h = harness(blank());
    sequence(&mut h, "NF", false);
    assert_eq!(h.state().keytips.prefix(), Some("NF"), "NF must stay armed waiting for more input");
}

#[test]
fn keytip_nfp_() {
    let mut h = harness(blank());
    sequence(&mut h, "NFP", false);
    assert_eq!(h.state().keytips.prefix(), Some("NFP"), "NFP must stay armed waiting for more input");
}

#[test]
fn keytip_nfpf_() {
    let mut h = harness(blank());
    sequence(&mut h, "NFPF", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "NFPF must journal insert.picture");
}

#[test]
fn keytip_nh1_() {
    let mut h = harness(blank());
    sequence(&mut h, "NH1", false);
    assert!(h.state().dialog.is_some(), "NH1 must open the headerFooter dialog");
}

#[test]
fn keytip_nhf_() {
    let mut h = harness(blank());
    sequence(&mut h, "NHF", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.headerFooter"), "NHF must journal pageLayout.headerFooter");
}

#[test]
fn keytip_nhi_() {
    let mut h = harness(blank());
    sequence(&mut h, "NHI", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "NHI must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_ni1_() {
    let mut h = harness(blank());
    sequence(&mut h, "NI1", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.chart"), "NI1 must journal insert.chart");
}

#[test]
fn keytip_ni2_() {
    let mut h = harness(blank());
    sequence(&mut h, "NI2", false);
    assert!(h.state().dialog.is_some(), "NI2 must open the insertLink dialog");
}

#[test]
fn keytip_nj_() {
    let mut h = harness(blank());
    sequence(&mut h, "NJ", false);
    assert!(h.state().dialog.is_none(), "NJ must not open a dialog");
    assert!(h.state().message.is_none(), "NJ must not open a message box");
    assert!(h.state().toast.is_none(), "NJ must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "NJ must not journal");
}

#[test]
fn keytip_nm1t_() {
    let mut h = harness(blank());
    sequence(&mut h, "NM1T", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "NM1T must journal insert.picture");
}

#[test]
fn keytip_nm2_() {
    let mut h = harness(blank());
    sequence(&mut h, "NM2", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.chart"), "NM2 must journal insert.chart");
}

#[test]
fn keytip_nn1_() {
    let mut h = harness(blank());
    sequence(&mut h, "NN1", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "NN1 must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_nns_() {
    let mut h = harness(blank());
    sequence(&mut h, "NNS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.icons"), "NNS must journal insert.icons");
}

#[test]
fn keytip_np_() {
    let mut h = harness(blank());
    sequence(&mut h, "NP", false);
    assert_eq!(h.state().keytips.prefix(), Some("NP"), "NP must stay armed waiting for more input");
}

#[test]
fn keytip_npb_() {
    let mut h = harness(blank());
    sequence(&mut h, "NPB", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "NPB must journal insert.picture");
}

#[test]
fn keytip_npf_() {
    let mut h = harness(blank());
    sequence(&mut h, "NPF", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "NPF must journal insert.picture");
}

#[test]
fn keytip_npi_() {
    let mut h = harness(blank());
    sequence(&mut h, "NPI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "NPI must journal insert.picture");
}

#[test]
fn keytip_npo_() {
    let mut h = harness(blank());
    sequence(&mut h, "NPO", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "NPO must journal insert.picture");
}

#[test]
fn keytip_nq_() {
    let mut h = harness(blank());
    sequence(&mut h, "NQ", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "NQ must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_nr_() {
    let mut h = harness(blank());
    sequence(&mut h, "NR", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "NR must invoke insert.recommendedCharts and surface its missing-params error");
}

#[test]
fn keytip_nsa_() {
    let mut h = harness(blank());
    sequence(&mut h, "NSA", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "NSA must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_nsc_() {
    let mut h = harness(blank());
    sequence(&mut h, "NSC", false);
    assert!(h.state().dialog.is_none(), "NSC must not open a dialog");
    assert!(h.state().message.is_none(), "NSC must not open a message box");
    assert!(h.state().toast.is_none(), "NSC must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "NSC must not journal");
}

#[test]
fn keytip_nsd_() {
    let mut h = harness(blank());
    sequence(&mut h, "NSD", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.chart"), "NSD must journal insert.chart");
}

#[test]
fn keytip_nsf_() {
    let mut h = harness(blank());
    sequence(&mut h, "NSF", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "NSF must invoke insert.slicer and surface its missing-params error");
}

#[test]
fn keytip_nsh_() {
    let mut h = harness(blank());
    sequence(&mut h, "NSH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.shape"), "NSH must journal insert.shape");
}

#[test]
fn keytip_nsp_() {
    let mut h = harness(blank());
    sequence(&mut h, "NSP", false);
    assert!(
        h.state().message.is_some() || h.state().toast.is_some(),
        "NSP must invoke insert.recommendedPivotTables and surface its missing-params error"
    );
}

#[test]
fn keytip_nst_() {
    let mut h = harness(blank());
    sequence(&mut h, "NST", false);
    assert!(h.state().dialog.is_none(), "NST must not open a dialog");
    assert!(h.state().message.is_none(), "NST must not open a message box");
    assert!(h.state().toast.is_none(), "NST must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "NST must not journal");
}

#[test]
fn keytip_nsz_() {
    let mut h = harness(blank());
    sequence(&mut h, "NSZ", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "NSZ must invoke insert.pivotChart and surface its missing-params error");
}

#[test]
fn keytip_nt_() {
    let mut h = harness(blank());
    sequence(&mut h, "NT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.table"), "NT must journal insert.table");
}

#[test]
fn keytip_nu_() {
    let mut h = harness(blank());
    sequence(&mut h, "NU", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.symbol"), "NU must journal insert.symbol");
}

#[test]
fn keytip_nzs_() {
    let mut h = harness(blank());
    sequence(&mut h, "NZS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.symbol"), "NZS must journal insert.symbol");
}

#[test]
fn keytip_nv_() {
    let mut h = harness(blank());
    sequence(&mut h, "NV", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "NV must invoke insert.pivotTable and surface its missing-params error");
}

#[test]
fn keytip_nw_() {
    let mut h = harness(blank());
    sequence(&mut h, "NW", false);
    assert!(h.state().dialog.is_none(), "NW must not open a dialog");
    assert!(h.state().message.is_none(), "NW must not open a message box");
    assert!(h.state().toast.is_none(), "NW must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "NW must not journal");
}

#[test]
fn keytip_nx_() {
    let mut h = harness(blank());
    sequence(&mut h, "NX", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.textBox"), "NX must journal insert.textBox");
}

#[test]
fn keytip_nzk_() {
    let mut h = harness(blank());
    sequence(&mut h, "NZK", false);
    assert_eq!(h.state().keytips.prefix(), Some("NZK"), "NZK must stay armed waiting for more input");
}

#[test]
fn keytip_nzksl_() {
    let mut h = harness(blank());
    sequence(&mut h, "NZKSL", false);
    assert!(h.state().dialog.is_some(), "NZKSL must open the sparkline dialog");
}

#[test]
fn keytip_nzkso_() {
    let mut h = harness(blank());
    sequence(&mut h, "NZKSO", false);
    assert!(h.state().dialog.is_some(), "NZKSO must open the sparkline dialog");
}

#[test]
fn keytip_nzksw_() {
    let mut h = harness(blank());
    sequence(&mut h, "NZKSW", false);
    assert!(h.state().dialog.is_some(), "NZKSW must open the sparkline dialog");
}

#[test]
fn keytip_nztx_() {
    let mut h = harness(blank());
    sequence(&mut h, "NZTX", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.textBox"), "NZTX must journal insert.textBox");
}

#[test]
fn keytip_nzt_() {
    let mut h = harness(blank());
    sequence(&mut h, "NZT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.textBox"), "NZT must journal insert.textBox");
}

#[test]
fn keytip_pae_() {
    let mut h = harness(blank());
    sequence(&mut h, "PAE", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "PAE must invoke arrange.sendBackward and surface its missing-params error");
}

#[test]
fn keytip_paf_() {
    let mut h = harness(blank());
    sequence(&mut h, "PAF", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "PAF must invoke arrange.bringForward and surface its missing-params error");
}

#[test]
fn keytip_pb_() {
    let mut h = harness(blank());
    sequence(&mut h, "PB", false);
    assert_eq!(h.state().keytips.prefix(), Some("PB"), "PB must stay armed waiting for more input");
}

#[test]
fn keytip_pbi_() {
    let mut h = harness(blank());
    sequence(&mut h, "PBI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.breaks"), "PBI must journal pageLayout.breaks");
}

#[test]
fn keytip_pbr_() {
    let mut h = harness(blank());
    sequence(&mut h, "PBR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.breaks"), "PBR must journal pageLayout.breaks");
}

#[test]
fn keytip_pgb_() {
    let mut h = harness(blank());
    sequence(&mut h, "PGB", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.background"), "PGB must journal pageLayout.background");
}

#[test]
fn keytip_pi_() {
    let mut h = harness(blank());
    sequence(&mut h, "PI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.printTitles"), "PI must journal pageLayout.printTitles");
}

#[test]
fn keytip_pm_() {
    let mut h = harness(blank());
    sequence(&mut h, "PM", false);
    assert_eq!(h.state().keytips.prefix(), Some("PM"), "PM must stay armed waiting for more input");
}

#[test]
fn keytip_pma_() {
    let mut h = harness(blank());
    sequence(&mut h, "PMA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.margins"), "PMA must journal pageLayout.margins");
}

#[test]
fn keytip_po_() {
    let mut h = harness(blank());
    sequence(&mut h, "PO", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.orientation"), "PO must journal pageLayout.orientation");
}

#[test]
fn keytip_ppg_() {
    let mut h = harness(blank());
    sequence(&mut h, "PPG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.printGridlines"), "PPG must journal pageLayout.printGridlines");
}

#[test]
fn keytip_pph_() {
    let mut h = harness(blank());
    sequence(&mut h, "PPH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.printHeadings"), "PPH must journal pageLayout.printHeadings");
}

#[test]
fn keytip_pr_() {
    let mut h = harness(blank());
    sequence(&mut h, "PR", false);
    assert_eq!(h.state().keytips.prefix(), Some("PR"), "PR must stay armed waiting for more input");
}

#[test]
fn keytip_prs_() {
    let mut h = harness(blank());
    sequence(&mut h, "PRS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.printArea"), "PRS must journal pageLayout.printArea");
}

#[test]
fn keytip_psp_() {
    let mut h = harness(blank());
    sequence(&mut h, "PSP", false);
    assert!(h.state().dialog.is_some(), "PSP must open the pageSetup dialog");
}

#[test]
fn keytip_psz_() {
    let mut h = harness(blank());
    sequence(&mut h, "PSZ", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.size"), "PSZ must journal pageLayout.size");
}

#[test]
fn keytip_ptc_() {
    let mut h = harness(blank());
    sequence(&mut h, "PTC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.themeColors"), "PTC must journal pageLayout.themeColors");
}

#[test]
fn keytip_pte_() {
    let mut h = harness(blank());
    sequence(&mut h, "PTE", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.themeEffects"), "PTE must journal pageLayout.themeEffects");
}

#[test]
fn keytip_ptf_() {
    let mut h = harness(blank());
    sequence(&mut h, "PTF", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.themeFonts"), "PTF must journal pageLayout.themeFonts");
}

#[test]
fn keytip_pth_() {
    let mut h = harness(blank());
    sequence(&mut h, "PTH", false);
    assert_eq!(h.state().keytips.prefix(), Some("PTH"), "PTH must stay armed waiting for more input");
}

#[test]
fn keytip_pthb_() {
    let mut h = harness(blank());
    sequence(&mut h, "PTHB", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "pageLayout.theme"), "PTHB must journal pageLayout.theme");
}

#[test]
fn keytip_pvg_() {
    let mut h = harness(blank());
    sequence(&mut h, "PVG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.gridlines"), "PVG must journal view.gridlines");
}

#[test]
fn keytip_pvh_() {
    let mut h = harness(blank());
    sequence(&mut h, "PVH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.headings"), "PVH must journal view.headings");
}

#[test]
fn keytip_ma_() {
    let mut h = harness(blank());
    sequence(&mut h, "MA", false);
    assert_eq!(h.state().keytips.prefix(), Some("MA"), "MA must stay armed waiting for more input");
}

#[test]
fn keytip_maa_() {
    let mut h = harness(blank());
    sequence(&mut h, "MAA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.removeArrows"), "MAA must journal formulas.removeArrows");
}

#[test]
fn keytip_map_() {
    let mut h = harness(blank());
    sequence(&mut h, "MAP", false);
    assert!(h.state().dialog.is_none(), "MAP must not open a dialog");
    assert!(h.state().message.is_none(), "MAP must not open a message box");
    assert!(h.state().toast.is_none(), "MAP must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "MAP must not journal");
}

#[test]
fn keytip_mb_() {
    let mut h = harness(blank());
    sequence(&mut h, "MB", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.calculateNow"), "MB must journal formulas.calculateNow");
}

#[test]
fn keytip_mc_() {
    let mut h = harness(blank());
    sequence(&mut h, "MC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.createFromSelection"), "MC must journal formulas.createFromSelection");
}

#[test]
fn keytip_md_() {
    let mut h = harness(blank());
    sequence(&mut h, "MD", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MD must run formulas.traceDependents cleanly");
}

#[test]
fn keytip_me_() {
    let mut h = harness(blank());
    sequence(&mut h, "ME", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "ME must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_mf_() {
    let mut h = harness(blank());
    sequence(&mut h, "MF", false);
    assert!(h.state().dialog.is_some(), "MF must open the insertFunction dialog");
}

#[test]
fn keytip_mg_() {
    let mut h = harness(blank());
    sequence(&mut h, "MG", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MG must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_mh_() {
    let mut h = harness(blank());
    sequence(&mut h, "MH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.showFormulas"), "MH must journal formulas.showFormulas");
}

#[test]
fn keytip_mi_() {
    let mut h = harness(blank());
    sequence(&mut h, "MI", false);
    assert_eq!(h.state().keytips.prefix(), Some("MI"), "MI must stay armed waiting for more input");
}

#[test]
fn keytip_mi1_() {
    let mut h = harness(blank());
    sequence(&mut h, "MI1", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MI1 must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_mj_() {
    let mut h = harness(blank());
    sequence(&mut h, "MJ", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.calculateSheet"), "MJ must journal formulas.calculateSheet");
}

#[test]
fn keytip_mk_() {
    let mut h = harness(blank());
    sequence(&mut h, "MK", false);
    assert_eq!(h.state().keytips.prefix(), Some("MK"), "MK must stay armed waiting for more input");
}

#[test]
fn keytip_mkk_() {
    let mut h = harness(blank());
    sequence(&mut h, "MKK", false);
    assert!(h.state().dialog.is_some(), "MKK must open the errorChecking dialog");
}

#[test]
fn keytip_ml_() {
    let mut h = harness(blank());
    sequence(&mut h, "ML", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "ML must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_mm_() {
    let mut h = harness(blank());
    sequence(&mut h, "MM", false);
    assert!(h.state().dialog.is_some(), "MM must open the defineName dialog");
}

#[test]
fn keytip_mn_() {
    let mut h = harness(blank());
    sequence(&mut h, "MN", false);
    assert!(h.state().dialog.is_some(), "MN must open the nameManager dialog");
}

#[test]
fn keytip_mo_() {
    let mut h = harness(blank());
    sequence(&mut h, "MO", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MO must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_mp_() {
    let mut h = harness(blank());
    sequence(&mut h, "MP", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MP must run formulas.tracePrecedents cleanly");
}

#[test]
fn keytip_mq_() {
    let mut h = harness(blank());
    sequence(&mut h, "MQ", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MQ must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_ms_() {
    let mut h = harness(blank());
    sequence(&mut h, "MS", false);
    assert_eq!(h.state().keytips.prefix(), Some("MS"), "MS must stay armed waiting for more input");
}

#[test]
fn keytip_msp_() {
    let mut h = harness(blank());
    sequence(&mut h, "MSP", false);
    assert!(h.state().dialog.is_none(), "MSP must not open a dialog");
    assert!(h.state().message.is_none(), "MSP must not open a message box");
    assert!(h.state().toast.is_none(), "MSP must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "MSP must not journal");
}

#[test]
fn keytip_mt_() {
    let mut h = harness(blank());
    sequence(&mut h, "MT", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MT must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_mu_() {
    let mut h = harness(blank());
    sequence(&mut h, "MU", false);
    assert_eq!(h.state().keytips.prefix(), Some("MU"), "MU must stay armed waiting for more input");
}

#[test]
fn keytip_mua_() {
    let mut h = harness(blank());
    sequence(&mut h, "MUA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "MUA must journal formulas.autoSum");
}

#[test]
fn keytip_muc_() {
    let mut h = harness(blank());
    sequence(&mut h, "MUC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "MUC must journal formulas.autoSum");
}

#[test]
fn keytip_muf_() {
    let mut h = harness(blank());
    sequence(&mut h, "MUF", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MUF must run formulas.moreFunctions cleanly");
}

#[test]
fn keytip_mui_() {
    let mut h = harness(blank());
    sequence(&mut h, "MUI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "MUI must journal formulas.autoSum");
}

#[test]
fn keytip_mum_() {
    let mut h = harness(blank());
    sequence(&mut h, "MUM", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "MUM must journal formulas.autoSum");
}

#[test]
fn keytip_mus_() {
    let mut h = harness(blank());
    sequence(&mut h, "MUS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.autoSum"), "MUS must journal formulas.autoSum");
}

#[test]
fn keytip_mv_() {
    let mut h = harness(blank());
    sequence(&mut h, "MV", false);
    assert!(h.state().dialog.is_some(), "MV must open the evaluateFormula dialog");
}

#[test]
fn keytip_mw_() {
    let mut h = harness(blank());
    sequence(&mut h, "MW", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "MW must run formulas.watchWindow cleanly");
}

#[test]
fn keytip_mx_() {
    let mut h = harness(blank());
    sequence(&mut h, "MX", false);
    assert_eq!(h.state().keytips.prefix(), Some("MX"), "MX must stay armed waiting for more input");
}

#[test]
fn keytip_mxa_() {
    let mut h = harness(blank());
    sequence(&mut h, "MXA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.calculationOptions"), "MXA must journal formulas.calculationOptions");
}

#[test]
fn keytip_mxm_() {
    let mut h = harness(blank());
    sequence(&mut h, "MXM", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.calculationOptions"), "MXM must journal formulas.calculationOptions");
}

#[test]
fn keytip_mxp_() {
    let mut h = harness(blank());
    sequence(&mut h, "MXP", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "formulas.calculationOptions"), "MXP must journal formulas.calculationOptions");
}

#[test]
fn keytip_ab_() {
    let mut h = harness(blank());
    sequence(&mut h, "AB", false);
    assert!(h.state().dialog.is_some(), "AB must open the subtotal dialog");
}

#[test]
fn keytip_ac_() {
    let mut h = harness(blank());
    sequence(&mut h, "AC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.clearFilter"), "AC must journal data.clearFilter");
}

#[test]
fn keytip_ad_() {
    let mut h = harness(blank());
    sequence(&mut h, "AD", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "AD must invoke data.consolidate and surface its missing-params error");
}

#[test]
fn keytip_ae_() {
    let mut h = harness(blank());
    sequence(&mut h, "AE", false);
    assert!(h.state().dialog.is_some(), "AE must open the textToColumns dialog");
}

#[test]
fn keytip_af1_() {
    let mut h = harness(blank());
    sequence(&mut h, "AF1", false);
    assert_eq!(h.state().keytips.prefix(), Some("AF1"), "AF1 must stay armed waiting for more input");
}

#[test]
fn keytip_af1c_() {
    let mut h = harness(blank());
    sequence(&mut h, "AF1C", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "AF1C must journal insert.picture");
}

#[test]
fn keytip_af1p_() {
    let mut h = harness(blank());
    sequence(&mut h, "AF1P", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "insert.picture"), "AF1P must journal insert.picture");
}

#[test]
fn keytip_afa_() {
    let mut h = harness(blank());
    sequence(&mut h, "AFA", false);
    assert!(h.state().dialog.is_none(), "AFA must not open a dialog");
    assert!(h.state().message.is_none(), "AFA must not open a message box");
    assert!(h.state().toast.is_none(), "AFA must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AFA must not journal");
}

#[test]
fn keytip_afc_() {
    let mut h = harness(blank());
    sequence(&mut h, "AFC", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "AFC must invoke data.forecastSheet and surface its missing-params error");
}

#[test]
fn keytip_afe_() {
    let mut h = harness(blank());
    sequence(&mut h, "AFE", false);
    assert!(h.state().dialog.is_none(), "AFE must not open a dialog");
    assert!(h.state().message.is_none(), "AFE must not open a message box");
    assert!(h.state().toast.is_none(), "AFE must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AFE must not journal");
}

#[test]
fn keytip_aff_() {
    let mut h = harness(blank());
    sequence(&mut h, "AFF", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "AFF must invoke edit.flashFill and surface its missing-params error");
}

#[test]
fn keytip_afo_() {
    let mut h = harness(blank());
    sequence(&mut h, "AFO", false);
    assert!(h.state().dialog.is_none(), "AFO must not open a dialog");
    assert!(h.state().message.is_none(), "AFO must not open a message box");
    assert!(h.state().toast.is_none(), "AFO must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AFO must not journal");
}

#[test]
fn keytip_aft_() {
    let mut h = harness(blank());
    sequence(&mut h, "AFT", false);
    assert!(h.state().dialog.is_none(), "AFT must not open a dialog");
    assert!(h.state().message.is_none(), "AFT must not open a message box");
    assert!(h.state().toast.is_none(), "AFT must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AFT must not journal");
}

#[test]
fn keytip_afw_() {
    let mut h = harness(blank());
    sequence(&mut h, "AFW", false);
    assert!(h.state().dialog.is_none(), "AFW must not open a dialog");
    assert!(h.state().message.is_none(), "AFW must not open a message box");
    assert!(h.state().toast.is_none(), "AFW must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AFW must not journal");
}

#[test]
fn keytip_ag_() {
    let mut h = harness(blank());
    sequence(&mut h, "AG", false);
    assert_eq!(h.state().keytips.prefix(), Some("AG"), "AG must stay armed waiting for more input");
}

#[test]
fn keytip_aga_() {
    let mut h = harness(blank());
    sequence(&mut h, "AGA", false);
    assert!(h.state().dialog.is_none(), "AGA must not open a dialog");
    assert!(h.state().message.is_none(), "AGA must not open a message box");
    assert!(h.state().toast.is_none(), "AGA must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AGA must not journal");
}

#[test]
fn keytip_agg_() {
    let mut h = harness(blank());
    sequence(&mut h, "AGG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.group"), "AGG must journal data.group");
}

#[test]
fn keytip_ah_() {
    let mut h = harness(blank());
    sequence(&mut h, "AH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.hideDetail"), "AH must journal data.hideDetail");
}

#[test]
fn keytip_aj_() {
    let mut h = harness(blank());
    sequence(&mut h, "AJ", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.showDetail"), "AJ must journal data.showDetail");
}

#[test]
fn keytip_ak_() {
    let mut h = harness(blank());
    sequence(&mut h, "AK", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "AK must run data.queriesConnections cleanly");
}

#[test]
fn keytip_am_() {
    let mut h = harness(blank());
    sequence(&mut h, "AM", false);
    assert!(h.state().dialog.is_some(), "AM must open the removeDuplicates dialog");
}

#[test]
fn keytip_ao_() {
    let mut h = harness(blank());
    sequence(&mut h, "AO", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "AO must run data.queriesConnections cleanly");
}

#[test]
fn keytip_ap1_() {
    let mut h = harness(blank());
    sequence(&mut h, "AP1", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.properties"), "AP1 must journal file.properties");
}

#[test]
fn keytip_apn_() {
    let mut h = harness(blank());
    sequence(&mut h, "APN", false);
    assert_eq!(h.state().keytips.prefix(), Some("APN"), "APN must stay armed waiting for more input");
}

#[test]
fn keytip_apnd_() {
    let mut h = harness(blank());
    sequence(&mut h, "APND", false);
    assert!(h.state().dialog.is_none(), "APND must not open a dialog");
    assert!(h.state().message.is_none(), "APND must not open a message box");
    assert!(h.state().toast.is_none(), "APND must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APND must not journal");
}

#[test]
fn keytip_apne_() {
    let mut h = harness(blank());
    sequence(&mut h, "APNE", false);
    assert!(h.state().dialog.is_none(), "APNE must not open a dialog");
    assert!(h.state().message.is_none(), "APNE must not open a message box");
    assert!(h.state().toast.is_none(), "APNE must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APNE must not journal");
}

#[test]
fn keytip_apng_() {
    let mut h = harness(blank());
    sequence(&mut h, "APNG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.getData"), "APNG must journal data.getData");
}

#[test]
fn keytip_apnh_() {
    let mut h = harness(blank());
    sequence(&mut h, "APNH", false);
    assert!(h.state().dialog.is_none(), "APNH must not open a dialog");
    assert!(h.state().message.is_none(), "APNH must not open a message box");
    assert!(h.state().toast.is_none(), "APNH must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APNH must not journal");
}

#[test]
fn keytip_apnl_() {
    let mut h = harness(blank());
    sequence(&mut h, "APNL", false);
    assert!(h.state().dialog.is_none(), "APNL must not open a dialog");
    assert!(h.state().message.is_none(), "APNL must not open a message box");
    assert!(h.state().toast.is_none(), "APNL must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APNL must not journal");
}

#[test]
fn keytip_apns_() {
    let mut h = harness(blank());
    sequence(&mut h, "APNS", false);
    assert!(h.state().dialog.is_none(), "APNS must not open a dialog");
    assert!(h.state().message.is_none(), "APNS must not open a message box");
    assert!(h.state().toast.is_none(), "APNS must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APNS must not journal");
}

#[test]
fn keytip_apnt_() {
    let mut h = harness(blank());
    sequence(&mut h, "APNT", false);
    assert!(h.state().dialog.is_none(), "APNT must not open a dialog");
    assert!(h.state().message.is_none(), "APNT must not open a message box");
    assert!(h.state().toast.is_none(), "APNT must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APNT must not journal");
}

#[test]
fn keytip_apr_() {
    let mut h = harness(blank());
    sequence(&mut h, "APR", false);
    assert!(h.state().dialog.is_none(), "APR must not open a dialog");
    assert!(h.state().message.is_none(), "APR must not open a message box");
    assert!(h.state().toast.is_none(), "APR must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APR must not journal");
}

#[test]
fn keytip_aps_() {
    let mut h = harness(blank());
    sequence(&mut h, "APS", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "APS must run data.queriesConnections cleanly");
}

#[test]
fn keytip_apt_() {
    let mut h = harness(blank());
    sequence(&mut h, "APT", false);
    assert!(h.state().dialog.is_none(), "APT must not open a dialog");
    assert!(h.state().message.is_none(), "APT must not open a message box");
    assert!(h.state().toast.is_none(), "APT must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "APT must not journal");
}

#[test]
fn keytip_aq_() {
    let mut h = harness(blank());
    sequence(&mut h, "AQ", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "AQ must invoke data.advancedFilter and surface its missing-params error");
}

#[test]
fn keytip_ar_() {
    let mut h = harness(blank());
    sequence(&mut h, "AR", false);
    assert_eq!(h.state().keytips.prefix(), Some("AR"), "AR must stay armed waiting for more input");
}

#[test]
fn keytip_ara_() {
    let mut h = harness(blank());
    sequence(&mut h, "ARA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.refreshAll"), "ARA must journal data.refreshAll");
}

#[test]
fn keytip_arc_() {
    let mut h = harness(blank());
    sequence(&mut h, "ARC", false);
    assert!(h.state().dialog.is_none(), "ARC must not open a dialog");
    assert!(h.state().message.is_none(), "ARC must not open a message box");
    assert!(h.state().toast.is_none(), "ARC must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "ARC must not journal");
}

#[test]
fn keytip_aro_() {
    let mut h = harness(blank());
    sequence(&mut h, "ARO", false);
    assert!(h.state().dialog.is_none(), "ARO must not open a dialog");
    assert!(h.state().message.is_none(), "ARO must not open a message box");
    assert!(h.state().toast.is_none(), "ARO must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "ARO must not journal");
}

#[test]
fn keytip_arr_() {
    let mut h = harness(blank());
    sequence(&mut h, "ARR", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "ARR must invoke pivot.refresh and surface its missing-params error");
}

#[test]
fn keytip_ars_() {
    let mut h = harness(blank());
    sequence(&mut h, "ARS", false);
    assert!(h.state().dialog.is_none(), "ARS must not open a dialog");
    assert!(h.state().message.is_none(), "ARS must not open a message box");
    assert!(h.state().toast.is_none(), "ARS must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "ARS must not journal");
}

#[test]
fn keytip_asa_() {
    let mut h = harness(blank());
    sequence(&mut h, "ASA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.sortAscending"), "ASA must journal data.sortAscending");
}

#[test]
fn keytip_asd_() {
    let mut h = harness(blank());
    sequence(&mut h, "ASD", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.sortDescending"), "ASD must journal data.sortDescending");
}

#[test]
fn keytip_ass_() {
    let mut h = harness(blank());
    sequence(&mut h, "ASS", false);
    assert!(h.state().dialog.is_some(), "ASS must open the sort dialog");
}

#[test]
fn keytip_at_() {
    let mut h = harness(blank());
    sequence(&mut h, "AT", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.filter"), "AT must journal data.filter");
}

#[test]
fn keytip_au_() {
    let mut h = harness(blank());
    sequence(&mut h, "AU", false);
    assert_eq!(h.state().keytips.prefix(), Some("AU"), "AU must stay armed waiting for more input");
}

#[test]
fn keytip_auc_() {
    let mut h = harness(blank());
    sequence(&mut h, "AUC", false);
    assert!(h.state().dialog.is_none(), "AUC must not open a dialog");
    assert!(h.state().message.is_none(), "AUC must not open a message box");
    assert!(h.state().toast.is_none(), "AUC must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AUC must not journal");
}

#[test]
fn keytip_auu_() {
    let mut h = harness(blank());
    sequence(&mut h, "AUU", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.ungroup"), "AUU must journal data.ungroup");
}

#[test]
fn keytip_av_() {
    let mut h = harness(blank());
    sequence(&mut h, "AV", false);
    assert_eq!(h.state().keytips.prefix(), Some("AV"), "AV must stay armed waiting for more input");
}

#[test]
fn keytip_avi_() {
    let mut h = harness(blank());
    sequence(&mut h, "AVI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.circleInvalid"), "AVI must journal data.circleInvalid");
}

#[test]
fn keytip_avr_() {
    let mut h = harness(blank());
    sequence(&mut h, "AVR", false);
    assert!(h.state().dialog.is_none(), "AVR must not open a dialog");
    assert!(h.state().message.is_none(), "AVR must not open a message box");
    assert!(h.state().toast.is_none(), "AVR must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AVR must not journal");
}

#[test]
fn keytip_avv_() {
    let mut h = harness(blank());
    sequence(&mut h, "AVV", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.validation"), "AVV must journal data.validation");
}

#[test]
fn keytip_aw_() {
    let mut h = harness(blank());
    sequence(&mut h, "AW", false);
    assert_eq!(h.state().keytips.prefix(), Some("AW"), "AW must stay armed waiting for more input");
}

#[test]
fn keytip_awg_() {
    let mut h = harness(blank());
    sequence(&mut h, "AWG", false);
    assert!(h.state().dialog.is_some(), "AWG must open the goalSeek dialog");
}

#[test]
fn keytip_aws_() {
    let mut h = harness(blank());
    sequence(&mut h, "AWS", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "AWS must run data.scenarioManager cleanly");
}

#[test]
fn keytip_awt_() {
    let mut h = harness(blank());
    sequence(&mut h, "AWT", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "AWT must invoke data.dataTable and surface its missing-params error");
}

#[test]
fn keytip_ax_() {
    let mut h = harness(blank());
    sequence(&mut h, "AX", false);
    assert!(h.state().dialog.is_none(), "AX must not open a dialog");
    assert!(h.state().message.is_none(), "AX must not open a message box");
    assert!(h.state().toast.is_none(), "AX must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "AX must not journal");
}

#[test]
fn keytip_ay_() {
    let mut h = harness(blank());
    sequence(&mut h, "AY", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "data.reapply"), "AY must journal data.reapply");
}

#[test]
fn keytip_ra1_() {
    let mut h = harness(blank());
    sequence(&mut h, "RA1", false);
    assert!(h.state().dialog.is_some(), "RA1 must open the accessibility dialog");
}

#[test]
fn keytip_ra2_() {
    let mut h = harness(blank());
    sequence(&mut h, "RA2", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "RA2 must run review.showComments cleanly");
}

#[test]
fn keytip_rc_() {
    let mut h = harness(blank());
    sequence(&mut h, "RC", false);
    assert!(h.state().dialog.is_some(), "RC must open the comment dialog");
}

#[test]
fn keytip_rd_() {
    let mut h = harness(blank());
    sequence(&mut h, "RD", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "review.deleteComment"), "RD must journal review.deleteComment");
}

#[test]
fn keytip_rg_() {
    let mut h = harness(blank());
    sequence(&mut h, "RG", false);
    assert!(h.state().dialog.is_none(), "RG must not open a dialog");
    assert!(h.state().message.is_none(), "RG must not open a message box");
    assert!(h.state().toast.is_none(), "RG must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "RG must not journal");
}

#[test]
fn keytip_rl_() {
    let mut h = harness(blank());
    sequence(&mut h, "RL", false);
    assert!(h.state().dialog.is_none(), "RL must not open a dialog");
    assert!(h.state().message.is_none(), "RL must not open a message box");
    assert!(h.state().toast.is_none(), "RL must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "RL must not journal");
}

#[test]
fn keytip_rn_() {
    let mut h = harness(blank());
    sequence(&mut h, "RN", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "RN must invoke review.nextComment and surface its missing-params error");
}

#[test]
fn keytip_ro_() {
    let mut h = harness(blank());
    sequence(&mut h, "RO", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "review.protectWorkbook"), "RO must journal review.protectWorkbook");
}

#[test]
fn keytip_rps_() {
    let mut h = harness(blank());
    sequence(&mut h, "RPS", false);
    assert!(h.state().dialog.is_some(), "RPS must open the protectSheet dialog");
}

#[test]
fn keytip_rpw_() {
    let mut h = harness(blank());
    sequence(&mut h, "RPW", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "review.protectWorkbook"), "RPW must journal review.protectWorkbook");
}

#[test]
fn keytip_rrs_() {
    let mut h = harness(blank());
    sequence(&mut h, "RRS", false);
    assert!(h.state().dialog.is_none(), "RRS must not open a dialog");
    assert!(h.state().message.is_none(), "RRS must not open a message box");
    assert!(h.state().toast.is_none(), "RRS must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "RRS must not journal");
}

#[test]
fn keytip_rs_() {
    let mut h = harness(blank());
    sequence(&mut h, "RS", false);
    assert!(h.state().dialog.is_some(), "RS must open the spelling dialog");
}

#[test]
fn keytip_rtn_() {
    let mut h = harness(blank());
    sequence(&mut h, "RTN", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "review.newNote"), "RTN must journal review.newNote");
}

#[test]
fn keytip_rv_() {
    let mut h = harness(blank());
    sequence(&mut h, "RV", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "RV must invoke review.previousComment and surface its missing-params error");
}

#[test]
fn keytip_wa_() {
    let mut h = harness(blank());
    sequence(&mut h, "WA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.arrangeAll"), "WA must journal view.arrangeAll");
}

#[test]
fn keytip_wc_() {
    let mut h = harness(blank());
    sequence(&mut h, "WC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.customViews"), "WC must journal view.customViews");
}

#[test]
fn keytip_wf_() {
    let mut h = harness(blank());
    sequence(&mut h, "WF", false);
    assert_eq!(h.state().keytips.prefix(), Some("WF"), "WF must stay armed waiting for more input");
}

#[test]
fn keytip_wfc_() {
    let mut h = harness(blank());
    sequence(&mut h, "WFC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.freezeFirstColumn"), "WFC must journal view.freezeFirstColumn");
}

#[test]
fn keytip_wff_() {
    let mut h = harness(blank());
    sequence(&mut h, "WFF", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.freezePanes"), "WFF must journal view.freezePanes");
}

#[test]
fn keytip_wfr_() {
    let mut h = harness(blank());
    sequence(&mut h, "WFR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.freezeTopRow"), "WFR must journal view.freezeTopRow");
}

#[test]
fn keytip_wg_() {
    let mut h = harness(blank());
    sequence(&mut h, "WG", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.zoomToSelection"), "WG must journal view.zoomToSelection");
}

#[test]
fn keytip_wh_() {
    let mut h = harness(blank());
    sequence(&mut h, "WH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.hideWindow"), "WH must journal view.hideWindow");
}

#[test]
fn keytip_wi_() {
    let mut h = harness(blank());
    sequence(&mut h, "WI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.pageBreakPreview"), "WI must journal view.pageBreakPreview");
}

#[test]
fn keytip_wj_() {
    let mut h = harness(blank());
    sequence(&mut h, "WJ", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "WJ must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_wl_() {
    let mut h = harness(blank());
    sequence(&mut h, "WL", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.normal"), "WL must journal view.normal");
}

#[test]
fn keytip_wm_() {
    let mut h = harness(blank());
    sequence(&mut h, "WM", false);
    assert_eq!(h.state().keytips.prefix(), Some("WM"), "WM must stay armed waiting for more input");
}

#[test]
fn keytip_wmr_() {
    let mut h = harness(blank());
    sequence(&mut h, "WMR", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "WMR must run view.macros cleanly");
}

#[test]
fn keytip_wmv_() {
    let mut h = harness(blank());
    sequence(&mut h, "WMV", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "WMV must run view.macros cleanly");
}

#[test]
fn keytip_wn_() {
    let mut h = harness(blank());
    sequence(&mut h, "WN", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.newWindow"), "WN must journal view.newWindow");
}

#[test]
fn keytip_wp_() {
    let mut h = harness(blank());
    sequence(&mut h, "WP", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.pageLayout"), "WP must journal view.pageLayout");
}

#[test]
fn keytip_wq_() {
    let mut h = harness(blank());
    sequence(&mut h, "WQ", false);
    assert!(h.state().dialog.is_some(), "WQ must open the zoom dialog");
}

#[test]
fn keytip_wr_() {
    let mut h = harness(blank());
    sequence(&mut h, "WR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.ruler"), "WR must journal view.ruler");
}

#[test]
fn keytip_ws_() {
    let mut h = harness(blank());
    sequence(&mut h, "WS", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.split"), "WS must journal view.split");
}

#[test]
fn keytip_wt_() {
    let mut h = harness(blank());
    sequence(&mut h, "WT", false);
    assert!(h.state().dialog.is_none(), "WT must not open a dialog");
    assert!(h.state().message.is_none(), "WT must not open a message box");
    assert!(h.state().toast.is_none(), "WT must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "WT must not journal");
}

#[test]
fn keytip_wu_() {
    let mut h = harness(blank());
    sequence(&mut h, "WU", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.unhideWindow"), "WU must journal view.unhideWindow");
}

#[test]
fn keytip_wvc_() {
    let mut h = harness(blank());
    sequence(&mut h, "WVC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.focusCell"), "WVC must journal view.focusCell");
}

#[test]
fn keytip_wvf_() {
    let mut h = harness(blank());
    sequence(&mut h, "WVF", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "WVF must route to its control without an error box");
    assert_eq!(h.state().keytips.prefix(), None);
}

#[test]
fn keytip_wvh_() {
    let mut h = harness(blank());
    sequence(&mut h, "WVH", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.headings"), "WVH must journal view.headings");
}

#[test]
fn keytip_wvn_() {
    let mut h = harness(blank());
    sequence(&mut h, "WVN", false);
    assert!(h.state().dialog.is_none(), "WVN must not open a dialog");
    assert!(h.state().message.is_none(), "WVN must not open a message box");
    assert!(h.state().toast.is_none(), "WVN must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "WVN must not journal");
}

#[test]
fn keytip_wvr_() {
    let mut h = harness(blank());
    sequence(&mut h, "WVR", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.ruler"), "WVR must journal view.ruler");
}

#[test]
fn keytip_ww_() {
    let mut h = harness(blank());
    sequence(&mut h, "WW", false);
    assert!(h.state().message.is_none() && h.state().toast.is_none(), "WW must run view.switchWindows cleanly");
}

#[test]
fn keytip_jam_() {
    let mut h = harness(blank());
    sequence(&mut h, "JAM", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "JAM must invoke chart.formatSelection and surface its missing-params error");
}

#[test]
fn keytip_jca_() {
    let mut h = harness(blank());
    sequence(&mut h, "JCA", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "JCA must invoke chart.addElement and surface its missing-params error");
}

#[test]
fn keytip_jcc_() {
    let mut h = harness(blank());
    sequence(&mut h, "JCC", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "JCC must invoke chart.changeType and surface its missing-params error");
}

#[test]
fn keytip_jts_() {
    let mut h = harness(blank());
    sequence(&mut h, "JTS", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "JTS must invoke table.style and surface its missing-params error");
}

#[test]
fn keytip_jtv_() {
    let mut h = harness(blank());
    sequence(&mut h, "JTV", false);
    assert!(h.state().message.is_some() || h.state().toast.is_some(), "JTV must invoke table.summarizePivot and surface its missing-params error");
}

#[test]
fn keytip_fa_() {
    let mut h = harness(blank());
    sequence(&mut h, "FA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.saveAs"), "FA must journal file.saveAs");
}

#[test]
fn keytip_fc_() {
    let mut h = harness(blank());
    sequence(&mut h, "FC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.close"), "FC must journal file.close");
}

#[test]
fn keytip_fea_() {
    let mut h = harness(blank());
    sequence(&mut h, "FEA", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.exportPdf"), "FEA must journal file.exportPdf");
}

#[test]
fn keytip_fec_() {
    let mut h = harness(blank());
    sequence(&mut h, "FEC", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.saveAs"), "FEC must journal file.saveAs");
}

#[test]
fn keytip_fi_() {
    let mut h = harness(blank());
    sequence(&mut h, "FI", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.properties"), "FI must journal file.properties");
}

#[test]
fn keytip_fn_() {
    let mut h = harness(blank());
    sequence(&mut h, "FN", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.new"), "FN must journal file.new");
}

#[test]
fn keytip_fo_() {
    let mut h = harness(blank());
    sequence(&mut h, "FO", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.open"), "FO must journal file.open");
}

#[test]
fn keytip_fp_() {
    let mut h = harness(blank());
    sequence(&mut h, "FP", false);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "file.print"), "FP must journal file.print");
}

#[test]
fn keytip_q_() {
    let mut h = harness(blank());
    sequence(&mut h, "Q", false);
    assert!(h.state().dialog.is_none(), "Q must not open a dialog");
    assert!(h.state().message.is_none(), "Q must not open a message box");
    assert!(h.state().toast.is_none(), "Q must not raise a toast");
    assert!(h.state().session.journal.is_empty(), "Q must not journal");
}

#[test]
fn generated_test_count_matches_the_table() {
    const EXPECTED: usize = 364;
    assert_eq!(gridcraft_ui_egui::keytips::keytip_table::SEQUENCES.len(), EXPECTED);
}

#[test]
fn sample_sequences_work_with_text_before_key() {
    // Some IME paths deliver Event::Text ahead of the key press; the matcher
    // must pair them either way. One representative of each action kind;
    // the dialog opener runs last so it does not eat later input.
    // Each step resets first: Escape twice leaves keytip mode so the next
    // F10 arms cleanly (a lingering prefix would make F10 disarm instead).
    let mut h = harness(blank());
    sequence(&mut h, "H", true);
    assert_eq!(h.state().ui.ribbon_tab, "Home");
    reset(&mut h);
    sequence(&mut h, "WVG", true);
    assert!(h.state().session.journal.iter().any(|(id, _)| id == "view.gridlines"));
    reset(&mut h);
    sequence(&mut h, "HV", true);
    assert_eq!(h.state().keytips.prefix(), Some("HV"));
    reset(&mut h);
    sequence(&mut h, "NE", true);
    assert!(h.state().dialog.is_none() && h.state().message.is_none() && h.state().toast.is_none());
    reset(&mut h);
    sequence(&mut h, "HVS", true);
    assert!(h.state().dialog.is_some(), "HVS must open the paste-special dialog");
}

fn reset(h: &mut Harness) {
    key(h, Key::Escape); // clear any prefix, back to the armed root
    key(h, Key::Escape); // leave keytip mode entirely
}
