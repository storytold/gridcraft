//! A line's or arrow's direction is its `a:xfrm` mirroring (`flipH`, `flipV`), kept on save and
//! reopen (#2904).

use std::io::Read;
use std::sync::Arc;

use gridcraft_model::{Anchor, Color, Shape, ShapeKind, Sheet, Workbook};

use crate::{read_xlsx, write_xlsx};

fn line(kind: ShapeKind, flip_h: bool, flip_v: bool) -> Workbook {
    let mut sheet = Sheet::new("Sheet1");
    sheet.shapes.push(Shape {
        id: 1,
        kind,
        anchor: Anchor { width: 120.0, height: 40.0, ..Default::default() },
        fill: Color::Auto,
        line: Color::Rgb(0x000000),
        text: String::new(),
        format: Default::default(),
        flip_h,
        flip_v,
    });
    Workbook { sheets: vec![Arc::new(sheet)], ..Default::default() }
}

fn drawing_xml(bytes: &[u8]) -> String {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut drawing = String::new();
    archive.by_name("xl/drawings/drawing1.xml").unwrap().read_to_string(&mut drawing).unwrap();
    drawing
}

#[test]
fn line_and_arrow_flips_survive_save_and_reopen() {
    for kind in [ShapeKind::Line, ShapeKind::Arrow] {
        for (flip_h, flip_v) in [(false, false), (true, false), (false, true), (true, true)] {
            let bytes = write_xlsx(&line(kind, flip_h, flip_v)).unwrap();
            let xml = drawing_xml(&bytes);
            assert_eq!(xml.contains(r#"flipH="1""#), flip_h, "{kind:?} {flip_h}/{flip_v}: {xml}");
            assert_eq!(xml.contains(r#"flipV="1""#), flip_v, "{kind:?} {flip_h}/{flip_v}: {xml}");
            let (back, _) = read_xlsx(&bytes).unwrap();
            let s = &back.sheet(0).unwrap().shapes[0];
            assert_eq!((s.kind, s.flip_h, s.flip_v), (kind, flip_h, flip_v));
            // The arrowhead stays on the end (`a:tailEnd`), which the flips place.
            if kind == ShapeKind::Arrow {
                assert!(xml.contains(r#"<a:tailEnd type="triangle"/>"#), "{xml}");
                assert_eq!(s.line_ends().1, [if flip_h { 0.0 } else { s.anchor.width }, if flip_v { 0.0 } else { s.anchor.height }]);
            }
        }
    }
}
