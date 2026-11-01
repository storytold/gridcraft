//! Page Layout view through real pointer and keyboard input: cells are hit through the page
//! transform, and headers are edited in place, committed on a click elsewhere, and undoable.

use egui::{Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};
use egui_kittest::kittest::Queryable;
use gridcraft_engine::Session;
use gridcraft_engine::core::CellRef;
use gridcraft_engine::model::SheetViewMode;
use gridcraft_ui_egui::SheetApp;
use gridcraft_ui_egui::grid::Geo;
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn fixture() -> Harness {
    fixture_with(false, None)
}

/// `rtl` mirrors the sheet; `setup` is extra `pageLayout.pageSetup` params.
fn fixture_with(rtl: bool, setup: Option<serde_json::Value>) -> Harness {
    let mut session = Session::new();
    session.new_workbook();
    if rtl {
        session.execute("view.rightToLeft", json!({"on": true})).unwrap();
    }
    if let Some(p) = setup {
        session.execute("cell.set", json!({"cell": "Z1", "input": "wide"})).unwrap();
        session.execute("pageLayout.pageSetup", p).unwrap();
    }
    for r in 1..=60 {
        session.execute("cell.set", json!({"cell": format!("A{r}"), "input": r.to_string()})).unwrap();
    }
    session.execute("pageLayout.headerFooter", json!({"header": "&LLeft"})).unwrap();
    session.execute("view.pageLayout", json!({})).unwrap();
    crate::common::harness_with(
        crate::common::builder().with_size(vec2(1200.0, 900.0)).with_step_dt(1.0 / 60.0).with_os(egui::os::OperatingSystem::Mac),
        crate::common::app(session),
        4,
    )
}

fn geo(h: &Harness) -> Geo {
    h.state().grid.geo.clone().unwrap()
}

fn click(h: &mut Harness, p: Pos2) {
    // Keep clicks apart from egui's double-click window.
    h.run_steps(40);
    h.input_mut().events.push(Event::PointerMoved(p));
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.step();
    h.input_mut().events.push(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

fn header(h: &Harness) -> String {
    h.state().session.doc().unwrap().wb.active().unwrap().print.header.clone()
}

/// Screen point of a layout point.
fn screen(g: &Geo, x: f64, y: f64) -> Pos2 {
    pos2(g.cells.left() + (x - g.scroll.x as f64) as f32 * g.z, g.cells.top() + (y - g.scroll.y as f64) as f32 * g.z)
}

#[test]
fn cells_are_hit_through_the_page_transform() {
    let mut h = fixture();
    let g = geo(&h);
    let pg = g.paper.clone().expect("Page Layout view lays out pages");
    let app = h.state();
    let sh = app.session.doc().unwrap().wb.active().unwrap();
    assert_eq!(sh.view, SheetViewMode::PageLayout);
    // Cells start inside the first page's margins, not at the grid's corner.
    let a1 = g.cell_rect(sh, CellRef::new(0, 0));
    let content = screen(&g, pg.page_x(0) + pg.margins[0], pg.page_y(0) + pg.margins[2]);
    assert!((a1.left() - content.x).abs() < 0.5 && (a1.top() - content.y).abs() < 0.5, "{a1:?} vs {content:?}");
    let c5 = g.cell_rect(sh, CellRef::parse("C5").unwrap()).center();
    assert_eq!(g.cell_at(sh, c5), CellRef::parse("C5").unwrap());
    click(&mut h, c5);
    assert_eq!(h.state().session.doc().unwrap().selection.active, CellRef::parse("C5").unwrap());
    // A click in the gap left of the page selects nothing new.
    let gap = screen(&g, pg.gap / 2.0, pg.page_y(0) + pg.margins[2] + 30.0);
    click(&mut h, gap);
    assert_eq!(h.state().session.doc().unwrap().selection.active, CellRef::parse("C5").unwrap());
}

#[test]
fn header_is_edited_in_place_committed_by_a_click_elsewhere_and_undoable() {
    let mut h = fixture();
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    // The centre third of the first page's top margin.
    let (px, py, w, _) = pg.page_rect(0, 0);
    let centre = screen(&g, px + w / 2.0, py + pg.margins[2] / 2.0);
    click(&mut h, centre);
    {
        let app = h.state();
        let e = app.grid.hf_edit.as_ref().expect("clicking the top margin edits the header");
        assert_eq!((e.footer, e.section), (false, 1));
        assert_eq!(app.ui.ribbon_tab, "Header & Footer");
    }
    h.input_mut().events.push(Event::Text("Report ".into()));
    h.run_steps(2);
    gridcraft_ui_egui::paper_view::insert_field(h.state_mut(), "&[Page]");
    h.run_steps(2);
    assert_eq!(h.state().grid.hf_edit.as_ref().unwrap().texts[1], "Report &[Page]");
    assert_eq!(header(&h), "&LLeft", "nothing is written while editing");
    // Clicking a cell commits (one undo step) and selects the cell.
    let sh_geo = geo(&h);
    let b3 = {
        let app = h.state();
        let sh = app.session.doc().unwrap().wb.active().unwrap();
        sh_geo.cell_rect(sh, CellRef::parse("B3").unwrap()).center()
    };
    click(&mut h, b3);
    assert!(h.state().grid.hf_edit.is_none());
    assert_eq!(header(&h), "&LLeft&CReport &P");
    assert_eq!(h.state().session.doc().unwrap().selection.active, CellRef::parse("B3").unwrap());
    assert_ne!(h.state().ui.ribbon_tab, "Header & Footer");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(header(&h), "&LLeft");
    // Escape cancels an edit without writing.
    click(&mut h, centre);
    h.input_mut().events.push(Event::Text("x".into()));
    h.run_steps(2);
    h.input_mut().events.push(Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    h.run_steps(2);
    assert!(h.state().grid.hf_edit.is_none());
    assert_eq!(header(&h), "&LLeft");
}

#[test]
fn the_first_page_edits_its_own_header_when_different_first_page_is_on() {
    let mut h = fixture();
    h.state_mut().run("pageLayout.headerFooter", json!({"differentFirst": true, "firstHeader": "&CCover"})).unwrap();
    h.run_steps(3);
    // Insert › Header & Footer edits the first page's centre header, here the first-page one.
    gridcraft_ui_egui::paper_view::start_editing(h.state_mut());
    h.run_steps(3);
    {
        let e = h.state().grid.hf_edit.as_ref().expect("Insert › Header & Footer starts editing");
        assert_eq!((e.footer, e.section, e.kind), (false, 1, gridcraft_engine::model::HfPage::First));
        assert_eq!(e.texts[1], "Cover");
    }
    h.input_mut().events.push(Event::Text(" page".into()));
    h.run_steps(2);
    let g = geo(&h);
    let b3 = {
        let app = h.state();
        g.cell_rect(app.session.doc().unwrap().wb.active().unwrap(), CellRef::parse("B3").unwrap()).center()
    };
    click(&mut h, b3);
    let pr = h.state().session.doc().unwrap().wb.active().unwrap().print.clone();
    assert_eq!(pr.first_header, "&CCover page");
    assert_eq!(pr.header, "&LLeft", "the other pages keep their header");
}

#[test]
fn scale_with_document_and_align_with_page_margins_are_check_boxes_on_the_header_and_footer_tab() {
    let mut h = fixture();
    gridcraft_ui_egui::paper_view::start_editing(h.state_mut());
    h.run_steps(4);
    let options = |h: &Harness| {
        let pr = &h.state().session.doc().unwrap().wb.active().unwrap().print;
        (pr.hf_scale_with_doc, pr.hf_align_with_margins)
    };
    assert_eq!(options(&h), (true, true));
    h.get_by_label("Scale with Document").click();
    h.run_steps(3);
    assert_eq!(options(&h), (false, true));
    h.get_by_label("Align with Page Margins").click();
    h.run_steps(3);
    assert_eq!(options(&h), (false, false));
    assert!(h.state().grid.hf_edit.is_some(), "the header stays in edit");
    h.state_mut().run("edit.undo", json!({})).unwrap();
    assert_eq!(options(&h), (false, true));
}

#[test]
fn insert_header_and_footer_switches_to_page_layout_view() {
    let mut h = fixture();
    h.state_mut().run("view.normal", json!({})).unwrap();
    h.run_steps(3);
    gridcraft_ui_egui::paper_view::start_editing(h.state_mut());
    h.run_steps(4);
    assert_eq!(h.state().session.doc().unwrap().wb.active().unwrap().view, SheetViewMode::PageLayout);
    let e = h.state().grid.hf_edit.as_ref().expect("the header is being edited");
    assert_eq!((e.footer, e.section, e.texts[0].as_str()), (false, 1, "Left"));
}

fn margins(h: &Harness) -> [f32; 6] {
    h.state().session.doc().unwrap().wb.active().unwrap().print.margins
}

fn mouse(h: &mut Harness, ev: Event) {
    h.input_mut().events.push(ev);
    h.step();
}

#[test]
fn dragging_a_ruler_edge_previews_commits_on_release_as_one_undo_step_and_esc_cancels() {
    let mut h = fixture();
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let start_margins = margins(&h);
    let ruler_y = h.state().grid.rect.unwrap().top() + 9.0;
    let edge = pos2(screen(&g, pg.page_x(0) + pg.margins[0], 0.0).x, ruler_y);
    // One inch further in = 96 layout pixels at 100% print scale; use the screen scale to be exact.
    let per_inch = (72.0 / (pg.paper_pt.0 as f64 / pg.page_w)) as f32 * g.z;
    let target = pos2(edge.x + per_inch, ruler_y);
    h.run_steps(40);
    mouse(&mut h, Event::PointerMoved(edge));
    mouse(&mut h, Event::PointerButton { pos: edge, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    mouse(&mut h, Event::PointerMoved(pos2(edge.x + 5.0, ruler_y)));
    mouse(&mut h, Event::PointerMoved(target));
    h.run_steps(2);
    // Live preview: the page content moved, the document did not change yet.
    let live = geo(&h).paper.unwrap().margins[0];
    assert!(live > pg.margins[0] + 1.0, "{live} vs {}", pg.margins[0]);
    assert_eq!(margins(&h), start_margins);
    let want = (start_margins[0] + 1.0 * 1.0).min(10.0);
    mouse(&mut h, Event::PointerButton { pos: target, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    let after = margins(&h);
    assert!((after[0] - want).abs() < 0.03, "{after:?} want left {want}");
    assert_eq!(after[1..], start_margins[1..]);
    // One undo step restores it.
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(margins(&h), start_margins);
    // Esc during a drag cancels: nothing is committed.
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let edge = pos2(screen(&g, pg.page_x(0) + pg.margins[0], 0.0).x, ruler_y);
    h.run_steps(40);
    mouse(&mut h, Event::PointerMoved(edge));
    mouse(&mut h, Event::PointerButton { pos: edge, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    mouse(&mut h, Event::PointerMoved(pos2(edge.x + per_inch, ruler_y)));
    mouse(&mut h, Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE });
    mouse(&mut h, Event::PointerButton { pos: edge, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert_eq!(margins(&h), start_margins);
}

fn press(h: &mut Harness, p: Pos2) {
    h.run_steps(40);
    mouse(h, Event::PointerMoved(p));
    mouse(h, Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
}

fn release(h: &mut Harness, p: Pos2) {
    mouse(h, Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// Screen pixels per inch of the page.
fn per_inch(g: &Geo) -> f32 {
    let pg = g.paper.clone().unwrap();
    (72.0 / (pg.paper_pt.0 as f64 / pg.page_w)) as f32 * g.z
}

#[test]
fn header_and_footer_margins_are_dragged_on_the_vertical_ruler() {
    let mut h = fixture();
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let start = margins(&h);
    let ruler_x = h.state().grid.rect.unwrap().left() + 9.0;
    // Header: distance from the top of the page, 0.2 in further down.
    let at = pos2(ruler_x, screen(&g, 0.0, pg.page_y(0) + pg.margins[4]).y);
    let to = pos2(ruler_x, at.y + 0.2 * per_inch(&g));
    press(&mut h, at);
    mouse(&mut h, Event::PointerMoved(pos2(ruler_x, at.y + 4.0)));
    mouse(&mut h, Event::PointerMoved(to));
    release(&mut h, to);
    let after = margins(&h);
    assert!((after[4] - (start[4] + 0.2)).abs() < 0.03, "{after:?} vs {start:?}");
    assert_eq!((after[..4].to_vec(), after[5]), (start[..4].to_vec(), start[5]));
    h.state_mut().run("edit.undo", json!({})).unwrap();
    h.run_steps(3);
    assert_eq!(margins(&h), start);
    // Footer: distance from the bottom, dragged up by 0.2 in grows it (zoom out so the page
    // bottom is on screen).
    h.state_mut().run("view.zoom", json!({"percent": 50})).unwrap();
    h.run_steps(4);
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let (_, py, _, ph) = pg.page_rect(0, 0);
    let at = pos2(ruler_x, screen(&g, 0.0, py + ph - pg.margins[5]).y);
    let to = pos2(ruler_x, at.y - 0.2 * per_inch(&g));
    press(&mut h, at);
    mouse(&mut h, Event::PointerMoved(pos2(ruler_x, at.y - 4.0)));
    mouse(&mut h, Event::PointerMoved(to));
    release(&mut h, to);
    assert!((margins(&h)[5] - (start[5] + 0.2)).abs() < 0.03, "{:?}", margins(&h));
}

#[test]
fn right_to_left_sheets_mirror_the_horizontal_ruler_drag() {
    let mut h = fixture_with(true, None);
    let g = geo(&h);
    assert!(g.rtl);
    let pg = g.paper.clone().unwrap();
    let start = margins(&h);
    let y = h.state().grid.rect.unwrap().top() + 9.0;
    // The left margin edge is drawn on the right of the mirrored page; dragging it towards the
    // page centre (screen-left) grows the left margin.
    let at = pos2(g.mirror_x(screen(&g, pg.page_x(0) + pg.margins[0], 0.0).x), y);
    let to = pos2(at.x - per_inch(&g), y);
    press(&mut h, at);
    mouse(&mut h, Event::PointerMoved(pos2(at.x - 4.0, y)));
    mouse(&mut h, Event::PointerMoved(to));
    release(&mut h, to);
    let after = margins(&h);
    assert!((after[0] - (start[0] + 1.0)).abs() < 0.03, "{after:?} vs {start:?}");
    assert_eq!(after[1], start[1]);
}

#[test]
fn preview_repaginates_so_fit_to_width_matches_the_commit() {
    let mut h = fixture_with(false, Some(json!({"fitWidth": 1, "fitHeight": 0})));
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let start = margins(&h);
    let y = h.state().grid.rect.unwrap().top() + 9.0;
    let at = pos2(screen(&g, pg.page_x(0) + pg.margins[0], 0.0).x, y);
    let to = pos2(at.x + 2.0 * per_inch(&g), y);
    press(&mut h, at);
    mouse(&mut h, Event::PointerMoved(pos2(at.x + 4.0, y)));
    mouse(&mut h, Event::PointerMoved(to));
    h.run_steps(2);
    let live_scale = geo(&h).paper.unwrap().scale;
    assert_eq!(margins(&h), start);
    assert!(live_scale < pg.scale, "live scale {live_scale} should shrink below {}", pg.scale);
    release(&mut h, to);
    h.run_steps(2);
    let committed = geo(&h).paper.unwrap().scale;
    assert!((committed - live_scale).abs() < 0.01, "committed {committed} vs preview {live_scale}");
}

#[test]
fn right_to_left_sheets_carry_the_vertical_ruler_on_the_right() {
    let mut h = fixture_with(true, None);
    let g = geo(&h);
    assert!(g.rtl);
    let pg = g.paper.clone().unwrap();
    let start = margins(&h);
    let outer = h.state().grid.rect.unwrap();
    // The grid leaves the ruler's width free on the right, not on the left.
    assert!((g.rect.right() - (outer.right() - gridcraft_ui_egui::paper_view::RULER)).abs() < 0.5, "{:?} {outer:?}", g.rect);
    assert!((g.rect.left() - outer.left()).abs() < 0.5);
    let x = outer.right() - 9.0;
    let at = pos2(x, screen(&g, 0.0, pg.page_y(0) + pg.margins[2]).y);
    let to = pos2(x, at.y + 0.25 * per_inch(&g));
    press(&mut h, at);
    mouse(&mut h, Event::PointerMoved(pos2(x, at.y + 4.0)));
    mouse(&mut h, Event::PointerMoved(to));
    release(&mut h, to);
    let after = margins(&h);
    assert!((after[2] - (start[2] + 0.25)).abs() < 0.03, "{after:?} vs {start:?}");
    // The left ruler strip is not a handle any more: nothing happens there.
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let at = pos2(outer.left() + 9.0, screen(&g, 0.0, pg.page_y(0) + pg.margins[3]).y);
    press(&mut h, at);
    mouse(&mut h, Event::PointerMoved(pos2(at.x, at.y - 30.0)));
    release(&mut h, pos2(at.x, at.y - 30.0));
    assert_eq!(margins(&h)[3], start[3]);
}

#[test]
fn header_distance_cannot_be_dragged_past_the_top_margin() {
    let mut h = fixture();
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let start = margins(&h);
    let ruler_x = h.state().grid.rect.unwrap().left() + 9.0;
    let at = pos2(ruler_x, screen(&g, 0.0, pg.page_y(0) + pg.margins[4]).y);
    let to = pos2(ruler_x, at.y + 3.0 * per_inch(&g));
    press(&mut h, at);
    mouse(&mut h, Event::PointerMoved(pos2(ruler_x, at.y + 4.0)));
    mouse(&mut h, Event::PointerMoved(to));
    release(&mut h, to);
    let after = margins(&h);
    assert!((after[4] - start[2]).abs() < 0.011, "header {} should stop at the top margin {}", after[4], start[2]);
    assert_eq!(after[2], start[2]);
}

// ---------------------------------------------------------------- header/footer pictures (#1257)

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba([20, 90, 160, 255]));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A Page Layout view of a sheet with `header` and a kept header/footer VML drawing (as reading
/// an Excel file gives it) holding one picture per `(shape id, style, image)`.
fn picture_fixture(header: &str, shapes: &[(&str, &str, Vec<u8>)]) -> Harness {
    use gridcraft_engine::model::{Preserved, PreservedPart, PreservedRel, PreservedXml};
    let mut session = Session::new();
    session.new_workbook();
    for r in 1..=60 {
        session.execute("cell.set", json!({"cell": format!("A{r}"), "input": r.to_string()})).unwrap();
    }
    session.execute("pageLayout.headerFooter", json!({"header": header})).unwrap();
    let rel = |id: String, target: String| PreservedRel { id, rel_type: "x/image".into(), target, external: false };
    let mut vml = String::from("<xml>");
    let (mut parts, mut vml_rels) = (vec![], vec![]);
    for (i, (id, style, data)) in shapes.iter().enumerate() {
        vml.push_str(&format!(r##"<v:shape id="{id}" type="#_x0000_t75" style="{style}"><v:imagedata o:relid="rId{i}"/></v:shape>"##));
        vml_rels.push(rel(format!("rId{i}"), format!("xl/media/image{i}.png")));
        parts.push(PreservedPart {
            name: format!("xl/media/image{i}.png"),
            content_type: "image/png".into(),
            data: data.as_slice().into(),
            rels: vec![],
        });
    }
    vml.push_str("</xml>");
    parts.push(PreservedPart { name: "xl/drawings/hf.vml".into(), content_type: String::new(), data: vml.as_bytes().into(), rels: vml_rels });
    let kept = Preserved {
        rels: vec![rel("rId9".into(), "xl/drawings/hf.vml".into())],
        elements: vec![PreservedXml { slot: "legacyDrawingHF".into(), xml: "<legacyDrawingHF r:id=\"rId9\"/>".into() }],
        parts,
        ..Preserved::default()
    };
    let d = session.doc_mut().unwrap();
    std::sync::Arc::make_mut(&mut d.wb).sheet_mut(0).unwrap().preserved = Some(std::sync::Arc::new(kept));
    session.execute("view.pageLayout", json!({})).unwrap();
    crate::common::harness_with(
        crate::common::builder().with_size(vec2(1200.0, 900.0)).with_step_dt(1.0 / 60.0).with_os(egui::os::OperatingSystem::Mac),
        crate::common::app(session),
        4,
    )
}

/// Screen rectangles of the textured meshes painted (pictures).
fn picture_rects(h: &Harness) -> Vec<egui::Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => {
                Some(egui::Rect::from_points(&m.vertices.iter().map(|v| v.pos).collect::<Vec<_>>()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn header_pictures_show_in_page_layout_view_where_they_print() {
    let mut h = picture_fixture("&L&G&CTitle", &[("LH", "width:120pt;height:40pt", png(30, 10))]);
    h.run_steps(4);
    let g = geo(&h);
    let pg = g.paper.clone().unwrap();
    let (px, py, _, _) = pg.page_rect(0, 0);
    // Left section, top on the header margin, the size printing gives it on this zoom.
    let at = screen(&g, px + pg.hf_sides[0], py + pg.margins[4]);
    let k = 96.0 / 72.0 * pg.hf_text_k * g.z;
    let rects = picture_rects(&h);
    let first = rects.iter().find(|r| (r.left() - at.x).abs() < 1.0 && (r.top() - at.y).abs() < 1.0);
    let r = first.unwrap_or_else(|| panic!("no picture at {at:?}: {rects:?}"));
    assert!((r.width() - 120.0 * k).abs() < 1.0 && (r.height() - 40.0 * k).abs() < 1.0, "{r:?}");
}

#[test]
fn header_pictures_need_their_g_and_a_readable_picture_on_screen_too() {
    let mut h = picture_fixture("&LNo picture", &[("LH", "width:120pt;height:40pt", png(4, 4))]);
    h.run_steps(4);
    assert!(picture_rects(&h).is_empty());
    let mut h = picture_fixture("&LBefore&G after", &[("LH", "width:120pt;height:40pt", b"\x89PNG\r\n\x1a\nbroken".to_vec())]);
    h.run_steps(4);
    assert!(picture_rects(&h).is_empty());
}

/// Text painted in the last frame.
fn painted_text(h: &Harness) -> Vec<String> {
    h.output()
        .shapes
        .iter()
        .filter_map(|c| match &c.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn print_preview_draws_headers_footers_and_their_pictures() {
    let mut h = picture_fixture("&L&G&CTitle &P/&N", &[("LH", "width:120pt;height:40pt", png(30, 10))]);
    h.run_steps(2);
    let on_sheet = picture_rects(&h).len();
    h.state_mut().run("ui.backstage", json!({"page": "print"})).unwrap();
    h.run_steps(4);
    assert!(painted_text(&h).iter().any(|t| t == "Title 1/2"), "{:?}", painted_text(&h));
    let rects = picture_rects(&h);
    assert_eq!(rects.len(), 1, "the preview page's picture, not the sheet's ({on_sheet} there)");
    // The picture keeps its proportions in miniature.
    assert!((rects[0].width() / rects[0].height() - 3.0).abs() < 0.05, "{rects:?}");
}

#[test]
fn print_preview_without_headers_draws_none() {
    let mut h = picture_fixture("", &[("LH", "width:120pt;height:40pt", png(30, 10))]);
    h.state_mut().run("ui.backstage", json!({"page": "print"})).unwrap();
    h.run_steps(4);
    assert!(picture_rects(&h).is_empty());
    assert!(!painted_text(&h).iter().any(|t| t.starts_with("Title")));
}
