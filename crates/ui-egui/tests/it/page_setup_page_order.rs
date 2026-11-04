//! Page Setup › Page order: the dialog shows the sheet's order, and OK sets the chosen one with the
//! margins in one undo step, so printing goes down, then over or over, then down.

use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::model::PageOrder;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::control::{Outcome, handle};
use gridcraft_ui_egui::i18n::Language;
use serde_json::{Value as Json, json};

type Harness = egui_kittest::Harness<'static, SheetApp>;

/// Four pages, two tall and two wide: print area A1:D4 with breaks before row 3 and column C.
fn harness() -> Harness {
    let mut h = crate::support::blank_harness(4);
    let app = h.state_mut();
    app.run("range.setValues", json!({"range": "A1", "values": [["red", null, "blue"], [], ["green", null, "yellow"]]})).unwrap();
    app.run("pageLayout.pageSetup", json!({"printArea": "A1:D4", "rowBreaks": [3], "colBreaks": ["C"], "left": 1.25})).unwrap();
    h.run_steps(2);
    h
}

fn order(h: &Harness) -> (PageOrder, f32) {
    let print = &h.state().session.active().unwrap().wb.active().unwrap().print;
    (print.page_order, print.margins[0])
}

/// The printed pages' ranges, in print order.
fn pages(h: &mut Harness) -> Vec<String> {
    let pv = h.state_mut().run("file.printPreview", json!({})).unwrap();
    pv["pageList"].as_array().unwrap().iter().map(|p| p["range"].as_str().unwrap().to_string()).collect()
}

fn control(h: &mut Harness, method: &str, params: Json) -> Json {
    let ctx = h.ctx.clone();
    match handle(h.state_mut(), &ctx, method, &params) {
        Outcome::Done(v) => v,
        Outcome::Screenshot { .. } => json!(null),
    }
}

#[test]
fn page_setup_sets_the_page_order() {
    let mut h = harness();
    assert_eq!(pages(&mut h), ["A1:B2", "A3:B4", "C1:D2", "C3:D4"]);
    // Page Layout › Print Titles opens Page Setup on its Sheet tab.
    h.state_mut().open_dialog("pageSetup", json!({"tab": "Sheet"}));
    h.run_steps(3);
    assert!(h.query_by_label("Page order").is_some());
    assert_eq!(h.state().dialog.as_ref().unwrap().values["pageOrder"], "downThenOver");
    h.get_by_label("Over, then down").click();
    h.run_steps(2);
    assert_eq!(h.state().dialog.as_ref().unwrap().values["pageOrder"], "overThenDown");
    h.get_by_label_contains("OK").click();
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "OK closes the dialog");
    assert_eq!(order(&h), (PageOrder::OverThenDown, 1.25), "the order is set and the margins kept");
    assert_eq!(pages(&mut h), ["A1:B2", "C1:D2", "A3:B4", "C3:D4"]);
    // Opened again, it shows the sheet's order.
    h.state_mut().open_dialog("pageSetup", json!({}));
    assert_eq!(h.state().dialog.as_ref().unwrap().values["pageOrder"], "overThenDown");
    h.state_mut().dialog = None;
    // One undo step takes it back.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(order(&h), (PageOrder::DownThenOver, 1.25));
    assert_eq!(pages(&mut h), ["A1:B2", "A3:B4", "C1:D2", "C3:D4"]);
}

#[test]
fn confirming_page_setup_sets_the_order_and_keeps_margins_at_zero_or_more() {
    let mut h = harness();
    control(&mut h, "ui.dialog", json!({"name": "pageSetup"}));
    control(&mut h, "ui.dialog.set", json!({"field": "pageOrder", "value": "overThenDown"}));
    // A margin dragged below zero is zero, as with the Margins command.
    control(&mut h, "ui.dialog.set", json!({"field": "left", "value": -0.5}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(true), "{r}");
    assert_eq!(order(&h), (PageOrder::OverThenDown, 0.0));
    // A bad order is refused and the dialog stays open.
    control(&mut h, "ui.dialog", json!({"name": "pageSetup"}));
    control(&mut h, "ui.dialog.set", json!({"field": "pageOrder", "value": "sideways"}));
    let r = control(&mut h, "ui.dialog.confirm", json!({}));
    assert_eq!(r["ok"], json!(false), "{r}");
    assert!(h.state().dialog.is_some());
    assert_eq!(order(&h).0, PageOrder::OverThenDown);
}

#[test]
fn page_order_is_translated_where_page_setup_is() {
    let mut h = harness();
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.state_mut().open_dialog("pageSetup", json!({"tab": "Sheet"}));
    h.run_steps(3);
    for (english, german) in [
        ("Page order", "Seitenreihenfolge"),
        ("Down, then over", "Seiten nach unten, dann nach rechts"),
        ("Over, then down", "Seiten nach rechts, dann nach unten"),
    ] {
        assert!(h.query_by_label(german).is_some(), "no {german}");
        assert!(h.query_by_label(english).is_none(), "{english} is untranslated");
    }
    // Every catalog with the dialog's margin labels has the page order too.
    for language in Language::ALL {
        if language.tr("Left margin (in):") == "Left margin (in):" {
            continue;
        }
        for label in ["Page order", "Down, then over", "Over, then down"] {
            assert_ne!(language.tr(label), label, "{language:?}: {label}");
        }
    }
}
