//! After a left-drag of the fill handle, the Auto Fill Options button redoes the fill as Copy Cells,
//! Fill Series, Fill Formatting Only or Fill Without Formatting (#1403), and from a date as Fill
//! Days, Fill Weekdays, Fill Months or Fill Years.

use egui::{Event, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::{CellRef, Value};
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn harness(setup: impl FnOnce(&mut Session)) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    setup(&mut session);
    crate::support::stepped_harness(session)
}

/// B1:B2 holds 1, 2 and is selected.
fn one_two(s: &mut Session) {
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [2]]})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
}

fn corner(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.mirror_pos(geo.cell_rect(sheet, CellRef::parse(cell).unwrap()).right_bottom() - vec2(1.0, 1.0))
}

fn center(h: &Harness, cell: &str) -> Pos2 {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    geo.screen_cell_rect(sheet, CellRef::parse(cell).unwrap()).center()
}

fn press(h: &mut Harness, pos: Pos2, button: PointerButton) {
    h.input_mut().events.extend([Event::PointerMoved(pos), Event::PointerButton { pos, button, pressed: true, modifiers: Modifiers::NONE }]);
    h.step();
}

fn move_to(h: &mut Harness, from: Pos2, to: Pos2) {
    for i in 1..=10 {
        h.input_mut().events.push(Event::PointerMoved(from + (to - from) * (i as f32 / 10.0)));
        h.step();
    }
}

fn release(h: &mut Harness, pos: Pos2, button: PointerButton) {
    h.input_mut().events.push(Event::PointerButton { pos, button, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// Drags the handle of the current selection (whose bottom-right cell is `from`) to `to`.
fn drag_handle(h: &mut Harness, from: &str, to: &str, button: PointerButton) {
    let (start, end) = (corner(h, from), center(h, to));
    press(h, start, button);
    move_to(h, start, end);
    release(h, end, button);
}

fn value(h: &Harness, cell: &str) -> Value {
    h.state().session.active().unwrap().wb.active().unwrap().value(CellRef::parse(cell).unwrap())
}

fn values(h: &Harness, cells: &[&str]) -> Vec<Value> {
    cells.iter().map(|c| value(h, c)).collect()
}

fn bold(h: &Harness, cell: &str) -> bool {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).font.bold
}

fn italic(h: &Harness, cell: &str) -> bool {
    let d = h.state().session.active().unwrap();
    let sh = d.wb.active().unwrap();
    d.wb.styles.get(sh.style_id(CellRef::parse(cell).unwrap())).font.italic
}

fn selection(h: &Harness) -> String {
    h.state().session.active().unwrap().selection.a1()
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn collapsed_refedit() -> Harness {
    let mut h = harness(one_two);
    h.state_mut().open_dialog("goalSeek", json!({}));
    h.run_steps(4);
    h.get_all_by_label("Collapse dialog").nth(1).unwrap().click();
    h.run_steps(3);
    assert_eq!(h.state().dialog.as_ref().unwrap().collapsed(), Some("changing"));
    h
}

#[test]
fn collapsed_refedit_arrows_keep_the_fill_source_until_release() {
    for button in [PointerButton::Primary, PointerButton::Secondary] {
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::COMMAND] {
            let mut h = collapsed_refedit();
            let before = h.state().session.active().unwrap();
            let (workbook, revision, undo) = (before.wb.clone(), before.revision, before.undo.len());
            let reference = h.state().dialog.as_ref().unwrap().values["changing"].clone();
            let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
            press(&mut h, start, button);
            move_to(&mut h, start, end);
            assert!(matches!(h.state().grid.drag, gridcraft_ui_egui::grid::Drag::Fill { .. }));
            h.input_mut().events.extend([
                Event::ModifiersChanged(modifiers),
                Event::Key { key: egui::Key::ArrowDown, physical_key: None, pressed: true, repeat: false, modifiers },
                Event::Key { key: egui::Key::ArrowDown, physical_key: None, pressed: false, repeat: false, modifiers },
            ]);
            h.run_steps(3);
            assert_eq!(selection(&h), "B1:B2", "{button:?}, {modifiers:?}: the fill keeps its source");
            assert_eq!(h.state().dialog.as_ref().unwrap().values["changing"], reference);
            let during = h.state().session.active().unwrap();
            assert!(std::sync::Arc::ptr_eq(&workbook, &during.wb));
            assert_eq!((during.revision, during.undo.len()), (revision, undo));
            if button == PointerButton::Primary {
                let label = if modifiers.command { "2" } else { "6" };
                assert!(h.query_by_label(label).is_some(), "the preview still uses B1:B2");
                if modifiers.is_none()
                    && let Ok(path) = std::env::var("GRIDCRAFT_FILL_REFEDIT_SCREENSHOT")
                {
                    h.render().unwrap().save(path).unwrap();
                }
            }
            h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
            release(&mut h, end, button);
            if button == PointerButton::Secondary {
                let (source, target, _) = h.state().grid.fill_menu.unwrap();
                assert_eq!((source.a1(), target.a1()), ("B1:B2".into(), "B1:B6".into()));
                h.get_by_label("Fill Series").click();
                h.run_steps(3);
            }
            assert_eq!(values(&h, &["B3", "B4", "B5", "B6"]), [n(3.0), n(4.0), n(5.0), n(6.0)]);
            assert_eq!(h.state().session.active().unwrap().undo.len(), undo + 1);
            assert_eq!(h.state().dialog.as_ref().unwrap().values["changing"], "$B$1:$B$6");
            assert_eq!(h.state().dialog.as_ref().unwrap().collapsed(), Some("changing"));
            h.key_press(egui::Key::ArrowDown);
            h.run_steps(3);
            assert_ne!(selection(&h), "B1:B6", "the picker gets its arrows back after release");
        }
    }
}

#[test]
fn collapsed_refedit_escape_cancels_the_fill_before_expanding_the_picker() {
    for button in [PointerButton::Primary, PointerButton::Secondary] {
        let mut h = collapsed_refedit();
        let before = h.state().session.active().unwrap();
        let (workbook, revision, undo) = (before.wb.clone(), before.revision, before.undo.len());
        let reference = h.state().dialog.as_ref().unwrap().values["changing"].clone();
        let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
        press(&mut h, start, button);
        move_to(&mut h, start, end);
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        release(&mut h, end, button);
        assert_eq!(h.state().grid.drag, gridcraft_ui_egui::grid::Drag::None);
        assert!(h.state().grid.fill_menu.is_none());
        assert_eq!(selection(&h), "B1:B2");
        let after = h.state().session.active().unwrap();
        assert!(std::sync::Arc::ptr_eq(&workbook, &after.wb));
        assert_eq!((after.revision, after.undo.len()), (revision, undo));
        assert_eq!(h.state().dialog.as_ref().unwrap().values["changing"], reference);
        assert_eq!(h.state().dialog.as_ref().unwrap().collapsed(), Some("changing"));
        h.key_press(egui::Key::Escape);
        h.run_steps(3);
        assert_eq!(h.state().dialog.as_ref().unwrap().collapsed(), None, "a fresh Escape expands the picker");
    }
}

#[test]
fn fill_drag_previews_endpoint_without_changing_the_workbook() {
    let mut h = harness(|s| {
        s.execute("range.setValues", json!({"range": "B1", "values": [[1], [2], [3]]})).unwrap();
        s.execute("selection.set", json!({"range": "B1:B3"})).unwrap();
    });
    let before = h.state().session.active().unwrap();
    let (undo_before, revision_before, dirty_before) = (before.undo.len(), before.revision, before.is_dirty());
    let workbook_before = before.wb.clone();
    let (start, end) = (corner(&h, "B3"), center(&h, "B6"));
    press(&mut h, start, PointerButton::Primary);
    move_to(&mut h, start, end);
    h.run_steps(3);

    assert_eq!(values(&h, &["B4", "B5", "B6"]), [Value::Empty, Value::Empty, Value::Empty]);
    assert_eq!(h.state().session.active().unwrap().undo.len(), undo_before);
    let during = h.state().session.active().unwrap();
    assert_eq!((during.revision, during.is_dirty()), (revision_before, dirty_before));
    assert!(std::sync::Arc::ptr_eq(&workbook_before, &during.wb));
    if let Ok(path) = std::env::var("GRIDCRAFT_FILL_PREVIEW_SCREENSHOT") {
        h.render().unwrap().save(path).unwrap();
    }
    assert!(h.query_by_label("6").is_some(), "the tooltip predicts the value at the drag endpoint");

    release(&mut h, end, PointerButton::Primary);
    assert_eq!(value(&h, "B6"), n(6.0));
    assert!(h.query_by_label("6").is_none(), "the tooltip disappears on release");
    assert!(h.query_by_label("Auto Fill Options").is_some());
    assert_eq!(h.state().session.active().unwrap().undo.len(), undo_before + 1);
}

#[test]
fn edge_auto_scroll_release_keeps_the_last_preview_target() {
    let mut failures = Vec::new();
    for (horizontal, rtl, zoom) in [
        (false, false, 100),
        (true, false, 100),
        (false, true, 100),
        (true, true, 100),
        (false, false, 150),
        (true, false, 150),
        (false, true, 150),
        (true, true, 150),
    ] {
        let mut h = harness(|s| {
            if horizontal {
                s.execute("range.setValues", json!({"range": "B2", "values": [[1, 2]]})).unwrap();
                s.execute("selection.set", json!({"range": "B2:C2"})).unwrap();
            } else {
                one_two(s);
            }
            s.execute("view.zoom", json!({"percent": zoom})).unwrap();
            std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb).sheet_mut(0).unwrap().right_to_left = rtl;
        });
        let undo = h.state().session.active().unwrap().undo.len();
        let start = corner(&h, if horizontal { "C2" } else { "B2" });
        let geo = h.state().grid.geo.as_ref().unwrap();
        let mut end = geo.mirror_pos(center(&h, if horizontal { "C2" } else { "B2" }));
        if horizontal {
            end.x = geo.cells.right() - 3.0;
        } else {
            end.y = geo.cells.bottom() - 3.0;
        }
        let end = geo.mirror_pos(end);
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        let gridcraft_ui_egui::grid::Drag::Fill { target: Some(target), .. } = h.state().grid.drag else { panic!("expected a fill drag") };
        let expected = if horizontal { target.end.col } else { target.end.row + 1 };
        assert!(h.query_by_label(&expected.to_string()).is_some(), "the endpoint is visible in the actual tooltip");
        if !horizontal
            && !rtl
            && zoom == 100
            && let Ok(path) = std::env::var("GRIDCRAFT_FILL_EDGE_SCREENSHOT")
        {
            h.render().unwrap().save(path).unwrap();
        }
        let beyond = if horizontal { target.end.offset_clamped(0, 1) } else { target.end.offset_clamped(1, 0) };
        assert_eq!(value(&h, &beyond.a1()), Value::Empty);
        release(&mut h, end, PointerButton::Primary);
        let actual = selection(&h);
        if actual != target.a1() || value(&h, &beyond.a1()) != Value::Empty {
            failures.push(format!(
                "horizontal={horizontal}, rtl={rtl}, zoom={zoom}: preview={}, committed={actual}, unseen={} ({:?})",
                target.a1(),
                beyond.a1(),
                value(&h, &beyond.a1())
            ));
        }
        assert_eq!(h.state().session.active().unwrap().undo.len(), undo + 1);
    }
    assert!(failures.is_empty(), "release filled beyond the preview: {failures:?}");
}

#[test]
fn edge_scroll_does_not_cross_sheets_before_an_immediate_release() {
    for rtl in [false, true] {
        let mut h = harness(|s| {
            one_two(s);
            s.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
            s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
            s.execute("range.setValues", json!({"range": "B1", "values": [[10], [20]]})).unwrap();
            s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
            s.execute("view.zoom", json!({"percent": 200})).unwrap();
            s.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
            s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
            s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
        });
        h.ctx.set_os(egui::os::OperatingSystem::Windows);
        let start = corner(&h, "B2");
        let geo = h.state().grid.geo.as_ref().unwrap();
        let mut end = geo.mirror_pos(center(&h, "B2"));
        end.y = geo.cells.bottom() - 3.0;
        let end = geo.mirror_pos(end);
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        h.input_mut().events.extend([
            Event::ModifiersChanged(Modifiers::COMMAND),
            Event::Key { key: egui::Key::PageDown, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND },
        ]);
        h.step();
        let app = h.state();
        let doc = app.session.active().unwrap();
        assert_eq!(doc.wb.active_sheet(), 1);
        assert_eq!(app.view().scroll, egui::Vec2::ZERO, "the destination sheet starts unscrolled");
        let sh = doc.wb.active().unwrap();
        let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
        let at = geo.cell_at(sh, geo.mirror_pos(end));
        let expected = format!("B1:B{}", at.row + 1);
        h.input_mut().events.extend([
            Event::ModifiersChanged(Modifiers::NONE),
            Event::Key { key: egui::Key::PageDown, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
        ]);
        release(&mut h, end, PointerButton::Primary);
        assert_eq!(selection(&h), expected, "release maps the current sheet without the previous sheet's queued scroll");
        assert_eq!(value(&h, &format!("B{}", at.row + 1)), n(f64::from(at.row + 1) * 10.0));
        assert_eq!(value(&h, &format!("B{}", at.row + 2)), Value::Empty);
        assert_eq!(h.state().session.active().unwrap().wb.sheet(0).unwrap().value(CellRef::parse("B3").unwrap()), Value::Empty);
    }
}

#[test]
fn fill_handle_takes_focus_from_the_font_size_box() {
    let mut failures = Vec::new();
    for key in [egui::Key::ArrowLeft, egui::Key::ArrowRight] {
        let mut h = harness(one_two);
        h.key_press(egui::Key::F10);
        h.run_steps(2);
        for keytip in [egui::Key::H, egui::Key::F, egui::Key::S] {
            h.input_mut().events.extend([
                Event::Key { key: keytip, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
                Event::Text(keytip.name().to_ascii_lowercase()),
            ]);
            h.run_steps(2);
        }
        h.run_steps(3);
        let field = egui::Id::unique("gridcraft.font_size_box");
        assert_eq!(h.ctx.memory(|m| m.focused()), Some(field), "the real Font Size keytip focuses its text box");
        let undo = h.state().session.active().unwrap().undo.len();
        let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        h.run_steps(3);
        let grid_focused = h.ctx.memory(|m| m.focused()).is_some_and(|id| id != field);
        // Losing focus can itself collapse the selection; only the later arrow must leave it alone.
        let cursor = egui::TextEdit::load_state(&h.ctx, field).unwrap().cursor.char_range();
        h.key_press(key);
        h.run_steps(2);
        let cursor_after = egui::TextEdit::load_state(&h.ctx, field).unwrap().cursor.char_range();
        let source = selection(&h);
        let preview = h.query_by_label("6").is_some();
        if key == egui::Key::ArrowLeft
            && let Ok(path) = std::env::var("GRIDCRAFT_FILL_FOCUS_SCREENSHOT")
        {
            h.render().unwrap().save(path).unwrap();
        }
        release(&mut h, end, PointerButton::Primary);
        let committed = values(&h, &["B3", "B4", "B5", "B6"]) == [n(3.0), n(4.0), n(5.0), n(6.0)];
        let once = h.state().session.active().unwrap().undo.len() == undo + 1;
        if !grid_focused || cursor_after != cursor || source != "B1:B2" || !preview || !committed || !once {
            failures.push(format!("{key:?}: grid_focused={grid_focused}, cursor={cursor:?} -> {cursor_after:?}, source={source}, preview={preview}, committed={committed}, once={once}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn arrows_keep_the_fill_source_and_preview_until_release() {
    let mut failures = Vec::new();
    for key in [egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::ArrowLeft, egui::Key::ArrowRight] {
        for modifiers in [
            Modifiers::NONE,
            Modifiers::SHIFT,
            Modifiers { ctrl: true, command: true, ..Modifiers::NONE },
            Modifiers { ctrl: true, command: true, shift: true, ..Modifiers::NONE },
        ] {
            let mut h = harness(one_two);
            let before = h.state().session.active().unwrap();
            let (workbook, revision, undo) = (before.wb.clone(), before.revision, before.undo.len());
            let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
            press(&mut h, start, PointerButton::Primary);
            move_to(&mut h, start, end);
            h.run_steps(3);
            assert!(h.query_by_label("6").is_some());
            h.input_mut()
                .events
                .extend([Event::ModifiersChanged(modifiers), Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
            h.step();
            let source = selection(&h);
            let label = if modifiers.command { "2" } else { "6" };
            let preview = h.query_by_label(label).is_some();
            let d = h.state().session.active().unwrap();
            let unchanged = std::sync::Arc::ptr_eq(&workbook, &d.wb) && d.revision == revision && d.undo.len() == undo;
            if key == egui::Key::ArrowDown
                && modifiers.is_none()
                && let Ok(path) = std::env::var("GRIDCRAFT_FILL_ARROW_SCREENSHOT")
            {
                h.render().unwrap().save(path).unwrap();
            }
            // Release without another pointer move: the target must still belong to B1:B2.
            h.input_mut().events.extend([
                Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
                Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers },
            ]);
            h.run_steps(3);
            let expected = if modifiers.command { [n(1.0), n(2.0), n(1.0), n(2.0)] } else { [n(3.0), n(4.0), n(5.0), n(6.0)] };
            let committed = values(&h, &["B3", "B4", "B5", "B6"]) == expected;
            let once = h.state().session.active().unwrap().undo.len() == undo + 1;
            if source != "B1:B2" || !preview || !unchanged || !committed || !once || h.state().message.is_some() {
                failures.push(format!(
                    "{key:?} {modifiers:?}: source={source}, preview={preview}, unchanged={unchanged}, committed={committed}, once={once}"
                ));
            } else {
                h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
                h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
                h.run_steps(2);
                h.key_press(egui::Key::ArrowRight);
                h.run_steps(2);
                assert_eq!(selection(&h), "E4", "ordinary arrows work after release");
            }
        }
    }
    assert!(failures.is_empty(), "arrows changed an active fill's source: {failures:?}");
}

#[test]
fn other_navigation_preserves_the_fill_source_until_release() {
    use egui::Key;
    let command = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
    let combined = Modifiers { shift: true, ..command };
    let mut cases = Vec::new();
    for key in [Key::Home, Key::End] {
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT, command, combined] {
            cases.push(("B1:B2", key, modifiers));
        }
    }
    for key in [Key::PageUp, Key::PageDown] {
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::ALT, Modifiers { alt: true, ..Modifiers::SHIFT }] {
            cases.push(("B1:B2", key, modifiers));
        }
    }
    for key in [Key::Tab, Key::Enter] {
        for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
            cases.push(("B2", key, modifiers));
        }
    }
    cases.extend([
        ("B1:B2", Key::A, command),
        ("B1:B2", Key::A, combined),
        ("B1:B2", Key::Space, Modifiers::SHIFT),
        ("B1:B2", Key::Space, command),
        ("B1:B2", Key::Space, combined),
        ("B2", Key::Num8, combined),
    ]);
    let mut failures = Vec::new();
    for (source, key, modifiers) in cases {
        let mut h = harness(|s| {
            one_two(s);
            s.execute("selection.set", json!({"range": source})).unwrap();
        });
        let undo = h.state().session.active().unwrap().undo.len();
        let series = if source == "B2" { modifiers.command } else { !modifiers.command };
        let want = if series { "6" } else { "2" };
        let labels_before = h.query_all_by_label(want).count();
        let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        h.input_mut()
            .events
            .extend([Event::ModifiersChanged(modifiers), Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }]);
        h.step();
        let actual_source = selection(&h);
        let preview = h.query_all_by_label(want).count() > labels_before;
        h.input_mut().events.extend([
            Event::Key { key, physical_key: Some(key), pressed: false, repeat: false, modifiers },
            Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers },
        ]);
        h.run_steps(3);
        let expected = if series {
            [n(3.0), n(4.0), n(5.0), n(6.0)]
        } else if source == "B2" {
            [n(2.0), n(2.0), n(2.0), n(2.0)]
        } else {
            [n(1.0), n(2.0), n(1.0), n(2.0)]
        };
        let committed = values(&h, &["B3", "B4", "B5", "B6"]) == expected;
        let once = h.state().session.active().unwrap().undo.len() == undo + 1;
        if actual_source != source || !preview || !committed || !once || h.state().message.is_some() {
            failures.push(format!("{source} {key:?} {modifiers:?}: source={actual_source}, preview={preview}, committed={committed}, once={once}"));
        } else {
            h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
            h.state_mut().run("selection.set", json!({"cell": "D4"})).unwrap();
            h.run_steps(2);
            h.key_press(Key::Home);
            h.run_steps(2);
            assert_eq!(selection(&h), "A4", "Home works normally after the fill ends");
        }
    }
    assert!(failures.is_empty(), "same-sheet navigation changed the fill source: {failures:?}");
}

#[test]
fn escape_cancels_the_fill_preview_without_committing() {
    let mut h = harness(one_two);
    let before = h.state().session.active().unwrap().wb.clone();
    let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
    press(&mut h, start, PointerButton::Primary);
    move_to(&mut h, start, end);
    h.run_steps(3);
    assert!(h.query_by_label("6").is_some());
    h.input_mut().events.push(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    release(&mut h, end, PointerButton::Primary);
    assert!(h.query_by_label("6").is_none());
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.active().unwrap().wb));
    assert!(h.query_by_label("Auto Fill Options").is_none());
}

#[test]
fn fill_preview_does_not_record_macro_steps_before_release() {
    let mut h = harness(one_two);
    h.state_mut().run("macro.record", json!({"name": "FillPreview", "relative": false})).unwrap();
    h.run_steps(2);
    let before = h.state().session.recorder.as_ref().unwrap().steps.clone();
    let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
    press(&mut h, start, PointerButton::Primary);
    move_to(&mut h, start, end);
    h.run_steps(3);
    assert!(h.query_by_label("6").is_some());
    assert_eq!(h.state().session.recorder.as_ref().unwrap().steps, before, "preview frames must not become recorded commands");
    release(&mut h, end, PointerButton::Primary);
    let steps = &h.state().session.recorder.as_ref().unwrap().steps;
    assert_eq!(steps.len(), before.len() + 1, "only the committed fill is recorded");
    let step = steps.last().unwrap();
    assert_eq!(step.id, "edit.autoFill");
    assert_eq!(step.params["source"], "B1:B2");
    assert_eq!(step.params["target"], "B1:B6");
    assert_eq!(value(&h, "B6"), n(6.0));
}

#[test]
fn escape_also_cancels_a_copy_modifier_drag() {
    let mut h = harness(one_two);
    let before = h.state().session.active().unwrap().wb.clone();
    let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
    press(&mut h, start, PointerButton::Primary);
    move_to(&mut h, start, end);
    let modifiers = Modifiers::COMMAND;
    h.input_mut().events.push(Event::ModifiersChanged(modifiers));
    h.run_steps(3);
    assert!(h.query_by_label("2").is_some());
    h.input_mut().events.push(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers });
    h.run_steps(3);
    release(&mut h, end, PointerButton::Primary);
    assert!(h.query_by_label("2").is_none());
    assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.active().unwrap().wb));
}

#[test]
fn escape_cancels_fill_when_the_copy_modifier_is_released_in_the_same_batch() {
    for modifiers in [Modifiers::CTRL, Modifiers::COMMAND] {
        let mut h = harness(one_two);
        let before = h.state().session.active().unwrap().wb.clone();
        let undo_before = h.state().session.active().unwrap().undo.len();
        let (start, end) = (corner(&h, "B2"), center(&h, "B6"));
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        h.input_mut().events.push(Event::ModifiersChanged(modifiers));
        h.run_steps(2);
        h.input_mut().events.push(Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers });
        h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
        h.run_steps(3);
        release(&mut h, end, PointerButton::Primary);
        assert!(std::sync::Arc::ptr_eq(&before, &h.state().session.active().unwrap().wb), "Escape cancels even after {modifiers:?} is released");
        assert_eq!(h.state().session.active().unwrap().undo.len(), undo_before);
        assert!(h.query_by_label("Auto Fill Options").is_none());
    }
}

#[test]
fn fill_preview_uses_the_visible_lane_when_the_pointer_is_outside_the_viewport() {
    let mut h = harness(|s| {
        s.execute("range.setValues", json!({"range": "B1", "values": [[1, 10, 100], [2, 20, 200]]})).unwrap();
        s.execute("selection.set", json!({"range": "B1:D2"})).unwrap();
    });
    // Keep the left source lanes offscreen while leaving the fill handle visible.
    h.state_mut().view_mut().unwrap().scroll.x = 130.0;
    h.run_steps(3);
    let (start, end) = (corner(&h, "D2"), center(&h, "C6"));
    press(&mut h, start, PointerButton::Primary);
    move_to(&mut h, start, end);
    let app = h.state();
    let sh = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
    let outside = egui::pos2(geo.cells.left() - 200.0, end.y);
    h.input_mut().events.push(Event::PointerMoved(outside));
    h.step();
    assert!(h.query_by_label("60").is_some(), "the preview uses visible column C, not offscreen B");
    assert!(h.query_by_label("6").is_none());
}

#[test]
fn fill_preview_follows_direction_format_and_copy_modifier() {
    for (source, input, from, to, expected, alternate, rtl) in [
        ("B4:B6", json!([[1], [2], [3]]), "B6", "B1", "-2", false, false),
        ("B2:D2", json!([[1, 2, 3]]), "D2", "G2", "6", false, false),
        ("E2:G2", json!([[1, 2, 3]]), "G2", "B2", "-2", false, false),
        ("B1:B3", json!([[1], [2], [3]]), "B3", "B5", "2", true, false),
        ("B1", json!([[5]]), "B1", "B4", "8", true, false),
        ("B1", json!([["1/31/2026"]]), "B1", "B4", "2/3/2026", false, false),
        ("B4", json!([["1/31/2026"]]), "B4", "B1", "1/28/2026", false, false),
        ("B2", json!([["1/31/2026"]]), "B2", "E2", "2/3/2026", false, false),
        ("E2", json!([["1/31/2026"]]), "E2", "B2", "1/28/2026", false, false),
        ("B1", json!([["Item 007"]]), "B1", "B4", "Item 010", false, false),
        ("B4", json!([["Item 007"]]), "B4", "B1", "Item 004", false, false),
        ("B2", json!([["Item 007"]]), "B2", "E2", "Item 010", false, false),
        ("E2", json!([["Item 007"]]), "E2", "B2", "Item 004", false, false),
        ("B1", json!([["=A1+1"]]), "B1", "B4", "=A4+1", false, false),
        ("B4", json!([["=A4+1"]]), "B4", "B1", "=A1+1", false, false),
        ("B2", json!([["=A2+1"]]), "B2", "E2", "=D2+1", false, false),
        ("E2", json!([["=D2+1"]]), "E2", "B2", "=A2+1", false, false),
        ("B1:B3", json!([[1], [2], [3]]), "B3", "B6", "6", false, true),
    ] {
        let mut h = harness(|s| {
            let start = source.split(':').next().unwrap();
            s.execute("range.setValues", json!({"range": start, "values": input})).unwrap();
            s.execute("selection.set", json!({"range": source})).unwrap();
            if rtl {
                s.execute("view.rightToLeft", json!({"on": true})).unwrap();
            }
        });
        let (start, end) = (corner(&h, from), center(&h, to));
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        // Change the modifier during the drag; the tooltip must use the release mode.
        let modifiers = Modifiers { command: alternate, ctrl: alternate, ..Modifiers::NONE };
        h.input_mut().events.push(Event::ModifiersChanged(modifiers));
        h.run_steps(3);
        assert!(h.query_by_label(expected).is_some(), "{source} -> {to}, alternate={alternate}, rtl={rtl}");
        assert_eq!(value(&h, to), Value::Empty);
        h.input_mut().events.push(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers });
        h.run_steps(3);
        assert!(h.query_by_label(expected).is_none(), "preview gone after release");
        let d = h.state().session.active().unwrap();
        let sh = d.wb.active().unwrap();
        let cell = CellRef::parse(to).unwrap();
        let actual = if sh.cell(cell).is_some_and(|c| c.formula.is_some()) {
            sh.input_text(cell)
        } else {
            gridcraft_engine::display::cell_text(&d.wb, sh, cell)
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn fill_preview_copies_a_python_result_without_showing_its_code() {
    use std::sync::Arc;
    const CODE: &str = "secret_python = 7\nsecret_python";
    for (protected, command) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut h = harness(|s| {
            s.execute("python.set", json!({"cell": "B1", "code": CODE})).unwrap();
            s.execute("home.formatCells", json!({"range": "B1", "style": {"protection": {"hidden": true}}})).unwrap();
            s.execute("home.formatCells", json!({"range": "B2:B4", "style": {"protection": {"locked": false}}})).unwrap();
            s.execute("selection.set", json!({"cell": "B1"})).unwrap();
            if protected {
                s.execute("review.protectSheet", json!({})).unwrap();
            }
            // The loaded workbook has a kept Python result; this test executes no Python.
            let doc = s.doc_mut().unwrap();
            let mut wb = (*doc.wb).clone();
            wb.sheet_mut(0).unwrap().cells.get_mut(CellRef::parse("B1").unwrap()).unwrap().value = n(7.0);
            doc.wb = Arc::new(wb);
        });
        h.state_mut().ui.status_bar = false;
        h.run_steps(2);
        let code_labels = h.query_all_by_label(CODE).count();
        let result_labels = h.query_all_by_label("7").count();
        assert_eq!(code_labels, usize::from(!protected), "only the unprotected formula bar shows source before the drag");
        let doc = h.state().session.doc().unwrap();
        let (wb, revision, undo) = (doc.wb.clone(), doc.revision, doc.undo.len());
        let (start, end) = (corner(&h, "B1"), center(&h, "B4"));
        press(&mut h, start, PointerButton::Primary);
        let modifiers = Modifiers { ctrl: command, command, ..Modifiers::NONE };
        h.input_mut().events.push(Event::ModifiersChanged(modifiers));
        move_to(&mut h, start, end);
        h.run_steps(3);
        assert_eq!(
            h.query_all_by_label("7").count(),
            result_labels + 1,
            "protected={protected}, command={command}: the tooltip adds the cached result"
        );
        assert_eq!(h.query_all_by_label(CODE).count(), code_labels, "the preview adds no source label");
        let doc = h.state().session.doc().unwrap();
        assert!(Arc::ptr_eq(&wb, &doc.wb));
        assert_eq!((doc.revision, doc.undo.len()), (revision, undo));
        assert_eq!(value(&h, "B4"), Value::Empty);
        if protected
            && command
            && let Ok(path) = std::env::var("GRIDCRAFT_FILL_PYTHON_SCREENSHOT")
        {
            h.render().unwrap().save(path).unwrap();
        }
        h.input_mut().events.push(Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers });
        h.run_steps(3);
        let doc = h.state().session.doc().unwrap();
        assert_eq!(doc.wb.active().unwrap().cell(CellRef::parse("B4").unwrap()).unwrap().py.as_ref().unwrap().code, CODE);
        assert_eq!(value(&h, "B4"), n(7.0));
        assert_eq!(doc.undo.len(), undo + 1);
    }
}

#[test]
fn fill_preview_keeps_protected_formulas_hidden() {
    for (protected, hidden, target_unlocked) in [(true, true, true), (true, true, false), (false, true, true), (true, false, true)] {
        let mut h = harness(|s| {
            s.execute("cell.set", json!({"cell": "B1", "input": "=A1*1.17"})).unwrap();
            s.execute("home.formatCells", json!({"range": "B1", "style": {"protection": {"hidden": hidden}}})).unwrap();
            if target_unlocked {
                s.execute("home.formatCells", json!({"range": "B2:B4", "style": {"protection": {"locked": false}}})).unwrap();
            }
            s.execute("selection.set", json!({"cell": "B1"})).unwrap();
            if protected {
                s.execute("review.protectSheet", json!({})).unwrap();
            }
        });
        let (start, end) = (corner(&h, "B1"), center(&h, "B4"));
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        h.run_steps(3);
        assert_eq!(
            h.query_by_label("=A4*1.17").is_some(),
            !(protected && hidden),
            "protected={protected}, hidden={hidden}, target_unlocked={target_unlocked}"
        );
        if !protected || target_unlocked {
            release(&mut h, end, PointerButton::Primary);
            let d = h.state().session.active().unwrap();
            let sh = d.wb.active().unwrap();
            assert_eq!(sh.input_text(CellRef::parse("B4").unwrap()), "=A4*1.17");
            assert_eq!(gridcraft_engine::display::formula_hidden(&d.wb, sh, CellRef::parse("B4").unwrap()), protected && hidden);
        }
    }
}

#[test]
fn formula_fill_preview_uses_the_selected_formula_language() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "B1", "input": "=SUM(A1,1.5)"})).unwrap();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    });
    h.state_mut().run("app.language.set", json!({"language": "es"})).unwrap();
    h.run_steps(3);
    let (start, end) = (corner(&h, "B1"), center(&h, "B4"));
    press(&mut h, start, PointerButton::Primary);
    move_to(&mut h, start, end);
    h.run_steps(3);
    assert!(h.query_by_label("=SUMA(A4;1,5)").is_some());
    release(&mut h, end, PointerButton::Primary);
    assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().input_text(CellRef::parse("B4").unwrap()), "=SUM(A4,1.5)");
}

#[test]
fn immediate_fill_release_after_sheet_navigation_uses_the_current_pointer_range() {
    let mut failures = Vec::new();
    for (zoom, rtl, os, key, modifiers) in [
        (50, false, egui::os::OperatingSystem::Windows, egui::Key::PageDown, Modifiers::COMMAND),
        (200, false, egui::os::OperatingSystem::Windows, egui::Key::PageDown, Modifiers::COMMAND),
        (50, true, egui::os::OperatingSystem::Windows, egui::Key::PageDown, Modifiers::COMMAND),
        (200, true, egui::os::OperatingSystem::Windows, egui::Key::PageDown, Modifiers::COMMAND),
        (50, false, egui::os::OperatingSystem::Mac, egui::Key::ArrowRight, Modifiers::ALT),
        (200, true, egui::os::OperatingSystem::Mac, egui::Key::ArrowRight, Modifiers::ALT),
    ] {
        let mut h = harness(|s| {
            s.execute("cell.set", json!({"cell": "A1", "input": "=C1*2.71"})).unwrap();
            s.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
            s.execute("home.insertSheet", json!({"name": "Other"})).unwrap();
            s.execute("cell.set", json!({"cell": "A1", "input": "=C1*1.17"})).unwrap();
            s.execute("view.zoom", json!({"percent": zoom})).unwrap();
            s.execute("view.rightToLeft", json!({"on": rtl})).unwrap();
            s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
            s.execute("selection.set", json!({"cell": "A1"})).unwrap();
        });
        h.ctx.set_os(os);
        let (start, end) = (corner(&h, "A1"), center(&h, "A4"));
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        h.input_mut()
            .events
            .extend([Event::ModifiersChanged(modifiers), Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        h.step();
        if h.state().session.active().unwrap().wb.active_sheet() != 1 {
            failures.push(format!("zoom={zoom} rtl={rtl} {os:?} {key:?}: sheet navigation was blocked"));
            continue;
        }
        let app = h.state();
        let d = app.session.active().unwrap();
        let sh = d.wb.active().unwrap();
        let geo = Geo::new(sh, app.grid.rect.unwrap(), app.view().scroll);
        let at = geo.cell_at(sh, geo.mirror_pos(end));
        assert_ne!(at.row, 3, "fixture maps the unchanged pointer to a different row after navigation");
        let undo = d.undo.len();
        // No pointer move or idle pass on the destination sheet before this release.
        h.input_mut().events.extend([
            Event::ModifiersChanged(Modifiers::NONE),
            Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE },
            Event::PointerButton { pos: end, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE },
        ]);
        h.run_steps(3);
        let d = h.state().session.active().unwrap();
        let sh = d.wb.active().unwrap();
        let last = CellRef::new(at.row, 0);
        let correct = sh.input_text(last) == format!("=C{}*1.17", at.row + 1);
        let bounded = sh.value(CellRef::new(at.row + 1, 0)) == Value::Empty;
        let original_untouched = d.wb.sheet(0).unwrap().value(CellRef::parse("A4").unwrap()) == Value::Empty;
        if !correct || !bounded || !original_untouched || d.undo.len() != undo + 1 || h.state().message.is_some() {
            failures.push(format!(
                "zoom={zoom} rtl={rtl}: pointer={at:?}, correct={correct}, bounded={bounded}, original_untouched={original_untouched}"
            ));
        }
    }
    assert!(failures.is_empty(), "release used the previous sheet's target: {failures:?}");
}

#[test]
fn fill_preview_waits_for_the_new_sheets_geometry_and_respects_its_protection() {
    for (hidden, zoom) in [(false, 200), (true, 200), (false, 100), (true, 100)] {
        let mut h = harness(|s| {
            s.execute("cell.set", json!({"cell": "A1", "input": "=C1*2.71"})).unwrap();
            s.execute("home.insertSheet", json!({"name": "Private"})).unwrap();
            s.execute("cell.set", json!({"cell": "A1", "input": "=C1*1.17"})).unwrap();
            s.execute("view.zoom", json!({"percent": zoom})).unwrap();
            s.execute("home.formatCells", json!({"range": "A1", "style": {"protection": {"hidden": hidden}}})).unwrap();
            s.execute("home.formatCells", json!({"range": "A2:A4", "style": {"protection": {"locked": false}}})).unwrap();
            s.execute("review.protectSheet", json!({})).unwrap();
            s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
            s.execute("selection.set", json!({"cell": "A1"})).unwrap();
        });
        let (start, end) = (corner(&h, "A1"), center(&h, "A4"));
        press(&mut h, start, PointerButton::Primary);
        move_to(&mut h, start, end);
        h.run_steps(2);
        assert!(h.query_by_label("=C4*2.71").is_some());
        let modifiers = Modifiers::COMMAND;
        h.input_mut().events.push(Event::ModifiersChanged(modifiers));
        h.input_mut().events.push(Event::Key { key: egui::Key::PageDown, physical_key: None, pressed: true, repeat: false, modifiers });
        h.step();
        assert_eq!(h.state().session.active().unwrap().wb.active_sheet(), 1, "the keyboard switches sheets during the drag");
        assert!(matches!(h.state().grid.drag, gridcraft_ui_egui::grid::Drag::Fill { target: Some(_), .. }));
        assert_eq!(value(&h, "A4"), Value::Empty, "the preview must not commit a fill");
        assert!(h.query_by_label("=C4*1.17").is_none(), "zoom={zoom}: the navigation frame must not use the old sheet's geometry");
        assert!(h.query_by_label("=C4*2.71").is_none(), "the old sheet's preview must also disappear");
        if let Ok(prefix) = std::env::var("GRIDCRAFT_FILL_NAV_SNAPSHOT") {
            h.render().unwrap().save(format!("{prefix}-{zoom}-{hidden}-navigation.png")).unwrap();
        }
        let end = center(&h, "A4");
        h.input_mut().events.push(Event::ModifiersChanged(Modifiers::NONE));
        h.input_mut().events.push(Event::Key {
            key: egui::Key::PageDown,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.input_mut().events.push(Event::PointerMoved(end));
        h.run_steps(3);
        assert_eq!(h.query_by_label("=C4*1.17").is_some(), !hidden, "zoom={zoom}: preview resumes using the new sheet's geometry and protection");
        assert_eq!(value(&h, "A4"), Value::Empty);
        if let Ok(prefix) = std::env::var("GRIDCRAFT_FILL_NAV_SNAPSHOT") {
            h.render().unwrap().save(format!("{prefix}-{zoom}-{hidden}-resumed.png")).unwrap();
        }
        release(&mut h, end, PointerButton::Primary);
        assert_eq!(h.state().session.active().unwrap().wb.active().unwrap().input_text(CellRef::parse("A4").unwrap()), "=C4*1.17");
    }
}

#[test]
fn left_drag_shows_the_auto_fill_options_button() {
    let mut h = harness(one_two);
    assert!(h.query_by_label("Auto Fill Options").is_none());
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);

    assert_eq!(values(&h, &["B3", "B6"]), [n(3.0), n(6.0)]);
    assert!(h.query_by_label("Auto Fill Options").is_some());
}

#[test]
fn copy_cells_replaces_the_series_the_drag_made() {
    let mut h = harness(one_two);
    let steps_before = h.state().session.active().unwrap().undo.len();
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Copy Cells").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B3", "B4", "B5", "B6"]), [n(1.0), n(2.0), n(1.0), n(2.0)]);
    assert_eq!(selection(&h), "B1:B6");
    assert_eq!(h.state().session.active().unwrap().undo.len(), steps_before + 1, "one fill step, not two");
}

#[test]
fn a_lone_number_copies_on_drag_and_counts_up_with_fill_series() {
    let mut h = harness(|s| {
        s.execute("cell.set", json!({"cell": "B1", "input": "5"})).unwrap();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    });
    drag_handle(&mut h, "B1", "B4", PointerButton::Primary);
    assert_eq!(values(&h, &["B2", "B3", "B4"]), [n(5.0), n(5.0), n(5.0)]);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Fill Series").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B2", "B3", "B4"]), [n(6.0), n(7.0), n(8.0)]);
}

#[test]
fn fill_months_redoes_a_date_drag_by_months() {
    let mut h = harness(|s| {
        // 1/31/2026 is serial 46053.
        s.execute("cell.set", json!({"cell": "B1", "input": "1/31/2026"})).unwrap();
        s.execute("selection.set", json!({"cell": "B1"})).unwrap();
    });
    drag_handle(&mut h, "B1", "B3", PointerButton::Primary);
    assert_eq!(values(&h, &["B2", "B3"]), [n(46054.0), n(46055.0)], "a plain drag steps by days");
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Fill Months").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B2", "B3"]), [n(46081.0), n(46112.0)], "Feb 28 and Mar 31");
}

#[test]
fn a_number_drag_offers_no_date_fills() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    assert!(h.query_by_label("Copy Cells").is_some());
    assert!(h.query_by_label("Fill Days").is_none());
}

fn formatted(s: &mut Session) {
    s.execute("range.setValues", json!({"range": "B1", "values": [[1], [2], [null], [9]]})).unwrap();
    s.execute("home.bold", json!({"range": "B1:B2", "on": true})).unwrap();
    s.execute("home.italic", json!({"range": "B4", "on": true})).unwrap();
    s.execute("selection.set", json!({"range": "B1:B2"})).unwrap();
}

#[test]
fn fill_without_formatting_keeps_the_targets_formats() {
    let mut h = harness(formatted);
    drag_handle(&mut h, "B2", "B4", PointerButton::Primary);
    h.get_by_label("Auto Fill Options").click();
    h.run_steps(3);
    h.get_by_label("Fill Without Formatting").click();
    h.run_steps(3);

    assert_eq!(values(&h, &["B3", "B4"]), [n(3.0), n(4.0)]);
    assert!(!bold(&h, "B3") && !bold(&h, "B4"));
    assert!(italic(&h, "B4"));
}

#[test]
fn the_button_goes_away_once_the_selection_moves() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    assert!(h.query_by_label("Auto Fill Options").is_some());
    let away = center(&h, "E9");
    h.input_mut().events.push(Event::PointerMoved(away));
    h.input_mut().events.push(Event::PointerButton { pos: away, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.input_mut().events.push(Event::PointerButton { pos: away, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);

    assert!(h.query_by_label("Auto Fill Options").is_none());
}

#[test]
fn right_drag_makes_no_button() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Secondary);
    assert!(h.query_by_label("Auto Fill Options").is_none());
}

#[test]
fn the_button_goes_away_once_the_workbook_changes() {
    let mut h = harness(one_two);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    assert!(h.query_by_label("Auto Fill Options").is_some());
    h.state_mut().session.execute("cell.set", json!({"cell": "D1", "input": "x"})).unwrap();
    h.run_steps(3);

    assert_eq!(selection(&h), "B1:B6", "the selection did not move");
    assert!(h.query_by_label("Auto Fill Options").is_none(), "its undo would take the unrelated edit with it");
}

#[test]
fn the_button_stays_inside_the_grid_on_a_right_to_left_sheet() {
    let mut h = harness(|s| {
        one_two(s);
        s.execute("view.rightToLeft", json!({"on": true})).unwrap();
    });
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);

    let button = h.get_by_label("Auto Fill Options").rect();
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    assert!(geo.cells.contains_rect(button), "{button:?} outside {:?}", geo.cells);
    let filled = geo.screen_cell_rect(sheet, CellRef::parse("B6").unwrap());
    assert!(button.center().x < filled.left() && button.top() >= filled.bottom() - 1.0, "below-left of the mirrored range's corner");
}

/// Right-to-left sheet with column A made `chars` wide (so column B's right edge, where the fill
/// range B1:B6 ends, sits far enough right to be scrolled to either edge of the cells area).
fn rtl_wide_a(chars: u32) -> Harness {
    harness(|s| {
        one_two(s);
        s.execute("home.columnWidth", json!({"cols": "A:A", "chars": chars})).unwrap();
        s.execute("view.rightToLeft", json!({"on": true})).unwrap();
    })
}

/// Logical right edge of the filled column B (scroll-dependent) and the current geometry.
fn b_right(h: &Harness) -> (f32, Geo) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    (geo.cell_rect(sheet, CellRef::parse("B6").unwrap()).right(), geo)
}

fn scroll_b_right_to(h: &mut Harness, logical_x: f32) {
    let (now, geo) = b_right(h);
    h.state_mut().view_mut().unwrap().scroll.x += (now - logical_x) / geo.z;
    h.run_steps(3);
    let got = b_right(h).0;
    assert!((got - logical_x).abs() < 1.0, "setup: column B ends at logical x {got}, wanted {logical_x}");
}

/// Logical x of the range's corner is `cells.right() - 25`: the 18 px button (at corner + 2 ..
/// corner + 20) fits in the cells area, so on screen it sits at the left edge of the mirrored cells
/// area, left of the filled range. `fill_options_tag` tested the mirrored button against the
/// unmirrored cells, i.e. against the row-heading strip it does not own, and hid it.
#[test]
fn the_button_shows_near_the_left_screen_edge_on_a_right_to_left_sheet() {
    let mut h = rtl_wide_a(220);
    let (_, geo) = b_right(&h);
    scroll_b_right_to(&mut h, geo.cells.right() - 25.0);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);

    let (_, geo) = b_right(&h);
    let cells = h.state().grid.cells_rect.unwrap();
    assert!(geo.header_w > 20.0 && geo.cells.right() - 25.0 + 20.0 > geo.cells.right() - geo.header_w, "setup: in the zone the old check hides");
    let button = h.query_by_label("Auto Fill Options").expect("the button fits inside the cells area, so it shows").rect();
    assert!(cells.contains_rect(button), "{button:?} outside {cells:?}");
}

/// The corner is 10 px left of `cells.left()` (scrolled partly behind the row headings, which are
/// on the right on screen). The button would start 8 px behind the headings, so it must not be
/// drawn there. The old check let it through because the mirrored button fits the unmirrored cells.
#[test]
fn the_button_stays_off_the_row_headings_on_a_right_to_left_sheet() {
    let mut h = rtl_wide_a(20);
    drag_handle(&mut h, "B2", "B6", PointerButton::Primary);
    assert!(h.query_by_label("Auto Fill Options").is_some(), "setup: the button shows before scrolling");
    let (_, geo) = b_right(&h);
    scroll_b_right_to(&mut h, geo.cells.left() - 10.0);

    let cells = h.state().grid.cells_rect.unwrap();
    if let Some(b) = h.query_by_label("Auto Fill Options") {
        let button = b.rect();
        assert!(cells.contains_rect(button), "{button:?} overlaps the row headings, cells area is {cells:?}");
    }
}
