//! Pictures, shapes and charts follow their cells as their placement says ("Move and size with
//! cells", "Move but don't size with cells", "Don't move or size with cells") when rows and columns
//! are resized, hidden, inserted or deleted under them. The expected numbers (points) were measured
//! in Excel for Microsoft 365 through COM: one object of each kind per placement, from C5 (offset 3
//! pt right, 4 pt down) over 144 x 57.6 pt, on a sheet with 14.4 pt rows and 48 pt columns.

use gridcraft_engine::Session;
use gridcraft_model::AnchorMode;
use serde_json::json;

/// A 1 x 1 PNG.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
/// The model sizes lines and objects in pixels at 96 dpi; Excel reports points.
const PT: f64 = 0.75;

#[derive(Clone, Copy)]
enum Kind {
    Picture,
    Shape,
    Chart,
}

/// One object of each kind in each placement, stacked at the same spot.
struct Fixture {
    s: Session,
    objects: Vec<(Kind, u32, AnchorMode)>,
}

const MODES: [AnchorMode; 3] = [AnchorMode::MoveAndSize, AnchorMode::MoveOnly, AnchorMode::Absolute];

/// The `mode` parameter value for a placement.
fn mode_name(m: AnchorMode) -> &'static str {
    match m {
        AnchorMode::MoveAndSize => "moveAndSize",
        AnchorMode::MoveOnly => "moveOnly",
        AnchorMode::Absolute => "absolute",
    }
}

/// A sheet with Excel's measured row and column sizes and the nine objects.
fn fixture() -> Fixture {
    let mut s = Session::new();
    s.new_workbook();
    {
        let wb = std::sync::Arc::make_mut(&mut s.doc_mut().unwrap().wb);
        let sh = wb.sheet_mut(0).unwrap();
        sh.default_row_height = (14.4 / PT) as f32;
        sh.default_col_width = (48.0 / PT) as f32;
    }
    s.execute("range.setValues", json!({"range": "A1", "values": [["h", 1], ["x", 2], ["y", 3]]})).unwrap();
    let (w, h) = (144.0 / PT, 57.6 / PT);
    let mut objects = Vec::new();
    for mode in MODES {
        let m = mode_name(mode);
        let r = s.execute("insert.picture", json!({"base64": PNG, "at": "C5", "width": w, "height": h, "mode": m})).unwrap();
        objects.push((Kind::Picture, r["image"].as_u64().unwrap() as u32, mode));
        let r = s.execute("insert.shape", json!({"kind": "rectangle", "at": "C5", "width": w, "height": h, "mode": m})).unwrap();
        objects.push((Kind::Shape, r["shape"].as_u64().unwrap() as u32, mode));
        let r = s.execute("insert.chart", json!({"range": "A1:B3", "at": "C5", "width": w, "height": h, "mode": m})).unwrap();
        objects.push((Kind::Chart, r["chart"].as_u64().unwrap() as u32, mode));
    }
    for &(kind, id, _) in &objects {
        let kind = match kind {
            Kind::Picture => "image",
            Kind::Shape => "shape",
            Kind::Chart => "chart",
        };
        s.execute("object.move", json!({"kind": kind, "id": id, "dx": 3.0 / PT, "dy": 4.0 / PT})).unwrap();
    }
    Fixture { s, objects }
}

impl Fixture {
    /// Left, top, width, height in points, as Excel reports them.
    fn rect(&self, kind: Kind, id: u32) -> [f64; 4] {
        let sh = self.s.doc().unwrap().wb.sheet(0).unwrap();
        let a = match kind {
            Kind::Picture => sh.images.iter().find(|o| o.id == id).unwrap().anchor,
            Kind::Shape => sh.shapes.iter().find(|o| o.id == id).unwrap().anchor,
            Kind::Chart => sh.charts.iter().find(|o| o.id == id).unwrap().anchor,
        };
        [(sh.col_left(a.cell.col) + a.dx as f64) * PT, (sh.row_top(a.cell.row) + a.dy as f64) * PT, a.width as f64 * PT, a.height as f64 * PT]
    }

    /// Runs `action`, then checks every object against the rectangle Excel gave its placement.
    fn expect(&mut self, what: &str, action: impl FnOnce(&mut Session), moves_and_sizes: [f64; 4], moves: [f64; 4], free: [f64; 4]) {
        action(&mut self.s);
        for &(kind, id, mode) in &self.objects {
            let want = match mode {
                AnchorMode::MoveAndSize => moves_and_sizes,
                AnchorMode::MoveOnly => moves,
                AnchorMode::Absolute => free,
            };
            let got = self.rect(kind, id);
            for (g, w) in got.iter().zip(want) {
                assert!((g - w).abs() < 0.01, "{what}: {} {:?} is {got:?}, Excel gives {want:?}", mode_name(mode), id);
            }
        }
    }
}

const HOME: [f64; 4] = [99.0, 61.6, 144.0, 57.6];

/// Runs a row command on `rows`.
fn rows(s: &mut Session, id: &str, rows: &str) {
    s.execute(id, json!({"rows": rows})).unwrap();
}

/// Runs a column command on `cols`.
fn cols(s: &mut Session, id: &str, cols: &str) {
    s.execute(id, json!({"cols": cols})).unwrap();
}

#[test]
/// Row resize above inside and below.
fn row_resize_above_inside_and_below() {
    let height = |s: &mut Session, r: &str, pt: f64| s.execute("home.rowHeight", json!({"rows": r, "points": pt})).unwrap();
    // Row 2 to 60 pt: everything but "don't move or size" slides down by 45.6 pt.
    let down = [99.0, 107.2, 144.0, 57.6];
    fixture().expect("row 2 to 60 pt", |s| drop(height(s, "2", 60.0)), down, down, HOME);
    // Rows 5 (the object's first), 6 and 8 inside it: only "move and size" stretches.
    let tall = [99.0, 61.6, 144.0, 103.2];
    for r in ["5", "6", "8"] {
        fixture().expect(&format!("row {r} to 60 pt"), |s| drop(height(s, r, 60.0)), tall, HOME, HOME);
    }
    // Row 12 is below it.
    fixture().expect("row 12 to 60 pt", |s| drop(height(s, "12", 60.0)), HOME, HOME, HOME);
}

#[test]
/// Column resize left inside and right.
fn column_resize_left_inside_and_right() {
    // 221.6 px is 166.2 pt, Excel's width for 30 characters on the machine it was measured on.
    let width = |s: &mut Session, c: &str| s.execute("home.columnWidth", json!({"cols": c, "width": 166.2 / PT})).unwrap();
    let right = [217.2, 61.6, 144.0, 57.6];
    fixture().expect("column A", |s| drop(width(s, "A")), right, right, HOME);
    let wide = [99.0, 61.6, 262.2, 57.6];
    fixture().expect("column D", |s| drop(width(s, "D")), wide, HOME, HOME);
    fixture().expect("column H", |s| drop(width(s, "H")), HOME, HOME, HOME);
}

#[test]
/// Row insert and delete.
fn row_insert_and_delete() {
    let down = [99.0, 76.0, 144.0, 57.6];
    fixture().expect("insert row 2", |s| rows(s, "home.insertRows", "2"), down, down, HOME);
    fixture().expect("insert row 5", |s| rows(s, "home.insertRows", "5"), down, down, HOME);
    let tall = [99.0, 61.6, 144.0, 72.0];
    fixture().expect("insert row 6", |s| rows(s, "home.insertRows", "6"), tall, HOME, HOME);
    // Row 9 holds the object's bottom edge.
    fixture().expect("insert row 9", |s| rows(s, "home.insertRows", "9"), tall, HOME, HOME);
    fixture().expect("insert row 12", |s| rows(s, "home.insertRows", "12"), HOME, HOME, HOME);
    let up = [99.0, 47.2, 144.0, 57.6];
    fixture().expect("delete row 2", |s| rows(s, "home.deleteRows", "2"), up, up, HOME);
    fixture().expect("delete row 12", |s| rows(s, "home.deleteRows", "12"), HOME, HOME, HOME);
    fixture().expect("delete row 6", |s| rows(s, "home.deleteRows", "6"), [99.0, 61.6, 144.0, 43.2], HOME, HOME);
    // The rows under the object's top edge go: the edge sits on the line that takes their place.
    fixture().expect("delete rows 5:8", |s| rows(s, "home.deleteRows", "5:8"), [99.0, 57.6, 144.0, 4.0], [99.0, 57.6, 144.0, 57.6], HOME);
    fixture().expect("delete rows 4:6", |s| rows(s, "home.deleteRows", "4:6"), [99.0, 43.2, 144.0, 32.8], [99.0, 43.2, 144.0, 57.6], HOME);
}

#[test]
/// Column insert and delete.
fn column_insert_and_delete() {
    let right = [147.0, 61.6, 144.0, 57.6];
    fixture().expect("insert column B", |s| cols(s, "home.insertColumns", "B"), right, right, HOME);
    fixture().expect("insert column D", |s| cols(s, "home.insertColumns", "D"), [99.0, 61.6, 192.0, 57.6], HOME, HOME);
    let left = [51.0, 61.6, 144.0, 57.6];
    fixture().expect("delete column B", |s| cols(s, "home.deleteColumns", "B"), left, left, HOME);
    fixture().expect("delete column D", |s| cols(s, "home.deleteColumns", "D"), [99.0, 61.6, 96.0, 57.6], HOME, HOME);
    fixture().expect("delete columns C:E", |s| cols(s, "home.deleteColumns", "C:E"), [96.0, 61.6, 3.0, 57.6], [96.0, 61.6, 144.0, 57.6], HOME);
}

#[test]
/// Hiding rows and columns.
fn hiding_rows_and_columns() {
    let up = [99.0, 47.2, 144.0, 57.6];
    fixture().expect("hide row 2", |s| rows(s, "home.hideRows", "2"), up, up, HOME);
    fixture().expect("hide row 6", |s| rows(s, "home.hideRows", "6"), [99.0, 61.6, 144.0, 43.2], HOME, HOME);
    let left = [51.0, 61.6, 144.0, 57.6];
    fixture().expect("hide column B", |s| cols(s, "home.hideColumns", "B"), left, left, HOME);
    fixture().expect("hide column D", |s| cols(s, "home.hideColumns", "D"), [99.0, 61.6, 96.0, 57.6], HOME, HOME);
    // Unhiding gives everything back, whichever placement.
    for (hide, show, r) in [("home.hideRows", "home.unhideRows", "2:2"), ("home.hideRows", "home.unhideRows", "5:8")] {
        fixture().expect(
            &format!("hide and unhide rows {r}"),
            |s| {
                rows(s, hide, r);
                rows(s, show, r);
            },
            HOME,
            HOME,
            HOME,
        );
    }
    fixture().expect(
        "hide and unhide row 9 at the far corner",
        |s| {
            rows(s, "home.hideRows", "9:9");
            rows(s, "home.unhideRows", "9:9");
        },
        HOME,
        HOME,
        HOME,
    );
    fixture().expect(
        "hide and unhide column F at the far corner",
        |s| {
            cols(s, "home.hideColumns", "F:F");
            cols(s, "home.unhideColumns", "F:F");
        },
        HOME,
        HOME,
        HOME,
    );
}

#[test]
/// Hiding the rows an object spans.
fn hiding_the_rows_an_object_spans() {
    // Excel measures a hidden top row from its edge, so the object keeps what is below it: 4 pt.
    // (It also draws the object's top at the row's edge, 57.6 pt; GridCraft keeps the 3 pt/4 pt
    // offset in the file and draws the top 4 pt lower.)
    let mut f = fixture();
    rows(&mut f.s, "home.hideRows", "5:8");
    for &(kind, id, mode) in &f.objects {
        let [_, _, _, h] = f.rect(kind, id);
        let want = if mode == AnchorMode::MoveAndSize { 4.0 } else { 57.6 };
        assert!((h - want).abs() < 0.01, "{} height {h}", mode_name(mode));
    }
    let mut f = fixture();
    rows(&mut f.s, "home.hideRows", "5");
    for &(kind, id, mode) in &f.objects {
        let [_, _, _, h] = f.rect(kind, id);
        let want = if mode == AnchorMode::MoveAndSize { 47.2 } else { 57.6 };
        assert!((h - want).abs() < 0.01, "{} height {h}", mode_name(mode));
    }
}

#[test]
fn hidden_far_corner_survives_json_round_trip() {
    let mut f = fixture();
    rows(&mut f.s, "home.hideRows", "9:9");
    let bytes = gridcraft_engine::io::save_bytes(&f.s.doc().unwrap().wb, "book.json").unwrap();
    let (wb, _) = gridcraft_engine::io::open_bytes("book.json", &bytes).unwrap();
    let mut s = Session::new();
    s.new_workbook();
    s.doc_mut().unwrap().wb = std::sync::Arc::new(wb);
    rows(&mut s, "home.unhideRows", "9:9");
    let sh = s.doc().unwrap().wb.sheet(0).unwrap();
    let anchor = sh.images.first().unwrap().anchor;
    assert!((anchor.height as f64 * PT - 57.6).abs() < 0.01);
}

#[test]
/// An applied filter hides rows like hiding them.
fn an_applied_filter_hides_rows_like_hiding_them() {
    // Rows 2 and 3 hold "x" and "y": keeping "y" hides row 2, above the object.
    let up = [99.0, 47.2, 144.0, 57.6];
    fixture().expect(
        "filter hides row 2",
        |s| {
            s.execute("data.filter", json!({"range": "A1:A3"})).unwrap();
            s.execute("data.filterBy", json!({"column": "A", "values": ["y"]})).unwrap();
            assert!(s.doc().unwrap().wb.sheet(0).unwrap().is_row_hidden(1));
        },
        up,
        up,
        HOME,
    );
}

#[test]
/// Undo puts objects back.
fn undo_puts_objects_back() {
    let mut f = fixture();
    let before: Vec<[f64; 4]> = f.objects.iter().map(|&(k, id, _)| f.rect(k, id)).collect();
    f.s.execute("home.rowHeight", json!({"rows": "2", "points": 60.0})).unwrap();
    f.s.execute("edit.undo", json!({})).unwrap();
    let after: Vec<[f64; 4]> = f.objects.iter().map(|&(k, id, _)| f.rect(k, id)).collect();
    assert_eq!(before, after);
}
