//! A sheet's gridline colour (#3056, #3100): the grid strokes its gridlines in it, Automatic keeps
//! the theme's, and File › Options › Display options for this worksheet sets it for the sheet
//! picked in its list.

use egui::accesskit::Role;
use egui::{Color32, Shape, vec2};
use egui_kittest::kittest::Queryable as _;
use gridcraft_engine::Session;
use gridcraft_ui_egui::theme::Tokens;
use gridcraft_ui_egui::{SheetApp, grid::Geo};
use serde_json::json;

type Harness = egui_kittest::Harness<'static, SheetApp>;

fn two_sheets() -> Session {
    let mut s = Session::new();
    s.new_workbook();
    s.execute("home.insertSheet", json!({"name": "Two"})).unwrap();
    s.execute("sheet.activate", json!({"sheet": 0})).unwrap();
    s
}

fn harness(session: Session) -> Harness {
    crate::common::harness_with(crate::common::builder().with_size(vec2(1200.0, 1400.0)), crate::common::app(session), 4)
}

/// The colours of the line segments that start over the cells: the gridlines (a new sheet has no
/// borders), counted per colour.
fn line_colors(h: &Harness) -> Vec<(Color32, usize)> {
    let app = h.state();
    let sheet = app.session.active().unwrap().wb.active().unwrap();
    let cells = Geo::new(sheet, app.grid.rect.unwrap(), app.view().scroll).cells;
    let mut counts: Vec<(Color32, usize)> = Vec::new();
    for shape in &h.output().shapes {
        if let Shape::LineSegment { points, stroke } = &shape.shape
            && cells.expand(1.0).contains(points[0])
        {
            match counts.iter_mut().find(|(c, _)| *c == stroke.color) {
                Some((_, n)) => *n += 1,
                None => counts.push((stroke.color, 1)),
            }
        }
    }
    counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    counts
}

/// The colour most of the gridlines are painted in.
fn gridline_color(h: &Harness) -> Color32 {
    let counts = line_colors(h);
    assert!(counts.first().is_some_and(|(_, n)| *n > 20), "the grid paints its gridlines: {counts:?}");
    counts[0].0
}

fn colors(h: &Harness) -> Vec<Option<u8>> {
    h.state().session.active().unwrap().wb.sheets.iter().map(|sh| sh.gridline_color).collect()
}

#[test]
fn the_grid_paints_the_sheets_gridline_colour() {
    let mut h = harness(two_sheets());
    let theme = Tokens::get(&h.ctx).gridline;
    assert_eq!(gridline_color(&h), theme, "Automatic is the theme's colour");

    h.state_mut().run("view.gridlineColor", json!({"colorIndex": 3})).unwrap();
    h.run_steps(2);
    assert_eq!(gridline_color(&h), Color32::from_rgb(0xFF, 0, 0));
    assert!(line_colors(&h).iter().all(|(c, _)| *c != theme), "no gridline keeps the theme's colour: {:?}", line_colors(&h));

    // The colour belongs to the sheet: the next one is still Automatic.
    h.state_mut().run("sheet.activate", json!({"sheet": 1})).unwrap();
    h.run_steps(2);
    assert_eq!(gridline_color(&h), theme);

    // An RGB colour draws as the palette colour it snaps to.
    h.state_mut().run("view.gridlineColor", json!({"color": "#123456"})).unwrap();
    h.run_steps(2);
    assert_eq!(gridline_color(&h), Color32::from_rgb(0x00, 0x33, 0x66));

    // Hidden gridlines draw nothing, and come back in their colour.
    h.state_mut().run("view.gridlines", json!({"on": false})).unwrap();
    h.run_steps(2);
    assert!(line_colors(&h).iter().all(|(c, _)| *c != Color32::from_rgb(0x00, 0x33, 0x66)), "{:?}", line_colors(&h));
    h.state_mut().run("view.gridlines", json!({"on": true})).unwrap();
    h.run_steps(2);
    assert_eq!(gridline_color(&h), Color32::from_rgb(0x00, 0x33, 0x66));

    h.state_mut().run("view.gridlineColor", json!({"colorIndex": "automatic"})).unwrap();
    h.run_steps(2);
    assert_eq!(gridline_color(&h), theme);
}

#[test]
fn options_set_the_display_of_the_sheet_picked_in_the_list() {
    let mut h = harness(two_sheets());
    h.state_mut().run("file.options", json!({})).unwrap();
    h.run_steps(3);
    h.check_accessibility();

    // The active sheet is picked first.
    h.get_by_role_and_label(Role::Button, "Gridline color").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "#FF0000").click();
    h.run_steps(2);
    assert_eq!(colors(&h), [Some(3), None]);
    assert!(h.query_by_role_and_label(Role::Button, "#FF0000").is_none(), "a pick closes the colours");

    // Another sheet from the list; its checkboxes and colour are its own.
    h.get_by_role_and_label(Role::ComboBox, "Display options for this worksheet").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Two").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Show gridlines").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::CheckBox, "Show a zero in cells that have zero value").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Gridline color").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "#333333").click();
    h.run_steps(2);
    let wb = h.state().session.active().unwrap().wb.clone();
    let shown = |i: usize| wb.sheet(i).map(|sh| (sh.show_gridlines, sh.show_zeros)).unwrap();
    assert_eq!((shown(0), shown(1)), ((true, true), (false, false)), "only Two changed");
    assert_eq!(colors(&h), [Some(3), Some(56)]);
    assert_eq!(wb.active_sheet(), 0, "picking a sheet in the list doesn't activate it");

    h.get_by_role_and_label(Role::Button, "Gridline color").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Button, "Automatic").click();
    h.run_steps(2);
    assert_eq!(colors(&h), [Some(3), None]);
    h.check_accessibility();

    // In German, as Excel words the options.
    h.state_mut().run("app.language.set", json!({"language": "de"})).unwrap();
    h.run_steps(3);
    for label in ["Optionen für die Anzeige dieses Arbeitsblatts", "Gitternetzlinien anzeigen", "Gitternetzlinienfarbe"] {
        assert!(h.query_all_by_label(label).next().is_some(), "no {label}");
    }
}
