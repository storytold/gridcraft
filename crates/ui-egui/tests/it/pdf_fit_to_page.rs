//! File › Export and Save As: the PDF "Fit Sheet on One Page" option.

use std::sync::{Arc, Mutex};

use egui_kittest::kittest::{NodeT as _, Queryable as _};
use gridcraft_ui_egui::i18n::Language;
use gridcraft_ui_egui::{Services, SheetApp};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Pages in a PDF the exporter wrote: the page tree's `/Count`.
fn pdf_pages(bytes: &[u8]) -> usize {
    let text = String::from_utf8_lossy(bytes);
    let at = text.find("/Count ").expect("a page tree") + "/Count ".len();
    text[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().expect("a page count")
}

/// A harness whose Save picker names each export `<dir>/Export<n>`, in order.
fn export_harness(tag: &str) -> (Harness, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("gridcraft-fit-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (count, target) = (Arc::new(Mutex::new(0)), dir.clone());
    let h = crate::support::services_harness(
        Services {
            pick_save: Some(Box::new(move |_| {
                let mut n = count.lock().unwrap();
                *n += 1;
                Some(target.join(format!("Export{n}")).to_string_lossy().into_owned())
            })),
            ..Default::default()
        },
        4,
    );
    (h, dir)
}

fn fill_wide_row(h: &mut Harness, columns: i64) {
    let row: Vec<i64> = (1..=columns).collect();
    h.state_mut().run("range.setValues", serde_json::json!({"range": "A1", "values": [row]})).unwrap();
}

/// Opens File (the Backstage view) if it is closed, then clicks a navigation entry or page button.
fn choose(h: &mut Harness, label: &str) {
    if h.state().backstage.is_none() {
        h.get_by_label("File").click();
        h.run_steps(3);
    }
    click(h, label);
}

fn click(h: &mut Harness, label: &str) {
    h.get_by_role_and_label(egui::accesskit::Role::Button, label).click();
    h.run_steps(3);
}

fn tick_fit_to_one_page(h: &mut Harness) {
    h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Fit Sheet on One Page").click();
    h.run_steps(3);
}

#[test]
fn export_pdf_fit_to_one_page_checkbox_fits_a_wide_sheet_and_resets_when_reopened() {
    let (mut h, dir) = export_harness("wide");
    fill_wide_row(&mut h, 40);
    choose(&mut h, "Export");
    assert!(!h.state().backstage.as_ref().unwrap().pdf_fit_to_page, "the option starts off");
    click(&mut h, "Create PDF");
    let off = pdf_pages(&std::fs::read(dir.join("Export1.pdf")).unwrap());
    assert!(off > 1, "without the option the wide sheet paginates: {off}");

    // The Backstage view was closed by the export; reopening starts from the default again.
    choose(&mut h, "Export");
    let bs = h.state().backstage.as_ref().unwrap();
    assert!(!bs.pdf_fit_to_page, "the setting does not outlive the Backstage view");
    tick_fit_to_one_page(&mut h);
    let bs = h.state().backstage.as_ref().unwrap();
    assert!(bs.pdf_fit_to_page && !bs.fit_to_page, "ticking it leaves the Print page's own option alone");
    click(&mut h, "Create PDF");
    assert_eq!(pdf_pages(&std::fs::read(dir.join("Export2.pdf")).unwrap()), 1);
    assert!(h.state().message.is_none(), "a sheet that fits gets no warning");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn save_as_pdf_offers_the_fit_option() {
    let (mut h, dir) = export_harness("saveas");
    fill_wide_row(&mut h, 40);
    choose(&mut h, "Save As");
    h.state_mut().backstage.as_mut().unwrap().save_format = "pdf";
    h.run_steps(2);
    tick_fit_to_one_page(&mut h);
    click(&mut h, "Browse…");
    assert_eq!(pdf_pages(&std::fs::read(dir.join("Export1.pdf")).unwrap()), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn export_pdf_says_so_when_a_sheet_is_too_large_for_one_page() {
    let (mut h, dir) = export_harness("toolarge");
    // 400 columns do not fit one page even at the smallest scale.
    fill_wide_row(&mut h, 400);
    choose(&mut h, "Export");
    tick_fit_to_one_page(&mut h);
    click(&mut h, "Create PDF");
    let pages = pdf_pages(&std::fs::read(dir.join("Export1.pdf")).unwrap());
    assert!(pages > 1);
    let (_, text) = h.state().message.clone().expect("the user is told the PDF has several pages");
    assert!(text.contains("too large") && text.contains(&pages.to_string()), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_fit_to_one_page_label_is_translated_in_every_language() {
    for l in Language::ALL {
        if l == Language::En {
            continue;
        }
        assert_ne!(l.tr("Fit Sheet on One Page"), "Fit Sheet on One Page", "{l:?} has no translation");
    }
}
