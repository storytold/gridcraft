//! Copy/cut route rich tables to the host without a later egui text write replacing them.

use std::cell::RefCell;
use std::rc::Rc;

use egui::accesskit::Role;
use egui::{Event, Modifiers, OutputCommand, PointerButton};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{Services, SheetApp, grid};
use serde_json::json;

fn harness(services: Services) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session.execute("range.setValues", json!({"range": "A1", "values": [["Name", "Score"], ["Ada", 42]]})).unwrap();
    session.execute("selection.set", json!({"range": "A1:B2"})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            let ctx = ui.ctx().clone();
            app.logic(&ctx);
            app.ui(ui);
        },
        SheetApp::new(session, services),
    );
    h.run_steps(4);
    h
}

#[test]
fn copy_and_cut_publish_html_and_text_without_overwriting_rich_clipboard() {
    for event in [Event::Copy, Event::Cut] {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let received = calls.clone();
        let mut h = harness(Services {
            copy_html: Some(Box::new(move |html, text| {
                received.borrow_mut().push((html.to_string(), text.to_string()));
                Ok(())
            })),
            ..Default::default()
        });
        h.input_mut().events.push(event.clone());
        h.step();
        let calls = calls.borrow();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].0.contains("<table"));
        assert!(calls[0].0.contains("Ada"));
        assert_eq!(calls[0].1, "Name\tScore\nAda\t42\n");
        assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(_))));
        assert_eq!(h.state().session.clipboard.as_ref().map(|c| c.cut), Some(matches!(event, Event::Cut)));
    }
}

#[test]
fn missing_or_failed_rich_clipboard_falls_back_to_plain_text() {
    for fail in [false, true] {
        let mut services = Services::default();
        if fail {
            services.copy_html = Some(Box::new(|_, _| Err("clipboard unavailable".into())));
        }
        let mut h = harness(services);
        h.input_mut().events.push(Event::Copy);
        h.step();
        assert!(
            h.output().platform_output.commands.iter().any(|c| { matches!(c, OutputCommand::CopyText(text) if text == "Name\tScore\nAda\t42\n") })
        );
    }
}

#[test]
fn programmatic_copy_does_not_write_to_the_system_clipboard() {
    let calls = Rc::new(RefCell::new(0));
    let received = calls.clone();
    let mut h = harness(Services {
        copy_html: Some(Box::new(move |_, _| {
            *received.borrow_mut() += 1;
            Ok(())
        })),
        ..Default::default()
    });
    let copied = h.state_mut().run("edit.copy", json!({})).unwrap();
    assert!(copied.get("html").is_none(), "HTML is only built when requested");
    let copied = h.state_mut().run("edit.copy", json!({"html": true})).unwrap();
    assert!(copied["html"].is_string());
    assert_eq!(*calls.borrow(), 0);
    assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(_))));
}

#[test]
fn pasting_reads_the_clipboard_html_for_the_numbers_behind_the_text() {
    // #257: currency text from another spreadsheet stays a number when the host offers the HTML.
    let mut h = harness(Services {
        paste_html: Some(Box::new(|| Some(r#"<table><tr><td x:num="10000">Rp10,000.00</td></tr></table>"#.to_string()))),
        ..Default::default()
    });
    h.state_mut().run("selection.set", json!({"cell": "D5"})).unwrap();
    h.input_mut().events.push(Event::Paste("Rp10,000.00".into()));
    h.run_steps(2);
    let value = h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(gridcraft_engine::core::CellRef::new(4, 3))));
    assert_eq!(value, Some(gridcraft_engine::core::Value::number(10000.0)));
}

#[test]
fn copying_multiple_areas_copies_lined_up_areas_and_explains_the_rest() {
    // Issue #348: lined-up areas copy as one block; others get Excel's message instead of nothing.
    let mut h = harness(Services::default());
    h.state_mut().session.execute("selection.set", json!({"range": "B1:B2,A1:A2"})).unwrap();
    h.input_mut().events.push(Event::Copy);
    h.step();
    assert!(h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(text) if text == "Name\tScore\nAda\t42\n")));
    assert!(h.state().message.is_none());
    assert_eq!(h.state().session.clipboard.as_ref().map(|c| c.areas.len()), Some(2));

    h.state_mut().session.execute("selection.set", json!({"range": "A1,B2"})).unwrap();
    h.input_mut().events.push(Event::Copy);
    h.step();
    assert!(!h.output().platform_output.commands.iter().any(|c| matches!(c, OutputCommand::CopyText(_))));
    assert_eq!(h.state().message.as_ref().map(|(_, text)| text.as_str()), Some("This action won't work on multiple selections."));
    assert_eq!(h.state().session.clipboard.as_ref().map(|c| c.areas.len()), Some(2), "the earlier copy stays");
}

#[test]
fn enter_pastes_the_copied_cells_and_ends_copy_mode() {
    // #845: with the marching ants showing, Enter pastes into the selection and cancels the copy.
    let mut h = harness(Services::default());
    h.input_mut().events.push(Event::Copy);
    h.step();
    h.state_mut().run("selection.set", json!({"cell": "D5"})).unwrap();
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let value = |r, c| h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(gridcraft_engine::core::CellRef::new(r, c))));
    assert_eq!(value(4, 3), Some(gridcraft_engine::core::Value::text("Name")));
    assert_eq!(value(5, 4), Some(gridcraft_engine::core::Value::number(42.0)));
    assert!(h.state().session.clipboard.is_none(), "the marching ants are gone");
    assert_eq!(h.state().session.active().map(|d| d.selection.active.a1()), Some("D5".to_string()), "Enter does not also move down");

    // Copy mode is over: the next Enter just moves.
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    assert_eq!(h.state().session.active().map(|d| d.selection.active.a1()), Some("D6".to_string()));
}

#[test]
fn clicking_the_ribbon_paste_button_or_its_menu_does_not_freeze() {
    // #273: a click on Paste deadlocked the UI thread (a click-state query inside the egui context
    // lock), so the frame never returned. Run the clicks on a worker and fail on a timeout instead of
    // hanging the test run if that comes back.
    let (done, finished) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let cell = value_at;
        let mut h = harness(Services::default());
        h.state_mut().run("edit.copy", json!({"range": "A1:B2"})).unwrap();
        h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
        // No host clipboard here (as on the web): GridCraft's own copy is pasted.
        h.get_by_role_and_label(Role::Button, "Paste").click();
        h.run_steps(3);
        assert_eq!(cell(&h, "D4"), Some(Value::from("Name")));
        assert_eq!(cell(&h, "E5"), Some(Value::number(42.0)));

        // The secondary click opens the paste options menu; its items paste too.
        h.state_mut().run("selection.set", json!({"cell": "G1"})).unwrap();
        h.get_by_role_and_label(Role::Button, "Paste").click_secondary();
        h.run_steps(3);
        h.get_by_label("Paste Values").click();
        h.run_steps(3);
        assert_eq!(cell(&h, "G1"), Some(Value::from("Name")));
        assert_eq!(cell(&h, "H2"), Some(Value::number(42.0)));
        let _ = done.send(());
    });
    match finished.recv_timeout(std::time::Duration::from_secs(60)) {
        Ok(()) => assert!(worker.join().is_ok()),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            if let Err(panic) = worker.join() {
                std::panic::resume_unwind(panic);
            }
        }
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => panic!("clicking Paste froze the UI thread"),
    }
}

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn value_at(h: &Harness, a1: &str) -> Option<Value> {
    let at = gridcraft_engine::core::CellRef::parse(a1).unwrap();
    h.state().session.active().and_then(|d| d.wb.active().map(|s| s.value(at)))
}

/// A host clipboard holding `text`, and how many times the app read it.
fn system_clipboard(text: &str) -> (Rc<RefCell<(String, usize)>>, Services) {
    let clipboard = Rc::new(RefCell::new((text.to_string(), 0)));
    let read = clipboard.clone();
    let services = Services {
        paste_text: Some(Box::new(move || {
            let mut clipboard = read.borrow_mut();
            clipboard.1 += 1;
            Some(clipboard.0.clone())
        })),
        ..Default::default()
    };
    (clipboard, services)
}

#[test]
fn ribbon_paste_pastes_the_system_clipboard_like_cmd_v() {
    // Text copied in another app: the Paste button pastes it, as Cmd/Ctrl+V does.
    let (clipboard, services) = system_clipboard("x\t1\r\ny\t2\r\n");
    let mut h = harness(services);
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    h.get_by_role_and_label(Role::Button, "Paste").click();
    h.run_steps(2);
    assert_eq!(clipboard.borrow().1, 1, "one clipboard read per click");
    assert_eq!(value_at(&h, "D4"), Some(Value::from("x")));
    assert_eq!(value_at(&h, "E4"), Some(Value::number(1.0)));
    assert_eq!(value_at(&h, "D5"), Some(Value::from("y")));
    assert_eq!(value_at(&h, "E5"), Some(Value::number(2.0)));
    assert!(h.state().message.is_none(), "{:?}", h.state().message);

    // The clipboard text is GridCraft's own copy: the copy is pasted with its formulas.
    h.state_mut().run("cell.set", json!({"cell": "A3", "input": "=B2*2"})).unwrap();
    let copied = h.state_mut().run("edit.copy", json!({"range": "A3"})).unwrap();
    clipboard.borrow_mut().0 = copied["text"].as_str().unwrap().to_string();
    h.state_mut().run("selection.set", json!({"cell": "B9"})).unwrap();
    h.get_by_role_and_label(Role::Button, "Paste").click();
    h.run_steps(2);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sh.cell(CellRef::parse("B9").unwrap()).unwrap().formula.as_ref().unwrap().text, "C8*2");

    // Nothing on the system clipboard (an image, say): GridCraft's own copy is pasted.
    clipboard.borrow_mut().0.clear();
    h.state_mut().run("selection.set", json!({"cell": "B12"})).unwrap();
    h.get_by_role_and_label(Role::Button, "Paste").click();
    h.run_steps(2);
    let sh = h.state().session.active().unwrap().wb.active().unwrap();
    assert_eq!(sh.cell(CellRef::parse("B12").unwrap()).unwrap().formula.as_ref().unwrap().text, "C11*2");
}

#[test]
fn paste_menu_items_paste_the_system_clipboard() {
    let (clipboard, services) = system_clipboard("from menu");
    let mut h = harness(services);
    let ribbon = h.get_by_role_and_label(Role::Button, "Paste").rect();
    let click_menu_paste = |h: &Harness| h.get_all_by_role_and_label(Role::Button, "Paste").find(|n| n.rect() != ribbon).unwrap().click();

    // Home › Paste options menu › Paste.
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    h.get_by_role_and_label(Role::Button, "Paste").click_secondary();
    h.run_steps(2);
    click_menu_paste(&h);
    h.run_steps(2);
    assert_eq!(value_at(&h, "D4"), Some(Value::from("from menu")));

    // The cell's context menu › Paste.
    clipboard.borrow_mut().0 = "from cell menu".into();
    h.state_mut().run("selection.set", json!({"cell": "F6"})).unwrap();
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = grid::Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let pos = geo.cell_rect(sh, CellRef::parse("F6").unwrap()).center();
    h.input_mut().events.push(Event::PointerMoved(pos));
    for pressed in [true, false] {
        h.input_mut().events.push(Event::PointerButton { pos, button: PointerButton::Secondary, pressed, modifiers: Modifiers::NONE });
        h.run_steps(2);
    }
    assert!(h.state().grid.context_menu.is_some());
    click_menu_paste(&h);
    h.run_steps(2);
    assert_eq!(value_at(&h, "F6"), Some(Value::from("from cell menu")));
    assert_eq!(clipboard.borrow().1, 2);
}

fn press(h: &mut Harness, key: egui::Key) {
    for pressed in [true, false] {
        h.input_mut().events.push(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
        if pressed && key != egui::Key::F10 {
            h.input_mut().events.push(Event::Text(key.name().to_ascii_lowercase()));
        }
    }
    h.run_steps(2);
}

#[test]
fn keytip_copy_and_cut_reach_the_system_clipboard_and_ribbon_paste_pastes_them() {
    // Home › Copy / Cut by keytip (Alt or F10, H, C / X) is the ribbon button: the host clipboard
    // gets the copy, and Paste pastes it rather than older text from another app.
    for (letter, cut) in [(egui::Key::C, false), (egui::Key::X, true)] {
        let (clipboard, mut services) = system_clipboard("stale text from earlier");
        let host = clipboard.clone();
        services.copy_html = Some(Box::new(move |_, text| {
            host.borrow_mut().0 = text.to_string();
            Ok(())
        }));
        let mut h = harness(services);
        for key in [egui::Key::F10, egui::Key::H, letter] {
            press(&mut h, key);
        }
        assert_eq!(h.state().session.clipboard.as_ref().map(|c| c.cut), Some(cut), "{letter:?}");
        assert_eq!(clipboard.borrow().0, "Name\tScore\nAda\t42\n", "{letter:?} publishes to the system clipboard");

        h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
        h.get_by_role_and_label(Role::Button, "Paste").click();
        h.run_steps(2);
        assert_eq!(value_at(&h, "D4"), Some(Value::from("Name")), "{letter:?}");
        assert_eq!(value_at(&h, "E5"), Some(Value::number(42.0)), "{letter:?}");
    }
}

#[test]
fn ribbon_paste_prefers_a_programmatic_copy_until_other_text_is_copied() {
    // A control-channel or MCP copy leaves the system clipboard alone. Its older text must not
    // win over the copy; text copied in another app afterwards must.
    let (clipboard, services) = system_clipboard("stale text from earlier");
    let mut h = harness(services);
    h.state_mut().run("edit.copy", json!({"range": "A1:B2"})).unwrap();
    assert_eq!(clipboard.borrow().0, "stale text from earlier", "the system clipboard is untouched");
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    h.get_by_role_and_label(Role::Button, "Paste").click();
    h.run_steps(2);
    assert_eq!(value_at(&h, "D4"), Some(Value::from("Name")));
    assert_eq!(value_at(&h, "E5"), Some(Value::number(42.0)));

    clipboard.borrow_mut().0 = "copied elsewhere\t7".into();
    h.state_mut().run("selection.set", json!({"cell": "G1"})).unwrap();
    h.get_by_role_and_label(Role::Button, "Paste").click();
    h.run_steps(2);
    assert_eq!(value_at(&h, "G1"), Some(Value::from("copied elsewhere")));
    assert_eq!(value_at(&h, "H1"), Some(Value::number(7.0)));
}

fn has_formula(h: &Harness, a1: &str) -> bool {
    let at = CellRef::parse(a1).unwrap();
    h.state().session.active().and_then(|d| d.wb.active().and_then(|s| s.cell(at)).map(|c| c.formula.is_some())).unwrap_or(false)
}

/// Close the "Nothing to paste." alert so the ribbon takes clicks again.
fn dismiss(h: &mut Harness) {
    h.state_mut().message = None;
    h.state_mut().toast = None;
    h.run_steps(2);
}

fn click_paste(h: &mut Harness) {
    h.get_by_role_and_label(Role::Button, "Paste").click();
    h.run_steps(2);
}

#[test]
fn a_pasted_ribbon_cut_is_not_pasted_again_from_the_system_clipboard() {
    // Excel pastes a cut once. After the move, the cut's text is still on the system clipboard:
    // a second Paste must not write it over the moved block (formulas would become values).
    let (clipboard, mut services) = system_clipboard("stale text from earlier");
    let host = clipboard.clone();
    services.copy_html = Some(Box::new(move |_, text| {
        host.borrow_mut().0 = text.to_string();
        Ok(())
    }));
    let mut h = harness(services);
    h.state_mut().run("cell.set", json!({"cell": "B3", "input": "=B2*2"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1:B3"})).unwrap();
    h.run_steps(2);
    for key in [egui::Key::F10, egui::Key::H, egui::Key::X] {
        press(&mut h, key);
    }
    assert_eq!(clipboard.borrow().0, "Name\tScore\nAda\t42\n\t84\n", "the cut is published");
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    click_paste(&mut h);
    assert_eq!(value_at(&h, "E6"), Some(Value::number(84.0)));
    assert!(has_formula(&h, "E6"), "the cut moves the formula");

    click_paste(&mut h);
    assert!(has_formula(&h, "E6"), "a second Paste leaves the moved formula alone");
    assert_eq!(value_at(&h, "D4"), Some(Value::from("Name")));
    assert!(h.state().toast.is_some() || h.state().message.is_some(), "Paste reports there is nothing to paste");

    // Text copied in another app afterwards still pastes.
    dismiss(&mut h);
    clipboard.borrow_mut().0 = "copied elsewhere\t7".into();
    h.state_mut().run("selection.set", json!({"cell": "H1"})).unwrap();
    click_paste(&mut h);
    assert_eq!(value_at(&h, "H1"), Some(Value::from("copied elsewhere")));
    assert_eq!(value_at(&h, "I1"), Some(Value::number(7.0)));
}

#[test]
fn a_pasted_programmatic_cut_does_not_paste_the_older_system_clipboard_text() {
    // A control-channel or MCP cut leaves the system clipboard alone; once the cut has been
    // pasted, the text that was there before it must not land on the moved data.
    let (clipboard, services) = system_clipboard("stale");
    let mut h = harness(services);
    h.state_mut().run("edit.cut", json!({"range": "A1:B2"})).unwrap();
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    click_paste(&mut h);
    assert_eq!(value_at(&h, "D4"), Some(Value::from("Name")));

    click_paste(&mut h);
    assert_eq!(value_at(&h, "D4"), Some(Value::from("Name")), "the moved data stays");
    assert_eq!(value_at(&h, "E5"), Some(Value::number(42.0)));

    dismiss(&mut h);
    clipboard.borrow_mut().0 = "copied elsewhere".into();
    click_paste(&mut h);
    assert_eq!(value_at(&h, "D4"), Some(Value::from("copied elsewhere")), "newer external text pastes");
}

#[test]
fn the_paste_keytip_pastes_the_system_clipboard_like_the_paste_button() {
    // Alt/F10, H, V, P is Home › Paste: it pastes text copied in another app.
    let (_clipboard, services) = system_clipboard("external\t7");
    let mut h = harness(services);
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    for key in [egui::Key::F10, egui::Key::H, egui::Key::V, egui::Key::P] {
        press(&mut h, key);
    }
    assert_eq!(value_at(&h, "D4"), Some(Value::from("external")));
    assert_eq!(value_at(&h, "E4"), Some(Value::number(7.0)));
    assert!(h.state().toast.is_none() && h.state().message.is_none());
}

#[test]
fn cmd_v_after_a_pasted_cut_does_not_paste_the_cut_text_again() {
    // Cmd/Ctrl+V hands the app the system clipboard's text, which still holds the cut after the
    // move. Excel pastes a cut once: the second Cmd/Ctrl+V must leave the moved formulas alone.
    let mut h = harness(Services::default());
    h.state_mut().run("cell.set", json!({"cell": "B3", "input": "=B2*2"})).unwrap();
    h.state_mut().run("selection.set", json!({"range": "A1:B3"})).unwrap();
    h.input_mut().events.push(Event::Cut);
    h.step();
    let text = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .expect("the cut is published");
    h.run_steps(2);
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    h.input_mut().events.push(Event::Paste(text.clone()));
    h.run_steps(2);
    assert!(has_formula(&h, "E6"), "the cut moves the formula");

    // The host may hand the text back with CRLF line endings.
    h.input_mut().events.push(Event::Paste(text.replace('\n', "\r\n")));
    h.run_steps(2);
    assert!(has_formula(&h, "E6"), "a second Cmd/Ctrl+V leaves the moved formula alone");
    assert_eq!(value_at(&h, "E6"), Some(Value::number(84.0)));
    assert_eq!(value_at(&h, "D4"), Some(Value::from("Name")));
    assert!(h.state().toast.is_some() || h.state().message.is_some(), "Paste reports there is nothing to paste");
}

#[test]
fn the_cut_text_pastes_again_once_the_clipboard_has_held_other_text() {
    // Once other text has been pasted, the clipboard has moved on from the cut: the same text
    // copied again later (from another app, or a cell editor) is a new copy and pastes.
    let mut h = harness(Services::default());
    h.state_mut().run("selection.set", json!({"cell": "A1"})).unwrap();
    h.input_mut().events.push(Event::Cut);
    h.step();
    let text = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::CopyText(text) => Some(text.clone()),
            _ => None,
        })
        .expect("the cut is published");
    h.run_steps(2);
    h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
    h.input_mut().events.push(Event::Paste(text.clone()));
    h.run_steps(2);
    assert_eq!(value_at(&h, "D4"), Some(Value::from("Name")), "the cut moves A1");
    h.state_mut().run("selection.set", json!({"cell": "G1"})).unwrap();
    h.input_mut().events.push(Event::Paste("other".into()));
    h.run_steps(2);
    assert_eq!(value_at(&h, "G1"), Some(Value::from("other")));
    h.state_mut().run("selection.set", json!({"cell": "H1"})).unwrap();
    h.input_mut().events.push(Event::Paste(text.replace('\n', "\r\n")));
    h.run_steps(2);
    assert_eq!(value_at(&h, "H1"), Some(Value::from("Name")), "the same text copied afresh pastes");
}
