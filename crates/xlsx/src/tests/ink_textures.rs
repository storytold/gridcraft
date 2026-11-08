//! The `GridCraft Ink` veryHidden sheet: textures and per-stroke ink data Excel can't hold.

use std::sync::Arc;

use gridcraft_core::{CellRef, Value};
use gridcraft_model::ink::InkTexture;
use gridcraft_model::{Anchor, AnchorMode, Color, InkStyle, InkTool, Shape, ShapeKind, Sheet, Visibility, Workbook};

use crate::ink_sheet::{INK_SHEET_NAME, InkExtras, InkSheetData, build, is_ink_sheet, parse, unique_name};
use crate::{read_xlsx, write_xlsx};

fn texture(name: &str, bytes: Vec<u8>, w: u32, h: u32) -> InkTexture {
    InkTexture::new(name, "image/png", Arc::new(bytes), w, h).expect("a texture within the caps")
}

fn sample() -> InkSheetData {
    InkSheetData {
        textures: vec![texture("a.png", vec![1, 2, 3, 4, 5], 2, 1), texture("b.png", vec![9; 1000], 40, 25)],
        ink: [
            (
                "{11111111-2222-3333-4444-555555555555}".to_string(),
                InkExtras { texture: Some("ab".repeat(16)), scale: Some(0.5), tool: None, order: Some(3) },
            ),
            (
                "{AAAAAAAA-0000-0000-0000-000000000000}".to_string(),
                InkExtras { texture: None, scale: None, tool: Some("brushPen".into()), order: None },
            ),
        ]
        .into(),
    }
}

#[test]
fn round_trips_textures_and_extras() {
    let data = sample();
    let sheet = build(INK_SHEET_NAME, &data);
    assert_eq!(sheet.visibility, Visibility::VeryHidden);
    assert!(is_ink_sheet(&sheet));
    let back = parse(&sheet).expect("our own sheet reads back");
    assert_eq!(back, data);
}

#[test]
fn a_large_texture_spans_cells() {
    // 100 KiB of base64 is four chunks under the 30,000-character cell limit.
    let bytes: Vec<u8> = (0..100 * 1024).map(|i| (i % 251) as u8).collect();
    let data = InkSheetData { textures: vec![texture("big.png", bytes.clone(), 512, 512)], ink: Default::default() };
    let sheet = build(INK_SHEET_NAME, &data);
    let used: Vec<u32> = (6..40).filter(|c| sheet.cells.get(CellRef::new(2, *c)).is_some()).collect();
    assert!(used.len() > 1, "the payload must be split");
    assert!(used.iter().all(|c| sheet.cells.get(CellRef::new(2, *c)).is_some_and(|cell| cell.value.to_text().unwrap().len() <= 30_000)));
    let back = parse(&sheet).expect("reads back");
    assert_eq!(back.textures.len(), 1);
    assert_eq!(back.textures[0].data.as_ref(), &bytes);
}

#[test]
fn a_sheet_without_the_marker_is_not_ours() {
    let mut sheet = Sheet::new(INK_SHEET_NAME);
    sheet.visibility = Visibility::VeryHidden;
    assert!(!is_ink_sheet(&sheet));
    assert!(parse(&sheet).is_none());
    let ours = build(INK_SHEET_NAME, &sample());
    let mut visible = ours.clone();
    visible.visibility = Visibility::Visible;
    assert!(!is_ink_sheet(&visible), "only a veryHidden sheet is ours");
    let mut renamed = ours;
    renamed.name = "Notes".into();
    assert!(!is_ink_sheet(&renamed));
}

#[test]
fn an_unknown_version_is_not_read() {
    let mut sheet = build(INK_SHEET_NAME, &sample());
    sheet.set_value(CellRef::new(0, 1), Value::number(2.0));
    assert!(is_ink_sheet(&sheet));
    assert!(parse(&sheet).is_none(), "a newer layout must not be half-read");
}

#[test]
fn hostile_rows_are_skipped_not_fatal() {
    let mut sheet = build(INK_SHEET_NAME, &sample());
    let row = |r: u32| sheet.cells.get(CellRef::new(r, 0)).map(|c| c.value.to_text().unwrap_or_default());
    let mut r = 2;
    while row(r).is_some() {
        r += 1;
    }
    // A forged id, a texture over the pixel cap, junk base64, and a non-JSON extra.
    let rows: [(&str, String, &str, &str); 4] = [
        ("texture", "not-a-real-id".into(), "4", "2"),
        ("texture", "cd".repeat(16), "9000", "1"),
        ("texture", "ef".repeat(16), "4", "2"),
        ("ink", "{BAD}".into(), "", ""),
    ];
    for (i, (kind, key, a, b)) in rows.iter().enumerate() {
        sheet.set_value(CellRef::new(r + i as u32, 0), Value::text(*kind));
        sheet.set_value(CellRef::new(r + i as u32, 1), Value::text(key.as_str()));
        sheet.set_value(CellRef::new(r + i as u32, 2), Value::text("x.png"));
        sheet.set_value(CellRef::new(r + i as u32, 3), Value::text("image/png"));
        sheet.set_value(CellRef::new(r + i as u32, 4), Value::text(*a));
        sheet.set_value(CellRef::new(r + i as u32, 5), Value::text(*b));
        sheet.set_value(CellRef::new(r + i as u32, 6), Value::text("!!!!"));
    }
    let back = parse(&sheet).expect("the sheet still reads");
    assert_eq!(back.textures.len(), 2, "only the well-formed textures survive");
    assert_eq!(back.ink.len(), 2, "the malformed extra is skipped");
}

#[test]
fn extras_are_clamped_and_blank_ones_dropped() {
    let mut sheet = build(INK_SHEET_NAME, &sample());
    let key = "{22222222-2222-2222-2222-222222222222}";
    let row = 10;
    sheet.set_value(CellRef::new(row, 0), Value::text("ink"));
    sheet.set_value(CellRef::new(row, 1), Value::text(key));
    sheet.set_value(CellRef::new(row, 2), Value::text(r#"{"texture":"zz","scale":-3,"tool":""}"#));
    let back = parse(&sheet).expect("reads");
    assert!(!back.ink.contains_key(key), "an entry with nothing usable is dropped");
    let row = 11;
    sheet.set_value(CellRef::new(row, 0), Value::text("ink"));
    sheet.set_value(CellRef::new(row, 1), Value::text(key));
    sheet.set_value(CellRef::new(row, 2), Value::text(r#"{"texture":"abababababababababababababababab","scale":1000}"#));
    let back = parse(&sheet).expect("reads");
    let extras = back.ink.get(key).expect("kept");
    assert_eq!(extras.scale, Some(512.0));
    assert_eq!(extras.texture.as_deref(), Some("abababababababababababababababab"));
}

#[test]
fn unique_name_steps_around_a_sheet_of_the_same_name() {
    let mut wb = Workbook::new();
    assert_eq!(unique_name(&wb), INK_SHEET_NAME);
    wb.sheets.push(Arc::new(Sheet::new(INK_SHEET_NAME)));
    assert_eq!(unique_name(&wb), "GridCraft Ink 2");
    wb.sheets.push(Arc::new(Sheet::new("GridCraft Ink 2")));
    assert_eq!(unique_name(&wb), "GridCraft Ink 3");
}

// ---------------------------------------------------------------- end to end through xlsx

fn ink_shape(id: u32, key: &str) -> Shape {
    Shape {
        id,
        kind: ShapeKind::Ink,
        anchor: Anchor { cell: CellRef::new(0, 0), dx: 0.0, dy: 0.0, width: 120.0, height: 60.0, mode: AnchorMode::Absolute },
        fill: Color::Auto,
        line: Color::rgb(0x1F, 0x5F, 0xC9),
        text: "0,0;60,30;120,0".into(),
        format: Default::default(),
        math: Default::default(),
        ink: Some(InkStyle { key: Some(key.into()), texture: None, texture_scale: 1.0, ..InkStyle::default() }),
    }
}

fn with_shape(shape: Shape) -> Workbook {
    let mut wb = Workbook::new();
    Arc::make_mut(&mut wb.sheets[0]).shapes.push(shape);
    wb
}

fn sheet_names(bytes: &[u8]) -> Vec<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("a zip");
    let mut xml = String::new();
    std::io::Read::read_to_string(&mut z.by_name("xl/workbook.xml").expect("workbook.xml"), &mut xml).expect("reads");
    xml.split("<sheet name=\"").skip(1).filter_map(|s| s.split('"').next()).map(str::to_string).collect()
}

#[test]
fn a_workbook_without_extension_data_gains_no_ink_sheet() {
    let wb = with_shape(ink_shape(1, "{11111111-2222-3333-4444-555555555555}"));
    let bytes = write_xlsx(&wb).expect("writes");
    assert!(!sheet_names(&bytes).iter().any(|n| n.starts_with(INK_SHEET_NAME)));
    let (back, _) = read_xlsx(&bytes).expect("reads");
    assert_eq!(back.sheets.len(), 1);
    assert!(back.ink_textures.is_empty());
}

#[test]
fn textures_survive_a_save_and_reopen() {
    let mut wb = with_shape(ink_shape(1, "{11111111-2222-3333-4444-555555555555}"));
    let texture = texture("stripes.png", vec![7u8; 400], 10, 20);
    let id = texture.id.clone();
    wb.ink_textures.push(texture);
    let bytes = write_xlsx(&wb).expect("writes");
    let names = sheet_names(&bytes);
    assert!(names.iter().any(|n| n.starts_with(INK_SHEET_NAME)), "the hidden sheet is written");
    let (back, _) = read_xlsx(&bytes).expect("reads");
    assert_eq!(back.sheets.len(), 1, "the hidden sheet is not a model sheet");
    assert_eq!(back.ink_textures.len(), 1);
    assert_eq!(back.ink_textures[0].id, id);
    assert_eq!(back.ink_textures[0].name, "stripes.png");
    assert_eq!(back.ink_textures[0].data.as_ref(), &vec![7u8; 400]);
}

#[test]
fn a_stroke_texture_and_subtype_survive_a_save_and_reopen() {
    let key = "{11111111-2222-3333-4444-555555555555}";
    let texture = texture("a.png", vec![1, 2, 3, 4], 4, 4);
    let id = texture.id.clone();
    let mut wb = with_shape(ink_shape(1, key));
    wb.ink_textures.push(texture);
    {
        let shape = &mut Arc::make_mut(&mut wb.sheets[0]).shapes[0];
        let ink = shape.ink.as_mut().expect("ink");
        ink.texture = Some(id.clone());
        ink.texture_scale = 3.5;
        ink.tool = InkTool::FountainPen;
        ink.order = 7;
    }
    let bytes = write_xlsx(&wb).expect("writes");
    let (back, _) = read_xlsx(&bytes).expect("reads");
    let ink = back.sheets[0].shapes[0].ink.as_ref().expect("the stroke keeps its brush");
    assert_eq!(ink.key.as_deref(), Some(key));
    assert_eq!(ink.texture.as_deref(), Some(id.as_str()));
    assert_eq!(ink.texture_scale, 3.5);
    assert_eq!(ink.tool, InkTool::FountainPen, "the subtype Excel can't hold comes back");
    assert_eq!(ink.order, 7, "the draw order comes back for Ink Replay");
    assert_eq!(back.ink_textures.len(), 1);
}

#[test]
fn a_second_save_after_reading_keeps_the_extension() {
    let texture = texture("a.png", vec![9, 8, 7], 3, 3);
    let mut wb = with_shape(ink_shape(1, "{22222222-2222-3333-4444-555555555555}"));
    let id = texture.id.clone();
    wb.ink_textures.push(texture);
    Arc::make_mut(&mut wb.sheets[0]).shapes[0].ink.as_mut().expect("ink").texture = Some(id.clone());
    let first = write_xlsx(&wb).expect("writes");
    let (back, _) = read_xlsx(&first).expect("reads");
    assert_eq!(back.sheets[0].shapes[0].ink.as_ref().and_then(|i| i.texture.as_deref()), Some(id.as_str()));
    // A save of the reopened workbook writes the same channel again (nothing accumulated).
    let second = write_xlsx(&back).expect("writes");
    let (again, _) = read_xlsx(&second).expect("reads");
    assert_eq!(again.sheets.len(), 1);
    assert_eq!(again.ink_textures.len(), 1);
    assert_eq!(again.sheets[0].shapes[0].ink.as_ref().and_then(|i| i.texture.as_deref()), Some(id.as_str()));
}

#[test]
fn an_unreadable_ink_sheet_is_kept_so_a_save_does_not_delete_it() {
    // A newer GridCraft writes a layout version this one does not read. Dropping the sheet on
    // open would make the next save delete every texture and per-stroke extra it holds, so the
    // sheet stays an ordinary (veryHidden) model sheet and round-trips until the version that
    // understands it opens the file again.
    let mut wb = with_shape(ink_shape(1, "{33333333-2222-3333-4444-555555555555}"));
    let mut newer = build(INK_SHEET_NAME, &sample());
    newer.set_value(CellRef::new(0, 1), Value::number(2.0));
    wb.sheets.push(Arc::new(newer));
    let bytes = write_xlsx(&wb).expect("writes");
    let (back, report) = read_xlsx(&bytes).expect("reads");
    assert!(report.warnings.iter().any(|w| w.contains("unknown layout")), "the reader warns: {:?}", report.warnings);
    let kept = back.sheets.iter().find(|s| s.name.starts_with(INK_SHEET_NAME)).expect("the unreadable sheet is kept");
    assert_eq!(kept.visibility, Visibility::VeryHidden);
    assert_eq!(kept.cells.get(CellRef::new(0, 1)).and_then(|c| c.value.as_f64()), Some(2.0), "its cells come through the ordinary sheet reader");
    assert!(back.ink_textures.is_empty(), "nothing is read out of a layout this version does not know");
    // The next save writes it back: the sheet (and its cells) survives a second open/save cycle.
    let second = write_xlsx(&back).expect("writes");
    let (again, _) = read_xlsx(&second).expect("reads");
    let kept = again.sheets.iter().find(|s| s.name.starts_with(INK_SHEET_NAME)).expect("still there after the second save");
    assert_eq!(kept.cells.get(CellRef::new(0, 1)).and_then(|c| c.value.as_f64()), Some(2.0));
}

#[test]
fn a_stroke_without_a_key_keeps_its_texture_by_its_emitted_id() {
    // The drawing writer mints a deterministic creation id when the stroke's key is missing or
    // malformed; the hidden sheet must key its extras by that same id, or the texture is lost.
    let texture = texture("a.png", vec![1, 2, 3], 2, 2);
    let id = texture.id.clone();
    let mut wb = with_shape(ink_shape(1, "{11111111-2222-3333-4444-555555555555}"));
    wb.ink_textures.push(texture);
    {
        let shape = &mut Arc::make_mut(&mut wb.sheets[0]).shapes[0];
        let ink = shape.ink.as_mut().expect("ink");
        ink.key = None;
        ink.texture = Some(id.clone());
        ink.texture_scale = 2.5;
    }
    let bytes = write_xlsx(&wb).expect("writes");
    let (back, _) = read_xlsx(&bytes).expect("reads");
    let ink = back.sheets[0].shapes[0].ink.as_ref().expect("the stroke keeps its brush");
    assert_eq!(ink.key.as_deref(), Some("{00000001-0000-4000-8000-000000000001}"), "the writer mints the id from the shape id");
    assert_eq!(ink.texture.as_deref(), Some(id.as_str()), "the texture association uses that same id");
    assert_eq!(ink.texture_scale, 2.5);
}
