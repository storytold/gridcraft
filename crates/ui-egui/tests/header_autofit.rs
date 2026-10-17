//! Header AutoFit and resize gestures, driven through real egui pointer events.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2, vec2};
use gridcraft_engine::Session;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

fn harness(selection: &str) -> egui_kittest::Harness<'static, SheetApp> {
    let mut session = Session::new();
    session.new_workbook();
    session
        .execute(
            "range.setValues",
            json!({"range": "A1", "values": [
                ["Short", "This column has considerably longer content", "Third", "Unchanged"],
                ["Two\nlines", "B", "C", "D"],
                ["Three\ntext\nlines", "B", "C", "D"]
            ]}),
        )
        .unwrap();
    session.execute("home.columnWidth", json!({"cols": "A:D", "width": 100})).unwrap();
    session.execute("home.rowHeight", json!({"rows": "1:4", "height": 60})).unwrap();
    session.execute("selection.set", json!({"range": selection})).unwrap();
    let mut h = egui_kittest::Harness::builder().with_step_dt(1.0 / 60.0).with_size(vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SheetApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SheetApp::new(session, Default::default()),
    );
    h.run_steps(4);
    h.state_mut().session.active_mut().unwrap().undo.clear();
    h.state_mut().session.journal.clear();
    h
}

fn edge(h: &egui_kittest::Harness<'_, SheetApp>, rows: bool, index: u32) -> (Pos2, f32) {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
    let pos = if rows {
        pos2(geo.rect.left() + geo.header_w / 2.0, geo.y(sheet, index + 1))
    } else {
        pos2(geo.x(sheet, index + 1), geo.rect.top() + geo.header_h / 2.0)
    };
    (pos, geo.z)
}

fn button(h: &mut egui_kittest::Harness<'_, SheetApp>, pos: Pos2, pressed: bool) {
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]);
    h.step();
}

fn sizes(h: &egui_kittest::Harness<'_, SheetApp>, rows: bool) -> Vec<f32> {
    let sheet = h.state().session.active().unwrap().wb.active().unwrap();
    (0..4).map(|index| if rows { sheet.row_height(index) } else { sheet.col_width(index) }).collect()
}

#[test]
fn header_double_click_autofits_selected_areas_or_only_the_clicked_line() {
    for (rows, selection, clicked, changed) in [
        (false, "B:B", 1, [false, true, false, false]),
        (false, "A:C", 1, [true, true, true, false]),
        (false, "A:A,C:C", 0, [true, false, true, false]),
        (false, "A:B", 2, [false, false, true, false]),
        (true, "1:3", 1, [true, true, true, false]),
        (true, "1:1,3:3", 0, [true, false, true, false]),
        (true, "1:2", 2, [false, false, true, false]),
    ] {
        let mut h = harness(selection);
        let before = sizes(&h, rows);
        let selection_before = h.state().session.active().unwrap().selection.clone();
        let (pos, _) = edge(&h, rows, clicked);
        button(&mut h, pos, true);
        button(&mut h, pos, false);
        h.step(); // Let the first click's resize state settle before the second click.
        button(&mut h, pos, true);
        button(&mut h, pos, false);
        h.run_steps(2);

        for (index, ((old, new), changed)) in before.iter().zip(sizes(&h, rows)).zip(changed).enumerate() {
            assert_eq!(*old != new, changed, "selection {selection}, clicked {clicked}, line {index}: {old} → {new}");
        }
        assert_eq!(h.state().session.active().unwrap().selection, selection_before);
        let command = if rows { "home.autofitRowHeight" } else { "home.autofitColumnWidth" };
        assert_eq!(h.state().session.journal.iter().filter(|(id, _)| id == command).count(), 1);
        assert_eq!(h.state().session.active().unwrap().undo.len(), 1);
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(sizes(&h, rows), before, "AutoFit should undo as a single operation");
    }
}

#[test]
fn dragging_a_header_boundary_still_resizes_normally() {
    for rows in [false, true] {
        let mut h = harness(if rows { "2:2" } else { "B:B" });
        let before = sizes(&h, rows);
        let (start, zoom) = edge(&h, rows, 1);
        let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        button(&mut h, start + delta, false);
        h.run_steps(2);
        let after = sizes(&h, rows);
        assert_eq!(after[1], (before[1] + 30.0 / zoom).round());
        for index in [0, 2, 3] {
            assert_eq!(before[index], after[index]);
        }
        assert!(!h.state().session.journal.iter().any(|(id, _)| id.starts_with("home.autofit")));
    }
}

#[test]
fn dragging_a_boundary_inside_a_multi_area_selection_resizes_every_selected_line() {
    // Issue #341: Ctrl-selected rows/columns resize together; unselected lines keep their size.
    for (rows, selection, dragged, changed) in [
        (false, "A:A,C:C", 2, [true, false, true, false]),
        (false, "A:A,C:C", 0, [true, false, true, false]),
        (false, "A:A,C:D", 3, [true, false, true, true]),
        (false, "A:A,C:C", 1, [false, true, false, false]),
        (false, "A:A,C3", 0, [true, false, false, false]),
        (true, "1:1,3:3", 2, [true, false, true, false]),
        (true, "1:1,3:3", 0, [true, false, true, false]),
        (true, "1:1,3:4", 3, [true, false, true, true]),
        (true, "1:1,3:3", 1, [false, true, false, false]),
        (true, "1:1,C3", 0, [true, false, false, false]),
    ] {
        let mut h = harness(selection);
        let before = sizes(&h, rows);
        let (start, zoom) = edge(&h, rows, dragged);
        let delta = if rows { vec2(0.0, 30.0) } else { vec2(30.0, 0.0) };
        button(&mut h, start, true);
        h.input_mut().events.push(Event::PointerMoved(start + delta / 2.0));
        h.step();
        h.input_mut().events.push(Event::PointerMoved(start + delta));
        h.step();
        button(&mut h, start + delta, false);
        h.run_steps(2);
        let after = sizes(&h, rows);
        let expected = (before[dragged as usize] + 30.0 / zoom).round();
        for (index, changed) in changed.iter().enumerate() {
            let want = if *changed { expected } else { before[index] };
            assert_eq!(after[index], want, "selection {selection}, dragged {dragged}, line {index}");
        }
        assert_eq!(h.state().session.active().unwrap().undo.len(), 1, "selection {selection}: one undo step");
        h.state_mut().session.execute("edit.undo", json!({})).unwrap();
        assert_eq!(sizes(&h, rows), before, "selection {selection}: undo restores every line");
    }
}

#[test]
fn table_filter_autofit_works_through_command_and_header_double_click() {
    for zoom in [50, 100, 200] {
        for double_click in [false, true] {
            let mut h = harness("A:A");
            h.state_mut().session.execute("range.setValues", json!({"range":"A1", "values":[["Quarterly Revenue"], ["1"], ["2"]]})).unwrap();
            h.state_mut().session.execute("insert.table", json!({"range":"A1:A3", "header":true})).unwrap();
            h.state_mut().session.execute("home.rowHeight", json!({"rows":"1:1", "height":20})).unwrap();
            h.state_mut().session.execute("view.zoom", json!({"percent":zoom})).unwrap();
            h.run_steps(3);
            h.state_mut().session.active_mut().unwrap().undo.clear();
            h.state_mut().session.journal.clear();
            if double_click {
                let (pos, _) = edge(&h, false, 0);
                button(&mut h, pos, true);
                button(&mut h, pos, false);
                h.step();
                button(&mut h, pos, true);
                button(&mut h, pos, false);
            } else {
                h.state_mut().run("home.autofitColumnWidth", json!({"cols":"A:A"})).unwrap();
            }
            h.run_steps(2);
            let app = h.state();
            let sheet = app.session.active().unwrap().wb.active().unwrap();
            let geo = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll);
            let cell = gridcraft_engine::core::CellRef::new(0, 0);
            let rect = geo.cell_rect(sheet, cell);
            let button = gridcraft_ui_egui::grid::filter_button_rect(&geo, sheet, cell);
            let style = app.session.active().unwrap().wb.styles.get(sheet.style_id(cell));
            let text_width = gridcraft_engine::cmd::format::approx_text_width("Quarterly Revenue", style.font.size, style.font.bold) * geo.z;
            assert!(rect.left() + text_width + 2.5 * geo.z < button.left(), "heading overlaps filter: zoom={zoom}, double_click={double_click}");
            assert_eq!(app.session.active().unwrap().undo.len(), 1);
            h.state_mut().session.execute("edit.undo", json!({})).unwrap();
            assert_eq!(sizes(&h, false)[0], 100.0);
        }
    }
}

#[test]
fn header_double_click_fits_the_text_as_the_grid_draws_it() {
    // Issue #1316: `@` and `%` are far wider in every real font than the engine's average-glyph
    // estimate, so AutoFit has to measure with the font the grid draws the cell in.
    let text = "sarmstrong@example.com %%% @@@";
    let mut h = harness("A1");
    h.state_mut().session.execute("range.setValues", json!({"range":"A1", "values":[[text]]})).unwrap();
    // Let the installed-font scan finish and bind its faces first, so the cell's font can't
    // change between the AutoFit and the measurement below.
    gridcraft_ui_egui::fonts::catalog();
    h.run_steps(6);
    let (pos, _) = edge(&h, false, 0);
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.step();
    button(&mut h, pos, true);
    button(&mut h, pos, false);
    h.run_steps(2);

    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let font = &app.session.active().unwrap().wb.styles.get(sheet.style_id(gridcraft_engine::core::CellRef::new(0, 0))).font;
    let id = egui::FontId::new(font.size * 96.0 / 72.0, gridcraft_ui_egui::theme::cell_family(&font.name, font.bold, font.italic));
    let drawn = h.ctx.fonts_mut(|f| f.layout_no_wrap(text.to_string(), id, egui::Color32::BLACK)).size().x;
    let estimate = gridcraft_engine::cmd::format::approx_text_width(text, font.size, font.bold);
    assert!(drawn > estimate + 6.0, "fixture: the drawn text ({drawn}) must be wider than the estimate ({estimate}) plus padding");
    // Two 2.5 px paddings and the gridline, as the grid lays out the cell.
    assert!(sheet.col_width(0) >= drawn + 6.0, "column {} is narrower than the drawn text {drawn} + padding", sheet.col_width(0));
}

#[test]
fn the_quick_measure_is_never_narrower_than_the_drawn_text() {
    // Columns with many distinct values are measured by summing glyph advances; that must cover
    // the laid-out text, or AutoFit would clip again on large columns.
    let h = harness("A1");
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let mut font = app.session.active().unwrap().wb.styles.get(sheet.style_id(gridcraft_engine::core::CellRef::new(0, 0))).font.clone();
    for bold in [false, true] {
        font.bold = bold;
        for text in ["sarmstrong@example.com", "Quarterly revenue summary", "WWW MMM Wide Headers", "AVATAR To Ty", "fi ffl 1,234.56"] {
            let quick = gridcraft_ui_egui::theme::cell_text_width(&h.ctx, text, &font, false);
            let exact = gridcraft_ui_egui::theme::cell_text_width(&h.ctx, text, &font, true);
            assert!(exact > 0.0 && quick >= exact, "{text:?} bold={bold}: quick {quick} < drawn {exact}");
        }
    }
}
