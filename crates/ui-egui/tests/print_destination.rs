//! The Print page's one Printer list (system queues plus "Network printer…") and its Grayscale choice,
//! driven with in-process fakes only: no spooler, no printer and no socket is touched.
use std::{cell::RefCell, rc::Rc};

use egui_kittest::kittest::{NodeT, Queryable};
use gridcraft_engine::Session;
use gridcraft_ui_egui::printing::{Network, Printer};
use gridcraft_ui_egui::{Services, SheetApp};
use serde_json::json;

/// A network service whose every worker is a stub: the tests never query or send.
fn fake_network() -> Network {
    Network {
        validate: Box::new(|_| Ok(())),
        query: Box::new(|_| std::sync::mpsc::channel().1),
        send: Box::new(|_| Box::new(|_, _| panic!("no network job is sent in these tests"))),
        formats: &["application/pdf"],
    }
}

fn app_with(printers: &[(&str, bool)], network: bool) -> SheetApp {
    let mut session = Session::new();
    session.new_workbook();
    let mut app = SheetApp::new(session, Services::default());
    app.run("cell.set", json!({"cell": "A1", "input": "A sheet to print"})).unwrap();
    app.run("home.fillColor", json!({"range": "A1", "color": "#DC1E28"})).unwrap();
    let list: Vec<Printer> =
        printers.iter().map(|(name, default)| Printer { name: (*name).into(), label: String::new(), default: *default }).collect();
    app.services.printers = Some(Box::new(move || {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(list.clone()).unwrap();
        receiver
    }));
    app.services.print_native = Some(Box::new(|_, _| std::sync::mpsc::channel().1));
    if network {
        app.services.network_print = Some(fake_network());
    }
    app.run("ui.backstage", json!({"page": "print"})).unwrap();
    app
}

fn harness(app: SheetApp) -> egui_kittest::Harness<'static, SheetApp> {
    let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(900.0, 700.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        app,
    );
    harness.run_steps(5);
    harness
}

fn open_printer_list(harness: &mut egui_kittest::Harness<'static, SheetApp>) {
    harness.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Printer").click();
    harness.run_steps(3);
}

#[test]
fn one_printer_list_holds_the_queues_and_the_network_entry() {
    let app = app_with(&[("Office Laser", false), ("Front Desk", true)], true);
    let mut harness = harness(app);
    assert!(harness.query_by_label("System printer").is_none(), "no System/Network destination switch");
    assert!(harness.query_by_label("Network printer").is_none());
    open_printer_list(&mut harness);
    // The default queue is first and marked; the network entry is the last entry.
    let top = |label: &str| harness.get_by_label(label).accesskit_node().raw_bounds().expect("laid out").y0;
    let (default, other, network) = (top("Front Desk (default)"), top("Office Laser"), top("Network printer…"));
    assert!(default < other && other < network, "default queue first, the network entry last: {default} {other} {network}");
}

#[test]
fn a_queue_shows_its_label_else_its_name_and_selecting_stores_the_name() {
    let mut app = app_with(&[], false);
    let list = vec![
        Printer { name: "HP_LJ_4".into(), label: "Front Desk Laser".into(), default: false },
        Printer { name: "Plain Queue".into(), label: String::new(), default: false },
    ];
    app.services.printers = Some(Box::new(move || {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(list.clone()).unwrap();
        receiver
    }));
    let mut harness = harness(app);
    open_printer_list(&mut harness);
    assert!(harness.query_by_label("HP_LJ_4").is_none(), "the label replaces the name in the row");
    assert!(harness.query_by_label("Plain Queue").is_some(), "no label: the name");
    harness.get_by_label("Front Desk Laser").click();
    harness.run_steps(4);
    assert_eq!(harness.state().printing.settings.printer, "HP_LJ_4", "the name stays the selection key");
    let combo = harness.get_by_role_and_label(egui::accesskit::Role::ComboBox, "Printer");
    let shown = combo.accesskit_node().value().or_else(|| combo.accesskit_node().label()).unwrap_or_default();
    assert_eq!(shown, "Front Desk Laser", "the selected text shows the label");
}

#[test]
fn choosing_the_network_entry_shows_the_network_section() {
    let app = app_with(&[("Office Laser", true)], true);
    let mut harness = harness(app);
    assert!(!harness.state().network_selected());
    assert!(harness.query_by_label("Query printer").is_none());
    assert!(harness.query_by_label("Grayscale").is_some(), "a queue is chosen: its options show");
    open_printer_list(&mut harness);
    harness.get_by_label("Network printer…").click();
    harness.run_steps(4);
    assert!(harness.state().network_selected());
    assert!(harness.query_by_label("Query printer").is_some(), "the address and query controls appear");
    assert!(harness.query_by_label("Grayscale").is_none(), "the queue's own options are not shown for the network");
    // Back to a queue.
    open_printer_list(&mut harness);
    harness.get_by_label("Office Laser (default)").click();
    harness.run_steps(4);
    assert!(!harness.state().network_selected());
    assert_eq!(harness.state().printing.settings.printer, "Office Laser");
    assert!(harness.query_by_label("Query printer").is_none());
}

#[test]
fn without_a_network_service_the_list_has_only_queues() {
    let app = app_with(&[("Office Laser", true)], false);
    let mut harness = harness(app);
    open_printer_list(&mut harness);
    assert!(harness.query_by_label("Network printer…").is_none());
    assert!(!harness.state().network_selected());
}

#[test]
fn the_colour_choice_everywhere_says_grayscale() {
    let labels = gridcraft_ui_egui::printing::network_labels(gridcraft_ui_egui::i18n::Language::En);
    assert_eq!(labels.color_monochrome, "Grayscale");
    assert_eq!(labels.unsupported_color, "This printer only prints in grayscale.");
    let app = app_with(&[("Office Laser", true)], false);
    let harness = harness(app);
    assert!(harness.query_by_label("Grayscale").is_some());
    assert!(harness.query_by_label("Black and white").is_none());
}

/// Every `r g b rg` / `RG` colour in a PDF's (uncompressed) page content.
fn colours(pdf: &[u8]) -> Vec<[String; 3]> {
    let text = String::from_utf8_lossy(pdf);
    let words: Vec<&str> = text.split_whitespace().collect();
    words.windows(4).filter(|w| w[3] == "rg" || w[3] == "RG").map(|w| [w[0].into(), w[1].into(), w[2].into()]).collect()
}

#[test]
fn grayscale_prints_a_gray_pdf_and_the_preview_follows_the_box() {
    let sent: Rc<RefCell<Vec<(Vec<u8>, bool)>>> = Rc::default();
    let mut app = app_with(&[("Office Laser", true)], false);
    let seen = sent.clone();
    app.services.print_native = Some(Box::new(move |bytes, settings| {
        seen.borrow_mut().push((bytes, settings.grayscale));
        let (sender, receiver) = std::sync::mpsc::channel();
        sender.send(Ok("queued".into())).unwrap();
        receiver
    }));
    let mut harness = harness(app);
    // Colour: the preview is not gray and the PDF keeps its colours.
    assert!(!harness.state().print_grayscale());
    harness.state_mut().run("ui.printSubmit", json!({})).unwrap();
    harness.run_steps(3);
    {
        let sent = sent.borrow();
        assert!(!sent[0].1);
        assert!(colours(&sent[0].0).iter().any(|c| c[0] != c[1] || c[1] != c[2]), "a colour print keeps its colours");
    }
    // Tick Grayscale: the preview's flag flips at once (the page paints from it every frame, so it
    // never shows colour after the toggle) and the PDF that is sent is gray, with the driver hint too.
    harness.get_by_label("Grayscale").click();
    harness.run_steps(3);
    assert!(harness.state().print_grayscale());
    harness.state_mut().run("ui.printSubmit", json!({})).unwrap();
    let sent = sent.borrow();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].1, "the driver is still asked for monochrome");
    let gray = colours(&sent[1].0);
    assert!(!gray.is_empty() && gray.iter().all(|c| c[0] == c[1] && c[1] == c[2]), "{gray:?}");
}
